//! 节模型（`MOD-10` 的 `SectionInfo`、`spec/16` 任务 5.1 / 5.2）与锚定绘图要的页面几何
//! （`spec/15` 任务 4.6c）。
//!
//! 两层：
//!
//! - [`SectionInfo`]：一个节的声明值。`props` 走节属性表（`schema/props/section.toml`，`PROP-01`），
//!   所以 `ST_TwipsMeasure` 的单位与容错、`Val::Raw` 降级都是共用的一份 codec。节的**继承**
//!   （页眉页脚引用缺失时沿用上一节）不在这里，在 `resolve::section`（`RES-10`）——模型只存声明值。
//! - [`SectionGeom`] / [`Sections`]：从 `props` 里挑出页宽页高、四边页边距、栏数的小结构，按
//!   字节偏移查询。`wp:anchor` 相对 `page` / `margin` 对齐时要拿它解横向位置（TS
//!   `resolveAnchorPagePos`），否则浮动框的 `offsetXEmu` / `pageRelX` / `pagePinned` 都定不下来。
//!
//! 节的边界：每个 `w:sectPr` **结束**它所在的节（最后一节的 `sectPr` 是 `w:body` 的末尾子元素，
//! 分节段落的写在自己的 `pPr` 里）。所以「管辖某个位置的节」= 第一个结束位置在它之后的
//! `sectPr`（TS `sectionAt` 同义）。一份 `w:sectPr` 都没有的文档给一个隐式节（`node: None`，
//! 全部取缺省，同 TS `DEFAULT_SECTION`）。

use std::ops::Range;

use crate::diag::Diagnostic;
use crate::model::block::{Block, ProtectedKind, Revision};
use crate::model::inline::RevisionMeta;
use crate::model::macros::named_enum;
use crate::semantic::props::{
    SectType, SectionProps, Val, read_section_props, read_section_props_change,
};
use crate::xml::{Dom, LocalName, NodeId, QName};

/// 缺省节：US Letter 竖排、四边 1 英寸（同 TS `DEFAULT_SECTION`）。
pub const DEFAULT_PAGE_WIDTH: i64 = 12_240;
pub const DEFAULT_PAGE_HEIGHT: i64 = 15_840;
pub const DEFAULT_MARGIN: i64 = 1_440;

named_enum! {
    /// 页眉还是页脚。名字是 TS 的字面值。
    pub enum HfKind {
        Header = "header",
        Footer = "footer",
    }
}

named_enum! {
    /// 页眉页脚的三种变体（`ST_HdrFtr`）。非 schema 的 `odd` 归 `Default`（`RES-10`）。
    pub enum HfVariant {
        Default = "default",
        First = "first",
        Even = "even",
    }
}

impl HfKind {
    pub const ALL: [HfKind; 2] = [HfKind::Header, HfKind::Footer];
}

impl HfVariant {
    pub const ALL: [HfVariant; 3] = [HfVariant::Default, HfVariant::First, HfVariant::Even];

    /// `w:type` 的建模值 → 变体。缺失与认不出的都算 `default`（Word 行为，`docs/01` §12）。
    pub fn of(kind: Option<&Val<crate::semantic::props::HdrFtrType>>) -> HfVariant {
        use crate::semantic::props::HdrFtrType as T;
        match kind {
            Some(Val::Value(T::First)) => HfVariant::First,
            Some(Val::Value(T::Even)) => HfVariant::Even,
            // default / odd（非 schema）/ Raw / 缺失
            _ => HfVariant::Default,
        }
    }
}

/// 一个节的 `w:sectPr` 长在哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionOwner {
    /// `w:body` 的末尾子元素：最后一节。
    Body,
    /// 分节段落的 `pPr/sectPr`；`NodeId` 是那个 `w:p`。
    Paragraph(NodeId),
    /// 文档里没有任何 `w:sectPr`：隐式节。
    Implicit,
}

/// 一个节（`MOD-10`）。全是**声明值**：继承看 `resolve::section`（`RES-10`）。
///
/// `PartialEq` 是 `MOD-13` 的 oracle 要的（`refresh == rebuild`）；生成的 `SectionProps` 的
/// `PartialEq` 不比 `raw_unmodeled`（未建模子元素的 `NodeId`），所以比较只看建模字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    /// `w:sectPr`；隐式节为 `None`。
    pub node: Option<NodeId>,
    /// 装箱：`SectionProps` 有几 KB，节列表按值传会把栈帧撑大（同 `TableBlock.props`）。
    pub props: Box<SectionProps>,
    pub owner: SectionOwner,
    /// 属于本节的块在 `Document.main` 里的下标区间；分节段落自己算本节的最后一块。
    pub block_range: Range<usize>,
    /// `sectPr/sectPrChange` 的旧值快照（`MOD-09`）。
    pub revisions: Vec<Revision>,
    /// `w:sectPr` 的结束字节偏移，`Sections::at` 用；隐式节为 `u32::MAX`。
    end_offset: u32,
}

impl SectionInfo {
    /// `w:type`：本节相对上一节如何开始。缺省 `nextPage`；第一节无意义。
    pub fn start_type(&self) -> SectType {
        match self.props.kind.as_ref() {
            Some(Val::Value(t)) => *t,
            _ => SectType::NextPage,
        }
    }

    /// `w:titlePg`：本节首页用 `first` 变体。
    pub fn title_pg(&self) -> bool {
        self.props.title_pg == Some(true)
    }

    /// 本节**声明**的某个槽的关系 id；没声明返回 `None`（继承在 `RES-10`）。
    ///
    /// 同一变体重复声明时取第一个（Word 读第一个）。
    pub fn hf_ref(&self, kind: HfKind, variant: HfVariant) -> Option<&str> {
        let list = match kind {
            HfKind::Header => &self.props.header_references,
            HfKind::Footer => &self.props.footer_references,
        };
        list.iter()
            .find(|r| HfVariant::of(r.kind.as_ref()) == variant)
            .and_then(|r| r.id.as_deref())
    }

    /// 本节声明的全部槽（供 `resolve::section` 做继承）。
    pub fn declared_refs(&self) -> impl Iterator<Item = (HfKind, HfVariant, &str)> + '_ {
        HfKind::ALL.into_iter().flat_map(move |k| {
            HfVariant::ALL.into_iter().filter_map(move |v| self.hf_ref(k, v).map(|id| (k, v, id)))
        })
    }

    /// 页面几何。
    pub fn geom(&self) -> SectionGeom {
        geom_of(self.node, &self.props)
    }
}

/// 一个 `w:sectPr` 的页面几何。长度单位是缇。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionGeom {
    /// `w:sectPr`；隐式节为 `None`。
    pub node: Option<NodeId>,
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

/// 按文档序的节几何，附各自的结束偏移。投影侧的查询缓存。
#[derive(Debug, Clone, Default)]
pub struct Sections {
    list: Vec<(u32, SectionGeom)>,
}

impl Sections {
    /// 从已建好的节列表取几何（`Document::rebuild` 之后的正路，不重复读属性）。
    pub fn from_sections(sections: &[SectionInfo]) -> Sections {
        Sections { list: sections.iter().map(|s| (s.end_offset, s.geom())).collect() }
    }

    /// 直接扫一个 part 里的全部 `w:sectPr`（没有 `Document` 时的退路，例如辅助 part）。
    pub fn build(dom: &Dom) -> Sections {
        let mut list = Vec::new();
        for n in dom.semantic_descendants(dom.root()) {
            if !dom.is(n, QName::w(LocalName::SectPr)) {
                continue;
            }
            let mut diags = Vec::new();
            let props = read_section_props(dom, Some(n), &mut diags);
            list.push((end_offset(dom, n), geom_of(Some(n), &props)));
        }
        list.sort_by_key(|&(end, _)| end);
        Sections { list }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 管辖某个字节偏移的节：**第一个结束位置在它之后**的 `w:sectPr`；都在它之前就取最后一个。
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

fn attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::w(local)).map(|v| v.into_owned())
}

fn end_offset(dom: &Dom, node: NodeId) -> u32 {
    dom.node(node).lex.as_ref().map_or(0, |l| l.range.end)
}

fn geom_of(node: Option<NodeId>, p: &SectionProps) -> SectionGeom {
    let (sz, mar) = (p.page_size.as_ref(), p.page_margins.as_ref());
    SectionGeom {
        node,
        page_width: twips(sz.and_then(|s| s.w.as_ref()), DEFAULT_PAGE_WIDTH),
        page_height: twips(sz.and_then(|s| s.h.as_ref()), DEFAULT_PAGE_HEIGHT),
        margin_top: twips(mar.and_then(|m| m.top.as_ref()), DEFAULT_MARGIN),
        margin_right: twips(mar.and_then(|m| m.right.as_ref()), DEFAULT_MARGIN),
        margin_bottom: twips(mar.and_then(|m| m.bottom.as_ref()), DEFAULT_MARGIN),
        margin_left: twips(mar.and_then(|m| m.left.as_ref()), DEFAULT_MARGIN),
        columns: twips(p.columns.as_ref().and_then(|c| c.num.as_ref()), 1).max(1),
    }
}

/// 一个顶层块里的 `w:sectPr`：body 级的 `w:sectPr` 自己，或分节段落 `pPr` 里的那个。
///
/// **不下钻**表格与文本框：框里的段落是另一个内容流，它的 `sectPr` 不结束正文的节
/// （TS 按 `originalXml` 里有没有 `<w:sectPr` 判，会把框里的算上；那是它的缺陷）。
fn sect_pr_of(dom: &Dom, block: &Block) -> Option<(NodeId, SectionOwner)> {
    let node = block.node();
    if dom.is(node, QName::w(LocalName::SectPr)) {
        return Some((node, SectionOwner::Body));
    }
    if !dom.is(node, QName::w(LocalName::P)) {
        return None;
    }
    let ppr = dom.semantic_children(node).find(|&c| dom.is(c, QName::w(LocalName::PPr)))?;
    let sect = dom.semantic_children(ppr).find(|&c| dom.is(c, QName::w(LocalName::SectPr)))?;
    Some((sect, SectionOwner::Paragraph(node)))
}

/// 读一个 `w:sectPr` 建出 `SectionInfo`。
///
/// `#[inline(never)]`：`SectionProps` 几 KB，读进来立刻装箱，调用方的栈帧只留一个指针
/// （同 `model/table.rs` 的 `boxed_reader!`）。
#[inline(never)]
fn info_of(
    dom: &Dom,
    node: Option<NodeId>,
    owner: SectionOwner,
    block_range: Range<usize>,
    diags: &mut Vec<Diagnostic>,
) -> SectionInfo {
    let props = Box::new(match node {
        Some(n) => read_section_props(dom, Some(n), diags),
        None => SectionProps::default(),
    });
    let mut revisions = Vec::new();
    if let Some(n) = node
        && let Some((change, old)) = read_section_props_change(dom, Some(n), diags)
    {
        revisions.push(Revision::SectPropsChange {
            meta: RevisionMeta {
                node: change,
                id: attr(dom, change, LocalName::Id),
                author: attr(dom, change, LocalName::Author),
                date: attr(dom, change, LocalName::Date),
            },
            old: Box::new(old),
        });
    }
    SectionInfo {
        node,
        props,
        owner,
        block_range,
        revisions,
        end_offset: node.map_or(u32::MAX, |n| end_offset(dom, n)),
    }
}

/// 正文的节序列（`MOD-10`）：按块序走一遍，每个 `w:sectPr` 结束它所在的节。
///
/// - 一个 `w:sectPr` 都没有 → 一个隐式节（全缺省）覆盖全部块，同 TS `readSections`。
/// - 最后一个 `sectPr` 之后还有块（畸形文档，`w:sectPr` 本该是 body 末尾）→ 并进最后一节，
///   与 `Sections::at` 的"都在它之前就取最后一个"一致。
pub fn build_sections(
    dom: &Dom,
    blocks: &[Block],
    diags: &mut Vec<Diagnostic>,
) -> Vec<SectionInfo> {
    let mut out: Vec<SectionInfo> = Vec::new();
    let mut first = 0usize;
    for (i, b) in blocks.iter().enumerate() {
        // 隐藏的 body 级 sectPr 块与分节段落都算；其余块不看
        let is_props_block =
            matches!(b, Block::Protected(p) if p.kind == ProtectedKind::SectionProps);
        let Some((node, owner)) = sect_pr_of(dom, b) else { continue };
        debug_assert!(is_props_block || matches!(owner, SectionOwner::Paragraph(_)));
        out.push(info_of(dom, Some(node), owner, first..i + 1, diags));
        first = i + 1;
    }
    match out.last_mut() {
        None => out.push(info_of(dom, None, SectionOwner::Implicit, 0..blocks.len(), diags)),
        Some(last) if first < blocks.len() => last.block_range.end = blocks.len(),
        Some(_) => {}
    }
    out
}

/// 管辖某个节点的节下标：第一个结束位置在它之后的 `sectPr`；都在它之前就取最后一个。
pub fn section_of(dom: &Dom, sections: &[SectionInfo], node: NodeId) -> Option<usize> {
    if sections.is_empty() {
        return None;
    }
    let start = dom.node(node).lex.as_ref().map_or(0, |l| l.range.start);
    Some(sections.iter().position(|s| s.end_offset > start).unwrap_or(sections.len() - 1))
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
