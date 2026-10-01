//! goal_planner — 目标与截止日规划区间的确定性倒排 + 复盘双轨重排
//!
//! 职责：
//! - **任务来源确定性**：给定某科目的生效区间，用 chapter_seq 顺序表
//!   算出区间内每天「该推进到哪个知识点」，据此生成每日任务（不调 AI 决定内容）。
//! - **任务估时由 AI 参与**：把当天的知识点交给 AI 细化为带估时的任务；
//!   AI 失败时保留进度表或标题规则的知识点基准估时。
//! - **复盘双轨重排**：复盘后按「当前进度 vs 目标差距」确定性重排截止日科目
//!   的后续任务量（完成多→减少、完成少→增多、达标→提前退出）。
//!
//! 非截止日科目不受影响，继续走现有的按学习时长 AI 重排链路。

use std::collections::HashMap;
use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::core::chapter_seq;
use crate::core::date_utils::{add_days, day_diff, deadline_active_on};
use crate::data::goal::{read_goals, save_goals, Goal};
use crate::data::plan::PlanTask;
use crate::data::state::{SubjectKey, TaskPriority, TaskStatus};
use crate::data::{clean_ai_json, DataResult};

/// 书本成员以节点归属为准，不能只用整科最大位置推进所有书。
pub(crate) fn book_positions(
    index: &crate::data::progress_tables::ProgressIndex,
    subject: &str,
    version: &str,
    book: &str,
) -> Option<Vec<usize>> {
    use crate::data::progress_tables::{active_progress_table, NodeLevel};
    let table = active_progress_table(index, subject)?;
    let chapter = table
        .nodes
        .iter()
        .find(|n| n.level == NodeLevel::Chapter && n.title == book)?;
    let mut positions: Vec<usize> = table
        .nodes
        .iter()
        .filter(|n| n.level == NodeLevel::Knowledge && n.parent_id.as_deref() == Some(&chapter.id))
        .filter_map(|n| chapter_seq::position(subject, version, &n.title))
        .collect();
    if positions.is_empty() {
        positions.extend(chapter_seq::position(subject, version, &chapter.title));
    }
    positions.sort_unstable();
    positions.dedup();
    (!positions.is_empty()).then_some(positions)
}

pub(crate) fn remaining_goal_points(
    goal: &Goal,
    version: &str,
    index: &crate::data::progress_tables::ProgressIndex,
) -> DataResult<Vec<(usize, String, f64)>> {
    let target = goal.target_position.ok_or("目标未初始化 target_position")?;
    let seq =
        chapter_seq::syllabus_points(goal.subject.key(), version).ok_or("目标科目没有可用考纲")?;
    if target >= seq.len() {
        return Err("目标位置超出当前考纲，请重新选择目标知识点".into());
    }
    let first = match goal.current_position {
        Some(position) => position.saturating_add(1),
        None => chapter_seq::position(goal.subject.key(), version, &goal.start_chapter)
            .or_else(|| {
                book_positions(index, goal.subject.key(), version, &goal.book)
                    .and_then(|p| p.first().copied())
            })
            .ok_or("无法确定目标起点，请选择起始知识点")?,
    };
    let members = if goal.book.is_empty() {
        None
    } else {
        book_positions(index, goal.subject.key(), version, &goal.book)
    };
    let table = crate::data::progress_tables::active_progress_table(index, goal.subject.key());
    if first > target {
        return Ok(Vec::new());
    }
    Ok((first..=target)
        .filter(|p| members.as_ref().is_none_or(|m| m.binary_search(p).is_ok()))
        .map(|p| {
            let title = seq[p].to_string();
            let hours = table
                .and_then(|t| {
                    t.nodes
                        .iter()
                        .find(|n| n.title == title)
                        .and_then(|n| n.estimated_hours)
                })
                .filter(|h| h.is_finite() && *h > 0.0)
                .unwrap_or_else(|| {
                    crate::core::estimated_time::estimate_knowledge_hours(
                        goal.subject.key(),
                        &title,
                    )
                });
            (p, title, hours)
        })
        .collect())
}

pub(crate) fn calibrated_goal_points(
    data_dir: &Path,
    goal: &Goal,
    version: &str,
    index: &crate::data::progress_tables::ProgressIndex,
) -> DataResult<Vec<(usize, String, f64)>> {
    let mut points = remaining_goal_points(goal, version, index)?;
    let adaptive = crate::core::adaptive_planner::read_adaptive_state(data_dir).unwrap_or_default();
    let factor = adaptive
        .subjects
        .get(goal.subject.key())
        .map(|s| s.estimation_factor)
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(1.0)
        .clamp(0.8, 1.25);
    for point in &mut points {
        point.2 *= factor;
    }
    Ok(points)
}

/// 一次读取每周排除日期；休息日、未开课日期与零容量日期均不参与倒排。
pub(crate) fn available_goal_days(
    data_dir: &Path,
    from: &str,
    to: &str,
    subject: &SubjectKey,
) -> Vec<(String, f64)> {
    use crate::core::date_utils::weekday_name;
    let settings = crate::load_settings(data_dir);
    let state = crate::data::state::read_state_or_default(data_dir);
    let allocation = settings.subject_time_allocation();
    let active_hours: f64 = SubjectKey::ALL
        .iter()
        .filter(|s| s.state(&state).active)
        .map(|s| s.state(&state).weekly_hours.max(0.0))
        .sum();
    let share = allocation
        .as_ref()
        .map(|a| a.get(subject.key()).copied().unwrap_or(0.0) / 100.0)
        .unwrap_or_else(|| {
            if active_hours > 0.0 {
                subject.state(&state).weekly_hours.max(0.0) / active_hours
            } else {
                1.0 / SubjectKey::ALL
                    .iter()
                    .filter(|s| s.state(&state).active)
                    .count()
                    .max(1) as f64
            }
        });
    let capacity = settings.daily_target_hours().max(0.0) * share;
    if to < from || !capacity.is_finite() || capacity <= 0.0 {
        return Vec::new();
    }
    let rest = settings.rest_days();
    let starts = settings.subject_start_dates();
    let start = starts
        .iter()
        .find(|(k, _)| *k == subject.key())
        .map(|(_, d)| d.as_str())
        .unwrap_or("");
    let mut weeks = HashMap::new();
    let mut out = Vec::new();
    let mut date = from.to_string();
    for _ in 0..3660 {
        let Ok(week) = crate::data::iso_week_string(&date) else {
            break;
        };
        let excluded = weeks.entry(week.clone()).or_insert_with(|| {
            crate::data::plan::read_week_plan(data_dir, &week)
                .ok()
                .map(|p| {
                    p.data
                        .excluded_days
                        .into_iter()
                        .map(|d| d.date)
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default()
        });
        if date.as_str() >= start
            && weekday_name(&date).is_ok_and(|w| !rest.contains(&w))
            && !excluded.contains(&date)
        {
            out.push((date.clone(), capacity));
        }
        if date == to {
            break;
        }
        let Ok(next) = add_days(&date, 1) else { break };
        date = next;
    }
    out
}

/// 按累计工时和累计容量的比例切分，保留考纲顺序及完整知识点的真实估时。
fn weighted_schedule(
    points: &[(usize, String, f64)],
    days: &[(String, f64)],
) -> Vec<(String, Vec<usize>)> {
    let demand: f64 = points.iter().map(|p| p.2).sum();
    let capacity: f64 = days.iter().map(|d| d.1).sum();
    if demand <= 0.0 || capacity <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut position = 0;
    let mut used = 0.0;
    let mut elapsed_capacity = 0.0;
    for (i, (date, hours)) in days.iter().enumerate() {
        elapsed_capacity += hours;
        let boundary = demand * elapsed_capacity / capacity;
        let mut slice = Vec::new();
        while position < points.len()
            && (i + 1 == days.len() || used + points[position].2 / 2.0 <= boundary + 1e-9)
        {
            slice.push(points[position].0);
            used += points[position].2;
            position += 1;
        }
        if !slice.is_empty() {
            out.push((date.clone(), slice));
        }
    }
    out
}

/// 按截止日累计检查同科多书需求，共享同一份容量，不能给每本书重复计算整科预算。
pub(crate) fn goal_capacity_warnings(data_dir: &Path, date: &str) -> Vec<String> {
    let Ok(file) = read_goals(data_dir) else {
        return Vec::new();
    };
    let state = crate::data::state::read_state_or_default(data_dir);
    let index = crate::data::progress_tables::load_progress_index(data_dir);
    let mut goals: Vec<_> = file
        .data
        .goals
        .iter()
        .filter(|g| g.active && g.deadline.as_str() >= date)
        .collect();
    goals.sort_by(|a, b| a.deadline.cmp(&b.deadline));
    let mut demands = HashMap::<String, f64>::new();
    let mut warnings = Vec::new();
    for goal in goals {
        let version = subject_version(&state, goal.subject.key());
        let Ok(points) = calibrated_goal_points(data_dir, goal, &version, &index) else {
            continue;
        };
        let demand = demands.entry(goal.subject.key().into()).or_default();
        *demand += points.iter().map(|p| p.2).sum::<f64>();
        let available: f64 = available_goal_days(data_dir, date, &goal.deadline, &goal.subject)
            .iter()
            .map(|d| d.1)
            .sum();
        if *demand > available + 1e-9 {
            warnings.push(format!("{}截至 {} 的目标累计需要约 {:.1}h，可用约 {:.1}h，缺口约 {:.1}h；请调整截止日期或学习容量。", goal.subject.label(), goal.deadline, demand, available, *demand - available));
        }
    }
    warnings
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
#[cfg(test)]
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
/// - 按知识点工时与日容量确定当天的推进内容（供同步 scheduler 使用）。
/// - 需要 AI 参与估时时，请使用 `plan_goal_tasks`（异步版）。
pub fn plan_goal_tasks_sync(
    data_dir: &Path,
    goal: &Goal,
    date: &str,
    version: &str,
) -> DataResult<Vec<PlanTask>> {
    // 空区间短路：截止日缺失或早于 `date`（已过期）时，倒排区间为空。
    // 必须在这里返回，而不是依赖下游——下游的 `rest_days_as_dates` / `backward_schedule`
    // 都按「从 date 逐日推进到 deadline」实现，区间反向时会等待永远不成立的
    // `cur == deadline` 而陷入死循环（2026-09-13 由运行时冒烟测试发现）。
    if goal.deadline.is_empty() || goal.deadline.as_str() < date {
        return Ok(Vec::new());
    }

    // 把「周日」这类休息日名称转成区间 [date, deadline] 内的具体日期集合
    let index = crate::data::progress_tables::load_progress_index(data_dir);
    let knowledge_points = calibrated_goal_points(data_dir, goal, version, &index)?;
    let start = if goal.planning_start.is_empty() {
        date
    } else {
        &goal.planning_start
    };
    crate::core::date_utils::validate_date(start)?;
    let days = available_goal_days(data_dir, start, &goal.deadline, &goal.subject);
    let schedule = weighted_schedule(&knowledge_points, &days);
    let day_slice = schedule.iter().find(|(d, _)| d == date).map(|(_, s)| s);
    let Some(pos_slice) = day_slice else {
        // 该日期不在倒排区间内（无推进）
        return Ok(Vec::new());
    };

    let subject = &goal.subject;
    let knowledge: Vec<(String, f64)> = pos_slice
        .iter()
        .filter_map(|&p| {
            knowledge_points
                .iter()
                .find(|point| point.0 == p)
                .map(|point| (point.1.clone(), point.2))
        })
        .collect();
    if knowledge.is_empty() {
        return Ok(Vec::new());
    }

    let tasks: Vec<PlanTask> = knowledge
        .into_iter()
        .enumerate()
        .map(|(i, (kp, hours))| PlanTask {
            id: format!("{}-{:02}", date, i + 1),
            subject: subject.clone(),
            title: format!(
                "（{}）{}{}{}",
                subject.label(),
                goal.book,
                if goal.book.is_empty() { "" } else { "｜" },
                kp
            ),
            priority: TaskPriority::A,
            estimated_hours: hours,
            goal: format!("推进至「{}」", kp),
            completion_criteria: vec![format!("完成 {} 的学习", kp)],
            textbook: (!goal.book.is_empty()).then(|| goal.book.clone()),
            style_tips: None,
            fallback_plan: None,
            status: TaskStatus::Pending,
            dida_task_id: None,
            source: crate::data::state::TaskSource::Ai,
            ai_reference: true,
        })
        .collect();

    Ok(tasks)
}

/// 为某科目当天生成任务，并在任务来源确定后由 AI 参与估时。
///
/// 流程：`plan_goal_tasks_sync` 确定今天推进的知识点 → AI 估算每条时长 → 叠加到任务上。
/// AI 失败时保留知识点基准估时。
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
    let prefix = format!("（{}）", subject.label());
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
    Ok(tasks)
}

/// 用 AI 估算当天每个知识点的学习时长（小时）。
///
/// AI 失败时返回空 map，调用方保留基准估时。
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
         按知识点的真实学习难度估算，不得为了满足预算而缩短估时。用户任务粒度为 {:.2} 小时，仅作为后续拆分参考。\
         知识点：{}\n\
         只返回 JSON 数组，每项 {json_example},不要输出其他内容。",
        subject.label(),
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
    let map = match result {
        Ok(resp) => parse_estimate_json(&resp.content),
        Err(e) => {
            crate::data::write_ai_debug_log(
                data_dir,
                "goal_estimate_fallback",
                &format!("AI 估时失败，保留知识点基准估时: {}", e),
            );
            HashMap::new()
        }
    };
    // 缺失项保留同步倒排使用的知识点估时，不能用任务粒度覆盖真实工作量。
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
            .filter(|i| !i.knowledge.is_empty() && i.hours.is_finite() && i.hours > 0.0)
            .map(|i| (i.knowledge, ((i.hours * 2.0).round() / 2.0).clamp(0.5, 4.0)))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

/// 目标超期顺延的**封顶日**：目标截止日 + 缓冲天数。
///
/// 缓冲天数由 `AppSettings::goal_extension_max_days` 给出（用户显式配置优先，
/// 否则按每日学习时段跨度推导，夹在 [7, 180] 天）；为 0 表示**不封顶**。
/// 放在这里只是把「+N 天」的日期运算收口，口径单一真源仍在设置层。
fn goal_extension_cap_date(extend_base: &str, cap_days: i64) -> Result<String, String> {
    if cap_days <= 0 || extend_base.is_empty() {
        return Ok(String::new());
    }
    add_days(extend_base, cap_days)
}

/// 单条目标的顺延/终止判定（就地修改，返回是否有改写）。
///
/// 抽成纯函数是为了让「未达标 → 顺延 1 天 → 直至完成」的主路径可被单元测试直接覆盖
/// （`replan_goals_after_review` 需要 data_dir + StudyState，不便构造多轮场景）。
///
/// 流水线（**顺序不可颠倒**）：
/// 1. 终态短路：`completed`（`!active` 且 status != "expired"）→ 不动，返回 false。
/// 2. **先顺延**：未达标（`!deadline_active_on`，含已过期、deadline 为空）→ deadline 推进一步，
///    并把 `active`/`status` 拉回 "active"。
///    步长以**基准日** `extend_base` 为原点累计：deadline 落在 base+N 天，本次取 base+N+1。
///    因此「同一天多次复盘」最多推进到 base+1（再次进入时 deadline 已 == today，自然停下），
///    「长期未复盘」也只推进一天，不会一次性补齐到 today。
/// 3. **后封顶**：`cap_days > 0` 时以「基准日 + cap_days」为封顶日。
///    截止日越过封顶日则回填到封顶日；`today` 严格越过截止日（或截止日本就超出封顶日）
///    才置 `expired`。**封顶日当天仍生效**——否则「顺延到封顶日」这一步会在同一天被自己判死。
///
/// 为什么先顺延后封顶：长期未复盘时若先判封顶，deadline 会停在基准日上被直接判死，
/// 用户看不到「实际能追到哪一天」；先顺延则至少推进一天，语义更直观。
fn extend_goal_deadline_on_miss(
    goal: &mut Goal,
    today: &str,
    cap_days: i64,
    extend_base: &str,
) -> Result<bool, String> {
    if !goal.active && goal.status != "expired" {
        return Ok(false);
    }

    let mut changed = false;

    // 第 1 步：未达标（deadline 早于 today，含已过期、deadline 为空）→ 顺延一天。
    //
    // 步长以**基准日** `extend_base` 为原点累计：deadline 落在 base+N 天，本次即取 base+N+1。
    // 于是「同一天多次复盘」最多推进到 base+1，第二次进来 deadline 已 == today 便停下；
    // 「长期未复盘」也只推进一天，不会一次性补齐到 today。
    if !deadline_active_on(&goal.deadline, today) {
        let base = if extend_base.is_empty() {
            goal.deadline.as_str()
        } else {
            extend_base
        };
        let step = if goal.deadline.is_empty() {
            0
        } else {
            day_diff(base, &goal.deadline)
        };
        let next = add_days(base, if step <= 0 { 1 } else { step + 1 })?;
        // 截止日真的被改写，或状态被从 expired/非 active 拉回 → 都算有改写（调用方据此落盘）
        if next != goal.deadline || !goal.active || goal.status != "active" {
            changed = true;
        }
        goal.deadline = next;
        goal.active = true;
        goal.status = "active".to_string();
    }

    // 第 2 步：封顶判定（必须晚于顺延）。
    //
    // 顺序很重要：先顺延再判封顶，才能保证「截止日至少推进到 base+1」，
    // 否则长期未复盘的目标会停在基准日上被直接判死，用户看不到实际能追到哪天。
    // 边界：封顶日**当天**仍生效，严格越过（today > 封顶日）才置 expired。
    if cap_days > 0 {
        let cap = goal_extension_cap_date(extend_base, cap_days)?;
        if !cap.is_empty() {
            let was_active = goal.active;
            // 截止日可能被手工改到封顶日之后，统一回填，保证「deadline = 实际能追到的最后一天」
            let deadline_beyond_cap = goal.deadline.as_str() > cap.as_str();
            if deadline_beyond_cap {
                goal.deadline = cap;
            }
            if today > goal.deadline.as_str() || deadline_beyond_cap {
                goal.active = false;
                goal.status = "expired".to_string();
            }
            changed |= goal.active != was_active;
        }
    }

    Ok(changed)
}

/// 复盘后双轨重排：根据复盘实际进度更新各截止日科目。
///
/// - 汇总各科实际推进到的位置（overcompletion + task_reviews 中已完成任务标题定位取最大）。
/// - 更新 goal.current_position。
/// - **达标** → active=false、status=completed（提前退出回退默认）。
/// - **未达标** → 把 deadline 顺延一天并**保持 active**（自动延期，直至完成为止；
///   步长以基准日累计，详见 `extend_goal_deadline_on_miss`）。
/// - **已完成的条目不回退**（`!active` 且 status != "expired" 直接跳过）。
/// - **历史遗留的过期条目**（status == "expired"）按未达标处理，同样走顺延路径：
///   它们在没有自动延期能力时被判过期，不应就此永久失效。
/// - 封顶：`AppSettings::goal_extension_max_days` > 0 时，`deadline` 超过
///   「基准截止日 + 该天数」即不再顺延，置 `active=false`、status=expired
///   （基准日由目标文件里的 `extend_base_deadline` 记录）。
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
    // 顺延封顶阈值：用户显式配置优先，否则按每日学习时段跨度 / 目标学时推导（0 = 不封顶）
    let cap_days = crate::load_settings(data_dir).goal_extension_max_days();
    let mut did_change = false;
    let mut updated: HashMap<String, usize> = HashMap::new();
    let index = crate::data::progress_tables::load_progress_index(data_dir);
    let daily = crate::data::plan::read_daily_plan(data_dir, today).ok();

    for goal in file.data.goals.iter_mut() {
        let key = goal.subject.key();
        let version = subject_version(state, key);

        // 已达标（completed）的条目终态，不再推进也不再顺延
        if !goal.active && goal.status != "expired" {
            continue;
        }
        if let Ok(next_day) = add_days(today, 1) {
            if goal.planning_start.as_str() < next_day.as_str() {
                goal.planning_start = next_day;
                did_change = true;
            }
        }
        // 旧数据没有封顶基准：把当前（尚未被顺延过的）截止日作为基准。
        // 历史遗留的 expired 条目会丢掉原本的真实截止日，只能以现状为基准，可接受。
        if goal.extend_base_deadline.is_empty() && !goal.deadline.is_empty() {
            goal.extend_base_deadline = goal.deadline.clone();
            did_change = true;
        }

        let members = if goal.book.is_empty() {
            None
        } else {
            book_positions(&index, key, &version, &goal.book)
        };
        let belongs = |position: usize, title: &str, task_id: Option<&str>| {
            if goal.book.is_empty() {
                return true;
            }
            if let Some(task) =
                task_id.and_then(|id| daily.as_ref()?.data.tasks.iter().find(|t| t.id == id))
            {
                if let Some(book) = &task.textbook {
                    if book != &goal.book {
                        return false;
                    }
                }
            }
            members
                .as_ref()
                .map(|m| m.binary_search(&position).is_ok())
                .unwrap_or_else(|| title.contains(&format!("{}｜", goal.book)))
        };
        let filtered_over: Vec<_> = overcompletion
            .iter()
            .filter(|oc| {
                chapter_seq::position(key, &version, &oc.chapter_reached)
                    .is_some_and(|p| belongs(p, &oc.chapter_reached, None))
            })
            .cloned()
            .collect();
        let filtered_tasks: Vec<_> = task_reviews
            .iter()
            .filter(|tr| {
                chapter_seq::position(key, &version, &tr.title)
                    .is_some_and(|p| belongs(p, &tr.title, Some(&tr.task_id)))
            })
            .cloned()
            .collect();
        let new_pos =
            actual_progress_position(&goal.subject, &version, &filtered_over, &filtered_tasks);
        let advanced_pos = match (goal.current_position, new_pos) {
            (Some(base), Some(next)) => Some(base.max(next)),
            (base, next) => next.or(base),
        };
        if advanced_pos != goal.current_position {
            goal.current_position = advanced_pos;
            if let Some(p) = advanced_pos {
                updated.insert(key.to_string(), p);
            }
            did_change = true;
        }

        let target_pos = goal.target_position.unwrap_or(usize::MAX);
        if advanced_pos.is_some_and(|p| p >= target_pos) {
            // 达标：结束目标，回退默认安排
            goal.active = false;
            goal.status = "completed".to_string();
            did_change = true;
            continue;
        }

        // 未达标：自动顺延（直至完成）；越过封顶阈值才真正过期
        let base = goal.extend_base_deadline.clone();
        match extend_goal_deadline_on_miss(goal, today, cap_days, &base) {
            Ok(changed) => did_change |= changed,
            Err(e) => {
                // 日期异常：不静默失败，记录原因并保持原状态等人工处理
                crate::data::write_ai_debug_log(
                    data_dir,
                    "goal_deadline_extend_failed",
                    &format!(
                        "目标 {}（{}）截止日顺延失败: {}",
                        goal.id,
                        goal.subject.label(),
                        e
                    ),
                );
            }
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
    let key = subject.key();
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
#[cfg(test)]
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

    // ── 超期自动顺延（直至完成） ──────────────────────────────────────────

    fn goal_at(deadline: &str, position: usize, target: usize) -> Goal {
        Goal {
            id: "g-ext".to_string(),
            subject: SubjectKey::Math,
            title: "目标".to_string(),
            deadline: deadline.to_string(),
            extend_base_deadline: deadline.to_string(),
            current_position: Some(position),
            target_position: Some(target),
            active: true,
            status: "active".to_string(),
            ..Default::default()
        }
    }

    /// 未达标 → 顺延 1 天并保持生效；连续多轮逐步追上（核心需求：「直至完成」）
    #[test]
    fn deadline_extends_one_day_until_reached() {
        // 18 号到期、还差 3 个知识点：每轮复盘只推进 1 个 → 需要顺延两次
        let mut g = goal_at("2026-09-18", 7, 10);
        // 截止日当天复盘（当天任务还没做完）：deadline 不早于 today → 不顺延，仍生效
        assert!(!extend_goal_deadline_on_miss(&mut g, "2026-09-18", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-18");
        assert!(g.active, "截止日当天不应被判过期");

        // 次日仍未达标 → 顺延到 19 号，继续生效
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-19", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-19");
        assert!(g.active);
        assert_eq!(g.status, "active");

        // 20 号仍未达标 → 再顺延一天
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-20", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-20");
        assert!(g.active);

        // 21 号仍未达标 → 再顺延一天（说明「不封顶」下会一直追下去）
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-21", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-21");

        // 22 号达标：调用方先判达标并置 completed，本函数随即停手
        g.current_position = Some(10);
        g.active = false;
        g.status = "completed".to_string();
        assert!(!extend_goal_deadline_on_miss(&mut g, "2026-09-22", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-21", "达标后截止日不应再被顺延");
    }

    /// 同一天多次复盘：截止日已被顺延到 today 后不再重复顺延（防止日期被无限前推）
    #[test]
    fn deadline_is_extended_at_most_once_per_day() {
        // 18 号到期，隔两天后才复盘：只顺延 1 天（19 号），不是直接补到 20 号
        let mut g = goal_at("2026-09-18", 7, 10);
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-20", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-19", "每次复盘只 +1 天");
        // 同一天再复盘两次：deadline 早于 today，各顺延 1 天
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-20", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-20");
        // 此时 deadline == today → 当天不再顺延（防止同一天反复复盘把日期一直前推）
        assert!(!extend_goal_deadline_on_miss(&mut g, "2026-09-20", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-20");
    }

    /// 历史遗留的 expired 条目应被重新拉起（旧版本没有顺延能力，不应永久失效）
    #[test]
    fn expired_goal_is_revived_by_extension() {
        let mut g = goal_at("2026-09-10", 3, 40);
        g.active = false;
        g.status = "expired".to_string();
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-18", 0, "2026-09-10").unwrap());
        assert_eq!(g.deadline, "2026-09-11");
        assert!(g.active);
        assert_eq!(g.status, "active");
    }

    /// completed 是终态：不再顺延，也不再被拉起
    #[test]
    fn completed_goal_is_never_extended() {
        let mut g = goal_at("2026-09-10", 40, 40);
        g.active = false;
        g.status = "completed".to_string();
        assert!(!extend_goal_deadline_on_miss(&mut g, "2026-09-18", 0, "2026-09-10").unwrap());
        assert_eq!(g.deadline, "2026-09-10");
        assert_eq!(g.status, "completed");
    }

    /// 封顶：顺延产物落在 [基准日, 封顶日] 区间内，越过封顶即真正过期
    #[test]
    fn extension_stops_at_cap() {
        // 基准 9/18 + 2 天 = 9/20 为封顶日
        let mut g = goal_at("2026-09-18", 7, 10);
        // 逐日顺延：18 →(19)→(20)，20 就是封顶日，仍生效（19 号那次只推进到 19）
        extend_goal_deadline_on_miss(&mut g, "2026-09-19", 2, "2026-09-18").unwrap();
        assert_eq!(g.deadline, "2026-09-19");
        for day in ["2026-09-19", "2026-09-20"] {
            extend_goal_deadline_on_miss(&mut g, day, 2, "2026-09-18").unwrap();
        }
        assert_eq!(g.deadline, "2026-09-20", "顺延到封顶日为止");
        assert!(g.active, "到达封顶日当天仍应生效");

        // 次日复盘：封顶日已过 → 不再顺延，目标真正过期
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-21", 2, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-20", "过期后截止日停在封顶日");
        assert!(!g.active);
        assert_eq!(g.status, "expired");
    }

    /// 封顶日已过去时（长期未复盘）直接判过期；截止日保留在最后一次顺延到的日期，
    /// 不跨越封顶日
    #[test]
    fn cap_reached_today_marks_expired() {
        // 基准 9/18 + 3 天 = 9/21，today 已是 9/28（远超过封顶）
        let mut g = goal_at("2026-09-18", 7, 10);
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-28", 3, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-19", "过期时截止日不超过封顶日");
        assert!(!g.active);
        assert_eq!(g.status, "expired");
    }

    /// 封顶前 deadline 到达 today：当天不再顺延（避免同一天反复复盘把日期推着走），
    /// 也不判过期——封顶只由 `today > 封顶日`（严格越过）触发。
    #[test]
    fn deadline_stops_at_today_before_cap() {
        // 基准 9/18 + 2 天 = 9/20 为封顶日；20 号时 deadline 已被推到 20
        let mut g = goal_at("2026-09-18", 7, 10);
        for day in ["2026-09-19", "2026-09-20"] {
            extend_goal_deadline_on_miss(&mut g, day, 2, "2026-09-18").unwrap();
        }
        assert_eq!(g.deadline, "2026-09-20");
        assert!(g.active);

        // 同日再复盘：deadline == today 且 today == 封顶日 → 不判过期（严格大于才过期）
        assert!(!extend_goal_deadline_on_miss(&mut g, "2026-09-20", 2, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-20");
        assert!(g.active, "封顶日当天仍生效，不应被判过期");
        assert_eq!(g.status, "active");

        // 次日（超过封顶日）→ 真正过期
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-09-21", 2, "2026-09-18").unwrap());
        assert!(!g.active);
        assert_eq!(g.status, "expired");
    }

    /// cap_days = 0 表示不封顶（用户显式选择，或配置缺失时的安全默认）：
    /// 无论拖多久都不会被判过期，直到达标
    #[test]
    fn zero_cap_means_unlimited() {
        let mut g = goal_at("2026-09-18", 7, 10);
        // 隔一个月未复盘 → 只 +1 天，仍生效
        assert!(extend_goal_deadline_on_miss(&mut g, "2026-10-19", 0, "2026-09-18").unwrap());
        assert_eq!(g.deadline, "2026-09-19");
        assert!(g.active);
        // 隔一年仍未达标 → 依然不封顶
        assert!(extend_goal_deadline_on_miss(&mut g, "2027-09-19", 0, "2026-09-18").unwrap());
        assert!(g.active);
        assert_eq!(g.status, "active");
        assert_eq!(g.deadline, "2026-09-20");
    }

    /// 封顶日由基准日推算，与当前（已被顺延的）deadline 无关
    #[test]
    fn cap_is_derived_from_base_not_current_deadline() {
        assert_eq!(
            goal_extension_cap_date("2026-09-18", 30).unwrap(),
            "2026-10-18"
        );
        // 不封顶 / 无基准 → 空串（调用方据此跳过封顶判定）
        assert!(goal_extension_cap_date("2026-09-18", 0).unwrap().is_empty());
        assert!(goal_extension_cap_date("", 30).unwrap().is_empty());
    }

    #[test]
    fn first_syllabus_point_is_not_already_completed() {
        let seq = chapter_seq::syllabus_points("math", "数二").unwrap();
        let goal = Goal {
            subject: SubjectKey::Math,
            start_chapter: seq[0].into(),
            target_position: Some(0),
            current_position: None,
            ..Default::default()
        };
        let points = remaining_goal_points(&goal, "数二", &Default::default()).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].0, 0);
        let schedule = weighted_schedule(&points, &[("2026-09-30".into(), 2.0)]);
        assert_eq!(schedule[0].1, vec![0]);
    }

    #[test]
    fn weighted_schedule_balances_hours_instead_of_point_counts() {
        let points = vec![
            (0, "难点".into(), 3.0),
            (1, "概念".into(), 1.0),
            (2, "定义".into(), 1.0),
            (3, "性质".into(), 1.0),
        ];
        let days = vec![("2026-09-30".into(), 3.0), ("2026-10-01".into(), 3.0)];
        let schedule = weighted_schedule(&points, &days);
        assert_eq!(schedule[0].1, vec![0]);
        assert_eq!(schedule[1].1, vec![1, 2, 3]);
        assert!(weighted_schedule(&points, &[]).is_empty());
    }

    #[test]
    fn review_progress_is_isolated_by_book_but_still_allows_jumps() {
        use crate::data::progress_tables::*;
        let dir = std::env::temp_dir().join(format!(
            "sa_goal_isolation_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let seq = chapter_seq::syllabus_points("math", "数二").unwrap();
        let mut state = crate::data::state::StudyState::default();
        state.subjects.math.version = Some("数二".into());
        let nodes = vec![
            ProgressNode {
                id: "a".into(),
                title: "书甲".into(),
                level: NodeLevel::Chapter,
                ..Default::default()
            },
            ProgressNode {
                id: "b".into(),
                title: "书乙".into(),
                level: NodeLevel::Chapter,
                ..Default::default()
            },
            ProgressNode {
                id: "a1".into(),
                title: seq[1].into(),
                parent_id: Some("a".into()),
                level: NodeLevel::Knowledge,
                ..Default::default()
            },
            ProgressNode {
                id: "a3".into(),
                title: seq[3].into(),
                parent_id: Some("a".into()),
                level: NodeLevel::Knowledge,
                ..Default::default()
            },
            ProgressNode {
                id: "b10".into(),
                title: seq[10].into(),
                parent_id: Some("b".into()),
                level: NodeLevel::Knowledge,
                ..Default::default()
            },
        ];
        let index = ProgressIndex {
            subjects: HashMap::from([(
                "math".into(),
                SubjectProgressSet {
                    active_id: "t".into(),
                    active_variant: "数二".into(),
                    tables: vec![ProgressTable {
                        id: "t".into(),
                        variant: "数二".into(),
                        nodes,
                        ..Default::default()
                    }],
                },
            )]),
            ..Default::default()
        };
        save_progress_index(&dir, &index).unwrap();
        let mut file = crate::data::goal::GoalPlanFile::default();
        for (book, target) in [("书甲", 3), ("书乙", 10)] {
            file.data.goals.push(Goal {
                id: book.into(),
                book: book.into(),
                subject: SubjectKey::Math,
                deadline: "2026-10-30".into(),
                current_position: Some(0),
                target_position: Some(target),
                active: true,
                status: "active".into(),
                ..Default::default()
            });
        }
        save_goals(&dir, &file).unwrap();
        let review = crate::data::records::TaskReviewEntry {
            subject: "math".into(),
            title: seq[10].into(),
            status: "completed".into(),
            ..Default::default()
        };
        replan_goals_after_review(&dir, &state, &[], &[review], "2026-09-30");
        let file = read_goals(&dir).unwrap();
        assert_eq!(file.data.goals[0].current_position, Some(0));
        assert!(file.data.goals[0].active);
        assert_eq!(file.data.goals[1].current_position, Some(10));
        assert_eq!(file.data.goals[1].status, "completed");
        let jump = crate::data::records::TaskReviewEntry {
            subject: "math".into(),
            title: seq[3].into(),
            status: "partial".into(),
            ..Default::default()
        };
        replan_goals_after_review(&dir, &state, &[], &[jump], "2026-09-30");
        assert_eq!(
            read_goals(&dir).unwrap().data.goals[0].current_position,
            Some(3)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn batch_goal_generation_uses_one_schedule_anchor() {
        let dir = std::env::temp_dir().join(format!(
            "sa_goal_anchor_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let goal = Goal {
            subject: SubjectKey::Math,
            planning_start: "2026-09-28".into(),
            deadline: "2026-10-02".into(),
            current_position: Some(0),
            target_position: Some(8),
            ..Default::default()
        };
        let mut titles = std::collections::HashSet::new();
        for date in [
            "2026-09-28",
            "2026-09-29",
            "2026-09-30",
            "2026-10-01",
            "2026-10-02",
        ] {
            for task in plan_goal_tasks_sync(&dir, &goal, date, "数二").unwrap() {
                assert!(
                    titles.insert(task.title),
                    "批量生成不同日期不能重复派发同一知识点"
                );
            }
        }
        assert_eq!(titles.len(), 8);
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
