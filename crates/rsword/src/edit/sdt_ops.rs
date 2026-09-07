//! 内容控件的内容操作（`EDIT-03`，`spec/18` 7.5）：`SetSdtContent` / `RemoveSdtShell`。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::{SdtInfo, SdtRefusal};
use crate::xml::{Dirty, LocalName, NodeEdit, NodeId, NsId, QName, Target};

use super::inline::NewInline;
use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;
use super::{EditContext, ops};

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// 这个 `w:sdt` 自己拒不拒绝内容编辑（`MOD-08` 的四态锁与数据绑定）。
fn guard(s: &EditSession, sdt: NodeId) -> Result<NodeId> {
    let dom = s.dom();
    if (sdt.0 as usize) >= dom.node_count()
        || dom.node(sdt).dirty == Dirty::Deleted
        || !dom.is(sdt, w(LocalName::Sdt))
    {
        return Err(Error::edit(DiagCode::EditBadPosition, "目标不是活的 w:sdt"));
    }
    let info = SdtInfo::read(dom, sdt);
    match info.refusal() {
        Some(SdtRefusal::Locked) => {
            Err(Error::edit(DiagCode::EditSdtLocked, "内容控件锁定了内容，拒绝编辑"))
        }
        Some(SdtRefusal::Bound) => {
            Err(Error::edit(DiagCode::EditSdtBound, "内容控件有数据绑定，第一阶段只读"))
        }
        None => dom
            .semantic_children(sdt)
            .find(|&c| dom.is(c, w(LocalName::SdtContent)))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "内容控件没有 w:sdtContent")),
    }
}

/// `SetSdtContent`：`w:sdtContent` 里的内联整体换掉。控件里装的是块（段落 / 表格）时拒绝
/// ——那要走 `ReplaceInlines` / `InsertBlock` 一族按块操作。
pub(crate) fn set_sdt_content(
    s: &mut EditSession,
    sdt: NodeId,
    inlines: &[NewInline],
    ctx: &EditContext,
) -> Result<MutationResult> {
    let content = guard(s, sdt)?;
    let dom = s.dom();
    if dom
        .semantic_children(content)
        .any(|c| dom.is(c, w(LocalName::P)) || dom.is(c, w(LocalName::Tbl)))
    {
        return Err(ops::unsupported(
            "这个内容控件装的是块级内容；请对里面的段落用 ReplaceInlines",
        ));
    }
    let para = dom
        .ancestors(content)
        .find(|&a| dom.is(a, w(LocalName::P)))
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "内联控件不在段落里"))?;
    ops::replace_container_inlines(s, para, content, inlines, ctx)
}

/// `RemoveSdtShell`：Word 的「删除内容控件」——内容搬到父节点，`w:sdt` 本身消失。
pub(crate) fn remove_sdt_shell(s: &mut EditSession, sdt: NodeId) -> Result<MutationResult> {
    let content = guard(s, sdt)?;
    let dom = s.dom();
    let parent = dom
        .parent(sdt)
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "w:sdt 没有父节点"))?;
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    if let Some(p) = dom.ancestors(sdt).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    for c in dom.children(content).iter().copied() {
        if dom.node(c).dirty == Dirty::Deleted || dom.element(c).is_none() {
            continue;
        }
        plan.node_edits.push(NodeEdit::Move {
            node: c,
            parent: Target::Node(parent),
            before: Some(sdt),
        });
    }
    plan.node_edits.push(NodeEdit::Delete(sdt));
    s.commit_plan(plan)
}
