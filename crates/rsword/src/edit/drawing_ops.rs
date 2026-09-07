//! 既有绘图的编辑（`EDIT-03`，`spec/18` 7.7）：尺寸 / 旋转 / 翻转 / 裁剪、z-order、形状样式。
//!
//! **只改属性**（分层决策 9）：`wp:extent` / `a:ext` / `wp:posOffset` / `relativeHeight` /
//! `a:xfrm` 的属性改动让那个元素 `SelfDirty`，`a:graphic` 子树永远原字节。
//!
//! 形态对照件是 `fixtures/word-ops/{z-order,move-resize}`：Word 自己做同一件事的前后两份。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::xml::{Dirty, Dom, LocalName, NewElement, NodeEdit, NodeId, NsId, QName, Target};

use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;

/// TS `applyImageZOrder` 的基数：`relativeHeight = Z_BASE + z`。
pub const Z_BASE: i64 = 251_658_240;

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

fn wp(local: LocalName) -> QName {
    QName::new(NsId::Wp, local)
}

fn live_children(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    dom.children(n).iter().copied().filter(|&c| dom.node(c).dirty != Dirty::Deleted)
}

/// `w:drawing` 下的 `wp:inline` 或 `wp:anchor`。
fn shell(dom: &Dom, drawing: NodeId) -> Result<NodeId> {
    live_children(dom, drawing)
        .find(|&c| dom.is(c, wp(LocalName::Inline)) || dom.is(c, wp(LocalName::Anchor)))
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "w:drawing 里没有 inline / anchor"))
}

fn require_drawing(s: &EditSession, drawing: NodeId) -> Result<()> {
    let dom = s.dom();
    if (drawing.0 as usize) >= dom.node_count()
        || dom.node(drawing).dirty == Dirty::Deleted
        || !dom.is(drawing, w(LocalName::Drawing))
    {
        return Err(Error::edit(DiagCode::EditBadPosition, "目标不是活的 w:drawing"));
    }
    Ok(())
}

/// 一次几何改动（`None` = 不动这一项）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawingGeometry {
    /// 显示尺寸（EMU）。
    pub extent_emu: Option<(i64, i64)>,
    /// 锚定位置（EMU）：`(positionH, positionV)` 的 `wp:posOffset`。随文图片没有位置，给了也不动。
    pub pos_offset_emu: Option<(i64, i64)>,
    /// 旋转角（度）；`Some(None)` = 去掉旋转。
    pub rot_deg: Option<Option<i64>>,
    pub flip_h: Option<bool>,
    pub flip_v: Option<bool>,
    /// 裁剪窗（`a:srcRect` 的四个千分比）；`Some(None)` = 去掉裁剪。
    pub crop: Option<Option<SrcRect>>,
}

/// `a:srcRect`：四边各裁掉的千分比（0..100000）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SrcRect {
    pub l: i64,
    pub t: i64,
    pub r: i64,
    pub b: i64,
}

/// `SetDrawingGeometry`。
pub(crate) fn set_geometry(
    s: &mut EditSession,
    drawing: NodeId,
    geom: &DrawingGeometry,
) -> Result<MutationResult> {
    require_drawing(s, drawing)?;
    let part = s.main_part();
    let dom = s.dom();
    let shell = shell(dom, drawing)?;
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, w(LocalName::P))) {
        plan.touch(p);
    }
    let set = |plan: &mut MutationPlan, node: NodeId, name: QName, v: String| {
        plan.node_edits.push(NodeEdit::SetAttr { node: Target::Node(node), name, value: v });
    };
    // ① `wp:extent` 与每个 `a:ext`（图片自己的 `a:xfrm/a:ext`）
    if let Some((cx, cy)) = geom.extent_emu {
        let (cx, cy) = (cx.max(1), cy.max(1));
        let none = |l: LocalName| QName::new(NsId::None, l);
        if let Some(ext) = live_children(dom, shell).find(|&c| dom.is(c, wp(LocalName::Extent))) {
            set(&mut plan, ext, none(LocalName::Cx), cx.to_string());
            set(&mut plan, ext, none(LocalName::Cy), cy.to_string());
        }
        // 旋转后的外接框（与 6.7 新建图片同一条公式）
        let rot = current_rot(dom, drawing, geom).rem_euclid(360);
        let rad = rot as f64 * std::f64::consts::PI / 180.0;
        let bw = (cx as f64 * rad.cos()).abs() + (cy as f64 * rad.sin()).abs();
        let bh = (cx as f64 * rad.sin()).abs() + (cy as f64 * rad.cos()).abs();
        let ex = (((bw - cx as f64) / 2.0).round() as i64).max(0);
        let ey = (((bh - cy as f64) / 2.0).round() as i64).max(0);
        if let Some(ee) =
            live_children(dom, shell).find(|&c| dom.is(c, wp(LocalName::EffectExtent)))
        {
            for (n, v) in
                [(LocalName::L, ex), (LocalName::T, ey), (LocalName::R, ex), (LocalName::B, ey)]
            {
                set(&mut plan, ee, none(n), v.to_string());
            }
        }
        for ext in xfrm_exts(dom, drawing) {
            set(&mut plan, ext, none(LocalName::Cx), cx.to_string());
            set(&mut plan, ext, none(LocalName::Cy), cy.to_string());
        }
    }
    // ② 锚定位置
    if let Some((x, y)) = geom.pos_offset_emu {
        for (which, v) in [(LocalName::PositionH, x), (LocalName::PositionV, y)] {
            let Some(pos) = live_children(dom, shell).find(|&c| dom.is(c, wp(which))) else {
                continue;
            };
            match live_children(dom, pos).find(|&c| dom.is(c, wp(LocalName::PosOffset))) {
                Some(off) => super::ops::set_segment_text(dom, off, &v.to_string(), &mut plan),
                None => plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(pos),
                    before: None,
                    node: NewElement::new(wp(LocalName::PosOffset)).with_text(v.to_string()),
                }),
            }
        }
    }
    // ③ `a:xfrm` 的旋转 / 翻转
    for xfrm in xfrms(dom, drawing) {
        let none = |l: LocalName| QName::new(NsId::None, l);
        if let Some(rot) = geom.rot_deg {
            match rot {
                Some(d) => set(&mut plan, xfrm, none(LocalName::Rot), (d * 60_000).to_string()),
                None => plan.node_edits.push(NodeEdit::RemoveAttr {
                    node: Target::Node(xfrm),
                    name: none(LocalName::Rot),
                }),
            }
        }
        for (flag, name) in [(geom.flip_h, LocalName::FlipH), (geom.flip_v, LocalName::FlipV)] {
            match flag {
                Some(true) => set(&mut plan, xfrm, none(name), "1".into()),
                Some(false) => plan
                    .node_edits
                    .push(NodeEdit::RemoveAttr { node: Target::Node(xfrm), name: none(name) }),
                None => {}
            }
        }
    }
    // ④ 裁剪窗
    if let Some(crop) = geom.crop {
        for fill in blip_fills(dom, drawing) {
            let existing = live_children(dom, fill).find(|&c| dom.is(c, a(LocalName::SrcRect)));
            match (crop, existing) {
                (None, Some(n)) => plan.node_edits.push(NodeEdit::Delete(n)),
                (None, None) => {}
                (Some(r), old) => {
                    let mut e = NewElement::new(a(LocalName::SrcRect));
                    let none = |l: LocalName| QName::new(NsId::None, l);
                    for (n, v) in [
                        (LocalName::L, r.l),
                        (LocalName::T, r.t),
                        (LocalName::R, r.r),
                        (LocalName::B, r.b),
                    ] {
                        if v != 0 {
                            e.push_attr(none(n), v.to_string());
                        }
                    }
                    match old {
                        Some(n) => plan.node_edits.push(NodeEdit::Replace { old: n, node: e }),
                        // `a:srcRect` 是 `a:blipFill` 的第一个子元素（在 `a:stretch` 之前）
                        None => plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(fill),
                            before: live_children(dom, fill)
                                .find(|&c| !dom.is(c, a(LocalName::Blip))),
                            node: e,
                        }),
                    }
                }
            }
        }
    }
    s.commit_plan(plan)
}

/// 这次改完之后的旋转角（度）：这次给了就用这次的，否则读现有的 `a:xfrm/@rot`。
fn current_rot(dom: &Dom, drawing: NodeId, geom: &DrawingGeometry) -> i64 {
    if let Some(r) = geom.rot_deg {
        return r.unwrap_or(0);
    }
    xfrms(dom, drawing)
        .into_iter()
        .find_map(|x| {
            dom.attr_value(x, QName::new(NsId::None, LocalName::Rot))?.trim().parse::<i64>().ok()
        })
        .map_or(0, |v| v / 60_000)
}

/// 绘图里全部 `a:xfrm`（图片的 `pic:spPr` 与形状的 `wps:spPr` 都算）。
fn xfrms(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
    dom.descendants(drawing)
        .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
        .filter(|&n| dom.is(n, a(LocalName::Xfrm)))
        .collect()
}

/// 每个 `a:xfrm` 下的 `a:ext`。
fn xfrm_exts(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
    xfrms(dom, drawing)
        .into_iter()
        .filter_map(|x| live_children(dom, x).find(|&c| dom.is(c, a(LocalName::Ext))))
        .collect()
}

fn blip_fills(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
    dom.descendants(drawing)
        .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
        .filter(|&n| {
            dom.is(n, a(LocalName::BlipFill))
                || dom.is(n, QName::new(NsId::Pic, LocalName::BlipFill))
        })
        .collect()
}

/// `SetDrawingZOrder`：`relativeHeight = Z_BASE + z`（TS `applyImageZOrder`）。随文图片没有
/// z-order，给了也不动（`wp:inline` 上没有这个属性）。
pub(crate) fn set_z_order(s: &mut EditSession, drawing: NodeId, z: i64) -> Result<MutationResult> {
    require_drawing(s, drawing)?;
    let part = s.main_part();
    let dom = s.dom();
    let shell = shell(dom, drawing)?;
    if !dom.is(shell, wp(LocalName::Anchor)) {
        return Err(super::ops::unsupported("随文图片没有 z-order；先用 SetDrawingWrap 改成锚定"));
    }
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, w(LocalName::P))) {
        plan.touch(p);
    }
    plan.node_edits.push(NodeEdit::SetAttr {
        node: Target::Node(shell),
        name: QName::new(NsId::None, LocalName::RelativeHeight),
        value: (Z_BASE + z).max(0).to_string(),
    });
    s.commit_plan(plan)
}

/// `SetShapeStyle`：`wps:spPr` 的填充与描边。`None` = 不动，`Some(None)` = 无填充 / 无描边。
pub(crate) fn set_shape_style(
    s: &mut EditSession,
    shape: NodeId,
    fill: Option<Option<String>>,
    outline: Option<Option<String>>,
) -> Result<MutationResult> {
    let part = s.main_part();
    let dom = s.dom();
    if (shape.0 as usize) >= dom.node_count() || dom.node(shape).dirty == Dirty::Deleted {
        return Err(Error::edit(DiagCode::EditBadPosition, "形状节点不存在"));
    }
    // 目标可以是 `wps:wsp` 自己，也可以是它的 `wps:spPr`
    let sp_pr = if dom.is(shape, QName::new(NsId::Wps, LocalName::SpPr)) {
        shape
    } else {
        live_children(dom, shape)
            .find(|&c| dom.is(c, QName::new(NsId::Wps, LocalName::SpPr)))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "形状里没有 wps:spPr"))?
    };
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(sp_pr).find(|&x| dom.is(x, w(LocalName::P))) {
        plan.touch(p);
    }
    if let Some(f) = fill {
        // `a:solidFill` / `a:noFill` 在 `a:prstGeom` 之后、`a:ln` 之前（CT_ShapeProperties）
        let before = live_children(dom, sp_pr).find(|&c| dom.is(c, a(LocalName::Ln)));
        replace_fill(dom, &mut plan, sp_pr, before, f.as_deref());
    }
    if let Some(o) = outline {
        let ln = live_children(dom, sp_pr).find(|&c| dom.is(c, a(LocalName::Ln)));
        match ln {
            Some(ln) => replace_fill(dom, &mut plan, ln, None, o.as_deref()),
            None => {
                let mut e = NewElement::new(a(LocalName::Ln));
                e.push_child(fill_element(o.as_deref()));
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(sp_pr),
                    before: None,
                    node: e,
                });
            }
        }
    }
    s.commit_plan(plan)
}

/// 容器里的 `a:solidFill` / `a:noFill` 换成新的。
fn replace_fill(
    dom: &Dom,
    plan: &mut MutationPlan,
    container: NodeId,
    before: Option<NodeId>,
    color: Option<&str>,
) {
    let old: Vec<NodeId> = live_children(dom, container)
        .filter(|&c| {
            dom.is(c, a(LocalName::SolidFill))
                || dom.is(c, a(LocalName::NoFill))
                || dom.is(c, a(LocalName::GradFill))
                || dom.is(c, a(LocalName::BlipFill))
        })
        .collect();
    let at = old.first().copied().or(before);
    for n in &old {
        plan.node_edits.push(NodeEdit::Delete(*n));
    }
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(container),
        before: at.filter(|n| !old.contains(n)).or(before),
        node: fill_element(color),
    });
}

fn fill_element(color: Option<&str>) -> NewElement {
    match color {
        Some(rgb) => NewElement::new(a(LocalName::SolidFill)).with_child(
            NewElement::new(a(LocalName::SrgbClr))
                .with_attr(QName::new(NsId::None, LocalName::Val), rgb.trim_start_matches('#')),
        ),
        None => NewElement::new(a(LocalName::NoFill)),
    }
}
