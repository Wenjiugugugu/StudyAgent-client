use std::path::{Path, PathBuf};
use studyagent_desktop_lib::{
    core::{review_sync, scheduler::DailyScheduler},
    data,
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "studyagent-safety-{}-{}",
            std::process::id(),
            data::new_progress_id("test", "fixture")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn sample_week(dir: &Path, date: &str) {
    let mut week = data::WeekPlanFile::default();
    week.meta.week_start = data::get_week_start(date).unwrap();
    week.meta.week_end = data::get_week_end(date).unwrap();
    week.data.days.push(data::WeekDayPlan {
        date: date.into(),
        subject_allocations: vec![data::DaySubjectAllocation {
            subject: data::SubjectKey::Math,
            hours: 1.0,
            task_templates: vec![data::TaskTemplate {
                title: "虚构知识点".into(),
                estimated_hours: 1.0,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    data::save_week_plan(dir, &week).unwrap();
}
#[test]
fn scheduler_preserves_corrupt_state_and_valid_backup() {
    let f = Fixture::new();
    let date = data::today_string();
    sample_week(&f.0, &date);
    let mut state = data::StudyState::default();
    state.progress.total_study_days = 42;
    data::save_state(&f.0, &state).unwrap();
    data::save_state(&f.0, &state).unwrap();
    let backup = std::fs::read(f.0.join("state/current.state.bak")).unwrap();
    std::fs::write(f.0.join("state/current.state"), "[invalid").unwrap();
    assert!(DailyScheduler::generate_daily_plan(&f.0, &date, true).is_err());
    assert_eq!(
        std::fs::read_to_string(f.0.join("state/current.state")).unwrap(),
        "[invalid"
    );
    assert_eq!(
        std::fs::read(f.0.join("state/current.state.bak")).unwrap(),
        backup
    );
}
#[test]
fn missing_state_is_initializable_but_unreadable_plan_is_not_empty() {
    let f = Fixture::new();
    assert_eq!(
        data::read_state_or_new(&f.0)
            .unwrap()
            .progress
            .total_study_days,
        0
    );
    let mut plan = data::DailyPlanFile::default();
    plan.meta.date = data::today_string();
    let path = data::daily_plan_path(&f.0, &plan.meta.date);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{invalid").unwrap();
    assert!(data::save_daily_plan(&f.0, &plan).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{invalid");
}
#[test]
fn progress_reader_never_repairs_or_overwrites_corruption() {
    let f = Fixture::new();
    let path = data::progress_index_path(&f.0);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{invalid").unwrap();
    assert!(data::read_progress_index(&f.0).is_err());
    assert!(data::load_progress_index(&f.0).subjects.is_empty());
    assert!(data::repair_progress_index_on_disk(&f.0).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{invalid");
}
#[test]
fn stale_revision_and_missing_revision_reject_before_overwrite() {
    let mut table = data::ProgressTable {
        name: "原表".into(),
        ..Default::default()
    };
    let revision = data::revision::content_revision(&table).unwrap();
    assert!(data::revision::check_revision(&table, Some(&revision)).is_ok());
    table.name = "后来修改".into();
    assert!(data::revision::check_revision(&table, Some(&revision)).is_err());
    assert!(data::revision::check_revision(&table, None).is_err());
    let dto = data::revision::with_revision(&table).unwrap();
    assert!(dto["revision"].is_string());
    assert!(serde_json::to_value(&table)
        .unwrap()
        .get("revision")
        .is_none());
}
#[test]
fn interrupted_file_transaction_recovers_and_keeps_newer_edit() {
    let f = Fixture::new();
    std::fs::write(f.0.join("a"), "old").unwrap();
    std::fs::create_dir(f.0.join("blocked")).unwrap();
    // Both candidates are read before journaling; unreadable target prevents any write.
    assert!(data::transaction::commit(
        &f.0,
        vec![("a".into(), "new".into()), ("blocked".into(), "new".into())]
    )
    .is_err());
    assert_eq!(std::fs::read_to_string(f.0.join("a")).unwrap(), "old");
    std::fs::create_dir_all(f.0.join("config")).unwrap();
    std::fs::write(
        f.0.join("config/pending-file-transaction.json"),
        r#"[{"path":"a","before":"old","after":"new"},{"path":"b","before":null,"after":"added"}]"#,
    )
    .unwrap();
    data::transaction::recover(&f.0).unwrap();
    assert_eq!(std::fs::read_to_string(f.0.join("a")).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(f.0.join("b")).unwrap(), "added");
    std::fs::write(
        f.0.join("config/pending-file-transaction.json"),
        r#"[{"path":"a","before":"old","after":"new"}]"#,
    )
    .unwrap();
    std::fs::write(f.0.join("a"), "later").unwrap();
    assert!(data::transaction::recover(&f.0).is_err());
    assert_eq!(std::fs::read_to_string(f.0.join("a")).unwrap(), "later");
}
#[test]
fn review_primary_survives_failed_sync_and_retry_is_idempotent() {
    let f = Fixture::new();
    let mut state = data::StudyState::default();
    let date = data::today_string();
    state.current_task.date = date.clone();
    state.current_task.tasks.push(data::StateTask {
        task_id: Some(format!("{date}-01")),
        task: "虚构知识点".into(),
        subject: "math".into(),
        accumulated_minutes: 37,
        ..Default::default()
    });
    data::save_state(&f.0, &state).unwrap();
    let mut review = data::ReviewFile::default();
    review.meta.date = date.clone();
    review.task_reviews.push(data::TaskReviewEntry {
        task_id: format!("{date}-01"),
        subject: "math".into(),
        title: "虚构知识点".into(),
        status: "completed".into(),
        ..Default::default()
    });
    let rev = review_sync::prepare(&f.0, &review).unwrap();
    data::save_review(&f.0, &review).unwrap();
    let primary = std::fs::read(data::records::review_file_path(&f.0, &date)).unwrap();
    std::fs::write(f.0.join("state/current.state"), "[invalid").unwrap();
    let pending = review_sync::retry(&f.0, &date, &rev).unwrap();
    assert_eq!(pending.sync_status, "pending");
    assert!(pending.pending_steps.contains(&"state".to_string()));
    assert!(review_sync::ensure_ready(&f.0).is_err());
    assert_eq!(
        std::fs::read(data::records::review_file_path(&f.0, &date)).unwrap(),
        primary
    );
    data::save_state(&f.0, &state).unwrap();
    let done = review_sync::retry(&f.0, &date, &rev).unwrap();
    assert_eq!(done.sync_status, "complete");
    review_sync::retry(&f.0, &date, &rev).unwrap();
    let after = data::read_state(&f.0).unwrap();
    assert_eq!(after.current_task.tasks[0].accumulated_minutes, 37);
    assert_eq!(after.current_task.tasks[0].status, data::TaskStatus::Done);
    assert_eq!(
        after
            .subjects
            .math
            .completed
            .iter()
            .filter(|s| s.as_str() == "虚构知识点")
            .count(),
        1
    );
}
#[test]
fn prepared_journal_does_not_submit_a_review_and_old_revision_cannot_retry() {
    let f = Fixture::new();
    let mut review = data::ReviewFile::default();
    let date = data::today_string();
    review.meta.date = date.clone();
    let rev = review_sync::prepare(&f.0, &review).unwrap();
    review_sync::recover(&f.0).unwrap();
    assert!(data::read_review(&f.0, &date).is_err());
    data::save_review(&f.0, &review).unwrap();
    review.view = Some("后来编辑".into());
    data::save_review(&f.0, &review).unwrap();
    assert!(review_sync::retry(&f.0, &date, &rev).is_err());
    assert_eq!(data::read_review(&f.0, &date).unwrap().view, review.view);
}

#[test]
fn unknown_energy_does_not_lower_the_known_energy_average() {
    let mut known = data::ReviewFile::default();
    known.data.energy_level = 5;
    let unknown = data::ReviewFile::default();
    assert_eq!(
        studyagent_desktop_lib::core::planner::today_intensity_label(&[known.clone()]),
        studyagent_desktop_lib::core::planner::today_intensity_label(&[known, unknown])
    );
}
#[test]
fn manual_transaction_does_not_write_a_plan_when_state_is_corrupt() {
    let f = Fixture::new();
    let mut state = data::StudyState::default();
    state.progress.total_study_days = 42;
    data::save_state(&f.0, &state).unwrap();
    std::fs::write(f.0.join("state/current.state"), "[invalid").unwrap();
    let mut plan = data::DailyPlanFile::default();
    plan.meta.date = data::today_string();
    assert!(data::save_manual_plan_and_state(&f.0, &plan).is_err());
    assert!(!data::daily_plan_path(&f.0, &plan.meta.date).exists());
    assert_eq!(
        std::fs::read_to_string(f.0.join("state/current.state")).unwrap(),
        "[invalid"
    );
}

#[test]
fn completion_counts_all_tasks_including_legacy_and_unclassified_rows() {
    let review = data::ReviewFile {
        task_reviews: vec![
            data::TaskReviewEntry {
                priority: "A".into(),
                status: "completed".into(),
                ..Default::default()
            },
            data::TaskReviewEntry {
                priority: "".into(),
                status: "partial".into(),
                completion: 0.5,
                ..Default::default()
            },
            data::TaskReviewEntry {
                priority: "B".into(),
                status: "incomplete".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(data::records::review_total_completion(&review), (1, 50.0));
    let (a, _, b, _, _) = data::records::review_completion_stats(&review);
    assert_eq!(a + b, 3);
}

#[test]
fn explicit_manual_deletion_is_not_restored_and_study_history_is_kept() {
    let f = Fixture::new();
    let date = data::today_string();
    sample_week(&f.0, &date);
    let mut plan = DailyScheduler::generate_daily_plan(&f.0, &date, false).unwrap();
    plan.data.tasks[0].source = data::TaskSource::Manual;
    data::save_manual_plan_and_state(&f.0, &plan).unwrap();
    let mut state = data::read_state(&f.0).unwrap();
    state.progress.total_study_days = 42;
    data::save_state(&f.0, &state).unwrap();
    plan.data.tasks.clear();
    data::recalc_totals(&mut plan.data);
    data::write_manual_plan_and_state(&f.0, &plan).unwrap();
    assert!(data::read_daily_plan(&f.0, &date)
        .unwrap()
        .data
        .tasks
        .is_empty());
    let after = data::read_state(&f.0).unwrap();
    assert!(after.current_task.tasks.is_empty());
    assert_eq!(after.progress.total_study_days, 42);
}

#[test]
fn retry_preserves_later_manual_progress_focus_and_timer_edits() {
    let f = Fixture::new();
    let date = data::today_string();
    let id = format!("{date}-01");
    let mut state = data::StudyState::default();
    state.current_task.date = date.clone();
    state.subjects.math.current_focus = "原重点".into();
    state.current_task.tasks.push(data::StateTask {
        task_id: Some(id.clone()),
        task: "虚构知识点".into(),
        subject: "math".into(),
        ..Default::default()
    });
    data::save_state(&f.0, &state).unwrap();
    let mut index = data::ProgressIndex::default();
    index.subjects.insert(
        "math".into(),
        data::SubjectProgressSet {
            active_id: "table".into(),
            tables: vec![data::ProgressTable {
                id: "table".into(),
                subject: "math".into(),
                nodes: vec![data::ProgressNode {
                    id: "node".into(),
                    title: "虚构知识点".into(),
                    level: data::NodeLevel::Knowledge,
                    status: data::NodeStatus::Learning,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    data::save_progress_index(&f.0, &index).unwrap();
    let mut review = data::ReviewFile::default();
    review.meta.date = date.clone();
    review.task_reviews.push(data::TaskReviewEntry {
        task_id: id,
        subject: "math".into(),
        title: "虚构知识点".into(),
        status: "completed".into(),
        completed_knowledge_points: Some(vec![data::KnowledgePointRef {
            table_id: "table".into(),
            node_id: "node".into(),
            title: "虚构知识点".into(),
        }]),
        ..Default::default()
    });
    review.overcompletion.push(data::OvercompletionEntry {
        subject: "math".into(),
        chapter_reached: "旧复盘进展".into(),
        ..Default::default()
    });
    let revision = review_sync::prepare(&f.0, &review).unwrap();
    data::save_review(&f.0, &review).unwrap();
    state.current_task.tasks[0].status = data::TaskStatus::Abandoned;
    state.current_task.tasks[0].accumulated_minutes = 67;
    state.current_task.tasks[0].started_at = Some("2026-10-13T09:00:00".into());
    state.subjects.math.current_focus = "后来学习重点".into();
    data::save_state(&f.0, &state).unwrap();
    index.subjects.get_mut("math").unwrap().tables[0].nodes[0].status = data::NodeStatus::Pending;
    data::save_progress_index(&f.0, &index).unwrap();
    assert_eq!(
        review_sync::retry(&f.0, &date, &revision)
            .unwrap()
            .sync_status,
        "complete"
    );
    let after = data::read_state(&f.0).unwrap();
    assert_eq!(
        after.current_task.tasks[0].status,
        data::TaskStatus::Abandoned
    );
    assert_eq!(after.current_task.tasks[0].accumulated_minutes, 67);
    assert_eq!(
        after.current_task.tasks[0].started_at,
        state.current_task.tasks[0].started_at
    );
    assert_eq!(after.subjects.math.current_focus, "后来学习重点");
    assert_eq!(
        data::read_progress_index(&f.0).unwrap().subjects["math"].tables[0].nodes[0].status,
        data::NodeStatus::Pending
    );
}

#[test]
fn failed_primary_save_keeps_previous_pending_journal_and_never_claims_new_record_saved() {
    let f = Fixture::new();
    let date = data::today_string();
    let mut old = data::ReviewFile::default();
    old.meta.date = date.clone();
    let old_revision = review_sync::prepare(&f.0, &old).unwrap();
    assert!(review_sync::status(&f.0, &date)
        .unwrap()
        .pending_steps
        .is_empty());
    data::save_review(&f.0, &old).unwrap();
    let mut new = old.clone();
    new.view = Some("未能写入的新版本".into());
    let new_revision = review_sync::prepare(&f.0, &new).unwrap();
    let blocked = data::records::review_file_path(&f.0, &date).with_extension("tmp");
    std::fs::create_dir(&blocked).unwrap();
    assert!(data::save_review(&f.0, &new).is_err());
    let status = review_sync::status(&f.0, &date).unwrap();
    assert_eq!(status.review_revision, old_revision);
    assert_ne!(status.review_revision, new_revision);
    assert!(!status.pending_steps.is_empty());
    std::fs::remove_dir(blocked).unwrap();
    review_sync::recover(&f.0).unwrap();
    assert_eq!(
        review_sync::status(&f.0, &date).unwrap().sync_status,
        "complete"
    );
    assert_eq!(data::read_review(&f.0, &date).unwrap().view, None);
}

#[test]
fn phase_promotion_is_retried_when_state_was_unreadable() {
    let f = Fixture::new();
    let date = data::today_string();
    let state = data::StudyState::default();
    data::save_state(&f.0, &state).unwrap();
    let mut index = data::ProgressIndex::default();
    index.subjects.insert(
        "math".into(),
        data::SubjectProgressSet {
            active_id: "table".into(),
            tables: vec![data::ProgressTable {
                id: "table".into(),
                subject: "math".into(),
                nodes: vec![data::ProgressNode {
                    id: "node".into(),
                    title: "虚构知识点".into(),
                    level: data::NodeLevel::Knowledge,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    data::save_progress_index(&f.0, &index).unwrap();
    let mut review = data::ReviewFile::default();
    review.meta.date = date.clone();
    review.task_reviews.push(data::TaskReviewEntry {
        subject: "math".into(),
        title: "虚构知识点".into(),
        status: "completed".into(),
        completed_knowledge_points: Some(vec![data::KnowledgePointRef {
            table_id: "table".into(),
            node_id: "node".into(),
            title: "虚构知识点".into(),
        }]),
        ..Default::default()
    });
    let rev = review_sync::prepare(&f.0, &review).unwrap();
    data::save_review(&f.0, &review).unwrap();
    std::fs::write(f.0.join("state/current.state"), "[invalid").unwrap();
    let pending = review_sync::retry(&f.0, &date, &rev).unwrap();
    assert!(pending.pending_steps.contains(&"phase".to_string()));
    data::save_state(&f.0, &state).unwrap();
    assert_eq!(
        review_sync::retry(&f.0, &date, &rev).unwrap().sync_status,
        "complete"
    );
    assert_eq!(
        data::read_state(&f.0).unwrap().subjects.math.phase,
        data::StudyPhase::Strengthen
    );
}

#[test]
fn unknown_energy_and_completion_are_shown_as_unknown_not_neutral_numbers() {
    let unknown = data::ReviewFile::default();
    let text = studyagent_desktop_lib::core::planner::today_intensity_label(&[unknown]);
    assert!(text.contains("精力均值 未记录"));
    assert!(text.contains("完成率均值 未记录"));
    assert!(!text.contains("3.0/5"));
    assert!(!text.contains("100%"));
}

#[test]
fn retry_does_not_complete_a_replacement_task_that_reuses_the_same_id() {
    let f = Fixture::new();
    let date = data::today_string();
    let id = format!("{date}-01");
    let mut state = data::StudyState::default();
    state.current_task.date = date.clone();
    state.current_task.tasks.push(data::StateTask {
        task_id: Some(id.clone()),
        task: "原任务".into(),
        subject: "math".into(),
        ..Default::default()
    });
    data::save_state(&f.0, &state).unwrap();
    let mut review = data::ReviewFile::default();
    review.meta.date = date.clone();
    review.task_reviews.push(data::TaskReviewEntry {
        task_id: id,
        subject: "math".into(),
        title: "原任务".into(),
        status: "completed".into(),
        ..Default::default()
    });
    let revision = review_sync::prepare(&f.0, &review).unwrap();
    data::save_review(&f.0, &review).unwrap();
    state.current_task.tasks[0].task = "后来新增的任务".into();
    data::save_state(&f.0, &state).unwrap();
    review_sync::retry(&f.0, &date, &revision).unwrap();
    let after = data::read_state(&f.0).unwrap();
    assert_eq!(
        after.current_task.tasks[0].status,
        data::TaskStatus::Pending
    );
    assert_eq!(after.current_task.tasks[0].task, "后来新增的任务");
}

#[test]
fn scheduler_does_not_transfer_timing_between_subjects_that_reuse_an_id_and_title() {
    let f = Fixture::new();
    let date = data::today_string();
    sample_week(&f.0, &date);
    let mut state = data::StudyState::default();
    state.current_task.date = date.clone();
    state.current_task.tasks.push(data::StateTask {
        task_id: Some(format!("{date}-01")),
        task: "虚构知识点".into(),
        subject: "english".into(),
        accumulated_minutes: 99,
        ..Default::default()
    });
    data::save_state(&f.0, &state).unwrap();
    DailyScheduler::generate_daily_plan(&f.0, &date, true).unwrap();
    let after = data::read_state(&f.0).unwrap();
    assert_eq!(after.current_task.tasks[0].subject, "math");
    assert_eq!(after.current_task.tasks[0].accumulated_minutes, 0);
}
