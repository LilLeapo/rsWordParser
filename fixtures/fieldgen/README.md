# `fixtures/fieldgen` · TS 生成器的对照件（`spec/18` 7.0④）

块字段生成器（TOC / SEQ / INDEX）与 `latexToOmml` 在 genoffice 里是 `apps/docs` 功能区调用的，
**不经 `docx-engine` 的测试**，所以语料里没有它们的输出。这里落一份固定输入 → TS 输出的对照件，
Rust 侧的移植逐字对齐它。

| 文件 | 生成器 | 谁用 |
| --- | --- | --- |
| `latex.json` | `latexToOmml` / `mathParagraphXml`（TS `src/math.ts`） | `model::omml::latex_to_omml`（7.5） |
| `blank.json` | `buildBlankDocx`（TS `src/blank.ts`） | `save::blank`（7.8a） |
| `generators.json` | `generateTocFieldXml` / `generateCaptionXml` / `generateIndexFieldXml`（TS `src/generate.ts`） | `span::field::generate`（7.8b/c） |

`blank.json`：`default`（不给 `w:eastAsia`）与 `eastAsia`（给了 `等线`）两套 part 表，
外加 `bulletNumId` / `orderedNumId`。Rust 侧逐字节比。

`generators.json`：`toc`（3 组条目 → 每行一段 XML）、`tocEmpty`、`caption`（2 组）、
`index`（2 组词表，含要去重 / trim 的）、`indexEmpty`。

`latex.json` 三段：`omml`（LaTeX 源 → OMML 片段，42 条，覆盖每一条分支）、`errors`
（解析不了的输入 → 错误文本，11 条；Rust 侧只断言"同样报错"，措辞不比）、`paragraphs`
（`mathParagraphXml` 的三种对齐）。

## 重新生成

```sh
GENOFFICE_DIR=~/code/genoffice tools/export-golden/try.sh fieldgen.export.test.ts /tmp/fg
cp /tmp/fg/fieldgen/latex.json fixtures/fieldgen/
GENOFFICE_DIR=~/code/genoffice tools/export-golden/try.sh blankgen.export.test.ts /tmp/bg
cp /tmp/bg/fieldgen/{blank,generators}.json fixtures/fieldgen/
```

生成器是 `tools/export-golden/{fieldgen,blankgen}.export.test.ts`。**改 Rust 侧不能改这份文件**——
它是 TS 的输出记录（同 `corpus/**` 的规矩，`CLAUDE.md`「TS 不是权威」那一节：
有意的差异要登记，不是改对照件）。
