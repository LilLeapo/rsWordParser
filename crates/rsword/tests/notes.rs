//! `MOD-10` 批注与脚注 / 尾注条目，`COMPAT-07` 的 `commentIds`（任务 2.6）。

mod common;

use rsword::model::{Document, Inline, NoteKind};
use rsword::package::Package;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const W16CID: &str = "http://schemas.microsoft.com/office/word/2016/wordml/cid";

fn doc_of(bytes: &[u8]) -> Document {
    let mut pkg = Package::open(bytes).unwrap();
    Document::rebuild(&mut pkg).unwrap()
}

fn corpus_doc(name: &str) -> Document {
    let bytes = std::fs::read(common::corpus_dir("synthetic").join(name)).unwrap();
    doc_of(&bytes)
}

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
fn compat_07_comment_ids_follow_the_ts_rules() {
    // 同段范围：范围内的 run 拿到 id，范围外的两个 run 没有
    let doc = corpus_doc("comments__001.docx");
    let first = doc.text_blocks().next().unwrap();
    let ids: Vec<(String, usize)> = first
        .inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Run(r) if !r.text.is_empty() => Some((r.text.clone(), r.comments.len())),
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
        let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
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
