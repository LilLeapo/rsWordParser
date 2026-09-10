//! 表格模型验收（`spec/06` MOD-07 / MOD-09 / MOD-13 的表格部分，`TEST-09` 的 `xml-deep-table`，
//! 任务 3.2）：sdt 包裹的行 / 格、`tblPrEx`、声明网格、65 层嵌套的 TooDeep、表格修订的附着位置、
//! 跨表格的块遍历，以及全语料与 TS `TableModel` 的行 / 格计数对照。

mod common;

use std::collections::BTreeMap;

use rsword::diag::DiagCode;
use rsword::edit::EditSession;
use rsword::model::{Block, BlockStep, Document, ProtectedKind, Revision, TableBlock};
use rsword::package::{Package, PartId, Rels};
use rsword::semantic::props::{JcTable, Val, VerticalJc};
use rsword::xml::{Dom, LocalName, QName};
use serde_json::Value;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn doc(body: &str) -> Dom {
    let xml = format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#);
    Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{xml}"))
}

fn build(body: &str) -> (Vec<Block>, Vec<rsword::Diagnostic>) {
    let d = doc(body);
    rsword::model::Document::build_main(&d, None, &Rels::default())
}

fn table(b: &Block) -> &TableBlock {
    match b {
        rsword::model::Block::Table(t) => t,
        other => panic!("not a table: {other:?}"),
    }
}

fn text_of(b: &Block) -> String {
    b.as_text().expect("text block").text()
}

#[test]
fn mod_07_sdt_wrapped_rows_and_cells_tbl_pr_ex_and_declared_grid() {
    let (blocks, diags) = build(
        r#"<w:tbl>
             <w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>
             <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="0"/><w:gridCol/></w:tblGrid>
             <w:bookmarkStart w:id="1" w:name="tblmark"/>
             <w:sdt><w:sdtPr><w:tag w:val="row"/></w:sdtPr><w:sdtContent>
               <w:tr><w:tblPrEx><w:jc w:val="center"/></w:tblPrEx><w:trPr><w:tblHeader/></w:trPr>
                 <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
                 <w:sdt><w:sdtContent><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:sdtContent></w:sdt>
               </w:tr>
             </w:sdtContent></w:sdt>
             <w:bookmarkEnd w:id="1"/>
             <w:tr><w:tc><w:p/></w:tc><w:tc><w:tcPr><w:hMerge w:val="restart"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:hMerge/></w:tcPr><w:p/></w:tc></w:tr>
           </w:tbl>"#,
    );
    assert!(!diags.iter().any(|d| d.code == DiagCode::ModTableShape), "{diags:#?}");
    assert!(!diags.iter().any(|d| d.code == DiagCode::ModUnknownBlock), "{diags:#?}");
    assert_eq!(blocks.len(), 1);
    let t = table(&blocks[0]);
    assert_eq!(t.style_id.as_deref(), Some("TableGrid"));
    assert_eq!(t.props.style.as_deref(), Some("TableGrid"));
    // 声明网格：0 与缺失都保留
    let widths: Vec<Option<Val<i32>>> = t.grid.iter().map(|g| g.w.clone()).collect();
    assert_eq!(widths, [Some(Val::Value(2000)), Some(Val::Value(0)), None]);
    assert_eq!(t.column_count(), 3);
    // sdt 包裹的行与格
    assert_eq!(t.rows.len(), 2);
    let r0 = &t.rows[0];
    assert!(r0.sdt.is_some(), "行带 SdtInfo");
    assert_eq!(r0.tbl_pr_ex.as_ref().unwrap().jc, Some(Val::Value(JcTable::Center)));
    assert_eq!(r0.props.tbl_header, Some(true));
    assert_eq!(r0.cells.len(), 2);
    assert!(r0.cells[0].sdt.is_none() && r0.cells[1].sdt.is_some(), "格带自己的 SdtInfo");
    assert_eq!(r0.cells[0].props.width.as_ref().unwrap().twips(), Some(2000));
    assert_eq!(r0.cells[1].grid_span(), 2);
    assert_eq!(r0.grid_width(), 3);
    assert_eq!(text_of(&r0.cells[0].blocks[0]), "A");
    assert_eq!(text_of(&r0.cells[1].blocks[0]), "B");
    // hMerge 不折叠（MOD-07）
    let r1 = &t.rows[1];
    assert_eq!(r1.cells.len(), 3);
    assert!(!r1.cells[1].is_hmerge_continue() && r1.cells[2].is_hmerge_continue());
    assert_eq!(r1.grid_width(), 3);
    assert!(t.grid_consistent());
    assert_eq!(t.cell(1, 2).map(|c| c.node), Some(r1.cells[2].node));
}

fn nested_tables(levels: usize) -> String {
    let mut s = String::from(r#"<w:p><w:r><w:t>deep</w:t></w:r></w:p>"#);
    for _ in 0..levels {
        s = format!(
            r#"<w:tbl><w:tblGrid><w:gridCol w:w="100"/></w:tblGrid><w:tr><w:tc>{s}<w:p/></w:tc></w:tr></w:tbl>"#
        );
    }
    s
}

#[test]
fn mod_07_sixty_five_levels_the_65th_is_too_deep() {
    let (blocks, diags) = build(&nested_tables(65));
    let mut t = table(&blocks[0]);
    for level in 2..=64 {
        let inner = &t.rows[0].cells[0].blocks[0];
        t = table(inner);
        assert_eq!(t.rows.len(), 1, "level {level}");
    }
    // 第 64 层的格里：第 65 层表格是 TooDeep，后面的空段照常
    let cell = &t.rows[0].cells[0];
    assert_eq!(cell.blocks.len(), 2);
    assert!(
        matches!(&cell.blocks[0], Block::Protected(p) if p.kind == ProtectedKind::TooDeep),
        "{:?}",
        cell.blocks[0]
    );
    assert!(cell.blocks[1].as_text().is_some());
    assert!(diags.iter().any(|d| d.code == DiagCode::ModTooDeep), "{diags:#?}");
    // 64 层不降级
    let (blocks, diags) = build(&nested_tables(64));
    let mut t = table(&blocks[0]);
    for _ in 2..=64 {
        t = table(&t.rows[0].cells[0].blocks[0]);
    }
    assert_eq!(text_of(&t.rows[0].cells[0].blocks[0]), "deep");
    assert!(!diags.iter().any(|d| d.code == DiagCode::ModTooDeep));
}

/// `TEST-09` 行：5000 层嵌套表格解析成功、深层为 TooDeep、无编辑保存字节相同。
#[test]
fn test_09_hostile_deep_table_parses_degrades_and_saves_identical() {
    let path = common::corpus_dir("hostile").join("xml-deep-table.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    assert!(
        doc.blocks().any(|b| matches!(b, Block::Protected(p) if p.kind == ProtectedKind::TooDeep)),
        "深层没有 TooDeep"
    );
    assert!(doc.warnings.iter().any(|d| d.code == DiagCode::ModTooDeep));
    assert!(doc.tables().count() >= 64, "{}", doc.tables().count());
    let out = EditSession::open(&bytes).unwrap().save().unwrap();
    assert_eq!(out, bytes, "无编辑保存字节相同");
}

#[test]
fn mod_09_table_revisions_attach_to_table_row_and_cell() {
    let (blocks, diags) = build(
        r#"<w:tbl>
             <w:tblPr><w:jc w:val="center"/><w:tblPrChange w:id="1" w:author="a"><w:tblPr><w:jc w:val="left"/></w:tblPr></w:tblPrChange></w:tblPr>
             <w:tblGrid><w:gridCol w:w="100"/><w:tblGridChange w:id="2"><w:tblGrid><w:gridCol w:w="50"/></w:tblGrid></w:tblGridChange></w:tblGrid>
             <w:tr><w:trPr><w:cantSplit/><w:trPrChange w:id="3" w:author="a"><w:trPr><w:cantSplit w:val="0"/></w:trPr></w:trPrChange></w:trPr>
               <w:tc><w:tcPr><w:vAlign w:val="center"/><w:cellMerge w:id="4" w:author="a" w:vMerge="cont"/><w:tcPrChange w:id="5" w:author="a"><w:tcPr><w:vAlign w:val="top"/></w:tcPr></w:tcPrChange></w:tcPr><w:p/></w:tc>
             </w:tr>
             <w:ins w:id="9" w:author="b"><w:tr><w:trPr><w:ins w:id="10" w:author="b"/></w:trPr><w:tc><w:tcPr><w:cellIns w:id="11" w:author="b"/></w:tcPr><w:p/></w:tc></w:tr></w:ins>
           </w:tbl>"#,
    );
    assert!(diags.is_empty(), "{diags:#?}");
    let t = table(&blocks[0]);
    assert!(t.revisions.iter().any(
        |r| matches!(r, Revision::TablePropsChange { meta, old } if meta.author.as_deref() == Some("a") && old.jc == Some(Val::Value(JcTable::Left)))
    ), "{:?}", t.revisions);
    let d = doc("");
    let _ = d;
    assert!(t.revisions.iter().any(
        |r| matches!(r, Revision::TableGridChange { meta, .. } if meta.id.as_deref() == Some("2"))
    ));
    let r0 = &t.rows[0];
    assert!(
        r0.revisions.iter().any(
            |r| matches!(r, Revision::RowPropsChange { old, .. } if old.cant_split == Some(false))
        ),
        "{:?}",
        r0.revisions
    );
    let c = &r0.cells[0];
    assert!(
        c.revisions
            .iter()
            .any(|r| matches!(r, Revision::CellMerge(m) if m.id.as_deref() == Some("4")))
    );
    assert!(c.revisions.iter().any(
        |r| matches!(r, Revision::CellPropsChange { old, .. } if old.v_align == Some(Val::Value(VerticalJc::Top)))
    ));
    assert_eq!(c.props.v_align, Some(Val::Value(VerticalJc::Center)), "当前值不受快照影响");
    // 包裹整行的 w:ins 与 trPr/ins 都进 Row.revisions；cellIns 进 Cell.revisions
    let r1 = &t.rows[1];
    let inserts: Vec<Option<&str>> = r1
        .revisions
        .iter()
        .filter_map(|r| match r {
            rsword::model::Revision::Insert(m) => Some(m.id.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(inserts, [Some("9"), Some("10")]);
    assert!(
        matches!(r1.cells[0].revisions.as_slice(), [Revision::CellInsert(m)] if m.id.as_deref() == Some("11"))
    );
}

/// `MOD-09` 验收行的表格部分：语料 `table-revisions__*` 的行 / 格修订。
#[test]
fn mod_09_corpus_table_revisions() {
    let expect = [("table-revisions__001", "del"), ("table-revisions__003", "ins")];
    for (name, kind) in expect {
        let bytes =
            std::fs::read(common::corpus_dir("synthetic").join(format!("{name}.docx"))).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
        let t = doc.tables().next().expect("a table");
        assert_eq!(t.rows.len(), 3, "{name}");
        let row_rev = t.rows[1].revisions.iter().find_map(|r| match (r, kind) {
            (rsword::model::Revision::Delete(m), "del") | (rsword::model::Revision::Insert(m), "ins") => Some(m),
            _ => None,
        });
        let m = row_rev
            .unwrap_or_else(|| panic!("{name}: 第 2 行缺 {kind} 修订：{:?}", t.rows[1].revisions));
        assert_eq!(m.author.as_deref(), Some("Alice"));
        assert_eq!(m.id.as_deref(), Some("11"));
        assert!(t.rows[0].revisions.is_empty() && t.rows[2].revisions.is_empty());
        assert!(
            matches!(t.rows[2].cells[0].revisions.as_slice(), [Revision::CellInsert(_)]),
            "{name}: {:?}",
            t.rows[2].cells[0].revisions
        );
        assert!(t.rows[2].cells[1].revisions.is_empty());
    }
}

#[test]
fn mod_07_table_shape_diagnostics_keep_the_model() {
    let (blocks, diags) = build(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/><w:gridCol w:w="1"/></w:tblGrid>
             <w:tr><w:tc><w:tcPr/></w:tc></w:tr>
             <w:tr></w:tr>
             <w:tr><w:tc><w:p/><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl></w:tc><w:tc><w:p/></w:tc></w:tr>
           </w:tbl>"#,
    );
    let t = table(&blocks[0]);
    assert_eq!(t.rows.len(), 3, "空行也保留（声明值）");
    assert!(t.rows[0].cells[0].blocks.is_empty());
    assert!(!t.grid_consistent());
    let shape: Vec<&str> = diags
        .iter()
        .filter(|d| d.code == DiagCode::ModTableShape)
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(shape.len(), 4, "{shape:#?}");
    assert!(shape.iter().any(|m| m.contains("不以 w:p 结尾")), "{shape:#?}");
    assert!(shape.iter().any(|m| m.contains("没有单元格")), "{shape:#?}");
    assert!(shape.iter().any(|m| m.contains("tblGrid")), "{shape:#?}");
    // 嵌套表自己的行没问题：第三行第一格以表格结尾（缺尾段）算一条，第一格缺段一条，空行一条，网格一条
    assert!(matches!(&t.rows[2].cells[0].blocks[1], Block::Table(_)));
}

#[test]
fn mod_13_blocks_paragraphs_and_block_path_reach_into_cells() {
    let bytes = common::docx_with_body(
        r#"<w:p><w:r><w:t>top</w:t></w:r></w:p>
           <w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc>
             <w:p><w:r><w:t>c1</w:t></w:r></w:p>
             <w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>nested</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
             <w:p/>
           </w:tc></w:tr></w:tbl>
           <w:p><w:r><w:t>tail</w:t></w:r></w:p>"#,
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let texts: Vec<String> = doc.paragraphs().map(|p| p.text()).collect();
    assert_eq!(texts, ["top", "c1", "nested", "", "tail"]);
    assert_eq!(doc.text_blocks().count(), 2, "text_blocks 仍只给顶层");
    assert_eq!(doc.tables().count(), 2);
    assert_eq!(doc.blocks().count(), 7);
    let nested = doc.paragraphs().find(|p| p.text() == "nested").unwrap().node;
    let path = doc.block_path(nested).unwrap();
    assert_eq!(
        path,
        [
            BlockStep::Main(1),
            BlockStep::Cell { row: 0, cell: 0, block: 1 },
            BlockStep::Cell { row: 0, cell: 0, block: 0 }
        ]
    );
    assert_eq!(doc.block_at(&path).unwrap().node(), nested);
    let tail = doc.paragraphs().find(|p| p.text() == "tail").unwrap().node;
    assert_eq!(doc.block_path(tail), Some(vec![BlockStep::Main(2)]));
    assert_eq!(doc.block_path(doc.body.unwrap()), None);
    let mut doc2 = doc.clone();
    assert_eq!(doc2.block_at_mut(&path).unwrap().node(), nested);
    assert_eq!(doc.block_at(&[BlockStep::Cell { row: 0, cell: 0, block: 0 }]), None);
}

#[derive(Default)]
struct Stats {
    docs: usize,
    tables: usize,
    nested: usize,
    rows: usize,
    cells: usize,
    skipped_docs: usize,
    mismatches: Vec<String>,
}

/// TS 模型的一格：`(colSpan, hMerge, gridGap)` 之外只关心它是不是占位。
fn ts_cells(row: &Value) -> usize {
    row.as_array()
        .map(|cells| cells.iter().filter(|c| c.get("gridGap").is_none()).count())
        .unwrap_or(0)
}

/// 对照一张表（TS 在深度 ≥ 8 处扁平化，只比到第 7 层）。
fn compare_table(name: &str, ours: &TableBlock, ts: &Value, depth: usize, st: &mut Stats) {
    st.tables += 1;
    let Some(ts_rows) = ts.get("rows").and_then(Value::as_array) else {
        st.mismatches.push(format!("{name}: TS 表没有 rows"));
        return;
    };
    // TS 丢掉没有格的行
    let rows: Vec<_> = ours.rows.iter().filter(|r| !r.cells.is_empty()).collect();
    if rows.len() != ts_rows.len() {
        st.mismatches.push(format!("{name}: 行数 {} vs TS {}", rows.len(), ts_rows.len()));
        return;
    }
    for (ri, (row, ts_row)) in rows.iter().zip(ts_rows).enumerate() {
        st.rows += 1;
        st.cells += row.cells.len();
        let folded = row.cells.iter().skip(1).filter(|c| c.is_hmerge_continue()).count();
        let expect = ts_cells(ts_row) + folded;
        if row.cells.len() != expect {
            st.mismatches.push(format!(
                "{name}: 第 {ri} 行物理格数 {} ≠ TS 格数 {} + 折叠 {folded}",
                row.cells.len(),
                ts_cells(ts_row)
            ));
        }
        if depth >= 7 {
            continue;
        }
        let ts_cells_arr = ts_row.as_array().cloned().unwrap_or_default();
        let ts_real: Vec<&Value> =
            ts_cells_arr.iter().filter(|c| c.get("gridGap").is_none()).collect();
        // 嵌套表按阅读顺序对照（TS 折叠了 hMerge 的格，这里只在无折叠的行里对照嵌套表）
        if folded > 0 {
            continue;
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            let nested: Vec<&TableBlock> = cell
                .blocks
                .iter()
                .filter_map(|b| match b {
                    rsword::model::Block::Table(t) if t.rows.iter().any(|r| !r.cells.is_empty()) => Some(t),
                    _ => None,
                })
                .collect();
            let ts_nested = ts_real
                .get(ci)
                .and_then(|c| c.get("nestedTables"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if nested.len() != ts_nested.len() {
                st.mismatches.push(format!(
                    "{name}: 第 {ri} 行第 {ci} 格嵌套表 {} vs TS {}",
                    nested.len(),
                    ts_nested.len()
                ));
                continue;
            }
            for (n, tsn) in nested.iter().zip(&ts_nested) {
                st.nested += 1;
                compare_table(name, n, tsn, depth + 1, st);
            }
        }
    }
}

/// 全语料：每张表的行数与 TS `rows.length` 一致，每行物理 `w:tc` 数 = TS 格数（去掉 `gridGap` 占位）
/// + 折叠掉的 `hMerge continue`；嵌套表按阅读顺序对照到第 7 层。
#[test]
fn mod_07_corpus_rows_and_cells_match_ts() {
    let mut st = Stats::default();
    let mut top_level_tables: BTreeMap<&str, usize> = BTreeMap::new();
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
        st.docs += 1;
        let ours: Vec<&TableBlock> = doc
            .main
            .iter()
            .filter_map(|b| match b {
                rsword::model::Block::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        if ours.len() != ts_tables.len() {
            st.skipped_docs += 1;
            eprintln!("table[{name}]: 顶层表格数 {} vs TS {}，跳过", ours.len(), ts_tables.len());
            continue;
        }
        *top_level_tables.entry("compared").or_default() += ours.len();
        for (t, ts_block) in ours.iter().zip(ts_tables) {
            match ts_block.get("table") {
                Some(ts) => compare_table(&name, t, ts, 1, &mut st),
                None => {
                    // TS 的 extractTable 失败（恶意深度）→ 没有 table；我们照常建模
                    *top_level_tables.entry("ts_no_table").or_default() += 1;
                }
            }
        }
    }
    eprintln!(
        "table: {} docs, {} tables ({} nested), {} rows, {} cells, {} docs skipped; {:?}",
        st.docs, st.tables, st.nested, st.rows, st.cells, st.skipped_docs, top_level_tables
    );
    assert!(st.tables >= 80, "{}", st.tables);
    assert!(st.mismatches.is_empty(), "{:#?}", st.mismatches);
    // 顶层 w:tbl 数对不上的文档不该超过个别（sdt 拆分规则）
    assert!(st.skipped_docs <= 2, "{}", st.skipped_docs);
    let _ = (LocalName::Tbl, QName::w(LocalName::Tbl));
}
