//! `mc:AlternateContent` 的孪生同步（`EDIT-03`，`spec/18` 7.7）：改 `mc:Choice` 里的文本框，
//! `mc:Fallback` 的 VML 孪生跟着变；直接改 Fallback → `EDIT_TARGET_FALLBACK`。

mod common;

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::xml::{Dirty, LocalName, NodeId, NsId, QName};

/// 某个分支（`mc:Choice` / `mc:Fallback`）下第 `i` 个 `w:txbxContent` 里第 `j` 个 `w:p`。
fn para_in(s: &EditSession, which: LocalName, i: usize, j: usize) -> NodeId {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let branch = dom
        .descendants(dom.root())
        .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
        .find(|&n| dom.is(n, QName::new(NsId::Mc, which)))
        .expect("有这个分支");
    let txbx = dom
        .descendants(branch)
        .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
        .filter(|&n| dom.is(n, QName::new(NsId::W, LocalName::TxbxContent)))
        .nth(i)
        .expect("有这么多文本框");
    dom.children(txbx)
        .iter()
        .copied()
        .filter(|&n| dom.node(n).dirty != Dirty::Deleted && dom.is(n, QName::w(LocalName::P)))
        .nth(j)
        .expect("有这么多段")
}

fn textbox_doc() -> Vec<u8> {
    std::fs::read(common::corpus_dir("synthetic").join("textbox-edit__001.docx"))
        .expect("textbox-edit__001.docx")
}

/// `mc:Choice` 里的段落一改，`mc:Fallback` 的同序 `w:txbxContent` 整体跟着换；
/// 没碰的那一段字节相同。
#[test]
fn editing_the_choice_textbox_syncs_the_vml_twin() {
    let bytes = textbox_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let para = para_in(&s, LocalName::Choice, 0, 1);
    s.apply(
        EditOp::InsertText {
            at: InlinePos::new(para, 0),
            text: "很".into(),
            props: Default::default(),
        },
        &EditContext::default(),
    )
    .expect("改 Choice 里的文本框");
    let out = s.save().unwrap();
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    let (choice, fallback) = xml.split_once("<mc:Fallback>").expect("两个分支");
    for (name, part) in [("Choice", choice), ("Fallback", fallback)] {
        assert!(part.contains(">很CRITICAL</w:t>"), "{name} 里应有改后的文字:\n{part}");
        // 没碰的第一段原样（连 `rPr` 的三项都在）
        assert!(part.contains(r#"<w:t>**Current date:** {Month}</w:t>"#), "{name} 里第一段应原样");
        assert!(part.contains(r#"<w:color w:val="415461"/>"#), "{name} 里第一段的格式应原样");
    }
    // VML 的壳没被动过
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//v:rect)", ["1"]),
            ("//v:rect/@id", ["box1"]),
            ("count(//v:textbox/w:txbxContent/w:p)", ["2"]),
            ("count(//wps:txbx/w:txbxContent/w:p)", ["2"]),
        ]
    );
}

/// 结构性的编辑（往文本框里插一段、删一段）也同步。
#[test]
fn structural_edits_in_a_textbox_sync_the_twin() {
    let mut s = EditSession::open(&textbox_doc()).unwrap();
    let para = para_in(&s, LocalName::Choice, 0, 0);
    s.apply(EditOp::DeleteBlock { part: None, node: para }, &EditContext::default())
        .expect("删掉文本框里的第一段");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//wps:txbx/w:txbxContent/w:p)", ["1"]),
            ("count(//v:textbox/w:txbxContent/w:p)", ["1"]),
        ]
    );
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert!(!xml.contains("**Current date:**"), "两边都不该再有第一段:\n{xml}");
    assert_eq!(xml.matches("<w:t>CRITICAL</w:t>").count(), 2, "两边各剩一段");
}

/// 位置落在 `mc:Fallback` 里 → 整体拒绝，一个字节都没动（`EDIT-05`）。
#[test]
fn editing_the_fallback_is_refused() {
    let bytes = textbox_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let para = para_in(&s, LocalName::Fallback, 0, 0);
    let err = s
        .apply(
            EditOp::InsertText {
                at: InlinePos::new(para, 0),
                text: "x".into(),
                props: Default::default(),
            },
            &EditContext::default(),
        )
        .expect_err("Fallback 不能直接改");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditTargetFallback),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：一个字节都没动");
}

/// 文本框外的编辑不惊动孪生：同一份文档里正文段落改了，两个分支都原字节。
#[test]
fn edits_outside_the_textbox_leave_the_twin_alone() {
    let bytes = textbox_doc();
    let before =
        String::from_utf8_lossy(&common::part_bytes(&bytes, "word/document.xml")).to_string();
    let mut s = EditSession::open(&bytes).unwrap();
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let body = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::Body)))
        .expect("w:body");
    let para = dom
        .children(body)
        .iter()
        .copied()
        .find(|&n| dom.is(n, QName::w(LocalName::P)))
        .expect("正文第一段");
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos { part: None, at: rsword::edit::BlockAt::After(para) },
            block: rsword::edit::NewBlock::Paragraph {
                props: Default::default(),
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("新的一段"))],
            },
        },
        &EditContext::default(),
    )
    .expect("正文里插一段");
    let out = s.save().unwrap();
    let after = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    let twin = |x: &str| {
        let i = x.find("<mc:AlternateContent").expect("AC");
        let j = x.find("</mc:AlternateContent>").expect("/AC");
        x[i..j].to_string()
    };
    assert_eq!(twin(&after), twin(&before), "文本框那一段原字节");
    assert!(after.contains("新的一段"));
}

// ---------------------------------------------------------------- SetTextboxContent

/// `SetTextboxContent`：框里的块整体换掉，孪生跟着换。
#[test]
fn set_textbox_content_replaces_both_branches() {
    let mut s = EditSession::open(&textbox_doc()).unwrap();
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let wsp = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::new(NsId::Wps, LocalName::Wsp)))
        .expect("wps:wsp");
    s.apply(
        EditOp::SetTextboxContent {
            textbox: wsp,
            blocks: vec![rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("换了"))],
            }],
        },
        &EditContext::default(),
    )
    .expect("整体替换");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//wps:txbx/w:txbxContent/w:p)", ["1"]),
            ("count(//v:textbox/w:txbxContent/w:p)", ["1"]),
        ]
    );
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert_eq!(xml.matches("换了").count(), 2, "两个分支各一份");
    assert!(!xml.contains("CRITICAL"), "旧内容两边都没了");
}

// ---------------------------------------------------------------- 新建文本框 / 形状 / 线条

fn blank() -> Vec<u8> {
    common::docx_with_body("<w:p/>")
}

fn insert(s: &mut EditSession, block: rsword::edit::NewBlock) -> Vec<u8> {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let body = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::Body)))
        .expect("w:body");
    let para = dom
        .children(body)
        .iter()
        .copied()
        .find(|&n| dom.is(n, QName::w(LocalName::P)))
        .expect("第一段");
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos { part: None, at: rsword::edit::BlockAt::After(para) },
            block,
        },
        &EditContext::default(),
    )
    .expect("插入");
    s.save().unwrap()
}

/// `NewBlock::Textbox`：`mc:Choice` + VML 孪生，两边内容相同；重解析后是一个文本框。
#[test]
fn new_textbox_emits_a_choice_and_a_vml_twin() {
    let mut s = EditSession::open(&blank()).unwrap();
    let out = insert(
        &mut s,
        rsword::edit::NewBlock::Textbox {
            look: rsword::edit::ShapeLook {
                extent_emu: Some((2_000_000, 1_000_000)),
                fill: Some("F5F5F5".into()),
                outline: Some("CCCCCC".into()),
                ..Default::default()
            },
            blocks: vec![rsword::edit::NewBlock::Paragraph {
                props: None,
                inlines: vec![rsword::edit::NewInline::Run(rsword::edit::NewRun::text("框里"))],
            }],
        },
    );
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//mc:Choice/@Requires", ["wps"]),
            ("//wps:cNvSpPr/@txBox", ["1"]),
            ("//wps:spPr/a:solidFill/a:srgbClr/@val", ["F5F5F5"]),
            ("//wps:spPr/a:ln/a:solidFill/a:srgbClr/@val", ["CCCCCC"]),
            ("//wp:anchor/wp:extent/@cx", ["2000000"]),
            ("count(//wps:txbx/w:txbxContent/w:p)", ["1"]),
            ("count(//mc:Fallback/w:pict/v:rect/v:textbox/w:txbxContent/w:p)", ["1"]),
            ("count(//wp:wrapSquare)", ["1"]),
        ]
    );
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert_eq!(xml.matches("框里").count(), 2, "Choice 与 Fallback 各一份");
    // 重解析后模型把它认成文本框
    let s2 = EditSession::open(&out).unwrap();
    assert!(has_textbox(&s2), "重解析后应有文本框");
}

fn has_textbox(s: &EditSession) -> bool {
    use rsword::model::{Block, Display, Inline};
    s.document().blocks().any(|b| {
        let rsword::model::Block::Text(t) = b else { return false };
        t.inlines.iter().any(|i| {
            let rsword::model::Inline::Run(r) = i else { return false };
            r.segments.iter().any(|seg| {
                seg.display.as_ref().and_then(rsword::model::Display::as_drawing).is_some_and(|d| {
                    d.shapes.iter().any(|sh| sh.txbx.is_some() && !sh.content.is_empty())
                })
            })
        })
    })
}

/// `NewBlock::Shape`：纯图形（没给 `text` 就没有 `wps:txbx`），`preset` 原样进属性。
#[test]
fn new_shape_writes_the_preset_geometry() {
    let mut s = EditSession::open(&blank()).unwrap();
    let out = insert(
        &mut s,
        rsword::edit::NewBlock::Shape {
            preset: rsword::edit::PresetGeom("ellipse".into()),
            look: rsword::edit::ShapeLook {
                extent_emu: Some((914_400, 914_400)),
                fill: Some("#FF0000".into()),
                wrap: Some(rsword::edit::ImageWrap::TopBottom),
                z_order: Some(7),
                ..Default::default()
            },
            text: None,
        },
    );
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//a:prstGeom/@prst", ["ellipse"]),
            ("//wps:spPr/a:solidFill/a:srgbClr/@val", ["FF0000"]),
            ("count(//wps:spPr/a:ln/a:noFill)", ["1"]),
            ("count(//wps:txbx)", ["0"]),
            ("count(//wp:wrapTopAndBottom)", ["1"]),
            ("//wp:anchor/@relativeHeight", ["251658247"]),
        ]
    );
}

/// `NewBlock::Line`：两点定位置与大小，只有描边，没有 VML 孪生。
#[test]
fn new_line_positions_itself_from_two_points() {
    let mut s = EditSession::open(&blank()).unwrap();
    let out = insert(
        &mut s,
        rsword::edit::NewBlock::Line {
            kind: rsword::edit::LineKind::LineArrowDouble,
            from: (900_000, 500_000),
            to: (300_000, 800_000),
            color: Some("0000FF".into()),
        },
    );
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//a:prstGeom/@prst", ["straightConnector1"]),
            ("count(//a:headEnd)", ["1"]),
            ("count(//a:tailEnd)", ["1"]),
            ("//a:ln/a:solidFill/a:srgbClr/@val", ["0000FF"]),
            // 大小 = 两点之差，位置 = 左上角
            ("//wp:anchor/wp:extent/@cx", ["600000"]),
            ("//wp:anchor/wp:extent/@cy", ["300000"]),
            ("//wp:positionH/wp:posOffset/text()", ["300000"]),
            ("//wp:positionV/wp:posOffset/text()", ["500000"]),
            ("count(//mc:AlternateContent)", ["0"]),
            ("count(//wp:wrapNone)", ["1"]),
        ]
    );
}

/// Strict 包里新建文本框不发 VML（Strict 没有 VML）。
#[test]
fn new_textbox_in_a_strict_package_has_no_vml() {
    let path = common::corpus_dir("synthetic").join("extra__strict-minimal.docx");
    let strict = std::fs::read(&path).expect("Strict 语料");
    let mut s = EditSession::open(&strict).unwrap();
    let out = insert(
        &mut s,
        rsword::edit::NewBlock::Textbox { look: Default::default(), blocks: Vec::new() },
    );
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//mc:AlternateContent)", ["0"]),
            ("count(//v:rect)", ["0"]),
            ("count(//wps:txbx/w:txbxContent/w:p)", ["1"]),
        ]
    );
}

/// `SetDrawingGeometry` / `SetShapeStyle` 同步 VML 孪生的 `@style` / `@fillcolor` / `@strokecolor`。
/// `@style` 里我们不认的键（`z-index`、`mso-*`）原样留着。
#[test]
fn geometry_and_style_sync_the_vml_attributes() {
    let body = concat!(
        r#"<w:p><w:r><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#,
        r#" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
        r#"<mc:Choice Requires="wps"><w:drawing><wp:anchor xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
        r#" distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="251658240""#,
        r#" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
        r#"<wp:simplePos x="0" y="0"/>"#,
        r#"<wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH>"#,
        r#"<wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>"#,
        r#"<wp:extent cx="914400" cy="914400"/><wp:wrapNone/><wp:docPr id="1" name="s1"/>"#,
        r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
        r#"<wps:wsp><wps:cNvSpPr/><wps:spPr>"#,
        r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm>"#,
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        r#"<a:solidFill><a:srgbClr val="FF0000"/></a:solidFill>"#,
        r#"<a:ln><a:noFill/></a:ln></wps:spPr><wps:bodyPr/></wps:wsp>"#,
        r#"</a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice>"#,
        r#"<mc:Fallback><w:pict><v:rect xmlns:v="urn:schemas-microsoft-com:vml" id="r1""#,
        r##" style="position:absolute;width:72pt;height:72pt;z-index:5" fillcolor="#FF0000" filled="t" stroked="f"/>"##,
        r#"</w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let drawing = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::Drawing)))
        .expect("w:drawing");
    // 尺寸 1828800 × 457200 = 144pt × 36pt，位置右下移 228600 = 18pt
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing,
            geom: rsword::edit::DrawingGeometry {
                extent_emu: Some((1_828_800, 457_200)),
                pos_offset_emu: Some((228_600, 228_600)),
                ..Default::default()
            },
        },
        &EditContext::default(),
    )
    .expect("改几何");
    let out = s.save().unwrap();
    let style = |docx: &[u8]| {
        let x = String::from_utf8_lossy(&common::part_bytes(docx, "word/document.xml")).to_string();
        let i = x.find("<v:rect").expect("v:rect");
        let t = &x[i..];
        let j = t.find("style=\"").expect("style") + 7;
        t[j..].split('"').next().unwrap_or_default().to_string()
    };
    assert_eq!(
        style(&out),
        "position:absolute;width:144.00pt;height:36.00pt;z-index:5;margin-left:18.00pt;margin-top:18.00pt",
        "尺寸与位置跟着改，z-index 与 position 原样留着"
    );

    // 换填充与描边
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let wsp = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::new(NsId::Wps, LocalName::Wsp)))
        .expect("wps:wsp");
    s.apply(
        EditOp::SetShapeStyle {
            shape: wsp,
            fill: Some(None),
            outline: Some(Some("00FF00".into())),
        },
        &EditContext::default(),
    )
    .expect("改样式");
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [
            ("//v:rect/@filled", ["f"]),
            ("//v:rect/@stroked", ["t"]),
            ("//v:rect/@strokecolor", ["#00FF00"]),
            // 旧的 fillcolor 留着但 filled="f"，与 Word / TS 同（VML 的 filled 才是开关）
            ("//v:rect/@fillcolor", ["#FF0000"]),
        ]
    );
}

/// 三种新块重解析后的分类：都是**锚定的绘图**（`Segment.kind = Drawing { anchored }`、
/// `DrawingDisplay.kind = Shape`），预置几何与 TS 生成的那份一样。
#[test]
fn new_shapes_classify_as_anchored_drawings() {
    use rsword::model::DrawingKind;
    for (what, prst, block) in [
        (
            "shape",
            "ellipse",
            rsword::edit::NewBlock::Shape {
                preset: rsword::edit::PresetGeom("ellipse".into()),
                look: Default::default(),
                text: None,
            },
        ),
        (
            "line",
            "curvedConnector3",
            rsword::edit::NewBlock::Line {
                kind: rsword::edit::LineKind::LineCurved,
                from: (0, 0),
                to: (914_400, 0),
                color: None,
            },
        ),
        (
            "textbox",
            "rect",
            rsword::edit::NewBlock::Textbox { look: Default::default(), blocks: Vec::new() },
        ),
    ] {
        let mut s = EditSession::open(&blank()).unwrap();
        let out = insert(&mut s, block);
        let s2 = EditSession::open(&out).unwrap();
        let d = only_drawing(&s2);
        assert_eq!(d.kind, DrawingKind::Shape, "{what}: 是形状");
        assert!(d.anchor.is_some(), "{what}: 是浮动的");
        assert_eq!(d.shapes.len(), 1, "{what}: 一个 wps:wsp");
        assert_eq!(d.shapes[0].prst.as_deref(), Some(prst), "{what}: 预置几何");
        assert_eq!(d.shapes[0].txbx.is_some(), what == "textbox", "{what}: 只有文本框带 wps:txbx");
    }
}

/// 文档里唯一那个 `w:drawing` 的投影。
fn only_drawing(s: &EditSession) -> rsword::model::DrawingDisplay {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let d = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::w(LocalName::Drawing)))
        .expect("w:drawing");
    rsword::model::drawing_display(dom, d)
}
