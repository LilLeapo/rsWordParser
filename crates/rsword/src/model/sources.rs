//! 参考文献源（`MOD-10`，`spec/16` 任务 5.7）。
//!
//! Word 把文献源放在一个 `customXml/item{N}.xml` part 里，根元素是 bibliography 命名空间的
//! `b:Sources`——放在这儿 Word 自己的"管理源"对话框才认。所以这份数据既不在主 part 里，
//! 也不在任何 `w:` 关系上，只能按"根元素叫什么"去找（`find_part`）。
//!
//! 读的是**投影**：`Source` 只收 TS `SourceInfo` 的六个字段，未建模的域（`b:Editor` /
//! `b:Volume` / `b:Pages` / 多作者列表…）留在 DOM 里，写回时原字节不动（`SAVE-07` 的权威列表
//! 只重建变了的条目）。

use crate::package::{Package, PartId};
use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

/// 一条文献源（TS `SourceInfo`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// `b:Tag`：引文里引用它的短标签，也是权威列表的键。
    pub tag: String,
    /// `b:SourceType`（`JournalArticle` / `Book` / `InternetSite` …）；缺失按 TS 记 `Misc`。
    pub kind: String,
    /// `b:Corporate`，否则第一个 `b:Person` 的 `"Last, First"`（两者都缺就是空串）。
    pub author: String,
    pub title: String,
    pub year: String,
    /// `b:Publisher` → `b:JournalName` → `b:InternetSiteTitle`，取第一个有的。
    pub publisher: Option<String>,
    pub url: Option<String>,
    /// 这条 `b:Source` 元素本身（写回时未变的条目原字节保留）。
    pub node: NodeId,
}

fn b(local: LocalName) -> QName {
    QName::new(NsId::B, local)
}

fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node(n).dirty != Dirty::Deleted
}

/// 子树里第一个该名字元素的文本（trim 过）。TS 用正则取"整条 `b:Source` 里第一个"，
/// 所以多作者时只看第一个 `b:Person`——这里按同一规则走文档序。
fn field(dom: &Dom, source: NodeId, local: LocalName) -> Option<String> {
    let n = dom.semantic_descendants(source).find(|&n| live(dom, n) && dom.is(n, b(local)))?;
    let text = crate::xml::xpath::string_value(dom, n);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// 一个 part 的 `b:Sources` 根元素。
fn sources_root(dom: &Dom) -> Option<NodeId> {
    let root = dom.root();
    dom.is(root, b(LocalName::Sources)).then_some(root)
}

/// 包里承载 `b:Sources` 的 part：`customXml/item{N}.xml` 里根元素是 `b:Sources` 的那个。
///
/// 按**根元素**找而不是按关系：customXml 的关系类型对每个 item 都一样，Word 也是这么找的
/// （TS `findSourcesPart` 用"文件名匹配 + 内容里出现命名空间"，同一件事）。
pub fn find_part(pkg: &mut Package) -> Option<PartId> {
    let candidates: Vec<PartId> = pkg
        .parts()
        .iter()
        .filter(|p| p.is_xml && is_custom_xml_item(p.uri.as_str()))
        .map(|p| p.id)
        .collect();
    candidates.into_iter().find(|&id| pkg.dom(id).ok().flatten().and_then(sources_root).is_some())
}

/// `customXml/item1.xml`（不含 `itemProps1.xml`）。
fn is_custom_xml_item(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("customXml/item") else { return false };
    let Some(digits) = rest.strip_suffix(".xml") else { return false };
    !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
}

/// 一个 `b:Sources` part 的全部条目（文档序）。`b:Tag` 缺失的条目跳过（同 TS：标签是键）。
pub fn read(dom: &Dom) -> Vec<Source> {
    let Some(root) = sources_root(dom) else { return Vec::new() };
    dom.semantic_children(root)
        .filter(|&n| live(dom, n) && dom.is(n, b(LocalName::Source)))
        .filter_map(|n| {
            let tag = field(dom, n, LocalName::UTag)?;
            let author = field(dom, n, LocalName::Corporate).unwrap_or_else(|| {
                let last = field(dom, n, LocalName::Last).unwrap_or_default();
                let first = field(dom, n, LocalName::First).unwrap_or_default();
                [last, first]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            Some(Source {
                tag,
                kind: field(dom, n, LocalName::SourceType).unwrap_or_else(|| "Misc".into()),
                author,
                title: field(dom, n, LocalName::UTitle).unwrap_or_default(),
                year: field(dom, n, LocalName::Year).unwrap_or_default(),
                publisher: field(dom, n, LocalName::Publisher)
                    .or_else(|| field(dom, n, LocalName::JournalName))
                    .or_else(|| field(dom, n, LocalName::InternetSiteTitle)),
                url: field(dom, n, LocalName::URL),
                node: n,
            })
        })
        .collect()
}

/// `b:SourceType` → 出版方字段的元素名（TS `sourceEntryXml` 的三分支）。
pub fn publisher_element(kind: &str) -> LocalName {
    match kind {
        "JournalArticle" => LocalName::JournalName,
        "InternetSite" => LocalName::InternetSiteTitle,
        _ => LocalName::Publisher,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_10_custom_xml_item_paths() {
        assert!(is_custom_xml_item("customXml/item1.xml"));
        assert!(is_custom_xml_item("customXml/item12.xml"));
        assert!(!is_custom_xml_item("customXml/itemProps1.xml"));
        assert!(!is_custom_xml_item("customXml/item.xml"));
        assert!(!is_custom_xml_item("word/document.xml"));
    }

    #[test]
    fn mod_10_publisher_element_per_source_type() {
        assert_eq!(publisher_element("JournalArticle"), LocalName::JournalName);
        assert_eq!(publisher_element("InternetSite"), LocalName::InternetSiteTitle);
        assert_eq!(publisher_element("Book"), LocalName::Publisher);
    }
}
