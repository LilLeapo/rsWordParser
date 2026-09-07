//! 既有绘图的编辑（`EDIT-03`，`spec/18` 7.7）。
//!
//! 两条与真实 Word 的对照：`fixtures/word-ops/z-order`（置于顶层）与 `move-resize`
//! （右下移 2 cm、等比缩到一半）。那里的 README 记着已复算的数字。

mod common;

use rsword::edit::{DrawingGeometry, EditContext, EditOp, EditSession};
use rsword::xml::{LocalName, NodeId, QName};

/// 主 part 里第 `i` 个 `w:drawing`。
fn drawing(s: &EditSession, i: usize) -> NodeId {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    dom.descendants(dom.root())
        .filter(|&n| dom.is(n, QName::w(LocalName::Drawing)))
        .nth(i)
        .expect("有这么多 w:drawing")
}

/// 真实 Word 的对照：把最底下那张浮动图片「置于顶层」。
#[test]
fn word_ops_z_order_matches_word() {
    let dir = common::repo_root().join("fixtures/word-ops/z-order");
    let before = std::fs::read(dir.join("before.docx")).expect("before.docx");
    let after = std::fs::read(dir.join("after.docx")).expect("after.docx");
    let mut s = EditSession::open(&before).unwrap();
    let d = drawing(&s, 0);
    // Word 把它抬到了 251661312 = 251658240 + 3072
    s.apply(EditOp::SetDrawingZOrder { drawing: d, z: 3072 }, &EditContext::default())
        .expect("置于顶层");
    let out = s.save().unwrap();
    let got = common::part_bytes(&out, "word/document.xml");
    let want = common::part_bytes(&after, "word/document.xml");
    let (g, w) = (String::from_utf8_lossy(&got), String::from_utf8_lossy(&want));
    let rh = |x: &str| {
        x.match_indices("relativeHeight=\"")
            .map(|(i, _)| x[i + 16..].split('"').next().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(rh(&g), rh(&w), "三个锚的 relativeHeight 与 Word 相同");
    // 其余数字一个没动
    let nums = |x: &str, tag: &str| {
        x.match_indices(tag)
            .map(|(i, _)| x[i..].chars().take(60).collect::<String>())
            .collect::<Vec<_>>()
    };
    assert_eq!(nums(&g, "<wp:posOffset>"), nums(&w, "<wp:posOffset>"), "位置没动");
    assert_eq!(nums(&g, "<wp:extent "), nums(&w, "<wp:extent "), "尺寸没动");
}

/// 真实 Word 的对照：浮动图片右下移约 2 cm，再等比缩到一半。
#[test]
fn word_ops_move_resize_matches_word() {
    let dir = common::repo_root().join("fixtures/word-ops/move-resize");
    let before = std::fs::read(dir.join("before.docx")).expect("before.docx");
    let after = std::fs::read(dir.join("after.docx")).expect("after.docx");
    let mut s = EditSession::open(&before).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry {
                extent_emu: Some((762_000, 381_000)),
                pos_offset_emu: Some((721_360, 1_033_780)),
                ..Default::default()
            },
        },
        &EditContext::default(),
    )
    .expect("移动 + 缩放");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//wp:anchor/wp:extent/@cx", ["762000"]),
            ("//wp:anchor/wp:extent/@cy", ["381000"]),
            ("//wp:positionH/wp:posOffset/text()", ["721360"]),
            ("//wp:positionV/wp:posOffset/text()", ["1033780"]),
            ("//a:xfrm/a:ext/@cx", ["762000"]),
            ("//a:xfrm/a:ext/@cy", ["381000"]),
        ]
    );
    // 与 Word 的 after.docx 逐项对：指纹不含绘图几何（那是版面数字，不是内容），
    // 所以这里直接比这四个数字
    let want_xml =
        String::from_utf8_lossy(&common::part_bytes(&after, "word/document.xml")).to_string();
    for needle in [
        r#"<wp:extent cx="762000" cy="381000"/>"#,
        r#"<wp:posOffset>721360</wp:posOffset>"#,
        r#"<wp:posOffset>1033780</wp:posOffset>"#,
        r#"<a:ext cx="762000" cy="381000"/>"#,
    ] {
        assert!(want_xml.contains(needle), "Word 的 after 里应有 {needle}");
        let ours =
            String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
        assert!(ours.contains(needle), "我们的输出里应有 {needle}");
    }
}

const INLINE_PIC: &str = concat!(
    r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">"#,
    r#"<wp:extent cx="914400" cy="914400"/><wp:effectExtent l="0" t="0" r="0" b="0"/>"#,
    r#"<wp:docPr id="1" name="p1"/>"#,
    r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
    r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
    r#"<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
    r#"<pic:nvPicPr><pic:cNvPr id="1" name="p1"/><pic:cNvPicPr/></pic:nvPicPr>"#,
    r#"<pic:blipFill><a:blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="rIdImg"/>"#,
    r#"<a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
    r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm>"#,
    r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
    r#"</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
);

fn inline_pic_doc() -> Vec<u8> {
    let png = common::b64(common::PNG_1X1);
    common::with_binary_part(&common::docx_with_body(INLINE_PIC), "word/media/image1.png", &png)
}

/// 改尺寸：`wp:extent` 与 `a:ext` 一起变，重解析后 `imageMeta` 的像素尺寸跟着变。
#[test]
fn set_geometry_resizes_and_keeps_the_graphic_bytes() {
    let bytes = inline_pic_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry { extent_emu: Some((457_200, 228_600)), ..Default::default() },
        },
        &EditContext::default(),
    )
    .expect("改尺寸");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//wp:inline/wp:extent/@cx", ["457200"]),
            ("//a:xfrm/a:ext/@cy", ["228600"]),
            // `a:graphic` 里没碰的东西原样（关系、几何形状）
            ("//a:blip/@r:embed", ["rIdImg"]),
            ("//a:prstGeom/@prst", ["rect"]),
        ]
    );
    // 媒体 part 一个字节都没动
    assert_eq!(
        common::part_bytes(&bytes, "word/media/image1.png"),
        common::part_bytes(&out, "word/media/image1.png"),
    );
}

/// 旋转与翻转写在 `a:xfrm` 上，`wp:effectExtent` 跟着旋转外接框走。
#[test]
fn set_geometry_rotation_updates_effect_extent() {
    let mut s = EditSession::open(&inline_pic_doc()).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry {
                extent_emu: Some((1_000_000, 500_000)),
                rot_deg: Some(Some(90)),
                flip_h: Some(true),
                ..Default::default()
            },
        },
        &EditContext::default(),
    )
    .expect("旋转");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//a:xfrm/@rot", ["5400000"]),
            ("//a:xfrm/@flipH", ["1"]),
            // 90° 之后外接框是 500000 × 1000000：左右各溢出 (500000-1000000)/2 → 0，上下 250000
            ("//wp:effectExtent/@t", ["250000"]),
            ("//wp:effectExtent/@l", ["0"]),
        ]
    );
}

/// 裁剪窗：`a:srcRect` 新建、替换、去掉。
#[test]
fn set_geometry_crop() {
    let mut s = EditSession::open(&inline_pic_doc()).unwrap();
    let d = drawing(&s, 0);
    let crop = rsword::edit::SrcRect { l: 1000, t: 2000, r: 0, b: 0 };
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry { crop: Some(Some(crop)), ..Default::default() },
        },
        &EditContext::default(),
    )
    .expect("裁剪");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//a:srcRect/@l", ["1000"]),
            ("//a:srcRect/@t", ["2000"]),
            ("count(//a:srcRect/@r)", ["0"]),
        ]
    );
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry { crop: Some(None), ..Default::default() },
        },
        &EditContext::default(),
    )
    .expect("去裁剪");
    let out = s.save().unwrap();
    common::xpath_asserts!(&out, "word/document.xml", [("count(//a:srcRect)", ["0"])]);
}

/// 随文图片没有 z-order。
#[test]
fn z_order_on_inline_is_refused() {
    let bytes = inline_pic_doc();
    let mut s = EditSession::open(&bytes).unwrap();
    let d = drawing(&s, 0);
    let err = s
        .apply(EditOp::SetDrawingZOrder { drawing: d, z: 1 }, &EditContext::default())
        .expect_err("随文没有 z-order");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditUnsupported),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：一个字节都没动");
}

/// `SetShapeStyle`：填充与描边。
#[test]
fn set_shape_style() {
    let body = concat!(
        r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">"#,
        r#"<wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="s1"/>"#,
        r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
        r#"<wps:wsp xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
        r#"<wps:cNvPr id="1" name="s1"/><wps:cNvSpPr/>"#,
        r#"<wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm>"#,
        r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#,
        r#"<a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></wps:spPr>"#,
        r#"<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let shape = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::new(rsword::xml::NsId::Wps, LocalName::Wsp)))
        .expect("有 wps:wsp");
    s.apply(
        EditOp::SetShapeStyle {
            shape,
            fill: Some(Some("00FF00".into())),
            outline: Some(Some("0000FF".into())),
        },
        &EditContext::default(),
    )
    .expect("改样式");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//wps:spPr/a:solidFill/a:srgbClr/@val", ["00FF00"]),
            ("//wps:spPr/a:ln/a:solidFill/a:srgbClr/@val", ["0000FF"]),
            ("count(//wps:spPr/a:solidFill)", ["1"]),
        ]
    );
    // 去掉填充
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let shape = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, QName::new(rsword::xml::NsId::Wps, LocalName::Wsp)))
        .expect("wsp");
    s.apply(
        EditOp::SetShapeStyle { shape, fill: Some(None), outline: None },
        &EditContext::default(),
    )
    .expect("无填充");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//wps:spPr/a:noFill)", ["1"]), ("count(//wps:spPr/a:solidFill)", ["0"]),]
    );
}

/// 追踪时绘图的格式改动不产生修订（Word 也不记），只留一条 `REV_NOT_TRACKED`。
#[test]
fn drawing_edits_are_not_tracked() {
    let mut s = EditSession::open(&inline_pic_doc()).unwrap();
    let d = drawing(&s, 0);
    let ctx = EditContext {
        track_changes: Some(rsword::edit::RevisionAuthor {
            author: "甲".into(),
            date: Some("2026-01-01T00:00:00Z".into()),
        }),
        ..Default::default()
    };
    s.apply(
        EditOp::SetDrawingGeometry {
            drawing: d,
            geom: DrawingGeometry { extent_emu: Some((100_000, 100_000)), ..Default::default() },
        },
        &ctx,
    )
    .expect("开着修订也能改尺寸");
    assert_eq!(
        s.diagnostics().iter().filter(|d| d.code == rsword::DiagCode::RevNotTracked).count(),
        1
    );
    assert!(s.document().revisions.is_empty(), "没有生成修订");
}
