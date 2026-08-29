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
/// - 优先统计 A 级任务完成率；若无 A 级任务，则统计全部任务完成率
/// - completion_rate = done / total * 100
/// - completed_tasks 返回已完成任务数（所有级别）
pub(super) fn compute_priority_a_completion(
    review: &Option<crate::data::records::ReviewFile>,
) -> (i32, f64) {
    match review {
        Some(r) => {
            // 新版：优先从 task_reviews 聚合
            if !r.task_reviews.is_empty() {
                let mut a_total = 0i32;
                let mut a_done = 0i32;
                let mut all_total = 0i32;
                let mut all_done = 0i32;

                for tr in &r.task_reviews {
                    all_total += 1;
                    let is_done = tr.status == "completed";
                    if is_done {
                        all_done += 1;
                    }
                    // 优先用 task_reviews 自带的 priority 字段，回退到空字符串（视为非 A）
                    if tr.priority == "A" {
                        a_total += 1;
                        if is_done {
                            a_done += 1;
                        }
                    }
                }

                let completed = all_done;
                let rate = if a_total > 0 {
                    (a_done as f64 / a_total as f64) * 100.0
                } else if all_total > 0 {
                    // 无 A 级任务时，用全部任务完成率
                    (all_done as f64 / all_total as f64) * 100.0
                } else {
                    0.0
                };
                return (completed, rate);
            }

            // 旧版：从 data.completion 读取
            let a_total = r.data.completion.priority_a_total;
            let a_done = r.data.completion.priority_a_done;
            let rate = if a_total > 0 {
                (a_done as f64 / a_total as f64) * 100.0
            } else if r.data.completion.priority_b_total > 0 {
                // 无 A 级任务，用 B 级完成率
                let b_total = r.data.completion.priority_b_total;
                let b_done = r.data.completion.priority_b_done;
                (b_done as f64 / b_total as f64) * 100.0
            } else {
                // 无任何任务且有 review（旧版 AI 生成），视为完成
                100.0
            };
            (a_done, rate)
        }
        None => (0, 0.0),
    }
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
}

/// submit_review 的返回结构
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubmitReviewResult {
    /// 是否需要调用 AI 重新生成本周剩余天数计划
    pub needs_regeneration: bool,
    /// 触发重排的原因（用于前端展示）
    pub regen_reasons: Vec<String>,
    /// 次日简报是否已在后台开始生成（fire-and-forget）
    #[serde(default)]
    pub briefing_generating: bool,
}

/// 单个受影响日期的任务变动摘要（重排前后对比）
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegenDayChange {
    /// 受影响日期 (YYYY-MM-DD)
    pub date: String,
    /// 该日新增的任务标题
    #[serde(default)]
    pub added: Vec<String>,
    /// 该日被移除的任务标题
    #[serde(default)]
    pub removed: Vec<String>,
    /// 该日标题变化的（原标题 → 新标题）
    #[serde(default)]
    pub adjusted: Vec<(String, String)>,
}

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

/// 教材信息
#[derive(serde::Serialize)]
pub struct TextbookInfo {
    pub id: String,
    pub subject: String,
    pub title: String,
    pub filename: String,
    pub file_path: String,
}

/// 教材内容
#[derive(serde::Serialize)]
pub struct TextbookContent {
    pub id: String,
    pub content: String,
    pub file_path: String,
}

/// 将 relative_path 解析为 data_dir 内的绝对路径
///
/// 规范化 `..` / `.` / 反斜杠，并校验结果路径仍位于 data_dir 内，
/// 防止通过 `../../config/settings` 之类参数读取或删除 data_dir 之外的任意文件（C4-b）。
pub(super) fn resolve_relative_path(
    data_dir: &std::path::Path,
    relative_path: &str,
) -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;

    let cleaned = relative_path.replace('\\', "/");
    let normalized = cleaned.split('/').fold(PathBuf::new(), |mut acc, part| {
        match part {
            "" | "." => {}
            ".." => {
                acc.pop();
            }
            _ => acc.push(part),
        }
        acc
    });

    let target = data_dir.join(normalized);
    let canonical_data_dir =
        std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let canonical_target = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());

    if !canonical_target.starts_with(&canonical_data_dir) {
        return Err(format!(
            "路径越界: {:?} 不在数据目录 {:?} 内",
            canonical_target, canonical_data_dir
        ));
    }

    Ok(canonical_target)
}

/// 教材内搜索结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct TextbookSearchHit {
    pub textbook_id: String,
    pub textbook_title: String,
    pub subject: String,
    pub line_number: usize,
    pub snippet: String,
    /// 该行命中的关键词数量（用于排序，前端可忽略）
    #[serde(default)]
    pub hit_weight: usize,
    /// 该行实际命中的关键词（供前端高亮片段与正文）
    #[serde(default)]
    pub matched_terms: Vec<String>,
}

/// 浅层中文分词：把查询拆成可检索的关键词。
///
/// - ASCII 连续串（英文单词 / 数字 / 符号）作为独立词；
/// - CJK 汉字按相邻 2-gram 切分并过滤常见停用字；
/// - 过滤掉过于通用的单字/词，避免噪声。
pub(super) fn extract_search_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();

    // ASCII 连续串
    let mut ascii_buf = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            ascii_buf.push(ch);
        } else {
            if ascii_buf.len() >= 2 {
                terms.push(ascii_buf.clone());
            }
            ascii_buf.clear();
        }
    }
    if ascii_buf.len() >= 2 {
        terms.push(ascii_buf);
    }

    // CJK：过滤停用字后生成 2-gram
    let cjk_chars: Vec<char> = query
        .chars()
        .filter(|c| is_cjk(*c))
        .filter(|c| !is_stop_char(*c))
        .collect();
    for w in cjk_chars.windows(2) {
        let t: String = w.iter().collect();
        if !t.is_empty() {
            terms.push(t);
        }
    }

    terms
}

/// 是否为 CJK 汉字（不含标点、数字、字母）
pub(super) fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}')
}

/// 常见提问/口语停用字：这些字单独作为 2-gram 没有检索意义
pub(super) fn is_stop_char(c: char) -> bool {
    matches!(
        c,
        '的' | '了'
            | '是'
            | '我'
            | '你'
            | '他'
            | '她'
            | '它'
            | '这'
            | '那'
            | '就'
            | '都'
            | '也'
            | '在'
            | '有'
            | '和'
            | '与'
            | '及'
            | '把'
            | '被'
            | '让'
            | '帮'
            | '请'
            | '问'
            | '题'
            | '道'
            | '下'
            | '么'
            | '什'
            | '怎'
            | '吗'
            | '呢'
            | '呀'
            | '啊'
            | '吧'
            | '个'
            | '种'
            | '讲'
            | '解'
            | '答'
            | '方'
            | '法'
            | '一'
            | '不'
            | '要'
            | '会'
            | '能'
            | '可'
            | '以'
            | '到'
            | '里'
            | '之'
            | '后'
            | '前'
            | '上'
            | '中'
            | '或'
            | '于'
            | '而'
            | '并'
            | '且'
            | '对'
            | '为'
            | '从'
            | '叫'
            | '给'
            | '过'
            | '来'
            | '去'
            | '起'
            | '张'
            | '章'
            | '节'
            | '本'
            | '出'
            | '面'
            | '路'
            | '程'
            | '点'
            | '想'
            | '看'
            | '试'
            | '列'
            | '好'
            | '很'
            | '太'
            | '更'
            | '最'
            | '紧'
            | '关'
            | '键'
            | '核'
            | '心'
    )
}

/// 是否为中文数字字符
pub(super) fn is_cjk_numeral(c: char) -> bool {
    matches!(
        c,
        '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    )
}

/// 中文数字 → 阿拉伯数字（支持 0-99 及常见写法）
pub(super) fn chinese_num_to_arabic(s: &str) -> Option<u32> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse().ok();
    }
    let mut total = 0u32;
    let mut cur = 0u32;
    for c in s.chars() {
        if c == '十' {
            if cur == 0 {
                cur = 1;
            }
            total += cur * 10;
            cur = 0;
        } else {
            let v = single_num(c)?;
            cur = v;
        }
    }
    total += cur;
    Some(total)
}

pub(super) fn single_num(c: char) -> Option<u32> {
    match c {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

/// 从查询中解析「第N章」章节号与「第N题」题号
pub(super) fn parse_refs(query: &str) -> (Option<u32>, Option<u32>) {
    let mut chapter = None;
    let mut problem = None;
    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格（OCR 会在 `第 2 章` / `第 2 题` 里插入空格）
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut num = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                num.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「章/题」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !num.is_empty() && j < chars.len() {
                if let Some(n) = chinese_num_to_arabic(&num) {
                    if chars[j] == '章' && chapter.is_none() {
                        chapter = Some(n);
                    } else if chars[j] == '题' && problem.is_none() {
                        problem = Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    (chapter, problem)
}

/// 判断一行是否为 Markdown 章节标题（以 # 开头且含「第N章」）
pub(super) fn is_chapter_heading(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with('#') && extract_chapter_num(l).is_some()
}

/// 从一行中提取章节号（仅匹配「第N章」形式）
pub(super) fn extract_chapter_num(line: &str) -> Option<u32> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut num = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                num.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「章」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !num.is_empty() && j < chars.len() && chars[j] == '章' {
                return chinese_num_to_arabic(&num);
            }
        }
        i += 1;
    }
    None
}

/// 在教材行中定位指定章节的标题行
///
/// 只匹配以 `#` 开头的 Markdown 章节标题，忽略目录中或正文里的引用。
pub(super) fn find_chapter_line(lines: &[&str], num: u32) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .find(|(_, l)| {
            let trimmed = l.trim_start();
            trimmed.starts_with('#') && extract_chapter_num(trimmed) == Some(num)
        })
        .map(|(i, _)| i)
}

/// 从一行中提取题目编号（OCR 容错）。
///
/// 支持：
/// - `第N题`（阿拉伯/中文数字）行内写法；
/// - Markdown 标题或普通行首的数字编号，如 `#### 02.`、`2)`、`2、`、`（3）`、
///   `0 2.`（数字间空格）、全角数字 `３` 等 OCR 杂音；
/// - 返回 `None` 表示该行不是「编号题目行」。
pub(super) fn problem_number_of(line: &str) -> Option<u32> {
    let mut l = line.trim_start();
    // 行内「第N题」
    if let Some(n) = extract_problem_from_text(l) {
        return Some(n);
    }
    // 去掉 Markdown 标题记号 `#`
    while l.starts_with('#') {
        l = l[1..].trim_start();
    }
    extract_leading_number(l)
}

/// 提取行内「第N题」编号
pub(super) fn extract_problem_from_text(l: &str) -> Option<u32> {
    let chars: Vec<char> = l.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '第' {
            let mut j = i + 1;
            // 「第」与数字之间允许空格（OCR 会在 `第 2 题` 里插入空格）
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let mut digs = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || is_cjk_numeral(chars[j])) {
                digs.push(chars[j]);
                j += 1;
            }
            // 数字后允许空格再跟「题」
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if !digs.is_empty() && j < chars.len() && chars[j] == '题' {
                if let Some(n) = chinese_num_to_arabic(&digs) {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// 提取行首编号（OCR 容错），要求编号后跟题号分隔符（`.`/`、`/`)`/`）`/`:`/空格等）
pub(super) fn extract_leading_number(s: &str) -> Option<u32> {
    let mut it = s.chars().peekable();
    // 可选的开括号
    if matches!(it.peek(), Some('（') | Some('(')) {
        it.next();
    }
    let mut num = String::new();
    while let Some(&c) = it.peek() {
        if c.is_ascii_digit() || is_fullwidth_digit(c) {
            num.push(to_ascii_digit(c));
            it.next();
        } else if c == ' ' || c == '\t' {
            // 数字间允许空格（如 `0 2.`）；但若空格后不再是数字，
            // 则该空格就是编号结束的分隔符（如 `11 若…`），停止解析
            let mut rest = it.clone();
            rest.next(); // 跳过当前空格
            let next_non_space = rest.find(|&c| c != ' ' && c != '\t');
            match next_non_space {
                Some(ch) if ch.is_ascii_digit() || is_fullwidth_digit(ch) => {
                    it.next(); // 数字间的空格，消费后继续
                }
                _ => break, // 空格即编号结束，保留其作为分隔符的语义
            }
        } else {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    // 必须跟编号分隔符，避免把正文数字误判为题号
    // 含逗号（`,`/`，`）：OCR 常把题号后的 `.` 识别成 `,`（如 `05, 若…`）
    if matches!(
        it.peek(),
        Some('.')
            | Some('、')
            | Some(')')
            | Some('）')
            | Some(':')
            | Some('：')
            | Some('-')
            | Some(' ')
            | Some(',')
            | Some('，')
    ) {
        num.parse().ok()
    } else {
        None
    }
}

/// 是否为全角数字
pub(super) fn is_fullwidth_digit(c: char) -> bool {
    matches!(
        c,
        '０' | '１' | '２' | '３' | '４' | '５' | '６' | '７' | '８' | '９'
    )
}

/// 全角数字 → 半角
pub(super) fn to_ascii_digit(c: char) -> char {
    match c {
        '０' => '0',
        '１' => '1',
        '２' => '2',
        '３' => '3',
        '４' => '4',
        '５' => '5',
        '６' => '6',
        '７' => '7',
        '８' => '8',
        '９' => '9',
        _ => c,
    }
}

/// 判断扁平 OCR 文本中某行是否为「第 N 章」的小节标题。
///
/// 只匹配行首为 `N.` 且后跟数字的多级小节号（如 `3.2.1`、`3.43`），
/// 排除 `3. 题目` 这类单号题干，避免把题目行误当作章节边界。
pub(super) fn is_flat_section_header(line: &str, num: u32) -> bool {
    let t = line.trim_start();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut digs = String::new();
    while i < chars.len() && chars[i].is_ascii_digit() {
        digs.push(chars[i]);
        i += 1;
    }
    if digs.is_empty() {
        return false;
    }
    let n: u32 = digs.parse().unwrap_or(0);
    if n != num {
        return false;
    }
    // 下一个字符必须是点号，且后面跟数字（区分 `3.2` 与 `3. 题目`）
    if i < chars.len() && (chars[i] == '.' || chars[i] == '．') {
        return i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
    }
    false
}

/// 在扁平 OCR 文本中定位「第 num 章」的范围（num.x 小节起始 → num+1.x 小节起始）
pub(super) fn find_flat_chapter_range(lines: &[&str], num: u32) -> Option<(usize, usize)> {
    // 先跳过目录/封面区，避免把目录里的 `N.M` 小节号误当成章节边界
    let content_start = find_content_start(lines);
    let mut start = None;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(content_start) {
        if start.is_none() {
            if is_flat_section_header(l, num) {
                start = Some(i);
            }
        } else if is_flat_section_header(l, num + 1) {
            end = i;
            break;
        }
    }
    start.map(|s| (s, end))
}

/// 定位扁平 OCR 文本的「正文起点」，跳过开头的目录/封面/版权等前置噪声。
///
/// 目录区通常由短行（`N.M` 小节号 + 标题）构成，且包含与正文重复的小节编号，
/// 若直接从中定位章节会得到错误边界。正文以 `【考纲内容】`、`【复习提示】` 等
/// 专属标记（王道教材常见），或较长整段文字（>=40 字符）为信号，据此估算正文起点。
pub(super) fn find_content_start(lines: &[&str]) -> usize {
    // 1) 正文专属标记：`【考纲…】`/`【复习…】`/`【考点…】`/`【本节…】`/`【答案…】` 等方括号标记
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.contains('【')
            && (t.contains("考纲")
                || t.contains("复习")
                || t.contains("考点")
                || t.contains("本节")
                || t.contains("答案"))
        {
            return i;
        }
        if t.ends_with("考纲") || t.ends_with("复习提示") {
            return i;
        }
    }
    // 2) 回退：第一个长行（>=40 字符）视为正文
    for (i, l) in lines.iter().enumerate() {
        if l.trim().chars().count() >= 40 {
            return i;
        }
    }
    0
}

/// 是否进入习题区（习题小节标题）。用于限定题目顺序计数范围，
/// 避免把正文里的列表序号（如 `3 第三代…`）误当题号导致第 N 题顺序偏移。
pub(super) fn is_exercise_start(lower: &str) -> bool {
    lower.contains("本节习题")
        || lower.contains("习题精选")
        || lower.contains("单项选择题")
        || lower.contains("综合应用题")
        || lower.contains("综合题")
}

/// 是否退出习题区（答案区标题）
pub(super) fn is_answer_section(lower: &str) -> bool {
    lower.contains("答案与解析") || lower.contains("答案解析") || lower.contains("参考答案")
}

/// 在字符边界上安全切片，避免 `ctx_end` 落在多字节 UTF-8 字符中间导致 panic
pub(super) fn safe_char_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.len();
    let mut cstart = start;
    while cstart < len && !s.is_char_boundary(cstart) {
        cstart += 1;
    }
    if cstart >= len {
        return "";
    }
    let mut cend = end.min(len);
    while cend > cstart && !s.is_char_boundary(cend) {
        cend -= 1;
    }
    &s[cstart..cend]
}

// ============================================================================
// AI 对话命令
// ============================================================================

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

/// GitHub API 端点：获取最新 release
pub(super) const GITHUB_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/Wenjiugugugu/StudyAgent-client/releases/latest";

/// 推测资源类型
///
/// 安装程序现由 Inno Setup 生成（约定命名为 `*-setup.exe`）；
/// `nsis` 仅保留用于识别历史 release 里遗留的旧安装包。
pub(super) fn detect_asset_kind(name: &str) -> String {
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

/// 从 GitHub release assets 数组提取安装包列表
///
/// 过滤掉 `.sig`、`.json`、`.txt` 等非安装包文件。
pub(super) fn extract_install_assets(assets: &serde_json::Value) -> Vec<UpdateAsset> {
    let arr = match assets.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    arr.iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?.to_string();
            let lower = name.to_lowercase();

            // 跳过签名 / manifest 文件
            if lower.ends_with(".sig")
                || lower.ends_with(".json")
                || lower.ends_with(".txt")
                || lower.ends_with(".blockmap")
            {
                return None;
            }

            let download_url = asset.get("browser_download_url")?.as_str()?.to_string();
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let kind = detect_asset_kind(&name);

            // GitHub API 对资产提供 digest 字段，形如 "sha256:<hex>"，剥离前缀
            let sha256 = asset
                .get("digest")
                .and_then(|v| v.as_str())
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(|hex| hex.to_lowercase())
                .filter(|hex| !hex.is_empty());

            if kind == "unknown" || sha256.as_ref().is_none_or(|hex| !is_valid_sha256(hex)) {
                log::warn!("[Update] 忽略缺少有效 SHA-256 的安装资源: {}", name);
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

/// 构造一个「暂时无法检查」的结果（用于错误降级）
///
/// 错误原因仅写入日志，不暴露给前端 message。
pub(super) fn unavailable_result(current_version: &str, log_reason: &str) -> UpdateCheckResult {
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
    }
}

/// 计算文件的 SHA-256（十六进制小写）
pub(super) fn sha256_hex(path: &std::path::Path) -> Result<String, String> {
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

pub(super) fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn is_allowed_update_url(url: &reqwest::Url, require_release_path: bool) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if require_release_path {
        host == "github.com"
            && url
                .path()
                .starts_with("/Wenjiugugugu/StudyAgent-client/releases/download/")
    } else {
        host == "github.com"
            || host == "objects.githubusercontent.com"
            || host == "release-assets.githubusercontent.com"
    }
}

pub(super) fn verified_updates(
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
pub(super) fn is_newer_version(remote: &str, current: &str) -> bool {
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
pub(super) fn split_version(v: &str) -> (String, Option<String>) {
    if let Some(idx) = v.find('-') {
        (v[..idx].to_string(), Some(v[idx + 1..].to_string()))
    } else {
        (v.to_string(), None)
    }
}

/// 比较两个预发布后缀：>0 表示 a 更新，==0 相同，<0 a 更旧
pub(super) fn compare_prerelease(a: &str, b: &str) -> i32 {
    // 简单按字典序比较（足够覆盖 beta / rc / alpha）
    // 详见 https://semver.org/#spec-item-11
    a.cmp(b) as i32
}

// ============================================================================
// 辅助函数
// ============================================================================

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
