//! 规范化文本（`TEST-05` / `COMPAT-08` 的等价比较）：把子树写成与前缀、属性顺序、命名空间声明、
//! 元素间空白、`Deleted` 节点无关的字符串——两棵树的规范化文本相同 ⇔ 任何 XPath 子集表达式在两者上结果相同。
//!
//! 名字写成 `{uri}local`（`Unbound` 前缀若是规范前缀按其命名空间，否则写 `{?prefix}local`），属性按名字排序，`xmlns:*` 忽略；
//! 文本只在 `w:t / w:delText / w:instrText / m:t` 这类文本容器里逐字保留，其他地方纯空白文本丢弃。
//! `ignore_attr(element, attr)` 让调用方剔除不参与比较的属性（例如 `w14:paraId` / `w:rsid*`）。

use std::fmt::Write as _;

use crate::xml::names::{LocalName, NsId, QName};
use crate::xml::{Dirty, Dom, NodeId, NodeKind};

/// 规范化选项。
pub struct CanonOptions<'a> {
    /// `(元素节点, 属性名) -> 是否忽略该属性`（拿节点而不是元素名：有的容忍要看子元素，比如墨迹锚的 `relativeHeight`）。
    pub ignore_attr: &'a dyn Fn(&Dom, NodeId, QName) -> bool,
}

impl Default for CanonOptions<'_> {
    fn default() -> Self {
        Self { ignore_attr: &|_, _, _| false }
    }
}

fn qname_str(dom: &Dom, q: QName) -> String {
    let interner = dom.interner();
    let local = q.local.as_str(interner);
    match q.ns {
        NsId::None => local.to_string(),
        // 未绑定但是规范前缀（TS 逐字写回片段时常见的 `o:` / `v:`）：按该前缀的命名空间比较
        NsId::Unbound(p) => match crate::xml::xpath::prefix_to_ns(interner.resolve(p))
            .and_then(|ns| ns.uri(crate::package::PartFlavor::Transitional))
        {
            Some(uri) => format!("{{{uri}}}{local}"),
            None => format!("{{?{}}}{local}", interner.resolve(p)),
        },
        NsId::Other(id) => format!("{{{}}}{local}", interner.resolve(id)),
        ns => match ns.uri(crate::package::PartFlavor::Transitional) {
            Some(uri) => format!("{{{uri}}}{local}"),
            None => format!("{{{}}}{local}", ns.describe(interner)),
        },
    }
}

fn is_text_container(q: QName) -> bool {
    (q.ns == NsId::W && matches!(q.local, LocalName::T | LocalName::DelText | LocalName::InstrText))
        || (q.ns == NsId::M && q.local == LocalName::T)
}

/// 子树的规范化文本（含 `root` 自身）。Strict 与 Transitional 的已知命名空间写成同一 URI。
/// 迭代实现（病态语料有数千层嵌套）。
pub fn canonical(dom: &Dom, root: NodeId, opts: &CanonOptions<'_>) -> String {
    enum Step {
        Enter(NodeId, bool),
        Exit,
    }
    let mut out = String::new();
    let mut stack = vec![Step::Enter(root, false)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Exit => out.push_str("</>"),
            Step::Enter(id, in_text) => {
                let node = dom.node(id);
                if node.dirty == Dirty::Deleted {
                    continue;
                }
                match &node.kind {
                    NodeKind::Text(_) => {
                        let t = dom.text(id).unwrap_or_default();
                        if in_text || !t.trim().is_empty() {
                            out.push_str("#text(");
                            for c in t.chars() {
                                if matches!(c, '(' | ')' | '\\') {
                                    out.push('\\');
                                }
                                out.push(c);
                            }
                            out.push(')');
                        }
                    }
                    NodeKind::Opaque => {}
                    NodeKind::Element(e) => {
                        out.push('<');
                        out.push_str(&qname_str(dom, e.name));
                        let mut attrs: Vec<(String, String)> = e
                            .attrs
                            .iter()
                            .filter(|a| {
                                a.name.ns != NsId::Xmlns && !(opts.ignore_attr)(dom, id, a.name)
                            })
                            .map(|a| (qname_str(dom, a.name), dom.attr_str(a).into_owned()))
                            .collect();
                        attrs.sort();
                        for (k, v) in attrs {
                            let _ = write!(out, " {k}={v:?}");
                        }
                        out.push('>');
                        let text_container = is_text_container(e.name);
                        stack.push(Step::Exit);
                        for &c in e.children.iter().rev() {
                            stack.push(Step::Enter(c, text_container));
                        }
                    }
                }
            }
        }
    }
    out
}

/// 两个规范化文本的首个差异附近的片段（调试输出用）。
pub fn first_difference(a: &str, b: &str) -> Option<(String, String)> {
    if a == b {
        return None;
    }
    let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let start = a[..common].char_indices().rev().nth(80).map_or(0, |(i, _)| i);
    let cut = |s: &str| {
        let end = s[start..].char_indices().nth(200).map_or(s.len(), |(i, _)| start + i);
        s[start..end].to_string()
    };
    Some((cut(a), cut(b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    #[test]
    fn test_05_canonical_ignores_prefixes_attr_order_and_whitespace() {
        let a = Dom::parse(
            PartId(0),
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
  <w:p><w:r><w:t xml:space="preserve">a b</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .unwrap();
        let b = Dom::parse(
            PartId(0),
            br#"<x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="urn:r"><x:body><x:p><x:r><x:t xml:space="preserve">a b</x:t></x:r></x:p></x:body></x:document>"#,
        )
        .unwrap();
        let opts = CanonOptions::default();
        assert_eq!(canonical(&a, a.root(), &opts), canonical(&b, b.root(), &opts));
        let c = Dom::parse(
            PartId(0),
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t xml:space="preserve">a  b</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .unwrap();
        let (x, y) =
            first_difference(&canonical(&a, a.root(), &opts), &canonical(&c, c.root(), &opts))
                .unwrap();
        assert_ne!(x, y);
    }
}
