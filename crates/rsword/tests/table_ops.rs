//! 表格的行列结构操作（`EDIT-03` 表格段，任务 3.8）。
//!
//! 语料在这个域上是空的（TS 的行列编辑在编辑器里做，引擎只收整表 XML），所以正确性全靠这里的
//! 单元用例：每条都按 ECMA-376 §17.4 与 Word 的实际形态断言，不以 TS 为准。

mod common;

use rsword::DiagCode;
use rsword::edit::{BlockPos, EditContext, EditOp, EditSession, NewBlock};
use rsword::model::{Document, TableBlock, TextBlock};
use rsword::xml::{LocalName, NodeId, QName};

fn table_of(s: &EditSession) -> &TableBlock {
    s.document().tables().next().expect("表格")
}

fn grid_widths(s: &EditSession) -> Vec<i32> {
    table_of(s)
        .grid
        .iter()
        .map(|g| g.w.as_ref().and_then(rsword::semantic::props::Val::value).copied().unwrap_or(0))
        .collect()
}

/// 每行 (物理格数, 各格跨度, gridBefore, gridAfter)。
fn shape(s: &EditSession) -> Vec<(usize, Vec<u32>, i32, i32)> {
    let n = |v: &Option<rsword::semantic::props::Val<i32>>| {
        v.as_ref().and_then(rsword::semantic::props::Val::value).copied().unwrap_or(0)
    };
    table_of(s)
        .rows
        .iter()
        .map(|r| {
            (
                r.cells.len(),
                r.cells.iter().map(rsword::model::Cell::grid_span).collect(),
                n(&r.props.grid_before),
                n(&r.props.grid_after),
            )
        })
        .collect()
}

fn cell_text(s: &EditSession, row: usize, cell: usize) -> String {
    table_of(s).rows[row].cells[cell]
        .text_blocks()
        .map(TextBlock::text)
        .collect::<Vec<_>>()
        .join("|")
}

fn assert_refresh_matches_rebuild(s: &mut EditSession, what: &str) {
    let refreshed = s.document().clone();
    let rebuilt = Document::rebuild(s.package_mut()).unwrap();
    assert_eq!(refreshed.main, rebuilt.main, "{what}: refresh 与 rebuild 不一致");
}

fn child_names(s: &EditSession, node: NodeId) -> Vec<&'static str> {
    let dom = s.dom();
    dom.semantic_children(node)
        .filter_map(|n| dom.name(n).and_then(|q| q.local.known_str()))
        .collect()
}

/// 2 列 × 2 行，首行首格有 `tcPr`（底纹 + 宽度）。
fn doc_2x2() -> Vec<u8> {
    common::docx_with_body(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>
             <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/></w:tblGrid>
             <w:tr><w:trPr><w:tblHeader/></w:trPr>
               <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:shd w:val="clear" w:fill="EEEEEE"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
               <w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc>
             </w:tr>
             <w:tr>
               <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc>
               <w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc>
             </w:tr>
           </w:tbl>"#,
    )
}

#[test]
fn edit_03_insert_row_clones_the_template() {
    let bytes = doc_2x2();
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    s.apply(EditOp::InsertRow { table: tbl, at: 1, template: None }, &EditContext::default())
        .unwrap();
    assert_eq!(table_of(&s).rows.len(), 3);
    // 新行在第 1 位，内容是空段落，格数与模板一致
    assert_eq!(cell_text(&s, 1, 0), "");
    assert_eq!(cell_text(&s, 1, 1), "");
    assert_eq!(cell_text(&s, 2, 0), "A2");
    assert_eq!(shape(&s)[1], (2, vec![1, 1], 0, 0));
    // trPr 与 tcPr 是模板的字节克隆（EDIT-03 验收行）
    let saved = document_xml(&s.save().unwrap());
    assert_eq!(
        saved.matches(r#"<w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:shd w:val="clear" w:fill="EEEEEE"/></w:tcPr>"#).count(),
        2,
        "新行首格的 tcPr 与模板逐字节相同"
    );
    assert_eq!(saved.matches("<w:trPr><w:tblHeader/></w:trPr>").count(), 2, "trPr 也克隆");
    assert_eq!(
        saved.matches(r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#).count(),
        2,
        "首段 pPr 克隆"
    );
    assert_refresh_matches_rebuild(&mut s, "InsertRow");
}

#[test]
fn edit_03_insert_row_drops_a_vmerge_continue_from_the_template() {
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>头</w:t></w:r></w:p></w:tc></w:tr>
             <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    // 以第 2 行（continue）为模板插在最后：新行不带 vMerge
    let tpl = table_of(&s).rows[1].node;
    s.apply(EditOp::InsertRow { table: tbl, at: 2, template: Some(tpl) }, &EditContext::default())
        .unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows.len(), 3);
    assert!(t.rows[2].cells[0].props.v_merge.is_none(), "模板是 continue → 新行不带 vMerge");
    assert!(t.rows[1].cells[0].is_vmerge_continue(), "原来的 continue 不动");
    assert_refresh_matches_rebuild(&mut s, "InsertRow(vMerge)");
}

#[test]
fn edit_03_delete_row_promotes_the_next_vmerge_continue() {
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>头</w:t></w:r></w:p></w:tc></w:tr>
             <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>
             <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    s.apply(EditOp::DeleteRow { table: tbl, at: 0 }, &EditContext::default()).unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows.len(), 2);
    assert!(
        t.rows[0].cells[0]
            .props
            .v_merge
            .as_ref()
            .is_some_and(rsword::semantic::props::Merge::is_restart),
        "合并区收缩：原来的 continue 变成 restart"
    );
    assert!(t.rows[1].cells[0].is_vmerge_continue());
    assert_refresh_matches_rebuild(&mut s, "DeleteRow");
}

#[test]
fn edit_03_insert_column_widens_a_spanned_cell_and_moves_bookmarks() {
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr>
               <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>宽</w:t></w:r></w:p></w:tc>
               <w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc>
             </w:tr>
             <w:tr>
               <w:bookmarkStart w:id="1" w:name="cols" w:colFirst="1" w:colLast="2"/>
               <w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc>
               <w:bookmarkEnd w:id="1"/>
             </w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    // 第 1 列落在跨列格中间 → 该格 gridSpan 3，本行格数不变；第二行插入一个新格
    s.apply(EditOp::InsertColumn { table: tbl, at: 1, width: 500 }, &EditContext::default())
        .unwrap();
    assert_eq!(grid_widths(&s), [1000, 500, 1000, 1000]);
    assert_eq!(shape(&s)[0], (2, vec![3, 1], 0, 0), "跨列格加宽，不新增 w:tc");
    assert_eq!(shape(&s)[1], (4, vec![1, 1, 1, 1], 0, 0), "普通行多一个格");
    assert_eq!(cell_text(&s, 1, 1), "", "新格是空段落");
    assert_eq!(
        table_of(&s).rows[0].cells[0]
            .props
            .width
            .as_ref()
            .and_then(rsword::semantic::props::TblWidth::twips),
        Some(2500),
        "跨列格的 tcW 加上新列宽"
    );
    // 书签的列区间跟着右移
    let dom = s.dom();
    let bm = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::BookmarkStart)))
        .unwrap();
    assert_eq!(dom.attr_value(bm, QName::w(LocalName::ColFirst)).as_deref(), Some("2"));
    assert_eq!(dom.attr_value(bm, QName::w(LocalName::ColLast)).as_deref(), Some("3"));
    assert_refresh_matches_rebuild(&mut s, "InsertColumn");
}

#[test]
fn edit_03_delete_column_shrinks_spans_and_removes_cells() {
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr>
               <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>宽</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc>
             </w:tr>
             <w:tr>
               <w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc>
             </w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    s.apply(EditOp::DeleteColumn { table: tbl, at: 1 }, &EditContext::default()).unwrap();
    assert_eq!(grid_widths(&s), [1000, 1000]);
    assert_eq!(shape(&s)[0], (2, vec![1, 1], 0, 0), "gridSpan 2 → 1，元素消失");
    assert!(table_of(&s).rows[0].cells[0].props.grid_span.is_none());
    assert_eq!(shape(&s)[1], (2, vec![1, 1], 0, 0));
    assert_eq!(cell_text(&s, 1, 0), "a");
    assert_eq!(cell_text(&s, 1, 1), "c", "中间的格被删掉");
    assert_refresh_matches_rebuild(&mut s, "DeleteColumn");
}

#[test]
fn edit_03_merge_cells_two_by_two() {
    let bytes = doc_2x2();
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    s.apply(EditOp::MergeCells { table: tbl, from: (0, 0), to: (1, 1) }, &EditContext::default())
        .unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(shape(&s)[0], (1, vec![2], 0, 0), "首行只剩一个跨两列的格");
    assert_eq!(shape(&s)[1], (1, vec![2], 0, 0), "第二行的格保留（vMerge continue）");
    let top = &t.rows[0].cells[0];
    assert!(top.props.v_merge.as_ref().is_some_and(rsword::semantic::props::Merge::is_restart));
    assert!(t.rows[1].cells[0].is_vmerge_continue());
    // 四格文字按文档序进了左上格
    assert_eq!(cell_text(&s, 0, 0), "A1|B1|A2|B2");
    assert_eq!(cell_text(&s, 1, 0), "", "被并入的格留一个空段落");
    assert_refresh_matches_rebuild(&mut s, "MergeCells");
    // 保存后重开形状不变
    let saved = s.save().unwrap();
    let re = EditSession::open(&saved).unwrap();
    assert_eq!(re.document().tables().next().unwrap().rows[0].cells[0].grid_span(), 2);
}

#[test]
fn edit_03_merge_cells_horizontal_only_removes_the_extra_cells() {
    let bytes = doc_2x2();
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    s.apply(EditOp::MergeCells { table: tbl, from: (0, 0), to: (0, 1) }, &EditContext::default())
        .unwrap();
    assert_eq!(shape(&s)[0], (1, vec![2], 0, 0));
    assert_eq!(shape(&s)[1], (2, vec![1, 1], 0, 0), "第二行不动");
    assert_eq!(cell_text(&s, 0, 0), "A1|B1");
    assert!(table_of(&s).rows[0].cells[0].props.v_merge.is_none(), "纯横向合并不写 vMerge");
    assert_refresh_matches_rebuild(&mut s, "MergeCells(横向)");
}

/// `EDIT-05`：几何非法与网格不一致时整体拒绝，DOM / 投影 / 保存字节都不动。
#[test]
fn edit_05_table_ops_reject_bad_geometry_without_touching_state() {
    // 网格不一致的表格（第一行只占 1 列，tblGrid 有 2 列）
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr>
             <w:tr><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    let before = format!("{:?}", s.document().main);
    for op in [
        EditOp::InsertColumn { table: tbl, at: 1, width: 100 },
        EditOp::DeleteColumn { table: tbl, at: 0 },
        EditOp::MergeCells { table: tbl, from: (0, 0), to: (1, 1) },
    ] {
        let name = format!("{op:?}");
        let err = s.apply(op, &EditContext::default()).expect_err(&name);
        assert!(
            matches!(err, rsword::Error::Edit { code, .. } if code == DiagCode::EditTableGridInconsistent),
            "{name}: {err}"
        );
    }
    assert_eq!(format!("{:?}", s.document().main), before);
    assert_eq!(s.save().unwrap(), bytes, "不变式 1");

    // 网格一致但合并区不是整格：EDIT_TABLE_GEOMETRY
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>宽</w:t></w:r></w:p></w:tc></w:tr>
             <w:tr><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    let err = s
        .apply(EditOp::MergeCells { table: tbl, from: (0, 0), to: (1, 0) }, &EditContext::default())
        .expect_err("跨列格与合并区边界不齐");
    assert!(
        matches!(err, rsword::Error::Edit { code, .. } if code == DiagCode::EditTableGeometry),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes);

    // 删掉唯一一列 → 会掏空行，拒绝
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let tbl = table_of(&s).node;
    let err = s
        .apply(EditOp::DeleteColumn { table: tbl, at: 0 }, &EditContext::default())
        .expect_err("会掏空行");
    assert!(
        matches!(err, rsword::Error::Edit { code, .. } if code == DiagCode::EditTableGeometry),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes);
}

/// `InsertBlock{NewBlock::Table}`：新表格的形状、`tblLook`、表头行与每格的空段落。
#[test]
fn edit_03_insert_new_table() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let body = s.document().body.unwrap();
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::end(body),
            block: NewBlock::Table {
                rows: 2,
                cols: 3,
                widths: Some(vec![1000, 2000, 3000]),
                style: Some("TableGrid".into()),
                header: true,
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0].cells.len(), 3);
    assert_eq!(grid_widths(&s), [1000, 2000, 3000]);
    assert_eq!(t.style_id.as_deref(), Some("TableGrid"));
    assert_eq!(t.rows[0].props.tbl_header, Some(true));
    assert_eq!(t.rows[1].props.tbl_header, None);
    assert!(t.grid_consistent());
    assert_eq!(cell_text(&s, 0, 0), "");
    assert_eq!(child_names(&s, t.node)[..2], ["tblPr", "tblGrid"]);
    let look = t.props.look.as_ref().unwrap();
    assert_eq!(look.val.as_deref(), Some("04A0"));
    assert_refresh_matches_rebuild(&mut s, "InsertBlock(Table)");
    // 保存后重开还是同一张表
    let saved = s.save().unwrap();
    let re = EditSession::open(&saved).unwrap();
    let t = re.document().tables().next().unwrap();
    assert_eq!((t.rows.len(), t.rows[0].cells.len()), (2, 3));
    assert!(t.grid_consistent());
}

/// `SAVE-02`：我们自己把网格弄坏时保存报错；输入本来就坏的（语料里有）不拦。
#[test]
fn save_02_grid_mismatch_is_ours_only() {
    // 输入本来就不一致：改格里的字照样能存
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let para = table_of(&s).rows[0].cells[0].text_blocks().next().unwrap().node;
    s.apply(
        EditOp::InsertText {
            at: rsword::edit::InlinePos::new(para, 1),
            text: "!".into(),
            props: None,
        },
        &EditContext::default(),
    )
    .unwrap();
    assert!(s.save().is_ok(), "输入本来就坏的网格不该拦保存");
    assert!(
        s.document().warnings.iter().any(|d| d.code == DiagCode::ModTableShape),
        "解析时应已记 MOD_TABLE_SHAPE"
    );
}

/// 保存结果里的 `word/document.xml`。
fn document_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut f = z.by_name("word/document.xml").unwrap();
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).unwrap();
    s
}

// ---- 恶意输入与随机序列（`TEST-09` / `TEST-07` 的表格子集，任务 3.9）--------------------------

/// `TEST-09`：网格不一致的输入解析成功、诊断是 PreExisting、无编辑保存字节相同、列操作被拒。
#[test]
fn test_09_hostile_table_grid_mismatch() {
    let path = common::corpus_dir("hostile").join("table-grid-mismatch.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut s = EditSession::open(&bytes).unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows.len(), 2);
    assert!(!t.grid_consistent(), "语料就是不一致的");
    let shape = s
        .document()
        .warnings
        .iter()
        .find(|d| d.code == DiagCode::ModTableShape)
        .expect("解析时记 MOD_TABLE_SHAPE");
    assert_eq!(shape.origin, rsword::ValidationOrigin::PreExistingDamage);
    assert_eq!(s.save().unwrap(), bytes, "无编辑保存字节相同");

    // 列操作拒绝，状态不变
    let tbl = table_of(&s).node;
    for op in [
        EditOp::InsertColumn { table: tbl, at: 0, width: 100 },
        EditOp::DeleteColumn { table: tbl, at: 0 },
        EditOp::MergeCells { table: tbl, from: (0, 0), to: (1, 0) },
    ] {
        let err = s.apply(op, &EditContext::default()).expect_err("网格不一致");
        assert!(
            matches!(err, rsword::Error::Edit { code, .. } if code == DiagCode::EditTableGridInconsistent)
        );
    }
    // 行操作与格内编辑不受影响（它们不依赖列几何）
    s.apply(EditOp::InsertRow { table: tbl, at: 1, template: None }, &EditContext::default())
        .unwrap();
    assert_eq!(table_of(&s).rows.len(), 3);
    assert!(s.save().is_ok(), "网格本来就坏，保存不该被 SAVE_TABLE_GRID 拦");
}

/// `TEST-09`：单元格里没有 `w:p` 的输入解析成功、记诊断、保存字节相同；往格里插块会补上段落。
#[test]
fn test_09_hostile_cell_without_paragraph() {
    let path = common::corpus_dir("hostile").join("table-cell-no-paragraph.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut s = EditSession::open(&bytes).unwrap();
    let t = table_of(&s);
    assert_eq!(t.rows[0].cells.len(), 2);
    assert!(t.rows[0].cells[0].blocks.is_empty(), "第一格是空的");
    assert!(
        s.document().warnings.iter().any(|d| d.code == DiagCode::ModTableShape),
        "记 MOD_TABLE_SHAPE"
    );
    assert_eq!(s.save().unwrap(), bytes, "无编辑保存字节相同");

    // 往第一格插一个段落：格尾仍是 w:p
    let cell = table_of(&s).rows[0].cells[0].node;
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::end(cell),
            block: NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("补"))],
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(cell_text(&s, 0, 0), "补");
    assert_eq!(*child_names(&s, cell).last().unwrap(), "p");
    // 第二格以嵌套表结尾：往里插表格后应补空段落
    let cell2 = table_of(&s).rows[0].cells[1].node;
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::end(cell2),
            block: NewBlock::Table { rows: 1, cols: 1, widths: None, style: None, header: false },
        },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(*child_names(&s, cell2).last().unwrap(), "p", "格尾补上 w:p");
    assert_refresh_matches_rebuild(&mut s, "hostile InsertBlock");
}

/// `TEST-07` 的表格子集：随机操作序列，每步断言投影一致、无引擎不变式破坏，定期保存重解析。
#[test]
fn test_07_random_table_edit_sequences() {
    const STEPS: usize = 200;
    let docs: Vec<_> = common::docx_paths("synthetic")
        .into_iter()
        .filter(|p| {
            let n = p.file_name().unwrap().to_str().unwrap();
            n.starts_with("table-display__")
                || n.starts_with("table-edit__")
                || n.starts_with("table-grid-reconcile__")
        })
        .take(10)
        .collect();
    assert!(docs.len() >= 10, "语料里应有足够的表格文档，实得 {}", docs.len());

    let mut applied = 0usize;
    let mut rejected = 0usize;
    for (di, path) in docs.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let mut s = EditSession::open(&bytes).unwrap();
        let mut rng = common::Rng(0x9E37_79B9_7F4A_7C15 ^ di as u64);
        for step in 0..STEPS {
            let Some(op) = random_table_op(&s, &mut rng) else { continue };
            let what = format!("{}: step {step} {op:?}", path.display());
            match s.apply(op, &EditContext::default()) {
                Ok(_) => applied += 1,
                Err(rsword::Error::Edit { .. }) => {
                    rejected += 1;
                    continue; // 拒绝是合法结果；EDIT-05 保证状态没动
                }
                Err(e) => panic!("{what}: 非编辑错误 {e}"),
            }
            // MOD-13：投影 == 重建
            let refreshed = s.document().clone();
            let rebuilt = Document::rebuild(s.package_mut()).unwrap();
            assert_eq!(refreshed.main, rebuilt.main, "{what}: refresh != rebuild");
            // SAVE-02：不能出现引擎不变式破坏
            assert!(
                !s.diagnostics()
                    .iter()
                    .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
                "{what}: {:?}",
                s.diagnostics()
            );
            if step % 20 == 19 {
                let saved = s.save().unwrap_or_else(|e| panic!("{what}: save {e}"));
                let re = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what}: reopen {e}"));
                // 重解析后的块投影与保存前一致（忽略 NodeId：比较文本形状）
                assert_eq!(
                    table_shapes(re.document()),
                    table_shapes(s.document()),
                    "{what}: 保存往返后表格形状变了"
                );
                s = re;
            }
        }
    }
    eprintln!("random table ops: {applied} 次生效，{rejected} 次被拒");
    assert!(applied > 200, "有效操作太少：{applied}");
}

/// 表格形状的可比较快照：每张表的 (行数, 每行各格跨度, 每格文本)。
fn table_shapes(doc: &Document) -> Vec<Vec<(Vec<u32>, Vec<String>)>> {
    doc.tables()
        .map(|t| {
            t.rows
                .iter()
                .map(|r| {
                    (
                        r.cells.iter().map(rsword::model::Cell::grid_span).collect(),
                        r.cells
                            .iter()
                            .map(|c| {
                                c.text_blocks().map(TextBlock::text).collect::<Vec<_>>().join("|")
                            })
                            .collect(),
                    )
                })
                .collect()
        })
        .collect()
}

/// 随机挑一个表格相关的操作；文档里没有表格时 `None`。
fn random_table_op(s: &EditSession, rng: &mut common::Rng) -> Option<EditOp> {
    let tables: Vec<&TableBlock> = s.document().tables().collect();
    if tables.is_empty() {
        return None;
    }
    let t = tables[rng.below(tables.len())];
    let table = t.node;
    let rows = t.rows.len() as u32;
    let cols = t.grid.len().max(1) as u32;
    Some(match rng.below(8) {
        0 => EditOp::InsertRow { table, at: rng.below(rows as usize + 1) as u32, template: None },
        1 if rows > 1 => EditOp::DeleteRow { table, at: rng.below(rows as usize) as u32 },
        2 => EditOp::InsertColumn {
            table,
            at: rng.below(cols as usize + 1) as u32,
            width: 500 + rng.below(1500) as i32,
        },
        3 if cols > 1 => EditOp::DeleteColumn { table, at: rng.below(cols as usize) as u32 },
        4 if rows > 1 && cols > 1 => {
            let r0 = rng.below(rows as usize) as u32;
            let c0 = rng.below(cols as usize) as u32;
            EditOp::MergeCells {
                table,
                from: (r0, c0),
                to: (
                    (r0 + rng.below(2) as u32).min(rows - 1),
                    (c0 + rng.below(2) as u32).min(cols - 1),
                ),
            }
        }
        5 => {
            let cell = pick_cell(t, rng)?;
            EditOp::SetCellProps {
                cell,
                patch: rsword::semantic::props::CellPropsPatch {
                    no_wrap: rsword::semantic::props::Change::Set(rng.below(2) == 0),
                    ..Default::default()
                },
            }
        }
        6 => {
            let para = pick_cell_paragraph(t, rng)?;
            EditOp::InsertText {
                at: rsword::edit::InlinePos::new(para, 0),
                text: "字".into(),
                props: None,
            }
        }
        _ => {
            let (para, len) = pick_cell_paragraph_len(t, rng)?;
            if len == 0 {
                return None;
            }
            EditOp::DeleteRange {
                from: rsword::edit::InlinePos::new(para, 0),
                to: rsword::edit::InlinePos::new(para, 1.min(len)),
            }
        }
    })
}

fn pick_cell(t: &TableBlock, rng: &mut common::Rng) -> Option<NodeId> {
    let row = t.rows.get(rng.below(t.rows.len()))?;
    row.cells.get(rng.below(row.cells.len())).map(|c| c.node)
}

fn pick_cell_paragraph(t: &TableBlock, rng: &mut common::Rng) -> Option<NodeId> {
    pick_cell_paragraph_len(t, rng).map(|(n, _)| n)
}

fn pick_cell_paragraph_len(t: &TableBlock, rng: &mut common::Rng) -> Option<(NodeId, u32)> {
    let row = t.rows.get(rng.below(t.rows.len()))?;
    let cell = row.cells.get(rng.below(row.cells.len()))?;
    let paras: Vec<&TextBlock> = cell.text_blocks().collect();
    let tb = paras.get(rng.below(paras.len()))?;
    Some((tb.node, tb.utf16_len()))
}
