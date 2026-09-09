//! BIND-11 / M9：模型 JSON 进入规划器，EditOp JSON 出来，再交协议校验执行。
use rsword::bind::native::SessionTable;
use serde_json::{Value, json};

// 最小确定性规划器；接入模型服务时保留同样的输入、输出和 apply 边界。
fn propose(model: &Value, replacement: &str) -> Result<Value, &'static str> {
    let para = model["main"]
        .as_array()
        .ok_or("缺少正文")?
        .iter()
        .find(|block| block["kind"] == "text")
        .ok_or("没有顶层可编辑段落")?;
    Ok(json!({"op":"replaceInlines", "para":para["node"],
        "inlines":[{"kind":"run","value":{"text":replacement,"props":null}}]}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("用法：agent input.docx [替换文字]")?;
    let replacement = std::env::args().nth(2).unwrap_or_else(|| "Agent 编辑示例".into());
    let mut sessions = SessionTable::default();
    let id = sessions.open(&std::fs::read(path)?, None)?;
    let model: Value = serde_json::from_str(&sessions.document(&id, None)?)?;
    let operation = propose(&model, &replacement)?.to_string();
    println!("{operation}"); // 可审计输出，尚未修改会话。
    sessions.apply(&id, &operation, None)?;
    let saved = sessions.save(&id, None)?;
    let reopened = sessions.open(&saved, None)?;
    let model: Value = serde_json::from_str(&sessions.document(&reopened, None)?)?;
    assert!(
        model["main"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["inlines"][0]["text"] == replacement)
    );
    sessions.close(&reopened);
    sessions.close(&id);
    Ok(())
}
