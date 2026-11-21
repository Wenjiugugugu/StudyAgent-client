use studyagent_desktop_lib::{core::scheduler::DailyScheduler, data};

fn main() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("studyagent-debt-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = probe(&dir);
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    result
}

fn probe(dir: &std::path::Path) -> Result<(), String> {
    let date = data::today_string();
    let mut week = data::WeekPlanFile::default();
    week.meta.week_start = data::get_week_start(&date)?;
    week.meta.week_end = data::get_week_end(&date)?;
    week.data.days.push(data::WeekDayPlan {
        date: date.clone(),
        subject_allocations: vec![data::DaySubjectAllocation {
            subject: data::SubjectKey::Math,
            hours: 1.0,
            task_templates: vec![data::TaskTemplate {
                title: "审查用虚构数学任务".into(),
                estimated_hours: 1.0,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    data::save_week_plan(dir, &week)?;
    let mut original = data::StudyState::default();
    original.progress.total_study_days = 42;
    data::save_state(dir, &original)?;
    data::save_state(dir, &original)?;
    let path = dir.join("state/current.state");
    std::fs::write(&path, "[invalid").map_err(|e| e.to_string())?;
    let result = DailyScheduler::generate_daily_plan(dir, &date, true);
    let after = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    println!("scheduler_success={} corrupt_state_preserved={} task_count={} study_days={:?}",
        result.is_ok(), after == "[invalid", result.as_ref().map(|p| p.data.tasks.len()).unwrap_or(0),
        data::read_state(dir).map(|s| s.progress.total_study_days));
    assert!(result.is_err());
    assert_eq!(after, "[invalid", "损坏状态必须保留，不能初始化为默认状态");
    Ok(())
}
