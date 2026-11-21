//! Deterministic calendar and plan constraints.

use crate::core::date_utils::{add_days, today_string, weekday_name};
use crate::data::plan::{ExcludedDay, WeekDayPlan, WeekPlanFile};
use crate::data::DataResult;

pub(crate) fn enforce_rest_days(
    plan: &mut WeekPlanFile,
    rest_days: &[String],
    week_start: &str,
    week_end: &str,
) -> DataResult<()> {
    // 收集已存在的日期
    let existing_dates: std::collections::HashSet<String> =
        plan.data.days.iter().map(|d| d.date.clone()).collect();

    // 遍历整周，校验/补全
    let mut current_date = week_start.to_string();
    loop {
        let weekday = weekday_name(&current_date)?;
        let should_rest = rest_days.contains(&weekday);

        if let Some(day) = plan.data.days.iter_mut().find(|d| d.date == current_date) {
            // 已存在：强制覆盖
            if should_rest && !day.is_rest_day {
                log::warn!(
                    "周计划校验: {}（{}）应为休息日但 AI 标记为学习日，已修正",
                    current_date,
                    weekday
                );
                day.is_rest_day = true;
                day.subject_allocations.clear();
            } else if !should_rest && day.is_rest_day {
                log::warn!(
                    "周计划校验: {}（{}）应为学习日但 AI 标记为休息日，已修正",
                    current_date,
                    weekday
                );
                day.is_rest_day = false;
            }
        } else if !existing_dates.contains(&current_date) {
            // 缺失：补全
            log::warn!(
                "周计划校验: {}（{}）缺失，已补全（is_rest_day={}）",
                current_date,
                weekday,
                should_rest
            );
            plan.data.days.push(WeekDayPlan {
                date: current_date.clone(),
                weekday,
                is_rest_day: should_rest,
                subject_allocations: Vec::new(),
            });
        }

        if current_date == week_end {
            break;
        }
        current_date = add_days(&current_date, 1)?;
    }

    // 按日期排序
    plan.data.days.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(())
}

pub(crate) fn enforce_excluded_days(
    plan: &mut WeekPlanFile,
    excluded_days: &[ExcludedDay],
) -> DataResult<()> {
    if excluded_days.is_empty() {
        return Ok(());
    }
    for ex in excluded_days {
        if let Some(day) = plan.data.days.iter_mut().find(|d| d.date == ex.date) {
            if !day.is_rest_day || !day.subject_allocations.is_empty() {
                log::info!(
                    "周计划校验: {} 标记为排除日（{}），清空 allocations",
                    ex.date,
                    ex.reason_type
                );
                day.is_rest_day = true;
                day.subject_allocations.clear();
            }
        } else {
            // 排除日缺失则补上
            let weekday = weekday_name(&ex.date)?;
            log::info!("周计划校验: 排除日 {} 缺失，补全为休息日", ex.date);
            plan.data.days.push(WeekDayPlan {
                date: ex.date.clone(),
                weekday,
                is_rest_day: true,
                subject_allocations: Vec::new(),
            });
        }
    }
    plan.data.days.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(())
}

pub(crate) fn enforce_past_days_empty(plan: &mut WeekPlanFile) {
    let today = today_string();
    let mut cleared = 0usize;
    for day in plan.data.days.iter_mut() {
        if day.date.as_str() < today.as_str() && !day.subject_allocations.is_empty() {
            day.subject_allocations.clear();
            cleared += 1;
        }
    }
    if cleared > 0 {
        log::warn!(
            "周计划校验: {} 个已过去日期（早于 {}）的任务分配已清空（周中生成时不再补排历史日）",
            cleared,
            today
        );
    }
}

/// 对休息、排除及过去日期取并集，避免重叠扣除。
pub(crate) fn available_dates(
    start: &str,
    end: &str,
    rest: &[String],
    excluded: &[ExcludedDay],
    today: &str,
) -> DataResult<Vec<String>> {
    let mut result = Vec::new();
    let mut date = start.to_string();
    while date.as_str() <= end {
        if date.as_str() >= today
            && !rest.contains(&weekday_name(&date)?)
            && !excluded.iter().any(|e| e.date == date)
        {
            result.push(date.clone());
        }
        date = add_days(&date, 1)?;
    }
    Ok(result)
}

/// 剩余预算按同一可学习日期集合缩放一次，不把整周工作量挤到剩余日期。
pub(crate) fn scale_remaining_budget(
    parameters: &mut crate::core::adaptive_planner::AdaptivePlanParameters,
    configured_days: usize,
    available_days: usize,
) {
    let ratio = available_days as f64 / configured_days.max(1) as f64;
    parameters.nominal_total_hours *= ratio;
    parameters.capacity_hours *= ratio;
    for hours in parameters.subject_hours.values_mut() {
        *hours *= ratio;
    }
}

#[cfg(test)]
mod calendar_tests {
    use super::*;
    #[test]
    fn overlap_and_past_days_are_counted_once() {
        let excluded = vec![
            ExcludedDay {
                date: "2026-10-18".into(),
                ..Default::default()
            },
            ExcludedDay {
                date: "2026-10-14".into(),
                ..Default::default()
            },
        ];
        let dates = available_dates(
            "2026-10-12",
            "2026-10-18",
            &["周日".into()],
            &excluded,
            "2026-10-14",
        )
        .unwrap();
        assert_eq!(dates, vec!["2026-10-15", "2026-10-16", "2026-10-17"]);
    }
    #[test]
    fn remaining_budget_is_scaled_once_and_no_days_means_no_budget() {
        let mut parameters = crate::core::adaptive_planner::AdaptivePlanParameters {
            nominal_total_hours: 30.0,
            capacity_hours: 24.0,
            subject_hours: std::collections::HashMap::from([("math".into(), 12.0)]),
            ..Default::default()
        };
        scale_remaining_budget(&mut parameters, 6, 3);
        assert_eq!(parameters.nominal_total_hours, 15.0);
        assert_eq!(parameters.capacity_hours, 12.0);
        assert_eq!(parameters.subject_hours["math"], 6.0);
        scale_remaining_budget(&mut parameters, 6, 0);
        assert_eq!(parameters.nominal_total_hours, 0.0);
    }
}
