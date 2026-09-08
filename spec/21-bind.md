# SPEC 21 · 原生协议（bind/native/）

> 修订：v1（`76f2542`，2026-09-08）初版评审通过，4 处意见落地（`784b96a`）。
> 修订：**v2（2026-09-08）协议语义变更**——BIND-03 的前提「`EditOp` 结构简单、无 arena 引用，
> 可直接 derive serde 并无上下文往返」被代码现实推翻：`NewElement` 经 `QName` 携带
> `NsId::Other(Interned)` / `LocalName::Other(Interned)`，而 `Interned` 是 **per-Dom** `Interner`
> 的句柄（`xml/interner.rs`：「每个 `Dom` 一张」「只在产生它的 `Interner` 内有意义」），
> 离开产生它的 Dom 既渲染不出名字、也反序列化不回来。错话源自 `spec/19`「实现约定」
> （已随 v2 订正）。v2 改为**线型与引擎型分离**（BIND-03 重写）。**v1 对 BIND-03 的评审通过
> 作废，重新评审**；BIND-01 / BIND-02 / BIND-04–11 未动，其 v1 评审结论仍然有效。

对应 `docs/03` v3.3 §12 的 M8′ 行与 `spec/19` 任务 8.1。职责：定义 rsword 独立交付的**唯一对外协议**——
`open → document / resolve / media → apply / save → close`。本文件是 M8′ 的**关口**：
评审通过之前不动 8.2–8.5 的代码；条目一经评审即冻结语义，后续只许增补。

与既有面的关系：`bind/compat_ts/`（TS `ParsedDoc` / `SaveBlock[]` 兼容投影）是**测试专用**的差分对接点
（`spec/10` 头部），不是对外接口；`crates/rsword-js` 的五函数 wasm 面是它的薄壳。本协议落在
`bind/native/`，对全部语言绑定（wasm、CLI、M9′ 的 MCP server）是同一份语言无关核心，
外壳只做类型转换（`spec/18` 7.10 确立的分层）。

`spec/19`「分层决策」是本文件条目的来源，每条注明出处。关键词按 `spec/00` §0.2。

## BIND-01 会话与生命周期（决策 1、5、6）

```
open(bytes: &[u8], opts: OpenOptions) -> Result<SessionId>     // 一份文档一个会话
close(id: SessionId)                                            // 幂等；不存在的 id 忽略
```

- **必须**：`open` 只做一次 `EditSession::open`（`EDIT-01`），成功才占表项；解析失败返回 `Err`，
  **禁止**留下半初始化的会话。会话表对调用方不透明；`SessionId` 是 JSON 字符串（`"s<N>"`，
  进程内单调分配），调用方**禁止**解析其内容或跨进程复用。
- **必须**：会话有状态、保存函数式（决策 5）——`apply` 推进会话状态；`save` 在会话当前状态的
  克隆上执行（`EDIT-05` 事务），保存失败**禁止**影响会话；调用方写盘成功后才继续编辑该会话。
- **必须**：任何导出收到不存在的 `SessionId` → `Err(BIND_NO_SESSION)`，**禁止**静默重建。
- **必须**：`apply` 失败（`Err` 或部分校验不过）会话状态与操作前逐字节一致（`EDIT-05` 延伸
  到协议层；`fuzz_bind` 的 oracle）。
- 导出全集（8.4 落地；`bind_export!` 同形收拢）：

  | 导出 | 入 → 出 | 条目 |
  | --- | --- | --- |
  | `open` / `close` | 见上 | 本条 |
  | `document` | `(id, opts?) → DocumentJson` | BIND-02 / BIND-10 |
  | `apply` | `(id, op, ctx?) → MutationResult` | BIND-03 |
  | `save` | `(id, opts?) → bytes` | BIND-04 |
  | `media` / `addMedia` | `(id, mediaId) → bytes` / `(id, bytes, mime) → MediaId` | BIND-05 |
  | `resolveRuns` / `resolveParas` / `resolveCells` / `resolveSections` / `resolveTable` | `(id, [nodeId]) → [{ value, provenance }]` | BIND-06 |
  | `partBytes` / `nodeXml` | `(id, partId) → bytes` / `(id, nodeId) → string` | BIND-09 |
  | `diagnostics` | `(id) → [Diagnostic]` | BIND-07 |
  | `version` | `() → { version, git, protocol }` | BIND-08 |

- 会话内并发：首版**不**承诺线程安全；同一 `SessionId` 的并发调用行为未定义（调用方串行化）。
- 验收：每个导出一条「不存在的 `sessionId` → `BIND_NO_SESSION`」单测（`bind_export!` 展开）；
  `save` 失败后 `document()` 与失败前逐字节相同；`open` 一份 hostile 文档失败后会话表为空。

## BIND-02 模型 JSON（决策 1、4、6、7；MOD-01–MOD-11）

`document()` 是 `Document`（`MOD-01`）经 serde 的**投影，不是第三个模型**（决策 1）。

- **必须**：JSON 字段名 = Rust 字段名的 camelCase；不为调用方方便新造语义字段（派生值问
  `resolve`）。枚举值 = `named_enum!` 的 `as_str` 字串。单位按 `spec/00` §0.4 **原值**
  （twips / half-points / eighth-points / EMU / UTF-16 code unit；旋转 1/60000 度）。
- **必须**：`Run` = 物理 `w:r`（`MOD-06`）；坐标流 UTF-16，原子 `U+FFFC` 占 1（决策 7，
  `docs/03` §8.1 已拍定不改）。偏移一律指坐标流。
- **禁止**出现：dataURL、原字节切片、TS 形态的半解析字段、排版决定的字段（`MOD-11` 的禁令
  延伸到协议）。媒体字节只经 BIND-05 的句柄取。
- **必须**：id（`nodeId` / `partId` / `spanId` / `fieldId` / `revisionId` / `mediaId`）**会话内
  稳定、跨会话无意义**（决策 6）——`nodeId` 是 arena 下标（`MOD-13`）；调用方**禁止**持久化 id，
  换会话后按块序 + 文本指纹重对齐。id 在 JSON 里一律为整数。
- **必须**：显示模型（`ChartDisplay` / `VmlDisplay` / `DiagramDisplay` / `AnchorGeom` 等，
  `MOD-11`）**不进默认 JSON**（决策 4）；`document({ display: true })` 才投影它们。
- **必须**：`document()` 的投影与 `Document::rebuild` 字段逐一对应——`MOD-01`–`MOD-11` 的
  字段清单做成 checklist 测试，投影**不丢字段**（`model_json!` 同表展开实现、schema 与
  checklist，见 `spec/19`「实现约定」）。**禁止**在 `model/` 类型上直接 `derive(Serialize)`
  （模型持 `NodeId` / 区间 / arena 引用）；独立投影层在 `bind/native/json.rs`。
- **必须**：全语料 `document()` 输出通过 JSON Schema 校验（schema 由同一张 `model_json!` 表
  生成，进 CI）；JSON → `DocumentJson` → JSON 的 serde 往返逐字节幂等。
- 顶层形态（`MOD-01` 的投影；`spans` / `fields` / `revisions` 一并投影，`warnings` 经
  BIND-07 的诊断形态）：

  ```
  DocumentJson {
    main: Block[], sections: SectionInfo[], hfParts: { [partId]: HfPart },
    footnotes: Note[], endnotes: Note[], comments: Comment[],
    styles, numbering, theme: Theme | null, fontTable, settings, sources, media: MediaEntryJson[],
    spans: SpanIndex, fields: FieldIndex, revisions: RevisionEntry[],
    warnings: DiagnosticJson[],
  }
  ```

- 验收：M8′ 门 1（全语料过 schema、serde 往返幂等、checklist 不丢字段）；门 6
  （`display` 关闭时带图语料 JSON 体积较 `compat_ts::parsed_doc` 降 ≥ 50%）。

## BIND-03 `EditOp` JSON（决策 1、10；EDIT-01–EDIT-06）—— v2 重写

- **必须**：**线型与引擎型分离**（v2 的核心更正）。引擎的 `EditOp`（`edit/mod.rs`）**禁止**
  derive serde：它经 `NewElement` 携带 `QName`，而 `QName` 可能装着 `NsId::Other(Interned)` /
  `LocalName::Other(Interned)`——`Interned` 是 **per-Dom** `Interner` 的句柄（`xml/interner.rs`：
  「每个 `Dom` 一张」「只在产生它的 `Interner` 内有意义」）。无上下文的 serde 往返对携带
  `NewElement` 的变体在原理上不成立：裸句柄离开产生它的 Dom，既渲染不出名字，也反序列化
  不回来。协议边界改在 `bind::native` 定义**线型 `EditOpJson`**，由它 derive serde
  （`#[serde(tag = "op", rename_all = "camelCase")]`——`{"op": "insertText", "at": …, "text": …}`）；
  引擎型 ↔ 线型是显式转换，不是反射。
- **必须**：转换是**上下文化**的（协议层持有会话的 Dom，表外名字经它的 `Interner` 驻留 /
  渲染）：

  ```
  edit_op_from_json(json: &str, dom: &mut Dom) -> Result<EditOp>   // 表外名字驻留进 dom 的 Interner
  edit_op_to_json(op: &EditOp, dom: &Dom) -> Result<String>        // Interned 经 Interner 渲染成名字
  ```

  签名以实现时能落地为准；形状固定为「线型文本 ↔ 引擎型，必须带 Dom 上下文」。
  `NewBlock` / `NewInline` / `NewAtom` / `NewField` / `NewComment` / `NewImage` / `NewChart` /
  `NewInk` 与属性表 `*Patch`（`PROP-06` 的 diff/patch 形态）中**不含** `NewElement` 的部分
  直接共享 serde 形态（见下「三类去处」）。
- **必须**：往返测试的定义——scratch Dom 上构造引擎型 → `to_json` → `to_string` →
  `from_str` → `from_json` → `PartialEq` 相等，60 变体各一条（`edit_op_json!` 同一张表展开
  线型、转换、测试与下文的变体清单）。同一 `Interner` 内同名同柄，故同一 scratch Dom 内
  相等成立；**跨 Dom 的句柄相等不做要求**（调用方禁止跨会话搬运操作 JSON 里的句柄——
  句柄根本不在线型里出现）。
- **必须**：`NewElement` 的**三类去处**分治（协议面共 7 处，穷举过；v1 一刀切「derive」
  正是漏在这里）：

  | 类 | 位置 | 线型 | 计数 |
  | --- | --- | --- | --- |
  | a. 真逃生口 | `NewBlock::Xml`、`NewBlock::Wrapped.wrapper`、`NewInline::Xml`、`ReplacePartXml`、`ReplacePartBytes` | XML 字符串（`ReplacePartBytes` 为 base64） | `BIND_XML_ESCAPE` 诊断 + 计数 |
  | b. 结构化属性 | `NewBlock::Paragraph.props`（`w:pPr`）、`NewRun.props`（`w:rPr`）、`NewInline::Field.props`（字段结构 run 的 `w:rPr`） | `ParaPropsPatch` / `RunPropsPatch` 的 serde 形态（`SetParaProps` / `SetRunProps` 已在用的同一族） | 不计逃生口 |
  | c. 整块替换 | `ReplaceParaProps.props`（`EDIT-04` rawPPr 语义） | XML 字符串 | `BIND_XML_ESCAPE` 诊断 + 计数 |

  - **a（真逃生口）**：`insertBlock`（`NewBlock::Xml` / `Wrapped.wrapper`）、内联片段
    （`NewInline::Xml`，`m:oMath`、带 `w:ruby` / `w:drawing` 的 run 等）与 part 整体替换
    （`ReplacePartXml` / `ReplacePartBytes`）的线型是 XML 字符串，每次使用记一条
    `BIND_XML_ESCAPE` 诊断并计数（会话级计数器随 `diagnostics` 返回）。M8′ 不要求计数为 0。
    （`NewInline::Xml` 与下表 b 的 `NewInline::Field.props` 是 v2 复核穷举时补进分类的两处，
    评审时请一并确认。**注意与 `NewField` 区分**：`NewField`（`edit/mod.rs:390`，`InsertField`
    的载荷）是另一个类型，字段只有 `instr` / `result` / `mark_dirty`，**不含** `props`，
    不在本表七处之列。）
  - **b（结构化属性）**：`NewBlock::Paragraph.props`、`NewRun.props`、`NewInline::Field.props`
    的**线型**是 `ParaPropsPatch` / `RunPropsPatch` 的 serde 形态，**不计逃生口**。理由：同一个
    概念不能在协议里有两种形态；一刀切成 XML 字符串会逼调用方手写 `w:pPr` / `w:rPr`——原生协议
    就是要消灭手写 XML，M9′ 的 Agent 尤其用不了。
  - **b 的落地方式：只在线型边界转换，`edit/` 的引擎型不动**（v2 复核时改定；原稿写的
    「8.3 把这三个位置改成持 patch」**撤销**）。`edit_op_from_json` 收到 patch 后就地物化成
    `NewElement` 再交给引擎：patch → `ParaProps` / `RunProps` → `emit_para_props` /
    `emit_run_props`（`build/props.rs` 生成，**不需要 `Dom`**，`compat_ts` 今天就在用，
    见 `save_blocks.rs:988`、`:1239`）。`edit_op_to_json` 反向读回。
    **为什么不改引擎型**：`compat_ts`（测试专用，扛着 `COMPAT-08` 的 `save_blocks` 204/208 门）
    把调用方的 `rawPPr` **原样透传**——`generated_paragraph` 用 `parse_fragment` 把整份
    `w:pPr` 变成 `NewElement` 直接塞进 `Paragraph.props`（`save_blocks.rs:900`–`:914`），
    rich run 的 `w:rPr` 同理（`:2034` 一带）。引擎型若改成 patch，凡是落在生成属性表之外的
    内容都会被丢掉，`COMPAT-08` 必然回归。只换线型则两边都成立：原生 / Agent 调用方发结构化
    patch，`compat_ts` 继续走原字节，互不干扰，8.3 的工作量也随之缩小。
    **代价与出口**：原生调用方若确实需要发属性表之外的 `w:pPr` 内容，走 a 类的
    `NewBlock::Xml`（计逃生口）——这是有名有姓的既定出口，不是缺口。
    **8.3 的 DoD 追加一条**：改完 `save_blocks` 仍为 **204/208 等价、0 跳过**（`spec/19` 门 5
    「既有门不许退」在这条上最容易破）。
  - **c（`ReplaceParaProps.props`）判定：留 XML 字符串线型，并入 a 的 `BIND_XML_ESCAPE`
    计数**（v2 评审要定的点）。理由：
    1. 这个操作的定义语义是「用调用方给的整份 `w:pPr` 替换现有容器」——它能表达 patch
       表达不了的东西：`pPrChange` 快照、`sectPr`、段落标记 `rPr`、32 张生成表之外的
       元素。patch 是合并语义，表达不了「整块换成未建模内容」；强行 patch 化要么丢语义，
       要么退化成 `SetParaProps` 的复制。
    2. 「Agent 手写不了 XML」由同协议的 `SetParaProps`（patch 形态）覆盖——结构化修改
       走它；`ReplaceParaProps` 留给确实需要容器级替换的调用方（compat 路径与外科手术式
       修 XML），这正是逃生口的定位（决策 10：有名有姓、计数、不禁止）。
    3. 与 b 不冲突：这不是「同一个概念两种形态」——merge 与 replace 是两个操作，
       线型各自反映语义。
- **必须**：位置形态与 `EDIT-02` 一致——`InlinePos { part: PartId | null, para: nodeId,
  offset }`（`part: null` = 主 part），`BlockPos { part: PartId | null, at: BlockAt }`；
  偏移是坐标流 UTF-16 code unit。
- **必须**：`EditContext` JSON 全字段可选：`{ trackChanges?: { author, date } | null,
  defaultRunProps?, keepOrphanComments?, markUpdatedFieldsDirty? }`（`EDIT-01`）。
- **必须**：`MutationResult` JSON 为 `{ created, affectedBlocks, structureChanged,
  diagnostics, offsetDelta }`（`EDIT-05`；`created` 的元素为 `nodeId | null`）。
- **必须**：变体清单 = 下表 60 个 + 8.3 依 BIND-04 新增的 5.7 族（见 BIND-04）；往返测试
  按本条上面的定义逐变体一条；同一条操作经协议 `apply`（线型进、`edit_op_from_json`
  转换）与原生 `EditSession::apply` 的保存结果**逐字节相同**（门 2）。

  **60 变体清单**（`edit/mod.rs`，与引擎侧逐一对表；线型按上面三类去处分流）。
  与 `docs/03` §8.2 冻结清单的差异共 **34 项，分两类**
  （`spec/18` 待决 5 在此收编，偏差登记 `docs/04` §8）：
  **★ = §8.2 没有的新增操作（20 个）**——分节符增删、墨迹增删、`linkHeaderFooter`、
  `regenerateBlockField`、`removeNote` / `removeSdtShell` / `setNoteContent`、part 整体替换与
  换图、绘图几何 / z 序 / 绕排 / 形状样式 / 文本框、数学 token、水印、页面底色；
  **▲ = 同名操作的形态变化（14 个）**——位置带 `part`（`replaceInlines` / `setParaProps` /
  `mergeWithNext` / `deleteBlock` / `moveBlock`）、`replaceParaProps`（`EDIT-04` rawPPr 语义）、
  `insertColumn` 带 `width`、`toggleCheckbox` 去 `checked`、`setLinkTarget` 枚举形态、
  `removeBookmark` 按 `name`、`removeComment` / `setCommentText` 按 `id`、
  `acceptAll` / `rejectAll` 带 `author` 过滤：

  | 组 | 变体（`op` 字串） |
  | --- | --- |
  | 内联 | `insertText`、`deleteRange`、`setRunProps`、`insertAtom`、`insertField`、`replaceInlines`（▲ 带 `part`） |
  | 段落 | `splitParagraph`、`mergeWithNext`（▲ 带 `part`）、`setParaProps`（▲ 带 `part`）、`replaceParaProps`（▲ `EDIT-04` rawPPr 语义） |
  | 块 | `insertBlock`、`deleteBlock`（▲ 带 `part`）、`moveBlock`（▲ `from`/`to` 带 part，跨 part 走 `XML-12` E′） |
  | 表格 | `setTableProps`、`setRowProps`、`setCellProps`、`insertRow`、`deleteRow`、`insertColumn`（▲ 带 `width`）、`deleteColumn`、`mergeCells` |
  | 字段 | `setFieldResultProps`、`toggleCheckbox`（▲ 无 `checked` 参数，语义=取反）、`setFormText`、`setLinkTarget`（▲ `LinkRef`/`LinkDest` 枚举形态）、`updateBlockField`、`regenerateBlockField`（★ 7.8 生成器） |
  | Span | `addBookmark`、`removeBookmark`（▲ 按 `name`）、`addComment`、`removeComment`（▲ 按 `id`）、`setCommentText`（▲ 按 `id`，带 `done`） |
  | 修订 | `acceptRevision`、`rejectRevision`、`acceptAll`（▲ `author` 过滤）、`rejectAll`（▲ 同） |
  | 节与页眉页脚 | `setSectionProps`、`setHeaderFooter`、`linkHeaderFooter`（★）、`setWatermark`（★）、`setPageColor`（★）、`insertSectionBreak`（★ 7.6）、`deleteSectionBreak`（★ 7.6） |
  | 声明 part | `setDocumentSettings`、`setNoteContent`（★ `endnote: bool`）、`removeNote`（★）、`setSdtContent`、`removeSdtShell`（★） |
  | 图表与 part | `setChartData`、`replacePartXml`（★）、`replacePartBytes`（★）、`replaceImageMedia`（★ 6.7） |
  | 绘图 | `setDrawingGeometry`（★ 7.7）、`setDrawingZOrder`（★ 7.7）、`setDrawingWrap`（★ 7.7）、`setShapeStyle`（★ 7.7）、`setTextboxContent`（★ 7.7）、`setMathTokens`（★） |
  | 墨迹 | `removeInks`（★ 6.8）、`insertInk`（★ 6.8） |

  `docs/03` 是冻结稿，§8.2 的旧清单**不**随本表更新（改动需项目负责人批准；见「待决」6）。
  本表是协议的权威清单。
- 验收：门 2 全绿（60 变体 × 上下文化往返 + 协议/原生保存字节相同，全语料抽样 +
  `TEST-07` 走协议）；三类去处各有线型形态的断言用例。

## BIND-04 保存与 `SaveOptions`（决策 1；SAVE-01、SAVE-07）

- **必须**：协议 `SaveOptions` **收缩到包级五项**——

  | 键 | 类型 | 缺省 | 语义 |
  | --- | --- | --- | --- |
  | `savedAt` | string | 不动 | `core.xml` 的 `dcterms:modified`；单独设置**不触发**保存（不变式 1 优先，`SAVE-07`） |
  | `removePersonalInfo` | bool | 沿用文档标志 | 写 `w:removePersonalInformation` 并按值清洗作者（`SAVE-07`） |
  | `removeDateAndTime` | bool | 沿用文档标志 | 写 `w:removeDateAndTime` 并按值删 `w:date`（`SAVE-07`，超过 TS 的能力） |
  | `pruneOrphans` | bool | true | 保存前回收**本次会话**造成的孤儿关系 / part（`SAVE-07`；原本就是孤儿的一字节不动） |
  | `normalizeZOrder` | bool | false | 主 part 浮动对象 z 序稳定重排（`spec/18` 待决 4 拍板 false） |

- **必须**：今天 `SaveOptions` 里其余为 TS 存在的翻译入口**全部公开为 `EditOp`**，不留在
  保存选项里——5.6 族（节四项 / 页眉页脚槽 / 逐节页眉页脚 / `hfAllSections` / 水印 / 页面
  底色 / 保护 / 奇偶页眉）与 6.8 墨迹权威列表在 BIND-03 的 60 变体里已有对应
  （`setSectionProps` / `setHeaderFooter` / `linkHeaderFooter` / `setWatermark` /
  `setPageColor` / `setDocumentSettings` / `removeInks` + `insertInk`）；**5.7 族的声明
  part 翻译（参考文献权威列表 / 编号追加 / 编号重启 / 主题字体 / 主题配色 / 样式 upsert）
  在 8.3 成为新 `EditOp` 变体**并增补进 BIND-03 的清单（同一协议版本内增补，见 BIND-08）。
- **必须**：`save` 的不变式不打折——无编辑保存字节相同（不变式 1）；改一处不动其他 zip
  条目（不变式 2）；`save` 在克隆上做，失败不影响会话（BIND-01）。
- 验收：`SaveOptions` 只剩五个键（serde `deny_unknown_fields` 之外的未知键报
  `BIND_BAD_ARGUMENT`）；每个被公开的翻译入口有一处「对应 `EditOp`」测试；无编辑
  `save({})` 与输入逐字节相同（全语料）。

## BIND-05 媒体句柄（决策 8；`PKG-11` MediaStore）

- **必须**：媒体**不进 JSON**——`document()` 的 `media[]` 只给条目
  `{ mediaId, partId, uri, mime, kind }`；字节经 `media(id, mediaId) -> bytes` **按需**取。
  `kind` 的 `Metafile` / `Tiff` 给原字节，转换是调用方的服务（`docs/03` §3.5 冻结）。
- **必须**：`addMedia(id, bytes, mime) -> MediaId`：内容哈希去重（已有同字节同 mime 的
  part → 返回已有 id）；part 名 `word/media/image{N}.{ext}` 取第一个空闲 N（`EDIT-06`）；
  不存在的 `SessionId` → `BIND_NO_SESSION`，不存在的 `mediaId` → `BIND_ID_UNKNOWN`。
- **必须**：外部目标（`TargetMode="External"`）不进 `media[]`，在引用它的显示模型 /
  字段处给原始 URL 字串。
- 验收：含外部图片的语料文档 `media[]` 不含外部 URL；`addMedia` 同字节两次返回同一 id；
  `media` 对全部语料媒体 part 返回的字节与包内原字节相同。

## BIND-06 `resolve` 查询（决策 9；RES-01–RES-12）

- **必须**：**只给批量接口**（`spec/19` 风险 6：逐 run 跨边界调用不可接受）——五个导出，
  输入 `[nodeId]`，输出每个 id 一条 `{ value, provenance }`（`RES-01` 的 `Effective<T>`
  形态；`provenance` 含 `Toggle { levels }`）：

  | 导出 | `value` | 输入 id |
  | --- | --- | --- |
  | `resolveRuns` | `EffectiveRunProps`（RES-03/04/05/06） | run `nodeId` |
  | `resolveParas` | `EffectiveParaProps`（RES-07） | 段落 `nodeId` |
  | `resolveCells` | `EffectiveCellProps`（RES-08） | 单元格 `nodeId` |
  | `resolveSections` | `EffectiveSection`（RES-10 六槽） | `sectPr` `nodeId`（隐式节用 `sectionIndex`） |
  | `resolveTable` | `TableView`（RES-08 列宽视图与条件格式） | 表格 `nodeId` |

- **必须**：不存在的 `nodeId` 或类型不符（把段落 id 喂给 `resolveRuns`）→ 该条返回
  `{ error: BIND_ID_UNKNOWN }`，**不**让整个批量失败。
- **必须**：编号显示计算（`lvlRestart` / `numStyleLink` / `startOverride` / 跨文档序累加）
  进 `resolve::list_markers`（决策 9）；编号显示的**文字与缩进**留给调用方。
  （`spec/19` 待决 4 需项目负责人确认，见「待决」3。）
- **必须**：五个导出对全语料与 `Resolver::*` 的直接调用逐字段相等（`resolve_query!` 同形
  展开导出、JSON 投影与这条比对）。
- 验收：门 5 的 resolve 部分；批量 1,000 个 id 的 `resolveRuns` 在最大真实文档上 p95 < 50 ms
  （8.4 实测，超了按 `spec/19` 风险 6 加 `resolveAll`）。

## BIND-07 诊断与错误码（决策 1；`spec/00` §0.5）

- **必须**：一切失败 = `{ code, message }`：`code` 稳定可依赖（`DiagCode::as_str` 的字串），
  `message` 只给人看（**禁止**调用方解析）。诊断形态沿用 `spec/00` §0.5：
  `{ part, range?: [start, end], code, origin: "preExistingDamage" | "engineInvariantViolation",
  message }`。
- **必须**：`BIND_*` 前缀的协议码（随实现登记进 `DiagCode` 的 BIND 段）：

  | code | 时机 |
  | --- | --- |
  | `BIND_NO_SESSION` | `SessionId` 不存在 |
  | `BIND_BAD_ARGUMENT` | 调用方 JSON 参数解析失败 / 类型不符（8.0② 已落地） |
  | `BIND_PROTOCOL_MISMATCH` | `protocolVersion` 不匹配（BIND-08） |
  | `BIND_ID_UNKNOWN` | `nodeId` / `spanId` / `fieldId` / `mediaId` / `partId` 不存在或跨会话引用 |
  | `BIND_XML_ESCAPE` | 逃生口使用计数（BIND-03；诊断级，不是错误） |

- **必须**：引擎层的 `Err` 码原样外发（`EDIT_*` / `PKG_*` / `SAVE_*` / `XML_*`）；包装层
  **禁止**改写或归并。不支持的输入 = `Err(EDIT_UNSUPPORTED)`（能力未到，不是文档坏了）。
- **必须**：`diagnostics(id)` 返回会话累计诊断（含解析期 `PreExistingDamage` 与编辑期
  `EngineInvariantViolation`），按发生顺序。
- 验收：每个 `BIND_*` 码一条触发用例；`Err` 后状态逐字节不变（`fuzz_bind` oracle）。

## BIND-08 协议版本（`spec/19` 风险 7）

- **必须**：`version()` 返回 `{ version: <crate 版本>, git: <构建提交>, protocol:
  "native/<major>" }`；`open` 可带 `expectProtocol` 声明，不匹配即
  `Err(BIND_PROTOCOL_MISMATCH)`，**禁止**静默继续。
- **必须**：版本规则——M8′ 落地期 `protocol = "native/0"`（可变）；M8′ 门 3 关闭时升
  `"native/1"` 并冻结语义。此后：兼容增补（新 `EditOp` 变体、新可选字段）不动 major；
  破坏性变更升 major，旧 major 至少保留一个 minor 周期。
- compat 绑定面（`crates/rsword-js` 的五函数）继续用 `compat/1`，与本协议互不影响。
- 验收：`expectProtocol: "native/0"` 匹配 / `expectProtocol: "native/99"` 拒绝各一条用例。

## BIND-09 只读出口 `partBytes` / `nodeXml`

- **必须**：`partBytes(id, partId) -> bytes`——part 在会话当前状态下的字节（`Clean` part
  为原字节；脏 part 为当前 DOM 的序列化）。`nodeXml(id, nodeId) -> string`——节点子树的
  XML 文本（含原前缀写法，`XML-13`）。
- **必须**：**只读**——**禁止**成为绕过 `EditOp` 的写路径；不改变任何状态、不产生诊断。
- **应**：两个出口标注为调试用途（文档与绑定层注释）；生产侧 lint（M9′ 的工具约定）应能
  禁用它们。不存在的 id → `BIND_ID_UNKNOWN`。
- 验收：无编辑会话的 `partBytes(主 part)` 与输入该 part 原字节相同；`nodeXml` 输出可重新
  解析且与原子树规范化相等。

## BIND-10 按需取与预算（决策 3）

- **必须**：`document(opts?)` 首版即支持按需取——

  | `opts` 键 | 类型 | 语义 |
  | --- | --- | --- |
  | `blockRange` | `{ from, to }`（顶层块下标，半开） | `main[]` 只投影该区间；越界裁剪到实际范围（**不报错**） |
  | `fields` | string[] | 顶层字段裁剪清单（`"main"` / `"styles"` / `"numbering"` / …）；缺省 = 全部 |
  | `depth` | u32 | 嵌套块（单元格内、文本框内）的投影深度上限；超出的子树给 `Protected(TooDeep)` 形态的占位（`MOD-02`） |
  | `display` | bool | 显示模型投影（BIND-02，缺省 false） |

- **必须**：无参 `document()` 返回整份（本地工具与测试保留这条路）。
- **必须**：应答带 `{ totalBlocks, truncated: bool }`；`blockRange` / `depth` 参数非法
  （负区间、类型错）→ `BIND_BAD_ARGUMENT`。
- **必须**：裁剪后的输出仍过同一份 JSON Schema（被裁剪的顶层字段在 schema 里为可选；
  `Block` 的字段集合不随裁剪变化）。
- **必须**：`spans` / `fields` / `revisions` 在裁剪时**仍投影全量**（它们是跨块索引，裁了
  就没法对齐；体积大头在 `main`，见门 6）。
- 验收：百块文档 `blockRange { from: 10, to: 20 }` 只返回 10 块且 `totalBlocks` 正确；
  裁剪与整份的对应块逐字节相同；schema 校验通过。

## BIND-11 crate 公共 API 的稳定性承诺（`spec/19` 风险 1）

- **必须**：稳定面 = `bind::native` 全部导出 + 六个核心类型（`Node` / `Dirty` / `Anchor` /
  `RangeSpan` / `FieldSpan` / `EditOp`）+ `EditSession` / `EditContext` / `MutationResult` /
  `Error` / `DiagCode`。**其余公共项**降为 `#[doc(hidden)]` 或 `unstable` feature，留一版
  观察期。（按 `spec/19` 待决 3 的建议形态写；**待项目负责人确认**，见「待决」1。）
- **必须**：稳定面内的公共类型全部 `#[non_exhaustive]`（纯数据值对象如位置类型可在文档里
  声明稳定，豁免）；`DiagCode` 只许追加变体。
- **必须**：`#![warn(missing_docs)]` 打开且为零；`cargo doc --no-deps` 零警告；
  `examples/` 至少 `read.rs` / `edit.rs` / `agent.rs` 三个并在 CI 跑（门 3）。
- **必须**：破坏性变更只在 crate minor 版本做，且变更日志写明迁移路径；`protocol`
  （BIND-08）与 crate 版本独立演进。
- 验收：门 3 全绿；默认 feature（`default = ["native"]`）构建不含 `compat_ts` 且能完成
  `open → document → apply → save`。

## 验收清单

| ID | 用例 |
| --- | --- |
| BIND-01 | 每个导出对假 `SessionId` → `BIND_NO_SESSION`；`save` 失败后 `document()` 不变；`open` 失败不留表项 |
| BIND-02 | 全语料过 schema；serde 往返幂等；checklist 覆盖 `MOD-01`–`MOD-11` 全字段；`display` 缺省关且体积达标 |
| BIND-03 | 60 变体 × 上下文化往返（scratch Dom：构造 → `to_json` → 文本 → `from_str` → `from_json` → 相等）；协议 vs 原生 `apply` 保存字节相同；`NewElement` 三类去处各有线型形态断言（a 计 `BIND_XML_ESCAPE`，b 为 patch 形态不计数，c 留字符串） |
| BIND-04 | `SaveOptions` 五键；无编辑 `save({})` 字节相同；翻译入口皆有对应 `EditOp` 测试 |
| BIND-05 | `addMedia` 去重；`media` 字节与包内相同；外部 URL 不进 `media[]` |
| BIND-06 | 五导出与 `Resolver::*` 全语料逐字段相等；错误条目不拖垮批量 |
| BIND-07 | 每个 `BIND_*` 码一条触发用例；`Err` 后状态不变 |
| BIND-08 | 版本匹配 / 拒绝各一例；`version()` 三字段齐 |
| BIND-09 | `partBytes` 无编辑字节相同；`nodeXml` 可重解析 |
| BIND-10 | 裁剪 = 整份对应切片；`totalBlocks` 正确；schema 通过 |
| BIND-11 | `cargo doc` 零警告；`missing_docs` 为零；默认 feature 无 `compat_ts`；三 example 进 CI |

## 待决（评审时请项目负责人一并拍板）

| # | 事项 | 本文写法 |
| --- | --- | --- |
| 1 | BIND-11 的保守程度（`spec/19` 待决 3）：一次全定 vs 只定 `bind::native` + 核心类型留观察期 | 按后者写 |
| 2 | `serde` derive 依赖进 `rsword`（`spec/19` 待决 1）：BIND-02/03 的 serde 投影与 schema 生成都预设 `serde`（+ 可选 `schemars`）；`ts-rs` 不引入，类型生成留给调用方 | 按建议写 |
| 3 | 编号显示计算归 `resolve::list_markers`（`spec/19` 待决 4） | 按决策 9 写进 BIND-06 |
| 4 | `*.model.json` 放 `corpus/` 内与 `*.expected.json` 并列（`spec/19` 待决 5；BIND-02 验收的快照位置） | 按建议写 |
| 5 | `protocol` 升 `native/1` 的时点（BIND-08） | 建议门 3 关闭时 |
| 6 | `docs/03` §8.2 与 BIND-03 的 60 变体清单脱节（34 项：20 个新增操作 ★ + 14 个形态变化 ▲）：`docs/03` 是冻结稿，是否升 v3.4 收编——本文件不改 `docs/03` | 需批准才动 |
