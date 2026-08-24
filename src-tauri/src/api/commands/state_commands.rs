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

/// 读取学习状态
///
/// 读取 `state/current.state` (TOML) 并解析为 StudyState。
/// 前端调用: `invoke('get_state')`
#[tauri::command]
pub async fn get_state(state: State<'_, Mutex<AppState>>) -> Result<StudyState, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::state::read_state(&data_dir)
}

/// 更新任务状态
///
/// 根据任务 ID（格式 `YYYY-MM-DD-NN`）更新 State 中的任务状态。
/// `status` 参数为: "pending" | "in_progress" | "done" | "abandoned"
/// 当任务标记为 done 时，自动更新科目的 completed 列表和 global progress。
/// 前端调用: `invoke('update_task_status', { taskId, status })`
#[tauri::command]
pub async fn update_task_status(
    task_id: String,
    status: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 的读-改-写，避免并发命令丢失更新
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    // 解析任务 ID: YYYY-MM-DD-NN
    let (date, task_index) = parse_task_id(&task_id)?;

    // H3：历史数据不可修改约束——仅允许修改今天和昨天的任务，更早日期拒绝
    // （前端 TodayView 已有相同限制，此处为后端防御层，防止绕过 UI 的调用）
    let today = crate::data::today_string();
    let yesterday = crate::data::add_days(&today, -1).unwrap_or_else(|_| today.clone());
    if date != today && date != yesterday {
        return Err(format!(
            "历史任务不可修改：仅支持修改今天（{}）或昨天（{}）的任务，当前任务日期为 {}",
            today, yesterday, date
        ));
    }

    // 解析新状态
    let new_status = parse_task_status(&status)?;

    // 读取当前 State
    let mut study_state = crate::data::state::read_state(&data_dir)?;

    // 确认日期匹配
    // 若 state.current_task.date 与 task_id 解析出的 date 不一致，说明 state 可能被污染或跨日未重置
    // 此时不能仅改 date（会保留错位的 task_id 与任务内容），需整体清空 current_task
    if study_state.current_task.date != date {
        log::warn!(
            "任务日期不匹配: state.date={}, task_id.date={}。重置 current_task 以避免状态污染",
            study_state.current_task.date,
            date
        );
        study_state.current_task.date = date.clone();
        // 清空错位任务：只保留 task_id 日期前缀与 date 一致的任务，其余丢弃
        study_state.current_task.tasks.retain(|t| match &t.task_id {
            Some(id) => crate::data::task_id_date_prefix(id)
                .map(|prefix| prefix == date)
                .unwrap_or(false),
            None => false, // 旧任务无 task_id 且日期不匹配，丢弃
        });
    }

    // 找到对应任务的 subject（用于更新科目进度）
    let task_subject = study_state
        .current_task
        .tasks
        .iter()
        .find(|t| t.task_id.as_deref() == Some(&task_id))
        .or_else(|| study_state.current_task.tasks.get(task_index))
        .map(|t| t.subject.clone());

    // 更新任务状态
    crate::data::state::update_task_status_by_id(
        &mut study_state,
        &task_id,
        task_index,
        new_status.clone(),
    )?;

    // 标记为 done 时，自动更新科目进度
    if new_status == crate::data::state::TaskStatus::Done {
        if let Some(ref subject_key) = task_subject {
            // 找到任务的标题
            let task_title = study_state
                .current_task
                .tasks
                .iter()
                .find(|t| t.task_id.as_deref() == Some(&task_id))
                .or_else(|| study_state.current_task.tasks.get(task_index))
                .map(|t| t.task.clone())
                .unwrap_or_default();

            // 更新科目的 completed 列表（去重）
            if !task_title.is_empty() {
                if let Some(subj_state) =
                    crate::data::state::get_subject_state_mut(&mut study_state, subject_key)
                {
                    if !subj_state.completed.contains(&task_title) {
                        subj_state.completed.push(task_title);
                    }
                }
            }
        }

        // 更新 global progress
        let today = crate::data::today_string();
        let progress = &mut study_state.progress;

        // 学习天数与连续天数：仅当「当天首个任务」完成时才 +1 / 更新 streak，
        // 避免同一天完成多个任务导致 total_study_days 被重复累加（C9）
        if progress.last_study_date == date {
            // 同一天，不重复计算
        } else {
            progress.total_study_days += 1;

            if !progress.last_study_date.is_empty() {
                let days_diff =
                    crate::data::days_between(&date, &progress.last_study_date).unwrap_or(0);
                if days_diff == 1 {
                    progress.streak_days += 1;
                } else {
                    progress.streak_days = 1;
                }
            } else {
                progress.streak_days = 1;
            }
        }
        progress.last_study_date = today;
    }

    // 保存 State
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!("任务状态已更新: {} -> {:?}", task_id, new_status);
    Ok(())
}
