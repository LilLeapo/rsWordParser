//! SmartArt 与绘图画布块的投影（`COMPAT-03`，任务 6.3）：TS `extractDiagramText` / `extractDiagramDrawing` /
//! `extractLockedCanvas` 的产物——`previewText`、`diagramDisplay`、同段其他绘图的 `textboxes[]`。
//!
//! 模型在 `model/diagram.rs`（EMU 与颜色定义）；这里做 px 换算、画布的子坐标系缩放、颜色解析（`RES-05`），
//! 以及**只属于显示层**的两条启发式：`lnWPx` 缺省 1，和 LibreOffice 对齐的溢出文本分栏（画布通常按远大于
//! 摆放尺寸的坐标系作图，原字号的文字装不进缩小后的框，LO 把这些文本形状叠成错开的列）。

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::model::EMU_PER_PX;
use crate::model::drawing::{DrawingDisplay, RectFrac, Wrap};
use crate::model::{DiagramPart, DiagramShape};
use crate::model::{Display, Document, ProtectedBlock};
use crate::package::Package;
use crate::resolve::drawingml::{Rgb, average, color_in, hex};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;
use super::media::{MediaMap, MediaSet};
use super::textbox;
use crate::bind::native::json::{display_json, set, set_some};

/// 主 part 的一个 SmartArt 关系：part 模型 + 绘图 part 的 DOM（形状里的颜色节点属于它）+ 绘图 part 的
/// 媒体表（图片填充按**那个 part** 的关系解）。
pub(super) struct DiagramEntry<'a> {
    pub part: &'a DiagramPart,
    pub dom: Option<&'a Dom>,
    pub media: Option<&'a MediaMap>,
}

/// `@r:dm` 关系 id → SmartArt。`parsed_doc_of` 建一次，挂在 `Ctx::diagrams`。
pub(super) type DiagramMap<'a> = BTreeMap<String, DiagramEntry<'a>>;

pub(super) fn diagram_map<'a>(
    pkg: &'a Package,
    doc: &'a Document,
    media: &'a MediaSet,
) -> DiagramMap<'a> {
    doc.diagram_by_rel
        .iter()
        .filter_map(|(rid, id)| {
            let part = doc.diagram_parts.get(id)?;
            let drawing = part.drawing;
            Some((
                rid.clone(),
                DiagramEntry {
                    part,
                    dom: drawing.and_then(|d| pkg.part(d).dom()),
                    media: drawing.map(|d| media.part(d)),
                },
            ))
        })
        .collect()
}

pub(super) fn empty_diagrams() -> &'static DiagramMap<'static> {
    static EMPTY: std::sync::LazyLock<DiagramMap<'static>> =
        std::sync::LazyLock::new(DiagramMap::new);
    &EMPTY
}

/// 图示 / 画布形状的 px 形态（TS `DiagramShape`）。先算成数字再投影，分栏启发式要改 `y`。
#[derive(Debug, Clone, Default)]
struct PxShape {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    prst: Option<String>,
    fill: Option<String>,
    /// `(lnHex, lnWPx)`
    line: Option<(String, i64)>,
    image: Option<String>,
    fill_rect: Option<RectFrac>,
    texts: Vec<String>,
    font_pt: Option<i64>,
    text_hex: Option<String>,
    rot_deg: Option<i64>,
}

impl PxShape {
    fn json(&self) -> Value {
        Value::Object(display_json! {
            "xPx" => self.x,
            "yPx" => self.y,
            "wPx" => self.w,
            "hPx" => self.h,
            opt "prst" => self.prst.clone(),
            opt "fillHex" => self.fill.clone(),
            opt "lnHex" => self.line.as_ref().map(|(c, _)| c.clone()),
            opt "lnWPx" => self.line.as_ref().map(|(_, w)| *w),
            opt "imageDataUrl" => self.image.clone(),
            opt "fillRect" => self.fill_rect.map(|r| {
                Value::Object(display_json! {
                    "l" => frac(r.l), "t" => frac(r.t), "r" => frac(r.r), "b" => frac(r.b),
                })
            }),
            opt "texts" => (!self.texts.is_empty()).then(|| self.texts.clone()),
            opt "fontSizePt" => self.font_pt,
            opt "textColorHex" => self.text_hex.clone(),
            opt "rotDeg" => self.rot_deg,
        })
    }
}

/// 千分之一百分比 → 分数（`-10000` → `-0.1`）；整数值写成整数。
fn frac(v: i64) -> Value {
    let f = v as f64 / 100_000.0;
    if f.fract() == 0.0 { Value::from(f as i64) } else { Value::from(f) }
}

fn px(emu: i64) -> i64 {
    (emu as f64 / EMU_PER_PX).round() as i64
}

/// 颜色容器节点（在 `dom` 里）→ 无 `#` 的大写 hex（`RES-05`，按文档的配色方案解）。
fn hex_in(ctx: &Ctx<'_>, dom: &Dom, node: Option<NodeId>) -> Option<String> {
    color_in(dom, node?)?.to_rgb(ctx.resolver.palette()).map(hex)
}

/// `a:gradFill` 各停靠点的等权平均（TS `gradFillApproxHex`）。
fn grad_hex(ctx: &Ctx<'_>, dom: &Dom, grad: NodeId) -> Option<String> {
    let a = |l: LocalName| QName::new(NsId::A, l);
    let gs_lst = dom.semantic_children(grad).find(|&c| dom.is(c, a(LocalName::GsLst)))?;
    let stops: Vec<Rgb> = dom
        .semantic_children(gs_lst)
        .filter(|&c| dom.is(c, a(LocalName::Gs)))
        .filter_map(|c| color_in(dom, c))
        .filter_map(|c| c.to_rgb(ctx.resolver.palette()))
        .collect();
    average(&stops).map(hex)
}

/// 形状文字的三个字段（两条路共用）：文字非空才给字号与颜色（TS 同）。
fn text_fields(ctx: &Ctx<'_>, dom: &Dom, s: &DiagramShape, out: &mut PxShape) {
    if s.texts.is_empty() {
        return;
    }
    out.texts = s.texts.clone();
    out.font_pt = s.font_size_100pt.map(|sz| (sz as f64 / 100.0).round() as i64).filter(|&v| v > 0);
    out.text_hex = hex_in(ctx, dom, s.text_color);
}

fn rot_deg(s: &DiagramShape) -> Option<i64> {
    s.rot_60k.filter(|&r| r != 0).map(|r| (r as f64 / 60_000.0).round() as i64)
}

// ---- SmartArt ------------------------------------------------------------------------------------

/// SmartArt 段落（R13；块的 `type` / `label` 由调用方写）。
///
/// TS：`previewText` = 数据 part 的节点文字（**没有则不给**）；`diagramDisplay` = 绘图 part 的形状（没有则不给）；
/// 段落里还有别的绘图时，那些绘图（照片 / 形状）进 `textboxes[]`，图示自己锚定的话 `diagramDisplay`
/// 带 `offsetXEmu / offsetYEmu / floating`——单绘图段落不做这两件事。
pub(super) fn smart_art_block(
    ctx: &Ctx<'_>,
    p: NodeId,
    pb: &ProtectedBlock,
    o: &mut Map<String, Value>,
) {
    let Some(d) = pb.display.as_ref().and_then(Display::as_drawing) else { return };
    let entry =
        d.diagram.as_ref().and_then(|r| r.rel_id.as_deref()).and_then(|rid| ctx.diagrams.get(rid));
    set_some!(o, "previewText" => entry.and_then(|e| e.part.text.clone()));
    let mut display = entry
        .and_then(|e| Some((e.part.shapes.as_deref()?, e.dom?, e.media)))
        .and_then(|(shapes, dom, media)| diagram_display(ctx, d, dom, shapes, media));
    if !pb.siblings.is_empty() {
        if let (Some(disp), Some(a)) = (display.as_mut(), d.anchor.as_ref()) {
            set_some!(disp, "offsetXEmu" => a.h.offset_emu, "offsetYEmu" => a.v.offset_emu);
            set(disp, "floating", true);
        }
        let drawings: Vec<&DrawingDisplay> =
            std::iter::once(d).chain(pb.siblings.iter().filter_map(Display::as_drawing)).collect();
        let docx_index = o.get("docxIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
        let boxes = textbox::sibling_boxes(ctx, p, docx_index, &drawings);
        if !boxes.is_empty() {
            set(o, "textboxes", Value::Array(boxes));
        }
    }
    set_some!(o, "diagramDisplay" => display.map(Value::Object));
}

/// TS `extractDiagramDrawing`：宿主 `wp:extent` 两边都 > 0、至少留下一个形状才有 `diagramDisplay`。
fn diagram_display(
    ctx: &Ctx<'_>,
    d: &DrawingDisplay,
    dom: &Dom,
    shapes: &[DiagramShape],
    media: Option<&MediaMap>,
) -> Option<Map<String, Value>> {
    let ext = d.extent.filter(|e| e.cx > 0 && e.cy > 0)?;
    let (w, h) = (px(ext.cx), px(ext.cy));
    if w == 0 || h == 0 {
        return None;
    }
    let shapes: Vec<Value> =
        shapes.iter().filter_map(|s| dsp_shape(ctx, dom, s, media)).map(|s| s.json()).collect();
    if shapes.is_empty() {
        return None;
    }
    Some(display_json! { "widthPx" => w, "heightPx" => h, "shapes" => shapes })
}

/// 绘图 part 的一个 `dsp:sp` → px 形状。连线（`prst=line` / `*Connector*`）允许零宽或零高，其他零尺寸的丢弃。
fn dsp_shape(
    ctx: &Ctx<'_>,
    dom: &Dom,
    s: &DiagramShape,
    media: Option<&MediaMap>,
) -> Option<PxShape> {
    let mut out = PxShape {
        x: px(s.off_emu.0),
        y: px(s.off_emu.1),
        w: px(s.ext_emu.cx),
        h: px(s.ext_emu.cy),
        prst: s.prst.clone(),
        rot_deg: rot_deg(s),
        ..PxShape::default()
    };
    let is_line = s.prst.as_deref().is_some_and(|p| p == "line" || p.contains("Connector"));
    if (out.w <= 0 || out.h <= 0) && !(is_line && (out.w > 0 || out.h > 0)) {
        return None;
    }
    // 线：有颜色才算有线；宽度缺省 1 px，写了就至少 1 px
    if let Some(ln) = &s.line
        && let Some(c) = hex_in(ctx, dom, ln.color)
    {
        let w = ln.width_emu.filter(|&w| w > 0).map_or(1, |w| px(w).max(1));
        out.line = Some((c, w));
    }
    match &s.picture {
        // 图片填充按绘图 part 自己的关系解；解不出就没有填充（TS 不退回纯色）
        Some(pic) => {
            if let Some(url) = pic
                .embed
                .as_deref()
                .and_then(|rid| media.and_then(|m| m.get(rid)))
                .map(|m| m.url.clone())
            {
                out.image = Some(url);
                out.fill_rect = pic.fill_rect.filter(|r| !r.is_zero());
            }
        }
        // TS 的半解析：`srgbClr` 直取，`schemeClr` 查主题（解不出的槽位给 `9AB5E4`）。本引擎经 `RES-05`
        // 解全部颜色写法与变换，槽位解不出时同样给 `9AB5E4`（`docs/04` §8）。
        None => {
            out.fill = s.fill.map(|c| hex_in(ctx, dom, Some(c)).unwrap_or_else(|| "9AB5E4".into()));
        }
    }
    text_fields(ctx, dom, s, &mut out);
    Some(out)
}

// ---- 画布 ----------------------------------------------------------------------------------------

/// 画布段落（R14；`label: "Drawing object"` 由调用方写）。TS `extractLockedCanvas` 有结果才有
/// `diagramDisplay{canvas:true}` 与 `previewText`（各形状文字 `\n` 连接，没有文字是 `""`）。
/// 锚定的画布只给 `offsetXEmu`（LO 从锚定段落顶部画起，丢竖向偏移），`wrapNone` / `behindDoc` → `floating`。
pub(super) fn canvas_block(ctx: &Ctx<'_>, d: &DrawingDisplay, o: &mut Map<String, Value>) {
    let Some(c) = d.canvas.as_deref() else { return };
    // 显示尺寸 = 宿主 `wp:extent`；没有（畸形的 `wp:inline`）就按子坐标系原尺寸画（TS 这时放弃画布，
    // 改取第一张图，`m6-canvas__006` 登记在 `KNOWN_DIFFS.md`）
    let ext =
        d.extent.filter(|e| e.cx > 0 && e.cy > 0).or(c.ch_ext.filter(|e| e.cx > 0 && e.cy > 0));
    let Some(ext) = ext else { return };
    let (ch_x, ch_y) = c.ch_off.unwrap_or((0, 0));
    let ch_ext = c.ch_ext.unwrap_or(ext);
    let sx = if ch_ext.cx > 0 { ext.cx as f64 / ch_ext.cx as f64 } else { 1.0 };
    let sy = if ch_ext.cy > 0 { ext.cy as f64 / ch_ext.cy as f64 } else { 1.0 };
    let scaled = |v: i64, s: f64| (v as f64 * s / EMU_PER_PX).round() as i64;
    let mut shapes: Vec<PxShape> = Vec::new();
    for s in &c.shapes {
        let mut out = PxShape {
            x: scaled(s.off_emu.0 - ch_x, sx),
            y: scaled(s.off_emu.1 - ch_y, sy),
            w: scaled(s.ext_emu.cx, sx),
            h: scaled(s.ext_emu.cy, sy),
            // `rect` 是缺省几何，不记
            prst: s.prst.clone().filter(|p| p != "rect"),
            rot_deg: rot_deg(s),
            ..PxShape::default()
        };
        if out.w <= 0 || out.h <= 0 {
            continue;
        }
        match &s.picture {
            // 画布里的 `a:pic`：媒体按主 part 的关系解
            Some(pic) => {
                out.image =
                    pic.embed.as_deref().and_then(|rid| ctx.media.get(rid)).map(|m| m.url.clone());
            }
            None if !s.no_fill => {
                out.fill = hex_in(ctx, ctx.dom, s.fill)
                    .or_else(|| s.gradient.and_then(|g| grad_hex(ctx, ctx.dom, g)));
            }
            None => {}
        }
        text_fields(ctx, ctx.dom, s, &mut out);
        shapes.push(out);
    }
    if shapes.is_empty() {
        return;
    }
    stack_overflowing_columns(&mut shapes);
    let preview: Vec<&str> =
        shapes.iter().flat_map(|s| s.texts.iter().map(String::as_str)).collect();
    set(o, "previewText", preview.join("\n"));
    let mut disp = display_json! {
        "widthPx" => px(ext.cx),
        "heightPx" => px(ext.cy),
        "shapes" => shapes.iter().map(PxShape::json).collect::<Vec<Value>>(),
        "canvas" => true,
    };
    if let Some(a) = &d.anchor {
        set_some!(&mut disp, "offsetXEmu" => a.h.offset_emu);
        if matches!(a.wrap, Wrap::None) || a.behind_doc {
            set(&mut disp, "floating", true);
        }
    }
    set(o, "diagramDisplay", Value::Object(disp));
}

/// 一个文本形状按原字号排版要占几行、行距多少 px（TS `colGeom`）：每行字符数 = `w / (fontPx × 0.72)`，
/// 行距 1.2 倍字号。字符数按 UTF-16 单元数（JS `.length`）。
fn col_geom(s: &PxShape) -> (i64, f64) {
    let font_px = s.font_pt.unwrap_or(0) as f64 * 96.0 / 72.0;
    let chars_per_line = ((s.w as f64 / (font_px * 0.72)).floor() as i64).max(1);
    let chars: i64 = s.texts.iter().map(|t| t.encode_utf16().count() as i64).sum();
    let lines = (chars + chars_per_line - 1) / chars_per_line;
    (lines, font_px * 1.2)
}

/// LO 对齐的溢出文本分栏（TS `extractLockedCanvas` 的后半段）：文字按原字号要占的高度超过缩放后框高
/// 两倍的文本形状，第一个从 y=0 起，之后每个从前一个「一半行数」的位置起再减 23 px；一行只装得下
/// 一个字的列拆成逐字的形状（行高 = 行距），最后全部按 y 排序，让阅读顺序与 LO 的绘制顺序一致。
fn stack_overflowing_columns(shapes: &mut Vec<PxShape>) {
    let overflowing: Vec<usize> = shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.texts.is_empty() && s.font_pt.is_some())
        .filter(|(_, s)| {
            let (lines, pitch) = col_geom(s);
            lines as f64 * pitch > 2.0 * s.h as f64
        })
        .map(|(i, _)| i)
        .collect();
    if overflowing.is_empty() {
        return;
    }
    shapes[overflowing[0]].y = 0;
    for w in overflowing.windows(2) {
        let (prev, cur) = (w[0], w[1]);
        let (lines, pitch) = col_geom(&shapes[prev]);
        shapes[cur].y =
            (shapes[prev].y as f64 + (lines as f64 / 2.0).ceil() * pitch - 23.0).round() as i64;
    }
    // 逐字拆分：按原下标从后往前替换，前面的下标不受影响
    for &i in overflowing.iter().rev() {
        let s = &shapes[i];
        let (lines, pitch) = col_geom(s);
        let joined: String = s.texts.concat();
        let chars: Vec<char> = joined.chars().collect();
        if lines < chars.len() as i64 {
            continue;
        }
        let letters: Vec<PxShape> = chars
            .iter()
            .enumerate()
            .map(|(k, ch)| PxShape {
                x: s.x,
                y: (s.y as f64 + k as f64 * pitch).round() as i64,
                w: s.w,
                h: pitch.ceil() as i64,
                texts: vec![ch.to_string()],
                font_pt: s.font_pt,
                text_hex: s.text_hex.clone(),
                ..PxShape::default()
            })
            .collect();
        shapes.splice(i..=i, letters);
    }
    shapes.sort_by_key(|s| s.y);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_shape(w: i64, h: i64, text: &str) -> PxShape {
        PxShape { w, h, texts: vec![text.to_string()], font_pt: Some(18), ..PxShape::default() }
    }

    #[test]
    fn columns_explode_single_letter_lines_and_sort_by_y() {
        // 18 pt → 24 px，0.72 × 24 = 17.28 px 一个字：宽 10 px 的框每行 1 个字，"ABCD" 4 行 × 28.8 > 2 × 5
        let mut shapes = vec![text_shape(10, 5, "ABCD")];
        shapes[0].y = 20;
        stack_overflowing_columns(&mut shapes);
        let ys: Vec<i64> = shapes.iter().map(|s| s.y).collect();
        assert_eq!(ys, vec![0, 29, 58, 86]);
        assert!(shapes.iter().all(|s| s.h == 29 && s.texts.len() == 1));
        // 装得下的文本形状不动
        let mut fits = vec![text_shape(300, 100, "Hi")];
        fits[0].y = 40;
        stack_overflowing_columns(&mut fits);
        assert_eq!(fits[0].y, 40);
    }
}
