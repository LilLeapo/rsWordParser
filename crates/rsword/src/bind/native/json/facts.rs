//! 段落事实的 JSON 投影（`BIND-02`，`MOD-04`）：对 `w:p` 一次遍历得到的事实，分类
//! （`MOD-05`）与 `TextKind` 判定（`MOD-03`）的输入。bool 一律 `flag` 行——为真才写
//! `true`，缺席即假。

use crate::model::block::{ListRef, SdtInfo};
use crate::model::{
    DrawingFacts, DrawingKind, MathFacts, ParagraphFacts, PictFacts, PictKind, RevisionFacts,
};
use crate::span::FieldId;
use crate::xml::NodeId;

use super::{as_str_json, json_str_enum, model_json};

// `DrawingKind` 是 `named_enum!`，`as_str` 即 JSON 值（`picture` / `chart` / …）。
as_str_json!(DrawingKind);

json_str_enum! {
    /// 一个 `w:pict` 是哪一类（`MOD-05` 的 R15–R18 要用；判定优先级照抄 TS，非文档序）。
    PictKind test json_fields_cover_pict_kind {
        ImageData => "imageData";
        TextBox => "textBox";
        WordArt => "wordArt";
        /// `v:rect[@o:hr]`。
        Hr => "hr";
        ShapeTypeOnly => "shapeTypeOnly";
        /// `visibility:hidden`。
        Hidden => "hidden";
        Other => "other";
    }
}

model_json! {
    /// 对 `w:p` 一次遍历得到的事实（`MOD-04`，`docs/03` §6.2）。
    struct ParagraphFacts(cx) test json_fields_cover_paragraph_facts {
        flag has_sect_pr => "hasSectPr" = has_sect_pr;
        /// 任一 `w:t`/`w:delText` trim 后非空（不含 `w:txbxContent` 内）。
        flag visible_text => "visibleText" = visible_text;
        /// 同上，且排除所有绘图 / VML / 对象内容。
        flag visible_text_outside_boxes => "visibleTextOutsideBoxes" = visible_text_outside_boxes;
        fields => "fields", Vec<FieldId> = fields;
        opt inside_field_result => "insideFieldResult", FieldId = inside_field_result;
        drawings => "drawings", Vec<DrawingFacts> = drawings;
        picts => "picts", Vec<PictFacts> = picts;
        /// `w:object` 节点（文档序）。
        objects => "objects", Vec<NodeId> = objects;
        math => "math", MathFacts = math;
        revision => "revision", RevisionFacts = revision;
        opt style_id => "styleId", String = style_id;
        /// 样式链 `vanish == true` 且段落里没有把它关掉、没有必须显示的内容。
        flag style_vanish => "styleVanish" = style_vanish;
        /// styleId 匹配 `^TOC ?([1-9])$`。
        opt toc_style_level => "tocStyleLevel", u8 = toc_style_level;
        opt numbering_ref => "numberingRef", ListRef = numbering_ref;
        /// `MOD-03`：直接 `outlineLvl` 0–8 → +1；否则样式链；否则 styleId 匹配。
        opt outline_level => "outlineLevel", u8 = outline_level;
        opt sdt => "sdt", SdtInfo = sdt;
        /// 段落里有 `w:vanish w:val="0|false|off"`（把样式的隐藏关掉）。
        flag unvanish => "unvanish" = unvanish;
        /// 段落里有书签起点或批注范围标记（TS `staysVanished` 的排除项）。
        flag has_range_marker => "hasRangeMarker" = has_range_marker;
    }
}

model_json! {
    /// 一个 `w:drawing` 的粗事实（`MOD-04`；种类按 `graphicData/@uri` 与子元素命名空间判定）。
    struct DrawingFacts(cx) test json_fields_cover_drawing_facts {
        node => "node", NodeId = node;
        kind => "kind", DrawingKind = kind;
        /// `wp:anchor`（否则 `wp:inline`）。
        flag anchored => "anchored" = anchored;
        flag has_txbx_text => "hasTxbxText" = has_txbx_text;
        flag has_blip => "hasBlip" = has_blip;
        /// `wp:docPr/@name` 以 `aidocs-ink` 开头。
        flag is_ink => "isInk" = is_ink;
        /// chartex 绘图配的预渲染图（`mc:Fallback` 里带 `a:blip` 的 `w:drawing`）；
        /// 只对 `ChartEx` 非 `None`。
        opt fallback_picture => "fallbackPicture", NodeId = fallback_picture;
    }
}

model_json! {
    /// 一个 `w:pict` 的粗事实（`MOD-04`）。
    struct PictFacts(cx) test json_fields_cover_pict_facts {
        node => "node", NodeId = node;
        kind => "kind", PictKind = kind;
    }
}

model_json! {
    /// 段落直接内容里的公式事实（`MOD-04`；不含绘图 / 文本框内）。
    struct MathFacts(cx) test json_fields_cover_math_facts {
        /// `m:oMath` 数。
        count => "count", u32 = count;
        flag omath_para => "omathPara" = omath_para;
    }
}

model_json! {
    /// 段落内的修订粗事实（`MOD-04`）。
    struct RevisionFacts(cx) test json_fields_cover_revision_facts {
        flag run_ins => "runIns" = run_ins;
        flag run_del => "runDel" = run_del;
        flag move_from => "moveFrom" = move_from;
        flag move_to => "moveTo" = move_to;
        flag del_instr_text => "delInstrText" = del_instr_text;
        flag para_mark_ins => "paraMarkIns" = para_mark_ins;
        flag para_mark_del => "paraMarkDel" = para_mark_del;
        flag ppr_change => "pprChange" = ppr_change;
    }
}
