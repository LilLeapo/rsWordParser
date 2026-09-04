//! `SAVE-01` 编排与 `SAVE-07` 保存选项（任务 1.14 第二批）：`save_with(opts)` 的六步流程、
//! 不变式 1 的短路条件、`saved_at` 只改 `core.xml`、`remove_personal_info` 的全包清洗与
//! `settings.xml` 标志。验收清单 `SAVE-07`：清洗后没有 `w:author` 不是 `Author` 的修订。

mod common;

use std::io::{Cursor, Read};

use rsword::diag::DiagCode;
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
    // remove_personal_info 是强制选项：进入流程，但这份语料没有 settings.xml 也没有作者信息，
    // 于是没有任何 part 变脏，写回仍是原字节（并记一条缺 settings.xml 的诊断）
    let forced = SaveOptions { remove_personal_info: Some(false), ..Default::default() };
    assert!(forced.forces_save());
    assert!(s.save_with(&forced).unwrap() == bytes, "无可改动时仍是原字节");
    assert!(
        s.diagnostics()
            .iter()
            .any(|d| d.code == DiagCode::EditUnsupported && d.message.contains("settings.xml")),
        "缺 settings.xml 应记诊断"
    );
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
    // 这份语料没有 settings.xml：标志写不进去，记诊断（新建 part 属 SAVE-05）
    assert!(
        s.diagnostics()
            .iter()
            .any(|d| d.code == DiagCode::EditUnsupported && d.message.contains("settings.xml")),
        "缺 settings.xml 应记诊断"
    );
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
