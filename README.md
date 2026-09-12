# rsWordParser

**高保真 DOCX 读写内核**（Rust）。把 `.docx` 解析成文档模型，接受编辑操作，以**字节级局部补丁**写回。

它解决的是一个具体问题：**用程序改 Word 文档，而不破坏你没碰的那部分。** 常见的 docx 库会重新序列化整个包，
把原作者的格式、未知元素、压缩字节全部改写一遍——文档在 Word 里可能还能打开，但已经不是原来那份。

## 四条不变式

这是本项目的全部意义所在，每一条都在调试构建里有自检、在 CI 里有门：

1. **无编辑保存 → 输出与输入字节相同。** 不是"语义等价"，是 `cmp` 无差异。
2. **编辑一个段落 → 其他 zip 条目的 CRC 与压缩字节相同**，`document.xml` 里其他块的原字节原样出现。
3. **规范状态 = DOM + Span。** `model` / `resolve` 是可重建的只读投影，禁止把投影当真相写回。
4. **病态输入局部降级，不 panic、不丢字节。** 三千层嵌套的段落会变成 `Protected` 块，其余部分照常可编辑。

自己验第 1、2 条：

```sh
rsword check input.docx --json --limit 100000 --maxBytes 400000
# noEditSaveIdentity / dirtyPropagation / engineInvariantDiagnostics 应全为 true
```

改完之后比 zip 条目的 CRC，只有被编辑的 part 该变。

## 不做什么

**不做布局，不做渲染。** 它不知道一行能放几个字、一页能放几行，所以像"填进去的内容太长导致换行"
这类问题它既不会报错也无法预防——那要靠人看渲染结果（见 [docs/20](docs/20-fill-playbook.md) §5）。

也不是 Word 的替代品，不生成 PDF，不做公式排版。目标形态是 Word / WPS 的**外挂工具**。

## 四种用法

| 形态 | 适合 | 入口 |
| --- | --- | --- |
| **Rust crate** | 嵌进自己的程序 | `rsword::bind::native::SessionTable` |
| **CLI** | 脚本、一次性批处理 | `rsword` |
| **MCP server** | 接给 Agent（Claude Code / Cursor 等） | `rsword-mcp`（stdio） |
| **wasm** | 浏览器 / Node | `crates/rsword-js/pkg` |

四者走**同一套原生协议**（`native/0`），所以四条不变式在哪个形态下都成立。

## 安装

命令行与 MCP：

```sh
cargo install --locked --path crates/rsword-cli    # → rsword
cargo install --locked --path crates/rsword-mcp    # → rsword-mcp
```

装到 `~/.cargo/bin`，不要直接把配置指向 `target/release/`——那里会被 `cargo clean` 清掉。

作为依赖（`publish = false`，尚未上 crates.io，用 path 或 git 依赖）：

```toml
[dependencies]
rsword = { path = "../rsWordParser/crates/rsword" }
```

wasm 产物（需要 `wasm32-unknown-unknown` target 与**版本对得上 `Cargo.lock`** 的 `wasm-bindgen-cli`）：

```sh
tools/build-js.sh            # → crates/rsword-js/pkg/
```

MSRV `1.88`，edition 2024。Feature：默认 `native`，另有 `serde`、`wasm`，以及**测试专用**的 `compat-ts`
（差分测试和 `diff-parse` 需要显式开启，默认构建不编译它）。

## 快速开始

### CLI

```sh
rsword outline 文档.docx --json                                  # 标题层级与块范围
rsword text    文档.docx --limit 100000 --maxBytes 2000000       # 正文
rsword find    文档.docx --pattern "关键词" --limit 1000 --maxBytes 20000
```

改文档：把操作写进 `ops.json`，**先干跑再落盘**。

```sh
rsword preview 文档.docx --ops ops.json                          # 克隆执行，不写文件
rsword ops     文档.docx --ops ops.json --output 新.docx --report r.json
```

`ops.json` 的 Agent 层形态（`selector` 里的 object 句柄来自 `outline` / `text` 的输出）：

```json
{"operations":[{"action":"replaceText",
  "selector":{"scope":[{"flow":0,"kind":"paragraph","node":2,"part":4}],"find":"旧文字"},
  "text":"新文字"}]}
```

加 `--native-ops` 则直接吃引擎层的 `EditOp`（`{"op":"insertText","at":{"para":N,"offset":0},"text":"…"}`），
共 66 个变体，绕过选择器直接给 node id。完整命令、退出码与预算语义见 [docs/18](docs/18-cli.md)。

### MCP

```sh
claude mcp add rsword -s user -- ~/.cargo/bin/rsword-mcp
```

16 个工具：`open` `close` `outline` `text` `find` `context` `model` `preview` `edit` `save`
`summary` `diff` `media` `addMedia` `check` `version`。会话式流程是
`open → outline → text/find → preview → edit → save → close`。连接示例与计费边界见 [docs/19](docs/19-mcp.md)。

### Rust

会话生命周期就是这五步：

```rust
use rsword::bind::native::SessionTable;

let mut sessions = SessionTable::default();
let id = sessions.open(&std::fs::read("in.docx")?, None)?;
let model = sessions.document(&id, None)?;      // 只读投影（JSON）
sessions.apply(&id, &op_json, None)?;           // 原子应用；失败不留半修改状态
let bytes = sessions.save(&id, None)?;          // 由调用方写盘
sessions.close(&id);
```

三个可运行示例在 `crates/rsword/examples/`，**CI 每次都真跑**：

```sh
cargo run -p rsword --example read  -- corpus/synthetic/bidi__001.docx
cargo run -p rsword --example edit  -- corpus/synthetic/bidi__001.docx target/edited.docx
cargo run -p rsword --example agent -- corpus/synthetic/bidi__001.docx "新的段落内容"
```

`read` 遍历模型打印大纲与文字，`edit` 用底层 `EditSession` 换掉一个段落，
`agent` 演示 Agent 边界：读模型 → 打印候选操作 → apply → 保存 → 重开验证。写入一律 `create_new`，不覆盖输入。

### wasm

有状态的 `SessionTable`，与 Rust 侧同构。**用仓库自带的加载器**，不要自己写 init：

```js
import { loadBinding } from './tools/js-parity/wasm-loader.mjs'
const glue = await loadBinding('crates/rsword-js/pkg', true)
const t = new glue.SessionTable()
const id = t.open(bytes, '{"expectProtocol":"native/0"}')
const doc = JSON.parse(t.document(id, null))
t.apply(id, JSON.stringify({op:'insertText', at:{para: node, offset:0}, text:'…'}), null)
const saved = t.save(id, null)   // Uint8Array，写盘由调用方负责
t.close(id)
```

## 上手最容易踩的四个坑

**1. 参数分两层。** 预算参数（`limit` / `maxBytes` / `cursor` / `expectedVersion` / `sessionId`）在**顶层**，
业务参数在 **`options`** 里。`options` 是 `additionalProperties: false`，塞错会被拒：

```
BIND_BAD_ARGUMENT  Additional properties are not allowed ('limit' was unexpected)
```

见到 `BIND_BAD_ARGUMENT` 找参数结构，见到 `AGENT_BUDGET_TOO_SMALL` 才是真超预算——后者会把
`minBytes` / `minLimit` 直接告诉你。

**2. 默认预算很小**（`limit` 8000、`maxBytes` 24000），稍大的文档必须显式调。

**3. 分页不是可选的。** `truncated` / `hasMore` 为真时必须用 `nextCursor` 续读，
`range.totalBlocks` 可以用来自查读全了没有。

**4. `replaceText` 有一个已登记的引擎缺陷**：`find` 恰好等于某个 run 的全部文本时会静默丢格式或冒用邻居格式
（[docs/04](docs/04-dev-plan.md) §8.0.1）。批量填充场景请按 [docs/20](docs/20-fill-playbook.md) 的作业法走原生三连。

## 文档

| 想知道 | 看 |
| --- | --- |
| 现在能做什么、数字是多少 | [docs/05 现状快照](docs/05-status.md) |
| 冻结架构、分层与核心类型 | [docs/03 架构 v3](docs/03-architecture-v3.md) |
| 逐任务进度、实现偏差、已知缺陷 | [docs/04 开发计划](docs/04-dev-plan.md) |
| 原生协议（模型 JSON / EditOp / 会话） | [spec/21](spec/21-bind.md) |
| 公共 API 稳定面清单 | [docs/13](docs/13-public-api.md) |
| CLI 命令与边界 | [docs/18](docs/18-cli.md) |
| MCP 工具与连接 | [docs/19](docs/19-mcp.md) |
| **往模板里批量填内容的作业法** | [docs/20](docs/20-fill-playbook.md) |
| 在本仓库改代码的硬规则 | [CLAUDE.md](CLAUDE.md) |

## 开发与验收

发布工作流见 [release.yml](.github/workflows/release.yml)：推送任意 tag 或在 Actions 页面手动运行。
在 tag 上运行会创建正式 GitHub Release；在分支上手动运行会创建 `draft-<run_id>` 草稿，指向本次构建提交。
WASM、JS binding、CLI、lib 四类独立并行构建，CLI 平台矩阵也并行；全部成功后将 artifacts 附加到 Release。

| Artifact | 内容 |
| --- | --- |
| `rsword-wasm` | 原始 `rsword_js.wasm`，需经 wasm-bindgen 生成配套绑定后使用 |
| `rsword-jsbinding` | 可直接集成的 web target JS、TypeScript 声明及配套 WASM |
| `rsword-cli-<target>` | Windows amd64、Linux x86_64/aarch64、macOS Intel/Apple Silicon 的原生 CLI，打包为 `.tar.gz` |
| `rsword-lib` | 已构建并经 `cargo package` 独立验证的 Rust 源码 `.crate` 包，可解包后用 path dependency 引入 |

CLI 在各目标系统上构建并运行进程测试；Linux 产物使用 GNU libc（Ubuntu 24.04 构建）。
发布工作流不向 crates.io 或 npm 发布包。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets                        # 必须零告警
cargo clippy --workspace --all-targets --features compat-ts   # 两条腿都要
cargo test --workspace
cargo test --workspace --release                              # 调试与发布腿自检行为不同，都要跑
cargo test --workspace --features compat-ts
cargo doc --no-deps                                           # RUSTDOCFLAGS=-D warnings
```

`fuzz/` **被 `Cargo.toml` 排除在 workspace 外**，所以上面任何命令都编译不到它。
改动 `model` / `xml` 的公共 API 后要手动：

```sh
cd fuzz && cargo check --all-targets
```

差分门（需 `compat-ts`）拿语料里记录的期望输出（`corpus/**/*.expected.json`）当**回归基准，不是格式权威**：

```sh
cargo run -p diff-parse --features compat-ts -- --scope all
cargo run -p diff-parse --features compat-ts -- --corpus corpus/real
```

语料：**799** 份 synthetic（含 208 份保存用例）、**266** 份真实 Word 文档、**38** 份恶意 / 畸形输入。
有意与基准不同的地方逐项登记，所以门断言的是"只有登记过的差异"而不是"零差异"。
**禁止**手改 `corpus/**/*.expected.json` 与 `*.save.*.json`——它们只能由 `tools/export-golden/run.sh` 重新生成。

## 状态

引擎侧（解析、编辑、保存、差分门）稳定；**Agent 任务的真实验收仍有缺口**：`docs/12` 的 22 项只过 3 项，
`updateToc` 尚不支持执行，多项写类操作缺桌面 Word 的"无修复提示"证据。别把"门全绿"当成"已验收"——
当前口径见 [docs/05](docs/05-status.md) 与 [docs/12](docs/12-agent-tasks.md)。

## 许可

双许可，任选其一：

- **Apache License 2.0** —— [LICENSE-APACHE](LICENSE-APACHE)（含明确的专利授权）
- **MIT** —— [LICENSE-MIT](LICENSE-MIT)

`corpus/`、`fixtures/`、`evidence/` 下的文档与产物均为本项目自行制作，同样按上述许可发布。

除你另有明确声明外，你有意提交并纳入本项目的任何贡献（按 Apache-2.0 的定义），均按上述双许可发布，
不附加其他条款。贡献方式与 sign-off 要求见 [CONTRIBUTING.md](CONTRIBUTING.md)。

<sub>Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.</sub>
