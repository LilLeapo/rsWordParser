//! 表格模型（`MOD-07`，`MOD-09` 的表格部分，任务 3.2）。
//!
//! 模型只存**声明值**：`grid` 是 `gridCol/@w:w` 原值（允许 0 与缺失），`hMerge` 不折叠、`trHeight`
//! 不截、`tcW` 不校正——折叠与校正在 `resolve`（`RES-08`）与 `compat_ts`（`COMPAT-10`）。
//! 行 / 格穿透 `w:sdt`、`w:customXml` 与修订包裹取得；单元格内容复用正文构建器
//! （[`Builder::build_container`]），所以嵌套表、sdt、修订包裹在格里和在正文里一个样。
//! 嵌套超过 [`MAX_CONTAINER_DEPTH`] 层的子表降级为 `Protected(TooDeep)`（语料有 2,000 层、hostile 有
//! 5,000 层的文档，深度上限就是栈的保险）。
//!
//! 另外给 [`Document`] 补跨表格的遍历：[`Document::blocks`] / [`Document::paragraphs`] 深入单元格，
//! [`Document::block_path`] 给任意块的祖先路径（`MOD-13` 的容器级刷新与 `EDIT-02` 的定位用）。

use crate::diag::{DiagCode, Diagnostic};
use crate::model::block::{Block, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TextBlock};
use crate::model::build::{Builder, Document, MAX_CONTAINER_DEPTH};
use crate::package::PartId;
use crate::semantic::props::codec::Twips;
use crate::semantic::props::{
    CellProps, Ctx, RowProps, TableProps, Val, read_attr, read_cell_props, read_cell_props_change,
    read_row_props, read_row_props_change, read_table_props, read_table_props_change,
};
use crate::span::is_range_marker;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// `w:tbl`（`MOD-07`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub node: NodeId,
    /// `w:tblPr` 声明值（装箱：三张表格属性表都有几 KB，嵌套 64 层时栈帧要小）。
    pub props: Box<TableProps>,
    /// `w:tblGrid/w:gridCol` 声明值。
    pub grid: Vec<GridCol>,
    pub rows: Vec<Row>,
    /// `tblPr/tblStyle`。
    pub style_id: Option<String>,
    pub sdt: Option<SdtInfo>,
    /// 块级包裹修订 + `TablePropsChange` / `TableGridChange`。
    pub revisions: Vec<Revision>,
}

/// `w:gridCol`：`w` 是声明值，可以是 0、`Raw`，也可以缺失。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCol {
    pub node: NodeId,
    pub w: Option<Val<i32>>,
}

/// `w:tr`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub node: NodeId,
    pub props: Box<RowProps>,
    /// `w:tblPrEx`：行级表格属性例外，该行优先于 `tblPr`（`RES-08`）。
    pub tbl_pr_ex: Option<Box<TableProps>>,
    pub cells: Vec<Cell>,
    /// 包裹这一行的 `w:sdt`（研究报告模板把 tr 包在 sdt 里）。
    pub sdt: Option<SdtInfo>,
    /// 包裹修订 + `trPr/ins|del`（`Insert` / `Delete`，整行）+ `RowPropsChange`。
    pub revisions: Vec<Revision>,
}

/// `w:tc`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub node: NodeId,
    pub props: Box<CellProps>,
    /// 单元格内容，与正文同一构建器；最后一个块应是 `w:p`（Word 约束，缺了记 `MOD_TABLE_SHAPE`）。
    pub blocks: Vec<Block>,
    pub sdt: Option<SdtInfo>,
    /// 包裹修订 + `CellInsert` / `CellDelete` / `CellMerge` + `CellPropsChange`。
    pub revisions: Vec<Revision>,
}

impl TableBlock {
    /// 声明网格的列数（`tblGrid` 缺失时为 0）。
    pub fn column_count(&self) -> usize {
        self.grid.len()
    }

    /// `(r, c)` 的物理单元格（不折叠 `hMerge`，不按网格坐标）。
    pub fn cell(&self, row: usize, cell: usize) -> Option<&Cell> {
        self.rows.get(row)?.cells.get(cell)
    }

    /// 每一行的网格宽度都等于列数（`EDIT-03` 表格通则的前提；`tblGrid` 缺失时不算一致）。
    pub fn grid_consistent(&self) -> bool {
        let cols = self.grid.len() as i64;
        cols > 0 && self.rows.iter().all(|r| r.grid_width() == cols)
    }
}

impl Row {
    /// `gridBefore + Σ gridSpan + gridAfter`：这一行在声明网格里占的列数。
    pub fn grid_width(&self) -> i64 {
        let n = |v: &Option<Val<i32>>| v.as_ref().and_then(Val::value).copied().unwrap_or(0).max(0);
        i64::from(n(&self.props.grid_before))
            + self.cells.iter().map(|c| i64::from(c.grid_span())).sum::<i64>()
            + i64::from(n(&self.props.grid_after))
    }
}

impl Cell {
    /// `gridSpan`，缺省 1；非正数或 `Raw` 也按 1。
    pub fn grid_span(&self) -> u32 {
        self.props
            .grid_span
            .as_ref()
            .and_then(Val::value)
            .copied()
            .filter(|n| *n > 0)
            .map_or(1, |n| n as u32)
    }

    /// 是否是纵向合并区的非首格（`vMerge` 存在且不是 restart）。
    pub fn is_vmerge_continue(&self) -> bool {
        self.props.v_merge.as_ref().is_some_and(|m| !m.is_restart())
    }

    /// 是否是旧式横向合并的非首格（`hMerge` 存在且不是 restart）；`resolve` 把它折叠进左格。
    pub fn is_hmerge_continue(&self) -> bool {
        self.props.h_merge.as_ref().is_some_and(|m| !m.is_restart())
    }

    /// 格内直接的文本块（不进嵌套表）。
    pub fn text_blocks(&self) -> impl Iterator<Item = &TextBlock> {
        self.blocks.iter().filter_map(Block::as_text)
    }
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// 读容器属性并装箱。**必须**是独立且不内联的函数：`TableProps` 2.1 KB、`CellProps` 2.4 KB，
/// 快照元组同样大；留在 `build_table` / `build_row` / `build_cell` 的栈帧里，它们会一直活到递归
/// 返回（debug 构建按帧分配临时值），64 层嵌套就把 2 MiB 的测试线程栈撑爆。装进 `Box` 之后每层
/// 只留一个指针，2000 层的语料与 5000 层的 hostile 文档都能在默认栈上跑完。
macro_rules! boxed_reader {
    ($(#[$m:meta])* $name:ident, $read:path, $props:ty) => {
        $(#[$m])*
        #[inline(never)]
        fn $name(dom: &Dom, container: Option<NodeId>, diags: &mut Vec<Diagnostic>) -> Box<$props> {
            Box::new($read(dom, container, diags))
        }
    };
}

/// 同上，读 `*PrChange` 的旧值快照。
macro_rules! boxed_change_reader {
    ($(#[$m:meta])* $name:ident, $read:path, $props:ty) => {
        $(#[$m])*
        #[inline(never)]
        fn $name(
            dom: &Dom,
            container: Option<NodeId>,
            diags: &mut Vec<Diagnostic>,
        ) -> Option<(NodeId, Box<$props>)> {
            $read(dom, container, diags).map(|(n, v)| (n, Box::new(v)))
        }
    };
}

boxed_reader!(
    /// `w:tblPr`（也用于 `w:tblPrEx`）。
    boxed_table_props, read_table_props, TableProps
);
boxed_reader!(
    /// `w:trPr`。
    boxed_row_props, read_row_props, RowProps
);
boxed_reader!(
    /// `w:tcPr`。
    boxed_cell_props, read_cell_props, CellProps
);
boxed_change_reader!(
    /// `w:tblPrChange`。
    boxed_table_props_change, read_table_props_change, TableProps
);
boxed_change_reader!(
    /// `w:trPrChange`。
    boxed_row_props_change, read_row_props_change, RowProps
);
boxed_change_reader!(
    /// `w:tcPrChange`。
    boxed_cell_props_change, read_cell_props_change, CellProps
);

/// 行 / 格收集时的包裹上下文：穿透 sdt 与修订包裹要带着它们往下走。
struct Wrap {
    node: NodeId,
    sdt: Option<SdtInfo>,
    revs: Vec<Revision>,
}

impl<'a> Builder<'a> {
    /// `w:tbl` → [`Block::Table`]；嵌套过深 → `Protected(TooDeep)`（`MOD-07`）。
    pub(super) fn build_table(
        &mut self,
        tbl: NodeId,
        sdt: Option<&SdtInfo>,
        revs: &[Revision],
    ) -> Block {
        let dom = self.dom;
        if self.depth > MAX_CONTAINER_DEPTH {
            self.warn(tbl, DiagCode::ModTooDeep, "表格嵌套过深，子表按只读保留");
            return Block::Protected(ProtectedBlock {
                node: tbl,
                kind: ProtectedKind::TooDeep,
                preview: String::new(),
                display: None,
                siblings: Vec::new(),
                sdt: sdt.cloned(),
                revisions: revs.to_vec(),
            });
        }
        let tbl_pr = dom.semantic_children(tbl).find(|&n| dom.is(n, w(LocalName::TblPr)));
        let props = boxed_table_props(dom, tbl_pr, &mut self.warnings);
        let mut revisions = revs.to_vec();
        if let Some((change, old)) = boxed_table_props_change(dom, tbl_pr, &mut self.warnings) {
            revisions.push(Revision::TablePropsChange { meta: self.meta(change), old });
        }
        let mut grid = Vec::new();
        if let Some(g) = dom.semantic_children(tbl).find(|&n| dom.is(n, w(LocalName::TblGrid))) {
            for c in dom.semantic_children(g) {
                if dom.is(c, w(LocalName::GridCol)) {
                    let mut ctx = Ctx::new(dom, &mut self.warnings);
                    ctx.enter(c);
                    let width = read_attr::<Twips>(c, w(LocalName::W), None, &mut ctx);
                    grid.push(GridCol { node: c, w: width });
                } else if dom.is(c, w(LocalName::TblGridChange)) {
                    let old = dom
                        .semantic_children(c)
                        .find(|&n| dom.is(n, w(LocalName::TblGrid)))
                        .unwrap_or(c);
                    revisions.push(Revision::TableGridChange { meta: self.meta(c), old });
                }
            }
        }
        let mut rows = Vec::new();
        self.collect_rows(tbl, &mut rows);
        let table = TableBlock {
            node: tbl,
            style_id: props.style.clone(),
            props,
            grid,
            rows,
            sdt: sdt.cloned(),
            revisions,
        };
        if !table.grid.is_empty() && !table.grid_consistent() {
            self.warn(
                tbl,
                DiagCode::ModTableShape,
                format!(
                    "行的网格宽度与 tblGrid 的 {} 列不一致：{:?}",
                    table.grid.len(),
                    table.rows.iter().map(Row::grid_width).collect::<Vec<_>>()
                ),
            );
        }
        Block::Table(table)
    }

    /// `w:tbl` 的行：穿透 `w:sdt/w:sdtContent`、`w:customXml` 与 `w:ins/w:del/w:moveFrom/w:moveTo`
    /// 包裹（带着 sdt / 修订上下文），跳过 `tblPr` / `tblGrid` / 范围标记。迭代实现，包裹层数不限。
    fn collect_rows(&mut self, tbl: NodeId, out: &mut Vec<Row>) {
        let dom = self.dom;
        let mut stack: Vec<Wrap> = vec![Wrap { node: tbl, sdt: None, revs: Vec::new() }];
        // 先进后出：一个包裹展开后，它的子节点要按文档序处理，所以逆序压栈
        while let Some(Wrap { node, sdt, revs }) = stack.pop() {
            if dom.is(node, w(LocalName::Tr)) {
                let row = self.build_row(node, sdt.as_ref(), &revs);
                out.push(row);
                continue;
            }
            let content = self.wrapped_children(node, &sdt, &revs, "表格");
            for wrap in content.into_iter().rev() {
                stack.push(wrap);
            }
        }
    }

    /// `w:tr` 的格：同 [`Self::collect_rows`]，跳过 `trPr` / `tblPrEx`。
    fn collect_cells(&mut self, tr: NodeId, out: &mut Vec<Cell>) {
        let dom = self.dom;
        let mut stack: Vec<Wrap> = vec![Wrap { node: tr, sdt: None, revs: Vec::new() }];
        while let Some(Wrap { node, sdt, revs }) = stack.pop() {
            if dom.is(node, w(LocalName::Tc)) {
                let cell = self.build_cell(node, sdt.as_ref(), &revs);
                out.push(cell);
                continue;
            }
            let content = self.wrapped_children(node, &sdt, &revs, "表格行");
            for wrap in content.into_iter().rev() {
                stack.push(wrap);
            }
        }
    }

    /// 把 `node`（tbl / tr / sdt / sdtContent / customXml / 修订包裹）的子节点变成待处理项：
    /// `tr` / `tc` 原样返回，包裹元素带上新的上下文，属性元素与范围标记丢弃，其他记 `MOD_UNKNOWN_BLOCK`。
    fn wrapped_children(
        &mut self,
        node: NodeId,
        sdt: &Option<SdtInfo>,
        revs: &[Revision],
        where_: &str,
    ) -> Vec<Wrap> {
        let dom = self.dom;
        let mut out = Vec::new();
        for child in dom.semantic_children(node) {
            let Some(name) = dom.name(child) else { continue };
            if is_range_marker(name) {
                continue;
            }
            if name.ns != NsId::W {
                self.warn(
                    child,
                    DiagCode::ModUnknownBlock,
                    format!("{where_}里无法分类的元素 {}", name.display(dom.interner())),
                );
                continue;
            }
            match name.local {
                LocalName::Tr | LocalName::Tc => {
                    out.push(Wrap { node: child, sdt: sdt.clone(), revs: revs.to_vec() });
                }
                LocalName::Sdt => {
                    let info = SdtInfo::read(dom, child);
                    if let Some(content) =
                        dom.semantic_children(child).find(|&n| dom.is(n, w(LocalName::SdtContent)))
                    {
                        out.push(Wrap { node: content, sdt: Some(info), revs: revs.to_vec() });
                    }
                }
                LocalName::SdtContent | LocalName::CustomXml | LocalName::SmartTag => {
                    out.push(Wrap { node: child, sdt: sdt.clone(), revs: revs.to_vec() });
                }
                LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
                    let meta = self.meta(child);
                    let rev = match name.local {
                        LocalName::Ins => Revision::Insert(meta),
                        LocalName::Del => Revision::Delete(meta),
                        LocalName::MoveFrom => Revision::MoveFrom(meta),
                        _ => Revision::MoveTo(meta),
                    };
                    let mut inner = revs.to_vec();
                    inner.push(rev);
                    out.push(Wrap { node: child, sdt: sdt.clone(), revs: inner });
                }
                LocalName::TblPr
                | LocalName::TblGrid
                | LocalName::TrPr
                | LocalName::TblPrEx
                | LocalName::TcPr
                | LocalName::SdtPr
                | LocalName::SdtEndPr
                | LocalName::CustomXmlPr
                | LocalName::SmartTagPr
                | LocalName::ProofErr => {}
                _ => self.warn(
                    child,
                    DiagCode::ModUnknownBlock,
                    format!("{where_}里无法分类的元素 {}", name.display(dom.interner())),
                ),
            }
        }
        out
    }

    fn build_row(&mut self, tr: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Row {
        let dom = self.dom;
        let tr_pr = dom.semantic_children(tr).find(|&n| dom.is(n, w(LocalName::TrPr)));
        let props = boxed_row_props(dom, tr_pr, &mut self.warnings);
        let tbl_pr_ex = dom
            .semantic_children(tr)
            .find(|&n| dom.is(n, w(LocalName::TblPrEx)))
            .map(|ex| boxed_table_props(dom, Some(ex), &mut self.warnings));
        let mut revisions = revs.to_vec();
        if let Some(pr) = tr_pr {
            for n in dom.semantic_children(pr) {
                if dom.is(n, w(LocalName::Ins)) {
                    revisions.push(Revision::Insert(self.meta(n)));
                } else if dom.is(n, w(LocalName::Del)) {
                    revisions.push(Revision::Delete(self.meta(n)));
                }
            }
        }
        if let Some((change, old)) = boxed_row_props_change(dom, tr_pr, &mut self.warnings) {
            revisions.push(Revision::RowPropsChange { meta: self.meta(change), old });
        }
        let mut cells = Vec::new();
        self.collect_cells(tr, &mut cells);
        if cells.is_empty() {
            self.warn(tr, DiagCode::ModTableShape, "表格行没有单元格");
        }
        Row { node: tr, props, tbl_pr_ex, cells, sdt: sdt.cloned(), revisions }
    }

    fn build_cell(&mut self, tc: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Cell {
        let dom = self.dom;
        let tc_pr = dom.semantic_children(tc).find(|&n| dom.is(n, w(LocalName::TcPr)));
        let props = boxed_cell_props(dom, tc_pr, &mut self.warnings);
        let mut revisions = revs.to_vec();
        if let Some(pr) = tc_pr {
            for n in dom.semantic_children(pr) {
                let Some(name) = dom.name(n) else { continue };
                if name.ns != NsId::W {
                    continue;
                }
                match name.local {
                    LocalName::CellIns => revisions.push(Revision::CellInsert(self.meta(n))),
                    LocalName::CellDel => revisions.push(Revision::CellDelete(self.meta(n))),
                    LocalName::CellMerge => revisions.push(Revision::CellMerge(self.meta(n))),
                    _ => {}
                }
            }
        }
        if let Some((change, old)) = boxed_cell_props_change(dom, tc_pr, &mut self.warnings) {
            revisions.push(Revision::CellPropsChange { meta: self.meta(change), old });
        }
        // 格内内容：不带外层的 sdt / 修订上下文——它们属于格与行，段落自己的包裹在格里另算
        let mut blocks = Vec::new();
        self.build_container(tc, None, &[], &mut blocks);
        let ends_with_paragraph = blocks.last().is_some_and(|b| {
            dom.is(b.node(), w(LocalName::P))
                || matches!(b, Block::Protected(p) if p.kind == ProtectedKind::TooDeep)
        });
        if !ends_with_paragraph {
            self.warn(tc, DiagCode::ModTableShape, "单元格不以 w:p 结尾");
        }
        Cell { node: tc, props, blocks, sdt: sdt.cloned(), revisions }
    }
}

// ---- 跨表格的块遍历（MOD-13 / EDIT-02 用）--------------------------------------------------------

/// [`Document::block_at_mut`] 的实现：只借块表，不借整个 `Document`（刷新时构建器同时借着样式）。
pub fn block_at_mut_in<'a>(main: &'a mut [Block], path: &[BlockStep]) -> Option<&'a mut Block> {
    let (first, rest) = path.split_first()?;
    let BlockStep::Main(i) = first else { return None };
    let mut cur = main.get_mut(*i)?;
    for step in rest {
        let BlockStep::Cell { row, cell, block } = step else { return None };
        let Block::Table(t) = cur else { return None };
        cur = t.rows.get_mut(*row)?.cells.get_mut(*cell)?.blocks.get_mut(*block)?;
    }
    Some(cur)
}

/// 从顶层块到某个块的一步（`Document::block_path`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockStep {
    /// `Document.main[i]`。
    Main(usize),
    /// 上一步是表格：`rows[row].cells[cell].blocks[block]`。
    Cell { row: usize, cell: usize, block: usize },
}

/// 深度优先、文档序的块迭代器：进单元格，不进文本框（那是别的内容流）。
pub struct Blocks<'a> {
    stack: Vec<&'a Block>,
}

impl<'a> Iterator for Blocks<'a> {
    type Item = &'a Block;

    fn next(&mut self) -> Option<&'a Block> {
        let b = self.stack.pop()?;
        if let Block::Table(t) = b {
            for row in t.rows.iter().rev() {
                for cell in row.cells.iter().rev() {
                    self.stack.extend(cell.blocks.iter().rev());
                }
            }
        }
        Some(b)
    }
}

/// 一个块直接挂着的文本框内容流：`(块列表, 这些 `NodeId` 属于哪个 part)`；`None` = 与宿主同 part。
pub fn box_flows(block: &Block) -> Vec<(&[Block], Option<PartId>)> {
    use crate::model::drawing::Display;
    let mut out: Vec<(&[Block], Option<PartId>)> = Vec::new();
    fn push<'b>(out: &mut Vec<(&'b [Block], Option<PartId>)>, d: Option<&'b Display>) {
        match d {
            Some(Display::Drawing(d)) => {
                out.extend(d.shapes.iter().map(|s| (s.content.as_slice(), s.content_part)));
            }
            Some(Display::Vml(v)) => {
                out.extend(v.shapes.iter().map(|s| (s.content.as_slice(), None)));
            }
            Some(Display::Formula(_)) | None => {}
        }
    }
    match block {
        Block::Text(t) => {
            for i in &t.inlines {
                let crate::model::Inline::Run(r) = i else { continue };
                for seg in &r.segments {
                    push(&mut out, seg.display.as_ref());
                }
            }
        }
        Block::Image(b) => push(&mut out, b.display.as_ref()),
        Block::Protected(b) => push(&mut out, b.display.as_ref()),
        Block::Table(_) => {}
    }
    out.retain(|(blocks, _)| !blocks.is_empty());
    out
}

/// 深搜（表格 → 单元格，文本框 → 内容流）找 `part` 里的段落 `para`。`here` 是 `blocks` 所属的 part。
fn text_block_deep(
    blocks: &[Block],
    here: PartId,
    part: PartId,
    para: NodeId,
) -> Option<&TextBlock> {
    for b in Blocks::over(blocks) {
        if here == part
            && b.node() == para
            && let Some(t) = b.as_text()
        {
            return Some(t);
        }
        for (content, cpart) in box_flows(b) {
            let hit = text_block_deep(content, cpart.unwrap_or(here), part, para);
            if hit.is_some() {
                return hit;
            }
        }
    }
    None
}

impl<'a> Blocks<'a> {
    /// 任意块列表的深度遍历（页眉页脚 part、注释 / 批注条目、文本框内容流都用它）。
    pub fn over(blocks: &'a [Block]) -> Blocks<'a> {
        Blocks { stack: blocks.iter().rev().collect() }
    }
}

impl Document {
    /// 全部块，文档序，深入单元格（嵌套表也算）。
    pub fn blocks(&self) -> Blocks<'_> {
        Blocks::over(&self.main)
    }

    /// 某个 part 的顶层块列表：主 part 是正文，其余是页眉页脚 part 或注释 / 批注条目
    /// （一个 part 里所有条目的块按文档序接起来）。找不到这个 part → `None`。
    pub fn blocks_of_part(&self, part: PartId) -> Option<Vec<&Block>> {
        if part == self.main_part {
            return Some(self.main.iter().collect());
        }
        if let Some(hf) = self.hf_parts.get(&part) {
            return Some(hf.blocks.iter().collect());
        }
        for notes in [&self.footnotes, &self.endnotes] {
            if notes.part == Some(part) {
                return Some(notes.items.iter().flat_map(|n| n.blocks.iter()).collect());
            }
        }
        if self.comments.part == Some(part) {
            return Some(self.comments.items.iter().flat_map(|c| c.blocks.iter()).collect());
        }
        None
    }

    /// 某个 part 的字段索引（`FLD-02`）：主 part 是 `fields`，辅助 part 在它自己的
    /// `AuxFlows` 里（页眉页脚 / 注释 / 批注 / 外部文本框 part）。找不到 → `None`。
    pub fn fields_in(&self, part: PartId) -> Option<&crate::span::field::FieldIndex> {
        if part == self.main_part {
            return Some(&self.fields);
        }
        if let Some(hf) = self.hf_parts.get(&part) {
            return Some(&hf.idx.fields);
        }
        for notes in [&self.footnotes, &self.endnotes] {
            if notes.part == Some(part) {
                return notes.idx.as_ref().map(|i| &i.fields);
            }
        }
        if self.comments.part == Some(part) {
            return self.comments.idx.as_ref().map(|i| &i.fields);
        }
        self.aux_flows.get(&part).map(|i| &i.fields)
    }

    /// 任意 part 里的文本段落（含单元格内任意深度、**文本框内容流**），按 part + 节点找
    /// （`EDIT-02`）。文本框里的段落是独立内容流，不在 [`Blocks`] 的平铺里，所以要单独下去
    /// （`spec/18` 7.7：`InlinePos.para` 任意深度）。
    pub fn text_block_in(&self, part: PartId, para: NodeId) -> Option<&TextBlock> {
        // 先走平铺（正文 / 单元格）：绝大多数位置在这里就命中，代价与文本框那条路无关
        if let Some(tops) = self.blocks_of_part(part)
            && let Some(hit) = tops
                .into_iter()
                .find_map(|b| Blocks::over(std::slice::from_ref(b)).find(|x| x.node() == para))
                .and_then(Block::as_text)
        {
            return Some(hit);
        }
        // 没命中才下到框里。宿主块可能在别的 part（页眉里的文本框），所以每个 part 都走一遍
        let mut parts = vec![self.main_part];
        parts.extend(self.hf_parts.keys().copied());
        parts
            .extend([self.footnotes.part, self.endnotes.part, self.comments.part].iter().flatten());
        for here in parts {
            let Some(tops) = self.blocks_of_part(here) else { continue };
            for b in tops {
                for (content, cpart) in box_flows(b) {
                    let hit = text_block_deep(content, cpart.unwrap_or(here), part, para);
                    if hit.is_some() {
                        return hit;
                    }
                }
                if let Block::Table(_) = b {
                    for x in Blocks::over(std::slice::from_ref(b)) {
                        for (content, cpart) in box_flows(x) {
                            let hit = text_block_deep(content, cpart.unwrap_or(here), part, para);
                            if hit.is_some() {
                                return hit;
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// 全部可编辑段落，含单元格内任意深度的。`text_blocks()` 仍只给顶层的。
    pub fn paragraphs(&self) -> impl Iterator<Item = &TextBlock> {
        self.blocks().filter_map(Block::as_text)
    }

    /// 全部表格，含嵌套表。
    pub fn tables(&self) -> impl Iterator<Item = &TableBlock> {
        self.blocks().filter_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
    }

    /// 从顶层到 `node` 所在块的路径；`node` 不是任何块的节点时 `None`。
    pub fn block_path(&self, node: NodeId) -> Option<Vec<BlockStep>> {
        let mut stack: Vec<(&Block, Vec<BlockStep>)> = self
            .main
            .iter()
            .enumerate()
            .rev()
            .map(|(i, b)| (b, vec![BlockStep::Main(i)]))
            .collect();
        while let Some((b, path)) = stack.pop() {
            if b.node() == node {
                return Some(path);
            }
            if let Block::Table(t) = b {
                for (ri, row) in t.rows.iter().enumerate().rev() {
                    for (ci, cell) in row.cells.iter().enumerate().rev() {
                        for (bi, inner) in cell.blocks.iter().enumerate().rev() {
                            let mut p = path.clone();
                            p.push(BlockStep::Cell { row: ri, cell: ci, block: bi });
                            stack.push((inner, p));
                        }
                    }
                }
            }
        }
        None
    }

    /// 按路径取块。
    pub fn block_at(&self, path: &[BlockStep]) -> Option<&Block> {
        let (first, rest) = path.split_first()?;
        let BlockStep::Main(i) = first else { return None };
        let mut cur = self.main.get(*i)?;
        for step in rest {
            let BlockStep::Cell { row, cell, block } = step else { return None };
            let Block::Table(t) = cur else { return None };
            cur = t.rows.get(*row)?.cells.get(*cell)?.blocks.get(*block)?;
        }
        Some(cur)
    }

    /// 按路径取块（可变）。
    pub fn block_at_mut(&mut self, path: &[BlockStep]) -> Option<&mut Block> {
        block_at_mut_in(&mut self.main, path)
    }
}
