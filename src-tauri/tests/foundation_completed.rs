use studyagent_desktop_lib::{core::scheduler::DailyScheduler, data};

/// 在实际安装数据的副本上检查完整日计划生成路径。必须显式提供副本目录。
#[test]
#[ignore]
fn installed_data_copy_does_not_reschedule_completed_foundation() {
    let dir = std::path::PathBuf::from(std::env::var("STUDYAGENT_TEST_DATA_COPY").unwrap());
    assert!(dir
        .canonicalize()
        .unwrap()
        .starts_with(std::env::temp_dir().canonicalize().unwrap()));
    let before = data::plan::read_daily_plan(&dir, "2026-10-12").unwrap();
    let stale = [
        "数据链路层·封装成帧与透明传输（字符计数、字节填充、零比特填充）",
        "数据链路层·差错检测：奇偶校验与 CRC 循环冗余检验",
        "计算机网络·物理层：非导向传输媒体与信道复用补做",
    ];
    assert!(before
        .data
        .tasks
        .iter()
        .any(|t| t.subject.key() == "professional"));
    let index_before = data::progress_tables::load_progress_index(&dir);
    let after = DailyScheduler::generate_daily_plan(&dir, "2026-10-12", false).unwrap();
    let tasks: Vec<_> = after
        .data
        .tasks
        .iter()
        .chain(&after.data.deferred_tasks)
        .collect();
    for title in stale {
        assert!(
            !tasks.iter().any(|t| t.title == title),
            "旧基础任务仍在排程：{title}"
        );
    }
    let professional: Vec<_> = tasks
        .iter()
        .filter(|t| t.subject.key() == "professional")
        .collect();
    assert!(!professional.is_empty());
    for task in professional {
        assert!(
            task.title.starts_with("数据结构"),
            "错误的强化起点：{}",
            task.title
        );
        assert!(
            task.knowledge_points
                .as_ref()
                .is_some_and(|p| !p.is_empty()),
            "没有知识点：{}",
            task.title
        );
    }
    let index_after = data::progress_tables::load_progress_index(&dir);
    for (key, set) in &index_before.subjects {
        for table in &set.tables {
            let new_table = index_after.subjects[key]
                .tables
                .iter()
                .find(|t| t.id == table.id)
                .unwrap();
            for node in &table.nodes {
                assert_eq!(
                    node.status,
                    new_table
                        .nodes
                        .iter()
                        .find(|n| n.id == node.id)
                        .unwrap()
                        .status
                );
            }
        }
    }
    std::fs::write(
        dir.join("foundation-fix-preview.json"),
        serde_json::to_vec_pretty(&after).unwrap(),
    )
    .unwrap();
    println!(
        "修复前 {} 条 / {:.1}h；修复后 {} 条 / {:.1}h",
        before.data.tasks.len(),
        before.data.total_hours,
        after.data.tasks.len(),
        after.data.total_hours
    );
}
