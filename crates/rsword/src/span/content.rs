//! 内容序列（`SPAN-01`）：容器的语义子节点去掉属性元素与范围标记后的有序列表。
//!
//! [`Anchor`](super::Anchor) 的 `index` 就是这个序列的边界（`0..=len`，标记自身不计入）。
//! 定义只在这里实现一次：索引构建（`SPAN-04`）、文档序比较（`SPAN-05`）与编辑期变换
//! （`SPAN-06`）必须共用同一份定义，否则锚点坐标会漂移。

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

use super::{is_property_element, is_range_marker};

/// 内容容器（`SPAN-01`）：可以持有内容序列边界的元素。
///
/// `w:sdt` 自身不是容器（它的内容在 `w:sdtContent` 里）；`w:r` 也不是——标记不能是 run 的子节点。
pub fn is_content_container(name: QName) -> bool {
    name == QName { ns: NsId::W14, local: LocalName::Txbx }
        || name.ns == NsId::W
            && matches!(
                name.local,
                LocalName::Body
                    | LocalName::P
                    | LocalName::Tc
                    | LocalName::Tr
                    | LocalName::Tbl
                    | LocalName::TxbxContent
                    | LocalName::DocPartBody
                    | LocalName::SdtContent
                    | LocalName::Hdr
                    | LocalName::Ftr
                    | LocalName::Footnote
                    | LocalName::Endnote
                    | LocalName::Comment
                    | LocalName::Ins
                    | LocalName::Del
                    | LocalName::Hyperlink
                    | LocalName::SmartTag
                    | LocalName::CustomXml
                    | LocalName::FldSimple
            )
}

/// 单个语义子节点是否算内容项。
///
/// 只有元素节点算：容器里的文本与 Opaque（注释 / PI）节点不是内容项。这是 `SPAN-01`
/// 的实现细化——排版缩进产生的空白文本节点若占据内容边界，同一份文档换个产出工具就会
/// 改变锚点坐标；而 `w:body` / `w:p` 一类容器的合法内容本来只有元素。
pub fn is_content_item(dom: &Dom, node: NodeId) -> bool {
    match dom.name(node) {
        Some(q) => !is_property_element(q) && !is_range_marker(q),
        None => false,
    }
}

/// 容器的内容序列（不含 `Deleted` 与非 active 的 MCE 分支——`semantic_children` 已经处理）。
pub fn content_children(dom: &Dom, container: NodeId) -> Vec<NodeId> {
    dom.semantic_children(container).filter(|&n| is_content_item(dom, n)).collect()
}

/// 内容序列长度（`index` 的上界）。
pub fn content_len(dom: &Dom, container: NodeId) -> u32 {
    dom.semantic_children(container).filter(|&n| is_content_item(dom, n)).count() as u32
}

/// `node` 作为 `container` 的直接内容项时的下标。
pub fn content_index_of(dom: &Dom, container: NodeId, node: NodeId) -> Option<u32> {
    let mut i = 0u32;
    for c in dom.semantic_children(container) {
        if !is_content_item(dom, c) {
            continue;
        }
        if c == node {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 包含 `node` 的内容项下标：`node` 可以是该项本身，也可以在它的子树深处。
/// 用于跨容器的文档序比较（`SPAN-05` 步骤 2）。
pub fn item_containing(dom: &Dom, container: NodeId, node: NodeId) -> Option<u32> {
    let mut i = 0u32;
    for c in dom.semantic_children(container) {
        if !is_content_item(dom, c) {
            continue;
        }
        if dom.is_ancestor_or_self(c, node) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `node` 所在容器：从父节点起向上第一个内容容器。
///
/// MCE 透明节点（`mc:AlternateContent` / `mc:Choice` / `mc:Fallback`）不是容器，会被跳过，
/// 与 `semantic_children` 的展平一致。
pub fn container_of(dom: &Dom, node: NodeId) -> Option<NodeId> {
    dom.ancestors(node).find(|&a| dom.name(a).is_some_and(is_content_container))
}

/// `node` 在 `container` 内容序列中所处的边界：它前面的内容项个数。
///
/// 内容项本身返回它的下标（与 [`content_index_of`] 相同）；标记与属性元素返回它所在的边界。
/// `node` 不是 `container` 的语义子节点时返回 `None`——插入 / 删除的目标是否落在这个容器的
/// 内容序列上，就靠这一条判定（`w:r` 里的 `w:t` 不是段落的内容项）。
pub fn boundary_before(dom: &Dom, container: NodeId, node: NodeId) -> Option<u32> {
    let mut i = 0u32;
    for c in dom.semantic_children(container) {
        if c == node {
            return Some(i);
        }
        if is_content_item(dom, c) {
            i += 1;
        }
    }
    None
}
