//! 墨迹（`aidocs-ink`，`MOD-06` / `MOD-11` / `SAVE-07`，`spec/17` 任务 6.8）。
//!
//! 读侧：墨迹 run 对分类与坐标流不可见（被批注的段落仍是文本块、`InsertText` 偏移与 TS `runs` 一致）、
//! `Document.inks` 与 compat `inks[]` 的几何 / 载荷 / `dataUrl`；写侧：`inks` 权威列表——追加在段落全部内容之后、
//! 自闭合空段落、`inks: []` 只删并回收媒体与关系、重复保存不累积、换锚点不重复 run、锚点不是段落时跳过且不留
//! 孤儿；`hostile/ink-garbage`：悬空 `r:embed`、实体载荷、非数字 `posOffset`。

mod common;

use std::io::Read;

#[cfg(feature = "compat-ts")]
use rsword::bind::compat_ts::parsed_doc;
use rsword::diag::DiagCode;
#[cfg(feature = "compat-ts")]
use rsword::edit::{EditContext, EditOp, InlinePos};
use rsword::edit::{EditSession, InkSave, NewInk};
#[cfg(feature = "compat-ts")]
use rsword::model::Block;
#[cfg(feature = "compat-ts")]
use rsword::package::Package;
use rsword::save::options::CompatSaveOptions as SaveOptions;
#[cfg(feature = "compat-ts")]
use serde_json::Value;

#[cfg(feature = "compat-ts")]
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
#[cfg(feature = "compat-ts")]
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
#[cfg(feature = "compat-ts")]
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
#[cfg(feature = "compat-ts")]
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// TS `anchoredInkRunXml` 的产物（`rIdInk` → `word/media/aidocsink1.png`）。
#[cfg(feature = "compat-ts")]
fn ink_run(x: i64, y: i64, descr: &str) -> String {
    format!(
        concat!(
            r#"<w:r><w:drawing><wp:anchor xmlns:wp="{WP}" distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="251667242" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
            r#"<wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>{x}</wp:posOffset></wp:positionH>"#,
            r#"<wp:positionV relativeFrom="paragraph"><wp:posOffset>{y}</wp:posOffset></wp:positionV><wp:extent cx="1905000" cy="762000"/>"#,
            r#"<wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="9002" name="aidocs-ink 9002"{descr}/><wp:cNvGraphicFramePr/>"#,
            r#"<a:graphic xmlns:a="{A}"><a:graphicData uri="{PIC}"><pic:pic xmlns:pic="{PIC}"><pic:nvPicPr><pic:cNvPr id="9002" name="aidocs-ink 9002"/><pic:cNvPicPr/></pic:nvPicPr>"#,
            r#"<pic:blipFill><a:blip xmlns:r="{R}" r:embed="rIdInk"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
            r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="762000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
            r#"</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
        ),
        WP = WP,
        A = A,
        PIC = PIC,
        R = R,
        x = x,
        y = y,
        descr = descr,
    )
}

/// 带一条 TS 写出的墨迹的文档：`body` 里 `{INK}` 处放墨迹 run。
#[cfg(feature = "compat-ts")]
fn docx_with_ink(body: &str) -> Vec<u8> {
    let run = ink_run(
        381_000,
        -95_250,
        r#" descr="{&quot;strokes&quot;:[{&quot;tool&quot;:&quot;pen&quot;}]}""#,
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdInk" Type="{R}/image" Target="media/aidocsink1.png"/></Relationships>"#
    );
    let d = common::docx_with_parts(
        &body.replace("{INK}", &run),
        &[("word/_rels/document.xml.rels", rels.as_str())],
    );
    common::with_binary_part(&d, "word/media/aidocsink1.png", &png())
}

fn png() -> Vec<u8> {
    common::b64(common::PNG_1X1)
}

fn ink(x: f64, y: f64, payload: Option<&str>) -> NewInk {
    NewInk {
        png: png(),
        width_px: 200.0,
        height_px: 80.0,
        offset_x_px: x,
        offset_y_px: y,
        payload: payload.map(str::to_string),
    }
}

fn open(bytes: &[u8]) -> EditSession {
    EditSession::open(bytes).expect("open")
}

fn names(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
}

fn text(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    let mut f = z.by_name(name).unwrap_or_else(|_| panic!("zip 里没有 {name}"));
    let mut out = String::new();
    f.read_to_string(&mut out).unwrap();
    out
}

fn media(bytes: &[u8]) -> Vec<String> {
    names(bytes).into_iter().filter(|n| n.starts_with("word/media/") && !n.ends_with('/')).collect()
}

#[cfg(feature = "compat-ts")]
fn parsed(bytes: &[u8]) -> Value {
    let mut pkg = Package::open(bytes).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

fn para(s: &EditSession, i: usize) -> rsword::xml::NodeId {
    s.document().main[i].node()
}

fn save_inks(s: &mut EditSession, inks: Vec<InkSave>) -> Vec<u8> {
    s.save_with_compat(&SaveOptions { inks: Some(inks), ..SaveOptions::default() }).expect("save")
}

/// 墨迹 run 对分类与坐标流不可见：被批注的段落是文本块、坐标流没有 U+FFFC、`InsertText` 的偏移按 TS `runs` 算；
/// 只含墨迹的段落是空段落不是图片块；`Document.inks` 记下几何与载荷。
#[test]
#[cfg(feature = "compat-ts")]
fn mod_06_ink_runs_are_invisible_to_classification_and_the_coordinate_stream() {
    let src = docx_with_ink(
        r#"<w:p><w:r><w:t>批注</w:t></w:r>{INK}<w:r><w:t>段</w:t></w:r></w:p><w:p>{INK}</w:p><w:p><w:r><w:t>尾段</w:t></w:r></w:p>"#,
    );
    let mut s = open(&src);
    let doc = s.document();
    assert!(matches!(&doc.main[0], Block::Text(tb) if tb.text() == "批注段"), "{:?}", doc.main[0]);
    assert!(
        matches!(&doc.main[1], Block::Text(tb) if tb.text().is_empty()),
        "只含墨迹的段落是空段落"
    );
    assert_eq!(doc.inks.len(), 2);
    let ink0 = &doc.inks[0];
    assert_eq!(ink0.para, doc.main[0].node());
    assert_eq!(ink0.offset_emu, (381_000, -95_250));
    assert_eq!(ink0.extent_emu, (1_905_000, 762_000));
    assert_eq!(ink0.rel_id.as_deref(), Some("rIdInk"));
    assert_eq!(ink0.payload.as_deref(), Some(r#"{"strokes":[{"tool":"pen"}]}"#));
    // 坐标流：`批注段` 三个字，墨迹长度 0
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 4), text: "Y".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(s.nth_text_block(0).unwrap().text(), "批注X段Y");
    assert_eq!(s.document().inks.len(), 2, "编辑后墨迹表（refresh）不变");
    let saved = s.save().unwrap();
    let doc_xml = text(&saved, "word/document.xml");
    assert_eq!(
        doc_xml.matches("aidocs-ink").count(),
        4,
        "两条墨迹 run 原样（docPr + cNvPr 各一处）：{doc_xml}"
    );
    let v = parsed(&saved);
    let runs: Vec<&str> = v["blocks"][0]["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["text"].as_str())
        .collect();
    assert_eq!(runs.concat(), "批注X段Y");
    assert!(v["blocks"][0]["runs"].as_array().unwrap().iter().all(|r| r.get("image").is_none()));
    assert_eq!(v["blocks"][1]["type"], "paragraph");
    assert_eq!(v["blocks"][1]["runs"].as_array().unwrap().len(), 0);
}

/// compat `inks[]`：`anchorIndex` 是块的 `docxIndex`（单元格里的算到表格块），px = EMU / 9525（整除给整数），
/// `dataUrl` 经媒体表，`payload` 实体解码。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_02_inks_json_matches_ts_shape() {
    let src = docx_with_ink(
        r#"<w:p><w:r><w:t>首段</w:t></w:r></w:p><w:p><w:r><w:t>批注段</w:t></w:r>{INK}</w:p><w:tbl><w:tr><w:tc><w:p>{INK}<w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let v = parsed(&src);
    let inks = v["inks"].as_array().expect("inks");
    assert_eq!(inks.len(), 2, "{inks:?}");
    assert_eq!(inks[0]["anchorIndex"], 1);
    assert_eq!(inks[0]["offsetXPx"], 40);
    assert_eq!(inks[0]["offsetYPx"], -10);
    assert_eq!(inks[0]["widthPx"], 200);
    assert_eq!(inks[0]["heightPx"], 80);
    assert!(inks[0]["dataUrl"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert_eq!(inks[0]["payload"], r#"{"strokes":[{"tool":"pen"}]}"#);
    assert_eq!(inks[1]["anchorIndex"], 2, "单元格里的墨迹算到表格块");
    assert_eq!(v["blocks"][1]["type"], "paragraph");
    assert_eq!(v["blocks"][1]["runs"][0]["text"], "批注段");
}

/// `SaveOptions.inks` 权威列表：追加在段落全部内容之后（模板与 TS 一致）、媒体 + 关系 + `Default` 内容类型；
/// 重开后 `inks[]` 与输入一致；`inks: Some([])` 删 run 并回收媒体与关系；重复保存不累积；换锚点旧 run 消失。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_inks_are_an_authoritative_list() {
    let src = common::docx_with_body(
        r#"<w:p><w:r><w:t>第一段</w:t></w:r></w:p><w:p><w:r><w:t>第二段</w:t></w:r></w:p>"#,
    );
    let mut s = open(&src);
    let p1 = para(&s, 1);
    let saved = save_inks(
        &mut s,
        vec![InkSave {
            para: p1,
            ink: ink(40.0, -10.0, Some(r#"{"strokes":[{"tool":"pen","color":"C00000"}]}"#)),
        }],
    );
    assert_eq!(media(&saved), vec!["word/media/image1.png".to_string()]);
    let doc_xml = text(&saved, "word/document.xml");
    // 命名空间声明由序列化器提到新 run 根上，所以按片段断言（TS 模板的每一段都在、run 在段落内容之后）
    assert!(
        doc_xml.contains("<w:t>第二段</w:t></w:r><w:r "),
        "墨迹 run 追加在全部内容之后：{doc_xml}"
    );
    for piece in [
        r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="251658241" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
        r#"<wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>381000</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>-95250</wp:posOffset></wp:positionV>"#,
        r#"<wp:extent cx="1905000" cy="762000"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/>"#,
        r#"<wp:docPr id="1" name="aidocs-ink 1" descr="{&quot;strokes&quot;:[{&quot;tool&quot;:&quot;pen&quot;,&quot;color&quot;:&quot;C00000&quot;}]}"/><wp:cNvGraphicFramePr/>"#,
        r#"<pic:nvPicPr><pic:cNvPr id="1" name="aidocs-ink 1"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId"#,
        r#"<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="762000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"#,
    ] {
        assert!(doc_xml.contains(piece), "缺 {piece}\n{doc_xml}");
    }
    assert!(text(&saved, "[Content_Types].xml").contains(r#"Extension="png""#));
    let rels = text(&saved, "word/_rels/document.xml.rels");
    assert!(rels.contains("media/image1.png"), "{rels}");
    let v = parsed(&saved);
    assert_eq!(v["inks"].as_array().unwrap().len(), 1);
    assert_eq!(v["inks"][0]["anchorIndex"], 1);
    assert_eq!(v["inks"][0]["offsetXPx"], 40);
    assert_eq!(v["inks"][0]["offsetYPx"], -10);
    assert_eq!(v["inks"][0]["payload"], r#"{"strokes":[{"tool":"pen","color":"C00000"}]}"#);
    assert_eq!(v["blocks"][1]["runs"][0]["text"], "第二段", "被批注的段落仍是可编辑正文");

    // 重复保存同一条：不累积媒体 / 关系 / run
    let mut s = open(&saved);
    let p1 = para(&s, 1);
    let again = save_inks(&mut s, vec![InkSave { para: p1, ink: ink(40.0, -10.0, None) }]);
    assert_eq!(media(&again).len(), 1, "{:?}", names(&again));
    assert_eq!(text(&again, "word/document.xml").matches("<wp:anchor").count(), 1);
    assert_eq!(text(&again, "word/_rels/document.xml.rels").matches("/image\"").count(), 1);

    // 换锚点：第 2 段 → 第 1 段，旧 run 删、新 run 只有一个
    let mut s = open(&again);
    let p0 = para(&s, 0);
    let moved = save_inks(&mut s, vec![InkSave { para: p0, ink: ink(5.0, 5.0, None) }]);
    let v = parsed(&moved);
    assert_eq!(v["inks"].as_array().unwrap().len(), 1);
    assert_eq!(v["inks"][0]["anchorIndex"], 0);
    assert_eq!(v["inks"][0]["offsetXPx"], 5);
    assert_eq!(text(&moved, "word/document.xml").matches("<wp:anchor").count(), 1);
    assert_eq!(media(&moved).len(), 1);

    // `inks: []`：只删，媒体与关系一起回收
    let mut s = open(&moved);
    let cleared = save_inks(&mut s, Vec::new());
    assert!(!text(&cleared, "word/document.xml").contains("aidocs-ink"));
    assert!(media(&cleared).is_empty(), "{:?}", names(&cleared));
    assert!(!text(&cleared, "word/_rels/document.xml.rels").contains("/image\""));
    assert_eq!(parsed(&cleared)["inks"].as_array().unwrap().len(), 0);

    // `inks: None`：不动，字节相同
    let mut s = open(&moved);
    assert_eq!(s.save().unwrap(), moved);
}

/// 自闭合 `<w:p/>` 也能做锚点；同一段两条墨迹各自一个媒体 part（不去重，TS 同）且 `docPr/@id` 递增。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_ink_into_empty_paragraph_and_two_inks_on_one_anchor() {
    let src = common::docx_with_body(r#"<w:p/><w:p><w:r><w:t>尾段</w:t></w:r></w:p>"#);
    let mut s = open(&src);
    let p0 = para(&s, 0);
    let saved = save_inks(
        &mut s,
        vec![
            InkSave { para: p0, ink: ink(40.0, -10.0, None) },
            InkSave { para: p0, ink: ink(90.0, 0.0, None) },
        ],
    );
    let doc_xml = text(&saved, "word/document.xml");
    assert_eq!(doc_xml.matches("<wp:anchor").count(), 2, "{doc_xml}");
    assert!(
        doc_xml.contains(r#"name="aidocs-ink 1""#) && doc_xml.contains(r#"name="aidocs-ink 2""#)
    );
    let rids: std::collections::BTreeSet<&str> = doc_xml
        .match_indices("r:embed=\"")
        .map(|(i, m)| doc_xml[i + m.len()..].split('"').next().unwrap())
        .collect();
    assert_eq!(rids.len(), 2, "两条墨迹各自一条关系（不去重，TS 同）：{doc_xml}");
    assert!(doc_xml.contains("<w:p><w:r "), "自闭合的 <w:p/> 展开后装下 run：{doc_xml}");
    assert_eq!(
        media(&saved),
        vec!["word/media/image1.png".to_string(), "word/media/image2.png".to_string()]
    );
    // 两个 png part 只补**一条** `Default Extension="png"`：重复的 Default 会让 Word 弹恢复提示
    // （真实 Word 核对，`docs/07` 任务 B / `corpus/real/ROUNDTRIP.md`）
    assert_eq!(text(&saved, "[Content_Types].xml").matches(r#"Extension="png""#).count(), 1);
    let v = parsed(&saved);
    assert_eq!(v["inks"].as_array().unwrap().len(), 2);
    assert_eq!(v["inks"][1]["offsetXPx"], 90);
    assert_eq!(v["blocks"][0]["type"], "paragraph");
    assert_eq!(v["blocks"][0]["runs"].as_array().unwrap().len(), 0);
}

/// 锚点不是段落（表格块）：跳过 + 诊断，不分配媒体也不分配关系，文档字节不变。
#[test]
fn save_07_non_paragraph_anchor_is_skipped_without_orphans() {
    let src = common::docx_with_body(
        r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>段</w:t></w:r></w:p>"#,
    );
    let mut s = open(&src);
    let tbl = para(&s, 0);
    let saved = save_inks(&mut s, vec![InkSave { para: tbl, ink: ink(1.0, 1.0, None) }]);
    assert!(
        s.diagnostics().iter().any(|d| d.code == DiagCode::EditBadPosition),
        "{:?}",
        s.diagnostics()
    );
    assert!(media(&saved).is_empty(), "{:?}", names(&saved));
    assert!(!text(&saved, "word/document.xml").contains("aidocs-ink"));
    assert!(
        !names(&saved).iter().any(|n| n == "word/_rels/document.xml.rels")
            || !text(&saved, "word/_rels/document.xml.rels").contains("/image\"")
    );
}

/// `hostile/ink-garbage.docx`：`r:embed` 悬空 → `dataUrl: null`；`descr` 的 `&quot;` / `&amp;` 解码；
/// `posOffset` 非数字 → 0；被批注的段落仍是正文。
#[test]
#[cfg(feature = "compat-ts")]
fn hostile_ink_garbage() {
    let bytes = std::fs::read(common::corpus_dir("hostile").join("ink-garbage.docx")).unwrap();
    let v = parsed(&bytes);
    let inks = v["inks"].as_array().unwrap();
    assert_eq!(inks.len(), 1, "{inks:?}");
    assert_eq!(inks[0]["anchorIndex"], 0);
    assert!(inks[0]["dataUrl"].is_null());
    assert_eq!(inks[0]["payload"], r#"{"strokes":[{"tool":"pen&ink"}]}"#);
    assert_eq!(inks[0]["offsetXPx"], 0);
    assert_eq!(inks[0]["offsetYPx"], 0);
    assert_eq!(inks[0]["widthPx"].as_f64().unwrap(), 1000.0 / 9525.0);
    assert_eq!(v["blocks"][0]["type"], "paragraph");
    assert_eq!(v["blocks"][0]["runs"][0]["text"], "批注段");
    // 无编辑保存字节相同
    let mut s = open(&bytes);
    assert_eq!(s.save().unwrap(), bytes);
}
