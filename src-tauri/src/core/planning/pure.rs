//! Deterministic planning rules.

use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, SubjectKey};

// ── 任务粒度与条数派生常量 ──

/// 标准任务粒度（小时/条）：默认 1.5h ≈ 2 个番茄钟（学习 50min×2 + 短休）。
pub(crate) const STANDARD_GRANULARITY_HOURS: f64 = 1.5;
/// 单条任务时长下限（小时）：低于该值且为同动作小项时合并。
pub(crate) const TASK_MIN_HOURS: f64 = 0.5;
/// 单条任务时长上限（小时）：超过则必须拆分。
pub(crate) const TASK_MAX_HOURS: f64 = 3.0;
/// 每日任务条数上限（认知负荷兜底）。
pub(crate) const MAX_DAILY_TASKS: i64 = 8;

/// 由「每日目标学时 × 效率系数」确定性派生每日任务条数。
///
/// 公式：`task_count = round(每日目标学时 × 效率 ÷ 标准任务粒度)`，
/// 再 clamp 到 `[活跃科目数, MAX_DAILY_TASKS]`。
/// 这样"每日 N 个任务"恒等于"N × 标准粒度"小时，含义稳定可复现；
/// 调学时即调条数，换算确定，避免条数与时长两套指标解耦。
pub(crate) fn derive_task_count(
    daily_target_hours: f64,
    efficiency: f64,
    active_subject_count: i64,
) -> i64 {
    let target = daily_target_hours.max(0.0);
    let eff = efficiency.clamp(0.0, 1.0);
    let raw = (target * eff) / STANDARD_GRANULARITY_HOURS;
    let count = raw.round() as i64;
    count.clamp(active_subject_count.max(1), MAX_DAILY_TASKS)
}

/// 活跃科目数：科目 active 且（未设置开始日期 或 开始日期不晚于 week_end）。
pub(crate) fn active_subject_count_for(
    state: &StudyState,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
) -> i64 {
    let subjects = [
        ("math", &state.subjects.math),
        ("english", &state.subjects.english),
        ("politics", &state.subjects.politics),
        ("professional", &state.subjects.professional),
    ];
    let mut n = 0i64;
    for (key, subj) in subjects {
        if !subj.active {
            continue;
        }
        if let Some((_, start)) = subject_start_dates.iter().find(|(k, _)| *k == key) {
            if !start.is_empty() && start.as_str() > week_end {
                continue;
            }
        }
        n += 1;
    }
    n.max(1)
}

pub(crate) fn weekly_self_calibration(prev_week_reviews: &[ReviewFile]) -> f64 {
    prev_week_calibration_stats_impl(prev_week_reviews).0
}

/// 每周自校准统计：返回 (系数, 上周复盘平均完成率%)。
///
/// 平均完成率按任务数加权（总完成 / 总计划），
/// 不能对每日完成率做简单算术平均：任务少但全完成的天（100%）会等权拉高整体，
/// 导致实际完成 8/23 却计算出虚高的 72%。
/// 仅统计有效复盘（有逐任务记录或 completion 汇总数据）。
pub(crate) fn prev_week_calibration_stats_impl(prev_week_reviews: &[ReviewFile]) -> (f64, f64) {
    let mut sum_done = 0i32;
    let mut sum_total = 0i32;
    for review in prev_week_reviews {
        // 跳过低质量复盘（既无逐任务记录也无 completion 汇总数据）
        let has_tasks = !review.task_reviews.is_empty()
            || review.data.completion.priority_a_total > 0
            || review.data.completion.priority_b_total > 0;
        if !has_tasks {
            continue;
        }
        let (a_total, a_done, b_total, b_done, _) =
            crate::data::records::review_completion_stats(review);
        sum_total += a_total + b_total;
        sum_done += a_done + b_done;
    }
    if sum_total == 0 {
        return (1.0, 0.0);
    }
    let avg_rate = (sum_done as f64 / sum_total as f64) * 100.0;
    let coeff = if avg_rate >= 90.0 {
        1.0
    } else if avg_rate >= 70.0 {
        0.9
    } else {
        0.8
    };
    (coeff, avg_rate)
}

pub(crate) fn today_intensity_label(reviews: &[ReviewFile]) -> String {
    if reviews.is_empty() {
        return String::new();
    }
    let mut list: Vec<&ReviewFile> = reviews.iter().collect();
    list.sort_by(|a, b| b.meta.date.cmp(&a.meta.date));
    let recent = list.into_iter().take(7);

    let mut rate_sum = 0.0f64;
    let mut rate_n = 0usize;
    let mut energy_sum = 0i32;
    let mut energy_n = 0usize;
    for review in recent {
        let has_tasks = !review.task_reviews.is_empty()
            || review.data.completion.priority_a_total > 0
            || review.data.completion.priority_b_total > 0;
        if has_tasks {
            let (_, _, _, _, rate) = crate::data::records::review_completion_stats(review);
            rate_sum += rate;
            rate_n += 1;
        }
        energy_sum += review.data.energy_level.max(1);
        energy_n += 1;
    }
    let avg_rate = if rate_n > 0 {
        rate_sum / rate_n as f64
    } else {
        100.0
    };
    let avg_energy = if energy_n > 0 {
        energy_sum as f64 / energy_n as f64
    } else {
        3.0
    };

    if avg_rate < 60.0 || avg_energy <= 1.5 {
        format!(
            "今日强度建议：偏轻（近期完成率偏低 / 精力不足，优先完成而非加量）。完成率均值 {:.0}%，精力均值 {:.1}/5。",
            avg_rate, avg_energy
        )
    } else if avg_rate >= 90.0 && avg_energy >= 4.0 {
        format!(
            "今日强度建议：可加量（近期完成度高且精力充沛）。完成率均值 {:.0}%，精力均值 {:.1}/5。",
            avg_rate, avg_energy
        )
    } else if avg_rate < 75.0 {
        format!(
            "今日强度建议：适中（近期完成率一般，保持节奏）。完成率均值 {:.0}%，精力均值 {:.1}/5。",
            avg_rate, avg_energy
        )
    } else {
        format!(
            "今日强度建议：正常。完成率均值 {:.0}%，精力均值 {:.1}/5。",
            avg_rate, avg_energy
        )
    }
}

pub(crate) fn subject_key_str(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "math",
        SubjectKey::English => "english",
        SubjectKey::Politics => "politics",
        SubjectKey::Professional => "professional",
    }
}

pub(crate) fn subject_cn(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}

pub(crate) fn weighted_spread(total: i64, weights: &[(SubjectKey, f64)]) -> Vec<i64> {
    if weights.is_empty() {
        return Vec::new();
    }
    if total <= 0 {
        return weights.iter().map(|_| 0).collect();
    }
    let wsum: f64 = weights.iter().map(|(_, weight)| *weight).sum();
    if wsum <= 0.0 {
        let base = total / weights.len() as i64;
        let rem = (total as usize) % weights.len();
        return weights
            .iter()
            .enumerate()
            .map(|(index, _)| base + if index < rem { 1 } else { 0 })
            .collect();
    }

    let shares: Vec<f64> = weights
        .iter()
        .map(|(_, weight)| (weight / wsum) * total as f64)
        .collect();
    let mut allocation: Vec<i64> = shares.iter().map(|share| share.floor() as i64).collect();
    let granted: i64 = allocation.iter().sum();
    let mut remaining = total - granted;

    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| {
        let fraction_a = shares[a] - shares[a].floor();
        let fraction_b = shares[b] - shares[b].floor();
        fraction_b
            .partial_cmp(&fraction_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });

    let mut index = 0usize;
    while remaining > 0 {
        allocation[order[index % order.len()]] += 1;
        remaining -= 1;
        index += 1;
    }
    allocation
}

pub(crate) fn subject_task_budget(
    state: &StudyState,
    total: i64,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
) -> Vec<(SubjectKey, i64)> {
    let subjects = [
        (SubjectKey::Math, &state.subjects.math),
        (SubjectKey::English, &state.subjects.english),
        (SubjectKey::Politics, &state.subjects.politics),
        (SubjectKey::Professional, &state.subjects.professional),
    ];
    let mut weights: Vec<(SubjectKey, f64)> = Vec::new();
    for (key, subject) in subjects {
        if !subject.active {
            continue;
        }
        let key_str = subject_key_str(&key);
        if let Some((_, start_date)) = subject_start_dates
            .iter()
            .find(|(candidate, date)| *candidate == key_str && !date.is_empty())
        {
            if start_date.as_str() > week_end {
                continue;
            }
        }
        weights.push((key, subject.weekly_hours.max(1.0)));
    }

    if weights.is_empty() {
        return Vec::new();
    }
    let subject_count = weights.len() as i64;
    let mut result: Vec<(SubjectKey, i64)> = Vec::new();
    if total >= subject_count {
        for (key, _) in &weights {
            result.push((key.clone(), 1));
        }
        for (index, extra) in weighted_spread(total - subject_count, &weights)
            .iter()
            .enumerate()
        {
            result[index].1 += extra;
        }
    } else {
        let allocation = weighted_spread(total, &weights);
        for (index, (key, _)) in weights.iter().enumerate() {
            result.push((key.clone(), allocation[index]));
        }
    }
    result
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryReviewItem {
    pub(crate) due_date: String,
    pub(crate) subject: SubjectKey,
    pub(crate) title: String,
}

pub(crate) fn memory_curve_review_items(
    reviews: &[ReviewFile],
    week_start: &str,
    week_end: &str,
) -> Vec<MemoryReviewItem> {
    const INTERVALS: [i64; 3] = [1, 3, 7];
    let mut result: Vec<MemoryReviewItem> = Vec::new();
    for review in reviews {
        for task_review in &review.task_reviews {
            if task_review.mastery != "weak" {
                continue;
            }
            let subject = match task_review.subject.as_str() {
                "math" => SubjectKey::Math,
                "english" => SubjectKey::English,
                "politics" => SubjectKey::Politics,
                "professional" => SubjectKey::Professional,
                _ => continue,
            };
            let title = if task_review.title.trim().is_empty() {
                "薄弱内容".to_string()
            } else {
                task_review.title.trim().to_string()
            };
            for interval in INTERVALS {
                let Ok(due_date) = crate::data::add_days(&review.meta.date, interval) else {
                    continue;
                };
                if due_date.as_str() >= week_start && due_date.as_str() <= week_end {
                    result.push(MemoryReviewItem {
                        due_date,
                        subject: subject.clone(),
                        title: format!("{}（+{}天回访）", title, interval),
                    });
                }
            }
        }
    }
    result.sort_by(|a, b| {
        a.due_date
            .cmp(&b.due_date)
            .then_with(|| subject_key_str(&a.subject).cmp(subject_key_str(&b.subject)))
            .then_with(|| a.title.cmp(&b.title))
    });
    result
}

pub(crate) fn check_review_needs_regeneration(review: &ReviewFile) -> bool {
    let has_uncompleted = review
        .task_reviews
        .iter()
        .any(|task| task.status == "incomplete" || task.status == "partial");
    let has_weak_mastery = review
        .task_reviews
        .iter()
        .any(|task| task.mastery == "weak");
    let feels_hard = review
        .daily_review
        .as_ref()
        .map(|daily| daily.overall_feeling == "hard")
        .unwrap_or(false);
    let has_overcompletion = !review.overcompletion.is_empty();
    has_uncompleted || has_weak_mastery || feels_hard || has_overcompletion
}

pub(crate) fn matches_completed(title: &str, completed: &str) -> bool {
    let title = title.trim();
    let completed = completed.trim();
    if title.is_empty() || completed.is_empty() {
        return false;
    }
    if title == completed {
        return true;
    }
    title
        .strip_prefix(completed)
        .and_then(|rest| rest.chars().next())
        .map(|character| {
            matches!(
                character,
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
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_task_count_basic() {
        // 5h 目标 ÷ 1.5h 粒度 ≈ 3.33 → 3
        assert_eq!(derive_task_count(5.0, 1.0, 2), 3);
        // 6h ÷ 1.5 = 4
        assert_eq!(derive_task_count(6.0, 1.0, 2), 4);
        // 3h ÷ 1.5 = 2
        assert_eq!(derive_task_count(3.0, 1.0, 2), 2);
    }

    #[test]
    fn derive_task_count_applies_efficiency() {
        // 效率 0.8：5×0.8=4h ÷1.5 ≈ 2.67 → 3
        assert_eq!(derive_task_count(5.0, 0.8, 2), 3);
        // 效率 0.9：5×0.9=4.5h ÷1.5 = 3
        assert_eq!(derive_task_count(5.0, 0.9, 2), 3);
        // 效率 0（异常）→ 0 → clamp 到活跃科目数
        assert_eq!(derive_task_count(5.0, 0.0, 2), 2);
    }

    #[test]
    fn derive_task_count_clamps() {
        // 下限：目标太小 → clamp 到活跃科目数
        assert_eq!(derive_task_count(0.5, 1.0, 2), 2);
        assert_eq!(derive_task_count(0.0, 1.0, 3), 3);
        // 上限：目标太大 → clamp 到 8
        assert_eq!(derive_task_count(20.0, 1.0, 2), 8);
        // 活跃科目数为 0（异常）→ 至少 1
        assert_eq!(derive_task_count(5.0, 1.0, 0), 3);
    }
}
