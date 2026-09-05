//! Progress Table Commands — 各科「进度表」命令
//!
//! 提供进度表的 列表 / 保存(增改) / 删除 / 启用切换 / AI 生成 / 联网搜索配置。
//! AI 生成默认以内置官方考研考纲（core::chapter_seq）为依据；当用户在设置中启用了
//! 联网搜索（WebSearchConfig）时，会先联网拉取「最新考研大纲」相关结果作为考纲参考。

use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::data::{
    load_progress_index, new_progress_id, now_string, save_progress_index, NodeLevel, NodeStatus,
    ProgressIndex, ProgressNode, ProgressTable, SubjectProgressSet, TableOrigin, WebSearchConfig,
};
use crate::{get_data_dir, get_data_dir_and_ai, AppState};

/// 校验学科 key（与教材导入一致的宽松规则：字母/数字/连字符/下划线）
fn validate_subject(subject: &str) -> Result<(), String> {
    if subject.is_empty()
        || !subject
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("无效的学科名称，仅允许字母、数字、连字符、下划线".to_string());
    }
    Ok(())
}

/// 获取某科目的进度表集合；不存在则返回空集合
fn subject_set<'a>(index: &'a mut ProgressIndex, subject: &str) -> &'a mut SubjectProgressSet {
    index.subjects.entry(subject.to_string()).or_default()
}

/// 生成校验唯一性的简单节点 id
fn ensure_node_ids(table: &mut ProgressTable) {
    for node in table.nodes.iter_mut() {
        if node.id.is_empty() {
            node.id = new_progress_id("n", &node.title);
        }
    }
}

// ============================================================================
// 进度表 CRUD
// ============================================================================

/// 列出全部进度表索引（含每科的启用表 id 与所有表）
/// 前端调用: `invoke('list_progress_tables')`
#[tauri::command]
pub fn list_progress_tables(state: State<'_, Mutex<AppState>>) -> Result<ProgressIndex, String> {
    let data_dir = get_data_dir(state.inner())?;
    Ok(load_progress_index(&data_dir))
}

/// 新增或更新某科的一份进度表
///
/// - `variant`: 考纲方案（数一/数二/数三/英一/英二/408/307/政治），空格则复用表自身/默认
/// - `make_active`: 是否将其设为该科唯一启用（true 时清掉其它表的启用状态，并把科目启用方案同步）
/// - 表 id 为空视为「新建」，否则为更新。
/// 前端调用: `invoke('save_progress_table', { subject, variant, table, makeActive })`
#[tauri::command]
pub fn save_progress_table(
    subject: String,
    variant: String,
    mut table: ProgressTable,
    make_active: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<ProgressTable, String> {
    validate_subject(&subject)?;
    if table.name.trim().is_empty() {
        return Err("进度表名称不能为空".to_string());
    }
    if !table.variant.is_empty() && variant.is_empty() {
        // 表自身已带方案，保留
    } else if !variant.is_empty() {
        table.variant = variant.clone();
    }
    let data_dir = get_data_dir(state.inner())?;

    let mut index = load_progress_index(&data_dir);
    let set = subject_set(&mut index, &subject);

    let now = now_string();
    let is_new = table.id.is_empty();
    let new_id = if is_new {
        let id = new_progress_id("p", &table.name);
        table.id = id.clone();
        if set.active_id.is_empty() || make_active {
            set.active_id = id.clone();
        }
        id
    } else {
        table.id.clone()
    };

    ensure_node_ids(&mut table);
    table.subject = subject.clone();
    if is_new {
        table.created_at = now.clone();
    }
    table.updated_at = now;

    if let Some(existing) = set.tables.iter_mut().find(|t| t.id == new_id) {
        existing.name = table.name.clone();
        existing.variant = table.variant.clone();
        existing.subject = table.subject.clone();
        existing.origin = table.origin;
        existing.updated_at = table.updated_at.clone();
        existing.nodes = table.nodes.clone();
    } else if is_new {
        set.tables.push(table.clone());
    } else {
        // 更新不存在的表：作为新表追加（防御异常数据）
        if set.active_id.is_empty() || make_active {
            set.active_id = new_id.clone();
        }
        set.tables.push(table.clone());
    }

    // 启用互斥：每科仅一份启用
    if make_active {
        set.active_id = new_id;
        // 启用方案与启用表联动
        if !table.variant.is_empty() {
            set.active_variant = table.variant.clone();
        }
    }

    save_progress_index(&data_dir, &index)?;
    Ok(table)
}

/// 删除某科的一份进度表；若删除的是启用表，自动启用剩余的第一份。
/// 前端调用: `invoke('delete_progress_table', { subject, id })`
#[tauri::command]
pub fn delete_progress_table(
    subject: String,
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;
    let mut index = load_progress_index(&data_dir);

    let removed = {
        let set = index.subjects.get_mut(&subject);
        match set {
            None => false,
            Some(set) => {
                let before = set.tables.len();
                set.tables.retain(|t| t.id != id);
                let removed_any = set.tables.len() != before;
                if set.active_id == id {
                    set.active_id = set.tables.first().map(|t| t.id.clone()).unwrap_or_default();
                }
                removed_any
            }
        }
    };

    if !removed {
        return Err("进度表不存在".to_string());
    }
    save_progress_index(&data_dir, &index)
}

/// 删除某科指定方案下全部内置考纲表（用于重新生成内置考纲前清理旧数据，避免污染/重复）。
/// - `variant` 为空时删除该科所有内置表。
/// 前端调用: `invoke('delete_builtin_progress_tables', { subject, variant })`
#[tauri::command]
pub fn delete_builtin_progress_tables(
    subject: String,
    variant: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<usize, String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;
    let mut index = load_progress_index(&data_dir);

    let set = subject_set(&mut index, &subject);
    let before = set.tables.len();
    let active_id = set.active_id.clone();

    set.tables.retain(|t| {
        if t.origin != TableOrigin::Builtin {
            return true;
        }
        if variant.trim().is_empty() {
            return false; // variant 为空：删除该科全部内置表
        }
        t.variant != variant.trim()
    });

    let removed = before - set.tables.len();
    if removed > 0 {
        // 若启用表被删，重新对齐启用状态
        if set.tables.iter().any(|t| t.id == active_id) {
            // active 仍在，保持原样
        } else if let Some(first) = set.tables.first() {
            set.active_id = first.id.clone();
            set.active_variant = first.variant.clone();
        } else {
            set.active_id.clear();
            set.active_variant.clear();
        }
        save_progress_index(&data_dir, &index)?;
    }
    Ok(removed)
}

/// 设定某科启用哪份进度表（同一时刻每科仅一份启用，并同步启用方案为该表所属方案）
/// 前端调用: `invoke('set_active_progress_table', { subject, id })`
#[tauri::command]
pub fn set_active_progress_table(
    subject: String,
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;
    let mut index = load_progress_index(&data_dir);

    let variant = {
        let set = subject_set(&mut index, &subject);
        let t = set
            .tables
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "进度表不存在".to_string())?;
        t.variant.clone()
    };
    let set = subject_set(&mut index, &subject);
    set.active_id = id;
    if !variant.is_empty() {
        set.active_variant = variant;
    }
    save_progress_index(&data_dir, &index)
}

/// 设定某科启用哪个考纲方案；启用表同步对齐到该方案下当前启用表或其第一张表。
/// 新增若该方案尚无任何表，则只设置启用方案（前端随后可生成/新建该方案的表）。
/// 前端调用: `invoke('set_active_progress_variant', { subject, variant })`
#[tauri::command]
pub fn set_active_progress_variant(
    subject: String,
    variant: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    validate_subject(&subject)?;
    if variant.trim().is_empty() {
        return Err("考纲方案不能为空".to_string());
    }
    let data_dir = get_data_dir(state.inner())?;
    let mut index = load_progress_index(&data_dir);
    let set = subject_set(&mut index, &subject);
    set.active_variant = variant.clone();
    // active_id 对齐：优先该方案当前启用表，否则取该方案第一张表
    if !set.tables.is_empty() {
        let active_is_in_variant = set
            .tables
            .iter()
            .any(|t| t.id == set.active_id && t.variant == variant);
        if !active_is_in_variant {
            if let Some(t) = set.tables.iter().find(|t| t.variant == variant) {
                set.active_id = t.id.clone();
            }
        }
    }
    save_progress_index(&data_dir, &index)
}

// ============================================================================
// 进度表与学习状态联动（首次确认 / 复盘联动）
// ============================================================================

/// 根据 State 预估某科知识点当前应处状态（首次打开弹窗确认用）。
/// 前端调用: `invoke('estimate_progress_from_state', { subject })`
#[tauri::command]
pub fn estimate_progress_from_state(
    subject: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::core::progress_sync::StatusEstimate>, String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;
    crate::core::progress_sync::estimate_from_state(&data_dir, &subject)
}

/// 批量应用用户在「首次状态确认」弹窗中勾选的结果（只升不降）。
/// 前端调用: `invoke('apply_progress_statuses', { subject, changes })`
#[tauri::command]
pub fn apply_progress_statuses(
    subject: String,
    changes: Vec<crate::core::progress_sync::StatusChange>,
    state: State<'_, Mutex<AppState>>,
) -> Result<usize, String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;
    crate::core::progress_sync::apply_estimated_statuses(&data_dir, &subject, &changes)
}

/// 把设置中的考试类型解析为各科默认考纲方案（科目 → 方案）。
/// 前端调用: `invoke('default_progress_variants')`
#[tauri::command]
pub fn default_progress_variants(
    state: State<'_, Mutex<AppState>>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let settings = crate::load_settings(&data_dir);
    Ok(crate::core::progress_sync::default_progress_variants(
        &settings.exam_type,
    ))
}

// ============================================================================
// 联网搜索配置
// ============================================================================

/// 读取进度表相关的设置（目前为联网搜索配置）
/// 前端调用: `invoke('get_progress_settings')`
#[tauri::command]
pub fn get_progress_settings(state: State<'_, Mutex<AppState>>) -> Result<WebSearchConfig, String> {
    let data_dir = get_data_dir(state.inner())?;
    Ok(load_progress_index(&data_dir).web_search)
}

/// 保存进度表相关的设置（联网搜索配置）
/// 前端调用: `invoke('set_progress_settings', { webSearch })`
#[tauri::command]
pub fn set_progress_settings(
    web_search: WebSearchConfig,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;
    let mut index = load_progress_index(&data_dir);
    index.web_search = web_search;
    save_progress_index(&data_dir, &index)
}

// ============================================================================
// AI 生成进度表
// ============================================================================

/// AI 生成的节点草稿（允许缺失字段）
#[derive(Deserialize)]
struct NodeDraft {
    #[serde(default)]
    title: String,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    level: NodeLevel,
    #[serde(default)]
    status: Option<NodeStatus>,
    #[serde(default)]
    planned_date: Option<String>,
    #[serde(default)]
    note: String,
}

/// 将考纲方案（variant，如「数二」「英一」「政治」「408」）映射为章节表版本键。
/// 返回的字符串会写进进度表的 `variant` 字段，因此必须和当前方案精确对应，
/// 否则前端按方案过滤/查重/清理都会失败。
fn version_for(subject: &str, variant: &str) -> Option<String> {
    let v = variant.trim();
    match subject {
        "math" => match v {
            "数一" | "数学一" => Some("数一".to_string()),
            "数二" | "数学二" => Some("数二".to_string()),
            "数三" | "数学三" => Some("数三".to_string()),
            _ => None, // 未声明数学版本，交由 AI 依据考纲生成
        },
        // 英语一/二共用同一套知识点顺序，但表的 variant 要保留当前方案，便于识别和清理
        "english" => match v {
            "英一" | "英语一" => Some("英一".to_string()),
            "英二" | "英语二" => Some("英二".to_string()),
            _ if !v.is_empty() => Some(v.to_string()),
            _ => Some("英一".to_string()),
        },
        "politics" => {
            if v.is_empty() {
                Some("政治".to_string())
            } else {
                Some(v.to_string())
            }
        }
        _ => None,
    }
}

/// 内置考纲兜底：从 chapter_seq 取有序知识点列表
fn builtin_syllabus(subject: &str, version: &str) -> Option<Vec<String>> {
    crate::core::chapter_seq::syllabus_points(subject, version)
        .map(|seq| seq.iter().map(|s| s.to_string()).collect())
}

/// 联网搜索最新考研大纲（provider: bocha 博查查）
/// 返回搜索结果摘要拼成的考纲文本；失败返回 Err（调用方回退内置考纲）。
async fn web_search_syllabus(
    subject_label: &str,
    exam_type: &str,
    cfg: &WebSearchConfig,
) -> Result<String, String> {
    if !cfg.enabled || cfg.api_key.is_empty() {
        return Err("联网搜索未启用或未填写 API Key".to_string());
    }
    let base = if cfg.base_url.trim().is_empty() {
        "https://api.bochaai.com/v1/web-search".to_string()
    } else {
        cfg.base_url.trim().to_string()
    };
    let ver_note = if exam_type.is_empty() {
        String::new()
    } else {
        format!(" {}", exam_type)
    };
    let query = format!("{}考研大纲{} 最新版完整考试内容", subject_label, ver_note);

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "query": query,
        "freshness": "oneYear",
        "summary": true,
        "count": 5,
    });
    let resp = client
        .post(&base)
        .header("Authorization", format!("Bearer {}", cfg.api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("联网搜索请求失败: {}", e))?;

    let status = resp.status();
    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("联网搜索返回错误 {}: {}", status, json));
    }

    let mut out = String::new();
    if let Some(pages) = json
        .pointer("/data/webPages/learning")
        .or_else(|| json.pointer("/data/webPages"))
    {
        if let Some(arr) = pages.as_array() {
            for item in arr.iter().take(5) {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let summary = item
                    .get("summary")
                    .or_else(|| item.get("snippet"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !summary.is_empty() {
                    out.push_str(&format!("【{}】{}\n", name, summary));
                }
            }
        }
    }
    if out.trim().is_empty() {
        return Err("联网搜索未返回有效考纲内容".to_string());
    }
    Ok(out)
}

/// AI 生成一份进度表（返回草稿，不自动落盘；前端预览确认后再保存）
///
/// - `subject`: 学科 key
/// - `variant`: 考纲方案（如「数二」「英一」「408 计算机」），可从进度表页当前启用方案读取
/// - `name`: 生成的进度表名称
/// - `use_web`: 是否优先联网查询最新考研大纲（若未配置则回退内置考纲）
/// 前端调用: `invoke('generate_progress_table', { subject, variant, name, useWeb })`
#[tauri::command]
pub async fn generate_progress_table(
    subject: String,
    variant: String,
    name: String,
    use_web: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<ProgressTable, String> {
    validate_subject(&subject)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成进度表。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    // 考试类型：优先使用传入，其次读设置
    let effective_exam = variant.clone();
    let ver = version_for(&subject, &effective_exam);
    let subject_label = crate::data::subject_label(&subject);

    // 1. 考纲来源：联网搜索（可选）→ 内置考纲 → 空（交由 AI 自行组织）
    let mut used_web = false;
    let syllabus_text = if use_web {
        let index = load_progress_index(&data_dir);
        match web_search_syllabus(subject_label, &effective_exam, &index.web_search).await {
            Ok(text) => {
                used_web = true;
                text
            }
            Err(e) => {
                log::warn!("[进度表] 联网搜索失败，回退内置考纲: {}", e);
                builtin_syllabus(&subject, ver.as_deref().unwrap_or(""))
                    .unwrap_or_default()
                    .join("\n")
            }
        }
    } else {
        builtin_syllabus(&subject, ver.as_deref().unwrap_or(""))
            .unwrap_or_default()
            .join("\n")
    };

    // 2. 构建 prompt
    let prompt = build_generate_prompt(
        subject_label,
        &effective_exam,
        &name,
        &syllabus_text,
        used_web,
    );

    let request = ChatRequest {
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: prompt,
            ..Default::default()
        }],
        agent: Some(AgentType::Assistant),
        temperature: Some(0.4),
        timeout_override: Some(240),
        math_version: ver.clone(),
        ..Default::default()
    };

    let response = ai_service.chat(request).await.map_err(|e| {
        log::warn!("[进度表] AI 调用失败: {}", e);
        format!("AI 生成进度表失败: {}", e)
    })?;

    let cleaned = crate::data::clean_ai_json(&response.content);
    let drafts: Vec<NodeDraft> = serde_json::from_str(&cleaned)
        .map_err(|e| format!("AI 返回的进度表格式无法解析: {}", e))?;
    if drafts.is_empty() {
        return Err("AI 未生成任何进度节点，请重试".to_string());
    }

    let now = now_string();
    // 两级结构：AI 以 phase 作为章节（首次出现顺序建立章节 id），知识点归属其下；
    // 若某节点显式带 level=chapter，则其本身作为章节节点。
    let mut id_for_phase: std::collections::HashMap<String, String> = Default::default();
    let mut chapter_order: Vec<String> = Vec::new();
    for d in &drafts {
        let ph = d.phase.trim();
        if !ph.is_empty() && !id_for_phase.contains_key(ph) {
            let cid = new_progress_id("c", ph);
            id_for_phase.insert(ph.to_string(), cid);
            chapter_order.push(ph.to_string());
        }
    }
    let mut nodes: Vec<ProgressNode> = Vec::new();
    let mut chapters_used: std::collections::HashSet<String> = Default::default();
    for d in &drafts {
        let ph = d.phase.trim().to_string();
        match d.level {
            NodeLevel::Chapter => {
                nodes.push(ProgressNode {
                    id: new_progress_id("n", &d.title),
                    title: d.title.trim().to_string(),
                    level: NodeLevel::Chapter,
                    parent_id: None,
                    phase: ph.clone(),
                    status: d.status.unwrap_or(NodeStatus::Pending),
                    planned_date: d.planned_date.clone(),
                    note: d.note.trim().to_string(),
                });
            }
            NodeLevel::Knowledge => {
                let pid = id_for_phase.get(&ph).cloned();
                if let Some(ref c) = pid {
                    chapters_used.insert(c.clone());
                }
                nodes.push(ProgressNode {
                    id: new_progress_id("n", &d.title),
                    title: d.title.trim().to_string(),
                    level: NodeLevel::Knowledge,
                    parent_id: pid,
                    phase: ph.clone(),
                    status: d.status.unwrap_or(NodeStatus::Pending),
                    planned_date: d.planned_date.clone(),
                    note: d.note.trim().to_string(),
                });
            }
        }
    }
    // 表头插入有知识点的章节节点
    let mut final_nodes: Vec<ProgressNode> = Vec::new();
    for cid in &chapter_order {
        let cid_r = id_for_phase.get(cid).cloned().unwrap_or_default();
        if chapters_used.contains(cid_r.as_str()) {
            final_nodes.push(ProgressNode {
                id: cid_r.clone(),
                title: cid.clone(),
                level: NodeLevel::Chapter,
                parent_id: None,
                phase: cid.clone(),
                status: NodeStatus::Pending,
                planned_date: None,
                note: String::new(),
            });
        }
    }
    for node in nodes {
        if node.level == NodeLevel::Knowledge || node.parent_id.is_none() {
            final_nodes.push(node);
        }
    }
    final_nodes.retain(|n| !n.title.is_empty());

    let table = ProgressTable {
        id: String::new(), // 草稿：id 留空，保存时分配
        subject: subject.clone(),
        variant: variant.clone(),
        name: if name.trim().is_empty() {
            format!("{}进度表", subject_label)
        } else {
            name.trim().to_string()
        },
        origin: TableOrigin::Custom,
        created_at: now.clone(),
        updated_at: now,
        nodes: final_nodes,
    };
    Ok(table)
}

/// 数学知识点按大章分组（用于内置考纲两级结构）。
/// 注意：高数里也有「向量代数与空间解析几何」，所以「向量」单独出现要留给高数，
/// 只有「向量组」及其后续线性代数特征词才归到线代。
fn chapter_for_math(title: &str) -> &'static str {
    let t = title;
    // 线代：用「向量组」而不是「向量」，避免把高数空间向量误判进线代
    if t.contains("行列式")
        || t.contains("矩阵")
        || t.contains("向量组")
        || t.contains("线性方程组")
        || t.contains("特征值")
        || t.contains("二次型")
        || t.contains("相似矩阵")
        || t.contains("基础解系")
    {
        "线性代数"
    } else if t.contains("随机")
        || t.contains("概率")
        || t.contains("分布")
        || t.contains("估计")
        || t.contains("假设检验")
        || t.contains("大数定律")
        || t.contains("中心极限")
        || t.contains("统计量")
        || t.contains("抽样分布")
    {
        "概率论与数理统计"
    } else {
        "高等数学"
    }
}

/// 政治知识点按大章分组
fn chapter_for_politics(title: &str) -> &'static str {
    let t = title;
    if t.contains("马克思主义")
        || t.contains("辩证唯物")
        || t.contains("唯物辩证法")
        || t.contains("认识论")
        || t.contains("唯物史观")
        || t.contains("商品")
        || t.contains("剩余价值")
        || t.contains("资本主义")
        || t.contains("垄断")
    {
        "马克思主义基本原理"
    } else if t.contains("毛泽东")
        || t.contains("新民主主义")
        || t.contains("社会主义改造")
        || t.contains("社会主义建设")
        || (t.contains("中国特色社会主义") && !t.contains("新时代"))
        || t.contains("经济发展")
        || t.contains("全面深化改革")
        || t.contains("社会主义民主政治")
        || t.contains("文化建设")
        || t.contains("社会主义核心价值观")
        || t.contains("民生")
        || t.contains("社会治理")
        || t.contains("生态文明")
        || t.contains("党的建设")
        || t.contains("从严治党")
    {
        "毛泽东思想和中国特色社会主义理论体系概论"
    } else if t.contains("新时代")
        || t.contains("中国式现代化")
        || t.contains("高质量发展")
        || t.contains("新质生产力")
        || t.contains("新发展格局")
    {
        "习近平新时代中国特色社会主义思想概论"
    } else if t.contains("近代")
        || t.contains("新民主主义革命")
        || t.contains("中华人民共和国")
        || t.contains("社会主义制度")
        || (t.contains("改革开放") && !t.contains("新时代"))
        || t.contains("社会主义建设道路")
    {
        "中国近现代史纲要"
    } else if t.contains("人生观")
        || t.contains("理想信念")
        || t.contains("道德")
        || t.contains("法治")
        || t.contains("宪法")
        || t.contains("依法治国")
    {
        "思想道德与法治"
    } else {
        "形势与政策以及当代世界经济与政治"
    }
}

/// 英语知识点按大章分组
fn chapter_for_english(title: &str) -> &'static str {
    let t = title;
    if t.contains("词汇") || t.contains("长难句") || t.contains("语法") {
        "词汇与语法"
    } else if t.contains("完形") || t.contains("知识运用") {
        "完形填空"
    } else if t.contains("阅读") || t.contains("新题型") {
        "阅读理解"
    } else if t.contains("翻译") {
        "翻译"
    } else if t.contains("作文") || t.contains("写作") {
        "写作"
    } else {
        "综合"
    }
}

/// 把普通科目的扁平知识点列表按大章整理为两级节点（章节 + 知识点）
fn build_subject_nodes(subject: &str, points: Vec<String>) -> Vec<ProgressNode> {
    let chapter_fn: fn(&str) -> &'static str = match subject {
        "math" => chapter_for_math,
        "politics" => chapter_for_politics,
        "english" => chapter_for_english,
        _ => |_| "未分组",
    };

    let mut nodes: Vec<ProgressNode> = Vec::new();
    let mut chapter_ids: std::collections::HashMap<&'static str, String> =
        std::collections::HashMap::new();

    for title in points {
        let ch = chapter_fn(&title);
        let cid = chapter_ids
            .entry(ch)
            .or_insert_with(|| new_progress_id("c", ch))
            .clone();

        // 确保章节节点只插入一次
        if !nodes
            .iter()
            .any(|n| n.level == NodeLevel::Chapter && n.id == cid)
        {
            nodes.push(ProgressNode {
                id: cid.clone(),
                title: ch.to_string(),
                level: NodeLevel::Chapter,
                parent_id: None,
                phase: ch.to_string(),
                status: NodeStatus::Pending,
                planned_date: None,
                note: String::new(),
            });
        }

        nodes.push(ProgressNode {
            id: new_progress_id("n", &title),
            title,
            level: NodeLevel::Knowledge,
            parent_id: Some(cid),
            phase: ch.to_string(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        });
    }
    nodes
}

// ============================================================================
// 内置考纲进度表（不依赖 AI）
// ============================================================================

/// 内置考纲转进度表：将随包内置的官方考研考纲（core::chapter_seq / core::professional）
/// 直接转换为进度表草稿（不自动落盘，前端预览确认后再逐份保存）。
///
/// - 普通科目（数学/英语/政治）：返回一份总表草稿；
/// - 专业课（subject=professional）：按考纲方案匹配内置统考专业课，返回多份草稿：
///   第 1 份为「总专业课进度表」（考纲板块→章节），其后为每本指定教材一张进度表。
/// - `variant`: 考纲方案（如「数二」「英一」「408 计算机」「法硕（非法学）」），用于定位内置考纲版本
/// 前端调用: `invoke('builtin_progress_table', { subject, variant })`
#[tauri::command]
pub fn builtin_progress_table(
    subject: String,
    variant: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ProgressTable>, String> {
    validate_subject(&subject)?;
    let data_dir = get_data_dir(state.inner())?;

    let effective_exam = {
        let settings = crate::load_settings(&data_dir);
        if variant.trim().is_empty() {
            settings.exam_type.clone()
        } else {
            variant.clone()
        }
    };

    // 专业课：匹配随包内置的统考专业课（返回总表 + 各教材表）
    if subject == "professional" || subject == "408" {
        match crate::core::professional::find(&effective_exam) {
            Some(exam) => return Ok(crate::core::professional::build_tables(&exam)),
            None => {
                return Err(format!(
                    "「专业课」暂未识别考试类型「{}」对应的内置专业课（当前支持：{}），\
                     请把专业课名称填写为其中一种，或使用 AI 生成。",
                    if effective_exam.trim().is_empty() {
                        "未填写"
                    } else {
                        effective_exam.trim()
                    },
                    crate::core::professional::all_names()
                ))
            }
        }
    }

    // 普通科目：内置考纲知识点顺序 → 一份带章节结构的总表草稿
    let resolved = if subject == "408" {
        "professional"
    } else {
        &subject
    };
    if resolved != "math" && resolved != "english" && resolved != "politics" {
        return Err(format!("暂不支持学科「{}」的内置考纲生成", subject));
    }
    let ver = version_for(&subject, &effective_exam);
    let points = builtin_syllabus(&subject, ver.as_deref().unwrap_or("")).ok_or_else(|| {
        format!(
            "学科「{}」暂无内置考纲数据",
            crate::data::subject_label(&subject)
        )
    })?;

    let now = now_string();
    let nodes = build_subject_nodes(&subject, points);

    let table = ProgressTable {
        id: String::new(), // 草稿：id 留空，保存时分配
        subject: subject.clone(),
        variant: ver.unwrap_or_default(),
        name: format!("{} · 内置官方考纲", crate::data::subject_label(&subject)),
        origin: TableOrigin::Builtin,
        created_at: now.clone(),
        updated_at: now,
        nodes,
    };
    Ok(vec![table])
}

/// 生成进度表的 prompt 构造
fn build_generate_prompt(
    subject_label: &str,
    exam_type: &str,
    name: &str,
    syllabus_text: &str,
    used_web: bool,
) -> String {
    let source_note = if used_web {
        "我已通过联网搜索为你获取了以下最新考研考纲内容（可能包含网页摘要，多来源）作为依据。"
    } else {
        "以下为内置的官方考研考纲知识点顺序（如未提供，请按该科目权威最新考纲自行组织，务必覆盖核心考点且符合最新考纲要求）。"
    };

    let syllabus_block = if syllabus_text.trim().is_empty() {
        "（未提供考纲文本，请你依据该科最新权威考试大纲自行构建，务必遵循最新考纲的考察范围，不得遗漏新大纲新增考点。）"
            .to_string()
    } else {
        format!("【{}考纲参考】\n{}", subject_label, syllabus_text)
    };

    format!(
        concat!(
            "你是考研规划助手，为 StudyAgent 桌面应用生成一份「{}」的完整学习进度表（名称：{}，考试类型：{}）。\n",
            "要求：\n",
            "1. 必须依据最新考研考试大纲，覆盖该科全部考查范围，并遵循大纲的章节先后顺序。\n",
            "2. 将考纲组织为首尾有序的进度节点，每个节点建议为「章/节」粒度（如「第一章 函数、极限、连续」下拆成知识点节点时口径保持一致），总节点数适中（约 30-80 个），能用于长期学习打卡。\n",
            "3. 同一章的多个知识点应归入相同 phase，phase 用作章节分组；phase 与 title 内容不同（phase=章名，title=具体知识点）。\n",
            "4. 生成顺序 = 建议学习顺序，从基础到进阶。\n",
            "{}\n",
            "{}\n",
            "只输出严格合法的 JSON 数组，不要包裹 ```json 代码块，不要输出任何解释文字。\n",
            "数组元素字段：{{\"title\": 知识点标题, \"phase\": 所属章节, \"status\": \"pending\", \"planned_date\": null, \"note\": \"一句话学习要点或真题提示,可空\"}}\n",
            "输出示例：[{{\"title\":\"函数的概念及表示法\",\"phase\":\"第一章 函数、极限、连续\",\"status\":\"pending\",\"planned_date\":null,\"note\":\"注重定义域与对应法则\"}}]",
        ),
        subject_label,
        if name.trim().is_empty() { "未命名".to_string() } else { name.trim().to_string() },
        if exam_type.trim().is_empty() { "未指定".to_string() } else { exam_type.trim().to_string() },
        source_note,
        syllabus_block,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_for_maps_subjects_correctly() {
        assert_eq!(version_for("math", "数二"), Some("数二".to_string()));
        assert_eq!(version_for("math", "数学二"), Some("数二".to_string()));
        assert_eq!(version_for("english", "英一"), Some("英一".to_string()));
        assert_eq!(version_for("english", "英语二"), Some("英二".to_string()));
        assert_eq!(version_for("politics", "政治"), Some("政治".to_string()));
        assert_eq!(version_for("politics", ""), Some("政治".to_string()));
    }

    #[test]
    fn chapter_for_math_distinguishes_vector_calculus_and_linalg() {
        // 高数中的「向量」不应被误判为线性代数
        assert_eq!(chapter_for_math("向量的概念及其线性运算"), "高等数学");
        assert_eq!(chapter_for_math("数量积、向量积、混合积"), "高等数学");
        // 「向量组」才是线代
        assert_eq!(chapter_for_math("向量组及其线性组合"), "线性代数");
        assert_eq!(chapter_for_math("行列式的定义与性质"), "线性代数");
        // 概率
        assert_eq!(chapter_for_math("随机事件与样本空间"), "概率论与数理统计");
    }

    #[test]
    fn build_subject_nodes_groups_math_by_chapter() {
        let nodes = build_subject_nodes(
            "math",
            vec![
                "向量的概念及其线性运算".to_string(),
                "向量组及其线性组合".to_string(),
                "随机事件与样本空间".to_string(),
            ],
        );
        let chapters: Vec<&str> = nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Chapter)
            .map(|n| n.title.as_str())
            .collect();
        assert_eq!(chapters, vec!["高等数学", "线性代数", "概率论与数理统计"]);

        // 知识点归属正确
        let vector_calc = nodes
            .iter()
            .find(|n| n.title == "向量的概念及其线性运算")
            .unwrap();
        assert_eq!(vector_calc.phase, "高等数学");
        let vector_group = nodes
            .iter()
            .find(|n| n.title == "向量组及其线性组合")
            .unwrap();
        assert_eq!(vector_group.phase, "线性代数");
    }
}
