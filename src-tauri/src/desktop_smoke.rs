//! 只在 desktop-smoke 构建中启用，数据限于带标记的独立临时目录。
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use studyagent_desktop_lib::{data, AppSettings};
use tauri::Listener;

pub fn directory() -> Result<Option<PathBuf>, String> {
    let args = std::env::args().collect::<Vec<_>>();
    let Some(index) = args.iter().position(|arg| arg == "--desktop-smoke-root") else {
        return Ok(None);
    };
    let root = PathBuf::from(args.get(index + 1).ok_or("缺少隔离目录")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let temp = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if root == temp || !root.starts_with(&temp) || !root.join(".studyagent-smoke-root").is_file() {
        return Err("桌面测试必须使用带标记的独立临时目录".into());
    }
    if std::fs::read_dir(&root).map_err(|e| e.to_string())?.count() != 1 {
        return Err("测试目录必须为空（除隔离标记）".into());
    }
    let mut state = data::StudyState::default();
    state.progress.total_study_days = 42;
    state.meta.exam_date = "2026-12-20".into();
    data::save_state(&root, &state)?;
    let settings = AppSettings {
        data_dir: root.to_string_lossy().into(),
        onboarding_completed: true,
        ..Default::default()
    };
    studyagent_desktop_lib::save_settings_file(&root, &settings)?;
    let mut plan = data::DailyPlanFile::default();
    plan.meta.date = data::today_string();
    data::save_daily_plan(&root, &plan)?;
    #[cfg(windows)]
    let root = PathBuf::from(root.to_string_lossy().trim_start_matches(r"\\?\"));
    Ok(Some(root))
}

pub fn install(app: &mut tauri::App, root: PathBuf) -> tauri::Result<()> {
    let received = Arc::new(AtomicBool::new(false));
    let signal = received.clone();
    let report_dir = root.clone();
    let handle = app.handle().clone();
    app.listen("studyagent-desktop-smoke", move |event| {
        if signal.swap(true, Ordering::SeqCst) {
            return;
        }
        let value = serde_json::from_str::<serde_json::Value>(event.payload()).unwrap_or_default();
        let valid = value["mounted"] == true
            && value["state"] == true
            && value["plan"] == true
            && value["write_read"] == true;
        let result = serde_json::json!({"ok":valid,"frontend":value});
        let written = data::atomic_write(
            &report_dir.join("desktop-smoke-result.json"),
            &result.to_string(),
        )
        .is_ok();
        handle.exit(if valid && written { 0 } else { 1 });
    });
    let timeout_handle = app.handle().clone();
    let timeout_root = root.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        if !received.swap(true, Ordering::SeqCst) {
            let _ = data::atomic_write(
                &timeout_root.join("desktop-smoke-result.json"),
                r#"{"ok":false,"error":"frontend readiness timeout"}"#,
            );
            timeout_handle.exit(1);
        }
    });
    data::atomic_write(&root.join("desktop-smoke-stage.txt"), "creating webview")
        .map_err(std::io::Error::other)?;
    tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
        .data_directory(root.join("webview-profile"))
        .build()?;
    data::atomic_write(&root.join("desktop-smoke-stage.txt"), "webview created")
        .map_err(std::io::Error::other)?;
    Ok(())
}
