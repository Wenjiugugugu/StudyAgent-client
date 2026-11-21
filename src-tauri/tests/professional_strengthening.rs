use data::progress_tables::{NodeStatus, ProgressIndex, SubjectProgressSet};
use studyagent_desktop_lib::{
    core::{professional, progress_sync, task_progress},
    data,
};

fn index() -> ProgressIndex {
    let mut tables = professional::build_tables(&professional::find("408").unwrap());
    for (i, table) in tables.iter_mut().enumerate() {
        table.id = format!("t{i}");
        for (j, node) in table.nodes.iter_mut().enumerate() {
            node.id = format!("n{i}-{j}");
            node.status = NodeStatus::Basic;
        }
    }
    let mut index = ProgressIndex::default();
    index.subjects.insert(
        "professional".into(),
        SubjectProgressSet {
            active_id: tables[0].id.clone(),
            active_variant: tables[0].variant.clone(),
            tables,
        },
    );
    index
}

fn temp() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sa_professional_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn textbook_links_cover_specific_chapters_and_composite_reviews() {
    let index = index();
    for title in [
        "计算机网络·物理层：非导向传输媒体（无线电波、微波与红外线）",
        "计算机网络·物理层：数字传输系统（PCM 与 SONET/SDH 概述）",
        "物理层真题二刷：传输媒体、奈氏准则与香农定理",
        "计算机网络·应用层：DNS 与 HTTP 真题二刷",
        "计算机网络限时模考：全科综合（二）",
    ] {
        let points = progress_sync::infer_task_points(&index, "professional", title)
            .unwrap_or_else(|| panic!("没有关联：{title}"));
        assert!(!points.is_empty());
        for point in points {
            let table = index.subjects["professional"]
                .tables
                .iter()
                .find(|t| t.id == point.table_id)
                .unwrap();
            let node = table.nodes.iter().find(|n| n.id == point.node_id).unwrap();
            assert!(
                table.name.contains("计算机网络") || node.phase == "计算机网络",
                "跨教材误关联：{title}"
            );
        }
    }
    let specific = progress_sync::infer_task_points(
        &index,
        "professional",
        "计算机网络·物理层：非导向传输媒体",
    )
    .unwrap();
    assert!(specific.iter().any(|p| p.title.contains("传输媒体")));
    assert!(specific.iter().all(|p| index.subjects["professional"]
        .tables
        .iter()
        .any(|t| t.id == p.table_id && t.name.contains("教材："))));
}

#[test]
fn strengthening_starts_with_data_structures_then_follows_mastered_progress() {
    let mut index = index();
    let dir = temp();
    let today = data::today_string();
    let (module, points) = task_progress::professional_strengthening_points(&dir, &index, &today)
        .unwrap()
        .unwrap();
    assert_eq!(module, "数据结构");
    assert!(!points.is_empty());
    for table in &mut index.subjects.get_mut("professional").unwrap().tables {
        for node in &mut table.nodes {
            if node.phase == "数据结构" || table.name.contains("教材：数据结构") {
                node.status = NodeStatus::Mastered;
            }
        }
    }
    assert_eq!(
        task_progress::professional_strengthening_points(&dir, &index, &today)
            .unwrap()
            .unwrap()
            .0,
        "计算机组成原理"
    );
    data::study_direction::update_direction(
        &dir,
        data::study_direction::ActualStudyEntry {
            subject: "professional".into(),
            content: "先做计算机网络强化".into(),
            scope: data::study_direction::StudyScope::Ongoing,
            ..Default::default()
        },
        "",
    )
    .unwrap();
    assert!(
        task_progress::professional_strengthening_points(&dir, &index, &today)
            .unwrap()
            .is_none()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn week_candidates_are_corrected_before_daily_generation_without_changing_hours() {
    let index = index();
    let dir = temp();
    let mut week = data::plan::WeekPlanFile {
        view: Some("计算机网络旧安排".into()),
        ..Default::default()
    };
    week.data.days.push(data::plan::WeekDayPlan {
        date: data::today_string(),
        subject_allocations: vec![data::plan::DaySubjectAllocation {
            subject: data::state::SubjectKey::Professional,
            hours: 1.3,
            task_templates: vec![
                data::plan::TaskTemplate {
                    title: "计算机网络·物理层：非导向传输媒体".into(),
                    estimated_hours: 0.5,
                    ..Default::default()
                },
                data::plan::TaskTemplate {
                    title: "计算机网络限时模考：全科综合".into(),
                    estimated_hours: 0.8,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    });
    task_progress::normalize_professional_strengthening_week(&dir, &index, &mut week, None)
        .unwrap();
    let tasks = &week.data.days[0].subject_allocations[0].task_templates;
    assert_eq!(tasks.iter().map(|t| t.estimated_hours).sum::<f64>(), 1.3);
    assert!(tasks.iter().all(|t| t.title.starts_with("数据结构")
        && t.knowledge_points.as_ref().is_some_and(|p| !p.is_empty())));
    assert!(week.view.is_none());
    let titles: Vec<_> = tasks.iter().map(|t| t.title.clone()).collect();
    task_progress::normalize_professional_strengthening_week(&dir, &index, &mut week, None)
        .unwrap();
    assert_eq!(
        titles,
        week.data.days[0].subject_allocations[0]
            .task_templates
            .iter()
            .map(|t| t.title.clone())
            .collect::<Vec<_>>()
    );
}
