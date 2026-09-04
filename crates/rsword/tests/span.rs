//! `SPAN-01`–`SPAN-05`：内容序列、Anchor、范围索引与文档序（任务 2.1）。

mod common;

use std::cmp::Ordering;

use rsword::diag::DiagCode;
use rsword::package::{Package, PartId};
use rsword::span::{
    Affinity, Anchor, RangeClass, RangeKind, SpanEnd, SpanIndex, SpanOrigin, content_children,
    content_len,
};
use rsword::xml::{Dom, LocalName, NodeId, QName};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn dom(body: &str) -> Dom {
    let xml = format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#);
    Dom::parse(PartId(0), xml.as_bytes()).unwrap()
}

fn body_of(dom: &Dom) -> NodeId {
    dom.semantic_children(dom.root()).next().unwrap()
}

fn find_all(dom: &Dom, local: LocalName) -> Vec<NodeId> {
    dom.descendants(dom.root()).filter(|&n| dom.is(n, QName::w(local))).collect()
}

fn find(dom: &Dom, local: LocalName) -> NodeId {
    find_all(dom, local)[0]
}

#[test]
fn span_01_content_sequence_skips_props_and_markers() {
    let d = dom(r#"<w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:bookmarkStart w:id="1" w:name="a"/>
           <w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="1"/><w:r><w:t>y</w:t></w:r></w:p>"#);
    let p = find(&d, LocalName::P);
    let content = content_children(&d, p);
    assert_eq!(content.len(), 2, "内容序列只含两个 run");
    assert!(content.iter().all(|&n| d.is(n, QName::w(LocalName::R))));
    assert_eq!(content_len(&d, p), 2);
    // 缩进产生的空白文本节点不占内容边界
    let pretty = dom("<w:p>\n  <w:r><w:t>x</w:t></w:r>\n</w:p>\n");
    let p2 = find(&pretty, LocalName::P);
    assert_eq!(content_len(&pretty, p2), 1);
}

#[test]
fn span_04_bookmark_across_three_paragraphs() {
    let d = dom(
        r#"<w:p><w:r><w:t>a</w:t></w:r><w:bookmarkStart w:id="3" w:name="_Ref9"/><w:r><w:t>b</w:t></w:r></w:p>
           <w:p><w:r><w:t>c</w:t></w:r></w:p>
           <w:p><w:r><w:t>d</w:t></w:r><w:bookmarkEnd w:id="3"/></w:p>"#,
    );
    let idx = SpanIndex::build(&d);
    assert_eq!(idx.len(), 1);
    let s = &idx.spans()[0];
    let paras = find_all(&d, LocalName::P);
    let start = s.start.expect("起点");
    let end = s.end.expect("终点");
    assert_eq!((start.container, start.index, start.affinity), (paras[0], 1, Affinity::Right));
    assert_eq!((end.container, end.index, end.affinity), (paras[2], 1, Affinity::Left));
    assert_eq!(start.marker, Some(find(&d, LocalName::BookmarkStart)));
    assert!(!s.is_collapsed() && s.is_paired());
    assert_eq!(s.origin, SpanOrigin::Parsed);
    assert!(idx.diagnostics().is_empty(), "{:?}", idx.diagnostics());
    match &s.kind {
        RangeKind::Bookmark { id, name, hidden, cols } => {
            assert_eq!((id.as_str(), name.as_str(), *hidden, *cols), ("3", "_Ref9", true, None));
        }
        k => panic!("{k:?}"),
    }
    // 倒排索引：两个段落各挂一个端点
    assert_eq!(idx.at_container(paras[0]), [(s.id, SpanEnd::Start)]);
    assert_eq!(idx.at_container(paras[2]), [(s.id, SpanEnd::End)]);
    assert!(idx.at_container(paras[1]).is_empty());
    assert!(idx.is_ordered(&d, s), "起在终前");
}

#[test]
fn span_04_collapsed_bookmark_shares_one_boundary() {
    let d = dom(
        r#"<w:p><w:r><w:t>a</w:t></w:r><w:bookmarkStart w:id="1" w:name="mark"/><w:bookmarkEnd w:id="1"/><w:r><w:t>b</w:t></w:r></w:p>"#,
    );
    let idx = SpanIndex::build(&d);
    let s = &idx.spans()[0];
    assert!(s.is_collapsed());
    let (start, end) = (s.start.unwrap(), s.end.unwrap());
    assert_eq!((start.index, end.index), (1, 1));
    // 空范围两端同向：否则 Left < Right 会判成"起在终后"，边界插入还会把它拆反
    assert_eq!((start.affinity, end.affinity), (Affinity::Right, Affinity::Right));
    assert_eq!(idx.compare(&d, &start, &end), Some(Ordering::Equal));
    assert!(idx.is_ordered(&d, s));
}

#[test]
fn span_04_reference_only_comment_is_a_collapsed_span_without_markers() {
    let d = dom(r#"<w:p><w:r><w:t>a</w:t></w:r><w:r><w:commentReference w:id="7"/></w:r></w:p>"#);
    let idx = SpanIndex::build(&d);
    assert_eq!(idx.len(), 1);
    let s = &idx.spans()[0];
    let runs = find_all(&d, LocalName::R);
    assert!(s.is_collapsed());
    let start = s.start.unwrap();
    assert_eq!((start.container, start.index), (find(&d, LocalName::P), 1), "reference run 之前");
    assert_eq!(start.marker, None, "文件里没有标记，物化不得补写");
    assert_eq!(s.end.unwrap().marker, None);
    assert_eq!(s.kind, RangeKind::Comment { id: "7".into(), reference: Some(runs[1]) });
    assert!(idx.diagnostics().is_empty());
}

#[test]
fn span_04_comment_range_picks_up_its_reference_run() {
    let d = dom(
        r#"<w:p><w:commentRangeStart w:id="2"/><w:r><w:t>a</w:t></w:r><w:commentRangeEnd w:id="2"/>
           <w:r><w:commentReference w:id="2"/></w:r></w:p>"#,
    );
    let idx = SpanIndex::build(&d);
    assert_eq!(idx.len(), 1, "只有一个范围，不额外生成折叠范围");
    let runs = find_all(&d, LocalName::R);
    assert_eq!(
        idx.spans()[0].kind,
        RangeKind::Comment { id: "2".into(), reference: Some(runs[1]) }
    );
    let s = &idx.spans()[0];
    assert_eq!((s.start.unwrap().index, s.end.unwrap().index), (0, 1));
}

#[test]
fn span_04_orphan_end_and_unclosed_start_are_pre_existing_damage() {
    let d = dom(
        r#"<w:p><w:bookmarkEnd w:id="9"/><w:r><w:t>a</w:t></w:r><w:bookmarkStart w:id="8" w:name="b"/></w:p>"#,
    );
    let idx = SpanIndex::build(&d);
    let codes: Vec<DiagCode> = idx.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, [DiagCode::SpanOrphanEnd, DiagCode::SpanUnclosed]);
    assert!(
        idx.diagnostics()
            .iter()
            .all(|d| matches!(d.origin, rsword::ValidationOrigin::PreExistingDamage))
    );
    let unclosed = idx.spans().iter().find(|s| s.end.is_none()).expect("未闭合起点");
    let orphan = idx.spans().iter().find(|s| s.start.is_none()).expect("孤儿终点");
    assert!(!unclosed.is_paired() && !orphan.is_paired());
    assert!(!idx.is_ordered(&d, orphan), "缺端点不算有序");
}

#[test]
fn span_04_duplicate_start_is_reported_and_nests() {
    let d = dom(r#"<w:p><w:bookmarkStart w:id="1" w:name="a"/><w:r><w:t>x</w:t></w:r>
           <w:bookmarkStart w:id="1" w:name="b"/><w:r><w:t>y</w:t></w:r>
           <w:bookmarkEnd w:id="1"/><w:bookmarkEnd w:id="1"/></w:p>"#);
    let idx = SpanIndex::build(&d);
    assert_eq!(idx.len(), 2);
    assert_eq!(
        idx.diagnostics().iter().map(|d| d.code).collect::<Vec<_>>(),
        [DiagCode::SpanDupStart]
    );
    // 后一个起点先闭合（就近配对），两个范围因此正确嵌套
    let outer = &idx.spans()[0];
    let inner = &idx.spans()[1];
    assert_eq!((outer.start.unwrap().index, outer.end.unwrap().index), (0, 2));
    assert_eq!((inner.start.unwrap().index, inner.end.unwrap().index), (1, 2));
    assert!(idx.is_ordered(&d, outer) && idx.is_ordered(&d, inner));
}

#[test]
fn span_04_cross_flow_pairing_is_refused() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:bookmarkStart w:id="1" w:name="a"/>
             <w:r><w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent>
               <w:p><w:bookmarkEnd w:id="1"/></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r>
           </w:p></w:body></w:document>"#
    );
    let d = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
    let idx = SpanIndex::build(&d);
    let codes: Vec<DiagCode> = idx.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, [DiagCode::SpanCrossFlow, DiagCode::SpanUnclosed]);
    assert!(idx.spans().iter().all(|s| !s.is_paired()), "跨流不配对");
}

#[test]
fn span_05_document_order_across_containers() {
    let d = dom(r#"<w:p><w:r><w:t>a</w:t></w:r></w:p>
           <w:tbl><w:tr><w:tc><w:p><w:r><w:t>c1</w:t></w:r></w:p></w:tc>
                        <w:tc><w:p><w:r><w:t>c2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
           <w:p><w:r><w:t>z</w:t></w:r></w:p>"#);
    let idx = SpanIndex::build(&d);
    let paras = find_all(&d, LocalName::P);
    let body = body_of(&d);
    let cells = find_all(&d, LocalName::Tc);
    let a = Anchor::new(paras[0], 1, Affinity::Left);
    let c1 = Anchor::new(paras[1], 0, Affinity::Left);
    let c2 = Anchor::new(paras[2], 0, Affinity::Left);
    let z = Anchor::new(paras[3], 0, Affinity::Left);
    let cmp = |x: &Anchor, y: &Anchor| idx.compare(&d, x, y);
    assert_eq!(cmp(&a, &c1), Some(Ordering::Less), "段落 < 表格第一格");
    assert_eq!(cmp(&c1, &c2), Some(Ordering::Less), "同行两格按文档序");
    assert_eq!(cmp(&c2, &z), Some(Ordering::Less), "表格 < 后一段");
    assert_eq!(cmp(&z, &a), Some(Ordering::Greater));
    // 祖先容器的边界与后代比较：body 边界 1 在表格（内容项 1）之前
    assert_eq!(cmp(&Anchor::new(body, 1, Affinity::Left), &c1), Some(Ordering::Less));
    assert_eq!(cmp(&Anchor::new(body, 2, Affinity::Left), &c1), Some(Ordering::Greater));
    // 单元格容器与其中的段落
    assert_eq!(cmp(&Anchor::new(cells[0], 0, Affinity::Left), &c1), Some(Ordering::Less));
    // 同容器同边界：Left < Right
    let l = Anchor::new(paras[0], 1, Affinity::Left);
    let r = Anchor::new(paras[0], 1, Affinity::Right);
    assert_eq!(cmp(&l, &r), Some(Ordering::Less));
    assert_eq!(cmp(&r, &l), Some(Ordering::Greater));
    assert_eq!(cmp(&l, &l), Some(Ordering::Equal));
}

#[test]
fn span_04_markers_inside_ins_belong_to_the_ins_container() {
    let d = dom(
        r#"<w:p><w:r><w:t>a</w:t></w:r><w:ins w:id="5" w:author="A"><w:bookmarkStart w:id="1" w:name="b"/>
           <w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="1"/></w:ins></w:p>"#,
    );
    let idx = SpanIndex::build(&d);
    let ins = find(&d, LocalName::Ins);
    let s = &idx.spans()[0];
    assert_eq!(s.start.unwrap().container, ins);
    assert_eq!((s.start.unwrap().index, s.end.unwrap().index), (0, 1));
    assert!(idx.is_ordered(&d, s));
}

#[test]
fn span_03_permission_and_move_ranges_carry_their_facts() {
    let d = dom(r#"<w:p><w:permStart w:id="1" w:ed="everyone" w:colFirst="0" w:colLast="2"/>
           <w:moveFromRangeStart w:id="2" w:name="m1" w:author="A" w:date="2024-01-01T00:00:00Z"/>
           <w:r><w:t>x</w:t></w:r>
           <w:moveFromRangeEnd w:id="2"/><w:permEnd w:id="1"/>
           <w:customXmlInsRangeStart w:id="3" w:author="B"/><w:customXmlInsRangeEnd w:id="3"/></w:p>"#);
    let idx = SpanIndex::build(&d);
    assert!(idx.diagnostics().is_empty(), "{:?}", idx.diagnostics());
    let perm = idx.find(RangeClass::Permission, "1").expect("permission");
    assert!(matches!(
        &perm.kind,
        RangeKind::Permission { editor, group, cols, .. }
            if editor.as_deref() == Some("everyone") && group.is_none() && *cols == Some((0, 2))
    ));
    let mv = idx.find(RangeClass::MoveFrom, "2").expect("moveFrom");
    assert!(matches!(
        &mv.kind,
        RangeKind::MoveFrom { name, meta, .. }
            if name == "m1" && meta.author.as_deref() == Some("A")
                && meta.date.as_deref() == Some("2024-01-01T00:00:00Z")
    ));
    assert!(idx.find(RangeClass::CustomXmlIns, "3").is_some_and(|s| s.is_collapsed()));
    assert_eq!(idx.iter_class(RangeClass::Bookmark).count(), 0);
}

/// 全语料：每个范围标记恰好被一个端点认领，配对的范围起在终前且同流。
#[test]
fn span_04_every_marker_in_the_corpus_is_accounted_for() {
    let mut docs = 0usize;
    let mut parts = 0usize;
    let mut markers = 0usize;
    let mut spans = 0usize;
    let mut collapsed = 0usize;
    let mut damaged = 0usize;
    let mut refs_only = 0usize;
    let mut by_class: std::collections::BTreeMap<String, usize> = Default::default();
    let mut diag_counts: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut failures: Vec<String> = Vec::new();

    for path in common::docx_paths("synthetic") {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        docs += 1;
        let ids: Vec<PartId> = pkg.parts().iter().map(|p| p.id).collect();
        for id in ids {
            let Ok(Some(dom)) = pkg.dom(id) else { continue };
            parts += 1;
            let idx = SpanIndex::build(dom);
            let physical: Vec<NodeId> = dom
                .descendants(dom.root())
                .filter(|&n| {
                    dom.name(n).is_some_and(rsword::span::is_range_marker)
                        && dom.node(n).dirty != rsword::xml::Dirty::Deleted
                })
                .collect();
            markers += physical.len();
            spans += idx.len();
            let mut claimed: Vec<NodeId> = Vec::new();
            for s in idx.spans() {
                *by_class.entry(format!("{:?}", s.class())).or_default() += 1;
                if s.is_collapsed() {
                    collapsed += 1;
                }
                if !s.is_paired() {
                    damaged += 1;
                }
                if s.is_paired() && s.start.unwrap().marker.is_none() {
                    refs_only += 1;
                }
                for which in [SpanEnd::Start, SpanEnd::End] {
                    if let Some(m) = s.anchor(which).and_then(|a| a.marker) {
                        claimed.push(m);
                    }
                }
                if s.is_paired() && !idx.is_ordered(dom, s) {
                    failures.push(format!("{stem} part#{}: {:?} 起在终后", id.0, s.kind));
                }
                for which in [SpanEnd::Start, SpanEnd::End] {
                    if let Some(a) = s.anchor(which) {
                        let len = content_len(dom, a.container);
                        if a.index > len {
                            failures.push(format!(
                                "{stem} part#{}: {:?} 的 {which:?} index {} > len {len}",
                                id.0, s.kind, a.index
                            ));
                        }
                    }
                }
            }
            claimed.sort();
            let before = claimed.len();
            claimed.dedup();
            if claimed.len() != before {
                failures.push(format!("{stem} part#{}: 标记被多个端点认领", id.0));
            }
            let mut missed: Vec<u32> =
                physical.iter().filter(|m| !claimed.contains(m)).map(|m| m.0).collect();
            if !missed.is_empty() {
                missed.truncate(5);
                failures.push(format!("{stem} part#{}: 标记未进索引 {missed:?}", id.0));
            }
            for d in idx.diagnostics() {
                *diag_counts.entry(d.code.as_str()).or_default() += 1;
            }
        }
    }

    eprintln!(
        "语料 {docs} 份 / {parts} 个 XML part：{markers} 个标记 → {spans} 个范围\
         （{collapsed} 空范围、{damaged} 缺端点、{refs_only} 只有 reference）"
    );
    eprintln!("按种类：{by_class:?}");
    eprintln!("诊断：{diag_counts:?}");
    assert!(docs > 500, "语料没找到（{docs} 份）");
    assert!(markers > 0);
    assert!(failures.is_empty(), "{} 处问题：\n{}", failures.len(), failures.join("\n"));
}

/// `TEST-09`：孤儿 `bookmarkEnd` 记 `PreExistingDamage` 诊断，保存仍成功且字节不变。
#[test]
fn span_09_hostile_orphan_end_saves_successfully() {
    let path = common::corpus_dir("hostile").join("span-orphan-end.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    let idx = SpanIndex::build(dom);
    assert_eq!(idx.len(), 1);
    let s = &idx.spans()[0];
    assert_eq!(s.start, None);
    assert_eq!(s.pair_id(), "7");
    let d = &idx.diagnostics()[0];
    assert_eq!(d.code, DiagCode::SpanOrphanEnd);
    assert_eq!(d.origin, rsword::ValidationOrigin::PreExistingDamage);
    assert!(d.range.is_some(), "诊断带标记的字节区间");
    assert_eq!(pkg.save().unwrap(), bytes, "未编辑保存字节相同");
}

// ---- SPAN-06 / SPAN-07：编辑期的 Anchor 变换（任务 2.2）----

use rsword::edit::{
    BlockPos, EditContext, EditOp, EditSession, InlinePos, NewBlock, NewInline, NewMarker, NewRun,
};
use rsword::semantic::props::{Change, RunPropsPatch};

/// `<w:p>` 里两个 run 中间夹一个书签：`[ab]` / 书签起点 / `[cd]` / 书签终点。
const TWO_RUNS_BOOKMARKED: &str = r#"<w:p><w:r><w:t>ab</w:t></w:r><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>cd</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#;

fn session(body: &str) -> EditSession {
    EditSession::open(&common::docx_with_body(body)).unwrap()
}

fn first_para(s: &EditSession) -> NodeId {
    s.document().text_blocks().next().unwrap().node
}

fn bookmark(s: &mut EditSession, id: &str) -> rsword::span::RangeSpan {
    s.spans().unwrap().find(RangeClass::Bookmark, id).expect("bookmark").clone()
}

fn bold() -> RunPropsPatch {
    RunPropsPatch { bold: Change::Set(true), ..Default::default() }
}

#[test]
fn span_06_insert_at_the_start_boundary_lands_outside_the_range() {
    let mut s = session(TWO_RUNS_BOOKMARKED);
    let p = first_para(&s);
    let before = bookmark(&mut s, "1");
    assert_eq!((before.start.unwrap().index, before.end.unwrap().index), (1, 2));
    // 边界插入（新 run）：起点 Right 右移、终点也右移 → 文字落在范围外
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: Some(bold()) },
        &EditContext::default(),
    )
    .unwrap();
    let after = bookmark(&mut s, "1");
    assert_eq!((after.start.unwrap().index, after.end.unwrap().index), (2, 3));
    assert!(after.start.unwrap().marker.is_some(), "标记节点没变");
}

#[test]
fn span_06_insert_inside_the_range_extends_it() {
    let mut s = session(TWO_RUNS_BOOKMARKED);
    let p = first_para(&s);
    // "abcd" 的偏移 3 落在第二个 run 内部（范围内），带 props → 先拆 run 再插新 run
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "X".into(), props: Some(bold()) },
        &EditContext::default(),
    )
    .unwrap();
    let bm = bookmark(&mut s, "1");
    // 拆分把 "cd" 变成 "c" + "d"，新 run 插在中间：范围要覆盖三项（起点 1，终点 4）
    assert_eq!((bm.start.unwrap().index, bm.end.unwrap().index), (1, 4));
    assert_eq!(s.document().text_blocks().next().unwrap().text(), "abcXd");
}

#[test]
fn span_06_insert_into_an_existing_run_does_not_move_anchors() {
    let mut s = session(TWO_RUNS_BOOKMARKED);
    let p = first_para(&s);
    // 路径 1（直接写 w:t）：内容序列不变
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 3), text: "X".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    let bm = bookmark(&mut s, "1");
    assert_eq!((bm.start.unwrap().index, bm.end.unwrap().index), (1, 2));
    assert_eq!(s.document().text_blocks().next().unwrap().text(), "abcXd");
}

#[test]
fn span_06_delete_across_the_start_moves_it_to_the_delete_point() {
    // [ab] [cd] 书签起点 [ef] 书签终点：删掉 "bcde" → 起点落到删除点
    let mut s = session(
        r#"<w:p><w:r><w:t>ab</w:t></w:r><w:r><w:t>cd</w:t></w:r><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>ef</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
    );
    let p = first_para(&s);
    let before = bookmark(&mut s, "1");
    assert_eq!((before.start.unwrap().index, before.end.unwrap().index), (2, 3));
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 1), to: InlinePos::new(p, 5) },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(s.document().text_blocks().next().unwrap().text(), "af");
    let bm = bookmark(&mut s, "1");
    // "cd" 整项被删 → 起点从 2 落到 1（"a" 之后）；终点仍在末尾
    assert_eq!((bm.start.unwrap().index, bm.end.unwrap().index), (1, 2));
    assert!(!bm.is_collapsed(), "只删掉一半，范围还在");
}

#[test]
fn span_07_whole_range_delete_collapses_a_bookmark_and_removes_a_comment() {
    let mut s = session(
        r#"<w:p><w:r><w:t>ab</w:t></w:r>
             <w:bookmarkStart w:id="1" w:name="bm"/><w:commentRangeStart w:id="9"/>
             <w:r><w:t>cd</w:t></w:r>
             <w:commentRangeEnd w:id="9"/><w:bookmarkEnd w:id="1"/>
             <w:r><w:commentReference w:id="9"/></w:r></w:p>"#,
    );
    let p = first_para(&s);
    let len = s.document().text_blocks().next().unwrap().text().encode_utf16().count() as u32;
    assert_eq!(len, 4, "commentReference 不占坐标");
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 2), to: InlinePos::new(p, 4) },
        &EditContext::default(),
    )
    .unwrap();
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "1").expect("书签折叠保留");
    assert!(bm.is_collapsed() && bm.start.unwrap().index == 1);
    assert!(idx.find(RangeClass::Comment, "9").is_none(), "批注整体删除");
    let xml = document_xml(&mut s);
    assert!(xml.contains("bookmarkStart"), "{xml}");
    assert!(!xml.contains("commentRangeStart"), "批注标记删除: {xml}");
    assert!(!xml.contains("commentReference"), "reference run 删除: {xml}");
}

#[test]
fn span_07_keep_orphan_comments_collapses_instead() {
    let mut s = session(
        r#"<w:p><w:r><w:t>ab</w:t></w:r><w:commentRangeStart w:id="9"/><w:r><w:t>cd</w:t></w:r>
           <w:commentRangeEnd w:id="9"/><w:r><w:commentReference w:id="9"/></w:r></w:p>"#,
    );
    let p = first_para(&s);
    let ctx = EditContext { keep_orphan_comments: true, ..Default::default() };
    s.apply(EditOp::DeleteRange { from: InlinePos::new(p, 2), to: InlinePos::new(p, 4) }, &ctx)
        .unwrap();
    let idx = s.spans().unwrap();
    let c = idx.find(RangeClass::Comment, "9").expect("批注折叠保留");
    assert!(c.is_collapsed());
    let xml = document_xml(&mut s);
    assert!(xml.contains("commentRangeStart"), "{xml}");
}

#[test]
fn span_06_deleting_a_paragraph_moves_its_bookmark_out_to_the_body() {
    let mut s = session(
        r#"<w:p><w:r><w:t>one</w:t></w:r></w:p>
           <w:p><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>two</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>
           <w:p><w:r><w:t>three</w:t></w:r></w:p>"#,
    );
    let second = s.document().text_blocks().nth(1).unwrap().node;
    s.apply(EditOp::DeleteBlock { node: second }, &EditContext::default()).unwrap();
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "1").expect("书签折叠到段落原位");
    let start = bm.start.unwrap();
    assert!(bm.is_collapsed());
    assert_eq!(start.index, 1, "落在 body 里被删段落原来的边界");
    assert_eq!(start.marker, None, "标记随段落消失，等物化重发（2.3）");
    assert_ne!(start.container, second, "锚点搬到了外层容器");
}

#[test]
fn span_06_inserting_a_block_shifts_body_anchors() {
    let mut s = session(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>one</w:t></w:r></w:p>
           <w:p><w:r><w:t>two</w:t></w:r></w:p>"#,
    );
    let first = first_para(&s);
    // body 层的锚点：段落里的书签不受影响，但如果锚点在 body 上就要移动——这里用块插入验证
    // 段落内锚点不动
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::Before(first),
            block: NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("zero"))],
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    let bm = bookmark(&mut s, "1");
    assert_eq!(bm.start.unwrap().index, 0, "段落内的边界不受块插入影响");
    assert_eq!(s.document().text_blocks().count(), 3);
}

#[test]
fn span_06_replace_inlines_rescans_the_container() {
    let mut s = session(TWO_RUNS_BOOKMARKED);
    let p = first_para(&s);
    // compat 路径整体重写段落内容：旧标记全删，新标记（这里重发一个书签）成为真相
    s.apply(
        EditOp::ReplaceInlines {
            para: p,
            inlines: vec![
                NewInline::Marker(NewMarker::BookmarkStart {
                    id: "5".into(),
                    name: "fresh".into(),
                }),
                NewInline::Run(NewRun::text("new")),
                NewInline::Marker(NewMarker::BookmarkEnd { id: "5".into() }),
            ],
        },
        &EditContext::default(),
    )
    .unwrap();
    let idx = s.spans().unwrap();
    assert!(idx.find(RangeClass::Bookmark, "1").is_none(), "旧书签随内容消失");
    let fresh = idx.find(RangeClass::Bookmark, "5").expect("新书签进索引");
    assert_eq!((fresh.start.unwrap().index, fresh.end.unwrap().index), (0, 1));
    assert!(fresh.start.unwrap().marker.is_some());
}

#[test]
fn span_06_rollback_restores_the_index() {
    let mut s = session(TWO_RUNS_BOOKMARKED);
    let p = first_para(&s);
    let before = bookmark(&mut s, "1");
    let err = s
        .apply_all(
            vec![
                EditOp::InsertText {
                    at: InlinePos::new(p, 2),
                    text: "X".into(),
                    props: Some(bold()),
                },
                EditOp::DeleteRange { from: InlinePos::new(p, 0), to: InlinePos::new(p, 99) },
            ],
            &EditContext::default(),
        )
        .expect_err("第二步越界");
    assert!(matches!(err, rsword::Error::Edit { .. }));
    let after = bookmark(&mut s, "1");
    assert_eq!(before, after, "索引回滚到操作前");
}

fn document_xml(s: &mut EditSession) -> String {
    let bytes = s.save().unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    dom.src().to_string()
}
