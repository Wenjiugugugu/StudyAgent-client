//! State 数据层 — 读取/解析 `state/current.state` (TOML)
//!
//! 对应前端 TypeScript 类型: `types/state.ts`

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{read_file_content, DataResult};

// ============================================================================
// 自定义反序列化函数 — 同时接受整数和浮点数，用于 f64 字段
// ============================================================================

/// 自定义反序列化函数，同时接受整数和浮点数，用于 f64 字段
///
/// TOML 中整数（如 `weekly_hours = 0`）不会自动转为 Rust 的 `f64`，
/// 此函数先解析为 `toml::Value`，再统一转换为 `f64`，避免反序列化失败。
fn deserialize_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    // 先尝试解析为 toml::Value
    let v = toml::Value::deserialize(deserializer)?;
    match v {
        toml::Value::Integer(i) => Ok(i as f64),
        toml::Value::Float(f) => Ok(f),
        toml::Value::String(s) => s
            .parse::<f64>()
            .map_err(|e| serde::de::Error::custom(format!("无法将 '{}' 转换为 f64: {}", s, e))),
        _ => Err(serde::de::Error::custom("期望整数、浮点数或字符串")),
    }
}

/// 自定义反序列化函数，用于 Option<f64> 字段
///
/// 同时接受整数、浮点数和字符串（空字符串或"未记录"视为 None）。
fn deserialize_option_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = toml::Value::deserialize(deserializer)?;
    match v {
        toml::Value::Integer(i) => Ok(Some(i as f64)),
        toml::Value::Float(f) => Ok(Some(f)),
        toml::Value::String(s) => {
            if s.is_empty() || s == "未记录" {
                Ok(None)
            } else {
                s.parse::<f64>().map(Some).map_err(|e| {
                    serde::de::Error::custom(format!("无法将 '{}' 转换为 f64: {}", s, e))
                })
            }
        }
        _ => Ok(None),
    }
}

// ============================================================================
// 枚举类型
// ============================================================================

/// 科目标识
///
/// 反序列化保持严格：AI 应在 prompt 中被明确约束只能使用 `math`/`english`/`politics`/`professional`，
/// 出现未知值时直接报错（解析失败），便于发现 prompt 失效。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKey {
    Math,
    English,
    Politics,
    Professional,
}

impl Default for SubjectKey {
    fn default() -> Self {
        SubjectKey::Math
    }
}

/// 学习阶段
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StudyPhase {
    Foundation,
    Strengthen,
    Sprint,
    Mock,
    Complete,
}

impl Default for StudyPhase {
    fn default() -> Self {
        StudyPhase::Foundation
    }
}

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
    Abandoned,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

/// 任务优先级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TaskPriority {
    A,
    B,
    C,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::A
    }
}

/// 风险等级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for RiskLevel {
    fn default() -> Self {
        RiskLevel::Medium
    }
}

/// 风险主体（SubjectKey 或 "overall"）
///
/// 严格反序列化：仅接受 `math`/`english`/`politics`/`professional`/`overall`。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RiskSubject {
    Math,
    English,
    Politics,
    Professional,
    #[default]
    Overall,
}

// ============================================================================
// State 数据结构
// ============================================================================

/// State 元信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateMeta {
    pub last_updated: String,
    pub exam_date: String,
    pub target_school: String,
    pub target_major: String,
}

/// 科目集合
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subjects {
    #[serde(default)]
    pub politics: SubjectState,
    #[serde(default)]
    pub english: SubjectState,
    #[serde(default)]
    pub math: SubjectState,
    #[serde(default)]
    pub professional: SubjectState,
}

/// 单个学习任务（State.current_task.tasks 项）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateTask {
    /// 任务 ID（与 PlanTask.id 对应，格式 YYYY-MM-DD-NN）
    /// 旧版 state 文件无此字段，反序列化时为 None，由 update_task_status 顺带补全
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub subject: String,
    pub task: String,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub status: TaskStatus,
    /// 计时开始时间戳（ISO 8601, +0800），仅当任务正在计时时存在
    /// 仅在启用 enable_time_tracking 时使用；旧 state 文件无此字段，反序列化为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// 累计已计时分钟数（不含当前正在进行中的时段）
    /// 仅在启用 enable_time_tracking 时维护；旧 state 文件无此字段，反序列化为 0
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub accumulated_minutes: i64,
}

/// 用于 skip_serializing_if：当值为 0 时不输出该字段，保持旧版 state 文件兼容
fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

/// 当前任务块
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurrentTask {
    pub date: String,
    pub focus: String,
    #[serde(default, deserialize_with = "deserialize_option_f64")]
    pub total_hours: Option<f64>,
    #[serde(default)]
    pub tasks: Vec<StateTask>,
    #[serde(default)]
    pub note: String,
}

/// 风险项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateRisk {
    pub subject: String,
    #[serde(default)]
    pub level: RiskLevel,
    pub description: String,
    pub suggested_action: String,
}

/// 风险集合
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Risks {
    #[serde(default)]
    pub items: Vec<StateRisk>,
}

/// 科目状态
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubjectState {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub phase: StudyPhase,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub textbook: Option<String>,
    #[serde(default)]
    pub textbook_note: Option<String>,
    #[serde(default)]
    pub target_score: i32,
    #[serde(default)]
    pub current_score: i32,
    #[serde(default, deserialize_with = "deserialize_f64")]
    pub weekly_hours: f64,
    #[serde(default)]
    pub weak_chapters: Vec<String>,
    #[serde(default)]
    pub strong_chapters: Vec<String>,
    #[serde(default)]
    pub completed: Vec<String>,
    #[serde(default)]
    pub current_focus: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// 用户学习画像（State 中的 user_model 段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserModel {
    #[serde(default)]
    pub preferred_study_time: String,
    #[serde(default, deserialize_with = "deserialize_f64")]
    pub avg_focus_hours_per_day: f64,
    #[serde(default)]
    pub best_subjects: Vec<String>,
    #[serde(default)]
    pub worst_subjects: Vec<String>,
    #[serde(default)]
    pub learning_style: String,
    #[serde(default)]
    pub common_error_types: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_f64")]
    pub review_compliance_rate: f64,
}

/// 全局进度
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalProgress {
    #[serde(default)]
    pub total_study_days: i32,
    #[serde(default)]
    pub last_study_date: String,
    #[serde(default)]
    pub streak_days: i32,
    #[serde(default)]
    pub total_practice_questions: i32,
    #[serde(default)]
    pub note: String,
}

/// 完整 State — 对应 `state/current.state`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StudyState {
    #[serde(default)]
    pub meta: StateMeta,
    #[serde(default)]
    pub subjects: Subjects,
    #[serde(default)]
    pub current_task: CurrentTask,
    /// 风险项（已废弃，仅为兼容旧 state 文件保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub risks: Risks,
    #[serde(default)]
    pub user_model: UserModel,
    #[serde(default)]
    pub progress: GlobalProgress,
}

// ============================================================================
// 读取/写入函数
// ============================================================================

/// State 文件名
pub const STATE_FILE_NAME: &str = "current.state";
/// State 目录名
pub const STATE_DIR: &str = "state";

/// 读取 state 文件并解析为 `StudyState`
///
/// `data_dir` 是 StudyAgent 的根数据目录
pub fn read_state(data_dir: &Path) -> DataResult<StudyState> {
    let state_path = data_dir.join(STATE_DIR).join(STATE_FILE_NAME);

    if !state_path.exists() {
        return Err(format!("State 文件不存在: {:?}", state_path));
    }

    let content = read_file_content(&state_path)?;
    parse_state_toml(&content)
}

/// 将 TOML 文本解析为 `StudyState`
pub fn parse_state_toml(content: &str) -> DataResult<StudyState> {
    // toml crate 可能无法处理文件头部的 Markdown 注释行（以 # 开头）
    // 需要先清理 TOML 内容中的 Markdown 标题行
    let cleaned = clean_state_toml(content);
    toml::from_str::<StudyState>(&cleaned).map_err(|e| format!("解析 State TOML 失败: {}", e))
}

/// 清理 state 文件中的非 TOML 内容（M16：加固）
///
/// state 文件开头可能残留 Markdown 标题/引用行（旧版写入 `# State — ...` 与
/// `> ...`）。新版 `save_state` 已改为纯 TOML 注释头部，不再产生此类内容。
///
/// 策略：只跳过**文件起始处**连续的空行、`#`、`>` 开头的头部块，一旦遇到
/// 第一个真正的 TOML 行（`[section]` 或以 `key =` 开头的非注释行），其后内容
/// **整体原样保留**（含 TOML 内合法的 `#` 注释与多行字符串），避免旧实现在
/// TOML 内部逐行过滤误删字符串值中的内容。
fn clean_state_toml(content: &str) -> String {
    let mut first_toml: Option<usize> = None;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // 跳过头部块：空行、Markdown 引用行、以 `#` 开头的注释/标题行
        if trimmed.is_empty() || trimmed.starts_with(">") || trimmed.starts_with('#') {
            continue;
        }
        // 第一个真正的 TOML 行起点，其后整体保留
        first_toml = Some(idx);
        break;
    }

    match first_toml {
        Some(idx) => content.lines().skip(idx).collect::<Vec<_>>().join("\n"),
        // 未找到 TOML 行（纯头部），返回空以触发解析失败而非脏数据
        None => String::new(),
    }
}

/// 读取 State，失败时记录警告日志并返回默认值
///
/// 用于非关键路径（如 AI prompt 构建、dashboard 汇总），
/// 这些路径在 State 缺失或损坏时应降级而非中断整个流程。
/// 关键路径（如 update_task_status）应直接调用 `read_state` 并处理错误。
pub fn read_state_or_default(data_dir: &Path) -> StudyState {
    match read_state(data_dir) {
        Ok(state) => state,
        Err(e) => {
            log::warn!("读取 State 失败，使用默认值: {}", e);
            StudyState::default()
        }
    }
}

/// 将 `StudyState` 序列化为 TOML 文本
pub fn write_state_toml(state: &StudyState) -> DataResult<String> {
    toml::to_string_pretty(state).map_err(|e| format!("序列化 State TOML 失败: {}", e))
}

/// 将 `StudyState` 写入 state 文件
///
/// H14：写入前先将旧文件备份为 `current.state.bak`，防止损坏后无法恢复。
pub fn save_state(data_dir: &Path, state: &StudyState) -> DataResult<()> {
    let state_path = data_dir.join(STATE_DIR).join(STATE_FILE_NAME);
    let toml_content = write_state_toml(state)?;

    // 生成文件头（M16：全部用 TOML 注释，不含 Markdown 引号行，确保纯 TOML 可解析）
    let header = "# State — Agent 对用户当前学习状态的理解\n# State 保存的是分析后的认知，不是原始事实。\n\n";

    let full_content = format!("{}{}", header, toml_content);

    // 备份旧文件（若存在）到 .bak，供损坏时恢复
    if state_path.exists() {
        if let Err(e) = std::fs::copy(&state_path, state_path.with_extension("state.bak")) {
            log::warn!("备份 State 文件失败: {}", e);
        }
    }

    super::atomic_write(&state_path, &full_content)
        .map_err(|e| format!("写入 State 文件失败 {:?}: {}", state_path, e))
}

/// 更新指定任务的状态（按 task_id 精确匹配，回退到索引）
///
/// 优先按 task_id 匹配；若未匹配到（旧版 state 文件无 task_id），
/// 回退到按索引匹配，并顺带补全 task_id 字段。
pub fn update_task_status_by_id(
    state: &mut StudyState,
    task_id: &str,
    task_index: usize,
    new_status: TaskStatus,
) -> DataResult<()> {
    // 第一轮：按 task_id 精确匹配
    for task in &mut state.current_task.tasks {
        if task.task_id.as_deref() == Some(task_id) {
            task.status = new_status.clone();
            return Ok(());
        }
    }

    // 第二轮：回退到按索引匹配，并补全 task_id
    if task_index < state.current_task.tasks.len() {
        let task = &mut state.current_task.tasks[task_index];
        if task.task_id.is_none() {
            task.task_id = Some(task_id.to_string());
        }
        task.status = new_status;
        return Ok(());
    }

    Err(format!(
        "未找到任务: task_id={}, index={}（共 {} 个任务）",
        task_id,
        task_index,
        state.current_task.tasks.len()
    ))
}

/// 根据科目名获取科目状态的可变引用
pub fn get_subject_state_mut<'a>(
    state: &'a mut StudyState,
    subject: &str,
) -> Option<&'a mut SubjectState> {
    match subject {
        "math" => Some(&mut state.subjects.math),
        "english" => Some(&mut state.subjects.english),
        "politics" => Some(&mut state.subjects.politics),
        "professional" => Some(&mut state.subjects.professional),
        _ => None,
    }
}

// ============================================================================
// 任务计时相关函数
// ============================================================================

/// 开始任务计时：为指定 task_id 的任务设置 started_at（当前时间）
///
/// 若该任务已有 started_at（正在计时中），直接返回 Ok（幂等）。
/// 若其他任务正在计时，不会自动暂停它们（由前端调用 pause 控制单任务计时）。
pub fn start_task_timer(state: &mut StudyState, task_id: &str) -> DataResult<()> {
    let task =
        find_task_by_id_mut(state, task_id).ok_or_else(|| format!("未找到任务: {}", task_id))?;
    if task.started_at.is_none() {
        task.started_at = Some(super::now_string());
    }
    Ok(())
}

/// 暂停任务计时：计算 started_at 到现在的分钟差，累加到 accumulated_minutes，清空 started_at
///
/// 若任务未在计时中（started_at 为 None），直接返回 Ok（幂等）。
pub fn pause_task_timer(state: &mut StudyState, task_id: &str) -> DataResult<i64> {
    let task =
        find_task_by_id_mut(state, task_id).ok_or_else(|| format!("未找到任务: {}", task_id))?;
    if let Some(started_at) = task.started_at.take() {
        let minutes = elapsed_minutes(&started_at)?;
        task.accumulated_minutes += minutes;
        Ok(minutes)
    } else {
        Ok(0)
    }
}

/// 直接累加指定任务的累计专注分钟数（供番茄钟等显式计时使用）
///
/// 与 `pause_task_timer` 不同：番茄钟以整段会话为单位，学习会话结束时把时长
/// 一次性累加，不依赖 started_at 的起止差值。若任务不存在则返回错误。
pub fn add_accumulated_minutes(
    state: &mut StudyState,
    task_id: &str,
    minutes: i64,
) -> DataResult<()> {
    if minutes <= 0 {
        return Ok(());
    }
    let task =
        find_task_by_id_mut(state, task_id).ok_or_else(|| format!("未找到任务: {}", task_id))?;
    task.accumulated_minutes += minutes;
    Ok(())
}

/// 获取任务当前累计专注分钟数（含正在进行的时段）
///
/// 若任务正在计时中（started_at 存在），将 started_at 至今的分钟数累加到 accumulated_minutes 返回。
pub fn task_total_minutes(state: &StudyState, task_id: &str) -> DataResult<i64> {
    let task = find_task_by_id(state, task_id).ok_or_else(|| format!("未找到任务: {}", task_id))?;
    let mut total = task.accumulated_minutes;
    if let Some(started_at) = &task.started_at {
        total += elapsed_minutes(started_at)?;
    }
    Ok(total)
}

/// 某日任务的实际累计学习分钟（含正在进行的时段）
///
/// 按 task_id 的日期前缀（前 10 位 YYYY-MM-DD）筛选当天任务，汇总其累计计时。
/// 任务计时不区分是否完成，未勾选完成的任务也会计入实际学习时长。
/// 无当天任务或计时时返回 0。
pub fn day_actual_minutes(state: &StudyState, date: &str) -> i64 {
    state
        .current_task
        .tasks
        .iter()
        .filter_map(|st| {
            let id = st.task_id.as_deref()?;
            if id.get(..10) != Some(date) {
                return None;
            }
            task_total_minutes(state, id).ok()
        })
        .sum()
}

/// 按 task_id 查找任务（不可变引用）
fn find_task_by_id<'a>(state: &'a StudyState, task_id: &str) -> Option<&'a StateTask> {
    state
        .current_task
        .tasks
        .iter()
        .find(|t| t.task_id.as_deref() == Some(task_id))
}

/// 按 task_id 查找任务（可变引用）
fn find_task_by_id_mut<'a>(state: &'a mut StudyState, task_id: &str) -> Option<&'a mut StateTask> {
    state
        .current_task
        .tasks
        .iter_mut()
        .find(|t| t.task_id.as_deref() == Some(task_id))
}

/// 计算 ISO 时间戳距今的分钟数（整数，向下取整）
///
/// 输入应为 +0800 时区的 ISO 字符串，如 "2026-07-30T14:30:00+08:00"。
fn elapsed_minutes(started_at: &str) -> DataResult<i64> {
    // 简化实现：解析两个 ISO 时间戳并计算差值
    // 这里复用 data 层的时间解析（如果存在 chrono 会更简洁，但项目暂未引入）
    let now = super::now_string();
    minutes_between_iso(&started_at, &now)
}

/// 计算两个 ISO 8601 时间戳之间的分钟差（end - start）
///
/// 支持格式："YYYY-MM-DDTHH:mm:ss+08:00" 或 "YYYY-MM-DDTHH:mm"
fn minutes_between_iso(start: &str, end: &str) -> DataResult<i64> {
    let start_min = iso_to_minutes(start)?;
    let end_min = iso_to_minutes(end)?;
    Ok(end_min - start_min)
}

/// 将 ISO 8601 时间戳转换为相对于 Unix 纪元的总分钟数
///
/// C7：改用 chrono 替换手写的日/月/闰年/时区解析，消除与项目已有 chrono 依赖的重复。
/// 兼容两种输入格式（旧数据/ runtime 格式）：
/// - "YYYY-MM-DDTHH:mm:ss+08:00"（含秒、带时区）
/// - "YYYY-MM-DDTHH:mm"（now_string 产生的无秒、无时区，视为 UTC+8）
fn iso_to_minutes(iso: &str) -> DataResult<i64> {
    let normalized = normalize_iso(iso);
    let dt = chrono::DateTime::parse_from_rfc3339(&normalized)
        .map_err(|e| format!("解析 ISO 时间失败: {} (输入: {})", e, iso))?;
    Ok(dt.timestamp() / 60)
}

/// 把两种 ISO 变体归一化为 chrono 可解析的 RFC3339：
/// 无秒补 ":00"，无时区补 "+08:00"（与 `now_string` 的 UTC+8 约定一致）。
///
/// 实际输入由 `now_string` 稳定产生 "YYYY-MM-DDTHH:mm"，这里同时兼容可能带秒/时区的旧数据。
fn normalize_iso(iso: &str) -> String {
    let mut s = iso.trim().to_string();
    // 已带时区（时间部分之后出现 + / - / Z）
    let has_tz = s.find('T').is_some_and(|t| {
        let rest = &s[t + 1..];
        rest.contains('+') || rest.contains('-') || rest.contains('Z')
    });
    if !has_tz {
        // 无时区：统一补 UTC+8（首个冒号之后没有第二个冒号 → 缺秒）
        if s.find('T').is_some_and(|t| {
            let time = &s[t + 1..];
            match time.find(':') {
                Some(c) => !time[c + 1..].contains(':'),
                None => true,
            }
        }) {
            s.push_str(":00+08:00");
        } else {
            s.push_str("+08:00");
        }
    } else {
        // 含时区但缺秒（HH:mm+08:00）：在时区标记前插入 :00
        let t_pos = s.find('T').unwrap_or(0);
        let time = &s[t_pos + 1..];
        let end_of_time = time.find(['+', '-', 'Z']).unwrap_or(time.len());
        let time_part = &time[..end_of_time];
        if !time_part[time_part.find(':').map_or(0, |c| c + 1)..].contains(':') {
            s = format!(
                "{}{}:00{}",
                &s[..t_pos + 1],
                time_part,
                &time[end_of_time..]
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_to_minutes_parses_both_variants_with_equal_elapsed() {
        // C7：chrono 替换后，两种格式（含秒带时区 / 无秒无时区）都应能解析，
        // 且同为 UTC+8 时刻时差值一致。
        let with_tz = "2026-07-30T14:30:00+08:00";
        let without_tz = "2026-07-30T14:30";
        let a = iso_to_minutes(with_tz).expect("带时区格式应可解析");
        let b = iso_to_minutes(without_tz).expect("无时区格式应按 UTC+8 解析");
        assert_eq!(a, b, "两种表示应指向同一时刻");
    }

    #[test]
    fn iso_to_minutes_reflects_timezone_offset() {
        // TZ 偏移应被计入绝对时刻：同一本地钟面 +00:00 比 +08:00 早 8 小时（480 分钟）
        let utc_str = "2026-07-30T14:30:00+00:00";
        let cn_str = "2026-07-30T14:30:00+08:00";
        let utc = iso_to_minutes(utc_str).expect("+00:00 可解析");
        let cn = iso_to_minutes(cn_str).expect("+08:00 可解析");
        assert_eq!(utc - cn, 8 * 60, "时区差 8 小时应差 480 分钟");
    }

    #[test]
    fn elapsed_minutes_uses_utc8_consistently() {
        // 无时区变体（now_string 产出）与显式 +08:00 表示同一 UTC+8 时刻，
        // 二者之差应为 0
        let start = "2026-07-30T14:30";
        let end = "2026-07-30T14:30:00+08:00";
        let diff = minutes_between_iso(start, end).expect("应可解析并求差");
        assert_eq!(diff, 0);
    }
}
