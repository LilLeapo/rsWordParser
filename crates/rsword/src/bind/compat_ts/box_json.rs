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

use crate::model::drawing::{
    Anchor, AnchorGeom, BodyPr, Extent, FillKind, ShapeDisplay, StyleRef, Wrap,
};
use crate::model::units::{EMU_PER_PX, emu_to_px};
use crate::model::vml::VmlShape;
use crate::model::{Block, Display};
use crate::resolve::drawingml::{DrawingColor, Rgb, average, color_in, hex, parse_color};
use crate::xml::{LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;

/// Word 的 `relativeHeight` 基数。
const Z_ORDER_BASE: i64 = 251_658_240;
/// 零高连线保留的抓取带（px）。
const LINE_GRAB_PX: i64 = 12;

fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

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
    if let Some(i) = txbx_index.filter(|_| !nested) {
        set(&mut o, "txbxIndex", i as i64);
    }
    if nested {
        set(&mut o, "readOnly", true);
    }
    if !nested && let Some(id) = &s.cnv_id {
        set(&mut o, "shapeId", id.clone());
    }
    fill_and_line(ctx, s, group_fill, &mut o);
    if let Some(prst) = s.prst.as_deref().filter(|p| *p != "rect") {
        set(&mut o, "prst", prst);
    }
    if let Some(rot) = s.rot_60k.filter(|&r| r != 0) {
        set(&mut o, "rotDeg", (rot as f64 / 60_000.0).round() as i64);
    }
    if let Some(cx) = s.ext.map(|e| e.cx).filter(|&cx| cx > 0) {
        set(&mut o, "widthPx", px_round(cx));
    }
    // Word 会裁掉溢出的文字，除非形状自适应；带上固定高度，稀疏的高框才不会撑爆版面。
    let auto_fit = s.body.is_some_and(|b| b.auto_fit);
    if !auto_fit && let Some(cy) = s.ext.map(|e| e.cy).filter(|&cy| cy > 0) {
        set(&mut o, "heightPx", px_round(cy));
        set(&mut o, "minHeightPx", px_round(cy));
    }
    if let Some(b) = s.body {
        body_pr(b, &mut o);
    }
    o
}

fn fill_and_line(
    ctx: &Ctx<'_>,
    s: &ShapeDisplay,
    group_fill: Option<&str>,
    o: &mut Map<String, Value>,
) {
    let no_fill = s.fill.as_ref().is_some_and(|f| f.kind == FillKind::None);
    if !no_fill {
        let fill = s.fill.as_ref().and_then(|f| match f.kind {
            FillKind::Solid => color_hex(ctx, f.node),
            FillKind::Gradient => grad_hex(ctx, f.node),
            FillKind::Pattern => patt_hex(ctx, f.node),
            // `a:grpFill` 继承所在组的填充
            FillKind::Group => group_fill.map(str::to_string),
            _ => None,
        });
        if let Some(c) = fill {
            set(o, "fill", c);
        }
        if let Some(f) = s.fill.as_ref().filter(|f| f.kind == FillKind::Blip)
            && let Some(m) = f.blip.as_deref().and_then(|r| ctx.media.get(r))
        {
            set(o, "fillImageDataUrl", m.url.clone());
            if f.tile {
                set(o, "fillTile", true);
            }
        }
    }
    if let Some(ln) = &s.line
        && !ln.no_fill
    {
        if let Some(c) = ln.fill.and_then(|f| color_hex(ctx, f)) {
            set(o, "borderColor", c);
        }
        if let Some(w) = ln.width_emu.filter(|&w| w > 0) {
            set(o, "borderWidthPx", px2(w));
        }
        if let Some(dash) = &ln.dash {
            set(o, "borderDash", if dash.contains("dot") { "dotted" } else { "dashed" });
        }
    }
    // `wps:style`：spPr 没写颜色的图库形状从主题引用取
    let ln_no_fill = s.line.as_ref().is_some_and(|l| l.no_fill);
    if !o.contains_key("fill")
        && !o.contains_key("fillImageDataUrl")
        && !no_fill
        && let Some(c) = style_ref_hex(ctx, s.fill_ref.as_ref(), true)
    {
        set(o, "fill", c);
    }
    if !o.contains_key("borderColor")
        && !ln_no_fill
        && let Some(c) = style_ref_hex(ctx, s.line_ref.as_ref(), true)
    {
        set(o, "borderColor", c);
    }
    // `a:fontRef` 是图库形状文字颜色的出处：缺省蓝形状引用 lt1，所以 Word 里不写 run 颜色
    // 也显示白字。run 自己写了 `w:color` 时以 run 为准。
    if let Some(c) = style_ref_hex(ctx, s.font_ref.as_ref(), false) {
        set(o, "textColor", c);
    }
}

fn body_pr(b: BodyPr, o: &mut Map<String, Value>) {
    for (key, v) in [
        ("insetLeftPx", b.l_ins),
        ("insetTopPx", b.t_ins),
        ("insetRightPx", b.r_ins),
        ("insetBottomPx", b.b_ins),
    ] {
        if let Some(v) = v.filter(|&v| v >= 0) {
            set(o, key, px2(v));
        }
    }
    match b.anchor {
        Some(Anchor::Bottom) => set(o, "vAlign", "bottom"),
        Some(Anchor::Center) => set(o, "vAlign", "center"),
        _ => {}
    }
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
    if let Some(w) = dim("width", s, 0).filter(|&w| w > 0) {
        set(&mut o, "widthPx", px_round(w));
    }
    if let Some(h) = dim("height", s, 1).filter(|&h| h > 0) {
        set(&mut o, "heightPx", px_round(h));
        set(&mut o, "minHeightPx", px_round(h));
    }
    if s.filled != Some(false)
        && let Some(c) = &s.fill_color
    {
        set(&mut o, "fill", c.clone());
    }
    if s.stroked != Some(false)
        && let Some(c) = &s.stroke_color
    {
        set(&mut o, "borderColor", c.clone());
    }
    // 组外的 `position:absolute` 才是真的绝对定位
    if group.is_none() && s.is_absolute() {
        set(&mut o, "floating", true);
        for (key, out) in [("margin-left", "offsetXEmu"), ("margin-top", "offsetYEmu")] {
            if let Some(v) = s.style_len(key).and_then(|l| l.to_emu()) {
                set(&mut o, out, v.round() as i64);
            }
        }
    }
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
    if let Some((x, y)) = s.off {
        set(o, "offsetXEmu", (ctm.tx + x as f64 * ctm.sx).round() as i64);
        set(o, "offsetYEmu", (ctm.ty + y as f64 * ctm.sy).round() as i64);
    }
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

// ---- 框里的段落 ---------------------------------------------------------------------------------

/// 框里内容流的块 → `paras[]`（TS `txbxContentParas`）。返回 `(段落, 是否只读)`。
pub(super) fn paras_json(ctx: &Ctx<'_>, content: &[Block]) -> (Vec<Value>, bool) {
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

/// TS `applyAnchor`：把锚定几何投到框上。`multi_drawing` 是「这一段锚了不止一个绘图」。
///
/// 用到节几何的分支（`resolveAnchorPagePos`、跨栏宽框的 band 推断）留到 M5，见模块文档。
pub(super) fn apply_anchor(
    a: &AnchorGeom,
    extent: Option<Extent>,
    multi_drawing: bool,
    grouped: bool,
    o: &mut Map<String, Value>,
) {
    if a.behind_doc {
        set(o, "behind", true);
    }
    if let Some(z) = a.relative_height.map(|h| h - Z_ORDER_BASE).filter(|&z| z != 0) {
        set(o, "z", z);
    }
    let add = |o: &mut Map<String, Value>, key: &str, v: i64| {
        let cur = o.get(key).and_then(Value::as_i64).unwrap_or(0);
        set(o, key, cur + v);
    };
    if let Some(x) = a.h.offset_emu {
        add(o, "offsetXEmu", x);
    }
    if let Some(y) = a.v.offset_emu {
        add(o, "offsetYEmu", y);
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
