//! 属性表验收（`spec/05` 验收清单 PROP-02 / 04 / 07 / 09，任务 1.1）。

use super::*;
use crate::diag::DiagCode;
use crate::package::PartId;
use crate::save::serialize;
use crate::xml::Dirty;

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
        e.child_elements().map(|c| c.name.display(d2.interner()).to_string()).collect();
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

// ---- ParaProps（任务 1.2）-------------------------------------------------------------------------

/// 用 `inner` 作 `w:pPr` 的内容解析。
fn ppr(inner: &str) -> (Dom, ParaProps, Vec<Diagnostic>) {
    let d = dom(&format!(r#"<w:pPr xmlns:w="{W_T}" xmlns:w14="{W14}">{inner}</w:pPr>"#));
    let mut diags = Vec::new();
    let p = read_para_props(&d, Some(d.root()), &mut diags);
    (d, p, diags)
}

#[test]
fn prop_04_three_state_on_off_in_ppr() {
    let (_, p, diags) =
        ppr(r#"<w:keepNext w:val="0"/><w:keepLines/><w:widowControl w:val="false"/>"#);
    assert!(diags.is_empty());
    assert_eq!(p.keep_next, Some(false), "显式关闭，覆盖样式");
    assert_eq!(p.keep_lines, Some(true));
    assert_eq!(p.widow_control, Some(false));
    assert_eq!(p.page_break_before, None, "未声明 = 继承");
    assert_eq!(p.bidi, None);
}

#[test]
fn prop_02_indent_legacy_spelling_by_flavor() {
    // `<w:ind w:left="1in"/>` → 1440（Transitional 拼写也读）
    let (_, p, diags) = ppr(r#"<w:ind w:left="1in" w:hanging="360" w:rightChars="100"/>"#);
    assert!(diags.is_empty());
    let ind = p.indent.as_ref().unwrap();
    assert_eq!(ind.start, Some(Val::Value(1440)));
    assert_eq!(ind.hanging, Some(Val::Value(360)));
    assert_eq!(ind.end_chars, Some(Val::Value(100)));
    assert_eq!(ind.end, None);
    // 生成：Transitional 写 left/rightChars，Strict 写 start/endChars
    let t = to_xml(&emit_para_props(&p, PartFlavor::Transitional), W_T);
    assert_eq!(t, r#"<w:pPr><w:ind w:left="1440" w:rightChars="100" w:hanging="360"/></w:pPr>"#);
    let s = to_xml(&emit_para_props(&p, PartFlavor::Strict), W_S);
    assert_eq!(s, r#"<w:pPr><w:ind w:start="1440" w:endChars="100" w:hanging="360"/></w:pPr>"#);
    // Strict 拼写优先于 legacy（两者同时出现时）
    let (_, p2, _) = ppr(r#"<w:ind w:start="10" w:left="20"/>"#);
    assert_eq!(p2.indent.unwrap().start, Some(Val::Value(10)));
}

#[test]
fn prop_09_jc_weird_kept_verbatim() {
    let (_, p, diags) = ppr(r#"<w:jc w:val="weird"/>"#);
    assert_eq!(p.jc, Some(Val::Raw("weird".into())));
    assert_eq!(diags.len(), 1);
    let out = to_xml(&emit_para_props(&p, PartFlavor::Strict), W_S);
    assert_eq!(out, r#"<w:pPr><w:jc w:val="weird"/></w:pPr>"#);
}

#[test]
fn prop_08_nested_tables_sub_tables_and_multi() {
    let (d, p, diags) = ppr(r#"<w:pStyle w:val="Heading1"/>
           <w:numPr><w:ilvl w:val="1"/><w:numId w:val="3"/></w:numPr>
           <w:pBdr><w:top w:val="single" w:sz="4" w:space="1" w:color="auto"/><w:bottom w:val="apples" w:sz="31"/></w:pBdr>
           <w:tabs><w:tab w:val="left" w:pos="720"/><w:tab w:val="right" w:leader="dot" w:pos="9000"/></w:tabs>
           <w:spacing w:before="240" w:after="0" w:line="360" w:lineRule="auto" w:beforeAutospacing="1"/>
           <w:outlineLvl w:val="0"/>
           <w:rPr><w:ins w:id="5" w:author="x" w:date="2020-01-01T00:00:00Z"/><w:b/><w:sz w:val="32"/></w:rPr>
           <w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>
           <w:pPrChange w:id="6" w:author="x" w:date="2020-01-01T00:00:00Z"><w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange>"#);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(p.style.as_deref(), Some("Heading1"));
    let num = p.num.as_ref().unwrap();
    assert_eq!(num.ilvl, Some(Val::Value(1)));
    assert_eq!(num.num_id, Some(Val::Value(3)));
    let b = p.borders.as_ref().unwrap();
    assert_eq!(b.top.as_ref().unwrap().val, Some(Val::Value(BorderStyle::Single)));
    assert_eq!(b.top.as_ref().unwrap().sz, Some(Val::Value(4)));
    assert_eq!(b.top.as_ref().unwrap().color, Some(Val::Value(HexColorOrAuto::Auto)));
    assert_eq!(b.bottom.as_ref().unwrap().val, Some(Val::Value(BorderStyle::Apples)));
    assert!(b.left.is_none() && b.between.is_none());
    let tabs = p.tabs.as_ref().unwrap();
    assert_eq!(tabs.tab.len(), 2);
    assert_eq!(tabs.tab[0].val, Some(Val::Value(TabJc::Left)));
    assert_eq!(tabs.tab[1].leader, Some(Val::Value(TabLeader::Dot)));
    assert_eq!(tabs.tab[1].pos, Some(Val::Value(9000)));
    let sp = p.spacing.as_ref().unwrap();
    assert_eq!(sp.line, Some(Val::Value(360)));
    assert_eq!(sp.line_rule, Some(Val::Value(LineSpacingRule::Auto)));
    assert_eq!(sp.before_autospacing, Some(true));
    assert_eq!(p.outline_lvl, Some(Val::Value(0)));
    // 段落标记 rPr：嵌套 RunProps，w:ins 进它的 raw_unmodeled
    let rpr = p.rpr.as_ref().unwrap();
    assert_eq!(rpr.bold, Some(true));
    assert_eq!(rpr.size, Some(Val::Value(32)));
    assert_eq!(rpr.raw_unmodeled.len(), 1);
    // sectPr 是 Raw 字段；pPrChange 进 raw_unmodeled
    assert!(d.is(p.sect_pr.unwrap(), QName::w(LocalName::SectPr)));
    assert_eq!(p.raw_unmodeled.len(), 1);
    assert!(d.is(p.raw_unmodeled[0], QName::w(LocalName::PPrChange)));
    // 快照
    let (_, old) = read_para_props_change(&d, Some(d.root()), &mut Vec::new()).unwrap();
    assert_eq!(old.jc, Some(Val::Value(Jc::Center)));
    assert_eq!(old.style, None);

    // 嵌套 diff：rPr 两侧都有 → Patch；pBdr 一侧无 → Set / Unset；tabs 整表替换
    let mut q = p.clone();
    q.rpr.as_mut().unwrap().italic = Some(true);
    q.borders = None;
    q.tabs.as_mut().unwrap().tab.pop();
    let patch = diff_para_props(&p, &q);
    assert!(
        matches!(&patch.rpr, TableChange::Patch(rp) if rp.italic == Change::Set(true) && rp.bold == Change::Keep)
    );
    assert_eq!(patch.kind(ParaPropsField::Rpr), ChangeKind::Patch);
    assert_eq!(patch.borders, TableChange::Unset);
    assert!(
        matches!(&patch.tabs, TableChange::Patch(tp) if matches!(&tp.tab, Change::Set(v) if v.len() == 1))
    );
    assert_eq!(patch.style, Change::Keep);
    assert!(
        patch.emit_field(ParaPropsField::Rpr, PartFlavor::Transitional).is_empty(),
        "Patch 不整体生成"
    );
    let rev = diff_para_props(&q, &p);
    assert!(matches!(&rev.borders, TableChange::Set(b) if b.top.is_some()));
    let e = rev.emit_field(ParaPropsField::Borders, PartFlavor::Transitional);
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].children.len(), 2);
    // 空 Patch 视为 Keep
    let same = diff_para_props(&p, &p);
    assert!(same.is_empty());
    assert_eq!(same.kind(ParaPropsField::Rpr), ChangeKind::Keep);
}

fn border(v: BorderStyle, sz: u32) -> Border {
    Border {
        val: Some(Val::Value(v)),
        sz: Some(Val::Value(sz)),
        space: Some(Val::Value(1)),
        color: Some(Val::Value(HexColorOrAuto::Rgb([0, 0x70, 0xC0]))),
        ..Default::default()
    }
}

/// 覆盖 `ParaProps` 全部非 Raw 字段的样本。
fn para_sample() -> ParaProps {
    ParaProps {
        style: Some("Normal".into()),
        keep_next: Some(true),
        keep_lines: Some(false),
        page_break_before: Some(true),
        frame: Some(FramePr {
            drop_cap: Some(Val::Value(DropCap::Drop)),
            lines: Some(Val::Value(3)),
            wrap: Some(Val::Value(FrameWrap::Around)),
            h_anchor: Some(Val::Value(FrameAnchor::Text)),
            v_anchor: Some(Val::Value(FrameAnchor::Margin)),
            x: Some(Val::Value(-100)),
            y_align: Some(Val::Value(YAlign::Top)),
            h_rule: Some(Val::Value(HeightRule::Exact)),
            anchor_lock: Some(true),
            ..Default::default()
        }),
        widow_control: Some(false),
        num: Some(NumPr {
            ilvl: Some(Val::Value(2)),
            num_id: Some(Val::Value(7)),
            ..Default::default()
        }),
        borders: Some(ParaBorders {
            top: Some(border(BorderStyle::Single, 4)),
            left: Some(border(BorderStyle::Double, 6)),
            bottom: Some(border(BorderStyle::Dashed, 8)),
            right: Some(border(BorderStyle::ThreeDEmboss, 12)),
            between: Some(border(BorderStyle::Nil, 0)),
            bar: Some(border(BorderStyle::Custom, 2)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Pct25)),
            fill: Some(Val::Value(HexColorOrAuto::Auto)),
            theme_fill: Some(Val::Value(ThemeColor::Accent2)),
            ..Default::default()
        }),
        tabs: Some(Tabs {
            tab: vec![
                Tab {
                    val: Some(Val::Value(TabJc::Center)),
                    pos: Some(Val::Value(4320)),
                    leader: None,
                },
                Tab {
                    val: Some(Val::Value(TabJc::Right)),
                    pos: Some(Val::Value(8640)),
                    leader: Some(Val::Value(TabLeader::Underscore)),
                },
            ],
            ..Default::default()
        }),
        auto_space_de: Some(false),
        auto_space_dn: Some(false),
        bidi: Some(true),
        snap_to_grid: Some(false),
        spacing: Some(Spacing {
            before: Some(Val::Value(120)),
            after: Some(Val::Value(0)),
            line: Some(Val::Value(276)),
            line_rule: Some(Val::Value(LineSpacingRule::Auto)),
            after_autospacing: Some(false),
            ..Default::default()
        }),
        indent: Some(Indent {
            start: Some(Val::Value(720)),
            end: Some(Val::Value(-10)),
            first_line: Some(Val::Value(420)),
            first_line_chars: Some(Val::Value(200)),
            ..Default::default()
        }),
        suppress_auto_hyphens: Some(false),
        contextual_spacing: Some(true),
        jc: Some(Val::Value(Jc::Both)),
        outline_lvl: Some(Val::Value(9)),
        rpr: Some(RunProps {
            bold: Some(true),
            size: Some(Val::Value(28)),
            color: Some(Color {
                val: Some(Val::Value(HexColorOrAuto::Rgb([1, 2, 3]))),
                ..Default::default()
            }),
            lang: Some(Language { val: Some("zh-CN".into()), ..Default::default() }),
            ..Default::default()
        }),
        sect_pr: None,
        raw_unmodeled: Vec::new(),
    }
}

/// PROP-07 每行往返：样本里每个非 Raw 字段都有值，emit → materialize → read 全等。
#[test]
fn prop_07_every_para_props_row_roundtrips() {
    let sample = para_sample();
    // 每一行都被样本覆盖
    for f in ParaPropsField::ALL {
        if f.info().kind == FieldKind::Raw {
            continue;
        }
        assert!(
            !emit_para_props_value(&sample, *f, PartFlavor::Transitional).is_empty(),
            "样本缺字段 {}",
            f.info().name
        );
    }
    for f in RunPropsField::ALL {
        assert_eq!(f.info().in_change, f.info().kind != FieldKind::Raw);
    }
    for flavor in [PartFlavor::Transitional, PartFlavor::Strict] {
        let ns = if flavor == PartFlavor::Strict { W_S } else { W_T };
        let e = emit_para_props(&sample, flavor);
        let xml = to_xml(&e, ns);
        let d = dom(&format!(r#"<w:p xmlns:w="{ns}" xmlns:w14="{W14}">{xml}</w:p>"#));
        let container = d.semantic_children(d.root()).next().unwrap();
        let mut diags = Vec::new();
        let back = read_para_props(&d, Some(container), &mut diags);
        assert!(diags.is_empty(), "{flavor:?}: {diags:?}");
        assert_eq!(back, sample, "{flavor:?}\n{xml}");
        assert!(diff_para_props(&sample, &back).is_empty());
        // 子元素顺序 = schema 顺序
        let mut last = 0;
        for c in d.semantic_children(container) {
            let i = order_index_para_props(d.name(c).unwrap()).unwrap();
            assert!(i >= last, "{xml}");
            last = i;
        }
        if flavor == PartFlavor::Strict {
            assert!(xml.contains(r#"<w:ind w:start="720" w:end="-10""#), "{xml}");
            assert!(xml.contains(r#"<w:keepLines w:val="false"/>"#), "{xml}");
        } else {
            assert!(xml.contains(r#"<w:ind w:left="720" w:right="-10""#), "{xml}");
            assert!(xml.contains(r#"<w:keepLines w:val="0"/>"#), "{xml}");
        }
    }
}

#[test]
fn prop_05_para_order_table() {
    let idx = |l: LocalName| order_index_para_props(QName::w(l)).unwrap();
    assert!(idx(LocalName::PStyle) < idx(LocalName::KeepNext));
    assert!(idx(LocalName::Spacing) < idx(LocalName::Ind));
    assert!(idx(LocalName::Ind) < idx(LocalName::Jc));
    assert!(idx(LocalName::Jc) < idx(LocalName::RPr));
    assert!(idx(LocalName::RPr) < idx(LocalName::SectPr));
    assert!(idx(LocalName::SectPr) < idx(LocalName::PPrChange));
    // 未建模但有序号：suppressLineNumbers 在 pBdr 前
    assert!(idx(LocalName::NumPr) < idx(LocalName::SuppressLineNumbers));
    assert!(idx(LocalName::SuppressLineNumbers) < idx(LocalName::PBdr));
    assert_eq!(order_index_para_props(QName::w(LocalName::B)), None);
    assert!(!ParaPropsField::Rpr.info().in_change);
    assert!(ParaPropsField::Num.info().in_change);
    assert_eq!(ParaPropsField::Rpr.info().kind, FieldKind::Table);
    assert_eq!(ParaPropsField::Tabs.info().kind, FieldKind::Table);
    assert!(TabsField::Tab.info().multi);
    assert_eq!(
        TABLES.len(),
        27,
        "run/para 5 + numbering 5 + styles 6 + fontTable 2 + settings 2 + table 3 + row 1 + cell 3"
    );
    assert!(PARA_PROPS.field("indent").is_some());
}

// ---- plan_apply（任务 1.3，PROP-05 / 06 / 07）------------------------------------------------------

fn p_dom(inner: &str) -> Dom {
    dom(&format!(r#"<w:p xmlns:w="{W_T}" xmlns:w14="{W14}">{inner}</w:p>"#))
}

fn first_child(d: &Dom, n: NodeId) -> NodeId {
    d.semantic_children(n).next().unwrap()
}

fn xml_of(d: &Dom) -> String {
    String::from_utf8(serialize(d).unwrap()).unwrap()
}

fn rgb(r: u8, g: u8, b: u8) -> Option<Val<HexColorOrAuto>> {
    Some(Val::Value(HexColorOrAuto::Rgb([r, g, b])))
}

#[test]
fn prop_05_new_element_inserted_before_first_greater_order() {
    // 向只有 w:jc 的 pPr 加 w:spacing → 插在 w:jc 之前
    let mut d = p_dom(r#"<w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>x</w:t></w:r>"#);
    let p = d.root();
    let ppr = first_child(&d, p);
    let jc = first_child(&d, ppr);
    let patch = ParaPropsPatch {
        spacing: Change::Set(Spacing { before: Some(Val::Value(240)), ..Default::default() }),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, p, Some(ppr), &patch, PartFlavor::Transitional);
    assert_eq!(edits.len(), 1);
    assert!(
        matches!(&edits[0], NodeEdit::Insert { parent: Target::Node(c), before: Some(b), node }
        if *c == ppr && *b == jc && node.name == QName::w(LocalName::Spacing))
    );
    d.apply_edits(&edits);
    assert!(
        xml_of(&d).contains(r#"<w:pPr><w:spacing w:before="240"/><w:jc w:val="center"/></w:pPr>"#),
        "{}",
        xml_of(&d)
    );

    // 向只有 w:sz 的 rPr 加 w:b → 插在 w:sz 之前
    let mut d = dom(&format!(
        r#"<w:r xmlns:w="{W_T}"><w:rPr><w:sz w:val="24"/></w:rPr><w:t>x</w:t></w:r>"#
    ));
    let r = d.root();
    let rpr = first_child(&d, r);
    let patch = RunPropsPatch { bold: Change::Set(true), ..Default::default() };
    d.apply_edits(&plan_apply_run_props(&d, r, Some(rpr), &patch, PartFlavor::Transitional));
    assert!(xml_of(&d).contains(r#"<w:rPr><w:b/><w:sz w:val="24"/></w:rPr>"#), "{}", xml_of(&d));

    // 没有更大序号的 → 追加到末尾，但在 rPrChange 之前
    let mut d = dom(&format!(
        r#"<w:r xmlns:w="{W_T}"><w:rPr><w:b/><w:rPrChange w:id="1" w:author="a" w:date="2020-01-01T00:00:00Z"><w:rPr/></w:rPrChange></w:rPr></w:r>"#
    ));
    let r = d.root();
    let rpr = first_child(&d, r);
    let patch = RunPropsPatch {
        lang: Change::Set(Language { val: Some("en-US".into()), ..Default::default() }),
        ..Default::default()
    };
    d.apply_edits(&plan_apply_run_props(&d, r, Some(rpr), &patch, PartFlavor::Transitional));
    assert!(xml_of(&d).contains(r#"<w:b/><w:lang w:val="en-US"/><w:rPrChange"#), "{}", xml_of(&d));
}

#[test]
fn prop_06_unmodeled_and_open_tag_bytes_preserved() {
    let src = format!(
        r#"<w:r xmlns:w="{W_T}"><w:rPr  w:x='1' ><w:b/><w:color w:val="FF0000"/><w:bdr w:val="single"  w:sz='4' w:space="0" w:color='auto'/><w:sz w:val="24"/></w:rPr><w:t>x</w:t></w:r>"#
    );
    let mut d = dom(&src);
    let r = d.root();
    let rpr = first_child(&d, r);
    let blue = Color { val: rgb(0, 0x70, 0xC0), ..Default::default() };
    let patch = RunPropsPatch { color: Change::Set(blue.clone()), ..Default::default() };
    let edits = plan_apply_run_props(&d, r, Some(rpr), &patch, PartFlavor::Transitional);
    assert_eq!(edits.len(), 1);
    assert!(matches!(&edits[0], NodeEdit::Replace { .. }));
    d.apply_edits(&edits);
    assert_eq!(
        xml_of(&d),
        format!(
            r#"<w:r xmlns:w="{W_T}"><w:rPr  w:x='1' ><w:b/><w:color w:val="0070C0"/><w:bdr w:val="single"  w:sz='4' w:space="0" w:color='auto'/><w:sz w:val="24"/></w:rPr><w:t>x</w:t></w:r>"#
        )
    );
    assert_eq!(d.node(rpr).dirty, Dirty::DescendantDirty, "容器只因子列表变化而变脏");
    assert!(d.check_dirty_invariants().is_ok());

    // Set 同值 → 空计划（PROP-07）
    let same =
        RunPropsPatch { color: Change::Set(blue), bold: Change::Set(true), ..Default::default() };
    assert!(plan_apply_run_props(&d, r, Some(rpr), &same, PartFlavor::Transitional).is_empty());

    // Unset → Deleted；未建模 w:bdr 仍在原位
    let unset = RunPropsPatch { bold: Change::Unset, size: Change::Unset, ..Default::default() };
    let edits = plan_apply_run_props(&d, r, Some(rpr), &unset, PartFlavor::Transitional);
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|e| matches!(e, NodeEdit::Delete(_))));
    d.apply_edits(&edits);
    assert_eq!(
        xml_of(&d),
        format!(
            r#"<w:r xmlns:w="{W_T}"><w:rPr  w:x='1' ><w:color w:val="0070C0"/><w:bdr w:val="single"  w:sz='4' w:space="0" w:color='auto'/></w:rPr><w:t>x</w:t></w:r>"#
        )
    );
    // Unset 不存在的字段、Keep → 无操作
    let noop = RunPropsPatch { italic: Change::Unset, ..Default::default() };
    assert!(plan_apply_run_props(&d, r, Some(rpr), &noop, PartFlavor::Transitional).is_empty());
}

#[test]
fn prop_06_missing_container_is_created_as_first_child() {
    let mut d = p_dom(r#"<w:r><w:t>x</w:t></w:r>"#);
    let p = d.root();
    let patch = ParaPropsPatch {
        jc: Change::Set(Val::Value(Jc::Center)),
        rpr: TableChange::Patch(RunPropsPatch { bold: Change::Set(true), ..Default::default() }),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, p, None, &patch, PartFlavor::Transitional);
    assert_eq!(edits.len(), 1, "{edits:#?}");
    d.apply_edits(&edits);
    assert_eq!(
        xml_of(&d),
        format!(
            r#"<w:p xmlns:w="{W_T}" xmlns:w14="{W14}"><w:pPr><w:jc w:val="center"/><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#
        )
    );
    // 空 patch / 只有 Unset → 不建容器
    assert!(
        plan_apply_para_props(&d, p, None, &ParaPropsPatch::default(), PartFlavor::Transitional)
            .is_empty()
    );
    let unset_only = ParaPropsPatch { jc: Change::Unset, ..Default::default() };
    assert!(plan_apply_para_props(&d, p, None, &unset_only, PartFlavor::Transitional).is_empty());
    // Strict：新容器里的 OnOff false 写 "false"
    let mut d = dom(&format!(r#"<w:r xmlns:w="{W_S}"><w:t>x</w:t></w:r>"#));
    let r = d.root();
    let patch = RunPropsPatch { bold: Change::Set(false), ..Default::default() };
    d.apply_edits(&plan_apply_run_props(&d, r, None, &patch, PartFlavor::Strict));
    assert!(xml_of(&d).contains(r#"<w:rPr><w:b w:val="false"/></w:rPr><w:t>"#), "{}", xml_of(&d));
}

#[test]
fn prop_06_nested_patch_and_sub_container_placement() {
    let mut d = p_dom(
        r#"<w:pPr><w:jc w:val="both"/><w:pPrChange w:id="1" w:author="a" w:date="2020-01-01T00:00:00Z"><w:pPr/></w:pPrChange></w:pPr>"#,
    );
    let p = d.root();
    let ppr = first_child(&d, p);
    let patch = ParaPropsPatch {
        rpr: TableChange::Patch(RunPropsPatch { italic: Change::Set(true), ..Default::default() }),
        borders: TableChange::Set(ParaBorders {
            top: Some(border(BorderStyle::Single, 4)),
            ..Default::default()
        }),
        ..Default::default()
    };
    d.apply_edits(&plan_apply_para_props(&d, p, Some(ppr), &patch, PartFlavor::Transitional));
    let out = xml_of(&d);
    assert!(
        out.contains(r#"<w:pPr><w:pBdr><w:top w:val="single" w:color="0070C0" w:sz="4" w:space="1"/></w:pBdr><w:jc w:val="both"/><w:rPr><w:i/></w:rPr><w:pPrChange"#),
        "{out}"
    );

    // 再对已存在的 rPr 打 Patch：只加 b，i 不动
    let rpr_node = d.semantic_children(ppr).find(|&n| d.is(n, QName::w(LocalName::RPr))).unwrap();
    let patch2 = ParaPropsPatch {
        rpr: TableChange::Patch(RunPropsPatch { bold: Change::Set(true), ..Default::default() }),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, p, Some(ppr), &patch2, PartFlavor::Transitional);
    assert_eq!(edits.len(), 1);
    assert!(
        matches!(&edits[0], NodeEdit::Insert { parent: Target::Node(c), before: Some(_), .. } if *c == rpr_node)
    );
    d.apply_edits(&edits);
    assert!(xml_of(&d).contains(r#"<w:rPr><w:b/><w:i/></w:rPr>"#), "{}", xml_of(&d));

    // TableChange::Set 在已有容器上按 diff 合并：只替换变化的字段
    let patch3 = ParaPropsPatch {
        rpr: TableChange::Set(RunProps {
            bold: Some(true),
            italic: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, p, Some(ppr), &patch3, PartFlavor::Transitional);
    assert_eq!(edits.len(), 1);
    assert!(matches!(&edits[0], NodeEdit::Replace { .. }));
    d.apply_edits(&edits);
    assert!(xml_of(&d).contains(r#"<w:rPr><w:b/><w:i w:val="0"/></w:rPr>"#), "{}", xml_of(&d));
    // 嵌套 Unset 删整个子容器
    let patch4 = ParaPropsPatch { rpr: TableChange::Unset, ..Default::default() };
    d.apply_edits(&plan_apply_para_props(&d, p, Some(ppr), &patch4, PartFlavor::Transitional));
    assert!(!xml_of(&d).contains("<w:rPr>"), "{}", xml_of(&d));
    assert!(d.check_dirty_invariants().is_ok());
}

#[test]
fn prop_06_multi_replaces_whole_list_in_place() {
    let mut d = p_dom(
        r#"<w:pPr><w:tabs><w:tab w:val="left" w:pos="1"/><w:tab w:val="left" w:pos="2"/></w:tabs><w:jc w:val="both"/></w:pPr>"#,
    );
    let p = d.root();
    let ppr = first_child(&d, p);
    let tab = Tab { val: Some(Val::Value(TabJc::Center)), pos: Some(Val::Value(3)), leader: None };
    let patch = ParaPropsPatch {
        tabs: TableChange::Patch(TabsPatch { tab: Change::Set(vec![tab]) }),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, p, Some(ppr), &patch, PartFlavor::Transitional);
    assert_eq!(edits.len(), 3, "{edits:#?}");
    d.apply_edits(&edits);
    assert!(
        xml_of(&d).contains(r#"<w:tabs><w:tab w:val="center" w:pos="3"/></w:tabs><w:jc"#),
        "{}",
        xml_of(&d)
    );
    // 空列表 = Unset：删掉全部 w:tab，容器保留
    let patch = ParaPropsPatch {
        tabs: TableChange::Patch(TabsPatch { tab: Change::Set(Vec::new()) }),
        ..Default::default()
    };
    d.apply_edits(&plan_apply_para_props(&d, p, Some(ppr), &patch, PartFlavor::Transitional));
    assert!(xml_of(&d).contains(r#"<w:tabs></w:tabs><w:jc"#), "{}", xml_of(&d));
}

#[test]
fn prop_06_raw_field_set_clones_subtree() {
    let mut d = dom(&format!(
        r#"<w:body xmlns:w="{W_T}"><w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p><w:p><w:pPr><w:jc w:val="both"/></w:pPr></w:p><w:p/></w:body>"#
    ));
    let body = d.root();
    let ps: Vec<NodeId> = d.semantic_children(body).collect();
    let ppr1 = first_child(&d, ps[0]);
    let ppr2 = first_child(&d, ps[1]);
    let sect = read_para_props(&d, Some(ppr1), &mut Vec::new()).sect_pr.unwrap();

    let patch = ParaPropsPatch { sect_pr: Change::Set(sect), ..Default::default() };
    let edits = plan_apply_para_props(&d, ps[1], Some(ppr2), &patch, PartFlavor::Transitional);
    assert_eq!(
        edits,
        vec![NodeEdit::InsertClone { parent: Target::Node(ppr2), before: None, source: sect }]
    );
    d.apply_edits(&edits);
    // 缺容器 + Raw：先建容器，再把克隆挂到新容器（Target::New）
    let patch = ParaPropsPatch {
        jc: Change::Set(Val::Value(Jc::Center)),
        sect_pr: Change::Set(sect),
        ..Default::default()
    };
    let edits = plan_apply_para_props(&d, ps[2], None, &patch, PartFlavor::Transitional);
    assert_eq!(edits.len(), 2);
    assert!(matches!(&edits[1], NodeEdit::InsertClone { parent: Target::New(0), .. }));
    d.apply_edits(&edits);
    let out = xml_of(&d);
    assert!(out.contains(r#"<w:p><w:pPr><w:jc w:val="both"/><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p>"#), "{out}");
    assert!(out.contains(r#"<w:p><w:pPr><w:jc w:val="center"/><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p>"#), "{out}");
    // 原件不动
    assert!(out.starts_with(&format!(r#"<w:body xmlns:w="{W_T}"><w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p>"#)), "{out}");
}

/// 与 [`para_sample`] 每个非 Raw 字段都不同的样本。
fn para_sample_alt() -> ParaProps {
    ParaProps {
        style: Some("Body".into()),
        keep_next: Some(false),
        keep_lines: Some(true),
        page_break_before: Some(false),
        frame: Some(FramePr {
            w: Some(Val::Value(5000)),
            wrap: Some(Val::Value(FrameWrap::None)),
            ..Default::default()
        }),
        widow_control: Some(true),
        num: Some(NumPr {
            ilvl: Some(Val::Value(0)),
            num_id: Some(Val::Value(1)),
            ..Default::default()
        }),
        borders: Some(ParaBorders {
            bottom: Some(border(BorderStyle::Wave, 18)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Solid)),
            fill: rgb(1, 2, 3),
            ..Default::default()
        }),
        tabs: Some(Tabs {
            tab: vec![Tab {
                val: Some(Val::Value(TabJc::Clear)),
                pos: Some(Val::Value(1)),
                leader: None,
            }],
            ..Default::default()
        }),
        auto_space_de: Some(true),
        auto_space_dn: Some(true),
        bidi: Some(false),
        snap_to_grid: Some(true),
        spacing: Some(Spacing { after: Some(Val::Value(200)), ..Default::default() }),
        indent: Some(Indent { hanging: Some(Val::Value(360)), ..Default::default() }),
        suppress_auto_hyphens: Some(true),
        contextual_spacing: Some(false),
        jc: Some(Val::Value(Jc::Start)),
        outline_lvl: Some(Val::Value(1)),
        rpr: Some(RunProps { italic: Some(true), ..Default::default() }),
        sect_pr: None,
        raw_unmodeled: Vec::new(),
    }
}

fn run_sample() -> RunProps {
    RunProps {
        style: Some("Emphasis".into()),
        fonts: Some(Fonts {
            ascii: Some("Calibri".into()),
            east_asia: Some("宋体".into()),
            ..Default::default()
        }),
        bold: Some(true),
        bold_cs: Some(true),
        italic: Some(false),
        italic_cs: Some(false),
        caps: Some(true),
        small_caps: Some(false),
        strike: Some(true),
        dstrike: Some(false),
        vanish: Some(true),
        color: Some(Color { val: rgb(0xFF, 0, 0), ..Default::default() }),
        spacing: Some(Val::Value(20)),
        scale: Some(Val::Value(90)),
        kern: Some(Val::Value(2)),
        position: Some(Val::Value(-4)),
        size: Some(Val::Value(24)),
        size_cs: Some(Val::Value(24)),
        highlight: Some(Val::Value(HighlightColor::Yellow)),
        underline: Some(Underline {
            val: Some(Val::Value(UnderlineKind::Single)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            fill: rgb(0xEE, 0xEE, 0xEE),
            ..Default::default()
        }),
        vert_align: Some(Val::Value(VerticalAlignRun::Superscript)),
        rtl: Some(false),
        cs: Some(false),
        em: Some(Val::Value(EmphasisMark::Dot)),
        lang: Some(Language { val: Some("en-US".into()), ..Default::default() }),
        spec_vanish: Some(false),
        text_fill: None,
        raw_unmodeled: Vec::new(),
    }
}

fn run_sample_alt() -> RunProps {
    RunProps {
        style: Some("Strong".into()),
        fonts: Some(Fonts { h_ansi: Some("Arial".into()), ..Default::default() }),
        bold: Some(false),
        bold_cs: Some(false),
        italic: Some(true),
        italic_cs: Some(true),
        caps: Some(false),
        small_caps: Some(true),
        strike: Some(false),
        dstrike: Some(true),
        vanish: Some(false),
        color: Some(Color {
            theme_color: Some(Val::Value(ThemeColor::Accent1)),
            ..Default::default()
        }),
        spacing: Some(Val::Value(-10)),
        scale: Some(Val::Value(200)),
        kern: Some(Val::Value(28)),
        position: Some(Val::Value(6)),
        size: Some(Val::Value(36)),
        size_cs: Some(Val::Value(32)),
        highlight: Some(Val::Value(HighlightColor::Green)),
        underline: Some(Underline {
            val: Some(Val::Value(UnderlineKind::Double)),
            color: rgb(0, 0, 0xFF),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Pct10)),
            ..Default::default()
        }),
        vert_align: Some(Val::Value(VerticalAlignRun::Subscript)),
        rtl: Some(true),
        cs: Some(true),
        em: Some(Val::Value(EmphasisMark::Circle)),
        lang: Some(Language { east_asia: Some("zh-CN".into()), ..Default::default() }),
        spec_vanish: Some(true),
        text_fill: None,
        raw_unmodeled: Vec::new(),
    }
}

/// PROP-07 每行：Set 同值 → 空计划；Set 新值 → commit → read 得新值；每个非 Raw 字段都参与。
#[test]
fn prop_07_plan_apply_every_row_same_value_empty_new_value_commits() {
    // ParaProps
    let (a, b) = (para_sample(), para_sample_alt());
    for flavor in [PartFlavor::Transitional, PartFlavor::Strict] {
        let ns = if flavor == PartFlavor::Strict { W_S } else { W_T };
        let mut d =
            dom(&format!(r#"<w:p xmlns:w="{ns}" xmlns:w14="{W14}"><w:r><w:t>x</w:t></w:r></w:p>"#));
        let p = d.root();
        let set_all = diff_para_props(&ParaProps::default(), &a);
        d.apply_edits(&plan_apply_para_props(&d, p, None, &set_all, flavor));
        let ppr = first_child(&d, p);
        assert!(d.is(ppr, QName::w(LocalName::PPr)));
        let mut diags = Vec::new();
        assert_eq!(read_para_props(&d, Some(ppr), &mut diags), a);
        assert!(diags.is_empty(), "{diags:?}");
        assert!(
            plan_apply_para_props(&d, p, Some(ppr), &set_all, flavor).is_empty(),
            "Set 同值应为空计划"
        );

        let to_b = diff_para_props(&a, &b);
        for f in ParaPropsField::ALL {
            if f.info().kind != FieldKind::Raw {
                assert_ne!(to_b.kind(*f), ChangeKind::Keep, "样本在字段 {} 上相同", f.info().name);
            }
        }
        let edits = plan_apply_para_props(&d, p, Some(ppr), &to_b, flavor);
        assert!(edits.len() >= ParaPropsField::ALL.len() - 1, "{}", edits.len());
        d.apply_edits(&edits);
        assert_eq!(read_para_props(&d, Some(ppr), &mut Vec::new()), b, "{}", xml_of(&d));
        assert!(d.check_dirty_invariants().is_ok());
        // 顺序仍单调
        let mut last = 0;
        for c in d.semantic_children(ppr) {
            let i = order_index_para_props(d.name(c).unwrap()).unwrap();
            assert!(i >= last, "{}", xml_of(&d));
            last = i;
        }
    }

    // RunProps
    let (a, b) = (run_sample(), run_sample_alt());
    let mut d = dom(&format!(r#"<w:r xmlns:w="{W_T}"><w:t>x</w:t></w:r>"#));
    let r = d.root();
    let set_all = diff_run_props(&RunProps::default(), &a);
    d.apply_edits(&plan_apply_run_props(&d, r, None, &set_all, PartFlavor::Transitional));
    let rpr = first_child(&d, r);
    assert_eq!(read_run_props(&d, Some(rpr), &mut Vec::new()), a);
    assert!(plan_apply_run_props(&d, r, Some(rpr), &set_all, PartFlavor::Transitional).is_empty());
    let to_b = diff_run_props(&a, &b);
    for f in RunPropsField::ALL {
        if f.info().kind != FieldKind::Raw {
            assert_ne!(to_b.kind(*f), ChangeKind::Keep, "样本在字段 {} 上相同", f.info().name);
        }
    }
    let edits = plan_apply_run_props(&d, r, Some(rpr), &to_b, PartFlavor::Transitional);
    assert_eq!(edits.len(), RunPropsField::ALL.len() - 1, "每个字段一次 Replace");
    assert!(edits.iter().all(|e| matches!(e, NodeEdit::Replace { .. })));
    d.apply_edits(&edits);
    assert_eq!(read_run_props(&d, Some(rpr), &mut Vec::new()), b);
}

// ---- 表格属性表（任务 3.1：PROP-02 / 05 / 06 / 07 / 08）--------------------------------------------

/// 用 `inner` 作 `w:tcPr` 的内容解析。
fn tcpr(inner: &str) -> (Dom, CellProps, Vec<Diagnostic>) {
    let d = dom(&format!(r#"<w:tcPr xmlns:w="{W_T}">{inner}</w:tcPr>"#));
    let mut diags = Vec::new();
    let p = read_cell_props(&d, Some(d.root()), &mut diags);
    (d, p, diags)
}

fn trpr(inner: &str) -> (Dom, RowProps, Vec<Diagnostic>) {
    let d = dom(&format!(r#"<w:trPr xmlns:w="{W_T}">{inner}</w:trPr>"#));
    let mut diags = Vec::new();
    let p = read_row_props(&d, Some(d.root()), &mut diags);
    (d, p, diags)
}

fn tblpr(inner: &str) -> (Dom, TableProps, Vec<Diagnostic>) {
    let d = dom(&format!(r#"<w:tblPr xmlns:w="{W_T}">{inner}</w:tblPr>"#));
    let mut diags = Vec::new();
    let p = read_table_props(&d, Some(d.root()), &mut diags);
    (d, p, diags)
}

#[test]
fn prop_02_tbl_width_measure_and_helpers() {
    let (_, p, diags) = tblpr(
        r#"<w:tblW w:w="2500" w:type="pct"/><w:tblInd w:w="1in" w:type="dxa"/><w:tblCellSpacing w:w="50%"/>"#,
    );
    assert!(diags.is_empty(), "{diags:?}");
    let w = p.width.as_ref().unwrap();
    assert_eq!(w.w, Some(Val::Value(Measure::Number(2500))));
    assert_eq!(w.kind, Some(Val::Value(TblWidthType::Pct)));
    assert_eq!(w.percent(), Some(50.0), "pct 的数是 1/50 百分点");
    assert_eq!(w.twips(), None);
    let ind = p.indent.as_ref().unwrap();
    assert_eq!(ind.twips(), Some(1440), "带单位的度量换算成 twips");
    assert_eq!(ind.percent(), None);
    let sp = p.cell_spacing.as_ref().unwrap();
    assert_eq!(sp.w, Some(Val::Value(Measure::Percent(5000))));
    assert_eq!(sp.kind, None);
    assert_eq!(sp.percent(), Some(50.0), "字面 NN% 不看 type");
    assert_eq!(TblWidth::dxa(1200).twips(), Some(1200));
    assert_eq!(TblWidth::pct(50.0).w, Some(Val::Value(Measure::Number(2500))));
    assert_eq!(TblWidth::pct(50.0).percent(), Some(50.0));
    assert_eq!(TblWidth::auto().twips(), None);
    // 写回：1in → 1440；百分数字面原样
    let out = to_xml(&emit_table_props(&p, PartFlavor::Transitional), W_T);
    assert!(out.contains(r#"<w:tblW w:w="2500" w:type="pct"/>"#), "{out}");
    assert!(out.contains(r#"<w:tblInd w:w="1440" w:type="dxa"/>"#), "{out}");
    assert!(out.contains(r#"<w:tblCellSpacing w:w="50%"/>"#), "{out}");
    // 坏值保留原文
    let (_, q, diags) = tblpr(r#"<w:tblW w:w="wide" w:type="dxa"/>"#);
    assert_eq!(q.width.as_ref().unwrap().w, Some(Val::Raw("wide".into())));
    assert_eq!(diags.len(), 1);
}

#[test]
fn prop_08_cell_props_read_emit_and_change() {
    let (d, p, diags) = tcpr(
        r#"<w:cnfStyle w:val="100000000000" w:firstRow="1"/><w:tcW w:w="2400" w:type="dxa"/><w:gridSpan w:val="2"/>
           <w:hMerge w:val="restart"/><w:vMerge/>
           <w:tcBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="nil"/><w:tl2br w:val="dashed" w:sz="8"/></w:tcBorders>
           <w:shd w:val="clear" w:color="auto" w:fill="1F3864"/><w:noWrap/>
           <w:tcMar><w:left w:w="100" w:type="dxa"/><w:end w:w="5%"/></w:tcMar>
           <w:textDirection w:val="tbRlV"/><w:tcFitText w:val="0"/><w:vAlign w:val="center"/><w:hideMark/>
           <w:headers><w:header w:val="h1"/></w:headers>
           <w:cellIns w:id="3" w:author="a" w:date="2020-01-01T00:00:00Z"/>
           <w:cellMerge w:id="4" w:author="a" w:vMerge="cont" w:vMergeOrig="rest"/>
           <w:tcPrChange w:id="9" w:author="a"><w:tcPr><w:vAlign w:val="bottom"/></w:tcPr></w:tcPrChange>"#,
    );
    assert!(diags.is_empty(), "{diags:?}");
    let cnf = p.cnf_style.as_ref().unwrap();
    assert_eq!(cnf.val.as_deref(), Some("100000000000"));
    assert_eq!(cnf.first_row, Some(true));
    assert_eq!(cnf.last_row, None);
    assert_eq!(p.width.as_ref().unwrap().twips(), Some(2400));
    assert_eq!(p.grid_span, Some(Val::Value(2)));
    assert!(p.h_merge.as_ref().unwrap().is_restart());
    let vm = p.v_merge.as_ref().unwrap();
    assert_eq!(vm.val, None, "裸 vMerge = continue");
    assert!(!vm.is_restart());
    let b = p.borders.as_ref().unwrap();
    assert_eq!(b.top.as_ref().unwrap().sz, Some(Val::Value(4)));
    assert_eq!(
        b.start.as_ref().unwrap().val,
        Some(Val::Value(BorderStyle::Nil)),
        "w:left 读进 start"
    );
    assert_eq!(b.tl2br.as_ref().unwrap().val, Some(Val::Value(BorderStyle::Dashed)));
    assert!(b.end.is_none() && b.inside_h.is_none() && b.tr2bl.is_none());
    assert_eq!(p.shading.as_ref().unwrap().fill, rgb(0x1F, 0x38, 0x64));
    assert_eq!(p.no_wrap, Some(true));
    let m = p.margins.as_ref().unwrap();
    assert_eq!(m.start.as_ref().unwrap().twips(), Some(100));
    assert_eq!(m.end.as_ref().unwrap().percent(), Some(5.0));
    assert!(m.top.is_none());
    assert_eq!(p.text_direction, Some(Val::Value(TextDirection::TbRlV)));
    assert_eq!(p.fit_text, Some(false));
    assert_eq!(p.v_align, Some(Val::Value(VerticalJc::Center)));
    assert_eq!(p.hide_mark, Some(true));
    assert!(d.is(p.headers.unwrap(), QName::w(LocalName::Headers)), "headers 是 Raw 字段");
    let ci = p.cell_ins.as_ref().unwrap();
    assert_eq!(ci.id, Some(Val::Value(3)));
    assert_eq!(ci.author.as_deref(), Some("a"));
    assert!(p.cell_del.is_none());
    let cm = p.cell_merge.as_ref().unwrap();
    assert_eq!(cm.v_merge, Some(Val::Value(AnnotationVMerge::Cont)));
    assert_eq!(cm.v_merge_orig, Some(Val::Value(AnnotationVMerge::Rest)));
    assert_eq!(p.raw_unmodeled.len(), 1);
    assert!(d.is(p.raw_unmodeled[0], QName::w(LocalName::TcPrChange)));
    let (_, old) = read_cell_props_change(&d, Some(d.root()), &mut Vec::new()).unwrap();
    assert_eq!(old.v_align, Some(Val::Value(VerticalJc::Bottom)));
    assert_eq!(old.grid_span, None);
    for f in [
        CellPropsField::Headers,
        CellPropsField::CellIns,
        CellPropsField::CellDel,
        CellPropsField::CellMerge,
    ] {
        assert!(!f.info().in_change, "{}", f.info().name);
    }

    // 写回：Transitional 用 left/right，Strict 用 start/end；裸 vMerge 保持裸；hMerge restart 带 val
    let t = to_xml(&emit_cell_props(&p, PartFlavor::Transitional), W_T);
    assert!(
        t.contains(
            r#"<w:hMerge w:val="restart"/><w:vMerge/><w:tcBorders><w:top w:val="single" w:color="auto" w:sz="4" w:space="0"/><w:left w:val="nil"/><w:tl2br w:val="dashed" w:sz="8"/></w:tcBorders>"#
        ),
        "{t}"
    );
    assert!(
        t.contains(r#"<w:tcMar><w:left w:w="100" w:type="dxa"/><w:right w:w="5%"/></w:tcMar>"#),
        "{t}"
    );
    assert!(t.contains(r#"<w:tcFitText w:val="0"/>"#), "{t}");
    assert!(!t.contains("w:headers"), "Raw 字段不由 emit 生成：{t}");
    let s = to_xml(&emit_cell_props(&p, PartFlavor::Strict), W_S);
    assert!(s.contains(r#"<w:start w:val="nil"/>"#) && s.contains(r#"<w:end w:w="5%"/>"#), "{s}");
    assert!(!s.contains("w:left") && !s.contains("w:right"), "{s}");
    assert!(s.contains(r#"<w:tcFitText w:val="false"/>"#), "{s}");
}

#[test]
fn prop_08_row_props_read_and_change() {
    let (d, p, diags) = trpr(
        r#"<w:cnfStyle w:val="000000100000"/><w:divId w:val="1"/><w:gridBefore w:val="1"/><w:gridAfter w:val="2"/>
           <w:wBefore w:w="1200" w:type="dxa"/><w:wAfter w:w="0" w:type="auto"/><w:cantSplit/>
           <w:trHeight w:val="400" w:hRule="exact"/><w:tblHeader/><w:tblCellSpacing w:w="15" w:type="dxa"/>
           <w:jc w:val="center"/><w:hidden w:val="0"/>
           <w:ins w:id="7" w:author="b" w:date="2021-02-02T00:00:00Z"/>
           <w:trPrChange w:id="8" w:author="b"><w:trPr><w:cantSplit w:val="0"/></w:trPr></w:trPrChange>"#,
    );
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(p.cnf_style.as_ref().unwrap().val.as_deref(), Some("000000100000"));
    assert_eq!(p.grid_before, Some(Val::Value(1)));
    assert_eq!(p.grid_after, Some(Val::Value(2)));
    assert_eq!(p.width_before.as_ref().unwrap().twips(), Some(1200));
    assert_eq!(p.width_after.as_ref().unwrap().kind, Some(Val::Value(TblWidthType::Auto)));
    assert_eq!(p.cant_split, Some(true));
    let h = p.height.as_ref().unwrap();
    assert_eq!(h.val, Some(Val::Value(400)));
    assert_eq!(h.h_rule, Some(Val::Value(HeightRule::Exact)));
    assert_eq!(p.tbl_header, Some(true));
    assert_eq!(p.cell_spacing.as_ref().unwrap().twips(), Some(15));
    assert_eq!(p.jc, Some(Val::Value(JcTable::Center)));
    assert_eq!(p.hidden, Some(false));
    assert_eq!(p.ins.as_ref().unwrap().author.as_deref(), Some("b"));
    assert!(p.del.is_none());
    // divId 未建模、trPrChange 不是字段 → raw_unmodeled
    assert_eq!(p.raw_unmodeled.len(), 2);
    let (_, old) = read_row_props_change(&d, Some(d.root()), &mut Vec::new()).unwrap();
    assert_eq!(old.cant_split, Some(false));
    assert!(!RowPropsField::Ins.info().in_change && !RowPropsField::Del.info().in_change);
    // Strict 下 jc 的 left/right 字面照样保留（PROP-09：不替调用方换字面）
    let t = to_xml(&emit_row_props(&p, PartFlavor::Strict), W_S);
    assert!(t.contains(r#"<w:trHeight w:val="400" w:hRule="exact"/><w:tblHeader/>"#), "{t}");
    assert!(t.contains(r#"<w:hidden w:val="false"/>"#), "{t}");
}

#[test]
fn prop_08_table_props_read_tbl_pr_ex_and_typed_style() {
    let (d, p, diags) = tblpr(
        r#"<w:tblStyle w:val="TableGrid"/>
           <w:tblpPr w:leftFromText="180" w:rightFromText="180" w:vertAnchor="text" w:horzAnchor="margin" w:tblpXSpec="right" w:tblpY="1"/>
           <w:tblOverlap w:val="never"/><w:bidiVisual/><w:tblStyleRowBandSize w:val="1"/><w:tblStyleColBandSize w:val="2"/>
           <w:tblW w:w="0" w:type="auto"/><w:jc w:val="center"/><w:tblCellSpacing w:w="20" w:type="dxa"/><w:tblInd w:w="-115" w:type="dxa"/>
           <w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders>
           <w:shd w:val="clear" w:color="auto" w:fill="F2F2F2"/><w:tblLayout w:type="fixed"/>
           <w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar>
           <w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>
           <w:tblCaption w:val="cap"/><w:tblDescription w:val="desc"/>
           <w:tblPrChange w:id="1" w:author="c"><w:tblPr><w:jc w:val="left"/></w:tblPr></w:tblPrChange>"#,
    );
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(p.style.as_deref(), Some("TableGrid"));
    let pos = p.position.as_ref().unwrap();
    assert_eq!(pos.tblp_x_spec, Some(Val::Value(XAlign::Right)));
    assert_eq!(pos.horz_anchor, Some(Val::Value(FrameAnchor::Margin)));
    assert_eq!(pos.vert_anchor, Some(Val::Value(FrameAnchor::Text)));
    assert_eq!(pos.left_from_text, Some(Val::Value(180)));
    assert_eq!(pos.tblp_y, Some(Val::Value(1)));
    assert!(pos.tblp_x.is_none() && pos.tblp_y_spec.is_none());
    assert_eq!(p.overlap, Some(Val::Value(TblOverlap::Never)));
    assert_eq!(p.bidi_visual, Some(true));
    assert_eq!(p.style_row_band_size, Some(Val::Value(1)));
    assert_eq!(p.style_col_band_size, Some(Val::Value(2)));
    assert_eq!(p.width.as_ref().unwrap().kind, Some(Val::Value(TblWidthType::Auto)));
    assert_eq!(p.width.as_ref().unwrap().twips(), None, "auto 不是绝对宽");
    assert_eq!(p.jc, Some(Val::Value(JcTable::Center)));
    assert_eq!(p.cell_spacing.as_ref().unwrap().twips(), Some(20));
    assert_eq!(p.indent.as_ref().unwrap().twips(), Some(-115));
    let b = p.borders.as_ref().unwrap();
    assert!(b.top.is_some() && b.start.is_some() && b.bottom.is_some() && b.end.is_some());
    assert!(b.inside_h.is_some() && b.inside_v.is_some());
    assert_eq!(p.shading.as_ref().unwrap().fill, rgb(0xF2, 0xF2, 0xF2));
    assert_eq!(p.layout.as_ref().unwrap().kind, Some(Val::Value(TblLayoutType::Fixed)));
    let m = p.cell_margins.as_ref().unwrap();
    assert_eq!(m.start.as_ref().unwrap().twips(), Some(108));
    assert_eq!(m.top.as_ref().unwrap().twips(), Some(0));
    let look = p.look.as_ref().unwrap();
    assert_eq!(look.val.as_deref(), Some("04A0"));
    assert_eq!(look.first_row, Some(true));
    assert_eq!(look.last_row, Some(false));
    assert_eq!(look.no_v_band, Some(true));
    assert_eq!(p.caption.as_deref(), Some("cap"));
    assert_eq!(p.description.as_deref(), Some("desc"));
    assert_eq!(p.raw_unmodeled.len(), 1);
    let (_, old) = read_table_props_change(&d, Some(d.root()), &mut Vec::new()).unwrap();
    assert_eq!(old.jc, Some(Val::Value(JcTable::Left)));
    // tblLayout 的属性是 w:type；tblLook 属性按声明顺序写回
    let t = to_xml(&emit_table_props(&p, PartFlavor::Transitional), W_T);
    assert!(t.contains(r#"<w:tblLayout w:type="fixed"/>"#), "{t}");
    assert!(
        t.contains(
            r#"<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>"#
        ),
        "{t}"
    );
    assert!(t.contains(r#"<w:tblpPr w:leftFromText="180" w:rightFromText="180" w:vertAnchor="text" w:horzAnchor="margin" w:tblpXSpec="right" w:tblpY="1"/>"#), "{t}");

    // 同一张表读 w:tblPrEx；tblPrExChange 与 tblPrChange 同序号（PROP-05 CT_TblPrEx）
    let d2 = dom(&format!(
        r#"<w:tblPrEx xmlns:w="{W_T}"><w:tblW w:w="5000" w:type="pct"/><w:jc w:val="end"/><w:tblPrExChange w:id="2" w:author="c"><w:tblPrEx/></w:tblPrExChange></w:tblPrEx>"#
    ));
    let ex = read_table_props(&d2, Some(d2.root()), &mut Vec::new());
    assert_eq!(ex.width.as_ref().unwrap().percent(), Some(100.0));
    assert_eq!(ex.jc, Some(Val::Value(JcTable::End)));
    assert_eq!(ex.raw_unmodeled.len(), 1);
    assert_eq!(
        order_index_table_props(QName::w(LocalName::TblPrExChange)),
        order_index_table_props(QName::w(LocalName::TblPrChange))
    );
    assert!(
        order_index_table_props(QName::w(LocalName::TblLook))
            < order_index_table_props(QName::w(LocalName::TblPrChange))
    );
    assert_eq!(TABLE_PROPS.change, Some(QName::w(LocalName::TblPrChange)));

    // 表格样式的三个容器有类型（styles.toml 不再是 Raw）
    let d3 = dom(&format!(
        r#"<w:style xmlns:w="{W_T}" w:type="table" w:styleId="T1"><w:name w:val="T1"/>
           <w:tblPr><w:tblBorders><w:insideH w:val="single" w:sz="4"/></w:tblBorders><w:tblCellMar><w:left w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr>
           <w:trPr><w:tblHeader/></w:trPr><w:tcPr><w:shd w:val="clear" w:fill="D9D9D9"/></w:tcPr>
           <w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="4472C4"/></w:tcPr></w:tblStylePr></w:style>"#
    ));
    let st = read_style(&d3, Some(d3.root()), &mut Vec::new());
    let tbl = st.tbl_pr.as_ref().unwrap();
    assert_eq!(tbl.borders.as_ref().unwrap().inside_h.as_ref().unwrap().sz, Some(Val::Value(4)));
    assert_eq!(tbl.cell_margins.as_ref().unwrap().start.as_ref().unwrap().twips(), Some(108));
    assert_eq!(st.tr_pr.as_ref().unwrap().tbl_header, Some(true));
    assert_eq!(st.tc_pr.as_ref().unwrap().shading.as_ref().unwrap().fill, rgb(0xD9, 0xD9, 0xD9));
    let first_row = &st.conditional[0];
    assert_eq!(first_row.kind, Some(Val::Value(TblStyleOverrideType::FirstRow)));
    assert_eq!(
        first_row.tc_pr.as_ref().unwrap().shading.as_ref().unwrap().fill,
        rgb(0x44, 0x72, 0xC4)
    );
    assert_eq!(first_row.rpr.as_ref().unwrap().bold, Some(true));
}

fn tw(twips: i32) -> TblWidth {
    TblWidth::dxa(twips)
}

fn mark(id: i32, author: &str, date: Option<&str>) -> TrackChangeMark {
    TrackChangeMark {
        id: Some(Val::Value(id)),
        author: Some(author.into()),
        date: date.map(str::to_owned),
    }
}

/// 覆盖 `CellProps` 全部非 Raw 字段的样本。
fn cell_sample() -> CellProps {
    CellProps {
        cnf_style: Some(CnfStyle {
            val: Some("100000000000".into()),
            first_row: Some(true),
            ..Default::default()
        }),
        width: Some(tw(2400)),
        grid_span: Some(Val::Value(2)),
        h_merge: Some(Merge::restart()),
        v_merge: Some(Merge::cont()),
        borders: Some(TcBorders {
            top: Some(border(BorderStyle::Single, 4)),
            start: Some(border(BorderStyle::Nil, 0)),
            bottom: Some(border(BorderStyle::Double, 6)),
            end: Some(border(BorderStyle::Dashed, 8)),
            inside_h: Some(border(BorderStyle::Dotted, 2)),
            inside_v: Some(border(BorderStyle::Thick, 12)),
            tl2br: Some(border(BorderStyle::Wave, 4)),
            tr2bl: Some(border(BorderStyle::Triple, 4)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            fill: rgb(0x1F, 0x38, 0x64),
            ..Default::default()
        }),
        no_wrap: Some(true),
        margins: Some(TcMar {
            top: Some(tw(0)),
            start: Some(tw(100)),
            bottom: Some(tw(0)),
            end: Some(TblWidth::pct(5.0)),
            ..Default::default()
        }),
        text_direction: Some(Val::Value(TextDirection::TbRlV)),
        fit_text: Some(false),
        v_align: Some(Val::Value(VerticalJc::Center)),
        hide_mark: Some(true),
        headers: None,
        cell_ins: Some(mark(3, "a", Some("2020-01-01T00:00:00Z"))),
        cell_del: Some(mark(4, "b", None)),
        cell_merge: Some(CellMergeMark {
            id: Some(Val::Value(5)),
            author: Some("c".into()),
            date: None,
            v_merge: Some(Val::Value(AnnotationVMerge::Cont)),
            v_merge_orig: Some(Val::Value(AnnotationVMerge::Rest)),
        }),
        raw_unmodeled: Vec::new(),
    }
}

/// 与 [`cell_sample`] 每个非 Raw 字段都不同的样本。
fn cell_sample_alt() -> CellProps {
    CellProps {
        cnf_style: Some(CnfStyle {
            val: Some("000000000001".into()),
            last_row_last_column: Some(true),
            ..Default::default()
        }),
        width: Some(TblWidth::pct(25.0)),
        grid_span: Some(Val::Value(3)),
        h_merge: Some(Merge::cont()),
        v_merge: Some(Merge::restart()),
        borders: Some(TcBorders {
            top: Some(border(BorderStyle::Double, 12)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Pct10)),
            fill: rgb(0xFF, 0xFF, 0x00),
            ..Default::default()
        }),
        no_wrap: Some(false),
        margins: Some(TcMar { end: Some(tw(50)), ..Default::default() }),
        text_direction: Some(Val::Value(TextDirection::BtLr)),
        fit_text: Some(true),
        v_align: Some(Val::Value(VerticalJc::Bottom)),
        hide_mark: Some(false),
        headers: None,
        cell_ins: Some(mark(30, "x", None)),
        cell_del: Some(mark(40, "y", Some("2022-03-03T00:00:00Z"))),
        cell_merge: Some(CellMergeMark {
            id: Some(Val::Value(50)),
            author: Some("z".into()),
            date: Some("2022-03-03T00:00:00Z".into()),
            v_merge: Some(Val::Value(AnnotationVMerge::Rest)),
            v_merge_orig: None,
        }),
        raw_unmodeled: Vec::new(),
    }
}

fn row_sample() -> RowProps {
    RowProps {
        cnf_style: Some(CnfStyle {
            val: Some("000000100000".into()),
            odd_h_band: Some(true),
            ..Default::default()
        }),
        grid_before: Some(Val::Value(1)),
        grid_after: Some(Val::Value(2)),
        width_before: Some(tw(1200)),
        width_after: Some(TblWidth::auto()),
        cant_split: Some(true),
        height: Some(TrHeight {
            val: Some(Val::Value(400)),
            h_rule: Some(Val::Value(HeightRule::Exact)),
        }),
        tbl_header: Some(true),
        cell_spacing: Some(tw(15)),
        jc: Some(Val::Value(JcTable::Center)),
        hidden: Some(false),
        ins: Some(mark(7, "b", Some("2021-02-02T00:00:00Z"))),
        del: Some(mark(8, "c", None)),
        raw_unmodeled: Vec::new(),
    }
}

fn row_sample_alt() -> RowProps {
    RowProps {
        cnf_style: Some(CnfStyle {
            val: Some("000001000000".into()),
            even_h_band: Some(true),
            ..Default::default()
        }),
        grid_before: Some(Val::Value(0)),
        grid_after: Some(Val::Value(1)),
        width_before: Some(TblWidth::pct(10.0)),
        width_after: Some(tw(500)),
        cant_split: Some(false),
        height: Some(TrHeight {
            val: Some(Val::Value(200)),
            h_rule: Some(Val::Value(HeightRule::AtLeast)),
        }),
        tbl_header: Some(false),
        cell_spacing: Some(tw(0)),
        jc: Some(Val::Value(JcTable::Start)),
        hidden: Some(true),
        ins: Some(mark(70, "p", None)),
        del: Some(mark(80, "q", Some("2023-04-04T00:00:00Z"))),
        raw_unmodeled: Vec::new(),
    }
}

fn table_sample() -> TableProps {
    TableProps {
        style: Some("TableGrid".into()),
        position: Some(TblpPr {
            left_from_text: Some(Val::Value(180)),
            right_from_text: Some(Val::Value(180)),
            vert_anchor: Some(Val::Value(FrameAnchor::Text)),
            horz_anchor: Some(Val::Value(FrameAnchor::Margin)),
            tblp_x_spec: Some(Val::Value(XAlign::Right)),
            tblp_y: Some(Val::Value(1)),
            ..Default::default()
        }),
        overlap: Some(Val::Value(TblOverlap::Never)),
        bidi_visual: Some(true),
        style_row_band_size: Some(Val::Value(1)),
        style_col_band_size: Some(Val::Value(2)),
        width: Some(TblWidth::auto()),
        jc: Some(Val::Value(JcTable::Center)),
        cell_spacing: Some(tw(20)),
        indent: Some(tw(-115)),
        borders: Some(TblBorders {
            top: Some(border(BorderStyle::Single, 4)),
            start: Some(border(BorderStyle::Single, 4)),
            bottom: Some(border(BorderStyle::Single, 4)),
            end: Some(border(BorderStyle::Single, 4)),
            inside_h: Some(border(BorderStyle::Single, 4)),
            inside_v: Some(border(BorderStyle::Single, 4)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            fill: rgb(0xF2, 0xF2, 0xF2),
            ..Default::default()
        }),
        layout: Some(TblLayout { kind: Some(Val::Value(TblLayoutType::Fixed)) }),
        cell_margins: Some(TblCellMar {
            top: Some(tw(0)),
            start: Some(tw(108)),
            bottom: Some(tw(0)),
            end: Some(tw(108)),
            ..Default::default()
        }),
        look: Some(TblLook {
            val: Some("04A0".into()),
            first_row: Some(true),
            last_row: Some(false),
            first_column: Some(true),
            last_column: Some(false),
            no_h_band: Some(false),
            no_v_band: Some(true),
        }),
        caption: Some("cap".into()),
        description: Some("desc".into()),
        raw_unmodeled: Vec::new(),
    }
}

fn table_sample_alt() -> TableProps {
    TableProps {
        style: Some("T2".into()),
        position: Some(TblpPr {
            tblp_x: Some(Val::Value(100)),
            tblp_y: Some(Val::Value(200)),
            vert_anchor: Some(Val::Value(FrameAnchor::Page)),
            horz_anchor: Some(Val::Value(FrameAnchor::Page)),
            tblp_y_spec: Some(Val::Value(YAlign::Center)),
            ..Default::default()
        }),
        overlap: Some(Val::Value(TblOverlap::Overlap)),
        bidi_visual: Some(false),
        style_row_band_size: Some(Val::Value(3)),
        style_col_band_size: Some(Val::Value(4)),
        width: Some(TblWidth::pct(100.0)),
        jc: Some(Val::Value(JcTable::End)),
        cell_spacing: Some(tw(0)),
        indent: Some(tw(0)),
        borders: Some(TblBorders {
            inside_h: Some(border(BorderStyle::Nil, 0)),
            ..Default::default()
        }),
        shading: Some(Shading {
            val: Some(Val::Value(ShadingPattern::Solid)),
            fill: rgb(0x00, 0x00, 0xFF),
            ..Default::default()
        }),
        layout: Some(TblLayout { kind: Some(Val::Value(TblLayoutType::Autofit)) }),
        cell_margins: Some(TblCellMar { top: Some(tw(50)), ..Default::default() }),
        look: Some(TblLook {
            val: Some("0000".into()),
            first_row: Some(false),
            ..Default::default()
        }),
        caption: Some("c2".into()),
        description: Some("d2".into()),
        raw_unmodeled: Vec::new(),
    }
}

/// `PROP-07` 的两条验收，对一张表一次跑完（三张表格表同形，收成宏）：
/// 1. 样本覆盖全部非 Raw 字段，两种 flavor 下 emit → read 全等、子元素顺序单调；
/// 2. `plan_apply` 在缺容器的 `parent` 上建容器为第一个子元素、`Set` 同值空计划、样本 a → b 每个
///    字段都被改到、之后顺序仍单调、脏标记不变式成立。
///
/// ```ignore
/// check_table_rows!("w:tc", CELL_PROPS, CellPropsField, CellProps, read_cell_props, emit_cell_props,
///     emit_cell_props_value, diff_cell_props, plan_apply_cell_props, order_index_cell_props,
///     cell_sample(), cell_sample_alt());
/// ```
macro_rules! check_table_rows {
    ($parent:literal, $info:ident, $Field:ident, $Props:ident, $read:ident, $emit:ident, $emit_value:ident,
     $diff:ident, $plan:ident, $order:ident, $a:expr, $b:expr) => {{
        let (a, b): ($Props, $Props) = ($a, $b);
        let to_b = $diff(&a, &b);
        for f in $Field::ALL {
            if f.info().kind == FieldKind::Raw {
                continue;
            }
            assert!(
                !$emit_value(&a, *f, PartFlavor::Transitional).is_empty(),
                "{} 样本缺字段 {}",
                stringify!($Props),
                f.info().name
            );
            assert_ne!(
                to_b.kind(*f),
                ChangeKind::Keep,
                "{} 样本在字段 {} 上相同",
                stringify!($Props),
                f.info().name
            );
        }
        for flavor in [PartFlavor::Transitional, PartFlavor::Strict] {
            let ns = if flavor == PartFlavor::Strict { W_S } else { W_T };
            let xml = to_xml(&$emit(&a, flavor), ns);
            let d = dom(&format!(r#"<{p} xmlns:w="{ns}">{xml}</{p}>"#, p = $parent));
            let container = d.semantic_children(d.root()).next().unwrap();
            let mut diags = Vec::new();
            let back = $read(&d, Some(container), &mut diags);
            assert!(diags.is_empty(), "{}: {flavor:?}: {diags:?}", stringify!($Props));
            assert_eq!(back, a, "{}: {flavor:?}\n{xml}", stringify!($Props));
            let mut last = 0;
            for c in d.semantic_children(container) {
                let i = $order(d.name(c).unwrap()).unwrap();
                assert!(i >= last, "{xml}");
                last = i;
            }

            let mut d = dom(&format!(r#"<{p} xmlns:w="{ns}"><w:p/></{p}>"#, p = $parent));
            let parent = d.root();
            let set_all = $diff(&$Props::default(), &a);
            d.apply_edits(&$plan(&d, parent, None, &set_all, flavor));
            let c = first_child(&d, parent);
            assert!(d.is(c, $info.element), "{}", xml_of(&d));
            assert_eq!($read(&d, Some(c), &mut Vec::new()), a);
            assert!($plan(&d, parent, Some(c), &set_all, flavor).is_empty(), "Set 同值应为空计划");
            let edits = $plan(&d, parent, Some(c), &to_b, flavor);
            assert!(!edits.is_empty());
            d.apply_edits(&edits);
            assert_eq!($read(&d, Some(c), &mut Vec::new()), b, "{}", xml_of(&d));
            assert!(d.check_dirty_invariants().is_ok());
            let mut last = 0;
            for k in d.semantic_children(c) {
                let i = $order(d.name(k).unwrap()).unwrap();
                assert!(i >= last, "{}", xml_of(&d));
                last = i;
            }
        }
    }};
}

#[test]
fn prop_07_every_cell_props_row_roundtrips_and_plans() {
    check_table_rows!(
        "w:tc",
        CELL_PROPS,
        CellPropsField,
        CellProps,
        read_cell_props,
        emit_cell_props,
        emit_cell_props_value,
        diff_cell_props,
        plan_apply_cell_props,
        order_index_cell_props,
        cell_sample(),
        cell_sample_alt()
    );
}

#[test]
fn prop_07_every_row_props_row_roundtrips_and_plans() {
    check_table_rows!(
        "w:tr",
        ROW_PROPS,
        RowPropsField,
        RowProps,
        read_row_props,
        emit_row_props,
        emit_row_props_value,
        diff_row_props,
        plan_apply_row_props,
        order_index_row_props,
        row_sample(),
        row_sample_alt()
    );
}

#[test]
fn prop_07_every_table_props_row_roundtrips_and_plans() {
    check_table_rows!(
        "w:tbl",
        TABLE_PROPS,
        TablePropsField,
        TableProps,
        read_table_props,
        emit_table_props,
        emit_table_props_value,
        diff_table_props,
        plan_apply_table_props,
        order_index_table_props,
        table_sample(),
        table_sample_alt()
    );
}

#[test]
fn prop_05_table_containers_order_tables() {
    let cell = |l: LocalName| order_index_cell_props(QName::w(l));
    assert!(cell(LocalName::CnfStyle) < cell(LocalName::TcW));
    assert!(cell(LocalName::GridSpan) < cell(LocalName::VMerge));
    assert!(cell(LocalName::VAlign) < cell(LocalName::Headers));
    assert!(cell(LocalName::CellMerge) < cell(LocalName::TcPrChange));
    assert_eq!(cell(LocalName::Left), cell(LocalName::Start), "tcBorders 的同义对不在 tcPr 里");
    assert_eq!(cell(LocalName::P), None);
    let row = |l: LocalName| order_index_row_props(QName::w(l));
    assert!(row(LocalName::TblPrEx).is_none(), "tblPrEx 是 w:tr 的兄弟，不在 trPr 里");
    assert!(row(LocalName::GridBefore) < row(LocalName::TrHeight));
    assert!(
        row(LocalName::Hidden) < row(LocalName::Ins)
            && row(LocalName::Del) < row(LocalName::TrPrChange)
    );
    let tbl = |l: LocalName| order_index_table_props(QName::w(l));
    assert!(
        tbl(LocalName::TblStyle) < tbl(LocalName::TblpPr)
            && tbl(LocalName::TblW) < tbl(LocalName::Jc)
    );
    assert!(tbl(LocalName::TblLook) < tbl(LocalName::TblCaption));
    let borders = |l: LocalName| order_index_tc_borders(QName::w(l));
    assert!(borders(LocalName::Top) < borders(LocalName::Left));
    assert_eq!(borders(LocalName::Left), borders(LocalName::Start));
    assert!(
        borders(LocalName::InsideV) < borders(LocalName::Tl2br)
            && borders(LocalName::Tl2br) < borders(LocalName::Tr2bl)
    );
    assert!(
        TABLES.iter().any(|t| t.name == "CellProps")
            && TABLES.iter().any(|t| t.name == "TblCellMar")
    );
    for f in CellPropsField::ALL {
        assert_eq!(f.info().order, order_index_cell_props(f.info().element).unwrap());
    }
    assert_eq!(CellPropsField::Borders.info().kind, FieldKind::Table);
    assert_eq!(CellPropsField::Width.info().kind, FieldKind::Struct);
    assert_eq!(CellPropsField::Headers.info().kind, FieldKind::Raw);
    assert_eq!(TC_BORDERS.field("start").unwrap().legacy, Some(QName::w(LocalName::Left)));
}
