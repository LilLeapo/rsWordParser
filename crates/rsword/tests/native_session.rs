//! BIND-01/05/07/08/09/10：会话边界、只读出口与预算。
mod common;
use rsword::bind::native::{SessionTable, document_schema};
use rsword::edit::EditSession;
use rsword::package::PartId;
use rsword::xml::Dom;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
fn value(s: &str) -> Value {
    let mut de = serde_json::Deserializer::from_str(s);
    de.disable_recursion_limit();
    Value::deserialize(&mut de).unwrap()
}
fn blank() -> Vec<u8> {
    common::docx_with_body("<w:p><w:r><w:t>before</w:t></w:r></w:p>")
}
fn snapshot(table: &mut SessionTable, id: &str) -> (String, String, Vec<u8>) {
    (
        table.document(id, None).unwrap(),
        table.diagnostics(id).unwrap(),
        table.save(id, None).unwrap(),
    )
}
#[test]
fn bind_01_close_is_idempotent_and_ids_are_process_wide() {
    let mut a = SessionTable::default();
    let mut b = SessionTable::default();
    let id = a.open(&blank(), None).unwrap();
    let other = b.open(&blank(), None).unwrap();
    assert_ne!(id, other);
    a.close(&id);
    a.close(&id);
    assert_eq!(a.document(&id, None).unwrap_err().code, "BIND_NO_SESSION");
    assert_eq!(a.document(&other, None).unwrap_err().code, "BIND_NO_SESSION");
    assert!(b.document(&other, None).is_ok());
}
#[test]
fn bind_08_version_negotiation() {
    let mut t = SessionTable::default();
    assert_eq!(SessionTable::version()["protocol"], "native/0");
    assert!(!SessionTable::version()["git"].as_str().unwrap().is_empty());
    assert_eq!(
        t.open(&blank(), Some(r#"{"expectProtocol":"native/99"}"#)).unwrap_err().code,
        "BIND_PROTOCOL_MISMATCH"
    );
    assert!(t.open(&blank(), Some(r#"{"expectProtocol":"native/0"}"#)).is_ok());
    assert_eq!(t.open(b"bad zip", None).unwrap_err().code, "ZIP");
}
#[test]
fn bind_01_apply_and_functional_save() {
    let bytes = blank();
    let engine = EditSession::open(&bytes).unwrap();
    let para = engine.document().paragraphs().next().unwrap().node.0;
    let mut t = SessionTable::default();
    let id = t.open(&bytes, None).unwrap();
    let before = snapshot(&mut t, &id);
    for op in [
        "not json",
        r#"{"op":"insertBlock","pos":{"index":99999},"block":{"kind":"xml","xml":"<zz:p xmlns:zz='urn:failed'/>"}}"#,
    ] {
        assert!(t.apply(&id, op, None).is_err());
        assert_eq!(snapshot(&mut t, &id), before);
    }
    assert_eq!(
        t.save(&id, Some(r#"{"removePersonalInfo":"bad"}"#)).unwrap_err().code,
        "BIND_BAD_ARGUMENT"
    );
    assert_eq!(snapshot(&mut t, &id), before);
    let op = json!({"op":"insertText","at":{"para":para,"offset":0},"text":"after "}).to_string();
    t.apply(&id, &op, None).unwrap();
    let edited = snapshot(&mut t, &id);
    assert_ne!(before.0, edited.0);
    assert_ne!(before.2, edited.2);
    assert_eq!(snapshot(&mut t, &id), edited, "save 不提交保存副作用");
}
#[test]
fn bind_05_media_dedup_existing_and_new() {
    let mut t = SessionTable::default();
    let id = t.open(&blank(), None).unwrap();
    let a = t.add_media(&id, b"image a", "image/png").unwrap();
    let b = t.add_media(&id, b"image b", "image/png").unwrap();
    assert_ne!(a, b);
    assert_eq!(a, t.add_media(&id, b"image a", "image/png").unwrap());
    assert_eq!(t.media(&id, b).unwrap(), b"image b");
    let saved = t.save(&id, None).unwrap();
    let reopened = t.open(&saved, None).unwrap();
    let model = value(&t.document(&reopened, None).unwrap());
    let entry = model["media"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| t.media(&reopened, e["mediaId"].as_u64().unwrap() as u32).unwrap() == b"image a")
        .unwrap();
    assert_eq!(
        t.add_media(&reopened, b"image a", "image/png").unwrap(),
        entry["mediaId"].as_u64().unwrap() as u32
    );
    assert_eq!(t.media(&id, u32::MAX).unwrap_err().code, "BIND_ID_UNKNOWN");
}
#[test]
fn bind_09_invalid_part_and_arena_are_bounded() {
    let mut t = SessionTable::default();
    let id = t.open(&blank(), None).unwrap();
    assert_eq!(t.part_bytes(&id, u32::MAX).unwrap_err().code, "BIND_ID_UNKNOWN");
    assert_eq!(t.node_xml(&id, 0, Some(u32::MAX)).unwrap_err().code, "BIND_ID_UNKNOWN");
    assert_eq!(t.node_xml(&id, u32::MAX, None).unwrap_err().code, "BIND_ID_UNKNOWN");
    assert_eq!(t.resolve_runs(&id, "[0]", Some(u32::MAX)).unwrap_err().code, "BIND_ID_UNKNOWN");
    assert_eq!(
        value(&t.resolve_runs(&id, "[4294967295,0]", None).unwrap()),
        json!([{"error":"BIND_ID_UNKNOWN"},{"error":"BIND_ID_UNKNOWN"}])
    );
}
#[test]
fn bind_10_selection_matches_full_schema() {
    let body = "<w:p><w:r><w:t>paragraph</w:t></w:r></w:p>".repeat(100);
    let mut t = SessionTable::default();
    let id = t.open(&common::docx_with_body(&body), None).unwrap();
    let full = value(&t.document(&id, None).unwrap());
    let selected = value(
        &t.document(&id, Some(r#"{"blockRange":{"from":10,"to":20},"fields":["main"]}"#)).unwrap(),
    );
    assert_eq!(selected["main"], json!(&full["main"].as_array().unwrap()[10..20]));
    assert_eq!(selected["totalBlocks"], 100);
    assert_eq!(selected["truncated"], true);
    for key in ["spans", "fields", "revisions"] {
        assert_eq!(selected[key], full[key]);
    }
    jsonschema::validator_for(&document_schema()).unwrap().validate(&selected).unwrap();
    for options in [r#"{"depth":-1}"#, r#"{"blockRange":{"from":2,"to":1}}"#] {
        assert_eq!(t.document(&id, Some(options)).unwrap_err().code, "BIND_BAD_ARGUMENT");
    }
}
#[test]
fn bind_05_09_read_only_full_corpus() {
    const REFUSED: [&str; 4] = [
        "xml-unbalanced-main.docx",
        "zip-part-too-large.docx",
        "zip-too-many-parts.docx",
        "zip-total-too-large.docx",
    ];
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103);
    let mut refused = BTreeSet::new();
    let mut opened = 0;
    let mut parts = 0;
    let mut media = 0;
    let mut nodes = 0;
    let mut t = SessionTable::default();
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let result = t.open(&bytes, None);
        if REFUSED.contains(&name) {
            assert!(result.is_err(), "{}", path.display());
            refused.insert(name.to_owned());
            continue;
        }
        let id = result.unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        opened += 1;
        let s = EditSession::open(&bytes).unwrap();
        let before = (t.document(&id, None).unwrap(), t.diagnostics(&id).unwrap());
        for part in s.package().parts() {
            let expected = s.package().zip().clone().read(part.zip_index).unwrap();
            assert_eq!(
                t.part_bytes(&id, part.id.0).unwrap(),
                expected,
                "{} {}",
                path.display(),
                part.uri
            );
            parts += 1;
            if let Some(dom) = part.dom() {
                // 根 + 所有已建模块：根检查整 part，子块检查继承命名空间的独立输出。
                let mut ids = vec![dom.root()];
                if let Some(blocks) = s.document().blocks_of_part(part.id) {
                    ids.extend(blocks.into_iter().map(|b| b.node()));
                }
                for node in ids {
                    let xml = t.node_xml(&id, node.0, Some(part.id.0)).unwrap();
                    let reparsed = Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| {
                        panic!("{} part {} node {}: {e:?}", path.display(), part.id.0, node.0)
                    });
                    assert_eq!(
                        rsword::xml::canon::canonical(dom, node, &Default::default()),
                        rsword::xml::canon::canonical(
                            &reparsed,
                            reparsed.root(),
                            &Default::default()
                        )
                    );
                    nodes += 1;
                }
            }
        }
        let model = value(&before.0);
        for entry in model["media"].as_array().unwrap() {
            assert!(!entry["uri"].as_str().unwrap().contains("://"));
            let part = &s.package().parts()[entry["partId"].as_u64().unwrap() as usize];
            assert_eq!(
                t.media(&id, entry["mediaId"].as_u64().unwrap() as u32).unwrap(),
                s.package().zip().clone().read(part.zip_index).unwrap()
            );
            media += 1;
        }
        assert_eq!((t.document(&id, None).unwrap(), t.diagnostics(&id).unwrap()), before);
        t.close(&id);
    }
    assert_eq!(opened, 1099);
    assert_eq!(refused, REFUSED.into_iter().map(String::from).collect());
    eprintln!("BIND-05/09: {opened} documents, {parts} parts, {media} media, {nodes} XML subtrees");
}

#[test]
fn bind_06_part_disambiguates_equal_node_ids() {
    let notes = r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>note</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
    let bytes = common::docx_with_parts(
        "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>main</w:t></w:r></w:p>",
        &[("word/footnotes.xml", notes)],
    );
    let s = EditSession::open(&bytes).unwrap();
    let main = s.dom();
    let part = s.document().footnotes.part.unwrap();
    let note = s.package().part(part).dom().unwrap();
    let run = |dom: &Dom| {
        dom.descendants(dom.root())
            .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::R)))
            .unwrap()
    };
    let a = run(main);
    let b = run(note);
    assert_eq!(a, b, "夹具必须真的有跨 part 同号");
    let mut table = SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    let ids = json!([a.0]).to_string();
    let main_value = value(&table.resolve_runs(&id, &ids, None).unwrap());
    let note_value = value(&table.resolve_runs(&id, &ids, Some(part.0)).unwrap());
    assert_eq!(main_value[0]["value"]["props"]["bold"], true);
    assert_eq!(note_value[0]["value"]["props"]["italic"], true);
    assert_ne!(main_value, note_value);
    assert!(table.node_xml(&id, a.0, None).unwrap().contains("main"));
    assert!(table.node_xml(&id, b.0, Some(part.0)).unwrap().contains("note"));
}
#[test]
fn bind_10_depth_has_schema_valid_protected_placeholders() {
    let bytes = common::docx_with_body(
        "<w:tbl><w:tblGrid><w:gridCol w:w='1000'/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>deep</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let mut table = SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    let full = value(&table.document(&id, None).unwrap());
    let limited = value(&table.document(&id, Some(r#"{"depth":0}"#)).unwrap());
    let child = &limited["main"][0]["rows"][0]["cells"][0]["blocks"][0];
    assert_eq!(child["kind"], "protected");
    assert_eq!(child["protectedKind"], json!({"kind":"tooDeep"}));
    assert_eq!(child["node"], full["main"][0]["rows"][0]["cells"][0]["blocks"][0]["node"]);
    assert_eq!(limited["truncated"], true);
    jsonschema::validator_for(&document_schema()).unwrap().validate(&limited).unwrap();
}
#[test]
fn bind_07_committed_escape_and_dirty_part_bytes() {
    let bytes = blank();
    let engine = EditSession::open(&bytes).unwrap();
    let part = engine.main_part();
    let para = engine.document().paragraphs().next().unwrap().node.0;
    let mut table = SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    let op=json!({"op":"replaceInlines","para":para,"inlines":[{"kind":"xml","value":"<w:r><w:t>committed</w:t></w:r>"}]}).to_string();
    table.apply(&id, &op, None).unwrap();
    assert_eq!(value(&table.diagnostics(&id).unwrap())["xmlEscapeCount"], 1);
    let current = table.part_bytes(&id, part.0).unwrap();
    assert!(String::from_utf8_lossy(&current).contains("committed"));
    assert_ne!(
        current,
        engine.package().zip().clone().read(engine.package().part(part).zip_index).unwrap()
    );
    let before = snapshot(&mut table, &id);
    assert!(table.apply(&id, "invalid json", None).is_err());
    assert_eq!(snapshot(&mut table, &id), before);
}

#[test]
fn bind_01_header_footer_out_of_arena_is_atomic_error() {
    let mut table = SessionTable::default();
    let id = table.open(&blank(), None).unwrap();
    let before = snapshot(&mut table, &id);
    let op=json!({"op":"setHeaderFooter","sect":u32::MAX,"kind":"header","variant":"default","content":[]}).to_string();
    assert_eq!(table.apply(&id, &op, None).unwrap_err().code, "EDIT_TARGET_MISSING");
    assert_eq!(snapshot(&mut table, &id), before);
}
