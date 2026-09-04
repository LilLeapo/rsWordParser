//! 文档模型在语料上的验收（任务 1.5–1.8，`MOD-13` / `MOD-06` / `MOD-03`）：
//! `Document::rebuild` 对全部语料无 panic 且幂等；文本段落的类型与坐标流文本与 TS 一致。

mod common;

use std::collections::BTreeMap;

use rsword::model::{Block, Document, Inline, OBJECT_REPLACEMENT, SegmentKind, TextKind};
use rsword::package::Package;
use serde_json::Value;

/// 已知差异（`src/bind/compat_ts/KNOWN_DIFFS.md`）。
const KNOWN_DIFFS: &[(&str, &str)] = &[("symbol-fonts__", "text")];

#[derive(Default)]
struct Stats {
    docs: usize,
    blocks: BTreeMap<&'static str, usize>,
    compared: BTreeMap<&'static str, usize>,
    skipped: BTreeMap<&'static str, usize>,
    mismatches: Vec<String>,
}

/// TS run 文本：`\f` 分页、`\v` 分栏、`\n` 软换行；本引擎分页 / 分栏用 U+FFFC。
fn normalize_ts(text: &str) -> String {
    text.replace(['\u{0C}', '\u{0B}'], &OBJECT_REPLACEMENT.to_string())
}

#[test]
fn mod_13_rebuild_is_idempotent_and_text_blocks_match_ts() {
    let mut st = Stats::default();
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
            st.docs += 1;
            let again = Document::rebuild(&mut pkg).unwrap();
            assert_eq!(doc, again, "{}: rebuild 不幂等", path.display());
            for b in &doc.main {
                let k = match b {
                    Block::Text(_) => "text",
                    Block::Table(_) => "table",
                    Block::Image(_) => "image",
                    Block::Protected(_) => "protected",
                };
                *st.blocks.entry(k).or_default() += 1;
            }

            let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
                continue;
            };
            let e: Value = serde_json::from_str(&text).unwrap();
            let Some(ts_blocks) = e.get("blocks").and_then(Value::as_array) else { continue };
            let Some(body) = doc.body else { continue };
            let dom = pkg.part(doc.main_part).dom().unwrap();
            // docxIndex → body 顶层元素；含 sdt 的文档 TS 会拆分，跳过
            let body_children: Vec<_> =
                dom.semantic_children(body).filter(|&n| dom.name(n).is_some()).collect();
            if body_children
                .iter()
                .any(|&n| dom.name(n).is_some_and(|q| q.local == rsword::xml::LocalName::Sdt))
            {
                *st.skipped.entry("doc_with_sdt").or_default() += 1;
                continue;
            }
            let by_node: BTreeMap<_, _> = doc.main.iter().map(|b| (b.node(), b)).collect();
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            for tb in ts_blocks {
                let ty = tb.get("type").and_then(Value::as_str).unwrap_or("");
                if !matches!(ty, "paragraph" | "heading" | "listItem") {
                    continue;
                }
                let Some(idx) = tb.get("docxIndex").and_then(Value::as_u64) else { continue };
                let Some(&node) = body_children.get(idx as usize) else {
                    st.mismatches.push(format!("{file}: docxIndex {idx} 超出 body 子节点"));
                    continue;
                };
                let Some(ours) = by_node.get(&node) else {
                    st.mismatches.push(format!("{file}: docxIndex {idx} 本引擎没有块"));
                    continue;
                };
                let Block::Text(t) = ours else {
                    st.mismatches.push(format!(
                        "{file}: docxIndex {idx} TS {ty}，本引擎 {}",
                        block_kind(ours)
                    ));
                    continue;
                };
                // 字段 / 符号 / 图片 / 脚注等 TS 另有折叠规则（M2），跳过含这些段的段落
                let has_special = t.inlines.iter().any(|i| match i {
                    Inline::Run(r) => r.segments.iter().any(|s| {
                        matches!(
                            s.kind,
                            SegmentKind::FldChar
                                | SegmentKind::InstrText
                                | SegmentKind::DelInstrText
                                | SegmentKind::Sym { .. }
                                | SegmentKind::Drawing { .. }
                                | SegmentKind::Pict
                                | SegmentKind::Object
                                | SegmentKind::Ruby { .. }
                                | SegmentKind::FootnoteRef { .. }
                                | SegmentKind::EndnoteRef { .. }
                                | SegmentKind::SoftHyphen
                                | SegmentKind::DelText
                                | SegmentKind::Other(_)
                        )
                    }),
                    Inline::Atom(_) | Inline::Field { .. } => true,
                }) || t.facts.revision.any();
                let our_ty = match &t.kind {
                    TextKind::Paragraph => "paragraph",
                    TextKind::Heading { .. } => "heading",
                    TextKind::ListItem { .. } => "listItem",
                };
                *st.compared.entry("type").or_default() += 1;
                if our_ty != ty && !known(&file, "type") {
                    st.mismatches
                        .push(format!("{file}: docxIndex {idx} type TS={ty} ours={our_ty}"));
                    continue;
                }
                match &t.kind {
                    TextKind::Heading { level } => {
                        let ts_level = tb.get("level").and_then(Value::as_u64);
                        *st.compared.entry("level").or_default() += 1;
                        if ts_level != Some(u64::from(*level)) {
                            st.mismatches.push(format!(
                                "{file}: docxIndex {idx} level TS={ts_level:?} ours={level}"
                            ));
                        }
                    }
                    TextKind::ListItem { list } => {
                        let l = tb.get("list");
                        let ts_num = l
                            .and_then(|l| l.get("numId"))
                            .and_then(Value::as_str)
                            .and_then(|s| s.parse::<i32>().ok());
                        let ts_ilvl = l.and_then(|l| l.get("ilvl")).and_then(Value::as_i64);
                        *st.compared.entry("list").or_default() += 1;
                        if ts_num != Some(list.num_id) || ts_ilvl != Some(i64::from(list.ilvl)) {
                            st.mismatches.push(format!(
                                "{file}: docxIndex {idx} list TS={l:?} ours={list:?}"
                            ));
                        }
                    }
                    TextKind::Paragraph => {}
                }
                let ts_style = tb.get("styleId").and_then(Value::as_str);
                *st.compared.entry("styleId").or_default() += 1;
                if ts_style != t.style_id.as_deref() {
                    st.mismatches.push(format!(
                        "{file}: docxIndex {idx} styleId TS={ts_style:?} ours={:?}",
                        t.style_id
                    ));
                }
                if has_special {
                    *st.skipped.entry("special_runs").or_default() += 1;
                    continue;
                }
                let ts_text: String = tb
                    .get("runs")
                    .and_then(Value::as_array)
                    .map(|rs| {
                        rs.iter().filter_map(|r| r.get("text").and_then(Value::as_str)).collect()
                    })
                    .unwrap_or_default();
                let ours = t.text();
                *st.compared.entry("text").or_default() += 1;
                if normalize_ts(&ts_text) != ours && !known(&file, "text") {
                    st.mismatches
                        .push(format!("{file}: docxIndex {idx} text TS={ts_text:?} ours={ours:?}"));
                }
            }
        }
    }
    eprintln!("model: {} docs; blocks {:?}", st.docs, st.blocks);
    eprintln!("model: compared {:?}; skipped {:?}", st.compared, st.skipped);
    for m in st.mismatches.iter().take(40) {
        eprintln!("model: MISMATCH {m}");
    }
    assert!(st.docs > 100);
    assert!(st.compared["text"] > 300, "{:?}", st.compared);
    assert!(st.mismatches.is_empty(), "{} mismatches against TS", st.mismatches.len());
}

fn known(file: &str, what: &str) -> bool {
    KNOWN_DIFFS.iter().any(|(p, w)| file.starts_with(p) && *w == what)
}

fn block_kind(b: &Block) -> &'static str {
    match b {
        Block::Text(_) => "text",
        Block::Table(_) => "table",
        Block::Image(_) => "image",
        Block::Protected(p) => p.kind.key(),
    }
}
