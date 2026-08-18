//! AI 用量日志 — 持久化记录每次 AI 调用的 token 消耗
//!
//! 日志文件位于 `{data_dir}/state/ai_usage_log.json`，JSON 数组格式。
//! 最多保留 `MAX_ENTRIES` 条记录，超出后自动裁剪最早的记录。
//!
//! 由 `AiService::chat` / `AiService::chat_stream` 在每次调用后写入，
//! 前端通过 Tauri 命令读取用于展示历史用量与估算费用。

use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// 最多保留的记录数
const MAX_ENTRIES: usize = 500;

/// 全局日志目录（应用启动时设置，切换数据目录时更新）
///
/// H17：用 RwLock 而非 OnceLock，使 `change_data_directory` 切换后
/// AI 用量日志能写入新数据目录。
static LOG_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 单条 AI 用量记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageEntry {
    /// 调用时间戳（ISO 字符串 YYYY-MM-DDTHH:mm）
    pub timestamp: String,
    /// Agent 类型标签（planner / reviewer / assistant / teacher / unknown）
    pub agent: String,
    /// 实际使用的模型名（来自 ChatResponse.model）
    pub model: String,
    /// 输入 token 数
    pub prompt_tokens: u32,
    /// 输出 token 数
    pub completion_tokens: u32,
    /// 总 token 数
    pub total_tokens: u32,
    /// 调用耗时（毫秒）
    pub duration_ms: u64,
    /// 状态：success / error
    pub status: String,
    /// 错误信息（仅失败时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 设置全局日志目录（应用启动时调用；切换数据目录时再次调用以更新）
pub fn set_log_dir(dir: PathBuf) {
    *LOG_DIR.write() = Some(dir);
}

/// 获取全局日志目录
fn log_dir() -> Option<PathBuf> {
    LOG_DIR.read().clone()
}

/// 日志文件路径
fn log_path() -> Option<PathBuf> {
    log_dir().map(|d| d.join("state").join("ai_usage_log.json"))
}

/// 追加一条用量记录
///
/// 如果全局日志目录未设置或写入失败，静默忽略（不影响 AI 调用主流程）。
pub fn append(entry: AiUsageEntry) {
    let path = match log_path() {
        Some(p) => p,
        None => return,
    };

    // 确保目录存在
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("创建 AI 用量日志目录失败: {}", e);
            return;
        }
    }

    // 读取现有记录
    let mut entries: Vec<AiUsageEntry> = match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    // 追加新记录
    entries.push(entry);

    // 裁剪到最大数量
    if entries.len() > MAX_ENTRIES {
        let drain_count = entries.len() - MAX_ENTRIES;
        entries.drain(..drain_count);
    }

    // 写入文件
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => {
            if let Err(e) = super::atomic_write(&path, &json) {
                log::warn!("写入 AI 用量日志失败: {}", e);
            }
        }
        Err(e) => {
            log::warn!("序列化 AI 用量日志失败: {}", e);
        }
    }
}

/// 读取全部用量记录（按时间升序）
pub fn read_all() -> Vec<AiUsageEntry> {
    let path = match log_path() {
        Some(p) => p,
        None => return Vec::new(),
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 清空全部用量记录
pub fn clear() {
    let path = match log_path() {
        Some(p) => p,
        None => return,
    };

    if let Err(e) = super::atomic_write(&path, "[]") {
        log::warn!("清空 AI 用量日志失败: {}", e);
    }
}
