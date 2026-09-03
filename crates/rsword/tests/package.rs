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

// ---------------------------------------------------------------------------------------------
// 任务 0.5：Package / 主 part / flavor / NamespaceContext
// ---------------------------------------------------------------------------------------------

use std::io::Write;

use rsword::NotOoxml;
use rsword::package::{Package, PackageFlavor, PartFlavor, PartUri, RelType};
use rsword::xml::{LocalName, NsId, QName};

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
const W_URI: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn content_types(overrides: &[(&str, &str)]) -> String {
    let mut s = format!(
        "{XML_DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>"
    );
    for (p, ct) in overrides {
        s.push_str(&format!("<Override PartName=\"{p}\" ContentType=\"{ct}\"/>"));
    }
    s.push_str("</Types>");
    s
}

#[test]
fn pkg_03_main_part_via_office_document_relationship() {
    let rels = format!(
        "{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/trial.xml\"/>\
</Relationships>"
    );
    let doc =
        format!("{XML_DECL}<w:document xmlns:w=\"{W_URI}\"><w:body><w:p/></w:body></w:document>");
    let ct = content_types(&[(
        "/word/trial.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    )]);
    let bytes = build_zip(&[
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/trial.xml", doc.as_bytes()),
    ]);
    let pkg = Package::open(&bytes).unwrap();
    assert_eq!(pkg.part(pkg.main_part()).uri, PartUri::from_entry_name("word/trial.xml"));
    assert_eq!(pkg.flavor(), PackageFlavor::Transitional);
    assert_eq!(pkg.root_rels().of_kind(RelType::OfficeDocument).count(), 1);
    assert!(pkg.part(pkg.main_part()).is_parsed(), "main part is parsed at open");
}

#[test]
fn pkg_03_non_ooxml_inputs() {
    let odt = build_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<office:document-content/>"),
    ]);
    match Package::open(&odt).unwrap_err() {
        Error::NotOoxml(NotOoxml::OpenDocument(m)) => {
            assert_eq!(m, "application/vnd.oasis.opendocument.text")
        }
        other => panic!("expected OpenDocument, got {other:?}"),
    }
    let empty = build_zip(&[]);
    assert!(matches!(
        Package::open(&empty).unwrap_err(),
        Error::NotOoxml(NotOoxml::MissingMainPart)
    ));
    let no_main = build_zip(&[
        ("[Content_Types].xml", content_types(&[]).as_bytes()),
        ("word/styles.xml", b"<a/>"),
    ]);
    assert!(matches!(
        Package::open(&no_main).unwrap_err(),
        Error::NotOoxml(NotOoxml::MissingMainPart)
    ));
}

#[test]
fn pkg_08_flavor_classification() {
    let strict =
        std::fs::read(common::corpus_dir("synthetic").join("extra__strict-minimal.docx")).unwrap();
    let pkg = Package::open(&strict).unwrap();
    assert_eq!(pkg.flavor(), PackageFlavor::Strict);
    assert_eq!(pkg.part(pkg.main_part()).flavor, Some(PartFlavor::Strict));
    assert_eq!(pkg.flavor_of(pkg.main_part()), PartFlavor::Strict);
    assert!(
        pkg.root_rels().of_kind(RelType::OfficeDocument).next().unwrap().family
            == Some(PartFlavor::Strict)
    );

    let mixed = Package::open(&hostile("mixed-flavor.docx")).unwrap();
    assert_eq!(mixed.flavor(), PackageFlavor::Mixed);
    assert!(mixed.diagnostics().iter().any(|d| d.code == DiagCode::PkgMixedFlavor));
    let main = mixed.main_part();
    assert_eq!(mixed.part(main).flavor, Some(PartFlavor::Strict));
    let header = mixed.related(main, RelType::Header).next().expect("header via Strict rel type");
    assert_eq!(mixed.part(header).flavor, Some(PartFlavor::Transitional));
    assert_eq!(mixed.flavor_of(header), PartFlavor::Transitional, "generation follows the part");
    assert_eq!(mixed.flavor_of(main), PartFlavor::Strict);

    let plain = std::fs::read(&common::docx_paths("synthetic")[0]).unwrap();
    let pkg = Package::open(&plain).unwrap();
    assert_eq!(pkg.flavor(), PackageFlavor::Transitional);
    let ct = pkg.content_types_part().unwrap();
    assert_eq!(pkg.part(ct).flavor, None, "package-level parts have no flavor");
}

#[test]
fn pkg_11_unparseable_parts() {
    // header 畸形：包打开成功，header 为 Opaque + 诊断
    let mut pkg = Package::open(&hostile("xml-unbalanced-header.docx")).unwrap();
    let main = pkg.main_part();
    let header = pkg.related(main, RelType::Header).next().unwrap();
    assert!(pkg.dom(header).unwrap().is_none());
    assert!(pkg.part(header).is_opaque());
    assert!(pkg.part(header).opaque_error().is_some());
    assert!(
        pkg.diagnostics().iter().any(|d| d.code == DiagCode::PkgOpaquePart && d.part == header)
    );
    let raw = pkg.read_bytes(header).unwrap();
    assert!(raw.starts_with(b"<?xml"), "opaque part still yields its original bytes");
    // 主 part 畸形：整体 Err
    match Package::open(&hostile("xml-unbalanced-main.docx")).unwrap_err() {
        Error::Malformed { part, .. } => assert_eq!(part, "word/document.xml"),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn pkg_04_missing_content_types_is_a_diagnostic() {
    let pkg = Package::open(&hostile("content-types-missing.docx")).unwrap();
    assert!(pkg.diagnostics().iter().any(|d| d.code == DiagCode::PkgNoContentTypes));
    assert!(pkg.content_types_part().is_none());
    assert!(pkg.part(pkg.main_part()).is_xml, "xml-ness falls back to the extension");
    assert_eq!(pkg.part(pkg.main_part()).content_type, None);
}

#[test]
fn pkg_09_namespace_context_from_root() {
    let doc = format!(
        "{XML_DECL}<w:document xmlns:w=\"{W_URI}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" \
xmlns:x=\"urn:vendor\" xmlns:x2=\"urn:vendor\" mc:Ignorable=\"w14 x\"><w:body><w:p/></w:body></w:document>"
    );
    let ct = content_types(&[(
        "/word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    )]);
    let bytes =
        build_zip(&[("[Content_Types].xml", ct.as_bytes()), ("word/document.xml", doc.as_bytes())]);
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    let ctx = pkg.namespace_context(main).unwrap().unwrap();
    assert_eq!(ctx.prefix_for(NsId::W), Some("w"));
    assert!(ctx.declares(NsId::W) && ctx.declares(NsId::R) && ctx.declares(NsId::Mc));
    assert_eq!(ctx.ignorable, vec!["w14".to_string(), "x".to_string()]);
    assert_eq!(ctx.flavor, PartFlavor::Transitional);
    assert!(ctx.is_understood(NsId::W14));
    assert_eq!(ctx.prefix_for(NsId::Wpc), Some("wpc"), "canonical prefix when not declared");
    assert_eq!(ctx.root_decls.len(), 6);
    let vendor = ctx.root_decls.iter().find(|(p, _)| p.as_deref() == Some("x")).unwrap().1;
    assert!(matches!(vendor, NsId::Other(_)));
    assert_eq!(ctx.prefix_for(vendor), Some("x"), "first prefix wins for a URI declared twice");
    let dom = pkg.dom(main).unwrap().unwrap();
    assert_eq!(dom.name(dom.root()), Some(QName::w(LocalName::Document)));
    // 二进制 part 没有上下文
    let ct_part = pkg.content_types_part().unwrap();
    assert!(
        pkg.namespace_context(ct_part).unwrap().is_some(),
        "[Content_Types].xml is an XML part"
    );
}

#[test]
fn pkg_open_all_synthetic_corpus() {
    let mut opened = 0usize;
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let ids: Vec<_> = pkg.parts().iter().filter(|p| p.is_xml).map(|p| p.id).collect();
        for id in ids {
            pkg.dom(id).unwrap();
        }
        assert!(
            !pkg.diagnostics().iter().any(|d| d.code == DiagCode::PkgOpaquePart),
            "{}",
            path.display()
        );
        opened += 1;
    }
    assert!(opened > 500);
}
