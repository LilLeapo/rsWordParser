# Rust binding

本包包含 `rsword` 库及其公开的 Rust 原生 binding：`rsword::bind::native::SessionTable`。
其他 Rust crate 可以直接依赖它，调用 `open → document → apply → save → close`。
源码随调用方的目标平台编译；最低 Rust 版本见 `rsword/Cargo.toml` 的 `rust-version`。

## 接入现有 Rust 项目

将压缩包解压到项目旁边，在调用方的 `Cargo.toml` 中加入（按实际位置调整路径）：

```toml
[dependencies]
rsword = { path = "../rsword-rustbinding/rsword" }
serde_json = "1"
```

```rust
use rsword::bind::native::SessionTable;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sessions = SessionTable::default();
    let id = sessions.open(&std::fs::read("input.docx")?, None)?;
    let document = sessions.document(&id, None)?;
    println!("{document}");
    let saved: Vec<u8> = sessions.save(&id, None)?;
    assert_eq!(saved, std::fs::read("input.docx")?);
    sessions.close(&id);
    Ok(())
}
```

`document` 返回只读 JSON 投影；修改通过 `apply` 传入 EditOp JSON。
`save` 返回 DOCX 字节，由调用方决定是否写文件。也可使用 `rsword::EditSession` 和
`rsword::EditOp` 的 Rust 类型接口。

## 运行包内示例

进入解压后的 `rsword-rustbinding/example`，传入 DOCX 的绝对路径：

```sh
cargo run --locked --release --bin read -- /absolute/path/input.docx
cargo run --locked --release --bin edit -- /absolute/path/input.docx "替换文字"
```

`read` 输出段落文本；`edit` 输出候选操作，修改首个正文段落，保存到内存并重新打开验证。
编辑示例要求文档含可编辑的顶层正文段落，不会覆盖输入文件。

发布前会在仓库外的独立 Cargo 项目实际编译并运行这两个示例。
包内的 `rsword/` 是 `cargo package` 生成的完整独立 crate，包含 binding 源码、属性 schema 和构建脚本；
`example/` 包含可直接运行的 Cargo 项目与锁文件。
