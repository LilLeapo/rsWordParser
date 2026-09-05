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

pub mod inline;
pub mod ops;
pub mod plan;
pub mod pos;
pub mod session;
pub mod table_ops;

pub use inline::{NewInline, NewLinkTarget, NewMarker, NewRevision, NewRun};
pub use plan::{MutationPlan, MutationResult};
pub use pos::{InlinePos, Loc, Utf16Offset, inline_spans, locate};
pub use session::EditSession;

use crate::package::PartId;
use crate::semantic::props::{
    CellPropsPatch, ParaPropsPatch, RowPropsPatch, RunProps, RunPropsPatch, TablePropsPatch,
};
use crate::span::FieldId;
use crate::xml::{NewElement, NodeId};

/// 修订作者（`track_changes` 开启时写入 `w:author` / `w:date`）。M1 不生成修订，字段保留供 M7。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionAuthor {
    pub author: String,
    pub date: Option<String>,
}

/// `EDIT-01`：一次操作的上下文。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditContext {
    /// `Some` → 生成修订（M7）；M1 忽略并按直接修改执行。
    pub track_changes: Option<RevisionAuthor>,
    /// 新 run 没有可继承的左侧 run 时使用的格式。
    pub default_run_props: Option<RunProps>,
    pub keep_orphan_comments: bool,
    pub mark_updated_fields_dirty: bool,
}

/// `EDIT-02`：块位置在容器里的落点。`End(body)` 落在尾部 `w:sectPr` 之前。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAt {
    Start(NodeId),
    Before(NodeId),
    After(NodeId),
    End(NodeId),
}

/// `EDIT-02`：块位置 = 哪个 part + 落点。`part` 为 `None` 表示主 part（任务 5.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// `EDIT-03 AddComment` 的内容。`text` 里的 `\n` 分段。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// `EDIT-03 MoveBlock`：同 part `move_within_part`。
    MoveBlock { node: NodeId, to: BlockPos },
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
}

/// `SetLinkTarget` 要改哪个链接。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRef {
    /// HYPERLINK 字段。
    Field(FieldId),
    /// `w:hyperlink` 元素。
    Element(NodeId),
}

/// 链接目标。
#[derive(Debug, Clone, PartialEq, Eq)]
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
