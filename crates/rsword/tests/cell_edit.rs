//! 单元格内编辑与容器级刷新（`MOD-13`、`EDIT-02`/`EDIT-03`，任务 3.6）。
//!
//! M2 之前 `EditSession` 只认正文顶层的段落，格里的段落根本定位不到；这里逐个验证既有操作在格内
//! 可用、投影就地刷新、且 `refresh` 的结果与整体 `rebuild` 相等。

mod common;

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::model::{Document, TextBlock};
use rsword::semantic::props::{Change, ParaPropsPatch, RunPropsPatch};
use rsword::xml::{LocalName, QName};

/// 一张 2×2 表格加前后各一个正文段落。
fn table_doc() -> Vec<u8> {
    common::docx_with_body(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p>
           <w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>
             <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
             <w:tr>
               <w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc>
             </w:tr>
             <w:tr>
               <w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p><w:p><w:r><w:t>A2b</w:t></w:r></w:p></w:tc>
               <w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc>
             </w:tr>
           </w:tbl>
           <w:p><w:r><w:t>after</w:t></w:r></w:p>"#,
    )
}

/// `(行, 列, 第几段)` 的单元格段落节点。
fn cell_para(doc: &Document, row: usize, cell: usize, para: usize) -> rsword::xml::NodeId {
    let t = doc.tables().next().expect("表格");
    t.rows[row].cells[cell]
        .text_blocks()
        .nth(para)
        .unwrap_or_else(|| panic!("({row},{cell}) 第 {para} 段"))
        .node
}

fn cell_texts(doc: &Document) -> Vec<Vec<Vec<String>>> {
    doc.tables()
        .next()
        .map(|t| {
            t.rows
                .iter()
                .map(|r| {
                    r.cells
                        .iter()
                        .map(|c| c.text_blocks().map(rsword::model::TextBlock::text).collect())
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 一个节点的元素子节点名（跳过缩进空白）。
fn child_names(s: &EditSession, node: rsword::xml::NodeId) -> Vec<&'static str> {
    let dom = s.dom();
    dom.semantic_children(node)
        .filter_map(|n| dom.name(n).and_then(|q| q.local.known_str()))
        .collect()
}

/// 投影与从 DOM 完整重建的结果一致（`MOD-13` 的 oracle）。
fn assert_refresh_matches_rebuild(s: &mut EditSession, what: &str) {
    let refreshed = s.document().clone();
    let rebuilt = rsword::model::Document::rebuild(s.package_mut()).unwrap();
    assert_eq!(refreshed.main, rebuilt.main, "{what}: refresh 与 rebuild 不一致");
}

#[test]
fn edit_02_locate_reaches_paragraphs_inside_cells() {
    let bytes = table_doc();
    let s = EditSession::open(&bytes).unwrap();
    let doc = s.document();
    // 顶层两段 + 表格块
    assert_eq!(doc.main.len(), 3);
    assert_eq!(doc.text_blocks().count(), 2, "text_blocks 仍只给顶层");
    assert_eq!(doc.paragraphs().count(), 7, "paragraphs 含格内 5 段");
    let node = cell_para(doc, 0, 0, 0);
    assert_eq!(s.text_block(node).map(TextBlock::text).as_deref(), Some("A1"));
    assert!(s.locate(InlinePos::new(node, 1)).is_ok());
}

#[test]
fn edit_03_inline_ops_inside_a_cell() {
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let ctx = EditContext::default();

    // InsertText
    let a1 = cell_para(s.document(), 0, 0, 0);
    s.apply(EditOp::InsertText { at: InlinePos::new(a1, 2), text: "x".into(), props: None }, &ctx)
        .unwrap();
    assert_eq!(cell_texts(s.document())[0][0][0], "A1x");
    assert_refresh_matches_rebuild(&mut s, "InsertText");

    // DeleteRange
    let a1 = cell_para(s.document(), 0, 0, 0);
    s.apply(EditOp::DeleteRange { from: InlinePos::new(a1, 0), to: InlinePos::new(a1, 1) }, &ctx)
        .unwrap();
    assert_eq!(cell_texts(s.document())[0][0][0], "1x");
    assert_refresh_matches_rebuild(&mut s, "DeleteRange");

    // SetRunProps
    let b1 = cell_para(s.document(), 0, 1, 0);
    let patch = RunPropsPatch { bold: Change::Set(true), ..Default::default() };
    s.apply(
        EditOp::SetRunProps { from: InlinePos::new(b1, 0), to: InlinePos::new(b1, 2), patch },
        &ctx,
    )
    .unwrap();
    let tb = s.text_block(cell_para(s.document(), 0, 1, 0)).unwrap();
    assert_eq!(tb.inlines.len(), 1);
    assert_refresh_matches_rebuild(&mut s, "SetRunProps");
    let dom = s.dom();
    let rpr = dom
        .descendants(cell_para(s.document(), 0, 1, 0))
        .find(|&n| dom.is(n, QName::w(LocalName::B)));
    assert!(rpr.is_some(), "格内 run 应加粗");

    // SetParaProps
    let a2 = cell_para(s.document(), 1, 0, 0);
    let patch = ParaPropsPatch { keep_next: Change::Set(true), ..Default::default() };
    s.apply(EditOp::SetParaProps { part: None, para: a2, patch }, &ctx).unwrap();
    assert_eq!(s.text_block(a2).unwrap().props.keep_next, Some(true));
    assert_refresh_matches_rebuild(&mut s, "SetParaProps");
}

#[test]
fn edit_03_paragraph_structure_ops_inside_a_cell() {
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let ctx = EditContext::default();

    // SplitParagraph：格里从 2 段变 3 段
    let a2 = cell_para(s.document(), 1, 0, 0);
    s.apply(EditOp::SplitParagraph { at: InlinePos::new(a2, 1) }, &ctx).unwrap();
    assert_eq!(cell_texts(s.document())[1][0], ["A", "2", "A2b"]);
    assert_refresh_matches_rebuild(&mut s, "SplitParagraph");

    // MergeWithNext：合回去
    let first = cell_para(s.document(), 1, 0, 0);
    s.apply(EditOp::MergeWithNext { part: None, para: first }, &ctx).unwrap();
    assert_eq!(cell_texts(s.document())[1][0], ["A2", "A2b"]);
    assert_refresh_matches_rebuild(&mut s, "MergeWithNext");

    // AddBookmark / AddComment 落在格内
    let b2 = cell_para(s.document(), 1, 1, 0);
    s.apply(
        EditOp::AddBookmark {
            name: "cellmark".into(),
            from: InlinePos::new(b2, 0),
            to: InlinePos::new(b2, 2),
        },
        &ctx,
    )
    .unwrap();
    assert!(
        s.spans().unwrap().spans().iter().any(|sp| sp.kind.bookmark_name() == Some("cellmark"))
    );
    assert_refresh_matches_rebuild(&mut s, "AddBookmark");
}

#[test]
fn edit_03_block_ops_keep_the_cell_ending_in_a_paragraph() {
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let ctx = EditContext::default();
    let dom_ends_with_p = |s: &EditSession, row: usize, col: usize| {
        let doc = s.document();
        let tc = doc.tables().next().unwrap().rows[row].cells[col].node;
        let dom = s.dom();
        let last = dom.children(tc).iter().copied().rev().find(|&c| {
            dom.element(c).is_some() && dom.node(c).dirty != rsword::xml::Dirty::Deleted
        });
        last.is_some_and(|n| dom.is(n, QName::w(LocalName::P)))
    };

    // DeleteBlock 删掉格里唯一的段落 → 自动补一个空段落
    let a1 = cell_para(s.document(), 0, 0, 0);
    s.apply(EditOp::DeleteBlock { part: None, node: a1 }, &ctx).unwrap();
    assert!(dom_ends_with_p(&s, 0, 0), "删空的格要补 w:p");
    assert_eq!(cell_texts(s.document())[0][0], [""], "补的是空段落");
    assert_refresh_matches_rebuild(&mut s, "DeleteBlock");

    // DeleteBlock 删掉两段中的一段：不补
    let a2b = cell_para(s.document(), 1, 0, 1);
    s.apply(EditOp::DeleteBlock { part: None, node: a2b }, &ctx).unwrap();
    assert_eq!(cell_texts(s.document())[1][0], ["A2"]);
    assert!(dom_ends_with_p(&s, 1, 0));
    assert_refresh_matches_rebuild(&mut s, "DeleteBlock(2)");

    // InsertBlock：段落插到格内
    let b2 = s.document().tables().next().unwrap().rows[1].cells[1].node;
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos::end(b2),
            block: rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("新段"))],
            },
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(cell_texts(s.document())[1][1], ["B2", "新段"]);
    assert_refresh_matches_rebuild(&mut s, "InsertBlock");
}

/// 保存后重开：格内的改动落到了字节里，其他条目不动。
#[test]
fn edit_03_cell_edit_round_trips() {
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let a1 = cell_para(s.document(), 0, 0, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(a1, 2), text: "!".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    let saved = s.save().unwrap();
    let re = EditSession::open(&saved).unwrap();
    assert_eq!(cell_texts(re.document())[0][0][0], "A1!");
    assert_eq!(cell_texts(re.document())[0][1][0], "B1", "别的格不动");
    // 顶层段落原字节不变
    let doc_xml = re.package().part(re.main_part()).dom().unwrap().src().to_string();
    assert!(doc_xml.contains("<w:t>before</w:t>") && doc_xml.contains("<w:t>after</w:t>"));
    assert!(doc_xml.contains("<w:t>B1</w:t>"));
}

/// 全语料：在一个**单元格**段落里插一个字，保存后其他 zip 条目原样、重解析后只有那张表变了。
/// M3 门第 2 条（`spec/14`）。
#[test]
#[cfg(feature = "compat-ts")]
fn test_04_corpus_cell_edit_fidelity() {
    let mut edited = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut s) = EditSession::open(&bytes) else { continue };
        // 找一个有非空文字的格内段落
        let mut target = None;
        'find: for t in s.document().tables() {
            for row in &t.rows {
                for cell in &row.cells {
                    for tb in cell.text_blocks() {
                        if !tb.text().is_empty() && tb.utf16_len() > 0 {
                            target = Some((tb.node, tb.text()));
                            break 'find;
                        }
                    }
                }
            }
        }
        let Some((node, before)) = target else { continue };
        let main_name = s.package().part(s.main_part()).uri.to_string();
        // 空媒体表：这个 oracle 比的是「编辑前后哪些块变了」，两侧同一张表就够（同 tests/save.rs）
        let media = rsword::bind::compat_ts::MediaSet::default();
        let before_blocks =
            rsword::bind::compat_ts::parsed_doc_of(s.package(), s.document(), &media)["blocks"]
                .as_array()
                .unwrap()
                .clone();

        if s.apply(
            EditOp::InsertText { at: InlinePos::new(node, 0), text: "Ж".into(), props: None },
            &EditContext::default(),
        )
        .is_err()
        {
            continue; // 锁定的内容控件等（EDIT-03 拒绝）
        }
        assert_eq!(
            s.text_block(node).unwrap().text(),
            format!("Ж{before}"),
            "{}: 格内投影文本不正确",
            path.display()
        );
        let saved = s.save().unwrap_or_else(|e| panic!("{}: save failed: {e}", path.display()));

        let before_entries = zip_entries(&bytes);
        let after_entries = zip_entries(&saved);
        assert_eq!(before_entries.len(), after_entries.len(), "{}", path.display());
        for (b, a) in before_entries.iter().zip(&after_entries) {
            assert_eq!(a.0, b.0, "{}", path.display());
            if a.0 != main_name {
                assert_eq!((a.1, &a.2), (b.1, &b.2), "{}: {} 变了", path.display(), a.0);
            }
        }

        let re = EditSession::open(&saved).unwrap();
        let after_blocks =
            rsword::bind::compat_ts::parsed_doc_of(re.package(), re.document(), &media)["blocks"]
                .as_array()
                .unwrap()
                .clone();
        assert_eq!(before_blocks.len(), after_blocks.len(), "{}", path.display());
        let changed: Vec<usize> = before_blocks
            .iter()
            .zip(&after_blocks)
            .enumerate()
            .filter_map(|(i, (b, a))| (b != a).then_some(i))
            .collect();
        assert_eq!(changed.len(), 1, "{}: 除目标表格外还有块变了：{changed:?}", path.display());
        assert_eq!(after_blocks[changed[0]]["type"], "table", "{}: 变的应是表格块", path.display());
        edited += 1;
    }
    eprintln!("cell edit fidelity: {edited} 份文档");
    assert!(edited >= 40, "语料里含表格的文档应有几十份，实得 {edited}");
}

/// zip 条目：(名字, CRC, 压缩字节)。
#[cfg(feature = "compat-ts")]
fn zip_entries(bytes: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index_raw(i).unwrap();
            let mut raw = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut raw).unwrap();
            (f.name().to_string(), f.crc32(), raw)
        })
        .collect()
}

// ---- 表格属性操作（`EDIT-03`，任务 3.7）----------------------------------------------------------

/// 三个属性操作：容器缺失时按 `PROP-05` 的位置新建，未建模子元素与其他属性原样保留。
#[test]
fn edit_03_table_row_cell_props() {
    use rsword::semantic::props::{
        CellPropsPatch, RowPropsPatch, TablePropsPatch, TblWidth, VerticalJc,
    };
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let ctx = EditContext::default();
    let tbl = s.document().tables().next().unwrap().node;

    // 表格：已有 tblPr，新元素按 schema 序号插入（tblStyle 在 tblW 之前）
    s.apply(
        EditOp::SetTableProps {
            table: tbl,
            patch: TablePropsPatch { style: Change::Set("TableGrid".into()), ..Default::default() },
        },
        &ctx,
    )
    .unwrap();
    let t = s.document().tables().next().unwrap();
    assert_eq!(t.style_id.as_deref(), Some("TableGrid"));
    assert!(t.props.width.as_ref().unwrap().kind.is_some(), "原有 tblW 保留");
    let dom = s.dom();
    let tbl_pr =
        dom.semantic_children(tbl).find(|&n| dom.is(n, QName::w(LocalName::TblPr))).unwrap();
    assert_eq!(child_names(&s, tbl_pr), ["tblStyle", "tblW"], "新元素按 PROP-05 顺序插入");
    assert_refresh_matches_rebuild(&mut s, "SetTableProps");

    // 行：没有 trPr，要新建；本例没有 tblPrEx，插为第一个子元素
    let row = s.document().tables().next().unwrap().rows[0].node;
    s.apply(
        EditOp::SetRowProps {
            row,
            patch: RowPropsPatch { tbl_header: Change::Set(true), ..Default::default() },
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(s.document().tables().next().unwrap().rows[0].props.tbl_header, Some(true));
    assert_eq!(child_names(&s, row)[0], "trPr", "trPr 应是第一个子元素");
    assert_refresh_matches_rebuild(&mut s, "SetRowProps");

    // 格：已有 tcPr（A1 有 tcW），加 vAlign
    let cell = s.document().tables().next().unwrap().rows[0].cells[0].node;
    s.apply(
        EditOp::SetCellProps {
            cell,
            patch: CellPropsPatch {
                v_align: Change::Set(rsword::semantic::props::Val::Value(VerticalJc::Center)),
                ..Default::default()
            },
        },
        &ctx,
    )
    .unwrap();
    let c = &s.document().tables().next().unwrap().rows[0].cells[0];
    assert_eq!(c.props.v_align.as_ref().unwrap().value(), Some(&VerticalJc::Center));
    assert_eq!(c.props.width.as_ref().and_then(TblWidth::twips), Some(2000), "原有 tcW 保留");
    assert_refresh_matches_rebuild(&mut s, "SetCellProps");

    // 格里没有 tcPr 时新建为第一个子元素
    let b1 = s.document().tables().next().unwrap().rows[0].cells[1].node;
    s.apply(
        EditOp::SetCellProps {
            cell: b1,
            patch: CellPropsPatch { no_wrap: Change::Set(true), ..Default::default() },
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(child_names(&s, b1)[0], "tcPr", "tcPr 应是第一个子元素");
    assert_refresh_matches_rebuild(&mut s, "SetCellProps(新建)");

    // 保存后重开，属性都在
    let saved = s.save().unwrap();
    let re = EditSession::open(&saved).unwrap();
    let t = re.document().tables().next().unwrap();
    assert_eq!(t.style_id.as_deref(), Some("TableGrid"));
    assert_eq!(t.rows[0].props.tbl_header, Some(true));
    assert_eq!(t.rows[0].cells[1].props.no_wrap, Some(true));
}

/// `w:trPr` 必须排在 `w:tblPrEx` 之后（`PROP-05` 的 `w:tr` 子元素顺序）。
#[test]
fn prop_05_tr_pr_goes_after_tbl_pr_ex() {
    use rsword::semantic::props::RowPropsPatch;
    let bytes = common::docx_with_body(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid>
             <w:tr><w:tblPrEx><w:tblW w:w="5000" w:type="pct"/></w:tblPrEx>
               <w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc></w:tr>
           </w:tbl>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let row = s.document().tables().next().unwrap().rows[0].node;
    s.apply(
        EditOp::SetRowProps {
            row,
            patch: RowPropsPatch { cant_split: Change::Set(true), ..Default::default() },
        },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(child_names(&s, row), ["tblPrEx", "trPr", "tc"], "trPr 插在 tblPrEx 之后");
    assert_eq!(s.document().tables().next().unwrap().rows[0].props.cant_split, Some(true));
    // 行级例外仍在
    assert!(s.document().tables().next().unwrap().rows[0].tbl_pr_ex.is_some());
}

/// 目标节点类型不对 → `Err(EDIT_BAD_POSITION)`，状态不变（`EDIT-05`）。
#[test]
fn edit_05_table_props_reject_wrong_targets() {
    use rsword::semantic::props::CellPropsPatch;
    let bytes = table_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let para = cell_para(s.document(), 0, 0, 0);
    let before = format!("{:?}", s.document().main);
    let err = s
        .apply(
            EditOp::SetCellProps {
                cell: para,
                patch: CellPropsPatch { no_wrap: Change::Set(true), ..Default::default() },
            },
            &EditContext::default(),
        )
        .expect_err("段落不是单元格");
    assert!(
        matches!(err, rsword::Error::Edit { code, .. } if code == rsword::DiagCode::EditBadPosition)
    );
    assert_eq!(format!("{:?}", s.document().main), before);
    assert_eq!(s.save().unwrap(), bytes, "不变式 1");
}
