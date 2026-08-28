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

// ============================================================================
// 滴答清单（Dida365）Token
// ============================================================================

const DIDA_SERVICE_NAME: &str = "com.studyagent.desktop.dida";
const DIDA_TOKEN_ENTRY: &str = "token";

fn dida_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(DIDA_SERVICE_NAME, DIDA_TOKEN_ENTRY)
        .map_err(|error| format!("无法访问系统凭据库: {error}"))
}

/// 获取滴答 Token：优先系统凭据库，其次环境变量 DIDA_TOKEN
/// （兼容现有每日计划脚本 scripts/push_plan_to_dida.py 的环境变量用法）
pub fn get_dida_token() -> Option<String> {
    if let Ok(entry) = dida_entry() {
        if let Ok(value) = entry.get_password() {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    std::env::var("DIDA_TOKEN").ok().filter(|s| !s.is_empty())
}

pub fn set_dida_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Ok(());
    }
    dida_entry()?
        .set_password(token)
        .map_err(|error| format!("保存滴答 Token 到系统凭据库失败: {error}"))
}

pub fn delete_dida_token() -> Result<(), String> {
    match dida_entry() {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("删除滴答 Token 失败: {error}")),
        },
        Err(e) => Err(e),
    }
}
