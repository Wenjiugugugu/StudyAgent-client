//! Tool Dispatcher — 统一工具调用入口，路由到对应 MCP server
//!
//! 设计要点：
//! - 维护多个 MCP Client（每个对应一个 MCP Server）
//! - 工具名 → Server 的映射（通过 MCP tools/list 自动建立）
//! - `dispatch(tool_name, args)` 统一入口

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::mcp::{McpClient, MCPServerConfig, MCPServerStatus, MCPTool, ToolCallResult};

/// Tool Dispatcher — 统一工具调用分发器
pub struct ToolDispatcher {
    /// 已连接的 MCP Client 列表
    ///
    /// 直接持有 `Arc<McpClient>`，McpClient 内部使用 `tokio::sync` 锁保证内部可变性，
    /// 这样在 `dispatch` 中调用 `call_tool().await` 时无需持有任何外部 `RwLock` 守卫，
    /// 从而使 future 满足 `Send`。
    clients: RwLock<HashMap<String, Arc<McpClient>>>,
    /// 工具名 → Server ID 的映射
    tool_to_server: RwLock<HashMap<String, String>>,
    /// 所有已发现的工具
    tools: RwLock<Vec<MCPTool>>,
    /// Server 配置列表
    configs: RwLock<Vec<MCPServerConfig>>,
}

impl ToolDispatcher {
    /// 创建空的 Tool Dispatcher
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
            tool_to_server: RwLock::new(HashMap::new()),
            tools: RwLock::new(Vec::new()),
            configs: RwLock::new(Vec::new()),
        }
    }

    /// 从配置列表创建 Tool Dispatcher 并连接所有 server
    pub async fn from_configs(configs: Vec<MCPServerConfig>) -> Self {
        let dispatcher = Self::new();

        for config in configs {
            if config.enabled {
                if let Err(e) = dispatcher.add_server(config).await {
                    log::warn!("连接 MCP Server 失败: {}", e);
                }
            }
        }

        dispatcher
    }

    /// 添加 MCP Server 并连接
    pub async fn add_server(&self, config: MCPServerConfig) -> Result<(), String> {
        let server_id = config.id.clone();
        let server_name = config.name.clone();

        let mut client = McpClient::new(config.clone());

        match client.connect().await {
            Ok(()) => {
                log::info!("MCP Server '{}' 已连接", server_name);

                // 获取工具列表并建立映射
                let tools = client.get_tools();

                // 更新工具映射
                {
                    let mut tool_map = self.tool_to_server.write().unwrap();
                    for tool in &tools {
                        tool_map.insert(tool.name.clone(), server_id.clone());
                    }
                }

                // 更新全局工具列表
                {
                    let mut global_tools = self.tools.write().unwrap();
                    global_tools.extend(tools);
                }

                // 存储 client
                self.clients
                    .write()
                    .unwrap()
                    .insert(server_id.clone(), Arc::new(client));

                // 存储配置
                self.configs.write().unwrap().push(config);
            }
            Err(e) => {
                log::warn!("MCP Server '{}' 连接失败: {}", server_name, e);

                // 即使连接失败也保存配置，以便后续重连
                self.configs.write().unwrap().push(config);
            }
        }

        Ok(())
    }

    /// 移除 MCP Server
    pub async fn remove_server(&self, server_id: &str) -> Result<(), String> {
        // 断开连接
        if let Some(client_arc) = self.clients.write().unwrap().remove(server_id) {
            client_arc.disconnect().await?;
        }

        // 清理工具映射
        {
            let mut tool_map = self.tool_to_server.write().unwrap();
            tool_map.retain(|_, sid| sid != server_id);
        }

        // 清理工具列表
        {
            let mut tools = self.tools.write().unwrap();
            tools.retain(|t| t.server_id != server_id);
        }

        // 清理配置
        {
            let mut configs = self.configs.write().unwrap();
            configs.retain(|c| c.id != server_id);
        }

        Ok(())
    }

    /// 统一工具调用入口
    ///
    /// 根据工具名路由到对应的 MCP Server 执行
    pub async fn dispatch(
        &self,
        tool_name: &str,
        args: Value,
    ) -> Result<ToolCallResult, String> {
        // 查找工具对应的 server
        let server_id = {
            let tool_map = self.tool_to_server.read().unwrap();
            tool_map
                .get(tool_name)
                .cloned()
                .ok_or_else(|| format!("未找到工具: {}", tool_name))?
        };

        // 获取对应的 client
        let client_arc = {
            let clients = self.clients.read().unwrap();
            clients
                .get(&server_id)
                .cloned()
                .ok_or_else(|| format!("MCP Server 不存在: {}", server_id))?
        };

        // 调用工具
        //
        // `client_arc` 是 `Arc<McpClient>`，`call_tool` 仅需 `&self`，
        // 调用期间不持有任何 `std::sync` 锁守卫，因此 future 保持 `Send`。
        client_arc.call_tool(tool_name, args).await
    }

    /// 列出所有可用工具
    pub fn list_tools(&self) -> Vec<MCPTool> {
        self.tools.read().unwrap().clone()
    }

    /// 获取所有 MCP Server 状态
    pub fn list_servers(&self) -> Vec<MCPServerStatus> {
        let clients = self.clients.read().unwrap();

        let mut statuses: Vec<MCPServerStatus> = clients
            .values()
            .map(|client_arc| client_arc.status())
            .collect();

        // 添加配置了但未连接的 server
        let connected_ids: Vec<String> = statuses.iter().map(|s| s.id.clone()).collect();
        let configs = self.configs.read().unwrap();

        for config in configs.iter() {
            if !connected_ids.contains(&config.id) {
                statuses.push(MCPServerStatus {
                    id: config.id.clone(),
                    name: config.name.clone(),
                    connected: false,
                    tools_count: 0,
                    last_error: Some("未连接".to_string()),
                });
            }
        }

        statuses
    }

    /// 获取指定工具的定义
    pub fn get_tool(&self, tool_name: &str) -> Option<MCPTool> {
        let tools = self.tools.read().unwrap();
        tools.iter().find(|t| t.name == tool_name).cloned()
    }

    /// 检查是否有可用的 MCP Server
    pub fn has_servers(&self) -> bool {
        !self.clients.read().unwrap().is_empty()
    }

    /// 重新连接指定 Server
    pub async fn reconnect_server(&self, server_id: &str) -> Result<(), String> {
        let config = {
            let configs = self.configs.read().unwrap();
            configs
                .iter()
                .find(|c| c.id == server_id)
                .cloned()
                .ok_or_else(|| format!("MCP Server 配置不存在: {}", server_id))?
        };

        // 先移除旧连接
        let _ = self.remove_server(server_id).await;

        // 重新连接
        self.add_server(config).await
    }

    /// 更新 Server 配置
    pub async fn update_server(&self, config: MCPServerConfig) -> Result<(), String> {
        let server_id = config.id.clone();

        // 移除旧配置
        let _ = self.remove_server(&server_id).await;

        // 添加新配置
        self.add_server(config).await
    }

    /// 获取所有 Server 配置
    pub fn get_configs(&self) -> Vec<MCPServerConfig> {
        self.configs.read().unwrap().clone()
    }
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 内置工具（不需要 MCP Server 的本地工具）
// ============================================================================

/// 内置工具名称前缀
pub const BUILTIN_TOOL_PREFIX: &str = "builtin.";

/// 检查是否是内置工具
pub fn is_builtin_tool(tool_name: &str) -> bool {
    tool_name.starts_with(BUILTIN_TOOL_PREFIX)
}

/// 执行内置工具
///
/// `data_dir` 用于需要访问项目文件的内置工具（如 read_file）。
pub fn execute_builtin_tool(
    tool_name: &str,
    args: &Value,
    data_dir: &std::path::Path,
) -> Result<ToolCallResult, String> {
    let tool = tool_name.strip_prefix(BUILTIN_TOOL_PREFIX).unwrap_or(tool_name);

    match tool {
        "echo" => {
            let result = serde_json::json!({
                "echo": args,
            });
            Ok(ToolCallResult {
                success: true,
                data: Some(result),
                error: None,
            })
        }
        "current_time" => {
            let now = crate::data::today_string();
            Ok(ToolCallResult {
                success: true,
                data: Some(Value::String(now)),
                error: None,
            })
        }
        "read_state" => {
            // 内置工具：读取学习状态摘要
            Ok(ToolCallResult {
                success: true,
                data: Some(Value::String("请使用 get_state 命令获取完整状态".to_string())),
                error: None,
            })
        }
        "read_file" => {
            // 内置工具：读取项目范围内的本地文件（只读）
            let rel_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("参数 path 缺失或不是字符串")?;

            let target = resolve_project_path(data_dir, rel_path)?;
            let content = crate::data::read_file_content(&target)?;
            Ok(ToolCallResult {
                success: true,
                data: Some(serde_json::json!({
                    "path": target.to_string_lossy().to_string(),
                    "content": content,
                })),
                error: None,
            })
        }
        _ => Err(format!("未知内置工具: {}", tool)),
    }
}

/// 将相对路径解析为项目内的绝对路径，并确保不会越界到项目目录之外
fn resolve_project_path(data_dir: &std::path::Path, rel_path: &str) -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;

    let cleaned = rel_path.replace('\\', "/");
    let normalized = cleaned
        .split('/')
        .fold(PathBuf::new(), |mut acc, part| {
            match part {
                "" | "." => {}
                ".." => {
                    acc.pop();
                }
                _ => acc.push(part),
            }
            acc
        });

    let target = data_dir.join(normalized);
    let canonical_data_dir = std::fs::canonicalize(data_dir)
        .unwrap_or_else(|_| data_dir.to_path_buf());
    let canonical_target = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());

    if !canonical_target.starts_with(&canonical_data_dir) {
        return Err(format!(
            "路径越界: {:?} 不在项目目录 {:?} 内",
            canonical_target, canonical_data_dir
        ));
    }

    Ok(canonical_target)
}
