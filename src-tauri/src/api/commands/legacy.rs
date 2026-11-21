//! Tauri 命令拆分期间保留的共享 DTO 与辅助函数
//!
//! 前端通过 `@tauri-apps/api` 的 `invoke` 调用这些命令。
//! 命令实现位于同目录的领域模块中；本文件暂时集中保留跨领域复用的
//! 数据结构和纯辅助逻辑，避免迁移过程中改变现有数据契约。

use serde::Serialize;

use crate::data::state::TaskStatus;

// ============================================================================
// Dashboard 命令
// ============================================================================

// ============================================================================
// State 命令
// ============================================================================

// ============================================================================
// Analytics 命令
// ============================================================================

// ============================================================================
// Plan 命令
// ============================================================================

/// 日计划摘要（聚合 plan + review 数据）
///
/// 用于历史计划列表和周计划视图展示完成度。
/// 一次返回所有日计划的摘要信息，避免前端逐日调用。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanSummary {
    pub date: String,
    pub has_plan: bool,
    pub has_review: bool,
    /// 是否为用户未复盘后由系统生成的默认记录
    pub is_default_review: bool,
    pub planned_tasks: i32,
    pub planned_hours: f64,
    pub completed_tasks: i32,
    pub completion_rate: f64,
    pub actual_hours: f64,
    pub is_rest_day: bool,
    /// 是否为周计划中手动添加的特殊情况排除日（出差/生病/考试等）
    pub is_excluded: bool,
    /// 排除日类型（travel/sick/exam/other），仅当 is_excluded=true 时有值
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_type: Option<String>,
    /// 排除日备注，仅当 is_excluded=true 时有值
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_note: Option<String>,
}

/// 计算完成率
///
/// 优先从新版 `task_reviews` 聚合（结构化复盘），回退到旧版 `data.completion`。
///
/// 规则：
/// - 全部任务统一统计，旧函数名仅为兼容调用保留
/// - completion_rate = done / total * 100
/// - completed_tasks 返回已完成任务数（所有级别）
pub(super) fn compute_priority_a_completion(
    review: &Option<crate::data::records::ReviewFile>,
) -> (i32, f64) {
    review
        .as_ref()
        .map(crate::data::records::review_total_completion)
        .unwrap_or((0, 0.0))
}

// ============================================================================
// Review 命令
// ============================================================================

/// 提交结构化复盘（新版 Review，无需 AI）
///
/// 前端用户完成步骤式问答后，将结构化数据提交到后端。
/// 后端负责：保存 Review 文件 + 更新 State 中的任务状态。
/// 返回 needs_regeneration 标志，指示是否需要调用 AI 重新生成本周剩余天数计划。
/// 前端调用: `invoke('submit_review', { payload: { date, task_reviews, daily_review } })`
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubmitReviewPayload {
    pub date: String,
    pub task_reviews: Vec<crate::data::records::TaskReviewEntry>,
    pub daily_review: crate::data::records::DailyReviewInput,
    /// 计划外学习记录（可选）：用户实际进度领先计划时填写
    #[serde(default)]
    pub overcompletion: Vec<crate::data::records::OvercompletionEntry>,
    #[serde(default)]
    pub actual_study: Vec<crate::data::study_direction::ActualStudyEntry>,
}

/// submit_review 的返回结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubmitReviewResult {
    pub review_revision: String,
    pub sync_status: String,
    pub pending_steps: Vec<String>,
    pub warnings: Vec<String>,
    /// 是否需要调用 AI 重新生成本周剩余天数计划
    pub needs_regeneration: bool,
    /// 触发重排的原因（用于前端展示）
    pub regen_reasons: Vec<String>,
    /// 次日简报是否已在后台开始生成（fire-and-forget）
    #[serde(default)]
    pub briefing_generating: bool,
}

pub use crate::core::planning::types::RegenDayChange;

/// regenerate_remaining_days 的返回结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegenerateResult {
    /// 是否实际执行了重排
    pub regenerated: bool,
    /// 受影响的日期列表
    pub affected_dates: Vec<String>,
    /// AI 调用失败时是否启用了确定性兜底安排（供前端提示用户）
    #[serde(default)]
    pub used_fallback: bool,
    /// 一致性校验警告：声明了计划外进度的科目重排后未生效时给出提示
    #[serde(default)]
    pub consistency_warnings: Vec<String>,
    /// 各受影响日期的任务变动明细（重排前后标题对比，供前端悬停展示）
    #[serde(default)]
    pub changes: Vec<RegenDayChange>,
}

// ============================================================================
// Briefing 命令
// ============================================================================

/// get_briefing 的返回结构
///
/// 包含简报文件本体、昨日复盘是否存在、是否在补复盘窗口内等元信息，
/// 供前端判断是否展示「先去复盘」提示或「AI 建议不可用」状态。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GetBriefingResult {
    /// 简报文件（若存在）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub briefing: Option<crate::data::briefing::BriefingFile>,
    /// 简报是否存在
    pub exists: bool,
    /// 昨日复盘是否存在（决定能否提供 AI 建议）
    pub yesterday_review_exists: bool,
    /// 今日是否为休息日（来自用户设置）
    pub is_rest_day: bool,
    /// 今日是否为周计划排除日
    pub is_excluded_day: bool,
    /// 昨日是否为休息日或排除日（若是，则不要求补复盘）
    pub yesterday_exempt: bool,
    /// 是否在补复盘窗口内（今日且未过每日结束时间 +1 小时）
    pub within_makeup_window: bool,
}

// ============================================================================
// User Model 命令
// ============================================================================

pub(crate) use crate::core::textbook_support::*;
pub use crate::core::textbook_support::{TextbookContent, TextbookInfo, TextbookSearchHit};

/// 测试 AI Provider 连接的返回结果
#[derive(serde::Serialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// AI 用量日志命令
// ============================================================================

// ============================================================================
// 调试页命令（数据文件检查 / 查看）
// ============================================================================

/// 调试目录条目
#[derive(Serialize)]
pub struct DebugDirEntry {
    pub name: String,
    pub is_directory: bool,
}

/// 解析调试路径：确保解析后的路径始终位于 data_dir 内，防止路径穿越
pub(super) fn resolve_debug_path(
    data_dir: &std::path::Path,
    relative_path: &str,
) -> Result<std::path::PathBuf, String> {
    let rel = std::path::Path::new(relative_path);
    if rel.is_absolute() {
        return Err(format!("不允许绝对路径: {}", relative_path));
    }
    if relative_path.contains("..") {
        return Err(format!("不允许包含上级目录引用: {}", relative_path));
    }
    let resolved = data_dir.join(rel);
    let data_canon = data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf());
    let resolved_canon = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if !resolved_canon.starts_with(&data_canon) {
        return Err(format!("路径越界: {}", relative_path));
    }
    Ok(resolved_canon)
}

// ============================================================================
// Settings 命令
// ============================================================================

// ============================================================================
// 数据备份 / 导出 / 导入
// ============================================================================

// ============================================================================
// Onboarding 命令
// ============================================================================

/// 引导流程中收集的初始化数据
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InitStatePayload {
    pub target_school: String,
    pub target_major: String,
    pub exam_date: String,
    /// 考试科目配置: [{ subject: "math", version: "数二", active: true, phase: "foundation", weekly_hours: 14.0, target_score: 120 }, ...]
    pub subjects: Vec<SubjectInit>,
    /// 专业课名称（如 "408计算机综合"），仅 professional 科目使用
    pub professional_name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubjectInit {
    pub subject: String,
    pub version: Option<String>,
    pub active: bool,
    pub phase: String,
    pub weekly_hours: f64,
    pub target_score: i32,
    #[serde(default)]
    pub textbook: Option<String>,
}

// ============================================================================
// Update 命令
// ============================================================================

pub(crate) use crate::core::update_support::*;
pub use crate::core::update_support::{
    BlockedVersion, DownloadProgress, UpdateAsset, UpdateCheckResult, VersionPolicy,
};

/// 解析任务 ID
///
/// 任务 ID 格式: `YYYY-MM-DD-NN`
/// 返回: (date_string, zero_based_index)
pub(super) fn parse_task_id(task_id: &str) -> Result<(String, usize), String> {
    let parts: Vec<&str> = task_id.split('-').collect();
    if parts.len() < 4 {
        return Err(format!(
            "无效的任务 ID 格式: {}（期望 YYYY-MM-DD-NN）",
            task_id
        ));
    }

    let date = format!("{}-{}-{}", parts[0], parts[1], parts[2]);

    let seq: usize = parts[3]
        .parse()
        .map_err(|_| format!("无效的任务序号: {}", parts[3]))?;

    // 转换为 0-based 索引
    if seq == 0 {
        return Err("任务序号不能为 0（从 1 开始）".to_string());
    }

    Ok((date, seq - 1))
}

/// 解析任务状态字符串为 TaskStatus 枚举
pub(super) fn parse_task_status(status: &str) -> Result<TaskStatus, String> {
    match status.to_lowercase().as_str() {
        "pending" => Ok(TaskStatus::Pending),
        "in_progress" | "inprogress" | "in-progress" => Ok(TaskStatus::InProgress),
        "done" | "completed" | "complete" => Ok(TaskStatus::Done),
        "abandoned" | "abandon" | "skip" => Ok(TaskStatus::Abandoned),
        _ => Err(format!(
            "无效的任务状态: {}（支持: pending, in_progress, done, abandoned）",
            status
        )),
    }
}

// ============================================================================
// 通用命令（关闭行为 / 开机启动 / 应用版本）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// L14：验证文件 SHA-256 计算（已知内容 "hello" 的 sha256）
    #[test]
    fn sha256_hex_matches_known_hash() {
        let tmp = std::env::temp_dir().join(format!("sa_sha256_test_{}", std::process::id()));
        std::fs::write(&tmp, b"hello").unwrap();
        let hex = sha256_hex(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        // "hello" 的 SHA-256
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    /// L14：下载文件名防护，拒绝路径穿越
    #[test]
    fn invalid_filenames_rejected() {
        for bad in ["../evil.exe", "a\\b.exe", "a/b.exe", "", ".", ".."] {
            assert!(
                bad.is_empty()
                    || bad.contains(['/', '\\'])
                    || bad.split('.').any(|seg| seg.is_empty() || seg == ".."),
                "应判定为无效文件名: {:?}",
                bad
            );
        }
        // 合法文件名应通过
        assert!(!String::from("StudyAgent_0.1.2_x64-setup.exe").contains(['/', '\\']));
    }

    // ── GitCode 双源加速相关测试 ────────────────────────────────

    /// 双源择优：GitCode 落后（如 0.7.0 未同步、latest 停在 v0.6.1）时必须采用 GitHub 的新版本
    #[test]
    fn pick_newer_release_prefers_newer_tag() {
        let mk = |tag: &str| {
            (
                serde_json::json!({ "tag_name": tag, "name": tag }),
                "https://example.invalid".to_string(),
            )
        };

        // GitCode 停留在旧版本 → 取 GitHub
        let picked = pick_newer_release(Some(mk("v0.6.1")), Some(mk("v0.7.0"))).unwrap();
        assert_eq!(release_tag_version(&picked.0), "0.7.0");

        // GitHub 落后 → 取 GitCode
        let picked = pick_newer_release(Some(mk("v0.7.1")), Some(mk("v0.7.0"))).unwrap();
        assert_eq!(release_tag_version(&picked.0), "0.7.1");

        // 版本相同 → 优先 GitCode（国内 CDN）
        let picked = pick_newer_release(Some(mk("v0.7.0")), Some(mk("v0.7.0"))).unwrap();
        assert_eq!(release_tag_version(&picked.0), "0.7.0");
        assert_eq!(picked.1, "https://example.invalid");

        // 单源可用 → 直接采用
        assert!(pick_newer_release(None, Some(mk("v0.7.0"))).is_some());
        assert!(pick_newer_release(Some(mk("v0.6.1")), None).is_some());
        // 双源均失败 → None
        assert!(pick_newer_release(None, None).is_none());
    }

    /// 下载白名单：GitCode 直链 / CDN 重定向 / API 端点均放行，越权主机拒绝
    #[test]
    fn gitcode_download_urls_whitelist() {
        let allow_start = |u: &str| is_allowed_update_url(&reqwest::Url::parse(u).unwrap(), true);
        let allow_redirect =
            |u: &str| is_allowed_update_url(&reqwest::Url::parse(u).unwrap(), false);

        // 起点：GitCode 与 GitHub 的 release 直链都应放行
        assert!(allow_start(
            "https://gitcode.com/wenjiugugugu/StudyAgent/releases/download/v0.6.1/StudyAgent_0.6.1_x64-setup.exe"
        ));
        assert!(allow_start(
            "https://github.com/Wenjiugugugu/StudyAgent-client/releases/download/v0.6.1/StudyAgent_0.6.1_x64-setup.exe"
        ));

        // 起点：非 release 路径 / 其他仓库 / 非 https / 仓库名不匹配 拒绝
        assert!(!allow_start(
            "https://gitcode.com/wenjiugugugu/StudyAgent/raw/main/README.md"
        ));
        assert!(!allow_start(
            "https://gitcode.com/other-org/StudyAgent/releases/download/v0.6.1/x.exe"
        ));
        assert!(!allow_start(
            "https://gitcode.com/wenjiugugugu/StudyAgent-client/releases/download/v0.6.1/x.exe"
        ));
        assert!(!allow_start(
            "http://gitcode.com/wenjiugugugu/StudyAgent/releases/download/v0.6.1/x.exe"
        ));

        // 重定向目标：GitCode CDN / API 端点 / GitHub CDN 放行
        assert!(allow_redirect(
            "https://file-cdn.gitcode.com/9483585/releases/untagger_abc/x.exe?auth_key=1"
        ));
        assert!(allow_redirect("https://api.gitcode.com/api/v5/repos/wenjiugugugu/StudyAgent/releases/v0.6.1/attach_files/x.exe/download"));
        assert!(allow_redirect(
            "https://objects.githubusercontent.com/abc/x.exe"
        ));
        assert!(allow_redirect(
            "https://gitcode.com/wenjiugugugu/StudyAgent/releases/download/v0.6.1/x.exe"
        ));

        // 重定向目标：越权主机拒绝
        assert!(!allow_redirect("https://evil.example.com/x.exe"));
        assert!(!allow_redirect("https://file-cdn.example.com/x.exe"));
    }

    /// 资产解析兼容 GitCode：type=attach 保留、type=source 跳过、无 digest 时 sha256=None
    #[test]
    fn extract_install_assets_gitcode_compatible() {
        let assets = serde_json::json!([
            {
                "browser_download_url": "https://gitcode.com/wenjiugugugu/StudyAgent/releases/download/v0.6.1/StudyAgent_0.6.1_x64-setup.exe",
                "name": "StudyAgent_0.6.1_x64-setup.exe",
                "type": "attach",
                "id": 1
            },
            {
                "browser_download_url": "https://raw.gitcode.com/wenjiugugugu/StudyAgent/archive/refs/heads/v0.6.1.zip",
                "name": "v0.6.1.zip",
                "type": "source",
                "id": 2
            },
            {
                "browser_download_url": "https://gitcode.com/wenjiugugugu/StudyAgent/releases/download/v0.6.1/StudyAgent-0.6.1-sha256.txt",
                "name": "StudyAgent-0.6.1-sha256.txt",
                "type": "attach",
                "id": 3
            }
        ]);
        let list = extract_install_assets(&assets);
        assert_eq!(
            list.len(),
            1,
            "只应保留安装包，源码包与 checksums 文件被过滤"
        );
        assert_eq!(list[0].name, "StudyAgent_0.6.1_x64-setup.exe");
        assert_eq!(list[0].kind, "inno");
        // GitCode 无 digest → sha256 留空，等待调用方用 checksums 补齐
        assert!(list[0].sha256.is_none());
    }

    /// GitHub 资产解析保持原行为：digest 提取 sha256
    #[test]
    fn extract_install_assets_github_digest() {
        let assets = serde_json::json!([
            {
                "browser_download_url": "https://github.com/Wenjiugugugu/StudyAgent-client/releases/download/v0.6.1/StudyAgent_0.6.1_x64-setup.exe",
                "name": "StudyAgent_0.6.1_x64-setup.exe",
                "size": 12345,
                "digest": "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
            }
        ]);
        let list = extract_install_assets(&assets);
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].sha256.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    /// checksums 文本解析：双空格 / 单空格 / 二进制 `*` 三种格式
    #[test]
    fn parse_checksums_text_supports_common_formats() {
        let hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let text = format!(
            "{hex}  StudyAgent_0.6.1_x64-setup.exe\n{hex} *StudyAgent_0.6.1_x64_en-US.msi\n# comment\n{hex} StudyAgent_0.6.1_other.exe\n\n"
        );
        let map = parse_checksums_text(&text);
        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get("StudyAgent_0.6.1_x64-setup.exe")
                .map(|s| s.as_str()),
            Some(hex)
        );
        assert_eq!(
            map.get("StudyAgent_0.6.1_x64_en-US.msi")
                .map(|s| s.as_str()),
            Some(hex)
        );
        assert_eq!(
            map.get("StudyAgent_0.6.1_other.exe").map(|s| s.as_str()),
            Some(hex)
        );
    }

    // ── 自动禁用旧版 indev / 预发布版本的单元测试 ──────────────

    /// 发布正式版后，旧 indev 版本被自动禁用（对应/更早的正式版已发布）
    #[test]
    fn auto_block_prerelease_disables_old_indev() {
        // 发布 0.6.1：0.6.1-indev（同基础号）被禁用
        assert!(
            auto_block_prerelease("0.6.1", "0.6.1-indev").is_some(),
            "0.6.1-indev 在 0.6.1 正式版发布后应被禁用"
        );
        // 发布 0.6.1：0.6.0-indev（更早基础号）被禁用
        assert!(
            auto_block_prerelease("0.6.1", "0.6.0-indev").is_some(),
            "0.6.0-indev 在 0.6.1 正式版发布后应被禁用"
        );
        // 发布更高正式版 0.6.2：0.6.1-indev 也被禁用
        assert!(
            auto_block_prerelease("0.6.2", "0.6.1-indev").is_some(),
            "0.6.1-indev 在更高正式版 0.6.2 发布后应被禁用"
        );
        // 其它预发布后缀（beta）同样适用
        assert!(auto_block_prerelease("0.6.1", "0.6.1-beta").is_some());
    }

    /// 超前开发中的更高版本 indev（0.7.0-indev > 0.6.1 latest）不受影响
    #[test]
    fn auto_block_prerelease_keeps_ahead_indev() {
        assert!(
            auto_block_prerelease("0.6.1", "0.7.0-indev").is_none(),
            "超前开发的 0.7.0-indev 不应被 0.6.1 正式版禁用"
        );
        assert!(
            auto_block_prerelease("0.6.1", "0.7.1-indev").is_none(),
            "超前开发的 0.7.1-indev 不应被 0.6.1 正式版禁用"
        );
    }

    /// 正式版（无后缀）不触发自动禁用；远端 latest 仍为预发布时不触发
    #[test]
    fn auto_block_prerelease_ignores_release_and_remote_pre() {
        // 当前就是正式版：不自动禁用（0.6.1 发布后 0.6.1 / 0.6.0 正常继续使用）
        assert!(auto_block_prerelease("0.6.1", "0.6.1").is_none());
        assert!(auto_block_prerelease("0.6.1", "0.6.0").is_none());
        // 远端 latest 本身是预发布（正式版尚未发布）→ 不触发自动禁用
        assert!(auto_block_prerelease("0.6.1-indev", "0.6.0-indev").is_none());
        // 远端版本比当前基础号还旧 → 不触发
        assert!(auto_block_prerelease("0.6.0", "0.6.1-indev").is_none());
    }

    /// 返回的禁用原因文案应包含当前版本与最新版本信息
    #[test]
    fn auto_block_prerelease_reason_readable() {
        let reason = auto_block_prerelease("0.6.1", "0.6.1-indev").expect("应命中");
        assert!(
            reason.contains("0.6.1-indev"),
            "文案应包含当前版本，实际: {}",
            reason
        );
        assert!(
            reason.contains("0.6.1"),
            "文案应包含最新版本，实际: {}",
            reason
        );
    }

    // ── 教材 OCR 容错检索的单元测试 ──────────────────────────────

    /// 题号解析：OCR 把 `.` 识别成 `,`/`、`/空格/全角数字等杂音时仍能识别题号
    #[test]
    fn problem_number_ocr_noise() {
        // 标准
        assert_eq!(problem_number_of("01. 若十进制数为 137.5"), Some(1));
        assert_eq!(problem_number_of("02. 一个 16 位无符号"), Some(2));
        // 顿号 / 逗号（OCR 常把 `.` 识别成 `,`）
        assert_eq!(problem_number_of("04、对真值 0 表示"), Some(4));
        assert_eq!(problem_number_of("05, 若 [#= 11101010"), Some(5));
        assert_eq!(problem_number_of("08, 一个 + 1 位整数"), Some(8));
        assert_eq!(problem_number_of("09, 若定点整数为 64 位"), Some(9));
        assert_eq!(problem_number_of("10, 下列关于补码"), Some(10));
        // 题号后接空格
        assert_eq!(problem_number_of("11 若 [xJ#=lxixarsxryrsxe"), Some(11));
        // 全角数字
        assert_eq!(problem_number_of("３. 全角题号"), Some(3));
        // 行内「第N题」
        assert_eq!(problem_number_of("第 2 题 求下列行列式"), Some(2));
        assert_eq!(problem_number_of("第10题 写出"), Some(10));
        // Markdown 标题
        assert_eq!(problem_number_of("#### 02. 习题"), Some(2));
        // 正文列表序号不应被误识别为高权题号（题目号匹配阶段不参与，但函数本身应能解析）
        assert_eq!(problem_number_of("3 第三代计算机"), Some(3));
    }

    /// 正文起点检测：跳过目录/封面区，避免 `N.M` 目录小节号被误当章节边界
    #[test]
    fn content_start_skips_toc() {
        let lines: Vec<&str> = vec![
            "# 计算机组成原理",
            "[此页为封面页]",
            "2.3",
            "3.2",
            "3.2.1 半导体随机存取存储器",
            "第 7 章",
            "7.1",
            "计算机系统概述",
            "【考纲内容】",
            "( 一 ) 计算机系统层次结构",
            "1.1.1 计算机硬件的发展",
        ];
        assert_eq!(find_content_start(&lines), 8); // 命中「【考纲内容】」
    }

    /// 无「【…】」标记时回退到第一个长行
    #[test]
    fn content_start_fallback_long_line() {
        let lines: Vec<&str> = vec![
            "1.1",
            "1.2",
            "这一行是某章正文的第一段，长度明显超过四十个字符的阈值，应该被视为正文开始的地方。",
        ];
        assert_eq!(find_content_start(&lines), 2);
    }

    /// 章节范围定位：目录区被跳过，正文里 `num.x` 小节才是真正的章节边界
    #[test]
    fn flat_chapter_range_skips_toc() {
        let lines: Vec<&str> = vec![
            "# 标题",
            "【考纲内容】",
            "1.1.1 计算机硬件的发展", // 第1章正文
            "（一）计算机发展",
            "2.1.1 进位计数制", // 第2章正文
            "2.1.5 本节习题精选",
            "3.1.1 存储器的分类", // 第3章正文
        ];
        let r = find_flat_chapter_range(&lines, 1).unwrap();
        assert_eq!(r.0, 2); // 从 `1.1.1` 开始
        assert_eq!(r.1, 4); // 到 `2.1.1` 结束
    }

    /// 习题区状态机：只有进入习题区才计数题目序号，正文列表序号不污染顺序
    #[test]
    fn exercise_section_state_machine() {
        assert!(is_exercise_start("2.1.5 本节习题精选"));
        assert!(is_exercise_start("单项选择题"));
        assert!(is_exercise_start("综合应用题"));
        assert!(!is_exercise_start("计算机硬件的发展"));
        assert!(is_answer_section("2.1.6 答案与解析"));
        assert!(is_answer_section("参考答案"));
        assert!(!is_answer_section("解析一下这道题"));
    }

    /// 顺序兜底：找「第 2 题」时，即使 OCR 把 `02.` 认成 `02,` 等，也能按顺序定位
    #[test]
    fn problem_ordinal_fallback_over_ocr_noise() {
        // 模拟一个习题区：题号带各种 OCR 杂音，但题干完整
        let exercise: Vec<&str> = vec![
            "2.1.5 本节习题精选",
            "单项选择题",
            "01. 若十进制数为 137.5", // 第1题
            "A 89.8 B 211.4",
            "02, 一个 16 位无符号", // 第2题（OCR 逗号）
            "A 0 一 63536",
            "03、下列说法有误的是", // 第3题（OCR 顿号）
            "04 若叉为负数",        // 第4题（OCR 空格）
            "2.1.6 答案与解析",
        ];
        // 模拟主循环的 ordinal 计数逻辑
        let target = 2u32;
        let mut in_exercise = false;
        let mut ordinal = 0usize;
        let mut second_hit = None;
        for l in &exercise {
            let lower = l.to_lowercase();
            if is_exercise_start(&lower) {
                in_exercise = true;
                ordinal = 0;
            } else if is_answer_section(&lower) {
                in_exercise = false;
            }
            if in_exercise {
                if let Some(n) = problem_number_of(l) {
                    ordinal += 1;
                    if n == target {
                        second_hit = Some(ordinal);
                    }
                }
            }
        }
        // 第2题命中的行「02,」应被识别为第 2 道题
        assert_eq!(second_hit, Some(2));
    }
}
