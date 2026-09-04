# rsWordParser

高保真 DOCX 编辑内核（Rust）。目标是替换 genoffice `packages/docx-engine` 的 `parseDocx` 与 `saveDocx`：解析 Word 文档为编辑器可消费的模型，接受编辑操作，并以**字节级局部补丁**写回。不做布局与渲染。

核心原则：**文件是真相。** 未编辑的内容一个字节都不动；编辑只发生在被标脏的 XML 节点上。

## 目录

| 目录 | 内容 |
| --- | --- |
| `docs/` | 设计文档。`01` 现有 TS 实现的参考规格；`02` v2（已被取代）；`03` **v3.2 冻结架构**（宪法）；`04` M0/M1 开发计划（环境核查、风险结论、执行顺序、待决事项） |
| `spec/` | 可验收的模块规范，每条规范带 ID（`XML-12`、`FLD-06`…），实现与测试引用这些 ID |
| `crates/rsword/` | 内核 crate。模块目录与 `docs/03` §2 的分层一一对应；`src/lib.rs` 有模块 ↔ 规范映射表 |
| `corpus/` | 测试语料：`synthetic/`（由 genoffice 测试导出的 docx + 期望 JSON + `SaveBlock[]` 记录）、`real/`（真实文档，每个带 `case.toml`）、`hostile/`（TEST-09 恶意输入） |
| `fixtures/resolve/` | `resolve/` 的 Word 实测校准 fixture |
| `tools/export-golden/` | 语料导出脚本（TS）。运行在 genoffice 仓库上，不修改它 |

## 阅读顺序

1. `docs/03-architecture-v3.md` 第 0 节（冻结项）与第 13 节（六个核心类型索引）
2. `spec/00-overview.md`（规范体系、术语、单位）
3. `docs/04-dev-plan.md`（当前在做什么、下一步做什么）
4. 按里程碑阅读对应 spec：M0 → `01-package`、`02-xml-dom`；M1 → `05-properties`、`06-model`、`10-compat-ts`；M2 → `03-span`、`04-field`；M7 → `08-edit`、`09-save`

## 构建

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh   # 重新导出语料（需要 genoffice 已 npm install）
```

## 状态

架构 v3.2 已冻结（2026-09-03）。**M0 已完成**（2026-09-04）：任意语料 `parse → serialize` 字节相同（含 Strict），无编辑保存字节相同，改一个节点后其他条目原样，两个 fuzz 目标各 10 分钟无崩溃。**M1 进行中**：组 P（1.1 属性表格式与 codec、1.2 `RunProps` / `ParaProps`、1.3 `plan_apply_*` 合并写回）、1.4 声明模型、组 M（1.5–1.8 坐标流 / `ParagraphFacts` / 分类 / `Document::rebuild`）、1.9 `resolve` 首版、1.10 `compat_ts` 文本块（193 份文本用例与 TS `ParsedDoc` 零差异）、1.15 `diff-parse` / `xpath-assert` 工具、1.14 第一批（保存校验、扩展命名空间声明、Strict 保存测试）、1.11 `EditSession` / 定位 / `MutationPlan` 事务、1.12 内联操作（`InsertText` / `DeleteRange` / `SetRunProps` / `SetParaProps` / `ReplaceInlines`，M1 门第二条通过）已完成（2026-09-04，`docs/04-dev-plan.md` §5.1）；1.13 `SaveBlock` 兼容映射进行中。

## 与 genoffice 的关系

- 第一阶段通过 `compat_ts` 适配器输出与今天 `ParsedDoc` 兼容的 JSON，编辑器零改动接入，并与 TS 解析器做差分测试。
- 语料导出脚本在本仓库 `tools/export-golden/`，运行时临时复制到 genoffice 并在结束后清理；产物提交到本仓库 `corpus/synthetic/`，`manifest.jsonl` 首行记录 genoffice 提交号。
