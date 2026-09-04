//! `EDIT-03` 操作实现（M1 子集）。每个操作是一个或多个 plan/commit 阶段；事务边界在
//! [`EditSession::apply`]（失败整体回滚）。这里的函数只读 DOM 与投影、产出 [`MutationPlan`]，
//! 写入全部经 [`EditSession::commit_plan`]。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::block::TextBlock;
use crate::model::inline::{Inline, Run, Segment, SegmentKind, utf16_len};
use crate::semantic::props::{
    ParaPropsPatch, RunPropsPatch, emit_run_props, plan_apply_para_props, plan_apply_run_props,
};
use crate::span::{Affinity, Anchor, FlowId, RangeClass, RangeKind, RangeSpan, SpanId, SpanOrigin};
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NodeEdit, NodeId, NodeKind, NsId, QName, Target,
};

use super::inline::{Emitter, has_control_chars, sanitize_text, text_segments};
use super::plan::{MutationPlan, MutationResult};
use super::pos::{InlinePos, Loc, inline_spans, locate, utf16_to_byte};
use super::{BlockPos, EditContext, EditOp, EditSession, NewBlock, NewInline};

pub(crate) fn run(s: &mut EditSession, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
    match op {
        EditOp::InsertText { at, text, props } => insert_text(s, at, &text, props, ctx),
        EditOp::DeleteRange { from, to } => delete_range(s, from, to, ctx),
        EditOp::SetRunProps { from, to, patch } => set_run_props(s, from, to, &patch),
        EditOp::ReplaceInlines { para, inlines } => replace_inlines(s, para, &inlines),
        EditOp::SetParaProps { para, patch } => set_para_props(s, para, &patch),
        EditOp::ReplaceParaProps { para, props } => replace_para_props(s, para, props),
        EditOp::InsertBlock { at, block } => insert_block(s, at, block),
        EditOp::DeleteBlock { node } => delete_block(s, node),
        EditOp::MoveBlock { node, to } => move_block(s, node, to),
        EditOp::AddComment { from, to, comment } => add_comment(s, from, to, &comment),
        EditOp::RemoveComment { id } => remove_comment(s, &id),
        EditOp::SetCommentText { id, text, done } => set_comment_text(s, &id, &text, done),
    }
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

fn insert_block(s: &mut EditSession, at: BlockPos, block: NewBlock) -> Result<MutationResult> {
    let dom = s.dom();
    let (parent, before) = block_site(dom, at)?;
    let node = new_block_element(dom, block);
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(parent), before, node });
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
    s.commit_plan(plan)
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

/// 批注条目的段落：首段带 `w:annotationRef`，每段一个 `w14:paraId`。
fn comment_paragraphs(text: &str, para_id: &str, rpr: Option<&NewElement>) -> Vec<NewElement> {
    let w14 = |l: LocalName| QName::new(NsId::W14, l);
    let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.split('\n').collect() };
    let last = lines.len() - 1;
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut p = NewElement::new(w(LocalName::P));
            // `commentsExtended` 按最后一段的 paraId 关联（`MOD-10`）
            if i == last {
                p.push_attr(w14(LocalName::ParaId), para_id.to_string());
            }
            if i == 0 {
                let mark_rpr = NewElement::new(w(LocalName::RPr)).with_child(
                    NewElement::new(w(LocalName::RStyle))
                        .with_attr(w(LocalName::Val), "CommentReference"),
                );
                p.push_child(
                    NewElement::new(w(LocalName::R))
                        .with_child(mark_rpr)
                        .with_child(NewElement::new(w(LocalName::AnnotationRef))),
                );
            }
            let mut r = NewElement::new(w(LocalName::R));
            if let Some(rpr) = rpr {
                r.push_child(rpr.clone());
            }
            r.push_child(NewElement::new(w(LocalName::T)).with_text(line));
            p.push_child(r);
            p
        })
        .collect()
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
    for p in comment_paragraphs(&c.text, &para_id, None) {
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
    for p in comment_paragraphs(text, &para_id, first_rpr.as_ref()) {
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
