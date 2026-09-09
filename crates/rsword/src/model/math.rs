//! 公式与 ruby 的模型（`MOD-11`，`spec/17` 任务 6.5）。
//!
//! 公式段落（R11：`m:oMathPara` 或没有可见正文的公式段）挂 [`FormulaDisplay`]：片段节点、可编辑的 token、
//! MathML 与 LaTeX（转换器在 [`crate::model::omml`]）。文字夹公式的段落（R19）里每个 `m:oMath` 是一个
//! `Inline::Atom(Math)`，投影时按需算 token。原字节（TS 的 `omml`）在投影层按 `lex.range` 切，与 `rawRPr` 同一做法。

use crate::model::omml::{self, latex, mathml};
use crate::xml::{Dom, LocalName, NodeId, QName};

/// 一个公式段落的显示模型（TS `FormulaDisplay`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaDisplay {
    /// 段落里的 `m:oMath`，文档序（`m:oMathPara` 展开）。
    pub fragments: Vec<NodeId>,
    /// 全部 `m:t` 文本，文档序：可编辑的 token 串。
    pub tokens: Vec<String>,
    /// MathML Core，各片段拼接。段落有可见正文时不算（TS：oMathPara 旁还有普通 run 的段落只保留平铺的 token 条），
    /// 一个片段都转不出内容也没有。
    pub mathml: Option<String>,
    /// LaTeX 子集：只有一个片段、且全在子集之内才有。
    pub latex: Option<String>,
}

/// 一个段落的公式显示模型。`visible_text`：段落有可见正文（`ParagraphFacts::visible_text`）。
pub fn formula_display(dom: &Dom, para: NodeId, visible_text: bool) -> FormulaDisplay {
    let fragments = omml::fragments(dom, para);
    let tokens: Vec<String> = fragments.iter().flat_map(|&f| omml::tokens(dom, f)).collect();
    let mathml = (!visible_text)
        .then(|| fragments.iter().map(|&f| mathml::to_mathml(dom, f)).collect::<String>())
        .filter(|s| !s.is_empty());
    let latex = match fragments.as_slice() {
        [only] => latex::to_latex(dom, *only).filter(|s| !s.is_empty()),
        _ => None,
    };
    FormulaDisplay { fragments, tokens, mathml, latex }
}

/// 一个 `m:oMath` 原子的 token（R19 的公式 run：`text` = token 拼接）。
pub fn math_tokens(dom: &Dom, omath: NodeId) -> Vec<String> {
    omml::tokens(dom, omath)
}

/// `w:ruby` 的一半（`w:rt` / `w:rubyBase`）的文字：直接 `w:r` 子节点的直接 `w:t` 子节点拼接（TS `rubyPartText`）。
pub fn ruby_part_text(dom: &Dom, ruby: NodeId, part: LocalName) -> String {
    let Some(part) = dom.semantic_children(ruby).find(|&c| dom.is(c, QName::w(part))) else {
        return String::new();
    };
    let mut out = String::new();
    for r in dom.semantic_children(part).filter(|&r| dom.is(r, QName::w(LocalName::R))) {
        for t in dom.semantic_children(r).filter(|&t| dom.is(t, QName::w(LocalName::T))) {
            out.push_str(&omml::text_of(dom, t));
        }
    }
    out
}
