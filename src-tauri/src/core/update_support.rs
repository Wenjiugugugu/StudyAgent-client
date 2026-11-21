//! 更新资源、校验和版本策略。
use serde::Deserialize;
/// 单个 Release 资源（一个安装包）
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateAsset {
    /// 文件名，如 `StudyAgent_0.1.2_x64-setup.exe`
    pub name: String,
    /// 直链下载地址
    pub download_url: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 资源类型推测：`nsis` / `msi` / `exe` / `unknown`
    pub kind: String,
    /// 文件 SHA-256（十六进制，来自 GitHub API 的 digest 字段）。
    ///
    /// 用于 `download_update` 下载完成后的完整性校验（L14）。
    /// 缺少有效摘要的资源会在返回前被过滤。
    #[serde(default)]
    pub sha256: Option<String>,
}

/// 检查更新结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheckResult {
    /// 是否有新版本
    pub has_update: bool,
    /// 当前版本（来自 Cargo.toml，形如 "0.1.2"）
    pub current_version: String,
    /// 远端最新版本号（剥离 v 前缀后的纯版本字符串）
    pub latest_version: String,
    /// Release 名称（标题，可能为空）
    pub release_name: String,
    /// 发布时间（ISO 8601 字符串，可能为空）
    pub published_at: String,
    /// Release notes（Markdown，可能为空）
    pub release_notes: String,
    /// 可下载的安装包列表（已过滤掉 .sig / .json / 签名等非安装包文件）
    pub assets: Vec<UpdateAsset>,
    /// 用户可读的提示信息（不包含技术细节）
    pub message: String,
    /// 是否强制更新（当前版本被远端策略清单禁用时为 true）
    #[serde(default)]
    pub force_update: bool,
    /// 强制更新原因（force_update=true 时展示给用户）
    #[serde(default)]
    pub force_update_reason: String,
}

/// 下载进度事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadProgress {
    /// 已下载字节
    pub downloaded: u64,
    /// 文件总字节（若服务端未返回 content-length 则为 0）
    pub total: u64,
    /// 进度百分比 0-100
    pub percent: f64,
}

/// GitHub API 端点：获取最新 release（兜底源）
pub(crate) const GITHUB_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Wenjiugugugu/StudyAgent-client/releases/latest";

/// GitCode API 端点：获取最新 release（国内加速源，优先使用）
///
/// GitCode 为国内部署，API / 附件下载 / raw 文件均走国内 CDN，
/// 响应字段（tag_name/name/body/assets[].browser_download_url）与 GitHub 兼容。
/// 注意 owner path 为小写 `wenjiugugugu`、仓库名为 `StudyAgent`（与 GitHub 的
/// `Wenjiugugugu/StudyAgent-client` 不同名）。
pub(crate) const GITCODE_RELEASES_LATEST_URL: &str =
    "https://api.gitcode.com/api/v5/repos/wenjiugugugu/StudyAgent/releases/latest";

/// 远端版本策略清单（仓库根目录维护，raw CDN 访问）。
///
/// 用于紧急禁用某版本：将该版本加入 blocked_versions 后，已更新到该版本
/// 的客户端启动时会进入强制更新模式（弹窗不可关闭，仅可更新或退出应用）。
/// 修改方式：直接编辑仓库该文件并 push，raw CDN 约 1–5 分钟刷新即生效。
/// 双源拉取：GitCode raw（国内快）优先，GitHub raw 兜底，任一端更新即生效。
pub(crate) const VERSION_POLICY_URL: &str =
    "https://raw.githubusercontent.com/Wenjiugugugu/StudyAgent-client/main/version-policy.json";

/// GitCode 版本策略清单（国内加速源，优先于 GitHub raw 拉取）。
///
/// 注：GitCode 的 raw 预览端点（raw.gitcode.com/.../raw/...）对 JSON 返回 403「暂不支持预览」，
/// 因此改用 contents API（与 Gitee v5 同源，匿名可读，返回 base64 编码的 content 字段）。
pub(crate) const GITCODE_VERSION_POLICY_URL: &str =
    "https://api.gitcode.com/api/v5/repos/wenjiugugugu/StudyAgent/contents/version-policy.json";

/// 推测资源类型
///
/// 安装程序现由 Inno Setup 生成（约定命名为 `*-setup.exe`）；
/// `nsis` 仅保留用于识别历史 release 里遗留的旧安装包。
pub(crate) fn detect_asset_kind(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with(".msi") {
        "msi".to_string()
    } else if lower.ends_with("-setup.exe") || lower.contains("inno") {
        "inno".to_string()
    } else if lower.contains("nsis") {
        "nsis".to_string()
    } else if lower.ends_with(".exe") {
        "exe".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 从 release assets 数组提取安装包列表（兼容 GitHub 与 GitCode）
///
/// - GitHub：assets 含 `digest`（"sha256:<hex>"）字段，直接提取；
/// - GitCode：assets 含 `type`（"attach" / "source"）字段但**无 digest**，
///   sha256 留空，由调用方通过同 release 的 checksums 附件补齐。
///
/// 均跳过 `.sig`、`.json`、`.txt`、`.blockmap` 等签名/manifest 文件与源码包。
/// 注意：这里不再按 sha256 过滤（GitCode 无 digest），由调用方在补齐后兜底过滤。
pub(crate) fn extract_install_assets(assets: &serde_json::Value) -> Vec<UpdateAsset> {
    let arr = match assets.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    arr.iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?.to_string();
            let lower = name.to_lowercase();

            // 跳过签名 / manifest / 源码包文件
            if lower.ends_with(".sig")
                || lower.ends_with(".json")
                || lower.ends_with(".txt")
                || lower.ends_with(".blockmap")
            {
                return None;
            }
            // GitCode 明确标记 `type: "source"` 的源码包直接跳过
            if asset.get("type").and_then(|v| v.as_str()) == Some("source") {
                return None;
            }

            let download_url = asset.get("browser_download_url")?.as_str()?.to_string();
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let kind = detect_asset_kind(&name);

            // GitHub API 对资产提供 digest 字段，形如 "sha256:<hex>"，剥离前缀；
            // GitCode 无该字段，此处保持 None，由调用方用 checksums 附件补齐。
            let sha256 = asset
                .get("digest")
                .and_then(|v| v.as_str())
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(|hex| hex.to_lowercase())
                .filter(|hex| !hex.is_empty());

            if kind == "unknown" {
                log::warn!("[Update] 忽略无法识别类型的安装资源: {}", name);
                return None;
            }

            Some(UpdateAsset {
                name,
                download_url,
                size,
                kind,
                sha256,
            })
        })
        .collect()
}

/// 从 release JSON 中定位并下载 checksums 附件（如 `StudyAgent-0.6.1-sha256.txt`），
/// 解析为 `文件名 -> SHA-256` 映射。
///
/// GitCode release API 的 assets 不提供 digest，完整性校验依赖发版时随附的
/// checksums 文件（sha256sum 格式：`<hex>  <filename>` 或 `<hex> *<filename>`）。
/// 任何失败（附件缺失 / 请求失败 / 格式不符）均返回空映射，不阻断主流程。
///
/// 注意：函数内部**自建独立 HTTP client**（30 秒超时），不复用调用方传入的
/// `check_for_updates` client——该 client 仅 15 秒短超时，GitCode checksums
/// 附件需 302 重定向到 `file-cdn.gitcode.com` 预签名 CDN，握手链路较长时
/// 容易在 15 秒内超时导致 SHA-256 补齐失败（曾造成应用内下载被拒绝）。
pub(crate) async fn fetch_checksums(
    release_json: &serde_json::Value,
) -> std::collections::HashMap<String, String> {
    let map = std::collections::HashMap::new();

    let Some(assets) = release_json.get("assets").and_then(|v| v.as_array()) else {
        return map;
    };

    // 定位 checksums 附件：文件名含 "sha256" 或 "checksum"，且以 .txt 结尾
    let Some(sum_asset) = assets.iter().find(|a| {
        a.get("name")
            .and_then(|v| v.as_str())
            .map(|n| {
                let lower = n.to_lowercase();
                (lower.contains("sha256") || lower.contains("checksum")) && lower.ends_with(".txt")
            })
            .unwrap_or(false)
    }) else {
        log::warn!("[Update] release 中未找到 checksums 附件（*sha256.txt），SHA-256 补齐跳过");
        return map;
    };

    let url = match sum_asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
    {
        Some(u) => u.to_string(),
        None => {
            log::warn!("[Update] checksums 附件缺少 browser_download_url，SHA-256 补齐跳过");
            return map;
        }
    };

    // 独立长超时 client：30s 总超时 + 15s 连接超时，覆盖 GitCode 302 → CDN 握手耗时
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("StudyAgent/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Update] 构造 checksums 下载 client 失败: {}", e);
            return map;
        }
    };

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[Update] checksums 附件下载请求失败: {}（{}）", e, url);
            return map;
        }
    };
    if !response.status().is_success() {
        log::warn!(
            "[Update] checksums 附件返回 HTTP {}（{}）",
            response.status(),
            url
        );
        return map;
    }
    const MAX_CHECKSUMS_BYTES: u64 = 1024 * 1024;
    if response.content_length().unwrap_or(0) > MAX_CHECKSUMS_BYTES {
        log::warn!("[Update] checksums 附件过大，已忽略");
        return map;
    }
    let text = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[Update] checksums 附件读取失败: {}", e);
            return map;
        }
    };

    let map = parse_checksums_text(&text);
    log::info!(
        "[Update] checksums 附件解析到 {} 条记录（{} 字节）",
        map.len(),
        text.len()
    );
    map
}

/// 解析 sha256sum 格式文本为 `文件名 -> SHA-256` 映射（纯函数，便于单测）。
///
/// 兼容三种行格式：`<hex>  <filename>`（双空格）、`<hex> *<filename>`（二进制）、
/// `<hex> <filename>`（单空格）；跳过空行与 `#` 注释行。
pub(crate) fn parse_checksums_text(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hex, name)) = line.split_once(' ') else {
            continue;
        };
        let hex = hex.trim();
        let name = name.trim_start_matches('*').trim();
        if !is_valid_sha256(hex) || name.is_empty() {
            continue;
        }
        map.insert(name.to_string(), hex.to_lowercase());
    }
    map
}

/// 构造一个「暂时无法检查」的结果（用于错误降级）
///
/// 错误原因仅写入日志，不暴露给前端 message。
pub(crate) fn unavailable_result(current_version: &str, log_reason: &str) -> UpdateCheckResult {
    log::warn!("[Update] 暂时无法检查更新，原因：{}", log_reason);
    UpdateCheckResult {
        has_update: false,
        current_version: current_version.to_string(),
        latest_version: String::new(),
        release_name: String::new(),
        published_at: String::new(),
        release_notes: String::new(),
        assets: Vec::new(),
        message: "暂时无法检查更新，请确认网络连接后重试".to_string(),
        force_update: false,
        force_update_reason: String::new(),
    }
}

/// 计算文件的 SHA-256（十六进制小写）
pub(crate) fn sha256_hex(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>())
}

pub(crate) fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 更新下载 URL 白名单（GitHub + GitCode 双源）
///
/// - `require_release_path=true`：下载起点必须是指定仓库的 release 直链路径；
///   两源路径模式一致（`/releases/download/{tag}/{file}`），但 owner/repo 不同名，
///   需按 host 分别校验前缀。
/// - `require_release_path=false`：重定向中间站 / 最终 CDN 域名白名单。
///   GitCode 的直链会 302 到 `file-cdn.gitcode.com` 的预签名 URL；
///   `api.gitcode.com` 的 `attach_files/.../download` 端点同为合法下载源。
pub(crate) fn is_allowed_update_url(url: &reqwest::Url, require_release_path: bool) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    // GitHub 与 GitCode 仓库名不同，各自维护 release 下载路径前缀
    const GITHUB_RELEASE_PATH: &str = "/Wenjiugugugu/StudyAgent-client/releases/download/";
    const GITCODE_RELEASE_PATH: &str = "/wenjiugugugu/StudyAgent/releases/download/";
    if require_release_path {
        (host == "github.com" && url.path().starts_with(GITHUB_RELEASE_PATH))
            || (host == "gitcode.com" && url.path().starts_with(GITCODE_RELEASE_PATH))
    } else {
        host == "github.com"
            || host == "objects.githubusercontent.com"
            || host == "release-assets.githubusercontent.com"
            || host == "gitcode.com"
            || host == "file-cdn.gitcode.com"
            || host == "api.gitcode.com"
    }
}

pub(crate) fn verified_updates(
) -> &'static std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, String>> {
    static VERIFIED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, String>>,
    > = std::sync::OnceLock::new();
    VERIFIED.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 比较语义化版本号：判断 `remote` 是否比 `current` 更新
///
/// 支持格式：`0.1.2`、`0.1.2-beta`、`0.1.2-beta.1`
/// 后缀（-beta、-rc.1、-alpha）视为预发布版本，比同版本号的正式版本更旧。
pub(crate) fn is_newer_version(remote: &str, current: &str) -> bool {
    let (remote_main, remote_pre) = split_version(remote);
    let (current_main, current_pre) = split_version(current);

    // 主版本号比较
    let remote_parts: Vec<u64> = remote_main
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    let current_parts: Vec<u64> = current_main
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();

    let max_len = remote_parts.len().max(current_parts.len());
    for i in 0..max_len {
        let r = remote_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if r != c {
            return r > c;
        }
    }

    // 主版本号相同：有预发布后缀的版本比无后缀的版本更旧
    match (remote_pre, current_pre) {
        (None, None) => false,    // 完全相同
        (Some(_), None) => false, // remote 是预发布，current 是正式版 → remote 更旧
        (None, Some(_)) => true,  // remote 是正式版，current 是预发布 → remote 更新
        (Some(r), Some(c)) => compare_prerelease(&r, &c) > 0,
    }
}

/// 拆分版本号：(主版本号, 预发布标识)
/// `"0.1.2"` -> `("0.1.2", None)`
/// `"0.1.2-beta"` -> `("0.1.2", Some("beta"))`
/// `"0.1.2-beta.1"` -> `("0.1.2", Some("beta.1"))`
pub(crate) fn split_version(v: &str) -> (String, Option<String>) {
    if let Some(idx) = v.find('-') {
        (v[..idx].to_string(), Some(v[idx + 1..].to_string()))
    } else {
        (v.to_string(), None)
    }
}

/// 比较两个预发布后缀：>0 表示 a 更新，==0 相同，<0 a 更旧
pub(crate) fn compare_prerelease(a: &str, b: &str) -> i32 {
    // 简单按字典序比较（足够覆盖 beta / rc / alpha）
    // 详见 https://semver.org/#spec-item-11
    a.cmp(b) as i32
}

/// 自动禁用规则：正式版发布后，旧版预发布（`-indev` 等）自动进入强制更新
///
/// 纯客户端规则（无需维护 version-policy.json，发布正式版即自动生效）：
/// 当前版本带预发布后缀（如 `0.6.1-indev`），且远端 latest 为正式版（无后缀），
/// 且正式版版本号 ≥ 当前版本去掉后缀后的基础版本号时，说明该 indev 所对应（或
/// 更早）的功能已随正式版发布，旧 indev 被自动禁用，强制升级到正式版。
///
/// 超前开发中的更高版本 indev（如 `0.7.0-indev` 之于已发布的正式版 `0.6.1`）
/// 不受影响；远端 latest 本身仍是预发布（正式版尚未发布）时也不触发。
///
/// 返回禁用原因文案（供 `force_update_reason` 展示）；不满足条件返回 `None`。
pub(crate) fn auto_block_prerelease(latest: &str, current: &str) -> Option<String> {
    let (current_main, current_pre) = split_version(current);
    let (latest_main, latest_pre) = split_version(latest);

    // 当前必须是预发布版本（-indev / -beta / -rc 等）
    current_pre.as_ref()?;
    // 远端 latest 必须是正式版（无预发布后缀）才触发自动禁用
    if latest_pre.is_some() {
        return None;
    }
    // 正式版版本号 < 当前基础版本号（超前开发）→ 不触发
    if is_newer_version(&current_main, &latest_main) {
        return None;
    }
    Some(format!(
        "当前版本 {} 为内部测试版本，正式版 {} 已发布，请更新到正式版后继续使用。",
        current, latest
    ))
}

// ============================================================================
// 版本策略（禁用版本清单）
// ============================================================================

/// 远端版本策略清单条目：一个被禁用的版本
#[derive(Debug, Clone, Deserialize)]
pub struct BlockedVersion {
    /// 被禁用的版本号（如 "0.6.1"，与 CARGO_PKG_VERSION 精确匹配）
    pub version: String,
    /// 展示给用户的原因文案
    pub reason: String,
    /// 禁用生效起始时间（ISO 8601，可选）；早于此时间不算禁用
    pub block_since: Option<String>,
    /// 建议升级到的最低安全版本（信息性，实际安装 latest）
    pub min_safe_version: Option<String>,
}

/// 远端版本策略清单
#[derive(Debug, Clone, Deserialize)]
pub struct VersionPolicy {
    /// 清单格式版本号
    pub schema_version: u32,
    /// 清单最后更新时间（ISO 8601），客户端用于判断本地缓存是否过期
    pub updated_at: String,
    /// 被禁用的版本列表
    pub blocked_versions: Vec<BlockedVersion>,
}

/// 按序尝试多个远端 JSON 端点，返回第一个成功的响应体与所用 URL。
///
/// 用于「GitCode 优先、GitHub 兜底」的双源更新检查。
/// GitHub 端点需携带 `Accept: application/vnd.github+json`，其余端点不附加。
pub(crate) async fn fetch_remote_json(
    client: &reqwest::Client,
    urls: &[&str],
) -> Option<(serde_json::Value, String)> {
    for url in urls {
        let mut req = client.get(*url);
        if url.contains("api.github.com") {
            req = req.header("Accept", "application/vnd.github+json");
        }
        match req.send().await {
            Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
                Ok(v) => {
                    log::info!("[Update] 远端源可用: {}", url);
                    return Some((v, (*url).to_string()));
                }
                Err(e) => log::warn!("[Update] JSON 解析失败({}): {}", url, e),
            },
            Ok(r) => log::warn!("[Update] 远端 {} 返回 HTTP {}", url, r.status()),
            Err(e) => log::warn!("[Update] 请求失败({}): {}", url, e),
        }
    }
    None
}

/// 从 release JSON 提取版本号（剥离 tag 的前导 v/V）
pub(crate) fn release_tag_version(release: &serde_json::Value) -> String {
    release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

/// 双源 latest release 择优（纯函数，便于测试）。
///
/// 规则：
/// - 仅一源成功 → 用该源；
/// - 两源都成功 → 比较 tag 版本取**更高**的一方；版本相同时优先 GitCode
///   （附件下载走国内 CDN，速度更快）。
///
/// 修复背景：此前检查更新「GitCode 成功即返回」，GitCode 源停留在旧版本
/// （如 0.7.0 未同步上传 GitCode、latest 停在 v0.6.1）时，0.6.1 / 0.7.0-indev
/// 客户端永远发现不了 GitHub 上已发布的新版本。
pub(crate) fn pick_newer_release(
    gitcode: Option<(serde_json::Value, String)>,
    github: Option<(serde_json::Value, String)>,
) -> Option<(serde_json::Value, String)> {
    match (gitcode, github) {
        (Some(g), None) => Some(g),
        (None, Some(h)) => Some(h),
        (Some(g), Some(h)) => {
            let gv = release_tag_version(&g.0);
            let hv = release_tag_version(&h.0);
            if is_newer_version(&hv, &gv) {
                log::info!(
                    "[Update] GitCode 源落后（{} < {}），采用 GitHub release",
                    gv,
                    hv
                );
                Some(h)
            } else {
                Some(g)
            }
        }
        (None, None) => None,
    }
}

/// 双源拉取 latest release 并择优（GitCode 优先、GitHub 兜底 → 版本更高者胜出）。
pub(crate) async fn fetch_best_release(
    client: &reqwest::Client,
) -> Option<(serde_json::Value, String)> {
    let gitcode = fetch_remote_json(client, &[GITCODE_RELEASES_LATEST_URL]).await;
    let github = fetch_remote_json(client, &[GITHUB_RELEASES_LATEST_URL]).await;
    pick_newer_release(gitcode, github)
}

/// 请求远端版本策略清单（双源：GitCode contents API 优先，GitHub raw 兜底）。
///
/// GitCode 端点返回 base64 编码的 `content` 字段，此处统一解码为原始文本后再解析。
/// 返回 `Ok(Some((policy, raw_text)))`；任何错误情况均返回 `Ok(None)`，
/// 不阻断主更新检查流程。`raw_text` 用于写入本地缓存。
pub(crate) async fn fetch_version_policy(
    client: &reqwest::Client,
) -> Result<Option<(VersionPolicy, String)>, String> {
    let mut last_err: Option<String> = None;
    for url in [GITCODE_VERSION_POLICY_URL, VERSION_POLICY_URL] {
        let response = match client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(format!("{}: {}", url, e));
                continue;
            }
        };

        if !response.status().is_success() {
            last_err = Some(format!("{}: HTTP {}", url, response.status()));
            continue;
        }

        let text = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                last_err = Some(format!("{}: 读取失败 {}", url, e));
                continue;
            }
        };

        // GitCode contents API 返回 { "content": "<base64>" }，需解码为原始 JSON 文本
        let raw_text = if let Ok(outer) = serde_json::from_str::<serde_json::Value>(&text) {
            match outer.get("content").and_then(|c| c.as_str()) {
                Some(encoded) => match decode_base64_text(encoded) {
                    Some(decoded) => decoded,
                    None => {
                        last_err = Some(format!("{}: base64 解码失败", url));
                        continue;
                    }
                },
                None => text,
            }
        } else {
            text
        };

        match serde_json::from_str::<VersionPolicy>(&raw_text) {
            Ok(p) => {
                log::info!("[Update] 版本策略获取成功: {}", url);
                return Ok(Some((p, raw_text)));
            }
            Err(e) => {
                last_err = Some(format!("{}: JSON 解析失败 {}", url, e));
            }
        }
    }

    if let Some(e) = last_err {
        log::warn!("[Update] 版本策略全部源失败: {}", e);
    }
    Ok(None)
}

/// 将 base64 字符串解码为 UTF-8 文本（用于 GitCode contents API 的 content 字段）
fn decode_base64_text(encoded: &str) -> Option<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// 判断 `current_version` 是否在策略清单中被禁用。
///
/// 精确匹配版本号；若条目带 `block_since`，则当前时间早于该时间不算禁用。
/// 返回命中的条目引用（供调用方取 reason）。
pub(crate) fn is_version_blocked<'a>(
    policy: &'a VersionPolicy,
    current_version: &str,
) -> Option<&'a BlockedVersion> {
    let now = chrono::Utc::now();
    policy.blocked_versions.iter().find(|b| {
        if b.version != current_version {
            return false;
        }
        // 校验 block_since（可选）：未到生效时间则不算禁用
        if let Some(since) = &b.block_since {
            if let Some(ts) = parse_iso8601(since) {
                if now < ts {
                    return false;
                }
            }
            // block_since 解析失败则忽略该约束（保守判定为已生效）
        }
        true
    })
}

/// 解析 ISO 8601 / RFC 3339 时间字符串为 UTC DateTime
fn parse_iso8601(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// 本地缓存文件路径：<data_dir>/version-policy.cache.json
pub(crate) fn cache_version_policy_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("version-policy.cache.json")
}

/// 读取本地缓存的策略；仅当 updated_at 在 7 天内才视为有效。
pub(crate) fn load_cached_policy(data_dir: &std::path::Path) -> Option<VersionPolicy> {
    let path = cache_version_policy_path(data_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let policy: VersionPolicy = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("[Update] 本地策略缓存解析失败: {}", e);
            return None;
        }
    };
    // 校验缓存有效期：updated_at 距今超过 7 天则弃用
    if let Some(updated) = parse_iso8601(&policy.updated_at) {
        let age = chrono::Utc::now().signed_duration_since(updated);
        if age.num_days() > 7 {
            log::info!("[Update] 本地策略缓存已过期（>7天），忽略");
            return None;
        }
    }
    Some(policy)
}

/// 原子写入本地策略缓存（先写 .tmp 再 rename，避免崩溃产生半截文件）。
pub(crate) fn save_cached_policy(data_dir: &std::path::Path, raw: &str) {
    let path = cache_version_policy_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

// ============================================================================
// 辅助函数
// ============================================================================
