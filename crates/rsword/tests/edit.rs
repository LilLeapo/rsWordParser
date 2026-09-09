//! L4 编辑引擎（任务 1.11 / 1.12，`EDIT-01/02/03/05`，M1 门第二条）。
//! 定位与代理对；`InsertText` 只脏一个 `w:t`、其他 zip 条目 CRC 不变；`DeleteRange` 截断 / 整 run 删除、
//! `SetRunProps` 拆 run；`SetParaProps` 建 `pPr`；
//! `ReplaceInlines`；批操作第 3 步失败 → DOM 与投影与操作前完全一致。
//! 任务 2.2 起：`DeleteRange` 覆盖整个书签范围按 `SPAN-07` 折叠，锚点由 `SPAN-06` 变换维护。

mod common;

use std::io::{Cursor, Read, Write};

use rsword::diag::DiagCode;
use rsword::edit::{
    EditContext, EditOp, EditSession, InlinePos, Loc, NewInline, NewMarker, NewRun, Utf16Offset,
};
use rsword::error::Error;
use rsword::package::Package;
use rsword::semantic::props::{Change, Jc, ParaPropsPatch, RunPropsPatch, Val};
use rsword::span::RangeClass;
use rsword::xml::{Dirty, LocalName, QName, xpath_strings};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn build_docx(document_xml: &str) -> Vec<u8> {
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        [("[Content_Types].xml", ct), ("_rels/.rels", rels), ("word/document.xml", document_xml)]
    {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn doc_with_body(body: &str) -> Vec<u8> {
    build_docx(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    ))
}

fn corpus(name: &str) -> Vec<u8> {
    let path = common::corpus_dir("synthetic").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn para(s: &EditSession, i: usize) -> rsword::xml::NodeId {
    s.nth_text_block(i).expect("text paragraph").node
}

fn para_text(s: &EditSession, i: usize) -> String {
    s.nth_text_block(i).expect("text paragraph").text()
}

fn saved_xml(s: &mut EditSession) -> String {
    let bytes = s.save().unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    pkg.dom(main).unwrap().unwrap().src().to_string()
}

fn edit_code(e: &Error) -> Option<DiagCode> {
    match e {
        Error::Edit { code, .. } => Some(*code),
        _ => None,
    }
}

/// EDIT-02：偏移落在 😀 中间 → Err；原子（段落级 `w:br`）前后偏移差 1；run 内段间 → `InRun`。
#[test]
fn edit_02_locate_offsets_surrogates_and_atoms() {
    let s = EditSession::open(&doc_with_body(
        r#"<w:p><w:r><w:t>😀a</w:t></w:r><w:r><w:tab/><w:t>b</w:t></w:r><w:br w:type="page"/><w:r><w:t>c</w:t></w:r></w:p>"#,
    ))
    .unwrap();
    let p = para(&s, 0);
    let at = |o: u32| s.locate(InlinePos::new(p, o));
    assert_eq!(s.nth_text_block(0).unwrap().utf16_len(), 7);
    assert_eq!(at(0).unwrap(), Loc::Boundary { index: 0 });
    let err = at(1).expect_err("代理对中间");
    assert_eq!(edit_code(&err), Some(DiagCode::EditSplitSurrogate));
    assert_eq!(at(2).unwrap(), Loc::InText { inline: 0, segment: 0, byte: 4 });
    assert_eq!(at(3).unwrap(), Loc::Boundary { index: 1 });
    assert_eq!(at(4).unwrap(), Loc::InRun { inline: 1, segment: 1 });
    assert_eq!(at(5).unwrap(), Loc::Boundary { index: 2 }, "原子之前");
    assert_eq!(at(6).unwrap(), Loc::Boundary { index: 3 }, "原子之后：差 1");
    assert_eq!(at(7).unwrap(), Loc::Boundary { index: 4 });
    assert_eq!(edit_code(&at(8).expect_err("越界")), Some(DiagCode::EditBadPosition));
    assert_eq!(
        edit_code(&s.locate(InlinePos::new(rsword::xml::NodeId(0), 0)).unwrap_err()),
        Some(DiagCode::EditBadPosition)
    );
}

/// EDIT-03 InsertText + M1 门第二条：在干净 run 中间插字 → 只有该 `w:t` 子树变脏，`w:p` 开标签原字节；
/// 保存后其他 zip 条目 CRC 相同，`document.xml` 里其他块的 `originalXml` 原样出现。
#[test]
fn edit_03_insert_text_in_clean_run_keeps_everything_else() {
    let bytes = corpus("insert-and-layout__001.docx");
    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            common::corpus_dir("synthetic").join("insert-and-layout__001.expected.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut s = EditSession::open(&bytes).unwrap();
    let p = para(&s, 1);
    assert!(para_text(&s, 1).starts_with("普通段落,包含"));
    let r = s
        .apply(
            EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: None },
            &EditContext::default(),
        )
        .unwrap();
    assert_eq!(r.offset_delta, vec![(p, Utf16Offset(2), 1)]);
    assert!(para_text(&s, 1).starts_with("普通X段落,包含"), "{}", para_text(&s, 1));
    // 脏状态：w:p DescendantDirty（开标签原字节），第一个 w:r 只有 w:t 子树非 Clean，其他 run Clean
    let dom = s.dom();
    assert_eq!(dom.node(p).dirty, Dirty::DescendantDirty);
    let runs: Vec<_> =
        dom.children(p).iter().copied().filter(|&c| dom.is(c, QName::w(LocalName::R))).collect();
    assert!(runs.len() >= 6);
    assert_eq!(dom.node(runs[0]).dirty, Dirty::DescendantDirty);
    for &r in &runs[1..] {
        assert_eq!(dom.node(r).dirty, Dirty::Clean, "其他 run 必须 Clean");
    }
    let t =
        dom.children(runs[0]).iter().copied().find(|&c| dom.is(c, QName::w(LocalName::T))).unwrap();
    assert_ne!(dom.node(t).dirty, Dirty::Clean);
    // 其他段落 Clean
    for b in &s.document().main {
        if b.node() != p {
            assert_eq!(dom.node(b.node()).dirty, Dirty::Clean);
        }
    }

    let saved = s.save().unwrap();
    // M1 门第二条：其他 zip 条目 CRC / 字节相同
    let mut a = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut b = zip::ZipArchive::new(Cursor::new(&saved)).unwrap();
    assert_eq!(a.len(), b.len());
    for i in 0..a.len() {
        let mut ea = a.by_index(i).unwrap();
        let name = ea.name().to_string();
        let mut eb = b.by_name(&name).unwrap();
        if name == "word/document.xml" {
            assert_ne!(ea.crc32(), eb.crc32());
            continue;
        }
        assert_eq!(ea.crc32(), eb.crc32(), "{name}: CRC 变了");
        let (mut ba, mut bb) = (Vec::new(), Vec::new());
        ea.read_to_end(&mut ba).unwrap();
        eb.read_to_end(&mut bb).unwrap();
        assert_eq!(ba, bb, "{name}: 字节变了");
    }
    // document.xml：其他块的 originalXml 原样出现；改过的 w:t 带 preserve
    let mut again = Package::open(&saved).unwrap();
    let main = again.main_part();
    let dom2 = again.dom(main).unwrap().unwrap();
    let xml = dom2.src().to_string();
    for blk in expected["blocks"].as_array().unwrap() {
        if blk["docxIndex"].as_u64() == Some(1) {
            continue;
        }
        let ox = blk["originalXml"].as_str().unwrap();
        assert!(xml.contains(ox), "块 {} 的 originalXml 应原样出现", blk["docxIndex"]);
    }
    assert_eq!(
        xpath_strings(dom2, "/w:document/w:body/w:p[2]/w:r[1]/w:t/text()").unwrap(),
        ["普通X段落,包含"]
    );
    assert_eq!(
        xpath_strings(dom2, "count(/w:document/w:body/w:p[2]/w:r[1]/w:t[@xml:space='preserve'])")
            .unwrap(),
        ["1"]
    );
    assert!(xml.contains("<w:p><w:r><w:t xml:space=\"preserve\">普通X段落,包含</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>加粗</w:t></w:r>"));
}

/// EDIT-03 InsertText：带 props 或含控制字符 → 边界插入 New run，rPr 克隆自左侧 run 再合并 props；
/// 段中间插入先拆 run。
#[test]
fn edit_03_insert_text_new_run_inherits_and_splits() {
    let mut s = EditSession::open(&doc_with_body(
        r#"<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>abcd</w:t></w:r><w:r><w:rPr><w:color w:val="FF0000"/></w:rPr><w:t>ef</w:t></w:r></w:p>"#,
    ))
    .unwrap();
    let p = para(&s, 0);
    let ctx = EditContext::default();
    // 末尾 + 加粗：新 run 继承 color 并加 b
    let bold = RunPropsPatch { bold: Change::Set(true), ..Default::default() };
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 6), text: "G".into(), props: Some(bold) },
        &ctx,
    )
    .unwrap();
    // 中间 + 制表符：拆 run，新 run 继承 i
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 2), text: "\tX".into(), props: None }, &ctx)
        .unwrap();
    assert_eq!(para_text(&s, 0), "ab\tXcdefG");
    let xml = saved_xml(&mut s);
    let mut pkg = Package::open(&s.save().unwrap()).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    let q = |x: &str| xpath_strings(dom, x).unwrap();
    assert_eq!(q("count(//w:p/w:r)"), ["5"]);
    assert_eq!(q("//w:p/w:r[1]/w:t/text()"), ["ab"]);
    assert_eq!(q("count(//w:p/w:r[2]/w:rPr/w:i)"), ["1"], "新 run 继承左侧 rPr");
    assert_eq!(q("count(//w:p/w:r[2]/w:tab)"), ["1"]);
    assert_eq!(q("//w:p/w:r[2]/w:t/text()"), ["X"]);
    assert_eq!(q("//w:p/w:r[3]/w:t/text()"), ["cd"]);
    assert_eq!(q("count(//w:p/w:r[3]/w:rPr/w:i)"), ["1"], "拆出的右半 rPr 字节克隆");
    assert_eq!(q("//w:p/w:r[5]/w:rPr/w:color/@w:val"), ["FF0000"]);
    assert_eq!(q("count(//w:p/w:r[5]/w:rPr/w:b)"), ["1"]);
    assert_eq!(q("//w:p/w:r[5]/w:t/text()"), ["G"]);
    assert!(
        xml.contains("<w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr>"),
        "PROP-05 顺序: {xml}"
    );
}

/// EDIT-03 DeleteRange：截断 / 整 run 删除 / 原子删除；覆盖整个书签范围 → `SPAN-07` 折叠到删除点
/// （标记物理上正好落在那里，不必重写）；覆盖 REF 字段结果 → 结果 run 删除、字段结构保留（2.4 全删）。
#[test]
fn edit_03_delete_range_truncates_runs_and_keeps_markers() {
    let mut s = EditSession::open(&doc_with_body(
        r#"<w:p><w:r><w:t>abc</w:t></w:r><w:bookmarkStart w:id="7" w:name="bm"/><w:r><w:rPr><w:b/></w:rPr><w:t>def</w:t></w:r><w:bookmarkEnd w:id="7"/><w:br/><w:r><w:t>gh</w:t></w:r></w:p>"#,
    ))
    .unwrap();
    let p = para(&s, 0);
    assert_eq!(para_text(&s, 0), "abcdef\u{FFFC}gh");
    let ctx = EditContext::default();
    let r = s
        .apply(EditOp::DeleteRange { from: InlinePos::new(p, 1), to: InlinePos::new(p, 8) }, &ctx)
        .unwrap();
    assert_eq!(para_text(&s, 0), "ah");
    assert_eq!(r.offset_delta, vec![(p, Utf16Offset(1), -7)]);
    // SPAN-07：书签两端都落进删除区间 → 折叠，范围仍在（`_Toc` / `_Ref` 目标不断链）
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "7").expect("书签仍在索引里");
    assert!(bm.is_collapsed(), "{bm:?}");
    assert_eq!(bm.start.unwrap().index, 1, "锚点落到删除点（r(a) 之后）");
    assert!(bm.start.unwrap().marker.is_some(), "标记还是原来那个节点");
    assert!(
        !s.diagnostics().iter().any(|d| d.code == DiagCode::EditAnchorUnmoved),
        "范围标记不再需要 EDIT_ANCHOR_UNMOVED"
    );
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"<w:bookmarkStart w:id="7" w:name="bm"/>"#), "标记原地保留: {xml}");
    assert!(xml.contains(r#"<w:bookmarkEnd w:id="7"/>"#));
    assert!(!xml.contains("<w:br/>"), "原子被删: {xml}");
    assert!(!xml.contains("def"), "整 run 删除: {xml}");
    assert!(xml.contains(r#"<w:t xml:space="preserve">a</w:t>"#));
    assert!(xml.contains(r#"<w:t xml:space="preserve">h</w:t>"#));

    // REF 字段是原子（`FLD-14`：坐标流里恒为 1 个 U+FFFC，与结果文字长度无关）
    let mut s2 = EditSession::open(&corpus("bookmarks-crossref__006.docx")).unwrap();
    let p2 = para(&s2, 1);
    let text = para_text(&s2, 1);
    assert!(text.starts_with("详见\u{FFFC}"), "REF 结果折成一个原子: {text}");
    // `FLD-07`：删除覆盖原子字段 → begin..end 整个删掉，不留半截结构
    s2.apply(EditOp::DeleteRange { from: InlinePos::new(p2, 2), to: InlinePos::new(p2, 3) }, &ctx)
        .unwrap();
    assert_eq!(para_text(&s2, 1), "详见一节。");
    let saved = s2.save().unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    assert_eq!(xpath_strings(dom, "count(//w:p[2]//w:fldChar)").unwrap(), ["0"], "字段整个删掉");
    assert_eq!(xpath_strings(dom, "count(//w:p[2]//w:instrText)").unwrap(), ["0"]);
    // 反向跨段与越界。（跨段删除本身从 7.5 起支持，见 `tests/para_ops.rs`；
    // 这里 `from` 在 `to` 之后，按位置非法拒绝）
    let e = s2
        .apply(
            EditOp::DeleteRange {
                from: InlinePos::new(p2, 0),
                to: InlinePos::new(para(&s2, 0), 1),
            },
            &ctx,
        )
        .unwrap_err();
    assert_eq!(edit_code(&e), Some(DiagCode::EditBadPosition));
    let e = s2
        .apply(
            EditOp::DeleteRange { from: InlinePos::new(p2, 0), to: InlinePos::new(p2, 99) },
            &ctx,
        )
        .unwrap_err();
    assert_eq!(edit_code(&e), Some(DiagCode::EditBadPosition));
}

/// EDIT-03 SetRunProps：两端拆 run（右半 New、rPr 字节克隆），范围内 run 按 PROP-06 改 rPr；
/// 未覆盖的 run 原字节。
#[test]
fn edit_03_set_run_props_splits_and_patches() {
    let mut s = EditSession::open(&corpus("insert-and-layout__001.docx")).unwrap();
    let p = para(&s, 1);
    let original_second_run = {
        let dom = s.dom();
        let runs: Vec<_> = dom
            .children(p)
            .iter()
            .copied()
            .filter(|&c| dom.is(c, QName::w(LocalName::R)))
            .collect();
        dom.lex_str(&dom.node(runs[2]).lex.as_ref().unwrap().range).to_string()
    };
    let patch = RunPropsPatch { bold: Change::Set(true), ..Default::default() };
    // [2, 8)：第一个 run "普通段落,包含"(7) 的 [2,7) + "加粗"(已加粗) 的 [7,8)
    s.apply(
        EditOp::SetRunProps { from: InlinePos::new(p, 2), to: InlinePos::new(p, 8), patch },
        &EditContext::default(),
    )
    .unwrap();
    assert!(para_text(&s, 1).starts_with("普通段落,包含加粗斜体"));
    let saved = s.save().unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    let q = |x: &str| xpath_strings(dom, x).unwrap();
    assert_eq!(q("/w:document/w:body/w:p[2]/w:r[1]/w:t/text()"), ["普通"]);
    assert_eq!(q("count(/w:document/w:body/w:p[2]/w:r[1]/w:rPr)"), ["0"]);
    assert_eq!(q("/w:document/w:body/w:p[2]/w:r[2]/w:t/text()"), ["段落,包含"]);
    assert_eq!(q("count(/w:document/w:body/w:p[2]/w:r[2]/w:rPr/w:b)"), ["1"]);
    assert_eq!(q("/w:document/w:body/w:p[2]/w:r[3]/w:t/text()"), ["加"]);
    assert_eq!(
        q("count(/w:document/w:body/w:p[2]/w:r[3]/w:rPr/w:b)"),
        ["1"],
        "已加粗：Set(相同) 空计划"
    );
    assert_eq!(q("/w:document/w:body/w:p[2]/w:r[4]/w:t/text()"), ["粗"]);
    assert_eq!(q("count(/w:document/w:body/w:p[2]/w:r[4]/w:rPr/w:b)"), ["1"], "右半 rPr 克隆");
    let xml = dom.src();
    assert!(xml.contains(&original_second_run), "未覆盖的 run 原字节: {original_second_run}");
    assert!(xml.contains(r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">加</w:t></w:r>"#));
}

/// EDIT-03 SetParaProps：无 `pPr` → New `pPr` 插为第一子；有 `pPr` → 子元素按 PROP-05 顺序插入。
#[test]
fn edit_03_set_para_props_creates_or_extends_ppr() {
    let mut s = EditSession::open(&corpus("insert-and-layout__001.docx")).unwrap();
    let plain = para(&s, 1);
    let heading = para(&s, 0);
    let ctx = EditContext::default();
    let center = ParaPropsPatch { jc: Change::Set(Val::Value(Jc::Center)), ..Default::default() };
    s.apply(EditOp::SetParaProps { part: None, para: plain, patch: center }, &ctx).unwrap();
    let keep = ParaPropsPatch { keep_next: Change::Set(true), ..Default::default() };
    s.apply(EditOp::SetParaProps { part: None, para: heading, patch: keep }, &ctx).unwrap();
    assert_eq!(s.nth_text_block(1).unwrap().props.jc, Some(Val::Value(Jc::Center)));
    assert_eq!(s.nth_text_block(0).unwrap().props.keep_next, Some(true));
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t xml:space="preserve">普通段落,包含</w:t></w:r>"#), "{xml}");
    assert!(xml.contains(r#"<w:pPr><w:pStyle w:val="Heading1"/><w:keepNext/></w:pPr>"#), "{xml}");
}

/// EDIT-03 ReplaceInlines：段落内容（含范围标记）全部 Deleted，新内容 New，`pPr` 不动；标记由调用方重发。
#[test]
fn edit_03_replace_inlines_keeps_ppr_and_reemits_markers() {
    let mut s = EditSession::open(&doc_with_body(
        r#"<w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:bookmarkStart w:id="1" w:name="x"/><w:r><w:t>old</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
    ))
    .unwrap();
    let p = para(&s, 0);
    let inlines = vec![
        NewInline::Marker(NewMarker::BookmarkStart { id: "1".into(), name: "x".into() }),
        NewInline::Run(NewRun::text("new ")),
        NewInline::Run(NewRun {
            text: "text".into(),
            props: Some(
                rsword::xml::NewElement::new(QName::w(LocalName::RPr))
                    .with_child(rsword::xml::NewElement::new(QName::w(LocalName::B))),
            ),
        }),
        NewInline::Marker(NewMarker::BookmarkEnd { id: "1".into() }),
    ];
    s.apply(EditOp::ReplaceInlines { part: None, para: p, inlines }, &EditContext::default())
        .unwrap();
    assert_eq!(para_text(&s, 0), "new text");
    let dom = s.dom();
    let ppr =
        dom.children(p).iter().copied().find(|&c| dom.is(c, QName::w(LocalName::PPr))).unwrap();
    assert_eq!(dom.node(ppr).dirty, Dirty::Clean, "pPr 不动");
    let xml = saved_xml(&mut s);
    assert!(xml.contains(
        r#"<w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:bookmarkStart w:id="1" w:name="x"/><w:r><w:t xml:space="preserve">new </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">text</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#
    ), "{xml}");
    assert!(!xml.contains("old"));
}

/// EDIT-05：批操作第 3 步失败 → 整批不生效：无脏节点（保存返回原字节）、投影与操作前相等、诊断不变。
#[test]
fn edit_05_failed_batch_rolls_back_everything() {
    let bytes = corpus("insert-and-layout__001.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let before = s.document().clone();
    let p = para(&s, 1);
    let ctx = EditContext::default();
    let ops = vec![
        EditOp::InsertText { at: InlinePos::new(p, 0), text: "A".into(), props: None },
        EditOp::SetRunProps {
            from: InlinePos::new(p, 1),
            to: InlinePos::new(p, 4),
            patch: RunPropsPatch { italic: Change::Set(true), ..Default::default() },
        },
        EditOp::DeleteRange { from: InlinePos::new(p, 0), to: InlinePos::new(p, 10_000) },
    ];
    let err = s.apply_all(ops, &ctx).expect_err("第 3 步越界");
    assert_eq!(edit_code(&err), Some(DiagCode::EditBadPosition));
    assert!(!s.package().is_dirty(), "回滚后无脏节点");
    assert_eq!(s.document(), &before, "投影与操作前相等");
    assert!(s.diagnostics().is_empty());
    assert_eq!(s.save().unwrap(), bytes, "保存返回原字节");
    // 单个操作失败同样回滚（内部多阶段：SetRunProps 先拆 run 再失败不会留下拆分）
    let e = s
        .apply(
            EditOp::SetRunProps {
                from: InlinePos::new(p, 2),
                to: InlinePos::new(para(&s, 0), 1),
                patch: RunPropsPatch::default(),
            },
            &ctx,
        )
        .unwrap_err();
    assert_eq!(edit_code(&e), Some(DiagCode::EditCrossParagraph));
    assert!(!s.package().is_dirty());
    assert_eq!(s.document(), &before);
    // 成功的批操作正常生效
    let ops = vec![
        EditOp::InsertText { at: InlinePos::new(p, 0), text: "A".into(), props: None },
        EditOp::DeleteRange { from: InlinePos::new(p, 1), to: InlinePos::new(p, 3) },
    ];
    let results = s.apply_all(ops, &ctx).unwrap();
    assert_eq!(results.len(), 2);
    assert!(para_text(&s, 1).starts_with("A段落,包含"), "{}", para_text(&s, 1));
}

/// `EDIT-06`：新外链在 `.rels` 里分配 `rId{max+1}`，其余条目原样；两次分配不撞号。
#[test]
fn edit_06_new_external_relationship_is_allocated_in_the_rels_part() {
    let bytes = corpus("insert-and-layout__001.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let main = s.main_part();
    let before: Vec<String> = s.package().part(main).rels.iter().map(|r| r.id.clone()).collect();
    let rid = s
        .add_external_relationship(main, rsword::package::RelType::Hyperlink, "https://x.test/")
        .unwrap();
    assert!(!before.contains(&rid), "新号不与已有的重复: {rid} vs {before:?}");
    let second = s
        .add_external_relationship(main, rsword::package::RelType::Hyperlink, "https://y.test/")
        .unwrap();
    assert_ne!(rid, second, "两次分配不撞号");
    // 内存视图与保存出来的 `.rels` 都有这条
    let rel = s.package().part(main).rels.by_id(&rid).expect("内存里的 Rels 也更新了");
    assert!(
        matches!(&rel.target, rsword::package::RelTarget::External(t) if t == "https://x.test/")
    );
    let saved = s.save().unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let rels_part = pkg.part(pkg.main_part()).rels_part.unwrap();
    let xml = pkg.dom(rels_part).unwrap().unwrap().src().to_string();
    assert!(
        xml.contains(&format!(r#"Id="{rid}""#)) && xml.contains(r#"Target="https://x.test/""#),
        "{xml}"
    );
    assert!(xml.contains(r#"TargetMode="External""#), "{xml}");
    for id in &before {
        assert!(xml.contains(&format!(r#"Id="{id}""#)), "原有关系还在: {id}");
    }
    // 重开后关系能被解析出来
    let reopened = pkg.part(pkg.main_part()).rels.by_id(&rid).cloned();
    assert!(reopened.is_some(), "{xml}");
}

/// `EDIT-03 InsertText` 在字段原子旁边的边界插入：插入点落在原子**之外**，左邻取字段的 end run、
/// 右邻取 begin run（`SPAN-10` 的同一条道理）。格式从字段结果的最后一个 run 继承。
#[test]
fn edit_03_insert_text_next_to_a_field_atom() {
    const BODY: &str = concat!(
        r#"<w:p><w:r><w:t>ab</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:rPr><w:b/></w:rPr><w:t>7</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        r#"<w:r><w:t>cd</w:t></w:r></w:p>"#
    );
    let ctx = EditContext::default();

    // 字段前的边界（偏移 2）：新 run 插在 begin run 之前，继承左侧 "ab"（无 rPr）
    let mut s = EditSession::open(&doc_with_body(BODY)).unwrap();
    let p = para(&s, 0);
    assert_eq!(para_text(&s, 0), "ab\u{FFFC}cd", "字段原子占 1 个坐标单位");
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 2), text: "\tX".into(), props: None }, &ctx)
        .unwrap();
    assert_eq!(para_text(&s, 0), "ab\tX\u{FFFC}cd");
    let xml = saved_xml(&mut s);
    let inserted = xml.find(">X<").expect("新 run");
    let begin = xml.find("begin").expect("begin run");
    assert!(inserted < begin, "插入点在字段原子之外（begin 之前）: {xml}");
    assert!(!xml[..inserted].contains("<w:b/>"), "继承左侧 run 的空格式: {xml}");

    // 字段后的边界（偏移 3）：新 run 插在 end run 之后，继承字段结果里最后一个 run 的 rPr
    let mut s = EditSession::open(&doc_with_body(BODY)).unwrap();
    let p = para(&s, 0);
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 3), text: "\tX".into(), props: None }, &ctx)
        .unwrap();
    assert_eq!(para_text(&s, 0), "ab\u{FFFC}\tXcd");
    let xml = saved_xml(&mut s);
    let end = xml.find(r#"w:fldCharType="end""#).expect("end run");
    let inserted = xml.find(">X<").expect("新 run");
    let cd = xml.find(">cd<").expect("cd run");
    assert!(end < inserted && inserted < cd, "插入点在 end run 之后、cd 之前: {xml}");
    assert!(
        xml[end..inserted].contains("<w:b/>"),
        "继承字段结果 run 的 rPr（新 run 带 w:b）: {xml}"
    );
    // 字段本身没被动过
    assert_eq!(xml.matches("<w:fldChar").count(), 3, "{xml}");
    assert!(xml.contains(r#"<w:instrText xml:space="preserve"> PAGE </w:instrText>"#), "{xml}");
}
