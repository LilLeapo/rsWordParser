//! MCE（`XML-09`）与语义遍历（`XML-10`）。
//!
//! 解析后一次遍历为每个元素计算 [`Mce`]：`mc:AlternateContent` 选分支、`mc:Ignorable` 标记可忽略元素、
//! `mc:ProcessContent` 标记透明容器、`mc:MustUnderstand` 记诊断。非 active 分支与可忽略元素都保留在 DOM，
//! 只对 [`Dom::semantic_children`] 不可见。

use crate::diag::{DiagCode, Diagnostic};
use crate::xml::Dirty;
use crate::xml::dom::{Dom, MceRole, NodeId, NodeKind};
use crate::xml::interner::Interned;
use crate::xml::names::{LocalName, NsId, QName};
use crate::xml::ns::{Scope, push_decls};

/// 默认的已理解命名空间集合（`PKG-09`）。
///
/// `c14`（Word 2010 的图表扩展）在里面：图表 part 里 `c:style` 一律包在 `mc:AlternateContent` 里，
/// `Choice Requires="c14"` 放 `c14:style`（101–148）、Fallback 放 `c:style`（1–48），两者是同一个值的两种写法，
/// 但 Word 2010+ 与 TS 读的都是 Choice 那份——语料 `m6-chart__043` 的 Choice 与 Fallback 故意不一致，
/// 走 Fallback 会把调色板认错（M6 6.1）。
pub const DEFAULT_UNDERSTOOD: &[NsId] = &[
    NsId::Wps,
    NsId::Wpg,
    // 真实 Word 的绘图画布：`mc:Choice Requires="wpc"` 里是 `wpc:wpc`（子形状是 `wps:wsp` / `pic:pic`），
    // Fallback 是 VML `v:group`。本引擎按组处理画布子形状，所以算理解（`corpus/real/canvas-*`）。
    NsId::Wpc,
    NsId::Wp14,
    NsId::W14,
    NsId::W15,
    NsId::Cx,
    NsId::C14,
];

/// `mc:ProcessContent` 里的一项：`p:x`（限定名）或 `p:*`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessPattern {
    ns: NsId,
    local: Option<LocalName>,
}

impl Dom {
    /// 为全部元素重算 MCE 状态。`Dom::parse` 已用 [`DEFAULT_UNDERSTOOD`] 调过；已理解集合可配置时再调。
    pub fn compute_mce(&mut self, understood: &[NsId]) {
        let mut scope = Scope::default();
        // 继承的可忽略命名空间与 ProcessContent 模式
        let mut ignorable: Vec<NsId> = Vec::new();
        let mut patterns: Vec<ProcessPattern> = Vec::new();
        let mut diags: Vec<Diagnostic> = Vec::new();
        enum Step {
            Enter(NodeId),
            Exit(usize, usize, usize),
        }
        let mut stack = vec![Step::Enter(self.root())];
        while let Some(step) = stack.pop() {
            match step {
                Step::Exit(s, i, p) => {
                    scope.truncate(s);
                    ignorable.truncate(i);
                    patterns.truncate(p);
                }
                Step::Enter(id) => {
                    let (name, children) = match &self.node(id).kind {
                        NodeKind::Element(e) => (e.name, e.children.clone()),
                        _ => continue,
                    };
                    stack.push(Step::Exit(scope.len(), ignorable.len(), patterns.len()));
                    {
                        let e = self.element(id).expect("element");
                        push_decls(self, e, &mut scope);
                    }
                    // 元素自身对语义的可见性：由祖先（含自身）的 Ignorable 决定
                    let is_ignorable =
                        ignorable.contains(&name.ns) && !understood.contains(&name.ns);
                    let process_content = is_ignorable
                        && patterns
                            .iter()
                            .any(|p| p.ns == name.ns && p.local.is_none_or(|l| l == name.local));
                    // 自身的 mc:* 属性
                    let mut must_understand = false;
                    for token in self.attr_list(id, QName::new(NsId::Mc, LocalName::Ignorable)) {
                        if let Some(ns) = self.resolve_prefix_in(&scope, &token) {
                            ignorable.push(ns);
                        }
                    }
                    for token in self.attr_list(id, QName::new(NsId::Mc, LocalName::ProcessContent))
                    {
                        let (prefix, local) =
                            token.split_once(':').map_or((token.as_str(), "*"), |(p, l)| (p, l));
                        if let Some(ns) = self.resolve_prefix_in(&scope, prefix) {
                            let local = if local == "*" {
                                None
                            } else {
                                LocalName::known(local)
                                    .or_else(|| self.interner().get(local).map(LocalName::Other))
                            };
                            if local.is_some() || token.ends_with(":*") || !token.contains(':') {
                                patterns.push(ProcessPattern { ns, local });
                            }
                        }
                    }
                    for token in self.attr_list(id, QName::new(NsId::Mc, LocalName::MustUnderstand))
                    {
                        let ns = self.resolve_prefix_in(&scope, &token);
                        if !ns.is_some_and(|n| understood.contains(&n)) {
                            must_understand = true;
                            diags.push(Diagnostic::pre_existing(
                                self.part(),
                                self.node(id).lex.as_ref().map(|l| l.open.clone()),
                                DiagCode::XmlMustUnderstand,
                                format!("mc:MustUnderstand names namespace prefix `{token}` which is not understood"),
                            ));
                        }
                    }
                    // 角色与分支选择
                    let role = if name.ns == NsId::Mc {
                        match name.local {
                            LocalName::AlternateContent => MceRole::AlternateContent,
                            LocalName::Choice => MceRole::Choice,
                            LocalName::Fallback => MceRole::Fallback,
                            _ => MceRole::None,
                        }
                    } else {
                        MceRole::None
                    };
                    let mut active_branch: Option<NodeId> = None;
                    if role == MceRole::AlternateContent {
                        active_branch = self.select_branch(id, &children, &scope, understood);
                        if active_branch.is_none() {
                            diags.push(Diagnostic::pre_existing(
                                self.part(),
                                self.node(id).lex.as_ref().map(|l| l.open.clone()),
                                DiagCode::XmlNoActiveBranch,
                                "mc:AlternateContent has no satisfiable Choice and no Fallback",
                            ));
                        }
                    }
                    if let Some(e) = self.element_mut(id) {
                        e.mce.role = role;
                        e.mce.ignorable = is_ignorable;
                        e.mce.process_content = process_content;
                        e.mce.must_understand = must_understand;
                        if !matches!(role, MceRole::Choice | MceRole::Fallback) {
                            e.mce.active = true;
                        }
                    }
                    if role == MceRole::AlternateContent {
                        for &c in &children {
                            if let Some(ce) = self.element_mut(c)
                                && matches!(
                                    ce.name,
                                    QName {
                                        ns: NsId::Mc,
                                        local: LocalName::Choice | LocalName::Fallback
                                    }
                                )
                            {
                                ce.mce.active = Some(c) == active_branch;
                            }
                        }
                    }
                    stack.extend(children.iter().rev().map(|&c| Step::Enter(c)));
                }
            }
        }
        self.diagnostics.extend(diags);
    }

    fn attr_list(&self, id: NodeId, name: QName) -> Vec<String> {
        self.attr_value(id, name)
            .map(|v| v.split_ascii_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// 子树里任意元素声明了 `xmlns:<prefix>` 时给出它的命名空间。只在作用域查找失败后当退路用。
    fn prefix_declared_in(&self, node: NodeId, prefix: &str) -> Option<NsId> {
        let want = self.interner().get(prefix)?;
        for n in self.descendants(node) {
            let Some(e) = self.element(n) else { continue };
            for a in &e.attrs {
                if a.name.ns == NsId::Xmlns
                    && a.name.local == LocalName::Other(want)
                    && let Some((ns, _)) = NsId::from_uri(&self.attr_str(a))
                {
                    return Some(ns);
                }
            }
        }
        None
    }

    fn resolve_prefix_in(&self, scope: &Scope, prefix: &str) -> Option<NsId> {
        if prefix == "xml" {
            return Some(NsId::Xml);
        }
        let i: Interned = self.interner().get(prefix)?;
        scope.lookup(Some(i))
    }

    /// 第一个 `Requires` 全部已理解的 `Choice`，否则第一个 `Fallback`。
    fn select_branch(
        &self,
        _alt: NodeId,
        children: &[NodeId],
        scope: &Scope,
        understood: &[NsId],
    ) -> Option<NodeId> {
        let requires_q = QName::new(NsId::None, LocalName::Requires);
        let mut fallback = None;
        for &c in children {
            let Some(e) = self.element(c) else { continue };
            if e.name.ns != NsId::Mc {
                continue;
            }
            match e.name.local {
                LocalName::Choice => {
                    let requires =
                        self.attr_value(c, requires_q).map(|v| v.into_owned()).unwrap_or_default();
                    let satisfied = requires.split_ascii_whitespace().all(|p| {
                        self.resolve_prefix_in(scope, p)
                            // 前缀在 `mc:Choice` 处不在作用域，但分支**里面**声明了它：合成
                            // 语料常把 `xmlns:wps` 写在 `wps:wsp` 自己身上，`Requires="wps"`
                            // 于是解析不出来。意图毫无歧义，按分支内的声明认（`docs/04` §8）。
                            .or_else(|| self.prefix_declared_in(c, p))
                            .is_some_and(|ns| understood.contains(&ns))
                    });
                    if satisfied {
                        return Some(c);
                    }
                }
                LocalName::Fallback if fallback.is_none() => fallback = Some(c),
                _ => {}
            }
        }
        fallback
    }

    /// `XML-10`：语义子节点。跳过 `Deleted`；`AlternateContent` 只产出 active 分支的子节点；跳过可忽略元素；
    /// `ProcessContent` 命中的元素产出其子节点而非自身。模型层禁止直接读 `children`。
    pub fn semantic_children(&self, node: NodeId) -> SemanticChildren<'_> {
        SemanticChildren { dom: self, stack: vec![(self.children(node), 0)] }
    }

    /// `XML-10`：语义前序遍历（含 `node` 自身），逐层走 [`Dom::semantic_children`]。
    ///
    /// 与 [`Dom::descendants`] 的区别是这里看不见非 active 的 `mc:Choice` / `mc:Fallback`
    /// 分支。凡是要按语义读子树的地方（绘图、VML、文本框）都用这个，否则会读到未生效的分支——
    /// 语料里有 `mc:Choice Requires="ma"`（未知前缀）里写着坏 `r:embed`、Fallback 里才是真图的文档。
    pub fn semantic_descendants(&self, node: NodeId) -> SemanticDescendants<'_> {
        SemanticDescendants { dom: self, stack: vec![node], scratch: Vec::new() }
    }
}

/// [`Dom::semantic_descendants`] 的迭代器。显式栈，不递归（语料里有几千层嵌套）。
pub struct SemanticDescendants<'a> {
    dom: &'a Dom,
    stack: Vec<NodeId>,
    scratch: Vec<NodeId>,
}

impl Iterator for SemanticDescendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.stack.pop()?;
        let dom = self.dom;
        self.scratch.clear();
        self.scratch.extend(dom.semantic_children(id));
        self.stack.extend(self.scratch.iter().rev().copied());
        Some(id)
    }
}

pub struct SemanticChildren<'a> {
    dom: &'a Dom,
    /// (子节点切片, 下一个索引)；进入透明容器时压栈。
    stack: Vec<(&'a [NodeId], usize)>,
}

impl Iterator for SemanticChildren<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        loop {
            let (slice, idx) = self.stack.last_mut()?;
            if *idx >= slice.len() {
                self.stack.pop();
                continue;
            }
            let id = slice[*idx];
            *idx += 1;
            let node = self.dom.node(id);
            if node.dirty == Dirty::Deleted {
                continue;
            }
            let NodeKind::Element(e) = &node.kind else { return Some(id) };
            match e.mce.role {
                MceRole::AlternateContent => {
                    // 只有 active 分支的子节点可见
                    if let Some(&branch) = e.children.iter().find(|&&c| {
                        self.dom.element(c).is_some_and(|b| {
                            b.mce.active
                                && matches!(b.mce.role, MceRole::Choice | MceRole::Fallback)
                        })
                    }) {
                        self.stack.push((self.dom.children(branch), 0));
                    }
                    continue;
                }
                MceRole::Choice | MceRole::Fallback => {
                    // 直接遇到分支元素（父不是 AlternateContent 的异常写法）：按透明容器处理 active 的
                    if e.mce.active {
                        self.stack.push((self.dom.children(id), 0));
                    }
                    continue;
                }
                MceRole::None => {}
            }
            if e.mce.ignorable {
                if e.mce.process_content {
                    self.stack.push((self.dom.children(id), 0));
                }
                continue;
            }
            return Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;
    use crate::save::serialize;

    const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap()
    }

    fn names(dom: &Dom, ids: impl Iterator<Item = NodeId>) -> Vec<String> {
        ids.filter_map(|id| dom.lex_name(id).map(str::to_string)).collect()
    }

    #[test]
    fn xml_09_choice_selected_when_requires_understood() {
        let src = format!(
            "<w:r xmlns:w=\"{W}\" xmlns:mc=\"{MC}\" xmlns:wps=\"{WPS}\"><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing/></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r>"
        );
        let dom = parse(&src);
        assert_eq!(names(&dom, dom.semantic_children(dom.root())), vec!["w:drawing"]);
        let alt = dom.children(dom.root())[0];
        assert_eq!(dom.element(alt).unwrap().mce.role, MceRole::AlternateContent);
        let choice = dom.children(alt)[0];
        let fallback = dom.children(alt)[1];
        assert!(dom.element(choice).unwrap().mce.active);
        assert!(!dom.element(fallback).unwrap().mce.active);
        assert_eq!(dom.children(dom.root()).len(), 1, "DOM keeps the AlternateContent");
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_09_fallback_selected_when_requires_unknown() {
        let src = format!(
            "<w:r xmlns:w=\"{W}\" xmlns:mc=\"{MC}\" xmlns:foo=\"urn:foo\" xmlns:wps=\"{WPS}\"><mc:AlternateContent><mc:Choice Requires=\"wps foo\"><w:a/></mc:Choice><mc:Choice Requires=\"undeclared\"><w:b/></mc:Choice><mc:Fallback><w:c/><w:d/></mc:Fallback></mc:AlternateContent><w:t>x</w:t></w:r>"
        );
        let dom = parse(&src);
        assert_eq!(names(&dom, dom.semantic_children(dom.root())), vec!["w:c", "w:d", "w:t"]);
        assert!(dom.diagnostics().is_empty());
        // 没有可选分支也没有 Fallback → 无 active，记诊断
        let dom2 = parse(&format!(
            "<w:r xmlns:w=\"{W}\" xmlns:mc=\"{MC}\"><mc:AlternateContent><mc:Choice Requires=\"nope\"><w:a/></mc:Choice></mc:AlternateContent></w:r>"
        ));
        assert!(dom2.semantic_children(dom2.root()).next().is_none());
        assert!(dom2.diagnostics().iter().any(|d| d.code == DiagCode::XmlNoActiveBranch));
    }

    #[test]
    fn xml_09_ignorable_hidden_but_preserved_and_understood_not_hidden() {
        let src = format!(
            "<w:p xmlns:w=\"{W}\" xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"x w14\"><x:junk><w:r/></x:junk><w14:glow/><w:r><w:t x:attr=\"1\">t</w:t></w:r></w:p>"
        );
        let dom = parse(&src);
        assert_eq!(
            names(&dom, dom.semantic_children(dom.root())),
            vec!["w14:glow", "w:r"],
            "w14 is understood, x is not"
        );
        assert_eq!(dom.children(dom.root()).len(), 3);
        let junk = dom.children(dom.root())[0];
        assert!(dom.element(junk).unwrap().mce.ignorable);
        assert!(!dom.element(junk).unwrap().mce.process_content);
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
        // Ignorable 只作用于声明它的元素子树
        let dom2 = parse(&format!(
            "<a xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\"><b mc:Ignorable=\"x\"><x:i/></b><x:j/></a>"
        ));
        let b = dom2.children(dom2.root())[0];
        assert!(dom2.semantic_children(b).next().is_none());
        assert_eq!(names(&dom2, dom2.semantic_children(dom2.root())), vec!["b", "x:j"]);
    }

    #[test]
    fn xml_09_process_content_flattens() {
        let src = format!(
            "<w:body xmlns:w=\"{W}\" xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\" mc:Ignorable=\"x\" mc:ProcessContent=\"x:wrap\"><x:wrap><w:p/><x:other><w:tbl/></x:other></x:wrap><w:p/></w:body>"
        );
        let dom = parse(&src);
        assert_eq!(
            names(&dom, dom.semantic_children(dom.root())),
            vec!["w:p", "w:p"],
            "wrap is flattened, other is hidden"
        );
        let wildcard = parse(&format!(
            "<a xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\" mc:Ignorable=\"x\" mc:ProcessContent=\"x:*\"><x:one><b/></x:one><x:two><c/></x:two></a>"
        ));
        assert_eq!(names(&wildcard, wildcard.semantic_children(wildcard.root())), vec!["b", "c"]);
    }

    #[test]
    fn xml_09_must_understand_is_a_diagnostic_not_an_error() {
        let dom = parse(&format!(
            "<a xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\" xmlns:wps=\"{WPS}\"><b mc:MustUnderstand=\"x\"/><c mc:MustUnderstand=\"wps\"/></a>"
        ));
        let b = dom.children(dom.root())[0];
        let c = dom.children(dom.root())[1];
        assert!(dom.element(b).unwrap().mce.must_understand);
        assert!(!dom.element(c).unwrap().mce.must_understand);
        assert_eq!(
            dom.diagnostics().iter().filter(|d| d.code == DiagCode::XmlMustUnderstand).count(),
            1
        );
    }

    #[test]
    fn xml_10_semantic_children_skips_deleted_and_yields_text() {
        let mut dom = parse("<a><b/>text<c/></a>");
        let b = dom.children(dom.root())[0];
        dom.node_mut(b).dirty = Dirty::Deleted;
        let kids: Vec<NodeId> = dom.semantic_children(dom.root()).collect();
        assert_eq!(kids.len(), 2);
        assert_eq!(dom.text(kids[0]).as_deref(), Some("text"));
        assert_eq!(dom.lex_name(kids[1]), Some("c"));
    }

    #[test]
    fn xml_09_recompute_with_custom_understood_set() {
        let src = format!("<a xmlns:mc=\"{MC}\" xmlns:x=\"urn:x\" mc:Ignorable=\"x\"><x:e/></a>");
        let mut dom = parse(&src);
        assert!(dom.semantic_children(dom.root()).next().is_none());
        let x = NsId::Other(dom.interner().get("urn:x").unwrap());
        dom.compute_mce(&[x]);
        assert_eq!(names(&dom, dom.semantic_children(dom.root())), vec!["x:e"]);
    }
}
