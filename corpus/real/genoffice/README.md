# genoffice 自带的真实文档

从 genoffice（`wjkj` 分支 `248d9c91`，docx-engine 与上游 genspark-ai/genoffice main `c1f71f90` 相同）复制，
期望值由 `tools/export-golden/real.export.test.ts` 跑同一份 genoffice 的 TS `parseDocx` 生成（同目录的 `*.expected.json`）。

| 目录 | 来源 | 许可 |
| --- | --- | --- |
| `pagination/` | `apps/docs/tests/pagination-corpus/docx`（Docs 分页对照语料，`meta/` 是各文档的说明） | genoffice，Apache-2.0 |
| `fixtures/` | `fixtures/generated`（genoffice 生成的样例） | genoffice，Apache-2.0 |
| `e2e/` | `e2e/assets/justify-pagegap-fr.docx` | genoffice，Apache-2.0 |
| `encrypted/` | `apps/docs/tests/encrypted-fixtures` 里两份**无密码**的样例（带密码的两份两侧都拒绝，不收） | msoffcrypto-tool，MIT（`LICENSE-msoffcrypto-tool.txt`） |
