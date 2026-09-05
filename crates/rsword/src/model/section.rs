//! 节的页面几何（`MOD-10` `SectionInfo` 的一小块；`spec/15` 任务 4.6c、`spec/16` 任务 5.1）。
//!
//! 完整的节模型——`SectionInfo { node, props, owner, block_range }` 与 `RES-10` 的继承——是任务 5.2 的。
//! 这里只给锚定绘图定位真正需要的那几个数：页宽页高、四边页边距、栏数。
//!
//! 为什么绘图要它：`wp:anchor` 相对 `page` / `margin` 对齐时，横向位置得拿页宽页边距去解
//! （TS `resolveAnchorPagePos`），否则浮动框的 `offsetXEmu` / `pageRelX` / `pagePinned` 都定不下来。
//!
//! 任务 5.1 起值不再手抄属性，而是走节属性表（`schema/props/section.toml`，`PROP-01`）：同一份
//! codec 负责 `ST_TwipsMeasure` 的单位与容错，读不出的值降级为 `Val::Raw` 并按缺省处理。
//! `read_section_props` 的 `PROP_BAD_VALUE` 诊断在这里**丢掉**——`Sections` 是投影侧的几何缓存，
//! 同一批 `w:sectPr` 会由任务 5.2 的 `SectionInfo` 再读一次并把诊断收进 `Document.warnings`。

use crate::diag::Diagnostic;
use crate::semantic::props::{SectionProps, Val, read_section_props};
use crate::xml::{Dom, LocalName, NodeId, QName};

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

/// 建模值 → 缇；`Val::Raw`（认不出的字面）与缺失都按 `default` 处理。
fn twips(v: Option<&Val<i32>>, default: i64) -> i64 {
    match v {
        Some(Val::Value(n)) => i64::from(*n),
        _ => default,
    }
}

fn geom(dom: &Dom, sect_pr: NodeId) -> SectionGeom {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let p: SectionProps = read_section_props(dom, Some(sect_pr), &mut diags);
    let (sz, mar) = (p.page_size.as_ref(), p.page_margins.as_ref());
    SectionGeom {
        node: sect_pr,
        page_width: twips(sz.and_then(|s| s.w.as_ref()), DEFAULT_PAGE_WIDTH),
        page_height: twips(sz.and_then(|s| s.h.as_ref()), DEFAULT_PAGE_HEIGHT),
        margin_top: twips(mar.and_then(|m| m.top.as_ref()), DEFAULT_MARGIN),
        margin_right: twips(mar.and_then(|m| m.right.as_ref()), DEFAULT_MARGIN),
        margin_bottom: twips(mar.and_then(|m| m.bottom.as_ref()), DEFAULT_MARGIN),
        margin_left: twips(mar.and_then(|m| m.left.as_ref()), DEFAULT_MARGIN),
        columns: twips(p.columns.as_ref().and_then(|c| c.num.as_ref()), 1).max(1),
    }
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

    /// 任务 5.1：几何走属性表之后，认不出的字面（`Val::Raw`）与缺失一样退到缺省，
    /// 不会变成 0 或者让 `body_width` 变成负数（`PROP-09` + hostile `sectpr-bad-values`）。
    #[test]
    fn prop_09_bad_section_values_fall_back_to_defaults() {
        let src = format!(
            concat!(
                "<w:body{}>",
                r#"<w:sectPr><w:pgSz w:w="abc" w:h="-1"/>"#,
                r#"<w:pgMar w:top="x" w:right="200" w:bottom="y" w:left="400"/>"#,
                r#"<w:cols w:num="0"/></w:sectPr>"#,
                "</w:body>"
            ),
            NS
        );
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let g = *Sections::build(&dom).at(0).expect("section");
        assert_eq!(g.page_width, DEFAULT_PAGE_WIDTH, "w=\"abc\" 退到缺省");
        assert_eq!(g.page_height, -1, "-1 是能解析的数，照原值给");
        assert_eq!(g.margin_top, DEFAULT_MARGIN);
        assert_eq!(g.margin_right, 200);
        assert_eq!(g.margin_bottom, DEFAULT_MARGIN);
        assert_eq!(g.margin_left, 400);
        assert_eq!(g.columns, 1, "num=0 至少一栏");
    }
}
