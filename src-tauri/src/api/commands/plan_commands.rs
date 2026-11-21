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
use crate::core::plan_adjustment::{self, AdjustmentOutcome, AdjustmentPreview, AdjustmentStatus};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{UserCapability, UserObservation};
use crate::data::plan::{
    iso_week_string, DailyPlanFile, ExcludedDay, PlanTask, WeekPlanFile, WorkloadAdjustment,
};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::data::state::{SubjectKey, TaskPriority, TaskSource};
use crate::tools::dispatcher::{execute_builtin_tool, is_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    get_ai_service, get_data_dir, get_data_dir_and_ai, get_data_dir_and_dispatcher,
    get_tool_dispatcher, load_settings, reinitialize_services, save_settings_file, AppSettings,
    AppState,
};

use super::legacy::*;

/// 今日有日计划且已到学习开始时间后，昨天才进入默认标记。
/// 今日尚无日计划时保留昨天的补复盘窗口；更早的缺失记录仍正常补记。
fn default_review_cutoff(data_dir: &std::path::Path) -> Result<String, String> {
    let today = crate::data::today_string();
    let now_hm = chrono::Local::now().format("%H:%M").to_string();
    default_review_cutoff_at(data_dir, &today, &now_hm)
}

fn default_review_cutoff_at(
    data_dir: &std::path::Path,
    today: &str,
    now_hm: &str,
) -> Result<String, String> {
    let yesterday = crate::data::add_days(today, -1)?;
    let settings = crate::load_settings(data_dir);
    let start_time = settings
        .study_schedule
        .get("start_time")
        .and_then(|value| value.as_str())
        .filter(|value| {
            let parts: Vec<_> = value.split(':').collect();
            parts.len() == 2
                && parts[0].parse::<u8>().is_ok_and(|hour| hour < 24)
                && parts[1].parse::<u8>().is_ok_and(|minute| minute < 60)
        })
        .unwrap_or("09:00");
    let today_plan_exists = crate::data::plan::daily_plan_path(data_dir, today)
        .try_exists()
        .map_err(|error| format!("检查今日日计划失败: {error}"))?;
    if now_hm < start_time || !today_plan_exists {
        crate::data::add_days(&yesterday, -1)
    } else {
        Ok(yesterday)
    }
}

fn backfill_due_default_reviews(data_dir: &std::path::Path) -> Result<(), String> {
    let cutoff = default_review_cutoff(data_dir)?;
    let created = crate::data::records::backfill_default_reviews(data_dir, &cutoff)?;
    if !created.is_empty() {
        log::info!("已生成系统默认复盘: {}", created.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod backfill_window_tests {
    use super::*;

    #[test]
    fn missing_today_plan_preserves_yesterday_until_a_plan_exists_after_start() {
        let dir = std::env::temp_dir().join(format!(
            "studyagent-backfill-window-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let today = "2026-10-12"; // 周一：尚未生成新一周计划
        for date in ["2026-10-10", "2026-10-11"] {
            let plan = DailyPlanFile {
                meta: crate::data::plan::DailyPlanMeta {
                    date: date.to_string(),
                    ..Default::default()
                },
                data: crate::data::plan::DailyPlanData {
                    tasks: vec![PlanTask {
                        id: format!("{date}-01"),
                        title: "学习任务".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            };
            crate::data::plan::save_daily_plan(&dir, &plan).unwrap();
        }

        let cutoff = default_review_cutoff_at(&dir, today, "12:00").unwrap();
        assert_eq!(cutoff, "2026-10-10");
        let created = crate::data::records::backfill_default_reviews(&dir, &cutoff).unwrap();
        assert_eq!(created, vec!["2026-10-10"]);
        assert!(!crate::data::records::review_file_path(&dir, "2026-10-11").exists());

        let mut today_plan = crate::data::plan::read_daily_plan(&dir, "2026-10-11").unwrap();
        today_plan.meta.date = today.to_string();
        crate::data::plan::save_daily_plan(&dir, &today_plan).unwrap();
        // 已有日计划但未到开始时间，仍保留补复盘窗口。
        assert_eq!(
            default_review_cutoff_at(&dir, today, "08:59").unwrap(),
            "2026-10-10"
        );
        // 到达开始时间后才恢复原规则，自动补记昨天。
        let cutoff = default_review_cutoff_at(&dir, today, "09:00").unwrap();
        assert_eq!(cutoff, "2026-10-11");
        let created = crate::data::records::backfill_default_reviews(&dir, &cutoff).unwrap();
        assert_eq!(created, vec!["2026-10-11"]);
        assert!(
            crate::data::records::read_review(&dir, "2026-10-11")
                .unwrap()
                .meta
                .default_marked
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// 读取今日计划
///
/// 读取今天的 `plan/YYYY-MM-DD_day.json` 文件。
/// 前端调用: `invoke('get_today_plan')`
#[tauri::command]
pub async fn get_today_plan(state: State<'_, Mutex<AppState>>) -> Result<DailyPlanFile, String> {
    let data_dir = get_data_dir(state.inner())?;
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    backfill_due_default_reviews(&data_dir)?;
    let today = crate::data::today_string();
    let mut plan = crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &today)?;
    crate::core::scheduler::append_missing_subject_warnings(&data_dir, &mut plan);
    Ok(plan)
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
    let mut plan = crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &date)?;
    if date == crate::data::today_string() {
        crate::core::scheduler::append_missing_subject_warnings(&data_dir, &mut plan);
    }
    Ok(plan)
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
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    backfill_due_default_reviews(&data_dir)?;
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
            is_default_review: review
                .as_ref()
                .map(|item| item.meta.default_marked)
                .unwrap_or(false),
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
                is_default_review: false,
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
                is_default_review: false,
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
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    backfill_due_default_reviews(&data_dir)?;

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
            is_default_review: review
                .as_ref()
                .map(|item| item.meta.default_marked)
                .unwrap_or(false),
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

/// 获取指定周的确定性学习计划分析。
///
/// 若该周尚未生成持久化分析，则只计算并返回原始分析，不更新自适应状态。
/// 前端调用: `invoke('get_week_planning_analysis', { weekStart: '2026-07-21' })`
#[tauri::command]
pub async fn get_week_planning_analysis(
    week_start: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::core::adaptive_planner::WeekPlanningAnalysis, String> {
    crate::data::validate_date(&week_start)?;
    let data_dir = get_data_dir(state.inner())?;
    match crate::core::adaptive_planner::read_week_planning_analysis(&data_dir, &week_start) {
        Ok(analysis) => Ok(analysis),
        Err(_) => crate::core::adaptive_planner::analyze_week(&data_dir, &week_start),
    }
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
/// 在独立副本中生成新增排除日的调整预览，确认应用前不写计划或同步滴答。
/// AI 会根据新增的排除日重新生成本周剩余天数的 subject_allocations，
/// 把原本安排在排除日的任务量分摊到剩余学习日。
///
/// 前端调用: `invoke('add_excluded_day_and_regenerate', { weekStart: '2026-07-21', excludedDay: { date, reason_type, note } })`
#[tauri::command]
pub async fn add_excluded_day_and_regenerate(
    week_start: String,
    excluded_day: ExcludedDay,
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentPreview, String> {
    crate::data::validate_date(&week_start)?;
    crate::data::validate_date(&excluded_day.date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // 捕获输入和保存预览时短暂加锁，AI 生成期间允许继续学习。
    let io_lock = crate::get_io_lock(state.inner())?;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    plan_adjustment::generate(
        &data_dir,
        &planner,
        "exclusion",
        &week_start,
        Some(excluded_day),
        Some(&io_lock),
    )
    .await
}

/// 复盘后重新生成本周剩余天数计划（AI 驱动）
///
/// 在 submit_review 返回 needs_regeneration=true 后由前端调用，仅生成候选预览。
/// AI 会根据复盘结果重新生成本周剩余天数的 subject_allocations，
/// 并在今天是剩余天数之一时重新生成今日日计划。
///
/// 前端调用: `invoke('regenerate_remaining_days', { reviewDate: '2026-07-30' })`
#[tauri::command]
pub async fn regenerate_remaining_days(
    review_date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentPreview, String> {
    crate::data::validate_date(&review_date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // 捕获输入和保存预览时短暂加锁，AI 生成期间允许继续学习。
    let io_lock = crate::get_io_lock(state.inner())?;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法重新生成计划。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    plan_adjustment::generate(
        &data_dir,
        &planner,
        "review",
        &review_date,
        None,
        Some(&io_lock),
    )
    .await
}

#[tauri::command]
pub async fn get_plan_adjustment(
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentStatus, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    plan_adjustment::status(&dir)
}

#[tauri::command]
pub async fn update_plan_adjustment_kept(
    id: String,
    keys: Vec<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentPreview, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    plan_adjustment::update_kept(&dir, &id, keys)
}

#[tauri::command]
pub async fn discard_plan_adjustment(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    plan_adjustment::discard(&dir, &id)
}

async fn sync_adjustment(
    dir: &std::path::Path,
    dates: Vec<String>,
    mut warnings: Vec<String>,
) -> AdjustmentOutcome {
    let mut pending = false;
    // 与现有后台对账相同，每批最多三天并行；不同日期只写各自的计划文件。
    for chunk in dates.chunks(3) {
        let mut set = tokio::task::JoinSet::new();
        for date in chunk {
            let date = date.clone();
            let dir = dir.to_path_buf();
            set.spawn(async move {
                let result = crate::sync::dida::reconcile_day(&dir, &date).await;
                (date, result)
            });
        }
        while let Some(result) = set.join_next().await {
            match result {
                Ok((date, Err(e))) => {
                    pending = true;
                    warnings.push(format!(
                        "{date} 的本地安排已保存，滴答同步失败：{e}。可点击“重试同步滴答”。"
                    ));
                }
                Err(e) => {
                    pending = true;
                    warnings.push(format!(
                        "本地安排已保存，滴答同步中断：{e}。可点击“重试同步滴答”。"
                    ));
                }
                _ => {}
            }
        }
    }
    if let Err(e) = plan_adjustment::record_sync_result(dir, warnings.clone(), pending) {
        warnings.push(format!("本地安排已保存，但同步状态未能保存：{e}"));
    }
    AdjustmentOutcome {
        affected_dates: dates,
        warnings,
    }
}

#[tauri::command]
pub async fn apply_plan_adjustment(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentOutcome, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    let dates = plan_adjustment::apply(&dir, &id)?;
    Ok(sync_adjustment(&dir, dates, Vec::new()).await)
}

#[tauri::command]
pub async fn undo_plan_adjustment(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentOutcome, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    let (dates, warnings) = plan_adjustment::undo(&dir, &id)?;
    Ok(sync_adjustment(&dir, dates, warnings).await)
}

#[tauri::command]
pub async fn retry_plan_adjustment_sync(
    state: State<'_, Mutex<AppState>>,
) -> Result<AdjustmentOutcome, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    let dates = plan_adjustment::latest_dates(&dir)?;
    Ok(sync_adjustment(&dir, dates, Vec::new()).await)
}

// ============================================================================
// 手动任务（用户显式添加 / 删除 / AI 参考开关）
// ============================================================================

/// 解析科目字符串（math / english / politics / professional）
fn parse_subject_key(subject: &str) -> Result<SubjectKey, String> {
    match subject.trim().to_lowercase().as_str() {
        "math" => Ok(SubjectKey::Math),
        "english" => Ok(SubjectKey::English),
        "politics" => Ok(SubjectKey::Politics),
        "professional" => Ok(SubjectKey::Professional),
        other => Err(format!("未知科目: {}", other)),
    }
}

/// 目标日期还没有日计划文件时使用的骨架计划
///
/// 让「今天还没有生成计划」的用户也能手动添加任务；AI 后续生成计划时
/// `save_daily_plan` 会保留其中的手动任务。
fn empty_daily_plan(date: &str) -> DailyPlanFile {
    DailyPlanFile {
        version: "1.0.0".to_string(),
        meta: crate::data::plan::DailyPlanMeta {
            date: date.to_string(),
            generated_at: crate::data::now_string(),
            r#type: "manual".to_string(),
            based_on: Default::default(),
        },
        data: crate::data::plan::DailyPlanData::default(),
        view: None,
    }
}

/// 手动添加一条任务到指定日期的日计划
///
/// - `subject`: math | english | politics | professional
/// - `title`: 任务具体内容
/// - `estimated_hours`: 预计时长（可选，默认 0）
/// - `ai_reference`: 是否允许 AI 参考该任务（可选，默认 true）
///
/// 前端调用: `invoke('add_manual_task', { date, subject, title, estimatedHours, aiReference })`
#[tauri::command]
pub async fn add_manual_task(
    date: String,
    subject: String,
    title: String,
    estimated_hours: Option<f64>,
    ai_reference: Option<bool>,
    knowledge_points: Option<Vec<crate::data::plan::KnowledgePointRef>>,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    crate::data::validate_date(&date)?;
    let subject_key = parse_subject_key(&subject)?;

    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("任务内容不能为空".to_string());
    }
    if title.chars().count() > 200 {
        return Err("任务内容过长（上限 200 字）".to_string());
    }
    let hours = estimated_hours.unwrap_or(0.0).clamp(0.0, 24.0);
    let allow_ai = ai_reference.unwrap_or(true);

    let data_dir = get_data_dir(state.inner())?;
    // H1 并发保护：串行化 plan/state 的读-改-写
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    crate::data::state::read_state_or_new(&data_dir)?;
    let mut plan = match std::fs::metadata(crate::data::plan::daily_plan_path(&data_dir, &date)) {
        Ok(_) => crate::data::plan::read_daily_plan(&data_dir, &date)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => empty_daily_plan(&date),
        Err(e) => return Err(format!("无法检查日计划: {}", e)),
    };

    let index = crate::data::progress_tables::read_progress_index(&data_dir)?;
    let knowledge_points = match knowledge_points {
        Some(points) if points.is_empty() => Some(Vec::new()),
        Some(points) => Some(crate::core::task_progress::validate_points(
            &index,
            subject_key.key(),
            &points,
        )?),
        None => crate::core::progress_sync::infer_task_points(&index, subject_key.key(), &title),
    };
    let task_id = crate::data::plan::next_task_id(&date, &plan.data.tasks);
    plan.data.tasks.push(PlanTask {
        remaining_from_task_id: None,
        knowledge_points,
        id: task_id,
        subject: subject_key,
        title,
        priority: TaskPriority::B,
        estimated_hours: hours,
        goal: String::new(),
        completion_criteria: Vec::new(),
        textbook: None,
        style_tips: None,
        fallback_plan: None,
        status: TaskStatus::Pending,
        dida_task_id: None,
        source: TaskSource::Manual,
        ai_reference: allow_ai,
    });

    crate::data::plan::recalc_totals(&mut plan.data);
    crate::data::plan::save_manual_plan_and_state(&data_dir, &plan)?;

    log::info!(
        "已手动添加任务: {} [{}] {}（AI 参考: {}）",
        date,
        subject,
        plan.data
            .tasks
            .last()
            .map(|t| t.title.as_str())
            .unwrap_or(""),
        allow_ai
    );

    crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &date)
}

/// 修正任务的知识点关联；空数组明确取消自动匹配，不改学习状态。
#[tauri::command]
pub async fn set_task_knowledge_points(
    task_id: String,
    points: Vec<crate::data::plan::KnowledgePointRef>,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    let date = crate::data::task_id_date_prefix(&task_id)
        .ok_or("无效的任务 ID")?
        .to_string();
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;
    let io_lock = crate::get_io_lock(state.inner())?;
    let _guard = io_lock.lock().await;
    // 已提交复盘的关联是历史快照，避免事后改变完成范围。
    if crate::data::records::read_review(&data_dir, &date).is_ok() {
        return Err("这一天已提交复盘，不能再修改任务关联".into());
    }
    let mut plan = crate::data::plan::read_daily_plan(&data_dir, &date)?;
    let task = plan
        .data
        .tasks
        .iter_mut()
        .find(|t| t.id == task_id)
        .ok_or("未找到任务")?;
    let index = crate::data::progress_tables::read_progress_index(&data_dir)?;
    task.knowledge_points = Some(if points.is_empty() {
        Vec::new()
    } else {
        crate::core::task_progress::validate_points(&index, task.subject.key(), &points)?
    });
    crate::data::plan::save_manual_plan_and_state(&data_dir, &plan)?;
    crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &date)
}

/// 设置某条任务是否允许 AI 参考
///
/// 关闭后该任务不会出现在任何发送给 AI 的提示词中（简报、复盘、周计划等）。
/// 前端调用: `invoke('set_task_ai_reference', { taskId, allow })`
#[tauri::command]
pub async fn set_task_ai_reference(
    task_id: String,
    allow: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let date = crate::data::task_id_date_prefix(&task_id)
        .ok_or_else(|| format!("无效的任务 ID: {}", task_id))?
        .to_string();
    crate::data::validate_date(&date)?;

    let data_dir = get_data_dir(state.inner())?;
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut plan = crate::data::plan::read_daily_plan(&data_dir, &date)?;
    let task = plan
        .data
        .tasks
        .iter_mut()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("未找到任务: {}", task_id))?;
    task.ai_reference = allow;

    crate::data::plan::save_manual_plan_and_state(&data_dir, &plan)?;
    log::info!("任务 AI 参考已更新: {} -> {}", task_id, allow);
    Ok(())
}

/// 删除一条手动添加的任务（AI 生成的任务不可删除）
///
/// 前端调用: `invoke('delete_manual_task', { taskId })`
#[tauri::command]
pub async fn delete_manual_task(
    task_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    let date = crate::data::task_id_date_prefix(&task_id)
        .ok_or_else(|| format!("无效的任务 ID: {}", task_id))?
        .to_string();
    crate::data::validate_date(&date)?;

    let data_dir = get_data_dir(state.inner())?;
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut plan = crate::data::plan::read_daily_plan(&data_dir, &date)?;
    let target = plan
        .data
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("未找到任务: {}", task_id))?;
    if !target.is_manual() {
        return Err("只能删除手动添加的任务".to_string());
    }

    plan.data.tasks.retain(|t| t.id != task_id);
    crate::data::plan::recalc_totals(&mut plan.data);
    crate::data::plan::write_manual_plan_and_state(&data_dir, &plan)?;

    log::info!("已删除手动任务: {}", task_id);
    crate::data::plan::read_daily_plan_with_merged_status(&data_dir, &date)
}
