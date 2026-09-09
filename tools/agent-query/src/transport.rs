//! AGENT-06/10：一份业务载荷，两种 MCP 信封；共同分页不依赖客户端选择。
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Text,
    Structured,
}
impl Shape {
    /// 计费包括完整 CallToolResult，不包括外层 JSON-RPC id/framing。
    /// structured 模式的短说明不是第二份载荷，也必须计费。
    pub fn wrap(self, value: &Value, failed: bool) -> Value {
        match self {
            Self::Text => {
                json!({"content":[{"type":"text","text":value.to_string()}],"isError":failed})
            }
            Self::Structured => {
                json!({"content":[{"type":"text","text":"Read structuredContent."}],"structuredContent":value,"isError":failed})
            }
        }
    }
    pub fn result(self, value: &Value, failed: bool) -> Value {
        let mut value = value.clone();
        if value.get("usage").is_some() {
            loop {
                let bytes = self.wrap(&value, failed).to_string().len();
                let tokens = bytes.div_ceil(4);
                if value["usage"]["responseBytes"] == bytes
                    && value["usage"]["estimatedTokens"] == tokens
                {
                    break;
                }
                value["usage"]["responseBytes"] = json!(bytes);
                value["usage"]["estimatedTokens"] = json!(tokens);
            }
        }
        self.wrap(&value, failed)
    }
}
/// minBytes 与前缀选择使用相同的两形态上界；不能按实际选用形态放宽。
pub fn common_bytes(value: &Value, failed: bool) -> usize {
    [Shape::Text, Shape::Structured]
        .into_iter()
        .map(|s| s.result(value, failed).to_string().len())
        .max()
        .unwrap()
}
