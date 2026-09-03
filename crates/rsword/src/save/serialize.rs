//! 无损序列化（`XML-13`、`XML-14`、`SAVE-03`）。迭代实现，深度不受栈限制。
//!
//! ```text
//! Clean           → 拷 lex.range
//! Deleted         → 空
//! DescendantDirty → 拷 lex.open；子节点逐个；拷 lex.close
//! SelfDirty / New → 重建开标签（属性原序、原引号、lex_name 优先）；子节点逐个；重建闭标签
//! ```
//!
//! 任务 0.7 阶段：`New`/改名节点缺少 `lex_name` 时返回 [`SerializeError::NeedsPrefixResolution`]；
//! 任务 0.11 接入 `Scope` + `NamespaceContext` 后由作用域决定前缀（`PKG-09`）。

use std::fmt;

use crate::xml::Dirty;
use crate::xml::dom::{AttrValue, Dom, Element, NodeId, NodeKind, TextValue};
use crate::xml::entities;
use crate::xml::lex::urange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeError {
    /// 元素或属性没有原始写法，需要按作用域生成前缀（任务 0.11）。
    NeedsPrefixResolution { node: NodeId },
    /// 非 `New` 节点缺少 `lex`：引擎不变式被破坏。
    MissingLex { node: NodeId },
}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedsPrefixResolution { node } => {
                write!(f, "node {} needs prefix resolution", node.0)
            }
            Self::MissingLex { node } => write!(f, "node {} is not New but has no lex", node.0),
        }
    }
}

impl std::error::Error for SerializeError {}

/// 整个 part：序言 + 根 + 尾声。
pub fn serialize(dom: &Dom) -> Result<Vec<u8>, SerializeError> {
    let src = dom.src_bytes();
    let mut out = Vec::with_capacity(src.len() + 64);
    out.extend_from_slice(&src[urange(&dom.prolog())]);
    serialize_subtree(dom, dom.root(), &mut out)?;
    out.extend_from_slice(&src[urange(&dom.epilog())]);
    Ok(out)
}

enum Step {
    Enter(NodeId),
    Exit(NodeId),
}

/// 序列化一棵子树到 `out`。
pub fn serialize_subtree(dom: &Dom, root: NodeId, out: &mut Vec<u8>) -> Result<(), SerializeError> {
    let src = dom.src_bytes();
    let mut stack = vec![Step::Enter(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(id) => {
                let node = dom.node(id);
                match node.dirty {
                    Dirty::Deleted => {}
                    Dirty::Clean => {
                        let lex =
                            node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                        out.extend_from_slice(&src[urange(&lex.range)]);
                    }
                    Dirty::DescendantDirty => match &node.kind {
                        NodeKind::Element(e) => {
                            let lex =
                                node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                            out.extend_from_slice(&src[urange(&lex.open)]);
                            if !lex.is_self_closing() {
                                stack.push(Step::Exit(id));
                            }
                            stack.extend(e.children.iter().rev().map(|&c| Step::Enter(c)));
                        }
                        _ => {
                            let lex =
                                node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                            out.extend_from_slice(&src[urange(&lex.range)]);
                        }
                    },
                    Dirty::SelfDirty | Dirty::New => match &node.kind {
                        NodeKind::Element(e) => {
                            let live =
                                e.children.iter().any(|&c| dom.node(c).dirty != Dirty::Deleted);
                            write_open_tag(dom, id, e, live, out)?;
                            if live {
                                stack.push(Step::Exit(id));
                                stack.extend(e.children.iter().rev().map(|&c| Step::Enter(c)));
                            }
                        }
                        NodeKind::Text(TextValue::Raw(r)) => out.extend_from_slice(&src[urange(r)]),
                        NodeKind::Text(TextValue::Owned(s)) => entities::escape_text(s, out),
                        NodeKind::Opaque => {
                            let lex =
                                node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                            out.extend_from_slice(&src[urange(&lex.range)]);
                        }
                    },
                }
            }
            Step::Exit(id) => {
                let node = dom.node(id);
                match node.dirty {
                    Dirty::DescendantDirty => {
                        let lex =
                            node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                        out.extend_from_slice(&src[urange(&lex.close)]);
                    }
                    _ => {
                        if let NodeKind::Element(e) = &node.kind {
                            out.extend_from_slice(b"</");
                            out.extend_from_slice(element_name(dom, id, e)?);
                            out.push(b'>');
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn element_name<'a>(dom: &'a Dom, id: NodeId, e: &Element) -> Result<&'a [u8], SerializeError> {
    match &e.lex_name {
        Some(r) => Ok(dom.lex_bytes(r)),
        None => Err(SerializeError::NeedsPrefixResolution { node: id }),
    }
}

fn write_open_tag(
    dom: &Dom,
    id: NodeId,
    e: &Element,
    live: bool,
    out: &mut Vec<u8>,
) -> Result<(), SerializeError> {
    out.push(b'<');
    out.extend_from_slice(element_name(dom, id, e)?);
    for a in &e.attrs {
        out.push(b' ');
        match &a.lex_name {
            Some(r) => out.extend_from_slice(dom.lex_bytes(r)),
            None => return Err(SerializeError::NeedsPrefixResolution { node: id }),
        }
        out.push(b'=');
        out.push(a.quote);
        match &a.value {
            AttrValue::Raw(r) => out.extend_from_slice(dom.lex_bytes(r)),
            AttrValue::Owned(s) => entities::escape_attr(s, a.quote, out),
        }
        out.push(a.quote);
    }
    if live {
        out.push(b'>');
    } else {
        out.extend_from_slice(b"/>");
    }
    Ok(())
}
