//! BIND-11：定位首个正文段落，以 ReplaceInlines 替换内容并另存。
use rsword::bind::native::edit_op_from_json;
use rsword::{EditContext, EditOp, EditSession};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("用法：edit input.docx output.docx".into());
    }
    let mut session = EditSession::open(&std::fs::read(&args[1])?)?;
    let model = rsword::bind::native::document_json(
        session.package(),
        session.document(),
        Default::default(),
    )
    .0;
    let para = model["main"]
        .as_array()
        .ok_or("缺少正文")?
        .iter()
        .find(|block| block["kind"] == "text")
        .ok_or("没有顶层可编辑段落")?["node"]
        .clone();
    let json = json!({"op":"replaceInlines", "para":para,
        "inlines":[{"kind":"run","value":{"text":"由 rsword 修改","props":null}}]});
    // 本例纯文本载荷不引入新 XML 名。一般 JSON 输入应使用 apply_edit_json 管理目标 DOM。
    let op: EditOp = edit_op_from_json(&json.to_string(), &mut session.dom().clone())?;
    assert!(matches!(op, EditOp::ReplaceInlines { .. }));
    session.apply(op, &EditContext::default())?;
    // create_new 防止误覆盖输入或已有文件。
    use std::io::Write;
    let bytes = session.save()?;
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
    output.write_all(&bytes)?;
    Ok(())
}
