# fuzz

`spec/11-testing.md` TEST-06。需要 nightly 与 cargo-fuzz：

```sh
rustup toolchain install nightly --profile minimal
cargo +nightly install cargo-fuzz
cargo +nightly fuzz run fuzz_xml -- -max_total_time=600
cargo +nightly fuzz run fuzz_zip -- -max_total_time=600
```

| 目标 | 输入 | 不变式 |
| --- | --- | --- |
| `fuzz_xml` | 任意字节作为 part | 不 panic；成功时 `serialize == input`（转码 part 除外） |
| `fuzz_zip` | 任意字节作为 docx | 不 panic；成功打开时无编辑保存字节相同 |

`corpus/` 下是种子（从 `corpus/synthetic` 抽取的小 part 与小 docx），`artifacts/` 是崩溃样本（发现后最小化并固化为回归测试）。
M2 加 `fuzz_instr`，M7 加 `fuzz_edit`。
