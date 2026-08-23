//! Anthropic Native Provider — 调用 Anthropic Messages API
//!
//! 原生支持 `https://api.anthropic.com/v1/messages`：
//! - 认证头：`x-api-key` + `anthropic-version`（而非 OpenAI 的 `Authorization: Bearer`）
//! - system prompt 为顶层 `system` 字段，不放入 messages
//! - 工具调用使用 `tool_use` / `tool_result` content block
//! - 流式响应为 `event: <type>` / `data: <json>` 两行一组的 SSE 格式

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::StreamExt;

use super::provider::*;

/// Anthropic Messages API 版本头
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic API 要求 max_tokens 必填
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic 原生 AI Provider 实现
pub struct AnthropicProvider {
    config: AIProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// 创建新的 Anthropic Provider 实例
    pub fn new(config: AIProviderConfig) -> Self {
        let timeout = Duration::from_secs(config.timeout.max(30));

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { config, client }
    }

    /// 构建 Messages API URL
    fn messages_url(&self) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        if base.ends_with("/messages") {
            return base.to_string();
        }
        if base.ends_with("/v1") {
            return format!("{}/messages", base);
        }
        format!("{}/v1/messages", base)
    }

    /// 构建 Models API URL
    fn models_url(&self) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        if base.ends_with("/models") {
            return base.to_string();
        }
        if base.ends_with("/v1") {
            return format!("{}/models", base);
        }
        format!("{}/v1/models", base)
    }

    /// 构建请求头
    ///
    /// Anthropic 原生认证：`x-api-key: <key>` + `anthropic-version: 2023-06-01`
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !self.config.api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&self.config.api_key) {
                headers.insert("x-api-key", val);
            }
        }
        if let Ok(val) = HeaderValue::from_str(ANTHROPIC_VERSION) {
            headers.insert("anthropic-version", val);
        }

        headers
    }

    /// 将统一 ChatRequest 转换为 Anthropic Messages 请求体
    ///
    /// 格式转换规则：
    /// - `system` 消息提取为顶层 `system` 字段（多个则用空行拼接）
    /// - 带 `tool_call_id` 的 user 消息 → `tool_result` content block
    /// - 带 `tool_calls` 的 assistant 消息 → `text` + `tool_use` content blocks
    /// - 其余 user/assistant 消息直接映射 role + 字符串 content
    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let temperature = req
            .temperature
            .unwrap_or(self.config.temperature)
            .clamp(0.0, 1.0);
        let max_tokens = req
            .max_tokens
            .or(self.config.max_tokens)
            .unwrap_or(DEFAULT_MAX_TOKENS);

        let mut system_parts: Vec<String> = Vec::new();
        let mut messages: Vec<Value> = Vec::new();

        for m in &req.messages {
            match m.role {
                MessageRole::System => {
                    if !m.content.is_empty() {
                        system_parts.push(m.content.clone());
                    }
                }
                MessageRole::User => {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        // 工具执行结果
                        messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_call_id,
                                "content": m.content,
                            }]
                        }));
                    } else {
                        messages.push(json!({
                            "role": "user",
                            "content": m.content,
                        }));
                    }
                }
                MessageRole::Assistant => {
                    let mut content: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        content.push(json!({ "type": "text", "text": m.content }));
                    }
                    if let Some(ref tool_calls) = m.tool_calls {
                        for tc in tool_calls {
                            let input: Value =
                                serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                            content.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": input,
                            }));
                        }
                    }
                    messages.push(json!({ "role": "assistant", "content": content }));
                }
                MessageRole::Tool => {
                    // Anthropic 无独立 tool 角色；视为 tool_result 附在 user 消息上
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                            "content": m.content,
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": messages,
            "stream": stream,
            "temperature": temperature,
        });

        if !system_parts.is_empty() {
            body["system"] = Value::String(system_parts.join("\n\n"));
        }

        if let Some(ref tools) = req.tools {
            if !tools.is_empty() {
                body["tools"] = Value::Array(
                    tools
                        .iter()
                        .map(|t| {
                            json!({
                                "name": t.function.name,
                                "description": t.function.description,
                                "input_schema": t.function.parameters,
                            })
                        })
                        .collect(),
                );
            }
        }

        body
    }

    /// 解析 Anthropic 非流式响应
    fn parse_response(&self, resp: AnthropicApiResponse, model: &str) -> ChatResponse {
        // 优先使用响应中返回的 model（更准确），缺失时回退到请求中的 model
        let model = if resp.model.is_empty() {
            model.to_string()
        } else {
            resp.model.clone()
        };
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in &resp.content {
            match block.r#type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        content.push_str(text);
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name)) = (&block.id, &block.name) {
                        tool_calls.push(ToolCall {
                            id: id.clone(),
                            r#type: "function".to_string(),
                            index: 0,
                            function: ToolCallFunction {
                                name: name.clone(),
                                arguments: block.input.to_string(),
                            },
                        });
                    }
                }
                _ => {}
            }
        }

        let finish_reason = match resp.stop_reason.as_deref() {
            Some("max_tokens") => "length".to_string(),
            Some("tool_use") => "tool_calls".to_string(),
            _ => "stop".to_string(),
        };

        ChatResponse {
            id: resp.id,
            model,
            content,
            role: MessageRole::Assistant,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: TokenUsage {
                prompt_tokens: resp.usage.input_tokens,
                completion_tokens: resp.usage.output_tokens,
                total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
            },
            finish_reason,
            reasoning: None,
        }
    }

    /// 解析 Anthropic SSE 流，返回（是否已结束, 可选错误）
    #[allow(clippy::too_many_arguments)]
    fn parse_stream_line(
        &self,
        line: &str,
        full_content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        blocks: &mut Vec<StreamBlock>,
        response_id: &mut String,
        model_name: &mut String,
        input_tokens: &mut u32,
        output_tokens: &mut u32,
        finish_reason: &mut String,
    ) -> Result<bool, String> {
        let line = line.trim();

        // event: 行仅作控制流标记，实际事件类型已由 data 行内嵌的 type 字段承载
        if line.starts_with("event: ") {
            return Ok(false);
        }

        let Some(data) = line.strip_prefix("data: ") else {
            return Ok(false);
        };
        let data = data.trim();

        let event: AnthropicStreamEvent = match serde_json::from_str(data) {
            Ok(e) => e,
            Err(_) => return Ok(false), // ping 等未知负载，忽略
        };

        match event.r#type.as_str() {
            "message_start" => {
                if let Some(message) = event.message {
                    if !message.id.is_empty() {
                        *response_id = message.id;
                    }
                    if !message.model.is_empty() {
                        *model_name = message.model;
                    }
                }
                if let Some(usage) = event.usage {
                    *input_tokens = usage.input_tokens;
                }
            }
            "content_block_start" => {
                if let Some(block) = event.content_block {
                    let idx = event.index.unwrap_or(0);
                    while blocks.len() <= idx {
                        blocks.push(StreamBlock::default());
                    }
                    let sb = &mut blocks[idx];
                    sb.block_type = block.r#type.clone();
                    sb.id = block.id.clone().unwrap_or_default();
                    sb.name = block.name.clone().unwrap_or_default();
                    sb.text = block.text.clone().unwrap_or_default();
                }
            }
            "content_block_delta" => {
                let idx = event.index.unwrap_or(0);
                if let Some(delta) = event.delta {
                    match delta.r#type.as_str() {
                        "text_delta" => {
                            if let Some(text) = delta.text {
                                full_content.push_str(&text);
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial) = delta.partial_json {
                                while blocks.len() <= idx {
                                    blocks.push(StreamBlock::default());
                                }
                                blocks[idx].input_json.push_str(&partial);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let idx = event.index.unwrap_or(0);
                if idx < blocks.len() {
                    let sb = &blocks[idx];
                    if sb.block_type == "tool_use" && !sb.name.is_empty() {
                        tool_calls.push(ToolCall {
                            id: if sb.id.is_empty() {
                                format!("toolu_{}", idx)
                            } else {
                                sb.id.clone()
                            },
                            r#type: "function".to_string(),
                            index: idx as u32,
                            function: ToolCallFunction {
                                name: sb.name.clone(),
                                arguments: sb.input_json.clone(),
                            },
                        });
                    }
                }
            }
            "message_delta" => {
                if let Some(delta) = event.delta {
                    if let Some(reason) = delta.stop_reason {
                        *finish_reason = match reason.as_str() {
                            "max_tokens" => "length".to_string(),
                            "tool_use" => "tool_calls".to_string(),
                            _ => "stop".to_string(),
                        };
                    }
                }
                if let Some(usage) = event.usage {
                    *output_tokens = usage.output_tokens;
                }
            }
            "message_stop" => return Ok(true),
            "error" => {
                let msg = event
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "未知错误".to_string());
                return Err(format!("Anthropic 流式错误: {}", msg));
            }
            _ => {}
        }

        Ok(false)
    }
}

/// 流式响应中某个 content block 的累积状态
#[derive(Default)]
struct StreamBlock {
    block_type: String,
    id: String,
    name: String,
    text: String,
    input_json: String,
}

#[async_trait::async_trait]
impl AiProvider for AnthropicProvider {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        // C4：不再全量 clone req（会深拷贝整份 messages）。stream 通过
        // build_body 显式参数传入，model 从引用直接 clone 取值。
        let url = self.messages_url();
        log::info!("Anthropic 请求 URL: {}", url);
        log::info!(
            "Anthropic 请求 Model: {}",
            req.model
                .clone()
                .unwrap_or_else(|| self.config.model.clone())
        );

        let body = self.build_body(req, false);
        let headers = self.build_headers();

        log::info!(
            "[AI-DEBUG] Anthropic 非流式请求 body: {}",
            serde_json::to_string(&body).unwrap_or_default()
        );

        let response = super::provider::send_with_retry(
            &self.client,
            &url,
            headers,
            &body,
            req.timeout_override,
        )
        .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            log::warn!(
                "[AI-DEBUG] Anthropic 请求失败 status={} body={}",
                status,
                error_text.chars().take(500).collect::<String>()
            );
            return Err(format!(
                "Anthropic 请求返回错误 ({}): {}",
                status,
                error_text.chars().take(500).collect::<String>()
            ));
        }

        let raw_text = response
            .text()
            .await
            .map_err(|e| format!("读取 Anthropic 响应失败: {}", e))?;
        log::info!(
            "[AI-DEBUG] Anthropic 原始响应长度: {} 字节, 前 500 字符: {}",
            raw_text.len(),
            raw_text.chars().take(500).collect::<String>()
        );

        let model = req
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let api_resp: AnthropicApiResponse = serde_json::from_str(&raw_text).map_err(|e| {
            format!(
                "解析 Anthropic 响应失败: {} | 原文(前200字符): {}",
                e,
                raw_text.chars().take(200).collect::<String>()
            )
        })?;

        Ok(self.parse_response(api_resp, &model))
    }

    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
    ) -> Result<ChatResponse, String> {
        // C4：不再全量 clone req。stream 通过 build_body 显式参数传入。
        let url = self.messages_url();
        log::info!("Anthropic 流式请求 URL: {}", url);

        let body = self.build_body(req, true);
        let headers = self.build_headers();

        let response = super::provider::send_with_retry(
            &self.client,
            &url,
            headers,
            &body,
            req.timeout_override,
        )
        .await
        .map_err(|e| format!("发送 Anthropic 流式请求失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!(
                "Anthropic 流式请求返回错误 ({}): {}",
                status,
                error_text.chars().take(500).collect::<String>()
            ));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut blocks: Vec<StreamBlock> = Vec::new();
        let mut finish_reason = "stop".to_string();
        let mut model_name = self.config.model.clone();
        let mut response_id = String::new();
        let mut input_tokens: u32 = 0;
        let mut output_tokens: u32 = 0;
        let mut stream_done = false;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("读取流失败: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=newline_pos).collect();
                let done = self.parse_stream_line(
                    &line,
                    &mut full_content,
                    &mut tool_calls,
                    &mut blocks,
                    &mut response_id,
                    &mut model_name,
                    &mut input_tokens,
                    &mut output_tokens,
                    &mut finish_reason,
                )?;
                if done {
                    stream_done = true;
                    break;
                }
            }
            if stream_done {
                break;
            }
        }

        // 流式推送：把累积的完整内容作为最终 chunk 发出（非流式消费方也能拿到结果）
        on_chunk(ChatStreamChunk {
            content: full_content.clone(),
            done: true,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls.clone())
            },
            reset: false,
            usage: None,
        });

        Ok(ChatResponse {
            id: response_id,
            model: model_name,
            content: full_content,
            role: MessageRole::Assistant,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: TokenUsage {
                prompt_tokens: input_tokens,
                completion_tokens: output_tokens,
                total_tokens: input_tokens + output_tokens,
            },
            finish_reason,
            reasoning: None,
        })
    }

    fn config(&self) -> &AIProviderConfig {
        &self.config
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            function_calling: true,
            vision: true,
            // 优先按模型名查表（Claude 各型号 200K/1M），查不到回退 Anthropic 200K
            max_context_length: crate::ai::provider::max_context_length_for(&self.config),
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        let url = self.models_url();
        log::info!("获取 Anthropic 模型列表: {}", url);

        let headers = self.build_headers();

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| format!("获取模型列表失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!(
                "获取模型列表返回错误 ({}): {}",
                status,
                error_text.chars().take(500).collect::<String>()
            ));
        }

        // Anthropic /v1/models 返回: { "data": [{ "id", "display_name", "created_at" }] }
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("解析模型列表响应失败: {}", e))?;

        let models: Vec<ModelInfo> = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let mut extra = m.clone();
                        crate::ai::provider::inject_context_length_fallback(
                            &self.config.r#type,
                            &id,
                            &mut extra,
                        );
                        ModelInfo {
                            id,
                            owned_by: "anthropic".to_string(),
                            created: m.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
                            extra,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        log::info!("获取到 {} 个 Anthropic 模型", models.len());
        Ok(models)
    }
}

// ============================================================================
// Anthropic API 响应类型
// ============================================================================

/// Anthropic 非流式响应
#[derive(Debug, Deserialize)]
struct AnthropicApiResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: AnthropicUsage,
}

/// Anthropic content block（text / tool_use）
#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Value,
}

/// Anthropic token 用量
#[derive(Debug, Default, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

/// Anthropic 流式事件（data 行）
#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    delta: Option<AnthropicStreamDelta>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    error: Option<AnthropicErrorDetail>,
}

/// message_start 事件中的 message 摘要
#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
}

/// content_block_delta / message_delta 事件中的 delta
#[derive(Debug, Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

/// error 事件详情
#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    #[serde(default)]
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> AnthropicProvider {
        AnthropicProvider::new(AIProviderConfig {
            id: "test".to_string(),
            name: "test".to_string(),
            r#type: ProviderType::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "sk-test".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn build_body_extracts_system_prompt() {
        let p = provider();
        let req = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "你是学习助手".to_string(),
                    ..Default::default()
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "你好".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let body = p.build_body(&req, false);
        assert_eq!(body["system"], "你是学习助手");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "你好");
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn build_body_converts_tool_result() {
        let p = provider();
        let req = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: "工具结果".to_string(),
                tool_call_id: Some("toolu_1".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let body = p.build_body(&req, false);
        let content = &body["messages"][0]["content"][0];
        assert_eq!(content["type"], "tool_result");
        assert_eq!(content["tool_use_id"], "toolu_1");
        assert_eq!(content["content"], "工具结果");
    }

    #[test]
    fn build_body_converts_tool_use() {
        let p = provider();
        let req = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::Assistant,
                content: "稍等".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "toolu_1".to_string(),
                    r#type: "function".to_string(),
                    index: 0,
                    function: ToolCallFunction {
                        name: "read_file".to_string(),
                        arguments: r#"{"path":"state/current.state"}"#.to_string(),
                    },
                }]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let body = p.build_body(&req, false);
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["name"], "read_file");
        assert_eq!(content[1]["input"]["path"], "state/current.state");
    }

    #[test]
    fn parse_response_maps_tool_use_and_usage() {
        let p = provider();
        let resp = AnthropicApiResponse {
            id: "msg_1".to_string(),
            model: "claude-3-5-sonnet-20241022".to_string(),
            content: vec![
                AnthropicContentBlock {
                    r#type: "text".to_string(),
                    text: Some("结果".to_string()),
                    id: None,
                    name: None,
                    input: Value::Null,
                },
                AnthropicContentBlock {
                    r#type: "tool_use".to_string(),
                    text: None,
                    id: Some("toolu_1".to_string()),
                    name: Some("read_file".to_string()),
                    input: json!({ "path": "a.md" }),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };
        let out = p.parse_response(resp, "fallback-model");
        assert_eq!(out.content, "结果");
        assert_eq!(out.model, "claude-3-5-sonnet-20241022");
        let tcs = out.tool_calls.unwrap();
        assert_eq!(tcs[0].function.name, "read_file");
        assert_eq!(tcs[0].function.arguments, r#"{"path":"a.md"}"#);
        assert_eq!(out.finish_reason, "tool_calls");
        assert_eq!(out.usage.total_tokens, 15);
    }
}
