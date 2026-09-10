//! 新建浮动文本框 / 形状 / 线条（`spec/18` 7.7 的 `NewBlock::Textbox / Shape / Line`）。
//!
//! 都是 DrawingML 的 `wps:wsp`。Transitional 包按 Word / TS 的形态发一对
//! `mc:Choice Requires="wps"` + `mc:Fallback`（VML 孪生）；**Strict 包只发 Choice**——
//! Strict 里没有 VML，发了反而是不合法的内容。
//!
//! `Requires="wps"` 的前缀必须在 `mc:AlternateContent` 那一层能解析出来，不然 Word 会把整份
//! 文件报成"内容有问题"（TS `generate.ts` 的同一条注释）。所以 `xmlns:wps` 声明写在
//! `mc:AlternateContent` 上，不写在里层的 `wps:wsp` 上。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::package::PartFlavor;
use crate::package::ns_context::NamespaceContext;
use crate::xml::{
    Dirty, LocalName, NewElement, NodeEdit, NodeId, NsId, QName, Target, parse_fragment,
};

use super::media_ops::{ImageWrap, NS_WP, PosOffset, Z_ORDER_BASE, prefix_or_decl};
use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;
use super::{EditContext, NewBlock};

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const NS_WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const NS_V: &str = "urn:schemas-microsoft-com:vml";
/// EMU / pt（VML 的 `@style` 用 pt）。
const EMU_PER_PT: f64 = 12700.0;

/// `a:prstGeom/@prst`（ECMA-376 `ST_ShapeType`）：`rect` / `roundRect` / `ellipse` / `triangle` / …
/// 不是闭集——Word 认的两百来个预置形状都能写，值原样进属性。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(transparent)]
pub struct PresetGeom(pub String);

impl PresetGeom {
    pub fn rect() -> Self {
        PresetGeom("rect".into())
    }
}

named_enum! {
    /// 可插入的线条 / 连接符（TS `LINE_KINDS`）。
    pub enum LineKind {
        Line = "line",
        LineArrow = "lineArrow",
        LineArrowDouble = "lineArrowDouble",
        LineBent = "lineBent",
        LineCurved = "lineCurved",
    }
}

impl LineKind {
    /// `(a:prstGeom/@prst, 起点箭头, 终点箭头)`（TS `LINE_KINDS` 的三个字段）。
    fn geom(self) -> (&'static str, bool, bool) {
        match self {
            LineKind::Line => ("line", false, false),
            LineKind::LineArrow => ("straightConnector1", false, true),
            LineKind::LineArrowDouble => ("straightConnector1", true, true),
            LineKind::LineBent => ("bentConnector3", false, false),
            LineKind::LineCurved => ("curvedConnector3", false, false),
        }
    }
}

/// 新建文本框 / 形状共用的外观与位置。
#[derive(Debug, Clone, PartialEq, Eq, Default, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShapeLook {
    /// 显示尺寸（EMU）；缺省 1800000 × 1080000（TS `buildTextboxParagraphXml`）。
    pub extent_emu: Option<(i64, i64)>,
    /// 锚定位置；`None` = 横向居中、纵向贴段落（TS 同）。
    pub pos_offset_emu: Option<PosOffset>,
    /// 绕排；缺省方形。
    pub wrap: Option<ImageWrap>,
    /// `relativeHeight = 251658240 + z_order`。
    pub z_order: Option<i64>,
    /// 填充色（六位十六进制）；`None` = `a:noFill`。
    pub fill: Option<String>,
    /// 描边色；`None` = `a:ln/a:noFill`。
    pub outline: Option<String>,
}

fn hex(v: &str) -> String {
    v.trim_start_matches('#').to_string()
}

/// `wps:spPr`：几何 + 填充 + 描边。
fn sp_pr(preset: &str, cx: i64, cy: i64, look: &ShapeLook) -> String {
    let fill = match &look.fill {
        Some(c) => format!(r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#, hex(c)),
        None => "<a:noFill/>".into(),
    };
    let line = match &look.outline {
        Some(c) => {
            format!(r#"<a:ln><a:solidFill><a:srgbClr val="{}"/></a:solidFill></a:ln>"#, hex(c))
        }
        None => "<a:ln><a:noFill/></a:ln>".into(),
    };
    format!(
        concat!(
            r#"<wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
            r#"<a:prstGeom prst="{preset}"><a:avLst/></a:prstGeom>{fill}{line}</wps:spPr>"#
        ),
        cx = cx,
        cy = cy,
        preset = preset,
        fill = fill,
        line = line,
    )
}

/// `wp:anchor` 的开头到 `wp:docPr`（`a:graphic` 由调用方接上）。
fn anchor_open(wp: &str, cx: i64, cy: i64, look: &ShapeLook, id: i64, name: &str) -> String {
    let wrap = look.wrap.unwrap_or(ImageWrap::SquareLeft);
    let behind = if wrap == ImageWrap::Behind { "1" } else { "0" };
    let position = match look.pos_offset_emu {
        Some(p) => {
            let (rh, rv) = if p.page { ("page", "page") } else { ("column", "paragraph") };
            format!(
                concat!(
                    r#"<{wp}:positionH relativeFrom="{rh}"><{wp}:posOffset>{x}</{wp}:posOffset></{wp}:positionH>"#,
                    r#"<{wp}:positionV relativeFrom="{rv}"><{wp}:posOffset>{y}</{wp}:posOffset></{wp}:positionV>"#
                ),
                wp = wp,
                rh = rh,
                rv = rv,
                x = p.x,
                y = p.y
            )
        }
        None => format!(
            concat!(
                r#"<{wp}:positionH relativeFrom="column"><{wp}:align>center</{wp}:align></{wp}:positionH>"#,
                r#"<{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>0</{wp}:posOffset></{wp}:positionV>"#
            ),
            wp = wp
        ),
    };
    let wrap_el = match wrap {
        ImageWrap::TopBottom => format!("<{wp}:wrapTopAndBottom/>"),
        ImageWrap::Front | ImageWrap::Behind => format!("<{wp}:wrapNone/>"),
        _ => format!(r#"<{wp}:wrapSquare wrapText="bothSides"/>"#),
    };
    format!(
        concat!(
            r#"<{wp}:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0""#,
            r#" relativeHeight="{z}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1">"#,
            r#"<{wp}:simplePos x="0" y="0"/>{position}"#,
            r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="0" t="0" r="0" b="0"/>{wrap_el}"#,
            r#"<{wp}:docPr id="{id}" name="{name} {id}"/>"#
        ),
        wp = wp,
        z = Z_ORDER_BASE + look.z_order.unwrap_or(0),
        behind = behind,
        position = position,
        cx = cx,
        cy = cy,
        wrap_el = wrap_el,
        id = id,
        name = name,
    )
}

/// 把 `wps:wsp` 裹成一个 `w:p`：Transitional 发 `mc:AlternateContent`（Choice + VML Fallback），
/// Strict 只发 `w:drawing`。
fn wrap_paragraph(
    flavor: PartFlavor,
    wp: &str,
    wp_decl: &str,
    anchor_head: &str,
    wsp: &str,
    vml: Option<&str>,
) -> String {
    let graphic = format!(
        r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{wps}">{wsp}</a:graphicData></a:graphic>"#,
        a = NS_A,
        wps = NS_WPS,
        wsp = wsp,
    );
    let drawing = format!("<w:drawing{wp_decl}>{anchor_head}{graphic}</{wp}:anchor></w:drawing>");
    match (flavor, vml) {
        (PartFlavor::Strict, _) | (_, None) => format!("<w:p><w:r>{drawing}</w:r></w:p>"),
        (_, Some(vml)) => format!(
            concat!(
                r#"<w:p><w:r><mc:AlternateContent xmlns:mc="{mc}" xmlns:wps="{wps}">"#,
                r#"<mc:Choice Requires="wps">{drawing}</mc:Choice>"#,
                r#"<mc:Fallback><w:pict>{vml}</w:pict></mc:Fallback>"#,
                r#"</mc:AlternateContent></w:r></w:p>"#
            ),
            mc = NS_MC,
            wps = NS_WPS,
            drawing = drawing,
            vml = vml,
        ),
    }
}

/// `NewBlock::Textbox` / `Shape` / `Line` → 段落子树（`chart_ops::materialize` 调）。
///
/// 模板里的 `w:txbxContent` 先留空，解析完再把内容块（以及 VML 孪生那份的克隆）挂进去——
/// 内容是 [`NewBlock`]，拼字符串拼不出来。
pub(crate) fn shape_paragraph(s: &mut EditSession, block: NewBlock) -> Result<NewElement> {
    let flavor = s.flavor();
    let main = s.main_part();
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let id = super::media_ops::next_doc_pr_id(dom);
    let ctx = NamespaceContext::from_dom(dom, flavor);
    let (wp, wp_decl) = prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
    let pt = |emu: i64| format!("{:.2}", emu as f64 / EMU_PER_PT);

    // 框里的内容块先落成 `NewElement`（空的话给一个空格段：Word 不接受空文本框）
    let inner: Vec<NewBlock> = match &block {
        NewBlock::Textbox { blocks, .. } if !blocks.is_empty() => blocks.clone(),
        NewBlock::Textbox { .. } => vec![space_paragraph()],
        NewBlock::Shape { text: Some(t), .. } => vec![text_paragraph(t)],
        _ => Vec::new(),
    };
    let inner = super::chart_ops::materialize_all(s, inner)?;
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let inner: Vec<NewElement> =
        inner.into_iter().map(|b| super::ops::new_block_element(dom, b)).collect();
    let has_text = !inner.is_empty();

    let xml = match &block {
        NewBlock::Textbox { look, .. } | NewBlock::Shape { look, .. } => {
            let (preset, name) = match &block {
                NewBlock::Shape { preset, .. } => (preset.0.as_str(), "Shape"),
                _ => ("rect", "TextBox"),
            };
            let (cx, cy) = look.extent_emu.unwrap_or((1_800_000, 1_080_000));
            let (cx, cy) = (cx.max(1), cy.max(1));
            let wsp = format!(
                "<wps:wsp><wps:cNvSpPr{tx}/>{sp}{txbx}<wps:bodyPr/></wps:wsp>",
                tx = if has_text { r#" txBox="1""# } else { "" },
                sp = sp_pr(preset, cx, cy, look),
                txbx = if has_text { "<wps:txbx><w:txbxContent/></wps:txbx>" } else { "" },
            );
            let vml = format!(
                concat!(
                    r#"<v:rect xmlns:v="{v}" style="position:absolute;width:{w}pt;height:{h}pt""#,
                    r#"{fill} stroked="{stroked}">{txbx}</v:rect>"#
                ),
                v = NS_V,
                w = pt(cx),
                h = pt(cy),
                fill = match &look.fill {
                    Some(c) => format!(r##" fillcolor="#{}" filled="t""##, hex(c)),
                    None => r#" filled="f""#.to_string(),
                },
                stroked = if look.outline.is_some() { "t" } else { "f" },
                txbx = if has_text { "<v:textbox><w:txbxContent/></v:textbox>" } else { "" },
            );
            wrap_paragraph(
                flavor,
                &wp,
                &wp_decl,
                &anchor_open(&wp, cx, cy, look, id, name),
                &wsp,
                Some(&vml),
            )
        }
        NewBlock::Line { kind, from, to, color } => {
            let (cx, cy) = ((to.0 - from.0).abs().max(1), (to.1 - from.1).abs().max(1));
            let (prst, head, tail) = kind.geom();
            let color = color.as_deref().map_or_else(|| "000000".to_string(), hex);
            let ln = format!(
                r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill>{h}{t}</a:ln>"#,
                h = if head { r#"<a:headEnd type="triangle"/>"# } else { "" },
                t = if tail { r#"<a:tailEnd type="triangle"/>"# } else { "" },
            );
            let wsp = format!(
                concat!(
                    r#"<wps:wsp><wps:cNvSpPr/><wps:spPr>"#,
                    r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                    r#"<a:prstGeom prst="{prst}"><a:avLst/></a:prstGeom><a:noFill/>{ln}"#,
                    r#"</wps:spPr><wps:bodyPr/></wps:wsp>"#
                ),
                cx = cx,
                cy = cy,
                prst = prst,
                ln = ln,
            );
            // 线条的位置是两点的左上角，大小是两点的差
            let look = ShapeLook {
                extent_emu: Some((cx, cy)),
                pos_offset_emu: Some(PosOffset {
                    x: from.0.min(to.0),
                    y: from.1.min(to.1),
                    page: false,
                }),
                wrap: Some(ImageWrap::Front),
                ..Default::default()
            };
            // 线条没有 VML 孪生：TS 的 `buildLineParagraphXml` 也只发 Choice
            wrap_paragraph(
                flavor,
                &wp,
                &wp_decl,
                &anchor_open(&wp, cx, cy, &look, id, "Line"),
                &wsp,
                None,
            )
        }
        _ => return Err(Error::edit(DiagCode::EditPlanInvalid, "不是形状块")),
    };
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let mut frags = parse_fragment(dom, &xml)
        .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("形状段落解析失败: {e}")))?;
    let mut para =
        frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "形状段落为空"))?;
    fill_txbx(&mut para, &inner);
    Ok(para)
}

fn space_paragraph() -> NewBlock {
    text_paragraph(" ")
}

fn text_paragraph(text: &str) -> NewBlock {
    NewBlock::Paragraph {
        props: None,
        inlines: vec![super::NewInline::Run(super::NewRun::text(text))],
    }
}

/// 树里每一处空的 `w:txbxContent` 都填上同一份内容（Choice 一处、VML 孪生一处）。
fn fill_txbx(node: &mut NewElement, blocks: &[NewElement]) {
    if node.name == QName::new(NsId::W, LocalName::TxbxContent) {
        node.children.extend(blocks.iter().cloned().map(crate::xml::NewNode::Element));
        return;
    }
    for c in &mut node.children {
        if let crate::xml::NewNode::Element(e) = c {
            fill_txbx(e, blocks);
        }
    }
}

/// `EDIT-03 SetTextboxContent`：一个 `w:txbxContent` 里的块整体换掉。`box_node` 可以是
/// `w:txbxContent` 自己，也可以是包着它的 `wps:wsp` / `wps:txbx` / `v:shape` / `v:textbox`。
/// 落在 `mc:Fallback` 里 → `EDIT_TARGET_FALLBACK`（守卫在 `ops::run`），孪生由它同步。
pub(crate) fn set_textbox_content(
    s: &mut EditSession,
    box_node: NodeId,
    blocks: Vec<NewBlock>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let blocks = super::chart_ops::materialize_all(s, blocks)?;
    let part = s.main_part();
    let dom = s.dom();
    if (box_node.0 as usize) >= dom.node_count() || dom.node(box_node).dirty == Dirty::Deleted {
        return Err(Error::edit(DiagCode::EditBadPosition, "文本框节点不存在"));
    }
    let txbx = QName::new(NsId::W, LocalName::TxbxContent);
    let container = if dom.is(box_node, txbx) {
        box_node
    } else {
        dom.descendants(box_node)
            .find(|&n| dom.node(n).dirty != Dirty::Deleted && dom.is(n, txbx))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "这里没有 w:txbxContent"))?
    };
    let mut tracker = super::track::Tracker::new(s.document(), ctx);
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    for c in dom.children(container).iter().copied() {
        if dom.node(c).dirty == Dirty::Deleted || dom.element(c).is_none() {
            continue;
        }
        match &mut tracker {
            Some(t) => super::ops::plan_delete_block_tracked(&mut plan, dom, t, c),
            None => plan.node_edits.push(NodeEdit::Delete(c)),
        }
    }
    for block in blocks {
        let opaque = matches!(block, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
        let node = super::ops::new_block_element(dom, block);
        let node = match &mut tracker {
            Some(t) => super::track::mark_new_block_inserted(t, node, opaque),
            None => node,
        };
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: None,
            node,
        });
    }
    s.commit_plan(plan)
}
