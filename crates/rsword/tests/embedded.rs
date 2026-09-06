//! 嵌入对象（OLE）的 run 投影与编辑（`COMPAT-07` / `MOD-06` / `EDIT-02`，`spec/17` 任务 6.4）。
//!
//! `w:object` 的预览图（`v:imagedata`）在文本段落里是一个 `text: ""` 的图片 run，与同 run 的文字拆开
//! （TS `splitImageRun`）；`{ EMBED }` / `{ LINK }` 字段包着的对象走嵌入对象块；单元格里的对象跟着 run 走，
//! 不进格的锚定框。坐标流里 `w:object` 是 1 个 UTF-16 单位的原子：插字绕过它、删除区间盖住它就整 run 消失，
//! 子树字节原样。

mod common;

use rsword::bind::compat_ts::{
    EmbeddedKind, block_of_path, diff_json, embedded_kind, known_diffs, parsed_doc, split_known,
};
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::package::Package;
use serde_json::{Value, json};

const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const V: &str = "urn:schemas-microsoft-com:vml";
const O: &str = "urn:schemas-microsoft-com:office:office";

/// 一个 `w:object`：32 pt 见方的预览图（→ 43 px）。`rid` 悬空时预览解析不出来。
fn object(rid: &str) -> String {
    format!(
        r#"<w:object w:dxaOrig="640" w:dyaOrig="640"><v:shape xmlns:v="{V}" id="_x0000_i1025" style="width:32pt;height:32pt"><v:imagedata xmlns:r="{R}" r:id="{rid}" o:title=""/></v:shape><o:OLEObject xmlns:o="{O}" xmlns:r="{R}" Type="Embed" ProgID="Excel.Sheet.12" ShapeID="_x0000_i1025" DrawAspect="Content" ObjectID="_1" r:id="rIdOle"/></w:object>"#
    )
}

fn docx(body: &str) -> Vec<u8> {
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="{R}/image" Target="media/image1.png"/><Relationship Id="rIdOle" Type="{R}/oleObject" Target="embeddings/oleObject1.bin"/></Relationships>"#
    );
    let d = common::docx_with_parts(body, &[("word/_rels/document.xml.rels", rels.as_str())]);
    let d = common::with_binary_part(&d, "word/media/image1.png", &common::b64(common::PNG_1X1));
    common::with_binary_part(&d, "word/embeddings/oleObject1.bin", b"\xD0\xCF\x11\xE0ole")
}

fn parsed(bytes: &[u8]) -> Value {
    let mut pkg = Package::open(bytes).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

fn is_png(v: &Value) -> bool {
    v.as_str().is_some_and(|u| u.starts_with("data:image/png;base64,"))
}

/// `COMPAT-07`：语料里每个嵌入对象文档在嵌入对象块上的差异为 0。
#[test]
fn compat_07_ole_projection_matches_ts_across_the_corpus() {
    let known = known_diffs();
    let (mut docs, mut unknown) = (0, Vec::new());
    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).expect("expected.json");
        let blocks = expected.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
        if !blocks.iter().any(|b| embedded_kind(b) == Some(EmbeddedKind::Ole)) {
            continue;
        }
        docs += 1;
        let bytes = std::fs::read(&path).unwrap();
        let actual = parsed(&bytes);
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (diffs, _) = split_known(diffs, &file, &known);
        for d in diffs.into_iter().filter(|d| {
            block_of_path(&d.path, &expected)
                .is_some_and(|b| embedded_kind(b) == Some(EmbeddedKind::Ole))
        }) {
            unknown.push(format!("{file}: {} TS={:?} ours={:?}", d.path, d.expected, d.actual));
        }
    }
    eprintln!("ole: {docs} 份文档");
    for u in unknown.iter().take(20) {
        eprintln!("ole: DIFF {u}");
    }
    assert!(docs >= 10, "{docs}");
    assert!(unknown.is_empty(), "{} 处差异", unknown.len());
}

/// 同一 run 里 `w:object` + 文字 + 空 `w:pict`：拆成图片 run（`text: ""`，尺寸来自 `v:shape` 的 style）
/// 与文字 run；空 pict 不算第二张图。只有一个图形的 run 不拆。
#[test]
fn compat_07_object_in_a_text_run_splits_like_ts() {
    let body = format!(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr>{}<w:t>bit map object</w:t><w:pict xmlns:v="{V}"></w:pict></w:r></w:p><w:p><w:r><w:t xml:space="preserve">前 </w:t></w:r><w:r>{}</w:r></w:p>"#,
        object("rIdImg"),
        object("rIdImg")
    );
    let v = parsed(&docx(&body));
    let runs = v["blocks"][0]["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2, "{runs:#?}");
    assert_eq!(runs[0]["text"], "");
    assert_eq!(runs[0]["bold"], true, "拆出来的每段都带原 run 的格式");
    assert!(is_png(&runs[0]["image"]["dataUrl"]));
    assert_eq!(
        (runs[0]["image"]["widthPx"].as_i64(), runs[0]["image"]["heightPx"].as_i64()),
        (Some(43), Some(43))
    );
    assert!(
        runs[0]["image"]["xml"]
            .as_str()
            .is_some_and(|x| x.starts_with("<w:object") && x.ends_with("</w:object>"))
    );
    assert_eq!(
        runs[1],
        json!({ "text": "bit map object", "bold": true, "rawRPr": "<w:rPr><w:b/></w:rPr>" })
    );
    // 单独一个 run 的对象：图片 run 紧跟文字 run
    let runs = v["blocks"][1]["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2, "{runs:#?}");
    assert_eq!(runs[0]["text"], "前 ");
    assert_eq!(runs[1]["text"], "");
    assert!(is_png(&runs[1]["image"]["dataUrl"]));
}

/// 只有对象的段落是嵌入对象块；预览解析不出来（悬空 `r:id`）时仍是芯片，`previewText` 带段落文字，
/// 尺寸照样给。`w:dxaOrig` 在 `v:shape` 没写 style 时顶上（缇 ÷ 15）。
#[test]
fn compat_07_object_only_paragraphs_are_embedded_object_chips() {
    let body = format!(
        r#"<w:p><w:r>{}</w:r></w:p><w:p><w:r><w:t xml:space="preserve">对象失效: </w:t></w:r><w:r>{}</w:r></w:p>"#,
        object("rIdImg"),
        object("rIdGone")
    );
    let v = parsed(&docx(&body));
    let b = &v["blocks"][0];
    assert_eq!(b["type"], "passthrough");
    assert_eq!(b["label"], "Embedded object");
    assert_eq!(b["oleProgId"], "Excel.Sheet.12");
    assert!(is_png(&b["imageDataUrl"]));
    assert_eq!((b["imageWidthPx"].as_i64(), b["imageHeightPx"].as_i64()), (Some(43), Some(43)));
    assert_eq!(b["previewText"], "");
    let b = &v["blocks"][1];
    assert_eq!(b["label"], "Embedded object", "{b}");
    assert!(b.get("imageDataUrl").is_none());
    assert_eq!(b["previewText"], "对象失效: ");
    assert_eq!(b["imageWidthPx"], 43);
}

/// `{ EMBED }` 包着的对象、后面还有文字：TS 走嵌入对象那条路（不是 `Field (EMBED)`）；段落里另有别的字段
/// 时仍归字段管。
#[test]
fn compat_07_field_form_ole_is_an_embedded_object_unless_other_fields_join() {
    let field = |instr: &str, tail: &str| {
        format!(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r>{}</w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>{tail}</w:p>"#,
            object("rIdImg")
        )
    };
    let body = format!(
        "{}{}",
        field(
            "EMBED Excel.Sheet.12",
            r#"<w:r><w:t xml:space="preserve"> 字段后的文字</w:t></w:r>"#
        ),
        field(
            "LINK Excel.Sheet.12 book.xlsx",
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
        )
    );
    let v = parsed(&docx(&body));
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "Embedded object", "{b}");
    assert_eq!(b["previewText"], " 字段后的文字");
    assert_eq!(b["oleProgId"], "Excel.Sheet.12");
    assert!(is_png(&b["imageDataUrl"]));
    assert!(b.get("fieldDisplay").is_none(), "{b}");
    let b = &v["blocks"][1];
    assert_eq!(b["label"], "Field (LINK)", "另有 PAGE 字段 → 字段芯片：{b}");
    assert!(b.get("oleProgId").is_none());
}

/// 单元格里的对象：图片 run 在 `richParas` 里，不进 `anchoredBoxes`（TS 的闸门只看 `wp:anchor` 与 `w:pict`）。
#[test]
fn compat_07_object_in_a_cell_rides_the_run() {
    let body = format!(
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>格内 </w:t></w:r><w:r>{}</w:r></w:p></w:tc></w:tr></w:tbl>"#,
        object("rIdImg")
    );
    let v = parsed(&docx(&body));
    let cell = &v["blocks"][0]["table"]["rows"][0][0];
    assert!(cell.get("anchoredBoxes").is_none(), "{cell}");
    let runs = cell["richParas"][0]["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2, "{runs:#?}");
    // run 文字按 Word 的规则去掉没有 xml:space 的首尾空白，`paras` 是 TS `plainText` 的原文
    assert_eq!(runs[0]["text"], "格内");
    assert!(is_png(&runs[1]["image"]["dataUrl"]));
    assert_eq!(cell["paras"], json!(["格内 "]));
}

/// `EDIT-02`：`w:object` 在坐标流里是 1 个原子。在它前后插字只脏 `w:t`，对象子树与 `o:OLEObject` 的关系原字节
/// 原样；删除区间盖住原子 → 整个 `w:r` 消失，内嵌二进制 part 还在包里（成为编辑引起的孤儿，6.7 回收）。
#[test]
fn edit_02_text_edits_step_around_the_ole_atom_and_deletion_removes_the_run() {
    let body = format!(
        r#"<w:p><w:r><w:t>ab</w:t></w:r><w:r>{}</w:r><w:r><w:t>cd</w:t></w:r></w:p>"#,
        object("rIdImg")
    );
    let bytes = docx(&body);
    let mut s = EditSession::open(&bytes).expect("open");
    let p = s.nth_text_block(0).expect("paragraph").node;
    assert_eq!(s.nth_text_block(0).unwrap().text(), "ab\u{FFFC}cd");
    let ctx = EditContext::default();
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: None }, &ctx)
        .expect("insert before the atom");
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 4), text: "Y".into(), props: None }, &ctx)
        .expect("insert after the atom");
    assert_eq!(s.nth_text_block(0).unwrap().text(), "abX\u{FFFC}Ycd");
    let saved = s.save().expect("save");
    let mut pkg = Package::open(&saved).expect("reopen");
    let main = pkg.main_part();
    let xml = pkg.dom(main).unwrap().unwrap().src().to_string();
    let obj = object("rIdImg");
    assert!(xml.contains(&obj), "w:object 子树原字节原样：{xml}");
    assert!(xml.contains("abX") && xml.contains("Ycd"), "{xml}");
    let rels = pkg.find_name("word/_rels/document.xml.rels").expect("rels");
    let rels_xml = pkg.dom(rels).unwrap().unwrap().src().to_string();
    assert!(rels_xml.contains(r#"Id="rIdOle""#), "OLE 关系不动");
    assert!(pkg.find_name("word/embeddings/oleObject1.bin").is_some());

    // 删除盖住原子的区间
    let mut s = EditSession::open(&saved).expect("open");
    let p = s.nth_text_block(0).unwrap().node;
    s.apply(EditOp::DeleteRange { from: InlinePos::new(p, 2), to: InlinePos::new(p, 5) }, &ctx)
        .expect("delete across the atom");
    assert_eq!(s.nth_text_block(0).unwrap().text(), "abcd");
    let saved = s.save().expect("save");
    let mut pkg = Package::open(&saved).expect("reopen");
    let main = pkg.main_part();
    let xml = pkg.dom(main).unwrap().unwrap().src().to_string();
    assert!(!xml.contains("<w:object"), "整个 w:r 连对象一起消失：{xml}");
    assert!(
        pkg.find_name("word/embeddings/oleObject1.bin").is_some(),
        "二进制 part 还在（孤儿，6.7 回收）"
    );
    // 剩下的 `ab` / `cd` 两个 run 格式相同，投影按 TS 的规则合并成一个
    let v = parsed(&saved);
    let runs = v["blocks"][0]["runs"].as_array().expect("runs");
    let text: String = runs.iter().map(|r| r["text"].as_str().unwrap_or_default()).collect();
    assert_eq!(text, "abcd", "{}", v["blocks"][0]);
    assert!(runs.iter().all(|r| r.get("image").is_none()));
}
