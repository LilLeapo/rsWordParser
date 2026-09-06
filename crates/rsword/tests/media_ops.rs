//! 媒体写侧（`PKG-05` / `EDIT-04` / `EDIT-06` / `SAVE-05` / `SAVE-06`，`spec/17` 任务 6.7）——**zip 级**断言。
//!
//! 新图片：随文 / 九种锚定 / 旋转的 `effectExtent` / `paraSpacing` / 同一字节去重；`ReplaceImageMedia`：改指新媒体、
//! 删裁剪、`r:link` 变内嵌、`svgBlip` 扩展丢弃；资源回收：反复替换只剩最新一份媒体、删图表连 part / `.rels` /
//! 工作簿 / Override 一起消失而其他条目 CRC 不变、别处仍引用时保留、原本就是孤儿的 part 不动、`prune_orphans: false`。

mod common;

use std::io::Read;

use rsword::bind::compat_ts::parsed_doc;
use rsword::diag::DiagCode;
use rsword::edit::{
    BlockPos, EditContext, EditOp, EditSession, ImageWrap, NewBlock, NewChart, NewChartKind,
    NewChartSeries, NewImage, ParaSpacing, PosOffset,
};
use rsword::model::{Block, ProtectedKind};
use rsword::package::Package;
use rsword::save::SaveOptions;
use rsword::xml::{LocalName, QName};
use serde_json::Value;

const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
/// 1×1 GIF。
const GIF_1X1: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

fn png() -> Vec<u8> {
    common::b64(common::PNG_1X1)
}

fn image(bytes: Vec<u8>) -> NewImage {
    NewImage {
        bytes,
        mime: "image/png".into(),
        extent_emu: (609_600, 304_800),
        align: None,
        wrap: None,
        pos_offset_emu: None,
        z_order: None,
        rot_deg: None,
        flip_h: false,
        flip_v: false,
        para_spacing: None,
    }
}

fn open(bytes: &[u8]) -> EditSession {
    EditSession::open(bytes).expect("open")
}

fn names(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
}

fn crcs(bytes: &[u8]) -> Vec<(String, u32)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    (0..z.len())
        .map(|i| {
            let f = z.by_index(i).unwrap();
            (f.name().to_string(), f.crc32())
        })
        .collect()
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    let mut f = z.by_name(name).unwrap_or_else(|_| panic!("zip 里没有 {name}"));
    let mut out = Vec::new();
    f.read_to_end(&mut out).unwrap();
    out
}

fn text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(entry(bytes, name)).expect("utf-8")
}

fn parsed(bytes: &[u8]) -> Value {
    let mut pkg = Package::open(bytes).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

fn media_entries(bytes: &[u8]) -> Vec<String> {
    names(bytes).into_iter().filter(|n| n.starts_with("word/media/") && !n.ends_with('/')).collect()
}

fn first_para(s: &EditSession) -> rsword::xml::NodeId {
    s.nth_text_block(0).expect("paragraph").node
}

/// 一张带图的文档：`copies` 段都引用 `rId10` → `word/media/image1.png`；`pre_orphan` 时再放一个没人引用的
/// 媒体 part 与关系（`rId99` → `orphan.png`）。
fn docx_with_picture(copies: usize, pre_orphan: bool) -> Vec<u8> {
    let para = format!(
        r#"<w:p><w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}"><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="Picture 1"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic xmlns:pic="{PIC}"><pic:blipFill><a:blip r:embed="rId10"/><a:srcRect l="10000" t="20000" r="30000" b="40000"/><a:stretch><a:fillRect l="5000"/></a:stretch></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    );
    let body = format!("{}<w:p><w:r><w:t>后文</w:t></w:r></w:p>", para.repeat(copies));
    let orphan = if pre_orphan {
        format!(r#"<Relationship Id="rId99" Type="{R}/image" Target="media/orphan.png"/>"#)
    } else {
        String::new()
    };
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId10" Type="{R}/image" Target="media/image1.png"/>{orphan}</Relationships>"#
    );
    let d = common::docx_with_parts(&body, &[("word/_rels/document.xml.rels", rels.as_str())]);
    let d = common::with_binary_part(&d, "word/media/image1.png", &png());
    if pre_orphan {
        common::with_binary_part(&d, "word/media/orphan.png", &common::b64(GIF_1X1))
    } else {
        d
    }
}

/// 主 part 里第一个 `w:drawing`。
fn first_drawing(s: &EditSession) -> rsword::xml::NodeId {
    let dom = s.dom();
    dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::Drawing))).expect("drawing")
}

/// 正文里第一个满足 `pred` 的块的节点。
fn block_node(s: &EditSession, pred: impl Fn(&Block) -> bool) -> rsword::xml::NodeId {
    s.document().main.iter().find(|b| pred(b)).expect("block").node()
}

/// 新图片：媒体 part、`image` 关系、`Default` 内容类型、随文段落（`effectExtent` 记旋转外接框），`pPr` 的 spacing / jc；
/// 同一字节插两次只有一个媒体 part、两个 run。
#[test]
fn edit_03_new_images_share_one_media_part_per_content() {
    let src = common::docx_with_body(r#"<w:p><w:r><w:t>前文</w:t></w:r></w:p>"#);
    let mut s = open(&src);
    let p = first_para(&s);
    let rotated = NewImage {
        rot_deg: Some(90),
        flip_h: true,
        align: Some("center".into()),
        para_spacing: Some(ParaSpacing {
            before_twips: Some(240),
            after_twips: Some(120),
            line_twips: Some(360),
            line_rule: Some("exact".into()),
        }),
        ..image(png())
    };
    s.apply_all(
        vec![
            EditOp::InsertBlock { at: BlockPos::after(p), block: NewBlock::Image(image(png())) },
            EditOp::InsertBlock { at: BlockPos::after(p), block: NewBlock::Image(rotated) },
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Image(NewImage {
                    mime: "image/gif".into(),
                    ..image(common::b64(GIF_1X1))
                }),
            },
        ],
        &EditContext::default(),
    )
    .expect("insert images");
    let saved = s.save().expect("save");
    // 同一字节只建一个 png part；gif 另建一个
    let media = media_entries(&saved);
    assert_eq!(
        media,
        vec!["word/media/image1.png".to_string(), "word/media/image2.gif".to_string()],
        "{media:?}"
    );
    let ct = text(&saved, "[Content_Types].xml");
    assert!(ct.contains(r#"Extension="png""#) && ct.contains(r#"Extension="gif""#), "{ct}");
    let rels = text(&saved, "word/_rels/document.xml.rels");
    assert_eq!(rels.matches("media/image1.png").count(), 1, "去重后只有一条关系：{rels}");
    let doc = text(&saved, "word/document.xml");
    // 旋转 90° 的 64×32 px：外接框 32×64 → effectExtent 左右 0、上下 (609600−304800)/2 = 152400
    assert!(doc.contains(r#"<wp:effectExtent l="0" t="152400" r="0" b="152400"/>"#), "{doc}");
    assert!(doc.contains(r#"<a:xfrm rot="5400000" flipH="1">"#), "{doc}");
    assert!(doc.contains(r#"<w:pPr><w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="exact"/><w:jc w:val="center"/></w:pPr>"#), "{doc}");
    assert!(
        doc.contains(r#"<wp:docPr id="1" name="Picture 1"/>"#)
            && doc.contains(r#"<wp:docPr id="3" name="Picture 3"/>"#),
        "{doc}"
    );
    let v = parsed(&saved);
    let imgs: Vec<&Value> =
        v["blocks"].as_array().unwrap().iter().filter(|b| b["type"] == "image").collect();
    assert_eq!(imgs.len(), 3, "{}", v["blocks"]);
    assert!(
        imgs.iter()
            .all(|b| b["imageDataUrl"].as_str().is_some_and(|u| u.starts_with("data:image/")))
    );
    assert_eq!(imgs[1]["imageWidthPx"], 64);
}

/// 九种 `wrap` → `wp:anchor`：位置、绕排元素、`behindDoc`、`relativeHeight`、`posOffsetEmu`。
#[test]
fn edit_03_anchored_images_follow_the_ts_wrap_templates() {
    let src = common::docx_with_body(r#"<w:p><w:r><w:t>前文</w:t></w:r></w:p>"#);
    let mut s = open(&src);
    let p = first_para(&s);
    let mk = |wrap: ImageWrap, pos: Option<PosOffset>, z: Option<i64>| EditOp::InsertBlock {
        at: BlockPos::after(p),
        block: NewBlock::Image(NewImage {
            wrap: Some(wrap),
            pos_offset_emu: pos,
            z_order: z,
            ..image(png())
        }),
    };
    s.apply_all(
        vec![
            mk(ImageWrap::SquareLeft, None, None),
            mk(
                ImageWrap::SquareRight,
                Some(PosOffset { x: 100_000, y: 200_000, page: false }),
                Some(5),
            ),
            mk(ImageWrap::TopBottom, None, None),
            mk(ImageWrap::Behind, Some(PosOffset { x: 0, y: 0, page: true }), None),
            mk(ImageWrap::TightLeft, None, None),
        ],
        &EditContext::default(),
    )
    .unwrap();
    let doc = text(&s.save().unwrap(), "word/document.xml");
    let anchors: Vec<&str> = doc.split("<wp:anchor").skip(1).collect();
    assert_eq!(anchors.len(), 5, "{doc}");
    // 插入顺序与文档顺序相反（都插在同一段之后）
    assert!(
        anchors[4].contains(
            r#"<wp:positionH relativeFrom="column"><wp:align>left</wp:align></wp:positionH>"#
        ) && anchors[4].contains(r#"<wp:wrapSquare wrapText="bothSides"/>"#),
        "{}",
        anchors[4]
    );
    assert!(
        anchors[3].contains(r#"relativeHeight="251658245""#)
            && anchors[3].contains(r#"<wp:posOffset>100000</wp:posOffset>"#)
            && anchors[3].contains(r#"<wp:posOffset>200000</wp:posOffset>"#),
        "{}",
        anchors[3]
    );
    assert!(
        anchors[2].contains(r#"<wp:align>center</wp:align>"#)
            && anchors[2].contains("<wp:wrapTopAndBottom/>"),
        "{}",
        anchors[2]
    );
    assert!(
        anchors[1].contains(r#"behindDoc="1""#)
            && anchors[1].contains("<wp:wrapNone/>")
            && anchors[1].contains(r#"<wp:positionH relativeFrom="page">"#),
        "{}",
        anchors[1]
    );
    assert!(
        anchors[0].contains(r#"<wp:wrapSquare wrapText="bothSides"/>"#),
        "tight 也写成 wrapSquare（TS 同）：{}",
        anchors[0]
    );
    // 绕排元素在 effectExtent 之后、docPr 之前
    assert!(
        anchors[2].contains(
            r#"<wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapTopAndBottom/><wp:docPr"#
        )
    );
}

/// `ReplaceImageMedia`：改指新媒体、删 `a:srcRect`、清 `a:fillRect` 属性；保存后旧媒体 part 与关系被回收。
/// 反复替换只剩最新一份（TS `resource-cleanup` 「keeps only the newest media part」）。
#[test]
fn edit_04_replace_image_media_and_prune_the_old_part() {
    let src = docx_with_picture(1, false);
    let mut s = open(&src);
    let drawing = first_drawing(&s);
    s.apply(
        EditOp::ReplaceImageMedia {
            drawing,
            bytes: common::b64(GIF_1X1),
            mime: "image/gif".into(),
        },
        &EditContext::default(),
    )
    .expect("replace");
    let saved = s.save().unwrap();
    let doc = text(&saved, "word/document.xml");
    assert!(doc.contains(r#"<a:blip r:embed="rId11"/>"#), "{doc}");
    assert!(!doc.contains("<a:srcRect"), "裁剪窗删掉：{doc}");
    assert!(doc.contains("<a:fillRect/>"), "填充窗属性清空：{doc}");
    assert_eq!(media_entries(&saved), vec!["word/media/image2.gif".to_string()], "旧媒体回收");
    let rels = text(&saved, "word/_rels/document.xml.rels");
    assert!(!rels.contains(r#"Id="rId10""#) && rels.contains(r#"Id="rId11""#), "{rels}");
    assert!(text(&saved, "word/document.xml").contains("后文"));
    // 再换一次：还是只剩最新的
    let mut s = open(&saved);
    let drawing = first_drawing(&s);
    s.apply(
        EditOp::ReplaceImageMedia { drawing, bytes: png(), mime: "image/png".into() },
        &EditContext::default(),
    )
    .unwrap();
    let saved2 = s.save().unwrap();
    assert_eq!(
        media_entries(&saved2),
        vec!["word/media/image1.png".to_string()],
        "{:?}",
        names(&saved2)
    );
    let v = parsed(&saved2);
    assert!(v["blocks"][0]["imageDataUrl"].as_str().unwrap().starts_with("data:image/png"));
}

/// `r:link` 外链图替换后变内嵌；`asvg:svgBlip` 扩展被丢弃；目标里没有 `a:blip` → 不动 + 诊断。
#[test]
fn edit_04_replace_image_edge_cases() {
    let para = |blip: &str| {
        format!(
            r#"<w:p><w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}"><wp:inline><wp:extent cx="914400" cy="914400"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic xmlns:pic="{PIC}"><pic:blipFill>{blip}<a:stretch><a:fillRect/></a:stretch></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        )
    };
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId30" Type="{R}/image" Target="http://example.com/a.png" TargetMode="External"/><Relationship Id="rId10" Type="{R}/image" Target="media/image1.png"/><Relationship Id="rId11" Type="{R}/image" Target="media/image1.svg"/></Relationships>"#
    );
    let body = format!(
        "{}{}{}",
        para(r#"<a:blip r:link="rId30"/>"#),
        para(r#"<a:blip r:embed="rId10"><a:extLst><a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}"><asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" r:embed="rId11"/></a:ext></a:extLst></a:blip>"#),
        r#"<w:p><w:r><w:drawing xmlns:wp="{WP}"><wp:inline><wp:extent cx="1" cy="1"/></wp:inline></w:drawing></w:r></w:p>"#.replace("{WP}", WP)
    );
    let d = common::docx_with_parts(&body, &[("word/_rels/document.xml.rels", rels.as_str())]);
    let d = common::with_binary_part(&d, "word/media/image1.png", &png());
    let d = common::with_binary_part(&d, "word/media/image1.svg", b"<svg/>");
    let mut s = open(&d);
    let drawings: Vec<rsword::xml::NodeId> = {
        let dom = s.dom();
        dom.descendants(dom.root()).filter(|&n| dom.is(n, QName::w(LocalName::Drawing))).collect()
    };
    assert_eq!(drawings.len(), 3);
    for &dr in &drawings {
        s.apply(
            EditOp::ReplaceImageMedia { drawing: dr, bytes: png(), mime: "image/png".into() },
            &EditContext::default(),
        )
        .unwrap();
    }
    assert!(
        s.diagnostics().iter().any(|d| d.code == DiagCode::EditUnsupported),
        "没有 a:blip 的目标记诊断"
    );
    let saved = s.save().unwrap();
    let doc = text(&saved, "word/document.xml");
    assert!(!doc.contains("r:link="), "外链变内嵌：{doc}");
    assert!(
        !doc.contains("svgBlip") && !doc.contains("<a:extLst"),
        "svg 扩展与空 extLst 删掉：{doc}"
    );
    assert_eq!(doc.matches(r#"r:embed="rId31""#).count(), 2, "同一字节两处共用一个 rId：{doc}");
    assert!(doc.contains(r#"<wp:extent cx="1" cy="1"/>"#), "没有 blip 的绘图原样");
    // 旧媒体（png / svg）都被回收，只剩新的一份
    assert_eq!(
        media_entries(&saved),
        vec!["word/media/image2.png".to_string()],
        "{:?}",
        names(&saved)
    );
}

/// 删掉图表段落：图表 part、它的 `.rels`、工作簿与 Override 全部消失，其他条目 CRC 不变。
#[test]
fn save_07_deleting_a_chart_prunes_its_subgraph() {
    let src = common::docx_with_body(r#"<w:p><w:r><w:t>前文</w:t></w:r></w:p>"#);
    let mut s = open(&src);
    let p = first_para(&s);
    let chart = NewChart {
        kind: NewChartKind::Bar,
        title: Some("Sales".into()),
        categories: vec!["Q1".into(), "Q2".into()],
        series: vec![NewChartSeries { name: "East".into(), values: vec![Some(1.0), Some(2.0)] }],
    };
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::after(p),
            block: NewBlock::Chart { chart, extent_emu: None },
        },
        &EditContext::default(),
    )
    .unwrap();
    let with_chart = s.save().unwrap();
    assert!(names(&with_chart).iter().any(|n| n == "word/charts/embeddings/workbook1.xlsx"));

    let mut s = open(&with_chart);
    let chart_para =
        block_node(&s, |b| matches!(b, Block::Protected(pb) if pb.kind == ProtectedKind::Chart));
    s.apply(EditOp::DeleteBlock { part: None, node: chart_para }, &EditContext::default()).unwrap();
    let pruned = s.save().unwrap();
    let left = names(&pruned);
    for gone in [
        "word/charts/chart1.xml",
        "word/charts/_rels/chart1.xml.rels",
        "word/charts/embeddings/workbook1.xlsx",
    ] {
        assert!(!left.iter().any(|n| n == gone), "{gone} 应被回收：{left:?}");
    }
    assert!(!text(&pruned, "[Content_Types].xml").contains("charts/chart1.xml"), "Override 删掉");
    assert!(
        !text(&pruned, "word/_rels/document.xml.rels").contains("charts/chart1.xml"),
        "关系删掉"
    );
    // 其他条目 CRC 不变
    let before = crcs(&with_chart);
    let after = crcs(&pruned);
    for (name, crc) in before {
        if matches!(
            name.as_str(),
            "word/document.xml" | "word/_rels/document.xml.rels" | "[Content_Types].xml"
        ) || name.starts_with("word/charts/")
        {
            continue;
        }
        assert_eq!(after.iter().find(|(n, _)| *n == name).map(|(_, c)| *c), Some(crc), "{name}");
    }
    let blocks = parsed(&pruned)["blocks"].as_array().unwrap().clone();
    assert_eq!(blocks.len(), 1, "只剩前文：{blocks:?}");
}

/// 删掉带图段落 → 媒体回收；另一段仍引用同一媒体 → 保留；原本就是孤儿的 part 一个字节不动；`prune_orphans: false` 全留。
#[test]
fn save_07_prunes_only_what_this_session_orphaned() {
    // 两段共用一张图（rId10），外加一个预先存在的孤儿 part（rId99 → orphan.png）
    let src = docx_with_picture(2, true);
    let mut s = open(&src);
    let first = block_node(&s, |b| matches!(b, Block::Image(_)));
    s.apply(EditOp::DeleteBlock { part: None, node: first }, &EditContext::default()).unwrap();
    let saved = s.save().unwrap();
    let media = media_entries(&saved);
    assert!(media.iter().any(|n| n == "word/media/image1.png"), "仍被引用：{media:?}");
    assert!(
        media.iter().any(|n| n == "word/media/orphan.png"),
        "原本就是孤儿的 part 不动：{media:?}"
    );
    let rels = text(&saved, "word/_rels/document.xml.rels");
    assert!(rels.contains(r#"Id="rId99""#) && rels.contains(r#"Id="rId10""#), "{rels}");
    assert_eq!(entry(&saved, "word/media/orphan.png"), common::b64(GIF_1X1));

    // 现在删掉最后一处引用 → image1 回收，孤儿照旧
    let mut s = open(&saved);
    let last = block_node(&s, |b| matches!(b, Block::Image(_)));
    s.apply(EditOp::DeleteBlock { part: None, node: last }, &EditContext::default()).unwrap();
    let saved2 = s.save().unwrap();
    let media = media_entries(&saved2);
    assert!(!media.iter().any(|n| n == "word/media/image1.png"), "{media:?}");
    assert!(media.iter().any(|n| n == "word/media/orphan.png"), "{media:?}");
    let rels = text(&saved2, "word/_rels/document.xml.rels");
    assert!(!rels.contains(r#"Id="rId10""#) && rels.contains(r#"Id="rId99""#), "{rels}");
    let blocks = parsed(&saved2)["blocks"].as_array().unwrap().clone();
    assert_eq!(blocks.len(), 1, "只剩后文：{blocks:?}");

    // 关掉回收：什么都留着
    let mut s = open(&saved);
    let last = block_node(&s, |b| matches!(b, Block::Image(_)));
    s.apply(EditOp::DeleteBlock { part: None, node: last }, &EditContext::default()).unwrap();
    let kept =
        s.save_with(&SaveOptions { prune_orphans: Some(false), ..SaveOptions::default() }).unwrap();
    assert!(media_entries(&kept).iter().any(|n| n == "word/media/image1.png"));
    assert!(text(&kept, "word/_rels/document.xml.rels").contains(r#"Id="rId10""#));
}
