//! `MOD-10` 批注与脚注 / 尾注条目，`COMPAT-07` 的 `commentIds`（任务 2.6）。

mod common;

use rsword::model::{Document, NoteKind};
use rsword::package::Package;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const W16CID: &str = "http://schemas.microsoft.com/office/word/2016/wordml/cid";

fn doc_of(bytes: &[u8]) -> Document {
    let mut pkg = Package::open(bytes).unwrap();
    rsword::model::Document::rebuild(&mut pkg).unwrap()
}

fn corpus_doc(name: &str) -> Document {
    let bytes = std::fs::read(common::corpus_dir("synthetic").join(name)).unwrap();
    doc_of(&bytes)
}

#[cfg(feature = "compat-ts")]
fn json_of(name: &str) -> serde_json::Value {
    let bytes = std::fs::read(common::corpus_dir("synthetic").join(name)).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap()
}

/// 一条批注要三个部件合起来：正文 / 回复与已解决 / durableId，按最后一段的 `w14:paraId` 关联。
#[test]
fn mod_10_comments_join_extended_and_ids() {
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{W}" xmlns:w14="{W14}">
             <w:comment w:id="1" w:author="Alice" w:initials="A" w:date="2026-07-01T10:00:00Z">
               <w:p w14:paraId="AAAA0001"><w:r><w:t>主批注</w:t></w:r></w:p>
               <w:p w14:paraId="AAAA0002"><w:r><w:t>第二段</w:t></w:r></w:p>
             </w:comment>
             <w:comment w:id="2" w:author="Bob"><w:p w14:paraId="BBBB0001"><w:r><w:t>回复</w:t></w:r></w:p></w:comment>
           </w:comments>"#
    );
    let extended = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w15:commentsEx xmlns:w15="{W15}">
             <w15:commentEx w15:paraId="AAAA0002" w15:done="1"/>
             <w15:commentEx w15:paraId="BBBB0001" w15:paraIdParent="AAAA0002"/>
           </w15:commentsEx>"#
    );
    let ids = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w16cid:commentsIds xmlns:w16cid="{W16CID}">
             <w16cid:commentId w16cid:paraId="AAAA0002" w16cid:durableId="7F2A1B33"/>
           </w16cid:commentsIds>"#
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>被批注</w:t></w:r><w:commentRangeEnd w:id="1"/>
           <w:r><w:commentReference w:id="1"/></w:r></w:p>"#,
        &[
            ("word/comments.xml", &comments),
            ("word/commentsExtended.xml", &extended),
            ("word/commentsIds.xml", &ids),
        ],
    );
    let doc = doc_of(&bytes);
    assert_eq!(doc.comments.items.len(), 2);
    let a = doc.comments.get("1").expect("批注 1");
    assert_eq!(a.author.as_deref(), Some("Alice"));
    assert_eq!(a.initials.as_deref(), Some("A"));
    assert_eq!(a.date.as_deref(), Some("2026-07-01T10:00:00Z"));
    assert_eq!(a.text, "主批注\n第二段", "段间用 \\n 连接");
    assert_eq!(a.para_id.as_deref(), Some("AAAA0002"), "取最后一段的 w14:paraId");
    assert!(a.done, "w15:done");
    assert_eq!(a.durable_id.as_deref(), Some("7F2A1B33"));
    assert_eq!(a.paragraphs.len(), 2);
    let b = doc.comments.get("2").expect("批注 2");
    assert_eq!(b.parent_id.as_deref(), Some("1"), "paraIdParent 反查成父批注的 id");
    assert!(!b.done);
    assert_eq!(doc.comments.next_id(), 3, "EDIT-06：批注 id 取最大值 + 1");
}

/// 语料：`comments.xml` 的字段与 TS 一致（缺 `date` / `initials` 时不臆造）。
#[test]
fn mod_10_comments_from_corpus() {
    let doc = corpus_doc("comments__009.docx");
    assert_eq!(doc.comments.items.len(), 2);
    let one = doc.comments.get("1").unwrap();
    assert_eq!(one.text, "加粗观点与普通文字\n第二段斜体高亮");
    assert_eq!(one.initials.as_deref(), Some("A"));
    assert_eq!(one.date, None);
    assert_eq!(doc.comments.get("2").unwrap().author.as_deref(), Some("Bob"));
    // 没有 comments.xml 的文档：集合为空，`part` 为 None（`AddComment` 据此建 part）
    let none = corpus_doc("comments__010.docx");
    assert!(none.comments.items.is_empty() && none.comments.part.is_none());
    assert_eq!(none.comments.next_id(), 1);
}

/// 结构条目（separator）不是正文；首段前导空白与自引用标记不进文字；`richParas` 只在有格式时出。
#[test]
fn mod_10_notes_skip_structural_entries_and_trim_first_line() {
    let doc = corpus_doc("text-patch__002.docx");
    assert_eq!(doc.footnotes.items.len(), 2, "separator 也在模型里（保存要原样留）");
    assert_eq!(doc.footnotes.items[0].kind, NoteKind::Separator);
    let normal: Vec<_> = doc.footnotes.normal().collect();
    assert_eq!(normal.len(), 1);
    let n = normal[0];
    assert_eq!(n.id, "2");
    assert_eq!(n.text, "斜体脚注", "首段的分隔空格与自引用标记都不算文字");
    assert!(!n.no_ref_mark, "这条有 w:footnoteRef");
    assert_eq!(n.rich.len(), 1);
    assert_eq!(n.rich[0].len(), 1, "只剩斜体那个 run（纯空白的 run 被吃掉）");
    assert_eq!(n.rich[0][0].props.italic, Some(true));
    assert_eq!(doc.footnotes.next_id(), 3);

    // 没有自引用标记的条目
    let wp = corpus_doc("write-protection__004.docx");
    assert!(wp.footnotes.normal().all(|n| n.no_ref_mark));
    assert!(wp.endnotes.normal().all(|n| n.no_ref_mark));
    assert_eq!(wp.footnotes.normal().next().unwrap().text, "Footnote content");
    assert_eq!(wp.endnotes.normal().next().unwrap().text, "Endnote content");
}

/// `COMPAT-07`：起止都在本段的批注挂到 run 上；只有一端在本段的只出块级 `commentStarts/Ends`；
/// 只有 `commentReference` 的批注挂最近的有字 run（先往前找，再往后找）。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_07_comment_ids_follow_the_ts_rules() {
    // 同段范围：范围内的 run 拿到 id，范围外的两个 run 没有
    let doc = corpus_doc("comments__001.docx");
    let first = doc.text_blocks().next().unwrap();
    let ids: Vec<(String, usize)> = first
        .inlines
        .iter()
        .filter_map(|i| match i {
            rsword::model::Inline::Run(r) if !r.text.is_empty() => {
                Some((r.text.clone(), r.comments.len()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![("before ".into(), 0), ("marked".into(), 1), (" after".into(), 0)]);
    let json = json_of("comments__001.docx");
    let runs = json["blocks"][0]["runs"].as_array().unwrap();
    assert_eq!(runs[1]["commentIds"], serde_json::json!(["1"]));
    assert!(runs[0].get("commentIds").is_none());
    // 跨段范围：块级 starts / ends，run 上不挂
    assert_eq!(json["blocks"][1]["commentStarts"], serde_json::json!(["2"]));
    assert_eq!(json["blocks"][2]["commentEnds"], serde_json::json!(["2"]));
    for b in [1usize, 2] {
        for r in json["blocks"][b]["runs"].as_array().unwrap() {
            assert!(r.get("commentIds").is_none(), "跨段范围不挂到 run: {r}");
        }
    }
    // 只有 reference：reference 在后 → 挂前一个 run；在前 → 挂后一个 run
    let after = json_of("comments__011.docx");
    assert_eq!(after["blocks"][0]["runs"][0]["text"], "hello");
    assert_eq!(after["blocks"][0]["runs"][0]["commentIds"], serde_json::json!(["1"]));
    let before = json_of("comments__012.docx");
    assert_eq!(before["blocks"][0]["runs"][0]["text"], "after");
    assert_eq!(before["blocks"][0]["runs"][0]["commentIds"], serde_json::json!(["1"]));
    // 段里只有 reference run（没有别的 run）→ 无处可挂
    let alone = json_of("comments__013.docx");
    assert!(alone["blocks"][0]["runs"].as_array().unwrap().is_empty());
}

/// 正文里的脚注 / 尾注引用是原子 run，`text` 是按 part 顺序的显示编号。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_07_note_reference_runs_carry_the_display_number() {
    let json = json_of("notes__001.docx");
    let runs = json["blocks"][0]["runs"].as_array().unwrap();
    let note = runs.iter().find(|r| r.get("noteRef").is_some()).expect("noteRef run");
    assert_eq!(note["noteRef"], serde_json::json!({"id": "2", "kind": "footnote"}));
    assert_eq!(note["text"], "1", "第一个正文条目编号 1（separator 不计）");
    assert_eq!(json["footnotes"], serde_json::json!([{"id": "2", "text": "这是脚注正文"}]));
}

/// 全语料：批注与注释部件解析不 panic，条目 id 唯一，`next_id` 单调。
#[test]
fn mod_10_comments_and_notes_across_the_corpus() {
    let mut docs = 0;
    let mut with_comments = 0;
    let mut comments = 0;
    let mut notes = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
        docs += 1;
        if !doc.comments.items.is_empty() {
            with_comments += 1;
        }
        comments += doc.comments.items.len();
        notes += doc.footnotes.normal().count() + doc.endnotes.normal().count();
        let ids: Vec<&str> = doc.comments.items.iter().map(|c| c.id.as_str()).collect();
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(ids.len(), uniq.len(), "{}: 批注 id 重复", path.display());
        for c in &doc.comments.items {
            if let Ok(n) = c.id.parse::<u32>() {
                assert!(doc.comments.next_id() > n, "{}", path.display());
            }
        }
    }
    eprintln!("语料 {docs} 份：{with_comments} 份带批注（{comments} 条），{notes} 条注释");
    assert!(docs > 500 && comments > 0 && notes > 0);
}

// ---- `SAVE-05` 新建 part 与批注编辑操作（任务 2.6 写侧）----

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, NewComment};

fn session(body: &str) -> EditSession {
    EditSession::open(&common::docx_with_body(body)).unwrap()
}

fn first_para(s: &EditSession) -> rsword::xml::NodeId {
    s.document().text_blocks().next().unwrap().node
}

fn part_text(bytes: &[u8], name: &str) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut f = z.by_name(name).ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).unwrap();
    Some(s)
}

#[cfg(feature = "compat-ts")]
fn entries(bytes: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index_raw(i).unwrap();
            let mut raw = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut raw).unwrap();
            (f.name().to_string(), f.crc32(), raw)
        })
        .collect()
}

/// `SAVE-05` 验收行：首次加批注 → 新建 `comments.xml` + 关系 + 内容类型，
/// 其他条目的原压缩数据一个字节不变。
#[test]
#[cfg(feature = "compat-ts")]
fn save_05_first_comment_creates_the_part_and_leaves_others_untouched() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>hello world</w:t></w:r></w:p>"#);
    let before = entries(&bytes);
    let mut s = EditSession::open(&bytes).unwrap();
    let p = first_para(&s);
    s.apply(
        EditOp::AddComment {
            from: InlinePos::new(p, 0),
            to: InlinePos::new(p, 5),
            comment: NewComment {
                author: "Alice".into(),
                initials: Some("A".into()),
                date: Some("2026-09-04T10:00:00Z".into()),
                text: "第一条\n第二段".into(),
                ..Default::default()
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    let saved = s.save().unwrap();

    let comments = part_text(&saved, "word/comments.xml").expect("新 part 在包里");
    assert!(comments.contains(r#"w:id="1""#), "{comments}");
    assert!(comments.contains(r#"w:author="Alice""#) && comments.contains(r#"w:initials="A""#));
    assert!(comments.contains("第一条") && comments.contains("第二段"));
    assert!(comments.contains("w14:paraId"), "最后一段带 paraId: {comments}");
    assert!(comments.contains("w:annotationRef"), "首段有引用标记 run: {comments}");
    // 关系与内容类型
    let rels = part_text(&saved, "word/_rels/document.xml.rels").unwrap();
    assert!(rels.contains("comments.xml") && rels.contains("/comments"), "{rels}");
    let ct = part_text(&saved, "[Content_Types].xml").unwrap();
    assert!(ct.contains("/word/comments.xml") && ct.contains("comments+xml"), "{ct}");
    // 正文：范围标记与 reference run
    let doc = part_text(&saved, "word/document.xml").unwrap();
    assert!(doc.contains(r#"<w:commentRangeStart w:id="1"/>"#), "{doc}");
    assert!(doc.contains(r#"<w:commentRangeEnd w:id="1"/>"#), "{doc}");
    assert!(doc.contains(r#"<w:commentReference w:id="1"/>"#), "{doc}");
    // `SAVE-06`：除主 part / rels / 内容类型外，其他条目原压缩数据不变
    let after = entries(&saved);
    for (name, crc, raw) in &before {
        let Some((_, c2, r2)) = after.iter().find(|(n, ..)| n == name) else {
            panic!("条目 {name} 丢了")
        };
        if ["word/document.xml", "word/_rels/document.xml.rels", "[Content_Types].xml"]
            .contains(&name.as_str())
        {
            continue;
        }
        assert_eq!((crc, raw), (c2, r2), "{name} 的压缩数据变了");
    }
    // 重开：模型看得到这条批注，run 上挂着 id
    let mut re = EditSession::open(&saved).unwrap();
    let doc = re.document();
    assert_eq!(doc.comments.items.len(), 1);
    assert_eq!(doc.comments.get("1").unwrap().text, "第一条\n第二段");
    let json = rsword::bind::compat_ts::parsed_doc(re.package_mut()).unwrap();
    assert_eq!(json["blocks"][0]["runs"][0]["commentIds"], serde_json::json!(["1"]));
    assert_eq!(json["comments"][0]["author"], "Alice");
}

/// `EDIT-06` 验收行：连续两次 `AddComment` 拿到不同的 `w:id`，`comments.xml` 有两条。
#[test]
fn edit_06_two_comments_get_different_ids() {
    let mut s = session(r#"<w:p><w:r><w:t>ab</w:t></w:r><w:r><w:t>cd</w:t></w:r></w:p>"#);
    let p = first_para(&s);
    let ctx = EditContext::default();
    for (a, b, who) in [(0u32, 2u32, "Alice"), (2, 4, "Bob")] {
        s.apply(
            EditOp::AddComment {
                from: InlinePos::new(p, a),
                to: InlinePos::new(p, b),
                comment: NewComment {
                    author: who.into(),
                    text: format!("{who} 说"),
                    ..Default::default()
                },
            },
            &ctx,
        )
        .unwrap();
    }
    let ids: Vec<&str> = s.document().comments.items.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["1", "2"]);
    let saved = s.save().unwrap();
    let comments = part_text(&saved, "word/comments.xml").unwrap();
    assert_eq!(comments.matches("<w:comment ").count(), 2, "{comments}");
    let doc = part_text(&saved, "word/document.xml").unwrap();
    assert_eq!(doc.matches("<w:commentRangeStart").count(), 2, "{doc}");
    assert_eq!(doc.matches("<w:commentReference").count(), 2, "{doc}");
}

/// `SetCommentText` 保留原有格式；`done` 与回复写进 `commentsExtended`（不存在则新建 part）。
#[test]
fn edit_03_set_comment_text_and_resolved_state() {
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{W}" xmlns:w14="{W14}">
             <w:comment w:id="1" w:author="Alice">
               <w:p w14:paraId="0000AAAA"><w:r><w:rPr><w:b/></w:rPr><w:t>原文</w:t></w:r></w:p>
             </w:comment></w:comments>"#
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>正文</w:t></w:r><w:commentRangeEnd w:id="1"/>
           <w:r><w:commentReference w:id="1"/></w:r></w:p>"#,
        &[("word/comments.xml", &comments)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    s.apply(
        EditOp::SetCommentText { id: "1".into(), text: "改过了".into(), done: Some(true) },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(s.document().comments.get("1").unwrap().text, "改过了");
    assert!(s.document().comments.get("1").unwrap().done);
    let saved = s.save().unwrap();
    let out = part_text(&saved, "word/comments.xml").unwrap();
    assert!(out.contains("改过了") && !out.contains("原文"), "{out}");
    assert!(out.contains("<w:b/>"), "保留原来的加粗: {out}");
    let ex = part_text(&saved, "word/commentsExtended.xml").expect("新建 commentsExtended");
    assert!(ex.contains(r#"w15:paraId="0000AAAA""#) && ex.contains(r#"w15:done="1""#), "{ex}");
    // 重开后 done 还在
    let re = EditSession::open(&saved).unwrap();
    assert!(re.document().comments.get("1").unwrap().done);
}

/// `RemoveComment`：条目、范围标记与 reference run 一起消失，正文其他内容不动。
#[test]
fn edit_03_remove_comment_clears_entry_markers_and_reference() {
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{W}">
             <w:comment w:id="1" w:author="Alice"><w:p><w:r><w:t>要删</w:t></w:r></w:p></w:comment>
             <w:comment w:id="2" w:author="Bob"><w:p><w:r><w:t>留着</w:t></w:r></w:p></w:comment>
           </w:comments>"#
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>被批注</w:t></w:r><w:commentRangeEnd w:id="1"/>
           <w:r><w:commentReference w:id="1"/></w:r><w:r><w:t>尾巴</w:t></w:r></w:p>"#,
        &[("word/comments.xml", &comments)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    s.apply(EditOp::RemoveComment { id: "1".into() }, &EditContext::default()).unwrap();
    assert!(s.document().comments.get("1").is_none());
    assert!(s.document().comments.get("2").is_some(), "另一条不受影响");
    let saved = s.save().unwrap();
    let doc = part_text(&saved, "word/document.xml").unwrap();
    assert!(!doc.contains("commentRangeStart") && !doc.contains("commentReference"), "{doc}");
    assert!(doc.contains("被批注") && doc.contains("尾巴"), "正文内容不动: {doc}");
    let out = part_text(&saved, "word/comments.xml").unwrap();
    assert!(!out.contains("要删") && out.contains("留着"), "{out}");
}

/// 加批注失败（位置非法）不留半改状态：包与投影都回到操作前。
#[test]
fn edit_05_failed_add_comment_rolls_back_the_new_part() {
    let bytes = common::docx_with_body(r#"<w:p><w:r><w:t>ab</w:t></w:r></w:p>"#);
    let mut s = EditSession::open(&bytes).unwrap();
    let p = first_para(&s);
    let err = s
        .apply(
            EditOp::AddComment {
                from: InlinePos::new(p, 0),
                to: InlinePos::new(p, 99),
                comment: NewComment { author: "A".into(), text: "x".into(), ..Default::default() },
            },
            &EditContext::default(),
        )
        .expect_err("越界");
    assert!(matches!(err, rsword::Error::Edit { .. }), "{err:?}");
    assert!(s.document().comments.items.is_empty());
    assert_eq!(s.save().unwrap(), bytes, "保存回到原字节（新 part 也回滚了）");
}

// ---- compat 的权威条目列表（`SaveOptions.comments` / `footnotes`，任务 2.6c）----

/// `footnotes` 权威列表：改已有条目时保住自引用标记 run 与结构条目，列表里的新条目建出来，
/// 列表外的条目删掉。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_04_footnote_list_rewrites_entries_and_keeps_separators() {
    let footnotes = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:footnotes xmlns:w="{W}">
             <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
             <w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>旧正文</w:t></w:r></w:p></w:footnote>
             <w:footnote w:id="4"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>要删的</w:t></w:r></w:p></w:footnote>
           </w:footnotes>"#
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:r><w:t>正文</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p>"#,
        &[("word/footnotes.xml", &footnotes)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let blocks = serde_json::json!([{ "kind": "original", "docxIndex": 0 }]);
    let options = serde_json::json!({
        "footnotes": [
            { "id": "2", "richParas": [[{ "text": "新正文", "italic": true }]], "text": "新正文" },
            { "id": "3", "text": "新脚注" }
        ]
    });
    let outcome = rsword::bind::compat_ts::apply_save_blocks(&mut s, &blocks, &options).unwrap();
    assert!(!outcome.unchanged && outcome.ops > 0);
    let saved = s.save().unwrap();
    let out = part_text(&saved, "word/footnotes.xml").unwrap();
    assert!(out.contains(r#"w:type="separator""#), "结构条目留着: {out}");
    assert!(out.contains("新正文") && !out.contains("旧正文"), "{out}");
    assert!(out.contains("<w:i/>"), "richParas 的格式发出来: {out}");
    assert_eq!(out.matches("<w:footnoteRef/>").count(), 2, "自引用标记 run 保住: {out}");
    assert!(out.contains(r#"w:id="3""#) && out.contains("新脚注"), "新条目: {out}");
    assert!(!out.contains("要删的"), "列表外的条目删掉: {out}");
    // 重开：模型读得到
    let re = EditSession::open(&saved).unwrap();
    let ids: Vec<&str> = re.document().footnotes.normal().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["2", "3"]);
    assert_eq!(re.document().footnotes.get("2").unwrap().text, "新正文");
}

/// `comments` 权威列表：列表外的批注连正文标记一起删，列表里的条目按内容改 / 建。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_04_comment_list_is_authoritative() {
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{W}">
             <w:comment w:id="1" w:author="Alice"><w:p><w:r><w:t>留着改</w:t></w:r></w:p></w:comment>
             <w:comment w:id="2" w:author="Bob"><w:p><w:r><w:t>要删</w:t></w:r></w:p></w:comment>
           </w:comments>"#
    );
    let bytes = common::docx_with_parts(
        concat!(
            r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>甲</w:t></w:r><w:commentRangeEnd w:id="1"/>"#,
            r#"<w:r><w:commentReference w:id="1"/></w:r>"#,
            r#"<w:commentRangeStart w:id="2"/><w:r><w:t>乙</w:t></w:r><w:commentRangeEnd w:id="2"/>"#,
            r#"<w:r><w:commentReference w:id="2"/></w:r></w:p>"#
        ),
        &[("word/comments.xml", &comments)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let blocks = serde_json::json!([{ "kind": "original", "docxIndex": 0 }]);
    let options = serde_json::json!({
        "comments": [{ "id": "1", "author": "Alice", "initials": "A", "text": "改过的" }]
    });
    rsword::bind::compat_ts::apply_save_blocks(&mut s, &blocks, &options).unwrap();
    let saved = s.save().unwrap();
    let out = part_text(&saved, "word/comments.xml").unwrap();
    assert!(out.contains("改过的") && !out.contains("留着改"), "{out}");
    assert!(!out.contains("要删"), "列表外的条目删掉: {out}");
    assert!(out.contains(r#"w:initials="A""#), "属性按列表更新: {out}");
    let doc = part_text(&saved, "word/document.xml").unwrap();
    assert!(doc.contains(r#"<w:commentRangeStart w:id="1"/>"#), "留下的批注标记不动: {doc}");
    assert!(!doc.contains(r#"w:id="2""#), "被删批注的标记与 reference 一起清掉: {doc}");
    assert!(doc.contains("甲") && doc.contains("乙"), "正文文字不动: {doc}");
}

/// 任务 5.3：注释与批注条目的内容也是 `Vec<Block>`，与正文同一构建器（`MOD-01`）。
/// `text` / `rich` 是 TS 形态的投影，与 `blocks` 并存。
#[test]
fn mod_01_note_and_comment_entries_carry_blocks() {
    let doc = corpus_doc("text-patch__002.docx");
    // separator 条目：内容是一个 `w:separator` run 的段落，照样成块（保存要原样留）
    let sep = &doc.footnotes.items[0];
    assert_eq!(sep.kind, NoteKind::Separator);
    assert_eq!(sep.blocks.len(), sep.paragraphs.len());

    let n = doc.footnotes.normal().next().expect("正文条目");
    assert_eq!(n.blocks.len(), 1);
    let b = n.blocks[0].as_text().expect("文本块");
    assert_eq!(b.node, n.paragraphs[0], "块就是条目里那个 w:p");
    // 坐标流含自引用标记（长度 0）与文字
    assert!(b.text().contains("斜体脚注"));
    assert!(doc.footnotes.idx.is_some(), "part 的索引建了");

    let c = corpus_doc("comments__001.docx");
    let first = c.comments.items.first().expect("批注");
    assert!(!first.blocks.is_empty());
    assert_eq!(
        first.blocks.iter().filter_map(rsword::model::Block::as_text).count(),
        first.paragraphs.len()
    );
    assert!(c.comments.idx.is_some());
}

/// 全语料：每个条目的块与它的 `w:p` 列表对得上（`MOD-01`）。
#[test]
fn mod_01_note_and_comment_blocks_across_the_corpus() {
    let mut entries = 0usize;
    let mut blocks = 0usize;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
        let notes = doc.footnotes.items.iter().chain(doc.endnotes.items.iter());
        for n in notes {
            entries += 1;
            blocks += n.blocks.len();
            if !n.paragraphs.is_empty() {
                assert!(!n.blocks.is_empty(), "{}: 条目 {} 没有块", path.display(), n.id);
            }
            for b in n.blocks.iter().filter_map(rsword::model::Block::as_text) {
                assert!(
                    n.paragraphs.contains(&b.node),
                    "{}: 条目 {} 的文本块不在 paragraphs 里",
                    path.display(),
                    n.id
                );
            }
        }
        for c in &doc.comments.items {
            entries += 1;
            blocks += c.blocks.len();
            if !c.paragraphs.is_empty() {
                assert!(!c.blocks.is_empty(), "{}: 批注 {} 没有块", path.display(), c.id);
            }
        }
    }
    eprintln!("notes/comments: {entries} 个条目、{blocks} 个块");
    assert!(entries > 20, "语料缺失？{entries}");
}
