# rsWordParser

高保真 DOCX 编辑内核（Rust）。目标是替换 genoffice `packages/docx-engine` 的 `parseDocx` 与 `saveDocx`：解析 Word 文档为编辑器可消费的模型，接受编辑操作，并以**字节级局部补丁**写回。不做布局与渲染。

核心原则：**文件是真相。** 未编辑的内容一个字节都不动；编辑只发生在被标脏的 XML 节点上。

## 目录

| 目录 | 内容 |
| --- | --- |
| `CLAUDE.md` | 在这个仓库里干活的规则：权威顺序、不变式、命令、硬规则、踩过的坑（人与 AI 同用） |
| `docs/` | 设计文档。`01` 现有 TS 实现的参考规格；`02` v2（已被取代）；`03` **v3.2 冻结架构**（宪法）；`04` 开发计划（环境核查、执行顺序、实现偏差、待决事项、M2 及以后）；`05` **现状快照**（能力矩阵、实测数字、明确未实现） |
| `spec/` | 可验收的模块规范，每条规范带 ID（`XML-12`、`FLD-06`…），实现与测试引用这些 ID |
| `crates/rsword/` | 内核 crate。模块目录与 `docs/03` §2 的分层一一对应；`src/lib.rs` 有模块 ↔ 规范映射表 |
| `corpus/` | 测试语料：`synthetic/`（由 genoffice 测试导出的 docx + 期望 JSON + `SaveBlock[]` 记录）、`real/`（真实文档，每个带 `case.toml`）、`hostile/`（TEST-09 恶意输入） |
| `fixtures/resolve/` | `resolve/` 的 Word 实测校准 fixture |
| `tools/export-golden/` | 语料导出脚本（TS）。运行在 genoffice 仓库上，不修改它 |

## 阅读顺序

0. `CLAUDE.md`（规则）与 `docs/05-status.md`（现在能做什么）
1. `docs/03-architecture-v3.md` 第 0 节（冻结项）与第 13 节（六个核心类型索引）
2. `spec/00-overview.md`（规范体系、术语、单位）
3. `docs/04-dev-plan.md`（当前在做什么、下一步做什么）
4. 按里程碑阅读对应 spec：M0 → `01-package`、`02-xml-dom`；M1 → `05-properties`、`06-model`、`10-compat-ts`；M2 → `03-span`、`04-field`、`13-m2-plan`；M7 → `08-edit`、`09-save`

## 构建

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh   # 重新导出语料（需要 genoffice 已 npm install）
```

## 状态

架构 v3.2 已冻结（2026-09-03）。**M0 与 M1 均已完成**（2026-09-04）：字节保真的读写骨架、属性表、文本段落
模型、`resolve` 首版、`compat_ts` 文本块、编辑引擎（`EditSession` + 五个内联操作 + 事务）、`SaveBlock[]`
兼容映射、`SAVE-01` 保存编排与保存选项、差分工具链。M1 门三条均有测试覆盖。

能力矩阵、实测数字、明确未实现的清单在 **`docs/05-status.md`**；任务清单与偏差记录在
`docs/04-dev-plan.md`；下一步是 M2（Span + 字段），任务分解在 `spec/13-m2-plan.md`。

验收政策：TS 是参考实现而非权威，目标是**功能等价或更强**，有意差异逐条登记（`docs/04` §8）。

## 与 genoffice 的关系

- 第一阶段通过 `compat_ts` 适配器输出与今天 `ParsedDoc` 兼容的 JSON，编辑器零改动接入，并与 TS 解析器做差分测试。
- 语料导出脚本在本仓库 `tools/export-golden/`，运行时临时复制到 genoffice 并在结束后清理；产物提交到本仓库 `corpus/synthetic/`，`manifest.jsonl` 首行记录 genoffice 提交号。
