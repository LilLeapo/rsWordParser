# docx 引擎 Rust 重写：核心架构 v3（冻结稿）

> 取代 `rust-parser-design-v2.md`。前置规格：`rust-parser-dev-guide.md`（现有 TS 实现）。
> 范围：方案 A。Rust 整体替换 `parseDocx` 与 `saveDocx`，产出编辑器消费的模型（含显示模型），不做布局与渲染。
> 状态：分层、六个核心类型、不变式为**冻结项**；EMF/WMF 转换、媒体句柄切换时点、`resolve` 的 Word 实测校准为**开放项**。
> 修订：v3.1（2026-09-03）吸收第二轮评审：命名空间作用域与子树移动、词法前缀保留、Anchor 坐标系与文档序比较、UTF-16 偏移协议、物理 run 与段、DOM + Span 为规范状态、不变式 2 措辞、校验来源区分。此后不再改大结构，进入 M0/M1。
> 修订：v3.2（2026-09-03）第三轮评审的五处实现级修正：补 `NsId::Xmlns`；跨 part 移动改为 `rehome`（relationship 是 part 作用域，不能保持 `Clean`）；Span 增加 `FlowId`；内联坐标流规定原子为 `U+FFFC`（1 个 UTF-16 单位）；`MutationPlan` 必须 validate → 原子 commit。架构评审到此结束。
> 基线：工作树 `f105f36`，2026-09-03。

---

## 0. 冻结的内容

1. **保真策略**：文件是真相。未编辑内容零字节改动，编辑只发生在被标脏的 XML 节点上。
2. **七层结构**：L0 包层 → L1 无损 XML → L2 Span 层（含字段子系统）→ L3 语义层与文档模型（旁挂 `resolve`）→ L4 编辑引擎 → 保存前校验与无损序列化 → 包写回。
3. **六个核心类型**：`Node`、`Dirty`、`Anchor`、`RangeSpan`、`FieldSpan`、`EditOp`。定义见第 4、5、8 节，索引见第 13 节。
4. **三条不变式**：无编辑保存字节相同；编辑一个节点，其他干净节点原文子串原样出现；病态输入局部降级、整体仍可保真保存。
5. **两条范围声明**：修订能力与现有 TS 对齐（解析尽量完整、保留必须完整、生成至少与 `saveDocx` 今天的能力相同）；Strict 包保持 Strict，转 Transitional 是显式导出功能。
6. **借鉴决策**：借 LibreOffice 的命名空间 token 表、MCE 处理、导出器的 schema 顺序知识、真实语料测试方式、`oox` 颜色与 VML 表；不借 DomainMapper、Writer/UNO 映射、TableManager、布局兼容逻辑；兼容标志作为文档事实自己建模；有效语义由 `resolve/` 自己实现。
7. **状态真相**：DOM + Span 是唯一可保存的规范状态；Document Model 是它们的语义投影，可增量刷新，但任何时刻都必须能从 DOM + Span 完整重建。编辑操作作用于 DOM + Span 事务，模型只做投影刷新。

---

## 1. 目标、边界、不变式

### 1.1 输入输出

- 解析：`.docx` 字节 → `Document`；第一阶段经 `compat_ts` 适配器输出与今天 TS `ParsedDoc` 字段兼容的 JSON。
- 修改：`EditOp` 序列（第一阶段兼容今天的 `SaveBlock[]`）→ 新 `.docx` 字节。

### 1.2 不做

- 分页、行布局、字体度量、渲染、像素级碰撞与避让。
- `.doc`、RTF、ODT。L1 与 L3 之间留事件流缝，将来可接。
- 解析器内的排版决策。现有 TS 中的碰撞位移、画布分栏、WordArt 字号压缩、连线抓取带、`wrapTopAndBottom` band、页宽 4680 twips 猜测、样式填充烘进单元格，全部移到渲染器或由 `resolve` 提供带来源的数据。

### 1.3 不变式

1. 无编辑保存 → 输出字节与输入完全相同。
2. 编辑一个节点 → 未修改 part 的内容与压缩数据不经过重新序列化或重新压缩（包级 zip 元数据是否逐字节一致不作保证）；被修改 part 中所有干净节点的原文子串原样出现在输出中。
3. 病态输入 → 局部降级为保护块（节点保持 `Clean`），文档整体仍能字节保真保存。

### 1.4 范围声明

- **修订**：解析全部修订类型（第 6.6 节枚举）；保存时全部保留；生成能力与现有 `saveDocx` 对齐（编辑器今天已能产生带追踪的 `w:ins`/`w:del`，见 `apps/docs/tests/ai-track-revisions.test.ts`），机制是 `EditContext.track_changes`。
- **Strict OOXML**：输入 Strict 则输出 Strict；重生成节点按 part 自身的命名空间上下文与包 flavor 生成；"导出为 Transitional"是独立功能。这与现有 TS 在装载时归一化为 Transitional 的行为不同，是有意的改变。

---

## 2. 分层架构

```mermaid
flowchart TB
  subgraph L0[L0 包层 package]
    flavor[PackageFlavor: Transitional / Strict / Mixed]
    graph[Part 图 · RelTarget Internal/External]
    nsctx[Part NamespaceContext]
    media[MediaStore 句柄 · 惰性解码]
  end
  subgraph L1[L1 无损 XML xml]
    tok[QName 按 URI · MCE · 字节区间] --> dom[Node 树 · Dirty: Clean/DescendantDirty/SelfDirty/New/Deleted]
  end
  subgraph L2[L2 Span 层 span]
    ranges[平铺 RangeSpan 索引: Bookmark / Comment / Permission / Move / CustomXml]
    fields[Field 子系统: FieldSpan 嵌套 · 指令 tokenization · FieldPolicy]
  end
  subgraph L3[L3 语义层 semantic + model]
    props[属性表] --> facts[ParagraphFacts] --> classify[分类表] --> model[Document Model]
    model --> resolve[resolve/: 样式链 · toggle · 编号 · 节继承 · 有效属性]
  end
  subgraph L4[L4 编辑引擎 edit]
    ops[EditOp → 模型变更 · dirty 传播 · Anchor 变换 · 修订生成]
  end
  subgraph L5[保存 save]
    validate[保存前校验: Span · 关系 · 字段 · 命名空间 · 诊断] --> ser[无损序列化] --> writer[包写回 raw_copy 未变 part]
  end
  L0 --> L1 --> L2 --> L3 --> L4 --> L5
```

目录对应：`package/`、`xml/`、`span/`（含 `span/field/`）、`semantic/`、`model/`、`resolve/`、`edit/`、`save/`、`bind/`（含 `bind/compat_ts/`）。不使用 `mapper` 一类名字。

---

## 3. L0：包层

### 3.1 zip 与限额

- 先对字节做 Info-ZIP Unicode Path（`0x7075`）字段中和，再交给 `zip` crate。
- 按 central directory 声明大小检查：≤10,000 part、单 part ≤512 MiB、总计 ≤1.5 GiB。
- 主 part：`word/document.xml`，否则 `_rels/.rels` 的 `officeDocument` 目标；ODF 与非 docx 报错。

### 3.2 PackageFlavor 与 Part

```rust
pub enum PackageFlavor { Transitional, Strict, Mixed }

pub struct Part {
    id: PartId,
    uri: PartUri,                 // 规范化的包内路径 "word/document.xml"
    content_type: String,
    flavor: PartFlavor,           // 由该 part 根元素绑定的 URI 判定
    rels: Vec<Relationship>,
    dom: Option<Dom>,             // XML part 惰性解析
    raw: ZipEntryRef,             // 原始条目（压缩字节可直接拷贝）
}
```

- 包 flavor 由主 part 与其 rels 判定；单 part 的 flavor 单独记录，包内不一致时包为 `Mixed`。产品只承诺 Transitional 与 Strict，`Mixed` 为防御性分类，保证不会把混合包归错类。

### 3.3 关系

```rust
pub struct Relationship { id: String, kind: RelType, target: RelTarget }
pub enum RelTarget { Internal(PartUri), External(String) }
```

- 路径解析只有一个函数：`/` 前缀为包根，相对路径相对当前 part 目录，`..` 归一化。`External` 不参与路径解析。
- `RelType` 同时识别 Strict 与 Transitional 两族 URI（`officeDocument/relationships/...` 与 `purl.oclc.org/ooxml/officeDocument/relationships/...`），映射到同一枚举值，并记录原始族别供生成使用。

### 3.4 NamespaceContext

```rust
pub struct NamespaceContext {
    root_decls: Vec<(Prefix, NsId)>,           // part 根上已声明的绑定
    preferred: HashMap<NsId, Prefix>,          // URI → 该 part 惯用前缀
    ignorable: Vec<Prefix>,                    // 根上 mc:Ignorable
    flavor: PartFlavor,
}

pub struct Scope { bindings: Vec<(Prefix, NsId)> }   // 某个节点位置上实际生效的 prefix → URI

impl Dom {
    /// 沿祖先链收集 xmlns / xmlns:* 属性得到的有效绑定；内层遮蔽外层
    pub fn namespace_scope(&self, at: NodeId) -> Scope;
    /// 子树实际使用的绑定（扣除子树内部自己的声明）是否都在目标位置的 scope 中且 URI 一致
    pub fn namespace_compatible(&self, subtree: NodeId, dest: NodeId) -> bool;
}
```

命名空间是有作用域的：**同 part 不等于同前缀上下文**。`root_decls` 只是常见情形的快路径，任何涉及位置的判断都用 `namespace_scope(node)`。

生成 QName 时的规则：`Schema QName + PartFlavor + 目标位置的 Scope → 输出前缀`。已有绑定复用；没有则按 flavor 选 URI、分配规范前缀，并**在新子树根上内联声明**，不碰 part 根；只有需要更新 `mc:Ignorable` 时才修改根（根变 `SelfDirty`）。生成器任何地方不得写死 `w:` 字面量。

### 3.5 MediaStore

`MediaId → {part, mime, bytes}`；EMF/WMF/EMZ/WMZ、TIFF 的转换是可插拔服务，默认惰性。模型只引用 `MediaId`。`compat_ts` 阶段内联 dataURL；之后改句柄 + 二进制表。EMF/WMF 转换 Rust 侧暂不实现，输出 `MediaKind::Metafile` 由 TS 侧继续转换。

---

## 4. L1：无损 XML

### 4.1 Node

```rust
pub struct NodeId(u32);          // arena 索引，会话内稳定；Deleted 节点保留在 arena 中

pub struct Dom {
    part: PartId,
    src: Arc<[u8]>,              // 原始 part 字节
    nodes: Vec<Node>,
    root: NodeId,
}

pub struct Node {
    kind: NodeKind,
    parent: Option<NodeId>,
    lex: Option<Lex>,            // 原文词法区间；New 节点为 None
    dirty: Dirty,
}

pub enum NodeKind {
    Element {
        name: QName,
        attrs: Vec<Attr>,        // 保持原顺序
        children: Vec<NodeId>,   // 元素与文本混排，保持顺序；不含 Deleted 之外的过滤
        mce: Mce,                // 见 4.4
    },
    Text(TextValue),
    Opaque,                      // 注释 / PI / CDATA：永远按 lex 原字节输出
}

pub struct Lex {
    range: Range<u32>,           // 整节点 [start, end)
    open: Range<u32>,            // 开标签（自闭合时等于 range）
    close: Range<u32>,           // 闭标签（自闭合时为空）
    name: Range<u32>,            // 开标签中的原始限定名，含前缀（"w:p" / "x:p"）
}

pub struct Attr {
    name: QName,                 // 语义身份
    lex_name: Option<Range<u32>>,// 原始写法（含前缀）；New 属性为 None
    value: AttrValue,
    quote: u8,                   // b'"' | b'\''
}
pub enum AttrValue { Raw(Range<u32>), Owned(String) }
pub enum TextValue { Raw(Range<u32>), Owned(String) }

pub struct QName { ns: NsId, local: LocalName }
pub enum NsId { W, R, A, Wp, Wps, Wpg, Pic, C, Cx, Dgm, Dsp, Lc, M, Mc, V, O, W10, W14, W15, Wp14, Xml, Xmlns, Rels, Ct, None, Unbound(Interned), Other(Interned) }
// Xml   = http://www.w3.org/XML/1998/namespace   （xml:space 等属性）
// Xmlns = http://www.w3.org/2000/xmlns/          （xmlns / xmlns:* 声明本身）—— 两者是不同的命名空间
pub enum LocalName { P, R, T, PPr, RPr, /* … 由 schema 表生成 … */ Other(Interned) }
```

- `QName` 是语义身份，`Lex.name` 与 `Attr.lex_name` 是原始写法。`SelfDirty` 重建开标签时，元素与属性未改名则沿用 `lex_name`，这才使"保留前缀"可兑现；`New` 节点的前缀由目标位置的 `Scope` 与 `NamespaceContext` 决定。
- `xmlns` / `xmlns:*` 声明照常存为 `Attr`（`NsId::Xmlns`），`namespace_scope` 由它们计算。
- `NsId` 中 Strict 与 Transitional 的同族 URI 映射到同一枚举值；原始 URI 族别记录在 `Part.flavor`。
- 文本值不 trim，不预解实体；读取时按需解码五个命名实体与数字实体；干净节点写回时直接拷字节。
- 迭代解析，深度上限 100k。
- 名字表由精选 schema 表在 `build.rs` 生成，覆盖 WordprocessingML、DrawingML 及 `wp/wps/wpg/pic/c/cx/dgm/dsp/lc`、VML `v/o/w10`、`m`、`mc`、`w14/w15/wp14`、`r`、rels、Content_Types。未知名落到 `Other`，仍可原样写回。

### 4.2 Dirty

```rust
pub enum Dirty {
    Clean,             // 自身与后代都未变：整节点拷 lex.range
    DescendantDirty,   // 自身未变、后代有变：拷 lex.open，逐子节点递归，拷 lex.close
    SelfDirty,         // 自身标签/属性有变：重建开标签（保留属性顺序、前缀、引号风格），逐子节点递归，重建闭标签
    New,               // 无 lex：整棵按 schema 生成
    Deleted,           // 不输出；保留在 arena 供 Anchor 变换与撤销
}
```

传播规则：把节点标为 `SelfDirty`/`New`/`Deleted`，或改变其 `children` 列表，则沿祖先链把 `Clean` 改为 `DescendantDirty`，遇到已非 `Clean` 的祖先停止。

移动子树有两个**不共用**的原语：

- `move_within_part(subtree, dest)`：先做 `namespace_compatible(subtree, dest)`。兼容 → 子树保持 `Clean`，原字节搬到新位置。不兼容（前缀缺失或在目标处绑定到别的 URI）→ 在子树根上补差异的 `xmlns:` 声明（内层声明遮蔽外层，任何冲突都可这样解决），子树根变 `SelfDirty`，后代仍 `Clean`。
- `rehome_subtree(source, target_part, dest) -> NodeId`：跨 part 时**禁止**保持 `Clean`，因为 relationship 是 part 作用域的：`r:id`/`r:embed`/`r:link`/`r:dm`/`r:lo`/`r:qs`/`r:cs`/`r:pict` 引用的 `rIdN` 只在源 part 的 `.rels` 里有意义，`wp:docPr/@id`、书签 `w:id` 也只在 part 内唯一。`rehome` 在一个原语内统一处理：把源 part 中被引用的关系复制到目标 part 的 `.rels`（共享目标如 media 不复制字节，只加关系）并重新分配 `rId`；重新分配 `docPr id`、书签 id；按目标 part 的 `Scope`/flavor 重写 QName 与编解码；产出一棵 `New` 子树，源子树按需 `Deleted`。

### 4.3 词法保真的边界

- 保留：开闭标签原字节、属性顺序、每个属性的引号风格、实体原文、空白。
- 不做：属性值级别的字节拼接。`SelfDirty` 节点重建整个开标签，代价是属性间空白归一，收益是实现简单且无转义风险。

### 4.4 MCE

```rust
pub struct Mce {
    role: MceRole,               // None | AlternateContent | Choice{requires} | Fallback
    active: bool,                // AlternateContent 下被选中的分支
    process_content: bool,       // 元素自身可忽略、内容仍需处理（mc:ProcessContent）
    must_understand: bool,       // mc:MustUnderstand 命中未理解命名空间：记诊断
}
```

- `mc:Ignorable`：记录到 `NamespaceContext.ignorable`；不在已理解集合中的可忽略前缀元素，模型层跳过但 DOM 保留。
- `mc:AlternateContent`：`Requires` 中命名空间全部在已理解集合 → 选该 `Choice`，否则 `Fallback`。两支都在树里，只有 `active` 分支参与语义遍历。已理解集合初始：`wps wpg wp14 w14 w15 cx`。
- `mc:ProcessContent`：语义遍历把该元素当透明容器。
- `mc:PreserveElements/PreserveAttributes`：无损 DOM 天然满足，只记录。

模型层一律通过 `dom.semantic_children(node)` 遍历：跳过非 `active` 分支与可忽略元素，展平 `process_content` 容器，跳过 `Deleted`。

---

## 5. L2：Span 层

### 5.1 为什么是独立层

书签、批注、权限、移动范围、customXml 修订范围的端点是 run 的**兄弟元素**，字段的边界是含 `fldChar` 的 run；两者都跨兄弟节点、可跨段落、可跨单元格。前者之间**任意交叠**，后者**正确嵌套**。树表达不了，必须是覆盖在节点序列上的位置结构，而且这些位置在编辑时要被维护。

### 5.2 Anchor 与 Affinity

```rust
pub struct Anchor {
    container: NodeId,           // 持有该边界的元素：w:p / w:tc / w:body / w:txbxContent / w:sdtContent …
    index: u32,                  // 容器**内容序列**中的逻辑边界 0..=content.len()
    affinity: Affinity,          // 在该边界处插入内容时锚点的去向
    marker: Option<NodeId>,      // 物理标记元素（bookmarkStart 等）；字段边界为 None
}
pub enum Affinity { Left, Right }   // Left = 吸附左侧内容（插入落在锚点之后）；Right = 吸附右侧内容

impl Dom {
    /// 文档序：沿祖先链到公共祖先，比较子序号；同容器比较 index，再比较 affinity
    pub fn compare(&self, a: &Anchor, b: &Anchor) -> Ordering;
}
```

- **坐标系**：内容序列 = 容器的语义子节点，去掉属性元素（`pPr/tcPr/trPr/tblPr/tblGrid/sectPr`）与所有范围标记元素，不含 `Deleted`。`index` 只描述逻辑内容边界，**标记自身不计入**。解析时由标记位置建立 Anchor；此后编辑期间 Anchor 是事实，标记只是它的物理投影，保存时按 Anchor 物化。不允许反向由标记推导 Anchor。
- **文档序**：跨容器的 Span（跨段落、跨单元格）用 `dom.compare` 判定起在终前。
- **内容流**：`FlowId(u32)` 标识独立文本流（body、每个 `txbxContent`、每个 `hdr`/`ftr`、每个脚注/尾注/批注条目）。`flow_of(&Anchor) -> FlowId` 由容器到流根的映射缓存给出；Span 两端**必须**同流，校验直接比较 `FlowId`，不靠祖先树推断。
- 默认 gravity：**起点 `Right`，终点 `Left`**。在范围边界输入的文字落在范围之外，范围不因边界输入膨胀，与 Word 一致；在范围内部输入自然扩展范围。
- 编辑引擎每次改变 `children` 列表都要变换受影响容器内的所有 `Anchor.index`；容器被删除时，锚点按 5.5 的类型策略处理。
- 序列化前，物理标记按 anchor 的当前位置物化：位置未变且标记 `Clean` → 拷字节；位置变了 → 旧位置 `Deleted`、新位置 `New`（标记是自闭合小元素，属性原样携带）。

### 5.3 RangeSpan

```rust
pub struct SpanId(u32);

pub struct RangeSpan {
    id: SpanId,
    part: PartId,
    kind: RangeKind,
    start: Anchor,
    end: Option<Anchor>,         // None = 该 part 内未闭合（损坏输入）
}

pub enum RangeKind {
    Bookmark { id: String, name: String, hidden: bool /* `_` 前缀 */, cols: Option<(u32, u32)> /* w:colFirst/colLast */ },
    Comment { id: String, reference: Option<NodeId> /* 承载 w:commentReference 的 run */ },
    Permission { id: String, editor: Option<String>, group: Option<String>, cols: Option<(u32, u32)> },
    MoveFrom { id: String, name: String, meta: RevisionMeta },
    MoveTo { id: String, name: String, meta: RevisionMeta },
    CustomXmlIns { meta: RevisionMeta },
    CustomXmlDel { meta: RevisionMeta },
}
```

索引是**平铺列表** `Vec<RangeSpan>`，另建按 `container` 的倒排索引供编辑变换。没有树。

### 5.4 字段子系统

```rust
pub struct FieldId(u32);

pub struct FieldSpan {
    id: FieldId,
    part: PartId,
    form: FieldForm,
    instr: Instruction,          // 语义视图；原文真相是 form 中的节点
    nested: Vec<FieldId>,        // 指令区或结果区内的子字段
    parent: Option<FieldId>,
    policy: FieldPolicy,
    lock: bool,                  // w:fldLock
    dirty_flag: bool,            // w:dirty
    ff_data: Option<NodeId>,     // 表单字段定义（begin run 内 w:ffData）
    cross_paragraph: bool,
}

pub enum FieldForm {
    Complex {
        begin: NodeId,           // 含 fldChar begin 的 w:r
        separate: Option<NodeId>,
        end: NodeId,
        instr_nodes: Vec<NodeId>,
        result_nodes: Vec<NodeId>,   // separate 与 end 之间的节点（run、嵌套字段的节点、跨段时的段落）
    },
    Simple { node: NodeId },     // w:fldSimple
}

pub struct Instruction { keyword: Keyword, tokens: Vec<InstrToken>, raw: String }
pub enum InstrToken {
    Word(String), Quoted(String),
    Switch { name: char, arg: Option<Box<InstrToken>> },       // \h \o \r \p \l …
    GeneralFormat { kind: char, arg: String },                 // \* \# \@ \!
    Nested(FieldId),
}
pub enum Keyword { Page, NumPages, Ref, PageRef, Seq, StyleRef, Date, Time, Hyperlink, Toc, Index, Xe, FormCheckBox, FormText, FormDropDown, IncludePicture, Embed, Link, Formula, If, MergeField, /* … */ Unknown(String) }
pub enum FieldPolicy { Marker, Atom, Link, Form, Picture, Object, Block, Unknown }
```

**构建**：按语义遍历顺序扫描每个 part 的 run，用栈配对 `fldChar begin/separate/end`；`fldSimple` 直接闭合，其子 `fldSimple` 为嵌套。指令文本 = begin 到 separate（或 end）之间 `w:instrText` 拼接；`w:delInstrText` 标为"指令被修订删除"。未闭合 → 所在段落降级为保护块并记诊断。

**指令解析**：只做可靠的 tokenization 与关键字提取，不构建完整 AST。`IF`、`=SUM(ABOVE)`、嵌套 `MERGEFIELD` 的语义不解释。保存时的真相是 `instr_nodes` 的原字节。

**策略表**（`keyword → FieldPolicy`，未命中 `Unknown`；只描述显示形态、可编辑性、保存行为）：

| 策略 | 显示 | 编辑 | 保存 | 关键字 |
| --- | --- | --- | --- | --- |
| `Marker` | 不可见零宽 | 随文本移动/删除 | 干净则原字节；段落重生成时按原文重发 | XE, TA, TC, RD, PRIVATE, SET |
| `Atom` | 结果作为一个不可键入的内联原子，**结果内部保留多 run 格式** | 整体删除、改结果 run 的格式 | 干净则原字节；改格式只重生成结果 run；删除 = 删 begin..end | PAGE, NUMPAGES, SECTION, SECTIONPAGES, NUMWORDS, NUMCHARS, DATE, TIME, CREATEDATE, SAVEDATE, PRINTDATE, EDITTIME, AUTHOR, TITLE, SUBJECT, KEYWORDS, COMMENTS, LASTSAVEDBY, FILENAME, FILESIZE, TEMPLATE, DOCPROPERTY, DOCVARIABLE, USERNAME, USERINITIALS, USERADDRESS, SEQ, STYLEREF, PAGEREF, NOTEREF, REF, QUOTE, SYMBOL, LISTNUM, AUTONUM, AUTONUMLGL, AUTONUMOUT, REVNUM, INFO, `=`, MERGEFIELD, MERGEREC, MERGESEQ, NEXT, NEXTIF, SKIPIF, IF（单段）, COMPARE, ADVANCE, EQ, GOTOBUTTON, MACROBUTTON, CITATION, FILLIN, ASK, GREETINGLINE, ADDRESSBLOCK, AUTOTEXT, AUTOTEXTLIST |
| `Link` | 超链接 | 改文字、改目标 | 仍以字段形式写回；新建链接才用 `w:hyperlink` | HYPERLINK |
| `Form` | 复选框/文本框/下拉的字形或值 | 切换/输入改 `w:ffData` | 重生成 begin run（含 ffData）与结果 | FORMCHECKBOX, FORMTEXT, FORMDROPDOWN |
| `Picture` | 图片 | 同图片原子 | 原字节 | INCLUDEPICTURE |
| `Object` | OLE 预览 | 同对象原子 | 原字节 | EMBED, LINK |
| `Block` | 结果为多个段落，整体为字段块 | 结果段落只读；"更新"由生成器重算整块 | 干净则原字节；更新时替换 separate..end 之间全部节点 | TOC, INDEX, BIBLIOGRAPHY, INCLUDETEXT, DATABASE，及**任何跨段**字段 |
| `Unknown` | 同 `Atom` | 同 `Atom` | 原字节 | 其余 |

页眉页脚不再有 `PAGE_MARK`/`TOTAL_PAGES_MARK`：PAGE/NUMPAGES 是 `Atom`，渲染器看到 `Keyword::Page` 自己替换；`has_page_number` 由字段列表推导。Word 的 TOC 通常是 `w:sdt(docPartGallery)` 包着一个 `Block` 字段，DOM 里只是两层节点，更新目录 = 替换 separate..end。

### 5.5 编辑时的 Span 语义（编辑引擎职责）

- **插入**：按 `Affinity` 决定锚点是否移动；范围内部插入扩展范围。
- **部分删除**：范围缩短，锚点移到删除区间边界。
- **整体删除**（起止都在被删内容中）：
  - Bookmark → 折叠为空书签留在删除点（内部 `_Toc/_Ref` 书签因此仍可被 REF/TOC 引用）。
  - Comment → 删除批注：范围标记、`commentReference` run、`comments.xml` 与 `commentsExtended.xml` 条目一起删除（Word 行为）。
  - Permission → 删除。
  - MoveFrom/MoveTo/CustomXml 范围 → 随修订操作处理。
- **段落拆分/合并**：容器变化时按位置重算 `container/index`。
- **字段**：`Atom`/`Link`/`Form`/`Picture`/`Object` 在编辑器里是原子，光标不落入内部，边界即原子边界；`Marker` 视为零宽原子；`Block` 的结果段落只读。

### 5.6 保存前校验（安全网，不是主修复器）

- 每个 `RangeSpan` 起止成对、起在终前、位于同一 part；孤儿端点成对删除并记诊断。
- 每个 `FieldSpan` begin/separate/end 顺序正确、嵌套闭合。
- `commentRangeStart/End` 有对应 `commentReference` 与 `comments.xml` 条目。
- 关系引用（`r:id`、`r:embed`）在 part 的 rels 中存在。
- 新增节点所需命名空间已声明。

---

## 6. L3：语义层与文档模型

### 6.1 属性表

`RunProps`/`ParaProps`/`CellProps`/`TableProps`/`RowProps`/`SectionProps` 由声明式表格生成（`build.rs`）。表列：字段名、元素 QName、值编解码（`OnOff` 三态、`HalfPoints`、`Twips`、`HexColor{theme_aware}`、`Enum`、`Raw`）、Cs 孪生（`w:bCs/iCs/szCs`）、schema 序号、是否出现在修订快照。编解码按 `PartFlavor` 输出（Strict 的 `ST_OnOff` 为 `true/false`，Transitional 允许 `1/0/on/off`）。

由表生成：读取（DOM → props）、比较（脏检测）、合并写回（未建模子元素原字节保留在原位，建模子元素按 schema 序号替换或插入）、修订快照读取。三态语义在表里显式声明，`keepNext` 与 `pageBreakBefore` 行为一致。

schema 顺序来源：ECMA-376 XSD，对照 LibreOffice `docxattributeoutput.cxx` 输出顺序交叉校验。

### 6.2 ParagraphFacts 与分类

```rust
pub struct ParagraphFacts {
    has_sect_pr: bool, visible_text: bool, visible_text_outside_boxes: bool,
    fields: Vec<FieldId>, inside_field_result: Option<FieldId>,
    drawings: Vec<DrawingFacts>, picts: Vec<PictFacts>, objects: u32,
    math: MathFacts, revision: RevisionFacts,
    style_id: Option<String>, style_vanish: bool, toc_style_level: Option<u8>,
    numbering_ref: Option<ListRef>, outline_level: Option<u8>,
    sdt: Option<SdtInfo>,
}
```

分类是 facts 的纯函数，用按优先级排列的规则表实现（约 25 条），每条可单测。现有 `buildBlock` 的 530 行 `if` 由此替代。

### 6.3 Block 与 Inline

```rust
pub enum Block { Text(TextBlock), Table(TableBlock), Image(ImageBlock), Protected(ProtectedBlock) }

pub struct TextBlock {
    node: NodeId, kind: TextKind /* Paragraph | Heading{level} | ListItem{list} */,
    style_id: Option<String>, props: ParaProps /* 声明值 */, para_mark_props: Option<RunProps>,
    inlines: Vec<Inline>, sdt: Option<SdtInfo>, revision: Vec<Revision>,
}

pub enum ProtectedKind {
    FieldBlockResult(FieldId), Equation(FormulaDisplay), TextBox(Vec<TextboxDisplay>),
    Chart(ChartDisplay), SmartArt(DiagramDisplay), Ole(OleDisplay), Rule(RuleDisplay),
    Invisible, SectionBreak, SectionProps, Unparseable,
}

pub enum Inline {
    Run(Run),
    Field { id: FieldId, result: Vec<Inline> },     // 编辑器视为原子；结果内部保留多 run 格式
    Atom(InlineAtom),
}
pub struct Run {                                   // 与一个物理 w:r 一一对应
    node: NodeId,
    segments: SmallVec<[Segment; 2]>,              // w:t / w:delText / w:tab / w:br … 子节点到 text 偏移的映射
    text: String,
    props: RunProps, link: Option<Link>, rev: Option<RevisionCtx>, comments: SmallVec<SpanId>,
}
pub struct Segment { node: NodeId, kind: SegmentKind, text: Range<u32>, utf16_len: u32 }
pub struct InlineAtom { node: NodeId, kind: AtomKind, props: RunProps, rev: Option<RevisionCtx> }
pub enum AtomKind {
    Math, Ruby { rt: String }, Image(ImageRef), NoteRef { kind: NoteKind, id: String },
    Break(BreakKind /* Page | Column | TextWrapping | SoftHyphen | NoBreakHyphen */),
    Symbol { font: String, code: u32 }, Object(OleDisplay), Drawing(NodeId),
}
```

- 所有块与 run 持有 `NodeId`；`docxIndex`/`originalXml`/`rawPPr`/`rawRPr`/`rawTcPr`/`rawTrPrs`/`sdtShell` 全部由 `NodeId` 与其 `lex` 替代。`compat_ts` 把 body 直接子节点序号映射回 `docxIndex`。
- `label` 变为 i18n key（`ProtectedKind` 即 key）。
- 控制字符协议废除：`\f`/`\v`/`\n`/`‑` 改为显式 `Break` 原子，`compat_ts` 再折回。
- 核心模型**不做逻辑 run 合并**：`Run` 与物理 `w:r` 一一对应，`segments` 给出文本偏移到子节点的映射，编辑定位不需要二次计算，DOM 与模型保持 1:1。相邻同格式 run 的合并是编辑器视图与 `compat_ts` 的投影行为，不进入规范状态。

### 6.4 SdtInfo

```rust
pub struct SdtInfo {
    node: NodeId, alias: Option<String>, tag: Option<String>, control_type: SdtControl,
    lock: SdtLock,                            // Unlocked | SdtLocked | ContentLocked | SdtContentLocked
    data_binding: Option<DataBinding>,        // { prefix_mappings, xpath, store_item_id }
    doc_part: Option<DocPart>,                // docPartGallery / docPartCategory / docPartUnique
}
```

编辑策略：普通 sdt 可编辑；`ContentLocked`/`SdtContentLocked` 只读；有 `data_binding` 的第一阶段限制编辑（显示文字只是绑定数据的缓存，Word 重开会从 customXml 刷回），后续再做 customXml 同步。

### 6.5 设置与兼容事实

```rust
pub struct CompatFacts {
    mode: Option<u32>,                                  // compatSetting compatibilityMode
    settings: Vec<CompatSetting { name: String, uri: String, val: String }>,
    flags: Vec<QName>,                                  // w:compat 下所有布尔子元素
}
```

parser 只记录，`resolve/` 与未来的布局层解释。其余设置（保护、写保护、`removePersonalInformation`、`evenAndOddHeaders`、`autoHyphenation`、`defaultTabStop`、`themeFontLang`、`trackRevisions`）语义与前置文档一致。

### 6.6 修订

```rust
pub struct RevisionMeta { id: Option<String>, author: String, date: Option<String> }
pub enum Revision {
    Insert(RevisionMeta), Delete(RevisionMeta),                  // w:ins / w:del 包裹 run 或块
    MoveFrom { meta: RevisionMeta, name: String }, MoveTo { meta: RevisionMeta, name: String },
    ParaMarkInsert(RevisionMeta), ParaMarkDelete(RevisionMeta),  // w:pPr/w:rPr/w:ins|w:del
    RunPropsChange { meta: RevisionMeta, old: RunProps },        // rPrChange
    ParaPropsChange { meta: RevisionMeta, old: ParaProps, old_style: Option<String>, old_list: Option<ListRef> },
    SectPropsChange { meta: RevisionMeta, old: NodeId },
    TablePropsChange { meta: RevisionMeta, old: NodeId }, TableGridChange { meta: RevisionMeta, old: NodeId },
    RowPropsChange { meta: RevisionMeta, old: NodeId }, CellPropsChange { meta: RevisionMeta, old: NodeId },
    NumberingChange(RevisionMeta),
    CellInsert(RevisionMeta), CellDelete(RevisionMeta), CellMerge(RevisionMeta),
    FieldInstrDelete(RevisionMeta),                              // w:delInstrText
    DeletedText(RevisionMeta),                                   // w:delText（随 Delete 出现）
}
```

解析尽量完整，保留必须完整（未建模的 `old` 用 `NodeId` 指向原节点），生成能力与现有 TS 对齐。

### 6.7 其余子模型

样式、编号（补 `lvlRestart`、`isLgl`、`lvlPicBulletId`）、主题、批注、脚注尾注、参考文献、fontTable、节信息、页眉页脚 part 与前置文档一致。页眉页脚**复用正文管线**（同一段落/表格构建器，不同上下文），内容即 `Vec<Block>`；`HfParagraph`/`HfTableCell`/`HfImage` 删除。显示模型只保留文档事实（几何原值、颜色、锚点元数据），颜色算法借 `oox::drawingml::Color`，VML 表借 `oox/source/vml`。`Document.warnings: Vec<Diagnostic{part, range, code, message}>`。

### 6.8 状态真相：DOM + Span 是规范状态

运行时有三份相关状态：DOM、Span 索引、Document Model。为避免"DOM 改了模型没改、模型改了 Anchor 没改"，冻结以下原则：

- **DOM 是可保存的结构真相**；**Span 是附着在 DOM 上的位置语义**；两者合起来是唯一的规范状态。
- **Document Model 是 DOM + Span 的语义投影**：可以增量刷新，但 `Document::rebuild(&dom, &spans)` 必须在任何时刻都能得到与增量结果相等的模型。测试用它作 oracle。
- 编辑操作不分别手改三份状态：`EditOp → MutationPlan → 对 DOM + Span 的一次事务 → MutationResult → semantic.refresh(&result)`（见 8.3）。

```
        规范状态                       投影
  ┌─────────────────┐          ┌──────────────────┐
  │  DOM  +  Span   │ ───────▶ │  Document Model  │ ───▶ resolve/ 视图
  └─────────────────┘  refresh └──────────────────┘
          ▲
          │ MutationPlan（事务）
       EditOp
```

---

## 7. resolve/

只读视图，不进模型：

- `resolve::run(run, para, styles, theme, defaults) -> Effective<RunProps>`：样式链（basedOn、linked，环检测）、字符样式叠加段落样式、主题字体/颜色、空 EA 槽回填、rtl 下 Cs 选择、docDefaults、默认样式规则（ECMA-376 §17.7.4.17）。每个值带来源 `Declared | FromStyle(id) | FromTableStyle(cond) | FromDefaults | Default`。
- **Toggle 属性**（`b/i/caps/smallCaps/strike/dstrike/outline/shadow/emboss/imprint/vanish`）：不采用普通的 child-overrides-parent 合并；按 ECMA-376 §17.7.3 的 toggle 语义由独立函数 `resolve_toggle(...)` 处理，并用真实 Word 文档做行为校准（Microsoft [MS-OI29500] 对 §17.7.3 记录了 Word 与规范文字的差异，涉及 docDefaults 为 true、多层出现、表格样式与版本差异）。现有 TS 的 `{...parent.display, ...own}` 在此处不正确。
- `resolve::para`：段落有效属性；`bidi` 段落 `jc` 存逻辑值，此处给视觉值。
- `resolve::table_cell`：表格样式条件格式（`tblLook`、行优先于列、条带）、边框与边距回退，带来源。
- `resolve::sections`：header/footer 未声明时继承上一节；`titlePg`、`evenAndOddHeaders` 的有效组合。
- `resolve::numbering`：`numId + ilvl` → 有效级别定义、标记文本（现有 `list-markers.ts` 语义）。
- 空段落的度量来源（段落标记 rPr、最后一个空 run）在此计算，不再由 parser 挑字段。

校准机制：`resolve/` 的每条规则配一组"规范规则 + Word 实测 fixture"，fixture 为真实 `.docx` 与 Word 实际显示的断言。

---

## 8. L4：编辑引擎

### 8.1 EditContext 与位置

```rust
pub struct EditContext {
    track_changes: Option<RevisionAuthor { author: String, date: String }>,   // Some = 所有变更生成修订
    default_run_props: Option<RunProps>,
}

pub struct Utf16Offset(u32);
pub struct InlinePos { para: NodeId, offset: Utf16Offset }   // 段落内容序列内的偏移；每个原子（Field/Atom）占 1 个单位
pub enum BlockPos { Start(NodeId /* container */), After(NodeId /* block */), End(NodeId) }
```

- **偏移单位对外统一为 UTF-16 code unit**，与 JS 编辑器一致（`"A😀B".length === 4`）。Rust 内部字符串为 UTF-8，`Segment.utf16_len` 缓存每段长度，边界处 O(段数) 转换。M9 若前端协议改变再考虑标量或字素单位；在此之前不引入第二种单位。
- **规范内联坐标流**：段落的坐标流由内容序列拼成，规则固定为：run 文本 → 其实际 UTF-16 长度；`Inline::Field` 与任何 `Inline::Atom` → 一个 `U+FFFC`（OBJECT REPLACEMENT CHARACTER），长度 1；范围标记 → 长度 0。字段的显示结果（`3`、`Figure 7`）**不参与坐标**，因此 PAGE 从 `3` 变成 `10` 不会移动后面的偏移。编辑器与 `compat_ts` 用同一坐标流。

### 8.2 EditOp

```rust
pub enum EditOp {
    // 内联
    InsertText { at: InlinePos, text: String, props: Option<RunPropsPatch> },
    DeleteRange { from: InlinePos, to: InlinePos },
    SetRunProps { from: InlinePos, to: InlinePos, patch: RunPropsPatch },
    InsertAtom { at: InlinePos, atom: NewAtom },                 // Break / Image / Math / NoteRef / Symbol
    InsertField { at: InlinePos, field: NewField },              // 由生成器产出 begin/instr/separate/result/end
    ReplaceInlines { para: NodeId, inlines: Vec<NewInline> },    // compat 路径：SaveBlock generated

    // 段落
    SplitParagraph { at: InlinePos },
    MergeWithNext { para: NodeId },
    SetParaProps { para: NodeId, patch: ParaPropsPatch },
    SetParaStyle { para: NodeId, style_id: Option<String> },
    SetList { para: NodeId, list: Option<ListRef> },

    // 块
    InsertBlock { at: BlockPos, block: NewBlock },
    DeleteBlock { node: NodeId },
    MoveBlock { node: NodeId, to: BlockPos },

    // 表格
    SetCellProps { cell: NodeId, patch: CellPropsPatch },
    SetTableProps { table: NodeId, patch: TablePropsPatch },
    InsertRow { table: NodeId, at: u32, template: Option<NodeId> },
    DeleteRow { table: NodeId, at: u32 },
    InsertColumn { table: NodeId, at: u32 }, DeleteColumn { table: NodeId, at: u32 },
    MergeCells { table: NodeId, from: (u32, u32), to: (u32, u32) },

    // 字段
    SetFieldResultProps { field: FieldId, patch: RunPropsPatch },
    ToggleCheckbox { field: FieldId, checked: bool },
    SetFormText { field: FieldId, text: String },
    SetLinkTarget { target: LinkTarget /* Field(FieldId) | Hyperlink(NodeId) */, href: String, tooltip: Option<String> },
    UpdateBlockField { field: FieldId, result: Vec<NewBlock> },

    // Span
    AddBookmark { name: String, from: InlinePos, to: InlinePos },
    RemoveBookmark { span: SpanId },
    AddComment { from: InlinePos, to: InlinePos, comment: NewComment },
    RemoveComment { span: SpanId },
    SetCommentText { span: SpanId, text: String, done: Option<bool> },

    // 修订
    AcceptRevision { rev: RevisionId }, RejectRevision { rev: RevisionId },
    AcceptAll, RejectAll,

    // 节与页眉页脚
    SetSectionProps { sect: NodeId, patch: SectionPropsPatch },
    SetHeaderFooter { sect: NodeId, kind: HfKind, variant: HfVariant, content: Vec<NewBlock> },

    // 其他 part
    SetNoteContent { kind: NoteKind, id: String, content: Vec<NewBlock> },
    SetSdtContent { sdt: NodeId, inlines: Vec<NewInline> },
    SetChartData { chart: MediaId, patch: ChartPatch },
    SetDocumentSettings { patch: SettingsPatch },
}
```

`add_media(bytes, mime) -> MediaId` 是会话方法而非操作。

### 8.3 执行语义

执行分三步，操作不直接手改模型：

```rust
pub struct MutationPlan {
    node_edits: Vec<NodeEdit>, anchor_moves: Vec<(SpanId, Anchor)>, revisions: Vec<NewRevision>,
    rel_edits: Vec<RelEdit>, allocated_ids: IdAllocations,   // rId / w:id / docPr id 在 plan 阶段全部分配完
}
pub struct MutationResult { dirty_nodes: SmallVec<[NodeId; 4]>, affected_containers: SmallVec<[NodeId; 2]>, affected_spans: SmallVec<[SpanId; 2]> }
// EditOp → plan(op, &session) -> MutationPlan → plan.validate(&session)? → session.commit(plan) -> MutationResult → semantic.refresh(&result)
```

**原子性**：`plan` 只读；`validate` 在不改任何状态的前提下完成 id 分配、命名空间检查、relationship 检查、Anchor 变换预测与 schema 合法性检查；`commit` 只做已验证过的机械写入，不再可能失败（除 OOM）。任何一步 `Err` 都不留下半修改状态。禁止"边做边发现问题"。

`refresh` 只重建 `affected_containers` 覆盖的段落/块投影。修订生成属于计划的一部分（`track_changes` 开启时）：`InsertText` 产生 `w:ins` 包裹的新 run；`DeleteRange` 把被删 run 改为 `w:del` 包裹并把 `w:t` 换成 `w:delText`；属性修改产生 `rPrChange`/`pPrChange` 并携带旧值快照；段落合并产生段落标记删除。关闭时直接修改。

第一阶段兼容 `SaveBlock[]`：`original` → 节点保持 `Clean`；`generated` → `ReplaceInlines` + `SetParaProps`；`xml` → 解析片段为 `New` 子树插入。

---

## 9. 保存

### 9.1 保存前校验

Span 完整性（5.6）、字段完整性、关系完整性、命名空间有效性、`[Content_Types].xml` 覆盖所有新 part。每条发现带来源：

```rust
pub enum ValidationOrigin { PreExistingDamage, EngineInvariantViolation }
```

- `PreExistingDamage`（输入文件本来如此）→ 容错修复并记诊断。
- `EngineInvariantViolation`（本次编辑造成）→ 调试构建与 CI 下报错，发布构建下修复并记诊断，绝不静默。否则编辑引擎的 bug 会被校验器掩盖，测试照样通过。

### 9.2 序列化

```
serialize(node):
  Clean           → 拷 lex.range
  DescendantDirty → 拷 lex.open；子节点逐个 serialize（跳过 Deleted）；拷 lex.close
  SelfDirty       → 重建开标签（原属性按原序、原引号；QName 经 NamespaceContext）；子节点逐个 serialize；重建闭标签
  New             → 按 schema 生成整棵子树（QName 经 NamespaceContext；属性容器按属性表顺序）
  Deleted         → 空
```

- 属性容器（`rPr/pPr/tcPr/tblPr/trPr/sectPr`）的 `SelfDirty` 走属性表合并：未建模子元素原字节留在原位，建模子元素按 schema 序号替换或插入。
- 重生成的 `w:t` 一律带 `xml:space="preserve"`；干净的 `w:t` 原字节。
- 新子树需要的未绑定前缀在子树根内联声明；需要进 `mc:Ignorable` 的才改根。

### 9.3 包写回

- 仅重写含脏节点的 part；其余 zip 条目用 `zip::ZipWriter::raw_copy_file` 直接拷压缩数据，保持条目顺序。
- 新增 part 通过同一 DOM 机制修改 `[Content_Types].xml` 与对应 `.rels`。
- 无脏节点且无新增 part → 直接返回原字节。
- `docProps/core.xml` 修改时间与 `removePersonalInformation` 为选项。

---

## 10. 借鉴对照

| 来源 | 决策 |
| --- | --- |
| LibreOffice writerfilter `ooxml/`：命名空间 token 表 | 借 |
| LibreOffice oox/core：MCE 处理 | 借 |
| LibreOffice DOCX 导出器：schema 顺序、必填属性、Word 容忍度 | 借 |
| LibreOffice sw/qa/extras：真实 bug 语料与测试方式 | 借（复用文件前确认许可） |
| LibreOffice oox drawingml/vml：颜色变换、VML 形状与样式表 | 借算法与表 |
| LibreOffice DomainMapper 实现 | 不借：不引入 OOXML → Writer/UNO 的全量映射层与布局兼容逻辑；Rust 直接由 DOM / Span / 属性表 / Facts 构造自身模型，`resolve/` 独立处理有效语义 |
| LibreOffice Writer/UNO 映射、TableManager、布局兼容 hack | 不借 |
| compatibility 事实的识别 | 借思想，自己建模（`CompatFacts`） |
| 有效语义解析 | 自己实现 `resolve/`，规范规则 + Word 实测 fixture |
| python-docx / docx4j：就地修改 DOM、未知节点自然保留 | 借思想；不借全量重序列化 |
| genoffice docx-engine：保真策略、保护块降级、限额、兼容性细节清单、测试场景 | 保留 |
| ECMA-376 §17.16 / [MS-OI29500]：字段关键字与开关全集、toggle 差异说明 | 数据来源 |

---

## 11. 测试

1. **差分**：TS 脚本把 77 个测试文件的合成 docx 落盘为 `.docx` + 期望 JSON；Rust `compat_ts` 输出与之 diff。
2. **字节保真**：每个语料无编辑往返字节相同；编辑单节点后其他干净节点原文子串全部出现。
3. **保存 XPath 断言**：对生成的 `document.xml` 断言 schema 顺序、字段结构、rels 一致性。
4. **真实语料**：落盘语料目录，每个文档一个最小断言与一次往返。
5. **恶意输入**：现有 `hostile-input` 三场景 + tokenizer 模糊测试（cargo-fuzz）。
6. **属性表**：每行生成读/写/合并往返用例。
7. **字段层**：每种策略至少一个正向用例；跨段、嵌套、未闭合、`fldSimple` 嵌套、`delInstrText`、锁定字段。
8. **随机编辑序列**：随机文档 × 随机 `EditOp` 序列（插字、删 run、设粗体、拆合 run、插字段、删段、改单元格）× 保存 × 重解析 × 语义断言，多轮迭代。每步之后断言增量刷新的模型等于 `Document::rebuild()` 从 DOM + Span 完整重建的结果（投影一致性 oracle），并在调试构建下把任何 `EngineInvariantViolation` 视为测试失败。
9. **resolve 校准**：真实 `.docx` + Word 实际显示的断言，覆盖 toggle、默认样式、表格条件格式、节继承。

---

## 12. 里程碑

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| M0 | L0（含 `PackageFlavor`、`RelTarget`、`NamespaceContext`）+ L1（tokenizer、`Node`/`Dirty`、MCE 含 `ProcessContent`、schema 名字表）；`serialize` 干净拷贝 | 任意语料 parse→serialize 字节相同（含 Strict） |
| M1 | 属性表 + `RunProps`/`ParaProps` 读写合并（按 flavor 编解码）；文本段落、heading、list；`compat_ts` 骨架；`DescendantDirty`/`SelfDirty` 序列化 | 文本段落 JSON 与 TS 一致；改一段文字往返满足不变式 2；Strict 文档改文字后仍为 Strict |
| M2 | L2：`Anchor`/`RangeSpan` 全部类型 + 字段子系统全部策略；批注/修订解析/书签/ruby/数学/noteRef 原子；符号字体 | 字段与 Span 测试场景；含字段段落可编辑 |
| M3 | 表格（嵌套、样式条件格式走 `resolve`）、`SdtInfo`（含 dataBinding/lock/docPart） | 表格测试场景；改单元格文本往返 |
| M4 | 绘图显示模型（不含排版启发式）、图片原子、VML、颜色算法 | 绘图测试场景（断言改为文档事实） |
| M5 | 页眉页脚复用正文管线、脚注尾注、参考文献、fontTable、节、保护、`CompatFacts`；`resolve/` 首版含 toggle 校准 fixture | hf/notes/sections 场景；toggle fixture 通过 |
| M6 | 图表、SmartArt、lockedCanvas、OLE、`MediaStore` | chart/smartart/ole 场景 |
| M7 | L4 + 保存：`EditOp` 全集、Span 变换、修订生成（`track_changes`）、保存前校验、部件写回、`SaveBlock[]` 兼容；与 `saveDocx` 差分 | 现有 roundtrip/text-patch/table-edit/textbox-edit/ai-track-revisions 场景通过；随机编辑序列测试通过 |
| M8 | 编辑器切换到 Rust 引擎（`compat_ts`） | e2e 通过 |
| M9 | 新模型 JSON、原生 `EditOp` 接口、媒体句柄、渲染器接管排版启发式；删除 `compat_ts` | 编辑器迁移完成 |

---

## 13. 核心类型索引

| 类型 | 定义位置 | 不变式 |
| --- | --- | --- |
| `Node` / `Lex` / `Attr` / `QName` | 4.1 | `lex` 与 `src` 同一字节体系；`Clean` 节点的 `lex.range` 即其输出；`QName` 是语义身份，`Lex.name`/`Attr.lex_name` 保留原始前缀 |
| `Dirty` | 4.2 | 非 `Clean` 节点的祖先不为 `Clean`；`Deleted` 节点不输出但保留在 arena；同 part 移动须过 `namespace_compatible`，跨 part 只能 `rehome` 为 `New` |
| `Anchor` / `Affinity` | 5.2 | `index` 是内容序列的逻辑边界，标记不计入；起点默认 `Right`、终点默认 `Left`；编辑期 Anchor 是事实，标记是投影；`dom.compare` 定义文档序；两端 `FlowId` 相同 |
| `RangeSpan` / `RangeKind` | 5.3 | 平铺列表，允许交叠；起止同 part，起在终前 |
| `FieldSpan` / `FieldForm` / `Instruction` / `FieldPolicy` | 5.4 | 正确嵌套；`instr_nodes` 原字节是保存真相；`Block` 策略覆盖所有跨段字段 |
| `EditOp` / `EditContext` / `InlinePos` | 8.1–8.3 | 偏移单位 UTF-16 code unit，原子为 `U+FFFC` 占 1；操作经 `MutationPlan` plan → validate → 原子 commit 作用于 DOM + Span，模型只做投影刷新；`track_changes` 开启时生成修订而不直接改内容 |
| `Run` / `Segment` | 6.3 | 与物理 `w:r` 一一对应；`segments` 覆盖全部文本且不重叠；逻辑合并只在投影层 |

---

## 14. 风险与开放项

- **schema 顺序表完整性**：从 XSD 生成并用 LO 输出交叉校验；条件顺序需人工审阅。
- **Strict 生成成本**：属性表编解码与 `NamespaceContext` 从 M0/M1 就要按 flavor 工作；Strict 禁用 VML，水印等 VML 生成在 Strict 包中需要 DrawingML 替代或拒绝并诊断。
- **`compat_ts` 成本**：复现今天的半解析字体字段、控制字符、dataURL 是纯负担，限定一个模块并设删除期限（M9）。
- **toggle 与 resolve 校准**：规范文字有歧义，必须以 Word 实测 fixture 为准，M5 前建好 fixture 集。
- **跨段字段编辑边界**：`Block` 结果段落只读，与今天一致；放开需逐字段评估生成器能力。
- **DOM 内存**：arena 节点约 48–64 字节，100 MB `document.xml` 约 200 万节点，可接受；文本用区间不复制。
- **EMF/WMF**：Rust 侧暂不转换；长期方案（移植到 `tiny-skia`、FFI 复用 JS 转换器）待定。
- **基线之后 TS 的变化**：`ooxml-normalize.ts`（装载时归一化为 Transitional）与本方案的 Strict 策略相反，M8 切换时 Strict 文档的行为会改变，需在发布说明中写明；`font-table.ts` 已纳入 6.7。
