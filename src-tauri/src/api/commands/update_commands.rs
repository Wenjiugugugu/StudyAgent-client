#![allow(unused_imports)]
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::analytics::{build_analytics, AnalyticsRange, AnalyticsSummary};
use crate::core::briefing::{yesterday_of, BriefingAgent};
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{UserCapability, UserObservation};
use crate::data::plan::{
    iso_week_string, DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment,
};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::tools::dispatcher::{execute_builtin_tool, is_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    get_ai_service, get_data_dir, get_data_dir_and_ai, get_data_dir_and_dispatcher,
    get_tool_dispatcher, load_settings, reinitialize_services, save_settings_file, AppSettings,
    AppState,
};

use super::legacy::*;

/// 检查更新
///
/// 双源获取最新 release：GitCode API（国内加速）优先，GitHub API 兜底。
/// 与当前版本比较。
/// **约定**：任何错误情况（网络错误、服务不可用、解析失败）
/// 一律返回 `has_update = false` + 友好的提示信息，详细错误仅写入日志。
#[tauri::command]
pub async fn check_for_updates(state: State<'_, Mutex<AppState>>) -> Result<UpdateCheckResult, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    log::info!("[Update] 开始检查更新：当前版本 {}", current_version);

    // 构造 HTTP 客户端（短超时，避免检查更新卡顿）
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("StudyAgent/{}", current_version))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Update] 构造 HTTP 客户端失败: {}", e);
            return Ok(unavailable_result(
                &current_version,
                &format!("client build failed: {}", e),
            ));
        }
    };

    // 请求 latest release：GitCode（国内加速）优先，GitHub 兜底
    let release_json = match fetch_remote_json(
        &client,
        &[GITCODE_RELEASES_LATEST_URL, GITHUB_RELEASES_LATEST_URL],
    )
    .await
    {
        Some((json, _used_url)) => json,
        None => {
            return Ok(unavailable_result(
                &current_version,
                "all release sources unavailable",
            ));
        }
    };

    // 提取 tag_name
    let tag_name = release_json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if tag_name.is_empty() {
        log::warn!("[Update] Release 不含 tag_name 字段");
        return Ok(unavailable_result(&current_version, "missing tag_name"));
    }

    // 剥离前导 'v' 或 'V'
    let latest_version = tag_name
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string();

    let has_update = is_newer_version(&latest_version, &current_version);
    log::info!(
        "[Update] 当前 {} | 远端 {} | has_update={}",
        current_version,
        latest_version,
        has_update
    );

    let release_name = release_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // GitHub 用 published_at，GitCode 用 created_at，兼容取其一
    let published_at = release_json
        .get("published_at")
        .and_then(|v| v.as_str())
        .or_else(|| release_json.get("created_at").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let release_notes = release_json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut assets = extract_install_assets(
        release_json
            .get("assets")
            .unwrap_or(&serde_json::Value::Null),
    );

    // GitCode 的 assets 无 digest 字段：从同 release 的 checksums 附件补齐 SHA-256
    if assets.iter().any(|a| a.sha256.is_none()) {
        let sums = fetch_checksums(&client, &release_json).await;
        if !sums.is_empty() {
            for asset in &mut assets {
                if asset.sha256.is_none() {
                    if let Some(hex) = sums.get(&asset.name) {
                        asset.sha256 = Some(hex.clone());
                    }
                }
            }
        }
    }

    // 兜底过滤：仍缺有效 SHA-256 的资源不可安全下载，从更新列表剔除
    assets.retain(|a| a.sha256.as_ref().is_some_and(|hex| is_valid_sha256(hex)));

    // ── 版本策略：判断当前版本是否被远端禁用 ──
    // 远端成功则写本地缓存；失败则读缓存兜底（缓存 updated_at 在 7 天内有效）。
    // 设计原则：有据才阻断，无据则放行，避免误锁离线用户。
    let data_dir = get_data_dir(state.inner()).ok();
    let (force_update, force_update_reason) = match fetch_version_policy(&client).await {
        Ok(Some((policy, raw))) => {
            if let Some(dir) = &data_dir {
                save_cached_policy(dir, &raw);
            }
            match is_version_blocked(&policy, &current_version) {
                Some(b) => (true, b.reason.clone()),
                None => (false, String::new()),
            }
        }
        _ => {
            // 远端失败：读本地缓存兜底
            match data_dir
                .as_ref()
                .and_then(|d| load_cached_policy(d))
                .and_then(|p| is_version_blocked(&p, &current_version).map(|b| (true, b.reason.clone())))
            {
                Some((fu, reason)) => (fu, reason),
                None => (false, String::new()),
            }
        }
    };

    // 被禁用的版本必须更新（即使远端 latest 与当前版本号相同也算有更新）
    let has_update = has_update || force_update;

    let message = if force_update {
        format!("当前版本 {} 存在已知问题，必须更新", current_version)
    } else if has_update {
        format!("发现新版本 {}（当前 {}）", latest_version, current_version)
    } else {
        format!("已是最新版本（{}）", current_version)
    };

    Ok(UpdateCheckResult {
        has_update,
        current_version,
        latest_version,
        release_name,
        published_at,
        release_notes,
        assets,
        message,
        force_update,
        force_update_reason,
    })
}

/// 下载更新
///
/// 流式下载安装包到临时目录，并通过 `update-download-progress` 事件
/// 推送下载进度（payload: `DownloadProgress`）。
///
/// 完整性校验：`expected_sha256` 必填，下载完成后及安装前都会计算文件
/// SHA-256 并比对，不匹配则删除文件并返回错误，防止安装被篡改的包。
/// 此外校验 `filename` 不含路径分隔符，防止路径穿越写出临时目录。
///
/// 下载完成后返回本地文件路径，供 `install_update` 使用。
#[tauri::command]
pub async fn download_update(
    url: String,
    filename: String,
    expected_sha256: Option<String>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    log::info!("[Update] 开始下载: {}", url);

    let parsed_url = reqwest::Url::parse(&url).map_err(|_| "无效的下载地址".to_string())?;
    if !is_allowed_update_url(&parsed_url, true) {
        return Err("仅允许从 StudyAgent 官方 Release（GitHub / GitCode）下载更新".to_string());
    }

    let expected = expected_sha256
        .map(|value| value.trim().to_lowercase())
        .filter(|value| is_valid_sha256(value))
        .ok_or_else(|| "该安装包缺少有效的 SHA-256，已拒绝下载".to_string())?;

    // 防御路径穿越：仅允许安全的 Windows 安装包文件名。
    let lower_filename = filename.to_lowercase();
    if filename.is_empty()
        || filename.contains(['/', '\\'])
        || !filename
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        || !(lower_filename.ends_with(".exe") || lower_filename.ends_with(".msi"))
    {
        return Err("无效的文件名".to_string());
    }

    // 临时目录：%TEMP%\StudyAgent-update\
    let temp_dir = std::env::temp_dir().join("StudyAgent-update");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let file_path = temp_dir.join(&filename);
    log::info!("[Update] 保存路径: {}", file_path.display());

    // 构造客户端（长超时，下载可能很大）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("更新下载重定向次数过多")
            } else if is_allowed_update_url(attempt.url(), false) {
                attempt.follow()
            } else {
                attempt.error("更新下载被重定向到不可信主机")
            }
        }))
        .user_agent(format!("StudyAgent/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化下载失败: {}", e))?;

    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("下载失败：服务返回 {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    const MAX_UPDATE_BYTES: u64 = 1024 * 1024 * 1024;
    if total > MAX_UPDATE_BYTES {
        return Err("安装包超过 1 GiB 安全上限".to_string());
    }
    log::info!("[Update] 文件大小: {} 字节", total);

    let mut file = std::fs::File::create(&file_path).map_err(|e| format!("创建文件失败: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    loop {
        let chunk = match response.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&file_path);
                return Err(format!("下载流读取失败: {}", e));
            }
        };

        if downloaded.saturating_add(chunk.len() as u64) > MAX_UPDATE_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&file_path);
            return Err("安装包超过 1 GiB 安全上限".to_string());
        }

        if let Err(error) = std::io::Write::write_all(&mut file, &chunk) {
            drop(file);
            let _ = std::fs::remove_file(&file_path);
            return Err(format!("写入文件失败: {error}"));
        }

        downloaded += chunk.len() as u64;

        // 每 256KB 推送一次进度，避免事件风暴
        if downloaded - last_emit >= 256 * 1024 || total > 0 && downloaded == total {
            let percent = if total > 0 {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            let _ = app.emit(
                "update-download-progress",
                DownloadProgress {
                    downloaded,
                    total,
                    percent,
                },
            );
            last_emit = downloaded;
        }
    }

    // 推送最终进度
    let _ = app.emit(
        "update-download-progress",
        DownloadProgress {
            downloaded,
            total,
            percent: if total > 0 { 100.0 } else { 0.0 },
        },
    );

    // 完整性校验：SHA-256 必填且必须匹配。
    drop(file);
    let actual = sha256_hex(&file_path).map_err(|e| format!("计算下载文件校验和失败: {}", e))?;
    if actual != expected {
        let _ = std::fs::remove_file(&file_path);
        return Err(
            "下载文件完整性校验失败（SHA-256 不匹配），已删除文件，请重试或稍后再更新".to_string(),
        );
    }
    let canonical =
        std::fs::canonicalize(&file_path).map_err(|e| format!("确认安装包路径失败: {e}"))?;
    verified_updates()
        .lock()
        .map_err(|_| "更新校验状态不可用".to_string())?
        .insert(canonical, expected);
    log::info!("[Update] 文件 SHA-256 校验通过");

    let path_str = file_path.to_string_lossy().to_string();
    log::info!("[Update] 下载完成: {} ({} 字节)", path_str, downloaded);
    Ok(path_str)
}

/// 安装更新
///
/// 启动下载好的安装包并退出当前应用。
/// Windows 上使用 DETACHED_PROCESS 让子进程独立运行。
#[tauri::command]
pub async fn install_update(file_path: String, app: tauri::AppHandle) -> Result<(), String> {
    let path = std::path::Path::new(&file_path);
    if !path.is_file() {
        return Err(format!("安装包不存在: {}", file_path));
    }

    let canonical = std::fs::canonicalize(path).map_err(|e| format!("确认安装包路径失败: {e}"))?;
    let canonical_temp = std::fs::canonicalize(std::env::temp_dir().join("StudyAgent-update"))
        .map_err(|e| format!("确认更新目录失败: {e}"))?;
    if !canonical.starts_with(&canonical_temp) {
        return Err("只能安装由 StudyAgent 下载并校验的更新包".to_string());
    }
    let expected = verified_updates()
        .lock()
        .map_err(|_| "更新校验状态不可用".to_string())?
        .remove(&canonical)
        .ok_or_else(|| "安装包未通过本次运行的完整性校验，请重新下载".to_string())?;
    let actual = sha256_hex(&canonical)?;
    if actual != expected {
        let _ = std::fs::remove_file(&canonical);
        return Err("安装前完整性复核失败，安装包已删除".to_string());
    }

    log::info!("[Update] 启动安装程序: {}", file_path);

    // Windows 上用 DETACHED_PROCESS 让子进程独立
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        let is_msi = canonical
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("msi"));
        let mut command = if is_msi {
            let mut command = std::process::Command::new("msiexec.exe");
            command.arg("/i").arg(&canonical);
            command
        } else {
            // 安装包由 Inno Setup 生成：静默覆盖安装。
            // /FORCECLOSEAPPLICATIONS 兜底关闭仍在运行的旧版本（本进程随后 exit），
            // 新版本由安装脚本在 ssPostInstall 阶段拉起，这里不再传 /RESTARTAPPLICATIONS
            // （否则可能出现「脚本启动 + Inno 重启」的双开）。
            let mut command = std::process::Command::new(&canonical);
            command.args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-",
                "/NOCANCEL",
                "/FORCECLOSEAPPLICATIONS",
            ]);
            command
        };
        command
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(&canonical)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    }

    log::info!("[Update] 安装程序已启动，应用退出");
    app.exit(0);
    Ok(())
}
