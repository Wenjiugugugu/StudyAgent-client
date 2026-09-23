//! Data Layer — 数据读取与解析
//!
//! 负责从文件系统读取 StudyAgent 的各类数据文件：
//! - `state/current.state` (TOML)
//! - `plan/YYYY-MM-DD_day.json` (结构化 JSON, 遵循 Data Contract)
//! - `plan/YYYY-Wxx_week.json` (结构化 JSON, 遵循 Data Contract)
//! - `records/YYYY-MM-DD_review.json` (结构化 JSON, 遵循 Data Contract)
//! - `assets/` 下的知识对象、用户画像、里程碑等 (Markdown + YAML frontmatter,
//!   解析逻辑位于 `assets` 模块内)
//!
//! Planning Layer (Week Plan / Today Plan / Review) 已统一采用
//! `{ version, meta, data, view }` 结构化 JSON 契约，不再使用 Markdown/YAML。

pub mod ai_usage;
pub mod assets;
pub mod backup;
pub mod briefing;
pub mod focus;
pub mod goal;
pub mod plan;
pub mod progress_tables;
pub mod records;
pub mod state;
pub mod ui_flags;

pub use ai_usage::*;
pub use assets::*;
pub use backup::*;
pub use briefing::*;
pub use focus::*;
pub use goal::*;
pub use plan::*;
pub use progress_tables::*;
pub use records::*;
pub use state::*;
pub use ui_flags::*;

// 日期 / 时间工具已收口到 `core::date_utils`（单一实现），此处再导出以保持
// data 层与命令层既有调用路径（`crate::data::add_days` 等）不变。
// 注意：date_utils 是不依赖任何业务模块的叶子模块，data → date_utils 不构成环。
pub use crate::core::date_utils::{
    add_days, day_diff, days_between, deadline_active_on, get_week_end, get_week_start,
    get_weekday, now_string, today_string, validate_date, weekday_name,
};

use std::io::Write;
use std::path::{Path, PathBuf};

/// 应用统一 Result 类型别名
pub type DataResult<T> = Result<T, String>;

/// 会影响学习连续性的持久化数据目录清单。
///
/// 初始化、备份、迁移和诊断应复用这一清单，避免新增数据域后遗漏备份。
/// `logs/` 属于可选诊断数据，不包含在这里。
pub const PERSISTED_DATA_SUBDIRS: [&str; 9] = [
    "state",
    "plan",
    "records",
    "config",
    "assets",
    "focus",
    "progress_tables",
    "adaptive",
    "analysis",
];

/// 写入串行锁：`atomic_write` 的临时文件名与目标同名（`<name>.tmp`），
/// 若同进程内两个 async 命令并发写同一文件，会互相覆盖临时文件并产生错乱内容。
/// 这里用进程级互斥把「创建临时文件 → 写入 → rename」整体串行化。
///
/// 注意：本锁只保证**单次写入**的原子性，不保证「读-改-写」序列的互斥
///（两个命令各自读到旧值再分别写回，仍会丢更新）。需要防丢更新的调用方
/// 应在命令层串行化，或改用带版本校验的读改写封装。
static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 原子写入文件：先写临时文件，再 rename 到目标路径
///
/// 临时文件与目标文件在同一目录，保证同卷 rename 在 Windows/Linux 上均为原子操作。
/// 若写入过程中进程崩溃，临时文件残留但目标文件不受影响
///（固定临时名意味着下次写入会直接覆盖残留，不会累积垃圾文件）。
pub fn atomic_write(path: &Path, content: &str) -> DataResult<()> {
    // 写入期间持锁；锁中毒（持锁线程 panic）时按「可继续使用」处理，避免一次 panic 永废写能力
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = path.with_extension("tmp");
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败 {:?}: {}", parent, e))?;
        }
    }
    let mut file =
        std::fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败 {:?}: {}", tmp, e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("写入临时文件失败 {:?}: {}", tmp, e))?;
    file.sync_all()
        .map_err(|e| format!("同步临时文件失败 {:?}: {}", tmp, e))?;
    drop(file);
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("原子重命名失败 {:?} -> {:?}: {}", tmp, path, e))?;
    Ok(())
}

/// 获取 AI 调试日志文件路径
pub fn ai_debug_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("ai-debug.log")
}

/// 获取运行时日志文件路径（env_logger 落盘输出）
///
/// 生产版无控制台窗口（`windows_subsystem = "windows"`），env_logger 若只输出
/// stderr 则日志完全丢失。主进程启动时将 env_logger 同时写入该文件，
/// `read_app_log` 命令可读取以排查更新等运行时问题。
pub fn app_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("app.log")
}

/// 将 AI 调试信息追加写入 {data_dir}/logs/ai-debug.log
///
/// 生产环境下 env_logger 输出到 stderr 不可见，此函数用于在 AI 调用失败时
/// 将关键信息（请求/响应/错误）持久化到文件，便于排查。
pub fn write_ai_debug_log(data_dir: &Path, tag: &str, message: &str) {
    let log_path = ai_debug_log_path(data_dir);
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let timestamp = now_string();
    let entry = format!(
        "[{}][{}] {}\n{}\n{}\n\n",
        timestamp,
        tag,
        "-".repeat(60),
        message,
        "-".repeat(60)
    );
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| f.write_all(entry.as_bytes()))
    {
        log::warn!("写入 AI 调试日志失败 {:?}: {}", log_path, e);
    }
}

// ============================================================================
// 文件系统工具
// ============================================================================

/// 读取文本文件内容
pub fn read_file_content(path: &Path) -> DataResult<String> {
    std::fs::read_to_string(path).map_err(|e| format!("读取文件失败 {:?}: {}", path, e))
}

/// 列出目录下的文件（非递归）
pub fn list_dir_files(dir: &Path) -> DataResult<Vec<std::path::PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    std::fs::read_dir(dir)
        .map_err(|e| format!("读取目录失败 {:?}: {}", dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .map(|entry| Ok(entry.path()))
        .collect()
}

/// 递归列出目录下所有文件
pub fn list_dir_files_recursive(dir: &Path) -> DataResult<Vec<std::path::PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败 {:?}: {}", dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            let sub = list_dir_files_recursive(&path)?;
            result.extend(sub);
        } else if path.is_file() {
            result.push(path);
        }
    }

    Ok(result)
}

/// 提取 task_id 的日期前缀（前 10 字符，即 YYYY-MM-DD），并确保落在字符边界上（M4）
///
/// task_id 形如 `2026-07-24-...`。若 id 不足 10 字符（异常数据）则返回 None，
/// 避免 `&str[..10]` 在非字符边界或越界时 panic。
pub fn task_id_date_prefix(id: &str) -> Option<&str> {
    id.get(..id.floor_char_boundary(10))
}

// ============================================================================
// JSON Value 辅助函数
// ============================================================================

/// 从 serde_json::Value 中反序列化为目标类型
pub fn from_value<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> DataResult<T> {
    serde_json::from_value(value.clone()).map_err(|e| format!("反序列化失败: {}", e))
}

/// 清理 AI 可能包裹的代码块，提取纯 JSON（M6：统一入口）
///
/// 原在 planner.rs 与 review.rs 中各自存在一份完全相同实现，现提取至此公共模块。
/// 处理两种常见包裹形式：
/// 1. ```json ... ``` / ``` ... ``` 代码围栏
/// 2. 纯文本前后带可有可无的叙述，取第一个 '{' 到最后一个 '}'
pub fn clean_ai_json(content: &str) -> String {
    let trimmed = content.trim();

    // 尝试提取 ```json ... ``` 或 ``` ... ``` 包裹的内容
    if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map(|p| p + 1).unwrap_or(0);
        // H1：未闭合围栏时 rfind 会命中开头的 ```（位置 0），导致 start > end 切片 panic。
        // 此时回退到取开头围栏之后的全部内容。
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        let end = if end < start { trimmed.len() } else { end };
        return trimmed[start..end].trim().to_string();
    }

    // 尝试找到第一个 '{' 和最后一个 '}'
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return trimmed[start..=end].to_string();
        }
    }

    trimmed.to_string()
}
