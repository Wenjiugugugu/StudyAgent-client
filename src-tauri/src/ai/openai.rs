//! OpenAI Compatible Provider — 使用 reqwest 调用 `/v1/chat/completions`
//!
//! 支持 OpenAI、OpenRouter、SiliconFlow、DashScope、Volcengine、Ollama 等
//! 所有兼容 OpenAI API 格式的服务商。

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use std::time::Duration;
use tokio_stream::StreamExt;

use super::provider::*;

/// M26：AI 响应体大小上限（8MB）。
///
/// 防止异常超大响应在读取/解析时导致内存耗尽（OOM）。
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

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
            .unwrap_or_else(|e| {
                // M23：client 构建失败时明确记录，避免静默回退到无配置的默认 client
                log::error!(
                    "[AI-DEBUG] 自定义 reqwest client 构建失败，回退到默认 client（超时/连接池配置将不生效）: {}",
                    e
                );
                reqwest::Client::new()
            });

        let provider = Self { config, client };
        provider.warn_insecure_endpoint();
        provider
    }

    /// M29：检查 base_url 是否为非 localhost 的明文 HTTP（会明文传输 API Key）
    fn warn_insecure_endpoint(&self) {
        let base = self.config.base_url.trim();
        if !base.starts_with("http://") {
            return;
        }
        let host = base
            .trim_start_matches("http://")
            .split(['/', ':'])
            .next()
            .unwrap_or("");
        // localhost / 127.0.0.1 / ::1 等本地端点可接受（如 Ollama），远程 HTTP 需警告
        let is_local = host == "localhost"
            || host == "127.0.0.1"
            || host == "[::1]"
            || host == "::1"
            || host.starts_with("127.");
        if !is_local {
            log::warn!(
                "[AI-DEBUG] base_url 使用明文 HTTP 且非本地地址（{}），API Key 将以明文传输，存在被中间人截获的风险",
                base
            );
        }
    }

    /// 构建 API URL 的通用方法
    ///
    /// 根据 `base_url` 和给定后缀（如 `/chat/completions`、`/models`）构建完整 URL。
    /// 处理以下情况：
    /// - base_url 已包含后缀：直接使用（避免重复追加）
    /// - base_url 以版本路径结尾（/v1, /v2, /v3）：直接追加后缀
    /// - base_url 路径中包含版本段（/v1/, /v2/, /v3/）：直接追加后缀
    /// - 默认：追加 /v1 + 后缀（适用于仅提供域名的场景）
    fn build_url(&self, suffix: &str) -> String {
        let base = self.config.base_url.trim().trim_end_matches('/');

        // 如果 base 已以 suffix 结尾，直接使用（避免重复追加）
        if base.ends_with(suffix) {
            return base.to_string();
        }

        // 如果以版本路径结尾（/v1, /v2, /v3 等），直接追加 suffix
        // 匹配 /v1, /v2, /v3, /api/v3 等
        if base.ends_with("/v1") || base.ends_with("/v2") || base.ends_with("/v3") {
            return format!("{}{}", base, suffix);
        }

        // 如果路径中包含版本段（/v1/, /v2/, /v3/），直接追加 suffix
        if base.contains("/v1/") || base.contains("/v2/") || base.contains("/v3/") {
            return format!("{}{}", base, suffix);
        }

        // 默认追加 /v1 + suffix（适用于仅提供域名的场景）
        format!("{}/v1{}", base, suffix)
    }

    /// 构建 chat completions 请求 URL
    fn chat_completions_url(&self) -> String {
        self.build_url("/chat/completions")
    }

    /// 构建请求头
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !self.config.api_key.is_empty() {
            let auth = format!("Bearer {}", self.config.api_key);
            match HeaderValue::from_str(&auth) {
                Ok(val) => {
                    headers.insert(AUTHORIZATION, val);
                }
                Err(e) => {
                    // M22：HeaderValue 构造失败时明确记录错误，避免「静默无认证头发送」
                    log::error!(
                        "[AI-DEBUG] API Key 含非法字符，无法构造 Authorization 头: {}",
                        e
                    );
                }
            }
        }

        headers
    }

    /// 构建请求体 JSON（M30：`stream` 作为显式参数传入）
    ///
    /// 将 `stream` 提升为参数后，`chat`/`chat_stream` 就无需先 `req.clone()`
    /// 再修改 `stream` 字段，从而避免对整个 `ChatRequest`（含 `messages` 数组）
    /// 的无谓深拷贝——`messages` 只需在此被序列化进 JSON 一次。
    fn build_request_body(&self, req: &ChatRequest, stream: bool) -> serde_json::Value {
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
            "stream": stream,
        });

        if let Some(max_tokens) = req.max_tokens.or(self.config.max_tokens) {
            body["max_tokens"] = serde_json::Value::Number(max_tokens.into());
        }

        // M17：流式请求启用 usage 统计（OpenAI 协议在最后一个 chunk 返回 usage）
        if stream {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
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
        self.build_url("/models")
    }

    /// 解析 OpenAI API 非流式响应
    fn parse_response(&self, resp: OpenAIApiResponse) -> ChatResponse {
        let choice = resp.choices.first();
        let (content, role, tool_calls, finish_reason, reasoning) = match choice {
            Some(c) => {
                // DeepSeek 推理模型会把思考过程放在 reasoning_content，content 才是最终回复
                let mut content = c.message.content.clone().unwrap_or_default();
                // 思考过程单独保留，供调试记录展示（不覆盖最终回复）
                let mut reasoning: Option<String> = c.message.reasoning_content.clone();
                if content.is_empty() {
                    // 兜底：部分推理模型在非流式下可能把回复放在 reasoning_content
                    content = reasoning.clone().unwrap_or_default();
                }
                if reasoning.as_ref().map(String::is_empty).unwrap_or(true) {
                    reasoning = None;
                }
                let role = c.message.role.clone();
                let tool_calls = c.message.tool_calls.clone();
                let finish_reason = c
                    .finish_reason
                    .clone()
                    .unwrap_or_else(|| "stop".to_string());
                (content, role, tool_calls, finish_reason, reasoning)
            }
            None => (
                String::new(),
                MessageRole::Assistant,
                None,
                "stop".to_string(),
                None,
            ),
        };

        log::info!(
            "AI 响应: model={}, content_len={}, reasoning_len={}, finish_reason={}",
            resp.model,
            content.len(),
            reasoning.as_ref().map(String::len).unwrap_or(0),
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
            reasoning,
        }
    }

    /// 解析 SSE 流中的单个 data 行
    ///
    /// H7：反序列化失败时记录日志并检测 error 字段，不再静默丢弃
    /// 解析单条 SSE 行，并回填流式元数据（model / id / finish_reason）
    ///
    /// B2：OpenAI 协议在每个 chunk 的最外层携带 `model` 与 `id`（值恒定），
    /// 并在最后一个 chunk 的 choice.finish_reason 携带真实结束原因（stop/length/content_filter…）。
    /// 通过可变引用回填，避免对外部变量初始化值不变。
    fn parse_sse_line(
        &self,
        line: &str,
        finish_reason: &mut String,
        model_name: &mut String,
        response_id: &mut String,
    ) -> Option<ChatStreamChunk> {
        let line = line.trim();

        if line.is_empty() || line.starts_with(':') {
            return None;
        }

        if line.starts_with("event: ") {
            // H7：部分 provider 使用 SSE event 事件（如 event: error）
            let event = line.trim_start_matches("event: ").trim();
            if event == "error" {
                log::warn!("[AI-DEBUG] 收到 SSE error 事件");
            } else {
                log::debug!("[AI-DEBUG] SSE 事件: {}", event);
            }
            return None;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            let data = data.trim();

            if data == "[DONE]" {
                return Some(ChatStreamChunk {
                    content: String::new(),
                    done: true,
                    tool_calls: None,
                    reset: false,
                    usage: None,
                });
            }

            // 解析 JSON
            match serde_json::from_str::<OpenAIStreamChunk>(data) {
                Ok(chunk) => {
                    // B2：回填流式元数据——chunk 最外层携带的 model/id（值恒定，任取一次即可）
                    if !chunk.model.is_empty() {
                        *model_name = chunk.model.clone();
                    }
                    if !chunk.id.is_empty() {
                        *response_id = chunk.id.clone();
                    }

                    // M17：提取流式 usage（OpenAI 协议在最后一个 chunk 返回）
                    let usage = chunk.usage.map(|u| TokenUsage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                    });

                    let delta = chunk.choices.first();

                    if let Some(choice) = delta {
                        // B2：回填真实结束原因（仅最后一个 chunk 携带，其余为 None 不覆盖）
                        if let Some(fr) = &choice.finish_reason {
                            *finish_reason = fr.clone();
                        }
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
                            reset: false,
                            usage,
                        });
                    }

                    // 无 choices 但携带 usage 的结束 chunk（M17）
                    if let Some(u) = usage {
                        return Some(ChatStreamChunk {
                            content: String::new(),
                            done: true,
                            tool_calls: None,
                            reset: false,
                            usage: Some(u),
                        });
                    }
                }
                Err(_) => {
                    // H7：反序列化失败。尝试解析 error 字段，区分「错误」与「无法识别行」
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(err) = value.get("error") {
                            let msg = err.to_string();
                            log::warn!(
                                "[AI-DEBUG] 流式响应包含错误: {}",
                                msg.get(..msg.floor_char_boundary(200)).unwrap_or(&msg)
                            );
                        } else {
                            log::warn!(
                                "[AI-DEBUG] 无法解析的 SSE data 行: {}",
                                data.get(..data.floor_char_boundary(200)).unwrap_or(data)
                            );
                        }
                    } else {
                        log::warn!(
                            "[AI-DEBUG] 无法解析的 SSE data 行: {}",
                            data.get(..data.floor_char_boundary(200)).unwrap_or(data)
                        );
                    }
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

    /// M26：读取响应体并限制大小，防止异常超大响应导致 OOM
    ///
    /// 先检查 `Content-Length` 头，再对实际读取的 `Bytes` 长度做二次校验，
    /// 超过上限 `MAX_RESPONSE_BYTES` 时返回清晰错误并记录 warn，避免把超大
    /// body 读入内存。
    async fn read_response_limited(resp: reqwest::Response) -> Result<String, String> {
        if let Some(len) = resp.content_length() {
            if len > MAX_RESPONSE_BYTES as u64 {
                log::warn!(
                    "[AI-DEBUG] AI 响应体过大（Content-Length={} 字节，上限 {}），拒绝读取",
                    len,
                    MAX_RESPONSE_BYTES
                );
                return Err(format!(
                    "AI 响应体过大（{} 字节，上限 {} 字节）",
                    len, MAX_RESPONSE_BYTES
                ));
            }
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("读取 AI 响应失败: {}", e))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            log::warn!(
                "[AI-DEBUG] AI 响应体过大（实际 {} 字节，上限 {}），丢弃响应",
                bytes.len(),
                MAX_RESPONSE_BYTES
            );
            return Err(format!(
                "AI 响应体过大（{} 字节，上限 {} 字节）",
                bytes.len(),
                MAX_RESPONSE_BYTES
            ));
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAIProvider {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        // M30：不再全量 clone req（会深拷贝整份 messages）。
        // 非流式只需把 stream 强制为 false，直接把显式参数传给
        // build_request_body 即可，messages 仅被序列化一次。
        let url = self.chat_completions_url();
        log::info!("AI 请求 URL: {}", url);
        log::info!(
            "AI 请求 Model: {}",
            req.model
                .clone()
                .unwrap_or_else(|| self.config.model.clone())
        );
        let body = self.build_request_body(req, false);
        let headers = self.build_headers();

        // 调试日志：完整请求体（脱敏 API Key 后输出）。H11：降为 debug 级别避免生产噪音与敏感数据泄露
        log::debug!(
            "[AI-DEBUG] 非流式请求 body: {}",
            serde_json::to_string(&body).unwrap_or_default()
        );
        log::info!(
            "[AI-DEBUG] 请求消息数: {}, 工具数: {}",
            req.messages.len(),
            req.tools.as_ref().map(|t| t.len()).unwrap_or(0)
        );

        let response = self
            .send_with_retry(&url, headers, &body, req.timeout_override)
            .await?;

        let status = response.status();
        if !status.is_success() {
            // M26：错误响应体同样限制大小，避免异常超大 body 读入内存
            let error_text = Self::read_response_limited(response)
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            // H12：错误信息截断，避免完整 body（可能回显请求头/请求体）泄露到用户界面与日志
            let truncated = error_text.chars().take(500).collect::<String>();
            log::warn!(
                "[AI-DEBUG] AI 请求失败 status={} body={}",
                status,
                truncated
            );
            return Err(format!("AI 请求返回错误 ({}): {}", status, truncated));
        }

        // M26：读取响应体前先限制大小，防止异常超大响应导致 OOM
        let raw_text = Self::read_response_limited(response).await?;
        log::debug!(
            "[AI-DEBUG] 原始响应长度: {} 字节, 前 500 字符: {}",
            raw_text.len(),
            raw_text.chars().take(500).collect::<String>()
        );
        log::debug!("[AI-DEBUG] 原始响应全文: {}", raw_text);

        let api_resp: OpenAIApiResponse = serde_json::from_str(&raw_text).map_err(|e| {
            format!(
                "解析 AI 响应失败: {} | 原文(前200字符): {}",
                e,
                raw_text.chars().take(200).collect::<String>()
            )
        })?;

        Ok(self.parse_response(api_resp))
    }

    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
    ) -> Result<ChatResponse, String> {
        // M30：同上，流式只需把 stream 置为 true，直接传给 build_request_body，
        // 无需 clone 整份 messages。
        let url = self.chat_completions_url();
        log::info!("AI 流式请求 URL: {}", url);
        log::info!(
            "AI 流式请求 Model: {}",
            req.model
                .clone()
                .unwrap_or_else(|| self.config.model.clone())
        );
        let body = self.build_request_body(req, true);
        let headers = self.build_headers();

        // H11：流式请求 body 也降为 debug 级别
        log::debug!(
            "[AI-DEBUG] 流式请求 body: {}",
            serde_json::to_string(&body).unwrap_or_default()
        );

        let response = self
            .send_with_retry(&url, headers, &body, req.timeout_override)
            .await
            .map_err(|e| format!("发送 AI 流式请求失败: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            // M26：流式错误响应体同样限制大小
            let error_text = Self::read_response_limited(response)
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            // H12：错误信息截断，避免完整 body 泄露到用户界面与日志
            let truncated = error_text.chars().take(500).collect::<String>();
            log::warn!(
                "[AI-DEBUG] 流式请求失败 status={} body={}",
                status,
                truncated
            );
            return Err(format!("AI 流式请求返回错误 ({}): {}", status, truncated));
        }

        // 读取 SSE 流
        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason = "stop".to_string();
        let mut model_name = self.config.model.clone();
        let mut response_id = String::new();
        // M17：流式 usage 累积（最后一个 chunk 携带）
        let mut stream_usage = TokenUsage::default();

        let mut stream = response.bytes_stream();

        let mut buffer = String::new();
        let mut sse_line_count: usize = 0;

        // H8：收到 [DONE] 后通过标签跳出外层循环，避免继续消费流
        'stream: while let Some(chunk_result) = stream.next().await {
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

                if let Some(stream_chunk) =
                    self.parse_sse_line(line, &mut finish_reason, &mut model_name, &mut response_id)
                {
                    if !stream_chunk.content.is_empty() {
                        full_content.push_str(&stream_chunk.content);
                    }

                    if let Some(u) = &stream_chunk.usage {
                        stream_usage = u.clone();
                    }

                    if let Some(ref tc) = stream_chunk.tool_calls {
                        // C5：按 index 合并 tool_calls（OpenAI 规范），而非按 id
                        // （后续 delta 块可能不携带 id，仅 index + arguments）
                        for new_tc in tc {
                            let existing = tool_calls.iter_mut().find(|t| {
                                if new_tc.id.is_empty() {
                                    t.index == new_tc.index
                                } else {
                                    t.id == new_tc.id
                                }
                            });
                            match existing {
                                Some(t) => {
                                    // 首次增量块带 id/name，后续仅追加 arguments
                                    if !new_tc.id.is_empty() {
                                        t.id = new_tc.id.clone();
                                    }
                                    if !new_tc.function.name.is_empty() {
                                        t.function.name = new_tc.function.name.clone();
                                    }
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
                        // 流结束：跳出外层循环，不再从 stream 读取
                        break 'stream;
                    }
                }
            }
        }

        // 处理 buffer 中剩余的数据
        if !buffer.is_empty() {
            let line = buffer.trim();
            if !line.is_empty() {
                if let Some(stream_chunk) =
                    self.parse_sse_line(line, &mut finish_reason, &mut model_name, &mut response_id)
                {
                    if !stream_chunk.content.is_empty() {
                        full_content.push_str(&stream_chunk.content);
                    }
                    if let Some(u) = &stream_chunk.usage {
                        stream_usage = u.clone();
                    }
                    on_chunk(stream_chunk);
                }
            }
        }

        // 调试日志：流式响应汇总
        log::info!(
            "[AI-DEBUG] 流式响应完成: SSE 行数={}, content_len={}, tool_calls={}, finish_reason={}, prompt_tokens={}, completion_tokens={}",
            sse_line_count,
            full_content.len(),
            tool_calls.len(),
            finish_reason,
            stream_usage.prompt_tokens,
            stream_usage.completion_tokens
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
            // M17：使用流式累积的 usage，避免恒为 0
            usage: stream_usage,
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
            vision: false,
            // 优先按模型名查表，查不到再按 Provider 类型兜底
            max_context_length: crate::ai::provider::max_context_length_for(&self.config),
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
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let mut extra = m.clone();
                        // 服务商 /models 未提供上下文长度时，按模型名查内置表兜底
                        crate::ai::provider::inject_context_length_fallback(
                            &self.config.r#type,
                            &id,
                            &mut extra,
                        );
                        ModelInfo {
                            id,
                            owned_by: m
                                .get("owned_by")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            created: m.get("created").and_then(|v| v.as_i64()).unwrap_or(0),
                            extra,
                        }
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
    #[allow(dead_code)]
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
    /// M17：最后一个 chunk 携带 usage（需服务端支持 stream_options.include_usage）
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

/// OpenAI API 流式选择项
#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    #[allow(dead_code)]
    index: u32,
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

/// OpenAI API 流式 Delta
#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    #[serde(default)]
    #[allow(dead_code)]
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
