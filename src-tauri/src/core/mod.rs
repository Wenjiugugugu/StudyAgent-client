//! Core Module — 业务逻辑层
//!
//! Core 层是 StudyAgent 的核心业务逻辑，包含：
//! - Dashboard：数据聚合与汇总
//! - Planner：调用 AI Service 生成周计划
//! - Scheduler：从周计划生成日计划（无 AI）
//! - Review Agent：调用 AI Service 生成复盘
//! - Briefing Agent：调用 AI Service 生成每日简报
//! - User Model：用户画像读取
//! - Analytics：学习数据分析聚合

pub mod adaptive_planner;
pub mod analytics;
pub mod briefing;
pub mod chapter_seq;
pub mod dashboard;
pub mod estimated_time;
pub mod goal_planner;
pub mod planner;
pub mod planning;
pub mod professional;
pub mod progress_sync;
pub mod review;
pub mod scheduler;
pub mod user_model;

pub use analytics::*;
pub use briefing::*;
pub use dashboard::*;
pub use planner::*;
pub use review::*;
pub use scheduler::*;
pub use user_model::*;
