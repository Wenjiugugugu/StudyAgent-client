//! goal_planner — 目标与截止日规划区间的确定性倒排 + 复盘双轨重排
//!
//! 职责：
//! - **任务来源确定性**：给定某科目的生效区间，用 chapter_seq 顺序表
//!   算出区间内每天「该推进到哪个知识点」，据此生成每日任务（不调 AI 决定内容）。
//! - **任务估时由 AI 参与**：把当天的知识点交给 AI 细化为带估时的任务；
//!   AI 失败时回退到用户设置的任务粒度（`AppSettings::standard_granularity()`）。
//! - **复盘双轨重排**：复盘后按「当前进度 vs 目标差距」确定性重排截止日科目
//!   的后续任务量（完成多→减少、完成少→增多、达标→提前退出）。
//!
//! 非截止日科目不受影响，继续走现有的按学习时长 AI 重排链路。

use std::collections::HashMap;
use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::core::chapter_seq;
use crate::data::goal::{read_goals, save_goals, Goal};
use crate::data::plan::PlanTask;
use crate::data::state::{SubjectKey, TaskPriority, TaskStatus};
use crate::data::{add_days, clean_ai_json, DataResult};

/// 把 subject 转为设置/顺序表键
pub fn subject_key_str(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "math",
        SubjectKey::English => "english",
        SubjectKey::Politics => "politics",
        SubjectKey::Professional => "professional",
    }
}

/// 科目显示名
pub fn subject_display_name(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}

/// 去掉任务标题中的「（科目）」前缀，取回纯知识点名。
///
/// 用 `strip_prefix` 只剥一次：`trim_start_matches` 会重复剥离，
/// 当知识点名本身也以该串开头时（如「（数学）数学分析」）会被误剥两层。
fn strip_subject_prefix<'a>(title: &'a str, prefix: &str) -> &'a str {
    title.strip_prefix(prefix).unwrap_or(title)
}

/// 从 StudyState 取某科目当前的 math/english 版本标签（用于 chapter_seq 定位）
pub fn subject_version(state: &crate::data::state::StudyState, key: &str) -> String {
    match key {
        "math" => state.subjects.math.version.clone().unwrap_or_default(),
        "english" => state.subjects.english.version.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// 倒排调度：生成某科目的知识点推进计划（每天应推进的位置区间）。
///
/// 返回 `Vec<(date, Vec<usize>)>`：每个学习日对应的待推进知识点位置列表。
/// `excluded_or_rest_days`：该区间内不可学习的具体日期（YY-MM-DD）集合。
fn backward_schedule(
    today: &str,
    deadline: &str,
    current_pos: usize,
    target_pos: usize,
    rest_or_excluded: &[String],
) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    if target_pos <= current_pos {
        // 已达标，无需安排
        return out;
    }
    // 截止日早于起始日：区间为空。调用方（调度器）已用 active_goals_for_subject 过滤过，
    // 但命令层可指定任意 goal_id，这里必须短路——否则下面的按日推进循环永远等不到
    // `cur == deadline`（日期会一路加到溢出），造成死循环。
    if deadline < today {
        return out;
    }
    let remaining = target_pos - current_pos;

    // 兜底上限（约 10 年）：即使日期字符串异常，也不允许无限推进
    const MAX_DAYS: usize = 3660;

    // 收集 today..=deadline 内可用的学习日（剔除休息/排除日）
    let mut study_days: Vec<String> = Vec::new();
    let mut cur = today.to_string();
    for _ in 0..MAX_DAYS {
        if !rest_or_excluded.iter().any(|d| d == &cur) {
            study_days.push(cur.clone());
        }
        if cur == deadline {
            break;
        }
        cur = match add_days(&cur, 1) {
            Ok(next) => next,
            Err(_) => break,
        };
    }

    let n = study_days.len().max(1);
    // 均匀分配：每个学习日 base 个知识点，前 extra 天再 +1（尽量均摊，用满全部学习日，
    // 理想执行下正好在截止日达标）。
    let base = remaining / n;
    let extra = remaining % n;

    let mut pos = current_pos;
    for (i, day) in study_days.iter().enumerate() {
        let count = base + usize::from(i < extra);
        if count == 0 {
            // 剩余知识点不足以排满每个学习日（base 为 0，仅前 extra 天有任务）
            break;
        }
        let slice: Vec<usize> = (pos + 1..=pos + count).collect();
        out.push((day.clone(), slice));
        pos += count;
        if pos >= target_pos {
            break;
        }
    }
    out
}

/// 为某科目当天生成任务（区间生效时的任务来源）。
///
/// - 确定性取出当天应推进的知识点，生成任务（估时用标准粒度兜底；供同步 scheduler 使用）。
/// - 需要 AI 参与估时时，请使用 `plan_goal_tasks`（异步版）。
pub fn plan_goal_tasks_sync(
    data_dir: &Path,
    goal: &Goal,
    date: &str,
    version: &str,
) -> DataResult<Vec<PlanTask>> {
    let Some((start_pos, target_pos)) = goal.current_position.zip(goal.target_position) else {
        return Err("目标未初始化 current_position/target_position".to_string());
    };

    // 空区间短路：截止日缺失或早于 `date`（已过期）时，倒排区间为空。
    // 必须在这里返回，而不是依赖下游——下游的 `rest_days_as_dates` / `backward_schedule`
    // 都按「从 date 逐日推进到 deadline」实现，区间反向时会等待永远不成立的
    // `cur == deadline` 而陷入死循环（2026-09-13 由运行时冒烟测试发现）。
    if goal.deadline.is_empty() || goal.deadline.as_str() < date {
        return Ok(Vec::new());
    }

    // 把「周日」这类休息日名称转成区间 [date, deadline] 内的具体日期集合
    let settings = crate::load_settings(data_dir);
    // 单条任务估时兜底沿用用户设置的任务粒度（不再硬编码 1.5h）
    let granularity =
        crate::core::planning::pure::normalize_granularity(settings.standard_granularity());
    let rest_date_set = rest_days_as_dates(&settings.rest_days(), date, &goal.deadline);

    let schedule = backward_schedule(date, &goal.deadline, start_pos, target_pos, &rest_date_set);
    let day_slice = schedule.iter().find(|(d, _)| d == date).map(|(_, s)| s);
    let Some(pos_slice) = day_slice else {
        // 该日期不在倒排区间内（无推进）
        return Ok(Vec::new());
    };

    let subject = &goal.subject;
    let knowledge: Vec<String> = pos_slice
        .iter()
        .filter_map(|&p| {
            chapter_seq::syllabus_points(subject_key_str(subject), version)
                .and_then(|seq| seq.get(p))
                .map(|s| s.to_string())
        })
        .collect();
    if knowledge.is_empty() {
        return Ok(Vec::new());
    }

    let mut tasks: Vec<PlanTask> = knowledge
        .into_iter()
        .enumerate()
        .map(|(i, kp)| PlanTask {
            id: format!("{}-{:02}", date, i + 1),
            subject: subject.clone(),
            title: format!("（{}）{}", subject_display_name(subject), kp),
            priority: TaskPriority::A,
            estimated_hours: granularity,
            goal: format!("推进至「{}」", kp),
            completion_criteria: vec![format!("完成 {} 的学习", kp)],
            textbook: None,
            style_tips: None,
            fallback_plan: None,
            status: TaskStatus::Pending,
            dida_task_id: None,
        })
        .collect();

    // 排序：大块头优先 > 标题
    tasks.sort_by(|a, b| {
        b.estimated_hours
            .partial_cmp(&a.estimated_hours)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(tasks)
}

/// 为某科目当天生成任务，并在任务来源确定后由 AI 参与估时。
///
/// 流程：`plan_goal_tasks_sync` 确定今天推进的知识点 → AI 估算每条时长 → 叠加到任务上。
/// AI 失败时保留标准粒度兜底。
pub async fn plan_goal_tasks(
    data_dir: &Path,
    ai: &AiService,
    goal: &Goal,
    date: &str,
    version: &str,
) -> DataResult<Vec<PlanTask>> {
    let mut tasks = plan_goal_tasks_sync(data_dir, goal, date, version)?;
    let subject = &goal.subject;
    // AI 估时（按知识点标题匹配叠加）
    let prefix = format!("（{}）", subject_display_name(subject));
    let knowledge: Vec<String> = tasks
        .iter()
        .map(|t| strip_subject_prefix(&t.title, &prefix).to_string())
        .collect();
    let estimate = estimate_tasks_hours(data_dir, ai, subject, version, &knowledge).await;
    for t in tasks.iter_mut() {
        let kp = strip_subject_prefix(&t.title, &prefix);
        if let Some(h) = estimate.get(kp) {
            t.estimated_hours = *h;
        }
    }
    tasks.sort_by(|a, b| {
        b.estimated_hours
            .partial_cmp(&a.estimated_hours)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(tasks)
}

/// 用 AI 估算当天每个知识点的学习时长（小时）。
///
/// AI 失败时返回空 map，调用方回退到标准粒度。
async fn estimate_tasks_hours(
    data_dir: &Path,
    ai: &AiService,
    subject: &SubjectKey,
    version: &str,
    knowledge: &[String],
) -> HashMap<String, f64> {
    if knowledge.is_empty() {
        return HashMap::new();
    }
    let gran = crate::core::planning::pure::normalize_granularity(
        crate::load_settings(data_dir).standard_granularity(),
    );
    let points = knowledge.join("、");
    let prompt = format!(
        "你是考研各科学习任务拆分与估时助手。请为以下「{}」科目的一小节学习知识点估算需要的学习时长（小时，取 0.5 的整数倍），\
         每个知识点拆成一条任务。用户设置的任务粒度为 {:.2} 小时/条，单条估时请尽量贴近该粒度。\
         知识点：{}\n\
         只返回 JSON 数组，每项 {json_example},不要输出其他内容。",
        subject_display_name(subject),
        gran,
        points,
        json_example = r#"{"knowledge":"知识点原文","hours":数字}"#,
    );
    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: prompt,
            ..Default::default()
        }],
        agent: Some(AgentType::Planner),
        temperature: Some(0.2),
        timeout_override: Some(60),
        math_version: if *subject == SubjectKey::Math {
            Some(version.to_string())
        } else {
            None
        },
        ..Default::default()
    };

    let result = ai.chat(request).await;
    let mut map = match result {
        Ok(resp) => parse_estimate_json(&resp.content),
        Err(e) => {
            crate::data::write_ai_debug_log(
                data_dir,
                "goal_estimate_fallback",
                &format!("AI 估时失败，回退标准粒度: {}", e),
            );
            HashMap::new()
        }
    };
    // 确保每个知识点都有估值（兜底沿用用户设置的任务粒度）
    for kp in knowledge {
        map.entry(kp.clone()).or_insert(gran);
    }
    map
}

/// 解析 AI 估时 JSON：`[{"knowledge":"...","hours":1.5}, ...]`
fn parse_estimate_json(content: &str) -> HashMap<String, f64> {
    #[derive(serde::Deserialize)]
    struct Item {
        knowledge: String,
        hours: f64,
    }
    let trimmed = content.trim();
    // clean_ai_json 会剥掉非 fenced 文本的 `[...]` 外层（按对象提取），对数组不友好。
    // 这里仅用其 fenced 分支取围栏内内容；否则直接按原始数组解析。
    let candidate = if trimmed.starts_with("```") {
        clean_ai_json(trimmed)
    } else {
        trimmed.to_string()
    };
    match serde_json::from_str::<Vec<Item>>(&candidate) {
        Ok(items) => items
            .into_iter()
            .filter(|i| !i.knowledge.is_empty() && i.hours > 0.0)
            .map(|i| (i.knowledge, (i.hours * 2.0).round() / 2.0))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

/// 复盘后双轨重排：根据复盘实际进度更新各截止日科目。
///
/// - 汇总各科实际推进到的位置（overcompletion + task_reviews 中已完成任务标题定位取最大）。
/// - 更新 goal.current_position。
/// - 达标或已过截止日 → active=false、status=completed/expired（提前退出回退默认）。
///
/// 返回受影响科目的当前 position 更新映射（供调用方决定是否重生成今日任务）。
pub fn replan_goals_after_review(
    data_dir: &Path,
    state: &crate::data::state::StudyState,
    overcompletion: &[crate::data::records::OvercompletionEntry],
    task_reviews: &[crate::data::records::TaskReviewEntry],
    today: &str,
) -> HashMap<String, usize> {
    let mut file = match read_goals(data_dir) {
        Ok(f) => f,
        Err(_) => return HashMap::new(),
    };
    let mut did_change = false;
    let mut updated: HashMap<String, usize> = HashMap::new();

    for goal in file.data.goals.iter_mut() {
        let key = subject_key_str(&goal.subject);
        let version = subject_version(state, key);

        // 已达标/已过期：标记退出并跳过
        if !goal.active {
            continue;
        }

        let new_pos =
            actual_progress_position(&goal.subject, &version, overcompletion, task_reviews);
        let base_pos = goal.current_position.unwrap_or(0);
        let advanced_pos = new_pos.map(|p| p.max(base_pos)).unwrap_or(base_pos);
        if advanced_pos > base_pos {
            goal.current_position = Some(advanced_pos);
            updated.insert(key.to_string(), advanced_pos);
            did_change = true;
        }

        // 终止判定
        let target_pos = goal.target_position.unwrap_or(usize::MAX);
        if advanced_pos >= target_pos {
            goal.active = false;
            goal.status = "completed".to_string();
            did_change = true;
        } else if today >= goal.deadline.as_str() {
            goal.active = false;
            goal.status = "expired".to_string();
            did_change = true;
        }
    }

    if did_change {
        let _ = save_goals(data_dir, &file);
    }
    updated
}

/// 从复盘内容识别某科实际推进到的知识点位置（取最大；无则 None）
fn actual_progress_position(
    subject: &SubjectKey,
    version: &str,
    overcompletion: &[crate::data::records::OvercompletionEntry],
    task_reviews: &[crate::data::records::TaskReviewEntry],
) -> Option<usize> {
    let key = subject_key_str(subject);
    let mut max_pos: Option<usize> = None;

    // 计划外进度：明确的章节位置
    for oc in overcompletion {
        if oc.subject != key {
            continue;
        }
        if let Some(p) = chapter_seq::position(key, version, &oc.chapter_reached) {
            max_pos = Some(max_pos.map_or(p, |m| m.max(p)));
        }
    }

    // 已完成的任务标题：取位置最大者代表今天推进到的位置
    for tr in task_reviews {
        if tr.subject != key {
            continue;
        }
        if tr.status != "completed" && tr.status != "partial" {
            continue;
        }
        if let Some(p) = chapter_seq::position(key, version, &tr.title) {
            max_pos = Some(max_pos.map_or(p, |m| m.max(p)));
        }
    }
    max_pos
}

/// 计算区间 [from, to] 内属于休息日（按中文名称，如"周日"）的所有具体日期。
fn rest_days_as_dates(rest_day_names: &[String], from: &str, to: &str) -> Vec<String> {
    if rest_day_names.is_empty() {
        return Vec::new();
    }
    let names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    // 解析星期名 -> 索引（0=周一 … 6=周日）
    let name_to_idx = |name: &str| names.iter().position(|n| *n == name);
    if !rest_day_names.iter().any(|n| name_to_idx(n).is_some()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 终点早于起点：区间为空。旧实现会一路 `add_days` 等 `cur == to`（永远等不到）→ 死循环。
    if to < from {
        return out;
    }
    // 兜底上限（约 10 年）：即使日期字符串异常，也不允许无限推进
    const MAX_DAYS: usize = 3660;
    let mut cur = from.to_string();
    for _ in 0..MAX_DAYS {
        if let Ok(wd) = crate::data::get_weekday(&cur) {
            let wd_name = names[wd.min(6) as usize];
            if rest_day_names.iter().any(|n| n == wd_name) {
                out.push(cur.clone());
            }
        }
        if cur == to {
            break;
        }
        match add_days(&cur, 1) {
            Ok(next) => cur = next,
            Err(_) => break,
        }
    }
    out
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backward_schedule_distributes_evenly() {
        // 6 个学习日（9/4..9/9），剩 13 个知识点 → 13/6 = 基 2 + 前 1 天 +1
        let schedule = backward_schedule("2026-09-04", "2026-09-09", 2, 15, &[]);
        assert_eq!(schedule.len(), 6);
        // 总推进 = 13
        let total: usize = schedule.iter().map(|(_, s)| s.len()).sum();
        assert_eq!(total, 13);
        // 第 1 天 3 个，其余 2 个
        assert_eq!(schedule[0].1.len(), 3);
        assert_eq!(schedule[1].1.len(), 2);
        assert_eq!(schedule[5].1.len(), 2);
    }

    #[test]
    fn backward_schedule_skips_rest_days() {
        // 9/5 是周六、9/6 是周日（2026-09-05 为周六）。
        // 用实际日期验证：排除这两个休息日
        let rest = vec!["2026-09-05".to_string(), "2026-09-06".to_string()];
        let schedule = backward_schedule("2026-09-04", "2026-09-09", 2, 15, &rest);
        // 剔掉 2 天休息日，剩余 4 个学习日
        assert_eq!(schedule.len(), 4);
        assert!(!schedule.iter().any(|(d, _)| rest.contains(d)));
    }

    #[test]
    fn backward_schedule_target_reached_is_empty() {
        let schedule = backward_schedule("2026-09-04", "2026-09-09", 10, 10, &[]);
        assert!(schedule.is_empty());
    }

    /// 截止日早于起始日：必须立即返回空，且**不能死循环**
    ///（旧实现会一路 add_days 等 `cur == deadline`，日期早于 today 时永远等不到）
    #[test]
    fn backward_schedule_with_past_deadline_returns_empty() {
        let schedule = backward_schedule("2026-09-10", "2026-09-01", 0, 5, &[]);
        assert!(schedule.is_empty());
        // 同一天不算过期，应正常排出 1 天
        let same_day = backward_schedule("2026-09-10", "2026-09-10", 0, 3, &[]);
        assert_eq!(same_day.len(), 1);
        assert_eq!(same_day[0].1, vec![1, 2, 3]);
    }

    /// 休息日展开：反向区间同样不得死循环（与 backward_schedule 同源的实现缺陷）
    #[test]
    fn rest_days_as_dates_with_reversed_range_returns_empty() {
        let rest = vec!["周日".to_string()];
        assert!(
            rest_days_as_dates(&rest, "2026-09-10", "2026-09-01").is_empty(),
            "终点早于起点时应返回空而非死循环"
        );
        // 正向仍正常：2026-09-06 是周日
        assert_eq!(
            rest_days_as_dates(&rest, "2026-09-01", "2026-09-07"),
            vec!["2026-09-06".to_string()]
        );
    }

    /// 过期目标：`plan_goal_tasks_sync` 应安全返回空（区间为空时短路）
    #[test]
    fn plan_goal_tasks_sync_returns_empty_for_expired_deadline() {
        let goal = Goal {
            id: "g-expired".to_string(),
            subject: SubjectKey::Math,
            title: "过期目标".to_string(),
            deadline: "2026-07-01".to_string(),
            target_chapter: String::new(),
            current_position: Some(0),
            target_position: Some(5),
            active: true,
            status: "active".to_string(),
            ..Default::default()
        };
        let out = plan_goal_tasks_sync(std::path::Path::new("."), &goal, "2026-07-28", "数二")
            .expect("过期目标应返回空而非报错");
        assert!(out.is_empty());
    }

    /// `current_position` 语义回归测试：它表示「已完成到的位置」，倒排必须从 +1 开始派发。
    ///
    /// 这条语义是 `create_goal`/`update_goal` 里「起始位置」取值方式的前提：
    /// 用户选的起始章本身要学，因此那里存的是 `position(start_chapter) - 1`。
    /// 一旦有人把 `backward_schedule` 改成从 `current_pos` 本身开始，起始章就会被重复学；
    /// 反之若 create/update 存了 `position`，起始章会被静默跳过（2026-09-13 修复的 bug）。
    #[test]
    fn backward_schedule_starts_right_after_current_position() {
        // 已完成到下标 4，目标下标 6 → 应推进 [5] 与 [6]
        let schedule = backward_schedule("2026-09-13", "2026-09-14", 4, 6, &[]);
        assert_eq!(schedule.len(), 2);
        assert_eq!(schedule[0].1, vec![5]);
        assert_eq!(schedule[1].1, vec![6]);
        assert!(
            !schedule.iter().any(|(_, s)| s.contains(&4)),
            "已完成的 current_position 不应再次出现在推进列表"
        );
    }

    /// 起始位置回归：`position(start_chapter) - 1` 时，起始章本身必须出现在第一个学习日。
    #[test]
    fn start_chapter_is_included_when_current_position_is_previous() {
        // 起始章下标 5 → current_position = 4（模拟 create_goal 的换算）
        let start_chapter_pos = 5usize;
        let current_position = start_chapter_pos.saturating_sub(1);
        let schedule = backward_schedule("2026-09-13", "2026-09-13", current_position, 5, &[]);
        assert_eq!(schedule.len(), 1);
        assert_eq!(
            schedule[0].1,
            vec![5],
            "用户选中的起始章（下标 5）必须被安排学习，不能被跳过"
        );
    }

    #[test]
    fn estimate_json_parsing() {
        let s = r#"[{"knowledge":"行列式","hours":2},{"knowledge":"矩阵运算","hours":1.5}]"#;
        let map = parse_estimate_json(s);
        assert_eq!(map.get("行列式"), Some(&2.0));
        assert_eq!(map.get("矩阵运算"), Some(&1.5));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn estimate_json_parsing_falls_back_empty_on_garbage() {
        let map = parse_estimate_json("not json");
        assert!(map.is_empty());
    }
}
