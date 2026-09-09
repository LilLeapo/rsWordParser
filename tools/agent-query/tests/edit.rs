//! AGENT-07/08/09：独立状态、审计还原、错误原子性与真实编辑。
#[path = "../../../crates/rsword/tests/common/mod.rs"]
mod common;
use rsword::{
    agent::text::{self, Scope},
    bind::native::{EditOpJson, SessionTable},
    model::Document,
    package::Package,
};
use rsword_agent_query::{
    audit::{self, Attachment, Audit},
    budget::Budget,
    edit::WorkerConfig,
    session::Sessions,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn worker() -> WorkerConfig {
    WorkerConfig {
        program: env!("CARGO_BIN_EXE_rsword-query-worker").into(),
        scratch: common::repo_root().join("target/m95-worker"),
        args: vec![],
    }
}
fn fixture() -> Vec<u8> {
    common::docx_with_body(
        "<w:p><w:r><w:t>甲方 AＢ 甲方</w:t></w:r></w:p><w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p>",
    )
}
fn objects(bytes: &[u8]) -> Vec<Value> {
    let mut p = Package::open(bytes).unwrap();
    let d = Document::rebuild(&mut p).unwrap();
    let t = text::project(&p, &d, Scope::Main, "test").unwrap();
    t.objects.values().filter(|o| o.object.kind == "paragraph").map(|o| json!(o.object)).collect()
}
fn selector(bytes: &[u8], pattern: &str, all: bool) -> Value {
    json!({"scope":objects(bytes),"find":pattern,"all":all})
}
fn request(selector: Value, text: &str) -> String {
    json!({"operations":[{"action":"replaceText","selector":selector,"text":text}]}).to_string()
}
fn full() -> Budget {
    Budget { limit: 1_048_576, max_bytes: 4_194_304 }
}
fn summary(s: &mut Sessions, id: &str, report: &str) -> Vec<Value> {
    let mut out = vec![];
    let mut cursor = None;
    loop {
        let page = s.summary(id, report, full(), cursor.as_deref()).unwrap();
        out.extend(page["content"].as_array().unwrap().clone());
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    out
}
#[test]
fn agent_07_zero_length_edit_cannot_cross_authorized_end() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let scope = vec![objects(&bytes)[0].clone()];
    let select = |pattern| json!({"scope":scope,"find":pattern,"search":{"mode":"regex"}});
    let end = request(select(r"\z"), "LEAK");
    let e = s.edit(&id, 0, &end, None, Some(&worker())).unwrap_err();
    assert_eq!(e.code, "AGENT_NOT_EDITABLE");
    assert!(e.details.get("owner").is_some());
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    s.edit(&id, 0, &request(select(r"\A"), "START"), None, Some(&worker())).unwrap();
    let xml =
        String::from_utf8(common::part_bytes(&s.save(&id, None).unwrap(), "word/document.xml"))
            .unwrap();
    assert!(xml.contains("START"));
    assert!(xml.contains("<w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p>"));
}
#[test]
fn agent_09_attachment_roundtrip_and_each_corruption_rejected() {
    let op: EditOpJson = serde_json::from_value(
        json!({"op":"replaceImageMedia","drawing":60,"bytes":[1,2,3],"mime":"image/png"}),
    )
    .unwrap();
    let audit = Audit::capture(std::slice::from_ref(&op));
    let key = audit.attachments[0].sha256.clone();
    let media = BTreeMap::from([(
        key.clone(),
        Attachment { bytes: vec![1, 2, 3], mime: "image/png".into() },
    )]);
    assert_eq!(audit::canonical(&audit.restore(&media).unwrap()), audit::canonical(&[op]));
    assert!(audit.operations[0]["bytes"].get("$attachment").is_some());
    assert_eq!(audit.restore(&BTreeMap::new()).unwrap_err().code, "AGENT_ATTACHMENT_MISSING");
    let mut wrong = media.clone();
    wrong.get_mut(&key).unwrap().bytes[1] = 9;
    let error = audit.restore(&wrong).unwrap_err();
    assert_eq!(error.code, "AGENT_ATTACHMENT_MISMATCH");
    assert_eq!(
        error.details,
        json!({"stage":"attachment","operation":0,"sha256Prefix":&key[..12]})
    );
    let mut wrong = media.clone();
    wrong.get_mut(&key).unwrap().mime = "image/jpeg".into();
    assert_eq!(audit.restore(&wrong).unwrap_err().code, "AGENT_ATTACHMENT_MISMATCH");
    for field in ["operation", "path", "sha256", "length", "mime"] {
        let mut wrong = json!(audit);
        wrong["attachments"][0][field] = match field {
            "operation" | "length" => json!(999),
            _ => json!("tampered"),
        };
        let wrong: Audit = serde_json::from_value(wrong).unwrap();
        assert!(wrong.restore(&media).is_err(), "{field}");
    }
    let mut wrong = audit.clone();
    wrong.attachments.clear();
    assert_eq!(wrong.restore(&media).unwrap_err().code, "AGENT_ATTACHMENT_MISMATCH");
    let mut wrong = audit.clone();
    wrong.operations[0]["drawing"] = json!(61);
    assert_eq!(wrong.restore(&media).unwrap_err().code, "AGENT_ATTACHMENT_MISMATCH");
}
#[test]
fn agent_07_ambiguity_all_order_and_untouched_bytes() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    for (pattern, code) in [("甲方", "AGENT_AMBIGUOUS"), ("不存在", "AGENT_TARGET_NOT_FOUND")]
    {
        assert_eq!(
            s.edit(&id, 0, &request(selector(&bytes, pattern, false), "乙"), None, Some(&worker()))
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(s.save(&id, None).unwrap(), bytes);
        assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
    }
    let receipt = s
        .edit(&id, 0, &request(selector(&bytes, "甲方", true), "乙"), None, Some(&worker()))
        .unwrap();
    let saved = s.save(&id, None).unwrap();
    let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
    assert!(!xml.contains("甲方"));
    assert_eq!(xml.matches("乙").count(), 2);
    assert!(xml.contains("<w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p>"));
    assert_eq!(receipt["counts"]["nativeOperations"], 4);
}
#[test]
fn agent_07_presentation_and_normalized_original_guard() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let err = s
        .edit(&id, 0, &request(selector(&bytes, "\n", true), ""), None, Some(&worker()))
        .unwrap_err();
    assert_eq!(err.code, "AGENT_NOT_EDITABLE");
    assert!(err.details["owner"]["node"].is_number());
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    let mut sel = selector(&bytes, "AB", false);
    sel["search"] = json!({"foldWidth":true});
    sel["original"] = json!("AB");
    assert_eq!(
        s.edit(&id, 0, &request(sel.clone(), "Z"), None, Some(&worker())).unwrap_err().code,
        "AGENT_PRECONDITION_FAILED"
    );
    sel["original"] = json!("AＢ");
    s.edit(&id, 0, &request(sel, "Z"), None, Some(&worker())).unwrap();
    assert!(
        String::from_utf8(common::part_bytes(&s.save(&id, None).unwrap(), "word/document.xml"))
            .unwrap()
            .contains('Z')
    );
}
#[test]
fn agent_08_preview_commit_and_audit_replay() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let input = request(selector(&bytes, "甲方", true), "乙方");
    let preview = s.preview(&id, 0, &input, full(), Some(&worker())).unwrap();
    let report = preview["range"]["reportId"].as_str().unwrap();
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
    let old = summary(&mut s, &id, report);
    let receipt = s.edit(&id, 0, &input, Some(report), Some(&worker())).unwrap();
    let rows = summary(&mut s, &id, receipt["reportId"].as_str().unwrap());
    assert_eq!(old, rows);
    let mut native = SessionTable::default();
    let nid = native.open(&bytes, None).unwrap();
    for row in rows.iter().filter(|r| r["kind"] == "editOp") {
        native.apply(&nid, &row["value"].to_string(), None).unwrap();
    }
    assert_eq!(native.save(&nid, None).unwrap(), s.save(&id, None).unwrap());
    assert_eq!(summary(&mut s, &id, report), old);
    let e = s.edit(&id, 1, &input, Some(report), Some(&worker())).unwrap_err();
    assert_eq!(e.code, "AGENT_PREVIEW_STALE");
}
#[test]
fn agent_07_late_failure_restores_created_nodes_and_reports() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let target = objects(&bytes)[0].clone();
    let input=json!({"operations":[{"action":"insertParagraphAfter","target":target,"text":"NEW","style":null},{"action":"deleteTableColumn","target":target,"column":1}]}).to_string();
    let e = s.edit(&id, 0, &input, None, None).unwrap_err();
    assert_eq!(e.details["operationIndex"], 1);
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
}
#[test]
fn agent_07_duplicate_keys_are_rejected_before_edit() {
    assert!(
        audit::parse(r#"{"operations":[],"operations":[]}"#)
            .unwrap_err()
            .message
            .contains("重复键")
    );
    assert!(audit::parse(r#"{"context":{"x":1,"x":2}}"#).is_err());
}
#[test]
fn agent_08_budget_failure_and_report_expiry() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let input = request(selector(&bytes, "甲方", true), "乙方");
    assert_eq!(
        s.preview(&id, 0, &input, Budget { limit: 1, max_bytes: 512 }, Some(&worker()))
            .unwrap_err()
            .code,
        "AGENT_BUDGET_TOO_SMALL"
    );
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    let first = s.edit(&id, 0, r#"{"operations":[]}"#, None, None).unwrap()["reportId"]
        .as_str()
        .unwrap()
        .to_string();
    for version in 1..33 {
        s.edit(&id, version, r#"{"operations":[]}"#, None, None).unwrap();
    }
    assert_eq!(s.summary(&id, &first, full(), None).unwrap_err().code, "AGENT_REPORT_EXPIRED");
    s.close(&id);
    assert_eq!(s.summary(&id, &first, full(), None).unwrap_err().code, "BIND_NO_SESSION");
}
#[test]
fn agent_07_compiled_all_uses_distinct_descending_positions() {
    let bytes = fixture();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = text::project(&pkg, &doc, Scope::All, "probe").unwrap();
    let mut native = SessionTable::default();
    let id = native.open(&bytes, None).unwrap();
    let w = worker();
    let input =
        rsword_agent_query::edit::Request::parse(&request(selector(&bytes, "甲方", true), "乙"))
            .unwrap();
    let mut cx = rsword_agent_query::edit::Compiler {
        native: &mut native,
        id: &id,
        projection: &p,
        worker: Some(&w),
    };
    let ops = input.operations[0].compile(&mut cx).unwrap();
    let values: Vec<_> = ops.iter().map(|op| serde_json::to_value(op).unwrap()).collect();
    assert_eq!(
        values
            .iter()
            .filter(|v| v["op"] == "deleteRange")
            .map(|v| (v["from"]["offset"].clone(), v["to"]["offset"].clone()))
            .collect::<Vec<_>>(),
        vec![(json!(6), json!(8)), (json!(0), json!(2))],
        "{values:?}"
    );
    for op in values {
        native.apply(&id, &op.to_string(), None).unwrap_or_else(|e| panic!("{op}: {e:?}"));
    }
}
fn real(name: &str) -> Vec<u8> {
    std::fs::read(common::repo_root().join("corpus/real").join(name)).unwrap()
}
fn projection(bytes: &[u8]) -> text::Projection {
    let mut p = Package::open(bytes).unwrap();
    let d = Document::rebuild(&mut p).unwrap();
    text::project(&p, &d, Scope::All, "fixture").unwrap()
}
fn real_target(bytes: &[u8], kind: &str, index: usize) -> Value {
    let p = projection(bytes);
    let mut objects: Vec<_> = p.objects.values().filter(|o| o.object.kind == kind).collect();
    objects.sort_by_key(|o| o.range.start);
    json!(objects[index].object)
}
#[test]
fn agent_07_w2_w5_w6_structured_operations() {
    let bytes = real("text/text-basic.docx");
    let p = projection(&bytes);
    let target = p
        .objects
        .values()
        .find(|o| {
            o.object.kind == "paragraph"
                && p.text_range(o.range.clone()).unwrap().contains("Mixed English")
        })
        .unwrap();
    let target = json!(target.object);
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let create = json!({"styleId":"AgentQuote","kind":"paragraph","name":"AgentQuote","basedOn":null,"runProps":null,"paraProps":{"indent":{"start":720}}});
    let input=json!({"operations":[{"action":"setBlockStyle","target":target,"styleId":"AgentQuote","createStyle":create}]}).to_string();
    let preview = s.preview(&id, 0, &input, full(), None).unwrap();
    assert!(preview["content"].as_array().unwrap().iter().any(|r| r["kind"] == "format"));
    s.edit(&id, 0, &input, None, None).unwrap();
    let saved = s.save(&id, None).unwrap();
    let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
    assert!(xml.contains("AgentQuote"));
    assert!(xml.contains("w:sz w:val=\"22\""));
    s.edit(&id, 1, &input, None, None).unwrap(); // 相同声明不再 upsert。
    let mut conflict: Value = serde_json::from_str(&input).unwrap();
    conflict["operations"][0]["createStyle"]["name"] = json!("Conflict");
    assert_eq!(
        s.edit(&id, 2, &conflict.to_string(), None, None).unwrap_err().code,
        "AGENT_STYLE_CONFLICT"
    );
    assert_eq!(s.save(&id, None).unwrap(), saved);
    let character_style =
        json!({"operations":[{"action":"setBlockStyle","target":target,"styleId":"a0"}]})
            .to_string();
    assert_eq!(
        s.edit(&id, 2, &character_style, None, None).unwrap_err().code,
        "AGENT_STYLE_CONFLICT"
    );
    assert_eq!(s.save(&id, None).unwrap(), saved);
    let comment=json!({"operations":[{"action":"addComment","selector":{"scope":[target],"find":"Mixed English"},"author":"Agent","text":"请核对英文表述","date":null}]}).to_string();
    s.edit(&id, 2, &comment, None, Some(&worker())).unwrap();
    let saved = s.save(&id, None).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let comment = doc.comments.items.iter().find(|c| c.author.as_deref() == Some("Agent")).unwrap();
    let range = doc
        .spans
        .live()
        .find(|r| r.class() == rsword::span::RangeClass::Comment && r.pair_id() == comment.id)
        .unwrap();
    let start = range.start.as_ref().unwrap().marker.unwrap();
    let end = range.end.as_ref().unwrap().marker.unwrap();
    // 物化后的 XML 标记之间读原始文字；Span.index 是内容项索引，不冒充 UTF-16。
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let mut inside = false;
    let mut selected = String::new();
    for node in dom.descendants(dom.root()) {
        if node == start {
            inside = true;
        } else if node == end {
            break;
        } else if inside
            && dom.parent(node).is_some_and(|parent| {
                dom.is(parent, rsword::xml::QName::w(rsword::xml::LocalName::T))
            })
            && let Some(t) = dom.text(node)
        {
            selected.push_str(&t);
        }
    }
    assert_eq!(selected, "Mixed English");
    assert!(
        String::from_utf8(common::part_bytes(&saved, "word/comments.xml"))
            .unwrap()
            .contains("请核对英文表述")
    );
    let insert=json!({"operations":[{"action":"insertParagraphAfter","target":target,"text":"摘要：本节介绍正文与列表。","style":null}]}).to_string();
    s.edit(&id, 3, &insert, None, None).unwrap();
    assert!(
        String::from_utf8(common::part_bytes(&s.save(&id, None).unwrap(), "word/document.xml"))
            .unwrap()
            .contains("摘要：本节介绍正文与列表。")
    );
}
#[test]
fn agent_07_w4_table_column_and_auxiliary_refusal() {
    let bytes = real("table/table-styled.docx");
    let target = real_target(&bytes, "table", 0);
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let mut wrong = target.clone();
    wrong["part"] = json!(99999);
    let action = |target| {
        json!({"operations":[{"action":"deleteTableColumn","target":target,"column":3}]})
            .to_string()
    };
    assert_eq!(
        s.edit(&id, 0, &action(wrong), None, None).unwrap_err().code,
        "AGENT_UNSUPPORTED_RANGE"
    );
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    s.edit(&id, 0, &action(target), None, None).unwrap();
    let saved = s.save(&id, None).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let d = Document::rebuild(&mut pkg).unwrap();
    let table = d
        .blocks()
        .find_map(|b| if let rsword::model::Block::Table(t) = b { Some(t) } else { None })
        .unwrap();
    assert_eq!(table.grid.len(), 2);
    assert_eq!(table.rows.len(), 3);
}
#[test]
fn agent_07_w9_shared_image_and_external_audit() {
    let bytes = real("image/image-two-in-run.docx");
    let target = real_target(&bytes, "image", 1);
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let media = common::part_bytes(&real("image/image-svg.docx"), "word/media/image1.png");
    let receipt = s.add_media(&id, 0, &media, "image/png").unwrap();
    let input=json!({"operations":[{"action":"replaceImage","target":target,"mediaId":receipt["mediaId"]}]}).to_string();
    let before = s.save(&id, None).unwrap();
    let preview = s.preview(&id, 1, &input, full(), None).unwrap();
    assert_eq!(s.save(&id, None).unwrap(), before);
    let receipt = s.edit(&id, 1, &input, preview["range"]["reportId"].as_str(), None).unwrap();
    let rows = summary(&mut s, &id, receipt["reportId"].as_str().unwrap());
    let row = rows.iter().find(|r| r["kind"] == "editOp").unwrap();
    assert!(row["value"]["bytes"].is_object());
    assert!(!row["value"]["bytes"].is_array());
    let saved = s.save(&id, None).unwrap();
    assert_eq!(
        common::part_bytes(&saved, "word/media/image1.png"),
        common::part_bytes(&bytes, "word/media/image1.png")
    );
    let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
    let original = String::from_utf8(common::part_bytes(&bytes, "word/document.xml")).unwrap();
    assert_ne!(xml, original);
    let first = real_target(&bytes, "image", 0);
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let node = rsword::xml::NodeId(first["node"].as_u64().unwrap() as u32);
    let raw = dom.lex_str(&dom.node(node).lex.as_ref().unwrap().range);
    assert!(xml.contains(raw), "第一次出现的 drawing 原字节必须保留");
    let compressed = |bytes: &[u8]| {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let entry = zip.by_name("word/media/image1.png").unwrap();
        let start = entry.data_start().unwrap() as usize;
        (entry.crc32(), bytes[start..start + entry.compressed_size() as usize].to_vec())
    };
    assert_eq!(compressed(&saved), compressed(&bytes));
}
use common::fingerprint;
#[test]
fn agent_07_w11_tracking_reject_view_equals_before() {
    let bytes = real("text/text-basic.docx");
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let mut input: Value = serde_json::from_str(&request(
        selector(&bytes, "before 前文", false),
        "Agent 改写后的首段",
    ))
    .unwrap();
    input["context"] = json!({"trackChanges":{"author":"Agent","date":null}});
    s.edit(&id, 0, &input.to_string(), None, Some(&worker())).unwrap();
    let before = rsword::EditSession::open(&bytes).unwrap();
    let after = rsword::EditSession::open(&s.save(&id, None).unwrap()).unwrap();
    assert_eq!(fingerprint::fingerprint(&before).reject, fingerprint::fingerprint(&after).reject);
    assert!(
        fingerprint::fingerprint(&after).accept.contains("Agent 改写后的首段"),
        "{}",
        fingerprint::fingerprint(&after).accept
    );
    assert!(after.document().revisions.entries().iter().all(|r| r.author() == Some("Agent")));
    assert!(!after.document().revisions.entries().is_empty());
}
#[test]
fn agent_07_w3_author_lock_preserves_other_author() {
    let bytes = real("revisions2/rev-insert-delete.docx");
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let input =
        json!({"operations":[{"action":"acceptRevisions","scope":objects(&bytes),"author":"作者甲"}]})
            .to_string();
    s.edit(&id, 0, &input, None, None).unwrap();
    let after = rsword::EditSession::open(&s.save(&id, None).unwrap()).unwrap();
    assert_eq!(
        after
            .document()
            .revisions
            .entries()
            .iter()
            .filter(|r| r.author() == Some("作者甲"))
            .count(),
        0
    );
    assert_eq!(
        after
            .document()
            .revisions
            .entries()
            .iter()
            .filter(|r| r.author() == Some("作者乙"))
            .count(),
        1
    );
    let mut oracle = rsword::EditSession::open(&bytes).unwrap();
    oracle
        .apply(rsword::EditOp::AcceptAll { author: Some("作者甲".into()) }, &Default::default())
        .unwrap();
    assert_eq!(fingerprint::fingerprint(&after), fingerprint::fingerprint(&oracle));
}
#[test]
fn agent_07_w8_header_and_failed_batch_full_rollback() {
    let bytes = real("text/text-basic.docx");
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let section = doc.sections[0].node.unwrap();
    let target = json!({"part":doc.main_part.0,"flow":doc.flow_of_in(doc.main_part,doc.body.unwrap()).unwrap().0,"node":section.0,"kind":"section"});
    let action = json!({"action":"setHeaderFooter","target":target,"kind":"header","variant":"default","paragraphs":["Agent 审阅稿"]});
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let bad=json!({"operations":[action,{"action":"deleteTableColumn","target":objects(&bytes)[0],"column":1}]}).to_string();
    assert_eq!(s.edit(&id, 0, &bad, None, None).unwrap_err().details["operationIndex"], 1);
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    let result = s.edit(&id, 0, &json!({"operations":[action]}).to_string(), None, None).unwrap();
    let mut independent = Sessions::default();
    let other = independent.open(&bytes).unwrap();
    independent.edit(&other, 0, &json!({"operations":[action]}).to_string(), None, None).unwrap();
    assert_eq!(s.save(&id, None).unwrap(), independent.save(&other, None).unwrap());
    let rows = summary(&mut s, &id, result["reportId"].as_str().unwrap());
    assert!(
        rows.iter().any(|r| r["kind"] == "part" && r["uri"].as_str().unwrap().contains("header"))
    );
}
#[test]
fn agent_07_w10_seven_original_blocks_move_intact() {
    let bytes = real("misc/large-report.docx");
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = projection(&bytes);
    let object = |node| {
        p.objects
            .values()
            .find(|o| o.object.part == doc.main_part.0 && o.object.node == node)
            .unwrap()
            .object
            .clone()
    };
    let targets: Vec<_> = doc.main[41..48].iter().map(|b| object(b.node().0)).collect();
    let destination = object(doc.main[0].node().0);
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let originals: Vec<_> = doc.main[41..48]
        .iter()
        .map(|b| dom.lex_str(&dom.node(b.node()).lex.as_ref().unwrap().range).to_owned())
        .collect();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let action =
        json!({"operations":[{"action":"moveBlocks","targets":targets,"before":destination}]})
            .to_string();
    let preview = s.preview(&id, 0, &action, full(), None).unwrap();
    assert!(preview["content"].as_array().unwrap().iter().any(|r| r["kind"] == "move"));
    s.edit(&id, 0, &action, None, None).unwrap();
    let saved = s.save(&id, None).unwrap();
    let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
    let positions: Vec<_> =
        originals.iter().map(|original| xml.find(original).expect("原块字节原样")).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]));
    let mut pkg = Package::open(&saved).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let dom = pkg.part(doc.main_part).dom().unwrap();
    for (i, original) in originals.iter().enumerate() {
        assert_eq!(
            dom.lex_str(&dom.node(doc.main[i].node()).lex.as_ref().unwrap().range),
            original
        );
    }
}
#[test]
fn agent_09_fingerprint_distinguishes_source_text() {
    let a = rsword::EditSession::open(&common::docx_with_body(
        "<w:p><w:r><w:t>FIRST</w:t></w:r></w:p>",
    ))
    .unwrap();
    let b = rsword::EditSession::open(&common::docx_with_body(
        "<w:p><w:r><w:t>OTHER</w:t></w:r></w:p>",
    ))
    .unwrap();
    assert_ne!(
        fingerprint::fingerprint(&a),
        fingerprint::fingerprint(&b),
        "不同正文不能得到相同指纹"
    );
}
#[test]
fn agent_09_report_cursor_survives_edit_and_preview_cache_eviction() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let input = request(selector(&bytes, "甲方", true), "乙方");
    let r = s.edit(&id, 0, &input, None, Some(&worker())).unwrap();
    let report = r["reportId"].as_str().unwrap();
    let expected = summary(&mut s, &id, report);
    let b = Budget { limit: 4000, max_bytes: 2000 };
    let page = s.summary(&id, report, b, None).unwrap();
    let mut rows = page["content"].as_array().unwrap().clone();
    let mut cursor = page["nextCursor"].as_str().map(str::to_owned);
    assert!(cursor.is_some());
    s.edit(&id, 1, r#"{"operations":[]}"#, None, None).unwrap();
    while let Some(token) = cursor {
        let p = s.summary(&id, report, b, Some(&token)).unwrap();
        rows.extend(p["content"].as_array().unwrap().clone());
        cursor = p["nextCursor"].as_str().map(str::to_owned);
    }
    assert_eq!(rows, expected);
    for _ in 0..33 {
        s.preview(&id, 2, r#"{"operations":[]}"#, full(), None).unwrap();
    }
    assert_eq!(summary(&mut s, &id, report), expected);
}
#[test]
fn agent_09_oversized_report_rolls_back_after_candidate_edit() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let target = objects(&bytes)[0].clone();
    let input=json!({"operations":[{"action":"insertParagraphAfter","target":target,"text":"X".repeat(3_000_000),"style":null}]}).to_string();
    assert!(input.len() < 16 * 1024 * 1024);
    let err = s.edit(&id, 0, &input, None, None).unwrap_err();
    assert_eq!(err.code, "AGENT_REPORT_TOO_LARGE");
    assert!(err.message.contains("完整审计报告"), "不能只在输入长度处拒绝");
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
}
#[test]
fn agent_07_media_mime_is_verified_before_version_advance() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let png = common::part_bytes(&real("image/image-svg.docx"), "word/media/image1.png");
    assert_eq!(
        s.add_media(&id, 0, &png, "image/jpeg").unwrap_err().code,
        "AGENT_ATTACHMENT_MISMATCH"
    );
    assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    let upload = s.add_media(&id, 0, &png, "image/png").unwrap();
    let uploaded = s.save(&id, None).unwrap();
    let invalid = request(selector(&bytes, "missing", false), "x");
    assert!(s.edit(&id, 1, &invalid, None, Some(&worker())).is_err());
    assert_eq!(s.save(&id, None).unwrap(), uploaded);
    assert_eq!(s.media(&id, upload["mediaId"].as_u64().unwrap() as u32).unwrap(), png);
}
#[test]
fn agent_07_w1_body_scope_excludes_matching_headers() {
    let bytes = real("hf/hf-variants.docx");
    let p = projection(&bytes);
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    assert!(doc.paragraphs().any(|p| p.text().contains('页')));
    let hf: Vec<_> = doc
        .hf_parts
        .keys()
        .map(|part| {
            (
                pkg.part(*part).uri.to_string(),
                common::part_bytes(&bytes, pkg.part(*part).uri.as_str()),
            )
        })
        .collect();
    assert!(hf.iter().any(|(_, b)| String::from_utf8_lossy(b).contains('页')));
    let body_scope: Vec<_> = p
        .objects
        .values()
        .filter(|o| o.object.kind == "paragraph" && o.object.part == doc.main_part.0)
        .map(|o| json!(o.object))
        .collect();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    s.edit(
        &id,
        0,
        &request(json!({"scope":body_scope,"find":"页","all":true}), "版"),
        None,
        Some(&worker()),
    )
    .unwrap();
    let saved = s.save(&id, None).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let d = Document::rebuild(&mut pkg).unwrap();
    assert!(d.paragraphs().all(|p| !p.text().contains('页')));
    for (name, original) in hf {
        assert_eq!(common::part_bytes(&saved, &name), original);
    }
}
#[test]
fn agent_07_w2_level_two_heading_and_original_block_bytes() {
    let bytes = real("text/text-basic.docx");
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = projection(&bytes);
    let headings: Vec<_> = doc
        .paragraphs()
        .filter(|p| matches!(p.kind, rsword::model::block::TextKind::Heading { level: 2 }))
        .collect();
    assert_eq!(headings.len(), 1);
    let target = p
        .objects
        .values()
        .find(|o| o.object.node == headings[0].node.0 && o.object.part == doc.main_part.0)
        .unwrap();
    let dom = pkg.part(doc.main_part).dom().unwrap();
    let originals: Vec<_> = doc
        .main
        .iter()
        .map(|b| dom.lex_str(&dom.node(b.node()).lex.as_ref().unwrap().range).to_owned())
        .collect();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    s.edit(&id,0,&json!({"operations":[{"action":"insertParagraphAfter","target":target.object,"text":"摘要：本节介绍正文与列表。","style":null}]}).to_string(),None,None).unwrap();
    let saved = s.save(&id, None).unwrap();
    let xml = String::from_utf8(common::part_bytes(&saved, "word/document.xml")).unwrap();
    for original in originals {
        assert!(xml.contains(&original));
    }
    let mut pkg = Package::open(&saved).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let paras: Vec<_> = doc.paragraphs().collect();
    let i = paras
        .iter()
        .position(|p| matches!(p.kind, rsword::model::block::TextKind::Heading { level: 2 }))
        .unwrap();
    assert_eq!(paras[i + 1].text(), "摘要：本节介绍正文与列表。");
}
#[test]
fn agent_07_action_table_matches_written_contract_and_fixed_tasks() {
    let contract = include_str!("../../../docs/17-agent-edit.md")
        .split("```agent-actions\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
        .trim();
    let actual = rsword_agent_query::edit::Action::COVERAGE
        .iter()
        .map(|(name, tasks, supported)| {
            format!("{name} {tasks} {}", if *supported { "supported" } else { "pending" })
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, contract);
    let tasks: std::collections::BTreeSet<_> = rsword_agent_query::edit::Action::COVERAGE
        .iter()
        .flat_map(|(_, tasks, _)| tasks.split('/'))
        .collect();
    assert_eq!(
        tasks,
        (1..=11)
            .map(|i| format!("W{i}"))
            .collect::<std::collections::BTreeSet<_>>()
            .iter()
            .map(String::as_str)
            .collect()
    );
    assert_eq!(
        rsword_agent_query::edit::Action::COVERAGE
            .iter()
            .filter(|(_, _, s)| !*s)
            .map(|(n, _, _)| *n)
            .collect::<Vec<_>>(),
        ["updateToc"]
    );
}
