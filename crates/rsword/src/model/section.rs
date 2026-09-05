//! 节的页面几何（`MOD-10` `SectionInfo` 的一小块；`spec/15` 任务 4.6c）。
//!
//! 完整的节模型——`SectionInfo { node, props, owner, block_range }` 与 `RES-10` 的继承——是 M5 的。
//! 这里只读锚定绘图定位真正需要的那几个数：页宽页高、四边页边距、栏数。M5 建 `SectionInfo` 时
//! 把这里换掉即可，字段语义一致。
//!
//! 为什么 M4 要它：`wp:anchor` 相对 `page` / `margin` 对齐时，横向位置得拿页宽页边距去解
//! （TS `resolveAnchorPagePos`），否则浮动框的 `offsetXEmu` / `pageRelX` / `pagePinned` 都定不下来。

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 缺省节：US Letter 竖排、四边 1 英寸（同 TS `DEFAULT_SECTION`）。
pub const DEFAULT_PAGE_WIDTH: i64 = 12_240;
pub const DEFAULT_PAGE_HEIGHT: i64 = 15_840;
pub const DEFAULT_MARGIN: i64 = 1_440;

/// 一个 `w:sectPr` 的页面几何。长度单位是缇。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionGeom {
    pub node: NodeId,
    /// `w:pgSz/@w:w` / `@w:h`。
    pub page_width: i64,
    pub page_height: i64,
    /// `w:pgMar` 四边。
    pub margin_top: i64,
    pub margin_right: i64,
    pub margin_bottom: i64,
    pub margin_left: i64,
    /// `w:cols/@w:num`，缺省 1。
    pub columns: i64,
}

impl SectionGeom {
    /// 正文可用宽度（页宽减左右页边距）。
    pub fn body_width(&self) -> i64 {
        self.page_width - self.margin_left - self.margin_right
    }
}

/// 正文里全部 `w:sectPr` 的几何，按文档序，附各自的结束偏移。
#[derive(Debug, Clone, Default)]
pub struct Sections {
    list: Vec<(u32, SectionGeom)>,
}

impl Sections {
    /// 扫一个 part 里的全部 `w:sectPr`（正文末尾那个也在内）。
    pub fn build(dom: &Dom) -> Sections {
        let mut list = Vec::new();
        for n in dom.semantic_descendants(dom.root()) {
            if !dom.is(n, QName::w(LocalName::SectPr)) {
                continue;
            }
            let end = dom.node(n).lex.as_ref().map_or(0, |l| l.range.end);
            list.push((end, geom(dom, n)));
        }
        list.sort_by_key(|&(end, _)| end);
        Sections { list }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 管辖某个字节偏移的节：**第一个结束位置在它之后**的 `w:sectPr`；都在它之前就取最后一个。
    ///
    /// 节是以自己的 `w:sectPr` 结尾的（最后一节的写在 `w:body` 末尾），所以「在我之后的第一个」
    /// 才是管我的那个。
    pub fn at(&self, offset: u32) -> Option<&SectionGeom> {
        self.list
            .iter()
            .find(|&&(end, _)| end > offset)
            .or_else(|| self.list.last())
            .map(|(_, g)| g)
    }
}

fn geom(dom: &Dom, sect_pr: NodeId) -> SectionGeom {
    let mut g = SectionGeom {
        node: sect_pr,
        page_width: DEFAULT_PAGE_WIDTH,
        page_height: DEFAULT_PAGE_HEIGHT,
        margin_top: DEFAULT_MARGIN,
        margin_right: DEFAULT_MARGIN,
        margin_bottom: DEFAULT_MARGIN,
        margin_left: DEFAULT_MARGIN,
        columns: 1,
    };
    for c in dom.semantic_children(sect_pr) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::W {
            continue;
        }
        match name.local {
            LocalName::PgSz => {
                if let Some(v) = num(dom, c, LocalName::W) {
                    g.page_width = v;
                }
                if let Some(v) = num(dom, c, LocalName::H) {
                    g.page_height = v;
                }
            }
            LocalName::PgMar => {
                for (field, attr) in [
                    (&mut g.margin_top, LocalName::Top),
                    (&mut g.margin_right, LocalName::Right),
                    (&mut g.margin_bottom, LocalName::Bottom),
                    (&mut g.margin_left, LocalName::Left),
                ] {
                    if let Some(v) = num(dom, c, attr) {
                        *field = v;
                    }
                }
            }
            LocalName::Cols => {
                if let Some(v) = num(dom, c, LocalName::Num) {
                    g.columns = v;
                }
            }
            _ => {}
        }
    }
    g
}

fn num(dom: &Dom, node: NodeId, local: LocalName) -> Option<i64> {
    dom.attr_value(node, QName::w(local))?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const NS: &str = r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn mod_10_section_geometry_and_defaults() {
        let src = format!(
            concat!(
                "<w:body{}>",
                r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
                r#"<w:pgMar w:top="100" w:right="200" w:bottom="300" w:left="400"/>"#,
                r#"<w:cols w:num="2"/></w:sectPr></w:pPr></w:p>"#,
                "<w:p/>",
                "<w:sectPr/>",
                "</w:body>"
            ),
            NS
        );
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let s = Sections::build(&dom);
        assert!(!s.is_empty());

        // 第一段（偏移落在第一个 sectPr 之前）归第一节
        let first = s.at(10).expect("section");
        assert_eq!(first.page_width, 11906);
        assert_eq!((first.margin_top, first.margin_left), (100, 400));
        assert_eq!(first.columns, 2);
        assert_eq!(first.body_width(), 11906 - 400 - 200);

        // 落在两者之间的偏移归正文末尾那个空 sectPr：一切取缺省
        let last = s.at(u32::MAX - 1).expect("section");
        assert_eq!((last.page_width, last.page_height), (DEFAULT_PAGE_WIDTH, DEFAULT_PAGE_HEIGHT));
        assert_eq!(last.margin_left, DEFAULT_MARGIN);
        assert_eq!(last.columns, 1);
    }
}
