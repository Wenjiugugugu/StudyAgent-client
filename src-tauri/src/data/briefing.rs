//! Briefing 数据层 — 读取/写入 `records/YYYY-MM-DD_briefing.json`
//!
//! 每日简报由 AI 在用户提交复盘后自动生成（针对次日），包含：
//! - 一句 AI 生成的「今日寄语」（结合昨日复盘感受与今日任务）
//! - 各科「预计还需多久学完当前阶段/教材」的估算
//!
//! 数据契约：{ version, meta, data }
//!
//! 对应前端 TypeScript 类型: types/briefing.ts

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{list_dir_files, read_file_content, DataResult};

// ============================================================================
// Briefing 数据结构
// ============================================================================

/// Briefing 文件（完整 JSON）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BriefingFile {
    pub version: String,
    pub meta: BriefingMeta,
    pub data: BriefingData,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BriefingMeta {
    /// 简报对应的日期 (YYYY-MM-DD)
    pub date: String,
    /// 生成时间 (YYYY-MM-DDTHH:mm)
    pub generated_at: String,
    /// 生成该简报所依据的复盘日期 (YYYY-MM-DD)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub based_on_review: String,
    /// 生成方式：auto（复盘后自动）/ manual（手动重新生成）
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BriefingData {
    /// AI 生成的「今日寄语」（1-3 句自然语言）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub greeting: String,
    /// 各科进度估算
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimations: Vec<SubjectEstimation>,
}

/// 单科进度估算
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubjectEstimation {
    /// 科目 key：math / english / politics / professional
    pub subject: String,
    /// 当前正在学习的章节/阶段（来自 state.current_focus 或前一日计划）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_chapter: String,
    /// 预计还需多少天学完当前教材/阶段
    #[serde(default)]
    pub estimated_days_to_finish: i32,
    /// AI 给出的简短说明（如「按当前进度可在 X 月前完成基础阶段」）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

// ============================================================================
// 常量与路径
// ============================================================================

pub const BRIEFING_FILE_SUFFIX: &str = "_briefing.json";

/// 获取指定日期的 briefing 文件路径
pub fn briefing_file_path(data_dir: &Path, date: &str) -> PathBuf {
    data_dir
        .join(super::records::RECORDS_DIR)
        .join(format!("{}{}", date, BRIEFING_FILE_SUFFIX))
}

/// 列出所有 briefing 文件的日期 (YYYY-MM-DD，升序)
pub fn list_briefing_dates(data_dir: &Path) -> DataResult<Vec<String>> {
    let records_dir = data_dir.join(super::records::RECORDS_DIR);
    let files = list_dir_files(&records_dir)?;

    let mut dates: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?;
            if name.ends_with(BRIEFING_FILE_SUFFIX) {
                Some(name.trim_end_matches(BRIEFING_FILE_SUFFIX).to_string())
            } else {
                None
            }
        })
        .collect();

    dates.sort();
    dates.dedup();
    Ok(dates)
}

// ============================================================================
// 读取 / 写入
// ============================================================================

/// 读取指定日期的 briefing JSON
pub fn read_briefing(data_dir: &Path, date: &str) -> DataResult<BriefingFile> {
    let path = briefing_file_path(data_dir, date);
    if !path.exists() {
        return Err(format!("Briefing 文件不存在: {:?}", path));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析 Briefing JSON 失败: {}", e))
}

/// 保存 briefing JSON
pub fn save_briefing(data_dir: &Path, briefing: &BriefingFile) -> DataResult<()> {
    let path = briefing_file_path(data_dir, &briefing.meta.date);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 records 目录失败: {}", e))?;
        }
    }
    let json = serde_json::to_string_pretty(briefing)
        .map_err(|e| format!("序列化 Briefing 失败: {}", e))?;
    super::atomic_write(&path, &json)
        .map_err(|e| format!("写入 Briefing 文件失败 {:?}: {}", path, e))?;
    Ok(())
}

/// 删除指定日期的 briefing（用于重新生成前清理）
pub fn delete_briefing(data_dir: &Path, date: &str) -> DataResult<()> {
    let path = briefing_file_path(data_dir, date);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("删除 Briefing 文件失败 {:?}: {}", path, e))?;
    }
    Ok(())
}
