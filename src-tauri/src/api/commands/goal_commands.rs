//! Tauri 命令 — 目标与截止日规划区间（CRUD + 按目标生成当日任务）
//!
//! 前端通过 `invoke('list_goals')` / `create_goal` / `update_goal` / `delete_goal`
//! 管理各科目各书/板块的「截止日 + 目标章节」区间；用 `generate_goal_plan` 对区间内
//! 某科目生成当天任务（内容确定性倒排，时长为标准粒度；需要 AI 估时时用异步版本）。
//!
//! **每书唯一**：同一 (subject, book) 组合只允许一条目标计划；
//! 跨书允许多条并行推进。`book == ""` 视为「整科一条」（兼容历史数据）。

use std::sync::Mutex;

use tauri::State;

use crate::core::goal_planner::{subject_key_str, subject_version};
use crate::data::goal::{goal_of_subject_book, read_goals, save_goals, Goal, GoalPlanFile};
use crate::data::plan::PlanTask;
use crate::data::progress_tables::load_progress_index;
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
/// **每书唯一**：同一 (subject, book) 组合已有目标计划（不论状态）时直接报错，
/// 前端应引导用户编辑或删除已有那条。`book` 必填，与目标章节所属书本一致。
///
/// 位置推导：
/// - `target_position` = `target_chapter` 在当前版本顺序表中的位置（必需命中考纲）。
/// - `current_position`（语义 = 「已完成到的位置」，倒排从 +1 开始派发）：
///   若提供了 `start_chapter`（UI 承诺「此前内容视为已学」，该章本身要学）→ `position-1`；
///   否则回退到 `book` 的第一知识点位置减一（自动限定到本书内，避免跨书乱排）。
/// - 两者均需属于所选 `book`（进度表能确定该书区间时强校验）。
///
/// 前端调用: `invoke('create_goal', { subject, title, deadline, targetChapter, startChapter, book })`
#[tauri::command]
pub async fn create_goal(
    subject: SubjectKey,
    title: String,
    deadline: String,
    target_chapter: String,
    start_chapter: Option<String>,
    book: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Goal, String> {
    crate::data::validate_date(&deadline)?;
    let data_dir = get_data_dir(state.inner())?;

    let book_trimmed = book.unwrap_or_default().trim().to_string();
    if book_trimmed.is_empty() {
        return Err("必须选择目标所属的书/板块（book），每书只允许配置一个目标计划".to_string());
    }

    let key = subject_key_str(&subject);
    let state_data = read_state_or_default(&data_dir);
    let version = subject_version(&state_data, key);
    let target_position = crate::core::chapter_seq::position(key, &version, &target_chapter)
        .ok_or_else(|| format!("未能在地图中考纲顺序表中定位目标章节「{}」", target_chapter))?;
    let start_chapter_str = start_chapter.unwrap_or_default();
    let start_position = if start_chapter_str.is_empty() {
        None
    } else {
        Some(
            crate::core::chapter_seq::position(key, &version, &start_chapter_str).ok_or_else(
                || {
                    format!(
                        "未能在地图中考纲顺序表中定位起始章节「{}」",
                        start_chapter_str
                    )
                },
            )?,
        )
    };

    // 计算书本第一知识点位置（顺序表中 position），用于 current_position 的兜底：
    // 用户在「中等章节」想推进一本书时，让倒排从该书第一点（的前一位置）开始，
    // 避免把目标范围扩展到这本书之前的内容。
    let book_first_pos = book_first_position(&data_dir, key, &version, &book_trimmed);

    // 归属守卫：目标章节 / 起始位置必须属于所选书（进度表能确定区间时才校验）
    ensure_positions_within_book(
        &data_dir,
        key,
        &version,
        &book_trimmed,
        (&target_chapter, target_position),
        start_position.map(|p| (start_chapter_str.as_str(), p)),
    )?;

    let mut file = read_goals(&data_dir)?;

    // 每书唯一守卫
    if let Some(existing) = goal_of_subject_book(&file, &subject, &book_trimmed) {
        return Err(format!(
            "{}「{}」已有目标计划「{}」，每书只允许一个；请直接编辑该目标，或先删除后再新建。",
            subject_display_zh(&subject),
            book_trimmed,
            existing.title
        ));
    }

    // current_position 语义 = 「已完成到的位置」，倒排从 current_position + 1 开始派发
    //（见 core/goal_planner::backward_schedule）。
    // - 显式给了 start_chapter：UI 承诺「此前内容视为已学」，即该章本身要学，故回退一位；
    // - 未给：用该书第一知识点位置的前一位置（saturate 至 0）。
    // 两个分支必须同为「前一位置」，否则用户选中的起始章会被静默跳过。
    let current_position = match start_position {
        Some(p) => Some(p.saturating_sub(1)),
        None => book_first_pos.map(|p| p.saturating_sub(1)),
    };

    let seq = file
        .data
        .goals
        .iter()
        .filter_map(|g| g.id.rsplit('-').next()?.parse::<usize>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let id = format!("goal-{}-{}-{}", key, slugify_book(&book_trimmed), seq);
    let goal = Goal {
        id,
        subject: subject.clone(),
        title,
        deadline: deadline.clone(),
        target_chapter: target_chapter.clone(),
        start_chapter: start_chapter_str,
        book: book_trimmed,
        current_position,
        target_position: Some(target_position),
        active: true,
        status: "active".to_string(),
    };
    file.data.goals.push(goal.clone());
    file.meta.generated_at = crate::data::now_string();
    save_goals(&data_dir, &file)?;
    Ok(goal)
}

/// 某书/板块在 chapter_seq 顺序表中的位置区间 `[min, max]`（含端点）。
///
/// 依据进度表中该书 chapter 节点下的知识点标题（无子节点时退化用 chapter 标题）
/// 逐个定位到顺序表，取最小 / 最大 position。
///
/// 返回 `None` 表示**无法确定区间**（进度表缺失、无该书、或该书下标题全未命中考纲），
/// 此时调用方跳过「章节归属校验」，避免因自定义进度表与官方考纲标题不一致而误伤创建。
fn book_position_range(
    data_dir: &std::path::Path,
    subject_key: &str,
    version: &str,
    book: &str,
) -> Option<(usize, usize)> {
    let idx = load_progress_index(data_dir);
    let set = idx.subjects.get(subject_key)?;
    let table = if !set.active_id.is_empty() {
        set.tables.iter().find(|t| t.id == set.active_id)
    } else {
        set.tables.first()
    }?;
    let chapter = table
        .nodes
        .iter()
        .find(|n| n.level == crate::data::progress_tables::NodeLevel::Chapter && n.title == book)?;
    let mut titles: Vec<String> = table
        .nodes
        .iter()
        .filter(|n| {
            n.level == crate::data::progress_tables::NodeLevel::Knowledge
                && n.parent_id.as_deref() == Some(chapter.id.as_str())
        })
        .map(|n| n.title.clone())
        .collect();
    if titles.is_empty() {
        // chapter 自身没有 knowledge 子节点：退化用 chapter 标题定位
        titles.push(chapter.title.clone());
    }
    let positions: Vec<usize> = titles
        .iter()
        .filter_map(|t| crate::core::chapter_seq::position(subject_key, version, t))
        .collect();
    let min = *positions.iter().min()?;
    let max = *positions.iter().max()?;
    Some((min, max))
}

/// 用进度表中该科目启用表的目标章节→book 节点关系，给出书本第一知识点的全局 position。
///
/// 取 `book_position_range` 的下界（即该书在考纲顺序表中最早的知识点）；
/// 若进度表无此书、或无法在 chapter_seq 顺序表中定位任一该书下的知识点，
/// 返回 None（create_goal/update_goal 会回退到 saturating_sub(1) 的兜底）。
fn book_first_position(
    data_dir: &std::path::Path,
    subject_key: &str,
    version: &str,
    book: &str,
) -> Option<usize> {
    book_position_range(data_dir, subject_key, version, book).map(|(min, _)| min)
}

/// 校验「章节 / 起始位置」确实属于所选书/板块。
///
/// 仅在能确定该书位置区间时才校验（`book_position_range` 返回 None 时放行）。
/// 前端两处 Select 已把选项限定在选中书内，此处是 Tauri invoke 层的兜底守卫，
/// 防止绕过 UI 直接传他书章节导致倒排区间跨书。
///
/// `target` / `start` 均为「(章节名, 顺序表位置)」元组：成对传入可避免两者错配。
fn ensure_positions_within_book(
    data_dir: &std::path::Path,
    subject_key: &str,
    version: &str,
    book: &str,
    target: (&str, usize),
    start: Option<(&str, usize)>,
) -> Result<(), String> {
    let Some((min, max)) = book_position_range(data_dir, subject_key, version, book) else {
        return Ok(());
    };
    let (target_chapter, target_position) = target;
    if target_position < min || target_position > max {
        return Err(format!(
            "目标章节「{}」不属于所选的「{}」，请重新选择该书的章节",
            target_chapter, book
        ));
    }
    if let Some((start_chapter, start_position)) = start {
        if start_position < min || start_position > max {
            return Err(format!(
                "起始位置「{}」不属于所选的「{}」，请重新选择该书的章节",
                start_chapter, book
            ));
        }
    }
    Ok(())
}

/// 把书名转成 id 友好的 slug（保留中文 + 数字 + 字母，去除其它字符）
fn slugify_book(book: &str) -> String {
    let mut out = String::new();
    for ch in book.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    if out.is_empty() {
        "book".to_string()
    } else {
        out
    }
}

/// 更新一条目标区间（就地 replace 并按字段重新校验）。
///
/// **每书唯一**：不允许把某书的目标改到另一已有目标的 (subject, book) 上。
/// 保存后按新截止日/新目标位置重算生效状态（过期或达标的目标顺延截止日即可恢复生效）。
/// 编辑 book 字段时，按新书重新计算 current_position 的兜底（取新书首知识点位置 - 1）。
///
/// 前端调用: `invoke('update_goal', { goal })`
#[tauri::command]
pub async fn update_goal(
    mut goal: Goal,
    state: State<'_, Mutex<AppState>>,
) -> Result<Goal, String> {
    if !goal.deadline.is_empty() {
        crate::data::validate_date(&goal.deadline)?;
    }
    if goal.book.trim().is_empty() {
        // 旧数据（book 为空的「整科一条」目标）编辑保存时必须补选所属书：
        // 保存是旧目标迁移到「每书唯一」新语义的入口，故此处强制非空并说明原因。
        return Err(
            "该目标未设置所属的书/板块（旧版数据）；请选择所属书后再保存，以便按书维护进度"
                .to_string(),
        );
    }

    let data_dir = get_data_dir(state.inner())?;
    let mut file = read_goals(&data_dir)?;
    let pos = file
        .data
        .goals
        .iter()
        .position(|g| g.id == goal.id)
        .ok_or_else(|| format!("目标 {} 不存在", goal.id))?;

    // 位置重新校验：目标章节必须命中考纲顺序表
    let key = subject_key_str(&goal.subject);
    let state_data = read_state_or_default(&data_dir);
    let version = subject_version(&state_data, key);
    let target_position = crate::core::chapter_seq::position(key, &version, &goal.target_chapter)
        .ok_or_else(|| {
        format!(
            "未能在地图中考纲顺序表中定位目标章节「{}」",
            goal.target_chapter
        )
    })?;
    goal.target_position = Some(target_position);

    // 起始位置的顺序表定位（与 create_goal 同一校验链：必须先命中考纲）
    let start_chapter_trimmed = goal.start_chapter.trim().to_string();
    let start_position = if start_chapter_trimmed.is_empty() {
        None
    } else {
        Some(
            crate::core::chapter_seq::position(key, &version, &start_chapter_trimmed).ok_or_else(
                || {
                    format!(
                        "未能在地图中考纲顺序表中定位起始章节「{}」",
                        goal.start_chapter
                    )
                },
            )?,
        )
    };

    // 归属守卫：目标章节 / 起始位置必须属于所选书（进度表能确定区间时才校验）
    ensure_positions_within_book(
        &data_dir,
        key,
        &version,
        &goal.book,
        (&goal.target_chapter, target_position),
        start_position.map(|p| (goal.start_chapter.as_str(), p)),
    )?;

    // 重新计算 current_position 的兜底
    // 语义统一：current_position = 「已完成到的位置」，倒排从 +1 开始派发。
    let prev = &file.data.goals[pos];
    let book_changed = prev.book != goal.book;
    match start_position {
        // 显式给了起点：UI 承诺「此前内容视为已学」，该章本身要学 → 回退一位
        Some(p) => goal.current_position = Some(p.saturating_sub(1)),
        None => {
            if book_changed || goal.current_position.is_none() {
                // 切书 / 首次补位置：按新书第一知识点位置的前一位置兜底
                goal.current_position = book_first_position(&data_dir, key, &version, &goal.book)
                    .map(|p| p.saturating_sub(1));
            }
        }
    }

    // 每书唯一守卫：改 subject/book 时不得撞上该组合已有的目标
    if prev.subject != goal.subject || prev.book != goal.book {
        if let Some(conflict) = goal_of_subject_book(&file, &goal.subject, &goal.book) {
            if conflict.id != goal.id {
                return Err(format!(
                    "{}「{}」已有目标计划「{}」，每书只允许一个。",
                    subject_display_zh(&goal.subject),
                    goal.book,
                    conflict.title
                ));
            }
        }
    }

    // 生命周期重算：每书唯一，无法用「删掉再建」重启目标，因此编辑即重启入口
    crate::data::goal::refresh_goal_lifecycle(&mut goal, &crate::data::today_string());

    file.data.goals[pos] = goal.clone();
    file.meta.generated_at = crate::data::now_string();
    save_goals(&data_dir, &file)?;
    Ok(goal)
}

/// 删除一条目标区间。
///
/// 前端调用: `invoke('delete_goal', { goalId })`
#[tauri::command]
pub async fn delete_goal(goal_id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
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
/// 若提供 `goal_id`，仅生成该条目标的任务；否则聚合该科目当天所有生效目标的任务
///（支持同一科目的不同书/板块并行推进）。当天不在任何倒排推进区间内则返回空数组。
/// 前端调用: `invoke('generate_goal_plan', { subject, date, goalId? })`
#[tauri::command]
pub async fn generate_goal_plan(
    subject: SubjectKey,
    date: String,
    goal_id: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<PlanTask>, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    let state_data = read_state_or_default(&data_dir);
    let key = subject_key_str(&subject);
    let version = subject_version(&state_data, key);

    // 收集要生成任务的目标：指定 id 时取一条；否则取当天该科目所有生效目标
    let goals: Vec<Goal> = if let Some(gid) = goal_id.as_deref() {
        let file = read_goals(&data_dir)?;
        let g = file
            .data
            .goals
            .iter()
            .find(|g| g.id == gid)
            .cloned()
            .ok_or_else(|| format!("目标 {} 不存在", gid))?;
        // 指定 id 时同样做生效校验，与「不指定 id」路径（active_goals_for_subject）口径一致：
        // - 未生效/已达标的目标不该产出任务；
        // - 截止日早于 date 的目标会让倒排区间为空（旧实现还会落入无终止的按日推进）。
        if !g.active {
            return Err(format!(
                "目标「{}」当前未生效（{}），无法生成 {} 的任务",
                g.title, g.status, date
            ));
        }
        if g.deadline.is_empty() || g.deadline.as_str() < date.as_str() {
            return Err(format!(
                "目标「{}」的截止日已过（{}），无法生成 {} 的任务",
                g.title, g.deadline, date
            ));
        }
        vec![g]
    } else {
        let goals_all = crate::data::goal::active_goals_for_subject(&data_dir, &subject, &date);
        if goals_all.is_empty() {
            return Err(format!(
                "{} 当天没有生效的目标区间",
                subject_display_zh(&subject)
            ));
        }
        goals_all
    };

    let mut all_tasks: Vec<PlanTask> = Vec::new();
    for goal in &goals {
        let tasks = crate::core::goal_planner::plan_goal_tasks(
            &data_dir,
            &ai_service,
            goal,
            &date,
            &version,
        )
        .await?;
        all_tasks.extend(tasks);
    }

    // 多书聚合后按全局序号重编任务 ID：
    // plan_goal_tasks* 内部对每个目标独立从 `{date}-01` 开始编号，直接聚合会出现同日同 ID
    //（前端落库/去重会互相覆盖）。这里统一重编，保证本命令返回的任务 ID 唯一。
    renumber_task_ids(&mut all_tasks, &date);
    Ok(all_tasks)
}

/// 按全局序号重编任务 ID 为 `{date}-{seq:02}`（原地修改）。
///
/// `plan_goal_tasks*` 对每个目标独立从 01 开始编号，多书聚合后会出现同日同 ID；
/// 该函数是 `generate_goal_plan` 的唯一收口点，保证返回值（及后续落库）的 ID 唯一。
pub(crate) fn renumber_task_ids(tasks: &mut [PlanTask], date: &str) {
    for (i, task) in tasks.iter_mut().enumerate() {
        task.id = format!("{}-{:02}", date, i + 1);
    }
}

fn subject_display_zh(subject: &SubjectKey) -> &'static str {
    match subject {
        SubjectKey::Math => "数学",
        SubjectKey::English => "英语",
        SubjectKey::Politics => "政治",
        SubjectKey::Professional => "专业课",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::progress_tables::{
        save_progress_index, NodeLevel, ProgressIndex, ProgressNode, ProgressTable,
        SubjectProgressSet,
    };

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "studyagent_goal_cmd_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ))
    }

    fn plan_task(id: &str) -> PlanTask {
        PlanTask {
            id: id.to_string(),
            subject: SubjectKey::Math,
            title: "t".to_string(),
            priority: crate::data::state::TaskPriority::A,
            estimated_hours: 1.0,
            goal: String::new(),
            completion_criteria: Vec::new(),
            textbook: None,
            style_tips: None,
            fallback_plan: None,
            status: crate::data::state::TaskStatus::Pending,
            dida_task_id: None,
        }
    }

    /// 多书聚合后的任务 ID 必须唯一（M1 回归）
    #[test]
    fn renumber_task_ids_dedups_across_books() {
        // 模拟两本书各自从 01 开始编号后的聚合结果：第 1 条与第 3 条撞号
        let mut tasks = vec![
            plan_task("2026-09-13-01"),
            plan_task("2026-09-13-02"),
            plan_task("2026-09-13-01"),
        ];
        renumber_task_ids(&mut tasks, "2026-09-13");
        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["2026-09-13-01", "2026-09-13-02", "2026-09-13-03"]);
    }

    /// 章节归属守卫：跨书章节必须被拒绝，同书章节放行（M13 回归）
    #[test]
    fn ensure_positions_within_book_rejects_cross_book_chapter() {
        let tmp = tmp_dir("ensure_book");
        std::fs::create_dir_all(&tmp).unwrap();

        // 用内置「数二」顺序表里的真实条目名构造进度表，保证 chapter_seq 能定位
        let version = "数二".to_string();
        let points = crate::core::chapter_seq::syllabus_points("math", &version)
            .expect("内置数二顺序表应存在");
        assert!(points.len() > 12, "顺序表条目过少，测试假设不成立");
        let item_a = points[3];
        let item_b = points[10];
        let pos_a = crate::core::chapter_seq::position("math", &version, item_a).unwrap();
        let pos_b = crate::core::chapter_seq::position("math", &version, item_b).unwrap();
        assert_ne!(pos_a, pos_b, "两条目位置应不同");

        let chapter = |id: &str, title: &str| ProgressNode {
            id: id.to_string(),
            title: title.to_string(),
            level: NodeLevel::Chapter,
            ..Default::default()
        };
        let knowledge = |id: &str, title: &str, parent: &str| ProgressNode {
            id: id.to_string(),
            title: title.to_string(),
            level: NodeLevel::Knowledge,
            parent_id: Some(parent.to_string()),
            ..Default::default()
        };
        let table = ProgressTable {
            id: "t-math".to_string(),
            subject: "math".to_string(),
            variant: version.clone(),
            name: "数学".to_string(),
            nodes: vec![
                chapter("ch-a", "书甲"),
                knowledge("kn-a", item_a, "ch-a"),
                chapter("ch-b", "书乙"),
                knowledge("kn-b", item_b, "ch-b"),
            ],
            ..Default::default()
        };
        let index = ProgressIndex {
            subjects: std::collections::HashMap::from([(
                "math".to_string(),
                SubjectProgressSet {
                    active_variant: version.clone(),
                    active_id: "t-math".to_string(),
                    tables: vec![table],
                },
            )]),
        };
        save_progress_index(&tmp, &index).unwrap();

        // 区间：该书下知识点的位置（单条目 → 区间退化为一个点）
        assert_eq!(
            book_position_range(&tmp, "math", &version, "书甲"),
            Some((pos_a, pos_a))
        );
        assert_eq!(
            book_first_position(&tmp, "math", &version, "书甲"),
            Some(pos_a)
        );

        // 同书 → 放行
        assert!(
            ensure_positions_within_book(&tmp, "math", &version, "书甲", (item_a, pos_a), None)
                .is_ok()
        );
        // 跨书目标章节 → 拒绝
        let err = ensure_positions_within_book(&tmp, "math", &version, "书甲", (item_b, pos_b), None)
            .expect_err("跨书章节必须被拒绝");
        assert!(err.contains("不属于所选的"), "错误信息应便于定位: {err}");
        // 跨书起始位置 → 拒绝
        assert!(ensure_positions_within_book(
            &tmp,
            "math",
            &version,
            "书乙",
            (item_b, pos_b),
            Some((item_a, pos_a))
        )
        .is_err());
        // 进度表里没有这本书 → 跳过校验（避免误伤自定义进度表）
        assert!(ensure_positions_within_book(
            &tmp,
            "math",
            &version,
            "不存在的书",
            (item_b, pos_b),
            None
        )
        .is_ok());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
