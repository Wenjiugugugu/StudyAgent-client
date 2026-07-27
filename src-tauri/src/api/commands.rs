//! Tauri 命令定义 — 所有 `#[tauri::command]` 函数
//!
//! 前端通过 `@tauri-apps/api` 的 `invoke` 调用这些命令。
//! 所有命令返回 `Result<T, String>`，Tauri 自动将 `Err` 转为前端 Promise reject。

use std::sync::Mutex;

use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::knowledge::KnowledgeService;
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{
    KnowledgeGraph, KnowledgeObject, KnowledgeSubjectIndex, UserCapability, UserObservation,
};
use crate::data::plan::{DailyPlanFile, WeekPlanFile, iso_week_string};
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

    // 解析任务 ID: YYYY-MM-DD-NN
    let (date, task_index) = parse_task_id(&task_id)?;

    // 解析新状态
    let new_status = parse_task_status(&status)?;

    // 读取当前 State
    let mut study_state = crate::data::state::read_state(&data_dir)?;

    // 确认日期匹配
    if study_state.current_task.date != date {
        log::warn!("任务日期不匹配...");
        study_state.current_task.date = date.clone();
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
        progress.total_study_days += 1;

        // 连续天数
        if progress.last_study_date == date {
            // 同一天，不重复计算
        } else if !progress.last_study_date.is_empty() {
            let days_diff = crate::data::days_between(&date, &progress.last_study_date).unwrap_or(0);
            if days_diff == 1 {
                progress.streak_days += 1;
            } else {
                progress.streak_days = 1;
            }
        } else {
            progress.streak_days = 1;
        }
        progress.last_study_date = today;
    }

    // 保存 State
    crate::data::state::save_state(&data_dir, &study_state)?;

    log::info!("任务状态已更新: {} -> {:?}", task_id, new_status);
    Ok(())
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

    let mut summaries = Vec::with_capacity(dates.len());
    for date in dates {
        let plan = crate::data::plan::read_daily_plan(&data_dir, &date).ok();
        let review = crate::data::records::read_review(&data_dir, &date).ok();

        let planned_tasks = plan.as_ref().map(|p| p.data.total_tasks).unwrap_or(0);
        let planned_hours = plan.as_ref().map(|p| p.data.total_hours).unwrap_or(0.0);
        let is_rest_day = plan
            .as_ref()
            .map(|p| p.data.tasks.is_empty() && p.data.total_tasks == 0)
            .unwrap_or(false);

        let (completed_tasks, completion_rate) = compute_priority_a_completion(&review);
        let actual_hours = review.as_ref().map(|r| r.data.total_hours).unwrap_or(0.0);

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
        });
    }
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
    let data_dir = get_data_dir(state.inner())?;

    let mut summaries = Vec::with_capacity(7);
    for i in 0..7 {
        let date_str = crate::data::add_days(&week_start, i)
            .map_err(|e| format!("无效的周起始日期 {}: {}", week_start, e))?;

        let plan = crate::data::plan::read_daily_plan(&data_dir, &date_str).ok();
        let review = crate::data::records::read_review(&data_dir, &date_str).ok();

        let planned_tasks = plan.as_ref().map(|p| p.data.total_tasks).unwrap_or(0);
        let planned_hours = plan.as_ref().map(|p| p.data.total_hours).unwrap_or(0.0);
        let is_rest_day = plan
            .as_ref()
            .map(|p| p.data.tasks.is_empty() && p.data.total_tasks == 0)
            .unwrap_or(false);

        let (completed_tasks, completion_rate) = compute_priority_a_completion(&review);
        let actual_hours = review.as_ref().map(|r| r.data.total_hours).unwrap_or(0.0);

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
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

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
/// 前端调用: `invoke('generate_week_plan', { weekStart: '2026-07-21' })`
#[tauri::command]
pub async fn generate_week_plan(
    week_start: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<WeekPlanFile, String> {
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成周计划。请先在「设置」中添加并启用 AI Provider。".to_string(),
        );
    }

    let planner = Planner::new(&ai_service);
    planner.generate_week_plan(&data_dir, &week_start).await
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
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

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

#[tauri::command]
pub async fn submit_review(
    payload: SubmitReviewPayload,
    app_state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(app_state.inner())?;

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
        data: Default::default(),
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
    Ok(())
}

// ============================================================================
// Knowledge 命令
// ============================================================================

/// 列出知识对象索引
///
/// `subject` 为学科标识（如 "408", "math"）或 "all" 列出所有学科。
/// 前端调用: `invoke('list_knowledge', { subject: '408' })`
#[tauri::command]
pub async fn list_knowledge(
    subject: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<KnowledgeSubjectIndex>, String> {
    let data_dir = get_data_dir(state.inner())?;
    KnowledgeService::list_knowledge(&data_dir, &subject)
}

/// 获取知识对象详情
///
/// 根据 ID 读取知识对象 Markdown 文件。
/// 前端调用: `invoke('get_knowledge', { id: '408-ds-03-bst' })`
#[tauri::command]
pub async fn get_knowledge(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<KnowledgeObject, String> {
    let data_dir = get_data_dir(state.inner())?;
    KnowledgeService::get_knowledge(&data_dir, &id)
}

/// 搜索知识对象
///
/// 在所有知识对象的标题、内容、标签和别名中搜索。
/// 前端调用: `invoke('search_knowledge', { query: '二叉搜索树' })`
#[tauri::command]
pub async fn search_knowledge(
    query: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<KnowledgeObject>, String> {
    let data_dir = get_data_dir(state.inner())?;
    KnowledgeService::search_knowledge(&data_dir, &query)
}

/// 获取知识图谱
///
/// 根据 prerequisites 字段构建有向无环图 (DAG)。
/// 前端调用: `invoke('get_knowledge_graph', { subject: '408' })`
#[tauri::command]
pub async fn get_knowledge_graph(
    subject: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<KnowledgeGraph, String> {
    let data_dir = get_data_dir(state.inner())?;
    KnowledgeService::get_knowledge_graph(&data_dir, &subject)
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
    let subject_dir = textbooks_dir.join(&subject);

    // 创建学科目录
    std::fs::create_dir_all(&subject_dir)
        .map_err(|e| format!("创建教材目录失败: {}", e))?;

    let src_path = std::path::Path::new(&file_path);
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
}

/// 在已导入教材中全文搜索
///
/// 前端调用: `invoke('search_in_textbook', { query: '二叉搜索树' })`
#[tauri::command]
pub async fn search_in_textbook(
    query: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TextbookSearchHit>, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");
    let query_lower = query.to_lowercase();

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

                    for (idx, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(&query_lower) {
                            // 截取匹配上下文（前后各 40 字符）
                            let lower = line.to_lowercase();
                            let start = lower.find(&query_lower).unwrap_or(0);
                            let ctx_start = start.saturating_sub(40);
                            let ctx_end = (start + query_lower.len() + 40).min(line.len());
                            let snippet = &line[ctx_start..ctx_end];
                            let prefix = if ctx_start > 0 { "…" } else { "" };
                            let suffix = if ctx_end < line.len() { "…" } else { "" };
                            hits.push(TextbookSearchHit {
                                textbook_id: textbook_id.clone(),
                                textbook_title: textbook_title.clone(),
                                subject: subject.clone(),
                                line_number: idx + 1,
                                snippet: format!("{}{}{}", prefix, snippet, suffix),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(hits)
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
// MCP / Tool 命令
// ============================================================================

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

    log::info!("调用工具: {} args={}", tool_name, args);
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

    let msg = format!(
        "数据目录已切换至 {:?}。旧目录 {:?} 中的历史数据未自动迁移，如需保留历史计划/复盘记录，请手动复制 state/、plan/、records/、assets/ 等子目录到新目录。重启应用后配置仍然生效。",
        new_dir, old_data_dir
    );
    log::info!("{}", msg);
    Ok(msg)
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

            Some(UpdateAsset {
                name,
                download_url,
                size,
                kind,
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
/// 下载完成后返回本地文件路径，供 `install_update` 使用。
#[tauri::command]
pub async fn download_update(
    url: String,
    filename: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    log::info!("[Update] 开始下载: {}", url);

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

    let path_str = file_path.to_string_lossy().to_string();
    log::info!("[Update] 下载完成: {} ({} 字节)", path_str, downloaded);
    Ok(path_str)
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
