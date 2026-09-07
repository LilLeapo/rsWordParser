//! 修订索引（`MOD-09` / `MOD-13` / `EDIT-06`，`spec/18` 7.1）与修订相关病态输入（`TEST-09`，7.0⑤）。

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rsword::bind::compat_ts::parsed_doc;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::model::{RevKind, RevOwner, RevisionEntry, RevisionIndex};
use rsword::package::Package;
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
type Ident = (String, String, String);

fn ident_of(o: &Map<String, Value>) -> Ident {
    let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    (s("author"), s("date"), s("id"))
}

/// 从 `ParsedDoc` 的 `blocks` 子树里按键收身份（`runs[].ins / del`、`paraMarkDel`、
/// `pPrChangeInfo`、`rowRevisions[]`、`cellRevision`）。
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
            (RevKind::RunInsert | RevKind::RunMoveTo, _) => "ins",
            (RevKind::RunDelete | RevKind::RunMoveFrom, _) => "del",
            (RevKind::ParaMarkDelete, _) => "paraMarkDel",
            (RevKind::ParaPropsChange, _) => "pPrChange",
            (RevKind::Insert, RevOwner::Row(_)) => "rowIns",
            (RevKind::Delete, RevOwner::Row(_)) => "rowDel",
            (RevKind::CellInsert, _) => "cellIns",
            (RevKind::CellDelete, _) => "cellDel",
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
        let want = if i % 2 == 0 { RevKind::RunInsert } else { RevKind::RunDelete };
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
