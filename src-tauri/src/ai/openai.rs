//! OpenAI Compatible Provider — 使用 reqwest 调用 `/v1/chat/completions`
//!
//! 支持 OpenAI、OpenRouter、SiliconFlow、DashScope、Volcengine、Ollama 等
//! 所有兼容 OpenAI API 格式的服务商。

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_stream::StreamExt;

use super::provider::*;

/// OpenAI Compatible AI Provider 实现
pub struct OpenAIProvider {
    config: AIProviderConfig,
    client: reqwest::Client,
}

impl OpenAIProvider {
    /// 创建新的 OpenAI Provider 实例
    pub fn new(config: AIProviderConfig) -> Self {
        let timeout = Duration::from_secs(config.timeout.max(30));

        let client = reqwest::Client::builder()
            .timeout(timeout)
            // 连接建立超时：ARK 等服务偶尔连接建立较慢，10s 过于激进，
            // 提升到 30s 并配合下面的重试逻辑，可显著降低 Connect 超时
            .connect_timeout(Duration::from_secs(30))
            // 保持连接池中空闲连接更久，减少重复握手
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { config, client }
    }

    /// 构建请求 URL
    fn chat_completions_url(&self) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        // 如果已包含 /chat/completions，直接使用
        if base.ends_with("/chat/completions") {
            return base.to_string();
        }

        // 如果以版本路径结尾（/v1, /v2, /v3 等），直接追加 /chat/completions
        // 匹配 /v1, /v2, /v3, /api/v3 等
        if base.ends_with("/v1") || base.ends_with("/v2") || base.ends_with("/v3") {
            return format!("{}/chat/completions", base);
        }

        // 如果路径中包含版本段（/v1/, /v2/, /v3/），直接追加 /chat/completions
        if base.contains("/v1/") || base.contains("/v2/") || base.contains("/v3/") {
            return format!("{}/chat/completions", base);
        }

        // 默认追加 /v1/chat/completions（适用于仅提供域名的场景）
        format!("{}/v1/chat/completions", base)
    }

    /// 构建请求头
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !self.config.api_key.is_empty() {
            let auth = format!("Bearer {}", self.config.api_key);
            if let Ok(val) = HeaderValue::from_str(&auth) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        headers
    }

    /// 构建请求体 JSON
    fn build_request_body(&self, req: &ChatRequest) -> serde_json::Value {
        let model = req
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());

        let temperature = req
            .temperature
            .unwrap_or(self.config.temperature)
            .clamp(0.0, 2.0);

        let mut body = serde_json::json!({
            "model": model,
            "messages": req.messages.iter().map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                });
                if let Some(ref name) = m.name {
                    msg["name"] = serde_json::Value::String(name.clone());
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    msg["tool_calls"] = serde_json::Value::Array(
                        tool_calls.iter().map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": tc.r#type,
                                "function": {
                                    "name": tc.function.name,
                                    "arguments": tc.function.arguments,
                                }
                            })
                        }).collect()
                    );
                }
                if let Some(ref tool_call_id) = m.tool_call_id {
                    msg["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
                }
                msg
            }).collect::<Vec<_>>(),
            "temperature": temperature,
            "stream": req.stream,
        });

        if let Some(max_tokens) = req.max_tokens.or(self.config.max_tokens) {
            body["max_tokens"] = serde_json::Value::Number(max_tokens.into());
        }

        if let Some(ref tools) = req.tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(
                    tools
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "type": t.r#type,
                                "function": {
                                    "name": t.function.name,
                                    "description": t.function.description,
                                    "parameters": t.function.parameters,
                                }
                            })
                        })
                        .collect(),
                );
            }
        }

        body
    }

    /// 构建 models API URL
    fn models_url(&self) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        // 如果以版本路径结尾（/v1, /v2, /v3），追加 /models
        if base.ends_with("/v1") || base.ends_with("/v2") || base.ends_with("/v3") {
            return format!("{}/models", base);
        }

        // 如果路径中包含版本段，追加 /models
        if base.contains("/v1/") || base.contains("/v2/") || base.contains("/v3/") {
            return format!("{}/models", base);
        }

        // 默认追加 /v1/models
        format!("{}/v1/models", base)
    }

    /// 解析 OpenAI API 非流式响应
    fn parse_response(&self, resp: OpenAIApiResponse) -> ChatResponse {
        let choice = resp.choices.first();
        let (content, role, tool_calls, finish_reason) = match choice {
            Some(c) => {
                // DeepSeek 推理模型会把实际回复放在 reasoning_content，content 可能为空
                let mut content = c.message.content.clone().unwrap_or_default();
                if content.is_empty() {
                    content = c.message.reasoning_content.clone().unwrap_or_default();
                }
                let role = c.message.role.clone();
                let tool_calls = c.message.tool_calls.clone();
                let finish_reason = c.finish_reason.clone().unwrap_or_else(|| "stop".to_string());
                (content, role, tool_calls, finish_reason)
            }
            None => (String::new(), MessageRole::Assistant, None, "stop".to_string()),
        };

        log::info!(
            "AI 响应: model={}, content_len={}, finish_reason={}",
            resp.model,
            content.len(),
            finish_reason
        );
        let usage = resp.usage.unwrap_or_default();

        ChatResponse {
            id: resp.id,
            model: resp.model,
            content,
            role,
            tool_calls,
            usage: TokenUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            },
            finish_reason,
        }
    }

    /// 解析 SSE 流中的单个 data 行
    fn parse_sse_line(&self, line: &str) -> Option<ChatStreamChunk> {
        let line = line.trim();

        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            let data = data.trim();

            if data == "[DONE]" {
                return Some(ChatStreamChunk {
                    content: String::new(),
                    done: true,
                    tool_calls: None,
                });
            }

            // 解析 JSON
            if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                let delta = chunk.choices.first();

                if let Some(choice) = delta {
                    let mut content = choice.delta.content.clone().unwrap_or_default();
                    if content.is_empty() {
                        content = choice.delta.reasoning_content.clone().unwrap_or_default();
                    }
                    let tool_calls = choice.delta.tool_calls.clone();

                    let done = choice.finish_reason.is_some();

                    return Some(ChatStreamChunk {
                        content,
                        done,
                        // L25：tool_calls 已通过 is_some() 检查，直接移动即可
                        tool_calls,
                    });
                }
            }
        }

        None
    }

    /// 发送请求，遇到连接级错误或 429/503 自动重试
    ///
    /// 实现已抽取到 `provider::send_with_retry` 供各 Provider 共用，
    /// 此处仅委托并复用共享的重试逻辑。
    async fn send_with_retry(
        &self,
        url: &str,
        headers: reqwest::header::HeaderMap,
        body: &serde_json::Value,
        timeout_override: Option<u64>,
    ) -> Result<reqwest::Response, String> {
        super::provider::send_with_retry(&self.client, url, headers, body, timeout_override).await
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAIProvider {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        // 确保非流式请求
        let mut req = req.clone();
        req.stream = false;

        let url = self.chat_completions_url();
        log::info!("AI 请求 URL: {}", url);
        log::info!(
            "AI 请求 Model: {}",
            req.model.clone().unwrap_or_else(|| self.config.model.clone())
        );
        let body = self.build_request_body(&req);
        let headers = self.build_headers();

        // 调试日志：完整请求体（脱敏 API Key 后输出）
        log::info!("[AI-DEBUG] 非流式请求 body: {}", serde_json::to_string(&body).unwrap_or_default());
        log::info!(
            "[AI-DEBUG] 请求消息数: {}, 工具数: {}",
            req.messages.len(),
            req.tools.as_ref().map(|t| t.len()).unwrap_or(0)
        );

        let response = self.send_with_retry(&url, headers, &body, req.timeout_override).await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            log::warn!("[AI-DEBUG] AI 请求失败 status={} body={}", status, error_text);
            return Err(format!("AI 请求返回错误 ({}): {}", status, error_text));
        }

        // 调试日志：原始响应文本（在解析前记录，便于排查 AI 返回格式异常）
        let raw_text = response
            .text()
            .await
            .map_err(|e| format!("读取 AI 响应失败: {}", e))?;
        log::info!(
            "[AI-DEBUG] 原始响应长度: {} 字节, 前 500 字符: {}",
            raw_text.len(),
            raw_text.chars().take(500).collect::<String>()
        );
        log::debug!("[AI-DEBUG] 原始响应全文: {}", raw_text);

        let api_resp: OpenAIApiResponse = serde_json::from_str(&raw_text)
            .map_err(|e| {
                format!("解析 AI 响应失败: {} | 原文(前200字符): {}", e, raw_text.chars().take(200).collect::<String>())
            })?;

        Ok(self.parse_response(api_resp))
    }

    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
    ) -> Result<ChatResponse, String> {
        let mut req = req.clone();
        req.stream = true;

        let url = self.chat_completions_url();
        log::info!("AI 流式请求 URL: {}", url);
        log::info!(
            "AI 流式请求 Model: {}",
            req.model.clone().unwrap_or_else(|| self.config.model.clone())
        );
        let body = self.build_request_body(&req);
        let headers = self.build_headers();

        log::info!("[AI-DEBUG] 流式请求 body: {}", serde_json::to_string(&body).unwrap_or_default());

        let response = self
            .send_with_retry(&url, headers, &body, req.timeout_override)
            .await
            .map_err(|e| format!("发送 AI 流式请求失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            log::warn!("[AI-DEBUG] 流式请求失败 status={} body={}", status, error_text);
            return Err(format!("AI 流式请求返回错误 ({}): {}", status, error_text));
        }

        // 读取 SSE 流
        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = "stop".to_string();
        let mut model_name = self.config.model.clone();
        let mut response_id = String::new();

        let mut stream = response.bytes_stream();

        let mut buffer = String::new();
        let mut sse_line_count: usize = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("读取流失败: {}", e))?;

            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // 按行处理 buffer
            while let Some(newline_pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=newline_pos).collect();
                let line = line.trim_end_matches('\r').trim_end_matches('\n');

                if line.is_empty() {
                    continue;
                }

                // 调试日志：记录 SSE 原始行（debug 级别避免噪音）
                if line.starts_with("data: ") {
                    sse_line_count += 1;
                    log::debug!(
                        "[AI-DEBUG] SSE #{}: {}",
                        sse_line_count,
                        line.get(..line.floor_char_boundary(200)).unwrap_or(line)
                    );
                } else {
                    log::debug!("[AI-DEBUG] SSE 非 data 行: {}", line);
                }

                if let Some(stream_chunk) = self.parse_sse_line(line) {
                    if !stream_chunk.content.is_empty() {
                        full_content.push_str(&stream_chunk.content);
                    }

                    if let Some(ref tc) = stream_chunk.tool_calls {
                        // 合并 tool_calls
                        for new_tc in tc {
                            // 查找是否已有相同 id 的 tool_call
                            let existing = tool_calls.iter_mut().find(|t| t.id == new_tc.id);
                            match existing {
                                Some(t) => {
                                    // 追加 arguments
                                    t.function.arguments.push_str(&new_tc.function.arguments);
                                }
                                None => {
                                    tool_calls.push(new_tc.clone());
                                }
                            }
                        }
                    }

                    on_chunk(stream_chunk.clone());

                    if stream_chunk.done {
                        // 流结束
                        break;
                    }
                }
            }
        }

        // 处理 buffer 中剩余的数据
        if !buffer.is_empty() {
            let line = buffer.trim();
            if !line.is_empty() {
                if let Some(stream_chunk) = self.parse_sse_line(line) {
                    if !stream_chunk.content.is_empty() {
                        full_content.push_str(&stream_chunk.content);
                    }
                    on_chunk(stream_chunk);
                }
            }
        }

        // 调试日志：流式响应汇总
        log::info!(
            "[AI-DEBUG] 流式响应完成: SSE 行数={}, content_len={}, tool_calls={}, finish_reason={}",
            sse_line_count,
            full_content.len(),
            tool_calls.len(),
            finish_reason
        );
        log::debug!(
            "[AI-DEBUG] 流式响应完整 content (前 1000 字符): {}",
            full_content.chars().take(1000).collect::<String>()
        );

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
            usage: TokenUsage::default(),
            finish_reason,
        })
    }

    fn config(&self) -> &AIProviderConfig {
        &self.config
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            function_calling: true,
            vision: false,
            max_context_length: match self.config.r#type {
                ProviderType::Ollama => 8192,
                _ => 32768,
            },
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        let url = self.models_url();
        log::info!("获取模型列表: {}", url);

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
            return Err(format!("获取模型列表返回错误 ({}): {}", status, error_text));
        }

        // OpenAI models API 返回格式: { "object": "list", "data": [...] }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("解析模型列表响应失败: {}", e))?;

        let models: Vec<ModelInfo> = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| ModelInfo {
                        id: m.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                        owned_by: m.get("owned_by").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        created: m.get("created").and_then(|v| v.as_i64()).unwrap_or(0),
                        extra: m.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        log::info!("获取到 {} 个模型", models.len());
        Ok(models)
    }
}

// ============================================================================
// OpenAI API 响应/流式响应类型
// ============================================================================

/// OpenAI API 非流式响应
#[derive(Debug, Deserialize)]
struct OpenAIApiResponse {
    id: String,
    model: String,
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

/// OpenAI API 选择项
#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    index: u32,
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

/// OpenAI API 消息
#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    role: MessageRole,
    #[serde(default)]
    content: Option<String>,
    /// 推理模型的 reasoning_content（DeepSeek-R1/V4 等会在 content 为空时放这里）
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

/// OpenAI API Usage
#[derive(Debug, Default, Deserialize)]
struct OpenAIUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

/// OpenAI API 流式响应块
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    id: String,
    model: String,
    choices: Vec<OpenAIStreamChoice>,
}

/// OpenAI API 流式选择项
#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    index: u32,
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

/// OpenAI API 流式 Delta
#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    #[serde(default)]
    role: Option<MessageRole>,
    #[serde(default)]
    content: Option<String>,
    /// 推理模型流式中的 reasoning_content
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCall>>,
}

/// OpenAI API 错误响应
#[derive(Debug, Deserialize)]
pub struct OpenAIError {
    pub error: OpenAIErrorDetail,
}

/// OpenAI API 错误详情
#[derive(Debug, Deserialize)]
pub struct OpenAIErrorDetail {
    pub message: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

/// 格式化 reqwest 错误，展开完整错误链以便定位根因
///
/// reqwest 默认的 Display 只显示顶层信息（如 "error sending request for url (URL)"），
/// 真正的原因（timeout / connection reset / TLS / DNS）藏在 `source()` 链中。
/// 此函数沿 `std::error::Error::source` 链逐层展开，把所有 cause 拼接出来。
pub fn format_reqwest_error(err: &reqwest::Error) -> String {
    use std::error::Error as StdError;
    let mut parts: Vec<String> = Vec::new();
    parts.push(err.to_string());

    let mut current: Option<&(dyn StdError + 'static)> = err.source();
    while let Some(source) = current {
        let s = source.to_string();
        // 避免重复（有些层 Display 重复父层信息）
        if !parts.iter().any(|p| p == &s) {
            parts.push(s);
        }
        current = source.source();
    }

    parts.join(" → ")
}
