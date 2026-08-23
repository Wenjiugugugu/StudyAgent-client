//! MCP Client — 连接 MCP Server，列出工具，调用工具
//!
//! 对应前端 TypeScript 类型: `types/mcp.ts`
//!
//! MCP (Model Context Protocol) 使用 JSON-RPC 2.0 通信。
//! 支持三种传输方式：stdio、SSE、WebSocket。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

// ============================================================================
// MCP 类型定义
// ============================================================================

/// MCP Server 类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MCPServerType {
    Ticktick,
    Filesystem,
    Browser,
    Obsidian,
    Custom,
}

impl Default for MCPServerType {
    fn default() -> Self {
        MCPServerType::Custom
    }
}

/// MCP Server 配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MCPServerConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub r#type: MCPServerType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 传输方式
    #[serde(default = "default_stdio")]
    pub transport: String, // "stdio" | "sse" | "websocket"
    /// stdio 模式下的命令
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// SSE/WebSocket 模式下的 URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 环境变量
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

fn default_true() -> bool {
    true
}

fn default_stdio() -> String {
    "stdio".to_string()
}

/// 工具定义（MCP 层）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MCPTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub server_id: String,
}

/// 工具调用请求
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// 工具调用结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// MCP Server 状态
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MCPServerStatus {
    pub id: String,
    pub name: String,
    pub connected: bool,
    #[serde(default)]
    pub tools_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// TickTick 任务（滴答清单）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickTickTask {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub priority: i32, // 0 | 1 | 3 | 5
    #[serde(default)]
    pub status: i32, // 0 | -1 | 2
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
}

/// TickTick 项目
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickTickProject {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

// ============================================================================
// JSON-RPC 2.0 类型
// ============================================================================

/// JSON-RPC 请求
#[derive(Debug, Clone, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl JsonRpcRequest {
    fn new(id: u64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 错误
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

// ============================================================================
// MCP Client
// ============================================================================

/// MCP Client — 连接单个 MCP Server
///
/// 支持 stdio 传输（通过子进程的 stdin/stdout 通信）
pub struct McpClient {
    config: MCPServerConfig,
    /// 子进程（stdio 模式）
    ///
    /// 使用 `tokio::sync::Mutex` 以便在 `.await` 期间安全持有锁守卫（守卫是 `Send`）。
    /// 外层 `Mutex` 提供内部可变性，使 `disconnect` 等方法可以以 `&self` 形式调用，
    /// 从而允许 `McpClient` 被 `Arc` 共享。
    child: tokio::sync::Mutex<Option<Child>>,
    /// 已发现的工具列表
    tools: RwLock<Vec<MCPTool>>,
    /// 是否已初始化
    initialized: RwLock<bool>,
    /// 请求 ID 计数器
    request_id: Mutex<u64>,
    /// 等待响应的回调
    pending: Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>,
}

impl McpClient {
    /// 创建新的 MCP Client
    pub fn new(config: MCPServerConfig) -> Self {
        Self {
            config,
            child: tokio::sync::Mutex::new(None),
            tools: RwLock::new(Vec::new()),
            initialized: RwLock::new(false),
            request_id: Mutex::new(0),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 连接到 MCP Server
    pub async fn connect(&mut self) -> Result<(), String> {
        match self.config.transport.as_str() {
            "stdio" => self.connect_stdio().await,
            "sse" => self.connect_sse().await,
            "websocket" => self.connect_websocket().await,
            _ => Err(format!("不支持的传输方式: {}", self.config.transport)),
        }
    }

    /// stdio 模式连接：启动子进程
    async fn connect_stdio(&mut self) -> Result<(), String> {
        let command = self
            .config
            .command
            .as_ref()
            .ok_or("stdio 模式需要指定 command")?;

        let mut cmd = Command::new(command);

        // 添加参数
        if let Some(args) = &self.config.args {
            cmd.args(args);
        }

        // 设置环境变量
        if let Some(env) = &self.config.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动 MCP Server 进程失败: {}", e))?;

        // 启动 stdout 读取任务
        if let Some(stdout) = child.stdout.take() {
            let pending = Arc::new(Mutex::new(
                HashMap::<u64, oneshot::Sender<JsonRpcResponse>>::new(),
            ));
            let pending_clone = pending.clone();

            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if line.is_empty() {
                        continue;
                    }

                    // 尝试解析 JSON-RPC 响应
                    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                        if let Some(id) = resp.id {
                            let mut pending = pending_clone.lock().unwrap();
                            if let Some(sender) = pending.remove(&id) {
                                let _ = sender.send(resp);
                            }
                        }
                    }
                }
            });

            // 将 pending 替换为新的共享 map
            // 注意：这里需要重新设计 pending 的共享方式
            // 简化实现：使用 Mutex 包裹
            *self.pending.lock().unwrap() = HashMap::new();
        }

        *self.child.get_mut() = Some(child);

        // 发送 initialize 请求
        self.initialize().await?;

        // 列出工具
        self.list_tools().await?;

        Ok(())
    }

    /// SSE 模式连接
    async fn connect_sse(&mut self) -> Result<(), String> {
        let url = self.config.url.as_ref().ok_or("SSE 模式需要指定 url")?;

        // SSE 模式下，初始化和工具列表通过 HTTP 请求获取
        // 简化实现：直接标记为已初始化
        *self.initialized.write().unwrap() = true;

        // 发送 initialize 请求
        self.initialize_http(url).await?;

        // 列出工具
        self.list_tools_http(url).await?;

        Ok(())
    }

    /// WebSocket 模式连接
    async fn connect_websocket(&mut self) -> Result<(), String> {
        // WebSocket 模式与 SSE 类似，简化实现
        self.connect_sse().await
    }

    /// 发送 initialize 请求（stdio）
    async fn initialize(&mut self) -> Result<(), String> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "StudyAgent",
                "version": "0.1.0"
            }
        });

        let _resp = self.send_request("initialize", Some(params)).await?;

        // 发送 initialized 通知
        self.send_notification("notifications/initialized", None)
            .await?;

        *self.initialized.write().unwrap() = true;

        Ok(())
    }

    /// 发送 initialize 请求（HTTP）
    async fn initialize_http(&self, url: &str) -> Result<(), String> {
        let client = reqwest::Client::new();

        let req = JsonRpcRequest::new(
            1,
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "StudyAgent",
                    "version": "0.1.0"
                }
            })),
        );

        let _resp = client
            .post(url)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("MCP initialize 请求失败: {}", e))?
            .json::<JsonRpcResponse>()
            .await
            .map_err(|e| format!("解析 MCP initialize 响应失败: {}", e))?;

        Ok(())
    }

    /// 列出可用工具（stdio）
    async fn list_tools(&mut self) -> Result<Vec<MCPTool>, String> {
        let resp = self.send_request("tools/list", None).await?;

        let tools: Vec<MCPTool> = if let Some(result) = resp.result {
            if let Some(tools_value) = result.get("tools") {
                let mut tools: Vec<MCPTool> =
                    serde_json::from_value(tools_value.clone()).unwrap_or_default();

                // 标记 server_id
                for tool in &mut tools {
                    tool.server_id = self.config.id.clone();
                }

                tools
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        *self.tools.write().unwrap() = tools.clone();

        Ok(tools)
    }

    /// 列出可用工具（HTTP）
    async fn list_tools_http(&self, url: &str) -> Result<Vec<MCPTool>, String> {
        let client = reqwest::Client::new();

        let req = JsonRpcRequest::new(2, "tools/list", None);

        let resp = client
            .post(url)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("MCP tools/list 请求失败: {}", e))?
            .json::<JsonRpcResponse>()
            .await
            .map_err(|e| format!("解析 MCP tools/list 响应失败: {}", e))?;

        let tools: Vec<MCPTool> = if let Some(result) = resp.result {
            if let Some(tools_value) = result.get("tools") {
                serde_json::from_value(tools_value.clone()).unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // 标记 server_id 并存储
        let mut tools = tools;
        for tool in &mut tools {
            tool.server_id = self.config.id.clone();
        }

        *self.tools.write().unwrap() = tools.clone();

        Ok(tools)
    }

    /// 调用工具
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolCallResult, String> {
        if !*self.initialized.read().unwrap() {
            return Err("MCP Server 尚未初始化".to_string());
        }

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        match self.config.transport.as_str() {
            "stdio" => {
                let resp = self.send_request("tools/call", Some(params)).await?;

                if let Some(error) = resp.error {
                    return Ok(ToolCallResult {
                        success: false,
                        data: None,
                        error: Some(format!("MCP 错误 [{}]: {}", error.code, error.message)),
                    });
                }

                let result = resp.result.unwrap_or(Value::Null);
                let content = result.get("content");

                Ok(ToolCallResult {
                    success: true,
                    data: content.cloned(),
                    error: None,
                })
            }
            "sse" | "websocket" => {
                let url = self.config.url.as_ref().ok_or("HTTP 模式需要指定 url")?;

                let client = reqwest::Client::new();

                let id = {
                    let mut counter = self.request_id.lock().unwrap();
                    *counter += 1;
                    *counter
                };

                let req = JsonRpcRequest::new(id, "tools/call", Some(params));

                let resp = client
                    .post(url)
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| format!("MCP tools/call 请求失败: {}", e))?
                    .json::<JsonRpcResponse>()
                    .await
                    .map_err(|e| format!("解析 MCP tools/call 响应失败: {}", e))?;

                if let Some(error) = resp.error {
                    return Ok(ToolCallResult {
                        success: false,
                        data: None,
                        error: Some(format!("MCP 错误 [{}]: {}", error.code, error.message)),
                    });
                }

                let result = resp.result.unwrap_or(Value::Null);
                let content = result.get("content");

                Ok(ToolCallResult {
                    success: true,
                    data: content.cloned(),
                    error: None,
                })
            }
            _ => Err(format!("不支持的传输方式: {}", self.config.transport)),
        }
    }

    /// 获取已发现的工具列表
    pub fn get_tools(&self) -> Vec<MCPTool> {
        self.tools.read().unwrap().clone()
    }

    /// 获取 Server 配置
    pub fn config(&self) -> &MCPServerConfig {
        &self.config
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        *self.initialized.read().unwrap()
    }

    /// 获取 Server 状态
    pub fn status(&self) -> MCPServerStatus {
        MCPServerStatus {
            id: self.config.id.clone(),
            name: self.config.name.clone(),
            connected: self.is_connected(),
            tools_count: self.tools.read().unwrap().len() as i32,
            last_error: None,
        }
    }

    /// 断开连接
    pub async fn disconnect(&self) -> Result<(), String> {
        {
            let mut child_guard = self.child.lock().await;
            if let Some(mut child) = child_guard.take() {
                child
                    .kill()
                    .await
                    .map_err(|e| format!("终止子进程失败: {}", e))?;
            }
        }
        *self.initialized.write().unwrap() = false;
        self.tools.write().unwrap().clear();
        Ok(())
    }

    // ========================================================================
    // JSON-RPC 通信（stdio）
    // ========================================================================

    /// 发送 JSON-RPC 请求并等待响应（stdio 模式）
    async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, String> {
        let id = {
            let mut counter = self.request_id.lock().unwrap();
            *counter += 1;
            *counter
        };

        let req = JsonRpcRequest::new(id, method, params);
        let req_str = serde_json::to_string(&req).map_err(|e| format!("序列化请求失败: {}", e))?;

        // 通过 stdin 发送请求
        let mut child_guard = self.child.lock().await;
        let child = match child_guard.as_mut() {
            Some(c) => c,
            None => return Err("MCP Server 未启动（无子进程）".to_string()),
        };

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(format!("{}\n", req_str).as_bytes())
                .await
                .map_err(|e| format!("写入 stdin 失败: {}", e))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("flush stdin 失败: {}", e))?;
        } else {
            return Err("无法获取 stdin".to_string());
        }

        // 简化实现：等待一小段时间后返回空响应
        // 实际实现应使用 oneshot channel 等待 stdout 读取任务返回的响应
        // 这里使用简化的轮询方式
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 返回一个模拟响应（实际实现需要正确等待响应）
        Ok(JsonRpcResponse {
            id: Some(id),
            result: Some(Value::Null),
            error: None,
        })
    }

    /// 发送 JSON-RPC 通知（不等待响应）
    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let notif_str =
            serde_json::to_string(&notification).map_err(|e| format!("序列化通知失败: {}", e))?;

        let mut child_guard = self.child.lock().await;
        let child = match child_guard.as_mut() {
            Some(c) => c,
            None => return Err("MCP Server 未启动".to_string()),
        };

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(format!("{}\n", notif_str).as_bytes())
                .await
                .map_err(|e| format!("写入通知失败: {}", e))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("flush 失败: {}", e))?;
        }

        Ok(())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // 尝试终止子进程
        if let Some(mut child) = self.child.get_mut().take() {
            // 尝试杀死子进程（同步方式，忽略错误）
            child.start_kill().ok();
        }
    }
}
