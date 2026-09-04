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

use std::io::Write;
use std::path::{Path, PathBuf};

// M13：日期运算改用 chrono；`NaiveDate::weekday` 来自 Datelike trait
use chrono::Datelike;

/// 应用统一 Result 类型别名
pub type DataResult<T> = Result<T, String>;

/// 原子写入文件：先写临时文件，再 rename 到目标路径
///
/// 临时文件与目标文件在同一目录，保证同卷 rename 在 Windows/Linux 上均为原子操作。
/// 若写入过程中进程崩溃，临时文件残留但目标文件不受影响。
pub fn atomic_write(path: &Path, content: &str) -> DataResult<()> {
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

/// 获取当前日期字符串 (YYYY-MM-DD)
///
/// 使用 UTC+8 时区（中国标准时间，M13：改用 chrono 实现），避免 UTC+8 用户在
/// 00:00-08:00 之间因 UTC 时间偏移而得到前一天的日期。
pub fn today_string() -> String {
    let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移有效");
    chrono::Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d")
        .to_string()
}

/// 获取当前时间字符串 (YYYY-MM-DDTHH:mm)
///
/// 同样使用 UTC+8 时区。
pub fn now_string() -> String {
    let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移有效");
    chrono::Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%dT%H:%M")
        .to_string()
}

/// 计算两个日期之间的天数差（date1 - date2）
pub fn days_between(date1: &str, date2: &str) -> DataResult<i64> {
    let d1 = parse_naive_date(date1)?;
    let d2 = parse_naive_date(date2)?;
    Ok((d1 - d2).num_days())
}

/// 解析 `YYYY-MM-DD` 为 chrono::NaiveDate
fn parse_naive_date(date: &str) -> DataResult<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("无效日期格式: {} ({})", date, e))
}

/// 校验日期字符串是否符合 `YYYY-MM-DD` 格式（C4-c）
///
/// 用于所有接收 `date` 参数的命令入口，防止恶意输入（如 `../../config/settings`）
/// 通过路径拼接穿越数据目录。
pub fn validate_date(date: &str) -> DataResult<()> {
    if !is_valid_date_format(date) {
        return Err(format!("无效日期格式: {}", date));
    }
    Ok(())
}

/// 判断字符串是否为合法的 `YYYY-MM-DD` 格式
fn is_valid_date_format(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            4 | 7 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_digit() {
                    return false;
                }
            }
        }
    }
    true
}

/// 提取 task_id 的日期前缀（前 10 字符，即 YYYY-MM-DD），并确保落在字符边界上（M4）
///
/// task_id 形如 `2026-07-24-...`。若 id 不足 10 字符（异常数据）则返回 None，
/// 避免 `&str[..10]` 在非字符边界或越界时 panic。
pub fn task_id_date_prefix(id: &str) -> Option<&str> {
    id.get(..id.floor_char_boundary(10))
}

/// 获取日期对应的星期几（0=周一, 6=周日）
pub fn get_weekday(date: &str) -> DataResult<u32> {
    let d = parse_naive_date(date)?;
    Ok(d.weekday().num_days_from_monday() as u32)
}

/// 获取日期对应的中文星期几名称（周一 至 周日）
pub fn weekday_name(date: &str) -> DataResult<String> {
    let weekday = get_weekday(date)?;
    let names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    Ok(names[weekday as usize].to_string())
}

/// 获取指定日期所在周的周一日期 (YYYY-MM-DD)
pub fn get_week_start(date: &str) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    let weekday = d.weekday().num_days_from_monday() as i64;
    Ok(days_from_epoch_to_date_string(d, -weekday))
}

/// 获取指定日期所在周的周日日期 (YYYY-MM-DD)
pub fn get_week_end(date: &str) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    let weekday = d.weekday().num_days_from_monday() as i64;
    Ok(days_from_epoch_to_date_string(d, 6 - weekday))
}

/// 获取从 start_date 开始的 n 天后的日期
pub fn add_days(date: &str, n: i64) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    Ok(days_from_epoch_to_date_string(d, n))
}

/// 以基准日期 d 为原点，偏移 offset_days 天后的日期字符串
fn days_from_epoch_to_date_string(d: chrono::NaiveDate, offset_days: i64) -> String {
    d.checked_add_signed(chrono::Duration::days(offset_days))
        .map(|nd| nd.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
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
