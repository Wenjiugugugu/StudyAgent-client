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

/// 获取关闭窗口时的动作设置
///
/// 返回值: "ask" | "tray" | "quit"
/// 前端调用: `invoke('get_close_action')`
#[tauri::command]
pub async fn get_close_action(state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    let data_dir = get_data_dir(state.inner())?;
    let settings = load_settings(&data_dir);
    Ok(settings.close_action)
}

/// 设置关闭窗口时的动作
///
/// action: "ask" | "tray" | "quit"
/// 前端调用: `invoke('set_close_action', { action: 'tray' })`
#[tauri::command]
pub async fn set_close_action(
    action: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let normalized = match action.as_str() {
        "ask" | "tray" | "quit" => action,
        _ => {
            return Err(format!(
                "无效的关闭动作: {}（支持: ask, tray, quit）",
                action
            ))
        }
    };
    let data_dir = get_data_dir(state.inner())?;
    // M15：settings 写操作与其他写命令串行化，避免与 save_settings 并发丢更新
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    let mut settings = load_settings(&data_dir);
    settings.close_action = normalized.clone();
    save_settings_file(&data_dir, &settings)?;
    log::info!("关闭动作已更新为: {}", normalized);
    Ok(())
}

/// 立即退出整个应用进程（包括销毁托盘图标）
///
/// 用于前端「关闭窗口询问弹窗」中选择"退出应用"时调用。
/// 不能仅调用 `window.destroy()`：存在 tray icon 时，销毁窗口后进程仍会驻留。
///
/// 前端调用: `invoke('quit_app')`
#[tauri::command]
pub async fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("收到 quit_app 命令，退出整个应用进程");
    app.exit(0);
    Ok(())
}

/// 查询开机启动是否启用
///
/// 前端调用: `invoke('get_autostart')`
#[tauri::command]
pub async fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    match manager.is_enabled() {
        Ok(enabled) => Ok(enabled),
        Err(e) => {
            log::warn!("查询开机启动状态失败: {}", e);
            Ok(false)
        }
    }
}

/// 启用或禁用开机启动
///
/// 前端调用: `invoke('set_autostart', { enabled: true })`
#[tauri::command]
pub async fn set_autostart(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("启用开机启动失败: {}", e))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("禁用开机启动失败: {}", e))?;
    }
    log::info!("开机启动已{}", if enabled { "启用" } else { "禁用" });
    Ok(())
}

/// 获取应用版本号（来自 tauri.conf.json）
///
/// 前端调用: `invoke('get_app_version')`
#[tauri::command]
pub async fn get_app_version(app: tauri::AppHandle) -> Result<String, String> {
    Ok(app.package_info().version.to_string())
}
