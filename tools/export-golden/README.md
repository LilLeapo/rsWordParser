# tools/export-golden

`spec/11-testing.md` TEST-02 的语料导出脚本。运行在 genoffice 仓库的 `packages/docx-engine` 上，
把每个 vitest 用例合成的 docx 与 TS `parseDocx` 的规范化输出落盘到本仓库 `corpus/synthetic/`，
把 TEST-09 清单的恶意输入生成到 `corpus/hostile/`。

## 原理

不改 genoffice 的任何文件。`run.sh` 把本目录的 `*.ts` 复制到
`<docx-engine>/export-golden.tmp/`，用 genoffice 自己的 vitest 跑一遍全部测试，配置里两条 alias
把测试的两个导入重定向到录制包装：

| 测试里的导入 | 重定向到 | 录制什么 |
| --- | --- | --- |
| `./helpers/build-docx` | `build-docx.wrapper.ts` | `buildDocx` / `buildKitchenSinkDocx` / `buildChartDocx` 产出的字节 → `<测试文件>__<序号>.docx` + `.expected.json`（`parseDocx` 输出规范化：Map→对象、Uint8Array→省略、undefined→删除、键排序）；解析抛错则写 `.error.json` |
| `../src/index` | `src-index.wrapper.ts` | `saveDocx(parsed, blocks, options)` → 按 `parsed.internal.originalBytes` 的哈希找到源 docx，写 `<stem>.save.<k>.json`（`SaveBlock[]`、`SaveOptions`、输出的 `word/document.xml`） |

同一字节内容只落盘一次；重复调用在 `manifest.jsonl` 里记为 `duplicate_of`。序号按测试文件内调用顺序编号，
`fileParallelism: false` 保证稳定。`hostile.export.test.ts` 作为额外用例文件生成 `corpus/hostile/`
与 `corpus/synthetic/extra__*.docx`（Strict 等 TS 测试没有经 `buildDocx` 构造的文档）。

## 运行

```sh
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh          # 全部
tools/export-golden/run.sh tests/text-patch.test.ts                 # 只跑部分测试文件
```

前提：genoffice 已 `npm install`（根 `node_modules/.bin/vitest` 存在）；Node ≥ 22（`node:zlib` 的 `crc32`）。

## 局限

- 只捕获经 `./helpers/build-docx` 与 `../src/index` 导入的调用；直接用 JSZip 拼包或从 `../src/parse` 导入 `saveDocx` 的用例不在其中（`manifest.jsonl` 里看不到就是没捕获）。
- 测试中对字节做过后处理（限额炸弹、0x7075 注入）的文档，包装只看到处理**前**的字节；这些用例由 `hostile.export.test.ts` 单独重建。
- 助手模块新增导出时，`build-docx.wrapper.ts` 的显式转发列表要同步。
