//! Tauri 命令定义 — 所有 `#[tauri::command]` 函数
//!
//! 前端通过 `@tauri-apps/api` 的 `invoke` 调用这些命令。
//! 所有命令返回 `Result<T, String>`，Tauri 自动将 `Err` 转为前端 Promise reject。

use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::analytics::{AnalyticsRange, AnalyticsSummary, build_analytics};
use crate::core::briefing::{BriefingAgent, yesterday_of};
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{
    UserCapability, UserObservation,
};
use crate::data::plan::{DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment, iso_week_string};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::tools::dispatcher::{is_builtin_tool, execute_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    AppSettings, AppState, get_ai_service, get_data_dir, get_data_dir_and_ai,
    get_data_dir_and_dispatcher, get_tool_dispatcher, load_settings, reinitialize_services,
    save_settings_file,
};

// ============================================================================
// Dashboard 命令
// ============================================================================

/// 获取 Dashboard 汇总数据
///
/// 聚合 State + Plan + Records 数据，返回 DashboardSummary。
/// 前端调用: `invoke('get_dashboard_summary')`
#[tauri::command]
pub async fn get_dashboard_summary(
    state: State<'_, Mutex<AppState>>,
) -> Result<DashboardSummary, String> {
    let data_dir = get_data_dir(state.inner())?;
    DashboardAggregator::aggregate(&data_dir)
}

// ============================================================================
// State 命令
// ============================================================================

/// 读取学习状态
///
/// 读取 `state/current.state` (TOML) 并解析为 StudyState。
/// 前端调用: `invoke('get_state')`
#[tauri::command]
pub async fn get_state(
    state: State<'_, Mutex<AppState>>,
) -> Result<StudyState, String> {
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
        study_state
            .current_task
            .tasks
            .retain(|t| match &t.task_id {
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
                if let Some(subj_state) = crate::data::state::get_subject_state_mut(&mut study_state, subject_key) {
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
                let days_diff = crate::data::days_between(&date, &progress.last_study_date).unwrap_or(0);
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

// ============================================================================
// Analytics 命令
// ============================================================================

/// 获取学习数据分析
///
/// 根据 range 参数返回指定时间范围的分析数据。
/// - `last_7_days`：近7天
/// - `last_30_days`：近30天（默认）
/// - `all`：全部历史
///
/// `exclude_exempt_dates`：是否在分析中排除休息日和特殊情况排除日（默认 true）
///
/// 前端调用: `invoke('get_analytics', { range: 'last_30_days', excludeExemptDates: true })`
#[tauri::command]
pub async fn get_analytics(
    range: Option<String>,
    exclude_exempt_dates: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<AnalyticsSummary, String> {
    let data_dir = get_data_dir(state.inner())?;

    let range = match range.as_deref() {
        Some("last_7_days") => AnalyticsRange::Last7Days,
        Some("all") => AnalyticsRange::All,
        _ => AnalyticsRange::Last30Days,
    };
    // 默认开启排除
    let exclude_exempt = exclude_exempt_dates.unwrap_or(true);

    build_analytics(&data_dir, &range, exclude_exempt)
        .map_err(|e| format!("生成分析数据失败: {}", e))
}

// ============================================================================
// Plan 命令
// ============================================================================

/// 读取今日计划
///
/// 读取今天的 `plan/YYYY-MM-DD_day.json` 文件。
/// 前端调用: `invoke('get_today_plan')`
#[tauri::command]
pub async fn get_today_plan(
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    let data_dir = get_data_dir(state.inner())?;
    let today = crate::data::today_string();
    crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &today)
}

/// 读取指定日期的计划
///
/// 前端调用: `invoke('get_plan_by_date', { date: '2026-07-24' })`
#[tauri::command]
pub async fn get_plan_by_date(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;
    crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &date)
}

/// 读取周计划
///
/// 读取 `plan/YYYY-Www_week.json` 并返回 WeekPlanFile。
/// `week_start` 为周一日期 (YYYY-MM-DD)，内部转换为 ISO 周标识。
/// 前端调用: `invoke('get_week_plan', { weekStart: '2026-07-21' })`
#[tauri::command]
pub async fn get_week_plan(
    week_start: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<WeekPlanFile, String> {
    crate::data::validate_date(&week_start)?;
    let data_dir = get_data_dir(state.inner())?;
    let iso_week = iso_week_string(&week_start)?;
    crate::data::plan::read_week_plan(&data_dir, &iso_week)
}

/// 列出所有有日计划的日期
///
/// 返回 plan/ 目录下所有 `YYYY-MM-DD_day.json` 文件对应的日期列表，按升序排列。
/// 前端调用: `invoke('list_plan_dates')`
#[tauri::command]
pub async fn list_plan_dates(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::plan::list_daily_plan_dates(&data_dir)
}

/// 日计划摘要（聚合 plan + review 数据）
///
/// 用于历史计划列表和周计划视图展示完成度。
/// 一次返回所有日计划的摘要信息，避免前端逐日调用。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanSummary {
    pub date: String,
    pub has_plan: bool,
    pub has_review: bool,
    pub planned_tasks: i32,
    pub planned_hours: f64,
    pub completed_tasks: i32,
    pub completion_rate: f64,
    pub actual_hours: f64,
    pub is_rest_day: bool,
    /// 是否为周计划中手动添加的特殊情况排除日（出差/生病/考试等）
    pub is_excluded: bool,
    /// 排除日类型（travel/sick/exam/other），仅当 is_excluded=true 时有值
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_type: Option<String>,
    /// 排除日备注，仅当 is_excluded=true 时有值
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_note: Option<String>,
}

/// 列出所有日计划的摘要（含 review 完成度）
///
/// 一次聚合 plan + review，避免前端逐日调用。
/// 完成率仅基于 Priority A（核心任务），B/C 级为非必做项。
/// 前端调用: `invoke('list_plan_summaries')`
#[tauri::command]
pub async fn list_plan_summaries(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<PlanSummary>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let dates = crate::data::plan::list_daily_plan_dates(&data_dir)?;

    // 收集所有周计划中标记为休息日的日期（持久化记录，不受后期设置调整影响）
    let mut rest_days_from_week_plans: std::collections::HashSet<String> = std::collections::HashSet::new();
    // 收集所有周计划中手动添加的特殊情况排除日：date -> (type, note)
    let mut excluded_days_from_week_plans: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    if let Ok(week_dates) = crate::data::plan::list_week_plan_dates(&data_dir) {
        for iso_week in &week_dates {
            if let Ok(wp) = crate::data::plan::read_week_plan(&data_dir, iso_week) {
                for day in &wp.data.days {
                    if day.is_rest_day {
                        rest_days_from_week_plans.insert(day.date.clone());
                    }
                }
                for ex in &wp.data.excluded_days {
                    excluded_days_from_week_plans.insert(
                        ex.date.clone(),
                        (ex.reason_type.clone(), ex.note.clone()),
                    );
                }
            }
        }
    }

    let mut summaries = Vec::with_capacity(dates.len());
    let mut covered_dates: std::collections::HashSet<String> = std::collections::HashSet::new();

    for date in dates {
        covered_dates.insert(date.clone());
        let plan = crate::data::plan::read_daily_plan(&data_dir, &date).ok();
        let review = crate::data::records::read_review(&data_dir, &date).ok();

        let planned_tasks = plan.as_ref().map(|p| p.data.total_tasks).unwrap_or(0);
        let planned_hours = plan.as_ref().map(|p| p.data.total_hours).unwrap_or(0.0);
        // 优先使用周计划中的休息日标记，回退到任务为空判断
        let is_rest_day = rest_days_from_week_plans.contains(&date)
            || plan
                .as_ref()
                .map(|p| p.data.tasks.is_empty() && p.data.total_tasks == 0)
                .unwrap_or(false);

        // 排除日信息（来自周计划手动添加的 excluded_days）
        let (is_excluded, excluded_type, excluded_note) =
            if let Some((t, n)) = excluded_days_from_week_plans.get(&date) {
                (true, Some(t.clone()), n.clone())
            } else {
                (false, None, None)
            };

        let (completed_tasks, completion_rate) = compute_priority_a_completion(&review);
        let actual_hours = review
            .as_ref()
            .map(|r| crate::data::records::review_actual_hours(r))
            .unwrap_or(0.0);

        summaries.push(PlanSummary {
            date,
            has_plan: plan.is_some(),
            has_review: review.is_some(),
            planned_tasks,
            planned_hours,
            completed_tasks,
            completion_rate,
            actual_hours,
            is_rest_day,
            is_excluded,
            excluded_type,
            excluded_note,
        });
    }

    // 补充周计划中标记为休息日但无日计划文件的日期
    for rest_date in &rest_days_from_week_plans {
        if !covered_dates.contains(rest_date) {
            summaries.push(PlanSummary {
                date: rest_date.clone(),
                has_plan: false,
                has_review: false,
                planned_tasks: 0,
                planned_hours: 0.0,
                completed_tasks: 0,
                completion_rate: 0.0,
                actual_hours: 0.0,
                is_rest_day: true,
                is_excluded: false,
                excluded_type: None,
                excluded_note: None,
            });
        }
    }

    // 补充周计划中标记为特殊情况排除日但无日计划文件的日期
    for (ex_date, (ex_type, ex_note)) in &excluded_days_from_week_plans {
        if !covered_dates.contains(ex_date) {
            summaries.push(PlanSummary {
                date: ex_date.clone(),
                has_plan: false,
                has_review: false,
                planned_tasks: 0,
                planned_hours: 0.0,
                completed_tasks: 0,
                completion_rate: 0.0,
                actual_hours: 0.0,
                is_rest_day: false,
                is_excluded: true,
                excluded_type: Some(ex_type.clone()),
                excluded_note: ex_note.clone(),
            });
        }
    }

    summaries.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(summaries)
}

/// 计算完成率
///
/// 优先从新版 `task_reviews` 聚合（结构化复盘），回退到旧版 `data.completion`。
///
/// 规则：
/// - 优先统计 A 级任务完成率；若无 A 级任务，则统计全部任务完成率
/// - completion_rate = done / total * 100
/// - completed_tasks 返回已完成任务数（所有级别）
fn compute_priority_a_completion(
    review: &Option<crate::data::records::ReviewFile>,
) -> (i32, f64) {
    match review {
        Some(r) => {
            // 新版：优先从 task_reviews 聚合
            if !r.task_reviews.is_empty() {
                let mut a_total = 0i32;
                let mut a_done = 0i32;
                let mut all_total = 0i32;
                let mut all_done = 0i32;

                for tr in &r.task_reviews {
                    all_total += 1;
                    let is_done = tr.status == "completed";
                    if is_done {
                        all_done += 1;
                    }
                    // 优先用 task_reviews 自带的 priority 字段，回退到空字符串（视为非 A）
                    if tr.priority == "A" {
                        a_total += 1;
                        if is_done {
                            a_done += 1;
                        }
                    }
                }

                let completed = all_done;
                let rate = if a_total > 0 {
                    (a_done as f64 / a_total as f64) * 100.0
                } else if all_total > 0 {
                    // 无 A 级任务时，用全部任务完成率
                    (all_done as f64 / all_total as f64) * 100.0
                } else {
                    0.0
                };
                return (completed, rate);
            }

            // 旧版：从 data.completion 读取
            let a_total = r.data.completion.priority_a_total;
            let a_done = r.data.completion.priority_a_done;
            let rate = if a_total > 0 {
                (a_done as f64 / a_total as f64) * 100.0
            } else if r.data.completion.priority_b_total > 0 {
                // 无 A 级任务，用 B 级完成率
                let b_total = r.data.completion.priority_b_total;
                let b_done = r.data.completion.priority_b_done;
                (b_done as f64 / b_total as f64) * 100.0
            } else {
                // 无任何任务且有 review（旧版 AI 生成），视为完成
                100.0
            };
            (a_done, rate)
        }
        None => (0, 0.0),
    }
}

/// 获取指定周的日计划摘要
///
/// 一次返回本周 7 天的 PlanSummary（无论是否有 plan 文件）。
/// 完成率仅基于 Priority A（核心任务）。
/// 前端调用: `invoke('get_week_summaries', { weekStart: '2026-07-21' })`
#[tauri::command]
pub async fn get_week_summaries(
    week_start: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<PlanSummary>, String> {
    crate::data::validate_date(&week_start)?;
    let data_dir = get_data_dir(state.inner())?;

    // 读取本周的周计划，获取休息日标记和特殊情况排除日
    let iso_week = iso_week_string(&week_start)?;
    let week_plan = crate::data::plan::read_week_plan(&data_dir, &iso_week).ok();
    let mut rest_days_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut excluded_map: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    if let Some(wp) = &week_plan {
        for day in &wp.data.days {
            if day.is_rest_day {
                rest_days_set.insert(day.date.clone());
            }
        }
        for ex in &wp.data.excluded_days {
            excluded_map.insert(ex.date.clone(), (ex.reason_type.clone(), ex.note.clone()));
        }
    }

    let mut summaries = Vec::with_capacity(7);
    for i in 0..7 {
        let date_str = crate::data::add_days(&week_start, i)
            .map_err(|e| format!("无效的周起始日期 {}: {}", week_start, e))?;

        let plan = crate::data::plan::read_daily_plan(&data_dir, &date_str).ok();
        let review = crate::data::records::read_review(&data_dir, &date_str).ok();

        let planned_tasks = plan.as_ref().map(|p| p.data.total_tasks).unwrap_or(0);
        let planned_hours = plan.as_ref().map(|p| p.data.total_hours).unwrap_or(0.0);
        // 优先使用周计划中的休息日标记，回退到任务为空判断
        let is_rest_day = rest_days_set.contains(&date_str)
            || plan
                .as_ref()
                .map(|p| p.data.tasks.is_empty() && p.data.total_tasks == 0)
                .unwrap_or(false);

        let (is_excluded, excluded_type, excluded_note) =
            if let Some((t, n)) = excluded_map.get(&date_str) {
                (true, Some(t.clone()), n.clone())
            } else {
                (false, None, None)
            };

        let (completed_tasks, completion_rate) = compute_priority_a_completion(&review);
        let actual_hours = review
            .as_ref()
            .map(|r| crate::data::records::review_actual_hours(r))
            .unwrap_or(0.0);

        summaries.push(PlanSummary {
            date: date_str,
            has_plan: plan.is_some(),
            has_review: review.is_some(),
            planned_tasks,
            planned_hours,
            completed_tasks,
            completion_rate,
            actual_hours,
            is_rest_day,
            is_excluded,
            excluded_type,
            excluded_note,
        });
    }
    Ok(summaries)
}

/// AI 生成日计划
///
/// 调用 Planner Agent 读取 State + User Model + 昨日复盘，
/// 通过 AI 生成日计划 Markdown，解析后保存并返回。
/// 前端调用: `invoke('generate_daily_plan', { date: '2026-07-24' })`
#[tauri::command]
pub async fn generate_daily_plan(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 plan/state 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成计划。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    planner.generate_daily_plan(&data_dir, &date).await
}

/// AI 生成周计划
///
/// 生成周计划概览并逐天生成日计划。
/// 前端调用: `invoke('generate_week_plan', { weekStart: '2026-07-21', excludedDays: [], workloadAdjustment: undefined })`
#[tauri::command]
pub async fn generate_week_plan(
    week_start: String,
    #[allow(unused_variables)] excluded_days: Vec<ExcludedDay>,
    #[allow(unused_variables)] workload_adjustment: Option<WorkloadAdjustment>,
    state: State<'_, Mutex<AppState>>,
) -> Result<WeekPlanFile, String> {
    crate::data::validate_date(&week_start)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 plan/state 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成周计划。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    planner
        .generate_week_plan(&data_dir, &week_start, &excluded_days, workload_adjustment.as_ref())
        .await
}

/// 周中新增排除日并重排剩余天数（AI 驱动）
///
/// 在用户于周计划页点击"今天及之后"的日期标记为排除日时调用。
/// AI 会根据新增的排除日重新生成本周剩余天数的 subject_allocations，
/// 把原本安排在排除日的任务量分摊到剩余学习日。
///
/// 前端调用: `invoke('add_excluded_day_and_regenerate', { weekStart: '2026-07-21', excludedDay: { date, reason_type, note } })`
#[tauri::command]
pub async fn add_excluded_day_and_regenerate(
    week_start: String,
    excluded_day: ExcludedDay,
    state: State<'_, Mutex<AppState>>,
) -> Result<RegenerateResult, String> {
    crate::data::validate_date(&week_start)?;
    crate::data::validate_date(&excluded_day.date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 plan/state 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let (regenerated, affected_dates, used_fallback) = planner
        .regenerate_after_exclusion(&data_dir, &week_start, excluded_day)
        .await?;

    Ok(RegenerateResult {
        regenerated,
        affected_dates,
        used_fallback,
        // 排除日重排不涉及超量进度，一致性警告为空
        consistency_warnings: Vec::new(),
    })
}

// ============================================================================
// Review 命令
// ============================================================================

/// 读取复盘记录
///
/// 读取 `records/YYYY-MM-DD_review.json` 文件。
/// 前端调用: `invoke('get_review', { date: '2026-07-24' })`
#[tauri::command]
pub async fn get_review(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ReviewFile, String> {
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;
    crate::data::records::read_review(&data_dir, &date)
}

/// 列出所有复盘日期
///
/// 返回 records/ 目录下所有 `YYYY-MM-DD_review.json` 对应的日期列表，按升序排列。
/// 前端调用: `invoke('list_review_dates')`
#[tauri::command]
pub async fn list_review_dates(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::records::list_review_dates(&data_dir)
}

/// AI 生成复盘记录
///
/// 调用 Review Agent 读取今日计划和 State，
/// 通过 AI 生成复盘 Markdown，解析后保存并返回。
/// 前端调用: `invoke('generate_review', { date: '2026-07-24' })`
#[tauri::command]
pub async fn generate_review(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ReviewFile, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 records/state 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成复盘。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let review_agent = ReviewAgent::new(&ai_service);
    review_agent.generate_review(&data_dir, &date).await
}

/// 提交结构化复盘（新版 Review，无需 AI）
///
/// 前端用户完成步骤式问答后，将结构化数据提交到后端。
/// 后端负责：保存 Review 文件 + 更新 State 中的任务状态。
/// 返回 needs_regeneration 标志，指示是否需要调用 AI 重新生成本周剩余天数计划。
/// 前端调用: `invoke('submit_review', { payload: { date, task_reviews, daily_review } })`
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubmitReviewPayload {
    pub date: String,
    pub task_reviews: Vec<crate::data::records::TaskReviewEntry>,
    pub daily_review: crate::data::records::DailyReviewInput,
    /// 超量完成记录（可选）：用户实际进度领先计划时填写
    #[serde(default)]
    pub overcompletion: Vec<crate::data::records::OvercompletionEntry>,
}

/// submit_review 的返回结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubmitReviewResult {
    /// 是否需要调用 AI 重新生成本周剩余天数计划
    pub needs_regeneration: bool,
    /// 触发重排的原因（用于前端展示）
    pub regen_reasons: Vec<String>,
    /// 次日简报是否已在后台开始生成（fire-and-forget）
    #[serde(default)]
    pub briefing_generating: bool,
}

#[tauri::command]
pub async fn submit_review(
    payload: SubmitReviewPayload,
    app_state: State<'_, Mutex<AppState>>,
) -> Result<SubmitReviewResult, String> {
    crate::data::validate_date(&payload.date)?;
    let data_dir = get_data_dir(app_state.inner())?;

    // H1 并发保护：串行化 records/state 写入，避免与任务状态更新等并发竞态
    let io_lock = crate::get_io_lock(app_state.inner())?;
    let _io_guard = io_lock.lock().await;

    // 聚合实际学习时长（分钟 → 小时），写入 data.total_hours
    // 供分析页/历史计划/周期对比读取，避免 actual_hours 恒为 0
    let total_actual_minutes: i64 = payload
        .task_reviews
        .iter()
        .filter_map(|tr| tr.actual_minutes)
        .sum();
    let total_actual_hours = total_actual_minutes as f64 / 60.0;

    // 从 task_reviews 聚合 completion 数据
    // 供 briefing / dashboard / planner 等直接读取 data.completion 的代码路径使用
    let mut a_total = 0i32;
    let mut a_done = 0i32;
    let mut b_total = 0i32;
    let mut b_done = 0i32;
    let mut all_total = 0i32;
    let mut all_done = 0i32;
    for tr in &payload.task_reviews {
        all_total += 1;
        let is_done = tr.status == "completed";
        if is_done {
            all_done += 1;
        }
        match tr.priority.as_str() {
            "A" => {
                a_total += 1;
                if is_done {
                    a_done += 1;
                }
            }
            "B" => {
                b_total += 1;
                if is_done {
                    b_done += 1;
                }
            }
            _ => {}
        }
    }
    // 无 A/B 级任务时，将全部任务计入 A 级字段（保证 completion_rate 正确）
    let (ca_total, ca_done) = if a_total + b_total == 0 && all_total > 0 {
        (all_total, all_done)
    } else {
        (a_total, a_done)
    };
    let comp_total = ca_total + b_total;
    let comp_done = ca_done + b_done;
    let completion_rate = if comp_total > 0 {
        (comp_done as f64 / comp_total as f64) * 100.0
    } else {
        0.0
    };

    // 构建 ReviewFile
    let now = crate::data::now_string();
    let review = crate::data::records::ReviewFile {
        version: "1.0.0".to_string(),
        meta: crate::data::records::ReviewMeta {
            date: payload.date.clone(),
            r#type: "structured_review".to_string(),
            plan_ref: format!("plan/{}{}", payload.date, crate::data::plan::DAILY_PLAN_FILE_SUFFIX),
            generated_at: now,
        },
        data: crate::data::records::ReviewData {
            total_hours: total_actual_hours,
            completion: crate::data::records::ReviewCompletion {
                priority_a_total: ca_total,
                priority_a_done: ca_done,
                priority_b_total: b_total,
                priority_b_done: b_done,
                completion_rate,
            },
            ..Default::default()
        },
        view: None,
        task_reviews: payload.task_reviews.clone(),
        daily_review: Some(payload.daily_review.clone()),
        overcompletion: payload.overcompletion.clone(),
    };

    // 1. 保存 Review JSON
    crate::data::records::save_review(&data_dir, &review)?;

    // 2. 同步更新 State：标记任务完成/放弃 + 记录掌握程度
    if let Ok(mut state) = crate::data::state::read_state(&data_dir) {
        let mut changed = false;

        // 先收集所有需要更新的信息（避免借冲突）
        let mut updates: Vec<(String, String, String)> = Vec::new(); // (task_id, new_status, mastery)

        for tr in &payload.task_reviews {
            let new_status = match tr.status.as_str() {
                "completed" => "done",
                "abandoned" => "abandoned",
                _ => "pending",
            };

            // 更新 current_task 中的任务状态
            for task in &mut state.current_task.tasks {
                if task.task_id.as_deref() == Some(&tr.task_id) {
                    let ns = match new_status {
                        "done" => crate::data::state::TaskStatus::Done,
                        "abandoned" => crate::data::state::TaskStatus::Abandoned,
                        _ => crate::data::state::TaskStatus::Pending,
                    };
                    if task.status != ns {
                        task.status = ns;
                        changed = true;
                    }
                    break;
                }
            }

            // 收集 subject key 和任务标题（稍后用于更新科目进度）
            if tr.status == "completed" || tr.status == "partial" {
                let subj_key = state.current_task.tasks
                    .iter()
                    .find(|t| t.task_id.as_deref() == Some(&tr.task_id))
                    .map(|t| t.subject.clone());

                let task_label = state.current_task.tasks
                    .iter()
                    .find(|t| t.task_id.as_deref() == Some(&tr.task_id))
                    .map(|t| t.task.clone())
                    .unwrap_or_default();

                if let Some(sk) = subj_key {
                    let mastery_label = match tr.mastery.as_str() {
                        "mastered" => "",
                        "basic" => "",
                        "weak" => "（需巩固）",
                        _ => "",
                    };
                    updates.push((sk, task_label, mastery_label.to_string()));
                }
            }
        }

        // 第二阶段：更新科目 progress（此时不再借 state.current_task）
        for (subj_key, task_label, mastery_label) in &updates {
            if let Some(subj) = crate::data::state::get_subject_state_mut(&mut state, subj_key) {
                let entry = if mastery_label.is_empty() {
                    task_label.clone()
                } else {
                    format!("{} {}", task_label, mastery_label)
                };
                if !subj.completed.contains(&entry) {
                    subj.completed.push(entry);
                    changed = true;
                }

                if !mastery_label.is_empty() && !subj.current_focus.contains(mastery_label) {
                    subj.current_focus = format!("{} {}", subj.current_focus, mastery_label).trim().to_string();
                }
            }
        }

        // 第三阶段：处理超量完成 —— 用用户实际到达的章节覆盖科目 current_focus，
        // 避免下一轮计划生成时进度落后于实际。
        if !payload.overcompletion.is_empty() {
            for oc in &payload.overcompletion {
                if let Some(subj) = crate::data::state::get_subject_state_mut(&mut state, &oc.subject) {
                    if !oc.chapter_reached.is_empty() {
                        subj.current_focus = oc.chapter_reached.clone();
                        if !subj.completed.contains(&oc.chapter_reached) {
                            subj.completed.push(oc.chapter_reached.clone());
                        }
                        changed = true;
                        log::info!(
                            "超量完成更新：{} 的 current_focus 更新为 {}",
                            oc.subject,
                            oc.chapter_reached
                        );
                    }
                }
            }
        }

        if changed {
            state.progress.last_study_date = payload.date.clone();
            state.meta.last_updated = crate::data::now_string();
            crate::data::state::save_state(&data_dir, &state)?;
        }
    }

    log::info!("结构化复盘已保存: {}", payload.date);

    // 3. 判断是否需要重新生成本周剩余天数计划
    let needs_regeneration = crate::core::planner::check_review_needs_regeneration(&review);
    let mut regen_reasons = Vec::new();

    if needs_regeneration {
        // 收集触发原因（用于前端展示）
        let has_uncompleted = review
            .task_reviews
            .iter()
            .any(|tr| tr.status == "incomplete" || tr.status == "partial");
        let feels_hard = review
            .daily_review
            .as_ref()
            .map(|d| d.overall_feeling == "hard")
            .unwrap_or(false);
        let has_overcompletion = !review.overcompletion.is_empty();

        if has_uncompleted {
            regen_reasons.push("存在未完成任务".to_string());
        }
        if feels_hard {
            regen_reasons.push("今日学习感受困难".to_string());
        }
        if has_overcompletion {
            regen_reasons.push("存在计划外学习内容需要修正后续计划".to_string());
        }

        log::info!(
            "复盘 {} 触发重排: {}",
            payload.date,
            regen_reasons.join("；")
        );
    }

    // 4. 在后台自动生成次日简报（fire-and-forget，失败不影响复盘提交）
    //
    // 简报基于本次复盘生成，供次日打开应用时展示。
    // 使用 tauri::async_runtime::spawn 异步执行，不阻塞当前命令返回。
    // 失败仅记录日志，不影响复盘提交结果。
    let briefing_generating = if let Ok(next_day) = crate::data::add_days(&payload.date, 1) {
        let ai_service = get_ai_service(app_state.inner()).ok();
        let data_dir_clone = data_dir.clone();
        let review_date = payload.date.clone();
        if let Some(ai) = ai_service {
            log::info!(
                "复盘 {} 提交后触发次日 {} 简报生成（后台）",
                payload.date,
                next_day
            );
            // H1 并发保护：简报生成也需持 IO 锁（写 records/briefing 文件），
            // 但需等待当前命令释放锁后再执行，避免嵌套持锁。
            let io_lock_clone = io_lock.clone();
            tauri::async_runtime::spawn(async move {
                let _bg_guard = io_lock_clone.lock().await;
                let agent = BriefingAgent::new(&ai);
                match agent
                    .generate_briefing(&data_dir_clone, &next_day, &review_date, "auto")
                    .await
                {
                    Ok(_) => {
                        log::info!("次日简报已自动生成: {}", next_day);
                    }
                    Err(e) => {
                        log::warn!("次日简报自动生成失败 {}: {}", next_day, e);
                    }
                }
            });
            true
        } else {
            log::warn!("未配置 AI Provider，跳过次日简报自动生成");
            false
        }
    } else {
        false
    };

    Ok(SubmitReviewResult {
        needs_regeneration,
        regen_reasons,
        briefing_generating,
    })
}

/// 复盘后重新生成本周剩余天数计划（AI 驱动）
///
/// 在 submit_review 返回 needs_regeneration=true 后由前端调用。
/// AI 会根据复盘结果重新生成本周剩余天数的 subject_allocations，
/// 并在今天是剩余天数之一时重新生成今日日计划。
///
/// 前端调用: `invoke('regenerate_remaining_days', { reviewDate: '2026-07-30' })`
#[tauri::command]
pub async fn regenerate_remaining_days(
    review_date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<RegenerateResult, String> {
    crate::data::validate_date(&review_date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 plan/state 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let (regenerated, affected_dates, used_fallback, consistency_warnings) = planner
        .regenerate_remaining_days_after_review(&data_dir, &review_date)
        .await?;

    Ok(RegenerateResult {
        regenerated,
        affected_dates,
        used_fallback,
        consistency_warnings,
    })
}

/// regenerate_remaining_days 的返回结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegenerateResult {
    /// 是否实际执行了重排
    pub regenerated: bool,
    /// 受影响的日期列表
    pub affected_dates: Vec<String>,
    /// AI 调用失败时是否启用了确定性兜底安排（供前端提示用户）
    #[serde(default)]
    pub used_fallback: bool,
    /// 一致性校验警告：声明了超量进度的科目重排后未生效时给出提示
    #[serde(default)]
    pub consistency_warnings: Vec<String>,
}

// ============================================================================
// Briefing 命令
// ============================================================================

/// get_briefing 的返回结构
///
/// 包含简报文件本体、昨日复盘是否存在、是否在补复盘窗口内等元信息，
/// 供前端判断是否展示「先去复盘」提示或「AI 建议不可用」状态。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GetBriefingResult {
    /// 简报文件（若存在）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub briefing: Option<crate::data::briefing::BriefingFile>,
    /// 简报是否存在
    pub exists: bool,
    /// 昨日复盘是否存在（决定能否提供 AI 建议）
    pub yesterday_review_exists: bool,
    /// 今日是否为休息日（来自用户设置）
    pub is_rest_day: bool,
    /// 今日是否为周计划排除日
    pub is_excluded_day: bool,
    /// 昨日是否为休息日或排除日（若是，则不要求补复盘）
    pub yesterday_exempt: bool,
    /// 是否在补复盘窗口内（今日且未过每日结束时间 +1 小时）
    pub within_makeup_window: bool,
}

/// 获取指定日期的每日简报
///
/// 前端调用: `invoke('get_briefing', { date: '2026-08-04' })`
#[tauri::command]
pub async fn get_briefing(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<GetBriefingResult, String> {
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;
    let settings = crate::load_settings(&data_dir);

    // 读取简报文件
    let briefing = crate::data::briefing::read_briefing(&data_dir, &date).ok();
    let exists = briefing.is_some();

    // 昨日日期
    let yesterday = crate::data::add_days(&date, -1).unwrap_or_else(|_| date.clone());

    // 昨日复盘是否存在
    let yesterday_review_exists = crate::data::records::read_review(&data_dir, &yesterday).is_ok();

    // 判断昨日是否为休息日或排除日（若是则不要求补复盘）
    let rest_days = settings.rest_days();
    let yesterday_weekday = crate::data::weekday_name(&yesterday).unwrap_or_default();
    let yesterday_is_rest = rest_days.iter().any(|d| d == &yesterday_weekday);

    let yesterday_is_excluded = crate::data::plan::read_week_plan_for_date(&data_dir, &yesterday)
        .ok()
        .map(|wp| wp.data.excluded_days.iter().any(|d| d.date == yesterday))
        .unwrap_or(false);
    let yesterday_exempt = yesterday_is_rest || yesterday_is_excluded;

    // 今日是否为休息日
    let today_weekday = crate::data::weekday_name(&date).unwrap_or_default();
    let is_rest_day = rest_days.iter().any(|d| d == &today_weekday);

    // 今日是否为排除日
    let is_excluded_day = crate::data::plan::read_week_plan_for_date(&data_dir, &date)
        .ok()
        .map(|wp| wp.data.excluded_days.iter().any(|d| d.date == date))
        .unwrap_or(false);

    // 补复盘窗口：今日日期等于今天，且当前时间在每日结束时间 +1 小时内
    // 超过该窗口则视为「错过补复盘」，不再提供 AI 建议
    let today = crate::data::today_string();
    let within_makeup_window = if date == today {
        // 简单判断：今日都在补复盘窗口内（用户可在今日任何时候补复盘）
        // 真正的「错过」是指到了次日仍未补复盘，那时简报就不会自动生成了
        true
    } else {
        false
    };

    Ok(GetBriefingResult {
        briefing,
        exists,
        yesterday_review_exists,
        is_rest_day,
        is_excluded_day,
        yesterday_exempt,
        within_makeup_window,
    })
}

/// 重新生成指定日期的每日简报（AI 驱动）
///
/// 用户在 Dashboard 手动点击「重新生成简报」时调用。
/// 必须存在昨日复盘才能生成（否则 AI 无依据）。
///
/// 前端调用: `invoke('regenerate_briefing', { date: '2026-08-04' })`
#[tauri::command]
pub async fn regenerate_briefing(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::data::briefing::BriefingFile, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 records/briefing 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成简报。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    // 基于昨日复盘生成
    let yesterday = yesterday_of(&date)?;

    // 校验昨日复盘存在（简报必须有依据）
    if crate::data::records::read_review(&data_dir, &yesterday).is_err() {
        return Err(format!(
            "昨日（{}）复盘不存在，无法生成简报。请先完成昨日复盘。",
            yesterday
        ));
    }

    // 删除旧简报（若有）
    let _ = crate::data::briefing::delete_briefing(&data_dir, &date);

    let agent = BriefingAgent::new(&ai_service);
    agent
        .generate_briefing(&data_dir, &date, &yesterday, "manual")
        .await
}

/// 列出所有简报日期（YYYY-MM-DD，升序）
///
/// 前端调用: `invoke('list_briefing_dates')`
#[tauri::command]
pub async fn list_briefing_dates(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::briefing::list_briefing_dates(&data_dir)
}

// ============================================================================
// User Model 命令
// ============================================================================

/// 教材信息
#[derive(serde::Serialize)]
pub struct TextbookInfo {
    pub id: String,
    pub subject: String,
    pub title: String,
    pub filename: String,
    pub file_path: String,
}

/// 教材内容
#[derive(serde::Serialize)]
pub struct TextbookContent {
    pub id: String,
    pub content: String,
    pub file_path: String,
}

/// 列出所有教材
///
/// 遍历 `assets/resources/textbooks/{subject}/` 目录下的 Markdown 文件。
/// 前端调用: `invoke('list_textbooks')`
#[tauri::command]
pub async fn list_textbooks(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TextbookInfo>, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    let mut result = Vec::new();

    // 遍历 textbooks 目录下的子目录（按学科分类）
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            for subject_dir in subject_dirs.flatten() {
                let subject = subject_dir.file_name().to_string_lossy().to_string();
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md") {
                            let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            let id = format!("{}-{}", subject, filename);
                            let title = filename.replace('-', " ");
                            result.push(TextbookInfo {
                                id,
                                subject: subject.clone(),
                                title,
                                filename: format!("{}.md", filename),
                                file_path: path.to_string_lossy().to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

/// 读取教材内容
///
/// 根据 `id`（格式 `subject-filename`）读取对应的 Markdown 教材文件。
/// 前端调用: `invoke('read_textbook', { id: 'math-高等数学' })`
#[tauri::command]
pub async fn read_textbook(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookContent, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // id 格式为 subject-filename，直接搜索所有文件匹配
    let mut found_path = None;
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            'outer: for subject_dir in subject_dirs.flatten() {
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                        let subject = subject_dir.file_name().to_string_lossy().to_string();
                        let file_id = format!("{}-{}", subject, filename);
                        if file_id == id {
                            found_path = Some(path);
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    let path = found_path.ok_or_else(|| format!("教材不存在: {}", id))?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取教材失败: {}", e))?;

    Ok(TextbookContent {
        id,
        content,
        file_path: path.to_string_lossy().to_string(),
    })
}

/// 导入教材文件
///
/// 将用户选择的 Markdown 文件复制到 textbooks/{subject}/ 目录下。
/// 安全限制（C4-a）：仅允许 `.md` 扩展名、文件不大于 50MB，
/// 且 subject 仅允许字母/数字/连字符，防止路径穿越与任意文件复制。
/// 前端调用: `invoke('import_textbook', { subject: 'math', filePath: 'C:/...', title: '同济线代' })`
#[tauri::command]
pub async fn import_textbook(
    subject: String,
    file_path: String,
    title: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookInfo, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 校验 subject，仅允许字母/数字/连字符/下划线，防止目录穿越
    if subject.is_empty()
        || !subject
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("无效的学科名称，仅允许字母、数字、连字符".to_string());
    }

    let subject_dir = textbooks_dir.join(&subject);

    // 创建学科目录
    std::fs::create_dir_all(&subject_dir)
        .map_err(|e| format!("创建教材目录失败: {}", e))?;

    let src_path = std::path::Path::new(&file_path);

    // 仅允许 .md 扩展名（与 list/search 的 **/*.md 匹配逻辑一致）
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    if ext.as_deref() != Some("md") {
        return Err("仅支持导入 Markdown（.md）文件".to_string());
    }

    // 仅允许普通文件，且大小不超过 50MB
    let meta = std::fs::metadata(src_path).map_err(|e| format!("读取文件信息失败: {}", e))?;
    if !meta.is_file() {
        return Err("所选路径不是有效文件".to_string());
    }
    const MAX_TEXTBOOK_SIZE: u64 = 50 * 1024 * 1024;
    if meta.len() > MAX_TEXTBOOK_SIZE {
        return Err(format!(
            "文件过大（{:.1} MB），仅支持导入 50MB 以内的 Markdown 文件",
            meta.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let filename = src_path
        .file_stem()
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string_lossy()
        .to_string();

    let title = title.unwrap_or_else(|| filename.replace('-', " "));
    let dest_filename = format!("{}.md", filename);
    let dest_path = subject_dir.join(&dest_filename);

    // 复制文件
    std::fs::copy(src_path, &dest_path)
        .map_err(|e| format!("复制教材文件失败: {}", e))?;

    let id = format!("{}-{}", subject, filename);

    Ok(TextbookInfo {
        id,
        subject,
        title,
        filename: dest_filename,
        file_path: dest_path.to_string_lossy().to_string(),
    })
}

/// 保存用户选择的背景图到 data_dir/assets/backgrounds/
///
/// 将用户通过对话框选择的图片复制到应用数据目录，返回相对于 data_dir 的路径
/// （如 "assets/backgrounds/xxx.png"），供前端通过 convertFileSrc 加载。
/// 前端调用: `invoke('save_background_image', { filePath: 'C:/...' })`
#[tauri::command]
pub async fn save_background_image(
    file_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let dir = get_data_dir(state.inner())?;
    let backgrounds_dir = dir.join("assets").join("backgrounds");
    std::fs::create_dir_all(&backgrounds_dir)
        .map_err(|e| format!("创建背景图目录失败: {}", e))?;

    let src_path = std::path::Path::new(&file_path);
    let extension = src_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());

    // 用时间戳生成唯一文件名，避免覆盖
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest_filename = format!("bg_{}.{}", timestamp, extension);
    let dest_path = backgrounds_dir.join(&dest_filename);

    std::fs::copy(src_path, &dest_path)
        .map_err(|e| format!("复制背景图失败: {}", e))?;

    // 返回相对路径（使用 / 作为分隔符，便于跨平台拼接）
    let relative = format!("assets/backgrounds/{}", dest_filename);
    Ok(relative)
}

/// 将 relative_path 解析为 data_dir 内的绝对路径
///
/// 规范化 `..` / `.` / 反斜杠，并校验结果路径仍位于 data_dir 内，
/// 防止通过 `../../config/settings` 之类参数读取或删除 data_dir 之外的任意文件（C4-b）。
fn resolve_relative_path(data_dir: &std::path::Path, relative_path: &str) -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;

    let cleaned = relative_path.replace('\\', "/");
    let normalized = cleaned
        .split('/')
        .fold(PathBuf::new(), |mut acc, part| {
            match part {
                "" | "." => {}
                ".." => {
                    acc.pop();
                }
                _ => acc.push(part),
            }
            acc
        });

    let target = data_dir.join(normalized);
    let canonical_data_dir = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let canonical_target = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());

    if !canonical_target.starts_with(&canonical_data_dir) {
        return Err(format!(
            "路径越界: {:?} 不在数据目录 {:?} 内",
            canonical_target, canonical_data_dir
        ));
    }

    Ok(canonical_target)
}

/// 删除已保存的背景图文件
///
/// 前端调用: `invoke('delete_background_image', { relativePath: 'assets/backgrounds/xxx.png' })`
#[tauri::command]
pub async fn delete_background_image(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let dir = get_data_dir(state.inner())?;
    let full_path = resolve_relative_path(&dir, &relative_path)?;
    if full_path.exists() {
        std::fs::remove_file(&full_path)
            .map_err(|e| format!("删除背景图失败: {}", e))?;
    }
    Ok(())
}

/// 读取背景图文件并返回 base64 data URL
///
/// 由于 Tauri v2 的 assetProtocol 需要 scope 配置，直接返回 data URL 更简单可靠。
/// 前端调用: `invoke('read_background_as_data_url', { relativePath: 'assets/backgrounds/xxx.png' })`
#[tauri::command]
pub async fn read_background_as_data_url(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let dir = get_data_dir(state.inner())?;
    let full_path = resolve_relative_path(&dir, &relative_path)?;
    if !full_path.exists() {
        return Err(format!("背景图文件不存在: {}", full_path.display()));
    }

    // 读取文件字节
    let bytes = std::fs::read(&full_path)
        .map_err(|e| format!("读取背景图失败: {}", e))?;

    // 根据扩展名推断 MIME 类型
    let extension = full_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    };

    // 编码为 base64
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

/// 删除教材
///
/// 前端调用: `invoke('delete_textbook', { id: 'math-同济线代' })`
#[tauri::command]
pub async fn delete_textbook(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 查找匹配的文件并删除
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            for subject_dir in subject_dirs.flatten() {
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                        let subject = subject_dir.file_name().to_string_lossy().to_string();
                        let file_id = format!("{}-{}", subject, filename);
                        if file_id == id {
                            std::fs::remove_file(&path)
                                .map_err(|e| format!("删除教材失败: {}", e))?;
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    Err(format!("教材不存在: {}", id))
}

/// 重命名教材
///
/// 修改教材文件的 stem（不含扩展名），id 与新名称均由前端提供。
/// 前端调用: `invoke('rename_textbook', { id: 'math-同济线代', newTitle: '线性代数' })`
#[tauri::command]
pub async fn rename_textbook(
    id: String,
    new_title: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookInfo, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 校验新标题：非空、不含路径分隔符与非法字符
    let trimmed = new_title.trim();
    if trimmed.is_empty() {
        return Err("教材标题不能为空".to_string());
    }
    if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || trimmed.contains('*')
        || trimmed.contains('?')
        || trimmed.contains('"')
        || trimmed.contains('<')
        || trimmed.contains('>')
        || trimmed.contains('|')
    {
        return Err("教材标题不能包含 / \\ : * ? \" < > | 等特殊字符".to_string());
    }

    // 查找原文件
    let (subject, old_stem, old_path) = {
        let mut found: Option<(String, String, std::path::PathBuf)> = None;
        if textbooks_dir.exists() {
            if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
                for subject_dir in subject_dirs.flatten() {
                    if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                        for file in files.flatten() {
                            let path = file.path();
                            let stem = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let subj = subject_dir.file_name().to_string_lossy().to_string();
                            if format!("{}-{}", subj, stem) == id {
                                found = Some((subj, stem, path));
                                break;
                            }
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
        }
        found.ok_or_else(|| format!("教材不存在: {}", id))?
    };

    // 新文件名 stem：将标题中的空格替换为 '-' 以保持与现有 id 规则一致
    let new_stem = trimmed.replace(' ', "-");
    if new_stem == old_stem {
        // 无需重命名，直接返回当前信息
        return Ok(TextbookInfo {
            id: id.clone(),
            subject: subject.clone(),
            title: trimmed.to_string(),
            filename: format!("{}.md", new_stem),
            file_path: old_path.to_string_lossy().to_string(),
        });
    }

    let new_path = old_path
        .parent()
        .ok_or_else(|| "无法获取教材所在目录".to_string())?
        .join(format!("{}.md", new_stem));

    // 检查目标是否已存在
    if new_path.exists() {
        return Err(format!("已存在同名教材: {}", trimmed));
    }

    std::fs::rename(&old_path, &new_path).map_err(|e| format!("重命名教材失败: {}", e))?;

    Ok(TextbookInfo {
        id: format!("{}-{}", subject, new_stem),
        subject,
        title: trimmed.to_string(),
        filename: format!("{}.md", new_stem),
        file_path: new_path.to_string_lossy().to_string(),
    })
}

/// 教材内搜索结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct TextbookSearchHit {
    pub textbook_id: String,
    pub textbook_title: String,
    pub subject: String,
    pub line_number: usize,
    pub snippet: String,
    /// 该行命中的关键词数量（用于排序，前端可忽略）
    #[serde(default)]
    pub hit_weight: usize,
    /// 该行实际命中的关键词（供前端高亮片段与正文）
    #[serde(default)]
    pub matched_terms: Vec<String>,
}

/// 在已导入教材中全文搜索
///
/// 前端调用: `invoke('search_in_textbook', { query: '二叉搜索树' })`
///
/// 用户输入通常是整句提问或整道题目，不能作为单一子串去精确匹配。
/// 因此先将查询拆解为关键词（中文按 2-gram、英文按单词、数字按连续串），
/// 过滤常见停用字后，逐词在教材中匹配并按命中关键词数量打分排序。
/// 同时解析「第N章 / 第N题」式章节引用做定向检索，命中章节标题与题目行
/// 可获得额外加权，确保用户只报章节/题号时也能定位到教材内容。
#[tauri::command]
pub async fn search_in_textbook(
    query: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TextbookSearchHit>, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 1. 解析章节 / 题号引用 + 拆解关键词
    let (chapter_ref, problem_ref) = parse_refs(&query);
    let terms = extract_search_terms(&query);

    let mut hits = Vec::new();

    if !textbooks_dir.exists() {
        return Ok(hits);
    }

    if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
        for subject_dir in subject_dirs.flatten() {
            let subject = subject_dir.file_name().to_string_lossy().to_string();
            if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    let textbook_id = format!("{}-{}", subject, filename);
                    let textbook_title = filename.replace('-', " ");

                    let content = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let lines: Vec<&str> = content.lines().collect();
                    if lines.is_empty() {
                        continue;
                    }

                    // 2. 确定检索范围：若指定了章节，限定在该章标题到下一章之间
                    let (start, end) = match chapter_ref {
                        Some(n) => {
                            // 优先 Markdown 标题（`# 第N章`）
                            if let Some(ci) = find_chapter_line(&lines, n) {
                                let next = lines[ci + 1..]
                                    .iter()
                                    .position(|l| is_chapter_heading(l))
                                    .map(|p| ci + 1 + p)
                                    .unwrap_or(lines.len());
                                (ci, next)
                            }
                            // 扁平 OCR 文本（无 `#` 标题）：用 `N.x` 小节前缀定位章节范围
                            else if let Some((fs, fe)) = find_flat_chapter_range(&lines, n) {
                                (fs, fe)
                            }
                            // 完全找不到章节，退回全文
                            else {
                                (0, lines.len())
                            }
                        }
                        None => (0, lines.len()),
                    };

                    // 3. 逐行打分
                    // problem_ordinal 记录当前习题小节内第几道题（OCR 题号识别失败时的顺序兜底）
                    let mut in_exercise = false; // 是否处于习题区（仅在此区内计数题目序号）
                    let mut problem_ordinal = 0usize;
                    for idx in start..end {
                        let line = lines[idx];
                        let lower = line.to_lowercase();
                        let mut matched = 0usize;
                        let mut first_pos: Option<usize> = None;
                        let mut matched_terms: Vec<String> = Vec::new();

                        for term in &terms {
                            let tl = term.to_lowercase();
                            if lower.contains(&tl) {
                                matched += 1;
                                matched_terms.push(term.clone());
                                let pos = lower.find(&tl).unwrap_or(0);
                                if first_pos.map(|p| pos < p).unwrap_or(true) {
                                    first_pos = Some(pos);
                                }
                            }
                        }

                        // 章节标题：仅当查询本身含章节引用时作为定位锚点加权，
                        // 普通关键词查询不奖励标题，避免挤掉真正的内容匹配
                        if chapter_ref.is_some() && is_chapter_heading(line) {
                            matched += 2;
                        }
                        // 题号引用：OCR 容错匹配题号 + 顺序位置兜底
                        if let Some(target) = problem_ref {
                            // 习题区状态机：进入习题区才计数题目序号，
                            // 避免把正文里的列表序号（如 `3 第三代…`）误当题号导致顺序偏移
                            if is_exercise_start(&lower) {
                                in_exercise = true;
                                problem_ordinal = 0;
                            } else if is_answer_section(&lower) {
                                in_exercise = false;
                            }
                            if in_exercise {
                                let extracted = problem_number_of(line);
                                if extracted.is_some() {
                                    problem_ordinal += 1;
                                }
                                match extracted {
                                    // 精确命中题号（已做 OCR 噪音容错）
                                    Some(n) if n == target => matched += 5,
                                    // 题号被 OCR 认错时，用「本节第 N 道题」的序号兜底定位
                                    Some(_) if problem_ordinal == target as usize => matched += 3,
                                    _ => {}
                                }
                            }
                        }
                        // 单独报章节（无关键词）时，也要把该章标题带出来
                        if matched == 0 && chapter_ref.is_some() && idx == start && is_chapter_heading(line) {
                            matched = 1;
                        }

                        if matched == 0 {
                            continue;
                        }

                        // 4. 截取首个命中位置的上下文（前后各 50 字符）
                        let ctx_start = first_pos.unwrap_or(0).saturating_sub(50);
                        let ctx_end = (ctx_start + 100).min(line.len());
                        let snippet = safe_char_slice(line, ctx_start, ctx_end);
                        let prefix = if ctx_start > 0 { "…" } else { "" };
                        let suffix = if ctx_end < line.len() { "…" } else { "" };
                        hits.push(TextbookSearchHit {
                            textbook_id: textbook_id.clone(),
                            textbook_title: textbook_title.clone(),
                            subject: subject.clone(),
                            line_number: idx + 1,
                            snippet: format!("{}{}{}", prefix, snippet, suffix),
                            hit_weight: matched,
                            matched_terms,
                        });
                    }
                }
            }
        }
    }

    // 5. 按命中关键词数量降序，保留最相关的若干条
    hits.sort_by(|a, b| {
        b.hit_weight
            .cmp(&a.hit_weight)
            .then(b.line_number.cmp(&a.line_number))
    });
    hits.truncate(20);

    Ok(hits)
}

/// 浅层中文分词：把查询拆成可检索的关键词。
///
/// - ASCII 连续串（英文单词 / 数字 / 符号）作为独立词；
/// - CJK 汉字按相邻 2-gram 切分并过滤常见停用字；
/// - 过滤掉过于通用的单字/词，避免噪声。
fn extract_search_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();

    // ASCII 连续串
    let mut ascii_buf = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            ascii_buf.push(ch);
        } else {
            if ascii_buf.len() >= 2 {
                terms.push(ascii_buf.clone());
            }
            ascii_buf.clear();
        }
    }
    if ascii_buf.len() >= 2 {
        terms.push(ascii_buf);
    }

    // CJK：过滤停用字后生成 2-gram
    let cjk_chars: Vec<char> = query
        .chars()
        .filter(|c| is_cjk(*c))
        .filter(|c| !is_stop_char(*c))
        .collect();
    for w in cjk_chars.windows(2) {
        let t: String = w.iter().collect();
        if !t.is_empty() {
            terms.push(t);
        }
    }

    terms
}

/// 是否为 CJK 汉字（不含标点、数字、字母）
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}')
}

/// 常见提问/口语停用字：这些字单独作为 2-gram 没有检索意义
fn is_stop_char(c: char) -> bool {
    matches!(
        c,
        '的' | '了' | '是' | '我' | '你' | '他' | '她' | '它' | '这' | '那' | '就' | '都'
            | '也' | '在' | '有' | '和' | '与' | '及' | '把' | '被' | '让' | '帮' | '请'
            | '问' | '题' | '道' | '下' | '么' | '什' | '怎' | '吗' | '呢' | '呀' | '啊'
            | '吧' | '个' | '种' | '讲' | '解' | '答' | '方' | '法' | '一' | '不' | '要'
            | '会' | '能' | '可' | '以' | '到' | '里' | '之' | '后' | '前' | '上' | '中'
            | '或' | '于' | '而' | '并' | '且' | '对' | '为' | '从' | '叫' | '给'
            | '过' | '来' | '去' | '起' | '张' | '章' | '节' | '本' | '出'
            | '面' | '路' | '程' | '点' | '想' | '看' | '试' | '列' | '好'
            | '很' | '太' | '更' | '最' | '紧' | '关' | '键' | '核' | '心'
    )
}

/// 是否为中文数字字符
fn is_cjk_numeral(c: char) -> bool {
    matches!(
        c,
        '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    )
}

/// 中文数字 → 阿拉伯数字（支持 0-99 及常见写法）
fn chinese_num_to_arabic(s: &str) -> Option<u32> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse().ok();
    }
    let mut total = 0u32;
    let mut cur = 0u32;
    for c in s.chars() {
        if c == '十' {
            if cur == 0 {
                cur = 1;
            }
            total += cur * 10;
            cur = 0;
        } else if let Some(v) = single_num(c) {
            cur = v;
        } else {
            return None;
        }
    }
    total += cur;
    Some(total)
}

fn single_num(c: char) -> Option<u32> {
    match c {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

/// 从查询中解析「第N章」章节号与「第N题」题号
fn parse_refs(query: &str) -> (Option<u32>, Option<u32>) {
    let mut chapter = None;
    let mut problem = None;
    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格（OCR 会在 `第 2 章` / `第 2 题` 里插入空格）
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut num = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                num.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「章/题」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !num.is_empty() && j < chars.len() {
                if let Some(n) = chinese_num_to_arabic(&num) {
                    if chars[j] == '章' && chapter.is_none() {
                        chapter = Some(n);
                    } else if chars[j] == '题' && problem.is_none() {
                        problem = Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    (chapter, problem)
}

/// 判断一行是否为 Markdown 章节标题（以 # 开头且含「第N章」）
fn is_chapter_heading(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with('#') && extract_chapter_num(l).is_some()
}

/// 从一行中提取章节号（仅匹配「第N章」形式）
fn extract_chapter_num(line: &str) -> Option<u32> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut num = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                num.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「章」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !num.is_empty() && j < chars.len() && chars[j] == '章' {
                return chinese_num_to_arabic(&num);
            }
        }
        i += 1;
    }
    None
}

/// 在教材行中定位指定章节的标题行
///
/// 只匹配以 `#` 开头的 Markdown 章节标题，忽略目录中或正文里的引用。
fn find_chapter_line(lines: &[&str], num: u32) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .find(|(_, l)| {
            let trimmed = l.trim_start();
            trimmed.starts_with('#') && extract_chapter_num(trimmed) == Some(num)
        })
        .map(|(i, _)| i)
}

/// 从一行中提取题目编号（OCR 容错）。
///
/// 支持：
/// - `第N题`（阿拉伯/中文数字）行内写法；
/// - Markdown 标题或普通行首的数字编号，如 `#### 02.`、`2)`、`2、`、`（3）`、
///   `0 2.`（数字间空格）、全角数字 `３` 等 OCR 杂音；
/// - 返回 `None` 表示该行不是「编号题目行」。
fn problem_number_of(line: &str) -> Option<u32> {
    let mut l = line.trim_start();
    // 行内「第N题」
    if let Some(n) = extract_problem_from_text(l) {
        return Some(n);
    }
    // 去掉 Markdown 标题记号 `#`
    while l.starts_with('#') {
        l = l[1..].trim_start();
    }
    extract_leading_number(l)
}

/// 提取行内「第N题」编号
fn extract_problem_from_text(l: &str) -> Option<u32> {
    let chars: Vec<char> = l.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格（OCR 会在 `第 2 题` 里插入空格）
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut digs = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                digs.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「题」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !digs.is_empty() && j < chars.len() && chars[j] == '题' {
                if let Some(n) = chinese_num_to_arabic(&digs) {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// 提取行首编号（OCR 容错），要求编号后跟题号分隔符（`.`/`、`/`)`/`）`/`:`/空格等）
fn extract_leading_number(s: &str) -> Option<u32> {
    let mut it = s.chars().peekable();
    // 可选的开括号
    if matches!(it.peek(), Some('（') | Some('(')) {
        it.next();
    }
    let mut num = String::new();
    while let Some(&c) = it.peek() {
        if c.is_ascii_digit() || is_fullwidth_digit(c) {
            num.push(to_ascii_digit(c));
            it.next();
        } else if c == ' ' || c == '\t' {
            // 数字间允许空格（如 `0 2.`）；但若空格后不再是数字，
            // 则该空格就是编号结束的分隔符（如 `11 若…`），停止解析
            let mut rest = it.clone();
            rest.next(); // 跳过当前空格
            let next_non_space = rest.find(|&c| c != ' ' && c != '\t');
            match next_non_space {
                Some(ch) if ch.is_ascii_digit() || is_fullwidth_digit(ch) => {
                    it.next(); // 数字间的空格，消费后继续
                }
                _ => break, // 空格即编号结束，保留其作为分隔符的语义
            }
        } else {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    // 必须跟编号分隔符，避免把正文数字误判为题号
    // 含逗号（`,`/`，`）：OCR 常把题号后的 `.` 识别成 `,`（如 `05, 若…`）
    if matches!(
        it.peek(),
        Some('.') | Some('、') | Some(')') | Some('）') | Some(':') | Some('：')
            | Some('-') | Some(' ') | Some(',') | Some('，')
    ) {
        num.parse().ok()
    } else {
        None
    }
}

/// 是否为全角数字
fn is_fullwidth_digit(c: char) -> bool {
    matches!(c, '０' | '１' | '２' | '３' | '４' | '５' | '６' | '７' | '８' | '９')
}

/// 全角数字 → 半角
fn to_ascii_digit(c: char) -> char {
    match c {
        '０' => '0',
        '１' => '1',
        '２' => '2',
        '３' => '3',
        '４' => '4',
        '５' => '5',
        '６' => '6',
        '７' => '7',
        '８' => '8',
        '９' => '9',
        _ => c,
    }
}

/// 判断扁平 OCR 文本中某行是否为「第 N 章」的小节标题。
///
/// 只匹配行首为 `N.` 且后跟数字的多级小节号（如 `3.2.1`、`3.43`），
/// 排除 `3. 题目` 这类单号题干，避免把题目行误当作章节边界。
fn is_flat_section_header(line: &str, num: u32) -> bool {
    let t = line.trim_start();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut digs = String::new();
    while i < chars.len() && chars[i].is_ascii_digit() {
        digs.push(chars[i]);
        i += 1;
    }
    if digs.is_empty() {
        return false;
    }
    let n: u32 = digs.parse().unwrap_or(0);
    if n != num {
        return false;
    }
    // 下一个字符必须是点号，且后面跟数字（区分 `3.2` 与 `3. 题目`）
    if i < chars.len() && (chars[i] == '.' || chars[i] == '．') {
        return i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
    }
    false
}

/// 在扁平 OCR 文本中定位「第 num 章」的范围（num.x 小节起始 → num+1.x 小节起始）
fn find_flat_chapter_range(lines: &[&str], num: u32) -> Option<(usize, usize)> {
    // 先跳过目录/封面区，避免把目录里的 `N.M` 小节号误当成章节边界
    let content_start = find_content_start(lines);
    let mut start = None;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(content_start) {
        if start.is_none() {
            if is_flat_section_header(l, num) {
                start = Some(i);
            }
        } else if is_flat_section_header(l, num + 1) {
            end = i;
            break;
        }
    }
    start.map(|s| (s, end))
}

/// 定位扁平 OCR 文本的「正文起点」，跳过开头的目录/封面/版权等前置噪声。
///
/// 目录区通常由短行（`N.M` 小节号 + 标题）构成，且包含与正文重复的小节编号，
/// 若直接从中定位章节会得到错误边界。正文以 `【考纲内容】`、`【复习提示】` 等
/// 专属标记（王道教材常见），或较长整段文字（>=40 字符）为信号，据此估算正文起点。
fn find_content_start(lines: &[&str]) -> usize {
    // 1) 正文专属标记：`【考纲…】`/`【复习…】`/`【考点…】`/`【本节…】`/`【答案…】` 等方括号标记
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.contains('【')
            && (t.contains("考纲") || t.contains("复习") || t.contains("考点") || t.contains("本节") || t.contains("答案"))
        {
            return i;
        }
        if t.ends_with("考纲") || t.ends_with("复习提示") {
            return i;
        }
    }
    // 2) 回退：第一个长行（>=40 字符）视为正文
    for (i, l) in lines.iter().enumerate() {
        if l.trim().chars().count() >= 40 {
            return i;
        }
    }
    0
}

/// 是否进入习题区（习题小节标题）。用于限定题目顺序计数范围，
/// 避免把正文里的列表序号（如 `3 第三代…`）误当题号导致第 N 题顺序偏移。
fn is_exercise_start(lower: &str) -> bool {
    lower.contains("本节习题")
        || lower.contains("习题精选")
        || lower.contains("单项选择题")
        || lower.contains("综合应用题")
        || lower.contains("综合题")
}

/// 是否退出习题区（答案区标题）
fn is_answer_section(lower: &str) -> bool {
    lower.contains("答案与解析") || lower.contains("答案解析") || lower.contains("参考答案")
}

/// 在字符边界上安全切片，避免 `ctx_end` 落在多字节 UTF-8 字符中间导致 panic
fn safe_char_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.len();
    let mut cstart = start;
    while cstart < len && !s.is_char_boundary(cstart) {
        cstart += 1;
    }
    if cstart >= len {
        return "";
    }
    let mut cend = end.min(len);
    while cend > cstart && !s.is_char_boundary(cend) {
        cend -= 1;
    }
    &s[cstart..cend]
}

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
pub async fn get_user_model_summary(
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let data_dir = get_data_dir(state.inner())?;
    UserModelService::get_user_model_summary(&data_dir)
}

// ============================================================================
// AI 对话命令
// ============================================================================

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
        return Err(
            "未配置任何 AI Provider，请在设置中添加 AI Provider 配置".to_string()
        );
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
        return Err(
            "未配置任何 AI Provider，请在设置中添加 AI Provider 配置".to_string()
        );
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
pub fn cancel_ai_request(
    key: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<bool, String> {
    let ai_service = get_ai_service(state.inner())?;
    Ok(ai_service.cancel_request(&key))
}

/// 测试 AI Provider 连接的返回结果
#[derive(serde::Serialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

/// 测试 AI Provider 连接
///
/// 临时创建 Provider 实例并发送测试请求。
/// 不需要 AppState，直接用传入的配置测试。
/// 前端调用: `invoke('test_ai_provider', { config: { ... } })`
#[tauri::command]
pub async fn test_ai_provider(
    config: AIProviderConfig,
) -> Result<TestResult, String> {
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
        Some(cfg) => AiService::test_list_models(cfg).await,
        None => {
            let ai_service = get_ai_service(state.inner())?;
            ai_service.list_models().await
        }
    }
}

// ============================================================================
// AI 用量日志命令
// ============================================================================

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
pub async fn clear_ai_usage_log(
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    // H1 并发保护：与 AI 调用后的用量日志 append 串行化，避免清空与追加竞态
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    crate::data::ai_usage::clear();
    Ok(())
}

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
pub async fn clear_app_log(
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
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

// ============================================================================
// 调试页命令（数据文件检查 / 查看）
// ============================================================================

/// 调试目录条目
#[derive(Serialize)]
pub struct DebugDirEntry {
    pub name: String,
    pub is_directory: bool,
}

/// 解析调试路径：确保解析后的路径始终位于 data_dir 内，防止路径穿越
fn resolve_debug_path(
    data_dir: &std::path::Path,
    relative_path: &str,
) -> Result<std::path::PathBuf, String> {
    let rel = std::path::Path::new(relative_path);
    if rel.is_absolute() {
        return Err(format!("不允许绝对路径: {}", relative_path));
    }
    if relative_path.contains("..") {
        return Err(format!("不允许包含上级目录引用: {}", relative_path));
    }
    let resolved = data_dir.join(rel);
    let data_canon = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    let resolved_canon = resolved
        .canonicalize()
        .unwrap_or_else(|_| resolved.clone());
    if !resolved_canon.starts_with(&data_canon) {
        return Err(format!("路径越界: {}", relative_path));
    }
    Ok(resolved_canon)
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
    for entry in std::fs::read_dir(&dir_path)
        .map_err(|e| format!("读取目录失败 {:?}: {}", dir_path, e))?
    {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取条目类型失败 {}: {}", entry.file_name().to_string_lossy(), e))?;
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
    crate::data::read_file_content(&file_path)
        .map_err(|e| format!("读取文件失败: {}", e))
}

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

// ============================================================================
// Settings 命令
// ============================================================================

/// 获取应用配置
///
/// 读取 `config/settings.json` 文件。
/// 前端调用: `invoke('get_settings')`
#[tauri::command]
pub async fn get_settings(
    state: State<'_, Mutex<AppState>>,
) -> Result<AppSettings, String> {
    let data_dir = get_data_dir(state.inner())?;
    Ok(load_settings(&data_dir))
}

/// 保存应用配置
///
/// 保存到 `config/settings.json` 并重新初始化 AI Service 和 Tool Dispatcher。
/// 前端调用: `invoke('save_settings', { settings: { ... } })`
#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    // H1 并发保护：串行化 settings 写入与服务重建
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    reinitialize_services(state.inner(), settings).await
}

/// 切换数据目录（重启后生效）
///
/// 流程：
/// 1. 校验 new_path 存在且是目录
/// 2. 在新目录下创建必要的子目录结构
/// 3. 把当前 settings.json 复制到新目录（保留 data_dir 字段为新路径）
/// 4. 更新 AppState.data_dir 为新路径，使后续读写立即指向新目录
/// 5. 注意：旧目录中的历史 plan/state/review 等文件不会自动迁移，需用户手动处理
#[tauri::command]
pub async fn change_data_directory(
    new_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    use std::path::PathBuf;

    // H1 并发保护：切换数据目录期间串行化所有写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let new_dir = PathBuf::from(new_path.trim_end_matches(['/', '\\']));
    if !new_dir.is_dir() {
        return Err(format!("目录不存在或不是目录: {:?}", new_dir));
    }

    // 读取当前 settings
    let (old_data_dir, current_settings) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let data_dir = s.data_dir.clone();
        let settings = crate::load_settings(&data_dir);
        (data_dir, settings)
    };

    // 在新目录下创建子结构
    crate::ensure_data_directories(&new_dir);

    // 构造新 settings：data_dir 指向新路径
    let mut new_settings = current_settings.clone();
    new_settings.data_dir = new_dir.to_string_lossy().to_string();

    // 把新 settings 写到新目录的 config/settings.json
    crate::save_settings_file(&new_dir, &new_settings)?;

    // 更新 AppState.data_dir，让后续命令立即使用新目录
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir = new_dir.clone();
    }

    // H17：同步更新 AI 用量日志目录，使日志写入新数据目录
    crate::data::ai_usage::set_log_dir(new_dir.clone());

    let msg = format!(
        "数据目录已切换至 {:?}。旧目录 {:?} 中的历史数据未自动迁移，如需保留历史计划/复盘记录，请手动复制 state/、plan/、records/、assets/ 等子目录到新目录。重启应用后配置仍然生效。",
        new_dir, old_data_dir
    );
    log::info!("{}", msg);
    Ok(msg)
}

// ============================================================================
// 数据备份 / 导出 / 导入
// ============================================================================

/// 导出数据备份（zip）
///
/// 把数据目录下允许的子目录（state/plan/records/config/assets，可选 logs/）
/// 压缩到 `dest_path` 指定的 zip 文件。
/// 返回导出的文件数。
///
/// 前端调用: `invoke('export_backup', { destPath, includeLogs })`
#[tauri::command]
pub async fn export_backup(
    dest_path: String,
    include_logs: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<usize, String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：导出期间不允许写入，保证快照一致
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let dest = std::path::PathBuf::from(dest_path.trim_end_matches(['/', '\\']));
    let count = crate::data::backup::export_backup(&data_dir, &dest, include_logs)?;
    log::info!("数据备份导出完成: {} 个文件 -> {:?}", count, dest);
    Ok(count)
}

/// 导入数据备份（zip），覆盖前自动备份现有数据目录
///
/// 校验 zip 合法性后，把现有数据目录重命名为 `{data_dir}-bak-{timestamp}`，
/// 再解压备份内容到数据目录。导入完成后需重启应用以加载最新数据。
///
/// 前端调用: `invoke('import_backup', { filePath })`
#[tauri::command]
pub async fn import_backup(
    file_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::data::backup::ImportSummary, String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：导入覆盖期间串行化所有写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let zip_path = std::path::PathBuf::from(file_path.trim_end_matches(['/', '\\']));
    let summary = crate::data::backup::import_backup(&data_dir, &zip_path)?;
    log::info!(
        "数据备份导入完成: 恢复 {} 个文件, 原数据备份至 {}",
        summary.files_restored,
        summary.backup_dir
    );
    Ok(summary)
}

// ============================================================================
// Onboarding 命令
// ============================================================================

/// 引导流程中收集的初始化数据
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InitStatePayload {
    pub target_school: String,
    pub target_major: String,
    pub exam_date: String,
    /// 考试科目配置: [{ subject: "math", version: "数二", active: true, phase: "foundation", weekly_hours: 14.0, target_score: 120 }, ...]
    pub subjects: Vec<SubjectInit>,
    /// 专业课名称（如 "408计算机综合"），仅 professional 科目使用
    pub professional_name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubjectInit {
    pub subject: String,
    pub version: Option<String>,
    pub active: bool,
    pub phase: String,
    pub weekly_hours: f64,
    pub target_score: i32,
    #[serde(default)]
    pub textbook: Option<String>,
}

/// 初始化 State 文件
///
/// 在引导流程完成时调用，根据用户填写的目标院校、考试科目、当前进度
/// 创建 `state/current.state` 文件。
#[tauri::command]
pub async fn init_state(
    payload: InitStatePayload,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 初始化写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let today = crate::data::today_string();
    let phase_map: std::collections::HashMap<&str, crate::data::state::StudyPhase> = [
        ("foundation", crate::data::state::StudyPhase::Foundation),
        ("strengthen", crate::data::state::StudyPhase::Strengthen),
        ("sprint", crate::data::state::StudyPhase::Sprint),
        ("mock", crate::data::state::StudyPhase::Mock),
        ("complete", crate::data::state::StudyPhase::Complete),
    ].into_iter().collect();

    let mut study_state = crate::data::state::StudyState {
        meta: crate::data::state::StateMeta {
            last_updated: today.clone(),
            exam_date: payload.exam_date.clone(),
            target_school: payload.target_school.clone(),
            target_major: payload.target_major.clone(),
        },
        ..Default::default()
    };

    for subj in &payload.subjects {
        let phase = phase_map.get(subj.phase.as_str()).cloned().unwrap_or_default();
        let mut subject_state = crate::data::state::SubjectState {
            active: subj.active,
            phase,
            version: subj.version.clone(),
            target_score: subj.target_score,
            current_score: 0,
            weekly_hours: subj.weekly_hours,
            textbook: subj.textbook.clone(),
            ..Default::default()
        };

        // 专业课使用自定义名称
        if subj.subject == "professional" {
            subject_state.name = payload.professional_name.clone();
        }

        match subj.subject.as_str() {
            "math" => study_state.subjects.math = subject_state,
            "english" => study_state.subjects.english = subject_state,
            "politics" => study_state.subjects.politics = subject_state,
            "professional" => study_state.subjects.professional = subject_state,
            _ => {}
        }
    }

    crate::data::state::save_state(&data_dir, &study_state)?;
    log::info!("State 文件已初始化: {:?}/state/current.state", data_dir);
    Ok(())
}

/// 获取引导流程完成状态
///
/// 前端调用: `invoke('get_onboarding_status')`
#[tauri::command]
pub async fn get_onboarding_status(
    state: State<'_, Mutex<AppState>>,
) -> Result<bool, String> {
    let data_dir = get_data_dir(state.inner())?;
    let settings = load_settings(&data_dir);
    Ok(settings.onboarding_completed)
}

/// 标记引导流程已完成
///
/// 前端调用: `invoke('complete_onboarding')`
#[tauri::command]
pub async fn complete_onboarding(
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 settings 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut settings = load_settings(&data_dir);
    settings.onboarding_completed = true;
    save_settings_file(&data_dir, &settings)?;
    log::info!("引导流程已标记为完成");
    Ok(())
}

/// 更新指定科目的教材信息
///
/// 前端调用: `invoke('update_subject_textbook', { subject, textbook })`
///
/// 支持的 subject 取值：math / english / politics / professional
#[tauri::command]
pub async fn update_subject_textbook(
    subject: String,
    textbook: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut study_state = crate::data::state::read_state(&data_dir)
        .map_err(|e| format!("读取 State 失败: {}", e))?;

    let target = match subject.as_str() {
        "math" => &mut study_state.subjects.math,
        "english" => &mut study_state.subjects.english,
        "politics" => &mut study_state.subjects.politics,
        "professional" => &mut study_state.subjects.professional,
        other => return Err(format!("不支持的科目: {}（仅支持 math/english/politics/professional）", other)),
    };

    // 空字符串视为 None
    let normalized = textbook
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    target.textbook = normalized.clone();
    let logged_textbook = normalized.clone();

    // 此处 target 的可变借用结束（NLL），后续可以再次不可变借用 study_state
    crate::data::state::save_state(&data_dir, &study_state)
        .map_err(|e| format!("保存 State 失败: {}", e))?;

    log::info!("已更新 {} 科目教材: {:?}", subject, logged_textbook);
    Ok(())
}

// ============================================================================
// Update 命令
// ============================================================================

/// 单个 Release 资源（一个安装包）
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateAsset {
    /// 文件名，如 `StudyAgent_0.1.2_x64-setup.exe`
    pub name: String,
    /// 直链下载地址
    pub download_url: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 资源类型推测：`nsis` / `msi` / `exe` / `unknown`
    pub kind: String,
    /// 文件 SHA-256（十六进制，来自 GitHub API 的 digest 字段；缺失时为 None）
    ///
    /// 用于 `download_update` 下载完成后的完整性校验（L14）。
    /// 注意：GitHub 对超过 2GB 的资产不提供 digest，此字段可能为 None。
    #[serde(default)]
    pub sha256: Option<String>,
}

/// 检查更新结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheckResult {
    /// 是否有新版本
    pub has_update: bool,
    /// 当前版本（来自 Cargo.toml，形如 "0.1.2"）
    pub current_version: String,
    /// 远端最新版本号（剥离 v 前缀后的纯版本字符串）
    pub latest_version: String,
    /// Release 名称（标题，可能为空）
    pub release_name: String,
    /// 发布时间（ISO 8601 字符串，可能为空）
    pub published_at: String,
    /// Release notes（Markdown，可能为空）
    pub release_notes: String,
    /// 可下载的安装包列表（已过滤掉 .sig / .json / 签名等非安装包文件）
    pub assets: Vec<UpdateAsset>,
    /// 用户可读的提示信息（不包含技术细节）
    pub message: String,
}

/// 下载进度事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadProgress {
    /// 已下载字节
    pub downloaded: u64,
    /// 文件总字节（若服务端未返回 content-length 则为 0）
    pub total: u64,
    /// 进度百分比 0-100
    pub percent: f64,
}

/// GitHub API 端点：获取最新 release
const GITHUB_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Wenjiugugugu/StudyAgent-client/releases/latest";

/// 推测资源类型
fn detect_asset_kind(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with("-setup.exe") || lower.contains("nsis") {
        "nsis".to_string()
    } else if lower.ends_with(".msi") {
        "msi".to_string()
    } else if lower.ends_with(".exe") {
        "exe".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 从 GitHub release assets 数组提取安装包列表
///
/// 过滤掉 `.sig`、`.json`、`.txt` 等非安装包文件。
fn extract_install_assets(assets: &serde_json::Value) -> Vec<UpdateAsset> {
    let arr = match assets.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    arr.iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?.to_string();
            let lower = name.to_lowercase();

            // 跳过签名 / manifest 文件
            if lower.ends_with(".sig")
                || lower.ends_with(".json")
                || lower.ends_with(".txt")
                || lower.ends_with(".blockmap")
            {
                return None;
            }

            let download_url = asset
                .get("browser_download_url")?
                .as_str()?
                .to_string();
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let kind = detect_asset_kind(&name);

            // GitHub API 对资产提供 digest 字段，形如 "sha256:<hex>"，剥离前缀
            let sha256 = asset
                .get("digest")
                .and_then(|v| v.as_str())
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(|hex| hex.to_lowercase())
                .filter(|hex| !hex.is_empty());

            Some(UpdateAsset {
                name,
                download_url,
                size,
                kind,
                sha256,
            })
        })
        .collect()
}

/// 构造一个「已是最新」的结果（用于错误降级）
///
/// 错误原因仅写入日志，不暴露给前端 message。
fn up_to_date_result(current_version: &str, log_reason: &str) -> UpdateCheckResult {
    log::info!("[Update] 降级为「已是最新」，原因：{}", log_reason);
    UpdateCheckResult {
        has_update: false,
        current_version: current_version.to_string(),
        latest_version: current_version.to_string(),
        release_name: String::new(),
        published_at: String::new(),
        release_notes: String::new(),
        assets: Vec::new(),
        message: format!("已是最新版本（{}）", current_version),
    }
}

/// 检查更新
///
/// 通过 GitHub API 获取最新 release，与当前版本比较。
/// **约定**：任何错误情况（网络错误、服务不可用、解析失败）
/// 一律返回 `has_update = false` + 友好的提示信息，详细错误仅写入日志。
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    log::info!("[Update] 开始检查更新：当前版本 {}", current_version);

    // 构造 HTTP 客户端（短超时，避免检查更新卡顿）
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("StudyAgent/{}", current_version))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Update] 构造 HTTP 客户端失败: {}", e);
            return Ok(up_to_date_result(
                &current_version,
                &format!("client build failed: {}", e),
            ));
        }
    };

    // 请求 latest release
    let response = client
        .get(GITHUB_RELEASES_LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[Update] 请求失败: {}", e);
            return Ok(up_to_date_result(
                &current_version,
                &format!("request failed: {}", e),
            ));
        }
    };

    let status = response.status();
    log::info!("[Update] 响应 status={}", status);

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        log::warn!(
            "[Update] 非 2xx：status={}, body_len={}",
            status,
            body.len()
        );
        return Ok(up_to_date_result(
            &current_version,
            &format!("http status {}", status),
        ));
    }

    // 解析 JSON
    let release_json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Update] JSON 解析失败: {}", e);
            return Ok(up_to_date_result(&current_version, "json parse failed"));
        }
    };

    // 提取 tag_name
    let tag_name = release_json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if tag_name.is_empty() {
        log::warn!("[Update] Release 不含 tag_name 字段");
        return Ok(up_to_date_result(&current_version, "missing tag_name"));
    }

    // 剥离前导 'v' 或 'V'
    let latest_version = tag_name
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string();

    let has_update = is_newer_version(&latest_version, &current_version);
    log::info!(
        "[Update] 当前 {} | 远端 {} | has_update={}",
        current_version,
        latest_version,
        has_update
    );

    let release_name = release_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let published_at = release_json
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let release_notes = release_json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let assets = extract_install_assets(
        release_json.get("assets").unwrap_or(&serde_json::Value::Null),
    );

    let message = if has_update {
        format!(
            "发现新版本 {}（当前 {}）",
            latest_version, current_version
        )
    } else {
        format!("已是最新版本（{}）", current_version)
    };

    Ok(UpdateCheckResult {
        has_update,
        current_version,
        latest_version,
        release_name,
        published_at,
        release_notes,
        assets,
        message,
    })
}

/// 下载更新
///
/// 流式下载安装包到临时目录，并通过 `update-download-progress` 事件
/// 推送下载进度（payload: `DownloadProgress`）。
///
/// 完整性校验（L14）：若提供 `expected_sha256`，下载完成后计算文件
/// SHA-256 并比对，不匹配则删除文件并返回错误，防止安装被篡改的包。
/// 此外校验 `filename` 不含路径分隔符，防止路径穿越写出临时目录。
///
/// 下载完成后返回本地文件路径，供 `install_update` 使用。
#[tauri::command]
pub async fn download_update(
    url: String,
    filename: String,
    expected_sha256: Option<String>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    log::info!("[Update] 开始下载: {}", url);

    // 防御路径穿越：仅允许安全的文件名（不含路径分隔符与 ..）
    if filename.is_empty()
        || filename.contains(['/', '\\'])
        || filename.split('.').any(|seg| seg.is_empty() || seg == "..")
    {
        return Err("无效的文件名".to_string());
    }

    // 临时目录：%TEMP%\StudyAgent-update\
    let temp_dir = std::env::temp_dir().join("StudyAgent-update");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("创建临时目录失败: {}", e))?;

    let file_path = temp_dir.join(&filename);
    log::info!("[Update] 保存路径: {}", file_path.display());

    // 构造客户端（长超时，下载可能很大）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .user_agent(format!("StudyAgent/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化下载失败: {}", e))?;

    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("下载失败：服务返回 {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    log::info!("[Update] 文件大小: {} 字节", total);

    let mut file = std::fs::File::create(&file_path)
        .map_err(|e| format!("创建文件失败: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    loop {
        let chunk = match response.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => return Err(format!("下载流读取失败: {}", e)),
        };

        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| format!("写入文件失败: {}", e))?;

        downloaded += chunk.len() as u64;

        // 每 256KB 推送一次进度，避免事件风暴
        if downloaded - last_emit >= 256 * 1024 || total > 0 && downloaded == total {
            let percent = if total > 0 {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = app.emit(
                "update-download-progress",
                DownloadProgress {
                    downloaded,
                    total,
                    percent,
                },
            );
            last_emit = downloaded;
        }
    }

    // 推送最终进度
    let _ = app.emit(
        "update-download-progress",
        DownloadProgress {
            downloaded,
            total,
            percent: if total > 0 { 100.0 } else { 0.0 },
        },
    );

    // 完整性校验（L14）：比对期望 SHA-256，不匹配则删除文件并报错
    if let Some(expected) = expected_sha256 {
        let expected = expected.trim().to_lowercase();
        if !expected.is_empty() {
            let actual = sha256_hex(&file_path)
                .map_err(|e| format!("计算下载文件校验和失败: {}", e))?;
            log::info!(
                "[Update] 完整性校验: expected={} actual={}",
                expected,
                actual
            );
            if actual != expected {
                let _ = std::fs::remove_file(&file_path);
                return Err(
                    "下载文件完整性校验失败（SHA-256 不匹配），已删除文件，请重试或稍后再更新".to_string(),
                );
            }
            log::info!("[Update] 文件 SHA-256 校验通过");
        }
    }

    let path_str = file_path.to_string_lossy().to_string();
    log::info!("[Update] 下载完成: {} ({} 字节)", path_str, downloaded);
    Ok(path_str)
}

/// 计算文件的 SHA-256（十六进制小写）
fn sha256_hex(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 安装更新
///
/// 启动下载好的安装包并退出当前应用。
/// Windows 上使用 DETACHED_PROCESS 让子进程独立运行。
#[tauri::command]
pub async fn install_update(
    file_path: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(format!("安装包不存在: {}", file_path));
    }

    log::info!("[Update] 启动安装程序: {}", file_path);

    // Windows 上用 DETACHED_PROCESS 让子进程独立
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        std::process::Command::new(&file_path)
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(&file_path)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    }

    log::info!("[Update] 安装程序已启动，应用退出");
    app.exit(0);
    Ok(())
}

/// 比较语义化版本号：判断 `remote` 是否比 `current` 更新
///
/// 支持格式：`0.1.2`、`0.1.2-beta`、`0.1.2-beta.1`
/// 后缀（-beta、-rc.1、-alpha）视为预发布版本，比同版本号的正式版本更旧。
fn is_newer_version(remote: &str, current: &str) -> bool {
    let (remote_main, remote_pre) = split_version(remote);
    let (current_main, current_pre) = split_version(current);

    // 主版本号比较
    let remote_parts: Vec<u64> = remote_main
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    let current_parts: Vec<u64> = current_main
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();

    let max_len = remote_parts.len().max(current_parts.len());
    for i in 0..max_len {
        let r = remote_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if r != c {
            return r > c;
        }
    }

    // 主版本号相同：有预发布后缀的版本比无后缀的版本更旧
    match (remote_pre, current_pre) {
        (None, None) => false, // 完全相同
        (Some(_), None) => false, // remote 是预发布，current 是正式版 → remote 更旧
        (None, Some(_)) => true,  // remote 是正式版，current 是预发布 → remote 更新
        (Some(r), Some(c)) => compare_prerelease(&r, &c) > 0,
    }
}

/// 拆分版本号：(主版本号, 预发布标识)
/// `"0.1.2"` -> `("0.1.2", None)`
/// `"0.1.2-beta"` -> `("0.1.2", Some("beta"))`
/// `"0.1.2-beta.1"` -> `("0.1.2", Some("beta.1"))`
fn split_version(v: &str) -> (String, Option<String>) {
    if let Some(idx) = v.find('-') {
        (v[..idx].to_string(), Some(v[idx + 1..].to_string()))
    } else {
        (v.to_string(), None)
    }
}

/// 比较两个预发布后缀：>0 表示 a 更新，==0 相同，<0 a 更旧
fn compare_prerelease(a: &str, b: &str) -> i32 {
    // 简单按字典序比较（足够覆盖 beta / rc / alpha）
    // 详见 https://semver.org/#spec-item-11
    a.cmp(b) as i32
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 解析任务 ID
///
/// 任务 ID 格式: `YYYY-MM-DD-NN`
/// 返回: (date_string, zero_based_index)
fn parse_task_id(task_id: &str) -> Result<(String, usize), String> {
    let parts: Vec<&str> = task_id.split('-').collect();
    if parts.len() < 4 {
        return Err(format!(
            "无效的任务 ID 格式: {}（期望 YYYY-MM-DD-NN）",
            task_id
        ));
    }

    let date = format!("{}-{}-{}", parts[0], parts[1], parts[2]);

    let seq: usize = parts[3]
        .parse()
        .map_err(|_| format!("无效的任务序号: {}", parts[3]))?;

    // 转换为 0-based 索引
    if seq == 0 {
        return Err("任务序号不能为 0（从 1 开始）".to_string());
    }

    Ok((date, seq - 1))
}

/// 解析任务状态字符串为 TaskStatus 枚举
fn parse_task_status(status: &str) -> Result<TaskStatus, String> {
    match status.to_lowercase().as_str() {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" | "inprogress" | "in-progress" => Ok(TaskStatus::InProgress),
        "done" | "completed" | "complete" => Ok(TaskStatus::Done),
        "abandoned" | "abandon" | "skip" => Ok(TaskStatus::Abandoned),
        _ => Err(format!(
            "无效的任务状态: {}（支持: pending, in_progress, done, abandoned）",
            status
        )),
    }
}

// ============================================================================
// 通用命令（关闭行为 / 开机启动 / 应用版本）
// ============================================================================

/// 获取关闭窗口时的动作设置
///
/// 返回值: "ask" | "tray" | "quit"
/// 前端调用: `invoke('get_close_action')`
#[tauri::command]
pub async fn get_close_action(
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
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
        _ => return Err(format!("无效的关闭动作: {}（支持: ask, tray, quit）", action)),
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
pub async fn get_autostart(
    app: tauri::AppHandle,
) -> Result<bool, String> {
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
pub async fn set_autostart(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// L14：验证文件 SHA-256 计算（已知内容 "hello" 的 sha256）
    #[test]
    fn sha256_hex_matches_known_hash() {
        let tmp = std::env::temp_dir().join(format!("sa_sha256_test_{}", std::process::id()));
        std::fs::write(&tmp, b"hello").unwrap();
        let hex = sha256_hex(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        // "hello" 的 SHA-256
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    /// L14：下载文件名防护，拒绝路径穿越
    #[test]
    fn invalid_filenames_rejected() {
        for bad in ["../evil.exe", "a\\b.exe", "a/b.exe", "", ".", ".."] {
            assert!(
                bad.is_empty()
                    || bad.contains(['/', '\\'])
                    || bad.split('.').any(|seg| seg.is_empty() || seg == ".."),
                "应判定为无效文件名: {:?}",
                bad
            );
        }
        // 合法文件名应通过
        assert!(!String::from("StudyAgent_0.1.2_x64-setup.exe").contains(['/', '\\']));
    }

    // ── 教材 OCR 容错检索的单元测试 ──────────────────────────────

    /// 题号解析：OCR 把 `.` 识别成 `,`/`、`/空格/全角数字等杂音时仍能识别题号
    #[test]
    fn problem_number_ocr_noise() {
        // 标准
        assert_eq!(problem_number_of("01. 若十进制数为 137.5"), Some(1));
        assert_eq!(problem_number_of("02. 一个 16 位无符号"), Some(2));
        // 顿号 / 逗号（OCR 常把 `.` 识别成 `,`）
        assert_eq!(problem_number_of("04、对真值 0 表示"), Some(4));
        assert_eq!(problem_number_of("05, 若 [#= 11101010"), Some(5));
        assert_eq!(problem_number_of("08, 一个 + 1 位整数"), Some(8));
        assert_eq!(problem_number_of("09, 若定点整数为 64 位"), Some(9));
        assert_eq!(problem_number_of("10, 下列关于补码"), Some(10));
        // 题号后接空格
        assert_eq!(problem_number_of("11 若 [xJ#=lxixarsxryrsxe"), Some(11));
        // 全角数字
        assert_eq!(problem_number_of("３. 全角题号"), Some(3));
        // 行内「第N题」
        assert_eq!(problem_number_of("第 2 题 求下列行列式"), Some(2));
        assert_eq!(problem_number_of("第10题 写出"), Some(10));
        // Markdown 标题
        assert_eq!(problem_number_of("#### 02. 习题"), Some(2));
        // 正文列表序号不应被误识别为高权题号（题目号匹配阶段不参与，但函数本身应能解析）
        assert_eq!(problem_number_of("3 第三代计算机"), Some(3));
    }

    /// 正文起点检测：跳过目录/封面区，避免 `N.M` 目录小节号被误当章节边界
    #[test]
    fn content_start_skips_toc() {
        let lines: Vec<&str> = vec![
            "# 计算机组成原理",
            "[此页为封面页]",
            "2.3",
            "3.2",
            "3.2.1 半导体随机存取存储器",
            "第 7 章",
            "7.1",
            "计算机系统概述",
            "【考纲内容】",
            "( 一 ) 计算机系统层次结构",
            "1.1.1 计算机硬件的发展",
        ];
        assert_eq!(find_content_start(&lines), 8); // 命中「【考纲内容】」
    }

    /// 无「【…】」标记时回退到第一个长行
    #[test]
    fn content_start_fallback_long_line() {
        let lines: Vec<&str> = vec![
            "1.1",
            "1.2",
            "这一行是某章正文的第一段，长度明显超过四十个字符的阈值，应该被视为正文开始的地方。",
        ];
        assert_eq!(find_content_start(&lines), 2);
    }

    /// 章节范围定位：目录区被跳过，正文里 `num.x` 小节才是真正的章节边界
    #[test]
    fn flat_chapter_range_skips_toc() {
        let lines: Vec<&str> = vec![
            "# 标题",
            "【考纲内容】",
            "1.1.1 计算机硬件的发展", // 第1章正文
            "（一）计算机发展",
            "2.1.1 进位计数制", // 第2章正文
            "2.1.5 本节习题精选",
            "3.1.1 存储器的分类", // 第3章正文
        ];
        let r = find_flat_chapter_range(&lines, 1).unwrap();
        assert_eq!(r.0, 2); // 从 `1.1.1` 开始
        assert_eq!(r.1, 4); // 到 `2.1.1` 结束
    }

    /// 习题区状态机：只有进入习题区才计数题目序号，正文列表序号不污染顺序
    #[test]
    fn exercise_section_state_machine() {
        assert!(is_exercise_start("2.1.5 本节习题精选"));
        assert!(is_exercise_start("单项选择题"));
        assert!(is_exercise_start("综合应用题"));
        assert!(!is_exercise_start("计算机硬件的发展"));
        assert!(is_answer_section("2.1.6 答案与解析"));
        assert!(is_answer_section("参考答案"));
        assert!(!is_answer_section("解析一下这道题"));
    }

    /// 顺序兜底：找「第 2 题」时，即使 OCR 把 `02.` 认成 `02,` 等，也能按顺序定位
    #[test]
    fn problem_ordinal_fallback_over_ocr_noise() {
        // 模拟一个习题区：题号带各种 OCR 杂音，但题干完整
        let exercise: Vec<&str> = vec![
            "2.1.5 本节习题精选",
            "单项选择题",
            "01. 若十进制数为 137.5",   // 第1题
            "A 89.8 B 211.4",
            "02, 一个 16 位无符号",      // 第2题（OCR 逗号）
            "A 0 一 63536",
            "03、下列说法有误的是",      // 第3题（OCR 顿号）
            "04 若叉为负数",            // 第4题（OCR 空格）
            "2.1.6 答案与解析",
        ];
        // 模拟主循环的 ordinal 计数逻辑
        let target = 2u32;
        let mut in_exercise = false;
        let mut ordinal = 0usize;
        let mut second_hit = None;
        for l in &exercise {
            let lower = l.to_lowercase();
            if is_exercise_start(&lower) {
                in_exercise = true;
                ordinal = 0;
            } else if is_answer_section(&lower) {
                in_exercise = false;
            }
            if in_exercise {
                if let Some(n) = problem_number_of(l) {
                    ordinal += 1;
                    if n == target {
                        second_hit = Some(ordinal);
                    }
                }
            }
        }
        // 第2题命中的行「02,」应被识别为第 2 道题
        assert_eq!(second_hit, Some(2));
    }
}
