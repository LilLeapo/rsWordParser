//! OMML（Office Math，`m:` 命名空间）的读法与两个转换器（`spec/17` 任务 6.5）。
//!
//! [`mathml`] 与 [`latex`] 是 TS `math.ts` 的 `ommlToMathML` / `ommlToLatex` 的逐字移植——差分按字符串比较，
//! `mn / mi / mo` 分类、运算符集、函数名表、转义规则都必须一样。两个转换器都是**迭代**实现（显式任务栈 +
//! 结果栈）：语料 `corpus/hostile/omml-deep.docx` 有 3,000 层嵌套，递归会把测试线程的栈吃光。
//! 这里放两者共用的小工具：语义子节点查找、属性包读取、run 文字、XML 转义。

pub mod latex;
pub mod mathml;

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

pub(crate) fn m(local: LocalName) -> QName {
    QName::new(NsId::M, local)
}

/// 第一个名为 `m:<local>` 的语义子节点。
pub(crate) fn child(dom: &Dom, node: NodeId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&c| dom.is(c, m(local)))
}

/// 全部名为 `m:<local>` 的语义子节点，文档序。
pub(crate) fn children_named(dom: &Dom, node: NodeId, local: LocalName) -> Vec<NodeId> {
    dom.semantic_children(node).filter(|&c| dom.is(c, m(local))).collect()
}

/// 内容子节点：元素，且名字不以 `Pr` 结尾（TS `contentChildren`：属性包不是内容）。
pub(crate) fn content_children(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    dom.semantic_children(node)
        .filter(|&c| dom.name(c).is_some())
        .filter(|&c| !dom.lex_name(c).is_some_and(|q| q.ends_with("Pr")))
        .collect()
}

/// `node/m:<pr>/m:<child>/@m:val`（TS `propVal`）。
pub(crate) fn prop_val(dom: &Dom, node: NodeId, pr: LocalName, name: LocalName) -> Option<String> {
    let pr = child(dom, node, pr)?;
    let c = child(dom, pr, name)?;
    dom.attr_value(c, m(LocalName::Val)).map(|v| v.into_owned())
}

/// 属性存在且不是 `0` / `false` / `off`（TS `propOn`）。
pub(crate) fn prop_on(dom: &Dom, node: NodeId, pr: LocalName, name: LocalName) -> bool {
    prop_val(dom, node, pr, name)
        .is_some_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
}

/// 一个 `m:t` 的文本（实体已解码）。
pub(crate) fn text_of(dom: &Dom, node: NodeId) -> String {
    let mut s = String::new();
    for c in dom.semantic_children(node) {
        if let Some(t) = dom.text(c) {
            s.push_str(&t);
        }
    }
    s
}

/// 一个 `m:r` 的全部 `m:t` 文本拼接。
pub(crate) fn run_text(dom: &Dom, run: NodeId) -> String {
    children_named(dom, run, LocalName::T).iter().map(|&t| text_of(dom, t)).collect()
}

/// `m:rPr/m:sty = "p"` 或有 `m:rPr/m:nor`：普通文字（不按数学斜体分类）。
pub(crate) fn is_plain_run(dom: &Dom, run: NodeId) -> bool {
    let sty = prop_val(dom, run, LocalName::RPr, LocalName::Sty);
    sty.as_deref() == Some("p")
        || child(dom, run, LocalName::RPr)
            .is_some_and(|pr| child(dom, pr, LocalName::Nor).is_some())
}

/// 容器下全部 `m:r` 的文字拼接（TS `plainTextOfRuns`：函数名 / `lim`）。
pub(crate) fn plain_text_of_runs(dom: &Dom, node: Option<NodeId>) -> String {
    let Some(node) = node else { return String::new() };
    children_named(dom, node, LocalName::R).iter().map(|&r| run_text(dom, r)).collect()
}

/// TS `escapeXmlText`：去掉 XML 1.0 不允许的控制字符，转义 `& < >`。
pub(crate) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// 一个节点下全部 `m:oMath` 片段，文档序（`m:oMathPara` 展开；`m:oMath` 不嵌套）。
pub fn fragments(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    dom.semantic_descendants(node).filter(|&n| dom.is(n, m(LocalName::OMath))).collect()
}

/// 片段里全部 `m:t` 的文本，文档序（TS `mathTokens`：可编辑的公式 token）。
pub fn tokens(dom: &Dom, omath: NodeId) -> Vec<String> {
    dom.semantic_descendants(omath)
        .filter(|&n| dom.is(n, m(LocalName::T)))
        .map(|t| text_of(dom, t))
        .collect()
}
