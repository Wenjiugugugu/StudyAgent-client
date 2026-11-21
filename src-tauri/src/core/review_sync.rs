//! 复盘主记录与派生同步分离；重试只读取当前记录，不回写旧快照。
use crate::data::{self, DataResult, ReviewFile, StudyState};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncStatus {
    pub review_revision: String,
    pub sync_status: String,
    pub pending_steps: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Box<Journal>>,
    status: SyncStatus,
    state_before: Option<StudyState>,
    progress_before: Option<data::ProgressIndex>,
}
fn path(dir: &Path, date: &str) -> std::path::PathBuf {
    dir.join("config/review-sync")
        .join(format!("{}.json", date))
}
fn persist(dir: &Path, date: &str, journal: &Journal) -> DataResult<()> {
    data::atomic_write(
        &path(dir, date),
        &serde_json::to_string_pretty(journal).map_err(|e| e.to_string())?,
    )
}
pub fn prepare(dir: &Path, review: &ReviewFile) -> DataResult<String> {
    let revision = data::revision::content_revision(review)?;
    let previous = match std::fs::read_to_string(path(dir, &review.meta.date)) {
        Ok(raw) => {
            let old: Journal = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            committed_journal(dir, &review.meta.date, old)?.map(Box::new)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let journal = Journal {
        previous,
        state_before: data::read_state(dir).ok(),
        progress_before: data::read_progress_index(dir).ok(),
        status: SyncStatus {
            review_revision: revision.clone(),
            sync_status: "pending".into(),
            pending_steps: vec![
                "state".into(),
                "progress".into(),
                "phase".into(),
                "analysis".into(),
            ],
            warnings: vec![],
        },
    };
    persist(dir, &review.meta.date, &journal)?;
    Ok(revision)
}
fn committed_journal(dir: &Path, date: &str, mut journal: Journal) -> DataResult<Option<Journal>> {
    let revision = match std::fs::metadata(data::records::review_file_path(dir, date)) {
        Ok(_) => data::revision::content_revision(&data::read_review(dir, date)?)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    loop {
        if journal.status.review_revision == revision {
            journal.previous = None;
            return Ok(Some(journal));
        }
        let Some(previous) = journal.previous.take() else {
            return Ok(None);
        };
        journal = *previous;
    }
}
pub fn status(dir: &Path, date: &str) -> DataResult<SyncStatus> {
    data::validate_date(date)?;
    let complete = || SyncStatus {
        sync_status: "complete".into(),
        ..Default::default()
    };
    match std::fs::read_to_string(path(dir, date)) {
        Ok(raw) => {
            let journal: Journal = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            Ok(committed_journal(dir, date, journal)?
                .map(|j| j.status)
                .unwrap_or_else(complete))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(complete()),
        Err(e) => Err(e.to_string()),
    }
}
fn sync_state(dir: &Path, review: &ReviewFile, before: Option<&StudyState>) -> DataResult<()> {
    let mut state = data::read_state_or_new(dir)?;
    for entry in &review.task_reviews {
        if state.current_task.date == review.meta.date {
            if let Some(task) = state
                .current_task
                .tasks
                .iter_mut()
                .find(|t| t.task_id.as_deref() == Some(&entry.task_id))
            {
                let old = before.and_then(|s| {
                    s.current_task
                        .tasks
                        .iter()
                        .find(|t| t.task_id == task.task_id)
                });
                // 有后来操作的任务保持原样；不改计时字段。
                if old.is_some_and(|old| {
                    old.status == task.status
                        && old.task == task.task
                        && old.subject == task.subject
                        && old.started_at == task.started_at
                        && old.accumulated_minutes == task.accumulated_minutes
                }) {
                    task.status = match entry.status.as_str() {
                        "completed" => data::TaskStatus::Done,
                        "abandoned" => data::TaskStatus::Abandoned,
                        _ => data::TaskStatus::Pending,
                    };
                }
            }
        }
        if entry.status != "completed" && entry.status != "partial" {
            continue;
        }
        if let Some(subject) = data::get_subject_state_mut(&mut state, &entry.subject) {
            let titles = entry
                .completed_knowledge_points
                .as_ref()
                .map(|points| points.iter().map(|p| p.title.clone()).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![entry.title.clone()]);
            for title in titles {
                let title = if entry.mastery == "weak" {
                    format!("{} （需巩固）", title)
                } else {
                    title
                };
                if !title.is_empty() && !subject.completed.contains(&title) {
                    subject.completed.push(title);
                }
            }
        }
    }
    for entry in &review.overcompletion {
        let old_focus = before
            .and_then(|s| {
                data::SubjectKey::ALL
                    .iter()
                    .find(|key| key.key() == entry.subject)
                    .map(|key| key.state(s))
            })
            .map(|s| &s.current_focus);
        if let Some(subject) = data::get_subject_state_mut(&mut state, &entry.subject) {
            if !entry.chapter_reached.is_empty() {
                if old_focus == Some(&subject.current_focus) {
                    subject.current_focus = entry.chapter_reached.clone();
                }
                if !subject.completed.contains(&entry.chapter_reached) {
                    subject.completed.push(entry.chapter_reached.clone());
                }
            }
        }
    }
    if state.progress.last_study_date < review.meta.date {
        state.progress.last_study_date = review.meta.date.clone();
    }
    data::save_state(dir, &state)
}
fn sync_progress(
    dir: &Path,
    tasks: &[super::progress_sync::ReviewSyncTask],
    before: Option<&data::ProgressIndex>,
) -> DataResult<()> {
    let current = data::read_progress_index(dir)?;
    let mut candidate = current.clone();
    super::progress_sync::apply_review_tasks(&mut candidate, tasks);
    if let Some(before) = before {
        for (subject, set) in &mut candidate.subjects {
            for table in &mut set.tables {
                let current_table = current
                    .subjects
                    .get(subject)
                    .and_then(|s| s.tables.iter().find(|t| t.id == table.id));
                let old_table = before
                    .subjects
                    .get(subject)
                    .and_then(|s| s.tables.iter().find(|t| t.id == table.id));
                for node in &mut table.nodes {
                    let now = current_table.and_then(|t| t.nodes.iter().find(|n| n.id == node.id));
                    let old = old_table.and_then(|t| t.nodes.iter().find(|n| n.id == node.id));
                    if let Some(now) = now {
                        if old.is_none_or(|old| {
                            old.status != now.status
                                || old.title != now.title
                                || old.level != now.level
                                || old.parent_id != now.parent_id
                                || old.phase != now.phase
                        }) || old_table.zip(current_table).is_none_or(|(old, now)| {
                            old.variant != now.variant || old.subject != now.subject
                        }) {
                            node.status = now.status;
                        }
                    }
                }
            }
        }
    }
    data::save_progress_index(dir, &candidate)
}
pub fn retry(dir: &Path, date: &str, expected: &str) -> DataResult<SyncStatus> {
    data::validate_date(date)?;
    let raw = std::fs::read_to_string(path(dir, date)).map_err(|e| e.to_string())?;
    let journal: Journal = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut journal = committed_journal(dir, date, journal)?
        .ok_or("此同步日志没有对应的已保存复盘，不会自动提交或覆盖旧记录")?;
    let review = data::read_review(dir, date)?;
    if expected != journal.status.review_revision
        || data::revision::content_revision(&review)? != expected
    {
        return Err("复盘记录已变化，请重新加载后同步；旧复盘不会写回。".into());
    }
    journal.status.warnings.clear();
    for step in journal.status.pending_steps.clone() {
        let result = match step.as_str() {
            "state" => sync_state(dir, &review, journal.state_before.as_ref()),
            "progress" => {
                let tasks = review
                    .task_reviews
                    .iter()
                    .map(|entry| super::progress_sync::ReviewSyncTask {
                        subject: entry.subject.clone(),
                        title: entry.title.clone(),
                        completed: entry.status == "completed" || entry.status == "partial",
                        mastery: entry.mastery.clone(),
                        completed_knowledge_points: entry.completed_knowledge_points.clone(),
                    })
                    .collect::<Vec<_>>();
                sync_progress(dir, &tasks, journal.progress_before.as_ref())
            }
            "phase" => data::read_progress_index(dir)
                .and_then(|index| {
                    data::progress_tables::promote_completed_subject_phases(dir, &index)
                })
                .map(|_| ()),
            "analysis" => data::get_week_start(date)
                .and_then(|start| {
                    super::adaptive_planner::refresh_week_planning_analysis(dir, &start)
                })
                .map(|_| ()),
            _ => Err("未知复盘同步步骤".into()),
        };
        match result {
            Ok(()) => journal.status.pending_steps.retain(|s| s != &step),
            Err(e) => {
                let label = match step.as_str() {
                    "state" => "学习状态",
                    "progress" => "知识点进度",
                    "phase" => "学习阶段",
                    "analysis" => "学习分析",
                    _ => "同步",
                };
                journal.status.warnings.push(format!("{}: {}", label, e));
            }
        }
        journal.status.sync_status = if journal.status.pending_steps.is_empty() {
            "complete"
        } else {
            "pending"
        }
        .into();
        persist(dir, date, &journal)?;
    }
    Ok(journal.status)
}
pub fn recover(dir: &Path) -> DataResult<()> {
    let root = dir.join("config/review-sync");
    let entries = match std::fs::read_dir(root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path();
        let Some(date) = name.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let status = status(dir, date)?;
        if status.pending_steps.is_empty() {
            continue;
        }
        // prepared 日志不等于已提交；没有对应主记录时保持待处理，不代用户提交。
        if let Err(e) = retry(dir, date, &status.review_revision) {
            log::warn!("复盘恢复 {}: {}", date, e);
        }
    }
    Ok(())
}
pub fn ensure_ready(dir: &Path) -> DataResult<()> {
    let root = dir.join("config/review-sync");
    let entries = match std::fs::read_dir(root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path();
        let Some(date) = name.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let status = status(dir, date)?;
        if !status.pending_steps.is_empty() {
            if let Ok(review) = data::read_review(dir, date) {
                if data::revision::content_revision(&review)? == status.review_revision {
                    return Err(format!(
                        "{} 的复盘已保存，进度同步尚未完成。请先在复盘页重试同步。",
                        date
                    ));
                }
            }
        }
    }
    Ok(())
}
