//! `PKG-*` 集成测试，语料来自 `corpus/hostile` 与 `corpus/synthetic`。

mod common;

use std::io::{Cursor, Read};

use rsword::package::{PartId, ZipPackage, neutralize_unicode_path};
use rsword::xml::Dom;
use rsword::{DiagCode, Error};

fn hostile(name: &str) -> Vec<u8> {
    std::fs::read(common::corpus_dir("hostile").join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn pkg_01_unicode_path_shadow_resolves_by_header_name() {
    let bytes = hostile("zip-unicode-path-shadow.docx");
    // 不中和时 zip crate 会按 0x7075 字段互换两个条目名（这就是要中和的原因）
    let mut naive = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let mut swapped = String::new();
    naive.by_name("word/document.xml").unwrap().read_to_string(&mut swapped).unwrap();
    assert!(swapped.contains("WRONG"), "precondition: the raw zip crate honours 0x7075");

    let mut pkg = ZipPackage::open(&bytes).unwrap();
    let idx = pkg.find("word/document.xml").expect("document.xml by header name");
    let doc = String::from_utf8(pkg.read(idx).unwrap()).unwrap();
    assert!(doc.contains("RIGHT"));
    let decoy = pkg.find("decoy-content.xml").unwrap();
    assert!(String::from_utf8(pkg.read(decoy).unwrap()).unwrap().contains("WRONG"));
    // 原始字节原样保留（不变式 1 的来源）
    assert_eq!(&pkg.original_bytes()[..], &bytes[..]);
    assert_ne!(neutralize_unicode_path(&bytes), bytes, "the field was actually patched");
}

#[test]
fn pkg_02_limits_are_checked_before_decompression() {
    for (file, code) in [
        ("zip-part-too-large.docx", DiagCode::PkgPartTooLarge),
        ("zip-total-too-large.docx", DiagCode::PkgTotalTooLarge),
        ("zip-too-many-parts.docx", DiagCode::PkgTooManyParts),
    ] {
        match ZipPackage::open(&hostile(file)) {
            Err(Error::Limit { code: got, .. }) => assert_eq!(got, code, "{file}"),
            other => panic!("{file}: expected Limit({code}), got {other:?}"),
        }
    }
}

#[test]
fn pkg_11_entries_keep_zip_order_and_parts_parse() {
    let path = &common::docx_paths("synthetic")[0];
    let bytes = std::fs::read(path).unwrap();
    let mut pkg = ZipPackage::open(&bytes).unwrap();
    let names: Vec<&str> = pkg.entries().iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"[Content_Types].xml"));
    assert!(names.contains(&"word/document.xml"));
    let zip_names: Vec<String> = zip::ZipArchive::new(Cursor::new(bytes.clone()))
        .unwrap()
        .file_names()
        .map(String::from)
        .collect();
    assert_eq!(
        names,
        zip_names.iter().map(String::as_str).collect::<Vec<_>>(),
        "entry order is the zip order"
    );
    let idx = pkg.find("word/document.xml").unwrap();
    assert_eq!(pkg.find_ignore_case("WORD/Document.XML"), Some(idx));
    let xml = pkg.read(idx).unwrap();
    let dom = Dom::parse(PartId(idx), &xml).unwrap();
    assert!(dom.node_count() > 3);
    let e = pkg.entry(idx);
    assert_eq!(e.size as usize, xml.len());
}
