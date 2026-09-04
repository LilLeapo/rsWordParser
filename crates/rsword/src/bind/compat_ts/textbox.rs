//! 文本框与形状块的分类（TS `extractTextboxes` + `buildBlock` 的绘图分支，`docs/01` §6.2.7、§8.2；
//! `spec/15` 任务 4.6）。
//!
//! 模型不把文本框当保护块——`spec/06` 明确写了「`TextBox` 不再是保护种类：文本框是 `Inline::Run`
//! 内的 `Drawing` 段，段落可编辑」。所以「这一段是 `Text box` 还是 `Drawing object`」整个是
//! `compat_ts` 的投影决定，模型侧一个字节都不动。
//!
//! 这一步只做**分类**与依赖分类的字段（`label` / `type` / `previewText` / `strayRuns` /
//! `decorative` / `rule*` / `imageMeta`）。`textboxes[]` 的完整载荷（几何、填充、内边距、band）
//! 在下一步补——先输出半截数组只会把一处差异拆成十几处。

use serde_json::{Map, Value};

use crate::model::drawing::{DrawingDisplay, FillKind, ShapeDisplay, Wrap};
use crate::model::units::emu_to_px;
use crate::model::{Display, Inline, SegmentKind, TextBlock, VmlDisplay};
use crate::resolve::drawingml::{color_in, hex};
use crate::xml::{LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;
use super::image;

/// 细横线的高度上限：`wp:extent cy` 在 (0, 130000] EMU（约 10 px）之内的无字形状是装饰线。
const THIN_RULE_EMU: i64 = 130_000;

/// 只描边、没有文字也要显示的连线预设（TS `LINE_PRSTS`）。
const LINE_PRSTS: &[&str] = &[
    "line",
    "straightConnector1",
    "bentConnector2",
    "bentConnector3",
    "bentConnector4",
    "curvedConnector2",
    "curvedConnector3",
    "curvedConnector4",
];

fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

/// 宿主段落：节点、模型块、框外文字。参数打包，免得每个分支都拖一串。
#[derive(Clone, Copy)]
struct Para<'a> {
    node: NodeId,
    block: &'a TextBlock,
    stray: &'a str,
}

/// 段落里提取出的一个「框」。这一步只用到文字；几何与样式在下一步。
pub(super) struct BoxInfo {
    /// 框里每个段落的文字。
    pub texts: Vec<String>,
}

impl BoxInfo {
    fn has_text(&self) -> bool {
        self.texts.iter().any(|t| !t.trim().is_empty())
    }
}

/// 段落里的绘图与 VML 显示模型，按文档序。
fn graphics(tb: &TextBlock) -> (Vec<&DrawingDisplay>, Vec<&VmlDisplay>) {
    let (mut drawings, mut vmls) = (Vec::new(), Vec::new());
    for i in &tb.inlines {
        let Inline::Run(r) = i else { continue };
        for s in &r.segments {
            match (&s.kind, s.display.as_ref()) {
                (SegmentKind::Drawing { .. }, Some(Display::Drawing(d))) => drawings.push(&**d),
                (SegmentKind::Pict | SegmentKind::Object, Some(Display::Vml(v))) => vmls.push(&**v),
                _ => {}
            }
        }
    }
    (drawings, vmls)
}

/// 绘图分支的块投影。返回 `None` 表示这一段按普通文本段落走。
pub(super) fn drawing_block(
    ctx: &Ctx<'_>,
    p: NodeId,
    tb: &TextBlock,
    o: Map<String, Value>,
) -> Option<Map<String, Value>> {
    // 嵌入对象有自己的分支（4.7），这里不掺和。
    if !tb.facts.objects.is_empty() {
        return None;
    }
    let (drawings, vmls) = graphics(tb);
    if drawings.is_empty() && vmls.is_empty() {
        return None;
    }
    let stray = stray_text(ctx, p);
    // TS 的决策树里 `w:pict` 分支在 `w:drawing` 之前，两者规则不同，所以先分流。
    if !vmls.is_empty() {
        return vml_block(ctx, Para { node: p, block: tb, stray: &stray }, &vmls, &drawings, o);
    }
    let has_wsp = drawings.iter().any(|d| d.shapes.iter().any(|s| !s.is_group));
    let anchored = drawings.iter().filter(|d| d.anchor.is_some()).count();
    // 没有 wps 形状的段落，图片路径（4.4）已经处理过了；只有「一段里好几张分别锚定的图」
    // 要走照片框这条路。
    if !has_wsp && anchored <= 1 {
        return None;
    }

    let boxes = boxes_of(ctx, &drawings, &vmls);
    // 装饰形状 + 真文字的段落按普通段落解析，文字才留得住可编辑（形状留在不再生成的 run 里，
    // 未编辑时字节不变）。有文字的框、以及提取出框的 wps 段落是例外。
    if !stray.trim().is_empty()
        && !boxes.iter().any(BoxInfo::has_text)
        && !(has_wsp && !boxes.is_empty())
    {
        return None;
    }

    let mut o = o;
    if !boxes.is_empty() {
        text_box_block(
            ctx,
            Para { node: p, block: tb, stray: &stray },
            &drawings,
            &boxes,
            false,
            &mut o,
        );
        return Some(o);
    }

    set(&mut o, "type", "passthrough");
    set(&mut o, "label", "Drawing object");
    if invisible_empty_shapes(ctx, &drawings) {
        set(&mut o, "invisibleMarker", true);
        return Some(o);
    }
    let decorative = drawings.iter().any(|d| is_thin_rule(d));
    set(&mut o, "decorative", decorative);
    if decorative {
        rule_display(ctx, &drawings, &mut o);
    }
    Some(o)
}

/// `w:pict` 分支（`docs/01` §6.2.6）。无框时返回 `None`，段落按带图的普通文本段落走。
fn vml_block(
    ctx: &Ctx<'_>,
    para: Para<'_>,
    vmls: &[&VmlDisplay],
    drawings: &[&DrawingDisplay],
    o: Map<String, Value>,
) -> Option<Map<String, Value>> {
    let Para { node: p, stray, .. } = para;
    // 框外还有文字、而且 pict 里有**带 `r:id` 的** `v:imagedata`：这是「图 + 文字」的段落，
    // 走文本框会把图丢掉。WPS 会在普通文本框形状上盖一个没有 `r:id` 的空 `v:imagedata`，
    // 所以这里必须看有没有 `r:id`（`wordart-vml__011`）。
    let has_picture = vmls.iter().any(|v| v.shapes.iter().any(|s| s.imagedata.is_some()));
    if !stray.trim().is_empty() && has_picture {
        return None;
    }
    let boxes = boxes_of(ctx, drawings, vmls);
    let mut o = o;
    if boxes.is_empty() {
        // 既没有框、又不是图 / 细横线 / 隐藏形状（那三种由模型的 R15–R17 分掉）：
        // TS 的 `w:pict` 分支最后落到嵌入对象，尺寸取 `v:shape` 的 style
        // （`decorated-paragraphs__004` 是一个只有 `<v:shape style="…"/>` 的空 pict）。
        if has_picture {
            return None;
        }
        set(&mut o, "type", "passthrough");
        set(&mut o, "label", "Embedded object");
        set(&mut o, "previewText", ctx.plain_text(p));
        if let Some(v) = vmls.first() {
            image::ole_display(ctx, p, v, &mut o);
        }
        return Some(o);
    }
    text_box_block(ctx, para, drawings, &boxes, true, &mut o);
    Some(o)
}

/// `Text box` 块的公共字段。
///
/// 两条路的载荷不一样（`docs/01` §6.2.6 vs §6.2.7）：`w:pict` 那条只带宿主段落的 `w:jc`，
/// 预览文字也只有框里的文字；`w:drawing` 那条把框外的文字并进预览，并且带整套 `imageMeta`。
fn text_box_block(
    ctx: &Ctx<'_>,
    para: Para<'_>,
    drawings: &[&DrawingDisplay],
    boxes: &[BoxInfo],
    vml: bool,
    o: &mut Map<String, Value>,
) {
    let Para { node: p, block: tb, stray } = para;
    set(o, "type", "passthrough");
    set(o, "label", "Text box");
    let mut preview: Vec<String> = Vec::new();
    let stray = stray.trim();
    if !vml && !stray.is_empty() {
        preview.push(stray.to_string());
    }
    preview.extend(boxes.iter().flat_map(|b| b.texts.iter().cloned()));
    set(o, "previewText", preview.join("\n"));
    if vml {
        if let Some(a) = image::jc_align(ctx.dom, p) {
            set(o, "imageAlign", a);
        }
        return;
    }
    // 段落自己带的文字（画布旁的说明、超链接）：作为只读的 `strayRuns` 留住，别让它凭空消失。
    if !stray.is_empty() {
        let runs = super::blocks::runs_json(ctx, tb);
        if !runs.is_empty() {
            set(o, "strayRuns", Value::Array(runs.into_iter().map(Value::Object).collect()));
            if let Some(id) = &tb.style_id {
                set(o, "strayStyleId", id.clone());
            }
        }
    }
    for d in drawings {
        image::image_meta(ctx, Some(p), d, o);
    }
}

/// TS `isThinRule`：无字锚定形状的 `wp:extent cy` 不超过约 10 px 就是装饰线。
/// Word 把纯水平线写成 `cy="0"`，所以 `cy == 0` 也算——只要 `cx` 有值。
fn is_thin_rule(d: &DrawingDisplay) -> bool {
    let Some(ext) = d.extent else { return false };
    ext.cy <= THIN_RULE_EMU && (ext.cy > 0 || ext.cx > 0)
}

/// TS `ruleDisplayOf`：装饰线的颜色、粗细与宽度。
fn rule_display(ctx: &Ctx<'_>, drawings: &[&DrawingDisplay], out: &mut Map<String, Value>) {
    let Some(d) = drawings.iter().find(|d| is_thin_rule(d)) else { return };
    if let Some(ln) = d.shapes.iter().find_map(|s| s.line.as_ref()) {
        if let Some(c) = ln.fill.and_then(|f| color_in(ctx.dom, f)) {
            // 只认字面 `a:srgbClr`（同 `picBorderOf`）
            if let crate::resolve::drawingml::ColorBase::Srgb(rgb) = c.base {
                set(
                    out,
                    "ruleColorHex",
                    hex([f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])]),
                );
            }
        }
        if let Some(w) = ln.width_emu.filter(|&w| w > 0) {
            set(out, "ruleThicknessPx", emu_to_px(w as f64).round().max(1.0) as i64);
        }
    }
    if let Some(cx) = d.extent.map(|e| e.cx).filter(|&cx| cx > 0) {
        set(out, "ruleWidthPx", emu_to_px(cx as f64).round() as i64);
    }
}

/// TS `isInvisibleEmptyShape`：每个 `wps:wsp` 都显式 noFill + 描边 noFill、没有图也没有文字，
/// Word 什么都不画，块也就什么都不显示。
fn invisible_empty_shapes(ctx: &Ctx<'_>, drawings: &[&DrawingDisplay]) -> bool {
    let shapes: Vec<&ShapeDisplay> =
        drawings.iter().flat_map(|d| d.shapes.iter()).filter(|s| !s.is_group).collect();
    if shapes.is_empty() {
        return false;
    }
    if drawings.iter().any(|d| d.picture.is_some()) {
        return false;
    }
    shapes.iter().all(|s| {
        s.fill.as_ref().is_some_and(|f| f.kind == FillKind::None)
            && s.line.as_ref().is_some_and(|l| l.no_fill)
            // 带阴影等效果的形状 Word 还是画得出来（`bugfix-regressions__020`）
            && !s.has_effects
            && s.txbx.is_none_or(|t| ctx.plain_text(t).trim().is_empty())
    })
}

/// 段落里能提取出的框（TS `extractTextboxes`，`shapes` 与 `pictures` 都开）。
fn boxes_of(ctx: &Ctx<'_>, drawings: &[&DrawingDisplay], vmls: &[&VmlDisplay]) -> Vec<BoxInfo> {
    let mut out = Vec::new();
    // `wrapSquare` 这道闸门把「转换器产出的装饰线」留在细横线那条路上：只有既方框绕排、
    // 又用了连线预设的段落，才把连线当成画出来的框（TS `hasLineShapes`）。
    let wrap_square = drawings
        .iter()
        .any(|d| d.anchor.as_ref().is_some_and(|a| matches!(a.wrap, Wrap::Square { .. })));
    let has_line_shapes = wrap_square && drawings.iter().any(|d| d.shapes.iter().any(is_line_prst));
    for d in drawings {
        let mut had_shape = false;
        for s in &d.shapes {
            if s.is_group {
                continue;
            }
            had_shape = true;
            if let Some(b) = wps_box(ctx, s, has_line_shapes) {
                out.push(b);
            }
        }
        // `pictures` 选项：片段里没有形状但有图片 → 只读图片框
        if !had_shape && d.picture.is_some() {
            out.push(BoxInfo { texts: Vec::new() });
        }
    }
    for v in vmls {
        for s in &v.shapes {
            if s.has_textbox {
                let texts = s.txbx.map(|t| ctx.para_texts(t)).unwrap_or_default();
                out.push(BoxInfo { texts });
            } else if let Some(t) = &s.textpath {
                // WordArt：`v:textpath/@string` 就是它的一行文字
                out.push(BoxInfo { texts: vec![t.clone()] });
            }
        }
    }
    out
}

fn is_line_prst(s: &ShapeDisplay) -> bool {
    s.prst.as_deref().is_some_and(|p| LINE_PRSTS.contains(&p))
}

/// TS `buildWpsBox` 的「留不留这个形状」判定。`opts.shapes` 在绘图分支恒为真。
fn wps_box(ctx: &Ctx<'_>, s: &ShapeDisplay, has_line_shapes: bool) -> Option<BoxInfo> {
    let texts = s.txbx.map(|t| ctx.para_texts(t)).unwrap_or_default();
    let has_text = texts.iter().any(|t| !t.is_empty());
    // `paint` 要查主题与媒体，所以放在最后算：前面的形状规则先把不用看颜色的情况筛掉。
    keeps_box(s, has_line_shapes, s.txbx.is_some(), has_text, || paint(ctx, s))
        .then_some(BoxInfo { texts: if has_text { texts } else { Vec::new() } })
}

/// TS `buildWpsBox` 的「留不留这个形状」判定，抽成纯函数好单测。`painted` 惰性求值。
fn keeps_box(
    s: &ShapeDisplay,
    has_line_shapes: bool,
    has_content: bool,
    has_text: bool,
    painted: impl FnOnce() -> bool,
) -> bool {
    if !has_content {
        if is_line_prst(s) {
            if has_line_shapes {
                return true;
            }
            // 画廊里的连接线：真有竖直高度、翻转、或者带箭头才算画出来的连线；
            // 近乎水平的细线走装饰线那条路。
            let tall = s.ext.is_some_and(|e| e.cy > THIN_RULE_EMU);
            let flipped = s.flip_h || s.flip_v;
            let arrowed = s.line.as_ref().is_some_and(|l| l.arrowed());
            return tall || flipped || arrowed;
        }
        if s.prst.is_none() && !s.cust_geom {
            return false;
        }
        if s.prst.as_deref() == Some("rect") && !s.ext.is_some_and(|e| e.cy > THIN_RULE_EMU) {
            return false;
        }
        // 无字预设形状：几何得有可见的墨才留
        return painted();
    }
    // 有文字就留；没文字的框有可见的墨也留（整页白框要占住它的尺寸）
    has_text || painted()
}

/// 形状是否有可见的填充 / 描边 / 图片（TS 的 `box.fill || box.borderColor || box.fillImageDataUrl`）。
fn paint(ctx: &Ctx<'_>, s: &ShapeDisplay) -> bool {
    let fill = match &s.fill {
        None => s.fill_ref.as_ref().is_some_and(|r| r.idx.unwrap_or(0) > 0),
        Some(f) => match f.kind {
            FillKind::None => false,
            FillKind::Blip => f.blip.as_deref().is_some_and(|r| ctx.media.get(r).is_some()),
            // 实色 / 渐变 / 图案 / 组继承：只要定得出颜色就算有墨
            _ => color_in(ctx.dom, f.node).is_some() || f.kind == FillKind::Group,
        },
    };
    let border = match &s.line {
        Some(l) if l.no_fill => false,
        Some(l) => {
            l.fill.and_then(|f| color_in(ctx.dom, f)).is_some()
                || s.line_ref.as_ref().is_some_and(|r| r.idx.unwrap_or(0) > 0)
        }
        None => s.line_ref.as_ref().is_some_and(|r| r.idx.unwrap_or(0) > 0),
    };
    fill || border
}

/// 段落里不在任何文本框内的可见文字（TS `plainText(stripTextboxes(detect))`）。
fn stray_text(ctx: &Ctx<'_>, p: NodeId) -> String {
    let dom = ctx.dom;
    let mut out = String::new();
    let mut stack = vec![p];
    let mut scratch = Vec::new();
    while let Some(n) = stack.pop() {
        if dom.is(n, QName::new(NsId::W, LocalName::TxbxContent)) {
            continue;
        }
        if dom.is(n, QName::new(NsId::W, LocalName::T)) {
            for c in dom.semantic_children(n) {
                if let Some(t) = dom.text(c) {
                    out.push_str(&t);
                }
            }
            continue;
        }
        scratch.clear();
        scratch.extend(dom.semantic_children(n));
        stack.extend(scratch.iter().rev().copied());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::drawing::{Extent, LineDisplay};
    use crate::xml::NodeId;

    fn shape(prst: Option<&str>, cy: Option<i64>) -> ShapeDisplay {
        ShapeDisplay {
            node: NodeId(0),
            is_group: false,
            cnv_id: None,
            prst: prst.map(str::to_string),
            cust_geom: false,
            ext: cy.map(|cy| Extent { cx: 5_230_495, cy }),
            off: None,
            ch_off: None,
            ch_ext: None,
            rot_60k: None,
            flip_h: false,
            flip_v: false,
            fill: None,
            line: None,
            fill_ref: None,
            line_ref: None,
            font_ref: None,
            has_effects: false,
            body: None,
            txbx: None,
            group: None,
        }
    }

    fn line(head: Option<&str>, tail: Option<&str>) -> LineDisplay {
        LineDisplay {
            node: NodeId(0),
            width_emu: None,
            no_fill: false,
            fill: None,
            dash: None,
            head_end: head.map(str::to_string),
            tail_end: tail.map(str::to_string),
        }
    }

    /// 无字形状留不留下来，是 `Text box` 与 `Drawing object` 的分水岭。
    #[test]
    fn compat_03_textless_shape_becomes_a_box_only_when_it_draws_something() {
        let yes = || true;
        let no = || false;

        // 细横线：`prst="line"`、没有 wrapSquare、不高不翻转不带箭头 → 不是框，走装饰线
        // （语料 bugfix-regressions__022）
        let thin = shape(Some("line"), Some(20_955));
        assert!(!keeps_box(&thin, false, false, false, yes));
        // 同一条线，段落里有 wrapSquare → TS 把它当画出来的连线
        assert!(keeps_box(&thin, true, false, false, no));
        // 带箭头的连接线是框（语料 shape-display__009 的 straightConnector1 + tailEnd）
        let mut arrow = shape(Some("straightConnector1"), Some(20_955));
        arrow.line = Some(line(None, Some("triangle")));
        assert!(keeps_box(&arrow, false, false, false, no));
        // `type="none"` 不算箭头
        arrow.line = Some(line(None, Some("none")));
        assert!(!keeps_box(&arrow, false, false, false, no));
        // 够高的连线也算
        assert!(keeps_box(&shape(Some("line"), Some(200_000)), false, false, false, no));
        // 翻转的连线也算
        let mut flipped = shape(Some("line"), Some(20_955));
        flipped.flip_v = true;
        assert!(keeps_box(&flipped, false, false, false, no));

        // 既没有 prstGeom 也没有 custGeom → 不是框（语料 decorated-paragraphs__003）
        assert!(!keeps_box(&shape(None, Some(9_525)), false, false, false, yes));
        // 矮矩形留给装饰线；够高的矩形是正经形状（前提是画得出东西）
        assert!(!keeps_box(&shape(Some("rect"), Some(9_525)), false, false, false, yes));
        assert!(keeps_box(&shape(Some("rect"), Some(900_000)), false, false, false, yes));
        assert!(!keeps_box(&shape(Some("rect"), Some(900_000)), false, false, false, no));
        // 其他预设形状：有墨才留
        assert!(keeps_box(&shape(Some("star5"), None), false, false, false, yes));
        assert!(!keeps_box(&shape(Some("star5"), None), false, false, false, no));
    }

    #[test]
    fn compat_03_box_with_content_keeps_text_or_visible_ink() {
        let yes = || true;
        let no = || false;
        let s = shape(Some("rect"), Some(900_000));
        // 有文字的框一定留
        assert!(keeps_box(&s, false, true, true, no));
        // 空框：有墨才留（整页白框要占住尺寸），否则丢掉
        assert!(keeps_box(&s, false, true, false, yes));
        assert!(!keeps_box(&s, false, true, false, no));
    }
}
