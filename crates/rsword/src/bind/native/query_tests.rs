//! BIND-06 全语料对照：从模型文档序独立收集请求，直接调用 Resolver 得到 oracle。
use super::super::SessionTable;
use super::*;
use crate::model::Blocks;
use std::collections::BTreeSet;
#[path = "../../../tests/common/mod.rs"]
mod common;

type LocatedBlocks<'a> = Vec<(&'a Block, Option<CellContext<'a>>)>;
type Compare = fn(&mut SessionTable, &str, &EditSession, PartId, LocatedBlocks<'_>) -> usize;
pub(super) fn corpus(compare: Compare) {
    const REFUSED: [&str; 4] = [
        "xml-unbalanced-main.docx",
        "zip-part-too-large.docx",
        "zip-too-many-parts.docx",
        "zip-total-too-large.docx",
    ];
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103);
    let mut refused = BTreeSet::new();
    let mut opened = 0;
    let mut count = 0;
    let mut table = SessionTable::default();
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let result = table.open(&bytes, None);
        if REFUSED.contains(&name) {
            assert!(result.is_err());
            refused.insert(name.to_owned());
            continue;
        }
        let id = result.unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        let s = EditSession::open(&bytes).unwrap();
        let doc = s.document();
        opened += 1;
        let mut roots = vec![(doc.main_part, doc.main.as_slice())];
        roots.extend(doc.hf_parts.iter().map(|(&part, hf)| (part, hf.blocks.as_slice())));
        for notes in [&doc.footnotes, &doc.endnotes] {
            if let Some(part) = notes.part {
                roots.extend(notes.items.iter().map(|n| (part, n.blocks.as_slice())));
            }
        }
        if let Some(part) = doc.comments.part {
            roots.extend(doc.comments.items.iter().map(|c| (part, c.blocks.as_slice())));
        }
        let mut all: BTreeMap<PartId, Vec<(&Block, Option<CellContext<'_>>)>> = BTreeMap::new();
        while let Some((part, blocks)) = roots.pop() {
            for block in Blocks::over(blocks) {
                all.entry(part).or_default().push((block, None));
                roots.extend(
                    crate::model::box_flows(block).into_iter().map(|(b, p)| (p.unwrap_or(part), b)),
                );
            }
        }
        // 用 DOM 祖先确定最近的单元格；不复用协议索引传播的上下文。
        for (&part, blocks) in &mut all {
            let dom = s.package().part(part).dom().unwrap();
            let cells: BTreeMap<NodeId, CellContext<'_>> = blocks
                .iter()
                .filter_map(|(b, _)| if let Block::Table(t) = b { Some(t) } else { None })
                .flat_map(|t| {
                    t.rows.iter().enumerate().flat_map(move |(r, row)| {
                        row.cells.iter().enumerate().map(move |(c, cell)| (cell.node, (t, r, c)))
                    })
                })
                .collect();
            for (block, context) in blocks.iter_mut() {
                let mut parent = dom.parent(block.node());
                while let Some(node) = parent {
                    if dom.is(node, crate::xml::QName::w(crate::xml::LocalName::TxbxContent)) {
                        break;
                    }
                    if let Some(cell) = cells.get(&node) {
                        *context = Some(*cell);
                        break;
                    }
                    parent = dom.parent(node);
                }
            }
        }
        for (part, blocks) in all {
            count += compare(&mut table, &id, &s, part, blocks);
        }
        table.close(&id);
    }
    assert_eq!(opened, 1099);
    assert_eq!(refused, REFUSED.into_iter().map(String::from).collect());
    assert!(count > 0);
    eprintln!("BIND-06: {opened} documents, {count} resolved items");
}

pub(super) fn requests(
    s: &EditSession,
    part: PartId,
    blocks: Vec<(&Block, Option<CellContext<'_>>)>,
    kind: &str,
) -> (Vec<Value>, Vec<Value>) {
    let resolver = Resolver::new(s.document());
    let dom = s.package().part(part).dom().unwrap();
    let cx = ProjCx { pkg: s.package(), display: false };
    let mut ids = vec![];
    let mut expected = vec![];
    for (block, context) in blocks {
        match (kind, block) {
            ("run", Block::Text(p)) => {
                for inline in &p.inlines {
                    if let Inline::Run(run) = inline {
                        let cell = context.map(|(t, r, c)| resolver.table(dom, t).cell(r, c));
                        let effective = resolver.run_in_table(
                            cell.as_ref().map(|c| &c.rpr),
                            p.style_id.as_deref(),
                            run.props.style.as_deref(),
                            &run.props,
                        );
                        ids.push(json!(run.node.0));
                        expected.push(checked_run(&effective, &cx));
                    }
                }
            }
            ("para", Block::Text(p)) => {
                let list =
                    if let TextKind::ListItem { list } = &p.kind { Some(list) } else { None };
                ids.push(json!(p.node.0));
                expected
                    .push(checked_para(&resolver.para(p.style_id.as_deref(), list, &p.props), &cx));
            }
            ("cell", Block::Table(t)) => {
                for (r, row) in t.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        ids.push(json!(cell.node.0));
                        expected.push(checked_cell(&resolver.table(dom, t).cell(r, c), &cx));
                    }
                }
            }
            ("table", Block::Table(t)) => {
                ids.push(json!(t.node.0));
                expected.push(checked_table(&resolver.table(dom, t), &cx));
            }
            _ => {}
        }
    }
    if kind == "section" && part == s.main_part() {
        for (i, section) in s.document().sections.iter().enumerate() {
            ids.push(section.node.map(|n| json!(n.0)).unwrap_or_else(|| json!({"sectionIndex":i})));
            expected
                .push(checked_section(&resolver.section(&s.document().sections, i).unwrap(), &cx));
        }
    }
    (ids, expected)
}

// 对照不只共用输出函数：属性从线型独立解码回引擎型；漏键 / 换值必须在这里失败。
macro_rules! checked_properties {
    ($($name:ident, $project:ident, $effective:ty, $props:ty, $fields:ty;)*) => {$(
        fn $name(e: &$effective, cx:&ProjCx<'_>) -> Value {
            let value=$project(e,cx);
            let decoded:$props=serde_json::from_value(value["value"]["props"].clone()).unwrap();
            assert_eq!(decoded,e.props);
            let sources=value["provenance"]["props"].as_object().unwrap();
            assert_eq!(sources.len(),<$fields>::ALL.len());
            for field in <$fields>::ALL { check_source(&sources[&camel(field.info().name)],&e.source(*field)); }
            value
        }
    )*};
}
checked_properties! {
    checked_run_props, run_json, EffectiveRunProps, crate::semantic::props::RunProps, RunPropsField;
    checked_para, para_json, EffectiveParaProps, crate::semantic::props::ParaProps, ParaPropsField;
    checked_cell_props, cell_json, EffectiveCellProps, crate::semantic::props::CellProps, CellPropsField;
}
fn check_source(wire: &Value, source: &Provenance) {
    let mut pending = vec![(wire, source)];
    while let Some((wire, source)) = pending.pop() {
        let kind = match source {
            Provenance::Direct => "direct",
            Provenance::Default => "default",
            Provenance::DocDefaults => "docDefaults",
            Provenance::Theme => "theme",
            Provenance::CharStyle(id) => {
                assert_eq!(wire["value"], *id);
                "charStyle"
            }
            Provenance::ParaStyle(id) => {
                assert_eq!(wire["value"], *id);
                "paraStyle"
            }
            Provenance::NumberingLevel { num_id, ilvl } => {
                assert_eq!(wire["numId"], *num_id);
                assert_eq!(wire["ilvl"], *ilvl);
                "numberingLevel"
            }
            Provenance::TableStyle { style, cond } => {
                assert_eq!(wire["style"], *style);
                assert_eq!(wire["cond"], json!(cond.map(|c| c.as_str())));
                "tableStyle"
            }
            Provenance::Toggle { levels } => {
                let values = wire["levels"].as_array().unwrap();
                assert_eq!(values.len(), levels.len());
                pending.extend(values.iter().zip(levels));
                "toggle"
            }
        };
        assert_eq!(wire["kind"], kind);
    }
}

fn checked_section(e: &EffectiveSection, cx: &ProjCx<'_>) -> Value {
    let wire = section_json(e, cx);
    let value = wire["value"].as_object().unwrap();
    assert_eq!(value.len(), 5);
    assert_eq!(value["idx"], e.idx);
    assert_eq!(value["titlePg"], e.title_pg);
    assert_eq!(value["evenAndOdd"], e.even_and_odd);
    assert_eq!(value["geom"]["pageWidth"], e.geom.page_width);
    assert_eq!(value["geom"]["pageHeight"], e.geom.page_height);
    assert_eq!(value["geom"]["marginTop"], e.geom.margin_top);
    assert_eq!(value["geom"]["marginRight"], e.geom.margin_right);
    assert_eq!(value["geom"]["marginBottom"], e.geom.margin_bottom);
    assert_eq!(value["geom"]["marginLeft"], e.geom.margin_left);
    assert_eq!(value["geom"]["columns"], e.geom.columns);
    assert_eq!(
        value["geom"].get("node").and_then(Value::as_u64),
        e.geom.node.map(|n| u64::from(n.0))
    );
    for (k, kind) in HfKind::ALL.into_iter().enumerate() {
        for (v, variant) in HfVariant::ALL.into_iter().enumerate() {
            let slot = e.slot(kind, variant);
            let actual = &value["hf"][k][v];
            use crate::resolve::section::HfSlot;
            match slot {
                HfSlot::Absent => assert_eq!(*actual, json!({"kind":"absent"})),
                HfSlot::Declared(id) => assert_eq!(*actual, json!({"kind":"declared","value":id})),
                HfSlot::Inherited { from, id } => {
                    assert_eq!(*actual, json!({"kind":"inherited","from":from,"id":id}))
                }
            }
        }
    }
    wire
}
fn checked_table(e: &TableView<'_>, cx: &ProjCx<'_>) -> Value {
    let wire = table_json(e, cx);
    let value = &wire["value"];
    assert_eq!(
        value.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["borders", "cellMargins", "cells", "columns", "look", "rowHeights", "style"]
    );
    use crate::resolve::table::ColumnSource;
    assert_eq!(
        value["columns"]["source"],
        match e.columns().source {
            ColumnSource::Grid => "grid",
            ColumnSource::TcW => "tcW",
            ColumnSource::Stretched => "stretched",
            ColumnSource::Reconciled => "reconciled",
            ColumnSource::None => "none",
        }
    );
    assert_eq!(value["columns"]["widthsTwips"], json!(e.columns().widths_twips));

    assert_eq!(value["columns"]["widthsPct"], json!(e.columns().widths_pct));
    assert_eq!(value["columns"]["rows"].as_array().unwrap().len(), e.columns().rows.len());
    for (r, row) in e.columns().rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            assert_eq!(
                value["columns"]["rows"][r][c],
                json!({"cell":cell.cell,"span":cell.span,"gap":cell.gap})
            );
        }
    }
    assert_eq!(
        value["look"],
        json!({"firstRow":e.look().first_row,"lastRow":e.look().last_row,"firstColumn":e.look().first_column,"lastColumn":e.look().last_column,"bandedRows":e.look().banded_rows,"bandedColumns":e.look().banded_columns})
    );
    assert_eq!(value["style"]["styleId"], json!(e.style().style_id));
    let mut layers = vec![(&value["style"]["whole"], &e.style().whole)];
    assert_eq!(
        value["style"]["conditional"].as_object().unwrap().len(),
        e.style().conditional.len()
    );
    layers.extend(
        e.style()
            .conditional
            .iter()
            .map(|(kind, layer)| (&value["style"]["conditional"][kind.as_str()], layer)),
    );
    for (wire, layer) in layers {
        use crate::semantic::props::*;
        macro_rules! layer_fields {($($field:ident:$ty:ty=>$key:literal;)*)=>{$(assert_eq!(serde_json::from_value::<$ty>(wire[$key].clone()).unwrap(),layer.$field);)*};}
        layer_fields! {tbl_pr:TableProps=>"tblPr";tr_pr:RowProps=>"trPr";tc_pr:CellProps=>"tcPr";rpr:RunProps=>"rpr";ppr:ParaProps=>"ppr";}
    }
    for (r, row) in e.table().rows.iter().enumerate() {
        assert_eq!(value["rowHeights"][r], e.row_height(r).to_json(cx));
        for (c, _) in row.cells.iter().enumerate() {
            assert_eq!(value["cells"][r][c]["effective"], checked_cell(&e.cell(r, c), cx));
            assert_eq!(value["cells"][r][c]["borders"], e.cell_borders(r, c).to_json(cx));
        }
    }
    macro_rules! optional_effective {($($method:ident=>$key:literal;)*)=>{$(
        if let Some(effective)=e.$method() {assert_eq!(value[$key]["value"],effective.value.to_json(cx));check_source(&value[$key]["provenance"],&effective.source);} else {assert!(value[$key].is_null());}
    )*};}
    optional_effective! {borders=>"borders";cell_margins=>"cellMargins";}
    wire
}

fn checked_run(e: &EffectiveRunProps, cx: &ProjCx<'_>) -> Value {
    let wire = checked_run_props(e, cx);
    assert_eq!(wire["value"].as_object().unwrap().len(), 2);
    assert_eq!(wire["value"]["cs"], e.cs.value);
    check_source(&wire["provenance"]["cs"], &e.cs.source);
    wire
}
fn checked_cell(e: &EffectiveCellProps, cx: &ProjCx<'_>) -> Value {
    let wire = checked_cell_props(e, cx);
    assert_eq!(wire["value"].as_object().unwrap().len(), 4);
    assert_eq!(
        serde_json::from_value::<crate::semantic::props::RunProps>(wire["value"]["rpr"].clone())
            .unwrap(),
        e.rpr
    );
    assert_eq!(
        serde_json::from_value::<crate::semantic::props::ParaProps>(wire["value"]["ppr"].clone())
            .unwrap(),
        e.ppr
    );
    assert_eq!(
        serde_json::from_value::<Vec<crate::semantic::props::TblStyleOverrideType>>(
            wire["value"]["conditions"].clone()
        )
        .unwrap(),
        e.conditions
    );
    wire
}
