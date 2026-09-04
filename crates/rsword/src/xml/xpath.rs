//! XPath 子集求值（`TEST-05`）：用于 `EDIT-03` / `SAVE-*` 用例断言与 `COMPAT-08` 的等价比较
//! （比较两份 part 的一组 XPath 结果，而不是字节）。
//!
//! 支持的语法（够写断言，不追求完整 XPath 1.0）：
//!
//! ```text
//! Expr      := count( Path ) | string( Path ) | normalize-space( Path ) | Path
//! Path      := [ '/' | '//' ] Step { ( '/' | '//' ) Step }
//! Step      := ( '@' Name | text() | '.' | '..' | '*' | Name ) { Predicate }
//! Predicate := '[' ( Number | last() | @Name | @Name = Literal | @Name != Literal
//!                  | RelPath | RelPath = Literal | text() = Literal ) ']'
//! RelPath   := Name { '/' Name } [ '/' '@' Name [ = Literal ] ]   // 谓词里的子路径：存在性、字符串值或属性比较
//! Name      := prefix ':' local | local
//! ```
//!
//! 前缀表固定为 `schema/namespaces.tsv` 的规范前缀（`w`、`r`、`a`、`wp`、`m`、`mc`、`w14`……），
//! 按 `QName` 匹配，因此同一表达式对 Strict 与 Transitional part 都成立。
//! `/` 开头从文档节点起算（`/w:document/w:body/w:p`）；相对路径从给定的上下文节点起算。

use std::fmt;

use crate::xml::Dirty;
use crate::xml::dom::{Dom, NodeId};
use crate::xml::names::NsId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPathError {
    pub message: String,
    pub at: usize,
}

impl fmt::Display for XPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "xpath: {} (at {})", self.message, self.at)
    }
}

impl std::error::Error for XPathError {}

/// 求值结果。
#[derive(Debug, Clone, PartialEq)]
pub enum XValue {
    Nodes(Vec<NodeId>),
    /// 属性值 / `text()` 的文本序列。
    Strings(Vec<String>),
    Number(f64),
}

impl XValue {
    /// 统一为字符串列表（节点取字符串值），供比较与打印。
    pub fn to_strings(&self, dom: &Dom) -> Vec<String> {
        match self {
            XValue::Nodes(ns) => ns.iter().map(|&n| string_value(dom, n)).collect(),
            XValue::Strings(s) => s.clone(),
            XValue::Number(n) => vec![format_number(*n)],
        }
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { n.to_string() }
}

/// 节点的字符串值：元素为全部后代文本拼接，文本节点为其文本。
pub fn string_value(dom: &Dom, node: NodeId) -> String {
    if let Some(t) = dom.text(node) {
        return t.into_owned();
    }
    let mut out = String::new();
    for d in dom.descendants(node) {
        if dom.node(d).dirty == Dirty::Deleted {
            continue;
        }
        if let Some(t) = dom.text(d) {
            out.push_str(&t);
        }
    }
    out
}

/// 从文档节点（`dom.root()` 的"父"：绝对路径的起点）求值。
pub fn eval(dom: &Dom, expr: &str) -> Result<XValue, XPathError> {
    eval_at(dom, None, expr)
}

/// 以 `context` 为上下文节点求值（相对路径从它的子节点起算）；`None` = 文档节点。
pub fn eval_at(dom: &Dom, context: Option<NodeId>, expr: &str) -> Result<XValue, XPathError> {
    let mut p = Parser { s: expr, pos: 0 };
    let v = p.expr(dom, context)?;
    p.skip_ws();
    if p.pos != expr.len() {
        return Err(p.err("表达式末尾有多余字符"));
    }
    Ok(v)
}

/// 求值并转成字符串列表。
pub fn eval_strings(dom: &Dom, expr: &str) -> Result<Vec<String>, XPathError> {
    Ok(eval(dom, expr)?.to_strings(dom))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Axis {
    Child,
    Descendant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeTest {
    /// `prefix:local` / `local`；`ns == None` 且 `prefix_given == false` 表示无命名空间。
    Name {
        ns: NsId,
        local: String,
    },
    Any,
    Text,
    Self_,
    Parent,
    Attr {
        ns: NsId,
        local: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Pred {
    Index(usize),
    Last,
    HasAttr(NsId, String),
    AttrEq(NsId, String, String, bool),
    /// 相对路径（子元素链）存在。
    HasPath(Vec<(NsId, String)>),
    /// 相对路径终点的字符串值等于字面。
    PathEq(Vec<(NsId, String)>, String),
    /// 相对路径终点的属性：存在，或等于字面（`w:pPr/w:pStyle/@w:val='Heading1'`）。
    PathAttr(Vec<(NsId, String)>, NsId, String, Option<String>),
    TextEq(String),
}

struct Step {
    axis: Axis,
    test: NodeTest,
    preds: Vec<Pred>,
}

struct Parser<'s> {
    s: &'s str,
    pos: usize,
}

impl<'s> Parser<'s> {
    fn err(&self, m: &str) -> XPathError {
        XPathError { message: m.to_string(), at: self.pos }
    }

    fn rest(&self) -> &'s str {
        &self.s[self.pos..]
    }

    fn skip_ws(&mut self) {
        while self.rest().starts_with(' ') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        self.skip_ws();
        if self.rest().starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, lit: &str) -> Result<(), XPathError> {
        if self.eat(lit) { Ok(()) } else { Err(self.err(&format!("期待 `{lit}`"))) }
    }

    fn expr(&mut self, dom: &Dom, ctx: Option<NodeId>) -> Result<XValue, XPathError> {
        self.skip_ws();
        for (func, kind) in [("count(", 0), ("string(", 1), ("normalize-space(", 2)] {
            if self.eat(func) {
                let nodes = self.path(dom, ctx)?;
                self.expect(")")?;
                return Ok(match kind {
                    0 => XValue::Number(match &nodes {
                        XValue::Nodes(n) => n.len() as f64,
                        XValue::Strings(s) => s.len() as f64,
                        XValue::Number(_) => 1.0,
                    }),
                    1 => XValue::Strings(vec![
                        nodes.to_strings(dom).into_iter().next().unwrap_or_default(),
                    ]),
                    _ => XValue::Strings(vec![
                        nodes
                            .to_strings(dom)
                            .into_iter()
                            .next()
                            .unwrap_or_default()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ]),
                });
            }
        }
        self.path(dom, ctx)
    }

    fn path(&mut self, dom: &Dom, ctx: Option<NodeId>) -> Result<XValue, XPathError> {
        self.skip_ws();
        // 上下文集合：文档节点用 None 表示，其"子节点"是根元素
        let mut current: Vec<Option<NodeId>> = vec![ctx];
        let mut first_axis = Axis::Child;
        if self.eat("//") {
            first_axis = Axis::Descendant;
            current = vec![None];
        } else if self.eat("/") {
            current = vec![None];
        }
        let mut steps = Vec::new();
        let mut axis = first_axis;
        loop {
            let step = self.step(axis)?;
            steps.push(step);
            if self.eat("//") {
                axis = Axis::Descendant;
            } else if self.eat("/") {
                axis = Axis::Child;
            } else {
                break;
            }
        }
        let mut strings: Option<Vec<String>> = None;
        for (i, step) in steps.iter().enumerate() {
            if strings.is_some() {
                return Err(self.err("属性或 text() 之后不能再有步"));
            }
            match &step.test {
                NodeTest::Attr { ns, local } => {
                    let mut out = Vec::new();
                    for c in &current {
                        let Some(n) = *c else { continue };
                        let nodes = if step.axis == Axis::Descendant {
                            descendants(dom, n)
                        } else {
                            vec![n]
                        };
                        for node in nodes {
                            if let Some(v) = attr_value(dom, node, *ns, local) {
                                out.push(v);
                            }
                        }
                    }
                    strings = Some(out);
                }
                NodeTest::Text => {
                    let mut out = Vec::new();
                    for c in &current {
                        let Some(n) = *c else { continue };
                        let scope = if step.axis == Axis::Descendant {
                            descendants(dom, n)
                        } else {
                            vec![n]
                        };
                        for node in scope {
                            for ch in children(dom, node) {
                                if let Some(t) = dom.text(ch) {
                                    out.push(t.into_owned());
                                }
                            }
                        }
                    }
                    strings = Some(out);
                }
                _ => {
                    let mut next: Vec<Option<NodeId>> = Vec::new();
                    for c in &current {
                        let matched = select(dom, *c, step);
                        for m in matched {
                            if !next.contains(&Some(m)) {
                                next.push(Some(m));
                            }
                        }
                    }
                    // 按文档序：NodeId 是前序分配的，排序即文档序
                    next.sort_by_key(|n| n.map(|x| x.0));
                    current = next;
                }
            }
            let _ = i;
        }
        Ok(match strings {
            Some(s) => XValue::Strings(s),
            None => XValue::Nodes(current.into_iter().flatten().collect()),
        })
    }

    fn step(&mut self, axis: Axis) -> Result<Step, XPathError> {
        self.skip_ws();
        let test = if self.eat("@") {
            let (ns, local) = self.name()?;
            NodeTest::Attr { ns, local }
        } else if self.eat("text()") {
            NodeTest::Text
        } else if self.eat("..") {
            NodeTest::Parent
        } else if self.eat(".") {
            NodeTest::Self_
        } else if self.eat("*") {
            NodeTest::Any
        } else {
            let (ns, local) = self.name()?;
            NodeTest::Name { ns, local }
        };
        let mut preds = Vec::new();
        while self.eat("[") {
            preds.push(self.predicate()?);
            self.expect("]")?;
        }
        Ok(Step { axis, test, preds })
    }

    fn predicate(&mut self) -> Result<Pred, XPathError> {
        self.skip_ws();
        if self.eat("last()") {
            return Ok(Pred::Last);
        }
        if self.eat("text()") {
            self.expect("=")?;
            return Ok(Pred::TextEq(self.literal()?));
        }
        let start = self.pos;
        let digits = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && !self.rest()[digits..].starts_with(|c: char| c.is_alphanumeric() || c == ':')
        {
            self.pos += digits;
            let n: usize = self.s[start..self.pos].parse().map_err(|_| self.err("下标不合法"))?;
            if n == 0 {
                return Err(self.err("XPath 下标从 1 起"));
            }
            return Ok(Pred::Index(n));
        }
        if self.eat("@") {
            let (ns, local) = self.name()?;
            if self.eat("!=") {
                return Ok(Pred::AttrEq(ns, local, self.literal()?, false));
            }
            if self.eat("=") {
                return Ok(Pred::AttrEq(ns, local, self.literal()?, true));
            }
            return Ok(Pred::HasAttr(ns, local));
        }
        let mut path = vec![self.name()?];
        while self.rest().starts_with('/') && !self.rest().starts_with("//") {
            self.pos += 1;
            if self.eat("@") {
                let (ns, local) = self.name()?;
                let value = if self.eat("=") { Some(self.literal()?) } else { None };
                return Ok(Pred::PathAttr(path, ns, local, value));
            }
            path.push(self.name()?);
        }
        if self.eat("=") {
            return Ok(Pred::PathEq(path, self.literal()?));
        }
        Ok(Pred::HasPath(path))
    }

    fn literal(&mut self) -> Result<String, XPathError> {
        self.skip_ws();
        let q = self.rest().chars().next().ok_or_else(|| self.err("期待字符串字面"))?;
        if q != '\'' && q != '"' {
            return Err(self.err("字符串须用引号"));
        }
        self.pos += 1;
        let end = self.rest().find(q).ok_or_else(|| self.err("字符串未闭合"))?;
        let s = self.rest()[..end].to_string();
        self.pos += end + 1;
        Ok(s)
    }

    fn name(&mut self) -> Result<(NsId, String), XPathError> {
        self.skip_ws();
        let is_name_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
        let len =
            self.rest().chars().take_while(|&c| is_name_char(c)).map(char::len_utf8).sum::<usize>();
        if len == 0 {
            return Err(self.err("期待名字"));
        }
        let first = &self.rest()[..len];
        self.pos += len;
        if self.rest().starts_with(':') {
            self.pos += 1;
            let len2 = self
                .rest()
                .chars()
                .take_while(|&c| is_name_char(c))
                .map(char::len_utf8)
                .sum::<usize>();
            if len2 == 0 {
                return Err(self.err("前缀后期待局部名"));
            }
            let local = self.rest()[..len2].to_string();
            self.pos += len2;
            let ns = prefix_to_ns(first).ok_or_else(|| self.err(&format!("未知前缀 `{first}`")))?;
            return Ok((ns, local));
        }
        Ok((NsId::None, first.to_string()))
    }
}

/// 规范前缀 → `NsId`（`schema/namespaces.tsv`）。
pub fn prefix_to_ns(prefix: &str) -> Option<NsId> {
    NsId::KNOWN.iter().copied().find(|ns| ns.canonical_prefix() == Some(prefix))
}

/// 前缀表（供工具打印）。
pub fn prefixes() -> Vec<(&'static str, NsId)> {
    NsId::KNOWN
        .iter()
        .copied()
        .filter_map(|ns| ns.canonical_prefix().filter(|p| !p.is_empty()).map(|p| (p, ns)))
        .collect()
}

fn children(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    dom.children(node).iter().copied().filter(|&c| dom.node(c).dirty != Dirty::Deleted).collect()
}

fn descendants(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    dom.descendants(node).filter(|&d| dom.node(d).dirty != Dirty::Deleted).collect()
}

fn attr_value(dom: &Dom, node: NodeId, ns: NsId, local: &str) -> Option<String> {
    let e = dom.element(node)?;
    let interner = dom.interner();
    e.attrs
        .iter()
        .find(|a| a.name.ns == ns && a.name.local.as_str(interner) == local)
        .map(|a| dom.attr_str(a).into_owned())
}

fn name_matches(dom: &Dom, node: NodeId, ns: NsId, local: &str) -> bool {
    dom.name(node).is_some_and(|q| q.ns == ns && q.local.as_str(dom.interner()) == local)
}

/// 一步：从上下文节点（`None` = 文档节点）出发选出节点并应用谓词。
fn select(dom: &Dom, ctx: Option<NodeId>, step: &Step) -> Vec<NodeId> {
    let candidates: Vec<NodeId> = match (&step.test, ctx) {
        (NodeTest::Self_, Some(n)) => vec![n],
        (NodeTest::Self_, None) => vec![],
        (NodeTest::Parent, Some(n)) => dom.parent(n).into_iter().collect(),
        (NodeTest::Parent, None) => vec![],
        (_, None) => {
            // 文档节点的子节点 = 根元素；`//` 从文档节点起 = 全部元素
            let root = dom.root();
            let scope =
                if step.axis == Axis::Descendant { descendants(dom, root) } else { vec![root] };
            scope.into_iter().filter(|&n| test_matches(dom, n, &step.test)).collect()
        }
        (_, Some(n)) => {
            let scope = if step.axis == Axis::Descendant {
                let mut v = Vec::new();
                for c in children(dom, n) {
                    v.extend(descendants(dom, c));
                }
                v
            } else {
                children(dom, n)
            };
            scope.into_iter().filter(|&n| test_matches(dom, n, &step.test)).collect()
        }
    };
    let mut result = candidates;
    for p in &step.preds {
        result = match p {
            Pred::Index(i) => result.get(*i - 1).copied().into_iter().collect(),
            Pred::Last => result.last().copied().into_iter().collect(),
            Pred::HasAttr(ns, l) => {
                result.into_iter().filter(|&n| attr_value(dom, n, *ns, l).is_some()).collect()
            }
            Pred::AttrEq(ns, l, v, eq) => result
                .into_iter()
                .filter(|&n| (attr_value(dom, n, *ns, l).as_deref() == Some(v.as_str())) == *eq)
                .collect(),
            Pred::HasPath(path) => {
                result.into_iter().filter(|&n| !follow_path(dom, n, path).is_empty()).collect()
            }
            Pred::PathEq(path, v) => result
                .into_iter()
                .filter(|&n| follow_path(dom, n, path).iter().any(|&t| string_value(dom, t) == *v))
                .collect(),
            Pred::PathAttr(path, ns, l, v) => result
                .into_iter()
                .filter(|&n| {
                    follow_path(dom, n, path).iter().any(|&t| {
                        match (attr_value(dom, t, *ns, l), v) {
                            (Some(a), Some(want)) => a == *want,
                            (Some(_), None) => true,
                            (None, _) => false,
                        }
                    })
                })
                .collect(),
            Pred::TextEq(v) => result.into_iter().filter(|&n| string_value(dom, n) == *v).collect(),
        };
    }
    result
}

/// 从 `node` 沿子元素链走：每一步取全部同名子元素。
fn follow_path(dom: &Dom, node: NodeId, path: &[(NsId, String)]) -> Vec<NodeId> {
    let mut current = vec![node];
    for (ns, local) in path {
        let mut next = Vec::new();
        for n in current {
            next.extend(children(dom, n).into_iter().filter(|&c| name_matches(dom, c, *ns, local)));
        }
        current = next;
    }
    current
}

fn test_matches(dom: &Dom, node: NodeId, test: &NodeTest) -> bool {
    match test {
        NodeTest::Any => dom.name(node).is_some(),
        NodeTest::Name { ns, local } => name_matches(dom, node, *ns, local),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const WS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

    fn doc(ns: &str) -> Dom {
        let xml = format!(
            r#"<w:document xmlns:w="{ns}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>
              <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> World</w:t></w:r></w:p>
              <w:p><w:r><w:t>Second</w:t></w:r></w:p>
              <w:p/>
              <w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>"#
        );
        Dom::parse(PartId(0), xml.as_bytes()).unwrap()
    }

    fn strs(dom: &Dom, expr: &str) -> Vec<String> {
        eval_strings(dom, expr).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn test_05_xpath_subset() {
        let d = doc(W);
        assert_eq!(strs(&d, "count(/w:document/w:body/w:p)"), ["3"]);
        assert_eq!(strs(&d, "count(//w:r)"), ["3"]);
        assert_eq!(strs(&d, "count(//w:p[w:pPr])"), ["1"]);
        assert_eq!(strs(&d, "count(/w:document/w:body/w:p[1]/w:r)"), ["2"]);
        assert_eq!(strs(&d, "/w:document/w:body/w:p[1]/w:pPr/w:pStyle/@w:val"), ["Heading1"]);
        assert_eq!(strs(&d, "//w:p[1]/w:r[2]/w:t/text()"), [" World"]);
        assert_eq!(strs(&d, "string(//w:p[1])"), ["Hello World"]);
        assert_eq!(strs(&d, "normalize-space(//w:p[1])"), ["Hello World"]);
        assert_eq!(strs(&d, "//w:p[last()]"), [""]);
        assert_eq!(strs(&d, "//w:t[text()='Second']/../..[1]"), ["Second"]);
        assert_eq!(strs(&d, "//w:r[w:rPr]/w:t"), ["Hello"]);
        assert_eq!(strs(&d, "//w:r[w:t='Second']"), ["Second"]);
        assert_eq!(strs(&d, "count(//w:pgSz[@w:w='11906'])"), ["1"]);
        assert_eq!(strs(&d, "count(//w:pgSz[@w:w!='11906'])"), ["0"]);
        assert_eq!(strs(&d, "count(//w:pgSz[@w:h])"), ["1"]);
        assert_eq!(strs(&d, "//w:body/*[last()]/w:pgSz/@w:w"), ["11906"]);
        assert_eq!(strs(&d, "count(//w:t[@xml:space])"), ["1"]);
        assert_eq!(strs(&d, "count(//w:nothing)"), ["0"]);
        assert_eq!(strs(&d, "count(/w:document/w:body/w:p[w:pPr/w:pStyle])"), ["1"]);
        assert_eq!(strs(&d, "count(//w:p[w:r/w:t='Second'])"), ["1"]);
        assert_eq!(strs(&d, "count(//w:body[w:p/w:pPr/w:pStyle])"), ["1"]);
        assert_eq!(
            strs(&d, "//w:p[w:pPr/w:pStyle/@w:val='Heading1']/w:r[1]/w:t/text()"),
            ["Hello"]
        );
        assert_eq!(strs(&d, "count(//w:p[w:pPr/w:pStyle/@w:val])"), ["1"]);
        assert_eq!(strs(&d, "count(//w:p[w:pPr/w:pStyle/@w:val='Nope'])"), ["0"]);
        assert!(eval(&d, "//zz:p").is_err(), "未知前缀");
        assert!(eval(&d, "//w:p[0]").is_err(), "下标从 1 起");
        assert!(eval(&d, "//w:p/@w:val/w:x").is_err(), "属性后不能再有步");
        // 相对路径从上下文节点起算
        let body = d.semantic_children(d.root()).next().unwrap();
        let v = eval_at(&d, Some(body), "w:p[2]/w:r/w:t").unwrap();
        assert_eq!(v.to_strings(&d), ["Second"]);
        // Strict 与 Transitional 同一表达式
        let s = doc(WS);
        assert_eq!(strs(&s, "count(/w:document/w:body/w:p)"), ["3"]);
        assert_eq!(strs(&s, "//w:pStyle/@w:val"), ["Heading1"]);
        assert!(prefixes().iter().any(|(p, ns)| *p == "w" && *ns == NsId::W));
    }
}
