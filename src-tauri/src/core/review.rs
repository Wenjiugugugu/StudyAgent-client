//! Review Agent — 调用 AI Service 生成复盘
//!
//! 数据契约：统一 JSON 格式 { version, meta, data, view? }
//! - 复盘：records/YYYY-MM-DD_review.json
//!
//! 流程：
//! 1. 读取今日计划和 State
//! 2. 构建复盘 prompt，要求 AI 输出 JSON
//! 3. 解析、兜底填充、保存 JSON
//! 4. 返回 ReviewData
//!
//! 注意：任务状态由 `submit_review` 根据用户勾选决定。

use std::path::Path;

use crate::ai::provider::{AgentType, ChatRequest};
use crate::ai::service::AiService;
#[cfg(test)]
use crate::core::date_utils::now_string;
use crate::core::date_utils::today_string;
use crate::data::records::ReviewFile;
#[cfg(test)]
use crate::data::state::{SubjectKey, TaskPriority};
use crate::data::{clean_ai_json, DataResult};

/// Review Agent — 复盘生成器
pub struct ReviewAgent<'a> {
    ai_service: &'a AiService,
}

impl<'a> ReviewAgent<'a> {
    /// 创建新的 Review Agent
    pub fn new(ai_service: &'a AiService) -> Self {
        Self { ai_service }
    }

    /// 生成复盘记录
    ///
    /// `dida_completed`：滴答清单当日已确认完成的任务标题（来自 sync::dida 回读），
    /// 注入 prompt 供 AI 作为「完成情况的事实来源」之一（以滴答勾选为准）。
    ///
    /// 流程：
    /// 1. 校验只能生成今天的复盘
    /// 2. 读取今日计划和 State
    /// 3. 调用 AI 生成复盘 JSON
    /// 4. 保存 JSON
    /// 5. 返回 ReviewFile
    pub async fn generate_review(
        &self,
        data_dir: &Path,
        date: &str,
        dida_completed: &[String],
    ) -> DataResult<ReviewFile> {
        // 0. 只允许生成今天的复盘
        let today = today_string();
        if date != today {
            return Err(format!(
                "只能生成今天的学习复盘（今天是 {}），不支持生成过去或未来的复盘",
                today
            ));
        }

        super::review_sync::ensure_ready(data_dir)?;
        let state = crate::data::state::read_state_or_new(data_dir)?;
        let mut review_file = factual_review(data_dir, date, &state, dida_completed)?;
        let prompt_facts = visible_review_facts(data_dir, date, &review_file)?;
        let prompt = format!("根据以下已确认的复盘事实生成简短中文摘要。精力 0 表示未记录，空 time_spent 表示没有已确认用时。仅输出 {{\"view\":\"摘要\"}}。\n<review_facts date=\"{}\">\n{}\n</review_facts>", date, serde_json::to_string(&prompt_facts).map_err(|e| e.to_string())?);
        crate::data::write_ai_debug_log(
            data_dir,
            "review_prompt_ready",
            &format!(
                "复盘 prompt 已构建, date={}, 长度={} 字符",
                date,
                prompt.len()
            ),
        );

        // 4. 调用 AI Service
        let request = ChatRequest {
            messages: crate::ai::task_prompts::messages(crate::ai::task_prompts::REVIEW, prompt),
            agent: Some(AgentType::Reviewer),
            temperature: Some(0.5),
            // 复盘 JSON 生成可能较慢，覆盖 Provider 默认 timeout 至 300s
            timeout_override: Some(300),
            ..Default::default()
        };

        crate::data::write_ai_debug_log(
            data_dir,
            "review_ai_request",
            &format!("即将发送复盘 AI 请求, date={}, timeout=300s", date),
        );
        let response = self.ai_service.chat(request).await.map_err(|e| {
            crate::data::write_ai_debug_log(
                data_dir,
                "review_ai_call_error",
                &format!("复盘 AI 调用失败: {}", e),
            );
            format!("AI 生成复盘失败: {}", e)
        })?;

        // 调试日志：AI 返回的原始复盘 JSON（解析前）
        let resp_preview: String = response.content.chars().take(500).collect();
        log::info!(
            "[AI-DEBUG] 复盘原始响应长度: {} 字符, 前 500 字符: {}",
            response.content.len(),
            resp_preview
        );
        log::debug!("[AI-DEBUG] 复盘原始响应全文:\n{}", response.content);
        crate::data::write_ai_debug_log(
            data_dir,
            "review_ai_response",
            &format!(
                "复盘 AI 响应已返回, 长度={} 字符, 前 500 字符:\n{}",
                response.content.len(),
                resp_preview
            ),
        );

        let output: serde_json::Value = serde_json::from_str(&clean_ai_json(&response.content))
            .map_err(|e| format!("复盘摘要格式无效: {}", e))?;
        review_file.view = Some(
            output
                .get("view")
                .and_then(|v| v.as_str())
                .ok_or("复盘摘要缺少 view 字符串")?
                .to_string(),
        );
        // 6. 保存 JSON
        crate::data::records::save_review(data_dir, &review_file)?;
        crate::data::write_ai_debug_log(
            data_dir,
            "review_saved",
            &format!("复盘已保存, date={}", date),
        );

        Ok(review_file)
    }
}

fn visible_review_facts(dir: &Path, date: &str, review: &ReviewFile) -> DataResult<ReviewFile> {
    let mut visible = review.clone();
    let hidden = match std::fs::metadata(crate::data::daily_plan_path(dir, date)) {
        Ok(_) => crate::data::read_daily_plan(dir, date)?
            .data
            .tasks
            .into_iter()
            .filter(|t| !t.ai_visible())
            .map(|t| t.id)
            .collect::<std::collections::HashSet<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => return Err(e.to_string()),
    };
    visible
        .data
        .completed_tasks
        .retain(|t| t.task_id.as_ref().is_none_or(|id| !hidden.contains(id)));
    visible
        .task_reviews
        .retain(|t| !hidden.contains(&t.task_id));
    visible.view = None;
    Ok(visible)
}

fn factual_review(
    dir: &Path,
    date: &str,
    state: &crate::data::StudyState,
    dida_completed: &[String],
) -> DataResult<ReviewFile> {
    let path = crate::data::records::review_file_path(dir, date);
    match std::fs::metadata(path) {
        Ok(_) => return crate::data::records::read_review(dir, date),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let plan = match std::fs::metadata(crate::data::plan::daily_plan_path(dir, date)) {
        Ok(_) => Some(crate::data::read_daily_plan(dir, date)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let mut review = ReviewFile {
        version: "1.0.0".into(),
        meta: crate::data::records::ReviewMeta {
            date: date.into(),
            r#type: "review".into(),
            plan_ref: format!("plan/{}_day.json", date),
            generated_at: crate::data::now_string(),
            default_marked: false,
        },
        ..Default::default()
    };
    if let Some(plan) = plan {
        for task in plan.data.tasks {
            let current = (state.current_task.date == date)
                .then(|| {
                    state
                        .current_task
                        .tasks
                        .iter()
                        .find(|t| t.task_id.as_deref() == Some(&task.id))
                })
                .flatten();
            let done = current.is_some_and(|t| t.status == crate::data::TaskStatus::Done)
                || task.status == crate::data::TaskStatus::Done
                || dida_completed.contains(&task.title);
            let minutes = current
                .filter(|t| t.accumulated_minutes > 0)
                .map(|t| t.accumulated_minutes);
            if let Some(minutes) = minutes {
                if let Some(row) = review
                    .data
                    .time_spent
                    .iter_mut()
                    .find(|r| r.subject == task.subject)
                {
                    row.hours += minutes as f64 / 60.0;
                } else {
                    review
                        .data
                        .time_spent
                        .push(crate::data::records::ReviewTimeSpent {
                            subject: task.subject.clone(),
                            hours: minutes as f64 / 60.0,
                            planned_hours: None,
                        });
                }
            }
            review
                .data
                .completed_tasks
                .push(crate::data::records::ReviewCompletedTask {
                    task_id: Some(task.id),
                    subject: task.subject,
                    title: task.title,
                    priority: task.priority,
                    completed: done,
                    completion_time: None,
                    note: None,
                });
        }
    }
    review.data.total_hours = review.data.time_spent.iter().map(|r| r.hours).sum();
    let all = review.data.completed_tasks.len() as i32;
    let done = review
        .data
        .completed_tasks
        .iter()
        .filter(|t| t.completed)
        .count() as i32;
    review.data.completion = crate::data::records::ReviewCompletion {
        priority_a_total: all,
        priority_a_done: done,
        priority_b_total: 0,
        priority_b_done: 0,
        completion_rate: if all > 0 {
            done as f64 / all as f64 * 100.0
        } else {
            0.0
        },
    };
    Ok(review)
}

/// 从 AI 响应中提取并解析复盘 JSON
#[cfg(test)]
fn parse_review_json(
    content: &str,
    date: &str,
    plan: Option<&crate::data::plan::DailyPlanFile>,
) -> DataResult<ReviewFile> {
    let cleaned = clean_ai_json(content);
    let mut review: ReviewFile = serde_json::from_str(&cleaned).map_err(|e| {
        let preview: String = cleaned.chars().take(200).collect();
        format!("解析 AI 返回的复盘 JSON 失败: {}\n内容片段: {}", e, preview)
    })?;

    // 兜底填充版本与 meta
    if review.version.is_empty() {
        review.version = "1.0.0".to_string();
    }
    if review.meta.date.is_empty() {
        review.meta.date = date.to_string();
    }
    if review.meta.r#type.is_empty() {
        review.meta.r#type = "review".to_string();
    }
    if review.meta.plan_ref.is_empty() {
        review.meta.plan_ref =
            format!("plan/{}{}", date, crate::data::plan::DAILY_PLAN_FILE_SUFFIX);
    }
    if review.meta.generated_at.is_empty() {
        review.meta.generated_at = now_string();
    }

    // 根据实际计划补齐未填写的 task_id / subject / priority
    if let Some(plan) = plan {
        let mut task_id_to_plan_task: std::collections::HashMap<
            &str,
            &crate::data::plan::PlanTask,
        > = std::collections::HashMap::new();
        for task in &plan.data.tasks {
            task_id_to_plan_task.insert(&task.id, task);
        }

        for completed in &mut review.data.completed_tasks {
            if let Some(ref task_id) = completed.task_id {
                if let Some(plan_task) = task_id_to_plan_task.get(task_id.as_str()) {
                    if completed.subject == SubjectKey::default() {
                        completed.subject = plan_task.subject.clone();
                    }
                    if completed.priority == TaskPriority::default() {
                        completed.priority = plan_task.priority.clone();
                    }
                    if completed.title.is_empty() {
                        completed.title = plan_task.title.clone();
                    }
                }
            }
        }
    }

    // 如果 AI 未填写 completion，根据 completed_tasks 自动计算
    if review.data.completion.completion_rate == 0.0
        && (review.data.completion.priority_a_total == 0
            && review.data.completion.priority_b_total == 0)
    {
        let mut a_total = 0i32;
        let mut a_done = 0i32;
        let mut b_total = 0i32;
        let mut b_done = 0i32;

        for task in &review.data.completed_tasks {
            match task.priority {
                TaskPriority::A => {
                    a_total += 1;
                    if task.completed {
                        a_done += 1;
                    }
                }
                TaskPriority::B => {
                    b_total += 1;
                    if task.completed {
                        b_done += 1;
                    }
                }
                _ => {}
            }
        }

        let total = a_total + b_total;
        let done = a_done + b_done;
        review.data.completion = crate::data::records::ReviewCompletion {
            priority_a_total: a_total,
            priority_a_done: a_done,
            priority_b_total: b_total,
            priority_b_done: b_done,
            completion_rate: if total > 0 {
                (done as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        };
    }

    Ok(review)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_ai_json_removes_fences() {
        let raw = r#"```json
{"version":"1.0.0","meta":{"date":"2026-07-25"},"data":{"completed_tasks":[]},"view":""}
```"#;
        let cleaned = clean_ai_json(raw);
        assert!(cleaned.starts_with('{'));
        assert!(cleaned.ends_with('}'));
        assert!(!cleaned.contains("```"));
    }

    #[test]
    fn test_parse_review_json() {
        let raw = r#"{"version":"1.0.0","meta":{"date":"2026-07-25","type":"review","plan_ref":"plan/2026-07-25_day.json","generated_at":"2026-07-25T22:00"},"data":{"completed_tasks":[{"task_id":"2026-07-25-01","subject":"math","title":"行列式","priority":"A","completed":true,"completion_time":"10:30","note":null}],"unplanned_tasks":[],"difficulties":[],"time_spent":[{"subject":"math","hours":2.0,"planned_hours":2.0}],"total_hours":2.0,"completion":{"priority_a_total":1,"priority_a_done":1,"priority_b_total":0,"priority_b_done":0,"completion_rate":100.0},"energy_level":4,"external_interference":"无","key_achievements":[],"next_steps":[]},"view":""}"#;
        let review = parse_review_json(raw, "2026-07-25", None).unwrap();
        assert_eq!(review.meta.date, "2026-07-25");
        assert_eq!(review.data.completed_tasks.len(), 1);
        assert_eq!(review.data.completion.completion_rate, 100.0);
    }
    #[test]
    fn existing_confirmed_review_is_preserved_and_unknown_energy_is_not_inferred() {
        let dir = std::env::temp_dir().join(crate::data::new_progress_id("review-facts", "test"));
        let date = crate::data::today_string();
        let state = crate::data::StudyState::default();
        let unknown = factual_review(&dir, &date, &state, &[]).unwrap();
        assert_eq!(unknown.data.energy_level, 0);
        assert!(unknown.data.time_spent.is_empty());
        let mut confirmed = unknown;
        confirmed.data.energy_level = 5;
        confirmed.data.total_hours = 2.5;
        confirmed
            .actual_study
            .push(crate::data::study_direction::ActualStudyEntry {
                subject: "math".into(),
                content: "用户确认的内容".into(),
                ..Default::default()
            });
        crate::data::save_review(&dir, &confirmed).unwrap();
        let after =
            factual_review(&dir, &date, &state, &["模型不得当成新事实的标题".into()]).unwrap();
        assert_eq!(
            serde_json::to_value(&confirmed).unwrap(),
            serde_json::to_value(after).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
