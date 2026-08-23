//! Analytics — 学习数据分析聚合器
//!
//! 从 Plan / Review / State 中读取数据并聚合为分析视图所需的结构化数据。
//! 支持三类分析：
//! 1. 学习量趋势（完成率、学习时长、任务量时序）
//! 2. 复盘质量分析（掌握度分布、阻碍因素、感受曲线、困难类型）
//! 3. 周期对比与预测（本周 vs 上周、本月 vs 上月、目标达成预测）

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::data::records::{self, ReviewFile};
use crate::data::{add_days, get_week_end, get_week_start, today_string, weekday_name, DataResult};

// ============================================================================
// 类型定义
// ============================================================================

/// 分析时间范围
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRange {
    Last7Days,
    #[default]
    Last30Days,
    All,
}

/// 每日学习量数据点
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyTrendPoint {
    pub date: String,
    /// 完成率（0-100）
    pub completion_rate: f64,
    /// 计划任务数
    pub planned_tasks: i32,
    /// 已完成任务数
    pub completed_tasks: i32,
    /// 计划学习时长（小时）
    pub planned_hours: f64,
    /// 实际学习时长（小时）
    pub actual_hours: f64,
}

/// 学习量趋势统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningTrend {
    pub points: Vec<DailyTrendPoint>,
    /// 平均完成率
    pub avg_completion_rate: f64,
    /// 累计学习时长
    pub total_actual_hours: f64,
    /// 累计计划学习时长
    pub total_planned_hours: f64,
    /// 累计任务数
    pub total_planned_tasks: i32,
    /// 累计完成任务数
    pub total_completed_tasks: i32,
    /// 学习天数（有实际时长>0 的天数）
    pub study_days: i32,
}

/// 掌握度分布项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MasteryDistribution {
    pub mastered: i32,
    pub basic: i32,
    pub weak: i32,
    pub not_marked: i32,
}

/// 阻碍因素统计项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockerItem {
    pub key: String,
    pub label: String,
    pub count: i32,
}

/// 感受曲线数据点
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeelingPoint {
    pub date: String,
    /// smooth=3, normal=2, hard=1
    pub score: i32,
    pub label: String,
}

/// 困难类型分布项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DifficultyItem {
    pub key: String,
    pub label: String,
    pub count: i32,
}

/// 复盘质量分析
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewQuality {
    pub mastery: MasteryDistribution,
    pub blockers: Vec<BlockerItem>,
    pub feelings: Vec<FeelingPoint>,
    pub difficulties: Vec<DifficultyItem>,
    /// 有效复盘天数
    pub review_count: i32,
}

/// 周期对比指标
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeriodMetrics {
    /// 平均完成率
    pub avg_completion_rate: f64,
    /// 总学习时长
    pub total_hours: f64,
    /// 总任务数
    pub total_tasks: i32,
    /// 总完成任务数
    pub total_completed: i32,
    /// 学习天数
    pub study_days: i32,
}

/// 周期对比结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeriodComparison {
    pub current: PeriodMetrics,
    pub previous: PeriodMetrics,
    /// 当前周期标签（如 "本周" / "本月"）
    pub current_label: String,
    pub previous_label: String,
    /// 完成率变化（百分点，正数表示提升）
    pub completion_rate_delta: f64,
    /// 学习时长变化（小时）
    pub hours_delta: f64,
    /// 任务量变化
    pub tasks_delta: i32,
}

/// 目标达成预测
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalPrediction {
    /// 近7天平均完成率
    pub recent_avg_completion_rate: f64,
    /// 近7天平均每日学习时长
    pub recent_avg_daily_hours: f64,
    /// 基于近7天完成率推算的预期完成率
    pub expected_completion_rate: f64,
    /// 预测状态：on_track / at_risk / off_track
    pub status: String,
    /// 状态描述
    pub description: String,
}

/// 周期对比与预测
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComparisonAndPrediction {
    pub week_comparison: PeriodComparison,
    pub month_comparison: PeriodComparison,
    pub prediction: GoalPrediction,
}

/// 完整分析数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub range: String,
    pub learning_trend: LearningTrend,
    pub review_quality: ReviewQuality,
    pub comparison: ComparisonAndPrediction,
}

// ============================================================================
// 阻碍因素与困难类型的中文标签
// ============================================================================

fn blocker_label(key: &str) -> String {
    match key {
        "time" => "时间不足".to_string(),
        "understanding" => "理解困难".to_string(),
        "practice" => "练习不足".to_string(),
        "memorization" => "遗忘较多".to_string(),
        "overload" => "工作量安排过多".to_string(),
        "interruption" => "临时事务".to_string(),
        "energy" => "今天状态不好".to_string(),
        "resource" => "资源不足".to_string(),
        "other" => "其它".to_string(),
        _ => key.to_string(),
    }
}

fn difficulty_label(key: &str) -> String {
    match key {
        "understanding" => "理解概念".to_string(),
        "problems" => "做题".to_string(),
        "memorization" => "记忆".to_string(),
        "attention" => "注意力".to_string(),
        "time_management" => "时间安排".to_string(),
        "environment" => "学习环境".to_string(),
        "other" => "其它".to_string(),
        _ => key.to_string(),
    }
}

fn feeling_score(feeling: &str) -> (i32, String) {
    match feeling {
        "smooth" => (3, "很顺利".to_string()),
        "normal" => (2, "一般".to_string()),
        "hard" => (1, "比较困难".to_string()),
        _ => (2, "一般".to_string()),
    }
}

// ============================================================================
// 聚合主函数
// ============================================================================

/// 收集所有应从分析中排除的日期（休息日 + 特殊情况排除日）
///
/// 休息日：根据用户设置的 `rest_days`（如"周日"）匹配日期范围内的日期
/// 排除日：从所有周计划文件的 `excluded_days` 中收集
fn collect_exempt_dates(data_dir: &Path, start: &str, end: &str) -> HashSet<String> {
    let mut exempt: HashSet<String> = HashSet::new();

    // 1. 休息日：读取设置并遍历日期范围
    let settings = crate::load_settings(data_dir);
    let rest_days = settings.rest_days();
    if !rest_days.is_empty() {
        let mut current = start.to_string();
        loop {
            if let Ok(wd) = weekday_name(&current) {
                if rest_days.contains(&wd) {
                    exempt.insert(current.clone());
                }
            }
            if current == end {
                break;
            }
            current = match add_days(&current, 1) {
                Ok(d) => d,
                Err(_) => break,
            };
        }
    }

    // 2. 排除日：从所有周计划中收集
    if let Ok(week_dates) = crate::data::plan::list_week_plan_dates(data_dir) {
        for iso_week in &week_dates {
            if let Ok(wp) = crate::data::plan::read_week_plan(data_dir, iso_week) {
                for ex in &wp.data.excluded_days {
                    exempt.insert(ex.date.clone());
                }
            }
        }
    }

    exempt
}

/// 获取指定时间范围的分析数据
///
/// `exclude_exempt`：是否在「学习量趋势」中排除休息日和特殊情况排除日（默认 true）。
/// 仅影响 learning_trend（完成率与任务量、学习时长两个图表）；
/// 复盘质量分析与周期对比基于 reviews，休息日/排除日本身无复盘，天然不受影响。
pub fn build_analytics(
    data_dir: &Path,
    range: &AnalyticsRange,
    exclude_exempt: bool,
) -> DataResult<AnalyticsSummary> {
    let today = today_string();

    // 1. 确定日期范围
    let (start_date, range_label) = match range {
        AnalyticsRange::Last7Days => {
            let s = add_days(&today, -6)?;
            (s, "last_7_days".to_string())
        }
        AnalyticsRange::Last30Days => {
            let s = add_days(&today, -29)?;
            (s, "last_30_days".to_string())
        }
        AnalyticsRange::All => {
            // 从最早的 review/plan 开始
            let earliest = find_earliest_date(data_dir)?;
            (earliest, "all".to_string())
        }
    };

    // 收集应排除的日期（休息日 + 排除日），仅用于学习量趋势
    // 当 exclude_exempt=false 时（用户在分析页关闭开关），使用空集保留所有日期
    let exempt_dates: HashSet<String> = if exclude_exempt {
        collect_exempt_dates(data_dir, &start_date, &today)
    } else {
        HashSet::new()
    };

    // 2. 收集范围内的所有复盘（不过滤 —— 休息日/排除日本身无复盘，天然不含）
    let reviews = collect_reviews_in_range(data_dir, &start_date, &today)?;

    // 3. 学习量趋势（受 exclude_exempt 控制，跳过豁免日）
    let learning_trend =
        build_learning_trend(data_dir, &start_date, &today, &reviews, &exempt_dates)?;

    // 4. 复盘质量分析（基于 reviews，不受排除开关影响）
    let review_quality = build_review_quality(&reviews);

    // 5. 周期对比与预测（基于 reviews，不受排除开关影响）
    let comparison = build_comparison_and_prediction(data_dir, &today, &reviews)?;

    Ok(AnalyticsSummary {
        range: range_label,
        learning_trend,
        review_quality,
        comparison,
    })
}

// ============================================================================
// 学习量趋势
// ============================================================================

fn build_learning_trend(
    data_dir: &Path,
    start: &str,
    end: &str,
    reviews: &[ReviewFile],
    exempt_dates: &HashSet<String>,
) -> DataResult<LearningTrend> {
    let mut points: Vec<DailyTrendPoint> = Vec::new();

    // 构建复盘索引：date -> review
    let mut review_map: HashMap<String, &ReviewFile> = HashMap::new();
    for r in reviews {
        review_map.insert(r.meta.date.clone(), r);
    }

    let mut current = start.to_string();
    loop {
        // 跳过豁免日期（休息日 + 特殊情况排除日），不计入趋势
        if exempt_dates.contains(&current) {
            if current == end {
                break;
            }
            current = add_days(&current, 1)?;
            continue;
        }

        let plan = crate::data::plan::read_daily_plan(data_dir, &current).ok();
        let review = review_map.get(&current).copied();

        let planned_tasks = plan.as_ref().map(|p| p.data.total_tasks).unwrap_or(0);
        let planned_hours = plan.as_ref().map(|p| p.data.total_hours).unwrap_or(0.0);
        // 实际学习时长：优先复盘记录；当天未复盘时，读取任务实际计时（含未完成任务）
        // 与未关联任务的番茄钟专注分钟，保证无论是否关联任务，专注时间都被计入。
        let actual_hours = match review {
            Some(r) => crate::data::records::review_actual_hours(r),
            None => crate::data::state::read_state(data_dir)
                .map(|state| {
                    (crate::data::state::day_actual_minutes(&state, &current)
                        + crate::data::focus::day_unlinked_focus_minutes(data_dir, &current))
                        as f64
                        / 60.0
                })
                .unwrap_or_else(|_| {
                    crate::data::focus::day_unlinked_focus_minutes(data_dir, &current) as f64 / 60.0
                }),
        };

        // 完成率：优先从 task_reviews 计算
        let (completed_tasks, completion_rate) = compute_completion(review);

        points.push(DailyTrendPoint {
            date: current.clone(),
            completion_rate,
            planned_tasks,
            completed_tasks,
            planned_hours,
            actual_hours,
        });

        if current == end {
            break;
        }
        current = add_days(&current, 1)?;
    }

    // 聚合统计
    let total_planned_tasks: i32 = points.iter().map(|p| p.planned_tasks).sum();
    let total_completed_tasks: i32 = points.iter().map(|p| p.completed_tasks).sum();
    let total_planned_hours: f64 = points.iter().map(|p| p.planned_hours).sum();
    let total_actual_hours: f64 = points.iter().map(|p| p.actual_hours).sum();
    let study_days = points.iter().filter(|p| p.actual_hours > 0.0).count() as i32;

    let days_with_plan: Vec<&DailyTrendPoint> =
        points.iter().filter(|p| p.planned_tasks > 0).collect();
    let avg_completion_rate = if days_with_plan.is_empty() {
        0.0
    } else {
        days_with_plan
            .iter()
            .map(|p| p.completion_rate)
            .sum::<f64>()
            / days_with_plan.len() as f64
    };

    Ok(LearningTrend {
        points,
        avg_completion_rate,
        total_actual_hours,
        total_planned_hours,
        total_planned_tasks,
        total_completed_tasks,
        study_days,
    })
}

/// 计算单日完成率（优先 A 级，回退全部）
fn compute_completion(review: Option<&ReviewFile>) -> (i32, f64) {
    match review {
        Some(r) => {
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
                    (all_done as f64 / all_total as f64) * 100.0
                } else {
                    0.0
                };
                (completed, rate)
            } else {
                let a_total = r.data.completion.priority_a_total;
                let a_done = r.data.completion.priority_a_done;
                let rate = if a_total > 0 {
                    (a_done as f64 / a_total as f64) * 100.0
                } else if r.data.completion.priority_b_total > 0 {
                    let b_total = r.data.completion.priority_b_total;
                    let b_done = r.data.completion.priority_b_done;
                    (b_done as f64 / b_total as f64) * 100.0
                } else {
                    100.0
                };
                (a_done, rate)
            }
        }
        None => (0, 0.0),
    }
}

// ============================================================================
// 复盘质量分析
// ============================================================================

fn build_review_quality(reviews: &[ReviewFile]) -> ReviewQuality {
    let mut mastery = MasteryDistribution::default();
    let mut blocker_counts: HashMap<String, i32> = HashMap::new();
    let mut feelings: Vec<FeelingPoint> = Vec::new();
    let mut difficulty_counts: HashMap<String, i32> = HashMap::new();

    // 按日期升序排列
    let mut sorted_reviews: Vec<&ReviewFile> = reviews.iter().collect();
    sorted_reviews.sort_by(|a, b| a.meta.date.cmp(&b.meta.date));

    for r in &sorted_reviews {
        // 掌握度统计
        for tr in &r.task_reviews {
            match tr.mastery.as_str() {
                "mastered" => mastery.mastered += 1,
                "basic" => mastery.basic += 1,
                "weak" => mastery.weak += 1,
                _ => mastery.not_marked += 1,
            }

            // 阻碍因素统计
            for b in &tr.blockers {
                *blocker_counts.entry(b.clone()).or_insert(0) += 1;
            }
        }

        // 感受曲线
        if let Some(dr) = &r.daily_review {
            let (score, label) = feeling_score(&dr.overall_feeling);
            feelings.push(FeelingPoint {
                date: r.meta.date.clone(),
                score,
                label,
            });

            // 困难类型统计
            if !dr.main_difficulty.is_empty() {
                *difficulty_counts
                    .entry(dr.main_difficulty.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    // 阻碍因素排序并转中文标签
    let mut blockers: Vec<BlockerItem> = blocker_counts
        .iter()
        .map(|(k, v)| BlockerItem {
            key: k.clone(),
            label: blocker_label(k),
            count: *v,
        })
        .collect();
    blockers.sort_by_key(|a| std::cmp::Reverse(a.count));

    // 困难类型转中文标签
    let mut difficulties: Vec<DifficultyItem> = difficulty_counts
        .iter()
        .map(|(k, v)| DifficultyItem {
            key: k.clone(),
            label: difficulty_label(k),
            count: *v,
        })
        .collect();
    difficulties.sort_by_key(|a| std::cmp::Reverse(a.count));

    ReviewQuality {
        mastery,
        blockers,
        feelings,
        difficulties,
        review_count: reviews.len() as i32,
    }
}

// ============================================================================
// 周期对比与预测
// ============================================================================

fn build_comparison_and_prediction(
    data_dir: &Path,
    today: &str,
    reviews: &[ReviewFile],
) -> DataResult<ComparisonAndPrediction> {
    // 本周 vs 上周
    let week_start = get_week_start(today)?;
    let week_end = get_week_end(today)?;
    let prev_week_start = add_days(&week_start, -7)?;
    let prev_week_end = add_days(&week_start, -1)?;

    let week_comparison = build_period_comparison(
        data_dir,
        &week_start,
        &week_end,
        &prev_week_start,
        &prev_week_end,
        "本周",
        "上周",
    )?;

    // 本月 vs 上月
    let (month_start, month_end) = current_month_range(today);
    let (prev_month_start, prev_month_end) = prev_month_range(today);
    let month_comparison = build_period_comparison(
        data_dir,
        &month_start,
        &month_end,
        &prev_month_start,
        &prev_month_end,
        "本月",
        "上月",
    )?;

    // 预测
    let prediction = build_prediction(reviews, today);

    Ok(ComparisonAndPrediction {
        week_comparison,
        month_comparison,
        prediction,
    })
}

fn build_period_comparison(
    data_dir: &Path,
    cur_start: &str,
    cur_end: &str,
    prev_start: &str,
    prev_end: &str,
    cur_label: &str,
    prev_label: &str,
) -> DataResult<PeriodComparison> {
    let current = compute_period_metrics(data_dir, cur_start, cur_end)?;
    let previous = compute_period_metrics(data_dir, prev_start, prev_end)?;

    let completion_rate_delta = current.avg_completion_rate - previous.avg_completion_rate;
    let hours_delta = current.total_hours - previous.total_hours;
    let tasks_delta = current.total_tasks - previous.total_tasks;

    Ok(PeriodComparison {
        current,
        previous,
        current_label: cur_label.to_string(),
        previous_label: prev_label.to_string(),
        completion_rate_delta,
        hours_delta,
        tasks_delta,
    })
}

fn compute_period_metrics(data_dir: &Path, start: &str, end: &str) -> DataResult<PeriodMetrics> {
    let reviews = collect_reviews_in_range(data_dir, start, end)?;

    let mut total_hours = 0.0;
    let mut total_tasks = 0i32;
    let mut total_completed = 0i32;
    let mut completion_rates: Vec<f64> = Vec::new();
    let mut study_days = 0i32;

    for r in &reviews {
        let hours = crate::data::records::review_actual_hours(r);
        total_hours += hours;
        if hours > 0.0 {
            study_days += 1;
        }

        if !r.task_reviews.is_empty() {
            total_tasks += r.task_reviews.len() as i32;
            total_completed += r
                .task_reviews
                .iter()
                .filter(|t| t.status == "completed")
                .count() as i32;

            let (a_total, a_done) = r
                .task_reviews
                .iter()
                .filter(|t| t.priority == "A")
                .fold((0, 0), |(t, d), tr| {
                    (t + 1, d + if tr.status == "completed" { 1 } else { 0 })
                });
            let rate = if a_total > 0 {
                (a_done as f64 / a_total as f64) * 100.0
            } else if !r.task_reviews.is_empty() {
                let done = r
                    .task_reviews
                    .iter()
                    .filter(|t| t.status == "completed")
                    .count();
                (done as f64 / r.task_reviews.len() as f64) * 100.0
            } else {
                0.0
            };
            completion_rates.push(rate);
        }
    }

    let avg_completion_rate = if completion_rates.is_empty() {
        0.0
    } else {
        completion_rates.iter().sum::<f64>() / completion_rates.len() as f64
    };

    Ok(PeriodMetrics {
        avg_completion_rate,
        total_hours,
        total_tasks,
        total_completed,
        study_days,
    })
}

fn build_prediction(reviews: &[ReviewFile], today: &str) -> GoalPrediction {
    // 近7天数据
    let seven_days_ago = add_days(today, -6).unwrap_or_else(|_| today.to_string());
    let recent: Vec<&ReviewFile> = reviews
        .iter()
        .filter(|r| {
            r.meta.date.as_str() >= seven_days_ago.as_str() && r.meta.date.as_str() <= today
        })
        .collect();

    if recent.is_empty() {
        return GoalPrediction {
            recent_avg_completion_rate: 0.0,
            recent_avg_daily_hours: 0.0,
            expected_completion_rate: 0.0,
            status: "no_data".to_string(),
            description: "近7天暂无复盘数据，无法预测".to_string(),
        };
    }

    let mut rates: Vec<f64> = Vec::new();
    let mut total_hours = 0.0;
    for r in &recent {
        if !r.task_reviews.is_empty() {
            let (a_total, a_done) = r
                .task_reviews
                .iter()
                .filter(|t| t.priority == "A")
                .fold((0, 0), |(t, d), tr| {
                    (t + 1, d + if tr.status == "completed" { 1 } else { 0 })
                });
            let rate = if a_total > 0 {
                (a_done as f64 / a_total as f64) * 100.0
            } else if !r.task_reviews.is_empty() {
                let done = r
                    .task_reviews
                    .iter()
                    .filter(|t| t.status == "completed")
                    .count();
                (done as f64 / r.task_reviews.len() as f64) * 100.0
            } else {
                0.0
            };
            rates.push(rate);
        }
        total_hours += crate::data::records::review_actual_hours(r);
    }

    let recent_avg_completion_rate = if rates.is_empty() {
        0.0
    } else {
        rates.iter().sum::<f64>() / rates.len() as f64
    };
    let recent_avg_daily_hours = total_hours / 7.0;

    // 预测状态：基于近7天平均完成率
    let (status, description) = if recent_avg_completion_rate >= 80.0 {
        (
            "on_track".to_string(),
            format!(
                "近7天平均完成率 {:.0}%，进度健康，按此节奏可顺利完成目标",
                recent_avg_completion_rate
            ),
        )
    } else if recent_avg_completion_rate >= 50.0 {
        (
            "at_risk".to_string(),
            format!(
                "近7天平均完成率 {:.0}%，存在风险，建议调整任务量或学习方式",
                recent_avg_completion_rate
            ),
        )
    } else {
        (
            "off_track".to_string(),
            format!(
                "近7天平均完成率 {:.0}%，明显偏离目标，需要重新评估计划",
                recent_avg_completion_rate
            ),
        )
    };

    GoalPrediction {
        recent_avg_completion_rate,
        recent_avg_daily_hours,
        expected_completion_rate: recent_avg_completion_rate,
        status,
        description,
    }
}

// ============================================================================
// 工具函数
// ============================================================================

/// 找到最早的复盘或计划日期
fn find_earliest_date(data_dir: &Path) -> DataResult<String> {
    let review_dates = records::list_review_dates(data_dir).unwrap_or_default();
    let plan_dates = crate::data::plan::list_daily_plan_dates(data_dir).unwrap_or_default();

    let earliest = review_dates.iter().chain(plan_dates.iter()).min().cloned();

    Ok(earliest.unwrap_or_else(today_string))
}

/// 收集指定日期范围内的所有复盘
fn collect_reviews_in_range(
    data_dir: &Path,
    start: &str,
    end: &str,
) -> DataResult<Vec<ReviewFile>> {
    let all_dates = records::list_review_dates(data_dir).unwrap_or_default();
    let filtered: Vec<ReviewFile> = all_dates
        .iter()
        .filter(|d| d.as_str() >= start && d.as_str() <= end)
        .filter_map(|d| records::read_review(data_dir, d).ok())
        .collect();
    Ok(filtered)
}

/// 获取当前月份范围
fn current_month_range(today: &str) -> (String, String) {
    let parts: Vec<&str> = today.split('-').collect();
    if parts.len() != 3 {
        return (today.to_string(), today.to_string());
    }
    let year: i32 = parts[0].parse().unwrap_or(2026);
    let month: u32 = parts[1].parse().unwrap_or(1);

    let start = format!("{:04}-{:02}-01", year, month);
    let end = format!("{:04}-{:02}-{:02}", year, month, days_in_month(year, month));
    (start, end)
}

/// 获取上个月份范围
fn prev_month_range(today: &str) -> (String, String) {
    let parts: Vec<&str> = today.split('-').collect();
    if parts.len() != 3 {
        return (today.to_string(), today.to_string());
    }
    let year: i32 = parts[0].parse().unwrap_or(2026);
    let month: u32 = parts[1].parse().unwrap_or(1);

    let (py, pm) = if month == 1 {
        (year - 1, 12u32)
    } else {
        (year, month - 1)
    };

    let start = format!("{:04}-{:02}-01", py, pm);
    let end = format!("{:04}-{:02}-{:02}", py, pm, days_in_month(py, pm));
    (start, end)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feeling_score() {
        assert_eq!(feeling_score("smooth").0, 3);
        assert_eq!(feeling_score("normal").0, 2);
        assert_eq!(feeling_score("hard").0, 1);
        assert_eq!(feeling_score("").0, 2);
    }

    #[test]
    fn test_blocker_label() {
        assert_eq!(blocker_label("time"), "时间不足");
        assert_eq!(blocker_label("understanding"), "理解困难");
        assert_eq!(blocker_label("unknown"), "unknown");
    }

    #[test]
    fn test_difficulty_label() {
        assert_eq!(difficulty_label("understanding"), "理解概念");
        assert_eq!(difficulty_label("problems"), "做题");
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 4), 30);
    }

    #[test]
    fn test_current_month_range() {
        let (s, e) = current_month_range("2026-07-15");
        assert_eq!(s, "2026-07-01");
        assert_eq!(e, "2026-07-31");
    }

    #[test]
    fn test_prev_month_range() {
        let (s, e) = prev_month_range("2026-07-15");
        assert_eq!(s, "2026-06-01");
        assert_eq!(e, "2026-06-30");

        let (s2, e2) = prev_month_range("2026-01-15");
        assert_eq!(s2, "2025-12-01");
        assert_eq!(e2, "2025-12-31");
    }
}
