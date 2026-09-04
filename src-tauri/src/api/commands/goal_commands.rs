//! Tauri 命令 — 目标与截止日规划区间（CRUD + 按目标生成当日任务）
//!
//! 前端通过 `invoke('list_goals')` / `create_goal` / `update_goal` / `delete_goal`
//! 管理每条科目的「截止日 + 目标章节」区间；用 `generate_goal_plan` 对区间内某科目
//! 生成当天任务（内容确定性倒排，时长为标准粒度；需要 AI 估时时用异步版本）。

use std::sync::Mutex;

use tauri::State;

use crate::core::goal_planner::{subject_key_str, subject_version};
use crate::data::goal::{read_goals, save_goals, Goal, GoalPlanFile};
use crate::data::plan::PlanTask;
use crate::data::state::{read_state_or_default, SubjectKey};
use crate::{get_data_dir, get_data_dir_and_ai, AppState};

/// 列出全部目标区间。
///
/// 前端调用: `invoke('list_goals')`
#[tauri::command]
pub async fn list_goals(state: State<'_, Mutex<AppState>>) -> Result<GoalPlanFile, String> {
    let data_dir = get_data_dir(state.inner())?;
    read_goals(&data_dir)
}

/// 创建一条目标区间。
///
/// 位置推导：
/// - `target_position` = `target_chapter` 在当前版本顺序表中的位置（必需命中考纲）。
/// - `current_position` = `start_chapter` 的位置（缺省取 0）。
/// 前端调用: `invoke('create_goal', { subject, title, deadline, targetChapter, startChapter })`
#[tauri::command]
pub async fn create_goal(
    subject: SubjectKey,
    title: String,
    deadline: String,
    target_chapter: String,
    #[allow(unused_variables)] start_chapter: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Goal, String> {
    crate::data::validate_date(&deadline)?;
    let data_dir = get_data_dir(state.inner())?;

    let key = subject_key_str(&subject);
    let state_data = read_state_or_default(&data_dir);
    let version = subject_version(&state_data, key);
    let target_position = crate::core::chapter_seq::position(key, &version, &target_chapter)
        .ok_or_else(|| format!("未能在地图中考纲顺序表中定位目标章节「{}」", target_chapter))?;
    let start_chapter_str = start_chapter.unwrap_or_default();
    let current_position =
        if start_chapter_str.is_empty() {
            0
        } else {
            crate::core::chapter_seq::position(key, &version, &start_chapter_str).unwrap_or(0)
        };

    let mut file = read_goals(&data_dir)?;
    let seq = file.data.goals.len() + 1;
    let id = format!("goal-{}-{}", key, seq);
    let goal = Goal {
        id,
        subject: subject.clone(),
        title,
        deadline: deadline.clone(),
        target_chapter: target_chapter.clone(),
        start_chapter: start_chapter_str,
        current_position: Some(current_position),
        target_position: Some(target_position),
        active: true,
        status: "active".to_string(),
    };
    file.data.goals.push(goal.clone());
    file.meta.generated_at = crate::data::now_string();
    save_goals(&data_dir, &file)?;
    Ok(goal)
}

/// 更新一条目标区间（就地 replace 并按字段重新校验）。
///
/// 前端调用: `invoke('update_goal', { goal })`
#[tauri::command]
pub async fn update_goal(
    goal: Goal,
    state: State<'_, Mutex<AppState>>,
) -> Result<Goal, String> {
    if !goal.deadline.is_empty() {
        crate::data::validate_date(&goal.deadline)?;
    }
    let data_dir = get_data_dir(state.inner())?;
    let mut file = read_goals(&data_dir)?;
    let pos = file
        .data
        .goals
        .iter()
        .position(|g| g.id == goal.id)
        .ok_or_else(|| format!("目标 {} 不存在", goal.id))?;
    file.data.goals[pos] = goal.clone();
    file.meta.generated_at = crate::data::now_string();
    save_goals(&data_dir, &file)?;
    Ok(goal)
}

/// 删除一条目标区间。
///
/// 前端调用: `invoke('delete_goal', { goalId })`
#[tauri::command]
pub async fn delete_goal(
    goal_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = get_data_dir(state.inner())?;
    let mut file = read_goals(&data_dir)?;
    let before = file.data.goals.len();
    file.data.goals.retain(|g| g.id != goal_id);
    if file.data.goals.len() == before {
        return Err(format!("目标 {} 不存在", goal_id));
    }
    file.meta.generated_at = crate::data::now_string();
    save_goals(&data_dir, &file)
}

/// 为目标区间内某科目生成当天任务（AI 参与估时）。
///
/// 若该科目当天无生效区间，或当天不在倒排推进区间内，返回空数组。
/// 前端调用: `invoke('generate_goal_plan', { subject, date })`
#[tauri::command]
pub async fn generate_goal_plan(
    subject: SubjectKey,
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<PlanTask>, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    let goal = crate::data::goal::active_goal_for(&data_dir, &subject, &date)
        .ok_or_else(|| format!("{} 当天没有生效的目标区间", subject_display_zh(&subject)))?;

    let state_data = read_state_or_default(&data_dir);
    let key = subject_key_str(&subject);
    let version = subject_version(&state_data, key);
    crate::core::goal_planner::plan_goal_tasks(&data_dir, &ai_service, &goal, &date, &version)
        .await
}

/// 从当前 state 反推某科目的当前进度章节名（用于新建区间时预填起点）。
///
/// 前端调用: `invoke('get_goal_start_chapter', { subject })`
#[tauri::command]
pub async fn get_goal_start_chapter(
    subject: SubjectKey,
    state: State<'_, Mutex<AppState>>,
) -> Result<Option<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    let state_data = read_state_or_default(&data_dir);
    let key = subject_key_str(&subject);
    let _version = subject_version(&state_data, key);
    // 当前完整实现未在 state 中维护逐科章节进度，返回 None 由前端自行决定起点。
    Ok(None)
}

fn subject_display_zh(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}