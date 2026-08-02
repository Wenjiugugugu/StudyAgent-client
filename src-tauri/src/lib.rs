//! StudyAgent Desktop — Rust 后端库入口
//!
//! 本 crate 是 StudyAgent 桌面应用（Tauri 2 + Vue 3）的 Rust 后端。
//! 采用分层架构：
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │           API Layer (api/)              │  Tauri 命令，前端调用入口
//! ├─────────────────────────────────────────┤
//! │          Core Layer (core/)             │  业务逻辑：Dashboard, Planner, Review, Knowledge
//! ├──────────────┬──────────────────────────┤
//! │  AI Layer    │   Tool Layer (tools/)     │  AI Provider + MCP Tool Dispatcher
//! │  (ai/)       │                          │
//! ├──────────────┴──────────────────────────┤
//! │         Data Layer (data/)              │  文件读取与解析：State, Plan, Records, Assets
//! └─────────────────────────────────────────┘
//! ```
//!
//! AppState 通过 `tauri::State<Mutex<AppState>>` 管理，
//! 包含数据目录路径、AI Service 实例和 MCP Tool Dispatcher 实例。

pub mod ai;
pub mod api;
pub mod core;
pub mod data;
pub mod tools;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use ai::service::AiService;
use tools::dispatcher::ToolDispatcher;
use tools::mcp::MCPServerConfig;

// ============================================================================
// AppState — 全局应用状态
// ============================================================================

/// 全局应用状态
///
/// 通过 `tauri::State<Mutex<AppState>>` 管理。
/// 内部的 `ai_service` 和 `tool_dispatcher` 使用 `Arc` 包装，
/// 使得在 async 命令中可以 clone 出引用后立即释放 Mutex 锁，
/// 避免在 await 点持有 `std::sync::MutexGuard`。
pub struct AppState {
    /// StudyAgent 数据根目录（如 `d:\StudyAgent`）
    pub data_dir: PathBuf,
    /// AI Service 实例（管理多个 AI Provider）
    pub ai_service: Arc<AiService>,
    /// MCP Tool Dispatcher 实例（统一工具调用入口）
    pub tool_dispatcher: Arc<ToolDispatcher>,
}

impl AppState {
    /// 创建新的 AppState
    pub fn new(
        data_dir: PathBuf,
        ai_service: Arc<AiService>,
        tool_dispatcher: Arc<ToolDispatcher>,
    ) -> Self {
        Self {
            data_dir,
            ai_service,
            tool_dispatcher,
        }
    }

    /// 获取数据目录路径的克隆
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

// ============================================================================
// AppSettings — 应用配置
// ============================================================================

/// 应用配置 — 对应前端 `types/settings.ts` 的 `AppSettings`
///
/// 持久化到 `{data_dir}/config/settings.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// 数据根目录路径（前端字段为 data_directory，兼容两者）
    #[serde(default = "default_data_dir", alias = "data_directory")]
    pub data_dir: String,
    /// AI Provider 配置列表
    #[serde(default)]
    pub ai_providers: Vec<ai::provider::AIProviderConfig>,
    /// MCP Server 配置列表
    #[serde(default)]
    pub mcp_servers: Vec<MCPServerConfig>,
    /// 主题（默认 "light"）
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 视觉模式（默认 "standard"）
    #[serde(default = "default_visual_mode")]
    pub visual_mode: String,
    /// 语言（默认 "zh-CN"）
    #[serde(default = "default_language")]
    pub language: String,
    /// 默认 AI Provider ID（默认空字符串）
    #[serde(default)]
    pub default_provider_id: String,
    /// 启用的 MCP ID 列表（默认空 Vec）
    #[serde(default)]
    pub enabled_mcp_ids: Vec<String>,
    /// 学习计划（JSON，默认 null）
    #[serde(default)]
    pub study_schedule: serde_json::Value,
    /// TickTick 配置（JSON，默认 null）
    #[serde(default)]
    pub ticktick: serde_json::Value,
    /// 窗口配置（JSON，默认 null）
    #[serde(default)]
    pub window: serde_json::Value,
    /// 引导流程是否已完成（默认 false）
    #[serde(default)]
    pub onboarding_completed: bool,
    /// 用户称呼（用于首页问候）
    #[serde(default)]
    pub user_name: String,
    /// 首页问候显示开关（默认 true）
    #[serde(default = "default_show_greeting")]
    pub show_greeting: bool,
    /// 考试类型（如 数学一/数学二/数学三/408计算机 等）
    #[serde(default)]
    pub exam_type: String,
    /// 目标院校
    #[serde(default)]
    pub target_school: String,
    /// 目标专业
    #[serde(default)]
    pub target_major: String,
    /// 考试日期 (YYYY-MM-DD)
    #[serde(default)]
    pub exam_date: String,
    /// 目标分数
    #[serde(default)]
    pub target_score: i32,
    /// 关闭窗口时的动作："ask" | "tray" | "quit"，默认 "ask"
    #[serde(default = "default_close_action")]
    pub close_action: String,
    /// 自定义主色调（hex 格式如 "#5b8def"，空字符串表示使用默认蓝色）
    #[serde(default)]
    pub accent_color: String,
    /// 是否显示左上角 Logo（默认 true）
    #[serde(default = "default_show_logo")]
    pub show_logo: bool,
    /// 自定义背景图相对路径（相对于 data_dir，如 "assets/backgrounds/xxx.png"）
    /// 空字符串表示使用默认纯色背景
    #[serde(default)]
    pub background_image: String,
    /// 背景图模糊度（0-20 px，0 为不模糊）
    #[serde(default = "default_background_blur")]
    pub background_blur: f64,
    /// 背景图不透明度（0-1，1 为完全不透明）
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f64,
}

fn default_background_blur() -> f64 {
    0.0
}

fn default_background_opacity() -> f64 {
    1.0
}

fn default_close_action() -> String {
    "ask".to_string()
}

fn default_show_logo() -> bool {
    true
}

impl AppSettings {
    /// 每日目标学习时长（小时），默认 5
    pub fn daily_target_hours(&self) -> f64 {
        self.study_schedule
            .get("daily_target_hours")
            .and_then(|v| v.as_f64())
            .filter(|n| *n > 0.0)
            .unwrap_or(5.0)
    }

    /// 每周学习天数，默认 6
    pub fn study_days_per_week(&self) -> i64 {
        self.study_schedule
            .get("study_days_per_week")
            .and_then(|v| v.as_i64())
            .filter(|n| *n > 0)
            .unwrap_or(6)
    }

    /// 从 study_schedule 中读取休息日列表，默认周日
    pub fn rest_days(&self) -> Vec<String> {
        self.study_schedule
            .get("rest_days")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| vec!["周日".to_string()])
    }

    /// 读取各科开始学习日期 (YYYY-MM-DD)，未设置返回空字符串
    ///
    /// AI 在生成周/日计划时，对未到开始日期的科目不应安排任务。
    /// 返回 (subject_key, start_date) 元组列表，未设置的科目不包含在内。
    pub fn subject_start_dates(&self) -> Vec<(&'static str, String)> {
        let mut result = Vec::new();
        if let Some(dates) = self.study_schedule.get("subject_start_dates").and_then(|v| v.as_object()) {
            for key in ["math", "english", "politics", "professional"] {
                if let Some(val) = dates.get(key).and_then(|v| v.as_str()) {
                    if !val.is_empty() {
                        result.push((key, val.to_string()));
                    }
                }
            }
        }
        result
    }

    /// 读取用户期望的每日任务数量，默认 3
    ///
    /// AI 在生成周/日计划时应据此控制每天的任务条数（每科约一条，
    /// 同时遵循各科开始学习日期，未开始的科目不安排）。
    pub fn daily_task_count(&self) -> i64 {
        self.study_schedule
            .get("daily_task_count")
            .and_then(|v| v.as_i64())
            .filter(|n| *n > 0)
            .unwrap_or(3)
    }

    /// 是否允许 AI 在计划中安排总结/复习任务（默认 true）。
    ///
    /// 关闭时，AI 在周/日计划中只推进新知识点，不安排"回顾"/"总结"/"复习"类任务，
    /// 适合希望持续向前推进的用户。
    pub fn enable_review_tasks(&self) -> bool {
        self.study_schedule
            .get("enable_review_tasks")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    /// 是否启用任务计时功能（默认 false）。
    ///
    /// 开启时：TodayView 任务卡显示开始/暂停按钮，State 中记录每个任务的累计专注分钟数。
    /// 关闭时：不显示计时 UI，State 中不写入计时字段（旧 state 文件无此字段也能正常解析）。
    pub fn enable_time_tracking(&self) -> bool {
        self.study_schedule
            .get("enable_time_tracking")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            ai_providers: Vec::new(),
            mcp_servers: Vec::new(),
            theme: default_theme(),
            visual_mode: default_visual_mode(),
            language: default_language(),
            default_provider_id: String::new(),
            enabled_mcp_ids: Vec::new(),
            study_schedule: serde_json::Value::Null,
            ticktick: serde_json::Value::Null,
            window: serde_json::Value::Null,
            onboarding_completed: false,
            user_name: String::new(),
            show_greeting: default_show_greeting(),
            exam_type: String::new(),
            target_school: String::new(),
            target_major: String::new(),
            exam_date: String::new(),
            target_score: 0,
            close_action: default_close_action(),
            accent_color: String::new(),
            show_logo: default_show_logo(),
            background_image: String::new(),
            background_blur: default_background_blur(),
            background_opacity: default_background_opacity(),
        }
    }
}

fn default_data_dir() -> String {
    get_default_data_dir().to_string_lossy().to_string()
}

/// 获取默认数据目录路径
///
/// 开发模式（debug_assertions）：从 exe 向上查找项目根目录（包含 `desktop/` 和 `plan/` 的目录）
/// 生产模式：使用 exe 同级目录下的 `data/` 子目录
pub fn get_default_data_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));

    // 始终尝试向上查找项目根目录（兼容 debug/release 开发构建）
    // 典型路径: .../desktop/src-tauri/target/{debug,release}/studyagent-desktop.exe
    let mut current = exe_dir.clone();
    for _ in 0..6 {
        if current.join("desktop").is_dir() && current.join("plan").is_dir() {
            log::info!("检测到项目根目录 {:?}", current);
            return current;
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    // 回退：正式安装场景使用 exe 同级的 data/ 目录
    log::info!("未检测到项目根目录，回退到 data 子目录: {:?}", exe_dir.join("data"));
    exe_dir.join("data")
}

fn default_theme() -> String {
    "light".to_string()
}

fn default_visual_mode() -> String {
    "standard".to_string()
}

fn default_language() -> String {
    "zh-CN".to_string()
}

fn default_show_greeting() -> bool {
    true
}

// ============================================================================
// Settings 持久化
// ============================================================================

/// Settings 文件所在的子目录名
pub const CONFIG_DIR: &str = "config";
/// Settings 文件名
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// 获取 settings 文件路径
pub fn settings_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_DIR).join(SETTINGS_FILE_NAME)
}

/// 从文件加载 AppSettings
///
/// 如果文件不存在或解析失败，返回默认值。
pub fn load_settings(data_dir: &Path) -> AppSettings {
    let path = settings_file_path(data_dir);

    if !path.exists() {
        log::info!("Settings 文件不存在，使用默认配置: {:?}", path);
        return AppSettings {
            data_dir: data_dir.to_string_lossy().to_string(),
            ..Default::default()
        };
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    // 若 JSON 中的 data_dir 非空且目录存在，则信任用户自定义目录
                    // 否则使用启动时检测到的 data_dir
                    let custom_dir = settings.data_dir.trim_end_matches(['/', '\\']);
                    if !custom_dir.is_empty()
                        && std::path::Path::new(custom_dir).is_dir()
                        && std::path::Path::new(custom_dir) != data_dir
                    {
                        log::info!(
                            "检测到用户自定义数据目录: {:?}（覆盖默认 {:?}）",
                            custom_dir,
                            data_dir
                        );
                        // 注意：此处不修改入参 data_dir，由调用方在 init_app_state 阶段重新读取
                    } else {
                        settings.data_dir = data_dir.to_string_lossy().to_string();
                    }
                    log::info!("已加载 Settings: {:?}", path);
                    settings
                }
                Err(e) => {
                    log::warn!("解析 Settings 文件失败: {}, 使用默认配置", e);
                    AppSettings {
                        data_dir: data_dir.to_string_lossy().to_string(),
                        ..Default::default()
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("读取 Settings 文件失败: {}, 使用默认配置", e);
            AppSettings {
                data_dir: data_dir.to_string_lossy().to_string(),
                ..Default::default()
            }
        }
    }
}

/// 保存 AppSettings 到文件
pub fn save_settings_file(data_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    let path = settings_file_path(data_dir);

    // 确保目录存在
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 config 目录失败: {}", e))?;
        }
    }

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("序列化 Settings 失败: {}", e))?;

    std::fs::write(&path, json)
        .map_err(|e| format!("写入 Settings 文件失败 {:?}: {}", path, e))?;

    log::info!("已保存 Settings: {:?}", path);
    Ok(())
}

// ============================================================================
// AppState 初始化
// ============================================================================

/// 初始化 AppState
///
/// 流程：
/// 1. 加载 Settings（如果存在）
/// 2. 确保数据目录结构存在
/// 3. 创建 AiService
/// 4. 创建 ToolDispatcher（需要 async 初始化）
/// 5. 返回 `Mutex<AppState>`
///
/// 此函数使用 `tauri::async_runtime::block_on` 来处理 ToolDispatcher 的异步初始化。
/// 应在 Tauri 的 `setup` 回调中调用。
pub fn init_app_state(data_dir: PathBuf) -> Mutex<AppState> {
    // 确保 data_dir 存在
    if !data_dir.exists() {
        log::warn!("数据目录不存在: {:?}, 尝试创建", data_dir);
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            log::error!("创建数据目录失败: {}", e);
        }
    }

    // 确保核心子目录存在
    ensure_data_directories(&data_dir);

    // 加载 settings（若用户在 settings.json 中指定了自定义 data_dir 且目录存在，
    // load_settings 会保留该值；此处需要据此切换实际使用的数据目录）
    let settings = load_settings(&data_dir);
    let effective_data_dir = {
        let custom = settings.data_dir.trim_end_matches(['/', '\\']);
        if !custom.is_empty()
            && std::path::Path::new(custom).is_dir()
            && std::path::Path::new(custom) != data_dir.as_path()
        {
            let custom_path = PathBuf::from(custom);
            log::info!("切换到用户自定义数据目录: {:?}", custom_path);
            // 确保新目录的子结构存在
            ensure_data_directories(&custom_path);
            custom_path
        } else {
            data_dir
        }
    };

    // 创建 AI Service
    let ai_service = Arc::new(AiService::from_configs(settings.ai_providers.clone()));

    // 创建 Tool Dispatcher（异步初始化）
    let tool_dispatcher = Arc::new(tauri::async_runtime::block_on(async {
        ToolDispatcher::from_configs(settings.mcp_servers.clone()).await
    }));

    let state = AppState::new(effective_data_dir.clone(), ai_service, tool_dispatcher);

    // 设置 AI 用量日志目录（全局 OnceLock，仅设置一次）
    crate::data::ai_usage::set_log_dir(effective_data_dir);

    Mutex::new(state)
}

/// 确保数据目录结构完整
///
/// 创建以下子目录（如果不存在）：
/// - state/
/// - plan/
/// - records/
/// - logs/
/// - assets/knowledge/objects/
/// - assets/user_model/capabilities/
/// - assets/user_model/observations/
/// - assets/milestones/
/// - assets/mapping/entries/
/// - assets/registry/
/// - config/
pub fn ensure_data_directories(data_dir: &Path) {
    let subdirs = [
        "state",
        "plan",
        "records",
        "logs",
        "assets/knowledge/objects",
        "assets/user_model/capabilities",
        "assets/user_model/observations",
        "assets/milestones",
        "assets/mapping/entries",
        "assets/registry",
        "config",
    ];

    for subdir in &subdirs {
        let path = data_dir.join(subdir);
        if !path.exists() {
            if let Err(e) = std::fs::create_dir_all(&path) {
                log::warn!("创建目录失败 {:?}: {}", path, e);
            }
        }
    }
}

/// 用新的 Settings 重新初始化 AI Service 和 Tool Dispatcher
///
/// 在 `save_settings` 命令中调用，以应用新的配置。
pub async fn reinitialize_services(
    state: &Mutex<AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    // 先保存 settings 文件
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    save_settings_file(&data_dir, &settings)?;

    // 创建新的 AI Service
    let new_ai_service = Arc::new(AiService::from_configs(settings.ai_providers.clone()));

    // 创建新的 Tool Dispatcher
    let new_tool_dispatcher = Arc::new(
        ToolDispatcher::from_configs(settings.mcp_servers.clone()).await,
    );

    // 替换 state 中的服务实例
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.ai_service = new_ai_service;
        s.tool_dispatcher = new_tool_dispatcher;
    }

    log::info!("已用新配置重新初始化 AI Service 和 Tool Dispatcher");
    Ok(())
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 从 Mutex<AppState> 中获取 data_dir 的克隆
///
/// 用于在命令中快速获取 data_dir，不持有锁过长时间。
pub fn get_data_dir(state: &Mutex<AppState>) -> Result<PathBuf, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.data_dir.clone())
}

/// 从 Mutex<AppState> 中获取 AI Service 的 Arc 克隆
pub fn get_ai_service(state: &Mutex<AppState>) -> Result<Arc<AiService>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.ai_service.clone())
}

/// 从 Mutex<AppState> 中获取 Tool Dispatcher 的 Arc 克隆
pub fn get_tool_dispatcher(state: &Mutex<AppState>) -> Result<Arc<ToolDispatcher>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.tool_dispatcher.clone())
}

/// 从 Mutex<AppState> 中同时获取 data_dir 和 AI Service
pub fn get_data_dir_and_ai(
    state: &Mutex<AppState>,
) -> Result<(PathBuf, Arc<AiService>), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok((s.data_dir.clone(), s.ai_service.clone()))
}

/// 从 Mutex<AppState> 中同时获取 data_dir 和 Tool Dispatcher
pub fn get_data_dir_and_dispatcher(
    state: &Mutex<AppState>,
) -> Result<(PathBuf, Arc<ToolDispatcher>), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok((s.data_dir.clone(), s.tool_dispatcher.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_settings_roundtrip_with_frontend_fields() {
        let json = r#"{
            "data_directory": ".",
            "theme": "dark",
            "language": "zh-CN",
            "user_name": "数二",
            "show_greeting": true,
            "exam_type": "数学二",
            "exam_date": "2026-12-26",
            "target_score": 360,
            "onboarding_completed": true,
            "ai_providers": [],
            "mcp_servers": [],
            "default_provider_id": "",
            "enabled_mcp_ids": [],
            "study_schedule": {"start_time":"09:00","end_time":"22:00","daily_target_hours":5,"study_days_per_week":6,"review_reminder_time":"23:00"},
            "ticktick": {"enabled":false,"tag_prefix":"计划"},
            "window": {"width":1280,"height":820,"maximized":false}
        }"#;

        let settings: AppSettings = serde_json::from_str(json).expect("应能解析前端 settings JSON");
        assert_eq!(settings.data_dir, ".");
        assert_eq!(settings.user_name, "数二");
        assert!(settings.show_greeting);
        assert_eq!(settings.exam_type, "数学二");
        assert_eq!(settings.exam_date, "2026-12-26");
        assert_eq!(settings.target_score, 360);

        let out = serde_json::to_string_pretty(&settings).expect("应能序列化 settings");
        assert!(out.contains("user_name"));
        assert!(out.contains("show_greeting"));
        assert!(out.contains("exam_type"));
        assert!(out.contains("exam_date"));
        assert!(out.contains("target_score"));
    }
}
