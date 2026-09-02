//! 滴答清单（Dida365）同步模块
//!
//! 基于 TickTick / Dida365 Open API（`https://api.dida365.com/open/v1/*`）直连滴答官方服务，
//! 认证方式为 Personal Access Token（`Authorization: Bearer <token>`）。
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::data::plan::{read_daily_plan, save_daily_plan, DailyPlanFile};
use crate::data::state::{SubjectKey, TaskPriority, TaskStatus};

/// TickTick / Dida365 Open API 基础地址（滴答国内站）
const API_BASE: &str = "https://api.dida365.com/open/v1";
/// 来源标记：所有本系统创建/管理的滴答任务固定携带
const SOURCE_TAG: &str = "studyagent";
const TIMEZONE: &str = "Asia/Shanghai";
/// 滴答 Open API 单次 HTTP 请求超时：同步是「尽力而为」的旁路，绝不允许拖垮主流程
const API_TIMEOUT: Duration = Duration::from_secs(10);
/// 单次对账（reconcile_day）总时限：内部多请求串行（含逐任务增删改），
/// 半死不活的服务端可能让每个请求都耗尽 10s，总时限保证整体封顶。
const RECONCILE_TIMEOUT: Duration = Duration::from_secs(60);

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

/// 为计划任务生成滴答标签集合（仅两类：科目前置、来源标记在后）
fn make_tags(s: &SubjectKey) -> Vec<String> {
    vec![subject_tag(s), SOURCE_TAG.to_string()]
}

/// 任务写入滴答的标题：`[科目] 原标题`（如 `[数学] 刷高数第3章习题`）
fn task_title(subject: &SubjectKey, title: &str) -> String {
    format!("[{}] {}", subject_tag(subject), title.trim())
}

/// 还原滴答标题为计划原标题：去掉学科前缀（`[数学] ` 等）与旧版优先级前缀（`[A] ` / `[B] ` / `[C] `）
fn strip_title_prefix(title: &str) -> String {
    let t = title.trim();
    for p in [
        "[数学] ",
        "[英语] ",
        "[政治] ",
        "[专业课] ",
        "[A] ",
        "[B] ",
        "[C] ",
    ] {
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
// TickTick / Dida365 Open API 客户端
// ============================================================================

/// 滴答 Open API 客户端（每次调用即用即建，不常驻）
struct DidaClient {
    token: String,
    /// 复用同一 http client，并施加总超时，避免网络挂起拖累主线程
    http: reqwest::Client,
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
    /// 开始日期（YYYY-MM-DD，取自 startDate；用于判断任务所属日期）
    start_date: Option<String>,
    /// 截止日期（YYYY-MM-DD，取自 dueDate；用于判断任务是否过期）
    due_date: Option<String>,
}

/// 解析滴答 API 返回的任务 JSON 为 `DidaTask`
fn parse_dida_task(t: &Value, done: bool) -> DidaTask {
    DidaTask {
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
        done,
        start_date: t
            .get("startDate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.get(..10))
            .map(String::from),
        due_date: t
            .get("dueDate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.get(..10))
            .map(String::from),
    }
}

impl DidaClient {
    fn new(token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { token, http }
    }

    /// 构造带认证头的请求构造器
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{API_BASE}{path}");
        self.http
            .request(method, url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
    }

    /// 拉取指定日期窗口内的任务（未完成或已完成）
    ///
    /// Open API 没有按日期窗口直接列任务的端点，分两步：
    /// 1. 未完成：`POST /task/undone`（按 startDate/endDate 范围筛选）
    /// 2. 已完成：`POST /task/completed`（按 completedTime 范围筛选）
    async fn list_tasks_in_window(
        &self,
        date: &str,
        completed: bool,
    ) -> Result<Vec<DidaTask>, String> {
        let end = crate::data::add_days(date, 1).unwrap_or_else(|_| date.to_string());
        let start_iso = format!("{}T00:00:00+0800", date);
        let end_iso = format!("{}T00:00:00+0800", end);

        let arr = if completed {
            // 已完成任务：POST /task/completed，按 completedTime 范围筛选（最多 200 条）
            let body = json!({
                "startDate": start_iso,
                "endDate": end_iso,
            });
            let resp = self
                .request(reqwest::Method::POST, "/task/completed")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("滴答 API 查询已完成任务失败: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("滴答 API 查询已完成任务 HTTP {}", resp.status()));
            }
            resp.json::<Value>()
                .await
                .map_err(|e| format!("解析已完成任务响应失败: {e}"))?
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else {
            // 未完成任务：POST /task/undone（startDate/endDate 必填，范围最大 14 天）
            let body = json!({
                "startDate": start_iso,
                "endDate": end_iso,
            });
            let resp = self
                .request(reqwest::Method::POST, "/task/undone")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("滴答 API 查询未完成任务失败: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("滴答 API 查询未完成任务 HTTP {}", resp.status()));
            }
            resp.json::<Value>()
                .await
                .map_err(|e| format!("解析未完成任务响应失败: {e}"))?
                .as_array()
                .cloned()
                .unwrap_or_default()
        };

        Ok(arr
            .into_iter()
            .map(|t| parse_dida_task(&t, completed))
            .collect())
    }

    /// 拉取 `[start_date, end_date]` 区间（含两端全天）的未完成任务
    ///
    /// Open API `/task/undone` 的 startDate/endDate 范围上限 14 天，用于过往任务清理等批量场景。
    async fn list_undone_between(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<DidaTask>, String> {
        let end = crate::data::add_days(end_date, 1).unwrap_or_else(|_| end_date.to_string());
        let body = json!({
            "startDate": format!("{}T00:00:00+0800", start_date),
            "endDate": format!("{}T00:00:00+0800", end),
        });
        let resp = self
            .request(reqwest::Method::POST, "/task/undone")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("滴答 API 查询未完成任务失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("滴答 API 查询未完成任务 HTTP {}", resp.status()));
        }
        Ok(resp
            .json::<Value>()
            .await
            .map_err(|e| format!("解析未完成任务响应失败: {e}"))?
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|t| parse_dida_task(&t, false))
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_task(
        &self,
        date: &str,
        title: &str,
        priority: i32,
        tags: &[String],
        project_id: Option<&str>,
    ) -> Result<String, String> {
        let mut task = serde_json::Map::new();
        task.insert("title".into(), json!(title));
        task.insert("priority".into(), json!(priority));
        task.insert("isAllDay".into(), json!(false));
        task.insert("dueDate".into(), json!(format!("{}T22:00:00+0800", date)));
        task.insert("startDate".into(), json!(format!("{}T09:00:00+0800", date)));
        task.insert("timeZone".into(), json!(TIMEZONE));
        task.insert("kind".into(), json!("TEXT"));
        task.insert("tags".into(), json!(tags));
        if let Some(pid) = project_id {
            task.insert("projectId".into(), json!(pid));
        }
        let resp = self
            .request(reqwest::Method::POST, "/task")
            .json(&Value::Object(task))
            .send()
            .await
            .map_err(|e| format!("滴答 API 创建任务失败: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "滴答 API 创建任务 HTTP {status}: {}",
                &body[..body.len().min(400)]
            ));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("解析创建任务响应失败: {e}"))?;
        v.get("id")
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| format!("create_task 响应缺少 id: {}", v))
    }

    /// 更新任务标题/优先级/标签
    ///
    /// Open API：`POST /task/{taskId}`，body 内 `id` 与 `projectId` 必填。
    #[allow(clippy::too_many_arguments)]
    async fn update_task(
        &self,
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
        let path = format!("/task/{id}");
        let resp = self
            .request(reqwest::Method::POST, &path)
            .json(&Value::Object(task))
            .send()
            .await
            .map_err(|e| format!("滴答 API 更新任务失败: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "滴答 API 更新任务 HTTP {status}: {}",
                &body[..body.len().min(400)]
            ));
        }
        Ok(())
    }

    /// 标记完成（Open API：`POST /project/{projectId}/task/{taskId}/complete`）
    async fn complete_task(&self, id: &str, project_id: Option<&str>) -> Result<(), String> {
        let pid = project_id.ok_or_else(|| "缺少滴答 project_id，无法完成任务".to_string())?;
        let path = format!("/project/{pid}/task/{id}/complete");
        let resp = self
            .request(reqwest::Method::POST, &path)
            .send()
            .await
            .map_err(|e| format!("滴答 API 完成任务失败: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "滴答 API 完成任务 HTTP {status}: {}",
                &body[..body.len().min(400)]
            ));
        }
        Ok(())
    }

    /// 删除任务（Open API：`DELETE /project/{projectId}/task/{taskId}`）
    async fn delete_task(&self, id: &str, project_id: Option<&str>) -> Result<(), String> {
        let pid = project_id.ok_or_else(|| "缺少滴答 project_id，无法删除任务".to_string())?;
        let path = format!("/project/{pid}/task/{id}");
        let resp = self
            .request(reqwest::Method::DELETE, &path)
            .send()
            .await
            .map_err(|e| format!("滴答 API 删除任务失败: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "滴答 API 删除任务 HTTP {status}: {}",
                &body[..body.len().min(400)]
            ));
        }
        Ok(())
    }
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
async fn resolve_project_id(data_dir: &Path, client: &DidaClient) -> Option<String> {
    if let Some(pid) = project_id_of(data_dir) {
        return Some(pid);
    }
    match client.list_projects().await {
        Ok(projects) => {
            let pid = pick_default_project(&projects);
            log::info!("[dida] 未配置 project_id，自动选用项目 {:?}", pid);
            if pid.is_none() {
                log::warn!("[dida] list_projects 返回为空，无法确定归属项目");
            }
            pid
        }
        Err(e) => {
            log::warn!("[dida] list_projects 失败: {}", e);
            None
        }
    }
}

/// 从项目列表中挑选归属项目 id
///
/// 策略：优先名称为「学习」的清单；否则第一个未关闭（closed != true）的清单；再退第一个。
fn pick_default_project(projects: &[DidaProject]) -> Option<String> {
    if projects.is_empty() {
        return None;
    }
    // 1) 名称为「学习」
    if let Some(p) = projects.iter().find(|p| p.name == "学习") {
        return Some(p.id.clone());
    }
    // 2) 第一个未关闭的清单（inbox 是特殊 id，跳过）
    if let Some(p) = projects
        .iter()
        .find(|p| p.id != "inbox" && !p.id.is_empty())
    {
        return Some(p.id.clone());
    }
    // 3) 兜底：第一个
    projects.first().map(|p| p.id.clone())
}

// ============================================================================
// 公开接口
// ============================================================================

/// 后台按日对账：不阻塞主流程（生成/勾选/复盘命令不再等待网络）。
///
/// 在独立任务中自行获取 io_lock 覆盖「读-改-写日计划」的落盘点（回填 dida_task_id），
/// 与计划生成等写命令串行化；单次请求有 API_TIMEOUT 兜底，网络异常最多延迟写操作几秒。
pub fn spawn_reconcile_day(io_lock: Arc<tokio::sync::Mutex<()>>, data_dir: PathBuf, date: String) {
    tauri::async_runtime::spawn(async move {
        let _guard = io_lock.lock().await;
        if let Err(e) = reconcile_day(&data_dir, &date).await {
            log::warn!("[dida] 后台对账 {} 失败: {}", date, e);
        }
    });
}

/// 后台多日对账：分批最多 3 个会话并行（避免串行放大延迟），失败仅记录日志。
///
/// 锁在整批同步期间持有：网络挂起有超时兜底，且同步在后台执行，不阻塞任何命令返回。
pub fn spawn_reconcile_days(
    io_lock: Arc<tokio::sync::Mutex<()>>,
    data_dir: PathBuf,
    dates: Vec<String>,
) {
    tauri::async_runtime::spawn(async move {
        let _guard = io_lock.lock().await;
        for chunk in dates.chunks(3) {
            let mut set = tokio::task::JoinSet::new();
            // 需要持有 String 所有权（spawn 任务要求 'static），clippy 的 cloned 建议会导致借用逃逸
            #[allow(clippy::unnecessary_to_owned)]
            for d in chunk.iter().cloned() {
                let dd = data_dir.clone();
                set.spawn(async move { reconcile_day(&dd, &d).await });
            }
            while let Some(joined) = set.join_next().await {
                match joined {
                    Ok(Ok((c, u, d))) => {
                        log::debug!("[dida] 后台对账完成: c={} u={} d={}", c, u, d)
                    }
                    Ok(Err(e)) => log::warn!("[dida] 后台对账失败: {}", e),
                    Err(e) => log::warn!("[dida] 后台对账任务异常: {}", e),
                }
            }
        }
    });
}

/// 按日对账：计划 JSON 任务 ↔ 滴答任务（增删改增量；只动带 studyagent 标签的任务）
///
/// 返回 `(created, updated, deleted)` 计数摘要；失败只记录日志。
/// 整体执行受 `RECONCILE_TIMEOUT` 总时限约束（覆盖内部逐任务写请求的累加时长）；
/// 中途超时放弃本次同步，残留的滴答侧改动会在下次对账时按标题收养/删除自动收敛。
pub async fn reconcile_day(data_dir: &Path, date: &str) -> Result<(i32, i32, i32), String> {
    match tokio::time::timeout(RECONCILE_TIMEOUT, reconcile_day_inner(data_dir, date)).await {
        Ok(Ok((c, u, d))) => Ok((c, u, d)),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            log::warn!(
                "[dida] 对账 {} 超过总时限 {}s，放弃本次同步",
                date,
                RECONCILE_TIMEOUT.as_secs()
            );
            Err(format!(
                "对账 {} 超过总时限 {}s",
                date,
                RECONCILE_TIMEOUT.as_secs()
            ))
        }
    }
}

/// 对账主体（无总时限，由 `reconcile_day` 统一包裹）
async fn reconcile_day_inner(data_dir: &Path, date: &str) -> Result<(i32, i32, i32), String> {
    if !is_sync_enabled(data_dir) {
        return Ok((0, 0, 0));
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        log::warn!(
            "[dida] 未配置滴答 Token（keyring 或 DIDA_TOKEN），跳过同步 {}",
            date
        );
        return Ok((0, 0, 0));
    };

    let Ok(mut plan) = read_daily_plan(data_dir, date) else {
        // 无计划文件：无需同步
        return Ok((0, 0, 0));
    };
    // M1：即使计划任务为空也继续对账——把当日已不再属于计划的未完成 studyagent 任务清理掉
    // （完成的任务保留完成历史，不会误删）

    let client = DidaClient::new(token);

    // 归属项目：优先 settings.ticktick.project_id，未配置则用 list_projects 兜底取第一个
    let project_id = resolve_project_id(data_dir, &client).await;

    let undone = client.list_tasks_in_window(date, false).await?;
    let completed = client.list_tasks_in_window(date, true).await?;
    let existing: Vec<DidaTask> = undone.into_iter().chain(completed).collect();

    let (created, updated, deleted) =
        reconcile_with_plan(&mut plan, &client, &existing, project_id.as_deref()).await;

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
        // 滴答侧统一存放 `[科目] 原标题`
        let push_title = task_title(&task.subject, &title);

        // 1) 已有 dida_task_id 且滴答侧存在 → 内容差异则更新
        if let Some(did) = task.dida_task_id.clone() {
            if let Some(&oi) = owned_id_to_idx.get(&did) {
                let cur = &owned[oi];
                let needs_update = cur.title != push_title || cur.tags != tags;
                if needs_update {
                    // 用任务自身所在清单 id 更新（任务可能在收件箱/其他清单，传目标清单 id 可能失效）
                    match client
                        .update_task(&did, &push_title, priority, &tags, pid_opt(&cur.project_id))
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
                .create_task(&plan.meta.date, &push_title, priority, &tags, project_id)
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

        // 3) 无 id：先按旧版标题格式收养（`[科目] 标题` / 旧 `[A] 标题`），命中则补 id + 规范化，否则新建
        let legacy_title = format!("[{:?}] {}", task.priority, task.title);
        let adopted = owned
            .iter()
            .enumerate()
            .find(|(oi, t)| {
                !keep.contains(oi) && (t.title == push_title || t.title == legacy_title)
            })
            .map(|(oi, t)| (oi, t.id.clone()));
        if let Some((oi, did)) = adopted {
            task.dida_task_id = Some(did.clone());
            // 收养旧任务同样用其自身清单 id 更新（顺带把标题与标签规范化为新格式）
            match client
                .update_task(
                    &did,
                    &push_title,
                    priority,
                    &tags,
                    pid_opt(&owned[oi].project_id),
                )
                .await
            {
                Ok(()) => updated += 1,
                Err(e) => log::warn!("[dida] 收养任务 {} 规范化失败: {}", title, e),
            }
            keep.insert(oi);
            continue;
        }

        match client
            .create_task(&plan.meta.date, &push_title, priority, &tags, project_id)
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
                log::info!(
                    "[dida] 任务 {} 已在滴答完成且不在计划中，保留完成历史",
                    t.title
                );
                continue;
            }
            // 删除必须用任务自身所在清单 id：传其他清单 id 时滴答会静默成功但不删除
            match client.delete_task(&t.id, pid_opt(&t.project_id)).await {
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
    // 防御：仅接受合法日期前缀的任务 id（`get(..10)` 在非字符边界返回 None，不会 panic）
    let Some(date) = task_id.get(..10) else {
        return;
    };
    if crate::data::validate_date(date).is_err() {
        return;
    }
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
    // complete_task 需要 project_id；未能解析则跳过（避免把任务标错的低频场景）
    let project_id = resolve_project_id(data_dir, &client).await;
    match status {
        TaskStatus::Done => {
            if let Err(e) = client.complete_task(&task, project_id.as_deref()).await {
                log::warn!("[dida] 标记完成失败 {}: {}", task, e);
            }
        }
        // 取消完成：滴答 Open API 不支持回退已完成任务，跳过（以滴答为准的回读会覆盖）
        _ => log::debug!(
            "[dida] 取消完成跳过（滴答 Open API 不支持），task_id={}",
            task_id
        ),
    }
}

/// 复盘回读：当日窗口内带来源标记的已完成任务标题（还原为计划原标题，去掉 `[科目] ` / 旧 `[A] ` 前缀）
pub async fn fetch_completed_titles(data_dir: &Path, date: &str) -> Vec<String> {
    if !is_sync_enabled(data_dir) {
        return Vec::new();
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        return Vec::new();
    };
    let client = DidaClient::new(token);
    match client.list_tasks_in_window(date, true).await {
        Ok(tasks) => tasks
            .iter()
            .filter(|t| is_owned(&t.tags))
            .map(|t| strip_title_prefix(&t.title))
            .collect(),
        Err(e) => {
            log::warn!("[dida] 复盘回读 {} 失败: {}", date, e);
            Vec::new()
        }
    }
}

/// 过往未完成任务回看窗口（天）：清理仅覆盖该范围内的过期任务。
/// Open API `/task/undone` 单次范围查询上限 14 天，超出部分会被服务端截断，故取 14。
const STALE_LOOKBACK_DAYS: i64 = 14;

/// 从滴答任务时间字段提取所属日期（YYYY-MM-DD）
///
/// 过期判断优先使用 `dueDate`（截止日期），缺失时回退 `startDate`；
/// 两者都无法解析则返回 `None`（保守跳过，不删除）。
fn task_date_of(t: &DidaTask) -> Option<String> {
    for raw in [&t.due_date, &t.start_date] {
        if let Some(date) = raw.as_deref() {
            if crate::data::validate_date(date).is_ok() {
                return Some(date.to_string());
            }
        }
    }
    None
}

/// 清理过往（已过期）未完成的 studyagent 任务（提交复盘时调用）
///
/// 规则：
/// - 仅处理带 `studyagent` 来源标签的未完成任务；
/// - 仅处理任务所属日期早于今天（< 今天）的「过期」任务；
/// - 已完成任务保留完成历史，不删除；今天及未来的任务由按日对账管理，不受影响；
/// - 单次范围查询（回看 `STALE_LOOKBACK_DAYS` 天），删除失败仅记录日志，不阻塞复盘提交。
///
/// 返回成功删除的任务数量。
pub async fn cleanup_stale_tasks(data_dir: &Path) -> i32 {
    if !is_sync_enabled(data_dir) {
        return 0;
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        return 0;
    };
    let client = DidaClient::new(token);
    let today = crate::data::today_string();
    let Ok(start) = crate::data::add_days(&today, -STALE_LOOKBACK_DAYS) else {
        return 0;
    };
    let Ok(tasks) = client.list_undone_between(&start, &today).await else {
        log::warn!("[dida] 过往未完成任务查询失败，跳过清理");
        return 0;
    };

    let mut deleted = 0i32;
    for t in tasks.iter().filter(|t| is_owned(&t.tags) && !t.done) {
        let Some(owned_date) = task_date_of(t) else {
            continue;
        };
        if owned_date.as_str() >= today.as_str() {
            continue; // 今天及未来不清理，由按日对账处理
        }
        // 删除必须用任务自身所在清单 id：传其他清单 id 时滴答会静默成功但不删除
        match client.delete_task(&t.id, pid_opt(&t.project_id)).await {
            Ok(()) => {
                deleted += 1;
                log::info!(
                    "[dida] 清理过往未完成任务: {}（原日期 {}）",
                    t.title,
                    owned_date
                );
            }
            Err(e) => log::warn!("[dida] 清理过往任务 {} 失败: {}", t.title, e),
        }
    }
    if deleted > 0 {
        log::info!("[dida] 过往未完成任务清理完成: 共删除 {} 条", deleted);
    }
    deleted
}

// ============================================================================
// 滴答清单项目（设置页归属清单选择）
// ============================================================================

/// 滴答清单项目（序列化给前端展示/选择）
#[derive(Debug, Clone, Serialize)]
pub struct DidaProject {
    pub id: String,
    pub name: String,
}

impl DidaClient {
    /// 拉取滴答清单项目列表（Open API：`GET /project`）
    async fn list_projects(&self) -> Result<Vec<DidaProject>, String> {
        let resp = self
            .request(reqwest::Method::GET, "/project")
            .send()
            .await
            .map_err(|e| format!("滴答 API 获取项目列表失败: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "滴答 API 获取项目列表 HTTP {status}: {}",
                &body[..body.len().min(400)]
            ));
        }
        let arr: Vec<Value> = resp
            .json()
            .await
            .map_err(|e| format!("解析项目列表响应失败: {e}"))?;
        Ok(arr
            .iter()
            .filter_map(|p| {
                let id = p.get("id").and_then(|x| x.as_str())?;
                Some(DidaProject {
                    id: id.to_string(),
                    name: p
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect())
    }
}

/// 拉取滴答清单项目列表（供设置页选择归属清单）；同步未启用/未配置 Token/失败返回空
pub async fn fetch_projects(data_dir: &Path) -> Vec<DidaProject> {
    if !is_sync_enabled(data_dir) {
        return Vec::new();
    }
    let Some(token) = crate::secrets::get_dida_token() else {
        return Vec::new();
    };
    let client = DidaClient::new(token);
    match client.list_projects().await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[dida] list_projects 失败: {}", e);
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

    /// 端到端验证滴答 Open API：查询 → create/update/complete/delete 往返。
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

        // 1. list_projects：确认可用项目
        let projects = client.list_projects().await.expect("list_projects 应成功");
        println!("[test] 项目数: {}", projects.len());
        let project_id: String = pick_default_project(&projects)
            .or_else(|| Some("6a5b50e2e9ae5b00000000f7".to_string()))
            .expect("应能确定一个项目 id");
        println!("[test] 选用 project_id={}", project_id);

        // 2. 只读：今日窗口查询
        let today = crate::data::today_string();
        let undone = client
            .list_tasks_in_window(&today, false)
            .await
            .expect("查询今日未完成任务应成功");
        let completed = client
            .list_tasks_in_window(&today, true)
            .await
            .expect("查询今日已完成任务应成功");
        println!(
            "[test] 今日窗口: undone={} completed={}",
            undone.len(),
            completed.len()
        );

        // 清理历史失败运行残留（带 studyagent 标签、标题前缀 SA连通性测试）
        for t in client
            .list_tasks_in_window(&today, false)
            .await
            .expect("查询残留应成功")
        {
            if t.title.starts_with("SA连通性测试") && is_owned(&t.tags) {
                match client.delete_task(&t.id, pid_opt(&t.project_id)).await {
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
            .create_task(&today, &title, 1, &tags, Some(project_id.as_str()))
            .await
            .expect("create_task 应返回任务 id");
        println!("[test] create ok, id={}", id);

        // 创建后应立即能被「今日窗口」查询到（验证重组/回读依赖的列表链路）
        let after = client
            .list_tasks_in_window(&today, false)
            .await
            .expect("查询创建后任务应成功");
        assert!(
            after.iter().any(|t| t.id == id),
            "创建的任务 {} 未能被 undone 窗口查询到",
            title
        );
        println!("[test] 创建后窗口可见 ok");

        client
            .update_task(
                &id,
                &format!("{}_改", title),
                1,
                &tags,
                Some(project_id.as_str()),
            )
            .await
            .expect("update_task 应成功");
        println!("[test] update ok");

        client
            .complete_task(&id, Some(project_id.as_str()))
            .await
            .expect("complete_task 应成功");
        println!("[test] complete ok");

        // 完成后应出现在「已完成窗口」（回读复盘的依赖链路）
        let completed_after = client
            .list_tasks_in_window(&today, true)
            .await
            .expect("查询已完成任务应成功");
        assert!(
            completed_after.iter().any(|t| t.id == id),
            "已完成任务 {} 未能被 completed 窗口查询到",
            id
        );
        println!("[test] 完成后窗口可见 ok");

        // 已完成任务通过 DELETE 清理（Open API 的 complete 会让任务变为已完成态，DELETE 仍可清理）
        client
            .delete_task(&id, Some(project_id.as_str()))
            .await
            .expect("delete_task 应成功");
        println!("[test] delete ok —— 全链路往返验证通过（create → update → complete → delete）");
    }

    /// 构造测试用日计划任务
    fn sa_test_task(
        date: &str,
        idx: usize,
        title: &str,
        priority: TaskPriority,
    ) -> crate::data::plan::PlanTask {
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
        let mut plan = DailyPlanFile {
            version: "2.0".to_string(),
            ..Default::default()
        };
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
        let mut leftovers: Vec<DidaTask> = Vec::new();
        for completed in [false, true] {
            leftovers.extend(
                client
                    .list_tasks_in_window(&today, completed)
                    .await
                    .expect("查询今日窗口应成功")
                    .into_iter()
                    .filter(|t| is_owned(&t.tags)),
            );
        }
        for t in &leftovers {
            if strip_title_prefix(&t.title).starts_with(PREFIX)
                || t.title.starts_with("SA连通性测试")
            {
                // 删除必须用任务自身清单 id（收件箱任务传「学习」id 会静默不生效）
                client
                    .delete_task(&t.id, pid_opt(&t.project_id))
                    .await
                    .expect("清理测试残留任务应成功");
                println!(
                    "[test] 已清理残留任务: {}（清单 {}）",
                    t.title, t.project_id
                );
            } else {
                println!(
                    "SKIP: 今日存在真实 studyagent 任务 {:?}，为避免误删跳过对账 E2E",
                    t.title
                );
                return;
            }
        }

        // 滴答列表接口对删除有短暂延迟：轮询等待残留彻底从窗口消失，
        // 避免被随后的对账重复计入 deleted 计数
        for _ in 0..10 {
            let visible = client
                .list_tasks_in_window(&today, false)
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
            format!(
                r#"{{"ticktick": {{"enabled": true, "project_id": "{}"}}}}"#,
                PROJECT_ID
            ),
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
            assert!(
                t.dida_task_id.is_some(),
                "首次同步后应回填 dida_task_id: {}",
                t.title
            );
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
        assert!(
            task3.dida_task_id.is_some(),
            "新增任务丙应回填 dida_task_id"
        );
        println!("[test] 重排对账 ok（新建1 更新1 删除1）");

        // ── 6. 复盘回读：滴答侧完成任务丙 → fetch_completed_titles 应包含其标题 ──
        let did3 = task3.dida_task_id.clone().unwrap();
        client
            .complete_task(&did3, Some(PROJECT_ID))
            .await
            .expect("滴答侧完成任务丙应成功");
        let titles = fetch_completed_titles(&tmp, &today).await;
        assert!(
            titles.contains(&t3),
            "复盘回读应包含滴答侧已完成的任务丙，实际: {:?}",
            titles
        );
        println!("[test] 复盘回读 ok（已完成标题含任务丙）");

        // ── 7. 清理：删除测试任务与临时目录 ──
        for completed in [false, true] {
            for t in client
                .list_tasks_in_window(&today, completed)
                .await
                .expect("清理前查询应成功")
            {
                if is_owned(&t.tags) && strip_title_prefix(&t.title).starts_with(PREFIX) {
                    client
                        .delete_task(&t.id, pid_opt(&t.project_id))
                        .await
                        .expect("清理测试任务应成功");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        println!("[test] 清理完成 —— reconcile_day 对账 E2E 全链路验证通过");
    }
}
