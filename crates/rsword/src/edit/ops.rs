//! `EDIT-03` 操作实现（M1 子集）。每个操作是一个或多个 plan/commit 阶段；事务边界在
//! [`EditSession::apply`]（失败整体回滚）。这里的函数只读 DOM 与投影、产出 [`MutationPlan`]，
//! 写入全部经 [`EditSession::commit_plan`]。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::block::TextBlock;
use crate::model::inline::{Inline, Run, Segment, SegmentKind, utf16_len};
use crate::model::{SdtRefusal, refusing_sdt};
use crate::package::{PartId, RelType};
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
use super::track::{TrackSite, Tracker, err_in_deleted, site_of};
use super::{
    BlockAt, BlockPos, EditContext, EditOp, EditSession, LinkRef, NewBlock, NewInline, NewRun,
};

pub(crate) fn run(s: &mut EditSession, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
    guard_sdt(s, &op)?;
    guard_main_only(s, &op)?;
    // `spec/18` 7.3：Word 自己也不把这些记成修订（或另有机制）。照常执行，留一条
    // `REV_NOT_TRACKED`——编辑器开着修订时改页面颜色不该失败（分层决策 5）
    if ctx.track_changes.is_some()
        && let Some(what) = not_tracked_name(&op)
    {
        let part = s.main_part();
        s.record(vec![Diagnostic::pre_existing(
            part,
            None,
            DiagCode::RevNotTracked,
            format!("{what} 不产生修订（Word 也不记，或另有机制）"),
        )]);
    }
    match op {
        EditOp::InsertText { at, text, props } => insert_text(s, at, &text, props, ctx),
        EditOp::DeleteRange { from, to } => delete_range(s, from, to, ctx),
        EditOp::SetRunProps { from, to, patch } => set_run_props(s, from, to, &patch, ctx),
        EditOp::ReplaceInlines { part, para, inlines } => {
            replace_inlines(s, part, para, &inlines, ctx)
        }
        EditOp::SetParaProps { part, para, patch } => set_para_props(s, part, para, &patch, ctx),
        EditOp::ReplaceParaProps { part, para, props } => {
            replace_para_props(s, part, para, props, ctx)
        }
        EditOp::InsertRow { table, at, template } => {
            super::table_ops::insert_row(s, table, at, template, ctx)
        }
        EditOp::DeleteRow { table, at } => super::table_ops::delete_row(s, table, at, ctx),
        EditOp::InsertColumn { table, at, width } => {
            super::table_ops::insert_column(s, table, at, width, ctx)
        }
        EditOp::DeleteColumn { table, at } => super::table_ops::delete_column(s, table, at, ctx),
        EditOp::MergeCells { table, from, to } => {
            super::table_ops::merge_cells(s, table, from, to, ctx)
        }
        EditOp::SetTableProps { table, patch } => set_table_props(s, table, &patch, ctx),
        EditOp::SetRowProps { row, patch } => set_row_props(s, row, &patch, ctx),
        EditOp::SetCellProps { cell, patch } => set_cell_props(s, cell, &patch, ctx),
        EditOp::InsertBlock { at, block } => insert_block(s, at, block, ctx),
        EditOp::DeleteBlock { part, node } => delete_block(s, part, node, ctx),
        EditOp::MoveBlock { node, to } => move_block(s, node, to, ctx),
        EditOp::AddComment { from, to, comment } => add_comment(s, from, to, &comment),
        EditOp::RemoveComment { id } => remove_comment(s, &id),
        EditOp::SetCommentText { id, text, done } => set_comment_text(s, &id, &text, done),
        EditOp::SplitParagraph { at } => split_paragraph(s, at, ctx),
        EditOp::MergeWithNext { part, para } => merge_with_next(s, part, para, ctx),
        EditOp::AddBookmark { name, from, to } => add_bookmark(s, &name, from, to),
        EditOp::RemoveBookmark { name } => remove_bookmark(s, &name),
        EditOp::InsertField { at, field } => insert_field(s, at, &field, ctx),
        EditOp::SetLinkTarget { link, target } => set_link_target(s, link, &target, ctx),
        EditOp::ToggleCheckbox { field } => toggle_checkbox(s, field),
        EditOp::SetFormText { field, text } => set_form_text(s, field, &text, ctx),
        EditOp::SetFieldResultProps { field, patch } => {
            set_field_result_props(s, field, &patch, ctx)
        }
        EditOp::UpdateBlockField { field, blocks } => update_block_field(s, field, blocks, ctx),
        EditOp::SetSectionProps { sect, patch } => {
            super::section_ops::set_section_props(s, sect, &patch, ctx)
        }
        EditOp::SetHeaderFooter { sect, kind, variant, content } => {
            super::section_ops::set_header_footer(s, sect, kind, variant, content, ctx)
        }
        EditOp::LinkHeaderFooter { sect, kind, variant, part } => {
            super::section_ops::link_header_footer(s, sect, kind, variant, part)
        }
        EditOp::SetWatermark { sect, text } => super::section_ops::set_watermark(s, sect, text),
        EditOp::SetPageColor { color } => super::section_ops::set_page_color(s, color),
        EditOp::SetDocumentSettings { patch } => set_document_settings(s, &patch),
        EditOp::SetChartData { part, patch } => super::chart_ops::set_chart_data(s, part, &patch),
        EditOp::ReplacePartXml { part, xml } => {
            s.replace_part_xml(part, &xml)?;
            Ok(MutationResult::default())
        }
        EditOp::ReplacePartBytes { part, bytes } => {
            s.replace_part_bytes(part, bytes)?;
            Ok(MutationResult::default())
        }
        EditOp::ReplaceImageMedia { drawing, bytes, mime } => {
            s.replace_image_media(drawing, bytes, &mime, ctx)
        }
        EditOp::RemoveInks => s.remove_inks(),
        EditOp::InsertInk { para, ink } => s.insert_ink(para, &ink),
    }
}

/// 追踪时**不产生修订**的操作（`spec/18` 7.3 的清单）：书签、批注、复选框、页眉链接、
/// 水印、页面底色、文档设置、图表数据、part 替换、墨迹。绘图几何与样式（7.7）到时候一起加。
fn not_tracked_name(op: &EditOp) -> Option<&'static str> {
    Some(match op {
        EditOp::AddBookmark { .. } => "AddBookmark",
        EditOp::RemoveBookmark { .. } => "RemoveBookmark",
        EditOp::AddComment { .. } => "AddComment",
        EditOp::RemoveComment { .. } => "RemoveComment",
        EditOp::SetCommentText { .. } => "SetCommentText",
        EditOp::ToggleCheckbox { .. } => "ToggleCheckbox",
        EditOp::LinkHeaderFooter { .. } => "LinkHeaderFooter",
        EditOp::SetWatermark { .. } => "SetWatermark",
        EditOp::SetPageColor { .. } => "SetPageColor",
        EditOp::SetDocumentSettings { .. } => "SetDocumentSettings",
        EditOp::SetChartData { .. } => "SetChartData",
        EditOp::ReplacePartXml { .. } => "ReplacePartXml",
        EditOp::ReplacePartBytes { .. } => "ReplacePartBytes",
        EditOp::InsertInk { .. } => "InsertInk",
        EditOp::RemoveInks => "RemoveInks",
        _ => return None,
    })
}

/// `EDIT-03 SetDocumentSettings`：`word/settings.xml` 按 `PROP-06` 合并；part 不存在就按
/// `SAVE-05` 建（`evenAndOddHeaders` / 保护标志要有地方写）。
fn set_document_settings(
    s: &mut EditSession,
    patch: &crate::semantic::props::SettingsPatch,
) -> Result<MutationResult> {
    let part = s.ensure_settings_part()?;
    let dom = s.dom_in(Some(part))?;
    let root = dom.root();
    let mut plan = MutationPlan::new(part);
    plan.node_edits = crate::semantic::props::plan_apply_settings(
        dom,
        root,
        Some(root),
        patch,
        s.flavor_in(Some(part)),
    );
    s.commit_plan(plan)
}

/// 只支持主 part 的操作（书签 / 批注 / 字段：它们的索引与 id 都只对主 part 建过）。位置带别的
/// part 时明确拒绝，而不是悄悄去改主 part 的同号节点（任务 5.5）。
fn guard_main_only(s: &EditSession, op: &EditOp) -> Result<()> {
    let main = s.main_part();
    let foreign = |p: &Option<PartId>| p.is_some_and(|x| x != main);
    let bad = match op {
        EditOp::AddBookmark { from, to, .. } | EditOp::AddComment { from, to, .. } => {
            foreign(&from.part) || foreign(&to.part)
        }
        EditOp::InsertField { at, .. } => foreign(&at.part),
        _ => false,
    };
    if bad {
        return Err(Error::edit(
            DiagCode::EditUnsupported,
            "该操作暂只支持主 part（书签 / 批注 / 字段的索引只对正文建）",
        ));
    }
    Ok(())
}

/// `EDIT-03` / `MOD-08`：编辑目标落在只读（`contentLocked` / `sdtContentLocked`）或数据绑定的内容
/// 控件里 → 整体拒绝，状态不变（`EDIT-05`）。第一阶段绑定控件一律只读：显示文字只是 customXml 的
/// 缓存，改了 Word 重开会刷回去。
///
/// 目标节点连它所在的 part 一起收集（任务 5.5）：`NodeId` 只在自己 part 的 DOM 里有意义，
/// 拿页眉的节点去主 part 的树上走祖先会越界。
fn guard_sdt(s: &EditSession, op: &EditOp) -> Result<()> {
    let pos = |p: &InlinePos| (p.part, p.para);
    let block_pos = |p: &BlockPos| (p.part, p.node());
    let field = |id: FieldId| s.document().fields.get(id).map(|f| (None, f.form.head()));
    let targets: Vec<(Option<PartId>, NodeId)> = match op {
        EditOp::InsertText { at, .. }
        | EditOp::SplitParagraph { at }
        | EditOp::InsertField { at, .. } => vec![pos(at)],
        EditOp::DeleteRange { from, to }
        | EditOp::SetRunProps { from, to, .. }
        | EditOp::AddComment { from, to, .. }
        | EditOp::AddBookmark { from, to, .. } => vec![pos(from), pos(to)],
        EditOp::ReplaceInlines { part, para, .. }
        | EditOp::SetParaProps { part, para, .. }
        | EditOp::ReplaceParaProps { part, para, .. }
        | EditOp::MergeWithNext { part, para } => vec![(*part, *para)],
        EditOp::SetTableProps { table: n, .. }
        | EditOp::SetRowProps { row: n, .. }
        | EditOp::SetCellProps { cell: n, .. }
        | EditOp::InsertRow { table: n, .. }
        | EditOp::DeleteRow { table: n, .. }
        | EditOp::InsertColumn { table: n, .. }
        | EditOp::DeleteColumn { table: n, .. }
        | EditOp::MergeCells { table: n, .. } => vec![(None, *n)],
        EditOp::InsertBlock { at, .. } => vec![block_pos(at)],
        EditOp::DeleteBlock { part, node } => vec![(*part, *node)],
        EditOp::MoveBlock { node, to } => vec![(to.part, *node), block_pos(to)],
        EditOp::SetLinkTarget { link, .. } => match link {
            LinkRef::Field(id) => field(*id).into_iter().collect(),
            LinkRef::Element(node) => vec![(None, *node)],
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
        // 图表 part 与整 part 替换：目标是别的 part，不在正文树上（任务 6.6）
        EditOp::SetChartData { .. }
        | EditOp::ReplacePartXml { .. }
        | EditOp::ReplacePartBytes { .. } => Vec::new(),
        EditOp::ReplaceImageMedia { drawing, .. } => vec![(None, *drawing)],
        // 墨迹：整层删除不在内容控件里定位；追加落在锚点段落上（任务 6.8）
        EditOp::RemoveInks => Vec::new(),
        EditOp::InsertInk { para, .. } => vec![(None, *para)],
        // 节与页眉页脚：目标是 `w:sectPr` 或整个 part，不在内容控件里（任务 5.5）
        EditOp::SetSectionProps { .. }
        | EditOp::SetHeaderFooter { .. }
        | EditOp::LinkHeaderFooter { .. }
        | EditOp::SetWatermark { .. }
        | EditOp::SetPageColor { .. }
        | EditOp::SetDocumentSettings { .. } => Vec::new(),
    };
    for (part, node) in targets {
        let dom = s.dom_in(part)?;
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

/// 某个 part 里的文本段落投影（`None` = 主 part，任务 5.5）。
fn text_block(s: &EditSession, part: Option<PartId>, para: NodeId) -> Result<&TextBlock> {
    s.text_block_in(part, para).ok_or_else(|| {
        Error::edit(
            DiagCode::EditBadPosition,
            format!("节点 {} 在 part {} 里不是文本段落", para.0, s.part_or_main(part).0),
        )
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
pub(super) fn next_element_sibling(dom: &Dom, n: NodeId) -> Option<NodeId> {
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
pub(super) fn set_segment_text(dom: &Dom, seg_node: NodeId, text: &str, plan: &mut MutationPlan) {
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
            | NodeEdit::Rename { .. }
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
fn split_run(
    s: &EditSession,
    part: Option<PartId>,
    para: NodeId,
    run: &Run,
    seg: usize,
    byte: usize,
) -> MutationPlan {
    let dom = s.dom_in(part).expect("caller resolved the part");
    let mut plan = MutationPlan::new(s.part_or_main(part));
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
    at: InlinePos,
    loc: Loc,
    result: &mut MutationResult,
) -> Result<Option<(NodeId, NodeId)>> {
    split_at_maybe_deleted(s, at, loc, result, false)
}

/// 同上；`in_deleted` 为真时允许在已删除的文字里拆分（追踪路径要把已删区间也切齐）。
fn split_at_maybe_deleted(
    s: &mut EditSession,
    at: InlinePos,
    loc: Loc,
    result: &mut MutationResult,
    in_deleted: bool,
) -> Result<Option<(NodeId, NodeId)>> {
    let (inline, segment, byte) = match loc {
        Loc::Boundary { .. } => return Ok(None),
        Loc::InRun { inline, segment } => (inline, segment, 0),
        Loc::InText { inline, segment, byte } => (inline, segment, byte),
    };
    let para = at.para;
    let tb = text_block(s, at.part, para)?;
    let Inline::Run(run) = &tb.inlines[inline] else { unreachable!("InRun/InText point at runs") };
    if !in_deleted && (in_deleted_run(run) || run.segments[segment].kind == SegmentKind::DelText) {
        return Err(unsupported("位置在已删除文本内（不追踪时不能在删除区里编辑）"));
    }
    let run_node = run.node;
    let plan = split_run(s, at.part, para, run, segment, byte);
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
fn boundary_node(
    s: &EditSession,
    part: Option<PartId>,
    tb: &TextBlock,
    i: &Inline,
    side: Side,
) -> Result<NodeId> {
    if let Some(n) = i.node() {
        return Ok(n);
    }
    let Inline::Field { id, .. } = i else {
        return Err(unsupported("inline 没有对应节点"));
    };
    // 字段索引是**按 part** 的（`FLD-02`）：页眉里的字段在那个 part 自己的索引里
    let f = s
        .document()
        .fields_in(s.part_or_main(part))
        .and_then(|idx| idx.get(*id))
        .ok_or_else(|| unsupported("字段不在索引里（投影过期）"))?;
    let n = match side {
        Side::Left => f.form.tail(),
        Side::Right => f.form.head(),
    };
    // 跨段字段（`FLD-06` 的 `Block`）另一端在别的段落里，结果段落只读
    if !s.dom_in(part)?.ancestors(n).any(|a| a == tb.node) {
        return Err(unsupported("跨段字段的边界（Block 字段的结果段落只读）"));
    }
    Ok(n)
}

/// 边界插入点：`(parent, before, 继承格式的 run)`。
fn boundary_site(
    s: &EditSession,
    part: Option<PartId>,
    tb: &TextBlock,
    index: usize,
) -> Result<(NodeId, Option<NodeId>, Option<NodeId>)> {
    let dom = s.dom_in(part)?;
    let para = tb.node;
    let left = (index > 0)
        .then(|| boundary_node(s, part, tb, &tb.inlines[index - 1], Side::Left))
        .transpose()?;
    let right = (index < tb.inlines.len())
        .then(|| boundary_node(s, part, tb, &tb.inlines[index], Side::Right))
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
    let part = s.part_or_main(at.part);
    let mut diags = Vec::new();
    let text = sanitize_text(text, part, &mut diags);
    if text.is_empty() {
        return Err(Error::edit(DiagCode::EditBadText, "插入文本为空"));
    }
    let delta = utf16_len(&text) as i32;
    let tb = text_block(s, at.part, at.para)?;
    let loc = locate(tb, at.offset)?;

    // 追踪时先看位置落在什么修订包裹里（`spec/18` 7.2 的同作者规则）
    let mut tracker = Tracker::new(s.document(), ctx);
    let dom0 = s.dom_in(at.part)?;
    if let Some(t) = &tracker {
        let probe = match loc {
            Loc::Boundary { .. } => at.para,
            Loc::InRun { inline, .. } | Loc::InText { inline, .. } => {
                tb.inlines[inline].node().unwrap_or(at.para)
            }
        };
        if matches!(site_of(dom0, probe, at.para, &t.author), TrackSite::Deleted(_)) {
            return Err(err_in_deleted());
        }
    }
    // 路径 1：紧邻 / 落在 Text 段 → 直接写该 w:t 的文本节点。
    // 追踪时只有落在**本作者自己的** `w:ins` 里才能这么做（Word：自己插的可以接着改）
    let own_ins = |seg: NodeId| {
        tracker
            .as_ref()
            .is_none_or(|t| matches!(site_of(dom0, seg, at.para, &t.author), TrackSite::OwnIns(_)))
    };
    if props.is_none()
        && !has_control_chars(&text)
        && let Some((seg_node, seg_text, byte)) = direct_text_target(tb, &loc)
        && own_ins(seg_node)
    {
        let new_text = format!("{}{}{}", &seg_text[..byte], text, &seg_text[byte..]);
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        plan.diagnostics = diags;
        set_segment_text(s.dom_in(at.part)?, seg_node, &new_text, &mut plan);
        plan.offset_delta.push((at.para, at.offset, delta));
        return s.commit_plan(plan);
    }

    // 路径 2：边界插入 New run（继承左侧 rPr 或 default_run_props）
    let mut result = MutationResult::default();
    let (parent, before, inherit) = match split_at(s, at, loc, &mut result)? {
        Some((left, right)) => {
            (s.dom_in(at.part)?.parent(left).expect("run has a parent"), Some(right), Some(left))
        }
        None => {
            let Loc::Boundary { index } = loc else { unreachable!("split_at handles the rest") };
            boundary_site(s, at.part, text_block(s, at.part, at.para)?, index)?
        }
    };
    let dom = s.dom_in(at.part)?;
    let flavor = s.flavor_in(at.part);
    let mut plan = MutationPlan::new(part);
    plan.touch(at.para);
    plan.diagnostics = diags;
    // 追踪：新 run 进 `w:ins`（同作者规则见 `plan_ins_site`）
    let (run_parent, run_before) = match &mut tracker {
        None => (Target::Node(parent), before),
        Some(t) => plan_ins_site(&mut plan, dom, t, at.para, parent, before)?,
    };
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: run_parent,
        before: run_before,
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
        let dom = s.dom_in(at.part)?;
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

/// 追踪时新 run 该落在哪里（`spec/18` 7.2「同作者规则」）：
///
/// - 本作者自己的 `w:ins` 里 → 直接插，不套第二层；
/// - 别人的 `w:ins` 里 → **拆开**外层（属性克隆、换新 `w:id`），把新的 `w:ins` 夹在中间——
///   否则按作者拒绝时会把两个人的字一起撤掉；
/// - 其余 → 新建一个 `w:ins` 包住。
fn plan_ins_site(
    plan: &mut MutationPlan,
    dom: &Dom,
    t: &mut Tracker,
    para: NodeId,
    parent: NodeId,
    before: Option<NodeId>,
) -> Result<(Target, Option<NodeId>)> {
    let wrap = |plan: &mut MutationPlan, t: &mut Tracker, parent: NodeId, before| {
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before,
            node: t.marker(LocalName::Ins),
        });
        (Target::New(k), None)
    };
    match site_of(dom, parent, para, &t.author) {
        TrackSite::Deleted(_) => Err(err_in_deleted()),
        TrackSite::OwnIns(_) => Ok((Target::Node(parent), before)),
        // 位置嵌在别人 `w:ins` 内更深的容器里（超链接、smartTag …）：拆不动外层，
        // 退化成内层再套一个 `w:ins`（形态合法，接受 / 拒绝都正确，只是按作者拒绝外层会连带）
        TrackSite::OtherIns(ins) if ins != parent => Ok(wrap(plan, t, parent, before)),
        TrackSite::OtherIns(ins) => {
            let gp = dom.parent(ins).ok_or_else(|| unsupported("w:ins 没有父节点"))?;
            let kids: Vec<NodeId> = live_children(dom, ins).collect();
            let cut = before.and_then(|b| kids.iter().position(|&k| k == b));
            match cut {
                // 落在末尾：整个 `w:ins` 之后另起一个
                None => Ok(wrap(plan, t, gp, next_sibling(dom, ins))),
                // 落在开头：整个 `w:ins` 之前另起一个
                Some(0) => Ok(wrap(plan, t, gp, Some(ins))),
                Some(i) => {
                    let after = next_sibling(dom, ins);
                    let (target, _) = wrap(plan, t, gp, after);
                    // 右半：同名同属性、新 `w:id`；插在同一个 `before` 上 → 落在我们这一段之后
                    let k2 = plan.node_edits.len();
                    let right = t.clone_marker(dom, ins);
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(gp),
                        before: after,
                        node: right,
                    });
                    for &c in &kids[i..] {
                        plan.node_edits.push(NodeEdit::Move {
                            node: c,
                            parent: Target::New(k2),
                            before: None,
                        });
                    }
                    Ok((target, None))
                }
            }
        }
        TrackSite::Clean => Ok(wrap(plan, t, parent, before)),
    }
}

// ---- DeleteRange ------------------------------------------------------------------------------

fn delete_range(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    ctx: &EditContext,
) -> Result<MutationResult> {
    if from.part != to.part {
        return Err(Error::edit(DiagCode::EditBadPosition, "DeleteRange 两端不在同一个 part"));
    }
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
    let part = s.part_or_main(from.part);
    let tb = text_block(s, from.part, from.para)?;
    locate(tb, from.offset)?;
    locate(tb, to.offset)?;
    let mut plan = MutationPlan::new(part);
    plan.span.keep_orphan_comments = ctx.keep_orphan_comments;
    plan.touch(from.para);
    if a == b {
        return s.commit_plan(plan);
    }
    if ctx.track_changes.is_some() {
        return delete_range_tracked(s, from, to, ctx);
    }
    let dom = s.dom_in(from.part)?;
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

/// 追踪时的 `DeleteRange`（`spec/18` 7.2）：**内容不删**，覆盖到的每个内容项原地包进
/// `w:del`，`w:t → w:delText`、`w:instrText → w:delInstrText`。
///
/// 三条与不追踪相反的性质：坐标流长度不变（`w:delText` 照样占位）、`offset_delta` 为 0、
/// 范围标记一个都不动（`SPAN-06` 的删除规则**不**调用，见 `SpanPolicy::rewraps`）。
///
/// 同作者规则：本作者自己插的（`w:ins` 在本作者名下）真删；别人插的 → `w:del` 嵌在那个
/// `w:ins` 里（包裹插在 run 原来的位置，父节点就是 `w:ins`，天然嵌进去）；已经在 `w:del`
/// 里的不动。
fn delete_range_tracked(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.part_or_main(from.part);
    let (a, b) = (from.offset.0, to.offset.0);
    let mut result = MutationResult::default();
    // 两端先拆 run（拆分不改坐标），之后区间里的 run 要么整个在内要么整个在外
    for off in [to.offset, from.offset] {
        let loc = locate(text_block(s, from.part, from.para)?, off)?;
        split_at_maybe_deleted(s, from, loc, &mut result, true)?;
    }
    let mut t = Tracker::new(s.document(), ctx).expect("调用方已确认在追踪");
    let tb = text_block(s, from.part, from.para)?;
    let spans = inline_spans(tb);
    let dom = s.dom_in(from.part)?;
    let fields =
        s.document().fields_in(part).ok_or_else(|| unsupported("这个 part 没有字段索引"))?;
    let mut plan = MutationPlan::new(part);
    plan.span.keep_orphan_comments = ctx.keep_orphan_comments;
    plan.touch(from.para);
    let mut kept_structure = 0usize;
    // `(节点, 真删还是标删)`，文档序
    let mut items: Vec<(NodeId, bool)> = Vec::new();
    for (inline, span) in tb.inlines.iter().zip(&spans) {
        if span.end <= a || span.start >= b {
            continue;
        }
        match inline {
            Inline::Atom(atom) => items.push((atom.node, false)),
            // `FLD-07`：原子形态字段被覆盖 → begin..end 整段进 `w:del`（条目 / 结果都留着）
            Inline::Field { id, .. } => {
                for n in fields.all_nodes(*id) {
                    items.push((n, false));
                }
            }
            Inline::Run(run) => {
                // 已经在删除区里：不动（不套第二层）
                if matches!(site_of(dom, run.node, from.para, &t.author), TrackSite::Deleted(_)) {
                    continue;
                }
                let structural = run.segments.iter().any(|sg| is_structural(&sg.kind));
                if structural
                    && run.field.is_none()
                    && run.segments.iter().any(|sg| {
                        is_field_structure(&sg.kind) && fields.field_of(run.node).is_none()
                    })
                {
                    kept_structure += 1;
                    continue;
                }
                // 本作者自己插的 → 真删（Word：自己插的字删掉就没了）
                let own =
                    matches!(site_of(dom, run.node, from.para, &t.author), TrackSite::OwnIns(_));
                items.push((run.node, own));
            }
        }
    }
    // 真删掉自己插的内容之后，空掉的 `w:ins` 壳一起删（Word 不留空包裹）
    let dropped: Vec<NodeId> = items.iter().filter(|(_, d)| *d).map(|&(n, _)| n).collect();
    let mut empty_wrappers: Vec<NodeId> = Vec::new();
    for &n in &dropped {
        if let TrackSite::OwnIns(ins) = site_of(dom, n, from.para, &t.author)
            && !empty_wrappers.contains(&ins)
            && live_children(dom, ins).all(|c| dropped.contains(&c))
        {
            empty_wrappers.push(ins);
        }
    }
    for (node, drop_it) in items {
        if drop_it {
            // 壳整个删掉就不用再删它的孩子
            let covered =
                empty_wrappers.iter().any(|&ins| dom.is_ancestor_or_self(ins, node) && ins != node);
            if !covered {
                plan.node_edits.push(NodeEdit::Delete(node));
            }
            continue;
        }
        t.wrap_item(&mut plan, dom, node, LocalName::Del);
        Tracker::rename_to_deleted(&mut plan, dom, node);
    }
    for ins in empty_wrappers {
        plan.node_edits.push(NodeEdit::Delete(ins));
    }
    if kept_structure > 0 {
        plan.diagnostics.push(Diagnostic::invariant_violation(
            part,
            None,
            DiagCode::EditAnchorUnmoved,
            format!("删除范围内有 {kept_structure} 个未闭合 / 畸形字段的结构 run 原地保留"),
        ));
    }
    // `offset_delta` 不写：追踪时内容还在坐标流里
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
}

// ---- SetRunProps ------------------------------------------------------------------------------

fn set_run_props(
    s: &mut EditSession,
    from: InlinePos,
    to: InlinePos,
    patch: &RunPropsPatch,
    ctx: &EditContext,
) -> Result<MutationResult> {
    if from.part != to.part {
        return Err(Error::edit(DiagCode::EditBadPosition, "SetRunProps 两端不在同一个 part"));
    }
    if from.para != to.para {
        return Err(Error::edit(DiagCode::EditCrossParagraph, "SetRunProps 两端不在同一段落"));
    }
    let (a, b) = (from.offset.0, to.offset.0);
    if a > b {
        return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
    }
    let part = s.part_or_main(from.part);
    let mut result = MutationResult::default();
    // 阶段 A/B：先在 to、再在 from 处拆分（拆分不改变坐标）
    for off in [to.offset, from.offset] {
        let loc = locate(text_block(s, from.part, from.para)?, off)?;
        split_at(s, from, loc, &mut result)?;
    }
    // 阶段 C0（追踪）：范围内每个还没有 `rPrChange` 的 run 记下旧格式（`spec/08`）。
    // 单独一个阶段：补丁要看到已经存在的 `w:rPrChange`，才能把新元素放在它**前面**（`PROP-05`）
    if let Some(plan) = plan_run_props_change(s, from, a, b, ctx)? {
        result.absorb(s.commit_plan(plan)?);
    }
    // 阶段 C：范围内的每个非零宽 run 按 PROP-06 计划 rPr 变更
    let tb = text_block(s, from.part, from.para)?;
    let spans = inline_spans(tb);
    let dom = s.dom_in(from.part)?;
    let flavor = s.flavor_in(from.part);
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

/// 追踪时 `[a, b)` 里每个 run 的 `w:rPrChange` 旧值快照。不追踪 → `None`。
fn plan_run_props_change(
    s: &EditSession,
    from: InlinePos,
    a: u32,
    b: u32,
    ctx: &EditContext,
) -> Result<Option<MutationPlan>> {
    let Some(mut t) = Tracker::new(s.document(), ctx) else { return Ok(None) };
    let tb = text_block(s, from.part, from.para)?;
    let spans = inline_spans(tb);
    let dom = s.dom_in(from.part)?;
    let mut plan = MutationPlan::new(s.part_or_main(from.part));
    plan.touch(from.para);
    for (inline, span) in tb.inlines.iter().zip(&spans) {
        if span.start < a || span.end > b || span.start == span.end {
            continue;
        }
        let Inline::Run(run) = inline else { continue };
        snapshot_run_props(&mut plan, dom, &mut t, run.node);
    }
    Ok((!plan.is_empty()).then_some(plan))
}

/// 一个 run 的 `w:rPrChange` 旧值快照；没有 `w:rPr` 就先建一个空的（旧格式全是继承来的）。
fn snapshot_run_props(plan: &mut MutationPlan, dom: &Dom, t: &mut Tracker, run: NodeId) {
    match rpr_of(dom, run) {
        Some(rpr) => {
            t.snapshot(plan, dom, rpr, LocalName::RPrChange, LocalName::RPr, &[]);
        }
        None => {
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(run),
                before: live_children(dom, run).next(),
                node: NewElement::new(w(LocalName::RPr)),
            });
            let change =
                t.marker(LocalName::RPrChange).with_child(NewElement::new(w(LocalName::RPr)));
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: change,
            });
        }
    }
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
    part: Option<PartId>,
    para: NodeId,
    patch: &ParaPropsPatch,
    ctx: &EditContext,
) -> Result<MutationResult> {
    require_paragraph(s.dom_in(part)?, para)?;
    let mut result = MutationResult::default();
    // 追踪：先把旧值快照成 `w:pPrChange`（`SAVE-04`），再打补丁——两个阶段，补丁看到的
    // `pPr` 里已经有 `pPrChange`，`order` 会把新元素放在它前面（`PROP-05`）
    if let Some(plan) = plan_para_props_change(s, part, para, ctx)? {
        result.absorb(s.commit_plan(plan)?);
    }
    let dom = s.dom_in(part)?;
    let mut plan = MutationPlan::new(s.part_or_main(part));
    plan.touch(para);
    plan.node_edits = plan_apply_para_props(dom, para, ppr_of(dom, para), patch, s.flavor_in(part));
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
}

/// 追踪时段落属性变更的旧值快照（`w:pPrChange`）。不追踪 → `None`。
fn plan_para_props_change(
    s: &EditSession,
    part: Option<PartId>,
    para: NodeId,
    ctx: &EditContext,
) -> Result<Option<MutationPlan>> {
    let Some(mut t) = Tracker::new(s.document(), ctx) else { return Ok(None) };
    let dom = s.dom_in(part)?;
    let mut plan = MutationPlan::new(s.part_or_main(part));
    plan.touch(para);
    match ppr_of(dom, para) {
        Some(ppr) => {
            t.snapshot(
                &mut plan,
                dom,
                ppr,
                LocalName::PPrChange,
                LocalName::PPr,
                // `in_change = false`：段落标记的 `rPr` 与段落级 `sectPr` 不进快照（`para.toml`）
                &[LocalName::RPr, LocalName::SectPr],
            );
        }
        None => {
            // 没有 `pPr`：旧值全是默认，快照是一个空的 `w:pPr`
            let before = live_children(dom, para).next();
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(para),
                before,
                node: NewElement::new(w(LocalName::PPr)),
            });
            let change =
                t.marker(LocalName::PPrChange).with_child(NewElement::new(w(LocalName::PPr)));
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: change,
            });
        }
    }
    Ok((!plan.is_empty()).then_some(plan))
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
    part: Option<PartId>,
    para: NodeId,
    inlines: &[NewInline],
    ctx: &EditContext,
) -> Result<MutationResult> {
    let dom = s.dom_in(part)?;
    require_paragraph(dom, para)?;
    if ctx.track_changes.is_some() {
        return replace_inlines_tracked(s, part, para, inlines, ctx);
    }
    let mut plan = MutationPlan::new(s.part_or_main(part));
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

/// 追踪时的 `ReplaceInlines`（`spec/18` 7.2）：坐标流 diff **聚到 run 边界**——
/// 相等的 run 保留原节点（原字节），删掉的进 `w:del`，新增的进 `w:ins`。
///
/// 相等的判据是「文本与 `w:rPr` 全同」，所以相等段保留原节点之后，接受视图与不追踪做一遍
/// 完全一致。两侧的 `w:rPr` 都化成 `NewElement` 再比，比较保守（属性顺序不同判成不等），
/// 保守只让 diff 变粗、不会误判相等。
///
/// **范围标记不动**（Word：在书签里替换文字，书签还在），而不追踪那条路是按调用方的描述
/// 整体重发标记——这条差异登记在 `docs/04` §8。
fn replace_inlines_tracked(
    s: &mut EditSession,
    part: Option<PartId>,
    para: NodeId,
    inlines: &[NewInline],
    ctx: &EditContext,
) -> Result<MutationResult> {
    use super::diff::{Step, Tok, diff};
    let mut t = Tracker::new(s.document(), ctx).expect("调用方已确认在追踪");
    let tb = text_block(s, part, para)?;
    let dom = s.dom_in(part)?;
    let mut interner = crate::xml::Interner::new();
    // 旧侧：每个 inline 一个 token；只有"纯文本 run 且是段落的直接子节点"才可能相等
    let mut old_toks: Vec<Tok> = Vec::new();
    let mut old_nodes: Vec<NodeId> = Vec::new();
    for inline in &tb.inlines {
        let node = match inline.node() {
            Some(n) => n,
            // 字段原子没有单一节点：整体当一个不可匹配的 token（下面会退化成整体替换）
            None => {
                old_toks.push(Tok::Atom(format!("{:?}", std::ptr::from_ref(inline))));
                old_nodes.push(para);
                continue;
            }
        };
        let plain = match inline {
            Inline::Run(r) => {
                r.segments.iter().all(|sg| sg.kind == SegmentKind::Text)
                    && dom.parent(node) == Some(para)
            }
            _ => false,
        };
        if plain {
            let props = rpr_of(dom, node)
                .and_then(|n| NewElement::from_dom(dom, n, &mut interner))
                .map(Box::new);
            let text = match inline {
                Inline::Run(r) => r.text.clone(),
                _ => unreachable!("plain 只对 Run 成立"),
            };
            old_toks.push(Tok::Run(text, props));
        } else {
            old_toks.push(Tok::Atom(crate::xml::canonical(
                dom,
                node,
                &crate::xml::CanonOptions::default(),
            )));
        }
        old_nodes.push(node);
    }
    // 新侧：每个 `NewInline` 一个 token，同时记下它展开成的元素
    let mut new_toks: Vec<Tok> = Vec::new();
    let mut new_nodes: Vec<Vec<NewElement>> = Vec::new();
    let mut has_marker = false;
    for i in inlines {
        let mut em = Emitter::new(next_revision_id(dom));
        let mut out = Vec::new();
        em.emit(i, false, &mut out);
        match i {
            NewInline::Run(r) => {
                new_toks.push(Tok::Run(r.text.clone(), r.props.clone().map(Box::new)))
            }
            other => {
                has_marker |= matches!(other, NewInline::Marker(_));
                new_toks.push(Tok::Atom(format!("{other:?}")));
            }
        }
        new_nodes.push(out);
    }
    // 标记要按新描述重发时没法只做局部 diff：退化成"旧内容整体标删 + 新内容整体标插"
    let script = (!has_marker).then(|| diff(&old_toks, &new_toks)).flatten();
    let mut plan = MutationPlan::new(s.part_or_main(part));
    plan.touch(para);
    match script {
        Some(steps) => {
            let (mut oi, mut ni) = (0usize, 0usize);
            for step in steps {
                match step {
                    Step::Equal(n) => {
                        oi += n;
                        ni += n;
                    }
                    Step::Delete(n) => {
                        for &node in &old_nodes[oi..oi + n] {
                            t.wrap_item(&mut plan, dom, node, LocalName::Del);
                            Tracker::rename_to_deleted(&mut plan, dom, node);
                        }
                        oi += n;
                    }
                    Step::Insert(n) => {
                        let before = old_nodes.get(oi).copied();
                        let k = plan.node_edits.len();
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(para),
                            before,
                            node: t.marker(LocalName::Ins),
                        });
                        for e in new_nodes[ni..ni + n].iter().flatten() {
                            plan.node_edits.push(NodeEdit::Insert {
                                parent: Target::New(k),
                                before: None,
                                node: e.clone(),
                            });
                        }
                        ni += n;
                    }
                }
            }
        }
        None => {
            for &node in &old_nodes {
                if dom.parent(node) == Some(para) {
                    t.wrap_item(&mut plan, dom, node, LocalName::Del);
                    Tracker::rename_to_deleted(&mut plan, dom, node);
                }
            }
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(para),
                before: None,
                node: t.marker(LocalName::Ins),
            });
            for e in new_nodes.iter().flatten() {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: e.clone(),
                });
            }
        }
    }
    s.commit_plan(plan)
}

fn replace_para_props(
    s: &mut EditSession,
    part: Option<PartId>,
    para: NodeId,
    props: Option<NewElement>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    require_paragraph(s.dom_in(part)?, para)?;
    let mut result = MutationResult::default();
    // 追踪：快照先做进**旧**的 `pPr`，第二阶段再把那个 `w:pPrChange` 搬进新的 `pPr`——
    // 整份替换会把旧容器删掉，克隆源必须在它还活着的时候取
    if let Some(plan) = plan_para_props_change(s, part, para, ctx)? {
        result.absorb(s.commit_plan(plan)?);
    }
    let tracked = ctx.track_changes.is_some();
    let dom = s.dom_in(part)?;
    let mut plan = MutationPlan::new(s.part_or_main(part));
    plan.touch(para);
    let first = live_children(dom, para).next();
    let old_ppr = ppr_of(dom, para);
    let change = old_ppr
        .filter(|_| tracked)
        .and_then(|p| live_children(dom, p).find(|&c| dom.is(c, w(LocalName::PPrChange))));
    let k = plan.node_edits.len();
    match (props, change) {
        (Some(p), _) => plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: first,
            node: p,
        }),
        // 追踪时即使调用方要求"没有 pPr"，也得留一个装快照的空壳
        (None, Some(_)) => plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: first,
            node: NewElement::new(w(LocalName::PPr)),
        }),
        (None, None) => {}
    }
    if let Some(c) = change {
        plan.node_edits.push(NodeEdit::Move { node: c, parent: Target::New(k), before: None });
    }
    for c in live_children(dom, para) {
        if dom.is(c, w(LocalName::PPr)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
    }
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
}

// ---- 块级 -------------------------------------------------------------------------------------

/// `BlockPos` 的落点 → `(parent, before)`。`End(c)` 落在尾部 `w:sectPr` 之前。
fn block_site(dom: &Dom, at: BlockAt) -> Result<(NodeId, Option<NodeId>)> {
    let live_elem = |c: NodeId| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some();
    let ok = |n: NodeId| (n.0 as usize) < dom.node_count() && live_elem(n);
    match at {
        BlockAt::Start(c) => {
            if !ok(c) {
                return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
            }
            Ok((c, dom.children(c).iter().copied().find(|&k| live_elem(k))))
        }
        BlockAt::Before(n) | BlockAt::After(n) => {
            if !ok(n) {
                return Err(Error::edit(DiagCode::EditBadPosition, "锚点块无效"));
            }
            let parent = dom
                .parent(n)
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "锚点块没有父节点"))?;
            let before =
                if matches!(at, BlockAt::Before(_)) { Some(n) } else { next_sibling(dom, n) };
            Ok((parent, before))
        }
        BlockAt::End(c) => {
            if !ok(c) {
                return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
            }
            let last = dom.children(c).iter().copied().rev().find(|&k| live_elem(k));
            let before = last.filter(|&l| dom.is(l, w(LocalName::SectPr)));
            Ok((c, before))
        }
    }
}

pub(super) fn new_block_element(dom: &Dom, block: NewBlock) -> NewElement {
    match block {
        NewBlock::Xml(e) => e,
        // 每个接收 `NewBlock` 的入口都先过 `chart_ops::materialize`（建 part、换成 `Xml`）
        NewBlock::Chart { .. } | NewBlock::Image(_) => {
            unreachable!("NewBlock::Chart / Image 必须先经 chart_ops::materialize")
        }
        NewBlock::Table { rows, cols, widths, style, header } => {
            super::table_ops::new_table(rows, cols, widths, style, header)
        }
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
    ($name:ident, $patch:ty, $owner:ident, $container:ident, $change:ident, $skip:expr,
     $plan_apply:path, $what:literal) => {
        fn $name(
            s: &mut EditSession,
            node: NodeId,
            patch: &$patch,
            ctx: &EditContext,
        ) -> Result<MutationResult> {
            let dom = s.dom();
            if (node.0 as usize) >= dom.node_count()
                || dom.node(node).dirty == Dirty::Deleted
                || !dom.is(node, w(LocalName::$owner))
            {
                return Err(Error::edit(DiagCode::EditBadPosition, concat!("目标不是", $what)));
            }
            let mut result = MutationResult::default();
            // 追踪：先把旧值快照成 `w:*PrChange`（两个阶段，理由同 `SetRunProps`）
            if let Some(mut t) = Tracker::new(s.document(), ctx) {
                let (container, before) = props_site(dom, node, LocalName::$container);
                let mut plan = MutationPlan::new(s.main_part());
                if let Some(tbl) = owning_table(dom, node) {
                    plan.touch(tbl);
                }
                match container {
                    Some(c) => {
                        t.snapshot(
                            &mut plan,
                            dom,
                            c,
                            LocalName::$change,
                            LocalName::$container,
                            $skip,
                        );
                    }
                    None => {
                        // 容器不存在：旧值全是默认，快照是一个空容器
                        let k = plan.node_edits.len();
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(node),
                            before,
                            node: NewElement::new(w(LocalName::$container)),
                        });
                        let change = t
                            .marker(LocalName::$change)
                            .with_child(NewElement::new(w(LocalName::$container)));
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::New(k),
                            before: None,
                            node: change,
                        });
                    }
                }
                if !plan.is_empty() {
                    result.absorb(s.commit_plan(plan)?);
                }
            }
            let dom = s.dom();
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
            result.absorb(s.commit_plan(plan)?);
            Ok(result)
        }
    };
}

table_props_op!(
    set_table_props,
    TablePropsPatch,
    Tbl,
    TblPr,
    TblPrChange,
    &[],
    crate::semantic::props::plan_apply_table_props_at,
    "表格"
);
table_props_op!(
    set_row_props,
    RowPropsPatch,
    Tr,
    TrPr,
    TrPrChange,
    // `in_change = false`（`row.toml`）：整行插入 / 删除的标记不进快照
    &[LocalName::Ins, LocalName::Del],
    crate::semantic::props::plan_apply_row_props_at,
    "表格行"
);
table_props_op!(
    set_cell_props,
    CellPropsPatch,
    Tc,
    TcPr,
    TcPrChange,
    // `in_change = false`（`cell.toml`）
    &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge, LocalName::Headers],
    crate::semantic::props::plan_apply_cell_props_at,
    "单元格"
);

fn insert_block(
    s: &mut EditSession,
    at: BlockPos,
    block: NewBlock,
    ctx: &EditContext,
) -> Result<MutationResult> {
    // 新图表先建 part（图表 / 工作簿 / 关系），块本身换成绘图段落（任务 6.6）
    let block = super::chart_ops::materialize(s, block)?;
    let part = s.part_or_main(at.part);
    let dom = s.dom_in(at.part)?;
    let (parent, before) = block_site(dom, at.at)?;
    let is_para = matches!(block, NewBlock::Paragraph { .. });
    let opaque = matches!(block, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
    let node = new_block_element(dom, block);
    // 追踪：段落的内容进 `w:ins` 且段落标记标插入；表格每行 `trPr/w:ins`；其他整块包 `w:ins`
    let node = match &mut Tracker::new(s.document(), ctx) {
        None => node,
        Some(t) => super::track::mark_new_block_inserted(t, node, opaque),
    };
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(parent), before, node });
    // 插在格尾的非段落块（表格等）后面要补一个空段落
    if before.is_none() && !is_para {
        keep_cell_paragraph(dom, parent, None, &mut plan);
    }
    s.commit_plan(plan)
}

fn delete_block(
    s: &mut EditSession,
    part: Option<PartId>,
    node: NodeId,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let dom = s.dom_in(part)?;
    if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
        return Err(Error::edit(DiagCode::EditBadPosition, "块不存在或已删除"));
    }
    // `FLD-13`：这个块里带着某个字段的一端、另一端在块外（TOC / INDEX / BIBLIOGRAPHY 这类跨段的块字段
    // 最常见）。删掉它会把另一端留成孤儿，`FLD_STRAY_END` 是引擎自己造成的缺陷，于是**每次**保存都失败、
    // 整个会话再也存不下去。在这里拒绝，`EDIT-05` 保证状态一点没动；要删整个字段请走 `UpdateBlockField`。
    // （真实 Word 语料 `fields-toc-stale` 撞到的，`docs/09` 第三轮。）
    if let Some(idx) = s.document().fields_in(s.part_or_main(part)) {
        let inside = |n: NodeId| n == node || dom.ancestors(n).any(|a| a == node);
        if let Some(f) =
            idx.fields().iter().find(|f| inside(f.form.head()) != inside(f.form.tail()))
        {
            return Err(Error::edit(
                DiagCode::EditSplitField,
                format!(
                    "这个块只含 {:?} 字段的一端，删掉它会让另一端变成孤儿；要删整个字段请用 UpdateBlockField",
                    f.keyword()
                ),
            ));
        }
    }
    // 追踪：**块留着**（`spec/18` 7.3）
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan_delete_block_tracked(&mut plan, dom, &mut t, node);
        return s.commit_plan(plan);
    }
    let mut plan = MutationPlan::new(s.part_or_main(part));
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Delete(node));
    if let Some(parent) = dom.parent(node) {
        keep_cell_paragraph(dom, parent, Some(node), &mut plan);
    }
    s.commit_plan(plan)
}

/// 追踪时删一个块（`spec/18` 7.3）：段落 → 内容逐项 `w:del` + 段落标记 `w:del`（段落保留）；
/// 表格 → 每行 `trPr/w:del`（行保留）；其他 → 整块包一层块级 `w:del`。
pub(super) fn plan_delete_block_tracked(
    plan: &mut MutationPlan,
    dom: &Dom,
    t: &mut Tracker,
    node: NodeId,
) {
    plan.touch(node);
    if dom.is(node, w(LocalName::P)) {
        // 段落标记**先**打：`para_mark` 在没有 `pPr` 时要插在第一个内容子节点之前，
        // 而下面的包裹会把那个子节点搬进 `w:del`，`before` 就不再是段落的子节点了
        t.para_mark(plan, dom, node, LocalName::Del);
        for c in live_children(dom, node).collect::<Vec<_>>() {
            let Some(name) = dom.name(c) else { continue };
            if is_property_element(name) || crate::span::is_range_marker(name) {
                continue;
            }
            t.wrap_item(plan, dom, c, LocalName::Del);
            Tracker::rename_to_deleted(plan, dom, c);
        }
    } else if dom.is(node, w(LocalName::Tbl)) {
        plan.structure_changed = true;
        for row in live_children(dom, node).collect::<Vec<_>>() {
            if !dom.is(row, w(LocalName::Tr)) {
                continue;
            }
            let (_, before) = props_site(dom, row, LocalName::TrPr);
            t.container_mark(
                plan,
                dom,
                super::track::MarkSite {
                    owner: row,
                    container: LocalName::TrPr,
                    container_before: before,
                },
                LocalName::Del,
                crate::semantic::props::order_index_row_props,
            );
        }
    } else {
        plan.structure_changed = true;
        t.wrap_item(plan, dom, node, LocalName::Del);
    }
}

fn move_block(
    s: &mut EditSession,
    node: NodeId,
    to: BlockPos,
    ctx: &EditContext,
) -> Result<MutationResult> {
    // `docs/03` §8.2 第一阶段：追踪时不生成 `moveFrom` / `moveTo`（调用方用 Delete + Insert）
    if ctx.track_changes.is_some() {
        return Err(Error::edit(
            DiagCode::EditUnsupportedTrackedMove,
            "track_changes 开启时不支持 MoveBlock；请用 DeleteBlock + InsertBlock",
        ));
    }
    let dom = s.dom_in(to.part)?;
    let (parent, before) = block_site(dom, to.at)?;
    let mut plan = MutationPlan::new(s.part_or_main(to.part));
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
    let tb = text_block(s, at.part, para)?;
    let dom = s.dom_in(at.part)?;
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
    let tb = text_block(s, from.part, from.para)?;
    let loc_to = locate(tb, to.offset)?;
    split_at(s, to, loc_to, &mut result)?;
    let tb = text_block(s, from.part, from.para)?;
    let loc_from = locate(tb, from.offset)?;
    split_at(s, from, loc_from, &mut result)?;

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

fn split_paragraph(
    s: &mut EditSession,
    at: InlinePos,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.part_or_main(at.part);
    // 块字段与它的结果段落只在主 part 有索引（`FLD-08`）
    if at.part.is_none() {
        refuse_block_field_result(s, at.para)?;
    }
    let tb = text_block(s, at.part, at.para)?;
    let loc = locate(tb, at.offset)?;
    // 位置落在 run 内部 → 先拆 run（原子内部的位置 `locate` 已经拒了）
    let mut result = MutationResult::default();
    split_at(s, at, loc, &mut result)?;
    // `k` 是内容序列里的边界下标（`SPAN-06` 的拆分规则要用）；字段跨段的检查只在主 part
    let k = content_boundary(s, at.para, at)?;
    if at.part.is_none()
        && let Some(id) = field_across(s, at.para, k)
    {
        return Err(Error::edit(
            DiagCode::EditSplitField,
            format!("拆分会让字段 {} 跨段（FLD-08）", id.0),
        ));
    }
    let dom = s.dom_in(at.part)?;
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
    let dom = s.dom_in(at.part)?;
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

    // 阶段 3（追踪）：拆出来的**前**段的段落标记是新加的 → `pPr/rPr/w:ins`。
    // 必须在阶段 1 克隆 `pPr` 之后做，否则后段会跟着带上这个标记
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let dom = s.dom_in(at.part)?;
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        t.para_mark(&mut plan, dom, at.para, LocalName::Ins);
        if !plan.is_empty() {
            result.absorb(s.commit_plan(plan)?);
        }
    }
    result.structure_changed = true;
    Ok(result)
}

fn merge_with_next(
    s: &mut EditSession,
    at: Option<PartId>,
    para: NodeId,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.part_or_main(at);
    require_paragraph(s.dom_in(at)?, para)?;
    // 块字段的结果段落只读（`FLD-08`）；字段索引只对主 part 建了，别的 part 里没有块字段的概念
    if at.is_none() {
        refuse_block_field_result(s, para)?;
    }
    let dom = s.dom_in(at)?;
    let next = next_element_sibling(dom, para)
        .filter(|&n| dom.is(n, w(LocalName::P)))
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "下一个块不是段落"))?;
    if at.is_none() {
        refuse_block_field_result(s, next)?;
    }
    // 追踪：**不合并**，只把本段的段落标记标成删除（`spec/08`；接受后才真的合并）
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let mut plan = MutationPlan::new(part);
        plan.touch(para);
        t.para_mark(&mut plan, dom, para, LocalName::Del);
        return s.commit_plan(plan);
    }
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
    let tb = text_block(s, from.part, from.para)?;
    let loc_to = locate(tb, to.offset)?;
    split_at(s, to, loc_to, &mut result)?;
    let tb = text_block(s, from.part, from.para)?;
    let loc_from = locate(tb, from.offset)?;
    split_at(s, from, loc_from, &mut result)?;

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
    let tb = text_block(s, at.part, at.para)?;
    let loc = locate(tb, at.offset)?;
    let (parent, before, inherit) = match split_at(s, at, loc, &mut result)? {
        Some((left, right)) => {
            (s.dom().parent(left).expect("run has a parent"), Some(right), Some(left))
        }
        None => {
            let Loc::Boundary { index } = loc else { unreachable!("split_at handles the rest") };
            boundary_site(s, at.part, text_block(s, at.part, at.para)?, index)?
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
    // 追踪：整套结构 run 一起进 `w:ins`（`spec/08`）
    let (fparent, fbefore) = match &mut Tracker::new(s.document(), ctx) {
        None => (Target::Node(parent), before),
        Some(t) => plan_ins_site(&mut plan, dom, t, at.para, parent, before)?,
    };
    for node in emit_inlines(dom, std::slice::from_ref(&inline)) {
        plan.node_edits.push(NodeEdit::Insert { parent: fparent, before: fbefore, node });
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
    ctx: &EditContext,
) -> Result<MutationResult> {
    match link {
        super::LinkRef::Field(id) => set_field_link_target(s, id, target, ctx),
        // `w:hyperlink` 的 `r:id` / `w:anchor` 是元素属性，Word 不把它记成修订
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
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    if *f.keyword() != crate::span::field::Keyword::Hyperlink {
        return Err(unsupported("SetLinkTarget 只用于 HYPERLINK 字段"));
    }
    let crate::span::FieldForm::Complex { instr_nodes, .. } = &f.form else {
        return Err(unsupported("SetLinkTarget 暂不支持 w:fldSimple（改 @w:instr 属性）"));
    };
    if instr_nodes.is_empty() {
        return Err(unsupported("字段没有指令 run"));
    }
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
    // 追踪：旧指令 run 进 `w:del` 并改名 `w:delInstrText`，新指令 run 进 `w:ins`（Word 形态）
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let has_instr = instr_nodes
            .iter()
            .any(|&r| dom.semantic_children(r).any(|c| dom.is(c, w(LocalName::InstrText))));
        if !has_instr {
            return Err(unsupported("字段没有 w:instrText 可改"));
        }
        let last = *instr_nodes.last().expect("checked above");
        let parent = dom.parent(last).ok_or_else(|| unsupported("指令 run 没有父节点"))?;
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before: next_sibling(dom, last),
            node: t.marker(LocalName::Ins),
        });
        let mut run = NewElement::new(w(LocalName::R));
        if let Some(rpr) = rpr_of(dom, instr_nodes[0])
            && let Some(e) = NewElement::from_dom(dom, rpr, &mut crate::xml::Interner::new())
        {
            run.push_child(e);
        }
        run.push_child(
            NewElement::new(w(LocalName::InstrText))
                .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
                .with_text(text),
        );
        plan.node_edits.push(NodeEdit::Insert { parent: Target::New(k), before: None, node: run });
        for &r in &instr_nodes {
            t.wrap_item(&mut plan, dom, r, LocalName::Del);
            Tracker::rename_to_deleted(&mut plan, dom, r);
        }
        return s.commit_plan(plan);
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
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    let results: Vec<NodeId> = f.form.result_nodes().to_vec();
    let tracked = ctx.track_changes.is_some();
    let dom = s.dom();
    // 只有一个结果 run 且它有唯一 `w:t` → 直接改文本（最小脏化）。追踪时不走这条：
    // 结果 run 要按 `DeleteRange` + `InsertText` 的规则留下痕迹
    if !tracked
        && results.len() == 1
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
    let new_run = {
        let mut r = NewElement::new(w(LocalName::R));
        if let Some(p) = &rpr {
            r.push_child(p.clone());
        }
        for seg in text_segments(text, false) {
            r.push_child(seg);
        }
        r
    };
    match Tracker::new(s.document(), ctx) {
        None => {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before,
                node: new_run,
            });
            for old in &results {
                plan.node_edits.push(NodeEdit::Delete(*old));
            }
        }
        Some(mut t) => {
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before,
                node: t.marker(LocalName::Ins),
            });
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: new_run,
            });
            for &old in &results {
                t.wrap_item(&mut plan, dom, old, LocalName::Del);
                Tracker::rename_to_deleted(&mut plan, dom, old);
            }
        }
    }
    s.commit_plan(plan)
}

/// `FLD-07`：字段结果 run 的格式（原子字段不能按 `SetRunProps` 那样定位，所以单列一个操作）。
fn set_field_result_props(
    s: &mut EditSession,
    id: crate::span::FieldId,
    patch: &RunPropsPatch,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let part = s.main_part();
    let f = field_of(s, id)?;
    let results: Vec<NodeId> = f.form.result_nodes().to_vec();
    let head = f.form.head();
    if results.is_empty() {
        return Err(unsupported("字段没有结果区可改格式"));
    }
    let mut result = MutationResult::default();
    // 追踪：与 `SetRunProps` 同规则，先快照 `w:rPrChange`（两个阶段，理由同上）
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let dom = s.dom();
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(head).find(|&a| dom.is(a, w(LocalName::P))) {
            plan.touch(p);
        }
        for r in &results {
            if !dom.is(*r, w(LocalName::R)) {
                continue;
            }
            snapshot_run_props(&mut plan, dom, &mut t, *r);
        }
        if !plan.is_empty() {
            result.absorb(s.commit_plan(plan)?);
        }
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
    result.absorb(s.commit_plan(plan)?);
    Ok(result)
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
    let blocks = super::chart_ops::materialize_all(s, blocks)?;
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
    let mut tracker = Tracker::new(s.document(), ctx);
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    plan.touch(begin_para);
    plan.touch(end_para);
    if cross {
        // 段落级：新块插在 end 所在段落之前
        let parent = dom.parent(end_para).ok_or_else(|| unsupported("段落没有父节点"))?;
        for b in blocks {
            let opaque = matches!(b, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
            let node = new_block_element(dom, b);
            // 追踪：新结果块按 `InsertBlock` 规则、旧结果块按 `DeleteBlock` 规则（`spec/18` 7.3）
            let node = match &mut tracker {
                Some(t) => super::track::mark_new_block_inserted(t, node, opaque),
                None => node,
            };
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before: Some(end_para),
                node,
            });
        }
    } else {
        // 同段：新块的 inline 直接插在 end run 之前（段落里不能塞段落）
        let parent = dom.parent(end).ok_or_else(|| unsupported("字段 end 没有父节点"))?;
        for b in blocks {
            let NewBlock::Paragraph { inlines, .. } = b else {
                return Err(unsupported("同段块字段的新内容只能是段落（它的 inline 会内联进去）"));
            };
            let (iparent, ibefore) = match &mut tracker {
                None => (Target::Node(parent), Some(end)),
                Some(t) => {
                    let k = plan.node_edits.len();
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(parent),
                        before: Some(end),
                        node: t.marker(LocalName::Ins),
                    });
                    (Target::New(k), None)
                }
            };
            for node in emit_inlines(dom, &inlines) {
                plan.node_edits.push(NodeEdit::Insert { parent: iparent, before: ibefore, node });
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
        match &mut tracker {
            Some(t) if dom.is(v, w(LocalName::P)) || dom.is(v, w(LocalName::Tbl)) => {
                plan_delete_block_tracked(&mut plan, dom, t, v);
            }
            Some(t) => {
                t.wrap_item(&mut plan, dom, v, LocalName::Del);
                Tracker::rename_to_deleted(&mut plan, dom, v);
            }
            None => plan.node_edits.push(NodeEdit::Delete(v)),
        }
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
