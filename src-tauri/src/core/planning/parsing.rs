//! AI 计划响应解析。

use crate::core::date_utils::now_string;
use crate::data::plan::{BasedOn, WeekPlanFile};
use crate::data::{clean_ai_json, DataResult};

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

/// 仅允许可证明不改变字符串和数值的尾逗号修复。
/// 缺失引号等有歧义的输出拒绝自动修复，避免把修改内容当成修语法。
pub(crate) fn repair_baseline(raw: &str) -> DataResult<serde_json::Value> {
    let raw = crate::data::clean_ai_json(raw);
    let chars: Vec<char> = raw.chars().collect();
    let mut normalized = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in chars.iter().enumerate() {
        if quoted {
            normalized.push(*c);
            if escaped {
                escaped = false;
            } else if *c == '\\' {
                escaped = true;
            } else if *c == '"' {
                quoted = false;
            }
        } else {
            if *c == '"' {
                quoted = true;
            }
            if *c == ','
                && chars[i + 1..]
                    .iter()
                    .find(|c| !c.is_whitespace())
                    .is_some_and(|c| *c == '}' || *c == ']')
            {
                continue;
            }
            normalized.push(*c);
        }
    }
    serde_json::from_str(&normalized)
        .map_err(|_| "原输出无法无损提取事实，已停止自动修复，请重新生成。".into())
}
pub(crate) fn verify_repair(baseline: &serde_json::Value, repaired: &str) -> DataResult<()> {
    let actual: serde_json::Value =
        serde_json::from_str(&crate::data::clean_ai_json(repaired)).map_err(|e| e.to_string())?;
    if baseline != &actual {
        return Err("自动修复改变了计划内容，未保存，请重新生成。".into());
    }
    Ok(())
}

#[cfg(test)]
mod repair_tests {
    use super::*;
    #[test]
    fn repairs_syntax_without_changing_dates_tasks_and_hours() {
        let baseline =
            repair_baseline(r#"{"date":"2026-10-12","title":"虚构内容,不改文本","hours":1.5,}"#)
                .unwrap();
        verify_repair(
            &baseline,
            r#"{"date":"2026-10-12","title":"虚构内容,不改文本","hours":1.5}"#,
        )
        .unwrap();
        assert!(verify_repair(
            &baseline,
            r#"{"date":"2026-10-12","title":"虚构内容,不改文本","hours":0.5}"#
        )
        .is_err());
        assert!(repair_baseline(r#"{"title":"未转义"引号"}"#).is_err());
    }
}
