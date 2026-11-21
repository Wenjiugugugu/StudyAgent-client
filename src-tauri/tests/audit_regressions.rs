use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use studyagent_desktop_lib::{
    ai::{
        provider::{AIProviderConfig, AgentType, ChatMessage, ChatRequest},
        service::AiService,
    },
    core::planner::Planner,
    data, AppSettings,
};

fn server(status: u16, content: String, delay: u64) -> (String, Arc<AtomicBool>) {
    server_steps(vec![(status, content, delay)])
}
fn server_steps(steps: Vec<(u16, String, u64)>) -> (String, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(AtomicBool::new(false));
    let signal = received.clone();
    std::thread::spawn(move || {
        let count = steps.len();
        for (i, (status, content, delay)) in steps.into_iter().enumerate() {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => return,
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut raw = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&raw[..end]);
                    let size = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|s| s.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if raw.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            if i + 1 == count {
                signal.store(true, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(delay));
            let body = if status == 200 {
                serde_json::json!({"id":"mock","model":"mock","choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string()
            } else {
                "mock unauthorized".into()
            };
            let header = format!("HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(format!("{header}{body}").as_bytes());
        }
    });
    (url, received)
}
fn config(id: &str, url: String, default: bool) -> AIProviderConfig {
    AIProviderConfig {
        id: id.into(),
        name: id.into(),
        base_url: url,
        model: "mock".into(),
        enabled: true,
        is_default: default,
        timeout: 30,
        ..Default::default()
    }
}

struct PlanningFixture {
    dir: std::path::PathBuf,
    today: String,
    start: String,
    state: data::StudyState,
    settings: AppSettings,
    week: data::WeekPlanFile,
}
fn planning_fixture(root: &std::path::Path) -> Result<PlanningFixture, String> {
    let dir = root.join("week");
    let today = data::today_string();
    let start = data::get_week_start(&today)?;
    let mut state = data::StudyState::default();
    state.subjects.math.active = true;
    state.meta.exam_date = "2026-12-20".into();
    data::save_state(&dir, &state)?;
    let settings = AppSettings {
        study_schedule: serde_json::json!({"daily_target_hours":5,"daily_task_count":3,"study_days_per_week":7,"rest_days":[],"enable_review_tasks":false}),
        ..Default::default()
    };
    studyagent_desktop_lib::save_settings_file(&dir, &settings)?;
    let mut week = data::WeekPlanFile {
        version: "1.0.0".into(),
        ..Default::default()
    };
    week.meta.week_start = start.clone();
    week.meta.week_end = data::get_week_end(&today)?;
    for n in 0..7 {
        week.data.days.push(data::WeekDayPlan {
            date: data::add_days(&start, n)?,
            subject_allocations: vec![data::DaySubjectAllocation {
                subject: data::SubjectKey::Math,
                hours: 1.0,
                task_templates: vec![data::TaskTemplate {
                    title: "probe math".into(),
                    estimated_hours: 1.0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    Ok(PlanningFixture {
        dir,
        today,
        start,
        state,
        settings,
        week,
    })
}
struct TestRoot(std::path::PathBuf);
impl TestRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "studyagent-regressions-{}-{}",
            std::process::id(),
            data::new_progress_id("root", "test")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn corrupt_state_and_backup_are_preserved() -> Result<(), String> {
    let fixture = TestRoot::new();
    let root = &fixture.0;
    // R01/R02: preserve the original findings as repeatable direct calls to current data code.
    let dir = root.join("corrupt-state");
    let mut original = data::StudyState::default();
    original.progress.total_study_days = 42;
    original.current_task.date = data::today_string();
    data::save_state(&dir, &original)?;
    data::save_state(&dir, &original)?;
    std::fs::write(dir.join("state/current.state"), "[invalid").map_err(|e| e.to_string())?;
    let mut daily = data::DailyPlanFile::default();
    daily.meta.date = data::today_string();
    data::save_daily_plan(&dir, &daily)?;
    let result = data::read_daily_plan_with_merged_status(&dir, &daily.meta.date);
    let backup =
        std::fs::read_to_string(dir.join("state/current.state.bak")).map_err(|e| e.to_string())?;
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("state/current.state")).unwrap(),
        "[invalid"
    );
    assert_eq!(
        data::parse_state_toml(&backup)?.progress.total_study_days,
        42
    );
    data::save_state(&dir, &original)?;
    assert_eq!(
        std::fs::read_to_string(dir.join("state/current.state.bak")).unwrap(),
        backup
    );
    let new_dir = root.join("first-run");
    data::save_daily_plan(&new_dir, &daily)?;
    data::read_daily_plan_with_merged_status(&new_dir, &daily.meta.date)?;
    assert!(data::read_state(&new_dir).is_ok());
    Ok(())
}

#[tokio::test]
async fn task_identity_mismatch_is_rejected() -> Result<(), String> {
    let today = data::today_string();
    let mut state = data::StudyState::default();
    state.current_task.date = today.clone();
    state.current_task.tasks = vec![data::StateTask {
        task_id: Some(format!("{today}-02")),
        task: "another task".into(),
        ..Default::default()
    }];
    let result = data::update_task_status_by_id(
        &mut state,
        &format!("{today}-01"),
        0,
        data::TaskStatus::Done,
    );
    assert!(result.is_err());
    assert_eq!(
        state.current_task.tasks[0].status,
        data::TaskStatus::Pending
    );
    state.current_task.tasks[0].task_id = None;
    data::update_task_status_by_id(
        &mut state,
        &format!("{today}-01"),
        0,
        data::TaskStatus::Done,
    )?;
    assert_eq!(
        state.current_task.tasks[0].task_id.as_deref(),
        Some(format!("{today}-01").as_str())
    );
    Ok(())
}

#[tokio::test]
async fn backup_provider_request_remains_cancellable() -> Result<(), String> {
    // R03: primary rejects, backup waits; cancellation is attempted while backup is active.
    let (primary, _) = server(401, String::new(), 0);
    let (backup, received) = server(200, "backup result".into(), 1000);
    let service = Arc::new(AiService::from_configs(
        vec![
            config("primary", primary, true),
            config("backup", backup, false),
        ],
        HashMap::new(),
    ));
    let running = service.clone();
    let task = tokio::spawn(async move {
        running
            .chat(ChatRequest {
                agent: Some(AgentType::Planner),
                messages: vec![ChatMessage {
                    content: "probe".into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !received.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    let found = service.cancel_request("planner");
    let response = task.await.map_err(|e| e.to_string())?;
    assert!(found);
    assert!(response
        .as_ref()
        .unwrap_err()
        .contains(studyagent_desktop_lib::ai::provider::REQUEST_CANCELLED));
    assert!(!service.cancel_request("planner"));
    println!(
        "R03 cancel_found={found} backup_returned_success={}",
        response.is_ok()
    );

    Ok(())
}

#[tokio::test]
async fn failed_daily_publication_is_reported() -> Result<(), String> {
    let fixture = TestRoot::new();
    let root = &fixture.0;
    let PlanningFixture {
        dir,
        today,
        start,
        week,
        ..
    } = planning_fixture(root)?;
    let blocked = data::daily_plan_path(&dir, &today);
    std::fs::create_dir_all(&blocked).map_err(|e| e.to_string())?;
    let (url, _) = server(200, serde_json::to_string(&week).unwrap(), 0);
    let service = AiService::from_configs(vec![config("week", url, true)], HashMap::new());
    let result = Planner::new(&service)
        .generate_week_plan(&dir, &start, &[], None)
        .await;
    assert!(result.as_ref().unwrap_err().contains("部分日计划未生成"));
    assert!(blocked.is_dir());
    println!(
        "R10 overall_success={} today_plan_is_directory={} result_error={:?}",
        result.is_ok(),
        blocked.is_dir(),
        result.err()
    );

    Ok(())
}

#[tokio::test]
async fn historical_review_preview_cannot_change_old_week() -> Result<(), String> {
    let fixture = TestRoot::new();
    let root = &fixture.0;
    let PlanningFixture {
        state,
        settings,
        week,
        start,
        ..
    } = planning_fixture(root)?;
    // R09: use a real previous calendar week, a Monday review and a Wednesday direction.
    // Invoke the public preview generator rather than just the scheduler comparison.
    let dir = root.join("old-preview");
    data::save_state(&dir, &state)?;
    studyagent_desktop_lib::save_settings_file(&dir, &settings)?;
    let source = data::add_days(&start, -7)?;
    let mut old_week = week.clone();
    old_week.meta.week_start = source.clone();
    old_week.meta.week_end = data::add_days(&source, 6)?;
    for (i, day) in old_week.data.days.iter_mut().enumerate() {
        day.date = data::add_days(&source, i as i64)?;
    }
    data::save_week_plan(&dir, &old_week)?;
    let mut review = data::ReviewFile::default();
    review.meta.date = source.clone();
    review.meta.generated_at = data::study_direction::timestamp();
    review.task_reviews = vec![data::TaskReviewEntry {
        task_id: format!("{source}-01"),
        subject: "math".into(),
        title: "probe math".into(),
        status: "incomplete".into(),
        ..Default::default()
    }];
    data::save_review(&dir, &review)?;
    let mut direction = data::ReviewFile::default();
    direction.meta.date = data::add_days(&source, 2)?;
    direction.meta.generated_at = data::study_direction::timestamp();
    direction.actual_study = vec![data::study_direction::ActualStudyEntry {
        subject: "math".into(),
        content: "probe math".into(),
        scope: data::study_direction::StudyScope::Ongoing,
        ..Default::default()
    }];
    data::save_review(&dir, &direction)?;
    let service = AiService::new();
    let result = studyagent_desktop_lib::core::plan_adjustment::generate(
        &dir,
        &Planner::new(&service),
        "review",
        &source,
        None,
        None,
    )
    .await;
    assert!(result.as_ref().unwrap().affected_dates.is_empty());

    Ok(())
}

#[tokio::test]
async fn preview_publication_detects_edits_while_ai_runs() -> Result<(), String> {
    let fixture = TestRoot::new();
    let root = &fixture.0;
    let PlanningFixture {
        state,
        settings,
        week,
        today,
        ..
    } = planning_fixture(root)?;
    let mut direction = data::ReviewFile::default();
    direction.meta.generated_at = data::study_direction::timestamp();
    direction.actual_study = vec![data::study_direction::ActualStudyEntry {
        subject: "math".into(),
        content: "probe math".into(),
        scope: data::study_direction::StudyScope::Ongoing,
        ..Default::default()
    }];
    // R08: observe the exact publication lock while the second AI validation request waits.
    let dir = root.join("locked-preview");
    data::save_state(&dir, &state)?;
    studyagent_desktop_lib::save_settings_file(&dir, &settings)?;
    data::save_week_plan(&dir, &week)?;
    direction.meta.date = today.clone();
    data::save_review(&dir, &direction)?;
    let (url, received) = server_steps(vec![
        (200, serde_json::to_string(&week).unwrap(), 0),
        (
            200,
            r#"{"checks":[{"subject":"math","satisfied":true,"reason":"probe"}]}"#.into(),
            1000,
        ),
    ]);
    let service = Arc::new(AiService::from_configs(
        vec![config("lock-preview", url, true)],
        HashMap::new(),
    ));
    let io_lock = Arc::new(tokio::sync::Mutex::new(()));
    let running_lock = io_lock.clone();
    let editable_dir = dir.clone();
    let task = tokio::spawn(async move {
        studyagent_desktop_lib::core::plan_adjustment::generate(
            &dir,
            &Planner::new(&service),
            "review",
            &today,
            None,
            Some(&running_lock),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !received.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    let blocked = tokio::time::timeout(Duration::from_millis(100), io_lock.lock())
        .await
        .is_err();
    assert!(!blocked);
    {
        let _guard = io_lock.lock().await;
        let mut live = data::read_state(&editable_dir)?;
        live.progress.total_study_days += 1;
        data::save_state(&editable_dir, &live)?;
    }
    let result = task.await.map_err(|e| e.to_string())?;
    assert!(result.as_ref().unwrap_err().contains("学习数据发生变化"));
    println!(
        "R08 io_lock_blocked_during_validation={blocked} preview_success={} error={:?}",
        result.is_ok(),
        result.err()
    );

    Ok(())
}

#[tokio::test]
async fn startup_loads_effective_directory_and_provider() -> Result<(), String> {
    let fixture = TestRoot::new();
    let root = &fixture.0;
    // R05: actual startup uses old entry settings even when a different effective directory exists.
    let boot = root.join("bootstrap");
    let custom = root.join("custom");
    std::fs::create_dir_all(&custom).map_err(|e| e.to_string())?;
    let entry = AppSettings {
        data_dir: boot.to_string_lossy().into(),
        ai_providers: vec![config("boot-a", "http://127.0.0.1:9".into(), true)],
        ..Default::default()
    };
    studyagent_desktop_lib::save_settings_file(&boot, &entry)?;
    studyagent_desktop_lib::save_data_directory_change(&boot, &custom, &entry)?;
    let mut latest = entry.clone();
    latest.data_dir = custom.to_string_lossy().into();
    latest.ai_providers = vec![config("boot-b", "http://127.0.0.1:9".into(), true)];
    studyagent_desktop_lib::save_settings_file(&custom, &latest)?;
    let boot_clone = boot.clone();
    let app =
        tokio::task::spawn_blocking(move || studyagent_desktop_lib::init_app_state(boot_clone))
            .await
            .map_err(|e| e.to_string())?;
    {
        let app = app.lock().unwrap();
        assert_eq!(app.data_dir, custom);
        assert_eq!(app.ai_service.default_provider_id(), "boot-b");
        println!(
            "R05 effective_dir_matches_custom={} startup_provider={} expected_provider=boot-b",
            app.data_dir == custom,
            app.ai_service.default_provider_id()
        );
    }
    let second = root.join("second-custom");
    std::fs::create_dir_all(&second).map_err(|e| e.to_string())?;
    studyagent_desktop_lib::save_data_directory_change(&boot, &second, &latest)?;
    let app = tokio::task::spawn_blocking(move || studyagent_desktop_lib::init_app_state(boot))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(app.lock().unwrap().data_dir, second);
    Ok(())
}
