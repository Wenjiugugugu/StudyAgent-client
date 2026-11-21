use super::legacy::{SubmitReviewPayload, SubmitReviewResult};
use crate::core::{briefing::BriefingAgent, review::ReviewAgent};
use crate::data::records::ReviewFile;
use crate::{get_ai_service, get_data_dir, get_data_dir_and_ai, AppState};
use std::sync::Mutex;
use tauri::State;

#[tauri::command]
pub async fn get_study_directions(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::data::study_direction::CurrentStudyDirection>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    crate::data::study_direction::current_directions(&data_dir, &crate::data::today_string())
}

#[tauri::command]
pub async fn update_study_direction(
    entry: crate::data::study_direction::ActualStudyEntry,
    expected_updated_at: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::data::study_direction::CurrentStudyDirection>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    crate::data::study_direction::update_direction(&data_dir, entry, &expected_updated_at)
}

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

    // 回读滴答完成状态：今日窗口带来源标记的已完成任务，注入复盘 prompt
    let dida_completed = crate::sync::dida::fetch_completed_titles(&data_dir, &date).await;

    let review_agent = ReviewAgent::new(&ai_service);
    review_agent
        .generate_review(&data_dir, &date, &dida_completed)
        .await
}

#[tauri::command]
pub async fn submit_review(
    payload: SubmitReviewPayload,
    app_state: State<'_, Mutex<AppState>>,
) -> Result<SubmitReviewResult, String> {
    crate::data::validate_date(&payload.date)?;
    let (data_dir, _) = get_data_dir_and_ai(app_state.inner())?;

    // 回读滴答完成状态：今日窗口带来源标记的已完成任务，用于补全 task_reviews
    let dida_completed_titles =
        crate::sync::dida::fetch_completed_titles(&data_dir, &payload.date).await;

    // 并发保护
    let io_lock = crate::get_io_lock(app_state.inner())?;
    let _io_guard = io_lock.lock().await;

    // 以滴答为准：把滴答中已勾选完成的任务合并进 task_reviews
    let mut payload = payload;
    crate::data::study_direction::validate_entries(&data_dir, &mut payload.actual_study)?;
    if !dida_completed_titles.is_empty() {
        // 1) 已上报但状态非 completed 的 → 升级为完成（滴答勾选优先）
        for tr in payload.task_reviews.iter_mut() {
            if tr.completed_knowledge_points.is_some()
                || tr.status == "completed"
                || tr.title.trim().is_empty()
            {
                continue;
            }
            if dida_completed_titles.iter().any(|t| t == &tr.title) {
                tr.status = "completed".to_string();
                tr.completion = 1.0;
            }
        }
        // 2) 滴答已完成但未包含在 task_reviews 中的 → 按计划任务补一条 completed 记录
        if let Ok(plan) = crate::data::plan::read_daily_plan(&data_dir, &payload.date) {
            for t in &plan.data.tasks {
                if !dida_completed_titles.iter().any(|x| x == &t.title) {
                    continue;
                }
                if payload
                    .task_reviews
                    .iter()
                    .any(|tr| !tr.title.is_empty() && tr.title == t.title)
                {
                    continue;
                }
                payload
                    .task_reviews
                    .push(crate::data::records::TaskReviewEntry {
                        knowledge_points: None,
                        completed_knowledge_points: None,
                        task_id: t.id.clone(),
                        status: "completed".to_string(),
                        completion: 1.0,
                        mastery: String::new(),
                        blockers: Vec::new(),
                        blocker_note: None,
                        title: t.title.clone(),
                        subject: serde_json::to_string(&t.subject)
                            .unwrap_or_default()
                            .trim_matches('"')
                            .to_string(),
                        priority: format!("{:?}", t.priority),
                        estimated_hours: Some(t.estimated_hours),
                        actual_minutes: None,
                    });
            }
        }
    }

    // 用户在复盘确认的范围优先于外部清单；以计划关联校验并归一化。
    if let Ok(plan) = crate::data::plan::read_daily_plan(&data_dir, &payload.date) {
        let index = crate::data::progress_tables::load_progress_index(&data_dir);
        for entry in &mut payload.task_reviews {
            if let Some(task) = plan.data.tasks.iter().find(|t| t.id == entry.task_id) {
                crate::core::task_progress::normalize_review(&index, task, entry)?;
            }
        }
    }

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
    let (ca_total, ca_done, b_total, b_done, completion_rate) =
        crate::data::records::review_completion_stats(&crate::data::records::ReviewFile {
            task_reviews: payload.task_reviews.clone(),
            ..Default::default()
        });

    // 构建 ReviewFile
    let now = crate::data::study_direction::timestamp();
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
            default_marked: false,
        },
        data: crate::data::records::ReviewData {
            total_hours: total_actual_hours,
            external_interference: payload.daily_review.external_interference.clone(),
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
        actual_study: payload.actual_study.clone(),
    };

    let review_revision = crate::core::review_sync::prepare(&data_dir, &review)?;
    crate::data::records::save_review(&data_dir, &review)?;
    let sync = crate::core::review_sync::retry(&data_dir, &payload.date, &review_revision)
        .unwrap_or_else(|e| crate::core::review_sync::SyncStatus {
            review_revision: review_revision.clone(),
            sync_status: "pending".into(),
            pending_steps: vec!["recovery".into()],
            warnings: vec![e],
        });

    // 3. 判断是否需要重新生成本周剩余天数计划
    let needs_regeneration = crate::core::planner::check_review_needs_regeneration(&review);
    // 占比变动也触发重排：设置中「学科时间占比」相对本周计划生成时发生了变化时，
    // 即便复盘内容一切正常，也按新占比调整剩余天数（最快于下一次复盘后生效）。
    let allocation_changed =
        crate::core::planner::allocation_changed_since_week_plan(&data_dir, &payload.date);
    let direction_changed =
        crate::data::study_direction::plan_direction_changed(&data_dir, &payload.date)
            .unwrap_or(true);
    let needs_regeneration = needs_regeneration || allocation_changed || direction_changed;
    let mut regen_reasons = Vec::new();

    if allocation_changed {
        regen_reasons.push("学科时间占比已调整".to_string());
    }
    if direction_changed {
        regen_reasons.push("后续计划尚未按当前学习方向调整".to_string());
    }

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
        if review
            .actual_study
            .iter()
            .any(|e| e.scope != crate::data::study_direction::StudyScope::Today)
        {
            regen_reasons.push("当前学习方向已更新".to_string());
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
    let briefing_generating = if sync.sync_status != "complete" {
        false
    } else if let Ok(next_day) = crate::data::add_days(&payload.date, 1) {
        let ai_service = get_ai_service(app_state.inner()).ok();
        let data_dir_clone = data_dir.clone();
        let review_date = payload.date.clone();
        if let Some(ai) = ai_service {
            log::info!(
                "复盘 {} 提交后触发次日 {} 简报生成（后台）",
                payload.date,
                next_day
            );
            // 复制输入与最终保存时持锁；等待模型期间允许正常学习操作。
            let io_lock_clone = io_lock.clone();
            tauri::async_runtime::spawn(async move {
                let agent = BriefingAgent::new(&ai);
                match agent
                    .generate_guarded(
                        &data_dir_clone,
                        &next_day,
                        &review_date,
                        "auto",
                        &io_lock_clone,
                    )
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
        review_revision,
        sync_status: sync.sync_status,
        pending_steps: sync.pending_steps,
        warnings: sync.warnings,
    })
}

#[tauri::command]
pub async fn get_review_sync_status(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::core::review_sync::SyncStatus, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    crate::core::review_sync::status(&dir, &date)
}
#[tauri::command]
pub async fn retry_review_sync(
    date: String,
    review_revision: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::core::review_sync::SyncStatus, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    crate::core::review_sync::retry(&dir, &date, &review_revision)
}
