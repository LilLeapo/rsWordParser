//! 命名空间作用域（`XML-11`，`docs/03` §3.4）。
//!
//! 同 part 不等于同前缀上下文：任何涉及位置的判断都用 [`Dom::namespace_scope`]。
//! 当前不缓存（每次沿祖先链 O(深度) 计算）；需要时再按子树失效缓存。

use std::collections::HashSet;

use crate::xml::dom::{Dom, Element, NodeId, NodeKind};
use crate::xml::interner::Interned;
use crate::xml::names::{LocalName, NsId};

/// 某个节点位置上实际生效的 `prefix → 命名空间` 映射。前缀 `None` 表示默认命名空间。
/// 内层声明在后，查找从末尾向前扫描（内层遮蔽外层）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    bindings: Vec<(Option<Interned>, NsId)>,
}

impl Scope {
    pub fn push(&mut self, prefix: Option<Interned>, ns: NsId) {
        self.bindings.push((prefix, ns));
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn truncate(&mut self, len: usize) {
        self.bindings.truncate(len);
    }

    /// 前缀当前绑定的命名空间；`xml` 前缀由调用方处理（恒为 [`NsId::Xml`]）。
    pub fn lookup(&self, prefix: Option<Interned>) -> Option<NsId> {
        self.bindings.iter().rev().find(|(p, _)| *p == prefix).map(|(_, ns)| *ns)
    }

    /// 当前**有效**（未被内层遮蔽）地绑定到 `ns` 的一个前缀。返回 `Some(None)` 表示默认命名空间。
    pub fn prefix_for(&self, ns: NsId) -> Option<Option<Interned>> {
        let mut shadowed: HashSet<Option<Interned>> = HashSet::new();
        for (p, n) in self.bindings.iter().rev() {
            if !shadowed.insert(*p) {
                continue;
            }
            if *n == ns {
                return Some(*p);
            }
        }
        None
    }

    /// 有效绑定（去掉被遮蔽的），内层优先。
    pub fn effective(&self) -> Vec<(Option<Interned>, NsId)> {
        let mut seen: HashSet<Option<Interned>> = HashSet::new();
        self.bindings.iter().rev().filter(|(p, _)| seen.insert(*p)).copied().collect()
    }
}

/// 把元素自身的 `xmlns` / `xmlns:*` 声明压入作用域。
pub(crate) fn push_decls(dom: &Dom, e: &Element, scope: &mut Scope) {
    for a in &e.attrs {
        if a.name.ns != NsId::Xmlns {
            continue;
        }
        let uri = dom.attr_str(a);
        let ns = if uri.is_empty() {
            NsId::None
        } else {
            match NsId::from_uri(&uri) {
                Some((id, _)) => id,
                None => match dom.interner().get(&uri) {
                    Some(i) => NsId::Other(i),
                    None => continue,
                },
            }
        };
        let prefix = if a.name.local == LocalName::Xmlns {
            None
        } else {
            match a.name.local {
                LocalName::Other(i) => Some(i),
                known => dom.interner().get(known.known_str().unwrap_or_default()),
            }
        };
        if prefix.is_none() && a.name.local != LocalName::Xmlns {
            continue; // 前缀未驻留（不应发生）
        }
        scope.push(prefix, ns);
    }
}

/// 原始限定名的前缀部分（`w:p` → `Some("w")`；`p` → `None`）。
fn lex_prefix(qname: &str) -> Option<&str> {
    qname.split_once(':').map(|(p, _)| p)
}

/// 一处"对外层作用域的依赖"：某前缀（`None` = 默认命名空间）必须绑定到 `ns`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrefixUse {
    pub prefix: Option<Interned>,
    pub ns: NsId,
}

impl Dom {
    /// 从根到 `at` 沿祖先链收集 `xmlns` 声明得到的有效绑定（含 `at` 自身的声明）。
    pub fn namespace_scope(&self, at: NodeId) -> Scope {
        let mut chain: Vec<NodeId> = self.ancestors(at).collect();
        chain.reverse();
        chain.push(at);
        let mut scope = Scope::default();
        for id in chain {
            if let Some(e) = self.element(id) {
                push_decls(self, e, &mut scope);
            }
        }
        scope
    }

    /// 子树对外层作用域的依赖：子树内**实际使用**且**未在子树内部声明**的前缀（元素名、属性名、
    /// `mc:Ignorable` / `mc:ProcessContent` / `mc:MustUnderstand` / `Requires` 里的前缀）及其当前绑定。
    /// 没有原始写法的 `New` 节点不计入：它们的前缀在序列化时按目标作用域生成（`XML-14`）。
    pub fn external_prefix_uses(&self, subtree: NodeId) -> Vec<PrefixUse> {
        let outer = self.parent(subtree).map_or_else(Scope::default, |p| self.namespace_scope(p));
        let mut internal = Scope::default();
        let mut uses: Vec<PrefixUse> = Vec::new();
        let mut seen: HashSet<PrefixUse> = HashSet::new();
        enum Step {
            Enter(NodeId),
            Exit(usize),
        }
        let mut stack = vec![Step::Enter(subtree)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Exit(len) => internal.truncate(len),
                Step::Enter(id) => {
                    let node = self.node(id);
                    let NodeKind::Element(e) = &node.kind else { continue };
                    stack.push(Step::Exit(internal.len()));
                    push_decls(self, e, &mut internal);
                    let mut record = |prefix: Option<Interned>, ns: NsId| {
                        if internal.lookup(prefix).is_some() {
                            return; // 子树内部声明覆盖
                        }
                        let u = PrefixUse { prefix, ns };
                        if seen.insert(u) {
                            uses.push(u);
                        }
                    };
                    // 元素名
                    if let Some(r) = &e.lex_name {
                        let prefix = lex_prefix(self.lex_str(r));
                        match prefix {
                            Some("xml") | Some("xmlns") => {}
                            Some(p) => {
                                if let Some(i) = self.interner().get(p) {
                                    record(Some(i), e.name.ns);
                                }
                            }
                            None => record(None, e.name.ns),
                        }
                    }
                    // 属性名（无前缀属性无命名空间，不计）
                    for a in &e.attrs {
                        if matches!(a.name.ns, NsId::None | NsId::Xml | NsId::Xmlns) {
                            continue;
                        }
                        if let Some(r) = &a.lex_name
                            && let Some(p) = lex_prefix(self.lex_str(r))
                            && let Some(i) = self.interner().get(p)
                        {
                            record(Some(i), a.name.ns);
                        }
                    }
                    // MCE 属性里的前缀列表
                    for a in &e.attrs {
                        let is_list = (a.name.ns == NsId::Mc
                            && matches!(
                                a.name.local,
                                LocalName::Ignorable
                                    | LocalName::ProcessContent
                                    | LocalName::MustUnderstand
                            ))
                            || (a.name.ns == NsId::None
                                && a.name.local == LocalName::Requires
                                && e.name.ns == NsId::Mc);
                        if !is_list {
                            continue;
                        }
                        let value = self.attr_str(a);
                        for token in value.split_ascii_whitespace() {
                            let p = token.split_once(':').map_or(token, |(p, _)| p);
                            if p == "*" {
                                continue;
                            }
                            if let Some(i) = self.interner().get(p) {
                                let ns = internal.lookup(Some(i)).or_else(|| outer.lookup(Some(i)));
                                if let Some(ns) = ns {
                                    record(Some(i), ns);
                                }
                            }
                        }
                    }
                    stack.extend(e.children.iter().rev().map(|&c| Step::Enter(c)));
                }
            }
        }
        uses
    }

    /// 子树的全部外部前缀依赖在 `dest`（作为新父节点）的作用域中都存在且 URI 一致。
    pub fn namespace_compatible(&self, subtree: NodeId, dest: NodeId) -> bool {
        self.required_decls(subtree, dest).is_empty()
    }

    /// 移到 `dest` 下后需要补在子树根上的声明：缺失或绑定到别的命名空间的前缀。
    pub fn required_decls(&self, subtree: NodeId, dest: NodeId) -> Vec<PrefixUse> {
        let dest_scope = self.namespace_scope(dest);
        self.external_prefix_uses(subtree)
            .into_iter()
            .filter(|u| {
                let bound = dest_scope.lookup(u.prefix).unwrap_or(NsId::None);
                bound != u.ns
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap()
    }

    fn other(dom: &Dom, uri: &str) -> NsId {
        NsId::Other(dom.interner().get(uri).unwrap())
    }

    #[test]
    fn xml_11_scope_shadowing() {
        let dom = parse(
            "<a xmlns:p=\"urn:u1\" xmlns=\"urn:d\"><b xmlns:p=\"urn:u2\"><c/></b><d xmlns=\"\"/></a>",
        );
        let root = dom.root();
        let b = dom.children(root)[0];
        let c = dom.children(b)[0];
        let d = dom.children(root)[1];
        let p = dom.interner().get("p").unwrap();
        assert_eq!(dom.namespace_scope(c).lookup(Some(p)), Some(other(&dom, "urn:u2")));
        assert_eq!(dom.namespace_scope(d).lookup(Some(p)), Some(other(&dom, "urn:u1")));
        assert_eq!(dom.namespace_scope(c).lookup(None), Some(other(&dom, "urn:d")));
        assert_eq!(dom.namespace_scope(d).lookup(None), Some(NsId::None), "xmlns=\"\" undeclares");
        assert_eq!(
            dom.namespace_scope(c).prefix_for(other(&dom, "urn:u1")),
            None,
            "u1 is shadowed at c"
        );
        assert_eq!(dom.namespace_scope(c).prefix_for(other(&dom, "urn:u2")), Some(Some(p)));
        assert_eq!(dom.namespace_scope(c).effective().len(), 2);
    }

    #[test]
    fn xml_11_compatible_when_same_uri_and_required_decls_otherwise() {
        let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        let dom = parse(&format!(
            "<root xmlns:w=\"{w}\" xmlns:x=\"urn:x\"><w:p x:a=\"1\"/><q xmlns:w=\"urn:w2\"><w:r/></q><same xmlns:w=\"{w}\"/><s xmlns:z=\"urn:z\"><z:t/></s></root>"
        ));
        let root = dom.root();
        let p = dom.children(root)[0];
        let q = dom.children(root)[1];
        let same = dom.children(root)[2];
        let s = dom.children(root)[3];
        let w_i = dom.interner().get("w").unwrap();
        let x_i = dom.interner().get("x").unwrap();
        let uses = dom.external_prefix_uses(p);
        assert_eq!(
            uses,
            vec![
                PrefixUse { prefix: Some(w_i), ns: NsId::W },
                PrefixUse { prefix: Some(x_i), ns: other(&dom, "urn:x") }
            ]
        );
        assert!(dom.namespace_compatible(p, root));
        assert!(
            dom.namespace_compatible(p, same),
            "same URI under a different declaration is compatible"
        );
        assert!(!dom.namespace_compatible(p, q), "w is bound to another URI under q");
        assert_eq!(dom.required_decls(p, q), vec![PrefixUse { prefix: Some(w_i), ns: NsId::W }]);
        // 子树内部声明的前缀不构成外部依赖；无前缀的 <s> 只依赖"默认命名空间未绑定"
        let z_i = dom.interner().get("z").unwrap();
        let s_uses = dom.external_prefix_uses(s);
        assert!(s_uses.iter().all(|u| u.prefix != Some(z_i)));
        assert_eq!(s_uses, vec![PrefixUse { prefix: None, ns: NsId::None }]);
        assert!(dom.namespace_compatible(s, q));
        // 默认命名空间依赖
        let dom2 = parse("<a xmlns=\"urn:d\"><b/><c xmlns=\"urn:e\"><d/></c></a>");
        let b = dom2.children(dom2.root())[0];
        let c = dom2.children(dom2.root())[1];
        assert_eq!(
            dom2.external_prefix_uses(b),
            vec![PrefixUse { prefix: None, ns: other(&dom2, "urn:d") }]
        );
        assert!(!dom2.namespace_compatible(b, c));
        assert_eq!(dom2.required_decls(b, c)[0].prefix, None);
    }

    #[test]
    fn xml_11_mce_attribute_prefix_lists_count_as_uses() {
        let dom = parse(
            "<a xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:v=\"urn:v\"><b mc:Ignorable=\"v\"/></a>",
        );
        let b = dom.children(dom.root())[0];
        let uses = dom.external_prefix_uses(b);
        let mc = dom.interner().get("mc").unwrap();
        let v = dom.interner().get("v").unwrap();
        assert!(uses.contains(&PrefixUse { prefix: Some(mc), ns: NsId::Mc }));
        assert!(uses.contains(&PrefixUse { prefix: Some(v), ns: other(&dom, "urn:v") }));
    }
}
