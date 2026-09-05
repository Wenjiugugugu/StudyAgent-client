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

use crate::data::plan::{
    BasedOn, DailyPlanData, DailyPlanFile, DailyPlanMeta, PlanTask, TaskTemplate, WeekDayPlan,
    WeekPlanFile,
};
use crate::data::state::{CurrentTask, StateTask, SubjectKey, TaskStatus};
use crate::data::{days_between, iso_week_string, now_string, DataResult};

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
        let remaining_days = days_between(&state.meta.exam_date, date).unwrap_or(0);
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
        for allocation in &day_plan.subject_allocations {
            // 兜底：若该科目在当天还未到开始学习日期，则跳过
            if subject_not_started(&allocation.subject, date, &subject_start_dates) {
                log::warn!(
                    "科目 {:?} 在 {} 未到开始学习日期，跳过其任务分配（兜底过滤）",
                    allocation.subject,
                    date
                );
                continue;
            }
            // 截止日规划区间：该科目当天有「生效区间」时，用倒排知识点任务接管，
            // 跳过周计划中该科的 task_templates（分区间的科目不占按学习时长的份额）。
            if let Some(goal) =
                crate::data::goal::active_goal_for(data_dir, &allocation.subject, date)
            {
                let version = goal_subject_version(&state, &allocation.subject);
                match crate::core::goal_planner::plan_goal_tasks_sync(
                    data_dir, &goal, date, &version,
                ) {
                    Ok(goal_tasks) if !goal_tasks.is_empty() => {
                        log::info!(
                            "科目 {:?} 在 {} 处于截止日规划区间，任务由目标倒排接管（{} 条）",
                            allocation.subject,
                            date,
                            goal_tasks.len()
                        );
                        // 将倒排任务视为模板加入 pending，走后续统一定序/预算裁剪
                        let owned: Vec<TaskTemplate> = goal_tasks
                            .into_iter()
                            .map(|t| TaskTemplate {
                                title: t.title,
                                priority: t.priority,
                                estimated_hours: t.estimated_hours,
                                goal: t.goal,
                                completion_criteria: t.completion_criteria,
                                ..Default::default()
                            })
                            .collect();
                        pending
                            .extend(owned.into_iter().map(|tp| (allocation.subject.clone(), tp)));
                        continue;
                    }
                    other => {
                        log::warn!(
                            "科目 {:?} 目标倒排生成失败或不含任务（{:?}），回退周计划任务",
                            allocation.subject,
                            other.map(|v| v.is_empty())
                        );
                    }
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
                        subject_display_name(&allocation.subject),
                        finished
                    );
                    continue;
                }
                pending.push((allocation.subject.clone(), template.clone()));
            }
        }

        // 排程可行性校验：任务数量告警（不裁剪，避免丢失任务；时长超额由下方预算归一化承担）
        let max_tasks = settings.daily_task_count() as usize;
        if pending.len() > max_tasks {
            log::warn!(
                "排程校验: {} 计划任务 {} 个，超过用户期望的每日 {} 个（仅提醒，不裁剪以免丢失任务）",
                date,
                pending.len(),
                max_tasks
            );
        }

        // 日计划确定性排序（任务分级 A/B 已下线）：大块头优先（防拖延）> 科目 > 标题
        pending.sort_by(|(sa, ta), (sb, tb)| {
            tb.estimated_hours
                .partial_cmp(&ta.estimated_hours)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| subject_ord(sa).cmp(&subject_ord(sb)))
                .then_with(|| ta.title.cmp(&tb.title))
        });

        let mut tasks = Vec::new();
        for (seq, (subject, template)) in (1i32..).zip(pending) {
            let task = template_to_task(&template, &subject, date, seq);
            tasks.push(task);
        }

        // 每日任务量预算约束：若当日任务预估时长总和超过设置中的每日目标时长，
        // 优先裁剪末尾任务（任务已按 estimated_hours 降序排列，大块头在前），
        // 保留剩余每条任务的真实时长语义，不再等比压缩时长扭曲语义。
        // 仅当单条任务本身超预算（AI 粒度问题，周计划层已被 normalize 拆分，
        // 此处是最后兜底）才对该条做等比压缩。
        let raw_total_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
        let budget = settings.daily_target_hours();
        if budget > 0.0 && raw_total_hours > budget && !tasks.is_empty() {
            let mut total = raw_total_hours;
            let mut trimmed = 0usize;
            while tasks.len() > 1 && total > budget {
                if let Some(removed) = tasks.pop() {
                    total -= removed.estimated_hours;
                    trimmed += 1;
                }
            }
            if total > budget {
                // 单条任务本身超预算：压缩该条作为最后兜底
                let scale = budget / total;
                for t in tasks.iter_mut() {
                    t.estimated_hours = (t.estimated_hours * scale * 100.0).round() / 100.0;
                }
                total = tasks.iter().map(|t| t.estimated_hours).sum();
            }
            log::warn!(
                "每日预算：今日任务原总时长 {:.2}h 超过预算 {:.2}h，已裁剪末尾 {} 个任务，最终 {:.2}h",
                raw_total_hours,
                budget,
                trimmed,
                total
            );
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
            .map(|a| format!("{}: {}", subject_display_name(&a.subject), a.focus))
            .collect::<Vec<_>>()
            .join("；");

        let mut style_tips: Vec<String> = Vec::new();
        if !intensity_note.is_empty() {
            style_tips.push(intensity_note);
        }

        let daily_data = DailyPlanData {
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
        };

        let plan = DailyPlanFile {
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
fn goal_subject_version(state: &crate::data::state::StudyState, subject: &SubjectKey) -> String {
    match subject {
        SubjectKey::Math => state.subjects.math.version.clone().unwrap_or_default(),
        SubjectKey::English => state.subjects.english.version.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

fn subject_display_name(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}
fn subject_key_str(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "math",
        SubjectKey::English => "english",
        SubjectKey::Politics => "politics",
        SubjectKey::Professional => "professional",
    }
}

/// 该科目已完成章节标题（用于防重复安排已完成内容）
fn completed_chapters(state: &crate::data::state::StudyState, subject: &SubjectKey) -> Vec<String> {
    let s = match subject {
        SubjectKey::Math => &state.subjects.math,
        SubjectKey::English => &state.subjects.english,
        SubjectKey::Politics => &state.subjects.politics,
        SubjectKey::Professional => &state.subjects.professional,
    };
    s.completed.clone()
}

/// 今日强度预测注记（E）：读取今天及之前的最近复盘，交给 planner 的强度判定，
/// 返回一行可写入日计划的学习提示；无复盘数据时返回空串。
fn today_intensity_note(data_dir: &Path) -> String {
    let dates = crate::data::records::list_review_dates(data_dir).unwrap_or_default();
    let today = crate::data::today_string();
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

/// 判断任务标题是否明确命中已完成章节（边界匹配，避免误杀"矩阵的特征值"这类子主题）。
///
/// 命中条件：
/// - 标题与已完成章节完全相等；
/// - 标题以章节名开头，且紧随其后为分隔符 / 标点（如"矩阵：性质"、"矩阵、性质"）。
///   其余情况（如"矩阵的特征值"中"的"）视为新内容，不命中，保留该任务（不丢任务）。
fn matches_completed(title: &str, completed: &str) -> bool {
    let t = title.trim();
    let c = completed.trim();
    if t.is_empty() || c.is_empty() {
        return false;
    }
    if t == c {
        return true;
    }
    if let Some(rest) = t.strip_prefix(c) {
        let is_delimiter = rest
            .chars()
            .next()
            .map(|ch| {
                matches!(
                    ch,
                    '：' | ':'
                        | '，'
                        | ','
                        | '、'
                        | '。'
                        | '；'
                        | ';'
                        | '('
                        | '（'
                        | '·'
                        | '-'
                        | '—'
                        | ')'
                        | '）'
                )
            })
            .unwrap_or(false);
        return is_delimiter;
    }
    false
}

/// 稳定的科目排序权重（用于无 AB 分级下的确定性日计划排序）
fn subject_ord(subject: &SubjectKey) -> i32 {
    match subject {
        SubjectKey::Math => 0,
        SubjectKey::English => 1,
        SubjectKey::Politics => 2,
        SubjectKey::Professional => 3,
    }
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
    let key = subject_key_str(subject);
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
    }
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

        // 休息日应返回错误
        let rest_result = DailyScheduler::generate_daily_plan(&tmp, "2026-07-25", true);
        assert!(rest_result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_daily_budget_caps_total_hours() {
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
        // 默认每日预算 5.0h，任务原 8.0h 应被归一化到 ≤ 预算
        let budget = crate::load_settings(&tmp).daily_target_hours();
        assert!(budget > 0.0);
        assert!(
            daily.data.total_hours <= budget,
            "总时长 {:.2} 应不超过预算 {:.2}",
            daily.data.total_hours,
            budget
        );
        assert!(daily.data.total_hours > 0.0);
        // 归一化后 total_hours 仍等于各任务之和
        let sum: f64 = daily.data.tasks.iter().map(|t| t.estimated_hours).sum();
        assert!((daily.data.total_hours - sum).abs() < 1e-6);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
