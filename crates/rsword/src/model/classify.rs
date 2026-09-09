//! 分类规则表（`MOD-05`）与 `TextKind` 判定（`MOD-03`）：都是 facts 的纯函数，按优先级首条命中。
//!
//! 与 TS 的 `buildBlock` 决策树不同处标 △（见 spec）。M1 只需 R01/R02(占位)/R07/R08/R10/R19，
//! 其余规则已按 facts 写出，但 M1 的 facts 里字段事实为空，R09 不会命中。

use crate::model::block::{ProtectedKind, TextKind};
use crate::model::facts::{DrawingKind, ParagraphFacts, PictKind};
use crate::span::{is_property_element, is_range_marker};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// body（或 sdtContent / 修订包裹）直接子节点的分类（R01–R07）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyClass {
    /// R01
    SectionProps,
    /// R02
    Table,
    /// R03：递归分类 `sdtContent` 的子节点
    Sdt,
    /// R04：不产生 Block
    RangeMarker,
    /// R05
    BodyBreak {
        page: bool,
    },
    /// R06：递归并附 `Revision`
    InsertWrap,
    DeleteWrap,
    MoveFromWrap,
    MoveToWrap,
    /// `w:customXml` / `w:smartTag` 块级包裹：透明递归（spec 未列；见 docs/04 §8）
    Transparent,
    /// 其他非 `w:p`（R07）
    Unknown(QName),
    /// `w:p`：继续按 facts 分类
    Paragraph,
}

pub fn classify_body_child(dom: &Dom, node: NodeId) -> (&'static str, BodyClass) {
    // 文本节点（缩进空白）不产生块
    let Some(name) = dom.name(node) else { return ("R04", BodyClass::RangeMarker) };
    if is_range_marker(name) || (name.ns == NsId::W && name.local == LocalName::ProofErr) {
        return ("R04", BodyClass::RangeMarker);
    }
    if name.ns != NsId::W {
        return ("R07", BodyClass::Unknown(name));
    }
    match name.local {
        LocalName::SectPr => ("R01", BodyClass::SectionProps),
        LocalName::Tbl => ("R02", BodyClass::Table),
        LocalName::Sdt => ("R03", BodyClass::Sdt),
        LocalName::Br => {
            let page =
                dom.attr_value(node, QName::w(LocalName::Type)).is_some_and(|t| t.trim() == "page");
            ("R05", BodyClass::BodyBreak { page })
        }
        LocalName::Ins => ("R06", BodyClass::InsertWrap),
        LocalName::Del => ("R06", BodyClass::DeleteWrap),
        LocalName::MoveFrom => ("R06", BodyClass::MoveFromWrap),
        LocalName::MoveTo => ("R06", BodyClass::MoveToWrap),
        LocalName::CustomXml | LocalName::SmartTag => ("R06", BodyClass::Transparent),
        LocalName::P => ("R19", BodyClass::Paragraph),
        _ if is_property_element(name) => ("R07", BodyClass::Unknown(name)),
        _ => ("R07", BodyClass::Unknown(name)),
    }
}

/// `w:p` 的分类结果（R08–R19）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParaClass {
    Protected(ProtectedKind),
    Image,
    Text,
}

/// 一条段落规则：命中返回结果。
pub type ParaRule = fn(&ParagraphFacts) -> Option<ParaClass>;

/// 按优先级排列的段落规则表；`classify_paragraph` 顺序求值，首条命中即结束。
pub const PARA_RULES: &[(&str, ParaRule)] = &[
    ("R08", r08_style_vanish),
    ("R09", r09_field_block_result),
    ("R10", r10_section_break),
    ("R11", r11_equation),
    ("R12", r12_chart),
    ("R13", r13_smart_art),
    ("R14", r14_locked_canvas),
    ("R15", r15_image),
    ("R16", r16_invisible_shapes),
    ("R17", r17_rule),
    ("R18", r18_ole),
    ("R19", r19_text),
];

pub fn classify_paragraph(f: &ParagraphFacts) -> (&'static str, ParaClass) {
    for (id, rule) in PARA_RULES {
        if let Some(c) = rule(f) {
            return (id, c);
        }
    }
    ("R19", ParaClass::Text)
}

pub fn r08_style_vanish(f: &ParagraphFacts) -> Option<ParaClass> {
    f.style_vanish.then_some(ParaClass::Protected(ProtectedKind::Invisible))
}

pub fn r09_field_block_result(f: &ParagraphFacts) -> Option<ParaClass> {
    f.inside_field_result.map(|id| ParaClass::Protected(ProtectedKind::FieldBlockResult(id)))
}

pub fn r10_section_break(f: &ParagraphFacts) -> Option<ParaClass> {
    (f.has_sect_pr && !f.visible_text).then_some(ParaClass::Protected(ProtectedKind::SectionBreak))
}

pub fn r11_equation(f: &ParagraphFacts) -> Option<ParaClass> {
    (f.math.omath_para || (f.math.count > 0 && !f.visible_text))
        .then_some(ParaClass::Protected(ProtectedKind::Equation))
}

pub fn r12_chart(f: &ParagraphFacts) -> Option<ParaClass> {
    let mut chart = false;
    for d in &f.drawings {
        match d.kind {
            // chartex（旭日图 / 瀑布图 …）配了预渲染的回退图：Word 之外的渲染器画的就是这张图，
            // 数据模型的降级读法只留给没有回退图的 part。`graphic_display` 取回退图的显示模型。
            DrawingKind::ChartEx if d.fallback_picture.is_some() => return Some(ParaClass::Image),
            DrawingKind::Chart | DrawingKind::ChartEx => chart = true,
            _ => {}
        }
    }
    chart.then_some(ParaClass::Protected(ProtectedKind::Chart))
}

pub fn r13_smart_art(f: &ParagraphFacts) -> Option<ParaClass> {
    f.drawings
        .iter()
        .any(|d| d.kind == DrawingKind::Diagram)
        .then_some(ParaClass::Protected(ProtectedKind::SmartArt))
}

pub fn r14_locked_canvas(f: &ParagraphFacts) -> Option<ParaClass> {
    f.drawings
        .iter()
        .any(|d| d.kind == DrawingKind::LockedCanvas)
        .then_some(ParaClass::Protected(ProtectedKind::SmartArt))
}

pub fn r15_image(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || !f.objects.is_empty() || f.math.count != 0 {
        return None;
    }
    let single_picture =
        f.drawings.len() == 1 && f.picts.is_empty() && f.drawings[0].kind == DrawingKind::Picture;
    let single_imagedata =
        f.picts.len() == 1 && f.drawings.is_empty() && f.picts[0].kind == PictKind::ImageData;
    (single_picture || single_imagedata).then_some(ParaClass::Image)
}

pub fn r16_invisible_shapes(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || f.picts.is_empty() || !f.drawings.is_empty() || !f.objects.is_empty() {
        return None;
    }
    f.picts
        .iter()
        .all(|p| matches!(p.kind, PictKind::Hidden | PictKind::ShapeTypeOnly))
        .then_some(ParaClass::Protected(ProtectedKind::Invisible))
}

pub fn r17_rule(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || f.picts.is_empty() || !f.drawings.is_empty() || !f.objects.is_empty() {
        return None;
    }
    f.picts
        .iter()
        .all(|p| p.kind == PictKind::Hr)
        .then_some(ParaClass::Protected(ProtectedKind::Rule))
}

pub fn r18_ole(f: &ParagraphFacts) -> Option<ParaClass> {
    (!f.visible_text && !f.objects.is_empty() && f.drawings.is_empty() && f.picts.is_empty())
        .then_some(ParaClass::Protected(ProtectedKind::Ole))
}

pub fn r19_text(_: &ParagraphFacts) -> Option<ParaClass> {
    Some(ParaClass::Text)
}

/// `MOD-03`：ListRef 存在 → ListItem；否则 Heading；否则 Paragraph。
pub fn text_kind(f: &ParagraphFacts) -> TextKind {
    if let Some(list) = &f.numbering_ref {
        return TextKind::ListItem { list: list.clone() };
    }
    if let Some(level) = f.outline_level {
        return TextKind::Heading { level };
    }
    TextKind::Paragraph
}
