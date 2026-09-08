//! `BIND-03` 线型验收；按 v3 对无损与具名拒绝分别计数。
use rsword::bind::native::{SchemaDefs, ToJson};
use rsword::semantic::props::{Change, RunProps, RunPropsPatch, TableChange};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Serialize, Deserialize)]
struct Nested {
    #[serde(default, skip_serializing_if = "TableChange::wire_is_keep")]
    run: TableChange<RunProps, RunPropsPatch>,
}

fn nested_roundtrip(
    value: TableChange<RunProps, RunPropsPatch>,
    expected: Value,
) -> TableChange<RunProps, RunPropsPatch> {
    let encoded = serde_json::to_value(Nested { run: value }).unwrap();
    assert_eq!(encoded, expected);
    serde_json::from_value::<Nested>(encoded).unwrap().run
}

#[test]
fn bind_03_table_change_keep_is_absent() {
    assert!(matches!(nested_roundtrip(TableChange::Keep, json!({})), TableChange::Keep));
}
#[test]
fn bind_03_table_change_unset_is_null() {
    assert!(matches!(
        nested_roundtrip(TableChange::Unset, json!({"run": null})),
        TableChange::Unset
    ));
}
#[test]
fn bind_03_table_change_set_default_keeps_branch() {
    let value = RunProps::default();
    let expected = json!({"run": serde_json::to_value(&value).unwrap()});
    assert!(matches!(nested_roundtrip(TableChange::Set(value), expected), TableChange::Set(_)));
}
#[test]
fn bind_03_table_change_empty_patch_keeps_branch() {
    assert!(matches!(
        nested_roundtrip(
            TableChange::Patch(RunPropsPatch::default()),
            json!({"run": {"$patch": {}}})
        ),
        TableChange::Patch(_)
    ));
}
fn check_change(source: Value, expected: &str) {
    let patch: RunPropsPatch = serde_json::from_value(source.clone()).unwrap();
    let arm = match patch.bold {
        Change::Keep => "keep",
        Change::Unset => "unset",
        Change::Set(false) => "set",
        _ => panic!("wrong value"),
    };
    assert_eq!(arm, expected);
    assert_eq!(serde_json::to_value(patch).unwrap(), source);
}
macro_rules! change_case {
    ($($name:ident: $value:expr => $arm:literal;)+) => {
        $(#[test] fn $name() { check_change($value, $arm); })+
    };
}
change_case! {
    bind_03_change_keep_is_absent: json!({}) => "keep";
    bind_03_change_unset_is_null: json!({"bold": null}) => "unset";
    bind_03_change_set_false_is_value: json!({"bold": false}) => "set";
}
#[test]
fn bind_03_table_change_schema_discriminates_patch() {
    let mut defs = SchemaDefs::default();
    let mut schema = <TableChange<RunProps, RunPropsPatch> as ToJson>::schema(&mut defs);
    schema["$defs"] = Value::Object(defs.into_map());
    let validator = jsonschema::validator_for(&schema).unwrap();
    for value in [
        Value::Null,
        serde_json::to_value(RunProps::default()).unwrap(),
        json!({"$patch": {}}),
        json!({"$patch": {"bold": false}}),
    ] {
        assert!(validator.is_valid(&value), "{value}");
    }
    for value in [json!({"$patch": {}, "bold": true}), json!({"$patch": 1})] {
        assert!(!validator.is_valid(&value), "{value}");
        assert!(serde_json::from_value::<TableChange<RunProps, RunPropsPatch>>(value).is_err());
    }
}

// 从 BIND-03 v3 权威清单独立抄录，不从 edit_op_json! 推导。
const BIND_03_OPS: &[&str] = &[
    "setSources",
    "addNumberingDefinition",
    "restartNumbering",
    "setThemeFonts",
    "setThemeColors",
    "upsertStyle",
    "insertText",
    "deleteRange",
    "setRunProps",
    "insertAtom",
    "insertField",
    "replaceInlines",
    "splitParagraph",
    "mergeWithNext",
    "setParaProps",
    "replaceParaProps",
    "insertBlock",
    "deleteBlock",
    "moveBlock",
    "setTableProps",
    "setRowProps",
    "setCellProps",
    "insertRow",
    "deleteRow",
    "insertColumn",
    "deleteColumn",
    "mergeCells",
    "setFieldResultProps",
    "toggleCheckbox",
    "setFormText",
    "setLinkTarget",
    "updateBlockField",
    "regenerateBlockField",
    "addBookmark",
    "removeBookmark",
    "addComment",
    "removeComment",
    "setCommentText",
    "acceptRevision",
    "rejectRevision",
    "acceptAll",
    "rejectAll",
    "setSectionProps",
    "setHeaderFooter",
    "linkHeaderFooter",
    "setWatermark",
    "setPageColor",
    "insertSectionBreak",
    "deleteSectionBreak",
    "setDocumentSettings",
    "setNoteContent",
    "removeNote",
    "setSdtContent",
    "removeSdtShell",
    "setChartData",
    "replacePartXml",
    "replacePartBytes",
    "replaceImageMedia",
    "setDrawingGeometry",
    "setDrawingZOrder",
    "setDrawingWrap",
    "setShapeStyle",
    "setTextboxContent",
    "setMathTokens",
    "removeInks",
    "insertInk",
];

#[test]
fn bind_03_variants_match_independent_spec_list() {
    use std::collections::BTreeSet;
    let actual: BTreeSet<_> = rsword::bind::native::EditOpJson::VARIANTS
        .iter()
        .map(|s| format!("{}{}", s[..1].to_ascii_lowercase(), &s[1..]))
        .collect();
    let expected: BTreeSet<_> = BIND_03_OPS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(BIND_03_OPS.len(), 66);
    assert_eq!(actual, expected);
}

mod common;
use rsword::bind::native::{
    ProjCx, apply_edit_json, edit_op_from_json, edit_op_to_json, xml_escape_count,
};
use rsword::edit::{BlockPos, EditContext, EditOp, EditSession, NewBlock, NewInline, NewRun};
use rsword::semantic::props::{ParaProps, emit_para_props, emit_run_props};
use rsword::xml::{Dom, LocalName, NodeId, QName};

fn first(dom: &Dom, name: LocalName) -> NodeId {
    let mut stack = vec![dom.root()];
    while let Some(id) = stack.pop() {
        if dom.is(id, QName::w(name)) {
            return id;
        }
        stack.extend(dom.children(id).iter().rev().copied());
    }
    panic!("missing {name:?}");
}

#[test]
fn bind_03_protocol_native_bytes_full_synthetic_real() {
    let paths: Vec<_> =
        common::docx_paths("synthetic").into_iter().chain(common::docx_paths("real")).collect();
    assert_eq!(paths.len(), 1065);
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        let mut native = EditSession::open(&bytes).unwrap();
        let mut protocol = EditSession::open(&bytes).unwrap();
        let options: rsword::save::SaveOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(protocol.save_with(&options).unwrap(), bytes, "{}: empty save", path.display());
        let body = first(native.dom(), LocalName::Body);
        let op = EditOp::InsertBlock {
            at: BlockPos::end(body),
            block: NewBlock::Paragraph {
                props: Some(emit_para_props(
                    &ParaProps { keep_next: Some(true), ..Default::default() },
                    native.dom().flavor(),
                )),
                inlines: vec![NewInline::Run(NewRun {
                    text: "BIND-03 🙂".into(),
                    props: Some(emit_run_props(
                        &RunProps { bold: Some(true), ..Default::default() },
                        native.dom().flavor(),
                    )),
                })],
            },
        };
        let json = edit_op_to_json(&op, native.dom()).unwrap();
        native
            .apply(op, &EditContext::default())
            .unwrap_or_else(|e| panic!("{}: native {e}", path.display()));
        apply_edit_json(&mut protocol, &json, &EditContext::default())
            .unwrap_or_else(|e| panic!("{}: protocol {e}", path.display()));
        assert_eq!(xml_escape_count(&protocol), 0);
        assert_eq!(
            protocol.save().unwrap(),
            native.save().unwrap(),
            "{}: protocol/native bytes",
            path.display()
        );
    }
    eprintln!(
        "BIND-03: {} synthetic + real documents: protocol/native saved bytes equal, empty save identical",
        paths.len()
    );
}

#[test]
fn bind_03_escape_commit_and_failed_apply_are_atomic() {
    let bytes = common::docx_with_body("<w:p><w:r><w:t>original</w:t></w:r></w:p>");
    let mut session = EditSession::open(&bytes).unwrap();
    let para = first(session.dom(), LocalName::P);
    let op = json!({"op":"replaceInlines", "para":para.0, "inlines":[{"kind":"xml", "value":"<w:r><w:t>escape</w:t></w:r>"}]}).to_string();
    let result = apply_edit_json(&mut session, &op, &EditContext::default()).unwrap();
    assert_eq!(xml_escape_count(&session), 1);
    assert_eq!(
        result.diagnostics.iter().filter(|d| d.code.as_str() == "BIND_XML_ESCAPE").count(),
        1
    );
    let before_bytes = session.save().unwrap();
    let before_state =
        format!("{:?}{:?}{:?}", session.package(), session.document(), session.diagnostics());
    let invalid = json!({"op":"replaceInlines", "para":session.dom().root().0, "inlines":[{"kind":"xml", "value":"<zz:neverSeen xmlns:zz=\"urn:never-seen\"/>"}]}).to_string();
    assert!(apply_edit_json(&mut session, &invalid, &EditContext::default()).is_err());
    assert_eq!(
        format!("{:?}{:?}{:?}", session.package(), session.document(), session.diagnostics()),
        before_state
    );
    assert_eq!(session.save().unwrap(), before_bytes);
    assert_eq!(xml_escape_count(&session), 1);
}

#[test]
fn bind_03_context_defaults_and_result_shape() {
    assert_eq!(serde_json::from_str::<EditContext>("{}").unwrap(), EditContext::default());
    let source = json!({"trackChanges":{"author":"audit","date":"2026-09-09T00:00:00Z"},"defaultRunProps":null,"keepOrphanComments":true,"markUpdatedFieldsDirty":true});
    let context: EditContext = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(serde_json::to_value(context).unwrap(), source);
    let bytes = common::docx_with_body("<w:p/>");
    let session = EditSession::open(&bytes).unwrap();
    let result = rsword::edit::MutationResult {
        created: vec![None, Some(NodeId(2))],
        affected_blocks: vec![NodeId(3)],
        structure_changed: true,
        diagnostics: vec![],
        offset_delta: vec![(NodeId(3), rsword::edit::Utf16Offset(4), -2)],
    };
    let value = result.to_json(&ProjCx { pkg: session.package(), display: false });
    assert_eq!(
        value,
        json!({"created":[null,2],"affectedBlocks":[3],"structureChanged":true,"diagnostics":[],"offsetDelta":[[3,4,-2]]})
    );
    let mut defs = SchemaDefs::default();
    let mut schema = rsword::edit::MutationResult::schema(&mut defs);
    schema["$defs"] = Value::Object(defs.into_map());
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
}

#[test]
fn bind_04_native_save_options_exactly_five_keys() {
    let value = serde_json::to_value(rsword::save::SaveOptions::default()).unwrap();
    let expected =
        ["normalizeZOrder", "pruneOrphans", "removeDateAndTime", "removePersonalInfo", "savedAt"];
    assert_eq!(value.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), expected);
    for key in ["sources", "numbering", "inks", "themeFonts", "settings"] {
        assert!(
            serde_json::from_value::<rsword::save::SaveOptions>(json!({key: []})).is_err(),
            "{key}"
        );
    }
}

#[test]
fn bind_03_xml_input_rejects_unrepresentable_content_without_interning() {
    let bytes = common::docx_with_body("<w:p/>");
    let session = EditSession::open(&bytes).unwrap();
    let mut dom = session.dom().clone();
    for xml in [
        "extra<w:p/>",
        "<w:p/><!--comment-->",
        "<w:p><!--comment--></w:p>",
        "<w:p><?pi value?></w:p>",
        "<w:p/><w:p/>",
    ] {
        let before = format!("{dom:?}");
        let wire = json!({"op":"replaceParaProps", "para":2, "props":xml}).to_string();
        assert!(edit_op_from_json(&wire, &mut dom).is_err(), "{xml}");
        assert_eq!(format!("{dom:?}"), before);
    }
}

fn part_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = vec![];
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn twice_is_once(op: EditOp) {
    let bytes = common::docx_with_body("<w:p/>");
    let mut native = EditSession::open(&bytes).unwrap();
    let mut protocol = EditSession::open(&bytes).unwrap();
    let json = edit_op_to_json(&op, native.dom()).unwrap();
    native.apply(op, &EditContext::default()).unwrap();
    apply_edit_json(&mut protocol, &json, &EditContext::default()).unwrap();
    let before = format!("{:?}", protocol.package());
    let result = apply_edit_json(&mut protocol, &json, &EditContext::default()).unwrap();
    assert!(result.created.is_empty(), "repeat creates nodes: {json}");
    assert_eq!(format!("{:?}", protocol.package()), before, "repeat mutates DOM: {json}");
    assert_eq!(protocol.save().unwrap(), native.save().unwrap(), "twice != once: {json}");
}

#[test]
fn bind_03_add_numbering_definition_twice_is_once() {
    twice_is_once(EditOp::AddNumberingDefinition {
        definition: rsword::save::options::decl::NumberingDefSave {
            num_id: "42".into(),
            bullet: false,
            levels: vec![],
        },
    });
}
#[test]
fn bind_03_restart_numbering_twice_is_once() {
    twice_is_once(EditOp::RestartNumbering {
        restart: rsword::save::options::decl::RestartNumSave {
            num_id: "43".into(),
            abstract_num_id: "0".into(),
            start_overrides: vec![(0, 3)],
        },
    });
}
#[test]
fn bind_03_upsert_style_twice_is_once() {
    twice_is_once(EditOp::UpsertStyle {
        style: rsword::save::options::decl::StyleUpsertSave {
            style_id: "Audit".into(),
            kind: "paragraph".into(),
            name: "Audit".into(),
            based_on: None,
            run_props: Some(RunProps { bold: Some(true), ..Default::default() }),
            para_props: None,
        },
    });
}
#[test]
fn bind_03_theme_operations_twice_is_once() {
    use rsword::save::options::decl::*;
    twice_is_once(EditOp::SetThemeFonts {
        fonts: ThemeFontsSave {
            major: "Arial".into(),
            minor: "Calibri".into(),
            east_asia: Some("SimSun".into()),
        },
    });
    twice_is_once(EditOp::SetThemeColors {
        colors: ThemeColorsSave {
            name: Some("Audit".into()),
            slots: vec![("accent1".into(), "123456".into())],
        },
    });
}
#[test]
fn bind_03_sources_preserve_unchanged_entry_bytes() {
    use rsword::save::options::decl::SourceSave;
    let bytes =
        std::fs::read(common::corpus_dir("synthetic").join("watermark-theme-sources__008.docx"))
            .unwrap();
    let mut native = EditSession::open(&bytes).unwrap();
    let source = native.document().sources.iter().find(|s| s.tag == "Zhao2022").unwrap();
    let part = native.package().find_name("customXml/item1.xml").unwrap();
    let dom = native.package().part(part).dom().unwrap();
    let original = dom.lex_bytes(&dom.node(source.node).lex.as_ref().unwrap().range).to_vec();
    let op = EditOp::SetSources {
        sources: vec![
            SourceSave {
                tag: "Zhao2022".into(),
                kind: "JournalArticle".into(),
                author: "赵, 一".into(),
                title: "大模型对齐".into(),
                year: "2022".into(),
                publisher: Some("软件学报".into()),
                url: None,
            },
            SourceSave {
                tag: "Audit2026".into(),
                kind: "Book".into(),
                title: "new".into(),
                ..Default::default()
            },
        ],
    };
    let json = edit_op_to_json(&op, native.dom()).unwrap();
    let mut protocol = EditSession::open(&bytes).unwrap();
    native.apply(op, &EditContext::default()).unwrap();
    apply_edit_json(&mut protocol, &json, &EditContext::default()).unwrap();
    let after = protocol.save().unwrap();
    assert_eq!(after, native.save().unwrap());
    let xml = part_bytes(&after, "customXml/item1.xml");
    assert!(!original.is_empty());
    assert!(xml.windows(original.len()).any(|w| w == original), "unchanged Source raw bytes lost");
}

#[test]
fn bind_04_privacy_flags_require_explicit_native_request() {
    let bytes =
        std::fs::read(common::corpus_dir("synthetic").join("write-protection__005.docx")).unwrap();
    let mut session = EditSession::open(&bytes).unwrap();
    assert!(session.remove_personal_info_flag());
    assert_eq!(session.save().unwrap(), bytes);
    let opts: rsword::save::SaveOptions =
        serde_json::from_value(json!({"removePersonalInfo":false})).unwrap();
    assert_eq!(session.save_with(&opts).unwrap(), bytes);
    let opts: rsword::save::SaveOptions =
        serde_json::from_value(json!({"removePersonalInfo":true})).unwrap();
    let saved = session.save_with(&opts).unwrap();
    let xml = String::from_utf8(part_bytes(&saved, "word/document.xml")).unwrap();
    assert!(!xml.contains("张三"));
    assert!(xml.contains("Author"));
}

#[test]
fn bind_03_header_escape_uses_target_dom_and_reports_count() {
    let bytes = common::docx_with_body("<w:p/><w:sectPr/>");
    let mut session = EditSession::open(&bytes).unwrap();
    let sect = first(session.dom(), LocalName::SectPr);
    let request = json!({"op":"setHeaderFooter","sect":sect.0,"kind":"header","variant":"default","content":[{"kind":"xml","value":"<w:p xmlns:zz=\"urn:bind:header\"><zz:custom zz:attr=\"value\"/></w:p>"}]}).to_string();
    apply_edit_json(&mut session, &request, &EditContext::default()).unwrap();
    let diagnostics = rsword::bind::native::edit::edit_diagnostics_json(&session);
    assert_eq!(diagnostics["xmlEscapeCount"], 1);
    assert_eq!(
        diagnostics["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "BIND_XML_ESCAPE")
            .count(),
        1
    );
    let saved = session.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    let header = reopened
        .package()
        .parts()
        .iter()
        .find(|p| p.uri.as_str().starts_with("word/header") && p.is_xml)
        .unwrap();
    let xml = String::from_utf8(part_bytes(&saved, header.uri.as_str())).unwrap();
    assert!(
        xml.contains("urn:bind:header")
            && xml.contains("zz:custom")
            && xml.contains("zz:attr=\"value\""),
        "{xml}"
    );
}
