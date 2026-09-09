//! OMML → MathML Core（TS `ommlToMathML`，`math.ts` 55–376 的逐字移植）。
//!
//! 迭代求值：`Item` 是一个待求值的项（元素 / 槽位 / 行 …），任务栈里 `Eval(item)` 展开出子项与一个
//! `Finish(item, arity)`；`Finish` 从结果栈取回 `arity` 个子结果拼成自己的字串。一个 3,000 层的公式
//! 只是一个长一点的栈。

use super::{
    child, children_named, content_children, escape_text, is_plain_run, prop_on, prop_val, text_of,
};
use crate::xml::{Dom, LocalName, NodeId, NsId};

/// 一个 `m:oMath` → `<math display="block"><mrow>…</mrow></math>`；一个内容都没有 → `""`。
pub fn to_mathml(dom: &Dom, omath: NodeId) -> String {
    let body = eval(dom, Item::Seq(omath));
    if body.is_empty() {
        String::new()
    } else {
        format!("<math display=\"block\"><mrow>{body}</mrow></math>")
    }
}

#[derive(Clone, Copy)]
enum Item {
    /// 一个 OMML 元素。
    Node(NodeId),
    /// `parent/m:<name>` 槽位 → `<mrow>内容</mrow>`；缺失 → `<mrow></mrow>`。
    Slot(NodeId, LocalName),
    /// 某元素的内容子节点包成 `<mrow>`（`m:d` / `m:m` 的 `m:e`）。
    Row(NodeId),
    /// `m:mr` → `<mtr><mtd>…</mtd>…</mtr>`。
    Cells(NodeId),
    /// `m:eqArr/m:e` → `<mtr><mtd><mrow>…</mrow></mtd></mtr>`。
    EqRow(NodeId),
    /// 内容子节点直接拼接，不包。
    Seq(NodeId),
}

enum Task {
    Eval(Item),
    Finish(Item, usize),
}

fn mo(ch: &str, extra: &str) -> String {
    format!("<mo{extra}>{}</mo>", escape_text(ch))
}

fn eval(dom: &Dom, root: Item) -> String {
    let mut tasks = vec![Task::Eval(root)];
    let mut results: Vec<String> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Eval(item) => {
                let subs = expand(dom, item, &mut results);
                if let Some(subs) = subs {
                    tasks.push(Task::Finish(item, subs.len()));
                    tasks.extend(subs.into_iter().rev().map(Task::Eval));
                }
            }
            Task::Finish(item, arity) => {
                let at = results.len() - arity;
                let parts: Vec<String> = results.drain(at..).collect();
                results.push(finish(dom, item, parts));
            }
        }
    }
    results.pop().unwrap_or_default()
}

/// 展开一个项：叶子直接把结果压栈并返回 `None`；否则返回子项。
fn expand(dom: &Dom, item: Item, results: &mut Vec<String>) -> Option<Vec<Item>> {
    let slot = |n: NodeId, l: LocalName| Item::Slot(n, l);
    let rows =
        |n: NodeId| -> Vec<Item> { content_children(dom, n).into_iter().map(Item::Node).collect() };
    match item {
        Item::Slot(parent, name) => match child(dom, parent, name) {
            None => {
                results.push("<mrow></mrow>".to_string());
                None
            }
            Some(s) => Some(rows(s)),
        },
        Item::Row(n) | Item::Seq(n) => Some(rows(n)),
        Item::Cells(mr) => {
            Some(children_named(dom, mr, LocalName::E).into_iter().map(Item::Row).collect())
        }
        Item::EqRow(e) => Some(vec![Item::Row(e)]),
        Item::Node(n) => {
            let Some(name) = dom.name(n) else {
                results.push(String::new());
                return None;
            };
            if name.ns != NsId::M {
                // 不认识的结构：渲染它的内容子节点，别让东西凭空消失
                return Some(rows(n));
            }
            Some(match name.local {
                LocalName::R => {
                    results.push(run_to_mml(dom, n));
                    return None;
                }
                LocalName::T => {
                    results.push(run_text_to_mml(&text_of(dom, n), false));
                    return None;
                }
                LocalName::F => vec![slot(n, LocalName::Num), slot(n, LocalName::Den)],
                LocalName::SSup => vec![slot(n, LocalName::E), slot(n, LocalName::Sup)],
                LocalName::SSub => vec![slot(n, LocalName::E), slot(n, LocalName::Sub)],
                LocalName::SSubSup | LocalName::SPre => {
                    vec![slot(n, LocalName::E), slot(n, LocalName::Sub), slot(n, LocalName::Sup)]
                }
                LocalName::Rad => {
                    if prop_on(dom, n, LocalName::RadPr, LocalName::DegHide)
                        || child(dom, n, LocalName::Deg).is_none()
                    {
                        vec![slot(n, LocalName::E)]
                    } else {
                        vec![slot(n, LocalName::E), slot(n, LocalName::Deg)]
                    }
                }
                LocalName::D => {
                    children_named(dom, n, LocalName::E).into_iter().map(Item::Row).collect()
                }
                LocalName::Nary => {
                    vec![slot(n, LocalName::Sub), slot(n, LocalName::Sup), slot(n, LocalName::E)]
                }
                LocalName::Func => vec![slot(n, LocalName::FName), slot(n, LocalName::E)],
                LocalName::LimLow | LocalName::LimUpp => {
                    vec![slot(n, LocalName::E), slot(n, LocalName::Lim)]
                }
                LocalName::Acc | LocalName::Bar | LocalName::GroupChr => {
                    vec![slot(n, LocalName::E)]
                }
                LocalName::M => {
                    children_named(dom, n, LocalName::Mr).into_iter().map(Item::Cells).collect()
                }
                LocalName::EqArr => {
                    children_named(dom, n, LocalName::E).into_iter().map(Item::EqRow).collect()
                }
                LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                    vec![slot(n, LocalName::E)]
                }
                _ => rows(n),
            })
        }
    }
}

fn finish(dom: &Dom, item: Item, parts: Vec<String>) -> String {
    let joined = || parts.concat();
    let p = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
    match item {
        Item::Slot(..) | Item::Row(_) => format!("<mrow>{}</mrow>", joined()),
        Item::Seq(_) => joined(),
        Item::Cells(_) => {
            let cells: String = parts.iter().map(|c| format!("<mtd>{c}</mtd>")).collect();
            format!("<mtr>{cells}</mtr>")
        }
        Item::EqRow(_) => format!("<mtr><mtd>{}</mtd></mtr>", p(0)),
        Item::Node(n) => {
            let Some(name) = dom.name(n) else { return String::new() };
            if name.ns != NsId::M {
                return joined();
            }
            match name.local {
                LocalName::F => {
                    let attrs = match prop_val(dom, n, LocalName::FPr, LocalName::Type).as_deref() {
                        Some("noBar") => " linethickness=\"0\"",
                        Some("lin" | "skw") => " bevelled=\"true\"",
                        _ => "",
                    };
                    format!("<mfrac{attrs}>{}{}</mfrac>", p(0), p(1))
                }
                LocalName::SSup => format!("<msup>{}{}</msup>", p(0), p(1)),
                LocalName::SSub => format!("<msub>{}{}</msub>", p(0), p(1)),
                LocalName::SSubSup => format!("<msubsup>{}{}{}</msubsup>", p(0), p(1), p(2)),
                LocalName::SPre => {
                    format!("<mmultiscripts>{}<mprescripts/>{}{}</mmultiscripts>", p(0), p(1), p(2))
                }
                LocalName::Rad => {
                    if parts.len() == 1 {
                        format!("<msqrt>{}</msqrt>", p(0))
                    } else {
                        format!("<mroot>{}{}</mroot>", p(0), p(1))
                    }
                }
                LocalName::D => {
                    let beg = prop_val(dom, n, LocalName::DPr, LocalName::BegChr)
                        .unwrap_or_else(|| "(".into());
                    let end = prop_val(dom, n, LocalName::DPr, LocalName::EndChr)
                        .unwrap_or_else(|| ")".into());
                    let sep = prop_val(dom, n, LocalName::DPr, LocalName::SepChr)
                        .unwrap_or_else(|| "|".into());
                    let sep_mo = if sep.is_empty() { String::new() } else { mo(&sep, "") };
                    let body = parts.join(&sep_mo);
                    let open =
                        if beg.is_empty() { String::new() } else { mo(&beg, " stretchy=\"true\"") };
                    let close =
                        if end.is_empty() { String::new() } else { mo(&end, " stretchy=\"true\"") };
                    format!("<mrow>{open}{body}{close}</mrow>")
                }
                LocalName::Nary => {
                    let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{222B}".into());
                    let lim_loc = prop_val(dom, n, LocalName::NaryPr, LocalName::LimLoc)
                        .unwrap_or_else(|| {
                            if chr == "\u{222B}" { "subSup".into() } else { "undOvr".into() }
                        });
                    let sub_hide = prop_on(dom, n, LocalName::NaryPr, LocalName::SubHide);
                    let sup_hide = prop_on(dom, n, LocalName::NaryPr, LocalName::SupHide);
                    let op = mo(&chr, " stretchy=\"false\"");
                    let und_ovr = lim_loc == "undOvr";
                    let scripted = match (sub_hide, sup_hide) {
                        (false, false) => {
                            let tag = if und_ovr { "munderover" } else { "msubsup" };
                            format!("<{tag}>{op}{}{}</{tag}>", p(0), p(1))
                        }
                        (false, true) => {
                            let tag = if und_ovr { "munder" } else { "msub" };
                            format!("<{tag}>{op}{}</{tag}>", p(0))
                        }
                        (true, false) => {
                            let tag = if und_ovr { "mover" } else { "msup" };
                            format!("<{tag}>{op}{}</{tag}>", p(1))
                        }
                        (true, true) => op,
                    };
                    format!("<mrow>{scripted}{}</mrow>", p(2))
                }
                LocalName::Func => format!("<mrow>{}<mo>\u{2061}</mo>{}</mrow>", p(0), p(1)),
                LocalName::LimLow => format!("<munder>{}{}</munder>", p(0), p(1)),
                LocalName::LimUpp => format!("<mover>{}{}</mover>", p(0), p(1)),
                LocalName::Acc => {
                    let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{0302}".into());
                    format!("<mover accent=\"true\">{}{}</mover>", p(0), mo(&chr, ""))
                }
                LocalName::Bar => {
                    let top = prop_val(dom, n, LocalName::BarPr, LocalName::Pos).as_deref()
                        == Some("top");
                    let (tag, line) =
                        if top { ("mover", "\u{00AF}") } else { ("munder", "\u{005F}") };
                    format!("<{tag}>{}{}</{tag}>", p(0), mo(line, " stretchy=\"true\""))
                }
                LocalName::GroupChr => {
                    let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
                        .unwrap_or_else(|| "\u{23DF}".into());
                    let top = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Pos).as_deref()
                        == Some("top");
                    let tag = if top { "mover" } else { "munder" };
                    format!("<{tag}>{}{}</{tag}>", p(0), mo(&chr, " stretchy=\"true\""))
                }
                LocalName::M | LocalName::EqArr => format!("<mtable>{}</mtable>", joined()),
                LocalName::Box | LocalName::BorderBox | LocalName::Phant => p(0).to_string(),
                _ => joined(),
            }
        }
    }
}

/// `m:r` → 各 `m:t` 分类后的 token 串（`sty="p"` / `m:nor` 的 run 整段是 `<mi>`）。
fn run_to_mml(dom: &Dom, run: NodeId) -> String {
    let plain = is_plain_run(dom, run);
    children_named(dom, run, LocalName::T)
        .iter()
        .map(|&t| run_text_to_mml(&text_of(dom, t), plain))
        .collect()
}

/// TS `OPERATOR_CHARS`。
const OPERATOR_CHARS: &str = "+-−=<>±∓×÷·⋅∙*/!%&|,;:()[]{}′″∞→←↔⇒⇐⇔∈∉⊂⊃∪∩∀∃∧∨¬≤≥≠≈≡∼∝⊥∥°∂∇";

fn is_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
        || ('\u{0370}'..='\u{03FF}').contains(&ch)
        || ('\u{1D400}'..='\u{1D7FF}').contains(&ch)
}

/// 一段 run 文字 → `mn / mi / mo / mtext`（TS `runTextToMml`）。
pub(crate) fn run_text_to_mml(text: &str, plain: bool) -> String {
    if plain {
        return if text.is_empty() {
            String::new()
        } else {
            format!("<mi>{}</mi>", escape_text(text))
        };
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_digit() || ch == '.' {
            let mut num = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                num.push(chars[i]);
                i += 1;
            }
            out.push_str(&format!("<mn>{num}</mn>"));
        } else if is_letter(ch) {
            out.push_str(&format!("<mi>{}</mi>", escape_text(&ch.to_string())));
            i += 1;
        } else if ch == ' ' {
            i += 1;
        } else if OPERATOR_CHARS.contains(ch) {
            // 普通 run 里的括号是字面字符，只有 m:d 包的定界符才可伸缩
            let s = ch.to_string();
            out.push_str(&if "()[]{}|".contains(ch) {
                mo(&s, " stretchy=\"false\"")
            } else {
                mo(&s, "")
            });
            i += 1;
        } else {
            out.push_str(&format!("<mtext>{}</mtext>", escape_text(&ch.to_string())));
            i += 1;
        }
    }
    out
}
