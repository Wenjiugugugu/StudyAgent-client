//! StudyAgent Desktop — Tauri 应用入口
//!
//! 本文件是 Tauri 2 桌面应用的入口点，负责：
//! 1. 初始化日志系统
//! 2. 初始化 AppState（数据目录、AI Service、Tool Dispatcher）
//! 3. 注册 Tauri 插件（fs、dialog、shell、store）
//! 4. 注册所有 Tauri 命令
//! 5. 启动应用
//!
//! 前端通过 `@tauri-apps/api` 的 `invoke` 函数调用注册的命令。

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use tauri::Manager;
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
        .manage(app_state)
        .setup(|app| {
            log::info!("Tauri 应用已启动");

            // 获取主窗口
            if let Some(window) = app.get_webview_window("main") {
                let title = window.title().unwrap_or_default();
                log::info!("主窗口: {}", title);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Dashboard
            get_dashboard_summary,
            // State
            get_state,
            update_task_status,
            update_subject_textbook,
            // Plan
            get_today_plan,
            get_plan_by_date,
            get_week_plan,
            list_plan_dates,
            list_plan_summaries,
            get_week_summaries,
            generate_daily_plan,
            generate_week_plan,
            // Review
            get_review,
            list_review_dates,
            generate_review,
            submit_review,
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
            // User Model
            get_capabilities,
            get_observations,
            get_user_model_summary,
            // AI 对话
            chat,
            chat_stream,
            test_ai_provider,
            list_ai_models,
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
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时发生错误");
}
