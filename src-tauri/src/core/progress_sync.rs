//! progress_sync — 进度表与学习状态(State)/复盘(Review) 的联动
//!
//! 1. `sync_review_to_progress`：每日复盘提交后，把「今日完成任务」对应的知识点节点
//!    升级为「基础 / 掌握」；并把「次日计划任务」对应的知识点节点预置为「学习中 / 强化中」。
//!    全程只升不降（`NodeStatus::max_with`），避免覆盖用户已手动推进的状态。
//! 2. `estimate_from_state`：首次打开进度页时，根据 State 中学科目进度
//!    （`completed` / `current_focus`）估算知识点当前应处的状态，供前端弹窗确认。
//! 3. `apply_estimated_statuses`：把用户在确认弹窗里勾选的结果批量落盘。
//! 4. `default_progress_variants`：把设置中的 `exam_type` 解析为各科默认考纲方案。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::data::plan::DailyPlanFile;
use crate::data::progress_tables::{
    load_progress_index, parse_node_status, save_progress_index, NodeLevel, NodeStatus,
    ProgressIndex, ProgressNode,
};
use crate::data::state::{read_state_or_default, StudyState, SubjectKey, SubjectState};

/// 科目进度表 key（state / plan 均为 snake_case，如 math）
pub fn progress_subject_key(subject: &str) -> String {
    subject.trim().to_lowercase()
}

fn subject_key_label(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "math",
        SubjectKey::English => "english",
        SubjectKey::Politics => "politics",
        SubjectKey::Professional => "professional",
    }
}

/// 从 State 取某科目状态
fn subject_state_of<'a>(state: &'a StudyState, subject: &str) -> Option<&'a SubjectState> {
    match subject {
        "math" => Some(&state.subjects.math),
        "english" => Some(&state.subjects.english),
        "politics" => Some(&state.subjects.politics),
        "professional" => Some(&state.subjects.professional),
        _ => None,
    }
}

/// 归一化：仅保留字母/数字/汉字（含下划线），转小写（用于标题/章节模糊匹配）
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// `b` 是否作为子串出现在 `a` 中（两侧均需至少 2 个字符，避免过短误配）
fn contains(a: &str, b: &str) -> bool {
    let b = b.trim();
    if b.is_empty() || b.chars().count() < 2 {
        return false;
    }
    a.contains(b)
}

/// 节点是否命中任务标题/章节条目（标题或章节名双向包含匹配）
pub fn node_matches(node: &ProgressNode, chapter_title: &str, haystack: &str) -> bool {
    let h = norm(haystack);
    if h.is_empty() {
        return false;
    }
    let t = norm(&node.title);
    let c = norm(chapter_title);
    contains(&h, &t) || contains(&t, &h) || contains(&h, &c) || contains(&c, &h)
}

/// 去掉任务标题开头的学科前缀（如「数学｜」「英语：」），提高匹配命中率
fn strip_subject_prefix(title: &str) -> String {
    let labels = ["数学", "英语", "政治", "专业课"];
    let mut t = title.trim().to_string();
    for _ in 0..3 {
        let mut cut = None;
        for l in &labels {
            if let Some(rest) = t.strip_prefix(*l) {
                if rest.starts_with('|')
                    || rest.starts_with('：')
                    || rest.starts_with(':')
                    || rest.starts_with('_')
                    || rest.starts_with('-')
                    || rest.starts_with('/')
                    || rest.starts_with('（')
                    || rest.starts_with('(')
                {
                    cut = Some(rest.trim_start());
                    break;
                }
            }
        }
        match cut {
            Some(rest) => t = rest.to_string(),
            None => break,
        }
    }
    t
}

/// 复盘联动的任务输入
#[derive(Debug, Clone)]
pub struct ReviewSyncTask {
    pub subject: String,
    pub title: String,
    /// 今日是否完成（completed / partial 均视为完成）
    pub completed: bool,
    /// 掌握程度：mastered / basic / weak / 空
    pub mastery: String,
}

/// 在指定科目的进度表中，把匹配到任务标题的知识点升级为目标状态。
/// 只修改当前启用进度表（active_id），避免跨方案/跨表污染。
/// 返回修改的节点数。
fn apply_task_to_subject(
    index: &mut ProgressIndex,
    subject: &str,
    task_title: &str,
    target: NodeStatus,
) -> usize {
    let task_title = strip_subject_prefix(task_title);
    if task_title.chars().count() < 2 {
        return 0;
    }
    let Some(set) = index.subjects.get_mut(subject) else {
        return 0;
    };

    // 仅处理当前启用表；active_id 缺失时取当前启用方案的第一张表
    let active_table = if !set.active_id.is_empty() {
        set.tables.iter_mut().find(|t| t.id == set.active_id)
    } else {
        None
    };
    let active_table = match active_table {
        Some(t) => Some(t),
        None => {
            if set.active_variant.is_empty() {
                set.tables.first_mut()
            } else {
                set.tables
                    .iter_mut()
                    .find(|t| t.variant == set.active_variant)
            }
        }
    };
    let Some(table) = active_table else {
        return 0;
    };

    // 若启用表 variant 与 active_variant 不一致（切换中），跳过避免写错方案
    if !set.active_variant.is_empty() && table.variant != set.active_variant {
        return 0;
    }

    let mut changed = 0;
    for node in table.nodes.iter_mut() {
        if node.level != NodeLevel::Knowledge {
            continue;
        }
        if !node_matches(node, &node.phase, &task_title) {
            continue;
        }
        let new_s = node.status.max_with(target);
        if new_s != node.status {
            node.status = new_s;
            changed += 1;
        }
    }
    if changed > 0 {
        table.updated_at = crate::data::now_string();
    }
    changed
}

/// 复盘提交后同步进度表：
/// - 今日完成的任务 → 对应知识点：mastered→掌握，basic/weak→基础
/// - 次日计划的每个任务 → 对应知识点：预置为「强化中」（今日困难或存在掌握不足）
///   否则「学习中」；只升不降。
/// 返回变更节点总数。
pub fn sync_review_to_progress(
    data_dir: &Path,
    tasks: &[ReviewSyncTask],
    feeling: &str,
    tomorrow_plan: Option<&DailyPlanFile>,
) -> Result<usize, String> {
    let mut index = load_progress_index(data_dir);
    let mut changed = 0usize;

    let has_weak = tasks.iter().any(|t| t.completed && t.mastery == "weak");
    let hard = feeling.trim() == "hard";

    // 今日已完成 → 基础 / 掌握
    for t in tasks {
        if !t.completed {
            continue;
        }
        let target = if t.mastery == "mastered" {
            NodeStatus::Mastered
        } else {
            NodeStatus::Basic
        };
        changed += apply_task_to_subject(
            &mut index,
            &progress_subject_key(&t.subject),
            &t.title,
            target,
        );
    }

    // 次日计划 → 学习中 / 强化中
    if let Some(plan) = tomorrow_plan {
        let target = if hard || has_weak {
            NodeStatus::Reinforcing
        } else {
            NodeStatus::Learning
        };
        for task in &plan.data.tasks {
            changed += apply_task_to_subject(
                &mut index,
                subject_key_label(&task.subject),
                &task.title,
                target,
            );
        }
    }

    if changed > 0 {
        save_progress_index(data_dir, &index)?;
    }
    Ok(changed)
}

/// 首次打开时的状态预估条目
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StatusEstimate {
    pub table_id: String,
    pub table_name: String,
    pub chapter_id: Option<String>,
    /// 章节标题（无章节节点时取 phase）
    pub chapter: String,
    pub node_id: String,
    pub node_title: String,
    /// 建议状态（pending/learning/basic/reinforcing/mastered）
    pub suggested: String,
}

/// 根据 State 预估算法的状态：
/// - `completed` 中的章节 → 建议「基础」
/// - `current_focus`（正在学的章节）→ 建议「学习中」
/// 仅对「当前启用方案」的表生成建议，避免跨方案污染。
pub fn estimate_from_state(data_dir: &Path, subject: &str) -> Result<Vec<StatusEstimate>, String> {
    let state = read_state_or_default(data_dir);
    let Some(s_state) = subject_state_of(&state, subject) else {
        return Ok(Vec::new());
    };

    let mut reached: Vec<String> = s_state.completed.clone();
    if !s_state.current_focus.trim().is_empty() && !reached.contains(&s_state.current_focus) {
        reached.push(s_state.current_focus.clone());
    }
    if reached.is_empty() {
        return Ok(Vec::new());
    }

    let index = load_progress_index(data_dir);
    let Some(set) = index.subjects.get(subject) else {
        return Ok(Vec::new());
    };

    // 只处理当前启用表；未设置启用表时取当前启用方案的第一张表
    let table = if !set.active_id.is_empty() {
        set.tables.iter().find(|t| t.id == set.active_id)
    } else {
        None
    };
    let table = match table {
        Some(t) => Some(t),
        None => {
            if set.active_variant.is_empty() {
                set.tables.first()
            } else {
                set.tables.iter().find(|t| t.variant == set.active_variant)
            }
        }
    };
    let Some(table) = table else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for node in &table.nodes {
        if node.level != NodeLevel::Knowledge {
            continue;
        }
        for entry in &reached {
            if entry.trim().is_empty() {
                continue;
            }
            if !node_matches(node, &node.phase, entry) {
                continue;
            }
            let is_focus =
                entry.trim() == s_state.current_focus.trim() && !s_state.completed.contains(entry);
            let suggested = if is_focus {
                NodeStatus::Learning
            } else {
                NodeStatus::Basic
            };
            out.push(StatusEstimate {
                table_id: table.id.clone(),
                table_name: table.name.clone(),
                chapter_id: node.parent_id.clone(),
                chapter: node.phase.clone(),
                node_id: node.id.clone(),
                node_title: node.title.clone(),
                suggested: suggested.as_str().to_string(),
            });
            break; // 一个节点只给一条建议
        }
    }
    Ok(out)
}

/// 批量应用确认结果（只升不降）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StatusChange {
    pub table_id: String,
    pub node_id: String,
    pub status: String,
}

pub fn apply_estimated_statuses(
    data_dir: &Path,
    subject: &str,
    changes: &[StatusChange],
) -> Result<usize, String> {
    let mut index = load_progress_index(data_dir);
    let mut changed = 0usize;
    if let Some(set) = index.subjects.get_mut(subject) {
        for ch in changes {
            let target = parse_node_status(&ch.status);
            let Some(table) = set.tables.iter_mut().find(|t| t.id == ch.table_id) else {
                continue;
            };
            let Some(node) = table.nodes.iter_mut().find(|n| n.id == ch.node_id) else {
                continue;
            };
            let new_s = node.status.max_with(target);
            if new_s != node.status {
                node.status = new_s;
                changed += 1;
            }
        }
        if changed > 0 {
            for t in set.tables.iter_mut() {
                if changes.iter().any(|c| c.table_id == t.id) {
                    t.updated_at = crate::data::now_string();
                }
            }
        }
    }
    if changed > 0 {
        save_progress_index(data_dir, &index)?;
    }
    Ok(changed)
}

/// 把设置中的 `exam_type` 解析为「科目 → 默认考纲方案」，用于：
/// - ProgressView 未设置启用方案时按用户考试类型自动选中
/// - 只对用户选择/输入的科目自动同步内置考纲表
pub fn default_progress_variants(exam_type: &str) -> HashMap<String, String> {
    let et = exam_type.trim();
    let mut m = HashMap::new();
    if et.is_empty() {
        return m;
    }

    // 数学：数三 > 数二 > 数一 > 含「数」
    if et.contains("数三") || et.contains("数学三") {
        m.insert("math".to_string(), "数三".to_string());
    } else if et.contains("数二") || et.contains("数学二") {
        m.insert("math".to_string(), "数二".to_string());
    } else if et.contains("数一") || et.contains("数学一") || et.contains("数") {
        m.insert("math".to_string(), "数一".to_string());
    }

    // 英语
    if et.contains("英语二") || et.contains("英二") {
        m.insert("english".to_string(), "英二".to_string());
    } else if et.contains("英语一") || et.contains("英一") || et.contains("英语") {
        m.insert("english".to_string(), "英一".to_string());
    }

    // 政治
    if et.contains("政治") {
        m.insert("politics".to_string(), "政治".to_string());
    }

    // 专业课（数字关键词优先，避免与中文描述误配）
    for (kw, v) in [
        ("408", "408 计算机"),
        ("计算机", "408 计算机"),
        ("307", "307 中医"),
        ("中医", "307 中医"),
        ("311", "311 教育学"),
        ("教育学", "311 教育学"),
        ("312", "312 心理学"),
        ("心理学", "312 心理学"),
        ("313", "313 历史学"),
        ("历史学", "313 历史学"),
        ("333", "333 教育综合"),
        ("教育综合", "333 教育综合"),
        ("396", "396 经济类"),
        ("经济类", "396 经济类"),
        ("199", "199 管理类"),
        ("管理类", "199 管理类"),
        ("306", "306 西医"),
        ("西医", "306 西医"),
        ("法硕", "法律硕士"),
        ("法律硕士", "法律硕士"),
    ] {
        if et.contains(kw) {
            m.insert("professional".to_string(), v.to_string());
            break;
        }
    }

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_strips_punctuation_case() {
        let a = norm("第三章 网络层，IP编址");
        let b = norm("第三章·网络层  IP 编址");
        assert_eq!(a, b);
        assert!(a.contains("网络层"));
    }

    #[test]
    fn node_matches_by_chapter_and_title() {
        let n = ProgressNode {
            id: "n1".into(),
            title: "IP 编址".into(),
            level: NodeLevel::Knowledge,
            parent_id: Some("c1".into()),
            phase: "网络层".into(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        };
        assert!(node_matches(
            &n,
            &n.phase,
            "第四章 网络层：重点复习 IP 编址"
        ));
        assert!(node_matches(&n, &n.phase, "IP编址与子网划分"));
        assert!(!node_matches(&n, &n.phase, "图的最短路径"));
    }

    #[test]
    fn short_strings_never_match() {
        let n = ProgressNode {
            id: "n2".into(),
            title: "树".into(),
            level: NodeLevel::Knowledge,
            parent_id: None,
            phase: "图".into(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        };
        assert!(!node_matches(&n, &n.phase, "树与二叉树（二叉树遍历）"));
    }

    #[test]
    fn default_variants_parses_exam_type() {
        let m = default_progress_variants("数学二、英语一、政治、408计算机综合");
        assert_eq!(m.get("math").map(|s| s.as_str()), Some("数二"));
        assert_eq!(m.get("english").map(|s| s.as_str()), Some("英一"));
        assert_eq!(m.get("politics").map(|s| s.as_str()), Some("政治"));
        assert_eq!(
            m.get("professional").map(|s| s.as_str()),
            Some("408 计算机")
        );

        let m2 = default_progress_variants("");
        assert!(m2.is_empty());
    }
}
