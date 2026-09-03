//! part 级命名空间上下文（`PKG-09`，`docs/03` §3.4）。
//!
//! 这是生成时的偏好来源与常见情形的快路径；任何与位置相关的判断必须用
//! `Dom::namespace_scope(node)`（`XML-11`，任务 0.8）。

use std::collections::HashMap;

use crate::package::PartFlavor;
use crate::xml::{Dom, LocalName, NsId, QName};

/// MCE 选择用的已理解命名空间集合（`PKG-09`、`XML-09`）。
pub const UNDERSTOOD: &[NsId] = &[NsId::Wps, NsId::Wpg, NsId::Wp14, NsId::W14, NsId::W15, NsId::Cx];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceContext {
    /// 根元素上的 `xmlns` 声明：`(前缀, 命名空间)`，前缀 `None` 表示默认命名空间。
    pub root_decls: Vec<(Option<String>, NsId)>,
    /// 该 part 对每个命名空间惯用的前缀（取根声明；同 URI 多前缀取第一个）。
    pub preferred: HashMap<NsId, String>,
    /// 根上 `mc:Ignorable` 列出的前缀。
    pub ignorable: Vec<String>,
    pub flavor: PartFlavor,
    /// 已理解命名空间集合，可配置。
    pub understood: Vec<NsId>,
}

impl NamespaceContext {
    pub fn from_dom(dom: &Dom, flavor: PartFlavor) -> Self {
        let root = dom.root();
        let mut root_decls = Vec::new();
        let mut preferred: HashMap<NsId, String> = HashMap::new();
        if let Some(e) = dom.element(root) {
            for a in &e.attrs {
                if a.name.ns != NsId::Xmlns {
                    continue;
                }
                let uri = dom.attr_str(a);
                let ns = match NsId::from_uri(&uri) {
                    Some((id, _)) => id,
                    None if uri.is_empty() => NsId::None,
                    None => match dom.interner().get(&uri) {
                        Some(i) => NsId::Other(i),
                        None => continue,
                    },
                };
                let prefix = if a.name.local == LocalName::Xmlns {
                    None
                } else {
                    Some(a.name.local.as_str(dom.interner()).to_string())
                };
                if let Some(p) = &prefix {
                    preferred.entry(ns).or_insert_with(|| p.clone());
                }
                root_decls.push((prefix, ns));
            }
        }
        let ignorable = dom
            .attr_value(root, QName::new(NsId::Mc, LocalName::Ignorable))
            .map(|v| v.split_ascii_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        Self { root_decls, preferred, ignorable, flavor, understood: UNDERSTOOD.to_vec() }
    }

    /// 生成时该命名空间应使用的前缀：根声明优先，否则规范前缀（`PKG-09` 表）。
    pub fn prefix_for(&self, ns: NsId) -> Option<&str> {
        self.preferred.get(&ns).map(String::as_str).or_else(|| ns.canonical_prefix())
    }

    /// 根上是否已声明该命名空间。
    pub fn declares(&self, ns: NsId) -> bool {
        self.root_decls.iter().any(|(_, n)| *n == ns)
    }

    pub fn is_understood(&self, ns: NsId) -> bool {
        self.understood.contains(&ns)
    }
}
