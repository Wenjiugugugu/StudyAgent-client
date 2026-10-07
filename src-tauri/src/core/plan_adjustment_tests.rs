use super::*;
use crate::data::{SubjectKey, TaskSource};

#[test]
fn adjustment_keeps_knowledge_links_in_week_and_daily_plans() {
    let (_workspace, mut draft) = setup();
    let choice = draft
        .preview
        .tasks
        .iter_mut()
        .find(|t| !t.task.is_manual())
        .unwrap();
    choice.task.knowledge_points = Some(vec![data::plan::KnowledgePointRef {
        table_id: "math".into(),
        node_id: "limit".into(),
        title: "极限".into(),
    }]);
    choice.task.remaining_from_task_id = Some("previous-task".into());
    let expected = choice.task.clone();
    draft.preview.kept_keys.push(choice.key.clone());
    project(&mut draft).unwrap();
    let day_file = draft
        .files
        .iter()
        .find(|f| f.path.ends_with("_day.json"))
        .unwrap();
    let daily: DailyPlanFile = serde_json::from_str(day_file.after.as_ref().unwrap()).unwrap();
    let retained = daily
        .data
        .tasks
        .iter()
        .find(|t| t.id == expected.id)
        .unwrap();
    assert_eq!(retained.knowledge_points, expected.knowledge_points);
    assert_eq!(
        retained.remaining_from_task_id,
        expected.remaining_from_task_id
    );
    let week_file = draft
        .files
        .iter()
        .find(|f| f.path.ends_with("_week.json"))
        .unwrap();
    let week: data::WeekPlanFile = serde_json::from_str(week_file.after.as_ref().unwrap()).unwrap();
    let template = week
        .data
        .days
        .iter()
        .find(|d| d.date == draft.today)
        .unwrap()
        .subject_allocations
        .iter()
        .flat_map(|a| &a.task_templates)
        .find(|t| t.title == expected.title)
        .unwrap();
    assert_eq!(template.knowledge_points, expected.knowledge_points);
    assert_eq!(
        template.remaining_from_task_id,
        expected.remaining_from_task_id
    );
    let mut different = expected.clone();
    different.knowledge_points = Some(Vec::new());
    assert!(!same_task(&expected, &different));
}

fn task(date: &str, seq: u32, title: &str, hours: f64) -> PlanTask {
    PlanTask {
        id: format!("{date}-{seq:02}"),
        subject: SubjectKey::Math,
        title: title.into(),
        estimated_hours: hours,
        ..Default::default()
    }
}
fn day(date: &str, tasks: Vec<PlanTask>) -> DailyPlanFile {
    let mut p = DailyPlanFile {
        version: "1.0.0".into(),
        ..Default::default()
    };
    p.meta.date = date.into();
    p.data.tasks = tasks;
    data::recalc_totals(&mut p.data);
    p
}
fn setup() -> (Workspace, Draft) {
    let workspace = Workspace::new(&BTreeMap::new()).unwrap();
    let dir = &workspace.0;
    let today = data::today_string();
    let mut manual = task(&today, 2, "手动任务", 0.25);
    manual.source = TaskSource::Manual;
    let old = day(&today, vec![task(&today, 1, "旧任务", 1.0), manual]);
    data::save_daily_plan(dir, &old).unwrap();
    let start = data::get_week_start(&today).unwrap();
    let mut week = data::WeekPlanFile::default();
    week.meta.week_start = start.clone();
    week.meta.week_end = data::get_week_end(&today).unwrap();
    for offset in 0..7 {
        let date = data::add_days(&start, offset).unwrap();
        week.data.days.push(data::WeekDayPlan {
            date: date.clone(),
            weekday: data::weekday_name(&date).unwrap_or_default(),
            is_rest_day: false,
            subject_allocations: vec![data::DaySubjectAllocation {
                subject: SubjectKey::Math,
                hours: 1.0,
                task_templates: vec![data::TaskTemplate {
                    title: "旧任务".into(),
                    estimated_hours: 1.0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
    }
    data::save_week_plan(dir, &week).unwrap();
    let mut state = data::StudyState::default();
    state.subjects.math.active = true;
    state.current_task.date = today.clone();
    data::save_state(dir, &state).unwrap();
    data::resync_plan_to_state(dir, &today, &old).unwrap();
    let generated = day(&today, vec![task(&today, 1, "新任务", 0.5)]);
    let tasks = task_choices(dir, std::slice::from_ref(&today)).unwrap();
    let mut draft = Draft {
        preview: AdjustmentPreview {
            id: "test-adjustment".into(),
            created_at: data::now_string(),
            kind: "review".into(),
            source_date: today.clone(),
            excluded_day: None,
            affected_dates: vec![today.clone()],
            reason: "复盘反馈".into(),
            used_fallback: false,
            warnings: vec![],
            conflicts: vec![],
            kept_keys: tasks
                .iter()
                .filter(|t| t.default_kept)
                .map(|t| t.key.clone())
                .collect(),
            tasks,
            changes: vec![],
            days: vec![],
            goals: vec![],
        },
        today: today.clone(),
        fingerprint: fingerprint(&inputs(dir).unwrap()),
        files: vec![
            FileChange {
                path: format!("plan/{}_week.json", data::iso_week_string(&today).unwrap()),
                before: Some(serde_json::to_string_pretty(&week).unwrap()),
                after: Some(serde_json::to_string_pretty(&week).unwrap()),
            },
            FileChange {
                path: format!("plan/{today}_day.json"),
                before: Some(serde_json::to_string_pretty(&old).unwrap()),
                after: Some(serde_json::to_string_pretty(&generated).unwrap()),
            },
        ],
        generated_days: BTreeMap::from([(today.clone(), generated)]),
        sequence_base: BTreeMap::from([(today, 2)]),
        generated_week: week,
        baseline_warnings: Vec::new(),
        daily_budget: 5.0,
    };
    project(&mut draft).unwrap();
    save(dir, DRAFT, &draft).unwrap();
    (workspace, draft)
}

#[test]
fn preview_selection_and_discard_do_not_write_plan_or_state() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    let before = inputs(dir).unwrap();
    let key = draft
        .preview
        .tasks
        .iter()
        .find(|t| !t.required)
        .unwrap()
        .key
        .clone();
    let preview = update_kept(dir, &draft.preview.id, vec![key]).unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert_eq!(preview.kept_keys.len(), 2);
    assert!(!preview.changes.iter().any(|c| c.title == "旧任务"));
    discard(dir, &draft.preview.id).unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert!(status(dir).unwrap().preview.is_none());
}
#[test]
fn apply_requires_current_preview_and_preserves_manual_tasks() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    assert!(apply(dir, "wrong-preview").is_err());
    apply(dir, &draft.preview.id).unwrap();
    let plan = data::read_daily_plan(dir, &draft.today).unwrap();
    assert!(plan
        .data
        .tasks
        .iter()
        .any(|t| t.title == "手动任务" && t.id.ends_with("-02")));
    assert!(plan
        .data
        .tasks
        .iter()
        .any(|t| t.title == "新任务" && !t.id.ends_with("-01")));
    assert_eq!(plan.data.total_hours, 0.75);
    assert!(status(dir).unwrap().preview.is_none());
    assert!(status(dir).unwrap().undo_id.is_some());
    assert!(apply(dir, &draft.preview.id).is_err());
}
#[test]
fn stale_preview_and_rest_conflict_are_blocked() {
    let (workspace, mut draft) = setup();
    let dir = &workspace.0;
    draft
        .generated_week
        .data
        .days
        .iter_mut()
        .find(|d| d.date == draft.today)
        .unwrap()
        .is_rest_day = true;
    project(&mut draft).unwrap();
    save(dir, DRAFT, &draft).unwrap();
    assert!(!draft.preview.conflicts.is_empty());
    assert!(apply(dir, &draft.preview.id).is_err());
    draft.preview.conflicts.clear();
    save(dir, DRAFT, &draft).unwrap();
    let mut state = data::read_state(dir).unwrap();
    state.current_task.tasks[0].status = TaskStatus::Done;
    data::save_state(dir, &state).unwrap();
    assert!(apply(dir, &draft.preview.id)
        .unwrap_err()
        .contains("发生变化"));
}
#[test]
fn undo_preserves_new_learning_timer_and_review() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    apply(dir, &draft.preview.id).unwrap();
    let mut state = data::read_state(dir).unwrap();
    let learned = state
        .current_task
        .tasks
        .iter_mut()
        .find(|t| t.task == "新任务")
        .unwrap();
    learned.status = TaskStatus::InProgress;
    learned.accumulated_minutes = 17;
    learned.started_at = Some(data::now_string());
    data::save_state(dir, &state).unwrap();
    let path = dir.join("records/kept_review.json");
    data::atomic_write(&path, "{\"record\":\"真实复盘\"}").unwrap();
    let (_, warnings) = undo(dir, &draft.preview.id).unwrap();
    assert!(!warnings.is_empty());
    let plan = data::read_daily_plan(dir, &draft.today).unwrap();
    assert!(plan.data.tasks.iter().any(|t| t.title == "旧任务"));
    assert!(plan
        .data
        .tasks
        .iter()
        .any(|t| t.title == "新任务" && t.status == TaskStatus::InProgress));
    let state = data::read_state(dir).unwrap();
    let learned = state
        .current_task
        .tasks
        .iter()
        .find(|t| t.task == "新任务")
        .unwrap();
    assert_eq!(learned.accumulated_minutes, 17);
    assert!(learned.started_at.is_some());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "{\"record\":\"真实复盘\"}"
    );
    assert!(undo(dir, &draft.preview.id).is_err());
}
#[test]
fn undo_does_not_overwrite_later_manual_edits() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    apply(dir, &draft.preview.id).unwrap();
    let mut plan = data::read_daily_plan(dir, &draft.today).unwrap();
    plan.data.tasks[0].title = "后来手动改过".into();
    data::save_daily_plan(dir, &plan).unwrap();
    let before = inputs(dir).unwrap();
    assert!(undo(dir, &draft.preview.id).is_err());
    assert_eq!(before, inputs(dir).unwrap());
}
#[test]
fn interrupted_commit_rolls_back_without_losing_preview() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    let before = inputs(dir).unwrap();
    let journal = Journal {
        id: draft.preview.id.clone(),
        created_at: data::now_string(),
        status: "prepared".into(),
        dates: draft.preview.affected_dates.clone(),
        files: draft.files.clone(),
        preview: draft.preview.clone(),
        warnings: Vec::new(),
        sync_pending: false,
    };
    save(dir, TRANSACTION, &journal).unwrap();
    write_files(dir, &journal.files[..1], true).unwrap();
    recover(dir).unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert!(status(dir).unwrap().preview.is_some());
    assert!(status(dir).unwrap().undo_id.is_none());
}

#[test]
fn failed_second_adjustment_keeps_the_previous_undo_record() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    apply(dir, &draft.preview.id).unwrap();
    let before = inputs(dir).unwrap();
    let mut transaction: Journal = load(dir, JOURNAL).unwrap().unwrap();
    transaction.id = "next-adjustment".into();
    transaction.status = "prepared".into();
    for file in &mut transaction.files {
        file.before = read_optional(&dir.join(&file.path)).unwrap();
    }
    save(dir, TRANSACTION, &transaction).unwrap();
    write_files(dir, &transaction.files[..1], true).unwrap();
    recover(dir).unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert_eq!(
        status(dir).unwrap().undo_id.as_deref(),
        Some(draft.preview.id.as_str())
    );
}

#[test]
fn committed_adjustment_is_not_rolled_back_if_cleanup_was_interrupted() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    apply(dir, &draft.preview.id).unwrap();
    let before = inputs(dir).unwrap();
    let mut transaction: Journal = load(dir, JOURNAL).unwrap().unwrap();
    transaction.status = "prepared".into();
    save(dir, TRANSACTION, &transaction).unwrap();
    recover(dir).unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert!(status(dir).unwrap().undo_id.is_some());
    assert!(!dir.join(TRANSACTION).exists());
}
#[test]
fn diff_pairs_moves_and_duration_without_duplicate_added_removed() {
    let before = BTreeMap::from([(
        "2026-10-06".into(),
        day("2026-10-06", vec![task("2026-10-06", 1, "同名任务", 1.0)]),
    )]);
    let after = BTreeMap::from([(
        "2026-10-07".into(),
        day("2026-10-07", vec![task("2026-10-07", 1, "同名任务", 0.5)]),
    )]);
    let diff = task_changes(&before, &after, "排除日");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].before_date.as_deref(), Some("2026-10-06"));
    assert_eq!(diff[0].after_date.as_deref(), Some("2026-10-07"));
    assert_eq!(diff[0].before_hours, Some(1.0));
    assert_eq!(diff[0].after_hours, Some(0.5));
}
#[tokio::test]
async fn exclusion_generation_is_isolated_and_returns_changes() {
    let (workspace, _) = setup();
    let dir = &workspace.0;
    let before = inputs(dir).unwrap();
    let ai = crate::ai::service::AiService::new();
    let planner = Planner::new(&ai);
    let today = data::today_string();
    let preview = generate(
        dir,
        &planner,
        "exclusion",
        &data::get_week_start(&today).unwrap(),
        Some(ExcludedDay {
            date: today,
            reason_type: "other".into(),
            note: Some("测试".into()),
        }),
        None,
    )
    .await
    .unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert!(!preview.days.is_empty());
    assert!(!preview.changes.is_empty());
    assert!(!preview.conflicts.is_empty());
    assert!(apply(dir, &preview.id).is_err());
}

#[test]
fn keeping_one_repeated_task_keeps_other_days_and_can_be_unselected() {
    let (workspace, mut draft) = setup();
    let dir = &workspace.0;
    let tomorrow = data::add_days(&draft.today, 1).unwrap();
    let repeat = day(&tomorrow, vec![task(&tomorrow, 1, "旧任务", 1.0)]);
    draft
        .generated_days
        .insert(tomorrow.clone(), repeat.clone());
    draft.files.push(FileChange {
        path: format!("plan/{tomorrow}_day.json"),
        before: Some(serde_json::to_string_pretty(&repeat).unwrap()),
        after: Some(serde_json::to_string_pretty(&repeat).unwrap()),
    });
    draft.preview.affected_dates.push(tomorrow.clone());
    let key = draft
        .preview
        .tasks
        .iter()
        .find(|t| t.task.title == "旧任务")
        .unwrap()
        .key
        .clone();
    draft.preview.kept_keys.push(key.clone());
    project(&mut draft).unwrap();
    let after: DailyPlanFile = serde_json::from_str(
        draft
            .files
            .iter()
            .find(|f| f.path == format!("plan/{tomorrow}_day.json"))
            .unwrap()
            .after
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(after.data.tasks.len(), 1);
    assert_eq!(after.data.tasks[0].title, "旧任务");
    assert!(draft.preview.changes.iter().all(|c| c.title != "旧任务"));
    save(dir, DRAFT, &draft).unwrap();
    let preview = update_kept(dir, &draft.preview.id, Vec::new()).unwrap();
    assert!(!preview.kept_keys.contains(&key));
    assert!(preview
        .changes
        .iter()
        .any(|c| c.title == "旧任务" && c.after_date.is_none()));
}

#[test]
fn undo_restores_goal_dates_but_preserves_actual_goal_progress() {
    let (workspace, mut draft) = setup();
    let dir = &workspace.0;
    let mut old = data::GoalPlanFile::default();
    old.data.goals.push(data::Goal {
        id: "math-goal".into(),
        subject: SubjectKey::Math,
        title: "目标".into(),
        deadline: data::add_days(&draft.today, 2).unwrap(),
        current_position: Some(1),
        target_position: Some(10),
        active: true,
        status: "active".into(),
        ..Default::default()
    });
    data::save_goals(dir, &old).unwrap();
    let mut next = old.clone();
    next.data.goals[0].deadline = data::add_days(&draft.today, 4).unwrap();
    next.data.goals[0].current_position = Some(2);
    draft.files.push(FileChange {
        path: "plan/goals.json".into(),
        before: Some(serde_json::to_string_pretty(&old).unwrap()),
        after: Some(serde_json::to_string_pretty(&next).unwrap()),
    });
    draft.fingerprint = fingerprint(&inputs(dir).unwrap());
    project(&mut draft).unwrap();
    save(dir, DRAFT, &draft).unwrap();
    assert_eq!(draft.preview.goals.len(), 1);
    apply(dir, &draft.preview.id).unwrap();
    let mut current = data::read_goals(dir).unwrap();
    current.data.goals[0].current_position = Some(3);
    data::save_goals(dir, &current).unwrap();
    undo(dir, &draft.preview.id).unwrap();
    let restored = data::read_goals(dir).unwrap();
    assert_eq!(restored.data.goals[0].deadline, old.data.goals[0].deadline);
    assert_eq!(restored.data.goals[0].current_position, Some(3));
}

#[test]
fn same_title_content_changes_are_visible_and_sync_failures_survive_restart() {
    let (workspace, draft) = setup();
    let dir = &workspace.0;
    let before = day(&draft.today, vec![task(&draft.today, 1, "练习", 1.0)]);
    let mut after = before.clone();
    after.data.tasks[0].completion_criteria = vec!["完成十道题".into()];
    let changes = task_changes(
        &BTreeMap::from([(draft.today.clone(), before)]),
        &BTreeMap::from([(draft.today.clone(), after)]),
        "复盘反馈",
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].after_content.as_deref(), Some("完成十道题"));
    apply(dir, &draft.preview.id).unwrap();
    record_sync_result(dir, vec!["同步失败".into()], true).unwrap();
    let status = status(dir).unwrap();
    assert!(status.sync_pending);
    assert_eq!(status.warnings, vec!["同步失败"]);
}

#[test]
fn undo_removes_a_new_unstarted_daily_plan_and_its_pending_state() {
    let (workspace, mut draft) = setup();
    let dir = &workspace.0;
    remove(&data::daily_plan_path(dir, &draft.today)).unwrap();
    let mut state = data::read_state(dir).unwrap();
    state.current_task.tasks.clear();
    state.current_task.total_hours = Some(0.0);
    data::save_state(dir, &state).unwrap();
    draft
        .files
        .iter_mut()
        .find(|f| f.path.ends_with("_day.json"))
        .unwrap()
        .before = None;
    draft.preview.tasks.clear();
    draft.preview.kept_keys.clear();
    draft.fingerprint = fingerprint(&inputs(dir).unwrap());
    project(&mut draft).unwrap();
    save(dir, DRAFT, &draft).unwrap();
    apply(dir, &draft.preview.id).unwrap();
    assert!(data::daily_plan_path(dir, &draft.today).exists());
    undo(dir, &draft.preview.id).unwrap();
    assert!(!data::daily_plan_path(dir, &draft.today).exists());
    assert!(data::read_state(dir).unwrap().current_task.tasks.is_empty());
}
#[tokio::test]
async fn review_generation_keeps_official_progress_and_plans_unchanged() {
    let (workspace, _) = setup();
    let dir = &workspace.0;
    let review_date = data::get_week_start(&data::today_string()).unwrap();
    let mut review = data::ReviewFile::default();
    review.meta.date = review_date.clone();
    review.task_reviews.push(data::TaskReviewEntry {
        task_id: format!("{review_date}-01"),
        title: "旧任务".into(),
        subject: "math".into(),
        status: "incomplete".into(),
        ..Default::default()
    });
    data::save_review(dir, &review).unwrap();
    let before = inputs(dir).unwrap();
    let ai = crate::ai::service::AiService::new();
    let planner = Planner::new(&ai);
    let preview = generate(dir, &planner, "review", &review_date, None, None)
        .await
        .unwrap();
    assert_eq!(before, inputs(dir).unwrap());
    assert!(!preview.days.is_empty());
}
