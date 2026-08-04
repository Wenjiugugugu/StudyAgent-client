//! AI Provider Module — 统一 AI Provider 接口
//!
//! 设计要点：
//! - `AiProvider` trait 定义统一接口，采用动态分发
//! - OpenAI Compatible 实现用 reqwest 调用 `/v1/chat/completions`
//! - AI Service 管理多个 provider，选择默认 provider，注入 system prompt

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod provider;
pub mod service;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use openai::OpenAIProvider;
pub use provider::*;
pub use service::AiService;
