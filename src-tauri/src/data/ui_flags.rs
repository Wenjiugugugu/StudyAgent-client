//! UI 状态标记的轻量持久化（跨重启保留的「已提示」类标记）
//!
//! 背景：浏览器 localStorage 在部分环境下会随 WebView2 数据目录丢失，
//! 导致「更新日志弹窗」「每日简报提示」等每次重启应用后都重新出现。
//! 这里提供基于文件（`config/ui_flags.json`）的 KV 读写，随数据目录稳定落盘。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{atomic_write, DataResult};

/// UI 标记文件路径（与用户设置同目录）
fn ui_flags_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config").join("ui_flags.json")
}

/// 读取某个 UI 标记；不存在或读取失败时返回空字符串
pub fn get_ui_flag(data_dir: &Path, key: &str) -> String {
    match std::fs::read_to_string(ui_flags_path(data_dir)) {
        Ok(content) => serde_json::from_str::<BTreeMap<String, String>>(&content)
            .ok()
            .and_then(|m| m.get(key).cloned())
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// 写入某个 UI 标记（原子落盘，保留其余键）
pub fn set_ui_flag(data_dir: &Path, key: &str, value: &str) -> DataResult<()> {
    let path = ui_flags_path(data_dir);
    let mut flags: BTreeMap<String, String> = match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    };
    flags.insert(key.to_string(), value.to_string());
    let json =
        serde_json::to_string_pretty(&flags).map_err(|e| format!("序列化 ui_flags 失败: {}", e))?;
    atomic_write(&path, &json)
}