//! `textboxes[]` 的载荷（TS `buildWpsBox` / `lineBoxOf` / `applyAnchor` / `txbxContentParas`；
//! `spec/15` 任务 4.6b）。
//!
//! 分类在 [`super::textbox`]，这里只管「一个框长什么样」：几何、填充、描边、内边距、框里的段落，
//! 以及锚定带来的偏移与浮动。px 换算与 band 计算都是投影，模型侧仍然只有 EMU 原值
//! （`spec/15` 分层决策）。
//!
//! 用到节几何的两处（`resolveAnchorPagePos` 的页面 / 页边距对齐、跨栏宽框的 band 推断）需要
//! 节的页宽页边距，那是 M5 的东西，这里先不做——受影响的字段是 `pageRelX` / `pageRelV` /
//! `pagePinned` / `bandOverflow`。

use serde_json::{Map, Value};

use crate::model::CustomGeom;
use crate::model::drawing::{
    Anchor, AnchorGeom, BodyPr, DrawingDisplay, Extent, FillKind, ShapeDisplay, StyleRef, Wrap,
};
use crate::model::section::SectionGeom;
use crate::model::units::{EMU_PER_PX, emu_to_px, parse_length, parse_style};
use crate::model::vml::{VmlKind, VmlShape, vml_color};
use crate::model::{Block, Display};
use crate::resolve::drawingml::{DrawingColor, Rgb, average, color_in, hex, parse_color};
use crate::xml::{LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;
use crate::bind::native::json::{set, set_if, set_some};

/// Word 的 `relativeHeight` 基数。
const Z_ORDER_BASE: i64 = 251_658_240;
/// 零高连线保留的抓取带（px）。
const LINE_GRAB_PX: i64 = 12;
/// 浮动框旁边还能排下字的最窄栏缝（36 px，TS `MIN_WRAP_SLIVER_EMU`）。
const MIN_WRAP_SLIVER_EMU: i64 = 36 * 9525;

fn px_round(emu: i64) -> i64 {
    emu_to_px(emu as f64).round() as i64
}

/// 保留两位小数（TS 的 `Math.round(x * 100) / 100`）。
fn px2(emu: i64) -> f64 {
    (emu as f64 / EMU_PER_PX * 100.0).round() / 100.0
}

// ---- 颜色 ---------------------------------------------------------------------------------------

fn rgb_hex(ctx: &Ctx<'_>, c: &DrawingColor) -> Option<String> {
    c.to_rgb(ctx.resolver.palette()).map(hex)
}

/// 颜色容器节点 → hex。
pub(super) fn color_hex(ctx: &Ctx<'_>, node: NodeId) -> Option<String> {
    rgb_hex(ctx, &color_in(ctx.dom, node)?)
}

/// `a:gradFill` 的等权平均（TS `gradFillApproxHex`）。
fn grad_hex(ctx: &Ctx<'_>, grad: NodeId) -> Option<String> {
    let dom = ctx.dom;
    let gs_lst = dom.semantic_children(grad).find(|&c| dom.is(c, a(LocalName::GsLst)))?;
    let stops: Vec<Rgb> = dom
        .semantic_children(gs_lst)
        .filter(|&c| dom.is(c, a(LocalName::Gs)))
        .filter_map(|c| color_in(dom, c))
        .filter_map(|c| c.to_rgb(ctx.resolver.palette()))
        .collect();
    average(&stops).map(hex)
}

/// `a:pattFill/a:fgClr`。
fn patt_hex(ctx: &Ctx<'_>, patt: NodeId) -> Option<String> {
    let dom = ctx.dom;
    let fg = dom.semantic_children(patt).find(|&c| dom.is(c, a(LocalName::FgClr)))?;
    color_hex(ctx, fg)
}

/// `a:fillRef` / `a:lnRef` / `a:fontRef`：`idx > 0` 才算引用了主题样式。
fn style_ref_hex(ctx: &Ctx<'_>, r: Option<&StyleRef>, require_idx: bool) -> Option<String> {
    let r = r?;
    if require_idx && r.idx.unwrap_or(0) <= 0 {
        return None;
    }
    color_hex(ctx, r.node)
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

// ---- 形状框 -------------------------------------------------------------------------------------

/// 一个 `wps:wsp` → 框的 JSON（TS `buildWpsBox` 的后半段：几何与样式）。
pub(super) fn wps_box_json(
    ctx: &Ctx<'_>,
    s: &ShapeDisplay,
    group_fill: Option<&str>,
    nested: bool,
    txbx_index: Option<usize>,
) -> Map<String, Value> {
    let mut o = Map::new();
    set_if!(&mut o, "readOnly" => nested);
    set_some!(&mut o,
        "txbxIndex" => txbx_index.filter(|_| !nested).map(|i| i as i64),
        "shapeId" => (!nested).then(|| s.cnv_id.clone()).flatten(),
    );
    fill_and_line(ctx, s, group_fill, &mut o);
    // Word 会裁掉溢出的文字，除非形状自适应；带上固定高度，稀疏的高框才不会撑爆版面。
    let height = (!s.body.is_some_and(|b| b.auto_fit))
        .then(|| s.ext.map(|e| e.cy).filter(|&cy| cy > 0).map(px_round))
        .flatten();
    set_some!(&mut o,
        "prst" => s.prst.as_deref().filter(|p| *p != "rect"),
        "pathData" => s.geom.as_ref().and_then(|g| path_data(g, s.ext)).map(Value::Object),
        "rotDeg" => s.rot_60k.filter(|&r| r != 0).map(|r| (r as f64 / 60_000.0).round() as i64),
        "widthPx" => s.ext.map(|e| e.cx).filter(|&cx| cx > 0).map(px_round),
        "heightPx" => height,
        "minHeightPx" => height,
    );
    if let Some(b) = s.body {
        body_pr(b, &mut o);
    }
    o
}

/// `a:custGeom` → `pathData`（TS `parseCustGeom` 的输出层）。
///
/// 每条 `a:path` 按它自己声明的 `@w` / `@h` 归一化到 0..1（缺省时退到形状的 `a:ext`），
/// 保留 5 位小数；按 `@fill` / `@stroke` 分进 `path` / `fillPath` / `strokePath` 三层，
/// 两者都是 none 的路径不画。
pub(super) fn path_data(geom: &CustomGeom, ext: Option<Extent>) -> Option<Map<String, Value>> {
    let (mut fill_only, mut stroke_only, mut both) = (Vec::new(), Vec::new(), Vec::new());
    for p in &geom.paths {
        if p.fill_none && p.stroke_none {
            continue;
        }
        let vw = p.w.or(ext.map(|e| e.cx)).filter(|&v| v != 0).unwrap_or(1) as f64;
        let vh = p.h.or(ext.map(|e| e.cy)).filter(|&v| v != 0).unwrap_or(1) as f64;
        let mut parts: Vec<String> = Vec::new();
        for c in &p.cmds {
            parts.push(c.letter().to_string());
            for pt in c.points() {
                parts.push(norm(pt[0] as f64 / vw));
                parts.push(norm(pt[1] as f64 / vh));
            }
        }
        if parts.is_empty() {
            continue;
        }
        let d = parts.join(" ");
        match (p.fill_none, p.stroke_none) {
            (true, _) => stroke_only.push(d),
            (_, true) => fill_only.push(d),
            _ => both.push(d),
        }
    }
    let mut o = Map::new();
    for (key, list) in [("path", both), ("fillPath", fill_only), ("strokePath", stroke_only)] {
        if !list.is_empty() {
            set(&mut o, key, list.join(" "));
        }
    }
    (!o.is_empty()).then_some(o)
}

/// 归一化坐标：5 位小数，去掉 `-0`。
pub(super) fn norm(v: f64) -> String {
    let r = (v * 100_000.0).round() / 100_000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

fn fill_and_line(
    ctx: &Ctx<'_>,
    s: &ShapeDisplay,
    group_fill: Option<&str>,
    o: &mut Map<String, Value>,
) {
    let no_fill = s.fill.as_ref().is_some_and(|f| f.kind == FillKind::None);
    let fill = s.fill.as_ref().filter(|_| !no_fill);
    let blip = fill
        .filter(|f| f.kind == FillKind::Blip)
        .filter(|f| f.blip.as_deref().is_some_and(|r| ctx.media.get(r).is_some()));
    let line = s.line.as_ref().filter(|l| !l.no_fill);
    set_some!(o,
        "fill" => fill.and_then(|f| match f.kind {
            FillKind::Solid => color_hex(ctx, f.node),
            FillKind::Gradient => grad_hex(ctx, f.node),
            FillKind::Pattern => patt_hex(ctx, f.node),
            // `a:grpFill` 继承所在组的填充
            FillKind::Group => group_fill.map(str::to_string),
            _ => None,
        }),
        "fillImageDataUrl" =>
            blip.and_then(|f| ctx.media.get(f.blip.as_deref()?)).map(|m| m.url.clone()),
        "borderColor" => line.and_then(|l| l.fill).and_then(|f| color_hex(ctx, f)),
        "borderWidthPx" => line.and_then(|l| l.width_emu).filter(|&w| w > 0).map(px2),
        "borderDash" => line.and_then(|l| l.dash.as_deref())
            .map(|d| if d.contains("dot") { "dotted" } else { "dashed" }),
    );
    set_if!(o, "fillTile" => blip.is_some_and(|f| f.tile));
    // `wps:style`：spPr 没写颜色的图库形状从主题引用取
    let need_fill = !o.contains_key("fill") && !o.contains_key("fillImageDataUrl") && !no_fill;
    let need_border = !o.contains_key("borderColor") && !s.line.as_ref().is_some_and(|l| l.no_fill);
    set_some!(o,
        "fill" => need_fill.then(|| style_ref_hex(ctx, s.fill_ref.as_ref(), true)).flatten(),
        "borderColor" =>
            need_border.then(|| style_ref_hex(ctx, s.line_ref.as_ref(), true)).flatten(),
        // `a:fontRef` 是图库形状文字颜色的出处：缺省蓝形状引用 lt1，所以 Word 里不写 run
        // 颜色也显示白字。run 自己写了 `w:color` 时以 run 为准。
        "textColor" => style_ref_hex(ctx, s.font_ref.as_ref(), false),
    );
}

fn body_pr(b: BodyPr, o: &mut Map<String, Value>) {
    let inset = |v: Option<i64>| v.filter(|&v| v >= 0).map(px2);
    set_some!(o,
        "insetLeftPx" => inset(b.l_ins),
        "insetTopPx" => inset(b.t_ins),
        "insetRightPx" => inset(b.r_ins),
        "insetBottomPx" => inset(b.b_ins),
        "vAlign" => match b.anchor {
            Some(Anchor::Bottom) => Some("bottom"),
            Some(Anchor::Center) => Some("center"),
            _ => None,
        },
    );
}

/// 连线形状 → 只读的线框（TS `lineBoxOf`）。合成的 `prst` 带上箭头信息。
pub(super) fn line_box_json(ctx: &Ctx<'_>, s: &ShapeDisplay) -> Map<String, Value> {
    let mut o = Map::new();
    set(&mut o, "readOnly", true);
    let border = s
        .line
        .as_ref()
        .and_then(|l| l.fill)
        .and_then(|f| literal_srgb(ctx, f))
        .or_else(|| style_ref_hex(ctx, s.line_ref.as_ref(), false))
        .unwrap_or_else(|| "000000".into());
    set(&mut o, "borderColor", border);
    let arrowed = |e: &Option<String>| e.as_deref().is_some_and(|t| t != "none");
    let (head, tail) = s
        .line
        .as_ref()
        .map(|l| (arrowed(&l.head_end), arrowed(&l.tail_end)))
        .unwrap_or((false, false));
    let prst = s.prst.as_deref().unwrap_or("line");
    let synth = if prst.starts_with("bentConnector") {
        "lineBent"
    } else if prst.starts_with("curvedConnector") {
        "lineCurved"
    } else if head && tail {
        "lineArrowDouble"
    } else if head || tail {
        "lineArrow"
    } else {
        "line"
    };
    set(&mut o, "prst", synth);
    if let Some(cx) = s.ext.map(|e| e.cx).filter(|&cx| cx > 0) {
        set(&mut o, "widthPx", px_round(cx));
    }
    let straight = matches!(synth, "line" | "lineArrow" | "lineArrowDouble");
    // 翻转在任何高度都算数：Word 的水平线 `cy="0"`，flipH 照样换箭头在哪一端
    let (mut flip_h, mut flip_v) = if straight { (s.flip_h, s.flip_v) } else { (false, false) };
    match s.ext.map(|e| e.cy).filter(|&cy| cy > 0) {
        Some(cy) => {
            let h = px_round(cy);
            set(&mut o, "heightPx", h);
            set(&mut o, "minHeightPx", h);
            // 真有竖直高度说明连线是斜着走的（≤12 px 留给我们自己插入的水平线抓取带）
            if straight && (h > LINE_GRAB_PX || flip_h || flip_v) {
                set(&mut o, "lineDiag", true);
            }
        }
        None => set(&mut o, "heightPx", LINE_GRAB_PX),
    }
    // `a:headEnd` 装饰的是起点：只有头箭头时反过来画，渲染器那一个箭头才落在对的一端
    if head && !tail && synth == "lineArrow" {
        (flip_h, flip_v) = (!flip_h, !flip_v);
    }
    if flip_h {
        set(&mut o, "flipH", true);
    }
    if flip_v {
        set(&mut o, "flipV", true);
    }
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut o, k, 0);
    }
    o
}

/// 只认字面 `a:srgbClr`（TS `lineBoxOf` 的描边取值就是直接读 `a:solidFill/a:srgbClr`）。
fn literal_srgb(ctx: &Ctx<'_>, container: NodeId) -> Option<String> {
    let dom = ctx.dom;
    let c = dom.semantic_children(container).find_map(|c| parse_color(dom, c))?;
    match c.base {
        crate::resolve::drawingml::ColorBase::Srgb(rgb) => {
            Some(hex([f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])]))
        }
        _ => None,
    }
}

// ---- VML 框 -------------------------------------------------------------------------------------

/// 一个 VML 文本框（TS `vmlBox` 的有字分支）。
pub(super) fn vml_box_json(
    s: &VmlShape,
    place: VmlPlace,
    txbx_index: Option<usize>,
) -> Map<String, Value> {
    let mut o = Map::new();
    set_some!(&mut o, "txbxIndex" => txbx_index.map(|i| i as i64));
    let h = vml_dim_px(s, "height", place.scale);
    set_some!(&mut o,
        "widthPx" => vml_dim_px(s, "width", place.scale),
        "heightPx" => h,
        "minHeightPx" => h,
        "fill" => (s.filled != Some(false)).then(|| s.fill_color.clone()).flatten(),
        "borderColor" => (s.stroked != Some(false)).then(|| {
            // 画布里的形状：VML 的描边缺省是开着的黑线，Word 真的会画出来。顶层形状不给
            // 这个缺省——转换器留下的占位家具要保持隐形。
            s.stroke_color.clone().or_else(|| place.scale.map(|_| "000000".to_string()))
        }).flatten(),
    );
    place_vml_box(s, place, &mut o);
    o
}

/// 组的仿射（TS `composeGroupCtm`）：`a:ext / a:chExt` 定缩放，`a:off - a:chOff*s` 定平移。
#[derive(Debug, Clone, Copy)]
pub(super) struct GroupCtm {
    pub sx: f64,
    pub sy: f64,
    pub tx: f64,
    pub ty: f64,
}

impl GroupCtm {
    /// 组链上的仿射。`chain` 从最外层到最内层。
    pub(super) fn compose(chain: &[&ShapeDisplay]) -> Option<GroupCtm> {
        let mut ctm = GroupCtm { sx: 1.0, sy: 1.0, tx: 0.0, ty: 0.0 };
        let mut any = false;
        for g in chain {
            let (ox, oy) = g.off.unwrap_or((0, 0));
            let (ex, ey) = g.ext.map_or((0, 0), |e| (e.cx, e.cy));
            let (cox, coy) = g.ch_off.unwrap_or((0, 0));
            let (cex, cey) = g.ch_ext.map_or((0, 0), |e| (e.cx, e.cy));
            let sx = if ex > 0 && cex > 0 { ex as f64 / cex as f64 } else { 1.0 };
            let sy = if ey > 0 && cey > 0 { ey as f64 / cey as f64 } else { 1.0 };
            ctm = GroupCtm {
                sx: ctm.sx * sx,
                sy: ctm.sy * sy,
                tx: ctm.tx + ctm.sx * (ox as f64 - cox as f64 * sx),
                ty: ctm.ty + ctm.sy * (oy as f64 - coy as f64 * sy),
            };
            any = true;
        }
        any.then_some(ctm)
    }
}

/// 组内形状：`a:off` 经仿射变成绝对偏移，宽高按比例缩放，并且一律浮动
/// （Word 就是把组内形状按映射后的位置绝对摆放的）。
pub(super) fn apply_group_ctm(ctm: GroupCtm, s: &ShapeDisplay, o: &mut Map<String, Value>) {
    set_some!(o,
        "offsetXEmu" => s.off.map(|(x, _)| (ctm.tx + x as f64 * ctm.sx).round() as i64),
        "offsetYEmu" => s.off.map(|(_, y)| (ctm.ty + y as f64 * ctm.sy).round() as i64),
    );
    if ctm.sx != 1.0
        && let Some(w) = o.get("widthPx").and_then(Value::as_i64)
    {
        set(o, "widthPx", (w as f64 * ctm.sx).round() as i64);
    }
    if ctm.sy != 1.0 {
        for key in ["heightPx", "minHeightPx"] {
            if let Some(h) = o.get(key).and_then(Value::as_i64) {
                set(o, key, (h as f64 * ctm.sy).round() as i64);
            }
        }
    }
    set(o, "floating", true);
}

// ---- VML 的组坐标与放置 -------------------------------------------------------------------------

/// 一个 VML 形状的放置环境：所在组的缩放（每个组坐标单位多少 px）与原点（离段落多少 px）。
///
/// `v:group`（Word 的「绘图画布」）给孩子定义了自己的坐标系：孩子 `style` 里的
/// `left` / `top` / `width` / `height` **不带单位**，量的是组坐标。
#[derive(Clone, Copy, Default)]
pub(super) struct VmlPlace {
    /// TS `VmlGroupScale`。
    pub scale: Option<(f64, f64)>,
    /// TS `VmlOrigin`。
    pub origin: Option<(f64, f64)>,
}

/// 组的缩放（TS `vmlGroupScale`）：组在页面上的 px 尺寸 ÷ `coordsize`。
pub(super) fn vml_group_scale(g: &VmlShape, parent: Option<(f64, f64)>) -> Option<(f64, f64)> {
    let w = vml_dim_px(g, "width", parent)?;
    let h = vml_dim_px(g, "height", parent)?;
    let (cw, ch) = g.coordsize?;
    (cw > 0 && ch > 0).then(|| (w as f64 / cw as f64, h as f64 / ch as f64))
}

/// 组内孩子的 `left` / `top` → 离段落多少 px（TS `vmlCoordPx`）：写了单位就是绝对长度，
/// 没写单位才按组坐标缩放。两种都要加上组原点。
pub(super) fn vml_coord_px(s: &VmlShape, key: &str, scale: (f64, f64), origin: (f64, f64)) -> f64 {
    let base = if key == "left" { origin.0 } else { origin.1 };
    let Some(l) = s.style_len(key) else { return base };
    match l.to_emu() {
        Some(emu) => base + emu_to_px(emu),
        None => base + l.value * if key == "left" { scale.0 } else { scale.1 },
    }
}

/// 形状 `style` 里的 `width` / `height` → px（TS `vmlShapeDimPx`）。
///
/// 写了单位就按单位算；没写单位且在组里就按组坐标缩放；没写单位又不在组里，VML 的缺省单位是磅。
pub(super) fn vml_dim_px(s: &VmlShape, key: &str, scale: Option<(f64, f64)>) -> Option<i64> {
    let l = s.style_len(key).filter(|l| l.value > 0.0)?;
    let px = match (l.to_emu(), scale) {
        (Some(emu), _) => emu_to_px(emu),
        (None, Some((sx, sy))) => l.value * if key == "width" { sx } else { sy },
        // 无单位、又不在组里：VML 的缺省单位是磅
        (None, None) => l.value / 72.0 * 96.0,
    };
    Some(px.round() as i64)
}

/// 把一个 VML 框放到页面上（TS `placeVmlBox`）：组外看 `position:absolute` 的页边距，
/// 组内按组原点加缩放后的组坐标。
pub(super) fn place_vml_box(s: &VmlShape, place: VmlPlace, o: &mut Map<String, Value>) {
    match (place.scale, place.origin) {
        (None, _) if s.is_absolute() => {
            set(o, "floating", true);
            for (key, out) in [("margin-left", "offsetXEmu"), ("margin-top", "offsetYEmu")] {
                if let Some(emu) = s.style_len(key).and_then(|l| l.to_emu()) {
                    set(o, out, emu.round() as i64);
                }
            }
        }
        (Some(scale), Some(origin)) => {
            set(o, "floating", true);
            set(
                o,
                "offsetXEmu",
                (vml_coord_px(s, "left", scale, origin) * EMU_PER_PX).round() as i64,
            );
            set(
                o,
                "offsetYEmu",
                (vml_coord_px(s, "top", scale, origin) * EMU_PER_PX).round() as i64,
            );
        }
        _ => {}
    }
}

/// 随文画布（没有 `position:absolute` 的 `v:group`）在文字流里占的位置。
///
/// 画布的孩子全都浮起来了，不给画布本身留一个占位框，整段就塌成零高、孩子飘到别的文字上。
pub(super) fn vml_canvas_box(
    g: &VmlShape,
    parent: Option<(f64, f64)>,
) -> Option<Map<String, Value>> {
    let w = vml_dim_px(g, "width", parent)?;
    let h = vml_dim_px(g, "height", parent)?;
    let mut o = Map::new();
    set(&mut o, "readOnly", true);
    set(&mut o, "widthPx", w);
    set(&mut o, "heightPx", h);
    set(&mut o, "minHeightPx", h);
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut o, k, 0);
    }
    o.insert("paras".into(), Value::Array(Vec::new()));
    Some(o)
}

/// 带 `v:imagedata` 的 VML 形状（TS `vmlPicBox`）：一张只读照片框。
///
/// 丢掉它就是丢掉页面上真实存在的图。
pub(super) fn vml_pic_box(
    ctx: &Ctx<'_>,
    s: &VmlShape,
    place: VmlPlace,
) -> Option<Map<String, Value>> {
    let m = ctx.media.get(s.imagedata.as_deref()?)?;
    let mut o = Map::new();
    set(&mut o, "readOnly", true);
    set(&mut o, "fillImageDataUrl", m.url.clone());
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut o, k, 0);
    }
    set_some!(&mut o,
        "widthPx" => vml_dim_px(s, "width", place.scale),
        "heightPx" => vml_dim_px(s, "height", place.scale),
    );
    place_vml_box(s, place, &mut o);
    o.insert("paras".into(), Value::Array(Vec::new()));
    Some(o)
}

/// 无字但有可见填充或描边的 VML 几何（标注块、画出来的表格底衬，TS `vmlGeomBox`）。
///
/// 这些形状 Word 是画得出来的，不该悄悄丢掉；但白色无描边的占位框 Word 什么都不画，
/// 跟着画反而多一块白斑。
pub(super) fn vml_geom_box(s: &VmlShape, place: VmlPlace) -> Option<Map<String, Value>> {
    if s.hr || s.is_hidden() {
        return None;
    }
    // 图片形状（t75）的图没解析出来，Word 连框都不画
    if s.is_picture_type() {
        return None;
    }
    let fill = (s.filled != Some(false)).then(|| s.fill_color.clone()).flatten();
    // VML 的描边缺省是开着的黑线，但只有画布（组）里的孩子吃这个缺省：顶层形状要显式
    // 写了 `strokecolor` 才画，转换器留下的占位家具才不会冒出边框。
    let stroke = (s.stroked != Some(false))
        .then(|| s.stroke_color.clone().or_else(|| place.scale.map(|_| "000000".to_string())))
        .flatten();
    // 白色又没描边：Word 什么都不画
    if stroke.is_none() && !fill.as_deref().is_some_and(|f| !f.eq_ignore_ascii_case("ffffff")) {
        return None;
    }
    let w = vml_dim_px(s, "width", place.scale)?;
    let h = vml_dim_px(s, "height", place.scale)?;
    let mut o = Map::new();
    set(&mut o, "readOnly", true);
    set(&mut o, "widthPx", w);
    set(&mut o, "heightPx", h);
    set(&mut o, "minHeightPx", h);
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut o, k, 0);
    }
    set_some!(&mut o,
        "fill" => fill,
        "borderColor" => stroke,
        "prst" => match s.kind {
            VmlKind::RoundRect => Some("roundRect"),
            VmlKind::Oval => Some("ellipse"),
            _ => None,
        },
    );
    if let Some(path) = s.path.as_deref() {
        // 转不出来的路径宁可整个形状不画，也不能退化成一个实心包围盒
        let (cw, ch) = s.coordsize?;
        let d = vml_path_to_norm_d(path, cw, ch)?;
        let mut pd = Map::new();
        set(&mut pd, "path", d);
        o.insert("pathData".into(), Value::Object(pd));
    }
    place_vml_box(s, place, &mut o);
    o.insert("paras".into(), Value::Array(Vec::new()));
    Some(o)
}

/// VML `@path` → 归一化到 0..1 的 SVG 路径（TS `vmlPathToNormD`）。
///
/// 只认直边子集：`m` / `l` 绝对、`t` / `r` 相对、`x` 闭合、`e` 结束，`nf` / `ns` 是填充 /
/// 描边提示不是几何。遇到曲线或弧就整条不给——画错的实心块比不画更糟。
fn vml_path_to_norm_d(path: &str, cw: i64, ch: i64) -> Option<String> {
    if cw <= 0 || ch <= 0 {
        return None;
    }
    let norm = |v: f64, c: i64| (v / c as f64 * 10_000.0).round() / 10_000.0;
    let b = path.as_bytes();
    let mut parts: Vec<String> = Vec::new();
    let (mut i, mut cx, mut cy) = (0usize, 0f64, 0f64);
    while i < b.len() {
        match b[i] {
            b' ' | b',' => i += 1,
            // `nf` / `ns` 要在 `e` 之前判，否则 `nf` 的 `n` 会先被当成未知命令
            b'n' if i + 1 < b.len() && (b[i + 1] == b'f' || b[i + 1] == b's') => i += 2,
            b'e' => i += 1,
            b'x' => {
                parts.push("Z".to_string());
                i += 1;
            }
            c @ (b'm' | b'l' | b't' | b'r') => {
                i += 1;
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'.' | b',' | b' '))
                {
                    i += 1;
                }
                let nums: Option<Vec<f64>> = path[start..i]
                    .trim()
                    .split([',', ' '])
                    // 分隔符之间缺一个坐标就是 0（`m,l,21600…`）
                    .map(|t| if t.is_empty() { Some(0.0) } else { t.parse().ok() })
                    .collect();
                let nums = nums?;
                if nums.is_empty() || nums.len() % 2 != 0 {
                    return None;
                }
                let (rel, mv) = (c == b't' || c == b'r', c == b'm' || c == b't');
                for (k, pair) in nums.as_chunks::<2>().0.iter().enumerate() {
                    cx = if rel { cx + pair[0] } else { pair[0] };
                    cy = if rel { cy + pair[1] } else { pair[1] };
                    let cmd = if k == 0 && mv { 'M' } else { 'L' };
                    parts.push(format!("{cmd} {} {}", norm(cx, cw), norm(cy, ch)));
                }
            }
            _ => return None,
        }
    }
    (parts.len() > 1).then(|| parts.join(" "))
}

/// VML WordArt（`v:textpath`）降级成一行带样式的文字（TS `vmlWordArtBox`）。
///
/// 不做路径扭曲与 3D，但文字按声明的大小与位置显示出来，好过一块不透明的占位芯片。
pub(super) fn vml_wordart_box(s: &VmlShape) -> Option<Map<String, Value>> {
    let text = s.textpath.as_deref().filter(|t| !t.trim().is_empty())?;
    let mut o = Map::new();
    set(&mut o, "readOnly", true);
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut o, k, 0);
    }
    let w_px = vml_dim_px(s, "width", None);
    let h_px = vml_dim_px(s, "height", None);
    // 浮动的 WordArt 和别的绝对定位形状一样离开文字流
    let absolute = s.is_absolute();
    let margin = |key: &str| {
        absolute.then(|| px_to_emu_round(s.style_len(key).map_or(0.0, |l| l.value) / 72.0 * 96.0))
    };
    set_if!(&mut o, "floating" => absolute);
    set_some!(&mut o,
        "widthPx" => w_px,
        "heightPx" => h_px,
        "offsetXEmu" => margin("margin-left"),
        "offsetYEmu" => margin("margin-top"),
    );
    let tp = s.textpath_style.as_deref().unwrap_or("");
    let tp_style = parse_style(tp);
    let get = |k: &str| tp_style.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let family = get("font-family").map(|v| v.trim().trim_matches('"').trim().to_string());
    let size_pt = get("font-size").and_then(parse_length).map(|l| l.value);

    // 填充色是**文字**颜色：`@fillcolor`，其次 `v:fill` 的 `color` / `color2`
    let fill = (s.filled != Some(false))
        .then(|| {
            s.fill_color.clone().or_else(|| {
                s.fill.as_ref().and_then(|f| {
                    f.color
                        .as_deref()
                        .and_then(vml_color)
                        .or_else(|| f.color2.as_deref().and_then(vml_color))
                })
            })
        })
        .flatten();
    if s.stroked != Some(false) {
        let mut outline = Map::new();
        set(&mut outline, "colorHex", s.stroke_color.clone().unwrap_or_else(|| "000000".into()));
        let weight =
            s.stroke_weight.as_deref().and_then(parse_length).map(|l| l.value).filter(|&v| v > 0.0);
        let px = match weight {
            Some(pt) => (pt / 72.0 * 96.0 * 100.0).round() / 100.0,
            None => 1.0,
        };
        set(&mut outline, "widthPx", px);
        set(&mut o, "textOutline", Value::Object(outline));
    }

    let mut run = Map::new();
    set(&mut run, "text", text);
    // `fitshape` 让字随框缩放；实测声明的 font-size 与框高对得上（框高 ≈ 字号 × 行距系数），
    // 所以优先用声明值，没有就按框高 / 1.4 估。
    let height_pt = h_px.map(|h| h as f64 / 96.0 * 72.0);
    let mut pt =
        size_pt.filter(|&v| v > 0.0).or_else(|| height_pt.filter(|&v| v > 0.0).map(|h| h / 1.4));
    // `fitpath` 会把长字串压进框里：按宽度收字号近似（一个字约 0.62 em）
    if let (Some(p), Some(w)) = (pt, w_px.map(|w| w as f64 / 96.0 * 72.0).filter(|&v| v > 0.0)) {
        let chars = text.chars().count() as f64;
        if chars > 0.0 {
            pt = Some(p.min(w / (0.62 * chars)).max(6.0));
        }
    }
    set(&mut o, "nowrap", true);
    set_some!(&mut run,
        "sizeHalfPoints" => pt.filter(|&v| v > 0.0).map(|p| (p * 2.0).round() as i64),
        "fontAscii" => family,
        "color" => fill,
    );
    let decl = |key: &str, want: &str| get(key).is_some_and(|v| v.trim() == want);
    set_if!(&mut run,
        "bold" => decl("font-weight", "bold"),
        "italic" => decl("font-style", "italic"),
    );
    let mut para = Map::new();
    para.insert("runs".into(), Value::Array(vec![Value::Object(run)]));
    set(&mut para, "align", "center");
    set(&mut o, "paras", Value::Array(vec![Value::Object(para)]));
    Some(o)
}

fn px_to_emu_round(px: f64) -> i64 {
    (px * EMU_PER_PX).round() as i64
}

// ---- 框里的段落 ---------------------------------------------------------------------------------

/// 框里内容流的块 → `paras[]`（TS `txbxContentParas`）。返回 `(段落, 是否只读)`。
pub(super) fn paras_json(ctx: &Ctx<'_>, content: &[Block]) -> (Vec<Value>, bool) {
    paras_json_in(ctx, content, None)
}

/// 同上，但内容可能属于**另一个 part**（外部文本框 part，`ShapeDisplay.content_part`）：
/// 那时换成那个 part 的投影上下文，并把整块标只读——内容不在本 part 里，重写本 part 的
/// `w:p` 列表救不了它（TS 同样把它排除在保存序号之外）。
pub(super) fn paras_json_in(
    ctx: &Ctx<'_>,
    content: &[Block],
    part: Option<crate::package::PartId>,
) -> (Vec<Value>, bool) {
    if let Some(p) = part {
        let Some(aux) = ctx.switch(p) else { return (Vec::new(), true) };
        let (paras, _) = paras_json(&aux, content);
        return (paras, true);
    }
    let (mut out, mut read_only) = own_paras(ctx, content);
    // 框里还套着框：TS 把所有层的 `w:txbxContent` 平铺进同一个 `paras`，并把整块标只读——
    // 提交时会重写外层的 `w:p` 列表，套在里面的形状就没了。
    let nested = nested_paras(ctx, content);
    if !nested.is_empty() {
        read_only = true;
        out.extend(nested);
    }
    (out, read_only)
}

/// 框自己那一层的段落。
fn own_paras(ctx: &Ctx<'_>, content: &[Block]) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut read_only = false;
    for b in content {
        match b {
            Block::Text(tb) => {
                // 框里直接放着结构化文档部件（`w:sdt`）：提交时会重写外层的 `w:p` 列表，
                // 部件就没了，所以整块只读（TS `txbxHasStructuredContent`）。
                read_only |= tb.sdt.is_some();
                let mut p = Map::new();
                let runs = super::blocks::runs_json(ctx, tb);
                set(&mut p, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
                if let Some(Value::Object(f)) = super::blocks::para_format_json(ctx, tb) {
                    for (k, v) in f {
                        p.insert(k, v);
                    }
                }
                if let Some(id) = &tb.style_id {
                    set(&mut p, "styleId", id.clone());
                }
                out.push(Value::Object(p));
            }
            // 框里的图片段落：`extractRuns(withImages)` 会把它变成一个带 image 的 run
            Block::Image(ib) => {
                let mut p = Map::new();
                let mut runs = Vec::new();
                if let Some(img) = ib.display.as_ref().and_then(|d| image_run(ctx, d)) {
                    runs.push(Value::Object(img));
                }
                set(&mut p, "runs", Value::Array(runs));
                out.push(Value::Object(p));
            }
            // 框里的表格：显示模型没有网格，一行一段、单元格之间空两格；块随之只读
            Block::Table(t) => {
                read_only = true;
                out.extend(table_rows(ctx, t.node));
            }
            Block::Protected(_) => read_only = true,
        }
    }
    (out, read_only)
}

/// 框内段落里再套的框，按文档序平铺出它们的段落。
fn nested_paras(ctx: &Ctx<'_>, content: &[Block]) -> Vec<Value> {
    let mut out = Vec::new();
    for b in content {
        let Block::Text(tb) = b else { continue };
        for i in &tb.inlines {
            let crate::model::Inline::Run(r) = i else { continue };
            for seg in &r.segments {
                let inner: Vec<&[Block]> = match seg.display.as_ref() {
                    Some(Display::Drawing(d)) => {
                        d.shapes.iter().map(|s| s.content.as_slice()).collect()
                    }
                    Some(Display::Vml(v)) => {
                        v.shapes.iter().map(|s| s.content.as_slice()).collect()
                    }
                    Some(Display::Formula(_)) | None => continue,
                };
                for blocks in inner {
                    if blocks.is_empty() {
                        continue;
                    }
                    let (paras, _) = own_paras(ctx, blocks);
                    out.extend(paras);
                    out.extend(nested_paras(ctx, blocks));
                }
            }
        }
    }
    out
}

fn image_run(ctx: &Ctx<'_>, d: &Display) -> Option<Map<String, Value>> {
    let pic = d.as_drawing()?.picture()?;
    let m = ctx.media.pick(pic.embed.as_deref(), pic.link.as_deref())?;
    let mut img = Map::new();
    set(&mut img, "dataUrl", m.url.clone());
    let mut run = Map::new();
    set(&mut run, "text", "");
    set(&mut run, "image", Value::Object(img));
    Some(run)
}

/// 框里的表格（TS `txbxTableParas`）。
///
/// 显示模型没有网格，所以一行渲染成一段：单元格之间隔两个 EN SPACE，同一格里的多个段落隔
/// 一个空格；单元格里再套的表格另起自己的行，跟在本行后面。整行没字又有嵌套行时，本行不出。
///
/// 单元格段落这里按整段文字给一个 run。真正按 `w:r` 分 run（连同格式）要等 M3 的表格模型；
/// 语料里框内表格的单元格都是单一格式，投影结果一致。
fn table_rows(ctx: &Ctx<'_>, tbl: NodeId) -> Vec<Value> {
    let dom = ctx.dom;
    let mut out = Vec::new();
    // 嵌套表格用显式工作栈，不递归（语料里有几千层嵌套的文档）
    enum Job {
        Table(NodeId),
        Row(Value),
    }
    let mut stack = vec![Job::Table(tbl)];
    while let Some(job) = stack.pop() {
        let t = match job {
            Job::Row(v) => {
                out.push(v);
                continue;
            }
            Job::Table(t) => t,
        };
        let mut jobs = Vec::new();
        for tr in children_through_sdt(dom, t, LocalName::Tr) {
            let mut runs: Vec<Value> = Vec::new();
            let mut nested = Vec::new();
            for tc in children_through_sdt(dom, tr, LocalName::Tc) {
                let mut cell: Vec<String> = Vec::new();
                for para in children_through_sdt(dom, tc, LocalName::P) {
                    let text = ctx.plain_text(para);
                    if text.trim().is_empty() {
                        continue;
                    }
                    cell.push(text);
                }
                if !cell.is_empty() {
                    if !runs.is_empty() {
                        runs.push(text_run("\u{2002}\u{2002}"));
                    }
                    for (i, t) in cell.iter().enumerate() {
                        if i > 0 {
                            runs.push(text_run(" "));
                        }
                        runs.push(text_run(t));
                    }
                }
                nested.extend(children_through_sdt(dom, tc, LocalName::Tbl));
            }
            if !runs.is_empty() || nested.is_empty() {
                let mut p = Map::new();
                set(&mut p, "runs", Value::Array(runs));
                jobs.push(Job::Row(Value::Object(p)));
            }
            jobs.extend(nested.into_iter().map(Job::Table));
        }
        stack.extend(jobs.into_iter().rev());
    }
    out
}

fn text_run(text: &str) -> Value {
    let mut r = Map::new();
    set(&mut r, "text", text);
    Value::Object(r)
}

/// 直接子元素里叫 `want` 的那些；遇到 `w:sdt` / `w:sdtContent` 就穿过去（TS `childrenThroughSdt`）。
fn children_through_sdt(dom: &crate::xml::Dom, node: NodeId, want: LocalName) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = dom.semantic_children(node).collect();
    stack.reverse();
    while let Some(n) = stack.pop() {
        if dom.is(n, QName::w(want)) {
            out.push(n);
        } else if dom.is(n, QName::w(LocalName::Sdt)) || dom.is(n, QName::w(LocalName::SdtContent))
        {
            let kids: Vec<NodeId> = dom.semantic_children(n).collect();
            stack.extend(kids.into_iter().rev());
        }
    }
    out
}

// ---- 锚定 ---------------------------------------------------------------------------------------

/// 一段里所有锚定绘图共享的上下文（TS `extractTextboxes` 的 `fragMetas` / `pinAll` /
/// `anchorUnionSpansColumn`）。
///
/// 三件事只有在**整段**的尺度上才定得下来，落到单个框上就晚了：
/// 1. `posOffset` 归一化——相对页面的偏移要减掉页边距换到栏原点空间，否则页边距算两遍；
/// 2. `pinAll`——「锚定绘图全都相对页面定位」的首页封面段整体保留页面原始坐标；
/// 3. 并集是否铺满栏——并排两个半宽框时缝里排不下字，Word 把文字挤到上下。
pub(super) struct AnchorCtx<'a> {
    sect: Option<&'a SectionGeom>,
    /// 这一段锚了不止一个绘图（TS `multiDrawing`）。
    multi: bool,
    /// 整段按页面原始坐标钉住（TS `pinAll`）。
    pin_all: bool,
    /// 锚定绘图的并集铺满了正文栏（TS `anchorUnionSpansColumn`）。
    union_spans: bool,
}

/// 一个绘图归一化之后的锚定量。
struct Norm {
    x: Option<i64>,
    y: Option<i64>,
    /// 钉页时的页面绝对坐标（TS `meta.pageXEmu` / `pageYEmu`）。
    page: Option<(i64, i64)>,
}

impl<'a> AnchorCtx<'a> {
    /// `first_page` 是 TS 的 `opts.firstPage`（见 [`super::blocks::Ctx::first_page`]）。
    pub(super) fn new(
        drawings: &[&DrawingDisplay],
        sect: Option<&'a SectionGeom>,
        first_page: bool,
    ) -> AnchorCtx<'a> {
        let multi = drawings.len() > 1;
        let mut anchors = drawings.iter().filter_map(|d| d.anchor.as_ref()).peekable();
        let pin_all = first_page
            && sect.is_some()
            && anchors.peek().is_some()
            && anchors.all(|a| pinnable(a, multi));
        let mut me = AnchorCtx { sect, multi, pin_all, union_spans: false };
        me.union_spans = me.union_spans_column(drawings);
        me
    }

    /// 归一化后的锚定量。
    fn norm(&self, a: &AnchorGeom, extent: Option<Extent>) -> Norm {
        let tw = |v: i64| v * EMU_PER_TWIP_I;
        let (mut x, mut y) = (a.h.offset_emu, a.v.offset_emu);
        // 钉页的绘图保留页面原始坐标，整段跳过归一化
        if self.pin_all
            && pinnable(a, self.multi)
            && let Some(s) = self.sect
        {
            let (mar_l, mar_t) = (tw(s.margin_left), tw(s.margin_top));
            let aligned = resolve_page_pos(a, extent, s);
            let page_x = match aligned {
                Some(p) => p.x_emu + mar_l,
                None => {
                    let base =
                        if a.h.relative_from.as_deref() == Some("margin") { mar_l } else { 0 };
                    base + x.unwrap_or(0)
                }
            };
            let page_y = match aligned.and_then(|p| p.y_emu) {
                Some(v) => v + mar_t,
                None => y.unwrap_or(0),
            };
            return Norm { x, y, page: Some((page_x, page_y)) };
        }
        if let Some(s) = self.sect {
            // 相对页面的 posOffset 从纸边量起；框却从栏 / 段落原点画，直接用会把页边距算两遍
            if a.h.relative_from.as_deref() == Some("page") && a.h.align.is_none() {
                x = x.map(|v| v - tw(s.margin_left));
            }
            if a.v.relative_from.as_deref() == Some("page") && a.v.align.is_none() {
                y = y.map(|v| v - tw(s.margin_top));
            }
        }
        Norm { x, y, page: None }
    }

    /// 锚定绘图的并集是否铺满正文栏：按归一化后的横向区间扫，最宽的空隙不够排字就算铺满。
    fn union_spans_column(&self, drawings: &[&DrawingDisplay]) -> bool {
        let Some(s) = self.sect else { return false };
        let col_w = (s.page_width - s.margin_left - s.margin_right) * EMU_PER_TWIP_I;
        let mut iv: Vec<(i64, i64)> = drawings
            .iter()
            .filter_map(|d| {
                let x = self.norm(d.anchor.as_ref()?, d.extent).x?;
                let w = d.extent.map_or(0, |e| e.cx);
                (w > 0).then_some((x, x + w))
            })
            .collect();
        if iv.is_empty() {
            return false;
        }
        iv.sort_unstable();
        let (mut gap, mut cursor) = (0, 0);
        for (a, b) in iv {
            gap = gap.max(a - cursor);
            cursor = cursor.max(b);
        }
        gap.max(col_w - cursor) < MIN_WRAP_SLIVER_EMU
    }
}

/// TS `pinnable`：纵向相对页面、横向相对页面 / 页边距、且不参与绕排的锚定绘图。
fn pinnable(a: &AnchorGeom, multi: bool) -> bool {
    let no_wrap = matches!(a.wrap, Wrap::None) || a.behind_doc;
    (no_wrap || multi)
        && a.v.relative_from.as_deref() == Some("page")
        && matches!(a.h.relative_from.as_deref(), Some("page") | Some("margin"))
}

/// `wp:anchor` 相对页面 / 页边距对齐时解出来的位置（TS `resolveAnchorPagePos`）。
#[derive(Debug, Clone, Copy)]
pub(super) struct PagePos {
    /// 相对正文左边界的横向位置（EMU）。
    pub x_emu: i64,
    /// 同上，纵向；只有纵向也按页面 / 页边距对齐时才有。
    pub y_emu: Option<i64>,
    /// 整个框落在正文栏之外（Word 简历式侧边栏）。
    pub outside_column: bool,
}

const EMU_PER_TWIP_I: i64 = 635;

/// 解析页面 / 页边距对齐的锚定位置。栏数为 1 时「相对栏」就等于「相对页边距」。
pub(super) fn resolve_page_pos(
    a: &AnchorGeom,
    extent: Option<Extent>,
    sect: &SectionGeom,
) -> Option<PagePos> {
    let rel_h = match a.h.relative_from.as_deref() {
        Some("column") if sect.columns <= 1 && a.h.pct.is_none() => "margin",
        Some(other) => other,
        None => return None,
    };
    if rel_h != "page" && rel_h != "margin" {
        return None;
    }
    if a.h.pct.is_none() && a.h.align.is_none() {
        return None;
    }
    let tw = |v: i64| v * EMU_PER_TWIP_I;
    let (page_w, page_h) = (tw(sect.page_width), tw(sect.page_height));
    let (mar_l, mar_r) = (tw(sect.margin_left), tw(sect.margin_right));
    let (mar_t, mar_b) = (tw(sect.margin_top), tw(sect.margin_bottom));
    let w = extent.map_or(0, |e| e.cx);
    let ref_w = if rel_h == "page" { page_w } else { page_w - mar_l - mar_r };
    let rel_x = axis_pos(ref_w, w, a.h.pct, a.h.align.as_deref(), "center", &["right", "outside"]);
    let page_x = if rel_h == "page" { rel_x } else { mar_l + rel_x };
    let mut pos = PagePos {
        x_emu: page_x - mar_l,
        y_emu: None,
        outside_column: page_x + w <= mar_l || page_x >= page_w - mar_r,
    };
    let rel_v = a.v.relative_from.as_deref();
    if matches!(rel_v, Some("page") | Some("margin")) && (a.v.pct.is_some() || a.v.align.is_some())
    {
        let h = extent.map_or(0, |e| e.cy);
        let ref_h = if rel_v == Some("page") { page_h } else { page_h - mar_t - mar_b };
        let rel_y =
            axis_pos(ref_h, h, a.v.pct, a.v.align.as_deref(), "center", &["bottom", "outside"]);
        pos.y_emu = Some(if rel_v == Some("page") { rel_y } else { mar_t + rel_y } - mar_t);
    }
    Some(pos)
}

/// 一个轴上的位置：百分比优先，其次居中 / 靠远端对齐，都没有就是 0。
fn axis_pos(
    reference: i64,
    size: i64,
    pct: Option<i64>,
    align: Option<&str>,
    center: &str,
    far: &[&str],
) -> i64 {
    if let Some(p) = pct {
        return (reference as f64 * p as f64 / 100_000.0).round() as i64;
    }
    match align {
        Some(v) if v == center => ((reference - size) as f64 / 2.0).round() as i64,
        Some(v) if far.contains(&v) => reference - size,
        _ => 0,
    }
}

/// TS `applyAnchor`：把锚定几何投到框上。
pub(super) fn apply_anchor(
    actx: &AnchorCtx<'_>,
    a: &AnchorGeom,
    extent: Option<Extent>,
    grouped: bool,
    o: &mut Map<String, Value>,
) {
    let n = actx.norm(a, extent);
    set_if!(o, "behind" => a.behind_doc);
    set_some!(o, "z" => a.relative_height.map(|h| h - Z_ORDER_BASE).filter(|&z| z != 0));
    let add = |o: &mut Map<String, Value>, key: &str, v: i64| {
        let cur = o.get(key).and_then(Value::as_i64).unwrap_or(0);
        set(o, key, cur + v);
    };
    // 首页封面：整段按页面原始坐标钉在纸上。从段落原点渲染会把锚点上方的内容（题图行、
    // 空引导段）再加一遍，整版就歪了。
    if let Some((x, y)) = n.page {
        add(o, "offsetXEmu", x);
        add(o, "offsetYEmu", y);
        set(o, "floating", true);
        set(o, "pagePinned", true);
        return;
    }
    // 相对页面 / 页边距的横向位置在 Word 里是页面上的绝对位置：栏平移不能把框带偏。
    let rel_x_absolute = matches!(a.h.relative_from.as_deref(), Some("page") | Some("margin"));
    let page_pos = actx.sect.and_then(|s| resolve_page_pos(a, extent, s));
    // 整个框落在正文栏之外（简历侧边栏）：直接按解出来的位置绝对摆放，不管绕排方式。
    if let Some(pp) = page_pos.filter(|p| p.outside_column) {
        add(o, "offsetXEmu", pp.x_emu);
        add(o, "offsetYEmu", pp.y_emu.or(n.y).unwrap_or(0));
        set(o, "floating", true);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
        return;
    }
    let top_bottom = matches!(a.wrap, Wrap::TopAndBottom);
    let no_wrap = matches!(a.wrap, Wrap::None) || a.behind_doc;
    if let Some(x) = n.x {
        add(o, "offsetXEmu", x);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
    } else if let Some(pp) = page_pos.filter(|_| top_bottom || no_wrap || actx.multi) {
        // 页边距对齐的浮动绘图（照片行）：按页边距框解算；随文的方框绕排保持老的排布
        add(o, "offsetXEmu", pp.x_emu);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
    }
    if let Some(y) = n.y {
        add(o, "offsetYEmu", y);
        // 相对页面 / 页边距的纵向 posOffset 在 Word 里是锚点所在页上的绝对位置
        if matches!(a.v.relative_from.as_deref(), Some("page") | Some("margin"))
            && a.v.align.is_none()
        {
            set(o, "pageRelV", true);
        }
    }
    // 方框绕排但（几乎）铺满整栏：旁边剩的缝里排不下字，Word 让框浮起来、文字排上下，
    // 跟 `wrapTopAndBottom` 一样占一条横带，而不是把框塞回文字流。
    // 一段里有多个绘图时，看的是这些框的**并集**（并排的两个半宽签名框）。
    let (mut own_spans, mut square_spans) = (false, false);
    if !no_wrap
        && !top_bottom
        && !grouped
        && let Some(s) = actx.sect.filter(|s| s.columns <= 1)
    {
        let col_w = (s.page_width - s.margin_left - s.margin_right) * EMU_PER_TWIP_I;
        let w_emu = o
            .get("widthPx")
            .and_then(Value::as_i64)
            .map(|w| w * EMU_PER_PX as i64)
            .or_else(|| extent.map(|e| e.cx));
        let x_emu = o.get("offsetXEmu").and_then(Value::as_i64).unwrap_or(0);
        own_spans = w_emu.is_some_and(|w| {
            w > 0 && x_emu < MIN_WRAP_SLIVER_EMU && col_w - x_emu - w < MIN_WRAP_SLIVER_EMU
        });
        square_spans = own_spans || (actx.multi && actx.union_spans);
    }
    // `wrapTopAndBottom`：正文被整条竖带排除在外。框浮在自己的偏移上，锚定段落把流高
    // 一直留到框底。
    if (top_bottom || square_spans)
        && matches!(a.v.relative_from.as_deref(), Some("paragraph") | Some("line"))
    {
        // `wp:extent cy` 覆盖整个绘图：只有非组内形状拿它兜底（组内每个孩子都会声称组高）
        let h = o
            .get("heightPx")
            .and_then(Value::as_i64)
            .or_else(|| (!grouped).then(|| extent.map(|e| px_round(e.cy)))?);
        let top = o.get("offsetYEmu").and_then(Value::as_i64).map_or(0, px_round_i);
        if let Some(h) = h
            && top + h > 0
        {
            set(o, "bandTopPx", top);
            set(o, "bandBottomPx", top + h);
            // 只有**自己**就铺满栏的框可以溢出页底（Word 把它留在锚点那一页）；并集带
            // （并排的框）整块推到下一页
            if own_spans && !top_bottom {
                set(o, "bandOverflow", true);
            }
        }
        set(o, "floating", true);
    }
    if no_wrap || actx.multi {
        set(o, "floating", true);
    }
}

fn px_round_i(emu: i64) -> i64 {
    px_round(emu)
}

/// 框里所有段落的文字（`previewText` 用）。
pub(super) fn box_texts(paras: &[Value]) -> Vec<String> {
    paras
        .iter()
        .map(|p| {
            p.get("runs")
                .and_then(Value::as_array)
                .map(|rs| {
                    rs.iter()
                        .filter_map(|r| r.get("text").and_then(Value::as_str))
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// 段落是不是「里面有 run」——TS 用它决定空框留不留。
pub(super) fn any_runs(paras: &[Value]) -> bool {
    paras.iter().any(|p| p.get("runs").and_then(Value::as_array).is_some_and(|r| !r.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VML 的 `path` 是直边子集：认得的照转，认不得的整条不给。
    #[test]
    fn compat_03_vml_path_straight_edges_only() {
        // `nf` / `x` / `e` 分别是「不填充」「闭合」「结束」，坐标按 coordsize 归一化
        assert_eq!(
            vml_path_to_norm_d("m,l21600,,21600,21600,,21600nfxe", 21600, 21600),
            Some("M 0 0 L 1 0 L 1 1 L 0 1 Z".to_string())
        );
        // `t` / `r` 是相对移动
        assert_eq!(
            vml_path_to_norm_d("m0,0r100,0r0,100x", 200, 200),
            Some("M 0 0 L 0.5 0 L 0.5 0.5 Z".to_string())
        );
        // 曲线命令：整条不给，绝不退化成一个实心包围盒
        assert_eq!(vml_path_to_norm_d("m0,0c10,10,20,20,30,30x", 100, 100), None);
        // 坐标不成对 / coordsize 非法
        assert_eq!(vml_path_to_norm_d("m0,0,5x", 100, 100), None);
        assert_eq!(vml_path_to_norm_d("m0,0l1,1x", 0, 100), None);
        // 只有一条命令：画不出东西
        assert_eq!(vml_path_to_norm_d("m0,0", 100, 100), None);
    }
}
