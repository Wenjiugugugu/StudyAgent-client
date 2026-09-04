//! Tauri 命令按业务领域拆分。
//!
//! 每个领域模块负责自己的命令实现，legacy.rs 仅保留跨领域共享的 DTO
//! 和辅助函数。通过统一 re-export，main.rs 与前端不需要感知文件拆分。

pub mod legacy;

pub mod analytics_commands;
pub mod app_commands;
pub mod briefing_commands;
pub mod dashboard_commands;
pub mod debug_commands;
pub mod focus_commands;
pub mod goal_commands;
pub mod mcp_commands;
pub mod plan_commands;
pub mod progress_commands;
pub mod provider_commands;
pub mod review_commands;
pub mod settings_commands;
pub mod state_commands;
pub mod textbook_commands;
pub mod update_commands;

pub use analytics_commands::*;
pub use app_commands::*;
pub use briefing_commands::*;
pub use dashboard_commands::*;
pub use debug_commands::*;
pub use focus_commands::*;
pub use goal_commands::*;
pub use mcp_commands::*;
pub use plan_commands::*;
pub use progress_commands::*;
pub use provider_commands::*;
pub use review_commands::*;
pub use settings_commands::*;
pub use state_commands::*;
pub use textbook_commands::*;
pub use update_commands::*;
