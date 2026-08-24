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
pub async fn list_review_dates(state: State<'_, Mutex<AppState>>) -> Result<Vec<String>, String> {
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
            "未配置 AI Provider，无法生成复盘。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    let review_agent = ReviewAgent::new(&ai_service);
    review_agent.generate_review(&data_dir, &date).await
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
            plan_ref: format!(
                "plan/{}{}",
                payload.date,
                crate::data::plan::DAILY_PLAN_FILE_SUFFIX
            ),
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
                let subj_key = state
                    .current_task
                    .tasks
                    .iter()
                    .find(|t| t.task_id.as_deref() == Some(&tr.task_id))
                    .map(|t| t.subject.clone());

                let task_label = state
                    .current_task
                    .tasks
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
                    subj.current_focus = format!("{} {}", subj.current_focus, mastery_label)
                        .trim()
                        .to_string();
                }
            }
        }

        // 第三阶段：处理计划外学习 —— 用用户实际到达的章节覆盖科目 current_focus，
        // 避免下一轮计划生成时进度落后于实际。
        if !payload.overcompletion.is_empty() {
            for oc in &payload.overcompletion {
                if let Some(subj) =
                    crate::data::state::get_subject_state_mut(&mut state, &oc.subject)
                {
                    if !oc.chapter_reached.is_empty() {
                        subj.current_focus = oc.chapter_reached.clone();
                        if !subj.completed.contains(&oc.chapter_reached) {
                            subj.completed.push(oc.chapter_reached.clone());
                        }
                        changed = true;
                        log::info!(
                            "计划外进展更新：{} 的 current_focus 更新为 {}",
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
