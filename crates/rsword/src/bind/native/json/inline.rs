//! 内联模型与坐标流的 JSON 投影（`BIND-02`，`MOD-06`）：`Run` = 物理 `w:r`，坐标流 UTF-16，
//! 原子 `U+FFFC` 占 1。`Segment.display` 只在 `display: true` 投影（决策 4）。

use std::ops::Range;

use crate::model::drawing::Display;
use crate::model::inline::{
    AtomKind, BreakKind, Inline, InlineAtom, Link, LinkTarget, RevisionCtx, Run, Segment,
    SegmentKind,
};
use crate::semantic::props::RunProps;
use crate::span::{FieldId, RevisionMeta, SpanId};
use crate::xml::{NodeId, QName};

use super::{json_str_enum, model_json};

json_str_enum! {
    /// `w:br/@w:type`。
    BreakKind test json_fields_cover_break_kind {
        TextWrapping => "textWrapping";
        Page => "page";
        Column => "column";
    }
}

model_json! {
    /// 段落内容（`MOD-06`）。
    enum Inline(cx) test json_fields_cover_inline {
        /// 物理 `w:r`。
        Run(Run) => "run";
        /// 原子形态的字段（`FLD-07`）：坐标流 1 个 `U+FFFC`，`result` 不参与坐标。
        Field { id, result } => "field" {
            id => "id", FieldId = id;
            result => "result", Vec<Inline> = result;
        };
        /// 段落级非 `w:r` 子节点（公式、裸 `w:br`、未知元素）。
        Atom(InlineAtom) => "atom";
    }
}

model_json! {
    /// 一个物理 `w:r`（`MOD-06`）。
    struct Run(cx) test json_fields_cover_run {
        node => "node", NodeId = node;
        segments => "segments", Vec<Segment> = segments;
        text => "text", String = text;
        utf16_len => "utf16Len", u32 = utf16_len;
        /// 声明值（`w:rPr`）。
        props => "props", RunProps = props;
        opt link => "link", Link = link;
        /// 透明字段（`Link` 策略）的 id；结构 run 也带它。
        opt field => "field", FieldId = field;
        opt rev => "rev", RevisionCtx = rev;
        comments => "comments", Vec<SpanId> = comments;
    }
}

model_json! {
    /// run 的一个子节点在坐标流中的投影（`text` 是 `Run.text` 内的字节区间）。
    struct Segment(cx) test json_fields_cover_segment {
        node => "node", NodeId = node;
        kind => "kind", SegmentKind = kind;
        text => "text", Range<u32> = text;
        utf16_len => "utf16Len", u32 = utf16_len;
        /// 显示模型（`MOD-11`）：绘图 / VML / OLE 段才有；`display: true` 才投影。
        raw opt display => "display", Display = display.as_ref().filter(|_| cx.display).map(|d| d.to_json(cx));
    }
}

model_json! {
    /// `w:r` 子节点的分类（`MOD-06`）。
    enum SegmentKind(cx) test json_fields_cover_segment_kind {
        Text => "text";
        DelText => "delText";
        Tab => "tab";
        PTab { align } => "pTab" {
            opt align => "align", String = align;
        };
        Br { kind, clear } => "br" {
            ~ kind => "breakKind", BreakKind = kind, "与枚举内标签键 kind 撞名";
            opt clear => "clear", String = clear;
        };
        Cr => "cr";
        NoBreakHyphen => "noBreakHyphen";
        SoftHyphen => "softHyphen";
        /// `w:sym`：符号字体与码位；显示解码在 `RES-05`。
        Sym { font, code } => "sym" {
            opt font => "font", String = font;
            opt code => "code", u32 = code;
        };
        Drawing { anchored } => "drawing" {
            flag anchored => "anchored" = anchored;
        };
        Pict => "pict";
        Object => "object";
        /// `w:ruby`：注音文字与被注的正文；坐标流里是 1 个原子。
        Ruby { rt, base } => "ruby" {
            rt => "rt", String = rt;
            base => "base", String = base;
        };
        /// `aidocs-ink` 墨迹批注的浮动图片（几何与载荷见 `Document.inks`）。
        Ink => "ink";
        FootnoteRef { id } => "footnoteRef" {
            opt id => "id", String = id;
        };
        EndnoteRef { id } => "endnoteRef" {
            opt id => "id", String = id;
        };
        FootnoteRefMark => "footnoteRefMark";
        EndnoteRefMark => "endnoteRefMark";
        Separator => "separator";
        ContinuationSeparator => "continuationSeparator";
        CommentRef => "commentRef";
        LastRenderedPageBreak => "lastRenderedPageBreak";
        FldChar => "fldChar";
        InstrText => "instrText";
        DelInstrText => "delInstrText";
        AnnotationRef => "annotationRef";
        Other(QName) as "name" => "other";
    }
}

model_json! {
    /// 段落级非 `w:r` 子节点。
    struct InlineAtom(cx) test json_fields_cover_inline_atom {
        node => "node", NodeId = node;
        ~ kind => "atomKind", AtomKind = kind, "Inline::Atom 平铺时与枚举内标签键 kind 撞名";
        /// 用于新输入继承格式。
        props => "props", RunProps = props;
    }
}

model_json! {
    /// 段落级原子的分类。
    enum AtomKind(cx) test json_fields_cover_atom_kind {
        /// `m:oMath`。
        Math => "math";
        /// run 外的 `w:br`。
        BareBreak { kind } => "bareBreak" {
            ~ kind => "breakKind", BreakKind = kind, "与枚举内标签键 kind 撞名";
        };
        Other(QName) as "name" => "other";
    }
}

model_json! {
    /// 超链接来源。
    enum Link(cx) test json_fields_cover_link {
        /// `w:hyperlink` 元素。
        Hyperlink { node, target, tooltip } => "hyperlink" {
            node => "node", NodeId = node;
            target => "target", LinkTarget = target;
            opt tooltip => "tooltip", String = tooltip;
        };
        /// 透明字段（`FLD-07`）。
        Field(FieldId) as "field" => "field";
    }
}

model_json! {
    /// 链接目标。
    enum LinkTarget(cx) test json_fields_cover_link_target {
        /// `w:anchor`：文内书签。
        Internal { anchor } => "internal" {
            anchor => "anchor", String = anchor;
        };
        /// `r:id`：`href` 是关系的外部目标（缺失或非外部时为 `None`）。
        External { rel_id, href } => "external" {
            rel_id => "relId", String = rel_id;
            opt href => "href", String = href;
        };
        Unresolved => "unresolved";
    }
}

model_json! {
    /// run 的修订上下文（`MOD-06`）：`moveFrom` 计入 `del`、`moveTo` 计入 `ins`（TS 语义），
    /// `move_*` 保留精确信息。
    struct RevisionCtx(cx) test json_fields_cover_revision_ctx {
        opt ins => "ins", RevisionMeta = ins;
        opt del => "del", RevisionMeta = del;
        opt move_from => "moveFrom", RevisionMeta = move_from;
        opt move_to => "moveTo", RevisionMeta = move_to;
        /// 自身 `w:rPrChange`：`[元数据, 旧值快照]`（元组一律 `[a, b]`）。
        opt props_change => "propsChange", (RevisionMeta, Box<RunProps>) = props_change;
    }
}
