//! AGENT-06：本轮导航共用的完整记录预算；会话游标注册由 9.4 接入。
use crate::{Result, error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    pub limit: usize,
    pub max_bytes: usize,
}
impl Budget {
    pub const OUTLINE: Self = Self { limit: 4000, max_bytes: 16000 };
    pub const FIND: Self = Self { limit: 4000, max_bytes: 24000 };
    pub const CONTEXT: Self = Self { limit: 8000, max_bytes: 24000 };
    pub fn validate(self) -> Result<Self> {
        if !(1..=1048576).contains(&self.limit) || !(512..=4194304).contains(&self.max_bytes) {
            return Err(error("BIND_BAD_ARGUMENT", "预算超出允许范围"));
        }
        Ok(self)
    }
}
/// nextCursor 为服务端提供的不可伪造句柄；本层不接受客户端提供的 offset。
pub fn envelope(
    snapshot: &str,
    content: Value,
    range: Value,
    truncated: bool,
    cursor: Option<&str>,
) -> Value {
    let content_units = serde_json::to_string(&content).unwrap().encode_utf16().count();
    let empty = content.as_array().is_some_and(Vec::is_empty);
    let mut out = json!({"snapshot":snapshot,"content":content,"empty":empty,"range":range,"truncated":truncated,"nextCursor":cursor,"omitted":{"page":[],"complete":!truncated},"anchorCounts":{"sourceUtf16":0,"presentationUtf16":0,"sourceScalars":0,"presentationScalars":0,"scope":"notApplicable"},"usage":{"contentUtf16":content_units,"responseBytes":0,"estimatedTokens":0}});
    loop {
        let size = serde_json::to_vec(&out).unwrap().len();
        let tokens = size.div_ceil(4);
        if out["usage"]["responseBytes"] == size && out["usage"]["estimatedTokens"] == tokens {
            break;
        }
        out["usage"]["responseBytes"] = json!(size);
        out["usage"]["estimatedTokens"] = json!(tokens);
    }
    out
}
pub fn fits(v: &Value, b: Budget) -> bool {
    v["usage"]["contentUtf16"].as_u64().unwrap() <= b.limit as u64
        && v["usage"]["responseBytes"].as_u64().unwrap() <= b.max_bytes as u64
}
pub fn too_small(v: &Value, object: Value) -> crate::QueryError {
    let mut e = error("AGENT_BUDGET_TOO_SMALL", "完整记录无法容纳；未返回内容、未消费游标");
    e.details = json!({"object":object,"minLimit":v["usage"]["contentUtf16"],"minBytes":v["usage"]["responseBytes"]});
    if v["usage"]["contentUtf16"].as_u64().unwrap() > 1048576
        || v["usage"]["responseBytes"].as_u64().unwrap() > 4194304
    {
        e.code = "AGENT_UNIT_TOO_LARGE".into();
    }
    e
}
