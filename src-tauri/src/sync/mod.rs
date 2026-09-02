//! Sync 模块 — 跨端同步（滴答清单）
//!
//! 方案定稿（2026-08-28，2026-08-31 改为 Open API 直连）：
//! - 滴答清单为「任务状态」的唯一事实源；计划内容以本地计划 JSON 为准（对账下行覆盖）。
//! - 基于 TickTick / Dida365 Open API（`https://api.dida365.com/open/v1/*`）直连，
//!   认证方式为 Personal Access Token（`Authorization: Bearer <token>`）。
//! - 只允许读取/修改带 `studyagent` 来源标记的任务，用户自建任务一律不动。
//! - 标签仅两类：`studyagent`（归属）+ 学科（数学/英语/政治/专业课）。
//! - 同步粒度为「按日对账 reconcile」：计划任务 ↔ 滴答任务（增删改 incremental）。

pub mod dida;

pub use dida::*;
