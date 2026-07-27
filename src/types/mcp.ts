/**
 * StudyAgent Core — MCP Tool Layer Types
 * 统一 Tool Layer 设计 (Tool Dispatcher → MCP Servers)
 */

/** MCP Server 类型 */
export type MCPServerType =
  | "ticktick"
  | "filesystem"
  | "browser"
  | "obsidian"
  | "custom";

/** MCP Server 配置 */
export interface MCPServerConfig {
  id: string;
  name: string;
  type: MCPServerType;
  enabled: boolean;
  /** 传输方式 */
  transport: "stdio" | "sse" | "websocket";
  /** stdio 模式下的命令 */
  command?: string;
  args?: string[];
  /** SSE/WebSocket 模式下的 URL */
  url?: string;
  /** 环境变量 */
  env?: Record<string, string>;
}

/** 工具定义（MCP 层） */
export interface MCPTool {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  server_id: string;
}

/** 工具调用请求 */
export interface ToolCallRequest {
  tool_name: string;
  arguments: Record<string, unknown>;
}

/** 工具调用结果 */
export interface ToolCallResult {
  success: boolean;
  data?: unknown;
  error?: string;
}

/** MCP Server 状态 */
export interface MCPServerStatus {
  id: string;
  name: string;
  connected: boolean;
  tools_count: number;
  last_error?: string;
}

/** TickTick 任务（滴答清单） */
export interface TickTickTask {
  id: string;
  title: string;
  content?: string;
  priority: 0 | 1 | 3 | 5;
  status: 0 | -1 | 2;
  due_date?: string;
  project_id?: string;
  tags?: string[];
  start_date?: string;
  end_date?: string;
}

/** TickTick 项目 */
export interface TickTickProject {
  id: string;
  name: string;
  color?: string;
}
