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
