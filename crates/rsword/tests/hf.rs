//! 页眉页脚 part 的内容流验收（`MOD-01` / `SPAN-01` / `FLD-11`，任务 5.3）。

mod common;

use std::collections::BTreeSet;

use rsword::model::{Block, Document, HfKind};
use rsword::package::Package;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const V: &str = "urn:schemas-microsoft-com:vml";
const R: &str = r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const HDR_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const FTR_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";

/// 按 `COMPAT-09` 的容忍规则比较（浮点 1e-6、`undefined` 与缺失等价）——与差分工具同一把尺子，
/// 这样 `50` 与 `50.0` 之类的 JSON 数字写法不会被判成差异。
fn same(expected: &serde_json::Value, actual: &serde_json::Value, what: &str) {
    let mut diffs = Vec::new();
    rsword::bind::compat_ts::diff_json(expected, actual, &mut diffs);
    assert!(diffs.is_empty(), "{what}: {diffs:#?}");
}

/// 索引里的书签名。
fn bookmark_names(spans: &[rsword::span::RangeSpan]) -> Vec<&str> {
    spans
        .iter()
        .filter_map(|s| match &s.kind {
            rsword::span::RangeKind::Bookmark { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

/// 主 part 的关系表：`(rId, Type, Target)`。
fn rels(list: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, ty, target) in list {
        s.push_str(&format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"/>"#));
    }
    s.push_str("</Relationships>");
    s
}

fn hdr(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}" xmlns:v="{V}">{inner}</w:hdr>"#
    )
}

fn ftr(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr xmlns:w="{W}">{inner}</w:ftr>"#
    )
}

/// 一份带页眉页脚 part 的最小文档。
fn doc_with_hf(body: &str, extra: &[(&str, &str)]) -> (Package, Document) {
    let bytes = common::docx_with_parts(body, extra);
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
    (pkg, doc)
}

/// 页眉复用正文管线：段落 / 表格 / sdt / 文本框都成块，形状与正文一致。
#[test]
fn mod_01_header_content_uses_the_body_pipeline() {
    let header = hdr(concat!(
        r#"<w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t>页眉</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:p><w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:sdt><w:sdtPr><w:alias w:val="a"/></w:sdtPr><w:sdtContent>"#,
        r#"<w:p><w:r><w:t>控件</w:t></w:r></w:p></w:sdtContent></w:sdt>"#,
    ));
    let (_pkg, doc) = doc_with_hf(
        "<w:p/>",
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &header),
        ],
    );
    assert_eq!(doc.hf_parts.len(), 1);
    let part = doc.hf_by_rel["rIdH"];
    let hf = &doc.hf_parts[&part];
    assert_eq!(hf.kind, HfKind::Header);
    assert_eq!(hf.part, part);

    // 三个块：段落、表格、sdt 里的段落
    assert_eq!(hf.blocks.len(), 3);
    let texts: Vec<String> = hf.text_blocks().map(|b| b.text()).collect();
    assert_eq!(texts, vec!["页眉".to_string(), "控件".to_string()]);
    // 表格是真的 TableBlock（M3 的模型，页眉里一视同仁）
    let tbl = hf
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("表格块");
    assert_eq!(tbl.rows.len(), 1);
    assert_eq!(tbl.rows[0].cells.len(), 1);
    // sdt 包裹的段落带 SdtInfo
    let sdt_block = hf.blocks.iter().find(|b| b.sdt().is_some()).expect("sdt 块");
    assert_eq!(sdt_block.sdt().unwrap().alias.as_deref(), Some("a"));
    // 内容流：`w:hdr` 是流根，框外段落都在同一个流里
    assert!(hf.idx.flows.flow_of(hf.root).is_some());
}

/// `FLD-11`：`has_page_number` / `has_num_pages` 由本 part 的字段索引推导；
/// 旧式 `w:pgNum`（Word 6.0/95 的页码 run 元素）也算。
#[test]
fn fld_11_page_number_comes_from_the_parts_own_field_index() {
    let field = |kw: &str| {
        format!(
            concat!(
                r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
                r#"<w:r><w:instrText xml:space="preserve"> {} </w:instrText></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
                r#"<w:r><w:t>1</w:t></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#
            ),
            kw
        )
    };
    let (_pkg, doc) = doc_with_hf(
        "<w:p/>",
        &[
            (
                "word/_rels/document.xml.rels",
                &rels(&[
                    ("rIdH", HDR_REL, "header1.xml"),
                    ("rIdF", FTR_REL, "footer1.xml"),
                    ("rIdF2", FTR_REL, "footer2.xml"),
                ]),
            ),
            ("word/header1.xml", &hdr(&format!("<w:p>{}</w:p>", field("PAGE")))),
            (
                "word/footer1.xml",
                &ftr(&format!("<w:p><w:r><w:t>共</w:t></w:r>{}</w:p>", field("NUMPAGES"))),
            ),
            // 旧式页码
            ("word/footer2.xml", &ftr(r#"<w:p><w:r><w:pgNum/></w:r></w:p>"#)),
        ],
    );
    let h = &doc.hf_parts[&doc.hf_by_rel["rIdH"]];
    assert!(h.has_page_number);
    assert!(!h.has_num_pages);
    assert_eq!(h.idx.fields.fields().len(), 1, "页眉的字段索引只看自己的 part");

    let f = &doc.hf_parts[&doc.hf_by_rel["rIdF"]];
    assert!(!f.has_page_number);
    assert!(f.has_num_pages);

    let f2 = &doc.hf_parts[&doc.hf_by_rel["rIdF2"]];
    assert!(f2.has_page_number, "w:pgNum 是旧式页码");
    assert!(f2.idx.fields.fields().is_empty(), "它不是字段");
}

/// 文字水印：页眉里的 VML `v:textpath/@string`（Word 的水印实现）。
#[test]
fn compat_05_watermark_text_from_the_header_part() {
    let shape = concat!(
        r##"<w:p><w:r><w:pict><v:shape id="PowerPlusWaterMarkObject1" type="#_x0000_t136">"##,
        r#"<v:textpath style="font-family:&quot;DengXian&quot;" string="草 &amp; 稿"/>"#,
        r#"</v:shape></w:pict></w:r></w:p>"#
    );
    let (_pkg, doc) = doc_with_hf(
        "<w:p/>",
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &hdr(shape)),
        ],
    );
    let h = &doc.hf_parts[&doc.hf_by_rel["rIdH"]];
    assert_eq!(h.watermark.as_deref(), Some("草 & 稿"), "实体解码过一次（XML-06）");

    // 没有 textpath 的页眉没有水印；空串按没有算
    let (_p2, d2) = doc_with_hf(
        "<w:p/>",
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            (
                "word/header1.xml",
                &hdr(
                    r#"<w:p><w:r><w:pict><v:shape><v:textpath string=""/></v:shape></w:pict></w:r></w:p>"#,
                ),
            ),
        ],
    );
    assert_eq!(d2.hf_parts[&d2.hf_by_rel["rIdH"]].watermark, None);
}

/// 页眉里的书签与批注标记由**本 part 的** `SpanIndex` 认领（`SPAN-01`：`w:hdr` 是独立内容流）。
#[test]
fn span_01_header_part_has_its_own_span_index() {
    let header = hdr(concat!(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="hdrMark"/>"#,
        r#"<w:r><w:t>标</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#
    ));
    let (_pkg, doc) = doc_with_hf(
        r#"<w:p><w:bookmarkStart w:id="9" w:name="bodyMark"/><w:r><w:t>正</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &header),
        ],
    );
    let h = &doc.hf_parts[&doc.hf_by_rel["rIdH"]];
    assert_eq!(
        bookmark_names(h.idx.spans.spans()),
        vec!["hdrMark"],
        "页眉的范围索引里只有页眉的书签"
    );
    assert_eq!(bookmark_names(doc.spans.spans()), vec!["bodyMark"]);
}

/// 关系里有但没被任何 `sectPr` 引用的孤儿 part 也进 `hf_parts`（TS `parseAllHfParts` 同样输出）。
#[test]
fn mod_01_orphan_header_parts_are_still_parsed() {
    let (_pkg, doc) = doc_with_hf(
        "<w:p/><w:sectPr/>",
        &[
            (
                "word/_rels/document.xml.rels",
                &rels(&[("rIdH", HDR_REL, "header1.xml"), ("rIdX", HDR_REL, "header9.xml")]),
            ),
            ("word/header1.xml", &hdr(r#"<w:p><w:r><w:t>用着的</w:t></w:r></w:p>"#)),
            ("word/header9.xml", &hdr(r#"<w:p><w:r><w:t>没人引用</w:t></w:r></w:p>"#)),
        ],
    );
    assert_eq!(doc.hf_parts.len(), 2);
    assert_eq!(doc.hf_by_rel.keys().cloned().collect::<Vec<_>>(), vec!["rIdH", "rIdX"]);
    // 节没有引用任何页眉
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].hf_ref(HfKind::Header, rsword::model::HfVariant::Default), None);
}

/// 根不是 `w:hdr` / `w:ftr` 的 part → 不建 `HfPart`，记一条诊断，其余照常
/// （part 本身畸形的情况在 `Package::dom` 就成了 `Opaque`，也走这里的 `continue`）。
#[test]
fn test_09_a_header_part_with_a_wrong_root_degrades_locally() {
    let (_pkg, doc) = doc_with_hf(
        r#"<w:p><w:r><w:t>正文照旧</w:t></w:r></w:p>"#,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            (
                "word/header1.xml",
                &format!(r#"<?xml version="1.0"?><w:notHdr xmlns:w="{W}"><w:p/></w:notHdr>"#),
            ),
        ],
    );
    assert!(doc.hf_parts.is_empty());
    assert_eq!(doc.hf_by_rel.len(), 1, "关系照记，只是没有内容");
    assert!(
        doc.warnings.iter().any(|d| d.message.contains("根不是 w:hdr")),
        "{:?}",
        doc.warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(doc.text_blocks().count(), 1, "正文不受影响");
}

/// 恶意语料 `xml-unbalanced-header`：header part 解析失败 → `Opaque`，没有 `HfPart`，
/// 正文照常（`TEST-09`）。
#[test]
fn test_09_unbalanced_header_part_is_opaque() {
    let path = common::corpus_dir("hostile").join("xml-unbalanced-header.docx");
    let bytes = std::fs::read(&path).expect("hostile 语料");
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
    assert!(doc.hf_parts.is_empty(), "不平衡的 header 建不出 HfPart");
    assert!(doc.text_blocks().count() > 0, "正文照常");
}

/// `COMPAT-05` 的 `text`：只取 `w:t`、不按 `xml:space` 去空白、`</w:tc>` 后补空格、
/// PAGE / NUMPAGES 换成私用区标记（连缓存结果一起丢）、其他字段只留缓存结果、旧式 `w:pgNum` 也算页码。
#[test]
fn compat_05_hf_text_uses_the_ts_plain_text_rules() {
    const PAGE_MARK: char = '\u{E001}';
    const TOTAL: char = '\u{E000}';
    let field = |kw: &str, result: &str| {
        format!(
            concat!(
                r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
                r#"<w:r><w:instrText xml:space="preserve"> {} </w:instrText></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
                r#"<w:r><w:t>{}</w:t></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#
            ),
            kw, result
        )
    };
    let header = hdr(&format!(
        concat!(
            // 不带 preserve 的首尾空格：坐标流会去掉，`text` 不去
            r#"<w:p><w:r><w:t>  第  </w:t></w:r>{page}<w:r><w:t> 页，共 </w:t></w:r>{total}</w:p>"#,
            // 其他字段：指令丢掉、缓存结果留着
            r#"<w:p>{date}</w:p>"#,
            // 表格：`</w:tc>` 之后还有文字就补一个空格
            r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1"/><w:gridCol w:w="1"/></w:tblGrid>"#,
            r#"<w:tr><w:tc><w:p><w:r><w:t>甲</w:t></w:r></w:p></w:tc>"#,
            r#"<w:tc><w:p><w:r><w:t>乙</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
            // 旧式页码
            r#"<w:p><w:r><w:pgNum/></w:r></w:p>"#,
            // 删除的文字不算（`w:delText` 不是 `w:t`）
            r#"<w:p><w:del w:id="1" w:author="x"><w:r><w:delText>删了</w:delText></w:r></w:del></w:p>"#
        ),
        page = field("PAGE", "7"),
        total = field("NUMPAGES", "9"),
        date = field("DATE", "2026-09-05")
    ));
    // 顶层 `headerText` 要有引用才有值（TS `readHeaderFooterPart` 从 `document.xml` 找引用）
    let body = format!(
        r#"<w:p/><w:sectPr><w:headerReference {R} w:type="default" r:id="rIdH"/></w:sectPr>"#
    );
    let (pkg, doc) = doc_with_hf(
        &body,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &header),
        ],
    );
    let json = rsword::bind::compat_ts::parsed_doc_of(
        &pkg,
        &doc,
        &rsword::bind::compat_ts::MediaSet::default(),
    );
    // 表格之后的 `pgNum`：TS 把标记也写成 `<w:t>`，所以 `</w:tc>` 的补空格同样作用在它前面
    let want = format!("  第  {PAGE_MARK} 页，共 {TOTAL}2026-09-05甲 乙 {PAGE_MARK}");
    assert_eq!(json["headerText"], serde_json::Value::String(want.clone()));
    assert_eq!(json["hfParts"]["rIdH"]["text"], serde_json::Value::String(want));
    assert_eq!(json["headerHasPageNumber"], serde_json::Value::Bool(true));
    assert_eq!(json["footerHasPageNumber"], serde_json::Value::Bool(false));
    assert_eq!(json["footerText"], serde_json::Value::Null);
}

/// default 变体的选法（TS `readHeaderFooterPart`）：全文第一个 `w:type="default"` →
/// 否则非 schema 的 `odd` → 否则没有 `w:type` 的。**不是**按节选。
#[test]
fn compat_05_default_variant_picks_the_first_reference_in_the_document() {
    let mk = |t: &str| format!(r#"<w:headerReference {} w:type="{t}" r:id="rId{t}"/>"#, R);
    // 第一节声明 odd，第二节声明 default：TS 的顶层 `headerText` 取 default 那个
    let body = format!(
        concat!(
            r#"<w:p><w:pPr><w:sectPr>{odd}</w:sectPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>"#,
            r#"<w:sectPr>{def}</w:sectPr>"#
        ),
        odd = mk("odd"),
        def = mk("default")
    );
    let (pkg, doc) = doc_with_hf(
        &body,
        &[
            (
                "word/_rels/document.xml.rels",
                &rels(&[("rIdodd", HDR_REL, "h1.xml"), ("rIddefault", HDR_REL, "h2.xml")]),
            ),
            ("word/h1.xml", &hdr(r#"<w:p><w:r><w:t>ODD</w:t></w:r></w:p>"#)),
            ("word/h2.xml", &hdr(r#"<w:p><w:r><w:t>DEF</w:t></w:r></w:p>"#)),
        ],
    );
    let json = rsword::bind::compat_ts::parsed_doc_of(
        &pkg,
        &doc,
        &rsword::bind::compat_ts::MediaSet::default(),
    );
    assert_eq!(json["headerText"], "DEF");
    // 只有 odd 时它就是缺省页
    let (pkg2, doc2) = doc_with_hf(
        &format!("<w:p/><w:sectPr>{}</w:sectPr>", mk("odd")),
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdodd", HDR_REL, "h1.xml")])),
            ("word/h1.xml", &hdr(r#"<w:p><w:r><w:t>ODD</w:t></w:r></w:p>"#)),
        ],
    );
    let json2 = rsword::bind::compat_ts::parsed_doc_of(
        &pkg2,
        &doc2,
        &rsword::bind::compat_ts::MediaSet::default(),
    );
    assert_eq!(json2["headerText"], "ODD");
}

/// `COMPAT-05` 的 `paras`：样式层的对齐与制表位、`w:ptab` 对齐、`w:framePr` 的 `xAlign`、
/// 表格一行一段（`cells`）、水印段落不出、框里的段落被提出来。
#[test]
fn compat_05_hf_paras_shape() {
    const PAGE_MARK: char = '\u{E001}';
    let header = hdr(concat!(
        // 普通段落：直接 jc 胜过样式层
        r#"<w:p><w:pPr><w:pStyle w:val="Header"/><w:jc w:val="right"/></w:pPr>"#,
        r#"<w:r><w:t>右</w:t></w:r></w:p>"#,
        // ptab：普通 tab 占一个空位
        r#"<w:p><w:r><w:tab/><w:ptab w:alignment="center" w:relativeTo="margin" w:leader="none"/>"#,
        r#"<w:t>中</w:t></w:r></w:p>"#,
        // framePr：右侧浮动的页码框
        r#"<w:p><w:pPr><w:framePr w:xAlign="outside" w:vAnchor="text" w:hAnchor="margin"/></w:pPr>"#,
        r#"<w:r><w:t>框</w:t></w:r></w:p>"#,
        // 水印段落：只有 VML 形状 → TS 与我们都不出这一段
        r#"<w:p><w:r><w:pict><v:shape><v:textpath string="草稿"/></v:shape></w:pict></w:r></w:p>"#,
        // 表格：一行一段，cells 带 widthPct / fill / align
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="1000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/><w:shd w:val="clear" w:fill="1F3864"/></w:tcPr>"#,
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p></w:tc>"#,
        r#"<w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#,
    ));
    let body = format!(
        r#"<w:p/><w:sectPr><w:headerReference {R} w:type="default" r:id="rIdH"/></w:sectPr>"#
    );
    let (pkg, doc) = doc_with_hf(
        &body,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &header),
        ],
    );
    let json = rsword::bind::compat_ts::parsed_doc_of(
        &pkg,
        &doc,
        &rsword::bind::compat_ts::MediaSet::default(),
    );
    let paras = json["headerParas"].as_array().expect("headerParas").clone();
    assert_eq!(paras.len(), 4, "水印段落不出：三段 + 一行表格\n{paras:#?}");

    assert_eq!(paras[0]["align"], "right");
    assert_eq!(paras[0]["runs"][0]["text"], "右");
    // ptabAligns 按整体制表位顺序：`w:tab` 是 null，`w:ptab` 是它的对齐
    assert_eq!(paras[1]["ptabAligns"], serde_json::json!([null, "center"]));
    assert_eq!(paras[2]["frameXAlign"], "right", "outside → right");
    // 表格行
    let cells = paras[3]["cells"].as_array().expect("cells");
    assert_eq!(paras[3]["runs"].as_array().map(Vec::len), Some(0), "行本身没有 runs");
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0]["fill"], "1F3864");
    assert_eq!(cells[0]["align"], "center");
    assert_eq!(cells[0]["paras"][0][0]["text"], "标题");
    assert_eq!(cells[0]["widthPct"], 75.0);
    assert_eq!(cells[1]["widthPct"], 25.0);
    // 水印仍然出现在 watermarkText 里
    assert_eq!(json["watermarkText"], "草稿");
    let _ = PAGE_MARK;
}

/// 框里的段落被提出来（政府公文的页码常放在 VML 文本框里），浮动框的段落带 `boxAnchored`。
#[test]
fn compat_05_textbox_paragraphs_are_surfaced() {
    let header = hdr(concat!(
        r#"<w:p><w:r><w:pict><v:shape style="position:absolute;width:100pt;height:20pt">"#,
        r#"<v:textbox><w:txbxContent><w:p><w:pPr><w:jc w:val="center"/></w:pPr>"#,
        r#"<w:r><w:t>— 1 —</w:t></w:r></w:p></w:txbxContent></v:textbox>"#,
        r#"</v:shape></w:pict></w:r></w:p>"#,
    ));
    let body = format!(
        r#"<w:p/><w:sectPr><w:headerReference {R} w:type="default" r:id="rIdH"/></w:sectPr>"#
    );
    let (pkg, doc) = doc_with_hf(
        &body,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &header),
        ],
    );
    let json = rsword::bind::compat_ts::parsed_doc_of(
        &pkg,
        &doc,
        &rsword::bind::compat_ts::MediaSet::default(),
    );
    let paras = json["headerParas"].as_array().expect("headerParas");
    assert_eq!(paras.len(), 1, "{paras:#?}");
    assert_eq!(paras[0]["runs"][0]["text"], "— 1 —");
    assert_eq!(paras[0]["align"], "center");
    assert_eq!(paras[0]["boxAnchored"], true, "position:absolute → 画在锚点上，不占行高");
}

/// 全语料：页眉页脚 part 的 `rId` 集合与 `hasPageNumber` 与 TS 逐份一致。
#[test]
fn compat_05_hf_parts_match_ts_on_corpus() {
    let mut docs = 0usize;
    let mut parts = 0usize;
    let mut blocks = 0usize;
    for path in common::docx_paths("synthetic") {
        let expected = path.with_extension("").to_string_lossy().to_string() + ".expected.json";
        let Ok(txt) = std::fs::read_to_string(&expected) else { continue };
        let j: serde_json::Value = serde_json::from_str(&txt).expect("expected.json");
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
        let ts = j["hfParts"].as_object().cloned().unwrap_or_default();
        if ts.is_empty() && doc.hf_by_rel.is_empty() {
            continue;
        }
        docs += 1;
        parts += doc.hf_parts.len();
        blocks += doc.hf_parts.values().map(|h| h.blocks.len()).sum::<usize>();
        let ours: BTreeSet<&str> = doc.hf_by_rel.keys().map(String::as_str).collect();
        let theirs: BTreeSet<&str> = ts.keys().map(String::as_str).collect();
        assert_eq!(ours, theirs, "{}: hfParts 的 rId 集合不一致", path.display());
        let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).expect("parsed_doc");
        for (rid, v) in &ts {
            let hf = &doc.hf_parts[&doc.hf_by_rel[rid]];
            assert_eq!(
                hf.has_page_number,
                v["hasPageNumber"].as_bool().unwrap_or(false),
                "{}: {rid} 的 hasPageNumber",
                path.display()
            );
            assert_eq!(
                json["hfParts"][rid]["text"],
                v["text"],
                "{}: {rid} 的 text",
                path.display()
            );
            if v["paras"].as_array().is_some_and(|a| !a.is_empty()) {
                assert!(!hf.blocks.is_empty(), "{}: {rid} TS 有段落我们没块", path.display());
            }
            same(
                &v["paras"],
                &json["hfParts"][rid]["paras"],
                &format!("{}: {rid} 的 paras", path.display()),
            );
        }
        for k in [
            "headerText",
            "footerText",
            "headerHasPageNumber",
            "footerHasPageNumber",
            "headerParas",
            "footerParas",
            "watermarkText",
            "headerFirst",
            "headerEven",
            "footerFirst",
            "footerEven",
        ] {
            same(&j[k], &json[k], &format!("{}: {k}", path.display()));
        }
    }
    eprintln!("hf: {docs} 份文档、{parts} 个 part、{blocks} 个块");
    assert!(docs >= 43, "语料缺失？{docs}");
}
