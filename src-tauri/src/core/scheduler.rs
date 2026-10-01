//! DailyScheduler — 从周计划 JSON 生成日计划 JSON
//!
//! 原则：
//! - 不调用 AI
//! - 读取当前周计划，把 WeekDayPlan.subject_allocations.task_templates
//!   映射为 PlanTask 实例
//! - 任务 ID 格式：{date}-{sequence}
//! - 跳过休息日
//! - 不再处理未完成任务顺延和即时调整：这些由 AI 在复盘后重排剩余天数时处理

use std::path::Path;

use crate::core::date_utils::{days_between, now_string, today_string};
use crate::data::plan::{
    BasedOn, DailyPlanData, DailyPlanFile, DailyPlanMeta, PlanTask, TaskTemplate, WeekDayPlan,
    WeekPlanFile,
};
use crate::data::state::{CurrentTask, StateTask, SubjectKey, TaskStatus};
use crate::data::{iso_week_string, DataResult};

/// 日计划调度器
pub struct DailyScheduler;

impl DailyScheduler {
    /// 为指定日期生成日计划
    ///
    /// 流程：
    /// 1. 根据 date 推导周起始，读取周计划 JSON
    /// 2. 在周计划 data.days 中查找对应日期
    /// 3. 若 is_rest_day=true 则返回错误（不生成）
    /// 4. 将每个 DaySubjectAllocation.task_templates 映射为 PlanTask
    /// 5. 汇总 total_hours / total_tasks
    /// 6. 补充 State 中的剩余天数与目标信息
    /// 7. 若 sync_state=true，同步初始化 State.current_task（仅当 state 中当天任务为空时）
    ///
    /// sync_state 参数：
    /// - true: 强制重置 State.current_task 为当天任务（用于前端按钮触发生成/重排今天）
    /// - false: 不同步 state（用于批量生成多天日计划，避免互相覆盖 current_task）
    pub fn generate_daily_plan(
        data_dir: &Path,
        date: &str,
        sync_state: bool,
    ) -> DataResult<DailyPlanFile> {
        let iso_week = iso_week_string(date)?;
        let week_plan = crate::data::plan::read_week_plan(data_dir, &iso_week)?;

        let day_plan = find_day_plan(&week_plan, date)?;
        if day_plan.is_rest_day {
            return Err(format!("{} 是休息日，不生成日计划", date));
        }

        let mut state = crate::data::state::read_state_or_default(data_dir);
        // 考试日期非法时不再静默按 0 处理：0 天会严重影响后续提示语气，至少留下可定位的日志
        let remaining_days = match days_between(&state.meta.exam_date, date) {
            Ok(d) => d,
            Err(e) => {
                log::warn!(
                    "考试日期「{}」无法解析（{}），剩余天数按 0 处理",
                    state.meta.exam_date,
                    e
                );
                0
            }
        };
        let target = format!(
            "{} {} | 总分 {} / 500",
            state.meta.target_school,
            state.meta.target_major,
            state.subjects.math.target_score
                + state.subjects.english.target_score
                + state.subjects.politics.target_score
                + state.subjects.professional.target_score
        );

        // 读取各科开始学习日期：未到开始日期的科目过滤掉，作为 prompt 失效的兜底
        let settings = crate::load_settings(data_dir);
        let subject_start_dates = settings.subject_start_dates();

        // 收集（科目，任务模板），统一做「防重复已完成内容 / 确定性排序」后再落 ID
        let mut pending: Vec<(SubjectKey, TaskTemplate)> = Vec::new();
        // 生成过程中的降级提示（目标倒排失败、预算裁剪），随日计划返回给前端展示
        let mut warnings = crate::core::goal_planner::goal_capacity_warnings(data_dir, date);
        let mut goals = crate::data::goal::read_goals(data_dir)?.data.goals;
        goals.sort_by(|a, b| a.deadline.cmp(&b.deadline).then(a.book.cmp(&b.book)));
        let mut allocations = day_plan.subject_allocations.clone();
        for goal in goals
            .iter()
            .filter(|g| g.active && g.deadline.as_str() >= date)
        {
            if goal.subject.state(&state).active
                && !allocations.iter().any(|a| a.subject == goal.subject)
            {
                allocations.push(crate::data::plan::DaySubjectAllocation {
                    subject: goal.subject.clone(),
                    ..Default::default()
                });
            }
        }
        for allocation in &allocations {
            // 兜底：若该科目在当天还未到开始学习日期，则跳过
            if subject_not_started(&allocation.subject, date, &subject_start_dates) {
                log::warn!(
                    "科目 {:?} 在 {} 未到开始学习日期，跳过其任务分配（兜底过滤）",
                    allocation.subject,
                    date
                );
                continue;
            }
            // 截止日规划区间：该科目当天有任一「生效区间」时，按目标倒排知识点任务接管。
            // 支持同科不同书/板块同时并行推进（每书一条独立目标），所以这里要遍历全部。
            let active_goals: Vec<_> = goals
                .iter()
                .filter(|g| {
                    g.subject == allocation.subject && g.active && g.deadline.as_str() >= date
                })
                .collect();
            if !active_goals.is_empty() {
                let version = goal_subject_version(&state, &allocation.subject);
                let mut all_goal_tasks: Vec<crate::data::plan::PlanTask> = Vec::new();
                let mut any_tasks = false;
                let mut valid_goal = false;
                // 生成失败的书/板块（书 → 失败原因），用于向用户提示
                let mut failed_books: Vec<(String, String)> = Vec::new();
                for goal in &active_goals {
                    match crate::core::goal_planner::plan_goal_tasks_sync(
                        data_dir, goal, date, &version,
                    ) {
                        Ok(ts) if !ts.is_empty() => {
                            valid_goal = true;
                            any_tasks = true;
                            all_goal_tasks.extend(ts);
                        }
                        Ok(_) => {
                            valid_goal = true;
                        }
                        Err(e) => {
                            log::warn!(
                                "科目 {:?} 书本「{}」目标倒排生成失败: {}",
                                allocation.subject,
                                goal.book,
                                e
                            );
                            failed_books.push((goal.book.clone(), e));
                        }
                    }
                }
                // 多书部分成功时，成功的书照常接管；失败的书当天不会有倒排任务，
                // 必须让用户能看到原因，否则表现为「某本书今天莫名没有任务」。
                if any_tasks && !failed_books.is_empty() {
                    let books = failed_books
                        .iter()
                        .map(|(b, _)| {
                            if b.is_empty() {
                                "整科目标"
                            } else {
                                b.as_str()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("、");
                    warnings.push(format!(
                        "{}的「{}」目标计划今日生成失败，未排入该书任务；可尝试重新生成周计划或检查目标设置。",
                        allocation.subject.label(),
                        books
                    ));
                }
                if any_tasks {
                    log::info!(
                        "科目 {:?} 在 {} 有 {} 条生效目标，共生成 {} 条倒排任务",
                        allocation.subject,
                        date,
                        active_goals.len(),
                        all_goal_tasks.len()
                    );
                    let owned: Vec<TaskTemplate> = all_goal_tasks
                        .into_iter()
                        .map(|t| TaskTemplate {
                            title: t.title,
                            priority: t.priority,
                            estimated_hours: t.estimated_hours,
                            goal: t.goal,
                            completion_criteria: t.completion_criteria,
                            textbook: t.textbook,
                            ..Default::default()
                        })
                        .collect();
                    pending.extend(owned.into_iter().map(|tp| (allocation.subject.clone(), tp)));
                    continue;
                }
                if valid_goal {
                    continue;
                }
                if !failed_books.is_empty() {
                    warnings.push(format!(
                        "{}目标倒排失败：{}",
                        allocation.subject.label(),
                        failed_books
                            .iter()
                            .map(|(book, error)| format!("{}：{}", book, error))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
            }
            // 排程可行性校验：过滤已完成章节的重复任务（防重复安排已完成内容）
            // 用边界匹配，避免把"矩阵的特征值"这类新子主题误判为已完成"矩阵"
            let completed = completed_chapters(&state, &allocation.subject);
            for template in &allocation.task_templates {
                if let Some(finished) = completed
                    .iter()
                    .find(|c| !c.is_empty() && matches_completed(&template.title, c.as_str()))
                {
                    log::warn!(
                        "排程校验: 跳过已完成章节任务「{}」（{} 已完成「{}」）",
                        template.title,
                        allocation.subject.label(),
                        finished
                    );
                    continue;
                }
                pending.push((allocation.subject.clone(), template.clone()));
            }
        }

        // 排程可行性校验：任务数量告警（不裁剪，避免丢失任务；时长超额由下方预算积压记录与容量告警承担）
        // 期望条数与设置页同口径：由「每日目标学时 ÷ 用户设置的任务粒度」派生
        let max_tasks = crate::core::planning::pure::derive_task_count_with_granularity(
            settings.daily_target_hours(),
            settings.standard_granularity(),
            1.0,
            1,
        ) as usize;
        if pending.len() > max_tasks {
            log::warn!(
                "排程校验: {} 计划任务 {} 个，超过用户期望的每日 {} 个（仅提醒，不裁剪以免丢失任务）",
                date,
                pending.len(),
                max_tasks
            );
        }

        // 保留考纲顺序；到期复习和上个学习日的积压内容优先，禁止按时长重排前置知识。
        let mut carry = carried_tasks(data_dir, date);
        carry.retain(|t| {
            !subject_not_started(&t.subject, date, &subject_start_dates)
                && !completed_chapters(&state, &t.subject)
                    .iter()
                    .any(|c| matches_completed(&t.title, c))
                && !goals.iter().any(|g| {
                    g.subject == t.subject
                        && t.textbook.as_deref() == Some(g.book.as_str())
                        && g.current_position.is_some_and(|position| {
                            crate::core::chapter_seq::position(
                                t.subject.key(),
                                &goal_subject_version(&state, &t.subject),
                                &t.title,
                            )
                            .is_some_and(|p| p <= position)
                        })
                })
        });
        for task in carry.into_iter().rev() {
            pending.retain(|(s, t)| *s != task.subject || t.title != task.title);
            pending.insert(
                0,
                (
                    task.subject,
                    TaskTemplate {
                        title: task.title,
                        estimated_hours: task.estimated_hours,
                        goal: task.goal,
                        completion_criteria: task.completion_criteria,
                        textbook: task.textbook,
                        ..Default::default()
                    },
                ),
            );
        }
        let recent = crate::data::records::list_review_dates(data_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.as_str() < date)
            .filter_map(|d| crate::data::records::read_review(data_dir, &d).ok())
            .collect::<Vec<_>>();
        let progress_index = crate::data::progress_tables::load_progress_index(data_dir);
        for item in crate::core::planning::pure::memory_curve_review_items(&recent, date, date)
            .into_iter()
            .rev()
        {
            if item.subject.state(&state).active
                && (settings.enable_review_tasks()
                    || crate::data::progress_tables::active_table_basics_done(
                        &progress_index,
                        item.subject.key(),
                    ))
                && !subject_not_started(&item.subject, date, &subject_start_dates)
                && !pending
                    .iter()
                    .any(|(s, t)| *s == item.subject && t.title == item.title)
            {
                pending.insert(
                    0,
                    (
                        item.subject,
                        TaskTemplate {
                            title: item.title,
                            estimated_hours: 0.5,
                            goal: "巩固薄弱内容并检查掌握情况".into(),
                            completion_criteria: vec!["完成回忆或练习，并检查错误".into()],
                            ..Default::default()
                        },
                    ),
                );
            }
        }

        let mut tasks = Vec::new();
        for (seq, (subject, template)) in (1i32..).zip(pending) {
            let task = template_to_task(&template, &subject, date, seq);
            tasks.push(task);
        }

        // 每科保底并保留真实估时，容量不足的其他任务进入积压队列。
        let raw_total_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
        let budget = settings.daily_target_hours();
        let original_tasks = tasks.clone();
        let (trimmed, overloaded) = trim_to_budget(&mut tasks, budget);
        let retained: std::collections::HashSet<_> = tasks.iter().map(|t| t.id.clone()).collect();
        let deferred_tasks: Vec<_> = original_tasks
            .into_iter()
            .filter(|t| !retained.contains(&t.id))
            .collect();
        if trimmed > 0 || overloaded {
            let total: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
            log::warn!(
                "每日预算：原总时长 {:.2}h，预算 {:.2}h，待安排 {} 条、保底超载={}，最终真实估时 {:.2}h",
                raw_total_hours,
                budget,
                trimmed,
                overloaded,
                total
            );
            if trimmed > 0 {
                warnings.push(format!(
                    "今日任务超出每日目标 {:.2}h，{} 条任务已保存为待安排任务，下个学习日继续安排。{}",
                    budget,
                    trimmed,
                    if overloaded { "每科保底任务仍超过预算，已保留真实估时，请调整学习容量或目标。" } else { "" }
                ));
            } else {
                warnings.push(format!(
                    "每科保底任务仍超出每日目标 {:.2}h，已保留真实估时；请调整学习容量或目标。",
                    budget
                ));
            }
        }

        let total_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
        let total_tasks = tasks.len() as i32;

        // 今日强度预测（E）：把强度建议作为当日的一条学习提示写入 style_tips
        let intensity_note = today_intensity_note(data_dir);

        // 构建策略：拼接当天各科 focus（仅包含未过滤的科目）
        let strategy = day_plan
            .subject_allocations
            .iter()
            .filter(|a| !subject_not_started(&a.subject, date, &subject_start_dates))
            .map(|a| format!("{}: {}", a.subject.label(), a.focus))
            .collect::<Vec<_>>()
            .join("；");

        let mut style_tips: Vec<String> = Vec::new();
        if !intensity_note.is_empty() {
            style_tips.push(intensity_note);
        }

        let daily_data = DailyPlanData {
            deferred_tasks,
            remaining_days,
            target,
            strategy: strategy.clone(),
            tasks: tasks.clone(),
            risks: Vec::new(),
            style_tips,
            after_today: String::new(),
            reminders: Vec::new(),
            total_hours,
            total_tasks,
            warnings,
        };

        let mut plan = DailyPlanFile {
            version: "1.0.0".to_string(),
            meta: DailyPlanMeta {
                date: date.to_string(),
                generated_at: now_string(),
                r#type: "daily".to_string(),
                based_on: BasedOn {
                    state: "state/current.state".to_string(),
                    user_model: "assets/user_model/_index.md".to_string(),
                    exam_config: "assets/config/exam-config.md".to_string(),
                    review_ref: None,
                    week_plan: Some(format!(
                        "plan/{}{}",
                        iso_week,
                        crate::data::plan::WEEK_PLAN_FILE_SUFFIX
                    )),
                },
            },
            data: daily_data,
            view: None,
        };

        // 生成时写入提示；读取旧日计划时也会运行同一检查。
        append_missing_subject_warnings(data_dir, &mut plan);

        // 同步初始化 State.current_task
        // 原则：每次生成新的日计划，都强制重置 current_task 为该日全新任务，状态全部 Pending。
        // 这能避免旧版本遗留的污染状态（错位 task_id、错误 done 状态）被带到新计划。
        // 用户在生成计划之后点击的完成状态，会在 update_task_status 中正常写入 state。
        // 批量生成多天日计划时传 sync_state=false，避免互相覆盖 current_task。
        if sync_state && !tasks.is_empty() {
            let state_clean = !state.current_task.tasks.iter().any(|t| {
                t.task_id
                    .as_ref()
                    .map(|id| {
                        crate::data::task_id_date_prefix(id)
                            .map(|prefix| prefix != date)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            });

            // 如果 state 已被污染（日期/内容不一致），先记录日志，再强制重置
            if !state_clean {
                log::warn!(
                    "生成 {} 日计划时发现 current_task 被污染（task_id 日期前缀不一致），强制重置",
                    date
                );
            }

            state.current_task = CurrentTask {
                date: date.to_string(),
                focus: strategy.clone(),
                total_hours: Some(total_hours),
                tasks: tasks
                    .iter()
                    .map(|t| StateTask {
                        task_id: Some(t.id.clone()),
                        subject: format!("{:?}", t.subject).to_lowercase(),
                        task: t.title.clone(),
                        priority: t.priority.clone(),
                        status: TaskStatus::Pending,
                        started_at: None,
                        accumulated_minutes: 0,
                        source: t.source.clone(),
                        ai_reference: t.ai_reference,
                    })
                    .collect(),
                note: String::new(),
            };
            // 保存 state（失败不阻塞日计划生成）
            if let Err(e) = crate::data::state::save_state(data_dir, &state) {
                log::warn!("同步初始化 State 失败（不阻塞日计划生成）: {}", e);
            }
        }

        Ok(plan)
    }
}

fn find_day_plan<'a>(week_plan: &'a WeekPlanFile, date: &str) -> DataResult<&'a WeekDayPlan> {
    week_plan
        .data
        .days
        .iter()
        .find(|d| d.date == date)
        .ok_or_else(|| {
            format!(
                "周计划 {} 中未找到 {} 的日安排",
                week_plan.meta.week_start, date
            )
        })
}

/// 取科目的版本标签（用于 chapter_seq 定位目标章节位置）
///
/// 仅数学/英语配有教材版本标签，其余科目无版本概念，返回空串。
fn goal_subject_version(state: &crate::data::state::StudyState, subject: &SubjectKey) -> String {
    match subject {
        SubjectKey::Math | SubjectKey::English => {
            subject.state(state).version.clone().unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// 对新生成和已有的日计划补充缺科提示。读取旧计划时只修改返回值，
/// 使安装版留下的 0 任务计划也能在页面上说明原因。
pub fn append_missing_subject_warnings(data_dir: &Path, plan: &mut DailyPlanFile) {
    let state = crate::data::state::read_state_or_default(data_dir);
    let settings = crate::load_settings(data_dir);
    let start_dates = settings.subject_start_dates();
    let shares = settings.subject_time_allocation();
    for subject in SubjectKey::ALL {
        if !subject.state(&state).active
            || subject_not_started(&subject, &plan.meta.date, &start_dates)
            || shares.as_ref().is_some_and(|allocation| {
                allocation.get(subject.key()).copied().unwrap_or(0.0) <= 0.0
            })
            || plan.data.tasks.iter().any(|task| task.subject == subject)
        {
            continue;
        }
        let warning = format!(
            "{}今日未排入任务：周计划可能漏排，或任务在生成时被过滤。请检查本周计划并重新生成。",
            subject.label()
        );
        if !plan.data.warnings.contains(&warning) {
            plan.data.warnings.push(warning);
        }
    }
}

/// 该科目已完成章节标题（用于防重复安排已完成内容）
fn completed_chapters(state: &crate::data::state::StudyState, subject: &SubjectKey) -> Vec<String> {
    subject.state(state).completed.clone()
}

/// 今日强度预测注记（E）：读取今天及之前的最近复盘，交给 planner 的强度判定，
/// 返回一行可写入日计划的学习提示；无复盘数据时返回空串。
fn today_intensity_note(data_dir: &Path) -> String {
    let dates = crate::data::records::list_review_dates(data_dir).unwrap_or_default();
    let today = today_string();
    let mut reviews = Vec::new();
    for d in dates
        .into_iter()
        .filter(|d| d.as_str() <= today.as_str())
        .rev()
        .take(7)
    {
        if let Ok(r) = crate::data::records::read_review(data_dir, &d) {
            reviews.push(r);
        }
    }
    if reviews.is_empty() {
        return String::new();
    }
    crate::core::planner::today_intensity_label(&reviews)
}

/// 与周计划去重使用同一条精确标题规则，避免日计划再次误删子主题。
fn matches_completed(title: &str, completed: &str) -> bool {
    crate::core::planning::pure::matches_completed(title, completed)
}

/// 判断某科目在指定日期是否还未到开始学习日期
///
/// 返回 true 表示该科目在 `date` 当天不应安排任务（开始日期晚于 `date`）。
/// 开始日期为空表示立即开始，返回 false。
fn subject_not_started(
    subject: &SubjectKey,
    date: &str,
    subject_start_dates: &[(&'static str, String)],
) -> bool {
    let key = subject.key();
    for (k, start_date) in subject_start_dates {
        if *k == key && !start_date.is_empty() {
            // 开始日期严格晚于当天日期，则未开始
            return start_date.as_str() > date;
        }
    }
    false
}

fn template_to_task(
    template: &TaskTemplate,
    subject: &SubjectKey,
    date: &str,
    seq: i32,
) -> PlanTask {
    PlanTask {
        id: format!("{}-{:02}", date, seq),
        subject: subject.clone(),
        title: template.title.clone(),
        priority: template.priority.clone(),
        estimated_hours: template.estimated_hours,
        goal: template.goal.clone(),
        completion_criteria: template.completion_criteria.clone(),
        textbook: template.textbook.clone(),
        style_tips: template.style_tips.clone(),
        fallback_plan: template.fallback_plan.clone(),
        status: TaskStatus::Pending,
        dida_task_id: None,
        source: crate::data::state::TaskSource::Ai,
        ai_reference: true,
    }
}

/// 取上一个已生成日计划的积压任务，并排除之后明确完成的内容。
fn carried_tasks(data_dir: &Path, date: &str) -> Vec<PlanTask> {
    let dates = crate::data::plan::list_daily_plan_dates(data_dir).unwrap_or_default();
    let previous = dates.iter().filter(|d| d.as_str() < date).max();
    let first_date = previous.map(|d| d.as_str()).unwrap_or(date);
    let mut queue = previous
        .and_then(|d| crate::data::plan::read_daily_plan(data_dir, d).ok())
        .map(|p| p.data.deferred_tasks)
        .unwrap_or_default();
    // 同日重新生成也必须保留已有积压，避免 AI 更换周计划后把旧任务丢掉。
    if let Ok(current) = crate::data::plan::read_daily_plan(data_dir, date) {
        queue.extend(current.data.deferred_tasks);
    }
    let mut completed: std::collections::HashSet<(String, String)> =
        crate::data::records::list_review_dates(data_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.as_str() >= first_date && d.as_str() <= date)
            .filter_map(|d| crate::data::records::read_review(data_dir, &d).ok())
            .flat_map(|r| r.task_reviews)
            .filter(|t| t.status == "completed")
            .map(|t| (t.subject, t.title))
            .collect();
    let state = crate::data::state::read_state_or_default(data_dir);
    if state.current_task.date == date {
        completed.extend(
            state
                .current_task
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Done)
                .map(|t| (t.subject.clone(), t.task.clone())),
        );
    }
    let mut seen = std::collections::HashSet::new();
    queue
        .into_iter()
        .filter(|t| {
            let key = (t.subject.key().to_string(), t.title.clone());
            !completed.contains(&key) && seen.insert(key)
        })
        .collect()
}

/// 优先保留各科最早的一条，其他任务仅在真实工时能放入预算时安排。
/// 返回 (待安排条数, 保底任务仍超预算)，绝不缩短任务估时。
fn trim_to_budget(tasks: &mut Vec<PlanTask>, budget: f64) -> (usize, bool) {
    if tasks.is_empty() {
        return (0, false);
    }
    let budget = if budget.is_finite() {
        budget.max(0.0)
    } else {
        0.0
    };
    if budget == 0.0 {
        let count = tasks.len();
        tasks.clear();
        return (count, false);
    }
    let mut seen = std::collections::HashSet::new();
    let mut keep = vec![false; tasks.len()];
    let mut hours = 0.0;
    for (i, task) in tasks.iter().enumerate() {
        if seen.insert(task.subject.key()) {
            keep[i] = true;
            hours += task.estimated_hours.max(0.0);
        }
    }
    let mut blocked = std::collections::HashSet::new();
    for (i, task) in tasks.iter().enumerate() {
        if keep[i] {
            continue;
        }
        // 某书前面的知识点被延后时，后面的内容也延后；不修改用户的跳跃进度判定。
        let key = (task.subject.key(), task.textbook.as_deref().unwrap_or(""));
        if !blocked.contains(&key) && hours + task.estimated_hours.max(0.0) <= budget + 1e-9 {
            keep[i] = true;
            hours += task.estimated_hours.max(0.0);
        } else {
            blocked.insert(key);
        }
    }
    let old_len = tasks.len();
    let mut i = 0;
    tasks.retain(|_| {
        let retain = keep[i];
        i += 1;
        retain
    });
    (old_len - tasks.len(), hours > budget + 1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::plan::{
        save_week_plan, WeekPlanData, WeekPlanFile, WeekPlanMeta, WeekSubjectPlan,
    };
    use crate::data::state::TaskPriority;
    use std::io::Write;

    fn sample_state_toml() -> String {
        r#"[meta]
last_updated = "2026-07-25T10:00:00+08:00"
exam_date = "2026-12-26"
target_school = "广东工业大学"
target_major = "计算机技术"

[subjects.math]
active = true
name = "数学（数二）"
phase = "foundation"
target_score = 120
current_score = 0
weekly_hours = 10.0
weak_chapters = []
strong_chapters = []
completed = []
current_focus = "线性代数"

[subjects.english]
active = true
name = "英语（二）"
phase = "foundation"
target_score = 75
current_score = 0
weekly_hours = 5.0
weak_chapters = []
strong_chapters = []
completed = []
current_focus = "阅读"

[subjects.politics]
active = false
name = "政治"
phase = "foundation"
target_score = 70
weekly_hours = 0.0

[subjects.professional]
active = true
name = "408 计算机综合"
phase = "foundation"
target_score = 110
weekly_hours = 8.0
weak_chapters = []
strong_chapters = []
completed = []
current_focus = "计组"
"#
        .to_string()
    }

    #[test]
    fn test_scheduler_generates_daily_plan_from_week_plan() {
        let tmp = std::env::temp_dir().join(format!(
            "studyagent_scheduler_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join("plan")).unwrap();
        std::fs::create_dir_all(tmp.join("state")).unwrap();

        // 写入 state
        let mut state_file =
            std::fs::File::create(tmp.join("state").join("current.state")).unwrap();
        state_file
            .write_all(sample_state_toml().as_bytes())
            .unwrap();

        // 写入周计划
        let week_plan = WeekPlanFile {
            version: "1.0.0".to_string(),
            meta: WeekPlanMeta {
                week_start: "2026-07-20".to_string(),
                week_end: "2026-07-26".to_string(),
                week_number: 30,
                generated_at: "2026-07-20T04:00".to_string(),
                subject_time_allocation_snapshot: None,
                based_on: BasedOn {
                    state: "state/current.state".to_string(),
                    user_model: "assets/user_model/_index.md".to_string(),
                    exam_config: "assets/config/exam-config.md".to_string(),
                    review_ref: None,
                    week_plan: None,
                },
            },
            data: WeekPlanData {
                goals: vec!["完成线代前2章".to_string()],
                subjects: vec![WeekSubjectPlan {
                    subject: SubjectKey::Math,
                    weekly_hours: 10.0,
                    focus: "线性代数".to_string(),
                    milestones: vec!["行列式".to_string()],
                }],
                days: vec![
                    crate::data::plan::WeekDayPlan {
                        date: "2026-07-20".to_string(),
                        weekday: "周一".to_string(),
                        is_rest_day: false,
                        subject_allocations: vec![crate::data::plan::DaySubjectAllocation {
                            subject: SubjectKey::Math,
                            hours: 2.0,
                            focus: "行列式定义与性质".to_string(),
                            task_templates: vec![crate::data::plan::TaskTemplate {
                                title: "行列式定义".to_string(),
                                priority: TaskPriority::A,
                                estimated_hours: 1.5,
                                goal: "理解行列式定义".to_string(),
                                completion_criteria: vec!["完成教材阅读".to_string()],
                                textbook: Some("同济线代第一章".to_string()),
                                style_tips: None,
                                fallback_plan: None,
                            }],
                        }],
                    },
                    crate::data::plan::WeekDayPlan {
                        date: "2026-07-25".to_string(),
                        weekday: "周六".to_string(),
                        is_rest_day: true,
                        subject_allocations: vec![],
                    },
                ],
                ..Default::default()
            },
            view: None,
        };

        save_week_plan(&tmp, &week_plan).unwrap();

        // 生成周一的日计划
        let daily = DailyScheduler::generate_daily_plan(&tmp, "2026-07-20", true).unwrap();
        assert_eq!(daily.meta.date, "2026-07-20");
        assert_eq!(daily.data.tasks.len(), 1);
        assert_eq!(daily.data.tasks[0].id, "2026-07-20-01");
        assert_eq!(daily.data.tasks[0].subject, SubjectKey::Math);
        assert_eq!(daily.data.total_hours, 1.5);
        assert_eq!(daily.data.total_tasks, 1);
        assert!(daily
            .data
            .warnings
            .iter()
            .any(|warning| warning.contains("专业课今日未排入任务")));

        // 读取已有计划再次检查时，提示不应重复堆积。
        let mut loaded = daily.clone();
        append_missing_subject_warnings(&tmp, &mut loaded);
        assert_eq!(loaded.data.warnings.len(), daily.data.warnings.len());

        // 休息日应返回错误
        let rest_result = DailyScheduler::generate_daily_plan(&tmp, "2026-07-25", true);
        assert!(rest_result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_daily_budget_reports_irreducible_overload() {
        let tmp = std::env::temp_dir().join(format!(
            "studyagent_scheduler_budget_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join("plan")).unwrap();
        std::fs::create_dir_all(tmp.join("state")).unwrap();

        let mut sf = std::fs::File::create(tmp.join("state").join("current.state")).unwrap();
        sf.write_all(sample_state_toml().as_bytes()).unwrap();

        let week_plan = crate::data::plan::WeekPlanFile {
            version: "1.0.0".to_string(),
            meta: crate::data::plan::WeekPlanMeta {
                week_start: "2026-07-20".to_string(),
                week_end: "2026-07-26".to_string(),
                week_number: 30,
                generated_at: "2026-07-20T04:00".to_string(),
                subject_time_allocation_snapshot: None,
                based_on: crate::data::plan::BasedOn {
                    state: "state/current.state".to_string(),
                    user_model: "assets/user_model/_index.md".to_string(),
                    exam_config: "assets/config/exam-config.md".to_string(),
                    review_ref: None,
                    week_plan: None,
                },
            },
            data: crate::data::plan::WeekPlanData {
                days: vec![crate::data::plan::WeekDayPlan {
                    date: "2026-07-20".to_string(),
                    weekday: "周一".to_string(),
                    is_rest_day: false,
                    subject_allocations: vec![crate::data::plan::DaySubjectAllocation {
                        subject: SubjectKey::Math,
                        hours: 8.0,
                        focus: "超预算测试".to_string(),
                        task_templates: vec![crate::data::plan::TaskTemplate {
                            title: "超长任务".to_string(),
                            priority: TaskPriority::A,
                            estimated_hours: 8.0,
                            goal: String::new(),
                            completion_criteria: Vec::new(),
                            textbook: None,
                            style_tips: None,
                            fallback_plan: None,
                        }],
                    }],
                }],
                ..Default::default()
            },
            view: None,
        };
        crate::data::plan::save_week_plan(&tmp, &week_plan).unwrap();

        let daily = DailyScheduler::generate_daily_plan(&tmp, "2026-07-20", true).unwrap();
        assert_eq!(
            daily.data.total_hours, 8.0,
            "任务内容未减少时必须保留真实估时"
        );
        assert!(daily
            .data
            .warnings
            .iter()
            .any(|w| w.contains("保留真实估时")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 构造一条用于预算裁剪测试的任务（字段与生产构造保持一致）
    fn plan_task(title: &str, subject: SubjectKey, hours: f64) -> PlanTask {
        PlanTask {
            id: title.to_string(),
            subject,
            title: title.to_string(),
            priority: crate::data::state::TaskPriority::A,
            estimated_hours: hours,
            goal: String::new(),
            completion_criteria: Vec::new(),
            textbook: None,
            style_tips: None,
            fallback_plan: None,
            status: TaskStatus::Pending,
            dida_task_id: None,
            source: crate::data::state::TaskSource::Ai,
            ai_reference: true,
        }
    }

    /// 预算内不做任何改动
    #[test]
    fn trim_to_budget_noop_within_budget() {
        let mut tasks = vec![
            plan_task("数学-1", SubjectKey::Math, 2.0),
            plan_task("英语-1", SubjectKey::English, 1.0),
        ];
        let (trimmed, compressed) = trim_to_budget(&mut tasks, 5.0);
        assert_eq!(trimmed, 0);
        assert!(!compressed);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].estimated_hours, 2.0);
    }

    /// 每科保底 1 条：只裁「同科多余」的任务，三科各 1 条时保留真实估时并报告超载
    #[test]
    fn trim_to_budget_keeps_one_task_per_subject() {
        // 已按 estimated_hours 降序（大块头在前）
        let mut tasks = vec![
            plan_task("数学-1", SubjectKey::Math, 3.0),
            plan_task("英语-1", SubjectKey::English, 3.0),
            plan_task("政治-1", SubjectKey::Politics, 3.0),
            plan_task("数学-2", SubjectKey::Math, 1.0),
        ];
        let (trimmed, compressed) = trim_to_budget(&mut tasks, 5.0);
        // 只能裁掉「数学」多出来的那条；三科各 1 条必须保留
        assert_eq!(trimmed, 1);
        assert!(compressed, "每科仅剩 1 条仍超预算时应报告容量不足");
        assert_eq!(tasks.len(), 3);
        let mut subjects: Vec<String> = tasks.iter().map(|t| format!("{:?}", t.subject)).collect();
        subjects.sort();
        subjects.dedup();
        assert_eq!(subjects.len(), 3, "每科至少保留 1 条任务");
        let total: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
        assert_eq!(total, 9.0, "保底任务超预算时必须如实展示容量缺口");
    }

    #[test]
    fn zero_budget_defers_every_task() {
        let mut tasks = vec![plan_task("数学", SubjectKey::Math, 2.0)];
        assert_eq!(trim_to_budget(&mut tasks, 0.0), (1, false));
        assert!(tasks.is_empty());
    }

    #[test]
    fn budget_selection_keeps_earlier_points_and_real_estimates() {
        let mut tasks = vec![
            plan_task("先学", SubjectKey::Math, 1.0),
            plan_task("后学", SubjectKey::Math, 3.0),
            plan_task("再后学", SubjectKey::Math, 0.5),
        ];
        assert_eq!(trim_to_budget(&mut tasks, 2.0), (2, false));
        assert_eq!(tasks[0].title, "先学");
        assert_eq!(tasks[0].estimated_hours, 1.0);
    }
    #[test]
    fn deferred_tasks_survive_roundtrip_and_next_day_generation() {
        let dir = std::env::temp_dir().join(format!(
            "sa_carry_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut week = WeekPlanFile::default();
        week.meta.week_start = "2026-09-28".into();
        week.meta.week_end = "2026-10-04".into();
        for date in ["2026-09-30", "2026-10-01"] {
            week.data.days.push(crate::data::plan::WeekDayPlan {
                date: date.into(),
                subject_allocations: vec![crate::data::plan::DaySubjectAllocation {
                    subject: SubjectKey::Math,
                    task_templates: vec![TaskTemplate {
                        title: "普通任务".into(),
                        estimated_hours: 1.0,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        week.data.days[0].subject_allocations[0]
            .task_templates
            .push(TaskTemplate {
                title: "积压任务".into(),
                estimated_hours: 5.0,
                ..Default::default()
            });
        save_week_plan(&dir, &week).unwrap();
        let first = DailyScheduler::generate_daily_plan(&dir, "2026-09-30", false).unwrap();
        assert_eq!(first.data.deferred_tasks.len(), 1);
        crate::data::plan::save_daily_plan(&dir, &first).unwrap();
        let second = DailyScheduler::generate_daily_plan(&dir, "2026-10-01", false).unwrap();
        assert_eq!(second.data.tasks[0].title, "积压任务");
        assert_eq!(second.data.tasks[0].estimated_hours, 5.0);
        assert_eq!(second.data.deferred_tasks.len(), 1);
        assert_eq!(second.data.deferred_tasks[0].title, "普通任务");
        crate::data::plan::save_daily_plan(&dir, &second).unwrap();
        let repeated = DailyScheduler::generate_daily_plan(&dir, "2026-10-01", false).unwrap();
        assert_eq!(repeated.data.tasks.len(), 1);
        assert_eq!(
            repeated.data.deferred_tasks.len(),
            1,
            "同日重排既不能丢失积压，也不能重复积压任务"
        );
        assert_eq!(repeated.data.deferred_tasks[0].title, "普通任务");

        std::fs::remove_dir_all(dir).unwrap();
    }
}
