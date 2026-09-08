//! 逐变体验收包含不可表达载荷；拒绝集独立于宏表，双向锁死。
use super::codec::Codec;
use super::*;
use crate::edit::*;
use crate::package::PartId;
use crate::xml::{Dom, LocalName, NewElement, QName};
use std::collections::BTreeSet;

pub fn scratch() -> Dom {
    Dom::parse(PartId(0), br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:zz="urn:bind:test"><w:body><w:p/></w:body></w:document>"#).unwrap()
}
pub fn xml(dom: &mut Dom) -> NewElement {
    codec::parse_one("<w:pPr zz:custom=\"audit\"><zz:unknown/></w:pPr>", dom).unwrap()
}
pub fn bad_run(dom: &mut Dom) -> NewRun {
    NewRun {
        text: "audit".into(),
        props: Some(codec::parse_one("<w:rPr><zz:unknown/></w:rPr>", dom).unwrap()),
    }
}
pub fn bad_block(dom: &mut Dom) -> NewBlock {
    NewBlock::Paragraph { props: Some(xml(dom)), inlines: vec![NewInline::Run(bad_run(dom))] }
}
pub fn bad_field(dom: &mut Dom) -> NewField {
    NewField { instr: "PAGE".into(), result: vec![NewInline::Run(bad_run(dom))], mark_dirty: true }
}

const REFUSED: [&str; 9] = [
    "InsertAtom",
    "InsertBlock",
    "InsertField",
    "ReplaceInlines",
    "SetHeaderFooter",
    "SetNoteContent",
    "SetSdtContent",
    "SetTextboxContent",
    "UpdateBlockField",
];
pub fn check(name: &str, op: EditOp, dom: &mut Dom, lossless: bool) -> bool {
    match edit_op_to_json(&op, dom) {
        Ok(json) => {
            assert!(lossless, "{name}: expected a named rejection");
            let wire: EditOpJson = serde_json::from_str(&json).unwrap();
            let text = serde_json::to_string(&wire).unwrap();
            assert_eq!(edit_op_from_json(&text, dom).unwrap(), op, "{name}");
            true
        }
        Err(EditJsonError::Unrepresentable(reason)) => {
            assert!(!lossless && REFUSED.contains(&name), "{name}: unexpected refusal: {reason}");
            false
        }
        Err(e) => panic!("{name}: unexpected error: {e}"),
    }
}
#[test]
fn bind_03_exact_variant_outcomes() {
    let mut dom = scratch();
    let mut accepted = BTreeSet::new();
    let mut refused = BTreeSet::new();
    for (name, op, expected) in super::ops::examples(&mut dom) {
        let set = if check(name, op, &mut dom, expected) { &mut accepted } else { &mut refused };
        assert!(set.insert(name), "duplicate variant {name}");
    }
    assert_eq!(refused, REFUSED.into_iter().collect());
    assert_eq!(accepted.len(), 57);
    assert_eq!(accepted.len() + refused.len(), 66);
    eprintln!(
        "BIND-03: {} variants lossless, {} variants explicitly refused; total {}. BIND-03 v3 classification.",
        accepted.len(),
        refused.len(),
        accepted.len() + refused.len()
    );
}
#[test]
fn bind_03_structured_run_forward_roundtrip() {
    let mut dom = scratch();
    let wire = r#"{"op":"replaceInlines","part":null,"para":2,"inlines":[{"kind":"run","value":{"text":"structured","props":{"bold":true}}}]}"#;
    let first = edit_op_from_json(wire, &mut dom).unwrap();
    let second = edit_op_from_json(&edit_op_to_json(&first, &dom).unwrap(), &mut dom).unwrap();
    assert_eq!(first, second);
}
#[test]
fn bind_03_xml_escape_roundtrip_custom_names() {
    let mut dom = scratch();
    let op = EditOp::InsertBlock {
        at: BlockPos::end(crate::xml::NodeId(1)),
        block: NewBlock::Xml(xml(&mut dom)),
    };
    check("InsertBlockXml", op, &mut dom, true);
}
#[test]
fn bind_03_property_order_refuses_loss() {
    let mut dom = scratch();
    let props = codec::parse_one("<w:rPr><w:i/><w:b/></w:rPr>", &mut dom).unwrap();
    let op = EditOp::ReplaceInlines {
        part: None,
        para: crate::xml::NodeId(2),
        inlines: vec![NewInline::Run(NewRun { text: "order".into(), props: Some(props) })],
    };
    assert!(matches!(edit_op_to_json(&op, &dom), Err(EditJsonError::Unrepresentable(_))));
    // 常规生成器产出的同一组属性可表示。
    let run = NewRun { text: "ok".into(), props: Some(NewElement::new(QName::w(LocalName::RPr))) };
    let value = payload::RunCodec::encode(&run, &dom).unwrap();
    let mut cx = DecodeCx { dom: &mut dom, escapes: vec![] };
    assert_eq!(payload::RunCodec::decode(&value, &mut cx).unwrap(), run);
}

#[test]
fn bind_03_all_refused_variants_have_structured_forward_roundtrips() {
    use serde_json::json;
    let run = json!({"text":"structured", "props":{"bold":true,"italic":false}});
    let inline = json!({"kind":"run","value":run});
    let block = json!({"kind":"paragraph","value":{"props":{"keepNext":true},"inlines":[inline]}});
    let at = json!({"para":2,"offset":0,"part":null});
    let cases = [
        json!({"op":"replaceInlines","para":2,"inlines":[inline]}),
        json!({"op":"insertBlock","at":{"part":null,"at":{"end":1}},"block":block}),
        json!({"op":"insertField","at":at,"field":{"instr":"PAGE","result":[inline],"markDirty":true}}),
        json!({"op":"updateBlockField","field":0,"blocks":[block]}),
        json!({"op":"setHeaderFooter","sect":2,"kind":"header","variant":"default","content":[block]}),
        json!({"op":"insertAtom","at":at,"atom":{"kind":"noteRef","value":{"endnote":false,"content":[[run]]}}}),
        json!({"op":"setNoteContent","endnote":false,"id":"1","content":[[run]]}),
        json!({"op":"setSdtContent","sdt":2,"inlines":[inline]}),
        json!({"op":"setTextboxContent","textbox":2,"blocks":[block]}),
    ];
    let mut seen = BTreeSet::new();
    for wire in cases {
        let name = wire["op"].as_str().unwrap();
        seen.insert(format!("{}{}", name[..1].to_uppercase(), &name[1..]));
        let mut dom = scratch();
        let first = edit_op_from_json(&wire.to_string(), &mut dom).unwrap();
        let encoded = edit_op_to_json(&first, &dom).unwrap();
        let second = edit_op_from_json(&encoded, &mut dom).unwrap();
        assert_eq!(first, second, "{name}: forward roundtrip");
    }
    assert_eq!(seen, REFUSED.iter().map(|s| (*s).to_owned()).collect());
}

#[test]
fn bind_03_recursive_inline_and_field_props_are_contextual() {
    use serde_json::json;
    let leaf = json!({"kind":"field","value":{"instr":"PAGE","result":[],"separate":true,"dirty":false,"props":{"bold":true}}});
    let wire = json!({"op":"replaceInlines","para":2,"inlines":[{"kind":"hyperlink","value":{"target":{"anchor":"bookmark"},"tooltip":null,"inlines":[{"kind":"ins","value":{"rev":{"id":"1","author":"audit","date":null},"inlines":[leaf]}}]}}]});
    let mut dom = scratch();
    let first = edit_op_from_json(&wire.to_string(), &mut dom).unwrap();
    let text = edit_op_to_json(&first, &dom).unwrap();
    assert_eq!(edit_op_from_json(&text, &mut dom).unwrap(), first);
}

#[test]
fn bind_03_escape_categories_count_conversion_metadata() {
    use serde_json::json;
    let cases = [
        json!({"op":"insertBlock","at":{"at":{"end":1}},"block":{"kind":"xml","value":"<w:p/>"}}),
        json!({"op":"insertBlock","at":{"at":{"end":1}},"block":{"kind":"wrapped","value":{"wrapper":"<w:ins w:id=\"1\"/>","block":{"kind":"xml","value":"<w:p/>"}}}}),
        json!({"op":"replaceInlines","para":2,"inlines":[{"kind":"xml","value":"<w:r/>"}]}),
        json!({"op":"replaceParaProps","para":2,"props":"<w:pPr/>"}),
        json!({"op":"replacePartXml","part":0,"xml":"<w:document/>"}),
        json!({"op":"replacePartBytes","part":0,"bytes":"AAEC"}),
    ];
    for (case, count) in cases.into_iter().zip([1, 2, 1, 1, 1, 1]) {
        let mut dom = scratch();
        let (_, escapes) = decode_with_escapes(&case.to_string(), &mut dom).unwrap();
        assert_eq!(escapes.len(), count, "{case}");
    }
}

#[test]
fn bind_03_refused_list_matches_written_contract() {
    let doc = include_str!("../../../../../../docs/native-edit-json.md");
    let written: BTreeSet<_> = doc
        .lines()
        .filter_map(|line| {
            line.strip_prefix("| `").and_then(|s| s.split_once('`').map(|(name, _)| name))
        })
        .collect();
    assert_eq!(written, REFUSED.into_iter().collect());
}
