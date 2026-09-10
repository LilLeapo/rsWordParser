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
use crate::xml::names::{LocalName, NsId, QName};

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
#[non_exhaustive]
/// 无损 DOM 节点；修改须经编辑事务维护脏状态和祖先关系（XML-12）。
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct Node {
    /// 元素、文本或不透明节点的内容。
    pub kind: NodeKind,
    /// 父节点，根节点为 `None`。
    pub parent: Option<NodeId>,
    /// 原文词法区间；`New` 节点为 `None`。
    pub lex: Option<Lex>,
    /// 当前编辑脏状态。
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
    /// 一个节点下全部 `m:oMath` 片段，文档序（`m:oMathPara` 展开；`m:oMath` 不嵌套）。
    /// 结果迭代器借用 DOM，不物化节点列表。
    #[inline]
    pub fn math_fragments(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.semantic_descendants(node)
            .filter(|&n| self.is(n, QName::new(NsId::M, LocalName::OMath)))
    }

    /// 片段里全部 `m:t` 的文本，文档序（TS `mathTokens`：可编辑的公式 token）。
    /// 每个 token 可能拼接多个解码文本节点；只在消费时创建独立文本，不收集中间列表。
    #[inline]
    pub fn math_tokens(&self, omath: NodeId) -> impl Iterator<Item = String> + '_ {
        self.semantic_descendants(omath)
            .filter(|&n| self.is(n, QName::new(NsId::M, LocalName::T)))
            .map(|t| self.omml_text_of(t))
    }

    /// 内容子节点：元素，且名字不以 `Pr` 结尾（TS `contentChildren`：属性包不是内容）。
    #[inline]
    pub fn content_children(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.semantic_children(node)
            .filter(|&c| self.name(c).is_some())
            .filter(|&c| !self.lex_name(c).is_some_and(|q| q.ends_with("Pr")))
    }

    /// `node/m:<pr>/m:<child>/@m:val`（TS `propVal`）。
    #[inline]
    pub fn prop_val(&self, node: NodeId, pr: LocalName, name: LocalName) -> Option<Cow<'_, str>> {
        let pr = self.children_named(node, QName::new(NsId::M, pr)).next()?;
        let c = self.children_named(pr, QName::new(NsId::M, name)).next()?;
        self.attr_value(c, QName::new(NsId::M, LocalName::Val))
    }

    /// 属性存在且不是 `0` / `false` / `off`（TS `propOn`）。
    #[inline]
    pub fn prop_on(&self, node: NodeId, pr: LocalName, name: LocalName) -> bool {
        self.prop_val(node, pr, name).is_some_and(|v| {
            v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off")
        })
    }

    /// 一个 `m:t` 的文本（实体已解码）。
    #[inline]
    pub fn omml_text_of(&self, node: NodeId) -> String {
        let mut s = String::new();
        for c in self.semantic_children(node) {
            if let Some(t) = self.text(c) {
                s.push_str(&t);
            }
        }
        s
    }

    /// 一个 `m:r` 的全部 `m:t` 文本拼接。
    #[inline]
    pub fn run_text(&self, run: NodeId) -> String {
        self.children_named(run, QName::new(NsId::M, LocalName::T))
            .map(|t| self.omml_text_of(t))
            .collect()
    }

    /// `m:rPr/m:sty = "p"` 或有 `m:rPr/m:nor`：普通文字（不按数学斜体分类）。
    #[inline]
    pub fn is_plain_run(&self, run: NodeId) -> bool {
        let sty = self.prop_val(run, LocalName::RPr, LocalName::Sty);
        sty.as_deref() == Some("p")
            || self.children_named(run, QName::new(NsId::M, LocalName::RPr)).next().is_some_and(
                |pr| self.children_named(pr, QName::new(NsId::M, LocalName::Nor)).next().is_some(),
            )
    }

    /// 容器下全部 `m:r` 的文字拼接（TS `plainTextOfRuns`：函数名 / `lim`）。
    #[inline]
    pub fn plain_text_of_runs(&self, node: Option<NodeId>) -> String {
        let Some(node) = node else { return String::new() };
        self.children_named(node, QName::new(NsId::M, LocalName::R))
            .map(|r| self.run_text(r))
            .collect()
    }

    /// 同名的语义子节点，保持文档顺序并跳过已删除或未选中的 MCE 分支。
    /// 迭代器借用此 DOM；仅遍历已有节点，不分配临时节点列表。
    #[inline]
    pub fn children_named(&self, node: NodeId, name: QName) -> impl Iterator<Item = NodeId> + '_ {
        self.semantic_children(node).filter(move |&child| self.is(child, name))
    }

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

#[cfg(test)]
mod test_model {
    #[test]
    fn math_iterators_preserve_order_empty_tokens_and_decoding() {
        let mut dom = super::Dom::parse(
            super::PartId(0),
            br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
                <w:oMath/><m:oMath><m:r><m:t>a&amp;<![CDATA[b]]>z</m:t><m:t/></m:r></m:oMath>
                <m:oMathPara><m:oMath><m:r><m:t>c</m:t></m:r></m:oMath></m:oMathPara>
            </w:p>"#,
        )
        .unwrap();
        let root = dom.root();
        let (first, second) = {
            let mut fragments = dom.math_fragments(root);
            let pair = (fragments.next().unwrap(), fragments.next().unwrap());
            assert!(fragments.next().is_none());
            pair
        };
        assert!(first < second);
        // CDATA is an opaque node in the existing OOXML parser; only text nodes contribute.
        assert!(dom.math_tokens(first).eq(["a&z", ""]));
        assert!(dom.math_tokens(second).eq(["c"]));
        assert_eq!(dom.math_tokens(root).collect::<String>(), "a&zc");
        dom.node_mut(first).dirty = super::Dirty::Deleted;
        assert!(dom.math_fragments(root).eq([second]));
        assert!(dom.math_tokens(root).eq(["c"]));
    }

    #[test]
    fn omml_properties_preserve_missing_empty_and_case_semantics() {
        for (value, enabled) in [
            ("0", false),
            ("false", false),
            ("FaLsE", false),
            ("OFF", false),
            ("", true),
            (" false ", true),
            ("1", true),
        ] {
            let xml = format!(
                r#"<m:d xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
                    <m:dPr><m:begChr m:val="&lt;"/><m:endChr m:val="{value}"/></m:dPr>
                    <m:e/><m:e/></m:d>"#
            );
            let dom = super::Dom::parse(super::PartId(0), xml.as_bytes()).unwrap();
            let root = dom.root();
            let pr = super::LocalName::DPr;
            assert_eq!(dom.content_children(root).count(), 2);
            assert_eq!(dom.prop_val(root, pr, super::LocalName::BegChr).as_deref(), Some("<"));
            assert!(matches!(
                dom.prop_val(root, pr, super::LocalName::EndChr),
                Some(super::Cow::Borrowed(_))
            ));
            assert_eq!(dom.prop_on(root, pr, super::LocalName::EndChr), enabled);
            assert_eq!(dom.prop_val(root, pr, super::LocalName::SepChr), None);
            assert!(!dom.prop_on(root, pr, super::LocalName::SepChr));
        }
    }

    #[test]
    fn named_children_preserve_namespace_order_and_mce_selection() {
        let mut dom = super::Dom::parse(
            super::PartId(0),
            br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"
                xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
                <w:r/><m:r/>
                <mc:AlternateContent><mc:Choice Requires="unknown"><w:r/></mc:Choice>
                    <mc:Fallback><w:r/></mc:Fallback></mc:AlternateContent><w:r/>
            </w:p>"#,
        )
        .unwrap();
        let root = dom.root();
        let name = super::QName::w(super::LocalName::R);
        let (first, second, third) = {
            let mut runs = dom.children_named(root, name);
            let ids = (runs.next().unwrap(), runs.next().unwrap(), runs.next().unwrap());
            assert!(runs.next().is_none());
            ids
        };
        assert!(first < second && second < third);
        dom.node_mut(second).dirty = super::Dirty::Deleted;
        assert!(dom.children_named(root, name).eq([first, third]));
        assert_eq!(dom.children_named(root, super::QName::w(super::LocalName::T)).count(), 0);
    }
}
