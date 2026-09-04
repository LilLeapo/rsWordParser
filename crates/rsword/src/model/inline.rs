//! 内联模型与坐标流（`MOD-06`，`docs/03` §6.3、§8.1）。
//!
//! `Run` 与物理 `w:r` 一一对应；`segments` 覆盖 run 的全部子节点、按顺序、区间不重叠，
//! 给出坐标流中的文本偏移到子节点的映射。偏移单位对外是 UTF-16 code unit，
//! 内部字符串是 UTF-8，`utf16_len` 缓存每段长度。

use std::ops::Range;

use crate::semantic::props::RunProps;
use crate::span::{FieldId, SpanId};
use crate::xml::{NodeId, QName};

/// 坐标流里代表一个原子（图片、字段、公式、分页符……）的字符，占 1 个 UTF-16 单位。
pub const OBJECT_REPLACEMENT: char = '\u{FFFC}';

#[derive(Debug, Clone, PartialEq, Eq)]
// `Run` 是绝对多数，装箱只会多一次分配；`Atom` / `Field` 少见，接受尺寸差。
#[allow(clippy::large_enum_variant)]
pub enum Inline {
    Run(Run),
    /// 原子形态的字段（`FLD-07`）：坐标流中 1 个 `U+FFFC`，`result` 不参与坐标。M2 建立。
    Field {
        id: FieldId,
        result: Vec<Inline>,
    },
    /// 段落级非 `w:r` 子节点（公式、裸 `w:br`、未知元素）。
    Atom(InlineAtom),
}

impl Inline {
    /// 坐标流贡献（UTF-16 单位）。
    pub fn utf16_len(&self) -> u32 {
        match self {
            Inline::Run(r) => r.utf16_len,
            Inline::Field { .. } | Inline::Atom(_) => 1,
        }
    }

    pub fn node(&self) -> Option<NodeId> {
        match self {
            Inline::Run(r) => Some(r.node),
            Inline::Field { .. } => None,
            Inline::Atom(a) => Some(a.node),
        }
    }

    /// 坐标流文本追加到 `out`。
    pub fn append_text(&self, out: &mut String) {
        match self {
            Inline::Run(r) => out.push_str(&r.text),
            Inline::Field { .. } | Inline::Atom(_) => out.push(OBJECT_REPLACEMENT),
        }
    }
}

/// 一个物理 `w:r`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub node: NodeId,
    /// 覆盖全部子节点（`w:rPr` 除外），按顺序，`text` 区间不重叠。
    pub segments: Vec<Segment>,
    /// 坐标流中该 run 的文本。
    pub text: String,
    pub utf16_len: u32,
    /// 声明值（`w:rPr`）。
    pub props: RunProps,
    /// 所在的 `w:hyperlink`，或透明字段。
    pub link: Option<Link>,
    /// 透明字段（`Link` 策略）的 id；结构 run 也带它，段长度为 0。M2 建立。
    pub field: Option<FieldId>,
    /// 祖先 `w:ins/w:del/w:moveFrom/w:moveTo` 与自身 `rPrChange`。
    pub rev: Option<RevisionCtx>,
    /// 覆盖该 run 的批注范围（由 Span 索引反查，M2）。
    pub comments: Vec<SpanId>,
}

impl Run {
    /// `text` 中某段的字符串。
    pub fn segment_text(&self, seg: &Segment) -> &str {
        &self.text[seg.text.start as usize..seg.text.end as usize]
    }
}

/// run 的一个子节点在坐标流中的投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub node: NodeId,
    pub kind: SegmentKind,
    /// 在 `Run.text` 中的字节区间（长度 0 的段也有位置）。
    pub text: Range<u32>,
    pub utf16_len: u32,
}

/// `w:br/@w:type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    TextWrapping,
    Page,
    Column,
}

impl BreakKind {
    pub fn parse(s: Option<&str>) -> BreakKind {
        match s.map(str::trim) {
            Some("page") => BreakKind::Page,
            Some("column") => BreakKind::Column,
            _ => BreakKind::TextWrapping,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKind {
    Text,
    DelText,
    Tab,
    PTab {
        align: Option<String>,
    },
    Br {
        kind: BreakKind,
        clear: Option<String>,
    },
    Cr,
    NoBreakHyphen,
    SoftHyphen,
    /// `w:sym`：`font` 与 `w:char` 的十六进制码；显示解码在 `RES-05`。
    Sym {
        font: Option<String>,
        code: Option<u32>,
    },
    Drawing {
        anchored: bool,
    },
    Pict,
    Object,
    Ruby {
        rt: String,
    },
    FootnoteRef {
        id: Option<String>,
    },
    EndnoteRef {
        id: Option<String>,
    },
    /// 脚注 / 尾注正文里的编号标记（`w:footnoteRef`）。
    FootnoteRefMark,
    EndnoteRefMark,
    Separator,
    ContinuationSeparator,
    CommentRef,
    LastRenderedPageBreak,
    FldChar,
    InstrText,
    DelInstrText,
    AnnotationRef,
    Other(QName),
}

impl SegmentKind {
    /// 该段是否在坐标流里占位（长度可能为 0 的段：结构标记）。
    pub fn is_zero_width(&self) -> bool {
        matches!(
            self,
            SegmentKind::FldChar
                | SegmentKind::InstrText
                | SegmentKind::DelInstrText
                | SegmentKind::CommentRef
                | SegmentKind::LastRenderedPageBreak
                | SegmentKind::AnnotationRef
                | SegmentKind::FootnoteRefMark
                | SegmentKind::EndnoteRefMark
        )
    }
}

/// 段落级非 `w:r` 子节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAtom {
    pub node: NodeId,
    pub kind: AtomKind,
    /// 用于新输入继承格式；M1 为默认。
    pub props: RunProps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomKind {
    /// `m:oMath` / `m:oMathPara`（占位，`FormulaDisplay` 在 M3）。
    Math,
    /// run 外的 `w:br`。
    BareBreak {
        kind: BreakKind,
    },
    Other(QName),
}

/// 超链接来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Hyperlink { node: NodeId, target: LinkTarget, tooltip: Option<String> },
    Field(FieldId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// `w:anchor`：文内书签。
    Internal { anchor: String },
    /// `r:id`：`href` 是关系的外部目标（关系缺失或不是外部目标时为 `None`）。
    External { rel_id: String, href: Option<String> },
    /// 两者都没有。
    Unresolved,
}

/// 一条修订的元数据（`w:id` / `w:author` / `w:date`）。定义在 L2（范围标记用同一组属性）。
pub use crate::span::RevisionMeta;

/// run 的修订上下文（`MOD-06`）：`w:moveFrom` 同时计入 `del`，`w:moveTo` 同时计入 `ins`（TS 语义），
/// `move_*` 保留精确信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionCtx {
    pub ins: Option<RevisionMeta>,
    pub del: Option<RevisionMeta>,
    pub move_from: Option<RevisionMeta>,
    pub move_to: Option<RevisionMeta>,
    /// 自身 `w:rPrChange`：元数据与旧值快照。
    pub props_change: Option<(RevisionMeta, Box<RunProps>)>,
}

impl RevisionCtx {
    pub fn is_empty(&self) -> bool {
        self.ins.is_none()
            && self.del.is_none()
            && self.move_from.is_none()
            && self.move_to.is_none()
            && self.props_change.is_none()
    }
}

/// UTF-16 长度。
pub fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}
