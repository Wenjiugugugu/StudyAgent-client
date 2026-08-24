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

/// 列出所有 MCP 服务器状态
///
/// 返回已连接和已配置但未连接的 MCP Server 状态列表。
/// 前端调用: `invoke('list_mcp_servers')`
#[tauri::command]
pub async fn list_mcp_servers(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MCPServerStatus>, String> {
    let dispatcher = get_tool_dispatcher(state.inner())?;
    Ok(dispatcher.list_servers())
}

/// 调用 MCP 工具
///
/// 统一工具调用入口，路由到对应的 MCP Server。
/// 如果工具名以 `builtin.` 开头，执行内置工具。
/// 前端调用:
/// ```typescript
/// invoke('call_tool', {
///   toolName: 'dida365_create_task',
///   args: { title: '数学复习', ... }
/// });
/// ```
#[tauri::command]
pub async fn call_tool(
    tool_name: String,
    args: Value,
    state: State<'_, Mutex<AppState>>,
) -> Result<ToolCallResult, String> {
    let (data_dir, dispatcher) = get_data_dir_and_dispatcher(state.inner())?;

    // 检查是否是内置工具
    if is_builtin_tool(&tool_name) {
        return execute_builtin_tool(&tool_name, &args, &data_dir);
    }

    // H16：工具参数可能包含敏感数据（如任务标题、token），降为 debug 级别并截断
    let args_str = args.to_string();
    log::debug!(
        "调用工具: {} args={}",
        tool_name,
        args_str.chars().take(200).collect::<String>()
    );
    dispatcher.dispatch(&tool_name, args).await
}
