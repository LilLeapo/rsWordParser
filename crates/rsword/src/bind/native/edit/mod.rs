//! `BIND-03 v3` 编辑操作线型与上下文化转换（任务 8.3）。
//!
//! 协议数据路径是 JSON → 引擎 apply。反向转换只服务往返测试、调试与 M9′ 审计，
//! 不是协议数据路径；不能把它当作任意引擎对象的无损出口。b 类属性不能无损表示时
//! 返回具名错误，绝不静默丢弃；b 类按 v3 正向往返与成文拒绝集验收。
//! 引擎 EditOp / NewElement 不 derive serde；字段表以完整解构覆盖间接包含闭包。

mod leaves;

mod codec;
pub use codec::EditJsonError;
mod payload;
pub use payload::{NewAtomJson, NewBlockJson, NewFieldJson, NewInlineJson, NewRunJson};
mod ops;
mod result;
pub use ops::EditOpJson;
#[cfg(test)]
mod fixtures;

use crate::edit::EditOp;
use crate::xml::Dom;
use codec::DecodeCx;

/// `BIND-03` 调试 / 审计出口；不可无损表示的引擎值明确拒绝。
pub fn edit_op_to_json(op: &EditOp, dom: &Dom) -> codec::Result<String> {
    serde_json::to_string(&EditOpJson::from_engine(op, dom)?)
        .map_err(|e| EditJsonError::BadArgument(e.to_string()))
}

/// `BIND-03` 正向转换；失败不改变目标 Dom（包括 interner）。
pub fn edit_op_from_json(json: &str, dom: &mut Dom) -> codec::Result<EditOp> {
    decode_with_escapes(json, dom).map(|(op, _)| op)
}

pub(crate) fn decode_with_escapes(
    json: &str,
    dom: &mut Dom,
) -> codec::Result<(EditOp, Vec<String>)> {
    let wire: EditOpJson =
        serde_json::from_str(json).map_err(|e| EditJsonError::BadArgument(e.to_string()))?;
    let mut scratch = dom.clone();
    let mut cx = DecodeCx { dom: &mut scratch, escapes: Vec::new() };
    let op = wire.to_engine(&mut cx)?;
    let escapes = cx.escapes;
    *dom = scratch;
    Ok((op, escapes))
}

/// `BIND-03` 协议 apply；在线型转换前克隆规范状态，失败不提交驻留名或诊断。
/// 会话表及克隆成本预算随 8.4 验收。
pub fn apply_edit_json(
    session: &mut crate::edit::EditSession,
    json: &str,
    context: &crate::edit::EditContext,
) -> crate::error::Result<crate::edit::MutationResult> {
    use crate::diag::{DiagCode, Diagnostic};
    use crate::error::Error;
    let bad = |e: EditJsonError| Error::edit(DiagCode::BindBadArgument, e.to_string());
    let wire: EditOpJson =
        serde_json::from_str(json).map_err(|e| bad(EditJsonError::BadArgument(e.to_string())))?;
    let mut candidate = session.clone();
    let part = if let EditOpJson::SetHeaderFooter { sect, kind, variant, .. } = &wire {
        // 目标可能尚不存在。仅在候选会话中准备它，让表外名字驻留到真正承载内容的 DOM。
        if sect.0 as usize >= candidate.dom().node_count()
            || candidate.dom().element(*sect).is_none()
            || !candidate.dom().is(*sect, crate::xml::QName::w(crate::xml::LocalName::SectPr))
        {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                "not a section properties element",
            ));
        }
        let part =
            crate::edit::section_ops::ensure_hf_part(&mut candidate, *sect, *kind, *variant)?;
        candidate.rebuild()?;
        part
    } else {
        wire.context_part().unwrap_or(candidate.main_part())
    };
    if part.0 as usize >= candidate.package().parts().len() {
        return Err(Error::edit(DiagCode::BindBadArgument, "unknown part"));
    }
    // 二进制 part 的替换不需要该 part 的 DOM；上下文取主 part。
    let context_part = if matches!(
        wire,
        EditOpJson::ReplacePartBytes { .. } | EditOpJson::ReplacePartXml { .. }
    ) {
        candidate.main_part()
    } else {
        part
    };
    let dom = candidate
        .package_mut()
        .dom_mut(context_part)?
        .ok_or_else(|| Error::edit(DiagCode::BindBadArgument, "part has no XML DOM"))?;
    let mut cx = DecodeCx { dom, escapes: vec![] };
    let op = wire.to_engine(&mut cx).map_err(bad)?;
    let escapes = cx.escapes;
    let mut result = candidate.apply(op, context)?;
    let diagnostics: Vec<_> = escapes
        .into_iter()
        .map(|usage| Diagnostic::pre_existing(part, None, DiagCode::BindXmlEscape, usage))
        .collect();
    result.diagnostics.extend(diagnostics.iter().cloned());
    candidate.record(diagnostics);
    *session = candidate;
    Ok(result)
}

/// `BIND-03` 会话级逃生口计数；从已成功提交的诊断计算，失败请求不会增加。
pub fn xml_escape_count(session: &crate::edit::EditSession) -> usize {
    session.diagnostics().iter().filter(|d| d.code == crate::diag::DiagCode::BindXmlEscape).count()
}

/// `BIND-03/07` 会话诊断连同逃生口累计次数；8.4 的 diagnostics 导出复用此投影。
pub fn edit_diagnostics_json(session: &crate::edit::EditSession) -> serde_json::Value {
    use crate::bind::native::{ProjCx, ToJson};
    let cx = ProjCx { pkg: session.package(), display: false };
    serde_json::json!({
        "diagnostics": session.diagnostics().to_json(&cx),
        "xmlEscapeCount": xml_escape_count(session),
    })
}
