//! `SAVE-01` 编排与 `SAVE-07` 保存选项（任务 1.14 第二批）：`save_with(opts)` 的六步流程、
//! 不变式 1 的短路条件、`saved_at` 只改 `core.xml`、`remove_personal_info` 的全包清洗与
//! `settings.xml` 标志、`remove_date_and_time`（OOXML 有、TS 没有的一项）。
//! 验收清单 `SAVE-07`：清洗后没有 `w:author` 不是 `Author` 的修订。

mod common;

use std::io::{Cursor, Read};

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::package::Package;
use rsword::save::options::CompatSaveOptions as SaveOptions;
use rsword::xml::xpath_strings;

fn corpus(name: &str) -> Vec<u8> {
    let path = common::corpus_dir("synthetic").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// 保存结果里某个 part 的文本。
fn part_text(saved: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(saved)).unwrap();
    let mut entry = zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut s = String::new();
    entry.read_to_string(&mut s).unwrap();
    s
}

/// 两份包里同名条目的 CRC 是否一致（`SAVE-06`：未变 part 原压缩数据直接拷贝）。
fn same_entry(a: &[u8], b: &[u8], name: &str) -> bool {
    let mut za = zip::ZipArchive::new(Cursor::new(a)).unwrap();
    let mut zb = zip::ZipArchive::new(Cursor::new(b)).unwrap();
    let (ca, cb) = (za.by_name(name).unwrap().crc32(), zb.by_name(name).unwrap().crc32());
    ca == cb
}

fn xpath_on(xml: &str, expr: &str) -> Vec<String> {
    let dom = rsword::xml::Dom::parse(rsword::package::PartId(0), xml.as_bytes()).unwrap();
    xpath_strings(&dom, expr).unwrap()
}

/// `SAVE-01` 步骤 1：没有脏节点、没有强制选项 → 原字节。单独设 `saved_at` 也不触发保存
/// （与 TS `isUnchanged` 一致；否则"打开→保存字节相同"的不变式会被时间戳破坏）。
#[test]
fn save_01_unchanged_document_returns_original_bytes() {
    let bytes = corpus("docprops__001.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    assert_eq!(s.save().unwrap(), bytes, "空选项");
    let stamped =
        SaveOptions { saved_at: Some("2026-07-28T08:30:00Z".into()), ..Default::default() };
    assert_eq!(s.save_with_compat(&stamped).unwrap(), bytes, "saved_at 不强制保存");
    // remove_personal_info 是强制选项：进入流程，但这份语料没有 settings.xml 也没有作者信息。
    // 写 `false` 不需要 part（标志缺失就等于 false），所以没有任何 part 变脏，仍是原字节
    let forced = SaveOptions { remove_personal_info: Some(false), ..Default::default() };
    assert!(forced.forces_save());
    assert!(s.save_with_compat(&forced).unwrap() == bytes, "无可改动时仍是原字节");
    assert!(s.diagnostics().is_empty(), "没什么可诊断的: {:?}", s.diagnostics());
    // 有 settings.xml 的文档：强制选项确实产生输出
    let other = corpus("revisions__013.docx");
    let mut s2 = EditSession::open(&other).unwrap();
    assert!(s2.save().unwrap() == other, "未编辑保存字节相同");
    let on = SaveOptions { remove_personal_info: Some(true), ..Default::default() };
    assert!(s2.save_with_compat(&on).unwrap() != other, "写入标志后输出不同");
}

/// `SAVE-07` `saved_at`：只改 `docProps/core.xml` 的 `dcterms:modified`（毫秒去掉）与 `cp:revision`，
/// 其他条目原压缩数据不动。
#[test]
fn save_07_saved_at_stamps_core_props_only() {
    let bytes = corpus("docprops__001.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let p = s.nth_text_block(0).unwrap().node;
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 0), text: "改".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    let opts =
        SaveOptions { saved_at: Some("2026-07-28T08:30:00.123Z".into()), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let core = part_text(&saved, "docProps/core.xml");
    assert_eq!(
        xpath_on(&core, "//dcterms:modified/text()"),
        ["2026-07-28T08:30:00Z"],
        "毫秒被去掉"
    );
    assert_eq!(xpath_on(&core, "//cp:revision/text()"), ["4"], "revision 3 → 4");
    assert_eq!(
        xpath_on(&core, "//dcterms:created/text()"),
        ["2020-01-01T00:00:00Z"],
        "created 不动"
    );
    assert_eq!(xpath_on(&core, "//dc:creator/text()"), ["作者甲"], "没要求清洗就不动作者");
    assert!(same_entry(&bytes, &saved, "word/styles.xml"), "未涉及的 part 原样");
    assert!(!same_entry(&bytes, &saved, "word/document.xml"), "改过字的 part 重写");
}

/// `SAVE-07` 验收：`remove_personal_info` 后没有 `w:author` 不是 `Author` 的修订 / 批注；
/// `w:initials` → `A`；`core.xml` 的 creator / lastModifiedBy 清空；`app.xml` 的 Manager / Company 清空；
/// `people.xml` 的 `w15:person` 删除；`customXml` 与 `docProps/custom.xml` 不动；`w:date` 保留（TS 行为）。
#[test]
fn save_07_remove_personal_info_scrubs_the_whole_package() {
    let bytes = corpus("write-protection__004.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { remove_personal_info: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();

    for part in [
        "word/document.xml",
        "word/comments.xml",
        "word/header1.xml",
        "word/footer1.xml",
        "word/footnotes.xml",
        "word/endnotes.xml",
        "word/glossary/document.xml",
    ] {
        let xml = part_text(&saved, part);
        assert_eq!(
            xpath_on(&xml, "count(//*[@w:author][@w:author!='Author'])"),
            ["0"],
            "{part}: 仍有非 Author 的作者"
        );
        assert_eq!(
            xpath_on(&xml, "count(//*[@w:initials][@w:initials!='A'])"),
            ["0"],
            "{part}: 仍有非 A 的缩写"
        );
    }
    let doc = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&doc, "count(//w:ins[@w:author='Author'])"), ["1"]);
    assert_eq!(
        xpath_on(&doc, "//w:ins/@w:date"),
        ["2024-02-01T02:03:04Z"],
        "w:date 保留（TS 行为）"
    );
    // 正文里作为文字出现的 author=… 不受影响（我们只改属性）
    assert!(doc.contains("Visible Person"), "run 文本不该被改写");

    let core = part_text(&saved, "docProps/core.xml");
    assert_eq!(xpath_on(&core, "//dc:creator/text()"), Vec::<String>::new(), "creator 清空");
    assert_eq!(xpath_on(&core, "//cp:lastModifiedBy/text()"), Vec::<String>::new());
    let app = part_text(&saved, "docProps/app.xml");
    assert_eq!(xpath_on(&app, "//ep:Manager/text()"), Vec::<String>::new(), "Manager 清空");
    assert_eq!(xpath_on(&app, "//ep:Company/text()"), Vec::<String>::new());
    assert_eq!(xpath_on(&app, "//ep:Application/text()"), ["Preserved App"], "其他属性不动");
    let people = part_text(&saved, "word/people.xml");
    assert_eq!(xpath_on(&people, "count(//w15:person)"), ["0"], "person 条目删除");
    assert!(same_entry(&bytes, &saved, "customXml/item1.xml"), "customXml 不清洗");
    assert!(same_entry(&bytes, &saved, "docProps/custom.xml"), "自定义属性不清洗");
    // 这份语料没有 settings.xml：按 `SAVE-05` 建出来，标志写进去（M1 时这里只记诊断）
    let settings = part_text(&saved, "word/settings.xml");
    // 裸元素就是 true（`ST_OnOff` 的缺省）
    assert_eq!(xpath_on(&settings, "count(//w:removePersonalInformation)"), ["1"], "{settings}");
    let ct = part_text(&saved, "[Content_Types].xml");
    assert!(ct.contains("/word/settings.xml"), "内容类型 Override: {ct}");
    let rels = part_text(&saved, "word/_rels/document.xml.rels");
    assert!(rels.contains("settings.xml"), "关系: {rels}");
}

/// `SAVE-07`：文档自带标志时，无编辑的保存也清洗（TS `scrubPersonalInfo`）；
/// `Some(false)` 写回时删除标志且不清洗，且标志元素按 `PROP-05` 位置处理（前缀不是 `w:` 也认）。
#[test]
fn save_07_document_flag_and_explicit_false() {
    let bytes = corpus("write-protection__005.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    assert!(s.remove_personal_info_flag(), "语料的 settings.xml 带标志（前缀是 s:）");
    let saved = s.save_with_compat(&SaveOptions::default()).unwrap();
    let doc = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&doc, "//w:ins/@w:author"), ["Author"], "文档标志触发清洗");
    assert!(same_entry(&bytes, &saved, "word/settings.xml"), "没要求改标志就不动 settings");

    let mut s2 = EditSession::open(&bytes).unwrap();
    let off = SaveOptions { remove_personal_info: Some(false), ..Default::default() };
    let saved2 = s2.save_with_compat(&off).unwrap();
    let settings = part_text(&saved2, "word/settings.xml");
    assert_eq!(xpath_on(&settings, "count(//w:removePersonalInformation)"), ["0"], "标志被删除");
    let doc2 = part_text(&saved2, "word/document.xml");
    assert_eq!(xpath_on(&doc2, "//w:ins/@w:author"), ["张三"], "关掉后不清洗");
    assert!(same_entry(&bytes, &saved2, "word/document.xml"), "document.xml 原样");
}

/// `SAVE-07`：`Some(true)` 在已有 settings.xml 的文档里写入标志（`PROP-05` 顺序由属性表保证）。
#[test]
fn save_07_flag_written_into_existing_settings() {
    let bytes = corpus("revisions__013.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    assert!(!s.remove_personal_info_flag());
    let on = SaveOptions { remove_personal_info: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&on).unwrap();
    let settings = part_text(&saved, "word/settings.xml");
    assert_eq!(xpath_on(&settings, "count(//w:removePersonalInformation)"), ["1"]);
    assert_eq!(xpath_on(&settings, "count(//w:zoom)"), ["1"], "原有设置不动");
    let mut again = Package::open(&saved).unwrap();
    let id = again.find_name("word/settings.xml").unwrap();
    let dom = again.dom(id).unwrap().unwrap();
    let mut diags = Vec::new();
    let root = dom.root();
    let read = rsword::semantic::props::read_settings(dom, Some(root), &mut diags);
    assert_eq!(read.remove_personal_information, Some(true), "重新解析后标志成立");
}

/// 最小 docx：便于构造带 `w:settings` 标志的文档（语料里没有带 `w:removeDateAndTime` 的）。
fn build_docx(document_xml: &str, settings_xml: Option<&str>) -> Vec<u8> {
    use std::io::Write;
    let ct = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut files: Vec<(&str, &str)> = vec![
        ("[Content_Types].xml", ct),
        ("_rels/.rels", rels),
        ("word/_rels/document.xml.rels", doc_rels),
        ("word/document.xml", document_xml),
    ];
    if let Some(sx) = settings_xml {
        files.push(("word/settings.xml", sx));
    }
    for (name, bytes) in files {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// `remove_date_and_time`（TS 没有这项能力）：批注元素上的 `w:date` 删除、作者不动、标志写进
/// `settings.xml` 且与已有设置同序；文档自带标志时无编辑的保存也执行。
#[test]
fn save_07_remove_date_and_time_drops_annotation_dates() {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body><w:p><w:ins w:id="1" w:author="张三" w:date="2026-01-01T00:00:00Z"><w:r><w:t>a</w:t></w:r></w:ins><w:del w:id="2" w:author="李四" w:date="2026-01-02T00:00:00Z"><w:r><w:delText>b</w:delText></w:r></w:del></w:p><w:sectPr/></w:body></w:document>"#
    );
    let settings = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"><w:zoom w:percent="100"/></w:settings>"#
    );
    let bytes = build_docx(&doc, Some(&settings));

    let mut s = EditSession::open(&bytes).unwrap();
    assert!(!s.remove_date_and_time_flag());
    let opts = SaveOptions { remove_date_and_time: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let out = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&out, "count(//*[@w:date])"), ["0"], "批注日期全删");
    assert_eq!(xpath_on(&out, "//w:ins/@w:author"), ["张三"], "作者不动");
    assert_eq!(xpath_on(&out, "//w:del/@w:author"), ["李四"]);
    let set = part_text(&saved, "word/settings.xml");
    assert_eq!(xpath_on(&set, "count(//w:removeDateAndTime)"), ["1"], "标志写入");
    assert_eq!(xpath_on(&set, "count(//w:removePersonalInformation)"), ["0"], "另一项不受影响");
    assert_eq!(xpath_on(&set, "count(//w:zoom)"), ["1"], "原有设置保留");

    // 文档自带标志 → 无编辑的保存也删日期
    let flagged = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"><w:removeDateAndTime/></w:settings>"#
    );
    let mut s2 = EditSession::open(&build_docx(&doc, Some(&flagged))).unwrap();
    assert!(s2.remove_date_and_time_flag());
    let saved2 = s2.save_with_compat(&SaveOptions::default()).unwrap();
    let out2 = part_text(&saved2, "word/document.xml");
    assert_eq!(xpath_on(&out2, "count(//*[@w:date])"), ["0"]);
    assert_eq!(xpath_on(&out2, "//w:ins/@w:author"), ["张三"], "只删日期不改作者");

    // 两项一起开：作者与日期都清
    let mut s3 = EditSession::open(&bytes).unwrap();
    let both = SaveOptions {
        remove_personal_info: Some(true),
        remove_date_and_time: Some(true),
        ..Default::default()
    };
    let saved3 = s3.save_with_compat(&both).unwrap();
    let out3 = part_text(&saved3, "word/document.xml");
    assert_eq!(xpath_on(&out3, "count(//*[@w:date])"), ["0"]);
    assert_eq!(xpath_on(&out3, "count(//*[@w:author][@w:author!='Author'])"), ["0"]);
    let set3 = part_text(&saved3, "word/settings.xml");
    assert_eq!(xpath_on(&set3, "count(//w:removePersonalInformation)"), ["1"]);
    assert_eq!(xpath_on(&set3, "count(//w:removeDateAndTime)"), ["1"]);
}

/// `remove_personal_info` 单独开启时日期保留（与 TS 一致）。
#[test]
fn save_07_personal_info_alone_keeps_dates() {
    let bytes = corpus("write-protection__003.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { remove_personal_info: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let out = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&out, "//w:ins/@w:date"), ["2026-01-01T00:00:00Z"]);
    assert_eq!(xpath_on(&out, "//w:ins/@w:author"), ["Author"]);
}

// ---------------------------------------------------------------------------
// 5.6a：节与包级保存选项（`SAVE-07`，`spec/16` 任务 5.6）
// ---------------------------------------------------------------------------

use rsword::save::options::{
    PgNumTypeOption, ProtectionOption, SectionSaveSettings, WriteProtectionOption,
};
use rsword::semantic::props::{DocProtect, NumberFormat, SectType};

/// 一份带 body 级 `w:sectPr` 的最小文档。
fn sect_docx(sect_pr: &str) -> Vec<u8> {
    common::docx_with_body(&format!(r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p>{sect_pr}"#))
}

fn a4() -> SectionSaveSettings {
    SectionSaveSettings {
        page_width: 11906,
        page_height: 16838,
        margin_top: 1440,
        margin_right: 1440,
        margin_bottom: 1440,
        margin_left: 1440,
        columns: 1,
        ..Default::default()
    }
}

/// `section`：页面尺寸 / 边距整体重写，`w:gutter` 这类没给出的属性沿用原值。
#[test]
fn save_07_section_settings_rewrite_page_setup() {
    let bytes = sect_docx(concat!(
        r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>"#,
        r#"<w:pgMar w:top="1" w:right="2" w:bottom="3" w:left="4" w:header="55" w:footer="66" w:gutter="77"/>"#,
        r#"</w:sectPr>"#
    ));
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        section: Some(SectionSaveSettings { landscape: true, ..a4() }),
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "//w:pgSz/@w:w"), ["11906"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgSz/@w:orient"), ["landscape"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:top"), ["1440"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:gutter"), ["77"], "没给出的属性沿用原值\n{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:header"), ["55"], "headerDist 缺省沿用\n{xml}");
    // 纵向不写 w:orient（同 TS）
    let mut s2 = EditSession::open(&bytes).unwrap();
    let portrait = SaveOptions { section: Some(a4()), ..Default::default() };
    let xml2 = part_text(&s2.save_with_compat(&portrait).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&xml2, "count(//w:pgSz/@w:orient)"), ["0"], "{xml2}");
}

/// `section.pageBorder` / `columns`：一圈单线边框；多栏与不等宽。
#[test]
fn save_07_section_settings_borders_and_columns() {
    let bytes = sect_docx(r#"<w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        section: Some(SectionSaveSettings {
            page_border: true,
            columns: 3,
            col_space: Some(360),
            ..a4()
        }),
        ..Default::default()
    };
    let xml = part_text(&s.save_with_compat(&opts).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&xml, "count(//w:pgBorders/*)"), ["4"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgBorders/@w:offsetFrom"), ["page"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgBorders/w:top/@w:sz"), ["4"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgBorders/w:top/@w:space"), ["24"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:cols/@w:num"), ["3"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:cols/@w:space"), ["360"], "{xml}");
    // 不等宽：最后一栏不带 w:space
    let mut s2 = EditSession::open(&bytes).unwrap();
    let uneven = SaveOptions {
        section: Some(SectionSaveSettings {
            columns: 2,
            col_widths: Some(vec![4000, 5000]),
            ..a4()
        }),
        ..Default::default()
    };
    let x2 = part_text(&s2.save_with_compat(&uneven).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&x2, "//w:cols/@w:equalWidth"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "//w:cols/w:col/@w:w"), ["4000", "5000"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:cols/w:col/@w:space)"), ["1"], "{x2}");
    // 关掉边框：元素删掉
    let bordered = s.save_with_compat(&opts).unwrap();
    let mut s3 = EditSession::open(&bordered).unwrap();
    let off = SaveOptions { section: Some(a4()), ..Default::default() };
    let x3 = part_text(&s3.save_with_compat(&off).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&x3, "count(//w:pgBorders)"), ["0"], "{x3}");
}

/// `sectionStartType` / `pgNumType` / `titlePg`：三项都落在最后一节，位置按 `PROP-05`。
#[test]
fn save_07_section_start_type_page_numbers_and_title_page() {
    let bytes = sect_docx(concat!(
        r#"<w:sectPr><w:type w:val="oddPage"/><w:pgSz w:w="1" w:h="2"/>"#,
        r#"<w:cols w:num="1"/><w:docGrid w:linePitch="312"/></w:sectPr>"#
    ));
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        section_start_type: Some(SectType::Continuous),
        pg_num_type: Some(PgNumTypeOption { fmt: Some(NumberFormat::UpperRoman), start: Some(5) }),
        title_pg: Some(true),
        ..Default::default()
    };
    let xml = part_text(&s.save_with_compat(&opts).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&xml, "//w:sectPr/w:type/@w:val"), ["continuous"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgNumType/@w:fmt"), ["upperRoman"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgNumType/@w:start"), ["5"], "{xml}");
    assert_eq!(xpath_on(&xml, "count(//w:titlePg)"), ["1"], "{xml}");
    // `w:pgNumType` 在 `w:cols` 之前（CT_SectPr 顺序），`w:titlePg` 在 `w:docGrid` 之前
    let (i_num, i_cols) = (xml.find("<w:pgNumType").unwrap(), xml.find("<w:cols").unwrap());
    assert!(i_num < i_cols, "w:pgNumType 要在 w:cols 之前\n{xml}");
    let (i_title, i_grid) = (xml.find("<w:titlePg").unwrap(), xml.find("<w:docGrid").unwrap());
    assert!(i_title < i_grid, "w:titlePg 要在 w:docGrid 之前\n{xml}");

    // `nextPage` 是缺省 → 删掉 `w:type`；两个字段都缺 → 删掉 `w:pgNumType`；`titlePg: false` → 删掉
    let mut s2 = EditSession::open(&s.save_with_compat(&opts).unwrap()).unwrap();
    let clear = SaveOptions {
        section_start_type: Some(SectType::NextPage),
        pg_num_type: Some(PgNumTypeOption::default()),
        title_pg: Some(false),
        ..Default::default()
    };
    let x2 = part_text(&s2.save_with_compat(&clear).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&x2, "count(//w:sectPr/w:type)"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:pgNumType)"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:titlePg)"), ["0"], "{x2}");
}

/// 一个 `w:sectPr` 都没有的文档：四项都无处可落，不凭空造分节符（`docs/04` §8）。
#[test]
fn save_07_section_options_need_an_existing_sect_pr() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>只有一段</w:t></w:r></w:p>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { section: Some(a4()), title_pg: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "count(//w:sectPr)"), ["0"], "{xml}");
    assert_eq!(saved, bytes, "没有可改的东西 → 原字节");
}

/// `pageColor`：`w:background` + `settings.xml` 的 `w:displayBackgroundShape`。
#[test]
fn save_07_page_color_also_opts_in_via_settings() {
    let bytes = sect_docx(r#"<w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { page_color: Some(Some("FFF2CC".into())), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "/w:document/w:background/@w:color"), ["FFF2CC"], "{xml}");
    let settings = part_text(&saved, "word/settings.xml");
    assert_eq!(xpath_on(&settings, "count(//w:displayBackgroundShape)"), ["1"], "{settings}");
    // 删底色：`w:background` 走了，开关留着（同 TS）
    let mut s2 = EditSession::open(&saved).unwrap();
    let clear = SaveOptions { page_color: Some(None), ..Default::default() };
    let cleared = s2.save_with_compat(&clear).unwrap();
    let x2 = part_text(&cleared, "word/document.xml");
    assert_eq!(xpath_on(&x2, "count(//w:background)"), ["0"], "{x2}");
}

/// `protection` / `writeProtection`：口令散列的七个属性只在有 `hash` 时写，缺省 sid 14 /
/// spinCount 100000；`None` 删除。
#[test]
fn save_07_protection_options() {
    let bytes = sect_docx(r#"<w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        protection: Some(Some(ProtectionOption {
            edit: DocProtect::ReadOnly,
            enforced: true,
            hash: None,
            salt: None,
            spin_count: None,
            algorithm_sid: None,
        })),
        write_protection: Some(Some(WriteProtectionOption {
            recommended: false,
            hash: Some("aGFzaA==".into()),
            salt: Some("c2FsdA==".into()),
            spin_count: None,
            algorithm_sid: None,
        })),
        ..Default::default()
    };
    let settings = part_text(&s.save_with_compat(&opts).unwrap(), "word/settings.xml");
    assert_eq!(xpath_on(&settings, "//w:documentProtection/@w:edit"), ["readOnly"], "{settings}");
    assert_eq!(xpath_on(&settings, "//w:documentProtection/@w:enforcement"), ["1"], "{settings}");
    assert_eq!(
        xpath_on(&settings, "count(//w:documentProtection/@w:hash)"),
        ["0"],
        "没有口令就不写 crypt 属性\n{settings}"
    );
    assert_eq!(
        xpath_on(&settings, "//w:writeProtection/@w:cryptAlgorithmSid"),
        ["14"],
        "{settings}"
    );
    assert_eq!(
        xpath_on(&settings, "//w:writeProtection/@w:cryptSpinCount"),
        ["100000"],
        "{settings}"
    );
    assert_eq!(xpath_on(&settings, "//w:writeProtection/@w:hash"), ["aGFzaA=="], "{settings}");

    // 删除
    let mut s2 = EditSession::open(&s.save_with_compat(&opts).unwrap()).unwrap();
    let clear =
        SaveOptions { protection: Some(None), write_protection: Some(None), ..Default::default() };
    let x2 = part_text(&s2.save_with_compat(&clear).unwrap(), "word/settings.xml");
    assert_eq!(xpath_on(&x2, "count(//w:documentProtection)"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:writeProtection)"), ["0"], "{x2}");
    // 既不 recommended 也没有口令 → 等于删除（同 TS）
    let mut s3 = EditSession::open(&bytes).unwrap();
    let empty = SaveOptions {
        write_protection: Some(Some(WriteProtectionOption::default())),
        ..Default::default()
    };
    let x3 = part_text(&s3.save_with_compat(&empty).unwrap(), "word/settings.xml");
    assert_eq!(xpath_on(&x3, "count(//w:writeProtection)"), ["0"], "{x3}");
}

/// `evenAndOddHeaders`：写入后重解析，投影里读得出来（重解析 oracle）。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_even_and_odd_headers_round_trips_through_the_projection() {
    let bytes = sect_docx(r#"<w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { even_and_odd_headers: Some(true), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["evenAndOddHeaders"], true);
    let mut s2 = EditSession::open(&saved).unwrap();
    let off = SaveOptions { even_and_odd_headers: Some(false), ..Default::default() };
    let cleared = s2.save_with_compat(&off).unwrap();
    let mut pkg2 = Package::open(&cleared).unwrap();
    let json2 = rsword::bind::compat_ts::parsed_doc(&mut pkg2).unwrap();
    assert_eq!(json2["evenAndOddHeaders"], false);
}

// ---------------------------------------------------------------------------
// 5.6b：页眉页脚保存选项（`SAVE-07` / `SAVE-05`，`spec/16` 任务 5.6b）
// ---------------------------------------------------------------------------

use rsword::edit::{NewBlock, NewInline, NewRun};
use rsword::model::{HfKind, HfVariant};
use rsword::save::options::SectionHfSave;

fn para(text: &str) -> NewBlock {
    NewBlock::Paragraph { props: None, inlines: vec![NewInline::Run(NewRun::text(text))] }
}

/// 六个槽的表：字段、kind / variant、TS 键名一一对应（`hf_slots!` 展开的那张表）。
#[test]
fn save_07_hf_slots_cover_all_six_variants() {
    let mut slots = rsword::save::options::HfSlots::default();
    assert!(slots.is_empty());
    for key in ["header", "footer", "headerFirst", "footerFirst", "headerEven", "footerEven"] {
        *slots.by_ts_key(key).unwrap_or_else(|| panic!("{key} 不在表里")) = Some(vec![para(key)]);
    }
    assert!(slots.by_ts_key("headerOdd").is_none(), "非法键要返回 None");
    let seen: Vec<(HfKind, HfVariant)> = slots.iter().map(|(k, v, _)| (k, v)).collect();
    assert_eq!(seen.len(), 6, "六个槽都能迭代到");
    assert_eq!(seen[0], (HfKind::Header, HfVariant::Default), "default 在最前（同 TS 的调用顺序）");
    for (i, a) in seen.iter().enumerate() {
        assert!(!seen[..i].contains(a), "六个 kind × variant 组合互不重复: {a:?}");
    }
}

/// 没声明变体 → 按 `SAVE-05` 新建 part；引用是 `sectPr` 第一个子元素。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_header_option_creates_the_part() {
    let bytes = sect_docx(r#"<w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let mut opts = SaveOptions::default();
    *opts.hf.by_ts_key("header").unwrap() = Some(vec![para("新页眉")]);
    *opts.hf.by_ts_key("footerFirst").unwrap() = Some(vec![para("首页页脚")]);
    let saved = s.save_with_compat(&opts).unwrap();
    let hdr = part_text(&saved, "word/header1.xml");
    assert!(hdr.contains("新页眉"), "{hdr}");
    let ftr = part_text(&saved, "word/footer1.xml");
    assert!(ftr.contains("首页页脚"), "{ftr}");
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "//w:sectPr/w:headerReference/@w:type"), ["default"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:sectPr/w:footerReference/@w:type"), ["first"], "{xml}");
    // 引用组在其他子元素之前（`PROP-05` 第 0 格）
    let (i_ref, i_sz) = (xml.find("Reference").unwrap(), xml.find("<w:pgSz").unwrap());
    assert!(i_ref < i_sz, "引用要在 w:pgSz 之前\n{xml}");
    // 重解析：投影里读得出来
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["headerText"], "新页眉");
    assert_eq!(json["footerFirst"]["text"], "首页页脚");
}

/// 外科合并：part 里的表格与带图的段落原字节保留，只有文本段落整体替换。
#[test]
fn save_07_header_option_merges_surgically() {
    let hdr = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:p><w:r><w:t>旧文字一</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:p><w:r><w:pict><v:rect xmlns:v="urn:schemas-microsoft-com:vml" id="logo"/></w:pict></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>旧文字二</w:t></w:r></w:p>"#,
        r#"</w:hdr>"#
    );
    let doc_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
        r#"</Relationships>"#
    );
    let body = concat!(
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:headerReference xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" w:type="default" r:id="rIdH"/><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#
    );
    let bytes = common::docx_with_parts(
        body,
        &[("word/_rels/document.xml.rels", doc_rels), ("word/header1.xml", hdr)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let mut opts = SaveOptions::default();
    *opts.hf.by_ts_key("header").unwrap() = Some(vec![para("新文字")]);
    let saved = s.save_with_compat(&opts).unwrap();
    let out = part_text(&saved, "word/header1.xml");
    assert!(out.contains("新文字"), "{out}");
    assert!(!out.contains("旧文字一") && !out.contains("旧文字二"), "文本段落整体替换\n{out}");
    assert!(out.contains("<w:tbl>") && out.contains("格"), "表格原样保留\n{out}");
    assert!(out.contains(r#"id="logo""#), "带图的段落原样保留\n{out}");
    // 新内容落在第一个文本段落的位置：表格之前
    let (i_new, i_tbl) = (out.find("新文字").unwrap(), out.find("<w:tbl>").unwrap());
    assert!(i_new < i_tbl, "新内容要落在第一个文本段落的位置\n{out}");
    assert_eq!(xpath_on(&out, "count(/w:hdr/w:p)"), ["2"], "一段新文字 + 一段图\n{out}");
}

/// `sectionHf`：指定某一节的页脚；只碰那一节的 `sectPr`。
#[test]
fn save_07_section_hf_targets_one_section() {
    let body = concat!(
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>二</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="2" w:h="2"/></w:sectPr>"#
    );
    let bytes = common::docx_with_body(body);
    let mut s = EditSession::open(&bytes).unwrap();
    let sect = s.document().sections[0].node.unwrap();
    let opts = SaveOptions {
        section_hf: vec![SectionHfSave {
            sect,
            kind: HfKind::Footer,
            variant: HfVariant::Default,
            blocks: vec![para("第一节页脚")],
        }],
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    let ftr = part_text(&saved, "word/footer1.xml");
    assert!(ftr.contains("第一节页脚"), "{ftr}");
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(
        xpath_on(&xml, "count(//w:p/w:pPr/w:sectPr/w:footerReference)"),
        ["1"],
        "引用落在第一节\n{xml}"
    );
    assert_eq!(
        xpath_on(&xml, "count(/w:document/w:body/w:sectPr/w:footerReference)"),
        ["0"],
        "最后一节不受影响\n{xml}"
    );
}

/// `hfAllSections`：新建的 part 挂到每个自己不带引用的 `sectPr` 上；带引用的那节不碰。
#[test]
fn save_07_hf_all_sections_propagates_only_new_parts() {
    let body = concat!(
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>二</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="2" w:h="2"/></w:sectPr>"#
    );
    let bytes = common::docx_with_body(body);
    let mut s = EditSession::open(&bytes).unwrap();
    let mut opts = SaveOptions { hf_all_sections: true, ..Default::default() };
    *opts.hf.by_ts_key("header").unwrap() = Some(vec![para("重复页眉")]);
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "count(//w:headerReference)"), ["2"], "两节各一条\n{xml}");
    let ids = xpath_on(&xml, "//w:headerReference/@r:id");
    assert_eq!(ids[0], ids[1], "两节指向同一个 part\n{xml}");
    assert!(part_text(&saved, "word/header1.xml").contains("重复页眉"));
    // 没开 hfAllSections：只有最后一节拿到引用
    let mut s2 = EditSession::open(&bytes).unwrap();
    let mut only_last = SaveOptions::default();
    *only_last.hf.by_ts_key("header").unwrap() = Some(vec![para("只最后一节")]);
    let x2 = part_text(&s2.save_with_compat(&only_last).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&x2, "count(//w:headerReference)"), ["1"], "{x2}");
}

/// `watermark` 单独出现 → 只动水印段落；与 `header` 同出现 → 先内容后水印。
#[test]
fn save_07_watermark_option_alone_and_with_content() {
    let bytes = sect_docx(r#"<w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let mut opts = SaveOptions { watermark: Some(Some("草稿".into())), ..Default::default() };
    *opts.hf.by_ts_key("header").unwrap() = Some(vec![para("页眉文字")]);
    let saved = s.save_with_compat(&opts).unwrap();
    let hdr = part_text(&saved, "word/header1.xml");
    assert!(hdr.contains("<v:textpath") && hdr.contains("页眉文字"), "{hdr}");
    let (i_wm, i_text) = (hdr.find("<v:textpath").unwrap(), hdr.find("页眉文字").unwrap());
    assert!(i_wm < i_text, "水印段落在最前\n{hdr}");

    // 只给水印：页眉文字不动
    let mut s2 = EditSession::open(&saved).unwrap();
    let only = SaveOptions { watermark: Some(Some("机密".into())), ..Default::default() };
    let x2 = part_text(&s2.save_with_compat(&only).unwrap(), "word/header1.xml");
    assert!(x2.contains(r#"string="机密""#), "{x2}");
    assert!(x2.contains("页眉文字"), "内容不动\n{x2}");
    assert_eq!(xpath_on(&x2, "count(//v:shape)"), ["1"], "只有一个水印形状\n{x2}");

    // 删水印：只有水印段落走
    let mut s3 = EditSession::open(&saved).unwrap();
    let clear = SaveOptions { watermark: Some(None), ..Default::default() };
    let x3 = part_text(&s3.save_with_compat(&clear).unwrap(), "word/header1.xml");
    assert_eq!(xpath_on(&x3, "count(//v:textpath)"), ["0"], "{x3}");
    assert!(x3.contains("页眉文字"), "内容不动\n{x3}");
}

// ---------------------------------------------------------------------------
// 5.7：声明 part 的保存选项（`SAVE-07` / `SAVE-05`，`spec/16` 任务 5.7）
// ---------------------------------------------------------------------------

use rsword::save::options::StyleUpsertSave;
#[cfg(feature = "compat-ts")]
use rsword::save::options::{
    NumberingDefSave, NumberingLevelSave, RestartNumSave, SourceSave, ThemeColorsSave,
    ThemeFontsSave,
};

/// 每个 zip 条目的 `(CRC, 压缩后字节)`。
#[cfg(feature = "compat-ts")]
fn raw_entries(bytes: &[u8]) -> std::collections::BTreeMap<String, (u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index_raw(i).unwrap();
        let name = f.name().to_string();
        let crc = f.crc32();
        let mut raw = Vec::new();
        Read::read_to_end(&mut f, &mut raw).unwrap();
        out.insert(name, (crc, raw));
    }
    out
}

/// 新建了某个 part 之后，其他条目的原压缩数据不变（`SAVE-05`）。
#[cfg(feature = "compat-ts")]
fn assert_only_added(before: &[u8], after: &[u8], added: &[&str], touched: &[&str]) {
    let (a, b) = (raw_entries(before), raw_entries(after));
    for (name, x) in &a {
        if touched.contains(&name.as_str()) {
            continue;
        }
        let y = b.get(name).unwrap_or_else(|| panic!("{name} 不见了"));
        assert_eq!(x, y, "{name} 的压缩字节变了");
    }
    for name in added {
        assert!(b.contains_key(*name), "{name} 没建出来");
        assert!(!a.contains_key(*name), "{name} 本来就在");
    }
}

/// `themeFonts` / `themeColors`：只改 `@typeface` 与槽里的颜色；重解析后投影读得出来。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_theme_fonts_and_colors() {
    let bytes = corpus("watermark-theme-sources__008.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        theme_fonts: Some(ThemeFontsSave {
            major: "Calibri Light".into(),
            minor: "Calibri".into(),
            east_asia: Some("宋体".into()),
        }),
        theme_colors: Some(ThemeColorsSave {
            name: Some("我的配色".into()),
            slots: vec![("accent1".into(), "1F4E79".into()), ("dk2".into(), "112233".into())],
        }),
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    let theme = part_text(&saved, "word/theme/theme1.xml");
    assert_eq!(xpath_on(&theme, "//a:majorFont/a:latin/@typeface"), ["Calibri Light"], "{theme}");
    assert_eq!(xpath_on(&theme, "//a:minorFont/a:latin/@typeface"), ["Calibri"], "{theme}");
    assert_eq!(xpath_on(&theme, "//a:minorFont/a:ea/@typeface"), ["宋体"], "{theme}");
    assert_eq!(xpath_on(&theme, "//a:clrScheme/a:accent1/a:srgbClr/@val"), ["1F4E79"], "{theme}");
    assert_eq!(xpath_on(&theme, "//a:clrScheme/a:dk2/a:srgbClr/@val"), ["112233"], "{theme}");
    assert_eq!(xpath_on(&theme, "//a:clrScheme/@name"), ["我的配色"], "{theme}");
    // 重解析：投影里读得出来（这是主题 part 唯一的 oracle）
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["themeFonts"]["major"], "Calibri Light");
    assert_eq!(json["themeFonts"]["minor"], "Calibri");
    assert_eq!(json["themeColors"]["accent1"], "1F4E79");
    assert_eq!(json["themeColors"]["dk2"], "112233");
    // `document.xml` 一个字节都没动
    assert!(same_entry(&bytes, &saved, "word/document.xml"));
}

/// 没有 theme part 的文档：按 `SAVE-05` 从模板新建，其他条目原压缩数据不变。
#[test]
#[cfg(feature = "compat-ts")]
fn save_05_theme_part_created_from_template() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        theme_fonts: Some(ThemeFontsSave {
            major: "Georgia".into(),
            minor: "Verdana".into(),
            east_asia: None,
        }),
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    assert_only_added(
        &bytes,
        &saved,
        &["word/theme/theme1.xml"],
        &["[Content_Types].xml", "word/_rels/document.xml.rels"],
    );
    let theme = part_text(&saved, "word/theme/theme1.xml");
    assert_eq!(xpath_on(&theme, "//a:majorFont/a:latin/@typeface"), ["Georgia"], "{theme}");
    assert_eq!(xpath_on(&theme, "count(//a:fmtScheme)"), ["1"], "Word 要求 fmtScheme\n{theme}");
    let ct = part_text(&saved, "[Content_Types].xml");
    assert!(ct.contains("theme+xml"), "{ct}");
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["themeFonts"]["minor"], "Verdana");
}

/// `numbering`：只追加；`abstractNum` 在 `w:num` 之前，既有条目不动。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_numbering_appends_definitions() {
    let numbering = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>"#,
        r#"<w:num w:numId="6"><w:abstractNumId w:val="3"/></w:num>"#,
        r#"</w:numbering>"#
    );
    let doc_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rIdN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
        r#"</Relationships>"#
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr/>"#,
        &[("word/_rels/document.xml.rels", doc_rels), ("word/numbering.xml", numbering)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        numbering_new_defs: vec![
            NumberingDefSave { num_id: "9".into(), bullet: true, levels: Vec::new() },
            NumberingDefSave {
                num_id: "10".into(),
                bullet: false,
                levels: vec![NumberingLevelSave {
                    num_fmt: "upperRoman".into(),
                    lvl_text: "%1)".into(),
                    indent_left: 480,
                    hanging: Some(240),
                    start: Some(3),
                }],
            },
        ],
        numbering_restart_nums: vec![RestartNumSave {
            num_id: "11".into(),
            abstract_num_id: "3".into(),
            start_overrides: vec![(0, 1)],
        }],
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/numbering.xml");
    // 既有条目原字节不动
    assert!(xml.contains(r#"<w:abstractNum w:abstractNumId="3">"#), "{xml}");
    assert!(xml.contains(r#"<w:num w:numId="6"><w:abstractNumId w:val="3"/></w:num>"#), "{xml}");
    // 新号从最大值 +1 起
    assert_eq!(xpath_on(&xml, "//w:abstractNum/@w:abstractNumId"), ["3", "4", "5"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:num/@w:numId"), ["6", "9", "10", "11"], "{xml}");
    // 缺省级别：5 级，项目符号带 Symbol 字体
    assert_eq!(
        xpath_on(&xml, r#"count(//w:abstractNum[@w:abstractNumId="4"]/w:lvl)"#),
        ["5"],
        "{xml}"
    );
    assert_eq!(
        xpath_on(&xml, r#"//w:abstractNum[@w:abstractNumId="4"]/w:lvl[1]/w:rPr/w:rFonts/@w:ascii"#),
        ["Symbol"],
        "{xml}"
    );
    // 自定义级别
    assert_eq!(
        xpath_on(&xml, r#"//w:abstractNum[@w:abstractNumId="5"]/w:lvl[1]/w:numFmt/@w:val"#),
        ["upperRoman"],
        "{xml}"
    );
    assert_eq!(
        xpath_on(&xml, r#"//w:abstractNum[@w:abstractNumId="5"]/w:lvl[1]/w:start/@w:val"#),
        ["3"],
        "{xml}"
    );
    // `lvlOverride`
    assert_eq!(
        xpath_on(&xml, r#"//w:num[@w:numId="11"]/w:lvlOverride/w:startOverride/@w:val"#),
        ["1"],
        "{xml}"
    );
    // 所有 `w:abstractNum` 都在 `w:num` 之前（schema 顺序）
    let (last_abs, first_num) =
        (xml.rfind("<w:abstractNum ").unwrap(), xml.find("<w:num ").unwrap());
    assert!(last_abs < first_num, "abstractNum 要全在 num 之前\n{xml}");
    // 重解析 oracle：新的 numId 在投影里查得到
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    let defs = &json["numbering"];
    assert!(defs.get("9").is_some(), "numbering[9] 缺失: {defs}");
    // `numbering[numId].levels` 是按 ilvl 的对象（TS 形态），不是数组
    assert_eq!(defs["10"]["levels"]["0"]["numFmt"], "upperRoman", "{defs}");
    assert_eq!(defs["10"]["levels"]["0"]["start"], 3, "{defs}");
    assert_eq!(defs["9"]["levels"]["0"]["numFmt"], "bullet", "{defs}");
    assert_eq!(defs["11"]["startOverrides"]["0"], 1, "{defs}");
    assert!(same_entry(&bytes, &saved, "word/document.xml"));
}

/// `styleUpserts`：同 `styleId` 整条替换，否则追加；`rPr` / `pPr` 由属性表生成。
#[test]
fn save_07_style_upserts_replace_or_append() {
    use rsword::semantic::props::{Jc, ParaProps, RunProps, Spacing, Val};
    let bytes = corpus("write-protection__001.docx");
    let before = part_text(&bytes, "word/styles.xml");
    let existing = xpath_on(&before, "//w:style/@w:styleId");
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        style_upserts: vec![StyleUpsertSave {
            style_id: "MyQuote".into(),
            kind: "paragraph".into(),
            name: "我的引用".into(),
            based_on: Some("Normal".into()),
            run_props: Some(RunProps {
                italic: Some(true),
                size: Some(Val::Value(20)),
                ..Default::default()
            }),
            para_props: Some(ParaProps {
                jc: Some(Val::Value(Jc::Center)),
                spacing: Some(Spacing {
                    before: Some(Val::Value(120)),
                    after: Some(Val::Value(120)),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "word/styles.xml");
    assert_eq!(
        xpath_on(&xml, r#"//w:style[@w:styleId="MyQuote"]/w:name/@w:val"#),
        ["我的引用"],
        "{xml}"
    );
    assert_eq!(xpath_on(&xml, r#"//w:style[@w:styleId="MyQuote"]/@w:customStyle"#), ["1"], "{xml}");
    assert_eq!(
        xpath_on(&xml, r#"count(//w:style[@w:styleId="MyQuote"]/w:pPr/w:jc)"#),
        ["1"],
        "{xml}"
    );
    assert_eq!(
        xpath_on(&xml, r#"//w:style[@w:styleId="MyQuote"]/w:rPr/w:sz/@w:val"#),
        ["20"],
        "{xml}"
    );
    // `w:pPr` 在 `w:rPr` 之前（CT_Style 顺序）
    let one = xml.find(r#"w:styleId="MyQuote""#).unwrap();
    let tail = &xml[one..];
    assert!(tail.find("<w:pPr>").unwrap() < tail.find("<w:rPr>").unwrap(), "{xml}");
    // 既有样式一条不少
    let after = xpath_on(&xml, "//w:style/@w:styleId");
    assert_eq!(after.len(), existing.len() + 1, "只多了一条");

    // 再 upsert 同一个 id：整条替换，不是追加
    let mut s2 = EditSession::open(&saved).unwrap();
    let again = SaveOptions {
        style_upserts: vec![StyleUpsertSave {
            style_id: "MyQuote".into(),
            kind: "paragraph".into(),
            name: "改名了".into(),
            based_on: None,
            run_props: None,
            para_props: None,
        }],
        ..Default::default()
    };
    let x2 = part_text(&s2.save_with_compat(&again).unwrap(), "word/styles.xml");
    assert_eq!(xpath_on(&x2, r#"count(//w:style[@w:styleId="MyQuote"])"#), ["1"], "还是一条\n{x2}");
    assert_eq!(
        xpath_on(&x2, r#"//w:style[@w:styleId="MyQuote"]/w:name/@w:val"#),
        ["改名了"],
        "{x2}"
    );
    assert_eq!(
        xpath_on(&x2, r#"count(//w:style[@w:styleId="MyQuote"]/w:rPr)"#),
        ["0"],
        "整条替换\n{x2}"
    );
}

/// `sources`：权威列表——未变的条目原字节不动（未建模的域因此保住）、变了的重建、列表外的删掉。
#[test]
#[cfg(feature = "compat-ts")]
fn save_07_sources_authoritative_list() {
    let bytes = corpus("watermark-theme-sources__008.docx");
    let mut s = EditSession::open(&bytes).unwrap();
    // 现有条目 Zhao2022 原样保留 + 新增 Wang2024
    let keep = SourceSave {
        tag: "Zhao2022".into(),
        kind: "JournalArticle".into(),
        author: "赵, 一".into(),
        title: "大模型对齐".into(),
        year: "2022".into(),
        publisher: Some("软件学报".into()),
        url: None,
    };
    let add = SourceSave {
        tag: "Wang2024".into(),
        kind: "Book".into(),
        author: "王明".into(),
        title: "深度学习实践".into(),
        year: "2024".into(),
        publisher: Some("清华出版社".into()),
        url: None,
    };
    let opts = SaveOptions { sources: Some(vec![keep.clone(), add]), ..Default::default() };
    let saved = s.save_with_compat(&opts).unwrap();
    let xml = part_text(&saved, "customXml/item1.xml");
    // 未变的条目原字节不动：未建模的 `b:Volume` / `b:Pages` 还在
    assert!(xml.contains("<b:Volume>33</b:Volume>"), "未建模的域要保住\n{xml}");
    assert!(xml.contains("<b:Pages>1-20</b:Pages>"), "{xml}");
    // 新条目：团体名之外的作者拆成 Last / First，Book 用 `b:Publisher`
    assert_eq!(xpath_on(&xml, "count(//b:Source)"), ["2"], "{xml}");
    assert_eq!(xpath_on(&xml, "//b:Source/b:Publisher"), ["清华出版社"], "{xml}");
    assert_eq!(
        xpath_on(&xml, "//b:Person/b:Last"),
        ["赵", "钱", "王明"],
        "没有逗号就整串当 Last\n{xml}"
    );
    // 重解析 oracle
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    let list = json["sources"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{json:#}");
    assert_eq!(list[1]["tag"], "Wang2024");
    assert_eq!(list[1]["publisher"], "清华出版社");

    // 列表外的删掉；改了字段的重建
    let mut s2 = EditSession::open(&saved).unwrap();
    let edited = SourceSave { year: "2023".into(), ..keep };
    let shrink = SaveOptions { sources: Some(vec![edited]), ..Default::default() };
    let x2 = part_text(&s2.save_with_compat(&shrink).unwrap(), "customXml/item1.xml");
    assert_eq!(xpath_on(&x2, "count(//b:Source)"), ["1"], "{x2}");
    assert_eq!(xpath_on(&x2, "//b:Source/b:Year"), ["2023"], "{x2}");
    assert_eq!(
        xpath_on(&x2, "count(//b:Volume)"),
        ["0"],
        "改过的条目整条重建，未建模的域随之丢掉\n{x2}"
    );
    let mut pkg2 = Package::open(&s2.save_with_compat(&shrink).unwrap()).unwrap();
    let json2 = rsword::bind::compat_ts::parsed_doc(&mut pkg2).unwrap();
    assert_eq!(json2["sources"].as_array().unwrap().len(), 1);
}

/// 没有 customXml 的文档：按 `SAVE-05` 建 `item{N}.xml` + `itemProps{N}.xml` + 两条关系。
#[test]
#[cfg(feature = "compat-ts")]
fn save_05_sources_part_created_with_item_props() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions {
        sources: Some(vec![SourceSave {
            tag: "Li2025".into(),
            kind: "InternetSite".into(),
            author: "李, 四".into(),
            title: "在线资料".into(),
            year: "2025".into(),
            publisher: Some("某站".into()),
            url: Some("https://example.com".into()),
        }]),
        ..Default::default()
    };
    let saved = s.save_with_compat(&opts).unwrap();
    assert_only_added(
        &bytes,
        &saved,
        &["customXml/item1.xml", "customXml/itemProps1.xml", "customXml/_rels/item1.xml.rels"],
        &["[Content_Types].xml", "word/_rels/document.xml.rels"],
    );
    let item = part_text(&saved, "customXml/item1.xml");
    assert_eq!(xpath_on(&item, "//b:Source/b:Tag"), ["Li2025"], "{item}");
    assert_eq!(xpath_on(&item, "//b:Person/b:Last"), ["李"], "{item}");
    assert_eq!(xpath_on(&item, "//b:Person/b:First"), ["四"], "{item}");
    // InternetSite 的出版方字段是 `b:InternetSiteTitle`
    assert_eq!(xpath_on(&item, "//b:Source/b:InternetSiteTitle"), ["某站"], "{item}");
    assert_eq!(xpath_on(&item, "//b:Source/b:URL"), ["https://example.com"], "{item}");
    let props = part_text(&saved, "customXml/itemProps1.xml");
    assert!(props.contains("datastoreItem") && props.contains("bibliography"), "{props}");
    let rels = part_text(&saved, "customXml/_rels/item1.xml.rels");
    assert!(rels.contains("itemProps1.xml"), "{rels}");
    let ct = part_text(&saved, "[Content_Types].xml");
    assert!(ct.contains("customXmlProperties+xml"), "{ct}");
    // 重解析 oracle
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["sources"][0]["tag"], "Li2025");
    assert_eq!(json["sources"][0]["author"], "李, 四");
    assert_eq!(json["sources"][0]["url"], "https://example.com");
}
