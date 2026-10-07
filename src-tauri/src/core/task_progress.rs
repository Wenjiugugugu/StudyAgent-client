//! 任务与知识点关联、复盘范围及剩余内容。None 保留旧数据标题匹配语义。
use crate::data::plan::{KnowledgePointRef, PlanTask, TaskTemplate, WeekPlanFile};
use crate::data::progress_tables::{NodeLevel, NodeStatus, ProgressIndex};
use crate::data::records::TaskReviewEntry;

pub fn same_point(a: &KnowledgePointRef, b: &KnowledgePointRef) -> bool {
    a.table_id == b.table_id && a.node_id == b.node_id
}

pub fn validate_points(
    index: &ProgressIndex,
    subject: &str,
    points: &[KnowledgePointRef],
) -> Result<Vec<KnowledgePointRef>, String> {
    let set = index
        .subjects
        .get(subject)
        .ok_or("该科目还没有进度表，请先创建进度表")?;
    let mut result = Vec::new();
    for point in points {
        let table = set
            .tables
            .iter()
            .find(|t| t.id == point.table_id)
            .ok_or("关联的进度表已不存在，请重新选择知识点")?;
        let node = table
            .nodes
            .iter()
            .find(|n| n.id == point.node_id && n.level == NodeLevel::Knowledge)
            .ok_or("关联的知识点已不存在，请重新选择")?;
        if !result.iter().any(|p| same_point(p, point)) {
            result.push(KnowledgePointRef {
                table_id: table.id.clone(),
                node_id: node.id.clone(),
                title: node.title.clone(),
            });
        }
    }
    Ok(result)
}

pub fn bind_task(index: &ProgressIndex, task: &mut PlanTask) {
    if task.knowledge_points.is_none() {
        task.knowledge_points =
            super::progress_sync::infer_task_points(index, task.subject.key(), &task.title);
    }
}

/// 安排日期与学习状态分开保存；生成计划不会将待学节点标为学习中。
pub fn record_planned_dates(
    index: &mut ProgressIndex,
    tasks: &[PlanTask],
    date: &str,
    today: &str,
) -> bool {
    let mut changed = false;
    for task in tasks {
        let Some(points) = &task.knowledge_points else {
            continue;
        };
        let Some(set) = index.subjects.get_mut(task.subject.key()) else {
            continue;
        };
        for table in &mut set.tables {
            for node in &mut table.nodes {
                if points
                    .iter()
                    .any(|p| p.table_id == table.id && p.node_id == node.id)
                    && node
                        .planned_date
                        .as_deref()
                        .is_none_or(|old| old < today || date < old)
                {
                    node.planned_date = Some(date.into());
                    changed = true;
                }
            }
        }
    }
    changed
}

pub fn bind_week(index: &ProgressIndex, week: &mut WeekPlanFile) {
    for day in &mut week.data.days {
        for allocation in &mut day.subject_allocations {
            for task in &mut allocation.task_templates {
                if task.knowledge_points.is_none() {
                    task.knowledge_points = super::progress_sync::infer_task_points(
                        index,
                        allocation.subject.key(),
                        &task.title,
                    );
                }
            }
        }
    }
}

/// 以日计划中的关联为准；完成比例由实际勾选数确定，不信任前端百分比。
pub fn normalize_review(
    index: &ProgressIndex,
    task: &PlanTask,
    entry: &mut TaskReviewEntry,
) -> Result<(), String> {
    entry.title = task.title.clone();
    entry.subject = task.subject.key().to_string();
    entry.priority = format!("{:?}", task.priority);
    entry.knowledge_points = task.knowledge_points.clone();
    let Some(all) = &task.knowledge_points else {
        if entry
            .completed_knowledge_points
            .as_ref()
            .is_some_and(|p| !p.is_empty())
        {
            return Err("请先关联任务的知识点，再记录部分完成".into());
        }
        entry.completed_knowledge_points = None;
        return Ok(());
    };
    let selected = if entry.status == "completed" {
        all.clone()
    } else {
        entry.completed_knowledge_points.clone().unwrap_or_default()
    };
    if selected
        .iter()
        .any(|p| !all.iter().any(|a| same_point(a, p)))
    {
        return Err("已完成范围超出了任务关联的知识点，请重新选择".into());
    }
    let done = if selected.is_empty() {
        Vec::new()
    } else {
        validate_points(index, task.subject.key(), &selected)?
    };
    if !all.is_empty() {
        entry.completion = done.len() as f64 / all.len() as f64;
        entry.status = if done.is_empty() {
            "incomplete"
        } else if done.len() == all.len() {
            "completed"
        } else {
            "partial"
        }
        .into();
    }
    entry.completed_knowledge_points = Some(done);
    Ok(())
}

pub fn apply_completed_points(
    index: &mut ProgressIndex,
    subject: &str,
    points: &[KnowledgePointRef],
) -> usize {
    let Some(set) = index.subjects.get_mut(subject) else {
        return 0;
    };
    let mut changed = 0;
    let mut tables = Vec::new();
    for table in &mut set.tables {
        let mut touched = false;
        for node in &mut table.nodes {
            if node.level == NodeLevel::Knowledge
                && points
                    .iter()
                    .any(|p| p.table_id == table.id && p.node_id == node.id)
            {
                let next = node.status.max_with(NodeStatus::Basic);
                if next != node.status {
                    node.status = next;
                    changed += 1;
                    touched = true;
                }
            }
        }
        if touched {
            table.updated_at = crate::data::now_string();
            tables.push((table.variant.clone(), table.name.clone()));
        }
    }
    if subject == "professional" {
        for (variant, name) in tables {
            changed += super::progress_sync::sync_professional_table_change(index, &variant, &name);
        }
    }
    changed
}

pub fn remaining_points(entry: &TaskReviewEntry) -> Option<Vec<KnowledgePointRef>> {
    let all = entry.knowledge_points.as_ref()?;
    let done = entry.completed_knowledge_points.as_ref()?;
    Some(
        all.iter()
            .filter(|p| !done.iter().any(|d| same_point(p, d)))
            .cloned()
            .collect(),
    )
}

/// 用户又按最远位置补记时，已经被明确补为基础的范围不再作为积压重排。
/// 只调整排程输入，不覆盖复盘中原先勾选的历史范围。
pub fn scheduling_entries(
    index: &ProgressIndex,
    entries: &[TaskReviewEntry],
    declared: &[crate::data::records::OvercompletionEntry],
) -> Vec<TaskReviewEntry> {
    let mut entries = entries.to_vec();
    for entry in entries.iter_mut().filter(|e| e.status == "partial") {
        let (Some(all), Some(done)) = (
            &entry.knowledge_points,
            &mut entry.completed_knowledge_points,
        ) else {
            continue;
        };
        let Some(set) = index.subjects.get(&entry.subject) else {
            continue;
        };
        for point in all {
            let Some(table) = set.tables.iter().find(|t| t.id == point.table_id) else {
                continue;
            };
            let Some(position) = table
                .nodes
                .iter()
                .position(|n| n.id == point.node_id && n.status.rank() >= NodeStatus::Basic.rank())
            else {
                continue;
            };
            let covered = declared
                .iter()
                .filter(|oc| oc.subject == entry.subject)
                .any(|oc| {
                    let reached = table.nodes.iter().position(|n| {
                        if oc.node_id.is_empty() {
                            n.title == oc.chapter_reached
                        } else {
                            n.id == oc.node_id
                        }
                    });
                    reached.is_some_and(|i| {
                        let node = &table.nodes[i];
                        let end = if node.level == NodeLevel::Chapter {
                            table
                                .nodes
                                .iter()
                                .enumerate()
                                .filter(|(_, n)| n.parent_id.as_deref() == Some(&node.id))
                                .map(|(i, _)| i)
                                .max()
                                .unwrap_or(i)
                        } else {
                            i
                        };
                        position <= end
                    })
                });
            if covered && !done.iter().any(|d| same_point(d, point)) {
                done.push(point.clone());
            }
        }
        if !all.is_empty() {
            entry.completion = done.len() as f64 / all.len() as f64;
            if done.len() == all.len() {
                entry.status = "completed".into();
            }
        }
    }
    entries
}

pub fn remaining_title(entry: &TaskReviewEntry) -> String {
    match remaining_points(entry) {
        Some(points) if !points.is_empty() => format!(
            "继续学习：{}",
            points
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>()
                .join("、")
        ),
        _ => entry.title.clone(),
    }
}

pub fn remaining_template(entry: &TaskReviewEntry) -> TaskTemplate {
    TaskTemplate {
        remaining_from_task_id: remaining_points(entry)
            .filter(|p| !p.is_empty())
            .map(|_| entry.task_id.clone()),
        title: remaining_title(entry),
        knowledge_points: remaining_points(entry).filter(|p| !p.is_empty()),
        estimated_hours: entry.estimated_hours.unwrap_or(0.5)
            * (1.0 - entry.completion.clamp(0.0, 1.0)),
        goal: "完成剩余学习内容".into(),
        ..Default::default()
    }
}

pub fn subtract_task_points(task: &mut PlanTask, done: &[KnowledgePointRef]) {
    let Some(points) = &mut task.knowledge_points else {
        return;
    };
    let before = points.len();
    if before == 0 {
        return;
    }
    points.retain(|p| !done.iter().any(|d| same_point(p, d)));
    if points.len() != before {
        task.estimated_hours *= points.len() as f64 / before as f64;
        task.title = format!(
            "继续学习：{}",
            points
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        task.goal = "完成剩余学习内容".into();
        task.completion_criteria = points
            .iter()
            .map(|p| format!("完成 {} 的学习", p.title))
            .collect();
    }
}

/// 不论 AI 是否遗漏，只要还有学习日，显式剩余范围必须有安排。
pub fn ensure_remaining_ranges(
    week: &mut WeekPlanFile,
    entries: &[TaskReviewEntry],
    dates: &[String],
    daily: Option<&crate::data::plan::DailyPlanFile>,
) {
    for entry in entries.iter().filter(|e| e.status == "partial") {
        let Some(remaining) = remaining_points(entry) else {
            continue;
        };
        if remaining.is_empty() {
            continue;
        }
        let scheduled: Vec<_> = week
            .data
            .days
            .iter()
            .filter(|d| dates.contains(&d.date))
            .flat_map(|d| &d.subject_allocations)
            .filter(|a| a.subject.key() == entry.subject)
            .flat_map(|a| &a.task_templates)
            .flat_map(|t| t.knowledge_points.iter().flatten().cloned())
            .collect();
        let missing: Vec<_> = remaining
            .into_iter()
            .filter(|p| !scheduled.iter().any(|s| same_point(s, p)))
            .collect();
        if missing.is_empty() {
            continue;
        }
        let Some(day) = week
            .data
            .days
            .iter_mut()
            .find(|d| dates.contains(&d.date) && !d.is_rest_day)
        else {
            continue;
        };
        let mut template = remaining_template(entry);
        let all_count = entry.knowledge_points.as_ref().map_or(1, Vec::len).max(1);
        let hours = daily
            .and_then(|p| p.data.tasks.iter().find(|t| t.id == entry.task_id))
            .map(|t| t.estimated_hours)
            .or(entry.estimated_hours)
            .unwrap_or(0.5);
        template.estimated_hours = hours * missing.len() as f64 / all_count as f64;
        template.title = format!(
            "继续学习：{}",
            missing
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        template.completion_criteria = missing
            .iter()
            .map(|p| format!("完成 {} 的学习", p.title))
            .collect();
        template.knowledge_points = Some(missing);
        let subject = crate::data::state::SubjectKey::ALL
            .iter()
            .find(|s| s.key() == entry.subject)
            .cloned();
        let Some(subject) = subject else { continue };
        if !day.subject_allocations.iter().any(|a| a.subject == subject) {
            day.subject_allocations
                .push(crate::data::plan::DaySubjectAllocation {
                    subject: subject.clone(),
                    ..Default::default()
                });
        }
        let allocation = day
            .subject_allocations
            .iter_mut()
            .find(|a| a.subject == subject)
            .unwrap();
        allocation.task_templates.insert(0, template);
        allocation.hours = allocation
            .task_templates
            .iter()
            .map(|t| t.estimated_hours)
            .sum();
    }
    // AI 正确返回剩余范围也要保留来源，让目标倒排和积压过滤不会吞掉它。
    for day in week
        .data
        .days
        .iter_mut()
        .filter(|d| dates.contains(&d.date))
    {
        for allocation in &mut day.subject_allocations {
            for task in &mut allocation.task_templates {
                for entry in entries
                    .iter()
                    .filter(|e| e.status == "partial" && e.subject == allocation.subject.key())
                {
                    if remaining_points(entry).is_some_and(|remaining| {
                        task.knowledge_points.as_ref().is_some_and(|points| {
                            points
                                .iter()
                                .any(|p| remaining.iter().any(|r| same_point(p, r)))
                        })
                    }) {
                        task.remaining_from_task_id = Some(entry.task_id.clone());
                        if let Some(original) =
                            daily.and_then(|p| p.data.tasks.iter().find(|t| t.id == entry.task_id))
                        {
                            let points = task.knowledge_points.as_ref().unwrap();
                            if points.iter().all(|p| {
                                entry
                                    .knowledge_points
                                    .as_ref()
                                    .is_some_and(|all| all.iter().any(|a| same_point(a, p)))
                            }) {
                                task.estimated_hours = original.estimated_hours
                                    * points.len() as f64
                                    / entry.knowledge_points.as_ref().map_or(1, Vec::len).max(1)
                                        as f64;
                            }
                        }
                    }
                }
            }
            allocation.hours = allocation
                .task_templates
                .iter()
                .map(|t| t.estimated_hours)
                .sum();
        }
    }
}

/// AI 重排返回整项内容时，确定性扣除本次已确认的范围。
pub fn remove_completed_ranges(
    index: &ProgressIndex,
    week: &mut WeekPlanFile,
    entries: &[TaskReviewEntry],
    dates: &[String],
) {
    bind_week(index, week);
    for day in &mut week.data.days {
        if !dates.contains(&day.date) {
            continue;
        }
        for allocation in &mut day.subject_allocations {
            for task in &mut allocation.task_templates {
                let Some(points) = &mut task.knowledge_points else {
                    continue;
                };
                let before = points.len();
                if before == 0 {
                    continue;
                }
                points.retain(|p| {
                    !entries
                        .iter()
                        .filter(|e| e.subject == allocation.subject.key())
                        .filter_map(|e| e.completed_knowledge_points.as_ref())
                        .flatten()
                        .any(|d| same_point(p, d))
                });
                if points.len() < before {
                    task.estimated_hours *= points.len() as f64 / before as f64;
                    task.title = format!(
                        "继续学习：{}",
                        points
                            .iter()
                            .map(|p| p.title.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    );
                    task.goal = "完成剩余学习内容".into();
                    task.completion_criteria = points
                        .iter()
                        .map(|p| format!("完成 {} 的学习", p.title))
                        .collect();
                }
            }
            allocation.task_templates.retain(|t| {
                !t.knowledge_points.as_ref().is_some_and(|p| p.is_empty())
                    || t.estimated_hours > 0.0
            });
            allocation.hours = allocation
                .task_templates
                .iter()
                .map(|t| t.estimated_hours)
                .sum();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::plan::{DailyPlanFile, DaySubjectAllocation, WeekDayPlan};
    use crate::data::progress_tables::{ProgressNode, ProgressTable, SubjectProgressSet};
    use crate::data::state::SubjectKey;

    fn fixture() -> (ProgressIndex, PlanTask) {
        let mut index = ProgressIndex::default();
        let nodes: Vec<_> = ["函数的概念", "函数的极限", "函数的连续性"]
            .iter()
            .enumerate()
            .map(|(i, title)| ProgressNode {
                id: format!("n{i}"),
                title: title.to_string(),
                ..Default::default()
            })
            .collect();
        index.subjects.insert(
            "math".into(),
            SubjectProgressSet {
                active_id: "t1".into(),
                tables: vec![
                    ProgressTable {
                        id: "t1".into(),
                        subject: "math".into(),
                        nodes: nodes.clone(),
                        ..Default::default()
                    },
                    ProgressTable {
                        id: "t2".into(),
                        subject: "math".into(),
                        nodes,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        let points = index.subjects["math"].tables[0]
            .nodes
            .iter()
            .map(|n| KnowledgePointRef {
                table_id: "t1".into(),
                node_id: n.id.clone(),
                title: n.title.clone(),
            })
            .collect();
        (
            index,
            PlanTask {
                id: "2026-10-06-01".into(),
                subject: SubjectKey::Math,
                title: "完全不同的任务标题".into(),
                estimated_hours: 3.0,
                knowledge_points: Some(points),
                ..Default::default()
            },
        )
    }

    fn partial(task: &PlanTask) -> TaskReviewEntry {
        TaskReviewEntry {
            task_id: task.id.clone(),
            status: "partial".into(),
            completion: 0.99,
            completed_knowledge_points: Some(vec![
                task.knowledge_points.as_ref().unwrap()[1].clone()
            ]),
            ..Default::default()
        }
    }

    #[test]
    fn partial_sync_uses_ids_and_only_confirmed_range_without_downgrade() {
        let (mut index, task) = fixture();
        let mut entry = partial(&task);
        normalize_review(&index, &task, &mut entry).unwrap();
        assert_eq!(entry.status, "partial");
        let review = crate::data::records::ReviewFile {
            task_reviews: vec![entry.clone()],
            ..Default::default()
        };
        assert!(
            (crate::data::records::review_completion_stats(&review).4 - 100.0 / 3.0).abs() < 0.001
        );
        assert!((entry.completion - 1.0 / 3.0).abs() < 0.001);
        assert_eq!(remaining_points(&entry).unwrap().len(), 2);
        assert_eq!(
            apply_completed_points(
                &mut index,
                "math",
                entry.completed_knowledge_points.as_ref().unwrap()
            ),
            1
        );
        let tables = &index.subjects["math"].tables;
        assert_eq!(tables[0].nodes[0].status, NodeStatus::Pending);
        assert_eq!(tables[0].nodes[1].status, NodeStatus::Basic);
        assert_eq!(tables[0].nodes[2].status, NodeStatus::Pending);
        assert!(
            tables[1]
                .nodes
                .iter()
                .all(|n| n.status == NodeStatus::Pending),
            "同编号的另一张表不能被改动"
        );
        index.subjects.get_mut("math").unwrap().tables[0].nodes[1].status = NodeStatus::Mastered;
        assert_eq!(
            apply_completed_points(
                &mut index,
                "math",
                entry.completed_knowledge_points.as_ref().unwrap()
            ),
            0
        );
    }

    #[test]
    fn normalize_rejects_unrelated_points_and_removes_duplicates() {
        let (index, task) = fixture();
        let mut entry = partial(&task);
        entry.completed_knowledge_points.as_mut().unwrap()[0].table_id = "t2".into();
        assert!(normalize_review(&index, &task, &mut entry).is_err());
        let mut entry = partial(&task);
        entry.completed_knowledge_points =
            Some(vec![
                entry.completed_knowledge_points.as_ref().unwrap()[0]
                    .clone();
                2
            ]);
        normalize_review(&index, &task, &mut entry).unwrap();
        assert_eq!(entry.completed_knowledge_points.unwrap().len(), 1);
    }

    #[test]
    fn canceled_association_never_falls_back_to_title_but_old_data_can() {
        let (index, mut task) = fixture();
        task.knowledge_points = Some(vec![]);
        let mut entry = TaskReviewEntry {
            status: "completed".into(),
            completion: 1.0,
            ..Default::default()
        };
        normalize_review(&index, &task, &mut entry).unwrap();
        assert_eq!(entry.completed_knowledge_points, Some(vec![]));
        task.knowledge_points = None;
        normalize_review(&index, &task, &mut entry).unwrap();
        assert!(entry.completed_knowledge_points.is_none());
        let mut json = serde_json::to_value(&task).unwrap();
        json.as_object_mut().unwrap().remove("knowledge_points");
        let old: PlanTask = serde_json::from_value(json).unwrap();
        assert!(old.knowledge_points.is_none());
    }

    #[test]
    fn full_completion_records_all_points_and_stale_links_require_repair() {
        let (index, task) = fixture();
        let mut entry = TaskReviewEntry {
            status: "completed".into(),
            ..Default::default()
        };
        normalize_review(&index, &task, &mut entry).unwrap();
        assert_eq!(entry.completed_knowledge_points.as_ref().unwrap().len(), 3);
        assert_eq!(entry.completion, 1.0);
        let mut stale = task.clone();
        stale.knowledge_points.as_mut().unwrap()[0].node_id = "deleted".into();
        assert!(normalize_review(&index, &stale, &mut entry).is_err());
    }

    #[test]
    fn plan_dates_do_not_change_learning_status() {
        let (mut index, task) = fixture();
        assert!(record_planned_dates(
            &mut index,
            &[task],
            "2026-10-07",
            "2026-10-06"
        ));
        let nodes = &index.subjects["math"].tables[0].nodes;
        assert!(nodes.iter().all(|n| n.status == NodeStatus::Pending));
        assert!(nodes
            .iter()
            .all(|n| n.planned_date.as_deref() == Some("2026-10-07")));
    }

    #[test]
    fn furthest_position_backfill_overrides_residual_without_rewriting_history() {
        let (mut index, task) = fixture();
        let mut entry = partial(&task);
        normalize_review(&index, &task, &mut entry).unwrap();
        for node in &mut index.subjects.get_mut("math").unwrap().tables[0].nodes {
            node.status = NodeStatus::Basic;
        }
        let unchanged = scheduling_entries(&index, &[entry.clone()], &[]);
        assert_eq!(
            remaining_points(&unchanged[0]).unwrap().len(),
            2,
            "已有基础状态本身不能替代本次确认"
        );
        let declared = crate::data::records::OvercompletionEntry {
            subject: "math".into(),
            node_id: "n2".into(),
            chapter_reached: "函数的连续性".into(),
            ..Default::default()
        };
        let scheduled = scheduling_entries(&index, &[entry.clone()], &[declared]);
        assert!(remaining_points(&scheduled[0]).unwrap().is_empty());
        assert_eq!(entry.status, "partial", "原复盘范围保持不变");
        assert_eq!(entry.completed_knowledge_points.unwrap().len(), 1);
    }

    #[test]
    fn scheduler_keeps_only_remaining_once_even_if_ai_replan_failed() {
        let dir = std::env::temp_dir().join(format!(
            "sa_partial_schedule_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (index, task) = fixture();
        crate::data::progress_tables::save_progress_index(&dir, &index).unwrap();
        let mut daily = DailyPlanFile::default();
        daily.meta.date = "2026-10-06".into();
        daily.data.tasks.push(task.clone());
        crate::data::plan::save_daily_plan(&dir, &daily).unwrap();
        let mut entry = partial(&task);
        normalize_review(&index, &task, &mut entry).unwrap();
        let mut review = crate::data::records::ReviewFile::default();
        review.meta.date = daily.meta.date.clone();
        review.task_reviews.push(entry);
        crate::data::records::save_review(&dir, &review).unwrap();
        let mut state = crate::data::state::StudyState::default();
        state.meta.exam_date = "2026-12-20".into();
        state.subjects.math.active = true;
        crate::data::state::save_state(&dir, &state).unwrap();
        let mut week = WeekPlanFile::default();
        week.meta.week_start = "2026-10-05".into();
        week.meta.week_end = "2026-10-11".into();
        week.data.days = vec![WeekDayPlan {
            date: "2026-10-07".into(),
            subject_allocations: vec![DaySubjectAllocation {
                subject: SubjectKey::Math,
                task_templates: vec![TaskTemplate {
                    title: task.title.clone(),
                    knowledge_points: task.knowledge_points.clone(),
                    estimated_hours: 3.0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }];
        crate::data::plan::save_week_plan(&dir, &week).unwrap();
        let result =
            super::super::scheduler::DailyScheduler::generate_daily_plan(&dir, "2026-10-07", false)
                .unwrap();
        let points: Vec<_> = result
            .data
            .tasks
            .iter()
            .chain(&result.data.deferred_tasks)
            .flat_map(|t| t.knowledge_points.iter().flatten())
            .collect();
        assert_eq!(
            points.len(),
            2,
            "原周计划和积压不能重复安排，也不能重复已完成点"
        );
        assert!(points.iter().all(|p| p.node_id != "n1"));
        assert_eq!(
            result
                .data
                .tasks
                .iter()
                .chain(&result.data.deferred_tasks)
                .map(|t| t.estimated_hours)
                .sum::<f64>(),
            2.0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sync_legacy_titles_still_work_and_tomorrow_plan_never_upgrades_nodes() {
        let dir = std::env::temp_dir().join(format!(
            "sa_partial_sync_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (index, task) = fixture();
        crate::data::progress_tables::save_progress_index(&dir, &index).unwrap();
        let tomorrow = DailyPlanFile {
            data: crate::data::plan::DailyPlanData {
                tasks: vec![task],
                ..Default::default()
            },
            ..Default::default()
        };
        super::super::progress_sync::sync_review_to_progress(&dir, &[], "hard", Some(&tomorrow))
            .unwrap();
        let unchanged = crate::data::progress_tables::load_progress_index(&dir);
        assert!(unchanged.subjects["math"].tables[0]
            .nodes
            .iter()
            .all(|n| n.status == NodeStatus::Pending));
        let legacy = super::super::progress_sync::ReviewSyncTask {
            subject: "math".into(),
            title: "函数的极限".into(),
            completed: true,
            mastery: String::new(),
            completed_knowledge_points: None,
        };
        super::super::progress_sync::sync_review_to_progress(&dir, &[legacy], "normal", None)
            .unwrap();
        let changed = crate::data::progress_tables::load_progress_index(&dir);
        assert_eq!(
            changed.subjects["math"].tables[0].nodes[1].status,
            NodeStatus::Basic
        );
        assert_eq!(
            changed.subjects["math"].tables[0].nodes[0].status,
            NodeStatus::Pending
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ai_replan_cannot_repeat_done_ranges_or_drop_remaining_even_when_time_tracking_is_off() {
        let (index, task) = fixture();
        let mut entry = partial(&task);
        normalize_review(&index, &task, &mut entry).unwrap();
        let dates: Vec<String> = vec!["2026-10-07".into()];
        let mut week = WeekPlanFile::default();
        week.data.days = vec![WeekDayPlan {
            date: dates[0].clone(),
            subject_allocations: vec![DaySubjectAllocation {
                subject: SubjectKey::Math,
                task_templates: vec![TaskTemplate {
                    knowledge_points: task.knowledge_points.clone(),
                    estimated_hours: 3.0,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }];
        remove_completed_ranges(&index, &mut week, &[entry.clone()], &dates);
        let residual = &week.data.days[0].subject_allocations[0].task_templates[0];
        assert_eq!(residual.knowledge_points.as_ref().unwrap().len(), 2);
        assert!((residual.estimated_hours - 2.0).abs() < 0.001);
        week.data.days[0].subject_allocations.clear(); // 模拟 AI 完全漏排
        let mut daily = DailyPlanFile::default();
        daily.data.tasks.push(task);
        ensure_remaining_ranges(&mut week, &[entry.clone()], &dates, Some(&daily));
        ensure_remaining_ranges(&mut week, &[entry], &dates, Some(&daily));
        let templates = &week.data.days[0].subject_allocations[0].task_templates;
        assert_eq!(templates.len(), 1, "重复协调不能重复安排");
        assert_eq!(
            templates[0].estimated_hours, 2.0,
            "关闭用时记录也应沿用原计划估时"
        );
        assert!(templates[0].remaining_from_task_id.is_some());
    }
}
