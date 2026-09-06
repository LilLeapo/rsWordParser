//! OMML → LaTeX 子集（TS `ommlToLatex`，`math.ts` 502–723 的逐字移植）。
//!
//! 子集之外的结构（`m:sPre`、`m:limUpp`、认不出的 n 元运算符 / 重音 / 定界符、`\` 与换行）→ `None`，
//! 调用方只保留 token 级编辑。与 [`super::mathml`] 同一套迭代求值骨架，错误一路短路。

use super::{
    child, children_named, content_children, is_plain_run, plain_text_of_runs, prop_on, prop_val,
    run_text, text_of,
};
use crate::xml::{Dom, LocalName, NodeId, NsId};

struct Unsupported;

/// 一个 `m:oMath` → LaTeX；子集之外 → `None`。结果 trim 并把连续空白压成一个空格。
pub fn to_latex(dom: &Dom, omath: NodeId) -> Option<String> {
    let raw = eval(dom, Item::Seq(omath)).ok()?;
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

#[derive(Clone)]
enum Item {
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

enum Task {
    Eval(Item),
    Finish(Item, usize),
}

fn eval(dom: &Dom, root: Item) -> Result<String, Unsupported> {
    let mut tasks = vec![Task::Eval(root)];
    let mut results: Vec<String> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Eval(item) => {
                if let Some(subs) = expand(dom, &item, &mut results)? {
                    tasks.push(Task::Finish(item, subs.len()));
                    tasks.extend(subs.into_iter().rev().map(Task::Eval));
                }
            }
            Task::Finish(item, arity) => {
                let at = results.len() - arity;
                let parts: Vec<String> = results.drain(at..).collect();
                results.push(finish(dom, &item, parts)?);
            }
        }
    }
    Ok(results.pop().unwrap_or_default())
}

fn nodes(dom: &Dom, n: NodeId) -> Vec<Item> {
    content_children(dom, n).into_iter().map(Item::Node).collect()
}

/// 矩阵的行：有 `m:mr` 就按行 / 格，否则每个 `m:e` 一行。
fn matrix_rows(dom: &Dom, node: NodeId) -> Vec<Item> {
    let mrs = children_named(dom, node, LocalName::Mr);
    if mrs.is_empty() {
        children_named(dom, node, LocalName::E).into_iter().map(Item::Seq).collect()
    } else {
        mrs.into_iter().map(Item::MatrixRow).collect()
    }
}

fn expand(
    dom: &Dom,
    item: &Item,
    results: &mut Vec<String>,
) -> Result<Option<Vec<Item>>, Unsupported> {
    let slot = |n: NodeId, l: LocalName| Item::Slot(n, l);
    Ok(match item {
        Item::Slot(parent, name) => match child(dom, *parent, *name) {
            None => {
                results.push(String::new());
                None
            }
            Some(s) => Some(nodes(dom, s)),
        },
        Item::Seq(n) => Some(nodes(dom, *n)),
        Item::Binom(f) => Some(vec![slot(*f, LocalName::Num), slot(*f, LocalName::Den)]),
        Item::Matrix { node, .. } => Some(matrix_rows(dom, *node)),
        Item::MatrixRow(mr) => {
            Some(children_named(dom, *mr, LocalName::E).into_iter().map(Item::Seq).collect())
        }
        Item::LeftRight { slot, .. } => Some(nodes(dom, *slot)),
        Item::Node(n) => {
            let n = *n;
            let Some(name) = dom.name(n) else {
                results.push(String::new());
                return Ok(None);
            };
            if name.ns != NsId::M {
                return Err(Unsupported);
            }
            Some(match name.local {
                LocalName::R => {
                    results.push(run_to_latex(dom, n)?);
                    return Ok(None);
                }
                LocalName::T => {
                    results.push(chars_to_latex(&text_of(dom, n))?);
                    return Ok(None);
                }
                LocalName::F => {
                    // 裸的 noBar 分式只出现在 \binom 的 m:d 包里（那边处理）；别的分式样式在子集之外
                    if prop_val(dom, n, LocalName::FPr, LocalName::Type).is_some_and(|t| t != "bar")
                    {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::Num), slot(n, LocalName::Den)]
                }
                LocalName::SSup => vec![slot(n, LocalName::E), slot(n, LocalName::Sup)],
                LocalName::SSub => vec![slot(n, LocalName::E), slot(n, LocalName::Sub)],
                LocalName::SSubSup => {
                    vec![slot(n, LocalName::E), slot(n, LocalName::Sub), slot(n, LocalName::Sup)]
                }
                LocalName::Rad => {
                    if prop_on(dom, n, LocalName::RadPr, LocalName::DegHide)
                        || child(dom, n, LocalName::Deg).is_none()
                    {
                        vec![slot(n, LocalName::E)]
                    } else {
                        vec![slot(n, LocalName::Deg), slot(n, LocalName::E)]
                    }
                }
                LocalName::D => return delimiter(dom, n).map(|it| Some(vec![it])),
                LocalName::Nary => {
                    let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                        .unwrap_or_else(|| "∫".into());
                    if nary_command(&chr).is_none() {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::Sub), slot(n, LocalName::Sup), slot(n, LocalName::E)]
                }
                LocalName::Func => {
                    let name = plain_text_of_runs(dom, child(dom, n, LocalName::FName));
                    let name = name.trim();
                    if !(LATEX_FUNCTIONS.contains(&name)
                        || name == "lim"
                        || (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic())))
                    {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::E)]
                }
                LocalName::LimLow => {
                    if plain_text_of_runs(dom, child(dom, n, LocalName::E)).trim() != "lim" {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::Lim)]
                }
                LocalName::Acc => {
                    let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{0302}".into());
                    if accent_command(&chr).is_none() {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::E)]
                }
                LocalName::Bar => vec![slot(n, LocalName::E)],
                LocalName::GroupChr => {
                    let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{23DF}".into());
                    if chr != "\u{23DF}" && chr != "\u{23DE}" {
                        return Err(Unsupported);
                    }
                    vec![slot(n, LocalName::E)]
                }
                LocalName::M => {
                    return Ok(Some(vec![Item::Matrix { node: n, env: "matrix".into() }]));
                }
                LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                    vec![slot(n, LocalName::E)]
                }
                _ => return Err(Unsupported),
            })
        }
    })
}

fn finish(dom: &Dom, item: &Item, parts: Vec<String>) -> Result<String, Unsupported> {
    let p = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
    Ok(match item {
        Item::Slot(..) | Item::Seq(_) => parts.concat(),
        Item::Binom(_) => format!("\\binom{{{}}}{{{}}}", p(0), p(1)),
        Item::Matrix { env, .. } => {
            format!("\\begin{{{env}}} {} \\end{{{env}}}", parts.join(" \\\\ "))
        }
        Item::MatrixRow(_) => parts.join(" & "),
        Item::LeftRight { beg, end, .. } => format!("\\left{beg} {} \\right{end}", parts.concat()),
        Item::Node(n) => {
            let n = *n;
            let Some(name) = dom.name(n) else { return Ok(String::new()) };
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
                    let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                        .unwrap_or_else(|| "∫".into());
                    let command = nary_command(&chr).ok_or(Unsupported)?;
                    let sub = if prop_on(dom, n, LocalName::NaryPr, LocalName::SubHide) {
                        String::new()
                    } else {
                        format!("_{{{}}}", p(0))
                    };
                    let sup = if prop_on(dom, n, LocalName::NaryPr, LocalName::SupHide) {
                        String::new()
                    } else {
                        format!("^{{{}}}", p(1))
                    };
                    format!("\\{command}{sub}{sup} {{{}}}", p(2))
                }
                LocalName::Func => {
                    let name = plain_text_of_runs(dom, child(dom, n, LocalName::FName));
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
                    let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{0302}".into());
                    format!("\\{}{{{}}}", accent_command(&chr).ok_or(Unsupported)?, p(0))
                }
                LocalName::Bar => {
                    let top = prop_val(dom, n, LocalName::BarPr, LocalName::Pos).as_deref()
                        == Some("top");
                    format!("\\{}{{{}}}", if top { "overline" } else { "underline" }, p(0))
                }
                LocalName::GroupChr => {
                    let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
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
                _ => return Err(Unsupported),
            }
        }
    })
}

/// `m:d`（TS `delimiterToLatex`）：`\binom`、矩阵环境、`\left … \right` 三种形态之一。
fn delimiter(dom: &Dom, d: NodeId) -> Result<Item, Unsupported> {
    let beg = prop_val(dom, d, LocalName::DPr, LocalName::BegChr).unwrap_or_else(|| "(".into());
    let end = prop_val(dom, d, LocalName::DPr, LocalName::EndChr).unwrap_or_else(|| ")".into());
    let slots = children_named(dom, d, LocalName::E);
    let [slot] = slots.as_slice() else { return Err(Unsupported) };
    let inner = content_children(dom, *slot);
    if let [only] = inner.as_slice() {
        let only = *only;
        if beg == "("
            && end == ")"
            && dom.is(only, super::m(LocalName::F))
            && prop_val(dom, only, LocalName::FPr, LocalName::Type).as_deref() == Some("noBar")
        {
            return Ok(Item::Binom(only));
        }
        if (dom.is(only, super::m(LocalName::M)) || dom.is(only, super::m(LocalName::EqArr)))
            && let Some(env) = matrix_env(&beg, &end)
        {
            return Ok(Item::Matrix { node: only, env: env.to_string() });
        }
    }
    let beg_tok = delim_token(&beg).ok_or(Unsupported)?;
    let end_tok = delim_token(&end).ok_or(Unsupported)?;
    Ok(Item::LeftRight { beg: beg_tok.to_string(), end: end_tok.to_string(), slot: *slot })
}

fn run_to_latex(dom: &Dom, run: NodeId) -> Result<String, Unsupported> {
    let text = run_text(dom, run);
    if !is_plain_run(dom, run) {
        return chars_to_latex(&text);
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
        return Err(Unsupported);
    }
    Ok(format!("\\text{{{text}}}"))
}

/// 普通数学文字：解析器的特殊字符转义，符号换成 `\命令 `（TS `charsToLatex`）。
fn chars_to_latex(text: &str) -> Result<String, Unsupported> {
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '\\' || ch == '\n' {
            return Err(Unsupported);
        }
        if let Some(esc) = char_escape(ch) {
            out.push_str(esc);
            continue;
        }
        match symbol_command(ch) {
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

/// 三张 TS 表的反查：符号 / n 元运算符 / 重音 → 命令名（同一字符有多个名字时**第一个**赢）。
macro_rules! latex_symbols {
    ($fn:ident: $($name:literal => $ch:literal),+ $(,)?) => {
        /// 字符 → `\命令`（表序，别名取第一个）。
        fn $fn(ch: char) -> Option<&'static str> {
            $( if ch == $ch { return Some($name); } )+
            None
        }
    };
}

latex_symbols! { symbol_command:
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

latex_symbols! { accent_char_command:
    "hat" => '\u{0302}', "bar" => '\u{0304}', "vec" => '\u{20D7}', "dot" => '\u{0307}', "ddot" => '\u{0308}',
    "tilde" => '\u{0303}', "check" => '\u{030C}', "breve" => '\u{0306}',
}

latex_symbols! { nary_char_command:
    "sum" => '∑', "prod" => '∏', "coprod" => '∐', "bigcup" => '⋃', "bigcap" => '⋂', "int" => '∫',
    "iint" => '∬', "iiint" => '∭', "oint" => '∮',
}

fn single(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

fn nary_command(chr: &str) -> Option<&'static str> {
    single(chr).and_then(nary_char_command)
}

fn accent_command(chr: &str) -> Option<&'static str> {
    single(chr).and_then(accent_char_command)
}

/// TS `LATEX_FUNCTIONS`。
const LATEX_FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos",
    "arctan", "ln", "log", "exp", "max", "min", "sup", "inf", "arg", "det", "gcd", "deg", "dim",
    "ker", "mod",
];

/// 定界字符 → `\left` / `\right` 后面的 token（TS `LEFT_RIGHT_CHARS` 的反查；`""` → `.`）。
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
