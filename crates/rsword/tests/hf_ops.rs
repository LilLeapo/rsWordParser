//! 节与页眉页脚编辑操作的验收（`EDIT-03` / `SAVE-05` / `SAVE-07`，任务 5.5）。

mod common;

use common::xpath_asserts;

use rsword::edit::{
    BlockAt, BlockPos, EditContext, EditOp, EditSession, InlinePos, NewBlock, NewInline, NewRun,
};
use rsword::model::{Document, HfKind, HfVariant, SectionOwner};
use rsword::package::Package;
#[cfg(feature = "compat-ts")]
use rsword::semantic::props::SettingsPatch;
use rsword::semantic::props::{Change, SectionPropsPatch, Val};
use rsword::xml::{LocalName, NodeId, QName};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const HDR_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";

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
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{inner}</w:hdr>"#
    )
}

fn session(body: &str, extra: &[(&str, &str)]) -> EditSession {
    let bytes = common::docx_with_parts(body, extra);
    EditSession::open(&bytes).expect("open")
}

/// `MOD-13` 的 oracle：增量刷新后的投影与从 DOM 整体重建相等（节序列一起比——
/// 节是块序的投影，`w:sectPr` 自己也是一个块）。
fn assert_refresh_matches_rebuild(s: &mut EditSession, what: &str) {
    let refreshed = s.document().clone();
    let rebuilt = rsword::model::Document::rebuild(s.package_mut()).expect("rebuild");
    assert_eq!(refreshed.main, rebuilt.main, "{what}: 块投影与重建不一致");
    assert_eq!(refreshed.sections, rebuilt.sections, "{what}: 节投影与重建不一致");
}

/// 正文里最后一个 `w:sectPr`（body 级的那个）。
fn body_sect_pr(s: &EditSession) -> NodeId {
    let dom = s.dom();
    dom.semantic_children(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::Body)))
        .and_then(|body| {
            dom.semantic_children(body).filter(|&n| dom.is(n, QName::w(LocalName::SectPr))).last()
        })
        .expect("body 级 sectPr")
}

/// 每个 zip 条目的 `(CRC, 压缩后字节)`——判"其他条目原压缩数据不变"用。
fn raw_entries(bytes: &[u8]) -> std::collections::BTreeMap<String, (u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index_raw(i).unwrap();
        let name = f.name().to_string();
        let crc = f.crc32();
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut raw).unwrap();
        out.insert(name, (crc, raw));
    }
    out
}

fn part_xml(bytes: &[u8], name: &str) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).ok()?;
    let mut f = z.by_name(name).ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).ok()?;
    Some(s)
}

/// `PROP-05` / `PROP-06`：新元素按 CT_SectPr 顺序插入，未碰的子元素与 `sectPr` 开标签原字节不动。
#[test]
fn edit_03_set_section_props_inserts_in_schema_order() {
    let body = concat!(
        r#"<w:p/><w:sectPr w:rsidR="00AB12CD">"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>"#,
        r#"<w:cols w:num="1"/></w:sectPr>"#
    );
    let mut s = session(body, &[]);
    let sect = body_sect_pr(&s);
    // `w:pgNumType` 在 CT_SectPr 里排在 `w:lnNumType` 之后、`w:cols` 之前
    let patch = SectionPropsPatch {
        page_numbers: Change::Set(rsword::semantic::props::PageNumber {
            fmt: Some(Val::Value(rsword::semantic::props::NumberFormat::UpperRoman)),
            start: Some(Val::Value(5)),
            chap_style: None,
            chap_sep: None,
        }),
        ..Default::default()
    };
    s.apply(EditOp::SetSectionProps { sect, patch }, &EditContext::default()).expect("apply");
    assert_refresh_matches_rebuild(&mut s, "SetSectionProps");
    let saved = s.save().expect("save");
    let xml = part_xml(&saved, "word/document.xml").expect("document.xml");
    assert!(
        xml.contains(r#"<w:pgNumType w:fmt="upperRoman" w:start="5"/><w:cols w:num="1"/>"#),
        "新元素要插在 w:cols 之前\n{xml}"
    );
    assert!(xml.contains(r#"<w:sectPr w:rsidR="00AB12CD">"#), "开标签原字节不动\n{xml}");
    assert!(
        xml.contains(r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/>"#),
        "未碰的子元素原字节不动\n{xml}"
    );
    xpath_asserts!(
        &saved,
        "word/document.xml",
        [
            ("count(/w:document/w:body/w:sectPr/w:pgNumType)", ["1"]),
            ("//w:sectPr/w:pgNumType/@w:fmt", ["upperRoman"]),
            ("//w:sectPr/w:pgNumType/@w:start", ["5"]),
            ("count(/w:document/w:body/w:sectPr/*)", ["4"]),
            ("//w:sectPr/@w:rsidR", ["00AB12CD"]),
        ]
    );
}

/// `SAVE-05`：这一节没声明该变体 → 新建 `word/header1.xml` + 关系 + 内容类型，
/// 引用插进这一节的 `sectPr` 最前；其他 zip 条目的压缩字节不变。
#[test]
fn save_05_set_header_footer_creates_the_part_and_reference() {
    let mut s = session(
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#,
        &[],
    );
    let before = s.save().expect("save");
    let sect = body_sect_pr(&s);
    s.apply(
        EditOp::SetHeaderFooter {
            sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            content: vec![NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("新页眉"))],
            }],
        },
        &EditContext::default(),
    )
    .expect("apply");
    assert_refresh_matches_rebuild(&mut s, "SetHeaderFooter 新建");
    let saved = s.save().expect("save");

    // 新 part 的内容
    let hdr_xml = part_xml(&saved, "word/header1.xml").expect("header1.xml");
    assert!(hdr_xml.contains("新页眉"), "{hdr_xml}");
    assert!(hdr_xml.starts_with(r#"<?xml version="1.0""#), "{hdr_xml}");
    // 关系与内容类型
    let rels_xml = part_xml(&saved, "word/_rels/document.xml.rels").expect(".rels");
    assert!(rels_xml.contains("header1.xml"), "{rels_xml}");
    assert!(rels_xml.contains("/relationships/header"), "{rels_xml}");
    let ct = part_xml(&saved, "[Content_Types].xml").expect("content types");
    assert!(ct.contains("wordprocessingml.header+xml"), "{ct}");
    // 引用是 sectPr 的第一个子元素（`PROP-05` 的引用组）
    let doc_xml = part_xml(&saved, "word/document.xml").expect("document.xml");
    let head = doc_xml.find("<w:headerReference").expect("引用");
    assert!(doc_xml[..head].ends_with("<w:sectPr>"), "引用要在 sectPr 最前\n{doc_xml}");
    assert!(doc_xml[head..].contains(r#"w:type="default""#), "{doc_xml}");
    assert!(doc_xml[head..].starts_with("<w:headerReference"), "{doc_xml}");
    // 其他条目的原压缩数据不变（`SAVE-05` 的页眉版）
    let (e0, e1) = (raw_entries(&before), raw_entries(&saved));
    for (name, a) in &e0 {
        if name == "word/document.xml" || name == "[Content_Types].xml" || name.ends_with(".rels") {
            continue;
        }
        let b = e1.get(name).unwrap_or_else(|| panic!("{name} 不见了"));
        assert_eq!(a, b, "{name} 的压缩字节变了");
    }

    // 模型里能查到，且这一节声明了它
    let mut pkg = Package::open(&saved).expect("reopen");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    assert_eq!(doc.hf_parts.len(), 1);
    assert_eq!(doc.sections.len(), 1);
    assert!(doc.sections[0].hf_ref(HfKind::Header, HfVariant::Default).is_some());

    xpath_asserts!(
        &saved,
        "word/header1.xml",
        [("count(/w:hdr/w:p)", ["1"]), ("string(/w:hdr/w:p/w:r/w:t)", ["新页眉"]),]
    );
    xpath_asserts!(
        &saved,
        "word/document.xml",
        [
            ("count(//w:sectPr/w:headerReference)", ["1"]),
            ("//w:sectPr/w:headerReference/@w:type", ["default"]),
            ("count(//w:sectPr/w:footerReference)", ["0"]),
        ]
    );
}

/// 这一节已经声明了该变体 → 改写它引用的 part，不新建。
#[test]
fn edit_03_set_header_footer_rewrites_the_declared_part() {
    let body = format!(
        r#"<w:p/><w:sectPr><w:headerReference {R} w:type="default" r:id="rIdH"/></w:sectPr>"#
    );
    let mut s = session(
        &body,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &hdr(r#"<w:p><w:r><w:t>旧页眉</w:t></w:r></w:p>"#)),
        ],
    );
    let sect = body_sect_pr(&s);
    s.apply(
        EditOp::SetHeaderFooter {
            sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            content: vec![NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("改过了"))],
            }],
        },
        &EditContext::default(),
    )
    .expect("apply");
    let saved = s.save().expect("save");
    let hdr_xml = part_xml(&saved, "word/header1.xml").expect("header1.xml");
    assert!(hdr_xml.contains("改过了"), "{hdr_xml}");
    assert!(!hdr_xml.contains("旧页眉"), "旧内容该被整体替换\n{hdr_xml}");
    assert!(part_xml(&saved, "word/header2.xml").is_none(), "不该新建 part");
    // `document.xml` 一个字节都没动
    let doc_xml = part_xml(&saved, "word/document.xml").expect("document.xml");
    assert_eq!(doc_xml.matches("headerReference").count(), 1, "{doc_xml}");
    xpath_asserts!(
        &saved,
        "word/header1.xml",
        [("count(/w:hdr/w:p)", ["1"]), ("string(/w:hdr/w:p/w:r/w:t)", ["改过了"]),]
    );
}

/// `LinkHeaderFooter`：给没有引用的节挂上已有 part（TS 的 `hfAllSections`）。
#[test]
fn edit_03_link_header_footer_attaches_an_existing_part() {
    // 两节：第一节（分节段落）没有引用，第二节（body 级）有
    let body = format!(
        concat!(
            r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr></w:pPr>"#,
            r#"<w:r><w:t>一</w:t></w:r></w:p>"#,
            r#"<w:p/><w:sectPr><w:headerReference {r} w:type="default" r:id="rIdH"/></w:sectPr>"#
        ),
        r = R
    );
    let mut s = session(
        &body,
        &[
            ("word/_rels/document.xml.rels", &rels(&[("rIdH", HDR_REL, "header1.xml")])),
            ("word/header1.xml", &hdr(r#"<w:p><w:r><w:t>页眉</w:t></w:r></w:p>"#)),
        ],
    );
    let (first_sect, part) = {
        let doc = s.document();
        let sect = doc.sections[0].node.expect("第一节有 sectPr");
        let part = *doc.hf_parts.keys().next().expect("页眉 part");
        (sect, part)
    };
    s.apply(
        EditOp::LinkHeaderFooter {
            sect: first_sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            part,
        },
        &EditContext::default(),
    )
    .expect("apply");
    assert_refresh_matches_rebuild(&mut s, "LinkHeaderFooter");
    let saved = s.save().expect("save");
    let doc_xml = part_xml(&saved, "word/document.xml").expect("document.xml");
    assert_eq!(doc_xml.matches("headerReference").count(), 2, "两节各一条\n{doc_xml}");
    assert!(part_xml(&saved, "word/header2.xml").is_none(), "挂已有 part，不新建");
    xpath_asserts!(
        &saved,
        "word/document.xml",
        [
            ("count(//w:p/w:pPr/w:sectPr/w:headerReference)", ["1"]),
            ("//w:p/w:pPr/w:sectPr/w:headerReference/@w:type", ["default"]),
            ("count(/w:document/w:body/w:sectPr/w:headerReference)", ["1"]),
        ]
    );

    // 再挂一次：已经有引用了，什么都不做
    let mut s2 = EditSession::open(&saved).expect("reopen");
    let (sect2, part2) = {
        let doc = s2.document();
        (doc.sections[0].node.expect("sectPr"), *doc.hf_parts.keys().next().expect("part"))
    };
    s2.apply(
        EditOp::LinkHeaderFooter {
            sect: sect2,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            part: part2,
        },
        &EditContext::default(),
    )
    .expect("apply");
    assert_eq!(s2.save().expect("save"), saved, "幂等：没有引用要加时不产生改动");
}

/// `SAVE-07 watermark`：页眉里的 VML 水印段落；`None` 删掉；Strict 包拒绝。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_watermark_round_trip() {
    let mut s = session(r#"<w:p/><w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#, &[]);
    let sect = body_sect_pr(&s);
    s.apply(EditOp::SetWatermark { sect, text: Some("草稿".into()) }, &EditContext::default())
        .expect("apply");
    assert_refresh_matches_rebuild(&mut s, "SetWatermark");
    let saved = s.save().expect("save");
    let hdr_xml = part_xml(&saved, "word/header1.xml").expect("header1.xml");
    assert!(hdr_xml.contains("<v:textpath"), "{hdr_xml}");
    assert!(hdr_xml.contains(r#"string="草稿""#), "{hdr_xml}");
    assert!(hdr_xml.contains("PowerPlusWaterMarkObject1"), "Word 认的形状 id\n{hdr_xml}");

    // 两个 `v:textpath`：`v:shapetype` 里那个是 136 型定义的一部分，带 `string` 的在 `v:shape` 上
    // （与 TS `watermarkParagraphXml` 的 `${shapetype}${shape}` 同序）
    xpath_asserts!(
        &saved,
        "word/header1.xml",
        [
            ("count(//v:textpath)", ["2"]),
            ("count(//v:shapetype/v:textpath)", ["1"]),
            ("//v:shape/v:textpath/@string", ["草稿"]),
            ("count(//v:shapetype)", ["1"]),
            ("//v:shape/@id", ["PowerPlusWaterMarkObject1"]),
            ("//w:pict/v:shape/@o:spid", ["_x0000_s2049"]),
            ("//w:p/w:pPr/w:jc/@w:val", ["center"]),
        ]
    );

    // 投影里读得出来
    let mut pkg = Package::open(&saved).expect("reopen");
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).expect("parsed_doc");
    assert_eq!(json["watermarkText"], "草稿");

    // 删掉
    let mut s2 = EditSession::open(&saved).expect("reopen");
    let sect2 = body_sect_pr(&s2);
    s2.apply(EditOp::SetWatermark { sect: sect2, text: None }, &EditContext::default())
        .expect("apply");
    let cleared = s2.save().expect("save");
    let hdr2 = part_xml(&cleared, "word/header1.xml").expect("header1.xml");
    assert!(!hdr2.contains("<v:textpath"), "{hdr2}");
    xpath_asserts!(&cleared, "word/header1.xml", [("count(//v:textpath)", ["0"])]);
    let mut pkg2 = Package::open(&cleared).expect("reopen");
    let json2 = rsword::bind::compat_ts::parsed_doc(&mut pkg2).expect("parsed_doc");
    assert_eq!(json2["watermarkText"], serde_json::Value::Null);
}

/// `SAVE-07 pageColor`：`w:background` 是 `w:document` 的第一个子元素；`None` 删掉。
#[test]
fn save_07_page_color_is_the_first_child_of_the_document() {
    let mut s = session(r#"<w:p/><w:sectPr/>"#, &[]);
    s.apply(EditOp::SetPageColor { color: Some("FFF2CC".into()) }, &EditContext::default())
        .expect("apply");
    assert_refresh_matches_rebuild(&mut s, "SetPageColor");
    let saved = s.save().expect("save");
    let xml = part_xml(&saved, "word/document.xml").expect("document.xml");
    assert!(xml.contains(r#"<w:background w:color="FFF2CC"/><w:body>"#), "{xml}");
    xpath_asserts!(
        &saved,
        "word/document.xml",
        [
            ("count(/w:document/w:background)", ["1"]),
            ("/w:document/w:background/@w:color", ["FFF2CC"]),
        ]
    );

    // 改色：只改属性
    let mut s2 = EditSession::open(&saved).expect("reopen");
    s2.apply(EditOp::SetPageColor { color: Some("112233".into()) }, &EditContext::default())
        .expect("apply");
    let x2 = part_xml(&s2.save().expect("save"), "word/document.xml").expect("document.xml");
    assert!(x2.contains(r#"w:color="112233""#), "{x2}");

    // 删掉
    let mut s3 = EditSession::open(&saved).expect("reopen");
    s3.apply(EditOp::SetPageColor { color: None }, &EditContext::default()).expect("apply");
    let x3 = part_xml(&s3.save().expect("save"), "word/document.xml").expect("document.xml");
    assert!(!x3.contains("w:background"), "{x3}");
}

/// `EDIT-03 SetDocumentSettings`：`settings.xml` 不存在时按 `SAVE-05` 新建。
#[test]
#[cfg(feature = "compat-ts")]
fn edit_03_set_document_settings_creates_the_part() {
    let mut s = session(r#"<w:p/><w:sectPr/>"#, &[]);
    s.apply(
        EditOp::SetDocumentSettings {
            patch: SettingsPatch { even_and_odd_headers: Change::Set(true), ..Default::default() },
        },
        &EditContext::default(),
    )
    .expect("apply");
    let saved = s.save().expect("save");
    let xml = part_xml(&saved, "word/settings.xml").expect("settings.xml");
    assert!(xml.contains("<w:evenAndOddHeaders/>"), "{xml}");
    xpath_asserts!(
        &saved,
        "word/settings.xml",
        [("count(/w:settings/w:evenAndOddHeaders)", ["1"])]
    );
    let mut pkg = Package::open(&saved).expect("reopen");
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).expect("parsed_doc");
    assert_eq!(json["evenAndOddHeaders"], true);
}

/// `EDIT-05`：失败的操作不留半修改状态——Strict 包写水印被拒，文档字节不变。
#[test]
fn edit_05_watermark_in_a_strict_package_is_refused() {
    let path = common::corpus_dir("synthetic").join("extra__strict-minimal.docx");
    let bytes = std::fs::read(&path).expect("Strict 语料");
    let mut s = EditSession::open(&bytes).expect("open");
    let sect = body_sect_pr(&s);
    let err = s
        .apply(EditOp::SetWatermark { sect, text: Some("X".into()) }, &EditContext::default())
        .expect_err("Strict 应拒绝");
    assert!(format!("{err}").contains("Strict"), "{err}");
    assert_eq!(s.save().expect("save"), bytes, "拒绝后一个字节都不该动");
}

/// 页眉 part 里也能用块级操作（5.5a 的位置带 part + `EDIT-03` 的块操作）。
#[test]
fn edit_03_block_operations_inside_a_header_part() {
    let body = format!(
        r#"<w:p/><w:sectPr><w:headerReference {R} w:type="default" r:id="rIdH"/></w:sectPr>"#
    );
    let doc_rels = rels(&[("rIdH", HDR_REL, "header1.xml")]);
    let header = hdr(r#"<w:p><w:r><w:t>甲</w:t></w:r></w:p>"#);
    let parts: Vec<(&str, &str)> = vec![
        ("word/_rels/document.xml.rels", doc_rels.as_str()),
        ("word/header1.xml", header.as_str()),
    ];
    let mut s = EditSession::open(&common::docx_with_parts(&body, &parts)).expect("open");
    let (part, para) = {
        let doc = s.document();
        let (&p, hf) = doc.hf_parts.iter().next().expect("页眉");
        (p, hf.text_blocks().next().expect("段落").node)
    };
    // 在页眉那段之后插一段
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::in_part(part, BlockAt::After(para)),
            block: NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("乙"))],
            },
        },
        &EditContext::default(),
    )
    .expect("insert");
    let saved = s.save().expect("save");
    let hdr_xml = part_xml(&saved, "word/header1.xml").expect("header1.xml");
    assert!(hdr_xml.contains("甲") && hdr_xml.contains("乙"), "{hdr_xml}");
    xpath_asserts!(
        &saved,
        "word/header1.xml",
        [("count(/w:hdr/w:p)", ["2"]), ("string(/w:hdr/w:p[2]/w:r/w:t)", ["乙"]),]
    );
    // `document.xml` 的原压缩数据没动（改的是页眉 part）
    let base = common::docx_with_parts(&body, &parts);
    assert_eq!(
        raw_entries(&base)["word/document.xml"],
        raw_entries(&saved)["word/document.xml"],
        "正文不该动"
    );
}

// ---------------------------------------------------------------------------
// 5.9：恶意输入与随机序列（`TEST-07` / `TEST-09`，`spec/16` 任务 5.9）
// ---------------------------------------------------------------------------

fn hostile(name: &str) -> Vec<u8> {
    let path = common::corpus_dir("hostile").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn codes(diags: &[rsword::diag::Diagnostic]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = diags.iter().map(|d| d.code.as_str()).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// 悬空的 `w:headerReference`：那个槽读成"没声明"，`hfParts` 不含悬空条目，留一条 `PKG_REL_MISSING`。
#[test]
fn test_09_dangling_header_reference() {
    let bytes = hostile("hf-dangling-reference.docx");
    let mut s = EditSession::open(&bytes).expect("open");
    {
        let doc = s.document();
        assert!(doc.hf_parts.is_empty(), "没有真的 part");
        assert!(doc.hf_by_rel.is_empty(), "悬空的 rId 不该进表");
        assert_eq!(doc.sections.len(), 1);
        // 声明还在（DOM 是真相），但查不到 part
        assert!(doc.sections[0].hf_ref(HfKind::Header, HfVariant::Default).is_some());
        assert!(codes(&doc.warnings).contains(&"PKG_REL_MISSING"), "{:?}", codes(&doc.warnings));
    }
    // 无编辑保存字节相同（不变式 1）
    assert_eq!(s.save().expect("save"), bytes);
    // 往这一节写页眉：引用在但 part 不在 → 按 `SAVE-05` 新建一个，引用换成新的
    let sect = body_sect_pr(&s);
    s.apply(
        EditOp::SetHeaderFooter {
            sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            content: vec![NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("修好了"))],
            }],
        },
        &EditContext::default(),
    )
    .expect("apply");
    let saved = s.save().expect("save");
    let hdr = part_xml(&saved, "word/header1.xml").expect("header1.xml");
    assert!(hdr.contains("修好了"), "{hdr}");
}

/// 页眉 part 是二进制垃圾：整 part 降级 `Opaque`，正文照旧可编辑，写这个 part 被拒。
#[test]
fn test_09_binary_header_part_is_opaque() {
    let bytes = hostile("hf-part-binary.docx");
    // `PKG_OPAQUE_PART` 是**包级**诊断（part 打开时就记下了），在 `Package` 的表里
    let mut probe = Package::open(&bytes).expect("open");
    let _ = rsword::model::Document::rebuild(&mut probe).expect("rebuild");
    assert!(
        codes(probe.diagnostics()).contains(&"PKG_OPAQUE_PART"),
        "{:?}",
        codes(probe.diagnostics())
    );
    let mut s = EditSession::open(&bytes).expect("open");
    assert!(s.document().hf_parts.is_empty(), "Opaque part 不进 hf_parts");
    assert_eq!(s.save().expect("save"), bytes, "无编辑保存字节相同");

    // 写这个 part 的页眉 → Err，且 DOM / 模型不变
    let sect = body_sect_pr(&s);
    let before = s.save().expect("save");
    let err = s
        .apply(
            EditOp::SetHeaderFooter {
                sect,
                kind: rsword::model::HfKind::Header,
                variant: rsword::model::HfVariant::Default,
                content: vec![NewBlock::Paragraph { props: None, inlines: Vec::new() }],
            },
            &EditContext::default(),
        )
        .expect_err("Opaque part 不能写");
    assert!(
        matches!(err, rsword::Error::Edit { code: rsword::diag::DiagCode::EditTargetOpaque, .. }),
        "{err}"
    );
    assert_eq!(s.save().expect("save"), before, "拒绝后一个字节都不该动");

    // 正文仍然可编辑
    let para = s.document().text_blocks().next().expect("正文段").node;
    s.apply(
        EditOp::InsertText { at: InlinePos::new(para, 0), text: "X".into(), props: None },
        &EditContext::default(),
    )
    .expect("正文照旧可编辑");
}

/// `w:sectPr` 里每个值都不合法：几何回退缺省，每处记 `PROP_BAD_VALUE`，无编辑保存字节相同。
#[test]
fn test_09_section_properties_with_bad_values() {
    let bytes = hostile("sectpr-bad-values.docx");
    let mut s = EditSession::open(&bytes).expect("open");
    {
        let doc = s.document();
        assert!(codes(&doc.warnings).contains(&"PROP_BAD_VALUE"), "{:?}", codes(&doc.warnings));
        let sect = &doc.sections[0];
        // 不可解析的值降级为 `Val::Raw`（原字面保留），几何按缺省算
        let sz = sect.props.page_size.as_ref().expect("w:pgSz");
        assert!(matches!(sz.w, Some(rsword::semantic::props::Val::Raw(_))), "{sz:?}");
        let geom = sect.geom();
        assert!(geom.page_width > 0 && geom.page_height > 0, "几何要回退到正数缺省: {geom:?}");
    }
    assert_eq!(s.save().expect("save"), bytes, "无编辑保存字节相同");
    // 在这个 sectPr 上改属性：坏值原位保留，新值照 `PROP-06` 落
    let sect = body_sect_pr(&s);
    s.apply(
        EditOp::SetSectionProps {
            sect,
            patch: SectionPropsPatch { title_pg: Change::Unset, ..Default::default() },
        },
        &EditContext::default(),
    )
    .expect("apply");
    let xml = part_xml(&s.save().expect("save"), "word/document.xml").expect("document.xml");
    assert!(xml.contains(r#"w:w="abc""#), "没碰的坏值原字节保留\n{xml}");
    assert!(!xml.contains("<w:titlePg"), "{xml}");
}

/// 页眉里 3000 层文本框套娃：投影不爆栈、不卡死，深处降级为 `MOD_TOO_DEEP`。
#[test]
#[cfg(feature = "compat-ts")]
fn test_09_deeply_nested_textboxes_in_a_header() {
    let bytes = hostile("hf-deep-txbx.docx");
    let mut s = EditSession::open(&bytes).expect("open");
    {
        let doc = s.document();
        assert_eq!(doc.hf_parts.len(), 1, "页眉 part 建出来了");
        assert!(codes(&doc.warnings).contains(&"MOD_TOO_DEEP"), "{:?}", codes(&doc.warnings));
    }
    assert_eq!(s.save().expect("save"), bytes, "无编辑保存字节相同");
    // 投影也要能跑完（`hfParts` 的文本取到最里层那句）
    let mut pkg = Package::open(&bytes).expect("reopen");
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).expect("parsed_doc");
    assert!(json["headerText"].is_string(), "{}", json["headerText"]);
}

/// 页眉页脚域的随机操作：页眉段落里的内联编辑 + 五个节 / 页眉页脚操作。
fn random_hf_op(s: &EditSession, rng: &mut common::Rng) -> Option<EditOp> {
    let doc = s.document();
    let sect = doc.sections.last().filter(|x| x.owner == rsword::model::SectionOwner::Body)?.node?;
    // 页眉页脚 part 里的段落（带 part 的位置，5.5a）
    let paras: Vec<(rsword::package::PartId, NodeId, u32)> = doc
        .hf_parts
        .iter()
        .flat_map(|(&p, hf)| {
            hf.text_blocks().map(move |b| (p, b.node, b.text().chars().count() as u32))
        })
        .collect();
    match rng.below(8) {
        0 | 1 if !paras.is_empty() => {
            let (part, para, len) = paras[rng.below(paras.len())];
            Some(EditOp::InsertText {
                at: InlinePos::in_part(part, para, rng.below(len as usize + 1) as u32),
                text: "随".into(),
                props: None,
            })
        }
        2 if !paras.is_empty() => {
            let (part, para, len) = paras[rng.below(paras.len())];
            if len == 0 {
                return None;
            }
            let a = rng.below(len as usize) as u32;
            Some(EditOp::DeleteRange {
                from: InlinePos::in_part(part, para, a),
                to: InlinePos::in_part(part, para, (a + 1).min(len)),
            })
        }
        3 => Some(EditOp::SetSectionProps {
            sect,
            patch: SectionPropsPatch {
                title_pg: if rng.below(2) == 0 { Change::Set(true) } else { Change::Unset },
                ..Default::default()
            },
        }),
        4 => {
            let kind = if rng.below(2) == 0 { rsword::model::HfKind::Header } else { rsword::model::HfKind::Footer };
            let variant = match rng.below(3) {
                0 => rsword::model::HfVariant::Default,
                1 => rsword::model::HfVariant::First,
                _ => rsword::model::HfVariant::Even,
            };
            Some(EditOp::SetHeaderFooter {
                sect,
                kind,
                variant,
                content: vec![NewBlock::Paragraph {
                    props: None,
                    inlines: vec![NewInline::Run(NewRun::text(format!("hf{}", rng.below(100))))],
                }],
            })
        }
        5 => Some(EditOp::SetWatermark {
            sect,
            text: (rng.below(2) == 0).then(|| format!("水{}", rng.below(10))),
        }),
        6 => {
            Some(EditOp::SetPageColor { color: (rng.below(2) == 0).then(|| "FFEEDD".to_string()) })
        }
        _ => {
            let part = *doc.hf_parts.keys().nth(rng.below(doc.hf_parts.len().max(1)))?;
            let kind = if rng.below(2) == 0 { rsword::model::HfKind::Header } else { rsword::model::HfKind::Footer };
            Some(EditOp::LinkHeaderFooter { sect, kind, variant: rsword::model::HfVariant::Default, part })
        }
    }
}

/// `TEST-07` 的页眉页脚子集：10 份带页眉页脚的语料各 100 步随机操作。
///
/// 每步断言 `MOD-13`（投影 == 重建）与"没有引擎不变式破坏"；每 20 步保存 + 重解析接着跑。
#[test]
fn test_07_random_header_footer_sequences() {
    const STEPS: usize = 100;
    let docs: Vec<_> = common::docx_paths("synthetic")
        .into_iter()
        .filter(|p| {
            let Ok(bytes) = std::fs::read(p) else { return false };
            let Ok(mut pkg) = Package::open(&bytes) else { return false };
            rsword::model::Document::rebuild(&mut pkg).is_ok_and(|d| !d.hf_parts.is_empty())
        })
        .take(10)
        .collect();
    assert_eq!(docs.len(), 10, "语料里应有至少 10 份带页眉页脚的文档");

    let mut applied = 0usize;
    let mut rejected = 0usize;
    for (di, path) in docs.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let mut s = EditSession::open(&bytes).unwrap();
        let mut rng = common::Rng(0x5DEE_CE66_D3B1_1EAD ^ di as u64);
        for step in 0..STEPS {
            let Some(op) = random_hf_op(&s, &mut rng) else { continue };
            let what = format!("{}: step {step}", path.display());
            match s.apply(op, &EditContext::default()) {
                Ok(_) => applied += 1,
                Err(rsword::Error::Edit { .. }) => {
                    rejected += 1;
                    continue; // 拒绝是合法结果；`EDIT-05` 保证状态没动
                }
                Err(e) => panic!("{what}: 非编辑错误 {e}"),
            }
            assert_refresh_matches_rebuild(&mut s, &what);
            assert!(
                !s.diagnostics()
                    .iter()
                    .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
                "{what}: {:?}",
                s.diagnostics()
            );
            if step % 20 == 19 {
                let saved = s.save().unwrap_or_else(|e| panic!("{what}: save {e}"));
                let before = hf_shapes(s.document());
                let re = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what}: reopen {e}"));
                assert_eq!(hf_shapes(re.document()), before, "{what}: 保存往返后页眉形状变了");
                s = re;
            }
        }
    }
    eprintln!("random hf ops: {applied} 次生效，{rejected} 次被拒");
    assert!(applied > 300, "有效操作太少：{applied}");
}

/// 页眉页脚的可比较快照：每个 part 的（种类, 各文本块的文本）。
fn hf_shapes(doc: &Document) -> Vec<(HfKind, Vec<String>)> {
    doc.hf_parts
        .values()
        .map(|hf| {
            (hf.kind, hf.text_blocks().map(rsword::model::TextBlock::text).collect::<Vec<_>>())
        })
        .collect()
}
