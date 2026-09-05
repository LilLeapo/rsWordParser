//! `XML-03` / `XML-13`：对语料中每个 XML part，`parse → serialize` 字节相同，且 `Lex` 子区间有序不重叠。
//! 任务 0.3 之前直接用 `zip` crate 读 part；包层就位后改走 `Package`。

mod common;

use std::fs::File;
use std::io::Read;

use rsword::package::PartId;
use rsword::save::serialize;
use rsword::xml::{Dom, NodeKind};

/// `XML-03` 不变式：子节点区间落在父的内容区间内、互不重叠、按顺序。
fn check_lex_invariants(dom: &Dom) -> Result<(), String> {
    for id in dom.descendants(dom.root()) {
        let node = dom.node(id);
        let Some(lex) = &node.lex else { return Err(format!("node {} has no lex", id.0)) };
        if let NodeKind::Element(e) = &node.kind {
            if lex.open.start != lex.range.start {
                return Err(format!("node {}: open.start != range.start", id.0));
            }
            if !lex.is_self_closing() && lex.close.end != lex.range.end {
                return Err(format!("node {}: close.end != range.end", id.0));
            }
            let content = lex.content();
            let mut prev = content.start;
            for &c in &e.children {
                let cl = dom.node(c).lex.as_ref().ok_or("child without lex")?;
                if cl.range.start < prev || cl.range.end > content.end {
                    return Err(format!("node {}: child {} out of order/overlapping", id.0, c.0));
                }
                prev = cl.range.end;
            }
        }
    }
    Ok(())
}

#[test]
fn xml_13_clean_roundtrip_all_corpus() {
    let mut parts = 0usize;
    let mut docs = 0usize;
    let mut failures: Vec<String> = Vec::new();
    // 已知必须解析失败的 part（TEST-09）
    let expected_err = [
        ("xml-unbalanced-main", "word/document.xml"),
        ("xml-unbalanced-header", "word/header1.xml"),
        // 页眉 part 整个是二进制垃圾（任务 5.9 的 hostile）：解析必须失败，part 降级为 `Opaque`
        ("hf-part-binary", "word/header1.xml"),
    ];

    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let file = File::open(&path).unwrap();
            let mut zip = match zip::ZipArchive::new(file) {
                Ok(z) => z,
                Err(e) if kind == "hostile" => {
                    eprintln!("skip {stem}: zip unreadable ({e})");
                    continue;
                }
                Err(e) => panic!("{stem}: {e}"),
            };
            docs += 1;
            for i in 0..zip.len() {
                let mut entry = match zip.by_index(i) {
                    Ok(e) => e,
                    Err(_) if kind == "hostile" => continue,
                    Err(e) => panic!("{stem}: entry {i}: {e}"),
                };
                let name = entry.name().to_string();
                if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                    continue;
                }
                let mut bytes = Vec::new();
                if entry.read_to_end(&mut bytes).is_err() {
                    if kind == "hostile" {
                        continue;
                    }
                    panic!("{stem}: cannot read {name}");
                }
                parts += 1;
                let must_fail = expected_err.iter().any(|(s, n)| *s == stem && *n == name);
                match Dom::parse(PartId(i as u32), &bytes) {
                    Ok(dom) => {
                        if must_fail {
                            failures.push(format!("{stem}:{name}: expected parse failure"));
                            continue;
                        }
                        if let Err(e) = check_lex_invariants(&dom) {
                            failures.push(format!("{stem}:{name}: {e}"));
                        }
                        // XML-01：转码过的 part 只能与转码后的字节一致
                        let transcoded_expected: Vec<u8>;
                        let expected: &[u8] = if dom.transcoded() {
                            // 转码 part：与转码后的字节一致，且 XML 声明的 encoding 改为 UTF-8
                            transcoded_expected = String::from_utf8(dom.src_bytes().to_vec())
                                .unwrap()
                                .replacen("encoding=\"UTF-16\"", "encoding=\"UTF-8\"", 1)
                                .into_bytes();
                            &transcoded_expected
                        } else {
                            &bytes
                        };
                        match serialize(&dom) {
                            Ok(out) if out == expected => {}
                            Ok(out) => {
                                let first = out
                                    .iter()
                                    .zip(expected)
                                    .position(|(a, b)| a != b)
                                    .unwrap_or(out.len().min(expected.len()));
                                failures.push(format!("{stem}:{name}: roundtrip differs at byte {first} (out {} vs in {})", out.len(), expected.len()));
                            }
                            Err(e) => failures.push(format!("{stem}:{name}: serialize error {e}")),
                        }
                    }
                    Err(e) => {
                        if !must_fail {
                            failures.push(format!("{stem}:{name}: {e}"));
                        }
                    }
                }
            }
        }
    }
    eprintln!("roundtrip: {docs} docs, {parts} xml parts");
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
    assert!(parts > 1000, "corpus too small: {parts} parts");
}
