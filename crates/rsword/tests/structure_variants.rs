//! handoff §5.3 的等价 XML 结构变体。
//!
//! 只声明等价前提：命名空间 URI 相同、直接格式声明值相同、透明 wrapper 不改变可见文本时，
//! 不同合法 XML 形态应投影为相同语义。比较的是测试侧规范化的 `(字符, bold, italic)` 区间，
//! 不比较 NodeId、run 数或字节。

mod common;

use std::io::{Cursor, Write};

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, Utf16Offset};
use rsword::model::{BreakKind, Inline, SegmentKind, TextBlock};
use rsword::package::{Package, PackageFlavor, PartFlavor};
use rsword::semantic::props::{Change, RunPropsPatch};

const W_T: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_S: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    ch: char,
    bold: Option<bool>,
    italic: Option<bool>,
}

fn docx_with_root(document_xml: &str) -> Vec<u8> {
    let ct = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
        r#"</Types>"#
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"</Relationships>"#
    );
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        [("[Content_Types].xml", ct), ("_rels/.rels", rels), ("word/document.xml", document_xml)]
    {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn docx_transitional(body: &str) -> Vec<u8> {
    docx_with_root(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W_T}"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    ))
}

fn docx_prefixed_x(body: &str) -> Vec<u8> {
    docx_with_root(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><x:document xmlns:x="{W_T}"><x:body>{body}<x:sectPr/></x:body></x:document>"#
    ))
}

fn docx_default_ns(body: &str) -> Vec<u8> {
    docx_with_root(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><document xmlns="{W_T}" xmlns:w="{W_T}"><body>{body}<sectPr/></body></document>"#
    ))
}

fn cells(block: &TextBlock) -> Vec<Cell> {
    let mut out = Vec::new();
    for inline in &block.inlines {
        match inline {
            Inline::Run(run) => {
                let style = (run.props.bold, run.props.italic);
                for seg in &run.segments {
                    let mut push = |ch: char| {
                        out.push(Cell { ch, bold: style.0, italic: style.1 });
                    };
                    match &seg.kind {
                        SegmentKind::Text | SegmentKind::DelText => {
                            for ch in run.segment_text(seg).chars() {
                                push(ch);
                            }
                        }
                        SegmentKind::Tab | SegmentKind::PTab { .. } => push('\t'),
                        SegmentKind::Br { kind, .. } => push(match kind {
                            BreakKind::TextWrapping => '\n',
                            BreakKind::Page | BreakKind::Column => '\u{FFFC}',
                        }),
                        SegmentKind::Cr => push('\n'),
                        SegmentKind::NoBreakHyphen => push('\u{2011}'),
                        SegmentKind::SoftHyphen => push('\u{00AD}'),
                        SegmentKind::Ink
                        | SegmentKind::FootnoteRefMark
                        | SegmentKind::EndnoteRefMark => {}
                        _ if seg.utf16_len == 0 => {}
                        _ => push('\u{FFFC}'),
                    }
                }
            }
            Inline::Field { .. } | Inline::Atom(_) => {
                out.push(Cell { ch: '\u{FFFC}', bold: None, italic: None });
            }
        }
    }
    out
}

fn only_cells(docx: &[u8]) -> Vec<Cell> {
    let s = EditSession::open(docx).unwrap();
    cells(s.document().text_blocks().next().expect("paragraph"))
}

fn insert_and_set_bold(docx: &[u8]) -> (EditSession, Vec<Cell>) {
    let mut s = EditSession::open(docx).unwrap();
    let para = s.document().text_blocks().next().unwrap().node;
    s.apply(
        EditOp::InsertText {
            at: InlinePos { part: None, para, offset: Utf16Offset(1) },
            text: "😀x".into(),
            props: Some(RunPropsPatch {
                bold: Change::Set(true),
                italic: Change::Set(false),
                ..Default::default()
            }),
        },
        &EditContext::default(),
    )
    .unwrap();
    s.apply(
        EditOp::SetRunProps {
            from: InlinePos { part: None, para, offset: Utf16Offset(0) },
            to: InlinePos { part: None, para, offset: Utf16Offset(4) },
            patch: RunPropsPatch { italic: Change::Set(true), ..Default::default() },
        },
        &EditContext::default(),
    )
    .unwrap();
    let got = cells(s.document().text_blocks().next().unwrap());
    (s, got)
}

#[test]
fn mod_06_namespace_prefix_default_ns_and_attribute_order_are_equivalent() {
    let canonical = concat!(
        r#"<w:p><w:r><w:rPr><w:b/><w:i w:val="false"/></w:rPr>"#,
        r#"<w:t xml:space="preserve">ab😀</w:t></w:r></w:p>"#
    );
    let prefix = concat!(
        r#"<x:p><x:r><x:rPr><x:i x:val="0"/><x:b x:val="1"/></x:rPr>"#,
        r#"<x:t xml:space="preserve">ab😀</x:t></x:r></x:p>"#
    );
    let default = concat!(
        r#"<p><r><rPr><b w:val="true" w:rsidB="00AA"/><i w:rsidI="00BB" w:val="off"/></rPr>"#,
        r#"<t xml:space="preserve">ab😀</t></r></p>"#
    );
    let expected = vec![
        Cell { ch: 'a', bold: Some(true), italic: Some(false) },
        Cell { ch: 'b', bold: Some(true), italic: Some(false) },
        Cell { ch: '😀', bold: Some(true), italic: Some(false) },
    ];
    assert_eq!(only_cells(&docx_transitional(canonical)), expected);
    assert_eq!(only_cells(&docx_prefixed_x(prefix)), expected);
    assert_eq!(only_cells(&docx_default_ns(default)), expected);
}

#[test]
fn mod_06_on_off_equivalent_spellings_and_explicit_false_are_distinct_from_absent() {
    for on in ["<w:b/>", r#"<w:b w:val="1"/>"#, r#"<w:b w:val="true"/>"#, r#"<w:b w:val="on"/>"#] {
        let docx =
            docx_transitional(&format!(r#"<w:p><w:r><w:rPr>{on}</w:rPr><w:t>x</w:t></w:r></w:p>"#));
        let got = only_cells(&docx);
        assert_eq!(got[0].bold, Some(true), "{on}");
    }
    for off in [r#"<w:b w:val="0"/>"#, r#"<w:b w:val="false"/>"#, r#"<w:b w:val="off"/>"#] {
        let docx = docx_transitional(&format!(
            r#"<w:p><w:r><w:rPr>{off}</w:rPr><w:t>x</w:t></w:r></w:p>"#
        ));
        let got = only_cells(&docx);
        assert_eq!(got[0].bold, Some(false), "{off}");
    }
    let absent = only_cells(&docx_transitional(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#));
    assert_eq!(absent[0].bold, None);
}

#[test]
fn edit_03_equivalent_run_splits_have_identical_edit_semantics() {
    let one = docx_transitional(concat!(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">ab</w:t></w:r></w:p>"#
    ));
    let split = docx_transitional(concat!(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>a</w:t></w:r>"#,
        r#"<w:r><w:rPr><w:b/></w:rPr><w:t>b</w:t></w:r></w:p>"#
    ));
    let (mut one_session, one_cells) = insert_and_set_bold(&one);
    let (mut split_session, split_cells) = insert_and_set_bold(&split);
    assert_eq!(one_cells, split_cells);
    assert_eq!(
        one_session.document().text_blocks().next().unwrap().text(),
        split_session.document().text_blocks().next().unwrap().text()
    );
    assert_ne!(one_session.save().unwrap(), split_session.save().unwrap(), "物理 run 形态可以不同");
}

#[test]
fn pkg_08_strict_transitional_and_mixed_parts_keep_their_flavor_and_no_edit_bytes() {
    let strict =
        std::fs::read(common::corpus_dir("synthetic").join("extra__strict-minimal.docx")).unwrap();
    let mut strict_pkg = Package::open(&strict).unwrap();
    assert_eq!(strict_pkg.flavor(), PackageFlavor::Strict);
    assert_eq!(strict_pkg.flavor_of(strict_pkg.main_part()), PartFlavor::Strict);
    assert_eq!(strict_pkg.save().unwrap(), strict);

    let mut strict_session = EditSession::open(&strict).unwrap();
    let strict_para = strict_session.document().text_blocks().next().unwrap().node;
    strict_session
        .apply(
            EditOp::InsertText {
                at: InlinePos { part: None, para: strict_para, offset: Utf16Offset(0) },
                text: "S".into(),
                props: None,
            },
            &EditContext::default(),
        )
        .unwrap();
    let strict_saved = strict_session.save().unwrap();
    let strict_xml = common::part_bytes(&strict_saved, "word/document.xml");
    let strict_xml = String::from_utf8(strict_xml).unwrap();
    assert!(strict_xml.contains(W_S));
    assert!(!strict_xml.contains(W_T));

    let mixed =
        std::fs::read(common::corpus_dir("synthetic").join("extra__mixed-flavor.docx")).unwrap();
    let mut mixed_pkg = Package::open(&mixed).unwrap();
    assert_eq!(mixed_pkg.flavor(), PackageFlavor::Mixed);
    assert_eq!(mixed_pkg.save().unwrap(), mixed, "Mixed 无编辑必须整包字节相同");
}
