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
//! 注意：不再自动将未完成任务标记为 abandoned。
//! 任务状态的更新完全由 `submit_review` 根据用户在结构化复盘表单中的勾选决定。

use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::data::records::{ReviewData, ReviewFile};
use crate::data::state::{SubjectKey, TaskPriority, TaskStatus};
use crate::data::{now_string, today_string, DataResult};

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
    ) -> DataResult<ReviewFile> {
        // 0. 只允许生成今天的复盘
        let today = today_string();
        if date != today {
            return Err(format!(
                "只能生成今天的学习复盘（今天是 {}），不支持生成过去或未来的复盘",
                today
            ));
        }

        // 1. 读取今日计划与状态
        let plan = crate::data::plan::read_daily_plan_with_merged_status(data_dir, date).ok();
        let state = crate::data::state::read_state(data_dir).unwrap_or_default();

        // 2. 构建复盘 prompt
        // 注意：不在此处修改任务状态。任务状态由 `submit_review` 根据用户输入更新。
        let prompt = self.build_review_prompt(plan.as_ref(), &state, date);
        crate::data::write_ai_debug_log(
            data_dir,
            "review_prompt_ready",
            &format!("复盘 prompt 已构建, date={}, 长度={} 字符", date, prompt.len()),
        );

        // 4. 调用 AI Service
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: prompt,
                ..Default::default()
            }],
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
        let response = self
            .ai_service
            .chat(request)
            .await
            .map_err(|e| {
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

        // 5. 解析并填充 ReviewFile
        let review_file = parse_review_json(&response.content, date, plan.as_ref())?;

        // 6. 保存 JSON
        crate::data::records::save_review(data_dir, &review_file)?;
        crate::data::write_ai_debug_log(
            data_dir,
            "review_saved",
            &format!("复盘已保存, date={}", date),
        );

        Ok(review_file)
    }

    /// 构建复盘 prompt
    fn build_review_prompt(
        &self,
        plan: Option<&crate::data::plan::DailyPlanFile>,
        state: &crate::data::state::StudyState,
        date: &str,
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str(&format!("请为 {} 生成学习复盘记录。\n\n", date));

        // 今日计划
        if let Some(plan) = plan {
            prompt.push_str("## 今日计划\n");
            prompt.push_str(&format!(
                "- 总时长: {:.1}h\n- 总任务数: {}\n- 目标: {}\n- 策略: {}\n\n",
                plan.data.total_hours,
                plan.data.total_tasks,
                plan.data.target,
                plan.data.strategy
            ));

            if !plan.data.tasks.is_empty() {
                prompt.push_str("### 任务列表\n");
                for task in &plan.data.tasks {
                    let status_str = match task.status {
                        TaskStatus::Done => "已完成",
                        TaskStatus::Abandoned => "已放弃",
                        TaskStatus::InProgress => "进行中",
                        TaskStatus::Pending => "未开始",
                    };
                    prompt.push_str(&format!(
                        "- [{}] {}（{:?}，优先级 {:?}，预估 {:.1}h）\n  目标: {}\n  完成标准: {}\n",
                        status_str,
                        task.title,
                        task.subject,
                        task.priority,
                        task.estimated_hours,
                        task.goal,
                        task.completion_criteria.join("；")
                    ));
                }
                prompt.push_str("\n");
            }
        } else {
            prompt.push_str("## 今日计划\n今日无计划文件。请根据 State 中的 current_task 生成复盘。\n\n");
            prompt.push_str(&format!(
                "### State 中的当前任务\n- 日期: {}\n- 重点: {}\n- 预估总时长: {:.1}h\n",
                state.current_task.date,
                state.current_task.focus,
                state.current_task.total_hours.unwrap_or(0.0)
            ));
            for task in &state.current_task.tasks {
                prompt.push_str(&format!(
                    "  - [{:?}] {} ({:?}) - {:?}\n",
                    task.priority, task.subject, task.task, task.status
                ));
            }
            prompt.push_str("\n");
        }

        // 全局进度
        prompt.push_str("## 全局进度\n");
        prompt.push_str(&format!(
            "- 总学习天数: {}\n- 连续学习天数: {}\n- 上次学习日期: {}\n- 总练习题数: {}\n\n",
            state.progress.total_study_days,
            state.progress.streak_days,
            state.progress.last_study_date,
            state.progress.total_practice_questions
        ));

        // 输出要求
        prompt.push_str("## 输出要求\n");
        prompt.push_str(
            r#"请直接输出一个合法的 JSON 对象（不要包裹 ```json 代码块），严格符合以下结构：

{
  "version": "1.0.0",
  "meta": {
    "date": "YYYY-MM-DD",
    "type": "review",
    "plan_ref": "plan/YYYY-MM-DD_day.json",
    "generated_at": "YYYY-MM-DDTHH:mm"
  },
  "data": {
    "completed_tasks": [
      {
        "task_id": "2026-07-25-01（可选）",
        "subject": "math",
        "title": "任务标题",
        "priority": "A",
        "completed": true,
        "completion_time": "10:30（可选）",
        "note": "可选备注"
      }
    ],
    "unplanned_tasks": [
      {
        "subject": "english",
        "title": "计划外任务标题",
        "hours": 0.5,
        "note": "可选"
      }
    ],
    "difficulties": [
      {
        "description": "困难描述",
        "root_cause": "可选根因",
        "resolution": "可选解决方式"
      }
    ],
    "time_spent": [
      {
        "subject": "math",
        "hours": 2.0,
        "planned_hours": 2.0
      }
    ],
    "total_hours": 2.0,
    "completion": {
      "priority_a_total": 1,
      "priority_a_done": 1,
      "priority_b_total": 0,
      "priority_b_done": 0,
      "completion_rate": 100.0
    },
    "energy_level": 4,
    "external_interference": "无",
    "key_achievements": ["关键成果1"],
    "next_steps": ["下一步行动1"]
  },
  "view": "用于人类阅读的 Markdown 摘要（可选，可为空字符串）"
}

重要约束：
1. 必须包含 data 和 view 两个字段；view 仅用于展示，不会被程序解析。
2. 复盘原则：只记录事实，不做分析、不评判、不给策略建议。
3. subject 字段只能是 "math" / "english" / "politics" / "professional"。
4. priority 字段只能是 "A" 或 "B"。
5. completed 字段必须如实反映计划任务的完成状态。
6. completion_rate 使用 0-100 之间的数值。
7. energy_level 使用 1-5 之间的整数。
"#,
        );

        prompt
    }

    /// 检查今日是否需要复盘
    pub fn needs_review(data_dir: &Path) -> bool {
        let today = today_string();
        !crate::data::records::review_file_path(data_dir, &today).exists()
    }
}

/// 从 AI 响应中提取并解析复盘 JSON
fn parse_review_json(
    content: &str,
    date: &str,
    plan: Option<&crate::data::plan::DailyPlanFile>,
) -> DataResult<ReviewFile> {
    let cleaned = clean_ai_json(content);
    let mut review: ReviewFile = serde_json::from_str(&cleaned)
        .map_err(|e| format!("解析 AI 返回的复盘 JSON 失败: {}\n内容片段: {}", e, &cleaned[..cleaned.len().min(200)]))?;

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
        review.meta.plan_ref = format!("plan/{}{}", date, crate::data::plan::DAILY_PLAN_FILE_SUFFIX);
    }
    if review.meta.generated_at.is_empty() {
        review.meta.generated_at = now_string();
    }

    // 根据实际计划补齐未填写的 task_id / subject / priority
    if let Some(plan) = plan {
        let mut task_id_to_plan_task: std::collections::HashMap<&str, &crate::data::plan::PlanTask> =
            std::collections::HashMap::new();
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

/// 清理 AI 可能包裹的代码块，提取纯 JSON
fn clean_ai_json(content: &str) -> String {
    let trimmed = content.trim();

    // 尝试提取 ```json ... ``` 或 ``` ... ``` 包裹的内容
    if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map(|p| p + 1).unwrap_or(0);
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        return trimmed[start..end].trim().to_string();
    }

    // 尝试找到第一个 '{' 和最后一个 '}'
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return trimmed[start..=end].to_string();
        }
    }

    trimmed.to_string()
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
}
