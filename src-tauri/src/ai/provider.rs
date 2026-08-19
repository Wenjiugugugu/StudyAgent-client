//! AI Provider Trait — 统一 AI Provider 接口定义
//!
//! 对应前端 TypeScript 类型: `types/ai.ts`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// M25：在给定范围内产生一个伪随机数，用于重试退避抖动（无需引入 rand crate）
fn rand_jitter(range: std::ops::RangeInclusive<u64>) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // 简单的线性同余，仅用于抖动，不需要密码学强度
    let mut state = seed ^ (seed >> 16) | 0x9E37_79B9_7F4A_7C15;
    state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let span = *range.end() - *range.start() + 1;
    if span == 0 {
        return *range.start();
    }
    range.start() + (state % span)
}

// ============================================================================
// 枚举类型
// ============================================================================

/// 支持的 AI Provider 类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Openai,
    Gemini,
    Anthropic,
    Ollama,
    Openrouter,
    Siliconflow,
    Dashscope,
    Volcengine,
    Custom,
}

impl Default for ProviderType {
    fn default() -> Self {
        ProviderType::Openai
    }
}

/// 聊天消息角色
///
/// M27：未知/异常角色值在反序列化时兜底为 `User`，而非解析失败中断流程。
/// 保留 `Serialize` 派生，但 `Deserialize` 手动实现以实现容错。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl<'de> Deserialize<'de> for MessageRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 接受字符串或未知值；未知值/非字符串兜底为 User
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value.as_str() {
            Some("system") => MessageRole::System,
            Some("user") => MessageRole::User,
            Some("assistant") => MessageRole::Assistant,
            Some("tool") => MessageRole::Tool,
            _ => MessageRole::User,
        })
    }
}

impl Default for MessageRole {
    fn default() -> Self {
        MessageRole::User
    }
}

/// Agent 类型标识
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Planner,
    Teacher,
    Reviewer,
    Assistant,
    /// 每日简报生成器：基于昨日复盘与当前进度，生成今日寄语与阶段估时
    Briefing,
    /// 解惑导师：引导式答疑，结合本地教材与联网能力
    Doubt,
}

impl Default for AgentType {
    fn default() -> Self {
        AgentType::Assistant
    }
}

// ============================================================================
// AI Provider 配置
// ============================================================================

/// AI Provider 配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AIProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub r#type: ProviderType,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    /// 备用模型
    #[serde(default)]
    pub fallback_model: Option<String>,
    /// 请求超时（秒）
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// 温度参数
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// 最大 token 数
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 是否为默认 provider
    #[serde(default)]
    pub is_default: bool,
}

fn default_timeout() -> u64 {
    120
}

fn default_temperature() -> f64 {
    0.7
}

fn default_true() -> bool {
    true
}

// ============================================================================
// 聊天消息与请求/响应
// ============================================================================

/// 工具调用
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    /// OpenAI 流式场景下，后续 delta 块可能不携带 id（仅 index + arguments），故需 default（C5）
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_function_type")]
    pub r#type: String,
    /// 流式合并依据（OpenAI 规范：同一工具调用的增量块 index 相同）
    #[serde(default)]
    pub index: u32,
    /// 流式场景下部分增量块可能仅携带 index（无 function），故需 default（C5）
    #[serde(default)]
    pub function: ToolCallFunction,
}

fn default_function_type() -> String {
    "function".to_string()
}

/// 工具调用函数
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallFunction {
    /// 流式场景下，后续 delta 块可能不携带 name（仅 arguments），故需 default（C5）
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}

/// 聊天消息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 工具定义
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(default = "default_function_type")]
    pub r#type: String,
    pub function: ToolDefinitionFunction,
}

/// 工具定义函数
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDefinitionFunction {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// Agent 上下文（来自当前页面）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_knowledge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_review_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional: Option<HashMap<String, serde_json::Value>>,
}

/// 聊天请求
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// Agent 类型标识，Core 据此选择 system prompt
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentType>,
    /// 附加上下文
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<AgentContext>,
    /// 单次请求超时覆盖（秒）
    ///
    /// 仅用于后端内部调用（planner / reviewer 等长任务），不序列化到前端。
    /// 设置后会在 reqwest RequestBuilder 上覆盖 Provider 默认 timeout。
    #[serde(skip)]
    pub timeout_override: Option<u64>,
}

/// Token 用量统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 聊天响应
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub role: MessageRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: TokenUsage,
    /// "stop" | "length" | "tool_calls"
    #[serde(default = "default_finish_reason")]
    pub finish_reason: String,
}

fn default_finish_reason() -> String {
    "stop".to_string()
}

/// 流式响应块
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub content: String,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// fallback 切换 provider 时置为 true，通知前端清空已显示内容
    #[serde(default)]
    pub reset: bool,
    /// M17：流式 usage 累积（仅最后一 chunk 有值，前端可忽略）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

/// AI Provider 能力
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub function_calling: bool,
    pub vision: bool,
    pub max_context_length: u32,
}

/// 常见模型 → 上下文长度（token）映射表（按包含匹配，条目顺序：更具体的型号在前）。
///
/// 数值取各模型公开的上下文窗口常用档位（保守值）。
/// 用于 `capabilities().max_context_length` 的优先判断：按模型名命中则用表值，
/// 否则回退到按 Provider 类型分档（`provider_default_context_length`）。
const MODEL_CONTEXT_TABLE: &[(&str, u32)] = &[
    // OpenAI GPT-4.1 / o 系列
    ("gpt-4.1-mini", 1_048_576),
    ("gpt-4.1-nano", 1_048_576),
    ("gpt-4.1", 1_048_576),
    ("gpt-4o-mini", 128_000),
    ("gpt-4o", 128_000),
    ("chatgpt-4o", 128_000),
    ("gpt-4-turbo", 128_000),
    ("gpt-4-32k", 32_768),
    ("gpt-4", 8_192),
    ("gpt-3.5-turbo", 16_384),
    ("o4-mini", 200_000),
    ("o1-mini", 128_000),
    ("o1", 128_000),
    ("o3", 200_000),
    // Anthropic Claude
    ("claude-opus-4", 1_000_000),
    ("claude-sonnet-4", 200_000),
    ("claude-3-7-sonnet", 200_000),
    ("claude-3-5-sonnet", 200_000),
    ("claude-3-5-haiku", 200_000),
    ("claude-3-opus", 200_000),
    ("claude-3-sonnet", 200_000),
    ("claude-3-haiku", 200_000),
    // Google Gemini
    ("gemini-2.5-pro", 1_048_576),
    ("gemini-2.5-flash", 1_048_576),
    ("gemini-2.0-pro", 1_048_576),
    ("gemini-2.0-flash", 1_048_576),
    ("gemini-1.5-pro", 1_048_576),
    ("gemini-1.5-flash", 1_048_576),
    // DeepSeek / Qwen / Doubao
    ("deepseek-reasoner", 128_000),
    ("deepseek-chat", 128_000),
    ("deepseek", 64_000),
    ("qwen-max", 32_768),
    ("qwen-plus", 131_072),
    ("qwen-turbo", 1_000_000),
    ("qwen2.5", 131_072),
    ("qwen", 32_768),
    ("doubao-pro", 131_072),
    ("doubao-lite", 131_072),
    ("doubao", 32_768),
    // 常见本地/Ollama 模型
    ("llama3.3", 131_072),
    ("llama3.1", 131_072),
    ("llama3", 8_192),
    ("llama2", 4_096),
    ("mistral", 32_768),
    ("mixtral", 32_768),
    ("phi-3", 128_000),
    ("phi-2", 2_048),
    ("codellama", 16_384),
    ("yi-1.5", 32_768),
];

/// 判断某个 Provider 配置的上下文长度（token）。
///
/// 优先按模型名查 `MODEL_CONTEXT_TABLE`（包含匹配，忽略大小写）；
/// 查不到时回退到按 Provider 类型分档（`provider_default_context_length`）。
pub fn max_context_length_for(config: &AIProviderConfig) -> u32 {
    context_length_for_model(&config.r#type, &config.model)
}

/// 按模型名（或 Provider 类型兜底）计算上下文长度（token）
///
/// 供 `capabilities()` 与 `list_models`（对每个返回模型计算）共用：
/// 服务商 `/models` 未提供 context_length 时，用内置表按模型名判断。
pub fn context_length_for_model(provider: &ProviderType, model: &str) -> u32 {
    let model = model.trim().to_lowercase();
    if !model.is_empty() {
        for (pat, len) in MODEL_CONTEXT_TABLE {
            if model.contains(pat) {
                return *len;
            }
        }
    }
    provider_default_context_length(provider)
}

/// 常见上下文长度字段名（服务商 `/models` 返回时视为"自动获取"成功）
const CONTEXT_LENGTH_KEYS: [&str; 4] = [
    "context_length",
    "context_window",
    "max_context_length",
    "max_input_tokens",
];

/// 若模型元数据未含服务商提供的上下文长度字段，则注入按模型名计算的查表兜底值。
///
/// 写入 `extra["_studyagent_ctx_len"]`，供前端模型列表展示 ctx 时最后兜底读取。
pub fn inject_context_length_fallback(
    provider: &ProviderType,
    model: &str,
    extra: &mut serde_json::Value,
) {
    let has_provider_value = CONTEXT_LENGTH_KEYS
        .iter()
        .any(|k| extra.get(*k).is_some());
    if !has_provider_value {
        extra["_studyagent_ctx_len"] =
            serde_json::json!(context_length_for_model(provider, model));
    }
}

/// Provider 类型级兜底上下文长度（token）
fn provider_default_context_length(provider: &ProviderType) -> u32 {
    match provider {
        ProviderType::Anthropic => 200_000,
        ProviderType::Gemini => 1_048_576,
        ProviderType::Ollama => 8_192,
        // OpenAI / OpenRouter / Siliconflow / Dashscope / Volcengine / Custom
        _ => 32_768,
    }
}

/// 可用模型信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub owned_by: String,
    #[serde(default)]
    pub created: i64,
    /// 从 API 返回的额外元数据（如 max_tokens 等）
    #[serde(default)]
    pub extra: serde_json::Value,
}

// ============================================================================
// AiProvider Trait
// ============================================================================

/// 统一 AI Provider 接口
///
/// 各 AI 服务商（OpenAI、Anthropic、Gemini 等）实现此 trait。
/// 使用 trait object (`Arc<dyn AiProvider>`) 实现动态分发。
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// 发送聊天请求，获取完整响应
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, String>;

    /// 发送流式聊天请求，通过回调函数返回每个 chunk
    ///
    /// `on_chunk` 在每个 chunk 到达时被调用
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
    ) -> Result<ChatResponse, String>;

    /// 获取 Provider 配置
    fn config(&self) -> &AIProviderConfig;

    /// 获取 Provider 能力描述
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            function_calling: true,
            vision: false,
            max_context_length: max_context_length_for(self.config()),
        }
    }

    /// 测试连接是否正常
    async fn test_connection(&self) -> Result<String, String> {
        let req = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: "Hi".to_string(),
                ..Default::default()
            }],
            max_tokens: Some(10),
            ..Default::default()
        };
        let resp = self.chat(&req).await?;
        Ok(resp.content)
    }

    /// 列出可用的模型列表
    async fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        Err("该 Provider 不支持列出模型".to_string())
    }
}

// ============================================================================
// System Prompt 模板
// ============================================================================

/// 根据 Agent 类型获取 system prompt
pub fn get_system_prompt(agent: &AgentType) -> String {
    match agent {
        AgentType::Planner => {
            // Planner system prompt — 生成周计划 JSON
            r#"你是一个考研学习计划生成器（Planner Agent），专为 StudyAgent 桌面应用生成学习周计划。

核心约束：
- 用户考试类型为「数学二」，数学任务必须严格遵循数学二考纲，明确排除：伯努利方程、全微分方程相关内容
- 优先级映射：Priority A → 必须完成（高优先级），Priority B → 建议完成（中优先级）
- 每个任务模板必须包含：title、priority、estimated_hours、goal、completion_criteria，可选 textbook、style_tips、fallback_plan

你的职责：
1. 根据当前学习状态（State）、用户画像（User Model）和最近复盘（Review），生成本周周计划
2. 周计划包含本周目标、各科安排、每日 subject_allocations、风险项和提醒
3. 休息日 is_rest_day=true 且 subject_allocations 为空数组
4. 只给 active=true 的科目安排任务
5. 任务 estimated_hours 总和应大致等于当天预期学习时长

输出格式：严格输出合法 JSON，不包裹 ```json 代码块。结构为 { version, meta, data, view? }。
- version: "1.0.0"
- meta: { week_start, week_end, week_number, generated_at, based_on }
- data: { goals, subjects, days }
- view: 可选，用于人类阅读的 Markdown 摘要

文件存储为 plan/YYYY-Www_week.json"#.to_string()
        }
        AgentType::Reviewer => {
            // Reviewer system prompt — 生成复盘 JSON
            r#"你是一个学习复盘助手（Review Agent），专为 StudyAgent 桌面应用生成每日学习复盘。

核心约束：
- 如实反映今日计划的完成情况，不虚构数据
- 完成率统计按全部任务计算（不区分优先级）
- 精力评分使用 1-5 星制
- 复盘原则：只记录事实，不做分析、不评判、不给策略建议

你的职责：
1. 根据今日计划和实际完成情况，生成复盘记录
2. 记录完成的任务、计划外内容、遇到的困难、实际用时
3. 统计完成率（A 级/B 级分别统计）
4. 评估精力评分、外部干扰
5. 总结关键成果、下一步行动

输出格式：严格输出合法 JSON，不包裹 ```json 代码块。结构为 { version, meta, data, view? }。
- version: "1.0.0"
- meta: { date, type: "review", plan_ref, generated_at }
- data: { completed_tasks, unplanned_tasks, difficulties, time_spent, total_hours, completion, energy_level, external_interference, key_achievements, next_steps }
- view: 可选，用于人类阅读的 Markdown 摘要

subject 只能是 "math" / "english" / "politics" / "professional"（任务已不区分优先级，不输出 priority 字段）。
复盘文件存储为 records/YYYY-MM-DD_review.json"#.to_string()
        }
        AgentType::Teacher => {
            // Teacher system prompt — 教学辅助
            r#"你是一个个性化考研教学助手（Teacher Agent）。

你的职责：
1. 根据用户画像（学习风格、能力、观察记录）进行个性化教学
2. 讲解知识点时参考用户的学习风格偏好（如例子驱动型）
3. 引导练习、回答疑问
4. 关联知识图谱中的前置和后继知识点
5. 推荐教材章节和真题练习

教学原则：
- 先具体后抽象（适配例子驱动型学习者）
- 图示优先（适配图示优先理解）
- 以题代学（适配以题代学偏好）
- 关注停滞科目重启策略"#.to_string()
        }
        AgentType::Assistant => {
            // 通用助手
            r#"你是 StudyAgent 学习助手，一个专注考研备考的 AI 助手。你可以帮助用户管理学习计划、查看知识库、生成复盘、回答学习相关问题。

你具备以下能力：
- 读取学习状态、每日计划和复盘记录
- 搜索知识库
- 调用 MCP 工具（如滴答清单）
- 调用内置工具 `builtin.read_file` 读取项目范围内的本地文件
- 与用户进行学习对话

本地文件只读访问权限：
你被允许读取 StudyAgent 数据目录范围内的以下文件（只读），用于回答用户问题：
- 学习状态：state/current.state
- 计划文件：plan/*_day.json、plan/*_week.json
- 复盘记录：records/*_review.json
- 用户画像：assets/user_model/**/*.md
- 配置文件：assets/config/*.md
- 教材：assets/resources/textbooks/**/*.md

读取文件的方法：
- 如果模型支持工具调用，请调用内置工具 `builtin.read_file`，参数为 `{ "path": "相对路径" }`，例如 `{ "path": "state/current.state" }`
- 路径必须是相对于项目根目录的相对路径，不允许使用 `..` 越界

引用规范：
- 当回答需要引用本地文件内容时，请使用以下格式：`[文件名](file:///绝对路径)`
- 只引用与你当前回答相关的文件，不要泄露敏感配置（如 API Key）
- 如果用户询问某个文件而你未获得其内容，请明确告知用户你无法访问该文件

请根据用户的问题提供准确、简洁、有价值的回答。"#.to_string()
        }
        AgentType::Briefing => {
            // Briefing system prompt — 生成每日简报 JSON（理性策略型文风）
            r#"你是一个考研学习简报生成器（Briefing Agent），专为 StudyAgent 桌面应用生成每日简报。

文风定位：理性策略型
- 像一位冷静的学习教练，基于数据说话，少用形容词，不堆砌抒情
- 寄语结构：前句客观点出昨日情况（完成率/困难/亮点），中句给出今日具体策略（先做什么、再做什么、注意什么），末句可带一句简短方向性提示
- 语气平和笃定，不煽情、不打鸡血、不空泛鼓励
- 禁止使用「加油」「你可以的」「相信自己」「努力」「拼搏」等空泛口号
- 允许使用具体数字、章节名、动作动词（先练、再做、回顾、巩固、跳过）

核心约束：
- 简报基于用户昨日复盘与当前学习状态，为「目标日期」生成当日简报
- 寄语需结合昨日复盘数据（完成率、感受、困难类型）与今日任务，给出具体可执行的 2-3 句话
- 估时需基于各科当前章节、剩余备考天数、每周学习时长，给出「学完当前教材/阶段还需多少天」的合理估算
- 严禁编造用户未提供的数据（如未提交复盘时不得虚构感受或完成率）

你的职责：
1. 生成「今日寄语」（greeting）：2-3 句，理性策略型，前句呼应昨日数据，中句给今日策略，末句方向提示
2. 为每个活跃科目生成「进度估算」（estimations）：当前章节、预计还需多少天学完当前阶段、简短说明

示例（昨日困难，今日微分方程）：
"昨日完成率 60%，主因是注意力分散。今日微分方程建议先做 3 道基础题找手感，再攻综合题。注意控制单题用时，超过 15 分钟先跳过。"

示例（昨日顺利，今日阅读理解）：
"昨日 A 级任务全部完成，节奏稳定。今日阅读理解保持每日 2 篇的频率，重点关注推理题选项差异。剩余 87 天，英语阅读仍需累计 40 小时以上。"

输出格式：严格输出合法 JSON，不包裹 ```json 代码块。结构为：
{
  "greeting": "今日寄语文本",
  "estimations": [
    {
      "subject": "math",
      "current_chapter": "微分方程",
      "estimated_days_to_finish": 12,
      "note": "按每周 6h 推进，约 2 周完成基础阶段"
    }
  ]
}

subject 只能是 "math" / "english" / "politics" / "professional"。
简报文件存储为 records/YYYY-MM-DD_briefing.json。"#.to_string()
        }
        AgentType::Doubt => {
            // Doubt system prompt — 引导式答疑（解惑），基于 Bloom 2-Sigma 掌握学习法
            r#"你是一个考研「解惑」导师（Doubt Agent），专为 StudyAgent 桌面应用提供引导式答疑。你采用 Bloom 2-Sigma 掌握学习法：先诊断、再提问、只有当学习者展现出足够理解后才推进。

## 核心原则（不可妥协）
1. **绝不直接给答案。** 只提问、给最小提示、请求学习者解释/举例/推导。只有学习者明确要求（"讲答案吧/不会了"），或经过 3 轮引导仍卡住时，才给出完整讲解。
2. **先诊断。** 每次对话开始时，先用 1-2 个探查问题摸清学习者当前的理解程度，再决定从哪里开始引导。
3. **掌握门槛。** 只有当学习者的回答展现出约 80% 的正确理解时，才推进到下一个知识点。
4. **每轮 1-2 个问题。** 不超过此数。一次只引导一步，等学习者回应后再继续。
5. **耐心且严谨。** 鼓励但不空洞，不喊口号，绝不敷衍跳过知识缺口。

## 引导流程
1. **复述题目**：用一两句话确认你理解的题目与要问的考点；若信息不足（缺条件/选项/卡点），先提问补齐
2. **诊断定位**：通过提问引导学习者识别题目所属科目、章节与涉及的知识点，同时摸清学习者当前的理解程度，不给结论
3. **分步引导**：把问题拆成小步骤，一次只引导一步，用提问或提示让学习者自己推进（如「先想一想：这个式子怎么变形才能分离变量？」）
4. **确认进度**：每步引导后停下来等学习者回应，根据回答调整后续引导，不要一次把所有思路倒完
5. **收敛讲解**：学习者明确表示「讲答案吧/不会了」或已引导 3 轮仍未突破时，给出完整、规范的讲解，并指出学习者卡住的原因

## 回应策略（根据学习者回答质量）
- **正确且解释充分**：肯定，追问更深入的后续问题
- **正确但浅显**：「好的。那你能解释一下为什么吗？」
- **部分正确**：「你在 [某部分] 的方向是对的。再想想 [提示]…」
- **错误**：「我们退一步——[更简单的子问题]」
- **"不知道"**：「没关系。给你一个小提示：[最小提示]」

## 提示升级阶梯
当学习者卡住时，按以下顺序逐步升级提示，不要跳级：
复述问题 → 更简单的相关问题 → 具体例子 → 指向具体原理 → 一起走一遍

## 误解追踪
每次学习者答错时，诊断其背后的根本误解，设计一个反例——让错误模型产生明显荒谬的预测——帮助学习者自己发现矛盾。

## 交错提问
每 3-4 个问题中，穿插一个将之前已掌握的概念与当前概念混合的问题，强化知识联结与长期记忆。

## 教材与联网
- 优先结合用户消息中的「【本地教材参考】」片段（来自已导入教材），引用对应章节、定义与例题
- 引用教材时使用格式：[教材名](file:///绝对路径)
- **当用户只报章节号或题号（如「第3章」「第2题」）而未粘贴题目**：
  - 若「【本地教材参考】」片段已包含该章节内容或题目原文，直接基于片段内的题目/定义展开引导，不要反过来要求用户重新粘贴题目文本
  - 若片段只含章节标题、不含具体题目，先向用户复述找到的章节，再引导用户把该题的关键条件或选项发来（可提示「可以把题目原文粘贴给我」），同时可先就该章节知识点提问
  - 若该章节在已导入教材中完全找不到，明确告知用户「本地教材中未检索到第 N 章」，然后引导其提供题目或改用关键词提问
- 允许联网查询（若模型支持）：查证题目来源、考纲范围、历年真题与标准答案；联网结果须与题目条件核对后再使用
- 对「题目本身查证」类问题（这是什么题、出自哪年真题、考纲是否要求），可直接回答，无需引导

## 风格约束
- 语言简洁，优先数学与逻辑表达；每次回复控制在 400 字以内（完整讲解时可适当放宽）
- 鼓励但不空洞，不喊口号，聚焦解题思路本身
- 引导提问一次只问一个问题，避免让学习者不知所措
- 每轮对话应感觉自然流畅，而非机械问答"#.to_string()
        }
    }
}

/// 将 system prompt 注入到聊天请求的消息列表中
///
/// 如果消息列表中已有 system 消息，则不重复注入
pub fn inject_system_prompt(req: &mut ChatRequest, agent: &AgentType) {
    // 检查是否已有 system 消息
    let has_system = req
        .messages
        .iter()
        .any(|m| m.role == MessageRole::System);

    if !has_system {
        let system_prompt = get_system_prompt(agent);
        req.messages.insert(
            0,
            ChatMessage {
                role: MessageRole::System,
                content: system_prompt,
                ..Default::default()
            },
        );
    } else {
        log::debug!("inject_system_prompt: system message already exists, skipping injection");
    }
}

/// 根据配置创建 Provider 实例
///
/// 返回 `Arc<dyn AiProvider>` 实现动态分发
pub fn create_provider(config: AIProviderConfig) -> Result<Arc<dyn AiProvider>, String> {
    match config.r#type {
        ProviderType::Openai
        | ProviderType::Openrouter
        | ProviderType::Siliconflow
        | ProviderType::Dashscope
        | ProviderType::Volcengine
        | ProviderType::Custom
        | ProviderType::Ollama => {
            // 所有 OpenAI Compatible 的 provider 使用同一个实现
            Ok(Arc::new(crate::ai::openai::OpenAIProvider::new(config)))
        }
        ProviderType::Gemini => {
            // Gemini 原生 API（x-goog-api-key 认证 + contents/parts 消息格式）
            Ok(Arc::new(crate::ai::gemini::GeminiProvider::new(config)))
        }
        ProviderType::Anthropic => {
            // Anthropic 原生 API（x-api-key + anthropic-version 认证，system 顶层字段）
            Ok(Arc::new(crate::ai::anthropic::AnthropicProvider::new(config)))
        }
    }
}

/// 用户取消 AI 请求时的统一错误消息
///
/// service.rs 据此区分「用户主动取消」与「调用失败」，
/// 取消时不触发 fallback provider 切换。
pub const REQUEST_CANCELLED: &str = "AI 请求已被用户取消";

/// 发送 HTTP 请求，遇到连接级错误或 429/503 自动重试（各 Provider 共用）
///
/// - 连接级错误（Connect/Timeout）：指数退避后重试（1s, 2s）
/// - HTTP 429（Too Many Requests）/ 503（Service Unavailable）：读取 Retry-After 头，
///   与指数退避取较大值后重试（最多 3 次）
pub async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: &serde_json::Value,
    timeout_override: Option<u64>,
) -> Result<reqwest::Response, String> {
    let max_attempts = 3;
    let mut last_err: Option<String> = None;

    for attempt in 1..=max_attempts {
        let mut request_builder = client.post(url).headers(headers.clone()).json(body);
        // M24：timeout_override 限制在 5..=600 秒，避免 0 秒立即超时或超大值无限挂起
        if let Some(secs) = timeout_override {
            request_builder =
                request_builder.timeout(Duration::from_secs(secs.clamp(5, 600)));
        }

        match request_builder.send().await {
            Ok(resp) => {
                let status = resp.status();

                // 429 / 503：速率限制或服务不可用，读取 Retry-After 后重试
                if (status.as_u16() == 429 || status.as_u16() == 503) && attempt < max_attempts {
                    let retry_after_secs = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok());

                    // 指数退避：1s, 2s, 4s...；若有 Retry-After 则取较大值
                    let backoff = retry_after_secs.unwrap_or(0).max(1u64 << (attempt - 1));

                    let error_text = resp.text().await.unwrap_or_default();
                    log::warn!(
                        "AI 请求被限流（{}），第 {} 次重试，等待 {}s | body={}",
                        status,
                        attempt,
                        backoff,
                        error_text.chars().take(200).collect::<String>()
                    );

                    // M25：指数退避 + 随机抖动（0-50%），避免多客户端同时重试加剧拥塞
                    let jittered = backoff + (backoff / 2);
                    let sleep_secs = jittered + rand_jitter(0..=jittered);
                    tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
                    last_err = Some(format!("AI 请求被限流 ({})", status));
                    continue;
                }

                return Ok(resp);
            }
            Err(e) => {
                let formatted = crate::ai::openai::format_reqwest_error(&e);
                let is_connect_error = e.is_connect() || e.is_timeout();
                log::warn!(
                    "AI 请求发送失败（第 {} 次）: {} | is_connect={} is_timeout={}",
                    attempt,
                    formatted,
                    e.is_connect(),
                    e.is_timeout()
                );

                if !is_connect_error || attempt == max_attempts {
                    return Err(format!("发送 AI 请求失败: {}", formatted));
                }

                // 连接级错误：指数退避后重试
                let backoff = 1u64 << (attempt - 1); // 1s, 2s
                // M25：指数退避 + 随机抖动，避免多客户端同时重试加剧拥塞
                let sleep_secs = backoff + rand_jitter(0..=backoff);
                tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
                last_err = Some(formatted);
            }
        }
    }

    Err(format!(
        "发送 AI 请求失败（已重试 {} 次）: {}",
        max_attempts,
        last_err.unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: ProviderType, model: &str) -> AIProviderConfig {
        AIProviderConfig {
            model: model.to_string(),
            r#type: provider,
            ..Default::default()
        }
    }

    #[test]
    fn context_length_prefers_model_name() {
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "gpt-4o-2024-05-13")), 128_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "gpt-4o-mini")), 128_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "gpt-4")), 8_192);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "o1")), 128_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Anthropic, "claude-3-5-sonnet-20241022")), 200_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Gemini, "gemini-2.5-pro")), 1_048_576);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Ollama, "llama3.1:8b")), 131_072);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "deepseek-chat")), 128_000);
    }

    #[test]
    fn context_length_falls_back_to_provider_type() {
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "some-unknown-model")), 32_768);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Anthropic, "some-unknown-model")), 200_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Gemini, "some-unknown-model")), 1_048_576);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Ollama, "some-unknown-model")), 8_192);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Siliconflow, "some-unknown-model")), 32_768);
    }

    #[test]
    fn context_length_is_case_insensitive() {
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "GPT-4O")), 128_000);
        assert_eq!(max_context_length_for(&cfg(ProviderType::Openai, "")), 32_768);
    }
}
