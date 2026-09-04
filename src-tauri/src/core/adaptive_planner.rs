//! 周级学习计划自适应算法。
//!
//! 这一层只负责确定性统计和参数计算：
//! - 不让单周完成率直接决定下一周任务量；
//! - 将容量、任务量反馈、学科估时误差和学科计划量分开计算；
//! - 将算法状态与当前 State 分离，便于审计和回滚；
//! - AI 只消费本模块输出的数字预算，不负责猜测调整幅度。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::data::plan::{self, WorkloadAdjustment};
use crate::data::records::{self, ReviewFile, TaskReviewEntry};
use crate::data::state::StudyState;
use crate::data::{atomic_write, get_week_end, iso_week_string, read_file_content, DataResult};
use crate::AppSettings;

const ADAPTIVE_DIR: &str = "adaptive";
const STATE_FILE: &str = "adaptive_state.json";
const ANALYSIS_DIR: &str = "analysis";
const DEFAULT_ESTIMATION_FACTOR: f64 = 1.0;
const MIN_ESTIMATION_FACTOR: f64 = 0.80;
const MAX_ESTIMATION_FACTOR: f64 = 1.25;

fn default_factor() -> f64 {
    DEFAULT_ESTIMATION_FACTOR
}

fn default_workload_factor() -> f64 {
    1.0
}

/// 长期自适应状态。该文件只保存算法状态，不覆盖 State.current_task。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveState {
    pub version: String,
    #[serde(default)]
    pub capacity_ema: f64,
    #[serde(default)]
    pub workload_ema: f64,
    #[serde(default = "default_workload_factor")]
    pub workload_factor: f64,
    #[serde(default)]
    pub workload_direction: i8,
    #[serde(default)]
    pub workload_streak: u32,
    #[serde(default)]
    pub last_processed_week: Option<String>,
    #[serde(default)]
    pub last_plan_week: Option<String>,
    #[serde(default)]
    pub last_parameters: Option<AdaptivePlanParameters>,
    #[serde(default)]
    pub subjects: HashMap<String, SubjectAdaptiveState>,
    #[serde(default)]
    pub capacity_history: Vec<CapacitySample>,
}

impl Default for AdaptiveState {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            capacity_ema: 0.0,
            workload_ema: 0.0,
            workload_factor: 1.0,
            workload_direction: 0,
            workload_streak: 0,
            last_processed_week: None,
            last_plan_week: None,
            last_parameters: None,
            subjects: HashMap::new(),
            capacity_history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectAdaptiveState {
    #[serde(default = "default_factor")]
    pub estimation_factor: f64,
    #[serde(default)]
    pub estimation_samples: f64,
    #[serde(default = "default_factor")]
    pub load_factor: f64,
    #[serde(default)]
    pub last_time_ratio: Option<f64>,
}

impl Default for SubjectAdaptiveState {
    fn default() -> Self {
        Self {
            estimation_factor: 1.0,
            estimation_samples: 0.0,
            load_factor: 1.0,
            last_time_ratio: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacitySample {
    pub week_start: String,
    pub observed_hours: f64,
    pub eligible: bool,
}

/// 供 Planner 使用的确定性参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdaptivePlanParameters {
    pub capacity_hours: f64,
    pub capacity_adjustment: f64,
    pub workload_factor: f64,
    pub nominal_total_hours: f64,
    pub daily_target_hours: f64,
    pub daily_task_count: i64,
    pub subject_hours: HashMap<String, f64>,
    pub subject_shares: HashMap<String, f64>,
    pub subject_load_factors: HashMap<String, f64>,
    pub estimation_factors: HashMap<String, f64>,
    pub confidence: f64,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekPlanningAnalysis {
    pub version: String,
    pub week_start: String,
    pub week_end: String,
    pub planned_total_hours: f64,
    pub eligible_planned_hours: f64,
    pub actual_total_hours: f64,
    pub eligible_actual_hours: f64,
    pub completed_planned_hours: f64,
    pub planned_completion_rate: f64,
    pub task_completion_rate: f64,
    pub planned_task_count: i32,
    pub eligible_task_count: i32,
    pub reviewed_task_count: i32,
    pub completed_task_count: i32,
    pub unfinished_task_count: i32,
    pub unfinished_reasons: HashMap<String, i32>,
    pub valid_review_days: i32,
    pub actual_data_days: i32,
    pub valid_days: i32,
    pub external_day_count: i32,
    pub feedback: WorkloadFeedbackSummary,
    pub subjects: Vec<SubjectPlanningAnalysis>,
    pub manual_override: bool,
    pub confidence: f64,
    pub capacity_before: f64,
    pub capacity_observation: f64,
    pub capacity_after: f64,
    pub capacity_adjustment: f64,
    pub workload_adjustment: f64,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkloadFeedbackSummary {
    pub too_little_days: i32,
    pub reasonable_days: i32,
    pub too_much_days: i32,
    pub valid_days: i32,
    pub weighted_score: f64,
    pub ema: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubjectPlanningAnalysis {
    pub subject: String,
    pub planned_hours: f64,
    pub actual_hours: f64,
    pub time_ratio: Option<f64>,
    pub planned_completion_rate: f64,
    pub task_count: i32,
    pub completed_task_count: i32,
    pub unfinished_task_count: i32,
    pub valid_time_tasks: i32,
    pub blockers: HashMap<String, i32>,
}

#[derive(Debug, Default)]
struct SubjectAccumulator {
    planned_hours: f64,
    actual_hours: f64,
    completion_numerator: f64,
    completion_denominator: f64,
    estimated_for_ratio: f64,
    actual_for_ratio: f64,
    task_count: i32,
    reviewed_task_count: i32,
    completed_task_count: i32,
    unfinished_task_count: i32,
    valid_time_tasks: i32,
    blockers: HashMap<String, i32>,
}

fn subject_names() -> [&'static str; 4] {
    ["math", "english", "politics", "professional"]
}

fn subject_cn(subject: &str) -> &'static str {
    match subject {
        "math" => "数学",
        "english" => "英语",
        "politics" => "政治",
        "professional" => "专业课",
        _ => "该学科",
    }
}

fn subject_key(subject: &crate::data::state::SubjectKey) -> String {
    format!("{:?}", subject).to_lowercase()
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn sign(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn feedback_score(value: &str) -> Option<f64> {
    match value {
        "too_little" => Some(1.0),
        "reasonable" => Some(0.0),
        "too_much" => Some(-1.0),
        _ => None,
    }
}

fn is_external(review: &ReviewFile) -> bool {
    let daily_value = review
        .daily_review
        .as_ref()
        .map(|d| d.external_interference.as_str())
        .unwrap_or("");
    let legacy_value = review.data.external_interference.as_str();
    let is_exception = |value: &str| !value.is_empty() && value != "none" && value != "无";
    is_exception(daily_value) || is_exception(legacy_value)
}

fn task_completion(entry: &TaskReviewEntry) -> f64 {
    if entry.status == "completed" && entry.completion <= 0.0 {
        1.0
    } else {
        clamp(entry.completion, 0.0, 1.0)
    }
}

fn analysis_path(data_dir: &Path, week_start: &str) -> DataResult<PathBuf> {
    let iso_week = iso_week_string(week_start)?;
    Ok(data_dir
        .join(ANALYSIS_DIR)
        .join(format!("{}_planning.json", iso_week)))
}

fn adaptive_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(ADAPTIVE_DIR).join(STATE_FILE)
}

pub fn read_adaptive_state(data_dir: &Path) -> DataResult<AdaptiveState> {
    let path = adaptive_state_path(data_dir);
    if !path.exists() {
        return Ok(AdaptiveState::default());
    }
    let content = read_file_content(&path)?;
    let mut state: AdaptiveState = serde_json::from_str(&content)
        .map_err(|e| format!("解析自适应状态失败 {:?}: {}", path, e))?;
    if state.version.is_empty() {
        state.version = "1.0.0".to_string();
    }
    if state.workload_factor <= 0.0 {
        state.workload_factor = 1.0;
    }
    Ok(state)
}

fn save_adaptive_state(data_dir: &Path, state: &AdaptiveState) -> DataResult<()> {
    let path = adaptive_state_path(data_dir);
    let json =
        serde_json::to_string_pretty(state).map_err(|e| format!("序列化自适应状态失败: {}", e))?;
    atomic_write(&path, &json).map_err(|e| format!("写入自适应状态失败: {}", e))
}

pub fn read_week_planning_analysis(
    data_dir: &Path,
    week_start: &str,
) -> DataResult<WeekPlanningAnalysis> {
    let path = analysis_path(data_dir, week_start)?;
    if !path.exists() {
        return Err(format!("周计划分析不存在: {:?}", path));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析周计划分析失败 {:?}: {}", path, e))
}

/// 刷新并持久化指定周的分析，但不更新长期自适应状态。
/// 复盘提交后调用它，使分析结果随着本周数据逐步完整。
pub fn refresh_week_planning_analysis(
    data_dir: &Path,
    week_start: &str,
) -> DataResult<WeekPlanningAnalysis> {
    let analysis = analyze_week(data_dir, week_start)?;
    save_week_planning_analysis(data_dir, &analysis)?;
    Ok(analysis)
}

fn save_week_planning_analysis(data_dir: &Path, analysis: &WeekPlanningAnalysis) -> DataResult<()> {
    let path = analysis_path(data_dir, &analysis.week_start)?;
    let json = serde_json::to_string_pretty(analysis)
        .map_err(|e| format!("序列化周计划分析失败: {}", e))?;
    atomic_write(&path, &json).map_err(|e| format!("写入周计划分析失败: {}", e))
}

/// 计算指定周的原始分析，不会修改自适应状态。
pub fn analyze_week(data_dir: &Path, week_start: &str) -> DataResult<WeekPlanningAnalysis> {
    let week_end = get_week_end(week_start)?;
    let iso_week = iso_week_string(week_start)?;
    let week_plan = plan::read_week_plan(data_dir, &iso_week).ok();
    let excluded_days: HashSet<String> = week_plan
        .as_ref()
        .map(|wp| {
            wp.data
                .excluded_days
                .iter()
                .map(|d| d.date.clone())
                .collect()
        })
        .unwrap_or_default();
    let rest_days: HashSet<String> = week_plan
        .as_ref()
        .map(|wp| {
            wp.data
                .days
                .iter()
                .filter(|d| d.is_rest_day)
                .map(|d| d.date.clone())
                .collect()
        })
        .unwrap_or_default();
    let manual_override = week_plan
        .as_ref()
        .and_then(|wp| wp.data.workload_adjustment.as_ref())
        .map(|a| a.direction != "unchanged")
        .unwrap_or(false);

    let daily_plans = plan::read_week_daily_plans(data_dir, week_start).unwrap_or_default();
    let mut subject_acc: HashMap<String, SubjectAccumulator> = subject_names()
        .iter()
        .map(|s| ((*s).to_string(), SubjectAccumulator::default()))
        .collect();
    let mut analysis = WeekPlanningAnalysis {
        version: "1.0.0".to_string(),
        week_start: week_start.to_string(),
        week_end,
        manual_override,
        ..Default::default()
    };

    for daily in &daily_plans {
        let date = &daily.meta.date;
        let is_excluded_day = excluded_days.contains(date);
        let review = records::read_review(data_dir, date).ok();
        let review_external = review.as_ref().map(is_external).unwrap_or(false);
        let external_day = is_excluded_day || review_external;
        if external_day {
            analysis.external_day_count += 1;
        }

        let planned_day_hours: f64 = daily
            .data
            .tasks
            .iter()
            .map(|task| task.estimated_hours.max(0.0))
            .sum();
        if planned_day_hours <= 0.0 && review.is_none() {
            continue;
        }
        analysis.planned_total_hours += planned_day_hours;
        analysis.planned_task_count += daily.data.tasks.len() as i32;
        if !external_day && !rest_days.contains(date) {
            analysis.eligible_planned_hours += planned_day_hours;
            analysis.eligible_task_count += daily.data.tasks.len() as i32;
        }

        let review_actual = review
            .as_ref()
            .map(records::review_actual_hours)
            .unwrap_or(0.0)
            .max(0.0);
        let focus_stats_hours = crate::data::focus::focus_day_stats(data_dir, date)
            .map(|stats| stats.focus_minutes.max(0) as f64 / 60.0)
            .unwrap_or(0.0);
        let unlinked_focus_hours =
            crate::data::focus::day_unlinked_focus_minutes(data_dir, date).max(0) as f64 / 60.0;
        let has_structured_task_actual = review
            .as_ref()
            .map(|r| {
                r.task_reviews
                    .iter()
                    .any(|entry| entry.actual_minutes.is_some())
            })
            .unwrap_or(false);
        let actual_day_hours = if review_actual > 0.0 {
            // 结构化复盘的 total_hours 来自任务实际时间，不包含未关联任务的
            // focus；把未关联 focus 补上。旧版 AI 复盘的 total_hours 不重复补算。
            review_actual
                + if has_structured_task_actual {
                    unlinked_focus_hours
                } else {
                    0.0
                }
        } else {
            focus_stats_hours
        };
        if actual_day_hours > 0.0 {
            analysis.actual_data_days += 1;
        }
        analysis.actual_total_hours += actual_day_hours;

        // 外部异常日仍保留在 raw actual_total_hours 中用于展示，但不参与
        // 完成率、容量、学科估时或任务量反馈的学习，避免污染长期状态。
        if external_day {
            continue;
        }

        let mut task_reviews: HashMap<&str, &TaskReviewEntry> = HashMap::new();
        if let Some(r) = &review {
            for entry in &r.task_reviews {
                task_reviews.insert(entry.task_id.as_str(), entry);
            }
        }

        let mut completed_planned_hours = 0.0;
        let mut has_task_actual = false;
        let mut day_feedback: Option<f64> = None;

        if let Some(r) = &review {
            if let Some(daily_review) = &r.daily_review {
                day_feedback = feedback_score(&daily_review.workload_feedback);
            }
            if day_feedback.is_none() {
                day_feedback = feedback_score("");
            }
        }

        for task in &daily.data.tasks {
            let subject = subject_key(&task.subject);
            let acc = subject_acc.entry(subject).or_default();
            let planned = task.estimated_hours.max(0.0);
            acc.planned_hours += planned;
            acc.task_count += 1;

            let review_entry = task_reviews.get(task.id.as_str()).copied();
            if let Some(entry) = review_entry {
                let completion = task_completion(entry);
                completed_planned_hours += planned * completion;
                acc.completion_denominator += planned;
                acc.completion_numerator += planned * completion;
                acc.reviewed_task_count += 1;
                analysis.reviewed_task_count += 1;
                if completion >= 0.999 {
                    acc.completed_task_count += 1;
                    analysis.completed_task_count += 1;
                } else {
                    acc.unfinished_task_count += 1;
                    analysis.unfinished_task_count += 1;
                }
                for blocker in &entry.blockers {
                    *acc.blockers.entry(blocker.clone()).or_insert(0) += 1;
                    *analysis
                        .unfinished_reasons
                        .entry(blocker.clone())
                        .or_insert(0) += 1;
                }

                if let Some(actual_minutes) = entry.actual_minutes {
                    if actual_minutes >= 0 {
                        let actual = actual_minutes as f64 / 60.0;
                        acc.actual_hours += actual;
                        let estimated_for_ratio = entry
                            .estimated_hours
                            .filter(|hours| *hours > 0.0)
                            .unwrap_or(planned);
                        if estimated_for_ratio > 0.0 {
                            acc.estimated_for_ratio += estimated_for_ratio;
                            acc.actual_for_ratio += actual;
                            acc.valid_time_tasks += 1;
                        }
                        has_task_actual = true;
                    }
                }
            }
        }

        // 旧版或未开启任务计时时，使用 subject time_spent 作为学科实际时间的回退。
        if !has_task_actual {
            if let Some(r) = &review {
                for spent in &r.data.time_spent {
                    let subject = subject_key(&spent.subject);
                    if let Some(acc) = subject_acc.get_mut(&subject) {
                        acc.actual_hours += spent.hours.max(0.0);
                        if let Some(planned) = spent.planned_hours {
                            if planned > 0.0 && spent.hours >= 0.0 {
                                acc.estimated_for_ratio += planned;
                                acc.actual_for_ratio += spent.hours;
                                acc.valid_time_tasks += 1;
                            }
                        }
                    }
                }
            }
        }

        if let Some(score) = day_feedback {
            analysis.feedback.valid_days += 1;
            analysis.feedback.weighted_score += score;
            match score {
                value if value > 0.0 => analysis.feedback.too_little_days += 1,
                value if value < 0.0 => analysis.feedback.too_much_days += 1,
                _ => analysis.feedback.reasonable_days += 1,
            }
        }

        if review.is_some() {
            analysis.valid_review_days += 1;
        }
        if review.is_some() || actual_day_hours > 0.0 {
            analysis.valid_days += 1;
        }
        analysis.eligible_actual_hours += actual_day_hours;

        // 旧版复盘没有 task_reviews，只能使用汇总完成率，不伪造学科级完成率。
        if task_reviews.is_empty() {
            if let Some(r) = &review {
                let (_, _, _, _, rate) = records::review_completion_stats(r);
                let completion = clamp(rate / 100.0, 0.0, 1.0);
                completed_planned_hours += planned_day_hours * completion;
                analysis.reviewed_task_count += daily.data.tasks.len() as i32;
                analysis.completed_task_count +=
                    (daily.data.tasks.len() as f64 * completion).round() as i32;
                analysis.unfinished_task_count +=
                    (daily.data.tasks.len() as f64 * (1.0 - completion)).round() as i32;
            }
        }

        analysis.completed_planned_hours += completed_planned_hours;
    }

    // 上面的 task_reviews 统计按任务累加；旧版补算的完成量可能已写入 completed_planned_hours。
    if analysis.eligible_planned_hours > 0.0 {
        analysis.planned_completion_rate = clamp(
            analysis.completed_planned_hours / analysis.eligible_planned_hours,
            0.0,
            1.0,
        );
    }
    if analysis.eligible_task_count > 0 {
        analysis.task_completion_rate = clamp(
            analysis.completed_task_count as f64 / analysis.eligible_task_count as f64,
            0.0,
            1.0,
        );
    }
    if analysis.feedback.valid_days > 0 {
        analysis.feedback.weighted_score /= analysis.feedback.valid_days as f64;
    }

    for subject in subject_names() {
        let acc = subject_acc.remove(subject).unwrap_or_default();
        let time_ratio = if acc.estimated_for_ratio > 0.0 {
            Some(clamp(
                acc.actual_for_ratio / acc.estimated_for_ratio,
                0.6,
                1.8,
            ))
        } else {
            None
        };
        let completion = if acc.completion_denominator > 0.0 {
            clamp(
                acc.completion_numerator / acc.completion_denominator,
                0.0,
                1.0,
            )
        } else {
            0.0
        };
        analysis.subjects.push(SubjectPlanningAnalysis {
            subject: subject.to_string(),
            planned_hours: acc.planned_hours,
            actual_hours: acc.actual_hours,
            time_ratio,
            planned_completion_rate: completion,
            task_count: acc.task_count,
            completed_task_count: acc.completed_task_count,
            unfinished_task_count: acc.unfinished_task_count,
            valid_time_tasks: acc.valid_time_tasks,
            blockers: acc.blockers,
        });
    }

    let feedback_confidence = if analysis.feedback.valid_days > 0 {
        clamp(analysis.feedback.valid_days as f64 / 3.0, 0.0, 1.0)
    } else {
        0.0
    };
    let review_confidence = clamp(analysis.valid_review_days as f64 / 3.0, 0.0, 1.0);
    let time_confidence = if analysis.actual_data_days > 0 {
        1.0
    } else {
        0.35
    };
    analysis.confidence = clamp(
        (0.45 * review_confidence + 0.35 * feedback_confidence + 0.20 * time_confidence)
            * clamp(analysis.valid_days as f64 / 3.0, 0.0, 1.0),
        0.0,
        1.0,
    );

    if analysis.feedback.valid_days == 0 {
        analysis
            .warnings
            .push("本周没有任务量合理性反馈，任务量调整置信度降低".to_string());
    }
    if analysis.actual_data_days == 0 {
        analysis
            .warnings
            .push("本周没有可靠实际学习时间，时间估计校准不会更新".to_string());
    }
    if analysis.external_day_count > 0 {
        analysis.reasons.push(format!(
            "本周识别到 {} 个外部异常/排除日，已从长期容量学习中排除",
            analysis.external_day_count
        ));
    }
    if analysis.manual_override {
        analysis
            .warnings
            .push("本周存在用户显式任务量调整，未将其当作系统计划失败".to_string());
    }
    Ok(analysis)
}

fn recent_median(state: &AdaptiveState) -> Option<f64> {
    let mut values: Vec<f64> = state
        .capacity_history
        .iter()
        .filter(|sample| sample.eligible && sample.observed_hours > 0.0)
        .map(|sample| sample.observed_hours)
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(values[values.len() / 2])
}

fn robust_capacity_observation(observation: f64, state: &AdaptiveState) -> (f64, bool) {
    if observation <= 0.0 {
        return (0.0, false);
    }
    let Some(median) = recent_median(state) else {
        return (observation, false);
    };
    if median <= 0.0 || (observation - median).abs() <= median * 0.35 {
        return (observation, false);
    }
    (clamp(observation, median * 0.65, median * 1.35), true)
}

fn active_subject_hours(state: &StudyState) -> Vec<(String, f64)> {
    let candidates = [
        (
            "math",
            state.subjects.math.active,
            state.subjects.math.weekly_hours,
        ),
        (
            "english",
            state.subjects.english.active,
            state.subjects.english.weekly_hours,
        ),
        (
            "politics",
            state.subjects.politics.active,
            state.subjects.politics.weekly_hours,
        ),
        (
            "professional",
            state.subjects.professional.active,
            state.subjects.professional.weekly_hours,
        ),
    ];
    let mut result: Vec<(String, f64)> = candidates
        .iter()
        .filter(|(_, active, hours)| *active && *hours > 0.0)
        .map(|(subject, _, hours)| ((*subject).to_string(), *hours))
        .collect();
    if result.is_empty() {
        result = candidates
            .iter()
            .filter(|(_, active, _)| *active)
            .map(|(subject, _, _)| ((*subject).to_string(), 1.0))
            .collect();
    }
    result
}

fn projected_shares(
    base: &HashMap<String, f64>,
    load_factors: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut raw = HashMap::new();
    let raw_sum: f64 = base
        .iter()
        .map(|(subject, share)| {
            let factor = load_factors.get(subject).copied().unwrap_or(1.0);
            let value = share * factor;
            raw.insert(subject.clone(), value);
            value
        })
        .sum();
    if raw_sum <= 0.0 {
        return base.clone();
    }

    let mut result: HashMap<String, f64> = base
        .iter()
        .map(|(subject, share)| {
            let projected = raw.get(subject).copied().unwrap_or(*share) / raw_sum;
            (
                subject.clone(),
                share + clamp(projected - share, -0.05, 0.05),
            )
        })
        .collect();
    let sum: f64 = result.values().sum();
    if sum > 0.0 {
        for value in result.values_mut() {
            *value /= sum;
        }
    }
    result
}

fn build_parameters(
    state: &AdaptiveState,
    analysis: &WeekPlanningAnalysis,
    study_state: &StudyState,
    settings: &AppSettings,
) -> AdaptivePlanParameters {
    let configured_capacity =
        (settings.daily_target_hours() * settings.study_days_per_week().max(1) as f64).max(2.0);
    let active = active_subject_hours(study_state);
    let configured_subject_hours: f64 = active.iter().map(|(_, hours)| *hours).sum();
    let baseline_capacity = if configured_subject_hours > 0.0 {
        configured_subject_hours.min(configured_capacity)
    } else {
        configured_capacity
    };
    let capacity = if state.capacity_ema > 0.0 {
        state.capacity_ema
    } else {
        baseline_capacity
    };

    let total_base: f64 = active.iter().map(|(_, hours)| *hours).sum();
    let base_shares: HashMap<String, f64> = if total_base > 0.0 {
        active
            .iter()
            .map(|(subject, hours)| (subject.clone(), *hours / total_base))
            .collect()
    } else {
        HashMap::new()
    };

    let mut estimation_factors = HashMap::new();
    let mut load_factors = HashMap::new();
    for (subject, _) in &active {
        let adaptive = state.subjects.get(subject).cloned().unwrap_or_default();
        estimation_factors.insert(
            subject.clone(),
            clamp(
                adaptive.estimation_factor,
                MIN_ESTIMATION_FACTOR,
                MAX_ESTIMATION_FACTOR,
            ),
        );
        load_factors.insert(subject.clone(), clamp(adaptive.load_factor, 0.85, 1.15));
    }
    let shares = projected_shares(&base_shares, &load_factors);
    let estimation_mix: f64 = shares
        .iter()
        .map(|(subject, share)| share * estimation_factors.get(subject).copied().unwrap_or(1.0))
        .sum::<f64>()
        .max(0.5);

    let min_capacity = (baseline_capacity * 0.60).max(2.0);
    let target_capacity = clamp(
        capacity * state.workload_factor.clamp(0.85, 1.15),
        min_capacity,
        configured_capacity,
    );
    let nominal_total = clamp(target_capacity / estimation_mix, 1.0, configured_capacity);

    let subject_hours: HashMap<String, f64> = shares
        .iter()
        .map(|(subject, share)| (subject.clone(), nominal_total * share))
        .collect();
    let task_count = ((settings.daily_task_count() as f64) * state.workload_factor)
        .round()
        .clamp(1.0, 8.0) as i64;

    let mut reasons = analysis.reasons.clone();
    if state.workload_factor < 0.995 {
        reasons.push(format!(
            "近期任务量信号偏向过载，下一周名义计划容量采用 {:.0}% 的保守系数",
            state.workload_factor * 100.0
        ));
    } else if state.workload_factor > 1.005 {
        reasons.push(format!(
            "近期任务量信号显示仍有余量，下一周名义计划容量采用 {:.0}% 的系数",
            state.workload_factor * 100.0
        ));
    }
    if reasons.is_empty() {
        reasons.push("有效历史数据不足，保持用户配置的学习容量".to_string());
    }

    let mut warnings = analysis.warnings.clone();
    if analysis.confidence < 0.5 {
        warnings.push("当前自适应结果置信度较低，建议用户确认下一周计划".to_string());
    }

    AdaptivePlanParameters {
        capacity_hours: target_capacity,
        capacity_adjustment: if baseline_capacity > 0.0 {
            target_capacity / baseline_capacity - 1.0
        } else {
            0.0
        },
        workload_factor: state.workload_factor,
        nominal_total_hours: nominal_total,
        daily_target_hours: settings.daily_target_hours(),
        daily_task_count: task_count,
        subject_hours,
        subject_shares: shares,
        subject_load_factors: load_factors,
        estimation_factors,
        confidence: analysis.confidence,
        reasons,
        warnings,
    }
}

/// 分析上一周并生成下一周参数。相同周重复调用不会重复累加 EMA。
pub fn prepare_next_week(
    data_dir: &Path,
    current_week_start: &str,
    previous_week_start: &str,
    study_state: &StudyState,
    settings: &AppSettings,
) -> DataResult<AdaptivePlanParameters> {
    let mut adaptive = read_adaptive_state(data_dir)?;
    if adaptive.last_processed_week.as_deref() == Some(previous_week_start)
        && adaptive.last_plan_week.as_deref() == Some(current_week_start)
    {
        if let Some(parameters) = adaptive.last_parameters.clone() {
            // 不重复累加 EMA，但重新根据当前 State 计算预算，确保用户在同一周
            // 修改学科周时长后，显式配置仍然优先于缓存参数。
            let mut refreshed = build_parameters(
                &adaptive,
                &WeekPlanningAnalysis::default(),
                study_state,
                settings,
            );
            refreshed.reasons = parameters.reasons;
            refreshed.warnings = parameters.warnings;
            refreshed.confidence = parameters.confidence;
            return Ok(refreshed);
        }
    }

    let mut analysis = analyze_week(data_dir, previous_week_start)?;
    let configured_capacity =
        (settings.daily_target_hours() * settings.study_days_per_week().max(1) as f64).max(2.0);
    let baseline_subject_hours: f64 = active_subject_hours(study_state)
        .iter()
        .map(|(_, hours)| *hours)
        .sum();
    let baseline_capacity = if baseline_subject_hours > 0.0 {
        baseline_subject_hours.min(configured_capacity)
    } else {
        configured_capacity
    };
    if adaptive.capacity_ema <= 0.0 {
        adaptive.capacity_ema = baseline_capacity;
    }
    let capacity_before = adaptive.capacity_ema;

    // 学科估时 EMA：至少 3 个有效任务且至少 2 小时才更新。
    let mut estimation_reasons = Vec::new();
    for subject_analysis in &analysis.subjects {
        if subject_analysis.valid_time_tasks < 3
            || subject_analysis.planned_hours < 2.0
            || subject_analysis.time_ratio.is_none()
        {
            continue;
        }
        let entry = adaptive
            .subjects
            .entry(subject_analysis.subject.clone())
            .or_default();
        let ratio = subject_analysis.time_ratio.unwrap_or(1.0);
        let sample_weight = clamp(subject_analysis.valid_time_tasks as f64 / 4.0, 0.0, 1.0);
        let old_factor = entry.estimation_factor;
        entry.estimation_factor = clamp(
            entry.estimation_factor + 0.25 * sample_weight * (ratio - entry.estimation_factor),
            MIN_ESTIMATION_FACTOR,
            MAX_ESTIMATION_FACTOR,
        );
        entry.estimation_samples += subject_analysis.valid_time_tasks as f64;
        entry.last_time_ratio = Some(ratio);
        if (entry.estimation_factor - old_factor).abs() >= 0.005 {
            let direction = if ratio >= 1.0 { "提高" } else { "降低" };
            estimation_reasons.push(format!(
                "{}本周有效任务实际用时约为预计的 {:.0}%，因此估时系数逐步{}至 {:.2}",
                subject_cn(&subject_analysis.subject),
                ratio * 100.0,
                direction,
                entry.estimation_factor
            ));
        }
    }
    analysis.reasons.extend(estimation_reasons);

    // 容量只从有效日学习；明确外部异常的整周不会改变容量。
    let capacity_observation = if analysis.eligible_actual_hours > 0.0 {
        analysis.eligible_actual_hours
    } else {
        analysis.completed_planned_hours
    };
    let (robust_observation, was_anomaly) =
        robust_capacity_observation(capacity_observation, &adaptive);
    analysis.capacity_before = capacity_before;
    analysis.capacity_observation = robust_observation;
    let eligible_history_count = adaptive
        .capacity_history
        .iter()
        .filter(|sample| sample.eligible)
        .count();
    let capacity_confidence = clamp(analysis.valid_days as f64 / 3.0, 0.0, 1.0)
        * if analysis.actual_data_days > 0 {
            1.0
        } else {
            0.45
        };
    if robust_observation > 0.0
        && analysis.valid_days >= 3
        && eligible_history_count >= 1
        && analysis.external_day_count < analysis.valid_days
    {
        let candidate = adaptive.capacity_ema
            + 0.25 * capacity_confidence * (robust_observation - adaptive.capacity_ema);
        adaptive.capacity_ema = clamp(
            candidate,
            adaptive.capacity_ema * 0.92,
            adaptive.capacity_ema * 1.08,
        )
        .clamp(baseline_capacity * 0.60, configured_capacity);
    }
    if was_anomaly {
        analysis
            .warnings
            .push("容量观测偏离近期中位数超过 35%，已使用中位数护栏截断".to_string());
    }
    if analysis.external_day_count > 0 && analysis.eligible_actual_hours <= 0.0 {
        analysis
            .reasons
            .push("本周有效学习日不足，容量 EMA 保持不变".to_string());
    }
    analysis.capacity_after = adaptive.capacity_ema;
    analysis.capacity_adjustment = if capacity_before > 0.0 {
        adaptive.capacity_ema / capacity_before - 1.0
    } else {
        0.0
    };

    // 任务量反馈和完成率共同决定 workload_factor；用户显式覆盖时保持旧值。
    if analysis.feedback.valid_days > 0 && !analysis.manual_override {
        let feedback_week = analysis.feedback.weighted_score;
        adaptive.workload_ema = 0.65 * adaptive.workload_ema + 0.35 * feedback_week;
        analysis.feedback.ema = adaptive.workload_ema;

        let completion_signal = clamp((analysis.planned_completion_rate - 0.85) / 0.15, -1.0, 1.0);
        let expected_actual: f64 = analysis
            .subjects
            .iter()
            .map(|subject| {
                let factor = adaptive
                    .subjects
                    .get(&subject.subject)
                    .map(|s| s.estimation_factor)
                    .unwrap_or(1.0);
                subject.planned_hours * factor
            })
            .sum();
        let residual_signal = if expected_actual > 0.0 && analysis.eligible_actual_hours > 0.0 {
            clamp(
                analysis.eligible_actual_hours / expected_actual - 1.0,
                -0.5,
                0.5,
            )
        } else {
            0.0
        };
        let load_signal = clamp(
            0.60 * adaptive.workload_ema + 0.25 * completion_signal + 0.15 * residual_signal,
            -1.0,
            1.0,
        );
        let direction = sign(load_signal);
        if direction != 0 {
            let reversing =
                adaptive.workload_direction != 0 && adaptive.workload_direction != direction;
            if adaptive.workload_direction == direction {
                adaptive.workload_streak = adaptive.workload_streak.saturating_add(1);
            } else {
                adaptive.workload_streak = 1;
            }
            adaptive.workload_direction = direction;
            let persistence = match adaptive.workload_streak {
                0 | 1 => 0.4,
                2 => 0.7,
                _ => 1.0,
            };
            let deadband = if load_signal.abs() <= 0.15 {
                0.0
            } else {
                load_signal.signum() * ((load_signal.abs() - 0.15) / 0.85)
            };
            let mut delta = 0.08 * deadband * analysis.confidence * persistence;
            if reversing {
                delta *= 0.5;
            }
            let old_workload_factor = adaptive.workload_factor;
            adaptive.workload_factor = clamp(adaptive.workload_factor + delta, 0.85, 1.15);
            analysis.workload_adjustment = adaptive.workload_factor - 1.0;
            if (adaptive.workload_factor - old_workload_factor).abs() >= 0.005 {
                let direction_label = if delta > 0.0 { "增加" } else { "降低" };
                analysis.reasons.push(format!(
                    "任务量反馈与执行数据连续指向{}，下一周自动任务量系数{}至 {:.1}%",
                    if delta > 0.0 {
                        "仍有余量"
                    } else {
                        "偏多"
                    },
                    direction_label,
                    adaptive.workload_factor * 100.0
                ));
            }
        }
    } else {
        analysis.feedback.ema = adaptive.workload_ema;
        if analysis.manual_override {
            analysis
                .reasons
                .push("检测到用户显式任务量覆盖，本周不更新自动任务量系数".to_string());
        }
    }

    // 学科计划量只做很小的累积调整；正向完成信号在没有余量反馈时减半。
    if analysis.external_day_count == 0 && !analysis.manual_override {
        for subject_analysis in &analysis.subjects {
            if subject_analysis.task_count < 3
                || subject_analysis.completed_task_count <= 0
                || subject_analysis.planned_hours <= 0.0
            {
                continue;
            }
            let entry = adaptive
                .subjects
                .entry(subject_analysis.subject.clone())
                .or_default();
            let completion_signal = clamp(
                (subject_analysis.planned_completion_rate - 0.85) / 0.15,
                -1.0,
                1.0,
            );
            let factor = entry.estimation_factor.max(0.5);
            let residual_signal = if subject_analysis.actual_hours > 0.0 {
                clamp(
                    subject_analysis.actual_hours / (subject_analysis.planned_hours * factor) - 1.0,
                    -0.5,
                    0.5,
                )
            } else {
                0.0
            };
            let mut subject_signal = 0.60 * completion_signal + 0.40 * residual_signal;
            if subject_signal > 0.0 && adaptive.workload_ema <= 0.0 {
                subject_signal *= 0.5;
            }
            let confidence = clamp(subject_analysis.task_count as f64 / 5.0, 0.0, 1.0);
            entry.load_factor = clamp(
                entry.load_factor + 0.06 * subject_signal * confidence,
                0.85,
                1.15,
            );
        }
    }

    if capacity_observation > 0.0 {
        adaptive.capacity_history.push(CapacitySample {
            week_start: previous_week_start.to_string(),
            observed_hours: robust_observation,
            eligible: analysis.valid_days >= 3 && analysis.external_day_count < analysis.valid_days,
        });
        if adaptive.capacity_history.len() > 8 {
            let remove_count = adaptive.capacity_history.len() - 8;
            adaptive.capacity_history.drain(0..remove_count);
        }
    }

    let parameters = build_parameters(&adaptive, &analysis, study_state, settings);
    analysis.capacity_after = adaptive.capacity_ema;
    analysis.capacity_adjustment = if capacity_before > 0.0 {
        adaptive.capacity_ema / capacity_before - 1.0
    } else {
        0.0
    };
    analysis.workload_adjustment = adaptive.workload_factor - 1.0;
    analysis.reasons.extend(parameters.reasons.iter().cloned());
    analysis
        .warnings
        .extend(parameters.warnings.iter().cloned());
    analysis.reasons.sort();
    analysis.reasons.dedup();
    analysis.warnings.sort();
    analysis.warnings.dedup();

    adaptive.last_processed_week = Some(previous_week_start.to_string());
    adaptive.last_plan_week = Some(current_week_start.to_string());
    adaptive.last_parameters = Some(parameters.clone());
    save_week_planning_analysis(data_dir, &analysis)?;
    save_adaptive_state(data_dir, &adaptive)?;
    Ok(parameters)
}

/// 当自适应状态文件损坏或写入失败时，给 Planner 一个不改变用户配置的安全基线。
pub fn baseline_parameters(
    study_state: &StudyState,
    settings: &AppSettings,
) -> AdaptivePlanParameters {
    let adaptive = AdaptiveState::default();
    let analysis = WeekPlanningAnalysis::default();
    build_parameters(&adaptive, &analysis, study_state, settings)
}

/// 应用用户本周显式的任务量覆盖。它优先于自动系数，但仍服从应用的
/// 每日目标 × 学习日数上限；覆盖本身不会回写长期容量 EMA。
pub fn apply_manual_workload_override(
    parameters: &mut AdaptivePlanParameters,
    adjustment: &WorkloadAdjustment,
    settings: &AppSettings,
) {
    if adjustment.direction == "unchanged" || parameters.nominal_total_hours <= 0.0 {
        return;
    }
    let multiplier = match (adjustment.direction.as_str(), adjustment.level.as_deref()) {
        ("increase", Some("large")) => 1.40,
        ("increase", _) => 1.20,
        ("decrease", Some("large")) => 0.60,
        ("decrease", _) => 0.80,
        _ => 1.0,
    };
    let hard_max =
        (settings.daily_target_hours() * settings.study_days_per_week().max(1) as f64).max(2.0);
    let next_total = clamp(parameters.nominal_total_hours * multiplier, 1.0, hard_max);
    let ratio = next_total / parameters.nominal_total_hours;
    parameters.nominal_total_hours = next_total;
    for hours in parameters.subject_hours.values_mut() {
        *hours *= ratio;
    }
    parameters.daily_task_count = ((parameters.daily_task_count as f64) * multiplier)
        .round()
        .clamp(1.0, 8.0) as i64;
    parameters.reasons.push(format!(
        "用户显式要求本周任务量{}，已应用 {:.0}% 覆盖（仍受每日/每周硬上限约束）",
        if adjustment.direction == "increase" {
            "增加"
        } else {
            "减少"
        },
        multiplier * 100.0
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_mapping_is_stable() {
        assert_eq!(feedback_score("too_little"), Some(1.0));
        assert_eq!(feedback_score("reasonable"), Some(0.0));
        assert_eq!(feedback_score("too_much"), Some(-1.0));
        assert_eq!(feedback_score(""), None);
    }

    #[test]
    fn task_completion_uses_completed_status_when_fraction_missing() {
        let entry = TaskReviewEntry {
            status: "completed".to_string(),
            completion: 0.0,
            ..Default::default()
        };
        assert_eq!(task_completion(&entry), 1.0);
    }

    #[test]
    fn subject_share_shift_is_bounded_and_normalized() {
        let base = HashMap::from([("math".to_string(), 0.5), ("english".to_string(), 0.5)]);
        let factors = HashMap::from([("math".to_string(), 1.15), ("english".to_string(), 0.85)]);
        let result = projected_shares(&base, &factors);
        let sum: f64 = result.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
        assert!((result["math"] - 0.5).abs() <= 0.05 + 1e-9);
    }
}
