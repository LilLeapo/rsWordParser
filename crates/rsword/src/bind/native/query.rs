//! 批量有效属性查询（`BIND-06`）。模型只读，按 part 建索引，表格中的 run 带条件样式上下文。
use super::session::{bad, query_dom};
use super::{ApiError, ProjCx, ToJson};
use crate::edit::EditSession;
use crate::model::{
    Block, Document, HfKind, HfVariant, Inline, Run, TableBlock, TextBlock, TextKind,
};
use crate::package::PartId;
use crate::resolve::section::EffectiveSection;
use crate::resolve::{
    EffectiveCellProps, EffectiveParaProps, EffectiveRunProps, Provenance, Resolver, TableView,
};
use crate::semantic::props::{CellPropsField, ParaPropsField, RunPropsField};
use crate::xml::{Dom, NodeId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum QueryId {
    Node(u32),
    Section {
        #[serde(rename = "sectionIndex")]
        section_index: usize,
    },
}

type CellContext<'a> = (&'a TableBlock, usize, usize);
#[derive(Default)]
pub(super) struct Index<'a> {
    pub paras: BTreeMap<(PartId, NodeId), &'a TextBlock>,
    pub runs: BTreeMap<(PartId, NodeId), (&'a TextBlock, &'a Run, Option<CellContext<'a>>)>,
    pub cells: BTreeMap<(PartId, NodeId), CellContext<'a>>,
    pub tables: BTreeMap<(PartId, NodeId), &'a TableBlock>,
}
impl<'a> Index<'a> {
    pub fn new(doc: &'a Document) -> Self {
        let mut out = Self::default();
        let mut parts = vec![doc.main_part];
        parts.extend(doc.hf_parts.keys().copied());
        parts.extend(
            [doc.footnotes.part, doc.endnotes.part, doc.comments.part].into_iter().flatten(),
        );
        let mut pending = Vec::new();
        for part in parts {
            if let Some(blocks) = doc.blocks_of_part(part) {
                pending.extend(blocks.into_iter().map(|b| (part, b, None)));
            }
        }
        while let Some((part, block, context)) = pending.pop() {
            for (blocks, external) in crate::model::box_flows(block) {
                pending.extend(blocks.iter().map(|b| (external.unwrap_or(part), b, None)));
            }
            match block {
                Block::Text(p) => {
                    out.paras.insert((part, p.node), p);
                    for inline in &p.inlines {
                        if let Inline::Run(r) = inline {
                            out.runs.insert((part, r.node), (p, r, context));
                        }
                    }
                }
                Block::Table(t) => {
                    out.tables.insert((part, t.node), t);
                    for (row, r) in t.rows.iter().enumerate() {
                        for (col, c) in r.cells.iter().enumerate() {
                            let context = Some((t, row, col));
                            out.cells.insert((part, c.node), (t, row, col));
                            pending.extend(c.blocks.iter().map(|b| (part, b, context)));
                        }
                    }
                }
                Block::Image(_) | Block::Protected(_) => {}
            }
        }
        out
    }
}

/// Provenance 可以包含 Toggle；显式栈避免嵌套来源占用调用栈。
pub(super) fn provenance(source: &Provenance) -> Value {
    let mut root = Value::Null;
    let mut pending = vec![(source, &mut root)];
    while let Some((source, out)) = pending.pop() {
        *out = match source {
            Provenance::Direct => json!({"kind":"direct"}),
            Provenance::CharStyle(id) => json!({"kind":"charStyle", "value":id}),
            Provenance::ParaStyle(id) => json!({"kind":"paraStyle", "value":id}),
            Provenance::NumberingLevel { num_id, ilvl } => {
                json!({"kind":"numberingLevel", "numId":num_id, "ilvl":ilvl})
            }
            Provenance::TableStyle { style, cond } => {
                json!({"kind":"tableStyle", "style":style, "cond":cond.map(|c| c.as_str())})
            }
            Provenance::DocDefaults => json!({"kind":"docDefaults"}),
            Provenance::Theme => json!({"kind":"theme"}),
            Provenance::Default => json!({"kind":"default"}),
            Provenance::Toggle { levels } => {
                json!({"kind":"toggle", "levels": vec![Value::Null; levels.len()]})
            }
        };
        if let Provenance::Toggle { levels } = source {
            pending.extend(levels.iter().zip(out["levels"].as_array_mut().expect("toggle levels")));
        }
    }
    root
}
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}
macro_rules! sources {
    ($effective:expr, $fields:ty) => {{
        let mut out = serde_json::Map::new();
        for field in <$fields>::ALL {
            out.insert(camel(field.info().name), provenance(&$effective.source(*field)));
        }
        Value::Object(out)
    }};
}
pub(super) fn run_json(e: &EffectiveRunProps, cx: &ProjCx<'_>) -> Value {
    let EffectiveRunProps { props, sources: _, cs } = e;
    json!({"value": {"props":props.to_json(cx), "cs":cs.value}, "provenance":{"props":sources!(e, RunPropsField), "cs":provenance(&cs.source)}})
}
pub(super) fn para_json(e: &EffectiveParaProps, cx: &ProjCx<'_>) -> Value {
    let EffectiveParaProps { props, sources: _ } = e;
    json!({"value":{"props":props.to_json(cx)}, "provenance":{"props":sources!(e, ParaPropsField)}})
}
pub(super) fn cell_json(e: &EffectiveCellProps, cx: &ProjCx<'_>) -> Value {
    let EffectiveCellProps { props, sources: _, rpr, ppr, conditions } = e;
    json!({"value":{"props":props.to_json(cx), "rpr":rpr.to_json(cx), "ppr":ppr.to_json(cx), "conditions":conditions.to_json(cx)}, "provenance":{"props":sources!(e, CellPropsField)}})
}
pub(super) fn section_json(e: &EffectiveSection, cx: &ProjCx<'_>) -> Value {
    let EffectiveSection { idx, hf: _, title_pg, even_and_odd, geom } = e;
    use crate::resolve::section::HfSlot;
    let hf: Vec<Vec<Value>> = HfKind::ALL
        .iter()
        .map(|&kind| {
            HfVariant::ALL
                .iter()
                .map(|&variant| match e.slot(kind, variant) {
                    HfSlot::Absent => json!({"kind":"absent"}),
                    HfSlot::Declared(id) => json!({"kind":"declared", "value":id}),
                    HfSlot::Inherited { from, id } => {
                        json!({"kind":"inherited", "from":from, "id":id})
                    }
                })
                .collect()
        })
        .collect();
    json!({"value":{"idx":idx,"hf":hf,"titlePg":title_pg,"evenAndOdd":even_and_odd,"geom":geom.to_json(cx)},"provenance":{}})
}
fn layer_json(l: &crate::resolve::table::TableStyleLayer, cx: &ProjCx<'_>) -> Value {
    let crate::resolve::table::TableStyleLayer { tbl_pr, tr_pr, tc_pr, rpr, ppr } = l;
    json!({"tblPr":tbl_pr.to_json(cx), "trPr":tr_pr.to_json(cx), "tcPr":tc_pr.to_json(cx), "rpr":rpr.to_json(cx), "ppr":ppr.to_json(cx)})
}
pub(super) fn table_json(e: &TableView<'_>, cx: &ProjCx<'_>) -> Value {
    use crate::resolve::table::{ColumnSource, ColumnView, TableStyleView, TblLookFlags, ViewCell};
    let TblLookFlags {
        first_row,
        last_row,
        first_column,
        last_column,
        banded_rows,
        banded_columns,
    } = e.look();
    let ColumnView { widths_twips, widths_pct, source, rows } = e.columns();
    let source = match source {
        ColumnSource::Grid => "grid",
        ColumnSource::TcW => "tcW",
        ColumnSource::Stretched => "stretched",
        ColumnSource::Reconciled => "reconciled",
        ColumnSource::None => "none",
    };
    let TableStyleView { style_id, whole, conditional } = e.style();
    let conditional: serde_json::Map<String, Value> =
        conditional.iter().map(|(k, v)| (k.as_str().into(), layer_json(v, cx))).collect();
    let rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|ViewCell { cell, span, gap }| json!({"cell":cell,"span":span,"gap":gap}))
                .collect()
        })
        .collect();
    let cells: Vec<Vec<Value>> = e
        .table()
        .rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            row.cells.iter().enumerate().map(|(c,_)| {
        json!({"effective":cell_json(&e.cell(r,c),cx), "borders":e.cell_borders(r,c).to_json(cx)})
    }).collect()
        })
        .collect();
    let heights: Vec<Value> =
        (0..e.table().rows.len()).map(|r| e.row_height(r).to_json(cx)).collect();
    let borders = e
        .borders()
        .map(|b| json!({"value":b.value.to_json(cx), "provenance":provenance(&b.source)}));
    let margins = e
        .cell_margins()
        .map(|b| json!({"value":b.value.to_json(cx), "provenance":provenance(&b.source)}));
    json!({"value":{"look":{"firstRow":first_row,"lastRow":last_row,"firstColumn":first_column,"lastColumn":last_column,"bandedRows":banded_rows,"bandedColumns":banded_columns},
        "columns":{"widthsTwips":widths_twips,"widthsPct":widths_pct,"source":source,"rows":rows},
        "style":{"styleId":style_id,"whole":layer_json(whole,cx),"conditional":conditional},
        "cells":cells,"rowHeights":heights,"borders":borders,"cellMargins":margins}, "provenance":{}})
}

pub(super) struct Query<'a> {
    session: &'a EditSession,
    dom: &'a Dom,
    part: PartId,
    index: Index<'a>,
    resolver: Resolver<'a>,
}
impl<'a> Query<'a> {
    fn new(session: &'a EditSession, part: Option<u32>) -> Result<Self, ApiError> {
        Ok(Self {
            dom: query_dom(session, part)?,
            part: PartId(part.unwrap_or(session.main_part().0)),
            index: Index::new(session.document()),
            resolver: Resolver::new(session.document()),
            session,
        })
    }
    fn run(&self, id: QueryId) -> Option<EffectiveRunProps> {
        let QueryId::Node(id) = id else { return None };
        let (p, r, cell) = self.index.runs.get(&(self.part, NodeId(id)))?;
        let cell = cell.map(|(t, row, col)| self.resolver.table(self.dom, t).cell(row, col));
        Some(self.resolver.run_in_table(
            cell.as_ref().map(|c| &c.rpr),
            p.style_id.as_deref(),
            r.props.style.as_deref(),
            &r.props,
        ))
    }
    fn para(&self, id: QueryId) -> Option<EffectiveParaProps> {
        let QueryId::Node(id) = id else { return None };
        let p = self.index.paras.get(&(self.part, NodeId(id)))?;
        let list = match &p.kind {
            TextKind::ListItem { list } => Some(list),
            TextKind::Paragraph | TextKind::Heading { .. } => None,
        };
        Some(self.resolver.para(p.style_id.as_deref(), list, &p.props))
    }
    fn cell(&self, id: QueryId) -> Option<EffectiveCellProps> {
        let QueryId::Node(id) = id else { return None };
        let (t, row, col) = self.index.cells.get(&(self.part, NodeId(id)))?;
        Some(self.resolver.table(self.dom, t).cell(*row, *col))
    }
    fn table(&self, id: QueryId) -> Option<TableView<'_>> {
        let QueryId::Node(id) = id else { return None };
        Some(self.resolver.table(self.dom, self.index.tables.get(&(self.part, NodeId(id)))?))
    }
    fn section(&self, id: QueryId) -> Option<EffectiveSection> {
        if self.part != self.session.main_part() {
            return None;
        }
        let sections = &self.session.document().sections;
        let index = match id {
            QueryId::Node(id) => sections.iter().position(|s| s.node == Some(NodeId(id)))?,
            QueryId::Section { section_index } => section_index,
        };
        self.resolver.section(sections, index)
    }
}

/// 五个导出共用参数校验、逐项错误隔离与投影；声明表也驱动全语料直接调用对照。
macro_rules! resolve_query {
    ($($name:ident => $method:ident, $project:ident, $test:ident;)*) => {$(
        pub(super) fn $name(session: &EditSession, ids: &str, part: Option<u32>) -> Result<String, ApiError> {
            let query = Query::new(session, part)?;
            let ids: Vec<QueryId> = serde_json::from_str(ids).map_err(|e| bad(e.to_string()))?;
            let cx = ProjCx {pkg:session.package(),display:false};
            let result: Vec<Value> = ids.into_iter().map(|id| query.$method(id).map(|e| $project(&e,&cx)).unwrap_or_else(|| json!({"error":"BIND_ID_UNKNOWN"}))).collect();
            Ok(Value::Array(result).to_string())
        }
        #[cfg(test)] #[test] fn $test() {
            tests::corpus(|table,id,s,part,blocks| {
                let (mut ids,mut expected)=tests::requests(s,part,blocks,stringify!($method));
                let count=ids.len();
                ids.insert(0,json!(u32::MAX)); expected.insert(0,json!({"error":"BIND_ID_UNKNOWN"}));
                ids.push(json!(u32::MAX)); expected.push(json!({"error":"BIND_ID_UNKNOWN"}));
                let output=table.$name(id,&Value::Array(ids).to_string(),Some(part.0)).unwrap();
                let actual:Value=serde_json::from_str(&output).unwrap();
                assert_eq!(actual,Value::Array(expected),"part {}",part.0); count
            });
        }
    )*};
}
resolve_query! {
    resolve_runs => run, run_json, bind_06_run_full_corpus;
    resolve_paras => para, para_json, bind_06_para_full_corpus;
    resolve_cells => cell, cell_json, bind_06_cell_full_corpus;
    resolve_sections => section, section_json, bind_06_section_full_corpus;
    resolve_table => table, table_json, bind_06_table_full_corpus;
}

use crate::model::SectionGeom;
super::json::model_json! {
    /// `RES-10` 的页面几何，字段穷尽。
    struct SectionGeom(cx) test bind_06_section_geometry_fields {
        opt node => "node", NodeId = node;
        page_width => "pageWidth", i64 = page_width;
        page_height => "pageHeight", i64 = page_height;
        margin_top => "marginTop", i64 = margin_top;
        margin_right => "marginRight", i64 = margin_right;
        margin_bottom => "marginBottom", i64 = margin_bottom;
        margin_left => "marginLeft", i64 = margin_left;
        columns => "columns", i64 = columns;
    }
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
