# rsWordParser

**独立的高保真 DOCX 读写内核**（Rust）。解析 `.docx` 为文档模型，接受 `EditOp` 编辑操作，并以**字节级局部补丁**写回。不做布局与渲染。

交付 **Rust crate 优先**，wasm / CLI 是它的绑定。目标形态是 Word / WPS 的**外挂应用**（文件级工具：CLI + MCP server），对 docx 阅读与修改、后续接入 Agent 读改内容——专注 parser，不碰渲染。

> **范围改定（2026-09-08）**：原目标「替换 genoffice `packages/docx-engine` 的 `parseDocx` / `saveDocx`」已撤销。genoffice 现在只是**测试基准**（只读地跑它的 TS 引擎生成期望值），不再是使用者。见 `docs/03` v3.3 与 `docs/04` §17。

核心原则：**文件是真相。** 未编辑的内容一个字节都不动；编辑只发生在被标脏的 XML 节点上。

## 目录

| 目录 | 内容 |
| --- | --- |
| `CLAUDE.md` | 在这个仓库里干活的规则：权威顺序、不变式、命令、硬规则、踩过的坑（人与 AI 同用） |
| `docs/` | 设计文档。`01` TS 实现的差分基准规格；`02` v2（已被取代）；`03` **v3.3 冻结架构**（宪法）；`04` 开发计划（环境核查、执行顺序、实现偏差、待决事项、逐里程碑进度）；`05` **现状快照**（能力矩阵、实测数字、明确未实现） |
| `spec/` | 可验收的模块规范，每条规范带 ID（`XML-12`、`FLD-06`…），实现与测试引用这些 ID |
| `crates/rsword/` | 内核 crate。模块目录与 `docs/03` §2 的分层一一对应；`src/lib.rs` 有模块 ↔ 规范映射表 |
| `corpus/` | 测试语料：`synthetic/` 799 份（由 genoffice 测试导出的 docx + 期望 JSON + `SaveBlock[]` 记录）、`real/` 266 份（真实文档，每个带 `case.toml`）、`hostile/` 38 份（TEST-09 恶意输入） |
| `fixtures/resolve/` | `resolve/` 的 Word 实测校准 fixture |
| `tools/export-golden/` | 语料导出脚本（TS）。运行在 genoffice 仓库上，不修改它 |

## 阅读顺序

0. `CLAUDE.md`（规则）与 `docs/05-status.md`（现在能做什么）
1. `docs/03-architecture-v3.md` 第 0 节（冻结项）与第 13 节（六个核心类型索引）
2. `spec/00-overview.md`（规范体系、术语、单位）
3. `docs/04-dev-plan.md`（当前在做什么、下一步做什么）
4. 按里程碑阅读对应 spec：M0 → `01-package`、`02-xml-dom`；M1 → `05-properties`、`06-model`、`10-compat-ts`；M2 → `03-span`、`04-field`、`13-m2-plan`；M3 → `06-model` 的表格部分；M7 → `08-edit`、`09-save`；**M8′ → `19-m8-plan`**（原生协议与独立交付）；**M9′ → `20-m9-plan`**（Agent 接口层与文件级工具）

## 构建

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh   # 重新导出语料（需要 genoffice 已 npm install）
```

## 状态

架构 **v3.3** 已冻结（2026-09-03 首版，2026-09-08 改定范围；分层、六个核心类型、三条不变式自 v3.1 起未动）。
**M0–M7 全部完成并并入 `main`**（`main` = 32234ce，2026-09-08）：字节保真的读写骨架、属性表、
文本段落模型、L2 范围层与字段子系统、表格与 `SdtInfo`、绘图与嵌入对象的显示模型、页眉页脚 / 节 /
声明 part、`resolve` 有效属性视图（Word 实测校准）、编辑引擎全集（60 个 `EditOp`）、修订生成与
接受 / 拒绝、块字段生成器、空白模板、保存前校验与包写回、`compat_ts` 差分投影、wasm 绑定。

实测：**646 测试**（debug 与 release 双跑）、九道 `diff-parse` 门 + 两条 `--via js` **0 处未知差异**、
208 份保存用例 204 等价、1,000 条随机编辑序列（`TEST-07`）无失败、四个 fuzz 目标。

能力矩阵、实测数字、明确未实现的清单在 **`docs/05-status.md`**；任务清单与偏差记录在
`docs/04-dev-plan.md`（§11–§16 是 M2–M7 的逐条进度，§17 是范围改定与 M8′）。
**下一个里程碑是 M8′**（原生协议与独立交付），分支 `m8-native`，任务分解在 `spec/19-m8-plan.md`。

验收政策：TS 是**测试基准**而非权威，目标是**功能等价或更强**，有意差异逐条登记（`docs/04` §8）。

## 与 genoffice 的关系

- 第一阶段通过 `compat_ts` 适配器输出与今天 `ParsedDoc` 兼容的 JSON，编辑器零改动接入，并与 TS 解析器做差分测试。
- 语料导出脚本在本仓库 `tools/export-golden/`，运行时临时复制到 genoffice 并在结束后清理；产物提交到本仓库 `corpus/synthetic/`，`manifest.jsonl` 首行记录 genoffice 提交号。
