//! AI 计划响应解析。

use crate::data::plan::{BasedOn, WeekPlanFile};
use crate::data::{clean_ai_json, now_string, DataResult};

pub(crate) fn parse_week_plan_json(
    content: &str,
    expected_week_start: &str,
    expected_week_end: &str,
) -> DataResult<WeekPlanFile> {
    let cleaned = clean_ai_json(content);
    let mut plan: WeekPlanFile = serde_json::from_str(&cleaned).map_err(|error| {
        let preview: String = cleaned.chars().take(200).collect();
        format!(
            "解析 AI 返回的周计划 JSON 失败: {}\n内容片段: {}",
            error, preview
        )
    })?;

    if plan.version.is_empty() {
        plan.version = "1.0.0".to_string();
    }
    if plan.meta.week_start.is_empty() {
        plan.meta.week_start = expected_week_start.to_string();
    }
    if plan.meta.week_end.is_empty() {
        plan.meta.week_end = expected_week_end.to_string();
    }
    if plan.meta.generated_at.is_empty() {
        plan.meta.generated_at = now_string();
    }
    if plan.meta.based_on.state.is_empty() {
        plan.meta.based_on = BasedOn {
            state: "state/current.state".to_string(),
            user_model: "assets/user_model/_index.md".to_string(),
            exam_config: "assets/config/exam-config.md".to_string(),
            review_ref: None,
            week_plan: None,
        };
    }

    Ok(plan)
}
