//! 节点、arena 与只读访问 API（`XML-02`、`XML-04`，`docs/03` §4.1）。
//!
//! 与 `docs/03` §4.1 的差别：原始限定名放在 [`Element::lex_name`]（与 [`Attr::lex_name`] 对称），
//! 而不是 `Lex.name`；两者语义相同，`None` 表示名字是新的或被改过、需要按作用域重新生成前缀。

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use crate::diag::Diagnostic;
use crate::package::{PartFlavor, PartId};
use crate::xml::Dirty;
use crate::xml::entities;
use crate::xml::interner::Interner;
use crate::xml::lex::{Lex, urange};
use crate::xml::names::{NsId, QName};

/// arena 索引，会话内稳定且永不复用；`Deleted` 节点保留在 arena 中。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    ::serde::Serialize,
    ::serde::Deserialize,
)]
#[serde(transparent)]
pub struct NodeId(pub u32);

impl NodeId {
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrValue {
    /// 引号内的原字节，读取时按 `XML-06` 解码。
    Raw(Range<u32>),
    /// 已解码的值。
    Owned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextValue {
    Raw(Range<u32>),
    Owned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// 语义身份。
    pub name: QName,
    /// 原始写法（含前缀）；`New` 属性或改名后为 `None`。
    pub lex_name: Option<Range<u32>>,
    pub value: AttrValue,
    /// `b'"'` 或 `b'\''`。
    pub quote: u8,
}

/// MCE 角色（`XML-09`，任务 0.9 填充）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MceRole {
    #[default]
    None,
    AlternateContent,
    Choice,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mce {
    pub role: MceRole,
    /// `AlternateContent` 下被选中的分支；非 MCE 元素恒为 `true`。
    pub active: bool,
    /// 元素自身可忽略、内容仍需处理（`mc:ProcessContent` 命中）。
    pub process_content: bool,
    /// `mc:MustUnderstand` 命中未理解命名空间。
    pub must_understand: bool,
    /// 元素属于祖先 `mc:Ignorable` 列出且未理解的命名空间：语义遍历跳过，DOM 保留。
    /// （`docs/03` §4.4 的结构里没有这个字段；它是遍历所需的缓存，避免每次重算作用域。）
    pub ignorable: bool,
}

impl Default for Mce {
    fn default() -> Self {
        Self {
            role: MceRole::None,
            active: true,
            process_content: false,
            must_understand: false,
            ignorable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: QName,
    /// 开标签中的原始限定名区间（`w:p`）；改名或 `New` 时为 `None`。
    pub lex_name: Option<Range<u32>>,
    /// 保持原顺序；重复属性全部保留（`XML-04`）。
    pub attrs: Vec<Attr>,
    /// 元素、文本、Opaque 混排，保持顺序；含 `Deleted` 节点（语义遍历用 `semantic_children`）。
    pub children: Vec<NodeId>,
    pub mce: Mce,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Element(Element),
    Text(TextValue),
    /// 注释 / PI / CDATA：永远按 `lex` 原字节输出。
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    /// 原文词法区间；`New` 节点为 `None`。
    pub lex: Option<Lex>,
    pub dirty: Dirty,
}

/// 一个 XML part 的无损 DOM。
#[derive(Debug, Clone)]
pub struct Dom {
    pub(crate) part: PartId,
    /// 原始 part 字节（或转码后的 UTF-8 字节）。
    pub(crate) src: Arc<str>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) root: NodeId,
    pub(crate) prolog: Range<u32>,
    pub(crate) epilog: Range<u32>,
    pub(crate) transcoded: bool,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) interner: Interner,
    /// 生成新节点时用的命名空间族（`PKG-08`）。解析时按根元素命名空间推断，包层可覆盖。
    pub(crate) flavor: PartFlavor,
}

impl Dom {
    pub fn part(&self) -> PartId {
        self.part
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// 词法区间所指向的字节体系。转码过的 part 这里是转码后的字节（`XML-01`）。
    pub fn src(&self) -> &str {
        &self.src
    }

    pub fn src_bytes(&self) -> &[u8] {
        self.src.as_bytes()
    }

    pub fn transcoded(&self) -> bool {
        self.transcoded
    }

    /// 生成新节点用的命名空间族。
    pub fn flavor(&self) -> PartFlavor {
        self.flavor
    }

    pub fn set_flavor(&mut self, flavor: PartFlavor) {
        self.flavor = flavor;
    }

    pub fn prolog(&self) -> Range<u32> {
        self.prolog.clone()
    }

    pub fn epilog(&self) -> Range<u32> {
        self.epilog.clone()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.idx()]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.idx()]
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.node(id).kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.node_mut(id).kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn name(&self, id: NodeId) -> Option<QName> {
        self.element(id).map(|e| e.name)
    }

    pub fn is(&self, id: NodeId, name: QName) -> bool {
        self.name(id) == Some(name)
    }

    /// 原始子节点列表（含 `Deleted` 与非 active MCE 分支）。模型层禁止直接用它（`XML-10`）。
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.element(id).map_or(&[], |e| e.children.as_slice())
    }

    pub fn lex_bytes(&self, range: &Range<u32>) -> &[u8] {
        &self.src.as_bytes()[urange(range)]
    }

    pub fn lex_str(&self, range: &Range<u32>) -> &str {
        &self.src[urange(range)]
    }

    /// 第一个同名属性（`XML-04`：重复属性语义取第一个）。
    pub fn attr(&self, id: NodeId, name: QName) -> Option<&Attr> {
        self.element(id)?.attrs.iter().find(|a| a.name == name)
    }

    /// 解码后的属性值。
    pub fn attr_str<'s>(&'s self, attr: &'s Attr) -> Cow<'s, str> {
        match &attr.value {
            AttrValue::Raw(r) => entities::decode(self.lex_str(r)),
            AttrValue::Owned(s) => Cow::Borrowed(s.as_str()),
        }
    }

    pub fn attr_value(&self, id: NodeId, name: QName) -> Option<Cow<'_, str>> {
        self.attr(id, name).map(|a| self.attr_str(a))
    }

    /// 文本节点解码后的内容；非文本节点返回 `None`。不 trim（`XML-07`）。
    pub fn text(&self, id: NodeId) -> Option<Cow<'_, str>> {
        match &self.node(id).kind {
            NodeKind::Text(TextValue::Raw(r)) => Some(entities::decode(self.lex_str(r))),
            NodeKind::Text(TextValue::Owned(s)) => Some(Cow::Borrowed(s.as_str())),
            _ => None,
        }
    }

    /// 元素的原始限定名写法（`w:p`）；`New` 或改名后为 `None`。
    pub fn lex_name(&self, id: NodeId) -> Option<&str> {
        self.element(id)?.lex_name.as_ref().map(|r| self.lex_str(r))
    }

    /// 从 `id` 到根的祖先链（不含自身）。
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.parent(id), move |&p| self.parent(p))
    }

    /// 元素是否属于 `want` 命名空间；前缀未绑定时按前缀字面量兜底。
    ///
    /// `XML-05` 下未绑定的前缀会记 `XML_UNBOUND_PREFIX` 并原样保留，语义上不属于任何命名空间。
    /// 但语料里有既写 `<v:shape>` / `<o:OLEObject>` 又不声明 `xmlns:v` / `xmlns:o` 的文档
    /// （`resource-cleanup__008`），这时前缀字面量是判断「这是什么」的唯一线索，宁可按它认，
    /// 也好过整段内容认不出来。只用在语义识别上，不影响写回。
    pub fn is_ns(&self, id: NodeId, want: NsId, prefix: &str) -> bool {
        let Some(name) = self.name(id) else { return false };
        if name.ns == want {
            return true;
        }
        matches!(name.ns, NsId::Unbound(_))
            && self.lex_name(id).and_then(|q| q.split_once(':')).is_some_and(|(p, _)| p == prefix)
    }

    /// 深度优先前序遍历（含 `Deleted`），迭代实现。
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants { dom: self, stack: vec![id] }
    }
}

pub struct Descendants<'a> {
    dom: &'a Dom,
    stack: Vec<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.stack.pop()?;
        let children = self.dom.children(id);
        self.stack.extend(children.iter().rev());
        Some(id)
    }
}
