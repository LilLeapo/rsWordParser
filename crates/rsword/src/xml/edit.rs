//! DOM 变更原语与脏状态规则（`XML-12`、`XML-14`）。L4 编辑引擎的 `commit` 只调用这里的函数。
//!
//! 规则 A：改名字 / 属性 / 文本 → `SelfDirty`（`New` 保持）。
//! 规则 B：子列表变化 → 父若 `Clean` 变 `DescendantDirty`。
//! 规则 C：任一节点变为非 `Clean`，祖先链上的 `Clean` 全部变 `DescendantDirty`，遇非 `Clean` 停止。
//! 规则 D：`Deleted` 保留 `parent` 与 `lex`，从语义遍历与序列化消失。
//! 规则 E：同 part 移动先做 `namespace_compatible`；不兼容则在子树根补 `xmlns:` 声明并 `SelfDirty`。
//! 规则 F：`Clean` 克隆共享 `lex`。

use crate::package::PartFlavor;
use crate::xml::Dirty;
use crate::xml::dom::{Attr, AttrValue, Dom, Element, Mce, Node, NodeId, NodeKind, TextValue};
use crate::xml::interner::Interned;
use crate::xml::names::{LocalName, NsId, QName};
use crate::xml::ns::PrefixUse;

impl Dom {
    /// 规则 C：从 `from` 的父节点起向上传播。
    fn propagate_dirty(&mut self, from: NodeId) {
        let mut cur = self.parent(from);
        while let Some(id) = cur {
            if !self.nodes[id.idx()].dirty.absorb_descendant_change() {
                break;
            }
            cur = self.parent(id);
        }
    }

    /// 规则 A：节点自身变更。
    fn mark_self(&mut self, id: NodeId) {
        self.nodes[id.idx()].dirty.mark_self_changed();
        self.propagate_dirty(id);
    }

    /// 规则 B：`parent` 的子列表变了。
    fn mark_children_changed(&mut self, parent: NodeId) {
        if self.nodes[parent.idx()].dirty.absorb_descendant_change() {
            self.propagate_dirty(parent);
        }
    }

    // ---- 自身变更 ----------------------------------------------------------------------------

    /// 替换文本节点内容。
    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) {
        let node = &mut self.nodes[id.idx()];
        assert!(matches!(node.kind, NodeKind::Text(_)), "set_text on a non-text node");
        node.kind = NodeKind::Text(TextValue::Owned(text.into()));
        self.mark_self(id);
    }

    /// 设置属性（同名取第一个，替换其值；不存在则追加，`lex_name` 为 `None`、引号 `"`）。
    pub fn set_attr(&mut self, id: NodeId, name: QName, value: impl Into<String>) {
        let value = value.into();
        let e = self.element_mut(id).expect("set_attr on a non-element");
        match e.attrs.iter_mut().find(|a| a.name == name) {
            Some(a) => a.value = AttrValue::Owned(value),
            None => e.attrs.push(Attr {
                name,
                lex_name: None,
                value: AttrValue::Owned(value),
                quote: b'"',
            }),
        }
        self.mark_self(id);
    }

    /// 删除全部同名属性。返回是否删除了任何一个。
    pub fn remove_attr(&mut self, id: NodeId, name: QName) -> bool {
        let e = self.element_mut(id).expect("remove_attr on a non-element");
        let before = e.attrs.len();
        e.attrs.retain(|a| a.name != name);
        let removed = e.attrs.len() != before;
        if removed {
            self.mark_self(id);
        }
        removed
    }

    /// 改元素名：原始写法作废，前缀在序列化时按作用域生成。
    pub fn rename_element(&mut self, id: NodeId, name: QName) {
        let e = self.element_mut(id).expect("rename_element on a non-element");
        e.name = name;
        e.lex_name = None;
        self.mark_self(id);
    }

    // ---- 新节点 --------------------------------------------------------------------------------

    /// 新建游离元素（`New`，无 `lex`）。
    pub fn new_element(&mut self, name: QName) -> NodeId {
        self.push_detached(Node {
            kind: NodeKind::Element(Element {
                name,
                lex_name: None,
                attrs: Vec::new(),
                children: Vec::new(),
                mce: Mce::default(),
            }),
            parent: None,
            lex: None,
            dirty: Dirty::New,
        })
    }

    /// 新建游离文本节点。
    pub fn new_text(&mut self, text: impl Into<String>) -> NodeId {
        self.push_detached(Node {
            kind: NodeKind::Text(TextValue::Owned(text.into())),
            parent: None,
            lex: None,
            dirty: Dirty::New,
        })
    }

    fn push_detached(&mut self, node: Node) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("node count fits u32"));
        self.nodes.push(node);
        id
    }

    // ---- 子列表变更 ----------------------------------------------------------------------------

    /// 把游离节点插到 `parent.children[index]`。子树内部状态不变（规则 E 的"兼容"情形亦复用此函数）。
    pub fn insert_child(&mut self, parent: NodeId, index: usize, child: NodeId) {
        assert!(self.nodes[child.idx()].parent.is_none(), "insert_child: child is still attached");
        assert!(!self.is_ancestor_or_self(child, parent), "insert_child: cycle");
        let e = self.element_mut(parent).expect("insert_child on a non-element");
        let index = index.min(e.children.len());
        e.children.insert(index, child);
        self.nodes[child.idx()].parent = Some(parent);
        self.mark_children_changed(parent);
        if self.nodes[child.idx()].dirty != Dirty::Clean {
            self.propagate_dirty(child);
        }
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let len = self.children(parent).len();
        self.insert_child(parent, len, child);
    }

    /// 从父节点摘下（不标 `Deleted`），返回原下标。子树状态不变。
    pub fn detach(&mut self, id: NodeId) -> Option<usize> {
        let parent = self.nodes[id.idx()].parent?;
        let e = self.element_mut(parent).expect("parent is an element");
        let pos = e.children.iter().position(|&c| c == id)?;
        e.children.remove(pos);
        self.nodes[id.idx()].parent = None;
        self.mark_children_changed(parent);
        Some(pos)
    }

    /// 规则 D：标记删除。保留 `parent` 与 `lex`；父按规则 B。
    pub fn delete(&mut self, id: NodeId) {
        if self.nodes[id.idx()].dirty == Dirty::Deleted {
            return;
        }
        self.nodes[id.idx()].dirty = Dirty::Deleted;
        if let Some(p) = self.nodes[id.idx()].parent {
            self.mark_children_changed(p);
        }
    }

    /// `child` 在 `parent.children` 中的下标（含 `Deleted`）。
    pub fn child_index(&self, parent: NodeId, child: NodeId) -> Option<usize> {
        self.children(parent).iter().position(|&c| c == child)
    }

    pub fn is_ancestor_or_self(&self, maybe_ancestor: NodeId, node: NodeId) -> bool {
        node == maybe_ancestor || self.ancestors(node).any(|a| a == maybe_ancestor)
    }

    // ---- 规则 E：同 part 移动 -------------------------------------------------------------------

    /// 先 `namespace_compatible(subtree, dest)`；兼容 → 子树保持原状态搬到新位置；
    /// 不兼容 → 在子树根补差异声明（`xmlns:` 属性）并 `SelfDirty`，后代不变。
    pub fn move_within_part(&mut self, subtree: NodeId, dest: NodeId, index: usize) {
        assert!(
            !self.is_ancestor_or_self(subtree, dest),
            "move_within_part: cannot move a node into itself"
        );
        let missing = self.required_decls(subtree, dest);
        self.detach(subtree);
        if !missing.is_empty() {
            self.add_declarations(subtree, &missing);
        }
        self.insert_child(dest, index, subtree);
    }

    /// 在元素上追加 `xmlns` 声明（元素变 `SelfDirty`）。
    pub fn add_declarations(&mut self, id: NodeId, decls: &[PrefixUse]) {
        let flavor = self.flavor;
        for d in decls {
            let uri = match self.namespace_uri(d.ns, flavor) {
                Some(u) => u,
                None => continue, // Unbound 前缀无法声明
            };
            let local = match d.prefix {
                None => LocalName::Xmlns,
                Some(p) => {
                    let s = self.interner.resolve(p).to_string();
                    LocalName::intern(&s, &mut self.interner)
                }
            };
            self.set_attr(id, QName::new(NsId::Xmlns, local), uri);
        }
    }

    /// 命名空间的 URI：表内按 flavor，`Other` 用驻留的原文，`None` 为空串（`xmlns=""`），`Unbound` 无。
    pub fn namespace_uri(&self, ns: NsId, flavor: PartFlavor) -> Option<String> {
        match ns {
            NsId::None => Some(String::new()),
            NsId::Unbound(_) => None,
            NsId::Other(i) => Some(self.interner.resolve(i).to_string()),
            known => known.uri(flavor).map(str::to_string),
        }
    }

    // ---- 规则 F：Clean 克隆 --------------------------------------------------------------------

    /// 深拷贝子树：新节点与源节点共享 `lex`（同一字节区间会被拷贝多次），状态照抄；结果游离。
    /// 目标位置的兼容性由调用方用 [`Dom::insert_clone`] 或 `move_within_part` 的规则处理。
    pub fn clone_subtree(&mut self, subtree: NodeId) -> NodeId {
        let mut map: Vec<(NodeId, Option<NodeId>)> = Vec::new(); // (源, 新父)
        let mut stack = vec![(subtree, None::<NodeId>)];
        let mut root = None;
        while let Some((src, new_parent)) = stack.pop() {
            let node = self.nodes[src.idx()].clone();
            let children: Vec<NodeId> = match &node.kind {
                NodeKind::Element(e) => e.children.clone(),
                _ => Vec::new(),
            };
            let mut node = node;
            node.parent = new_parent;
            if let NodeKind::Element(e) = &mut node.kind {
                e.children.clear();
            }
            let new_id = self.push_detached(node);
            if let Some(p) = new_parent
                && let Some(pe) = self.element_mut(p)
            {
                pe.children.push(new_id);
            }
            if root.is_none() {
                root = Some(new_id);
            }
            map.push((src, Some(new_id)));
            // 子节点按原顺序：逆序压栈，但 children.push 会按出栈顺序 → 用索引占位保持顺序
            for &c in children.iter().rev() {
                stack.push((c, Some(new_id)));
            }
        }
        let root = root.expect("clone of at least one node");
        // 出栈顺序导致子节点顺序正确（逆序压栈 → 正序出栈）
        debug_assert!(self.nodes[root.idx()].parent.is_none());
        root
    }

    /// 克隆并插入到 `dest`：不兼容时在克隆根上补声明（克隆根 `SelfDirty`，后代仍 `Clean`）。
    pub fn insert_clone(&mut self, subtree: NodeId, dest: NodeId, index: usize) -> NodeId {
        let missing = self.required_decls(subtree, dest);
        let cloned = self.clone_subtree(subtree);
        if !missing.is_empty() {
            self.add_declarations(cloned, &missing);
        }
        self.insert_child(dest, index, cloned);
        cloned
    }

    // ---- XML-14 -------------------------------------------------------------------------------

    /// 为 `New` 子树里所有需要前缀的命名空间在子树根上补声明（目标位置作用域中未绑定或绑定到别的 URI 的）。
    /// 返回补上的声明。序列化器另有兜底（`save/serialize`），但显式调用能让 `namespace_scope` 看到它们。
    pub fn declare_for_new_subtree(&mut self, root: NodeId) -> Vec<PrefixUse> {
        let needed = self.namespaces_needing_prefix(root);
        // 含根自身已有的声明：重复调用不再补
        let scope = self.namespace_scope(root);
        let mut decls = Vec::new();
        for ns in needed {
            if scope.prefix_for(ns).is_some() {
                continue;
            }
            let prefix = self.pick_prefix(ns, &scope, &decls);
            decls.push(PrefixUse { prefix, ns });
        }
        if !decls.is_empty() {
            self.add_declarations(root, &decls);
        }
        decls
    }

    /// 子树中没有原始写法的元素名 / 属性名所用的命名空间（去重、按出现顺序）。
    pub fn namespaces_needing_prefix(&self, root: NodeId) -> Vec<NsId> {
        let mut out: Vec<NsId> = Vec::new();
        for id in self.descendants(root) {
            let Some(e) = self.element(id) else { continue };
            if self.node(id).dirty == Dirty::Deleted {
                continue;
            }
            let mut add = |ns: NsId| {
                if !matches!(ns, NsId::None | NsId::Xml | NsId::Xmlns | NsId::Unbound(_))
                    && !out.contains(&ns)
                {
                    out.push(ns);
                }
            };
            if e.lex_name.is_none() {
                add(e.name.ns);
            }
            for a in &e.attrs {
                if a.lex_name.is_none() {
                    add(a.name.ns);
                }
            }
        }
        out
    }

    /// 选前缀：规范前缀（`PKG-09`）若在作用域中空闲则用，否则 `ns1`、`ns2`…。
    pub fn pick_prefix(
        &mut self,
        ns: NsId,
        scope: &crate::xml::ns::Scope,
        pending: &[PrefixUse],
    ) -> Option<Interned> {
        let taken = |dom: &Dom, p: Option<Interned>| -> bool {
            scope.lookup(p).is_some_and(|bound| bound != ns)
                || pending.iter().any(|d| d.prefix == p && d.ns != ns)
                || {
                    // 已在根声明里被别的 URI 占用
                    let _ = dom;
                    false
                }
        };
        if let Some(canon) = ns.canonical_prefix()
            && !canon.is_empty()
        {
            let p = self.interner.intern(canon);
            if !taken(self, Some(p)) {
                return Some(p);
            }
        }
        let mut n = 1;
        loop {
            let candidate = format!("ns{n}");
            let p = self.interner.intern(&candidate);
            if !taken(self, Some(p)) {
                return Some(p);
            }
            n += 1;
        }
    }

    /// 调试断言：非 `Clean` 节点的祖先不为 `Clean`；`Clean` 节点的后代全为 `Clean`（`XML-12` 不变式）。
    pub fn check_dirty_invariants(&self) -> Result<(), String> {
        for id in self.descendants(self.root()) {
            let d = self.node(id).dirty;
            if d != Dirty::Clean {
                for a in self.ancestors(id) {
                    if self.node(a).dirty == Dirty::Clean {
                        return Err(format!(
                            "node {} is {:?} but ancestor {} is Clean",
                            id.0, d, a.0
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;
    use crate::save::serialize;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap()
    }

    fn doc() -> (Dom, NodeId, NodeId, NodeId, NodeId) {
        let dom = parse(&format!(
            "<w:document xmlns:w=\"{W}\"><w:body><w:p w:rsidR=\"1\"><w:r><w:t>old</w:t></w:r><w:r><w:t>keep</w:t></w:r></w:p><w:p/></w:body></w:document>"
        ));
        let root = dom.root();
        let body = dom.children(root)[0];
        let p = dom.children(body)[0];
        let r = dom.children(p)[0];
        let t = dom.children(r)[0];
        (dom, body, p, r, t)
    }

    #[test]
    fn xml_12_dirty_propagation() {
        let (mut dom, body, p, r, t) = doc();
        let text = dom.children(t)[0];
        dom.set_text(text, "new");
        assert_eq!(dom.node(text).dirty, Dirty::SelfDirty);
        for id in [t, r, p, body, dom.root()] {
            assert_eq!(dom.node(id).dirty, Dirty::DescendantDirty, "node {}", id.0);
        }
        dom.check_dirty_invariants().unwrap();
        let out = serialize(&dom).unwrap();
        let out = std::str::from_utf8(&out).unwrap();
        assert!(
            out.contains(
                "<w:p w:rsidR=\"1\"><w:r><w:t>new</w:t></w:r><w:r><w:t>keep</w:t></w:r></w:p>"
            ),
            "{out}"
        );
        // 再改一次：已 DescendantDirty 的祖先不变
        dom.set_attr(t, QName::new(NsId::Xml, LocalName::Space), "preserve");
        assert_eq!(dom.node(t).dirty, Dirty::SelfDirty);
        assert_eq!(dom.node(r).dirty, Dirty::DescendantDirty);
    }

    #[test]
    fn xml_12_delete_and_detach() {
        let (mut dom, _body, p, r, _t) = doc();
        dom.delete(r);
        assert_eq!(dom.node(r).dirty, Dirty::Deleted);
        assert_eq!(dom.node(p).dirty, Dirty::DescendantDirty);
        assert_eq!(dom.children(p).len(), 2, "Deleted stays in children");
        assert_eq!(dom.semantic_children(p).count(), 1);
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(out.contains("<w:p w:rsidR=\"1\"><w:r><w:t>keep</w:t></w:r></w:p>"), "{out}");
        dom.check_dirty_invariants().unwrap();
        // detach 后再插回别处
        let r2 = dom.children(p)[1];
        assert_eq!(dom.detach(r2), Some(1));
        assert_eq!(dom.children(p).len(), 1);
        let p2 = dom.children(dom.children(dom.root())[0])[1];
        dom.insert_child(p2, 0, r2);
        assert_eq!(dom.node(p2).dirty, Dirty::DescendantDirty);
        assert_eq!(dom.node(r2).dirty, Dirty::Clean, "moved Clean subtree stays Clean");
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(
            out.contains("<w:p w:rsidR=\"1\"></w:p><w:p><w:r><w:t>keep</w:t></w:r></w:p>"),
            "{out}"
        );
        dom.check_dirty_invariants().unwrap();
    }

    #[test]
    fn xml_12_move_keeps_clean_when_compatible() {
        let mut dom = parse(&format!("<a xmlns:w=\"{W}\"><b><w:x/></b><c xmlns:w=\"{W}\"/></a>"));
        let root = dom.root();
        let b = dom.children(root)[0];
        let c = dom.children(root)[1];
        let x = dom.children(b)[0];
        dom.move_within_part(x, c, 0);
        assert_eq!(dom.node(x).dirty, Dirty::Clean);
        assert_eq!(dom.parent(x), Some(c));
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert_eq!(out, format!("<a xmlns:w=\"{W}\"><b></b><c xmlns:w=\"{W}\"><w:x/></c></a>"));
        dom.check_dirty_invariants().unwrap();
    }

    #[test]
    fn xml_12_move_adds_xmlns_when_incompatible() {
        let mut dom = parse(
            "<a><b xmlns:x=\"urn:x\"><x:s><x:t k=\"v\"/></x:s></b><c xmlns:x=\"urn:other\"/><d/></a>",
        );
        let root = dom.root();
        let b = dom.children(root)[0];
        let c = dom.children(root)[1];
        let d = dom.children(root)[2];
        let s = dom.children(b)[0];
        let t = dom.children(s)[0];
        // 移到 d：x 未声明 → 子树根补 xmlns:x
        dom.move_within_part(s, d, 0);
        assert_eq!(dom.node(s).dirty, Dirty::SelfDirty);
        assert_eq!(dom.node(t).dirty, Dirty::Clean, "descendants stay Clean");
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert_eq!(
            out,
            "<a><b xmlns:x=\"urn:x\"></b><c xmlns:x=\"urn:other\"/><d><x:s xmlns:x=\"urn:x\"><x:t k=\"v\"/></x:s></d></a>"
        );
        // 移到 c：x 绑定到别的 URI → 同样补声明（内层遮蔽）
        dom.move_within_part(s, c, 0);
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(
            out.contains(
                "<c xmlns:x=\"urn:other\"><x:s xmlns:x=\"urn:x\"><x:t k=\"v\"/></x:s></c>"
            ),
            "{out}"
        );
        assert_eq!(
            dom.element(s).unwrap().attrs.len(),
            1,
            "declaration added once, then replaced in place"
        );
        dom.check_dirty_invariants().unwrap();
    }

    #[test]
    fn xml_12_clone_shares_lex() {
        let mut dom = parse(&format!(
            "<w:tbl xmlns:w=\"{W}\"><w:tr><w:tc><w:p>a</w:p></w:tc></w:tr></w:tbl>"
        ));
        let tbl = dom.root();
        let tr = dom.children(tbl)[0];
        let copy = dom.insert_clone(tr, tbl, 1);
        assert_eq!(dom.node(copy).dirty, Dirty::Clean);
        assert_eq!(dom.node(copy).lex, dom.node(tr).lex);
        assert_eq!(dom.children(copy).len(), 1);
        assert_eq!(dom.parent(dom.children(copy)[0]), Some(copy));
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert_eq!(
            out,
            format!(
                "<w:tbl xmlns:w=\"{W}\"><w:tr><w:tc><w:p>a</w:p></w:tc></w:tr><w:tr><w:tc><w:p>a</w:p></w:tc></w:tr></w:tbl>"
            )
        );
        // 克隆到不兼容位置：克隆根补声明
        let mut dom2 = parse(&format!("<a><b xmlns:w=\"{W}\"><w:r/></b><c/></a>"));
        let b = dom2.children(dom2.root())[0];
        let c = dom2.children(dom2.root())[1];
        let r = dom2.children(b)[0];
        let copy = dom2.insert_clone(r, c, 0);
        assert_eq!(dom2.node(copy).dirty, Dirty::SelfDirty);
        let out = String::from_utf8(serialize(&dom2).unwrap()).unwrap();
        assert_eq!(
            out,
            format!("<a><b xmlns:w=\"{W}\"><w:r/></b><c><w:r xmlns:w=\"{W}\"/></c></a>")
        );
        dom2.check_dirty_invariants().unwrap();
    }

    #[test]
    fn xml_12_remove_attr_and_rename() {
        let mut dom = parse("<a x=\"1\" y='2' x=\"3\"><b/></a>");
        let root = dom.root();
        assert!(dom.remove_attr(root, QName::new(NsId::None, LocalName::X)));
        assert_eq!(dom.element(root).unwrap().attrs.len(), 1, "all duplicates removed");
        assert!(!dom.remove_attr(root, QName::new(NsId::None, LocalName::X)));
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert_eq!(out, "<a y='2'><b/></a>");
    }
}
