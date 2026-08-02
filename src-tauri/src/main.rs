//! StudyAgent Desktop — Tauri 应用入口
//!
//! 本文件是 Tauri 2 桌面应用的入口点，负责：
//! 1. 初始化日志系统
//! 2. 初始化 AppState（数据目录、AI Service、Tool Dispatcher）
//! 3. 注册 Tauri 插件（fs、dialog、shell、store、notification、autostart）
//! 4. 注册所有 Tauri 命令
//! 5. 配置系统托盘图标与菜单
//! 6. 配置窗口关闭行为（最小化到托盘或退出）
//! 7. 启动应用
//!
//! 前端通过 `@tauri-apps/api` 的 `invoke` 函数调用注册的命令。

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use studyagent_desktop_lib::init_app_state;
use studyagent_desktop_lib::api::commands::*;
use studyagent_desktop_lib::get_default_data_dir;

fn main() {
    // 初始化日志
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    log::info!("StudyAgent Desktop 启动中...");

    // 默认数据目录（开发模式自动定位项目根目录，生产模式使用 exe 同级 data/）
    let default_data_dir = get_default_data_dir();

    // 检查数据目录是否存在
    if !default_data_dir.exists() {
        log::warn!(
            "数据目录不存在: {:?}, 将在启动时自动创建",
            default_data_dir
        );
    }

    // 初始化 AppState
    let app_state = init_app_state(default_data_dir);

    log::info!("AppState 初始化完成");

    // 构建 Tauri 应用
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .manage(app_state)
        .setup(|app| {
            log::info!("Tauri 应用已启动");

            // 获取主窗口
            if let Some(window) = app.get_webview_window("main") {
                let title = window.title().unwrap_or_default();
                log::info!("主窗口: {}", title);
            }

            // 构建系统托盘菜单
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 StudyAgent", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            // 构建系统托盘图标
            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("StudyAgent")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        log::info!("用户从托盘菜单退出应用");
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 双击托盘图标显示主窗口
                    if let TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // 拦截关闭事件，根据 close_action 设置决定行为
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.try_state::<std::sync::Mutex<studyagent_desktop_lib::AppState>>();
                let close_action = state
                    .and_then(|s| {
                        let data_dir = studyagent_desktop_lib::get_data_dir(s.inner()).ok()?;
                        let settings = studyagent_desktop_lib::load_settings(&data_dir);
                        Some(settings.close_action)
                    })
                    .unwrap_or_else(|| "ask".to_string());

                match close_action.as_str() {
                    "tray" => {
                        // 最小化到托盘：阻止默认关闭，隐藏窗口
                        log::info!("close_action=tray，隐藏窗口到系统托盘");
                        let _ = window.hide();
                        api.prevent_close();

                        // 通过事件通知前端（用于显示一次「已最小化到托盘」提示）
                        let _ = window.app_handle().emit("window-minimized-to-tray", ());
                    }
                    "quit" => {
                        // 直接退出
                        log::info!("close_action=quit，直接退出应用");
                    }
                    _ => {
                        // "ask"：交给前端处理（前端会显示对话框并调用 prevent_close 通过 hide）
                        // 这里通过事件通知前端，由前端决定
                        log::info!("close_action=ask，转发到前端处理");
                        let _ = window.app_handle().emit("close-requested", ());
                        api.prevent_close();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Dashboard
            get_dashboard_summary,
            // State
            get_state,
            update_task_status,
            update_subject_textbook,
            start_task_timer,
            pause_task_timer,
            get_task_total_minutes,
            // Plan
            get_today_plan,
            get_plan_by_date,
            get_week_plan,
            list_plan_dates,
            list_plan_summaries,
            get_week_summaries,
            generate_daily_plan,
            generate_week_plan,
            // Analytics
            get_analytics,
            // Review
            get_review,
            list_review_dates,
            generate_review,
            submit_review,
            regenerate_remaining_days,
            // Knowledge
            list_knowledge,
            get_knowledge,
            search_knowledge,
            get_knowledge_graph,
            // Textbook
            list_textbooks,
            read_textbook,
            import_textbook,
            delete_textbook,
            rename_textbook,
            search_in_textbook,
            // Background image
            save_background_image,
            delete_background_image,
            read_background_as_data_url,
            // User Model
            get_capabilities,
            get_observations,
            get_user_model_summary,
            // AI 对话
            chat,
            chat_stream,
            test_ai_provider,
            list_ai_models,
            // AI 用量日志
            get_ai_usage_log,
            clear_ai_usage_log,
            // MCP / Tool
            list_mcp_servers,
            call_tool,
            // Settings
            get_settings,
            save_settings,
            change_data_directory,
            // Onboarding
            get_onboarding_status,
            complete_onboarding,
            init_state,
            // Update
            check_for_updates,
            download_update,
            install_update,
            // 通用：关闭动作 / 开机启动 / 应用版本
            get_close_action,
            set_close_action,
            quit_app,
            get_autostart,
            set_autostart,
            get_app_version,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时发生错误");
}
