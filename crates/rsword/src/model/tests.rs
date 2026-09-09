//! 模型验收（`spec/06` 验收清单 MOD-03 / 05 / 06，任务 1.5–1.8）。

use super::*;
use crate::diag::DiagCode;
use crate::package::{PartId, Rels};
use crate::semantic::props::Val;
use crate::xml::{Dom, LocalName, QName};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const V: &str = "urn:schemas-microsoft-com:vml";

fn doc(body: &str) -> Dom {
    let xml = format!(
        r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:m="{M}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:v="{V}"><w:body>{body}</w:body></w:document>"#
    );
    Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{xml}"))
}

fn styles(xml: &str) -> Styles {
    let d =
        Dom::parse(PartId(0), format!(r#"<w:styles xmlns:w="{W}">{xml}</w:styles>"#).as_bytes())
            .unwrap();
    Styles::from_dom(&d, &mut Vec::new()).unwrap()
}

fn build(body: &str) -> (Vec<Block>, Vec<crate::diag::Diagnostic>) {
    let d = doc(body);
    Document::build_main(&d, None, &Rels::default())
}

fn build_with(body: &str, s: &Styles) -> Vec<Block> {
    let d = doc(body);
    Document::build_main(&d, Some(s), &Rels::default()).0
}

fn text_of(b: &Block) -> String {
    b.as_text().expect("text block").text()
}

#[test]
fn mod_06_coordinate_flow_acceptance() {
    // "Hello" + <w:tab/> + "World" → Hello\tWorld
    let (blocks, _) = build(r#"<w:p><w:r><w:t>Hello</w:t><w:tab/><w:t>World</w:t></w:r></w:p>"#);
    assert_eq!(text_of(&blocks[0]), "Hello\tWorld");
    let tb = blocks[0].as_text().unwrap();
    assert_eq!(tb.utf16_len(), 11);
    let Inline::Run(run) = &tb.inlines[0] else { panic!() };
    assert_eq!(run.segments.len(), 3);
    assert_eq!(run.segments[1].kind, SegmentKind::Tab);
    assert_eq!(run.segments[1].text, 5..6);
    assert_eq!(run.segment_text(&run.segments[2]), "World");

    // 含图片 run 的段落坐标流含 1 个 U+FFFC
    let (blocks, _) = build(
        r#"<w:p><w:r><w:t>A</w:t></w:r><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"/></a:graphic></wp:inline></w:drawing></w:r><w:r><w:t>B</w:t></w:r></w:p>"#,
    );
    let t = text_of(&blocks[0]);
    assert_eq!(t, format!("A{OBJECT_REPLACEMENT}B"));
    assert_eq!(t.chars().filter(|&c| c == OBJECT_REPLACEMENT).count(), 1);
    assert_eq!(blocks[0].as_text().unwrap().utf16_len(), 3);

    // 无 preserve 的 <w:t> x </w:t> 文本为 x；有 preserve 原样；xml:space 沿祖先继承、default 复位
    let (blocks, _) =
        build(r#"<w:p><w:r><w:t> x </w:t><w:t xml:space="preserve"> y </w:t></w:r></w:p>"#);
    assert_eq!(text_of(&blocks[0]), "x y ");
    let (blocks, _) = build(
        r#"<w:p xml:space="preserve"><w:r><w:t> a </w:t><w:t xml:space="default"> b </w:t></w:r></w:p>"#,
    );
    assert_eq!(text_of(&blocks[0]), " a b");

    // 换行 / 分页 / 连字符 / 代理对
    let (blocks, _) = build(
        r#"<w:p><w:r><w:t>a</w:t><w:br/><w:t>b</w:t><w:br w:type="page"/><w:cr/><w:noBreakHyphen/><w:softHyphen/><w:t>😀</w:t><w:lastRenderedPageBreak/><w:fldChar w:fldCharType="begin"/></w:r></w:p>"#,
    );
    let tb = blocks[0].as_text().unwrap();
    assert_eq!(tb.text(), format!("a\nb{OBJECT_REPLACEMENT}\n\u{2011}\u{00AD}😀"));
    assert_eq!(tb.utf16_len(), 9, "😀 占 2 个 UTF-16 单位");
    let Inline::Run(run) = &tb.inlines[0] else { panic!() };
    let kinds: Vec<&SegmentKind> = run.segments.iter().map(|s| &s.kind).collect();
    assert!(matches!(kinds[1], SegmentKind::Br { kind: BreakKind::TextWrapping, .. }));
    assert!(matches!(kinds[3], SegmentKind::Br { kind: BreakKind::Page, .. }));
    assert_eq!(kinds[8], &SegmentKind::LastRenderedPageBreak);
    assert_eq!(run.segments[8].utf16_len, 0);
    assert_eq!(run.segments[9].kind, SegmentKind::FldChar);
    assert_eq!(run.segments[7].utf16_len, 2);
    // 段区间覆盖且不重叠
    let mut pos = 0;
    for s in &run.segments {
        assert_eq!(s.text.start, pos);
        pos = s.text.end;
    }
    assert_eq!(pos as usize, run.text.len());
}

#[test]
fn mod_06_symbols_atoms_and_run_props() {
    let (blocks, _) = build(
        r#"<w:p><w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:sym w:font="Wingdings" w:char="F0FC"/></w:r>
           <m:oMath><m:r><m:t>x</m:t></m:r></m:oMath><w:br w:type="page"/>
           <w:r><w:ruby><w:rt><w:r><w:t>rt</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>base</w:t></w:r></w:rubyBase></w:ruby></w:r>
           <w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
    );
    let tb = blocks[0].as_text().unwrap();
    assert_eq!(tb.inlines.len(), 5);
    let Inline::Run(run) = &tb.inlines[0] else { panic!() };
    assert_eq!(run.props.bold, Some(true));
    assert_eq!(run.props.size, Some(Val::Value(28)));
    assert_eq!(run.text, "\u{F0FC}", "符号字体映射表在 M2，先按 U+F000 + (code & 0xFF)");
    assert!(
        matches!(&run.segments[0].kind, SegmentKind::Sym { font: Some(f), code: Some(0xF0FC) } if f == "Wingdings")
    );
    assert!(matches!(&tb.inlines[1], Inline::Atom(InlineAtom { kind: AtomKind::Math, .. })));
    assert!(matches!(
        &tb.inlines[2],
        Inline::Atom(InlineAtom { kind: AtomKind::BareBreak { kind: BreakKind::Page }, .. })
    ));
    let Inline::Run(ruby) = &tb.inlines[3] else { panic!() };
    assert!(matches!(&ruby.segments[0].kind, SegmentKind::Ruby { rt, .. } if rt == "rt"));
    let Inline::Run(fn_ref) = &tb.inlines[4] else { panic!() };
    assert!(
        matches!(&fn_ref.segments[0].kind, SegmentKind::FootnoteRef { id: Some(id) } if id == "1")
    );
    assert_eq!(
        tb.text(),
        format!(
            "\u{F0FC}{OBJECT_REPLACEMENT}{OBJECT_REPLACEMENT}{OBJECT_REPLACEMENT}{OBJECT_REPLACEMENT}"
        )
    );
    assert_eq!(tb.utf16_len(), 5);
}

#[test]
fn mod_06_hyperlink_revisions_and_transparent_containers() {
    let (blocks, _) = build(
        r#"<w:p><w:hyperlink r:id="rId9" w:tooltip="tip"><w:r><w:t>link</w:t></w:r></w:hyperlink>
           <w:hyperlink w:anchor="bm1"><w:r><w:t>in</w:t></w:r></w:hyperlink>
           <w:ins w:id="3" w:author="A" w:date="2024-01-01T00:00:00Z"><w:r><w:t>new</w:t></w:r></w:ins>
           <w:moveFrom w:id="4" w:author="B" w:date="2024-01-02T00:00:00Z"><w:r><w:delText>gone</w:delText></w:r></w:moveFrom>
           <w:sdt><w:sdtPr/><w:sdtContent><w:r><w:t>sdt</w:t></w:r></w:sdtContent></w:sdt>
           <w:bookmarkStart w:id="0" w:name="bm1"/><w:proofErr w:type="spellStart"/>
           <w:r><w:rPr><w:rPrChange w:id="7" w:author="C" w:date="2024-01-03T00:00:00Z"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>chg</w:t></w:r>
           <w:bookmarkEnd w:id="0"/></w:p>"#,
    );
    let tb = blocks[0].as_text().unwrap();
    assert_eq!(tb.text(), "linkinnewgonesdtchg", "范围标记与 proofErr 不占位");
    let runs: Vec<&Run> = tb
        .inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 6);
    assert!(
        matches!(&runs[0].link, Some(Link::Hyperlink { target: LinkTarget::External { rel_id, href: None }, tooltip: Some(t), .. }) if rel_id == "rId9" && t == "tip")
    );
    assert!(
        matches!(&runs[1].link, Some(Link::Hyperlink { target: LinkTarget::Internal { anchor }, .. }) if anchor == "bm1")
    );
    let ins = runs[2].rev.as_ref().unwrap();
    assert_eq!(ins.ins.as_ref().unwrap().author.as_deref(), Some("A"));
    assert!(ins.del.is_none());
    let mv = runs[3].rev.as_ref().unwrap();
    assert!(
        mv.del.is_some() && mv.move_from.is_some() && mv.ins.is_none(),
        "moveFrom 同时计入 del"
    );
    assert_eq!(runs[3].segments[0].kind, SegmentKind::DelText);
    assert!(runs[4].rev.is_none() && runs[4].link.is_none());
    let chg = runs[5].rev.as_ref().unwrap();
    let (meta, old) = chg.props_change.as_ref().unwrap();
    assert_eq!(meta.id.as_deref(), Some("7"));
    assert_eq!(old.italic, Some(true));
}

#[test]
fn mod_03_text_kind_acceptance() {
    let s = styles(
        r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
           <w:style w:type="paragraph" w:styleId="Sub"><w:name w:val="Sub"/><w:basedOn w:val="Heading1"/></w:style>
           <w:style w:type="paragraph" w:styleId="ListPara"><w:name w:val="List Para"/><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="5"/></w:numPr></w:pPr></w:style>
           <w:style w:type="paragraph" w:styleId="NoList"><w:name w:val="No List"/><w:basedOn w:val="ListPara"/><w:pPr><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr></w:style>
           <w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Heading1"/><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>"#,
    );
    let body = r#"
      <w:p><w:pPr><w:pStyle w:val="Heading1"/><w:outlineLvl w:val="9"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="Sub"/></w:pPr><w:r><w:t>b</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="TOCHeading"/></w:pPr><w:r><w:t>c</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="ListPara"/></w:pPr><w:r><w:t>d</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="NoList"/></w:pPr><w:r><w:t>e</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="ListPara"/><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr><w:r><w:t>f</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="ListPara"/><w:numPr><w:ilvl w:val="2"/></w:numPr></w:pPr><w:r><w:t>g</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="Heading1"/><w:numPr><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>h</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>i</w:t></w:r></w:p>
      <w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>j</w:t></w:r></w:p>"#;
    let blocks = build_with(body, &s);
    let kinds: Vec<&TextKind> = blocks.iter().map(|b| &b.as_text().unwrap().kind).collect();
    assert_eq!(kinds[0], &TextKind::Paragraph, "outlineLvl=9 且样式为 Heading1 → Paragraph");
    assert_eq!(kinds[1], &TextKind::Heading { level: 1 }, "basedOn 继承标题级别");
    assert_eq!(kinds[2], &TextKind::Paragraph, "outlineLvl 9 阻断继承");
    assert_eq!(
        kinds[3],
        &TextKind::ListItem { list: ListRef { num_id: 5, ilvl: 1, from_style: true } }
    );
    assert_eq!(kinds[4], &TextKind::Paragraph, "样式 numId 0 取消继承编号");
    assert_eq!(kinds[5], &TextKind::Paragraph, "直接 numId 0 → 无编号");
    assert_eq!(
        kinds[6],
        &TextKind::ListItem { list: ListRef { num_id: 5, ilvl: 2, from_style: true } },
        "ilvl 直接、numId 来自样式"
    );
    assert_eq!(
        kinds[7],
        &TextKind::ListItem { list: ListRef { num_id: 9, ilvl: 0, from_style: false } },
        "ListRef 优先于 Heading"
    );
    assert_eq!(kinds[8], &TextKind::Heading { level: 3 }, "文档未定义的内建 Heading3");
    assert_eq!(kinds[9], &TextKind::Heading { level: 3 }, "直接 outlineLvl");
}

#[test]
fn mod_05_body_rules() {
    let s = styles(
        r#"<w:style w:type="paragraph" w:styleId="Hidden"><w:name w:val="Hidden"/><w:rPr><w:vanish/></w:rPr></w:style>"#,
    );
    let body = r#"
      <w:p><w:r><w:t>text</w:t></w:r></w:p>
      <w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>
      <w:sdt><w:sdtContent><w:p><w:r><w:t>s1</w:t></w:r></w:p><w:p><w:r><w:t>s2</w:t></w:r></w:p></w:sdtContent></w:sdt>
      <w:sdt><w:sdtContent/></w:sdt>
      <w:bookmarkStart w:id="1" w:name="x"/><w:bookmarkEnd w:id="1"/>
      <w:br w:type="page"/>
      <w:ins w:id="2" w:author="A" w:date="2024-01-01T00:00:00Z"><w:p><w:r><w:t>inserted</w:t></w:r></w:p></w:ins>
      <w:altChunk r:id="rId5"/>
      <w:p><w:pPr><w:pStyle w:val="Hidden"/></w:pPr><w:r><w:t>hidden</w:t></w:r></w:p>
      <w:p><w:pPr><w:pStyle w:val="Hidden"/></w:pPr><w:r><w:rPr><w:vanish w:val="0"/></w:rPr><w:t>shown</w:t></w:r></w:p>
      <w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p>
      <w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>with text</w:t></w:r></w:p>
      <w:p><m:oMathPara><m:oMath/></m:oMathPara></w:p>
      <w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><a:blip/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
      <w:p><w:r><w:drawing><wp:anchor><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></wp:anchor></w:drawing></w:r></w:p>
      <w:p><w:r><w:pict><v:rect o:hr="t" xmlns:o="urn:schemas-microsoft-com:office:office"/></w:pict></w:r></w:p>
      <w:p><w:r><w:object><v:shape/></w:object></w:r></w:p>
      <w:p><w:r><w:t>text</w:t><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
      <w:sectPr><w:pgSz w:w="11906"/></w:sectPr>"#;
    let d = doc(body);
    let (blocks, warnings) = Document::build_main(&d, Some(&s), &Rels::default());
    let kinds: Vec<String> = blocks
        .iter()
        .map(|b| match b {
            Block::Text(t) => format!("Text:{}", t.text()),
            Block::Table(_) => "Table".into(),
            Block::Image(_) => "Image".into(),
            Block::Protected(p) => format!("Protected:{}", p.kind.key()),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "Text:text",
            "Table",
            "Text:s1",
            "Text:s2",
            "Protected:protected.invisible",
            "Protected:protected.body_break",
            "Text:inserted",
            "Protected:protected.unknown",
            "Protected:protected.invisible",
            "Text:shown",
            "Protected:protected.section_break",
            "Text:with text",
            "Protected:protected.equation",
            "Image",
            "Protected:protected.chart",
            "Protected:protected.rule",
            "Protected:protected.ole",
            &format!("Text:text{OBJECT_REPLACEMENT}"),
            "Protected:protected.section_props",
        ]
    );
    // sdt 信息、修订包裹、预览、诊断
    assert!(blocks[2].sdt().is_some() && blocks[3].sdt().is_some() && blocks[0].sdt().is_none());
    assert!(
        matches!(blocks[6].revisions(), [Revision::Insert(m)] if m.author.as_deref() == Some("A"))
    );
    let Block::Protected(unknown) = &blocks[7] else { panic!() };
    assert!(matches!(&unknown.kind, ProtectedKind::Unknown(q) if q.local == LocalName::AltChunk));
    let Block::Protected(hidden) = &blocks[8] else { panic!() };
    assert_eq!(hidden.preview, "hidden");
    assert!(matches!(&blocks[10], Block::Protected(p) if p.kind == ProtectedKind::SectionBreak));
    assert_eq!(warnings.iter().filter(|d| d.code == DiagCode::ModUnknownBlock).count(), 1);
    // 每条规则可单测
    let f = ParagraphFacts { has_sect_pr: true, visible_text: false, ..Default::default() };
    assert_eq!(classify_paragraph(&f), ("R10", ParaClass::Protected(ProtectedKind::SectionBreak)));
    let f = ParagraphFacts { has_sect_pr: true, visible_text: true, ..Default::default() };
    assert_eq!(classify_paragraph(&f), ("R19", ParaClass::Text));
    let (rule, class) = classify_body_child(&d, blocks[1].node());
    assert_eq!((rule, class), ("R02", BodyClass::Table));
}

#[test]
fn mod_04_paragraph_facts() {
    let s = styles(
        r#"<w:style w:type="paragraph" w:styleId="TOC 2"><w:name w:val="toc 2"/></w:style>"#,
    );
    let d = doc(
        r#"<w:p><w:pPr><w:pStyle w:val="TOC 2"/><w:rPr><w:del w:id="1" w:author="a" w:date="2024-01-01T00:00:00Z"/></w:rPr>
             <w:pPrChange w:id="2" w:author="a" w:date="2024-01-01T00:00:00Z"><w:pPr/></w:pPrChange></w:pPr>
           <w:r><w:t> </w:t></w:r>
           <w:r><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r>
           <w:del w:id="3" w:author="a" w:date="2024-01-01T00:00:00Z"><w:r><w:delText>gone</w:delText></w:r></w:del>
           <m:oMath/></w:p>"#,
    );
    let blocks = Document::build_main(&d, Some(&s), &Rels::default()).0;
    let tb = blocks[0].as_text().unwrap();
    let f = &tb.facts;
    assert!(f.visible_text, "delText 也算可见文本");
    assert!(f.visible_text_outside_boxes);
    assert_eq!(f.toc_style_level, Some(2));
    assert_eq!(f.picts.len(), 1);
    assert_eq!(f.picts[0].kind, PictKind::TextBox);
    assert_eq!(f.math.count, 1);
    assert!(f.revision.run_del && f.revision.para_mark_del && f.revision.ppr_change);
    assert!(!f.revision.run_ins);
    assert!(matches!(
        tb.revisions.as_slice(),
        [Revision::ParaMarkDelete(_), Revision::ParaPropsChange { .. }]
    ));
    // 只有文本框里有字：visible_text 为假
    let d2 = doc(
        r#"<w:p><w:r><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
    );
    let blocks = Document::build_main(&d2, None, &Rels::default()).0;
    let f = &blocks[0].as_text().unwrap().facts;
    assert!(!f.visible_text && !f.visible_text_outside_boxes);
}

#[test]
fn mod_01_document_without_body_warns() {
    let xml = format!(r#"<w:document xmlns:w="{W}"/>"#);
    let d = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
    let (blocks, warnings) = Document::build_main(&d, None, &Rels::default());
    assert!(blocks.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, DiagCode::ModUnparseable);
    let _ = QName::w(LocalName::Body);
}
