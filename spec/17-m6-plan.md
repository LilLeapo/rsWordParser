# SPEC 17 · M6 任务分解

对应 `docs/03` 第 12 节 M6 行（「图表、SmartArt、lockedCanvas、OLE、`MediaStore`」）加上前几个里程碑明确推给 M6 的
两块：公式显示模型（`spec/15`「被阻塞」表：`formulaDisplay` 未分配里程碑，留待 M6 一并定）与墨迹（`spec/13` §「不在 M2」、
`spec/15`「不在 M4」）。格式同 `spec/12`–`spec/16`：每个任务给出产出、依赖的规范条目与完成定义（DoD）。顺序即建议的
实现顺序；同一编号内的子任务可并行。排期依据（实测差距）见下文；现状数字见 `docs/05-status.md`。

M6 是 M5 之后的**串行**里程碑：M0–M5 已全部并入 `main`（bf1f906，2026-09-06），分支 `m6-embedded` 从它开出，
工作树 `../rsWordParser-m6`（本计划最初在 dcd653d 上写成，M5 并入后重定基并按新基线重新实测）。`spec/16` 是 M5 的，
所以本文件编号 17。M5 留下的东西 M6 直接站上去：页眉页脚 part 是独立内容流（`HfPart` / `AuxFlows`）、编辑位置带
`PartId`（`InlinePos.part` / `BlockPos { part, at }`）、`save/options/` 已是目录、`--scope hf` 是第六道门。

## M6 · 嵌入对象：图表、SmartArt / 画布、OLE、公式、墨迹；媒体写侧

目标：主 part 之外的**内容 part** 进模型并可编辑——图表 part（`word/charts/chartN.xml`，含 chartex 降级）、
SmartArt 的数据 / 绘图 part、绘图画布（`lc:lockedCanvas`）、`w:object` 嵌入对象、OMML 公式、墨迹批注；
`compat_ts` 输出 TS 的 `chartDisplay / diagramDisplay / formulaDisplay / runs[].math / runs[].ruby / inks[] /
extras.chartParts`；`MediaStore` 补上**写侧**（新媒体 part、`replaceImage`、编辑引起的孤儿回收）；TS `SaveBlock`
的 `kind: 'chart' / 'image'`、`xml.replaceImage` 与 `SaveOptions` 的 `inks / partXml / partBinary` 全部翻成
`EditOp`（`SAVE-07`，没有旁路）。M6 合入后保存语料的跳过清单为空——今天剩下的 20 份跳过全属本域。

**M6 门**（`spec/11` TEST-10 「M3–M6 对应域 diff 为 0」的具体化）：

1. `cargo run -p diff-parse -- --scope embedded` 0 未知差异。按**路径 + 期望块的 label** 筛（`compat_ts::is_embedded_diff(path,
   &expected)`）：路径 `blocks[*].chartDisplay*` / `diagramDisplay*` / `formulaDisplay*` / `runs[*].math*` / `runs[*].ruby*`、
   `extras.chartParts*`、`inks*` 无条件属于本域；`blocks[i].label / type / previewText / runs*` 当且仅当 `expected.blocks[i].label`
   ∈ {`Chart`, `SmartArt`, `Equation`, `Embedded object`} 或该块 `originalXml` 含 `<w:object` / `<m:oMath` 时属于本域。
   `--scope text / fields / tables / drawing / hf` 继续为 0。
2. **保存差分**：`tests/save_blocks.rs` 里被 M6 阻塞的 **61 份**用例（main 上原有 20 份：`kind:"chart"` 6 + `kind:"image"` 4 +
   `inks` 8 + `partXml` 1 + `xml.replaceImage` 1；m6.0a 扩充语料再加 41 份：chart 7 + image 7 + inks 15 + `partXml` 6 +
   `partBinary` 1 + `replaceImage` 5）全部转为「等价」或登记进 `INTENTIONAL`；跳过清单为空，`save_blocks` 断言跳过数为 0。
3. **往返**（`TEST-04` 扩到嵌入对象）：① 带墨迹保存 → 重解析，`inks[]` 的锚定段落 / 偏移（±0.1 px）/ 尺寸 / payload /
   PNG 字节全部相等，被批注的段落仍是可编辑的 `paragraph` 且 `runs` 不含墨迹；② `NewBlock::Chart` 保存 → 重解析，
   `chartDisplay` 的 kind / title / categories / series 与输入相等，图表 part、其 `.rels`、内嵌工作簿、
   `[Content_Types]` Override 都在；③ `SetChartData` 后只有被改的 `c:v` / `a:t` 文本节点脏，part 其余字节原样；
   ④ 以上每一种保存都满足 `SAVE-06`：其他 zip 条目 CRC 与压缩字节不变。
4. `corpus/hostile` 新增的 6 份嵌入对象病态输入解析成功、局部降级、无编辑保存字节相同（`TEST-09`）；
   `fuzz_embedded`（图表 part / 图示数据 / OMML 三个解析入口）10 分钟无崩溃（`TEST-08`）。
5. **全域收尾**：本域之外的 32 处零散差异（见 6.9）修掉或按路径登记，`--scope all` 归零并接进 CI（第八道门，
   `embedded` 是第七道）。

### 实测差距（2026-09-06，`main` = bf1f906，M5 已并入，`cargo run -p diff-parse -- --scope all --json`）

全域 62 处未知差异 / 29 份文档（另 217 处已登记）。嵌入对象域直接命中 **22 处 / 12 份**：

| 路径 | 差异点 / 文档 | 归属任务 |
| --- | --- | --- |
| `blocks[*].formulaDisplay` + 同块 `previewText` | 4 + 4 / 4（`math__001/004`、`insert-and-layout__001`、`protected-text-edit__002`） | 6.5 |
| `blocks[*].runs[*].math`（含把周围文字拆成独立 run） | 4 / 2（`math__002/003`） | 6.5 |
| `blocks[*].chartDisplay`、`extras.chartParts.<path>`、`previewText`（图表标题） | 1 + 1 + 1 / 1（`chart-edit__001`） | 6.1 / 6.2 |
| `previewText`：SmartArt 节点文字 | 1 / 1（`smartart-ole__001`） | 6.3 |
| `previewText`：TS **不给**、我们给 `""`（缺 part 的 Chart / SmartArt、VML 细横线） | 4 / 3（`resource-cleanup__008` ×2、`smartart-ole__005/006`） | 6.2 / 6.3 |
| OLE 与文字同段 → run 级图片（`runs[*].text` / `runs[*]`） | 2 / 1（`smartart-ole__017`） | 6.4 |

**扩充语料后（2026-09-06，m6.0a，799 份）**：两位 agent 按 `tools/export-golden/M6-CORPUS.md` 补了 226 份合成文档、46 份
保存用例、6 份 hostile（`m6-chart` 91 / `m6-chartex` 9 / `m6-smartart` 10 / `m6-canvas` 8 / `m6-omml` 41 / `m6-ruby` 5 /
`m6-ole` 8 / `m6-ink` 28 / `m6-image` 26）。`--scope embedded`（m6.0b）**420 处 / 182 份**：`m6-chart` 188、`m6-omml` 82、
`m6-ink` 43、`m6-canvas` 25、`m6-smartart` 16、`m6-chartex` 14、`m6-ruby` 14、旧语料 30（ruby 9、math 8、smartart-ole 4、
chart-edit 3、insert-and-layout / protected-text-edit / resource-cleanup 各 2）；按路径：`previewText` 94、`chartDisplay` 88、
`extras.chartParts` 80、`formulaDisplay` 41、`runs[]` 41、`inks[]` 15、`diagramDisplay` 13、`label` 11。`--scope all` 452 / 196，
本域之外 32 处 / 14 份（6.9）。五道既有的门在 799 份上仍全为 0（text 256 / fields 283 / tables 352 份，drawing / hf 按路径）。
语料扩充时发现的 TS 行为，实现时按路径登记的候选：一段两张图表 TS 只给第一张；图表 / SmartArt / 公式 / ruby 在表格单元格里
TS 整个丢掉（格投影空 `runs`，`cell.paras` 的纯文本却含公式字符）；图表 / 图示 / 画布与正文同段 TS 丢正文；锚定画布丢竖向
偏移；画布无 `wp:extent` 退化成其中的图片；新建锚定图片时 `tight-*` / `through-*` 落盘成 `wrapSquare`；墨迹锚点段在 `w:sdt`
里被静默丢弃；超链接里的公式 / ruby run 不带 `link`。

`inks` 在 main 的解析语料里没有差异（573 份的期望值全是 `[]`）；m6.0a 用 TS 保存产物补了 15 份带墨迹的解析 golden，
但语料之外的墨迹读侧仍只能靠「保存 → 重解析」
这一个 oracle（门第 3 条）。其余 40 处不属于任何未完成里程碑的域，归 6.9 收尾（清单见那里）。

**工作面按语料实测**（`corpus/synthetic`，573 份）：

| 量 | 值 | 备注 |
| --- | --- | --- |
| 带图表 part 的文档 | 1（`chart-edit__001`：`word/charts/chart1.xml`，柱状图 2 系列 3 类别） | `resource-cleanup__008` 引用 `c:chart r:id` 但 part 缺失（降级路径的正例） |
| 带 SmartArt 的文档 | 3（`smartart-ole__001` 有 data part；`__006` 缺 part；`resource-cleanup__008`） | 语料里**没有** `diagrams/drawingN.xml`，`diagramDisplay` 的期望值 0 份 |
| `lc:lockedCanvas` | 0 份 | 没有 golden 覆盖，全靠构造文档 |
| `m:oMath` | 6 份（`oMathPara` 1 份） | 4 份整段公式、2 份文字夹公式 |
| `w:object` | 14 份（`smartart-ole__*` 12、`emf-image__005`、`resource-cleanup__008`） | M4 已做块级 `oleProgId` / 预览；本域只剩「与文字同段」的 run 投影 |
| `w:ruby` | 4 份 | M2 建了 `SegmentKind::Ruby`，compat 投影没做（今天把 `rt` 文字混进正文） |
| 墨迹 | 解析侧 0 份；保存侧 3 份文档 9 个用例（1 个 no-op 已通过） | |
| 保存用例（本域） | 20 份被跳过 + 4 份已通过（`math__003/005`、`smartart-ole__007/011`：`math.omml` / `w:object` 原样重发在 1.13 就有了） | |

TS 的图表 / 公式单元测试（`chart-parse-model.test.ts` 16 例、`chart-insert.test.ts` 中 `parseChartPartXml` 4 例 +
`buildChartPartXml` 2 例、`chart-edit.test.ts` 中 `parseChartPartXml` 2 例 + `patchChartPartXml` 2 例、`math.test.ts` 中
`ommlToMathML` 7 例 + `ommlToLatex` 3 例）
直接对 XML 字符串断言，**不经过 `buildDocx`**，所以不在语料里。M6 把这些字面量搬成 Rust 夹具（见 6.1 / 6.5 DoD），
这是本域大部分行为的唯一对照。

### 保存侧差距（`cargo test -p rsword --test save_blocks -- --nocapture`）

| 跳过原因 | 用例 | 归属任务 |
| --- | --- | --- |
| `SaveBlock kind "chart"` | 6：`chart-insert__001.save.1/2`、`verify16-p1p2__001.save.1/2`、`write-protection__001.save.4`（一次保存两张图）、`resource-cleanup__006.save.1`（删图表 → 回收 part / 关系 / 工作簿 / Override） | 6.6 / 6.7 |
| `SaveOptions "partXml"` | 1：`chart-edit__001.save.1`（`patchChartPartXml` 的产物整 part 换入） | 6.6 |
| `SaveBlock kind "image"` | 4：`insert-and-layout__001.save.3`（媒体 + 关系 + `Default` 内容类型）、`.save.4`（旋转的 `effectExtent`）、`sections__013.save.1`（`posOffsetEmu` 浮动）、`write-protection__001.save.5`（`wrap` → `wp:anchor`） | 6.7 |
| `xml.replaceImage` | 1：`resource-cleanup__001.save.2`（反复替换只留最新媒体 part） | 6.7 |
| `SaveOptions "inks"` | 8：`ink__001.save.1/3/4/5/6/7`、`ink__002.save.1`（自闭合空段）、`ink__003.save.1`（非段落锚点：不留孤儿媒体） | 6.8 |

M5 关掉它的 48 份之后剩下的正是这 20 份。`*.save.json` 只记录 `documentXml`，所以图表 part、工作簿、媒体 part、
`.rels`、`[Content_Types]` 的正确性全靠 `tests/` 里的 zip 级断言（门第 3 条），TS 的 golden 不覆盖它们。

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 6.1 | **图表 part 的模型**（`model/chart.rs`）：图表 part 是**有自己 DOM 的 XML part**（L1），`ChartDisplay` 是它的投影。`Document.chart_parts: BTreeMap<PartId, ChartPart { display: Option<ChartDisplay>, chartex: bool }>`，`ProtectedBlock.display` 增 `Display::Chart(Box<ChartRef { part, extent_emu }>)`。`ChartDisplay { kind: ChartKind /* Bar \| Line \| Pie \| Area \| Scatter \| Bubble \| Other */, horizontal, grouping: Option<Stacked \| PercentStacked>, markers, hole_pct: Option<u32>, legend_pos: Option<LegendPos>, title: Option<String>, categories: Vec<String>, series: Vec<ChartSeries { name, values: Vec<Option<f64>>, color: Option<Rgb>, point_colors: Option<Vec<Option<Rgb>>>, x_values, sizes, line }>, palette: Option<[Rgb; 6]>, style_val: Option<u8> }`，规则逐条照 `docs/01` §8.5 与 TS `chart.ts`：`c:plotArea` 第一个 `*Chart` 子元素定 kind（`bar3D / line3D / pie3D / doughnut / area3D` 归并）；`c:barDir=bar` → horizontal；`grouping` 只对 bar / area 认 `stacked / percentStacked`；markers：line 看 `c:marker val=1`，scatter 看 `scatterStyle` 缺省或含 `marker`；doughnut 的 `holeSize` 缺省 50；`c:legend` 存在 → `legendPos` ∈ {b,l,r,t,tr} 否则 `r`；标题：`c:title` 内 `a:t` 拼接 → 否则 `c:v` → 否则自动标题 `Chart Title`（`autoTitleDeleted` 为真则无）→ 单系列自动标题取系列名；类别取第一个带 `c:cat` / `c:xVal` 的系列，缓存点按 `idx` 排、`ptCount` 补空，`formatCode` 含 `y` / `d` 时 Excel 序列号 → `m/d/yyyy`，`xVal` 四舍五入到 4 位；系列值 `c:val` / `c:yVal` 的 `numCache`，非数字 → `None`，没有值的系列跳过；颜色 `c:spPr/a:solidFill`（`srgbClr` / `sysClr@lastClr` / `schemeClr` 经主题）+ `lumMod / lumOff / shade / tint` **复用 `RES-05`（`resolve/drawingml.rs`）**；`c:dPt` 按 idx 稀疏；scatter / bubble 的 `xValues / sizes / line`（`scatterStyle` 含 line / smooth 且系列 `a:ln` 非 `noFill`）；调色板：`c:style` 1–48（或 `mc:AlternateContent` 里 `c14:style` 101–148 减 100），`(v-1)%8+1`：1 灰阶常量表、2 六个 accent、3–8 单色阶梯（tint 0.6 / shade 0.75 / tint 0.3 / shade 0.5 / tint 0.8）；**chartex**（`cx:chartSpace`）：`cx:chartData/cx:data` 的 `strDim / numDim` 各 `cx:lvl/cx:pt` 按 idx，系列 `layoutId` → 最近的经典 kind（`clusteredColumn / boxWhisker / waterfall / funnel` → Bar，`paretoLine` → Line，`sunburst / treemap` → Pie），`cx:title` 的 `a:t`；找不到 `cx:chartData` 时接受任何带 `cx:data` 的子元素（TS 的宽容）。没有带缓存值的系列 → `display: None` + 诊断 `CHART_NO_SERIES`；part 缺失 / 不良构 → `None` + `PKG_REL_MISSING` / `PKG_OPAQUE_PART`。EMU 与颗粒值留原值，px 换算在 compat（`MOD-11`）。`local_names.txt` 补 `c:` / `cx:` 名字 | MOD-10, MOD-11, RES-05, PKG-05 | `chart-edit__001` 的 chart part 解析出 kind=bar、2 系列、3 类别、标题「销售统计」、调色板 = 主题 6 个 accent；TS 三个测试文件里的 chart part XML 字面量搬成 `crates/rsword/tests/fixtures/chart/*.xml`（约 26 个：grouping 1、系列填充 / lumMod / dPt 3、调色板 3、scatter / bubble 4、doughnut / legend 4、chartex 1、barDir 1、自动标题 3、`buildChartPartXml` 回读 2、其余），期望值按 TS 断言手抄并标注来源行号，`tests/chart.rs` 用 `fixture_tests!` 逐个展开；`hostile` 的畸形 part 走降级。**实测（2026-09-06）**：TS 单测的 XML 字面量已随 m6.0a 进入语料（`m6-chart__*` / `m6-chartex__*`），`tests/chart.rs` 直接拿 TS golden 对照 92 份 / 93 个图表块的每个字段（全部相等，颜色连 ±1 都没有），不另建 `tests/fixtures/chart`；顺带把 `c14` 加进 MCE 已理解集合（`PKG-09`） |
| 6.2 | **图表投影与 `--scope embedded`**（`bind/compat_ts/chart.rs`）：`chartDisplay`（`partPath`、`widthPx / heightPx` = 宿主 `wp:extent` / 9525 四舍五入，颜色 hex 无 `#`，`palette` 仅在能定出时给）、`previewText` = `title`（无标题 → `""`；**没有 display 时不给 `previewText`**——TS 用 `...(x ? {} : {})` 展开，`undefined` 与 `""` 在差分里不等价，投影层统一用 `set_some!`）、`extras.chartParts[path]` = 非 chartex 图表 part 的**原字节**（UTF-8 → 字符串，不重新序列化）；R12 细化：`ChartEx` 且 `mc:Fallback` 里有能解析的图片 → `Image` 块（走 M4 的图片投影）——`ParagraphFacts.drawings[*]` 增 `fallback_picture: Option<NodeId>`，读法沿用 M4 `pict_kind` 读 Fallback VML 的方式；缺 part 的 Chart 块保持 `passthrough` + label `Chart`，无 `previewText`。`tools/diff-parse` 增 `--scope embedded`（第七档，`hf` 是第六档；筛法见门第 1 条，`is_embedded_diff(path, &expected)` 与 `is_drawing_path` / `is_hf_path` 并列，`diff.rs` 单测钉边界：图表 / 公式 / OLE run 归本域，`imageDataUrl` 仍归绘图域，`hfParts` 仍归页眉页脚域），接进 CI | COMPAT-02, COMPAT-03, MOD-05, TEST-03, TEST-10 | `chart-edit__001` 3 处、`resource-cleanup__008` 的 Chart 块 1 处归零；`--scope embedded` 在 573 份上跑通并列出本域差异；`.github/workflows/ci.yml` 多一步。**实测（2026-09-06）**：图表域全部归零（`chartDisplay` 88、`extras.chartParts` 80、图表块 `previewText`、`m6-chartex__008` 的图片块），`embedded` 420 → 214 / 90 份，`all` 452 → 246；门还没关上，CI 里用新加的 `diff-parse --max-unknown 214` 做棘轮（每个任务往下拧，6.9 归零时删参数）。`fallback_picture` 挂在 `DrawingFacts` 上（不是 `ParagraphFacts.drawings[*]` 之外的新字段），回退图的尺寸取 Choice 的 extent（`docs/04` §8）；媒体预取对 chartex 的 Fallback 放行 |
| 6.3 | **SmartArt 与绘图画布**（`model/diagram.rs`）：`DiagramDisplay { extent_emu, shapes: Vec<DiagramShape { off_emu, ext_emu, rot_60k, prst: Option<String>, fill: Option<Rgb>, line: Option<{ color: Rgb, w_emu: Option<u32> }>, picture: Option<{ media: MediaId, fill_rect }>, texts: Vec<String>, font_size_100pt: Option<u32>, text_color: Option<Rgb> }>, canvas: Option<CanvasGeom { ch_off_emu, ch_ext_emu }>, text: Option<String> /* 数据 part 的节点文字 */, anchor: Option<AnchorGeom> }`，三个来源：① **数据 part**（`dgm:relIds/@r:dm` 经主 part rels）：`dgm:pt` 去掉 `type ∈ {pres, parTrans, sibTrans}` 的，`a:t` 拼接后 trim；`dgm:cxn` 无 `type` 或 `type=parOf` 的按 `srcOrd` 建树，根 = 没有父的源点，先序遍历（**显式栈 + `seen` 集**，环不会死循环），孤立点按文件序追加，`\n` 连接；② **绘图 part**：路径由数据 part 路径 `data(\d*).xml → drawing$1.xml` 替换（TS 约定；没有关系类型可循），`dsp:sp/dsp:spPr/a:xfrm` 的 off / ext / rot、`a:prstGeom`、`a:ln`（`noFill` 除外；宽度缺省 1 px 在 compat）、`a:blipFill`（经**该 part 自己的 rels**——`MediaStore::resolve` 已按 part 解析——与 `a:fillRect`）、`a:solidFill`（`srgbClr` 直取；`schemeClr` 查主题原值**不做变换**、缺省 `9AB5E4`——TS 就是这么半解析的，照抄并注明）、`dsp:txBody` 段落文字 / 第一个 `sz` / 第一个 `srgbClr`；连线（`prst=line` 或含 `Connector`）允许零宽或零高，其他零尺寸形状丢弃；宿主尺寸取含 `dgm:relIds` 的那个 drawing 的 `wp:extent`；③ **画布**（`lc:lockedCanvas`，R14）：`a:grpSpPr/a:xfrm` 的 `chOff / chExt`（缺省 0 / 宿主 extent）给出缩放，子 `a:sp` / `a:pic` 的 xfrm / rot / `prst`（`rect` 不记）/ 图片（`a:blip` 经媒体）/ 填充（`solidFill` 或渐变等权平均，复用 `RES-05`）/ `a:txSp/a:txBody` 文字与 `sz`（**不缩放**）/ 颜色。**排版启发式全部在 compat**（`bind/compat_ts/diagram.rs`）：px 换算与缩放、`lnWPx` 缺省 1、LibreOffice 对齐的溢出文本分栏（`colGeom`：每行字符数 = `w / (fontPx × 0.72)`，行距 1.2，溢出 > 2 倍高度的文本形状从 y=0 起按半列高度错开减 23 px，单字母列拆成逐字形状并按 y 排序）、`canvas: true`、锚定画布只给 `offsetXEmu`（LO 丢竖向偏移）且 `wrapNone` → `floating`。同段其他绘图（照片 / 形状）→ `textboxes[]`（复用 M4 `box_json` 的框提取，各自的锚点），图示自己锚定时 `diagramDisplay.offsetXEmu / offsetYEmu / floating`。compat：SmartArt 块 label `SmartArt`，`previewText` = 数据 part 文字（**没有则不给**），`diagramDisplay` 有绘图 part 才给；画布块 label `Drawing object` + `diagramDisplay{canvas:true}` + `previewText` = 各形状文字 `\n` 连接；VML 细横线块（M4 `Rule`）**不给** `previewText`（`smartart-ole__005`） | MOD-05, MOD-11, RES-05, PKG-05 | `smartart-ole__001`（节点文字顺序「总经理办 / 研发部 / 产品部 / 销售部 / 运营部」）、`__005`、`__006`、`resource-cleanup__008` 的 SmartArt 块归零；**没有 golden 的部分用构造文档**（`tests/common::docx_with_parts`）：一份带 `diagrams/drawing1.xml` 的 SmartArt（4 个 `dsp:sp` 含 1 条连线、1 个图片填充、1 个 schemeClr 填充）、一份画布（`chExt` 为 extent 的 3 倍、2 个文本形状触发分栏、1 个 `a:pic`、1 个 `prst=ellipse`），期望 px 按 `docs/01` §8.5 的公式手算并写进测试注释；`dgm:cxn` 成环 / 自指 / `srcOrd` 缺失的构造用例不死循环、文字不丢；`tests/diagram.rs` 新建。**实测（2026-09-06）**：SmartArt / 画布域全部归零（23 份语料），`embedded` 214 → 170 / 70 份，`all` 202，棘轮拧到 170。与计划的三处不同：颜色在模型里是**容器节点**而不是 `Rgb`（与 M4 `FillDisplay` 同约定，`DrawingDisplay` 保住 `Eq`）；`extent_emu` 不进 `DiagramDisplay`，尺寸取宿主 `DrawingDisplay.extent`（与图表同）；`siblings` 挂在 `ProtectedBlock` 上。绘图 part 先走数据 part 的 `diagramDrawing` 关系再退路径约定；没有 `wp:extent` 的画布按 `chExt` 画（`m6-canvas__006` 登记）——两条都在 `docs/04` §8 |
| 6.4 | **OLE 与文字同段的 run 投影**（`bind/compat_ts/image.rs` 扩展）：`SegmentKind::Object`（M1 已有）在 `Text` 块里投影成 run 级图片 `{ image: { dataUrl /* v:imagedata 预览，经媒体 */, xml /* 整个 w:object 的原字节 */, widthPx / heightPx /* v:shape style 的 pt → px；缺省 w:object 的 dxaOrig / dyaOrig twips ÷ 15 */ } }`，与 M4 4.7 的块级 `oleDisplay` 共用尺寸 / ProgID 读法（`model/vml.rs` 的 `OleFacts`）；同一 run 里 `w:object` 之后的空 `w:pict` 不算第二张图（「第一个带图的 pict 赢」，`smartart-ole__017`）；预览解析不出来 → 保持 `passthrough` `Embedded object` 芯片 + `previewText` 含段落文字（TS 行为，`smartart-ole` 测试「unresolvable preview」）；`EMBED` / `LINK` 字段包着的 `w:object` 走块级 `oleDisplay`（M4 已对齐，回归测试钉住）。模型侧：`OleDisplay { prog_id, preview: Option<MediaId>, size_emu, draw_aspect, object_rel: Option<RelId> /* o:OLEObject/@r:id，内嵌二进制 */, field_form: bool }` 挂到 `Segment.display`（`MOD-11` 的 `OleDisplay` 与 TS 字段对齐）；坐标流里 `w:object` 已是 1 个 UTF-16 单位的原子（`EDIT-02`），`InsertText` / `DeleteRange` 绕过或整体删除，字节不动 | MOD-06, MOD-11, COMPAT-07, EDIT-02 | `smartart-ole__017` 2 处归零；`tests/embedded.rs`：在 OLE 前后 `InsertText` 后保存，`w:object` 子树原字节原样、`o:OLEObject` 关系不变；`DeleteRange` 跨过 OLE 原子 → 整个 `w:r` 消失、内嵌二进制 part 成为编辑引起的孤儿（6.7 回收）；`emf-image__005` 的 metafile 预览仍按 `KNOWN_DIFFS` 放行。**实测（2026-09-06）**：OLE 域归零（`m6-ole` 8 处、`smartart-ole__017` 2 处），`embedded` 170 → 160 / 67 份，`all` 188，棘轮 160。没有另立 `OleDisplay`：`Segment.display` 的 `VmlDisplay.ole`（M4）已覆盖，只补了 `draw_aspect` / `rel_id`。真正缺的是 TS 的 `splitImageRun`（同 run 多图形拆 run），补在 `blocks.rs::run_jsons`，`inline-image-mixed` 的「同 run 两张图」零散差异一并归零 |
| 6.5 | **公式与 ruby**（`model/math.rs`、`model/omml/{mathml,latex}.rs`、`bind/compat_ts/math.rs`）：`FormulaDisplay { fragments: Vec<NodeId> /* 段落里的 m:oMath，oMathPara 展开 */, tokens: Vec<String> /* m:t 文本按文档序，实体解码 */, mathml: Option<String>, latex: Option<String> }` 挂 `ProtectedBlock.display`（R11：`oMathPara` 或纯公式段）；`mathml` 只在段落**没有**可见正文时算（TS：`plainText(detect).trim() === ''`），否则不给；`latex` 转不出（子集之外）不给；`omml` = 各 `m:oMath` 片段**原字节**拼接（`lex.range`，与 `rawRPr` 同一做法）。`ommlToMathML`（TS `math.ts` 55–376）与 `ommlToLatex`（502–723）**逐字移植**——差分按字符串比较，`mn / mi / mo` 分类、运算符集、`LATEX_FUNCTIONS`、转义规则都得一样；两个转换器**迭代实现**（显式栈 + 输出帧，`omml-deep` 3000 层）。文字夹公式的段落（R19）：`Inline::Atom(Math)` → run `{ text: tokens.join(''), math: { omml } }`，前后文字拆成独立 run（`math__002`：`"see "` / 公式 / `" here"`）。**ruby**：`SegmentKind::Ruby { rt }`（M2）→ run `{ text: rubyBase 文字, ruby: { rt, xml /* 整个 w:ruby 原字节 */ } }`，`rt` 文字不进正文（今天把 `rt` 混进 `text` 是 bug）。`latexToOmml`（作者方向）**不在 M6**（M7 `InsertAtom Math`） | MOD-05, MOD-06, MOD-11, COMPAT-07 | 8 处 `formulaDisplay` / `previewText`、4 处 `runs[*].math`、9 处 `runs[*].ruby`（4 份 `ruby__*`）归零；TS `math.test.ts` 的 7 例 `ommlToMathML` + 3 例 `ommlToLatex`（含 Word 生成的带属性包的 OMML、子集外返回 `None`、特殊字符转义）搬成 `tests/math.rs` 夹具，期望字符串照抄；`fuzz_embedded` 覆盖两个转换器；`tests/embedded.rs` 在公式原子前后 `InsertText`，`m:oMath` 字节原样。**实测（2026-09-06）**：公式 / ruby 域归零（41 个 `formulaDisplay`、`runs[].math` 6、`runs[].ruby` 16），`embedded` 160 → 43 / 12 份（只剩墨迹），`all` 71，棘轮 43。`FormulaDisplay` 走 `Display::Formula`（`Display` 加第三个变体，比另开一个字段少一处 `Option`）；`omml` 原字节在投影层切；夹具与编辑用例放在 `tests/math.rs`（不是 `tests/embedded.rs`）。单元格里的公式 run 与 ruby 是 TS 的缺陷，按路径登记（`m6-omml__033` / `m6-ruby__005`）。`fuzz_embedded` 归 6.9 |
| 6.6 | **图表的保存：`SetChartData`、新建图表、整 part 替换**（`edit/chart_ops.rs`、`save/parts.rs`）：`EditOp::SetChartData { part: PartId, patch: ChartPatch { title: Option<String>, categories: Option<Vec<Option<String>>>, series: Option<Vec<Option<ChartSeriesPatch { name, values: Option<Vec<Option<f64>>> }>>> } }`——**只改缓存文本**（`spec/08`「`chart.ts` 补丁语义」）：标题 → `c:title` 里第一个 `a:t` 改、其余 `a:t` 清空，没有 `a:t` 则 `c:strCache/c:v`，两者都没有（自动标题）→ 在 `c:tx/c:rich/a:p` 的 `a:endParaRPr` 之前注入 `a:r/a:t`，`c:tx` 是无缓存 `strRef` → 整个换成 rich body，没有 `c:tx` → 插为 `c:title` 第一个子元素；系列名 → `c:ser/c:tx` 下第一个 `c:v`；值 → `c:val` 缓存点按 idx 改文本，**缺的点不补**（TS：锚不到的留着）；类别 → 每个系列的 `c:cat` 都改（缓存按系列各存一份）；chartex part → `Err(EDIT_UNSUPPORTED)`（TS 静默 no-op；我们报错，登记 `docs/04` §8）。全部走 `NodeEdit` 文本替换，所以只有被改的文本节点脏（门第 3 条 ③）。`NewBlock::Chart { data: NewChart { kind: Bar \| Line \| Pie, title, categories, series: Vec<{ name, values }> }, extent_emu: Option<(u64, u64)> /* 缺省 5486400 × 3200400 */ }`：新 part `word/charts/chart{N}.xml`（第一个空闲 N，同一事务内已分配的也算）按 TS `buildChartPartXml` 模板生成（bar / line 带 `catAx` / `valAx` 轴对 `111111111` / `222222222`，pie 无轴；`c:externalData r:id` 指工作簿），模板用 `xml::fragment` 解析成该 part 的 DOM（不拼字符串）；`[Content_Types]` Override `…drawingml.chart+xml`（`SAVE-05` 已列）；主 part 关系 `chart` 型；内嵌工作簿 `word/charts/embeddings/workbook{N}.xlsx`（最小 xlsx：`[Content_Types]` / `_rels/.rels` / `xl/workbook.xml` / `xl/_rels/workbook.xml.rels` / `xl/worksheets/sheet1.xml` / `xl/sharedStrings.xml`，A 列类别、B/C… 列系列，首行系列名——按 TS `buildChartWorkbookXlsxBase64` 的布局，用 `zip` crate 打包成二进制 part）+ `word/charts/_rels/chart{N}.xml.rels`（`package` 型关系 `rId1`）+ xlsx 的 `Default` 内容类型；绘图段落 `w:p/w:r/w:drawing/wp:inline`（`wp:extent`、`wp:docPr`、`a:graphicData uri=…/chart`、`c:chart r:id`），`wp:docPr/@id` 按 `EDIT-06`。`EditOp::ReplacePartXml { part, xml }` / `ReplacePartBytes { part, bytes }`（TS `partXml` / `partBinary`）：**part 级操作，不是旁路**——新 XML 经 `xml::fragment` 解析成该 part 的新 DOM（良构校验，根 `New`），二进制直接换字节；只允许**已存在**的 part（TS 语义），不存在 → `Err(EDIT_TARGET_MISSING)`（TS 静默忽略，登记 §8）。compat：`SaveBlock kind:"chart"` → `InsertBlock{NewBlock::Chart}`；`options.partXml / partBinary` → 对应操作 | EDIT-03, EDIT-04, EDIT-06, SAVE-05, SAVE-06, SAVE-07 | 7 份用例（chart 6 + `partXml` 1）等价；保存比较对 `wp:docPr/@id`、`pic:cNvPr/@id` 与由它派生的 `@name`（`Chart <id>` / `Picture <id>`）**容忍**（`COMPAT-09` 新增一条：TS 从 8000 / 9000 起计数、我们按 `EDIT-06` 最大值 + 1，都是分配细节）；TS `patchChartPartXml` 的 2 例 + `chart-insert.test.ts` 的 3 例标题补丁（strRef 缓存、自动标题注入 rich body）搬成 `tests/chart_ops.rs`；`NewBlock::Chart` 保存 → 重解析 `chartDisplay` 与输入相等、xlsx 能被 `zip` 重新打开且 `sheet1.xml` 单元格与数据一致；`SetChartData` 后 part 里未改的字节原样（`SAVE-08` 抽样）；**人工核对项**（非门）：生成的 docx 在 Word 里打开、图表可显示、「编辑数据」能打开工作簿。**实测（2026-09-06）**：保存语料 143 → 164 等价（chart 13 + `partXml` 7 + `partBinary` 1），跳过 61 → 40。与计划的不同：模板没有上 `part_template!` 宏——图表 part 与 xlsx 六个条目都是 `format!` 出整份文本再解析 / 打包，没有「运行时占位替换」这一步可收；`ReplacePartXml` 的良构校验就是 `Dom::parse`；`NewBlock::Chart` 在入口处 `materialize` 成 `Xml` 段落而不是给 `new_block_element` 加 `&mut EditSession`。TS 的 `chart-insert` 标题补丁三例是按 TS 源码语义构造的（那个测试文件里没有单独的 `patchChartPartXml` 用例） |
| 6.7 | **媒体写侧：新图片、`replaceImage`、资源回收**（`package/media.rs` 写侧、`edit/media_ops.rs`、`save/prune.rs`）：`MediaStore::add(pkg, bytes, mime) -> MediaId`——**相同字节只建一个 part**（内容哈希去重，TS 同），part 名 `word/media/image{N}.{ext}`（`EDIT-06`；TS 用 `aidocs{N}`，路径不进 `documentXml`，无差分影响），`[Content_Types]` 缺该扩展名的 `Default` 就补，主 part 关系 `image` 型；`NewBlock::Image(NewImage { media, extent_emu, align, wrap: Option<ImageWrap>, pos_offset_emu, z_order, rot_deg, flip_h, flip_v, para_spacing })` → 段落 + `w:r/w:drawing`：无 `wrap` → `wp:inline`（`wp:effectExtent` 按旋转外接框 `(bw - cx) / 2`），有 `wrap` → `wp:anchor`（`AnchorGeom` 的**生成方向**，`model/drawing.rs` 加 `to_anchor_xml`：positionH 相对 column、positionV 相对 paragraph、九种 `ImageWrap` → `wrapSquare/@wrapText` / `wrapTopAndBottom` / `wrapNone` + `behindDoc`、`relativeHeight` 由 `z_order`），`pPr` 的 `spacing` / `jc` 走 `plan_apply_para_props`；`EditOp::ReplaceImageMedia { drawing: NodeId, media }`（TS `xml.replaceImage`）：第一个 `a:blip` 的 `r:embed` 改指新关系（`r:link` 删掉；只有 `r:link` 时改成 `r:embed`）、删 `a:srcRect`、`a:fillRect` 属性清空、删 `asvg:svgBlip` 扩展与空 `a:extLst`。**资源回收**（`SaveOptions.prune_orphans: bool`，缺省 `true`）：保存第 1 步之后，对每个脏的内容 part 收集 DOM 里仍被引用的 rId（`r:embed / r:link / r:id / r:dm / r:lo / r:qs / r:cs / r:pict / o:OLEObject@r:id`），只删**本次会话让引用数归零**的关系（TS `DOCUMENT_OWNED_REL_TYPES`：image / chart / diagram × 4 / hyperlink / oleObject），目标 part 在整个包里再无引用时删 part 并**递归其子图**（图表 → 它的 `.rels` → 工作簿；图示 → data / layout / quickStyle / colors / drawing），连带 `[Content_Types]` 的 Override；`Default` 不动；**原本就是孤儿的 part 一个字节不动**（文件是真相；TS 会把它们也删掉——登记 §8「有意不同」）。compat：`kind:"image"` → `InsertBlock{NewBlock::Image}`（`base64` 解码进 `MediaStore::add`）；`xml` 块的 `replaceImage` → 先 `InsertBlock{Xml}` 再对新块 `ReplaceImageMedia` | PKG-05, MOD-11, EDIT-04, EDIT-06, SAVE-05, SAVE-06 | 5 份用例（image 4 + `replaceImage` 1）等价；`tests/media_ops.rs` **zip 级**断言：`resource-cleanup__001.save.2` 反复替换后包里只剩最新一份媒体 part、旧关系消失；`resource-cleanup__006.save.1` 删图表后 chart part / 其 `.rels` / 工作簿 / Override 全部消失而其他条目 CRC 不变；同一字节插两次 → 一个媒体 part 两个 run；删掉带图段落 → 媒体 part 回收，但另一个段落（或页眉 part）仍引用时保留；预先存在的孤儿 part 保存后仍在；`hostile`：`replaceImage` 目标没有 `a:blip` → 不动 + 诊断。**实测（2026-09-06）**：保存语料 164 → 181 等价（image 11 + `replaceImage` 6），跳过 40 → 23（全是墨迹）。与计划的不同：`MediaStore::add` 落在 `EditSession::add_media`（去重表是会话状态、要随事务回滚；`MediaStore` 保持只读），part 名 `image{N}`；没有 `to_anchor_xml` / `part_template!` / `owned_rel_types!`——锚定子树是一个 `format!` 模板（九种 `wrap` 只是「对齐字串 + 绕排元素 + behindDoc」三列的匹配表），关系类型表是一个 `const` 切片、属性扫描按 `r:` 命名空间 + `rId` 前缀兜底（TS 同），没有同形重复可收；`pPr` 的 `spacing` / `jc` 直接写在新段落模板里（整棵 `New` 子树，无旧容器可合并，`docs/04` §8）；回收只动本次会话造成的孤儿（TS 把原本就孤儿的也删，§8）；`referenced_rids` 跳过 `Deleted` 子树但**不**跳过不活跃的 `mc:Fallback`；`replaceImage` 作为第二批 `ReplaceImageMedia` 在 `InsertBlock` 之后按新块里第一个 `a:blip` 定位。`tests/media_ops.rs` 六个（zip 级），`tests/embedded.rs` 的 OLE 删除用例改为断言二进制 part 被回收 |
| 6.8 | **墨迹**（`model/ink.rs`、`edit/ink_ops.rs`、`save/options` 的 `inks`）：读侧 `Document.inks: Vec<InkInfo { para: NodeId, run: NodeId, offset_emu: (i64, i64) /* positionH / positionV 的 posOffset */, extent_emu, media: Option<MediaId>, payload: Option<String> /* wp:docPr/@descr 实体解码 */ }>`，判据 `DrawingFacts.is_ink`（`wp:docPr/@name` 以 `aidocs-ink` 开头，M1 已有）；墨迹 run **对分类与坐标流不可见**（TS 在 `detect` 前 `stripInkRuns`：被批注的段落仍是可编辑正文，不是图片块 / 绘图对象）——`ParagraphFacts.drawings` 剔除墨迹，`Run.segments` 里它是长度 0 的 `SegmentKind::Ink`，`runs[]` 投影跳过；compat `inks[] { anchorIndex: docxIndex, offsetXPx / offsetYPx / widthPx / heightPx = EMU / 9525 **不取整**（TS 浮点），dataUrl, payload }`。写侧 `SaveOptions.inks: Option<Vec<NewInk { anchor: BlockPos, png: Vec<u8>, extent_px, offset_px, payload }>>`（权威列表，与 `comments` 同语义）→ `EditOp::RemoveInks`（删主 part 里全部墨迹 run；它们的媒体与关系随 6.7 回收）+ 每条一个 `EditOp::InsertInk { para, ink }`：run 追加在段落**全部内容之后**（自闭合 `<w:p/>` 展开），子树按 TS `anchoredInkRunXml` 模板（`wp:anchor` `positionH relativeFrom=column` / `positionV relativeFrom=paragraph`、`wp:wrapNone`、`behindDoc=0`、`relativeHeight = 251658240 + id`、`wp:docPr name="aidocs-ink {id}" descr=payload`、`pic:pic` + `a:blip r:embed`），`docPr/@id` 按 `EDIT-06`，媒体走 `MediaStore::add`（PNG；TS 的 `aidocsink{N}.png` 前缀只是 TS 自己找「我们的」媒体的手段，我们用 `docPr` 名字定位，不依赖文件名）；锚点不是段落（表格 / sdt 外壳）→ **plan 阶段**跳过 + 诊断，不分配媒体也不分配关系（`ink__003`）；`inks: Some([])` → 只删；`None` → 不动。compat：`blockIndex` 是 `finalBlocks` 的下标，在块序列操作全部定下之后再解析成 `BlockPos`（TS 逐个最终块注入） | MOD-06, MOD-11, EDIT-03, EDIT-06, SAVE-05, SAVE-07, COMPAT-02 | 8 份用例等价；门第 3 条 ①（`ink__001.save.3/4` 的语义）在 `tests/ink.rs` 里跑通；重复保存不累积媒体 / 关系、换锚点时旧 run 删、新 run 只有一个（`save.6/7`）；`inks` 段落上 `InsertText` 的偏移与 TS `runs` 一致（墨迹长度 0）；`hostile` 的 `ink-garbage`：`r:embed` 悬空 → `media: None` / `dataUrl: null`，`descr` 里的 `&quot;` / `&amp;` 解码正确，`posOffset` 非数字 → 0。**实测（2026-09-06）**：`--scope embedded` 43 → **0**（CI 第七步去掉 `--max-unknown`），保存语料 181 → 204 等价、跳过 23 → 0，`all` 71 → 28 / 11 份（全是 6.9 的零散项）。与计划的不同：`InkInfo` 存 `rel_id` 而不是 `MediaId`（模型层不解析媒体，compat 经主 part 媒体表出 `dataUrl`）并多记 `run` / `drawing` 节点（`RemoveInks` 直接删 run）；`SaveOptions.inks` 的条目是 `InkSave { para: NodeId, ink }` 而不是 `BlockPos`（锚点就是一个段落节点）；墨迹媒体**不去重**（每条一个 part，TS 同——`m6-ink__003/024` 同段两条的 `r:embed` 才能对上，`docs/04` §8）；判据不要求 `<w:r><w:drawing>` 紧邻（§8）；锚点不是段落的检查放在 `InsertInk` 里（跳过 + 诊断，媒体分配之前），compat 不预先过滤；`collect_inks` 在 `rebuild` 与 `refresh_blocks` 都全量重算（后者本来就全量重算节与两个索引）；保存比较新增一条容忍：墨迹锚的 `relativeHeight`（由 `docPr/@id` 派生），`CanonOptions.ignore_attr` 因此拿节点。`tests/ink.rs` 六个（含 `hostile/ink-garbage`），`tests/model.rs` 的 `m6-ink__` 块类型放行已删 |
| 6.9 | **恶意输入、fuzz、全域收尾与 M6 门**：`corpus/hostile` 补 6 份（生成器进 `tools/export-golden/hostile.export.test.ts`）：`chart-part-malformed`（chart part 标签不闭合 → `Protected(Chart)` 无 display、`PKG_OPAQUE_PART`）、`chart-missing-rel`（`c:chart r:id` 悬空 + `cx:chart` 无 Fallback）、`diagram-cyclic-cxn`（`dgm:cxn` A→B→A、自指、`srcOrd` 缺失，5,000 个点）、`canvas-degenerate`（`chExt` 0 / 负数、坐标 `1e30`、`sz=-5`、`a:pic` 无 blip）、`omml-deep`（3,000 层 `m:f` 套娃 → 转换器不爆栈、超深度截断并诊断 `MOD_TOO_DEEP`）、`ink-garbage`（见 6.8）。`fuzz/fuzz_targets/fuzz_embedded.rs`：随机字节喂 `parse_chart_part` / `diagram_text` / `omml_to_mathml` / `omml_to_latex`（10 分钟；进 `fuzz.yml`）。**随机序列**（`tests/embedded_ops.rs`）：`SetChartData` / `InsertBlock{Chart}` / `InsertBlock{Image}` / `ReplaceImageMedia` / `InsertInk` / `RemoveInks` / `DeleteBlock` 混合 100 步 × 10 份带图片或图表的语料，每步 `refresh == rebuild`、`SAVE-02` 无 `EngineInvariantViolation`，每 20 步保存 + 重解析 + 检查包里没有悬空关系与孤儿新 part。**全域收尾**（32 处 / 14 份，不属于本域也没有别的归属，能修的修、TS 缺陷的按路径登记；ruby 已随 6.5 归本域）：`inline-image-mixed__009/010` 3（同一 `w:r` 里两个 `w:drawing` → 两个 run 图片，M4 漏项）、`__002/003` 2（`format.pageBreakBefore`）；`hostile-input__005` 4（`TooDeep` 块 TS 给 `passthrough` + label `Paragraph` + `previewText`，我们给 `runs: []`）；`wordart-vml__006` 1（隐藏判定 vs `Drawing object`）、`__012/013` 2（WordArt 填充色 → run `color`）；`vml-textbox__008` 1（字段 label）；`out-of-run-breaks__004` 1（`fieldDisplay.pageBreak`）；`shape-extraction__014` 4（`mc:Choice Requires="wps"` 前缀未声明 → 我们进 Fallback，与 `cell-anchored-boxes__002/003` / `hf-images__011` 同因，**登记**）；`smartart-ole__005` 1（VML 细横线块 TS 不给 `previewText`，随 6.3 的 `set_some!`）；`write-protection__004` 6 与 `extra__mixed-flavor` 7（TS 装载时把主 part 改写成规范前缀 / Transitional，`originalXml` / `internal.*` / `extras.elements[*]` 的差异**按路径登记**，与已登记的 `extra__strict-minimal` 同因）。CI 加 `diff-parse --scope embedded` 与 `--scope all`；`docs/05` 全部数字更新 | TEST-07, TEST-08, TEST-09, TEST-10 | 6 份 hostile：解析成功、诊断分别为 `PKG_OPAQUE_PART` / `PKG_REL_MISSING` / （不死循环）/ `MOD_BAD_GEOMETRY` / `MOD_TOO_DEEP` / （见 6.8），无编辑保存字节相同；fuzz 10 分钟无崩溃；100 × 10 无失败，失败用例最小化后固化；32 处零散差异归零或登记；`--scope embedded` 与 `--scope all` 都进 CI 且绿。**实测（2026-09-06）**：六份 hostile 在 m6.0a 已进语料，各域的降级断言随 6.1–6.8 写在 `tests/chart.rs` / `diagram.rs` / `math.rs` / `ink.rs`，6.9 补的是共同底线（无编辑保存字节相同、无引擎不变式破坏，`tests/embedded.rs`）；`fuzz_embedded` 建好、本地 60 秒无崩溃，10 分钟在 `fuzz.yml` 每周跑；随机序列 5 份图表 + 5 份图片语料 × 100 步：633 次生效、0 次被拒、36 次保存，孤儿 / 悬空检查与源文档基线比（只许不新增）。全域收尾时的 28 处（6.8 后的数字）：修 12 处——内联过深整段 `TooDeep`（`docs/04` §8）、图片块前的分页 → `pageBreakBefore`、文本框宿主分页 → `fieldDisplay`、`fldSimple` 参与字段标签、仅 shapetype 的 VML → `Drawing object`、`w14:textFill` 取色（`drawingml` 颜色解析按命名空间参数化、带前缀的 `w14:val` 也认）；登记 17 处——`extra__mixed-flavor` 7、`write-protection__004` 6（`x:` 前缀，TS 改写成 `w:`）、`shape-extraction__014` 4。计划里点名的 `inline-image-mixed__009/010` 同 run 两图与 `smartart-ole__005` 早在 6.4 / 6.3 归零。`--scope all` 进 CI（第八步）。M6 门五条全部通过（`docs/04` §15 的表） |

**顺序说明**：6.1 → 6.2 是读侧主链（投影要 part 模型），6.2 一落地就先把 `--scope embedded` 跑起来，让数字有地方掉。
6.3 / 6.4 / 6.5 三条读侧任务互不依赖，可与 6.6–6.8 并行。写侧 6.7 的 `MediaStore::add` 是 6.6（工作簿是二进制 part）
与 6.8（PNG）的前置，所以 6.7 的**媒体分配**这一半先做（第一个提交），`replaceImage` 与回收随后；6.6 的
`ReplacePartXml / Bytes` 很小，先做掉能立刻收 `chart-edit__001.save.1`。6.9 收尾。

## 分层决策（实现前定死）

1. **图表 part 是有 DOM 的 XML part，`ChartDisplay` 是投影**。`SetChartData` 通过 `NodeEdit` 改文本节点，序列化走 L1
   的字节级局部重写——未改的字节按构造保持原样，比 TS 的字符串切片更强；`extras.chartParts` 给原字节只是为 compat，
   M9 随 `compat_ts` 一起删。`Document::rebuild` 解析被引用的图表 / 图示 part（都很小；`refresh` 时只在 part 脏了才重算）。
2. **MathML / LaTeX 是纯函数的派生串，放 `model/omml/`**，不是排版；差分按字符串逐字比较，所以**逐字移植** TS 的算法，
   任何「改进」都会变成差异——要改就先登记，不然不改。`latexToOmml` 是作者方向，归 M7。
3. **画布的溢出分栏是排版启发式，只在 `compat_ts`**（`MOD-11` 禁止进模型）；模型只存缩放前的 EMU 与 `chOff / chExt`。
   SmartArt 的 `schemeClr` 查主题原值不做变换、缺省 `9AB5E4`，是 TS 的半解析——照抄在 compat，模型里颜色仍走 `RES-05`。
4. **`MediaStore` 的写侧仍以 `MediaId` 为界**：模型只引用 `MediaId`，dataURL 只在 compat；新 part 追加在 zip 末尾（`SAVE-06`）；
   相同字节去重。
5. **`partXml / partBinary` 是 part 级操作，不是旁路**：新 XML 必须良构（解析成 DOM），只能替换已存在的 part；
   路径不存在报错而不是静默忽略（比 TS 严，登记）。
6. **资源回收只回收本次会话造成的孤儿**。判定「本次会话」= 事务开始时该关系被 DOM 引用、保存时不再被引用；原本就孤儿的
   part 一个字节不动。TS 每次保存都按可达性删 document-owned 类型的孤儿——差异登记 `docs/04` §8（我们不误删用户包里
   我们不认识的东西）。
7. **墨迹是批注层**：run 在 DOM 里就是普通节点（未编辑时字节原样），但对分类、坐标流、`runs[]` 不可见；`inks` 选项是
   权威列表（与 `comments` 同语义）。识别只靠 `wp:docPr/@name` 前缀（与 TS 同一约定，`docs/01` §10），不靠媒体文件名。
8. **保存比较容忍分配细节**：`wp:docPr/@id`、`pic:cNvPr/@id` 及由它们派生的 `@name`（`Chart N` / `Picture N`）在
   `tests/save_blocks.rs` 的规范化比较里忽略（`COMPAT-09` 加一条），与既有的 `w14:paraId` / `w:rsid*` / `xml:space` 同类；
   不为了逐字节等价去抄 TS 的 8000 / 9000 计数器。
9. **`previewText` 的有无是语义**：TS 用展开语法只在有值时给字段，`undefined` 与 `""` 在差分里不等价。投影层一律
   `set_some!`，不再用 `set(&mut o, "previewText", "")` 兜底。

## 实现约定：多用声明宏（用户要求，2026-09-05；与 `spec/14` / `spec/16` 同一条）

嵌入对象域的样板比页眉页脚还「矩阵」：图表有四种数值缓存容器、六种图形字段、两套（`c:` / `cx:`）几乎同形的读法；
MathML 与 LaTeX 转换各有二十来种 OMML 元素、绝大多数都是「取几个槽位 → 包一层标签」；投影层每个 display 又是十来个
`Option` 字段。**判断标准仍是同一形状重复三次以上就收成 `macro_rules!`**；写法沿用 CLAUDE.md：宏带文档注释与 ```ignore
用例，跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径，会把函数定义藏起来、让人跳不到声明处的
用共享模块而不是宏。每个任务开工前先列出它的重复形状，宏与第一处使用同一个提交落地。

**沿用已有的**：`bind/compat_ts/json.rs` 的 `set_some!` / `set_if!`（投影层所有新字段；`previewText` 的有无靠它）、
`model/macros.rs` 的 `named_enum!`（`ChartKind` / `ChartGrouping` / `LegendPos` / `DiagramPointType` / `ImageWrap` 的
`as_str` 就是 TS 字面值）、M3 的 `boxed_reader!`（`ChartDisplay` / `DiagramDisplay` 这类大结构的装箱读取）、
M2 `FLD-06` 的关键字表宏（`EMBED` / `LINK` 已在）、M5 的 `xpath_asserts!`（`tests/common`，`TEST-05` 风格）与
`fixture_tests!`（今天在 `tests/resolve_fixtures.rs` 里，6.1 第一次共用时挪到 `tests/common`）、`patch_some!` /
`settings_flag!`（`save/options/`）。

**预期新增**（按任务）：

| 宏 | 收的形状 | 任务 |
| --- | --- | --- |
| `chart_kinds!` | 元素名 → `ChartKind` 的两张表（`c:barChart` 等 11 项、chartex `layoutId` 7 项），同时展开 `from_name` 与单测里的穷举 | 6.1 |
| `num_cache!` / `str_cache!` | `c:val` / `c:yVal` / `c:xVal` / `c:bubbleSize` 四个数值缓存容器与 `c:cat` / `c:tx` 两个文本缓存容器的同一读法（`strRef \| numRef → strCache \| numCache → pt[idx]/v`，`ptCount` 补空，`strLit / numLit` 字面量） | 6.1 |
| `color_mods!` | `lumMod / lumOff / shade / tint / alpha` 子元素 → 变换枢轴表（M4 `resolve/drawingml.rs` 若已有同形表则扩它，不另造） | 6.1 / 6.3 |
| `cache_edit!` | `SetChartData` 里「定位容器 → 按 idx 找 `c:v` → 文本替换或跳过」在标题 / 系列名 / 值 / 类别四处同形 | 6.6 |
| `mml_slots!` | OMML 元素 → MathML 的「取槽位、包标签」：`f → mfrac(num, den)`、`rad → msqrt \| mroot`、`sSup → msup(e, sup)`、`sSub`、`sSubSup`、`d → mrow(开, e…, 闭)`、`nary → munderover`、`limLow / limUpp`、`acc → mover`、`bar`、`box`、`groupChr`、`func`、`m → mtable`、`eqArr` 共二十来种；每项一行「元素名, 槽位列表, 输出模板」 | 6.5 |
| `latex_slots!` | 同一批元素的 LaTeX 模板（`\frac{}{}`、`\sqrt[]{}`、`^{}`、`_{}`、`\left … \right`、`\sum_{}^{}`…），与 `mml_slots!` 共用槽位提取 | 6.5 |
| `latex_symbols!` | 希腊字母 / 运算符 / 函数名三张映射表（TS `LATEX_FUNCTIONS` 与符号表），展开成 `phf` 风格的 `match` | 6.5 |
| `display_json!` | 「模型结构体 → TS JSON 对象」的字段级投影：`ChartDisplay`（14 个字段）、`ChartSeries`（7）、`DiagramShape`（12）、`DiagramDisplay`（7）、`InkInfo`（7）、`FormulaDisplay`（4）——每个字段一行 `key => expr` 或 `key => opt expr`，内部展开成 `set_some!` / `set_if!` | 6.2 / 6.3 / 6.5 / 6.8 |
| `part_template!` | 新 part 的 XML 模板（图表 part 的 bar / line / pie 三种、图表 `.rels`、xlsx 的六个条目、墨迹 run、新图片 run、`wp:anchor`）：编译期拼字符串常量 + 运行时占位替换，统一走 `xml::fragment` 解析，杜绝手拼 | 6.6 / 6.7 / 6.8 |
| `owned_rel_types!` | 回收范围的关系类型表（image / chart / diagramData / diagramLayout / diagramQuickStyle / diagramColors / diagramDrawing / hyperlink / oleObject）与它们引用属性的名字表（`r:embed / r:link / r:id / r:dm / r:lo / r:qs / r:cs / r:pict`），一处声明、`RelType` 匹配与属性扫描两处展开 | 6.7 |
| `fixture_tests!` | 每个 `tests/fixtures/chart/*.xml` / `math/*.xml` 一个 `#[test]`（失败信息带夹具名与 TS 来源行号） | 6.1 / 6.5 |
| `hostile_cases!` | 6 份 hostile 的「文件名 → 期望诊断 / 期望降级」表，展开成逐份测试 | 6.9 |

**不该上宏的**：`extractLockedCanvas` 的分栏启发式（一处、有分支逻辑）、`retargetImageBlip` 的五步替换（一处）、
`dgm:cxn` 建树（一处）。这些写成普通函数。

其他约定与前几个里程碑相同：**树遍历写成迭代**（`omml-deep` 3,000 层、`diagram-cyclic-cxn` 5,000 点；两个转换器用显式栈 +
输出帧）；**属性容器只走 `plan_apply_*`**（新图片段落的 `pPr` 用 `plan_apply_para_props`；图表 part 不是属性表，
它的编辑是文本节点替换）；**一个任务一个提交** `m6.<n>: 英文摘要 (SPEC-ID…)`，提交前同步 `docs/04` §15 勾选、§8 偏差表、
`docs/05` 数字。

## 从 M0–M4 带过来的债（M6 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `AtomKind::Math` 注释「`FormulaDisplay` 在 M3」 | `model/inline.rs` | 6.5 |
| `r12_chart` 注释「ChartEx 且有 Fallback 图 → Image 的判定 … 随显示模型补」 | `model/classify.rs` | 6.2 |
| `compat_ts/mod.rs` 的占位 `inks: []`、`extras.chartParts: {}` | `bind/compat_ts/mod.rs` | 6.2 / 6.8 |
| `save_blocks.rs` 对 `chart` / `image` / `replaceImage` 的 `Err(EDIT_UNSUPPORTED)`（消息还写着「在 M3」） | `bind/compat_ts/save_blocks.rs` | 6.6 / 6.7 |
| `diff.rs` 注释「公式与 ruby 仍不在（M6 / 后续里程碑）」 | `bind/compat_ts/diff.rs` | 6.5 |
| ruby 的 `rt` 文字混进正文 `text` | `bind/compat_ts/blocks.rs` | 6.5 |
| `docs/05`「明确未实现」：图表 part 新建、图表 / SmartArt / lockedCanvas / 墨迹的内容、`replaceImage` | `docs/05-status.md` | 6.6 / 6.3 / 6.8 / 6.7 |
| `spec/06` 第 31 行把 display 载荷写在 `ProtectedKind` 变体里（`Chart(ChartDisplay)`），实现从 M4 起放 `ProtectedBlock.display` | `spec/06-model.md` | 6.1（改规范措辞，`docs/04` §8 已有 M4 那条） |
| `spec/08` 的 `SetChartData` 只有一句「`chart.ts` 补丁语义」 | `spec/08-edit.md` | 6.6（补条目与验收行） |
| `SAVE-07` 的 `SaveOptions` 列表没有 `inks / partXml / partBinary / prune_orphans` | `spec/09-save.md` | 6.6 / 6.7 / 6.8 |

## 基线与复用

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M4 `resolve/drawingml.rs` | 颜色算法（图表填充 / 调色板阶梯 / 画布填充 / 渐变平均） | 6.1 / 6.3 |
| M4 `model/units.rs` | EMU ↔ px / pt / twips 换算 | 6.2 / 6.3 / 6.4 / 6.8 |
| M4 `package/media.rs` | `MediaStore::resolve` 按 part 自己的 rels（图示绘图 part 的图片、画布图片、OLE 预览）；写侧在同一文件加 | 6.3 / 6.4 / 6.7 |
| M4 `model/drawing.rs` | `AnchorGeom`（墨迹与新图片的锚定几何；本里程碑加生成方向 `to_anchor_xml`） | 6.7 / 6.8 |
| M4 `bind/compat_ts/box_json.rs` | SmartArt 同段其他绘图的 `textboxes[]` 提取 | 6.3 |
| M4 `model/vml.rs` | `OleFacts`（ProgID、`v:shape style` 尺寸、`dxaOrig / dyaOrig`） | 6.4 |
| M4 `compat_ts/diff.rs` | `is_drawing_path` 的筛法与边界单测，`is_embedded_diff` 并列 | 6.2 |
| M2 `xml/fragment.rs`、`EDIT-06` | 模板解析；rId / `docPr` id / 新 part 名分配 | 6.6 / 6.7 / 6.8 |
| M2 2.6 `SAVE-05` | 新建 part + 关系 + Override 的机制（批注 / 注释 part 那套），扩到图表 / 工作簿 / 媒体 / 图表 `.rels` | 6.6 / 6.7 |
| M1 1.13 `save_blocks.rs` | `math.omml` / `ruby.xml` / `image.xml` 的 `parse_fragment` 重发（已在） | 6.5（读侧对上就行） |
| M3 `model/table.rs` | `boxed_reader!` | 6.1 / 6.3 |
| M1 `tests/common::docx_with_parts` | 构造带额外 part 的文档（图示绘图 part、画布、墨迹） | 6.3 / 6.8 / 6.9 |
| M5 `edit/section_ops.rs`、`InlinePos.part` / `BlockPos { part, at }` | 带 part 的位置：`InsertInk` / `ReplaceImageMedia` / `SetChartData` 的定位形状照抄 | 6.6 / 6.7 / 6.8 |
| M5 `save/options/{mod,section,settings,hf,decl}.rs` | 目录形状与「选项 → 操作 / `plan_all`」两条路；M6 加 `embedded.rs` / `media.rs` | 6.6 / 6.7 / 6.8 |
| M5 `model/hf.rs` / `model/aux.rs` | 辅助 part 的内容流；图表 / 图示 part 的「有自己 DOM 的 part」按同一模式登记到 `Document` | 6.1 / 6.3 |
| M5 `compat_ts/hf.rs` 的 `Ctx::for_aux` / `Ctx::switch` | 投影时切换到另一个 part 的 DOM（图示绘图 part 的图片、图表 part） | 6.2 / 6.3 |
| M5 `tests/common::xpath_asserts!`、`tools/gen-fixtures` | XPath 断言；构造最小 docx 的生成器（画布 / 图示 fixture 可复用其骨架） | 6.6–6.9 |

`--scope` 今天六档（`text` / `fields` / `tables` 按文档，`drawing` / `hf` 按路径，`all`），M6 加 `embedded`
（按路径 + 期望块 label），收尾后 `all` 也进 CI。

## 从 M5 直接接过来的接口（没有并行分支，不再有合并面）

| 接口 | 用在哪 |
| --- | --- |
| `HfPart` / `AuxFlows`（辅助 part 的 `Vec<Block>` + 三份索引） | 6.1 / 6.3 的图表 / 图示 part 按同一「有自己 DOM 的 part」模式登记到 `Document`；页眉里的图表 / 墨迹读侧自然可用，投影与写侧仍只做主 part（见「依赖与被阻塞」） |
| `InlinePos.part` / `BlockPos { part, at }`、`part_or_main` / `dom_in` | 6.6–6.8 的新操作全部按 part 定位，不再有「只认主 part」的路径 |
| `save/options/` 目录、`plan_all` 与 `apply_all` 两条路 | `inks` / `partXml` / `partBinary` 走 `apply_all`（它们是编辑操作）；`prune_orphans` 在保存第 1 步之后（不是选项，是 `SaveOptions` 的开关） |
| `Scope::Hf` + `is_hf_path`、`diff.rs` 的边界单测 | `Scope::Embedded` 照抄；`main.rs` 的 `retain` 闭包改成带 `&expected`，`hf` / `drawing` 两档不受影响 |
| `hf_slots!` / `patch_some!` / `settings_flag!` / `crypt_attrs!` / `toggle_fields!` / `xpath_asserts!` / `fixture_tests!` | 见「实现约定」 |
| `tools/gen-fixtures`（生成最小 docx 的 workspace 成员） | 6.3 / 6.9 构造画布 / 图示 / 病态输入时复用其骨架，不再手拼 zip |

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| 页眉页脚 part 里的图表 / 墨迹 / OLE | 读侧模型随 `HfPart` 的 `build_container` 自然建出；TS 的 `hfImages` 不输出图表 / 墨迹，所以投影没有对照，写侧（`InsertInk` 到页眉、页眉里的图表数据）不在 M6 门里，能做则做 |
| `latexToOmml`、`InsertAtom Math`、公式 token 编辑（`patchMathTokens`） | 作者方向，M7 |
| 图表 / 图片 / 墨迹操作的修订生成（`track_changes`） | M7 |
| `applyImageZOrder` 的 `relativeHeight` 回写、既有图片的裁剪 / 缩放 / 换 wrap 操作 | M7（M6 只做 `replaceImage` 与新图片） |
| **人工核对**：生成的图表 docx 在 Word 里打开、图表可见、「编辑数据」能打开工作簿；带墨迹的 docx 在 Word 里显示为浮动图片 | 需要真实 Word，项目负责人做；不作为门，作为 6.6 / 6.8 的检查清单项 |
| **真实 Word 语料**（Word 写出的图表 / SmartArt 绘图 part / 画布 / 公式属性包 / 原生墨迹 / SVG 图片等形态） | 清单与做法在 `docs/07-real-word-corpus.md`；放 `corpus/real/`，接入由 6.1 前完成（`real.export.test.ts`、`diff-parse --corpus`、往返与编辑保真扫描）。往返字节相同与 CRC 不变对它们**是**门，TS 差分只作参考 |
| 语料在本域极薄（图表 1 份、SmartArt 绘图 part 0 份、画布 0 份、chartex 0 份、墨迹解析侧 0 份） | 行为正确性主要靠移植 TS 单测夹具与构造文档；**可选**：往 genoffice 的 `tests/` 加带绘图 part 的 SmartArt / 画布 / chartex 用例后重导语料（`TEST-02`，改期望值的唯一合法途径），列为可选项不作为门 |

## 不在 M6

- **图表的渲染与布局**、SmartArt 的布局引擎（只读 Word 预算好的绘图 part）、OLE 内嵌二进制的内容解析（`embeddings/*.bin`
  原字节不动）。
- **metafile / TIFF 转换**：`docs/03` §3.5 冻结不在 Rust 侧做，`emf-image__*` 的已知差异保留（`docs/01` §13.7 把 metafile
  写进 M6 行，那是 TS 的视角）。
- **公式的作者方向**（`latexToOmml` / `mathParagraphXml` / token 编辑）与 `InsertAtom Math`——M7。
- **既有图片的编辑**（裁剪、缩放、换 wrap、z-order 回写）与绘图形状的编辑——M7；M6 只做新图片、`replaceImage`、墨迹。
- **修订生成**——M7。
- **页眉页脚 part 里的嵌入对象**——随 M5 合并后的跨 part 流。
- `extras.chartParts` 与 `runs[].image.xml` 这类「原字节直出」的 compat 形态——M9 随 `compat_ts` 删。

## 风险提示（实现前确认）

1. **MathML / LaTeX 的逐字等价**：差分比的是字符串。语料只有 4 个公式 + 2 个行内公式，其余全靠移植 TS 的 11 个单测；
   实体、空白、`mn / mi / mo` 分类边界（`OPERATOR_CHARS` 集合）、`\left \right`、矩阵分隔符都要一样。要改进就登记，
   否则不改。
2. **图表颜色的 ±1**：TS `lumHex` 走 HSL 与四舍五入，M4 4.2 的 `RES-05` 按 Word 校到 ±1/255；调色板阶梯（tint / shade
   连乘）可能差 1。先用 `RES-05`，出现差异**按路径登记、Word 为准**，不为 TS 抄第二套颜色公式。
3. **chartex 的 Fallback 图片**：语义遍历剥 `mc:Fallback`，R12 细化要专门读 Fallback 里的 `w:pict / w:drawing`——与 M4
   `pict_kind` 读 Fallback VML 是同一种「例外读法」，集中在 `facts.rs` 一处，注释写明是 TS 决策树的对应分支。
4. **xlsx 是 zip 套 zip**：工作簿对我们是二进制 part，生成用 `zip` crate 写内存流；`PKG-02` 的限额与 `raw_copy_file` 不受
   影响。`patchChartWorkbookXlsxBase64`（改内嵌工作簿的 Sheet1）TS 有函数但保存路径靠调用方传 `partBinary`；M6 提供
   `ReplacePartBytes` 就够，**不**在 `SetChartData` 里顺手改工作簿（Word「编辑数据」看到旧数、TS 也是这样）；要做另立条目。
5. **`w:object` 原子的坐标流**：`SegmentKind::Object` 在 M1 就是 1 个单位，但含 `w:object` 且有文字的段落今天全被投影成
   `passthrough`，编辑路径没在语料上跑过——6.4 的 `InsertText` / `DeleteRange` 用例要覆盖，`TEST-04` 的 400+ 份编辑保真里
   有 14 份带 `w:object`。
6. **墨迹对坐标流「不可见」**是一条新的段落级规则（长度 0 的段），要保证 `Run.segments` 的 UTF-16 偏移与 TS 的 `runs`
   一致，否则 `inks` 段落上的 `InsertText` 位置会错一个原子。
7. **回收的「本次会话」判定**需要事务开始时的引用快照：`EditSession::open` 时对主 part 的 DOM 扫一遍 rId 引用
   （几毫秒），存 `HashSet<RelId>`；保存时再扫一遍求差。多 part（M5 合入后）按 part 各存一份。
8. **`--scope embedded` 的筛法依赖期望 JSON 的 label**：这是第一次让 scope 判定读 `expected`，`main.rs` 的 `retain`
   要传 `&expected`；别顺手改动 `drawing` 档的语义。
9. **`previewText` 的有 / 无**：今天 `set(&mut o, "previewText", "")` 在多处兜底，改成 `set_some!` 会波及非本域的块——
   先跑四道门确认没有回归再提交。
10. **`previewText` / `runs` 的连带项会先变多再变少**：图表 / 公式块一旦从 `passthrough` 换成带 display 的形态，
    同块的 `previewText` 与 `runs` 会短暂出现新差异，`--scope embedded` 的 label 判定把它们算进本域——按块一次收齐，
    不要半截数组提交（M4 4.6b 的教训）。
