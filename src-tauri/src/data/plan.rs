//! Plan 数据层 — 读取/写入 JSON 学习计划
//!
//! 统一数据契约：{ version, meta, data, view? }
//! - 日计划：plan/YYYY-MM-DD_day.json
//! - 周计划：plan/YYYY-Www_week.json
//!
//! 对应前端 TypeScript 类型: types/plan.ts

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::state::{RiskLevel, RiskSubject, SubjectKey, TaskPriority, TaskStatus};
use super::{add_days, get_week_end, list_dir_files, read_file_content, DataResult};

// ============================================================================
// 通用结构
// ============================================================================

/// 计划依赖的数据源
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BasedOn {
    pub state: String,
    pub user_model: String,
    pub exam_config: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week_plan: Option<String>,
}

/// 风险提示项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanRisk {
    pub subject: RiskSubject,
    pub item: String,
    pub level: RiskLevel,
    pub suggestion: String,
}

// ============================================================================
// 周计划结构
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekPlanFile {
    pub version: String,
    pub meta: WeekPlanMeta,
    pub data: WeekPlanData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekPlanMeta {
    pub week_start: String,
    pub week_end: String,
    pub week_number: i32,
    pub generated_at: String,
    pub based_on: BasedOn,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekPlanData {
    pub goals: Vec<String>,
    pub subjects: Vec<WeekSubjectPlan>,
    pub days: Vec<WeekDayPlan>,
    /// 风险项（已废弃，仅为兼容旧 plan JSON 保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub risks: Vec<PlanRisk>,
    /// 今日提醒（已废弃，仅为兼容旧 plan JSON 保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub reminders: Vec<String>,
    /// 本周特殊情况排除日期（不生成计划，自动免复盘）
    #[serde(default)]
    pub excluded_days: Vec<ExcludedDay>,
    /// 本周任务量调整（相对上周）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload_adjustment: Option<WorkloadAdjustment>,
}

/// 特殊情况排除日（用户主动声明本周某天不学习）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExcludedDay {
    /// YYYY-MM-DD
    pub date: String,
    /// 预设类型：travel / sick / exam / other
    pub reason_type: String,
    /// 自由备注（可空）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 周任务量调整（相对上周）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkloadAdjustment {
    /// 方向：increase / decrease / unchanged
    pub direction: String,
    /// 幅度档位：small / large（仅 direction != unchanged 时有意义）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// 用户备注（可空）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekSubjectPlan {
    pub subject: SubjectKey,
    pub weekly_hours: f64,
    pub focus: String,
    pub milestones: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeekDayPlan {
    pub date: String,
    pub weekday: String,
    pub is_rest_day: bool,
    pub subject_allocations: Vec<DaySubjectAllocation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaySubjectAllocation {
    pub subject: SubjectKey,
    #[serde(default)]
    pub hours: f64,
    #[serde(default)]
    pub focus: String,
    #[serde(default)]
    pub task_templates: Vec<TaskTemplate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskTemplate {
    pub title: String,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub estimated_hours: f64,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textbook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_tips: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_plan: Option<String>,
}

// ============================================================================
// 日计划结构
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyPlanFile {
    pub version: String,
    pub meta: DailyPlanMeta,
    pub data: DailyPlanData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyPlanMeta {
    pub date: String,
    pub generated_at: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub based_on: BasedOn,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyPlanData {
    pub remaining_days: i64,
    pub target: String,
    pub strategy: String,
    pub tasks: Vec<PlanTask>,
    /// 风险项（已废弃，仅为兼容旧 plan JSON 保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub risks: Vec<PlanRisk>,
    pub style_tips: Vec<String>,
    pub after_today: String,
    /// 今日提醒（已废弃，仅为兼容旧 plan JSON 保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub reminders: Vec<String>,
    pub total_hours: f64,
    pub total_tasks: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: String,
    pub subject: SubjectKey,
    pub title: String,
    pub priority: TaskPriority,
    pub estimated_hours: f64,
    pub goal: String,
    pub completion_criteria: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub textbook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_tips: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_plan: Option<String>,
    pub status: TaskStatus,
}

// ============================================================================
// 兼容别名（用于前端过渡期间仍可使用旧名称）
// ============================================================================

/// 兼容别名：DailyPlan 等价于 DailyPlanFile（完整文件）
pub type DailyPlan = DailyPlanFile;

/// 兼容别名：WeekPlan 等价于 WeekPlanFile（完整文件）
pub type WeekPlan = WeekPlanFile;

// ============================================================================
// 常量与路径
// ============================================================================

pub const PLAN_DIR: &str = "plan";
pub const DAILY_PLAN_FILE_SUFFIX: &str = "_day.json";
pub const WEEK_PLAN_FILE_SUFFIX: &str = "_week.json";

/// 获取指定日期的日计划文件路径
pub fn daily_plan_path(data_dir: &Path, date: &str) -> PathBuf {
    data_dir
        .join(PLAN_DIR)
        .join(format!("{}{}", date, DAILY_PLAN_FILE_SUFFIX))
}

/// 获取指定 ISO 周（如 2026-W30）的周计划文件路径
pub fn week_plan_path(data_dir: &Path, iso_week: &str) -> PathBuf {
    data_dir
        .join(PLAN_DIR)
        .join(format!("{}{}", iso_week, WEEK_PLAN_FILE_SUFFIX))
}

/// 将日期转换为 ISO 8601 周标识（如 2026-W30）
pub fn iso_week_string(date: &str) -> DataResult<String> {
    let weekday = super::get_weekday(date)?; // 0=周一, 6=周日
                                             // 找到本周周四（ISO 周由周四所在年决定）
    let thursday_offset = 3i64 - weekday as i64;
    let thursday = super::add_days(date, thursday_offset)?;

    let iso_year: i32 = thursday
        .split('-')
        .next()
        .ok_or("无效日期")?
        .parse()
        .map_err(|_| "无效年份")?;

    let jan_1 = format!("{}-01-01", iso_year);
    let ordinal = super::days_between(&thursday, &jan_1)? + 1;
    let week = ((ordinal - 1) / 7 + 1) as i32;

    Ok(format!("{}-W{:02}", iso_year, week))
}

/// 列出所有日计划文件的日期 (YYYY-MM-DD)
pub fn list_daily_plan_dates(data_dir: &Path) -> DataResult<Vec<String>> {
    let plan_dir = data_dir.join(PLAN_DIR);
    let files = list_dir_files(&plan_dir)?;

    let mut dates: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?;
            if name.ends_with(DAILY_PLAN_FILE_SUFFIX) {
                Some(name.trim_end_matches(DAILY_PLAN_FILE_SUFFIX).to_string())
            } else {
                None
            }
        })
        .collect();

    dates.sort();
    dates.dedup();
    Ok(dates)
}

/// 列出所有周计划文件的周标识 (YYYY-Www)
pub fn list_week_plan_dates(data_dir: &Path) -> DataResult<Vec<String>> {
    let plan_dir = data_dir.join(PLAN_DIR);
    let files = list_dir_files(&plan_dir)?;

    let mut weeks: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?;
            if name.ends_with(WEEK_PLAN_FILE_SUFFIX) {
                Some(name.trim_end_matches(WEEK_PLAN_FILE_SUFFIX).to_string())
            } else {
                None
            }
        })
        .collect();

    weeks.sort();
    weeks.dedup();
    Ok(weeks)
}

// ============================================================================
// 读取 / 写入
// ============================================================================

/// 读取指定日期的日计划 JSON
pub fn read_daily_plan(data_dir: &Path, date: &str) -> DataResult<DailyPlanFile> {
    let path = daily_plan_path(data_dir, date);
    if !path.exists() {
        return Err(format!("日计划文件不存在: {:?}", path));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析日计划 JSON 失败: {}", e))
}

/// 保存日计划 JSON
pub fn save_daily_plan(data_dir: &Path, plan: &DailyPlanFile) -> DataResult<()> {
    let path = daily_plan_path(data_dir, &plan.meta.date);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(plan).map_err(|e| format!("序列化日计划失败: {}", e))?;
    super::atomic_write(&path, &json)
        .map_err(|e| format!("写入日计划文件失败 {:?}: {}", path, e))?;
    Ok(())
}

/// 读取指定 ISO 周的周计划 JSON
pub fn read_week_plan(data_dir: &Path, iso_week: &str) -> DataResult<WeekPlanFile> {
    let path = week_plan_path(data_dir, iso_week);
    if !path.exists() {
        return Err(format!("周计划文件不存在: {:?}", path));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析周计划 JSON 失败: {}", e))
}

/// 保存周计划 JSON（文件名自动使用 ISO 周格式）
pub fn save_week_plan(data_dir: &Path, plan: &WeekPlanFile) -> DataResult<()> {
    let iso_week = iso_week_string(&plan.meta.week_start)?;
    let path = week_plan_path(data_dir, &iso_week);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(plan).map_err(|e| format!("序列化周计划失败: {}", e))?;
    super::atomic_write(&path, &json)
        .map_err(|e| format!("写入周计划文件失败 {:?}: {}", path, e))?;
    Ok(())
}

/// 读取本周周计划（根据日期自动推导 ISO 周）
pub fn read_week_plan_for_date(data_dir: &Path, date: &str) -> DataResult<WeekPlanFile> {
    let iso_week = iso_week_string(date)?;
    read_week_plan(data_dir, &iso_week)
}

/// 读取日计划并合并任务完成状态
///
/// 状态合并优先级：
/// 1. 若 State.current_task.date 与目标日期匹配，从 State 合并实时状态（今天）
/// 2. 否则尝试从 Review.completed_tasks 合并完成状态（历史日期）
pub fn read_daily_plan_with_merged_status(
    data_dir: &Path,
    date: &str,
) -> DataResult<DailyPlanFile> {
    let mut plan = read_daily_plan(data_dir, date)?;

    // 仅今天才从 State 合并实时状态并同步；历史日期只读 Review，避免覆盖今天的 state
    let is_today = date == crate::data::today_string();

    let state_matched = if is_today {
        if let Ok(state) = crate::data::state::read_state(data_dir) {
            // 校验 state.current_task.date 与 date 一致，且 state 中任务的 task_id 日期前缀也与 date 一致
            // 防止 state 被污染（task_id 与任务内容错位）导致跨日状态继承
            if state.current_task.date == date
                && state_tasks_date_prefix_matches(&state.current_task.tasks, date)
            {
                merge_status_by_task_id(&mut plan.data.tasks, &state.current_task.tasks);
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // 历史日期：从 Review 合并完成状态
    if !state_matched {
        if let Ok(review) = crate::data::records::read_review(data_dir, date) {
            // 新版：优先从 task_reviews 合并（结构化复盘）
            if !review.task_reviews.is_empty() {
                merge_status_from_task_reviews(&mut plan.data.tasks, &review.task_reviews);
            } else {
                // 旧版：从 data.completed_tasks 合并
                merge_status_from_review(&mut plan.data.tasks, &review.data.completed_tasks);
            }
        }
    }

    // 仅今天才同步到 state（避免历史日期覆盖今天的 state，导致刷新时状态丢失）
    if is_today && (!state_matched || state_has_no_tasks_for_date(data_dir, date)) {
        if let Err(e) = sync_plan_tasks_to_state(data_dir, date, &plan) {
            log::warn!("自动同步 plan 任务到 state 失败: {}", e);
        }
    }

    Ok(plan)
}

/// 校验 state 中所有任务的 task_id 日期前缀是否都与 date 一致
///
/// task_id 格式为 `YYYY-MM-DD-NN`，取前 10 位作为日期前缀。
/// 若任一任务的 task_id 日期前缀与 date 不一致，返回 false（说明 state 被污染）。
/// 旧任务 task_id 为 None 时视为合法（由后续 legacy 匹配处理）。
fn state_tasks_date_prefix_matches(
    state_tasks: &[crate::data::state::StateTask],
    date: &str,
) -> bool {
    for st in state_tasks {
        if let Some(id) = &st.task_id {
            // task_id 格式 YYYY-MM-DD-NN，日期前缀为前 10 字符
            if id.len() >= 10 && &id[..10] != date {
                return false;
            }
        }
    }
    true
}

/// 检查 state 中是否没有当前日期的任务
fn state_has_no_tasks_for_date(data_dir: &Path, date: &str) -> bool {
    crate::data::state::read_state(data_dir)
        .map(|s| s.current_task.date != date || s.current_task.tasks.is_empty())
        .unwrap_or(true)
}

/// 将 plan 任务同步到 state.current_task
fn sync_plan_tasks_to_state(data_dir: &Path, date: &str, plan: &DailyPlanFile) -> DataResult<()> {
    let mut state = crate::data::state::read_state(data_dir).unwrap_or_default();

    // 已有任务时不覆盖（保护已完成状态）
    // 但需校验 task_id 日期前缀与 date 一致，防止被污染的 state（task_id 与任务内容错位）被保留
    if state.current_task.date == date
        && !state.current_task.tasks.is_empty()
        && state_tasks_date_prefix_matches(&state.current_task.tasks, date)
    {
        return Ok(());
    }

    use std::collections::HashMap;

    // 保留已有的任务状态（如果已有部分任务）
    // 仅继承日期前缀与 date 一致的 task_id 的状态，避免从被污染的 state 继承错位状态
    let existing_status: HashMap<&str, &crate::data::state::TaskStatus> = state
        .current_task
        .tasks
        .iter()
        .filter_map(|t| {
            t.task_id.as_ref().and_then(|id| {
                // task_id 格式 YYYY-MM-DD-NN，校验日期前缀
                if id.len() >= 10 && &id[..10] == date {
                    Some((id.as_str(), &t.status))
                } else {
                    None
                }
            })
        })
        .collect();

    let tasks: Vec<crate::data::state::StateTask> = plan
        .data
        .tasks
        .iter()
        .map(|task| {
            // 优先继承 state 中已有的完成状态；否则沿用 plan 中已合并的 status（保护 review 合并结果）
            let status = existing_status
                .get(task.id.as_str())
                .cloned()
                .cloned()
                .unwrap_or_else(|| task.status.clone());
            crate::data::state::StateTask {
                task_id: Some(task.id.clone()),
                subject: format!("{:?}", task.subject).to_lowercase(),
                task: task.title.clone(),
                priority: task.priority.clone(),
                status,
                started_at: None,
                accumulated_minutes: 0,
            }
        })
        .collect();

    let focus = if plan.data.strategy.is_empty() {
        state.current_task.focus.clone()
    } else {
        plan.data.strategy.clone()
    };

    state.current_task = crate::data::state::CurrentTask {
        date: date.to_string(),
        focus,
        total_hours: Some(plan.data.total_hours),
        tasks,
        note: String::new(),
    };

    crate::data::state::save_state(data_dir, &state)
}

/// 从 Review 的完成任务列表合并状态到 Plan 任务
///
/// 匹配策略：
/// 1. 优先按 task_id 精确匹配
/// 2. 回退到按 title 模糊匹配（兼容旧 review 无 task_id 的情况）
fn merge_status_from_review(
    plan_tasks: &mut [PlanTask],
    completed_tasks: &[crate::data::records::ReviewCompletedTask],
) {
    use crate::data::state::TaskStatus;
    use std::collections::HashMap;

    // 第一轮：构建 task_id → completed 映射
    let mut id_to_completed: HashMap<&str, bool> = HashMap::new();
    let mut title_to_completed: HashMap<&str, bool> = HashMap::new();

    for ct in completed_tasks {
        if let Some(id) = &ct.task_id {
            id_to_completed.insert(id.as_str(), ct.completed);
        }
        title_to_completed.insert(ct.title.as_str(), ct.completed);
    }

    // 第二轮：按 task_id 精确匹配
    let mut matched_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if let Some(&completed) = id_to_completed.get(pt.id.as_str()) {
            if completed {
                pt.status = TaskStatus::Done;
            }
            matched_indices.insert(idx);
        }
    }

    // 第三轮：对未匹配的任务，按 title 回退匹配
    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if matched_indices.contains(&idx) {
            continue;
        }
        if let Some(&completed) = title_to_completed.get(pt.title.as_str()) {
            if completed {
                pt.status = TaskStatus::Done;
            }
        }
    }
}

/// 从新版 task_reviews 合并任务状态到 plan
///
/// 匹配优先级：
/// 1. 按 task_id 精确匹配
/// 2. 按 title 回退匹配（task_reviews 自带的 title 字段）
///
/// 状态映射：
/// - completed → Done
/// - abandoned → Abandoned
/// - incomplete / partial → Pending（部分完成视为未完成，便于在计划页看到需要继续推进）
fn merge_status_from_task_reviews(
    plan_tasks: &mut [PlanTask],
    task_reviews: &[crate::data::records::TaskReviewEntry],
) {
    use crate::data::state::TaskStatus;
    use std::collections::HashMap;

    // 构建 task_id → status 映射
    let mut id_to_status: HashMap<&str, &str> = HashMap::new();
    let mut title_to_status: HashMap<&str, &str> = HashMap::new();

    for tr in task_reviews {
        if !tr.task_id.is_empty() {
            id_to_status.insert(tr.task_id.as_str(), tr.status.as_str());
        }
        if !tr.title.is_empty() {
            title_to_status.insert(tr.title.as_str(), tr.status.as_str());
        }
    }

    let mut matched_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // 第一轮：按 task_id 精确匹配
    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if let Some(&status_str) = id_to_status.get(pt.id.as_str()) {
            pt.status = match status_str {
                "completed" => TaskStatus::Done,
                "abandoned" => TaskStatus::Abandoned,
                _ => TaskStatus::Pending,
            };
            matched_indices.insert(idx);
        }
    }

    // 第二轮：按 title 回退匹配
    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if matched_indices.contains(&idx) {
            continue;
        }
        if let Some(&status_str) = title_to_status.get(pt.title.as_str()) {
            pt.status = match status_str {
                "completed" => TaskStatus::Done,
                "abandoned" => TaskStatus::Abandoned,
                _ => TaskStatus::Pending,
            };
        }
    }
}

/// 按 task_id 精确匹配，回退到按索引匹配（兼容旧 state 文件）
///
/// 匹配优先级：
/// 1. 优先按 task_id 精确匹配（新数据走此路径）
/// 2. 对未匹配的任务，回退到按索引匹配（仅对 state 中 task_id=None 的旧任务）
/// 3. 未匹配的 plan 任务保持原 status（不再错误地用错位状态覆盖）
///
/// 防污染保护：仅当 state task 的 task_id 日期前缀与 plan task 的 task_id 日期前缀一致时才合并状态。
/// 这避免了 state 被污染（task_id 与任务内容错位）时，把昨天的完成状态错误地应用到今天相同序号的任务上。
fn merge_status_by_task_id(
    plan_tasks: &mut [PlanTask],
    state_tasks: &[crate::data::state::StateTask],
) {
    use std::collections::HashMap;

    // 第一轮：构建 task_id → status 映射
    let mut id_to_status: HashMap<&str, &crate::data::state::TaskStatus> = HashMap::new();
    for st in state_tasks {
        if let Some(id) = &st.task_id {
            id_to_status.insert(id.as_str(), &st.status);
        }
    }

    // 第二轮：按 task_id 精确匹配
    // 额外校验：plan task 的 task_id 日期前缀必须与 state task 的 task_id 日期前缀一致
    // 防止 state 被污染（例如 task_id=2026-07-25-02 但任务内容是 07-24 的）时错误继承状态
    let mut matched_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if let Some(status) = id_to_status.get(pt.id.as_str()) {
            // 校验日期前缀一致：plan task_id 与 state task_id 必须来自同一天
            // task_id 格式 YYYY-MM-DD-NN，前 10 位为日期
            let plan_date_prefix = pt.id.get(..10);
            let state_date_prefix = status_date_prefix_from_id(pt.id.as_str(), state_tasks);
            if plan_date_prefix.is_some()
                && state_date_prefix.is_some()
                && plan_date_prefix == state_date_prefix
            {
                pt.status = (*status).clone();
                matched_indices.insert(idx);
            }
        }
    }

    // 第三轮：对未匹配的 plan 任务，回退到按索引匹配（仅对 state 中无 task_id 的旧任务）
    let mut legacy_state_iter = state_tasks
        .iter()
        .filter(|st| st.task_id.is_none())
        .enumerate();

    for (idx, pt) in plan_tasks.iter_mut().enumerate() {
        if matched_indices.contains(&idx) {
            continue;
        }
        // 找下一个未消耗的旧 state 任务
        while let Some((_, st)) = legacy_state_iter.next() {
            pt.status = st.status.clone();
            break;
        }
    }
}

/// 从 state_tasks 中找到与给定 task_id 对应的 state task，返回其 task_id 的日期前缀
/// 用于校验 plan task 与 state task 是否来自同一天
fn status_date_prefix_from_id<'a>(
    task_id: &str,
    state_tasks: &'a [crate::data::state::StateTask],
) -> Option<&'a str> {
    for st in state_tasks {
        if let Some(id) = &st.task_id {
            if id == task_id {
                return id.get(..10);
            }
        }
    }
    None
}

/// 旧的按索引合并函数（保留用于回退测试和兼容）
#[allow(dead_code)]
fn merge_status_from_current_task(
    plan_tasks: &mut [PlanTask],
    state_tasks: &[crate::data::state::StateTask],
) {
    for (idx, task) in plan_tasks.iter_mut().enumerate() {
        if let Some(state_task) = state_tasks.get(idx) {
            task.status = state_task.status.clone();
        }
    }
}

// ============================================================================
// 工具函数
// ============================================================================

/// 计算 ISO 8601 周数（简化实现：基于周一的周起始）
pub fn calculate_week_number(date: &str) -> DataResult<i32> {
    let year: i32 = date
        .split('-')
        .next()
        .ok_or("无效日期")?
        .parse()
        .map_err(|_| "无效年份")?;

    let jan_1 = format!("{}-01-01", year);
    let days = super::days_between(date, &jan_1)?;
    Ok((days / 7 + 1) as i32)
}

/// 读取某周所有日计划并聚合为周视图
///
/// 注意：返回的是 DailyPlanFile 列表，调用方可自行汇总。
pub fn read_week_daily_plans(data_dir: &Path, week_start: &str) -> DataResult<Vec<DailyPlanFile>> {
    let week_end = get_week_end(week_start)?;
    let mut result = Vec::new();
    let mut current_date = week_start.to_string();

    loop {
        if let Ok(plan) = read_daily_plan_with_merged_status(data_dir, &current_date) {
            result.push(plan);
        }
        if current_date == week_end {
            break;
        }
        current_date = add_days(&current_date, 1)?;
    }

    Ok(result)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_daily_plan() -> DailyPlanFile {
        DailyPlanFile {
            version: "1.0.0".to_string(),
            meta: DailyPlanMeta {
                date: "2026-07-25".to_string(),
                generated_at: "2026-07-25T04:00".to_string(),
                r#type: "daily".to_string(),
                based_on: BasedOn {
                    state: "state/current.state".to_string(),
                    user_model: "assets/user_model/_index.md".to_string(),
                    exam_config: "assets/config/exam-config.md".to_string(),
                    review_ref: Some("records/2026-07-24_review.json".to_string()),
                    week_plan: Some("plan/2026-W30_week.json".to_string()),
                },
            },
            data: DailyPlanData {
                remaining_days: 148,
                target: "广东工业大学 计算机技术/人工智能 | 总分 375 / 500".to_string(),
                strategy: "上午数学，下午专业课，晚上英语".to_string(),
                tasks: vec![
                    PlanTask {
                        id: "2026-07-25-01".to_string(),
                        subject: SubjectKey::Math,
                        title: "启动线代第一章：行列式".to_string(),
                        priority: TaskPriority::A,
                        estimated_hours: 2.0,
                        goal: "理解行列式的定义、性质与展开定理".to_string(),
                        completion_criteria: vec!["完成教材对应章节阅读".to_string()],
                        textbook: Some("《线性代数》同济版 第一章".to_string()),
                        style_tips: Some("例子驱动型学习者".to_string()),
                        fallback_plan: Some("若 2h 内无法完成，至少确保概念理解".to_string()),
                        status: TaskStatus::Pending,
                    },
                    PlanTask {
                        id: "2026-07-25-02".to_string(),
                        subject: SubjectKey::English,
                        title: "阅读真题第10篇 + 单词复习".to_string(),
                        priority: TaskPriority::A,
                        estimated_hours: 1.0,
                        goal: "完成英语二阅读 Text 10".to_string(),
                        completion_criteria: vec!["限时 18 分钟完成阅读".to_string()],
                        textbook: Some("历年英语二真题".to_string()),
                        style_tips: Some("通过真题语境记忆单词".to_string()),
                        fallback_plan: Some("若时间紧张，至少完成阅读与生词标记".to_string()),
                        status: TaskStatus::Pending,
                    },
                ],
                style_tips: vec!["例子驱动型学习者".to_string()],
                after_today: "若数学任务完成，明日继续推进".to_string(),
                total_hours: 3.0,
                total_tasks: 2,
                ..Default::default()
            },
            view: None,
        }
    }

    #[test]
    fn test_daily_plan_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "studyagent_plan_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let plan = sample_daily_plan();
        save_daily_plan(&tmp, &plan).expect("应能保存日计划");

        let read = read_daily_plan(&tmp, "2026-07-25").expect("应能读取日计划");
        assert_eq!(read.meta.date, "2026-07-25");
        assert_eq!(read.data.tasks.len(), 2);
        assert_eq!(read.data.tasks[0].subject, SubjectKey::Math);
        assert_eq!(read.data.tasks[0].priority, TaskPriority::A);
        assert_eq!(read.data.total_hours, 3.0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_week_plan_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "studyagent_week_plan_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

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
                days: vec![WeekDayPlan {
                    date: "2026-07-20".to_string(),
                    weekday: "周一".to_string(),
                    is_rest_day: false,
                    subject_allocations: vec![DaySubjectAllocation {
                        subject: SubjectKey::Math,
                        hours: 2.0,
                        focus: "行列式".to_string(),
                        task_templates: vec![TaskTemplate {
                            title: "行列式定义".to_string(),
                            priority: TaskPriority::A,
                            estimated_hours: 2.0,
                            goal: "理解行列式定义".to_string(),
                            completion_criteria: vec!["完成教材阅读".to_string()],
                            textbook: None,
                            style_tips: None,
                            fallback_plan: None,
                        }],
                    }],
                }],
                ..Default::default()
            },
            view: None,
        };

        save_week_plan(&tmp, &week_plan).expect("应能保存周计划");
        let read = read_week_plan(&tmp, "2026-W30").expect("应能读取周计划");
        assert_eq!(read.meta.week_number, 30);
        assert_eq!(read.data.subjects[0].subject, SubjectKey::Math);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 构造测试用 PlanTask
    fn make_plan_task(id: &str, title: &str) -> PlanTask {
        PlanTask {
            id: id.to_string(),
            subject: SubjectKey::Math,
            title: title.to_string(),
            priority: TaskPriority::A,
            estimated_hours: 1.0,
            goal: String::new(),
            completion_criteria: vec![],
            textbook: None,
            style_tips: None,
            fallback_plan: None,
            status: TaskStatus::Pending,
        }
    }

    /// 构造测试用 StateTask（带 task_id）
    fn make_state_task_with_id(task_id: &str, status: TaskStatus) -> crate::data::state::StateTask {
        crate::data::state::StateTask {
            task_id: Some(task_id.to_string()),
            subject: "math".to_string(),
            task: String::new(),
            priority: TaskPriority::A,
            status,
            ..Default::default()
        }
    }

    /// 构造测试用 StateTask（无 task_id，模拟旧版 state 文件）
    fn make_legacy_state_task(status: TaskStatus) -> crate::data::state::StateTask {
        crate::data::state::StateTask {
            task_id: None,
            subject: "math".to_string(),
            task: String::new(),
            priority: TaskPriority::A,
            status,
            ..Default::default()
        }
    }

    /// 测试 1：按 task_id 精确匹配，顺序不一致也不错位
    #[test]
    fn test_merge_by_task_id_precise_match() {
        // plan 顺序 [01, 02, 03]，state 顺序 [02, 01, 03]（顺序错乱）
        let mut plan_tasks = vec![
            make_plan_task("2026-07-25-01", "数学"),
            make_plan_task("2026-07-25-02", "英语"),
            make_plan_task("2026-07-25-03", "专业课"),
        ];
        let state_tasks = vec![
            make_state_task_with_id("2026-07-25-02", TaskStatus::Done),
            make_state_task_with_id("2026-07-25-01", TaskStatus::InProgress),
            make_state_task_with_id("2026-07-25-03", TaskStatus::Pending),
        ];

        merge_status_by_task_id(&mut plan_tasks, &state_tasks);

        // 验证：每个 plan task 的 status 与对应 task_id 的 state status 一致
        assert_eq!(
            plan_tasks[0].status,
            TaskStatus::InProgress,
            "task 01 应为 InProgress"
        );
        assert_eq!(plan_tasks[1].status, TaskStatus::Done, "task 02 应为 Done");
        assert_eq!(
            plan_tasks[2].status,
            TaskStatus::Pending,
            "task 03 应为 Pending"
        );
    }

    /// 测试 2：旧版 state 文件（task_id=None）回退到按索引匹配
    #[test]
    fn test_merge_fallback_to_index_for_legacy_state() {
        let mut plan_tasks = vec![
            make_plan_task("2026-07-25-01", "数学"),
            make_plan_task("2026-07-25-02", "英语"),
        ];
        let state_tasks = vec![
            make_legacy_state_task(TaskStatus::Done),
            make_legacy_state_task(TaskStatus::Pending),
        ];

        merge_status_by_task_id(&mut plan_tasks, &state_tasks);

        assert_eq!(
            plan_tasks[0].status,
            TaskStatus::Done,
            "legacy task 0 应为 Done"
        );
        assert_eq!(
            plan_tasks[1].status,
            TaskStatus::Pending,
            "legacy task 1 应为 Pending"
        );
    }

    /// 测试 3：plan 任务多于 state 任务时，未匹配的保持 Pending
    #[test]
    fn test_merge_unmatched_task_keeps_pending() {
        let mut plan_tasks = vec![
            make_plan_task("2026-07-25-01", "数学"),
            make_plan_task("2026-07-25-02", "英语"),
            make_plan_task("2026-07-25-03", "专业课"),
            make_plan_task("2026-07-25-04", "政治"),
        ];
        // state 只有 2 个任务（带 task_id）
        let state_tasks = vec![
            make_state_task_with_id("2026-07-25-02", TaskStatus::Done),
            make_state_task_with_id("2026-07-25-01", TaskStatus::InProgress),
        ];

        merge_status_by_task_id(&mut plan_tasks, &state_tasks);

        // task 01 和 02 按 task_id 匹配
        assert_eq!(plan_tasks[0].status, TaskStatus::InProgress);
        assert_eq!(plan_tasks[1].status, TaskStatus::Done);
        // task 03 和 04 未匹配，保持 Pending（不被错位状态覆盖）
        assert_eq!(
            plan_tasks[2].status,
            TaskStatus::Pending,
            "未匹配的 task 03 应保持 Pending"
        );
        assert_eq!(
            plan_tasks[3].status,
            TaskStatus::Pending,
            "未匹配的 task 04 应保持 Pending"
        );
    }

    /// 测试 4：混合场景（部分有 task_id，部分无）
    #[test]
    fn test_merge_mixed_legacy_and_new_state() {
        let mut plan_tasks = vec![
            make_plan_task("2026-07-25-01", "数学"),
            make_plan_task("2026-07-25-02", "英语"),
            make_plan_task("2026-07-25-03", "专业课"),
        ];
        // state: task 0 有 task_id（匹配 plan 02），task 1/2 无 task_id（走索引回退）
        let state_tasks = vec![
            make_state_task_with_id("2026-07-25-02", TaskStatus::Done),
            make_legacy_state_task(TaskStatus::Abandoned),
            make_legacy_state_task(TaskStatus::InProgress),
        ];

        merge_status_by_task_id(&mut plan_tasks, &state_tasks);

        // task 02 按 task_id 匹配，应为 Done
        assert_eq!(
            plan_tasks[1].status,
            TaskStatus::Done,
            "task 02 应按 task_id 匹配为 Done"
        );
        // task 01 和 03 走索引回退（消耗 legacy state 任务）
        // plan[0] 对应 legacy state_tasks[1] (Abandoned)
        // plan[2] 对应 legacy state_tasks[2] (InProgress)
        assert_eq!(
            plan_tasks[0].status,
            TaskStatus::Abandoned,
            "task 01 走索引回退为 Abandoned"
        );
        assert_eq!(
            plan_tasks[2].status,
            TaskStatus::InProgress,
            "task 03 走索引回退为 InProgress"
        );
    }
}
