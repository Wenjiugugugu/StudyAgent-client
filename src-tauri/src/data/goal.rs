//! Goal 数据层 — 目标与截止日规划区间（plan/goals.json）
//!
//! 统一数据契约：{ version, meta, data }
//! - 文件：plan/goals.json
//!
//! **每科唯一**：一个科目最多拥有一条目标计划（不论状态 active/completed/expired）。
//! 需要改变目标时编辑该条，需要重设时先删除该条。
//!
//! 每个科目可拥有一条独立的「截止日 + 目标章节」区间。区间生效期内，
//! 该科目的每日任务由 core::goal_planner 用 chapter_seq 倒排生成；
//! 区间外/已达标/已过截止日则该科目回退到默认按学习时长安排。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::state::SubjectKey;
use super::{atomic_write, read_file_content, DataResult};

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

/// 一条截止日规划区间（绑定单一科目 + 书/板块）
///
/// **每书唯一**：同一科目下，相同的「书/板块」（`book`）只允许一条目标计划。
/// `book` 为空字符串时表示「整科一条」（兼容历史数据：旧目标在迁移期内继续走「每科唯一」守卫）。
/// 新建目标时 `book` 必填（与目标章节所属书本一致）；每个书最多一条，跨书允许多条并行推进。
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
    /// 目标所属的书/板块（进度表 chapter 节点标题），如「高等数学」「线性代数」。
    /// 同一 (subject, book) 组合只允许一条目标；空串视为「整科一条」兼容旧数据。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub book: String,
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
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(file).map_err(|e| format!("序列化 goals.json 失败: {}", e))?;
    atomic_write(&path, &json).map_err(|e| format!("写入 goals.json 失败 {:?}: {}", path, e))
}

// ============================================================================
// 查询
// ============================================================================

/// 按「是否达标 / 是否过期」重算目标的生命周期状态（就地修改）。
///
/// 每科唯一意味着目标不能靠「删掉再建」来重启，因此编辑保存后需要重算：
/// - 已达目标位置 → completed（不再生效）
/// - 截止日已过 → expired（不再生效）
/// - 其余 → active（恢复生效）
pub fn refresh_goal_lifecycle(goal: &mut Goal, today: &str) {
    let reached = matches!(
        (goal.current_position, goal.target_position),
        (Some(cur), Some(tgt)) if cur >= tgt
    );
    if reached {
        goal.active = false;
        goal.status = "completed".to_string();
    } else if !goal.deadline.is_empty() && goal.deadline.as_str() < today {
        goal.active = false;
        goal.status = "expired".to_string();
    } else {
        goal.active = true;
        goal.status = "active".to_string();
    }
}

/// 取某科目 + 某书的目标计划（不论状态）。
///
/// 用于「每书只允许一条」的写入守卫：已存在则不允许再新建。
/// `book == ""` 视为「整科唯一」，对应历史行为。
pub fn goal_of_subject_book<'a>(
    file: &'a GoalPlanFile,
    subject: &SubjectKey,
    book: &str,
) -> Option<&'a Goal> {
    file.data
        .goals
        .iter()
        .find(|g| g.subject == *subject && g.book == book)
}

/// 某科目 + 某书是否已有目标计划（守卫用便捷判断）。
pub fn has_goal_for_book(file: &GoalPlanFile, subject: &SubjectKey, book: &str) -> bool {
    goal_of_subject_book(file, subject, book).is_some()
}

/// 兼容旧调用：「每科一条」查询（book == ""）。
///
/// 历史数据（无 `book` 字段）下所有目标 `book == ""`，等价于每科一条。
pub fn goal_of_subject<'a>(file: &'a GoalPlanFile, subject: &SubjectKey) -> Option<&'a Goal> {
    goal_of_subject_book(file, subject, "")
}

/// 某科目是否已有「整科」目标计划（兼容旧 API）。
pub fn has_goal_for(file: &GoalPlanFile, subject: &SubjectKey) -> bool {
    goal_of_subject(file, subject).is_some()
}

/// 某科目在指定日期的全部「生效」目标（多个书/板块同时推进）。
///
/// `book == ""` 的「整科」目标也包含在内。
pub fn active_goals_for_subject(data_dir: &Path, subject: &SubjectKey, today: &str) -> Vec<Goal> {
    let Ok(file) = read_goals(data_dir) else {
        return Vec::new();
    };
    file.data
        .goals
        .iter()
        .filter(|g| {
            g.subject == *subject
                && g.active
                && !g.deadline.is_empty()
                && g.deadline.as_str() >= today
        })
        .cloned()
        .collect()
}

/// 判断某科目在指定日期是否有「生效区间」（**仅匹配 `book == ""` 的整科旧目标**）。
///
/// 注意：本函数是「每科一条」历史语义的兼容 API —— 它只查 `book` 为空字符串的目标，
/// 因此当该科目只存在带 `book` 的新目标时返回 `None`（**这不是 bug，是刻意收窄**）。
/// 支持同科多书并行的调用方请改用 `active_goals_for_subject`（返回全部生效目标）。
///
/// 现状：生产代码已无调用者，与 `goal_of_subject` / `has_goal_for` 一起保留，
/// 仅用于旧语义查询与兼容性测试。新增逻辑请优先使用每书语义的 API。
pub fn active_goal_for(data_dir: &Path, subject: &SubjectKey, today: &str) -> Option<Goal> {
    let file = read_goals(data_dir).ok()?;
    let goal = goal_of_subject(&file, subject)?;
    if goal.active && !goal.deadline.is_empty() && goal.deadline.as_str() >= today {
        Some(goal.clone())
    } else {
        None
    }
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
                    book: "线性代数".to_string(),
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
        assert_eq!(read.data.goals[0].book, "线性代数");
        assert!(read.data.goals[0].active);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 「每书唯一」守卫：同一 subject 下不同 book 可同时存在，同一 book 不可重复。
    #[test]
    fn goal_of_subject_book_distinguishes_books() {
        let file = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta::default(),
            data: GoalPlanData {
                goals: vec![
                    Goal {
                        id: "g1".into(),
                        subject: SubjectKey::Math,
                        book: "高等数学".into(),
                        ..Default::default()
                    },
                    Goal {
                        id: "g2".into(),
                        subject: SubjectKey::Math,
                        book: "线性代数".into(),
                        ..Default::default()
                    },
                ],
            },
        };
        assert!(has_goal_for_book(&file, &SubjectKey::Math, "高等数学"));
        assert!(has_goal_for_book(&file, &SubjectKey::Math, "线性代数"));
        // 第三本书允许
        assert!(!has_goal_for_book(&file, &SubjectKey::Math, "概率论"));
        // 旧查询（book=""）查不到带 book 的目标
        assert!(!has_goal_for(&file, &SubjectKey::Math));
    }

    /// 兼容：旧数据无 `book` 字段时，所有目标 `book == ""`，`goal_of_subject` 仍正确识别
    /// 「每科一条」旧语义。
    #[test]
    fn goal_of_subject_matches_legacy_empty_book() {
        let file = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta::default(),
            data: GoalPlanData {
                goals: vec![Goal {
                    id: "g1".into(),
                    subject: SubjectKey::Math,
                    book: "".into(),
                    ..Default::default()
                }],
            },
        };
        assert!(has_goal_for(&file, &SubjectKey::Math));
        assert_eq!(
            goal_of_subject(&file, &SubjectKey::Math).map(|g| g.id.as_str()),
            Some("g1")
        );
        // 也可被「book == ''」查询命中
        assert!(has_goal_for_book(&file, &SubjectKey::Math, ""));
    }

    /// 守卫语义：goal_of_subject_book 跨 (subject, book) 区分；book=="" 视为「整科一条」旧语义。
    #[test]
    fn goal_of_subject_matches_any_status() {
        let file = GoalPlanFile {
            version: "1.0.0".to_string(),
            meta: GoalPlanMeta::default(),
            data: GoalPlanData {
                goals: vec![
                    Goal {
                        id: "goal-math-1".to_string(),
                        subject: SubjectKey::Math,
                        book: "".into(),
                        deadline: "2026-09-20".to_string(),
                        active: false,
                        status: "completed".to_string(),
                        ..Default::default()
                    },
                    Goal {
                        id: "goal-math-2".to_string(),
                        subject: SubjectKey::Math,
                        book: "".into(),
                        deadline: "2026-10-01".to_string(),
                        active: true,
                        status: "active".to_string(),
                        ..Default::default()
                    },
                ],
            },
        };
        // 已达标的历史条目同样视为「该科已占用」，因此不允许再建第三条
        assert!(has_goal_for(&file, &SubjectKey::Math));
        assert_eq!(
            goal_of_subject(&file, &SubjectKey::Math).map(|g| g.id.as_str()),
            Some("goal-math-1")
        );
        assert!(!has_goal_for(&file, &SubjectKey::English));
    }

    /// 编辑保存后的生命周期重算：改到未来的截止日可让过期/达标目标恢复生效。
    #[test]
    fn refresh_lifecycle_reactivates_when_valid() {
        let mut g = Goal {
            id: "goal-math-1".to_string(),
            subject: SubjectKey::Math,
            deadline: "2026-09-10".to_string(),
            current_position: Some(3),
            target_position: Some(40),
            active: false,
            status: "expired".to_string(),
            ..Default::default()
        };
        // 截止日已过 → 仍为 expired
        refresh_goal_lifecycle(&mut g, "2026-09-11");
        assert!(!g.active);
        assert_eq!(g.status, "expired");

        // 顺延截止日到未来 → 恢复进行中
        g.deadline = "2026-10-01".to_string();
        refresh_goal_lifecycle(&mut g, "2026-09-11");
        assert!(g.active);
        assert_eq!(g.status, "active");

        // 改目标到更晚章节前仍生效；一旦目标位置已被达到 → completed
        g.target_position = Some(3);
        refresh_goal_lifecycle(&mut g, "2026-09-11");
        assert!(!g.active);
        assert_eq!(g.status, "completed");
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
            data: GoalPlanData {
                goals: vec![active.clone()],
            },
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
            data: GoalPlanData {
                goals: vec![active],
            },
        };
        save_goals(&tmp, &file2).unwrap();
        assert!(active_goal_for(&tmp, &SubjectKey::Math, "2026-09-04").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
