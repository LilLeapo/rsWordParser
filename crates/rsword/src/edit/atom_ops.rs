//! `EDIT-03 InsertAtom`：往坐标流里插一个**原子**（`spec/18` 7.5）。
//!
//! 原子在坐标流里恒占 **1 个 UTF-16 单位**（`EDIT-02`），所以插入前后的偏移差正好是 1。
//! 五种原子共用 [`super::ops`] 的边界定位（拆 run → `boundary_site`）与追踪时的
//! `w:ins` 包裹（`plan_ins_site`），差别只在 run 里放什么。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::BreakKind;
use crate::xml::{LocalName, NewElement, NodeEdit, NsId, QName, Target};

use super::plan::{MutationPlan, MutationResult};
use super::pos::{InlinePos, Loc, locate};
use super::session::EditSession;
use super::track::Tracker;
use super::{EditContext, NewAtom, NewMath};

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// `EDIT-06`：注释条目的下一个 `w:id`（0 / -1 是 separator 一族的保留号）。
fn next_note_id(s: &EditSession, endnote: bool) -> i64 {
    let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
    notes.items.iter().filter_map(|n| n.id.trim().parse::<i64>().ok()).max().unwrap_or(0).max(0) + 1
}

pub(crate) fn insert_atom(
    s: &mut EditSession,
    at: InlinePos,
    atom: &NewAtom,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.part_or_main(at.part);
    let mut result = MutationResult::default();
    // 注释条目先建（它在另一个 part 里，`w:id` 要先定下来才能发引用 run）
    let note_ref = match atom {
        NewAtom::NoteRef { endnote, content } => {
            if at.part.is_some() {
                return Err(super::ops::unsupported("注释引用只能插在主 part"));
            }
            let id = next_note_id(s, *endnote).to_string();
            result.absorb(super::ops::upsert_note_entry(s, *endnote, &id, content)?);
            Some((*endnote, id))
        }
        _ => None,
    };
    // 图片要先分配媒体关系（`&mut` 的活儿都在建计划之前做完）
    let image_run = match atom {
        NewAtom::Image(img) => Some(super::media_ops::image_run(s, img)?),
        _ => None,
    };
    let math_run = match atom {
        NewAtom::Math(m) => Some(math_element(s, m)?),
        _ => None,
    };

    let tb = super::ops::text_block(s, at.part, at.para)?;
    let loc = locate(tb, at.offset)?;
    let (parent, before, inherit) = match super::ops::split_at_public(s, at, loc, &mut result)? {
        Some((left, right)) => {
            (s.dom_in(at.part)?.parent(left).expect("run has a parent"), Some(right), Some(left))
        }
        None => {
            let Loc::Boundary { index } = loc else { unreachable!("split_at handles the rest") };
            super::ops::boundary_site_public(s, at.part, at.para, index)?
        }
    };
    let dom = s.dom_in(at.part)?;
    let mut plan = MutationPlan::new(part);
    plan.touch(at.para);
    let (run_parent, run_before) = match &mut Tracker::new(s.document(), ctx) {
        None => (Target::Node(parent), before),
        Some(t) => super::ops::plan_ins_site_public(&mut plan, dom, t, at.para, parent, before)?,
    };
    // `m:oMath` 不是 run：它自己就是段落的内容项
    if let Some(math) = math_run {
        plan.node_edits.push(NodeEdit::Insert {
            parent: run_parent,
            before: run_before,
            node: math,
        });
        plan.offset_delta.push((at.para, at.offset, 1));
        result.absorb(s.commit_plan(plan)?);
        return Ok(result);
    }
    let k = plan.node_edits.len();
    match image_run {
        // 图片的 run 是整份生成好的（含 `w:drawing` 与命名空间声明）
        Some(run) => plan.node_edits.push(NodeEdit::Insert {
            parent: run_parent,
            before: run_before,
            node: run,
        }),
        None => {
            plan.node_edits.push(NodeEdit::Insert {
                parent: run_parent,
                before: run_before,
                node: NewElement::new(w(LocalName::R)),
            });
            // 继承左邻 run 的 `w:rPr`（与 `InsertText` 同一条）
            if let Some(rpr) = inherit.and_then(|r| super::ops::rpr_of(dom, r)) {
                plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::New(k),
                    before: None,
                    source: rpr,
                });
            }
            let child = match atom {
                NewAtom::Break { kind, clear } => break_element(*kind, clear.as_deref()),
                NewAtom::Symbol { font, code } => symbol_element(font, *code),
                NewAtom::NoteRef { .. } => {
                    let (endnote, id) = note_ref.as_ref().expect("built above");
                    note_ref_element(*endnote, id)
                }
                NewAtom::Image(_) | NewAtom::Math(_) => unreachable!("handled above"),
            };
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: child,
            });
        }
    }
    plan.offset_delta.push((at.para, at.offset, 1));
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
}

/// `w:br`：`type` 缺省是文字换行；`clear` 只对文字换行有意义。
fn break_element(kind: BreakKind, clear: Option<&str>) -> NewElement {
    let mut e = NewElement::new(w(LocalName::Br));
    match kind {
        BreakKind::Page => e.push_attr(w(LocalName::Type), "page"),
        BreakKind::Column => e.push_attr(w(LocalName::Type), "column"),
        BreakKind::TextWrapping => {
            if let Some(c) = clear {
                e.push_attr(w(LocalName::Clear), c.to_string());
            }
        }
    }
    e
}

/// `w:sym`：`w:char` 是四位大写十六进制（`RES-05` 的反向；PUA 码位写低四位）。
fn symbol_element(font: &str, code: u32) -> NewElement {
    let mut e = NewElement::new(w(LocalName::Sym));
    e.push_attr(w(LocalName::Font), font.to_string());
    e.push_attr(w(LocalName::Char), format!("{:04X}", code & 0xFFFF));
    e
}

fn note_ref_element(endnote: bool, id: &str) -> NewElement {
    let local = if endnote { LocalName::EndnoteReference } else { LocalName::FootnoteReference };
    NewElement::new(w(local)).with_attr(w(LocalName::Id), id.to_string())
}

/// `m:oMath`：OMML 直接解析，LaTeX 先转 OMML（7.5b 的 `latex_to_omml`）。
fn math_element(s: &mut EditSession, m: &NewMath) -> Result<NewElement> {
    let omml = match m {
        NewMath::Omml(x) => x.clone(),
        NewMath::Latex(tex) => crate::model::omml::latex_to_omml(tex)?,
    };
    let main = s.part_or_main(None);
    let dom = s
        .package_mut()
        .dom_mut(main)?
        .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
    let xml = if omml.trim_start().starts_with("<m:oMath") {
        omml
    } else {
        format!(r#"<m:oMath xmlns:m="{}">{omml}</m:oMath>"#, crate::model::omml::NS_M)
    };
    let mut frags = crate::xml::parse_fragment(dom, &xml)
        .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("OMML 解析失败: {e}")))?;
    frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "OMML 片段为空"))
}
