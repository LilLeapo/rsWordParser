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

use crate::model::drawing::{DrawingDisplay, FillKind, ImageDisplay, ShapeDisplay, Wrap};
use crate::model::units::emu_to_px;
use crate::model::vml::{VmlKind, VmlShape};
use crate::model::{Block, Display, Inline, SegmentKind, TextBlock, VmlDisplay};
use crate::resolve::drawingml::{ColorBase, color_in, hex};
use crate::xml::{LocalName, NodeId, NsId, QName};

use super::blocks::Ctx;
use super::box_json;
use super::image;
use super::json::{set, set_some};

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

/// 宿主段落：节点、模型块、框外文字。参数打包，免得每个分支都拖一串。
#[derive(Clone, Copy)]
struct Para<'a> {
    node: NodeId,
    block: &'a TextBlock,
    stray: &'a str,
}

/// 段落里提取出的一个「框」。这一步只用到文字；几何与样式在下一步。
pub(super) struct BoxInfo {
    /// 框里每个段落的文字（`previewText` 用）。
    pub texts: Vec<String>,
    /// `textboxes[]` 里的那一项。
    pub json: Map<String, Value>,
}

impl BoxInfo {
    fn has_text(&self) -> bool {
        self.texts.iter().any(|t| !t.trim().is_empty())
    }
}

/// 段落里的绘图与 VML 显示模型，按文档序。`objects`：`w:object`（OLE 预览）算不算 VML——正文的绘图分支算
/// （TS 的 `w:pict` 分支同时看 `<w:object`），单元格的锚定框闸门不算（TS 只看 `<wp:anchor` 与 `<w:pict`）。
fn graphics(tb: &TextBlock, objects: bool) -> (Vec<&DrawingDisplay>, Vec<&VmlDisplay>) {
    let (mut drawings, mut vmls) = (Vec::new(), Vec::new());
    for i in &tb.inlines {
        let Inline::Run(r) = i else { continue };
        for s in &r.segments {
            match (&s.kind, s.display.as_ref()) {
                (SegmentKind::Drawing { .. }, Some(Display::Drawing(d))) => drawings.push(&**d),
                (SegmentKind::Pict, Some(Display::Vml(v))) => vmls.push(&**v),
                (SegmentKind::Object, Some(Display::Vml(v))) if objects => vmls.push(&**v),
                _ => {}
            }
        }
    }
    (drawings, vmls)
}

/// TS `extractCell` 的锚定框分支：**只取框，不做块分类**——单元格里的锚定形状 Word 是画在格里的
/// （行会长高来容纳它们），所以它们不像正文段落那样把整块降级成 `Text box`，而是作为
/// `cell.anchoredBoxes` 挂在格上（`COMPAT-10`）。
///
/// 返回框，以及**计算格内文字时要剥掉的子树**：TS 会把锚定的非图片绘图与带 `txbxContent` 的
/// `w:pict` 从段落里删掉再重新解析，否则框里的文字与 `wp:posOffset` 的数字会漏进单元格文本。
pub(super) fn anchored_boxes_in_cell(
    ctx: &Ctx<'_>,
    p: NodeId,
    tb: &TextBlock,
    docx_index: usize,
) -> (Vec<BoxInfo>, Vec<NodeId>) {
    // `w:object` 的预览图跟着 run 走（`richParas` 的图片 run），不进格的锚定框（任务 6.4，`m6-ole__006`）
    let (drawings, vmls) = graphics(tb, false);
    if drawings.is_empty() && vmls.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // TS 的闸门：段落里有锚定绘图，或者有任何 `w:pict`
    if !drawings.iter().any(|d| d.anchor.is_some()) && vmls.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let boxes = boxes_of(ctx, p, ctx.first_page(p, docx_index), &drawings, &vmls);
    if boxes.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // 剥掉：锚定且整棵里没有 `pic:pic` 的绘图（锚定的图片留着走 run 图片那条路）、
    // 带 `txbxContent` 的 `w:pict`
    let dom = ctx.dom;
    let mut stripped: Vec<NodeId> = drawings
        .iter()
        .filter(|d| d.anchor.is_some() && d.pictures.is_empty())
        .map(|d| d.node)
        .collect();
    stripped.extend(
        vmls.iter()
            .map(|v| v.node)
            .filter(|&n| dom.descendants(n).any(|c| dom.is(c, QName::w(LocalName::TxbxContent)))),
    );
    (boxes, stripped)
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
    let (drawings, vmls) = graphics(tb, true);
    if drawings.is_empty() && vmls.is_empty() {
        return None;
    }
    let docx_index = o.get("docxIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
    let stray = stray_text(ctx, p);
    // TS 的决策树里 `w:pict` 分支在 `w:drawing` 之前，两者规则不同，所以先分流。
    if !vmls.is_empty() {
        let first_page = ctx.first_page(p, docx_index);
        return vml_block(
            ctx,
            Para { node: p, block: tb, stray: &stray },
            first_page,
            &vmls,
            &drawings,
            o,
        );
    }
    let has_wsp = drawings.iter().any(|d| d.shapes.iter().any(|s| !s.is_group));
    let anchored = drawings.iter().filter(|d| d.anchor.is_some()).count();
    // 没有 wps 形状的段落，图片路径（4.4）已经处理过了；只有「一段里好几张分别锚定的图」
    // 要走照片框这条路。这条早退只在段落里**确实有一张解析得出的图**时成立（TS 的
    // `if (image)` 分支里面）——一张图都没有的绘图段落照样要往下走框 / 绘图对象的分类
    // （`hostile-input__006`：一个只有深嵌套 `a:g` 的空 `wp:anchor`）。
    let has_image = drawings
        .iter()
        .filter_map(|d| d.picture())
        .any(|pic| ctx.media.pick(pic.embed.as_deref(), pic.link.as_deref()).is_some());
    if has_image && !has_wsp && anchored <= 1 {
        return None;
    }

    let first_page = ctx.first_page(p, docx_index);
    let boxes = boxes_of(ctx, p, first_page, &drawings, &vmls);
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
    first_page: bool,
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
    let mut boxes = boxes_of(ctx, p, first_page, drawings, vmls);
    // 形状旁边的段落文字（画布说明、超链接行）：`w:pict` 这条路把它也做成一个只读框，
    // 不像 `w:drawing` 那条走 `strayRuns`（`docs/01` §6.2.6）。
    if !boxes.is_empty()
        && !stray.trim().is_empty()
        && let Some(b) = stray_box(ctx, para.block)
    {
        boxes.push(b);
    }
    let boxes = boxes;
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
    set(
        o,
        "textboxes",
        Value::Array(boxes.iter().map(|b| Value::Object(b.json.clone())).collect()),
    );
    // TS `hostPageBreak`：宿主段落自己带分页 `w:br`（框里的不算）→ 渲染器要在这里翻页（`out-of-run-breaks__004`）
    if super::blocks::host_page_break(ctx, p) {
        let mut fd = Map::new();
        set(&mut fd, "kind", "pageBreak");
        set(o, "fieldDisplay", Value::Object(fd));
    }
    if vml {
        set_some!(o, "imageAlign" => image::jc_align(ctx.dom, p));
        return;
    }
    // 段落自己带的文字（画布旁的说明、超链接）：作为只读的 `strayRuns` 留住，别让它凭空消失。
    // 锚定绘图段落里的**随文**图片同样走这条路：它们跟着行走，做成不浮动的照片框会把整块
    // 拽出零高的浮层。这时哪怕一个字都没有，也要出 `strayRuns`。
    let inline_run_pics = drawings.iter().any(|d| d.anchor.is_some())
        && drawings.iter().any(|d| d.anchor.is_none() && d.picture().is_some());
    let stray_runs = (!stray.is_empty() || inline_run_pics)
        .then(|| super::blocks::stray_runs_json(ctx, tb, inline_run_pics))
        .filter(|r| !r.is_empty());
    set_some!(o,
        "strayStyleId" => stray_runs.as_ref().and(tb.style_id.clone()),
        "strayRuns" =>
            stray_runs.map(|r| Value::Array(r.into_iter().map(Value::Object).collect())),
    );
    // TS 的 `imageMeta(xml)` 拿整段 XML 跑正则，每个字段取**第一处**匹配；一段里锚了多个绘图时
    // 后面的绘图不该覆盖前面的（`wrap-topbottom-band__001` 的两张卡片）。
    for d in drawings {
        let mut m = Map::new();
        image::image_meta(ctx, Some(p), d, &mut m);
        for (k, v) in m {
            o.entry(k).or_insert(v);
        }
    }
}

/// TS `paragraphStrayBox`：宿主段落里形状之外的文字，做成一个只读的展示框。
///
/// 段落已经整块降级成 `Text box` 了；这行字不做成框就只能从页面上消失。
fn stray_box(ctx: &Ctx<'_>, tb: &TextBlock) -> Option<BoxInfo> {
    // TS 是在**剥掉 `w:pict` 之后**的段落上取 run 的：形状自己贡献的图片 run 不算数
    let runs: Vec<Map<String, Value>> = super::blocks::runs_json(ctx, tb)
        .into_iter()
        .map(|mut r| {
            r.remove("image");
            r
        })
        .filter(|r| r.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty()))
        .collect();
    if !runs
        .iter()
        .any(|r| r.get("text").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty()))
    {
        return None;
    }
    let mut para = Map::new();
    set(&mut para, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
    if let Some(Value::Object(f)) = super::blocks::para_format_json(ctx, tb) {
        for (k, v) in f {
            para.insert(k, v);
        }
    }
    let texts = box_json::box_texts(std::slice::from_ref(&Value::Object(para.clone())));
    let mut json = Map::new();
    set(&mut json, "readOnly", true);
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut json, k, 0);
    }
    json.insert("paras".into(), Value::Array(vec![Value::Object(para)]));
    Some(BoxInfo { texts, json })
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
    let ln = d.shapes.iter().find_map(|s| s.line.as_ref());
    set_some!(out,
        // 只认字面 `a:srgbClr`（同 `picBorderOf`）
        "ruleColorHex" => ln
            .and_then(|l| l.fill)
            .and_then(|f| color_in(ctx.dom, f))
            .and_then(|c| match c.base {
                ColorBase::Srgb(rgb) => Some(hex(rgb.map(f64::from))),
                _ => None,
            }),
        "ruleThicknessPx" => ln
            .and_then(|l| l.width_emu)
            .filter(|&w| w > 0)
            .map(|w| emu_to_px(w as f64).round().max(1.0) as i64),
        "ruleWidthPx" =>
            d.extent.map(|e| e.cx).filter(|&cx| cx > 0).map(|cx| emu_to_px(cx as f64).round() as i64),
    );
}

/// TS `isInvisibleEmptyShape`：每个 `wps:wsp` 都显式 noFill + 描边 noFill、没有图也没有文字，
/// Word 什么都不画，块也就什么都不显示。
fn invisible_empty_shapes(ctx: &Ctx<'_>, drawings: &[&DrawingDisplay]) -> bool {
    let shapes: Vec<&ShapeDisplay> =
        drawings.iter().flat_map(|d| d.shapes.iter()).filter(|s| !s.is_group).collect();
    if shapes.is_empty() {
        return false;
    }
    if drawings.iter().any(|d| d.picture().is_some()) {
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
/// SmartArt 段落里的其他绘图（照片 / 形状）→ `textboxes[]`（TS 对多绘图的图示段落调 `extractTextboxes`，
/// 形状与图片都开；任务 6.3）。`drawings` 是整段的顶层绘图，图示自己没有 `wps` / `pic` 内容、不出框。
pub(super) fn sibling_boxes(
    ctx: &Ctx<'_>,
    para_node: NodeId,
    docx_index: usize,
    drawings: &[&DrawingDisplay],
) -> Vec<Value> {
    let first_page = ctx.first_page(para_node, docx_index);
    boxes_of(ctx, para_node, first_page, drawings, &[])
        .into_iter()
        .map(|b| Value::Object(b.json))
        .collect()
}

fn boxes_of(
    ctx: &Ctx<'_>,
    para_node: NodeId,
    first_page: bool,
    drawings: &[&DrawingDisplay],
    vmls: &[&VmlDisplay],
) -> Vec<BoxInfo> {
    let mut out = Vec::new();
    // `wrapSquare` 这道闸门把「转换器产出的装饰线」留在细横线那条路上：只有既方框绕排、
    // 又用了连线预设的段落，才把连线当成画出来的框（TS `hasLineShapes`）。
    let wrap_square = drawings
        .iter()
        .any(|d| d.anchor.as_ref().is_some_and(|a| matches!(a.wrap, Wrap::Square { .. })));
    let has_line_shapes = wrap_square && drawings.iter().any(|d| d.shapes.iter().any(is_line_prst));
    // 每个 `w:txbxContent` 占一个保存路径序号，不管框最后留没留下来。
    let mut ordinal = 0usize;
    // 管辖这一段的节：页面 / 页边距对齐的锚定位置要用它解（`model::section`）。
    let sect = ctx.section_at(para_node);
    let actx = box_json::AnchorCtx::new(drawings, sect, first_page);
    let no_anchor = drawings.iter().all(|d| d.anchor.is_none());

    for d in drawings {
        // 组的填充供组内 `a:grpFill` 继承
        let group_fills: Vec<Option<String>> = d
            .shapes
            .iter()
            .map(|g| {
                g.is_group
                    .then(|| box_json::wps_box_json(ctx, g, None, true, None))
                    .and_then(|j| j.get("fill").and_then(Value::as_str).map(str::to_string))
            })
            .collect();
        // 文档序遍历：框的顺序必须和 `w:txbxContent` 的顺序一致（保存路径按序号找回）。
        // `NodeId` 是解析时前序分配的，按它排就是文档序。
        enum Item<'a> {
            Shape(&'a ShapeDisplay),
            Pic(&'a ImageDisplay),
        }
        let mut items: Vec<(NodeId, Item<'_>)> = Vec::new();
        items.extend(d.shapes.iter().filter(|s| !s.is_group).map(|s| (s.node, Item::Shape(s))));
        // TS 的 `pushPic` 只对**组内**的图片开火；顶层图片走下面的 `photo_box`
        items.extend(
            d.pictures
                .iter()
                .filter(|p| p.group.is_some())
                .filter_map(|p| p.node.map(|n| (n, Item::Pic(p)))),
        );
        items.sort_by_key(|(n, _)| *n);
        for (_, item) in items {
            let s = match item {
                Item::Shape(s) => s,
                Item::Pic(pic) => {
                    if let Some(mut b) = picture_box(ctx, d, pic) {
                        if let Some(a) = &d.anchor {
                            box_json::apply_anchor(
                                &actx,
                                a,
                                d.extent,
                                pic.group.is_some(),
                                &mut b.json,
                            );
                        }
                        out.push(b);
                    }
                    continue;
                }
            };
            let index = s.txbx.is_some().then(|| {
                ordinal += 1;
                ordinal - 1
            });
            let group_fill = s.group.and_then(|g| group_fills.get(g)).and_then(Option::as_deref);
            let ctm = group_chain(d, s).and_then(|c| box_json::GroupCtm::compose(&c));
            if let Some(mut b) = wps_box(ctx, s, has_line_shapes, group_fill, false, index) {
                if let Some(ctm) = ctm {
                    box_json::apply_group_ctm(ctm, s, &mut b.json);
                }
                if let Some(a) = &d.anchor {
                    // 组内形状已经按组仿射摆到绝对位置了，锚定只再叠一层偏移
                    box_json::apply_anchor(&actx, a, d.extent, ctm.is_some(), &mut b.json);
                }
                out.push(b);
            }
            // 形状文本框里再嵌的绘图（TS 的 `walkShapes` 一路往下走）：照样成框，只是整块
            // 只读、不占保存序号——保存时改写的是外层那一份 `w:p` 列表，里面的形状会没。
            // 锚定信息用的是**外层**绘图的（嵌套绘图自己是随文的）。
            for sh in nested_shapes(&s.content) {
                let Some(mut b) = wps_box(ctx, sh, has_line_shapes, None, true, None) else {
                    continue;
                };
                if let Some(a) = &d.anchor {
                    box_json::apply_anchor(&actx, a, d.extent, false, &mut b.json);
                }
                out.push(b);
            }
        }
        // 只有图、一个形状都没有的绘图，与别的绘图挤在同一段里：整张图降级成只读照片框。
        // 尺寸取 `wp:extent`（`pic:spPr` 里往往没有 `a:xfrm`）。段落里另有锚定绘图时，
        // 随文的图不进来——它们按 run 内图片随行排，框才能全都浮起来（TS 末尾那段）。
        if !d.shapes.iter().any(|s| !s.is_group)
            && (d.anchor.is_some() || no_anchor)
            && let Some(mut b) = photo_box(ctx, d)
        {
            if let Some(a) = &d.anchor {
                box_json::apply_anchor(&actx, a, d.extent, false, &mut b.json);
            }
            out.push(b);
        }
    }

    for v in vmls {
        vml_boxes(ctx, v, &mut ordinal, &mut out);
    }
    out
}

/// 一个 `w:pict` / `w:object` 里的 VML 框（TS `walkVml` + `vmlBox`）。
///
/// `v:group` 就是 Word 的「绘图画布」：它给孩子定义了自己的坐标系，孩子的 `style` 里
/// 写的是组坐标而不是长度。缩放与原点沿着组链往下传，`shapes` 是前序表——组一定排在
/// 自己的孩子前面，所以一遍扫过去就能把每个形状的放置环境算出来。
fn vml_boxes(ctx: &Ctx<'_>, v: &VmlDisplay, ordinal: &mut usize, out: &mut Vec<BoxInfo>) {
    let mut places: Vec<box_json::VmlPlace> = Vec::with_capacity(v.shapes.len());
    for s in &v.shapes {
        let parent = s.parent.map_or(box_json::VmlPlace::default(), |p| places[p]);
        if s.kind == VmlKind::Group {
            places.push(group_place(s, parent, out));
            continue;
        }
        places.push(parent);
        // TS 的 `walkVml` 只对 `v:shape` / `rect` / `roundrect` / `oval` 建框
        if !matches!(s.kind, VmlKind::Shape | VmlKind::Rect | VmlKind::RoundRect | VmlKind::Oval) {
            continue;
        }
        if !s.has_textbox {
            // 无字形状按 TS 的顺序试三条路：WordArt → 图片 → 有可见填充 / 描边的几何
            if let Some(json) = box_json::vml_wordart_box(s) {
                out.push(BoxInfo { texts: s.textpath.clone().into_iter().collect(), json });
                continue;
            }
            // 别人框里的形状不该以兄弟框的身份冒到页面上
            if s.nested {
                continue;
            }
            let json =
                box_json::vml_pic_box(ctx, s, parent).or_else(|| box_json::vml_geom_box(s, parent));
            if let Some(json) = json {
                out.push(BoxInfo { texts: Vec::new(), json });
            }
            continue;
        }
        let (paras, read_only) = box_json::paras_json(ctx, &s.content);
        // 别人框里的形状不占保存路径序号——保存时改写的是外层那一份 `w:p` 列表。
        let index = (!s.nested).then(|| {
            let i = *ordinal;
            if s.txbx.is_some() {
                *ordinal += 1;
            }
            i
        });
        let mut json = box_json::vml_box_json(s, parent, index.filter(|_| s.txbx.is_some()));
        if read_only || s.nested {
            json.insert("readOnly".into(), Value::Bool(true));
        }
        // 一个 run 都没有的框什么都画不出来：留着只会在版面上多一个空洞
        if !box_json::any_runs(&paras) {
            continue;
        }
        let texts = box_json::box_texts(&paras);
        json.insert("paras".into(), Value::Array(paras));
        out.push(BoxInfo { texts, json });
    }
}

/// 一个 `v:group` 给孩子定的放置环境；随文画布顺带占住自己的流内位置。
fn group_place(
    g: &VmlShape,
    parent: box_json::VmlPlace,
    out: &mut Vec<BoxInfo>,
) -> box_json::VmlPlace {
    let Some(scale) = box_json::vml_group_scale(g, parent.scale) else {
        // 定不出缩放：孩子沿用外层的缩放，但原点断掉（TS `gScale ? gOrigin : null`）
        return box_json::VmlPlace { scale: parent.scale, origin: None };
    };
    let mut origin = match (parent.scale, parent.origin) {
        // 组里套组：按外层组的坐标系放
        (Some(ps), Some(po)) => Some((
            box_json::vml_coord_px(g, "left", ps, po),
            box_json::vml_coord_px(g, "top", ps, po),
        )),
        _ if g.is_absolute() => {
            let px = |key: &str| {
                g.style_len(key)
                    .and_then(|l| l.to_emu())
                    .map_or(0.0, crate::model::units::emu_to_px)
            };
            Some((px("margin-left"), px("margin-top")))
        }
        // 随文画布：先给它在文字流里占一块，孩子再浮在上面；不占的话整段塌成零高
        _ if !g.nested => box_json::vml_canvas_box(g, parent.scale).map(|json| {
            out.push(BoxInfo { texts: Vec::new(), json });
            (0.0, 0.0)
        }),
        _ => None,
    };
    // 画布的孩子从 `coordorigin` 量起，不是从 0,0
    if let (Some(o), Some((cox, coy))) = (origin, g.coordorigin) {
        origin = Some((o.0 - cox as f64 * scale.0, o.1 - coy as f64 * scale.1));
    }
    box_json::VmlPlace { scale: Some(scale), origin }
}

fn px(emu: i64) -> i64 {
    crate::model::units::emu_to_px(emu as f64).round() as i64
}

/// `pictures` 选项下的只读照片框（TS `pushPic`）：组内图片按组仿射映射到绝对位置。
fn picture_box(ctx: &Ctx<'_>, d: &DrawingDisplay, pic: &ImageDisplay) -> Option<BoxInfo> {
    let m = ctx.media.pick(pic.embed.as_deref(), pic.link.as_deref())?;
    let ext = pic.ext.filter(|e| e.cx > 0 && e.cy > 0)?;
    let ctm = pic
        .group
        .and_then(|g| d.shapes.get(g))
        .and_then(|g| group_chain_from(d, g))
        .and_then(|c| box_json::GroupCtm::compose(&c));
    let (sx, sy) = ctm.map_or((1.0, 1.0), |c| (c.sx, c.sy));
    let mut json = Map::new();
    json.insert("readOnly".into(), Value::Bool(true));
    json.insert("fillImageDataUrl".into(), Value::String(m.url.clone()));
    json.insert("widthPx".into(), Value::from(px((ext.cx as f64 * sx).round() as i64)));
    json.insert("heightPx".into(), Value::from(px((ext.cy as f64 * sy).round() as i64)));
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        json.insert(k.into(), Value::from(0));
    }
    if let Some(rot) = pic.rot_60k.filter(|&r| r != 0) {
        json.insert("rotDeg".into(), Value::from((rot as f64 / 60_000.0).round() as i64));
    }
    if let Some(c) = ctm
        && let Some((x, y)) = pic.off
    {
        json.insert("offsetXEmu".into(), Value::from((c.tx + x as f64 * c.sx).round() as i64));
        json.insert("offsetYEmu".into(), Value::from((c.ty + y as f64 * c.sy).round() as i64));
        json.insert("floating".into(), Value::Bool(true));
    }
    json.insert("paras".into(), Value::Array(Vec::new()));
    Some(BoxInfo { texts: Vec::new(), json })
}

/// 整个绘图就是一张图（一个 `wps:wsp` 都没有）时的只读照片框。
///
/// 与 [`picture_box`] 的区别是尺寸来源：组内图片有自己的 `a:xfrm/a:ext`，顶层图片通常没有，
/// 尺寸就是绘图的 `wp:extent`。
fn photo_box(ctx: &Ctx<'_>, d: &DrawingDisplay) -> Option<BoxInfo> {
    let pic = d.picture()?;
    let m = ctx.media.pick(pic.embed.as_deref(), pic.link.as_deref())?;
    let ext = d.extent.filter(|e| e.cx > 0 && e.cy > 0)?;
    let mut json = Map::new();
    set(&mut json, "readOnly", true);
    set(&mut json, "fillImageDataUrl", m.url.clone());
    set(&mut json, "widthPx", px(ext.cx));
    set(&mut json, "heightPx", px(ext.cy));
    for k in ["insetTopPx", "insetRightPx", "insetBottomPx", "insetLeftPx"] {
        set(&mut json, k, 0);
    }
    set_some!(&mut json,
        "rotDeg" => pic.rot_60k.filter(|&r| r != 0).map(|r| (r as f64 / 60_000.0).round() as i64),
    );
    json.insert("paras".into(), Value::Array(Vec::new()));
    Some(BoxInfo { texts: Vec::new(), json })
}

/// 一段内容块里所有的绘图形状（文档序，不含组本身）。
fn shapes_in(content: &[Block]) -> Vec<&ShapeDisplay> {
    let mut out = Vec::new();
    for b in content {
        let Block::Text(tb) = b else { continue };
        for i in &tb.inlines {
            let Inline::Run(r) = i else { continue };
            for seg in &r.segments {
                let Some(d) = seg.display.as_ref().and_then(Display::as_drawing) else { continue };
                out.extend(d.shapes.iter().filter(|s| !s.is_group));
            }
        }
    }
    out
}

/// 形状文本框里再嵌的绘图形状，前序、迭代。
fn nested_shapes(content: &[Block]) -> Vec<&ShapeDisplay> {
    let mut out = Vec::new();
    let mut stack = shapes_in(content);
    stack.reverse();
    while let Some(s) = stack.pop() {
        out.push(s);
        let mut kids = shapes_in(&s.content);
        kids.reverse();
        stack.extend(kids);
    }
    out
}

/// 从 `start` 这个组开始往外的组链（最外层在前）。
fn group_chain_from<'a>(
    d: &'a DrawingDisplay,
    start: &'a ShapeDisplay,
) -> Option<Vec<&'a ShapeDisplay>> {
    let mut chain = vec![start];
    let mut cur = start.group;
    while let Some(i) = cur {
        let g = d.shapes.get(i)?;
        chain.push(g);
        cur = g.group;
    }
    chain.reverse();
    Some(chain)
}

/// 形状所在的组链，从最外层到最内层。
fn group_chain<'a>(d: &'a DrawingDisplay, s: &ShapeDisplay) -> Option<Vec<&'a ShapeDisplay>> {
    let mut chain = Vec::new();
    let mut cur = s.group;
    while let Some(i) = cur {
        let g = d.shapes.get(i)?;
        chain.push(g);
        cur = g.group;
    }
    (!chain.is_empty()).then(|| {
        chain.reverse();
        chain
    })
}

fn is_line_prst(s: &ShapeDisplay) -> bool {
    s.prst.as_deref().is_some_and(|p| LINE_PRSTS.contains(&p))
}

/// TS `buildWpsBox` 的「留不留这个形状」判定。`opts.shapes` 在绘图分支恒为真。
fn wps_box(
    ctx: &Ctx<'_>,
    s: &ShapeDisplay,
    has_line_shapes: bool,
    group_fill: Option<&str>,
    nested: bool,
    index: Option<usize>,
) -> Option<BoxInfo> {
    let (paras, structured) = box_json::paras_json_in(ctx, &s.content, s.content_part);
    let texts = box_json::box_texts(&paras);
    let has_text = box_json::any_runs(&paras);
    // `paint` 要查主题与媒体，所以放在最后算：前面的形状规则先把不用看颜色的情况筛掉。
    if !keeps_box(s, has_line_shapes, s.txbx.is_some(), has_text, || paint(ctx, s)) {
        return None;
    }
    // 连线形状走 `lineBoxOf`：合成的 `prst` 带箭头信息，内容恒为空。
    if s.txbx.is_none() && is_line_prst(s) {
        let mut json = box_json::line_box_json(ctx, s);
        json.insert("paras".into(), Value::Array(Vec::new()));
        return Some(BoxInfo { texts: Vec::new(), json });
    }
    let mut json = box_json::wps_box_json(ctx, s, group_fill, nested, index);
    if structured {
        json.insert("readOnly".into(), Value::Bool(true));
    }
    if s.txbx.is_none() {
        // 无字预设形状：没有 `w:txbxContent` 可以打补丁，只有 `cNvPr` 的 id 能让保存路径
        // 塞进一个新的 `wps:txbx` 时才可编辑；Word 把形状文字居中，没写 anchor 时对齐它。
        if s.cnv_id.is_none() {
            json.insert("readOnly".into(), Value::Bool(true));
        } else if !json.contains_key("vAlign") && s.body.is_none_or(|b| b.anchor.is_none()) {
            json.insert("vAlign".into(), Value::String("center".into()));
        }
    }
    let paras = if has_text { paras } else { Vec::new() };
    let texts = if has_text { texts } else { Vec::new() };
    json.insert("paras".into(), Value::Array(paras));
    Some(BoxInfo { texts, json })
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
            geom: None,
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
            txbx_rel: None,
            content_part: None,
            content: Vec::new(),
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
