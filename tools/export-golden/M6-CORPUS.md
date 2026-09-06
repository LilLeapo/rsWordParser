# M6 嵌入对象语料任务书（给 codex / kimi）

> 读完本文再动手。相关背景：`CLAUDE.md`（仓库规则）、`spec/17-m6-plan.md`（M6 计划，「实测差距」一节说明了为什么缺语料）、
> `tools/export-golden/README.md`（导出机制）、`tools/export-golden/hostile.export.test.ts`（一个现成的、与你要写的同类型的文件）。

## 1. 背景与目标

`rsword` 是 genoffice `packages/docx-engine`（TypeScript）的 Rust 重写。`corpus/synthetic/` 里的语料是 **TS `parseDocx` 的输出记录**
（每份 `.docx` 配一份 `.expected.json`，保存用例配 `.save.<k>.json`），Rust 侧用差分工具对照它们。M6 要做的域——图表、
SmartArt、绘图画布（`lc:lockedCanvas`）、OLE 嵌入对象、OMML 公式、ruby、墨迹批注、新图片 / 图表的保存——在今天的 573 份
语料里几乎是空的：

| 特征 | 今天有几份 | 缺什么 |
| --- | --- | --- |
| 图表 part（`word/charts/chartN.xml`） | 1（柱状图 2 系列） | 折线 / 饼 / 环 / 面积 / 散点 / 气泡 / 3D / 堆积 / 组合、各种缓存形态、标题形态、颜色与 `c:style` 调色板、图例位置 |
| chartex（`cx:chartSpace`） | 0 | sunburst / treemap / waterfall / boxWhisker / funnel；带与不带 `mc:Fallback` 图片 |
| SmartArt 数据 part | 2（其中 1 份 part 缺失） | 节点树顺序、pres 点排除、孤立点、多 run 文本 |
| SmartArt 绘图 part（`diagrams/drawingN.xml`） | **0** | 全部：形状几何 / 连线 / 图片填充 / 主题色 / 文字 |
| `lc:lockedCanvas` | **0** | 全部：缩放、文字溢出、图片、锚定 |
| OMML 公式 | 6 | 大部分元素种类、Word 风格属性包、`oMathPara` 与正文混排 |
| ruby | 4 | 对齐 / 字号变体、多 run、与超链接 / 表格组合 |
| OLE `w:object` | 14 | 一段多对象、Link 型、缺预览 + 文字、格里带文字 |
| 墨迹（`aidocs-ink`）解析侧 | **0**（保存用例 9 份） | 解析侧 golden 只能由 TS 保存产物再解析得到 |
| 新图片保存（`kind:"image"`）/ `replaceImage` | 4 + 1 | 九种 wrap、旋转翻转、去重、`r:link` 与 `a:srcRect` 的 replaceImage |

目标：用 TS 自己的测试助手合成一批小 docx，让 TS 为它们生成 golden。**不是**写 Rust，**不是**手写期望值。

## 2. 铁律

1. **禁止手写或修改 `corpus/**/*.json`、`corpus/**/*.docx`**。它们只能由 `tools/export-golden/run.sh` 生成（`CLAUDE.md`）。
2. **不改 genoffice 仓库（`~/code/genoffice`）的任何文件**。你的文件放在本仓库 `tools/export-golden/` 下，导出时被复制过去执行。
3. **不要运行 `tools/export-golden/run.sh`**：它会先清空整个 `corpus/synthetic` 与 `corpus/hostile` 再重建，只由任务发起人最后跑一次。
   开发验证一律用 `tools/export-golden/try.sh <你的文件名>`（产物进临时目录，不碰 corpus）。
4. **TS 是参考实现不是权威**：TS 的输出看起来不对也照录，不要"修"它，也不要为了让输出好看去改输入；把疑似缺陷列在回复里。
5. 不改 `record.ts` / `build-docx.wrapper.ts` / `src-index.wrapper.ts` / `vitest.config.ts` / `run.sh` / `try.sh`。需要改就在回复里说。
6. 只碰分给你的文件；**不要 `git commit`**（发起人审过、全量重导后一起提交）。
7. 注释中文、标识符英文（仓库约定）。

## 3. 机制（照 `hostile.export.test.ts` 的写法）

你的文件 `tools/export-golden/<name>.export.test.ts` 会被复制到 genoffice `packages/docx-engine/export-golden.tmp*/` 里，
用 genoffice 自己的 vitest 执行。因此相对导入是相对**那个目录**写的：

```ts
import { describe, expect, it } from 'vitest'
import * as real from '../tests/helpers/build-docx'   // 真实构造助手：real.buildDocx / TINY_PNG_BASE64 / CHART_PART_XML …
import { record } from './record'                      // 录制：record(bytes, builder, forcedStem)
import { parseDocx, saveDocx, patchChartPartXml } from '../src/index'  // 必须写成这个路径：alias 把它换成录制包装
```

- `record(bytes, 'buildDocx', stem)`：写 `<stem>.docx` + `<stem>.expected.json`（`parseDocx` 抛错则写 `.error.json`），
  并在 `manifest.jsonl` 记一行（含 `"parse":"ok"`）。**stem 必须全局唯一**：`<前缀>__<三位序号>`，前缀见分工表；
  写一个小计数器 `const stem = (prefix: string) => \`${prefix}__${String(++n[prefix]).padStart(3, '0')}\``。
- `real.buildDocx({ bodyXml, extraParts, binaryParts, extraRels, withImage, extraStylesXml, sectPrExtra })`：
  `extraParts` 是 `{ path, xml, contentType }`（会写 `[Content_Types]` Override），`binaryParts` 是
  `{ path, base64, extension, contentType }`（写 Default），`extraRels` 是拼进 `word/_rels/document.xml.rels` 的
  `<Relationship …/>` 字符串，`withImage: true` 给一张 `rId10` 的 1×1 PNG（`word/media/image1.png`）。
  图表的现成材料：`real.CHART_PART_XML` / `real.CHART_PARAGRAPH_XML` / `real.CHART_RELS`。
  **`buildDocx` 不带 theme part**：凡是要测 `schemeClr` / 调色板 accent 的用例，自己用 `extraParts` 加一份最小
  `word/theme/theme1.xml`（`a:clrScheme` 六个 accent + dk1/lt1/dk2/lt2），`extraRels` 加 theme 关系，内容类型
  `application/vnd.openxmlformats-officedocument.theme+xml`；没有 theme 时 TS 的 `palette` 是 `undefined`、`schemeClr` 解析不出——这本身也值得各留一份。
- **保存用例**：`await saveDocx(parsed, blocks, options)`——`parsed` 必须来自**刚 record 过的字节**（包装按源字节哈希找 stem），
  会自动写 `<stem>.save.<k>.json`（`SaveBlock[]` + `SaveOptions` + 输出的 `word/document.xml`）。
  `blocks` 里的 `{ kind: 'original', docxIndex }` 用 `parsed.blocks[i].docxIndex`。
- **保存产物再录制**：`await record(saved, 'saveDocx-output', stem('m6-ink'))` 把 TS 保存出来的 docx 当作一份新的解析语料。
  墨迹的解析侧 golden **只能**这么来（TS 只在保存时写墨迹 run）；图表插入 / 新图片保存的产物也值得各录一份。
- **在 `it` 里写断言**（`expect(parsed.blocks[0].chartDisplay?.kind).toBe('pie')` 一类），断言你要的特征真的出现在 golden 里。
  这样 `run.sh` 全量重导时也在自检；断言失败只影响你这条用例（`run.sh` 会继续并给 WARNING）。
- 验证：`tools/export-golden/try.sh embedded-graphics.export.test.ts`，看末尾统计（`error: 0`），
  `jq -c 'select(.parse != null) | {stem, parse}' <out>/manifest.jsonl | grep -v '"ok"'` 应为空，再用 `jq` 抽查几份 `.expected.json`。
  环境：Node ≥ 22；genoffice 在 `~/code/genoffice`，vitest 用它根目录的 `node_modules/.bin/vitest`（`try.sh` 已处理；**不要用 pnpm**）。

## 4. 分工

| 谁 | 文件 | 前缀 | 内容 |
| --- | --- | --- | --- |
| **codex** | `tools/export-golden/embedded-graphics.export.test.ts` | `m6-chart` / `m6-chartex` / `m6-smartart` / `m6-canvas` | A 组：图表、chartex、SmartArt、画布；图表相关的保存用例 |
| **kimi** | `tools/export-golden/embedded-text.export.test.ts` | `m6-omml` / `m6-ruby` / `m6-ole` / `m6-ink` / `m6-image` | B 组：公式、ruby、OLE、墨迹、新图片与 `replaceImage` 的保存用例 |
| **kimi** | `tools/export-golden/hostile.export.test.ts`（**追加**一个 `describe`，别动已有用例） | — | 6 份 M6 病态输入（第 7 节） |

工作树：`/Users/lilleap/code/rsWordParser-m6-corpus`（分支 `m6-corpus`）。两个人在同一个工作树里改**不同的文件**。

参考 TS 源码（`~/code/genoffice/packages/docx-engine/src/`）：`chart.ts`（图表 part 解析 / 生成 / 补丁）、`parse.ts` 的
`extractChart`（≈5567 行）/ `extractDiagramText`（≈5171）/ `extractLockedCanvas`（≈5230）/ `extractDiagramDrawing`（≈5376）/
图表与 SmartArt 的分类分支（≈1000–1080）/ 公式块（≈835）、`math.ts`、`ink.ts`、`patch.ts` 的 `embedChart`（≈528）/
`embedImage`（≈480）/ 墨迹注入（≈584）。**最快的第一批**：`tests/chart-parse-model.test.ts`、`tests/chart-insert.test.ts`、
`tests/chart-edit.test.ts`、`tests/math.test.ts` 里直接对 XML 字符串断言（没经过 `buildDocx`）的字面量——把它们包进 docx
就是语料；这些用例今天不在 corpus 里。

## 5. A 组清单（codex）

每一行至少一份文档；"期望"一栏是你在 `it` 里要断言的东西（TS 行为，不是我们的意愿）。

**经典图表（`c:` 命名空间；part 放 `word/charts/chartN.xml`，关系类型 `…/relationships/chart`）**

| # | 用例 | 期望（`blocks[i].chartDisplay`） |
| --- | --- | --- |
| A1 | 簇状柱形（`c:barDir val="col"`） | `kind: 'bar'`，无 `horizontal` |
| A2 | 条形（`c:barDir val="bar"`） | `horizontal: true` |
| A3 | 堆积柱形 / 百分比堆积面积（`c:grouping`） | `grouping: 'stacked'` / `'percentStacked'`；`clustered` 不出现 |
| A4 | 折线带标记（`c:marker val="1"`）/ 不带 | `markers: true` / 无 |
| A5 | 饼 / 环（`c:holeSize val="30"` / 缺省）/ 3D 饼 | `kind: 'pie'`，`holePct: 30` / `50` / 无 |
| A6 | 面积、bar3D、line3D、area3D | 对应 kind |
| A7 | 散点：markers only / `lineMarker` / `smoothMarker` / 系列 `a:ln/a:noFill` | `kind: 'scatter'`，`xValues`，`line` 的有无，`markers` |
| A8 | 气泡（`c:bubbleSize`） | `kind: 'bubble'`，`sizes` |
| A9 | 组合图（barChart + lineChart 都在 plotArea） | 第一个 `*Chart` 决定 kind |
| A10 | 雷达 / 股价等未映射种类 | `kind: 'other'` |
| A11 | 缓存形态：`strRef/strCache`、`numRef/numCache`、`strLit`、`numLit`、`ptCount` 大于实际点数（留空）、非数字 `c:v` | `categories` 补 `''`，`values` 出 `null` |
| A12 | 类别是日期序列号（`formatCode` 含 `m/d/yyyy`）/ `xVal` 长小数 `0.70000000000000062` | 类别成日期文本 / 四舍五入到 4 位 |
| A13 | 标题：`a:t` 富文本 / `strRef` 的 `c:v` / 空 `c:title`（自动标题）/ `autoTitleDeleted val="1"` / 单系列自动标题 | `title` 各形态；`previewText` = title |
| A14 | 颜色：`srgbClr` / `schemeClr accent1 + lumMod 75000 lumOff 25000` / `sysClr lastClr` / `c:dPt` 逐点填充（饼） | `series[].color` / `pointColors` |
| A15 | `c:style` 1 / 2 / 5 / 40、`mc:AlternateContent` 里的 `c14:style 102`、无 `c:style` | `palette`（1 灰阶、2 六 accent、3–8 单色阶梯） |
| A16 | 图例：`legendPos` b / l / r / t / tr、有 `c:legend` 无 `legendPos`、无 `c:legend` | `legendPos` |
| A17 | 无任何带缓存的系列 | `chartDisplay` **缺失**（TS 返回 null），块仍是 `passthrough` `Chart` |
| A18 | `c:chart r:id` 悬空 / 关系 `TargetMode="External"` / part 缺失 | 同上 |
| A19 | 图表锚定（`wp:anchor`）而不是 inline；一段两张图表；图表与文字同段；图表在表格单元格里 | 看 TS 怎么分（照录） |
| A20 | 图表 part 带 `c:externalData r:id` + `word/charts/_rels/chart1.xml.rels` + 内嵌 `embeddings/Microsoft_Excel_Worksheet.xlsx`（可以用任意小字节当占位） | `extras.chartParts` 有该 part 原文 |

**chartex（`cx:chartSpace`，`a:graphicData uri="http://schemas.microsoft.com/office/drawing/2014/chartex"`）**

| # | 用例 | 期望 |
| --- | --- | --- |
| A21 | sunburst / treemap / waterfall / boxWhisker / funnel / paretoLine，各带 `cx:chartData/cx:data` 的 `strDim` + `numDim` | `kind` 按最近的经典种类；`extras.chartParts` **不含** chartex part |
| A22 | `cx:chartData` 改名成别的元素（TS 的宽容分支） | 仍解析 |
| A23 | `mc:AlternateContent`：Choice 里 chartex，Fallback 里 `w:drawing` 图片（`withImage`） | 块成 `type: 'image'`（TS 偏爱 Fallback 图） |
| A24 | 同上但没有 Fallback | `passthrough` `Chart` + `chartDisplay` |

**SmartArt（`dgm:relIds r:dm r:lo r:qs r:cs`，四个 part 都给，内容类型 `…drawingml.diagramData+xml` 等）**

| # | 用例 | 期望 |
| --- | --- | --- |
| A25 | 只有数据 part：`dgm:pt` 若干（含 `type="pres"` / `parTrans` / `sibTrans` 的干扰点），`dgm:cxn` 建两层树且 `srcOrd` 与文件序**不同**；再加一个孤立点；`a:t` 拆在多个 `a:r` 里；文本含 `&amp;` | `previewText` 为按树序的 `\n` 连接文本 |
| A26 | 数据 part + 绘图 part `diagrams/drawing1.xml`：`dsp:sp` 各一——`rect`、`roundRect`、`ellipse`、连线 `prst="line"` `cy=0`、`bentConnector3`；填充 `srgbClr` / `schemeClr accent2` / 无填充；`a:ln` 有色 + 宽度 / `noFill`；`blipFill` 图片（用**绘图 part 自己的** `_rels/drawing1.xml.rels` 指到 `../media/image1.png`）+ `a:fillRect` 负值；`dsp:txBody` 多段文字 + `sz="1400"` + `srgbClr`；一个 `rot` | `diagramDisplay.shapes[]` 逐字段 |
| A27 | 数据 part 命名 `data.xml`（无编号）/ `data3.xml` | 绘图 part 路径替换规则 `data(\d*).xml → drawing$1.xml` |
| A28 | 绘图 part 缺失 | 只有 `previewText`，无 `diagramDisplay` |
| A29 | 图示锚定（`wp:anchor`）且同段还有一张锚定照片（`pic:pic`） | `diagramDisplay.floating/offsetXEmu/offsetYEmu`，`textboxes[]` 里出现照片框 |
| A30 | 图示在表格单元格里；图示与正文同段 | 照录 |
| A31 | `dgm:cxn` 成环（A→B→A）与自指（温和版；极端版归 hostile） | 不死循环，文本不丢 |

**绘图画布（`a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas"`，`lc:lockedCanvas`）**

| # | 用例 | 期望 |
| --- | --- | --- |
| A32 | inline 画布，`a:grpSpPr/a:xfrm` 的 `chOff` 非零、`chExt` 是 `wp:extent` 的 3 倍；子 `a:sp` 矩形带文字（`sz="1800"`）、`ellipse` 填 `schemeClr accent1`、`a:pic` 图片、一个 `rot`、一个 `prstGeom prst="rect"` | `diagramDisplay.canvas: true`，`shapes[]` 的 px 按缩放，`fontSizePt` **不缩放**，`prst` 为 rect 时不给 |
| A33 | 文字严重溢出：小盒子里放长文本（触发 TS 的分栏启发式），另一个用例文本恰好"每行一字"（触发逐字拆分） | `shapes` 被重排 / 拆成逐字形状（照录，这是 TS 的排版启发式） |
| A34 | 锚定画布 `wp:anchor` + `wp:wrapNone` / `wrapSquare` | `offsetXEmu`、`floating` 的有无 |
| A35 | 画布没有 `wp:extent` 或 `chExt` 为 0；画布与正文同段 | 照录 |

**保存用例（A 组，写在同一文件里；每个都先 `record` 源文档再 `saveDocx`）**

| # | 用例 | 要点 |
| --- | --- | --- |
| A36 | `{ kind: 'chart', chart: { kind: 'bar' \| 'line' \| 'pie', title, categories, series } }` 各一份；一份带 `extentPx`；一份 `values` 含 `null`；一份一次插两张；一份插在两个 original 之间 | 保存后把产物再 `record` 一份（`saveDocx-output`）：得到"TS 生成的图表"解析 golden |
| A37 | `options.partXml`：用 `patchChartPartXml(原 part XML, patch)` 生成新 part 内容再传入——分别改标题 / 系列名 / 值（含 `null` 保留）/ 类别 / 自动标题注入 / `strRef` 标题 | 源文档用 A13 的几种标题形态 |
| A38 | `options.partBinary`：替换 A20 那份的内嵌工作簿字节 | |
| A39 | 删掉图表块（`blocks` 里不列它的 original）→ TS 的资源回收 | 产物再 `record`，看 part / 关系是否消失（在 `it` 里用 JSZip 断言并写进用例名） |
| A40 | 图表文档无编辑保存（全 original） | `outputIdenticalToSource: true` |

## 6. B 组清单（kimi）

**OMML 公式（`m:` 命名空间已在 `buildDocx` 根上声明；参考 `real.MATH_PARAGRAPH_XML`）**

| # | 用例 | 期望（`formulaDisplay` 或 `runs[].math`） |
| --- | --- | --- |
| B1 | 每种元素一份纯公式段（`w:p` 里只有 `m:oMath`）：`m:f`（含 `m:fPr/m:type val="noBar"`）、`m:rad`（含 `degHide`）、`m:sSup`、`m:sSub`、`m:sSubSup`、`m:sPre`、`m:d`（缺省括号 / `begChr="["` `endChr="]"` / 多个 `m:e` 带 `sepChr`）、`m:nary`（∑ `undOvr`、∫ `subSup`、`supHide`）、`m:func`（sin、lim）、`m:limLow` / `m:limUpp`、`m:acc`、`m:bar`、`m:box`、`m:borderBox`、`m:groupChr`、`m:eqArr`、`m:m`（2×2 矩阵）、`m:d` 里套 `m:m`、`m:phant` | `formulaDisplay.tokens / mathml / latex / omml`；哪些 `latex` 缺失（TS 子集外）照录 |
| B2 | `m:oMathPara` 包两个 `m:oMath`（带 `m:oMathParaPr/m:jc`） | 仍是 `Equation` 块 |
| B3 | `m:oMathPara` + 同段还有普通 `w:r` 文字 | `mathml` **缺失**（TS 只给纯公式 2D） |
| B4 | 正文夹公式：`see <oMath> here`；一段两处公式；公式在 `w:hyperlink` 里；公式在表格单元格里 | `runs[]` 拆成 文字 / `{text, math:{omml}}` / 文字 |
| B5 | Word 风格属性包：`m:oMathPara/m:oMathParaPr`、`m:ctrlPr` 里带 `w:rPr`、`m:r` 里 `m:rPr/m:sty val="p"` + `w:rPr`、`m:t xml:space="preserve"` 带空格、实体 `&lt;` `&amp;` | `tokens` 解码正确 |
| B6 | 运算符与符号：`±×÷≤≥≠→∞∂∇`、希腊字母、`m:r` 里多字符混合 `2x+1` | `mathml` 的 `mn/mi/mo` 分类照录 |
| B7 | 公式段带 `w:pPr`（居中、样式）；公式段带书签 / 批注范围标记 | 照录 |

**ruby（`w:ruby`）**

| # | 用例 | 期望 |
| --- | --- | --- |
| B8 | `rubyAlign` 各值、`hps` / `hpsRaise` / `hpsBaseText` / `lid`；`rt` 两个 run；`rubyBase` 两个 run 且带 `w:rPr`；ruby 前后有普通文字；ruby 在 `w:hyperlink` 里；ruby 在表格单元格里；一段三个 ruby | `runs[].ruby { rt, xml }`，`text` = base 文字 |

**OLE（`w:object`）**

| # | 用例 | 期望 |
| --- | --- | --- |
| B9 | 一段两个 `w:object`；`w:object` 所在 run 带 `w:rPr`（颜色 / 加粗）；`o:OLEObject Type="Link"` + `o:LinkType`；`v:shape` 无 `style`（回退 `dxaOrig/dyaOrig`）；预览 `r:id` 悬空 + 段落有文字；`w:object` 在表格单元格里且格里有文字；EMBED 字段包着 `w:object` 且字段后还有文字；`w:object` 与 `w:drawing` 图片同段 | `runs[].image { dataUrl, xml 含 <o:OLEObject, widthPx, heightPx }` 或 `passthrough` `Embedded object`（照录） |

**墨迹（只能经 TS 保存产生；用 `saveDocx(..., { inks })` 后把产物 `record`）**

| # | 用例 | 要点 |
| --- | --- | --- |
| B10 | 一条墨迹锚在第 2 段；两条锚同一段；两条锚不同段；负偏移；`payload` 含引号 / `&` / 中文 / 换行；空段（自闭合 `<w:p/>`）为锚；锚点是表格块（应被跳过）；锚点段在 `w:sdt` 里 | 每份都：`save.json` 一条 + 产物 `record` 一份（`m6-ink__NNN`，期望 `inks[]` 非空、`inks[0].payload` 等于输入、被批注段仍 `type: 'paragraph'`） |
| B11 | 把 B10 的产物 `parseDocx` 后再 `saveDocx`：`inks: []`（清除）、同一条改锚点、再加一条、`inks` 不传（no-op） | 二次产物再 `record`（期望 `inks` 变化正确；用 JSZip 断言媒体 part 与关系没有累积） |

**新图片与 `replaceImage`（保存用例）**

| # | 用例 | 要点 |
| --- | --- | --- |
| B12 | `{ kind: 'image', image: { base64: real.TINY_PNG_BASE64, mime, widthPx, heightPx, … } }`：`mime` 三种；`align` 三种；`wrap` 九种（`square-left` / `square-right` / `topBottom` / `behind` / `front` / `tight-*` / `through-*`，以 TS `ImageWrap` 类型为准）；`posOffsetEmu`；`zOrder`；`rotDeg 90` + `flipH`；`paraSpacing`；同一字节插两次（去重成一个媒体 part） | 每份 `save.json` + 产物 `record`（`m6-image__NNN`，期望 `blocks[i].type === 'image'`、`imageWrap` 等） |
| B13 | `{ kind: 'xml', xml: 原图片段落 originalXml, docxIndex, replaceImage: { base64, mime } }`：源图片有 `a:srcRect` 裁剪；源是 `r:link` 外链图；源带 `asvg:svgBlip` 扩展；连续替换两次（第二次以第一次的产物为源） | 产物 `record`；用 JSZip 断言旧媒体 part 是否被回收（照录 TS 行为） |
| B14 | 删掉一个图片块（original 不列）；删掉带图的表格块 | 产物 `record` + JSZip 断言 |

## 7. hostile（kimi，追加到 `hostile.export.test.ts`，用已有的 `emit(name, bytes, expectation)`；期望字符串按下表）

| 文件名 | 病态 | expectation |
| --- | --- | --- |
| `chart-part-malformed` | chart part 标签不闭合 | `parses; chart block without chartDisplay; PKG_OPAQUE_PART; unedited save byte-identical` |
| `chart-missing-rel` | `c:chart r:id` 悬空；另一段 `cx:chart` 无 Fallback | `parses; PKG_REL_MISSING; no chartDisplay` |
| `diagram-cyclic-cxn` | `dgm:cxn` A→B→A、自指、`srcOrd` 缺失，5,000 个 `dgm:pt` | `parses without hanging; all texts kept once` |
| `canvas-degenerate` | `chExt` 0 与负数、坐标 `1e30`、`sz="-5"`、`a:pic` 无 `a:blip` | `parses; MOD_BAD_GEOMETRY; no non-finite numbers in output` |
| `omml-deep` | 3,000 层 `m:f/m:num/m:f/...` 套娃 | `parses; MOD_TOO_DEEP; no stack overflow` |
| `ink-garbage` | `aidocs-ink` run：`r:embed` 悬空、`descr` 含 `&quot;&amp;`、`posOffset="abc"` | `parses; inks[0].dataUrl null; payload decoded; offsets 0` |

这些文档 TS 可能解析失败或很慢——`emit` 只落字节不解析，没问题；但**别**用 `record` 录它们。

## 8. 完成标准与交付

1. `tools/export-golden/try.sh <你的文件>` 退出码 0（若 TS 对某份文档抛错而你认为那是 TS 缺陷，保留用例、把断言改成记录现状，并在回复里列出）。
2. 临时目录里 `*.error.json` 为 0；`manifest.jsonl` 里你的 stem 全部 `"parse":"ok"`。
3. 清单每一行至少一份文档，`it` 里有针对该行"期望"的 `expect`。
4. 每份文档尽量小（PNG 用 `real.TINY_PNG_BASE64`；工作簿占位几十字节即可），XML 像 Word 写的（命名空间在元素上声明齐全；
   `mc:AlternateContent` 的 `Requires` 前缀必须声明，否则 Rust 侧会走 Fallback）。
5. 回复里给：① 每个前缀的份数（解析 / 保存）；② 疑似 TS 缺陷清单（用例名 + 现象）；③ 你没做到的行与原因。
6. 不提交、不改分给别人的文件、不碰 corpus。
