//! 绘图显示模型（`MOD-11`；`spec/15` 任务 4.3）。
//!
//! 一个 `w:drawing` 的**文档事实**：锚定几何、`wp:extent`、`wp:docPr`，以及 `pic:pic` 的图片信息。
//! 这里**没有**任何由排版决定的字段（px、band、猜出来的浮动方向）——那些是 `bind/compat_ts` 的
//! 投影（`spec/15` 分层决策）。长度一律 EMU 原值，角度一律 1/60000 度原值。
//!
//! ## 遍历边界
//!
//! `w:txbxContent` 是**独立内容流**：文本框里的段落有自己的 run 与自己的图。所以扫一个 drawing 时
//! 不下钻进 `txbxContent`，也不下钻进嵌套的 `w:drawing`——否则文本框里的图会被当成段落级图片，
//! 分类全错（`spec/15` 风险 3，对应 TS `topLevelDrawings` 的平衡匹配）。
//!
//! 遍历是迭代的，带深度上限：语料里有几千层嵌套的恶意输入。

use crate::model::block::Block;
use crate::model::custgeom::{CustomGeom, custom_geom};
use crate::model::facts::DrawingKind;
use crate::model::vml::VmlDisplay;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 绘图子树的深度上限，与 `MOD-07` 的块嵌套上限同值。
const MAX_DEPTH: u32 = 64;

/// `Segment.display` / `ProtectedBlock.display` / `ImageBlock.display`：显示载荷（`MOD-11`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Display {
    /// `w:drawing`
    Drawing(Box<DrawingDisplay>),
    /// `w:pict` / `w:object`（含 OLE 信息）
    Vml(Box<VmlDisplay>),
}

impl Display {
    pub fn as_drawing(&self) -> Option<&DrawingDisplay> {
        match self {
            Display::Drawing(d) => Some(d),
            Display::Vml(_) => None,
        }
    }

    pub fn as_vml(&self) -> Option<&VmlDisplay> {
        match self {
            Display::Vml(v) => Some(v),
            Display::Drawing(_) => None,
        }
    }
}

/// 一个 `w:drawing`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingDisplay {
    /// `w:drawing` 节点。
    pub node: NodeId,
    pub kind: DrawingKind,
    /// `wp:anchor` 的锚定几何；`wp:inline` → `None`（随文）。
    pub anchor: Option<AnchorGeom>,
    /// `wp:extent`（EMU）。
    pub extent: Option<Extent>,
    pub doc_pr: DocPr,
    /// 全部 `pic:pic`，文档序。段落级图片取第一个（[`DrawingDisplay::picture`]）；
    /// 组里的图片各有自己的位置与所属组。图表 / SmartArt 的载荷在 M6。
    pub pictures: Vec<ImageDisplay>,
    /// `wps:wsp` 形状与 `wpg` 组，文档序；组内形状排在组之后，`group` 指回组。
    pub shapes: Vec<ShapeDisplay>,
    /// `a:graphicData` 里的 `c:chart` / `cx:chart`：图表 part 的引用（M6 6.1）。part 本身在
    /// `Document.chart_parts`，按 `rel_id` 经 `Document.chart_by_rel` 找。
    pub chart: Option<ChartRef>,
}

/// `c:chart r:id` / `cx:chart r:id`：一个绘图对图表 part 的引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartRef {
    pub node: NodeId,
    /// `r:id`；写丢了 → `None`（TS 同样解析不出图表）。
    pub rel_id: Option<String>,
    /// `cx:chart`（2014 chartex）。
    pub chartex: bool,
}

/// 一个 `wps:wsp` 形状，或一个 `wpg:wgp` / `wpg:grpSp` 组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeDisplay {
    pub node: NodeId,
    /// 组本身（`wpg`）不画东西，只提供子坐标系。
    pub is_group: bool,
    /// `wps:cNvPr/@id`：保存路径要靠它往形状里塞新的 `wps:txbx`。
    pub cnv_id: Option<String>,
    /// `a:prstGeom/@prst`：预设几何名（`rect` / `line` / `straightConnector1` …）。
    pub prst: Option<String>,
    /// 有 `a:custGeom`：自定义路径几何。
    pub cust_geom: bool,
    /// `a:custGeom` 的路径；用到公式或圆弧时为 `None`（`model::custgeom`）。
    pub geom: Option<CustomGeom>,
    /// `a:xfrm/a:ext`（EMU）。
    pub ext: Option<Extent>,
    /// `a:xfrm/a:off`（EMU）。
    pub off: Option<(i64, i64)>,
    /// 组的子坐标系原点与尺寸（`a:chOff` / `a:chExt`），用来算组内形状的仿射。
    pub ch_off: Option<(i64, i64)>,
    pub ch_ext: Option<Extent>,
    /// `a:xfrm/@rot`，1/60000 度。
    pub rot_60k: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    /// `spPr` 的填充。
    pub fill: Option<FillDisplay>,
    /// `spPr/a:ln`。
    pub line: Option<LineDisplay>,
    /// `wps:style/a:fillRef` / `a:lnRef`：主题引用，`idx > 0` 时补缺省颜色。
    pub fill_ref: Option<StyleRef>,
    pub line_ref: Option<StyleRef>,
    /// `wps:style/a:fontRef`：图库形状的文字颜色出处（缺省蓝形状引用 `lt1`，所以 Word 里
    /// 不写任何 run 颜色也显示白字）。
    pub font_ref: Option<StyleRef>,
    /// `spPr/a:effectLst` 里有内容（阴影等）。空形状判定要看它。
    pub has_effects: bool,
    /// `wps:bodyPr`。
    pub body: Option<BodyPr>,
    /// `wps:txbx/w:txbxContent`：框里的独立内容流。
    pub txbx: Option<NodeId>,
    /// `wps:txbx/@r:txbx`：框的内容在**另一个 part** 里（`word/txbx1.xml`，根是 `w14:txbx`）。
    /// 与 `txbx` 互斥：本 part 里没有 `w:txbxContent` 时才看它。
    pub txbx_rel: Option<String>,
    /// 框里内容流的块（`MOD-11` 的 `content`）。由 `Document::rebuild` 复用段落管线构建。
    pub content: Vec<Block>,
    /// `content` 里的 `NodeId` 属于哪个 part。`None` = 本 part（`txbx`）；
    /// `Some` = 外部文本框 part（`txbx_rel`），投影要换那个 part 的 DOM。
    pub content_part: Option<crate::package::PartId>,
    /// 所属组在 `shapes` 里的下标。
    pub group: Option<usize>,
}

impl ShapeDisplay {
    /// 有可见的填充、描边或图片——空形状据此判断要不要留下（TS `buildWpsBox`）。
    pub fn has_paint(&self) -> bool {
        self.fill.as_ref().is_some_and(|f| f.kind != FillKind::None)
            || self.line.as_ref().is_some_and(|l| !l.no_fill && l.fill.is_some())
            || self.fill_ref.is_some()
    }
}

/// `spPr` 的填充种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillKind {
    /// `a:noFill`
    None,
    Solid,
    Gradient,
    Pattern,
    /// `a:blipFill`：图片填充。
    Blip,
    /// `a:grpFill`：继承所在 `wpg` 组的填充。
    Group,
}

/// 填充。颜色留原始定义（容器节点），解析成 sRGB 走 [`crate::resolve::drawingml`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillDisplay {
    pub node: NodeId,
    pub kind: FillKind,
    /// `a:blipFill/a:blip/@r:embed`。
    pub blip: Option<String>,
    /// `a:blipFill/a:tile`：平铺而非拉伸。
    pub tile: bool,
}

/// `a:fillRef` / `a:lnRef`：主题样式引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRef {
    pub node: NodeId,
    /// `@idx`：0 表示「无」。
    pub idx: Option<i64>,
}

/// `wps:bodyPr`：文字框的内边距与对齐。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BodyPr {
    /// `@lIns` / `@tIns` / `@rIns` / `@bIns`（EMU）。缺省值由投影层按 OOXML 补。
    pub l_ins: Option<i64>,
    pub t_ins: Option<i64>,
    pub r_ins: Option<i64>,
    pub b_ins: Option<i64>,
    /// `@anchor`：`t` / `ctr` / `b`。
    pub anchor: Option<Anchor>,
    /// 有 `a:spAutoFit`：框高随文字自适应。
    pub auto_fit: bool,
}

/// `wps:bodyPr/@anchor`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Center,
    Bottom,
}

impl DrawingDisplay {
    /// 段落级图片：第一个 `pic:pic`。
    pub fn picture(&self) -> Option<&ImageDisplay> {
        self.pictures.first()
    }
}

/// `wp:extent` / `a:ext`：EMU 宽高。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub cx: i64,
    pub cy: i64,
}

/// `wp:docPr`：无障碍与标识信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocPr {
    pub id: Option<String>,
    pub name: Option<String>,
    /// `@descr`：替代文字。
    pub descr: Option<String>,
    pub title: Option<String>,
    pub hidden: bool,
}

/// `wp:anchor` 的锚定几何。布尔属性的缺省值按 ECMA-376。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorGeom {
    pub node: NodeId,
    /// 绘在正文文字下面（只影响绘制次序，不等于不绕排）。
    pub behind_doc: bool,
    /// `@allowOverlap`，缺省 `true`。
    pub allow_overlap: bool,
    pub locked: bool,
    /// `@layoutInCell`，缺省 `true`。
    pub layout_in_cell: bool,
    pub simple_pos: bool,
    /// `@relativeHeight` 原值。z 序是它减去 Word 的基数，换算在投影层。
    pub relative_height: Option<i64>,
    /// `@distT/@distB/@distL/@distR`（EMU）。
    pub dist: Dist,
    pub h: Position,
    pub v: Position,
    pub wrap: Wrap,
}

/// 绕排边距（EMU）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dist {
    pub top: Option<i64>,
    pub bottom: Option<i64>,
    pub left: Option<i64>,
    pub right: Option<i64>,
}

/// `wp:positionH` / `wp:positionV`。三种定位写法互斥，但畸形文档可能都写，全都记下来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    /// `@relativeFrom`（`margin` / `page` / `column` / `paragraph` / `line` …）。
    pub relative_from: Option<String>,
    /// `wp:align` 的文本（`left` / `center` / `right` / `top` / `bottom` / `inside` / `outside`）。
    pub align: Option<String>,
    /// `wp:posOffset`（EMU）。
    pub offset_emu: Option<i64>,
    /// `wp14:pctPosHOffset` / `wp14:pctPosVOffset`（千分之一百分比原值）。
    pub pct: Option<i64>,
}

/// 绕排方式（`wp:wrap*`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrap {
    /// `wp:wrapNone`：不绕排，浮在文字上/下。
    None,
    /// `wp:wrapSquare`，`@wrapText` 说文字走哪一侧。
    Square {
        text: Option<String>,
    },
    Tight {
        text: Option<String>,
    },
    Through {
        text: Option<String>,
    },
    TopAndBottom,
    /// 随文（`wp:inline`），或 anchor 里没写绕排元素。
    Unspecified,
}

impl Wrap {
    /// `@wrapText`（`bothSides` / `left` / `right` / `largest`）。
    pub fn text(&self) -> Option<&str> {
        match self {
            Wrap::Square { text } | Wrap::Tight { text } | Wrap::Through { text } => {
                text.as_deref()
            }
            _ => None,
        }
    }
}

/// `pic:pic`：一张图片。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageDisplay {
    /// `pic:pic` 节点。
    pub node: Option<NodeId>,
    /// `pic:spPr/a:xfrm` 的 `a:off` / `a:ext`（EMU）。组里的图片靠它定位。
    pub off: Option<(i64, i64)>,
    pub ext: Option<Extent>,
    /// 所属 `wpg` 组在 `shapes` 里的下标。
    pub group: Option<usize>,
    /// `a:blip/@r:embed`：包内媒体的关系 id。
    pub embed: Option<String>,
    /// `a:blip/@r:link`：外链媒体的关系 id。
    pub link: Option<String>,
    /// `a:srcRect`：源图裁剪，四边各千分之一百分比。
    pub crop: Option<RectFrac>,
    /// `a:stretch/a:fillRect`：填充矩形。
    pub fill_rect: Option<RectFrac>,
    /// `pic:spPr/a:xfrm/@rot`，1/60000 度原值。
    pub rot_60k: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    /// `pic:spPr/a:ln`：图片边框。
    pub border: Option<LineDisplay>,
}

/// `a:srcRect` / `a:fillRect` 的四边，千分之一百分比原值（`10000` = 10%）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RectFrac {
    pub l: i64,
    pub t: i64,
    pub r: i64,
    pub b: i64,
}

impl RectFrac {
    pub fn is_zero(&self) -> bool {
        self.l == 0 && self.t == 0 && self.r == 0 && self.b == 0
    }
}

/// `a:ln`：线条。颜色留原始定义，解析成 sRGB 走 [`crate::resolve::drawingml`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDisplay {
    pub node: NodeId,
    /// `@w`（EMU）。
    pub width_emu: Option<i64>,
    /// `a:noFill` → 没有线。
    pub no_fill: bool,
    /// 颜色容器节点（`a:solidFill` 等），供 `resolve::drawingml::color_in`。
    pub fill: Option<NodeId>,
    /// `a:prstDash/@val`。
    pub dash: Option<String>,
    /// `a:headEnd/@type` / `a:tailEnd/@type`（`none` 视为没有箭头）。
    pub head_end: Option<String>,
    pub tail_end: Option<String>,
}

impl LineDisplay {
    /// 两端任一有箭头。
    pub fn arrowed(&self) -> bool {
        [&self.head_end, &self.tail_end].iter().any(|e| e.as_deref().is_some_and(|t| t != "none"))
    }
}

// ---- 解析 ---------------------------------------------------------------------------------------

/// 建一个 `w:drawing` 的显示模型。
pub fn drawing_display(dom: &Dom, drawing: NodeId) -> DrawingDisplay {
    let mut d = DrawingDisplay {
        node: drawing,
        kind: DrawingKind::Unknown,
        anchor: None,
        extent: None,
        doc_pr: DocPr::default(),
        pictures: Vec::new(),
        shapes: Vec::new(),
        chart: None,
    };
    let mut pic_nodes: Vec<(NodeId, Option<usize>)> = Vec::new();
    // 组的下标要在遍历时跟着走，所以这里用带父组的显式栈，而不是 `walk`。
    let mut stack: Vec<(NodeId, u32, Option<usize>)> = vec![(drawing, 0, None)];
    let mut scratch: Vec<NodeId> = Vec::new();
    while let Some((n, depth, parent)) = stack.pop() {
        let mut group = parent;
        if let Some(name) = dom.name(n) {
            match (eff_ns(dom, n), name.local) {
                (NsId::Wp, LocalName::Anchor) => d.anchor = Some(anchor_geom(dom, n)),
                (NsId::Wp, LocalName::Extent) if d.extent.is_none() => d.extent = extent_of(dom, n),
                (NsId::Wp, LocalName::DocPr) => d.doc_pr = doc_pr(dom, n),
                (NsId::A, LocalName::GraphicData) if d.kind == DrawingKind::Unknown => {
                    d.kind = crate::model::facts::graphic_data_kind(dom, n);
                }
                (NsId::Pic, LocalName::Pic) => pic_nodes.push((n, parent)),
                (ns @ (NsId::C | NsId::Cx), LocalName::Chart) if d.chart.is_none() => {
                    d.chart = Some(ChartRef {
                        node: n,
                        rel_id: attr(dom, n, NsId::R, LocalName::Id),
                        chartex: ns == NsId::Cx,
                    });
                }
                (NsId::Wps, LocalName::Wsp) => d.shapes.push(shape_display(dom, n, false, parent)),
                (NsId::Wpg, LocalName::Wgp | LocalName::GrpSp) => {
                    d.shapes.push(shape_display(dom, n, true, parent));
                    group = Some(d.shapes.len() - 1);
                }
                _ => {}
            }
        }
        if depth < MAX_DEPTH {
            scratch.clear();
            scratch.extend(dom.semantic_children(n).filter(|&c| !is_own_flow(dom, c)));
            stack.extend(scratch.iter().rev().map(|&c| (c, depth + 1, group)));
        }
    }
    for (pic, group) in pic_nodes {
        let mut img = image_display(dom, pic);
        img.group = group;
        d.pictures.push(img);
    }
    d
}

/// `wps:wsp` / `wpg:wgp` / `wpg:grpSp` → [`ShapeDisplay`]。只看形状自己的属性子树，
/// 不下钻进 `txbxContent`（框里的内容是独立内容流）。
fn shape_display(dom: &Dom, node: NodeId, is_group: bool, group: Option<usize>) -> ShapeDisplay {
    let mut s = ShapeDisplay {
        node,
        is_group,
        cnv_id: None,
        prst: None,
        cust_geom: false,
        geom: None,
        ext: None,
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
        content: Vec::new(),
        content_part: None,
        group,
    };
    // 只走形状自己的属性容器：`spPr` / `grpSpPr` / `style` / `bodyPr` / `txbx`。
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        match (eff_ns(dom, c), name.local) {
            (NsId::Wps, LocalName::SpPr) | (NsId::Wpg, LocalName::GrpSpPr) => sp_pr(dom, c, &mut s),
            (NsId::Wps, LocalName::Style) => {
                for r in dom.semantic_children(c) {
                    let Some(n) = dom.name(r) else { continue };
                    let sr = StyleRef { node: r, idx: num(dom, r, LocalName::Idx) };
                    match (n.ns, n.local) {
                        (NsId::A, LocalName::FillRef) => s.fill_ref = Some(sr),
                        (NsId::A, LocalName::LnRef) => s.line_ref = Some(sr),
                        (NsId::A, LocalName::FontRef) => s.font_ref = Some(sr),
                        _ => {}
                    }
                }
            }
            (NsId::Wps, LocalName::BodyPr) => s.body = Some(body_pr(dom, c)),
            (NsId::Wps, LocalName::CNvPr) => s.cnv_id = attr(dom, c, NsId::None, LocalName::Id),
            (NsId::Wps, LocalName::Txbx) => {
                s.txbx_rel =
                    dom.attr_value(c, QName::new(NsId::R, LocalName::Txbx)).map(|v| v.into_owned());
                s.txbx = dom
                    .semantic_children(c)
                    .find(|&t| dom.is(t, QName::new(NsId::W, LocalName::TxbxContent)));
            }
            _ => {}
        }
    }
    s
}

fn sp_pr(dom: &Dom, sp_pr: NodeId, s: &mut ShapeDisplay) {
    for c in dom.semantic_children(sp_pr) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::Xfrm => {
                s.rot_60k = num(dom, c, LocalName::Rot);
                s.flip_h = flag(dom, c, LocalName::FlipH).unwrap_or(false);
                s.flip_v = flag(dom, c, LocalName::FlipV).unwrap_or(false);
                for g in dom.semantic_children(c) {
                    let Some(gn) = dom.name(g) else { continue };
                    match gn.local {
                        LocalName::Off => s.off = xy(dom, g),
                        LocalName::Ext => s.ext = extent_of(dom, g),
                        LocalName::ChOff => s.ch_off = xy(dom, g),
                        LocalName::ChExt => s.ch_ext = extent_of(dom, g),
                        _ => {}
                    }
                }
            }
            LocalName::PrstGeom => s.prst = attr(dom, c, NsId::None, LocalName::Prst),
            LocalName::CustGeom => {
                s.cust_geom = true;
                s.geom = custom_geom(dom, c);
            }
            LocalName::GrpFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Group, blip: None, tile: false })
            }
            LocalName::NoFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::None, blip: None, tile: false })
            }
            LocalName::SolidFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Solid, blip: None, tile: false })
            }
            LocalName::GradFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Gradient, blip: None, tile: false })
            }
            LocalName::PattFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Pattern, blip: None, tile: false })
            }
            LocalName::BlipFill if s.fill.is_none() => {
                let mut blip = None;
                let mut tile = false;
                for b in dom.semantic_children(c) {
                    match dom.name(b).map(|n| n.local) {
                        Some(LocalName::Blip) => blip = attr(dom, b, NsId::R, LocalName::Embed),
                        Some(LocalName::Tile) => tile = true,
                        _ => {}
                    }
                }
                s.fill = Some(FillDisplay { node: c, kind: FillKind::Blip, blip, tile });
            }
            LocalName::Ln if s.line.is_none() => s.line = Some(line_display(dom, c)),
            LocalName::EffectLst => {
                s.has_effects = dom.semantic_children(c).next().is_some();
            }
            _ => {}
        }
    }
}

fn body_pr(dom: &Dom, node: NodeId) -> BodyPr {
    BodyPr {
        l_ins: num(dom, node, LocalName::LIns),
        t_ins: num(dom, node, LocalName::TIns),
        r_ins: num(dom, node, LocalName::RIns),
        b_ins: num(dom, node, LocalName::BIns),
        anchor: match attr(dom, node, NsId::None, LocalName::Anchor).as_deref() {
            Some("t") => Some(Anchor::Top),
            Some("ctr") => Some(Anchor::Center),
            Some("b") => Some(Anchor::Bottom),
            _ => None,
        },
        auto_fit: dom
            .semantic_children(node)
            .any(|c| dom.is(c, QName::new(NsId::A, LocalName::SpAutoFit))),
    }
}

fn xy(dom: &Dom, node: NodeId) -> Option<(i64, i64)> {
    Some((num(dom, node, LocalName::X)?, num(dom, node, LocalName::Y)?))
}

/// `pic:pic` → [`ImageDisplay`]。
pub fn image_display(dom: &Dom, pic: NodeId) -> ImageDisplay {
    let mut img = ImageDisplay { node: Some(pic), ..ImageDisplay::default() };
    let mut in_sp_pr = false;
    for n in walk(dom, pic) {
        let Some(name) = dom.name(n) else { continue };
        match (eff_ns(dom, n), name.local) {
            (NsId::Pic, LocalName::SpPr) => in_sp_pr = true,
            (NsId::A, LocalName::Blip) if img.embed.is_none() && img.link.is_none() => {
                img.embed = attr(dom, n, NsId::R, LocalName::Embed);
                img.link = attr(dom, n, NsId::R, LocalName::Link);
            }
            (NsId::A, LocalName::SrcRect) if img.crop.is_none() => {
                img.crop = Some(rect_frac(dom, n));
            }
            (NsId::A, LocalName::FillRect) if img.fill_rect.is_none() => {
                img.fill_rect = Some(rect_frac(dom, n));
            }
            // 旋转与翻转只认 `pic:spPr` 自己的 `a:xfrm`：锚定文本框兄弟有它自己的 `wps` xfrm。
            (NsId::A, LocalName::Xfrm) if in_sp_pr && img.rot_60k.is_none() => {
                img.rot_60k = num(dom, n, LocalName::Rot);
                img.flip_h = flag(dom, n, LocalName::FlipH).unwrap_or(false);
                img.flip_v = flag(dom, n, LocalName::FlipV).unwrap_or(false);
                for g in dom.semantic_children(n) {
                    match dom.name(g).map(|q| q.local) {
                        Some(LocalName::Off) => img.off = xy(dom, g),
                        Some(LocalName::Ext) => img.ext = extent_of(dom, g),
                        _ => {}
                    }
                }
            }
            (NsId::A, LocalName::Ln) if in_sp_pr && img.border.is_none() => {
                img.border = Some(line_display(dom, n));
            }
            _ => {}
        }
    }
    img
}

/// `a:ln` → [`LineDisplay`]。
pub fn line_display(dom: &Dom, ln: NodeId) -> LineDisplay {
    let mut l = LineDisplay {
        node: ln,
        width_emu: num(dom, ln, LocalName::W),
        no_fill: false,
        fill: None,
        dash: None,
        head_end: None,
        tail_end: None,
    };
    for c in dom.semantic_children(ln) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::NoFill => l.no_fill = true,
            LocalName::PrstDash => l.dash = attr(dom, c, NsId::None, LocalName::Val),
            // 注意是小写的 `type`（`LocalName::Type`）；大写 `Type` 是 `UType`
            LocalName::HeadEnd => l.head_end = attr(dom, c, NsId::None, LocalName::Type),
            LocalName::TailEnd => l.tail_end = attr(dom, c, NsId::None, LocalName::Type),
            LocalName::SolidFill | LocalName::GradFill | LocalName::PattFill
                if l.fill.is_none() =>
            {
                l.fill = Some(c);
            }
            _ => {}
        }
    }
    l
}

fn anchor_geom(dom: &Dom, anchor: NodeId) -> AnchorGeom {
    let mut g = AnchorGeom {
        node: anchor,
        behind_doc: flag(dom, anchor, LocalName::BehindDoc).unwrap_or(false),
        allow_overlap: flag(dom, anchor, LocalName::AllowOverlap).unwrap_or(true),
        locked: flag(dom, anchor, LocalName::Locked).unwrap_or(false),
        layout_in_cell: flag(dom, anchor, LocalName::LayoutInCell).unwrap_or(true),
        simple_pos: flag(dom, anchor, LocalName::SimplePos).unwrap_or(false),
        relative_height: num(dom, anchor, LocalName::RelativeHeight),
        dist: Dist {
            top: num(dom, anchor, LocalName::DistT),
            bottom: num(dom, anchor, LocalName::DistB),
            left: num(dom, anchor, LocalName::DistL),
            right: num(dom, anchor, LocalName::DistR),
        },
        h: Position::default(),
        v: Position::default(),
        wrap: Wrap::Unspecified,
    };
    // 只看 anchor 的直接子节点：位置与绕排是 anchor 自己的属性，图形内部的同名元素不算。
    for c in dom.semantic_children(anchor) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::Wp {
            continue;
        }
        match name.local {
            LocalName::PositionH => g.h = position(dom, c, LocalName::PctPosHOffset),
            LocalName::PositionV => g.v = position(dom, c, LocalName::PctPosVOffset),
            LocalName::WrapNone => g.wrap = Wrap::None,
            LocalName::WrapSquare => g.wrap = Wrap::Square { text: wrap_text(dom, c) },
            LocalName::WrapTight => g.wrap = Wrap::Tight { text: wrap_text(dom, c) },
            LocalName::WrapThrough => g.wrap = Wrap::Through { text: wrap_text(dom, c) },
            LocalName::WrapTopAndBottom => g.wrap = Wrap::TopAndBottom,
            _ => {}
        }
    }
    g
}

fn wrap_text(dom: &Dom, node: NodeId) -> Option<String> {
    attr(dom, node, NsId::None, LocalName::WrapText)
}

fn position(dom: &Dom, node: NodeId, pct: LocalName) -> Position {
    let mut p = Position {
        relative_from: attr(dom, node, NsId::None, LocalName::RelativeFrom),
        ..Position::default()
    };
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        match (name.ns, name.local) {
            (NsId::Wp, LocalName::Align) => p.align = text_of(dom, c),
            (NsId::Wp, LocalName::PosOffset) => {
                p.offset_emu = text_of(dom, c).and_then(|s| s.trim().parse().ok());
            }
            (NsId::Wp14, l) if l == pct => {
                p.pct = text_of(dom, c).and_then(|s| s.trim().parse().ok());
            }
            _ => {}
        }
    }
    p
}

fn doc_pr(dom: &Dom, node: NodeId) -> DocPr {
    DocPr {
        id: attr(dom, node, NsId::None, LocalName::Id),
        name: attr(dom, node, NsId::None, LocalName::Name),
        descr: attr(dom, node, NsId::None, LocalName::Descr),
        title: attr(dom, node, NsId::None, LocalName::Title),
        hidden: flag(dom, node, LocalName::Hidden).unwrap_or(false),
    }
}

fn extent_of(dom: &Dom, node: NodeId) -> Option<Extent> {
    Some(Extent { cx: num(dom, node, LocalName::Cx)?, cy: num(dom, node, LocalName::Cy)? })
}

fn rect_frac(dom: &Dom, node: NodeId) -> RectFrac {
    let side = |l: LocalName| num(dom, node, l).unwrap_or(0);
    RectFrac {
        l: side(LocalName::L),
        t: side(LocalName::T),
        r: side(LocalName::R),
        b: side(LocalName::B),
    }
}

fn attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.trim().to_string())
}

fn num(dom: &Dom, node: NodeId, local: LocalName) -> Option<i64> {
    attr(dom, node, NsId::None, local)?.parse().ok()
}

/// OOXML 布尔属性：`1` / `true` / `on` 为真，`0` / `false` / `off` 为假。
fn flag(dom: &Dom, node: NodeId, local: LocalName) -> Option<bool> {
    match attr(dom, node, NsId::None, local)?.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

fn text_of(dom: &Dom, node: NodeId) -> Option<String> {
    let mut s = String::new();
    for c in dom.semantic_children(node) {
        if let Some(t) = dom.text(c) {
            s.push_str(&t);
        }
    }
    (!s.is_empty()).then_some(s)
}

/// 绘图子树的语义前序遍历，遇到独立内容流（`w:txbxContent`）与嵌套 `w:drawing` 就不再下钻。
fn walk(dom: &Dom, root: NodeId) -> Walk<'_> {
    Walk { dom, stack: vec![(root, 0)], scratch: Vec::new() }
}

struct Walk<'a> {
    dom: &'a Dom,
    stack: Vec<(NodeId, u32)>,
    scratch: Vec<NodeId>,
}

impl Iterator for Walk<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let (id, depth) = self.stack.pop()?;
        if depth < MAX_DEPTH {
            let dom = self.dom;
            self.scratch.clear();
            self.scratch.extend(dom.semantic_children(id).filter(|&c| !is_own_flow(dom, c)));
            self.stack.extend(self.scratch.iter().rev().map(|&c| (c, depth + 1)));
        }
        Some(id)
    }
}

/// 该节点是否开启了一条独立内容流（不属于当前 drawing 的几何）。
/// 节点的**有效**命名空间：前缀绑不上时按字面量认。
///
/// 语料里有一批合成文档只在根上声明了 `w` / `wp` / `a` / `pic`，`wps` 与 `wpg` 一个都没声明
/// （`field-display__015`）。TS 用字符串匹配 `<wps:wsp`，压根不看声明；我们走 DOM，就得在
/// 这里补一条：绑不上的前缀按字面量认，其余照旧按 URI。
fn eff_ns(dom: &Dom, node: NodeId) -> NsId {
    let Some(name) = dom.name(node) else { return NsId::None };
    if !matches!(name.ns, NsId::Unbound(_)) {
        return name.ns;
    }
    match dom.lex_name(node).and_then(|q| q.split_once(':')).map(|(p, _)| p) {
        Some("wps") => NsId::Wps,
        Some("wpg") => NsId::Wpg,
        Some("wp") => NsId::Wp,
        Some("c") => NsId::C,
        Some("cx") => NsId::Cx,
        Some("pic") => NsId::Pic,
        Some("a") => NsId::A,
        _ => name.ns,
    }
}

fn is_own_flow(dom: &Dom, node: NodeId) -> bool {
    dom.name(node).is_some_and(|n| {
        n.ns == NsId::W && matches!(n.local, LocalName::TxbxContent | LocalName::Drawing)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const NS: &str = concat!(
        r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
        r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
        r#" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
        r#" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing""#,
        r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
        r#" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#,
    );

    fn parse(inner: &str) -> (Dom, DrawingDisplay) {
        let src = format!("<w:drawing{NS}>{inner}</w:drawing>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let d = drawing_display(&dom, root);
        (dom, d)
    }

    const PIC: &str = concat!(
        r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
        r#"<pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill></pic:pic>"#,
        r#"</a:graphicData></a:graphic>"#,
    );

    #[test]
    fn mod_11_inline_picture_facts() {
        let (_, d) = parse(&format!(
            r#"<wp:inline distT="0" distB="0"><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Logo" descr="a photo"/>{PIC}</wp:inline>"#
        ));
        assert_eq!(d.kind, DrawingKind::Picture);
        assert!(d.anchor.is_none(), "wp:inline 是随文，没有锚定几何");
        assert_eq!(d.extent, Some(Extent { cx: 914_400, cy: 457_200 }));
        assert_eq!(d.doc_pr.name.as_deref(), Some("Logo"));
        assert_eq!(d.doc_pr.descr.as_deref(), Some("a photo"));
        let p = d.picture().expect("picture").clone();
        assert_eq!(p.embed.as_deref(), Some("rId7"));
        assert!(p.link.is_none());
    }

    #[test]
    fn mod_11_anchor_geometry_defaults_and_values() {
        let (_, d) = parse(concat!(
            r#"<wp:anchor behindDoc="1" allowOverlap="0" distT="10" distB="20" distL="30" distR="40" relativeHeight="251658242">"#,
            r#"<wp:positionH relativeFrom="page"><wp:posOffset>-1270</wp:posOffset></wp:positionH>"#,
            r#"<wp:positionV relativeFrom="margin"><wp:align>center</wp:align><wp14:pctPosVOffset>25000</wp14:pctPosVOffset></wp:positionV>"#,
            r#"<wp:wrapSquare wrapText="left"/>"#,
            r#"<wp:extent cx="100" cy="200"/>"#,
            r#"</wp:anchor>"#,
        ));
        let a = d.anchor.expect("anchor");
        assert!(a.behind_doc);
        assert!(!a.allow_overlap, "allowOverlap=0");
        assert!(a.layout_in_cell, "没写 layoutInCell 时缺省为 true");
        assert!(!a.locked);
        assert_eq!(a.relative_height, Some(251_658_242));
        assert_eq!(
            a.dist,
            Dist { top: Some(10), bottom: Some(20), left: Some(30), right: Some(40) }
        );
        assert_eq!(a.h.relative_from.as_deref(), Some("page"));
        assert_eq!(a.h.offset_emu, Some(-1270));
        assert_eq!(a.v.relative_from.as_deref(), Some("margin"));
        assert_eq!(a.v.align.as_deref(), Some("center"));
        assert_eq!(a.v.pct, Some(25000));
        assert_eq!(a.wrap, Wrap::Square { text: Some("left".into()) });
        assert_eq!(a.wrap.text(), Some("left"));
    }

    #[test]
    fn mod_11_picture_crop_rotation_and_border() {
        let (_, d) = parse(concat!(
            r#"<wp:inline><wp:extent cx="100" cy="100"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic>"#,
            r#"<pic:blipFill><a:blip r:link="rId9"/><a:srcRect l="5000" b="10000"/>"#,
            r#"<a:stretch><a:fillRect t="1000"/></a:stretch></pic:blipFill>"#,
            r#"<pic:spPr><a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/></a:xfrm>"#,
            r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:prstDash val="dash"/></a:ln>"#,
            r#"</pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline>"#,
        ));
        let p = d.picture().expect("picture").clone();
        assert_eq!(p.link.as_deref(), Some("rId9"));
        assert!(p.embed.is_none());
        assert_eq!(p.crop, Some(RectFrac { l: 5000, t: 0, r: 0, b: 10000 }));
        assert_eq!(p.fill_rect, Some(RectFrac { l: 0, t: 1000, r: 0, b: 0 }));
        assert_eq!(p.rot_60k, Some(5_400_000));
        assert!(p.flip_h && !p.flip_v);
        let b = p.border.expect("border");
        assert_eq!(b.width_emu, Some(12700));
        assert!(!b.no_fill);
        assert_eq!(b.dash.as_deref(), Some("dash"));
        assert!(b.fill.is_some(), "颜色容器节点要留给 resolve::drawingml");
    }

    #[test]
    fn mod_11_textbox_content_is_not_this_drawings_geometry() {
        // 文本框里的图属于框内段落，不能被宿主 drawing 认领（`spec/15` 风险 3）。
        let (_, d) = parse(concat!(
            r#"<wp:anchor><wp:extent cx="100" cy="100"/><wp:docPr id="1" name="Box"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
            r#"<w:txbxContent><w:p><w:r><w:drawing><wp:inline><wp:extent cx="999" cy="888"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
            r#"<pic:pic><pic:blipFill><a:blip r:embed="rIdInner"/></pic:blipFill></pic:pic>"#,
            r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:txbxContent>"#,
            r#"</a:graphicData></a:graphic></wp:anchor>"#,
        ));
        assert_eq!(d.extent, Some(Extent { cx: 100, cy: 100 }), "取宿主的 extent");
        assert_eq!(d.kind, DrawingKind::Shape);
        assert!(d.picture().is_none(), "框里的 pic:pic 不属于宿主 drawing");
    }

    #[test]
    fn mod_11_deep_nesting_terminates() {
        // 恶意输入：深嵌套不能栈溢出，也不能死循环。
        let deep = format!("{}{}", "<a:grpSp>".repeat(500), "</a:grpSp>".repeat(500));
        let (_, d) = parse(&format!(r#"<wp:inline><wp:extent cx="1" cy="2"/>{deep}</wp:inline>"#));
        assert_eq!(d.extent, Some(Extent { cx: 1, cy: 2 }));
    }
}
