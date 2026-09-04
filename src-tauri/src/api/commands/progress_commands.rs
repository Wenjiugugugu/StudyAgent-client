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
    load_progress_index, new_progress_id, now_string, save_progress_index, NodeStatus,
    ProgressIndex, ProgressNode, ProgressTable, SubjectProgressSet, WebSearchConfig,
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
    index
        .subjects
        .entry(subject.to_string())
        .or_insert_with(SubjectProgressSet::default)
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
/// - `make_active`: 是否将其设为该科唯一启用（true 时清掉其它表的启用状态）
/// - 表 id 为空视为「新建」，否则为更新。
/// 前端调用: `invoke('save_progress_table', { subject, table, makeActive })`
#[tauri::command]
pub fn save_progress_table(
    subject: String,
    mut table: ProgressTable,
    make_active: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<ProgressTable, String> {
    validate_subject(&subject)?;
    if table.name.trim().is_empty() {
        return Err("进度表名称不能为空".to_string());
    }
    let data_dir = get_data_dir(state.inner())?;

    let mut index = load_progress_index(&data_dir);
    let set = subject_set(&mut index, &subject);

    let now = now_string();
    let is_new = table.id.is_empty();
    let new_id = if is_new {
        let id = new_progress_id("p", &table.name);
        table.id = id.clone();
        set.active_id = if set.active_id.is_empty() || make_active {
            id.clone()
        } else {
            set.active_id.clone()
        };
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
        existing.subject = table.subject.clone();
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
                    set.active_id = set
                        .tables
                        .first()
                        .map(|t| t.id.clone())
                        .unwrap_or_default();
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

/// 设定某科启用哪份进度表（同一时刻每科仅一份启用）
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

    let set = subject_set(&mut index, &subject);
    if !set.tables.iter().any(|t| t.id == id) {
        return Err("进度表不存在".to_string());
    }
    set.active_id = id;
    save_progress_index(&data_dir, &index)
}

// ============================================================================
// 联网搜索配置
// ============================================================================

/// 读取进度表相关的设置（目前为联网搜索配置）
/// 前端调用: `invoke('get_progress_settings')`
#[tauri::command]
pub fn get_progress_settings(
    state: State<'_, Mutex<AppState>>,
) -> Result<WebSearchConfig, String> {
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
    status: Option<NodeStatus>,
    #[serde(default)]
    planned_date: Option<String>,
    #[serde(default)]
    note: String,
}

/// 将 exam_type（如「数学二」）映射为章节表版本键（数一/数二/数三），
/// 非数学科目或无匹配时返回空字符串。
fn version_for(subject: &str, exam_type: &str) -> Option<String> {
    if subject == "math" {
        for key in ["数一", "数二", "数三"] {
            if exam_type.contains(key) {
                return Some(key.to_string());
            }
        }
        return None; // 未声明数学版本，交由 AI 依据考纲生成
    }
    if subject == "english" {
        if exam_type.contains("英语二") || exam_type.contains("英二") {
            return Some(String::new());
        }
        return Some(String::new()); // 英一/英二 采用同一占位表
    }
    if subject == "politics" {
        return Some(String::new());
    }
    None
}

/// 内置考纲兜底：从 chapter_seq 取有序知识点列表
fn builtin_syllabus(subject: &str, version: &str) -> Option<Vec<String>> {
    // 兼容旧键：408 视为 professional（无内置表）
    let resolved = if subject == "408" { "professional" } else { subject };
    crate::core::chapter_seq::syllabus_points(resolved, version)
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
    if let Some(pages) = json.pointer("/data/webPages/learning")
        .or_else(|| json.pointer("/data/webPages")) {
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
/// - `exam_type`: 考试类型文本（如「数学二」），可从设置读取
/// - `name`: 生成的进度表名称
/// - `use_web`: 是否优先联网查询最新考研大纲（若未配置则回退内置考纲）
/// 前端调用: `invoke('generate_progress_table', { subject, examType, name, useWeb })`
#[tauri::command]
pub async fn generate_progress_table(
    subject: String,
    exam_type: String,
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
    let effective_exam = {
        let settings = crate::load_settings(&data_dir);
        if exam_type.trim().is_empty() {
            settings.exam_type.clone()
        } else {
            exam_type.clone()
        }
    };
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
    let nodes: Vec<ProgressNode> = drafts
        .into_iter()
        .map(|d| ProgressNode {
            id: new_progress_id("n", &d.title),
            title: d.title.trim().to_string(),
            phase: d.phase.trim().to_string(),
            status: d.status.unwrap_or(NodeStatus::Pending),
            planned_date: d.planned_date,
            note: d.note.trim().to_string(),
        })
        .collect();

    let table = ProgressTable {
        id: String::new(), // 草稿：id 留空，保存时分配
        subject: subject.clone(),
        name: if name.trim().is_empty() {
            format!("{}进度表", subject_label)
        } else {
            name.trim().to_string()
        },
        created_at: now.clone(),
        updated_at: now,
        nodes,
    };
    Ok(table)
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