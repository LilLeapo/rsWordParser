//! 公式与 ruby 的投影（`COMPAT-07`，任务 6.5）：公式块的 `formulaDisplay` / `previewText`，文字夹公式的
//! `runs[].math`，`runs[].ruby`。原字节按 `lex.range` 切（与 `rawRPr` 同一做法）。

use serde_json::{Map, Value};

use crate::model::math::{FormulaDisplay, math_tokens};
use crate::model::{Display, ProtectedBlock};
use crate::xml::NodeId;

use super::blocks::Ctx;
use crate::bind::native::json::{display_json, set};

/// 公式块（R11，`label: "Equation"` 由调用方写）。TS：`previewText` = token 拼接，`formulaDisplay` 四个字段里
/// `mathml` / `omml` / `latex` 空则不给。
pub(super) fn formula_block(ctx: &Ctx<'_>, pb: &ProtectedBlock, o: &mut Map<String, Value>) {
    let Some(f) = pb.display.as_ref().and_then(Display::as_formula) else { return };
    set(o, "previewText", f.tokens.concat());
    set(o, "formulaDisplay", Value::Object(formula_json(ctx, f)));
}

fn formula_json(ctx: &Ctx<'_>, f: &FormulaDisplay) -> Map<String, Value> {
    let omml: String = f.fragments.iter().map(|&n| ctx.node_xml(n)).collect();
    display_json! {
        "tokens" => f.tokens.clone(),
        opt "mathml" => f.mathml.clone(),
        opt "omml" => (!omml.is_empty()).then_some(omml),
        opt "latex" => f.latex.clone(),
    }
}

/// 文字夹公式的段落里一个 `m:oMath` 原子 → `{ text: token 拼接, math: { omml: 原字节 } }`。
pub(super) fn math_run(ctx: &Ctx<'_>, omath: NodeId) -> Map<String, Value> {
    display_json! {
        "text" => math_tokens(ctx.dom, omath).concat(),
        "math" => Value::Object(display_json! { "omml" => ctx.node_xml(omath).to_string() }),
    }
}

/// `w:ruby` 的 run → `{ text: 被注正文, ruby: { rt, xml: 整个 w:ruby 原字节 } }`（TS 不带格式键）。
pub(super) fn ruby_run(ctx: &Ctx<'_>, ruby: NodeId, rt: &str, base: &str) -> Map<String, Value> {
    display_json! {
        "text" => base,
        "ruby" => Value::Object(display_json! { "rt" => rt, "xml" => ctx.node_xml(ruby).to_string() }),
    }
}
