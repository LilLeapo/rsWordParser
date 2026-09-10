//! 显示模型（`MOD-11`）的 JSON 投影（`BIND-02`，`spec/21` 决策 4）。
//!
//! 这批类型只在 `document({ display: true })` 才出现在输出里，门控在调用方（`Segment.display`、
//! `ProtectedBlock.display` 等），表内**不再**做 display 门控——唯二的例外是 [`ChartPart`] 与
//! [`DiagramPart`]：它们在 `Document` 顶层、display 关闭时也投影，所以 `display` / `shapes` 行
//! 各自按 `cx.display` 门控。
//!
//! 几何留原值（EMU、1/60000 度、千分之一百分比），px 换算在 `bind/compat_ts`；媒体只投关系 id /
//! 外部 URL 字串，字节经 `media(id, mediaId)` 取（`BIND-05`）。

use serde_json::Value;

use crate::model::Block;
use crate::model::DrawingKind;
use crate::model::FormulaDisplay;
use crate::model::ThemeSlot;
use crate::model::chart::{
    ChartColor, ChartDisplay, ChartGrouping, ChartKind, ChartPart, ChartSeries, LegendPos,
};
use crate::model::vml::{OleInfo, VmlDisplay, VmlFill, VmlKind, VmlShape};
use crate::model::{
    Anchor, AnchorGeom, BodyPr, ChartRef, DiagramRef, Display, Dist, DocPr, DrawingDisplay, Extent,
    FillDisplay, FillKind, ImageDisplay, LineDisplay, Position, RectFrac, ShapeDisplay, StyleRef,
    Wrap,
};
use crate::model::{CanvasDisplay, DiagramLine, DiagramPart, DiagramPicture, DiagramShape};
use crate::model::{CustomGeom, GeomCmd, GeomPath};
use crate::package::PartId;
use crate::resolve::drawingml::{ColorBase, ColorTransform, DrawingColor, Rgb};
use crate::xml::NodeId;

use super::{ProjCx, SchemaDefs, ToJson, as_str_json, json_str_enum, model_json};

// ---- 定长数组的投影（容器 / 基础类型，手写；`spec/21`：区间与元组一律二元数组） ---------------------

/// `a:pt` 的路径坐标（`model::custgeom`）：`[x, y]`。
impl ToJson for [i64; 2] {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::Array(vec![Value::from(self[0]), Value::from(self[1])])
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::pair_schema(
            crate::bind::native::schema::int_schema(),
            crate::bind::native::schema::int_schema(),
        )
    }
}

/// 贝塞尔控制点序列（`a:quadBezTo` 2 点、`a:cubicBezTo` 3 点）：`[[x, y], …]`。
impl<const N: usize> ToJson for [[i64; 2]; N] {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(self.iter().map(|p| p.to_json(cx)).collect())
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::arr_schema(<[i64; 2] as ToJson>::schema(defs))
    }
}

/// `RES-05` 的 `Rgb`（sRGB 三通道，0–255 保留小数）：`[r, g, b]`。
impl ToJson for [f64; 3] {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::Array(self.iter().map(|c| Value::from(*c)).collect())
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::arr_schema(crate::bind::native::schema::num_schema())
    }
}

/// 调色板 `[Rgb; 6]`：`[[r, g, b], …]`。
impl<const N: usize> ToJson for [[f64; 3]; N] {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(self.iter().map(|c| c.to_json(cx)).collect())
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::arr_schema(<[f64; 3] as ToJson>::schema(defs))
    }
}

// ---- `named_enum!` / 手写 `as_str` 的枚举：`as_str` 字串即 JSON 值 -----------------------------------

as_str_json!(VmlKind, ChartKind, ChartGrouping, LegendPos);
// `DrawingKind` 的 impl 在 facts.rs、`ThemeSlot` 的在 theme.rs（归口一处，防重复 impl）。

// ---- 绘图（`MOD-11`；`spec/15` 任务 4.3）------------------------------------------------------------

model_json! {
    /// `Segment.display` / `ProtectedBlock.display` / `ImageBlock.display` 的显示载荷（`MOD-11`）。
    enum Display(cx) test json_fields_cover_display {
        /// `w:drawing`。
        Drawing(Box<DrawingDisplay>) => "drawing";
        /// `w:pict` / `w:object`（含 OLE 信息）。
        Vml(Box<VmlDisplay>) => "vml";
        /// 公式段落（R11）的 `m:oMath` 片段、token、MathML / LaTeX（M6 6.5）。
        Formula(Box<FormulaDisplay>) => "formula";
    }
}

model_json! {
    /// 一个 `w:drawing`（`MOD-11`；`spec/15` 任务 4.3）。
    struct DrawingDisplay(cx) test json_fields_cover_drawing_display {
        node => "node", NodeId = node;
        ~ kind => "drawingKind", DrawingKind = kind, "Display::Drawing 平铺时与枚举内标签键 kind 撞名";
        /// `wp:anchor` 的锚定几何；`wp:inline`（随文）→ 缺席。
        opt anchor => "anchor", AnchorGeom = anchor;
        /// `wp:extent`。
        opt extent => "extent", Extent = extent;
        doc_pr => "docPr", DocPr = doc_pr;
        /// 全部 `pic:pic`，文档序。
        pictures => "pictures", Vec<ImageDisplay> = pictures;
        /// `wps:wsp` 形状与 `wpg` 组，文档序；组内形状排在组之后，`group` 指回组。
        shapes => "shapes", Vec<ShapeDisplay> = shapes;
        /// `c:chart` / `cx:chart`：图表 part 的引用（M6 6.1）。
        opt chart => "chart", ChartRef = chart;
        /// `dgm:relIds`：SmartArt 两个 part 的引用（M6 6.3）。
        opt diagram => "diagram", DiagramRef = diagram;
        /// `lc:lockedCanvas`：绘图画布的子坐标系与形状（M6 6.3）。
        opt canvas => "canvas", Box<CanvasDisplay> = canvas;
    }
}

model_json! {
    /// `dgm:relIds`：一个绘图对 SmartArt part 的引用（M6 6.3）。
    struct DiagramRef(cx) test json_fields_cover_diagram_ref {
        node => "node", NodeId = node;
        /// `@r:dm`（数据 part）。
        opt rel_id => "relId", String = rel_id;
    }
}

model_json! {
    /// `c:chart r:id` / `cx:chart r:id`：一个绘图对图表 part 的引用（M6 6.1）。
    struct ChartRef(cx) test json_fields_cover_chart_ref {
        node => "node", NodeId = node;
        opt rel_id => "relId", String = rel_id;
        /// `cx:chart`（2014 chartex）。
        flag chartex => "chartex" = chartex;
    }
}

model_json! {
    /// 一个 `wps:wsp` 形状，或一个 `wpg:wgp` / `wpg:grpSp` 组（`MOD-11`）。
    struct ShapeDisplay(cx) test json_fields_cover_shape_display {
        node => "node", NodeId = node;
        /// 组本身（`wpg`）不画东西，只提供子坐标系。
        flag is_group => "isGroup" = is_group;
        /// `wps:cNvPr/@id`。
        opt cnv_id => "cnvId", String = cnv_id;
        /// `a:prstGeom/@prst`。
        opt prst => "prst", String = prst;
        /// 有 `a:custGeom`：自定义路径几何。
        flag cust_geom => "custGeom" = cust_geom;
        /// `a:custGeom` 的路径；用到公式或圆弧时缺席（`model::custgeom`）。
        opt geom => "geom", CustomGeom = geom;
        /// `a:xfrm/a:ext`。
        opt ext => "ext", Extent = ext;
        /// `a:xfrm/a:off`。
        opt off => "off", (i64, i64) = off;
        /// 组的子坐标系原点与尺寸（`a:chOff` / `a:chExt`）。
        opt ch_off => "chOff", (i64, i64) = ch_off;
        opt ch_ext => "chExt", Extent = ch_ext;
        /// `a:xfrm/@rot`。
        ~ opt rot_60k => "rot60k", i64 = rot_60k, "camel 规则把 `60k` 的首字符当字母大写会得到 rot6k；保留 `60k`（1/60000 度）的可读写法";
        flag flip_h => "flipH" = flip_h;
        flag flip_v => "flipV" = flip_v;
        /// `spPr` 的填充。
        opt fill => "fill", FillDisplay = fill;
        /// `spPr/a:ln`。
        opt line => "line", LineDisplay = line;
        /// `wps:style/a:fillRef` / `a:lnRef`：主题引用。
        opt fill_ref => "fillRef", StyleRef = fill_ref;
        opt line_ref => "lineRef", StyleRef = line_ref;
        /// `wps:style/a:fontRef`：图库形状的文字颜色出处。
        opt font_ref => "fontRef", StyleRef = font_ref;
        /// `spPr/a:effectLst` 里有内容（阴影等）。
        flag has_effects => "hasEffects" = has_effects;
        /// `wps:bodyPr`。
        opt body => "body", BodyPr = body;
        /// `wps:txbx/w:txbxContent`：框里的独立内容流。
        opt txbx => "txbx", NodeId = txbx;
        /// `wps:txbx/@r:txbx`：框的内容在另一个 part 里（与 `txbx` 互斥）。
        opt txbx_rel => "txbxRel", String = txbx_rel;
        /// 框里内容流的块（`Block` 的投影在块域）。
        content => "content", Vec<Block> = content;
        /// `content` 里的 `NodeId` 属于哪个 part；缺席 = 本 part。
        opt content_part => "contentPart", PartId = content_part;
        /// 所属组在 `shapes` 里的下标。
        opt group => "group", usize = group;
    }
}

json_str_enum! {
    /// `spPr` 的填充种类。
    FillKind test json_fields_cover_fill_kind {
        /// `a:noFill`。
        None => "none";
        Solid => "solid";
        Gradient => "gradient";
        Pattern => "pattern";
        /// `a:blipFill`：图片填充。
        Blip => "blip";
        /// `a:grpFill`：继承所在 `wpg` 组的填充。
        Group => "group";
    }
}

model_json! {
    /// 填充（`MOD-11`）。颜色留原始定义（容器节点），解析成 sRGB 走 `resolve::drawingml`。
    struct FillDisplay(cx) test json_fields_cover_fill_display {
        node => "node", NodeId = node;
        kind => "kind", FillKind = kind;
        /// `a:blipFill/a:blip/@r:embed`。
        opt blip => "blip", String = blip;
        /// `a:blipFill/a:tile`：平铺而非拉伸。
        flag tile => "tile" = tile;
    }
}

model_json! {
    /// `a:fillRef` / `a:lnRef` / `a:fontRef`：主题样式引用。
    struct StyleRef(cx) test json_fields_cover_style_ref {
        node => "node", NodeId = node;
        /// `@idx`：0 表示「无」。
        opt idx => "idx", i64 = idx;
    }
}

model_json! {
    /// `wps:bodyPr`：文字框的内边距与对齐。
    struct BodyPr(cx) test json_fields_cover_body_pr {
        opt l_ins => "lIns", i64 = l_ins;
        opt t_ins => "tIns", i64 = t_ins;
        opt r_ins => "rIns", i64 = r_ins;
        opt b_ins => "bIns", i64 = b_ins;
        /// `@anchor`：`t` / `ctr` / `b`。
        opt anchor => "anchor", Anchor = anchor;
        /// 有 `a:spAutoFit`：框高随文字自适应。
        flag auto_fit => "autoFit" = auto_fit;
    }
}

json_str_enum! {
    /// `wps:bodyPr/@anchor`。
    Anchor test json_fields_cover_anchor {
        Top => "top";
        Center => "center";
        Bottom => "bottom";
    }
}

model_json! {
    /// `wp:extent` / `a:ext`：宽 `cx`、高 `cy`。
    // 字段名 `cx` 会遮蔽上下文参数，所以这张表的上下文参数改名 `pcx`。
    struct Extent(pcx) test json_fields_cover_extent {
        cx => "cx", i64 = cx;
        cy => "cy", i64 = cy;
    }
}

model_json! {
    /// `wp:docPr`：无障碍与标识信息。
    struct DocPr(cx) test json_fields_cover_doc_pr {
        opt id => "id", String = id;
        opt name => "name", String = name;
        /// `@descr`：替代文字。
        opt descr => "descr", String = descr;
        opt title => "title", String = title;
        flag hidden => "hidden" = hidden;
    }
}

model_json! {
    /// `wp:anchor` 的锚定几何（`MOD-11`）。布尔属性的缺省值按 ECMA-376。
    struct AnchorGeom(cx) test json_fields_cover_anchor_geom {
        node => "node", NodeId = node;
        /// 绘在正文文字下面（只影响绘制次序，不等于不绕排）。
        flag behind_doc => "behindDoc" = behind_doc;
        flag allow_overlap => "allowOverlap" = allow_overlap;
        flag locked => "locked" = locked;
        flag layout_in_cell => "layoutInCell" = layout_in_cell;
        flag simple_pos => "simplePos" = simple_pos;
        /// `@relativeHeight` 原值。
        opt relative_height => "relativeHeight", i64 = relative_height;
        /// `@distT/@distB/@distL/@distR`。
        dist => "dist", Dist = dist;
        h => "h", Position = h;
        v => "v", Position = v;
        wrap => "wrap", Wrap = wrap;
    }
}

model_json! {
    /// 绕排边距。
    struct Dist(cx) test json_fields_cover_dist {
        opt top => "top", i64 = top;
        opt bottom => "bottom", i64 = bottom;
        opt left => "left", i64 = left;
        opt right => "right", i64 = right;
    }
}

model_json! {
    /// `wp:positionH` / `wp:positionV`。三种定位写法互斥，但畸形文档可能都写，全都记下来。
    struct Position(cx) test json_fields_cover_position {
        /// `@relativeFrom`（`margin` / `page` / `column` / `paragraph` / `line` …）。
        opt relative_from => "relativeFrom", String = relative_from;
        /// `wp:align` 的文本。
        opt align => "align", String = align;
        /// `wp:posOffset`。
        opt offset_emu => "offsetEmu", i64 = offset_emu;
        /// `wp14:pctPosHOffset` / `wp14:pctPosVOffset`（千分之一百分比原值）。
        opt pct => "pct", i64 = pct;
    }
}

model_json! {
    /// 绕排方式（`wp:wrap*`）。
    enum Wrap(cx) test json_fields_cover_wrap {
        /// `wp:wrapNone`：不绕排，浮在文字上/下。
        None => "none";
        /// `wp:wrapSquare`，`@wrapText` 说文字走哪一侧。
        Square { text } => "square" {
            opt text => "text", String = text;
        };
        Tight { text } => "tight" {
            opt text => "text", String = text;
        };
        Through { text } => "through" {
            opt text => "text", String = text;
        };
        TopAndBottom => "topAndBottom";
        /// 随文（`wp:inline`），或 anchor 里没写绕排元素。
        Unspecified => "unspecified";
    }
}

model_json! {
    /// `pic:pic`：一张图片（`MOD-11`）。
    struct ImageDisplay(cx) test json_fields_cover_image_display {
        /// `pic:pic` 节点。
        opt node => "node", NodeId = node;
        /// `pic:spPr/a:xfrm` 的 `a:off`；组里的图片靠它定位。
        opt off => "off", (i64, i64) = off;
        opt ext => "ext", Extent = ext;
        /// 所属 `wpg` 组在 `shapes` 里的下标。
        opt group => "group", usize = group;
        /// `a:blip/@r:embed`：包内媒体的关系 id（字节不进 JSON，`BIND-05`）。
        opt embed => "embed", String = embed;
        /// `a:blip/@r:link`：外链媒体的关系 id。
        opt link => "link", String = link;
        /// `a:srcRect`：源图裁剪，四边各千分之一百分比。
        opt crop => "crop", RectFrac = crop;
        /// `a:stretch/a:fillRect`：填充矩形。
        opt fill_rect => "fillRect", RectFrac = fill_rect;
        /// `pic:spPr/a:xfrm/@rot`。
        ~ opt rot_60k => "rot60k", i64 = rot_60k, "camel 规则把 `60k` 的首字符当字母大写会得到 rot6k；保留 `60k`（1/60000 度）的可读写法";
        flag flip_h => "flipH" = flip_h;
        flag flip_v => "flipV" = flip_v;
        /// `pic:spPr/a:ln`：图片边框。
        opt border => "border", LineDisplay = border;
    }
}

model_json! {
    /// `a:srcRect` / `a:fillRect` 的四边，千分之一百分比原值（`10000` = 10%）。
    struct RectFrac(cx) test json_fields_cover_rect_frac {
        l => "l", i64 = l;
        t => "t", i64 = t;
        r => "r", i64 = r;
        b => "b", i64 = b;
    }
}

model_json! {
    /// `a:ln`：线条（`MOD-11`）。颜色留原始定义，解析成 sRGB 走 `resolve::drawingml`。
    struct LineDisplay(cx) test json_fields_cover_line_display {
        node => "node", NodeId = node;
        /// `@w`。
        opt width_emu => "widthEmu", i64 = width_emu;
        /// `a:noFill` → 没有线。
        flag no_fill => "noFill" = no_fill;
        /// 颜色容器节点（`a:solidFill` 等），供 `resolve::drawingml::color_in`。
        opt fill => "fill", NodeId = fill;
        /// `a:prstDash/@val`。
        opt dash => "dash", String = dash;
        /// `a:headEnd/@type` / `a:tailEnd/@type`（`none` 视为没有箭头）。
        opt head_end => "headEnd", String = head_end;
        opt tail_end => "tailEnd", String = tail_end;
    }
}

// ---- 自定义几何（`MOD-11`；`spec/15` 任务 4.6d，`ShapeDisplay::geom` 引用） ----------------------------

model_json! {
    /// 一个 `a:custGeom` 的全部路径。
    struct CustomGeom(cx) test json_fields_cover_custom_geom {
        node => "node", NodeId = node;
        paths => "paths", Vec<GeomPath> = paths;
    }
}

model_json! {
    /// 一条 `a:path`。坐标在这条路径自己的坐标系里（`@w` / `@h`）。
    struct GeomPath(cx) test json_fields_cover_geom_path {
        /// `@w` / `@h`：这条路径的坐标空间；缺省时用形状的 `a:ext`。
        opt w => "w", i64 = w;
        opt h => "h", i64 = h;
        /// `@fill="none"`。
        flag fill_none => "fillNone" = fill_none;
        /// `@stroke="0" | "false" | "none"`。
        flag stroke_none => "strokeNone" = stroke_none;
        cmds => "cmds", Vec<GeomCmd> = cmds;
    }
}

model_json! {
    /// 路径命令。点是路径坐标系里的绝对坐标。
    enum GeomCmd(cx) test json_fields_cover_geom_cmd {
        MoveTo([i64; 2]) as "point" => "moveTo";
        LineTo([i64; 2]) as "point" => "lineTo";
        /// `a:quadBezTo`：控制点 + 终点。
        QuadTo([[i64; 2]; 2]) as "points" => "quadTo";
        /// `a:cubicBezTo`：两个控制点 + 终点。
        CubicTo([[i64; 2]; 3]) as "points" => "cubicTo";
        Close => "close";
    }
}

// ---- VML（`MOD-11`；`spec/15` 任务 4.5 / 4.7）--------------------------------------------------------

model_json! {
    /// 一个 `w:pict` / `w:object` 的 VML 内容（`MOD-11`）。
    struct VmlDisplay(cx) test json_fields_cover_vml_display {
        /// `w:pict` 或 `w:object` 节点。
        node => "node", NodeId = node;
        /// 文档序的形状表；组内形状排在组之后，`parent` 指回组。
        shapes => "shapes", Vec<VmlShape> = shapes;
        /// `w:object` 的嵌入对象信息。
        opt ole => "ole", OleInfo = ole;
        /// 框套得比 `MAX_BOX_NESTING` 还深，摊平表在那一层截断（`MOD_TOO_DEEP`）。
        flag too_deep => "tooDeep" = too_deep;
    }
}

model_json! {
    /// 一个 VML 形状（`MOD-11`）。`style` 键值对原样保留，不做语义解释。
    struct VmlShape(cx) test json_fields_cover_vml_shape {
        node => "node", NodeId = node;
        ~ kind => "vmlKind", VmlKind = kind, "Display::Vml 平铺时与枚举内标签键 kind 撞名";
        /// `@style` 的键值对：`[[键, 值], …]`，键小写、值原样。
        style => "style", Vec<(String, String)> = style;
        /// `@fillcolor`，归一成 6 位 hex；认不出的颜色写法 → 缺席。
        opt fill_color => "fillColor", String = fill_color;
        /// `@filled`（`f` / `false` 为假）。
        opt filled => "filled", bool = filled;
        opt stroke_color => "strokeColor", String = stroke_color;
        opt stroked => "stroked", bool = stroked;
        /// `@coordsize`：组的子坐标系尺寸。
        opt coordsize => "coordsize", (i64, i64) = coordsize;
        /// `@coordorigin`：组子坐标系的原点。
        opt coordorigin => "coordorigin", (i64, i64) = coordorigin;
        /// `@path`：VML 自定义路径。
        opt path => "path", String = path;
        /// `@type`：引用 `v:shapetype` 的 id。
        opt shape_type => "shapeType", String = shape_type;
        /// `@o:spt`：形状类型号。
        opt spt => "spt", String = spt;
        /// `v:imagedata/@r:id`（字节不进 JSON，`BIND-05`）。
        opt imagedata => "imagedata", String = imagedata;
        /// `v:textpath/@string`（WordArt 文字）。
        opt textpath => "textpath", String = textpath;
        /// `v:textpath/@style`：WordArt 的字体与字号。
        opt textpath_style => "textpathStyle", String = textpath_style;
        /// `@strokeweight`（多半带 `pt`）。
        opt stroke_weight => "strokeWeight", String = stroke_weight;
        /// `v:fill` 元素上的颜色。WordArt 拿它当文字颜色。
        opt fill => "fill", VmlFill = fill;
        /// `@o:hr="t"`：HTML `<hr>` 导入的细横线。
        flag hr => "hr" = hr;
        /// 直接挂着 `v:textbox`。
        flag has_textbox => "hasTextbox" = has_textbox;
        /// `v:textbox/w:txbxContent`：框里的独立内容流。
        opt txbx => "txbx", NodeId = txbx;
        /// 框里内容流的块（`Block` 的投影在块域）。
        content => "content", Vec<Block> = content;
        /// 所属 `v:group` 在 `shapes` 里的下标。
        opt parent => "parent", usize = parent;
        /// 这个形状躺在别的形状的 `w:txbxContent` 里。
        flag nested => "nested" = nested;
    }
}

model_json! {
    /// `v:fill` 元素上的颜色。
    struct VmlFill(cx) test json_fields_cover_vml_fill {
        opt color => "color", String = color;
        opt color2 => "color2", String = color2;
    }
}

model_json! {
    /// `w:object` 的嵌入对象（`MOD-11`；`spec/15` 任务 4.7）。
    struct OleInfo(cx) test json_fields_cover_ole_info {
        /// `o:OLEObject` 节点。
        node => "node", NodeId = node;
        /// `@ProgID`：`Excel.Sheet.12` 等。
        opt prog_id => "progId", String = prog_id;
        /// `@Type`：`Embed` / `Link`。
        opt kind => "kind", String = kind;
        /// `@DrawAspect`：`Content` / `Icon`。
        opt draw_aspect => "drawAspect", String = draw_aspect;
        /// `@r:id`：内嵌二进制 part 或外链的关系（字节不进 JSON，`BIND-05`）。
        opt rel_id => "relId", String = rel_id;
        /// `w:object/@w:dxaOrig`，预览图的声明宽度。
        opt dxa_orig => "dxaOrig", i64 = dxa_orig;
        opt dya_orig => "dyaOrig", i64 = dya_orig;
    }
}

// ---- 图表（`MOD-11`；`spec/17` 任务 6.1）--------------------------------------------------------------

model_json! {
    /// 一个图表里的颜色（`MOD-11`）：原始定义（`RES-05` 的 `DrawingColor`）与按文档配色方案
    /// 解出的 sRGB。
    struct ChartColor(cx) test json_fields_cover_chart_color {
        def => "def", DrawingColor = def;
        /// 解不出（未知槽位、主题里缺）→ 缺席，按「没写颜色」处理。
        opt rgb => "rgb", Rgb = rgb;
    }
}

model_json! {
    /// 一个 DrawingML 颜色（`RES-05`，经 `ChartColor` 引用）：底色 + 按文档顺序排列的变换。
    struct DrawingColor(cx) test json_fields_cover_drawing_color {
        node => "node", NodeId = node;
        base => "base", ColorBase = base;
        transforms => "transforms", Vec<ColorTransform> = transforms;
    }
}

model_json! {
    /// 颜色元素的底色定义（`RES-05`）。
    enum ColorBase(cx) test json_fields_cover_color_base {
        /// `a:srgbClr/@val`。
        Srgb([u8; 3]) as "rgb" => "srgb";
        /// `a:sysClr`：Word 会把解析后的系统色写进 `lastClr`。
        Sys { val, last } => "sys" {
            opt val => "val", String = val;
            opt last => "last", [u8; 3] = last;
        };
        /// `a:prstClr/@val`。
        Prst { name, rgb } => "prst" {
            name => "name", String = name;
            opt rgb => "rgb", [u8; 3] = rgb;
        };
        /// `a:schemeClr/@val`（含别名 `tx1/bg1/tx2/bg2`）。
        Scheme { name, slot } => "scheme" {
            name => "name", String = name;
            opt slot => "slot", ThemeSlot = slot;
        };
        /// `a:scrgbClr`：三个百分比。
        Scrgb(Rgb) as "rgb" => "scrgb";
        /// `a:hslClr`：`hue` 是 1/60000 度，`sat` / `lum` 是千分之一百分比。
        Hsl { hue, sat, lum } => "hsl" {
            hue => "hue", f64 = hue;
            sat => "sat", f64 = sat;
            lum => "lum", f64 = lum;
        };
    }
}

model_json! {
    /// 颜色变换子元素（`RES-05`）。数值已归一（百分比 → 0.0–1.0，色相 → 度）。
    enum ColorTransform(cx) test json_fields_cover_color_transform {
        LumMod(f64) as "value" => "lumMod";
        LumOff(f64) as "value" => "lumOff";
        Shade(f64) as "value" => "shade";
        Tint(f64) as "value" => "tint";
        SatMod(f64) as "value" => "satMod";
        HueMod(f64) as "value" => "hueMod";
        /// 不参与 sRGB 计算，只记录。
        Alpha(f64) as "value" => "alpha";
    }
}

model_json! {
    /// 一个系列（`c:ser` / `cx:series`，`MOD-11` 任务 6.1）。
    struct ChartSeries(cx) test json_fields_cover_chart_series {
        node => "node", NodeId = node;
        /// `c:tx`：字面 `c:v` 或缓存的第一个点。
        opt name => "name", String = name;
        /// `c:val` / `c:yVal` 缓存；非数字或缺点 → `null`（画成空档）。
        values => "values", Vec<Option<f64>> = values;
        /// `c:spPr/a:solidFill`。
        opt color => "color", ChartColor = color;
        /// `c:dPt` 逐点填充，按 `c:idx` 稀疏。
        opt point_colors => "pointColors", Vec<Option<ChartColor>> = point_colors;
        /// scatter / bubble：`c:xVal` 缓存。
        opt x_values => "xValues", Vec<Option<f64>> = x_values;
        /// bubble：`c:bubbleSize` 缓存。
        opt sizes => "sizes", Vec<Option<f64>> = sizes;
        /// scatter：点之间连线。
        flag line => "line" = line;
    }
}

model_json! {
    /// 一个图表 part 的显示模型（`MOD-11` 任务 6.1；几何在宿主 `DrawingDisplay` 上）。
    struct ChartDisplay(cx) test json_fields_cover_chart_display {
        /// `c:chartSpace` / `cx:chartSpace`。
        root => "root", NodeId = root;
        kind => "kind", ChartKind = kind;
        /// 绘图区里第一个图表元素（组合图以它为准）；chartex 没有。
        opt plot => "plot", NodeId = plot;
        /// bar：`c:barDir val="bar"`（水平条形）。
        flag horizontal => "horizontal" = horizontal;
        opt grouping => "grouping", ChartGrouping = grouping;
        /// line / scatter 的标记。
        flag markers => "markers" = markers;
        /// doughnut：`c:holeSize`。
        opt hole_pct => "holePct", u32 = hole_pct;
        opt legend_pos => "legendPos", LegendPos = legend_pos;
        /// 标题文字。
        opt title => "title", String = title;
        /// `c:title` 节点（6.6 改标题的落点）。
        opt title_node => "titleNode", NodeId = title_node;
        /// 类别文本。
        categories => "categories", Vec<String> = categories;
        series => "series", Vec<ChartSeries> = series;
        /// `c:style/@val`（1–48）。
        opt style_val => "styleVal", u8 = style_val;
        /// 系列颜色循环（`c:style` 的列与主题 accent 的纯函数）。
        opt palette => "palette", [Rgb; 6] = palette;
        /// `cx:chartSpace`：只有降级读法，`SetChartData` 不接受。
        flag chartex => "chartex" = chartex;
    }
}

model_json! {
    /// 主 part 引用的一个图表 part（`MOD-11` 任务 6.1）。本类型在 `Document` 顶层、display
    /// 关闭时也投影，所以 `display` 行自己按 `cx.display` 门控（决策 4）。
    struct ChartPart(cx) test json_fields_cover_chart_part {
        part => "part", PartId = part;
        /// part 解析不了（`Opaque`）时缺席。
        opt root => "root", NodeId = root;
        flag chartex => "chartex" = chartex;
        /// 没有带缓存值的系列 → 缺席（记 `CHART_NO_SERIES`）。
        raw opt display => "display", ChartDisplay = display.as_ref().filter(|_| cx.display).map(|d| d.to_json(cx));
    }
}

// ---- SmartArt 与绘图画布（`MOD-11`；`spec/17` 任务 6.3）------------------------------------------------

model_json! {
    /// 一个形状（`MOD-11` 任务 6.3）：绘图 part 的 `dsp:sp`，或画布里的 `a:sp` / `a:pic`。
    struct DiagramShape(cx) test json_fields_cover_diagram_shape {
        node => "node", NodeId = node;
        /// `a:xfrm/a:off`。
        off_emu => "offEmu", (i64, i64) = off_emu;
        /// `a:xfrm/a:ext`。连线（`prst=line` / `*Connector*`）可以一边为 0。
        ext_emu => "extEmu", Extent = ext_emu;
        /// `a:xfrm/@rot`。
        ~ opt rot_60k => "rot60k", i64 = rot_60k, "camel 规则把 `60k` 的首字符当字母大写会得到 rot6k；保留 `60k`（1/60000 度）的可读写法";
        /// `a:prstGeom/@prst`。
        opt prst => "prst", String = prst;
        /// `a:noFill`。
        flag no_fill => "noFill" = no_fill;
        /// `a:solidFill` 容器节点（颜色原始定义，属于形状所在的 part）。
        opt fill => "fill", NodeId = fill;
        /// `a:gradFill` 节点。
        opt gradient => "gradient", NodeId = gradient;
        /// `a:ln`（有 `a:noFill` 的线不记）。
        opt line => "line", DiagramLine = line;
        /// `a:blipFill`。
        opt picture => "picture", DiagramPicture = picture;
        /// `txBody` 各段文字（`a:r/a:t` 拼接，空段不记）。
        texts => "texts", Vec<String> = texts;
        /// 第一个带 `sz` 的 `a:rPr`：字号，1/100 pt 原值。
        opt font_size_100pt => "fontSize100pt", i64 = font_size_100pt;
        /// 同一轮里读到的 `a:rPr/a:solidFill` 容器节点。
        opt text_color => "textColor", NodeId = text_color;
    }
}

model_json! {
    /// `a:ln`（`MOD-11` 任务 6.3）：颜色容器（`a:solidFill`）与 `@w`。
    struct DiagramLine(cx) test json_fields_cover_diagram_line {
        node => "node", NodeId = node;
        opt color => "color", NodeId = color;
        opt width_emu => "widthEmu", i64 = width_emu;
    }
}

model_json! {
    /// 图片填充（`MOD-11` 任务 6.3）：`a:blip/@r:embed`（按所在 part 的关系解）与
    /// `a:stretch/a:fillRect`。
    struct DiagramPicture(cx) test json_fields_cover_diagram_picture {
        opt embed => "embed", String = embed;
        opt fill_rect => "fillRect", RectFrac = fill_rect;
    }
}

model_json! {
    /// 一个 `lc:lockedCanvas`（`MOD-11` 任务 6.3）：子坐标系与直接子元素里的 `a:sp` / `a:pic`。
    struct CanvasDisplay(cx) test json_fields_cover_canvas_display {
        node => "node", NodeId = node;
        /// `a:grpSpPr/a:xfrm/a:chOff`（缺省 0,0）。
        opt ch_off => "chOff", (i64, i64) = ch_off;
        /// `a:grpSpPr/a:xfrm/a:chExt`（缺省 = 宿主 `wp:extent`）。
        opt ch_ext => "chExt", Extent = ch_ext;
        shapes => "shapes", Vec<DiagramShape> = shapes;
    }
}

model_json! {
    /// 主 part 引用的一个 SmartArt（`MOD-11` 任务 6.3）：数据 part 与（可能没有的）绘图 part。
    /// 本类型在 `Document` 顶层、display 关闭时也投影，所以 `shapes` 行自己按 `cx.display`
    /// 门控（决策 4）。
    struct DiagramPart(cx) test json_fields_cover_diagram_part {
        /// 数据 part（`dgm:dataModel`）。
        data => "data", PartId = data;
        /// 绘图 part（`dsp:drawing`）。
        opt drawing => "drawing", PartId = drawing;
        /// 数据 part 的节点文字（树序，`\n` 连接）。
        opt text => "text", String = text;
        /// 绘图 part 的形状；part 缺失 / 解析不了 / 一个形状都没有 → 缺席。
        raw opt shapes => "shapes", Vec<DiagramShape> = shapes.as_ref().filter(|_| cx.display).map(|s| s.to_json(cx));
    }
}

// ---- 公式（`MOD-11`；`spec/17` 任务 6.5）--------------------------------------------------------------

model_json! {
    /// 一个公式段落的显示模型（`MOD-11` 任务 6.5）。
    struct FormulaDisplay(cx) test json_fields_cover_formula_display {
        /// 段落里的 `m:oMath`，文档序（`m:oMathPara` 展开）。
        fragments => "fragments", Vec<NodeId> = fragments;
        /// 全部 `m:t` 文本，文档序：可编辑的 token 串。
        tokens => "tokens", Vec<String> = tokens;
        /// MathML Core，各片段拼接。
        opt mathml => "mathml", String = mathml;
        /// LaTeX 子集：只有一个片段、且全在子集之内才有。
        opt latex => "latex", String = latex;
    }
}
