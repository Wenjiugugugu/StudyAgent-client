//! Gemini Native Provider — 调用 Google Gemini API
//!
//! 原生支持 `https://generativelanguage.googleapis.com/v1beta`：
//! - 认证：`x-goog-api-key` 请求头（避免 API Key 出现在 URL 与日志中）
//! - 消息格式：`contents[].parts[].text`，system 为顶层 `systemInstruction`
//! - 工具调用：`functionDeclarations` / `functionCall` / `functionResponse`
//! - 流式：`:streamGenerateContent?alt=sse`，chunk 与普通响应结构一致

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::StreamExt;

use super::provider::*;

/// Gemini 默认最大输出 token
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Gemini 原生 AI Provider 实现
pub struct GeminiProvider {
    config: AIProviderConfig,
    client: reqwest::Client,
}

impl GeminiProvider {
    /// 创建新的 Gemini Provider 实例
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

    /// 当前模型名
    fn model(&self, req: &ChatRequest) -> String {
        req.model.clone().unwrap_or_else(|| self.config.model.clone())
    }

    /// 构建 generateContent / streamGenerateContent URL
    fn generate_url(&self, model: &str, stream: bool) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        // base 可能包含 /v1beta 或 /v1 版本段；否则默认补 /v1beta
        let root = if base.contains("/v1beta") || base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{}/v1beta", base)
        };

        if stream {
            format!("{}/models/{}:streamGenerateContent?alt=sse", root, model)
        } else {
            format!("{}/models/{}:generateContent", root, model)
        }
    }

    /// 构建 Models 列表 URL
    fn models_url(&self) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        let root = if base.contains("/v1beta") || base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{}/v1beta", base)
        };

        format!("{}/models", root)
    }

    /// 构建请求头（Gemini 使用 x-goog-api-key，而非 Authorization: Bearer）
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !self.config.api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&self.config.api_key) {
                headers.insert("x-goog-api-key", val);
            }
        }

        headers
    }

    /// 将统一 ChatRequest 转换为 Gemini 请求体
    ///
    /// 格式转换规则：
    /// - `system` 消息 → 顶层 `systemInstruction.parts[].text`
    /// - 带 `tool_call_id` 的 user 消息 → `functionResponse` part
    /// - 带 `tool_calls` 的 assistant 消息 → `functionCall` parts
    /// - Gemini 角色仅 `user` / `model`（assistant → model）
    fn build_body(&self, req: &ChatRequest) -> Value {
        let temperature = req
            .temperature
            .unwrap_or(self.config.temperature)
            .clamp(0.0, 2.0);
        let max_output_tokens = req
            .max_tokens
            .or(self.config.max_tokens)
            .unwrap_or(DEFAULT_MAX_TOKENS);

        let mut system_parts: Vec<String> = Vec::new();
        let mut contents: Vec<Value> = Vec::new();

        for m in &req.messages {
            match m.role {
                MessageRole::System => {
                    if !m.content.is_empty() {
                        system_parts.push(m.content.clone());
                    }
                }
                MessageRole::User => {
                    if m.tool_call_id.is_some() {
                        // 工具执行结果 → functionResponse
                        let resp_val: Value = serde_json::from_str(&m.content)
                            .unwrap_or_else(|_| Value::String(m.content.clone()));
                        contents.push(json!({
                            "role": "user",
                            "parts": [{
                                "functionResponse": {
                                    "name": m.name.clone().unwrap_or_else(|| "unknown".to_string()),
                                    "response": resp_val,
                                }
                            }]
                        }));
                    } else {
                        contents.push(json!({
                            "role": "user",
                            "parts": [{ "text": m.content }]
                        }));
                    }
                }
                MessageRole::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        parts.push(json!({ "text": m.content }));
                    }
                    if let Some(ref tool_calls) = m.tool_calls {
                        for tc in tool_calls {
                            let args: Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or_else(|_| Value::Null);
                            parts.push(json!({
                                "functionCall": {
                                    "name": tc.function.name,
                                    "args": args,
                                }
                            }));
                        }
                    }
                    contents.push(json!({ "role": "model", "parts": parts }));
                }
                MessageRole::Tool => {
                    let resp_val: Value = serde_json::from_str(&m.content)
                        .unwrap_or_else(|_| Value::String(m.content.clone()));
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": m.name.clone().unwrap_or_else(|| "unknown".to_string()),
                                "response": resp_val,
                            }
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "temperature": temperature,
                "maxOutputTokens": max_output_tokens,
            },
        });

        if !system_parts.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": system_parts.join("\n\n") }]
            });
        }

        if let Some(ref tools) = req.tools {
            if !tools.is_empty() {
                body["tools"] = Value::Array(vec![json!({
                    "functionDeclarations": tools.iter().map(|t| {
                        json!({
                            "name": t.function.name,
                            "description": t.function.description,
                            "parameters": t.function.parameters,
                        })
                    }).collect::<Vec<_>>()
                })]);
            }
        }

        body
    }

    /// 解析 Gemini 响应（普通与流式 chunk 结构一致，可共用）
    fn parse_response(&self, resp: GeminiResponse, model: &str) -> ChatResponse {
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(candidate) = resp.candidates.first() {
            for part in &candidate.content.parts {
                if let Some(text) = &part.text {
                    content.push_str(text);
                }
                if let Some(fc) = &part.function_call {
                    tool_calls.push(ToolCall {
                        id: format!("fc_{}", tool_calls.len()),
                        r#type: "function".to_string(),
                        index: 0,
                        function: ToolCallFunction {
                            name: fc.name.clone(),
                            arguments: fc.args.to_string(),
                        },
                    });
                }
            }
        }

        let finish_reason = match resp
            .candidates
            .first()
            .and_then(|c| c.finish_reason.as_deref())
        {
            Some("MAX_TOKENS") => "length".to_string(),
            Some("SAFETY") | Some("RECITATION") | Some("BLOCKLIST") | Some("PROHIBITED_CONTENT") => {
                "content_filter".to_string()
            }
            _ => "stop".to_string(),
        };

        let usage = resp.usage_metadata.unwrap_or_default();

        ChatResponse {
            id: format!("gemini-{}", model),
            model: model.to_string(),
            content,
            role: MessageRole::Assistant,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: TokenUsage {
                prompt_tokens: usage.prompt_token_count,
                completion_tokens: usage.candidates_token_count,
                total_tokens: usage.total_token_count,
            },
            finish_reason,
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for GeminiProvider {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        let mut req = req.clone();
        req.stream = false;

        let model = self.model(&req);
        let url = self.generate_url(&model, false);
        log::info!("Gemini 请求 URL: {}", url);

        let body = self.build_body(&req);
        let headers = self.build_headers();

        log::info!(
            "[AI-DEBUG] Gemini 非流式请求 body: {}",
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
                "[AI-DEBUG] Gemini 请求失败 status={} body={}",
                status,
                error_text.chars().take(500).collect::<String>()
            );
            return Err(format!(
                "Gemini 请求返回错误 ({}): {}",
                status,
                error_text.chars().take(500).collect::<String>()
            ));
        }

        let raw_text = response
            .text()
            .await
            .map_err(|e| format!("读取 Gemini 响应失败: {}", e))?;
        log::info!(
            "[AI-DEBUG] Gemini 原始响应长度: {} 字节, 前 500 字符: {}",
            raw_text.len(),
            raw_text.chars().take(500).collect::<String>()
        );

        let api_resp: GeminiResponse = serde_json::from_str(&raw_text).map_err(|e| {
            format!(
                "解析 Gemini 响应失败: {} | 原文(前200字符): {}",
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
        let mut req = req.clone();
        req.stream = true;

        let model = self.model(&req);
        let url = self.generate_url(&model, true);
        log::info!("Gemini 流式请求 URL: {}", url);

        let body = self.build_body(&req);
        let headers = self.build_headers();

        let response = super::provider::send_with_retry(
            &self.client,
            &url,
            headers,
            &body,
            req.timeout_override,
        )
        .await
        .map_err(|e| format!("发送 Gemini 流式请求失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!(
                "Gemini 流式请求返回错误 ({}): {}",
                status,
                error_text.chars().take(500).collect::<String>()
            ));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = "stop".to_string();
        let mut usage = GeminiUsage::default();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("读取流失败: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(newline_pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=newline_pos).collect();
                let line = line.trim();

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                let data = data.trim();

                let chunk_resp: GeminiResponse = match serde_json::from_str(data) {
                    Ok(r) => r,
                    Err(_) => continue, // 忽略 ping 等未知行
                };

                if let Some(candidate) = chunk_resp.candidates.first() {
                    for part in &candidate.content.parts {
                        if let Some(text) = &part.text {
                            if !text.is_empty() {
                                full_content.push_str(text);
                                on_chunk(ChatStreamChunk {
                                    content: text.clone(),
                                    done: false,
                                    tool_calls: None,
                                    reset: false,
                                    usage: None,
                                });
                            }
                        }
                        if let Some(fc) = &part.function_call {
                            tool_calls.push(ToolCall {
                                id: format!("fc_{}", tool_calls.len()),
                                r#type: "function".to_string(),
                                index: 0,
                                function: ToolCallFunction {
                                    name: fc.name.clone(),
                                    arguments: fc.args.to_string(),
                                },
                            });
                        }
                    }
                    if let Some(reason) = &candidate.finish_reason {
                        finish_reason = match reason.as_str() {
                            "MAX_TOKENS" => "length".to_string(),
                            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" => {
                                "content_filter".to_string()
                            }
                            _ => "stop".to_string(),
                        };
                    }
                }

                let has_usage = chunk_resp.usage_metadata.is_some();
                if let Some(u) = chunk_resp.usage_metadata {
                    usage = u;
                }

                if chunk_resp.candidates.is_empty() && has_usage {
                    // 流式结束 chunk（仅含 usageMetadata 或 finishReason 空 candidates）
                    on_chunk(ChatStreamChunk {
                        content: String::new(),
                        done: true,
                        tool_calls: None,
                        reset: false,
                        usage: Some(TokenUsage {
                            prompt_tokens: usage.prompt_token_count,
                            completion_tokens: usage.candidates_token_count,
                            total_tokens: usage.total_token_count,
                        }),
                    });
                }
            }
        }

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
            id: format!("gemini-{}", model),
            model,
            content: full_content,
            role: MessageRole::Assistant,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: TokenUsage {
                prompt_tokens: usage.prompt_token_count,
                completion_tokens: usage.candidates_token_count,
                total_tokens: usage.total_token_count,
            },
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
            vision: true,
            // 优先按模型名查表（Gemini 各型号 1M），查不到回退 Gemini 1M
            max_context_length: crate::ai::provider::max_context_length_for(&self.config),
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        let url = self.models_url();
        log::info!("获取 Gemini 模型列表: {}", url);

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

        // Gemini /v1beta/models 返回: { "models": [{ "name": "models/gemini-...", "displayName": ... }] }
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("解析模型列表响应失败: {}", e))?;

        let models: Vec<ModelInfo> = body
            .get("models")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        // name 形如 "models/gemini-1.5-pro"，剥离前缀作为 id
                        let name = m
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let id = name.strip_prefix("models/").unwrap_or(&name).to_string();
                        let mut extra = m.clone();
                        crate::ai::provider::inject_context_length_fallback(
                            &self.config.r#type,
                            &id,
                            &mut extra,
                        );
                        ModelInfo {
                            id,
                            owned_by: "google".to_string(),
                            created: 0,
                            extra,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        log::info!("获取到 {} 个 Gemini 模型", models.len());
        Ok(models)
    }
}

// ============================================================================
// Gemini API 响应类型
// ============================================================================

/// Gemini 响应（普通与流式 chunk 结构一致）
#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsage>,
}

/// Gemini 候选回答
#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    #[serde(default)]
    content: GeminiContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

/// Gemini content（parts 列表）
#[derive(Debug, Default, Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

/// Gemini part（text / functionCall）
#[derive(Debug, Deserialize)]
struct GeminiPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    function_call: Option<GeminiFunctionCall>,
}

/// Gemini functionCall
#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Value,
}

/// Gemini token 用量
#[derive(Debug, Default, Deserialize)]
struct GeminiUsage {
    #[serde(default)]
    prompt_token_count: u32,
    #[serde(default)]
    candidates_token_count: u32,
    #[serde(default)]
    total_token_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> GeminiProvider {
        GeminiProvider::new(AIProviderConfig {
            id: "test".to_string(),
            name: "test".to_string(),
            r#type: ProviderType::Gemini,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            api_key: "sk-test".to_string(),
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn build_body_maps_system_and_messages() {
        let p = provider();
        let req = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "你是助手".to_string(),
                    ..Default::default()
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "你好".to_string(),
                    ..Default::default()
                },
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: "你好！".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let body = p.build_body(&req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "你是助手");
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "你好");
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][1]["parts"][0]["text"], "你好！");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 4096);
    }

    #[test]
    fn build_body_maps_function_call_and_response() {
        let p = provider();
        let req = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: "调用工具".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "fc_1".to_string(),
                        r#type: "function".to_string(),
                        index: 0,
                        function: ToolCallFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"a"}"#.to_string(),
                        },
                    }]),
                    ..Default::default()
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "文件内容".to_string(),
                    tool_call_id: Some("fc_1".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let body = p.build_body(&req);
        let parts0 = &body["contents"][0]["parts"];
        assert_eq!(parts0[0]["text"], "调用工具");
        assert_eq!(parts0[1]["functionCall"]["name"], "read_file");
        assert_eq!(parts0[1]["functionCall"]["args"]["path"], "a");
        let parts1 = &body["contents"][1]["parts"];
        assert_eq!(parts1[0]["functionResponse"]["name"], "unknown");
        assert_eq!(parts1[0]["functionResponse"]["response"], "文件内容");
    }

    #[test]
    fn parse_response_maps_finish_reason_and_usage() {
        let p = provider();
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![
                        GeminiPart {
                            text: Some("你好".to_string()),
                            function_call: None,
                        },
                        GeminiPart {
                            text: None,
                            function_call: Some(GeminiFunctionCall {
                                name: "read_file".to_string(),
                                args: json!({ "path": "a" }),
                            }),
                        },
                    ],
                },
                finish_reason: Some("MAX_TOKENS".to_string()),
            }],
            usage_metadata: Some(GeminiUsage {
                prompt_token_count: 10,
                candidates_token_count: 5,
                total_token_count: 15,
            }),
        };
        let out = p.parse_response(resp, "gemini-1.5-pro");
        assert_eq!(out.content, "你好");
        assert_eq!(out.finish_reason, "length");
        assert_eq!(out.usage.total_tokens, 15);
        let tcs = out.tool_calls.unwrap();
        assert_eq!(tcs[0].function.name, "read_file");
        assert_eq!(tcs[0].function.arguments, r#"{"path":"a"}"#);
    }
}
