//! 注释条目的内容操作（`EDIT-03`，`spec/18` 7.5）：`SetNoteContent` / `RemoveNote`。
//!
//! 条目内容本身在注释 part 的内容流里，改文字用 `InlinePos { part: 注释 part }` 上的
//! `InsertText` / `DeleteRange` 就行（TS 的 `text-patch` 场景就是这么做的，格式与超链接
//! 原样保留）。这两个操作是**整体替换**与**整条删除**。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::xml::{Dirty, LocalName, NodeEdit, NodeId, NsId, QName};

use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;
use super::{NewRun, ops};

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// `SetNoteContent`：整条条目的正文段落换成 `content`（自引用标记 run 保留）。
pub(crate) fn set_note_content(
    s: &mut EditSession,
    endnote: bool,
    id: &str,
    content: &[Vec<NewRun>],
) -> Result<MutationResult> {
    let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
    if notes.get(id).is_none() {
        return Err(Error::edit(
            DiagCode::EditTargetMissing,
            format!("没有 id 为 {id:?} 的{}", if endnote { "尾注" } else { "脚注" }),
        ));
    }
    let paras: Vec<Vec<NewRun>> =
        if content.is_empty() { vec![vec![NewRun::text("")]] } else { content.to_vec() };
    ops::upsert_note_entry(s, endnote, id, &paras)
}

/// `RemoveNote`：删条目 + 删正文里的引用 run（run 里只剩引用时整 run 删，否则只删引用元素）。
pub(crate) fn remove_note(s: &mut EditSession, endnote: bool, id: &str) -> Result<MutationResult> {
    let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
    if notes.get(id).is_none() {
        return Err(Error::edit(
            DiagCode::EditTargetMissing,
            format!("没有 id 为 {id:?} 的{}", if endnote { "尾注" } else { "脚注" }),
        ));
    }
    let mut result = drop_references(s, endnote, id)?;
    result.absorb(ops::remove_note_entry(s, endnote, id)?);
    Ok(result)
}

/// 正文里指向这条注释的引用 run（`w:footnoteReference` / `w:endnoteReference`）。
pub(crate) fn drop_references(
    s: &mut EditSession,
    endnote: bool,
    id: &str,
) -> Result<MutationResult> {
    let refname = if endnote { LocalName::EndnoteReference } else { LocalName::FootnoteReference };
    let main = s.main_part();
    let dom = s.dom();
    let mut plan = MutationPlan::new(main);
    let mut stack = vec![dom.root()];
    while let Some(n) = stack.pop() {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        stack.extend(e.children.iter().rev());
        if !dom.is(n, w(refname)) || dom.attr_value(n, w(LocalName::Id)).as_deref() != Some(id) {
            continue;
        }
        let Some(run) = dom.parent(n).filter(|&r| dom.is(r, w(LocalName::R))) else {
            plan.node_edits.push(NodeEdit::Delete(n));
            continue;
        };
        // run 里除 `w:rPr` 之外只有这个引用 → 整 run 删
        let others = dom
            .children(run)
            .iter()
            .copied()
            .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
            .filter(|&c| c != n && !dom.is(c, w(LocalName::RPr)))
            .count();
        let victim: NodeId = if others == 0 { run } else { n };
        plan.node_edits.push(NodeEdit::Delete(victim));
        if let Some(p) = dom.ancestors(victim).find(|&a| dom.is(a, w(LocalName::P))) {
            plan.touch(p);
        }
    }
    if plan.is_empty() {
        return Ok(MutationResult::default());
    }
    s.commit_plan(plan)
}
