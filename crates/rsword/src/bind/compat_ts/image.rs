//! 图片段落的 TS 投影（`COMPAT-03`，TS `imageMeta` / `picBorderOf`；`spec/15` 任务 4.4）。
//!
//! 模型里存的是文档事实（EMU、1/60000 度、`relativeFrom` 原文）；这里换算成 TS 的 `image*` 字段。
//! 排版味道最重的两处——绕排落在哪一侧、z 序归一化——按 `spec/15` 的分层决策放在这一层，不进模型。

use serde_json::{Map, Value};

use crate::model::drawing::{AnchorGeom, DrawingDisplay, Wrap};
use crate::model::units::{EMU_PER_PT, Length, emu_to_px};
use crate::model::{Segment, SegmentKind, VmlDisplay};
use crate::resolve::drawingml::{ColorBase, color_in, hex};
use crate::xml::{Dom, LocalName, NodeId, QName};

use super::blocks::Ctx;

/// Word 的 `relativeHeight` 基数：`relativeHeight - 251658240` 才是 z 序。
const Z_ORDER_BASE: i64 = 251_658_240;

/// `wrapText="bothSides"` 时判「贴右侧」的分界：正文可用宽度的一半，4680 缇。
const HALF_BODY_TWIPS: i64 = 4680;
const EMU_PER_TWIP: i64 = 635;

fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

/// TS `imageMeta(xml)`：把一个绘图的事实投影成 `image*` 字段，写进 `out`。
///
/// `para` 是宿主段落（`w:p`），用来取 `w:jc`、`w:ind` 与图前的引导文字。
pub(super) fn image_meta(
    ctx: &Ctx<'_>,
    para: Option<NodeId>,
    d: &DrawingDisplay,
    out: &mut Map<String, Value>,
) {
    let dom = ctx.dom;
    if let Some(p) = para {
        leading(dom, p, out);
        paragraph_indent(dom, p, out);
        if let Some(a) = jc_align(dom, p) {
            set(out, "imageAlign", a);
        }
    }
    if let Some(ext) = d.extent {
        if ext.cx > 0 {
            set(out, "imageWidthPx", emu_to_px(ext.cx as f64).round() as i64);
        }
        if ext.cy > 0 {
            set(out, "imageHeightPx", emu_to_px(ext.cy as f64).round() as i64);
        }
    }
    if let Some(pic) = d.picture() {
        if let Some(rot) = pic.rot_60k.filter(|&r| r != 0) {
            let deg = (rot as f64 / 60_000.0).round() as i64;
            set(out, "imageRotDeg", deg.rem_euclid(360));
        }
        if pic.flip_h {
            set(out, "imageFlipH", true);
        }
        if pic.flip_v {
            set(out, "imageFlipV", true);
        }
        if let Some(b) = border(ctx, pic.border.as_ref()) {
            set(out, "imageBorder", Value::Object(b));
        }
        if let Some(c) = pic.crop.filter(|c| !c.is_zero()) {
            set(out, "imageCrop", Value::Object(rect(c)));
        }
        if let Some(c) = pic.fill_rect.filter(|c| !c.is_zero()) {
            set(out, "imageFillRect", Value::Object(rect(c)));
        }
    }
    if let Some(a) = &d.anchor {
        anchor_meta(a, d, out);
    }
}

/// TS `buildRun` 的图片 run：`{dataUrl, xml, widthPx, heightPx, border}`。
///
/// 只对能解析出媒体的 DrawingML 图片给；形状（`wps:wsp`）与解析失败的图不给——TS 那两条路
/// 分别走文本框投影与 `brokenImage`，不会走到「带图的文本段落」。
pub(super) fn run_image(ctx: &Ctx<'_>, seg: &Segment) -> Option<Map<String, Value>> {
    match seg.kind {
        SegmentKind::Drawing { .. } => drawing_run_image(ctx, seg),
        // `w:pict` / `w:object` 的预览图也是一个原子 run（`smartart-ole__016`：OLE 预览与
        // 随后的图片各占一个 run）。
        SegmentKind::Pict | SegmentKind::Object => vml_run_image(ctx, seg),
        _ => None,
    }
}

fn vml_run_image(ctx: &Ctx<'_>, seg: &Segment) -> Option<Map<String, Value>> {
    let v = seg.display.as_ref()?.as_vml()?;
    let m = ctx.media.get(v.image()?.imagedata.as_deref()?)?;
    let mut o = Map::new();
    set(&mut o, "dataUrl", m.url.clone());
    set(&mut o, "xml", ctx.node_xml(seg.node).to_string());
    let (w, h) = vml_px(v);
    if let Some(w) = w {
        set(&mut o, "widthPx", w);
    }
    if let Some(h) = h {
        set(&mut o, "heightPx", h);
    }
    Some(o)
}

fn drawing_run_image(ctx: &Ctx<'_>, seg: &Segment) -> Option<Map<String, Value>> {
    let d = seg.display.as_ref()?.as_drawing()?;
    let pic = d.picture()?;
    let m = ctx.media.pick(pic.embed.as_deref(), pic.link.as_deref())?;
    let mut o = Map::new();
    set(&mut o, "dataUrl", m.url.clone());
    set(&mut o, "xml", ctx.node_xml(seg.node).to_string());
    if let Some(ext) = d.extent {
        if ext.cx > 0 {
            set(&mut o, "widthPx", emu_to_px(ext.cx as f64).round() as i64);
        }
        if ext.cy > 0 {
            set(&mut o, "heightPx", emu_to_px(ext.cy as f64).round() as i64);
        }
    }
    if let Some(b) = border(ctx, pic.border.as_ref()) {
        set(&mut o, "border", Value::Object(b));
    }
    if let Some(a) = &d.anchor {
        run_anchor_meta(a, d, &mut o);
    }
    Some(o)
}

/// 锚定图片 run 的定位信息（`docs/01` §6.5）。键名没有 `image` 前缀，是块级 `imageMeta` 的子集：
/// 块级还带 z 序与位置预设，run 级只要绕排与偏移。
fn run_anchor_meta(a: &AnchorGeom, d: &DrawingDisplay, out: &mut Map<String, Value>) {
    for (key, v) in [
        ("wrapDistTopEmu", a.dist.top),
        ("wrapDistBottomEmu", a.dist.bottom),
        ("wrapDistLeftEmu", a.dist.left),
        ("wrapDistRightEmu", a.dist.right),
    ] {
        if let Some(v) = v {
            set(out, key, v);
        }
    }
    set(out, "wrap", wrap_kind(a, d));
    if let Some(x) = a.h.offset_emu {
        set(out, "offsetXEmu", x);
    }
    if let Some(y) = a.v.offset_emu {
        set(out, "offsetYEmu", y);
    }
    if !a.allow_overlap {
        set(out, "noOverlap", true);
    }
    // 相对「行」居中：Word 把图片压在行中线上（LibreOffice tdf#162551）。
    if a.v.relative_from.as_deref() == Some("line") && a.v.align.as_deref() == Some("center") {
        set(out, "lineCenterV", true);
    }
}

/// VML 细横线（`v:rect o:hr="t"`）的显示字段（`docs/01` §6.2.6；`spec/15` 4.5）。
///
/// `width:0` 在 VML HR 里表示「铺满可用宽度」，所以不设 `ruleWidthPx`——这一点和 DrawingML
/// 细线不同，那边的宽度来自 `wp:extent cx`。
pub(super) fn vml_rule(v: &VmlDisplay, out: &mut Map<String, Value>) {
    let Some(r) = v.rule() else { return };
    set(out, "decorative", true);
    if let Some(c) = &r.fill_color {
        // 细横线这条路 TS 会转大写（VML 框那条不会）
        set(out, "ruleColorHex", c.to_ascii_uppercase());
    }
    if let Some(h) = r.style_len("height").and_then(Length::to_emu).filter(|&h| h > 0.0) {
        set(out, "ruleThicknessPx", emu_to_px(h).round().max(1.0) as i64);
    }
}

/// `w:object` 的嵌入对象（TS `oleDisplay`；`spec/15` 4.7）。
///
/// 预览图按**声明尺寸**画：`v:shape` 的 `style`（磅）优先，退到 `w:object` 的
/// `dxaOrig`/`dyaOrig`（缇）。不给尺寸的话，metafile 预览的原始像素会撑满整个正文宽度。
pub(super) fn ole_display(
    ctx: &Ctx<'_>,
    para: NodeId,
    v: &VmlDisplay,
    out: &mut Map<String, Value>,
) {
    let ole = v.ole.as_ref();
    if let Some(id) = ole.and_then(|o| o.prog_id.clone()) {
        set(out, "oleProgId", id);
    }
    if let Some(m) = v.image().and_then(|s| s.imagedata.as_deref()).and_then(|r| ctx.media.get(r)) {
        set(out, "imageDataUrl", m.url.clone());
    }
    let (w, h) = vml_px(v);
    if let Some(w) = w {
        set(out, "imageWidthPx", w);
    }
    if let Some(h) = h {
        set(out, "imageHeightPx", h);
    }
    if let Some(a) = jc_align(ctx.dom, para) {
        set(out, "imageAlign", a);
    }
}

/// VML 预览图的声明尺寸：`v:shape` 的 `style`（磅）优先，退到 `w:object` 的
/// `dxaOrig`/`dyaOrig`（缇，1 px = 15 缇）。
fn vml_px(v: &VmlDisplay) -> (Option<i64>, Option<i64>) {
    // 预览图所在的形状；没有 `v:imagedata` 时退到第一个形状（空 pict 也带 `style` 尺寸）。
    let shape = v.shapes.iter().find(|s| s.imagedata.is_some()).or_else(|| v.shapes.first());
    let ole = v.ole.as_ref();
    let px = |key: &str, twips: Option<i64>| -> Option<i64> {
        let from_style = shape
            .and_then(|s| s.style_len(key))
            .and_then(Length::to_emu)
            .filter(|&v| v > 0.0)
            .map(emu_to_px);
        let from_twips = twips.filter(|&t| t > 0).map(|t| t as f64 / 15.0);
        from_style.or(from_twips).map(|v| v.round() as i64)
    };
    (px("width", ole.and_then(|o| o.dxa_orig)), px("height", ole.and_then(|o| o.dya_orig)))
}

fn anchor_meta(a: &AnchorGeom, d: &DrawingDisplay, out: &mut Map<String, Value>) {
    for (key, v) in [
        ("imageWrapDistTopEmu", a.dist.top),
        ("imageWrapDistBottomEmu", a.dist.bottom),
        ("imageWrapDistLeftEmu", a.dist.left),
        ("imageWrapDistRightEmu", a.dist.right),
    ] {
        if let Some(v) = v {
            set(out, key, v);
        }
    }
    if !a.allow_overlap {
        set(out, "imageNoOverlap", true);
    }
    if a.locked {
        set(out, "imageAnchorLocked", true);
    }
    if let Some(z) = a.relative_height.map(|h| h - Z_ORDER_BASE).filter(|&z| z != 0) {
        set(out, "imageZOrder", z);
    }
    set(out, "imageWrap", wrap_kind(a, d));
    if let Some(x) = a.h.offset_emu {
        set(out, "imageOffsetXEmu", x);
    }
    if let Some(y) = a.v.offset_emu {
        set(out, "imageOffsetYEmu", y);
    }
    // margin 对齐的一对 `wp:align` 是 Word 的「位置库」预设，原样带回去才能往返。
    let h_from = a.h.relative_from.as_deref();
    let v_from = a.v.relative_from.as_deref();
    let h_align = a.h.align.as_deref().filter(|s| matches!(*s, "left" | "center" | "right"));
    let v_align = a.v.align.as_deref().filter(|s| matches!(*s, "top" | "center" | "bottom"));
    match (h_from, v_from, h_align, v_align) {
        (Some("margin"), Some("margin"), Some(h), Some(v)) => {
            set(out, "imagePosH", h);
            set(out, "imagePosV", v);
        }
        // 横向对齐、纵向按偏移的混合定位：至少保住横向预设
        (Some("margin" | "page"), _, Some(h), None) => set(out, "imagePosH", h),
        _ => {}
    }
}

/// TS 的九种 `ImageWrap`。绕排元素比 `behindDoc` 优先：Word 会把 behindDoc + wrapTight 的对象
/// 画在文字下面**并且**继续绕排。
fn wrap_kind(a: &AnchorGeom, d: &DrawingDisplay) -> &'static str {
    let kind = match &a.wrap {
        Wrap::TopAndBottom => return "topBottom",
        Wrap::Square { text } => ("square-left", "square-right", text.as_deref()),
        Wrap::Tight { text } => ("tight-left", "tight-right", text.as_deref()),
        Wrap::Through { text } => ("through-left", "through-right", text.as_deref()),
        Wrap::None | Wrap::Unspecified => {
            return if a.behind_doc { "behind" } else { "front" };
        }
    };
    // 居中且相对栏定位：渲染器没有「两侧绕排」，用居中的 topBottom 位近似
    if a.h.relative_from.as_deref() == Some("column") && a.h.align.as_deref() == Some("center") {
        return "topBottom";
    }
    let (left, right, wrap_text) = kind;
    // `wrapText` 说的是**文字**走哪一侧，对象浮在另一侧；`bothSides` 时看对象中心过没过正文中线
    // （只看左边缘会把「左边缘在中线左、但整体压在右半页」的宽图判错）。
    let center_past_middle = match (a.h.offset_emu, d.extent) {
        (Some(off), Some(ext)) => off + ext.cx / 2 > HALF_BODY_TWIPS * EMU_PER_TWIP,
        (Some(off), None) => off > HALF_BODY_TWIPS * EMU_PER_TWIP,
        (None, _) => false,
    };
    let to_right = a.h.align.as_deref() == Some("right")
        || wrap_text == Some("left")
        || (wrap_text != Some("right") && center_past_middle);
    if to_right { right } else { left }
}

fn border(
    ctx: &Ctx<'_>,
    ln: Option<&crate::model::drawing::LineDisplay>,
) -> Option<Map<String, Value>> {
    let ln = ln?;
    if ln.no_fill {
        return None;
    }
    // 只认字面 `a:srgbClr`：主题色描边由渲染器按主题自己上色，TS 也不投影（`docs/01` §8.3）。
    let c = color_in(ctx.dom, ln.fill?)?;
    let ColorBase::Srgb(rgb) = c.base else { return None };
    let mut o = Map::new();
    set(&mut o, "color", hex([f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])]));
    // DrawingML 缺省描边宽 0.75pt
    let pt = match ln.width_emu {
        Some(w) if w > 0 => w as f64 / EMU_PER_PT,
        _ => 0.75,
    };
    set(&mut o, "widthPt", pt);
    Some(o)
}

fn rect(c: crate::model::drawing::RectFrac) -> Map<String, Value> {
    let mut o = Map::new();
    for (k, v) in [("l", c.l), ("t", c.t), ("r", c.r), ("b", c.b)] {
        set(&mut o, k, v as f64 / 100_000.0);
    }
    o
}

/// 段落的 `w:jc`：`center` → center，`right` / `end` → right。
pub(super) fn jc_align(dom: &Dom, para: NodeId) -> Option<&'static str> {
    let ppr = dom.semantic_children(para).find(|&n| dom.is(n, QName::w(LocalName::PPr)))?;
    let jc = dom.semantic_children(ppr).find(|&n| dom.is(n, QName::w(LocalName::Jc)))?;
    match dom.attr_value(jc, QName::w(LocalName::Val))?.trim() {
        "center" => Some("center"),
        "right" | "end" => Some("right"),
        _ => None,
    }
}

fn paragraph_indent(dom: &Dom, para: NodeId, out: &mut Map<String, Value>) {
    let Some(ppr) = dom.semantic_children(para).find(|&n| dom.is(n, QName::w(LocalName::PPr)))
    else {
        return;
    };
    let Some(ind) = dom.semantic_children(ppr).find(|&n| dom.is(n, QName::w(LocalName::Ind)))
    else {
        return;
    };
    let twips = |l: LocalName| -> Option<i64> {
        dom.attr_value(ind, QName::w(l))?.trim().parse::<i64>().ok()
    };
    if let Some(v) = twips(LocalName::Left) {
        set(out, "imageParagraphIndentLeft", v);
    }
    if let Some(v) = twips(LocalName::Right) {
        set(out, "imageParagraphIndentRight", v);
    }
    // `firstLine` 缺省时用 `-hanging`
    if let Some(v) = twips(LocalName::FirstLine).or_else(|| twips(LocalName::Hanging).map(|h| -h)) {
        set(out, "imageParagraphIndentFirstLine", v);
    }
}

/// 图之前的内容：文字、字体、以及用来占位的空格 run。
///
/// TS 是按第一个 `<w:drawing`/`<w:pict` 的偏移切 XML 再正则扫，所以「跨在图前面的同一个 run」
/// 的文字算引导文字，但那个 run 本身不算完整的空格 run。这里按文档序遍历复现这条边界。
fn leading(dom: &Dom, para: NodeId, out: &mut Map<String, Value>) {
    let mut text = String::new();
    let mut font: Option<String> = None;
    let mut explicit_px = 0.0f64;
    let mut implicit = 0usize;

    for child in dom.semantic_children(para) {
        let mut run_text = String::new();
        let hit = scan(dom, child, &mut run_text, &mut font);
        text.push_str(&run_text);
        if hit {
            break;
        }
        // 空格 run 只算「整个 run 都在图前面」的那些（TS 切 XML 后正则匹配完整的 `<w:r>…</w:r>`）。
        if dom.is(child, QName::w(LocalName::R))
            && !run_text.is_empty()
            && run_text.bytes().all(|b| b == b' ')
        {
            match run_size_half_points(dom, child) {
                // Word 的 CJK 单字节空格是半个字宽：半磅数 / 3 就是 CSS px。
                Some(sz) => explicit_px += run_text.len() as f64 * f64::from(sz) / 3.0,
                None => implicit += run_text.len(),
            }
        }
    }

    if !text.is_empty() {
        set(out, "imageLeadingText", text);
    }
    if let Some(f) = font {
        set(out, "imageLeadingFont", f);
    }
    if explicit_px > 0.0 {
        set(out, "imageLeadingExplicitSpaceWidthPx", explicit_px);
    }
    if implicit > 0 {
        set(out, "imageLeadingImplicitSpaceCount", implicit as i64);
    }
}

/// 前序扫一棵子树，收文字与「最后一个 `w:rFonts`」，遇到图就停并返回 `true`。
///
/// 字体要连 `w:pPr/w:rPr/w:rFonts`（段落标记的字体）一起看：TS 是在切出来的 XML 上取最后一个
/// `<w:rFonts>`，段落属性也在里面（语料 `image-wrap__014`）。
fn scan(dom: &Dom, node: NodeId, text: &mut String, font: &mut Option<String>) -> bool {
    for d in dom.semantic_descendants(node) {
        if is_graphic(dom, d) {
            return true;
        }
        if dom.is(d, QName::w(LocalName::RFonts))
            && let Some(f) = rfonts_face(dom, d)
        {
            *font = Some(f);
        }
        if dom.is(d, QName::w(LocalName::T)) {
            for c in dom.semantic_children(d) {
                if let Some(t) = dom.text(c) {
                    text.push_str(&t);
                }
            }
        }
    }
    false
}

fn is_graphic(dom: &Dom, n: NodeId) -> bool {
    dom.is(n, QName::w(LocalName::Drawing)) || dom.is(n, QName::w(LocalName::Pict))
}

/// `w:rFonts`：`eastAsia` 优先，其次 `ascii`。
fn rfonts_face(dom: &Dom, f: NodeId) -> Option<String> {
    dom.attr_value(f, QName::w(LocalName::EastAsia))
        .or_else(|| dom.attr_value(f, QName::w(LocalName::Ascii)))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn rpr_of(dom: &Dom, run: NodeId) -> Option<NodeId> {
    dom.semantic_children(run).find(|&n| dom.is(n, QName::w(LocalName::RPr)))
}

fn run_size_half_points(dom: &Dom, run: NodeId) -> Option<u32> {
    let rpr = rpr_of(dom, run)?;
    let sz = dom.semantic_children(rpr).find(|&n| dom.is(n, QName::w(LocalName::Sz)))?;
    dom.attr_value(sz, QName::w(LocalName::Val))?.trim().parse().ok()
}

/// TS `normalizeImageZOrders`：LibreOffice 写 `relativeHeight` 为 1、2、… 而不是 Word 的基数加偏移，
/// 于是 z 序会大得离谱。任何一块的 `|imageZOrder| > 10000` 就把所有块按 z 序稳定重排成 0..n，
/// 0 的那块删掉字段，全部标 `imageZOrderNormalized`。
pub(super) fn normalize_z_orders(blocks: &mut [Value]) {
    // 图片与浮动形状共用 Word 的 z 空间，要一起排：照片压在背景形状上的次序才不会乱。
    // 位置用 (块下标, 框下标)；框下标 `None` 表示块自己的 `imageZOrder`。
    let mut anchored: Vec<((usize, Option<usize>), i64)> = Vec::new();
    for (bi, b) in blocks.iter().enumerate() {
        if let Some(z) = b.get("imageZOrder").and_then(Value::as_i64) {
            anchored.push(((bi, None), z));
        }
        if let Some(boxes) = b.get("textboxes").and_then(Value::as_array) {
            for (ti, t) in boxes.iter().enumerate() {
                if let Some(z) = t.get("z").and_then(Value::as_i64) {
                    anchored.push(((bi, Some(ti)), z));
                }
            }
        }
    }
    if !anchored.iter().any(|(_, z)| z.abs() > 10_000) {
        return;
    }
    // 稳定排序：同 z 时按文档序
    anchored.sort_by_key(|&(pos, z)| (z, pos));
    for (rank, ((bi, ti), _)) in anchored.into_iter().enumerate() {
        let rank = rank as i64;
        match ti {
            None => {
                let Some(o) = blocks[bi].as_object_mut() else { continue };
                if rank == 0 {
                    o.remove("imageZOrder");
                } else {
                    o.insert("imageZOrder".into(), Value::from(rank));
                }
                // 原 XML 里还是那个离谱的值，标记出来供保存时统一改写
                o.insert("imageZOrderNormalized".into(), Value::Bool(true));
            }
            // 框是纯显示的，XML 里的 relativeHeight 不动
            Some(ti) => {
                if let Some(t) = blocks[bi]
                    .get_mut("textboxes")
                    .and_then(Value::as_array_mut)
                    .and_then(|a| a.get_mut(ti))
                    .and_then(Value::as_object_mut)
                {
                    t.insert("z".into(), Value::from(rank));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::drawing::{Dist, Extent, Position};
    use crate::xml::NodeId;

    fn anchor(wrap: Wrap, align: Option<&str>, from: Option<&str>, off: Option<i64>) -> AnchorGeom {
        AnchorGeom {
            node: NodeId(0),
            behind_doc: false,
            allow_overlap: true,
            locked: false,
            layout_in_cell: true,
            simple_pos: false,
            relative_height: None,
            dist: Dist::default(),
            h: Position {
                relative_from: from.map(str::to_string),
                align: align.map(str::to_string),
                offset_emu: off,
                pct: None,
            },
            v: Position::default(),
            wrap,
        }
    }

    fn drawing(cx: i64) -> DrawingDisplay {
        DrawingDisplay {
            node: NodeId(0),
            kind: crate::model::DrawingKind::Picture,
            anchor: None,
            extent: Some(Extent { cx, cy: 0 }),
            doc_pr: Default::default(),
            pictures: Vec::new(),
            shapes: Vec::new(),
        }
    }

    fn sq(text: Option<&str>) -> Wrap {
        Wrap::Square { text: text.map(str::to_string) }
    }

    #[test]
    fn compat_03_wrap_kind_picks_the_side_the_object_floats_on() {
        let d = drawing(914_400);
        // wrapText 说文字走左边 → 对象在右边
        assert_eq!(wrap_kind(&anchor(sq(Some("left")), None, None, None), &d), "square-right");
        assert_eq!(wrap_kind(&anchor(sq(Some("right")), None, None, None), &d), "square-left");
        // 明确右对齐压过 wrapText
        assert_eq!(
            wrap_kind(&anchor(sq(Some("right")), Some("right"), None, None), &d),
            "square-right"
        );
        // bothSides：看对象**中心**过没过正文中线，而不是左边缘
        let near_middle = 4680 * 635 - 914_400 / 2 + 1;
        assert_eq!(
            wrap_kind(&anchor(sq(Some("bothSides")), None, None, Some(near_middle)), &d),
            "square-right"
        );
        assert_eq!(
            wrap_kind(&anchor(sq(Some("bothSides")), None, None, Some(0)), &d),
            "square-left"
        );
        // 相对栏居中：渲染器没有两侧绕排，退成居中的 topBottom
        assert_eq!(
            wrap_kind(&anchor(sq(None), Some("center"), Some("column"), None), &d),
            "topBottom"
        );
        // 绕排元素比 behindDoc 优先；没有绕排元素时才看 behindDoc
        let mut behind = anchor(Wrap::None, None, None, None);
        behind.behind_doc = true;
        assert_eq!(wrap_kind(&behind, &d), "behind");
        let mut behind_but_wrapped = anchor(sq(Some("right")), None, None, None);
        behind_but_wrapped.behind_doc = true;
        assert_eq!(wrap_kind(&behind_but_wrapped, &d), "square-left");
        assert_eq!(wrap_kind(&anchor(Wrap::Unspecified, None, None, None), &d), "front");
        assert_eq!(wrap_kind(&anchor(Wrap::TopAndBottom, None, None, None), &d), "topBottom");
    }

    #[test]
    fn compat_03_z_order_normalization_only_fires_on_libreoffice_values() {
        let mk = |zs: &[i64]| -> Vec<Value> {
            zs.iter().map(|z| serde_json::json!({ "imageZOrder": z })).collect()
        };
        // Word 写的是基数 + 小偏移，不动
        let mut word = mk(&[3, 1, 2]);
        normalize_z_orders(&mut word);
        assert_eq!(word[0]["imageZOrder"], 3);
        assert!(word[0].get("imageZOrderNormalized").is_none());
        // LibreOffice 从 1 开始写 relativeHeight，减掉基数后是巨大的负数 → 按 z 序重排成 0..n，
        // 排到 0 的那块删掉字段（文档序稳定）。
        let mut lo = mk(&[-251_658_238, 5, -251_658_239]);
        normalize_z_orders(&mut lo);
        assert!(lo[2].get("imageZOrder").is_none(), "最小的那块排到 0，删掉字段");
        assert_eq!(lo[0]["imageZOrder"], 1);
        assert_eq!(lo[1]["imageZOrder"], 2);
        for b in &lo {
            assert_eq!(b["imageZOrderNormalized"], Value::Bool(true));
        }
    }
}
