# fuzz

`spec/11-testing.md` TEST-06。需要 nightly 与 cargo-fuzz：

```sh
rustup toolchain install nightly --profile minimal
cargo +nightly install cargo-fuzz
cargo +nightly fuzz run fuzz_xml -- -max_total_time=600
cargo +nightly fuzz run fuzz_zip -- -max_total_time=600
cargo +nightly fuzz run fuzz_instr -- -max_total_time=600
```

| 目标 | 输入 | 不变式 |
| --- | --- | --- |
| `fuzz_xml` | 任意字节作为 part | 不 panic；成功时 `serialize == input`（转码 part 除外） |
| `fuzz_zip` | 任意字节作为 docx | 不 panic；成功打开时无编辑保存字节相同 |
| `fuzz_instr` | 任意字符串当字段指令 | 不 panic；`raw` 是原文、token 文本不超过原文、`Nested` 只引用给定字段、`Unknown` 关键字为大写 |

`corpus/` 下**只放种子**：`fuzz_xml` / `fuzz_zip` 是从 `corpus/synthetic` 抽的小 part 与小 docx，
`fuzz_instr` 是手写的指令样本（各种开关、引号转义、嵌套占位、未闭合引号）。跑一轮 fuzz 会往同一个
目录里写几百到上万个覆盖单元——**那些不要提交**，本地跑完 `git clean -fd fuzz/corpus` 即可（CI 的
检出是一次性的）。`artifacts/` 是崩溃样本（发现后最小化并固化为回归测试）。M7 加 `fuzz_edit`。

已跑过的门：`fuzz_instr` 13,572,886 次执行 / 601 秒无崩溃（2026-09-05）。
