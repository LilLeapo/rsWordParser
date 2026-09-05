//! `SAVE-01` 编排与 `SAVE-07` 保存选项（任务 1.14 第二批）：`save_with(opts)` 的六步流程、
//! 不变式 1 的短路条件、`saved_at` 只改 `core.xml`、`remove_personal_info` 的全包清洗与
//! `settings.xml` 标志、`remove_date_and_time`（OOXML 有、TS 没有的一项）。
//! 验收清单 `SAVE-07`：清洗后没有 `w:author` 不是 `Author` 的修订。

mod common;

use std::io::{Cursor, Read};

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::package::Package;
use rsword::save::SaveOptions;
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
    assert_eq!(s.save_with(&stamped).unwrap(), bytes, "saved_at 不强制保存");
    // remove_personal_info 是强制选项：进入流程，但这份语料没有 settings.xml 也没有作者信息。
    // 写 `false` 不需要 part（标志缺失就等于 false），所以没有任何 part 变脏，仍是原字节
    let forced = SaveOptions { remove_personal_info: Some(false), ..Default::default() };
    assert!(forced.forces_save());
    assert!(s.save_with(&forced).unwrap() == bytes, "无可改动时仍是原字节");
    assert!(s.diagnostics().is_empty(), "没什么可诊断的: {:?}", s.diagnostics());
    // 有 settings.xml 的文档：强制选项确实产生输出
    let other = corpus("revisions__013.docx");
    let mut s2 = EditSession::open(&other).unwrap();
    assert!(s2.save().unwrap() == other, "未编辑保存字节相同");
    let on = SaveOptions { remove_personal_info: Some(true), ..Default::default() };
    assert!(s2.save_with(&on).unwrap() != other, "写入标志后输出不同");
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
    let saved = s.save_with(&opts).unwrap();
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
    let saved = s.save_with(&opts).unwrap();

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
    let saved = s.save_with(&SaveOptions::default()).unwrap();
    let doc = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&doc, "//w:ins/@w:author"), ["Author"], "文档标志触发清洗");
    assert!(same_entry(&bytes, &saved, "word/settings.xml"), "没要求改标志就不动 settings");

    let mut s2 = EditSession::open(&bytes).unwrap();
    let off = SaveOptions { remove_personal_info: Some(false), ..Default::default() };
    let saved2 = s2.save_with(&off).unwrap();
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
    let saved = s.save_with(&on).unwrap();
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
    let saved = s.save_with(&opts).unwrap();
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
    let saved2 = s2.save_with(&SaveOptions::default()).unwrap();
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
    let saved3 = s3.save_with(&both).unwrap();
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
    let saved = s.save_with(&opts).unwrap();
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
    let saved = s.save_with(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "//w:pgSz/@w:w"), ["11906"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgSz/@w:orient"), ["landscape"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:top"), ["1440"], "{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:gutter"), ["77"], "没给出的属性沿用原值\n{xml}");
    assert_eq!(xpath_on(&xml, "//w:pgMar/@w:header"), ["55"], "headerDist 缺省沿用\n{xml}");
    // 纵向不写 w:orient（同 TS）
    let mut s2 = EditSession::open(&bytes).unwrap();
    let portrait = SaveOptions { section: Some(a4()), ..Default::default() };
    let xml2 = part_text(&s2.save_with(&portrait).unwrap(), "word/document.xml");
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
    let xml = part_text(&s.save_with(&opts).unwrap(), "word/document.xml");
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
    let x2 = part_text(&s2.save_with(&uneven).unwrap(), "word/document.xml");
    assert_eq!(xpath_on(&x2, "//w:cols/@w:equalWidth"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "//w:cols/w:col/@w:w"), ["4000", "5000"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:cols/w:col/@w:space)"), ["1"], "{x2}");
    // 关掉边框：元素删掉
    let bordered = s.save_with(&opts).unwrap();
    let mut s3 = EditSession::open(&bordered).unwrap();
    let off = SaveOptions { section: Some(a4()), ..Default::default() };
    let x3 = part_text(&s3.save_with(&off).unwrap(), "word/document.xml");
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
    let xml = part_text(&s.save_with(&opts).unwrap(), "word/document.xml");
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
    let mut s2 = EditSession::open(&s.save_with(&opts).unwrap()).unwrap();
    let clear = SaveOptions {
        section_start_type: Some(SectType::NextPage),
        pg_num_type: Some(PgNumTypeOption::default()),
        title_pg: Some(false),
        ..Default::default()
    };
    let x2 = part_text(&s2.save_with(&clear).unwrap(), "word/document.xml");
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
    let saved = s.save_with(&opts).unwrap();
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
    let saved = s.save_with(&opts).unwrap();
    let xml = part_text(&saved, "word/document.xml");
    assert_eq!(xpath_on(&xml, "/w:document/w:background/@w:color"), ["FFF2CC"], "{xml}");
    let settings = part_text(&saved, "word/settings.xml");
    assert_eq!(xpath_on(&settings, "count(//w:displayBackgroundShape)"), ["1"], "{settings}");
    // 删底色：`w:background` 走了，开关留着（同 TS）
    let mut s2 = EditSession::open(&saved).unwrap();
    let clear = SaveOptions { page_color: Some(None), ..Default::default() };
    let cleared = s2.save_with(&clear).unwrap();
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
    let settings = part_text(&s.save_with(&opts).unwrap(), "word/settings.xml");
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
    let mut s2 = EditSession::open(&s.save_with(&opts).unwrap()).unwrap();
    let clear =
        SaveOptions { protection: Some(None), write_protection: Some(None), ..Default::default() };
    let x2 = part_text(&s2.save_with(&clear).unwrap(), "word/settings.xml");
    assert_eq!(xpath_on(&x2, "count(//w:documentProtection)"), ["0"], "{x2}");
    assert_eq!(xpath_on(&x2, "count(//w:writeProtection)"), ["0"], "{x2}");
    // 既不 recommended 也没有口令 → 等于删除（同 TS）
    let mut s3 = EditSession::open(&bytes).unwrap();
    let empty = SaveOptions {
        write_protection: Some(Some(WriteProtectionOption::default())),
        ..Default::default()
    };
    let x3 = part_text(&s3.save_with(&empty).unwrap(), "word/settings.xml");
    assert_eq!(xpath_on(&x3, "count(//w:writeProtection)"), ["0"], "{x3}");
}

/// `evenAndOddHeaders`：写入后重解析，投影里读得出来（重解析 oracle）。
#[test]
fn save_07_even_and_odd_headers_round_trips_through_the_projection() {
    let bytes = sect_docx(r#"<w:sectPr/>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let opts = SaveOptions { even_and_odd_headers: Some(true), ..Default::default() };
    let saved = s.save_with(&opts).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap();
    assert_eq!(json["evenAndOddHeaders"], true);
    let mut s2 = EditSession::open(&saved).unwrap();
    let off = SaveOptions { even_and_odd_headers: Some(false), ..Default::default() };
    let cleared = s2.save_with(&off).unwrap();
    let mut pkg2 = Package::open(&cleared).unwrap();
    let json2 = rsword::bind::compat_ts::parsed_doc(&mut pkg2).unwrap();
    assert_eq!(json2["evenAndOddHeaders"], false);
}
