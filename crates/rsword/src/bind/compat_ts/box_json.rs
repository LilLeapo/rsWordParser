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
    Anchor, AnchorGeom, BodyPr, Extent, FillKind, ShapeDisplay, StyleRef, Wrap,
};
use crate::model::section::SectionGeom;
use crate::model::units::{EMU_PER_PX, emu_to_px, parse_length, parse_style};
use crate::model::vml::{VmlShape, vml_color};
use crate::model::{Block, Display};
use crate::resolve::drawingml::{DrawingColor, Rgb, average, color_in, hex, parse_color};
use crate::xml::{LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;
use super::json::{set, set_if, set_some};

/// Word 的 `relativeHeight` 基数。
const Z_ORDER_BASE: i64 = 251_658_240;
/// 零高连线保留的抓取带（px）。
const LINE_GRAB_PX: i64 = 12;

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
fn color_hex(ctx: &Ctx<'_>, node: NodeId) -> Option<String> {
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
fn path_data(geom: &CustomGeom, ext: Option<Extent>) -> Option<Map<String, Value>> {
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
fn norm(v: f64) -> String {
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

/// 一个 VML 形状 → 框的 JSON。几何全在 `style` 里，长度多半是磅。
pub(super) fn vml_box_json(
    ctx: &Ctx<'_>,
    s: &VmlShape,
    group: Option<&VmlShape>,
    txbx_index: Option<usize>,
) -> Map<String, Value> {
    let _ = ctx;
    let mut o = Map::new();
    if let Some(i) = txbx_index {
        set(&mut o, "txbxIndex", i as i64);
    }
    // 组内形状的 style 用的是组坐标，要按组的 `coordsize` 换算成绝对长度
    let scale = group.and_then(|g| {
        let (cw, ch) = g.coordsize?;
        let w = g.style_len("width")?.to_emu()?;
        let h = g.style_len("height")?.to_emu()?;
        (cw > 0 && ch > 0).then(|| (w / cw as f64, h / ch as f64))
    });
    let dim = |key: &str, s: &VmlShape, axis: usize| -> Option<i64> {
        let l = s.style_len(key)?;
        match (l.to_emu(), scale) {
            (Some(emu), _) => Some(emu.round() as i64),
            (None, Some((sx, sy))) => {
                Some((l.value * if axis == 0 { sx } else { sy }).round() as i64)
            }
            _ => None,
        }
    };
    let h = dim("height", s, 1).filter(|&h| h > 0).map(px_round);
    // 组外的 `position:absolute` 才是真的绝对定位
    let absolute = group.is_none() && s.is_absolute();
    let margin = |key: &str| {
        absolute
            .then(|| s.style_len(key).and_then(|l| l.to_emu()).map(|v| v.round() as i64))
            .flatten()
    };
    set_if!(&mut o, "floating" => absolute);
    set_some!(&mut o,
        "widthPx" => dim("width", s, 0).filter(|&w| w > 0).map(px_round),
        "heightPx" => h,
        "minHeightPx" => h,
        "fill" => (s.filled != Some(false)).then(|| s.fill_color.clone()).flatten(),
        "borderColor" => (s.stroked != Some(false)).then(|| s.stroke_color.clone()).flatten(),
        "offsetXEmu" => margin("margin-left"),
        "offsetYEmu" => margin("margin-top"),
    );
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
    let w_px = vml_dim_px(s, "width");
    let h_px = vml_dim_px(s, "height");
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

/// VML `style` 里的尺寸 → px。**没写单位时按磅算**（TS `vmlStyleDimPx`）。
fn vml_dim_px(s: &VmlShape, key: &str) -> Option<i64> {
    let l = s.style_len(key).filter(|l| l.value > 0.0)?;
    let px = match l.to_emu() {
        Some(emu) => emu_to_px(emu),
        // 无单位：按磅
        None => l.value / 72.0 * 96.0,
    };
    Some(px.round() as i64)
}

fn px_to_emu_round(px: f64) -> i64 {
    (px * EMU_PER_PX).round() as i64
}

// ---- 框里的段落 ---------------------------------------------------------------------------------

/// 框里内容流的块 → `paras[]`（TS `txbxContentParas`）。返回 `(段落, 是否只读)`。
pub(super) fn paras_json(ctx: &Ctx<'_>, content: &[Block]) -> (Vec<Value>, bool) {
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
                    None => continue,
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

/// 表格 → 每行一段，单元格之间空两格。
fn table_rows(ctx: &Ctx<'_>, tbl: NodeId) -> Vec<Value> {
    let dom = ctx.dom;
    let mut out = Vec::new();
    for tr in dom.semantic_descendants(tbl).filter(|&n| dom.is(n, QName::w(LocalName::Tr))) {
        let cells: Vec<String> = dom
            .semantic_children(tr)
            .filter(|&c| dom.is(c, QName::w(LocalName::Tc)))
            .map(|c| ctx.plain_text(c))
            .collect();
        let text = cells.join("  ");
        let mut run = Map::new();
        set(&mut run, "text", text);
        let mut p = Map::new();
        set(&mut p, "runs", Value::Array(vec![Value::Object(run)]));
        out.push(Value::Object(p));
    }
    out
}

// ---- 锚定 ---------------------------------------------------------------------------------------

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

/// TS `applyAnchor`：把锚定几何投到框上。`multi_drawing` 是「这一段锚了不止一个绘图」。
///
/// 用到节几何的分支（`resolveAnchorPagePos`、跨栏宽框的 band 推断）留到 M5，见模块文档。
pub(super) fn apply_anchor(
    a: &AnchorGeom,
    extent: Option<Extent>,
    sect: Option<&SectionGeom>,
    multi_drawing: bool,
    grouped: bool,
    o: &mut Map<String, Value>,
) {
    set_if!(o, "behind" => a.behind_doc);
    set_some!(o, "z" => a.relative_height.map(|h| h - Z_ORDER_BASE).filter(|&z| z != 0));
    let add = |o: &mut Map<String, Value>, key: &str, v: i64| {
        let cur = o.get(key).and_then(Value::as_i64).unwrap_or(0);
        set(o, key, cur + v);
    };
    // 相对页面 / 页边距的横向位置在 Word 里是页面上的绝对位置：栏平移不能把框带偏。
    let rel_x_absolute = matches!(a.h.relative_from.as_deref(), Some("page") | Some("margin"));
    let page_pos = sect.and_then(|s| resolve_page_pos(a, extent, s));
    // 整个框落在正文栏之外（简历侧边栏）：直接按解出来的位置绝对摆放，不管绕排方式。
    if let Some(pp) = page_pos.filter(|p| p.outside_column) {
        add(o, "offsetXEmu", pp.x_emu);
        add(o, "offsetYEmu", pp.y_emu.or(a.v.offset_emu).unwrap_or(0));
        set(o, "floating", true);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
        return;
    }
    let top_bottom0 = matches!(a.wrap, Wrap::TopAndBottom);
    let no_wrap0 = matches!(a.wrap, Wrap::None) || a.behind_doc;
    if let Some(x) = a.h.offset_emu {
        add(o, "offsetXEmu", x);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
    } else if let Some(pp) = page_pos.filter(|_| top_bottom0 || no_wrap0 || multi_drawing) {
        // 页边距对齐的浮动绘图（照片行）：按页边距框解算；随文的方框绕排保持老的排布
        add(o, "offsetXEmu", pp.x_emu);
        if rel_x_absolute {
            set(o, "pageRelX", true);
        }
    }
    if let Some(y) = a.v.offset_emu {
        add(o, "offsetYEmu", y);
        // 相对页面 / 页边距的纵向 posOffset 在 Word 里是锚点所在页上的绝对位置
        if matches!(a.v.relative_from.as_deref(), Some("page") | Some("margin"))
            && a.v.align.is_none()
        {
            set(o, "pageRelV", true);
        }
    }
    let top_bottom = matches!(a.wrap, Wrap::TopAndBottom);
    let no_wrap = matches!(a.wrap, Wrap::None) || a.behind_doc;
    // `wrapTopAndBottom` 且相对段落 / 行：给框预留一条横带，文字排在上下
    if top_bottom && matches!(a.v.relative_from.as_deref(), Some("paragraph") | Some("line")) {
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
        }
        set(o, "floating", true);
    }
    if no_wrap || multi_drawing {
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
