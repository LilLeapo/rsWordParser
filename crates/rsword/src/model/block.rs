//! 块模型（`MOD-02`、`MOD-03`、`MOD-08`、`MOD-09`，`docs/03` §6.3）。

use crate::model::drawing::Display;
use crate::model::facts::ParagraphFacts;
use crate::model::inline::{Inline, RevisionMeta};
pub use crate::model::sdt::SdtInfo;
pub use crate::model::table::TableBlock;
use crate::semantic::props::{CellProps, ParaProps, RowProps, RunProps, TableProps};
use crate::span::FieldId;
use crate::xml::{NodeId, QName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 装箱：`TextBlock`（含 `ParaProps` 与 facts）比其他变体大几十倍。
    Text(Box<TextBlock>),
    Table(TableBlock),
    Image(ImageBlock),
    Protected(ProtectedBlock),
}

impl Block {
    /// 块对应的节点（`w:p` / `w:tbl` / `w:sectPr` / 未知元素）。
    pub fn node(&self) -> NodeId {
        match self {
            Block::Text(b) => b.node,
            Block::Table(b) => b.node,
            Block::Image(b) => b.node,
            Block::Protected(b) => b.node,
        }
    }

    pub fn sdt(&self) -> Option<&SdtInfo> {
        match self {
            Block::Text(b) => b.sdt.as_ref(),
            Block::Table(b) => b.sdt.as_ref(),
            Block::Image(b) => b.sdt.as_ref(),
            Block::Protected(b) => b.sdt.as_ref(),
        }
    }

    pub fn revisions(&self) -> &[Revision] {
        match self {
            Block::Text(b) => &b.revisions,
            Block::Table(b) => &b.revisions,
            Block::Image(b) => &b.revisions,
            Block::Protected(b) => &b.revisions,
        }
    }

    pub fn as_text(&self) -> Option<&TextBlock> {
        match self {
            Block::Text(b) => Some(b),
            _ => None,
        }
    }
}

/// 可编辑段落。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub node: NodeId,
    pub kind: TextKind,
    pub style_id: Option<String>,
    /// 声明值（`w:pPr`），段落标记 rPr 在 `props.rpr`。
    pub props: ParaProps,
    pub inlines: Vec<Inline>,
    pub sdt: Option<SdtInfo>,
    /// 块级修订：`w:ins/w:del` 包裹、段落标记 ins/del、`pPrChange`。
    pub revisions: Vec<Revision>,
    pub facts: ParagraphFacts,
}

impl TextBlock {
    /// 坐标流文本（`MOD-06`）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        for i in &self.inlines {
            i.append_text(&mut s);
        }
        s
    }

    /// 坐标流长度（UTF-16 单位）。
    pub fn utf16_len(&self) -> u32 {
        self.inlines.iter().map(Inline::utf16_len).sum()
    }

    pub fn para_mark_props(&self) -> Option<&RunProps> {
        self.props.rpr.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextKind {
    Paragraph,
    Heading { level: u8 },
    ListItem { list: ListRef },
}

/// 编号引用（`MOD-03`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRef {
    pub num_id: i32,
    pub ilvl: i32,
    /// 来自段落样式链而非直接 `w:numPr`。
    pub from_style: bool,
}

/// 只含一张图片的段落（`MOD-05` R15）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBlock {
    pub node: NodeId,
    /// 该段唯一那个绘图的显示模型（`MOD-11`）。VML 图片（`w:pict`）的显示模型在 4.5。
    pub display: Option<Display>,
    pub sdt: Option<SdtInfo>,
    pub revisions: Vec<Revision>,
}

/// 只读块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedBlock {
    pub node: NodeId,
    pub kind: ProtectedKind,
    /// 可见文本预览（最多 80 个字符），供编辑器显示占位。
    pub preview: String,
    /// 显示载荷（`MOD-11`）：细横线 / 嵌入对象的 VML，图表与 SmartArt 的载荷在 M6。
    pub display: Option<Display>,
    pub sdt: Option<SdtInfo>,
    pub revisions: Vec<Revision>,
}

/// 保护原因。显示载荷挂在 [`ProtectedBlock::display`]；图表 / SmartArt 的载荷在 M6。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedKind {
    FieldBlockResult(FieldId),
    Equation,
    Chart,
    SmartArt,
    Ole,
    Rule,
    Invisible,
    SectionBreak,
    SectionProps,
    BodyBreak { page: bool },
    Unknown(QName),
    TooDeep,
    Unparseable,
}

impl ProtectedKind {
    /// i18n key（`docs/03` §6.3："`label` 变为 i18n key"）。
    pub fn key(&self) -> &'static str {
        match self {
            ProtectedKind::FieldBlockResult(_) => "protected.field_block_result",
            ProtectedKind::Equation => "protected.equation",
            ProtectedKind::Chart => "protected.chart",
            ProtectedKind::SmartArt => "protected.smart_art",
            ProtectedKind::Ole => "protected.ole",
            ProtectedKind::Rule => "protected.rule",
            ProtectedKind::Invisible => "protected.invisible",
            ProtectedKind::SectionBreak => "protected.section_break",
            ProtectedKind::SectionProps => "protected.section_props",
            ProtectedKind::BodyBreak { .. } => "protected.body_break",
            ProtectedKind::Unknown(_) => "protected.unknown",
            ProtectedKind::TooDeep => "protected.too_deep",
            ProtectedKind::Unparseable => "protected.unparseable",
        }
    }
}

/// 块级 / 段落标记修订（`MOD-09`）。run 级修订在 [`crate::model::inline::RevisionCtx`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// 顶层 `w:ins` 包裹的块；也用于 `trPr/ins`（整行插入，挂在 `Row.revisions`）。
    Insert(RevisionMeta),
    /// 顶层 `w:del` 包裹的块；也用于 `trPr/del`（整行删除）。
    Delete(RevisionMeta),
    MoveFrom(RevisionMeta),
    MoveTo(RevisionMeta),
    /// `pPr/rPr/ins`：段落标记被插入。
    ParaMarkInsert(RevisionMeta),
    /// `pPr/rPr/del`：段落标记被删除（与下一段合并）。
    ParaMarkDelete(RevisionMeta),
    /// `pPrChange`：旧值快照。
    ParaPropsChange {
        meta: RevisionMeta,
        old: Box<ParaProps>,
    },
    /// `numPr/numberingChange`。
    NumberingChange(RevisionMeta),
    /// `tblPr/tblPrChange`：表格属性旧值（`TableBlock.revisions`）。
    TablePropsChange {
        meta: RevisionMeta,
        old: Box<TableProps>,
    },
    /// `sectPr/sectPrChange`：旧值快照（`SectionInfo.revisions`，任务 5.2）。
    ///
    /// 与 `TablePropsChange` 一族同形（typed + `Box`）而不是 `spec/06` 早先写的 `old: NodeId`：
    /// 四个 `*PrChange` 同一形状，M7 的 Accept / Reject 就能共用一条 `plan_apply_*` 路径。
    /// 快照元素本身仍能从 `meta.node`（`w:sectPrChange`）一步走到，信息没丢。
    SectPropsChange {
        meta: RevisionMeta,
        old: Box<crate::semantic::props::SectionProps>,
    },
    /// `tblGrid/tblGridChange`：旧网格；`old` 是快照里的 `w:tblGrid`（没有就是 change 元素本身）。
    TableGridChange {
        meta: RevisionMeta,
        old: NodeId,
    },
    /// `trPr/trPrChange`（`Row.revisions`）。
    RowPropsChange {
        meta: RevisionMeta,
        old: Box<RowProps>,
    },
    /// `tcPr/tcPrChange`（`Cell.revisions`）。
    CellPropsChange {
        meta: RevisionMeta,
        old: Box<CellProps>,
    },
    /// `tcPr/cellIns`。
    CellInsert(RevisionMeta),
    /// `tcPr/cellDel`。
    CellDelete(RevisionMeta),
    /// `tcPr/cellMerge`。
    CellMerge(RevisionMeta),
}
