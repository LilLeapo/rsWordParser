# 贡献指南

## 许可与 sign-off

本项目按 **MIT OR Apache-2.0** 双许可发布。提交贡献即表示你同意以同样的双许可发布它。

用 **DCO**（Developer Certificate of Origin）而不是 CLA。每个提交都要带 sign-off：

```sh
git commit -s -m "……"
```

`-s` 会在提交信息末尾追加一行：

```
Signed-off-by: 你的名字 <你的邮箱>
```

这一行的含义是 [DCO 1.1](https://developercertificate.org/) 的全文：你确认自己有权提交这份代码，
并同意它按本项目的许可发布。**它不转让版权**——版权仍归你，只是授予本项目及其下游按双许可使用的权利。

忘了加可以补：`git commit --amend -s`（最后一个提交）或 `git rebase --signoff <base>`（一串提交）。

## 提交约定

- **一个任务一个提交**，标题格式 `m<里程碑>.<任务>: 英文摘要 (SPEC-ID…)`；不属于里程碑任务的用
  `fix:` / `docs:` / `refactor:` 这类前缀。正文说清做了什么与为什么。
- **注释与文档用中文，标识符与提交信息用英文。** 模块头注明对应的 spec 文件与条目。
- 提交前同步文档：`docs/04` §5.1 勾选清单、§8 偏差表（若有偏差）、`docs/05-status.md` 的数字。

## 提交前必须跑的门

```sh
cargo fmt --all
cargo clippy --workspace --all-targets                        # 必须零告警
cargo clippy --workspace --all-targets --features compat-ts   # 两条腿都要
cargo test --workspace
cargo test --workspace --release                              # 调试与发布腿自检行为不同
cargo test --workspace --features compat-ts
```

改动 `model` / `xml` 的公共 API 后**还要手动跑 fuzz**——`fuzz/` 被 `Cargo.toml` 排除在 workspace 外，
上面任何命令都编译不到它：

```sh
cd fuzz && cargo check --all-targets
```

碰到解析或保存逻辑时加上差分门：

```sh
cargo run -p diff-parse --features compat-ts -- --scope all
cargo run -p diff-parse --features compat-ts -- --corpus corpus/real
```

串联多条检查时**逐条捕获退出码**，`grep | head` 这类管道会吞掉失败。

## 几条不可协商的规则

改代码前请读 [CLAUDE.md](CLAUDE.md)，那里是完整清单。最容易触雷的几条：

1. **四条不变式不能破**（见 [README](README.md#四条不变式)）。调试构建里有自检，绕过它们等于让错误实现"看起来能跑"。
2. **属性容器只能通过生成的 `plan_apply_*` 改。** 手写 `append_child` 会违反 `PROP-05` 子元素顺序。
3. **树遍历写成迭代的。** 语料里有几千层嵌套的文档，递归会栈溢出，表现为测试 SIGABRT。
4. **禁止手改 `corpus/**/*.expected.json` 与 `*.save.*.json`** 来让测试通过。它们是基准输出的记录，
   只能由 `tools/export-golden/run.sh` 重新生成。
5. 新的元素名 / 属性名先加进 `crates/rsword/schema/local_names.txt`。

## 有意的行为差异要登记

与基准不同的地方，差分测试会当成回归。每一处有意的不同都必须登记，否则门会红：

- 解析侧 → `crates/rsword/src/bind/compat_ts/KNOWN_DIFFS.md` 的 ```known-diffs 块
- 保存侧 → `crates/rsword/tests/save_blocks.rs` 的 `INTENTIONAL` 表
- 语义层 → `docs/04-dev-plan.md` §8 的偏差表

## 报缺陷

开 issue 请带**最小复现**：一份能触发的 docx（或造它的脚本）、跑的命令、期望与实际。
如果是保真问题，附上 zip 条目的 CRC 对照或 `rsword check` 的输出最有帮助。

涉及格式漂移（文字对了但版式变了）的问题，[docs/20](docs/20-fill-playbook.md) §4 那套
「清空文字后 diff XML 骨架」的方法能一次把差异全捞出来，附上结果可以省很多来回。
