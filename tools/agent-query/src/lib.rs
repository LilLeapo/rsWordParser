//! AGENT-03/04/05：工具侧共享查询；依赖与可终止工作进程不进入 DOCX 内核。
pub mod budget;
pub mod detail;
pub mod find;
pub mod nav;
pub mod search;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryError {
    pub code: String,
    pub message: String,
    pub details: Value,
}
pub type Result<T> = std::result::Result<T, QueryError>;
pub fn error(code: &str, message: impl Into<String>) -> QueryError {
    QueryError { code: code.into(), message: message.into(), details: json!({}) }
}
impl From<rsword::agent::anchors::AgentError> for QueryError {
    fn from(e: rsword::agent::anchors::AgentError) -> Self {
        error(e.code, e.message)
    }
}
impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for QueryError {}
