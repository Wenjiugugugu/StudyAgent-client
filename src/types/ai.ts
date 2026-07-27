/**
 * StudyAgent Core — AI Provider Types
 * 统一 AI Provider 接口设计 (OpenAI Compatible)
 */

/** 支持的 AI Provider 类型 */
export type ProviderType =
  | "openai"
  | "gemini"
  | "anthropic"
  | "ollama"
  | "openrouter"
  | "siliconflow"
  | "dashscope"
  | "volcengine"
  | "custom";

/** AI Provider 配置 */
export interface AIProviderConfig {
  id: string;
  name: string;
  type: ProviderType;
  base_url: string;
  api_key: string;
  model: string;
  /** 可选：备用模型 */
  fallback_model?: string;
  /** 请求超时（秒） */
  timeout?: number;
  /** 温度参数 */
  temperature?: number;
  /** 最大 token 数 */
  max_tokens?: number;
  /** 是否启用 */
  enabled: boolean;
  /** 是否为默认 provider */
  is_default: boolean;
}

/** 聊天消息角色 */
export type MessageRole = "system" | "user" | "assistant" | "tool";

/** 聊天消息 */
export interface ChatMessage {
  role: MessageRole;
  content: string;
  name?: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string;
}

/** 工具调用 */
export interface ToolCall {
  id: string;
  type: "function";
  function: {
    name: string;
    arguments: string;
  };
}

/** 工具定义 */
export interface ToolDefinition {
  type: "function";
  function: {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
  };
}

/** 聊天请求 */
export interface ChatRequest {
  messages: ChatMessage[];
  model?: string;
  temperature?: number;
  max_tokens?: number;
  stream?: boolean;
  tools?: ToolDefinition[];
  /** Agent 类型标识，Core 据此选择 system prompt */
  agent?: "planner" | "teacher" | "reviewer" | "assistant";
  /** 附加上下文（当前页面信息） */
  context?: AgentContext;
}

/** Agent 上下文（来自当前页面） */
export interface AgentContext {
  current_view?: string;
  current_task_id?: string;
  current_knowledge_id?: string;
  current_review_date?: string;
  additional?: Record<string, unknown>;
}

/** 聊天响应 */
export interface ChatResponse {
  id: string;
  model: string;
  content: string;
  role: MessageRole;
  tool_calls?: ToolCall[];
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
  finish_reason: "stop" | "length" | "tool_calls";
}

/** 流式响应块 */
export interface ChatStreamChunk {
  content: string;
  done: boolean;
  tool_calls?: ToolCall[];
}

/** AI Provider 能力 */
export interface ProviderCapabilities {
  streaming: boolean;
  function_calling: boolean;
  vision: boolean;
  max_context_length: number;
}

/** 可用模型信息 */
export interface ModelInfo {
  id: string;
  owned_by: string;
  created: number;
  /** 从 API 返回的额外元数据（如 max_tokens 等） */
  extra: Record<string, unknown>;
}
