# rsWordParser 全面测试报告

运行 ID：`20260909T052808Z`。本报告只描述本次实际执行和留档的证据，不把计划中的规模或未实现的投影当作通过。

## 受测版本与环境

- 分支：`docs/codex-test-handoff`。
- 测试起点：`72924cc`（C00-C02 已归档）；本次后续提交在同一分支完成，未 push。
- 基线：`origin/main` 的 `11759c1` 已合并到本地 `e35a79a`。
- 环境与工具链：见 `environment.txt`、`source-manifest.txt`、`hashes.txt`。
- Microsoft Word：16.112.3；macOS 26.6.2；Rust/Cargo 1.88.0。

## 最终门禁

最终执行记录：`logs/final-gates-after-crosscutting-tests.log`；最后一次 driver 文档修改后的确认记录为 `logs/final-confirmation-clippy.log` 和 `logs/final-confirmation-tests.log`。

- `cargo fmt --all --check`：PASS（最终确认复跑）。
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS。
- `cargo test --workspace --locked`：PASS，83 个测试二进制、981 passed、0 failed、13 ignored。
- 兼容层最终门禁：`logs/final-compat-gates.log`，strict clippy with `--features compat-ts` PASS；`cargo test --workspace --locked --features compat-ts` 为 84 个测试二进制、1100 passed、0 failed、13 ignored；`RUSTFLAGS=-D warnings cargo check -p rsword --lib` PASS。
- 更早的 `logs/tests-final.log` 还记录了默认、`--features compat-ts` 和 release `compat-ts` 三次运行；`logs/global-final-gates.log` 记录了仓库 CI 等价脚本通过。

## 独立自动化判据

### 参考模型、穷举和结构变体

- `crates/rsword/tests/reference_model.rs`：测试侧独立维护 UTF-16 字符、run 格式和段落格式模型，不调用生产 locate/edit/resolve 计算预期。
- 穷举：1,554 个 1-4 步序列，5,910 次操作；见 `logs/reference-model-1000x100.log`。
- 随机：先完成 1,000 seeds x 100 steps = 100,000 次操作和 9,828 次 save/reopen。
- 扩展随机：10,000 seeds x 100 steps = 1,000,000 次操作和 97,480 次 save/reopen；另有 1,000 seeds x 1,000 steps = 1,000,000 次操作和 105,394 次 save/reopen；两者均 PASS，见 `logs/reference-model-extended-final.log`。
- `crates/rsword/tests/structure_variants.rs`：覆盖 namespace prefix/default namespace、属性顺序、等价 on/off 拼写、run 拆分、Strict/Transitional/Mixed。
- `crates/rsword/tests/metamorphic.rs`：覆盖插入后删除、属性幂等、保存重开等价、独立段落交换、无关闭包内容保留。

### 字节保真和独立 OOXML oracle

- 既有语料 no-edit 保存字节一致、单节点编辑重开检查均通过；见 `crates/rsword/tests/save.rs`。
- `tools/independent-ooxml-audit.py` 只用 Python 标准库解析 ZIP/XML，不调用 rsword；自测 4/4 通过，见 `logs/independent-audit-self-test-final.log`。
- 对 `CASE-COMBINED-01-C05` 的 29/29 XML/relationship parts 检查为 0 failures；no-edit 整包字节一致，允许变化的 `word/document.xml` 之外 ZIP local record 保持一致；见 `word-authored/CASE-COMBINED-01/checkpoints/independent-audit/`。

### 变异测试

- 初始 `edit/plan.rs` campaign：34 个 mutant，19 caught、13 missed、2 unviable；见 `logs/mutants-plan-final.log`。
- 针对 13 个 survivor 行补充边界测试后，25 个目标 mutant 中 23 caught、1 unviable、1 missed；见 `logs/mutants-plan-survivors-final.log`。
- 对唯一漏网 `plan.rs:141:24 delete !` 增加“已标记删除但仍有 parent 的 Replace old”回归测试后，单 mutant 复跑 1/1 caught；见 `logs/mutants-plan-141-final.log`。
- 范围限制：没有对全 workspace 运行 cargo-mutants；该缺口保留在 `capability-matrix.csv` 的 `SKIPPED_WITH_REASON` 行。

### Fuzz 和资源行为

六个 fuzz target 各运行 601 秒，均无崩溃或不变式失败：

| target | executions | evidence |
| --- | ---: | --- |
| `fuzz_xml` | 7,807,035 | `logs/fuzz-long-fuzz_xml.log` |
| `fuzz_zip` | 4,918,426 | `logs/fuzz-long-fuzz_zip.log` |
| `fuzz_instr` | 5,224,001 | `logs/fuzz-long-fuzz_instr.log` |
| `fuzz_bind` | 17,796 | `logs/fuzz-long-fuzz_bind.log` |
| `fuzz_edit` | 136,175 | `logs/fuzz-long-fuzz_edit.log` |
| `fuzz_embedded` | 7,854,670 | `logs/fuzz-long-fuzz_embedded.log` |

所有运行使用 `-timeout=20 -rss_limit_mb=4096`；执行次数不是覆盖率证明，未发现崩溃是本次资源窗口内的事实。

## Word UI 真实语料

六个 Word-authored 文档均通过 Word UI 创建，未用脚本生成 DOCX 结构：

- `CASE-REPORT-01`：标题体系、TOC、脚注、分节、页眉页脚和横向附录。
- `CASE-BID-01`：多级列表、宽表、水平合并、单元格图片、横向节和修订/批注。
- `CASE-CONTRACT-01`：书签、外部超链接、`REF` 交叉引用、直接格式、修订和批注。
- `CASE-LAYOUT-01`：行内/浮动图片、环绕、题注、文本框和公式。
- `CASE-UNICODE-01`：中英混排、emoji、组合字符、双向文字、Tab、换行、NBSP 和直接格式。
- `CASE-COMBINED-01`：上述结构的组合，C00-C05 全部阶段；最终文件为 59,924 bytes，SHA-256 `459a4016749e178503cfd9d7a6e976ec90c9c9cb98802cd3775669eace406a84`。

每个主题的 Word UI 操作、截图、独立 XML/zip 检查、Rust driver 结果和最终 SHA-256 见各自的 `read-report.md`、`manifest.md` 和 `checkpoints/`。`CASE-COMBINED-01-C02.docx` 的 SHA-256 与交接值一致：`c0a0a88ec8013f1c15ecd3f2118efc0f947b0b308c1f9817e6e9014df24ac207`。

Word 结果分类：

- 包级保真、稳定锚点读取、无编辑保存字节一致：PASS。
- 表格内部、批注、文本框、字段和页眉页脚的完整语义投影：`NOT_IMPLEMENTED`，由独立 XML 作为结构 oracle。
- Word TOC 更新后 `w:hyperlink` 数量变化是 Word 实际重写行为，已保留原始计数差异，不伪称等价。
- 少数合成 fixture 在 Word 中触发修复/错误提示；对应原始 fixture 也失败，归为源语料兼容性发现，不归为本次编辑回归。

## 已知缺口和解释边界

- 当前模型对表格内部、批注正文、文本框和部分 header/footer/field 内容没有完整语义投影；保留 XML 和包字节不等于读取语义通过。
- mutation campaign 只覆盖 `edit/plan.rs`，不是全 workspace mutation coverage。
- fuzz 运行是 601 秒窗口，不是对所有输入空间的证明。
- 未执行 10,000 seeds x 1,000 steps 的单批次；已执行 10,000 x 100 和 1,000 x 1,000 两个扩展方向，具体数字见 `logs/reference-model-extended-final.log`。
- 本报告不声称“所有文档绝无错误”。结论范围限于本运行保存的版本、语料、工具链和 Word build。

## 产物入口

- 功能矩阵：`capability-matrix.csv`。
- 最终门禁：`logs/final-gates-after-crosscutting-tests.log`。
- 参考模型：`logs/reference-model-1000x100.log`、`logs/reference-model-extended-final.log`。
- Fuzz：`logs/fuzz-long-fuzz_*.log`。
- 变异测试：`logs/mutants-plan-final.log`、`logs/mutants-plan-survivors-final.log`、`logs/mutants-plan-141-final.log`。
- Word 语料：`word-authored/*/read-report.md`。
- 独立审计：`tools/independent-ooxml-audit.py`、`word-authored/CASE-COMBINED-01/checkpoints/independent-audit/`。
