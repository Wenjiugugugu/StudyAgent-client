//! 教材搜索、引用与文本定位。
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
pub(crate) fn resolve_relative_path(
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
pub(crate) fn extract_search_terms(query: &str) -> Vec<String> {
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
pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}')
}

/// 常见提问/口语停用字：这些字单独作为 2-gram 没有检索意义
pub(crate) fn is_stop_char(c: char) -> bool {
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
pub(crate) fn is_cjk_numeral(c: char) -> bool {
    matches!(
        c,
        '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    )
}

/// 中文数字 → 阿拉伯数字（支持 0-99 及常见写法）
pub(crate) fn chinese_num_to_arabic(s: &str) -> Option<u32> {
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

pub(crate) fn single_num(c: char) -> Option<u32> {
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
pub(crate) fn parse_refs(query: &str) -> (Option<u32>, Option<u32>) {
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
pub(crate) fn is_chapter_heading(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with('#') && extract_chapter_num(l).is_some()
}

/// 从一行中提取章节号（仅匹配「第N章」形式）
pub(crate) fn extract_chapter_num(line: &str) -> Option<u32> {
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
pub(crate) fn find_chapter_line(lines: &[&str], num: u32) -> Option<usize> {
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
pub(crate) fn problem_number_of(line: &str) -> Option<u32> {
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
pub(crate) fn extract_problem_from_text(l: &str) -> Option<u32> {
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
pub(crate) fn extract_leading_number(s: &str) -> Option<u32> {
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
pub(crate) fn is_fullwidth_digit(c: char) -> bool {
    matches!(
        c,
        '０' | '１' | '２' | '３' | '４' | '５' | '６' | '７' | '８' | '９'
    )
}

/// 全角数字 → 半角
pub(crate) fn to_ascii_digit(c: char) -> char {
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
pub(crate) fn is_flat_section_header(line: &str, num: u32) -> bool {
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
pub(crate) fn find_flat_chapter_range(lines: &[&str], num: u32) -> Option<(usize, usize)> {
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
pub(crate) fn find_content_start(lines: &[&str]) -> usize {
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
pub(crate) fn is_exercise_start(lower: &str) -> bool {
    lower.contains("本节习题")
        || lower.contains("习题精选")
        || lower.contains("单项选择题")
        || lower.contains("综合应用题")
        || lower.contains("综合题")
}

/// 是否退出习题区（答案区标题）
pub(crate) fn is_answer_section(lower: &str) -> bool {
    lower.contains("答案与解析") || lower.contains("答案解析") || lower.contains("参考答案")
}

/// 在字符边界上安全切片，避免 `ctx_end` 落在多字节 UTF-8 字符中间导致 panic
pub(crate) fn safe_char_slice(s: &str, start: usize, end: usize) -> &str {
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
