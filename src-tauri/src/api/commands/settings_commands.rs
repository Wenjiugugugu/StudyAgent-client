#![allow(unused_imports)]
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::analytics::{build_analytics, AnalyticsRange, AnalyticsSummary};
use crate::core::briefing::{yesterday_of, BriefingAgent};
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{UserCapability, UserObservation};
use crate::data::plan::{
    iso_week_string, DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment,
};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::tools::dispatcher::{execute_builtin_tool, is_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    get_ai_service, get_data_dir, get_data_dir_and_ai, get_data_dir_and_dispatcher,
    get_tool_dispatcher, load_settings, reinitialize_services, save_settings_file, AppSettings,
    AppState,
};

use super::legacy::*;

/// 保存用户选择的背景图到 data_dir/assets/backgrounds/
///
/// 将用户通过对话框选择的图片复制到应用数据目录，返回相对于 data_dir 的路径
/// （如 "assets/backgrounds/xxx.png"），供前端通过 convertFileSrc 加载。
/// 前端调用: `invoke('save_background_image', { filePath: 'C:/...' })`
#[tauri::command]
pub async fn save_background_image(
    file_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let dir = get_data_dir(state.inner())?;
    let backgrounds_dir = dir.join("assets").join("backgrounds");
    std::fs::create_dir_all(&backgrounds_dir).map_err(|e| format!("创建背景图目录失败: {}", e))?;

    let src_path = std::path::Path::new(&file_path);
    let extension = src_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());

    // 用时间戳生成唯一文件名，避免覆盖
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest_filename = format!("bg_{}.{}", timestamp, extension);
    let dest_path = backgrounds_dir.join(&dest_filename);

    std::fs::copy(src_path, &dest_path).map_err(|e| format!("复制背景图失败: {}", e))?;

    // 返回相对路径（使用 / 作为分隔符，便于跨平台拼接）
    let relative = format!("assets/backgrounds/{}", dest_filename);
    Ok(relative)
}

/// 删除已保存的背景图文件
///
/// 前端调用: `invoke('delete_background_image', { relativePath: 'assets/backgrounds/xxx.png' })`
#[tauri::command]
pub async fn delete_background_image(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let dir = get_data_dir(state.inner())?;
    let full_path = resolve_relative_path(&dir, &relative_path)?;
    if full_path.exists() {
        std::fs::remove_file(&full_path).map_err(|e| format!("删除背景图失败: {}", e))?;
    }
    Ok(())
}

/// 读取背景图文件并返回 base64 data URL
///
/// 由于 Tauri v2 的 assetProtocol 需要 scope 配置，直接返回 data URL 更简单可靠。
/// 前端调用: `invoke('read_background_as_data_url', { relativePath: 'assets/backgrounds/xxx.png' })`
#[tauri::command]
pub async fn read_background_as_data_url(
    relative_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let dir = get_data_dir(state.inner())?;
    let full_path = resolve_relative_path(&dir, &relative_path)?;
    if !full_path.exists() {
        return Err(format!("背景图文件不存在: {}", full_path.display()));
    }

    // 读取文件字节
    let bytes = std::fs::read(&full_path).map_err(|e| format!("读取背景图失败: {}", e))?;

    // 根据扩展名推断 MIME 类型
    let extension = full_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    };

    // 编码为 base64
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

/// 获取应用配置
///
/// 读取 `config/settings.json` 文件。
/// 前端调用: `invoke('get_settings')`
#[tauri::command]
pub async fn get_settings(state: State<'_, Mutex<AppState>>) -> Result<AppSettings, String> {
    let data_dir = get_data_dir(state.inner())?;
    let mut settings = load_settings(&data_dir);
    // 前端只需要知道密钥是否已配置，不应接收可直接复用的明文。
    for provider in settings.ai_providers.iter_mut() {
        provider.api_key = if provider.api_key.is_empty() {
            String::new()
        } else {
            crate::secrets::CONFIGURED_SENTINEL.to_string()
        };
    }
    Ok(settings)
}

/// 保存应用配置
///
/// 保存到 `config/settings.json` 并重新初始化 AI Service 和 Tool Dispatcher。
/// 前端调用: `invoke('save_settings', { settings: { ... } })`
#[tauri::command]
pub async fn save_settings(
    settings: AppSettings,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    log::info!(
        "[save_settings] entered, sidebar_style={}",
        settings.sidebar_style
    );
    // H1 并发保护：串行化 settings 写入与服务重建
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;
    reinitialize_services(state.inner(), settings).await
}

/// 切换数据目录（重启后生效）
///
/// 流程：
/// 1. 校验 new_path 存在且是目录
/// 2. 在新目录下创建必要的子目录结构
/// 3. 把当前 settings.json 复制到新目录（保留 data_dir 字段为新路径）
/// 4. 更新 AppState.data_dir 为新路径，使后续读写立即指向新目录
/// 5. 注意：旧目录中的历史 plan/state/review 等文件不会自动迁移，需用户手动处理
#[tauri::command]
pub async fn change_data_directory(
    new_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    use std::path::PathBuf;

    // H1 并发保护：切换数据目录期间串行化所有写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let new_dir = PathBuf::from(new_path.trim_end_matches(['/', '\\']));
    if !new_dir.is_dir() {
        return Err(format!("目录不存在或不是目录: {:?}", new_dir));
    }

    // 读取当前 settings
    let (old_data_dir, current_settings) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let data_dir = s.data_dir.clone();
        let settings = crate::load_settings(&data_dir);
        (data_dir, settings)
    };

    // 在新目录下创建子结构
    crate::ensure_data_directories(&new_dir);

    // 构造新 settings：data_dir 指向新路径
    let mut new_settings = current_settings.clone();
    new_settings.data_dir = new_dir.to_string_lossy().to_string();

    // 把新 settings 写到新目录的 config/settings.json
    crate::save_settings_file(&new_dir, &new_settings)?;

    // 更新 AppState.data_dir，让后续命令立即使用新目录
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir = new_dir.clone();
    }

    // H17：同步更新 AI 用量日志目录，使日志写入新数据目录
    crate::data::ai_usage::set_log_dir(new_dir.clone());

    let msg = format!(
        "数据目录已切换至 {:?}。旧目录 {:?} 中的历史数据未自动迁移，如需保留历史计划/复盘记录，请手动复制 state/、plan/、records/、assets/ 等子目录到新目录。重启应用后配置仍然生效。",
        new_dir, old_data_dir
    );
    log::info!("{}", msg);
    Ok(msg)
}

/// 导出数据备份（zip）
///
/// 把数据目录下允许的子目录（state/plan/records/config/assets，可选 logs/）
/// 压缩到 `dest_path` 指定的 zip 文件。
/// 返回导出的文件数。
///
/// 前端调用: `invoke('export_backup', { destPath, includeLogs })`
#[tauri::command]
pub async fn export_backup(
    dest_path: String,
    include_logs: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<usize, String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：导出期间不允许写入，保证快照一致
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let dest = std::path::PathBuf::from(dest_path.trim_end_matches(['/', '\\']));
    let count = crate::data::backup::export_backup(&data_dir, &dest, include_logs)?;
    log::info!("数据备份导出完成: {} 个文件 -> {:?}", count, dest);
    Ok(count)
}

/// 导入数据备份（zip），覆盖前自动备份现有数据目录
///
/// 校验 zip 合法性后，把现有数据目录重命名为 `{data_dir}-bak-{timestamp}`，
/// 再解压备份内容到数据目录。导入完成后需重启应用以加载最新数据。
///
/// 前端调用: `invoke('import_backup', { filePath })`
#[tauri::command]
pub async fn import_backup(
    file_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::data::backup::ImportSummary, String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：导入覆盖期间串行化所有写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let zip_path = std::path::PathBuf::from(file_path.trim_end_matches(['/', '\\']));
    let summary = crate::data::backup::import_backup(&data_dir, &zip_path)?;
    log::info!(
        "数据备份导入完成: 恢复 {} 个文件, 原数据备份至 {}",
        summary.files_restored,
        summary.backup_dir
    );
    Ok(summary)
}

/// 初始化 State 文件
///
/// 在引导流程完成时调用，根据用户填写的目标院校、考试科目、当前进度
/// 创建 `state/current.state` 文件。
#[tauri::command]
pub async fn init_state(
    payload: InitStatePayload,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 初始化写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let today = crate::data::today_string();
    let phase_map: std::collections::HashMap<&str, crate::data::state::StudyPhase> = [
        ("foundation", crate::data::state::StudyPhase::Foundation),
        ("strengthen", crate::data::state::StudyPhase::Strengthen),
        ("sprint", crate::data::state::StudyPhase::Sprint),
        ("mock", crate::data::state::StudyPhase::Mock),
        ("complete", crate::data::state::StudyPhase::Complete),
    ]
    .into_iter()
    .collect();

    let mut study_state = crate::data::state::StudyState {
        meta: crate::data::state::StateMeta {
            last_updated: today.clone(),
            exam_date: payload.exam_date.clone(),
            target_school: payload.target_school.clone(),
            target_major: payload.target_major.clone(),
        },
        ..Default::default()
    };

    for subj in &payload.subjects {
        let phase = phase_map
            .get(subj.phase.as_str())
            .cloned()
            .unwrap_or_default();
        let mut subject_state = crate::data::state::SubjectState {
            active: subj.active,
            phase,
            version: subj.version.clone(),
            target_score: subj.target_score,
            current_score: 0,
            weekly_hours: subj.weekly_hours,
            textbook: subj.textbook.clone(),
            ..Default::default()
        };

        // 专业课使用自定义名称
        if subj.subject == "professional" {
            subject_state.name = payload.professional_name.clone();
        }

        match subj.subject.as_str() {
            "math" => study_state.subjects.math = subject_state,
            "english" => study_state.subjects.english = subject_state,
            "politics" => study_state.subjects.politics = subject_state,
            "professional" => study_state.subjects.professional = subject_state,
            _ => {}
        }
    }

    crate::data::state::save_state(&data_dir, &study_state)?;
    log::info!("State 文件已初始化: {:?}/state/current.state", data_dir);
    Ok(())
}

/// 获取引导流程完成状态
///
/// 前端调用: `invoke('get_onboarding_status')`
#[tauri::command]
pub async fn get_onboarding_status(state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let data_dir = get_data_dir(state.inner())?;
    let settings = load_settings(&data_dir);
    Ok(settings.onboarding_completed)
}

/// 标记引导流程已完成
///
/// 前端调用: `invoke('complete_onboarding')`
#[tauri::command]
pub async fn complete_onboarding(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 settings 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut settings = load_settings(&data_dir);
    settings.onboarding_completed = true;
    save_settings_file(&data_dir, &settings)?;
    log::info!("引导流程已标记为完成");
    Ok(())
}

/// 更新指定科目的教材信息
///
/// 前端调用: `invoke('update_subject_textbook', { subject, textbook })`
///
/// 支持的 subject 取值：math / english / politics / professional
#[tauri::command]
pub async fn update_subject_textbook(
    subject: String,
    textbook: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;

    // H1 并发保护：串行化 state 写操作
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    let mut study_state =
        crate::data::state::read_state(&data_dir).map_err(|e| format!("读取 State 失败: {}", e))?;

    let target = match subject.as_str() {
        "math" => &mut study_state.subjects.math,
        "english" => &mut study_state.subjects.english,
        "politics" => &mut study_state.subjects.politics,
        "professional" => &mut study_state.subjects.professional,
        other => {
            return Err(format!(
                "不支持的科目: {}（仅支持 math/english/politics/professional）",
                other
            ))
        }
    };

    // 空字符串视为 None
    let normalized = textbook
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    target.textbook = normalized.clone();
    let logged_textbook = normalized.clone();

    // 此处 target 的可变借用结束（NLL），后续可以再次不可变借用 study_state
    crate::data::state::save_state(&data_dir, &study_state)
        .map_err(|e| format!("保存 State 失败: {}", e))?;

    log::info!("已更新 {} 科目教材: {:?}", subject, logged_textbook);
    Ok(())
}
