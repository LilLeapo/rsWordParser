# corpus

测试语料（`spec/11-testing.md` TEST-01）。

| 目录 | 内容 | 来源 |
| --- | --- | --- |
| `synthetic/` | `<测试文件>__<序号>.docx` + `.expected.json`（TS `parseDocx` 规范化输出）+ 可选 `.save.<k>.json`（`SaveBlock[]` 与 TS `saveDocx` 输出的 `document.xml`）；`manifest.jsonl` 记录每个文件对应的测试名、字节哈希与去重关系 | `tools/export-golden/`，运行在 genoffice 仓库上（TEST-02） |
| `real/<source>/<case>/` | `doc.docx` + `case.toml`（source、license、word_version、assertions） | 真实文档；复制前确认许可 |
| `hostile/` | 恶意与畸形输入（TEST-09 清单） | `tools/export-golden/hostile.ts` |

语料是二进制，提交时附带产生它的 genoffice 提交号（见 `synthetic/manifest.jsonl` 首行）。
