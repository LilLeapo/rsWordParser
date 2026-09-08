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
pub const Z_BASE: i64 = super::media_ops::Z_ORDER_BASE;

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

/// 一根轴的定位（`wp:positionH` / `wp:positionV`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorAxis {
    /// `@relativeFrom`：`column` / `page` / `margin` / `paragraph` / `character` / `line` …
    pub relative_from: String,
    pub pos: AxisPos,
}

/// 轴上的位置：偏移或对齐（`wp:posOffset` / `wp:align`，两者互斥）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxisPos {
    /// `wp:posOffset`（EMU）。
    Offset(i64),
    /// `wp:align`：`left` / `center` / `right` / `top` / `bottom` / `inside` / `outside`。
    Align(String),
}

/// 锚定图片的两根轴。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorPos {
    pub h: AnchorAxis,
    pub v: AnchorAxis,
}

/// 换壳时**搬进新壳**的子元素，按 `CT_Inline` / `CT_Anchor` 的次序。壳里别的东西
/// （`simplePos` / `positionH` / `positionV` / `wrap*` / `wp14:sizeRel*`）只属于旧壳，跟着它一起走。
const CARRIED: [(NsId, LocalName); 5] = [
    (NsId::Wp, LocalName::Extent),
    (NsId::Wp, LocalName::EffectExtent),
    (NsId::Wp, LocalName::DocPr),
    (NsId::Wp, LocalName::CNvGraphicFramePr),
    (NsId::A, LocalName::Graphic),
];

fn is_wrap_element(dom: &Dom, n: NodeId) -> bool {
    [
        LocalName::WrapNone,
        LocalName::WrapSquare,
        LocalName::WrapTight,
        LocalName::WrapThrough,
        LocalName::WrapTopAndBottom,
    ]
    .iter()
    .any(|&l| dom.is(n, wp(l)))
}

/// `SetDrawingWrap`：`wp:inline ↔ wp:anchor` 的绕排切换。
///
/// 壳的种类不变（锚定 → 锚定）时**就地改**：只换绕排元素、`behindDoc` / `relativeHeight`、
/// 给了 `pos` 才动 `positionH` / `positionV`。种类变了才重建外壳，`wp:extent` /
/// `effectExtent` / `docPr` / `cNvGraphicFramePr` / `a:graphic` 用 `move_within_part`
/// 搬进去，原字节保住（`SAVE-08`）。
pub(crate) fn set_wrap(
    s: &mut EditSession,
    drawing: NodeId,
    wrap: Option<super::media_ops::ImageWrap>,
    pos: Option<&AnchorPos>,
    z_order: Option<i64>,
) -> Result<MutationResult> {
    require_drawing(s, drawing)?;
    let part = s.main_part();
    let dom = s.dom();
    let old = shell(dom, drawing)?;
    let was_anchor = dom.is(old, wp(LocalName::Anchor));
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, w(LocalName::P))) {
        plan.touch(p);
    }
    match (was_anchor, wrap) {
        // 锚定 → 锚定：就地改
        (true, Some(new_wrap)) => in_place_anchor(dom, &mut plan, old, new_wrap, pos, z_order),
        // 随文 → 随文：绕排本来就没有，只有 `pos` / `z` 无处可放
        (false, None) => {}
        // 换壳
        (_, next) => rebuild_shell(dom, &mut plan, drawing, old, next, pos, z_order),
    }
    s.commit_plan(plan)
}

/// 锚定壳的就地改写。
fn in_place_anchor(
    dom: &Dom,
    plan: &mut MutationPlan,
    anchor: NodeId,
    wrap: super::media_ops::ImageWrap,
    pos: Option<&AnchorPos>,
    z_order: Option<i64>,
) {
    use super::media_ops::ImageWrap;
    let none = |l: LocalName| QName::new(NsId::None, l);
    plan.node_edits.push(NodeEdit::SetAttr {
        node: Target::Node(anchor),
        name: none(LocalName::BehindDoc),
        value: if wrap == ImageWrap::Behind { "1".into() } else { "0".into() },
    });
    if let Some(z) = z_order {
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(anchor),
            name: none(LocalName::RelativeHeight),
            value: (Z_BASE + z).max(0).to_string(),
        });
    }
    // 绕排元素：旧的删掉，新的插在原位（没有旧的就插在 `docPr` 之前）
    let old_wrap = live_children(dom, anchor).find(|&c| is_wrap_element(dom, c));
    let polygon = old_wrap
        .filter(|_| keeps_polygon(dom, old_wrap, wrap))
        .and_then(|w| live_children(dom, w).find(|&c| dom.is(c, wp(LocalName::WrapPolygon))));
    let before =
        old_wrap.or_else(|| live_children(dom, anchor).find(|&c| dom.is(c, wp(LocalName::DocPr))));
    let at = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(anchor),
        before,
        node: wrap_element(wrap, polygon.is_none()),
    });
    if let Some(poly) = polygon {
        plan.node_edits.push(NodeEdit::Move { node: poly, parent: Target::New(at), before: None });
    }
    if let Some(w) = old_wrap {
        plan.node_edits.push(NodeEdit::Delete(w));
    }
    let Some(p) = pos else {
        // 没给位置：`square-left` ↔ `square-right` 说的正是图靠哪一边，所以横轴**用对齐写着**
        // 的时候跟着绕排走；写着明确偏移的（用户摆过位置）不动。
        if let Some(align) = live_children(dom, anchor)
            .find(|&c| dom.is(c, wp(LocalName::PositionH)))
            .and_then(|h| live_children(dom, h).find(|&c| dom.is(c, wp(LocalName::Align))))
        {
            super::ops::set_segment_text(dom, align, super::media_ops::default_align(wrap), plan);
        }
        return;
    };
    for (which, axis) in [(LocalName::PositionH, &p.h), (LocalName::PositionV, &p.v)] {
        let e = position_element(which, axis);
        match live_children(dom, anchor).find(|&c| dom.is(c, wp(which))) {
            Some(old) => plan.node_edits.push(NodeEdit::Replace { old, node: e }),
            None => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(anchor),
                before: live_children(dom, anchor).find(|&c| !dom.is(c, wp(LocalName::SimplePos))),
                node: e,
            }),
        }
    }
}

/// 同类绕排（紧密 ↔ 穿越）之间保留原来的 `wp:wrapPolygon`，别的情况重新生成矩形。
fn keeps_polygon(dom: &Dom, old_wrap: Option<NodeId>, next: super::media_ops::ImageWrap) -> bool {
    use super::media_ops::ImageWrap;
    let polygonal = matches!(
        next,
        ImageWrap::TightLeft
            | ImageWrap::TightRight
            | ImageWrap::ThroughLeft
            | ImageWrap::ThroughRight
    );
    polygonal
        && old_wrap.is_some_and(|w| {
            dom.is(w, wp(LocalName::WrapTight)) || dom.is(w, wp(LocalName::WrapThrough))
        })
}

/// 重建外壳：新壳插在旧壳之前，要保的子元素搬进去，旧壳删掉。
fn rebuild_shell(
    dom: &Dom,
    plan: &mut MutationPlan,
    drawing: NodeId,
    old: NodeId,
    wrap: Option<super::media_ops::ImageWrap>,
    pos: Option<&AnchorPos>,
    z_order: Option<i64>,
) {
    let shell_at = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(drawing),
        before: Some(old),
        node: shell_element(wrap, pos, z_order),
    });
    let carry = |plan: &mut MutationPlan, upto: usize| {
        for &(ns, local) in &CARRIED[..upto] {
            if let Some(n) = live_children(dom, old).find(|&c| dom.is(c, QName::new(ns, local))) {
                plan.node_edits.push(NodeEdit::Move {
                    node: n,
                    parent: Target::New(shell_at),
                    before: None,
                });
            }
        }
    };
    // `wp:extent` / `effectExtent` 在绕排元素之前，`docPr` 之后的三个在它之后
    carry(plan, 2);
    if let Some(w) = wrap {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(shell_at),
            before: None,
            node: wrap_element(w, true),
        });
    }
    for &(ns, local) in &CARRIED[2..] {
        if let Some(n) = live_children(dom, old).find(|&c| dom.is(c, QName::new(ns, local))) {
            plan.node_edits.push(NodeEdit::Move {
                node: n,
                parent: Target::New(shell_at),
                before: None,
            });
        }
    }
    plan.node_edits.push(NodeEdit::Delete(old));
}

/// 新的 `wp:inline` / `wp:anchor` 外壳（不含要搬进去的子元素与绕排元素）。
fn shell_element(
    wrap: Option<super::media_ops::ImageWrap>,
    pos: Option<&AnchorPos>,
    z_order: Option<i64>,
) -> NewElement {
    use super::media_ops::ImageWrap;
    let none = |l: LocalName| QName::new(NsId::None, l);
    let Some(wrap) = wrap else {
        let mut e = NewElement::new(wp(LocalName::Inline));
        for l in [LocalName::DistT, LocalName::DistB, LocalName::DistL, LocalName::DistR] {
            e.push_attr(none(l), "0");
        }
        return e;
    };
    let mut e = NewElement::new(wp(LocalName::Anchor));
    for (l, v) in [
        (LocalName::DistT, "0"),
        (LocalName::DistB, "0"),
        (LocalName::DistL, "114300"),
        (LocalName::DistR, "114300"),
        (LocalName::SimplePos, "0"),
    ] {
        e.push_attr(none(l), v);
    }
    e.push_attr(
        none(LocalName::RelativeHeight),
        (Z_BASE + z_order.unwrap_or(0)).max(0).to_string(),
    );
    e.push_attr(none(LocalName::BehindDoc), if wrap == ImageWrap::Behind { "1" } else { "0" });
    for (l, v) in
        [(LocalName::Locked, "0"), (LocalName::LayoutInCell, "1"), (LocalName::AllowOverlap, "1")]
    {
        e.push_attr(none(l), v);
    }
    e.push_child(
        NewElement::new(wp(LocalName::SimplePos))
            .with_attr(none(LocalName::X), "0")
            .with_attr(none(LocalName::Y), "0"),
    );
    let default = default_pos(wrap);
    let pos = pos.unwrap_or(&default);
    e.push_child(position_element(LocalName::PositionH, &pos.h));
    e.push_child(position_element(LocalName::PositionV, &pos.v));
    e
}

/// 没给位置时的缺省（TS `applyImageWrap`：横向按绕排方向对齐、纵向贴段落）。
fn default_pos(wrap: super::media_ops::ImageWrap) -> AnchorPos {
    AnchorPos {
        h: AnchorAxis {
            relative_from: "column".into(),
            pos: AxisPos::Align(super::media_ops::default_align(wrap).into()),
        },
        v: AnchorAxis { relative_from: "paragraph".into(), pos: AxisPos::Offset(0) },
    }
}

fn position_element(which: LocalName, axis: &AnchorAxis) -> NewElement {
    let mut e = NewElement::new(wp(which))
        .with_attr(QName::new(NsId::None, LocalName::RelativeFrom), axis.relative_from.clone());
    e.push_child(match &axis.pos {
        AxisPos::Offset(v) => NewElement::new(wp(LocalName::PosOffset)).with_text(v.to_string()),
        AxisPos::Align(a) => NewElement::new(wp(LocalName::Align)).with_text(a.clone()),
    });
    e
}

/// 绕排元素。`fresh_polygon` 为真时给紧密 / 穿越配一个矩形多边形（否则等着把旧的搬进来）。
fn wrap_element(wrap: super::media_ops::ImageWrap, fresh_polygon: bool) -> NewElement {
    use super::media_ops::ImageWrap;
    let none = |l: LocalName| QName::new(NsId::None, l);
    let both = |e: NewElement| e.with_attr(none(LocalName::WrapText), "bothSides");
    match wrap {
        ImageWrap::Front | ImageWrap::Behind => NewElement::new(wp(LocalName::WrapNone)),
        ImageWrap::TopBottom => NewElement::new(wp(LocalName::WrapTopAndBottom)),
        ImageWrap::SquareLeft | ImageWrap::SquareRight => {
            both(NewElement::new(wp(LocalName::WrapSquare)))
        }
        ImageWrap::TightLeft
        | ImageWrap::TightRight
        | ImageWrap::ThroughLeft
        | ImageWrap::ThroughRight => {
            let name = if matches!(wrap, ImageWrap::TightLeft | ImageWrap::TightRight) {
                LocalName::WrapTight
            } else {
                LocalName::WrapThrough
            };
            let mut e = both(NewElement::new(wp(name)));
            if fresh_polygon {
                e.push_child(rect_polygon());
            }
            e
        }
    }
}

/// 整幅图的矩形多边形（21600 = 一幅图的宽 / 高，OOXML 的相对坐标）。
fn rect_polygon() -> NewElement {
    let none = |l: LocalName| QName::new(NsId::None, l);
    let pt = |name: LocalName, x: &str, y: &str| {
        NewElement::new(wp(name)).with_attr(none(LocalName::X), x).with_attr(none(LocalName::Y), y)
    };
    let mut e = NewElement::new(wp(LocalName::WrapPolygon))
        .with_attr(none(LocalName::Edited), "0")
        .with_child(pt(LocalName::Start, "0", "0"));
    for (x, y) in [("0", "21600"), ("21600", "21600"), ("21600", "0"), ("0", "0")] {
        e.push_child(pt(LocalName::LineTo, x, y));
    }
    e
}
