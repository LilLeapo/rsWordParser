//! 块字段生成器（`FLD-09`，`spec/18` 7.8）：`ts_shape` 模式下与 TS 的输出逐字相同。
//!
//! 对照件 `fixtures/fieldgen/generators.json` 由 `tools/export-golden/blankgen.export.test.ts`
//! 导出（TS `generateTocFieldXml` / `generateCaptionXml` / `generateIndexFieldXml`）。

mod common;

#[cfg(feature = "compat-ts")]
use rsword::span::field::generate::index::Collation;
use rsword::span::field::generate::index::{self, IndexOptions};
#[cfg(feature = "compat-ts")]
use rsword::span::field::generate::seq;
use rsword::span::field::generate::toc::{self, TocEntry, TocOptions};

#[cfg(feature = "compat-ts")]
fn fixture() -> serde_json::Value {
    let path = common::repo_root().join("fixtures/fieldgen/generators.json");
    serde_json::from_slice(&std::fs::read(&path).expect("generators.json")).expect("JSON")
}

const TEXT_OPEN: &str = r#"<w:t xml:space="preserve">"#;

#[cfg(feature = "compat-ts")]
fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array().expect("数组").iter().map(|x| x.as_str().expect("字符串").to_string()).collect()
}

/// TOC：三组条目，`ts_shape` 的输出与 TS 逐字相同。
#[test]
#[cfg(feature = "compat-ts")]
fn toc_ts_shape_matches_the_ts_generator() {
    let want = fixture();
    let cases = want["toc"].as_array().expect("toc");
    assert_eq!(cases.len(), 3);
    for case in cases {
        let entries: Vec<TocEntry> = case["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|e| TocEntry {
                level: e["level"].as_u64().expect("level") as u8,
                text: e["text"].as_str().expect("text").to_string(),
                page_no: e["pageNo"].as_u64().map(|n| n as u32),
                bookmark: None,
            })
            .collect();
        let opts = TocOptions { ts_shape: true, ..Default::default() };
        assert_eq!(toc::generate(&entries, &opts), strings(&case["xml"]), "{entries:?}");
    }
    assert!(toc::generate(&[], &TocOptions { ts_shape: true, ..Default::default() }).is_empty());
    assert!(strings(&want["tocEmpty"]).is_empty(), "TS 对空条目也不生成");
}

/// SEQ 题注：两组，与 TS 逐字相同（第二组 `text` 为空）。
#[test]
#[cfg(feature = "compat-ts")]
fn caption_matches_the_ts_generator() {
    let want = fixture();
    let cases = want["caption"].as_array().expect("caption");
    assert_eq!(cases.len(), 2);
    for case in cases {
        let got = seq::caption(
            case["label"].as_str().expect("label"),
            case["number"].as_u64().expect("number") as u32,
            case["text"].as_str().expect("text"),
            true,
        );
        assert_eq!(got, case["xml"].as_str().expect("xml"));
    }
}

/// INDEX：`ts_shape` 的骨架与 TS 相同。**排序不同**：TS 用 `localeCompare('zh-CN')`（ICU），
/// 我们用码位序（`docs/04` §8），所以逐段比之前先把 TS 那份的次序当成 `Collation::Given` 喂进去。
#[test]
#[cfg(feature = "compat-ts")]
fn index_ts_shape_matches_the_ts_generator_given_its_order() {
    let want = fixture();
    let cases = want["index"].as_array().expect("index");
    assert_eq!(cases.len(), 2);
    for case in cases {
        let terms: Vec<String> = strings(&case["terms"]);
        let ts = strings(&case["xml"]);
        // TS 的次序：从它每段的 `<w:t xml:space="preserve">…</w:t>` 里读回来
        let order: Vec<String> = ts
            .iter()
            .map(|p| {
                let i = p.find(TEXT_OPEN).expect("文字 run") + TEXT_OPEN.len();
                p[i..].split("</w:t>").next().unwrap_or_default().to_string()
            })
            .collect();
        let opts = IndexOptions {
            ts_shape: true,
            collation: Collation::Given(order),
            ..Default::default()
        };
        assert_eq!(index::generate(&terms, &opts), ts, "{terms:?}");
    }
    assert!(strings(&want["indexEmpty"]).is_empty());
    assert!(
        index::generate(
            &["".into(), "  ".into()],
            &IndexOptions { ts_shape: true, ..Default::default() }
        )
        .is_empty()
    );
}

/// 码位序是我们的缺省：去重、trim、按码位排。
#[test]
fn index_default_collation_is_code_point_order() {
    let terms: Vec<String> = ["banana", " apple ", "Apple", "", "apple", "中文", "Ähnlich"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let out = index::generate(&terms, &IndexOptions::default());
    let words: Vec<String> = out
        .iter()
        .map(|p| {
            let i = p.find(TEXT_OPEN).expect("文字 run") + TEXT_OPEN.len();
            p[i..].split("</w:t>").next().unwrap_or_default().to_string()
        })
        .collect();
    // A(0x41) < a(0x61) < b(0x62) < Ä(0xC4) < 中(0x4E2D)
    assert_eq!(words, ["Apple", "apple", "banana", "Ähnlich", "中文"]);
}

/// Word 形态（缺省）：`\h` 时每条包 `w:hyperlink w:anchor`、页码走 `PAGEREF … \h` 字段、
/// 制表位按调用方给的版心宽。
#[test]
fn toc_word_shape_emits_hyperlinks_and_pageref_fields() {
    let entries = [
        TocEntry {
            level: 1,
            text: "第一章".into(),
            page_no: Some(1),
            bookmark: Some("_Toc000000001".into()),
        },
        TocEntry {
            level: 2,
            text: "第一节".into(),
            page_no: Some(4),
            bookmark: Some("_Toc000000002".into()),
        },
    ];
    let opts = TocOptions { levels: (1, 3), tab_pos: Some(9026), ..Default::default() };
    let out = toc::generate(&entries, &opts);
    assert_eq!(out.len(), 2);
    assert!(out[0].contains(r#"<w:tab w:val="right" w:leader="dot" w:pos="9026"/>"#));
    assert!(out[0].contains(r#" TOC \o "1-3" \h \u "#), "{}", out[0]);
    assert!(out[0].contains(r#"<w:fldChar w:fldCharType="begin" w:dirty="true"/>"#));
    for (i, e) in entries.iter().enumerate() {
        let b = e.bookmark.as_deref().unwrap();
        assert!(out[i].contains(&format!(r#"<w:hyperlink w:anchor="{b}">"#)), "{}", out[i]);
        assert!(out[i].contains(&format!(" PAGEREF {b} \\h ")), "{}", out[i]);
    }
    assert!(out[1].ends_with(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#));
    // 首段末尾是超链接的收尾，不是字段的 end（`PAGEREF` 自己的 end 在超链接里面）
    assert!(out[0].ends_with("</w:hyperlink></w:p>"), "{}", out[0]);
    // `\n`：一个页码都不写
    let out = toc::generate(&entries, &TocOptions { no_page_numbers: true, ..opts.clone() });
    assert!(!out[0].contains("PAGEREF"));
    assert!(out[0].contains(r#" \n "#), "{}", out[0]);
}

// ---------------------------------------------------------------- 从文档重算

use rsword::edit::{
    BlockAt, BlockFieldOptions, BlockPos, EditContext, EditOp, EditSession, NewBlock,
    NewBlockField, NewInline, NewRun,
};
use rsword::span::field::Keyword;
use rsword::xml::{LocalName, NodeId, QName};

/// 主 part 里第一个 `TOC` 字段。
fn toc_field(s: &EditSession) -> rsword::span::FieldId {
    s.document()
        .fields_in(s.document().main_part)
        .expect("字段索引")
        .fields()
        .iter()
        .find(|f| f.instr.keyword == Keyword::Toc)
        .expect("有 TOC 字段")
        .id
}

fn heading(level: u8, text: &str) -> NewBlock {
    NewBlock::Paragraph {
        props: Some(
            rsword::xml::NewElement::new(QName::w(LocalName::PPr)).with_child(
                rsword::xml::NewElement::new(QName::w(LocalName::PStyle))
                    .with_attr(QName::w(LocalName::Val), format!("Heading{level}")),
            ),
        ),
        inlines: vec![NewInline::Run(NewRun::text(text))],
    }
}

/// 在空白模板上堆三级标题，返回会话与最后一段。
fn doc_with_headings() -> (EditSession, NodeId) {
    let mut s = EditSession::blank(None).expect("blank");
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let mut at =
        dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::P))).expect("空段");
    for (level, text) in [(1u8, "第一章"), (2, "第一节"), (3, "小节 & 细则"), (1, "第二章")]
    {
        let r = s
            .apply(
                EditOp::InsertBlock {
                    at: BlockPos { part: None, at: BlockAt::After(at) },
                    block: heading(level, text),
                },
                &EditContext::default(),
            )
            .expect("插标题");
        at = r.created.iter().flatten().copied().next().expect("新段");
    }
    (s, at)
}

/// `NewBlock::Field(Toc)`：条目按文档里的标题生成，每条一段，`\h` 时挂 `_Toc` 书签 +
/// `w:hyperlink w:anchor`，`PAGEREF` 指向同一个书签。
#[test]
fn inserting_a_toc_field_generates_one_paragraph_per_heading() {
    let (mut s, at) = doc_with_headings();
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos { part: None, at: BlockAt::After(at) },
            block: NewBlock::Field(NewBlockField::Toc { opts: Box::default(), pages: None }),
        },
        &EditContext::default(),
    )
    .expect("插目录");
    let out = s.save().expect("save");
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            // 四条标题 → 四段目录
            ("count(//w:pStyle[@w:val='TOC1'])", ["2"]),
            ("count(//w:pStyle[@w:val='TOC2'])", ["1"]),
            ("count(//w:pStyle[@w:val='TOC3'])", ["1"]),
            ("count(//w:hyperlink)", ["4"]),
            // 字段结构只有一份：begin + separate 在首段、end 在末段
            ("count(//w:fldChar[@w:fldCharType='begin'])", ["1"]),
            ("count(//w:fldChar[@w:fldCharType='end'])", ["1"]),
            ("count(//w:fldChar[@w:fldCharType='begin'][@w:dirty='true'])", ["1"]),
            // 四个隐藏书签，成对
            ("count(//w:bookmarkStart)", ["4"]),
            ("count(//w:bookmarkEnd)", ["4"]),
        ]
    );
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert!(xml.contains(r#" TOC \o "1-9" \h \u "#), "指令: {xml}");
    // 每条超链接指向一个存在的书签
    for anchor in anchors(&xml) {
        assert!(
            xml.contains(&format!(r#"w:name="{anchor}""#)),
            "{anchor} 没有对应的 bookmarkStart"
        );
    }
    // 文字与级别对得上（`&` 要转义）
    for (style, text) in
        [("TOC1", "第一章"), ("TOC2", "第一节"), ("TOC3", "小节 &amp; 细则"), ("TOC1", "第二章")]
    {
        assert!(xml.contains(&format!(r#"<w:pStyle w:val="{style}"/>"#)), "{style}");
        assert!(xml.contains(&format!(">{text}</w:t>")), "{text}");
    }
    // 没给页码 → 一个 PAGEREF 都不写
    assert!(!xml.contains("PAGEREF"));
}

fn anchors(xml: &str) -> Vec<String> {
    xml.match_indices(r#"<w:hyperlink w:anchor=""#)
        .map(|(i, m)| xml[i + m.len()..].split('"').next().unwrap_or_default().to_string())
        .collect()
}

/// `RegenerateBlockField`：重算已有的目录，页码由调用方给，书签不重铸。
#[test]
fn regenerating_a_toc_reuses_its_bookmarks_and_writes_pagerefs() {
    let (mut s, at) = doc_with_headings();
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos { part: None, at: BlockAt::After(at) },
            block: NewBlock::Field(NewBlockField::Toc { opts: Box::default(), pages: None }),
        },
        &EditContext::default(),
    )
    .expect("插目录");
    let first =
        String::from_utf8_lossy(&common::part_bytes(&s.save().expect("save"), "word/document.xml"))
            .to_string();
    let names = anchors(&first);
    assert_eq!(names.len(), 4);

    // 页码：四个标题段落各给一个
    let pages: std::collections::HashMap<NodeId, u32> = {
        let doc = s.document();
        doc.blocks()
            .filter_map(rsword::model::Block::as_text)
            .filter(|tb| tb.style_id.as_deref().is_some_and(|i| i.starts_with("Heading")))
            .zip([1u32, 2, 3, 5])
            .map(|(tb, p)| (tb.node, p))
            .collect()
    };
    assert_eq!(pages.len(), 4);
    let field = toc_field(&s);
    s.apply(
        EditOp::RegenerateBlockField {
            field,
            options: BlockFieldOptions::Auto { pages: Some(pages) },
        },
        &EditContext::default().with_mark_updated_fields_dirty(true),
    )
    .expect("重算");
    let out = s.save().expect("save");
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert_eq!(anchors(&xml), names, "书签不重铸");
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:bookmarkStart)", ["4"]),
            ("count(//w:hyperlink)", ["4"]),
            // 目录字段自己的 begin 打上 dirty；四个 PAGEREF 不打（Word 也不打）
            ("count(//w:fldChar[@w:fldCharType='begin'][@w:dirty='true'])", ["1"]),
            ("count(//w:fldChar[@w:fldCharType='begin'])", ["5"]),
        ]
    );
    for (n, page) in names.iter().zip([1, 2, 3, 5]) {
        assert!(xml.contains(&format!(" PAGEREF {n} \\h ")), "{n} 的 PAGEREF");
        assert!(xml.contains(&format!(">{page}</w:t>")), "页码 {page}");
    }
}

/// `tocLine` 的八份语料：重算之后字段还是完整的（begin / separate / end 各一份），
/// 保存能通过校验。这些文档里一个标题都没有，所以条目为空——正是「与文档标题一致」。
#[test]
fn regenerating_the_toc_corpus_keeps_every_field_well_formed() {
    let mut seen = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).expect("语料");
        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let Ok(mut s) = EditSession::open(&bytes) else { continue };
        let Some(idx) = s.document().fields_in(s.document().main_part) else { continue };
        let tocs: Vec<rsword::span::FieldId> = idx
            .fields()
            .iter()
            .filter(|f| f.instr.keyword == Keyword::Toc)
            .filter(|f| {
                matches!(f.form, rsword::span::FieldForm::Complex { separate: Some(_), .. })
            })
            .map(|f| f.id)
            .collect();
        if tocs.is_empty() {
            continue;
        }
        seen += 1;
        let before = String::from_utf8_lossy(&common::part_bytes(&bytes, "word/document.xml"))
            .matches("fldCharType=\"begin\"")
            .count();
        for field in tocs {
            s.apply(
                EditOp::RegenerateBlockField { field, options: Default::default() },
                &EditContext::default().with_mark_updated_fields_dirty(true),
            )
            .unwrap_or_else(|e| panic!("{name}: 重算失败 {e}"));
        }
        let out = s.save().unwrap_or_else(|e| panic!("{name}: 保存失败 {e}"));
        let out_bytes = common::part_bytes(&out, "word/document.xml");
        let xml = String::from_utf8_lossy(&out_bytes);
        assert_eq!(
            xml.matches("fldCharType=\"begin\"").count(),
            before,
            "{name}: 字段结构 run 的个数不该变"
        );
        assert_eq!(
            xml.matches("fldCharType=\"begin\"").count(),
            xml.matches("fldCharType=\"end\"").count(),
            "{name}: begin 与 end 成对"
        );
        assert!(xml.contains(r#"w:dirty="true""#), "{name}: begin 打上了 dirty");
        // 重开一遍：字段索引还认得它，没有新缺陷
        let s2 = EditSession::open(&out).unwrap_or_else(|e| panic!("{name}: 重开失败 {e}"));
        assert!(
            s2.document()
                .fields_in(s2.document().main_part)
                .is_some_and(|i| i.fields().iter().any(|f| f.instr.keyword == Keyword::Toc)),
            "{name}: 重开后还有 TOC 字段"
        );
    }
    // 八份带 `tocLine` 投影的语料里只有四份有**完整**的 TOC 字段（另外几份是显示层的
    // 对照件，begin 之后没有 end），能重算的就是这四份
    assert_eq!(seen, 4, "带完整 TOC 字段的语料");
}

/// `NewBlock::Caption`：编号 = 位置之前同标签的 `SEQ` 数 + 1。
#[test]
fn captions_number_themselves_from_the_preceding_seq_fields() {
    let mut s = EditSession::blank(None).expect("blank");
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let mut at =
        dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::P))).expect("空段");
    for (label, text) in [("图", "第一张"), ("图", "第二张"), ("表", "第一张表")] {
        let r = s
            .apply(
                EditOp::InsertBlock {
                    at: BlockPos { part: None, at: BlockAt::After(at) },
                    block: NewBlock::Caption { label: label.into(), text: text.into() },
                },
                &EditContext::default(),
            )
            .expect("插题注");
        at = r.created.iter().flatten().copied().next().expect("新段");
    }
    let out = s.save().expect("save");
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    // 图 1、图 2、表 1
    for needle in [r#" SEQ 图 \* ARABIC "#, r#" SEQ 表 \* ARABIC "#] {
        assert!(xml.contains(needle), "{needle}");
    }
    // 编号 run 就在 `separate` 之后：取每段 separate 与 end 之间那个 `w:t`
    let numbers: Vec<&str> = xml
        .match_indices(r#"<w:fldChar w:fldCharType="separate"/>"#)
        .filter_map(|(i, _)| {
            let tail = &xml[i..];
            let j = tail.find(TEXT_OPEN)? + TEXT_OPEN.len();
            tail[j..].split("</w:t>").next()
        })
        .collect();
    assert_eq!(numbers, ["1", "2", "1"], "图 1 / 图 2 / 表 1");
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:fldChar[@w:fldCharType='begin'][@w:dirty='true'])", ["3"])]
    );
}
