//! L1 无损 XML（`spec/02-xml-dom.md`，`docs/03` §4、§9.2）。
//!
//! 把一个 XML part 的字节解析成带词法区间（`Lex`）的节点树，维护脏状态，并能把树序列化回字节，
//! 使干净节点逐字节一致。核心类型 `Node`/`Lex`/`Attr`/`QName`/`NsId`/[`Dirty`] 见 `docs/03` §4.1–4.2。
//!
//! 决策（2026-09-03，见 `docs/04-dev-plan.md` §2）：tokenizer 自写，不用 `quick-xml`——
//! 它不公开属性名/属性值的字节区间与引号风格，而 `XML-03/04/13` 全部依赖这些区间。
//!
//! M0 任务：0.6 名字表（`XML-05`）、0.7 tokenizer（`XML-01..08`）、0.8 作用域（`XML-11`）、
//! 0.9 MCE（`XML-09/10`）、0.10 脏状态（`XML-12`）、0.11 序列化（`XML-13/14`）。

pub mod dom;
pub mod edit;
pub mod entities;
pub mod interner;
pub mod lex;
pub mod mce;
pub mod names;
pub mod ns;
pub mod parse;
pub mod plan;
pub mod xpath;

pub use dom::{Attr, AttrValue, Dom, Element, Mce, MceRole, Node, NodeId, NodeKind, TextValue};
pub use interner::{Interned, Interner};
pub use lex::Lex;
pub use mce::{DEFAULT_UNDERSTOOD, SemanticChildren};
pub use names::{LocalName, NsId, QName};
pub use ns::{PrefixUse, Scope};
pub use parse::{RootInfo, XmlError, sniff_root};
pub use plan::{NewElement, NewNode, NodeEdit, Target};
pub use xpath::{XPathError, XValue, eval as xpath_eval, eval_strings as xpath_strings};

/// `XML-08`：迭代解析的深度上限。POI 5000 层嵌套表格必须成功。
pub const MAX_DEPTH: u32 = 100_000;

/// 节点脏状态（`XML-12`，`docs/03` §4.2）。
///
/// 不变式：非 `Clean` 节点的祖先不为 `Clean`；`Clean` 节点的后代全为 `Clean`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Dirty {
    /// 自身与后代都未变：整节点拷 `lex.range`。
    #[default]
    Clean,
    /// 自身未变、后代有变：拷 `lex.open`，逐子节点递归，拷 `lex.close`。
    DescendantDirty,
    /// 自身标签/属性有变：重建开标签（保留属性顺序、前缀、引号风格），逐子节点递归，重建闭标签。
    SelfDirty,
    /// 无 `lex`：整棵按 schema 生成。
    New,
    /// 不输出；保留在 arena 供 Anchor 变换与撤销。
    Deleted,
}

impl Dirty {
    /// 规则 C（`XML-12`）：某后代变为非 `Clean` 时，祖先若为 `Clean` 则变 `DescendantDirty`，
    /// 否则保持不变（传播在此停止）。返回 `true` 表示状态发生了改变、需要继续向上传播。
    pub fn absorb_descendant_change(&mut self) -> bool {
        if *self == Self::Clean {
            *self = Self::DescendantDirty;
            true
        } else {
            false
        }
    }

    /// 规则 A（`XML-12`）：自身变更。`New` 保持 `New`；`Deleted` 不应再被修改（调用方保证）。
    pub fn mark_self_changed(&mut self) {
        match self {
            Self::New | Self::Deleted => {}
            _ => *self = Self::SelfDirty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_12_rule_a_self_change() {
        let mut d = Dirty::Clean;
        d.mark_self_changed();
        assert_eq!(d, Dirty::SelfDirty);
        let mut n = Dirty::New;
        n.mark_self_changed();
        assert_eq!(n, Dirty::New);
        let mut dd = Dirty::DescendantDirty;
        dd.mark_self_changed();
        assert_eq!(dd, Dirty::SelfDirty);
    }

    #[test]
    fn xml_12_rule_c_propagation_stops_at_non_clean() {
        let mut clean = Dirty::Clean;
        assert!(clean.absorb_descendant_change());
        assert_eq!(clean, Dirty::DescendantDirty);
        for mut d in [Dirty::DescendantDirty, Dirty::SelfDirty, Dirty::New] {
            let before = d;
            assert!(!d.absorb_descendant_change());
            assert_eq!(d, before);
        }
    }
}
