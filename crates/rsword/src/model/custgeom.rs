//! `a:custGeom` 自定义几何（`MOD-11`；`spec/15` 任务 4.6d）。
//!
//! 只记录路径命令与每条 `a:path` 的声明尺寸；归一化到 0..1 与拼成 SVG 路径串是显示投影，
//! 在 `bind/compat_ts`。
//!
//! ## 只做能如实表达的那部分
//!
//! OOXML 的自定义几何可以在 `a:avLst` / `a:gdLst` 里写公式（`gd/@fmla`），坐标写成引导名而不是
//! 数字，还能用 `a:arcTo` 画椭圆弧。公式求值器与弧转贝塞尔不在 M4 范围里，所以**遇到这些就整条
//! 几何返回 `None`**——宁可不给路径，也不能给一条少了几段、或者坐标当成 0 的错路径。
//!
//! 语料里的四份 `shape-extraction__*` 都是 `moveTo` + 三段 `lnTo` + `close` 的直边矩形，落在
//! 支持范围内。

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 一个 `a:custGeom` 的全部路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomGeom {
    pub node: NodeId,
    pub paths: Vec<GeomPath>,
}

/// 一条 `a:path`。坐标在这条路径自己的坐标系里（`@w` / `@h`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeomPath {
    /// `@w` / `@h`：这条路径的坐标空间；缺省时用形状的 `a:ext`。
    pub w: Option<i64>,
    pub h: Option<i64>,
    /// `@fill="none"`。
    pub fill_none: bool,
    /// `@stroke="0" | "false" | "none"`。
    pub stroke_none: bool,
    pub cmds: Vec<GeomCmd>,
}

/// 路径命令。点是路径坐标系里的绝对坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomCmd {
    MoveTo([i64; 2]),
    LineTo([i64; 2]),
    /// `a:quadBezTo`：控制点 + 终点。
    QuadTo([[i64; 2]; 2]),
    /// `a:cubicBezTo`：两个控制点 + 终点。
    CubicTo([[i64; 2]; 3]),
    Close,
}

impl GeomCmd {
    /// SVG 的命令字母。
    pub fn letter(&self) -> char {
        match self {
            GeomCmd::MoveTo(_) => 'M',
            GeomCmd::LineTo(_) => 'L',
            GeomCmd::QuadTo(_) => 'Q',
            GeomCmd::CubicTo(_) => 'C',
            GeomCmd::Close => 'Z',
        }
    }

    /// 命令带的点。
    pub fn points(&self) -> &[[i64; 2]] {
        match self {
            GeomCmd::MoveTo(p) | GeomCmd::LineTo(p) => std::slice::from_ref(p),
            GeomCmd::QuadTo(p) => p,
            GeomCmd::CubicTo(p) => p,
            GeomCmd::Close => &[],
        }
    }
}

/// 解析 `a:custGeom`。用到公式或圆弧时返回 `None`（见模块文档）。
pub fn custom_geom(dom: &Dom, cust_geom: NodeId) -> Option<CustomGeom> {
    // 有公式的引导表就整条放弃：坐标可能引用引导名，我们求不了值。
    for c in dom.semantic_descendants(cust_geom) {
        if dom.is(c, a(LocalName::Gd)) {
            return None;
        }
    }
    let path_lst = dom.semantic_children(cust_geom).find(|&c| dom.is(c, a(LocalName::PathLst)))?;
    let mut paths = Vec::new();
    for p in dom.semantic_children(path_lst) {
        if !dom.is(p, a(LocalName::Path)) {
            continue;
        }
        paths.push(geom_path(dom, p)?);
    }
    (!paths.is_empty()).then_some(CustomGeom { node: cust_geom, paths })
}

fn geom_path(dom: &Dom, path: NodeId) -> Option<GeomPath> {
    let mut out = GeomPath {
        w: num(dom, path, LocalName::W),
        h: num(dom, path, LocalName::H),
        fill_none: attr(dom, path, LocalName::Fill).as_deref() == Some("none"),
        stroke_none: matches!(
            attr(dom, path, LocalName::Stroke).as_deref(),
            Some("0") | Some("false") | Some("none")
        ),
        cmds: Vec::new(),
    };
    for c in dom.semantic_children(path) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        let need = match name.local {
            LocalName::MoveTo | LocalName::LnTo => 1,
            LocalName::QuadBezTo => 2,
            LocalName::CubicBezTo => 3,
            LocalName::Close => {
                out.cmds.push(GeomCmd::Close);
                continue;
            }
            // 圆弧要转成贝塞尔，M4 不做
            LocalName::ArcTo => return None,
            _ => continue,
        };
        let pts: Vec<[i64; 2]> = dom
            .semantic_children(c)
            .filter(|&n| dom.is(n, a(LocalName::Pt)))
            .map(|n| point(dom, n))
            .collect::<Option<Vec<_>>>()?;
        if pts.len() < need {
            return None;
        }
        out.cmds.push(match need {
            1 if name.local == LocalName::MoveTo => GeomCmd::MoveTo(pts[0]),
            1 => GeomCmd::LineTo(pts[0]),
            2 => GeomCmd::QuadTo([pts[0], pts[1]]),
            _ => GeomCmd::CubicTo([pts[0], pts[1], pts[2]]),
        });
    }
    Some(out)
}

/// `a:pt`。`ST_AdjCoordinate` 要么是整数，要么是引导名——引导名我们求不了值，整条几何作废。
fn point(dom: &Dom, pt: NodeId) -> Option<[i64; 2]> {
    let coord = |l: LocalName| -> Option<i64> {
        let v = attr(dom, pt, l)?.parse::<f64>().ok()?;
        (v.is_finite() && v.fract() == 0.0).then_some(v as i64)
    };
    Some([coord(LocalName::X)?, coord(LocalName::Y)?])
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

fn attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, local)).map(|s| s.trim().to_string())
}

fn num(dom: &Dom, node: NodeId, local: LocalName) -> Option<i64> {
    attr(dom, node, local)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const NS: &str = r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

    fn parse(inner: &str) -> (Dom, Option<CustomGeom>) {
        let src = format!("<a:custGeom{NS}>{inner}</a:custGeom>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let g = custom_geom(&dom, root);
        (dom, g)
    }

    const RECT: &str = concat!(
        r#"<a:avLst/><a:gdLst/><a:pathLst><a:path w="952500" h="476250">"#,
        r#"<a:moveTo><a:pt x="0" y="476250"/></a:moveTo>"#,
        r#"<a:lnTo><a:pt x="952500" y="476250"/></a:lnTo>"#,
        r#"<a:lnTo><a:pt x="952500" y="0"/></a:lnTo>"#,
        r#"<a:lnTo><a:pt x="0" y="0"/></a:lnTo>"#,
        r#"<a:close/></a:path></a:pathLst>"#,
    );

    #[test]
    fn mod_11_custgeom_straight_edges() {
        let (_, g) = parse(RECT);
        let g = g.expect("geom");
        assert_eq!(g.paths.len(), 1);
        let p = &g.paths[0];
        assert_eq!((p.w, p.h), (Some(952_500), Some(476_250)));
        assert!(!p.fill_none && !p.stroke_none);
        assert_eq!(
            p.cmds,
            vec![
                GeomCmd::MoveTo([0, 476_250]),
                GeomCmd::LineTo([952_500, 476_250]),
                GeomCmd::LineTo([952_500, 0]),
                GeomCmd::LineTo([0, 0]),
                GeomCmd::Close,
            ]
        );
    }

    #[test]
    fn mod_11_custgeom_bails_on_formulas_and_arcs() {
        // 引导公式：坐标可能写成引导名，求不了值 → 整条作废
        let (_, g) =
            parse(&format!(r#"<a:gdLst><a:gd name="adj" fmla="val 50000"/></a:gdLst>{RECT}"#));
        assert!(g.is_none(), "有 gd 公式就不给路径");
        // 圆弧要转贝塞尔，不做
        let (_, g) = parse(concat!(
            r#"<a:pathLst><a:path w="100" h="100">"#,
            r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo>"#,
            r#"<a:arcTo wR="50" hR="50" stAng="0" swAng="5400000"/>"#,
            r#"</a:path></a:pathLst>"#,
        ));
        assert!(g.is_none(), "有 arcTo 就不给路径");
        // 坐标是引导名而不是数字
        let (_, g) = parse(concat!(
            r#"<a:pathLst><a:path w="100" h="100">"#,
            r#"<a:moveTo><a:pt x="hc" y="0"/></a:moveTo></a:path></a:pathLst>"#,
        ));
        assert!(g.is_none(), "非数字坐标就不给路径");
        // 没有 pathLst
        let (_, g) = parse("<a:avLst/>");
        assert!(g.is_none());
    }

    #[test]
    fn mod_11_custgeom_fill_and_stroke_flags() {
        let (_, g) = parse(concat!(
            r#"<a:pathLst><a:path w="10" h="10" fill="none" stroke="0">"#,
            r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo>"#,
            r#"<a:cubicBezTo><a:pt x="1" y="2"/><a:pt x="3" y="4"/><a:pt x="5" y="6"/></a:cubicBezTo>"#,
            r#"</a:path></a:pathLst>"#,
        ));
        let g = g.expect("geom");
        let p = &g.paths[0];
        assert!(p.fill_none && p.stroke_none);
        assert_eq!(p.cmds[1], GeomCmd::CubicTo([[1, 2], [3, 4], [5, 6]]));
        assert_eq!(p.cmds[1].letter(), 'C');
        assert_eq!(p.cmds[1].points().len(), 3);
    }
}
