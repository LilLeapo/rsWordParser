//! AGENT-06/10：一份业务载荷，两种 MCP 信封；共同分页不依赖客户端选择。
use crate::assemble::{digits, escape_extra, fixpoint, shells};
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
            if let Some((bytes, tokens)) = self.settle(&value, failed) {
                value["usage"]["responseBytes"] = json!(bytes);
                value["usage"]["estimatedTokens"] = json!(tokens);
            } else {
                // 账本没收敛（正常输入不会）：回退到逐轮整包序列化的老定点。
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
        }
        self.wrap(&value, failed)
    }
    /// `usage` 定点的算术版（docs/21 WP1-2 D-3）：内层只序列化一次，剩下的用整数算。
    ///
    /// 两形态的包装长度都是 `外壳常量 + 内层长度 (+ Text 的二次转义增量)`，而内层长度
    /// 只随 `responseBytes` / `estimatedTokens` 的**十进制位数**变化——数字本身不含可转义
    /// 字节，所以转义增量是常量。老实现每轮都 `wrap(..).to_string()`，Text 形态因此把
    /// 内层序列化并转义三四遍；这里只序列化一次求出 `base` 与转义增量。
    ///
    /// `None` 表示没在迭代上限内收敛，调用方回退到老循环。
    fn settle(self, value: &Value, failed: bool) -> Option<(usize, usize)> {
        let bytes = serde_json::to_vec(value).unwrap();
        let rb = value["usage"]["responseBytes"].as_u64()? as usize;
        let et = value["usage"]["estimatedTokens"].as_u64()? as usize;
        let base = bytes.len().checked_sub(digits(rb) + digits(et))?;
        let (text_shell, structured_shell) = shells();
        let k = match self {
            Self::Text => text_shell + base + escape_extra(&bytes),
            Self::Structured => structured_shell + base,
        };
        // 外壳常量是按 `isError:false` 量的；失败信封里 `true` 比 `false` 短一个字节。
        let k = if failed { k - 1 } else { k };
        fixpoint(k, (rb, et))
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
