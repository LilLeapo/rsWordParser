//! resolve 验收（`spec/07` 验收清单 RES-02 / 05 / 06，任务 1.9）。

use super::*;
use crate::model::Styles;
use crate::model::theme::Theme;
use crate::package::PartId;
use crate::semantic::props::{Color, Fonts, HexColorOrAuto, StyleType, ThemeColor, ThemeFont, Val};
use crate::xml::Dom;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

fn styles(xml: &str) -> Styles {
    let d =
        Dom::parse(PartId(0), format!(r#"<w:styles xmlns:w="{W}">{xml}</w:styles>"#).as_bytes())
            .unwrap();
    Styles::from_dom(&d, &mut Vec::new()).unwrap()
}

fn theme() -> Theme {
    let xml = format!(
        r#"<a:theme xmlns:a="{A}"><a:themeElements>
          <a:clrScheme name="x"><a:accent1><a:srgbClr val="4472C4"/></a:accent1></a:clrScheme>
          <a:fontScheme name="x"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Hans" typeface="等线 Light"/></a:majorFont>
          <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Arial"/></a:minorFont></a:fontScheme>
        </a:themeElements></a:theme>"#
    );
    Theme::from_dom(&Dom::parse(PartId(0), xml.as_bytes()).unwrap()).unwrap()
}

fn settings(ea: &str) -> crate::model::Settings {
    let d = Dom::parse(PartId(0), format!(r#"<w:settings xmlns:w="{W}"><w:themeFontLang w:val="en-US" w:eastAsia="{ea}"/></w:settings>"#).as_bytes()).unwrap();
    crate::model::Settings::from_dom(&d, &mut Vec::new()).unwrap()
}

fn on(v: bool) -> Option<bool> {
    Some(v)
}

#[test]
fn res_02_chain_cycle_default_and_outline_block() {
    let s = styles(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:rFonts w:ascii="Calibri"/></w:rPr></w:rPrDefault></w:docDefaults>
           <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:spacing w:after="160"/></w:pPr></w:style>
           <w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/><w:rPr><w:b/></w:rPr></w:style>
           <w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/><w:rPr><w:i/><w:sz w:val="40"/></w:rPr></w:style>
           <w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:link w:val="H1Char"/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:color w:val="2F5496"/></w:rPr></w:style>
           <w:style w:type="paragraph" w:styleId="TOC"><w:name w:val="TOC Heading"/><w:basedOn w:val="H1"/><w:pPr><w:outlineLvl w:val="9"/></w:pPr><w:rPr><w:b w:val="0"/></w:rPr></w:style>
           <w:style w:type="character" w:styleId="H1Char"><w:name w:val="Heading 1 Char"/><w:link w:val="H1"/><w:rPr><w:i/></w:rPr></w:style>
           <w:style w:type="paragraph" w:styleId="Other"><w:name w:val="Other"/><w:basedOn w:val="H1Char"/></w:style>"#,
    );
    let r = Resolver::from_parts(Some(&s), None, None, None);
    // 环：A → B → A 不死循环，两层都进链
    let chain: Vec<_> =
        r.chain("A", StyleType::Paragraph).iter().map(|s| s.id().unwrap()).collect();
    assert_eq!(chain, ["A", "B"]);
    let a = r.style_run_props("A", StyleType::Paragraph).unwrap();
    assert_eq!((a.bold, a.italic, a.size), (on(true), on(true), Some(Val::Value(40))));
    // 最后一个 default 胜出（这里只有一个）
    assert_eq!(r.default_style(StyleType::Paragraph).unwrap().id(), Some("Normal"));
    // outlineLvl 9 阻断
    assert_eq!(r.heading_level("H1"), Some(1));
    assert_eq!(r.heading_level("TOC"), None);
    assert_eq!(r.heading_level("Heading4"), Some(4), "文档未定义的内建样式");
    // 类型不一致的 basedOn 视为链结束
    assert_eq!(r.chain("Other", StyleType::Paragraph).len(), 1);
    // linked：字符侧缺失项由段落侧补，自身声明优先
    let h1c = r.style_run_props("H1Char", StyleType::Character).unwrap();
    assert_eq!(h1c.italic, on(true));
    assert_eq!(h1c.bold, on(true));
    assert_eq!(h1c.size, Some(Val::Value(32)));
    assert!(r.is_linked_char_shell("H1Char"));
    assert!(!r.is_linked_char_shell("H1"));
    // 段落侧也从字符侧补
    let h1 = r.style_run_props("H1", StyleType::Paragraph).unwrap();
    assert_eq!(h1.italic, on(true));
    // 段落属性链
    let toc = r.style_para_props("TOC").unwrap();
    assert_eq!(toc.keep_next, on(true), "从 H1 继承");
    assert_eq!(toc.spacing.as_ref().unwrap().after, Some(Val::Value(160)), "从 Normal 继承");
    assert_eq!(toc.outline_lvl, Some(Val::Value(9)));
}

#[test]
fn res_03_run_cascade_with_provenance() {
    let s = styles(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault></w:docDefaults>
           <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
           <w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:link w:val="H1Char"/><w:rPr><w:b/><w:sz w:val="32"/><w:color w:val="2F5496"/></w:rPr></w:style>
           <w:style w:type="character" w:styleId="Emph"><w:name w:val="Emphasis"/><w:rPr><w:i/><w:color w:val="FF0000"/></w:rPr></w:style>
           <w:style w:type="character" w:styleId="H1Char"><w:name w:val="Heading 1 Char"/><w:link w:val="H1"/></w:style>"#,
    );
    let r = Resolver::from_parts(Some(&s), None, None, None);
    let direct = RunProps { size: Some(Val::Value(48)), ..Default::default() };
    let e = r.run(Some("H1"), Some("Emph"), &direct);
    assert_eq!(e.props.size, Some(Val::Value(48)));
    assert_eq!(e.source(RunPropsField::Size), Provenance::Direct);
    assert_eq!(e.props.bold, on(true));
    assert_eq!(e.source(RunPropsField::Bold), Provenance::ParaStyle("H1".into()));
    assert_eq!(e.props.italic, on(true));
    assert_eq!(e.source(RunPropsField::Italic), Provenance::CharStyle("Emph".into()));
    // 字符样式覆盖段落样式的颜色
    assert_eq!(
        e.props.color.as_ref().unwrap().val,
        Some(Val::Value(HexColorOrAuto::Rgb([0xFF, 0, 0])))
    );
    assert_eq!(e.source(RunPropsField::Color), Provenance::CharStyle("Emph".into()));
    assert_eq!(e.props.lang.as_ref().unwrap().val.as_deref(), Some("en-US"));
    assert_eq!(e.source(RunPropsField::Lang), Provenance::DocDefaults);
    assert_eq!(e.source(RunPropsField::Kern), Provenance::Default);
    // linked 补缺：Normal 段落里的 H1Char run 得到 H1 的 b / sz / color
    let e2 = r.run(Some("Normal"), Some("H1Char"), &RunProps::default());
    assert_eq!(e2.props.bold, on(true));
    assert_eq!(e2.props.size, Some(Val::Value(32)));
    assert_eq!(e2.source(RunPropsField::Bold), Provenance::CharStyle("H1Char".into()));
    // RES-04：直接 w:b w:val="0" 压住样式的 b（两条候选规则都这么说，见 resolve::toggle）
    let off = RunProps { bold: on(false), ..Default::default() };
    assert_eq!(r.run(Some("H1"), None, &off).props.bold, on(false));
}

#[test]
fn res_06_rtl_reads_cs_twins_only() {
    let s = styles(
        r#"<w:style w:type="paragraph" w:styleId="Rtl"><w:name w:val="Rtl"/><w:rPr><w:rtl/><w:bCs/></w:rPr></w:style>
           <w:style w:type="character" w:styleId="NoRtl"><w:name w:val="NoRtl"/><w:rPr><w:rtl w:val="0"/></w:rPr></w:style>"#,
    );
    let r = Resolver::from_parts(Some(&s), None, None, None);
    // w:rtl run 只读 bCs，w:b 被忽略
    let direct = RunProps {
        rtl: on(true),
        bold: on(false),
        bold_cs: on(true),
        size: Some(Val::Value(20)),
        size_cs: Some(Val::Value(28)),
        ..Default::default()
    };
    let e = r.run(None, None, &direct);
    assert!(e.cs.value);
    assert_eq!(e.cs.source, Provenance::Direct);
    assert_eq!(e.bold(), on(true));
    assert_eq!(e.size(), Some(28));
    // 无直接 rtl：字符样式链 → 段落样式链 → false
    let e = r.run(Some("Rtl"), Some("NoRtl"), &RunProps::default());
    assert!(!e.cs.value);
    assert_eq!(e.cs.source, Provenance::CharStyle("NoRtl".into()));
    let e = r.run(Some("Rtl"), None, &RunProps { bold: on(true), ..Default::default() });
    assert!(e.cs.value);
    assert_eq!(e.cs.source, Provenance::ParaStyle("Rtl".into()));
    assert_eq!(e.bold(), on(true), "cs 读 bCs（来自样式），直接 w:b 不算");
    assert_eq!(e.italic(), None, "无交叉回退");
    let e = r.run(None, None, &RunProps::default());
    assert_eq!(e.cs, Effective { value: false, source: Provenance::Default });
}

#[test]
fn res_05_theme_fonts_and_colors() {
    let t = theme();
    let st = settings("ja-JP");
    let r = Resolver::from_parts(None, None, Some(&t), Some(&st));
    // 空 EA 槽 + themeFontLang ja → Yu Mincho（minor）/ Yu Gothic（major）
    let f = Fonts {
        east_asia_theme: Some(Val::Value(ThemeFont::MinorEastAsia)),
        east_asia: Some("Stale".into()),
        ascii_theme: Some(Val::Value(ThemeFont::MinorHAnsi)),
        ..Default::default()
    };
    let rf = r.fonts(&RunProps { fonts: Some(f.clone()), ..Default::default() });
    assert_eq!(rf.east_asia.as_deref(), Some("Yu Mincho"));
    assert!(rf.ea_slot_empty);
    assert_eq!(rf.ascii.as_deref(), Some("Calibri"));
    assert_eq!(rf.display(), Some("Yu Mincho"));
    let major =
        Fonts { east_asia_theme: Some(Val::Value(ThemeFont::MajorEastAsia)), ..Default::default() };
    assert_eq!(
        r.fonts(&RunProps { fonts: Some(major.clone()), ..Default::default() })
            .east_asia
            .as_deref(),
        Some("Yu Gothic")
    );
    // script 表优先：zh-CN → Hans → 等线 Light（major）
    let zh = settings("zh-CN");
    let r2 = Resolver::from_parts(None, None, Some(&t), Some(&zh));
    assert_eq!(
        r2.fonts(&RunProps { fonts: Some(major), ..Default::default() }).east_asia.as_deref(),
        Some("等线 Light")
    );
    // minor 没有 Hans 表项、不是 ja/ko → DengXian
    assert_eq!(
        r2.fonts(&RunProps { fonts: Some(f), ..Default::default() }).east_asia.as_deref(),
        Some("DengXian")
    );
    // 主题槽有值：cstheme minorBidi → Arial；字面值兜底
    let cs = Fonts {
        cs_theme: Some(Val::Value(ThemeFont::MinorBidi)),
        cs: Some("Times".into()),
        h_ansi: Some("Verdana".into()),
        ..Default::default()
    };
    let rf = r.fonts(&RunProps { fonts: Some(cs), ..Default::default() });
    assert_eq!(rf.cs.as_deref(), Some("Arial"));
    assert_eq!(rf.h_ansi.as_deref(), Some("Verdana"));
    assert!(!rf.ea_slot_empty);
    // 无主题：主题属性退回字面值，不标空槽
    let r3 = Resolver::from_parts(None, None, None, None);
    let lit = Fonts {
        east_asia_theme: Some(Val::Value(ThemeFont::MinorEastAsia)),
        east_asia: Some("宋体".into()),
        ..Default::default()
    };
    let rf = r3.fonts(&RunProps { fonts: Some(lit), ..Default::default() });
    assert_eq!(rf.east_asia.as_deref(), Some("宋体"));
    assert!(!rf.ea_slot_empty);

    // 颜色：themeColor accent1 + tint 99 与 Word 一致（允许 ±1）
    let c = Color {
        val: Some(Val::Value(HexColorOrAuto::Rgb([1, 2, 3]))),
        theme_color: Some(Val::Value(ThemeColor::Accent1)),
        theme_tint: Some(Val::Value(0x99)),
        ..Default::default()
    };
    let rgb = r.color(&c).unwrap();
    // 4472C4 * 0x99/255 + 255*(1-0x99/255) = (0x44*153/255+102, ...) ≈ (142.8, 170.4, 219.6)
    for (got, exp) in rgb.iter().zip([143u8, 170, 220]) {
        assert!((i32::from(*got) - i32::from(exp)).abs() <= 1, "{rgb:?}");
    }
    // shade 后 tint
    let c2 = Color {
        theme_color: Some(Val::Value(ThemeColor::Text1)),
        theme_shade: Some(Val::Value(0x80)),
        ..Default::default()
    };
    assert_eq!(r.color(&c2), Some([0, 0, 0]), "dk1 缺省 000000");
    let c3 = Color {
        theme_color: Some(Val::Value(ThemeColor::Background1)),
        theme_shade: Some(Val::Value(0x80)),
        ..Default::default()
    };
    assert_eq!(r.color(&c3), Some([128, 128, 128]), "lt1 缺省 FFFFFF 再 shade");
    // 无 theme part → 内建调色板
    assert_eq!(
        r3.color(&Color {
            theme_color: Some(Val::Value(ThemeColor::Accent2)),
            ..Default::default()
        }),
        Some([0xED, 0x7D, 0x31])
    );
    // auto → None；字面值
    assert_eq!(
        r3.color(&Color { val: Some(Val::Value(HexColorOrAuto::Auto)), ..Default::default() }),
        None
    );
    assert_eq!(
        r3.color(&Color {
            val: Some(Val::Value(HexColorOrAuto::Rgb([9, 8, 7]))),
            ..Default::default()
        }),
        Some([9, 8, 7])
    );
    assert_eq!(rgb_hex([0xAB, 0x00, 0x1F]), "AB001F");
}

#[test]
fn res_05_doc_default_fonts_ea_backfill() {
    let s = styles(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:eastAsiaTheme="minorEastAsia"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
    );
    let t = theme();
    let r = Resolver::from_parts(Some(&s), None, Some(&t), None);
    let f = r.doc_default_fonts();
    assert_eq!(f.ascii.as_deref(), Some("Calibri"));
    assert_eq!(f.east_asia.as_deref(), Some("SimSun"), "空槽 + w:lang zh-CN → SimSun");
    assert!(f.ea_from_lang && f.ea_slot_empty);
    // themeFontLang 命中 script 表时优先于 w:lang
    let zh = settings("zh-CN");
    let s2 = styles(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:eastAsiaTheme="majorEastAsia"/><w:lang w:eastAsia="ja-JP"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
    );
    let r2 = Resolver::from_parts(Some(&s2), None, Some(&t), Some(&zh));
    assert_eq!(r2.doc_default_fonts().east_asia.as_deref(), Some("等线 Light"));
    // 无 w:lang：槽空则不发明 EA 字体
    let s3 = styles(
        r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:eastAsiaTheme="minorEastAsia"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
    );
    let r3 = Resolver::from_parts(Some(&s3), None, Some(&t), Some(&zh));
    assert_eq!(r3.doc_default_fonts().east_asia, None);
}

#[test]
fn res_07_09_para_cascade_and_numbering_indent() {
    let s = styles(
        r#"<w:docDefaults><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
           <w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>
           <w:style w:type="numbering" w:styleId="NumStyle"><w:name w:val="NumStyle"/><w:pPr><w:numPr><w:numId w:val="7"/></w:numPr></w:pPr></w:style>"#,
    );
    let n_xml = format!(
        r#"<w:numbering xmlns:w="{W}">
          <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>
          <w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="NumStyle"/></w:abstractNum>
          <w:abstractNum w:abstractNumId="2"><w:styleLink w:val="NumStyle"/><w:lvl w:ilvl="0"><w:pPr><w:ind w:left="99"/></w:pPr></w:lvl></w:abstractNum>
          <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
          <w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:lvl w:ilvl="0"><w:pPr><w:ind w:left="5000"/></w:pPr></w:lvl></w:lvlOverride></w:num>
          <w:num w:numId="3"><w:abstractNumId w:val="1"/></w:num>
          <w:num w:numId="7"><w:abstractNumId w:val="2"/></w:num>
        </w:numbering>"#
    );
    let n = Numbering::from_dom(&Dom::parse(PartId(0), n_xml.as_bytes()).unwrap(), &mut Vec::new())
        .unwrap();
    let r = Resolver::from_parts(Some(&s), Some(&n), None, None);
    let list = ListRef { num_id: 1, ilvl: 0, from_style: false };
    let e = r.para(Some("ListParagraph"), Some(&list), &ParaProps::default());
    let ind = e.props.indent.as_ref().unwrap();
    assert_eq!(ind.start, Some(Val::Value(1440)), "编号级别 ind 覆盖样式 ind");
    assert_eq!(ind.hanging, Some(Val::Value(360)));
    assert_eq!(e.source(ParaPropsField::Indent), Provenance::NumberingLevel { num_id: 1, ilvl: 0 });
    assert_eq!(e.props.contextual_spacing, on(true));
    assert_eq!(
        e.source(ParaPropsField::ContextualSpacing),
        Provenance::ParaStyle("ListParagraph".into())
    );
    assert_eq!(e.props.spacing.as_ref().unwrap().after, Some(Val::Value(160)));
    assert_eq!(e.source(ParaPropsField::Spacing), Provenance::DocDefaults);
    // 段落自身有 ind → 编号级别不参与
    let direct = ParaProps {
        indent: Some(crate::semantic::props::Indent {
            start: Some(Val::Value(10)),
            ..Default::default()
        }),
        ..Default::default()
    };
    let e = r.para(Some("ListParagraph"), Some(&list), &direct);
    assert_eq!(e.props.indent.as_ref().unwrap().start, Some(Val::Value(10)));
    assert_eq!(e.source(ParaPropsField::Indent), Provenance::Direct);
    // RES-09 级别查找：override 整级、numStyleLink 链
    assert_eq!(
        r.level(2, 0).unwrap().ppr.as_ref().unwrap().indent.as_ref().unwrap().start,
        Some(Val::Value(5000))
    );
    assert_eq!(
        r.level(3, 0).unwrap().ppr.as_ref().unwrap().indent.as_ref().unwrap().start,
        Some(Val::Value(99)),
        "numStyleLink → NumStyle → numId 7 → abstractNum 2"
    );
    assert!(r.level(1, 3).is_none());
    assert!(r.level(42, 0).is_none());
}
