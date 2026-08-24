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

/// 读取应用日志文件内容（`logs/ai-debug.log`）
///
/// 返回日志的原始文本。为避免一次性加载超大文件，仅返回末尾 `max_chars` 字符
/// （默认 200_000，约 200KB）。文件不存在或为空时返回空字符串。
///
/// 前端调用: `invoke('read_app_log', { maxChars })`
#[tauri::command]
pub async fn read_app_log(
    max_chars: Option<usize>,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let data_dir = get_data_dir(state.inner())?;
    let log_path = crate::data::ai_debug_log_path(&data_dir);
    if !log_path.exists() {
        return Ok(String::new());
    }
    let content = crate::data::read_file_content(&log_path)
        .map_err(|e| format!("读取日志文件失败: {}", e))?;
    let max = max_chars.unwrap_or(200_000);
    // 取末尾 max 字符，且尽量从字符边界截断
    if content.chars().count() <= max {
        Ok(content)
    } else {
        let start = content.floor_char_boundary(content.len() - max);
        Ok(content[start..].to_string())
    }
}

/// 清空应用日志文件（`logs/ai-debug.log`）
///
/// 前端调用: `invoke('clear_app_log')`
#[tauri::command]
pub async fn clear_app_log(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    // H1 并发保护：与日志写入串行化，避免清空与追加竞态
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    let data_dir = get_data_dir(state.inner())?;
    let log_path = crate::data::ai_debug_log_path(&data_dir);
    if log_path.exists() {
        std::fs::write(&log_path, "")
            .map_err(|e| format!("清空日志文件失败 {:?}: {}", log_path, e))?;
    }
    Ok(())
}

/// 调试：列出数据目录下某相对路径的条目（目录不存在时返回空列表）
///
/// 前端调用: `invoke('debug_list_dir', { relativePath })`
#[tauri::command]
pub async fn debug_list_dir(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<DebugDirEntry>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let dir_path = resolve_debug_path(&data_dir, &relative_path)?;
    if !dir_path.exists() || !dir_path.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in
        std::fs::read_dir(&dir_path).map_err(|e| format!("读取目录失败 {:?}: {}", dir_path, e))?
    {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
        let file_type = entry.file_type().map_err(|e| {
            format!(
                "读取条目类型失败 {}: {}",
                entry.file_name().to_string_lossy(),
                e
            )
        })?;
        entries.push(DebugDirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_directory: file_type.is_dir(),
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// 调试：读取数据目录下某相对路径的文件文本内容
///
/// 前端调用: `invoke('debug_read_file', { relativePath })`
#[tauri::command]
pub async fn debug_read_file(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let data_dir = get_data_dir(state.inner())?;
    let file_path = resolve_debug_path(&data_dir, &relative_path)?;
    if !file_path.is_file() {
        return Err(format!("文件不存在: {}", relative_path));
    }
    crate::data::read_file_content(&file_path).map_err(|e| format!("读取文件失败: {}", e))
}
