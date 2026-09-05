#![allow(unused_imports)]
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::provider::{AIProviderConfig, ChatRequest, ChatResponse};
use crate::ai::service::AiService;
use crate::core::analytics::{build_analytics, AnalyticsRange, AnalyticsSummary};
use crate::core::briefing::{yesterday_of, BriefingAgent};
use crate::core::dashboard::{DashboardAggregator, DashboardSummary};
use crate::core::planner::Planner;
use crate::core::review::ReviewAgent;
use crate::core::user_model::UserModelService;
use crate::data::assets::{UserCapability, UserObservation};
use crate::data::plan::{
    iso_week_string, DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment,
};
use crate::data::records::ReviewFile;
use crate::data::state::{StudyState, TaskStatus};
use crate::tools::dispatcher::{execute_builtin_tool, is_builtin_tool};
use crate::tools::mcp::{MCPServerStatus, ToolCallResult};
use crate::{
    get_ai_service, get_data_dir, get_data_dir_and_ai, get_data_dir_and_dispatcher,
    get_tool_dispatcher, load_settings, reinitialize_services, save_settings_file, AppSettings,
    AppState,
};

use super::legacy::*;

/// 获取指定日期的每日简报
///
/// 前端调用: `invoke('get_briefing', { date: '2026-08-04' })`
#[tauri::command]
pub async fn get_briefing(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<GetBriefingResult, String> {
    crate::data::validate_date(&date)?;
    let data_dir = get_data_dir(state.inner())?;
    let settings = crate::load_settings(&data_dir);

    // 读取简报文件
    let mut briefing = crate::data::briefing::read_briefing(&data_dir, &date).ok();
    let exists = briefing.is_some();

    // 无 AI 兜底：简报缺失或 AI 未给出估时时，用确定性「阶段估时」补齐——
    // 基于内置/启用进度表的隐藏预估时长（estimated_hours）按自适应复合校准系数调整，
    // 保证未配置 AI / AI 不可用时首页也能正常显示各科阶段估时。
    let deterministic = crate::core::briefing::deterministic_estimations(&data_dir);
    if let Some(b) = &mut briefing {
        if b.data.estimations.is_empty() && !deterministic.is_empty() {
            b.data.estimations = deterministic;
        }
    } else if !deterministic.is_empty() {
        briefing = Some(crate::data::briefing::BriefingFile {
            version: "1.0.0".to_string(),
            meta: crate::data::briefing::BriefingMeta {
                date: date.clone(),
                generated_at: crate::data::now_string(),
                ..Default::default()
            },
            data: crate::data::briefing::BriefingData {
                estimations: deterministic,
                ..Default::default()
            },
        });
    }

    // 昨日日期
    let yesterday = crate::data::add_days(&date, -1).unwrap_or_else(|_| date.clone());

    // 昨日复盘是否存在
    let yesterday_review_exists = crate::data::records::read_review(&data_dir, &yesterday).is_ok();

    // 判断昨日是否为休息日或排除日（若是则不要求补复盘）
    let rest_days = settings.rest_days();
    let yesterday_weekday = crate::data::weekday_name(&yesterday).unwrap_or_default();
    let yesterday_is_rest = rest_days.iter().any(|d| d == &yesterday_weekday);

    let yesterday_is_excluded = crate::data::plan::read_week_plan_for_date(&data_dir, &yesterday)
        .ok()
        .map(|wp| wp.data.excluded_days.iter().any(|d| d.date == yesterday))
        .unwrap_or(false);
    let yesterday_exempt = yesterday_is_rest || yesterday_is_excluded;

    // 今日是否为休息日
    let today_weekday = crate::data::weekday_name(&date).unwrap_or_default();
    let is_rest_day = rest_days.iter().any(|d| d == &today_weekday);

    // 今日是否为排除日
    let is_excluded_day = crate::data::plan::read_week_plan_for_date(&data_dir, &date)
        .ok()
        .map(|wp| wp.data.excluded_days.iter().any(|d| d.date == date))
        .unwrap_or(false);

    // 补复盘窗口：今日日期等于今天，且当前时间在每日结束时间 +1 小时内
    // 超过该窗口则视为「错过补复盘」，不再提供 AI 建议
    let today = crate::data::today_string();
    let within_makeup_window = if date == today {
        // 简单判断：今日都在补复盘窗口内（用户可在今日任何时候补复盘）
        // 真正的「错过」是指到了次日仍未补复盘，那时简报就不会自动生成了
        true
    } else {
        false
    };

    Ok(GetBriefingResult {
        briefing,
        exists,
        yesterday_review_exists,
        is_rest_day,
        is_excluded_day,
        yesterday_exempt,
        within_makeup_window,
    })
}

/// 重新生成指定日期的每日简报（AI 驱动）
///
/// 用户在 Dashboard 手动点击「重新生成简报」时调用。
/// 必须存在昨日复盘才能生成（否则 AI 无依据）。
///
/// 前端调用: `invoke('regenerate_briefing', { date: '2026-08-04' })`
#[tauri::command]
pub async fn regenerate_briefing(
    date: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::data::briefing::BriefingFile, String> {
    crate::data::validate_date(&date)?;
    let (data_dir, ai_service) = get_data_dir_and_ai(state.inner())?;

    // H1 并发保护：覆盖 AI 生成 + 写回全程，串行化 records/briefing 写入
    let io_lock = crate::get_io_lock(state.inner())?;
    let _io_guard = io_lock.lock().await;

    if !ai_service.has_provider() {
        return Err(
            "未配置 AI Provider，无法生成简报。请先在「设置」中添加并启用 AI Provider。"
                .to_string(),
        );
    }

    // 基于昨日复盘生成
    let yesterday = yesterday_of(&date)?;

    // 校验昨日复盘存在（简报必须有依据）
    if crate::data::records::read_review(&data_dir, &yesterday).is_err() {
        return Err(format!(
            "昨日（{}）复盘不存在，无法生成简报。请先完成昨日复盘。",
            yesterday
        ));
    }

    // 删除旧简报（若有）
    let _ = crate::data::briefing::delete_briefing(&data_dir, &date);

    let agent = BriefingAgent::new(&ai_service);
    agent
        .generate_briefing(&data_dir, &date, &yesterday, "manual")
        .await
}

/// 列出所有简报日期（YYYY-MM-DD，升序）
///
/// 前端调用: `invoke('list_briefing_dates')`
#[tauri::command]
pub async fn list_briefing_dates(state: State<'_, Mutex<AppState>>) -> Result<Vec<String>, String> {
    let data_dir = get_data_dir(state.inner())?;
    crate::data::briefing::list_briefing_dates(&data_dir)
}
