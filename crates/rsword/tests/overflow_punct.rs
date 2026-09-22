//! PROP-04/05/06/07、RES-07、BIND-02：标点溢出属性的三态、继承、写回与 JSON 投影。

#![cfg(feature = "native")]

mod common;

use rsword::bind::native::{SessionTable, document_schema};
use rsword::model::Styles;
use rsword::package::{PartFlavor, PartId};
use rsword::resolve::{Provenance, Resolver};
use rsword::save::serialize;
use rsword::semantic::props::{
    Change, ParaProps, ParaPropsField, ParaPropsPatch, plan_apply_para_props, read_para_props,
    read_para_props_change,
};
use rsword::xml::Dom;
use serde_json::{Value, json};

const W_T: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_S: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

#[test]
fn bind_02_overflow_punct_preserves_all_on_off_forms_and_absence() {
    let cases = [
        ("", None),
        ("<w:overflowPunct/>", Some(true)),
        (r#"<w:overflowPunct w:val="true"/>"#, Some(true)),
        (r#"<w:overflowPunct w:val="1"/>"#, Some(true)),
        (r#"<w:overflowPunct w:val="on"/>"#, Some(true)),
        (r#"<w:overflowPunct w:val="false"/>"#, Some(false)),
        (r#"<w:overflowPunct w:val="0"/>"#, Some(false)),
        (r#"<w:overflowPunct w:val="off"/>"#, Some(false)),
    ];
    let schema = document_schema();
    assert_eq!(schema["$defs"]["ParaProps"]["properties"]["overflowPunct"]["type"], "boolean");
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (element, expected) in cases {
        let bytes = common::docx_with_body(&format!(
            "<w:p><w:pPr>{element}</w:pPr><w:r><w:t>x</w:t></w:r></w:p>"
        ));
        let mut table = SessionTable::default();
        let id = table.open(&bytes, None).unwrap();
        let document: Value = serde_json::from_str(&table.document(&id, None).unwrap()).unwrap();
        assert_eq!(
            document["main"][0]["props"].get("overflowPunct").and_then(Value::as_bool),
            expected,
            "{element}"
        );
        assert!(validator.is_valid(&document), "{document}");
        // 新增投影字段不能改变未编辑保存的字节。
        assert_eq!(table.save(&id, None).unwrap(), bytes);
    }
}

#[test]
fn prop_05_06_overflow_punct_patch_preserves_order_and_roundtrips() {
    let off: ParaPropsPatch = serde_json::from_value(json!({"overflowPunct": false})).unwrap();
    assert_eq!(off.overflow_punct, Change::Set(false));
    let unset: ParaPropsPatch = serde_json::from_value(json!({"overflowPunct": null})).unwrap();
    assert_eq!(unset.overflow_punct, Change::Unset);
    assert_eq!(serde_json::to_value(&off).unwrap(), json!({"overflowPunct": false}));
    assert_eq!(serde_json::to_value(&unset).unwrap(), json!({"overflowPunct": null}));

    for (ns, flavor, value) in
        [(W_T, PartFlavor::Transitional, "0"), (W_S, PartFlavor::Strict, "false")]
    {
        let xml = format!(
            r#"<w:p xmlns:w="{ns}"><w:pPr><w:kinsoku/><w:autoSpaceDE/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#
        );
        let mut dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
        let paragraph = dom.root();
        let ppr = dom.semantic_children(paragraph).next().unwrap();
        dom.apply_edits(&plan_apply_para_props(&dom, paragraph, Some(ppr), &off, flavor));
        let edited = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(
            edited.contains(&format!(
                r#"<w:kinsoku/><w:overflowPunct w:val="{value}"/><w:autoSpaceDE/>"#
            )),
            "{edited}"
        );
        let mut diagnostics = Vec::new();
        let props = read_para_props(&dom, Some(ppr), &mut diagnostics);
        assert_eq!(props.overflow_punct, Some(false));
        assert_eq!(props.raw_unmodeled.len(), 1, "未建模的 kinsoku 原样保留");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let on = ParaPropsPatch { overflow_punct: Change::Set(true), ..Default::default() };
        dom.apply_edits(&plan_apply_para_props(&dom, paragraph, Some(ppr), &on, flavor));
        assert_eq!(read_para_props(&dom, Some(ppr), &mut Vec::new()).overflow_punct, Some(true));
        dom.apply_edits(&plan_apply_para_props(&dom, paragraph, Some(ppr), &unset, flavor));
        assert_eq!(read_para_props(&dom, Some(ppr), &mut Vec::new()).overflow_punct, None);
        assert_eq!(String::from_utf8(serialize(&dom).unwrap()).unwrap(), xml);
    }
}

#[test]
fn prop_07_overflow_punct_reads_previous_revision_value() {
    let xml = format!(
        r#"<w:pPr xmlns:w="{W_T}"><w:overflowPunct/><w:pPrChange w:id="1" w:author="test"><w:pPr><w:overflowPunct w:val="false"/></w:pPr></w:pPrChange></w:pPr>"#
    );
    let dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
    let current = read_para_props(&dom, Some(dom.root()), &mut Vec::new());
    let (_, previous) = read_para_props_change(&dom, Some(dom.root()), &mut Vec::new()).unwrap();
    assert_eq!(current.overflow_punct, Some(true));
    assert_eq!(previous.overflow_punct, Some(false));
}

#[test]
fn res_07_overflow_punct_inherits_but_does_not_invent_a_default() {
    let empty = Resolver::from_parts(None, None, None, None);
    assert_eq!(empty.para(None, None, &ParaProps::default()).props.overflow_punct, None);
    let xml = format!(
        r#"<w:styles xmlns:w="{W_T}">
          <w:docDefaults><w:pPrDefault><w:pPr><w:overflowPunct/></w:pPr></w:pPrDefault></w:docDefaults>
          <w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:overflowPunct w:val="false"/></w:pPr></w:style>
          <w:style w:type="paragraph" w:styleId="Child"><w:basedOn w:val="Base"/></w:style>
        </w:styles>"#
    );
    let dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
    let styles = Styles::from_dom(&dom, &mut Vec::new()).unwrap();
    let resolver = Resolver::from_parts(Some(&styles), None, None, None);
    let defaults = resolver.para(None, None, &ParaProps::default());
    assert_eq!(defaults.props.overflow_punct, Some(true));
    assert_eq!(defaults.source(ParaPropsField::OverflowPunct), Provenance::DocDefaults);
    let inherited = resolver.para(Some("Child"), None, &ParaProps::default());
    assert_eq!(inherited.props.overflow_punct, Some(false));
    assert_eq!(
        inherited.source(ParaPropsField::OverflowPunct),
        Provenance::ParaStyle("Base".into())
    );
    let direct = ParaProps { overflow_punct: Some(true), ..Default::default() };
    let explicit = resolver.para(Some("Child"), None, &direct);
    assert_eq!(explicit.props.overflow_punct, Some(true));
    assert_eq!(explicit.source(ParaPropsField::OverflowPunct), Provenance::Direct);
}
