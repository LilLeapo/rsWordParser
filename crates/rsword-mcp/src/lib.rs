//! AGENT-10：原生 stdio MCP 适配；工具、预算、游标和事务均复用 Agent 层。
pub mod protocol;
mod service;
pub use service::{Config, Service};
