//! AGENT-06：所有 Agent 读取共用的完整信封双预算。
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
    let content_units = content.as_str().map_or_else(
        || serde_json::to_string(&content).unwrap().encode_utf16().count(),
        |s| s.encode_utf16().count(),
    );
    let empty = content.as_array().is_some_and(Vec::is_empty) || content.as_str() == Some("");
    let mut out = json!({"snapshot":snapshot,"content":content,"empty":empty,"range":range,"truncated":truncated,"nextCursor":cursor,"omitted":{"page":[],"complete":!truncated},"anchorCounts":{"sourceUtf16":0,"presentationUtf16":0,"sourceScalars":0,"presentationScalars":0,"scope":"notApplicable"},"usage":{"contentUtf16":content_units,"responseBytes":0,"estimatedTokens":0}});
    if let Ok(value) = serde_json::from_str::<Value>(snapshot) {
        out["snapshot"] = value;
    }
    measure(&mut out);
    out
}
pub fn measure(out: &mut Value) {
    loop {
        let size = serde_json::to_vec(&out).unwrap().len();
        let tokens = size.div_ceil(4);
        if out["usage"]["responseBytes"] == size && out["usage"]["estimatedTokens"] == tokens {
            break;
        }
        out["usage"]["responseBytes"] = json!(size);
        out["usage"]["estimatedTokens"] = json!(tokens);
    }
}
pub fn fits(v: &Value, b: Budget) -> bool {
    v["usage"]["contentUtf16"].as_u64().unwrap() <= b.limit as u64
        && crate::transport::common_bytes(v, false) <= b.max_bytes
}
/// 所有读取的最长完整前缀选择；末页省去游标，字节预算不是单调的。
pub fn longest_prefix<T>(
    first: usize,
    last: usize,
    b: Budget,
    object: Value,
    mut candidate: impl FnMut(usize) -> (Value, T),
) -> Result<(Value, T)> {
    let mut best = None;
    let mut failure = None;
    for end in first..=last {
        let (value, state) = candidate(end);
        if fits(&value, b) {
            best = Some((value, state));
        } else {
            if failure.is_none() {
                failure = Some(too_small(&value, object.clone()));
            }
            if value["usage"]["contentUtf16"].as_u64().unwrap() > b.limit as u64 {
                break;
            }
        }
    }
    best.ok_or_else(|| failure.unwrap_or_else(|| error("AGENT_BAD_CURSOR", "没有可返回单位")))
}
pub fn too_small(v: &Value, object: Value) -> crate::QueryError {
    let mut e = error("AGENT_BUDGET_TOO_SMALL", "完整记录无法容纳；未返回内容、未消费游标");
    let bytes = crate::transport::common_bytes(v, false);
    e.details = json!({"object":object,"minLimit":v["usage"]["contentUtf16"],"minBytes":bytes});
    if v["usage"]["contentUtf16"].as_u64().unwrap() > 1048576 || bytes > 4194304 {
        e.code = "AGENT_UNIT_TOO_LARGE".into();
    }
    e
}
