//! 候选规划的确定性整理与保底处理。
use super::*;
use crate::core::planning::types::RegenDayChange;
pub(super) fn apply_adaptive_parameters(
    plan: &mut WeekPlanFile,
    parameters: &AdaptivePlanParameters,
) {
    use std::collections::HashMap;
    plan.data.days.sort_by(|a, b| a.date.cmp(&b.date));
    let study_indices: Vec<_> = plan
        .data
        .days
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.is_rest_day)
        .map(|(i, _)| i)
        .collect();
    let Some(&last_day) = study_indices.last() else {
        return;
    };
    let mut queue = Vec::new();
    for &i in &study_indices {
        for allocation in &mut plan.data.days[i].subject_allocations {
            let factor = parameters
                .estimation_factors
                .get(allocation.subject.key())
                .copied()
                .unwrap_or(1.0)
                .clamp(0.8, 1.25);
            for mut task in allocation.task_templates.drain(..) {
                // 只允许用户效率校准改变估时，容量限制不能改变真实工作量。
                if task.estimated_hours.is_finite() && task.estimated_hours > 0.0 {
                    task.estimated_hours *= factor;
                } else {
                    task.estimated_hours = crate::core::planning::pure::STANDARD_GRANULARITY_HOURS;
                }
                queue.push((
                    i,
                    allocation.subject.clone(),
                    allocation.focus.clone(),
                    task,
                ));
            }
            allocation.hours = 0.0;
        }
    }
    let mut daily_hours = vec![0.0; plan.data.days.len()];
    let mut subject_hours: HashMap<String, f64> = HashMap::new();
    let mut scheduled_total = 0.0;
    // nominal_total_hours 是校准前的名义工作量；校准后的任务估时需与实际容量比较。
    let actual_week_budget =
        if parameters.capacity_hours.is_finite() && parameters.capacity_hours > 0.0 {
            parameters.capacity_hours
        } else {
            parameters.nominal_total_hours
        };
    let mut deferred_hours = 0.0;
    let mut deferred_count = 0;
    let mut blocked: HashMap<(String, Option<String>), usize> = HashMap::new();
    for (source, subject, focus, task) in queue {
        let hours = task.estimated_hours;
        let key = (subject.key().to_string(), task.textbook.clone());
        let first_day = source.max(blocked.get(&key).copied().unwrap_or(source));
        let subject_limit = parameters
            .subject_hours
            .get(subject.key())
            .copied()
            .map(|h| {
                h * parameters
                    .estimation_factors
                    .get(subject.key())
                    .copied()
                    .unwrap_or(1.0)
                    .clamp(0.8, 1.25)
            })
            .unwrap_or(actual_week_budget);
        let destination = study_indices.iter().copied().find(|&i| {
            i >= first_day
                && daily_hours[i] + hours <= parameters.daily_target_hours + 1e-9
                && scheduled_total + hours <= actual_week_budget + 1e-9
                && subject_hours.get(subject.key()).copied().unwrap_or(0.0) + hours
                    <= subject_limit + 1e-9
        });
        let i = destination.unwrap_or(last_day);
        blocked.insert(key, i);
        if destination.is_none() {
            deferred_count += 1;
            deferred_hours += hours;
        } else {
            scheduled_total += hours;
            *subject_hours.entry(subject.key().into()).or_default() += hours;
        }
        daily_hours[i] += hours;
        let day = &mut plan.data.days[i];
        if !day.subject_allocations.iter().any(|a| a.subject == subject) {
            day.subject_allocations
                .push(crate::data::plan::DaySubjectAllocation {
                    subject: subject.clone(),
                    focus,
                    ..Default::default()
                });
        }
        let allocation = day
            .subject_allocations
            .iter_mut()
            .find(|a| a.subject == subject)
            .unwrap();
        allocation.hours += hours;
        allocation.task_templates.push(task);
    }
    if deferred_count > 0 {
        plan.data.goals.push(format!("容量不足：{} 条任务约 {:.1}h 无法放入本周预算，已保留至最后学习日交由日计划记录积压，请调整目标或学习容量。", deferred_count, deferred_hours));
    }
    for subject in &mut plan.data.subjects {
        subject.weekly_hours = plan
            .data
            .days
            .iter()
            .flat_map(|d| &d.subject_allocations)
            .filter(|a| a.subject == subject.subject)
            .map(|a| a.hours)
            .sum();
    }
}

/// 对 AI 周计划做最终科目覆盖校验：每个未来学习日都必须为当日应排的科目
/// 至少保留一条任务。缺失时优先取该科进度表中尚未被本周任务覆盖的待学知识点；
/// 没有可用知识点时复用该科已有任务，或按当前学习状态生成一条明确的兜底任务。
#[allow(clippy::too_many_arguments)]
pub(super) fn enforce_daily_subject_task_coverage(
    plan: &mut WeekPlanFile,
    data_dir: &Path,
    state: &StudyState,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
    subject_time_allocation: Option<&std::collections::HashMap<String, f64>>,
    daily_task_count: i64,
    standard_granularity: f64,
    only_dates: Option<&[String]>,
) {
    use std::collections::HashMap;

    let budgets = subject_task_budget(
        state,
        daily_task_count,
        week_end,
        subject_start_dates,
        subject_time_allocation,
    );
    if budgets.is_empty() {
        return;
    }

    let mut planned_titles: HashMap<String, Vec<String>> = HashMap::new();
    let mut reusable_templates: HashMap<String, crate::data::plan::TaskTemplate> = HashMap::new();
    for day in &plan.data.days {
        for allocation in &day.subject_allocations {
            let mut valid_tasks = allocation
                .task_templates
                .iter()
                .filter(|task| !task.title.trim().is_empty());
            let Some(first_task) = valid_tasks.next() else {
                continue;
            };
            let key = allocation.subject.key().to_string();
            planned_titles.entry(key.clone()).or_default().extend(
                allocation
                    .task_templates
                    .iter()
                    .filter(|task| !task.title.trim().is_empty())
                    .map(|task| task.title.clone()),
            );
            reusable_templates
                .entry(key)
                .or_insert_with(|| (*first_task).clone());
        }
    }

    let progress_index = crate::data::load_progress_index(data_dir);
    let today = today_string();
    for day in &mut plan.data.days {
        // 历史日计划不在本次生成范围内，休息日也不补任务。
        if day.is_rest_day
            || day.date.as_str() < today.as_str()
            || only_dates.is_some_and(|dates| !dates.contains(&day.date))
        {
            continue;
        }

        for (subject, count) in &budgets {
            if *count <= 0 || planner_subject_not_started(subject, &day.date, subject_start_dates) {
                continue;
            }
            let key = subject.key().to_string();
            let has_task = day.subject_allocations.iter().any(|allocation| {
                allocation.subject == *subject
                    && allocation
                        .task_templates
                        .iter()
                        .any(|task| !task.title.trim().is_empty())
            });
            if has_task {
                continue;
            }

            let titles = planned_titles.entry(key.clone()).or_default();
            let task = subject_coverage_fallback_template(
                subject,
                state,
                &progress_index,
                titles,
                reusable_templates.get(&key),
                standard_granularity,
            );
            let hours = task.estimated_hours.max(0.0);
            if let Some(allocation) = day
                .subject_allocations
                .iter_mut()
                .find(|allocation| allocation.subject == *subject)
            {
                allocation.hours += hours;
                if allocation.focus.trim().is_empty() {
                    allocation.focus = "当日科目覆盖".to_string();
                }
                allocation.task_templates.push(task.clone());
            } else {
                day.subject_allocations
                    .push(crate::data::plan::DaySubjectAllocation {
                        subject: subject.clone(),
                        hours,
                        focus: "当日科目覆盖".to_string(),
                        task_templates: vec![task.clone()],
                    });
            }
            titles.push(task.title.clone());
            reusable_templates.entry(key).or_insert(task);
        }
    }
}

pub(super) fn subject_coverage_fallback_template(
    subject: &crate::data::state::SubjectKey,
    state: &StudyState,
    progress_index: &crate::data::progress_tables::ProgressIndex,
    planned_titles: &[String],
    reusable: Option<&crate::data::plan::TaskTemplate>,
    standard_granularity: f64,
) -> crate::data::plan::TaskTemplate {
    use crate::data::progress_tables::{active_progress_table, NodeLevel, NodeStatus};

    if let Some(table) = active_progress_table(progress_index, subject.key()) {
        if let Some(node) = table.nodes.iter().find(|node| {
            node.level == NodeLevel::Knowledge
                && matches!(node.status, NodeStatus::Pending | NodeStatus::Learning)
                && !node.title.trim().is_empty()
                && !planned_titles
                    .iter()
                    .any(|title| crate::core::progress_sync::node_matches(node, &node.phase, title))
        }) {
            let title = if !node.phase.trim().is_empty() && !node.title.contains(&node.phase) {
                format!("{}：{}", node.phase, node.title)
            } else {
                node.title.clone()
            };
            return crate::data::plan::TaskTemplate {
                title,
                estimated_hours: node
                    .estimated_hours
                    .filter(|hours| *hours > 0.0)
                    .unwrap_or(standard_granularity),
                goal: format!("完成知识点「{}」的首次学习。", node.title),
                ..Default::default()
            };
        }
    }

    if let Some(template) = reusable {
        let mut task = template.clone();
        if task.estimated_hours <= 0.0 {
            task.estimated_hours = standard_granularity;
        }
        return task;
    }

    let subject_state = subject.state(state);
    let focus = subject_state.current_focus.trim();
    let title = if !focus.is_empty() {
        format!("{}：复习与练习", focus)
    } else if active_progress_table(progress_index, subject.key()).is_some() {
        format!("{}：滚动复习已学内容与错题", subject.label())
    } else {
        format!("{}：按当前计划完成一项学习内容", subject.label())
    };
    crate::data::plan::TaskTemplate {
        title,
        estimated_hours: standard_granularity,
        ..Default::default()
    }
}

/// 每科每日任务数确定性分配（"每科至少一条，条数多了才按时长权重分散"）。
///
/// 规则：
/// - 仅计入本周已开课科目（active 且开始日期 ≤ week_end，或无开始日期约束）；
/// - 若每日任务总数 >= 科目数：先给每科保底 1 条，多余条数按 weekly_hours 权重分摊；
/// - 若总数 < 科目数：只按权重分配总数（高时长科目优先占名额）。
pub(super) fn subject_task_budget(
    state: &StudyState,
    total: i64,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
    allocation: Option<&std::collections::HashMap<String, f64>>,
) -> Vec<(crate::data::state::SubjectKey, i64)> {
    crate::core::planning::pure::subject_task_budget(
        state,
        total,
        week_end,
        subject_start_dates,
        allocation,
    )
}
type MemoryReviewItem = crate::core::planning::pure::MemoryReviewItem;
pub(super) fn memory_curve_review_items(
    reviews: &[crate::data::records::ReviewFile],
    week_start: &str,
    week_end: &str,
) -> Vec<MemoryReviewItem> {
    crate::core::planning::pure::memory_curve_review_items(reviews, week_start, week_end)
}
pub(super) fn enforce_rest_days(
    plan: &mut WeekPlanFile,
    rest_days: &[String],
    week_start: &str,
    week_end: &str,
) -> DataResult<()> {
    crate::core::planning::constraints::enforce_rest_days(plan, rest_days, week_start, week_end)
}

/// 后置校验：把排除日标记为 is_rest_day=true 并清空 subject_allocations
///
/// 与 enforce_rest_days 不同，排除日是用户主动声明的"本周临时跳过"日期，
/// 语义上区别于每周固定休息日。但为了复用 scheduler 的"休息日不生成日计划"逻辑，
/// 排除日也设置 is_rest_day=true。前端通过 week_plan.data.excluded_days 区分展示。
pub(super) fn enforce_excluded_days(
    plan: &mut WeekPlanFile,
    excluded_days: &[ExcludedDay],
) -> DataResult<()> {
    crate::core::planning::constraints::enforce_excluded_days(plan, excluded_days)
}

/// 后置校验：清空「早于今天」的日期的任务分配（周中生成周计划的确定性兜底）
///
/// 周计划正常在周一生成，覆盖整周 7 天。但当用户**周中**才首次生成周计划时，
/// 之前已过去的几天（如周一、周二）本应已有计划或已完成学习，AI 若按整周
/// 生成会把这些过去的日期也补上任务，造成与历史进度/已完成的日计划冲突。
///
/// 规则：凡 `date < today` 的日期，`subject_allocations` 一律清空（不安排任务）。
/// 这是确定性硬保证，不依赖 AI 是否遵循 prompt 中的「从今天开始」约束。
/// 周初（today = 周一）生成时，没有日期早于今天，此函数为空操作。
pub(super) fn enforce_past_days_empty(plan: &mut WeekPlanFile) {
    crate::core::planning::constraints::enforce_past_days_empty(plan)
}

// ============================================================================
// 复盘后重排相关：辅助函数
// ============================================================================

/// 判断复盘是否需要触发剩余天数重排
///
/// 触发条件（任一命中）：
/// - 存在未完成任务（status 为 incomplete 或 partial）
/// - 有任务掌握不足（mastery == "weak"，需要巩固）
/// - 用户感受困难（daily_review.overall_feeling == "hard"）
/// - 有额外进度记录（overcompletion 非空）
pub fn check_review_needs_regeneration(review: &crate::data::records::ReviewFile) -> bool {
    crate::core::planning::pure::check_review_needs_regeneration(review)
}

/// 设置中「学科时间占比」相对当前周计划生成时的快照是否发生了变化。
///
/// 占比变动时，即使复盘内容不需要重排（无未完成/困难/额外进度），
/// 也要在下次复盘后按新占比调整剩余天数；占比未变化时，
/// 是否重排完全由复盘实际情况决定。
///
/// 本周无周计划、或复盘日之后已无剩余天数（已到本周最后一天）时返回 false。
pub fn allocation_changed_since_week_plan(data_dir: &Path, review_date: &str) -> bool {
    let Ok(iso_week) = iso_week_string(review_date) else {
        return false;
    };
    let Ok(week_plan) = crate::data::plan::read_week_plan(data_dir, &iso_week) else {
        return false;
    };
    let has_remaining = add_days(review_date, 1)
        .map(|d| d <= week_plan.meta.week_end)
        .unwrap_or(false);
    if !has_remaining {
        return false;
    }
    let settings = crate::load_settings(data_dir);
    week_plan.meta.subject_time_allocation_snapshot.as_ref()
        != settings.subject_time_allocation().as_ref()
}

/// 汇总重排前后各受影响日期的任务变动明细（标题级 diff，供前端悬停展示）
///
/// - `added`：新计划有而旧计划没有的任务标题
/// - `removed`：旧计划有而新计划没有的任务标题
/// - `adjusted`：标题发生变化的（原标题 → 新标题）
pub(super) fn compute_regen_changes(
    before: &WeekPlanFile,
    after: &WeekPlanFile,
    dates: &[String],
) -> Vec<RegenDayChange> {
    fn day_titles(plan: &WeekPlanFile, date: &str) -> Vec<String> {
        plan.data
            .days
            .iter()
            .find(|d| d.date == date)
            .map(|d| {
                d.subject_allocations
                    .iter()
                    .flat_map(|a| a.task_templates.iter().map(|t| t.title.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    dates
        .iter()
        .map(|date| {
            let before_titles = day_titles(before, date);
            let after_titles = day_titles(after, date);

            // adjusted：同一顺序/同名位置的标题变化简化为「原标题 → 新标题」配对（按位置对齐，尽力而为）
            let mut adjusted: Vec<(String, String)> = Vec::new();
            for (b, a) in before_titles.iter().zip(after_titles.iter()) {
                if b != a {
                    adjusted.push((b.clone(), a.clone()));
                }
            }

            let before_set: std::collections::HashSet<&String> = before_titles.iter().collect();
            let after_set: std::collections::HashSet<&String> = after_titles.iter().collect();
            let added: Vec<String> = after_titles
                .iter()
                .filter(|t| !before_set.contains(t))
                .cloned()
                .collect();
            let removed: Vec<String> = before_titles
                .iter()
                .filter(|t| !after_set.contains(t))
                .cloned()
                .collect();

            RegenDayChange {
                date: date.clone(),
                added,
                removed,
                adjusted,
            }
        })
        .collect()
}
pub(super) fn save_week_plan_original(data_dir: &Path, plan: &WeekPlanFile) -> DataResult<()> {
    let iso_week = iso_week_string(&plan.meta.week_start)?;
    let path = crate::data::plan::week_plan_path(data_dir, &format!("{}_original", iso_week));

    // 已存在则不覆盖（保留最初版本）
    if path.exists() {
        log::info!("原始周计划副本已存在，不覆盖: {:?}", path);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(plan).map_err(|e| format!("序列化原始周计划失败: {}", e))?;
    crate::data::atomic_write(&path, &json)
        .map_err(|e| format!("写入原始周计划文件失败 {:?}: {}", path, e))?;
    log::info!("原始周计划副本已保存: {:?}", path);
    Ok(())
}

/// 该科目「已完成」章节列表
pub(super) fn subject_completed_list<'a>(
    state: &'a StudyState,
    subject: &crate::data::state::SubjectKey,
) -> &'a [String] {
    subject.state(state).completed.as_slice()
}

/// 判断任务标题是否命中已完成章节（边界匹配，与 scheduler 一致，避免误杀子主题）
pub(super) fn matches_completed(title: &str, completed: &str) -> bool {
    crate::core::planning::pure::matches_completed(title, completed)
}
pub(super) fn consistency_check_and_correct(
    week_plan: &mut WeekPlanFile,
    state: &StudyState,
    regen_dates: &[String],
    original_before: &WeekPlanFile,
    declared_subjects: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();

    // 1. 确定性去重：剔除与「已完成」重复的 forward 任务
    let mut removed = false;
    for day in week_plan.data.days.iter_mut() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        for alloc in day.subject_allocations.iter_mut() {
            let completed = subject_completed_list(state, &alloc.subject);
            if completed.is_empty() {
                continue;
            }
            let before = alloc.task_templates.len();
            alloc
                .task_templates
                .retain(|t| !completed.iter().any(|c| matches_completed(&t.title, c)));
            if alloc.task_templates.len() != before {
                removed = true;
            }
        }
    }
    if removed {
        log::info!("一致性校验：已剔除与「已完成」重复的计划任务");
        // 同步修正 allocation 时长（仅当有剩余模板可求和时）
        for day in week_plan.data.days.iter_mut() {
            for alloc in day.subject_allocations.iter_mut() {
                let sum: f64 = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
                if sum > 0.0 {
                    alloc.hours = sum;
                }
            }
        }
    }

    // 2. 进度生效检查（delta）：声明了实际进度的科目，若剩余安排与重排前完全一致 → 警告
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();
    for day in week_plan.data.days.iter() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        let orig_day = original_before
            .data
            .days
            .iter()
            .find(|d| d.date == day.date);
        for alloc in &day.subject_allocations {
            let key = alloc.subject.key();
            if !declared_subjects.contains(key) || warned.contains(key) {
                continue;
            }
            let unchanged = match orig_day.and_then(|od| {
                od.subject_allocations
                    .iter()
                    .find(|a| a.subject == alloc.subject)
            }) {
                // 仅当原日确有该科分配时才比对；否则视为「未改动的新分配」不报警
                Some(oa) => serde_json::to_value(oa).ok() == serde_json::to_value(alloc).ok(),
                None => false,
            };
            if unchanged {
                warnings.push(format!(
                    "{} 的计划外进度（当前重点）未反映到后续计划：重排后该科目剩余安排与重排前一致，请到周计划中手动调整。",
                    alloc.subject.label()
                ));
                warned.insert(key.to_string());
            }
        }
    }

    warnings
}

/// 学习活动词库：用于原子性拆分判定（一条任务 = 单一主旨的同动作单元）。
///
/// 标题含并列连词（+/＋/、/与/及）且命中 ≥2 个不同活动词时，
/// 判定为"多个独立学习单元"（如"背 250 词 + 阅读 1 篇 + 长难句 3 个"），
/// 拆为独立任务——不同学习动作即使总时长合理也必须拆。
const ACTIVITY_WORDS: [&str; 24] = [
    "单词",
    "词汇",
    "背诵",
    "默写",
    "听写",
    "阅读",
    "精读",
    "泛读",
    "真题",
    "听力",
    "长难句",
    "翻译",
    "作文",
    "完形",
    "新题型",
    "习题",
    "刷题",
    "做题",
    "错题",
    "例题",
    "讲义",
    "笔记",
    "教材",
    "章节",
];

/// 任务粒度规范化：AI 输出的兜底确定性后处理。
///
/// 1. 原子性判据：标题含并列连词且命中多个活动词时拆为独立任务。
/// 2. 时长判据：单条 < TASK_MIN_HOURS 时合并到同科相邻任务（仅同 allocation 内）；
///    之后按用户设置的**任务粒度**切分（目标条数 = 该科当日总时长 ÷ 粒度）。
/// 3. 拆分/合并后重算 allocation.hours（保持周计划层时长不失真）。
pub(super) fn normalize_task_granularity(week_plan: &mut WeekPlanFile, standard_granularity: f64) {
    for day in week_plan.data.days.iter_mut() {
        normalize_allocations(&mut day.subject_allocations, standard_granularity);
    }
}

/// 对单个 allocation 列表做任务粒度规范化。
pub(super) fn normalize_allocations(
    allocations: &mut [crate::data::plan::DaySubjectAllocation],
    standard_granularity: f64,
) {
    let gran = crate::core::planning::pure::normalize_granularity(standard_granularity);
    for alloc in allocations.iter_mut() {
        let mut expanded: Vec<crate::data::plan::TaskTemplate> = Vec::new();
        for t in alloc.task_templates.drain(..) {
            expanded.extend(split_task_by_atomicity(t));
        }

        // 时长下限合并（< TASK_MIN_HOURS 且存在前一条时并入，标题用 + 拼接）
        let mut merged: Vec<crate::data::plan::TaskTemplate> = Vec::new();
        for t in expanded {
            if t.estimated_hours < crate::core::planning::pure::TASK_MIN_HOURS {
                if let Some(last) = merged.last_mut() {
                    last.title = format!("{} + {}", t.title, last.title);
                    last.estimated_hours += t.estimated_hours;
                    last.knowledge_points = match (last.knowledge_points.take(), t.knowledge_points)
                    {
                        (Some(mut a), Some(b)) => {
                            for point in b {
                                if !a
                                    .iter()
                                    .any(|p| crate::core::task_progress::same_point(p, &point))
                                {
                                    a.push(point);
                                }
                            }
                            Some(a)
                        }
                        _ => None,
                    };
                    continue;
                }
            }
            merged.push(t);
        }

        // 按用户设置的任务粒度切分：目标条数 = 该科当日总时长 ÷ 粒度。
        // AI 常见输出是「5 条 × 1.4h」——总时长对得上，但「7 条 × 1h」的条数与细粒度
        // 设置都没落地。粒度设置必须在确定性后处理里真正生效，不能只进 prompt。
        normalize_allocation_granularity(&mut merged, gran);

        alloc.task_templates = merged;
        let sum: f64 = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
        if sum > 0.0 {
            alloc.hours = sum;
        } else if !alloc.task_templates.is_empty() {
            // 兜底：AI 漏填 estimated_hours（历史上输出示例缺该字段导致过全 0）时，
            // 优先按 allocation.hours 均分；仍无值则退回标准任务粒度，
            // 避免日计划出现「预估 0h」的任务。
            let fallback_total = if alloc.hours > 0.0 {
                alloc.hours
            } else {
                standard_granularity
            };
            let n = alloc.task_templates.len() as f64;
            let each = (fallback_total / n).max(0.5);
            for t in alloc.task_templates.iter_mut() {
                t.estimated_hours = each;
            }
            alloc.hours = fallback_total;
        }
    }
}

/// 按用户设置的任务粒度切分一个 allocation 的任务列表。
///
/// 目标条数 `target = round(该科当日总时长 ÷ 粒度)`（上限 `MAX_DAILY_TASKS`）。
/// 每条任务的目标份数 `parts = round(时长 ÷ 粒度)`，再按「单份时长最大 / 最小」把总份数
/// 收敛到 `target`：份数不足 → 继续切分单份最长的任务；份数超出 → 把单份最短的合并回。
/// 这样「每天 N 条、每条 ≈ 粒度小时」在 AI 输出不规范时也能确定性成立。
pub(super) fn normalize_allocation_granularity(
    templates: &mut Vec<crate::data::plan::TaskTemplate>,
    gran: f64,
) {
    use crate::core::planning::pure::MAX_DAILY_TASKS;

    let total: f64 = templates.iter().map(|t| t.estimated_hours.max(0.0)).sum();
    if templates.is_empty() || total <= 0.0 {
        return;
    }
    let target = ((total / gran).round() as i64).clamp(1, MAX_DAILY_TASKS) as usize;

    let mut parts: Vec<usize> = templates
        .iter()
        .map(|t| granularity_part_count(t.estimated_hours, gran))
        .collect();

    // 份数不足：优先切分「单份时长」最大的（越接近粒度上限越该再切一刀）
    let mut guard = 0usize;
    while parts.iter().sum::<usize>() < target && guard < 64 {
        guard += 1;
        let idx = (0..parts.len())
            .filter(|&i| parts[i] < 12)
            .max_by(|&a, &b| {
                let fa = per_part_hours(templates[a].estimated_hours, parts[a]);
                let fb = per_part_hours(templates[b].estimated_hours, parts[b]);
                fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
            });
        match idx {
            Some(i) => parts[i] += 1,
            None => break,
        }
    }
    // 份数超出：把「单份时长」最小的合并回（合并后仍不超过粒度上限太多）
    while parts.iter().sum::<usize>() > target && guard < 128 {
        guard += 1;
        let idx = (0..parts.len()).filter(|&i| parts[i] > 1).min_by(|&a, &b| {
            let fa = per_part_hours(templates[a].estimated_hours, parts[a]);
            let fb = per_part_hours(templates[b].estimated_hours, parts[b]);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        });
        match idx {
            Some(i) => parts[i] -= 1,
            None => break,
        }
    }

    let mut out: Vec<crate::data::plan::TaskTemplate> = Vec::new();
    for (t, k) in templates.drain(..).zip(parts) {
        let k = k.max(1);
        if k == 1 || t.knowledge_points.is_some() {
            out.push(t);
            continue;
        }
        let hours = t.estimated_hours;
        let each = round2(hours / k as f64);
        for i in 0..k {
            let mut sub = t.clone();
            sub.title = format!("{}({})", t.title, i + 1);
            sub.estimated_hours = if i == k - 1 {
                round2(hours - each * (k as f64 - 1.0))
            } else {
                each
            };
            out.push(sub);
        }
    }
    // 单份时长不超过硬上限由 `granularity_part_count` 保证，此处无需再兜底切分。
    *templates = out;
}

/// 单条任务的目标份数：`round(时长 ÷ 粒度)`，至少 1 份、最多 12 份；
/// 同时保证单份不超过硬上限 `TASK_MAX_HOURS`。
pub(super) fn granularity_part_count(hours: f64, gran: f64) -> usize {
    if !hours.is_finite() || hours <= 0.0 {
        return 1;
    }
    let mut k = ((hours / gran).round() as usize).clamp(1, 12);
    let max_parts = (hours / crate::core::planning::pure::TASK_MAX_HOURS).ceil() as usize;
    if max_parts > k {
        k = max_parts.min(12);
    }
    k
}

/// 单份时长（用于挑选该切分/合并哪条任务）。
pub(super) fn per_part_hours(hours: f64, parts: usize) -> f64 {
    if parts == 0 || !hours.is_finite() {
        return 0.0;
    }
    hours / parts as f64
}

/// 两位小数取整。
pub(super) fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 原子性拆分：标题含并列连词且命中 ≥2 个不同学习活动词时，按连接符拆为多条。
///
/// 拆分/保留时除 title 与 estimated_hours 外的其余字段
/// （priority/goal/completion_criteria/textbook/style_tips/fallback_plan）
/// 必须原样保留——早期实现用 `..Default::default()` 重建模板，
/// 曾把 AI 输出的任务目标与完成标准整批清空。
pub(super) fn split_task_by_atomicity(
    t: crate::data::plan::TaskTemplate,
) -> Vec<crate::data::plan::TaskTemplate> {
    let title = t.title.clone();
    let hours = t.estimated_hours;
    if !title.contains(['+', '＋', '、', '与', '及']) {
        return vec![t];
    }
    let matched: Vec<&str> = ACTIVITY_WORDS
        .iter()
        .filter(|w| title.contains(**w))
        .copied()
        .collect();
    let mut unique: Vec<&str> = Vec::new();
    for w in matched {
        if !unique.contains(&w) {
            unique.push(w);
        }
    }
    // 命中不同活动词 < 2 时不拆（如"行列式定义与性质"只有一个活动，是同一单元）
    if unique.len() < 2 {
        return vec![t];
    }
    let parts: Vec<&str> = title
        .split(['+', '＋', '、', '与', '及'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        return vec![t];
    }
    if t.knowledge_points.is_some() {
        return vec![t];
    }
    let each = hours / parts.len() as f64;
    parts
        .into_iter()
        .map(|p| crate::data::plan::TaskTemplate {
            title: p.to_string(),
            estimated_hours: each,
            ..t.clone()
        })
        .collect()
}

/// 找出声明了实际进度、但重排后剩余安排与重排前完全一致的科目（进度未生效）。
///
/// 返回科目 key 列表（math/english/politics/professional）。
/// 用于驱动「进度未生效 -> 联网/教纲自查 + 按实际进度强制校正重排」。
pub(super) fn find_declared_subjects_unchanged(
    week_plan: &WeekPlanFile,
    original_before: &WeekPlanFile,
    regen_dates: &[String],
    declared_subjects: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut unchanged: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for day in week_plan.data.days.iter() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        let orig_day = original_before
            .data
            .days
            .iter()
            .find(|d| d.date == day.date);
        for alloc in &day.subject_allocations {
            let key = alloc.subject.key();
            if !declared_subjects.contains(key) || seen.contains(key) {
                continue;
            }
            let is_unchanged = match orig_day.and_then(|od| {
                od.subject_allocations
                    .iter()
                    .find(|a| a.subject == alloc.subject)
            }) {
                // 仅当原日确有该科分配时才比对；否则视为「未改动的新分配」不算 no-op
                Some(oa) => serde_json::to_value(oa).ok() == serde_json::to_value(alloc).ok(),
                None => false,
            };
            if is_unchanged {
                unchanged.push(key.to_string());
                seen.insert(key.to_string());
            }
        }
    }
    unchanged
}

/// 取科目对应的版本标签（用于章节顺序表定位；政治无版本，空串即可）。
pub(super) fn subject_version(state: &StudyState, key: &str) -> String {
    match key {
        "math" => state.subjects.math.version.clone().unwrap_or_default(),
        "english" => state.subjects.english.version.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// 收集「基础已学完」的科目键集合（启用进度表知识点全部达「基础」及以上）。
///
/// 这类科目已进入强化阶段：周计划中总结/复习任务开关不再对其生效，
/// 改为必须安排巩固类任务；同时也参与复习任务开关的提示文案。
pub(super) fn basics_done_subjects(data_dir: &Path) -> Vec<String> {
    let index = crate::data::load_progress_index(data_dir);
    crate::data::state::SubjectKey::ALL
        .iter()
        .map(|subject| subject.key())
        .filter(|key| crate::data::progress_tables::active_table_basics_done(&index, key))
        .map(|key| key.to_string())
        .collect()
}

/// 「基础已学完」科目的中文名（按固定科目顺序，供 prompt 注入）
pub(super) fn basics_done_labels(data_dir: &Path) -> Vec<String> {
    basics_done_subjects(data_dir)
        .iter()
        .map(|key| planner_subject_cn_from_str(key).to_string())
        .collect()
}

/// 收集某日期日计划中「禁止 AI 参考」的任务 ID 集合
///
/// 复盘数据（`review.task_reviews`）由用户在本机提交，可能包含这些任务；
/// 重排 prompt 引用复盘明细时必须据此过滤，避免间接把任务内容喂给 AI。
pub(super) fn hidden_task_ids_for_date(
    data_dir: &Path,
    date: &str,
) -> std::collections::HashSet<String> {
    crate::data::plan::read_daily_plan(data_dir, date)
        .map(|plan| {
            plan.data
                .tasks
                .iter()
                .filter(|t| !t.ai_visible())
                .map(|t| t.id.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// 收集本周处于「截止日规划区间」的科目键集合。
///
/// 区间生效判定：该科目存在任意一条 active 目标且其 deadline 不早于本周开始。
/// （deadline 落在本周内或更远则视为本周区间内；已过期不在其中。）
/// 每书一条独立目标，但「该科目整体是否走倒排」仍以科目为粒度判断。
pub(super) fn goal_active_subjects(data_dir: &Path, today: &str, week_start: &str) -> Vec<String> {
    use crate::data::state::SubjectKey;
    let directions = crate::data::study_direction::current_directions(data_dir, &today_string())
        .unwrap_or_default();
    SubjectKey::ALL
        .into_iter()
        .filter(|subject| {
            if directions.iter().any(|d| d.subject == subject.key()) {
                return false;
            }
            let goals = crate::data::goal::active_goals_for_subject(data_dir, subject, today);
            // 区间生效判定：存在任意一条 active 目标且其 deadline 不早于本周开始。
            // active_goals_for_subject 已过滤掉 deadline < today 的过期目标，而本周
            // 的 week_start ≤ today，因此这里只需一个下界比较即可覆盖「本周内或更远」。
            goals
                .iter()
                .any(|g| !g.deadline.is_empty() && g.deadline.as_str() >= week_start)
        })
        .map(|subject| subject.key().to_string())
        .collect()
}

/// 扣除截止日规划区间的科目学时，并把剩余科目占比重新归一化。
///
/// 返回调整后的 subject_time_allocation 与每日目标学时。
/// 全部科目都在区间内时，保留原始值（无法扣到 0，避免计划为空）。
pub(super) fn deduct_goal_allocations(
    subject_time_allocation: Option<std::collections::HashMap<String, f64>>,
    daily_target_hours: f64,
    goal_subjects: &[String],
) -> (Option<std::collections::HashMap<String, f64>>, f64) {
    if goal_subjects.is_empty() {
        return (subject_time_allocation, daily_target_hours);
    }
    // 保留原始配置，供「全部科目都在区间内/剩余占比为 0」时原样退回
    let original = subject_time_allocation.clone();
    let Some(mut alloc) = subject_time_allocation else {
        // 无显式占比配置：仅按科目数先扣除一个均摊份额的直观近似。
        // 由于无占比无从精确扣减，这里不改变学时，交由 scheduler 直接替换任务。
        return (None, daily_target_hours);
    };

    // 累计区间科目占比
    let mut deducted_share = 0.0;
    for key in goal_subjects {
        if let Some(share) = alloc.get(key) {
            deducted_share += *share;
        }
        alloc.remove(key);
    }
    if alloc.is_empty() {
        // 全部科目都在区间内：保留原始配置，避免零学时
        return (original, daily_target_hours);
    }

    // 重新归一化剩余科目占比到 100%
    let remaining_sum: f64 = alloc.values().sum();
    if remaining_sum <= 0.0 {
        return (original, daily_target_hours);
    }
    for v in alloc.values_mut() {
        *v = *v / remaining_sum * 100.0;
    }

    // 每日目标学时：扣除区间科目份额
    let adjusted_hours = (daily_target_hours * (100.0 - deducted_share.min(100.0)) / 100.0)
        .max(1.0)
        .max(daily_target_hours * 0.2); // 保底不低于原来的 20%，避免计划过空
    (Some(alloc), adjusted_hours)
}

/// 确定性超前剔除：对声明了实际进度的科目，用内置章节顺序表定位，
/// 把剩余计划中排在实际进度**之前**（用户已学过）的任务模板剔除，并重算对应 allocation 时长。
///
/// 已学内容绝不允许再次安排，这是不依赖 AI、确定能落地的进度锚定兜底。
/// 返回是否剔除了内容。
pub(super) fn filter_ahead_of_progress(
    week_plan: &mut WeekPlanFile,
    state: &StudyState,
    regen_dates: &[String],
    overcompletion: &[crate::data::records::OvercompletionEntry],
) -> bool {
    let mut changed = false;
    for oc in overcompletion {
        let version = subject_version(state, &oc.subject);
        let Some(prog_pos) =
            crate::core::chapter_seq::position(&oc.subject, &version, &oc.chapter_reached)
        else {
            continue;
        };
        for day in week_plan.data.days.iter_mut() {
            if !regen_dates.contains(&day.date) {
                continue;
            }
            for alloc in day.subject_allocations.iter_mut() {
                if alloc.subject.key() != oc.subject {
                    continue;
                }
                let before = alloc.task_templates.len();
                if before == 0 {
                    continue;
                }
                alloc.task_templates.retain(|t| {
                    match crate::core::chapter_seq::position(&oc.subject, &version, &t.title) {
                        // 能定位且位置不晚于实际进度 → 已学过，剔除；定位不到 → 保留
                        Some(p) => p > prog_pos,
                        None => true,
                    }
                });
                if alloc.task_templates.len() != before {
                    changed = true;
                    // M4：剔除后按剩余模板重算时长（全部被剔除时为 0，避免「0 模板残留时长」）
                    alloc.hours = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
                }
            }
        }
    }
    changed
}

/// 更新周计划中剩余天数的 subject_allocations
///
/// 用 AI 返回的新安排覆盖对应日期的 subject_allocations。
/// 休息日的 is_rest_day 保持不变，仅更新非休息日的 allocations。
pub(super) fn update_week_plan_remaining_days(
    week_plan: &mut WeekPlanFile,
    updated_days: &[RegenDayPlan],
    standard_granularity: f64,
) {
    for updated in updated_days {
        if let Some(day) = week_plan
            .data
            .days
            .iter_mut()
            .find(|d| d.date == updated.date)
        {
            if day.is_rest_day {
                log::info!(
                    "更新周计划: {} 是休息日，跳过 subject_allocations 更新",
                    updated.date
                );
                continue;
            }
            day.subject_allocations = updated.subject_allocations.clone();
            // 任务粒度规范化：兜底 AI 重排输出的不规范粒度
            normalize_allocations(&mut day.subject_allocations, standard_granularity);
            log::info!(
                "更新周计划: {} 的 subject_allocations 已更新（{} 个科目分配）",
                updated.date,
                updated.subject_allocations.len()
            );
        } else {
            log::warn!("更新周计划: 未找到日期 {}，跳过", updated.date);
        }
    }
}

// ============================================================================
// AI 重排响应解析
// ============================================================================

/// 未完成任务确定性兜底：当 AI 重排失败或返回空时，把复盘中标记为
/// 「未完成 / 部分完成」的任务以 TaskTemplate 形式插入重排范围的学习日，
/// 逐条分摊到连续的剩余学习日（从 review_date 次日起升序），
/// 保证「未完成任务不丢失」这条硬边界在任何情况下都成立。
pub(super) fn uncompleted_tasks_fallback(
    week_plan: &mut crate::data::plan::WeekPlanFile,
    review: &crate::data::records::ReviewFile,
    regen_dates: &[String],
) -> bool {
    let unfinished: Vec<(
        crate::data::state::SubjectKey,
        crate::data::plan::TaskTemplate,
    )> = review
        .task_reviews
        .iter()
        .filter(|tr| tr.status == "incomplete" || tr.status == "partial")
        .map(|tr| {
            let subject = match tr.subject.as_str() {
                "math" => crate::data::state::SubjectKey::Math,
                "english" => crate::data::state::SubjectKey::English,
                "politics" => crate::data::state::SubjectKey::Politics,
                "professional" => crate::data::state::SubjectKey::Professional,
                _ => crate::data::state::SubjectKey::Math,
            };
            (subject, crate::core::task_progress::remaining_template(tr))
        })
        .collect();

    if unfinished.is_empty() {
        return false;
    }

    // 游标：定位重排范围内下一个有效学习日（非休息日/排除日）在 regen_dates 的索引
    let mut cursor: Option<usize> = None;
    let mut placed_any = false;
    for (subject, template) in unfinished {
        let idx = loop {
            let i = cursor.unwrap_or(0);
            if i >= regen_dates.len() {
                break None;
            }
            let date = &regen_dates[i];
            let is_study = week_plan
                .data
                .days
                .iter()
                .any(|d| d.date == *date && !d.is_rest_day);
            if is_study {
                break Some(i);
            }
            cursor = Some(i + 1);
        };

        let Some(i) = idx else { break };
        let date = regen_dates[i].clone();

        if let Some(day) = week_plan.data.days.iter_mut().find(|d| d.date == date) {
            // 放入该日对应科目 allocation 开头；uknown 科目不存在则新建
            let alloc = match day
                .subject_allocations
                .iter_mut()
                .find(|a| a.subject == subject)
            {
                Some(a) => a,
                None => {
                    day.subject_allocations.insert(
                        0,
                        crate::data::plan::DaySubjectAllocation {
                            subject,
                            hours: 0.0,
                            focus: "未完成任务（兜底安排）".to_string(),
                            task_templates: Vec::new(),
                        },
                    );
                    day.subject_allocations.first_mut().unwrap()
                }
            };
            alloc.task_templates.insert(0, template);
            placed_any = true;
            // 下一条未完成任务从下一天开始，避免同一天堆叠过多
            cursor = Some(i + 1);
        }
    }
    placed_any
}

/// 排除日任务量确定性再分摊兜底：当 AI 重排失败或返回空时，
/// 把原本排在排除日的任务逐条轮转分摊到剩余学习日（升序），
/// 保证「排除日的任务量不丢失」这条硬边界在任何情况下都成立。
///
/// 分摊策略：按「已到开始学习日期」过滤任务后，以字母轮转方式逐条放入
/// 剩余学习日对应科目的 allocation；科目 allocation 不存在则新建。
pub(super) fn exclusion_redistribute_fallback(
    week_plan: &mut crate::data::plan::WeekPlanFile,
    excluded_allocations: &[crate::data::plan::DaySubjectAllocation],
    regen_dates: &[String],
    subject_start_dates: &[(&'static str, String)],
) -> bool {
    if excluded_allocations.is_empty() {
        return false;
    }

    // 收集排除日的所有待分摊任务（含所属科目），过滤未到开始学习日期的科目
    let mut templates: Vec<(
        crate::data::state::SubjectKey,
        crate::data::plan::TaskTemplate,
    )> = Vec::new();
    for alloc in excluded_allocations {
        for t in &alloc.task_templates {
            templates.push((alloc.subject.clone(), t.clone()));
        }
    }
    if templates.is_empty() {
        return false;
    }

    // 剩余有效学习日（非休息日/排除日；排除日已被 enforce 标记为 is_rest_day=true）
    let study_days: Vec<String> = regen_dates
        .iter()
        .filter(|date| regen_dates_contains_study_day(week_plan, date))
        .cloned()
        .collect();
    if study_days.is_empty() {
        log::warn!("排除日再分摊兜底: 剩余学习日为空，无法分摊排除日任务量");
        return false;
    }

    let mut placed_any = false;
    for (i, (subject, template)) in templates.into_iter().enumerate() {
        // 该科目在目标日未到开始学习日期则跳过（由后续其它学习日 / 周计划继续容纳）
        if planner_subject_not_started(
            &subject,
            &study_days[i % study_days.len()],
            subject_start_dates,
        ) {
            log::warn!(
                "排除日再分摊兜底: 科目 {:?} 在 {} 未到开始学习日期，跳过任务「{}」",
                subject,
                study_days[i % study_days.len()],
                template.title
            );
            continue;
        }
        let date = study_days[i % study_days.len()].clone();
        let day = week_plan
            .data
            .days
            .iter_mut()
            .find(|d| d.date == date)
            .expect("study_days 由上一步校验得出，日期必然存在");
        let alloc = match day
            .subject_allocations
            .iter_mut()
            .find(|a| a.subject == subject)
        {
            Some(a) => a,
            None => {
                day.subject_allocations
                    .push(crate::data::plan::DaySubjectAllocation {
                        subject: subject.clone(),
                        hours: 0.0,
                        focus: "排除日任务量（兜底分摊）".to_string(),
                        task_templates: Vec::new(),
                    });
                day.subject_allocations.last_mut().unwrap()
            }
        };
        // 同步累加 hours，确保周计划展示与后续 prompt 引用的任务量不失真
        // （日计划用 estimated_hours，不受影响；此处保持周计划层数据一致）
        alloc.hours += template.estimated_hours;
        alloc.task_templates.push(template);
        placed_any = true;
    }
    placed_any
}

/// 判断某个重排范围日期是否为有效学习日（存在且非休息日）
pub(super) fn regen_dates_contains_study_day(
    week_plan: &crate::data::plan::WeekPlanFile,
    date: &str,
) -> bool {
    week_plan
        .data
        .days
        .iter()
        .any(|d| d.date == date && !d.is_rest_day)
}

/// 判断某科目在指定日期是否还未到开始学习日期（planner 侧，逻辑与 scheduler 一致）
pub(super) fn planner_subject_not_started(
    subject: &crate::data::state::SubjectKey,
    date: &str,
    subject_start_dates: &[(&'static str, String)],
) -> bool {
    let key = subject.key();
    for (k, start_date) in subject_start_dates {
        if *k == key && !start_date.is_empty() {
            return start_date.as_str() > date;
        }
    }
    false
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct RegenDayPlan {
    pub(super) date: String,
    #[serde(default)]
    pub(super) subject_allocations: Vec<crate::data::plan::DaySubjectAllocation>,
}

/// 从 AI 响应中解析重排后的剩余天数安排
///
/// 期望 AI 返回格式：
/// ```json
/// { "days": [ { "date": "...", "subject_allocations": [...] } ] }
/// ```
///
/// 兼容处理：AI 有时会返回完整的周计划结构 `{ version, meta, data: { days: [...] } }`，
/// 此时从 `data.days` 提取。
pub(super) fn parse_regenerate_response(
    content: &str,
    data_dir: &Path,
) -> DataResult<Vec<RegenDayPlan>> {
    let cleaned = clean_ai_json(content);

    // 方式1：尝试解析为顶层 { days: [...] }
    #[derive(serde::Deserialize)]
    struct RegenResponse {
        #[serde(default)]
        days: Vec<RegenDayPlan>,
    }

    if let Ok(parsed) = serde_json::from_str::<RegenResponse>(&cleaned) {
        if !parsed.days.is_empty() {
            return Ok(parsed.days);
        }
    }

    // 方式2：AI 返回了完整周计划结构，尝试从 data.days 提取
    #[derive(serde::Deserialize)]
    struct FullWeekPlanResponse {
        #[serde(default)]
        data: FullWeekPlanData,
    }

    #[derive(serde::Deserialize, Default)]
    struct FullWeekPlanData {
        #[serde(default)]
        days: Vec<RegenDayPlan>,
    }

    if let Ok(parsed) = serde_json::from_str::<FullWeekPlanResponse>(&cleaned) {
        if !parsed.data.days.is_empty() {
            log::info!(
                "AI 返回了完整周计划结构，已从 data.days 提取 {} 天的重排结果",
                parsed.data.days.len()
            );
            return Ok(parsed.data.days);
        }
    }

    // 两种方式都失败：记录调试日志并返回错误
    let preview: String = cleaned.chars().take(500).collect();
    let raw_preview: String = content.chars().take(1000).collect();
    crate::data::write_ai_debug_log(data_dir, "regenerate_parse_error", &format!(
        "解析 AI 重排响应失败：无法从响应中提取 days 数组。\n\ncleaned 内容前 500 字符:\n{}\n\n原始响应前 1000 字符:\n{}",
        preview,
        raw_preview,
    ));
    Err(
        "解析 AI 重排响应失败：无法从响应中提取 days 数组。AI 可能返回了非预期格式。详细日志已写入 logs/ai-debug.log"
            .to_string(),
    )
}
