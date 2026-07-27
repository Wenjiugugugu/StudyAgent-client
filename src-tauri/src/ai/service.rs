//! AI Service — 管理多个 Provider，选择默认 Provider，注入 System Prompt
//!
//! AiService 是 Core 层（Planner、Reviewer、Teacher）调用 AI 能力的统一入口。
//! 它管理多个 AIProvider 实例，根据 Agent 类型注入 system prompt，
//! 并提供 fallback 机制（默认 provider 失败时尝试备用 provider）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::provider::*;

/// AI Service — 统一 AI 服务管理器
pub struct AiService {
    /// Provider 实例缓存（按 provider ID 索引）
    providers: RwLock<HashMap<String, Arc<dyn AiProvider>>>,
    /// Provider 配置列表
    configs: RwLock<Vec<AIProviderConfig>>,
    /// 默认 Provider ID
    default_provider_id: RwLock<String>,
}

impl AiService {
    /// 创建空的 AI Service
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            configs: RwLock::new(Vec::new()),
            default_provider_id: RwLock::new(String::new()),
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
                    service.providers.write().unwrap().insert(id.clone(), provider);

                    if is_default {
                        *service.default_provider_id.write().unwrap() = id.clone();
                    }
                }
            }
        }

        // 如果没有默认 provider，使用第一个
        let default_id = service.default_provider_id.read().unwrap().clone();
        if default_id.is_empty() {
            let providers = service.providers.read().unwrap();
            if let Some(first_id) = providers.keys().next() {
                *service.default_provider_id.write().unwrap() = first_id.clone();
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
            self.providers.write().unwrap().insert(id.clone(), provider);

            if is_default {
                self.set_default_provider(&id)?;
            }
        }

        Ok(())
    }

    /// 移除 Provider
    pub fn remove_provider(&self, provider_id: &str) {
        self.providers.write().unwrap().remove(provider_id);

        let default_id = self.default_provider_id.read().unwrap().clone();
        if default_id == provider_id {
            // 如果移除的是默认 provider，选择第一个可用的
            let providers = self.providers.read().unwrap();
            if let Some(first_id) = providers.keys().next() {
                *self.default_provider_id.write().unwrap() = first_id.clone();
            } else {
                self.default_provider_id.write().unwrap().clear();
            }
        }
    }

    /// 设置默认 Provider
    pub fn set_default_provider(&self, provider_id: &str) -> Result<(), String> {
        let providers = self.providers.read().unwrap();
        if !providers.contains_key(provider_id) {
            return Err(format!("Provider 不存在: {}", provider_id));
        }
        *self.default_provider_id.write().unwrap() = provider_id.to_string();
        Ok(())
    }

    /// 获取默认 Provider ID
    pub fn default_provider_id(&self) -> String {
        self.default_provider_id.read().unwrap().clone()
    }

    /// 获取默认 Provider
    fn get_default_provider(&self) -> Result<Arc<dyn AiProvider>, String> {
        let default_id = self.default_provider_id.read().unwrap().clone();
        let providers = self.providers.read().unwrap();

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
        let providers = self.providers.read().unwrap();
        providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| format!("Provider 不存在: {}", provider_id))
    }

    /// 列出所有已注册的 Provider 配置
    pub fn list_providers(&self) -> Vec<AIProviderConfig> {
        let providers = self.providers.read().unwrap();
        let default_id = self.default_provider_id.read().unwrap();

        providers
            .values()
            .map(|p| {
                let mut config = p.config().clone();
                config.is_default = config.id == *default_id;
                config
            })
            .collect()
    }

    /// 聊天（非流式）
    ///
    /// 1. 根据 agent 类型注入 system prompt
    /// 2. 使用默认 provider 发送请求
    /// 3. 如果默认 provider 失败，尝试备用 provider
    pub async fn chat(&self, mut req: ChatRequest) -> Result<ChatResponse, String> {
        // 注入 system prompt
        if let Some(agent) = req.agent.clone() {
            inject_system_prompt(&mut req, &agent);
        }

        let default_provider = self.get_default_provider()?;

        match default_provider.chat(&req).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                log::warn!("默认 Provider 调用失败: {}", e);
                // 没有备用 provider 时直接返回原始错误，方便排查
                let fallback = self.fallback_chat(&req).await;
                if fallback.is_err() {
                    return Err(format!("Provider 调用失败: {}", e));
                }
                fallback
            }
        }
    }

    /// 聊天（流式）
    ///
    /// 通过回调函数返回每个 chunk
    pub async fn chat_stream(
        &self,
        mut req: ChatRequest,
        on_chunk: impl Fn(ChatStreamChunk) + Send + Sync + 'static,
    ) -> Result<ChatResponse, String> {
        // 注入 system prompt
        if let Some(agent) = req.agent.clone() {
            inject_system_prompt(&mut req, &agent);
        }

        let default_provider = self.get_default_provider()?;

        let on_chunk_ref: &(dyn Fn(ChatStreamChunk) + Send + Sync) = &on_chunk;

        match default_provider.chat_stream(&req, on_chunk_ref).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                log::warn!("默认 Provider 流式调用失败: {}", e);
                Err(e)
            }
        }
    }

    /// Fallback 聊天：尝试其他 provider
    async fn fallback_chat(&self, req: &ChatRequest) -> Result<ChatResponse, String> {
        let default_id = self.default_provider_id.read().unwrap().clone();

        // 克隆出 provider 列表后立即释放读锁，避免在 `.await` 期间持有
        // `std::sync::RwLockReadGuard`（该守卫不是 `Send`，会导致 future 不满足 `Send`）。
        // `Arc<dyn AiProvider>` 的克隆仅增加引用计数，开销很小。
        let providers: Vec<(String, Arc<dyn AiProvider>)> = {
            let providers = self.providers.read().unwrap();
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
        !self.providers.read().unwrap().is_empty()
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
        });
        on_chunk(ChatStreamChunk {
            content: "请配置真实的 AI Provider。".to_string(),
            done: true,
            tool_calls: None,
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
