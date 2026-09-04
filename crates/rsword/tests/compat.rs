//! `compat_ts` 差分（任务 1.10，`COMPAT-02/03/04/06/07`，M1 门第一条）：对 `corpus/synthetic` 里的
//! "文本段落"用例（paragraph / heading / listItem，无字段 / 表格 / 绘图 / 批注 / 脚注），适配器输出
//! 与 TS `.expected.json` 的差异除 `KNOWN_DIFFS` 外为 0。

mod common;

use std::collections::BTreeMap;

use std::io::{Cursor, Write};

use rsword::bind::compat_ts::{diff_json, filter_known, parsed_doc};
use rsword::package::Package;
use serde_json::Value;

/// 已知差异的路径模式（`src/bind/compat_ts/KNOWN_DIFFS.md`）。
const KNOWN_PATHS: &[&str] = &[
    // 表格样式的显示模型（M2 随表格）
    "styles.*.tableDisplay*",
    // 字符单位缩进的换算需要字体度量（TS withCharIndents），显示层决定
    "blocks[*].format.charIndents*",
];

/// 已知差异的文档（前缀）：整份跳过。
const KNOWN_DOCS: &[&str] = &[
    "symbol-fonts__",         // 符号字体解码（M2 RES-05）
    "char-unit-indents__",    // *Chars 缩进换算需字体度量（TS withCharIndents）
    "extra__strict-minimal", // TS 装载时把 Strict 改写为 Transitional，internal.documentXml 与索引不同
    "balance-dbcs-spacing__", // TS 按双字节比例缩放显示用 charSpacingTwips（显示层）
    "numbering-defs__012",   // 未声明前缀的 mc:Choice Requires（KNOWN_DIFFS 第一条）
];

/// TS 块是否属于 M1 的"文本段落"范围。
fn is_text_case(e: &Value) -> bool {
    let Some(blocks) = e.get("blocks").and_then(Value::as_array) else { return false };
    let mut text_blocks = 0;
    for b in blocks {
        let ty = b.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "paragraph" | "heading" | "listItem" => {
                text_blocks += 1;
                let runs = b.get("runs").and_then(Value::as_array).cloned().unwrap_or_default();
                for r in &runs {
                    for k in [
                        "image",
                        "math",
                        "ruby",
                        "noteRef",
                        "xeTerm",
                        "refField",
                        "instrField",
                        "fldBeginXml",
                        "commentIds",
                    ] {
                        if r.get(k).is_some() {
                            return false;
                        }
                    }
                }
                for k in [
                    "textboxes",
                    "strayRuns",
                    "bookmarks",
                    "hiddenBookmarks",
                    "commentStarts",
                    "commentEnds",
                ] {
                    if b.get(k).is_some() {
                        return false;
                    }
                }
                // 字段（含被 TS 折叠成 link 的 HYPERLINK）与 w14 文字填充（M2）
                let xml = b.get("originalXml").and_then(Value::as_str).unwrap_or("");
                if xml.contains("<w:fldChar")
                    || xml.contains("<w:fldSimple")
                    || xml.contains("<w:instrText")
                    || xml.contains("w14:textFill")
                {
                    return false;
                }
            }
            "passthrough" => {
                let label = b.get("label").and_then(Value::as_str).unwrap_or("");
                let ok = label == "Section properties"
                    || label == "Section break paragraph"
                    || label == "Hidden paragraph"
                    || label == "Page break"
                    || b.get("invisibleMarker").and_then(Value::as_bool).unwrap_or(false);
                if !ok {
                    return false;
                }
            }
            _ => return false,
        }
    }
    text_blocks > 0
        && e.get("comments").and_then(Value::as_array).is_some_and(Vec::is_empty)
        && e.get("footnotes").and_then(Value::as_array).is_some_and(Vec::is_empty)
        && e.get("endnotes").and_then(Value::as_array).is_some_and(Vec::is_empty)
        && e.get("sources").and_then(Value::as_array).is_some_and(Vec::is_empty)
        && e.get("headerText").is_none_or(Value::is_null)
        && e.get("footerText").is_none_or(Value::is_null)
        && e.get("hfParts").and_then(Value::as_object).is_some_and(|m| m.is_empty())
}

#[test]
fn compat_02_text_cases_match_ts_parsed_doc() {
    let mut docs = 0;
    let mut text_docs = 0;
    let mut known = 0;
    let mut by_path: BTreeMap<String, usize> = BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();
    for path in common::docx_paths("synthetic") {
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).unwrap();
        docs += 1;
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        if KNOWN_DOCS.iter().any(|p| file.starts_with(p)) || !is_text_case(&expected) {
            continue;
        }
        text_docs += 1;
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let actual = parsed_doc(&mut pkg).unwrap();
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (unknown, k) = filter_known(diffs, KNOWN_PATHS);
        known += k;
        for d in unknown {
            // 路径去下标，便于按类别统计
            let key: String = d.path.chars().filter(|c| !c.is_ascii_digit()).collect();
            *by_path.entry(key).or_default() += 1;
            if samples.len() < 40 {
                let short = |v: &Option<Value>| {
                    v.as_ref().map(|x| x.to_string().chars().take(160).collect::<String>())
                };
                samples.push(format!(
                    "{file}: {} TS={:?} ours={:?}",
                    d.path,
                    short(&d.expected),
                    short(&d.actual)
                ));
            }
        }
    }
    eprintln!(
        "compat: {docs} docs with expected.json, {text_docs} text cases, {known} known diffs"
    );
    eprintln!("compat: unknown diffs by path: {by_path:?}");
    for s in &samples {
        eprintln!("compat: DIFF {s}");
    }
    assert!(text_docs > 100, "text cases: {text_docs}");
    let total: usize = by_path.values().sum();
    assert_eq!(total, 0, "{total} unknown diffs against TS ParsedDoc");
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
