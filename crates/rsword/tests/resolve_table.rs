//! 表格有效属性视图的验收（`spec/07` RES-08，任务 3.4）：`tblLook` 六个开关、表格样式链的条件格式、
//! 列宽视图的四条启发式。列宽与跨度用全语料对着 TS 的 `TableModel` 校，条件格式用 `table-style__*`
//! 的五个场景。

mod common;

use rsword::model::{Block, Document};
use rsword::package::Package;
use rsword::resolve::{ColumnSource, Resolver, TblLookFlags};
use rsword::semantic::props::TblStyleOverrideType;
use rsword::semantic::props::{TblLook, Val};
use serde_json::Value;

fn look(val: Option<&str>) -> TblLook {
    TblLook { val: val.map(str::to_owned), ..Default::default() }
}

#[test]
fn res_08_tbl_look_attributes_beat_bits_beat_defaults() {
    // 验收行：`tblLook w:val="04A0"` = 0x20 firstRow + 0x80 firstColumn + 0x400 noVBand
    let f = TblLookFlags::read(Some(&look(Some("04A0"))));
    assert!(f.first_row && f.first_column);
    assert!(!f.last_row && !f.last_column);
    assert!(f.banded_rows, "noHBand 没置位 → 横向条带开");
    assert!(!f.banded_columns, "noVBand 置位 → 纵向条带关");
    // 缺 tblLook 时的缺省与 04A0 相同（Word 的缺省值）
    assert_eq!(TblLookFlags::read(None), f);
    assert_eq!(TblLookFlags::read(Some(&TblLook::default())), f);
    // 全 0：只剩条带
    let f0 = TblLookFlags::read(Some(&look(Some("0000"))));
    assert!(!f0.first_row && !f0.first_column && !f0.last_row && !f0.last_column);
    assert!(f0.banded_rows && f0.banded_columns);
    // 属性形式优先于位掩码
    let mixed = TblLook {
        val: Some("04A0".into()),
        first_row: Some(false),
        last_row: Some(true),
        no_h_band: Some(true),
        no_v_band: Some(false),
        ..Default::default()
    };
    let f = TblLookFlags::read(Some(&mixed));
    assert!(!f.first_row && f.last_row);
    assert!(!f.banded_rows && f.banded_columns);
    assert!(f.first_column, "没给属性的位仍看 w:val");
    // 坏的 w:val 退回缺省
    assert_eq!(TblLookFlags::read(Some(&look(Some("zzzz")))), TblLookFlags::default());
}

/// 打开一份语料文档，取第 `i` 张顶层表的视图并做断言。
fn with_table<T>(
    name: &str,
    i: usize,
    f: impl FnOnce(&Document, &rsword::resolve::TableView<'_>) -> T,
) -> T {
    let bytes =
        std::fs::read(common::corpus_dir("synthetic").join(format!("{name}.docx"))).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let tables: Vec<_> = doc
        .main
        .iter()
        .filter_map(|b| match b {
            rsword::model::Block::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    let r = Resolver::new(&doc);
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let view = r.table(dom, tables[i]);
    f(&doc, &view)
}

#[test]
fn res_08_conditional_formats_follow_tbl_look() {
    use TblStyleOverrideType as T;
    // table-style__001：firstRow + 条带，tblLook 打开首行
    with_table("table-style__001", 0, |_, v| {
        assert!(v.look().first_row);
        assert_eq!(v.conditions(0, 0).first(), Some(&T::FirstRow));
        // 条带行号从 firstRow 之后起算：第 1 行是 band1，第 2 行 band2
        assert!(v.conditions(1, 0).contains(&T::Band1Horz), "{:?}", v.conditions(1, 0));
        assert!(v.conditions(2, 0).contains(&T::Band2Horz), "{:?}", v.conditions(2, 0));
        // 样式里声明了 firstRow 与 band1Horz 的层
        assert!(v.style().layer(T::FirstRow).is_some());
        // 首行格的有效底纹来自条件层
        let c = v.cell(0, 0);
        assert!(c.props.shading.is_some(), "首行应从样式拿到底纹");
        assert!(c.conditions.contains(&T::FirstRow));
    });
    // table-style__002：noHBand + firstRow=0
    with_table("table-style__002", 0, |_, v| {
        let f = v.look();
        assert!(!f.first_row || !f.banded_rows, "至少关掉了一个：{f:?}");
        if !f.banded_rows {
            assert!(!v.conditions(1, 0).iter().any(|c| matches!(c, T::Band1Horz | T::Band2Horz)));
        }
    });
    // table-style__003：显式底纹胜过样式
    with_table("table-style__003", 0, |_, v| {
        let mut explicit = None;
        for (r, row) in v.table().rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.props.shading.is_some() {
                    explicit = Some((r, c));
                }
            }
        }
        let (r, c) = explicit.expect("语料里应有显式底纹的格");
        let eff = v.cell(r, c);
        assert_eq!(
            eff.props.shading,
            v.table().rows[r].cells[c].props.shading,
            "自身声明优先于条件格式"
        );
        assert!(matches!(
            eff.source(rsword::semantic::props::CellPropsField::Shading),
            rsword::resolve::Provenance::Direct
        ));
    });
    // table-style__004 / 005 没有顶层表格，直接校样式链：子样式继承父样式的条件层与整表层
    for name in ["table-style__004", "table-style__005"] {
        let bytes =
            std::fs::read(common::corpus_dir("synthetic").join(format!("{name}.docx"))).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
        let styles = doc.styles.as_ref().expect("styles.xml");
        let r = Resolver::new(&doc);
        let table_styles: Vec<&str> = styles
            .styles
            .iter()
            .filter(|s| s.kind() == Some(rsword::semantic::props::StyleType::Table))
            .filter_map(|s| s.id())
            .collect();
        assert!(table_styles.len() >= 2, "{name}: 应有基样式与子样式");
        let based: Vec<&str> = styles
            .styles
            .iter()
            .filter(|s| {
                s.kind() == Some(rsword::semantic::props::StyleType::Table) && s.based_on.is_some()
            })
            .filter_map(|s| s.id())
            .collect();
        let child = based.first().unwrap_or_else(|| panic!("{name}: 应有 basedOn 的表格样式"));
        let view = r.table_style(Some(child));
        let chain = r.chain(child, rsword::semantic::props::StyleType::Table);
        assert!(chain.len() >= 2, "{name}: basedOn 链应有两层");
        // 子样式自己没声明、父样式声明了的层也要在
        let own_conds: usize = chain[0].conditional.len();
        assert!(
            view.conditional.len() >= own_conds,
            "{name}: 条件层没有从 basedOn 链继承：{:?}",
            view.conditional.keys().collect::<Vec<_>>()
        );
    }
}

/// 表格边框 / 边距在文档未声明时回退到样式链（`RES-08`）。
#[test]
fn res_08_borders_and_margins_fall_back_to_the_style() {
    with_table("table-display__006", 0, |_, v| {
        let doc_borders = v.table().props.borders.is_some();
        let b = v.borders();
        if doc_borders {
            assert!(matches!(b.unwrap().source, rsword::resolve::Provenance::Direct));
        } else if let Some(b) = b {
            assert!(
                matches!(b.source, rsword::resolve::Provenance::TableStyle { .. }),
                "{:?}",
                b.source
            );
        }
    });
}

#[test]
fn res_08_row_height_is_clamped() {
    // 语料里有 EMU 污染的 trHeight（table-display__012 一类）；模型保原值，视图截断
    let mut checked = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
        let r = Resolver::new(&doc);
        let dom = pkg.part(doc.main_part).dom().unwrap();
        for t in doc.tables() {
            let v = r.table(dom, t);
            for (i, row) in t.rows.iter().enumerate() {
                let declared =
                    row.props.height.as_ref().and_then(|h| h.val.as_ref()).and_then(Val::value);
                if let Some(&d) = declared.filter(|&&d| d > 0) {
                    let (h, _) = v.row_height(i).expect("声明了行高");
                    assert!(h <= rsword::resolve::table::MAX_ROW_HEIGHT_TWIPS);
                    assert_eq!(h, d.min(rsword::resolve::table::MAX_ROW_HEIGHT_TWIPS));
                    checked += 1;
                } else {
                    assert!(v.row_height(i).is_none());
                }
            }
        }
    }
    assert!(checked > 0, "语料里应有声明行高的行");
}

#[derive(Default)]
struct Stats {
    docs: usize,
    tables: usize,
    by_source: std::collections::BTreeMap<&'static str, usize>,
    mismatches: Vec<String>,
}

fn source_name(s: ColumnSource) -> &'static str {
    match s {
        ColumnSource::Grid => "grid",
        ColumnSource::TcW => "tcW",
        ColumnSource::Stretched => "stretched",
        ColumnSource::Reconciled => "reconciled",
        ColumnSource::None => "none",
    }
}

/// 全语料：列宽（绝对与百分比）与每行的格跨度 / `gridGap` 占位与 TS 的 `TableModel` 一致。
/// 这是四条启发式唯一靠得住的验收——语料里 `table-grid-reconcile__*` 六份专门造了不一致的网格。
#[test]
fn res_08_column_widths_match_ts_on_the_corpus() {
    let mut st = Stats::default();
    for path in common::docx_paths("synthetic") {
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let e: Value = serde_json::from_str(&text).unwrap();
        let ts_tables: Vec<&Value> = e
            .get("blocks")
            .and_then(Value::as_array)
            .map(|bs| {
                bs.iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("table"))
                    .filter_map(|b| b.get("table"))
                    .collect()
            })
            .unwrap_or_default();
        if ts_tables.is_empty() {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
        let r = Resolver::new(&doc);
        let dom = pkg.part(doc.main_part).dom().unwrap();
        let ours: Vec<_> = doc
            .main
            .iter()
            .filter_map(|b| match b {
                rsword::model::Block::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        if ours.len() != ts_tables.len() {
            continue;
        }
        st.docs += 1;
        for (t, ts) in ours.iter().zip(ts_tables) {
            st.tables += 1;
            let v = r.table(dom, t);
            let cols = v.columns();
            *st.by_source.entry(source_name(cols.source)).or_default() += 1;
            let num = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
            if let Some(ts_twips) = ts.get("colWidthsTwips").and_then(Value::as_array) {
                let ours: Vec<f64> = cols.widths_twips.iter().map(|&w| f64::from(w)).collect();
                let want: Vec<f64> = ts_twips.iter().map(num).collect();
                if ours != want {
                    st.mismatches.push(format!("{name}: colWidthsTwips {ours:?} vs TS {want:?}"));
                }
            }
            if let Some(ts_pct) = ts.get("colWidthsPct").and_then(Value::as_array) {
                let want: Vec<f64> = ts_pct.iter().map(num).collect();
                if cols.widths_pct.len() != want.len()
                    || cols.widths_pct.iter().zip(&want).any(|(a, b)| (a - b).abs() > 1e-6)
                {
                    st.mismatches
                        .push(format!("{name}: colWidthsPct {:?} vs TS {want:?}", cols.widths_pct));
                }
            }
            // 每行的格形状：跨度与 gridGap 占位
            let ts_rows = ts.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
            let rows: Vec<&Vec<rsword::resolve::ViewCell>> = cols
                .rows
                .iter()
                .zip(&t.rows)
                .filter(|(_, r)| !r.cells.is_empty())
                .map(|(v, _)| v)
                .collect();
            if rows.len() != ts_rows.len() {
                st.mismatches.push(format!("{name}: 行数 {} vs TS {}", rows.len(), ts_rows.len()));
                continue;
            }
            for (ri, (row, ts_row)) in rows.iter().zip(&ts_rows).enumerate() {
                let ts_cells = ts_row.as_array().cloned().unwrap_or_default();
                if row.len() != ts_cells.len() {
                    st.mismatches.push(format!(
                        "{name}: 第 {ri} 行格数 {} vs TS {}",
                        row.len(),
                        ts_cells.len()
                    ));
                    continue;
                }
                for (ci, (cell, ts_cell)) in row.iter().zip(&ts_cells).enumerate() {
                    let ts_span =
                        ts_cell.get("colSpan").and_then(Value::as_u64).unwrap_or(1) as u16;
                    let ts_gap = ts_cell.get("gridGap").and_then(Value::as_bool).unwrap_or(false);
                    if (cell.span, cell.gap) != (ts_span, ts_gap) {
                        st.mismatches.push(format!(
                            "{name}: 第 {ri} 行第 {ci} 格 span/gap {:?} vs TS {:?}",
                            (cell.span, cell.gap),
                            (ts_span, ts_gap)
                        ));
                    }
                }
            }
        }
    }
    eprintln!("resolve_table: {} docs, {} tables, 来源 {:?}", st.docs, st.tables, st.by_source);
    assert!(st.tables >= 60, "{}", st.tables);
    assert!(st.mismatches.is_empty(), "{:#?}", st.mismatches);
}

/// `RES-03` 第 4 层：表格样式的 run 属性在段落样式链之后、字符样式链之前。
#[test]
fn res_03_table_style_layer_sits_between_para_and_char_styles() {
    use rsword::semantic::props::{RunProps, Val};
    let bytes =
        std::fs::read(common::corpus_dir("synthetic").join("table-style__001.docx")).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let r = Resolver::new(&doc);
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let t = doc.tables().next().unwrap();
    let v = r.table(dom, t);
    // 首行的条件层带 rPr（GridBlue 的 firstRow 是加粗白字一类）
    let eff = v.cell(0, 0);
    let plain = r.run(None, None, &RunProps::default());
    let with_table = r.run_in_table(Some(&eff.rpr), None, None, &RunProps::default());
    if eff.rpr != RunProps::default() {
        assert_ne!(plain.props, with_table.props, "表格样式层应生效");
    }
    // 直接格式压过表格样式
    let direct = RunProps { bold: Some(false), size: Some(Val::Value(48)), ..Default::default() };
    let e = r.run_in_table(Some(&eff.rpr), None, None, &direct);
    assert_eq!(e.props.bold, Some(false));
    assert_eq!(e.props.size, Some(Val::Value(48)));
    assert!(matches!(
        e.source(rsword::semantic::props::RunPropsField::Bold),
        rsword::resolve::Provenance::Direct
    ));
    // 不给表格层时与 run() 等价
    assert_eq!(r.run_in_table(None, None, None, &direct).props, r.run(None, None, &direct).props);
}
