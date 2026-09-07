//! 追踪修订的生成（`EDIT-03` 的「修订」行，`spec/18` 7.2）与门 1 的三条 oracle。
//!
//! oracle 用 [`ModelFingerprint`] 的两个视图表达（`spec/18` 分层决策 3），
//! **不依赖 7.4 的 `AcceptAll` / `RejectAll`**：
//!
//! 1. **拒绝还原**：`apply(op, track = A)` 之后的 **reject 视图** == 操作前的 reject 视图。
//! 2. **接受等价**：`apply(op, track = A)` 之后的 **accept 视图** == `apply(op, 不追踪)` 之后的
//!    accept 视图。
//! 3. **往返**：追踪后保存 → 重解析，`Document.revisions` 恰好多出预期的条目（种类 / 作者 / 个数），
//!    两个视图都不变。

mod common;

use common::fingerprint::{diff_str, fingerprint};
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, RevisionAuthor};
use rsword::model::RevKind;
use rsword::semantic::props::{Change, Jc, ParaPropsPatch, RunPropsPatch, Val};
use rsword::xml::NodeId;

const A: &str = "作者甲";
const B: &str = "作者乙";
const DATE: &str = "2026-09-07T10:00:00Z";

fn tracked(author: &str) -> EditContext {
    EditContext {
        track_changes: Some(RevisionAuthor {
            author: author.to_string(),
            date: Some(DATE.to_string()),
        }),
        ..Default::default()
    }
}

fn open(body: &str) -> EditSession {
    EditSession::open(&common::docx_with_body(body)).expect("open")
}

/// 第 `i` 个正文文本段落。
fn para(s: &EditSession, i: usize) -> NodeId {
    s.document()
        .main
        .iter()
        .filter_map(|b| b.as_text().map(|t| t.node))
        .nth(i)
        .expect("有这么多文本段落")
}

/// 三条 oracle 的公共骨架：同一份文档、同一个操作，一份追踪一份不追踪。
///
/// `op` 拿到会话后返回要执行的操作（位置要在那一份会话里解析）。
fn oracle(body: &str, what: &str, op: impl Fn(&EditSession) -> EditOp) {
    let before = fingerprint(&open(body));

    let mut plain = open(body);
    let plain_op = op(&plain);
    plain.apply(plain_op, &EditContext::default()).unwrap_or_else(|e| panic!("{what} 不追踪：{e}"));
    let plain_fp = fingerprint(&plain);

    let mut track = open(body);
    let track_op = op(&track);
    track.apply(track_op, &tracked(A)).unwrap_or_else(|e| panic!("{what} 追踪：{e}"));
    let track_fp = fingerprint(&track);

    // ① 拒绝还原
    if let Some((x, y)) = diff_str(&before.reject, &track_fp.reject) {
        panic!("{what}：拒绝视图没回到操作前\n  操作前: {x}\n  追踪后: {y}");
    }
    // ② 接受等价
    if let Some((x, y)) = diff_str(&plain_fp.accept, &track_fp.accept) {
        panic!("{what}：接受视图与不追踪不同\n  不追踪: {x}\n  追踪: {y}");
    }
    // ③ 往返：保存 → 重解析，两个视图都不变，且确实生成了修订
    let saved = track.save().unwrap_or_else(|e| panic!("{what} 保存：{e}"));
    let reopened = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what} 重解析：{e}"));
    let round = fingerprint(&reopened);
    if let Some((view, x, y)) = track_fp.diff(&round) {
        panic!("{what}：往返后指纹变了\n  视图 {view}\n  保存前: {x}\n  重解析: {y}");
    }
    let _ = &before.accept;
    let revs = &reopened.document().revisions;
    assert!(!revs.is_empty(), "{what}：追踪却没有生成任何修订");
    assert!(
        revs.entries().iter().all(|e| e.author() == Some(A)),
        "{what}：修订作者应该都是 {A}，实际 {:?}",
        revs.authors()
    );
    assert!(
        revs.entries().iter().all(|e| e.meta.date.as_deref() == Some(DATE)),
        "{what}：`w:date` 应该是上下文给的原串"
    );
}

const TWO_PARAS: &str = concat!(
    "<w:p><w:r><w:t>第一段原文。</w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>第二段加粗。</w:t></w:r></w:p>",
);

// ---- 门 1 的三条 oracle × 七个操作 --------------------------------------------------------------

#[test]
fn oracle_insert_text() {
    oracle(TWO_PARAS, "InsertText 段中", |s| EditOp::InsertText {
        at: InlinePos::new(para(s, 0), 3),
        text: "插入".into(),
        props: None,
    });
    oracle(TWO_PARAS, "InsertText 段首", |s| EditOp::InsertText {
        at: InlinePos::new(para(s, 0), 0),
        text: "前缀".into(),
        props: None,
    });
    oracle(TWO_PARAS, "InsertText 段尾", |s| EditOp::InsertText {
        at: InlinePos::new(para(s, 1), 6),
        text: "后缀".into(),
        props: None,
    });
}

#[test]
fn oracle_delete_range() {
    oracle(TWO_PARAS, "DeleteRange 段中", |s| EditOp::DeleteRange {
        from: InlinePos::new(para(s, 0), 1),
        to: InlinePos::new(para(s, 0), 4),
    });
    oracle(TWO_PARAS, "DeleteRange 整段", |s| EditOp::DeleteRange {
        from: InlinePos::new(para(s, 1), 0),
        to: InlinePos::new(para(s, 1), 6),
    });
}

#[test]
fn oracle_set_run_props() {
    oracle(TWO_PARAS, "SetRunProps 加粗", |s| EditOp::SetRunProps {
        from: InlinePos::new(para(s, 0), 1),
        to: InlinePos::new(para(s, 0), 4),
        patch: RunPropsPatch { bold: Change::Set(true), ..Default::default() },
    });
    oracle(TWO_PARAS, "SetRunProps 取消加粗", |s| EditOp::SetRunProps {
        from: InlinePos::new(para(s, 1), 0),
        to: InlinePos::new(para(s, 1), 6),
        patch: RunPropsPatch { bold: Change::Unset, ..Default::default() },
    });
}

#[test]
fn oracle_set_para_props() {
    oracle(TWO_PARAS, "SetParaProps 居中", |s| EditOp::SetParaProps {
        part: None,
        para: para(s, 0),
        patch: ParaPropsPatch { jc: Change::Set(Val::Value(Jc::Center)), ..Default::default() },
    });
}

#[test]
fn oracle_split_paragraph() {
    oracle(TWO_PARAS, "SplitParagraph", |s| EditOp::SplitParagraph {
        at: InlinePos::new(para(s, 0), 3),
    });
}

#[test]
fn oracle_merge_with_next() {
    oracle(TWO_PARAS, "MergeWithNext", |s| EditOp::MergeWithNext { part: None, para: para(s, 0) });
}

// ---- `EDIT-03` 验收行与同作者规则 ---------------------------------------------------------------

/// `spec/08`：在干净 run 中间插字 → 出现 `w:ins`，`w:p` 的开标签字节不变。
#[test]
fn edit_03_insert_makes_w_ins() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "新".into(), props: None },
        &tracked(A),
    )
    .expect("插字");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p[1]/w:ins)", ["1"]),
            ("//w:p[1]/w:ins/@w:author", [A]),
            ("//w:p[1]/w:ins/@w:date", [DATE]),
            ("//w:p[1]/w:ins/w:r/w:t/text()", ["新"]),
        ]
    );
}

/// 追踪时合并 → 段落仍分开，`pPr/rPr/w:del` 出现（`spec/08`）。
#[test]
fn edit_03_tracked_merge_marks_paragraph() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(EditOp::MergeWithNext { part: None, para: p }, &tracked(A)).expect("合并");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["2"]),
            ("count(//w:p[1]/w:pPr/w:rPr/w:del)", ["1"]),
            ("//w:p[1]/w:pPr/w:rPr/w:del/@w:author", [A]),
        ]
    );
}

/// 追踪删除：内容留着、`w:t` 变 `w:delText`、坐标流长度不变。
#[test]
fn edit_03_tracked_delete_keeps_text() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    let before = s.text_block(p).expect("段落").text().chars().count();
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 0), to: InlinePos::new(p, 3) },
        &tracked(A),
    )
    .expect("删除");
    let after = s.text_block(para(&s, 0)).expect("段落").text().chars().count();
    assert_eq!(before, after, "追踪删除不改变坐标流长度");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p[1]/w:del)", ["1"]),
            ("count(//w:p[1]/w:del/w:r/w:delText)", ["1"]),
            ("count(//w:p[1]/w:del/w:r/w:t)", ["0"]),
        ]
    );
}

/// 同作者规则：落在**自己**的 `w:ins` 里插字 → 直接插，不套第二层。
#[test]
fn same_author_insert_inside_own_ins() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "AA".into(), props: None },
        &tracked(A),
    )
    .expect("第一次");
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 4), text: "BB".into(), props: None },
        &tracked(A),
    )
    .expect("第二次");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:ins)", ["1"]), ("count(//w:ins//w:ins)", ["0"]),]
    );
}

/// 同作者规则：删掉**自己**插的字 → 真删，不留 `w:del`。
#[test]
fn same_author_delete_own_insertion() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "XY".into(), props: None },
        &tracked(A),
    )
    .expect("插入");
    let p = para(&s, 0);
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 3), to: InlinePos::new(p, 5) },
        &tracked(A),
    )
    .expect("删除");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:ins)", ["0"]),
            ("count(//w:del)", ["0"]),
            // Word 也不会把切开的 run 合回去（`fixtures/revisions/README.md`）：字符一致即可
            ("//w:p[1]/w:r/w:t/text()", ["第一段", "原文。"]),
        ]
    );
}

/// 同作者规则：删掉**别人**插的字 → `w:ins` 里嵌一个 `w:del`。
#[test]
fn other_author_delete_nests_del_in_ins() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "XY".into(), props: None },
        &tracked(B),
    )
    .expect("乙插入");
    let p = para(&s, 0);
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 3), to: InlinePos::new(p, 5) },
        &tracked(A),
    )
    .expect("甲删除");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:ins/w:del)", ["1"]),
            ("//w:ins/@w:author", [B]),
            ("//w:ins/w:del/@w:author", [A]),
            ("count(//w:ins/w:del/w:r/w:delText)", ["1"]),
        ]
    );
}

/// 同作者规则：落在**别人**的 `w:ins` 中间插字 → 拆开外层，新的 `w:ins` 夹在中间。
#[test]
fn other_author_insert_splits_outer_ins() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "乙乙乙".into(), props: None },
        &tracked(B),
    )
    .expect("乙插入");
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 4), text: "甲".into(), props: None },
        &tracked(A),
    )
    .expect("甲插入");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:ins)", ["3"]),
            ("count(//w:ins//w:ins)", ["0"]),
            ("//w:p[1]/w:ins/@w:author", [B, A, B]),
        ]
    );
}

/// 同作者规则：落在 `w:del` 里插字 → `EDIT_IN_DELETED`，状态不变。
#[test]
fn insert_inside_deleted_is_refused() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 1), to: InlinePos::new(p, 4) },
        &tracked(A),
    )
    .expect("删除");
    let fp = fingerprint(&s);
    let p = para(&s, 0);
    let err = s
        .apply(
            EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: None },
            &tracked(A),
        )
        .expect_err("删除区里不能打字");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. } if *code == rsword::DiagCode::EditInDeleted),
        "{err}"
    );
    assert_fingerprint_eq!(fp, fingerprint(&s), "EDIT-05：失败不留半修改");
}

/// `rPrChange` 记住旧格式，`PROP-05` 位置正确（`rPr` 的最后一个子元素）。
#[test]
fn tracked_run_props_records_old() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 1);
    s.apply(
        EditOp::SetRunProps {
            from: InlinePos::new(p, 0),
            to: InlinePos::new(p, 6),
            patch: RunPropsPatch { italic: Change::Set(true), ..Default::default() },
        },
        &tracked(A),
    )
    .expect("设格式");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p[2]/w:r/w:rPr/w:rPrChange)", ["1"]),
            ("count(//w:p[2]/w:r/w:rPr/w:rPrChange/w:rPr/w:b)", ["1"]),
            ("count(//w:p[2]/w:r/w:rPr/w:rPrChange/w:rPr/w:i)", ["0"]),
            ("count(//w:p[2]/w:r/w:rPr/w:i)", ["1"]),
        ]
    );
}

/// `pPrChange` 记住旧对齐。
#[test]
fn tracked_para_props_records_old() {
    let mut s = open(r#"<w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>一段</w:t></w:r></w:p>"#);
    let p = para(&s, 0);
    s.apply(
        EditOp::SetParaProps {
            part: None,
            para: p,
            patch: ParaPropsPatch { jc: Change::Set(Val::Value(Jc::Center)), ..Default::default() },
        },
        &tracked(A),
    )
    .expect("设对齐");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//w:p[1]/w:pPr/w:jc/@w:val", ["center"]),
            ("//w:p[1]/w:pPr/w:pPrChange/w:pPr/w:jc/@w:val", ["left"]),
            ("//w:p[1]/w:pPr/w:pPrChange/@w:author", [A]),
        ]
    );
}

/// 修订 `w:id` 按 `EDIT-06` 全局递增、互不相同。
#[test]
fn revision_ids_are_unique() {
    let mut s = open(TWO_PARAS);
    for i in 0..3 {
        let p = para(&s, 0);
        s.apply(
            EditOp::InsertText { at: InlinePos::new(p, i * 2), text: "x".into(), props: None },
            &tracked(if i == 1 { B } else { A }),
        )
        .expect("插字");
    }
    let ids: Vec<Option<u32>> = s.document().revisions.entries().iter().map(|e| e.w_id()).collect();
    let mut sorted: Vec<u32> = ids.iter().flatten().copied().collect();
    let n = sorted.len();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), n, "修订 id 互不相同：{ids:?}");
    assert!(
        s.document().revisions.entries().iter().all(|e| e.kind == RevKind::RunInsert),
        "三次插入都是 run 级插入"
    );
}

// ---- 门 1：语料上的三条 oracle ------------------------------------------------------------------

/// 在一份真实语料上跑三条 oracle。`op` 返回 `None` 或不追踪那一遍就失败 → 跳过（这份语料
/// 不适合这个操作，例如块字段的结果段落只读）。返回是否真的跑了。
fn oracle_on(bytes: &[u8], what: &str, op: impl Fn(&EditSession) -> Option<EditOp>) -> bool {
    let Ok(base) = EditSession::open(bytes) else { return false };
    let before = fingerprint(&base);

    let mut plain = EditSession::open(bytes).expect("open");
    let Some(plain_op) = op(&plain) else { return false };
    if plain.apply(plain_op, &EditContext::default()).is_err() {
        return false;
    }
    let plain_fp = fingerprint(&plain);

    let mut track = EditSession::open(bytes).expect("open");
    let track_op = op(&track).expect("同一份文档，位置也在");
    track
        .apply(track_op, &tracked(A))
        .unwrap_or_else(|e| panic!("{what}：不追踪成功、追踪失败 {e}"));
    let track_fp = fingerprint(&track);

    if let Some((x, y)) = diff_str(&before.reject, &track_fp.reject) {
        panic!("{what}：拒绝视图没回到操作前\n  操作前: {x}\n  追踪后: {y}");
    }
    if let Some((x, y)) = diff_str(&plain_fp.accept, &track_fp.accept) {
        panic!("{what}：接受视图与不追踪不同\n  不追踪: {x}\n  追踪: {y}");
    }
    let saved = track.save().unwrap_or_else(|e| panic!("{what} 保存：{e}"));
    let reopened = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what} 重解析：{e}"));
    if let Some((view, x, y)) = track_fp.diff(&fingerprint(&reopened)) {
        panic!("{what}：往返后指纹变了\n  视图 {view}\n  保存前: {x}\n  重解析: {y}");
    }
    true
}

/// 文本域语料里第一个够长、且在 body 顶层的文本段落。
fn first_long_para(s: &EditSession) -> Option<(NodeId, u32)> {
    s.document().main.iter().find_map(|b| {
        let t = b.as_text()?;
        let len: u32 = t.text().encode_utf16().count() as u32;
        (len >= 6).then_some((t.node, len))
    })
}

/// 一个可以在任意语料上跑的追踪操作。
type CorpusOp = (&'static str, fn(&EditSession) -> Option<EditOp>);

/// 门 1：七个操作 × ≥ 20 份语料。
#[test]
fn gate_1_oracles_over_corpus() {
    let mut docs: Vec<std::path::PathBuf> = common::docx_paths("synthetic");
    docs.extend(common::docx_paths("real"));
    docs.sort();
    let ops: [CorpusOp; 7] = [
        ("InsertText", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::InsertText {
                at: InlinePos::new(p, 1), text: "追踪".into(), props: None
            })
        }),
        ("InsertText@0", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::InsertText { at: InlinePos::new(p, 0), text: "头".into(), props: None })
        }),
        ("DeleteRange", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::DeleteRange { from: InlinePos::new(p, 1), to: InlinePos::new(p, 3) })
        }),
        ("SetRunProps", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::SetRunProps {
                from: InlinePos::new(p, 1),
                to: InlinePos::new(p, 3),
                patch: RunPropsPatch { bold: Change::Set(true), ..Default::default() },
            })
        }),
        ("SetParaProps", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::SetParaProps {
                part: None,
                para: p,
                patch: ParaPropsPatch {
                    jc: Change::Set(Val::Value(Jc::Center)),
                    ..Default::default()
                },
            })
        }),
        ("SplitParagraph", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::SplitParagraph { at: InlinePos::new(p, 2) })
        }),
        ("MergeWithNext", |s| {
            let (p, _) = first_long_para(s)?;
            Some(EditOp::MergeWithNext { part: None, para: p })
        }),
    ];
    let mut ran: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut docs_used = 0usize;
    for path in &docs {
        if docs_used >= 40 {
            break;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut any = false;
        for (what, op) in &ops {
            if oracle_on(&bytes, &format!("{name} / {what}"), op) {
                *ran.entry(what).or_default() += 1;
                any = true;
            }
        }
        if any {
            docs_used += 1;
        }
    }
    assert!(docs_used >= 20, "只在 {docs_used} 份语料上跑成了（要 ≥ 20）");
    for (what, n) in &ran {
        assert!(*n >= 10, "{what} 只跑了 {n} 份（要 ≥ 10）");
    }
}

// ---- 7.2b：字段类操作、`ReplaceInlines` 的 diff、compat 的 `rPrChange` -------------------------

const FIELD_PARA: &str = concat!(
    "<w:p><w:r><w:t>前 </w:t></w:r>",
    r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
    r#"<w:r><w:instrText xml:space="preserve"> HYPERLINK "https://a.example/" </w:instrText></w:r>"#,
    r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
    "<w:r><w:t>链接文字</w:t></w:r>",
    r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
    "<w:r><w:t> 后</w:t></w:r></w:p>",
);

fn first_field(s: &EditSession) -> rsword::span::FieldId {
    s.document().fields.roots().next().expect("有字段").id
}

/// 追踪时 `InsertField` 的全套结构 run 进一个 `w:ins`。
#[test]
fn tracked_insert_field_wraps_in_ins() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertField {
            at: InlinePos::new(p, 3),
            field: rsword::edit::NewField {
                instr: "PAGE".into(),
                result: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("1"))],
                mark_dirty: false,
            },
        },
        &tracked(A),
    )
    .expect("插字段");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p[1]/w:ins)", ["1"]),
            ("count(//w:p[1]/w:ins/w:r/w:fldChar)", ["3"]),
            ("count(//w:p[1]/w:ins/w:r/w:instrText)", ["1"]),
        ]
    );
}

/// 追踪时 `SetLinkTarget`：旧指令 run 进 `w:del` 并改名，新指令 run 进 `w:ins`。
#[test]
fn tracked_set_link_target_marks_instruction() {
    let mut s = open(FIELD_PARA);
    let id = first_field(&s);
    s.apply(
        EditOp::SetLinkTarget {
            link: rsword::edit::LinkRef::Field(id),
            target: rsword::edit::LinkDest::Url("https://b.example/".into()),
        },
        &tracked(A),
    )
    .expect("改链接");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:del/w:r/w:delInstrText)", ["1"]),
            ("count(//w:ins/w:r/w:instrText)", ["1"]),
            ("count(//w:r/w:instrText)", ["1"]),
        ]
    );
    let reopened = EditSession::open(&out).unwrap();
    let f = reopened.document().fields.roots().next().expect("字段还在");
    assert!(f.instr.raw.contains("b.example"), "有效指令是新的：{:?}", f.instr.raw);
    assert!(!f.instr.raw.contains("a.example"), "删掉的指令不进有效指令：{:?}", f.instr.raw);
}

/// 追踪时 `SetFormText`：旧结果 run 进 `w:del`，新结果 run 进 `w:ins`。
#[test]
fn tracked_set_form_text() {
    let mut s = open(FIELD_PARA);
    let id = first_field(&s);
    s.apply(EditOp::SetFormText { field: id, text: "新结果".into() }, &tracked(A)).expect("改结果");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:del/w:r/w:delText)", ["1"]),
            ("//w:del/w:r/w:delText/text()", ["链接文字"]),
            ("//w:ins/w:r/w:t/text()", ["新结果"]),
        ]
    );
}

/// 追踪时 `ReplaceInlines` 的 diff 聚到 run 边界：没变的 run 原节点原字节，改了的删 + 插。
#[test]
fn tracked_replace_inlines_diffs_to_run_boundaries() {
    use rsword::edit::{NewInline, NewRun};
    let body = concat!(
        "<w:p><w:r><w:t>甲</w:t></w:r>",
        "<w:r><w:t>乙</w:t></w:r>",
        "<w:r><w:t>丙</w:t></w:r></w:p>",
    );
    let mut s = open(body);
    let p = para(&s, 0);
    s.apply(
        EditOp::ReplaceInlines {
            part: None,
            para: p,
            inlines: vec![
                NewInline::Run(NewRun::text("甲")),
                NewInline::Run(NewRun::text("新")),
                NewInline::Run(NewRun::text("丙")),
            ],
        },
        &tracked(A),
    )
    .expect("替换");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            // 只有中间那个 run 被换掉
            ("count(//w:del)", ["1"]),
            ("count(//w:ins)", ["1"]),
            ("//w:del/w:r/w:delText/text()", ["乙"]),
            ("//w:ins/w:r/w:t/text()", ["新"]),
            // 首尾两个 run 原样留在段落里
            ("//w:p/w:r/w:t/text()", ["甲", "丙"]),
        ]
    );
}

/// 三条 oracle 对 `ReplaceInlines`（段落里没有范围标记时）。
#[test]
fn oracle_replace_inlines() {
    use rsword::edit::{NewInline, NewRun};
    let body = concat!(
        "<w:p><w:r><w:t>一</w:t></w:r>",
        "<w:r><w:t>二</w:t></w:r>",
        "<w:r><w:t>三</w:t></w:r></w:p>",
    );
    oracle(body, "ReplaceInlines", |s| EditOp::ReplaceInlines {
        part: None,
        para: para(s, 0),
        inlines: vec![
            NewInline::Run(NewRun::text("一")),
            NewInline::Run(NewRun::text("贰")),
            NewInline::Run(NewRun::text("三")),
        ],
    });
}

/// compat 的 `runs[].rPrChange` 重发（7.2b：`save_blocks.rs` 不再拒绝）。
#[test]
fn compat_run_rpr_change_round_trips() {
    let bytes = common::docx_with_body("<w:p><w:r><w:t>格式改过的</w:t></w:r></w:p>");
    let mut s = EditSession::open(&bytes).unwrap();
    let blocks = serde_json::json!([{
        "kind": "generated",
        "block": { "type": "paragraph", "runs": [{
            "text": "格式改过的",
            "bold": true,
            "rPrChange": {
                "author": A,
                "date": DATE,
                "old": { "italic": true, "sizeHalfPoints": 22, "color": "FF0000" }
            }
        }] }
    }]);
    let outcome =
        rsword::bind::compat_ts::apply_save_blocks(&mut s, &blocks, &serde_json::json!({}))
            .expect("rPrChange 不再被拒绝");
    assert!(!outcome.unchanged);
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:r/w:rPr/w:rPrChange)", ["1"]),
            ("//w:r/w:rPr/w:rPrChange/@w:author", [A]),
            ("//w:r/w:rPr/w:rPrChange/@w:date", [DATE]),
            ("count(//w:r/w:rPr/w:rPrChange/w:rPr/w:i)", ["1"]),
            ("//w:r/w:rPr/w:rPrChange/w:rPr/w:color/@w:val", ["FF0000"]),
            ("//w:r/w:rPr/w:rPrChange/w:rPr/w:sz/@w:val", ["22"]),
            ("count(//w:r/w:rPr/w:b)", ["1"]),
        ]
    );
    // 重解析：模型认得这条修订
    let re = EditSession::open(&out).unwrap();
    let e = re.document().revisions.entries().first().expect("有一条修订");
    assert_eq!(e.kind, RevKind::RunPropsChange);
    assert_eq!(e.author(), Some(A));
}
