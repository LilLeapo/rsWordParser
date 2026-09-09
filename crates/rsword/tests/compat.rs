#![cfg(feature = "compat-ts")]
//! `compat_ts` 差分（任务 1.10 / 1.15，`COMPAT-02/03/04/06/07`，M1 门第一条）：对 `corpus/synthetic` 里的
//! "文本段落"用例（paragraph / heading / listItem，无字段 / 表格 / 绘图 / 批注 / 脚注），适配器输出
//! 与 TS `.expected.json` 的差异除 `KNOWN_DIFFS.md` 外为 0。判定与已知差异清单与 `tools/diff-parse` 共用。

mod common;

use std::io::{Cursor, Write};

use rsword::bind::compat_ts::{
    Report, diff_json, is_text_case, known_diffs, parsed_doc, split_known,
};
use rsword::package::Package;
use serde_json::Value;

#[test]
fn compat_02_text_cases_match_ts_parsed_doc() {
    let known = known_diffs();
    let mut docs = 0;
    let mut report = Report::default();
    for path in common::docx_paths("synthetic") {
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).unwrap();
        docs += 1;
        if !is_text_case(&expected) {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let actual = parsed_doc(&mut pkg).unwrap();
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (unknown, k) = split_known(diffs, &file, &known);
        report.add(&file, unknown, k);
    }
    eprintln!(
        "compat: {docs} docs with expected.json, {} text cases, {} known diffs, {} unknown",
        report.docs, report.known, report.unknown
    );
    for (k, st) in &report.by_path {
        eprintln!(
            "compat: DIFF {k} ×{} ({} docs); e.g. {}: {} TS={:?} ours={:?}",
            st.count, st.docs, st.sample_doc, st.sample_path, st.sample_expected, st.sample_actual
        );
    }
    assert!(report.docs > 100, "text cases: {}", report.docs);
    assert_eq!(report.unknown, 0, "{} unknown diffs against TS ParsedDoc", report.unknown);
}

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

/// COMPAT-04：多段 sdt 拆成多条 `elements` / `blocks`，`docxIndex == 下标`，区间首块从 sdt 开头、末块到 sdt 结尾；
/// COMPAT-06：含 emoji 的 `document.xml` 用 UTF-16 索引。
#[test]
fn compat_04_06_sdt_split_and_utf16_indices() {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>😀a</w:t></w:r></w:p><w:sdt><w:sdtPr><w:alias w:val="A"/><w:tag w:val="t"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>s1</w:t></w:r></w:p><w:bookmarkStart w:id="0" w:name="x"/><w:p><w:r><w:t>s2</w:t></w:r></w:p></w:sdtContent></w:sdt><w:bookmarkEnd w:id="0"/><w:sectPr/></w:body></w:document>"#
    );
    let mut pkg = Package::open(&build_docx(&xml)).unwrap();
    let doc = parsed_doc(&mut pkg).unwrap();
    let elements = doc["extras"]["elements"].as_array().unwrap();
    let blocks = doc["blocks"].as_array().unwrap();
    assert_eq!(elements.len(), 5);
    assert_eq!(blocks.len(), 5);
    let names: Vec<&str> = elements.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["w:p", "w:p", "w:p", "w:bookmarkEnd", "w:sectPr"]);
    for (i, b) in blocks.iter().enumerate() {
        assert_eq!(b["docxIndex"].as_u64().unwrap() as usize, i);
        assert_eq!(b["id"].as_str().unwrap(), format!("b{i}"));
    }
    // UTF-16：emoji 占 2 个单位，body 首子元素起点等于 elements[0].start
    let doc_xml = doc["internal"]["documentXml"].as_str().unwrap();
    let body_inner = doc["internal"]["bodyInnerStart"].as_u64().unwrap();
    let u16_of = |byte: usize| doc_xml[..byte].encode_utf16().count() as u64;
    assert_eq!(body_inner, u16_of(xml.find("<w:p>").unwrap()));
    assert_eq!(elements[0]["start"].as_u64().unwrap(), body_inner);
    let sdt_start = xml.find("<w:sdt>").unwrap();
    let sdt_end = xml.find("</w:sdt>").unwrap() + "</w:sdt>".len();
    assert_eq!(elements[1]["start"].as_u64().unwrap(), u16_of(sdt_start), "首块从 sdt 开头");
    assert_eq!(elements[2]["end"].as_u64().unwrap(), u16_of(sdt_end), "末块到 sdt 结尾");
    assert_eq!(elements[1]["end"], elements[2]["start"], "中间块到下一子块开头");
    // originalXml 拼起来正好等于整个 sdt
    let joined = format!(
        "{}{}",
        blocks[1]["originalXml"].as_str().unwrap(),
        blocks[2]["originalXml"].as_str().unwrap()
    );
    assert_eq!(joined, &xml[sdt_start..sdt_end]);
    assert_eq!(blocks[1]["sdtShell"]["group"], blocks[2]["sdtShell"]["group"]);
    assert_eq!(blocks[1]["sdtShell"]["alias"], "A");
    // 首块的 closeXml 是子元素结尾到下一子块开头的区间（TS：中间的书签标记归前一块）
    assert_eq!(blocks[1]["sdtShell"]["closeXml"], r#"<w:bookmarkStart w:id="0" w:name="x"/>"#);
    assert_eq!(blocks[2]["sdtShell"]["closeXml"], "</w:sdtContent></w:sdt>");
    assert_eq!(blocks[1]["label"], "A");
    assert_eq!(blocks[1]["type"], "paragraph");
    assert_eq!(blocks[1]["runs"][0]["text"], "s1");
    assert_eq!(blocks[3]["label"], "w:bookmarkEnd");
    assert_eq!(blocks[3]["invisibleMarker"], true);
    assert_eq!(blocks[4]["hidden"], true);
    assert_eq!(
        doc["internal"]["bodyInnerEnd"].as_u64().unwrap(),
        u16_of(xml.find("</w:body>").unwrap())
    );
}
