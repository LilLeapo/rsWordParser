//! 内容控件验收（`spec/06` MOD-08、`spec/08` EDIT-03 的 sdt 策略，任务 3.3）：
//! `sdtPr` 的全部建模字段、控件种类的命名空间无关判定、锁与数据绑定对编辑的拒绝（状态不变）。

mod common;

use rsword::DiagCode;
use rsword::edit::{EditOp, EditSession, InlinePos};
use rsword::model::{Block, Document, SdtControl, SdtLock, SdtRefusal, refusing_sdt};
use rsword::package::{PartId, Rels};
use rsword::xml::{Dom, LocalName, QName};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

fn doc(body: &str) -> Dom {
    let xml = format!(
        r#"<w:document xmlns:w="{W}" xmlns:w14="{W14}" xmlns:w15="{W15}"><w:body>{body}</w:body></w:document>"#
    );
    Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{xml}"))
}

/// 一个包着空段落的 sdt，`pr` 是 `w:sdtPr` 的内容。
fn sdt_body(pr: &str) -> String {
    format!(
        r#"<w:sdt><w:sdtPr>{pr}</w:sdtPr><w:sdtContent><w:p><w:r><w:t>x</w:t></w:r></w:p></w:sdtContent></w:sdt>"#
    )
}

/// 建模并取出块上的 `SdtInfo`。
fn info_of(pr: &str) -> rsword::model::SdtInfo {
    let d = doc(&sdt_body(pr));
    let (blocks, _) = rsword::model::Document::build_main(&d, None, &Rels::default());
    blocks[0].sdt().expect("块带 SdtInfo").clone()
}

#[test]
fn mod_08_acceptance_data_binding_and_content_lock() {
    // `spec/06` 验收行：带 `w:dataBinding` 与 `w:lock w:val="sdtContentLocked"` 的 sdt 字段正确
    let i = info_of(
        r#"<w:alias w:val="客户名称"/><w:tag w:val="customer"/><w:id w:val="-1234567"/>
           <w:lock w:val="sdtContentLocked"/>
           <w:placeholder><w:docPart w:val="DefaultPlaceholder_1081868574"/></w:placeholder>
           <w:showingPlcHdr/>
           <w:dataBinding w:prefixMappings="xmlns:ns0='urn:x'" w:xpath="/ns0:root[1]/ns0:name[1]" w:storeItemID="{A1B2}"/>
           <w:text/>"#,
    );
    assert_eq!(i.alias.as_deref(), Some("客户名称"));
    assert_eq!(i.tag.as_deref(), Some("customer"));
    assert_eq!(i.id, Some(-1_234_567));
    assert_eq!(i.lock, SdtLock::SdtContentLocked);
    assert!(i.lock.content_locked() && i.lock.sdt_locked());
    assert_eq!(i.placeholder.as_deref(), Some("DefaultPlaceholder_1081868574"));
    assert!(i.showing_placeholder);
    let b = i.data_binding.as_ref().expect("dataBinding");
    assert_eq!(b.xpath.as_deref(), Some("/ns0:root[1]/ns0:name[1]"));
    assert_eq!(b.store_item_id.as_deref(), Some("{A1B2}"));
    assert_eq!(b.prefix_mappings.as_deref(), Some("xmlns:ns0='urn:x'"));
    assert_eq!(i.control, SdtControl::PlainText);
    // 两条拒绝理由同时成立时先报锁
    assert_eq!(i.refusal(), Some(SdtRefusal::Locked));
}

#[test]
fn mod_08_control_kinds_ignore_the_prefix() {
    let kinds = [
        ("<w:richText/>", rsword::model::SdtControl::RichText),
        ("<w:text/>", rsword::model::SdtControl::PlainText),
        ("<w:picture/>", rsword::model::SdtControl::Picture),
        ("<w:comboBox/>", rsword::model::SdtControl::ComboBox),
        ("<w:dropDownList/>", rsword::model::SdtControl::DropDownList),
        (
            r#"<w:date w:fullDate="2026-01-01T00:00:00Z"><w:dateFormat w:val="yyyy"/></w:date>"#,
            rsword::model::SdtControl::Date,
        ),
        ("<w14:checkbox/>", rsword::model::SdtControl::Checkbox),
        ("<w:checkbox/>", rsword::model::SdtControl::Checkbox),
        ("<w:group/>", rsword::model::SdtControl::Group),
        ("<w:citation/>", rsword::model::SdtControl::Citation),
        ("<w:bibliography/>", rsword::model::SdtControl::Bibliography),
        ("<w:equation/>", rsword::model::SdtControl::Equation),
        ("<w15:repeatingSection/>", rsword::model::SdtControl::RepeatingSection),
        ("<w15:repeatingSectionItem/>", rsword::model::SdtControl::RepeatingSectionItem),
        ("<w:docPartList/>", rsword::model::SdtControl::DocPartList),
        ("", rsword::model::SdtControl::Unknown),
        ("<w:id w:val=\"1\"/>", rsword::model::SdtControl::Unknown),
    ];
    for (pr, want) in kinds {
        assert_eq!(info_of(pr).control, want, "{pr}");
    }
    // docPartObj 的内容
    let i = info_of(
        r#"<w:docPartObj><w:docPartGallery w:val="Cover Pages"/><w:docPartCategory w:val="General"/><w:docPartUnique/></w:docPartObj>"#,
    );
    assert_eq!(i.control, SdtControl::DocPartObj);
    let dp = i.doc_part.as_ref().unwrap();
    assert_eq!(dp.gallery.as_deref(), Some("Cover Pages"));
    assert_eq!(dp.category.as_deref(), Some("General"));
    assert!(dp.unique);
    // `w:docPartUnique w:val="0"` 是显式关闭
    let i = info_of(r#"<w:docPartObj><w:docPartUnique w:val="0"/></w:docPartObj>"#);
    assert!(!i.doc_part.as_ref().unwrap().unique);
    assert!(info_of("<w:text/>").doc_part.is_none());
    // 名字表：变体与字面一一对应
    for c in rsword::model::SdtControl::ALL {
        assert_eq!(SdtControl::parse(c.as_str()), Some(*c), "{c}");
    }
    for l in rsword::model::SdtLock::ALL {
        assert_eq!(SdtLock::parse(l.as_str()), Some(*l), "{l}");
    }
}

#[test]
fn mod_08_locks_and_defaults() {
    let cases = [
        ("", rsword::model::SdtLock::Unlocked, false, false),
        (r#"<w:lock w:val="unlocked"/>"#, rsword::model::SdtLock::Unlocked, false, false),
        (r#"<w:lock w:val="sdtLocked"/>"#, rsword::model::SdtLock::SdtLocked, false, true),
        (r#"<w:lock w:val="contentLocked"/>"#, rsword::model::SdtLock::ContentLocked, true, false),
        (
            r#"<w:lock w:val="sdtContentLocked"/>"#,
            rsword::model::SdtLock::SdtContentLocked,
            true,
            true,
        ),
        // 不认识的字面按未锁（PROP-09 的保值只管属性表；这里是模型的降级）
        (r#"<w:lock w:val="weird"/>"#, rsword::model::SdtLock::Unlocked, false, false),
    ];
    for (pr, lock, content, sdt) in cases {
        let i = info_of(pr);
        assert_eq!(i.lock, lock, "{pr}");
        assert_eq!(i.content_locked(), content, "{pr}");
        assert_eq!(i.lock.sdt_locked(), sdt, "{pr}");
    }
    // 没有 sdtPr：全缺省
    let d = doc(r#"<w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#);
    let (blocks, _) = rsword::model::Document::build_main(&d, None, &Rels::default());
    let i = blocks[0].sdt().unwrap();
    assert_eq!((i.lock, i.control), (SdtLock::Unlocked, SdtControl::Unknown));
    assert!(i.alias.is_none() && i.tag.is_none() && i.id.is_none() && !i.showing_placeholder);
    assert!(i.refusal().is_none());
    // showingPlcHdr 是三态 OnOff
    assert!(!info_of(r#"<w:showingPlcHdr w:val="0"/>"#).showing_placeholder);
    assert!(info_of(r#"<w:showingPlcHdr w:val="1"/>"#).showing_placeholder);
}

#[test]
fn mod_08_refusing_sdt_walks_ancestors() {
    let d = doc(&format!(
        r#"<w:sdt><w:sdtPr><w:tag w:val="outer"/><w:lock w:val="contentLocked"/></w:sdtPr><w:sdtContent>
             {}
           </w:sdtContent></w:sdt>"#,
        sdt_body(r#"<w:tag w:val="inner"/>"#)
    ));
    let (blocks, _) = rsword::model::Document::build_main(&d, None, &Rels::default());
    // 最近的 sdt 是内层（未锁），但守卫要一直找到外层
    let para = blocks[0].node();
    assert_eq!(blocks[0].sdt().unwrap().tag.as_deref(), Some("inner"));
    let (info, why) = refusing_sdt(&d, para).expect("外层 sdt 拒绝");
    assert_eq!(info.tag.as_deref(), Some("outer"));
    assert_eq!(why, SdtRefusal::Locked);
    // 都不锁 → None
    let d = doc(&sdt_body(r#"<w:tag w:val="free"/>"#));
    let (blocks, _) = rsword::model::Document::build_main(&d, None, &Rels::default());
    assert!(refusing_sdt(&d, blocks[0].node()).is_none());
}

/// `EDIT-03` / `EDIT-05`：落在只读或绑定控件里的编辑被拒绝，且状态一个字节都没动。
#[test]
fn edit_03_locked_and_bound_sdt_refuse_edits_without_touching_state() {
    let cases = [
        (r#"<w:lock w:val="contentLocked"/>"#, DiagCode::EditSdtLocked),
        (r#"<w:lock w:val="sdtContentLocked"/>"#, DiagCode::EditSdtLocked),
        (r#"<w:dataBinding w:xpath="/a[1]" w:storeItemID="{X}"/>"#, DiagCode::EditSdtBound),
    ];
    for (pr, code) in cases {
        let bytes = common::docx_with_body(&sdt_body(pr));
        let mut s = EditSession::open(&bytes).unwrap();
        let para = s.document().main[0].node();
        let before = format!("{:?}", s.document());
        let ops = [
            EditOp::InsertText { at: InlinePos::new(para, 1), text: "y".into(), props: None },
            EditOp::DeleteRange { from: InlinePos::new(para, 0), to: InlinePos::new(para, 1) },
            EditOp::SplitParagraph { at: InlinePos::new(para, 1) },
            EditOp::DeleteBlock { part: None, node: para },
            EditOp::ReplaceInlines { part: None, para, inlines: Vec::new() },
        ];
        for op in ops {
            let name = format!("{op:?}");
            let err = s.apply(op, &Default::default()).expect_err(&name);
            match err {
                rsword::Error::Edit { code: got, message } => {
                    assert_eq!(got, code, "{pr} / {name}: {message}");
                }
                other => panic!("{pr} / {name}: 期望 Edit 错误，得到 {other:?}"),
            }
        }
        assert_eq!(format!("{:?}", s.document()), before, "{pr}: 投影不该变");
        assert_eq!(s.save().unwrap(), bytes, "{pr}: 保存字节不该变（不变式 1）");
    }
    // 未锁的同形文档能改，证明拒绝来自 sdt 而不是别的原因
    let bytes = common::docx_with_body(&sdt_body(r#"<w:tag w:val="free"/>"#));
    let mut s = EditSession::open(&bytes).unwrap();
    let para = s.document().main[0].node();
    s.apply(
        EditOp::InsertText { at: InlinePos::new(para, 1), text: "y".into(), props: None },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(s.document().text_blocks().next().unwrap().text(), "xy");
    // 锁在别的 sdt 里，不影响这一段
    let bytes = common::docx_with_body(&format!(
        "{}{}",
        sdt_body(r#"<w:lock w:val="contentLocked"/>"#),
        r#"<w:p><w:r><w:t>free</w:t></w:r></w:p>"#
    ));
    let mut s = EditSession::open(&bytes).unwrap();
    let free = s.document().main[1].node();
    s.apply(
        EditOp::InsertText { at: InlinePos::new(free, 4), text: "!".into(), props: None },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(s.document().text_blocks().nth(1).unwrap().text(), "free!");
}

/// 语料里的 sdt：每个 `w:sdt` 都能读出信息，块上的 `SdtInfo` 与直接读一致。
#[test]
fn mod_08_corpus_sdt_reads() {
    let mut docs = 0;
    let mut sdts = 0;
    let mut controls: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = rsword::package::Package::open(&bytes) else { continue };
            let Ok(document) = rsword::model::Document::rebuild(&mut pkg) else { continue };
            docs += 1;
            let dom = pkg.part(document.main_part).dom().unwrap();
            for n in dom.descendants(dom.root()).filter(|&n| dom.is(n, QName::w(LocalName::Sdt))) {
                sdts += 1;
                let info = rsword::model::SdtInfo::read(dom, n);
                *controls.entry(info.control.as_str()).or_default() += 1;
                // 块上挂的信息与直接读一致
                for b in document.blocks() {
                    if let Some(s) = b.sdt()
                        && s.node == n
                    {
                        assert_eq!(*s, info, "{}", path.display());
                    }
                }
            }
        }
    }
    eprintln!("sdt: {docs} docs, {sdts} sdt, controls {controls:?}");
    assert!(sdts >= 14, "{sdts}");
    // 语料里没有锁与数据绑定（行为靠上面的合成用例保证）
    let _ = rsword::model::Block::Text;
}
