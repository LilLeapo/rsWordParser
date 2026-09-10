//! 修订索引（`MOD-09` / `MOD-13` / `EDIT-06`，`spec/18` 7.1）与修订相关病态输入（`TEST-09`，7.0⑤）。

mod common;

#[cfg(feature = "compat-ts")]
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[cfg(feature = "compat-ts")]
use rsword::bind::compat_ts::parsed_doc;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
#[cfg(feature = "compat-ts")]
use rsword::model::RevisionIndex;
use rsword::model::{RevKind, RevisionEntry};
use rsword::package::Package;
#[cfg(feature = "compat-ts")]
use serde_json::{Map, Value};

/// `TEST-09` 三条：解析成功、局部降级（无引擎不变式违规）、无编辑保存字节相同。
#[test]
fn test_09_hostile_revision_documents() {
    for name in [
        "rev-nested-wrappers.docx",
        "rev-move-unpaired.docx",
        "rev-change-empty.docx",
        "rev-del-with-t.docx",
        "sectpr-in-cell.docx",
        "drawing-anchor-no-extent.docx",
    ] {
        let bytes = std::fs::read(common::corpus_dir("hostile").join(name)).unwrap();
        let mut s = EditSession::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!s.document().main.is_empty(), "{name}: 投影出块");
        assert!(
            !s.diagnostics()
                .iter()
                .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
            "{name}: {:?}",
            s.diagnostics()
        );
        assert_eq!(s.save().unwrap(), bytes, "{name}: 无编辑保存字节相同");
    }
}

// ---- 索引与投影的对照 ---------------------------------------------------------------------------

/// 一条修订的身份：`w:author` / `w:date` / `w:id` 三个属性。索引与 `compat_ts` 都按它归类——
/// TS 的 run 会按属性合并 / 拆分，个数对不上，但**身份的集合**必须逐份相同。
#[cfg(feature = "compat-ts")]
type Ident = (String, String, String);

#[cfg(feature = "compat-ts")]
fn ident_of(o: &Map<String, Value>) -> Ident {
    let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    (s("author"), s("date"), s("id"))
}

/// 从 `ParsedDoc` 的 `blocks` 子树里按键收身份（`runs[].ins / del`、`paraMarkDel`、
/// `pPrChangeInfo`、`rowRevisions[]`、`cellRevision`）。
#[cfg(feature = "compat-ts")]
fn compat_idents(blocks: &Value) -> BTreeMap<&'static str, BTreeSet<Ident>> {
    let mut out: BTreeMap<&'static str, BTreeSet<Ident>> = BTreeMap::new();
    let mut stack = vec![blocks];
    while let Some(v) = stack.pop() {
        match v {
            Value::Array(a) => stack.extend(a.iter()),
            Value::Object(o) => {
                for (k, child) in o {
                    stack.push(child);
                    let Some(m) = child.as_object() else { continue };
                    if !m.contains_key("author") {
                        continue;
                    }
                    let key = match (k.as_str(), m.get("kind").and_then(Value::as_str)) {
                        ("ins", _) => "ins",
                        ("del", _) => "del",
                        ("paraMarkDel", _) => "paraMarkDel",
                        ("pPrChangeInfo", _) => "pPrChange",
                        ("cellRevision", Some("ins")) => "cellIns",
                        ("cellRevision", Some("del")) => "cellDel",
                        _ => continue,
                    };
                    out.entry(key).or_default().insert(ident_of(m));
                }
                // `rowRevisions` 是数组，每项自带 kind
                if let Some(rows) = o.get("rowRevisions").and_then(Value::as_array) {
                    for r in rows {
                        let Some(m) = r.as_object() else { continue };
                        let key = match m.get("kind").and_then(Value::as_str) {
                            Some("ins") => "rowIns",
                            Some("del") => "rowDel",
                            _ => continue,
                        };
                        out.entry(key).or_default().insert(ident_of(m));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 索引侧的同一张表（只看主 part：`compat_ts` 的 `blocks` 就是主 part 的正文）。
#[cfg(feature = "compat-ts")]
fn index_idents(
    idx: &RevisionIndex,
    main: rsword::package::PartId,
    dom: &rsword::xml::Dom,
) -> BTreeMap<&'static str, BTreeSet<Ident>> {
    let in_cell = |n: rsword::xml::NodeId| {
        dom.ancestors(n).any(|a| dom.is(a, rsword::xml::QName::w(rsword::xml::LocalName::Tc)))
    };
    let mut out: BTreeMap<&'static str, BTreeSet<Ident>> = BTreeMap::new();
    for e in idx.of_part(main) {
        // TS 的段落 `revExtras`（`paraMarkDel` / `pPrChangeInfo`）只挂在**顶层**段落块上：
        // 单元格里的段落走表格投影，那里没有这两个键（TS 的缺口，登记在 `docs/04` §8）。
        // run 级修订在单元格里照样投影，所以只滤这两种。
        if matches!(e.kind, RevKind::ParaMarkDelete | RevKind::ParaPropsChange)
            && e.owner.node().is_some_and(in_cell)
        {
            continue;
        }
        // TS 把 `w:moveFrom` 也投成 `del`、`w:moveTo` 投成 `ins`（`RevisionCtx` 两格都填）
        let key = match (e.kind, e.owner) {
            (rsword::model::RevKind::RunInsert | rsword::model::RevKind::RunMoveTo, _) => "ins",
            (rsword::model::RevKind::RunDelete | rsword::model::RevKind::RunMoveFrom, _) => "del",
            (rsword::model::RevKind::ParaMarkDelete, _) => "paraMarkDel",
            (rsword::model::RevKind::ParaPropsChange, _) => "pPrChange",
            (rsword::model::RevKind::Insert, rsword::model::RevOwner::Row(_)) => "rowIns",
            (rsword::model::RevKind::Delete, rsword::model::RevOwner::Row(_)) => "rowDel",
            (rsword::model::RevKind::CellInsert, _) => "cellIns",
            (rsword::model::RevKind::CellDelete, _) => "cellDel",
            _ => continue,
        };
        let meta = &e.meta;
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        out.entry(key).or_default().insert((s(&meta.author), s(&meta.date), s(&meta.id)));
    }
    out
}

/// 门 1 的前置：索引与 `compat_ts` 对同一份文档看到的**同一批修订**。
#[test]
#[cfg(feature = "compat-ts")]
fn mod_09_index_matches_compat_projection() {
    let mut checked = 0usize;
    for path in revision_docs() {
        let bytes = std::fs::read(&path).unwrap();
        let name = short(&path);
        let session = EditSession::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let main = session.document().main_part;
        let dom = session.package().part(main).dom().expect("主 part 已解析");
        let mine = index_idents(&session.document().revisions, main, dom);
        let mut pkg = Package::open(&bytes).unwrap();
        let json = parsed_doc(&mut pkg).unwrap();
        let theirs = compat_idents(&json["blocks"]);
        assert_eq!(mine, theirs, "{name}");
        if !mine.is_empty() {
            checked += 1;
        }
    }
    assert!(checked >= 16, "带修订的对照文档只有 {checked} 份");
}

/// 语料里带修订的文档（合成 + 真实 + `fixtures/revisions` 四态）。
fn revision_docs() -> Vec<PathBuf> {
    let mut v = common::docx_paths("synthetic");
    v.extend(common::docx_paths("real"));
    let fx = common::repo_root().join("fixtures/revisions");
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(&fx).unwrap().filter_map(|d| d.ok().map(|d| d.path())).collect();
    dirs.sort();
    for d in dirs.into_iter().filter(|d| d.is_dir()) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|f| f.ok().map(|f| f.path()))
            .filter(|f| f.extension().is_some_and(|e| e == "docx"))
            .collect();
        files.sort();
        v.extend(files);
    }
    v
}

fn short(p: &std::path::Path) -> String {
    p.strip_prefix(common::repo_root()).unwrap_or(p).display().to_string()
}

// ---- 搬移的配对 ---------------------------------------------------------------------------------

/// 两半齐全的 move 按 `w:moveFromRangeStart/@w:name` 配上对；落单的记 `REV_UNPAIRED_MOVE`。
#[test]
fn mod_09_move_pairs() {
    for rel in
        ["corpus/real/revisions2/rev-move.docx", "fixtures/revisions/table-and-move/tracked.docx"]
    {
        let bytes = std::fs::read(common::repo_root().join(rel)).unwrap();
        let s = EditSession::open(&bytes).unwrap();
        let idx = &s.document().revisions;
        let moves: Vec<&RevisionEntry> =
            idx.entries().iter().filter(|e| e.kind.is_move()).collect();
        assert!(moves.len() >= 2, "{rel}: 应该两半都有");
        for e in &moves {
            let pair = e.pair.unwrap_or_else(|| panic!("{rel}: {:?} 没有孪生", e.kind));
            let twin = idx.get(pair).expect("孪生在索引里");
            assert_ne!(e.kind.is_move_from(), twin.kind.is_move_from(), "{rel}: 孪生方向相反");
            assert_eq!(twin.pair, Some(e.id), "{rel}: 配对是双向的");
            assert_eq!(e.move_name, twin.move_name, "{rel}: 同一个 w:name");
        }
        assert!(
            !s.document().warnings.iter().any(|d| d.code == rsword::DiagCode::RevUnpairedMove),
            "{rel}: 两半齐全不该有 REV_UNPAIRED_MOVE"
        );
    }
}

/// `hostile/rev-move-unpaired`：三种半截 move 都是 `pair = None` + 一条诊断。
#[test]
fn mod_09_unpaired_move_diagnostic() {
    let bytes =
        std::fs::read(common::corpus_dir("hostile").join("rev-move-unpaired.docx")).unwrap();
    let s = EditSession::open(&bytes).unwrap();
    let idx = &s.document().revisions;
    let moves: Vec<&RevisionEntry> = idx.entries().iter().filter(|e| e.kind.is_move()).collect();
    assert_eq!(moves.len(), 5, "四段 moveFrom + 一段 moveTo");
    assert!(moves.iter().all(|e| e.pair.is_none()), "全都配不上对");
    let warnings = &s.document().warnings;
    let unpaired: Vec<_> =
        warnings.iter().filter(|d| d.code == rsword::DiagCode::RevUnpairedMove).collect();
    assert_eq!(unpaired.len(), moves.len(), "每个落单的 move 一条诊断");
    assert!(
        unpaired.iter().all(|d| d.origin == rsword::ValidationOrigin::PreExistingDamage),
        "输入本来如此，不是引擎造成的"
    );
}

// ---- 套娃的层数 ---------------------------------------------------------------------------------

/// `hostile/rev-nested-wrappers`：500 层 `w:ins` / `w:del` 交替，层数正确、遍历不爆栈、
/// `iter_inner_first` 从最内层开始。
#[test]
fn mod_09_nested_wrappers_depth() {
    let bytes =
        std::fs::read(common::corpus_dir("hostile").join("rev-nested-wrappers.docx")).unwrap();
    let s = EditSession::open(&bytes).unwrap();
    let idx = &s.document().revisions;
    let nested: Vec<&RevisionEntry> =
        idx.entries().iter().filter(|e| e.kind.is_wrapper()).collect();
    assert_eq!(nested.len(), 500, "500 层各一条");
    for (i, e) in nested.iter().enumerate() {
        assert_eq!(e.depth as usize, i, "第 {i} 层的 depth");
        let want = if i % 2 == 0 {
            rsword::model::RevKind::RunInsert
        } else {
            rsword::model::RevKind::RunDelete
        };
        assert_eq!(e.kind, want, "第 {i} 层 ins / del 交替");
    }
    let inner_first = idx.iter_inner_first();
    assert_eq!(inner_first.len(), idx.len());
    assert_eq!(inner_first[0].depth, 499, "最内层排第一");
    assert_eq!(inner_first[499].depth, 0, "最外层排最后");
}

/// `iter_inner_first` 在同一段里的兄弟修订上保持文档序，只把嵌套关系倒过来。
#[test]
fn mod_09_inner_first_order() {
    let docx = common::docx_with_body(concat!(
        r#"<w:p><w:ins w:id="1" w:author="A" w:date="2024-01-01T00:00:00Z">"#,
        r#"<w:del w:id="2" w:author="A" w:date="2024-01-01T00:00:00Z">"#,
        r#"<w:r><w:delText>x</w:delText></w:r></w:del></w:ins>"#,
        r#"<w:ins w:id="3" w:author="A" w:date="2024-01-01T00:00:00Z">"#,
        r#"<w:r><w:t>y</w:t></w:r></w:ins></w:p>"#,
    ));
    let s = EditSession::open(&docx).unwrap();
    let order: Vec<Option<&str>> =
        s.document().revisions.iter_inner_first().iter().map(|e| e.meta.id.as_deref()).collect();
    assert_eq!(order, vec![Some("2"), Some("1"), Some("3")], "先内层 del，再外层 ins，再兄弟");
}

// ---- 会话内稳定的 id ----------------------------------------------------------------------------

/// `MOD-13`：编辑后刷新，仍然存在的修订保住原来的 [`rsword::model::RevisionId`]。
#[test]
fn mod_13_revision_ids_survive_refresh() {
    let bytes = std::fs::read(common::corpus_dir("real").join("revisions2/rev-insert-delete.docx"))
        .unwrap();
    let mut s = EditSession::open(&bytes).unwrap();
    let before: Vec<(u32, RevKind)> =
        s.document().revisions.entries().iter().map(|e| (e.id.0, e.kind)).collect();
    assert!(before.len() >= 3);
    let para =
        s.document().main.iter().find_map(|b| b.as_text().map(|t| t.node)).expect("有文本段落");
    s.apply(
        EditOp::InsertText { at: InlinePos::new(para, 0), text: "前缀".into(), props: None },
        &EditContext::default(),
    )
    .expect("插字");
    let after: Vec<(u32, RevKind)> =
        s.document().revisions.entries().iter().map(|e| (e.id.0, e.kind)).collect();
    assert_eq!(before, after, "承载节点没变，id 与种类都不变");
    s.rebuild().unwrap();
    let rebuilt: Vec<(u32, RevKind)> =
        s.document().revisions.entries().iter().map(|e| (e.id.0, e.kind)).collect();
    assert_eq!(before, rebuilt, "整体重建后 id 照样稳定");
}

/// 无会话的 [`rsword::model::Document::rebuild`] 从 0 起编号。
#[test]
fn mod_13_ids_start_at_zero_without_session() {
    let bytes =
        std::fs::read(common::corpus_dir("real").join("revisions2/rev-format.docx")).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let ids: Vec<u32> = doc.revisions.entries().iter().map(|e| e.id.0).collect();
    assert_eq!(ids, (0..ids.len() as u32).collect::<Vec<_>>());
}

// ---- `EDIT-06` 的全局最大 `w:id` ----------------------------------------------------------------

/// 跨 part 取最大值；非数字的 `w:id` 不参与。
#[test]
fn edit_06_max_revision_id_is_package_wide() {
    let docx = common::docx_with_parts(
        concat!(
            r#"<w:p><w:ins w:id="7" w:author="A" w:date="2024-01-01T00:00:00Z">"#,
            r#"<w:r><w:t>x</w:t></w:r></w:ins></w:p>"#,
            r#"<w:p><w:ins w:id="abc" w:author="A" w:date="2024-01-01T00:00:00Z">"#,
            r#"<w:r><w:t>y</w:t></w:r></w:ins></w:p>"#,
        ),
        &[],
    );
    let s = EditSession::open(&docx).unwrap();
    assert_eq!(s.document().revisions.max_w_id(), Some(7));
    let non_numeric = s.document().revisions.entries().iter().find(|e| e.w_id().is_none());
    assert_eq!(non_numeric.and_then(|e| e.meta.id.as_deref()), Some("abc"), "原串保留");
}

/// 作者过滤（`AcceptAll { author }` 的基础）。
#[test]
fn mod_09_by_author() {
    let bytes = std::fs::read(
        common::repo_root().join("fixtures/revisions/tracked-two-authors/tracked.docx"),
    )
    .unwrap();
    let s = EditSession::open(&bytes).unwrap();
    let idx = &s.document().revisions;
    let authors = idx.authors();
    assert_eq!(authors.len(), 2, "两个作者：{authors:?}");
    let total: usize = authors.iter().map(|a| idx.by_author(a).count()).sum();
    assert_eq!(total, idx.len(), "每条都归到某个作者名下");
}

// ---- `w:trackRevisions` 的写侧 ------------------------------------------------------------------

/// `SetDocumentSettings { track_revisions }` 写出 `w:trackRevisions`，位置符合 `PROP-05`。
#[test]
fn edit_03_set_track_revisions() {
    let docx = common::docx_with_body("<w:p><w:r><w:t>hi</w:t></w:r></w:p>");
    let mut s = EditSession::open(&docx).unwrap();
    let patch = rsword::semantic::props::SettingsPatch {
        track_revisions: rsword::semantic::props::Change::Set(true),
        ..Default::default()
    };
    s.apply(EditOp::SetDocumentSettings { patch }, &EditContext::default()).expect("设置");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/settings.xml",
        [("count(/w:settings/w:trackRevisions)", ["1"]), ("count(/w:settings/*)", ["1"]),]
    );
    let reopened = EditSession::open(&out).unwrap();
    assert_eq!(
        reopened.document().settings.as_ref().and_then(|s| s.track_revisions),
        Some(true),
        "重解析读得回来"
    );
}

// ---- 7.4：接受 / 拒绝修订 ----------------------------------------------------------------------

use common::fingerprint::{diff_str, fingerprint};

fn accept_all(bytes: &[u8], author: Option<&str>) -> EditSession {
    let mut s = EditSession::open(bytes).expect("open");
    s.apply(EditOp::AcceptAll { author: author.map(str::to_string) }, &EditContext::default())
        .expect("AcceptAll");
    s
}

fn reject_all(bytes: &[u8], author: Option<&str>) -> EditSession {
    let mut s = EditSession::open(bytes).expect("open");
    s.apply(EditOp::RejectAll { author: author.map(str::to_string) }, &EditContext::default())
        .expect("RejectAll");
    s
}

/// 门 3：真实 Word 的四态对照件。我们对 `tracked.docx` 做 `AcceptAll` / `RejectAll`，
/// 指纹应分别与 Word 自己「接受所有修订」/「拒绝所有修订」另存的文档相等
/// （`fixtures/revisions/README.md`）。
#[test]
fn gate_3_word_accept_reject_fixtures() {
    let dir = common::repo_root().join("fixtures/revisions");
    let mut cases: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|d| d.ok())
        .filter(|d| d.path().is_dir())
        .map(|d| d.file_name().to_string_lossy().to_string())
        .collect();
    cases.sort();
    assert_eq!(cases.len(), 4, "四个 case：{cases:?}");
    for case in cases {
        let read = |name: &str| std::fs::read(dir.join(&case).join(name)).expect(name);
        let tracked = read("tracked.docx");
        let want_accept = fingerprint(&EditSession::open(&read("accepted.docx")).unwrap());
        let want_reject = fingerprint(&EditSession::open(&read("rejected.docx")).unwrap());
        // Word 自己的四态里两个视图都不再有修订，所以两个视图相同——直接比 accept 那一半
        let got_accept = fingerprint(&accept_all(&tracked, None));
        if let Some((x, y)) = diff_str(&want_accept.accept, &got_accept.accept) {
            panic!("{case}：AcceptAll 与 Word 的 accepted.docx 不同\n  Word: {x}\n  我们: {y}");
        }
        let got_reject = fingerprint(&reject_all(&tracked, None));
        if let Some((x, y)) = diff_str(&want_reject.accept, &got_reject.accept) {
            panic!("{case}：RejectAll 与 Word 的 rejected.docx 不同\n  Word: {x}\n  我们: {y}");
        }
    }
}

/// 门 2：语料里每一份带修订的文档都能 `AcceptAll` / `RejectAll`，之后重解析
/// `Document.revisions` 为空、没有引擎不变式违规。
#[test]
fn gate_2_accept_reject_all_corpus() {
    let mut checked = 0usize;
    for path in revision_docs() {
        let bytes = std::fs::read(&path).unwrap();
        let name = short(&path);
        let Ok(probe) = EditSession::open(&bytes) else { continue };
        if probe.document().revisions.is_empty() {
            continue;
        }
        // `w:cellMerge` 的拒绝方向不支持（`spec/18`「不在 M7」）
        let has_cell_merge = probe
            .document()
            .revisions
            .entries()
            .iter()
            .any(|e| e.kind == rsword::model::RevKind::CellMerge);
        checked += 1;
        for accept in [true, false] {
            if !accept && has_cell_merge {
                continue;
            }
            let mut s = EditSession::open(&bytes).unwrap();
            let op = if accept {
                EditOp::AcceptAll { author: None }
            } else {
                EditOp::RejectAll { author: None }
            };
            s.apply(op, &EditContext::default())
                .unwrap_or_else(|e| panic!("{name} accept={accept}: {e}"));
            assert!(
                !s.diagnostics()
                    .iter()
                    .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
                "{name} accept={accept}: {:?}",
                s.diagnostics()
            );
            let saved = s.save().unwrap_or_else(|e| panic!("{name} accept={accept} 保存: {e}"));
            let re = EditSession::open(&saved)
                .unwrap_or_else(|e| panic!("{name} accept={accept} 重解析: {e}"));
            let left: Vec<&str> =
                re.document().revisions.entries().iter().map(|e| e.kind.as_str()).collect();
            assert!(left.is_empty(), "{name} accept={accept}: 还剩修订 {left:?}");
        }
    }
    assert!(checked >= 16, "只检查了 {checked} 份带修订的语料");
}

/// `AcceptAll { author }` 只动那个作者的；另一个作者的修订一条不少。
#[test]
fn accept_all_filters_by_author() {
    let bytes = std::fs::read(
        common::repo_root().join("fixtures/revisions/tracked-two-authors/tracked.docx"),
    )
    .unwrap();
    let base = EditSession::open(&bytes).unwrap();
    let authors = base.document().revisions.authors();
    let (a, b) = (authors[0].to_string(), authors[1].to_string());
    let before_b = base.document().revisions.by_author(&b).count();
    let s = accept_all(&bytes, Some(&a));
    let idx = &s.document().revisions;
    assert_eq!(idx.by_author(&a).count(), 0, "{a} 的修订都处理掉了");
    assert_eq!(idx.by_author(&b).count(), before_b, "{b} 的修订一条不少");
}

/// 单条接受 / 拒绝：`AcceptRevision` 只动那一条。
#[test]
fn accept_one_revision() {
    let bytes = std::fs::read(common::corpus_dir("real").join("revisions2/rev-insert-delete.docx"))
        .unwrap();
    let mut s = EditSession::open(&bytes).unwrap();
    let before = s.document().revisions.len();
    let first = s.document().revisions.entries()[0].id;
    s.apply(EditOp::AcceptRevision { rev: first }, &EditContext::default()).expect("接受一条");
    assert_eq!(s.document().revisions.len(), before - 1, "只少一条");
}

/// 拒绝 `rPrChange` 时**整体**换回快照的子元素，连本引擎没建模的子元素也回来。
#[test]
fn reject_run_props_change_restores_unmodeled_children() {
    let docx = common::docx_with_body(concat!(
        r#"<w:p><w:r><w:rPr><w:b/><w:rPrChange w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z">"#,
        r#"<w:rPr><w:i/><w:oMath/></w:rPr></w:rPrChange></w:rPr><w:t>字</w:t></w:r></w:p>"#,
    ));
    let mut s = EditSession::open(&docx).unwrap();
    let rev = s.document().revisions.entries()[0].id;
    s.apply(EditOp::RejectRevision { rev }, &EditContext::default()).expect("拒绝");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:r/w:rPr/w:b)", ["0"]),
            ("count(//w:r/w:rPr/w:i)", ["1"]),
            // `w:oMath` 在 `rPr` 里是本引擎没建模的字段，整体克隆把它也带回来了
            ("count(//w:r/w:rPr/w:oMath)", ["1"]),
            ("count(//w:rPrChange)", ["0"]),
        ]
    );
}

/// 接受段落标记的删除 = 无追踪的 `MergeWithNext`；拒绝只去掉标记。
#[test]
fn para_mark_delete_accept_merges() {
    let body = concat!(
        r#"<w:p><w:pPr><w:rPr><w:del w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"/></w:rPr></w:pPr>"#,
        r#"<w:r><w:t>前</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>后</w:t></w:r></w:p>"#,
    );
    let docx = common::docx_with_body(body);
    let mut s = EditSession::open(&docx).unwrap();
    let rev = s.document().revisions.entries()[0].id;
    s.apply(EditOp::AcceptRevision { rev }, &EditContext::default()).expect("接受");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["1"]),
            ("//w:p/w:r/w:t/text()", ["前", "后"]),
            ("count(//w:pPr/w:rPr/w:del)", ["0"]),
        ]
    );

    let mut s = EditSession::open(&docx).unwrap();
    let rev = s.document().revisions.entries()[0].id;
    s.apply(EditOp::RejectRevision { rev }, &EditContext::default()).expect("拒绝");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:body/w:p)", ["2"]), ("count(//w:pPr/w:rPr/w:del)", ["0"]),]
    );
}

/// 先内层后外层：`w:ins` 里套 `w:del`，接受时先接受 `del`（内容消失）再解包 `ins`。
#[test]
fn inner_first_ins_wrapping_del() {
    let docx = common::docx_with_body(concat!(
        r#"<w:p><w:ins w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z">"#,
        r#"<w:r><w:t>留下</w:t></w:r>"#,
        r#"<w:del w:id="2" w:author="A" w:date="2026-01-01T00:00:00Z">"#,
        r#"<w:r><w:delText>删掉</w:delText></w:r></w:del></w:ins></w:p>"#,
    ));
    let s = accept_all(&docx, None);
    let mut s = s;
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:ins)", ["0"]), ("count(//w:del)", ["0"]), ("//w:p/w:r/w:t/text()", ["留下"]),]
    );
    // 拒绝：外层的插入整段撤掉，什么都不剩
    let mut s = reject_all(&docx, None);
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:p/w:r)", ["0"]), ("count(//w:ins)", ["0"]),]
    );
}

/// 搬移：接受 = 来源内容消失、落点解包；范围标记一起删。
#[test]
fn move_revision_accept_and_reject() {
    let bytes = std::fs::read(common::corpus_dir("real").join("revisions2/rev-move.docx")).unwrap();
    let mut s = accept_all(&bytes, None);
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:moveFrom)", ["0"]),
            ("count(//w:moveTo)", ["0"]),
            ("count(//w:moveFromRangeStart)", ["0"]),
            ("count(//w:moveToRangeStart)", ["0"]),
            ("count(//w:moveFromRangeEnd)", ["0"]),
            ("count(//w:moveToRangeEnd)", ["0"]),
        ]
    );
    let mut s = reject_all(&bytes, None);
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:moveFrom)", ["0"]),
            ("count(//w:moveTo)", ["0"]),
            ("count(//w:delText)", ["0"]),
        ]
    );
}

// ---- 门 2 的另一半：`MOD-09` 每种修订，Accept / Reject 各一条 XPath ---------------------------

const D: &str = r#"w:author="甲" w:date="2026-01-01T00:00:00Z""#;

/// 一份构造文档 + 一个方向：断言索引里有这种修订，处理完之后 XPath 符合预期。
fn check(label: &str, body: &str, kind: RevKind, accept: bool, checks: &[(&str, &[&str])]) {
    let docx = common::docx_with_body(body);
    let s = EditSession::open(&docx).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert!(
        s.document().revisions.entries().iter().any(|e| e.kind == kind),
        "{label}: 索引里没有 {kind}，只有 {:?}",
        s.document().revisions.entries().iter().map(|e| e.kind.as_str()).collect::<Vec<_>>()
    );
    let mut s = if accept { accept_all(&docx, None) } else { reject_all(&docx, None) };
    let out = s.save().unwrap_or_else(|e| panic!("{label} {accept}: 保存 {e}"));
    let dom = common::xpath_dom(&out, "word/document.xml");
    for (expr, want) in checks {
        let got = rsword::xml::xpath::eval_strings(&dom, expr)
            .unwrap_or_else(|e| panic!("{label}: XPath `{expr}` {e}"));
        let want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want, "{label} accept={accept}: `{expr}`");
    }
    // 处理完之后一条修订都不该剩
    let re = EditSession::open(&out).unwrap();
    let left: Vec<&str> =
        re.document().revisions.entries().iter().map(|e| e.kind.as_str()).collect();
    assert!(left.is_empty(), "{label} accept={accept}: 还剩 {left:?}");
}

/// 内容包裹八种（块级与 run 级各四种）。
#[test]
fn mod_09_content_wrappers_accept_reject() {
    let cases: [(&str, String, RevKind, &str); 4] = [
        (
            "块级 w:ins",
            format!(r#"<w:ins w:id="1" {D}><w:p><w:r><w:t>新块</w:t></w:r></w:p></w:ins>"#),
            rsword::model::RevKind::Insert,
            "新块",
        ),
        (
            "块级 w:del",
            format!(
                r#"<w:del w:id="1" {D}><w:p><w:r><w:delText>旧块</w:delText></w:r></w:p></w:del>"#
            ),
            rsword::model::RevKind::Delete,
            "旧块",
        ),
        (
            "块级 w:moveFrom",
            format!(
                r#"<w:moveFrom w:id="1" {D}><w:p><w:r><w:delText>搬走</w:delText></w:r></w:p></w:moveFrom>"#
            ),
            rsword::model::RevKind::MoveFrom,
            "搬走",
        ),
        (
            "块级 w:moveTo",
            format!(r#"<w:moveTo w:id="1" {D}><w:p><w:r><w:t>搬来</w:t></w:r></w:p></w:moveTo>"#),
            rsword::model::RevKind::MoveTo,
            "搬来",
        ),
    ];
    for (label, body, kind, text) in cases {
        // 插入类：接受 = 内容留下（解包），拒绝 = 内容消失
        let kept = matches!(kind, RevKind::Insert | RevKind::MoveTo);
        let body = format!("{body}<w:p><w:r><w:t>尾</w:t></w:r></w:p>");
        check(
            label,
            &body,
            kind,
            true,
            &[
                ("count(//w:ins)", &["0"]),
                ("count(//w:del)", &["0"]),
                ("count(//w:moveFrom)", &["0"]),
                ("count(//w:moveTo)", &["0"]),
                ("count(//w:body/w:p)", &[if kept { "2" } else { "1" }]),
                ("count(//w:delText)", &["0"]),
            ],
        );
        check(
            label,
            &body,
            kind,
            false,
            &[
                ("count(//w:body/w:p)", &[if kept { "1" } else { "2" }]),
                ("count(//w:delText)", &["0"]),
            ],
        );
        let _ = text;
    }
}

/// run 级四种。
#[test]
fn mod_09_run_wrappers_accept_reject() {
    let cases: [(&str, String, RevKind, bool); 4] = [
        (
            "run w:ins",
            format!(r#"<w:ins w:id="1" {D}><w:r><w:t>甲</w:t></w:r></w:ins>"#),
            rsword::model::RevKind::RunInsert,
            true,
        ),
        (
            "run w:del",
            format!(r#"<w:del w:id="1" {D}><w:r><w:delText>甲</w:delText></w:r></w:del>"#),
            rsword::model::RevKind::RunDelete,
            false,
        ),
        (
            "run w:moveFrom",
            format!(
                r#"<w:moveFrom w:id="1" {D}><w:r><w:delText>甲</w:delText></w:r></w:moveFrom>"#
            ),
            rsword::model::RevKind::RunMoveFrom,
            false,
        ),
        (
            "run w:moveTo",
            format!(r#"<w:moveTo w:id="1" {D}><w:r><w:t>甲</w:t></w:r></w:moveTo>"#),
            rsword::model::RevKind::RunMoveTo,
            true,
        ),
    ];
    for (label, inner, kind, kept_on_accept) in cases {
        let body = format!("<w:p>{inner}<w:r><w:t>乙</w:t></w:r></w:p>");
        let accept_text: &[&str] = if kept_on_accept { &["甲", "乙"] } else { &["乙"] };
        let reject_text: &[&str] = if kept_on_accept { &["乙"] } else { &["甲", "乙"] };
        check(
            label,
            &body,
            kind,
            true,
            &[("//w:p/w:r/w:t/text()", accept_text), ("count(//w:delText)", &["0"])],
        );
        check(
            label,
            &body,
            kind,
            false,
            &[("//w:p/w:r/w:t/text()", reject_text), ("count(//w:delText)", &["0"])],
        );
    }
}

/// 段落标记四种。
#[test]
fn mod_09_para_marks_accept_reject() {
    let two = |mark: &str| {
        format!(
            "<w:p><w:pPr><w:rPr>{mark}</w:rPr></w:pPr><w:r><w:t>前</w:t></w:r></w:p>\
             <w:p><w:r><w:t>后</w:t></w:r></w:p>"
        )
    };
    let cases: [(&str, String, RevKind, bool); 4] = [
        (
            "pPr/rPr/w:ins",
            two(&format!(r#"<w:ins w:id="1" {D}/>"#)),
            rsword::model::RevKind::ParaMarkInsert,
            false,
        ),
        (
            "pPr/rPr/w:del",
            two(&format!(r#"<w:del w:id="1" {D}/>"#)),
            rsword::model::RevKind::ParaMarkDelete,
            true,
        ),
        (
            "pPr/rPr/w:moveFrom",
            two(&format!(r#"<w:moveFrom w:id="1" {D}/>"#)),
            rsword::model::RevKind::ParaMarkMoveFrom,
            true,
        ),
        (
            "pPr/rPr/w:moveTo",
            two(&format!(r#"<w:moveTo w:id="1" {D}/>"#)),
            rsword::model::RevKind::ParaMarkMoveTo,
            false,
        ),
    ];
    for (label, body, kind, merge_on_accept) in cases {
        let paras = |merged: bool| if merged { "1" } else { "2" };
        check(
            label,
            &body,
            kind,
            true,
            &[("count(//w:body/w:p)", &[paras(merge_on_accept)]), ("count(//w:pPr/w:rPr)", &["0"])],
        );
        check(
            label,
            &body,
            kind,
            false,
            &[
                ("count(//w:body/w:p)", &[paras(!merge_on_accept)]),
                ("count(//w:pPr/w:rPr)", &["0"]),
            ],
        );
    }
}

/// 属性快照八种 + `numberingChange`。
#[test]
fn mod_09_props_changes_accept_reject() {
    // rPrChange
    check(
        "rPrChange",
        &format!(
            r#"<w:p><w:r><w:rPr><w:b/><w:rPrChange w:id="1" {D}><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>字</w:t></w:r></w:p>"#
        ),
        rsword::model::RevKind::RunPropsChange,
        true,
        &[("count(//w:rPr/w:b)", &["1"]), ("count(//w:rPr/w:i)", &["0"])],
    );
    check(
        "rPrChange",
        &format!(
            r#"<w:p><w:r><w:rPr><w:b/><w:rPrChange w:id="1" {D}><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>字</w:t></w:r></w:p>"#
        ),
        rsword::model::RevKind::RunPropsChange,
        false,
        &[("count(//w:rPr/w:b)", &["0"]), ("count(//w:rPr/w:i)", &["1"])],
    );
    // pPrChange
    let ppr = format!(
        r#"<w:p><w:pPr><w:jc w:val="center"/><w:pPrChange w:id="1" {D}><w:pPr><w:jc w:val="right"/></w:pPr></w:pPrChange></w:pPr><w:r><w:t>段</w:t></w:r></w:p>"#
    );
    check(
        "pPrChange",
        &ppr,
        rsword::model::RevKind::ParaPropsChange,
        true,
        &[("//w:pPr/w:jc/@w:val", &["center"]), ("count(//w:pPrChange)", &["0"])],
    );
    check(
        "pPrChange",
        &ppr,
        rsword::model::RevKind::ParaPropsChange,
        false,
        &[("//w:pPr/w:jc/@w:val", &["right"])],
    );
    // numberingChange（只有属性、没有内层容器：两个方向都只去掉标记）
    let num = format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/><w:numberingChange w:id="1" {D} w:original="0"/></w:numPr></w:pPr><w:r><w:t>项</w:t></w:r></w:p>"#
    );
    for accept in [true, false] {
        check(
            "numberingChange",
            &num,
            rsword::model::RevKind::NumberingChange,
            accept,
            &[("count(//w:numberingChange)", &["0"]), ("//w:numPr/w:numId/@w:val", &["1"])],
        );
    }
    // sectPrChange
    let sect = format!(
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:sectPrChange w:id="1" {D}><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:sectPrChange></w:sectPr>"#
    );
    check(
        "sectPrChange",
        &sect,
        rsword::model::RevKind::SectPropsChange,
        true,
        &[("//w:sectPr/w:pgSz/@w:w", &["11906"]), ("count(//w:sectPrChange)", &["0"])],
    );
    check(
        "sectPrChange",
        &sect,
        rsword::model::RevKind::SectPropsChange,
        false,
        &[("//w:sectPr/w:pgSz/@w:w", &["12240"])],
    );
}

/// 表格里的五种属性快照与三种单元格标记。
#[test]
fn mod_09_table_revisions_accept_reject() {
    let table = |tbl_pr: &str, grid: &str, tr_pr: &str, tbl_pr_ex: &str, tc_pr: &str| {
        format!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>{tbl_pr}</w:tblPr>
               <w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/>{grid}</w:tblGrid>
               <w:tr>{tbl_pr_ex}<w:trPr>{tr_pr}</w:trPr>
               <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/>{tc_pr}</w:tcPr>
               <w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
               <w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>
               <w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
               <w:p><w:r><w:t>尾</w:t></w:r></w:p>"#
        )
    };
    // tblPrChange
    let body = table(
        &format!(
            r#"<w:tblPrChange w:id="1" {D}><w:tblPr><w:tblStyle w:val="旧"/></w:tblPr></w:tblPrChange>"#
        ),
        "",
        "",
        "",
        "",
    );
    check(
        "tblPrChange",
        &body,
        rsword::model::RevKind::TablePropsChange,
        true,
        &[("count(//w:tblPr/w:tblStyle)", &["0"]), ("count(//w:tblPrChange)", &["0"])],
    );
    check(
        "tblPrChange",
        &body,
        rsword::model::RevKind::TablePropsChange,
        false,
        &[("//w:tblPr/w:tblStyle/@w:val", &["旧"])],
    );
    // tblGridChange
    let body = table(
        "",
        r#"<w:tblGridChange w:id="1"><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="5000"/></w:tblGrid></w:tblGridChange>"#,
        "",
        "",
        "",
    );
    check(
        "tblGridChange",
        &body,
        rsword::model::RevKind::TableGridChange,
        true,
        &[("count(/w:document/w:body/w:tbl/w:tblGrid/w:gridCol)", &["2"])],
    );
    check(
        "tblGridChange",
        &body,
        rsword::model::RevKind::TableGridChange,
        false,
        &[("/w:document/w:body/w:tbl/w:tblGrid/w:gridCol/@w:w", &["3000", "5000"])],
    );
    // trPrChange
    let body = table(
        "",
        "",
        &format!(r#"<w:trPrChange w:id="1" {D}><w:trPr><w:tblHeader/></w:trPr></w:trPrChange>"#),
        "",
        "",
    );
    check(
        "trPrChange",
        &body,
        rsword::model::RevKind::RowPropsChange,
        true,
        &[("count(//w:trPr/w:tblHeader)", &["0"])],
    );
    check(
        "trPrChange",
        &body,
        rsword::model::RevKind::RowPropsChange,
        false,
        &[("count(//w:trPr/w:tblHeader)", &["1"])],
    );
    // tblPrExChange
    let body = table(
        "",
        "",
        "",
        &format!(
            r#"<w:tblPrEx><w:tblCellMar><w:left w:w="10" w:type="dxa"/></w:tblCellMar><w:tblPrExChange w:id="1" {D}><w:tblPrEx/></w:tblPrExChange></w:tblPrEx>"#
        ),
        "",
    );
    check(
        "tblPrExChange",
        &body,
        rsword::model::RevKind::TablePropsExChange,
        true,
        &[("count(//w:tblPrExChange)", &["0"]), ("count(//w:tblPrEx/w:tblCellMar)", &["1"])],
    );
    check(
        "tblPrExChange",
        &body,
        rsword::model::RevKind::TablePropsExChange,
        false,
        &[
            // 旧值是空的 → 整个 `w:tblPrEx` 去掉（真实 Word 的形态）
            ("count(//w:tblPrEx)", &["0"]),
        ],
    );
    // tcPrChange
    let body = table(
        "",
        "",
        "",
        "",
        &format!(
            r#"<w:tcPrChange w:id="1" {D}><w:tcPr><w:tcW w:w="1234" w:type="dxa"/></w:tcPr></w:tcPrChange>"#
        ),
    );
    check(
        "tcPrChange",
        &body,
        rsword::model::RevKind::CellPropsChange,
        true,
        &[("//w:tr/w:tc[1]/w:tcPr/w:tcW/@w:w", &["4000"])],
    );
    check(
        "tcPrChange",
        &body,
        rsword::model::RevKind::CellPropsChange,
        false,
        &[("//w:tr/w:tc[1]/w:tcPr/w:tcW/@w:w", &["1234"])],
    );
    // cellIns / cellDel：接受 / 拒绝的方向相反，整列都带标记 → 网格也少一列
    for (label, mark, kind, cell_gone_on_accept) in [
        (
            "cellIns",
            format!(r#"<w:cellIns w:id="1" {D}/>"#),
            rsword::model::RevKind::CellInsert,
            false,
        ),
        (
            "cellDel",
            format!(r#"<w:cellDel w:id="1" {D}/>"#),
            rsword::model::RevKind::CellDelete,
            true,
        ),
    ] {
        let body = table("", "", "", "", &mark);
        // 只有一行 → 带标记的那个格就是"整列"，格没了网格也少一列
        let n = |gone: bool| if gone { "1" } else { "2" };
        check(
            label,
            &body,
            kind,
            true,
            &[
                ("count(//w:tr/w:tc)", &[n(cell_gone_on_accept)]),
                ("count(/w:document/w:body/w:tbl/w:tblGrid/w:gridCol)", &[n(cell_gone_on_accept)]),
            ],
        );
        check(
            label,
            &body,
            kind,
            false,
            &[
                ("count(//w:tr/w:tc)", &[n(!cell_gone_on_accept)]),
                ("count(/w:document/w:body/w:tbl/w:tblGrid/w:gridCol)", &[n(!cell_gone_on_accept)]),
            ],
        );
    }
    // cellMerge：接受 = 去标记；拒绝不支持（`vMergeOrig` 待真实 Word 校准）
    let body = table("", "", "", "", &format!(r#"<w:cellMerge w:id="1" {D} w:vMerge="cont"/>"#));
    check(
        "cellMerge",
        &body,
        rsword::model::RevKind::CellMerge,
        true,
        &[("count(//w:cellMerge)", &["0"]), ("count(//w:tr/w:tc)", &["2"])],
    );
    let docx = common::docx_with_body(&body);
    let mut s = EditSession::open(&docx).unwrap();
    let err = s
        .apply(EditOp::RejectAll { author: None }, &EditContext::default())
        .expect_err("cellMerge 的拒绝方向不支持");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. } if *code == rsword::DiagCode::EditUnsupported),
        "{err}"
    );
    let fresh = EditSession::open(&docx).unwrap();
    assert_eq!(
        fresh.document().revisions.len(),
        s.document().revisions.len(),
        "EDIT-05：被拒后状态不变"
    );
}
