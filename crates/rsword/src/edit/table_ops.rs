//! 表格的行列结构操作（`EDIT-03` 表格段，任务 3.8）。
//!
//! **几何以声明网格为准**：列数 = `tblGrid/gridCol` 个数，行宽 = `gridBefore + Σ gridSpan + gridAfter`。
//! 任一行的宽度与列数不符时，列操作与 `MergeCells` 直接 `Err(EDIT_TABLE_GRID_INCONSISTENT)`——**不**
//! 偷偷修网格，修不修由调用方决定（`spec/14` 风险 3）。折叠、校正一律不做：那是 `resolve` 的视图。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::table::{Cell, Row, TableBlock};
use crate::semantic::props::{
    Change, Merge, RowPropsPatch, TblWidth, Val, plan_apply_cell_props_at, plan_apply_row_props_at,
};
use crate::span::RangeKind;
use crate::xml::{Dirty, Dom, LocalName, NewElement, NodeEdit, NodeId, QName, Target};

use super::CellPropsPatch;
use super::EditContext;
use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;
use super::track::Tracker;

fn w(local: LocalName) -> QName {
    QName::w(local)
}

fn geometry_error(msg: impl Into<String>) -> Error {
    Error::edit(DiagCode::EditTableGeometry, msg)
}

/// 合并区里某一行涉及的格：(`w:tc` 节点, 起始列, 跨度)。
type RegionCells = Vec<(NodeId, u32, u32)>;

/// 一行在声明网格上的布局。
#[derive(Debug, Clone)]
pub(crate) struct RowGeometry {
    pub node: NodeId,
    /// `gridBefore` / `gridAfter`。
    pub before: u32,
    pub after: u32,
    /// 每个物理格：(`w:tc` 节点, 起始列, 跨度)。
    pub cells: Vec<(NodeId, u32, u32)>,
}

impl RowGeometry {
    /// 这一行占的网格列数。
    pub fn width(&self) -> u32 {
        self.before + self.cells.iter().map(|c| c.2).sum::<u32>() + self.after
    }

    /// 覆盖第 `col` 列的格在 `cells` 里的下标。
    fn cell_at(&self, col: u32) -> Option<usize> {
        self.cells.iter().position(|&(_, start, span)| col >= start && col < start + span)
    }
}

/// 整张表在声明网格上的布局。
#[derive(Debug, Clone)]
pub(crate) struct Geometry {
    pub cols: u32,
    pub rows: Vec<RowGeometry>,
    /// `tblGrid/gridCol` 节点与声明宽度。
    pub grid: Vec<(NodeId, i32)>,
}

fn u32_of(v: &Option<Val<i32>>) -> u32 {
    v.as_ref().and_then(Val::value).copied().unwrap_or(0).max(0) as u32
}

pub(crate) fn geometry(table: &TableBlock) -> Geometry {
    let rows = table
        .rows
        .iter()
        .map(|r: &Row| {
            let before = u32_of(&r.props.grid_before);
            let mut col = before;
            let cells = r
                .cells
                .iter()
                .map(|c: &Cell| {
                    let span = c.grid_span();
                    let at = col;
                    col += span;
                    (c.node, at, span)
                })
                .collect();
            RowGeometry { node: r.node, before, after: u32_of(&r.props.grid_after), cells }
        })
        .collect();
    Geometry {
        cols: table.grid.len() as u32,
        rows,
        grid: table
            .grid
            .iter()
            .map(|g| (g.node, g.w.as_ref().and_then(Val::value).copied().unwrap_or(0)))
            .collect(),
    }
}

impl Geometry {
    /// 每行的网格宽度都等于列数（列操作的前提）。
    fn consistent(&self) -> bool {
        self.cols > 0 && self.rows.iter().all(|r| r.width() == self.cols)
    }

    pub(super) fn require_consistent(&self) -> Result<()> {
        if self.consistent() {
            return Ok(());
        }
        Err(Error::edit(
            DiagCode::EditTableGridInconsistent,
            format!(
                "表格网格不一致：tblGrid {} 列，各行 {:?}",
                self.cols,
                self.rows.iter().map(RowGeometry::width).collect::<Vec<_>>()
            ),
        ))
    }
}

/// 会话里的表格块（含嵌套表）。
fn table_of(s: &EditSession, node: NodeId) -> Result<&TableBlock> {
    s.document()
        .tables()
        .find(|t| t.node == node)
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "目标不是表格"))
}

/// 元素子节点（跳过缩进空白与已删除的）。
fn element_children(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    dom.children(node)
        .iter()
        .copied()
        .filter(|&c| dom.element(c).is_some() && dom.node(c).dirty != Dirty::Deleted)
        .collect()
}

/// `node` 所在的、`parent` 的那个直接子节点（行 / 格可能被 `w:sdt` 包着，插入锚点要用外层的）。
fn direct_child(dom: &Dom, parent: NodeId, node: NodeId) -> Option<NodeId> {
    let mut x = node;
    loop {
        let p = dom.parent(x)?;
        if p == parent {
            return Some(x);
        }
        x = p;
    }
}

fn child_named(dom: &Dom, parent: NodeId, local: LocalName) -> Option<NodeId> {
    element_children(dom, parent).into_iter().find(|&c| dom.is(c, w(local)))
}

/// 空 `w:p`；`clone_ppr` 是要克隆 `w:pPr` 的来源段落。
fn empty_paragraph(dom: &Dom, plan: &mut MutationPlan, parent: Target, clone_ppr: Option<NodeId>) {
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent,
        before: None,
        node: NewElement::new(w(LocalName::P)),
    });
    if let Some(src) = clone_ppr.and_then(|p| child_named(dom, p, LocalName::PPr)) {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: src,
        });
    }
}

// ---- InsertRow / DeleteRow ---------------------------------------------------------------------

pub(crate) fn insert_row(
    s: &mut EditSession,
    table: NodeId,
    at: u32,
    template: Option<NodeId>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let t = table_of(s, table)?;
    let rows: Vec<NodeId> = t.rows.iter().map(|r| r.node).collect();
    if at as usize > rows.len() {
        return Err(geometry_error(format!("行号 {at} 超出 {} 行", rows.len())));
    }
    // 模板：给定的行，否则 at 的前一行（at == 0 时取第一行）
    let tpl_idx = match template {
        Some(n) => rows
            .iter()
            .position(|&r| r == n)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "模板行不属于这张表"))?,
        None if rows.is_empty() => {
            return Err(geometry_error("空表格没有可用的模板行"));
        }
        None => (at as usize).saturating_sub(1).min(rows.len() - 1),
    };
    let tpl = &t.rows[tpl_idx];
    // `vMerge`：模板是 continue → 新行不带；模板是 restart 且新行插在它与它的 continue 之间 → 新行是 continue
    let next_is_continue = |cell_idx: usize| {
        t.rows
            .get(at as usize)
            .and_then(|r| r.cells.get(cell_idx))
            .is_some_and(Cell::is_vmerge_continue)
    };
    let cells: Vec<(NodeId, Option<NodeId>, Option<NodeId>, VMergeFix)> = tpl
        .cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let fix = if c.is_vmerge_continue() {
                VMergeFix::Drop
            } else if c.props.v_merge.is_some() && next_is_continue(i) {
                VMergeFix::Continue
            } else {
                VMergeFix::Keep
            };
            let dom = s.dom();
            let tc_pr = child_named(dom, c.node, LocalName::TcPr);
            let first_para =
                c.blocks.first().map(|b| b.node()).filter(|&n| dom.is(n, w(LocalName::P)));
            (c.node, tc_pr, first_para, fix)
        })
        .collect();
    let tr_pr = child_named(s.dom(), tpl.node, LocalName::TrPr);
    let tbl_pr_ex = child_named(s.dom(), tpl.node, LocalName::TblPrEx);
    let before = rows.get(at as usize).and_then(|&r| direct_child(s.dom(), table, r));

    let mut tracker = Tracker::new(s.document(), ctx);
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    let row_k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(table),
        before,
        node: NewElement::new(w(LocalName::Tr)),
    });
    if let Some(src) = tbl_pr_ex {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(row_k),
            before: None,
            source: src,
        });
    }
    // 追踪：整行插入 → `trPr/w:ins`（`spec/08`）。`trPr` 是模板行的字节克隆，标记加在克隆之后。
    // 模板行**自己的**修订标记不跟着走：新行是这次插进来的，不是模板那次被删 / 被改的
    // （`TEST-07` 在「先追踪删一行、再照它插一行」上抓到过：`w:ins` 落在 `w:del` 后面，
    // `PROP-05` 的顺序自检当场拦下）
    let src_revs = tr_pr.is_some_and(|src| row_revision_marks(s.dom(), src).next().is_some());
    match (&mut tracker, tr_pr) {
        (None, Some(src)) if !src_revs => plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(row_k),
            before: None,
            source: src,
        }),
        (None, Some(src)) => {
            clone_row_props_without_revisions(&mut plan, s.dom(), row_k, src);
        }
        (None, None) => {}
        (Some(t), Some(src)) => {
            let k = plan.node_edits.len();
            if src_revs {
                clone_row_props_without_revisions(&mut plan, s.dom(), row_k, src);
            } else {
                plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::New(row_k),
                    before: None,
                    source: src,
                });
            }
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: t.marker(LocalName::Ins),
            });
        }
        (Some(t), None) => {
            let marker = t.marker(LocalName::Ins);
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(row_k),
                before: None,
                node: NewElement::new(w(LocalName::TrPr)).with_child(marker),
            });
        }
    }
    let dom = s.dom();
    for (cell_node, tc_pr, first_para, fix) in cells {
        let cell_k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(row_k),
            before: None,
            node: NewElement::new(w(LocalName::Tc)),
        });
        match (fix, tc_pr) {
            // 需要改 vMerge 时按模型重新生成 tcPr（克隆的子树没法就地改）
            (VMergeFix::Drop | VMergeFix::Continue, _) => {
                let mut props =
                    crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
                props.v_merge = match fix {
                    VMergeFix::Continue => Some(Merge::cont()),
                    _ => None,
                };
                let node = crate::semantic::props::emit_cell_props(&props, s.flavor());
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(cell_k),
                    before: None,
                    node,
                });
            }
            (VMergeFix::Keep, Some(src)) => plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(cell_k),
                before: None,
                source: src,
            }),
            (VMergeFix::Keep, None) => {}
        }
        let _ = cell_node;
        empty_paragraph(dom, &mut plan, Target::New(cell_k), first_para);
    }
    plan.touch(table);
    s.commit_plan(plan)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VMergeFix {
    Keep,
    /// 模板格是合并区的非首格：新行该格不带 `vMerge`。
    Drop,
    /// 新行插进了合并区中间：该格是 continue。
    Continue,
}

pub(crate) fn delete_row(
    s: &mut EditSession,
    table: NodeId,
    at: u32,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let t = table_of(s, table)?;
    let row = t
        .rows
        .get(at as usize)
        .ok_or_else(|| geometry_error(format!("行号 {at} 超出 {} 行", t.rows.len())))?;
    let geo = geometry(t);
    let node = row.node;
    // 追踪：**行留着**，只加 `trPr/w:del`（`spec/08`）。vMerge 的提升不做——内容一点没变
    if let Some(mut t) = Tracker::new(s.document(), ctx) {
        let dom = s.dom();
        let mut plan = MutationPlan::new(s.main_part());
        plan.touch(table);
        let before =
            element_children(dom, node).into_iter().find(|&c| !dom.is(c, w(LocalName::TblPrEx)));
        t.container_mark(
            &mut plan,
            dom,
            crate::edit::track::MarkSite {
                owner: node,
                container: LocalName::TrPr,
                container_before: before,
            },
            LocalName::Del,
            crate::semantic::props::order_index_row_props,
        );
        return s.commit_plan(plan);
    }
    // 被删行里 vMerge restart 的格：把下一行同列的 continue 提升为 restart
    let mut promote: Vec<NodeId> = Vec::new();
    if let (Some(this), Some(next)) = (geo.rows.get(at as usize), geo.rows.get(at as usize + 1)) {
        for (i, &(_, col, _)) in this.cells.iter().enumerate() {
            let cell = &t.rows[at as usize].cells[i];
            if !cell.props.v_merge.as_ref().is_some_and(Merge::is_restart) {
                continue;
            }
            if let Some(j) = next.cell_at(col)
                && t.rows[at as usize + 1].cells[j].is_vmerge_continue()
            {
                promote.push(next.cells[j].0);
            }
        }
    }
    let dom = s.dom();
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    plan.node_edits.push(NodeEdit::Delete(node));
    for tc in promote {
        let tc_pr = child_named(dom, tc, LocalName::TcPr);
        let patch = CellPropsPatch { v_merge: Change::Set(Merge::restart()), ..Default::default() };
        let before = element_children(dom, tc).first().copied();
        plan_apply_cell_props_at(
            dom,
            Target::Node(tc),
            tc_pr,
            before,
            &patch,
            s.flavor(),
            &mut plan.node_edits,
        );
    }
    plan.touch(table);
    s.commit_plan(plan)
}

// ---- InsertColumn / DeleteColumn ----------------------------------------------------------------

/// 行属性里的 `gridBefore` / `gridAfter` 增减。
fn bump_row_gap(
    s: &EditSession,
    row: &RowGeometry,
    before: Option<u32>,
    after: Option<u32>,
    plan: &mut MutationPlan,
) {
    let dom = s.dom();
    let patch = RowPropsPatch {
        grid_before: before.map_or(Change::Keep, |v| {
            if v == 0 { Change::Unset } else { Change::Set(Val::Value(v as i32)) }
        }),
        grid_after: after.map_or(Change::Keep, |v| {
            if v == 0 { Change::Unset } else { Change::Set(Val::Value(v as i32)) }
        }),
        ..Default::default()
    };
    let tr_pr = child_named(dom, row.node, LocalName::TrPr);
    let anchor =
        element_children(dom, row.node).into_iter().find(|&c| !dom.is(c, w(LocalName::TblPrEx)));
    plan_apply_row_props_at(
        dom,
        Target::Node(row.node),
        tr_pr,
        anchor,
        &patch,
        s.flavor(),
        &mut plan.node_edits,
    );
}

/// 一个格的 `gridSpan` / `tcW` 调整。
fn patch_cell_span(
    s: &EditSession,
    cell: NodeId,
    span: Option<u32>,
    width_delta: i32,
    plan: &mut MutationPlan,
) {
    let dom = s.dom();
    let tc_pr = child_named(dom, cell, LocalName::TcPr);
    let current = crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
    let mut patch = CellPropsPatch::default();
    if let Some(span) = span {
        patch.grid_span =
            if span <= 1 { Change::Unset } else { Change::Set(Val::Value(span as i32)) };
    }
    if width_delta != 0
        && let Some(old) = current.width.as_ref().and_then(TblWidth::twips)
    {
        patch.width = Change::Set(TblWidth::dxa((old + width_delta).max(0)));
    }
    let before = element_children(dom, cell).first().copied();
    plan_apply_cell_props_at(
        dom,
        Target::Node(cell),
        tc_pr,
        before,
        &patch,
        s.flavor(),
        &mut plan.node_edits,
    );
}

/// 书签 / 权限范围的 `w:colFirst` / `w:colLast` 随列增删移动（`SPAN-03`）。
fn shift_bookmark_columns(s: &mut EditSession, at: u32, delta: i32, plan: &mut MutationPlan) {
    let part = s.main_part();
    let Ok(index) = s.spans_of(part) else { return };
    let mut edits: Vec<(NodeId, LocalName, Option<u32>)> = Vec::new();
    for span in index.spans() {
        let cols = match &span.kind {
            RangeKind::Bookmark { cols: Some(c), .. }
            | RangeKind::Permission { cols: Some(c), .. } => *c,
            _ => continue,
        };
        let Some(start) = span.start.as_ref().and_then(|a| a.marker) else { continue };
        let shift = |v: u32| -> Option<u32> {
            if delta > 0 {
                Some(if v >= at { v + 1 } else { v })
            } else if v > at {
                Some(v - 1)
            } else if v == at {
                None // 该列被删：区间收缩由下面的两端一起决定
            } else {
                Some(v)
            }
        };
        let (first, last) = (shift(cols.0), shift(cols.1));
        let new = match (first, last) {
            (Some(a), Some(b)) if a <= b => (a, b),
            // 起点落在被删列上：区间从下一列开始
            (None, Some(b)) => (at.min(b), b),
            (Some(a), None) => (a, a),
            _ => continue,
        };
        if new != cols {
            edits.push((start, LocalName::ColFirst, Some(new.0)));
            edits.push((start, LocalName::ColLast, Some(new.1)));
        }
    }
    for (node, name, value) in edits {
        if let Some(v) = value {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(node),
                name: w(name),
                value: v.to_string(),
            });
        }
    }
}

pub(crate) fn insert_column(
    s: &mut EditSession,
    table: NodeId,
    at: u32,
    width: i32,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let t = table_of(s, table)?;
    let geo = geometry(t);
    geo.require_consistent()?;
    if at > geo.cols {
        return Err(geometry_error(format!("列号 {at} 超出 {} 列", geo.cols)));
    }
    // 每行的落点：拿到需要克隆的模板（左邻格，行首取右邻）
    enum Where {
        Gap { before: Option<u32>, after: Option<u32> },
        Widen(NodeId, u32),
        NewCell { before: Option<NodeId>, template: Option<(NodeId, Option<NodeId>)> },
    }
    let dom = s.dom();
    let mut plans: Vec<(usize, Where)> = Vec::new();
    for (ri, row) in geo.rows.iter().enumerate() {
        let end_of_cells = row.before + row.cells.iter().map(|c| c.2).sum::<u32>();
        let place = if at < row.before {
            Where::Gap { before: Some(row.before + 1), after: None }
        } else if at >= end_of_cells && row.after > 0 {
            Where::Gap { before: None, after: Some(row.after + 1) }
        } else {
            match row.cells.iter().position(|&(_, start, span)| at > start && at < start + span) {
                // 落在某个跨列格中间 → 加宽
                Some(i) => Where::Widen(row.cells[i].0, row.cells[i].2 + 1),
                None => {
                    let idx = row.cells.iter().position(|&(_, start, _)| start == at);
                    let template_idx = match idx {
                        Some(0) => row.cells.first(),
                        Some(i) => row.cells.get(i - 1),
                        None => row.cells.last(),
                    };
                    Where::NewCell {
                        before: idx.and_then(|i| direct_child(dom, row.node, row.cells[i].0)),
                        template: template_idx.map(|&(n, _, _)| {
                            let tc_pr = child_named(dom, n, LocalName::TcPr);
                            let para = element_children(dom, n)
                                .into_iter()
                                .find(|&c| dom.is(c, w(LocalName::P)));
                            (n, para.and(tc_pr))
                        }),
                    }
                }
            }
        };
        plans.push((ri, place));
    }

    let mut tracker = Tracker::new(s.document(), ctx);
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    // tblGrid
    let grid_parent = child_named(dom, table, LocalName::TblGrid);
    match grid_parent {
        Some(g) => {
            // 追踪：网格要变，先把**旧**网格快照进 `w:tblGridChange`（这两条编辑排在新
            // `w:gridCol` 之前，克隆到的就是原来的列）
            if let Some(t) = &mut tracker {
                t.snapshot(&mut plan, dom, g, LocalName::TblGridChange, LocalName::TblGrid, &[]);
            }
            let before = geo.grid.get(at as usize).map(|&(n, _)| n);
            let mut col = NewElement::new(w(LocalName::GridCol));
            col.push_attr(w(LocalName::W), width.to_string());
            plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(g), before, node: col });
        }
        None => return Err(geometry_error("表格没有 tblGrid")),
    }
    for (ri, place) in plans {
        let row = &geo.rows[ri];
        match place {
            Where::Gap { before, after } => {
                // 追踪：`gridBefore` / `gridAfter` 是行属性 → 旧值进 `w:trPrChange`
                if let Some(t) = &mut tracker
                    && let Some(trpr) = child_named(dom, row.node, LocalName::TrPr)
                {
                    t.snapshot(
                        &mut plan,
                        dom,
                        trpr,
                        LocalName::TrPrChange,
                        LocalName::TrPr,
                        &[LocalName::Ins, LocalName::Del],
                    );
                }
                bump_row_gap(s, row, before, after, &mut plan)
            }
            Where::Widen(cell, span) => {
                // 追踪：加宽跨列格改的是 `gridSpan` / `tcW` → 旧值进 `w:tcPrChange`
                if let Some(t) = &mut tracker
                    && let Some(tcpr) = child_named(dom, cell, LocalName::TcPr)
                {
                    t.snapshot(
                        &mut plan,
                        dom,
                        tcpr,
                        LocalName::TcPrChange,
                        LocalName::TcPr,
                        &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge],
                    );
                }
                patch_cell_span(s, cell, Some(span), width, &mut plan)
            }
            Where::NewCell { before, template } => {
                let cell_k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(row.node),
                    before,
                    node: NewElement::new(w(LocalName::Tc)),
                });
                // 新格的 tcPr：克隆模板但去掉 gridSpan / vMerge，宽度换成新列宽
                // 新格的 `tcPr`：克隆模板但去掉 gridSpan / vMerge，宽度换成新列宽。
                // 追踪时 `w:cellIns` 要放进**同一个** `w:tcPr`（两个 `w:tcPr` 不合法）
                let props = template.map(|(_, tc_pr)| {
                    let mut props =
                        crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
                    props.grid_span = None;
                    props.v_merge = None;
                    props.h_merge = None;
                    if props.width.is_some() {
                        props.width = Some(TblWidth::dxa(width));
                    }
                    props
                });
                let want_container =
                    tracker.is_some() || props.as_ref().is_some_and(|p| !props_is_empty(p));
                if want_container {
                    let mut node = match &props {
                        Some(p) => crate::semantic::props::emit_cell_props(p, s.flavor()),
                        None => NewElement::new(w(LocalName::TcPr)),
                    };
                    if let Some(t) = &mut tracker {
                        crate::edit::track::insert_ordered(
                            &mut node,
                            t.marker(LocalName::CellIns),
                            crate::semantic::props::order_index_cell_props,
                        );
                    }
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::New(cell_k),
                        before: None,
                        node,
                    });
                }
                let para = template.and_then(|(tpl, _)| {
                    element_children(dom, tpl).into_iter().find(|&c| dom.is(c, w(LocalName::P)))
                });
                empty_paragraph(dom, &mut plan, Target::New(cell_k), para);
            }
        }
    }
    shift_bookmark_columns(s, at, 1, &mut plan);
    plan.touch(table);
    s.commit_plan(plan)
}

fn props_is_empty(p: &crate::semantic::props::CellProps) -> bool {
    crate::semantic::props::diff_cell_props(&Default::default(), p)
        == crate::semantic::props::CellPropsPatch::default()
}

pub(crate) fn delete_column(
    s: &mut EditSession,
    table: NodeId,
    at: u32,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let t = table_of(s, table)?;
    let geo = geometry(t);
    geo.require_consistent()?;
    if at >= geo.cols {
        return Err(geometry_error(format!("列号 {at} 超出 {} 列", geo.cols)));
    }
    // 追踪：**格与网格都留着**，只给这一列的格加 `tcPr/w:cellDel`（`spec/08`）。
    // 网格不动，所以**不发** `w:tblGridChange`——收缩发生在接受修订时（7.4），
    // 那时才知道最终的列宽（登记在 `docs/04` §8）
    if let Some(mut tr) = Tracker::new(s.document(), ctx) {
        let dom = s.dom();
        let mut plan = MutationPlan::new(s.main_part());
        plan.touch(table);
        for row in &geo.rows {
            let Some(i) = row.cell_at(at) else { continue };
            let cell = row.cells[i].0;
            let before = element_children(dom, cell).first().copied();
            tr.container_mark(
                &mut plan,
                dom,
                crate::edit::track::MarkSite {
                    owner: cell,
                    container: LocalName::TcPr,
                    container_before: before,
                },
                LocalName::CellDel,
                crate::semantic::props::order_index_cell_props,
            );
        }
        return s.commit_plan(plan);
    }
    let width = geo.grid.get(at as usize).map_or(0, |&(_, w)| w);
    // 先看会不会把某一行掏空
    for row in &geo.rows {
        if row.cells.len() == 1
            && let Some(i) = row.cell_at(at)
            && row.cells[i].2 == 1
        {
            return Err(geometry_error(
                "删掉这一列会让某一行没有单元格；请改用 DeleteBlock 删整表",
            ));
        }
    }
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;
    if let Some(&(node, _)) = geo.grid.get(at as usize) {
        plan.node_edits.push(NodeEdit::Delete(node));
    }
    for row in &geo.rows {
        let end_of_cells = row.before + row.cells.iter().map(|c| c.2).sum::<u32>();
        if at < row.before {
            bump_row_gap(s, row, Some(row.before - 1), None, &mut plan);
        } else if at >= end_of_cells {
            if row.after > 0 {
                bump_row_gap(s, row, None, Some(row.after - 1), &mut plan);
            }
        } else if let Some(i) = row.cell_at(at) {
            let (node, _, span) = row.cells[i];
            if span == 1 {
                plan.node_edits.push(NodeEdit::Delete(node));
            } else {
                patch_cell_span(s, node, Some(span - 1), -width, &mut plan);
            }
        }
    }
    shift_bookmark_columns(s, at, -1, &mut plan);
    plan.touch(table);
    s.commit_plan(plan)
}

/// 一个 `w:tc` 在网格里的 `(起始列, 跨度)`（7.4 的格接受 / 拒绝用）。
pub(super) fn cell_column(s: &EditSession, table: NodeId, cell: NodeId) -> Option<(u32, u32)> {
    let t = table_of(s, table).ok()?;
    let geo = geometry(t);
    geo.rows.iter().find_map(|row| {
        row.cells.iter().find(|&&(n, _, _)| n == cell).map(|&(_, start, span)| (start, span))
    })
}

/// 一个格被单独删掉时，把它的网格跨度与宽度并进同行的邻格（左邻优先，行首取右邻），
/// 保住「一行的网格宽度 = `tblGrid` 列数」（`MOD-07` / `SAVE-02` 的 `SAVE_TABLE_GRID`）。
/// 7.4 拒绝 `cellIns` / 接受 `cellDel` 时，只有一行少一个格的情况走这里。
pub(super) fn absorb_cell_width(
    s: &EditSession,
    table: NodeId,
    cell: NodeId,
    plan: &mut MutationPlan,
) {
    let Ok(t) = table_of(s, table) else { return };
    let geo = geometry(t);
    let Some(row) = geo.rows.iter().find(|r| r.cells.iter().any(|&(n, _, _)| n == cell)) else {
        return;
    };
    let i = row.cells.iter().position(|&(n, _, _)| n == cell).expect("checked above");
    let pick = if i > 0 { row.cells.get(i - 1) } else { row.cells.get(i + 1) };
    let Some(&(neighbour, _, nspan)) = pick else { return };
    let width = {
        let dom = s.dom();
        let tc_pr = child_named(dom, cell, LocalName::TcPr);
        crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new())
            .width
            .as_ref()
            .and_then(TblWidth::twips)
            .unwrap_or(0)
    };
    let span = row.cells[i].2;
    patch_cell_span(s, neighbour, Some(nspan + span), width, plan);
}

/// 每一行在第 `col` 列上的那个 `w:tc`（`(行节点, 格节点)`）。
pub(super) fn column_cells(s: &EditSession, table: NodeId, col: u32) -> Vec<(NodeId, NodeId)> {
    let Ok(t) = table_of(s, table) else { return Vec::new() };
    let geo = geometry(t);
    geo.rows.iter().filter_map(|row| row.cell_at(col).map(|i| (row.node, row.cells[i].0))).collect()
}

/// `w:tblGrid` 的 `w:gridCol` 节点，按列序。
pub(super) fn grid_cols(s: &EditSession, table: NodeId) -> Vec<NodeId> {
    let Ok(t) = table_of(s, table) else { return Vec::new() };
    geometry(t).grid.iter().map(|&(n, _)| n).collect()
}

// ---- MergeCells ---------------------------------------------------------------------------------

pub(crate) fn merge_cells(
    s: &mut EditSession,
    table: NodeId,
    from: (u32, u32),
    to: (u32, u32),
    ctx: &EditContext,
) -> Result<MutationResult> {
    // 第一阶段：追踪时拒绝（`spec/18`「不在 M7」）。`w:cellMerge` + `vMergeOrig` 的形态要等
    // 真实 Word 的 fixture 校准；顺带一条已知的 Word 行为：Word 的「拒绝所有修订」**不**撤销
    // 单元格合并（`fixtures/revisions/table-and-move/rejected.docx` 首行仍是合并的一格）
    if ctx.track_changes.is_some() {
        return Err(Error::edit(
            DiagCode::EditUnsupportedTrackedMerge,
            "track_changes 开启时不支持 MergeCells（w:cellMerge 的形态待真实 Word 校准）",
        ));
    }
    let t = table_of(s, table)?;
    let geo = geometry(t);
    geo.require_consistent()?;
    let (r0, c0) = from;
    let (r1, c1) = to;
    if r0 > r1 || c0 > c1 || r1 as usize >= geo.rows.len() || c1 >= geo.cols {
        return Err(geometry_error(format!("合并区 {from:?}..={to:?} 越界或方向反了")));
    }
    if r0 == r1 && c0 == c1 {
        return s.commit_plan(MutationPlan::new(s.main_part())); // 单格，无事可做
    }
    // 每行的合并区必须正好由整格组成，且不能与既有的纵向合并区交叠
    let mut regions: Vec<(usize, RegionCells)> = Vec::new();
    for r in r0..=r1 {
        let row = &geo.rows[r as usize];
        let inside: RegionCells = row
            .cells
            .iter()
            .copied()
            .filter(|&(_, start, span)| start + span > c0 && start < c1 + 1)
            .collect();
        let Some(&(_, first_start, _)) = inside.first() else {
            return Err(geometry_error(format!("第 {r} 行在合并区里没有单元格")));
        };
        let (_, last_start, last_span) = *inside.last().expect("non-empty");
        if first_start != c0 || last_start + last_span != c1 + 1 {
            return Err(geometry_error(format!(
                "第 {r} 行的单元格边界与合并区不齐（{first_start}..{} vs {c0}..{}）",
                last_start + last_span,
                c1 + 1
            )));
        }
        // 区内的格不能是别的合并区的延续
        for (i, &(node, _, _)) in inside.iter().enumerate() {
            let cell = t.rows[r as usize]
                .cells
                .iter()
                .find(|c| c.node == node)
                .expect("geometry 与模型同源");
            if cell.is_vmerge_continue() && !(r > r0 && i == 0) {
                return Err(geometry_error("合并区与既有的纵向合并交叠"));
            }
        }
        regions.push((r as usize, inside));
    }
    let vertical = r1 > r0;
    let span = c1 - c0 + 1;
    let dom = s.dom();
    let top = regions[0].1[0].0;
    let mut plan = MutationPlan::new(s.main_part());
    plan.structure_changed = true;

    for (idx, (_, inside)) in regions.iter().enumerate() {
        let keeper = inside[0].0;
        // ① 先改属性：`plan_apply_*` 的插入锚点是改动前的第一个子元素，内容一搬走它就不在了，
        //    所以属性编辑必须排在搬移之前；跨度与 vMerge 合成一个 patch，免得插出两个 tcPr
        let mut patch = CellPropsPatch::default();
        if span > 1 {
            patch.grid_span = Change::Set(Val::Value(span as i32));
            // 合并后的宽度是区内各格宽度之和（都声明了 dxa 才算）
            let widths: Option<i32> = inside
                .iter()
                .map(|&(n, _, _)| {
                    let pr = child_named(dom, n, LocalName::TcPr);
                    crate::semantic::props::read_cell_props(dom, pr, &mut Vec::new())
                        .width
                        .as_ref()
                        .and_then(TblWidth::twips)
                })
                .sum();
            if let Some(total) = widths {
                patch.width = Change::Set(TblWidth::dxa(total));
            }
        }
        if vertical {
            patch.v_merge = Change::Set(if idx == 0 { Merge::restart() } else { Merge::cont() });
        }
        if patch != CellPropsPatch::default() {
            let tc_pr = child_named(dom, keeper, LocalName::TcPr);
            let before = element_children(dom, keeper).first().copied();
            plan_apply_cell_props_at(
                dom,
                Target::Node(keeper),
                tc_pr,
                before,
                &patch,
                s.flavor(),
                &mut plan.node_edits,
            );
        }
        // ② 再搬内容：纵向合并全都并到左上格，纯横向合并并到本行首格；按文档序
        let target = if vertical { top } else { keeper };
        for (j, &(node, _, _)) in inside.iter().enumerate() {
            if j == 0 && node == target {
                continue;
            }
            move_cell_content(dom, node, target, &mut plan);
            if j > 0 {
                plan.node_edits.push(NodeEdit::Delete(node));
            }
        }
        // ③ 被搬空的 continue 格留一个空段落
        if vertical && idx > 0 {
            empty_paragraph(dom, &mut plan, Target::Node(keeper), None);
        }
    }
    plan.touch(table);
    s.commit_plan(plan)
}

/// 把 `from` 格里的内容块（`w:tcPr` 之外的元素）按文档序搬到 `into` 格末尾。
fn move_cell_content(dom: &Dom, from: NodeId, into: NodeId, plan: &mut MutationPlan) {
    for child in element_children(dom, from) {
        if dom.is(child, w(LocalName::TcPr)) {
            continue;
        }
        // 末尾的空段落不搬（Word 合并后不会留下一串空行）
        if dom.is(child, w(LocalName::P)) && crate::span::content_len(dom, child) == 0 {
            continue;
        }
        plan.node_edits.push(NodeEdit::Move {
            node: child,
            parent: Target::Node(into),
            before: None,
        });
    }
}

// ---- NewBlock::Table 生成器 ---------------------------------------------------------------------

/// `rows` × `cols` 的新表格（`EDIT-03 InsertBlock`）。
pub(crate) fn new_table(
    rows: u32,
    cols: u32,
    widths: Option<Vec<i32>>,
    style: Option<String>,
    header: bool,
) -> NewElement {
    const BODY_WIDTH: i32 = 9360;
    let cols = cols.max(1);
    let rows = rows.max(1);
    let widths = widths
        .filter(|v| v.len() == cols as usize)
        .unwrap_or_else(|| vec![BODY_WIDTH / cols as i32; cols as usize]);
    let mut tbl = NewElement::new(w(LocalName::Tbl));

    let mut tbl_pr = NewElement::new(w(LocalName::TblPr));
    if let Some(id) = style {
        let mut e = NewElement::new(w(LocalName::TblStyle));
        e.push_attr(w(LocalName::Val), id);
        tbl_pr.push_child(e);
    }
    let mut tbl_w = NewElement::new(w(LocalName::TblW));
    tbl_w.push_attr(w(LocalName::W), "0".to_string());
    tbl_w.push_attr(w(LocalName::UType), "auto".to_string());
    tbl_pr.push_child(tbl_w);
    let mut look = NewElement::new(w(LocalName::TblLook));
    look.push_attr(w(LocalName::Val), "04A0".to_string());
    for (name, on) in [
        (LocalName::FirstRow, "1"),
        (LocalName::LastRow, "0"),
        (LocalName::FirstColumn, "1"),
        (LocalName::LastColumn, "0"),
        (LocalName::NoHBand, "0"),
        (LocalName::NoVBand, "1"),
    ] {
        look.push_attr(w(name), on.to_string());
    }
    tbl_pr.push_child(look);
    tbl.push_child(tbl_pr);

    let mut grid = NewElement::new(w(LocalName::TblGrid));
    for &width in &widths {
        let mut col = NewElement::new(w(LocalName::GridCol));
        col.push_attr(w(LocalName::W), width.max(1).to_string());
        grid.push_child(col);
    }
    tbl.push_child(grid);

    for r in 0..rows {
        let mut tr = NewElement::new(w(LocalName::Tr));
        if header && r == 0 {
            let mut tr_pr = NewElement::new(w(LocalName::TrPr));
            tr_pr.push_child(NewElement::new(w(LocalName::TblHeader)));
            tr.push_child(tr_pr);
        }
        for &width in &widths {
            let mut tc = NewElement::new(w(LocalName::Tc));
            let mut tc_pr = NewElement::new(w(LocalName::TcPr));
            let mut tc_w = NewElement::new(w(LocalName::TcW));
            tc_w.push_attr(w(LocalName::W), width.max(1).to_string());
            tc_w.push_attr(w(LocalName::UType), "dxa".to_string());
            tc_pr.push_child(tc_w);
            tc.push_child(tc_pr);
            tc.push_child(NewElement::new(w(LocalName::P)));
            tr.push_child(tc);
        }
        tbl.push_child(tr);
    }
    tbl
}

/// 行属性里的修订标记（`trPr/w:ins` / `w:del` / `w:trPrChange`）。
fn row_revision_marks(dom: &Dom, tr_pr: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    element_children(dom, tr_pr).into_iter().filter(move |&c| {
        [LocalName::Ins, LocalName::Del, LocalName::TrPrChange].iter().any(|&l| dom.is(c, w(l)))
    })
}

/// 把模板行的 `trPr` 逐个子元素克隆过去，**跳过修订标记**。子元素各自还是字节克隆
/// （`XML-12` 规则 F），只是那几个不跟着走。
fn clone_row_props_without_revisions(
    plan: &mut MutationPlan,
    dom: &Dom,
    row_k: usize,
    src: NodeId,
) {
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::New(row_k),
        before: None,
        node: NewElement::new(w(LocalName::TrPr)),
    });
    let revs: Vec<NodeId> = row_revision_marks(dom, src).collect();
    for c in element_children(dom, src) {
        if revs.contains(&c) {
            continue;
        }
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: c,
        });
    }
}
