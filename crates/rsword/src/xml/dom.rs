//! 节点、arena 与只读访问 API（`XML-02`、`XML-04`，`docs/03` §4.1）。
//!
//! 与 `docs/03` §4.1 的差别：原始限定名放在 [`Element::lex_name`]（与 [`Attr::lex_name`] 对称），
//! 而不是 `Lex.name`；两者语义相同，`None` 表示名字是新的或被改过、需要按作用域重新生成前缀。

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::package::{PartFlavor, PartId};
use crate::xml::Dirty;
use crate::xml::entities;
use crate::xml::entities::FragmentText;
use crate::xml::interner::Interner;
use crate::xml::lex::{Lex, urange};
use crate::xml::names::{LocalName, NsId, QName};

/// 三张 TS 表的反查：符号 / n 元运算符 / 重音 → 命令名（同一字符有多个名字时**第一个**赢）。
macro_rules! latex_symbols {
    ($fn:ident / $rev:ident: $($name:literal => $ch:literal),+ $(,)?) => {
        impl LatexParser<'_> {
            /// 字符 → `\命令`（表序，别名取第一个）。
            #[inline]
            fn $fn(ch: char) -> Option<&'static str> {
                $( if ch == $ch { return Some($name); } )+
                None
            }

            /// `\命令` → 字符（同一张表的反方向，LaTeX 解析器用）。
            #[inline]
            fn $rev(name: &str) -> Option<char> {
                $( if name == $name { return Some($ch); } )+
                None
            }
        }
    };
}

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

#[derive(Clone)]
enum LatexItem {
    Node(NodeId),
    /// `parent/m:<name>` 的内容（缺失 → `""`）。
    Slot(NodeId, LocalName),
    /// 内容子节点直接拼接。
    Seq(NodeId),
    /// `\binom{num}{den}`（`(` `)` 包着的单个 noBar 分式）。
    Binom(NodeId),
    /// `m:m` / `m:eqArr` 的行体；`env` 是环境名（`matrix` / `pmatrix` / … / `cases`）。
    Matrix {
        node: NodeId,
        env: String,
    },
    /// 一行 `m:mr`：各格 ` & ` 连接。
    MatrixRow(NodeId),
    /// `\left<beg> … \right<end>`。
    LeftRight {
        beg: String,
        end: String,
        slot: NodeId,
    },
}

enum LatexTask {
    Eval(LatexItem),
    Finish(LatexItem, usize),
}

latex_symbols! { symbol_command / symbol_char:
    "alpha" => 'α', "beta" => 'β', "gamma" => 'γ', "delta" => 'δ', "epsilon" => 'ε', "zeta" => 'ζ',
    "eta" => 'η', "theta" => 'θ', "vartheta" => 'ϑ', "iota" => 'ι', "kappa" => 'κ', "lambda" => 'λ',
    "mu" => 'μ', "nu" => 'ν', "xi" => 'ξ', "pi" => 'π', "rho" => 'ρ', "sigma" => 'σ', "tau" => 'τ',
    "upsilon" => 'υ', "phi" => 'φ', "varphi" => 'ϕ', "chi" => 'χ', "psi" => 'ψ', "omega" => 'ω',
    "Gamma" => 'Γ', "Delta" => 'Δ', "Theta" => 'Θ', "Lambda" => 'Λ', "Xi" => 'Ξ', "Pi" => 'Π',
    "Sigma" => 'Σ', "Upsilon" => 'Υ', "Phi" => 'Φ', "Psi" => 'Ψ', "Omega" => 'Ω',
    "infty" => '∞', "pm" => '±', "mp" => '∓', "times" => '×', "div" => '÷', "cdot" => '⋅', "ast" => '*',
    "le" => '≤', "ge" => '≥', "ne" => '≠', "approx" => '≈', "equiv" => '≡', "sim" => '∼', "propto" => '∝',
    "to" => '→', "leftarrow" => '←', "leftrightarrow" => '↔', "Rightarrow" => '⇒', "Leftarrow" => '⇐',
    "Leftrightarrow" => '⇔', "partial" => '∂', "nabla" => '∇', "in" => '∈', "notin" => '∉',
    "subset" => '⊂', "supset" => '⊃', "subseteq" => '⊆', "supseteq" => '⊇', "cup" => '∪', "cap" => '∩',
    "forall" => '∀', "exists" => '∃', "wedge" => '∧', "vee" => '∨', "neg" => '¬', "angle" => '∠',
    "perp" => '⊥', "parallel" => '∥', "ldots" => '…', "cdots" => '⋯', "vdots" => '⋮', "ddots" => '⋱',
    "prime" => '′', "circ" => '∘', "degree" => '°', "bullet" => '∙', "star" => '⋆', "emptyset" => '∅',
    "hbar" => 'ℏ', "ell" => 'ℓ', "Re" => 'ℜ', "Im" => 'ℑ', "aleph" => 'ℵ', "therefore" => '∴', "because" => '∵',
}

latex_symbols! { accent_char_command / accent_char:
    "hat" => '\u{0302}', "bar" => '\u{0304}', "vec" => '\u{20D7}', "dot" => '\u{0307}', "ddot" => '\u{0308}',
    "tilde" => '\u{0303}', "check" => '\u{030C}', "breve" => '\u{0306}',
}

latex_symbols! { nary_char_command / nary_char:
    "sum" => '∑', "prod" => '∏', "coprod" => '∐', "bigcup" => '⋃', "bigcap" => '⋂', "int" => '∫',
    "iint" => '∬', "iiint" => '∭', "oint" => '∮',
}

/// TS `LATEX_FUNCTIONS`。
const LATEX_FUNCTIONS: [&str; 27] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos",
    "arctan", "ln", "log", "exp", "max", "min", "sup", "inf", "arg", "det", "gcd", "deg", "dim",
    "ker", "mod",
];

// LaTeX → OMML（TS `math.ts` 的 `latexToOmml`，`spec/18` 7.5 逐字移植）。
//
// 输出与 TS **逐字相等**（`fixtures/fieldgen/` 是对照件）：同样的元素顺序、同样的属性顺序、
// 同样的转义。这是**用户输入**的解析器，不是文档遍历，所以按 `spec/18` 的约定用递归下降 +
// 深度上限（256），超限 `Err(EDIT_MATH_TOO_DEEP)` 而不是写显式栈。

/// 递归深度上限（用户输入，不是文档；`spec/18` 风险 11）。
const LATEX_MAX_DEPTH: usize = 256;

struct LatexParser<'a> {
    src: &'a [char],
    pos: usize,
    depth: usize,
}

impl LatexParser<'_> {
    #[inline]
    fn peek(&self) -> char {
        self.src.get(self.pos).copied().unwrap_or('\0')
    }

    #[inline]
    fn rest_starts_with(&self, pat: &str) -> bool {
        let Some(rest) = self.src.get(self.pos..) else {
            return false;
        };
        let mut chars = rest.iter();
        pat.chars().all(|ch| chars.next() == Some(&ch))
    }

    #[inline]
    fn skip_spaces(&mut self) {
        while self.peek().is_whitespace() {
            self.pos += 1;
        }
    }

    #[inline]
    fn slice(&self, from: usize, to: usize) -> String {
        self.src[from.min(self.src.len())..to.min(self.src.len())].iter().collect()
    }

    #[inline]
    fn deeper(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > LATEX_MAX_DEPTH { Err(LatexParser::too_deep()) } else { Ok(()) }
    }

    /// TS `readControlName`：反斜杠之后的命令名（字母串，否则单个字符）。
    #[inline]
    fn read_control_name(&mut self) -> String {
        let start = self.pos;
        while self.src.get(self.pos).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        if self.pos > start {
            return self.slice(start, self.pos);
        }
        let ch = self.peek();
        self.pos += 1;
        if ch == '\0' { String::new() } else { ch.to_string() }
    }

    /// TS `parseGroup`：必需的 `{...}`，或者按 LaTeX 语义的**一个** token。
    #[inline]
    fn parse_group(&mut self) -> Result<String> {
        self.skip_spaces();
        if self.peek() == '{' {
            self.pos += 1;
            self.deeper()?;
            let out = self.parse_sequence(&|p: &LatexParser<'_>| p.peek() == '}')?;
            self.depth -= 1;
            if self.peek() != '}' {
                return Err(LatexParser::err("Missing matching }"));
            }
            self.pos += 1;
            return Ok(out);
        }
        if self.peek() == '\\' {
            self.pos += 1;
            self.deeper()?;
            let out = self.parse_control()?;
            self.depth -= 1;
            return Ok(out);
        }
        let ch = self.peek();
        if ch == '\0' || "{}^_&".contains(ch) {
            return Err(LatexParser::err("An argument is required here"));
        }
        self.pos += 1;
        Ok(Omml::math_run(&ch.to_string(), false))
    }

    /// TS `readBraceText`：`{...}` 的原文（`\text` / `\begin` 的名字）。
    #[inline]
    fn read_brace_text(&mut self) -> Result<String> {
        self.skip_spaces();
        if self.peek() != '{' {
            return Err(LatexParser::err("Expected { here"));
        }
        self.pos += 1;
        let mut depth = 1usize;
        let mut out = String::new();
        while self.pos < self.src.len() {
            let ch = self.src[self.pos];
            self.pos += 1;
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    return Ok(out);
                }
            }
            if depth > 0 {
                out.push(ch);
            }
        }
        Err(LatexParser::err("Missing matching }"))
    }

    /// TS `parseSequence`：一串原子，`^` / `_` 作用在前一个原子上。
    #[inline]
    fn parse_sequence(&mut self, stop: &Stop<'_>) -> Result<String> {
        let mut atoms: Vec<String> = Vec::new();
        loop {
            self.skip_spaces();
            if self.pos >= self.src.len() || stop(self) {
                break;
            }
            let ch = self.peek();
            if ch == '^' || ch == '_' {
                self.pos += 1;
                let script = self.parse_group()?;
                let other = self.peek();
                let base = atoms.pop().unwrap_or_else(|| Omml::math_run("", false));
                if (other == '^' || other == '_') && other != ch {
                    self.pos += 1;
                    let second = self.parse_group()?;
                    let (sub, sup) = if ch == '_' { (script, second) } else { (second, script) };
                    atoms.push(format!(
                            "<m:sSubSup><m:e>{base}</m:e><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup></m:sSubSup>"
                        ));
                } else if ch == '^' {
                    atoms
                        .push(format!("<m:sSup><m:e>{base}</m:e><m:sup>{script}</m:sup></m:sSup>"));
                } else {
                    atoms
                        .push(format!("<m:sSub><m:e>{base}</m:e><m:sub>{script}</m:sub></m:sSub>"));
                }
                continue;
            }
            atoms.push(self.parse_atom()?);
        }
        Ok(atoms.concat())
    }

    /// TS `parseAtom`。
    #[inline]
    fn parse_atom(&mut self) -> Result<String> {
        self.skip_spaces();
        let ch = self.peek();
        if ch == '\0' {
            return Ok(String::new());
        }
        if ch == '{' {
            return self.parse_group();
        }
        if ch == '}' {
            return Err(LatexParser::err("Unexpected }"));
        }
        if ch == '\\' {
            self.pos += 1;
            self.deeper()?;
            let out = self.parse_control()?;
            self.depth -= 1;
            return Ok(out);
        }
        let start = self.pos;
        while self.pos < self.src.len() {
            let c = self.src[self.pos];
            if "\\{}^_&".contains(c) || c == '\n' {
                break;
            }
            self.pos += 1;
        }
        let text = self.slice(start, self.pos);
        if text.is_empty() {
            return Err(LatexParser::err(format!("Cannot parse: \"{ch}\"")));
        }
        // 紧跟的上下标只作用在**最后一个字符**上（"ab^2" = a·b²）：退回去让它自成一个原子
        if (self.peek() == '^' || self.peek() == '_')
            && let Some((last, _)) = text.char_indices().next_back().filter(|&(index, _)| index > 0)
        {
            self.pos -= 1;
            return Ok(Omml::math_run(&text[..last], false));
        }
        Ok(Omml::math_run(&text, false))
    }

    /// TS `naryOmml`。
    #[inline]
    fn nary_omml(&mut self, chr: &str, lim_loc: &str) -> Result<String> {
        let (mut sub, mut sup) = (String::new(), String::new());
        for _ in 0..2 {
            self.skip_spaces();
            let ch = self.peek();
            if ch == '_' && sub.is_empty() {
                self.pos += 1;
                sub = self.parse_group()?;
            } else if ch == '^' && sup.is_empty() {
                self.pos += 1;
                sup = self.parse_group()?;
            } else {
                break;
            }
        }
        self.skip_spaces();
        let operand = if self.peek() == '{' { self.parse_group()? } else { String::new() };
        let pr = format!(
            r#"<m:naryPr><m:chr m:val="{}"/><m:limLoc m:val="{lim_loc}"/>{}{}</m:naryPr>"#,
            Omml::escape_attr(chr),
            if sub.is_empty() { r#"<m:subHide m:val="1"/>"# } else { "" },
            if sup.is_empty() { r#"<m:supHide m:val="1"/>"# } else { "" },
        );
        Ok(format!(
            "<m:nary>{pr}{}{}<m:e>{operand}</m:e></m:nary>",
            if sub.is_empty() { String::new() } else { format!("<m:sub>{sub}</m:sub>") },
            if sup.is_empty() { String::new() } else { format!("<m:sup>{sup}</m:sup>") },
        ))
    }

    /// TS `matrixOmml`。
    #[inline]
    fn matrix_omml(&mut self, env: &str) -> Result<String> {
        let delims = LatexParser::matrix_delims(env).expect("caller checked the environment");
        let mut rows: Vec<Vec<String>> = vec![Vec::new()];
        loop {
            let cell = self.parse_sequence(&|p: &LatexParser<'_>| {
                p.peek() == '&' || p.rest_starts_with("\\\\") || p.rest_starts_with("\\end")
            })?;
            rows.last_mut().expect("never empty").push(cell);
            if self.peek() == '&' {
                self.pos += 1;
            } else if self.rest_starts_with("\\\\") {
                self.pos += 2;
                rows.push(Vec::new());
            } else if self.rest_starts_with("\\end") {
                self.pos += 4;
                let closing = self.read_brace_text()?;
                if closing != env {
                    return Err(LatexParser::err(format!(
                        "\\end{{{closing}}} does not match \\begin{{{env}}}"
                    )));
                }
                break;
            } else {
                return Err(LatexParser::err(format!(
                    "\\begin{{{env}}} is missing \\end{{{env}}}"
                )));
            }
        }
        let body: String = rows
            .iter()
            .filter(|row| row.len() > 1 || row.first().is_some_and(|c| !c.is_empty()))
            .map(|row| {
                let cells: String = row.iter().map(|c| format!("<m:e>{c}</m:e>")).collect();
                format!("<m:mr>{cells}</m:mr>")
            })
            .collect();
        let matrix = format!("<m:m>{body}</m:m>");
        let Some((beg, end)) = delims else {
            return Ok(matrix);
        };
        Ok(format!(
            concat!(
                r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
                "</m:dPr><m:e>{matrix}</m:e></m:d>"
            ),
            beg = Omml::escape_attr(beg),
            end = Omml::escape_attr(end),
            matrix = matrix,
        ))
    }

    /// TS `readDelimiter`。
    #[inline]
    fn read_delimiter(&mut self) -> Result<String> {
        self.skip_spaces();
        if self.peek() == '\\' {
            let start = self.pos;
            self.pos += 1;
            let name = self.read_control_name();
            if let Some(ch) = LatexParser::left_right_char(&format!("\\{name}")) {
                return Ok(ch.to_string());
            }
            self.pos = start;
            return Err(LatexParser::err(format!("Unsupported delimiter: \\{name}")));
        }
        let ch = self.peek();
        if let Some(mapped) = LatexParser::left_right_char(&ch.to_string()) {
            self.pos += 1;
            return Ok(mapped.to_string());
        }
        Err(LatexParser::err(format!("Unsupported delimiter: \"{ch}\"")))
    }

    /// TS `parseControl`。
    #[inline]
    fn parse_control(&mut self) -> Result<String> {
        let name = self.read_control_name();
        if let Some(ch) = LatexParser::symbol_char(&name) {
            return Ok(Omml::math_run(&ch.to_string(), false));
        }
        if let Some((chr, lim_loc)) = LatexParser::nary_op(&name) {
            return self.nary_omml(&chr.to_string(), lim_loc);
        }
        if let Some(ch) = LatexParser::accent_char(&name) {
            let base = self.parse_group()?;
            return Ok(format!(
                r#"<m:acc><m:accPr><m:chr m:val="{}"/></m:accPr><m:e>{base}</m:e></m:acc>"#,
                Omml::escape_attr(&ch.to_string())
            ));
        }
        if LatexParser::is_latex_function(&name) {
            return Ok(Omml::math_run(&name, true));
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.parse_group()?;
                let den = self.parse_group()?;
                Ok(format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>"))
            }
            "binom" => {
                let top = self.parse_group()?;
                let bottom = self.parse_group()?;
                Ok(format!(
                    concat!(
                        r#"<m:d><m:e><m:f><m:fPr><m:type m:val="noBar"/></m:fPr>"#,
                        "<m:num>{top}</m:num><m:den>{bottom}</m:den></m:f></m:e></m:d>"
                    ),
                    top = top,
                    bottom = bottom
                ))
            }
            "sqrt" => {
                self.skip_spaces();
                let mut deg = String::new();
                if self.peek() == '[' {
                    // 普通字符串不会在 ']' 停下：把次数的源码单独切出来当一段解析
                    self.pos += 1;
                    let close = (self.pos..self.src.len()).find(|&i| self.src[i] == ']');
                    let Some(close) = close else {
                        return Err(LatexParser::err("Missing matching ]"));
                    };
                    let mut sub =
                        LatexParser { src: &self.src[self.pos..close], pos: 0, depth: self.depth };
                    deg = sub.parse_sequence(&|q: &LatexParser<'_>| q.pos >= q.src.len())?;
                    self.pos = close + 1;
                }
                let inner = self.parse_group()?;
                if deg.is_empty() {
                    return Ok(format!(
                        r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>{inner}</m:e></m:rad>"#
                    ));
                }
                Ok(format!("<m:rad><m:deg>{deg}</m:deg><m:e>{inner}</m:e></m:rad>"))
            }
            "overline" => Ok(format!(
                r#"<m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e>{}</m:e></m:bar>"#,
                self.parse_group()?
            )),
            "underline" => Ok(format!(
                r#"<m:bar><m:barPr><m:pos m:val="bot"/></m:barPr><m:e>{}</m:e></m:bar>"#,
                self.parse_group()?
            )),
            "underbrace" => Ok(format!(
                concat!(
                    r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏟"/><m:pos m:val="bot"/></m:groupChrPr>"#,
                    "<m:e>{}</m:e></m:groupChr>"
                ),
                self.parse_group()?
            )),
            "overbrace" => Ok(format!(
                concat!(
                    r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏞"/><m:pos m:val="top"/></m:groupChrPr>"#,
                    "<m:e>{}</m:e></m:groupChr>"
                ),
                self.parse_group()?
            )),
            "text" | "mathrm" | "operatorname" => {
                let t = self.read_brace_text()?;
                Ok(Omml::math_run(&t, true))
            }
            "lim" => {
                self.skip_spaces();
                if self.peek() == '_' {
                    self.pos += 1;
                    let lim = self.parse_group()?;
                    return Ok(format!(
                        "<m:limLow><m:e>{}</m:e><m:lim>{lim}</m:lim></m:limLow>",
                        Omml::math_run("lim", true)
                    ));
                }
                Ok(Omml::math_run("lim", true))
            }
            "left" => {
                let beg = self.read_delimiter()?;
                self.deeper()?;
                let body =
                    self.parse_sequence(&|p: &LatexParser<'_>| p.rest_starts_with("\\right"))?;
                self.depth -= 1;
                if !self.rest_starts_with("\\right") {
                    return Err(LatexParser::err("\\left is missing a matching \\right"));
                }
                self.pos += "\\right".chars().count();
                let end = self.read_delimiter()?;
                Ok(format!(
                    concat!(
                        r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
                        "</m:dPr><m:e>{body}</m:e></m:d>"
                    ),
                    beg = Omml::escape_attr(&beg),
                    end = Omml::escape_attr(&end),
                    body = body,
                ))
            }
            "begin" => {
                let env = self.read_brace_text()?;
                if LatexParser::matrix_delims(&env).is_none() {
                    return Err(LatexParser::err(format!(
                        "Unsupported environment: \\begin{{{env}}}"
                    )));
                }
                self.deeper()?;
                let out = self.matrix_omml(&env)?;
                self.depth -= 1;
                Ok(out)
            }
            "," | ";" | " " | "quad" | "qquad" => Ok(Omml::math_run(" ", false)),
            "\\" => Err(LatexParser::err("\\\\ is only allowed inside matrix environments")),
            "{" => Ok(Omml::math_run("{", false)),
            "}" => Ok(Omml::math_run("}", false)),
            "%" | "&" | "$" | "#" | "_" | "^" => Ok(Omml::math_run(&name, false)),
            other => Err(LatexParser::err(format!("Unsupported command: \\{other}"))),
        }
    }
}

type Stop<'s> = dyn Fn(&LatexParser<'_>) -> bool + 's;
/// Borrowed LaTeX source interpreted by the supported formula grammar.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Latex<'a>(&'a str);
impl<'a> From<&'a str> for Latex<'a> {
    #[inline]
    fn from(value: &'a str) -> Self {
        Self(value)
    }
}
/// Owned OMML fragment content, without its outer `m:oMath` element.
#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omml(String);
impl From<String> for Omml {
    #[inline]
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<Omml> for String {
    #[inline]
    fn from(value: Omml) -> Self {
        value.0
    }
}
impl AsRef<str> for Omml {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<Latex<'_>> for Omml {
    type Error = Error;
    #[inline]
    /// TS `latexToOmml`：整串 LaTeX → `m:oMath` 的**内容**（不含 `m:oMath` 本身）。
    fn try_from(value: Latex<'_>) -> Result<Self> {
        let chars: Vec<char> = value.0.chars().collect();
        let mut p = LatexParser { src: &chars, pos: 0, depth: 0 };
        let out = p.parse_sequence(&|p: &LatexParser<'_>| p.pos >= p.src.len())?;
        if p.pos < p.src.len() {
            return Err(LatexParser::err(format!(
                "Cannot parse: \"{}\"",
                p.slice(p.pos, p.pos + 12)
            )));
        }
        Ok(Self(out))
    }
}
impl LatexItem {
    /// 普通数学文字：解析器的特殊字符转义，符号换成 `\命令 `（TS `charsToLatex`）。
    #[inline]
    fn chars_to_latex(text: &str) -> Result<String> {
        let mut out = String::new();
        for ch in text.chars() {
            if ch == '\\' || ch == '\n' {
                return Err(Error::LatexUnsupported);
            }
            if let Some(esc) = LatexItem::char_escape(ch) {
                out.push_str(esc);
                continue;
            }
            match LatexParser::symbol_command(ch) {
                Some(cmd) => {
                    out.push('\\');
                    out.push_str(cmd);
                    out.push(' ');
                }
                None => out.push(ch),
            }
        }
        Ok(out)
    }
    #[inline]
    fn char_escape(ch: char) -> Option<&'static str> {
        Some(match ch {
            '{' => "\\{ ",
            '}' => "\\} ",
            '_' => "\\_ ",
            '^' => "\\^ ",
            '&' => "\\& ",
            '%' => "\\% ",
            '$' => "\\$ ",
            '#' => "\\# ",
            _ => return None,
        })
    }
    #[inline]
    fn single(s: &str) -> Option<char> {
        let mut it = s.chars();
        let c = it.next()?;
        it.next().is_none().then_some(c)
    }
    #[inline]
    fn nary_command(chr: &str) -> Option<&'static str> {
        LatexItem::single(chr).and_then(LatexParser::nary_char_command)
    }
    #[inline]
    fn accent_command(chr: &str) -> Option<&'static str> {
        LatexItem::single(chr).and_then(LatexParser::accent_char_command)
    }
    /// 定界字符 → `\left` / `\right` 后面的 token（TS `LEFT_RIGHT_CHARS` 的反查；`""` → `.`）。
    #[inline]
    fn delim_token(ch: &str) -> Option<&'static str> {
        Some(match ch {
            "" => ".",
            "(" => "(",
            ")" => ")",
            "[" => "[",
            "]" => "]",
            "|" => "|",
            "{" => "\\{",
            "}" => "\\}",
            "‖" => "\\|",
            "⟨" => "\\langle",
            "⟩" => "\\rangle",
            "⌊" => "\\lfloor",
            "⌋" => "\\rfloor",
            "⌈" => "\\lceil",
            "⌉" => "\\rceil",
            _ => return None,
        })
    }
    /// 定界符对 → 矩阵环境（TS `MATRIX_DELIMS`；`cases` 是 `{` 配空的右侧）。
    #[inline]
    fn matrix_env(beg: &str, end: &str) -> Option<&'static str> {
        Some(match (beg, end) {
            ("(", ")") => "pmatrix",
            ("[", "]") => "bmatrix",
            ("{", "}") => "Bmatrix",
            ("|", "|") => "vmatrix",
            ("‖", "‖") => "Vmatrix",
            ("{", "") => "cases",
            _ => return None,
        })
    }
}
impl LatexParser<'_> {
    /// TS `LATEX_FUNCTIONS.has(name)`（`latex_to_omml` 用）。
    #[inline]
    fn is_latex_function(name: &str) -> bool {
        LATEX_FUNCTIONS.contains(&name)
    }
    #[inline]
    fn err(msg: impl Into<String>) -> Error {
        Error::edit(DiagCode::EditMathBadLatex, msg)
    }
    #[inline]
    fn too_deep() -> Error {
        Error::edit(DiagCode::EditMathTooDeep, "LaTeX 嵌套超过 256 层")
    }
    /// TS `NARY_OPS`：符号取自同一张 LaTeX 符号表（`nary_char`），这里只补 `limLoc`。
    #[inline]
    fn nary_op(name: &str) -> Option<(char, &'static str)> {
        let chr = LatexParser::nary_char(name)?;
        let lim_loc = match name {
            "int" | "iint" | "iiint" | "oint" => "subSup",
            _ => "undOvr",
        };
        Some((chr, lim_loc))
    }
    /// TS `MATRIX_DELIMS`：外层 `None` = 不是矩阵环境，内层 `None` = 没有定界符。
    #[inline]
    fn matrix_delims(env: &str) -> Option<Option<(&'static str, &'static str)>> {
        Some(match env {
            "matrix" => None,
            "pmatrix" => Some(("(", ")")),
            "bmatrix" => Some(("[", "]")),
            "Bmatrix" => Some(("{", "}")),
            "vmatrix" => Some(("|", "|")),
            "Vmatrix" => Some(("‖", "‖")),
            "cases" => Some(("{", "")),
            _ => return None,
        })
    }
    /// TS `LEFT_RIGHT_CHARS`。
    #[inline]
    fn left_right_char(key: &str) -> Option<&'static str> {
        Some(match key {
            "(" => "(",
            ")" => ")",
            "[" => "[",
            "]" => "]",
            "|" => "|",
            "." => "",
            "\\{" => "{",
            "\\}" => "}",
            "\\|" => "‖",
            "\\langle" => "⟨",
            "\\rangle" => "⟩",
            "\\lfloor" => "⌊",
            "\\rfloor" => "⌋",
            "\\lceil" => "⌈",
            "\\rceil" => "⌉",
            _ => return None,
        })
    }
}
impl Omml {
    /// TS `escapeXmlAttr`。
    #[inline]
    fn escape_attr(s: &str) -> String {
        String::from(FragmentText::from(s)).replace('"', "&quot;")
    }
    /// TS `mathParagraphXml`：编辑器新建的独立公式段。
    #[inline]
    pub fn paragraph(&self, align: &str) -> String {
        let omml = self.0.as_str();
        let jc = if align == "center" {
            String::new()
        } else {
            format!(r#"<w:pPr><w:jc w:val="{}"/></w:pPr>"#, Omml::escape_attr(align))
        };
        format!(
            concat!(
                r#"<w:p>{jc}<m:oMathPara><m:oMathParaPr><m:jc m:val="{align}"/></m:oMathParaPr>"#,
                r#"<m:oMath>{omml}</m:oMath></m:oMathPara></w:p>"#
            ),
            jc = jc,
            align = Omml::escape_attr(align),
            omml = omml,
        )
    }
    /// TS `mathRun`。
    #[inline]
    fn math_run(text: &str, plain: bool) -> String {
        if text.is_empty() {
            return String::new();
        }
        let rpr = if plain { r#"<m:rPr><m:sty m:val="p"/></m:rPr>"# } else { "" };
        format!(
            r#"<m:r>{rpr}<m:t xml:space="preserve">{}</m:t></m:r>"#,
            String::from(FragmentText::from(text))
        )
    }
}
impl Dom {
    // OMML（Office Math，`m:` 命名空间）的读法与两个转换器（`spec/17` 任务 6.5）。
    //
    // [`to_mathml`] 与 [`to_latex`] 是 TS `math.ts` 的 `ommlToMathML` / `ommlToLatex` 的逐字移植——差分按字符串比较，
    // `mn / mi / mo` 分类、运算符集、函数名表、转义规则都必须一样。两个转换器都是**迭代**实现（显式任务栈 +
    // 结果栈）：语料 `corpus/hostile/omml-deep.docx` 有 3,000 层嵌套，递归会把测试线程的栈吃光。
    // 这里放两者共用的小工具：语义子节点查找、属性包读取、run 文字、XML 转义。

    // OMML → LaTeX 子集（TS `ommlToLatex`，`math.ts` 502–723 的逐字移植）。
    //
    // 子集之外的结构（`m:sPre`、`m:limUpp`、认不出的 n 元运算符 / 重音 / 定界符、`\` 与换行）→ `None`，
    // 调用方只保留 token 级编辑。与 [`to_mathml`] 同一套迭代求值骨架，错误一路短路。

    /// 一个 `m:oMath` → LaTeX；子集之外 → `None`。结果 trim 并把连续空白压成一个空格。
    #[inline]
    pub fn latex(&self, omath: NodeId) -> Option<String> {
        let raw = self.eval_latex(LatexItem::Seq(omath)).ok()?;
        let mut out = String::with_capacity(raw.len());
        let mut ws = 0usize;
        for ch in raw.trim().chars() {
            if ch.is_whitespace() {
                ws += 1;
                if ws == 1 {
                    out.push(ch);
                } else if ws == 2 {
                    out.pop();
                    out.push(' ');
                }
            } else {
                ws = 0;
                out.push(ch);
            }
        }
        Some(out)
    }
    #[inline]
    fn eval_latex(&self, root: LatexItem) -> Result<String> {
        let mut tasks = vec![LatexTask::Eval(root)];
        let mut results: Vec<String> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                LatexTask::Eval(item) => {
                    if let Some(subs) = self.expand_latex(&item, &mut results)? {
                        tasks.push(LatexTask::Finish(item, subs.len()));
                        tasks.extend(subs.into_iter().rev().map(LatexTask::Eval));
                    }
                }
                LatexTask::Finish(item, arity) => {
                    let at = results.len() - arity;
                    let parts: Vec<String> = results.drain(at..).collect();
                    results.push(self.finish_latex(&item, parts)?);
                }
            }
        }
        Ok(results.pop().unwrap_or_default())
    }
    #[inline]
    fn nodes(&self, n: NodeId) -> Vec<LatexItem> {
        self.content_children(n).map(LatexItem::Node).collect()
    }
    /// 矩阵的行：有 `m:mr` 就按行 / 格，否则每个 `m:e` 一行。
    #[inline]
    fn matrix_rows(&self, node: NodeId) -> Vec<LatexItem> {
        let mut mrs = self.children_named(node, QName::new(NsId::M, LocalName::Mr)).peekable();
        if mrs.peek().is_none() {
            self.children_named(node, QName::new(NsId::M, LocalName::E))
                .map(LatexItem::Seq)
                .collect()
        } else {
            mrs.map(LatexItem::MatrixRow).collect()
        }
    }
    #[inline]
    fn expand_latex(
        &self,
        item: &LatexItem,
        results: &mut Vec<String>,
    ) -> Result<Option<Vec<LatexItem>>> {
        let slot = |n: NodeId, l: LocalName| LatexItem::Slot(n, l);
        Ok(match item {
            LatexItem::Slot(parent, name) => {
                match self.children_named(*parent, QName::new(NsId::M, *name)).next() {
                    None => {
                        results.push(String::new());
                        None
                    }
                    Some(s) => Some(self.nodes(s)),
                }
            }
            LatexItem::Seq(n) => Some(self.nodes(*n)),
            LatexItem::Binom(f) => Some(vec![slot(*f, LocalName::Num), slot(*f, LocalName::Den)]),
            LatexItem::Matrix { node, .. } => Some(self.matrix_rows(*node)),
            LatexItem::MatrixRow(mr) => Some(
                self.children_named(*mr, QName::new(NsId::M, LocalName::E))
                    .map(LatexItem::Seq)
                    .collect(),
            ),
            LatexItem::LeftRight { slot, .. } => Some(self.nodes(*slot)),
            LatexItem::Node(n) => {
                let n = *n;
                let Some(name) = self.name(n) else {
                    results.push(String::new());
                    return Ok(None);
                };
                if name.ns != NsId::M {
                    return Err(Error::LatexUnsupported);
                }
                Some(match name.local {
                    LocalName::R => {
                        results.push(self.run_to_latex(n)?);
                        return Ok(None);
                    }
                    LocalName::T => {
                        results.push(LatexItem::chars_to_latex(&self.omml_text_of(n))?);
                        return Ok(None);
                    }
                    LocalName::F => {
                        // 裸的 noBar 分式只出现在 \binom 的 m:d 包里（那边处理）；别的分式样式在子集之外
                        if self
                            .prop_val(n, LocalName::FPr, LocalName::Type)
                            .is_some_and(|t| t != "bar")
                        {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![slot(n, LocalName::Num), slot(n, LocalName::Den)]
                    }
                    LocalName::SSup => vec![slot(n, LocalName::E), slot(n, LocalName::Sup)],
                    LocalName::SSub => vec![slot(n, LocalName::E), slot(n, LocalName::Sub)],
                    LocalName::SSubSup => {
                        vec![
                            slot(n, LocalName::E),
                            slot(n, LocalName::Sub),
                            slot(n, LocalName::Sup),
                        ]
                    }
                    LocalName::Rad => {
                        if self.prop_on(n, LocalName::RadPr, LocalName::DegHide)
                            || self
                                .children_named(n, QName::new(NsId::M, LocalName::Deg))
                                .next()
                                .is_none()
                        {
                            vec![slot(n, LocalName::E)]
                        } else {
                            vec![slot(n, LocalName::Deg), slot(n, LocalName::E)]
                        }
                    }
                    LocalName::D => return self.delimiter(n).map(|it| Some(vec![it])),
                    LocalName::Nary => {
                        let chr = self
                            .prop_val(n, LocalName::NaryPr, LocalName::Chr)
                            .unwrap_or_else(|| "∫".into());
                        if LatexItem::nary_command(&chr).is_none() {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![
                            slot(n, LocalName::Sub),
                            slot(n, LocalName::Sup),
                            slot(n, LocalName::E),
                        ]
                    }
                    LocalName::Func => {
                        let name = self.plain_text_of_runs(
                            self.children_named(n, QName::new(NsId::M, LocalName::FName)).next(),
                        );
                        let name = name.trim();
                        if !(LATEX_FUNCTIONS.contains(&name)
                            || name == "lim"
                            || (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic())))
                        {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![slot(n, LocalName::E)]
                    }
                    LocalName::LimLow => {
                        if self
                            .plain_text_of_runs(
                                self.children_named(n, QName::new(NsId::M, LocalName::E)).next(),
                            )
                            .trim()
                            != "lim"
                        {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![slot(n, LocalName::Lim)]
                    }
                    LocalName::Acc => {
                        let chr = self
                            .prop_val(n, LocalName::AccPr, LocalName::Chr)
                            .unwrap_or_else(|| "\u{0302}".into());
                        if LatexItem::accent_command(&chr).is_none() {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![slot(n, LocalName::E)]
                    }
                    LocalName::Bar => vec![slot(n, LocalName::E)],
                    LocalName::GroupChr => {
                        let chr = self
                            .prop_val(n, LocalName::GroupChrPr, LocalName::Chr)
                            .unwrap_or_else(|| "\u{23DF}".into());
                        if chr != "\u{23DF}" && chr != "\u{23DE}" {
                            return Err(Error::LatexUnsupported);
                        }
                        vec![slot(n, LocalName::E)]
                    }
                    LocalName::M => {
                        return Ok(Some(vec![LatexItem::Matrix { node: n, env: "matrix".into() }]));
                    }
                    LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                        vec![slot(n, LocalName::E)]
                    }
                    _ => return Err(Error::LatexUnsupported),
                })
            }
        })
    }
    #[inline]
    fn finish_latex(&self, item: &LatexItem, parts: Vec<String>) -> Result<String> {
        let p = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
        Ok(match item {
            LatexItem::Slot(..) | LatexItem::Seq(_) => parts.concat(),
            LatexItem::Binom(_) => format!("\\binom{{{}}}{{{}}}", p(0), p(1)),
            LatexItem::Matrix { env, .. } => {
                format!("\\begin{{{env}}} {} \\end{{{env}}}", parts.join(" \\\\ "))
            }
            LatexItem::MatrixRow(_) => parts.join(" & "),
            LatexItem::LeftRight { beg, end, .. } => {
                format!("\\left{beg} {} \\right{end}", parts.concat())
            }
            LatexItem::Node(n) => {
                let n = *n;
                let Some(name) = self.name(n) else {
                    return Ok(String::new());
                };
                match name.local {
                    LocalName::F => format!("\\frac{{{}}}{{{}}}", p(0), p(1)),
                    LocalName::SSup => format!("{{{}}}^{{{}}}", p(0), p(1)),
                    LocalName::SSub => format!("{{{}}}_{{{}}}", p(0), p(1)),
                    LocalName::SSubSup => format!("{{{}}}_{{{}}}^{{{}}}", p(0), p(1), p(2)),
                    LocalName::Rad => {
                        if parts.len() == 1 {
                            format!("\\sqrt{{{}}}", p(0))
                        } else {
                            format!("\\sqrt[{}]{{{}}}", p(0), p(1))
                        }
                    }
                    LocalName::D => parts.concat(),
                    LocalName::Nary => {
                        let chr = self
                            .prop_val(n, LocalName::NaryPr, LocalName::Chr)
                            .unwrap_or_else(|| "∫".into());
                        let command =
                            LatexItem::nary_command(&chr).ok_or(Error::LatexUnsupported)?;
                        let sub = if self.prop_on(n, LocalName::NaryPr, LocalName::SubHide) {
                            String::new()
                        } else {
                            format!("_{{{}}}", p(0))
                        };
                        let sup = if self.prop_on(n, LocalName::NaryPr, LocalName::SupHide) {
                            String::new()
                        } else {
                            format!("^{{{}}}", p(1))
                        };
                        format!("\\{command}{sub}{sup} {{{}}}", p(2))
                    }
                    LocalName::Func => {
                        let name = self.plain_text_of_runs(
                            self.children_named(n, QName::new(NsId::M, LocalName::FName)).next(),
                        );
                        let name = name.trim();
                        let arg = format!("{{{}}}", p(0));
                        if LATEX_FUNCTIONS.contains(&name) || name == "lim" {
                            format!("\\{name} {arg}")
                        } else {
                            format!("\\operatorname{{{name}}} {arg}")
                        }
                    }
                    LocalName::LimLow => format!("\\lim_{{{}}}", p(0)),
                    LocalName::Acc => {
                        let chr = self
                            .prop_val(n, LocalName::AccPr, LocalName::Chr)
                            .unwrap_or_else(|| "\u{0302}".into());
                        format!(
                            "\\{}{{{}}}",
                            LatexItem::accent_command(&chr).ok_or(Error::LatexUnsupported)?,
                            p(0)
                        )
                    }
                    LocalName::Bar => {
                        let top = self.prop_val(n, LocalName::BarPr, LocalName::Pos).as_deref()
                            == Some("top");
                        format!("\\{}{{{}}}", if top { "overline" } else { "underline" }, p(0))
                    }
                    LocalName::GroupChr => {
                        let chr = self
                            .prop_val(n, LocalName::GroupChrPr, LocalName::Chr)
                            .unwrap_or_else(|| "\u{23DF}".into());
                        format!(
                            "\\{}{{{}}}",
                            if chr == "\u{23DE}" { "overbrace" } else { "underbrace" },
                            p(0)
                        )
                    }
                    LocalName::Box | LocalName::BorderBox | LocalName::Phant => p(0).to_string(),
                    // `m:m` 展开成一个 `Matrix` 项，结果就是它
                    LocalName::M => p(0).to_string(),
                    _ => return Err(Error::LatexUnsupported),
                }
            }
        })
    }
    /// `m:d`（TS `delimiterToLatex`）：`\binom`、矩阵环境、`\left … \right` 三种形态之一。
    #[inline]
    fn delimiter(&self, d: NodeId) -> Result<LatexItem> {
        let beg = self.prop_val(d, LocalName::DPr, LocalName::BegChr).unwrap_or_else(|| "(".into());
        let end = self.prop_val(d, LocalName::DPr, LocalName::EndChr).unwrap_or_else(|| ")".into());
        let mut slots = self.children_named(d, QName::new(NsId::M, LocalName::E));
        let (Some(slot), None) = (slots.next(), slots.next()) else {
            return Err(Error::LatexUnsupported);
        };
        let mut inner = self.content_children(slot);
        if let (Some(only), None) = (inner.next(), inner.next()) {
            if beg == "("
                && end == ")"
                && self.is(only, QName::new(NsId::M, LocalName::F))
                && self.prop_val(only, LocalName::FPr, LocalName::Type).as_deref() == Some("noBar")
            {
                return Ok(LatexItem::Binom(only));
            }
            if (self.is(only, QName::new(NsId::M, LocalName::M))
                || self.is(only, QName::new(NsId::M, LocalName::EqArr)))
                && let Some(env) = LatexItem::matrix_env(&beg, &end)
            {
                return Ok(LatexItem::Matrix { node: only, env: env.to_string() });
            }
        }
        let beg_tok = LatexItem::delim_token(&beg).ok_or(Error::LatexUnsupported)?;
        let end_tok = LatexItem::delim_token(&end).ok_or(Error::LatexUnsupported)?;
        Ok(LatexItem::LeftRight { beg: beg_tok.to_string(), end: end_tok.to_string(), slot })
    }
    #[inline]
    fn run_to_latex(&self, run: NodeId) -> Result<String> {
        let text = self.run_text(run);
        if !self.is_plain_run(run) {
            return LatexItem::chars_to_latex(&text);
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(" ".into());
        }
        if LATEX_FUNCTIONS.contains(&trimmed) {
            return Ok(format!("\\{trimmed} "));
        }
        if trimmed == "lim" {
            return Ok("\\lim ".into());
        }
        if text.contains(['{', '}', '\\']) {
            return Err(Error::LatexUnsupported);
        }
        Ok(format!("\\text{{{text}}}"))
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
