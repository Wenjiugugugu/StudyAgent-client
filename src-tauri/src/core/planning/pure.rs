//! Deterministic planning rules.

use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, SubjectKey};

pub(crate) fn weekly_self_calibration(prev_week_reviews: &[ReviewFile]) -> f64 {
    if prev_week_reviews.is_empty() {
        return 1.0;
    }
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for review in prev_week_reviews {
        let has_tasks = !review.task_reviews.is_empty()
            || review.data.completion.priority_a_total > 0
            || review.data.completion.priority_b_total > 0;
        if !has_tasks {
            continue;
        }
        let (_, _, _, _, rate) = crate::data::records::review_completion_stats(review);
        sum += rate;
        count += 1;
    }
    if count == 0 {
        return 1.0;
    }
    let avg_rate = sum / count as f64;
    if avg_rate >= 90.0 {
        1.0
    } else if avg_rate >= 70.0 {
        0.9
    } else {
        0.8
    }
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
    let has_weak_mastery = review.task_reviews.iter().any(|task| task.mastery == "weak");
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
