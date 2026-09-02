//! Dashboard — 数据聚合
//!
//! 读取 State + Plan + Records 汇总为 Dashboard 数据，
//! 供前端 Dashboard 页面展示。
//!
//! 对应前端 TypeScript 类型: `types/index.ts` 中的 `DashboardSummary`

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::data::plan::DailyPlanData;
use crate::data::records::ReviewData;
use crate::data::state::StudyState;
use crate::data::state::TaskStatus;
use crate::data::{add_days, days_between, get_week_end, get_week_start, today_string, DataResult};

// ============================================================================
// Dashboard 类型定义
// ============================================================================

/// 今日任务统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodayTaskStats {
    pub total: i32,
    pub done: i32,
    pub in_progress: i32,
    pub pending: i32,
}

/// 周进度
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekProgress {
    pub week_start: String,
    pub week_end: String,
    pub completed_hours: f64,
    pub target_hours: f64,
    pub daily_breakdown: Vec<DailyBreakdown>,
}

/// 每日明细
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyBreakdown {
    pub date: String,
    pub hours: f64,
    pub tasks_done: i32,
}

/// 即将到来的截止日期
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpcomingDeadline {
    pub date: String,
    pub title: String,
    pub subject: String,
    pub priority: String,
}

/// 复盘提醒
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewReminder {
    pub last_review_date: String,
    pub pending_review: bool,
}

/// 科目进度
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubjectProgress {
    pub subject: String,
    pub name: String,
    pub phase: String,
    pub weekly_hours: f64,
    pub target_score: i32,
    pub completion_percentage: f64,
    /// 最近学习的知识点（从前一日计划提取）
    #[serde(default)]
    pub current_topic: String,
}

/// Dashboard 汇总数据 — 对应前端 `DashboardSummary`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DashboardSummary {
    pub date: String,
    pub remaining_days: i64,
    pub today_tasks: TodayTaskStats,
    pub week_progress: WeekProgress,
    pub current_phase: String,
    pub streak_days: i32,
    pub total_study_days: i32,
    pub upcoming_deadlines: Vec<UpcomingDeadline>,
    pub review_reminder: ReviewReminder,
    pub subject_progress: Vec<SubjectProgress>,
}

// ============================================================================
// Dashboard 聚合器
// ============================================================================

/// Dashboard 数据聚合器
///
/// 从 State、Plan、Records 中读取数据并聚合为 DashboardSummary
pub struct DashboardAggregator;

impl DashboardAggregator {
    /// 聚合 Dashboard 数据
    ///
    /// 读取以下数据源：
    /// 1. `state/current.state` — 学习状态
    /// 2. `plan/YYYY-MM-DD_day.json` — 今日计划
    /// 3. `records/YYYY-MM-DD_review.json` — 复盘记录
    /// 4. `assets/milestones/` — 里程碑
    pub fn aggregate(data_dir: &Path) -> DataResult<DashboardSummary> {
        let today = today_string();

        // 读取 State
        let state = crate::data::state::read_state_or_default(data_dir);

        // 读取今日计划（合并 State.current_task + Review 完成度）
        let today_plan = crate::data::plan::read_daily_plan_with_merged_status(data_dir, &today)
            .ok()
            .map(|f| f.data);

        // 读取今日复盘
        let today_review = crate::data::records::read_review(data_dir, &today)
            .ok()
            .map(|f| f.data);

        // 计算剩余天数
        let remaining_days = days_between(&state.meta.exam_date, &today).unwrap_or(0);

        // 聚合今日任务统计
        let today_tasks = Self::aggregate_today_tasks(&state, &today_plan, &today_review);

        // 聚合周进度
        let week_progress = Self::aggregate_week_progress(data_dir, &today);

        // 确定当前阶段
        let current_phase = Self::determine_current_phase(&state);

        // 读取最近的复盘日期
        let review_reminder = Self::build_review_reminder(data_dir, &today);

        // 科目进度
        let subject_progress = Self::build_subject_progress(data_dir, &today, &state);

        // 即将到来的截止日期（从里程碑和风险中提取）
        let upcoming_deadlines = Self::extract_upcoming_deadlines(data_dir, &state);

        Ok(DashboardSummary {
            date: today,
            remaining_days,
            today_tasks,
            week_progress,
            current_phase,
            streak_days: state.progress.streak_days,
            total_study_days: state.progress.total_study_days,
            upcoming_deadlines,
            review_reminder,
            subject_progress,
        })
    }

    /// 聚合今日任务统计
    fn aggregate_today_tasks(
        state: &StudyState,
        plan: &Option<DailyPlanData>,
        review: &Option<ReviewData>,
    ) -> TodayTaskStats {
        // 如果有今日复盘，从复盘中统计
        if let Some(r) = review {
            let total = r.completion.priority_a_total + r.completion.priority_b_total;
            let done = r.completion.priority_a_done + r.completion.priority_b_done;
            return TodayTaskStats {
                total,
                done,
                in_progress: 0,
                // M5：防御数据不一致导致的负数
                pending: (total - done).max(0),
            };
        }

        // 如果有今日计划，从计划中统计
        if let Some(p) = plan {
            let total = p.tasks.len() as i32;
            let done = p
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Done)
                .count() as i32;
            let in_progress = p
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::InProgress)
                .count() as i32;
            return TodayTaskStats {
                total,
                done,
                in_progress,
                pending: (total - done - in_progress).max(0),
            };
        }

        // 回退到 State 的 current_task
        let total = state.current_task.tasks.len() as i32;
        let done = state
            .current_task
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count() as i32;
        let in_progress = state
            .current_task
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count() as i32;

        TodayTaskStats {
            total,
            done,
            in_progress,
            pending: (total - done - in_progress).max(0),
        }
    }

    /// 聚合周进度
    fn aggregate_week_progress(data_dir: &Path, today: &str) -> WeekProgress {
        let week_start = get_week_start(today).unwrap_or_else(|_| today.to_string());
        let week_end = get_week_end(today).unwrap_or_else(|_| today.to_string());

        let settings = crate::load_settings(data_dir);
        let daily_target_hours = settings.daily_target_hours();
        let study_days_per_week = settings.study_days_per_week();

        let mut daily_breakdown: Vec<DailyBreakdown> = Vec::new();
        let mut completed_hours = 0.0;

        let mut current = week_start.clone();
        loop {
            let breakdown = Self::compute_daily_breakdown(data_dir, &current, daily_target_hours);
            completed_hours += breakdown.hours;
            daily_breakdown.push(breakdown);

            if current == week_end {
                break;
            }
            current = add_days(&current, 1).unwrap_or(week_end.clone());
        }

        // 目标时长 = 每日目标 * 每周学习天数
        let target_hours = daily_target_hours * study_days_per_week as f64;

        WeekProgress {
            week_start,
            week_end,
            completed_hours,
            target_hours,
            daily_breakdown,
        }
    }

    /// 计算单日学习明细
    ///
    /// 优先级：
    /// 1. 若存在当日复盘，以复盘中的实际时长/完成任务为准。
    /// 2. 否则读取日计划合并状态；若有任务已完成（Done）或进行中（InProgress），
    ///    按已完成任务 estimated_hours 估算，并将该日记为已学习。
    fn compute_daily_breakdown(
        data_dir: &Path,
        date: &str,
        daily_target_hours: f64,
    ) -> DailyBreakdown {
        // 1. 优先读取复盘
        if let Ok(review) = crate::data::records::read_review(data_dir, date) {
            // 优先从 task_reviews 统计已完成任务数（兼容旧版 data.completion 全零的复盘文件）
            let tasks_done = if !review.task_reviews.is_empty() {
                review
                    .task_reviews
                    .iter()
                    .filter(|tr| tr.status == "completed")
                    .count() as i32
            } else {
                review.data.completion.priority_a_done + review.data.completion.priority_b_done
            };
            return DailyBreakdown {
                date: date.to_string(),
                hours: crate::data::records::review_actual_hours(&review),
                tasks_done,
            };
        }

        // 2. 无复盘时读取日计划（合并 State/Review 状态）
        if let Ok(plan) = crate::data::plan::read_daily_plan_with_merged_status(data_dir, date) {
            let mut hours = 0.0;
            let mut tasks_done = 0;
            let mut has_in_progress = false;
            for task in &plan.data.tasks {
                match task.status {
                    crate::data::state::TaskStatus::Done => {
                        tasks_done += 1;
                        hours += task.estimated_hours;
                    }
                    crate::data::state::TaskStatus::InProgress => {
                        has_in_progress = true;
                    }
                    _ => {}
                }
            }

            // 学习时长优先取当天任务的实际累计计时（含进行中时段）。
            // 任务计时不区分是否完成，即使任务未标记 Done，其计时也应计入学习时间；
            // 无计时数据时才回退到已完成任务的估时。
            let actual_minutes = crate::data::state::read_state(data_dir)
                .map(|state| crate::data::state::day_actual_minutes(&state, date))
                .unwrap_or(0);
            // 未关联任务的番茄钟专注分钟：已关联的任务其时长已计入任务累计计时，
            // 未关联的专注只存在于会话文件，需单独计入当日实际学习时长（避免重复）。
            let actual_minutes =
                actual_minutes + crate::data::focus::day_unlinked_focus_minutes(data_dir, date);

            if actual_minutes > 0 {
                hours = actual_minutes as f64 / 60.0;
            } else if hours <= 0.0 && has_in_progress {
                // 有进行中任务但无完成时长时，给最小占位时长，确保首页"已学习"圆点亮起
                hours = (daily_target_hours * 0.1).max(0.1);
            }

            return DailyBreakdown {
                date: date.to_string(),
                hours,
                tasks_done,
            };
        }

        // 3. 无计划也无复盘：仍计入明确的番茄钟学习时长（任务累计 + 未关联专注），
        //    保证正计时/番茄结束自动计入今日学习时长，口径与分析页 build_learning_trend 一致。
        let actual_minutes = crate::data::state::read_state(data_dir)
            .map(|state| crate::data::state::day_actual_minutes(&state, date))
            .unwrap_or(0)
            + crate::data::focus::day_unlinked_focus_minutes(data_dir, date);
        if actual_minutes > 0 {
            DailyBreakdown {
                date: date.to_string(),
                hours: actual_minutes as f64 / 60.0,
                tasks_done: 0,
            }
        } else {
            DailyBreakdown {
                date: date.to_string(),
                hours: 0.0,
                tasks_done: 0,
            }
        }
    }

    /// 确定当前学习阶段
    fn determine_current_phase(state: &StudyState) -> String {
        // 取第一个活跃科目的阶段
        let phase = if state.subjects.math.active {
            state.subjects.math.phase.clone()
        } else if state.subjects.english.active {
            state.subjects.english.phase.clone()
        } else if state.subjects.professional.active {
            state.subjects.professional.phase.clone()
        } else if state.subjects.politics.active {
            state.subjects.politics.phase.clone()
        } else {
            return "未知".to_string();
        };
        phase_to_chinese(&phase)
    }

    /// 构建复盘提醒
    fn build_review_reminder(data_dir: &Path, today: &str) -> ReviewReminder {
        let review_dates = crate::data::records::list_review_dates(data_dir).unwrap_or_default();

        let last_review_date = review_dates.last().cloned().unwrap_or_default();

        // 检查今天是否已有复盘
        let pending_review = !review_dates.iter().any(|d| d == today);

        ReviewReminder {
            last_review_date,
            pending_review,
        }
    }

    /// 构建科目进度
    ///
    /// `current_topic` 优先从前一日日计划的任务标题中提取（按科目分组），
    /// 这样可以把"学科进度"具体到章节（如"微分方程"），而不是泛化的阶段标签。
    /// 若前一日无计划，则回退到今日计划；若仍无，则使用 state 的 `current_focus`。
    fn build_subject_progress(
        data_dir: &Path,
        today: &str,
        state: &StudyState,
    ) -> Vec<SubjectProgress> {
        use crate::data::state::SubjectKey;
        use std::collections::HashMap;

        // 读取前一日的日计划，提取每个科目的章节标题
        let prev_day = crate::data::add_days(today, -1).unwrap_or_else(|_| today.to_string());
        let prev_plan = crate::data::plan::read_daily_plan(data_dir, &prev_day).ok();

        // 前一日没有计划时回退到今日计划
        let plan_for_topics: Option<crate::data::plan::DailyPlanFile> =
            prev_plan.or_else(|| crate::data::plan::read_daily_plan(data_dir, today).ok());

        // 按科目收集任务标题，去重保序
        let mut topics_by_subject: HashMap<SubjectKey, Vec<String>> = HashMap::new();
        if let Some(plan) = &plan_for_topics {
            for task in &plan.data.tasks {
                let topic = strip_subject_prefix(&task.title);
                if topic.is_empty() {
                    continue;
                }
                let entry = topics_by_subject.entry(task.subject.clone()).or_default();
                if !entry.iter().any(|t| t == &topic) {
                    entry.push(topic);
                }
            }
        }

        let mut result = Vec::new();
        let subjects = &state.subjects;

        let mut push_progress = |subject_key: &str,
                                 name: Option<&String>,
                                 subj: &crate::data::state::SubjectState,
                                 subj_key: SubjectKey| {
            let version = subj.version.as_deref().unwrap_or("");
            let seq_total = crate::core::chapter_seq::total_count(subject_key, version);
            let completion = if !subj.completed.is_empty() && seq_total > 0 {
                // 按已完成内容在章节顺序表中的最高位置计算进度（完成到第 N 章 = N/总数）
                let max_pos = subj
                    .completed
                    .iter()
                    .filter_map(|c| crate::core::chapter_seq::position(subject_key, version, c))
                    .max()
                    .map(|p| (p + 1) as f64)
                    .unwrap_or(0.0);
                (max_pos / seq_total as f64 * 100.0).min(100.0)
            } else if !subj.completed.is_empty() {
                // 无章节表时兜底：按已完成条目数估算（沿用旧口径，每科约 50 章）
                (subj.completed.len() as f64 / 50.0 * 100.0).min(100.0)
            } else {
                0.0
            };
            // 优先使用前一日计划的章节标题；其次 state.current_focus；
            // 都没有时留空（前端展示为 "—"），不再回退到阶段标签
            let current_topic = topics_by_subject
                .get(&subj_key)
                .map(|titles| titles.join("、"))
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    if !subj.current_focus.is_empty() {
                        Some(subj.current_focus.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            result.push(SubjectProgress {
                subject: subject_key.to_string(),
                name: name.cloned().unwrap_or_else(|| subject_key.to_string()),
                phase: format!("{:?}", subj.phase).to_lowercase(),
                weekly_hours: subj.weekly_hours,
                target_score: subj.target_score,
                completion_percentage: completion,
                current_topic,
            });
        };

        if subjects.math.active {
            push_progress(
                "math",
                subjects.math.name.as_ref(),
                &subjects.math,
                SubjectKey::Math,
            );
        }
        if subjects.english.active {
            push_progress(
                "english",
                subjects.english.name.as_ref(),
                &subjects.english,
                SubjectKey::English,
            );
        }
        if subjects.politics.active {
            push_progress(
                "politics",
                subjects.politics.name.as_ref(),
                &subjects.politics,
                SubjectKey::Politics,
            );
        }
        if subjects.professional.active {
            push_progress(
                "professional",
                subjects.professional.name.as_ref(),
                &subjects.professional,
                SubjectKey::Professional,
            );
        }

        result
    }

    /// 从里程碑和风险中提取即将到来的截止日期
    fn extract_upcoming_deadlines(data_dir: &Path, _state: &StudyState) -> Vec<UpcomingDeadline> {
        let mut deadlines = Vec::new();

        // 从里程碑提取
        if let Ok(milestones) = crate::data::assets::read_milestones(data_dir) {
            for m in &milestones {
                if (m.status == "pending" || m.status == "in_progress") && !m.target_date.is_empty()
                {
                    deadlines.push(UpcomingDeadline {
                        date: m.target_date.clone(),
                        title: m.title.clone(),
                        subject: "overall".to_string(),
                        priority: if m.status == "in_progress" {
                            "high".to_string()
                        } else {
                            "medium".to_string()
                        },
                    });
                }
            }
        }

        // 风险项已废弃，不再从中提取截止日期

        // 按日期排序
        deadlines.sort_by(|a, b| a.date.cmp(&b.date));

        // 限制返回数量
        deadlines.truncate(10);

        deadlines
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 将 `StudyPhase` 枚举映射为中文标签
///
/// 用于 Dashboard 状态区展示，避免出现 `foundation`/`strengthen` 等英文。
fn phase_to_chinese(phase: &crate::data::state::StudyPhase) -> String {
    use crate::data::state::StudyPhase;
    match phase {
        StudyPhase::Foundation => "基础".to_string(),
        StudyPhase::Strengthen => "强化".to_string(),
        StudyPhase::Sprint => "冲刺".to_string(),
        StudyPhase::Mock => "模拟".to_string(),
        StudyPhase::Complete => "已完成".to_string(),
    }
}

/// 从任务标题中剥离科目前缀，提取章节/主题描述
///
/// AI 生成的任务标题常带科目前缀，例如：
/// - "数学：微分方程练习" → "微分方程练习"
/// - "英语: 阅读理解训练" → "阅读理解训练"
/// - "政治 - 马原精讲"   → "马原精讲"
///   无前缀时原样返回。
fn strip_subject_prefix(title: &str) -> String {
    let trimmed = title.trim();
    // 支持全角/半角冒号、连字符分隔
    for sep in ['：', ':'] {
        if let Some(idx) = trimmed.find(sep) {
            let prefix = trimmed[..idx].trim();
            let rest = trimmed[idx..].trim_start_matches(|c: char| c == sep || c.is_whitespace());
            // 仅当 prefix 看起来是科目标签时才剥离（限定 1-4 个字符，避免误伤）
            if !rest.is_empty() && prefix.chars().count() <= 4 {
                return rest.to_string();
            }
        }
    }
    // 处理 "科目 - 主题" 形式
    if let Some(idx) = trimmed.find(" - ") {
        let prefix = trimmed[..idx].trim();
        let rest = trimmed[idx + 3..].trim();
        if !rest.is_empty() && prefix.chars().count() <= 4 {
            return rest.to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::focus::{
        append_focus_session, FocusSession, FocusSessionStatus, FocusSessionType,
    };
    use crate::data::state::{save_state, StateTask, StudyState};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "studyagent_dashboard_test_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn strip_subject_prefix_handles_fullwidth_colon() {
        assert_eq!(strip_subject_prefix("数学：微分方程练习"), "微分方程练习");
    }

    #[test]
    fn strip_subject_prefix_handles_halfwidth_colon() {
        assert_eq!(strip_subject_prefix("英语: 阅读理解训练"), "阅读理解训练");
    }

    #[test]
    fn strip_subject_prefix_handles_dash() {
        assert_eq!(strip_subject_prefix("政治 - 马原精讲"), "马原精讲");
    }

    #[test]
    fn strip_subject_prefix_keeps_no_prefix() {
        assert_eq!(strip_subject_prefix("微分方程练习"), "微分方程练习");
    }

    #[test]
    fn strip_subject_prefix_keeps_long_prefix() {
        // 前缀超过 4 字符不视为科目标签
        assert_eq!(
            strip_subject_prefix("高等数学复习：微分方程"),
            "高等数学复习：微分方程"
        );
    }

    #[test]
    fn stopwatch_finish_counts_into_daily_study_hours_e2e() {
        let dir = tmp_dir("stopwatch_count");
        let date = "2026-08-19";

        // 构造 state：任务 id 前缀匹配当日，已累计 25 分钟（模拟已关联番茄累加）
        let mut st = StudyState::default();
        st.current_task.date = date.to_string();
        let task = StateTask {
            task_id: Some(format!("{}-01", date)),
            subject: "math".to_string(),
            task: "测试任务".to_string(),
            accumulated_minutes: 25,
            ..Default::default()
        };
        st.current_task.tasks.push(task);
        save_state(&dir, &st).unwrap();

        // focus 记录：一条未关联的已完成正计时（25 分钟，即正计时结束后落盘），
        // 一条已关联番茄（15 分钟，仅元数据，其时间已计入任务累计，不应重复计入）
        append_focus_session(
            &dir,
            FocusSession {
                id: "sw1".to_string(),
                r#type: FocusSessionType::Stopwatch,
                started_at: format!("{}T09:00:00Z", date),
                ended_at: format!("{}T09:25:00Z", date),
                duration_minutes: 25,
                task_id: None,
                status: FocusSessionStatus::Completed,
            },
        )
        .unwrap();
        append_focus_session(
            &dir,
            FocusSession {
                id: "f1".to_string(),
                r#type: FocusSessionType::Focus,
                started_at: format!("{}T08:00:00Z", date),
                ended_at: format!("{}T08:15:00Z", date),
                duration_minutes: 15,
                task_id: Some(format!("{}-01", date)),
                status: FocusSessionStatus::Completed,
            },
        )
        .unwrap();

        // 无当日复盘、无日计划文件：正计时结束后应自动计入今日学习时长
        let breakdown = DashboardAggregator::compute_daily_breakdown(&dir, date, 4.0);
        // 25（已关联番茄累计进任务） + 25（未关联正计时） = 50 分钟
        assert!(
            (breakdown.hours - 50.0 / 60.0).abs() < 1e-6,
            "hours = {}",
            breakdown.hours
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
