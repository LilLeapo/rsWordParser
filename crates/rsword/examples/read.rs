//! BIND-11：读取主文档的大纲与段落文字，包括表格内段落。
use rsword::bind::native::SessionTable;
use serde_json::Value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("用法：read input.docx")?;
    let mut sessions = SessionTable::default();
    let id = sessions.open(&std::fs::read(path)?, None)?;
    let model: Value = serde_json::from_str(&sessions.document(&id, None)?)?;
    let mut work = vec![&model["main"]];
    while let Some(value) = work.pop() {
        match value {
            Value::Array(items) => work.extend(items.iter().rev()),
            Value::Object(fields) => {
                if value["kind"] == "text" {
                    if let Some(level) = value["textKind"]["level"].as_u64() {
                        print!("[标题 {level}] ");
                    }
                    let mut inlines = vec![&value["inlines"]];
                    while let Some(inline) = inlines.pop() {
                        if inline["kind"] == "run" {
                            print!("{}", inline["text"].as_str().unwrap_or_default());
                        } else if let Some(items) = inline.as_array() {
                            inlines.extend(items.iter().rev());
                        } else if inline["kind"] == "field" {
                            inlines.push(&inline["result"]);
                        }
                    }
                    println!();
                } else {
                    work.extend(fields.values().rev());
                }
            }
            _ => {}
        }
    }
    sessions.close(&id);
    Ok(())
}
