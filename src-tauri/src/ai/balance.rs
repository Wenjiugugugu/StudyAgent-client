//! AI Provider 余额/用量查询（参考 cc-Switch「用量查询」的模板化设计）
//!
//! 查询模式选择逻辑：
//! 1. 按 Provider 类型识别：`openrouter` / `siliconflow` / `kimi` / `zhipu` / `minimax`；
//! 2. 按 `base_url` 域名识别：DeepSeek / Moonshot(Kimi) / OpenRouter / SiliconFlow /
//!    智谱(bigmodel.cn、z.ai) / MiniMax(minimaxi.com)；
//! 3. 未识别 → 通用端点链（对应 cc-Switch 的「通用模板」）：
//!    - `{origin}/dashboard/billing/credit_grants`（OpenAI 旧版赠费端点）
//!    - `{origin}/user/balance`（one-api / new-api 风格余额端点）
//!
//! 依次尝试，第一个能解析出余额的端点即命中。
//!
//! 注意：同一请求地址可能同时支持多种查询模式（套餐配额 vs 账户余额），
//! 本实现为免配置的自动推断，如某端点查询失败请以服务商控制台为准。

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::Value;

use super::provider::{AIProviderConfig, ProviderType};

/// 单次 HTTP 请求超时（对齐 cc-Switch 默认 10s）
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// 余额查询结果（统一数据模型，字段语义对齐 cc-Switch extractor 输出）
#[derive(Debug, Clone, Serialize)]
pub struct BalanceResult {
    pub success: bool,
    /// 命中的查询模板：openrouter / siliconflow / deepseek / moonshot / zhipu_quota /
    /// minimax_plan / credit_grants / general_balance
    pub mode: String,
    /// 剩余额度（套餐百分比模板下为剩余百分比）
    pub remaining: Option<f64>,
    /// 已使用额度
    pub used: Option<f64>,
    /// 总额度
    pub total: Option<f64>,
    /// 货币/单位（USD / CNY / % 等，未知为空字符串）
    pub unit: String,
    /// 套餐/条目名（如智谱套餐等级、MiniMax 模型名），无则空
    pub plan_name: String,
    /// 展示用消息（失败原因；成功时为空）
    pub message: String,
}

impl BalanceResult {
    #[allow(clippy::too_many_arguments)]
    fn success(
        mode: &str,
        remaining: Option<f64>,
        used: Option<f64>,
        total: Option<f64>,
        unit: &str,
        plan_name: &str,
    ) -> Self {
        Self {
            success: true,
            mode: mode.to_string(),
            remaining,
            used,
            total,
            unit: unit.to_string(),
            plan_name: plan_name.to_string(),
            message: String::new(),
        }
    }

    fn failure(message: String) -> Self {
        Self {
            success: false,
            mode: String::new(),
            remaining: None,
            used: None,
            total: None,
            unit: String::new(),
            plan_name: String::new(),
            message,
        }
    }
}

/// 查询模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryMode {
    OpenRouter,
    SiliconFlow,
    DeepSeek,
    Moonshot,
    /// 智谱 GLM 套餐用量（剩余百分比）
    Zhipu,
    /// MiniMax Token/Coding Plan 用量
    MiniMax,
    /// 通用端点链（OpenAI 兼容中转等）
    Generic,
}

/// 查询指定 Provider 的余额/用量。
///
/// `config.api_key` 必须已是真实 Key（命令层负责把哨兵值解析为凭据库中的明文）。
/// 始终返回 `BalanceResult`（查询失败时 `success: false`），不向上抛错，
/// 便于前端区分「网络失败」「端点不支持」「数据无法解析」等情形。
pub async fn query_balance(config: &AIProviderConfig) -> BalanceResult {
    if config.api_key.trim().is_empty() {
        return BalanceResult::failure("该 Provider 未配置 API Key，无法查询余额".to_string());
    }

    let client = match reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return BalanceResult::failure(format!("HTTP 客户端构建失败：{e}")),
    };

    let mode = detect_mode(config);
    let result = match mode {
        QueryMode::OpenRouter => query_openrouter(&client, config).await,
        QueryMode::SiliconFlow => query_siliconflow(&client, config).await,
        QueryMode::DeepSeek => query_deepseek(&client, config).await,
        QueryMode::Moonshot => query_moonshot(&client, config).await,
        QueryMode::Zhipu => query_zhipu(&client, config).await,
        QueryMode::MiniMax => query_minimax(&client, config).await,
        QueryMode::Generic => query_generic(&client, config).await,
    };

    match result {
        Ok(r) => r,
        Err(e) => BalanceResult::failure(e),
    }
}

/// 按 Provider 类型 / base_url 域名选择查询模式
fn detect_mode(config: &AIProviderConfig) -> QueryMode {
    match config.r#type {
        ProviderType::Openrouter => return QueryMode::OpenRouter,
        ProviderType::Siliconflow => return QueryMode::SiliconFlow,
        // Kimi 即月之暗面（Moonshot），同一余额端点
        ProviderType::Kimi => return QueryMode::Moonshot,
        ProviderType::Zhipu => return QueryMode::Zhipu,
        ProviderType::Minimax => return QueryMode::MiniMax,
        _ => {}
    }

    let host = host_of(&config.base_url).to_ascii_lowercase();
    if host.ends_with("deepseek.com") {
        return QueryMode::DeepSeek;
    }
    if host.ends_with("moonshot.cn") || host.ends_with("moonshot.ai") {
        return QueryMode::Moonshot;
    }
    if host.ends_with("openrouter.ai") {
        return QueryMode::OpenRouter;
    }
    if host.ends_with("siliconflow.cn") || host.ends_with("siliconflow.com") {
        return QueryMode::SiliconFlow;
    }
    if host.ends_with("bigmodel.cn") || host.ends_with("z.ai") {
        return QueryMode::Zhipu;
    }
    if host.ends_with("minimaxi.com") || host.ends_with("minimax.chat") {
        return QueryMode::MiniMax;
    }
    QueryMode::Generic
}

/// 从 base_url 提取 host（无 scheme/路径），解析失败返回空串
fn host_of(base_url: &str) -> String {
    base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split(['/', ':', '?'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// 从 base_url 提取 origin（scheme://host[:port]），异常时回退为去除尾斜杠的原值
fn origin_of(base_url: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(base_url.trim()) {
        if let Some(host) = url.host_str() {
            let scheme = url.scheme();
            let mut origin = format!("{scheme}://{host}");
            if let Some(port) = url.port() {
                origin.push_str(&format!(":{port}"));
            }
            return origin;
        }
    }
    base_url.trim().trim_end_matches('/').to_string()
}

fn bearer_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", api_key.trim())) {
        headers.insert(AUTHORIZATION, v);
    }
    headers
}

/// GET 请求并解析 JSON；非 2xx 时返回带状态码与响应片段的错误
async fn get_json(
    client: &reqwest::Client,
    url: &str,
    headers: &HeaderMap,
) -> Result<Value, String> {
    let resp = client
        .get(url)
        .headers(headers.clone())
        .send()
        .await
        .map_err(|e| format!("请求失败（{url}）：{e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败（{url}）：{e}"))?;

    if !status.is_success() {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!(
            "HTTP {}（{url}）：{}",
            status.as_u16(),
            snippet.trim()
        ));
    }

    serde_json::from_str(&body).map_err(|e| format!("响应不是有效 JSON（{url}）：{e}"))
}

/// 解析金额：支持数字与字符串（"112.00" / "CNY 12.34" 等混合货币前缀）
fn parse_money(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            for token in s.split_whitespace() {
                let t = token
                    .trim_start_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '+')
                    .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
                if let Ok(n) = t.parse::<f64>() {
                    return Some(n);
                }
            }
            None
        }
        _ => None,
    }
}

/// 从形如 "CNY 12.34" 的字符串中提取货币单位
fn extract_unit(v: Option<&Value>) -> String {
    if let Some(Value::String(s)) = v {
        if let Some(first) = s.split_whitespace().next() {
            let is_currency = !first.is_empty() && first.chars().all(|c| c.is_ascii_alphabetic());
            if is_currency {
                return first.to_ascii_uppercase();
            }
        }
    }
    String::new()
}

// ============================================================================
// 各查询模板实现
// ============================================================================

/// OpenRouter：GET {origin}/api/v1/credits（USD）
async fn query_openrouter(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let url = format!("{}/api/v1/credits", origin_of(&config.base_url));
    let headers = bearer_headers(&config.api_key);
    let v = get_json(client, &url, &headers).await?;
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());

    let total = parse_money(data.get("total_credits"));
    let used = parse_money(data.get("total_usage"));
    let remaining = match (total, used) {
        (Some(t), Some(u)) => Some(t - u),
        _ => None,
    };
    if remaining.is_none() && total.is_none() && used.is_none() {
        return Err("OpenRouter 响应中未找到额度字段（total_credits / total_usage）".to_string());
    }
    Ok(BalanceResult::success(
        "openrouter",
        remaining,
        used,
        total,
        "USD",
        "",
    ))
}

/// SiliconFlow：GET {origin}/v1/user/info（CNY，金额形如 "CNY 12.34"）
async fn query_siliconflow(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let url = format!("{}/v1/user/info", origin_of(&config.base_url));
    let headers = bearer_headers(&config.api_key);
    let v = get_json(client, &url, &headers).await?;
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());

    let balance_raw = data.get("balance");
    let remaining = parse_money(balance_raw);
    if remaining.is_none() {
        return Err("SiliconFlow 响应中未找到 balance 字段".to_string());
    }
    let charge = parse_money(data.get("chargeBalance"));
    let total = parse_money(data.get("totalBalance"));
    // 已用 = 充值额 - 当前余额（近似；赠费场景下可能偏差）
    let used = match (charge, remaining) {
        (Some(c), Some(r)) if c >= r => Some(c - r),
        _ => None,
    };
    let unit = extract_unit(balance_raw);
    let unit = if unit.is_empty() {
        "CNY".to_string()
    } else {
        unit
    };
    Ok(BalanceResult::success(
        "siliconflow",
        remaining,
        used,
        total,
        &unit,
        "",
    ))
}

/// DeepSeek：GET {origin}/user/balance
async fn query_deepseek(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let url = format!("{}/user/balance", origin_of(&config.base_url));
    let headers = bearer_headers(&config.api_key);
    let v = get_json(client, &url, &headers).await?;

    let info = v
        .get("balance_infos")
        .and_then(|infos| infos.get(0))
        .ok_or_else(|| "DeepSeek 响应中未找到 balance_infos".to_string())?;

    let remaining = parse_money(info.get("total_balance"));
    if remaining.is_none() {
        return Err("DeepSeek 响应中未找到 total_balance 字段".to_string());
    }
    let unit = extract_unit(info.get("currency"));
    Ok(BalanceResult::success(
        "deepseek", remaining, None, None, &unit, "",
    ))
}

/// 智谱 GLM 套餐用量：GET {origin}/api/monitor/usage/quota/limit
///
/// 响应形如 `{ success, msg, data: { level, limits: [{ type: "TOKENS_LIMIT",
/// percentage, nextResetTime }] } }`，percentage 为已用百分比。
/// 参考 cc-switch Discussion #1038 的查询脚本。Z.ai 域名同样适用。
async fn query_zhipu(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let url = format!(
        "{}/api/monitor/usage/quota/limit",
        origin_of(&config.base_url)
    );
    // 智谱该端点直接把 API Key 放入 Authorization（无 Bearer 前缀），Key 形如 "id.secret"
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(v) = HeaderValue::from_str(config.api_key.trim()) {
        headers.insert(AUTHORIZATION, v);
    }

    let v = get_json(client, &url, &headers).await?;
    if v.get("success") == Some(&Value::Bool(false)) {
        let msg = v
            .get("msg")
            .and_then(|m| m.as_str())
            .unwrap_or("智谱返回查询失败");
        return Err(format!("智谱查询失败：{msg}"));
    }
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let limits = data
        .get("limits")
        .and_then(|l| l.as_array())
        .ok_or_else(|| "智谱响应中未找到 limits 字段".to_string())?;

    let token_limit = limits
        .iter()
        .find(|item| item.get("type").and_then(|t| t.as_str()) == Some("TOKENS_LIMIT"))
        .ok_or_else(|| "智谱响应中未找到 TOKENS_LIMIT 项".to_string())?;

    // percentage 为已用百分比 → 剩余 = 100 - 已用
    let used_pct = parse_money(token_limit.get("percentage")).unwrap_or(0.0);
    let remaining = (100.0 - used_pct).clamp(0.0, 100.0);
    let plan_name = data
        .get("level")
        .and_then(|l| l.as_str())
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_else(|| "ZHIPU".to_string());
    Ok(BalanceResult::success(
        "zhipu_quota",
        Some(remaining),
        Some(used_pct),
        Some(100.0),
        "%",
        &plan_name,
    ))
}

/// MiniMax 用量：Coding Plan 与 Token Plan 是两条独立产品线、端点不通用
/// （这也是 cc-switch 默认模板对 MiniMax 失败的根因），故两条链依次尝试。
///
/// - Coding Plan：GET {origin}/v1/api/openplatform/coding_plan/remains
///   响应 `{ model_remains: [{ model_name, current_interval_total_count,
///   current_interval_usage_count }] }`（注意 usage_count 字段实际语义为「剩余」）
/// - Token Plan：GET {origin}/v1/token_plan/remains
async fn query_minimax(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let origin = origin_of(&config.base_url);
    let headers = bearer_headers(&config.api_key);

    // 1) Coding Plan
    let coding_url = format!("{origin}/v1/api/openplatform/coding_plan/remains");
    match get_json(client, &coding_url, &headers).await {
        Ok(v) => {
            if let Some(r) = parse_minimax_model_remains(&v) {
                return Ok(r);
            }
        }
        Err(e) => log::debug!("[BALANCE] MiniMax coding_plan/remains 查询失败：{e}"),
    }

    // 2) Token Plan
    let token_url = format!("{origin}/v1/token_plan/remains");
    match get_json(client, &token_url, &headers).await {
        Ok(v) => {
            if let Some(r) = parse_minimax_model_remains(&v) {
                return Ok(r);
            }
            if let Some(r) = parse_minimax_token_plan(&v) {
                return Ok(r);
            }
            Err("MiniMax 响应中未找到可识别的用量字段（model_remains / remains）".to_string())
        }
        Err(e) => Err(format!(
            "MiniMax 用量查询失败（已尝试 coding_plan 与 token_plan 端点）：{e}"
        )),
    }
}

/// 解析 MiniMax model_remains 数组：取第一个 total > 0 的条目
fn parse_minimax_model_remains(v: &Value) -> Option<BalanceResult> {
    let items = v.get("model_remains")?.as_array()?;
    let target = items
        .iter()
        .find(|item| parse_money(item.get("current_interval_total_count")).unwrap_or(0.0) > 0.0)?;

    let total = parse_money(target.get("current_interval_total_count"));
    // 字段名叫 usage_count，但按官方提取器语义实为「剩余」
    let remaining = parse_money(target.get("current_interval_usage_count"));
    let used = match (total, remaining) {
        (Some(t), Some(r)) if t >= r => Some(t - r),
        _ => None,
    };
    let plan_name = target
        .get("model_name")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    Some(BalanceResult::success(
        "minimax_plan",
        remaining,
        used,
        total,
        "次",
        &plan_name,
    ))
}

/// 解析 MiniMax Token Plan 端点的通用 remains 字段（官方文档未给出固定结构，尽力兼容）
fn parse_minimax_token_plan(v: &Value) -> Option<BalanceResult> {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let candidates = ["remains", "remaining", "remain", "balance"];
    for key in candidates {
        if let Some(remaining) = parse_money(data.get(key)) {
            let total =
                parse_money(data.get("total")).or_else(|| parse_money(data.get("total_count")));
            let used =
                parse_money(data.get("used")).or_else(|| parse_money(data.get("used_count")));
            return Some(BalanceResult::success(
                "minimax_plan",
                Some(remaining),
                used,
                total,
                "次",
                "",
            ));
        }
    }
    None
}

/// Moonshot(Kimi)：GET {origin}/v1/users/me/balance
async fn query_moonshot(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let url = format!("{}/v1/users/me/balance", origin_of(&config.base_url));
    let headers = bearer_headers(&config.api_key);
    let v = get_json(client, &url, &headers).await?;
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());

    let remaining = parse_money(data.get("available_balance"));
    if remaining.is_none() {
        return Err("Moonshot 响应中未找到 available_balance 字段".to_string());
    }
    let used = parse_money(data.get("total_used"));
    let total = parse_money(data.get("total_balance"));
    let unit = extract_unit(data.get("currency"));
    Ok(BalanceResult::success(
        "moonshot", remaining, used, total, &unit, "",
    ))
}

/// 通用端点链：credit_grants → {origin}/user/balance → {base}/user/balance
async fn query_generic(
    client: &reqwest::Client,
    config: &AIProviderConfig,
) -> Result<BalanceResult, String> {
    let origin = origin_of(&config.base_url);
    let base = config.base_url.trim().trim_end_matches('/');

    let mut attempts: Vec<(&str, String)> = vec![
        (
            "credit_grants",
            format!("{origin}/dashboard/billing/credit_grants"),
        ),
        ("general_balance", format!("{origin}/user/balance")),
    ];
    if base != origin && !base.is_empty() {
        attempts.push(("general_balance", format!("{base}/user/balance")));
    }

    let headers = bearer_headers(&config.api_key);
    let mut last_err = String::new();

    for (template, url) in attempts {
        match get_json(client, &url, &headers).await {
            Ok(v) => {
                let parsed = match template {
                    "credit_grants" => parse_credit_grants(&v),
                    _ => parse_general_balance(&v),
                };
                match parsed {
                    Some(r) => return Ok(r),
                    None => last_err = format!("端点 {url} 返回了无法识别的数据结构"),
                }
            }
            Err(e) => last_err = e,
        }
    }

    Err(format!(
        "未能查询到余额（已尝试 credit_grants 与 /user/balance 端点）：{last_err}"
    ))
}

/// OpenAI 旧版赠费端点：{ total_granted, total_used, total_available }（USD）
fn parse_credit_grants(v: &Value) -> Option<BalanceResult> {
    let total = parse_money(v.get("total_granted"));
    let used = parse_money(v.get("total_used"));
    let remaining = parse_money(v.get("total_available"));
    if remaining.is_none() && total.is_none() {
        return None;
    }
    Some(BalanceResult::success(
        "credit_grants",
        remaining,
        used,
        total,
        "USD",
        "",
    ))
}

/// one-api / new-api 风格：顶层或 data 下的 balance 字段
fn parse_general_balance(v: &Value) -> Option<BalanceResult> {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let remaining = parse_money(data.get("balance"));
    remaining?;
    let used = parse_money(data.get("used_amount"));
    let total = parse_money(data.get("total_balance"));
    let unit = extract_unit(data.get("currency"));
    Some(BalanceResult::success(
        "general_balance",
        remaining,
        used,
        total,
        &unit,
        "",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_money_handles_number_and_string() {
        assert_eq!(parse_money(Some(&Value::from(12.5))), Some(12.5));
        assert_eq!(parse_money(Some(&Value::from("112.00"))), Some(112.0));
        assert_eq!(parse_money(Some(&Value::from("CNY 12.34"))), Some(12.34));
        assert_eq!(parse_money(None), None);
    }

    #[test]
    fn extract_unit_detects_currency_prefix() {
        assert_eq!(extract_unit(Some(&Value::from("CNY 12.34"))), "CNY");
        assert_eq!(extract_unit(Some(&Value::from("12.34"))), "");
    }

    #[test]
    fn origin_of_extracts_scheme_and_host() {
        assert_eq!(
            origin_of("https://api.deepseek.com/v1"),
            "https://api.deepseek.com"
        );
        assert_eq!(
            origin_of("http://localhost:8080/v1/"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn detect_mode_by_host_and_type() {
        let mut cfg = AIProviderConfig {
            base_url: "https://api.deepseek.com".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_mode(&cfg), QueryMode::DeepSeek);

        cfg.r#type = ProviderType::Openrouter;
        cfg.base_url = "https://example.com/v1".to_string();
        assert_eq!(detect_mode(&cfg), QueryMode::OpenRouter);

        cfg.r#type = ProviderType::Openai;
        cfg.base_url = "https://relay.example.com/v1".to_string();
        assert_eq!(detect_mode(&cfg), QueryMode::Generic);
    }

    #[test]
    fn detect_mode_for_new_provider_types() {
        let mut cfg = AIProviderConfig {
            r#type: ProviderType::Kimi,
            base_url: "https://api.moonshot.cn/v1".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_mode(&cfg), QueryMode::Moonshot);

        cfg.r#type = ProviderType::Zhipu;
        cfg.base_url = "https://open.bigmodel.cn/api/paas/v4".to_string();
        assert_eq!(detect_mode(&cfg), QueryMode::Zhipu);

        cfg.r#type = ProviderType::Minimax;
        cfg.base_url = "https://www.minimaxi.com/v1".to_string();
        assert_eq!(detect_mode(&cfg), QueryMode::MiniMax);

        // 域名兜底：类型为 openai 但指向智谱域名
        cfg.r#type = ProviderType::Openai;
        cfg.base_url = "https://api.z.ai/v1".to_string();
        assert_eq!(detect_mode(&cfg), QueryMode::Zhipu);
    }

    #[test]
    fn parse_minimax_model_remains_picks_valid_entry() {
        let v: Value = serde_json::json!({
            "model_remains": [
                { "model_name": "MiniMax-M2", "current_interval_total_count": 1000, "current_interval_usage_count": 400 },
                { "model_name": "MiniMax-M1", "current_interval_total_count": 0, "current_interval_usage_count": 0 }
            ]
        });
        let r = parse_minimax_model_remains(&v).expect("should parse");
        assert_eq!(r.remaining, Some(400.0));
        assert_eq!(r.total, Some(1000.0));
        assert_eq!(r.used, Some(600.0));
        assert_eq!(r.plan_name, "MiniMax-M2");
    }

    #[test]
    fn parse_zhipu_quota_percent() {
        let v: Value = serde_json::json!({
            "success": true,
            "data": {
                "level": "glm",
                "limits": [ { "type": "TOKENS_LIMIT", "percentage": 42 } ]
            }
        });
        let data = v.get("data").unwrap();
        let limits = data.get("limits").unwrap().as_array().unwrap();
        let token_limit = limits
            .iter()
            .find(|item| item.get("type").and_then(|t| t.as_str()) == Some("TOKENS_LIMIT"))
            .unwrap();
        let used_pct = parse_money(token_limit.get("percentage")).unwrap();
        assert_eq!(used_pct, 42.0);
        assert_eq!(100.0 - used_pct, 58.0);
    }
}
