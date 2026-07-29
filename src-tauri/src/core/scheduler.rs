//! DailyScheduler — 从周计划 JSON 生成日计划 JSON
//!
//! 原则：
//! - 不调用 AI
//! - 读取当前周计划，把 WeekDayPlan.subject_allocations.task_templates
//!   映射为 PlanTask 实例
//! - 任务 ID 格式：{date}-{sequence}
//! - 跳过休息日
//! - 读取昨日复盘，若用户压力大/精力低，按规则削减当日任务量（即时调整）
//! - 读取昨日复盘，将未完成任务（incomplete/partial）顺延至今日（按科目替换）

use std::path::Path;

use crate::data::plan::{
    BasedOn, DailyPlanData, DailyPlanFile, DailyPlanMeta, PlanRisk, PlanTask, TaskTemplate,
    WeekDayPlan, WeekPlanFile,
};
use crate::data::state::{CurrentTask, StateTask, SubjectKey, TaskStatus};
use crate::data::{add_days, days_between, iso_week_string, now_string, DataResult};

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
    /// 7. 同步初始化 State.current_task（仅当 state 中当天任务为空时）
    pub fn generate_daily_plan(data_dir: &Path, date: &str) -> DataResult<DailyPlanFile> {
        let iso_week = iso_week_string(date)?;
        let week_plan = crate::data::plan::read_week_plan(data_dir, &iso_week)?;

        let day_plan = find_day_plan(&week_plan, date)?;
        if day_plan.is_rest_day {
            return Err(format!("{} 是休息日，不生成日计划", date));
        }

        let mut state = crate::data::state::read_state(data_dir).unwrap_or_default();
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

        let mut tasks = Vec::new();
        let mut seq = 1i32;

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
            for template in &allocation.task_templates {
                let task = template_to_task(template, &allocation.subject, date, seq);
                tasks.push(task);
                seq += 1;
            }
        }

        // 顺延昨日未完成任务：读取昨日复盘，将 incomplete/partial 的任务
        // 按科目替换今日同科目第一个任务（保证章节连贯），今日无该科目则追加
        let mut style_tips: Vec<String> = Vec::new();
        let carryover_tips = Self::carryover_uncompleted_tasks(data_dir, date, &mut tasks);
        if !carryover_tips.is_empty() {
            style_tips.extend(carryover_tips);
        }

        // 即时调整：读取昨日复盘，根据用户压力/精力削减当日任务量
        let adjustment = Self::compute_adjustment_from_prev_review(data_dir, date);
        if let Some(adj) = adjustment.as_ref() {
            let before_count = tasks.len();
            let before_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
            tasks = Self::apply_adjustment(tasks, adj);
            // 重新编排任务 ID（保持连续）
            for (idx, task) in tasks.iter_mut().enumerate() {
                task.id = format!("{}-{:02}", date, idx + 1);
            }
            let after_count = tasks.len();
            let after_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
            let dropped_count = before_count.saturating_sub(after_count);
            if dropped_count > 0 {
                let tip = format!(
                    "检测到昨日状态欠佳（{}），已自动削减 {} 个低优先级任务（{}→{} 个 / {:.1}h→{:.1}h）。",
                    adj.reason, dropped_count, before_count, after_count, before_hours, after_hours
                );
                log::info!("日计划即时调整: {}", tip);
                style_tips.push(tip);
            }
        }

        let total_hours: f64 = tasks.iter().map(|t| t.estimated_hours).sum();
        let total_tasks = tasks.len() as i32;

        // 将周计划风险按日期过滤（暂全部继承）
        let risks: Vec<PlanRisk> = week_plan.data.risks.clone();

        // 构建策略：拼接当天各科 focus（仅包含未过滤的科目）
        let strategy = day_plan
            .subject_allocations
            .iter()
            .filter(|a| !subject_not_started(&a.subject, date, &subject_start_dates))
            .map(|a| format!("{}: {}", subject_display_name(&a.subject), a.focus))
            .collect::<Vec<_>>()
            .join("；");

        let daily_data = DailyPlanData {
            remaining_days,
            target,
            strategy: strategy.clone(),
            tasks: tasks.clone(),
            risks,
            style_tips,
            after_today: String::new(),
            reminders: week_plan.data.reminders.clone(),
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
                    week_plan: Some(format!("plan/{}{}", iso_week, crate::data::plan::WEEK_PLAN_FILE_SUFFIX)),
                },
            },
            data: daily_data,
            view: None,
        };

        // 同步初始化 State.current_task
        // 原则：每次生成新的日计划，都强制重置 current_task 为该日全新任务，状态全部 Pending。
        // 这能避免旧版本遗留的污染状态（错位 task_id、错误 done 状态）被带到新计划。
        // 用户在生成计划之后点击的完成状态，会在 update_task_status 中正常写入 state。
        if !tasks.is_empty() {
            let state_clean = !state.current_task.tasks.iter().any(|t| {
                t.task_id
                    .as_ref()
                    .map(|id| id.len() >= 10 && &id[..10] != date)
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

    /// 顺延昨日未完成任务到今日
    ///
    /// 读取昨日复盘，将 status 为 incomplete/partial 的任务按科目替换今日同科目的
    /// 第一个任务（保证章节连贯，如行列式没学完则今天继续行列式而非矩阵）。
    /// 今日无该科目则追加到任务列表末尾。
    ///
    /// 返回顺延提示文本（用于 style_tips 展示）。
    fn carryover_uncompleted_tasks(
        data_dir: &Path,
        date: &str,
        tasks: &mut Vec<PlanTask>,
    ) -> Vec<String> {
        let mut tips = Vec::new();

        let prev_date = match add_days(date, -1) {
            Ok(d) => d,
            Err(_) => return tips,
        };

        // 读取昨日复盘
        let review = match crate::data::records::read_review(data_dir, &prev_date) {
            Ok(r) => r,
            Err(_) => return tips, // 无昨日复盘，无需顺延
        };

        // 筛选未完成任务（incomplete / partial）
        let uncompleted: Vec<&crate::data::records::TaskReviewEntry> = review
            .task_reviews
            .iter()
            .filter(|tr| tr.status == "incomplete" || tr.status == "partial")
            .collect();

        if uncompleted.is_empty() {
            return tips;
        }

        // 读取昨日日计划，获取未完成任务的完整信息
        let prev_plan = match crate::data::plan::read_daily_plan(data_dir, &prev_date) {
            Ok(p) => p,
            Err(_) => {
                log::warn!(
                    "顺延未完成任务：昨日 {} 日计划不存在，跳过",
                    prev_date
                );
                return tips;
            }
        };

        // 构建 task_id -> PlanTask 映射
        let prev_task_map: std::collections::HashMap<&str, &PlanTask> = prev_plan
            .data
            .tasks
            .iter()
            .filter_map(|t| Some((t.id.as_str(), t)))
            .collect();

        for tr in uncompleted {
            // 优先按 task_id 匹配，回退到 title 匹配
            let prev_task = prev_task_map
                .get(tr.task_id.as_str())
                .copied()
                .or_else(|| {
                    prev_plan
                        .data
                        .tasks
                        .iter()
                        .find(|t| t.title == tr.title)
                });

            let prev_task = match prev_task {
                Some(t) => t,
                None => {
                    log::warn!(
                        "顺延未完成任务：task_id={} title={} 在昨日日计划中未找到，跳过",
                        tr.task_id,
                        tr.title
                    );
                    continue;
                }
            };

            let subject_str = format!("{:?}", prev_task.subject).to_lowercase();
            let carried_title = prev_task.title.clone();

            // 在今日任务中找同科目的第一个任务
            let replace_pos = tasks
                .iter()
                .position(|t| format!("{:?}", t.subject).to_lowercase() == subject_str);

            if let Some(pos) = replace_pos {
                // 保留被覆盖的原任务内容，追加到今日末尾（学完顺延任务后可继续）
                let displaced = tasks[pos].clone();
                // 用昨日未完成任务替换今日同科目第一个任务
                let today_task = &mut tasks[pos];
                today_task.title = prev_task.title.clone();
                today_task.goal = prev_task.goal.clone();
                today_task.completion_criteria = prev_task.completion_criteria.clone();
                today_task.textbook = prev_task.textbook.clone();
                today_task.style_tips = prev_task.style_tips.clone();
                today_task.fallback_plan = prev_task.fallback_plan.clone();
                today_task.estimated_hours = prev_task.estimated_hours;
                today_task.priority = prev_task.priority.clone();

                // 被覆盖任务追加到末尾，标记为顺延产生
                let mut displaced_task = displaced;
                displaced_task.id = format!("{}-{:02}", date, tasks.len() + 1);
                displaced_task.status = TaskStatus::Pending;
                tasks.push(displaced_task);
            } else {
                // 今日无该科目，追加到末尾
                let new_task = PlanTask {
                    id: format!("{}-{:02}", date, tasks.len() + 1),
                    subject: prev_task.subject.clone(),
                    title: prev_task.title.clone(),
                    priority: prev_task.priority.clone(),
                    estimated_hours: prev_task.estimated_hours,
                    goal: prev_task.goal.clone(),
                    completion_criteria: prev_task.completion_criteria.clone(),
                    textbook: prev_task.textbook.clone(),
                    style_tips: prev_task.style_tips.clone(),
                    fallback_plan: prev_task.fallback_plan.clone(),
                    status: TaskStatus::Pending,
                };
                tasks.push(new_task);
            }

            let status_label = if tr.status == "partial" {
                "部分完成"
            } else {
                "未完成"
            };
            let subject_label = match prev_task.subject {
                SubjectKey::Math => "数学",
                SubjectKey::English => "英语",
                SubjectKey::Politics => "政治",
                SubjectKey::Professional => "专业课",
            };
            let tip = format!(
                "昨日{}「{}」{}，已顺延至今日继续学习",
                subject_label, carried_title, status_label
            );
            tips.push(tip.clone());
            log::info!("顺延未完成任务: {}", tip);
        }

        // 重新编排任务 ID（保持连续）
        for (idx, task) in tasks.iter_mut().enumerate() {
            task.id = format!("{}-{:02}", date, idx + 1);
        }

        tips
    }

    /// 读取昨日复盘，计算当日任务量调整策略
    ///
    /// 规则（任一命中即触发削减）：
    /// - `overall_feeling == "hard"`：削减当日所有 B 类任务
    /// - `energy_level <= 2`（1-5 分制）：削减当日所有 B 类任务
    /// - 同时命中两者：再额外削减最低优先级的 A 类任务中的最后一个（保留核心 A 类）
    ///
    /// 返回 None 表示无需调整（无昨日复盘 / 字段缺失 / 状态正常）。
    fn compute_adjustment_from_prev_review(
        data_dir: &Path,
        date: &str,
    ) -> Option<Adjustment> {
        let prev_date = add_days(date, -1).ok()?;
        let review = crate::data::records::read_review(data_dir, &prev_date).ok()?;

        let feeling = review
            .daily_review
            .as_ref()
            .map(|d| d.overall_feeling.as_str())
            .unwrap_or("");
        let energy = review.data.energy_level;

        let hard_feeling = feeling == "hard";
        let low_energy = energy > 0 && energy <= 2;

        if !hard_feeling && !low_energy {
            return None;
        }

        let reason = match (hard_feeling, low_energy) {
            (true, true) => format!("昨日感受「比较困难」且精力仅 {} 分", energy),
            (true, false) => "昨日感受「比较困难」".to_string(),
            (false, true) => format!("昨日精力仅 {} 分", energy),
            _ => unreachable!(),
        };

        // 削减强度：双触发 > 单触发
        let drop_b_class = true; // 削减所有 B 类
        let drop_lowest_a = hard_feeling && low_energy; // 双触发再砍最低 A 类

        Some(Adjustment {
            reason,
            drop_b_class,
            drop_lowest_a,
        })
    }

    /// 应用调整策略：按优先级从低到高移除任务
    ///
    /// 保留顺序：
    /// 1. 先保留所有 A 类任务（若 drop_lowest_a 则移除最后一个 A 类）
    /// 2. 再保留 B 类任务（若 drop_b_class 则全部移除）
    /// 3. 同优先级内按原顺序保留（不重排）
    fn apply_adjustment(mut tasks: Vec<PlanTask>, adj: &Adjustment) -> Vec<PlanTask> {
        use crate::data::state::TaskPriority::{A, B};
        if adj.drop_lowest_a {
            // 找到最后一个 A 类任务的索引并移除
            if let Some(pos) = tasks.iter().rposition(|t| t.priority == A) {
                tasks.remove(pos);
            }
        }

        if adj.drop_b_class {
            tasks.retain(|t| t.priority != B);
        }

        tasks
    }
}

/// 任务量调整描述（由昨日复盘派生）
struct Adjustment {
    /// 调整原因（用于日志和 style_tips 展示）
    reason: String,
    /// 是否移除所有 B 类任务
    drop_b_class: bool,
    /// 是否移除最后一个 A 类任务（双触发时启用）
    drop_lowest_a: bool,
}

fn find_day_plan<'a>(week_plan: &'a WeekPlanFile, date: &str) -> DataResult<&'a WeekDayPlan> {
    week_plan
        .data
        .days
        .iter()
        .find(|d| d.date == date)
        .ok_or_else(|| format!("周计划 {} 中未找到 {} 的日安排", week_plan.meta.week_start, date))
}

fn subject_display_name(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}

/// 将 SubjectKey 转为设置中使用的字符串键
fn subject_key_str(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "math",
        SubjectKey::English => "english",
        SubjectKey::Politics => "politics",
        SubjectKey::Professional => "professional",
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::plan::{
        save_week_plan, WeekPlanData, WeekPlanFile, WeekPlanMeta, WeekSubjectPlan,
    };
    use crate::data::state::{RiskLevel, RiskSubject, TaskPriority};
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
        let mut state_file = std::fs::File::create(tmp.join("state").join("current.state")).unwrap();
        state_file.write_all(sample_state_toml().as_bytes()).unwrap();

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
                risks: vec![PlanRisk {
                    subject: RiskSubject::Math,
                    item: "线代启动".to_string(),
                    level: RiskLevel::High,
                    suggestion: "安排在上午".to_string(),
                }],
                reminders: vec!["保持节奏".to_string()],
            },
            view: None,
        };

        save_week_plan(&tmp, &week_plan).unwrap();

        // 生成周一的日计划
        let daily = DailyScheduler::generate_daily_plan(&tmp, "2026-07-20").unwrap();
        assert_eq!(daily.meta.date, "2026-07-20");
        assert_eq!(daily.data.tasks.len(), 1);
        assert_eq!(daily.data.tasks[0].id, "2026-07-20-01");
        assert_eq!(daily.data.tasks[0].subject, SubjectKey::Math);
        assert_eq!(daily.data.total_hours, 1.5);
        assert_eq!(daily.data.total_tasks, 1);

        // 休息日应返回错误
        let rest_result = DailyScheduler::generate_daily_plan(&tmp, "2026-07-25");
        assert!(rest_result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
