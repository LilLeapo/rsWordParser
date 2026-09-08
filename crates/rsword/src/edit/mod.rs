//! L4 编辑引擎（`spec/08-edit.md`，`docs/03` §8）。
//!
//! `EditOp → plan（只读）→ validate（只读）→ commit（机械写入，不可失败）→ model.refresh`。
//! 任何一步 `Err` 都不留下半修改状态（`EDIT-05`）：[`EditSession::apply`] 在操作前对主 part 的 DOM
//! 做快照，操作内部可以有多个 plan/commit 阶段（例如先拆 run 再改属性），任一阶段失败整体回滚。
//! 偏移单位对外统一为 UTF-16 code unit，原子为一个 `U+FFFC`（`EDIT-02`）。
//!
//! M1（任务 1.11–1.13）覆盖：`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、
//! `ReplaceInlines`、`ReplaceParaProps` 与块级 `InsertBlock` / `DeleteBlock` / `MoveBlock`（compat 路径），
//! 不生成修订；范围标记不做 Anchor 变换（`SPAN-06` 在 M2），删除范围覆盖到标记时标记原地保留并记
//! `EDIT_ANCHOR_UNMOVED`（`EngineInvariantViolation`）。

pub mod atom_ops;
pub mod chart_ops;
pub(crate) mod diff;
pub mod drawing_ops;
pub mod field_ops;
pub mod ink_ops;
pub mod inline;
pub mod media_ops;
pub mod note_ops;
pub mod ops;
pub mod plan;
pub mod pos;
pub(crate) mod revision_ops;
pub mod sdt_ops;
pub mod section_ops;
pub mod session;
pub mod shape_gen;
pub mod table_ops;
pub(crate) mod track;
pub(crate) mod twin;

pub use chart_ops::{ChartPatch, ChartSeriesPatch, NewChart, NewChartKind, NewChartSeries};
pub use drawing_ops::{AnchorAxis, AnchorPos, AxisPos, DrawingGeometry, SrcRect};
pub use field_ops::{BlockFieldOptions, NewBlockField};
pub use ink_ops::{InkSave, NewInk};
pub use inline::{NewInline, NewLinkTarget, NewMarker, NewRevision, NewRun};
pub use media_ops::{ImageWrap, NewImage, ParaSpacing, PosOffset};
pub use plan::{MutationPlan, MutationResult};
pub use pos::{InlinePos, Loc, Utf16Offset, inline_spans, locate};
pub use session::EditSession;
pub use shape_gen::{LineKind, PresetGeom, ShapeLook};

use crate::model::{HfKind, HfVariant};
use crate::package::PartId;
use crate::semantic::props::{
    CellPropsPatch, ParaPropsPatch, RowPropsPatch, RunProps, RunPropsPatch, SectionPropsPatch,
    SettingsPatch, TablePropsPatch,
};
use crate::span::FieldId;
use crate::xml::{NewElement, NodeId};

/// 修订作者（`track_changes` 开启时写入 `w:author` / `w:date`）。M1 不生成修订，字段保留供 M7。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionAuthor {
    pub author: String,
    pub date: Option<String>,
}

/// `EDIT-01`：一次操作的上下文。
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(default)]
pub struct EditContext {
    /// `Some` → 生成修订（M7）；M1 忽略并按直接修改执行。
    pub track_changes: Option<RevisionAuthor>,
    /// 新 run 没有可继承的左侧 run 时使用的格式。
    pub default_run_props: Option<RunProps>,
    pub keep_orphan_comments: bool,
    pub mark_updated_fields_dirty: bool,
}

/// `EDIT-02`：块位置在容器里的落点。`End(body)` 落在尾部 `w:sectPr` 之前。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BlockAt {
    Start(NodeId),
    Before(NodeId),
    After(NodeId),
    End(NodeId),
}

/// `EDIT-02`：块位置 = 哪个 part + 落点。`part` 为 `None` 表示主 part（任务 5.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockPos {
    pub part: Option<PartId>,
    pub at: BlockAt,
}

impl BlockPos {
    /// 主 part 里的落点。
    pub fn main(at: BlockAt) -> Self {
        Self { part: None, at }
    }

    /// 指定 part 里的落点（页眉页脚 part 等）。
    pub fn in_part(part: PartId, at: BlockAt) -> Self {
        Self { part: Some(part), at }
    }

    pub fn start(container: NodeId) -> Self {
        Self::main(BlockAt::Start(container))
    }

    pub fn before(block: NodeId) -> Self {
        Self::main(BlockAt::Before(block))
    }

    pub fn after(block: NodeId) -> Self {
        Self::main(BlockAt::After(block))
    }

    pub fn end(container: NodeId) -> Self {
        Self::main(BlockAt::End(container))
    }

    /// 落点涉及的节点（容器或参照块）。
    pub fn node(&self) -> NodeId {
        match self.at {
            BlockAt::Start(c) | BlockAt::End(c) => c,
            BlockAt::Before(n) | BlockAt::After(n) => n,
        }
    }
}

/// `InsertBlock` 的内容。
#[derive(Debug, Clone, PartialEq)]
pub enum NewBlock {
    /// 新段落：`props` 为完整的 `w:pPr`（`None` = 无 `pPr`）。
    Paragraph { props: Option<NewElement>, inlines: Vec<NewInline> },
    /// 新表格：`rows` × `cols`，`widths` 是各列宽（twips；缺省等分 9360），`style` 是 `tblStyle`，
    /// `header` 为真时首行带 `w:tblHeader`。每格一个空 `w:p`。
    Table { rows: u32, cols: u32, widths: Option<Vec<i32>>, style: Option<String>, header: bool },
    /// 任意块级片段（`w:p` / `w:tbl` / …），通常来自 [`crate::xml::parse_fragment`]。
    Xml(NewElement),
    /// 外层包裹（compat 侧显式给出的块级 `w:ins` / `w:del`，属性已填好）里放一个块。
    Wrapped { wrapper: NewElement, block: Box<NewBlock> },
    /// 新图表（任务 6.6）：图表 part + 内嵌工作簿 + 关系 + 绘图段落。`extent_emu` 缺省 5486400 × 3200400。
    /// 进入 `InsertBlock` 时先由 `chart_ops::materialize` 建好 part、换成 `Xml` 段落。
    Chart { chart: NewChart, extent_emu: Option<(i64, i64)> },
    /// 新图片（任务 6.7）：媒体 part（相同字节只建一个）+ `image` 关系 + 段落（随文或锚定）。
    Image(NewImage),
    /// 独立公式段（TS `mathParagraphXml`，7.5）：`omml` 是 `m:oMath` 的**内容**，
    /// `align` 是 `left` / `center` / `right`。
    MathPara { omml: NewMath, align: String },
    /// 新建浮动文本框（`spec/18` 7.7）：`wps:wsp` + `w:txbxContent`。Transitional 包发
    /// `mc:AlternateContent`（Choice + VML 孪生），Strict 包只发 Choice。`blocks` 为空时
    /// 放一个空格段（Word 不接受空文本框）。
    Textbox { look: ShapeLook, blocks: Vec<NewBlock> },
    /// 新建形状：`preset` 进 `a:prstGeom/@prst`。`text` 给了就当一个段落放进框里，
    /// 不给就没有 `wps:txbx`（纯图形）。
    Shape { preset: PresetGeom, look: ShapeLook, text: Option<String> },
    /// 新建线条 / 连接符（TS `LINE_KINDS`）：两点（EMU）定位置与大小，只有描边。
    /// 没有 VML 孪生（TS 同），永远浮在文字上（`wrapNone`）。
    Line { kind: LineKind, from: (i64, i64), to: (i64, i64), color: Option<String> },
    /// 新块字段（`FLD-09`，7.8）：条目由当前文档算，一条一段（`FLD-08` / `FLD-12` 的多段字段
    /// 形态：begin + 指令 + separate 在首段开头、end 在末段末尾）。
    Field(NewBlockField),
    /// SEQ 题注段（TS `generateCaptionXml`）：`<标签> <SEQ 字段> <说明>`；
    /// 编号 = 插入位置之前同标签的 `SEQ` 字段数 + 1。
    Caption { label: String, text: String },
    /// 一次插入好几个块（生成器展开成多段时用）。只由 `chart_ops::materialize` 产出。
    Many(Vec<NewBlock>),
}

/// `EDIT-03 InsertAtom` 的内容（`spec/18` 7.5）。每一种在坐标流里都恒占 1 个 UTF-16 单位。
#[derive(Debug, Clone, PartialEq)]
pub enum NewAtom {
    /// `w:r/w:br`；`clear` 只对文字换行有意义。
    Break { kind: crate::model::inline::BreakKind, clear: Option<String> },
    /// `w:r/w:sym`：符号字体里的一个码位（`RES-05` 的反向）。
    Symbol { font: String, code: u32 },
    /// 脚注 / 尾注：新建条目（`w:id` 按 `EDIT-06`，part 不存在按 `SAVE-05` 建）+ 引用 run。
    NoteRef { endnote: bool, content: Vec<Vec<NewRun>> },
    /// 随文图片：run 内一个 `wp:inline`（不另起段落）。
    Image(NewImage),
    /// `m:oMath` 原子。
    Math(NewMath),
}

/// 公式的两种给法。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewMath {
    /// 现成的 OMML（可以是 `<m:oMath>…</m:oMath>`，也可以只给里面的内容）。
    Omml(String),
    /// LaTeX 源码，由 `model::omml::latex_to_omml` 转成 OMML。
    Latex(String),
}

/// `EDIT-03 AddComment` 的内容。`text` 里的 `\n` 分段。
#[derive(Debug, Clone, Default, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewComment {
    pub author: String,
    pub initials: Option<String>,
    /// ISO 时间戳；`None` 时不写 `w:date`。
    pub date: Option<String>,
    pub text: String,
    /// 回复哪条批注（写 `commentsExtended` 的 `w15:paraIdParent`）。
    pub parent_id: Option<String>,
    pub done: bool,
}

/// `docs/03` §8.2 的操作枚举（M1 子集）。
// 属性补丁（`ParaPropsPatch`）体积大；操作是一次性传入的值，不装箱。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EditOp {
    /// `EDIT-03 InsertText`：`props == None` 且紧邻 `Text` 段 → 写入该 `w:t`；否则边界插入继承格式的新 run。
    InsertText { at: InlinePos, text: String, props: Option<RunPropsPatch> },
    /// `EDIT-03 DeleteRange`（同段）。
    DeleteRange { from: InlinePos, to: InlinePos },
    /// `EDIT-03 SetRunProps`：两端拆分 run，范围内每个 run 按 `PROP-06` 计划 `rPr` 变更。
    SetRunProps { from: InlinePos, to: InlinePos, patch: RunPropsPatch },
    /// `EDIT-03 ReplaceInlines`（compat 路径）：段落全部内容子节点 `Deleted`，新内容为 `New`；`pPr` 不动。
    ReplaceInlines { part: Option<PartId>, para: NodeId, inlines: Vec<NewInline> },
    /// `EDIT-03 SetParaProps`：`PROP-06` 计划 `pPr` 变更（无 `pPr` → `New` 插为第一子）。
    SetParaProps { part: Option<PartId>, para: NodeId, patch: ParaPropsPatch },
    /// compat 路径：整个 `w:pPr` 替换为给定片段（`None` = 删除 `pPr`）。`EDIT-04` 的 `rawPPr` 语义。
    ReplaceParaProps { part: Option<PartId>, para: NodeId, props: Option<NewElement> },
    /// `EDIT-03 SetTableProps`：`w:tblPr` 按 `PROP-06` 合并（容器缺失时插为 `w:tbl` 第一个子元素）。
    SetTableProps { table: NodeId, patch: TablePropsPatch },
    /// `EDIT-03 SetRowProps`：`w:trPr` 按 `PROP-06` 合并（容器缺失时插在 `w:tblPrEx` 之后、首个 `w:tc` 之前）。
    SetRowProps { row: NodeId, patch: RowPropsPatch },
    /// `EDIT-03 SetCellProps`：`w:tcPr` 按 `PROP-06` 合并（容器缺失时插为 `w:tc` 第一个子元素）。
    SetCellProps { cell: NodeId, patch: CellPropsPatch },
    /// `EDIT-03 InsertRow`：在第 `at` 行前插入一行；`template` 缺省取 `at` 的前一行（`at == 0` 取第 0 行），
    /// `trPr` / `tblPrEx` / 各 `tcPr` 字节克隆，内容为一个空 `w:p`（克隆模板格首段的 `pPr`）。
    InsertRow { table: NodeId, at: u32, template: Option<NodeId> },
    /// `EDIT-03 DeleteRow`：删第 `at` 行；被删行的 `vMerge restart` 会把下一行的 continue 提升为 restart。
    DeleteRow { table: NodeId, at: u32 },
    /// `EDIT-03 InsertColumn`：在第 `at` 列前插入一列（`width` 是新列宽，twips）。
    InsertColumn { table: NodeId, at: u32, width: i32 },
    /// `EDIT-03 DeleteColumn`：删第 `at` 列。
    DeleteColumn { table: NodeId, at: u32 },
    /// `EDIT-03 MergeCells`：合并网格坐标闭区间 `from..=to`（行, 列）。
    MergeCells { table: NodeId, from: (u32, u32), to: (u32, u32) },
    /// `EDIT-03 InsertBlock`：`New` 子树。
    InsertBlock { at: BlockPos, block: NewBlock },
    /// `EDIT-03 DeleteBlock`：`Deleted`。`part` 为 `None` 表示主 part（任务 5.5）。
    DeleteBlock { part: Option<PartId>, node: NodeId },
    /// `EDIT-03 MoveBlock`：同 part 走 `move_within_part`；`from` 与 `to.part` 不同的时候
    /// 走 `XML-12` 规则 E′（子树按目标 part 的作用域重解析前缀，`spec/18` 7.6）。
    /// `from` 为 `None` 表示主 part。
    MoveBlock { from: Option<PartId>, node: NodeId, to: BlockPos },
    /// `EDIT-03 AddComment`（同段）：`comments.xml` 不存在则新建 part（`SAVE-05`），
    /// 正文里插范围标记与 `w:commentReference` run，`w:id` 按 `EDIT-06` 取最大值 + 1。
    AddComment { from: InlinePos, to: InlinePos, comment: NewComment },
    /// `EDIT-03 RemoveComment`：条目、范围标记与 reference run 一起删。
    RemoveComment { id: String },
    /// `EDIT-03 SetCommentText`：改条目正文（保留第一个文字 run 的格式）与 `w15:done`。
    SetCommentText { id: String, text: String, done: Option<bool> },
    /// `EDIT-03 SplitParagraph`：`at` 之后的内容搬进新段落（`pPr` 字节克隆）。
    /// 透明字段会因此跨段 → `Err(EDIT_SPLIT_FIELD)`。
    SplitParagraph { at: InlinePos },
    /// `EDIT-03 MergeWithNext`：下一段内容接到本段末尾，下一段删除（保留**前**段的 `pPr`）。
    MergeWithNext { part: Option<PartId>, para: NodeId },
    /// `EDIT-03 AddBookmark`（同段）：`w:id` 按 `EDIT-06` 取最大值 + 1；名字全文档唯一。
    AddBookmark { name: String, from: InlinePos, to: InlinePos },
    /// `EDIT-03 RemoveBookmark`：按名字删（标记 `Deleted`，索引里作废）。
    RemoveBookmark { name: String },
    /// `FLD-12 InsertField`：生成 begin / instrText / separate / 结果 / end 五组 run。
    InsertField { at: InlinePos, field: NewField },
    /// `FLD-07 Link`：改链接目标。HYPERLINK 字段只重写 `instrText`（开关原样保留）；
    /// `w:hyperlink` 元素改 `r:id`（外部 URL 按 `EDIT-06` 分配关系）或 `w:anchor`。
    SetLinkTarget { link: LinkRef, target: LinkDest },
    /// `FLD-10`：FORMCHECKBOX 的 `w:checked` 取反。
    ToggleCheckbox { field: FieldId },
    /// `FLD-10`：FORMTEXT 的结果文字。
    SetFormText { field: FieldId, text: String },
    /// `FLD-07`：字段结果 run 的格式。
    SetFieldResultProps { field: FieldId, patch: RunPropsPatch },
    /// `FLD-09`：用给定的块替换块字段的 `separate..end`（生成器在 M7；`w:fldLock` 拒绝）。
    UpdateBlockField { field: FieldId, blocks: Vec<NewBlock> },

    // ---- 图表与 part（`EDIT-03`，任务 6.6）------------------------------------------------------
    /// `EDIT-03 SetChartData`：改图表 part 里的缓存文本（标题 / 系列名 / 值 / 类别），结构与引用不动
    /// （`spec/08`「`chart.ts` 补丁语义」）；chartex part → `Err(EDIT_UNSUPPORTED)`。
    SetChartData { part: PartId, patch: ChartPatch },
    /// 整个 XML part 换成给定内容（TS `partXml`）。只接受已存在的 XML part（不存在 → `EDIT_TARGET_MISSING`）。
    ReplacePartXml { part: PartId, xml: String },
    /// 整个 part 换成给定字节（TS `partBinary`）。只接受已存在的 part，主 part 除外。
    ReplacePartBytes { part: PartId, bytes: Vec<u8> },
    /// `EDIT-04 ReplaceImageMedia`（TS `xml.replaceImage`，任务 6.7）：`drawing` 子树里第一个 `a:blip` 改指新媒体
    /// （字节按内容去重落成媒体 part），删裁剪窗、清填充窗、删 `svgBlip` 扩展。
    ReplaceImageMedia { drawing: NodeId, bytes: Vec<u8>, mime: String },

    // ---- 墨迹（`SAVE-07 inks`，任务 6.8）--------------------------------------------------------
    /// 删掉主 part 里全部 `aidocs-ink` 墨迹 run（`Document.inks`）；它们的媒体与关系随保存时的资源回收消失。
    RemoveInks,
    /// 在段落 `para` 的全部内容之后追加一条墨迹 run（TS `anchoredInkRunXml`；`docPr/@id` 按 `EDIT-06`，
    /// 媒体每条一个 part）。`para` 不是 `w:p`（表格 / sdt 外壳）→ 跳过 + 诊断，不分配媒体与关系。
    InsertInk { para: NodeId, ink: NewInk },

    // ---- 节与页眉页脚（`EDIT-03`，任务 5.5）--------------------------------------------------
    /// `EDIT-03 SetSectionProps`：给定 `w:sectPr` 按 `PROP-06` 合并（未建模的子元素原字节不动，
    /// 新元素按 CT_SectPr 顺序插入）。**新建分节符**（给段落加一个 `sectPr`）不在 M5。
    SetSectionProps { sect: NodeId, patch: SectionPropsPatch },
    /// `EDIT-03 SetHeaderFooter`：这一节这个变体的页眉页脚内容整体替换。
    ///
    /// 该节**自己声明**了这个变体（`RES-10` 的 `Declared`）→ 改写它引用的 part；没声明（含从上一节
    /// 继承）→ 按 `SAVE-05` 新建 `word/header{N}.xml` 并把引用插进这一节的 `sectPr`，这一节因此
    /// 独立、前面的节不受影响（Word 与 TS 的 `sectionHf` 语义）。
    SetHeaderFooter { sect: NodeId, kind: HfKind, variant: HfVariant, content: Vec<NewBlock> },
    /// 给一个没有该变体引用的节挂上**已有** part 的引用（TS 的 `hfAllSections`）。
    LinkHeaderFooter { sect: NodeId, kind: HfKind, variant: HfVariant, part: PartId },
    /// `SAVE-07 watermark`：这一节 default 页眉里的文字水印。`None` 删掉（连同页眉里所有
    /// 含 `v:textpath` 的段落）；页眉不存在时先建。Strict 包拒绝（VML 不在 Strict 里）。
    SetWatermark { sect: NodeId, text: Option<String> },
    /// `SAVE-07 pageColor`：`w:background/@w:color`（`w:document` 的第一个子元素）。`None` 删掉。
    SetPageColor { color: Option<String> },
    /// `EDIT-03 SetDocumentSettings`：`word/settings.xml` 按 `PROP-06` 合并（part 不存在就建）。
    SetDocumentSettings { patch: SettingsPatch },
    /// `EDIT-03 InsertAtom`：往坐标流里插一个原子（`spec/18` 7.5）。
    InsertAtom { at: InlinePos, atom: NewAtom },
    /// `EDIT-03 SetNoteContent`：整条脚注 / 尾注的正文段落换掉（自引用标记 run 保留）。
    SetNoteContent { endnote: bool, id: String, content: Vec<Vec<NewRun>> },
    /// `EDIT-03 RemoveNote`：删条目 + 正文里的引用 run。
    RemoveNote { endnote: bool, id: String },
    /// `EDIT-03 SetSdtContent`：内联内容控件里的内容整体换掉。
    SetSdtContent { sdt: NodeId, inlines: Vec<NewInline> },
    /// `EDIT-03 RemoveSdtShell`：Word 的「删除内容控件」——内容留下，`w:sdt` 消失。
    RemoveSdtShell { sdt: NodeId },
    /// `EDIT-03 SetMathTokens`（TS `patchMathTokens`）：按序替换 `m:oMath` 里每个 `m:t` 的文字；
    /// 个数不等 → `EDIT_MATH_TOKEN_COUNT`。
    SetMathTokens { math: NodeId, tokens: Vec<String> },
    /// `EDIT-03 SetDrawingGeometry`（`spec/18` 7.7）：尺寸 / 位置 / 旋转 / 翻转 / 裁剪。
    /// 只改属性，`a:graphic` 子树原字节。
    SetDrawingGeometry { drawing: NodeId, geom: DrawingGeometry },
    /// `EDIT-03 SetDrawingZOrder`：`relativeHeight = 251658240 + z`（只对锚定图片有意义）。
    SetDrawingZOrder { drawing: NodeId, z: i64 },
    /// `EDIT-03 SetDrawingWrap`（`spec/18` 7.7）：绕排方式。`wrap: None` = 随文（`wp:inline`），
    /// `Some(_)` = 锚定（`wp:anchor`）。壳换了也只重建壳：`wp:extent` / `effectExtent` /
    /// `docPr` / `cNvGraphicFramePr` / `a:graphic` 原字节搬过去（`SAVE-08`）。
    SetDrawingWrap {
        drawing: NodeId,
        wrap: Option<media_ops::ImageWrap>,
        /// 不给时：本来就锚定的保留原 `positionH` / `positionV`，随文转锚定的按绕排方向取缺省。
        pos: Option<AnchorPos>,
        /// 不给时：本来就锚定的保留原 `relativeHeight`，随文转锚定的取基数。
        z_order: Option<i64>,
    },
    /// `EDIT-03 SetShapeStyle`：`wps:spPr` 的填充与描边。`None` = 不动，`Some(None)` = 无。
    SetShapeStyle { shape: NodeId, fill: Option<Option<String>>, outline: Option<Option<String>> },
    /// `EDIT-03 RegenerateBlockField`（`spec/18` 7.8）：按生成器重算一个块字段的结果区
    /// （`TOC` / `INDEX`）。走 `UpdateBlockField` 那条机制，`w:fldLock` 一样拒绝。
    RegenerateBlockField { field: crate::span::FieldId, options: BlockFieldOptions },
    /// `EDIT-03 SetTextboxContent`（`spec/18` 7.7）：一个文本框里的块整体换掉。`textbox` 可以是
    /// `w:txbxContent` 自己，也可以是包着它的 `wps:wsp` / `wps:txbx` / `v:shape` / `v:textbox`。
    /// 落在 `mc:Choice` 里时 `mc:Fallback` 的 VML 孪生跟着同步。
    SetTextboxContent { textbox: NodeId, blocks: Vec<NewBlock> },
    /// `EDIT-03 InsertSectionBreak`（`spec/18` 7.6）：在 `after` 这一段之后断节。
    /// 该段的 `pPr` 里新建一个 `w:sectPr`（原节属性的克隆，含页眉页脚引用），
    /// 原来的 `sectPr` 从此描述**后**一节，它的 `w:type` 换成 `kind`。
    InsertSectionBreak { after: NodeId, kind: crate::semantic::props::SectType },
    /// `EDIT-03 DeleteSectionBreak`：删掉一个**段落级** `w:sectPr`，这些块并入后一节
    /// （Word 语义：合并后由后一节的页面设置接管）。body 级的 `sectPr` 不能删。
    DeleteSectionBreak { sect: NodeId },
    /// `EDIT-03 AcceptRevision`：接受一条修订（`Document.revisions` 里的 id）。
    AcceptRevision { rev: crate::model::RevisionId },
    /// `EDIT-03 RejectRevision`：拒绝一条修订。
    RejectRevision { rev: crate::model::RevisionId },
    /// `EDIT-03 AcceptAll`：接受全部修订；`author` 给定时只接受那个作者的
    /// （编辑器按作者接受，`docs/03` §8.2 之外的扩展，登记在 `docs/04` §8）。
    AcceptAll { author: Option<String> },
    /// `EDIT-03 RejectAll`：拒绝全部修订；`author` 同上。
    RejectAll { author: Option<String> },
    /// `BIND-03 v3`：参考文献权威列表，未变条目保留原字节。
    SetSources { sources: Vec<crate::save::options::decl::SourceSave> },
    /// `BIND-03 v3`：按 numId 追加，已存在即 no-op。
    AddNumberingDefinition { definition: crate::save::options::decl::NumberingDefSave },
    /// `BIND-03 v3`：按 numId 追加重启定义，已存在即 no-op。
    RestartNumbering { restart: crate::save::options::decl::RestartNumSave },
    /// `BIND-03 v3`：替换主题字体槽。
    SetThemeFonts { fonts: crate::save::options::decl::ThemeFontsSave },
    /// `BIND-03 v3`：替换主题配色槽。
    SetThemeColors { colors: crate::save::options::decl::ThemeColorsSave },
    /// `BIND-03 v3`：按 styleId upsert，相同请求不改状态。
    UpsertStyle { style: crate::save::options::decl::StyleUpsertSave },
}

/// `SetLinkTarget` 要改哪个链接。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LinkRef {
    /// HYPERLINK 字段。
    Field(FieldId),
    /// `w:hyperlink` 元素。
    Element(NodeId),
}

/// 链接目标。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LinkDest {
    /// 外部 URL。`w:hyperlink` 会先按 `EDIT-06` 分配一条外部关系。
    Url(String),
    /// 文内书签（字段写 `\l "name"`，元素写 `w:anchor`）。
    Anchor(String),
    /// 已有的关系 id（只用于 `w:hyperlink`）。
    Rel(String),
}

/// `FLD-12` 的新字段。`instr` 是指令原文（不含首尾空格，生成时补上）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewField {
    pub instr: String,
    /// 结果区内容（空 = 只有 begin..end 的空结果）。
    pub result: Vec<NewInline>,
    /// begin 的 `w:fldChar` 上打 `w:dirty="true"`，Word 打开时重算。
    pub mark_dirty: bool,
}
