//! `SAVE-01`（不变式 1）、`SAVE-06`（未变条目原样）、`TEST-04` 单节点编辑保真（M0 L1 全量 + M1 L4 全量）。

mod common;

use std::io::{Cursor, Read};

use rsword::bind::compat_ts;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::model::{Inline, SegmentKind};
use rsword::package::Package;
use rsword::xml::{Dirty, LocalName, NodeKind, QName};

fn entries(bytes: &[u8]) -> Vec<(String, u32, u64, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut v = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index_raw(i).unwrap();
        let mut raw = Vec::new();
        f.read_to_end(&mut raw).unwrap();
        v.push((f.name().to_string(), f.crc32(), f.compressed_size(), raw));
    }
    v
}

#[test]
fn save_01_no_edit_returns_original_bytes_for_all_corpus() {
    let mut n = 0;
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            assert!(!pkg.is_dirty());
            let saved = pkg.save().unwrap();
            assert_eq!(saved, bytes, "{}", path.display());
            n += 1;
        }
    }
    assert!(n > 570, "{n}");
}

/// 找到主 part 里第一个非空 `w:t` 文本节点。
fn first_text(pkg: &mut Package) -> (rsword::package::PartId, rsword::xml::NodeId) {
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    let t = QName::w(LocalName::T);
    for id in dom.descendants(dom.root()) {
        if dom.is(id, t) {
            for &c in dom.children(id) {
                if matches!(dom.node(c).kind, NodeKind::Text(_))
                    && dom.text(c).is_some_and(|s| !s.trim().is_empty())
                {
                    return (main, c);
                }
            }
        }
    }
    panic!("no w:t text in main part");
}

#[test]
fn save_06_untouched_entries_keep_crc_and_compressed_bytes() {
    let path = common::docx_paths("synthetic")
        .into_iter()
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("word-basics__"))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let (main, text) = first_text(&mut pkg);
    let dom = pkg.dom_mut(main).unwrap().unwrap();
    let old = dom.text(text).unwrap().into_owned();
    dom.set_text(text, format!("{old} EDITED<&>"));
    assert_eq!(pkg.dirty_parts(), vec![main]);
    let saved = pkg.save().unwrap();
    assert_ne!(saved, bytes);

    let before = entries(&bytes);
    let after = entries(&saved);
    assert_eq!(before.len(), after.len());
    let main_name = pkg.part(main).uri.to_string();
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(a.0, b.0, "entry order and names preserved");
        if a.0 == main_name {
            assert_ne!(a.1, b.1, "edited part has a new CRC");
        } else {
            assert_eq!(a.1, b.1, "{}: crc", a.0);
            assert_eq!(a.2, b.2, "{}: compressed size", a.0);
            assert_eq!(a.3, b.3, "{}: compressed bytes", a.0);
        }
    }
    // 重解析：改动可见，其他干净节点原字节在输出中
    let mut reopened = Package::open(&saved).unwrap();
    let main2 = reopened.main_part();
    let doc2 = reopened.read_bytes(main2).unwrap();
    let doc2 = String::from_utf8(doc2).unwrap();
    assert!(doc2.contains("EDITED&lt;&amp;&gt;"), "{doc2}");
    assert!(doc2.contains("xml:space=\"preserve\""));
    let dom_old = Package::open(&bytes).unwrap();
    let _ = dom_old;
    let doc1 = String::from_utf8(pkg.read_bytes(main).unwrap()).unwrap();
    let dom1 = pkg.dom(main).unwrap().unwrap();
    // 所有 Clean 元素的原文都是输出子串
    for id in dom1.descendants(dom1.root()) {
        let node = dom1.node(id);
        if node.dirty == Dirty::Clean
            && let Some(lex) = &node.lex
            && matches!(node.kind, NodeKind::Element(_))
        {
            let s = &doc1[lex.range.start as usize..lex.range.end as usize];
            assert!(doc2.contains(s), "clean node {} missing from output: {s}", id.0);
        }
    }
    assert!(reopened.dom(main2).unwrap().is_some());
}

#[test]
fn test_04_single_node_edit_roundtrips_on_every_synthetic_doc() {
    let mut edited = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let main = pkg.main_part();
        let dom = pkg.dom(main).unwrap().unwrap();
        let t = QName::w(LocalName::T);
        let target = dom
            .descendants(dom.root())
            .find(|&id| dom.is(id, t))
            .and_then(|id| dom.children(id).first().copied());
        let Some(text) = target else { continue };
        let dom = pkg.dom_mut(main).unwrap().unwrap();
        dom.set_text(text, "Ж");
        let saved = pkg.save().unwrap();
        let mut reopened = Package::open(&saved)
            .unwrap_or_else(|e| panic!("{}: reopen failed: {e}", path.display()));
        let m2 = reopened.main_part();
        let dom2 = reopened.dom(m2).unwrap().unwrap();
        assert!(
            dom2.descendants(dom2.root()).any(|id| dom2.text(id).is_some_and(|s| s == "Ж")),
            "{}",
            path.display()
        );
        edited += 1;
    }
    assert!(edited > 400, "{edited}");
}

#[test]
fn test_04_corpus_edit_fidelity() {
    const INSERTED: &str = "Ж";

    // 在合法 UTF-16 偏移处插入字符串；段坐标流偏移来自 EditSession 投影，因此必须精确。
    fn insert_at_utf16(text: &str, at: u32, insert: &str) -> String {
        let mut units = 0u32;
        for (byte, ch) in text.char_indices() {
            if units == at {
                return format!("{}{}{}", &text[..byte], insert, &text[byte..]);
            }
            units += ch.len_utf16() as u32;
        }
        assert_eq!(units, at, "UTF-16 offset out of range");
        format!("{text}{insert}")
    }

    let mut edited = 0;
    let mut skipped_no_text_block = 0;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let mut s = EditSession::open(&bytes)
            .unwrap_or_else(|e| panic!("{}: EditSession::open failed: {e}", path.display()));

        // M1 的 Document 只投影正文顶层的 w:p；表格单元格等段落属于 M2。对这类文档没有
        // EditSession 可用的文本段落，跳过；其余每份都选一个非空 Text 段，全都没有时退化为
        // 在第一个文本段落的 offset 0 处插入（会新建 run，但同样走 L4 InsertText）。
        let Some((fallback_idx, first_block)) = s.document().text_blocks().enumerate().next()
        else {
            skipped_no_text_block += 1;
            continue;
        };
        let (fallback_node, fallback_text) = (first_block.node, first_block.text());
        let mut target = None;
        'blocks: for (block_idx, block) in s.document().text_blocks().enumerate() {
            let mut offset = 0u32;
            for inline in &block.inlines {
                let Inline::Run(run) = inline else {
                    offset += inline.utf16_len();
                    continue;
                };
                let mut segment_start = offset;
                for segment in &run.segments {
                    if segment.kind == SegmentKind::Text
                        && segment.utf16_len > 0
                        && run.rev.as_ref().is_none_or(|rev| rev.del.is_none())
                    {
                        target = Some((
                            block_idx,
                            InlinePos::new(block.node, segment_start),
                            block.text(),
                        ));
                        break 'blocks;
                    }
                    segment_start += segment.utf16_len;
                }
                offset += inline.utf16_len();
            }
        }
        let (target_idx, at, before_text) = target
            .unwrap_or_else(|| (fallback_idx, InlinePos::new(fallback_node, 0), fallback_text));

        let before_text_block_count = s.document().text_blocks().count();
        // 空的媒体表：这个 oracle 比的是「编辑前后哪些块变了」，两侧用同一张表就够；
        // 真正读字节的 `MediaSet::build` 要 `&mut Package`，这里只有不可变借用。
        let media = compat_ts::MediaSet::default();
        let before_compat_blocks =
            compat_ts::parsed_doc_of(s.package(), s.document(), &media)["blocks"]
                .as_array()
                .unwrap()
                .clone();
        let main_name = s.package().part(s.main_part()).uri.to_string();

        let result = s
            .apply(
                EditOp::InsertText { at, text: INSERTED.to_string(), props: None },
                &EditContext::default(),
            )
            .unwrap_or_else(|e| panic!("{}: apply(InsertText) failed: {e}", path.display()));
        assert!(!result.structure_changed, "{}", path.display());
        let expected = insert_at_utf16(&before_text, at.offset.0, INSERTED);
        assert_eq!(
            s.document().text_blocks().nth(target_idx).unwrap().text(),
            expected,
            "{}: 编辑后投影文本不正确",
            path.display()
        );

        let saved = s.save().unwrap_or_else(|e| panic!("{}: save failed: {e}", path.display()));
        assert_ne!(saved, bytes, "{}: 编辑后保存不能回到原字节", path.display());

        // TEST-04 / SAVE-06：除主 part 外，每个 zip 条目的 CRC、压缩大小与压缩字节都保持不变。
        let before_entries = entries(&bytes);
        let after_entries = entries(&saved);
        assert_eq!(before_entries.len(), after_entries.len(), "{}", path.display());
        for (b, a) in before_entries.iter().zip(&after_entries) {
            assert_eq!(a.0, b.0, "{}: zip 条目顺序或名字变化", path.display());
            if a.0 == main_name {
                assert_ne!(a.1, b.1, "{}: 主 part CRC 应变化", path.display());
            } else {
                assert_eq!(a.1, b.1, "{}: {} CRC 变化", path.display(), a.0);
                assert_eq!(a.2, b.2, "{}: {} 压缩大小变化", path.display(), a.0);
                assert_eq!(a.3, b.3, "{}: {} 压缩字节变化", path.display(), a.0);
            }
        }

        let reopened = EditSession::open(&saved)
            .unwrap_or_else(|e| panic!("{}: saved bytes reopen failed: {e}", path.display()));
        assert_eq!(
            before_text_block_count,
            reopened.document().text_blocks().count(),
            "{}: 重解析后文本段落数变化",
            path.display()
        );
        assert_eq!(
            reopened.document().text_blocks().nth(target_idx).unwrap().text(),
            expected,
            "{}: 重解析后目标段文本不正确",
            path.display()
        );

        // TEST-04 的补缺 oracle：compat_ts 的 blocks[] 是正文块投影（paragraph 类型含
        // style/format/runs/bookmarks），不含 arena NodeId；重解析后应只有被插字的块发生变化。
        let after_compat_blocks =
            compat_ts::parsed_doc_of(reopened.package(), reopened.document(), &media)["blocks"]
                .as_array()
                .unwrap()
                .clone();
        assert_eq!(
            before_compat_blocks.len(),
            after_compat_blocks.len(),
            "{}: compat_ts 块数变化",
            path.display()
        );
        let changed_blocks: Vec<usize> = before_compat_blocks
            .iter()
            .zip(&after_compat_blocks)
            .enumerate()
            .filter_map(|(idx, (b, a))| (b != a).then_some(idx))
            .collect();
        assert_eq!(
            changed_blocks.len(),
            1,
            "{}: 除目标块外 compat_ts 块投影被修改：{changed_blocks:?}",
            path.display()
        );
        edited += 1;
    }
    assert!(
        edited > 400,
        "只覆盖了 {edited} 份文档（跳过 {skipped_no_text_block} 份无正文顶层 TextBlock）"
    );
}
