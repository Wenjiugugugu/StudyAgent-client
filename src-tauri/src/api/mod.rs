//! API Layer — Tauri 命令注册
//!
//! 本模块按业务领域组织所有 `#[tauri::command]` 函数，作为前端 Vue 3 调用 Rust 后端的入口。
//!
//! ## 命令列表
//!
//! | 命令 | 返回类型 | 说明 |
//! |------|----------|------|
//! | `get_dashboard_summary` | `DashboardSummary` | Dashboard 数据聚合 |
//! | `get_state` | `StudyState` | 读取学习状态 |
//! | `get_today_plan` | `DailyPlan` | 读取今日计划 |
//! | `get_plan_by_date` | `DailyPlan` | 读取指定日期计划 |
//! | `get_week_plan` | `WeekPlan` | 读取周计划 |
//! | `generate_daily_plan` | `DailyPlan` | AI 生成日计划 |
//! | `generate_week_plan` | `WeekPlan` | AI 生成周计划 |
//! | `update_task_status` | `()` | 更新任务状态 |
//! | `get_review` | `ReviewRecord` | 读取复盘记录 |
//! | `generate_review` | `ReviewRecord` | AI 生成复盘 |
//! | `chat` | `ChatResponse` | AI 对话（非流式） |
//! | `chat_stream` | `()` | AI 对话（流式，通过事件推送） |
//! | `list_mcp_servers` | `Vec<MCPServerStatus>` | 列出 MCP 服务器状态 |
//! | `call_tool` | `ToolCallResult` | 调用 MCP 工具 |
//! | `get_settings` | `AppSettings` | 获取应用配置 |
//! | `save_settings` | `()` | 保存应用配置 |
//! | `test_ai_provider` | `String` | 测试 AI Provider 连接 |
//!
//! 所有命令返回 `Result<T, String>` 格式，前端通过 `invoke` 调用。

pub mod commands;

pub use commands::*;
