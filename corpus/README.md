# corpus

测试语料（`spec/11-testing.md` TEST-01）。

| 目录 | 内容 | 来源 |
| --- | --- | --- |
| `synthetic/` | `<测试文件>__<序号>.docx` + `.expected.json`（TS `parseDocx` 规范化输出）+ 可选 `.save.<k>.json`（`SaveBlock[]` 与 TS `saveDocx` 输出的 `document.xml`）；`manifest.jsonl` 记录每个文件对应的测试名、字节哈希与去重关系 | `tools/export-golden/`，运行在 genoffice 仓库上（TEST-02） |
| `real/<source>/<case>/` | `doc.docx` + `case.toml`（source、license、word_version、assertions） | 真实文档；复制前确认许可 |
| `hostile/` | 恶意与畸形输入（TEST-09 清单） | `tools/export-golden/hostile.ts` |

语料是二进制，提交时附带产生它的 genoffice 提交号（见 `synthetic/manifest.jsonl` 首行）。

## 自有模型快照（TEST-10，M8′ 8.6）

每份可打开的 DOCX 旁放同名 `*.model.json`：原生 `SessionTable::document()` 的完整输出，
固定 `display: false`，紧凑 JSON 加一个换行。与 TS 的 `*.expected.json` / `*.save.*.json` 并存。
位置按负责人未反对的 `spec/21` 待决 4 建议执行，**待追认**。

`*.model.json` 是我们自己的输出，**可以修改**；每次提交必须说明改变原因与影响面。
**禁止手改** TS 的 `*.expected.json` 与 `*.save.*.json`，不能通过改参考答案消除未知差异。
普通测试只读；显式生成/更新命令：

```sh
RSWORD_UPDATE_MODEL_SNAPSHOTS=1 cargo test -p rsword --test model_snapshot
```

测试精确锁定 1,103 份 DOCX、1,099 份模型及无编辑保存原字节；4 份打不开的 hostile
文档在测试的 `UNOPENABLE` 清单中双向锁死，不生成虚假的模型快照。
CI 比较 PR 基线到 HEAD（push 比较前一提交），变化超过 **20** 份即打醒目 warning 与任务摘要，
提醒人工逐份复核。20 份是批量变更提示阈值，不是允许快照漂移的阈值；任一快照不匹配都让测试失败。
