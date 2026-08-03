//! AI Provider Trait — 统一 AI Provider 接口定义
//!
//! 对应前端 TypeScript 类型: `types/ai.ts`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
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
    pub id: String,
    #[serde(default = "default_function_type")]
    pub r#type: String,
    pub function: ToolCallFunction,
}

fn default_function_type() -> String {
    "function".to_string()
}

/// 工具调用函数
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
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
}

/// AI Provider 能力
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub function_calling: bool,
    pub vision: bool,
    pub max_context_length: u32,
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
            max_context_length: 32768,
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
- 完成率统计需区分 Priority A 与 Priority B 分别计算
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

subject 只能是 "math" / "english" / "politics" / "professional"；priority 只能是 "A" / "B"。
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
- 知识库：assets/knowledge/**/*.md
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
            // Gemini 也可以用 OpenAI 兼容接口
            Ok(Arc::new(crate::ai::openai::OpenAIProvider::new(config)))
        }
        ProviderType::Anthropic => {
            // Anthropic 可以用 OpenAI 兼容接口（如果配置了兼容端点）
            Ok(Arc::new(crate::ai::openai::OpenAIProvider::new(config)))
        }
    }
}
