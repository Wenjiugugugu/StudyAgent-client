//! Records 数据层 — 读取/写入 `records/YYYY-MM-DD_review.json`
//!
//! 统一数据契约：{ version, meta, data, view?, task_reviews?, daily_review? }
//! - 复盘：records/YYYY-MM-DD_review.json
//!
//! 对应前端 TypeScript 类型: types/review.ts

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::state::{SubjectKey, TaskPriority};
use super::{list_dir_files, read_file_content, DataResult};

// ============================================================================
// Review 数据结构
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewFile {
    pub version: String,
    pub meta: ReviewMeta,
    pub data: ReviewData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// 新版：每个任务的结构化复盘（兼容历史数据，默认空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_reviews: Vec<TaskReviewEntry>,
    /// 新版：每日整体回顾（兼容历史数据，默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_review: Option<DailyReviewInput>,
    /// 计划外学习记录（用户实际进度领先计划时填写，用于校正下一轮计划）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overcompletion: Vec<OvercompletionEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewMeta {
    pub date: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub plan_ref: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewData {
    pub completed_tasks: Vec<ReviewCompletedTask>,
    pub unplanned_tasks: Vec<ReviewUnplannedTask>,
    pub difficulties: Vec<ReviewDifficulty>,
    pub time_spent: Vec<ReviewTimeSpent>,
    pub total_hours: f64,
    pub completion: ReviewCompletion,
    pub energy_level: i32,
    pub external_interference: String,
    pub key_achievements: Vec<String>,
    /// 已解除风险（已废弃，仅为兼容旧 review JSON 保留反序列化；新数据不再写入）
    #[serde(default, skip_serializing)]
    pub risks_resolved: Vec<String>,
    pub next_steps: Vec<String>,
}

/// 完成任务记录
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewCompletedTask {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub subject: SubjectKey,
    pub title: String,
    pub priority: TaskPriority,
    pub completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 计划外任务
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewUnplannedTask {
    pub subject: SubjectKey,
    pub title: String,
    pub hours: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 遇到的困难
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewDifficulty {
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
}

/// 实际用时
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewTimeSpent {
    pub subject: SubjectKey,
    pub hours: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_hours: Option<f64>,
}

/// 完成情况统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewCompletion {
    #[serde(default)]
    pub priority_a_total: i32,
    #[serde(default)]
    pub priority_a_done: i32,
    #[serde(default)]
    pub priority_b_total: i32,
    #[serde(default)]
    pub priority_b_done: i32,
    #[serde(default)]
    pub completion_rate: f64,
}

// ============================================================================
// 新版 Review 数据结构（结构化问答，非 AI 生成）
// ============================================================================

/// 任务复盘条目（新版 Review 的核心数据）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskReviewEntry {
    pub task_id: String,
    /// completed | partial | incomplete | abandoned
    pub status: String,
    /// 0.0 - 1.0 完成百分比
    #[serde(default)]
    pub completion: f64,
    /// mastered | basic | weak（掌握程度，仅 completed/partial 时有意义）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mastery: String,
    /// 未完成原因标签（多选）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    /// 其他原因说明（仅当 blockers 包含 "other" 时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_note: Option<String>,
    /// 任务标题（自包含字段，便于复盘记录独立展示，不依赖 plan 文件）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// 科目：math / english / politics / professional（自包含字段）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    /// 优先级：A / B（自包含字段）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub priority: String,
    /// AI 估时（小时），仅在启用「记录学习时长」时由前端带入并持久化
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_hours: Option<f64>,
    /// 实际用时（分钟），仅在启用「记录学习时长」时由前端带入并持久化
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_minutes: Option<i64>,
}

/// 每日整体回顾（新版 Review）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyReviewInput {
    /// smooth | normal | hard
    pub overall_feeling: String,
    /// understanding | problems | memorization | attention | time_management | environment | other
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub main_difficulty: String,
}

/// 计划外学习记录条目（用户实际进度领先于计划进度时填写）
///
/// 提交后会更新对应科目的 `current_focus`，避免下一轮计划进度落后于实际进度。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OvercompletionEntry {
    /// 科目：math / english / politics / professional
    pub subject: String,
    /// 实际已学习到的章节（如 "多元函数微分学"）
    pub chapter_reached: String,
    /// 备注（可选）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ============================================================================
// 兼容别名
// ============================================================================

/// 兼容别名：ReviewRecord 等价于 ReviewFile（完整文件）
pub type ReviewRecord = ReviewFile;

// ============================================================================
// 常量与路径
// ============================================================================

pub const RECORDS_DIR: &str = "records";
pub const REVIEW_FILE_SUFFIX: &str = "_review.json";

/// 获取指定日期的 review 文件路径
pub fn review_file_path(data_dir: &Path, date: &str) -> PathBuf {
    data_dir
        .join(RECORDS_DIR)
        .join(format!("{}{}", date, REVIEW_FILE_SUFFIX))
}

/// 计算复盘的实际学习时长（小时）。
///
/// 优先使用 `data.total_hours`（旧版 AI 复盘写入）；
/// 当其为 0 时，回退从新版结构化复盘的 `task_reviews[].actual_minutes` 聚合，
/// 以兼容 `submit_review` 早期未写入 total_hours 的历史复盘文件。
pub fn review_actual_hours(review: &ReviewFile) -> f64 {
    if review.data.total_hours > 0.0 {
        return review.data.total_hours;
    }
    let total_minutes: i64 = review
        .task_reviews
        .iter()
        .filter_map(|tr| tr.actual_minutes)
        .sum();
    total_minutes as f64 / 60.0
}

/// 从复盘文件计算完成率统计
///
/// 优先从 `task_reviews` 聚合（新版复盘），回退到 `data.completion`（旧版复盘）。
/// 返回 `(a_total, a_done, b_total, b_done, completion_rate)`。
///
/// 当无 A/B 级任务但有其他任务时，将全部任务计入 A 级字段以保证 completion_rate 正确。
pub fn review_completion_stats(review: &ReviewFile) -> (i32, i32, i32, i32, f64) {
    if !review.task_reviews.is_empty() {
        let mut a_total = 0i32;
        let mut a_done = 0i32;
        let mut b_total = 0i32;
        let mut b_done = 0i32;
        let mut all_total = 0i32;
        let mut all_done = 0i32;
        for tr in &review.task_reviews {
            all_total += 1;
            let is_done = tr.status == "completed";
            if is_done {
                all_done += 1;
            }
            match tr.priority.as_str() {
                "A" => {
                    a_total += 1;
                    if is_done {
                        a_done += 1;
                    }
                }
                "B" => {
                    b_total += 1;
                    if is_done {
                        b_done += 1;
                    }
                }
                _ => {}
            }
        }
        let (ca_total, ca_done) = if a_total + b_total == 0 && all_total > 0 {
            (all_total, all_done)
        } else {
            (a_total, a_done)
        };
        let total = ca_total + b_total;
        let done = ca_done + b_done;
        let rate = if total > 0 {
            (done as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        (ca_total, ca_done, b_total, b_done, rate)
    } else {
        let c = &review.data.completion;
        (
            c.priority_a_total,
            c.priority_a_done,
            c.priority_b_total,
            c.priority_b_done,
            c.completion_rate,
        )
    }
}

/// 列出所有 review 文件的日期 (YYYY-MM-DD)
pub fn list_review_dates(data_dir: &Path) -> DataResult<Vec<String>> {
    let records_dir = data_dir.join(RECORDS_DIR);
    let files = list_dir_files(&records_dir)?;

    let mut dates: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let name = f.file_name()?.to_str()?;
            if name.ends_with(REVIEW_FILE_SUFFIX) {
                Some(name.trim_end_matches(REVIEW_FILE_SUFFIX).to_string())
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

/// 读取指定日期的 review JSON
pub fn read_review(data_dir: &Path, date: &str) -> DataResult<ReviewFile> {
    let path = review_file_path(data_dir, date);
    if !path.exists() {
        return Err(format!("Review 文件不存在: {:?}", path));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析 Review JSON 失败: {}", e))
}

/// 保存 review JSON
pub fn save_review(data_dir: &Path, review: &ReviewFile) -> DataResult<()> {
    let path = review_file_path(data_dir, &review.meta.date);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 records 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(review).map_err(|e| format!("序列化 Review 失败: {}", e))?;
    super::atomic_write(&path, &json)
        .map_err(|e| format!("写入 Review 文件失败 {:?}: {}", path, e))?;
    Ok(())
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "studyagent_review_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let review = ReviewFile {
            version: "1.0.0".to_string(),
            meta: ReviewMeta {
                date: "2026-07-25".to_string(),
                r#type: "review".to_string(),
                plan_ref: "plan/2026-07-25_day.json".to_string(),
                generated_at: "2026-07-25T22:00".to_string(),
            },
            data: ReviewData {
                completed_tasks: vec![ReviewCompletedTask {
                    task_id: Some("2026-07-25-01".to_string()),
                    subject: SubjectKey::Math,
                    title: "启动线代第一章：行列式".to_string(),
                    priority: TaskPriority::A,
                    completed: true,
                    completion_time: Some("10:30".to_string()),
                    note: None,
                }],
                unplanned_tasks: vec![],
                difficulties: vec![],
                time_spent: vec![ReviewTimeSpent {
                    subject: SubjectKey::Math,
                    hours: 2.0,
                    planned_hours: Some(2.0),
                }],
                total_hours: 2.0,
                completion: ReviewCompletion {
                    priority_a_total: 1,
                    priority_a_done: 1,
                    priority_b_total: 0,
                    priority_b_done: 0,
                    completion_rate: 100.0,
                },
                energy_level: 4,
                external_interference: "无".to_string(),
                key_achievements: vec!["完成线代第一章".to_string()],
                next_steps: vec!["继续第二章".to_string()],
                ..Default::default()
            },
            view: None,
            task_reviews: vec![],
            daily_review: None,
            overcompletion: vec![],
        };

        save_review(&tmp, &review).expect("应能保存 Review");

        let read = read_review(&tmp, "2026-07-25").expect("应能读取 Review");
        assert_eq!(read.meta.date, "2026-07-25");
        assert_eq!(read.data.completed_tasks.len(), 1);
        assert_eq!(read.data.completed_tasks[0].subject, SubjectKey::Math);
        assert_eq!(read.data.completion.completion_rate, 100.0);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
