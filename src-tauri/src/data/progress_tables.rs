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

/// 节点学习状态（5 级：待学 → 学习中 → 基础 → 强化中 → 掌握）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// 待学
    #[default]
    Pending,
    /// 学习中
    Learning,
    /// 基础（第一轮基础已过）
    Basic,
    /// 强化中
    Reinforcing,
    /// 掌握
    Mastered,
}

impl NodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeStatus::Pending => "pending",
            NodeStatus::Learning => "learning",
            NodeStatus::Basic => "basic",
            NodeStatus::Reinforcing => "reinforcing",
            NodeStatus::Mastered => "mastered",
        }
    }

    /// 进度层级：待学 0 < 学习中 1 < 基础 2 < 强化中 3 < 掌握 4
    pub fn rank(&self) -> u8 {
        match self {
            NodeStatus::Pending => 0,
            NodeStatus::Learning => 1,
            NodeStatus::Basic => 2,
            NodeStatus::Reinforcing => 3,
            NodeStatus::Mastered => 4,
        }
    }

    /// 取两者中更靠后的进度（用于复盘/预估联动：只升不降，避免覆盖用户手动掌握的节点）
    pub fn max_with(&self, other: NodeStatus) -> NodeStatus {
        if other.rank() > self.rank() {
            other
        } else {
            *self
        }
    }
}

/// 从五态字符串解析（兼容旧数据 pending/learning/mastered；未知值回退 pending）
pub fn parse_node_status(s: &str) -> NodeStatus {
    match s.trim() {
        "learning" => NodeStatus::Learning,
        "basic" => NodeStatus::Basic,
        "reinforcing" => NodeStatus::Reinforcing,
        "mastered" => NodeStatus::Mastered,
        _ => NodeStatus::Pending,
    }
}

/// 节点级别：章节 / 知识点（默认知识点，兼容旧数据）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLevel {
    /// 知识点
    #[default]
    Knowledge,
    /// 章节（作为分组）
    Chapter,
}

impl NodeLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeLevel::Knowledge => "knowledge",
            NodeLevel::Chapter => "chapter",
        }
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
    /// 节点级别：章节 / 知识点
    pub level: NodeLevel,
    /// 知识点归属的章节节点 id；章节节点为 None
    pub parent_id: Option<String>,
    /// 所属阶段/章节（如「第一章 函数、极限、连续」），可空（旧数据兜底显示）
    pub phase: String,
    /// 学习状态
    pub status: NodeStatus,
    /// 计划学习日期（YYYY-MM-DD），可空
    pub planned_date: Option<String>,
    /// 备注
    pub note: String,
    /// 预估学习时长（小时）——隐藏数据，不展示给用户。
    ///
    /// 内置考纲表与 AI 生成的进度表会写入基准预估值；周计划生成时可作为任务
    /// 时长参考，并按自适应周计划学到的用户效率系数（estimation_factor）缩放。
    /// 为空表示暂无预估值（自定义/手工节点）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_hours: Option<f64>,
}

impl Default for ProgressNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            level: NodeLevel::Knowledge,
            parent_id: None,
            phase: String::new(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
            estimated_hours: None,
        }
    }
}

/// 进度表来源：内置考纲表 / 自定义表
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableOrigin {
    /// 自定义表（用户新建 / AI 生成）
    #[default]
    Custom,
    /// 内置官方考纲表
    Builtin,
}

/// 进度表
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ProgressTable {
    pub id: String,
    pub subject: String,
    /// 考纲方案：数一/数二/数三/英一/英二/408/307/政治
    pub variant: String,
    pub name: String,
    /// 来源：内置考纲表 / 自定义表（默认自定义，兼容旧数据）
    pub origin: TableOrigin,
    pub created_at: String,
    pub updated_at: String,
    pub nodes: Vec<ProgressNode>,
}

/// 单一科目的进度表集合
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SubjectProgressSet {
    /// 当前启用的考纲方案（空 = 默认取该科第一个方案）
    pub active_variant: String,
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

/// 加载索引；文件缺失或解析失败时返回空索引。
/// 加载后自动修复历史脏数据：旧版 `new_progress_id` 缺少递增序号，同一毫秒内生成的
/// 中文章节节点 id 全部相同，导致知识点 parent_id 全部指向同一个章节，界面上每个
/// 板块都串成同一份知识点。这里检测重复 id 并重建节点 id 与 parent_id 关联。
pub fn load_progress_index(data_dir: &Path) -> ProgressIndex {
    let path = progress_index_path(data_dir);
    if !path.exists() {
        return ProgressIndex::default();
    }
    let mut idx = match std::fs::read_to_string(&path) {
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
    };
    if repair_progress_index(&mut idx) {
        if let Err(e) = save_progress_index(data_dir, &idx) {
            log::warn!("修复进度表重复 id 后保存失败 {:?}: {}", path, e);
        }
    }
    idx
}

/// 修复索引中所有表的重复节点 id。返回 true 表示有修复发生。
fn repair_progress_index(index: &mut ProgressIndex) -> bool {
    let mut repaired = false;
    for set in index.subjects.values_mut() {
        for table in set.tables.iter_mut() {
            if repair_table_ids(table) {
                repaired = true;
            }
        }
    }
    repaired
}

/// 检测并修复单张表内重复的节点 id。
///
/// 若存在重复 id，则为每个节点按位置重新生成唯一 id。由于旧 id 重复导致无法用
/// 旧 id 做映射，章节归属改用知识点的 `phase` 字段匹配章节 `title` 来重建
/// （内置考纲生成的表中知识点 phase 始终等于所属章节 title）。返回 true 表示
/// 该表被修复过。
fn repair_table_ids(table: &mut ProgressTable) -> bool {
    let mut seen = std::collections::HashSet::new();
    let has_dup = table.nodes.iter().any(|n| !seen.insert(n.id.clone()));
    if !has_dup {
        return false;
    }

    log::warn!(
        "进度表「{}」存在重复节点 id，正在重建（{} 个节点）",
        table.name,
        table.nodes.len()
    );

    // 1. 按位置为每个节点生成新 id（不能用旧 id 做 key，因为旧 id 本身重复）
    let new_ids: Vec<String> = table
        .nodes
        .iter()
        .map(|n| {
            let kind = if n.level == NodeLevel::Chapter {
                "c"
            } else {
                "n"
            };
            new_progress_id(kind, &n.title)
        })
        .collect();

    // 2. 构建 章节 title → 新章节 id 映射（用于按 phase 修复知识点归属）
    let mut chapter_by_title: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (i, node) in table.nodes.iter().enumerate() {
        if node.level == NodeLevel::Chapter {
            chapter_by_title
                .entry(node.title.clone())
                .or_insert_with(|| new_ids[i].clone());
        }
    }

    // 3. 更新节点 id 与 parent_id
    for (i, node) in table.nodes.iter_mut().enumerate() {
        node.id = new_ids[i].clone();
        if node.level == NodeLevel::Chapter {
            node.parent_id = None;
        } else if node.parent_id.is_some() {
            // 知识点：优先按 phase 匹配章节 title 来修复归属
            node.parent_id = chapter_by_title.get(&node.phase).cloned();
        }
    }
    true
}

/// 保存索引
pub fn save_progress_index(data_dir: &Path, index: &ProgressIndex) -> DataResult<()> {
    let path = progress_index_path(data_dir);
    let json =
        serde_json::to_string_pretty(index).map_err(|e| format!("序列化进度表索引失败: {}", e))?;
    atomic_write(&path, &json)
}

/// 进程内全局递增计数器：保证同一毫秒内生成的 id 也互不冲突
static ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 生成一个本机内相对唯一的 id（毫秒时间戳 + 进程内递增序号 + 名字盐）
///
/// 注意：不能只依赖「毫秒 + 名字盐」——内置考纲批量生成时同毫秒内大量节点的
/// 种子是中文（如章节名「全书章节」），按 ascii 过滤后盐为空，会导致
/// 同一批所有节点 id 相同，界面按 id 分组时所有章节都串成同一份知识点
/// （典型表现：408 的总专业课进度表每个板块都显示「数据结构」的内容）。
/// 递增序号保证同一毫秒内多次调用也返回不同 id。
pub fn new_progress_id(kind: &str, seed: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let seq = ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let salt = seed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(6)
        .collect::<String>();
    format!("{}{}-{}-{}", kind, millis, seq, salt)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dup_table() -> ProgressTable {
        let dup_id = "c-dup".to_string();
        ProgressTable {
            id: "t1".to_string(),
            subject: "professional".to_string(),
            variant: "408 计算机".to_string(),
            name: "测试表".to_string(),
            origin: TableOrigin::Builtin,
            created_at: String::new(),
            updated_at: String::new(),
            nodes: vec![
                ProgressNode {
                    id: dup_id.clone(),
                    title: "数据结构".to_string(),
                    level: NodeLevel::Chapter,
                    parent_id: None,
                    phase: "数据结构".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
                ProgressNode {
                    id: dup_id.clone(),
                    title: "计算机组成原理".to_string(),
                    level: NodeLevel::Chapter,
                    parent_id: None,
                    phase: "计算机组成原理".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
                ProgressNode {
                    id: "n1".to_string(),
                    title: "线性表".to_string(),
                    level: NodeLevel::Knowledge,
                    parent_id: Some(dup_id.clone()),
                    phase: "数据结构".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
                ProgressNode {
                    id: "n2".to_string(),
                    title: "存储系统".to_string(),
                    level: NodeLevel::Knowledge,
                    parent_id: Some(dup_id.clone()),
                    phase: "计算机组成原理".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
            ],
        }
    }

    #[test]
    fn repair_fixes_duplicate_ids_and_keeps_parent_relation() {
        let mut table = make_dup_table();
        let fixed = repair_table_ids(&mut table);
        assert!(fixed);

        let mut ids = std::collections::HashSet::new();
        for n in &table.nodes {
            assert!(ids.insert(n.id.clone()), "修复后不应有重复 id: {}", n.id);
        }

        let chapters: Vec<_> = table
            .nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Chapter)
            .collect();
        assert_eq!(chapters.len(), 2);
        assert_ne!(chapters[0].id, chapters[1].id);

        let ds = chapters.iter().find(|c| c.title == "数据结构").unwrap();
        let co = chapters
            .iter()
            .find(|c| c.title == "计算机组成原理")
            .unwrap();
        let linear = table.nodes.iter().find(|n| n.title == "线性表").unwrap();
        let storage = table.nodes.iter().find(|n| n.title == "存储系统").unwrap();

        assert_eq!(linear.parent_id.as_ref(), Some(&ds.id));
        assert_eq!(storage.parent_id.as_ref(), Some(&co.id));
    }

    #[test]
    fn repair_noop_when_ids_unique() {
        let mut table = ProgressTable {
            id: "t2".to_string(),
            subject: "math".to_string(),
            variant: "数二".to_string(),
            name: "数学表".to_string(),
            origin: TableOrigin::Builtin,
            created_at: String::new(),
            updated_at: String::new(),
            nodes: vec![
                ProgressNode {
                    id: "c1".to_string(),
                    title: "高数".to_string(),
                    level: NodeLevel::Chapter,
                    parent_id: None,
                    phase: "高数".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
                ProgressNode {
                    id: "n1".to_string(),
                    title: "极限".to_string(),
                    level: NodeLevel::Knowledge,
                    parent_id: Some("c1".to_string()),
                    phase: "高数".to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                },
            ],
        };
        let fixed = repair_table_ids(&mut table);
        assert!(!fixed);
        assert_eq!(table.nodes[0].id, "c1");
        assert_eq!(table.nodes[1].id, "n1");
    }
}
