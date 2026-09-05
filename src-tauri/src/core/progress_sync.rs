//! progress_sync — 进度表与学习状态(State)/复盘(Review) 的联动
//!
//! 1. `sync_review_to_progress`：每日复盘提交后，把「今日完成任务」对应的知识点节点
//!    升级为「基础 / 掌握」；并把「次日计划任务」对应的知识点节点预置为「学习中 / 强化中」。
//!    全程只升不降（`NodeStatus::max_with`），避免覆盖用户已手动推进的状态。
//! 2. `estimate_from_state`：首次打开进度页时，根据 State 中学科目进度
//!    （`completed` / `current_focus`）估算知识点当前应处的状态，供前端弹窗确认。
//! 3. `apply_estimated_statuses`：把用户在确认弹窗里勾选的结果批量落盘。
//! 4. `default_progress_variants`：把设置中的 `exam_type` 解析为各科默认考纲方案。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::data::plan::DailyPlanFile;
use crate::data::progress_tables::{
    load_progress_index, parse_node_status, save_progress_index, NodeLevel, NodeStatus,
    ProgressIndex, ProgressNode, ProgressTable,
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
///   返回变更节点总数。
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
///   仅对「当前启用方案」的表生成建议，避免跨方案污染。
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

// ============================================================================
// 批量更改进度（整表按「学到第几章」向前推进 + 总专业课进度表联动）
// ============================================================================

/// 单张进度表的「学到第几章」覆盖输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TableCoverage {
    pub table_id: String,
    /// 当前学到哪一章（章节节点 id）；None = 本轮未推进该表
    pub reached_chapter: Option<String>,
    /// 当前章是否整章学完（true 时忽略 current_points，整章覆盖）
    pub current_full: bool,
    /// 当前章内已学到的知识点 id（current_full=false 时生效）
    pub current_points: Vec<String>,
}

/// 某科某轮（基础/强化）的批量覆盖输入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchRoundUpdate {
    pub subject: String,
    /// 本轮目标状态："basic" / "reinforcing"（也接受 learning/mastered）
    pub round: String,
    pub tables: Vec<TableCoverage>,
}

/// 批量更改进度的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchRoundResult {
    /// 状态发生变化的表数（含被联动更新的总专业课进度表）
    pub tables_updated: usize,
    /// 被推进状态的知识点/章节节点总数（只升不降）
    pub nodes_changed: usize,
}

/// 批量更改进度：对每张表按「本章及之前全部覆盖，当前章勾选部分覆盖」向前推进（只升不降）。
/// 专业课内置场景下，总专业课进度表的节点再由各教材覆盖度自动推导（书本更改 → 总表联动）。
pub fn apply_batch_round(
    data_dir: &Path,
    updates: &[BatchRoundUpdate],
) -> Result<BatchRoundResult, String> {
    let mut index = load_progress_index(data_dir);
    let mut tables_updated = 0usize;
    let mut nodes_changed = 0usize;

    for u in updates {
        let target = parse_node_status(&u.round);
        if target == NodeStatus::Pending {
            continue; // 本轮的无效目标状态：跳过
        }

        // 1) 各表「学到第几章」覆盖推进（只升不降）
        {
            let Some(set) = index.subjects.get_mut(&u.subject) else {
                continue;
            };
            for tc in &u.tables {
                let Some(table) = set.tables.iter_mut().find(|t| t.id == tc.table_id) else {
                    continue;
                };
                let changed = apply_table_coverage(table, tc, target);
                if changed > 0 {
                    table.updated_at = crate::data::now_string();
                    tables_updated += 1;
                    nodes_changed += changed;
                }
            }
        }

        // 2) 总专业课进度表联动：本次更新涉及的各方案，按教材覆盖度重新推导总表
        let variants: Vec<String> = {
            let set = index.subjects.get(&u.subject);
            let mut v: Vec<String> = Vec::new();
            if let Some(set) = set {
                for tc in &u.tables {
                    if let Some(t) = set.tables.iter().find(|t| t.id == tc.table_id) {
                        if !t.variant.is_empty() && !v.contains(&t.variant) {
                            v.push(t.variant.clone());
                        }
                    }
                }
            }
            v
        };
        for variant in &variants {
            let changed = apply_master_derivation(&mut index, &u.subject, variant, target);
            if changed > 0 {
                tables_updated += 1;
                nodes_changed += changed;
            }
        }
    }

    if tables_updated > 0 || nodes_changed > 0 {
        save_progress_index(data_dir, &index)?;
    }
    Ok(BatchRoundResult {
        tables_updated,
        nodes_changed,
    })
}

/// 单表覆盖推进：覆盖知识点 → max(现状, target)；未覆盖不变。
/// 章节节点：其下知识点全部将 ≥ target → max(现状, target)，否则不变。
fn apply_table_coverage(
    table: &mut ProgressTable,
    tc: &TableCoverage,
    target: NodeStatus,
) -> usize {
    // 章节顺序 = 存储顺序；收集「章节 id → 知识点 id」
    let mut chapter_ids: Vec<String> = Vec::new();
    let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
    for n in &table.nodes {
        if n.level == NodeLevel::Chapter {
            chapter_ids.push(n.id.clone());
        } else if let Some(pid) = &n.parent_id {
            children_of
                .entry(pid.clone())
                .or_default()
                .push(n.id.clone());
        }
    }

    let Some(ri) = tc
        .reached_chapter
        .as_ref()
        .and_then(|id| chapter_ids.iter().position(|c| c == id))
    else {
        return 0; // 未选择「学到第几章」：本轮不处理该表
    };

    // 覆盖集合 = 之前章节全部 + 当前章（整章或按勾选知识点）
    let mut covered: HashSet<String> = HashSet::new();
    for cid in chapter_ids.iter().take(ri) {
        if let Some(kids) = children_of.get(cid) {
            covered.extend(kids.iter().cloned());
        }
    }
    if let Some(cur) = chapter_ids.get(ri) {
        if tc.current_full {
            if let Some(kids) = children_of.get(cur) {
                covered.extend(kids.iter().cloned());
            }
        } else {
            covered.extend(tc.current_points.iter().cloned());
        }
    }

    // 预判：各章节是否「全部知识点将 ≥ target」（被本轮覆盖或原本已达标）
    let mut full_chapters: HashSet<&str> = HashSet::new();
    for (cid, kids) in &children_of {
        if kids.is_empty() {
            continue;
        }
        let all = kids.iter().all(|kid| {
            if covered.contains(kid) {
                return true;
            }
            table
                .nodes
                .iter()
                .find(|n| n.id == *kid)
                .map(|n| n.status.rank() >= target.rank())
                .unwrap_or(false)
        });
        if all {
            full_chapters.insert(cid.as_str());
        }
    }

    let mut changed = 0usize;
    for n in table.nodes.iter_mut() {
        if n.level == NodeLevel::Knowledge {
            if covered.contains(&n.id) {
                let ns = n.status.max_with(target);
                if ns != n.status {
                    n.status = ns;
                    changed += 1;
                }
            }
        } else if full_chapters.contains(n.id.as_str()) {
            let ns = n.status.max_with(target);
            if ns != n.status {
                n.status = ns;
                changed += 1;
            }
        }
    }
    changed
}

/// 总专业课进度表联动：某方案下，按各教材当前覆盖度（rank ≥ target 的知识点占比）
/// 推导总表每个板块的推进量 —— 板块内前 `round(fraction × K)` 个知识点推进，其余不变；
/// 板块章节节点在板块全部覆盖时才推进。返回变更节点数。
fn apply_master_derivation(
    index: &mut ProgressIndex,
    subject: &str,
    variant: &str,
    target: NodeStatus,
) -> usize {
    let Some(exam) = crate::core::professional::find(variant) else {
        return 0;
    };
    if exam.books.is_empty() {
        return 0;
    }
    let master_prefix = format!("{} · 总专业课进度表", exam.name);

    // 1) 统计每本内置教材表（匹配考书名）当前的知识点覆盖度（rank ≥ target）
    let mut idx_of: HashMap<&str, usize> = HashMap::new();
    for (i, b) in exam.books.iter().enumerate() {
        idx_of.insert(b.name, i);
    }
    let mut book_cov: HashMap<usize, (usize, usize)> = HashMap::new(); // book_idx -> (advanced, total)
    if let Some(set) = index.subjects.get(subject) {
        for t in &set.tables {
            if t.variant != variant {
                continue;
            }
            let Some(rest) = t.name.strip_prefix(&format!("{} · 教材：", exam.name)) else {
                continue;
            };
            let Some(&bi) = idx_of.get(rest) else {
                continue;
            };
            let nodes: Vec<&ProgressNode> = t
                .nodes
                .iter()
                .filter(|n| n.level == NodeLevel::Knowledge)
                .collect();
            let adv = nodes
                .iter()
                .filter(|n| n.status.rank() >= target.rank())
                .count();
            let entry = book_cov.entry(bi).or_insert((0, 0));
            entry.0 += adv;
            entry.1 += nodes.len();
        }
    }
    if book_cov.is_empty() {
        return 0;
    }

    // 2) 板块 → 覆盖度（教材按知识点数加权平均）
    let mut sec_avg: HashMap<&'static str, f64> = HashMap::new(); // section phase -> 覆盖度
    let mut sec_cnt: HashMap<&'static str, usize> = HashMap::new();
    for (bi, (adv, total)) in &book_cov {
        let frac = if *total == 0 {
            0.0
        } else {
            *adv as f64 / *total as f64
        };
        for link in exam.books[*bi].master_links {
            let e = sec_avg.entry(link).or_insert(0.0);
            *e += frac;
            *sec_cnt.entry(link).or_insert(0) += 1;
        }
    }

    // 3) 更新总表：板块内前 round(fraction × K) 个知识点 → max(现状, target)
    let Some(set) = index.subjects.get_mut(subject) else {
        return 0;
    };
    let Some(master) = set
        .tables
        .iter_mut()
        .find(|t| t.variant == variant && t.name == master_prefix)
    else {
        return 0;
    };

    let mut kids_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut chapter_of_phase: HashMap<&'static str, String> = HashMap::new();
    for n in &master.nodes {
        if n.level == NodeLevel::Chapter {
            if let Some(s) = exam.master.iter().find(|s| s.phase == n.phase) {
                chapter_of_phase.insert(s.phase, n.id.clone());
            }
        } else if let Some(pid) = &n.parent_id {
            kids_of.entry(pid.clone()).or_default().push(n.id.clone());
        }
    }

    let mut changed = 0usize;
    for section in exam.master {
        let (&sum, &n) = match (sec_avg.get(section.phase), sec_cnt.get(section.phase)) {
            (Some(s), Some(n)) => (s, n),
            _ => continue, // 该板块无教材登记：不动
        };
        let Some(cid) = chapter_of_phase.get(section.phase) else {
            continue;
        };
        let kids = kids_of.get(cid).cloned().unwrap_or_default();
        if kids.is_empty() {
            continue;
        }
        let frac = sum / n as f64;
        let k = kids.len() as f64;
        let n_adv = (k * frac).round().min(k) as usize;

        // 板块内前 n_adv 个知识点 → max(现状, target)
        for (i, kid_id) in kids.iter().enumerate() {
            if i >= n_adv {
                break;
            }
            if let Some(n) = master.nodes.iter_mut().find(|n| n.id == *kid_id) {
                let ns = n.status.max_with(target);
                if ns != n.status {
                    n.status = ns;
                    changed += 1;
                }
            }
        }
        // 有推进但不足以推进最前一项到目标状态（如只学到第一章第几节）：把最前
        // 一项标记为「学习中」，让总表体现「正在学」的阶段，而不是停留在待学。
        if frac > 0.0 && n_adv == 0 {
            if let Some(kid_id) = kids.first() {
                if let Some(n) = master.nodes.iter_mut().find(|n| n.id == *kid_id) {
                    let ns = n.status.max_with(NodeStatus::Learning);
                    if ns != n.status {
                        n.status = ns;
                        changed += 1;
                    }
                }
            }
        }
        // 板块章节节点：聚合其下知识点当前最高状态（部分推进 → 学习中，全推进 → target）
        let max_item = kids.iter().fold(NodeStatus::Pending, |acc, kid_id| {
            master
                .nodes
                .iter()
                .find(|n| n.id == *kid_id)
                .map(|n| acc.max_with(n.status))
                .unwrap_or(acc)
        });
        if let Some(ch) = master.nodes.iter_mut().find(|n| n.id == *cid) {
            let ns = ch.status.max_with(max_item);
            if ns != ch.status {
                ch.status = ns;
                changed += 1;
            }
        }
    }
    if changed > 0 {
        master.updated_at = crate::data::now_string();
    }
    changed
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
            estimated_hours: None,
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
            estimated_hours: None,
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

    // ── 批量更改进度 ──

    /// 构造知识点 + 章节的两级节点表
    fn table_with_chapters(chapters: &[&str], per_chapter: usize) -> ProgressTable {
        use crate::data::progress_tables::new_progress_id;
        let mut nodes = Vec::new();
        for (ci, ch_title) in chapters.iter().enumerate() {
            let cid = new_progress_id("c", &format!("c{}", ci));
            nodes.push(ProgressNode {
                id: cid.clone(),
                title: ch_title.to_string(),
                level: NodeLevel::Chapter,
                parent_id: None,
                phase: ch_title.to_string(),
                status: NodeStatus::Pending,
                planned_date: None,
                note: String::new(),
                estimated_hours: None,
            });
            for pi in 0..per_chapter {
                nodes.push(ProgressNode {
                    id: new_progress_id("n", &format!("{}-{}", ci, pi)),
                    title: format!("知识点 {}-{}", ci, pi),
                    level: NodeLevel::Knowledge,
                    parent_id: Some(cid.clone()),
                    phase: ch_title.to_string(),
                    status: NodeStatus::Pending,
                    planned_date: None,
                    note: String::new(),
                    estimated_hours: None,
                });
            }
        }
        ProgressTable {
            id: "t1".into(),
            subject: "math".into(),
            variant: "数二".into(),
            name: "测试表".into(),
            origin: crate::data::progress_tables::TableOrigin::Custom,
            created_at: String::new(),
            updated_at: String::new(),
            nodes,
        }
    }

    fn chapter_ids(t: &ProgressTable) -> Vec<String> {
        t.nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Chapter)
            .map(|n| n.id.clone())
            .collect()
    }

    #[test]
    fn batch_table_coverage_marks_before_reached_chapter() {
        let mut t = table_with_chapters(&["第一章", "第二章", "第三章"], 2);
        let chs = chapter_ids(&t);
        // 布局：ch0, k00, k01, ch1, k10, k11, ch2, k20, k21（每章 2 个知识点）
        // 学到第二章：第一章全部 + 第二章勾选第 1 个知识点（k10 = index 4）
        let tc = TableCoverage {
            table_id: "t1".into(),
            reached_chapter: Some(chs[1].clone()),
            current_full: false,
            current_points: vec![t.nodes[4].id.clone()], // k10
        };
        let changed = apply_table_coverage(&mut t, &tc, NodeStatus::Basic);
        assert_eq!(changed, 4, "第一章 2 个 + k10 + 第一章章节节点 = 4 处推进");
        // 第一章的知识点已为基础
        let s0 = t
            .nodes
            .iter()
            .find(|n| n.id == t.nodes[2].id)
            .unwrap()
            .status;
        assert_eq!(s0, NodeStatus::Basic);
        // 第二章未勾选的另一个知识点（k11 = index 5）保持待学
        let s4 = t
            .nodes
            .iter()
            .find(|n| n.id == t.nodes[5].id)
            .unwrap()
            .status;
        assert_eq!(s4, NodeStatus::Pending);
        // 第三章保持待学
        assert!(t
            .nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Knowledge)
            .skip(4)
            .all(|n| n.status == NodeStatus::Pending));
    }

    #[test]
    fn batch_table_coverage_full_chapter_and_no_downgrade() {
        let mut t = table_with_chapters(&["第一章", "第二章"], 2);
        let chs = chapter_ids(&t);
        // 先把某知识点手动设为「掌握」
        let mastered_idx = 1; // 第一章第 2 个知识点
        t.nodes[mastered_idx].status = NodeStatus::Mastered;
        // 学到第二章且整章学完；但目标为「基础」——已掌握的不能被降级
        let tc = TableCoverage {
            table_id: "t1".into(),
            reached_chapter: Some(chs[1].clone()),
            current_full: true,
            current_points: vec![],
        };
        let changed = apply_table_coverage(&mut t, &tc, NodeStatus::Basic);
        // k01/k10/k11 →基础（3），两章章节节点全覆盖 →基础（2）＝ 5 处；
        // k00 已「掌握」不会被降级
        assert_eq!(changed, 5);
        assert_eq!(t.nodes[0].status, NodeStatus::Basic); // 第一章章节节点
        assert_eq!(t.nodes[1].status, NodeStatus::Mastered); // 已掌握不被降级
        assert_eq!(t.nodes[2].status, NodeStatus::Basic);
        assert_eq!(t.nodes[3].status, NodeStatus::Basic); // 第二章章节节点
    }

    #[test]
    fn batch_master_partial_learning_marks_learning_status() {
        use crate::core::professional::{build_tables, find};
        let exam = find("408计算机").expect("408 应可识别");
        let mut index = ProgressIndex::default();
        {
            let set = index
                .subjects
                .entry("professional".to_string())
                .or_default();
            set.active_variant = exam.short.to_string();
            let mut tables = build_tables(&exam);
            for t in tables.iter_mut() {
                t.id = crate::data::progress_tables::new_progress_id("p", &t.name);
            }
            set.tables = tables;
        }

        // 操作系统教材（books[2]）：只学到第一章第 1 个知识点，本轮 = 学习中
        let book_name = format!("{} · 教材：{}", exam.name, exam.books[2].name);
        let (book_id, first_kid) = {
            let set = index.subjects.get("professional").unwrap();
            let book = set.tables.iter().find(|t| t.name == book_name).unwrap();
            let first: &ProgressNode = book
                .nodes
                .iter()
                .find(|n| n.level == NodeLevel::Chapter)
                .unwrap();
            let kid = book
                .nodes
                .iter()
                .find(|n| {
                    n.level == NodeLevel::Knowledge
                        && n.parent_id.as_deref() == Some(first.id.as_str())
                })
                .unwrap();
            (book.id.clone(), kid.id.clone())
        };
        {
            let set = index.subjects.get_mut("professional").unwrap();
            let book = set.tables.iter_mut().find(|t| t.id == book_id).unwrap();
            let chs = chapter_ids(book);
            apply_table_coverage(
                book,
                &TableCoverage {
                    table_id: book_id.clone(),
                    reached_chapter: Some(chs[0].clone()),
                    current_full: false,
                    current_points: vec![first_kid],
                },
                NodeStatus::Learning,
            );
        }

        let changed = apply_master_derivation(
            &mut index,
            "professional",
            &exam.short,
            NodeStatus::Learning,
        );
        assert!(changed > 0, "部分学习也应推动总表进入「学习中」状态");

        let master = index
            .subjects
            .get("professional")
            .and_then(|s| s.tables.iter().find(|t| t.name.contains("总专业课进度表")))
            .unwrap();
        let os_chapter = master
            .nodes
            .iter()
            .find(|n| n.level == NodeLevel::Chapter && n.phase == "操作系统")
            .unwrap();
        // 板块章节节点聚合其知识点状态 → 学习中
        assert_eq!(os_chapter.status, NodeStatus::Learning);
        // 板块内最前一项 → 学习中（体现「正在学」阶段，而非停留在待学）
        let first_item = master
            .nodes
            .iter()
            .find(|n| {
                n.level == NodeLevel::Knowledge
                    && n.parent_id.as_deref() == Some(os_chapter.id.as_str())
            })
            .unwrap();
        assert_eq!(first_item.status, NodeStatus::Learning);
    }

    #[test]
    fn batch_master_derivation_follows_book_coverage() {
        use crate::core::professional::{build_tables, find};
        let exam = find("408计算机").expect("408 应可识别");
        let mut index = ProgressIndex::default();
        {
            let set = index
                .subjects
                .entry("professional".to_string())
                .or_default();
            set.active_variant = exam.short.to_string();
            // 总表 + 教材表入库（id 分配）
            let mut tables = build_tables(&exam);
            for t in tables.iter_mut() {
                t.id = crate::data::progress_tables::new_progress_id("p", &t.name);
            }
            set.tables = tables;
        }

        // 数据结构教材：学到最后一章（整本书覆盖，基础）；其余教材保持待学
        let book_name = format!("{} · 教材：{}", exam.name, exam.books[0].name);
        let (book_id, reached) = {
            let set = index.subjects.get("professional").unwrap();
            let book = set.tables.iter().find(|t| t.name == book_name).unwrap();
            let chs = chapter_ids(book);
            (book.id.clone(), chs.last().expect("教材应有章节").clone())
        };
        {
            let set = index.subjects.get_mut("professional").unwrap();
            let book = set.tables.iter_mut().find(|t| t.id == book_id).unwrap();
            apply_table_coverage(
                book,
                &TableCoverage {
                    table_id: book_id.clone(),
                    reached_chapter: Some(reached),
                    current_full: true,
                    current_points: vec![],
                },
                NodeStatus::Basic,
            );
        }

        let changed =
            apply_master_derivation(&mut index, "professional", &exam.short, NodeStatus::Basic);
        assert!(changed > 0, "总表应随教材推进而变化");

        // 数据结构板块：整本书覆盖 → 该板块全部知识点为基础，板块章节节点同步推进
        let master = index
            .subjects
            .get("professional")
            .and_then(|s| s.tables.iter().find(|t| t.name.contains("总专业课进度表")))
            .unwrap();
        let data_struct_chapter = master
            .nodes
            .iter()
            .find(|n| n.level == NodeLevel::Chapter && n.phase == "数据结构")
            .unwrap();
        assert_eq!(data_struct_chapter.status, NodeStatus::Basic);
        assert!(master
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some(data_struct_chapter.id.as_str()))
            .all(|n| n.status == NodeStatus::Basic));
        // 其他板块（计算机组成原理，对应教材未推进）不受影响
        let other_chapter = master
            .nodes
            .iter()
            .find(|n| n.level == NodeLevel::Chapter && n.phase == "计算机组成原理")
            .unwrap();
        assert!(master
            .nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == Some(other_chapter.id.as_str()))
            .all(|n| n.status == NodeStatus::Pending));
    }
}
