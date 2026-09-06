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
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh          # 全部（会先清空 corpus/synthetic 与 corpus/hostile 再重建）
tools/export-golden/try.sh embedded-graphics.export.test.ts        # 只跑一个 *.export.test.ts，产物进临时目录，不碰 corpus/
```

`run.sh` 带过滤参数（`run.sh tests/text-patch.test.ts`）也会先清空 corpus，只适合在一次性检查里用；开发新的
导出用例文件一律用 `try.sh`。本目录里每个 `*.export.test.ts` 都会被 `run.sh` 一起跑（`vitest.config.ts` 的
include 是 `export-golden.tmp*/*.export.test.ts`）。

前提：genoffice 已 `npm install`（根 `node_modules/.bin/vitest` 存在）；Node ≥ 22（`node:zlib` 的 `crc32`）。

## 重导的稳定性与噪音（2026-09-06 起）

- **文件顺序固定**：录制器按字节哈希去重（同一份 docx 谁先跑到谁拿 stem）、`.save.<k>` 的 k 按调用序编号，
  所以 vitest 的文件顺序一变，既有 stem 与 k 就会漂（`resource-cleanup__001` 曾整份换名成 `image-wrap__006`）。
  `vitest.config.ts` 的 `StableSequencer` 按 `file-order.ts`（首次导出时的实际录制顺序，由 main 上的 manifest 反推）
  排文件，新文件排最后。**`file-order.ts` 只许追加**：新的 `*.export.test.ts` 不必登记（自动排在最后、按字典序），
  genoffice 新增的测试文件也一样。
- **新导出文件里的源文档要字节唯一**：与 genoffice 自己测试同形的文档会被去重、抢走对方的 stem。约定在
  `bodyXml` 开头放一条 `<!--<stem>-->` 注释（`embedded-*.export.test.ts` 都这么做；TS 与 Rust 两侧的
  `bodyInnerStart` / `extras.elements` 都跳过注释，已核对无差异）。
- **每次重导必然变、但没有意义的字节**（重导后按下面的规则**还原**成 HEAD 版本再提交，保持 diff 可审）：
  `*.save.*.json` 的 `outputSha256`（`saveDocx` 写入保存时间）；`write-protection__001.save.{1,2,10}.json` 的
  `options.protection / writeProtection` 的 `hash / salt`（TS 每次随机生成盐）；`corpus/hostile/*.docx` 与
  `corpus/synthetic/extra__*.docx` 的 zip 时间戳（内容与大小不变）。还原的判据：save.json 只有上述键不同、
  docx 大小不变。
- `corpus/hostile/table-cell-no-paragraph.docx` 与 `table-grid-mismatch.docx` 不是本工具生成的（M3 手工构造），
  `run.sh` 会删掉它们：重导后 `git checkout` 回来，并把它们的两条记录并回 `manifest.json`。

## 局限

- 只捕获经 `./helpers/build-docx` 与 `../src/index` 导入的调用；直接用 JSZip 拼包或从 `../src/parse` 导入 `saveDocx` 的用例不在其中（`manifest.jsonl` 里看不到就是没捕获）。
- 测试中对字节做过后处理（限额炸弹、0x7075 注入）的文档，包装只看到处理**前**的字节；这些用例由 `hostile.export.test.ts` 单独重建。
- 助手模块新增导出时，`build-docx.wrapper.ts` 的显式转发列表要同步。
