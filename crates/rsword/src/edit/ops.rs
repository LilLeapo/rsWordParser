//! `EDIT-03` 操作实现（M1 子集）。每个操作是一个或多个 plan/commit 阶段；事务边界在
//! [`EditSession::apply`]（失败整体回滚）。这里的函数只读 DOM 与投影、产出 [`MutationPlan`]，
//! 写入全部经 [`EditSession::commit_plan`]。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::block::TextBlock;
use crate::model::inline::{Inline, Run, Segment, SegmentKind, utf16_len};
use crate::model::{SdtRefusal, refusing_sdt};
use crate::package::RelType;
use crate::semantic::props::{
    CellPropsPatch, ParaPropsPatch, RowPropsPatch, RunPropsPatch, TablePropsPatch, emit_run_props,
    plan_apply_para_props, plan_apply_run_props,
};
use crate::span::{
    Affinity, Anchor, FieldId, FlowId, RangeClass, RangeKind, RangeSpan, SpanId, SpanOrigin,
    is_property_element,
};
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NodeEdit, NodeId, NodeKind, NsId, QName, Target,
};

use super::inline::{Emitter, has_control_chars, sanitize_text, text_segments};
use super::plan::{MutationPlan, MutationResult};
use super::pos::{InlinePos, Loc, inline_spans, locate, utf16_to_byte};
use super::{BlockPos, EditContext, EditOp, EditSession, LinkRef, NewBlock, NewInline, NewRun};

pub(crate) fn run(s: &mut EditSession, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
    guard_sdt(s, &op)?;
    match op {
        EditOp::InsertText { at, text, props } => insert_text(s, at, &text, props, ctx),
        EditOp::DeleteRange { from, to } => delete_range(s, from, to, ctx),
        EditOp::SetRunProps { from, to, patch } => set_run_props(s, from, to, &patch),
        EditOp::ReplaceInlines { para, inlines } => replace_inlines(s, para, &inlines),
        EditOp::SetParaProps { para, patch } => set_para_props(s, para, &patch),
        EditOp::ReplaceParaProps { para, props } => replace_para_props(s, para, props),
        EditOp::SetTableProps { table, patch } => set_table_props(s, table, &patch),
        EditOp::SetRowProps { row, patch } => set_row_props(s, row, &patch),
        EditOp::SetCellProps { cell, patch } => set_cell_props(s, cell, &patch),
        EditOp::InsertBlock { at, block } => insert_block(s, at, block),
        EditOp::DeleteBlock { node } => delete_block(s, node),
        EditOp::MoveBlock { node, to } => move_block(s, node, to),
        EditOp::AddComment { from, to, comment } => add_comment(s, from, to, &comment),
        EditOp::RemoveComment { id } => remove_comment(s, &id),
        EditOp::SetCommentText { id, text, done } => set_comment_text(s, &id, &text, done),
        EditOp::SplitParagraph { at } => split_paragraph(s, at),
        EditOp::MergeWithNext { para } => merge_with_next(s, para),
        EditOp::AddBookmark { name, from, to } => add_bookmark(s, &name, from, to),
        EditOp::RemoveBookmark { name } => remove_bookmark(s, &name),
        EditOp::InsertField { at, field } => insert_field(s, at, &field, ctx),
        EditOp::SetLinkTarget { link, target } => set_link_target(s, link, &target),
        EditOp::ToggleCheckbox { field } => toggle_checkbox(s, field),
        EditOp::SetFormText { field, text } => set_form_text(s, field, &text),
        EditOp::SetFieldResultProps { field, patch } => set_field_result_props(s, field, &patch),
        EditOp::UpdateBlockField { field, blocks } => update_block_field(s, field, blocks, ctx),
    }
}

/// `EDIT-03` / `MOD-08`：编辑目标落在只读（`contentLocked` / `sdtContentLocked`）或数据绑定的内容
/// 控件里 → 整体拒绝，状态不变（`EDIT-05`）。第一阶段绑定控件一律只读：显示文字只是 customXml 的
/// 缓存，改了 Word 重开会刷回去。
///
/// 只看主 part：位置类操作都在正文（批注 / 注释条目按 id 定位，条目里不会有内容控件）。
fn guard_sdt(s: &EditSession, op: &EditOp) -> Result<()> {
    let dom = s.dom();
    let pos = |p: &InlinePos| p.para;
    let block_pos = |p: &BlockPos| match p {
        BlockPos::Start(c) | BlockPos::End(c) => *c,
        BlockPos::After(n) | BlockPos::Before(n) => *n,
    };
    let field = |id: FieldId| s.document().fields.get(id).map(|f| f.form.head());
    let targets: Vec<NodeId> = match op {
        EditOp::InsertText { at, .. }
        | EditOp::SplitParagraph { at }
        | EditOp::InsertField { at, .. } => vec![pos(at)],
        EditOp::DeleteRange { from, to }
        | EditOp::SetRunProps { from, to, .. }
        | EditOp::AddComment { from, to, .. }
        | EditOp::AddBookmark { from, to, .. } => vec![pos(from), pos(to)],
        EditOp::ReplaceInlines { para, .. }
        | EditOp::SetParaProps { para, .. }
        | EditOp::ReplaceParaProps { para, .. }
        | EditOp::MergeWithNext { para } => vec![*para],
        EditOp::SetTableProps { table: n, .. }
        | EditOp::SetRowProps { row: n, .. }
        | EditOp::SetCellProps { cell: n, .. } => vec![*n],
        EditOp::InsertBlock { at, .. } => vec![block_pos(at)],
        EditOp::DeleteBlock { node } => vec![*node],
        EditOp::MoveBlock { node, to } => vec![*node, block_pos(to)],
        EditOp::SetLinkTarget { link, .. } => match link {
            LinkRef::Field(id) => field(*id).into_iter().collect(),
            LinkRef::Element(node) => vec![*node],
        },
        EditOp::ToggleCheckbox { field: id }
        | EditOp::SetFormText { field: id, .. }
        | EditOp::SetFieldResultProps { field: id, .. }
        | EditOp::UpdateBlockField { field: id, .. } => field(*id).into_iter().collect(),
        // 按 id 定位的操作（批注条目、书签名）不在正文树上。这里**不写通配分支**：
        // 新增操作时编译器会提醒你决定它要不要守卫。
        EditOp::RemoveComment { .. }
        | EditOp::SetCommentText { .. }
        | EditOp::RemoveBookmark { .. } => Vec::new(),
    };
    for node in targets {
        let Some((info, why)) = refusing_sdt(dom, node) else { continue };
        let what = info
            .alias
            .clone()
            .or_else(|| info.tag.clone())
            .unwrap_or_else(|| info.control.as_str().to_string());
        return Err(match why {
            SdtRefusal::Locked => Error::edit(
                DiagCode::EditSdtLocked,
                format!("内容控件 `{what}` 的 w:lock 是 {}，内容只读", info.lock),
            ),
            SdtRefusal::Bound => Error::edit(
                DiagCode::EditSdtBound,
                format!("内容控件 `{what}` 绑定了 customXml 数据，第一阶段不可编辑"),
            ),
        });
    }
    Ok(())
}

// ---- 小工具 ----------------------------------------------------------------------------------

fn w(local: LocalName) -> QName {
    QName::w(local)
}

fn unsupported(msg: &str) -> Error {
    Error::edit(DiagCode::EditUnsupported, msg)
}

fn text_block(s: &EditSession, para: NodeId) -> Result<&TextBlock> {
    s.text_block(para).ok_or_else(|| {
        Error::edit(DiagCode::EditBadPosition, format!("节点 {} 不是正文文本段落", para.0))
    })
}

fn live_children(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    dom.children(n).iter().copied().filter(move |&c| dom.node(c).dirty != Dirty::Deleted)
}

fn child_named(dom: &Dom, parent: NodeId, name: QName) -> Option<NodeId> {
    live_children(dom, parent).find(|&c| dom.is(c, name))
}

pub(crate) fn rpr_of(dom: &Dom, run: NodeId) -> Option<NodeId> {
    child_named(dom, run, w(LocalName::RPr))
}

pub(crate) fn ppr_of(dom: &Dom, para: NodeId) -> Option<NodeId> {
    child_named(dom, para, w(LocalName::PPr))
}

/// 下一个未删除的兄弟。
fn next_sibling(dom: &Dom, n: NodeId) -> Option<NodeId> {
    let p = dom.parent(n)?;
    let kids = dom.children(p);
    let i = kids.iter().position(|&c| c == n)?;
    kids[i + 1..].iter().copied().find(|&c| dom.node(c).dirty != Dirty::Deleted)
}

/// 下一个未删除的**元素**兄弟（跳过缩排产生的空白文本节点）。
fn next_element_sibling(dom: &Dom, n: NodeId) -> Option<NodeId> {
    let p = dom.parent(n)?;
    let kids = dom.children(p);
    let i = kids.iter().position(|&c| c == n)?;
    kids[i + 1..]
        .iter()
        .copied()
        .find(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
}

/// `w:t` 的唯一文本子节点；其他形态（空元素、多个子节点）返回 `None`，调用方整体替换。
fn sole_text_child(dom: &Dom, t: NodeId) -> Option<NodeId> {
    let mut it = live_children(dom, t);
    let first = it.next()?;
    if it.next().is_some() {
        return None;
    }
    matches!(dom.node(first).kind, NodeKind::Text(_)).then_some(first)
}

fn clone_attrs(dom: &Dom, from: NodeId, to: &mut NewElement) {
    if let Some(e) = dom.element(from) {
        for a in &e.attrs {
            to.push_attr(a.name, dom.attr_str(a).into_owned());
        }
    }
}

/// 段文本设为 `text`：有唯一文本子节点 → `SetText`（`w:t` 由序列化补 preserve）；否则替换整个元素。
fn set_segment_text(dom: &Dom, seg_node: NodeId, text: &str, plan: &mut MutationPlan) {
    match sole_text_child(dom, seg_node) {
        Some(tn) => plan.node_edits.push(NodeEdit::SetText { node: tn, text: text.to_string() }),
        None => {
            let name = dom.name(seg_node).expect("segment is an element");
            let mut e = NewElement::new(name);
            clone_attrs(dom, seg_node, &mut e);
            plan.node_edits.push(NodeEdit::Replace { old: seg_node, node: e.with_text(text) });
        }
    }
}

/// 字段 / 批注 / 脚注结构段：删除范围覆盖时原地保留（M2 由 `FieldSpan` 接管）。
/// 字段结构段（`fldChar` / 指令文本）：`is_structural` 的子集。
fn is_field_structure(kind: &SegmentKind) -> bool {
    matches!(kind, SegmentKind::FldChar | SegmentKind::InstrText | SegmentKind::DelInstrText)
}

fn is_structural(kind: &SegmentKind) -> bool {
    matches!(
        kind,
        SegmentKind::FldChar
            | SegmentKind::InstrText
            | SegmentKind::DelInstrText
            | SegmentKind::CommentRef
            | SegmentKind::AnnotationRef
            | SegmentKind::FootnoteRefMark
            | SegmentKind::EndnoteRefMark
            | SegmentKind::Separator
            | SegmentKind::ContinuationSeparator
    )
}

fn in_deleted_run(run: &Run) -> bool {
    run.rev.as_ref().is_some_and(|r| r.del.is_some())
}

/// 把另一份计划的编辑追加进来：`Target::New(k)` 按偏移重定位。
fn append_edits(plan: &mut MutationPlan, mut edits: Vec<NodeEdit>) {
    let base = plan.node_edits.len();
    let shift = |t: &mut Target| {
        if let Target::New(k) = t {
            *k += base;
        }
    };
    for e in &mut edits {
        match e {
            NodeEdit::Insert { parent, .. }
            | NodeEdit::InsertClone { parent, .. }
            | NodeEdit::Move { parent, .. } => shift(parent),
            NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => shift(node),
            NodeEdit::Replace { .. }
            | NodeEdit::ReplaceClone { .. }
            | NodeEdit::Delete(_)
            | NodeEdit::SetText { .. } => {}
        }
    }
    plan.node_edits.extend(edits);
}

/// `EDIT-06`：文档内全部修订 `w:id` 的最大值 + 1（M1 只看主 part）。
pub(crate) fn next_revision_id(dom: &Dom) -> u32 {
    let mut max = 0u32;
    for n in dom.descendants(dom.root()) {
        let node = dom.node(n);
        if node.dirty == Dirty::Deleted {
            continue;
        }
        let NodeKind::Element(e) = &node.kind else { continue };
        if e.name.ns != NsId::W
            || !matches!(
                e.name.local,
                LocalName::Ins
                    | LocalName::Del
                    | LocalName::MoveFrom
                    | LocalName::MoveTo
                    | LocalName::RPrChange
                    | LocalName::PPrChange
                    | LocalName::SectPrChange
                    | LocalName::TblPrChange
                    | LocalName::TblGridChange
                    | LocalName::TcPrChange
                    | LocalName::TblPrExChange
                    | LocalName::NumberingChange
                    | LocalName::CellIns
                    | LocalName::CellDel
                    | LocalName::CellMerge
                    | LocalName::MoveFromRangeStart
                    | LocalName::MoveToRangeStart
            )
        {
            continue;
        }
        if let Some(v) = dom.attr_value(n, w(LocalName::Id))
            && let Ok(id) = v.trim().parse::<u32>()
        {
            max = max.max(id);
        }
    }
    max + 1
}

// ---- 拆分 run ---------------------------------------------------------------------------------

/// 在 `run.segments[seg]` 的 `byte` 处拆分：左半留在原 run（文本 `Owned` 截断），右半为 `New` run，
/// `rPr` 为字节克隆（`XML-12` 规则 F），其后的段整段克隆过去、原节点 `Deleted`。
/// `byte == 0` → 段 `seg` 整段归右；`byte ≥ len` → 归左。新 run 是 `created[0]`。
fn split_run(s: &EditSession, para: NodeId, run: &Run, seg: usize, byte: usize) -> MutationPlan {
    let dom = s.dom();
    let mut plan = MutationPlan::new(s.main_part());
    plan.touch(para);
    // 后半是原 run 的延续：该边界上的 `Left` 锚点也要右移（`SPAN-06` 的补充，见 `SpanPolicy`）
    plan.span.split_items.push(run.node);
    let parent = dom.parent(run.node).expect("run has a parent");
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(parent),
        before: next_sibling(dom, run.node),
        node: NewElement::new(w(LocalName::R)),
    });
    if let Some(rpr) = rpr_of(dom, run.node) {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: rpr,
        });
    }
    let segs = &run.segments;
    let text = run.segment_text(&segs[seg]);
    let move_from = if byte == 0 {
        seg
    } else if byte >= text.len() {
        seg + 1
    } else {
        set_segment_text(dom, segs[seg].node, &text[..byte], &mut plan);
        let name = dom.name(segs[seg].node).expect("segment is an element");
        let mut e = NewElement::new(name);
        clone_attrs(dom, segs[seg].node, &mut e);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(k),
            before: None,
            node: e.with_text(&text[byte..]),
        });
        seg + 1
    };
    for sg in &segs[move_from.min(segs.len())..] {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: sg.node,
        });
        plan.node_edits.push(NodeEdit::Delete(sg.node));
    }
    plan
}

/// 位置若在某个 run 内部（段间或文本段内），先拆分；返回 `(左 run, 右 run)`。边界位置返回 `None`。
fn split_at(
    s: &mut EditSession,
    para: NodeId,
    loc: Loc,
    result: &mut MutationResult,
) -> Result<Option<(NodeId, NodeId)>> {
    let (inline, segment, byte) = match loc {
        Loc::Boundary { .. } => return Ok(None),
        Loc::InRun { inline, segment } => (inline, segment, 0),
        Loc::InText { inline, segment, byte } => (inline, segment, byte),
    };
    let tb = text_block(s, para)?;
    let Inline::Run(run) = &tb.inlines[inline] else { unreachable!("InRun/InText point at runs") };
    if in_deleted_run(run) || run.segments[segment].kind == SegmentKind::DelText {
        return Err(unsupported("位置在已删除文本内（修订编辑在 M7）"));
    }
    let run_node = run.node;
    let plan = split_run(s, para, run, segment, byte);
    let r = s.commit_plan(plan)?;
    let right = r.created[0].expect("split creates the right run");
    result.absorb(r);
    Ok(Some((run_node, right)))
}

// ---- InsertText -------------------------------------------------------------------------------

/// `props == None` 时可直接写入的 `Text` 段：`(段节点, 段文本, 字节偏移)`。
fn direct_text_target<'a>(tb: &'a TextBlock, loc: &Loc) -> Option<(NodeId, &'a str, usize)> {
    let text_seg = |run: &'a Run, seg: &'a Segment| -> Option<(NodeId, &'a str)> {
        (seg.kind == SegmentKind::Text && !in_deleted_run(run))
            .then(|| (seg.node, run.segment_text(seg)))
    };
    let run_at = |i: usize| match &tb.inlines[i] {
        Inline::Run(r) => Some(r),
        _ => None,
    };
    match *loc {
        Loc::InText { inline, segment, byte } => {
            let run = run_at(inline)?;
            text_seg(run, &run.segments[segment]).map(|(n, t)| (n, t, byte))
        }
        Loc::InRun { inline, segment } => {
            let run = run_at(inline)?;
            if let Some((n, t)) = text_seg(run, &run.segments[segment - 1]) {
                return Some((n, t, t.len()));
            }
            text_seg(run, &run.segments[segment]).map(|(n, t)| (n, t, 0))
        }
        Loc::Boundary { index } => {
            if index > 0
                && let Some(run) = run_at(index - 1)
                && let Some(seg) = run.segments.last()
                && let Some((n, t)) = text_seg(run, seg)
            {
                return Some((n, t, t.len()));
            }
            if index < tb.inlines.len()
                && let Some(run) = run_at(index)
                && let Some(seg) = run.segments.first()
                && let Some((n, t)) = text_seg(run, seg)
            {
                return Some((n, t, 0));
            }
            None
        }
    }
}

/// `n` 的祖先或自身中直接挂在 `para` 下的那个。
fn top_child(dom: &Dom, para: NodeId, n: NodeId) -> NodeId {
    let mut x = n;
    while dom.parent(x).is_some_and(|p| p != para) {
        x = dom.parent(x).expect("checked");
    }
    x
}

/// 左右两个 inline 节点的最深公共容器与插入锚点（`before` = 含右节点的那个子节点）。
fn common_site(dom: &Dom, para: NodeId, left: NodeId, right: NodeId) -> (NodeId, Option<NodeId>) {
    let anc_left: Vec<NodeId> =
        std::iter::successors(Some(left), |&x| dom.parent(x).filter(|&p| p != para)).collect();
    let mut x = right;
    loop {
        let p = dom.parent(x).expect("inline is inside the paragraph");
        if p == para || anc_left.contains(&p) {
            return (p, Some(x));
        }
        x = p;
    }
}

/// 边界的哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// 边界一侧的 inline 对应的 DOM 节点。
///
/// 字段原子（`Inline::Field`）自己没有单一节点，取它靠着边界的那一端：左邻取 `tail`
/// （end run / `w:fldSimple`）、右邻取 `head`（begin run）。插入点因此落在原子**之外**，
/// 与 `SPAN-10`"端点落在字段原子内部时移到原子边界"是同一条道理。
fn boundary_node(s: &EditSession, tb: &TextBlock, i: &Inline, side: Side) -> Result<NodeId> {
    if let Some(n) = i.node() {
        return Ok(n);
    }
    let Inline::Field { id, .. } = i else {
        return Err(unsupported("inline 没有对应节点"));
    };
    let f =
        s.document().fields.get(*id).ok_or_else(|| unsupported("字段不在索引里（投影过期）"))?;
    let n = match side {
        Side::Left => f.form.tail(),
        Side::Right => f.form.head(),
    };
    // 跨段字段（`FLD-06` 的 `Block`）另一端在别的段落里，结果段落只读
    if !s.dom().ancestors(n).any(|a| a == tb.node) {
        return Err(unsupported("跨段字段的边界（Block 字段的结果段落只读）"));
    }
    Ok(n)
}

/// 边界插入点：`(parent, before, 继承格式的 run)`。
fn boundary_site(
    s: &EditSession,
    tb: &TextBlock,
    index: usize,
) -> Result<(NodeId, Option<NodeId>, Option<NodeId>)> {
    let dom = s.dom();
    let para = tb.node;
    let left = (index > 0)
        .then(|| boundary_node(s, tb, &tb.inlines[index - 1], Side::Left))
        .transpose()?;
    let right = (index < tb.inlines.len())
        .then(|| boundary_node(s, tb, &tb.inlines[index], Side::Right))
        .transpose()?;
    // 继承格式的 run：先看平铺的 run，再看字段结果里的 run（紧邻字段插字沿用结果的格式）
    let run_node = |i: &Inline| match i {
        Inline::Run(r) => Some(r.node),
        Inline::Field { result, .. } => result.iter().rev().find_map(|r| match r {
            Inline::Run(r) => Some(r.node),
            _ => None,
        }),
        Inline::Atom(_) => None,
    };
    let inherit = tb.inlines[..index]
        .iter()
        .rev()
        .find_map(run_node)
        .or_else(|| tb.inlines[index..].iter().find_map(run_node));
    let (parent, before) = match (left, right) {
        (Some(l), Some(r)) => common_site(dom, para, l, r),
        (Some(_), None) | (None, None) => (para, None),
        (None, Some(r)) => (para, Some(top_child(dom, para, r))),
    };
    Ok((parent, before, inherit))
}

fn insert_text(
    s: &mut EditSession,
    at: InlinePos,
    text: &str,
    props: Option<RunPropsPatch>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.main_part();
    let mut diags = Vec::new();
    let text = sanitize_text(text, part, &mut diags);
    if text.is_empty() {
        return Err(Error::edit(DiagCode::EditBadText, "插入文本为空"));
    }
    let delta = utf16_len(&text) as i32;
    let tb = text_block(s, at.para)?;
    let loc = locate(tb, at.offset)?;

    // 路径 1：紧邻 / 落在 Text 段 → 直接写该 w:t 的文本节点
    if props.is_none()
        && !has_control_chars(&text)
        && let Some((seg_node, seg_text, byte)) = direct_text_target(tb, &loc)
    {
        let new_text = format!("{}{}{}", &seg_text[..byte], text, &seg_text[byte..]);
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        plan.diagnostics = diags;
        set_segment_text(s.dom(), seg_node, &new_text, &mut plan);
        plan.offset_delta.push((at.para, at.offset, delta));
        return s.commit_plan(plan);
    }

    // 路径 2：边界插入 New run（继承左侧 rPr 或 default_run_props）
    let mut result = MutationResult::default();
    let (parent, before, inherit) = match split_at(s, at.para, loc, &mut result)? {
        Some((left, right)) => {
            (s.dom().parent(left).expect("run has a parent"), Some(right), Some(left))
        }
        None => {
            let Loc::Boundary { index } = loc else { unreachable!("split_at handles the rest") };
            boundary_site(s, text_block(s, at.para)?, index)?
        }
    };
    let dom = s.dom();
    let flavor = s.flavor();
    let mut plan = MutationPlan::new(part);
    plan.touch(at.para);
    plan.diagnostics = diags;
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(parent),
        before,
        node: NewElement::new(w(LocalName::R)),
    });
    match inherit.and_then(|r| rpr_of(dom, r)) {
        Some(rpr) => plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: rpr,
        }),
        None => {
            if let Some(d) = &ctx.default_run_props {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: emit_run_props(d, flavor),
                });
            }
        }
    }
    for seg in text_segments(&text, false) {
        plan.node_edits.push(NodeEdit::Insert { parent: Target::New(k), before: None, node: seg });
    }
    plan.offset_delta.push((at.para, at.offset, delta));
    let r = s.commit_plan(plan)?;
    let new_run = r.created[k].expect("insert creates the run");
    result.absorb(r);

    // 路径 2c：合并 props
    if let Some(patch) = props {
        let dom = s.dom();
        let edits = plan_apply_run_props(dom, new_run, rpr_of(dom, new_run), &patch, flavor);
        if !edits.is_empty() {
            let mut plan = MutationPlan::new(part);
            plan.touch(at.para);
            plan.node_edits = edits;
            result.absorb(s.commit_plan(plan)?);
        }
    }
    Ok(result)
}

// ---- DeleteRange ------------------------------------------------------------------------------

fn delete_range(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    ctx: &EditContext,
) -> Result<MutationResult> {
    if from.para != to.para {
        return Err(Error::edit(
            DiagCode::EditCrossParagraph,
            "DeleteRange 两端不在同一段落（跨段删除在 M2）",
        ));
    }
    let (a, b) = (from.offset.0, to.offset.0);
    if a > b {
        return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
    }
    let part = s.main_part();
    let tb = text_block(s, from.para)?;
    locate(tb, from.offset)?;
    locate(tb, to.offset)?;
    let mut plan = MutationPlan::new(part);
    plan.span.keep_orphan_comments = ctx.keep_orphan_comments;
    plan.touch(from.para);
    if a == b {
        return s.commit_plan(plan);
    }
    let dom = s.dom();
    let fields = &s.document().fields;
    let spans = inline_spans(tb);
    let mut kept_structure = 0usize;
    for (inline, span) in tb.inlines.iter().zip(&spans) {
        if span.end <= a || span.start >= b {
            continue;
        }
        match inline {
            Inline::Atom(atom) => plan.node_edits.push(NodeEdit::Delete(atom.node)),
            // `FLD-07`：原子形态字段被覆盖 → 整个字段（begin..end，含嵌套）一起删。
            // 原子只占 1 个坐标单位，区间与它相交就必然把它整个盖住。
            Inline::Field { id, .. } => {
                for n in fields.all_nodes(*id) {
                    plan.node_edits.push(NodeEdit::Delete(n));
                }
            }
            Inline::Run(run) => {
                let fully = span.start >= a && span.end <= b;
                let structural = run.segments.iter().any(|sg| is_structural(&sg.kind));
                if fully && !structural {
                    plan.node_edits.push(NodeEdit::Delete(run.node));
                    continue;
                }
                // 属于已识别字段的结构 run 原地保留是**正确**行为：透明字段（`Link`）的结果可以
                // 正常编辑，字段本身不该跟着消失。剩下的（畸形 / 未闭合字段的 fldChar）才是缺陷。
                if structural
                    && run.field.is_none()
                    && run.segments.iter().any(|sg| {
                        is_field_structure(&sg.kind) && fields.field_of(run.node).is_none()
                    })
                {
                    kept_structure += 1;
                }
                let mut ss = span.start;
                for seg in &run.segments {
                    let se = ss + seg.utf16_len;
                    let inside =
                        if seg.utf16_len == 0 { ss > a && ss < b } else { se > a && ss < b };
                    if inside {
                        match seg.kind {
                            SegmentKind::Text | SegmentKind::DelText => {
                                let c0 = a.max(ss) - ss;
                                let c1 = b.min(se) - ss;
                                if c0 == 0 && c1 == seg.utf16_len {
                                    plan.node_edits.push(NodeEdit::Delete(seg.node));
                                } else {
                                    let t = run.segment_text(seg);
                                    let b0 = utf16_to_byte(t, c0)?;
                                    let b1 = utf16_to_byte(t, c1)?;
                                    let nt = format!("{}{}", &t[..b0], &t[b1..]);
                                    set_segment_text(dom, seg.node, &nt, &mut plan);
                                }
                            }
                            _ if seg.utf16_len > 0 => {
                                plan.node_edits.push(NodeEdit::Delete(seg.node))
                            }
                            ref k if is_structural(k) => {}
                            _ => plan.node_edits.push(NodeEdit::Delete(seg.node)),
                        }
                    }
                    ss = se;
                }
            }
        }
    }
    // 范围标记不再原地"漏"着：删除内容项后标记物理上就落在删除点，正好是 `SPAN-06`
    // 把锚点算到的位置（`commit_plan` 统一变换）。剩下的只有字段结构，等 2.4 的 `FieldSpan`。
    if kept_structure > 0 {
        plan.diagnostics.push(Diagnostic::invariant_violation(
            part,
            None,
            DiagCode::EditAnchorUnmoved,
            format!("删除范围内有 {kept_structure} 个未闭合 / 畸形字段的结构 run 原地保留"),
        ));
    }
    plan.offset_delta.push((from.para, from.offset, -((b - a) as i32)));
    s.commit_plan(plan)
}

// ---- SetRunProps ------------------------------------------------------------------------------

fn set_run_props(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    patch: &RunPropsPatch,
) -> Result<MutationResult> {
    if from.para != to.para {
        return Err(Error::edit(DiagCode::EditCrossParagraph, "SetRunProps 两端不在同一段落"));
    }
    let (a, b) = (from.offset.0, to.offset.0);
    if a > b {
        return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
    }
    let part = s.main_part();
    let mut result = MutationResult::default();
    // 阶段 A/B：先在 to、再在 from 处拆分（拆分不改变坐标）
    for off in [to.offset, from.offset] {
        let loc = locate(text_block(s, from.para)?, off)?;
        split_at(s, from.para, loc, &mut result)?;
    }
    // 阶段 C：范围内的每个非零宽 run 按 PROP-06 计划 rPr 变更
    let tb = text_block(s, from.para)?;
    let spans = inline_spans(tb);
    let dom = s.dom();
    let flavor = s.flavor();
    let mut plan = MutationPlan::new(part);
    plan.touch(from.para);
    for (inline, span) in tb.inlines.iter().zip(&spans) {
        if span.start < a || span.end > b || span.start == span.end {
            continue;
        }
        if let Inline::Run(run) = inline {
            let edits = plan_apply_run_props(dom, run.node, rpr_of(dom, run.node), patch, flavor);
            append_edits(&mut plan, edits);
        }
    }
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
}

// ---- 段落属性与 compat 路径 ---------------------------------------------------------------------

fn require_paragraph(dom: &Dom, para: NodeId) -> Result<()> {
    if (para.0 as usize) < dom.node_count()
        && dom.node(para).dirty != Dirty::Deleted
        && dom.is(para, w(LocalName::P))
    {
        Ok(())
    } else {
        Err(Error::edit(DiagCode::EditBadPosition, format!("节点 {} 不是活的 w:p", para.0)))
    }
}

fn set_para_props(
    s: &mut EditSession,
    para: NodeId,
    patch: &ParaPropsPatch,
) -> Result<MutationResult> {
    let dom = s.dom();
    require_paragraph(dom, para)?;
    let mut plan = MutationPlan::new(s.main_part());
    plan.touch(para);
    plan.node_edits = plan_apply_para_props(dom, para, ppr_of(dom, para), patch, s.flavor());
    s.commit_plan(plan)
}

fn emit_inlines(dom: &Dom, inlines: &[NewInline]) -> Vec<NewElement> {
    let mut em = Emitter::new(next_revision_id(dom));
    let mut out = Vec::new();
    for i in inlines {
        em.emit(i, false, &mut out);
    }
    out
}

fn replace_inlines(
    s: &mut EditSession,
    para: NodeId,
    inlines: &[NewInline],
) -> Result<MutationResult> {
    let dom = s.dom();
    require_paragraph(dom, para)?;
    let mut plan = MutationPlan::new(s.main_part());
    plan.touch(para);
    // 内容（含范围标记）被外部描述整体重写：提交后按新标记重建这个容器的端点（`SPAN-06` rescan）
    plan.span.rescan.push(para);
    for c in live_children(dom, para) {
        if !dom.is(c, w(LocalName::PPr)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
    }
    for e in emit_inlines(dom, inlines) {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: None,
            node: e,
        });
    }
    s.commit_plan(plan)
}

fn replace_para_props(
    s: &mut EditSession,
    para: NodeId,
    props: Option<NewElement>,
) -> Result<MutationResult> {
    let dom = s.dom();
    require_paragraph(dom, para)?;
    let mut plan = MutationPlan::new(s.main_part());
    plan.touch(para);
    let first = live_children(dom, para).next();
    for c in live_children(dom, para) {
        if dom.is(c, w(LocalName::PPr)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
    }
    if let Some(p) = props {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: first,
            node: p,
        });
    }
    s.commit_plan(plan)
}

// ---- 块级 -------------------------------------------------------------------------------------

/// `BlockPos` → `(parent, before)`。`End(c)` 落在尾部 `w:sectPr` 之前。
fn block_site(dom: &Dom, at: BlockPos) -> Result<(NodeId, Option<NodeId>)> {
    let live_elem = |c: NodeId| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some();
    let ok = |n: NodeId| (n.0 as usize) < dom.node_count() && live_elem(n);
    match at {
        BlockPos::Start(c) => {
            if !ok(c) {
                return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
            }
            Ok((c, dom.children(c).iter().copied().find(|&k| live_elem(k))))
        }
        BlockPos::Before(n) | BlockPos::After(n) => {
            if !ok(n) {
                return Err(Error::edit(DiagCode::EditBadPosition, "锚点块无效"));
            }
            let parent = dom
                .parent(n)
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "锚点块没有父节点"))?;
            let before =
                if matches!(at, BlockPos::Before(_)) { Some(n) } else { next_sibling(dom, n) };
            Ok((parent, before))
        }
        BlockPos::End(c) => {
            if !ok(c) {
                return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
            }
            let last = dom.children(c).iter().copied().rev().find(|&k| live_elem(k));
            let before = last.filter(|&l| dom.is(l, w(LocalName::SectPr)));
            Ok((c, before))
        }
    }
}

fn new_block_element(dom: &Dom, block: NewBlock) -> NewElement {
    match block {
        NewBlock::Xml(e) => e,
        NewBlock::Paragraph { props, inlines } => {
            let mut p = NewElement::new(w(LocalName::P));
            if let Some(pp) = props {
                p.push_child(pp);
            }
            for e in emit_inlines(dom, &inlines) {
                p.push_child(e);
            }
            p
        }
        NewBlock::Wrapped { mut wrapper, block } => {
            // 块级 w:ins / w:del：空的 w:id 占位 → EDIT-06 分配
            if let Some(id) =
                wrapper.attrs.iter_mut().find(|(n, v)| *n == w(LocalName::Id) && v.is_empty())
            {
                id.1 = next_revision_id(dom).to_string();
            }
            let inner = new_block_element(dom, *block);
            wrapper.push_child(inner);
            wrapper
        }
    }
}

// ---- 表格属性（`EDIT-03`，任务 3.7）------------------------------------------------------------

/// `node` 所属的最内层 `w:tbl`（投影刷新的单位）。
fn owning_table(dom: &Dom, node: NodeId) -> Option<NodeId> {
    std::iter::once(node).chain(dom.ancestors(node)).find(|&n| dom.is(n, w(LocalName::Tbl)))
}

/// 属性容器与"缺失时插在谁之前"。`w:tblPr` / `w:tcPr` 是第一个子元素；`w:trPr` 在 `w:tblPrEx`
/// 之后、第一个 `w:tc` 之前（`PROP-05` 的 `w:tr` 子元素顺序）。
fn props_site(dom: &Dom, parent: NodeId, container: LocalName) -> (Option<NodeId>, Option<NodeId>) {
    let live = |c: NodeId| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some();
    let kids: Vec<NodeId> = dom.children(parent).iter().copied().filter(|&c| live(c)).collect();
    let existing = kids.iter().copied().find(|&c| dom.is(c, w(container)));
    let before = if container == LocalName::TrPr {
        kids.iter().copied().find(|&c| !dom.is(c, w(LocalName::TblPrEx)))
    } else {
        kids.first().copied()
    };
    (existing, before)
}

/// 三个表格属性操作同形：定位容器 → `plan_apply_*_at` → 标记所属表格刷新。
///
/// ```ignore
/// table_props_op!(set_cell_props, CellPropsPatch, Tc, TcPr, plan_apply_cell_props_at, "单元格");
/// ```
macro_rules! table_props_op {
    ($name:ident, $patch:ty, $owner:ident, $container:ident, $plan_apply:path, $what:literal) => {
        fn $name(s: &mut EditSession, node: NodeId, patch: &$patch) -> Result<MutationResult> {
            let dom = s.dom();
            if (node.0 as usize) >= dom.node_count()
                || dom.node(node).dirty == Dirty::Deleted
                || !dom.is(node, w(LocalName::$owner))
            {
                return Err(Error::edit(DiagCode::EditBadPosition, concat!("目标不是", $what)));
            }
            let (container, before) = props_site(dom, node, LocalName::$container);
            let mut plan = MutationPlan::new(s.main_part());
            $plan_apply(
                dom,
                Target::Node(node),
                container,
                before,
                patch,
                s.flavor(),
                &mut plan.node_edits,
            );
            if let Some(tbl) = owning_table(dom, node) {
                plan.touch(tbl);
            }
            s.commit_plan(plan)
        }
    };
}

table_props_op!(
    set_table_props,
    TablePropsPatch,
    Tbl,
    TblPr,
    crate::semantic::props::plan_apply_table_props_at,
    "表格"
);
table_props_op!(
    set_row_props,
    RowPropsPatch,
    Tr,
    TrPr,
    crate::semantic::props::plan_apply_row_props_at,
    "表格行"
);
table_props_op!(
    set_cell_props,
    CellPropsPatch,
    Tc,
    TcPr,
    crate::semantic::props::plan_apply_cell_props_at,
    "单元格"
);

fn insert_block(s: &mut EditSession, at: BlockPos, block: NewBlock) -> Result<MutationResult> {
    let dom = s.dom();
    let (parent, before) = block_site(dom, at)?;
    let is_para = matches!(block, NewBlock::Paragraph { .. });
    let node = new_block_element(dom, block);
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(parent), before, node });
    // 插在格尾的非段落块（表格等）后面要补一个空段落
    if before.is_none() && !is_para {
        keep_cell_paragraph(dom, parent, None, &mut plan);
    }
    s.commit_plan(plan)
}

fn delete_block(s: &mut EditSession, node: NodeId) -> Result<MutationResult> {
    let dom = s.dom();
    if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
        return Err(Error::edit(DiagCode::EditBadPosition, "块不存在或已删除"));
    }
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Delete(node));
    if let Some(parent) = dom.parent(node) {
        keep_cell_paragraph(dom, parent, Some(node), &mut plan);
    }
    s.commit_plan(plan)
}

fn move_block(s: &mut EditSession, node: NodeId, to: BlockPos) -> Result<MutationResult> {
    let dom = s.dom();
    let (parent, before) = block_site(dom, to)?;
    let mut plan = MutationPlan::new(s.main_part());
    if before == Some(node) {
        return s.commit_plan(plan); // 已在目标位置
    }
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Move { node, parent: Target::Node(parent), before });
    // 搬出单元格后原格可能空了；搬进格尾的非段落块后面要补段落
    if let Some(from) = dom.parent(node).filter(|&f| f != parent) {
        keep_cell_paragraph(dom, from, Some(node), &mut plan);
    }
    if before.is_none() && !dom.is(node, w(LocalName::P)) {
        keep_cell_paragraph(dom, parent, None, &mut plan);
    }
    s.commit_plan(plan)
}

/// `EDIT-03` 表格通则：**单元格最后一个块必须是 `w:p`**（Word 的约束）。计划生效后 `container`
/// （只管 `w:tc`）的末尾不是段落时，追加一个 `New` 空 `w:p`。`removed` 是这次计划里要删除 / 搬走的节点。
fn keep_cell_paragraph(
    dom: &Dom,
    container: NodeId,
    removed: Option<NodeId>,
    plan: &mut MutationPlan,
) {
    if !dom.is(container, w(LocalName::Tc)) {
        return;
    }
    let last = dom
        .children(container)
        .iter()
        .copied()
        .rev()
        .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
        .find(|&c| Some(c) != removed);
    if last.is_some_and(|n| dom.is(n, w(LocalName::P))) {
        return;
    }
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(container),
        before: None,
        node: NewElement::new(w(LocalName::P)),
    });
}

// ---- 批注（`EDIT-03` AddComment / RemoveComment / SetCommentText，任务 2.6）--------------------

/// 段落里第 `boundary` 个内容项（`None` = 边界在末尾，插入时追加）。
fn content_site(dom: &Dom, para: NodeId, boundary: u32) -> Option<NodeId> {
    crate::span::content_children(dom, para).get(boundary as usize).copied()
}

/// `InlinePos` → 段落内容序列的边界。位置必须已经在 inline 边界上（先 `split_at`）。
fn content_boundary(s: &EditSession, para: NodeId, at: InlinePos) -> Result<u32> {
    let tb = text_block(s, para)?;
    let dom = s.dom();
    match locate(tb, at.offset)? {
        Loc::Boundary { index } => {
            let len = crate::span::content_len(dom, para);
            match tb.inlines.get(index).and_then(Inline::node) {
                // 段落层的内容项：inline 可能在 `w:hyperlink` / `w:ins` 里，取它在段落下的那一层
                Some(n) => {
                    Ok(crate::span::boundary_before(dom, para, top_child(dom, para, n))
                        .unwrap_or(len))
                }
                None => Ok(len),
            }
        }
        _ => Err(unsupported("批注端点没落在 inline 边界上（内部错误）")),
    }
}

/// `w:commentReference` run（带 `CommentReference` 字符样式，与 Word 一致）。
fn comment_reference_run(id: &str) -> NewElement {
    let rpr = NewElement::new(w(LocalName::RPr)).with_child(
        NewElement::new(w(LocalName::RStyle)).with_attr(w(LocalName::Val), "CommentReference"),
    );
    NewElement::new(w(LocalName::R))
        .with_child(rpr)
        .with_child(NewElement::new(w(LocalName::CommentReference)).with_attr(w(LocalName::Id), id))
}

/// 条目段落：每段一串 run（`NewRun.props` 是整份 `w:rPr`）。
pub(crate) type EntryParas = Vec<Vec<NewRun>>;

/// 纯文本按 `\n` 分段，每段一个 run（可带一份共用的 `rPr`）。
pub(crate) fn text_entry_paras(text: &str, rpr: Option<&NewElement>) -> EntryParas {
    let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.split('\n').collect() };
    lines
        .into_iter()
        .map(|line| vec![NewRun { text: line.to_string(), props: rpr.cloned() }])
        .collect()
}

/// 一个 `w:r`：`props` 是整份 `w:rPr`，控制字符按 `MOD-06` 折回 `w:tab` / `w:br`。
fn entry_run(r: &NewRun) -> NewElement {
    let mut e = NewElement::new(w(LocalName::R));
    if let Some(p) = &r.props {
        e.push_child(p.clone());
    }
    for seg in text_segments(&r.text, false) {
        e.push_child(seg);
    }
    e
}

/// 批注 / 注释条目的段落：首段可带一个引导 run（批注的 `w:annotationRef`、注释的
/// `w:footnoteRef`），末段带 `w14:paraId`（`commentsExtended` 按它关联）。
fn entry_paragraphs(
    paras: &EntryParas,
    para_id: Option<&str>,
    lead: Option<NewElement>,
) -> Vec<NewElement> {
    let w14 = |l: LocalName| QName::new(NsId::W14, l);
    let last = paras.len().saturating_sub(1);
    paras
        .iter()
        .enumerate()
        .map(|(i, runs)| {
            let mut p = NewElement::new(w(LocalName::P));
            if i == last
                && let Some(pid) = para_id
            {
                p.push_attr(w14(LocalName::ParaId), pid.to_string());
            }
            if i == 0
                && let Some(l) = &lead
            {
                p.push_child(l.clone());
            }
            for r in runs {
                p.push_child(entry_run(r));
            }
            p
        })
        .collect()
}

/// 批注条目首段的引用标记 run（`CommentReference` 样式 + `w:annotationRef`）。
fn annotation_ref_run() -> NewElement {
    let rpr = NewElement::new(w(LocalName::RPr)).with_child(
        NewElement::new(w(LocalName::RStyle)).with_attr(w(LocalName::Val), "CommentReference"),
    );
    NewElement::new(w(LocalName::R))
        .with_child(rpr)
        .with_child(NewElement::new(w(LocalName::AnnotationRef)))
}

/// 会话内唯一的 `w14:paraId`（8 位十六进制，避开已用的）。
fn fresh_para_id(s: &EditSession, seed: u32) -> String {
    let used: Vec<&str> =
        s.document().comments.items.iter().filter_map(|c| c.para_id.as_deref()).collect();
    let mut n = 0x1000_0000u32.wrapping_add(seed.wrapping_mul(0x9E37_79B9));
    loop {
        let candidate = format!("{n:08X}");
        if !used.contains(&candidate.as_str()) {
            return candidate;
        }
        n = n.wrapping_add(1);
    }
}

fn add_comment(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    c: &super::NewComment,
) -> Result<MutationResult> {
    if from.para != to.para {
        return Err(Error::edit(
            DiagCode::EditCrossParagraph,
            "AddComment 两端不在同一段落（M2 只支持同段）",
        ));
    }
    if from.offset > to.offset {
        return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
    }
    let part = s.main_part();
    // 位置先落到 inline 边界（拆 run 是独立阶段，失败由事务回滚）
    let mut result = MutationResult::default();
    let tb = text_block(s, from.para)?;
    let loc_to = locate(tb, to.offset)?;
    split_at(s, to.para, loc_to, &mut result)?;
    let tb = text_block(s, from.para)?;
    let loc_from = locate(tb, from.offset)?;
    split_at(s, from.para, loc_from, &mut result)?;

    // 批注部件与条目（`SAVE-05` + `EDIT-06`）
    let comments_part = s.ensure_comments_part()?;
    let id = s.document().comments.next_id().to_string();
    let para_id = fresh_para_id(s, s.document().comments.items.len() as u32 + 1);
    let mut entry = NewElement::new(w(LocalName::Comment)).with_attr(w(LocalName::Id), id.clone());
    entry.push_attr(w(LocalName::Author), c.author.clone());
    if let Some(i) = &c.initials {
        entry.push_attr(w(LocalName::Initials), i.clone());
    }
    if let Some(d) = &c.date {
        entry.push_attr(w(LocalName::Date), d.clone());
    }
    for p in entry_paragraphs(
        &text_entry_paras(&c.text, None),
        Some(&para_id),
        Some(annotation_ref_run()),
    ) {
        entry.push_child(p);
    }
    let cdom = s.package().part(comments_part).dom().expect("comments part is parsed");
    let croot = cdom.root();
    let mut cplan = MutationPlan::new(comments_part);
    cplan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(croot),
        before: None,
        node: entry,
    });
    result.absorb(s.commit_plan(cplan)?);

    // 正文：范围标记 + reference run
    let a = content_boundary(s, from.para, from)?;
    let b = content_boundary(s, to.para, to)?;
    let dom = s.dom();
    let start_before = content_site(dom, from.para, a);
    let end_before = content_site(dom, to.para, b);
    let marker =
        |local: LocalName| NewElement::new(w(local)).with_attr(w(LocalName::Id), id.clone());
    let mut plan = MutationPlan::new(part);
    plan.touch(from.para);
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(from.para),
        before: start_before,
        node: marker(LocalName::CommentRangeStart),
    });
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(to.para),
        before: end_before,
        node: marker(LocalName::CommentRangeEnd),
    });
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(to.para),
        before: end_before,
        node: comment_reference_run(&id),
    });
    let r = s.commit_plan(plan)?;
    let (start_marker, end_marker, ref_run) = (
        r.created[0].ok_or_else(|| unsupported("范围起点没创建"))?,
        r.created[1].ok_or_else(|| unsupported("范围终点没创建"))?,
        r.created[2].ok_or_else(|| unsupported("reference run 没创建"))?,
    );
    result.absorb(r);

    // 登记范围：标记已经在 DOM 里，物化时正好在锚点位置上（不会重发）
    let dom = s.dom();
    let anchor = |node: NodeId, aff: Affinity| {
        Anchor::at(
            from.para,
            crate::span::boundary_before(dom, from.para, node).unwrap_or(0),
            aff,
            node,
        )
    };
    let span = RangeSpan {
        id: SpanId(0),
        part,
        flow: s.document().flows.flow_of(from.para).unwrap_or(FlowId(0)),
        kind: RangeKind::Comment { id: id.clone(), reference: Some(ref_run) },
        origin: SpanOrigin::New,
        implicit: false,
        removed: false,
        start: Some(anchor(start_marker, Affinity::Right)),
        end: Some(anchor(end_marker, Affinity::Left)),
    };
    s.push_span(part, span)?;
    // 回复与已解决在 `commentsExtended`
    if c.parent_id.is_some() || c.done {
        let parent_para = c
            .parent_id
            .as_deref()
            .and_then(|pid| s.document().comments.get(pid))
            .and_then(|p| p.para_id.clone());
        result.absorb(set_comment_ex(s, &para_id, parent_para.as_deref(), c.done)?);
    }
    s.rebuild()?;
    Ok(result)
}

/// `commentsExtended` 里的一条 `w15:commentEx`（没有就建 part / 建条目）。
fn set_comment_ex(
    s: &mut EditSession,
    para_id: &str,
    parent_para: Option<&str>,
    done: bool,
) -> Result<MutationResult> {
    let part = s.ensure_comments_extended_part()?;
    let w15 = |l: LocalName| QName::new(NsId::W15, l);
    let dom = s.package().part(part).dom().expect("commentsExtended is parsed");
    let root = dom.root();
    let existing = dom.semantic_children(root).find(|&n| {
        dom.is(n, w15(LocalName::CommentEx))
            && dom.attr_value(n, w15(LocalName::ParaId)).as_deref() == Some(para_id)
    });
    let mut plan = MutationPlan::new(part);
    if let Some(node) = existing {
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(node),
            name: w15(LocalName::Done),
            value: if done { "1".into() } else { "0".into() },
        });
        match parent_para {
            Some(p) => plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(node),
                name: w15(LocalName::ParaIdParent),
                value: p.to_string(),
            }),
            None => plan.node_edits.push(NodeEdit::RemoveAttr {
                node: Target::Node(node),
                name: w15(LocalName::ParaIdParent),
            }),
        }
    } else {
        let mut e =
            NewElement::new(w15(LocalName::CommentEx)).with_attr(w15(LocalName::ParaId), para_id);
        if let Some(p) = parent_para {
            e.push_attr(w15(LocalName::ParaIdParent), p);
        }
        e.push_attr(w15(LocalName::Done), if done { "1" } else { "0" });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(root),
            before: None,
            node: e,
        });
    }
    s.commit_plan(plan)
}

fn remove_comment(s: &mut EditSession, id: &str) -> Result<MutationResult> {
    let part = s.main_part();
    let entry =
        s.document().comments.get(id).map(|c| (c.node, c.para_id.clone())).ok_or_else(|| {
            Error::edit(DiagCode::EditBadPosition, format!("没有 id 为 {id} 的批注"))
        })?;
    let comments_part = s
        .document()
        .comments
        .part
        .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "批注条目在的 part 找不到"))?;
    // 正文：范围标记与 reference run
    let mut victims: Vec<NodeId> = Vec::new();
    let mut spans: Vec<SpanId> = Vec::new();
    {
        // 索引可能还没建（这是本次会话第一次写主 part）
        let index = s.spans_of(part)?;
        for span in index.live() {
            if span.class() != RangeClass::Comment || span.pair_id() != id {
                continue;
            }
            spans.push(span.id);
            victims.extend(span.start.and_then(|a| a.marker));
            victims.extend(span.end.and_then(|a| a.marker));
            if let RangeKind::Comment { reference: Some(r), .. } = &span.kind {
                victims.push(*r);
            }
        }
    }
    let dom = s.dom();
    victims.retain(|&n| dom.node(n).dirty != Dirty::Deleted);
    if !victims.is_empty() {
        let mut plan = MutationPlan::new(part);
        for n in &victims {
            if let Some(p) = dom.ancestors(*n).find(|&a| dom.is(a, w(LocalName::P))) {
                plan.touch(p);
            }
            plan.node_edits.push(NodeEdit::Delete(*n));
        }
        s.commit_plan(plan)?;
    }
    for span in spans {
        s.drop_span(part, span);
    }
    // 条目与 `commentsExtended` 条目
    let mut plan = MutationPlan::new(comments_part);
    plan.node_edits.push(NodeEdit::Delete(entry.0));
    let mut result = s.commit_plan(plan)?;
    if let (Some(ex_part), Some(pid)) = (s.document().comments.extended_part, entry.1) {
        let dom = s.package().part(ex_part).dom().expect("commentsExtended is parsed");
        let w15 = |l: LocalName| QName::new(NsId::W15, l);
        let victim = dom.semantic_children(dom.root()).find(|&n| {
            dom.is(n, w15(LocalName::CommentEx))
                && dom.attr_value(n, w15(LocalName::ParaId)).as_deref() == Some(pid.as_str())
        });
        if let Some(node) = victim {
            let mut plan = MutationPlan::new(ex_part);
            plan.node_edits.push(NodeEdit::Delete(node));
            result.absorb(s.commit_plan(plan)?);
        }
    }
    s.rebuild()?;
    Ok(result)
}

fn set_comment_text(
    s: &mut EditSession,
    id: &str,
    text: &str,
    done: Option<bool>,
) -> Result<MutationResult> {
    let (node, para_id, first_rpr) = {
        let c = s.document().comments.get(id).ok_or_else(|| {
            Error::edit(DiagCode::EditBadPosition, format!("没有 id 为 {id} 的批注"))
        })?;
        let dom = s.package().part(s.document().comments.part.expect("有条目就有 part")).dom();
        // 保留第一个有字 run 的格式（加粗 / 颜色的批注改字后不变素）
        let rpr = dom.and_then(|d| {
            c.rich
                .iter()
                .flatten()
                .next()
                .and_then(|r| d.semantic_children(r.node).find(|&n| d.is(n, w(LocalName::RPr))))
                .and_then(|n| NewElement::from_dom(d, n, &mut crate::xml::Interner::new()))
        });
        (c.node, c.para_id.clone(), rpr)
    };
    let comments_part =
        s.document().comments.part.ok_or_else(|| unsupported("批注条目在的 part 找不到"))?;
    let para_id = para_id.unwrap_or_else(|| fresh_para_id(s, 1));
    let dom = s.package().part(comments_part).dom().expect("comments part is parsed");
    let mut plan = MutationPlan::new(comments_part);
    for c in dom.semantic_children(node) {
        if dom.is(c, w(LocalName::P)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
    }
    let paras = text_entry_paras(text, first_rpr.as_ref());
    for p in entry_paragraphs(&paras, Some(&para_id), Some(annotation_ref_run())) {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(node),
            before: None,
            node: p,
        });
    }
    let mut result = s.commit_plan(plan)?;
    if let Some(done) = done {
        let parent = s.document().comments.get(id).and_then(|c| c.parent_id.clone());
        let parent_para = parent
            .as_deref()
            .and_then(|pid| s.document().comments.get(pid))
            .and_then(|p| p.para_id.clone());
        result.absorb(set_comment_ex(s, &para_id, parent_para.as_deref(), done)?);
    }
    s.rebuild()?;
    Ok(result)
}

/// compat 的权威列表路径用的批注条目 upsert（`COMPAT-04` 的 `SaveOptions.comments`）。
///
/// 条目在就改（正文重写、属性按需改），不在就新建；**不动正文里的范围标记**——标记的位置由
/// 块的 `commentStarts` / `commentEnds` / `commentIds` 决定。
pub(crate) fn upsert_comment_entry(
    s: &mut EditSession,
    id: &str,
    c: &super::NewComment,
    paras: &EntryParas,
) -> Result<MutationResult> {
    let (author, initials, date) = (
        (!c.author.is_empty()).then_some(c.author.as_str()),
        c.initials.as_deref(),
        c.date.as_deref(),
    );
    let (parent_id, done) = (c.parent_id.as_deref(), c.done);
    let part = s.ensure_comments_part()?;
    let existing = s.document().comments.get(id).map(|c| (c.node, c.para_id.clone()));
    let mut result = MutationResult::default();
    let para_id = match &existing {
        Some((_, Some(pid))) => pid.clone(),
        _ => fresh_para_id(s, id.len() as u32 + 1),
    };
    let body = entry_paragraphs(paras, Some(&para_id), Some(annotation_ref_run()));
    let mut plan = MutationPlan::new(part);
    match existing {
        Some((node, _)) => {
            let dom = s.package().part(part).dom().expect("comments part is parsed");
            for c in dom.semantic_children(node) {
                if dom.is(c, w(LocalName::P)) {
                    plan.node_edits.push(NodeEdit::Delete(c));
                }
            }
            for (name, value) in [
                (LocalName::Author, author),
                (LocalName::Initials, initials),
                (LocalName::Date, date),
            ] {
                match value {
                    Some(v) => plan.node_edits.push(NodeEdit::SetAttr {
                        node: Target::Node(node),
                        name: w(name),
                        value: v.to_string(),
                    }),
                    None => plan
                        .node_edits
                        .push(NodeEdit::RemoveAttr { node: Target::Node(node), name: w(name) }),
                }
            }
            for p in body {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(node),
                    before: None,
                    node: p,
                });
            }
        }
        None => {
            let dom = s.package().part(part).dom().expect("comments part is parsed");
            let root = dom.root();
            let mut entry = NewElement::new(w(LocalName::Comment)).with_attr(w(LocalName::Id), id);
            if let Some(a) = author {
                entry.push_attr(w(LocalName::Author), a);
            }
            if let Some(i) = initials {
                entry.push_attr(w(LocalName::Initials), i);
            }
            if let Some(d) = date {
                entry.push_attr(w(LocalName::Date), d);
            }
            for p in body {
                entry.push_child(p);
            }
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node: entry,
            });
        }
    }
    result.absorb(s.commit_plan(plan)?);
    if parent_id.is_some() || done {
        let parent_para = parent_id
            .and_then(|pid| s.document().comments.get(pid))
            .and_then(|p| p.para_id.clone());
        result.absorb(set_comment_ex(s, &para_id, parent_para.as_deref(), done)?);
    }
    s.rebuild()?;
    Ok(result)
}

/// 注释条目 upsert（`SaveOptions.footnotes` / `endnotes`）。
///
/// 条目在就只重写正文段落、**保留自引用标记 run**（`w:footnoteRef` 是编号，不能丢）；
/// 不在就新建条目。结构条目（`separator` 一类）一个字节不动。
pub(crate) fn upsert_note_entry(
    s: &mut EditSession,
    endnote: bool,
    id: &str,
    paras: &EntryParas,
) -> Result<MutationResult> {
    let part = s.ensure_notes_part(endnote)?;
    let (entry_name, ref_name) = if endnote {
        (LocalName::Endnote, LocalName::EndnoteRef)
    } else {
        (LocalName::Footnote, LocalName::FootnoteRef)
    };
    let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
    let existing = notes.get(id).map(|n| n.node);
    let mut plan = MutationPlan::new(part);
    let dom = s.package().part(part).dom().expect("notes part is parsed");
    match existing {
        Some(node) => {
            // 只重发正文段落：把段落里除自引用标记 run 之外的内容换掉
            let ref_run = dom
                .descendants(node)
                .find(|&n| dom.is(n, w(ref_name)))
                .and_then(|m| dom.ancestors(m).find(|&a| dom.is(a, w(LocalName::R))));
            let lead = ref_run
                .and_then(|r| NewElement::from_dom(dom, r, &mut crate::xml::Interner::new()));
            for c in dom.semantic_children(node) {
                if dom.is(c, w(LocalName::P)) {
                    plan.node_edits.push(NodeEdit::Delete(c));
                }
            }
            for p in entry_paragraphs(paras, None, lead) {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(node),
                    before: None,
                    node: p,
                });
            }
        }
        None => {
            let root = dom.root();
            let lead = NewElement::new(w(LocalName::R)).with_child(NewElement::new(w(ref_name)));
            let mut entry = NewElement::new(w(entry_name)).with_attr(w(LocalName::Id), id);
            for p in entry_paragraphs(paras, None, Some(lead)) {
                entry.push_child(p);
            }
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node: entry,
            });
        }
    }
    let r = s.commit_plan(plan)?;
    s.rebuild()?;
    Ok(r)
}

/// 从注释部件里删掉一条正文条目（结构条目不动）。
pub(crate) fn remove_note_entry(
    s: &mut EditSession,
    endnote: bool,
    id: &str,
) -> Result<MutationResult> {
    let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
    let Some(part) = notes.part else { return Ok(MutationResult::default()) };
    let Some(node) = notes.get(id).map(|n| n.node) else { return Ok(MutationResult::default()) };
    let mut plan = MutationPlan::new(part);
    plan.node_edits.push(NodeEdit::Delete(node));
    let r = s.commit_plan(plan)?;
    s.rebuild()?;
    Ok(r)
}

// ---- 段落拆分与合并（`EDIT-03`，任务 2.9）------------------------------------------------------

/// 段落里横跨内容边界 `k` 的字段：拆分会让它跨段（`FLD-08` 里那就变成 `Block` 策略）。
///
/// 判定按段落层的内容项下标：字段的 head 与 tail 在段落下各属一个内容项（可能是同一个），
/// 边界落在两者**之间**就是横跨。原子形态字段的内部位置在 `locate` 那一步就被拒了。
fn field_across(s: &EditSession, para: NodeId, k: u32) -> Option<crate::span::FieldId> {
    let dom = s.dom();
    let fields = &s.document().fields;
    for f in fields.fields() {
        let (head, tail) = (f.form.head(), f.form.tail());
        if !dom.ancestors(head).any(|a| a == para) && head != para {
            continue;
        }
        let item_of = |n: NodeId| crate::span::boundary_before(dom, para, top_child(dom, para, n));
        let (Some(h), Some(t)) = (item_of(head), item_of(tail)) else { continue };
        if k > h && k <= t {
            return Some(f.id);
        }
    }
    None
}

/// `Block` 策略字段的结果段落只读（`FLD-07`）。
fn refuse_block_field_result(s: &EditSession, para: NodeId) -> Result<()> {
    if s.document().fields.block_result_paragraphs(s.dom()).contains_key(&para) {
        return Err(unsupported("Block 字段（TOC 等）的结果段落只读"));
    }
    Ok(())
}

fn split_paragraph(s: &mut EditSession, at: InlinePos) -> Result<MutationResult> {
    let part = s.main_part();
    refuse_block_field_result(s, at.para)?;
    let tb = text_block(s, at.para)?;
    let loc = locate(tb, at.offset)?;
    // 位置落在 run 内部 → 先拆 run（原子内部的位置 `locate` 已经拒了）
    let mut result = MutationResult::default();
    split_at(s, at.para, loc, &mut result)?;
    let k = content_boundary(s, at.para, at)?;
    if let Some(id) = field_across(s, at.para, k) {
        return Err(Error::edit(
            DiagCode::EditSplitField,
            format!("拆分会让字段 {} 跨段（FLD-08）", id.0),
        ));
    }
    let dom = s.dom();
    let parent = dom.parent(at.para).ok_or_else(|| unsupported("段落没有父节点"))?;
    let ppr = ppr_of(dom, at.para);
    let after = next_sibling(dom, at.para);
    // 阶段 1：建新段落（`pPr` 字节克隆，`XML-12` 规则 F），插在原段之后
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(parent),
        before: after,
        node: NewElement::new(w(LocalName::P)),
    });
    if let Some(ppr) = ppr {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(0),
            before: None,
            source: ppr,
        });
    }
    let r = s.commit_plan(plan)?;
    let tail = r.created[0].ok_or_else(|| unsupported("新段落没创建"))?;
    result.absorb(r);

    // 阶段 2：把边界之后的内容项与范围标记搬进新段落（`SPAN-06` 拆分规则由 `SpanPolicy` 表达）
    let dom = s.dom();
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.span.splits.push(crate::span::ContainerSplit { source: at.para, boundary: k, tail });
    let mut index = 0u32;
    for c in dom.semantic_children(at.para).collect::<Vec<_>>() {
        let Some(name) = dom.name(c) else { continue };
        if is_property_element(name) {
            continue;
        }
        if crate::span::is_range_marker(name) {
            // 标记跟着它右边的内容走：边界处的标记留在前段（终点）或跟去后段（起点），
            // 物化会按锚点摆正，这里只要不把它落在错误的段里
            if index > k {
                plan.node_edits.push(NodeEdit::Move {
                    node: c,
                    parent: Target::Node(tail),
                    before: None,
                });
            }
            continue;
        }
        if index >= k {
            plan.node_edits.push(NodeEdit::Move {
                node: c,
                parent: Target::Node(tail),
                before: None,
            });
        }
        index += 1;
    }
    result.absorb(s.commit_plan(plan)?);
    result.structure_changed = true;
    Ok(result)
}

fn merge_with_next(s: &mut EditSession, para: NodeId) -> Result<MutationResult> {
    let part = s.main_part();
    require_paragraph(s.dom(), para)?;
    refuse_block_field_result(s, para)?;
    let dom = s.dom();
    let next = next_element_sibling(dom, para)
        .filter(|&n| dom.is(n, w(LocalName::P)))
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "下一个块不是段落"))?;
    refuse_block_field_result(s, next)?;
    let offset = crate::span::content_len(dom, para);
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.touch(para);
    // `SPAN-06` 合并行：`next` 里的锚点整体搬到 `para`，下标加上原有内容项数
    plan.span.merges.push(crate::span::ContainerMerge { source: next, into: para, offset });
    // 内容项与范围标记按原顺序接到 `para` 末尾；`next` 的 `pPr` 随它一起消失
    // （Word 语义：合并后保留**前**段属性）
    for c in dom.semantic_children(next).collect::<Vec<_>>() {
        let Some(name) = dom.name(c) else { continue };
        if is_property_element(name) {
            continue;
        }
        plan.node_edits.push(NodeEdit::Move { node: c, parent: Target::Node(para), before: None });
    }
    plan.node_edits.push(NodeEdit::Delete(next));
    s.commit_plan(plan)
}

// ---- 书签（`EDIT-03` AddBookmark / RemoveBookmark，任务 2.9）-----------------------------------

/// `EDIT-06`：书签 `w:id` 在 part 内取最大值 + 1。
fn next_bookmark_id(s: &EditSession) -> u32 {
    let dom = s.dom();
    let mut max = 0u32;
    for n in dom.descendants(dom.root()) {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let is_marker =
            dom.is(n, w(LocalName::BookmarkStart)) || dom.is(n, w(LocalName::BookmarkEnd));
        if is_marker
            && let Some(v) = dom.attr_value(n, w(LocalName::Id))
            && let Ok(id) = v.trim().parse::<u32>()
        {
            max = max.max(id);
        }
    }
    max + 1
}

fn add_bookmark(
    s: &mut EditSession,
    name: &str,
    from: InlinePos,
    to: InlinePos,
) -> Result<MutationResult> {
    if name.is_empty() {
        return Err(Error::edit(DiagCode::EditBadPosition, "书签名为空"));
    }
    if from.para != to.para {
        return Err(Error::edit(
            DiagCode::EditCrossParagraph,
            "AddBookmark 两端不在同一段落（M2 只支持同段）",
        ));
    }
    if from.offset > to.offset {
        return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
    }
    let part = s.main_part();
    // 名字全文档唯一（`EDIT-03`）
    if s.spans_of(part)?.live().any(|sp| sp.kind.bookmark_name() == Some(name)) {
        return Err(Error::edit(DiagCode::EditBadPosition, format!("书签名 {name:?} 已存在")));
    }
    // 两端落到 inline 边界
    let mut result = MutationResult::default();
    let tb = text_block(s, from.para)?;
    let loc_to = locate(tb, to.offset)?;
    split_at(s, to.para, loc_to, &mut result)?;
    let tb = text_block(s, from.para)?;
    let loc_from = locate(tb, from.offset)?;
    split_at(s, from.para, loc_from, &mut result)?;

    let id = next_bookmark_id(s).to_string();
    let a = content_boundary(s, from.para, from)?;
    let b = content_boundary(s, to.para, to)?;
    let dom = s.dom();
    let start_before = content_site(dom, from.para, a);
    let end_before = content_site(dom, to.para, b);
    let mut plan = MutationPlan::new(part);
    plan.touch(from.para);
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(from.para),
        before: start_before,
        node: NewElement::new(w(LocalName::BookmarkStart))
            .with_attr(w(LocalName::Id), id.clone())
            .with_attr(w(LocalName::Name), name),
    });
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(to.para),
        before: end_before,
        node: NewElement::new(w(LocalName::BookmarkEnd)).with_attr(w(LocalName::Id), id.clone()),
    });
    let r = s.commit_plan(plan)?;
    let (start_marker, end_marker) = (
        r.created[0].ok_or_else(|| unsupported("书签起点没创建"))?,
        r.created[1].ok_or_else(|| unsupported("书签终点没创建"))?,
    );
    result.absorb(r);
    let dom = s.dom();
    let anchor = |node: NodeId, aff: Affinity| {
        Anchor::at(
            from.para,
            crate::span::boundary_before(dom, from.para, node).unwrap_or(0),
            aff,
            node,
        )
    };
    let (mut start, mut end) =
        (anchor(start_marker, Affinity::Right), anchor(end_marker, Affinity::Left));
    // 空书签两端同向（`SPAN-02` 例外）
    if start.same_place(&end) {
        start.affinity = Affinity::Right;
        end.affinity = Affinity::Right;
    }
    s.push_span(
        part,
        RangeSpan {
            id: SpanId(0),
            part,
            flow: s.document().flows.flow_of(from.para).unwrap_or(FlowId(0)),
            kind: RangeKind::Bookmark {
                id,
                name: name.to_string(),
                hidden: name.starts_with('_'),
                cols: None,
            },
            origin: SpanOrigin::New,
            implicit: false,
            removed: false,
            start: Some(start),
            end: Some(end),
        },
    )?;
    Ok(result)
}

fn remove_bookmark(s: &mut EditSession, name: &str) -> Result<MutationResult> {
    let part = s.main_part();
    let mut victims: Vec<NodeId> = Vec::new();
    let mut spans: Vec<SpanId> = Vec::new();
    {
        let index = s.spans_of(part)?;
        for sp in index.live() {
            if sp.kind.bookmark_name() != Some(name) {
                continue;
            }
            spans.push(sp.id);
            victims.extend(sp.start.and_then(|a| a.marker));
            victims.extend(sp.end.and_then(|a| a.marker));
        }
    }
    if spans.is_empty() {
        return Err(Error::edit(DiagCode::EditBadPosition, format!("没有名为 {name:?} 的书签")));
    }
    let dom = s.dom();
    victims.retain(|&n| dom.node(n).dirty != Dirty::Deleted);
    let mut plan = MutationPlan::new(part);
    for n in &victims {
        if let Some(p) = dom.ancestors(*n).find(|&a| dom.is(a, w(LocalName::P))) {
            plan.touch(p);
        }
        plan.node_edits.push(NodeEdit::Delete(*n));
    }
    let r = s.commit_plan(plan)?;
    for span in spans {
        s.drop_span(part, span);
    }
    Ok(r)
}

// ---- 字段操作（`FLD-09`–`FLD-12`，任务 2.9）----------------------------------------------------

fn field_of(s: &EditSession, id: crate::span::FieldId) -> Result<&crate::span::FieldSpan> {
    s.document()
        .fields
        .get(id)
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, format!("没有字段 {}", id.0)))
}

fn insert_field(
    s: &mut EditSession,
    at: InlinePos,
    field: &super::NewField,
    ctx: &EditContext,
) -> Result<MutationResult> {
    refuse_block_field_result(s, at.para)?;
    let part = s.main_part();
    let mut result = MutationResult::default();
    let tb = text_block(s, at.para)?;
    let loc = locate(tb, at.offset)?;
    let (parent, before, inherit) = match split_at(s, at.para, loc, &mut result)? {
        Some((left, right)) => {
            (s.dom().parent(left).expect("run has a parent"), Some(right), Some(left))
        }
        None => {
            let Loc::Boundary { index } = loc else { unreachable!("split_at handles the rest") };
            boundary_site(s, text_block(s, at.para)?, index)?
        }
    };
    let flavor = s.flavor();
    let dom = s.dom();
    // 继承格式：优先左侧 run 的 `rPr` 字节克隆，其次 `default_run_props`
    let rpr = match inherit.and_then(|r| rpr_of(dom, r)) {
        Some(node) => NewElement::from_dom(dom, node, &mut crate::xml::Interner::new()),
        None => ctx.default_run_props.as_ref().map(|d| emit_run_props(d, flavor)),
    };
    let mut plan = MutationPlan::new(part);
    plan.touch(at.para);
    let inline = NewInline::Field {
        instr: field.instr.clone(),
        result: field.result.clone(),
        separate: true,
        dirty: field.mark_dirty,
        props: rpr,
    };
    for node in emit_inlines(dom, std::slice::from_ref(&inline)) {
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(parent), before, node });
    }
    result.absorb(s.commit_plan(plan)?);
    // 字段索引是投影：提交后已经重建，新字段按 `FLD-06` 归策略
    Ok(result)
}

/// `FLD-07 Link`：改链接目标。
fn set_link_target(
    s: &mut EditSession,
    link: super::LinkRef,
    target: &super::LinkDest,
) -> Result<MutationResult> {
    match link {
        super::LinkRef::Field(id) => set_field_link_target(s, id, target),
        super::LinkRef::Element(node) => set_hyperlink_target(s, node, target),
    }
}

/// `w:hyperlink` 元素：外部 URL 先按 `EDIT-06` 分配关系，再改 `r:id`；书签改 `w:anchor`。
/// 两个属性互斥（Word 只认一个），所以设一个就删另一个。
fn set_hyperlink_target(
    s: &mut EditSession,
    node: NodeId,
    target: &super::LinkDest,
) -> Result<MutationResult> {
    let part = s.main_part();
    if !s.dom().is(node, w(LocalName::Hyperlink)) {
        return Err(Error::edit(DiagCode::EditBadPosition, "节点不是 w:hyperlink"));
    }
    let rid_q = QName::new(NsId::R, LocalName::Id);
    let anchor_q = w(LocalName::Anchor);
    let (set, remove, value) = match target {
        super::LinkDest::Url(url) => {
            let rid = s.add_external_relationship(part, RelType::Hyperlink, url)?;
            (rid_q, anchor_q, rid)
        }
        super::LinkDest::Rel(rid) => (rid_q, anchor_q, rid.clone()),
        super::LinkDest::Anchor(name) => (anchor_q, rid_q, name.clone()),
    };
    let dom = s.dom();
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(node).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    plan.node_edits.push(NodeEdit::SetAttr { node: Target::Node(node), name: set, value });
    if dom.attr(node, remove).is_some() {
        plan.node_edits.push(NodeEdit::RemoveAttr { node: Target::Node(node), name: remove });
    }
    s.commit_plan(plan)
}

/// HYPERLINK 字段：只重写 `instrText` 的文本，第一个参数之后的开关原文保留。
fn set_field_link_target(
    s: &mut EditSession,
    id: crate::span::FieldId,
    target: &super::LinkDest,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    if *f.keyword() != crate::span::field::Keyword::Hyperlink {
        return Err(unsupported("SetLinkTarget 只用于 HYPERLINK 字段"));
    }
    let crate::span::FieldForm::Complex { instr_nodes, .. } = &f.form else {
        return Err(unsupported("SetLinkTarget 暂不支持 w:fldSimple（改 @w:instr 属性）"));
    };
    let instr_nodes = instr_nodes.clone();
    let raw = f.instr.raw.clone();
    let dom = s.dom();
    // 保留除第一个参数之外的全部原文（开关 `\o "tip"` 等）
    let rest = remaining_after_first_argument(&raw);
    let head = match target {
        super::LinkDest::Url(url) => format!("HYPERLINK \"{url}\""),
        // 文内链接：Word 写 `HYPERLINK \l "bookmark"`
        super::LinkDest::Anchor(name) => format!("HYPERLINK \\l \"{name}\""),
        super::LinkDest::Rel(_) => {
            return Err(unsupported("字段形式的链接没有关系 id（用 LinkDest::Url）"));
        }
    };
    let rest = if matches!(target, super::LinkDest::Anchor(_)) {
        // `\l` 自己就是开关，去掉原来的 `\l`
        rest.split_whitespace().collect::<Vec<_>>().join(" ").replace("\\l ", "")
    } else {
        rest
    };
    let text = if rest.is_empty() { format!(" {head} ") } else { format!(" {head} {rest} ") };
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(instr_nodes[0]).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    // 指令拆在多个 `w:instrText` 里时（`FLD-03`）：第一个写全量，其余清空
    let mut first = true;
    for run in &instr_nodes {
        for seg in dom.semantic_children(*run).collect::<Vec<_>>() {
            if !dom.is(seg, w(LocalName::InstrText)) {
                continue;
            }
            let value = if first { text.clone() } else { String::new() };
            first = false;
            set_segment_text(dom, seg, &value, &mut plan);
        }
    }
    if first {
        return Err(unsupported("字段没有 w:instrText 可改"));
    }
    s.commit_plan(plan)
}

/// 指令原文里第一个参数之后的部分（开关等），已 trim。
fn remaining_after_first_argument(raw: &str) -> String {
    let t = raw.trim();
    let after_keyword = t.split_once(char::is_whitespace).map(|(_, r)| r.trim()).unwrap_or("");
    if after_keyword.is_empty() {
        return String::new();
    }
    let rest = if let Some(stripped) = after_keyword.strip_prefix('"') {
        stripped.split_once('"').map(|(_, r)| r).unwrap_or("")
    } else {
        after_keyword.split_once(char::is_whitespace).map(|(_, r)| r).unwrap_or("")
    };
    rest.trim().to_string()
}

/// `FLD-10`：`w:ffData` 里 `w:checkBox` 的 `w:checked` 取反（不存在则按顺序插在 `w:default` 之后）。
fn toggle_checkbox(s: &mut EditSession, id: crate::span::FieldId) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    let dom = s.dom();
    let data = crate::span::field::read_form_data(dom, f.ff_data)
        .ok_or_else(|| unsupported("字段没有 w:ffData 表单定义（FLD-10）"))?;
    let crate::span::FormData::CheckBox { node, checked, .. } = data else {
        return Err(unsupported("这个字段不是复选框"));
    };
    let head = f.form.head();
    let want = !checked;
    let existing = dom.semantic_children(node).find(|&c| dom.is(c, w(LocalName::Checked)));
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(head).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    match existing {
        Some(c) => plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(c),
            name: w(LocalName::Val),
            value: if want { "1".into() } else { "0".into() },
        }),
        None => {
            // 顺序：`w:size`|`w:sizeAuto`, `w:default`, `w:checked`（`FLD-10`）→ 追加在末尾即可
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(node),
                before: None,
                node: NewElement::new(w(LocalName::Checked))
                    .with_attr(w(LocalName::Val), if want { "1" } else { "0" }),
            });
        }
    }
    s.commit_plan(plan)
}

/// `FLD-10`：FORMTEXT 的结果文字。结果 run 只留一个，文本为 `text`（格式沿用第一个结果 run）。
fn set_form_text(
    s: &mut EditSession,
    id: crate::span::FieldId,
    text: &str,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    let results: Vec<NodeId> = f.form.result_nodes().to_vec();
    let dom = s.dom();
    // 只有一个结果 run 且它有唯一 `w:t` → 直接改文本（最小脏化）
    if results.len() == 1
        && let Some(t) = dom.semantic_children(results[0]).find(|&c| dom.is(c, w(LocalName::T)))
    {
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(results[0]).find(|&a| dom.is(a, w(LocalName::P))) {
            plan.touch(p);
        }
        set_segment_text(dom, t, text, &mut plan);
        return s.commit_plan(plan);
    }
    let rpr = results
        .first()
        .and_then(|&r| rpr_of(dom, r))
        .and_then(|n| NewElement::from_dom(dom, n, &mut crate::xml::Interner::new()));
    let (parent, before) = match results.first() {
        Some(&first) => (dom.parent(first).expect("run has a parent"), Some(first)),
        None => {
            // 没有结果区：插在 end run 之前
            let end = f.form.tail();
            (dom.parent(end).expect("run has a parent"), Some(end))
        }
    };
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(f.form.head()).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(parent),
        before,
        node: {
            let mut r = NewElement::new(w(LocalName::R));
            if let Some(p) = &rpr {
                r.push_child(p.clone());
            }
            for seg in text_segments(text, false) {
                r.push_child(seg);
            }
            r
        },
    });
    for old in &results {
        plan.node_edits.push(NodeEdit::Delete(*old));
    }
    s.commit_plan(plan)
}

/// `FLD-07`：字段结果 run 的格式（原子字段不能按 `SetRunProps` 那样定位，所以单列一个操作）。
fn set_field_result_props(
    s: &mut EditSession,
    id: crate::span::FieldId,
    patch: &RunPropsPatch,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    let results: Vec<NodeId> = f.form.result_nodes().to_vec();
    let head = f.form.head();
    if results.is_empty() {
        return Err(unsupported("字段没有结果区可改格式"));
    }
    let dom = s.dom();
    let mut plan = MutationPlan::new(part);
    if let Some(p) = dom.ancestors(head).find(|&a| dom.is(a, w(LocalName::P))) {
        plan.touch(p);
    }
    let flavor = s.flavor();
    for r in &results {
        if !dom.is(*r, w(LocalName::R)) {
            continue;
        }
        let rpr = rpr_of(dom, *r);
        plan.node_edits.extend(plan_apply_run_props(dom, *r, rpr, patch, flavor));
    }
    s.commit_plan(plan)
}

/// `FLD-09`：块字段更新——用给定的块替换 `separate..end` 之间的全部节点。
///
/// 生成器（TOC 重算等）在 M7；这里是机制：调用方给内容，`w:fldLock` 的字段拒绝（`FLD_LOCKED`）。
/// 结构 run（begin / 指令 / separate / end）与外层容器都保留。跨段字段的结果区里，
/// 中间的整段直接删，begin / end 所在段落里只删属于结果的 run。
fn update_block_field(
    s: &mut EditSession,
    id: crate::span::FieldId,
    blocks: Vec<NewBlock>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    if f.lock {
        return Err(Error::edit(DiagCode::FldLocked, "字段带 w:fldLock，拒绝更新"));
    }
    let crate::span::FieldForm::Complex { separate, end, result_nodes, begin, .. } = &f.form else {
        return Err(unsupported("w:fldSimple 没有 separate..end 区间"));
    };
    if separate.is_none() {
        return Err(unsupported("字段没有 separate，无法定位结果区"));
    }
    let (end, begin) = (*end, *begin);
    let old: Vec<NodeId> = result_nodes.clone();
    let dom = s.dom();
    let para_of = |n: NodeId| {
        std::iter::once(n).chain(dom.ancestors(n)).find(|&a| dom.is(a, w(LocalName::P)))
    };
    let end_para = para_of(end).ok_or_else(|| unsupported("字段 end 不在段落里"))?;
    let begin_para = para_of(begin).ok_or_else(|| unsupported("字段 begin 不在段落里"))?;
    let cross = end_para != begin_para;
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.touch(begin_para);
    plan.touch(end_para);
    if cross {
        // 段落级：新块插在 end 所在段落之前
        let parent = dom.parent(end_para).ok_or_else(|| unsupported("段落没有父节点"))?;
        for b in blocks {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before: Some(end_para),
                node: new_block_element(dom, b),
            });
        }
    } else {
        // 同段：新块的 inline 直接插在 end run 之前（段落里不能塞段落）
        let parent = dom.parent(end).ok_or_else(|| unsupported("字段 end 没有父节点"))?;
        for b in blocks {
            let NewBlock::Paragraph { inlines, .. } = b else {
                return Err(unsupported("同段块字段的新内容只能是段落（它的 inline 会内联进去）"));
            };
            for node in emit_inlines(dom, &inlines) {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(parent),
                    before: Some(end),
                    node,
                });
            }
        }
    }
    // 旧结果：中间整段删掉，begin / end 所在段落里只删结果 run
    let mut victims: Vec<NodeId> = Vec::new();
    for n in old {
        let owner = para_of(n);
        let victim = match owner {
            Some(p) if p == end_para || p == begin_para => n,
            Some(p) => p,
            None => n,
        };
        if victim == end || victim == begin || victim == end_para || victim == begin_para {
            continue;
        }
        if !victims.contains(&victim) {
            victims.push(victim);
        }
    }
    for v in victims {
        plan.node_edits.push(NodeEdit::Delete(v));
    }
    if ctx.mark_updated_fields_dirty
        && let Some(fld) = dom.semantic_children(begin).find(|&c| dom.is(c, w(LocalName::FldChar)))
    {
        // begin run 的 `w:fldChar` 上打 `w:dirty="true"`，Word 打开时重算
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(fld),
            name: w(LocalName::Dirty),
            value: "true".into(),
        });
    }
    s.commit_plan(plan)
}
