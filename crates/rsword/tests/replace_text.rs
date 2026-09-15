//! EDIT-03 / EDIT-05 / BIND-03：范围替换的格式来源、修订、坐标与原子性。
mod common;

use common::fingerprint::fingerprint;
use rsword::bind::native::{apply_edit_json, edit_op_to_json};
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, NewComment, RevisionAuthor};
use rsword::model::{Inline, Run};
use rsword::semantic::props::RunProps;
use rsword::xml::xpath_strings;

const SOURCE: &str = r#"<w:rPr xmlns:extra="urn:replace-test"><w:rFonts w:eastAsia="Source"/><w:sz w:val="32"/><extra:custom extra:value="keep" /></w:rPr>"#;
const OTHER: &str = r#"<w:rPr><w:sz w:val="24"/><w:u w:val="single"/></w:rPr>"#;
const UNTOUCHED: &str = "<w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p>";

fn run(text: &str, props: &str) -> String {
    format!("<w:r>{props}<w:t>{text}</w:t></w:r>")
}

fn open(inlines: &str) -> EditSession {
    EditSession::open(&common::docx_with_body(&format!("<w:p>{inlines}</w:p>{UNTOUCHED}"))).unwrap()
}

fn replacement(s: &EditSession, from: u32, to: u32, text: &str) -> EditOp {
    let para = s.nth_text_block(0).unwrap().node;
    EditOp::ReplaceText {
        from: InlinePos::new(para, from),
        to: InlinePos::new(para, to),
        text: text.into(),
    }
}

fn containing<'a>(s: &'a EditSession, text: &str) -> &'a Run {
    s.nth_text_block(0)
        .unwrap()
        .inlines
        .iter()
        .find_map(|i| match i {
            Inline::Run(r) if r.text.contains(text) => Some(r),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {text:?}"))
}

#[test]
fn edit_03_replace_text_uses_first_source_run_and_keeps_raw_properties() {
    let a = run("A", SOURCE);
    let b = run("B", OTHER);
    let cases = [
        (a.clone(), 0, 1, "XYZ", "XYZ"),
        (format!("{a}{b}"), 0, 1, "XYZ", "XYZB"),
        (format!("{b}{a}"), 1, 2, "XYZ", "BXYZ"),
        (format!("{a}{b}"), 0, 2, "XYZ", "XYZ"),
        (
            format!("<w:r>{SOURCE}<w:t>A</w:t><w:lastRenderedPageBreak/></w:r>{b}"),
            0,
            1,
            "XYZ",
            "XYZB",
        ),
        (
            format!("<w:r>{SOURCE}<w:t>A</w:t><w:lastRenderedPageBreak/><w:t>C</w:t></w:r>{b}"),
            1,
            2,
            "XYZ",
            "AXYZB",
        ),
        (format!("{a}{b}"), 0, 1, "X\tY\nZ", "X\tY\nZB"),
        (run("A😀B", SOURCE), 1, 3, "中😎", "A中😎B"),
        (format!("{a}{b}"), 0, 1, "X\0Y😀", "XY😀B"),
    ];
    for (body, from, to, text, expected) in cases {
        let mut s = open(&body);
        let op = replacement(&s, from, to, text);
        s.apply(op, &EditContext::default()).unwrap();
        assert_eq!(s.nth_text_block(0).unwrap().text(), expected, "{body}");
        s.dom().check_dirty_invariants().unwrap();
        let saved = s.save().unwrap();
        let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
        assert!(xml.contains(SOURCE), "源 rPr 必须原字节保留：{xml}");
        assert!(xml.contains(UNTOUCHED));
        let reopened = EditSession::open(&saved).unwrap();
        let needle = if text.starts_with('中') { "中" } else { "X" };
        let edited = containing(&reopened, needle);
        assert_eq!(edited.props, containing(&open(&a), "A").props, "{body}");
        assert_eq!(reopened.nth_text_block(0).unwrap().text(), expected);
    }
}

#[test]
fn edit_03_replace_text_without_rpr_does_not_adopt_neighbor_or_context_defaults() {
    let mut s = open(&format!("{}{}", run("B", OTHER), run("A", "")));
    let defaults = RunProps { bold: Some(true), ..Default::default() };
    let ctx = EditContext::default().with_default_run_props(Some(defaults));
    let op = replacement(&s, 1, 2, "X\nY");
    s.apply(op, &ctx).unwrap();
    let saved = s.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    assert_eq!(containing(&reopened, "X").props, RunProps::default());
    assert_eq!(reopened.nth_text_block(0).unwrap().text(), "BX\nY");
}

#[test]
fn edit_03_replace_text_empty_range_and_empty_replacement() {
    for (from, to, text, expected) in [(0, 1, "", ""), (0, 0, "X", "XA"), (0, 0, "", "A")] {
        let mut s = open(&run("A", SOURCE));
        let before = s.save().unwrap();
        let op = replacement(&s, from, to, text);
        s.apply(op, &EditContext::default()).unwrap();
        assert_eq!(s.nth_text_block(0).unwrap().text(), expected);
        let after = s.save().unwrap();
        if from == to && text.is_empty() {
            assert_eq!(after, before);
        }
    }
}

#[test]
fn edit_03_replace_text_tracking_accepts_and_rejects_with_source_format() {
    let ctx = EditContext::default().with_track_changes(Some(RevisionAuthor {
        author: "Editor".into(),
        date: Some("2026-09-10T00:00:00Z".into()),
    }));
    for (prefix, suffix) in [
        ("", ""),
        (r#"<w:ins w:id="7" w:author="Editor">"#, "</w:ins>"),
        (r#"<w:ins w:id="7" w:author="Other">"#, "</w:ins>"),
        (r#"<w:hyperlink w:anchor="Target">"#, "</w:hyperlink>"),
    ] {
        let body = format!("{prefix}{}{suffix}{}", run("ABC", SOURCE), run("D", OTHER));
        for (from, to, text) in [(0, 3, "XYZ"), (1, 2, "X\tY"), (0, 3, "")] {
            let before = open(&body);
            let mut plain = before.clone();
            let mut tracked = before.clone();
            let op = replacement(&before, from, to, text);
            plain.apply(op.clone(), &EditContext::default()).unwrap();
            tracked.apply(op, &ctx).unwrap();
            assert_eq!(fingerprint(&tracked).accept, fingerprint(&plain).accept, "{body}");
            assert_eq!(fingerprint(&tracked).reject, fingerprint(&before).reject, "{body}");
            let saved = tracked.save().unwrap();
            let mut accepted = EditSession::open(&saved).unwrap();
            let mut rejected = accepted.clone();
            accepted.apply(EditOp::AcceptAll { author: None }, &EditContext::default()).unwrap();
            rejected.apply(EditOp::RejectAll { author: None }, &EditContext::default()).unwrap();
            assert_eq!(fingerprint(&accepted).accept, fingerprint(&plain).accept);
            assert_eq!(fingerprint(&rejected).reject, fingerprint(&before).reject);
            accepted.save().unwrap();
            rejected.save().unwrap();
        }
    }
}

#[test]
fn edit_03_replace_text_preserves_span_policies() {
    let mut s = open(&run("ABC", SOURCE));
    let para = s.nth_text_block(0).unwrap().node;
    let from = InlinePos::new(para, 0);
    let to = from.with_offset(3);
    s.apply_all(
        vec![
            EditOp::AddBookmark { name: "Target".into(), from, to },
            EditOp::AddComment {
                from,
                to,
                comment: NewComment {
                    author: "Editor".into(),
                    text: "note".into(),
                    ..Default::default()
                },
            },
        ],
        &EditContext::default(),
    )
    .unwrap();
    let op = replacement(&s, 0, 3, "XYZ");
    s.apply(op, &EditContext::default()).unwrap();
    let saved = s.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    assert_eq!(containing(&reopened, "XYZ").props, containing(&open(&run("A", SOURCE)), "A").props);
    assert_eq!(xpath_strings(reopened.dom(), "count(//w:bookmarkStart)").unwrap(), ["1"]);
    assert_eq!(xpath_strings(reopened.dom(), "count(//w:bookmarkEnd)").unwrap(), ["1"]);
    assert_eq!(xpath_strings(reopened.dom(), "count(//w:commentRangeStart)").unwrap(), ["0"]);
    assert_eq!(xpath_strings(reopened.dom(), "count(//w:commentReference)").unwrap(), ["0"]);
}

#[test]
fn edit_05_replace_text_bad_positions_and_failed_batch_are_atomic() {
    let mut s = open(&run("A😀B", SOURCE));
    let before = s.save().unwrap();
    for (from, to, text) in [(2, 3, "X"), (0, 2, "X"), (3, 1, "X"), (0, 99, "X"), (0, 1, "\0")] {
        let op = replacement(&s, from, to, text);
        assert!(s.apply(op, &EditContext::default()).is_err());
        assert_eq!(s.save().unwrap(), before);
    }
    let from = InlinePos::new(s.nth_text_block(0).unwrap().node, 0);
    let to = InlinePos::new(s.nth_text_block(1).unwrap().node, 1);
    assert!(
        s.apply(EditOp::ReplaceText { from, to, text: "X".into() }, &EditContext::default())
            .is_err()
    );
    assert_eq!(s.save().unwrap(), before);
    let valid = replacement(&s, 0, 1, "longer");
    let bad = replacement(&s, 0, 999, "invalid");
    assert!(s.apply_all(vec![valid, bad], &EditContext::default()).is_err());
    assert_eq!(s.save().unwrap(), before);
    let mut structural = open("<w:r><w:separator/></w:r>");
    let before = structural.save().unwrap();
    for text in ["X", ""] {
        let op = replacement(&structural, 0, 1, text);
        assert!(structural.apply(op, &EditContext::default()).is_err());
        assert_eq!(structural.save().unwrap(), before);
    }
}

#[test]
fn bind_03_replace_text_wire_matches_native_bytes() {
    let mut native = open(&format!("{}{}", run("A", SOURCE), run("B", OTHER)));
    let mut wire = native.clone();
    let op = replacement(&native, 0, 2, "X\nY");
    let json = edit_op_to_json(&op, native.dom()).unwrap();
    native.apply(op, &EditContext::default()).unwrap();
    apply_edit_json(&mut wire, &json, &EditContext::default()).unwrap();
    assert_eq!(wire.save().unwrap(), native.save().unwrap());
}

#[test]
fn bind_03_replace_text_in_header_keeps_other_parts_byte_identical() {
    let header = format!(
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p>{}{}<w:fldSimple w:instr="PAGE"><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p></w:hdr>"#,
        run("A", SOURCE),
        run("B", OTHER)
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    let bytes = common::docx_with_parts(
        UNTOUCHED,
        &[("word/header1.xml", &header), ("word/_rels/document.xml.rels", rels)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let (&part, hf) = s.document().hf_parts.iter().next().unwrap();
    let para = hf.text_blocks().next().unwrap().node;
    let json = serde_json::json!({
        "op": "replaceText",
        "from": {"part": part.0, "para": para.0, "offset": 0},
        "to": {"part": part.0, "para": para.0, "offset": 1},
        "text": "XYZ"
    });
    apply_edit_json(&mut s, &json.to_string(), &EditContext::default()).unwrap();
    let saved = s.save().unwrap();
    let xml = String::from_utf8(common::part_bytes(&saved, "word/header1.xml")).unwrap();
    assert!(xml.contains(SOURCE));
    let reopened = EditSession::open(&saved).unwrap();
    assert_eq!(
        reopened.document().hf_parts[&part].text_blocks().next().unwrap().text(),
        "XYZB\u{fffc}"
    );
    let compressed = |bytes: &[u8], name: &str| {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let entry = zip.by_name(name).unwrap();
        let start = entry.data_start().unwrap() as usize;
        (entry.crc32(), bytes[start..start + entry.compressed_size() as usize].to_vec())
    };
    for name in
        ["word/document.xml", "_rels/.rels", "[Content_Types].xml", "word/_rels/document.xml.rels"]
    {
        assert_eq!(compressed(&saved, name), compressed(&bytes, name), "{name}");
    }
    // 范围末尾的原子字段必须查页眉的索引，不能误用正文的同号 FieldId。
    for text in ["XYZ", ""] {
        let mut s = EditSession::open(&bytes).unwrap();
        let mut json = json.clone();
        json["to"]["offset"] = serde_json::json!(3);
        json["text"] = serde_json::json!(text);
        apply_edit_json(&mut s, &json.to_string(), &EditContext::default()).unwrap();
        let saved = s.save().unwrap();
        let reopened = EditSession::open(&saved).unwrap();
        assert_eq!(reopened.document().hf_parts[&part].text_blocks().next().unwrap().text(), text);
        assert!(
            !String::from_utf8(common::part_bytes(&saved, "word/header1.xml"))
                .unwrap()
                .contains("fldSimple")
        );
        assert_eq!(
            compressed(&saved, "word/document.xml"),
            compressed(&bytes, "word/document.xml")
        );
    }
}

#[test]
fn edit_03_replace_text_keeps_complex_link_structure_and_result() {
    let begin = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> HYPERLINK "https://example.test" </w:instrText></w:r>"#;
    let separate = r#"<w:fldChar w:fldCharType="separate"/>"#;
    let end = r#"<w:fldChar w:fldCharType="end"/>"#;
    for result in [
        format!("<w:r>{separate}</w:r>{}<w:r>{end}</w:r>", run("A", SOURCE)),
        format!("<w:r>{SOURCE}{separate}<w:t>A</w:t></w:r><w:r>{end}</w:r>"),
        format!("<w:r>{separate}</w:r><w:r>{SOURCE}<w:t>A</w:t>{end}</w:r>"),
    ] {
        for tracked in [false, true] {
            let mut s = open(&format!("{begin}{result}"));
            let mut ctx = EditContext::default();
            if tracked {
                ctx.track_changes = Some(RevisionAuthor { author: "Editor".into(), date: None });
            }
            let op = replacement(&s, 0, 1, "XYZ");
            s.apply(op, &ctx).unwrap();
            if tracked {
                let mut rejected = s.clone();
                rejected
                    .apply(EditOp::RejectAll { author: None }, &EditContext::default())
                    .unwrap();
                assert_eq!(rejected.nth_text_block(0).unwrap().text(), "A");
                rejected.save().unwrap();
                s.apply(EditOp::AcceptAll { author: None }, &EditContext::default()).unwrap();
            }
            let saved = s.save().unwrap();
            let reopened = EditSession::open(&saved).unwrap();
            assert_eq!(reopened.nth_text_block(0).unwrap().text(), "XYZ", "{result}");
            assert!(
                matches!(containing(&reopened, "XYZ").link, Some(rsword::model::Link::Field(_))),
                "{result}"
            );
            assert_eq!(xpath_strings(reopened.dom(), "count(//w:fldChar)").unwrap(), ["3"]);
        }
    }
}

#[test]
fn edit_03_replace_text_tracking_deletes_text_sharing_reference_run() {
    for leading in ["", "<w:r><w:t>P</w:t></w:r>"] {
        for text in ["XYZ", ""] {
            let body =
                format!("{leading}<w:r>{SOURCE}<w:t>A</w:t><w:commentReference w:id=\"7\"/></w:r>");
            let mut s = open(&body);
            let ctx = EditContext::default()
                .with_track_changes(Some(RevisionAuthor { author: "Editor".into(), date: None }));
            let to = s.nth_text_block(0).unwrap().utf16_len();
            let op = replacement(&s, 0, to, text);
            s.apply(op, &ctx).unwrap();
            s.apply(EditOp::AcceptAll { author: None }, &EditContext::default()).unwrap();
            assert_eq!(s.nth_text_block(0).unwrap().text(), text);
            s.save().unwrap();
        }
    }
}

#[test]
fn edit_02_replace_text_reports_own_revision_deletion_offsets() {
    let mut s = open(&format!(
        r#"<w:ins w:id="7" w:author="Editor">{}</w:ins>{}"#,
        run("A", SOURCE),
        run("B", OTHER)
    ));
    let ctx = EditContext::default()
        .with_track_changes(Some(RevisionAuthor { author: "Editor".into(), date: None }));
    let op = replacement(&s, 0, 1, "XYZ");
    let result = s.apply(op, &ctx).unwrap();
    assert_eq!(result.offset_delta.iter().map(|(_, _, delta)| delta).sum::<i32>(), 2);
    let mut old_b_position = 1i32;
    for (_, from, delta) in result.offset_delta {
        if old_b_position >= from.0 as i32 {
            old_b_position += delta;
        }
    }
    assert_eq!(old_b_position, 3);
    assert_eq!(s.nth_text_block(0).unwrap().text(), "XYZB");
}
