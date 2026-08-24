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

/// 获取学习数据分析
///
/// 根据 range 参数返回指定时间范围的分析数据。
/// - `last_7_days`：近7天
/// - `last_30_days`：近30天（默认）
/// - `all`：全部历史
///
/// `exclude_exempt_dates`：是否在分析中排除休息日和特殊情况排除日（默认 true）
///
/// 前端调用: `invoke('get_analytics', { range: 'last_30_days', excludeExemptDates: true })`
#[tauri::command]
pub async fn get_analytics(
    range: Option<String>,
    exclude_exempt_dates: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<AnalyticsSummary, String> {
    let data_dir = get_data_dir(state.inner())?;

    let range = match range.as_deref() {
        Some("last_7_days") => AnalyticsRange::Last7Days,
        Some("all") => AnalyticsRange::All,
        _ => AnalyticsRange::Last30Days,
    };
    // 默认开启排除
    let exclude_exempt = exclude_exempt_dates.unwrap_or(true);

    build_analytics(&data_dir, &range, exclude_exempt)
        .map_err(|e| format!("生成分析数据失败: {}", e))
}
