//! 滴答清单（Dida365）同步模块
//!
//! 基于 MCP Streamable HTTP 协议直连滴答官方服务 `https://mcp.dida365.com/`，
//! 协议实现参考 `scripts/push_plan_to_dida.py`（initialize + tools/call + SSE 响应解析）。
//!
//! ## 职责
//! - `reconcile_day`：按日对账「计划 JSON 任务 ↔ 滴答任务」（增删改增量，只动带 studyagent 标签的任务）
//! - `sync_task_status`：应用内勾选完成/取消时，单向同步滴答完成状态
//! - `fetch_completed_titles`：复盘前回读滴答当日已完成任务标题（供 AI prompt 确认完成情况）
//!
//! ## 约束（定稿）
//! - 只对带 `studyagent` 来源标签的任务做读取/修改/删除；其余任务保持不动。
//! - 标签仅两类：`studyagent` + 学科标签（数学/英语/政治/专业课）。
//! - 已完成且已不在计划中的滴答任务不删除（保留完成历史）。
//! - 所有同步失败只记录日志，不阻塞计划生成/复盘等主流程。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::data::plan::{read_daily_plan, save_daily_plan, DailyPlanFile};
use crate::data::state::{SubjectKey, TaskPriority, TaskStatus};

const MCP_URL: &str = "https://mcp.dida365.com/";
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// 来源标记：所有本系统创建/管理的滴答任务固定携带
const SOURCE_TAG: &str = "studyagent";
const TIMEZONE: &str = "Asia/Shanghai";

// ============================================================================
// 标签与字段映射（规范化）
// ============================================================================

/// 任务优先级 → 滴答 priority 字段（A=5 高 / B=3 中 / C=1 低 / 其余 0）
fn priority_value(p: &TaskPriority) -> i32 {
    match p {
        TaskPriority::A => 5,
        TaskPriority::B => 3,
        TaskPriority::C => 1,
    }
}

/// 学科 → 滴答学科标签（固定词表，仅用于筛选与统计）
fn subject_tag(s: &SubjectKey) -> String {
    match s {
        SubjectKey::Math => "数学".to_string(),
        SubjectKey::English => "英语".to_string(),
        SubjectKey::Politics => "政治".to_string(),
        SubjectKey::Professional => "专业课".to_string(),
    }
}

/// 为计划任务生成滴答标签集合（仅两类：来源标记 + 学科）
fn make_tags(s: &SubjectKey) -> Vec<String> {
    vec![SOURCE_TAG.to_string(), subject_tag(s)]
}

/// 去除旧版脚本写入的标题前缀（`[A] ` / `[B] ` / `[C] `）
fn strip_legacy_prefix(title: &str) -> String {
    let t = title.trim();
    for p in ["[A] ", "[B] ", "[C] "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim().to_string();
        }
    }
    t.to_string()
}

/// 判断滴答任务是否带来源标记
fn is_owned(tags: &[String]) -> bool {
    tags.iter().any(|t| t == SOURCE_TAG)
}

/// 空字符串项目 id → None（滴答写接口的 project_id 为可选项）
fn pid_opt(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

// ============================================================================
// MCP Streamable HTTP 客户端
// ============================================================================

/// 滴答 MCP 客户端（每次调用即用即建，不常驻）
struct DidaClient {
    token: String,
}

/// 滴答任务（对账用）
#[derive(Debug, Clone)]
struct DidaTask {
    id: String,
    title: String,
    tags: Vec<String>,
    done: bool,
    /// 任务实际所在清单 id（删除/完成必须用它；传错清单 id 时滴答会静默成功但不生效）
    project_id: String,
}

impl DidaClient {
    fn new(token: String) -> Self {
        Self { token }
    }

    /// 发送 JSON-RPC 请求，自动解析 JSON 或 SSE 响应，透传会话 ID
    async fn post(&self, payload: Value, session: &mut Option<String>) -> Result<Value, String> {
        let client = reqwest::Client::new();
        let mut req = client
            .post(MCP_URL)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .header("Authorization", format!("Bearer {}", self.token));
        if let Some(sid) = session.as_deref() {
            req = req.header("Mcp-Session-Id", sid);
        }
        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("滴答 MCP 请求失败: {e}"))?;

        // streamable-http 规范：服务端可能返回会话 ID，后续请求需携带
        if let Some(v) = resp.headers().get("Mcp-Session-Id") {
            if let Ok(s) = v.to_str() {
                *session = Some(s.to_string());
            }
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("读取滴答 MCP 响应失败: {e}"))?;
        parse_mcp_response(&body, &content_type)
    }

    async fn initialize(&self, session: &mut Option<String>) -> Result<(), String> {
        let resp = self
            .post(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": { "name": "studyagent-desktop", "version": "0.6.0" }
                    }
                }),
                session,
            )
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(format!("滴答 MCP initialize 失败: {}", err));
        }
        // 可选：枚举工具，确认服务端实际工具名（对账 schema 以便后续适配）
        let _ = self
            .post(
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
                session,
            )
            .await
            .map(|v| {
                let names: Vec<String> = v
                    .get("result")
                    .and_then(|r| r.get("tools"))
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                if !names.is_empty() {
                    log::info!("[dida] 服务端可用工具: {:?}", names);
                }
            });
        let _ = self
            .post(
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} }),
                session,
            )
            .await;
        Ok(())
    }

    /// 调用 MCP 工具，返回 structuredContent（不存在则退回 content[0].text 解析）
    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        session: &mut Option<String>,
        rid: u64,
    ) -> Result<Value, String> {
        let resp = self
            .post(
                json!({
                    "jsonrpc": "2.0",
                    "id": rid,
                    "method": "tools/call",
                    "params": { "name": name, "arguments": arguments }
                }),
                session,
            )
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(format!("滴答 MCP 工具 {} 返回错误: {}", name, err));
        }
        let result = resp.get("result").cloned().unwrap_or(Value::Null);
        if result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let detail = result
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.get("text"))
                .cloned()
                .unwrap_or_else(|| json!("无错误详情"));
            return Err(format!("滴答 MCP 工具 {} 调用失败: {}", name, detail));
        }
        if let Some(sc) = result.get("structuredContent") {
            return Ok(sc.clone());
        }
        if let Some(arr) = result.get("content").and_then(|c| c.as_array()) {
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    if let Ok(v) = serde_json::from_str::<Value>(text) {
                        return Ok(v);
                    }
                    return Ok(json!({ "_text": text }));
                }
            }
        }
        Ok(Value::Null)
    }

    /// 拉取指定日期窗口内的任务（未完成或已完成）
    async fn list_tasks_in_window(
        &self,
        session: &mut Option<String>,
        date: &str,
        completed: bool,
        rid: &mut u64,
    ) -> Result<Vec<DidaTask>, String> {
        let tool = if completed {
            "list_completed_tasks_by_date"
        } else {
            "list_undone_tasks_by_date"
        };
        let end = crate::data::add_days(date, 1).unwrap_or_else(|_| date.to_string());
        *rid += 1;
        let v = self
            .call_tool(
                tool,
                json!({ "search": {
                    "startDate": format!("{}T00:00:00+08:00", date),
                    "endDate": format!("{}T00:00:00+08:00", end),
                }}),
                session,
                *rid,
            )
            .await?;
        let arr = v
            .as_array()
            .or_else(|| v.get("tasks").and_then(|t| t.as_array()))
            .or_else(|| v.get("result").and_then(|r| r.as_array()))
            .or_else(|| v.get("result").and_then(|r| r.get("tasks")).and_then(|t| t.as_array()))
            .cloned()
            .unwrap_or_default();
        Ok(arr
            .into_iter()
            .map(|t| DidaTask {
                id: t
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                title: t
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                project_id: t
                    .get("projectId")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                tags: t
                    .get("tags")
                    .and_then(|x| x.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
                done: completed,
            })
            .collect())
    }

    async fn create_task(
        &self,
        session: &mut Option<String>,
        rid: &mut u64,
        date: &str,
        title: &str,
        priority: i32,
        tags: &[String],
        project_id: Option<&str>,
    ) -> Result<String, String> {
        let mut task = serde_json::Map::new();
        if let Some(pid) = project_id {
            task.insert("projectId".into(), json!(pid));
        }
        task.insert("title".into(), json!(title));
        task.insert("priority".into(), json!(priority));
        task.insert("isAllDay".into(), json!(false));
        task.insert("dueDate".into(), json!(format!("{}T22:00:00+08:00", date)));
        task.insert("startDate".into(), json!(format!("{}T09:00:00+08:00", date)));
        task.insert("timeZone".into(), json!(TIMEZONE));
        task.insert("kind".into(), json!("TEXT"));
        task.insert("tags".into(), json!(tags));
        *rid += 1;
        let v = self
            .call_tool(
                "create_task",
                json!({ "task": Value::Object(task) }),
                session,
                *rid,
            )
            .await?;
        v.get("id")
            .or_else(|| v.get("task").and_then(|t| t.get("id")))
            .or_else(|| v.get("result").and_then(|r| r.get("id")))
            .or_else(|| v.get("result").and_then(|r| r.get("task")).and_then(|t| t.get("id")))
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| format!("create_task 响应缺少 id: {}", v))
    }

    /// 更新任务标题/优先级/标签
    ///
    /// 官方 schema（经 tools/list + 实测）：`{ task_id, task }`，
    /// task 内为 OpenTask 模型且 **id 必填**；projectId 建议携带。
    async fn update_task(
        &self,
        session: &mut Option<String>,
        rid: &mut u64,
        id: &str,
        title: &str,
        priority: i32,
        tags: &[String],
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let mut task = serde_json::Map::new();
        task.insert("id".into(), json!(id));
        task.insert("title".into(), json!(title));
        task.insert("priority".into(), json!(priority));
        task.insert("tags".into(), json!(tags));
        if let Some(pid) = project_id {
            task.insert("projectId".into(), json!(pid));
        }
        *rid += 1;
        let payload = json!({ "task_id": id, "task": Value::Object(task) });
        self.call_tool("update_task", payload, session, *rid).await.map(|_| ())
    }

    /// 标记完成（官方 schema：`{ project_id, task_id }` 均必填）
    async fn complete_task(
        &self,
        session: &mut Option<String>,
        rid: &mut u64,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let pid = project_id.ok_or_else(|| "缺少滴答 project_id，无法完成任务".to_string())?;
        *rid += 1;
        self.call_tool(
            "complete_task",
            json!({ "project_id": pid, "task_id": id }),
            session,
            *rid,
        )
        .await
        .map(|_| ())
    }

    /// 删除任务（官方 schema：`{ project_id, task_id }` 均必填）
    async fn delete_task(
        &self,
        session: &mut Option<String>,
        rid: &mut u64,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let pid = project_id.ok_or_else(|| "缺少滴答 project_id，无法删除任务".to_string())?;
        *rid += 1;
        self.call_tool(
            "delete_task",
            json!({ "project_id": pid, "task_id": id }),
            session,
            *rid,
        )
        .await
        .map(|_| ())
    }
}

/// 解析 MCP 响应：JSON 或 SSE（按 data: 行解析）
fn parse_mcp_response(body: &str, content_type: &str) -> Result<Value, String> {
    let data_lines: Vec<&str> = body
        .lines()
        .filter_map(|l| l.strip_prefix("data:").map(|d| d.trim()))
        .collect();
    if content_type.contains("text/event-stream") || data_lines.len() > 1 {
        let joined = data_lines.join("\n");
        if let Ok(v) = serde_json::from_str::<Value>(&joined) {
            return Ok(v);
        }
        for d in &data_lines {
            if let Ok(v) = serde_json::from_str::<Value>(d) {
                return Ok(v);
            }
        }
        return Err(format!(
            "SSE 响应解析失败: {}",
            &body[..body.len().min(400)]
        ));
    }
    serde_json::from_str(body)
        .map_err(|e| format!("JSON 响应解析失败: {e}, body={}", &body[..body.len().min(400)]))
}

// ============================================================================
// 配置读取
// ============================================================================

fn ticktick_cfg(data_dir: &Path) -> Value {
    crate::load_settings(data_dir).ticktick.clone()
}

/// 同步开关：settings.ticktick.enabled == true 且存在 Token 时启用
fn is_sync_enabled(data_dir: &Path) -> bool {
    ticktick_cfg(data_dir)
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn project_id_of(data_dir: &Path) -> Option<String> {
    ticktick_cfg(data_dir)
        .get("project_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|s| !s.is_empty())
}

/// 归属项目解析：优先 settings.ticktick.project_id；
/// 未配置时调用 list_projects 取第一个项目（通常为「学习」或用户首个清单）
async fn resolve_project_id(
    data_dir: &Path,
    client: &DidaClient,
    session: &mut Option<String>,
    rid: &mut u64,
) -> Option<String> {
    if let Some(pid) = project_id_of(data_dir) {
        return Some(pid);
    }
    *rid += 1;
    match client
        .call_tool("list_projects", json!({}), session, *rid)
        .await
    {
        Ok(v) => {
            let pid = extract_first_project_id(&v);
            log::info!("[dida] 未配置 project_id，自动选用项目 {:?}", pid);
            if pid.is_none() {
                log::warn!("[dida] list_projects 响应形状未匹配: {}", v);
            }
            pid
        }
        Err(e) => {
            log::warn!("[dida] list_projects 失败: {}", e);
            None
        }
    }
}

/// 从 list_projects 响应中挑选归属项目 id
///
/// 策略：优先名称为「学习」的清单；否则第一个未关闭（closed != true）的清单；再退第一个。
/// 兼容数组、`{ "projects": [...] }`、`{ "result": [...] }` 等形状。
fn extract_first_project_id(v: &Value) -> Option<String> {
    let arr = v
        .as_array()
        .or_else(|| v.get("projects").and_then(|p| p.as_array()))
        .or_else(|| {
            v.get("result")
                .and_then(|r| r.as_array())
                .or_else(|| v.get("result").and_then(|r| r.get("projects")).and_then(|p| p.as_array()))
        });

    let items: Vec<&Value> = arr.into_iter().flatten().collect();
    if items.is_empty() {
        return None;
    }
    // 1) 名称为「学习」
    if let Some(p) = items
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("学习"))
    {
        return p.get("id").and_then(|x| x.as_str()).map(String::from);
    }
    // 2) 第一个未关闭的清单
    if let Some(p) = items.iter().find(|p| {
        !p.get("closed").and_then(|c| c.as_bool()).unwrap_or(false)
            && p.get("id").and_then(|x| x.as_str()) != Some("inbox")
    }) {
        return p.get("id").and_then(|x| x.as_str()).map(String::from);
    }
    // 3) 兜底：第一个
    items
        .first()
        .and_then(|p| p.get("id"))
        .and_then(|x| x.as_str())
        .map(String::from)
}

// ============================================================================
// 公开接口
// ============================================================================

/// 按日对账：计划 JSON 任务 ↔ 滴答任务（增删改增量；只动带 studyagent 标签的任务）
///
/// 返回 `(created, updated, deleted)` 计数摘要；失败只记录日志。
pub async fn reconcile_day(data_dir: &Path, date: &str) -> Result<(i32, i32, i32), String> {
    if !is_sync_enabled(data_dir) {
        return Ok((0, 0, 0));
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        log::warn!("[dida] 未配置滴答 Token（keyring 或 DIDA_TOKEN），跳过同步 {}", date);
        return Ok((0, 0, 0));
    };

    let Ok(mut plan) = read_daily_plan(data_dir, date) else {
        // 无计划文件：无需同步
        return Ok((0, 0, 0));
    };
    if plan.data.tasks.is_empty() {
        return Ok((0, 0, 0));
    }

    let client = DidaClient::new(token);
    let mut session: Option<String> = None;
    client.initialize(&mut session).await?;
    let mut rid: u64 = 100;

    // 归属项目：优先 settings.ticktick.project_id，未配置则用 list_projects 兜底取第一个
    let project_id = resolve_project_id(data_dir, &client, &mut session, &mut rid).await;

    let undone = client
        .list_tasks_in_window(&mut session, date, false, &mut rid)
        .await?;
    let completed = client
        .list_tasks_in_window(&mut session, date, true, &mut rid)
        .await?;
    let existing: Vec<DidaTask> = undone.into_iter().chain(completed).collect();

    let (created, updated, deleted) = reconcile_with_plan(
        &mut plan,
        &client,
        &mut session,
        &mut rid,
        &existing,
        project_id.as_deref(),
    )
    .await;

    // 回填 dida_task_id 后原子写回（即使无变化也无害）
    if let Err(e) = save_daily_plan(data_dir, &plan) {
        log::warn!("[dida] {} 回填 dida_task_id 保存失败: {}", date, e);
    }

    log::info!(
        "[dida] {} 同步完成: created={} updated={} deleted={}",
        date,
        created,
        updated,
        deleted
    );
    Ok((created, updated, deleted))
}

/// 对账核心：返回 (created, updated, deleted)
async fn reconcile_with_plan(
    plan: &mut DailyPlanFile,
    client: &DidaClient,
    session: &mut Option<String>,
    rid: &mut u64,
    existing: &[DidaTask],
    project_id: Option<&str>,
) -> (i32, i32, i32) {
    // 只把带来源标记的任务纳入对账集；其余任务保持不动
    let owned: Vec<DidaTask> = existing
        .iter()
        .filter(|t| is_owned(&t.tags))
        .cloned()
        .collect();
    let owned_id_to_idx: HashMap<String, usize> = owned
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.clone(), i))
        .collect();

    let mut keep: HashSet<usize> = HashSet::new();
    let mut created = 0i32;
    let mut updated = 0i32;
    let mut deleted = 0i32;

    for task in plan.data.tasks.iter_mut() {
        let title = task.title.trim().to_string();
        let priority = priority_value(&task.priority);
        let tags = make_tags(&task.subject);

        // 1) 已有 dida_task_id 且滴答侧存在 → 内容差异则更新
        if let Some(did) = task.dida_task_id.clone() {
            if let Some(&oi) = owned_id_to_idx.get(&did) {
                let cur = &owned[oi];
                let needs_update = cur.title != title || cur.tags != tags;
                if needs_update {
                    // 用任务自身所在清单 id 更新（任务可能在收件箱/其他清单，传目标清单 id 可能失效）
                    match client
                        .update_task(session, rid, &did, &title, priority, &tags, pid_opt(&cur.project_id))
                        .await
                    {
                        Ok(()) => updated += 1,
                        Err(e) => log::warn!("[dida] 更新任务 {} 失败: {}", did, e),
                    }
                }
                keep.insert(oi);
                continue;
            }
            // 2) 有 id 但滴答侧未找到（可能在滴答手动删除）→ 重建
            match client
                .create_task(session, rid, &plan.meta.date, &title, priority, &tags, project_id)
                .await
            {
                Ok(new_id) => {
                    task.dida_task_id = Some(new_id);
                    created += 1;
                }
                Err(e) => log::warn!("[dida] 重建任务失败 {}: {}", title, e),
            }
            continue;
        }

        // 3) 无 id：先按旧版标题格式收养（`[A] 标题`），命中则补 id + 打标签，否则新建
        let legacy_title = format!("[{:?}] {}", task.priority, task.title);
        let adopted = owned
            .iter()
            .enumerate()
            .find(|(oi, t)| !keep.contains(oi) && t.title == legacy_title)
            .map(|(oi, t)| (oi, t.id.clone()));
        if let Some((oi, did)) = adopted {
            task.dida_task_id = Some(did.clone());
            // 收养旧任务同样用其自身清单 id 更新
            match client
                .update_task(session, rid, &did, &title, priority, &tags, pid_opt(&owned[oi].project_id))
                .await
            {
                Ok(()) => updated += 1,
                Err(e) => log::warn!("[dida] 收养任务 {} 补标签失败: {}", title, e),
            }
            keep.insert(oi);
            continue;
        }

        match client
            .create_task(session, rid, &plan.meta.date, &title, priority, &tags, project_id)
            .await
        {
            Ok(new_id) => {
                task.dida_task_id = Some(new_id);
                created += 1;
            }
            Err(e) => log::warn!("[dida] 创建任务失败 {}: {}", title, e),
        }
    }

    // 4) 对账集内未被计划保留的 owned 任务：删除（已完成任务保留完成历史）
    for (oi, t) in owned.iter().enumerate() {
        if !keep.contains(&oi) {
            if t.done {
                log::info!("[dida] 任务 {} 已在滴答完成且不在计划中，保留完成历史", t.title);
                continue;
            }
            // 删除必须用任务自身所在清单 id：传其他清单 id 时滴答会静默成功但不删除
            match client.delete_task(session, rid, &t.id, pid_opt(&t.project_id)).await {
                Ok(()) => deleted += 1,
                Err(e) => log::warn!("[dida] 删除任务 {} 失败: {}", t.title, e),
            }
        }
    }

    (created, updated, deleted)
}

/// 应用内勾选完成/取消 → 单向同步滴答完成状态（最佳努力，失败仅记录）
pub async fn sync_task_status(data_dir: &Path, task_id: &str, status: &TaskStatus) {
    if !is_sync_enabled(data_dir) {
        return;
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        return;
    };
    if task_id.len() < 10 {
        return;
    }
    let date = &task_id[..10];
    let Ok(plan) = read_daily_plan(data_dir, date) else {
        return;
    };
    let Some(task) = plan
        .data
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .and_then(|t| t.dida_task_id.clone())
    else {
        return;
    };

    let client = DidaClient::new(token);
    let mut session: Option<String> = None;
    if let Err(e) = client.initialize(&mut session).await {
        log::warn!("[dida] 状态同步初始化失败 {}: {}", task_id, e);
        return;
    }
    let mut rid: u64 = 400;
    // complete_task 需要 project_id；未能解析则跳过（避免把任务标错的低频场景）
    let project_id = resolve_project_id(data_dir, &client, &mut session, &mut rid).await;
    match status {
        TaskStatus::Done => {
            if let Err(e) = client
                .complete_task(&mut session, &mut rid, &task, project_id.as_deref())
                .await
            {
                log::warn!("[dida] 标记完成失败 {}: {}", task, e);
            }
        }
        // 取消完成：滴答官方 MCP 不支持 status=0，跳过（以滴答为准的回读会覆盖）
        _ => log::debug!("[dida] 取消完成跳过（滴答 MCP 不支持），task_id={}", task_id),
    }
}

/// 复盘回读：当日窗口内带来源标记的已完成任务标题（去掉旧版标题前缀）
pub async fn fetch_completed_titles(data_dir: &Path, date: &str) -> Vec<String> {
    if !is_sync_enabled(data_dir) {
        return Vec::new();
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        return Vec::new();
    };
    let client = DidaClient::new(token);
    let mut session: Option<String> = None;
    if let Err(e) = client.initialize(&mut session).await {
        log::warn!("[dida] 复盘回读初始化失败 {}: {}", date, e);
        return Vec::new();
    }
    let mut rid: u64 = 300;
    match client
        .list_tasks_in_window(&mut session, date, true, &mut rid)
        .await
    {
        Ok(tasks) => tasks
            .iter()
            .filter(|t| is_owned(&t.tags))
            .map(|t| strip_legacy_prefix(&t.title))
            .collect(),
        Err(e) => {
            log::warn!("[dida] 复盘回读 {} 失败: {}", date, e);
            Vec::new()
        }
    }
}

// ============================================================================
// 真实连通性验证（手动运行，默认忽略）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端验证滴答 MCP：握手 → tools/list → 今日窗口查询 → create/update/complete/delete 往返。
    ///
    /// 需要 DIDA_TOKEN（keyring 或环境变量；运行前可 `$env:DIDA_TOKEN=...`）。
    /// 运行：
    ///   cargo test --lib sync::dida::tests::dida_roundtrip_connectivity -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn dida_roundtrip_connectivity() {
        let Some(token) = crate::secrets::get_dida_token() else {
            println!("SKIP: 未配置 DIDA_TOKEN（keyring 或环境变量）");
            return;
        };
        let client = DidaClient::new(token);
        let mut session: Option<String> = None;
        client
            .initialize(&mut session)
            .await
            .expect("initialize 与滴答 MCP 服务握手应成功");

        // 1. tools/list：确认服务端实际工具名（写路径 schema 依赖）
        let tools = client
            .post(
                json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/list", "params": {} }),
                &mut session,
            )
            .await
            .expect("tools/list 调用应成功");
        let names: Vec<String> = tools
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        println!("[test] 服务端可用工具: {:?}", names);
        for need in [
            "create_task",
            "update_task",
            "complete_task",
            "delete_task",
            "list_undone_tasks_by_date",
            "list_completed_tasks_by_date",
        ] {
            assert!(
                names.iter().any(|n| n == need),
                "服务端缺少工具 {}（应以 tools/list 返回为准适配）",
                need
            );
        }

        // 打印写路径工具的官方参数 schema，作为实现依据
        if let Some(tool_arr) = tools.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
            for t in tool_arr {
                let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
                if matches!(name, "create_task" | "update_task" | "complete_task" | "delete_task" | "list_undone_tasks_by_date" | "list_completed_tasks_by_date") {
                    println!("[test] schema {}: {}", name, t.get("inputSchema").cloned().unwrap_or_default());
                }
            }
        }

        // 2. 只读：今日窗口查询
        let today = crate::data::today_string();
        let mut rid: u64 = 10;
        let undone = client
            .list_tasks_in_window(&mut session, &today, false, &mut rid)
            .await
            .expect("查询今日未完成任务应成功");
        let completed = client
            .list_tasks_in_window(&mut session, &today, true, &mut rid)
            .await
            .expect("查询今日已完成任务应成功");
        println!(
            "[test] 今日窗口: undone={} completed={}",
            undone.len(),
            completed.len()
        );

        // 2.5 归属项目：优先借 list_projects 取一个真实项目（验证项目内任务可被窗口查询到）
        let proj_rid: u64 = 150;
        let projects = client
            .call_tool("list_projects", json!({}), &mut session, proj_rid)
            .await
            .expect("list_projects 应成功");
        let project_id: Option<String> = extract_first_project_id(&projects);
        // 兜底：脚本已知的「学习」清单（push_plan_to_dida.py 中 PROJECT_ID）
        let project_id = project_id
            .or_else(|| Some("6a5b50e2e9ae5b00000000f7".to_string()))
            .expect("应能确定一个项目 id");
        println!(
            "[test] list_projects 原始响应: {}",
            projects.to_string().chars().take(400).collect::<String>()
        );
        println!("[test] 选用 project_id={}", project_id);

        // 清理历史失败运行残留（带 studyagent 标签、标题前缀 SA连通性测试）
        let mut cleanup_rid: u64 = 200;
        for t in client
            .list_tasks_in_window(&mut session, &today, false, &mut cleanup_rid)
            .await
            .expect("查询残留应成功")
        {
            if t.title.starts_with("SA连通性测试") && is_owned(&t.tags) {
                match client
                    .delete_task(&mut session, &mut cleanup_rid, &t.id, pid_opt(&t.project_id))
                    .await
                {
                    Ok(()) => println!("[test] 已清理残留任务 {}", t.id),
                    Err(e) => eprintln!("[test] 清理残留失败 {}: {}", t.id, e),
                }
            }
        }

        // 3. 写路径往返：create → 窗口可见 → update → complete → delete（全程带 studyagent 标签）
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let title = format!("SA连通性测试{}", stamp);
        let tags = vec![SOURCE_TAG.to_string(), "测试".to_string()];

        let id = client
            .create_task(
                &mut session,
                &mut rid,
                &today,
                &title,
                1,
                &tags,
                Some(&project_id),
            )
            .await
            .expect("create_task 应返回任务 id");
        println!("[test] create ok, id={}", id);

        // 创建后应立即能被「今日窗口」查询到（验证重组/回读依赖的列表链路）
        let after = client
            .list_tasks_in_window(&mut session, &today, false, &mut rid)
            .await
            .expect("查询创建后任务应成功");
        assert!(
            after.iter().any(|t| t.id == id),
            "创建的任务 {} 未能被 list_undone_tasks_by_date 查询到（id={}）",
            title,
            id
        );
        println!("[test] 创建后窗口可见 ok");

        client
            .update_task(
                &mut session,
                &mut rid,
                &id,
                &format!("{}_改", title),
                1,
                &tags,
                Some(&project_id),
            )
            .await
            .expect("update_task 应成功");
        println!("[test] update ok");

        client
            .complete_task(&mut session, &mut rid, &id, Some(&project_id))
            .await
            .expect("complete_task 应成功");
        println!("[test] complete ok");

        // 完成后应出现在「已完成窗口」（回读复盘的依赖链路）
        let completed_after = client
            .list_tasks_in_window(&mut session, &today, true, &mut rid)
            .await
            .expect("查询已完成任务应成功");
        assert!(
            completed_after.iter().any(|t| t.id == id),
            "已完成任务 {} 未能被 list_completed_tasks_by_date 查询到",
            id
        );
        println!("[test] 完成后窗口可见 ok");

        client
            .delete_task(&mut session, &mut rid, &id, Some(&project_id))
            .await
            .expect("delete_task 应成功");
        println!("[test] delete ok —— 全链路往返验证通过（create → update → complete → delete）");
    }

    /// 构造测试用日计划任务
    fn sa_test_task(date: &str, idx: usize, title: &str, priority: TaskPriority) -> crate::data::plan::PlanTask {
        crate::data::plan::PlanTask {
            id: format!("{}-{:02}", date, idx),
            subject: SubjectKey::Math,
            title: title.to_string(),
            priority,
            estimated_hours: 1.0,
            goal: "对账E2E测试".to_string(),
            completion_criteria: vec![],
            textbook: None,
            style_tips: None,
            fallback_plan: None,
            status: TaskStatus::Pending,
            dida_task_id: None,
        }
    }

    /// 构造测试用日计划文件（meta.date 为指定日期）
    fn sa_test_plan(date: &str, tasks: Vec<crate::data::plan::PlanTask>) -> DailyPlanFile {
        let mut plan = DailyPlanFile::default();
        plan.version = "2.0".to_string();
        plan.meta.date = date.to_string();
        plan.meta.generated_at = format!("{}T00:00", date);
        plan.meta.r#type = "daily".to_string();
        plan.data.total_tasks = tasks.len() as i32;
        plan.data.total_hours = tasks.len() as f64;
        plan.data.tasks = tasks;
        plan
    }

    /// 端到端验证 reconcile_day 按日对账核心（临时数据目录 + 真实滴答 API，跑完自动清理）。
    ///
    /// 覆盖：首次同步创建并回填 dida_task_id → 幂等重跑无变化 → 重排（更新/删除/新建）
    /// → 复盘回读 fetch_completed_titles。
    /// 安全：若今日窗口存在非本测试前缀的 studyagent 任务则直接跳过，绝不触碰真实同步数据。
    ///
    /// 需要 DIDA_TOKEN（keyring 或环境变量）。运行：
    ///   cargo test --lib sync::dida::tests::dida_reconcile_day_e2e -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn dida_reconcile_day_e2e() {
        let Some(token) = crate::secrets::get_dida_token() else {
            println!("SKIP: 未配置 DIDA_TOKEN（keyring 或环境变量）");
            return;
        };
        const PROJECT_ID: &str = "6a5b50e2e9ae5b00000000f7"; // 「学习」清单
        const PREFIX: &str = "SA对账测试";
        let today = crate::data::today_string();

        // ── 0. 直连客户端：预清理本测试残留 + 安全检查（存在真实 studyagent 任务则跳过）──
        let client = DidaClient::new(token);
        let mut session: Option<String> = None;
        client
            .initialize(&mut session)
            .await
            .expect("initialize 应成功");
        let mut rid: u64 = 500;
        let mut leftovers: Vec<DidaTask> = Vec::new();
        for completed in [false, true] {
            leftovers.extend(
                client
                    .list_tasks_in_window(&mut session, &today, completed, &mut rid)
                    .await
                    .expect("查询今日窗口应成功")
                    .into_iter()
                    .filter(|t| is_owned(&t.tags)),
            );
        }
        for t in &leftovers {
            if t.title.starts_with(PREFIX) || t.title.starts_with("SA连通性测试") {
                // 删除必须用任务自身清单 id（收件箱任务传「学习」id 会静默不生效）
                client
                    .delete_task(&mut session, &mut rid, &t.id, pid_opt(&t.project_id))
                    .await
                    .expect("清理测试残留任务应成功");
                println!("[test] 已清理残留任务: {}（清单 {}）", t.title, t.project_id);
            } else {
                println!("SKIP: 今日存在真实 studyagent 任务 {:?}，为避免误删跳过对账 E2E", t.title);
                return;
            }
        }

        // 滴答列表接口对删除有短暂延迟：轮询等待残留彻底从窗口消失，
        // 避免被随后的对账重复计入 deleted 计数
        for _ in 0..10 {
            let visible = client
                .list_tasks_in_window(&mut session, &today, false, &mut rid)
                .await
                .expect("轮询查询应成功")
                .into_iter()
                .filter(|t| is_owned(&t.tags))
                .count();
            if visible == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // ── 1. 临时数据目录 + 启用同步的 settings ──
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!("sa_dida_e2e_{}", stamp));
        std::fs::create_dir_all(tmp.join("config")).expect("创建临时目录应成功");
        std::fs::write(
            tmp.join("config").join("settings.json"),
            format!(r#"{{"ticktick": {{"enabled": true, "project_id": "{}"}}}}"#, PROJECT_ID),
        )
        .expect("写入 settings 应成功");

        // ── 2. 写入含 2 个任务的今日计划 ──
        let t1 = format!("{}甲·高数复习", PREFIX);
        let t2 = format!("{}乙·英语阅读", PREFIX);
        let plan = sa_test_plan(
            &today,
            vec![
                sa_test_task(&today, 1, &t1, TaskPriority::A),
                sa_test_task(&today, 2, &t2, TaskPriority::B),
            ],
        );
        save_daily_plan(&tmp, &plan).expect("写入日计划应成功");

        // ── 3. 首次对账：全部新建，并回填 dida_task_id ──
        assert_eq!(
            reconcile_day(&tmp, &today).await.expect("首次对账应成功"),
            (2, 0, 0),
            "首次对账应新建 2 个任务"
        );
        let mut plan = read_daily_plan(&tmp, &today).expect("回读日计划应成功");
        for t in &plan.data.tasks {
            assert!(t.dida_task_id.is_some(), "首次同步后应回填 dida_task_id: {}", t.title);
        }
        println!("[test] 首次对账 ok（新建 2 · dida_task_id 已回填）");

        // ── 4. 幂等：重跑无变化 ──
        assert_eq!(
            reconcile_day(&tmp, &today).await.expect("幂等对账应成功"),
            (0, 0, 0),
            "无变化重跑不应产生任何增删改"
        );
        println!("[test] 幂等重跑 ok（0 增 0 改 0 删）");

        // ── 5. 重排：任务甲改标题、任务乙移除、新增任务丙 → (1 新建, 1 更新, 1 删除) ──
        let t1_new = format!("{}甲·高数复习(重排)", PREFIX);
        let t3 = format!("{}丙·政治刷题", PREFIX);
        plan.data.tasks[0].title = t1_new.clone();
        plan.data.tasks.pop();
        plan.data
            .tasks
            .push(sa_test_task(&today, 3, &t3, TaskPriority::C));
        save_daily_plan(&tmp, &plan).expect("写入重排后日计划应成功");
        assert_eq!(
            reconcile_day(&tmp, &today).await.expect("重排对账应成功"),
            (1, 1, 1),
            "重排后应 新建1(丙) 更新1(甲改标题) 删除1(乙被移除)"
        );
        let plan = read_daily_plan(&tmp, &today).expect("回读重排后日计划应成功");
        let task3 = plan
            .data
            .tasks
            .iter()
            .find(|t| t.title == t3)
            .expect("重排后应存在任务丙");
        assert!(task3.dida_task_id.is_some(), "新增任务丙应回填 dida_task_id");
        println!("[test] 重排对账 ok（新建1 更新1 删除1）");

        // ── 6. 复盘回读：滴答侧完成任务丙 → fetch_completed_titles 应包含其标题 ──
        let did3 = task3.dida_task_id.clone().unwrap();
        client
            .complete_task(&mut session, &mut rid, &did3, Some(PROJECT_ID))
            .await
            .expect("滴答侧完成任务丙应成功");
        let titles = fetch_completed_titles(&tmp, &today).await;
        assert!(
            titles.iter().any(|t| *t == t3),
            "复盘回读应包含滴答侧已完成的任务丙，实际: {:?}",
            titles
        );
        println!("[test] 复盘回读 ok（已完成标题含任务丙）");

        // ── 7. 清理：删除测试任务与临时目录 ──
        for completed in [false, true] {
            for t in client
                .list_tasks_in_window(&mut session, &today, completed, &mut rid)
                .await
                .expect("清理前查询应成功")
            {
                if is_owned(&t.tags) && t.title.starts_with(PREFIX) {
                    client
                        .delete_task(&mut session, &mut rid, &t.id, pid_opt(&t.project_id))
                        .await
                        .expect("清理测试任务应成功");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        println!("[test] 清理完成 —— reconcile_day 对账 E2E 全链路验证通过");
    }
}