//! AGENT-01/02：真实模型、UTF-16 双向锚点与必须失败的破坏用例。
mod common;
use rsword::{
    agent::{
        anchors::{Anchor, Target},
        text::{Scope, project},
    },
    model::Document,
    package::Package,
};
use std::collections::BTreeSet;
fn heading() -> (Package, Document) {
    let bytes = common::docx_with_body(
        "<w:p><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr><w:r><w:t>中😀文</w:t></w:r></w:p>",
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    (pkg, doc)
}
#[test]
fn agent_02_utf16_example_and_reverse() {
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    assert_eq!(p.content, "# 中😀文\n");
    for (offset, native) in [(2, 0), (3, 1), (5, 3)] {
        let a = p.anchors.to_anchor(offset, None).unwrap();
        let Target::Source { inline_pos, .. } = a.target else { panic!("source required") };
        assert_eq!(inline_pos.offset.0, native);
    }
    assert_eq!(p.anchors.counts.source_utf16, 4);
    assert_eq!(p.anchors.counts.source_scalars, 3);
    assert_eq!(p.anchors.counts.presentation_utf16, 3);
    assert_eq!(p.anchors.counts.presentation_scalars, 3);
    p.anchors.validate(&p.content, &doc).unwrap();
    for offset in 0..=p.anchors.len() {
        if offset == 4 {
            continue;
        }
        let a = p.anchors.to_anchor(offset, None).unwrap();
        assert_eq!(p.anchors.to_text_offset(&a).unwrap(), offset);
        let json = serde_json::to_string(&a).unwrap();
        let restored: Anchor = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, a);
    }
}
#[test]
fn agent_02_missing_synthetic_mapping_must_fail() {
    let (pkg, doc) = heading();
    let mut p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    p.anchors.segments.remove(0);
    assert_eq!(p.anchors.validate(&p.content, &doc).unwrap_err().code, "AGENT_BAD_ANCHOR");
}
#[test]
fn agent_01_presentation_reason_must_be_a_declared_category() {
    let (pkg, doc) = heading();
    let original = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    for reason in ["", "paragraph", "zzUnknown"] {
        let mut p = original.clone();
        let Target::Presentation { reason: actual, .. } = &mut p.anchors.segments[0].target else {
            panic!("标题前缀必须是呈现字符");
        };
        *actual = reason.into();
        assert_eq!(p.anchors.validate(&p.content, &doc).unwrap_err().code, "AGENT_BAD_ANCHOR");
    }
}
#[test]
fn agent_02_wrong_part_must_fail() {
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    let mut a = p.anchors.to_anchor(2, None).unwrap();
    let Target::Source { part, .. } = &mut a.target else { panic!() };
    *part += 1;
    assert_eq!(p.anchors.to_text_offset(&a).unwrap_err().code, "AGENT_BAD_ANCHOR");
}
#[test]
fn agent_02_forged_source_must_fail() {
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    let mut a = p.anchors.to_anchor(0, None).unwrap();
    a.target = p.anchors.to_anchor(2, None).unwrap().target;
    assert_eq!(p.anchors.to_text_offset(&a).unwrap_err().code, "AGENT_BAD_ANCHOR");
}
#[test]
fn agent_02_surrogate_midpoint_must_fail() {
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    assert_eq!(p.anchors.to_anchor(4, None).unwrap_err().code, "AGENT_BAD_OFFSET");
}
#[test]
fn agent_01_cell_selection_same_range_and_union_dedup() {
    let bytes = common::docx_with_body(
        "<w:p><w:r><w:t>outside</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>cell one</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>cell two</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = project(&pkg, &doc, Scope::All, "test:1").unwrap();
    let cell =
        p.objects.values().find(|o| o.object.kind == "cell" && o.metadata["column"] == 0).unwrap();
    let table = p.objects.values().find(|o| o.object.kind == "table").unwrap();
    assert_eq!(cell.object.flow, table.object.flow);
    let selected = p.select_objects(std::slice::from_ref(&cell.object)).unwrap();
    assert_eq!(selected, vec![cell.range.clone()]);
    assert_eq!(p.text_range(selected[0].clone()).unwrap(), "cell one\n");
    assert_eq!(p.content.matches("cell one").count(), 1);
    assert_eq!(
        p.select_objects(&[table.object.clone(), cell.object.clone(), cell.object.clone()])
            .unwrap(),
        vec![table.range.clone()]
    );
}
#[test]
fn agent_01_02_corpus_determinism_and_complete_anchors() {
    let root = common::repo_root().join("corpus");
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103);
    let mut refused = BTreeSet::new();
    let mut count = 0;
    let mut no_flow = Vec::new();
    let mut categories = BTreeSet::new();
    for path in paths {
        let name = path.strip_prefix(&root).unwrap().to_str().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = match Package::open(&bytes) {
            Ok(p) => {
                assert!(!common::UNOPENABLE.contains(&name));
                p
            }
            Err(e) => {
                assert!(common::UNOPENABLE.contains(&name), "{name}: {e}");
                refused.insert(name.to_owned());
                continue;
            }
        };
        let doc = Document::rebuild(&mut pkg).unwrap();
        // 穷尽所有可解析 XML part，不只枚举 Agent 碰巧输出的段落。
        let xml_parts: Vec<_> = pkg.parts().iter().filter(|p| p.is_xml).map(|p| p.id).collect();
        for id in xml_parts {
            if let Ok(Some(dom)) = pkg.dom(id) {
                let flows = rsword::span::FlowMap::build(dom);
                for node in dom.descendants(dom.root()) {
                    if dom.is(node, rsword::xml::QName::w(rsword::xml::LocalName::P))
                        && flows.flow_of(node).is_none()
                    {
                        no_flow.push(format!(
                            "{name}: part {} root {:?} paragraph {}",
                            id.0,
                            dom.name(dom.root()),
                            node.0
                        ));
                    }
                }
            }
        }
        let p =
            project(&pkg, &doc, Scope::All, "corpus:1").unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = project(&pkg, &doc, Scope::All, "corpus:1").unwrap();
        assert_eq!(
            serde_json::to_vec(&p).unwrap(),
            serde_json::to_vec(&again).unwrap(),
            "{name}: 投影不确定"
        );
        p.anchors.validate(&p.content, &doc).unwrap_or_else(|e| panic!("{name}: {e}"));
        for segment in &p.anchors.segments {
            if let Target::Presentation { reason, .. } = &segment.target {
                assert!(!reason.is_empty(), "{name}: 空呈现原因");
                assert!(
                    rsword::agent::text::CATEGORIES.iter().any(|c| c.name() == reason),
                    "{name}: 未声明呈现原因 {reason}"
                );
            }
        }
        assert_eq!(p.anchor_counts, p.anchors.counts);
        for r in doc.revisions.entries() {
            let flow = doc.flow_of_in(r.part, r.meta.node).expect("修订有原生流");
            let object = rsword::agent::anchors::ObjectRef {
                part: r.part.0,
                node: r.meta.node.0,
                flow: flow.0,
                kind: "revision".into(),
            };
            let meta = &p
                .objects
                .get(&object.key())
                .unwrap_or_else(|| panic!("{name}: 修订 {} 未投影", r.id.0))
                .metadata;
            assert_eq!(meta["id"], r.id.0);
            assert_eq!(meta["author"], serde_json::json!(r.meta.author));
            assert_eq!(meta["date"], serde_json::json!(r.meta.date));
            assert_eq!(meta["kind"], r.kind.as_str());
        }
        let mut offset = 0;
        for c in p.content.chars() {
            let a = p.anchors.to_anchor(offset, None).unwrap();
            assert_eq!(p.anchors.to_text_offset(&a).unwrap(), offset, "{name}");
            offset += c.len_utf16() as u32;
        }
        let a = p.anchors.to_anchor(offset, None).unwrap();
        assert_eq!(p.anchors.to_text_offset(&a).unwrap(), offset);
        for omission in p.omitted["page"].as_array().unwrap() {
            let cat = omission["category"].as_str().unwrap().to_owned();
            if categories.insert(cat.clone()) {
                eprintln!("AGENT category {cat}: {name}");
            }
        }
        count += 1;
    }
    assert_eq!(count, 1099);
    assert_eq!(no_flow.len(), 0, "全语料无流段落必须为 0: {no_flow:?}");
    assert_eq!(refused, common::UNOPENABLE.into_iter().map(str::to_owned).collect());
    assert_eq!(
        categories,
        rsword::agent::text::CATEGORIES.iter().map(|c| c.name().to_owned()).collect()
    );
}

#[test]
fn span_01_external_textbox_flows_and_unmapped_diagnostic() {
    use rsword::{
        DiagCode,
        package::PartId,
        span::{FlowMap, SpanIndex},
        xml::{Dom, NodeId},
    };
    let xml=br#"<w14:txbx xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:bookmarkStart w:id="1" w:name="test"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p><w:txbxContent><w:p/></w:txbxContent></w14:txbx>"#;
    let dom = Dom::parse(PartId(0), xml).unwrap();
    let flows = FlowMap::build(&dom);
    assert_eq!(flows.flow_count(), 2);
    assert_eq!(flows.flow_of(dom.root()).unwrap().0, 1, "新增根排在旧根之后");
    assert_eq!(
        flows.root_of(rsword::span::FlowId(0)),
        dom.descendants(dom.root())
            .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::TxbxContent)))
            .unwrap()
    );
    let mut spans = SpanIndex::build(&dom);
    assert_eq!(spans.len(), 1);
    assert!(spans.take_diagnostics().is_empty());
    let bad=Dom::parse(PartId(0),br#"<w:unknown xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:unknown>"#).unwrap();
    assert!(FlowMap::build(&bad).flow_of(NodeId(1)).is_none());
    let mut spans = SpanIndex::build(&bad);
    assert_eq!(
        spans.take_diagnostics().iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagCode::SpanNoFlow]
    );
}
#[test]
fn agent_02_unknown_anchor_fields_rejected() {
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    let a = p.anchors.to_anchor(2, None).unwrap();
    let mut value = serde_json::to_value(&a).unwrap();
    value["zzExtra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Anchor>(value).is_err());
}

#[test]
fn span_01_glossary_entries_are_distinct_and_reported() {
    let bytes =
        std::fs::read(common::corpus_dir("real").join("sdt/content-controls.docx")).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let flows = rsword::model::table::glossary_flows(&pkg).unwrap();
    assert_eq!(flows.len(), 3);
    assert_eq!(flows.iter().map(|x| (x.0, x.2)).collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(flows.iter().map(|x| x.3).sum::<usize>(), 3);
    let p = project(&pkg, &doc, Scope::All, "test:1").unwrap();
    let absent = p.omitted["addressableNotProjected"].as_array().unwrap();
    assert_eq!(absent.len(), 3);
    assert!(
        p.omitted["page"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["category"] == "glossary" && x["count"] == 3)
    );
    for x in absent {
        let object = serde_json::from_value(x["object"].clone()).unwrap();
        assert_eq!(p.select_objects(&[object]).unwrap_err().code, "AGENT_NOT_PROJECTED");
    }
}

#[test]
fn agent_01_catalogs_match_written_contract_both_directions() {
    let contract = include_str!("../../../docs/14-agent-text.md");
    let block = |tag: &str| {
        contract
            .split(&format!("```{tag}\n"))
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap()
            .lines()
            .collect::<BTreeSet<_>>()
    };
    let categories: BTreeSet<_> =
        rsword::agent::text::CATEGORY_FIXTURES.iter().map(|(c, f)| format!("{c} {f}")).collect();
    assert_eq!(categories, block("agent-categories").into_iter().map(str::to_owned).collect());
    let diagnostics: BTreeSet<_> =
        rsword::agent::diagnostics::KNOWN.iter().map(|(c, m, i)| format!("{c}|{m}|{i}")).collect();
    assert_eq!(
        diagnostics,
        block(&format!("agent-diagnostics-v{}", rsword::agent::diagnostics::VERSION))
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
}
#[test]
fn agent_01_escaped_literal_hidden_and_revision_text() {
    let bytes = common::docx_with_body(
        "<w:p><w:r><w:t>[image #1]|\\é😀</w:t></w:r><w:r><w:rPr><w:vanish/></w:rPr><w:t>SECRET</w:t></w:r><w:ins w:id=\"1\" w:author=\"张三\"><w:r><w:t>inserted</w:t></w:r></w:ins><w:del w:id=\"2\" w:author=\"李四\"><w:r><w:delText>deleted</w:delText></w:r></w:del></w:p>",
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    assert!(p.content.starts_with("\\[image \\#1\\]\\|\\\\é😀"));
    assert!(!p.content.contains("SECRET"));
    assert!(p.content.contains("[hidden #"));
    assert!(p.content.contains("inserted[/ins]"));
    assert!(p.content.contains("deleted[/del]"));
    assert!(!p.objects.values().any(|x| x.object.kind == "image"), "用户文字不得伪造图片对象");
    p.anchors.validate(&p.content, &doc).unwrap();
}
#[test]
fn agent_02_affinity_candidates_and_stale_identity() {
    use rsword::agent::anchors::Affinity;
    let (pkg, doc) = heading();
    let p = project(&pkg, &doc, Scope::Main, "test:1").unwrap();
    let left = p.anchors.to_anchor(2, Some(Affinity::Left)).unwrap();
    let right = p.anchors.to_anchor(2, None).unwrap();
    assert_eq!(left.affinity, Affinity::Left);
    assert_eq!(right.affinity, Affinity::Right);
    assert!(matches!(left.target, Target::Presentation { .. }));
    assert!(matches!(right.target, Target::Source { .. }));
    assert_eq!(p.anchors.to_text_offset(&left).unwrap(), 2);
    let Target::Source { inline_pos, .. } = right.target else { panic!() };
    assert_eq!(p.anchors.source_candidates(inline_pos).unwrap(), vec![2]);
    assert_eq!(
        p.anchors
            .source_candidates(rsword::edit::InlinePos::new(inline_pos.para, inline_pos.offset.0))
            .unwrap(),
        vec![2]
    );
    let mut repeated = doc.clone();
    repeated.main.push(repeated.main[0].clone());
    let twice = project(&pkg, &repeated, Scope::Main, "test:1").unwrap();
    let candidates = twice.anchors.source_candidates(inline_pos).unwrap();
    assert_eq!(candidates.len(), 2);
    let a = twice.anchors.to_anchor(candidates[0], None).unwrap();
    let b = twice.anchors.to_anchor(candidates[1], None).unwrap();
    assert_ne!(a.segment_key, b.segment_key);
    assert_eq!(twice.anchors.to_text_offset(&b).unwrap(), candidates[1]);
    let mut forged = right.clone();
    forged.snapshot = "test:2".into();
    assert_eq!(p.anchors.to_text_offset(&forged).unwrap_err().code, "AGENT_BAD_ANCHOR");
    let end = p.anchors.to_anchor(p.anchors.len(), Some(Affinity::Right)).unwrap();
    assert_eq!(end.affinity, Affinity::Left);
}
#[test]
fn res_09_list_markers_restart_override_and_legal() {
    use rsword::model::block::ListRef;
    let numbering = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlRestart w:val="0"/><w:isLgl/><w:lvlText w:val="%1.%2"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="3"/></w:lvlOverride></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;
    let bytes = common::docx_with_parts("<w:p/>", &[("word/numbering.xml", numbering)]);
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let items: Vec<_> = [(1, 0), (1, 1), (2, 0), (1, 1), (1, 0)]
        .into_iter()
        .map(|(num_id, ilvl)| ListRef { num_id, ilvl, from_style: false })
        .collect();
    assert_eq!(
        rsword::resolve::Resolver::new(&doc).list_markers(&items),
        ["III.", "3.1", "IV.", "4.2", "V."].map(|s| Some(s.to_owned()))
    );
}

#[test]
fn agent_01_same_media_two_occurrences_have_distinct_objects() {
    let drawing = r#"<w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:extent cx="100" cy="100"/><wp:docPr id="1" name="image"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:blipFill><a:blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="sameMedia"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let bytes = common::docx_with_body(&format!("<w:p>{drawing}{drawing}</w:p>"));
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = project(&pkg, &doc, Scope::All, "test:1").unwrap();
    let objects: Vec<_> = p.objects.values().filter(|x| x.object.kind == "image").collect();
    assert_eq!(objects.len(), 2);
    assert_ne!(objects[0].object.node, objects[1].object.node);
    assert_ne!(objects[0].range, objects[1].range);
    assert_eq!(p.content.matches("[image #").count(), 2);
}
#[test]
fn agent_02_empty_projection_has_real_end_owner() {
    let bytes = common::docx_with_body("");
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    // 空 body 没有合成节点，末尾仍指向真实流根。
    let empty = doc;
    let p = project(&pkg, &empty, Scope::Main, "test:1").unwrap();
    assert!(p.content.is_empty());
    let anchor = p.anchors.to_anchor(0, None).unwrap();
    assert_eq!(p.anchors.to_text_offset(&anchor).unwrap(), 0);
    let Target::Presentation { owner, .. } = anchor.target else { panic!() };
    assert_eq!(owner.node, empty.body.unwrap().0);
    assert_eq!(owner.part, empty.main_part.0);
}
