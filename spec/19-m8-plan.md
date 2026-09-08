# SPEC 19 · M8 任务分解

对应 `docs/03` 第 12 节 M8 行（「编辑器切换到 Rust 引擎（`compat_ts`）」，验收「e2e 通过」）与 `spec/11` TEST-10 的 M8 行
（「genoffice e2e 通过」）。格式同 `spec/12`–`spec/18`：每个任务给出产出、依赖的规范条目与完成定义（DoD）。顺序即建议的
实现顺序；同一编号内的子任务可并行。

M8 的内容 = 让 genoffice `apps/docs` 的三个引擎入口（`parseDocx` / `saveDocx` / `buildBlankDocx`）换成 rsword 的 wasm 绑定，
编辑器**其余代码零改动**——这正是 `compat_ts` 存在的意义（`docs/03` §1.1「第一阶段经 `compat_ts` 适配器输出与今天 TS
`ParsedDoc` 字段兼容的 JSON」、`spec/10` COMPAT-01「使编辑器零改动接入」）。按出处：

- **绑定**：`spec/18` 7.10（已拍板进 M7、用 wasm-bindgen，`docs/04` §16「待决的落地」第 1 行）。7.10 的最小面是
  `parse / save / blank / version`，DoD 最后一条「`apps/docs` 里用一行替换 `parseDocx` 能打开语料文档（不进本里程碑门，是 M8
  第一步的预演）」在 M8 变成门。`bind/mod.rs` 模块头「napi / wasm 绑定在 M8 接入编辑器时加入」。
- **编辑器接入与 e2e**：`spec/18`「不在 M7」明确推给 M8；`hashProtectionPassword` 编辑器继续调 TS（同处）。
- **发布说明**：`docs/03` §14「基线之后 TS 的变化：`ooxml-normalize.ts`（装载时归一化为 Transitional）与本方案的 Strict 策略
  相反，M8 切换时 Strict 文档的行为会改变，需在发布说明中写明」。
- **metafile 转换留 TS**：`docs/03` §3.5「EMF/WMF 转换 Rust 侧暂不实现，输出 `MediaKind::Metafile` 由 TS 侧继续转换」——
  `KNOWN_DIFFS.md` 里 `emf-image__*` / `image-emf` / `ole-*` 那几条一直靠这句口头承诺，M8 要把它落成代码。
- **零改动接入**：`docs/01` §13.4「`apps/docs` 可以先零改动接入」「`internal.originalBytes` 不进 JSON（编辑器自己持有字节）」。

M8 是 M7 之后的**串行**里程碑，工作面**主要在 genoffice 仓库**（`~/code/genoffice`，见 `docs/04` §1 环境核查与 `tools/export-golden`）。
rsWordParser 侧只有绑定扩展与对照工具，分支 `m8-editor`（从并入 M7 后的 `main` 开）；genoffice 侧建议分支 `rsword-engine`
（见「待决」1）。本计划在 2026-09-07 写成，当时 M7 只完成到 7.1（`m7-edit` = 70e8333），7.8 `blank()` 与 7.10 绑定都还没开工——
**它们是 M8 的前置**；下文凡涉及 M7 产物的地方都按 `spec/18` 的 DoD 假定，开工前按并入后的 `main` 与 genoffice 当时的 HEAD 重测基线。

## M8 · 编辑器切换：wasm 绑定包、drop-in 替换、双引擎对照、e2e 与视觉基线、切换开关与发布说明

目标：（1）`apps/docs` 的五条引擎路径——打开、新建、保存、保存后重解析、比较文档——全部走 rsword；（2）编辑器其余代码零改动：
TS 的 XML 生成器 / 补丁函数 / 节与保护与公式与图表工具函数继续用，它们的产物是 `SaveBlock kind:'xml'` 与 `SaveOptions`，
`compat_ts` 已全部覆盖（`docs/05`「保存选项：TS `SaveOptions` 已全部覆盖」）；（3）与 TS 引擎的每一处行为差异**有名有姓**——
要么已登记（`KNOWN_DIFFS.md` / `INTENTIONAL` / `docs/04` §8）且编辑器不可见，要么由绑定包吸收（metafile / TIFF 转换），要么
进发布说明；（4）genoffice 全部测试与 e2e 在 rs 引擎下通过；（5）一个开关可回退到 TS 引擎一个发布周期；TS 引擎代码**不删**（M9）。

**M8 门**（`spec/11` TEST-10 M8 行的具体化）：

1. **绑定等价**：`diff-parse --via js` 对八道 scope（`text / fields / tables / drawing / hf / embedded / all`）与 `--corpus corpus/real`
   全部 0 未知差异，且绑定输出的 JSON 与原生 `compat_ts::parsed_doc` **逐字节相同**；`tests/save_blocks.rs` 的 208 份用例经绑定
   `save` 的结果与原生逐字节相同；`blank({ eastAsiaFont })` 对编辑器支持的每种 UI 语言各一份，与 TS `buildBlankDocx` 输出 canon 相等
   （`xml::canon`）。
2. **编辑器测试**：`apps/docs` 的 151 个 vitest 文件在 `GENOFFICE_DOCX_ENGINE=rs` 下全部通过（49 个直接调引擎的无一 skip）；
   `packages/file-parse` 与 `apps/markdown` 的测试同样（8.3 决定同批切换时）。
3. **e2e**：genoffice `npm run test:e2e` 的 22 个 playwright spec 全绿（Linux CI + xvfb，与今天同一 job）；`docs-visual` 的 5 份像素
   基线**零 diff、不重录**（JSON 相同 → DOM 相同 → 像素相同）；新增 `docs-rsword-roundtrip.spec.ts`（打开 → 改字 → 保存 → 重开 →
   文字在，且保存文件里其他 zip 条目 CRC 不变）通过。这一条就是 TEST-10 M8 行。
4. **差异审计闭合**：`KNOWN_DIFFS.md` 全部条目 + `docs/05`「与 TS 有意不同」15 行 + `INTENTIONAL` 4 条逐条分类为
   「编辑器不可见 / 绑定包吸收 / 用户可见改进」三类之一，表进 `docs/10-m8-engine-switch.md`；metafile / TIFF 图片在编辑器里
   **显示为图片**而不是 `brokenImage`（8.1 的转换表生效）。
5. **性能与体积**（记录并设上限，超限要解释）：`corpus/real` 里字节数最大的 10 份文档上 wasm `parse` 中位耗时 ≤ TS `parseDocx` 的
   1.0×、`save` ≤ 1.0×；`.wasm` gzip 后 ≤ 3 MiB；数字进 `docs/05`。
6. **既有门不退**：rsWordParser 全部 Rust 门继续绿；`corpus/real` 往返与 `tests/real_edits.rs` 的 Word 验收流程不变；rsWordParser CI
   新增 `wasm32-unknown-unknown` 构建 + `--via js` 等价两步。

### 实测基线（2026-09-07）

| 量 | 值 | 来源 |
| --- | --- | --- |
| genoffice 基线 | HEAD `f105f36` + 32 个脏文件（与语料导出时相同，`manifest.jsonl` 首行） | `git -C ~/code/genoffice status --porcelain` |
| TS 引擎规模 | `packages/docx-engine/src` 33 个文件 21,309 行（`parse.ts` 5,591、`generate.ts` 3,268、`patch.ts` 1,892、`types.ts` 1,710）；87 个测试文件 | `wc -l` |
| 编辑器对引擎的**值**导入 | 54 个名字。M8 要换的 3 个：`parseDocx`（`file-actions.ts:265/380/822`、`review-actions.ts:244`）、`saveDocx`（`file-actions.ts:551`）、`buildBlankDocx`（`file-actions.ts:379`）；`BLANK_BULLET_NUM_ID / BLANK_ORDERED_NUM_ID` 随 `blank()` 一起来；其余 49 个是 XML 生成 / 补丁 / 节 / 公式 / 图表 / 保护 / 参考文献 / 列表标记工具函数，M8 **不动** | node 脚本统计 `apps/docs/src` 的 import（多行 import 也算） |
| 编辑器对引擎的**类型**导入 | 57 个类型（全在 `types.ts`） | 同上 |
| 编辑器对 compat 形态字段的消费（文件数） | `docxIndex` 28、`textboxes` 15、`dataUrl` 15、`imageDataUrl` 7、`originalXml` 7、`rawRPr` 7、`sdtShell` 5、`fieldDisplay` 5、`hfParts` 4、`extras` 3、`strayRuns` 2、`rawPPr` 1、`chartParts` 1、`internal.documentXml` 0 | `grep -l`；M8 只读它做风险面，M9 用它做迁移清单 |
| 编辑器测试 | `apps/docs/tests` 151 个文件，49 个直接调 `parseDocx / saveDocx`；vitest `jsdom` 环境，`@genoffice/docx-engine` 用 alias 指到源码 `packages/docx-engine/src/index.ts` | `ls`、`grep -l`、`apps/docs/vitest.config.ts` |
| e2e | 22 个 spec；5 个打开 docs：`docs-visual`（像素回归，5 份文档，**只在 Linux 跑**）、`docs-table-gap-flicker`、`new-file-tab`、`home`、`theme-visual`；CI job `e2e` 在 ubuntu-22.04，`npm ci → build:all → xvfb-run npm run test:e2e`，已装 Rust stable（给 sheets 的 sidecar） | `e2e/`、`.github/workflows/ci.yml` |
| 其他引擎消费者 | `packages/file-parse/src/docx.ts`（`parseDocx` → 纯文本）、`apps/markdown/src/renderer/export/docxExport.ts`（`parseDocx(buildBlankDocx())` + `saveDocx`）；`pdf2docx` 不依赖引擎 | `grep` |
| genoffice 里的 Rust 先例 | `apps/sheets/native/xlsx-engine`（crate `xlsx-sidecar`：独立进程 sidecar，`cargo build --release`，CI 有缓存与 universal 构建脚本） | `apps/sheets/package.json` |
| rsword 的 wasm 可移植性 | 依赖只有 `zip`（`deflate-flate2-zlib-rs`，纯 Rust）/ `memchr` / `thiserror` / `serde_json`；源码里没有 `std::fs` / 时间 / 随机 / 线程（`fresh_para_id` 是确定性的） | `Cargo.toml`、`grep` |
| 本机工具链 | `wasm-pack` / `wasm-bindgen` 未装、`wasm32-unknown-unknown` 目标未装；node v24.15.0（genoffice 要求 ≥ 22.12，CI 用 22） | `which`、`rustup target list --installed` |
| `compat_ts` | 18 个文件 12,684 行；`KNOWN_DIFFS.md` 三十余条；`INTENTIONAL` 4 条；`parsed_doc` 经 `serde_json::Value` 中转 | `wc`、`bind/compat_ts/mod.rs:43` |
| M7 状态 | `m7-edit` = 70e8333：7.0 部分、7.1 完成；7.8 `blank()`、7.9 门、7.10 绑定未开始 | `docs/04` §16 |

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 8.0 | **基线、环境与 genoffice 分支**：① M7 并入 `main` 后重测 rsword 数字（语料 / 门 / 测试数）写进 `docs/04` §17 开头；② genoffice 基线：从 genoffice 最新 `main` 开分支 `rsword-engine`，记录提交号；是否借机重导语料见「待决」1；③ 工具链：`rustup target add wasm32-unknown-unknown`、`cargo install wasm-bindgen-cli --locked`（版本与 `Cargo.lock` 的 `wasm-bindgen` 一致，写进 `rust-toolchain` 旁的 `TOOLS.md`）、binaryen `wasm-opt`（可选）；rsWordParser CI 加 `cargo build -p rsword-js --target wasm32-unknown-unknown --release` 一步；④ 盘点脚本 `tools/m8-audit/`（node，只读 genoffice 源码）：编辑器引擎导入面（值 / 类型）、compat 形态字段消费点、`SaveOptions` 键的使用点，输出上表那几行；⑤ 在 TS 引擎下跑一遍 `npm test`（`apps/docs`）与 `npm run test:e2e`，记录通过集合与耗时作为「不退」参照 | TEST-10 | 数字进 `docs/04` §17；CI 能编 wasm；genoffice 分支就位且 TS 引擎下的基线结果记录在 `docs/10` |
| 8.1 | **绑定扩展与绑定包**（rsWordParser `crates/rsword-js`；genoffice `packages/docx-engine-rs/`）：**Rust 侧**在 7.10 的四个入口之上补齐：`parse(bytes) -> string`（JSON，与 `compat_ts::parsed_doc` 同一函数、同一 `to_string`）、`parse_diagnostics(bytes) -> string`（`Document.warnings` 的 JSON，**单独出口**，不进 `parse` 的 JSON——差分要逐字节相同）、`save(bytes, blocks_json, options_json) -> Vec<u8>`、`blank(options_json) -> Vec<u8>`（`{ eastAsiaFont? }`，对应 TS `BlankDocxOptions`：给了才写 docDefaults 的 `w:eastAsia`；7.8 若按无参移植则在此补参数）、`version() -> string`（crate 版本 + git sha + `protocol: "compat/1"`）；错误 → JS `Error`，带 `code`（`DiagCode` / `Error` 变体名）与 `message`；四个同形导出用 `wasm_export!` 收拢。构建 `wasm-bindgen --target web` + `wasm-opt -Oz`（有则用）；`rsword-js/Cargo.toml` 开 `panic = "abort"`、`opt-level = "z"` 的 release profile。**产物提交进 genoffice** `packages/docx-engine-rs/pkg/`（`.wasm` + glue），旁边 `RSWORD_COMMIT` 记 rsWordParser 提交号；rsWordParser 侧 `tools/sync-js.sh`：构建 → 拷贝 → 写提交号；genoffice CI 加一步「按 `RSWORD_COMMIT` 检出 rsWordParser、重建、`git diff --exit-code packages/docx-engine-rs/pkg`」（与它已有的 `fixtures/generated` 漂移检查同款；CI 已装 Rust）。**TS 包装** `packages/docx-engine-rs/src/index.ts`：导出与 TS 引擎**同名同签名**的 `parseDocx(bytes): Promise<ParsedDocFull>`、`saveDocx(parsed, blocks, options): Promise<Uint8Array>`、`buildBlankDocx(opts)`、`BLANK_BULLET_NUM_ID / BLANK_ORDERED_NUM_ID`、另加 `parseDocxDiagnostics(bytes): Promise<Diagnostic[]>`。`parseDocx` = wasm `parse` → `JSON.parse` → **重建 TS 形态**：`styles` / `numbering` / `headingStyleIds`（键是数字）从对象回到 `Map`（TS 类型是 `Map`，编辑器到处 `.get()`；差分工具比的是导出时 `Map → 对象` 的规范化 JSON，所以 JSON 里是对象——`TEST-02` 第 2 步）、`internal.originalBytes = bytes`（TS 的 `saveDocx` 读它，我们的 `save` 也从这里拿原字节）、`undefined` 语义靠字段缺失天然成立（`COMPAT-09`）。`saveDocx(parsed, blocks, options)` = `save(parsed.internal.originalBytes, JSON.stringify(blocks), JSON.stringify(options))`；复核 TS `SaveOptions` 里二进制字段的编码（`partBinary` / 图片 `base64` / `inks[]` 在 TS 里已是 base64 或 dataURL 字符串；若有 `Uint8Array` 字段则包装层转 base64）。**metafile / TIFF 转换留在 TS**（`docs/03` §3.5）：包装在 `parseDocx` 之后按一张路径表 `DATA_URL_PATHS`（由 8.0 的脚本从 `types.ts` 生成：`blocks[*].imageDataUrl`、`blocks[*].runs[*].image.dataUrl`、`blocks[*].table.rows[*][*].richParas[*].runs[*].image.dataUrl`、`blocks[*].textboxes[*]…`、`headerImages / footerImages / hfParts.*.images[*].dataUrl`、`inks[*].dataUrl`、`diagramDisplay.shapes[*].picture`、`oleDisplay` 预览……）遍历，凡 MIME 为 `image/x-emf \| emf \| x-wmf \| wmf \| x-emz \| x-wmz \| tiff` 的 dataURL 调 `metafileToDataUrl`（`@genoffice/docx-engine/metafile` 已单独导出）/ `tiffToDataUrl`（`tiff.ts` 今天没从包入口导出，加一行）转 PNG dataURL；转换失败按 TS 行为：图片块置 `brokenImage`、run 去掉 `dataUrl`。**加载**：渲染进程用 vite `?url` 拿 wasm 地址 + `init(url)`；vitest / node 用 `initSync(readFileSync(wasmPath))`——包装里按 `typeof process` 分支，一份产物两处用；`init` 惰性、首个调用前 `await` | COMPAT-02, COMPAT-08, COMPAT-09, TEST-02, TEST-03 | 门 1 全部（`diff-parse --via js` 八道 scope + `corpus/real` 逐字节相同；`save_blocks --via js` 208 份相同；`blank` canon 相等）；`packages/docx-engine-rs/tests`：三处 `Map` 重建、`originalBytes`、`DATA_URL_PATHS` 覆盖全部 dataURL 路径（用 `corpus/synthetic/emf-image__*` 与 `corpus/real/image-emf` / `ole-*` 验证转换后是 PNG dataURL）、错误映射（非 docx → `Error.code`、超限、主 part 畸形各一）、`version()` 与 `RSWORD_COMMIT` 一致；genoffice CI 的重建校验绿；体积与 parse 耗时表初稿进 `docs/05` |
| 8.2 | **双引擎对照**（genoffice `packages/docx-engine/src/engine.ts`；rsWordParser `tools/js-parity/`）：`packages/docx-engine/src/index.ts` 的 `parseDocx / saveDocx / buildBlankDocx` 三个导出改为**分派**：TS 实现改名 `parseDocxTs` 等，`engine.ts` 按 `GENOFFICE_DOCX_ENGINE=ts \| rs`（缺省见 8.5）选择；类型与其余导出不变——`apps/docs` 的 import 一行不改就切换。`apps/docs/tests` 151 个文件在 `rs` 下跑，失败逐个归因为三类：绑定包 bug（回 8.1 修）/ 已登记差异被断言到（改测试并**登记**进 `docs/10`）/ 测试断言的是 TS 私有形态（同上）；不放宽断言。`packages/docx-engine/tests` 87 个文件仍只测 TS（它们是语料源，`TEST-02`）。rsWordParser 侧：`diff-parse --via js` 扩到 `--corpus corpus/real`；`tests/save_blocks.rs` 加 `--via js` 模式——建议由 node 脚本 `tools/js-parity/` 跑绑定把 208 份输出落到临时目录，Rust 测试只比字节，不给 Rust 测试加 wasm 运行时依赖 | TEST-03, TEST-10, COMPAT-08 | 门 1 与门 2；两套引擎下 `apps/docs/tests` 的通过集合相同（`ts` 全过 → `rs` 全过）；归因表进 `docs/10` |
| 8.3 | **编辑器接入与差异审计**（`apps/docs`、`packages/file-parse`、`apps/markdown`、`docs/10-m8-engine-switch.md`）：五条路径（`file-actions.ts` 打开 / 新建 / 保存 / 保存后重解析、`review-actions.ts` 比较）经 8.2 的分派层自动切换，**代码零改动**是目标；允许的改动只有两处：① 诊断透出——`parseDocxDiagnostics` 的结果在开发模式打 console、生产写日志；`PreExistingDamage` 不打扰用户，`EngineInvariantViolation` 弹错并对该文档回退 TS 解析（8.5）；② `catch` 分支按 `Error.code` 给更准确的文案（`PKG_NOT_OOXML` / `PKG_PART_TOO_LARGE` / `XML_MALFORMED`…）。其他消费者走同一分派层零成本切换：`file-parse`（纯文本提取是 `ParsedDoc` 的子集）、`apps/markdown` docxExport（`blank` + `saveDocx`）；「待决」4。**差异审计**：逐条过 `KNOWN_DIFFS.md`、`docs/05`「与 TS 有意不同」15 行、`INTENTIONAL` 4 条，三类处置——(a) 编辑器不可见（`rawRPr` 字节 vs 重序列化、`w14:paraId` / `w:rsid*` 保留、修订 `w:id` 分配、`xml:space` 一律 preserve、`commentReference` 空 run 删除…）记「无影响」并写明理由；(b) 绑定包吸收（metafile / TIFF dataURL）→ 8.1；(c) 用户可见的行为变化 → 发布说明条目：Strict 文档保存后仍是 Strict（`docs/03` §14）、`x:` 等非规范前缀原样保留、`remove_date_and_time` 新能力、保存不再删除文件里原有的孤儿 part、`replaceImage` / 墨迹锚点非法时不再静默、EMF 图片块不再 broken、VML 文本框里的图片能显示、反斜杠超链接可点、单元格里的公式 / ruby / OLE 可见、画布子形状位置正确、SmartArt 箭头颜色按 tint、TOC 结果区整体保护、`REF` 指令拆多个 `instrText` 也能认……（`KNOWN_DIFFS` 里标「功能更强」的每一条都是一条）。主进程不动：`docx-encryption.ts`（officecrypto 在字节层解密 / 加密）、`external-change.ts`、`atomic-write.ts`、`window.desktop.saveDocx` 收的仍是字节 | COMPAT-01, COMPAT-09, TEST-10 | 门 4；`apps/docs/src` 的 diff 只含诊断透出与错误文案（评审时 `git diff --stat` 钉住）；`docs/10` 审计表每条有处置与理由；`file-parse` / `markdown` 测试在 `rs` 下通过 |
| 8.4 | **e2e、往返 e2e 与视觉基线**（genoffice `e2e/`）：全套 `npm run test:e2e` 在 `rs` 下跑（Linux CI；本机 macOS 只能跑非视觉的 spec）。`docs-visual` 5 份基线**不重录**：若像素不同，先比两套引擎的 JSON（应为 0 差异），再查包装层（`Map` / metafile），基线本身不动。新增 `e2e/docs-rsword-roundtrip.spec.ts`：用 `launchShell({ openFile })` 打开 `fixtures/generated/kitchen-sink.docx` 与一份从 `corpus/real` 挑的真实 Word 文档（许可允许，拷进 genoffice `fixtures/`；「待决」6），打字 → 保存 → 关标签 → 重开 → 断言文字在；保存文件读回后用 jszip 比 CRC：除主 part（及 `docProps/core.xml`）外每个条目 CRC 与原文件相同（不变式 2 在真实应用流程里成立）。`new-file-tab` 加分支：新建（`blank()`）→ 打字 → 保存 → 重开 | TEST-04, TEST-10 | 门 3；CI `e2e` job 在 `GENOFFICE_DOCX_ENGINE=rs` 下绿；新 spec 进 `e2e/`，本机 macOS 能跑 |
| 8.5 | **切换开关、回退与发布说明**（genoffice `engine.ts`、`app-settings.json`、`docs/10` 第 2 节）：缺省引擎切到 `rs`；`ts` 保留一个发布周期。开关三层：环境变量（测试）、`app-settings.json`（用户 / 支持人员可改）、运行期自动回退——`rs` 抛 `EngineInvariantViolation` 或 wasm 初始化失败时对**该文档**回退 TS 解析，状态栏提示 + 日志，不是静默换引擎；同一份文档不会一半 rs 一半 ts。发布说明：8.3 (c) 类全部条目 + 「打开 Strict 文档后保存仍为 Strict」的显著提示，进 genoffice 的 CHANGELOG。日志：每次 parse / save 记引擎名、耗时、诊断计数（本地，不上传） | — | 三层开关各一条测试；自动回退一条测试（注入 `EngineInvariantViolation`）；发布说明就位并经项目负责人过目 |
| 8.6 | **性能、体积与收尾**（`tools/js-parity/bench.mjs`、`docs/`）：`corpus/real` 最大 10 份 + `kitchen-sink`，两套引擎各跑 parse / save 5 次取中位数；`.wasm` 体积 raw / gzip；写 `docs/05`。若 rs 慢于 TS（预期不会）先查 JSON 序列化：`compat_ts::parsed_doc` 经 `serde_json::Value` 中转，可改直接写 `String`（`json.rs` 的 `set_some!` / `set_if!` 不受影响）。文档：`docs/04` §17 逐条进度、§8 若有新偏差、`docs/05` 数字与「明确未实现」、`README` / `CLAUDE.md` 状态行与命令表（加 `--via js`）、`spec/11` TEST-10 M8 行细化为门 3 的措辞 | TEST-10 | 门 5、门 6；文档同步 |

建议顺序：8.0 → 8.1（rsWordParser 侧先把 `--via js` 逐字节相同做出来，这是后面一切的地基）→ 8.2（分派层 + 双引擎测试）→
8.3 / 8.4 并行 → 8.5 → 8.6。8.1 的 metafile 路径表与 8.3 的审计表由同一个人做最省——两者都是把 `KNOWN_DIFFS.md` 从头过一遍。

## 分层决策（实现前定死）

1. **绑定无状态**：`parse(bytes)` / `save(bytes, …)` 每次从字节开始，wasm 里不留会话；M9 才引入句柄。代价是保存时重新解析一次
   （`EditSession::open` 在最大真实文档上是毫秒到几十毫秒量级，8.6 量），换来的是编辑器与主进程之间的字节流协议一个字不用改。
2. **JSON 逐字节相同是绑定的合同**：wasm `parse` 输出 = `compat_ts::parsed_doc` 的 `serde_json::to_string`（同一函数），
   `diff-parse --via js` 直接比字节；包装层的加工（`Map`、`originalBytes`、metafile）**只发生在 JS 侧**，不进 wasm。
3. **差异只在三处登记，不新增第四处**：解析侧 `KNOWN_DIFFS.md`、保存侧 `INTENTIONAL`、语义 `docs/04` §8（`CLAUDE.md` 既有政策）。
   `docs/10` 的审计表是这三处的**分类视图**（对编辑器与用户的影响），不是新的登记处。
4. **TS 引擎不删、不改语义**：只加分派层与函数改名；`packages/docx-engine/tests` 继续测 TS——它们是语料源（`TEST-02`）。删除是 M9。
5. **wasm 产物提交进 genoffice，CI 重建校验**：`npm ci` 不需要 Rust；漂移靠 CI 的 `git diff --exit-code` 抓（genoffice 对
   `fixtures/generated` 已经这么做）。两仓库的版本关系由 `RSWORD_COMMIT` + `version()` 显式表达。
6. **编辑器零改动既是目标也是门**：`apps/docs/src` 允许的 diff 只有诊断透出与错误文案（8.3）。任何「为了让 rs 过而改编辑器逻辑」
   都说明 `compat_ts` 有 bug，回 rsword 修，不改编辑器。
7. **metafile / TIFF 转换是 TS 侧的可插拔服务**（`docs/03` §3.5 冻结）：在包装层做，不进 rsword；M9 换成媒体句柄后同一服务改为按需转换。
8. **回退按文档、有提示**：不做静默双写；`EngineInvariantViolation` 是引擎自己承认出错的唯一信号，只有它触发回退。

## 实现约定：多用声明宏（用户要求，2026-09-05；与 `spec/14` / `spec/16` / `spec/17` / `spec/18` 同一条）

M8 的 Rust 侧很小，同形重复只有一处：

- `wasm_export!`：四个导出（`parse / parse_diagnostics / save / blank`）同形——「`&[u8]` / `&str` 入 → 调 rsword → `Result` 映射为
  `JsValue` 错误 `{ code, message }`」，收成一张表，同时展开导出函数与「非 docx → `Error.code`」的单测。
- 沿用 `xpath_asserts!`、`set_some!` / `set_if!`（8.6 若把 `parsed_doc` 从 `Value` 改成直写 `String`，字段写法不变）。
- TS 侧不适用宏，但 metafile 路径表 `DATA_URL_PATHS` 用数据表驱动一个遍历器，不写散落的 `if`；表由 8.0 的脚本从 `types.ts` 生成并
  带一条「`types.ts` 里每个 `dataUrl` / `imageDataUrl` 字段都在表里」的单测。

其余约定照旧：一个任务一个提交 `m8.<n>: 英文摘要 (SPEC-ID…)`（genoffice 侧提交信息同款，前缀 `rsword:`）；提交前同步 `docs/04` §17
勾选、§8 偏差表、`docs/05` 数字。

## 从 M0–M7 带过来的债（M8 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `bind/mod.rs` 模块头「napi / wasm 绑定在 M8 接入编辑器时加入」 | `bind/mod.rs` | 8.1（改措辞：绑定在 `crates/rsword-js`，M7 7.10 建、M8 接入） |
| 7.10 DoD 最后一条「`apps/docs` 里用一行替换 `parseDocx` 能打开语料文档（不进本里程碑门）」 | `spec/18` | 8.2 变成门 2 |
| `blank()` 没有 `eastAsiaFont` 参数（7.8 按 TS `blank.ts` 逐字移植，`blank-template__001` 的期望值是无参输出） | `save/blank.rs` | 8.1 |
| `KNOWN_DIFFS.md` 里 `emf-image__*` / `image-emf` / `ole-*` / `ole-with-text`「转换在 TS 侧」只是口头承诺 | `KNOWN_DIFFS.md` | 8.1 的 `DATA_URL_PATHS` |
| `compat_ts::parsed_doc` 经 `serde_json::Value` 中转 | `bind/compat_ts/mod.rs` | 8.6（若成为耗时瓶颈） |
| `docs/03` §14「M8 切换时 Strict 文档的行为会改变，需在发布说明中写明」 | — | 8.5 |
| `tiff.ts` 没从 `@genoffice/docx-engine` 入口导出 | genoffice `packages/docx-engine/src/index.ts` | 8.1 |

## 基线与复用

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M7 7.10 `crates/rsword-js` | 四个入口、错误映射、`diff-parse --via js` | 8.1 / 8.2 |
| M7 7.8 `EditSession::blank()` | 空白模板 | 8.1 |
| `bind/compat_ts/` 全部 | `ParsedDoc` JSON 与 `SaveBlock[]` / `SaveOptions` 翻译 | 8.1（绑定只是它的出口） |
| `tools/diff-parse`、`tests/save_blocks.rs`、`xml::canon` | 差分与保存等价比较 | 8.2 |
| genoffice `e2e/helpers.ts`（`launchShell({ openFile })`、`waitForPageWithUrl`） | 往返 e2e | 8.4 |
| genoffice CI 的 `fixtures/generated` 漂移检查 | wasm 产物重建校验 | 8.1 |
| `@genoffice/docx-engine/metafile`（vendored `emf-converter`）、`tiff.ts`（UTIF） | 转换服务 | 8.1 |
| `apps/sheets/native/xlsx-engine` | 「Rust 进 genoffice」的构建 / CI 缓存先例（sidecar 形态；docx 选 wasm 是因为要在渲染进程与 vitest 里同位置调用，`spec/18` 待决 1） | 8.0 |
| `docs/05`「与 TS 有意不同」表、`KNOWN_DIFFS.md`、`INTENTIONAL` | 审计表的输入 | 8.3 |

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| M7 全部并入 `main`（尤其 7.8 `blank()`、7.9 门、7.10 绑定） | **前置**；8.0 之前 |
| genoffice 基线（`f105f36` + 32 脏文件）：8.0 决定是否重导语料 | 「待决」1 |
| Linux CI：`docs-visual` 只在 Linux 跑，本机 macOS 判不了像素门 | 门 3 在 CI 上判 |
| 真实 Word：M8 不新增 Word 人工核对——M7 的 `real_edits` 流程已覆盖引擎输出；往返 e2e 用的真实文档从 `corpus/real` 挑许可允许的一份 | 「待决」6 |
| genoffice 仓库的评审 / 合并权限与 CI 时长（`e2e` job 上限 45 分钟） | 项目负责人 |
| `docs/06` 的 toggle 桌面版复核 | 与 M8 无关（`compat_ts` 的 `runs[].bold` 发声明值，不走 `resolve`）；照旧择机 |

## 不在 M8

- **删除**任何东西：TS 引擎、`compat_ts`、`types.ts`、`KNOWN_DIFFS.md`——M9。
- **原生协议**：会话句柄、模型 JSON、`EditOp` JSON、媒体句柄、去 dataURL——M9。编辑器在 M8 里仍走 `SaveBlock[]` / `SaveOptions`，
  M7 的新能力（`\h` TOC、`AcceptAll { author }` 原生、`remove_date_and_time`、分节符增删…）编辑器在 M8 **用不到**，随 M9 协议来。
- **渲染器接管排版启发式**、`resolve` 暴露给编辑器——M9。
- `hashProtectionPassword` 搬 Rust——不搬（`spec/18`「不在 M7」）。
- napi 形态——只在 wasm 性能不够时考虑（`spec/18` 待决 1 的备选；8.6 的数字决定要不要重开这个问题）。
- 编辑器 UI 变化、Web 版 / 移动端部署。

## 风险提示（实现前确认）

1. **`Map` 与 `undefined`**：TS 类型里 `styles / numbering / headingStyleIds` 是 `Map`，编辑器到处 `.get()`；JSON 回来是对象，包装忘了
   重建就是运行期 `TypeError`，而且 vitest 全绿之外的地方（e2e）才暴露。8.1 单测钉住三处；`headingStyleIds` 的键是**数字**，
   `JSON.parse` 后是字符串键，重建时要 `Number()`。
2. **wasm 内存**：`parse(bytes)` 要把字节拷进线性内存、JSON 再拷出来（`internal.documentXml` 是整个主 part 的字符串，带图文档的 dataURL
   几十 MB）；线性内存只增不减，大文档反复保存会让驻留内存停在峰值。8.6 量化；M9 去 dataURL 与句柄化是根治。
3. **jsdom / node 里加载 wasm**：`--target web` 的 glue 用 `fetch` / `URL`，node 需要 `initSync(bytes)`；vitest 的 alias 把
   `@genoffice/docx-engine` 指向源码，分派层必须在源码入口 `index.ts` 生效，不能只在打包产物里。
4. **像素基线的假阴性**：`docs-visual` 只有 5 份文档且全是表格；JSON 相同不代表所有渲染路径被覆盖——门 3 只能证明「这 5 份不变」，
   编辑器级正确性主要靠门 2 的 151 个测试与 8.3 的审计。
5. **测试断言 TS 私有形态**：`apps/docs/tests` 里可能有断言 `rawRPr` 自闭合、修订 `w:id === '0'`、EMF 占位 dataURL 之类的用例；
   改测试必须登记（`docs/10`），防止把 rs 的 bug 当差异放过。
6. **Strict 文档的用户可见变化**：TS 装载时归一化为 Transitional，rs 保 Strict。Word 两种都能开，但下游若有只认 Transitional 的工具
   会受影响（`docs/03` §14 原话）；发布说明 + 开关回退。
7. **genoffice 是另一个仓库**：分支策略、评审、CI 时间都不在本仓库控制；8.0 先把分支与 CI 跑通再动代码。
8. **7.10 若在 M7 里被顺延**：M8 第一步就是它。8.1 写成「扩展」，7.10 没做则 8.1 连它一起做，工期相应加。
9. **保存路径的 `isUnchanged` 短路**：TS `saveDocx` 在「全 original 顺序不变、无选项」时返回原字节（`EDIT-04`），编辑器可能依赖
   「保存后字节相同 → 不重解析」这类隐含行为；`compat_ts` 已复现该短路（`save_blocks` 有 43 份逐字节相同），但 `saved_at` 单独
   设置时我们不触发保存（`docs/05` 有意不同表第 2 行）——审计时点名核对编辑器有没有靶向 `dcterms:modified` 的逻辑。

## 待决（需要项目负责人拍板）

| # | 事项 | 建议 |
| --- | --- | --- |
| 1 | genoffice 基线：从当前脏树还是干净提交开分支；是否借机重导语料（`TEST-02`） | 建议：从 genoffice 最新 `main` 开 `rsword-engine`，**重导一次**语料对齐（M7 7.0 的 `changedParts` 重导本来就要做，合成一次），`manifest.jsonl` 记新提交号 |
| 2 | 绑定包放哪：genoffice `packages/docx-engine-rs/`（提交产物）vs rsWordParser 发 npm 包 vs git submodule | 建议：genoffice 内提交产物 + `RSWORD_COMMIT` + CI 重建校验（`npm ci` 不需 Rust；两仓库版本关系显式） |
| 3 | 缺省引擎切换时机：M8 合并即缺省 `rs`，还是先 `ts` 缺省灰度一个版本 | 建议：M8 合并即 `rs` 缺省（否则门 3 不算通过），`ts` 保留一个发布周期可回退 |
| 4 | `file-parse` / `markdown` 是否同批切换 | 建议：同批（同一分派层零成本；`file-parse` 的纯文本提取是 `ParsedDoc` 的子集，`markdown` 的导出走 `blank + saveDocx`） |
| 5 | 诊断透出的 UI 形态 | 建议：开发模式 console + 生产仅日志；只有 `EngineInvariantViolation` 提示用户并回退 |
| 6 | 往返 e2e 用的真实文档：从 `corpus/real` 挑哪份、许可 | 项目负责人挑一份自有样本（`docs/07` 任务 A 那批） |
