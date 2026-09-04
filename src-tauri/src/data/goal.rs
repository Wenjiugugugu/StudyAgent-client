//! Goal 数据层 — 目标与截止日规划区间（plan/goals.json）
//!
//! 统一数据契约：{ version, meta, data }
//! - 文件：plan/goals.json
//!
//! 每个科目可拥有一条独立的「截止日 + 目标章节」区间。区间生效期内，
//! 该科目的每日任务由 core::goal_planner 用 chapter_seq 倒排生成；
//! 区间外/已达标/已过截止日则该科目回退到默认按学习时长安排。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{atomic_write, read_file_content, DataResult};
use super::state::SubjectKey;

// ============================================================================
// 结构
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalPlanFile {
    pub version: String,
    pub meta: GoalPlanMeta,
    pub data: GoalPlanData,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalPlanMeta {
    pub generated_at: String,
    pub based_on: super::plan::BasedOn,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalPlanData {
    pub goals: Vec<Goal>,
}

/// 一条截止日规划区间（绑定单一科目）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Goal {
    /// 唯一标识，如 "goal-math-1"
    pub id: String,
    /// 关联科目
    pub subject: SubjectKey,
    /// 目标描述（用户自定义），如 "9/20 前完成线性方程组"
    pub title: String,
    /// 截止日期 YYYY-MM-DD
    pub deadline: String,
    /// 目标知识点（用 chapter_seq::position 在当前版本顺序表中定位），如 "线性方程组"
    pub target_chapter: String,
    /// 生效起点章节（创建时自动取当前进度，仅展示用）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_chapter: String,
    /// 当前进度在顺序表中的位置（自动维护，由复盘/超进度推进）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_position: Option<usize>,
    /// 目标在顺序表中的位置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_position: Option<usize>,
    /// 是否仍生效（未到截止日且未达标）
    #[serde(default = "default_true")]
    pub active: bool,
    /// active | completed | expired
    #[serde(default = "default_active_status")]
    pub status: String,
}

fn default_true() -> bool {
    true
}

fn default_active_status() -> String {
    "active".to_string()
}

// ============================================================================
// 常量与路径
// ============================================================================

pub const GOALS_FILE: &str = "plan/goals.json";

/// 获取 goals.json 文件路径
pub fn goals_path(data_dir: &Path) -> PathBuf {
    data_dir.join(GOALS_FILE)
}

// ============================================================================
// 读取 / 写入
// ============================================================================

/// 读取目标清单；文件不存在时返回空清单
pub fn read_goals(data_dir: &Path) -> DataResult<GoalPlanFile> {
    let path = goals_path(data_dir);
    if !path.exists() {
        return Ok(GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta {
                generated_at: super::now_string(),
                based_on: crate::data::plan::BasedOn::default(),
            },
            data: GoalPlanData { goals: Vec::new() },
        });
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析 goals.json 失败: {}", e))
}

/// 保存目标清单
pub fn save_goals(data_dir: &Path, file: &GoalPlanFile) -> DataResult<()> {
    let path = goals_path(data_dir);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(file).map_err(|e| format!("序列化 goals.json 失败: {}", e))?;
    atomic_write(&path, &json).map_err(|e| format!("写入 goals.json 失败 {:?}: {}", path, e))
}

// ============================================================================
// 查询
// ============================================================================

/// 判断某科目在指定日期是否有「生效区间」。
///
/// 生效条件：goal.active == true 且 今天 <= deadline 且 goal 关联该科目。
pub fn active_goal_for(
    data_dir: &Path,
    subject: &SubjectKey,
    today: &str,
) -> Option<Goal> {
    let file = read_goals(data_dir).ok()?;
    file.data
        .goals
        .into_iter()
        .find(|g| {
            g.active && g.subject == *subject && !g.deadline.is_empty() && g.deadline.as_str() >= today
        })
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "studyagent_goal_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ))
    }

    #[test]
    fn goals_roundtrip() {
        let tmp = tmp_dir("roundtrip");
        std::fs::create_dir_all(&tmp).unwrap();

        let file = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta {
                generated_at: "2026-09-04T10:00".to_string(),
                based_on: crate::data::plan::BasedOn::default(),
            },
            data: GoalPlanData {
                goals: vec![Goal {
                    id: "goal-math-1".to_string(),
                    subject: SubjectKey::Math,
                    title: "9/20 前完成线性方程组".to_string(),
                    deadline: "2026-09-20".to_string(),
                    target_chapter: "线性方程组".to_string(),
                    start_chapter: "行列式".to_string(),
                    current_position: Some(3),
                    target_position: Some(40),
                    active: true,
                    status: "active".to_string(),
                }],
            },
        };
        save_goals(&tmp, &file).expect("应能保存");
        let read = read_goals(&tmp).expect("应能读取");
        assert_eq!(read.data.goals.len(), 1);
        assert_eq!(read.data.goals[0].subject, SubjectKey::Math);
        assert_eq!(read.data.goals[0].target_position, Some(40));
        assert_eq!(read.data.goals[0].active, true);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn active_goal_filters() {
        let tmp = tmp_dir("active");
        std::fs::create_dir_all(&tmp).unwrap();
        let mut active = Goal {
            id: "goal-math-1".to_string(),
            subject: SubjectKey::Math,
            title: "t".to_string(),
            deadline: "2026-09-20".to_string(),
            target_chapter: "线性方程组".to_string(),
            active: true,
            status: "active".to_string(),
            ..Default::default()
        };
        let file = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta::default(),
            data: GoalPlanData { goals: vec![active.clone()] },
        };
        save_goals(&tmp, &file).unwrap();

        // 今天早于 deadline → 生效
        assert!(active_goal_for(&tmp, &SubjectKey::Math, "2026-09-04").is_some());
        // 其他科目 → 不生效
        assert!(active_goal_for(&tmp, &SubjectKey::English, "2026-09-04").is_none());
        // 已过截止日 → 不生效
        assert!(active_goal_for(&tmp, &SubjectKey::Math, "2026-09-21").is_none());

        active.active = false;
        let file2 = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta::default(),
            data: GoalPlanData { goals: vec![active] },
        };
        save_goals(&tmp, &file2).unwrap();
        assert!(active_goal_for(&tmp, &SubjectKey::Math, "2026-09-04").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}