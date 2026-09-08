//! 接受 / 拒绝修订（`EDIT-03` 的那张表，`spec/18` 7.4）。
//!
//! **接受 / 拒绝是普通 `EditOp`**（分层决策 4）：走 plan / validate / commit，`AcceptAll` 是**一个
//! 事务**（任一步失败整体回滚，`EDIT-05`），事务内部按修订逐条提交——顺序是文档序、
//! **先内层后外层**（[`crate::model::RevisionIndex::iter_inner_first`]：`w:ins` 里套 `w:del` 时
//! 先处理 `del`，内容先消失再解包空壳）。
//!
//! 拒绝 `*PrChange` 用快照子元素的**整体克隆**，不走属性补丁：快照里可能有本引擎还没建模的
//! 子元素，补丁只会还原建模过的那部分（typed 的 `old` 只服务模型与 compat）。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::{RevKind, RevOwner, RevisionId};
use crate::package::PartId;
use crate::span::{RangeKind, SpanId};
use crate::xml::{Dirty, Dom, LocalName, NodeEdit, NodeId, NsId, QName, Target};

use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;
use super::track::Tracker;

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

fn live_children(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    dom.children(n).iter().copied().filter(|&c| dom.node(c).dirty != Dirty::Deleted)
}

/// 一条修订在某个方向上要做的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// 解包：子节点搬到包裹原来的位置，包裹本身删掉。
    Unwrap,
    /// 解包，并把 `w:delText` / `w:delInstrText` 改回 `w:t` / `w:instrText`。
    UnwrapLive,
    /// 整棵子树删掉。
    Drop,
    /// 只删标记元素本身（段落标记的 `w:ins`、`*PrChange` 的接受方向）。
    DropMark,
    /// 删标记，再把这一段与下一段合并（无追踪的 `MergeWithNext`）。
    Merge,
    /// 用 `*Change` 里的快照还原容器：`(容器名, 不动的字段)`。
    Restore(LocalName, &'static [LocalName]),
    /// 删这个格并收缩网格。
    DropCell,
    /// 这个方向不支持。
    Unsupported,
}

/// `RevKind` → （接受动作, 拒绝动作）。一张表同时给出两个方向，
/// `tests/revisions.rs` 的用例列表按同一张表写。
macro_rules! accept_reject {
    ($($kind:ident => $accept:expr, $reject:expr;)+) => {
        fn actions(kind: RevKind) -> (Act, Act) {
            match kind {
                $(RevKind::$kind => ($accept, $reject),)+
            }
        }
    };
}

// `in_change = false` 的字段不在快照里，还原时不能动它们
const PPR_KEEP: &[LocalName] = &[LocalName::RPr, LocalName::SectPr];
const SECT_KEEP: &[LocalName] = &[LocalName::HeaderReference, LocalName::FooterReference];
const ROW_KEEP: &[LocalName] = &[LocalName::Ins, LocalName::Del];
const CELL_KEEP: &[LocalName] =
    &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge, LocalName::Headers];

accept_reject! {
    // 内容包裹（`owner` 是 `Row` 时另有一套，见 `row_actions`）
    Insert        => Act::Unwrap,   Act::Drop;
    Delete        => Act::Drop,     Act::UnwrapLive;
    MoveFrom      => Act::Drop,     Act::UnwrapLive;
    MoveTo        => Act::Unwrap,   Act::Drop;
    RunInsert     => Act::Unwrap,   Act::Drop;
    RunDelete     => Act::Drop,     Act::UnwrapLive;
    RunMoveFrom   => Act::Drop,     Act::UnwrapLive;
    RunMoveTo     => Act::Unwrap,   Act::Drop;
    // 段落标记
    ParaMarkInsert   => Act::DropMark, Act::Merge;
    ParaMarkDelete   => Act::Merge,    Act::DropMark;
    ParaMarkMoveFrom => Act::Merge,    Act::DropMark;
    ParaMarkMoveTo   => Act::DropMark, Act::Merge;
    // 属性快照
    RunPropsChange     => Act::DropMark, Act::Restore(LocalName::RPr, &[]);
    ParaPropsChange    => Act::DropMark, Act::Restore(LocalName::PPr, PPR_KEEP);
    TablePropsChange   => Act::DropMark, Act::Restore(LocalName::TblPr, &[]);
    TablePropsExChange => Act::DropMark, Act::Restore(LocalName::TblPrEx, &[]);
    SectPropsChange    => Act::DropMark, Act::Restore(LocalName::SectPr, SECT_KEEP);
    TableGridChange    => Act::DropMark, Act::Restore(LocalName::TblGrid, &[]);
    RowPropsChange     => Act::DropMark, Act::Restore(LocalName::TrPr, ROW_KEEP);
    CellPropsChange    => Act::DropMark, Act::Restore(LocalName::TcPr, CELL_KEEP);
    // `w:numberingChange` 只有 `w:original` 属性、没有内层容器（§17.13.5.14，已废弃），
    // 拒绝无从还原：两个方向都只删标记，登记在 `docs/04` §8
    NumberingChange    => Act::DropMark, Act::DropMark;
    // 单元格
    CellInsert => Act::DropMark, Act::DropCell;
    CellDelete => Act::DropCell, Act::DropMark;
    // `vMergeOrig` 的还原形态待真实 Word 校准（`spec/18`「不在 M7」）。顺带一条实测：
    // Word 的「拒绝所有修订」本来也**不**撤销单元格合并
    CellMerge  => Act::DropMark, Act::Unsupported;
}

/// `trPr/w:ins|w:del`：标记在行属性里，动的是**整行**。
fn row_actions(kind: RevKind) -> Option<(Act, Act)> {
    match kind {
        RevKind::Insert => Some((Act::DropMark, Act::Drop)),
        RevKind::Delete => Some((Act::Drop, Act::DropMark)),
        _ => None,
    }
}

/// 空掉之后可以整个去掉的属性容器（`w:tblGrid` / `w:sectPr` 不在其中：它们必须存在）。
///
/// 真实 Word 的对照件是这条的出处：`fixtures/revisions/table-and-move/tracked.docx` 有 6 个
/// `w:tblPrEx`，`accepted.docx` 与 `rejected.docx` **一个都没有**——那些行属性覆盖是跟踪操作
/// 的产物，修订一解决 Word 就把整个容器丢掉。
const DROPPABLE_EMPTY: &[LocalName] = &[
    LocalName::RPr,
    LocalName::PPr,
    LocalName::TrPr,
    LocalName::TcPr,
    LocalName::TblPr,
    LocalName::TblPrEx,
    LocalName::NumPr,
];

/// 要处理的一条修订（把索引里的数据抄出来：处理过程中索引会重建）。
#[derive(Debug, Clone)]
struct Job {
    part: PartId,
    node: NodeId,
    kind: RevKind,
    owner: RevOwner,
    move_name: Option<String>,
    pair: Option<(PartId, NodeId)>,
}

/// `AcceptRevision` / `RejectRevision`。
pub(crate) fn one(s: &mut EditSession, rev: RevisionId, accept: bool) -> Result<MutationResult> {
    let e = s.document().revisions.get(rev).ok_or_else(|| {
        Error::edit(DiagCode::EditPlanInvalid, format!("修订 {} 不在索引里", rev.0))
    })?;
    // 一个字段、一个表格的列改动都要整个一起解决（见 `field_group` / `table_group`）
    let mut group = field_group(s, e);
    for id in table_group(s, e) {
        if !group.contains(&id) {
            group.push(id);
        }
    }
    group.sort_by_key(|&id| {
        // 网格快照最后还原：它整块替换 `w:tblGrid`，前面掉格时删掉的 `w:gridCol` 会被它盖掉
        let last = s
            .document()
            .revisions
            .get(id)
            .is_some_and(|x| x.kind == crate::model::RevKind::TableGridChange);
        (last, id.0)
    });
    let jobs: Vec<Job> = group
        .into_iter()
        .filter_map(|id| s.document().revisions.get(id))
        .map(|e| job_of(s, e))
        .collect();
    apply_jobs(s, jobs, accept)
}

/// 与这条修订同属**一次列改动**的那几条：一张表的 `tblGridChange` 与它的 `cellIns` / `cellDel`
/// 是同一次操作的两面，单独解决一面就会让网格与行里的格数对不上（`SAVE_TABLE_GRID`）。
fn table_group(s: &EditSession, e: &crate::model::RevisionEntry) -> Vec<RevisionId> {
    use crate::model::RevKind;
    if !matches!(e.kind, RevKind::TableGridChange | RevKind::CellInsert | RevKind::CellDelete) {
        return Vec::new();
    }
    let Ok(dom) = s.dom_in(Some(e.part)) else { return Vec::new() };
    let table_of = |n: NodeId| dom.ancestors(n).find(|&a| dom.is(a, w(LocalName::Tbl)));
    let Some(tbl) = table_of(e.meta.node) else { return Vec::new() };
    s.document()
        .revisions
        .entries()
        .iter()
        .filter(|x| x.part == e.part && x.author() == e.author())
        .filter(|x| {
            matches!(x.kind, RevKind::TableGridChange | RevKind::CellInsert | RevKind::CellDelete)
        })
        .filter(|x| table_of(x.meta.node) == Some(tbl))
        .map(|x| x.id)
        .collect()
}

/// 与这条修订同属一个字段的那几条（含它自己），按索引序。
///
/// 追踪删除时**每个内容项各包一层** `w:del`（7.2：一个包裹顶替一个内容项，锚点才不动），
/// 于是一个字段的 begin / 指令 / separate / 结果 / end 分在好几条修订里。单独接受其中一条就
/// 丢了半个字段，另一半成孤儿——`FLD-13` 从此每次保存都失败（`TEST-07` 用两步就抓到了：
/// 追踪删一段盖住 `REF` 字段 → 接受其中一条）。
fn field_group(s: &EditSession, e: &crate::model::RevisionEntry) -> Vec<RevisionId> {
    let Some(fields) = s.document().fields_in(e.part) else { return vec![e.id] };
    let Ok(dom) = s.dom_in(Some(e.part)) else { return vec![e.id] };
    let ids_under = |node: NodeId| -> Vec<crate::span::FieldId> {
        let mut out: Vec<crate::span::FieldId> = Vec::new();
        for n in dom.descendants(node) {
            if dom.node(n).dirty == Dirty::Deleted {
                continue;
            }
            if let Some(f) = fields.field_of(n)
                && !out.contains(&f.id)
            {
                out.push(f.id);
            }
        }
        out
    };
    let mine = ids_under(e.meta.node);
    if mine.is_empty() {
        return vec![e.id];
    }
    s.document()
        .revisions
        .entries()
        .iter()
        .filter(|x| x.part == e.part && x.kind == e.kind && x.author() == e.author())
        .filter(|x| x.id == e.id || ids_under(x.meta.node).iter().any(|f| mine.contains(f)))
        .map(|x| x.id)
        .collect()
}

/// `AcceptAll` / `RejectAll`（`author` 给定时只处理那个作者的）。
pub(crate) fn all(
    s: &mut EditSession,
    author: Option<&str>,
    accept: bool,
) -> Result<MutationResult> {
    let ordered = s.document().revisions.iter_inner_first();
    let picked: Vec<Job> = ordered
        .into_iter()
        .filter(|e| author.is_none_or(|a| e.author() == Some(a)))
        .map(|e| job_of(s, e))
        .collect();
    // **段落标记放最后，而且倒着来**。放最后：解决它可能是"与下一段合并"，那要等这一段的
    // 内容先处理完（接受一个被搬走的段落 = 内容消失 + 标记合并 = 整段没了；反过来做会留下空段）。
    // 倒着来：连续几段都被删时，从后往前解决，每一段看到的"下一段"都已经定型了——顺着来的话
    // 第一段会先与还没消失的第二段合并，第三段就并不进来了。
    // 内容那一组保持 `iter_inner_first` 的次序，`w:ins` 套 `w:del` 的内外顺序不受影响
    let (mut marks, content): (Vec<Job>, Vec<Job>) =
        picked.into_iter().partition(|j| j.kind.is_para_mark());
    marks.reverse();
    // 网格标记排最后：先让掉格阶段同步删除当前 gridCol，再决定是否仍需恢复属性快照。
    // 反过来先缩网格再掉格，会把同一列删两遍。
    let (grid, content): (Vec<Job>, Vec<Job>) =
        content.into_iter().partition(|j| j.kind == crate::model::RevKind::TableGridChange);
    let jobs: Vec<Job> = content.into_iter().chain(grid).chain(marks).collect();
    apply_jobs(s, jobs, accept)
}

fn job_of(s: &EditSession, e: &crate::model::RevisionEntry) -> Job {
    Job {
        part: e.part,
        node: e.node(),
        kind: e.kind,
        owner: e.owner,
        move_name: e.move_name.clone(),
        pair: e.pair.and_then(|p| s.document().revisions.get(p)).map(|t| (t.part, t.node())),
    }
}

/// 逐条处理。整批在**一个事务**里（调用方 `EditSession::apply` 已经开了），任一步 `Err`
/// 就把每个碰过的 part 恢复到写前镜像。
fn apply_jobs(s: &mut EditSession, jobs: Vec<Job>, accept: bool) -> Result<MutationResult> {
    // 记录本事务开始时的网格节点。掉格阶段已删掉对应 gridCol 时，不能再用旧快照
    // 覆盖它：存活列可能包含后来未追踪插入的列及其宽度。
    let mut grids = Vec::new();
    if !accept {
        for job in &jobs {
            if job.kind != RevKind::TableGridChange || !alive(s, job.part, job.node)? {
                continue;
            }
            let dom = s.dom_in(Some(job.part))?;
            if let Some(grid) = dom.parent(job.node)
                && let Some(table) = dom.ancestors(grid).find(|&n| dom.is(n, w(LocalName::Tbl)))
            {
                let cols: Vec<_> = live_children(dom, grid)
                    .filter(|&n| dom.is(n, w(LocalName::GridCol)))
                    .collect();
                grids.push((job.part, job.node, table, cols));
            }
        }
    }
    let mut result = MutationResult::default();
    let mut done: Vec<(PartId, NodeId)> = Vec::new();
    for job in jobs {
        if done.contains(&(job.part, job.node)) {
            continue;
        }
        // 前面的步骤可能已经把它连着的子树删了（`w:ins` 里套 `w:del`，拒绝外层时内层随之消失）
        if !alive(s, job.part, job.node)? {
            continue;
        }
        done.push((job.part, job.node));
        // 搬移的两半一起处理：孪生的方向相反
        let twin = job.pair.filter(|&(p, n)| !done.contains(&(p, n)));
        if let Some((tp, tn)) = twin {
            done.push((tp, tn));
        }
        let mut dead_spans: Vec<(PartId, SpanId)> = Vec::new();
        let reconciled_grid = grids
            .iter()
            .find(|(p, n, _, _)| (*p, *n) == (job.part, job.node))
            .is_some_and(|(_, _, _, cols)| {
                let dom = s.dom_in(Some(job.part)).expect("已验证 part");
                cols.iter().any(|&n| dom.node(n).dirty == Dirty::Deleted)
            });
        for plan in plan_job(s, &job, accept, reconciled_grid, &mut dead_spans)? {
            result.absorb(s.commit_plan(plan)?);
        }
        if let Some((tp, tn)) = twin
            && alive(s, tp, tn)?
        {
            let kind = s.document().revisions.by_node(tp, tn).map(|e| e.kind).unwrap_or(job.kind);
            let twin_job = Job { part: tp, node: tn, kind, ..job.clone() };
            for plan in plan_job(s, &twin_job, accept, false, &mut dead_spans)? {
                result.absorb(s.commit_plan(plan)?);
            }
        }
        // 范围本身要从索引里摘掉，否则 `SPAN-09` 会在保存时按索引把标记重新物化出来
        for (part, span) in dead_spans {
            s.drop_span(part, span);
        }
    }
    // 外层事务提交前检查最终几何；包含 Dirty::New 的行，不依赖保存校验的跳过规则。
    // 任一失败由 EditSession 的完整检查点回滚，非法中间态不会发布给调用方。
    for (part, _, table, _) in grids {
        if !alive(s, part, table)? {
            continue;
        }
        let mut pending = s.document().blocks_of_part(part).unwrap_or_default();
        let mut found = false;
        while let Some(block) = pending.pop() {
            if let crate::model::Block::Table(t) = block {
                if t.node == table {
                    super::table_ops::geometry(t).require_consistent()?;
                    found = true;
                    break;
                }
                pending.extend(t.rows.iter().flat_map(|r| &r.cells).flat_map(|c| &c.blocks));
            }
            for (blocks, here) in crate::model::table::box_flows(block) {
                if here.is_none_or(|p| p == part) {
                    pending.extend(blocks);
                }
            }
        }
        if !found {
            return Err(Error::edit(
                DiagCode::EditTableGridInconsistent,
                "无法验证还原后的表格网格",
            ));
        }
    }
    Ok(result)
}

fn alive(s: &EditSession, part: PartId, node: NodeId) -> Result<bool> {
    let dom = s.dom_in(Some(part))?;
    Ok((node.0 as usize) < dom.node_count()
        && dom.node(node).dirty != Dirty::Deleted
        && dom.ancestors(node).all(|a| dom.node(a).dirty != Dirty::Deleted))
}

/// 一条修订的计划（合并段落要两个阶段：先删标记再合并）。
fn plan_job(
    s: &mut EditSession,
    job: &Job,
    accept: bool,
    reconciled_grid: bool,
    dead_spans: &mut Vec<(PartId, SpanId)>,
) -> Result<Vec<MutationPlan>> {
    let (a, r) = match (row_actions(job.kind), job.owner) {
        (Some(row), RevOwner::Row(_)) => row,
        _ => actions(job.kind),
    };
    let act = if accept { a } else { r };
    // 行级的 `Drop` 动的是整行，不是标记本身
    let target = match (act, job.owner) {
        (Act::Drop, RevOwner::Row(row)) => row,
        _ => job.node,
    };
    let part = Some(job.part);
    let dom = s.dom_in(part)?;
    let mut plan = MutationPlan::new(job.part);
    match dom.ancestors(job.node).find(|&x| dom.is(x, w(LocalName::P))) {
        Some(p) => plan.touch(p),
        // 不在段落里的修订（body 级 `w:sectPr` 的 `sectPrChange`、行 / 格标记…）：
        // 没有块可以刷，整体重建。解决修订不是热路径，稳比快要紧（`TEST-07` 抓到的）
        None => plan.structure_changed = true,
    }
    match act {
        Act::Unsupported => {
            return Err(Error::edit(
                DiagCode::EditUnsupported,
                format!("{} 的{}方向暂不支持", job.kind, if accept { "接受" } else { "拒绝" }),
            ));
        }
        Act::Unwrap | Act::UnwrapLive => {
            if act == Act::UnwrapLive {
                Tracker::rename_to_live(&mut plan, dom, target);
            }
            unwrap(&mut plan, dom, target);
        }
        Act::Drop => {
            plan.structure_changed = true;
            plan.node_edits.push(NodeEdit::Delete(target));
            if let Some(parent) = dom.parent(target) {
                super::ops::keep_cell_paragraph(dom, parent, Some(target), &mut plan);
                // 表格的最后一行也走了 → 整张表跟着走（没有行的 `w:tbl` 不合法）
                if dom.is(target, w(LocalName::Tr))
                    && live_children(dom, parent)
                        .filter(|&c| dom.is(c, w(LocalName::Tr)))
                        .all(|c| c == target)
                {
                    plan.node_edits.push(NodeEdit::Delete(parent));
                }
            }
        }
        Act::DropMark => {
            plan.node_edits.push(NodeEdit::Delete(job.node));
            drop_empty_containers(&mut plan, dom, job.node);
        }
        Act::Restore(container, keep) => {
            let c = dom
                .parent(job.node)
                .filter(|&c| dom.is(c, w(container)))
                .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "快照不在预期的容器里"))?;
            if container == LocalName::TblGrid && reconciled_grid {
                // plan_drop_cells 已按当前列位置同时删除格与 gridCol；只摘掉历史标记。
                plan.node_edits.push(NodeEdit::Delete(job.node));
            } else {
                restore(&mut plan, dom, c, job.node, container, keep);
            }
        }
        Act::Merge => {
            plan.node_edits.push(NodeEdit::Delete(job.node));
            drop_empty_containers(&mut plan, dom, job.node);
            let para = dom
                .ancestors(job.node)
                .find(|&x| dom.is(x, w(LocalName::P)))
                .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "段落标记不在段落里"))?;
            // 内容也没剩下 → **整段消失**，而不是与下一段合并。
            //
            // 这两件事不一样：合并保留**本段**的属性（下一段的样式会丢），而"这一段整个被
            // 删掉 / 整个是插进来的"应该让下一段原样留着。段落标记排在内容之后处理
            // （见 `all`），所以这里看到的就是内容解决之后的样子。
            let empty = live_children(dom, para).all(|c| {
                dom.name(c).is_none_or(|q| {
                    crate::span::is_property_element(q) || crate::span::is_range_marker(q)
                })
            });
            if empty {
                plan.structure_changed = true;
                plan.node_edits.push(NodeEdit::Delete(para));
                if let Some(parent) = dom.parent(para) {
                    super::ops::keep_cell_paragraph(dom, parent, Some(para), &mut plan);
                }
                return Ok(vec![plan]);
            }
            let merge = super::ops::plan_merge_with_next(s, part, para)?;
            return Ok(match merge {
                Some(m) => vec![plan, m],
                // 后面没有同容器的段落：只把标记去掉（Word 也只能这样）
                None => {
                    plan.diagnostics.push(Diagnostic::pre_existing(
                        job.part,
                        None,
                        DiagCode::EditUnsupported,
                        "段落标记的修订没有可合并的下一段，只去掉标记",
                    ));
                    vec![plan]
                }
            });
        }
        Act::DropCell => return plan_drop_cells(s, job),
    }
    // 搬移的范围随内容一起消失（要 `&mut` 拿范围索引，所以放在 `dom` 的借用之后）
    drop_move_range(s, &mut plan, job, dead_spans)?;
    Ok(vec![plan])
}

/// 解包：子节点按原顺序搬到包裹的位置，包裹删掉。内容序列从 1 项变成 N 项，
/// `SPAN-06` 的通用推导正好算得出（`Move` 插入 N、`Delete` 移除 1）。
fn unwrap(plan: &mut MutationPlan, dom: &Dom, wrapper: NodeId) {
    let Some(parent) = dom.parent(wrapper) else { return };
    plan.structure_changed = true;
    for c in live_children(dom, wrapper).collect::<Vec<_>>() {
        plan.node_edits.push(NodeEdit::Move {
            node: c,
            parent: Target::Node(parent),
            before: Some(wrapper),
        });
    }
    plan.node_edits.push(NodeEdit::Delete(wrapper));
}

/// 删掉 `marker` 之后空掉的属性容器一路往上也删（`w:rPr` → `w:pPr`、`w:tblPrEx` …）。
/// 那正是真实 Word 的形态，见 [`DROPPABLE_EMPTY`]。
fn drop_empty_containers(plan: &mut MutationPlan, dom: &Dom, marker: NodeId) {
    let mut gone = vec![marker];
    let mut cur = dom.parent(marker);
    while let Some(c) = cur {
        let Some(name) = dom.name(c) else { break };
        if name.ns != NsId::W || !DROPPABLE_EMPTY.contains(&name.local) {
            break;
        }
        if live_children(dom, c).any(|x| !gone.contains(&x)) {
            break;
        }
        plan.node_edits.push(NodeEdit::Delete(c));
        gone.push(c);
        cur = dom.parent(c);
    }
}

/// 搬移的范围（`w:moveFromRangeStart` / `End` / `moveToRange*`）随内容一起消失：标记节点删掉，
/// **范围本身也要从索引里摘掉**——否则 `SPAN-09` 会在保存时按索引把标记重新物化出来。
fn drop_move_range(
    s: &mut EditSession,
    plan: &mut MutationPlan,
    job: &Job,
    dead_spans: &mut Vec<(PartId, SpanId)>,
) -> Result<()> {
    if !job.kind.is_move() {
        return Ok(());
    }
    let Some(name) = job.move_name.clone() else { return Ok(()) };
    let mut victims: Vec<NodeId> = Vec::new();
    {
        let index = s.spans_of(job.part)?;
        for sp in index.live() {
            let same = match &sp.kind {
                RangeKind::MoveFrom { name: n, .. } | RangeKind::MoveTo { name: n, .. } => {
                    *n == name
                }
                _ => false,
            };
            if !same || dead_spans.iter().any(|&(p, id)| p == job.part && id == sp.id) {
                continue;
            }
            dead_spans.push((job.part, sp.id));
            victims.extend(sp.start.and_then(|a| a.marker));
            victims.extend(sp.end.and_then(|a| a.marker));
        }
    }
    let dom = s.dom_in(Some(job.part))?;
    for m in victims {
        if dom.node(m).dirty != Dirty::Deleted {
            plan.node_edits.push(NodeEdit::Delete(m));
        }
    }
    Ok(())
}

/// 用 `*Change` 里的快照还原容器：容器现有子元素（除 `keep` 与 `*Change` 自己）全删，
/// 快照内层容器的子元素**整体克隆**进来。容器因此空掉时整个去掉。
fn restore(
    plan: &mut MutationPlan,
    dom: &Dom,
    container: NodeId,
    change: NodeId,
    inner: LocalName,
    keep: &[LocalName],
) {
    let mut kept = 0usize;
    for c in live_children(dom, container).collect::<Vec<_>>() {
        if c == change {
            continue;
        }
        let Some(name) = dom.name(c) else { continue };
        if name.ns == NsId::W && keep.contains(&name.local) {
            kept += 1;
            continue;
        }
        plan.node_edits.push(NodeEdit::Delete(c));
    }
    let snapshot = live_children(dom, change).find(|&c| dom.is(c, w(inner)));
    let mut restored = 0usize;
    if let Some(snapshot) = snapshot {
        // 还原的子元素要与**留下来的**那些排在一起（`PROP-05`）：`w:pPr` 里 `w:rPr`（33）不在
        // 快照里、原地不动，还原的 `w:ind`（22）就得插在它前面。一律插在 `*Change` 之前会排到
        // 它后面去，保存时的顺序自检当场拦下（`TEST-07` 五步就抓到：追踪改两次段落属性 +
        // 中间拒绝一次）
        let order = crate::semantic::props::TABLES
            .iter()
            .find(|t| dom.name(container) == Some(t.element))
            .map(|t| t.order_index);
        for c in live_children(dom, snapshot).collect::<Vec<_>>() {
            let before = order
                .zip(dom.name(c).and_then(|q| order.and_then(|f| f(q))))
                .and_then(|(f, mine)| {
                    live_children(dom, container)
                        .filter(|&k| k != change)
                        .find(|&k| dom.name(k).and_then(f).is_some_and(|i| i > mine))
                })
                .unwrap_or(change);
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::Node(container),
                before: Some(before),
                source: c,
            });
            restored += 1;
        }
    }
    plan.node_edits.push(NodeEdit::Delete(change));
    // 旧值是"什么都没有"→ 容器整个去掉（Word 的形态，见 `DROPPABLE_EMPTY`）
    if kept == 0
        && restored == 0
        && dom.name(container).is_some_and(|q| DROPPABLE_EMPTY.contains(&q.local))
    {
        plan.node_edits.push(NodeEdit::Delete(container));
    }
}

/// `CellInsert` 拒绝 / `CellDelete` 接受：删这个格；整列的格都带同一种标记时连
/// `w:gridCol` 一起删，否则把左邻格加宽，保住"行的网格宽度 = `tblGrid` 列数"。
fn plan_drop_cells(s: &mut EditSession, job: &Job) -> Result<Vec<MutationPlan>> {
    let part = Some(job.part);
    let dom = s.dom_in(part)?;
    let cell = dom
        .parent(job.node)
        .and_then(|tcpr| dom.parent(tcpr))
        .filter(|&c| dom.is(c, w(LocalName::Tc)))
        .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "格标记不在 w:tc/w:tcPr 里"))?;
    let mark = dom.name(job.node).map(|q| q.local).unwrap_or(LocalName::CellIns);
    let table = dom
        .ancestors(cell)
        .find(|&a| dom.is(a, w(LocalName::Tbl)))
        .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "格不在表格里"))?;
    let Some((col, span)) = super::table_ops::cell_column(s, table, cell) else {
        let mut plan = MutationPlan::new(job.part);
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Delete(cell));
        return Ok(vec![plan]);
    };
    // 这一列上带同种标记的格；有一行没标记 → 网格不能收缩
    let column = super::table_ops::column_cells(s, table, col);
    let dom = s.dom_in(part)?;
    let marked = |c: NodeId| {
        live_children(dom, c)
            .find(|&x| dom.is(x, w(LocalName::TcPr)))
            .is_some_and(|tcpr| live_children(dom, tcpr).any(|m| dom.is(m, w(mark))))
    };
    let whole_column = !column.is_empty() && column.iter().all(|&(_, c)| marked(c));
    let mut plan = MutationPlan::new(job.part);
    plan.structure_changed = true;
    plan.touch(table);
    if whole_column {
        for &(_, c) in &column {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
        if span == 1 {
            for g in super::table_ops::grid_cols(s, table).into_iter().skip(col as usize).take(1) {
                plan.node_edits.push(NodeEdit::Delete(g));
            }
        }
    } else {
        // 只这一行少一个格：把它的宽度并进邻格，行的网格宽度才还对得上 `tblGrid`
        plan.node_edits.push(NodeEdit::Delete(cell));
        super::table_ops::absorb_cell_width(s, table, cell, &mut plan);
    }
    Ok(vec![plan])
}
