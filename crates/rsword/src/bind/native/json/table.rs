//! 表格模型的 JSON 投影（`BIND-02`；`MOD-07` 表格、`MOD-09` 表格修订）。投影的是声明值：
//! `grid` 允许 0 / `Raw` / 缺失，`tblPrEx` 不折叠、`tcW` 不校正——折叠与校正是 `RES-08`
//! （`resolve`）与 `compat_ts` 的事。行 / 格的修订包裹与 `*PropsChange` 随各表投影。

use crate::model::table::{Cell, GridCol, Row, TableBlock};
use crate::model::{Block, Revision, SdtInfo};
use crate::semantic::props::{CellProps, RowProps, TableProps, Val};
use crate::xml::NodeId;

use super::model_json;

model_json! {
    /// `w:tbl`（`MOD-07`）。
    struct TableBlock(cx) test json_fields_cover_table_block {
        node => "node", NodeId = node;
        /// `w:tblPr` 声明值。
        props => "props", Box<TableProps> = props;
        /// `w:tblGrid/w:gridCol` 声明值。
        grid => "grid", Vec<GridCol> = grid;
        rows => "rows", Vec<Row> = rows;
        /// `tblPr/tblStyle`。
        opt style_id => "styleId", String = style_id;
        opt sdt => "sdt", SdtInfo = sdt;
        /// 块级包裹修订 + `TablePropsChange` / `TableGridChange`。
        revisions => "revisions", Vec<Revision> = revisions;
    }
}

model_json! {
    /// `w:gridCol`：`w` 是声明值，可以是 0、`Raw`，也可以缺失。
    struct GridCol(cx) test json_fields_cover_grid_col {
        node => "node", NodeId = node;
        opt w => "w", Val<i32> = w;
    }
}

model_json! {
    /// `w:tr`。
    struct Row(cx) test json_fields_cover_row {
        node => "node", NodeId = node;
        props => "props", Box<RowProps> = props;
        /// `w:tblPrEx`：行级表格属性例外，该行优先于 `tblPr`（`RES-08`）。
        opt tbl_pr_ex => "tblPrEx", Box<TableProps> = tbl_pr_ex;
        cells => "cells", Vec<Cell> = cells;
        /// 包裹这一行的 `w:sdt`。
        opt sdt => "sdt", SdtInfo = sdt;
        /// 包裹修订 + `trPr/ins|del`（整行）+ `RowPropsChange`。
        revisions => "revisions", Vec<Revision> = revisions;
    }
}

model_json! {
    /// `w:tc`。
    struct Cell(cx) test json_fields_cover_cell {
        node => "node", NodeId = node;
        props => "props", Box<CellProps> = props;
        /// 单元格内容，与正文同一构建器。
        blocks => "blocks", Vec<Block> = blocks;
        opt sdt => "sdt", SdtInfo = sdt;
        /// 包裹修订 + `CellInsert` / `CellDelete` / `CellMerge` + `CellPropsChange`。
        revisions => "revisions", Vec<Revision> = revisions;
    }
}
