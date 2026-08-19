//! Assets 数据层 — 读取 `assets/` 下的知识对象、用户画像、里程碑等
//!
//! 对应前端 TypeScript 类型: `types/knowledge.ts`
//!
//! 注意：Assets 层仍使用 Markdown + YAML frontmatter 格式存储知识对象。
//! 这些 YAML/Markdown 解析工具仅用于此模块，不适用于 Planning Layer
//! （Week Plan / Today Plan / Review 已迁移为结构化 JSON）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{DataResult, list_dir_files_recursive, read_file_content};

// ============================================================================
// YAML Frontmatter 解析工具（仅用于 Assets 层）
// ============================================================================

/// 从 Markdown 内容中分离 YAML frontmatter 和正文。
///
/// 返回 `(frontmatter_yaml, body)`。如果没有 frontmatter，
/// frontmatter 为 `None`，body 为原始内容。
fn split_frontmatter(content: &str) -> (Option<String>, String) {
    let content = content.trim_start_matches('\u{feff}');
    let content = content.trim_start();

    if !content.starts_with("---") {
        return (None, content.to_string());
    }

    // 跳过开头的 "---"
    let after_delim = &content[3..];

    // 跳过 "---" 后面的换行
    let after_delim = if after_delim.starts_with('\n') {
        &after_delim[1..]
    } else if after_delim.starts_with("\r\n") {
        &after_delim[2..]
    } else {
        after_delim
    };

    // 查找闭合的 "---"
    if let Some(end_pos) = find_closing_delimiter(after_delim) {
        let yaml_content = &after_delim[..end_pos];

        // 跳过闭合的 "---" 及其后的换行
        let body_start = end_pos + 3;
        let body = if body_start < after_delim.len() {
            let rest = &after_delim[body_start..];
            let rest = if rest.starts_with('\n') {
                &rest[1..]
            } else if rest.starts_with("\r\n") {
                &rest[2..]
            } else {
                rest
            };
            rest.trim_start().to_string()
        } else {
            String::new()
        };

        (Some(yaml_content.to_string()), body)
    } else {
        // 没有找到闭合的 "---"
        (None, content.to_string())
    }
}

/// 在 YAML 内容中查找闭合的 `---` 分隔符位置（行首）
///
/// M19：改为按行累积字节偏移定位当前行，避免 `s.find(line)` 命中更早的相同内容
/// （如 frontmatter 值里出现 `---`）导致 frontmatter/正文切分错误。
fn find_closing_delimiter(s: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in s.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            return Some(offset);
        }
        offset += line.len();
    }
    // 回退：搜索 "\n---"
    s.find("\n---").map(|p| p + 1)
}

/// 将简单 YAML 文本解析为 `serde_json::Value`。
fn parse_yaml_to_value(yaml: &str) -> DataResult<serde_json::Value> {
    let lines: Vec<(usize, String)> = yaml
        .lines()
        .filter_map(|raw_line| {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let indent = raw_line.len() - raw_line.trim_start().len();
            Some((indent, trimmed.to_string()))
        })
        .collect();

    let mut parser = YamlParser { lines, pos: 0 };

    let result = parser.parse_block(0)?;
    Ok(result)
}

struct YamlParser {
    lines: Vec<(usize, String)>,
    pos: usize,
}

impl YamlParser {
    fn parse_block(&mut self, indent: usize) -> DataResult<serde_json::Value> {
        if self.pos >= self.lines.len() {
            return Ok(serde_json::Value::Null);
        }

        let (line_indent, ref content) = self.lines[self.pos].clone();

        if line_indent < indent {
            return Ok(serde_json::Value::Null);
        }

        // 数组检测
        if content.starts_with("- ") || content == "-" {
            return self.parse_array(line_indent);
        }

        // 映射检测
        self.parse_mapping(line_indent)
    }

    fn parse_mapping(&mut self, indent: usize) -> DataResult<serde_json::Value> {
        let mut map = serde_json::Map::new();

        while self.pos < self.lines.len() {
            let (line_indent, content) = self.lines[self.pos].clone();

            if line_indent < indent {
                break;
            }
            if line_indent > indent {
                break;
            }

            let parsed = if let Some(rest) = content.strip_suffix(':') {
                let key = clean_yaml_key(rest);
                self.pos += 1;
                if self.pos < self.lines.len() && self.lines[self.pos].0 > indent {
                    let nested = self.parse_block(self.lines[self.pos].0)?;
                    (key, nested)
                } else {
                    (key, serde_json::Value::Null)
                }
            } else if let Some(colon_pos) = find_colon_separator(&content) {
                let key = clean_yaml_key(&content[..colon_pos]);
                let value_str = content[colon_pos + 1..].trim();

                if value_str.is_empty() {
                    self.pos += 1;
                    if self.pos < self.lines.len() && self.lines[self.pos].0 > indent {
                        let nested = self.parse_block(self.lines[self.pos].0)?;
                        (key, nested)
                    } else {
                        (key, serde_json::Value::Null)
                    }
                } else if value_str == ">" || value_str == "|" {
                    self.pos += 1;
                    let block_indent = if self.pos < self.lines.len() {
                        self.lines[self.pos].0
                    } else {
                        indent + 1
                    };
                    let folded = value_str == ">";
                    let text = self.collect_multiline_scalar(block_indent, folded);
                    (key, serde_json::Value::String(text))
                } else {
                    let value = parse_scalar_or_inline(value_str);
                    self.pos += 1;
                    (key, value)
                }
            } else {
                self.pos += 1;
                continue;
            };

            map.insert(parsed.0, parsed.1);
        }

        Ok(serde_json::Value::Object(map))
    }

    fn parse_array(&mut self, indent: usize) -> DataResult<serde_json::Value> {
        let mut arr = Vec::new();

        while self.pos < self.lines.len() {
            let (line_indent, content) = self.lines[self.pos].clone();

            if line_indent < indent {
                break;
            }
            if line_indent > indent {
                break;
            }

            if !content.starts_with('-') {
                break;
            }

            let item_content = if content == "-" {
                String::new()
            } else {
                content[1..].trim().to_string()
            };

            if item_content.is_empty() {
                self.pos += 1;
                if self.pos < self.lines.len() && self.lines[self.pos].0 > indent {
                    let nested = self.parse_block(self.lines[self.pos].0)?;
                    arr.push(nested);
                } else {
                    arr.push(serde_json::Value::Null);
                }
            } else if item_content.starts_with('[') {
                arr.push(parse_scalar_or_inline(&item_content));
                self.pos += 1;
            } else if find_colon_separator(&item_content).is_some() {
                let new_indent = indent + 2;
                self.lines[self.pos] = (new_indent, item_content.clone());
                let nested = self.parse_mapping(new_indent)?;
                arr.push(nested);
            } else {
                arr.push(parse_scalar_or_inline(&item_content));
                self.pos += 1;
            }
        }

        Ok(serde_json::Value::Array(arr))
    }

    fn collect_multiline_scalar(&mut self, block_indent: usize, folded: bool) -> String {
        let mut lines = Vec::new();
        while self.pos < self.lines.len() && self.lines[self.pos].0 >= block_indent {
            lines.push(self.lines[self.pos].1.clone());
            self.pos += 1;
        }
        if folded {
            lines.join(" ")
        } else {
            lines.join("\n")
        }
    }
}

fn clean_yaml_key(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn find_colon_separator(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_quotes = false;
    let mut quote_char = b'"';

    for i in 0..bytes.len() {
        let c = bytes[i];

        if !in_quotes && (c == b'"' || c == b'\'') {
            in_quotes = true;
            quote_char = c;
        } else if in_quotes && c == quote_char {
            in_quotes = false;
        }

        if !in_quotes && c == b':' {
            if i + 1 >= bytes.len() || bytes[i + 1] == b' ' {
                return Some(i);
            }
        }
    }

    None
}

fn parse_scalar_or_inline(s: &str) -> serde_json::Value {
    let s = s.trim();

    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        let items: Vec<serde_json::Value> = split_inline_array_items(inner)
            .into_iter()
            .map(|item| parse_scalar_or_inline(&item))
            .collect();
        return serde_json::Value::Array(items);
    }

    if s.starts_with('{') && s.ends_with('}') {
        let inner = &s[1..s.len() - 1];
        let mut map = serde_json::Map::new();
        for pair in split_inline_array_items(inner) {
            if let Some(pos) = find_colon_separator(&pair) {
                let key = clean_yaml_key(&pair[..pos]);
                let value = parse_scalar_or_inline(pair[pos + 1..].trim());
                map.insert(key, value);
            }
        }
        return serde_json::Value::Object(map);
    }

    parse_scalar(s)
}

fn parse_scalar(s: &str) -> serde_json::Value {
    let s = s.trim();

    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return serde_json::Value::String(s[1..s.len() - 1].to_string());
    }

    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return serde_json::Value::String(s[1..s.len() - 1].to_string());
    }

    match s {
        "true" | "True" | "TRUE" => return serde_json::Value::Bool(true),
        "false" | "False" | "FALSE" => return serde_json::Value::Bool(false),
        "null" | "Null" | "NULL" | "~" => return serde_json::Value::Null,
        _ => {}
    }

    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }

    if let Ok(f) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(num);
        }
    }

    serde_json::Value::String(s.to_string())
}

fn split_inline_array_items(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_quotes = false;
    let mut quote_char = '"';

    for c in s.chars() {
        if !in_quotes && (c == '"' || c == '\'') {
            in_quotes = true;
            quote_char = c;
            current.push(c);
        } else if in_quotes && c == quote_char {
            in_quotes = false;
            current.push(c);
        } else if !in_quotes && (c == '[' || c == '{') {
            depth += 1;
            current.push(c);
        } else if !in_quotes && (c == ']' || c == '}') {
            depth -= 1;
            current.push(c);
        } else if !in_quotes && c == ',' && depth == 0 {
            items.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }

    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }

    items
}

// ============================================================================
// 用户画像类型
// ============================================================================

/// 用户能力（Capability）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserCapability {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: String, // "learning_style" | "cognitive_trait" | "subject_profile"
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub activity: String, // "active" | "stale"
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub source_observation: Option<String>,
}

/// 用户观察（Observation）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserObservation {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub evidence_count: i32,
    #[serde(default)]
    pub status: String, // "pending" | "confirmed" | "archived"
    #[serde(default)]
    pub suggested_capability: Option<String>,
    #[serde(default)]
    pub generated: String,
}

/// 用户画像索引
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserModelIndex {
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub capabilities: Vec<UserCapability>,
    #[serde(default)]
    pub observations: Vec<UserObservation>,
}

// ============================================================================
// 里程碑类型
// ============================================================================

/// 里程碑
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Milestone {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target_date: String,
    #[serde(default)]
    pub status: String, // "pending" | "in_progress" | "done"
    #[serde(default)]
    pub content: String,
}

// ============================================================================
// 路径常量
// ============================================================================

pub const ASSETS_DIR: &str = "assets";
pub const USER_MODEL_DIR: &str = "user_model";
pub const CAPABILITIES_DIR: &str = "capabilities";
pub const OBSERVATIONS_DIR: &str = "observations";
pub const MILESTONES_DIR: &str = "milestones";
pub const MAPPING_DIR: &str = "mapping";
pub const MAPPING_ENTRIES_DIR: &str = "entries";
pub const REGISTRY_DIR: &str = "registry";

// ============================================================================
// 用户画像读取
// ============================================================================

/// 读取用户画像索引
pub fn read_user_model_index(data_dir: &Path) -> DataResult<UserModelIndex> {
    let index_path = data_dir
        .join(ASSETS_DIR)
        .join(USER_MODEL_DIR)
        .join("_index.md");

    if !index_path.exists() {
        return Ok(UserModelIndex::default());
    }

    let content = read_file_content(&index_path)?;
    Ok(parse_user_model_index(&content))
}

/// 解析用户画像索引 Markdown
pub fn parse_user_model_index(content: &str) -> UserModelIndex {
    let mut model = UserModelIndex::default();

    // 解析 frontmatter
    let (yaml, body) = split_frontmatter(content);
    if let Some(yaml_str) = yaml {
        if let Ok(value) = parse_yaml_to_value(&yaml_str) {
            if let Some(updated) = value.get("updated").and_then(|v| v.as_str()) {
                model.updated = updated.to_string();
            }
        }
    }

    // 解析 Capabilities 表格
    let mut current_section = "";
    let mut in_table = false;
    let mut header_found = false;

    for line in body.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## Capabilities") {
            current_section = "capabilities";
            continue;
        }
        if trimmed.starts_with("## Observations") {
            current_section = "observations";
            continue;
        }

        if trimmed.starts_with('|') {
            if !in_table {
                in_table = true;
                header_found = false;
                continue;
            } else if !header_found {
                header_found = true;
                continue;
            } else {
                let cells: Vec<&str> = trimmed
                    .trim_start_matches('|')
                    .trim_end_matches('|')
                    .split('|')
                    .map(|c| c.trim())
                    .collect();

                if current_section == "capabilities" && cells.len() >= 2 {
                    let cap = UserCapability {
                        id: cells.first().map(|s| s.to_string()).unwrap_or_default(),
                        title: cells.get(1).map(|s| s.to_string()).unwrap_or_default(),
                        category: cells.get(2).map(|s| s.to_string()).unwrap_or_default(),
                        confidence: cells
                            .get(3)
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0),
                        activity: cells.get(4).map(|s| s.to_string()).unwrap_or_default(),
                        created_at: cells.get(6).map(|s| s.to_string()).unwrap_or_default(),
                        updated_at: cells.get(7).map(|s| s.to_string()).unwrap_or_default(),
                        // M18：capabilities 表无 Status 列，不再从 Activity 列重复取值；
                        // status 由详情文件 frontmatter 填充（read_capability）
                        status: String::new(),
                        description: String::new(),
                        evidence_refs: Vec::new(),
                        source_observation: None,
                    };

                    if !cap.id.is_empty() && cap.id != "ID" {
                        model.capabilities.push(cap);
                    }
                } else if current_section == "observations" && cells.len() >= 2 {
                    let obs = UserObservation {
                        id: cells.first().map(|s| s.to_string()).unwrap_or_default(),
                        summary: cells.get(1).map(|s| s.to_string()).unwrap_or_default(),
                        confidence: cells
                            .get(2)
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0),
                        evidence_count: cells
                            .get(3)
                            .and_then(|s| s.parse::<i32>().ok())
                            .unwrap_or(0),
                        status: cells.get(4).map(|s| s.to_string()).unwrap_or_default(),
                        suggested_capability: cells.get(5).map(|s| s.to_string()),
                        generated: cells.get(6).map(|s| s.to_string()).unwrap_or_default(),
                    };

                    if !obs.id.is_empty() && obs.id != "ID" {
                        model.observations.push(obs);
                    }
                }
            }
        } else if in_table && !trimmed.is_empty() && !trimmed.starts_with('|') {
            in_table = false;
            header_found = false;
        }
    }

    // 读取每个 capability 的详情
    model
}

/// 读取单个 Capability 详情
pub fn read_capability(data_dir: &Path, cap_id: &str) -> DataResult<UserCapability> {
    let path = data_dir
        .join(ASSETS_DIR)
        .join(USER_MODEL_DIR)
        .join(CAPABILITIES_DIR)
        .join(format!("{}.md", cap_id));

    if !path.exists() {
        return Err(format!("Capability 文件不存在: {:?}", path));
    }

    let content = read_file_content(&path)?;

    let (frontmatter_value, _body) = {
        let (yaml, body) = split_frontmatter(&content);
        match yaml {
            Some(yaml_str) => {
                let value = parse_yaml_to_value(&yaml_str)?;
                (value, body)
            }
            None => (serde_json::Value::Null, content.clone()),
        }
    };

    let mut cap = UserCapability::default();
    cap.id = frontmatter_value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(cap_id)
        .to_string();
    cap.title = frontmatter_value
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.category = frontmatter_value
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.description = frontmatter_value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.confidence = frontmatter_value
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    cap.activity = frontmatter_value
        .get("activity")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.evidence_refs = frontmatter_value
        .get("evidence_refs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    cap.created_at = frontmatter_value
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.updated_at = frontmatter_value
        .get("updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.status = frontmatter_value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    cap.source_observation = frontmatter_value
        .get("source_observation")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(cap)
}

// ============================================================================
// 里程碑读取
// ============================================================================

/// 读取所有里程碑
pub fn read_milestones(data_dir: &Path) -> DataResult<Vec<Milestone>> {
    let milestones_dir = data_dir
        .join(ASSETS_DIR)
        .join(MILESTONES_DIR);

    let files = list_dir_files_recursive(&milestones_dir)?;

    let mut milestones = Vec::new();

    for file in files {
        let name = match file.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if !name.ends_with(".md") || name.starts_with('_') || name == "README.md" {
            continue;
        }

        let id = name.trim_end_matches(".md").to_string();

        match read_file_content(&file) {
            Ok(content) => {
            let (yaml, body) = split_frontmatter(&content);
            let mut milestone = Milestone {
                id: id.clone(),
                content: body,
                ..Default::default()
            };

            if let Some(yaml_str) = yaml {
                if let Ok(value) = parse_yaml_to_value(&yaml_str) {
                    milestone.title = value
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    milestone.description = value
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    milestone.target_date = value
                        .get("target_date")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    milestone.status = value
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending")
                        .to_string();
                }
            }

            milestones.push(milestone);
            }
            Err(e) => {
                log::warn!("读取里程碑文件 {:?} 失败: {}", file, e);
            }
        }
    }

    // 按 ID 排序
    milestones.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(milestones)
}
