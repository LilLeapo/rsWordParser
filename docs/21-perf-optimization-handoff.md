# rsWordParser 性能优化交接（2026-09-16）

交接日期：2026-09-16。基线 `main` = d9fc127。本文是可直接交给执行代理（Codex）或工程师的任务说明，使用中文汇报。
所有数字均为本机实测（Apple M5 10 核 / 25 GB / macOS 26.5 / rustc 1.98.0），复现命令见 §2；换机器请先重测基线再动手。

先读 `CLAUDE.md`（或 `AGENTS.md`），再读本文。本文只讲"哪里慢、为什么、怎么改、怎么验收"，不重复项目规范。

## 0. 一句话结论

"项目慢"有三层，互相独立，可以并行开工：

| 层 | 现象 | 根因 | 工作包 |
| --- | --- | --- | --- |
| 运行时 | `text` 读 326 KB 文档 590 ms（release），`outline` 只要 20 ms | `budget::longest_prefix` 对每个候选页尾重建并整包序列化约 20 次，O(n²) | WP1 |
| 测试 | `cargo test --workspace` debug 254 s，CPU 只用 106% | 7 个单函数串行扫全语料 + opt-level 0（比 release 慢约 9 倍） | WP2 |
| 构建 | 冷编 60 s；文档里的命令互相不共享产物；12 个 worktree 的 target 合计约 190 GB | `jsonschema` 默认特性拉进 reqwest/tokio/rustls/aws-lc；`-p`/`--workspace`/`--features compat-ts`/`build` vs `test` 各编一份 | WP3、WP4 |

目标（在 M5 上，release 除非注明）：

| 指标 | 现在 | 目标 |
| --- | ---: | ---: |
| `rsword text large-report.docx --json` | 590 ms | ≤ 20 ms |
| 同上，debug 构建 | 7.8 s | ≤ 0.3 s |
| `cargo test --workspace`（debug）墙钟 / CPU 利用率 | 254 s / 106% | ≤ 90 s / ≥ 400% |
| `cargo test --workspace --no-run` 冷编 | 60 s | ≤ 45 s，且依赖树无 aws-lc-sys |
| 单个 worktree 一小时开发后的 target | 14 GB | 依赖共享后显著下降（记录实际值） |

目标是估计值，不是承诺。达不到时如实报告数字，不调目标。

## 1. 授权边界与不可动的东西

- 只做性能。**不改任何输出语义**：`usage.responseBytes` / `estimatedTokens` 的定义、分页边界、游标格式、错误码、`*.model.json` 快照、`corpus/**` 一个字节都不能变。快照或语料 diff 出现即视为失败。
- 遵守 `CLAUDE.md` 四条不变式与"改代码时的硬规则"。调试构建的自检（`check_dirty_invariants`、`SAVE-08` 抽样、`PROP-05` 顺序）必须保持开启；任何 profile 改动不得关掉 `debug-assertions`。
- 一个工作包一个提交，提交信息遵循 `CLAUDE.md`（近期历史用 `perf(agent-query): …` / `test(...)` / `build(...)` 这类前缀即可）。每个提交前跑 §6 的门。
- 不自动 push / 合并；不删除任何 worktree 或 target 目录（WP4 的清理只给命令，由人执行）。
- 本文的 file:line 基于 d9fc127，接手时用函数名重新定位。

## 2. 基线与复现

```sh
# 冷编译（先 rm -rf target 或换一个新 worktree）
cargo test --workspace --no-run --timings          # 实测 60 s 墙钟；报告在 target/cargo-timings/
# 测试
time cargo test --workspace                        # 实测 254 s，262 s user → 106% CPU
time cargo test --workspace --release              # 实测 68 s，28 s user
# 单个慢测试（debug，独跑）
cargo test -p rsword --test random_ops test_07_random_edit_sequences -- --exact      # 42 s
cargo test -p rsword --test tracked_ops gate_1_oracles_over_corpus -- --exact        # 21 s
cargo test -p rsword --test tracked_ops gate_1_oracles_over_table_corpus -- --exact  # 12 s
cargo test -p rsword --test native_session bind_05_09_read_only_full_corpus -- --exact          # 17 s
cargo test -p rsword --test native_edit bind_03_protocol_native_bytes_full_synthetic_real -- --exact  # 16 s
cargo test -p rsword-mcp --test memory                                                          # 13 s（套件内）
cargo test -p rsword --test model_snapshot -- --exact test_10_full_corpus_model_snapshots_and_no_edit_save_identity  # 5 s
# 运行时
cargo build -p rsword-cli --release
time target/release/rsword text corpus/real/misc/large-report.docx --json >/dev/null           # 0.59 s
time target/release/rsword outline corpus/real/misc/large-report.docx --json >/dev/null        # 0.02 s
for l in 2000 4000 8000; do time target/release/rsword text corpus/real/misc/large-report.docx --json --limit $l --maxBytes 4194304 >/dev/null; done
#   → 0.07 s / 0.18 s / 0.62 s（超线性）
cargo build -p rsword-agent-query --release --bins && cargo bench -p rsword-agent-query --bench agent   # docs/05 §9.8 的口径
# 剖析（debug 二进制跑 7.8 s，够 sample 抓）
target/debug/rsword text corpus/real/misc/large-report.docx --json >/dev/null & sample $! 5 -file /tmp/text.sample
```

冷编译关键路径：依赖在 33 s 完成（最长单元 `aws-lc-sys` 构建脚本 16.7 s），随后 `rsword` lib（test 配置）单 crate 26.9 s。85 个可执行文件（64 个 rsword 集成测试）共 1.65 GB。

## 3. 根因细节

### 3.1 运行时：`text` 的最长前缀选择是 O(n²)

调用链：`Sessions::read_inner` → `paging::page`（[tools/agent-query/src/paging.rs:185](../tools/agent-query/src/paging.rs)）→ `budget::longest_prefix`（[tools/agent-query/src/budget.rs:58](../tools/agent-query/src/budget.rs)）。5 s 采样的 4020 个样本 100% 在 `longest_prefix` 之下。

每个候选页尾 `end` 的开销：

1. `for end in first..=last` 线性扫描。只有 `contentUtf16 > limit` 才 `break`；字节超限**不**停（docs/16 §"分页"：末页省游标、字节非单调）。large-report 正文 8306 UTF-16、默认 limit 8000，于是 862 个段落几乎全部被评估，而首屏早在 1140 字符处就已撞到 24000 B 的字节上限。
2. 每个候选调 `paging::response`：拼 `content`、克隆全部 `anchors`，`budget::envelope` 里 `serde_json::to_string(&content)` 一次、`measure` 定点循环 ≥ 2 次 `to_vec`，`response` 末尾再 `measure` ≥ 2 次。
3. `fits` → `transport::common_bytes`（[tools/agent-query/src/transport.rs:41](../tools/agent-query/src/transport.rs)）：对 Text / Structured 两种 MCP 形态各跑一遍 `Shape::result` 的定点循环，Text 形态的 `wrap` 内部还有一次 `value.to_string()`。合计每个候选约 20 次整包序列化。
4. 信封本身很大：正文只有 8.3K 字符，`anchors.segments` 却有 158 KB（每个 range 一条 `presentation/rangeMetadata`），完整信封 153 KB。所以越靠后的候选越贵。

有利于优化的事实（已核对）：
- 单位元数据（`diagnostics`、`caret`、`unrequestedFlows`…）只挂在**首个**单位上（paging.rs:124–131），后续单位不会覆盖，所以随 `end` 增长信封只增不减。
- 游标 token：会话模式定长句柄；文件模式是 `{"unit":end,"offset":…}` 的 hex，长度随 `end` 单调不减（cursor.rs:99–110）。
- 唯一的非单调点就是**末页**（`end == units.len()` 时没有 `nextCursor`，`truncated=false`），docs/16 已明说，测试 `agent_06_final_page_without_cursor_can_fit_after_rejected_prefix`（[tools/agent-query/tests/paging.rs:690](../tools/agent-query/tests/paging.rs)）锁着这个行为。

### 3.2 测试：串行全语料 + 未优化代码

- 7 个 `#[test]` 单函数各自串行遍历 1103 份语料或 100 条随机序列（§2 列表，合计约 125 s），harness 无法并行一个函数；cargo 又逐个运行 85 个测试二进制，所以整机只用 1 核。
- 全部在 opt-level 0 跑：同一套件 release 用户态 CPU 28 s，debug 262 s。
- `rsword-mcp` 的 `memory` 测试在 debug 下开关 large-report 五轮 13 s（[crates/rsword-mcp/tests/memory.rs:27](../crates/rsword-mcp/tests/memory.rs)）。
- release 套件 68 s 里 CPU 只有 46%：剩下是 `mcp.rs` 的 `sleep(1300ms)`、CLI/MCP/worker 进程反复启动和 1 ms 轮询——这是第二梯队，WP2 不强求。

### 3.3 构建：重依赖、变体重复、无共享缓存

- `jsonschema = "0.55.0"` 默认特性 `resolve-http + tls-aws-lc-rs + idna` 把 reqwest / tokio / hyper / rustls / aws-lc-sys 拉进 dev-deps：[crates/rsword/Cargo.toml:34](../crates/rsword/Cargo.toml)、[crates/rsword-cli/Cargo.toml:22](../crates/rsword-cli/Cargo.toml)、[tools/agent-query/Cargo.toml:27](../tools/agent-query/Cargo.toml)。`rsword-mcp` 已经 `default-features = false`，说明不需要这些特性。所有用法都是 `validator_for(&inline_schema)`，`$ref` 只指 `#/$defs/...`。
- 变体重复（同一机器、无源码改动、仅切换命令时的重编时间）：`cargo test --workspace` ↔ `cargo test -p rsword-cli` 22 s（重编 url/tower-http/reqwest/jsonschema/agent-query/cli）；↔ `cargo build -p diff-parse --features compat-ts` 17 s（重编 serde_json + rsword）；`cargo build -p rsword-cli` ↔ `cargo test` 4 s。一小时内新 target 已有 6 份 `librsword`、4 份 `libjsonschema`；主库 target 有 11 份 rsword rlib（1.6 GB）、217 个测试可执行文件（4 GB）。
- 12 个 worktree 各有 target，合计约 190 GB（m8j 85 GB、主库 38 GB、m7 20 GB、m4 13 GB…）；没有 `CARGO_TARGET_DIR`、sccache、mold。

## 4. 工作包

### WP1 `text`/所有读取的前缀选择与输出路径（最高优先）

目标：`text` 首屏在 large-report 上 ≤ 20 ms（release），结果字节与现在**逐字节相同**。分两个提交：WP1-1 = A + B（尺寸账本与二分，输出路径不动），WP1-2 = D（输出路径只序列化一次）。C 视剖析结果决定。

**已核对的前提**（实现与代码注释直接引用，接手时用 `cargo tree -e features -i serde_json` 复核第一条）：
- serde_json 解析出的特性是 `default, float_roundtrip, std, unbounded_depth`，**没有** `preserve_order`：`Value::Object` 是 BTreeMap，键按字典序输出，compact 格式。实测响应键序就是 `anchorCounts, anchors, content, diagnostics, empty, nextCursor, omitted, range, snapshot, truncated, usage`。
- 转义规则固定：`"`、`\` 各加 1 字节；0x20 以下控制字符中 `\b \f \n \r \t` 为 2 字节，其余为 `\u00XX` 6 字节；非 ASCII 与 0x7F 不转义。转义逐字符、无跨边界状态，所以片段可以直接拼接，只要不在片段内部切。
- 单位元数据（`diagnostics`、`caret`、`unrequestedFlows`、`addressableNotProjected`）只挂在**首个**单位上（paging.rs:124–131），后续单位不覆盖。
- 单位的 UTF-16 区间连续：一个前缀的 `contentUtf16` = `units[end-1].range.end − units[start].range.start`。现在 `envelope` 先序列化再 `encode_utf16().count()` 是纯浪费。
- 游标 token：会话模式定长句柄；文件模式是 `{"unit":end,"offset":…}` 的 hex，长度随位数变（cursor.rs:13–16、99–110），要按候选算，不能当常量。
- 唯一的非单调点是末页（无 `nextCursor`、`truncated=false`）。

**A. 尺寸账本：每个单位序列化一次，之后每个候选 O(1) 算术。**
- 单位构造时（`text_units` / `Unit::record`）用 serde_json 把本单位会进入信封的部分各序列化**一次**并保存 u8 片段：正文转义后的字节（不含两侧引号）、本单位全部 anchors 元素（compact，元素间不带逗号）、记录型单位的整条记录。**不要自己实现转义**，让 serde_json 产出片段，账本只数长度。
- 每个片段记 `JsonLen { bytes, quotes, backslashes, ctrl_extra }` 与 `anchors_count`，在 `page` 里做前缀和。
- 候选 `end` 的内层字节 = 骨架常量 + Σ 片段长度 + 逗号数 + `nextCursor` token 长度(end) + `usage` 两个数字的位数；`contentUtf16` 用区间差。骨架常量（snapshot、range、首单位元数据、`omitted`、`anchorCounts` 等）每次 `page` 调用算一次；里面若出现浮点数，让 serde_json 算该标量的长度，不自己格式化。
- `usage.responseBytes` / `estimatedTokens` 的自引用（budget.rs:42 的 `measure`）：改成对位数的算术迭代 `size = base + digits(size) + digits(ceil(size/4))`，迭代序列必须与现在的循环一致（从当前值出发、同样的停止条件），否则可能落在不同的定点。
- Text / Structured 两种 MCP 形态（transport.rs:22 `Shape::result`、:41 `common_bytes`）：Text 长度 = 外壳常量 + 内层 `bytes + quotes + backslashes + ctrl_extra + 2`；Structured = 外壳常量 + 内层 `bytes`；两者对 `usage` 的再次定点同样算术化。
- **输出路径在 A 里一个字节不动**：选定 `end` 之后仍由现在的 `response()`（paging.rs:143）生成最终 `Value` 并序列化，所以输出不可能变。加 `debug_assert_eq!(账本字节, serde_json::to_vec(&out).len())`，以及对两种 Shape 的同样断言；调试构建与测试里账本算错一个字节立刻报。`response` 里 `envelope` 与末尾各一次 `measure` 合并成一次。
- 旧的 `measure` / `common_bytes` 保留为测试 oracle（`#[cfg(test)]` 或放 tests/），属性测试覆盖全语料每份文档的每一页（复用 `agent_06_text_pages_equal_full_projection_and_counts` 的遍历）与若干随机 `Value`，`new == old`。
- 不引入 SIMD 序列化库（`sonic-rs` 之类）：收益约 2 倍，却给核心运行期加依赖（现为 5 个），且 A/B/D 之后这里已不是热点。

**B. 候选数从 O(n) 降到 O(log n)。**
- 先对单位做一次前缀和（每个单位的 `contentUtf16`），算出 `E` = 满足 `contentUtf16 ≤ limit` 的最大 `end`（现在的 `break` 条件的等价物；注意现有循环会评估第一个超限候选再 break，它只影响 `failure` 的取值）。
- 评估顺序：
  1. 先评估 `first`。不 fit → 走现在完全相同的 `too_small` 路径（`minLimit` / `minBytes` 必须取自同一个候选，`AGENT_UNIT_TOO_LARGE` 阈值不变）。
  2. 在非末页候选 `[first, min(E, last)]`（排除 `units.len()`）上按 fit 单调做 galloping + 二分，得到最大 fit 的 `end`。
  3. 若 `units.len() ≤ min(E, last)`，单独评估末页候选（无游标）；fit 则它就是答案，否则取第 2 步的结果。
- 正确性论证写进代码注释：非末页字节单调（§3.1 三条事实）。**守门测试**：把旧的线性 `longest_prefix` 留作 oracle，对全语料 × 预算网格（`max_bytes ∈ {512, 1000, 2000, 4000, 8000, 16000, 24000, 65536}` × `limit ∈ {8, 64, 512, 4000, 8000, 1048576}`）比较选中的 `end` 与完整响应字节。出现任何不一致就停下报告，不许用"多扫几个邻居"掩盖。
- `Registry::candidate`（cursor.rs:99）只对被评估的候选调用，会话模式里那次线性查找不必再改。

**C. 可选：投影缓存。** `text::project` 每次读取都重算；large-report 上 `outline` 全程 3.5 ms，说明投影不是瓶颈。做完 A/B 后重新剖析，只有投影仍占 > 30% 时才在 `Session` 里按 (native 版本, scope) 缓存 `Arc<Projection>`，写入成功即失效。

**D. 最终输出路径：只序列化一次（WP1-2，A/B 落地并重新剖析之后再做）。**
量级先说清：一页 20 KB 序列化几十微秒，整份 153 KB 不到 1 ms；A/B 之后一次调用回到投影主导（约 3 ms），最终序列化只占约 1%。所以 D 的目标是去掉多余的遍数和往返，不是换更快的序列化器。按收益排序，每项单独提交、单独可回退：
1. **去掉 parse → serialize 往返。** `read_inner` 里 `document` / `diagnostics` / `media` 路径先让原生绑定产出 JSON 字符串，再 `parse()` 回 `Value`、改几个字段、再序列化（session.rs 中的 `parse(&…native.document(…))`、`parse(&native.diagnostics(…))`）。让原生绑定直接返回 `Value` 或类型化结构。`text` 路径没有这个问题。
2. **只序列化一次并直接写出。** CLI 现在 `value.to_string()`（`Display` 走 `fmt::Write` 适配器）再 `write_all`（crates/rsword-cli/src/main.rs:69–79）；MCP 在 protocol.rs:164 `to_vec`。尺寸由账本给，不再为量尺寸多序列化；输出改 `serde_json::to_writer` 写到已有的写入器。CLI 对 EBADF 的特殊处理与 Windows 的刷新逻辑保持不变。
3. **MCP Text 形态一次转义。** `Shape::wrap` 把整个内层 JSON 当字符串再包一层，等于整份转义两遍（实测 19559 B → 22914 B），外加一轮 `usage` 定点。改成：内层序列化一次得到字节，把这段字节作为一个 `&str` 用 `to_writer` 写进外壳，一次转义完成；`usage` 用账本值。Structured 形态本来就是原样嵌入，docs/19 里建议代理优先用它。
4. **类型化借用结构代替 `Value`。** `#[derive(Serialize)] struct Envelope<'a>` 借用投影里的 `&str` / `&[Anchor]`，零克隆、零中间分配（`Value` 树的建树分配通常比序列化本身还贵）。这是账本的自然载体，也让 1–3 更容易。
5. **片段 memcpy。** 账本里已有每个单位序列化好的 u8 片段，用 `serde_json::value::RawValue`（需开 `raw_value` 特性）原样嵌入，最后那一次序列化就变成拷贝。常数级收益，且引入"输出可能变"的风险：只在 4 完成、A 的 `debug_assert` 与差分守门测试都在的前提下顺手做；做不做都写进汇报。

D 的验收：输出与 WP1-1 之后逐字节相同（差分测试不改）；`tools/ci/check-agent-transports.mjs` 通过；`crates/rsword-cli/tests/cli.rs` 里关于 stdout 写入失败与回执的用例通过；每项附前后计时。

**不在本包内（需项目负责人裁定，本包只给数字）**：
- `anchors` 体积：首屏 1140 字符配 20 KB 信封（估算 5108 token），整份 8.3K 字符配 158 KB 锚点。选项：合并连续的 `presentation/rangeMetadata` 段、offset 增量编码、`anchors=false` 按需再取。同时减少序列化、传输与代理侧 token。属于 AGENT-02。
- `usage` 自引用：让 `responseBytes` 不指向包含自己的那份字节（或不回传），不动点与"先知总长才能写"的约束一起消失，输出可以流式。属于 AGENT-06 与 docs/16。

DoD：
- §2 的三条 `text` 计时与 `cargo bench --bench agent` 的 text median/p95 写进 docs/05 §9.8 表（替换 563.5 / 652.6 ms 那一行，保留旧值作历史）；WP1-1 与 WP1-2 各给一组。
- 账本的 `debug_assert_eq!` 对内层与两种 Shape 都在；属性测试与差分守门测试进 `tools/agent-query/tests/paging.rs`（或新文件），测试名带 `agent_06_`。
- `tools/ci/check-agent-transports.mjs` 跨传输等价照常通过；`agent_06_*` 全部不改断言即通过。
- docs/16 §"分页"补一段：账本与选择算法、单调性前提、末页例外、oracle 测试；docs/19 补一句 Structured 形态的建议。

### WP2 测试套件

目标：debug `cargo test --workspace` ≤ 90 s、CPU ≥ 400%；测试语义与失败复现方式不变。

**2a. 优化级别。** 在根 `Cargo.toml` 试两种方案并都测量（套件墙钟、`touch crates/rsword/src/edit/mod.rs` 后 `cargo test --workspace --no-run` 的增量时间）：
- 方案 1：`[profile.dev] opt-level = 1`（test 继承 dev）。
- 方案 2：`[profile.dev.package."*"] opt-level = 2` 只优化依赖（zip / zlib-rs / serde_json / regex），workspace crate 保持 0。
选择套件提速 ≥ 2× 且增量重编增幅 < 50% 的那个；两个都不满足就只上方案 2 并记录数字。`debug-assertions` / `overflow-checks` 保持默认 true，并在 DoD 里用一个故意触发 `debug_assert!` 的既有测试（如 `xml_12_*`）确认自检仍生效。

**2b. 全语料单函数分片。** `tests/common/mod.rs` 加 `docx_paths_shard(kind, shard, of)`（按 `docx_paths` 排序后的下标取模，确定性），再加一个声明宏（项目偏好宏而不是复制粘贴），例如 `corpus_shards!(bind_05_09_read_only_full_corpus, 8, |paths| { … })` 展开成 8 个 `#[test]`，名字带 `_shard_<i>_of_8`。应用到：
- `random_ops::test_07_random_edit_sequences`：按序列下标 `i % N` 分片，`(文档, 种子)` 的映射保持原样，`RSWORD_RANDOM_ONLY / SEED / SEQUENCES / STEPS / TRACE / DUMP` 全部照旧生效（`.github/workflows/random.yml` 用 `RSWORD_RANDOM_SEQUENCES=1000` 跑它）。断言 `applied > sequences * 10` 按分片份额换算，或改为把 `Stats` 汇总后在最后一个分片断言——二选一并说明。
- `tracked_ops::gate_1_oracles_over_corpus`、`gate_1_oracles_over_table_corpus`
- `native_session::bind_05_09_read_only_full_corpus`、`native_edit::bind_03_protocol_native_bytes_full_synthetic_real`、`native_bind` 的三处语料循环
- `model_snapshot::test_10_*`、`agent_text::agent_01_02_corpus_determinism_and_complete_anchors`、`paging::agent_06_text_pages_equal_full_projection_and_counts`、`crates/rsword/src/bind/native/query_tests.rs` 的全语料遍历
分片数以机器核数为上限（8 即可）。测试总数会变，docs/05 的 "1013 / 0 / 13" 等计数必须同步更新并说明原因。

**2c. 其他。** `rsword-mcp` 的 `memory` 测试若 2a 后仍 > 3 s，把轮数从 5 降到 3 并保留"≥ 90% 回落"断言。`mcp.rs` 的 1.3 s sleep、CLI 进程测试不动。`cargo nextest`（按二进制并行）可作为后续选项，本包不引入新工具。

DoD：`time cargo test --workspace` 与 `--release` 两组数字写进 docs/05；`random.yml`、`ci.yml` 不改命令即通过；任何测试的失败复现命令（`--exact` 名字、环境变量）在 docs/05 或测试文件头注释里更新。

### WP3 去掉 dev-deps 里的网络栈

- 三处 `jsonschema = "0.55.0"` 改为 `{ version = "0.55.0", default-features = false }`。
- 验证：`cargo tree --workspace -e normal,dev -i aws-lc-sys` / `-i reqwest` / `-i tokio` 均报 "package not found"；`Cargo.lock` 相应收缩；两条 clippy（默认与 `--features compat-ts`）零告警；`cargo test --workspace` 通过（`native_*`、`cli.rs` 里的 `validator_for` 都是内联 schema）。
- 记录：干净 target 下 `cargo test --workspace --no-run --timings` 的新墙钟与关键路径。

### WP4 构建缓存与命令纪律（部分需人决定）

- **4a 共享依赖产物**：在用户级 `~/.cargo/config.toml` 设 `[build] target-dir = "/Users/lilleap/code/rsWordParser-target"`（或 shell 里 `CARGO_TARGET_DIR`），不要提交进仓库（绝对路径、机器相关）。效果：registry 依赖跨 worktree 复用，workspace crate 仍按路径各编一份；代价：多个会话同时构建会在 build lock 上排队。是否启用由人决定；文档里写清取舍。sccache 是备选，不在本包安装。
- **4b 命令统一**：`CLAUDE.md`、`README.md`、docs/18/19 的日常命令统一用 `cargo test --workspace`（不再建议 `cargo test -p rsword-cli` / `-p rsword-mcp` 单独跑），并在命令表旁注明哪些命令会另编一份 rsword（`--features compat-ts`、`--release`、`cargo build` vs `cargo test`），建议把它们成批跑而不是交替跑。CI 不动。
- **4c 磁盘清理（只给命令，人来执行）**：已并入 main 的里程碑 worktree（m1.15、m3、m4、m6、m7、m8、m8n、test-results）与主库、m8j 的 `target/` 合计约 190 GB，可 `rm -rf <worktree>/target` 或在各处 `cargo clean`；随后 `git worktree prune`。不要删 worktree 本身。

DoD：4b 的文档改动一个提交；4a/4c 以"建议 + 命令"的形式写进 docs/05 或 README，不代替人执行。

### WP5 备忘（不在本轮范围）

- `find` 每次 spawn 一个 worker 并 1 ms 轮询（`tools/agent-query/src/search.rs:301`）：单次 5–8 ms，测试期"2 s 初始化超时"抖动与此相关；可预热一个备用 worker。需要先量再改。
- 64 个集成测试二进制各链接一份 rsword（1.65 GB / profile）：合并成少数二进制能省链接与磁盘，但违背 `tests/<域>.rs` 约定并改变 `--test <域>` 用法，需负责人决定。
- `anchors.segments` 体积（见 WP1）。

## 5. 顺序、依赖与预期收益

1. WP3（10 分钟改动，立刻少编一套网络栈）→ 2. WP1-1 = A + B（读侧主收益）→ 3. WP2-2a 与 2b（可与 WP1 并行）→ 4. 重新剖析后做 WP1-2 = D 的 1–3 项 → 5. WP4-4b 文档 → WP1-C、D 的 4–5 项、WP2-2c 视剖析结果。

预期：`text` 590 ms → 个位数到十几毫秒；debug 套件 254 s → 60–90 s；冷编 60 s → 40 s 上下；一次日常开发循环里的重复编译显著减少。以上是估计，以实测为准。

## 6. 每个提交前必过的门

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo clippy --workspace --all-targets --features compat-ts
cargo test --workspace
cargo test --workspace --release
cargo test --workspace --features compat-ts            # WP1/WP2 至少跑一次
cargo build -p rsword-cli -p rsword-mcp && node tools/ci/check-agent-transports.mjs target/debug/rsword target/debug/rsword-mcp
cargo run -p diff-parse --features compat-ts -- --scope all --json   # 未知差异必须仍为 0
git status --short corpus fixtures                      # 必须为空
```

WP1 额外：`cargo bench -p rsword-agent-query --bench agent` 前后各一次，并把 `--via js` 等价门（docs/04 的两条）跑一遍。

## 7. 汇报模板

每个工作包交付时给：改了什么（文件、函数）；前后数字（同一台机器、同一命令、至少各跑 2 次取中位数）；跑过的门与结果原样；未做的和原因；docs/04 §18（或新增 §19 "性能"）、docs/05、docs/16、CLAUDE.md 哪些段落已更新。不要把估计值写成实测值，不要把 debug 与 release 的数字混在一起。

## 8. 接手后的第一轮动作

1. `git worktree list`，在新分支（如 `perf/read-path`）开工；记录 HEAD 与 `rustc --version`。
2. 按 §2 重测基线，写进汇报草稿。
3. 先做 WP3，跑 §6 的门，提交。
4. 做 WP1-A（账本，输出路径不动，`debug_assert` 到位），用 oracle 属性测试证明字节相等；再做 WP1-B 与差分守门测试；剖析确认 `longest_prefix` 从热点消失后提交 WP1-1。
5. WP2-2a 两方案各测一次；再做 2b 分片；更新 docs/05 计数；提交。
6. 重新剖析一次 `text` / `model` / MCP 调用；按 D 的顺序做 1–3 项，每项单独提交并附前后计时；4–5 项按剖析结果决定，做不做都写明。
7. WP4-4b 文档提交；4a/4c 以建议形式交给人。
8. 最后按 §7 汇报，把新的 bench 表与测试计时留在 docs/05。
