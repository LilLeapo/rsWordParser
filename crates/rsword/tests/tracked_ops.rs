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

// 语料里已经有「作者甲」/「作者乙」的修订：测试用的作者名要与它们不同，
// 否则按作者过滤的 oracle 会把语料自带的修订也一起处理掉
const A: &str = "M7 甲";
const B: &str = "M7 乙";
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

    // ①' / ②' 视图代理（便宜的信号）
    if let Some((x, y)) = diff_str(&before.reject, &track_fp.reject) {
        panic!("{what}：拒绝视图没回到操作前\n  操作前: {x}\n  追踪后: {y}");
    }
    if let Some((x, y)) = diff_str(&plain_fp.accept, &track_fp.accept) {
        panic!("{what}：接受视图与不追踪不同\n  不追踪: {x}\n  追踪: {y}");
    }
    // ① **真的** `RejectAll { author: A }` → 回到操作前
    let mut rej = open(body);
    rej.apply(op(&rej), &tracked(A)).expect("追踪重放");
    rej.apply(EditOp::RejectAll { author: Some(A.into()) }, &EditContext::default())
        .unwrap_or_else(|e| panic!("{what} RejectAll: {e}"));
    assert_fingerprint_eq!(before, fingerprint(&rej), "{what}：RejectAll 没回到操作前");
    // ② **真的** `AcceptAll` → 等于不追踪做一遍
    let mut acc = open(body);
    acc.apply(op(&acc), &tracked(A)).expect("追踪重放");
    // 只接受**本次**操作的作者：语料本来就带别人的修订，不追踪那条路也没动它们
    acc.apply(EditOp::AcceptAll { author: Some(A.into()) }, &EditContext::default())
        .unwrap_or_else(|e| panic!("{what} AcceptAll: {e}"));
    assert_fingerprint_eq!(plain_fp, fingerprint(&acc), "{what}：AcceptAll 与不追踪不同");
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

    // 语料上只跑**真的** `RejectAll` / `AcceptAll`（门 1 的正式形态）。视图代理是 7.4 落地
    // 之前的脚手架，它对"接受段落标记的删除"这类结构变化只是近似建模，语料里各式各样的
    // 段落属性组合会让近似和真实结果对不上——真实的那两条才是判据。
    let mut rej = EditSession::open(bytes).expect("open");
    rej.apply(op(&rej).expect("同一份文档"), &tracked(A)).expect("追踪重放");
    rej.apply(EditOp::RejectAll { author: Some(A.into()) }, &EditContext::default())
        .unwrap_or_else(|e| panic!("{what} RejectAll: {e}"));
    assert_fingerprint_eq!(before, fingerprint(&rej), "{what}：RejectAll 没回到操作前");
    let mut acc = EditSession::open(bytes).expect("open");
    acc.apply(op(&acc).expect("同一份文档"), &tracked(A)).expect("追踪重放");
    // 只接受**本次**操作的作者：语料本来就带别人的修订，不追踪那条路也没动它们
    acc.apply(EditOp::AcceptAll { author: Some(A.into()) }, &EditContext::default())
        .unwrap_or_else(|e| panic!("{what} AcceptAll: {e}"));
    assert_fingerprint_eq!(plain_fp, fingerprint(&acc), "{what}：AcceptAll 与不追踪不同");
    let saved = track.save().unwrap_or_else(|e| panic!("{what} 保存：{e}"));
    let reopened = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what} 重解析：{e}"));
    if let Some((view, x, y)) = track_fp.diff(&fingerprint(&reopened)) {
        panic!("{what}：往返后指纹变了\n  视图 {view}\n  保存前: {x}\n  重解析: {y}");
    }
    true
}

/// 这棵子树里**一条未解决的修订都没有**。
///
/// 门 1 的 oracle 要求这次操作产生的修订能按作者单独拒绝 / 接受，而 Word 的模型里
/// **一个容器只能带一条同类修订**（`w:rPr` 只有一份 `rPrChange`、一行只有一个
/// `w:ins` / `w:del`）。容器上已经有别人未解决的修订时，我们这次的改动挂不上自己的标记，
/// 也就无法单独回退——那不是缺陷，是 `*PrChange` / 行标记模型本身的性质（登记在 `docs/04` §8）。
fn revision_free(s: &EditSession, node: NodeId) -> bool {
    let Some(dom) = s.package().part(s.document().main_part).dom() else { return true };
    let inside = |n: NodeId| n == node || dom.ancestors(n).any(|a| a == node);
    !s.document().revisions.entries().iter().any(|e| inside(e.node()))
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
            if !revision_free(s, p) {
                return None;
            }
            Some(EditOp::SetRunProps {
                from: InlinePos::new(p, 1),
                to: InlinePos::new(p, 3),
                patch: RunPropsPatch { bold: Change::Set(true), ..Default::default() },
            })
        }),
        ("SetParaProps", |s| {
            let (p, _) = first_long_para(s)?;
            if !revision_free(s, p) {
                return None;
            }
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

// ---- 7.3：块、表格、节、其他 part -------------------------------------------------------------

const TABLE_DOC: &str = concat!(
    r#"<w:p><w:r><w:t>表前一段</w:t></w:r></w:p>"#,
    r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
    r#"<w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>"#,
    r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr>"#,
    r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
    r#"<w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    r#"<w:p><w:r><w:t>表后一段</w:t></w:r></w:p>"#,
);

fn table_block(s: &EditSession) -> &rsword::model::table::TableBlock {
    s.document()
        .main
        .iter()
        .find_map(|b| match b {
            rsword::model::Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("有表格")
}

fn first_table(s: &EditSession) -> NodeId {
    table_block(s).node
}

/// 三条 oracle × 表格与块操作。
#[test]
fn oracle_table_and_block_ops() {
    oracle(TABLE_DOC, "InsertRow", |s| EditOp::InsertRow {
        table: first_table(s),
        at: 1,
        template: None,
    });
    oracle(TABLE_DOC, "DeleteRow", |s| EditOp::DeleteRow { table: first_table(s), at: 1 });
    oracle(TABLE_DOC, "InsertColumn", |s| EditOp::InsertColumn {
        table: first_table(s),
        at: 1,
        width: 2000,
    });
    oracle(TABLE_DOC, "DeleteColumn", |s| EditOp::DeleteColumn { table: first_table(s), at: 1 });
    oracle(TABLE_DOC, "SetTableProps", |s| EditOp::SetTableProps {
        table: first_table(s),
        patch: rsword::semantic::props::TablePropsPatch {
            style: Change::Set("TableGrid".into()),
            ..Default::default()
        },
    });
    oracle(TABLE_DOC, "DeleteBlock 段落", |s| EditOp::DeleteBlock {
        part: None,
        node: para(s, 0),
    });
    oracle(TABLE_DOC, "DeleteBlock 表格", |s| EditOp::DeleteBlock {
        part: None,
        node: first_table(s),
    });
    oracle(TABLE_DOC, "InsertBlock 段落", |s| EditOp::InsertBlock {
        at: rsword::edit::BlockPos::after(para(s, 0)),
        block: rsword::edit::NewBlock::Paragraph {
            props: None,
            inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("新段落"))],
        },
    });
    oracle(TABLE_DOC, "InsertBlock 表格", |s| EditOp::InsertBlock {
        at: rsword::edit::BlockPos::after(para(s, 0)),
        block: rsword::edit::NewBlock::Table {
            rows: 2,
            cols: 2,
            widths: None,
            style: None,
            header: false,
        },
    });
}

/// `EDIT-03` 表格验收行的追踪版：新行带 `trPr/w:ins`，`tcPr` 与模板行字节相同。
#[test]
fn edit_03_tracked_insert_row() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    s.apply(EditOp::InsertRow { table, at: 1, template: None }, &tracked(A)).expect("插行");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:tbl/w:tr)", ["3"]),
            ("count(//w:tbl/w:tr/w:trPr/w:ins)", ["1"]),
            ("//w:tbl/w:tr[2]/w:trPr/w:ins/@w:author", [A]),
            // 模板行的 `tcPr` 原样克隆
            ("//w:tbl/w:tr[2]/w:tc/w:tcPr/w:tcW/@w:w", ["4000", "4000"]),
        ]
    );
}

/// `DeleteRow` 追踪版：行仍在且带 `trPr/w:del`。
#[test]
fn edit_03_tracked_delete_row() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    s.apply(EditOp::DeleteRow { table, at: 1 }, &tracked(A)).expect("删行");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:tbl/w:tr)", ["2"]),
            ("count(//w:tbl/w:tr[2]/w:trPr/w:del)", ["1"]),
            ("//w:tbl/w:tr[2]/w:tc/w:p/w:r/w:t/text()", ["A2", "B2"]),
        ]
    );
}

/// `InsertColumn` 追踪版：新格带 `tcPr/w:cellIns`，`tblGridChange` 里是旧网格。
#[test]
fn edit_03_tracked_insert_column() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    s.apply(EditOp::InsertColumn { table, at: 1, width: 2000 }, &tracked(A)).expect("插列");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(/w:document/w:body/w:tbl/w:tblGrid/w:gridCol)", ["3"]),
            ("count(/w:document/w:body/w:tbl/w:tblGrid/w:tblGridChange)", ["1"]),
            ("count(//w:tblGrid/w:tblGridChange/w:tblGrid/w:gridCol)", ["2"]),
            ("//w:tblGrid/w:tblGridChange/w:tblGrid/w:gridCol/@w:w", ["4000", "4000"]),
            ("count(//w:tc/w:tcPr/w:cellIns)", ["2"]),
        ]
    );
}

/// `DeleteColumn` 追踪版：该列的 `w:tc` 仍在且带 `w:cellDel`，网格不动。
#[test]
fn edit_03_tracked_delete_column() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    s.apply(EditOp::DeleteColumn { table, at: 1 }, &tracked(A)).expect("删列");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(/w:document/w:body/w:tbl/w:tblGrid/w:gridCol)", ["2"]),
            ("count(//w:tc/w:tcPr/w:cellDel)", ["2"]),
            ("//w:tbl/w:tr[1]/w:tc/w:p/w:r/w:t/text()", ["A1", "B1"]),
        ]
    );
}

/// 追踪时删段落：段落留着、内容进 `w:del`、段落标记 `w:del`。
#[test]
fn edit_03_tracked_delete_paragraph_keeps_it() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(EditOp::DeleteBlock { part: None, node: p }, &tracked(A)).expect("删段落");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["2"]),
            ("count(//w:p[1]/w:del/w:r/w:delText)", ["1"]),
            ("count(//w:p[1]/w:pPr/w:rPr/w:del)", ["1"]),
        ]
    );
}

/// 追踪时删表格：行都留着，每行 `trPr/w:del`。
#[test]
fn edit_03_tracked_delete_table_marks_rows() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    s.apply(EditOp::DeleteBlock { part: None, node: table }, &tracked(A)).expect("删表");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:tbl)", ["1"]),
            ("count(//w:tbl/w:tr)", ["2"]),
            ("count(//w:tbl/w:tr/w:trPr/w:del)", ["2"]),
        ]
    );
}

/// 追踪时插段落：内容进 `w:ins`，段落标记也标插入。
#[test]
fn edit_03_tracked_insert_paragraph() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos::after(p),
            block: rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("新段"))],
            },
        },
        &tracked(A),
    )
    .expect("插段落");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["3"]),
            ("count(//w:p[2]/w:ins/w:r/w:t)", ["1"]),
            ("//w:p[2]/w:ins/w:r/w:t/text()", ["新段"]),
            ("count(//w:p[2]/w:pPr/w:rPr/w:ins)", ["1"]),
        ]
    );
}

/// 追踪时插表格：每行 `trPr/w:ins`。
#[test]
fn edit_03_tracked_insert_table() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos::after(p),
            block: rsword::edit::NewBlock::Table {
                rows: 2,
                cols: 2,
                widths: None,
                style: None,
                header: false,
            },
        },
        &tracked(A),
    )
    .expect("插表");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:tbl/w:tr)", ["2"]), ("count(//w:tbl/w:tr/w:trPr/w:ins)", ["2"]),]
    );
}

/// `SetTableProps` / `SetRowProps` / `SetCellProps` 追踪版：三种 `*PrChange` 各记旧值。
#[test]
fn edit_03_tracked_table_props_changes() {
    let mut s = open(TABLE_DOC);
    let table = first_table(&s);
    let (row, cell) = {
        let t = table_block(&s);
        (t.rows[0].node, t.rows[0].cells[0].node)
    };
    s.apply(
        EditOp::SetTableProps {
            table,
            patch: rsword::semantic::props::TablePropsPatch {
                style: Change::Set("TableGrid".into()),
                ..Default::default()
            },
        },
        &tracked(A),
    )
    .expect("表格属性");
    s.apply(
        EditOp::SetRowProps {
            row,
            patch: rsword::semantic::props::RowPropsPatch {
                tbl_header: Change::Set(true),
                ..Default::default()
            },
        },
        &tracked(A),
    )
    .expect("行属性");
    s.apply(
        EditOp::SetCellProps {
            cell,
            patch: rsword::semantic::props::CellPropsPatch {
                v_align: Change::Set(Val::Value(rsword::semantic::props::VerticalJc::Center)),
                ..Default::default()
            },
        },
        &tracked(A),
    )
    .expect("单元格属性");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:tblPr/w:tblPrChange)", ["1"]),
            ("count(//w:tblPr/w:tblPrChange/w:tblPr/w:tblW)", ["1"]),
            ("count(//w:tblPr/w:tblPrChange/w:tblPr/w:tblStyle)", ["0"]),
            ("count(//w:tr[1]/w:trPr/w:trPrChange)", ["1"]),
            ("count(//w:tr[1]/w:tc[1]/w:tcPr/w:tcPrChange)", ["1"]),
            ("count(//w:tr[1]/w:tc[1]/w:tcPr/w:tcPrChange/w:tcPr/w:tcW)", ["1"]),
        ]
    );
}

/// 追踪时的 `SetSectionProps` → `w:sectPrChange`，快照里没有页眉页脚引用。
#[test]
fn edit_03_tracked_section_props() {
    let mut s = open(concat!(
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
    ));
    let sect = s.document().sections[0].node.expect("body 级 sectPr");
    s.apply(
        EditOp::SetSectionProps {
            sect,
            patch: rsword::semantic::props::SectionPropsPatch {
                kind: Change::Set(Val::Value(rsword::semantic::props::SectType::NextPage)),
                ..Default::default()
            },
        },
        &tracked(A),
    )
    .expect("节属性");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:sectPr/w:sectPrChange)", ["1"]),
            ("//w:sectPr/w:sectPrChange/@w:author", [A]),
            ("count(//w:sectPr/w:sectPrChange/w:sectPr/w:headerReference)", ["0"]),
            ("count(//w:sectPr/w:sectPrChange/w:sectPr/w:pgSz)", ["1"]),
        ]
    );
}

/// tracked `MoveBlock` / `MergeCells` → `Err` 且状态一点没动（`EDIT-05`）。
#[test]
fn tracked_move_and_merge_are_refused() {
    let mut s = open(TABLE_DOC);
    let fp = fingerprint(&s);
    let p = para(&s, 0);
    let last = para(&s, 1);
    let err = s
        .apply(
            EditOp::MoveBlock { from: None, node: p, to: rsword::edit::BlockPos::after(last) },
            &tracked(A),
        )
        .expect_err("追踪时不支持 MoveBlock");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditUnsupportedTrackedMove),
        "{err}"
    );
    assert_fingerprint_eq!(fp, fingerprint(&s), "MoveBlock 被拒后状态不变");

    let table = first_table(&s);
    let err = s
        .apply(EditOp::MergeCells { table, from: (0, 0), to: (0, 1) }, &tracked(A))
        .expect_err("追踪时不支持 MergeCells");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditUnsupportedTrackedMerge),
        "{err}"
    );
    assert_fingerprint_eq!(fp, fingerprint(&s), "MergeCells 被拒后状态不变");
}

/// 不追踪的那批操作：照常执行 + 一条 `REV_NOT_TRACKED`（分层决策 5）。
#[test]
fn rev_not_tracked_is_recorded_but_the_op_runs() {
    let mut s = open(TWO_PARAS);
    s.apply(EditOp::SetPageColor { color: Some("FFFF00".into()) }, &tracked(A))
        .expect("开着修订也能改页面底色");
    let notes: Vec<_> =
        s.diagnostics().iter().filter(|d| d.code == rsword::DiagCode::RevNotTracked).collect();
    assert_eq!(notes.len(), 1, "留一条记录：{:?}", s.diagnostics());
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//w:background/@w:color", ["FFFF00"]),
            ("count(//w:ins)", ["0"]),
            ("count(//w:del)", ["0"]),
        ]
    );
}

/// 门 1：表格 / 块操作 × 语料里带表格的文档（DoD 要求 ≥ 10 份）。
#[test]
fn gate_1_oracles_over_table_corpus() {
    let mut docs: Vec<std::path::PathBuf> = common::docx_paths("synthetic");
    docs.extend(common::docx_paths("real"));
    docs.sort();
    let ops: [CorpusOp; 5] = [
        ("InsertRow", |s| Some(EditOp::InsertRow { table: any_table(s)?, at: 1, template: None })),
        ("DeleteRow", |s| Some(EditOp::DeleteRow { table: any_table(s)?, at: 0 })),
        // 容器上已经有未解决的 `tblPrChange` 时，再改属性不新建快照（Word 也是这样：
        // 一个容器只留最早的那份旧值），那次改动就没法按作者单独拒绝——跳过这种文档
        ("SetTableProps", |s| {
            Some(EditOp::SetTableProps {
                table: any_table(s)?,
                patch: rsword::semantic::props::TablePropsPatch {
                    style: Change::Set("TableGrid".into()),
                    ..Default::default()
                },
            })
        }),
        ("DeleteBlock 表格", |s| Some(EditOp::DeleteBlock { part: None, node: any_table(s)? })),
        // 段落里不能有范围标记：不追踪那条路会按 `SPAN-07` 把整条批注 / 书签删掉，
        // 追踪那条路内容还在、标记必须留着（拒绝时要能回来）。两者只有在**真的**
        // `AcceptAll`（7.4）之后才等价，视图代理比不出来
        ("DeleteBlock 段落", |s| {
            let p = plain_para(s)?;
            Some(EditOp::DeleteBlock { part: None, node: p })
        }),
    ];
    let mut docs_used = 0usize;
    let mut ran: std::collections::BTreeMap<&str, usize> = Default::default();
    for path in &docs {
        if docs_used >= 25 {
            break;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(probe) = EditSession::open(&bytes) else { continue };
        if any_table(&probe).is_none() {
            continue;
        }
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
    assert!(docs_used >= 10, "只在 {docs_used} 份带表格的语料上跑成了（要 ≥ 10）");
    for (what, n) in &ran {
        assert!(*n >= 5, "{what} 只跑了 {n} 份（要 ≥ 5）");
    }
}

/// 第一个够长的顶层文本段落，但**整份文档**都不能有范围标记。
///
/// 视图代理比不出 `SPAN-07`：不追踪删一个块会把落在里面的批注 / 书签整条删掉，追踪时内容
/// 还在、标记必须留着（拒绝要能回来）。两者只有在**真的** `AcceptAll`（7.4）之后才等价。
fn plain_para(s: &EditSession) -> Option<NodeId> {
    let main = s.document().main_part;
    let dom = s.package().part(main).dom()?;
    if dom.descendants(dom.root()).any(|n| dom.name(n).is_some_and(rsword::span::is_range_marker)) {
        return None;
    }
    s.document()
        .main
        .iter()
        .filter_map(|b| b.as_text().filter(|t| t.text().encode_utf16().count() >= 6))
        .map(|t| t.node)
        .find(|&n| revision_free(s, n))
}

/// 顶层第一张至少两行、且不带未解决修订的表（见 [`revision_free`]）。
fn any_table(s: &EditSession) -> Option<NodeId> {
    s.document()
        .main
        .iter()
        .filter_map(|b| match b {
            rsword::model::Block::Table(t) if t.rows.len() >= 2 => Some(t.node),
            _ => None,
        })
        .find(|&n| revision_free(s, n))
}

/// 追踪时 `SetHeaderFooter`：在页眉 part 里按段落规则 del + ins。
#[test]
fn tracked_set_header_footer_marks_the_part() {
    let mut s = open(concat!(
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
    ));
    let sect = s.document().sections[0].node.expect("body 级 sectPr");
    // 先不追踪建一个页眉，再追踪着改它的内容
    s.apply(
        EditOp::SetHeaderFooter {
            sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            content: vec![rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("旧页眉"))],
            }],
        },
        &EditContext::default(),
    )
    .expect("建页眉");
    let sect = s.document().sections[0].node.expect("sectPr");
    s.apply(
        EditOp::SetHeaderFooter {
            sect,
            kind: rsword::model::HfKind::Header,
            variant: rsword::model::HfVariant::Default,
            content: vec![rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("新页眉"))],
            }],
        },
        &tracked(A),
    )
    .expect("改页眉");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/header1.xml",
        [
            ("count(//w:hdr/w:p)", ["2"]),
            ("//w:del/w:r/w:delText/text()", ["旧页眉"]),
            ("//w:ins/w:r/w:t/text()", ["新页眉"]),
            ("count(//w:p[1]/w:pPr/w:rPr/w:del)", ["1"]),
            ("count(//w:p[2]/w:pPr/w:rPr/w:ins)", ["1"]),
        ]
    );
    // 修订索引跨 part：这些条目在页眉 part 名下
    let re = EditSession::open(&out).unwrap();
    let hf_part = *re.document().hf_parts.keys().next().expect("有页眉 part");
    assert!(re.document().revisions.of_part(hf_part).count() >= 4, "页眉里的修订进了索引");
}

/// 追踪时 `ReplaceImageMedia`：旧 run 进 `w:del`，换了图的克隆 run 进 `w:ins`。
#[test]
fn tracked_replace_image_media() {
    let png = common::b64(common::PNG_1X1);
    let body = concat!(
        r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">"#,
        r#"<wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="p1"/>"#,
        r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
        r#"<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
        r#"<pic:nvPicPr><pic:cNvPr id="1" name="p1"/><pic:cNvPicPr/></pic:nvPicPr>"#,
        r#"<pic:blipFill><a:blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="rIdImg"/>"#,
        r#"<a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
        r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm>"#,
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
        r#"</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
    );
    let bytes =
        common::with_binary_part(&common::docx_with_body(body), "word/media/image1.png", &png);
    let mut s = EditSession::open(&bytes).unwrap();
    let main = s.document().main_part;
    let dom = s.package().part(main).dom().expect("主 part");
    let root = dom.root();
    let drawing = dom
        .descendants(root)
        .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::Drawing)))
        .expect("有 w:drawing");
    s.apply(
        EditOp::ReplaceImageMedia { drawing, bytes: png.clone(), mime: "image/png".into() },
        &tracked(A),
    )
    .expect("换图");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:del/w:r/w:drawing)", ["1"]), ("count(//w:ins/w:r/w:drawing)", ["1"]),]
    );
}

/// `NewBlock::Xml` 追踪时整块包进块级 `w:ins`（TS 的形态，解析器已认）。
#[test]
fn tracked_insert_xml_block_wraps_at_block_level() {
    let mut s = open(TWO_PARAS);
    let p = para(&s, 0);
    let frag = {
        let main = s.document().main_part;
        let dom = s.package_mut().dom_mut(main).expect("主 part").expect("已解析");
        rsword::xml::parse_fragment(
            dom,
            r#"<w:tbl xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>X</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        )
        .expect("片段")
        .into_iter()
        .next()
        .expect("一个元素")
    };
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos::after(p),
            block: rsword::edit::NewBlock::Xml(frag),
        },
        &tracked(A),
    )
    .expect("插片段");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(/w:document/w:body/w:ins/w:tbl)", ["1"]),
            ("/w:document/w:body/w:ins/@w:author", [A]),
        ]
    );
    let re = EditSession::open(&out).unwrap();
    let kinds: Vec<&str> =
        re.document().revisions.entries().iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"insert"), "块级插入修订：{kinds:?}");
}
