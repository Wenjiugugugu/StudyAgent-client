//! 调整在临时副本中生成；正式数据只在确认时写入。日志保留一次可撤销事务。
use crate::core::planner::Planner;
use crate::data::{self, DailyPlanFile, DataResult, ExcludedDay, PlanTask, TaskStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const INPUT_DIRS: &[&str] = &[
    "plan",
    "state",
    "records",
    "config",
    "assets",
    "progress_tables",
    "adaptive",
];
const DRAFT: &str = "plan/adjustments/preview.json";
const JOURNAL: &str = "plan/adjustments/latest.json";
const TRANSACTION: &str = "plan/adjustments/transaction.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChoice {
    pub key: String,
    pub date: String,
    pub task: PlanTask,
    pub default_kept: bool,
    pub required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChange {
    pub subject: String,
    pub title: String,
    pub before_date: Option<String>,
    pub after_date: Option<String>,
    pub before_hours: Option<f64>,
    pub after_hours: Option<f64>,
    pub reason: String,
    pub before_content: Option<String>,
    pub after_content: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayChange {
    pub date: String,
    pub before_hours: f64,
    pub after_hours: f64,
    pub before_rest: bool,
    pub after_rest: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalChange {
    pub title: String,
    pub before_deadline: String,
    pub after_deadline: String,
    pub before_start: String,
    pub after_start: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustmentPreview {
    pub id: String,
    pub created_at: String,
    pub kind: String,
    pub source_date: String,
    pub excluded_day: Option<ExcludedDay>,
    pub affected_dates: Vec<String>,
    pub reason: String,
    pub used_fallback: bool,
    pub warnings: Vec<String>,
    pub conflicts: Vec<String>,
    pub tasks: Vec<TaskChoice>,
    pub kept_keys: Vec<String>,
    pub changes: Vec<TaskChange>,
    pub days: Vec<DayChange>,
    pub goals: Vec<GoalChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileChange {
    path: String,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Draft {
    preview: AdjustmentPreview,
    today: String,
    fingerprint: String,
    files: Vec<FileChange>,
    generated_days: BTreeMap<String, DailyPlanFile>,
    sequence_base: BTreeMap<String, u32>,
    generated_week: data::WeekPlanFile,
    baseline_warnings: Vec<String>,
    daily_budget: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    id: String,
    created_at: String,
    status: String,
    dates: Vec<String>,
    files: Vec<FileChange>,
    preview: AdjustmentPreview,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    sync_pending: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct AdjustmentStatus {
    pub preview: Option<AdjustmentPreview>,
    pub undo_id: Option<String>,
    pub undo_created_at: Option<String>,
    pub applied_preview: Option<AdjustmentPreview>,
    pub warnings: Vec<String>,
    pub sync_pending: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct AdjustmentOutcome {
    pub affected_dates: Vec<String>,
    pub warnings: Vec<String>,
}

fn read_optional(path: &Path) -> DataResult<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取调整数据失败：{e}")),
    }
}
fn load<T: for<'a> Deserialize<'a>>(dir: &Path, path: &str) -> DataResult<Option<T>> {
    read_optional(&dir.join(path))?
        .map(|s| serde_json::from_str(&s).map_err(|e| format!("读取调整记录失败：{e}")))
        .transpose()
}
fn save<T: Serialize>(dir: &Path, path: &str, value: &T) -> DataResult<()> {
    data::atomic_write(
        &dir.join(path),
        &serde_json::to_string_pretty(value).map_err(|e| e.to_string())?,
    )
}

/// 只复制规划所需的数据，不跟随符号链接，也不复制调整记录、诊断输出或同步缓存。
fn visit(dir: &Path, relative: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> DataResult<()> {
    let path = dir.join(relative);
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("规划数据包含链接目录，无法安全生成预览".into());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let next = relative.join(entry.file_name());
            if next == Path::new("plan/adjustments") {
                continue;
            }
            visit(dir, &next, files)?;
        }
    } else if metadata.is_file()
        && path
            .extension()
            .is_some_and(|e| e == "json" || e == "state" || e == "md" || e == "toml")
    {
        files.insert(
            relative.to_string_lossy().replace('\\', "/"),
            std::fs::read(&path).map_err(|e| e.to_string())?,
        );
    }
    Ok(())
}
fn inputs(dir: &Path) -> DataResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for name in INPUT_DIRS {
        visit(dir, Path::new(name), &mut files)?;
    }
    Ok(files)
}
fn fingerprint(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in files {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
struct Workspace(PathBuf);
impl Workspace {
    fn new(files: &BTreeMap<String, Vec<u8>>) -> DataResult<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "studyagent-preview-{}-{}-{id}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir(&path).map_err(|e| e.to_string())?;
        let workspace = Self(path);
        for (name, bytes) in files {
            let target = workspace.0.join(name);
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
        // 旧配置中的明文密钥不能在预览期间触发系统密钥迁移；规划只需要学习设置。
        let config = workspace.0.join("config/settings.json");
        if let Some(raw) = read_optional(&config)? {
            let mut value: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            value["data_dir"] = serde_json::Value::String(workspace.0.to_string_lossy().into());
            value["ai_providers"] = serde_json::json!([]);
            data::atomic_write(
                &config,
                &serde_json::to_string(&value).map_err(|e| e.to_string())?,
            )?;
        }
        Ok(workspace)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn same_task(a: &PlanTask, b: &PlanTask) -> bool {
    a.subject == b.subject && a.title == b.title && a.textbook == b.textbook && a.source == b.source
}
fn live_plan(dir: &Path, date: &str) -> DataResult<Option<DailyPlanFile>> {
    if !data::daily_plan_path(dir, date).exists() {
        return Ok(None);
    }
    let mut plan = data::read_daily_plan(dir, date)?;
    let state = data::read_state_or_default(dir);
    if state.current_task.date == date {
        for task in &mut plan.data.tasks {
            if let Some(live) = state.current_task.tasks.iter().find(|t| {
                (t.task_id.as_deref() == Some(&task.id) || t.task_id.is_none())
                    && t.task == task.title
                    && t.subject == task.subject.key()
            }) {
                task.status = live.status.clone();
            }
        }
    }
    Ok(Some(plan))
}
fn task_choices(dir: &Path, dates: &[String]) -> DataResult<Vec<TaskChoice>> {
    let mut tasks = Vec::new();
    for date in dates {
        if let Some(plan) = live_plan(dir, date)? {
            for task in plan.data.tasks {
                let required = matches!(task.status, TaskStatus::Done | TaskStatus::InProgress)
                    || task.is_manual();
                tasks.push(TaskChoice {
                    key: format!("{date}/{}", task.id),
                    date: date.clone(),
                    task,
                    default_kept: required,
                    required,
                });
            }
        }
    }
    Ok(tasks)
}

pub async fn generate(
    dir: &Path,
    planner: &Planner<'_>,
    kind: &str,
    date: &str,
    excluded: Option<ExcludedDay>,
    io_lock: Option<&std::sync::Arc<tokio::sync::Mutex<()>>>,
) -> DataResult<AdjustmentPreview> {
    let capture_guard = match io_lock {
        Some(lock) => Some(lock.lock().await),
        None => None,
    };
    recover(dir)?;
    let input = inputs(dir)?;
    let baseline = fingerprint(&input);
    let workspace = Workspace::new(&input)?;
    drop(capture_guard);
    let excluded_day = excluded.clone();
    let allocation_changed =
        crate::core::planner::allocation_changed_since_week_plan(&workspace.0, date);
    let (mut dates, fallback, warnings, reason) = if kind == "review" {
        let (_, dates, fallback, warnings, _) = planner
            .regenerate_remaining_days_after_review(&workspace.0, date)
            .await?;
        let review = data::read_review(&workspace.0, date)?;
        let mut reasons = Vec::new();
        if review
            .task_reviews
            .iter()
            .any(|t| t.status == "incomplete" || t.status == "partial")
        {
            reasons.push("复盘中有尚未完成的任务");
        }
        if !review.overcompletion.is_empty() {
            reasons.push("计划外学习改变了后续起点");
        }
        if allocation_changed {
            reasons.push("学科时间占比发生变化");
        }
        if reasons.is_empty() {
            reasons.push("依据本次复盘反馈调整后续学习量");
        }
        (dates, fallback, warnings, reasons.join("；"))
    } else {
        let excluded = excluded.ok_or("缺少排除日期")?;
        let reason = format!(
            "{} 设为排除日，将该日任务重新安排到剩余学习日{}",
            excluded.date,
            excluded
                .note
                .as_ref()
                .map(|s| format!("：{s}"))
                .unwrap_or_default()
        );
        let (_, dates, fallback) = planner
            .regenerate_after_exclusion(&workspace.0, date, excluded)
            .await?;
        (dates, fallback, Vec::new(), reason)
    };
    dates.retain(|d| d >= &data::today_string());
    let _publish_guard = match io_lock {
        Some(lock) => Some(lock.lock().await),
        None => None,
    };
    if fingerprint(&inputs(dir)?) != baseline {
        return Err("生成预览期间学习数据发生变化，请重新生成".into());
    }
    let week = data::iso_week_string(date)?;
    let mut generated_week = data::read_week_plan(&workspace.0, &week)?;
    let original_week = data::read_week_plan(dir, &week)?;
    for day in &mut generated_week.data.days {
        if day.date < data::today_string() {
            if let Some(old) = original_week.data.days.iter().find(|d| d.date == day.date) {
                *day = old.clone();
            }
        }
    }
    data::save_week_plan(&workspace.0, &generated_week)?;
    let mut paths = vec![format!("plan/{week}_week.json"), "plan/goals.json".into()];
    paths.extend(dates.iter().map(|d| format!("plan/{d}_day.json")));
    let mut files = Vec::new();
    for path in paths {
        files.push(FileChange {
            before: read_optional(&dir.join(&path))?,
            after: read_optional(&workspace.0.join(&path))?,
            path,
        });
    }
    let tasks = task_choices(dir, &dates)?;
    let mut sequence_base = BTreeMap::new();
    for task in &tasks {
        let seq = task
            .task
            .id
            .get(11..)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let max = sequence_base.entry(task.date.clone()).or_insert(0u32);
        *max = (*max).max(seq);
    }
    // 撤销过的编号也不复用，避免历史计时误关联到新任务。
    if let Some(journal) = load::<Journal>(dir, JOURNAL)? {
        for file in journal
            .files
            .iter()
            .filter(|f| f.path.ends_with("_day.json"))
        {
            for raw in [&file.before, &file.after].into_iter().flatten() {
                let plan: DailyPlanFile = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                for task in plan.data.tasks {
                    let seq = task
                        .id
                        .get(11..)
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    let max = sequence_base.entry(plan.meta.date.clone()).or_insert(0u32);
                    *max = (*max).max(seq);
                }
            }
        }
    }
    let kept_keys = tasks
        .iter()
        .filter(|t| t.default_kept)
        .map(|t| t.key.clone())
        .collect();
    let mut generated_days = BTreeMap::new();
    for date in &dates {
        let plan = match data::read_daily_plan(&workspace.0, date) {
            Ok(plan) => plan,
            Err(e) => {
                let week_plan = data::read_week_plan(&workspace.0, &week)?;
                if !week_plan
                    .data
                    .days
                    .iter()
                    .any(|d| d.date == *date && d.is_rest_day)
                {
                    return Err(format!("{date} 的候选计划未完整生成，请重试：{e}"));
                }
                let mut empty = DailyPlanFile::default();
                empty.version = "1.0.0".into();
                empty.meta.date = date.clone();
                empty.meta.r#type = "daily_plan".into();
                empty.meta.generated_at = data::now_string();
                empty
            }
        };
        generated_days.insert(date.clone(), plan);
    }
    let id = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let baseline_warnings = warnings.clone();
    let daily_budget = crate::load_settings(&workspace.0).daily_target_hours();
    let mut draft = Draft {
        preview: AdjustmentPreview {
            id,
            created_at: data::now_string(),
            kind: kind.into(),
            source_date: date.into(),
            excluded_day,
            affected_dates: dates,
            reason,
            used_fallback: fallback,
            warnings,
            conflicts: Vec::new(),
            tasks,
            kept_keys,
            changes: Vec::new(),
            days: Vec::new(),
            goals: Vec::new(),
        },
        today: data::today_string(),
        fingerprint: baseline,
        files,
        generated_days,
        sequence_base,
        generated_week,
        baseline_warnings,
        daily_budget,
    };
    project(&mut draft)?;
    if fingerprint(&inputs(dir)?) != draft.fingerprint {
        return Err("生成预览期间学习数据发生变化，请重新生成".into());
    }
    save(dir, DRAFT, &draft)?;
    Ok(draft.preview)
}

fn original_days(draft: &Draft) -> DataResult<BTreeMap<String, DailyPlanFile>> {
    let mut days = BTreeMap::new();
    for file in &draft.files {
        if file.path.ends_with("_day.json") {
            if let Some(raw) = &file.before {
                let plan: DailyPlanFile = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                days.insert(plan.meta.date.clone(), plan);
            }
        }
    }
    // 从 state 读取的最新状态只存在于 choices 中，不写入真实数据。
    for choice in &draft.preview.tasks {
        if let Some(day) = days.get_mut(&choice.date) {
            if let Some(task) = day.data.tasks.iter_mut().find(|t| t.id == choice.task.id) {
                *task = choice.task.clone();
            }
        }
    }
    Ok(days)
}

fn project(draft: &mut Draft) -> DataResult<()> {
    let before = original_days(draft)?;
    let mut after = draft.generated_days.clone();
    draft.preview.conflicts.clear();
    draft.preview.warnings = draft.baseline_warnings.clone();
    let week_file = draft
        .files
        .iter()
        .find(|f| f.path.ends_with("_week.json"))
        .ok_or("缺少周计划")?;
    let mut week = draft.generated_week.clone();
    let kept: Vec<_> = draft
        .preview
        .tasks
        .iter()
        .filter(|t| t.required || draft.preview.kept_keys.contains(&t.key))
        .cloned()
        .collect();
    draft.preview.kept_keys = kept.iter().map(|t| t.key.clone()).collect();
    for choice in &kept {
        if week
            .data
            .days
            .iter()
            .any(|d| d.date == choice.date && d.is_rest_day)
        {
            draft.preview.conflicts.push(format!("{}「{}」需要保留，但该日为休息日。请选择其他排除日期，或先在日计划中处理这个任务。", choice.date, choice.task.title));
        }
        // 优先移除同日候选；跨日只移除多出来的一份，保留每日重复的复习任务。
        let same_date = after
            .get(&choice.date)
            .and_then(|d| d.data.tasks.iter().position(|t| same_task(t, &choice.task)));
        if let Some(index) = same_date {
            after
                .get_mut(&choice.date)
                .unwrap()
                .data
                .tasks
                .remove(index);
        } else {
            let moved_date = after
                .iter()
                .find(|(date, d)| {
                    **date != choice.date
                        && d.data
                            .tasks
                            .iter()
                            .filter(|t| same_task(t, &choice.task))
                            .count()
                            > before
                                .get(*date)
                                .map(|p| {
                                    p.data
                                        .tasks
                                        .iter()
                                        .filter(|t| same_task(t, &choice.task))
                                        .count()
                                })
                                .unwrap_or(0)
                })
                .map(|(date, _)| date.clone());
            if let Some(date) = moved_date {
                let day = after.get_mut(&date).unwrap();
                if let Some(index) = day
                    .data
                    .tasks
                    .iter()
                    .position(|t| same_task(t, &choice.task))
                {
                    day.data.tasks.remove(index);
                }
            }
        }
    }
    for choice in &kept {
        if let Some(day) = after.get_mut(&choice.date) {
            day.data.tasks.push(choice.task.clone());
        }
    }
    let old_week: data::WeekPlanFile = week_file
        .before
        .as_ref()
        .map(|s| serde_json::from_str(s))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let mut kept_templates = Vec::new();
    for choice in &kept {
        if choice.task.is_manual() {
            continue;
        }
        let original = old_week
            .data
            .days
            .iter()
            .find(|d| d.date == choice.date)
            .and_then(|d| {
                d.subject_allocations
                    .iter()
                    .find(|a| a.subject == choice.task.subject)
            })
            .and_then(|a| {
                a.task_templates
                    .iter()
                    .find(|t| t.title == choice.task.title)
            })
            .cloned();
        let Some(template) = original else {
            continue;
        };
        kept_templates.push((
            choice.clone(),
            data::TaskTemplate {
                title: choice.task.title.clone(),
                priority: choice.task.priority.clone(),
                estimated_hours: choice.task.estimated_hours,
                goal: choice.task.goal.clone(),
                completion_criteria: choice.task.completion_criteria.clone(),
                textbook: choice.task.textbook.clone(),
                style_tips: choice.task.style_tips.clone(),
                fallback_plan: choice.task.fallback_plan.clone(),
            },
        ));
        let _ = template;
        for day in &mut week.data.days {
            if !draft.preview.affected_dates.contains(&day.date) {
                continue;
            }
            for allocation in &mut day.subject_allocations {
                if allocation.subject == choice.task.subject {
                    let previous_count = old_week
                        .data
                        .days
                        .iter()
                        .find(|d| d.date == day.date)
                        .and_then(|d| {
                            d.subject_allocations
                                .iter()
                                .find(|a| a.subject == choice.task.subject)
                        })
                        .map(|a| {
                            a.task_templates
                                .iter()
                                .filter(|t| t.title == choice.task.title)
                                .count()
                        })
                        .unwrap_or(0);
                    let current_count = allocation
                        .task_templates
                        .iter()
                        .filter(|t| t.title == choice.task.title)
                        .count();
                    if day.date == choice.date || current_count > previous_count {
                        if let Some(index) = allocation
                            .task_templates
                            .iter()
                            .position(|t| t.title == choice.task.title)
                        {
                            allocation.task_templates.remove(index);
                        }
                    }
                    allocation.hours = allocation
                        .task_templates
                        .iter()
                        .map(|t| t.estimated_hours)
                        .sum();
                }
            }
        }
    }
    for (choice, template) in kept_templates {
        if let Some(day) = week
            .data
            .days
            .iter_mut()
            .find(|d| d.date == choice.date && !d.is_rest_day)
        {
            if !day
                .subject_allocations
                .iter()
                .any(|a| a.subject == choice.task.subject)
            {
                day.subject_allocations.push(data::DaySubjectAllocation {
                    subject: choice.task.subject.clone(),
                    ..Default::default()
                });
            }
            let allocation = day
                .subject_allocations
                .iter_mut()
                .find(|a| a.subject == choice.task.subject)
                .unwrap();
            allocation.task_templates.push(template);
            allocation.hours = allocation
                .task_templates
                .iter()
                .map(|t| t.estimated_hours)
                .sum();
        }
    }
    // 相同内容保留编号；新任务使用新编号，避免把旧计时或完成状态挂到另一任务。
    for (date, day) in &mut after {
        let original = before.get(date);
        let mut used = HashSet::new();
        for (index, task) in day.data.tasks.iter_mut().enumerate() {
            if let Some(old) = original.and_then(|d| {
                d.data
                    .tasks
                    .iter()
                    .find(|t| same_task(t, task) && !used.contains(&t.id))
            }) {
                task.id = old.id.clone();
                task.status = old.status.clone();
                task.dida_task_id = old.dida_task_id.clone();
            } else {
                let seq = draft
                    .sequence_base
                    .get(date)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(index as u32 + 1)
                    .ok_or("任务编号超出范围")?;
                task.id = format!("{date}-{seq:02}");
                task.dida_task_id = None;
            }
            used.insert(task.id.clone());
        }
        data::recalc_totals(&mut day.data);
        draft.preview.warnings.extend(day.data.warnings.clone());
        if draft.daily_budget > 0.0 && day.data.total_hours > draft.daily_budget + 0.01 {
            draft.preview.warnings.push(format!(
                "{date} 的候选安排为 {} 分钟，超过每日设置的 {} 分钟。请检查保留任务和当天学习量。",
                (day.data.total_hours * 60.0).round(),
                (draft.daily_budget * 60.0).round()
            ));
        }
    }
    draft.preview.changes = task_changes(&before, &after, &draft.preview.reason);
    draft.preview.days = draft
        .preview
        .affected_dates
        .iter()
        .map(|date| DayChange {
            date: date.clone(),
            before_hours: before.get(date).map(|p| p.data.total_hours).unwrap_or(0.0),
            after_hours: after.get(date).map(|p| p.data.total_hours).unwrap_or(0.0),
            before_rest: week_file
                .before
                .as_ref()
                .and_then(|s| serde_json::from_str::<data::WeekPlanFile>(s).ok())
                .is_some_and(|p| p.data.days.iter().any(|d| d.date == *date && d.is_rest_day)),
            after_rest: week
                .data
                .days
                .iter()
                .any(|d| d.date == *date && d.is_rest_day),
        })
        .collect();
    for file in &mut draft.files {
        if file.path.ends_with("_week.json") {
            file.after = Some(serde_json::to_string_pretty(&week).map_err(|e| e.to_string())?);
        }
        if file.path.ends_with("_day.json") {
            if let Some(plan) = after
                .values()
                .find(|p| file.path == format!("plan/{}_day.json", p.meta.date))
            {
                file.after = Some(serde_json::to_string_pretty(plan).map_err(|e| e.to_string())?);
            }
        }
    }
    draft.preview.goals.clear();
    if let Some(file) = draft.files.iter().find(|f| f.path == "plan/goals.json") {
        let old: data::GoalPlanFile = file
            .before
            .as_ref()
            .map(|s| serde_json::from_str(s))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let new: data::GoalPlanFile = file
            .after
            .as_ref()
            .map(|s| serde_json::from_str(s))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        for goal in new.data.goals {
            if let Some(prev) = old.data.goals.iter().find(|g| g.id == goal.id) {
                if prev.deadline != goal.deadline || prev.planning_start != goal.planning_start {
                    draft.preview.goals.push(GoalChange {
                        title: goal.title,
                        before_deadline: prev.deadline.clone(),
                        after_deadline: goal.deadline,
                        before_start: prev.planning_start.clone(),
                        after_start: goal.planning_start,
                    });
                }
            }
        }
    }
    Ok(())
}

fn task_changes(
    before: &BTreeMap<String, DailyPlanFile>,
    after: &BTreeMap<String, DailyPlanFile>,
    reason: &str,
) -> Vec<TaskChange> {
    let old: Vec<_> = before
        .iter()
        .flat_map(|(date, p)| p.data.tasks.iter().map(move |t| (date, t)))
        .collect();
    let new: Vec<_> = after
        .iter()
        .flat_map(|(date, p)| p.data.tasks.iter().map(move |t| (date, t)))
        .collect();
    let mut used = HashSet::new();
    let mut changes = Vec::new();
    for (date, task) in old {
        let matched = new
            .iter()
            .enumerate()
            .find(|(i, (d, t))| !used.contains(i) && *d == date && same_task(task, t))
            .or_else(|| {
                new.iter()
                    .enumerate()
                    .find(|(i, (_, t))| !used.contains(i) && same_task(task, t))
            });
        let next = matched.map(|(i, v)| {
            used.insert(i);
            *v
        });
        if next.is_some_and(|(d, t)| {
            d == date
                && (t.estimated_hours - task.estimated_hours).abs() < 0.001
                && task_content(t) == task_content(task)
        }) {
            continue;
        }
        changes.push(TaskChange {
            subject: task.subject.label().into(),
            title: task.title.clone(),
            before_date: Some(date.clone()),
            after_date: next.map(|(d, _)| d.clone()),
            before_hours: Some(task.estimated_hours),
            after_hours: next.map(|(_, t)| t.estimated_hours),
            reason: reason.into(),
            before_content: Some(task_content(task)),
            after_content: next.map(|(_, t)| task_content(t)),
        });
    }
    for (i, (date, task)) in new.into_iter().enumerate() {
        if !used.contains(&i) {
            changes.push(TaskChange {
                subject: task.subject.label().into(),
                title: task.title.clone(),
                before_date: None,
                after_date: Some(date.clone()),
                before_hours: None,
                after_hours: Some(task.estimated_hours),
                reason: reason.into(),
                before_content: None,
                after_content: Some(task_content(task)),
            });
        }
    }
    changes
}
fn task_content(task: &PlanTask) -> String {
    let mut parts = Vec::new();
    if !task.goal.is_empty() {
        parts.push(task.goal.clone());
    }
    parts.extend(task.completion_criteria.clone());
    if let Some(book) = &task.textbook {
        parts.push(book.clone());
    }
    parts.join("；")
}

fn get_draft(dir: &Path, id: &str) -> DataResult<Draft> {
    let draft: Draft = load(dir, DRAFT)?.ok_or("预览已失效，请重新生成")?;
    if draft.preview.id != id {
        return Err("预览已被新的调整替换，请重新打开".into());
    }
    Ok(draft)
}
pub fn update_kept(dir: &Path, id: &str, keys: Vec<String>) -> DataResult<AdjustmentPreview> {
    let mut draft = get_draft(dir, id)?;
    if keys
        .iter()
        .any(|k| !draft.preview.tasks.iter().any(|t| &t.key == k))
    {
        return Err("保留列表包含不属于本次预览的任务".into());
    }
    draft.preview.kept_keys = keys;
    project(&mut draft)?;
    save(dir, DRAFT, &draft)?;
    Ok(draft.preview)
}
pub fn discard(dir: &Path, id: &str) -> DataResult<()> {
    get_draft(dir, id)?;
    remove(&dir.join(DRAFT))
}
fn remove(path: &Path) -> DataResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn write_files(dir: &Path, files: &[FileChange], forward: bool) -> DataResult<()> {
    for file in files {
        // 持久化记录也必须校验路径，禁止路径逃逸。
        let valid = file
            .path
            .strip_prefix("plan/")
            .is_some_and(|name| !name.contains('/') && !name.contains(':'))
            && !file.path.contains("..")
            && !file.path.contains('\\')
            && file.path.ends_with(".json");
        if !valid {
            return Err("调整记录包含无效文件路径".into());
        }
    }
    for file in files {
        let value = if forward { &file.after } else { &file.before };
        match value {
            Some(raw) => data::atomic_write(&dir.join(&file.path), raw)?,
            None => remove(&dir.join(&file.path))?,
        }
    }
    Ok(())
}
/// 未完成的多文件提交在下次访问时回滚到提交前，避免崩溃留下半套计划。
pub fn recover(dir: &Path) -> DataResult<()> {
    if let Some(transaction) = load::<Journal>(dir, TRANSACTION)? {
        let latest: Option<Journal> = load(dir, JOURNAL)?;
        let committed = latest.is_some_and(|j| {
            j.id == transaction.id
                && ((transaction.status == "prepared" && j.status == "applied")
                    || (transaction.status == "undo_prepared" && j.status == "undone"))
        });
        if !committed {
            write_files(dir, &transaction.files, false)?;
            sync_changed_today(dir, &transaction.files)?;
        }
        remove(&dir.join(TRANSACTION))?;
    }
    Ok(())
}
fn sync_today(dir: &Path) -> DataResult<()> {
    let date = data::today_string();
    if let Some(plan) = live_plan(dir, &date)? {
        data::resync_plan_to_state(dir, &date, &plan)?;
    } else {
        let mut state = data::read_state_or_default(dir);
        if state.current_task.date == date
            && !state
                .current_task
                .tasks
                .iter()
                .any(|t| matches!(t.status, TaskStatus::Done | TaskStatus::InProgress))
        {
            state.current_task.tasks.clear();
            state.current_task.total_hours = Some(0.0);
            data::save_state(dir, &state)?;
        }
    }
    Ok(())
}
fn sync_changed_today(dir: &Path, files: &[FileChange]) -> DataResult<()> {
    let path = format!("plan/{}_day.json", data::today_string());
    if files.iter().any(|f| f.path == path) {
        sync_today(dir)?;
    }
    Ok(())
}
pub fn status(dir: &Path) -> DataResult<AdjustmentStatus> {
    recover(dir)?;
    let draft: Option<Draft> = load(dir, DRAFT)?;
    let journal: Option<Journal> = load(dir, JOURNAL)?;
    let warnings = journal
        .as_ref()
        .map(|j| j.warnings.clone())
        .unwrap_or_default();
    let sync_pending = journal.as_ref().is_some_and(|j| j.sync_pending);
    let undo = journal.filter(|j| j.status == "applied");
    let applied_preview = undo.as_ref().map(|j| j.preview.clone());
    Ok(AdjustmentStatus {
        preview: draft.map(|d| d.preview),
        undo_id: undo.as_ref().map(|j| j.id.clone()),
        undo_created_at: undo.map(|j| j.created_at),
        applied_preview,
        warnings,
        sync_pending,
    })
}
pub fn latest_dates(dir: &Path) -> DataResult<Vec<String>> {
    recover(dir)?;
    let journal: Journal = load(dir, JOURNAL)?.ok_or("没有需要同步的调整")?;
    Ok(journal.dates)
}
pub fn record_sync_result(dir: &Path, warnings: Vec<String>, pending: bool) -> DataResult<()> {
    if let Some(mut journal) = load::<Journal>(dir, JOURNAL)? {
        journal.warnings = warnings;
        journal.sync_pending = pending;
        save(dir, JOURNAL, &journal)?;
    }
    Ok(())
}
pub fn apply(dir: &Path, id: &str) -> DataResult<Vec<String>> {
    recover(dir)?;
    let draft = get_draft(dir, id)?;
    if draft.today != data::today_string() || fingerprint(&inputs(dir)?) != draft.fingerprint {
        return Err("预览后计划、进度或设置发生变化，请重新生成预览后再应用".into());
    }
    if !draft.preview.conflicts.is_empty() {
        return Err("请先处理保留任务与休息日的冲突".into());
    }
    if draft.preview.affected_dates.is_empty() && draft.preview.goals.is_empty() {
        return Err("本次没有需要应用的调整".into());
    }
    let mut journal = Journal {
        id: id.into(),
        created_at: data::now_string(),
        status: "prepared".into(),
        dates: draft.preview.affected_dates.clone(),
        files: draft.files,
        preview: draft.preview,
        warnings: Vec::new(),
        sync_pending: true,
    };
    save(dir, TRANSACTION, &journal)?;
    if let Err(e) =
        write_files(dir, &journal.files, true).and_then(|_| sync_changed_today(dir, &journal.files))
    {
        recover(dir)?;
        return Err(e);
    }
    journal.status = "applied".into();
    if let Err(e) = save(dir, JOURNAL, &journal) {
        recover(dir)?;
        return Err(e);
    }
    remove(&dir.join(TRANSACTION))?;
    remove(&dir.join(DRAFT))?;
    Ok(journal.dates)
}

/// 比较安排时忽略同步回填和真实完成状态；用户后来编辑的内容仍会阻止覆盖。
fn arrangement(raw: &str, path: &str) -> DataResult<serde_json::Value> {
    let mut value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if path.ends_with("_day.json") {
        if let Some(tasks) = value
            .pointer_mut("/data/tasks")
            .and_then(|v| v.as_array_mut())
        {
            for task in tasks {
                if let Some(o) = task.as_object_mut() {
                    o.remove("status");
                    o.remove("dida_task_id");
                }
            }
        }
    } else if path == "plan/goals.json" {
        value.as_object_mut().map(|v| v.remove("meta"));
        if let Some(goals) = value
            .pointer_mut("/data/goals")
            .and_then(|v| v.as_array_mut())
        {
            for goal in goals {
                if let Some(o) = goal.as_object_mut() {
                    o.remove("current_position");
                    o.remove("active");
                    o.remove("status");
                }
            }
        }
    }
    Ok(value)
}
pub fn undo(dir: &Path, id: &str) -> DataResult<(Vec<String>, Vec<String>)> {
    recover(dir)?;
    let mut journal: Journal = load(dir, JOURNAL)?.ok_or("没有可撤销的调整")?;
    if journal.id != id || journal.status != "applied" {
        return Err("只能撤销最近一次已应用的调整".into());
    }
    let mut restore = Vec::new();
    let mut warnings = Vec::new();
    for file in &journal.files {
        let current = read_optional(&dir.join(&file.path))?;
        if current
            .as_ref()
            .map(|s| arrangement(s, &file.path))
            .transpose()?
            != file
                .after
                .as_ref()
                .map(|s| arrangement(s, &file.path))
                .transpose()?
        {
            return Err("调整后你又修改了计划或目标安排，不能直接撤销覆盖；请重新预览调整".into());
        }
        let mut restored = file.before.clone();
        if file.path.ends_with("_day.json") {
            let date = file
                .path
                .strip_prefix("plan/")
                .unwrap()
                .strip_suffix("_day.json")
                .unwrap();
            if date < data::today_string().as_str() {
                warnings.push(format!("{date} 已成为历史日期，保留当日实际安排"));
                continue;
            }
            if let Some(live) = live_plan(dir, date)? {
                let mut old: DailyPlanFile = restored
                    .as_ref()
                    .map(|s| serde_json::from_str(s))
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or_else(|| {
                        let mut p = live.clone();
                        p.data.tasks.clear();
                        p
                    });
                for task in &mut old.data.tasks {
                    if let Some(now) = live.data.tasks.iter().find(|t| same_task(t, task)) {
                        task.id = now.id.clone();
                        task.status = now.status.clone();
                        task.dida_task_id = now.dida_task_id.clone();
                    }
                }
                for now in &live.data.tasks {
                    if matches!(now.status, TaskStatus::Done | TaskStatus::InProgress)
                        && !old.data.tasks.iter().any(|t| same_task(t, now))
                    {
                        old.data.tasks.push(now.clone());
                        warnings.push(format!("{date}「{}」已经学习，撤销时继续保留", now.title));
                    }
                }
                data::recalc_totals(&mut old.data);
                restored = if file.before.is_none() && old.data.tasks.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string_pretty(&old).map_err(|e| e.to_string())?)
                };
            }
        } else if file.path.ends_with("_week.json") {
            if let (Some(raw), Some(current)) = (&restored, &current) {
                let mut old: data::WeekPlanFile =
                    serde_json::from_str(raw).map_err(|e| e.to_string())?;
                let now: data::WeekPlanFile =
                    serde_json::from_str(current).map_err(|e| e.to_string())?;
                for day in &mut old.data.days {
                    if day.date < data::today_string() {
                        if let Some(live) = now.data.days.iter().find(|d| d.date == day.date) {
                            *day = live.clone();
                        }
                    }
                }
                restored = Some(serde_json::to_string_pretty(&old).map_err(|e| e.to_string())?);
            }
        } else if file.path == "plan/goals.json" {
            if let (Some(raw), Some(current)) = (&restored, &current) {
                let mut old: data::GoalPlanFile =
                    serde_json::from_str(raw).map_err(|e| e.to_string())?;
                let now: data::GoalPlanFile =
                    serde_json::from_str(current).map_err(|e| e.to_string())?;
                for goal in &mut old.data.goals {
                    if let Some(live) = now.data.goals.iter().find(|g| g.id == goal.id) {
                        goal.current_position = live.current_position;
                        data::refresh_goal_lifecycle(goal, &data::today_string());
                    }
                }
                restored = Some(serde_json::to_string_pretty(&old).map_err(|e| e.to_string())?);
            }
        }
        restore.push(FileChange {
            path: file.path.clone(),
            before: current,
            after: restored,
        });
    }
    journal.files = restore;
    journal.status = "undo_prepared".into();
    journal.sync_pending = true;
    save(dir, TRANSACTION, &journal)?;
    if let Err(e) =
        write_files(dir, &journal.files, true).and_then(|_| sync_changed_today(dir, &journal.files))
    {
        recover(dir)?;
        return Err(e);
    }
    journal.status = "undone".into();
    if let Err(e) = save(dir, JOURNAL, &journal) {
        recover(dir)?;
        return Err(e);
    }
    remove(&dir.join(TRANSACTION))?;
    remove(&dir.join(DRAFT))?;
    Ok((journal.dates, warnings))
}

#[cfg(test)]
#[path = "plan_adjustment_tests.rs"]
mod tests;
