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
  | "zhipu"
  | "kimi"
  | "longcat"
  | "minimax"
  | "mimo"
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
  /** 流式合并依据（OpenAI 规范） */
  index?: number;
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
  agent?: "planner" | "teacher" | "reviewer" | "assistant" | "doubt";
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
  /** 推理模型的思考过程（DeepSeek-R1/V4 等的 reasoning_content），非推理模型不存在 */
  reasoning?: string;
}

/** 流式响应块 */
export interface ChatStreamChunk {
  content: string;
  done: boolean;
  tool_calls?: ToolCall[];
  /** fallback 切换 provider 时置为 true，调用方应清空已显示内容 */
  reset?: boolean;
  /** M17：流式 usage（最后一 chunk 携带，可选） */
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
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

/** 余额查询结果（对应后端 `ai::balance::BalanceResult`，参考 cc-Switch 用量查询 extractor） */
export interface ProviderBalanceResult {
  success: boolean;
  /** 命中的查询模板：openrouter / siliconflow / deepseek / moonshot / credit_grants / general_balance */
  mode: string;
  /** 剩余额度 */
  remaining: number | null;
  /** 已使用额度 */
  used: number | null;
  /** 总额度 */
  total: number | null;
  /** 货币/单位（USD / CNY / % / 次 等，未知为空字符串） */
  unit: string;
  /** 套餐/条目名（如智谱套餐等级、MiniMax 模型名），无则空 */
  plan_name: string;
  /** 展示用消息（失败原因；成功时为空） */
  message: string;
}

/** 单条 AI 用量记录（对应后端 `data::ai_usage::AiUsageEntry`） */
export interface AiUsageEntry {
  /** 调用时间戳（ISO 字符串 YYYY-MM-DDTHH:mm） */
  timestamp: string;
  /** Agent 类型标签（planner / reviewer / assistant / teacher / unknown） */
  agent: string;
  /** 实际使用的模型名（来自 ChatResponse.model） */
  model: string;
  /** 输入 token 数 */
  prompt_tokens: number;
  /** 输出 token 数 */
  completion_tokens: number;
  /** 总 token 数 */
  total_tokens: number;
  /** 调用耗时（毫秒） */
  duration_ms: number;
  /** 状态：success / error */
  status: string;
  /** 错误信息（仅失败时） */
  error?: string | null;
}
