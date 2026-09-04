//! 表格有效属性视图（`RES-08`，任务 3.4）。
//!
//! 三件事：`tblLook` 六个开关、表格样式链的条件格式（`firstRow` > `lastRow` > `firstCol` >
//! `lastCol` > 条带 > 整表，单元格自身声明优先于一切）、以及列宽视图 [`ColumnView`]。
//!
//! **全部是只读视图**：`hMerge` 折叠、`tcW` 校正、网格修复、`trHeight` 截断都只出现在这里，
//! 模型保持声明值（`MOD-07`）。列宽的四条启发式照抄 TS（`docs/01` §7.2），每条都标了来源，
//! 调用方能知道这个宽度是从 `tblGrid` 来的还是被改过的。

use std::collections::BTreeMap;

use crate::model::table::{Cell, Row, TableBlock};
use crate::model::{Style, StyleType, TableStylePr};
use crate::resolve::{Effective, Provenance, Resolver};
use crate::semantic::props::{
    CellProps, CellPropsField, Ctx, HeightRule, ParaProps, RowProps, RunProps, TableProps,
    TblBorders, TblCellMar, TblLayoutType, TblLook, TblStyleOverrideType, TblWidth, TcBorders, Val,
    merge_cell_props, merge_para_props, merge_row_props, merge_run_props, merge_table_props,
    merge_tbl_borders, merge_tc_borders, read_attr, read_tbl_borders, read_tc_borders,
};
use crate::xml::{Dom, LocalName, NodeId, QName};

/// 边界吸附容差（twips）：各行累计出的列边界差在这个数以内算同一条网格线（生成器逐行写宽度会有
/// 舍入漂移）。与 TS 的 `GRID_SNAP_TOL` 一致。
const GRID_SNAP_TOL: i64 = 20;

/// Word 的网格列数上限；并集超过这个数说明输入本身是坏的，放弃修复。
const MAX_RECONCILED_COLUMNS: usize = 96;

/// `w:trHeight` 的上限（22 英寸，[MS-OI29500] 2.1.51）。
pub const MAX_ROW_HEIGHT_TWIPS: i32 = 31680;

/// `w:tblLook` 的六个条件格式开关（`RES-08`）。
///
/// 属性形式优先于 `w:val` 位掩码；两者都没有时用 Word 的缺省（等价于 `w:val="04A0"`：
/// 首行、首列、无纵向条带）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TblLookFlags {
    pub first_row: bool,
    pub last_row: bool,
    pub first_column: bool,
    pub last_column: bool,
    pub banded_rows: bool,
    pub banded_columns: bool,
}

impl Default for TblLookFlags {
    fn default() -> Self {
        TblLookFlags {
            first_row: true,
            last_row: false,
            first_column: true,
            last_column: false,
            banded_rows: true,
            banded_columns: false,
        }
    }
}

impl TblLookFlags {
    pub fn read(look: Option<&TblLook>) -> TblLookFlags {
        let Some(look) = look else { return TblLookFlags::default() };
        let bits = look
            .val
            .as_deref()
            .map(str::trim)
            .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());
        let flag = |attr: Option<bool>, bit: u32, dflt: bool| match (attr, bits) {
            (Some(v), _) => v,
            (None, Some(b)) => b & bit != 0,
            (None, None) => dflt,
        };
        TblLookFlags {
            first_row: flag(look.first_row, 0x20, true),
            last_row: flag(look.last_row, 0x40, false),
            first_column: flag(look.first_column, 0x80, true),
            last_column: flag(look.last_column, 0x100, false),
            banded_rows: !flag(look.no_h_band, 0x200, false),
            banded_columns: !flag(look.no_v_band, 0x400, true),
        }
    }
}

/// 列宽视图的来源（`RES-08`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnSource {
    /// `w:tblGrid` 的声明值。
    Grid,
    /// 各行 `w:tcW` 与 grid 不一致，以 tcW 为准（生成器常留下过时的均分 grid）。
    TcW,
    /// grid 总宽明显小于 `w:tblW`，按比例拉伸到 `tblW`。
    Stretched,
    /// 各行的网格宽度对不上，用各行累计边界的并集重算列与跨度。
    Reconciled,
    /// 既没有 grid 也没有可用的 `tcW`。
    None,
}

/// 折叠 `hMerge` 并补上 `gridBefore` / `gridAfter` 占位之后的一格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewCell {
    /// 对应 `Row.cells` 的下标；占位格没有物理单元格。
    pub cell: Option<usize>,
    /// 占的网格列数。
    pub span: u16,
    /// `gridBefore` / `gridAfter` 的显示占位（不是 `w:tc`，不参与条件格式）。
    pub gap: bool,
}

/// 列宽与各行的格布局（`RES-08`）。
///
/// 绝对宽与百分比宽**分开**：`tblGrid` 里有 0 宽列时只有百分比宽可用；拉伸（`Stretched`）只改绝对宽，
/// 百分比仍是拉伸前的比例——TS 就是这么算的，两者不是同一个数组的两种单位。
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnView {
    /// 每列宽度（twips）；没有可用来源时为空。
    pub widths_twips: Vec<i32>,
    /// 每列宽度占表宽的百分比；没有可用来源时为空。
    pub widths_pct: Vec<f64>,
    pub source: ColumnSource,
    /// 每行的格视图，与 `TableBlock.rows` 一一对应。
    pub rows: Vec<Vec<ViewCell>>,
}

impl ColumnView {
    /// 列数：优先按百分比宽的长度（它比绝对宽更常有值）。
    pub fn column_count(&self) -> usize {
        if self.widths_pct.is_empty() { self.widths_twips.len() } else { self.widths_pct.len() }
    }
}

/// 按总和归一成百分比；总宽 ≤ 0 → 空。
fn to_pct(widths: &[i32]) -> Vec<f64> {
    let total: i64 = widths.iter().map(|&w| i64::from(w)).sum();
    if total <= 0 {
        return Vec::new();
    }
    widths.iter().map(|&w| f64::from(w) * 100.0 / total as f64).collect()
}

/// 表格样式一层（整表或一种条件格式）叠出来的属性。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableStyleLayer {
    pub tbl_pr: TableProps,
    pub tr_pr: RowProps,
    pub tc_pr: CellProps,
    pub rpr: RunProps,
    pub ppr: ParaProps,
}

impl TableStyleLayer {
    fn merge(&mut self, over: &TableStyleLayer) {
        merge_table_props(&mut self.tbl_pr, &over.tbl_pr);
        merge_row_props(&mut self.tr_pr, &over.tr_pr);
        merge_cell_props(&mut self.tc_pr, &over.tc_pr);
        merge_run_props(&mut self.rpr, &over.rpr);
        merge_para_props(&mut self.ppr, &over.ppr);
    }

    fn from_style(s: &Style) -> TableStyleLayer {
        TableStyleLayer {
            tbl_pr: s.tbl_pr.clone().unwrap_or_default(),
            tr_pr: s.tr_pr.clone().unwrap_or_default(),
            tc_pr: s.tc_pr.clone().unwrap_or_default(),
            rpr: s.rpr.clone().unwrap_or_default(),
            ppr: s.ppr.clone().unwrap_or_default(),
        }
    }

    fn from_conditional(c: &TableStylePr) -> TableStyleLayer {
        TableStyleLayer {
            tbl_pr: c.tbl_pr.clone().unwrap_or_default(),
            tr_pr: c.tr_pr.clone().unwrap_or_default(),
            tc_pr: c.tc_pr.clone().unwrap_or_default(),
            rpr: c.rpr.clone().unwrap_or_default(),
            ppr: c.ppr.clone().unwrap_or_default(),
        }
    }
}

/// 表格样式链解析后的整表层与条件层（`RES-08`）。basedOn 链根 → 叶层叠，同一 `w:type` 的条件块也逐层叠。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableStyleView {
    pub style_id: Option<String>,
    pub whole: TableStyleLayer,
    pub conditional: BTreeMap<TblStyleOverrideType, TableStyleLayer>,
}

impl TableStyleView {
    pub fn layer(&self, cond: TblStyleOverrideType) -> Option<&TableStyleLayer> {
        self.conditional.get(&cond)
    }
}

/// 一格生效的属性与来源（`RES-08`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveCellProps {
    pub props: CellProps,
    sources: Vec<Option<Provenance>>,
    /// 条件格式叠出来的 run 属性（`RES-03` 第 4 层：表格内 run 的样式层）。
    pub rpr: RunProps,
    /// 条件格式叠出来的段落属性。
    pub ppr: ParaProps,
    /// 命中的条件格式，优先级从高到低。
    pub conditions: Vec<TblStyleOverrideType>,
}

impl EffectiveCellProps {
    pub fn source(&self, field: CellPropsField) -> Provenance {
        self.sources[field as usize].clone().unwrap_or(Provenance::Default)
    }
}

/// 折叠后一行里各真实格的 (`Row.cells` 下标, 起始列, 跨度)：条件格式的首列 / 末列判定用。
type StyleCols = Vec<Vec<(usize, usize, u16)>>;

/// 一张表的只读视图。
pub struct TableView<'a> {
    dom: &'a Dom,
    table: &'a TableBlock,
    look: TblLookFlags,
    style: TableStyleView,
    columns: ColumnView,
    /// 每行折叠后各真实格的 (`Row.cells` 下标, 列位置, 跨度)，用于条件格式的首列 / 末列判定。
    style_cols: StyleCols,
    /// 条件格式用的总列数：各行真实格跨度之和的最大值（不含 `gridBefore` / `gridAfter`）。
    style_total_cols: u16,
}

impl<'a> TableView<'a> {
    pub fn table(&self) -> &'a TableBlock {
        self.table
    }

    pub fn look(&self) -> TblLookFlags {
        self.look
    }

    pub fn style(&self) -> &TableStyleView {
        &self.style
    }

    pub fn columns(&self) -> &ColumnView {
        &self.columns
    }

    /// 行高（`w:trHeight`），按 Word 上限截断；未声明 → `None`。
    pub fn row_height(&self, row: usize) -> Option<(i32, HeightRule)> {
        let h = self.table.rows.get(row)?.props.height.as_ref()?;
        let v = h.val.as_ref().and_then(Val::value).copied().filter(|&v| v > 0)?;
        let rule = h
            .h_rule
            .as_ref()
            .and_then(Val::value)
            .copied()
            .filter(|r| *r == HeightRule::Exact)
            .unwrap_or(HeightRule::AtLeast);
        Some((v.min(MAX_ROW_HEIGHT_TWIPS), rule))
    }

    /// 表格边框：重复的 `w:tblBorders` 容器按边合并（后者胜，同 `RES-07` 对 `pBdr` 的规则），
    /// 文档 `tblPr` 未声明时回退到样式链（`RES-08`）。
    pub fn borders(&self) -> Option<Effective<TblBorders>> {
        let pr = self.child(self.table.node, LocalName::TblPr);
        match merged_borders(
            self.dom,
            pr,
            LocalName::TblBorders,
            read_tbl_borders,
            merge_tbl_borders,
        ) {
            Some(value) => Some(Effective { value, source: Provenance::Direct }),
            None => self
                .style
                .whole
                .tbl_pr
                .borders
                .clone()
                .map(|value| Effective { value, source: self.style_provenance(None) }),
        }
    }

    /// 一格的边框：重复的 `w:tcBorders` 同样按边合并；都没有 → `None`（用表格级 / 样式的）。
    pub fn cell_borders(&self, row: usize, cell: usize) -> Option<TcBorders> {
        let node = self.table.rows.get(row)?.cells.get(cell)?.node;
        let pr = self.child(node, LocalName::TcPr);
        merged_borders(self.dom, pr, LocalName::TcBorders, read_tc_borders, merge_tc_borders)
    }

    fn child(&self, parent: NodeId, local: LocalName) -> Option<NodeId> {
        self.dom.semantic_children(parent).find(|&n| self.dom.is(n, QName::w(local)))
    }

    /// 表格级单元格边距：同 [`Self::borders`] 的回退。
    pub fn cell_margins(&self) -> Option<Effective<TblCellMar>> {
        match &self.table.props.cell_margins {
            Some(m) => Some(Effective { value: m.clone(), source: Provenance::Direct }),
            None => self
                .style
                .whole
                .tbl_pr
                .cell_margins
                .clone()
                .map(|m| Effective { value: m, source: self.style_provenance(None) }),
        }
    }

    fn style_provenance(&self, cond: Option<TblStyleOverrideType>) -> Provenance {
        Provenance::TableStyle { style: self.style.style_id.clone().unwrap_or_default(), cond }
    }

    /// 一格的条件格式命中顺序（高 → 低）：首行 > 末行 > 首列 > 末列 > 条带 > 整表。
    /// `cell` 是 `Row.cells` 的下标（被折叠掉的 `hMerge continue` 不参与）。
    pub fn conditions(&self, row: usize, cell: usize) -> Vec<TblStyleOverrideType> {
        use TblStyleOverrideType as T;
        let mut out = Vec::new();
        let Some(cols) = self.style_cols.get(row) else { return out };
        // 被折叠掉的 hMerge continue 不参与条件格式
        let Some(&(_, col, span)) = cols.iter().find(|&&(i, _, _)| i == cell) else { return out };
        let rows = self.table.rows.len();
        if self.look.first_row && row == 0 {
            out.push(T::FirstRow);
        }
        if self.look.last_row && row + 1 == rows {
            out.push(T::LastRow);
        }
        if self.look.first_column && col == 0 {
            out.push(T::FirstCol);
        }
        if self.look.last_column && col + usize::from(span) == usize::from(self.style_total_cols) {
            out.push(T::LastCol);
        }
        if self.look.banded_rows {
            // 条带行号从 firstRow 之后起算；偶数 band1，奇数 band2
            let band = if self.look.first_row { row as i64 - 1 } else { row as i64 };
            if band >= 0 {
                out.push(if band % 2 == 0 { T::Band1Horz } else { T::Band2Horz });
            }
        }
        if self.look.banded_columns {
            let band = if self.look.first_column { col as i64 - 1 } else { col as i64 };
            if band >= 0 {
                out.push(if band % 2 == 0 { T::Band1Vert } else { T::Band2Vert });
            }
        }
        out
    }

    /// 一格的有效属性：整表层 → 条件层（低优先级先叠）→ 单元格自身声明。
    pub fn cell(&self, row: usize, cell: usize) -> EffectiveCellProps {
        let n = CellPropsField::ALL.len();
        let mut props = CellProps::default();
        let mut sources: Vec<Option<Provenance>> = vec![None; n];
        let mut rpr = RunProps::default();
        let mut ppr = ParaProps::default();
        let conditions = self.conditions(row, cell);

        let mut layers: Vec<(Option<TblStyleOverrideType>, &TableStyleLayer)> =
            vec![(None, &self.style.whole)];
        // conditions 是高 → 低，叠加要低 → 高
        for cond in conditions.iter().rev() {
            if let Some(layer) = self.style.layer(*cond) {
                layers.push((Some(*cond), layer));
            }
        }
        for (cond, layer) in layers {
            let prov = self.style_provenance(cond);
            for f in merge_cell_props(&mut props, &layer.tc_pr) {
                sources[f as usize] = Some(prov.clone());
            }
            merge_run_props(&mut rpr, &layer.rpr);
            merge_para_props(&mut ppr, &layer.ppr);
        }
        if let Some(own) = self.table.rows.get(row).and_then(|r| r.cells.get(cell)) {
            for f in merge_cell_props(&mut props, &own.props) {
                sources[f as usize] = Some(Provenance::Direct);
            }
        }
        EffectiveCellProps { props, sources, rpr, ppr, conditions }
    }
}

impl Resolver<'_> {
    /// 一张表的有效属性视图（`RES-08`）。需要 `dom`：重复声明的取舍（`w:tcW` 取最后一个、
    /// 边框容器按边合并）要看模型去重时留在 `raw_unmodeled` 里的那些元素。
    pub fn table<'t>(&self, dom: &'t Dom, table: &'t TableBlock) -> TableView<'t> {
        let look = TblLookFlags::read(table.props.look.as_ref());
        let style = self.table_style(table.style_id.as_deref());
        let (columns, style_cols, style_total_cols) = columns(dom, table);
        TableView { dom, table, look, style, columns, style_cols, style_total_cols }
    }

    /// 表格样式链（basedOn，根 → 叶层叠）解析出的整表层与条件层。
    pub fn table_style(&self, style_id: Option<&str>) -> TableStyleView {
        let mut view =
            TableStyleView { style_id: style_id.map(str::to_owned), ..Default::default() };
        let Some(id) = style_id else { return view };
        let chain = self.chain(id, StyleType::Table);
        for s in chain.iter().rev() {
            view.whole.merge(&TableStyleLayer::from_style(s));
            for c in &s.conditional {
                let Some(kind) = c.kind.as_ref().and_then(Val::value).copied() else { continue };
                view.conditional
                    .entry(kind)
                    .or_default()
                    .merge(&TableStyleLayer::from_conditional(c));
            }
        }
        view
    }
}

/// 列宽与各行格布局。返回 (视图, 每行真实格的列位置, 条件格式用的总列数)。
fn columns(dom: &Dom, table: &TableBlock) -> (ColumnView, StyleCols, u16) {
    {
        // ① 折叠 hMerge continue：并进左邻格，tcW 相加
        let mut folded: Vec<Vec<(usize, u16, Option<i32>)>> = Vec::new();
        for row in &table.rows {
            let mut cells: Vec<(usize, u16, Option<i32>)> = Vec::new();
            for (i, c) in row.cells.iter().enumerate() {
                let w = cell_tcw(dom, c);
                if c.is_hmerge_continue()
                    && let Some(prev) = cells.last_mut()
                {
                    prev.1 += c.grid_span() as u16;
                    prev.2 = match (prev.2, w) {
                        (Some(a), Some(b)) => Some(a + b),
                        _ => None,
                    };
                    continue;
                }
                cells.push((i, c.grid_span() as u16, w));
            }
            folded.push(cells);
        }
        let style_cols: StyleCols = folded
            .iter()
            .map(|cells| {
                let mut out = Vec::new();
                let mut col = 0usize;
                for &(i, span, _) in cells {
                    out.push((i, col, span));
                    col += usize::from(span);
                }
                out
            })
            .collect();
        let style_total_cols =
            folded.iter().map(|cells| cells.iter().map(|c| c.1).sum::<u16>()).max().unwrap_or(0);

        // ② 声明网格（原始值，含 0 宽列）。百分比按它算；绝对宽只在每列都 > 0 时可用。
        let grid: Vec<i32> = table
            .grid
            .iter()
            .map(|g| g.w.as_ref().and_then(Val::value).copied().unwrap_or(0))
            .collect();
        let grid_total: i64 = grid.iter().map(|&w| i64::from(w)).sum();
        let grid_raw = (grid_total > 0).then_some(grid);
        let mut pct: Vec<f64> = grid_raw.as_deref().map(to_pct).unwrap_or_default();
        let mut twips: Vec<i32> = grid_raw
            .as_deref()
            .filter(|g| g.iter().all(|&w| w > 0))
            .map(<[i32]>::to_vec)
            .unwrap_or_default();
        let mut source = if pct.is_empty() { ColumnSource::None } else { ColumnSource::Grid };

        let fixed = matches!(
            table.props.layout.as_ref().and_then(|l| l.kind.as_ref()).and_then(Val::value),
            Some(TblLayoutType::Fixed)
        );
        let edges: Vec<(u16, u16, Option<i32>, Option<i32>)> =
            table.rows.iter().map(row_grid_edges).collect();

        // ③ 各行 tcW 推出的列宽；与 grid 不一致（列数不同 / 任一列差 > 2 个百分点 /
        //    fixed 布局下总宽差 > 列数）时以 tcW 为准——生成器常留下过时的均分 grid
        if let Some(tcw) = tcw_column_widths(&folded, &edges) {
            let tcw_total: i64 = tcw.iter().map(|&w| i64::from(w)).sum();
            let tcw_pct = to_pct(&tcw);
            let twips_total: i64 = twips.iter().map(|&w| i64::from(w)).sum();
            let disagree = pct.is_empty()
                || pct.len() != tcw_pct.len()
                || pct.iter().zip(&tcw_pct).any(|(a, b)| (a - b).abs() > 2.0)
                || (fixed && (twips_total - tcw_total).abs() > tcw.len() as i64);
            if disagree {
                pct = tcw_pct;
                twips = tcw;
                source = ColumnSource::TcW;
            }
        }

        // ④ 占位网格：grid 总宽明显小于 `w:tblW`（dxa）→ 按比例拉伸绝对宽（百分比不动）
        if !fixed && !twips.is_empty() {
            let tbl_w = table.props.width.as_ref().and_then(|w| w.twips()).unwrap_or(0);
            let total: i64 = twips.iter().map(|&w| i64::from(w)).sum();
            if tbl_w > 0 && total > 0 && total < i64::from(tbl_w) - twips.len() as i64 {
                let scale = f64::from(tbl_w) / total as f64;
                twips = twips.iter().map(|&w| (f64::from(w) * scale).round() as i32).collect();
                source = ColumnSource::Stretched;
            }
        }

        // ⑤ 补 gridBefore / gridAfter 占位（TS：在条件格式之后补，所以不进 style_cols）
        let mut rows: Vec<Vec<ViewCell>> = folded
            .iter()
            .map(|cells| {
                cells
                    .iter()
                    .map(|&(i, span, _)| ViewCell { cell: Some(i), span, gap: false })
                    .collect()
            })
            .collect();
        let mut row_widths: Vec<Vec<Option<i32>>> =
            folded.iter().map(|cells| cells.iter().map(|c| c.2).collect()).collect();
        for (r, &(before, after, w_before, w_after)) in edges.iter().enumerate() {
            if before > 0 {
                rows[r].insert(0, ViewCell { cell: None, span: before, gap: true });
                row_widths[r].insert(0, w_before);
            }
            if after > 0 {
                rows[r].push(ViewCell { cell: None, span: after, gap: true });
                row_widths[r].push(w_after);
            }
        }

        // ⑥ 各行网格宽度对不上 → 用累计边界的并集重算列与跨度。**用原始 grid**（不是 ③ 的结果）：
        //    列数与缺宽格的回退宽度都以声明网格为准，TS 同
        if let Some(fixup) = reconcile_grid_columns(&rows, &row_widths, grid_raw.as_deref()) {
            for (r, spans) in fixup.spans.iter().enumerate() {
                for (c, &span) in spans.iter().enumerate() {
                    rows[r][c].span = span;
                }
            }
            pct = to_pct(&fixup.widths);
            twips = fixup.widths;
            source = ColumnSource::Reconciled;
        }

        (
            ColumnView { widths_twips: twips, widths_pct: pct, source, rows },
            style_cols,
            style_total_cols,
        )
    }
}

/// `trPr` 的 `gridBefore` / `gridAfter` 与它们的宽度。
fn row_grid_edges(row: &Row) -> (u16, u16, Option<i32>, Option<i32>) {
    let count = |v: &Option<Val<i32>>| {
        v.as_ref().and_then(Val::value).copied().filter(|&n| n > 0).unwrap_or(0) as u16
    };
    let width = |w: &Option<crate::semantic::props::TblWidth>| {
        w.as_ref().and_then(|w| w.twips()).filter(|&w| w > 0)
    };
    (
        count(&row.props.grid_before),
        count(&row.props.grid_after),
        width(&row.props.width_before),
        width(&row.props.width_after),
    )
}

/// 各行未跨列的格的 `w:tcW` 每列取最大值；必须每列都有值，否则放弃（TS `tcwColumnWidths`）。
fn tcw_column_widths(
    folded: &[Vec<(usize, u16, Option<i32>)>],
    edges: &[(u16, u16, Option<i32>, Option<i32>)],
) -> Option<Vec<i32>> {
    let mut cols: Vec<i32> = Vec::new();
    let mut col_count = 0usize;
    for (cells, &(before, after, _, _)) in folded.iter().zip(edges) {
        let mut idx = usize::from(before);
        for &(_, span, w) in cells {
            if span == 1
                && let Some(w) = w
                && w > 0
            {
                if cols.len() <= idx {
                    cols.resize(idx + 1, 0);
                }
                cols[idx] = cols[idx].max(w);
            }
            idx += usize::from(span);
        }
        col_count = col_count.max(idx + usize::from(after));
    }
    if col_count == 0 {
        return None;
    }
    if cols.len() < col_count || cols[..col_count].iter().any(|&w| w <= 0) {
        return None;
    }
    Some(cols[..col_count].to_vec())
}

struct GridFixup {
    widths: Vec<i32>,
    spans: Vec<Vec<u16>>,
}

/// 各行网格宽度与声明列数不一致时，用各行累计右边界的并集重算列宽与每格跨度
/// （TS `reconcileGridColumns`）。修不了就返回 `None`，宁可不改。
fn reconcile_grid_columns(
    rows: &[Vec<ViewCell>],
    row_widths: &[Vec<Option<i32>>],
    grid: Option<&[i32]>,
) -> Option<GridFixup> {
    let span_sums: Vec<usize> =
        rows.iter().map(|r| r.iter().map(|c| usize::from(c.span)).sum()).collect();
    let col_count = grid.map_or_else(|| span_sums.iter().copied().max().unwrap_or(0), <[i32]>::len);
    if span_sums.iter().all(|&s| s == col_count) {
        return None;
    }
    let mut bounds: Vec<Vec<i64>> = Vec::with_capacity(rows.len());
    for (r, cells) in rows.iter().enumerate() {
        let mut row_bounds = Vec::with_capacity(cells.len());
        let mut x = 0i64;
        let mut pos = 0usize;
        for (c, cell) in cells.iter().enumerate() {
            let span = usize::from(cell.span);
            let w = match row_widths[r][c].filter(|&w| w > 0) {
                Some(w) => i64::from(w),
                None => {
                    let g = grid?;
                    let end = (pos + span).min(g.len());
                    let sum: i64 = g.get(pos..end)?.iter().map(|&w| i64::from(w)).sum();
                    if sum <= 0 {
                        return None;
                    }
                    sum
                }
            };
            x += w;
            row_bounds.push(x);
            pos += span;
        }
        bounds.push(row_bounds);
    }
    let mut sorted: Vec<i64> = bounds.iter().flatten().copied().collect();
    sorted.sort_unstable();
    let mut reps: Vec<i64> = Vec::new();
    for b in sorted {
        if reps.last().is_none_or(|&last| b - last > GRID_SNAP_TOL) {
            reps.push(b);
        }
    }
    if reps.is_empty() || reps.len() > MAX_RECONCILED_COLUMNS {
        return None;
    }
    let rep_index = |b: i64| -> Option<usize> {
        let i = reps.partition_point(|&r| r <= b).checked_sub(1)?;
        (b - reps[i] <= GRID_SNAP_TOL).then_some(i)
    };
    let mut spans: Vec<Vec<u16>> = Vec::with_capacity(rows.len());
    for row_bounds in &bounds {
        let mut row_spans = Vec::with_capacity(row_bounds.len());
        let mut prev: i64 = -1;
        for &b in row_bounds {
            let idx = rep_index(b)? as i64;
            if idx <= prev {
                return None;
            }
            row_spans.push(u16::try_from(idx - prev).ok()?);
            prev = idx;
        }
        spans.push(row_spans);
    }
    let widths = reps
        .iter()
        .enumerate()
        .map(|(i, &v)| i32::try_from(v - if i > 0 { reps[i - 1] } else { 0 }).unwrap_or(0))
        .collect();
    Some(GridFixup { widths, spans })
}

/// 一格生效的 `w:tcW`：**取最后一个**（Word 与 TS 的规则；模型按属性表的通则取第一个，
/// 重复的进了 `raw_unmodeled`）。只认 `dxa` 或缺省类型的正值。
fn cell_tcw(dom: &Dom, cell: &Cell) -> Option<i32> {
    let pr = dom.semantic_children(cell.node).find(|&n| dom.is(n, QName::w(LocalName::TcPr)))?;
    let last = dom.semantic_children(pr).filter(|&n| dom.is(n, QName::w(LocalName::TcW))).last()?;
    let mut diags = Vec::new();
    let mut ctx = Ctx::new(dom, &mut diags);
    ctx.enter(last);
    let w = TblWidth {
        w: read_attr::<crate::semantic::props::codec::MeasureOrPercent>(
            last,
            QName::w(LocalName::W),
            None,
            &mut ctx,
        ),
        kind: read_attr::<crate::semantic::props::TblWidthType>(
            last,
            QName::w(LocalName::UType),
            None,
            &mut ctx,
        ),
    };
    w.twips().filter(|&w| w > 0)
}

/// 容器里重复出现的边框容器按边合并，后者胜；一个都没有 → `None`。
fn merged_borders<T: Default, F, M, R>(
    dom: &Dom,
    container: Option<NodeId>,
    local: LocalName,
    read: F,
    merge: M,
) -> Option<T>
where
    F: Fn(&Dom, Option<NodeId>, &mut Vec<crate::diag::Diagnostic>) -> T,
    M: Fn(&mut T, &T) -> Vec<R>,
{
    let pr = container?;
    let name = QName::w(local);
    let mut found = false;
    let mut out = T::default();
    let mut diags = Vec::new();
    for n in dom.semantic_children(pr).filter(|&n| dom.is(n, name)) {
        let one = read(dom, Some(n), &mut diags);
        merge(&mut out, &one);
        found = true;
    }
    found.then_some(out)
}
