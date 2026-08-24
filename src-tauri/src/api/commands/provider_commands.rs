#![allow(unused_imports)]
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::analytics::{build_analytics, AnalyticsRange, AnalyticsSummary};
use crate::core::briefing::{yesterday_of, BriefingAgent};
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{UserCapability, UserObservation};
use crate::data::plan::{
    iso_week_string, DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment,
};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::tools::dispatcher::{execute_builtin_tool, is_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    get_ai_service, get_data_dir, get_data_dir_and_ai, get_data_dir_and_dispatcher,
    get_tool_dispatcher, load_settings, reinitialize_services, save_settings_file, AppSettings,
    AppState,
};

use super::legacy::*;

/// 获取用户能力列表
///
/// 前端调用: `invoke('get_capabilities')`
#[tauri::command]
pub async fn get_capabilities(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<UserCapability>, String> {
    let data_dir = get_data_dir(state.inner())?;
    UserModelService::get_capabilities(&data_dir)
}

/// 获取用户观察列表
///
/// 前端调用: `invoke('get_observations')`
#[tauri::command]
pub async fn get_observations(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<UserObservation>, String> {
    let data_dir = get_data_dir(state.inner())?;
    UserModelService::get_observations(&data_dir)
}

/// 获取用户画像摘要
///
/// 返回用于 AI prompt 注入的摘要文本。
/// 前端调用: `invoke('get_user_model_summary')`
#[tauri::command]
pub async fn get_user_model_summary(state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    let data_dir = get_data_dir(state.inner())?;
    UserModelService::get_user_model_summary(&data_dir)
}

/// AI 对话（非流式）
///
/// 发送聊天请求到 AI Provider，获取完整响应。
/// 前端调用: `invoke('chat', { request: { messages, agent, ... } })`
#[tauri::command]
pub async fn chat(
    request: ChatRequest,
    state: State<'_, Mutex<AppState>>,
) -> Result<ChatResponse, String> {
    let ai_service = get_ai_service(state.inner())?;

    if !ai_service.has_provider() {
        return Err("未配置任何 AI Provider，请在设置中添加 AI Provider 配置".to_string());
    }

    ai_service.chat(request).await
}

/// AI 对话（流式）
///
/// 通过事件推送流式响应块。前端通过 `listen` 监听 `on_event` 事件名。
/// 命令完成后返回完整的 ChatResponse。
/// 前端调用:
/// ```typescript
/// listen('chat-stream-chunk', (event) => {
///   console.log(event.payload); // ChatStreamChunk
/// });
/// invoke('chat_stream', { request, onEvent: 'chat-stream-chunk' });
/// ```
#[tauri::command]
pub async fn chat_stream(
    request: ChatRequest,
    on_event: String,
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<ChatResponse, String> {
    let ai_service = get_ai_service(state.inner())?;

    if !ai_service.has_provider() {
        return Err("未配置任何 AI Provider，请在设置中添加 AI Provider 配置".to_string());
    }

    // 克隆 AppHandle 用于在回调中发送事件
    let app_handle = app.clone();
    let event_name = on_event.clone();

    // 调用流式 API
    let response = ai_service
        .chat_stream(request, move |chunk| {
            // 发送流式 chunk 到前端
            if let Err(e) = app_handle.emit(&event_name, chunk) {
                log::warn!("发送流式事件失败: {}", e);
            }
        })
        .await?;

    // 发送完成事件
    let _ = app.emit(
        &format!("{}-done", on_event),
        &serde_json::json!({
            "id": response.id,
            "model": response.model,
        }),
    );

    Ok(response)
}

/// 取消指定 agent 的进行中 AI 请求
///
/// key 为 agent 类型小写（planner / reviewer / briefing / teacher / assistant）。
/// 返回是否找到了对应请求并已发送取消信号。
/// 前端调用: `invoke('cancel_ai_request', { key: 'planner' })`
#[tauri::command]
pub fn cancel_ai_request(key: String, state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let ai_service = get_ai_service(state.inner())?;
    Ok(ai_service.cancel_request(&key))
}

/// 测试 AI Provider 连接
///
/// 临时创建 Provider 实例并发送测试请求。
/// 不需要 AppState，直接用传入的配置测试。
/// 前端调用: `invoke('test_ai_provider', { config: { ... } })`
#[tauri::command]
pub async fn test_ai_provider(mut config: AIProviderConfig) -> Result<TestResult, String> {
    if config.api_key == crate::secrets::CONFIGURED_SENTINEL {
        config.api_key = crate::secrets::get_provider_api_key(&config.id)?
            .ok_or_else(|| "系统凭据库中未找到该 Provider 的 API Key".to_string())?;
    }
    match AiService::test_provider(config).await {
        Ok(msg) => Ok(TestResult {
            success: true,
            message: msg,
        }),
        Err(e) => Ok(TestResult {
            success: false,
            message: e,
        }),
    }
}

/// 获取 AI Provider 可用模型列表
///
/// 如果提供 `config`，临时测试该配置获取模型；否则从默认 Provider 获取。
/// 前端调用: `invoke('list_ai_models', { config: null })`
#[tauri::command]
pub async fn list_ai_models(
    config: Option<AIProviderConfig>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::ai::provider::ModelInfo>, String> {
    match config {
        Some(mut cfg) => {
            if cfg.api_key == crate::secrets::CONFIGURED_SENTINEL {
                cfg.api_key = crate::secrets::get_provider_api_key(&cfg.id)?
                    .ok_or_else(|| "系统凭据库中未找到该 Provider 的 API Key".to_string())?;
            }
            AiService::test_list_models(cfg).await
        }
        None => {
            let ai_service = get_ai_service(state.inner())?;
            ai_service.list_models().await
        }
    }
}

/// 读取 AI 用量日志（持久化记录，重启后不丢失）
///
/// 返回所有历史 AI 调用的 token 消耗记录，按时间升序。
/// 前端调用: `invoke('get_ai_usage_log')`
#[tauri::command]
pub async fn get_ai_usage_log() -> Result<Vec<crate::data::ai_usage::AiUsageEntry>, String> {
    Ok(crate::data::ai_usage::read_all())
}

/// 清空 AI 用量日志
///
/// 前端调用: `invoke('clear_ai_usage_log')`
#[tauri::command]
pub async fn clear_ai_usage_log(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    // H1 并发保护：与 AI 调用后的用量日志 append 串行化，避免清空与追加竞态
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    crate::data::ai_usage::clear();
    Ok(())
}
