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
    let ctx = EditContext::default().with_track_changes(Some(rsword::edit::RevisionAuthor {
        author: "甲".into(),
        date: Some("2026-01-01T00:00:00Z".into()),
    }));
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

// ---------------------------------------------------------------- SetDrawingWrap

use rsword::edit::ImageWrap;
use rsword::model::drawing_Wrap as Wrap;

/// 九种绕排 + 随文，`SetDrawingWrap` 的完整取值域。
const ALL_WRAPS: [Option<ImageWrap>; 10] = [
    None,
    Some(ImageWrap::SquareLeft),
    Some(ImageWrap::SquareRight),
    Some(ImageWrap::TightLeft),
    Some(ImageWrap::TightRight),
    Some(ImageWrap::ThroughLeft),
    Some(ImageWrap::ThroughRight),
    Some(ImageWrap::TopBottom),
    Some(ImageWrap::Front),
    Some(ImageWrap::Behind),
];

/// `word/document.xml` 里每一棵 `a:graphic` 的原字节。
fn graphics(docx: &[u8]) -> Vec<String> {
    let xml = String::from_utf8_lossy(&common::part_bytes(docx, "word/document.xml")).to_string();
    let mut out = Vec::new();
    let mut rest = xml.as_str();
    while let Some(i) = rest.find("<a:graphic ").or_else(|| rest.find("<a:graphic>")) {
        let tail = &rest[i..];
        let Some(j) = tail.find("</a:graphic>") else { break };
        out.push(tail[..j + 12].to_string());
        rest = &tail[j + 12..];
    }
    out
}

/// `image-wrap__*` 的二十份文档（`spec/18` 写的「11 份」是导语料之前的估数）：逐份切到
/// 每一种绕排再切回。内容指纹一路不变、
/// `a:graphic` 子树一个字节没动（`SAVE-08`），最后一步回到原来的绕排。
#[test]
fn set_wrap_round_trips_on_the_image_wrap_corpus() {
    let mut seen = 0;
    for path in common::docx_paths("synthetic") {
        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if !name.starts_with("image-wrap__") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("语料");
        let base = EditSession::open(&bytes).unwrap();
        seen += 1;
        let want_fp = common::fingerprint::fingerprint(&base);
        let want_graphics = graphics(&bytes);
        let start = original_wrap(&base);
        let mut s = EditSession::open(&bytes).unwrap();
        for step in ALL_WRAPS.into_iter().chain([start]) {
            let d = drawing(&s, 0);
            s.apply(
                EditOp::SetDrawingWrap { drawing: d, wrap: step, pos: None, z_order: None },
                &EditContext::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: 切到 {step:?} 失败: {e}"));
            let out = s.save().unwrap();
            assert_fingerprint_eq!(
                common::fingerprint::fingerprint(&s),
                want_fp,
                "{name}: 切到 {step:?} 之后内容变了"
            );
            assert_eq!(graphics(&out), want_graphics, "{name}: 切到 {step:?} 动了 a:graphic");
            assert_eq!(shell_wrap(&s), step, "{name}: 切到 {step:?} 之后模型读回来不一样");
        }
    }
    assert_eq!(seen, 20, "image-wrap__* 应有 20 份带图文档");
}

/// 从模型读回当前的绕排（`None` = 随文）。`wrapNone` 靠 `behindDoc` 分前后，
/// 方形 / 紧密 / 穿越靠 `positionH` 的对齐分左右——正好是我们生成时写进去的那两样。
fn shell_wrap(s: &EditSession) -> Option<ImageWrap> {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let d = rsword::model::drawing_display(dom, drawing(s, 0));
    let a = d.anchor.as_ref()?;
    let right = a.h.align.as_deref() == Some("right");
    Some(match &a.wrap {
        rsword::model::drawing_Wrap::None if a.behind_doc => ImageWrap::Behind,
        rsword::model::drawing_Wrap::None => ImageWrap::Front,
        rsword::model::drawing_Wrap::TopAndBottom => ImageWrap::TopBottom,
        rsword::model::drawing_Wrap::Square { .. } if right => ImageWrap::SquareRight,
        rsword::model::drawing_Wrap::Square { .. } => ImageWrap::SquareLeft,
        rsword::model::drawing_Wrap::Tight { .. } if right => ImageWrap::TightRight,
        rsword::model::drawing_Wrap::Tight { .. } => ImageWrap::TightLeft,
        rsword::model::drawing_Wrap::Through { .. } if right => ImageWrap::ThroughRight,
        rsword::model::drawing_Wrap::Through { .. } => ImageWrap::ThroughLeft,
        rsword::model::drawing_Wrap::Unspecified => return None,
    })
}

/// 打开时第一张图的绕排（回环的终点）。
fn original_wrap(s: &EditSession) -> Option<ImageWrap> {
    shell_wrap(s)
}

/// 随文 → 锚定：新壳的属性与子元素次序按 `CT_Anchor`，搬进去的五样原字节。
#[test]
fn set_wrap_inline_to_anchor_rebuilds_only_the_shell() {
    let bytes = inline_pic_doc();
    let before = graphics(&bytes);
    let mut s = EditSession::open(&bytes).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::SquareRight),
            pos: None,
            z_order: Some(5),
        },
        &EditContext::default(),
    )
    .expect("换成方形绕排");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//wp:inline)", ["0"]),
            ("//wp:anchor/@relativeHeight", ["251658245"]),
            ("//wp:anchor/@behindDoc", ["0"]),
            ("//wp:anchor/@simplePos", ["0"]),
            ("//wp:anchor/@distL", ["114300"]),
            ("//wp:positionH/@relativeFrom", ["column"]),
            ("//wp:positionH/wp:align/text()", ["right"]),
            ("//wp:positionV/wp:posOffset/text()", ["0"]),
            ("//wp:wrapSquare/@wrapText", ["bothSides"]),
            // 搬进去的五样都在，一份不多
            ("count(//wp:anchor/wp:extent)", ["1"]),
            ("count(//wp:anchor/wp:effectExtent)", ["1"]),
            ("count(//wp:anchor/wp:docPr)", ["1"]),
            ("count(//wp:anchor/a:graphic)", ["1"]),
        ]
    );
    assert_eq!(graphics(&out), before, "a:graphic 子树原字节（SAVE-08）");
    // 次序：simplePos → positionH → positionV → extent → effectExtent → wrapSquare → docPr → graphic
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    let at = |t: &str| xml.find(t).unwrap_or_else(|| panic!("找不到 {t}"));
    let order = [
        "<wp:simplePos",
        "<wp:positionH",
        "<wp:positionV",
        "<wp:extent",
        "<wp:effectExtent",
        "<wp:wrapSquare",
        "<wp:docPr",
        "<a:graphic",
    ]
    .map(at);
    assert!(order.windows(2).all(|w| w[0] < w[1]), "CT_Anchor 的子元素次序: {order:?}");
}

/// 紧密与穿越之间保留原来的 `wp:wrapPolygon`；从方形切过来则生成整幅图的矩形。
#[test]
fn set_wrap_keeps_a_hand_edited_polygon_between_tight_and_through() {
    let mut s = EditSession::open(&inline_pic_doc()).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::TightLeft),
            pos: None,
            z_order: None,
        },
        &EditContext::default(),
    )
    .expect("紧密");
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [
            ("//wp:wrapTight/wp:wrapPolygon/@edited", ["0"]),
            ("//wp:wrapTight/wp:wrapPolygon/wp:start/@x", ["0"]),
            ("count(//wp:wrapTight/wp:wrapPolygon/wp:lineTo)", ["4"]),
        ]
    );
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::ThroughRight),
            pos: None,
            z_order: None,
        },
        &EditContext::default(),
    )
    .expect("穿越");
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [
            ("count(//wp:wrapTight)", ["0"]),
            ("count(//wp:wrapThrough/wp:wrapPolygon)", ["1"]),
            ("count(//wp:wrapThrough/wp:wrapPolygon/wp:lineTo)", ["4"]),
            ("//wp:positionH/wp:align/text()", ["right"]),
        ]
    );
    // 切到方形再切回紧密：方形没有多边形，回来时重新生成
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::SquareLeft),
            pos: None,
            z_order: None,
        },
        &EditContext::default(),
    )
    .unwrap();
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [("count(//wp:wrapPolygon)", ["0"]), ("count(//wp:wrapSquare)", ["1"])]
    );
}

/// 给了 `pos` 就照写；锚定 → 随文把位置与绕排一起丢掉。
#[test]
fn set_wrap_explicit_position_then_back_to_inline() {
    let mut s = EditSession::open(&inline_pic_doc()).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::Behind),
            pos: Some(rsword::edit::AnchorPos {
                h: rsword::edit::AnchorAxis {
                    relative_from: "page".into(),
                    pos: rsword::edit::AxisPos::Offset(914_400),
                },
                v: rsword::edit::AnchorAxis {
                    relative_from: "page".into(),
                    pos: rsword::edit::AxisPos::Align("center".into()),
                },
            }),
            z_order: None,
        },
        &EditContext::default(),
    )
    .expect("衬于文字下方");
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [
            ("//wp:anchor/@behindDoc", ["1"]),
            ("count(//wp:wrapNone)", ["1"]),
            ("//wp:positionH/@relativeFrom", ["page"]),
            ("//wp:positionH/wp:posOffset/text()", ["914400"]),
            ("//wp:positionV/wp:align/text()", ["center"]),
        ]
    );
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap { drawing: d, wrap: None, pos: None, z_order: None },
        &EditContext::default(),
    )
    .expect("回到随文");
    common::xpath_asserts!(
        &s.save().unwrap(),
        "word/document.xml",
        [
            ("count(//wp:anchor)", ["0"]),
            ("count(//wp:positionH)", ["0"]),
            ("count(//wp:wrapNone)", ["0"]),
            ("//wp:inline/@distL", ["0"]),
            ("count(//wp:inline/wp:extent)", ["1"]),
        ]
    );
}

/// 文档里本来就有的手绘多边形：紧密 → 穿越同类，原样搬过去（连 `edited="1"` 与坐标）。
#[test]
fn set_wrap_moves_an_existing_polygon_verbatim() {
    let body = concat!(
        r#"<w:p><w:r><w:drawing><wp:anchor xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
        r#" distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251658240""#,
        r#" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
        r#"<wp:simplePos x="0" y="0"/>"#,
        r#"<wp:positionH relativeFrom="column"><wp:align>left</wp:align></wp:positionH>"#,
        r#"<wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>"#,
        r#"<wp:extent cx="914400" cy="914400"/>"#,
        r#"<wp:wrapTight wrapText="largest"><wp:wrapPolygon edited="1">"#,
        r#"<wp:start x="123" y="456"/><wp:lineTo x="789" y="1011"/><wp:lineTo x="123" y="456"/>"#,
        r#"</wp:wrapPolygon></wp:wrapTight>"#,
        r#"<wp:docPr id="1" name="p1"/>"#,
        r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
        r#"<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"/>"#,
        r#"</a:graphic></wp:anchor></w:drawing></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let d = drawing(&s, 0);
    s.apply(
        EditOp::SetDrawingWrap {
            drawing: d,
            wrap: Some(ImageWrap::ThroughLeft),
            pos: None,
            z_order: None,
        },
        &EditContext::default(),
    )
    .expect("穿越");
    let out = s.save().unwrap();
    let xml = String::from_utf8_lossy(&common::part_bytes(&out, "word/document.xml")).to_string();
    assert!(
        xml.contains(concat!(
            r#"<wp:wrapPolygon edited="1">"#,
            r#"<wp:start x="123" y="456"/><wp:lineTo x="789" y="1011"/><wp:lineTo x="123" y="456"/>"#,
            r#"</wp:wrapPolygon>"#
        )),
        "多边形应当原字节搬过去:\n{xml}"
    );
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//wp:wrapTight)", ["0"]),
            ("//wp:wrapThrough/@wrapText", ["bothSides"]),
            ("count(//wp:wrapThrough/wp:wrapPolygon)", ["1"]),
        ]
    );
}

// ---------------------------------------------------------------- normalize_z_order

fn rel_heights(docx: &[u8]) -> Vec<i64> {
    let xml = String::from_utf8_lossy(&common::part_bytes(docx, "word/document.xml")).to_string();
    xml.match_indices("relativeHeight=\"")
        .filter_map(|(i, _)| xml[i + 16..].split('"').next()?.parse().ok())
        .collect()
}

/// `SaveOptions.normalize_z_order`：LibreOffice 写 `relativeHeight="1" / "2" / "3"` 的语料
/// 开启后按 z 序稳定重排成 `251658240 + 0..n`，再跑一遍不动；关闭时一个字节都不动。
#[test]
fn normalize_z_order_rewrites_wild_relative_heights() {
    let path = common::corpus_dir("synthetic").join("anchor-z-order__003.docx");
    let bytes = std::fs::read(&path).expect("anchor-z-order__003.docx");
    // 文档序是 3、1、2：正好能看出名次按 z 排、不是按文档序
    assert_eq!(rel_heights(&bytes), [3, 1, 2], "语料前提：LibreOffice 的野值");

    // 关着：一个字节都不动（不变式 1）
    let mut s = EditSession::open(&bytes).unwrap();
    assert_eq!(s.save().unwrap(), bytes, "缺省不归一");

    let opts = rsword::save::SaveOptions { normalize_z_order: true, ..Default::default() };
    let mut s = EditSession::open(&bytes).unwrap();
    let out = s.save_with(&opts).unwrap();
    assert_eq!(rel_heights(&out), [251_658_242, 251_658_240, 251_658_241]);
    // 幂等：归一过的文档再归一什么都不发生
    let mut s = EditSession::open(&out).unwrap();
    assert_eq!(s.save_with(&opts).unwrap(), out, "归一是幂等的");
}

/// z 序本来就规矩（Word 自己写的 `251658240 + 小数`）的文档，开着归一也不动。
#[test]
fn normalize_z_order_leaves_sane_documents_alone() {
    let path = common::corpus_dir("synthetic").join("anchor-z-order__001.docx");
    let bytes = std::fs::read(&path).expect("anchor-z-order__001.docx");
    assert_eq!(rel_heights(&bytes), [251_658_243, 251_658_241], "语料前提：z = 3 与 1");
    let mut s = EditSession::open(&bytes).unwrap();
    let out = s
        .save_with(&rsword::save::SaveOptions { normalize_z_order: true, ..Default::default() })
        .unwrap();
    assert_eq!(rel_heights(&out), [251_658_243, 251_658_241], "闸门没开，名次不动");
    assert_eq!(out, bytes, "一个字节都不动");
}
