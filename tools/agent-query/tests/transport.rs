//! AGENT-06：实际信封成本与共同分页；不以同一路径计算期望长度。
use rsword_agent_query::{
    budget::{self, Budget},
    tools::{self, Tool},
    transport::{self, Shape},
};
use serde_json::{Value, json};
#[test]
fn agent_06_shapes_charge_actual_bytes_and_share_minimum() {
    let v = tools::fixed(Tool::Version, tools::version(), Budget::CONTEXT).unwrap();
    let text = Shape::Text.result(&v, false);
    let structured = Shape::Structured.result(&v, false);
    let a: Value = serde_json::from_str(text["content"][0]["text"].as_str().unwrap()).unwrap();
    let b = &structured["structuredContent"];
    assert_eq!(a["usage"]["responseBytes"], text.to_string().len());
    assert_eq!(b["usage"]["responseBytes"], structured.to_string().len());
    assert!(text.get("structuredContent").is_none());
    assert_eq!(structured["content"], json!([{"type":"text","text":"Read structuredContent."}]));
    let mut a = a;
    let mut b = b.clone();
    // 只允许这两个实际传输计数字段不同，其余字段一律相等。
    for value in [&mut a, &mut b] {
        value["usage"].as_object_mut().unwrap().remove("responseBytes");
        value["usage"].as_object_mut().unwrap().remove("estimatedTokens");
    }
    assert_eq!(a, b);
    let small = text.to_string().len().min(structured.to_string().len());
    let large = text.to_string().len().max(structured.to_string().len());
    assert!(small < large, "此负例必须真有成本差");
    let between = Budget { limit: 8000, max_bytes: small };
    assert!(!budget::fits(&v, between));
    assert_eq!(budget::too_small(&v, Value::Null).details["minBytes"], large);
    assert_eq!(transport::common_bytes(&v, false), large);
    assert!(budget::fits(&v, Budget { max_bytes: large, ..between }));
}

/// WP1-2 D-3 守门：`Shape::result` 的算术定点必须与逐轮整包序列化的老实现同结果。
///
/// 账本与 `Shape::result` 现在共用同一套算术（`assemble::fixpoint` 等），所以拿
/// 账本去校验它是循环论证；这里把**老循环**原样抄成本地 oracle，在真实语料的响应
/// 上逐个比对，成功与失败两种信封都覆盖。
#[test]
fn agent_10_shape_result_matches_serialize_loop_oracle() {
    /// 老实现：每轮 `wrap(..).to_string()` 求长度，直到 usage 稳定。
    fn oracle(shape: Shape, value: &Value, failed: bool) -> Value {
        let mut value = value.clone();
        if value.get("usage").is_some() {
            loop {
                let bytes = shape.wrap(&value, failed).to_string().len();
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
        shape.wrap(&value, failed)
    }
    use rsword::package::Package;
    use rsword_agent_query::session::{ReadRequest, ReadTool, Sessions};
    #[path = "../../../crates/rsword/tests/common/mod.rs"]
    mod common;
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    let mut checked = 0usize;
    let mut values: Vec<Value> = vec![
        tools::fixed(Tool::Version, tools::version(), Budget::CONTEXT).unwrap(),
        // 没有 usage 的载荷：`result` 必须原样包装，不进定点。
        json!({"code":"AGENT_IO","message":"x"}),
        // usage 数字初值荒谬时也要落到同一个不动点。
        json!({"content":"abc","usage":{"contentUtf16":3,"responseBytes":999999,"estimatedTokens":1}}),
        json!({"content":"\"\\\n\u{1}控制字符","usage":{"contentUtf16":9,"responseBytes":0,"estimatedTokens":0}}),
    ];
    for (i, path) in paths.into_iter().enumerate() {
        if i % 37 != 0 {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let mut sessions = Sessions::default();
        if Package::open(&bytes).is_err() {
            continue;
        }
        let Ok(id) = sessions.open(&bytes) else { continue };
        for tool in [ReadTool::Text, ReadTool::Outline, ReadTool::Context] {
            let options = if tool == ReadTool::Context {
                json!({"anchor":sessions.anchor(&id, rsword::agent::text::Scope::Main, 0).unwrap(),"before":0,"after":80})
            } else {
                json!({})
            };
            if let Ok(v) = sessions.read(&id, &ReadRequest { tool, options }, None, None, None) {
                values.push(v);
            }
        }
    }
    assert!(values.len() > 40, "取样过少: {}", values.len());
    for value in &values {
        for shape in [Shape::Text, Shape::Structured] {
            for failed in [false, true] {
                assert_eq!(
                    shape.result(value, failed),
                    oracle(shape, value, failed),
                    "{shape:?} failed={failed} 与老循环不一致"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 160, "覆盖过少: {checked}");
}
