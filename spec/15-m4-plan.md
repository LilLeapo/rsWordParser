# SPEC 15 · M4 任务分解

对应 `docs/03` 第 12 节 M4 行。格式同 `spec/12` / `spec/13`：每个任务给出产出、依赖的规范条目与完成
定义（DoD）。顺序即建议的实现顺序；同一编号内的子任务可并行。

M4 与 M2（`spec/13`）并行开发：绘图是读侧最大的一块，且不依赖 Span 与字段（`docs/04` §10）。
编号 14 预留给 M3（表格）的任务分解。

## M4 · L3 显示模型：绘图、图片、VML、颜色算法

目标：`w:drawing` / `w:pict` / `w:object` 的**文档事实**进入模型（`MOD-11` 显示模型），媒体可解析，
DrawingML 颜色算法可用；`compat_ts` 把这些事实投影成 TS 的 `image*` / `textboxes` / `rule*` 字段。

CI 门（`spec/11` TEST-10 的「M3–M6 对应域 diff 为 0」）：`diff-parse --scope all` 里绘图域未知差异为 0，
即下面「实测差距」表的全部路径归零（表格内与页眉页脚内的图片除外，见「被阻塞」）。

### 实测差距（2026-09-04，`cargo run -p diff-parse -- --scope all`）

全域 1,925 处未知差异 / 341 份文档。其中绘图域直接命中 **1,016 处**，涉及 **165 份**文档；
其中 **155 份**文档的差异全部是绘图域字段与它们的连带项（`label` / `type` / `previewText` / `runs`）。

| 路径 | 差异点 | 归属任务 |
| --- | --- | --- |
| `imageWidthPx` / `imageHeightPx` | 129 / 127 | 4.3 |
| `imageWrap` | 102 | 4.4 |
| `textboxes` | 99 | 4.6 |
| `imageOffsetYEmu` / `imageOffsetXEmu` | 76 / 59 | 4.3 |
| `imageDataUrl` | 48 | 4.1 |
| `imageWrapDist{Top,Bottom,Left,Right}Emu` | 30 × 4 | 4.3 |
| `imageZOrderNormalized` / `imageZOrder` | 19 / 8 | 4.4 |
| `imageAlign` | 17 | 4.4 |
| `decorative` | 12 | 4.5 |
| `oleProgId` | 8 | 4.7 |
| `rule{WidthPx,ColorHex,ThicknessPx}` | 5 / 3 / 3 | 4.5 |
| `runs[].image` | 6 | 4.3 |
| `imageLeading*` / `imageParagraphIndentFirstLine` | 3 / 2 / 1 / 1 / 2 | 4.4 |
| `strayRuns` / `strayStyleId` | 3 / 1 | 4.6 |
| `brokenImage` / `imageRotDeg` / `imageBorder` | 2 / 2 / 1 | 4.1 / 4.3 |

连带项（分类正确后自动归零）：`label` 155 处（`Text box` 98、`Image` 43、`Drawing object` 12、
`Embedded object` 2）、`type` 148 处（TS `passthrough` vs 我们的 `paragraph`）、`previewText` 约 130 处、
`runs` 148 处（TS 的 passthrough 块不带 `runs`，我们输出 `[]`）。

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 4.1 | `MediaStore` 首版：`MediaId → {part, mime, bytes, kind}`；`r:embed` / `r:link` / `v:imagedata r:id` 经**所在 part 自己的 rels** 解析（含 `..` 段归一化）；MIME 判定顺序=扩展名表 → `Override` → `Default`，须以 `image/` 开头；External 与 `http(s)://` 目标直出 URL；EMF/WMF/EMZ/WMZ/TIFF 标 `MediaKind::Metafile` / `Tiff` **不转换**（`docs/03` §3.5）。`compat_ts` 内联 `data:<mime>;base64,…` | PKG-05, MOD-11 | 全语料每个 `a:blip r:embed` / `r:link` / `v:imagedata r:id` 都解析到 part + mime（单测按语料统计，未命中的只有真正缺关系的用例）；dataURL 生成与 metafile 判定有单测。**注意**：`imageDataUrl` / `brokenImage` 这 50 处路径要等 4.4 的分类到位才会输出该字段，归零在 4.4 一并验收；4 份 `emf-image__*` 的 metafile 占位在本任务登记为有意差异 |
| 4.2 | DrawingML 颜色算法：`srgbClr`/`schemeClr`/`sysClr`/`prstClr`/`scrgbClr`/`hslClr` + `lumMod`/`lumOff`/`tint`/`shade`/`satMod`/`hueMod`/`alpha` 按规范要求的色彩空间变换 → sRGB；`schemeClr` 别名 `tx1→dk1, bg1→lt1, tx2→dk2, bg2→lt2`；`gradFill` 等权平均、`pattFill` fgClr；EMU/px/pt/twips 换算集中一处（px = EMU/9525） | RES-05 | 单测覆盖每种变换与别名；`accent1 + lumMod/lumOff` 与 Word 显示一致（允许 ±1/255）；`gradFillApproxHex` 与 TS 对齐 |
| 4.3 | Drawing 索引与 `ImageDisplay` / `AnchorGeom`：`wp:inline` / `wp:anchor` → 锚定几何（`relativeFrom`、`align`、`posOffset`、`pct*`、`wrap*`、`behindDoc`、`allowOverlap`、`relativeHeight`、`dist{T,B,L,R}`、`layoutInCell`、`hidden`）；`pic:pic` → `ImageDisplay`（media、`wp:extent` EMU、`a:srcRect` crop、`a:stretch/fillRect`、`a:xfrm` rot/flip、`pic:spPr/a:ln` 边框、`wp:docPr` 的 name/descr/decorative）。挂到 `Segment.display`，遍历写成**迭代** | MOD-06, MOD-11 | 全语料每个 `w:drawing` 都建出显示模型，`wp:extent` 与 TS 的 `imageWidthPx/HeightPx` 逐项一致；文本框里的图不被宿主 drawing 认领。**注意**：这些字段同样要等 4.4 的分类才会输出，`imageWidthPx/HeightPx/OffsetXEmu/OffsetYEmu/WrapDist*Emu/RotDeg/Border` 与 `runs[].image` 的归零在 4.4 一并验收 |
| 4.4 | 图片段落分类与 TS 投影：R15 细化（`type: image` / 带图文本段 / `passthrough "Image"`）、`imageWrap` 九种取值的判定、`imageAlign`（宿主段落 `w:jc`）、`imagePosH/V`、`imageNoOverlap`、`imageZOrder` 与 `normalizeImageZOrders`、`imageLeading*`（图前引导文字）与 `imageParagraphIndentFirstLine` | MOD-05, COMPAT-03 | `imageWrap` 102、`imageAlign` 17、`imageZOrder*` 27、`imageLeading*` 9 归零；4.1 / 4.3 挂账的 `imageDataUrl` 48、`brokenImage` 2、`imageWidthPx/HeightPx` 256、`imageOffset*Emu` 135、`imageWrapDist*Emu` 120、`imageRotDeg` 2、`imageBorder` 1 与 `runs[].image` 6 一并归零；`type` / `label` / `previewText` / `runs` 的图片相关连带项归零 |
| 4.5 | VML 显示模型 `VmlDisplay`：`v:shape/rect/roundrect/oval/line/group` 的 `style` 键值原样保留、`fillcolor`/`filled`/`strokecolor`/`stroked`、`v:imagedata`、`v:textpath`、`v:group` 的 `coordsize` 缩放；细横线（`v:rect o:hr`）与 DrawingML 细线（`wp:extent cy ∈ (0,130000]`）→ `decorative` + `rule*`；`vmlImageMeta` | MOD-11, MOD-05 | `VmlDisplay` 挂到 `Segment.display` / `ProtectedBlock.display` / `ImageBlock.display`；VML 细横线（`smartart-ole__005`）的 `decorative` / `ruleColorHex` / `ruleThicknessPx` 归零。**注意**：另外 11 份 `decorative` 与 9 处 DrawingML `rule*` 挂在 「本该是 Drawing object、现在还被当普通段落」的块上，那条分类要等 4.6 的文本框判定，所以 `isThinRule` / `ruleDisplayOf` 与它们一起在 4.6 验收 |
| 4.6 | 文本框：`ShapeDisplay`（`wps:wsp` 的 `spPr` 几何 / fill / line / `bodyPr` insets / `wps:style` 的 `fillRef`/`lnRef`）、`wpg` 组的 CTM 合成、`txbxContent` 作为**独立内容流**复用段落管线、VML 文本框与 WordArt（`v:textpath`）；`strayRuns` / `strayStyleId`；锚定投影（`applyAnchor`：`bandTopPx`/`bandBottomPx`/`floating`）放在 `compat_ts`，不进模型 | MOD-11, COMPAT-03 | 分两步。**4.6a 分类**：`ShapeDisplay` 进模型，投影层判 `Text box` / `Drawing object` / 隐藏形状，带出 `label` / `type` / `previewText` / `strayRuns` / `decorative` / DrawingML `rule*` / `imageMeta`。**4.6b 载荷**：`textboxes[]` 的几何、填充、内边距、band 与 `paras[].runs`——半截数组会把一处差异拆成十几处，所以整块一起上 |
| 4.7 | OLE 与 `w:object`：`o:OLEObject/@ProgID` → `oleProgId`；`v:imagedata` 预览图；尺寸取 `v:shape style` 的 pt，缺省 `w:object` 的 `dxaOrig/dyaOrig` twips；`w:jc` | MOD-11, COMPAT-03 | `oleProgId` 8 处归零（含 `onlyOleFields`：段落里还有别的字段时归字段管）；嵌入对象的 `imageDataUrl` / 尺寸 / `imageAlign` 归零；OLE 预览进 `runs[].image` |
| 4.8 | 恶意输入与 M4 门：绘图树深嵌套 / 环状组 / 缺关系 / 畸形 `style` 的降级路径；`corpus/hostile` 补用例；`diff-parse` 绘图域接入 CI | TEST-09, TEST-10 | 绘图域未知差异 0；hostile 语料不 panic 不丢字节；`cargo test --workspace` 与 `--release` 全绿 |

**M4 完成**（4.1–4.8，2026-09-05）：`diff-parse --scope drawing` 在 573 份用例上未知差异为 0，
已接入 CI；`--scope all` 从 341 份 / 1,925 点降到 184 份 / 554 点，剩下的按域全部归 M2/M3/M5/M6。

**进度**：4.1 完成（`package/media.rs` + `tests/media.rs` 语料普查；同时给 L1 补了 `Dom::semantic_descendants`）。4.2 完成（`resolve/drawingml.rs` 颜色 + `model/units.rs` 单位换算；语料 89 个颜色容器全部能定出 sRGB）。4.3 完成（`model/drawing.rs` 挂到 `Segment.display`；112 个绘图、122 项 `wp:extent` 与 TS 一致）。4.4 完成（`bind/compat_ts/{media,image}.rs`：图片块与 run 内图片的 `image*` 投影；全域未知差异 1,925 → 1,560，文档 341 → 300）。4.5 + 4.7 完成（`model/vml.rs` + 细横线与嵌入对象投影；全域 1,560 → 1,517，文档 300 → 291；`oleProgId` 归零）。**4.6a 完成**（`model/drawing.rs` 的 `ShapeDisplay` + `bind/compat_ts/textbox.rs` 分类；全域 1,517 → 704，文档 291 → 279）——`label` / `type` / `previewText` / `runs` / `decorative` / `rule*` 的绘图部分全部归零，剩下的同名差异是字段（M2）与图表（M6）。**4.6b 完成**（`bind/compat_ts/box_json.rs`：框的几何 / 填充 / 描边 / 内边距 / 组仿射 / 锚定偏移 / 框内段落；全域文档 279 → 236，41 份文档彻底对齐）。

**4.6c 完成**（`model/section.rs` 的页面几何 + `resolveAnchorPagePos`；全域 799 → 770，文档 236 → 230）。
节的完整模型（`SectionInfo` + `RES-10` 继承）仍归 M5，这里只读锚定定位真正要的页宽页高、四边页边距、
栏数，M5 建 `SectionInfo` 时替换即可。顺带修掉两处判错：TS 的 `nested` 指「形状在另一个形状的
`txbxContent` 里」而不是「在组里」（组内形状照样有保存序号、照样可编辑），以及框里套框时要把各层
`w:txbxContent` 平铺进同一个 `paras` 并整块标只读。

**4.6d 完成**（`model/custgeom.rs` + `pathData` 投影；全域 770 → 764，文档 230 → 226，那 4 份
`shape-extraction__*` 整份对齐）。只做能如实表达的部分：`moveTo` / `lnTo` / `quadBezTo` /
`cubicBezTo` / `close` 且坐标是数字；遇到 `a:gd` 公式、引导名坐标或 `a:arcTo` 就整条几何不给
——宁可不给路径，也不能给一条少了段或坐标当 0 的错路径。公式求值器与弧转贝塞尔要的话是独立一块活。

**4.6e 完成**（VML WordArt 框 + 投影层重复样板的声明宏）。

**4.6f 完成**：绘图域清零（166 → 0）。补齐的是「放置」这一层——它不是新字段，而是同一批字段
在整段尺度上的解算：

- **锚定上下文**（`box_json::AnchorCtx`）：`posOffset` 归一化（相对页面的偏移减掉页边距，换到栏
  原点空间，否则页边距算两遍）、`pinAll` 首页钉页（`pagePinned`，靠新的 `Ctx::first_page`：块不是
  第一个、且在首个分页之前）、并集铺满栏（并排两个半宽框时缝里排不下字）。
- **wrapSquare 成带**：框（或多绘图段落的并集）几乎铺满整栏时按 `wrapTopAndBottom` 处理，
  `bandTopPx` / `bandBottomPx` / `bandOverflow`。
- **照片框**：组内图片走 `pushPic`（要 `a:xfrm/a:ext`），顶层「图片独占一个绘图」走 `wp:extent`；
  锚定绘图段落里的**随文**图片反过来不成框，作为 `strayRuns` 随行走。
- **VML 画布**：`v:group` 的缩放 / 原点 / `coordorigin`、随文画布的流内占位框、画布孩子的缺省黑
  描边、`vmlPicBox` / `vmlGeomBox`（含 `@path` → 归一化 SVG 路径）、空框丢弃、共享的 `txbxIndex`
  序号、`paragraphStrayBox`（`w:pict` 这条路把框外文字也做成只读框）。
- **嵌套形状**：形状文本框里再嵌的绘图照样成框（只读、不占序号），锚定用外层绘图的。
- **两处分类修正**：`pict_kind` 的优先级按 TS 决策树（文本框 / WordArt → 图片 → 隐藏 → 细横线）
  而不是文档序；`drawing_display` 加了 `eff_ns`，`wps` / `wpg` 前缀没声明时按字面量认
  （`field-display__015`）。
- **两处 TS 细节**：`imageMeta` 的每个字段取整段第一处匹配（多绘图段落里后面的不覆盖前面的），
  `w:jc` 在 `w:drawing` 这条路上是整段扫的（框里的对齐会漏上来），`w:pict` 那条才剥框。
- 另外补了 `allowOverlap="0"` 撞车时的 run 图片位移、框内表格的行 / 格 / 段落分隔（TS
  `txbxTableParas`），以及框里直接放 `w:sdt` 时整块只读。

剩下 3 处登记为已知差异：TS 没给 VML 框里的随文图片预取媒体（缺陷不跟随）、框里字段的
`instrField`（M2）、外部文本框 part（M5，见被阻塞表）。

**4.8 完成**：`diff-parse` 增加 `--scope drawing`（按**路径**过滤而不是按文档：所有用例照跑，
只计绘图域的未知差异——绘图文档同时带着 M2/M3/M5/M6 的差异，按文档过滤这道门永远关不上），
域的定义在 `compat_ts::is_drawing_path`，接进 CI（`.github/workflows/ci.yml`）。另加 `--by-doc`
列出每份文档的差异数，迭代时按份收。

`corpus/hostile` 补 4 份绘图用例（生成器在 `tools/export-golden/hostile.export.test.ts`，
跟着 `run.sh` 重生成）：

| 用例 | 病态 | 实测降级 |
| --- | --- | --- |
| `drawing-deep-groups` | 3000 层 `wpg:grpSp` 套娃 | 不爆栈；深度上限外的形状认不到，整段成 `Drawing object` |
| `drawing-cyclic-group` | `coordsize="0,0"`、坐标 `1e400`、`v:group` 与 `v:shapetype` 同 id | 组链靠下标向上走（孩子下标恒大于组），构造不出环；定不出的尺寸不给，框里的字留住 |
| `drawing-missing-rels` | `a:blip` / `v:imagedata` / `wps:txbx` 的 `r:id` 全悬空 | 一个 dataURL 都不编；原字节原样带出 |
| `drawing-bad-style` | `width:--3pt`、`margin-left:NaNpt`、`coordsize="not,numbers"`、`path="m0,0c1"`、`fillcolor="#zzzzzz"` | 认不出的值一律不给，不猜；曲线路径整条不给（画错的实心块比不画更糟） |

验收在 `tests/drawing.rs` 的 `test_09_hostile_drawing_trees_degrade_locally`：四份都要解析成功、
旁边那段正常文字一个不少、投影里没有非有限的数；未编辑保存字节不变由全语料的
`save_01_no_edit_returns_original_bytes_for_all_corpus` 覆盖。

**顺序说明**：4.1 → 4.3 → 4.4 是主链（几何与投影依赖媒体解析）；4.2 是 4.5 / 4.6 的前置（形状颜色）；
4.5 / 4.7 可与主链并行。4.6 最重，建议在 4.3 的锚定几何稳定之后再动。

## 分层决策（实现前定死）

`MOD-11` 规定显示模型里**禁止**出现由排版决定的字段。TS 的 `textboxes[]` 却带 `bandTopPx` /
`bandBottomPx` / `floating` / `outsideColumn` 这类值。两者不矛盾，但必须分层：

- **模型层**只放文档事实：EMU 原值、`relativeFrom`、`wrapText`、`behindDoc`、`relativeHeight` 原值、
  颜色的原始定义 + 解析后的 sRGB。
- **`compat_ts` 层**做 TS 投影：px 换算、`imageWrap` 的九种取值、`resolveAnchorPagePos`（用节的页宽页边距，
  仍是文档事实）、`applyAnchor` 的 band 计算、`normalizeImageZOrders`。

也就是说：TS 那些"启发式"绝大多数是 XML + 节属性的确定性函数，放在适配器里可以让绘图域 diff 归零，
同时不污染规范状态。真正的排版启发式（`extractLockedCanvas` 的溢出文本分栏 `colGeom`）属于 M6，不在 M4。

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| 表格单元格内的图片（4 份语料） | 需要 M3 的表格模型；M4 内只保证事实可解析，`table.richParas[].runs[].image` 留到 M3 |
| 页眉页脚里的图片（`headerImages`/`footerImages`，15 份） | 需要 M5 的 hf 管线复用；M4 提供 `MediaStore` 与 `ImageDisplay`，投影留到 M5 |
| `chartDisplay` / `diagramDisplay` / lockedCanvas / 墨迹 | M6 |
| `formulaDisplay`（4 处） | 公式显示模型未在 `docs/03` §12 分配里程碑，M4 不做，留待 M6 一并定 |
| 外部文本框 part（`wps:txbx/@r:txbx` → `word/txbx1.xml`，1 份） | 框的内容在另一个 part 里，`Block` 的 `NodeId` 是相对单个 DOM 的；跨 part 内容流随 M5 的页眉页脚管线一起做 |
| 保存侧 `xml.replaceImage`（1 份跳过用例） | 需要 `EDIT-06` 的 rId 分配（M2 的 2.8）；M4 完成读侧后再回头接 |

## 不在 M4

图表 / SmartArt / lockedCanvas / OLE 内容 / 墨迹的**内容**解析（M6，M4 只做 `w:object` 的 ProgID 与预览图）、
页眉页脚与节（M5）、表格（M3）、绘图的编辑操作与保存改写（M7，含 `applyImageZOrder` 的 `relativeHeight` 回写）。

## 风险提示（实现前确认）

1. **metafile 不转换**：`docs/03` §3.5 定了 EMF/WMF/EMZ/WMZ 与 TIFF 的转换不在 Rust 侧做。语料里
   4 份 `emf-image__*` 的期望值是导出工具打的占位 `data:image/png;base64,EMFPNG`，我们输出的会是
   `MediaKind::Metafile` 的原始字节 dataURL。这是**有意差异**，开工时就登记进 `KNOWN_DIFFS.md`，
   否则 4.1 的 DoD 永远差 4 处。
2. **递归**：绘图树可以嵌套很深（组里套组、文本框里套 drawing）。按 CLAUDE.md 的硬规则，遍历一律写成
   迭代；`extractTextboxes` 的组递归改成显式栈 + 深度上限，超限降级并记诊断。
3. **`topLevelDrawings` 的语义**：TS 是按字符串平衡匹配取顶层 `w:drawing`，文本框内嵌套的 drawing
   留在父片段里。我们走 DOM，要显式实现"不下钻进 `txbxContent`"这条，否则文本框里的图会被当成
   段落级图片，分类全错。
4. **MCE**：绘图是 `mc:AlternateContent` 的重灾区（`wps` vs VML Fallback）。语义遍历已剥 Fallback
   （M0 的 `xml/mce.rs`），但 TS 的某些分支恰恰读 Fallback 的 VML。逐条对照 `docs/01` §8，
   哪些走 Choice、哪些走 Fallback 要写在代码注释里。
5. **与 M2 的合并面**：M2 在 `span/`、`model/inline.rs`、`edit/ops.rs`；M4 在 `model/`（新文件）、
   `bind/compat_ts/blocks.rs`、`resolve/`、`package/`。重叠只在 `model/build.rs` 与
   `compat_ts/blocks.rs`。M4 的新代码尽量放新模块（`model/display.rs`、`model/drawing.rs`、
   `bind/compat_ts/image.rs`），在这两个文件里只留调用点，减少冲突。
