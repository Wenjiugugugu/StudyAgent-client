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

/// 开始任务计时
///
/// 为指定 task_id 的任务设置 started_at（当前时间）。
/// 仅在启用 enable_time_tracking 设置时有效；否则返回错误提示用户开启设置。
///
/// 前端调用: `invoke('start_task_timer', { taskId })`
#[tauri::command]
pub async fn start_task_timer(
    task_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    // 检查是否启用计时功能
    let settings = crate::load_settings(&data_dir);
    if !settings.enable_time_tracking() {
        return Err("计时功能未启用，请在设置中开启「记录学习时长」".to_string());
    }

    let mut study_state = crate::data::state::read_state(&data_dir)?;
    crate::data::state::start_task_timer(&mut study_state, &task_id)?;
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!("任务计时开始: {}", task_id);
    Ok(())
}

/// 暂停任务计时
///
/// 计算 started_at 到现在的分钟差，累加到 accumulated_minutes，清空 started_at。
/// 返回本次新增的计时分钟数。
///
/// 前端调用: `invoke('pause_task_timer', { taskId })`
#[tauri::command]
pub async fn pause_task_timer(
    task_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<i64, String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    // 检查是否启用计时功能
    let settings = crate::load_settings(&data_dir);
    if !settings.enable_time_tracking() {
        return Err("计时功能未启用，请在设置中开启「记录学习时长」".to_string());
    }

    let mut study_state = crate::data::state::read_state(&data_dir)?;
    let added_minutes = crate::data::state::pause_task_timer(&mut study_state, &task_id)?;
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!("任务计时暂停: {}, 本次新增 {} 分钟", task_id, added_minutes);
    Ok(added_minutes)
}

/// 获取任务累计专注分钟数（含正在进行的时段）
///
/// 前端调用: `invoke('get_task_total_minutes', { taskId })`
#[tauri::command]
pub async fn get_task_total_minutes(
    task_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<i64, String> {
    let data_dir = get_data_dir(state.inner())?;
    let study_state = crate::data::state::read_state(&data_dir)?;
    crate::data::state::task_total_minutes(&study_state, &task_id)
}

/// 番茄钟：把完成的学习会话分钟数累加到关联任务
///
/// 前端在「专注」页完成一个学习番茄后调用，把实际专注分钟数写入
/// 关联任务的 accumulated_minutes，使今日计划/复盘自动累计实际用时。
/// 不依赖「记录学习时长」设置（番茄钟本身是显式计时）。
///
/// 前端调用: `invoke('focus_add_minutes', { taskId, minutes })`
#[tauri::command]
pub async fn focus_add_minutes(
    task_id: String,
    minutes: i64,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // M2：校验专注分钟数范围，防止负值/超大值污染 accumulated_minutes
    if !(0..=1440).contains(&minutes) {
        return Err(format!("无效的专注分钟数: {}（允许 0-1440）", minutes));
    }

    // H1 并发保护：串行化 state 写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut study_state = crate::data::state::read_state(&data_dir)?;
    crate::data::state::add_accumulated_minutes(&mut study_state, &task_id, minutes)?;
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!("番茄钟：任务 {} 累加专注 {} 分钟", task_id, minutes);
    Ok(())
}

/// 番茄钟：记录一条专注会话（学习/休息/长休息）
///
/// 会话按结束日期落盘到 `focus/YYYY-MM-DD_focus.json`。
/// 前端调用: `invoke('record_focus_session', { session })`
#[tauri::command]
pub async fn record_focus_session(
    session: crate::data::focus::FocusSession,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：append_focus_session 是读-改-写，需与其他写命令串行化
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    crate::data::focus::append_focus_session(&data_dir, session)
        .map_err(|e| format!("记录专注会话失败: {}", e))?;
    Ok(())
}

/// 番茄钟：为某条专注会话手动绑定任务
///
/// 在专注记录里把未关联的学习番茄 / 正计时补充归属到今日任务：
/// 写入会话的 task_id，并把该段专注分钟累加到对应任务计时。
/// 前端调用: `invoke('link_focus_session', { sessionId, taskId, date })`
#[tauri::command]
pub async fn link_focus_session(
    session_id: String,
    task_id: String,
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;

    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let minutes = crate::data::focus::link_focus_session(&data_dir, &date, &session_id, &task_id)?;

    // 绑定后把该段沉浸分钟累加到关联任务，使其计入具体任务的学习时长；
    // 该会话此后带 task_id，不再被「未关联专注」口径重复统计。
    let mut study_state = crate::data::state::read_state(&data_dir)?;
    crate::data::state::add_accumulated_minutes(&mut study_state, &task_id, minutes)?;
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!(
        "番茄钟：会话 {} 关联任务 {}，累加 {} 分钟",
        session_id,
        task_id,
        minutes
    );
    Ok(())
}

/// 番茄钟：读取某天的专注会话列表
///
/// 前端调用: `invoke('get_focus_sessions', { date })`
#[tauri::command]
pub async fn get_focus_sessions(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::data::focus::FocusSession>, String> {
    let data_dir = get_data_dir(state.inner())?;
    // M1：与其他 date 参数命令保持一致，先校验 YYYY-MM-DD 格式，防止路径穿越
    crate::data::validate_date(&date)?;
    crate::data::focus::list_focus_sessions(&data_dir, &date)
        .map_err(|e| format!("读取专注记录失败: {}", e))
}

/// 番茄钟：读取 [start, end] 日期区间内的全部专注会话
///
/// 前端调用: `invoke('get_focus_sessions_range', { start, end })`
#[tauri::command]
pub async fn get_focus_sessions_range(
    start: String,
    end: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::data::focus::FocusSession>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::focus::list_focus_sessions_in_range(&data_dir, &start, &end)
        .map_err(|e| format!("读取专注记录失败: {}", e))
}

/// 番茄钟：今日统计（番茄数 / 专注分钟 / 休息次数）
///
/// 前端调用: `invoke('get_focus_today_stats')`
#[tauri::command]
pub async fn get_focus_today_stats(
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::data::focus::FocusDayStats, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::focus::focus_today_stats(&data_dir)
        .map_err(|e| format!("读取今日专注统计失败: {}", e))
}
