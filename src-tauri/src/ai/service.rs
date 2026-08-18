//! AI Service — 管理多个 Provider，选择默认 Provider，注入 System Prompt
//!
//! AiService 是 Core 层（Planner、Reviewer、Teacher）调用 AI 能力的统一入口。
//! 它管理多个 AIProvider 实例，根据 Agent 类型注入 system prompt，
//! 并提供 fallback 机制（默认 provider 失败时尝试备用 provider）。

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use super::provider::*;

/// AI Service — 统一 AI 服务管理器
pub struct AiService {
    /// Provider 实例缓存（按 provider ID 索引）
    providers: RwLock<HashMap<String, Arc<dyn AiProvider>>>,
    /// 默认 Provider ID
    default_provider_id: RwLock<String>,
    /// 活跃请求的取消令牌（按 agent 键索引）
    ///
    /// 前端通过 `cancel_ai_request` 命令触发取消，`chat`/`chat_stream`
    /// 用 `tokio::select!` 监听取消信号并提前返回，避免用户等待到超时。
    cancellations: Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>,
}

impl AiService {
    /// 创建空的 AI Service
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            default_provider_id: RwLock::new(String::new()),
            cancellations: Mutex::new(HashMap::new()),
        }
    }

    /// 从配置列表创建 AI Service
    pub fn from_configs(configs: Vec<AIProviderConfig>) -> Self {
        let service = Self::new();

        for config in configs {
            if config.enabled {
                let id = config.id.clone();
                let is_default = config.is_default;

                if let Ok(provider) = create_provider(config) {
                    service.providers.write().insert(id.clone(), provider);

                    if is_default {
                        *service.default_provider_id.write() = id.clone();
                    }
                }
            }
        }

        // 如果没有默认 provider，使用第一个
        let default_id = service.default_provider_id.read().clone();
        if default_id.is_empty() {
            let providers = service.providers.read();
            if let Some(first_id) = providers.keys().next() {
                *service.default_provider_id.write() = first_id.clone();
            }
        }

        service
    }

    /// 添加 Provider
    pub fn add_provider(&self, config: AIProviderConfig) -> Result<(), String> {
        let id = config.id.clone();
        let is_default = config.is_default;
        let enabled = config.enabled;

        if enabled {
            let provider = create_provider(config)?;
            self.providers.write().insert(id.clone(), provider);

            if is_default {
                self.set_default_provider(&id)?;
            }
        }

        Ok(())
    }

    /// 移除 Provider
    pub fn remove_provider(&self, provider_id: &str) {
        self.providers.write().remove(provider_id);

        let default_id = self.default_provider_id.read().clone();
        if default_id == provider_id {
            // 如果移除的是默认 provider，选择第一个可用的
            let providers = self.providers.read();
            if let Some(first_id) = providers.keys().next() {
                *self.default_provider_id.write() = first_id.clone();
            } else {
                self.default_provider_id.write().clear();
            }
        }
    }

    /// 设置默认 Provider
    pub fn set_default_provider(&self, provider_id: &str) -> Result<(), String> {
        let providers = self.providers.read();
        if !providers.contains_key(provider_id) {
            return Err(format!("Provider 不存在: {}", provider_id));
        }
        *self.default_provider_id.write() = provider_id.to_string();
        Ok(())
    }

    /// 获取默认 Provider ID
    pub fn default_provider_id(&self) -> String {
        self.default_provider_id.read().clone()
    }

    /// 获取默认 Provider
    fn get_default_provider(&self) -> Result<Arc<dyn AiProvider>, String> {
        let default_id = self.default_provider_id.read().clone();
        let providers = self.providers.read();

        if default_id.is_empty() {
            return Err("未配置任何 AI Provider".to_string());
        }

        providers
            .get(&default_id)
            .cloned()
            .ok_or_else(|| format!("默认 Provider 不存在: {}", default_id))
    }

    /// 根据 ID 获取 Provider
    pub fn get_provider(&self, provider_id: &str) -> Result<Arc<dyn AiProvider>, String> {
        let providers = self.providers.read();
        providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| format!("Provider 不存在: {}", provider_id))
    }

    /// 列出所有已注册的 Provider 配置
    pub fn list_providers(&self) -> Vec<AIProviderConfig> {
        let providers = self.providers.read();
        let default_id = self.default_provider_id.read();

        providers
            .values()
            .map(|p| {
                let mut config = p.config().clone();
                config.is_default = config.id == *default_id;
                config
            })
            .collect()
    }

    /// M31：是否存在备用 provider（已注册 provider 数量 > 1）
    ///
    /// 仅有一个 provider（即默认 provider 自身）时没有可切换的备用目标，
    /// 直接据此跳过 fallback 遍历，避免无意义的循环与困惑日志。
    fn has_backup_provider(&self) -> bool {
        self.providers.read().len() > 1
    }

    /// 取消指定 agent 键的进行中 AI 请求
    ///
    /// 返回是否找到了对应请求。取消后请求会以 `REQUEST_CANCELLED` 错误提前结束，
    /// 不会触发 fallback provider 切换。
    pub fn cancel_request(&self, key: &str) -> bool {
        let cancellations = self.cancellations.lock();
        match cancellations.get(key) {
            Some(tx) => {
                let _ = tx.send(true);
                log::info!("已请求取消 AI 请求（agent={}）", key);
                true
            }
            None => false,
        }
    }

    /// 根据请求的 agent 类型生成取消键
    fn cancel_key(req: &ChatRequest) -> String {
        req.agent
            .as_ref()
            .map(|a| format!("{:?}", a).to_lowercase())
            .unwrap_or_else(|| "assistant".to_string())
    }

    /// 聊天（非流式）
    ///
    /// 1. 根据 agent 类型注入 system prompt
    /// 2. 使用默认 provider 发送请求
    /// 3. 如果默认 provider 失败，尝试备用 provider
    /// 4. 记录 token 用量到持久化日志
    pub async fn chat(&self, mut req: ChatRequest) -> Result<ChatResponse, String> {
        // 注入 system prompt
        let agent_tag = req
            .agent
            .as_ref()
            .map(|a| format!("{:?}", a).to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        if let Some(agent) = req.agent.clone() {
            inject_system_prompt(&mut req, &agent);
        }

        // 注册取消令牌（按 agent 键），供前端 cancel_ai_request 触发
        let cancel_key = Self::cancel_key(&req);
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        self.cancellations
            .lock()
            .insert(cancel_key.clone(), cancel_tx);

        let default_provider = self.get_default_provider()?;
        let req_model = req
            .model
            .clone()
            .unwrap_or_else(|| default_provider.config().model.clone());
        let started = std::time::Instant::now();

        let result = if *cancel_rx.borrow() {
            Err(REQUEST_CANCELLED.to_string())
        } else {
            tokio::select! {
                r = default_provider.chat(&req) => r,
                _ = cancel_rx.changed() => Err(REQUEST_CANCELLED.to_string()),
            }
        };

        self.cancellations.lock().remove(&cancel_key);

        let result = match result {
            Ok(resp) => Ok(resp),
            Err(e) => {
                if e.contains(REQUEST_CANCELLED) {
                    // 用户主动取消：不切换 fallback provider
                    Err(e)
                } else {
                    log::warn!("默认 Provider 调用失败: {}", e);
                    // M31：仅有一个 provider（无备用）时，跳过 fallback 遍历，
                    // 直接返回主调用的原始错误，避免无意义的循环与困惑日志
                    if !self.has_backup_provider() {
                        log::debug!("[AI-DEBUG] 无备用 provider，跳过 fallback");
                        Err(e)
                    } else {
                        let fallback = self.fallback_chat(&req).await;
                        if fallback.is_err() {
                            return Err(format!("Provider 调用失败: {}", e));
                        }
                        fallback
                    }
                }
            }
        };

        let duration_ms = started.elapsed().as_millis() as u64;
        log_usage(&agent_tag, &result, duration_ms, &req_model);

        result
    }

    /// 聊天（流式）
    ///
    /// 通过回调函数返回每个 chunk
    /// 完成后记录 token 用量到持久化日志
    ///
    /// H9：默认 provider 失败时尝试备用 provider；
    /// 若已发出部分 chunk，先发送 reset 标记通知前端清空再切换。
    pub async fn chat_stream(
        &self,
        mut req: ChatRequest,
        on_chunk: impl Fn(ChatStreamChunk) + Send + Sync + 'static,
    ) -> Result<ChatResponse, String> {
        // 注入 system prompt
        let agent_tag = req
            .agent
            .as_ref()
            .map(|a| format!("{:?}", a).to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        if let Some(agent) = req.agent.clone() {
            inject_system_prompt(&mut req, &agent);
        }

        // 注册取消令牌（按 agent 键），供前端 cancel_ai_request 触发
        let cancel_key = Self::cancel_key(&req);
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        self.cancellations
            .lock()
            .insert(cancel_key.clone(), cancel_tx);

        let on_chunk_ref: &(dyn Fn(ChatStreamChunk) + Send + Sync) = &on_chunk;
        let started = std::time::Instant::now();

        let default_provider = self.get_default_provider()?;
        let req_model = req
            .model
            .clone()
            .unwrap_or_else(|| default_provider.config().model.clone());
        let default_id = self.default_provider_id.read().clone();

        let result = if *cancel_rx.borrow() {
            Err(REQUEST_CANCELLED.to_string())
        } else {
            tokio::select! {
                r = default_provider.chat_stream(&req, on_chunk_ref) => r,
                _ = cancel_rx.changed() => Err(REQUEST_CANCELLED.to_string()),
            }
        };

        let result = match result {
            Ok(resp) => Ok(resp),
            Err(e) if e.contains(REQUEST_CANCELLED) => Err(e),
            Err(e) => {
                log::warn!("默认 Provider 流式调用失败: {}", e);
                // 若默认 provider 已发出部分内容，先发 reset 标记通知前端清空，再切换
                on_chunk(ChatStreamChunk {
                    content: String::new(),
                    done: false,
                    tool_calls: None,
                    reset: true,
                    usage: None,
                });

                // M31：仅有一个 provider（无备用）时，跳过 fallback 遍历，
                // 直接返回主调用的原始错误，避免无意义的循环与困惑日志
                if !self.has_backup_provider() {
                    log::debug!("[AI-DEBUG] 无备用 provider，跳过 fallback");
                    Err(e)
                } else {
                    // 尝试备用 provider（带取消检测）
                    match self
                        .fallback_chat_stream(&req, on_chunk_ref, &mut cancel_rx)
                        .await
                    {
                        Ok(resp) => Ok(resp),
                        Err(fb_e) if fb_e.contains(REQUEST_CANCELLED) => Err(fb_e),
                        Err(_) => Err(format!("Provider 流式调用失败: {}", e)),
                    }
                }
            }
        };

        self.cancellations.lock().remove(&cancel_key);

        let duration_ms = started.elapsed().as_millis() as u64;
        log_usage(&agent_tag, &result, duration_ms, &req_model);

        result
    }

    /// Fallback 流式聊天：依次尝试备用 provider（跳过默认 provider）
    ///
    /// 与 `fallback_chat` 对应，供 `chat_stream` 在默认 provider 失败时使用。
    async fn fallback_chat_stream(
        &self,
        req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
        cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<ChatResponse, String> {
        let default_id = self.default_provider_id.read().clone();

        // 克隆出 provider 列表后立即释放读锁，避免在 `.await` 期间
        // 持有 `RwLockReadGuard`（该守卫不是 `Send`）。
        let providers: Vec<(String, Arc<dyn AiProvider>)> = {
            let providers = self.providers.read();
            providers
                .iter()
                .map(|(id, provider)| (id.clone(), provider.clone()))
                .collect()
        };

        for (id, provider) in &providers {
            if *id == default_id {
                continue; // 跳过已失败的默认 provider
            }
            log::info!("尝试备用 Provider 流式: {}", id);
            let r = if *cancel_rx.borrow() {
                Err(REQUEST_CANCELLED.to_string())
            } else {
                tokio::select! {
                    r = provider.chat_stream(req, on_chunk) => r,
                    _ = cancel_rx.changed() => Err(REQUEST_CANCELLED.to_string()),
                }
            };
            match r {
                Ok(resp) => return Ok(resp),
                Err(fb_e) if fb_e.contains(REQUEST_CANCELLED) => return Err(fb_e),
                Err(fb_e) => {
                    log::warn!("备用 Provider {} 流式调用失败: {}", id, fb_e);
                    continue;
                }
            }
        }

        Err("所有 Provider 均调用失败".to_string())
    }

    /// Fallback 聊天：尝试其他 provider
    async fn fallback_chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        let default_id = self.default_provider_id.read().clone();

        // 克隆出 provider 列表后立即释放读锁，避免在 `.await` 期间持有
        // `RwLockReadGuard`（该守卫不是 `Send`，会导致 future 不满足 `Send`）。
        // `Arc<dyn AiProvider>` 的克隆仅增加引用计数，开销很小。
        let providers: Vec<(String, Arc<dyn AiProvider>)> = {
            let providers = self.providers.read();
            providers
                .iter()
                .map(|(id, provider)| (id.clone(), provider.clone()))
                .collect()
        };

        for (id, provider) in &providers {
            if *id == default_id {
                continue; // 跳过已失败的默认 provider
            }

            log::info!("尝试备用 Provider: {}", id);
            match provider.chat(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    log::warn!("备用 Provider {} 调用失败: {}", id, e);
                    continue;
                }
            }
        }

        Err("所有 Provider 均调用失败".to_string())
    }

    /// 测试指定 Provider 配置
    ///
    /// 临时创建 Provider 实例并发送测试请求
    pub async fn test_provider(config: AIProviderConfig) -> Result<String, String> {
        let provider = create_provider(config)?;
        match provider.test_connection().await {
            Ok(mut msg) => {
                if msg.trim().is_empty() {
                    msg = "连接成功（模型返回空内容）".to_string();
                }
                Ok(msg)
            }
            Err(e) => Err(e),
        }
    }

    /// 获取 Provider 能力
    pub fn get_capabilities(&self, provider_id: Option<&str>) -> Result<ProviderCapabilities, String> {
        let provider = match provider_id {
            Some(id) => self.get_provider(id)?,
            None => self.get_default_provider()?,
        };
        Ok(provider.capabilities())
    }

    /// 检查是否已配置 Provider
    pub fn has_provider(&self) -> bool {
        !self.providers.read().is_empty()
    }

    /// 更新 Provider 配置
    ///
    /// 移除旧配置并添加新配置
    pub fn update_provider(&self, config: AIProviderConfig) -> Result<(), String> {
        let id = config.id.clone();
        self.remove_provider(&id);
        self.add_provider(config)
    }

    /// 从默认 Provider 获取可用模型列表
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, String> {
        let provider = self.get_default_provider()?;
        provider.list_models().await
    }

    /// 测试临时 Provider 配置并获取模型列表
    pub async fn test_list_models(config: AIProviderConfig) -> Result<Vec<ModelInfo>, String> {
        let provider = create_provider(config)?;
        provider.list_models().await
    }
}

impl Default for AiService {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// AI 用量日志辅助
// ============================================================================

/// 记录一次 AI 调用的 token 用量到持久化日志
fn log_usage(
    agent_tag: &str,
    result: &Result<ChatResponse, String>,
    duration_ms: u64,
    req_model: &str,
) {
    let (status, model, usage, error) = match result {
        Ok(resp) => (
            "success",
            resp.model.clone(),
            resp.usage.clone(),
            None,
        ),
        Err(e) => (
            "error",
            req_model.to_string(),
            TokenUsage::default(),
            Some(e.clone()),
        ),
    };

    let entry = crate::data::ai_usage::AiUsageEntry {
        timestamp: crate::data::now_string(),
        agent: agent_tag.to_string(),
        model,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        duration_ms,
        status: status.to_string(),
        error,
    };

    crate::data::ai_usage::append(entry);
}

// ============================================================================
// 测试用 Provider（不实际发送请求）
// ============================================================================

/// 创建测试用的 mock provider
pub fn create_mock_provider() -> Arc<dyn AiProvider> {
    Arc::new(MockProvider)
}

struct MockProvider;

#[async_trait::async_trait]
impl AiProvider for MockProvider {
    async fn chat(&self, _req: &ChatRequest) -> Result<ChatResponse, String> {
        Ok(ChatResponse {
            id: "mock-response".to_string(),
            model: "mock-model".to_string(),
            content: "这是一个模拟响应。请配置真实的 AI Provider。".to_string(),
            role: MessageRole::Assistant,
            tool_calls: None,
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            finish_reason: "stop".to_string(),
        })
    }

    async fn chat_stream(
        &self,
        _req: &ChatRequest,
        on_chunk: &(dyn Fn(ChatStreamChunk) + Send + Sync),
    ) -> Result<ChatResponse, String> {
        on_chunk(ChatStreamChunk {
            content: "这是一个模拟流式响应。".to_string(),
            done: false,
            tool_calls: None,
            reset: false,
            usage: None,
        });
        on_chunk(ChatStreamChunk {
            content: "请配置真实的 AI Provider。".to_string(),
            done: true,
            tool_calls: None,
            reset: false,
            usage: None,
        });

        Ok(ChatResponse {
            id: "mock-stream-response".to_string(),
            model: "mock-model".to_string(),
            content: "这是一个模拟流式响应。请配置真实的 AI Provider。".to_string(),
            role: MessageRole::Assistant,
            tool_calls: None,
            usage: TokenUsage::default(),
            finish_reason: "stop".to_string(),
        })
    }

    fn config(&self) -> &AIProviderConfig {
        use std::sync::OnceLock;
        static CONFIG: OnceLock<AIProviderConfig> = OnceLock::new();
        CONFIG.get_or_init(|| AIProviderConfig {
            id: "mock".to_string(),
            name: "Mock Provider".to_string(),
            r#type: ProviderType::Custom,
            base_url: "http://localhost".to_string(),
            api_key: String::new(),
            model: "mock-model".to_string(),
            fallback_model: None,
            timeout: 30,
            temperature: 0.7,
            max_tokens: None,
            enabled: true,
            is_default: false,
        })
    }
}
