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

/// 读取今日计划
///
/// 读取今天的 `plan/YYYY-MM-DD_day.json` 文件。
/// 前端调用: `invoke('get_today_plan')`
#[tauri::command]
pub async fn get_today_plan(state: State<'_, Mutex<AppState>>) -> Result<DailyPlanFile, String> {
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
pub async fn list_plan_dates(state: State<'_, Mutex<AppState>>) -> Result<Vec<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::plan::list_daily_plan_dates(&data_dir)
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
    let mut rest_days_from_week_plans: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // 收集所有周计划中手动添加的特殊情况排除日：date -> (type, note)
    let mut excluded_days_from_week_plans: std::collections::HashMap<
        String,
        (String, Option<String>),
    > = std::collections::HashMap::new();
    if let Ok(week_dates) = crate::data::plan::list_week_plan_dates(&data_dir) {
        for iso_week in &week_dates {
            if let Ok(wp) = crate::data::plan::read_week_plan(&data_dir, iso_week) {
                for day in &wp.data.days {
                    if day.is_rest_day {
                        rest_days_from_week_plans.insert(day.date.clone());
                    }
                }
                for ex in &wp.data.excluded_days {
                    excluded_days_from_week_plans
                        .insert(ex.date.clone(), (ex.reason_type.clone(), ex.note.clone()));
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
            .map(crate::data::records::review_actual_hours)
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
            .map(crate::data::records::review_actual_hours)
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
            "未配置 AI Provider，无法生成计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let result = planner.generate_daily_plan(&data_dir, &date).await;

    // 同步滴答：日计划生成后后台按日对账（H1：不阻塞主流程；后台任务自行持 io_lock 串行化落盘）
    if result.is_ok() {
        let io_lock = crate::get_io_lock(state.inner())?;
        crate::sync::dida::spawn_reconcile_day(io_lock, data_dir, date);
    }
    result
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
            "未配置 AI Provider，无法生成周计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let result = planner
        .generate_week_plan(
            &data_dir,
            &week_start,
            &excluded_days,
            workload_adjustment.as_ref(),
        )
        .await;

    // 同步滴答：周计划生成会逐天生成日计划，后台对本周 7 天按日对账（H1：不阻塞主流程）
    if result.is_ok() {
        let mut dates = Vec::with_capacity(7);
        for i in 0..7i64 {
            if let Ok(d) = crate::data::add_days(&week_start, i) {
                dates.push(d);
            }
        }
        let io_lock = crate::get_io_lock(state.inner())?;
        crate::sync::dida::spawn_reconcile_days(io_lock, data_dir, dates);
    }
    result
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
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let (regenerated, affected_dates, used_fallback) = planner
        .regenerate_after_exclusion(&data_dir, &week_start, excluded_day)
        .await?;

    // 同步滴答：对重排影响的日期后台按日对账（H1：不阻塞主流程）
    let io_lock = crate::get_io_lock(state.inner())?;
    crate::sync::dida::spawn_reconcile_days(io_lock, data_dir, affected_dates.clone());

    Ok(RegenerateResult {
        regenerated,
        affected_dates,
        used_fallback,
        // 排除日重排不涉及计划外进度，一致性警告为空
        consistency_warnings: Vec::new(),
        // 排除日重排暂不提供逐日变更明细
        changes: Vec::new(),
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
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    let (regenerated, affected_dates, used_fallback, consistency_warnings, changes) = planner
        .regenerate_remaining_days_after_review(&data_dir, &review_date)
        .await?;

    // 同步滴答：对重排影响的日期按日对账（失败仅记录）
    for d in &affected_dates {
        if let Err(e) = crate::sync::dida::reconcile_day(&data_dir, d).await {
            log::warn!("[dida] 复盘重排后同步 {} 失败: {}", d, e);
        }
    }

    Ok(RegenerateResult {
        regenerated,
        affected_dates,
        used_fallback,
        consistency_warnings,
        changes,
    })
}
