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
/// 任务粒度可接受范围（小时/条）：超出该范围的用户设置视为异常，回退标准粒度。
pub(crate) const TASK_GRANULARITY_MIN_HOURS: f64 = 0.5;
pub(crate) const TASK_GRANULARITY_MAX_HOURS: f64 = 3.0;
/// 每日任务条数上限（认知负荷兜底）。
pub(crate) const MAX_DAILY_TASKS: i64 = 8;

/// 归一化用户配置的任务粒度：非法/缺省值回退 `STANDARD_GRANULARITY_HOURS`。
///
/// 所有「按粒度折算条数 / 按粒度拆分任务」的路径都必须先过这里，
/// 避免 `0`、`NaN`、负数导致除零或条数爆炸。
pub(crate) fn normalize_granularity(granularity: f64) -> f64 {
    if granularity.is_finite()
        && granularity >= TASK_GRANULARITY_MIN_HOURS
        && granularity <= TASK_GRANULARITY_MAX_HOURS
    {
        granularity
    } else {
        STANDARD_GRANULARITY_HOURS
    }
}

/// 由「每日目标学时 × 效率系数 ÷ 任务粒度」确定性派生每日任务条数。
///
/// 公式：`task_count = round(每日目标学时 × 效率 ÷ 任务粒度)`，
/// 再 clamp 到 `[活跃科目数, MAX_DAILY_TASKS]`。
/// 这样"每日 N 个任务"恒等于"N × 任务粒度"小时，含义稳定可复现；
/// 调学时或调粒度即调条数，换算确定，避免条数与时长两套指标解耦。
///
/// `granularity` 传用户设置（`AppSettings::standard_granularity()`）；
/// 非法值由 `normalize_granularity` 回退标准粒度。
pub(crate) fn derive_task_count_with_granularity(
    daily_target_hours: f64,
    granularity: f64,
    efficiency: f64,
    active_subject_count: i64,
) -> i64 {
    let target = daily_target_hours.max(0.0);
    let eff = efficiency.clamp(0.0, 1.0);
    let gran = normalize_granularity(granularity);
    let raw = (target * eff) / gran;
    let count = raw.round() as i64;
    count.clamp(active_subject_count.max(1), MAX_DAILY_TASKS)
}

/// 标准粒度版本：仅供单测与无用户粒度上下文的场景使用，
/// 生产路径一律走 `derive_task_count_with_granularity`（必须传用户设置）。
#[cfg(test)]
pub(crate) fn derive_task_count(
    daily_target_hours: f64,
    efficiency: f64,
    active_subject_count: i64,
) -> i64 {
    derive_task_count_with_granularity(
        daily_target_hours,
        STANDARD_GRANULARITY_HOURS,
        efficiency,
        active_subject_count,
    )
}

#[cfg(test)]
mod granularity_tests {
    use super::*;

    #[test]
    fn normalize_granularity_falls_back_on_invalid() {
        assert_eq!(normalize_granularity(1.0), 1.0);
        assert_eq!(normalize_granularity(0.5), 0.5);
        // 0 / 负数 / NaN / 超上限 → 回退标准粒度，不得导致除零或条数爆炸
        assert_eq!(normalize_granularity(0.0), STANDARD_GRANULARITY_HOURS);
        assert_eq!(normalize_granularity(-1.0), STANDARD_GRANULARITY_HOURS);
        assert_eq!(normalize_granularity(f64::NAN), STANDARD_GRANULARITY_HOURS);
        assert_eq!(
            normalize_granularity(TASK_GRANULARITY_MAX_HOURS + 1.0),
            STANDARD_GRANULARITY_HOURS
        );
    }

    #[test]
    fn derive_task_count_follows_user_granularity() {
        // 用户设置：每天 7h、粒度 1h/条 → 7 条（旧实现硬编码 1.5h 会得到 5 条）
        assert_eq!(derive_task_count_with_granularity(7.0, 1.0, 1.0, 1), 7);
        // 同一学时、默认 1.5h 粒度 → 5 条
        assert_eq!(derive_task_count_with_granularity(7.0, 1.5, 1.0, 1), 5);
        // 粒度更细 → 条数更多；更粗 → 更少
        assert_eq!(derive_task_count_with_granularity(6.0, 0.5, 1.0, 1), 8); // 12 → 上限 8
        assert_eq!(derive_task_count_with_granularity(6.0, 2.0, 1.0, 1), 3);
    }

    #[test]
    fn derive_task_count_with_granularity_still_clamps() {
        // 下限仍不得低于活跃科目数（每科每天至少 1 条）
        assert_eq!(derive_task_count_with_granularity(1.0, 1.0, 1.0, 4), 4);
        // 非法粒度回退标准粒度后派生，不 panic
        assert_eq!(
            derive_task_count_with_granularity(6.0, 0.0, 1.0, 1),
            derive_task_count_with_granularity(6.0, STANDARD_GRANULARITY_HOURS, 1.0, 1)
        );
    }
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

/// 每日任务数下限：不得低于「当周应有任务的科目数」，保证任务量下调时**每科每天至少 1 条任务**。
///
/// 这是「任务量调整不得抹掉某科全部任务」的确定性护栏：自适应下调
/// （`workload_factor` < 1）或用户手动减量只允许调整总量/时长，不允许把某科
/// 当天任务清零。计入条件与 `subject_task_budget` 的分配池逐条一致：
/// - 科目 `active`；
/// - 开始日期为空或不晚于 `week_end`（未开课科目本周本就不排任务）；
/// - 传入的 `allocation` 中占比必须 > 0。
///
/// 关于「占比 0」的两种来源（调用方需注意口径）：
///
/// 1. 用户在设置里把该科占比设为 0，属于显式排除，不应计入下限；
/// 2. 该科处于「截止日规划区间」时，`planner::deduct_goal_allocations` 会先把它的占比从分配池里扣掉（区间科目由目标倒排单独出任务，不占「按学习时长」份额），因此也表现为占比 0 而不计入下限。这是有意为之：护栏只兜「按学习时长」的科目。
///
/// 传 `None`（无显式占比配置）时不做占比过滤，按 active + 已开课计入。
///
/// 返回值至少为 1（异常数据下也保证计划非空）。
pub(crate) fn min_daily_task_count(
    state: &StudyState,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
    allocation: Option<&std::collections::HashMap<String, f64>>,
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
        if let Some(alloc) = allocation {
            if alloc.get(key).copied().unwrap_or(0.0) <= 0.0 {
                continue;
            }
        }
        n += 1;
    }
    n.max(1)
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
    let coeff = (0.5 + avg_rate / 200.0).clamp(0.5, 1.0);
    (coeff, avg_rate)
}

// ────────────────────────────────────────────────────────────────
// v2：近因/趋势/连续达标 三信号自校准
//
// 取代旧逻辑「整周平均完成率 → 连续系数」（见上，现仅供展示历史均值）。
// 设计文档：plan/任务数自动调整算法v2-滑动窗口与趋势.md
//   - W  窗口均值：最近 ≤5 个有效学习日，越近权重越高（最新权重 3，向前步减 0.5）；
//   - T  趋势：最近 2 日均值 − 前 3 日均值（需满 5 天，否则视为 0 = 无趋势信息）；
//   - S  连续达标：从最近一天往回数 rate ≥ 90% 的连续天数；
//   - E  精力闸门：最近 3 日精力均值（样本 < 2 视为未知，不设闸门）。
// ────────────────────────────────────────────────────────────────

/// 有效学习日样本数低于该值时不做任何调整。
pub(crate) const V2_MIN_DAYS: usize = 3;
/// 滑窗大小：最近 N 个有效学习日。
pub(crate) const V2_WINDOW_DAYS: usize = 5;
/// 连续达标阈值（%）。
pub(crate) const V2_STREAK_RATE: f64 = 90.0;
/// 单日骤降阈值（%）：仅最新一天低于它且此前稳定时触发观察保护。
pub(crate) const V2_CRASH_RATE: f64 = 70.0;
/// 明显趋势阈值（百分点）。
pub(crate) const V2_TREND_PP: f64 = 15.0;
/// 精力闸门：均值低于它时冻结上调。
pub(crate) const V2_ENERGY_GATE: f64 = 3.0;
/// 单次调整幅度上限（防抖）。
pub(crate) const V2_MAX_STEP: f64 = 0.15;
/// 系数下限：与自适应引擎既有 0.85 下限一致（文档中 D1 档 0.75 由该下限吸收）。
pub(crate) const V2_MIN_FACTOR: f64 = 0.85;
pub(crate) const V2_MAX_FACTOR: f64 = 1.15;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct V2Signals {
    /// 近 ≤5 个有效学习日加权均值（0..100）。
    pub window_mean: f64,
    /// 趋势（百分点）：满 5 天 = 近2日均 − 前3日均，否则 0。
    pub trend_pp: f64,
    /// 连续达标天数（rate ≥ 90%，从最新往回数）。
    pub streak_days: u32,
    /// 近 3 日精力均值（1..5）；样本不足时 0（未知，不设闸门）。
    pub energy_mean: f64,
    pub valid_days: usize,
    /// 单日骤降保护：最新一天 <70% 且此前 ≥3 个连续有效日 ≥90%。
    pub crash_guard: bool,
    /// 最近 2 个有效日均 <60%（持续低谷判定）。
    pub two_recent_below: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct V2Decision {
    /// 目标系数（0.85..1.15）。
    pub target: f64,
    /// 命中的规则标识（A1/A2/A3/M1/D1..D4/observe_*/keep_*），供留痕展示。
    pub rule: &'static str,
    /// +1 上调 / -1 下调 / 0 维持。
    pub direction: i8,
}

/// 输入：`rates`（每日完成率%，时间升序）、`energies`（每日精力 1..5，时间升序，缺失为 0）。
/// 仅「有效学习日」（有计划 + 有复盘 + 非外部异常/休息日）进入序列。
///
/// **不变量**：`rates` 与 `energies` 必须按同一天对齐、索引一一对应（同一有效学习日的
/// 第 i 项）。`energies` 短于 `rates` 时只影响精力均值样本数；长于 `rates` 会让尾部
/// 取到错位的日子，故调用方须构造等长序列（当前调用点都来自同一循环内 push，满足）。
pub(crate) fn v2_signals(rates: &[f64], energies: &[f64]) -> V2Signals {
    let mut signals = V2Signals {
        valid_days: rates.len(),
        ..Default::default()
    };
    if rates.is_empty() {
        return signals;
    }

    let window_n = rates.len().min(V2_WINDOW_DAYS);
    let window = &rates[rates.len() - window_n..];
    // 近因权重：最新一天 3，向前每个样本减 0.5（不足 5 天同样适用）。
    let mut weighted = 0.0f64;
    let mut weight_sum = 0.0f64;
    for (i, rate) in window.iter().enumerate() {
        let weight = 3.0 - 0.5 * (window_n as f64 - 1.0 - i as f64);
        weighted += rate * weight;
        weight_sum += weight;
    }
    signals.window_mean = if weight_sum > 0.0 {
        weighted / weight_sum
    } else {
        0.0
    };

    // 趋势：仅当满 5 个样本时可用（近 2 日均 − 前 3 日均）。
    if window_n == V2_WINDOW_DAYS {
        let first3 = window[..3].iter().sum::<f64>() / 3.0;
        let last2 = window[3..].iter().sum::<f64>() / 2.0;
        signals.trend_pp = last2 - first3;
    }

    // 连续达标（从最新往回数 ≥90%）。
    signals.streak_days = rates
        .iter()
        .rev()
        .take_while(|rate| **rate >= V2_STREAK_RATE)
        .count() as u32;

    // 最近 2 个有效日均 <60%。
    if rates.len() >= 2 {
        let n = rates.len();
        signals.two_recent_below = rates[n - 2] < 60.0 && rates[n - 1] < 60.0;
    }

    // 精力均值（最近 ≤3 个有效日；有效样本 <2 视为未知）。
    let energy_tail = energies.len().saturating_sub(3)..;
    let valid_energy: Vec<f64> = energies[energy_tail]
        .iter()
        .copied()
        .filter(|e| (1.0..=5.0).contains(e))
        .collect();
    if valid_energy.len() >= 2 {
        signals.energy_mean = valid_energy.iter().sum::<f64>() / valid_energy.len() as f64;
    }

    // 单日骤降保护：最新一天 <70%，且此前 ≥3 个连续有效日 ≥90%。
    if rates.len() >= 4 {
        let n = rates.len();
        let previous_stable = rates[n - 4..n - 1]
            .iter()
            .all(|rate| *rate >= V2_STREAK_RATE);
        if rates[n - 1] < V2_CRASH_RATE && previous_stable {
            signals.crash_guard = true;
        }
    }

    signals
}

/// 判定规则（按序命中即停，与设计文档第四节一致）。
pub(crate) fn v2_decide(signals: &V2Signals) -> V2Decision {
    fn hold(rule: &'static str) -> V2Decision {
        V2Decision {
            target: 1.0,
            rule,
            direction: 0,
        }
    }
    fn up(rule: &'static str, target: f64) -> V2Decision {
        V2Decision {
            target,
            rule,
            direction: 1,
        }
    }
    fn down(rule: &'static str, target: f64) -> V2Decision {
        V2Decision {
            target,
            rule,
            direction: -1,
        }
    }

    if signals.valid_days < V2_MIN_DAYS {
        return hold("no_adjust_insufficient_data");
    }
    if signals.crash_guard {
        return hold("observe_single_day_crash");
    }

    // —— 上调档 A（精力闸门：样本充足且均值 <3 时冻结上调）——
    let energy_ok = signals.energy_mean <= 0.0 || signals.energy_mean >= V2_ENERGY_GATE;
    if energy_ok {
        if signals.streak_days >= 7 && signals.window_mean >= 90.0 && signals.trend_pp >= 0.0 {
            return up("A1_1.15", 1.15);
        }
        if signals.streak_days >= 5 && signals.window_mean >= 90.0 {
            return up("A2_1.10", 1.10);
        }
        if signals.streak_days >= 3 && signals.window_mean >= 85.0 && signals.trend_pp >= 0.0 {
            return up("A3_1.05", 1.05);
        }
    } else if signals.streak_days >= 3 && signals.window_mean >= 85.0 {
        return hold("upgrade_blocked_low_energy");
    }

    // —— 下调档 D ——
    if signals.window_mean < 60.0 && signals.trend_pp <= 0.0 {
        return down("D1_0.85", 0.85);
    }
    if signals.window_mean < 75.0 && signals.trend_pp <= 0.0 {
        return down("D2_0.90", 0.90);
    }
    if signals.window_mean < 85.0 && signals.trend_pp <= -V2_TREND_PP {
        return down("D3_0.90", 0.90);
    }
    if signals.streak_days == 0 && signals.two_recent_below && signals.window_mean < 85.0 {
        return down("D4_0.85", 0.85);
    }

    // —— 维持档 M ——
    if signals.trend_pp >= V2_TREND_PP {
        return hold("recovering_observe");
    }
    hold("keep_1.00")
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
    allocation: Option<&std::collections::HashMap<String, f64>>,
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
        // 权重来源：配置了占比 → 用占比（0 占比 = 0 权重，缺失 key 视为 0）；
        // 未配置 → 回退各科周学时；0 学时（新开始科目）给最低权重 0.5，
        // 避免被 max(1.0) 抬成与高时长科目同权而多分任务条数。
        let weight = if let Some(allocation) = allocation {
            allocation.get(key_str).copied().unwrap_or(0.0).max(0.0)
        } else if subject.weekly_hours > 0.0 {
            subject.weekly_hours
        } else {
            0.5
        };
        weights.push((key, weight));
    }

    if weights.is_empty() {
        return Vec::new();
    }
    // 占比模式下，0 占比科目剔除出分配池（不给条数、不出现在结果）；
    // 未配置占比时 pool = weights，与旧行为逐字等价。
    let pool: Vec<(SubjectKey, f64)> = if allocation.is_some() {
        weights.into_iter().filter(|(_, w)| *w > 0.0).collect()
    } else {
        weights
    };
    if pool.is_empty() {
        return Vec::new(); // 全部 0 占比（异常配置）→ 无预算
    }
    let subject_count = pool.len() as i64;
    let mut result: Vec<(SubjectKey, i64)> = Vec::new();
    if total >= subject_count {
        for (key, _) in &pool {
            result.push((key.clone(), 1));
        }
        for (index, extra) in weighted_spread(total - subject_count, &pool)
            .iter()
            .enumerate()
        {
            result[index].1 += extra;
        }
    } else {
        let allocation = weighted_spread(total, &pool);
        for (index, (key, _)) in pool.iter().enumerate() {
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

    /// 构造 4 科全活跃的 State（周学时 14/7/5/10）
    fn allocation_state() -> StudyState {
        let mut s = StudyState {
            subjects: Default::default(),
            ..Default::default()
        };
        s.subjects.math.active = true;
        s.subjects.math.weekly_hours = 14.0;
        s.subjects.english.active = true;
        s.subjects.english.weekly_hours = 7.0;
        s.subjects.politics.active = true;
        s.subjects.politics.weekly_hours = 5.0;
        s.subjects.professional.active = true;
        s.subjects.professional.weekly_hours = 10.0;
        s
    }

    #[test]
    fn min_daily_task_count_blocks_subject_starvation() {
        let s = allocation_state();
        // 4 科活跃、无开始日期、无占比配置 → 下限 = 4（每科每天至少 1 条）
        assert_eq!(min_daily_task_count(&s, "2026-08-09", &[], None), 4);

        // 占比为 0 / 缺失 key 属用户显式排除，不计入下限
        let mut alloc = std::collections::HashMap::new();
        alloc.insert("math".to_string(), 60.0);
        alloc.insert("english".to_string(), 40.0);
        alloc.insert("politics".to_string(), 0.0);
        alloc.insert("professional".to_string(), 0.0);
        assert_eq!(min_daily_task_count(&s, "2026-08-09", &[], Some(&alloc)), 2);

        // 未开课科目（开始日期晚于 week_end）不计入下限
        let starts = vec![
            ("politics", "2026-08-20".to_string()),
            ("professional", "2026-08-30".to_string()),
        ];
        assert_eq!(
            min_daily_task_count(&s, "2026-08-09", &starts, None),
            2,
            "未开课科目不计入下限"
        );

        // 全部科目未开课（异常）→ 至少 1，避免 0 任务计划
        let all_starts = vec![
            ("math", "2026-09-01".to_string()),
            ("english", "2026-09-01".to_string()),
            ("politics", "2026-09-01".to_string()),
            ("professional", "2026-09-01".to_string()),
        ];
        assert_eq!(min_daily_task_count(&s, "2026-08-09", &all_starts, None), 1);
    }

    #[test]
    fn min_daily_task_count_satisfies_budget_pool() {
        // 下限 ≥ 分配池大小 ⇒ subject_task_budget 必走「每科保底 1 条」分支，
        // 不会出现某科被分到 0 条。
        for allocation in [
            None,
            Some({
                let mut m = std::collections::HashMap::new();
                m.insert("math".to_string(), 60.0);
                m.insert("english".to_string(), 40.0);
                m
            }),
        ] {
            let s = allocation_state();
            let floor = min_daily_task_count(&s, "2026-08-09", &[], allocation.as_ref());
            let budget = subject_task_budget(&s, floor, "2026-08-09", &[], allocation.as_ref());
            assert!(
                budget.iter().all(|(_, n)| *n >= 1),
                "下限 {} 下仍出现 0 条科目: {:?}",
                floor,
                budget
            );
            // 下调到 1 条时，正是本护栏防止「去掉某科所有任务」
            let starved = subject_task_budget(&s, 1, "2026-08-09", &[], allocation.as_ref());
            assert!(starved.iter().any(|(_, n)| *n == 0));
        }
    }

    #[test]
    fn subject_task_budget_allocation_zeros_excluded() {
        let s = allocation_state();
        let mut alloc = std::collections::HashMap::new();
        alloc.insert("math".to_string(), 60.0);
        alloc.insert("english".to_string(), 40.0);
        alloc.insert("politics".to_string(), 0.0);
        alloc.insert("professional".to_string(), 0.0);
        let budget = subject_task_budget(&s, 4, "2026-08-09", &[], Some(&alloc));
        // 0 占比科目剔除出分配池
        assert!(!budget.iter().any(|(k, _)| *k == SubjectKey::Politics));
        assert!(!budget.iter().any(|(k, _)| *k == SubjectKey::Professional));
        let sum: i64 = budget.iter().map(|(_, n)| n).sum();
        assert_eq!(sum, 4, "总条数守恒");
        // 数学占比最高，应 ≥ 英语
        let m = budget
            .iter()
            .find(|(k, _)| *k == SubjectKey::Math)
            .unwrap()
            .1;
        let e = budget
            .iter()
            .find(|(k, _)| *k == SubjectKey::English)
            .unwrap()
            .1;
        assert!(m >= e);
    }

    #[test]
    fn subject_task_budget_allocation_missing_key_zero() {
        let s = allocation_state();
        // 只有数学/英语配置了占比 → 政治/专业课缺失 key 权重 0，不进预算
        let mut alloc = std::collections::HashMap::new();
        alloc.insert("math".to_string(), 60.0);
        alloc.insert("english".to_string(), 40.0);
        let budget = subject_task_budget(&s, 4, "2026-08-09", &[], Some(&alloc));
        assert_eq!(budget.len(), 2);
        assert!(!budget.iter().any(|(k, _)| *k == SubjectKey::Politics));
        assert!(!budget.iter().any(|(k, _)| *k == SubjectKey::Professional));
    }

    #[test]
    fn subject_task_budget_allocation_all_zero_empty() {
        let s = allocation_state();
        let mut alloc = std::collections::HashMap::new();
        alloc.insert("math".to_string(), 0.0);
        alloc.insert("english".to_string(), 0.0);
        let budget = subject_task_budget(&s, 4, "2026-08-09", &[], Some(&alloc));
        assert!(budget.is_empty(), "全部 0 占比 → 无预算");
    }

    #[test]
    fn subject_task_budget_allocation_fallback_matches_weekly_hours() {
        let s = allocation_state();
        // None 与 Some(按周学时推导的占比) 应得到相同的条数分配（权重成比例等价）
        let weekly_budget = subject_task_budget(&s, 4, "2026-08-09", &[], None);
        let mut alloc = std::collections::HashMap::new();
        alloc.insert("math".to_string(), 14.0);
        alloc.insert("english".to_string(), 7.0);
        alloc.insert("politics".to_string(), 5.0);
        alloc.insert("professional".to_string(), 10.0);
        let alloc_budget = subject_task_budget(&s, 4, "2026-08-09", &[], Some(&alloc));
        assert_eq!(weekly_budget, alloc_budget);
    }

    // ── v2 三信号自校准测试 ──────────────────────────────

    /// 用户真实场景：33/71/100/100/100/100（整体约 87%）→ 不得下调，应上调。
    #[test]
    fn v2_early_low_day_does_not_downgrade_user_case() {
        let rates = [33.0, 71.0, 100.0, 100.0, 100.0, 100.0];
        let energies = [4.0, 4.0, 4.0, 4.0, 4.0, 4.0];
        let signals = v2_signals(&rates, &energies);
        // 滑窗取最近 5 个有效日：71,100,100,100,100，权重 1/1.5/2/2.5/3
        assert!(
            (signals.window_mean - 97.1).abs() < 0.05,
            "W={}",
            signals.window_mean
        );
        assert!(
            (signals.trend_pp - 9.67).abs() < 0.05,
            "T={}",
            signals.trend_pp
        );
        assert_eq!(signals.streak_days, 4);
        assert!(!signals.crash_guard);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "A3_1.05");
        assert_eq!(decision.direction, 1);
        assert!((decision.target - 1.05).abs() < 1e-9);
    }

    /// 反向：最新一天骤降（前 4 天全 100，最后 33）→ 单日骤降保护，观察不降。
    #[test]
    fn v2_single_day_crash_is_observed_not_penalized() {
        let rates = [100.0, 100.0, 100.0, 100.0, 100.0, 33.0];
        let signals = v2_signals(&rates, &[]);
        assert!(
            signals.crash_guard,
            "前 3 个连续有效日 ≥90 且最新 <70 应触发保护"
        );
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "observe_single_day_crash");
        assert_eq!(decision.direction, 0);
        assert!((decision.target - 1.0).abs() < 1e-9);
    }

    /// 持续低谷（窗口 <60 且无回升）→ D1 下限档。
    #[test]
    fn v2_persistent_low_window_downgrades() {
        let rates = [50.0, 55.0, 45.0, 52.0, 48.0];
        let signals = v2_signals(&rates, &[]);
        assert!(signals.window_mean < 60.0);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "D1_0.85");
        assert_eq!(decision.direction, -1);
    }

    /// 均值 <75 且无回升趋势 → D2。
    #[test]
    fn v2_medium_low_without_recovery_downgrades() {
        let rates = [80.0, 75.0, 70.0, 65.0, 68.0];
        let signals = v2_signals(&rates, &[]);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "D2_0.90");
    }

    /// 窗口 <85 且明显恶化（趋势 ≤ −15pp）→ D3。
    #[test]
    fn v2_clear_worsening_downgrades() {
        let rates = [95.0, 95.0, 90.0, 70.0, 55.0];
        let signals = v2_signals(&rates, &[]);
        assert!(signals.trend_pp <= -15.0);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "D3_0.90");
    }

    /// 明显回升（趋势 ≥ +15）→ 观察一天，不降不加。
    #[test]
    fn v2_recovering_trend_holds() {
        let rates = [40.0, 45.0, 50.0, 95.0, 100.0];
        let signals = v2_signals(&rates, &[]);
        assert!(signals.trend_pp >= 15.0);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "recovering_observe");
        assert_eq!(decision.direction, 0);
    }

    /// 数据不足（<3 个有效学习日）→ 不做任何调整。
    #[test]
    fn v2_insufficient_data_holds() {
        let rates = [100.0, 100.0];
        let signals = v2_signals(&rates, &[]);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "no_adjust_insufficient_data");
        assert!((decision.target - 1.0).abs() < 1e-9);
    }

    /// 精力闸门：连续达标但近 3 日精力 <3 → 冻结上调。
    #[test]
    fn v2_low_energy_blocks_upgrade() {
        let rates = [90.0, 100.0, 100.0, 100.0, 100.0];
        let energies = [2.0, 2.0, 2.0, 2.0, 2.0];
        let signals = v2_signals(&rates, &energies);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "upgrade_blocked_low_energy");
        assert_eq!(decision.direction, 0);
    }

    /// 一周满勤 7 天 → A1 上限档。
    #[test]
    fn v2_full_week_reaches_top_tier() {
        let rates = [100.0; 7];
        let energies = [4.0; 7];
        let signals = v2_signals(&rates, &energies);
        assert_eq!(signals.streak_days, 7);
        let decision = v2_decide(&signals);
        assert_eq!(decision.rule, "A1_1.15");
    }
}
