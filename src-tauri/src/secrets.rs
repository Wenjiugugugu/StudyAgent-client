//! AI Provider 密钥的系统级安全存储。
//!
//! 配置文件只保存 Provider 元数据；API Key 使用 Windows Credential Manager。

const SERVICE_NAME: &str = "com.studyagent.desktop.ai-provider";
/// 仅用于前端表示“已有密钥”，不会作为真实凭据发送给 Provider。
pub const CONFIGURED_SENTINEL: &str = "__STUDYAGENT_KEY_CONFIGURED__";

fn entry(provider_id: &str) -> Result<keyring::Entry, String> {
    let id = provider_id.trim();
    if id.is_empty() || id.len() > 128 {
        return Err("Provider ID 无效".to_string());
    }
    keyring::Entry::new(SERVICE_NAME, id).map_err(|error| format!("无法访问系统凭据库: {error}"))
}

pub fn set_provider_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    if api_key.is_empty() {
        return Ok(());
    }
    entry(provider_id)?
        .set_password(api_key)
        .map_err(|error| format!("保存 API Key 到系统凭据库失败: {error}"))
}

pub fn get_provider_api_key(provider_id: &str) -> Result<Option<String>, String> {
    match entry(provider_id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取系统凭据库失败: {error}")),
    }
}

pub fn delete_provider_api_key(provider_id: &str) -> Result<(), String> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("删除系统凭据失败: {error}")),
    }
}
