//! L2 范围层（`spec/03-span.md`，`docs/03` §5.1–5.3、5.5、5.6）。
//!
//! 书签、批注、权限、移动范围、customXml 修订范围的端点是 run 的兄弟元素，可任意交叠，
//! 树表达不了：这里用附着在 DOM 上的 `Anchor`（容器 + 内容序列边界 + affinity）与平铺的
//! `RangeSpan` 列表表示，并在编辑时维护。字段子系统见 [`field`]。
//!
//! 里程碑：M2 任务 2.1 建立索引（[`SpanIndex`]，`SPAN-01`–`SPAN-05`）；Anchor 变换（`SPAN-06/07`）
//! 与物化（`SPAN-08`）在 2.2 / 2.3。字段子系统见 [`field`]（2.4）。

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

pub mod content;
pub mod field;
pub mod index;
pub mod transform;

pub use content::{
    content_children, content_index_of, content_len, is_content_container, is_content_item,
    item_containing,
};
pub use field::FieldId;
pub use index::{
    Affinity, Anchor, RangeClass, RangeKind, RangeSpan, SpanEnd, SpanIndex, SpanOrigin, compare,
};
pub use transform::{SpanAction, SpanPolicy, SpanUpdate, plan_update};

/// 一条修订的元数据（`w:id` / `w:author` / `w:date`）。
///
/// 范围标记（`w:moveFromRangeStart`、`w:customXmlInsRangeStart` …）与内容修订元素
/// （`w:ins` / `w:del` / `w:rPrChange` …）携带同一组属性，所以类型放在 L2；
/// L3 通过 [`crate::model::RevisionMeta`] 使用同一个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionMeta {
    pub node: NodeId,
    pub id: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

/// 范围（书签 / 批注 / 权限 / 移动 / customXml 修订）的会话内稳定 id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpanId(pub u32);

/// 内容流 id（`SPAN-01`）：body、每个 `w:txbxContent`、每个 `w:hdr` / `w:ftr`、每个脚注 / 尾注 / 批注条目
/// 各为一个流。范围禁止跨流；同流判定必须比较 `FlowId`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FlowId(pub u32);

/// 流根元素（`SPAN-01`）。
pub fn is_flow_root(name: QName) -> bool {
    name.ns == NsId::W
        && matches!(
            name.local,
            LocalName::Body
                | LocalName::TxbxContent
                | LocalName::Hdr
                | LocalName::Ftr
                | LocalName::Footnote
                | LocalName::Endnote
                | LocalName::Comment
        )
}

/// 范围标记元素（`SPAN-01`/`SPAN-03`）：不进入内容序列，不产生 Block / Inline。
pub fn is_range_marker(name: QName) -> bool {
    name.ns == NsId::W
        && matches!(
            name.local,
            LocalName::BookmarkStart
                | LocalName::BookmarkEnd
                | LocalName::CommentRangeStart
                | LocalName::CommentRangeEnd
                | LocalName::PermStart
                | LocalName::PermEnd
                | LocalName::MoveFromRangeStart
                | LocalName::MoveFromRangeEnd
                | LocalName::MoveToRangeStart
                | LocalName::MoveToRangeEnd
                | LocalName::CustomXmlInsRangeStart
                | LocalName::CustomXmlInsRangeEnd
                | LocalName::CustomXmlDelRangeStart
                | LocalName::CustomXmlDelRangeEnd
                | LocalName::CustomXmlMoveFromRangeStart
                | LocalName::CustomXmlMoveFromRangeEnd
                | LocalName::CustomXmlMoveToRangeStart
                | LocalName::CustomXmlMoveToRangeEnd
        )
}

/// 属性元素（`SPAN-01`）：内容序列里也不算。
pub fn is_property_element(name: QName) -> bool {
    name.ns == NsId::W
        && matches!(
            name.local,
            LocalName::PPr
                | LocalName::RPr
                | LocalName::TcPr
                | LocalName::TrPr
                | LocalName::TblPr
                | LocalName::TblGrid
                | LocalName::SectPr
                | LocalName::TblPrEx
        )
}

/// 一个 part 里"容器 → 流"的缓存映射（`SPAN-01`）。按 `NodeId` 索引，覆盖全部元素节点；
/// 不在任何流根之下的元素（如 `w:document` 自身）为 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowMap {
    by_node: Vec<Option<FlowId>>,
    /// `roots[flow.0]` 是该流的流根元素。
    roots: Vec<NodeId>,
}

impl FlowMap {
    /// 一次前序遍历：进入流根时分配新 `FlowId`，其后代（含嵌套流根之外的）都属于它。
    pub fn build(dom: &Dom) -> FlowMap {
        let mut by_node = vec![None; dom.node_count()];
        let mut roots = Vec::new();
        // (节点, 当前流)
        let mut stack: Vec<(NodeId, Option<FlowId>)> = vec![(dom.root(), None)];
        while let Some((node, mut flow)) = stack.pop() {
            if dom.name(node).is_some_and(is_flow_root) {
                let id = FlowId(u32::try_from(roots.len()).expect("flow count fits u32"));
                roots.push(node);
                flow = Some(id);
            }
            by_node[node.0 as usize] = flow;
            for &c in dom.children(node).iter().rev() {
                stack.push((c, flow));
            }
        }
        FlowMap { by_node, roots }
    }

    /// 任一节点所在的流（文本节点取父元素的）。
    pub fn flow_of(&self, node: NodeId) -> Option<FlowId> {
        self.by_node.get(node.0 as usize).copied().flatten()
    }

    pub fn root_of(&self, flow: FlowId) -> NodeId {
        self.roots[flow.0 as usize]
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    pub fn flow_count(&self) -> usize {
        self.roots.len()
    }

    /// 两节点同流（都在某个流里且 id 相同）。
    pub fn same_flow(&self, a: NodeId, b: NodeId) -> bool {
        matches!((self.flow_of(a), self.flow_of(b)), (Some(x), Some(y)) if x == y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    #[test]
    fn span_01_flow_map_body_and_textbox_are_separate_flows() {
        let xml = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>a</w:t></w:r>
              <w:r><w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent><w:p><w:r><w:t>box</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>
              <w:p/><w:sectPr/></w:body></w:document>"#
        );
        let dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
        let flows = FlowMap::build(&dom);
        assert_eq!(flows.flow_count(), 2);
        let body = dom.semantic_children(dom.root()).next().unwrap();
        assert_eq!(flows.flow_of(dom.root()), None, "w:document 不在任何流里");
        assert_eq!(flows.flow_of(body), Some(FlowId(0)));
        let p1 = dom.semantic_children(body).next().unwrap();
        let txbx =
            dom.descendants(p1).find(|&n| dom.is(n, QName::w(LocalName::TxbxContent))).unwrap();
        let inner_p = dom.semantic_children(txbx).next().unwrap();
        assert_eq!(flows.flow_of(txbx), Some(FlowId(1)));
        assert_eq!(flows.flow_of(inner_p), Some(FlowId(1)));
        assert_eq!(flows.root_of(FlowId(1)), txbx);
        assert!(flows.same_flow(p1, body));
        assert!(!flows.same_flow(p1, inner_p));
        // 文本节点取父元素的流
        let t = dom.descendants(inner_p).find(|&n| dom.text(n).is_some()).unwrap();
        assert_eq!(flows.flow_of(t), Some(FlowId(1)));
    }
}
