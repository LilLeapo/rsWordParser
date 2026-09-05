//! VML 显示模型（`MOD-11`；`spec/15` 任务 4.5 / 4.7）。
//!
//! `w:pict` 与 `w:object` 里装的都是 VML：形状把几何写在 `style` 属性里（CSS 语法），填充与描边
//! 写在 `fillcolor` / `strokecolor` 上。`MOD-11` 要求 `style` **原样保留**，所以这里不做语义解释，
//! 只把键值对拆出来；要用的地方自己按 [`crate::model::units::parse_length`] 取长度。
//!
//! 组（`v:group`）用 `coordsize` 定义子坐标系，子形状的 `style` 里是组坐标不是绝对长度。这里
//! 只记录关系（`VmlShape::parent`）与 `coordsize`，缩放留给用的人（4.6 的文本框投影）。
//!
//! 遍历是迭代的、带深度上限，和绘图那边同一条规矩。

use crate::model::block::Block;
use crate::model::macros::named_enum;
use crate::model::units::{Length, parse_length, parse_style};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// VML 子树的深度上限，与绘图同值。
const MAX_DEPTH: u32 = 64;

/// 摊平表最多穿几层**框**（`w:txbxContent`）。
///
/// `vml_display` 把整棵 `w:pict` 里的形状摊平成一张表，别人框里的形状也在表里（compat 要按
/// TS 的形态把它们当只读的兄弟框输出）。表里每一层都会重复列出更深的层，所以**表的规模**随
/// 嵌套层数是 O(n²)；语料里框套框最多 2 层（`textbox-edit__012`），Word 的界面根本做不出更深的。
/// 超过这个数的层不进表（内容仍在 DOM 里，`too_deep` 记 `MOD_TOO_DEEP`），
/// 这样 `corpus/hostile/hf-deep-txbx.docx` 那种 3000 层套娃不会把投影拖死。
pub(crate) const MAX_BOX_NESTING: u32 = 8;

/// 一个 `w:pict` / `w:object` 的 VML 内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmlDisplay {
    /// `w:pict` 或 `w:object` 节点。
    pub node: NodeId,
    /// 文档序的形状表；组内形状排在组之后，`parent` 指回组。
    pub shapes: Vec<VmlShape>,
    /// `w:object` 的嵌入对象信息。
    pub ole: Option<OleInfo>,
    /// 框套得比 [`MAX_BOX_NESTING`] 还深，摊平表在那一层截断（`MOD_TOO_DEEP`）。
    pub too_deep: bool,
}

impl VmlDisplay {
    /// 第一个带 `o:hr="t"` 的形状（HTML `<hr>` 导入的细横线）。
    pub fn rule(&self) -> Option<&VmlShape> {
        self.shapes.iter().find(|s| s.hr)
    }

    /// 第一个带 `v:imagedata` 的形状。
    pub fn image(&self) -> Option<&VmlShape> {
        self.shapes.iter().find(|s| s.imagedata.is_some())
    }

    pub fn has_textbox(&self) -> bool {
        self.shapes.iter().any(|s| s.has_textbox)
    }
}

named_enum! {
    /// VML 元素种类（`v:` 命名空间下的元素名）。
    pub enum VmlKind {
        Shape = "shape",
        Rect = "rect",
        RoundRect = "roundRect",
        Oval = "oval",
        Line = "line",
        Group = "group",
        /// `v:shapetype`：只是形状模板，不画东西。
        ShapeType = "shapeType",
        Other = "other",
    }
}

impl VmlKind {
    fn from_local(local: LocalName) -> Option<VmlKind> {
        Some(match local {
            LocalName::Shape => VmlKind::Shape,
            LocalName::Rect => VmlKind::Rect,
            LocalName::Roundrect => VmlKind::RoundRect,
            LocalName::Oval => VmlKind::Oval,
            LocalName::Line => VmlKind::Line,
            LocalName::Group => VmlKind::Group,
            LocalName::Shapetype => VmlKind::ShapeType,
            _ => return None,
        })
    }

    /// 会画出东西的形状（`v:shapetype` 只是模板）。
    pub fn is_drawn(self) -> bool {
        !matches!(self, VmlKind::ShapeType | VmlKind::Other)
    }
}

/// 一个 VML 形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmlShape {
    pub node: NodeId,
    pub kind: VmlKind,
    /// `@style` 的键值对，键小写、值原样（`MOD-11`：不做语义解释）。
    pub style: Vec<(String, String)>,
    /// `@fillcolor`，归一成 6 位大写 hex；认不出的颜色写法 → `None`。
    pub fill_color: Option<String>,
    /// `@filled`（`f` / `false` 为假）。
    pub filled: Option<bool>,
    pub stroke_color: Option<String>,
    pub stroked: Option<bool>,
    /// `@coordsize`：组的子坐标系尺寸（整数对）。
    pub coordsize: Option<(i64, i64)>,
    /// `@coordorigin`：组子坐标系的原点（整数对）。画布里的孩子从它量起，不是从 0,0。
    pub coordorigin: Option<(i64, i64)>,
    /// `@path`：VML 自定义路径，坐标在 `coordsize` 定的空间里。
    pub path: Option<String>,
    /// `@type`：引用 `v:shapetype` 的 id（`#_x0000_t202` 是文本框、`t75` 是图片）。
    pub shape_type: Option<String>,
    /// `@o:spt`：形状类型号。`75` 同样是图片。
    pub spt: Option<String>,
    /// `v:imagedata/@r:id`。
    pub imagedata: Option<String>,
    /// `v:textpath/@string`（WordArt 文字）。
    pub textpath: Option<String>,
    /// `v:textpath/@style`：WordArt 的字体与字号写在这里。
    pub textpath_style: Option<String>,
    /// `@strokeweight`（多半带 `pt`）。
    pub stroke_weight: Option<String>,
    /// `v:fill` 元素上的颜色。WordArt 拿它当**文字**颜色。
    pub fill: Option<VmlFill>,
    /// `@o:hr="t"`：HTML `<hr>` 导入的细横线。
    pub hr: bool,
    /// 直接挂着 `v:textbox`。
    pub has_textbox: bool,
    /// `v:textbox/w:txbxContent`：框里的独立内容流。
    pub txbx: Option<NodeId>,
    /// 框里内容流的块（`MOD-11` 的 `content`）。由 `Document::rebuild` 复用段落管线构建。
    pub content: Vec<Block>,
    /// 所属 `v:group` 在 `shapes` 里的下标。
    pub parent: Option<usize>,
    /// 这个形状躺在别的形状的 `w:txbxContent` 里：它不是页面上的兄弟框，
    /// 也不占保存路径的序号。
    pub nested: bool,
}

impl VmlShape {
    /// `style` 里某个键的原值。
    pub fn style_get(&self, key: &str) -> Option<&str> {
        self.style.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// `style` 里某个键当长度解析（`width:96pt` 之类）。
    pub fn style_len(&self, key: &str) -> Option<Length> {
        parse_length(self.style_get(key)?)
    }

    /// `position:absolute`：脱离文字流。
    pub fn is_absolute(&self) -> bool {
        self.style_get("position").is_some_and(|v| v.eq_ignore_ascii_case("absolute"))
    }

    /// `visibility:hidden`。
    pub fn is_hidden(&self) -> bool {
        self.style_get("visibility").is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
    }

    /// 图片形状类型（`o:spt="75"` 或 `type="#_x0000_t75"`）。图解析不出来时它什么都不画。
    pub fn is_picture_type(&self) -> bool {
        self.spt.as_deref() == Some("75")
            || self.shape_type.as_deref().is_some_and(|t| t.contains("_x0000_t75"))
    }
}

/// `v:fill` 元素上的颜色。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmlFill {
    pub color: Option<String>,
    pub color2: Option<String>,
}

/// `w:object` 的嵌入对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OleInfo {
    /// `o:OLEObject` 节点。
    pub node: NodeId,
    /// `@ProgID`：`Excel.Sheet.12` 等。
    pub prog_id: Option<String>,
    /// `@Type`：`Embed` / `Link`。
    pub kind: Option<String>,
    /// `w:object/@w:dxaOrig`（缇），预览图的声明宽度。
    pub dxa_orig: Option<i64>,
    pub dya_orig: Option<i64>,
}

/// 建一个 `w:pict` / `w:object` 的 VML 显示模型。
pub fn vml_display(dom: &Dom, node: NodeId) -> VmlDisplay {
    let mut shapes: Vec<VmlShape> = Vec::new();
    let mut ole = None;
    // (节点, 深度, 所属组下标, 穿过了几层别人的 txbxContent)
    let mut stack: Vec<(NodeId, u32, Option<usize>, u32)> = vec![(node, 0, None, 0)];
    let mut scratch: Vec<NodeId> = Vec::new();
    let mut too_deep = false;
    while let Some((n, depth, parent, boxes)) = stack.pop() {
        let nested = boxes > 0;
        let mut group = parent;
        if let Some(name) = dom.name(n) {
            if dom.is_ns(n, NsId::V, "v")
                && let Some(kind) = VmlKind::from_local(name.local)
            {
                shapes.push(shape(dom, n, kind, parent, nested));
                if kind == VmlKind::Group {
                    group = Some(shapes.len() - 1);
                }
            }
            if name.local == LocalName::OLEObject && ole.is_none() && dom.is_ns(n, NsId::O, "o") {
                ole = Some(OleInfo {
                    node: n,
                    prog_id: attr(dom, n, NsId::None, LocalName::ProgID),
                    kind: attr(dom, n, NsId::None, LocalName::UType),
                    dxa_orig: num(dom, node, NsId::W, LocalName::DxaOrig),
                    dya_orig: num(dom, node, NsId::W, LocalName::DyaOrig),
                });
            }
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let boxes = boxes + u32::from(dom.is(n, QName::new(NsId::W, LocalName::TxbxContent)));
        if boxes > MAX_BOX_NESTING {
            too_deep = true;
            continue;
        }
        scratch.clear();
        scratch.extend(dom.semantic_children(n));
        stack.extend(scratch.iter().rev().map(|&c| (c, depth + 1, group, boxes)));
    }
    VmlDisplay { node, shapes, ole, too_deep }
}

fn shape(dom: &Dom, n: NodeId, kind: VmlKind, parent: Option<usize>, nested: bool) -> VmlShape {
    let mut s = VmlShape {
        node: n,
        kind,
        style: attr(dom, n, NsId::None, LocalName::Style)
            .map(|v| parse_style(&v))
            .unwrap_or_default(),
        fill_color: attr(dom, n, NsId::None, LocalName::Fillcolor).and_then(|v| vml_color(&v)),
        filled: flag(dom, n, NsId::None, LocalName::Filled),
        stroke_color: attr(dom, n, NsId::None, LocalName::Strokecolor).and_then(|v| vml_color(&v)),
        stroked: flag(dom, n, NsId::None, LocalName::Stroked),
        coordsize: attr(dom, n, NsId::None, LocalName::Coordsize).and_then(|v| pair(&v)),
        coordorigin: attr(dom, n, NsId::None, LocalName::Coordorigin).and_then(|v| pair(&v)),
        path: attr(dom, n, NsId::None, LocalName::Path),
        shape_type: attr(dom, n, NsId::None, LocalName::Type),
        spt: attr(dom, n, NsId::O, LocalName::Spt),
        imagedata: None,
        textpath: None,
        textpath_style: None,
        stroke_weight: attr(dom, n, NsId::None, LocalName::Strokeweight),
        fill: None,
        hr: dom.attr(n, QName::new(NsId::O, LocalName::Hr)).is_some_and(|_| {
            attr(dom, n, NsId::O, LocalName::Hr).is_some_and(|v| v == "t" || v == "true")
        }),
        has_textbox: false,
        txbx: None,
        content: Vec::new(),
        parent,
        nested,
    };
    // 直接子节点上的图片 / WordArt 文字 / 文本框
    for c in dom.semantic_children(n) {
        let Some(name) = dom.name(c) else { continue };
        if !dom.is_ns(c, NsId::V, "v") {
            continue;
        }
        match name.local {
            LocalName::Imagedata if s.imagedata.is_none() => {
                s.imagedata = attr(dom, c, NsId::R, LocalName::Id);
            }
            LocalName::Textpath if s.textpath.is_none() => {
                s.textpath = attr(dom, c, NsId::None, LocalName::String);
                s.textpath_style = attr(dom, c, NsId::None, LocalName::Style);
            }
            LocalName::Fill if s.fill.is_none() => {
                s.fill = Some(VmlFill {
                    color: attr(dom, c, NsId::None, LocalName::Color),
                    color2: attr(dom, c, NsId::None, LocalName::Color2),
                });
            }
            LocalName::Textbox => {
                s.has_textbox = true;
                s.txbx = dom
                    .semantic_children(c)
                    .find(|&t| dom.is(t, QName::new(NsId::W, LocalName::TxbxContent)));
            }
            _ => {}
        }
    }
    s
}

fn attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.trim().to_string())
}

fn num(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<i64> {
    attr(dom, node, ns, local)?.parse().ok()
}

/// VML 布尔属性：`f` / `false` 为假，其余（`t` / `true`）为真。
fn flag(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<bool> {
    let v = attr(dom, node, ns, local)?.to_ascii_lowercase();
    Some(!(v == "f" || v == "false"))
}

/// HTML 颜色名，VML 属性里常见（TS `VML_NAMED_COLORS`）。
const NAMED: &[(&str, &str)] = &[
    ("black", "000000"),
    ("white", "FFFFFF"),
    ("red", "FF0000"),
    ("green", "008000"),
    ("blue", "0000FF"),
    ("yellow", "FFFF00"),
    ("silver", "C0C0C0"),
    ("gray", "808080"),
    ("grey", "808080"),
    ("maroon", "800000"),
    ("olive", "808000"),
    ("navy", "000080"),
    ("purple", "800080"),
    ("teal", "008080"),
    ("fuchsia", "FF00FF"),
    ("lime", "00FF00"),
    ("aqua", "00FFFF"),
    ("cyan", "00FFFF"),
    ("orange", "FFA500"),
];

/// VML 颜色属性 → 6 位 hex（不带 `#`）。`#dbe5f1` / `#aaa` / `#dbe5f1 [3204]` / `silver` 都认。
///
/// **保留原大小写**：TS 的 VML 路径直接把 `fillcolor` 去掉 `#` 就用（`vml-textbox__002` 的
/// `borderColor` 是小写），只有细横线那条路会 `toUpperCase`。
pub fn vml_color(v: &str) -> Option<String> {
    let s = v.trim();
    let body = s.trim_start_matches('#');
    let head: String = body.chars().take(6).collect();
    if head.len() == 6 && head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(head);
    }
    // `#abc` 简写（后面不能再跟十六进制位）
    if let Some(short) = s.strip_prefix('#') {
        let three: String = short.chars().take(3).collect();
        if three.len() == 3
            && three.bytes().all(|b| b.is_ascii_hexdigit())
            && !short.chars().nth(3).is_some_and(|c| c.is_ascii_hexdigit())
        {
            return Some(three.chars().flat_map(|c| [c, c]).collect());
        }
    }
    let name = s.split([' ', '[']).next().unwrap_or("").to_ascii_lowercase();
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, h)| (*h).to_string())
}

/// `coordsize="2000,1000"`。
fn pair(v: &str) -> Option<(i64, i64)> {
    let (a, b) = v.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const NS: &str = concat!(
        r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
        r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
        r#" xmlns:v="urn:schemas-microsoft-com:vml""#,
        r#" xmlns:o="urn:schemas-microsoft-com:office:office""#,
    );

    fn parse(inner: &str) -> (Dom, VmlDisplay) {
        let src = format!("<w:pict{NS}>{inner}</w:pict>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let v = vml_display(&dom, root);
        (dom, v)
    }

    #[test]
    fn mod_11_vml_horizontal_rule() {
        // 语料 smartart-ole__005：HTML <hr> 导入的细横线
        let (_, v) = parse(
            r##"<v:rect id="_x0000_i1026" style="width:0;height:1.5pt" o:hralign="center" o:hr="t" fillcolor="#aca899" stroked="f"/>"##,
        );
        let r = v.rule().expect("hr");
        assert_eq!(r.kind, VmlKind::Rect);
        assert_eq!(r.fill_color.as_deref(), Some("aca899"), "去掉 # 但保留原大小写");
        assert_eq!(r.stroked, Some(false));
        assert_eq!(r.style_len("height").unwrap().to_emu(), Some(1.5 * 12700.0));
        // width:0 是「铺满可用宽度」，不是 0 宽
        assert_eq!(r.style_get("width"), Some("0"));
    }

    #[test]
    fn mod_11_vml_group_records_coordsize_and_parent() {
        let (_, v) = parse(concat!(
            r#"<v:group style="width:150pt;height:75pt" coordsize="2000,1000">"#,
            r#"<v:shape id="pic" style="position:absolute;width:200;height:100"><v:imagedata r:id="rId10"/></v:shape>"#,
            r#"<v:shape id="tb" style="position:absolute"><v:textbox><w:txbxContent/></v:textbox></v:shape>"#,
            r#"</v:group>"#,
        ));
        assert_eq!(v.shapes.len(), 3);
        assert_eq!(v.shapes[0].kind, VmlKind::Group);
        assert_eq!(v.shapes[0].coordsize, Some((2000, 1000)));
        assert_eq!(v.shapes[1].parent, Some(0));
        assert_eq!(v.shapes[2].parent, Some(0));
        assert_eq!(v.image().and_then(|s| s.imagedata.as_deref()), Some("rId10"));
        assert!(v.shapes[1].is_absolute());
        assert!(v.has_textbox());
        // 组坐标里的长度没有单位，换算要靠组比例（4.6）
        assert_eq!(v.shapes[1].style_len("width").unwrap().to_emu(), None);
    }

    #[test]
    fn mod_11_vml_wordart_and_hidden() {
        let (_, v) =
            parse(r#"<v:shape style="visibility:hidden"><v:textpath string="HELLO"/></v:shape>"#);
        assert_eq!(v.shapes[0].textpath.as_deref(), Some("HELLO"));
        assert!(v.shapes[0].is_hidden());
        assert!(v.rule().is_none());
    }

    #[test]
    fn mod_11_vml_color_forms() {
        assert_eq!(vml_color("#ACA899"), Some("ACA899".into()));
        assert_eq!(vml_color("aca899"), Some("aca899".into()));
        assert_eq!(vml_color("#ffffff [65535]"), Some("ffffff".into()));
        // HTML 颜色名与 `#abc` 简写也认（TS `vmlColorHex`）
        assert_eq!(vml_color("red"), Some("FF0000".into()));
        assert_eq!(vml_color("Silver [2]"), Some("C0C0C0".into()));
        assert_eq!(vml_color("#abc"), Some("aabbcc".into()));
        assert_eq!(vml_color("window"), None);
    }
}
