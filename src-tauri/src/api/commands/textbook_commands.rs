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

/// 列出所有教材
///
/// 遍历 `assets/resources/textbooks/{subject}/` 目录下的 Markdown 文件。
/// 前端调用: `invoke('list_textbooks')`
#[tauri::command]
pub async fn list_textbooks(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TextbookInfo>, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    let mut result = Vec::new();

    // 遍历 textbooks 目录下的子目录（按学科分类）
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            for subject_dir in subject_dirs.flatten() {
                let subject = subject_dir.file_name().to_string_lossy().to_string();
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md") {
                            let filename = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let id = format!("{}-{}", subject, filename);
                            let title = filename.replace('-', " ");
                            result.push(TextbookInfo {
                                id,
                                subject: subject.clone(),
                                title,
                                filename: format!("{}.md", filename),
                                file_path: path.to_string_lossy().to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

/// 读取教材内容
///
/// 根据 `id`（格式 `subject-filename`）读取对应的 Markdown 教材文件。
/// 前端调用: `invoke('read_textbook', { id: 'math-高等数学' })`
#[tauri::command]
pub async fn read_textbook(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookContent, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // id 格式为 subject-filename，直接搜索所有文件匹配
    let mut found_path = None;
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            'outer: for subject_dir in subject_dirs.flatten() {
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        let filename = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let subject = subject_dir.file_name().to_string_lossy().to_string();
                        let file_id = format!("{}-{}", subject, filename);
                        if file_id == id {
                            found_path = Some(path);
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    let path = found_path.ok_or_else(|| format!("教材不存在: {}", id))?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取教材失败: {}", e))?;

    Ok(TextbookContent {
        id,
        content,
        file_path: path.to_string_lossy().to_string(),
    })
}

/// 导入教材文件
///
/// 将用户选择的 Markdown 文件复制到 textbooks/{subject}/ 目录下。
/// 安全限制（C4-a）：仅允许 `.md` 扩展名、文件不大于 50MB，
/// 且 subject 仅允许字母/数字/连字符，防止路径穿越与任意文件复制。
/// 前端调用: `invoke('import_textbook', { subject: 'math', filePath: 'C:/...', title: '同济线代' })`
#[tauri::command]
pub async fn import_textbook(
    subject: String,
    file_path: String,
    title: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookInfo, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 校验 subject，仅允许字母/数字/连字符/下划线，防止目录穿越
    if subject.is_empty()
        || !subject
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("无效的学科名称，仅允许字母、数字、连字符".to_string());
    }

    let subject_dir = textbooks_dir.join(&subject);

    // 创建学科目录
    std::fs::create_dir_all(&subject_dir).map_err(|e| format!("创建教材目录失败: {}", e))?;

    let src_path = std::path::Path::new(&file_path);

    // 仅允许 .md 扩展名（与 list/search 的 **/*.md 匹配逻辑一致）
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    if ext.as_deref() != Some("md") {
        return Err("仅支持导入 Markdown（.md）文件".to_string());
    }

    // 仅允许普通文件，且大小不超过 50MB
    let meta = std::fs::metadata(src_path).map_err(|e| format!("读取文件信息失败: {}", e))?;
    if !meta.is_file() {
        return Err("所选路径不是有效文件".to_string());
    }
    const MAX_TEXTBOOK_SIZE: u64 = 50 * 1024 * 1024;
    if meta.len() > MAX_TEXTBOOK_SIZE {
        return Err(format!(
            "文件过大（{:.1} MB），仅支持导入 50MB 以内的 Markdown 文件",
            meta.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let filename = src_path
        .file_stem()
        .ok_or_else(|| "无效的文件名".to_string())?
        .to_string_lossy()
        .to_string();

    let title = title.unwrap_or_else(|| filename.replace('-', " "));
    let dest_filename = format!("{}.md", filename);
    let dest_path = subject_dir.join(&dest_filename);

    // 复制文件
    std::fs::copy(src_path, &dest_path).map_err(|e| format!("复制教材文件失败: {}", e))?;

    let id = format!("{}-{}", subject, filename);

    Ok(TextbookInfo {
        id,
        subject,
        title,
        filename: dest_filename,
        file_path: dest_path.to_string_lossy().to_string(),
    })
}

/// 删除教材
///
/// 前端调用: `invoke('delete_textbook', { id: 'math-同济线代' })`
#[tauri::command]
pub async fn delete_textbook(id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 查找匹配的文件并删除
    if textbooks_dir.exists() {
        if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
            for subject_dir in subject_dirs.flatten() {
                if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                    for file in files.flatten() {
                        let path = file.path();
                        let filename = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let subject = subject_dir.file_name().to_string_lossy().to_string();
                        let file_id = format!("{}-{}", subject, filename);
                        if file_id == id {
                            std::fs::remove_file(&path)
                                .map_err(|e| format!("删除教材失败: {}", e))?;
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    Err(format!("教材不存在: {}", id))
}

/// 重命名教材
///
/// 修改教材文件的 stem（不含扩展名），id 与新名称均由前端提供。
/// 前端调用: `invoke('rename_textbook', { id: 'math-同济线代', newTitle: '线性代数' })`
#[tauri::command]
pub async fn rename_textbook(
    id: String,
    new_title: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<TextbookInfo, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 校验新标题：非空、不含路径分隔符与非法字符
    let trimmed = new_title.trim();
    if trimmed.is_empty() {
        return Err("教材标题不能为空".to_string());
    }
    if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || trimmed.contains('*')
        || trimmed.contains('?')
        || trimmed.contains('"')
        || trimmed.contains('<')
        || trimmed.contains('>')
        || trimmed.contains('|')
    {
        return Err("教材标题不能包含 / \\ : * ? \" < > | 等特殊字符".to_string());
    }

    // 查找原文件
    let (subject, old_stem, old_path) = {
        let mut found: Option<(String, String, std::path::PathBuf)> = None;
        if textbooks_dir.exists() {
            if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
                for subject_dir in subject_dirs.flatten() {
                    if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                        for file in files.flatten() {
                            let path = file.path();
                            let stem = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let subj = subject_dir.file_name().to_string_lossy().to_string();
                            if format!("{}-{}", subj, stem) == id {
                                found = Some((subj, stem, path));
                                break;
                            }
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
            }
        }
        found.ok_or_else(|| format!("教材不存在: {}", id))?
    };

    // 新文件名 stem：将标题中的空格替换为 '-' 以保持与现有 id 规则一致
    let new_stem = trimmed.replace(' ', "-");
    if new_stem == old_stem {
        // 无需重命名，直接返回当前信息
        return Ok(TextbookInfo {
            id: id.clone(),
            subject: subject.clone(),
            title: trimmed.to_string(),
            filename: format!("{}.md", new_stem),
            file_path: old_path.to_string_lossy().to_string(),
        });
    }

    let new_path = old_path
        .parent()
        .ok_or_else(|| "无法获取教材所在目录".to_string())?
        .join(format!("{}.md", new_stem));

    // 检查目标是否已存在
    if new_path.exists() {
        return Err(format!("已存在同名教材: {}", trimmed));
    }

    std::fs::rename(&old_path, &new_path).map_err(|e| format!("重命名教材失败: {}", e))?;

    Ok(TextbookInfo {
        id: format!("{}-{}", subject, new_stem),
        subject,
        title: trimmed.to_string(),
        filename: format!("{}.md", new_stem),
        file_path: new_path.to_string_lossy().to_string(),
    })
}

/// 在已导入教材中全文搜索
///
/// 前端调用: `invoke('search_in_textbook', { query: '二叉搜索树' })`
///
/// 用户输入通常是整句提问或整道题目，不能作为单一子串去精确匹配。
/// 因此先将查询拆解为关键词（中文按 2-gram、英文按单词、数字按连续串），
/// 过滤常见停用字后，逐词在教材中匹配并按命中关键词数量打分排序。
/// 同时解析「第N章 / 第N题」式章节引用做定向检索，命中章节标题与题目行
/// 可获得额外加权，确保用户只报章节/题号时也能定位到教材内容。
#[tauri::command]
#[allow(clippy::needless_range_loop)]
pub async fn search_in_textbook(
    query: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<TextbookSearchHit>, String> {
    let dir = get_data_dir(state.inner())?;
    let textbooks_dir = dir.join("assets").join("resources").join("textbooks");

    // 1. 解析章节 / 题号引用 + 拆解关键词
    let (chapter_ref, problem_ref) = parse_refs(&query);
    let terms = extract_search_terms(&query);

    let mut hits = Vec::new();

    if !textbooks_dir.exists() {
        return Ok(hits);
    }

    if let Ok(subject_dirs) = std::fs::read_dir(&textbooks_dir) {
        for subject_dir in subject_dirs.flatten() {
            let subject = subject_dir.file_name().to_string_lossy().to_string();
            if let Ok(files) = std::fs::read_dir(subject_dir.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    let filename = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let textbook_id = format!("{}-{}", subject, filename);
                    let textbook_title = filename.replace('-', " ");

                    let content = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let lines: Vec<&str> = content.lines().collect();
                    if lines.is_empty() {
                        continue;
                    }

                    // 2. 确定检索范围：若指定了章节，限定在该章标题到下一章之间
                    let (start, end) = match chapter_ref {
                        Some(n) => {
                            // 优先 Markdown 标题（`# 第N章`）
                            if let Some(ci) = find_chapter_line(&lines, n) {
                                let next = lines[ci + 1..]
                                    .iter()
                                    .position(|l| is_chapter_heading(l))
                                    .map(|p| ci + 1 + p)
                                    .unwrap_or(lines.len());
                                (ci, next)
                            }
                            // 扁平 OCR 文本（无 `#` 标题）：用 `N.x` 小节前缀定位章节范围
                            else if let Some((fs, fe)) = find_flat_chapter_range(&lines, n) {
                                (fs, fe)
                            }
                            // 完全找不到章节，退回全文
                            else {
                                (0, lines.len())
                            }
                        }
                        None => (0, lines.len()),
                    };

                    // 3. 逐行打分
                    // problem_ordinal 记录当前习题小节内第几道题（OCR 题号识别失败时的顺序兜底）
                    let mut in_exercise = false; // 是否处于习题区（仅在此区内计数题目序号）
                    let mut problem_ordinal = 0usize;
                    for idx in start..end {
                        let line = lines[idx];
                        let lower = line.to_lowercase();
                        let mut matched = 0usize;
                        let mut first_pos: Option<usize> = None;
                        let mut matched_terms: Vec<String> = Vec::new();

                        for term in &terms {
                            let tl = term.to_lowercase();
                            if lower.contains(&tl) {
                                matched += 1;
                                matched_terms.push(term.clone());
                                let pos = lower.find(&tl).unwrap_or(0);
                                if first_pos.map(|p| pos < p).unwrap_or(true) {
                                    first_pos = Some(pos);
                                }
                            }
                        }

                        // 章节标题：仅当查询本身含章节引用时作为定位锚点加权，
                        // 普通关键词查询不奖励标题，避免挤掉真正的内容匹配
                        if chapter_ref.is_some() && is_chapter_heading(line) {
                            matched += 2;
                        }
                        // 题号引用：OCR 容错匹配题号 + 顺序位置兜底
                        if let Some(target) = problem_ref {
                            // 习题区状态机：进入习题区才计数题目序号，
                            // 避免把正文里的列表序号（如 `3 第三代…`）误当题号导致顺序偏移
                            if is_exercise_start(&lower) {
                                in_exercise = true;
                                problem_ordinal = 0;
                            } else if is_answer_section(&lower) {
                                in_exercise = false;
                            }
                            if in_exercise {
                                let extracted = problem_number_of(line);
                                if extracted.is_some() {
                                    problem_ordinal += 1;
                                }
                                match extracted {
                                    // 精确命中题号（已做 OCR 噪音容错）
                                    Some(n) if n == target => matched += 5,
                                    // 题号被 OCR 认错时，用「本节第 N 道题」的序号兜底定位
                                    Some(_) if problem_ordinal == target as usize => matched += 3,
                                    _ => {}
                                }
                            }
                        }
                        // 单独报章节（无关键词）时，也要把该章标题带出来
                        if matched == 0
                            && chapter_ref.is_some()
                            && idx == start
                            && is_chapter_heading(line)
                        {
                            matched = 1;
                        }

                        if matched == 0 {
                            continue;
                        }

                        // 4. 截取首个命中位置的上下文（前后各 50 字符）
                        let ctx_start = first_pos.unwrap_or(0).saturating_sub(50);
                        let ctx_end = (ctx_start + 100).min(line.len());
                        let snippet = safe_char_slice(line, ctx_start, ctx_end);
                        let prefix = if ctx_start > 0 { "…" } else { "" };
                        let suffix = if ctx_end < line.len() { "…" } else { "" };
                        hits.push(TextbookSearchHit {
                            textbook_id: textbook_id.clone(),
                            textbook_title: textbook_title.clone(),
                            subject: subject.clone(),
                            line_number: idx + 1,
                            snippet: format!("{}{}{}", prefix, snippet, suffix),
                            hit_weight: matched,
                            matched_terms,
                        });
                    }
                }
            }
        }
    }

    // 5. 按命中关键词数量降序，保留最相关的若干条
    hits.sort_by(|a, b| {
        b.hit_weight
            .cmp(&a.hit_weight)
            .then(b.line_number.cmp(&a.line_number))
    });
    hits.truncate(20);

    Ok(hits)
}
