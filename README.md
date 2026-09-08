# rsWordParser

**独立的高保真 DOCX 读写内核**（Rust）。读取文档事实和内容，接受 `EditOp`，以字节级局部补丁写回。不做布局与渲染。

文件是真相：未编辑内容保持原字节，模型 JSON 是只读投影；修改必须经编辑操作提交。交付以 Rust crate 为先，wasm 是同一原生协议的薄壳。genoffice 仅用于差分测试。

## Rust API

推荐从 `rsword::bind::native::SessionTable` 开始，完成 `open → document → apply → save → close`。
也可使用根部的 `EditSession` / `EditOp` 做上下文化编辑。错误提供稳定机器码，失败操作不留下半修改状态。

BIND-11 的稳定面为 `bind::native` 全部导出和根部列出的核心类型。旧模块路径保留一个观察版本，
在文档中隐藏，不承诺其内部类型或辅助函数稳定。`DiagCode` 只追加变体；可扩展类型使用
`#[non_exhaustive]`，值对象的例外在各类型文档中注明。破坏性变更只在 crate minor 版本做，
变更记录须给出迁移方式；协议版本独立演进，目前仍为 `native/0`。

隐藏项仍可被下游调用。负责人于 2026-09-09 决定将缩小实际 semver 面推迟到观察期之后，
门 3 的“缺省小面”这半条未达成。稳定面文档由 audit cfg 下的 rustc 硬检查，注解位置与
[成文清单](docs/11-public-api.md) 双向锁定。

Feature 名称：默认 `native`，另有 `serde`、`wasm`、测试专用 `compat-ts`。
**8.5 只固定 feature 名称与依赖关系；compat_ts 的实际编译门控留到 8.7，当前默认构建仍含兼容层。
关闭 serde feature 也暂不移除共享 serde 依赖。门 3 的默认构建排除兼容层这一半尚未完成。**

## 读取大纲和文本

在仓库根目录运行：

```sh
cargo run -p rsword --example read -- corpus/synthetic/bidi__001.docx
```

对应 `crates/rsword/examples/read.rs`（示例使用 `serde_json` 读取协议值）：

```rust
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
```

## 替换段落并另存

输出必须是不存在的新文件；示例不会覆盖输入。

```sh
cargo run -p rsword --example edit -- corpus/synthetic/bidi__001.docx target/example-edited.docx
```

对应 `crates/rsword/examples/edit.rs`：

```rust
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
```

## Agent 输入输出边界

示例用确定性规划器代替外部模型服务：读取模型 JSON，打印候选 EditOp JSON，经协议 apply，
保存并重新打开验证结果。不会调用外部服务，也不覆盖文件。

```sh
cargo run -p rsword --example agent -- corpus/synthetic/bidi__001.docx "新的段落内容"
```

对应 `crates/rsword/examples/agent.rs`：

```rust
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
```

## 开发与验收

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo doc --no-deps
cargo test --workspace
cargo test --workspace --release
```

三个示例在 CI 实际执行。差分门继续使用 synthetic / real 语料，与自身协议测试并行保留。
测试基准不是格式权威，有意差异逐项登记，禁止手改 `*.expected.json` 和 `*.save.*.json`。

仓库约束见 [CLAUDE.md](CLAUDE.md)，当前能力与实测数字见 [docs/05](docs/05-status.md)，
逐任务进度和偏差见 [docs/04](docs/04-dev-plan.md)，冻结架构见 [docs/03](docs/03-architecture-v3.md)，
原生协议见 [spec/21](spec/21-bind.md)。
