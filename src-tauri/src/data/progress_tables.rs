//! Progress Tables — 各科「进度表」数据层
//!
//! 每科可保存多份个性化进度表（如「数二全程」「数二强化」），同一时刻每科仅一份为
//! 启用状态（active_id）。节点含：标题、阶段/章节、学习状态、计划日期、备注。
//!
//! 持久化到 `{data_dir}/progress_tables/progress_index.json`：
//! ```json
//! {
//!   "subjects": { "math": { "active_id": "t1", "tables": [ ... ] } },
//!   "web_search": { "enabled": false, "provider": "bocha", "base_url": "", "api_key": "" }
//! }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data::{atomic_write, DataResult};

/// 数据子目录名
pub const PROGRESS_DIR: &str = "progress_tables";
/// 索引文件名
pub const PROGRESS_INDEX_FILE: &str = "progress_index.json";

/// 节点学习状态
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// 待学
    Pending,
    /// 学习中
    Learning,
    /// 已掌握
    Mastered,
}

impl NodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeStatus::Pending => "pending",
            NodeStatus::Learning => "learning",
            NodeStatus::Mastered => "mastered",
        }
    }
}

impl Default for NodeStatus {
    fn default() -> Self {
        NodeStatus::Pending
    }
}

/// 进度表节点
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ProgressNode {
    /// 节点 id（进度表内唯一）
    pub id: String,
    /// 知识点/章节标题
    pub title: String,
    /// 所属阶段/章节（如「第一章 函数、极限、连续」），可空
    pub phase: String,
    /// 学习状态
    pub status: NodeStatus,
    /// 计划学习日期（YYYY-MM-DD），可空
    pub planned_date: Option<String>,
    /// 备注
    pub note: String,
}

impl Default for ProgressNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            phase: String::new(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        }
    }
}

/// 进度表
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ProgressTable {
    pub id: String,
    pub subject: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub nodes: Vec<ProgressNode>,
}

impl Default for ProgressTable {
    fn default() -> Self {
        Self {
            id: String::new(),
            subject: String::new(),
            name: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            nodes: Vec::new(),
        }
    }
}

/// 单一科目的进度表集合
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SubjectProgressSet {
    /// 当前启用的进度表 id（为空表示该科暂无启用表）
    pub active_id: String,
    pub tables: Vec<ProgressTable>,
}

/// 联网搜索配置（AI 生成进度表时可选拉取最新考研大纲）
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct WebSearchConfig {
    pub enabled: bool,
    /// 搜索厂商：暂支持 "bocha"（博查查）
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
}

/// 全部进度表索引
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ProgressIndex {
    pub subjects: HashMap<String, SubjectProgressSet>,
    pub web_search: WebSearchConfig,
}

// ============================================================================
// 索引读写
// ============================================================================

/// 索引文件路径
pub fn progress_index_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PROGRESS_DIR).join(PROGRESS_INDEX_FILE)
}

/// 加载索引；文件缺失或解析失败时返回空索引
pub fn load_progress_index(data_dir: &Path) -> ProgressIndex {
    let path = progress_index_path(data_dir);
    if !path.exists() {
        return ProgressIndex::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<ProgressIndex>(&content) {
            Ok(idx) => idx,
            Err(e) => {
                log::warn!("解析进度表索引失败 {:?}: {}", path, e);
                ProgressIndex::default()
            }
        },
        Err(e) => {
            log::warn!("读取进度表索引失败 {:?}: {}", path, e);
            ProgressIndex::default()
        }
    }
}

/// 保存索引
pub fn save_progress_index(data_dir: &Path, index: &ProgressIndex) -> DataResult<()> {
    let path = progress_index_path(data_dir);
    let json =
        serde_json::to_string_pretty(index).map_err(|e| format!("序列化进度表索引失败: {}", e))?;
    atomic_write(&path, &json)
}

/// 生成一个本机内相对唯一的 id（毫秒时间戳 + 取名字符）
pub fn new_progress_id(kind: &str, seed: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let salt = seed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(6)
        .collect::<String>();
    format!("{}{}-{}", kind, millis, salt)
}

/// 主题显示名（用于提示）
pub fn subject_label(subject: &str) -> &'static str {
    match subject {
        "math" => "数学",
        "english" => "英语",
        "politics" => "政治",
        "professional" => "专业课",
        _ => "课程",
    }
}
