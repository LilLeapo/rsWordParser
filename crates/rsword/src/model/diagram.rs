//! SmartArt 与绘图画布的模型（`MOD-11`，`spec/17` 任务 6.3）。
//!
//! SmartArt 有两个 part：**数据 part**（`dgm:dataModel`，主 part 里 `dgm:relIds/@r:dm` 指向）给节点文字，
//! **绘图 part**（`dsp:drawing`，Word 保存下来的已排版结果）给形状。两者都是有自己 DOM 的 XML part（L1），
//! 这里只读**事实**：EMU、1/60000 度、颜色的原始定义；px 换算、画布缩放与排版启发式全在
//! `bind/compat_ts/diagram.rs`。画布（`lc:lockedCanvas`，R14）在主 part 里，形状用同一个
//! [`DiagramShape`]，外加子坐标系（[`CanvasDisplay`]）。

use std::collections::{HashMap, HashSet};

use crate::diag::{DiagCode, Diagnostic};
use crate::model::drawing::{Extent, RectFrac, attr, extent_of, num, rect_frac, text_of, xy};
use crate::package::PartId;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 一个形状：绘图 part 的 `dsp:sp`，或画布里的 `a:sp` / `a:pic`。几何是原值（EMU、1/60000 度）；
/// 画布形状的几何在**子坐标系**里（[`CanvasDisplay::ch_off`] / `ch_ext`），缩放在投影层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramShape {
    pub node: NodeId,
    /// `a:xfrm/a:off`。
    pub off_emu: (i64, i64),
    /// `a:xfrm/a:ext`。连线（`prst=line` / `*Connector*`）可以一边为 0。
    pub ext_emu: Extent,
    /// `a:xfrm/@rot`。
    pub rot_60k: Option<i64>,
    /// `a:prstGeom/@prst`。
    pub prst: Option<String>,
    /// `a:noFill`。
    pub no_fill: bool,
    /// `a:solidFill` 容器节点。颜色留原始定义，解析成 sRGB 走 [`crate::resolve::drawingml::color_in`]
    /// （与 M4 的 [`crate::model::drawing::FillDisplay`] 同一约定；节点属于形状所在的 part）。
    pub fill: Option<NodeId>,
    /// `a:gradFill` 节点（投影层取各停靠点的等权平均）。
    pub gradient: Option<NodeId>,
    /// `a:ln`（有 `a:noFill` 的线不记）。
    pub line: Option<DiagramLine>,
    /// `a:blipFill`（`dsp:spPr` 里的图片填充，或 `a:pic` 自己的图）。
    pub picture: Option<DiagramPicture>,
    /// `txBody` 各段文字（`a:r/a:t` 拼接，空段不记）。
    pub texts: Vec<String>,
    /// 第一个带 `sz` 的 `a:rPr`：字号，1/100 pt 原值。
    pub font_size_100pt: Option<i64>,
    /// 同一轮里读到的 `a:rPr/a:solidFill` 容器节点。
    pub text_color: Option<NodeId>,
}

/// `a:ln`：颜色容器（`a:solidFill`）与 `@w`（EMU）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramLine {
    pub node: NodeId,
    pub color: Option<NodeId>,
    pub width_emu: Option<i64>,
}

/// 图片填充：`a:blip/@r:embed`（按**所在 part** 的关系解）与 `a:stretch/a:fillRect`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramPicture {
    pub embed: Option<String>,
    pub fill_rect: Option<RectFrac>,
}

/// 主 part 引用的一个 SmartArt：数据 part 与（可能没有的）绘图 part。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramPart {
    pub data: PartId,
    /// 绘图 part：数据 part 的 `diagramDrawing` 关系，找不到时按 TS 的路径约定 `data{N}.xml → drawing{N}.xml`。
    pub drawing: Option<PartId>,
    /// 数据 part 的节点文字（树序，`\n` 连接）；一个字都没有、part 缺失或不是 `dgm:dataModel` → `None`。
    pub text: Option<String>,
    /// 绘图 part 的形状；part 缺失 / 解析不了 / 一个形状都没有 → `None`。
    pub shapes: Option<Vec<DiagramShape>>,
}

impl DiagramPart {
    /// `data_dom` / 绘图 part 的 DOM 为 `None` = part 是二进制或解析失败（`Package` 已记 `PKG_OPAQUE_PART`）。
    pub fn build(
        data: PartId,
        data_dom: Option<&Dom>,
        drawing: Option<(PartId, Option<&Dom>)>,
        warnings: &mut Vec<Diagnostic>,
    ) -> DiagramPart {
        let text = data_dom.and_then(|dom| {
            let root = dom.root();
            if !dom.is(root, QName::new(NsId::Dgm, LocalName::DataModel)) {
                warnings.push(Diagnostic::pre_existing(
                    data,
                    dom.node(root).lex.as_ref().map(|l| l.range.clone()),
                    DiagCode::ModUnparseable,
                    "SmartArt 数据 part 的根不是 dgm:dataModel",
                ));
                return None;
            }
            diagram_text(dom)
        });
        let shapes = drawing.and_then(|(_, dom)| dom).map(diagram_shapes).filter(|s| !s.is_empty());
        DiagramPart { data, drawing: drawing.map(|(id, _)| id), text, shapes }
    }
}

/// 数据 part 的节点文字，按内容树的先序（TS `extractDiagramText`）。
///
/// `dgm:pt` 里 `type ∈ {pres, parTrans, sibTrans}` 的是排版点，不算；文字是 `a:t` 拼接后 trim。
/// `dgm:cxn` 没写 `type` 或 `type="parOf"` 的是父子边，按 `srcOrd` 排；根 = 出现过做源点、没有父的点，
/// 按首次出现的顺序各走一遍先序（显式栈 + `seen`，成环 / 自指不会死循环）；没进树的点按文件序追加。
pub fn diagram_text(dom: &Dom) -> Option<String> {
    let root = dom.root();
    // (modelId, 文字)，文件序
    let mut points: Vec<(String, String)> = Vec::new();
    let mut text_of_id: HashMap<String, usize> = HashMap::new();
    let mut src_order: Vec<String> = Vec::new();
    let mut children: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    let mut has_parent: HashSet<String> = HashSet::new();
    for n in dom.semantic_descendants(root) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::Dgm {
            continue;
        }
        match name.local {
            LocalName::Pt => {
                let Some(id) = attr(dom, n, NsId::None, LocalName::ModelId) else { continue };
                if attr(dom, n, NsId::None, LocalName::Type)
                    .is_some_and(|t| matches!(t.as_str(), "pres" | "parTrans" | "sibTrans"))
                {
                    continue;
                }
                let mut s = String::new();
                for t in dom.semantic_descendants(n) {
                    if dom.is(t, QName::new(NsId::A, LocalName::T))
                        && let Some(x) = text_of(dom, t)
                    {
                        s.push_str(&x);
                    }
                }
                let s = s.trim();
                if s.is_empty() || text_of_id.contains_key(&id) {
                    continue;
                }
                text_of_id.insert(id.clone(), points.len());
                points.push((id, s.to_string()));
            }
            LocalName::Cxn => {
                if attr(dom, n, NsId::None, LocalName::Type).is_some_and(|t| t != "parOf") {
                    continue;
                }
                let (Some(src), Some(dst)) = (
                    attr(dom, n, NsId::None, LocalName::SrcId),
                    attr(dom, n, NsId::None, LocalName::DestId),
                ) else {
                    continue;
                };
                let ord = num(dom, n, LocalName::SrcOrd).unwrap_or(0);
                if !children.contains_key(&src) {
                    src_order.push(src.clone());
                }
                children.entry(src).or_default().push((ord, dst.clone()));
                has_parent.insert(dst);
            }
            _ => {}
        }
    }
    for kids in children.values_mut() {
        kids.sort_by_key(|(ord, _)| *ord);
    }
    let mut texts: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = Vec::new();
    for root_id in src_order.iter().filter(|s| !has_parent.contains(*s)) {
        stack.push(root_id);
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(&i) = text_of_id.get(id) {
                texts.push(&points[i].1);
            }
            if let Some(kids) = children.get(id) {
                stack.extend(kids.iter().rev().map(|(_, d)| d.as_str()));
            }
        }
    }
    for (id, t) in &points {
        if !seen.contains(id.as_str()) {
            texts.push(t);
        }
    }
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// 绘图 part 里全部 `dsp:sp`（含 `dsp:grpSp` 里的），文档序；没有 `dsp:spPr` 或 `a:xfrm` 不全的跳过。
pub fn diagram_shapes(dom: &Dom) -> Vec<DiagramShape> {
    let mut out = Vec::new();
    for sp in dom.semantic_descendants(dom.root()) {
        if !dom.is(sp, QName::new(NsId::Dsp, LocalName::Sp)) {
            continue;
        }
        let Some(sp_pr) = child(dom, sp, NsId::Dsp, LocalName::SpPr) else { continue };
        let tx_body = child(dom, sp, NsId::Dsp, LocalName::TxBody);
        if let Some(s) = read_shape(dom, sp, sp_pr, None, tx_body) {
            out.push(s);
        }
    }
    out
}

/// 一个 `lc:lockedCanvas`：子坐标系与直接子元素里的 `a:sp` / `a:pic`（TS 不下钻 `a:grpSp`，这里也不）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasDisplay {
    pub node: NodeId,
    /// `a:grpSpPr/a:xfrm/a:chOff`（缺省 0,0）。
    pub ch_off: Option<(i64, i64)>,
    /// `a:grpSpPr/a:xfrm/a:chExt`（缺省 = 宿主 `wp:extent`）。
    pub ch_ext: Option<Extent>,
    pub shapes: Vec<DiagramShape>,
}

pub fn canvas_display(dom: &Dom, lc: NodeId) -> CanvasDisplay {
    let mut c = CanvasDisplay { node: lc, ch_off: None, ch_ext: None, shapes: Vec::new() };
    for n in dom.semantic_children(lc) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::GrpSpPr => {
                if let Some(xfrm) = child(dom, n, NsId::A, LocalName::Xfrm) {
                    c.ch_off = child(dom, xfrm, NsId::A, LocalName::ChOff).and_then(|g| xy(dom, g));
                    c.ch_ext =
                        child(dom, xfrm, NsId::A, LocalName::ChExt).and_then(|g| extent_of(dom, g));
                }
            }
            LocalName::Sp => {
                let Some(sp_pr) = child(dom, n, NsId::A, LocalName::SpPr) else { continue };
                let tx_body = child(dom, n, NsId::A, LocalName::TxSp)
                    .and_then(|t| child(dom, t, NsId::A, LocalName::TxBody));
                if let Some(s) = read_shape(dom, n, sp_pr, None, tx_body) {
                    c.shapes.push(s);
                }
            }
            LocalName::Pic => {
                let Some(sp_pr) = child(dom, n, NsId::A, LocalName::SpPr) else { continue };
                let blip = child(dom, n, NsId::A, LocalName::BlipFill);
                if let Some(s) = read_shape(dom, n, sp_pr, blip, None) {
                    c.shapes.push(s);
                }
            }
            _ => {}
        }
    }
    c
}

/// `spPr`（几何 / 填充 / 线）+ 可选的独立 `a:blipFill`（`a:pic`）+ 可选的文字体 → 形状。
fn read_shape(
    dom: &Dom,
    node: NodeId,
    sp_pr: NodeId,
    blip_fill: Option<NodeId>,
    tx_body: Option<NodeId>,
) -> Option<DiagramShape> {
    let xfrm = child(dom, sp_pr, NsId::A, LocalName::Xfrm)?;
    let off = child(dom, xfrm, NsId::A, LocalName::Off).and_then(|g| xy(dom, g))?;
    let ext = child(dom, xfrm, NsId::A, LocalName::Ext).and_then(|g| extent_of(dom, g))?;
    let mut s = DiagramShape {
        node,
        off_emu: off,
        ext_emu: ext,
        rot_60k: num(dom, xfrm, LocalName::Rot),
        prst: None,
        no_fill: false,
        fill: None,
        gradient: None,
        line: None,
        picture: None,
        texts: Vec::new(),
        font_size_100pt: None,
        text_color: None,
    };
    for c in dom.semantic_children(sp_pr) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::PrstGeom => s.prst = attr(dom, c, NsId::None, LocalName::Prst),
            LocalName::NoFill => s.no_fill = true,
            LocalName::SolidFill if s.fill.is_none() => s.fill = Some(c),
            LocalName::GradFill if s.gradient.is_none() => s.gradient = Some(c),
            LocalName::BlipFill if s.picture.is_none() => s.picture = Some(picture_of(dom, c)),
            LocalName::Ln
                if s.line.is_none() && child(dom, c, NsId::A, LocalName::NoFill).is_none() =>
            {
                s.line = Some(DiagramLine {
                    node: c,
                    color: child(dom, c, NsId::A, LocalName::SolidFill),
                    width_emu: num(dom, c, LocalName::W),
                });
            }
            _ => {}
        }
    }
    if let Some(b) = blip_fill {
        s.picture = Some(picture_of(dom, b));
    }
    if let Some(body) = tx_body {
        read_text_body(dom, body, &mut s);
    }
    Some(s)
}

fn picture_of(dom: &Dom, blip_fill: NodeId) -> DiagramPicture {
    let mut p = DiagramPicture { embed: None, fill_rect: None };
    for n in dom.semantic_descendants(blip_fill) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::Blip if p.embed.is_none() => {
                p.embed = attr(dom, n, NsId::R, LocalName::Embed)
            }
            LocalName::FillRect if p.fill_rect.is_none() => p.fill_rect = Some(rect_frac(dom, n)),
            _ => {}
        }
    }
    p
}

/// `txBody/a:p/a:r`：段文字、第一个带 `sz` 的 `a:rPr` 的字号与颜色（TS 的读法：字号没定下来之前
/// 每个 `a:rPr` 都看一眼，颜色跟着最后看的那个）。
fn read_text_body(dom: &Dom, body: NodeId, s: &mut DiagramShape) {
    for p in dom.semantic_children(body) {
        if !dom.is(p, QName::new(NsId::A, LocalName::P)) {
            continue;
        }
        let mut text = String::new();
        for r in dom.semantic_children(p) {
            if !dom.is(r, QName::new(NsId::A, LocalName::R)) {
                continue;
            }
            if let Some(t) = child(dom, r, NsId::A, LocalName::T)
                && let Some(x) = text_of(dom, t)
            {
                text.push_str(&x);
            }
            if s.font_size_100pt.is_none()
                && let Some(rpr) = child(dom, r, NsId::A, LocalName::RPr)
            {
                s.font_size_100pt = num(dom, rpr, LocalName::Sz).filter(|&v| v > 0);
                if let Some(c) = child(dom, rpr, NsId::A, LocalName::SolidFill) {
                    s.text_color = Some(c);
                }
            }
        }
        if !text.trim().is_empty() {
            s.texts.push(text);
        }
    }
}

fn child(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&c| dom.is(c, QName::new(ns, local)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    fn data(pts: &str, cxns: &str) -> Dom {
        let src = format!(
            r#"<dgm:dataModel xmlns:dgm="{DGM}" xmlns:a="{A}"><dgm:ptLst>{pts}</dgm:ptLst><dgm:cxnLst>{cxns}</dgm:cxnLst></dgm:dataModel>"#
        );
        Dom::parse(PartId(0), src.as_bytes()).expect("parse")
    }

    fn pt(id: &str, text: &str, ty: &str) -> String {
        let ty = if ty.is_empty() { String::new() } else { format!(r#" type="{ty}""#) };
        format!(
            r#"<dgm:pt modelId="{id}"{ty}><dgm:t><a:p><a:r><a:t>{text}</a:t></a:r></a:p></dgm:t></dgm:pt>"#
        )
    }

    fn cxn(src: &str, dst: &str, ord: &str, ty: &str) -> String {
        let ty = if ty.is_empty() { String::new() } else { format!(r#" type="{ty}""#) };
        let ord = if ord.is_empty() { String::new() } else { format!(r#" srcOrd="{ord}""#) };
        format!(r#"<dgm:cxn modelId="c"{ty} srcId="{src}" destId="{dst}"{ord}/>"#)
    }

    #[test]
    fn tree_order_then_isolated_points() {
        let dom = data(
            &[
                pt("root", "Root", ""),
                pt("later", "Later", ""),
                pt("first", "First", ""),
                pt("leaf", "Leaf", ""),
                pt("alone", "Alone", ""),
                pt("pres", "IGNORED", "pres"),
            ]
            .concat(),
            &[
                cxn("root", "later", "9", "parOf"),
                cxn("root", "first", "1", ""),
                cxn("first", "leaf", "0", ""),
                cxn("root", "alone", "0", "presOf"),
            ]
            .concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("Root\nFirst\nLeaf\nLater\nAlone"));
    }

    #[test]
    fn cycles_self_loops_and_missing_src_ord_terminate() {
        // root ↔ later 成环、first 自指：没有根，全部按文件序当孤立点
        let dom = data(
            &[pt("root", "Root", ""), pt("later", "Later", ""), pt("first", "First", "")].concat(),
            &[
                cxn("root", "later", "", ""),
                cxn("later", "root", "", ""),
                cxn("first", "first", "", ""),
            ]
            .concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("Root\nLater\nFirst"));
        // 有根、子树里成环：环上的点各出现一次
        let dom = data(
            &[pt("a", "A", ""), pt("b", "B", ""), pt("c", "C", "")].concat(),
            &[cxn("a", "b", "", ""), cxn("b", "c", "", ""), cxn("c", "b", "", "")].concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("A\nB\nC"));
        assert_eq!(diagram_text(&data("", "")), None);
    }
}
