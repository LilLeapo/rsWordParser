//! 同一操作表展开的逐操作审计验收，拒绝分母独立点名。
#[path = "../../../crates/rsword/tests/common/mod.rs"]
mod common;
use super::*;
use crate::audit::{Attachment, Audit, canonical};
use rsword::agent::anchors::Affinity;
use std::collections::BTreeMap;
pub fn check(name: &str) {
    let bytes = if name == "replaceImage" {
        std::fs::read(common::repo_root().join("corpus/real/image/image-two-in-run.docx")).unwrap()
    } else if name == "acceptRevisions" {
        std::fs::read(common::repo_root().join("corpus/real/revisions2/rev-insert-delete.docx"))
            .unwrap()
    } else {
        common::docx_with_body(
            "<w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:r><w:t>World</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl><w:sectPr/>",
        )
    };
    let mut native = SessionTable::default();
    let id = native.open(&bytes, None).unwrap();
    let p = native
        .inspect(&id, |s, _| {
            rsword::agent::text::project(
                s.package(),
                s.document(),
                rsword::agent::text::Scope::All,
                "probe",
            )
            .unwrap()
        })
        .unwrap();
    let mut paragraphs: Vec<_> =
        p.objects.values().filter(|o| o.object.kind == "paragraph").collect();
    paragraphs.sort_by_key(|o| o.range.start);
    let target = &paragraphs[0].object;
    let segment =
        p.anchors.segments.iter().find(|s| matches!(s.target, Target::Source { .. })).unwrap();
    let start = p.anchors.to_anchor(segment.range.start, None).unwrap();
    let end = p.anchors.to_anchor(segment.range.start + 1, Some(Affinity::Left)).unwrap();
    let selector = json!({"scope":[target],"start":start,"end":end,"original":p.text_range(segment.range.start..segment.range.start+1).unwrap()});
    let action = match name {
        "replaceText" => json!({"action":name,"selector":selector,"text":"replacement"}),
        "insertParagraphAfter" => json!({"action":name,"target":target,"text":"new","style":null}),
        "setBlockStyle" => {
            json!({"action":name,"target":target,"styleId":"Agent","createStyle":{"styleId":"Agent","kind":"paragraph","name":"Agent","basedOn":null,"runProps":null,"paraProps":null}})
        }
        "deleteBlock" => json!({"action":name,"target":target}),
        "moveBlocks" => json!({"action":name,"targets":[target],"before":paragraphs[1].object}),
        "deleteTableColumn" => {
            json!({"action":name,"target":p.objects.values().find(|o|o.object.kind=="table").unwrap().object,"column":1})
        }
        "addComment" => {
            json!({"action":name,"selector":selector,"author":"Agent","text":"comment","date":null})
        }
        "acceptRevisions" => {
            json!({"action":name,"scope":paragraphs.iter().map(|p|&p.object).collect::<Vec<_>>(),"author":"作者甲"})
        }
        "updateToc" => json!({"action":name,"target":target}),
        "setHeaderFooter" => {
            let node =
                native.inspect(&id, |s, _| s.document().sections[0].node.unwrap().0).unwrap();
            let mut section = target.clone();
            section.node = node;
            section.kind = "section".into();
            json!({"action":name,"target":section,"kind":"header","variant":"default","paragraphs":["header"]})
        }
        "replaceImage" => {
            let model: Value = serde_json::from_str(&native.document(&id, None).unwrap()).unwrap();
            json!({"action":name,"target":p.objects.values().find(|o|o.object.kind=="image").unwrap().object,"mediaId":model["media"][0]["mediaId"]})
        }
        _ => panic!("缺少操作样例 {name}"),
    };
    let validator = jsonschema::validator_for(&Action::schema()).unwrap();
    assert!(validator.is_valid(&action), "schema {action}");
    let mut extra = action.clone();
    extra["zzExtra"] = json!(true);
    assert!(!validator.is_valid(&extra));
    assert!(serde_json::from_value::<Action>(extra).is_err());
    let action: Action = serde_json::from_value(action).unwrap();
    let mut cx = Compiler { native: &mut native, id: &id, projection: &p, worker: None };
    let compiled = action.compile(&mut cx);
    if name == "updateToc" {
        assert_eq!(compiled.unwrap_err().code, "AGENT_UNSUPPORTED_RANGE");
        return;
    }
    let ops = compiled.unwrap();
    assert!(!ops.is_empty());
    let audit = Audit::capture(&ops);
    let mut media = BTreeMap::new();
    for op in &ops {
        let value = serde_json::to_value(op).unwrap();
        if value["op"] == "replaceImageMedia" {
            let bytes: Vec<u8> = serde_json::from_value(value["bytes"].clone()).unwrap();
            media.insert(
                crate::cursor::hash(&bytes),
                Attachment { bytes, mime: value["mime"].as_str().unwrap().into() },
            );
        }
    }
    assert_eq!(canonical(&audit.restore(&media).unwrap()), canonical(&ops));
    for op in ops {
        native.apply(&id, &serde_json::to_string(&op).unwrap(), None).unwrap();
    }
    let diagnostics: Value = serde_json::from_str(&native.diagnostics(&id).unwrap()).unwrap();
    assert_eq!(diagnostics["xmlEscapeCount"], 0);
}
