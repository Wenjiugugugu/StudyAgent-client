//! 运行时冒烟测试：用 `demo-data/` 的**真实数据**驱动核心链路。
//!
//! 与 `src` 内的单元测试分工不同：单元测试用构造数据穷举算法分支，
//! 这里验证「真实输入 → 真实产出」的完整性——真实周计划能否排出日计划、
//! 已完成内容过滤是否生效、多科任务在预算约束下是否都被保留、
//! 目标倒排是否覆盖用户选中的起始章。
//!
//! 说明：不依赖 Tauri 运行时（命令层需要 `State<Mutex<AppState>>`，测试无法构造），
//! 因此覆盖的是命令层内部逻辑所依赖的 core / data 公共 API。

use std::path::{Path, PathBuf};

use studyagent_desktop_lib::core::goal_planner;
use studyagent_desktop_lib::core::scheduler::DailyScheduler;
use studyagent_desktop_lib::data::goal::Goal;
use studyagent_desktop_lib::data::iso_week_string;
use studyagent_desktop_lib::data::plan::read_week_plan;
use studyagent_desktop_lib::data::state::{read_state_or_default, SubjectKey};

const SMOKE_DATE: &str = "2026-07-28";

fn demo_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = desktop/src-tauri
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo-data")
}

/// demo-data 位于**仓库之外**（仓库根是 desktop/），因此在 CI 或他人 clone 上并不存在。
/// 这些用例只在具备该演示数据的本机开发环境运行，缺失时跳过而不是失败。
macro_rules! require_demo_data {
    () => {
        if !demo_dir().exists() {
            eprintln!(
                "跳过 {}：未找到 demo-data（{:?}）",
                module_path!(),
                demo_dir()
            );
            return;
        }
    };
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

/// 把 demo-data 复制到临时目录再跑（`generate_daily_plan` 会写文件，不能污染源数据）
fn tmp_demo_copy(tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!(
        "studyagent_smoke_{}_{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    copy_dir(&demo_dir(), &dst);
    dst
}

/// 清空 state 中所有 `completed = [...]`（只改数组内容，TOML 结构不变）。
///
/// 用来构造「没有任何已完成内容」的场景，从而观察多科任务在预算约束下的真实分配。
fn clear_completed(state_path: &Path) {
    let text = std::fs::read_to_string(state_path).unwrap();
    let key = "completed = [";
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(pos) = rest.find(key) {
        let (before, after) = rest.split_at(pos + key.len());
        out.push_str(before);
        match after.find(']') {
            Some(end) => {
                out.push(']');
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    std::fs::write(state_path, out).unwrap();
}

/// 真实数据：过滤已完成内容 + 任务 ID 唯一 + 总时长不超预算
#[test]
fn demo_data_daily_plan_filters_completed_and_fits_budget() {
    require_demo_data!();
    let dir = tmp_demo_copy("daily");

    let plan = DailyScheduler::generate_daily_plan(&dir, SMOKE_DATE, false)
        .expect("真实周计划应能生成日计划");
    assert!(!plan.data.tasks.is_empty(), "真实数据应产出任务");
    assert!(plan.data.total_hours > 0.0, "总时长应大于 0");

    // 任务 ID 唯一（落库/去重前提）
    let mut ids: Vec<&str> = plan.data.tasks.iter().map(|t| t.id.as_str()).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "日计划任务 ID 不应重复");

    // 预算约束（两位小数舍入留 0.05 容差）
    let budget = studyagent_desktop_lib::load_settings(&dir).daily_target_hours();
    assert!(budget > 0.0, "demo-data 应配置每日目标时长");
    assert!(
        plan.data.total_hours <= budget + 0.05,
        "总时长 {:.2}h 超过预算 {:.2}h",
        plan.data.total_hours,
        budget
    );

    // 任务数不得超过周计划当天给出的模板数（不应凭空产生任务）
    let iso_week = iso_week_string(SMOKE_DATE).unwrap();
    let week = read_week_plan(&dir, &iso_week).unwrap();
    let day = week
        .data
        .days
        .iter()
        .find(|d| d.date == SMOKE_DATE)
        .expect("周计划应含当天");
    let planned_templates: usize = day
        .subject_allocations
        .iter()
        .map(|a| a.task_templates.len())
        .sum();
    assert!(
        plan.data.tasks.len() <= planned_templates,
        "生成 {} 条任务，超过周计划当天的 {} 条模板",
        plan.data.tasks.len(),
        planned_templates
    );

    // 「防重复安排已完成内容」必须生效：所有任务标题都不在 state.completed 中
    let state = read_state_or_default(&dir);
    let mut completed: Vec<String> = Vec::new();
    for subj in [
        &state.subjects.math,
        &state.subjects.english,
        &state.subjects.politics,
        &state.subjects.professional,
    ] {
        completed.extend(subj.completed.iter().cloned());
    }
    for task in &plan.data.tasks {
        assert!(
            !completed.iter().any(|c| c == &task.title),
            "任务「{}」已完成却仍被排入日计划",
            task.title
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 清空已完成记录后：多科任务应全部保留（每科保底 1 条），
/// 且总时长超预算时走「等比压缩」而非删除整科任务（M3 护栏的真实数据验证）
#[test]
fn demo_data_daily_plan_keeps_all_subjects_when_nothing_completed() {
    require_demo_data!();
    let dir = tmp_demo_copy("daily_clear");
    clear_completed(&dir.join("state").join("current.state"));

    let plan = DailyScheduler::generate_daily_plan(&dir, SMOKE_DATE, false)
        .expect("清空已完成内容后仍应能生成日计划");

    let mut subjects: Vec<String> = plan
        .data
        .tasks
        .iter()
        .map(|t| format!("{:?}", t.subject))
        .collect();
    subjects.sort();
    subjects.dedup();
    assert_eq!(
        subjects.len(),
        3,
        "周计划当天给了 3 科各 1 条任务，清空已完成后应全部保留，实际 {subjects:?}"
    );
    assert_eq!(plan.data.tasks.len(), 3, "每科恰好 1 条任务");

    let budget = studyagent_desktop_lib::load_settings(&dir).daily_target_hours();
    assert!(
        plan.data.total_hours <= budget + 0.05,
        "压缩后总时长 {:.2}h 应回到预算 {:.2}h 附近",
        plan.data.total_hours,
        budget
    );
    // 3 条任务原始总时长 > 预算时，应留下用户可见的降级提示
    assert!(
        !plan.data.warnings.is_empty(),
        "超预算处理应写入 warnings 供前端展示"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 目标倒排：H1 修复后的语义（`current_position = position(start_chapter) - 1`）
/// 必须让用户选中的起始章在第一个学习日就被安排
#[test]
fn goal_backward_schedule_includes_start_chapter_on_first_day() {
    require_demo_data!();
    let dir = tmp_demo_copy("goal");

    let state = read_state_or_default(&dir);
    let version = state.subjects.math.version.clone().unwrap_or_default();
    assert!(!version.is_empty(), "demo-data 数学应有版本（数二）");

    let points = studyagent_desktop_lib::core::chapter_seq::syllabus_points("math", &version)
        .expect("内置数学顺序表应存在");
    let idx = 10.min(points.len() - 1);
    assert!(idx >= 1, "顺序表条目不足，测试假设不成立");
    let start_chapter = points[idx].to_string();
    let start_pos =
        studyagent_desktop_lib::core::chapter_seq::position("math", &version, &start_chapter)
            .expect("起始章应能在顺序表中定位");

    // 模拟修复后的 create_goal：起始章本身要学 → current_position = position - 1
    let goal = Goal {
        id: "goal-math-smoke-1".to_string(),
        subject: SubjectKey::Math,
        title: "冒烟：从指定章开始".to_string(),
        deadline: "2026-07-29".to_string(),
        target_chapter: start_chapter.clone(),
        start_chapter: start_chapter.clone(),
        book: String::new(),
        current_position: Some(start_pos.saturating_sub(1)),
        target_position: Some(start_pos),
        active: true,
        status: "active".to_string(),
    };

    let tasks = goal_planner::plan_goal_tasks_sync(&dir, &goal, SMOKE_DATE, &version)
        .expect("应能生成倒排任务");
    assert!(!tasks.is_empty(), "目标区间内当天应有任务");
    assert!(
        tasks.iter().any(|t| t.title.contains(&start_chapter)),
        "起始章「{start_chapter}」必须出现在当天任务中，实际 {:?}",
        tasks.iter().map(|t| t.title.clone()).collect::<Vec<_>>()
    );

    // 反例：旧的（错误）语义 current_position = position(start_chapter)
    // 会让起始章当天不被安排——本断言用于证明修复的必要性
    let mut wrong = goal.clone();
    wrong.current_position = Some(start_pos);
    let tasks_wrong = goal_planner::plan_goal_tasks_sync(&dir, &wrong, SMOKE_DATE, &version)
        .expect("应能生成倒排任务");
    assert!(
        !tasks_wrong.iter().any(|t| t.title.contains(&start_chapter)),
        "旧语义下起始章不应被安排（本次用例即复现该偏差）"
    );

    // 过期截止日：不得死循环，应返回空
    let mut expired = goal.clone();
    expired.deadline = "2026-07-01".to_string();
    let tasks_expired = goal_planner::plan_goal_tasks_sync(&dir, &expired, SMOKE_DATE, &version)
        .expect("过期目标应安全返回而非挂死");
    assert!(tasks_expired.is_empty(), "过期目标不应产出任务");

    let _ = std::fs::remove_dir_all(&dir);
}
