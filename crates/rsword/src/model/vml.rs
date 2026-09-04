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

use crate::model::units::{Length, parse_length, parse_style};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// VML 子树的深度上限，与绘图同值。
const MAX_DEPTH: u32 = 64;

/// 一个 `w:pict` / `w:object` 的 VML 内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmlDisplay {
    /// `w:pict` 或 `w:object` 节点。
    pub node: NodeId,
    /// 文档序的形状表；组内形状排在组之后，`parent` 指回组。
    pub shapes: Vec<VmlShape>,
    /// `w:object` 的嵌入对象信息。
    pub ole: Option<OleInfo>,
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

/// VML 元素种类（`v:` 命名空间下的元素名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmlKind {
    Shape,
    Rect,
    RoundRect,
    Oval,
    Line,
    Group,
    /// `v:shapetype`：只是形状模板，不画东西。
    ShapeType,
    Other,
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
    /// `v:imagedata/@r:id`。
    pub imagedata: Option<String>,
    /// `v:textpath/@string`（WordArt 文字）。
    pub textpath: Option<String>,
    /// `@o:hr="t"`：HTML `<hr>` 导入的细横线。
    pub hr: bool,
    /// 直接挂着 `v:textbox`。
    pub has_textbox: bool,
    /// 所属 `v:group` 在 `shapes` 里的下标。
    pub parent: Option<usize>,
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
    // (节点, 深度, 所属组下标)
    let mut stack: Vec<(NodeId, u32, Option<usize>)> = vec![(node, 0, None)];
    let mut scratch: Vec<NodeId> = Vec::new();
    while let Some((n, depth, parent)) = stack.pop() {
        let mut group = parent;
        if let Some(name) = dom.name(n) {
            if dom.is_ns(n, NsId::V, "v")
                && let Some(kind) = VmlKind::from_local(name.local)
            {
                shapes.push(shape(dom, n, kind, parent));
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
        scratch.clear();
        scratch.extend(dom.semantic_children(n));
        stack.extend(scratch.iter().rev().map(|&c| (c, depth + 1, group)));
    }
    VmlDisplay { node, shapes, ole }
}

fn shape(dom: &Dom, n: NodeId, kind: VmlKind, parent: Option<usize>) -> VmlShape {
    let mut s = VmlShape {
        node: n,
        kind,
        style: attr(dom, n, NsId::None, LocalName::Style)
            .map(|v| parse_style(&v))
            .unwrap_or_default(),
        fill_color: attr(dom, n, NsId::None, LocalName::Fillcolor).and_then(|v| hex6(&v)),
        filled: flag(dom, n, NsId::None, LocalName::Filled),
        stroke_color: attr(dom, n, NsId::None, LocalName::Strokecolor).and_then(|v| hex6(&v)),
        stroked: flag(dom, n, NsId::None, LocalName::Stroked),
        coordsize: attr(dom, n, NsId::None, LocalName::Coordsize).and_then(|v| pair(&v)),
        imagedata: None,
        textpath: None,
        hr: dom.attr(n, QName::new(NsId::O, LocalName::Hr)).is_some_and(|_| {
            attr(dom, n, NsId::O, LocalName::Hr).is_some_and(|v| v == "t" || v == "true")
        }),
        has_textbox: false,
        parent,
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
            }
            LocalName::Textbox => s.has_textbox = true,
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

/// `#aca899` / `aca899` / `#ffffff [65535]` → `ACA899`。认不出的写法（`red`、`window`）→ `None`。
fn hex6(v: &str) -> Option<String> {
    let s = v.trim().trim_start_matches('#');
    let head: String = s.chars().take(6).collect();
    (head.len() == 6 && head.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| head.to_ascii_uppercase())
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
        assert_eq!(r.fill_color.as_deref(), Some("ACA899"), "去掉 # 并转大写");
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
        assert_eq!(hex6("#ACA899"), Some("ACA899".into()));
        assert_eq!(hex6("aca899"), Some("ACA899".into()));
        assert_eq!(hex6("#ffffff [65535]"), Some("FFFFFF".into()));
        assert_eq!(hex6("red"), None);
        assert_eq!(hex6("#abc"), None);
    }
}
