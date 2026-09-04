//! 属性表验收（`spec/05` 验收清单 PROP-02 / 04 / 07 / 09，任务 1.1）。

use super::*;
use crate::diag::DiagCode;
use crate::package::PartId;
use crate::save::serialize;

const W_T: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_S: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

fn dom(xml: &str) -> Dom {
    Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{xml}"))
}

/// 用 `inner` 作 `w:rPr` 的内容解析，返回 (dom, props, 诊断)。
fn rpr(inner: &str) -> (Dom, RunProps, Vec<Diagnostic>) {
    let d = dom(&format!(r#"<w:rPr xmlns:w="{W_T}" xmlns:w14="{W14}">{inner}</w:rPr>"#));
    let mut diags = Vec::new();
    let p = read_run_props(&d, Some(d.root()), &mut diags);
    (d, p, diags)
}

fn to_xml(e: &NewElement, ns: &str) -> String {
    let mut d = dom(&format!(r#"<w:r xmlns:w="{ns}" xmlns:w14="{W14}"/>"#));
    let id = e.materialize(&mut d);
    d.append_child(d.root(), id);
    let bytes = serialize(&d).unwrap();
    let s = String::from_utf8(bytes).unwrap();
    // 去掉外层 w:r
    let start = s.find('>').unwrap() + 1;
    let end = s.rfind("</w:r>").unwrap();
    s[start..end].to_string()
}

#[test]
fn prop_02_acceptance_rows() {
    // `<w:sz w:val="12pt"/>` → 24
    let (_, p, diags) = rpr(r#"<w:sz w:val="12pt"/><w:spacing w:val="1in"/><w:b w:val="off"/>"#);
    assert_eq!(p.size, Some(Val::Value(24)));
    assert_eq!(p.spacing, Some(Val::Value(1440)));
    assert_eq!(p.bold, Some(false));
    assert!(diags.is_empty(), "{diags:?}");

    // Strict 生成 `w:val="false"`；Transitional 生成 `w:val="0"`；true 为裸元素
    let v = RunProps { bold: Some(false), italic: Some(true), ..Default::default() };
    assert_eq!(
        to_xml(&emit_run_props(&v, PartFlavor::Strict), W_S),
        r#"<w:rPr><w:b w:val="false"/><w:i/></w:rPr>"#
    );
    assert_eq!(
        to_xml(&emit_run_props(&v, PartFlavor::Transitional), W_T),
        r#"<w:rPr><w:b w:val="0"/><w:i/></w:rPr>"#
    );
}

#[test]
fn prop_02_on_off_variants_and_bad_value() {
    let (_, p, diags) = rpr(r#"<w:b/><w:i w:val="1"/><w:caps w:val="true"/><w:strike w:val="on"/>
           <w:vanish w:val="0"/><w:rtl w:val="false"/><w:cs w:val="off"/><w:dstrike w:val="maybe"/>"#);
    assert_eq!(p.bold, Some(true));
    assert_eq!(p.italic, Some(true));
    assert_eq!(p.caps, Some(true));
    assert_eq!(p.strike, Some(true));
    assert_eq!(p.vanish, Some(false));
    assert_eq!(p.rtl, Some(false));
    assert_eq!(p.cs, Some(false));
    // 其他 → Some(true) + 诊断
    assert_eq!(p.dstrike, Some(true));
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagCode::PropBadValue);
    assert!(diags[0].message.contains("w:dstrike"), "{}", diags[0].message);
    // 未声明 → None（PROP-04 三态）
    assert_eq!(p.small_caps, None);
}

#[test]
fn prop_02_struct_attrs_and_enums() {
    let (_, p, diags) = rpr(
        r##"<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="宋体" w:asciiTheme="minorHAnsi" w:hint="eastAsia"/>
           <w:color w:val="#ff0000" w:themeColor="accent1" w:themeTint="99"/>
           <w:highlight w:val="yellow"/>
           <w:u w:val="single" w:color="auto"/>
           <w:shd w:val="clear" w:color="auto" w:fill="D9D9D9"/>
           <w:vertAlign w:val="superscript"/>
           <w:em w:val="dot"/>
           <w:lang w:val="en-US" w:eastAsia="zh-CN" w:bidi="ar-SA"/>
           <w:w w:val="150%"/><w:kern w:val="2"/><w:position w:val="-6"/><w:szCs w:val="21"/>"##,
    );
    assert!(diags.is_empty(), "{diags:?}");
    let f = p.fonts.as_ref().unwrap();
    assert_eq!(f.ascii.as_deref(), Some("Calibri"));
    assert_eq!(f.east_asia.as_deref(), Some("宋体"));
    assert_eq!(f.ascii_theme, Some(Val::Value(ThemeFont::MinorHAnsi)));
    assert_eq!(f.hint, Some(Val::Value(FontHint::EastAsia)));
    assert_eq!(f.cs, None);
    let c = p.color.as_ref().unwrap();
    assert_eq!(c.val, Some(Val::Value(HexColorOrAuto::Rgb([255, 0, 0]))));
    assert_eq!(c.theme_color, Some(Val::Value(ThemeColor::Accent1)));
    assert_eq!(c.theme_tint, Some(Val::Value(0x99)));
    assert_eq!(c.theme_shade, None);
    assert_eq!(p.highlight, Some(Val::Value(HighlightColor::Yellow)));
    assert_eq!(p.underline.as_ref().unwrap().val, Some(Val::Value(UnderlineKind::Single)));
    assert_eq!(p.underline.as_ref().unwrap().color, Some(Val::Value(HexColorOrAuto::Auto)));
    let s = p.shading.as_ref().unwrap();
    assert_eq!(s.val, Some(Val::Value(ShadingPattern::Clear)));
    assert_eq!(s.fill, Some(Val::Value(HexColorOrAuto::Rgb([0xD9, 0xD9, 0xD9]))));
    assert_eq!(p.vert_align, Some(Val::Value(VerticalAlignRun::Superscript)));
    assert_eq!(p.em, Some(Val::Value(EmphasisMark::Dot)));
    assert_eq!(p.lang.as_ref().unwrap().bidi.as_deref(), Some("ar-SA"));
    assert_eq!(p.scale, Some(Val::Value(150)));
    assert_eq!(p.kern, Some(Val::Value(2)));
    assert_eq!(p.position, Some(Val::Value(-6)));
    assert_eq!(p.size_cs, Some(Val::Value(21)));

    // 写回：颜色大写、去掉 `#`；tint 两位大写；百分比写整数
    let out = to_xml(&emit_run_props(&p, PartFlavor::Transitional), W_T);
    assert!(
        out.contains(r#"<w:color w:val="FF0000" w:themeColor="accent1" w:themeTint="99"/>"#),
        "{out}"
    );
    assert!(out.contains(r#"<w:w w:val="150"/>"#), "{out}");
    assert!(out.contains(r#"<w:rFonts w:hint="eastAsia" w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="宋体" w:asciiTheme="minorHAnsi"/>"#), "{out}");
}

#[test]
fn prop_09_raw_values_survive_and_write_back_verbatim() {
    let (_, p, diags) =
        rpr(r#"<w:highlight w:val="weird"/><w:color w:val="notacolor" w:themeShade="xyz"/>
           <w:sz w:val="big"/><w:sz w:val="24"/><w:kern/>"#);
    assert_eq!(p.highlight, Some(Val::Raw("weird".into())));
    let c = p.color.as_ref().unwrap();
    assert_eq!(c.val, Some(Val::Raw("notacolor".into())));
    assert_eq!(c.theme_shade, Some(Val::Raw("xyz".into())));
    // 第一个 w:sz 生效（Raw），第二个进 raw_unmodeled
    assert_eq!(p.size, Some(Val::Raw("big".into())));
    assert_eq!(p.raw_unmodeled.len(), 1);
    // 缺 w:val → Raw("")
    assert_eq!(p.kern, Some(Val::Raw(String::new())));
    let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
    assert_eq!(codes.len(), 5, "{diags:#?}");
    assert!(codes.iter().all(|c| *c == DiagCode::PropBadValue));

    let out = to_xml(&emit_run_props(&p, PartFlavor::Strict), W_S);
    assert!(out.contains(r#"<w:highlight w:val="weird"/>"#), "{out}");
    assert!(out.contains(r#"<w:color w:val="notacolor" w:themeShade="xyz"/>"#), "{out}");
    assert!(out.contains(r#"<w:sz w:val="big"/>"#), "{out}");
    assert!(out.contains(r#"<w:kern w:val=""/>"#), "{out}");

    // 比较按原文：同样的 Raw 相等，Raw 与 Value 不等
    assert_eq!(Val::<u32>::Raw("big".into()), Val::Raw("big".into()));
    assert_ne!(Val::Raw("24".into()), Val::Value(24u32));
}

#[test]
fn prop_07_read_emit_read_roundtrip_and_unmodeled() {
    let src = r#"<w:rStyle w:val="Emphasis"/><w:b/><w:bCs w:val="0"/><w:i/>
        <w:bdr w:val="single" w:sz="4" w:space="0" w:color="auto"/>
        <w:color w:val="0070C0"/><w:sz w:val="28"/>
        <w:u w:val="double"/><w14:textFill><w14:solidFill><w14:srgbClr w14:val="FF0000"/></w14:solidFill></w14:textFill>
        <w:rPrChange w:id="1" w:author="a" w:date="2020-01-01T00:00:00Z"><w:rPr><w:b w:val="0"/><w:sz w:val="20"/></w:rPr></w:rPrChange>"#;
    let (d, p, diags) = rpr(src);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(p.style.as_deref(), Some("Emphasis"));
    assert!(p.text_fill.is_some(), "Raw 字段读为 NodeId");
    // 未建模：w:bdr、w:rPrChange
    let unmodeled: Vec<String> = p
        .raw_unmodeled
        .iter()
        .map(|n| d.name(*n).unwrap().display(d.interner()).to_string())
        .collect();
    assert_eq!(unmodeled, ["w:bdr", "w:rPrChange"]);

    // 修订快照
    let (change, old) = read_run_props_change(&d, Some(d.root()), &mut Vec::new()).unwrap();
    assert!(d.is(change, QName::w(LocalName::RPrChange)));
    assert_eq!(old.bold, Some(false));
    assert_eq!(old.size, Some(Val::Value(20)));
    assert_eq!(old.italic, None);

    // emit → materialize → read：建模字段全等（raw_unmodeled 与 Raw 字段不在比较里）
    let e = emit_run_props(&p, PartFlavor::Transitional);
    let mut d2 = dom(&format!(r#"<w:r xmlns:w="{W_T}" xmlns:w14="{W14}"/>"#));
    let id = e.materialize(&mut d2);
    d2.append_child(d2.root(), id);
    let mut p2 = read_run_props(&d2, Some(id), &mut Vec::new());
    assert!(p2.raw_unmodeled.is_empty());
    assert_eq!(p2.text_fill, None, "Raw 字段不由 emit 生成");
    p2.text_fill = p.text_fill;
    assert_eq!(p2, p);
    assert!(diff_run_props(&p, &p2).is_empty());

    // 生成顺序 = schema 顺序（PROP-05）
    let names: Vec<String> =
        e.children.iter().map(|c| c.name.display(d2.interner()).to_string()).collect();
    assert_eq!(names, ["w:rStyle", "w:b", "w:bCs", "w:i", "w:color", "w:sz", "w:u"]);
}

#[test]
fn prop_06_diff_and_patch_emit() {
    let (_, a, _) = rpr(r#"<w:b/><w:sz w:val="24"/><w:color w:val="FF0000"/><w:i/>"#);
    let (_, b, _) =
        rpr(r#"<w:b/><w:sz w:val="28"/><w:color w:val="FF0000" w:themeColor="accent1"/>"#);
    let patch = diff_run_props(&a, &b);
    assert_eq!(patch.bold, Change::Keep);
    assert_eq!(patch.size, Change::Set(Val::Value(28)));
    assert_eq!(patch.italic, Change::Unset);
    assert!(matches!(patch.color, Change::Set(_)));
    assert!(!patch.is_empty());
    assert!(diff_run_props(&a, &a).is_empty());

    assert_eq!(patch.kind(RunPropsField::Bold), ChangeKind::Keep);
    assert_eq!(patch.kind(RunPropsField::Size), ChangeKind::Set);
    assert_eq!(patch.kind(RunPropsField::Italic), ChangeKind::Unset);
    assert!(patch.emit_field(RunPropsField::Bold, PartFlavor::Transitional).is_empty());
    assert!(patch.emit_field(RunPropsField::Italic, PartFlavor::Transitional).is_empty());
    let sz = patch.emit_field(RunPropsField::Size, PartFlavor::Transitional);
    assert_eq!(sz.len(), 1);
    assert_eq!(sz[0].name, QName::w(LocalName::Sz));
    assert_eq!(sz[0].attrs, vec![(QName::w(LocalName::Val), "28".to_string())]);
}

#[test]
fn prop_05_order_index_and_field_info() {
    let idx = |l: LocalName| order_index_run_props(QName::w(l));
    assert!(idx(LocalName::RStyle) < idx(LocalName::B));
    assert!(idx(LocalName::B) < idx(LocalName::Sz));
    // 未建模但在 schema 里的元素也有序号
    assert!(idx(LocalName::Sz) < idx(LocalName::Bdr));
    assert!(idx(LocalName::Bdr) < idx(LocalName::RPrChange));
    assert_eq!(order_index_run_props(QName::w(LocalName::P)), None);
    assert!(
        order_index_run_props(QName::new(NsId::W14, LocalName::TextFill)) > idx(LocalName::OMath)
    );

    // 字段元数据与字段枚举一致
    for (i, f) in RunPropsField::ALL.iter().enumerate() {
        assert_eq!(*f as usize, i);
        assert_eq!(f.info().order, order_index_run_props(f.info().element).unwrap());
    }
    let b = RunPropsField::Bold.info();
    assert_eq!(b.cs_twin, Some("bold_cs"));
    assert_eq!(RUN_PROPS.field("bold_cs").unwrap().cs_twin, Some("bold"));
    assert_eq!(RUN_PROPS.change, Some(QName::w(LocalName::RPrChange)));
    assert_eq!(RunPropsField::TextFill.info().kind, FieldKind::Raw);
    assert!(!RunPropsField::TextFill.info().in_change);
    assert_eq!((RUN_PROPS.order_index)(QName::w(LocalName::Sz)), idx(LocalName::Sz));
    assert!(TABLES.iter().any(|t| t.name == "RunProps"));
}

#[test]
fn prop_02_enum_api() {
    assert_eq!(ThemeColor::parse("accent3"), Some(ThemeColor::Accent3));
    assert_eq!(ThemeColor::Accent3.as_str(), "accent3");
    assert_eq!(ThemeColor::parse("Accent3"), None, "枚举字面大小写敏感");
    assert_eq!(ThemeColor::ALL.len(), 17);
    assert_eq!(HighlightColor::None.as_str(), "none");
}
