# SPEC 11 · 测试基础设施

对应 `docs/03` 第 11 节。职责：定义语料布局、差分与断言工具、模糊与随机编辑测试、`resolve` 校准 fixture，以及各里程碑的 CI 门。

## TEST-01 语料布局

```
corpus/
  synthetic/<name>.docx            # 由 genoffice 测试用 build-docx 合成，落盘
  synthetic/<name>.expected.json   # TS parseDocx 输出（规范化后）
  synthetic/<name>.save.json       # 可选：该测试用的 SaveBlock[] 与 TS saveDocx 输出的 document.xml
  real/<source>/<case>/doc.docx
  real/<source>/<case>/case.toml   # source, license, word_version, assertions = [...]
  hostile/<name>.docx | .zip       # 恶意与畸形输入
fixtures/resolve/<area>/<case>/    # RES-12
```

- `real/` 的文件**必须**在 `case.toml` 里写明来源与许可；LibreOffice `sw/qa/extras/*/data` 的文件需确认其许可允许再复制。
- 每个 `real` 用例至少一个断言与一次往返（`TEST-04`）。

## TEST-02 语料导出（运行在 genoffice 仓库）

> 自 `docs/03` v3.3（2026-09-08）起，这是**唯一**用到 genoffice 的地方，而且是**只读**的：跑它的 TS 引擎产出期望值，
> 不改它的任何代码。实现在本仓库的 `tools/export-golden/`（`GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh`）。
> 每个里程碑至少跑通一次，别让它烂掉（`spec/19` 风险 9）。

`tools/export-golden.ts`（待写于 genoffice）：

1. 对 `packages/docx-engine/tests/*.test.ts` 中每次 `buildDocx(...)`/`buildKitchenSinkDocx()` 调用，用 vitest 的自定义 reporter 或包装 helper 拦截生成的字节，以 `<测试文件>__<用例序号>.docx` 落盘。
2. 对每个 docx 运行 `parseDocx`，输出经规范化的 JSON（`Map` → 对象，`Uint8Array` → 省略，`undefined` 删除，键排序）。
3. 若用例调用了 `saveDocx`，同时落盘 `SaveBlock[]` 与输出 `document.xml`。
4. 产物提交到本仓库 `corpus/synthetic/`，同时记录 genoffice 提交号。

## TEST-03 差分工具

`tools/diff-parse`：对每个 `synthetic/*.docx` 运行 Rust `compat_ts` → JSON，与 `expected.json` 按 `COMPAT-09` 规则 diff；输出按 JSON path 聚合的差异计数与首个样例。`KNOWN_DIFFS.md` 中的路径模式（glob）跳过并单独计数；CI 断言"非已知差异为 0"。

`--scope`（各里程碑门的取样范围，按 `expected.json` 的内容判定）：`text` = 纯文本段落用例；`fields` = `text` ∪
字段 / 范围标记 / 批注 / 注释；`tables` = `fields` ∪ 含 `type: table` 块的文档，剔除单元格内含 `anchoredBoxes` 或
run `image / math / ruby` 的文档（M4 域）；`all` = 全部。每个 scope 是前一个的超集。

## TEST-04 保真测试

- **往返**：对全部语料 `open → save` → 字节相同（不变式 1）。
- **单节点编辑**：对每个语料，随机选一个 `Text` 段落做一次 `InsertText`，保存后：其他 zip 条目 CRC 与压缩字节相同；主 part 中所有 `Clean` 节点的 `lex` 字节都是输出子串（`SAVE-08`）；重解析后该段文本正确、其他段落模型相等。
- **Strict**：Strict 语料编辑后根命名空间仍为 Strict 族，`ST_OnOff` 写法为 `true/false`。

## TEST-05 XPath 断言

`tools/xpath-assert`：对保存输出的任一 part 求 XPath（命名空间前缀表固定），支持 `count()`、属性值、文本；用于 `EDIT-03`/`SAVE-*` 用例与 `COMPAT-08` 的等价比较（比较两份 `document.xml` 的一组 XPath 结果，而不是字节）。

## TEST-06 模糊测试

`cargo fuzz` 目标：

| 目标 | 输入 | 不变式 |
| --- | --- | --- |
| `fuzz_zip` | 任意字节 | 不 panic；要么 `Err`，要么成功解析 |
| `fuzz_xml` | 任意字节作为 part | 不 panic；成功时 `serialize == input`（Clean） |
| `fuzz_instr` | 任意字符串 | 指令 tokenizer 不 panic |
| `fuzz_edit` | 语料文档 + 随机 `EditOp` 字节流 | 不 panic；`Err` 时状态不变（`EDIT-05`）；保存输出良构 |

## TEST-07 随机编辑序列（property-based）

生成器：从语料文档出发，随机产生 `EditOp` 序列（`InsertText`、`DeleteRange`、`SetRunProps`、`SplitParagraph`、`MergeWithNext`、`InsertField`、`DeleteBlock`、`SetCellProps`、`AddBookmark`、`AddComment`、`Accept/RejectRevision`），`track_changes` 随机开关。每步之后断言：

1. `document.refresh` 结果 == `Document::rebuild()`（忽略 `RevisionId` 与缓存字段）。
2. 所有 `RangeSpan`/`FieldSpan` 通过 `SPAN-09`/`FLD-13` 校验且无 `EngineInvariantViolation`。
3. 每 N 步保存一次：输出良构；重解析 → 模型与保存前投影相等（忽略 `NodeId`）；再继续编辑（多轮）。

失败用例最小化后固化为回归测试。

## TEST-08 resolve 校准

`fixtures/resolve/<area>/<case>/{doc.docx, expected.toml, README.md}`（`RES-12`）。测试读取 `expected.toml` 中的 `[[run]]`/`[[para]]`/`[[cell]]`/`[[section]]` 条目，与 `resolve` 输出比较。`RES-04` toggle 至少 5 个；每条 `RES-*` 至少 1 个。fixture 与规则冲突时，以 Word 为准修改规则并在 README 记录。

## TEST-09 恶意与畸形输入清单

| 名称 | 内容 | 期望 |
| --- | --- | --- |
| `zip-part-too-large` | central directory 声明 600 MiB | `PKG_PART_TOO_LARGE` |
| `zip-total-too-large` | 多 part 合计超 1.5 GiB | `PKG_TOTAL_TOO_LARGE` |
| `zip-too-many-parts` | 10,010 个条目 | `PKG_TOO_MANY_PARTS` |
| `zip-unicode-path-shadow` | 0x7075 字段指向别的名字 | 正文正确 |
| `xml-deep-smarttag` | 3000 层 `w:smartTag` | 成功；段落可编辑 |
| `xml-deep-table` | 5000 层嵌套表格 | 成功；深层为 `TooDeep` |
| `xml-unbalanced-main` | 主 part 标签不平衡 | `Err(XML_MALFORMED)` |
| `xml-unbalanced-header` | header part 不平衡 | 成功；header `Opaque` + 诊断 |
| `field-unclosed` | 只有 begin | 诊断；段落可编辑；保存字节相同 |
| `span-orphan-end` | 孤儿 `bookmarkEnd` | PreExisting 诊断；保存成功 |
| `rels-missing-target` | `r:embed` 指向不存在 part | 图片 `broken`；保存字节相同 |
| `rels-escape-root` | `../../x` 目标 | 诊断；当作缺失 |
| `content-types-missing` | 无 `[Content_Types].xml` | 诊断；解析继续 |
| `encoding-utf16-part` | UTF-16 编码的 styles.xml | 转码 + 诊断 |
| `mixed-flavor` | Strict 主 part + Transitional header | `Mixed`；各 part 按自身 flavor |
| `dup-ids` | 重复修订 `w:id` | 诊断；保存成功 |
| `table-grid-mismatch` | 行 gridSpan 总和 ≠ `tblGrid` 列数 | 解析成功；`SAVE_TABLE_GRID` PreExisting 诊断；列操作 `Err(EDIT_TABLE_GRID_INCONSISTENT)`；保存字节相同 |
| `table-cell-no-paragraph` | `w:tc` 内没有 `w:p` | 解析成功；诊断；保存字节相同；格内 `InsertBlock` 后格尾有 `w:p` |

## TEST-10 CI 门

| 里程碑 | 门 |
| --- | --- |
| M0 | 全部语料 `parse → serialize` 字节相同；`fuzz_zip`/`fuzz_xml` 各 10 分钟无崩溃 |
| M1 | `synthetic` 文本段落用例 diff 为 0；单节点编辑保真；Strict 编辑保持 Strict |
| M2 | 字段与 Span 用例 diff 为 0；`fuzz_instr` |
| M3 | `diff-parse --scope tables` 0 未知差异；单元格段落的单节点编辑保真（`TEST-04` 扩展）；`xml-deep-table` 通过；表格操作随机序列 200 步 × 10 份无失败（`spec/14`） |
| M4 | 绘图域**路径**的 diff 为 0（`--scope drawing`：全部文档照跑，只计绘图域路径；按文档筛关不上——绘图文档同时带着别的域的差异） |
| M5 / M6 | 对应域的 `synthetic` diff 为 0 |
| M7 | `COMPAT-08` XPath 等价全部通过；`TEST-07` 1,000 序列无失败；`fuzz_edit` |
| ~~M8~~ | ~~genoffice e2e 通过~~ —— **2026-09-08 撤销**（`docs/03` v3.3：genoffice 退为测试基准，不再切换其引擎） |
| ~~M9~~ | ~~协议一致性 + 逃生口归零 + genoffice 迁移完成 + 删除 `compat_ts` 与 TS 引擎~~ —— **2026-09-08 重划**为 M8′ / M9′ |
| M8′ | 六道门（`spec/19`「M8′ 门」）：① 协议一致性——全语料 `document()` 过 JSON Schema、serde 往返幂等、`MOD-01`–`MOD-11` 字段不丢；② 60 个 `EditOp` 变体 JSON 往返，协议 `apply` 与原生 `apply` 保存结果逐字节相同；③ 公共 API——`cargo doc` 零警告、`missing_docs` 为零、**默认 feature 不含 `compat_ts`** 且能完成 `open → document → apply → save`、三个 example 在 CI 跑；④ 回归网换代——`*.model.json` 快照进 CI、`TEST-07` 走协议、`fuzz_bind` 10 分钟无崩溃；⑤ 既有门不退（`--features compat-ts` 下九道差分门仍 0 未知差异）；⑥ `document()` JSON 体积较 `parsed_doc` 降 ≥ 50%、`apply` p95 < 5 ms、`save` < 50 ms/MB、`.wasm` gzip ≤ 3 MiB |
| M9′ | 六道门（`spec/20`「M9′ 门」）：① 9.0 的 Agent 任务集全通过，改类任务的输出 docx 过 Word 打开检查且满足不变式 2；② 文本投影双向锚点全语料往返一致、丢弃项逐类计数且不静默；③ 预算——`outline()` 在 token 上限内、任何读取可限量且续读拼接等于一次性读取；④ 改类任务的逃生口 `BIND_XML_ESCAPE` 计数为 0；⑤ CLI 每个子命令端到端测试 + 一次真实 Agent 会话经 MCP 完成三条改类任务；⑥ 既有门不退 |
