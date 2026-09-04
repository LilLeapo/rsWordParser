//! `ParagraphFacts`（`MOD-04`，`docs/03` §6.2）：对 `w:p` 一次遍历得到的事实，分类（`MOD-05`）
//! 与 `TextKind` 判定（`MOD-03`）都是它的纯函数。
//!
//! M1 范围：文本、sectPr、样式、编号、outline、公式与修订计数、绘图 / VML 的粗事实
//! （种类按 `graphicData/@uri` 与 VML 子元素判定）。字段事实（`fields` / `inside_field_result`）在 M2。

use crate::model::block::{ListRef, SdtInfo};
use crate::model::decl::{OwnHeadingLevel, Styles};
use crate::semantic::props::{ParaProps, Style, StyleType, Val};
use crate::span::FieldId;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParagraphFacts {
    pub has_sect_pr: bool,
    /// 任一 `w:t`/`w:delText` trim 后非空（不含 `w:txbxContent` 内）。
    pub visible_text: bool,
    /// 同上，且排除所有绘图 / VML / 对象内容。
    pub visible_text_outside_boxes: bool,
    pub fields: Vec<FieldId>,
    pub inside_field_result: Option<FieldId>,
    pub drawings: Vec<DrawingFacts>,
    pub picts: Vec<PictFacts>,
    /// `w:object` 数量。
    pub objects: u32,
    pub math: MathFacts,
    pub revision: RevisionFacts,
    pub style_id: Option<String>,
    /// 样式链 `vanish == true` 且段落里没有把它关掉、没有必须显示的内容。
    pub style_vanish: bool,
    /// styleId 匹配 `^TOC ?([1-9])$`。
    pub toc_style_level: Option<u8>,
    pub numbering_ref: Option<ListRef>,
    /// `MOD-03`：直接 `outlineLvl` 0–8 → +1（9 → `None`，不再看样式）；否则样式链；否则 styleId 匹配。
    pub outline_level: Option<u8>,
    pub sdt: Option<SdtInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingFacts {
    pub node: NodeId,
    pub kind: DrawingKind,
    /// `wp:anchor`（否则 `wp:inline`）。
    pub anchored: bool,
    pub has_txbx_text: bool,
    pub has_blip: bool,
    /// `wp:docPr/@name` 以 `aidocs-ink` 开头。
    pub is_ink: bool,
}

/// 按 `a:graphicData/@uri` 判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingKind {
    Picture,
    Chart,
    ChartEx,
    Diagram,
    LockedCanvas,
    Shape,
    Group,
    Line,
    Unknown,
}

impl DrawingKind {
    pub fn from_uri(uri: &str) -> DrawingKind {
        match uri {
            "http://schemas.openxmlformats.org/drawingml/2006/picture" => DrawingKind::Picture,
            "http://schemas.openxmlformats.org/drawingml/2006/chart" => DrawingKind::Chart,
            "http://schemas.microsoft.com/office/drawing/2014/chartex" => DrawingKind::ChartEx,
            "http://schemas.openxmlformats.org/drawingml/2006/diagram" => DrawingKind::Diagram,
            "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas" => {
                DrawingKind::LockedCanvas
            }
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape" => {
                DrawingKind::Shape
            }
            "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" => {
                DrawingKind::Group
            }
            _ => DrawingKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictFacts {
    pub node: NodeId,
    pub kind: PictKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictKind {
    ImageData,
    TextBox,
    WordArt,
    /// `v:rect[@o:hr]`。
    Hr,
    ShapeTypeOnly,
    /// `visibility:hidden`。
    Hidden,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathFacts {
    /// 段落直接内容里的 `m:oMath` 数（不含绘图 / 文本框内）。
    pub count: u32,
    pub omath_para: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionFacts {
    pub run_ins: bool,
    pub run_del: bool,
    pub move_from: bool,
    pub move_to: bool,
    pub del_instr_text: bool,
    pub para_mark_ins: bool,
    pub para_mark_del: bool,
    pub ppr_change: bool,
}

impl RevisionFacts {
    pub fn any(&self) -> bool {
        self.run_ins
            || self.run_del
            || self.move_from
            || self.move_to
            || self.del_instr_text
            || self.para_mark_ins
            || self.para_mark_del
            || self.ppr_change
    }
}

fn is_w(name: QName, local: LocalName) -> bool {
    name.ns == NsId::W && name.local == local
}

fn attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
}

/// 元素下所有文本节点拼接后 trim 是否非空。
fn has_visible_text(dom: &Dom, node: NodeId) -> bool {
    dom.semantic_children(node)
        .any(|c| dom.text(c).is_some_and(|t| !t.trim_matches([' ', '\t', '\r', '\n']).is_empty()))
}

impl ParagraphFacts {
    /// 一次遍历；`props` 是已读出的 `w:pPr`（可缺）。
    pub fn compute(
        dom: &Dom,
        p: NodeId,
        props: &ParaProps,
        styles: Option<&Styles>,
        sdt: Option<SdtInfo>,
    ) -> ParagraphFacts {
        let mut f = ParagraphFacts { sdt, ..Default::default() };
        f.has_sect_pr = props.sect_pr.is_some();
        f.style_id = props.style.clone();
        f.toc_style_level = f.style_id.as_deref().and_then(toc_level_of_id);
        for &n in &props.raw_unmodeled {
            if dom.is(n, QName::w(LocalName::PPrChange)) {
                f.revision.ppr_change = true;
            }
        }
        if let Some(rpr) = &props.rpr {
            for &n in &rpr.raw_unmodeled {
                match dom.name(n) {
                    Some(q) if is_w(q, LocalName::Ins) => f.revision.para_mark_ins = true,
                    Some(q) if is_w(q, LocalName::Del) => f.revision.para_mark_del = true,
                    _ => {}
                }
            }
        }

        // 内容遍历：(节点, 在文本框内, 在绘图/VML/对象内)
        let mut unvanish = false;
        let mut has_marker = false;
        let mut stack: Vec<(NodeId, bool, bool)> = dom
            .semantic_children(p)
            .filter(|&c| !dom.is(c, QName::w(LocalName::PPr)))
            .map(|c| (c, false, false))
            .collect();
        stack.reverse();
        while let Some((node, in_txbx, in_gfx)) = stack.pop() {
            let Some(name) = dom.name(node) else {
                continue;
            };
            let (mut txbx, mut gfx) = (in_txbx, in_gfx);
            match (name.ns, name.local) {
                (NsId::W, LocalName::T | LocalName::DelText) => {
                    if !in_txbx && has_visible_text(dom, node) {
                        f.visible_text = true;
                        if !in_gfx {
                            f.visible_text_outside_boxes = true;
                        }
                    }
                    continue;
                }
                (NsId::W, LocalName::TxbxContent) => txbx = true,
                (NsId::W, LocalName::Drawing) if !in_gfx => {
                    f.drawings.push(drawing_facts(dom, node));
                    gfx = true;
                }
                (NsId::W, LocalName::Pict) if !in_gfx => {
                    f.picts.push(PictFacts { node, kind: pict_kind(dom, node) });
                    gfx = true;
                }
                (NsId::W, LocalName::Object) if !in_gfx => {
                    f.objects += 1;
                    gfx = true;
                }
                (NsId::M, LocalName::OMath) if !in_gfx && !in_txbx => f.math.count += 1,
                (NsId::M, LocalName::OMathPara) if !in_gfx && !in_txbx => f.math.omath_para = true,
                (NsId::W, LocalName::Ins) => f.revision.run_ins = true,
                (NsId::W, LocalName::Del) => f.revision.run_del = true,
                (NsId::W, LocalName::MoveFrom) => f.revision.move_from = true,
                (NsId::W, LocalName::MoveTo) => f.revision.move_to = true,
                (NsId::W, LocalName::DelInstrText) => f.revision.del_instr_text = true,
                (NsId::W, LocalName::Vanish) if !in_txbx => {
                    if let Some(v) = attr(dom, node, NsId::W, LocalName::Val)
                        && matches!(v.trim(), "0" | "false" | "off")
                    {
                        unvanish = true;
                    }
                }
                (
                    NsId::W,
                    LocalName::BookmarkStart
                    | LocalName::CommentRangeStart
                    | LocalName::CommentRangeEnd,
                ) => {
                    has_marker = true;
                }
                _ => {}
            }
            for &c in dom.children(node).iter().rev() {
                stack.push((c, txbx, gfx));
            }
        }

        // MOD-03：编号与标题级别
        let chain: Vec<&Style> = match (styles, f.style_id.as_deref()) {
            (Some(s), Some(id)) => s.chain(id, StyleType::Paragraph),
            _ => Vec::new(),
        };
        f.numbering_ref = list_ref(props, &chain);
        f.outline_level = outline_level(props, &chain, f.style_id.as_deref());
        // style_vanish（TS `staysVanished`）
        let chain_vanish =
            chain.iter().find_map(|s| s.rpr.as_ref().and_then(|r| r.vanish)).unwrap_or(false);
        f.style_vanish = chain_vanish
            && !unvanish
            && !has_marker
            && f.drawings.is_empty()
            && f.picts.is_empty()
            && f.objects == 0
            && !f.has_sect_pr
            && props.num.is_none();
        f
    }
}

/// `MOD-03` `ListRef`：直接 `w:numPr`（numId 0 → 无编号；无 numId 时用样式链的，ilvl 缺省用样式的再缺省 0）。
fn list_ref(props: &ParaProps, chain: &[&Style]) -> Option<ListRef> {
    let direct = props.num.as_ref();
    let direct_num = direct.and_then(|n| n.num_id.as_ref()).and_then(|v| v.value().copied());
    let direct_ilvl = direct.and_then(|n| n.ilvl.as_ref()).and_then(|v| v.value().copied());
    if let Some(id) = direct_num {
        if id == 0 {
            return None;
        }
        let ilvl = direct_ilvl.or_else(|| style_list(chain).map(|(_, l)| l)).unwrap_or(0);
        return Some(ListRef { num_id: id, ilvl, from_style: false });
    }
    let (num_id, style_ilvl) = style_list(chain)?;
    Some(ListRef { num_id, ilvl: direct_ilvl.unwrap_or(style_ilvl), from_style: true })
}

/// 样式链上第一个 `numPr`：numId 0 为显式取消（返回 `None`）。
fn style_list(chain: &[&Style]) -> Option<(i32, i32)> {
    for s in chain {
        let Some(num) = s.ppr.as_ref().and_then(|p| p.num.as_ref()) else { continue };
        let id = num.num_id.as_ref().and_then(|v| v.value().copied());
        match id {
            Some(0) => return None,
            Some(id) => {
                let ilvl = num.ilvl.as_ref().and_then(|v| v.value().copied()).unwrap_or(0);
                return Some((id, ilvl));
            }
            None => continue,
        }
    }
    None
}

/// `MOD-03` 标题级别。
fn outline_level(props: &ParaProps, chain: &[&Style], style_id: Option<&str>) -> Option<u8> {
    if let Some(Val::Value(l)) = &props.outline_lvl {
        return match *l {
            0..=8 => Some(*l as u8 + 1),
            _ => None,
        };
    }
    for s in chain {
        match Styles::own_heading_level(s) {
            OwnHeadingLevel::Level(l) => return Some(l),
            OwnHeadingLevel::Blocked => return None,
            OwnHeadingLevel::Inherit => {}
        }
    }
    if chain.is_empty() {
        // 文档未定义的内建样式：`^Heading([1-9])$`（忽略大小写）
        let id = style_id?;
        let rest = id.get(..7).filter(|p| p.eq_ignore_ascii_case("heading")).map(|_| &id[7..])?;
        let mut it = rest.chars();
        let d = it.next()?;
        if it.next().is_none() && d.is_ascii_digit() && d != '0' {
            return Some(d as u8 - b'0');
        }
    }
    None
}

/// `^TOC ?([1-9])$`
fn toc_level_of_id(id: &str) -> Option<u8> {
    let rest = id.strip_prefix("TOC")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !d.is_ascii_digit() || d == '0' {
        return None;
    }
    Some(d as u8 - b'0')
}

fn drawing_facts(dom: &Dom, drawing: NodeId) -> DrawingFacts {
    let mut f = DrawingFacts {
        node: drawing,
        kind: DrawingKind::Unknown,
        anchored: false,
        has_txbx_text: false,
        has_blip: false,
        is_ink: false,
    };
    for n in dom.descendants(drawing) {
        let Some(name) = dom.name(n) else { continue };
        match (name.ns, name.local) {
            (NsId::Wp, LocalName::Anchor) => f.anchored = true,
            (NsId::Wp, LocalName::DocPr) => {
                if attr(dom, n, NsId::None, LocalName::Name)
                    .is_some_and(|s| s.starts_with("aidocs-ink"))
                {
                    f.is_ink = true;
                }
            }
            (NsId::A, LocalName::GraphicData) if f.kind == DrawingKind::Unknown => {
                if let Some(uri) = attr(dom, n, NsId::None, LocalName::Uri) {
                    f.kind = DrawingKind::from_uri(&uri);
                }
            }
            (NsId::A, LocalName::Blip) => f.has_blip = true,
            (NsId::W, LocalName::T) if has_visible_text(dom, n) => f.has_txbx_text = true,
            _ => {}
        }
    }
    f
}

fn pict_kind(dom: &Dom, pict: NodeId) -> PictKind {
    let mut kind = PictKind::Other;
    let mut only_shapetype = true;
    for c in dom.semantic_children(pict) {
        let Some(name) = dom.name(c) else { continue };
        if !(name.ns == NsId::V && name.local == LocalName::Shapetype) {
            only_shapetype = false;
        }
    }
    if only_shapetype && dom.semantic_children(pict).next().is_some() {
        return PictKind::ShapeTypeOnly;
    }
    for n in dom.descendants(pict) {
        let Some(name) = dom.name(n) else { continue };
        match (name.ns, name.local) {
            (NsId::V, LocalName::Imagedata) => return PictKind::ImageData,
            (NsId::V, LocalName::Textbox) => kind = PictKind::TextBox,
            (NsId::V, LocalName::Textpath)
                if attr(dom, n, NsId::None, LocalName::String).is_some() =>
            {
                return PictKind::WordArt;
            }
            (NsId::V, LocalName::Rect)
                if dom.attr(n, QName::new(NsId::O, LocalName::Hr)).is_some() =>
            {
                return PictKind::Hr;
            }
            (
                NsId::V,
                LocalName::Shape
                | LocalName::Rect
                | LocalName::Oval
                | LocalName::Roundrect
                | LocalName::Line,
            ) if kind == PictKind::Other
                && attr(dom, n, NsId::None, LocalName::Style)
                    .is_some_and(|s| s.contains("visibility:hidden")) =>
            {
                kind = PictKind::Hidden;
            }
            _ => {}
        }
    }
    kind
}

impl Styles {
    /// basedOn 链（叶 → 根），带环检测；类型不一致的 basedOn 视为链结束（`RES-02`）。
    pub fn chain(&self, id: &str, kind: StyleType) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(id) = cur {
            let Some(s) = self.get(&id) else { break };
            if s.kind() != Some(kind)
                || out.iter().any(|x: &&Style| std::ptr::eq(*x, s))
                || out.len() > 64
            {
                break;
            }
            out.push(s);
            cur = s.based_on.clone();
        }
        out
    }
}
