//! 用户明确确认的当日缩减；复用日计划积压队列，不重排后续计划。
use crate::data::plan::{DailyPlanFile, PlanTask};
use crate::data::state::{StudyState, TaskStatus};
use crate::{get_data_dir, AppState};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::State;

#[derive(Serialize)]
pub struct EnergyPreview {
    plan: DailyPlanFile,
    protected_ids: Vec<String>,
    snapshot: String,
    goals: Vec<crate::data::goal::Goal>,
}

/// 预览只读取文件，避免通用计划读取接口的自动同步改写旧 State。
fn current_plan(dir: &std::path::Path, date: &str) -> Result<(DailyPlanFile, StudyState), String> {
    let mut plan = crate::data::plan::read_daily_plan(dir, date)?;
    let study = crate::data::state::read_state(dir)?;
    if study.current_task.date == date {
        for task in &mut plan.data.tasks {
            if let Some(old) = study
                .current_task
                .tasks
                .iter()
                .find(|st| st.task_id.as_deref() == Some(task.id.as_str()))
                .or_else(|| {
                    study.current_task.tasks.iter().find(|st| {
                        st.task_id.is_none()
                            && st.task == task.title
                            && st.subject == task.subject.key()
                    })
                })
            {
                task.status = old.status.clone();
            }
        }
    }
    Ok((plan, study))
}

fn protected_ids(plan: &DailyPlanFile, state: &StudyState) -> Vec<String> {
    plan.data
        .tasks
        .iter()
        .filter(|task| {
            task.is_manual()
                || task.status != TaskStatus::Pending
                || (state.current_task.date == plan.meta.date
                    && state.current_task.tasks.iter().any(|st| {
                        (st.task_id.as_deref() == Some(task.id.as_str())
                            || (st.task_id.is_none()
                                && st.task == task.title
                                && st.subject == task.subject.key()))
                            && (st.started_at.is_some()
                                || st.accumulated_minutes > 0
                                || st.status != TaskStatus::Pending)
                    }))
        })
        .map(|t| t.id.clone())
        .collect()
}

fn defer_selected(
    plan: &mut DailyPlanFile,
    ids: &[String],
    protected: &[String],
) -> Result<(), String> {
    let selected: HashSet<_> = ids.iter().collect();
    if selected.is_empty() {
        return Err("请先选择需要推迟的任务".into());
    }
    if selected
        .iter()
        .any(|id| protected.contains(id) || !plan.data.tasks.iter().any(|t| &t.id == *id))
    {
        return Err("任务已变化，或包含手动、已开始、已完成的任务，请重新查看方案".into());
    }
    let deferred: Vec<PlanTask> = plan
        .data
        .tasks
        .iter()
        .filter(|t| selected.contains(&t.id))
        .cloned()
        .collect();
    plan.data.tasks.retain(|t| !selected.contains(&t.id));
    for task in deferred {
        if !plan.data.deferred_tasks.iter().any(|t| t.id == task.id) {
            plan.data.deferred_tasks.push(task);
        }
    }
    crate::data::plan::recalc_totals(&mut plan.data);
    plan.view = None;
    Ok(())
}

#[tauri::command]
pub async fn preview_low_energy(
    state: State<'_, Mutex<AppState>>,
) -> Result<EnergyPreview, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    let date = crate::data::today_string();
    let (plan, study) = current_plan(&dir, &date)?;
    let goals = crate::data::goal::read_goals(&dir)?.data.goals;
    Ok(EnergyPreview {
        protected_ids: protected_ids(&plan, &study),
        snapshot: serde_json::to_string(&(&plan, &goals)).map_err(|e| e.to_string())?,
        plan,
        goals,
    })
}

#[tauri::command]
pub async fn apply_low_energy(
    task_ids: Vec<String>,
    snapshot: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<DailyPlanFile, String> {
    let dir = get_data_dir(state.inner())?;
    let lock = crate::get_io_lock(state.inner())?;
    let _guard = lock.lock().await;
    let date = crate::data::today_string();
    let (mut plan, study) = current_plan(&dir, &date)?;
    let goals = crate::data::goal::read_goals(&dir)?.data.goals;
    if serde_json::to_string(&(&plan, &goals)).map_err(|e| e.to_string())? != snapshot {
        return Err("今日计划已变化，请重新打开缩减方案后确认".into());
    }
    let protected = protected_ids(&plan, &study);
    let original = plan.clone();
    defer_selected(&mut plan, &task_ids, &protected)?;
    let mut updated = study.clone();
    updated.current_task.date = date.clone();
    updated.current_task.total_hours = Some(plan.data.total_hours);
    updated.current_task.tasks = plan
        .data
        .tasks
        .iter()
        .map(|task| {
            let old = if study.current_task.date == date {
                study
                    .current_task
                    .tasks
                    .iter()
                    .find(|st| st.task_id.as_deref() == Some(task.id.as_str()))
                    .or_else(|| {
                        study.current_task.tasks.iter().find(|st| {
                            st.task_id.is_none()
                                && st.task == task.title
                                && st.subject == task.subject.key()
                        })
                    })
            } else {
                None
            };
            let mut retained = old.cloned().unwrap_or_default();
            retained.task_id = Some(task.id.clone());
            retained.task = task.title.clone();
            retained.subject = task.subject.key().into();
            retained.priority = task.priority.clone();
            retained.status = task.status.clone();
            retained.source = task.source.clone();
            retained.ai_reference = task.ai_reference;
            retained
        })
        .collect();
    crate::data::plan::save_daily_plan(&dir, &plan)?;
    if let Err(error) = crate::data::state::save_state(&dir, &updated) {
        crate::data::plan::save_daily_plan(&dir, &original)
            .map_err(|rollback| format!("保存状态失败：{}；恢复计划失败：{}", error, rollback))?;
        return Err(error);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str) -> PlanTask {
        PlanTask {
            id: id.into(),
            title: id.into(),
            ..PlanTask::default()
        }
    }
    #[test]
    fn deferral_preserves_content_and_existing_queue() {
        let mut plan = DailyPlanFile::default();
        plan.data.tasks = vec![task("keep"), task("defer")];
        plan.data.tasks[1].estimated_hours = 2.5;
        plan.data.deferred_tasks = vec![task("old")];
        defer_selected(&mut plan, &["defer".into()], &[]).unwrap();
        assert_eq!(plan.data.total_tasks, 1);
        assert_eq!(plan.data.deferred_tasks.len(), 2);
        assert_eq!(plan.data.deferred_tasks[1].estimated_hours, 2.5);
    }
    #[test]
    fn rejects_protected_unknown_and_empty_selections_without_mutation() {
        let mut plan = DailyPlanFile::default();
        plan.data.tasks = vec![task("keep")];
        for ids in [vec![], vec!["unknown".into()], vec!["keep".into()]] {
            assert!(defer_selected(&mut plan, &ids, &["keep".into()]).is_err());
            assert_eq!(plan.data.tasks.len(), 1);
            assert!(plan.data.deferred_tasks.is_empty());
        }
    }

    #[test]
    fn legacy_timer_and_manual_tasks_are_protected() {
        let mut plan = DailyPlanFile::default();
        plan.meta.date = "2026-10-06".into();
        let mut manual = task("manual");
        manual.source = crate::data::state::TaskSource::Manual;
        plan.data.tasks = vec![task("timer"), manual, task("pending")];
        let mut study = StudyState::default();
        study.current_task.date = plan.meta.date.clone();
        study
            .current_task
            .tasks
            .push(crate::data::state::StateTask {
                task: "timer".into(),
                subject: plan.data.tasks[0].subject.key().into(),
                accumulated_minutes: 15,
                ..Default::default()
            });
        assert_eq!(protected_ids(&plan, &study), vec!["timer", "manual"]);
        let serialized = serde_json::to_string(&plan).unwrap();
        let restored: DailyPlanFile = serde_json::from_str(&serialized).unwrap();
        assert!(restored.data.deferred_tasks.is_empty());
    }

    #[test]
    fn preview_preserves_legacy_state_file_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "studyagent-energy-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let mut plan = DailyPlanFile::default();
        plan.meta.date = "2026-10-06".into();
        plan.data.tasks = vec![task("timer")];
        let mut study = StudyState::default();
        study.current_task.date = plan.meta.date.clone();
        study
            .current_task
            .tasks
            .push(crate::data::state::StateTask {
                task: "timer".into(),
                subject: plan.data.tasks[0].subject.key().into(),
                status: TaskStatus::InProgress,
                accumulated_minutes: 15,
                ..Default::default()
            });
        crate::data::plan::save_daily_plan(&dir, &plan).unwrap();
        crate::data::state::save_state(&dir, &study).unwrap();
        let state_path = dir.join("state/current.state");
        let before = std::fs::read(&state_path).unwrap();
        let (preview, state) = current_plan(&dir, &plan.meta.date).unwrap();
        assert_eq!(preview.data.tasks[0].status, TaskStatus::InProgress);
        assert_eq!(protected_ids(&preview, &state), vec!["timer"]);
        assert_eq!(std::fs::read(&state_path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
