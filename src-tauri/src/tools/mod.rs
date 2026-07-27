//! MCP Tool Layer — 统一工具调用入口
//!
//! 设计要点：
//! - Tool Dispatcher 统一路由工具调用到对应 MCP server
//! - MCP Client 连接 MCP server，列出工具，调用工具
//! - 支持 stdio / SSE / WebSocket 三种传输方式

pub mod dispatcher;
pub mod mcp;

pub use dispatcher::*;
pub use mcp::*;
