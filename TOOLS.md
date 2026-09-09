# 工具链版本备忘（M8′ 8.0②）

rsword-js 的 wasm 构建对版本敏感的两处：**wasm-bindgen-cli 必须与 Cargo.lock 里的
`wasm-bindgen` 精确同版本**（生成的 glue 与运行时不匹配会链接失败或行为怪异），node
只用于差分脚本（`tools/js-parity/`），≥ 22。

| 工具 | 版本 | 安装 | 核对 |
| --- | --- | --- | --- |
| `wasm-bindgen-cli` | `0.2.128`（与根 Cargo.lock 的 `wasm-bindgen = "=0.2.128"` 一致） | `cargo install wasm-bindgen-cli --version 0.2.128 --locked` | `wasm-bindgen --version` |
| `wasm32-unknown-unknown` target | stable 工具链自带 | `rustup target add wasm32-unknown-unknown` | `rustup target list --installed` |
| `wasm-opt`（可选，binaryen） | 任意近期版 | `brew install binaryen` | `wasm-opt --version`；没装时 `tools/build-js.sh` 只跳过 `-Oz` |
| node | ≥ 22 | 只跑 `tools/js-parity/`，不构建任何东西 | `node --version` |

升 `wasm-bindgen` 的步骤：改 `crates/rsword-js/Cargo.toml` 的 `=0.2.x` → `cargo update -p
wasm-bindgen` → 按锁文件里的新版本重装 cli → 重跑 `tools/build-js.sh` 与 `--via-js` 门。

## 常用命令

```sh
tools/build-js.sh                                  # 构建 wasm + glue → crates/rsword-js/pkg/
cargo run -p diff-parse -- --via-js --scope all    # 绑定等价门（node + pkg 就位后）
node tools/js-parity/parse_parity.mjs --help       # 单独跑某个 runner（见脚本头注释）
```
