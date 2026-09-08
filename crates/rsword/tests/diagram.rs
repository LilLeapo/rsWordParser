//! SmartArt 与绘图画布（`MOD-11` / `COMPAT-03`，`spec/17` 任务 6.3）。
//!
//! 语料级：每份带 SmartArt / 画布块的文档，`parsed_doc` 在图示域（块上的一切字段）与 TS golden 无未知差异。
//! 构造级：没有 golden 的部分——带绘图 part 的 SmartArt（连线 / 图片填充 / 主题色 / 零尺寸）、绘图 part 的两种
//! 定位方式、分栏启发式的画布（期望 px 按 `docs/01` §8.5 的公式手算，见注释）、同段其他绘图、悬空关系；
//! 两份 hostile（成环的 `dgm:cxn`、退化的画布）解析与投影都不崩。

mod common;

#[cfg(feature = "compat-ts")]
use rsword::bind::compat_ts::{
    EmbeddedKind, block_of_path, diff_json, embedded_kind, known_diffs, parsed_doc, split_known,
};
use rsword::model::{Block, Display, Document, ProtectedKind};
use rsword::package::Package;
#[cfg(feature = "compat-ts")]
use serde_json::{Value, json};

const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
#[cfg(feature = "compat-ts")]
const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
#[cfg(feature = "compat-ts")]
const DSP: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
const LC: &str = "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas";
#[cfg(feature = "compat-ts")]
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

#[cfg(feature = "compat-ts")]
fn parsed(docx: &[u8]) -> Value {
    let mut pkg = Package::open(docx).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

#[cfg(feature = "compat-ts")]
fn is_diagram_block(b: &Value) -> bool {
    matches!(embedded_kind(b), Some(EmbeddedKind::SmartArt | EmbeddedKind::Canvas))
}

/// `COMPAT-03`：语料里每个 SmartArt / 画布文档的图示域差异为 0（`m6-canvas__006` 按路径登记）。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_diagram_projection_matches_ts_across_the_corpus() {
    let known = known_diffs();
    let (mut docs, mut displays, mut previews) = (0, 0, 0);
    let mut unknown: Vec<String> = Vec::new();
    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).expect("expected.json");
        let blocks = expected.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
        let mine: Vec<&Value> = blocks.iter().filter(|b| is_diagram_block(b)).collect();
        if mine.is_empty() {
            continue;
        }
        docs += 1;
        displays += mine.iter().filter(|b| b.get("diagramDisplay").is_some()).count();
        previews += mine.iter().filter(|b| b.get("previewText").is_some()).count();
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).expect("open");
        let actual = parsed_doc(&mut pkg).expect("parsed_doc");
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (diffs, _) = split_known(diffs, &file, &known);
        for d in diffs
            .into_iter()
            .filter(|d| block_of_path(&d.path, &expected).is_some_and(is_diagram_block))
        {
            unknown.push(format!("{file}: {} TS={:?} ours={:?}", d.path, d.expected, d.actual));
        }
    }
    eprintln!("diagram: {docs} 份文档，{displays} 个 diagramDisplay，{previews} 个 previewText");
    for u in unknown.iter().take(40) {
        eprintln!("diagram: DIFF {u}");
    }
    assert!(docs >= 20 && displays >= 12, "{docs} / {displays}");
    assert!(unknown.is_empty(), "{} 处图示域差异", unknown.len());
}

// ---- 构造：SmartArt ------------------------------------------------------------------------------

#[cfg(feature = "compat-ts")]
fn dgm_pt(id: &str, text: &str, ty: &str) -> String {
    let ty = if ty.is_empty() { String::new() } else { format!(r#" type="{ty}""#) };
    format!(
        r#"<dgm:pt modelId="{id}"{ty}><dgm:t><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></dgm:t></dgm:pt>"#
    )
}

#[cfg(feature = "compat-ts")]
fn data_part() -> String {
    let pts = [
        dgm_pt("root", "Root &amp; team", ""),
        dgm_pt("later", "Later", ""),
        dgm_pt("first", "First", ""),
        dgm_pt("leaf", "Leaf", ""),
        dgm_pt("alone", "Isolated", ""),
        dgm_pt("pres", "IGNORED", "pres"),
    ]
    .concat();
    format!(
        r#"<dgm:dataModel xmlns:dgm="{DGM}" xmlns:a="{A}"><dgm:ptLst>{pts}</dgm:ptLst><dgm:cxnLst><dgm:cxn modelId="e1" type="parOf" srcId="root" destId="later" srcOrd="9"/><dgm:cxn modelId="e2" srcId="root" destId="first" srcOrd="1"/><dgm:cxn modelId="e3" srcId="first" destId="leaf" srcOrd="0"/><dgm:cxn modelId="e4" type="presOf" srcId="root" destId="alone" srcOrd="0"/></dgm:cxnLst></dgm:dataModel>"#
    )
}

#[cfg(feature = "compat-ts")]
fn dsp_sp(sp_pr: &str, tx_body: &str) -> String {
    format!(
        r#"<dsp:sp><dsp:nvSpPr><dsp:cNvPr id="1" name="s"/><dsp:cNvSpPr/></dsp:nvSpPr><dsp:spPr>{sp_pr}</dsp:spPr>{tx_body}</dsp:sp>"#
    )
}

#[cfg(feature = "compat-ts")]
fn drawing_part() -> String {
    let xfrm = |x: i64, y: i64, cx: i64, cy: i64, rot: &str| {
        format!(r#"<a:xfrm{rot}><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#)
    };
    let shapes = [
        // 红色矩形，转 30°，边线 2 px，两段文字 14 pt 深蓝
        dsp_sp(
            &format!(r#"{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln w="19050"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln>"#, xfrm(95250, 190500, 285750, 381000, r#" rot="1800000""#)),
            r#"<dsp:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="1400"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:rPr><a:t>First</a:t></a:r></a:p><a:p><a:r><a:t>Second</a:t></a:r></a:p><a:p><a:r><a:t>  </a:t></a:r></a:p></dsp:txBody>"#,
        ),
        // 主题色 accent2（没有 theme part → 内建 Office 调色板）；ln noFill → 没有线
        dsp_sp(
            &format!(r#"{}<a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent2"/></a:solidFill><a:ln><a:noFill/></a:ln>"#, xfrm(428625, 190500, 285750, 381000, "")),
            "",
        ),
        // 连线：cy=0 允许；线宽没写 → 1 px
        dsp_sp(
            &format!(r#"{}<a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:solidFill><a:srgbClr val="445566"/></a:solidFill></a:ln>"#, xfrm(1095375, 190500, 285750, 0, "")),
            "",
        ),
        // 图片填充：按绘图 part 自己的 rels 解，fillRect 负值 = 出血
        dsp_sp(
            &format!(r#"{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:blipFill><a:blip r:embed="rIdLocalImage"/><a:stretch><a:fillRect l="-10000" t="-20000" r="0" b="0"/></a:stretch></a:blipFill>"#, xfrm(1762125, 190500, 285750, 381000, "")),
            "",
        ),
        // 零尺寸、不是连线 → 丢弃
        dsp_sp(&format!(r#"{}<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom>"#, xfrm(0, 0, 0, 0, "")), ""),
        // 解不出的主题槽位 → TS 的缺省 9AB5E4
        dsp_sp(
            &format!(r#"{}<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#, xfrm(2095500, 190500, 285750, 381000, "")),
            "",
        ),
    ]
    .concat();
    format!(
        r#"<dsp:drawing xmlns:dsp="{DSP}" xmlns:a="{A}" xmlns:r="{R}"><dsp:spTree>{shapes}</dsp:spTree></dsp:drawing>"#
    )
}

#[cfg(feature = "compat-ts")]
fn diagram_run(anchor: Option<(&str, &str)>) -> String {
    let graphic = format!(
        r#"<a:graphic xmlns:a="{A}"><a:graphicData uri="{DGM}"><dgm:relIds xmlns:dgm="{DGM}" xmlns:r="{R}" r:dm="rIdDm" r:lo="rIdLo" r:qs="rIdQs" r:cs="rIdCs"/></a:graphicData></a:graphic>"#
    );
    let body = match anchor {
        None => format!(
            r#"<wp:inline><wp:extent cx="2857500" cy="1905000"/><wp:docPr id="1" name="Graphic 1"/>{graphic}</wp:inline>"#
        ),
        Some((wrap, _)) => format!(
            r#"<wp:anchor simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>190500</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>285750</wp:posOffset></wp:positionV><wp:extent cx="2857500" cy="1905000"/>{wrap}<wp:docPr id="1" name="Graphic 1"/>{graphic}</wp:anchor>"#
        ),
    };
    format!(r#"<w:r><w:drawing xmlns:wp="{WP}">{body}</w:drawing></w:r>"#)
}

/// 锚定的照片（`wrapNone`），SmartArt 的邻居。
#[cfg(feature = "compat-ts")]
fn photo_run() -> String {
    format!(
        r#"<w:r><w:drawing xmlns:wp="{WP}"><wp:anchor simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>190500</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>285750</wp:posOffset></wp:positionV><wp:extent cx="2857500" cy="1905000"/><wp:wrapNone/><wp:docPr id="20" name="Photo"/><a:graphic xmlns:a="{A}"><a:graphicData uri="{PIC}"><pic:pic xmlns:pic="{PIC}"><pic:nvPicPr><pic:cNvPr id="20" name="Photo"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip xmlns:r="{R}" r:embed="rIdImg"/></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="285750" cy="381000"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
    )
}

/// 一份 SmartArt 文档。`drawing_via_rel`：绘图 part 由数据 part 的 `diagramDrawing` 关系指向（真实 Word 的写法），
/// 否则只能靠 `data1.xml → drawing1.xml` 的路径约定；`data_target` 让关系指向不存在的 part。
#[cfg(feature = "compat-ts")]
fn smart_art_docx(paragraph: &str, drawing_via_rel: bool, data_target: &str) -> Vec<u8> {
    let main_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDm" Type="{R}/diagramData" Target="{data_target}"/><Relationship Id="rIdLo" Type="{R}/diagramLayout" Target="diagrams/layout1.xml"/><Relationship Id="rIdImg" Type="{R}/image" Target="media/image1.png"/></Relationships>"#
    );
    let drawing_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLocalImage" Type="{R}/image" Target="../media/image1.png"/></Relationships>"#
    );
    let data_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDrawing" Type="http://schemas.microsoft.com/office/2007/relationships/diagramDrawing" Target="layoutDrawing.xml"/></Relationships>"#;
    // 走关系时把绘图 part 放在约定之外的名字下，证明找到它靠的是关系不是路径
    let (drawing_name, drawing_rels_name) = if drawing_via_rel {
        ("word/diagrams/layoutDrawing.xml", "word/diagrams/_rels/layoutDrawing.xml.rels")
    } else {
        ("word/diagrams/drawing1.xml", "word/diagrams/_rels/drawing1.xml.rels")
    };
    let (data, drawing) = (data_part(), drawing_part());
    let mut parts: Vec<(&str, &str)> = vec![
        ("word/_rels/document.xml.rels", main_rels.as_str()),
        ("word/diagrams/data1.xml", data.as_str()),
        (drawing_name, drawing.as_str()),
        (drawing_rels_name, drawing_rels.as_str()),
    ];
    if drawing_via_rel {
        parts.push(("word/diagrams/_rels/data1.xml.rels", data_rels));
    }
    let docx = common::docx_with_parts(paragraph, &parts);
    common::with_binary_part(&docx, "word/media/image1.png", &common::b64(common::PNG_1X1))
}

#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_smart_art_text_tree_and_drawing_part_shapes() {
    let para = format!("<w:p>{}</w:p>", diagram_run(None));
    for via_rel in [true, false] {
        let docx = smart_art_docx(&para, via_rel, "diagrams/data1.xml");
        // 模型：数据 part 与绘图 part 都找到了
        let mut pkg = Package::open(&docx).expect("open");
        let doc = Document::rebuild(&mut pkg).expect("rebuild");
        let dp = doc.diagram_parts.values().next().expect("diagram part");
        assert!(dp.drawing.is_some(), "via_rel={via_rel}: 绘图 part 没找到");
        assert_eq!(dp.text.as_deref(), Some("Root & team\nFirst\nLeaf\nLater\nIsolated"));
        assert_eq!(dp.shapes.as_ref().map(Vec::len), Some(6), "零尺寸的形状在模型里还在，投影时丢");

        let v = parsed(&docx);
        let b = &v["blocks"][0];
        assert_eq!(b["type"], "passthrough");
        assert_eq!(b["label"], "SmartArt");
        // 树序：root → first(srcOrd 1) → leaf → later(srcOrd 9)；presOf 边不算；alone 没进树，按文件序追加
        assert_eq!(b["previewText"], "Root & team\nFirst\nLeaf\nLater\nIsolated");
        let dd = &b["diagramDisplay"];
        assert_eq!((dd["widthPx"].as_i64(), dd["heightPx"].as_i64()), (Some(300), Some(200)));
        let shapes = dd["shapes"].as_array().expect("shapes");
        assert_eq!(shapes.len(), 5, "{shapes:#?}");
        // 95250/9525 = 10, 190500/9525 = 20, 285750/9525 = 30, 381000/9525 = 40；1800000/60000 = 30°；19050/9525 = 2
        assert_eq!(
            shapes[0],
            json!({ "xPx": 10, "yPx": 20, "wPx": 30, "hPx": 40, "prst": "rect", "rotDeg": 30, "fillHex": "FF0000",
                    "lnHex": "123456", "lnWPx": 2, "texts": ["First", "Second"], "fontSizePt": 14, "textColorHex": "112233" })
        );
        assert_eq!(
            shapes[1],
            json!({ "xPx": 45, "yPx": 20, "wPx": 30, "hPx": 40, "prst": "roundRect", "fillHex": "ED7D31" })
        );
        assert_eq!(
            shapes[2],
            json!({ "xPx": 115, "yPx": 20, "wPx": 30, "hPx": 0, "prst": "line", "lnHex": "445566", "lnWPx": 1 })
        );
        assert_eq!(shapes[3]["prst"], "rect");
        assert!(
            shapes[3]["imageDataUrl"]
                .as_str()
                .is_some_and(|u| u.starts_with("data:image/png;base64,")),
            "{}",
            shapes[3]
        );
        assert_eq!(shapes[3]["fillRect"], json!({ "l": -0.1, "t": -0.2, "r": 0, "b": 0 }));
        assert!(shapes[3].get("fillHex").is_none());
        assert_eq!(
            shapes[4],
            json!({ "xPx": 220, "yPx": 20, "wPx": 30, "hPx": 40, "prst": "ellipse", "fillHex": "9AB5E4" })
        );
        assert!(b.get("textboxes").is_none(), "单绘图段落没有 textboxes");
        assert!(dd.get("floating").is_none() && dd.get("offsetXEmu").is_none());
    }
}

/// 同段其他绘图：照片进 `textboxes[]`（各自的锚点），图示自己锚定时 `diagramDisplay` 带偏移与 `floating`；
/// 单绘图的锚定图示不给这些（TS 的 `frags.length > 1` 闸门）。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_smart_art_siblings_become_textboxes_and_anchor_offsets() {
    let square = r#"<wp:wrapSquare wrapText="bothSides"/>"#;
    let para = format!("<w:p>{}{}</w:p>", diagram_run(Some((square, ""))), photo_run());
    let v = parsed(&smart_art_docx(&para, true, "diagrams/data1.xml"));
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "SmartArt");
    let dd = &b["diagramDisplay"];
    assert_eq!(dd["floating"], true);
    assert_eq!(
        (dd["offsetXEmu"].as_i64(), dd["offsetYEmu"].as_i64()),
        (Some(190_500), Some(285_750))
    );
    let boxes = b["textboxes"].as_array().expect("textboxes");
    assert_eq!(boxes.len(), 1, "{boxes:#?}");
    let bx = &boxes[0];
    assert_eq!(bx["readOnly"], true);
    assert_eq!(bx["floating"], true);
    assert_eq!(bx["paras"], json!([]));
    assert!(bx["fillImageDataUrl"].as_str().is_some_and(|u| u.starts_with("data:image/png")));
    assert_eq!((bx["widthPx"].as_i64(), bx["heightPx"].as_i64()), (Some(300), Some(200)));
    assert_eq!(
        (bx["offsetXEmu"].as_i64(), bx["offsetYEmu"].as_i64()),
        (Some(190_500), Some(285_750))
    );

    let alone = format!("<w:p>{}</w:p>", diagram_run(Some((square, ""))));
    let v = parsed(&smart_art_docx(&alone, true, "diagrams/data1.xml"));
    let dd = &v["blocks"][0]["diagramDisplay"];
    assert!(
        dd.is_object() && dd.get("floating").is_none() && dd.get("offsetXEmu").is_none(),
        "{dd}"
    );
    assert!(v["blocks"][0].get("textboxes").is_none());
}

/// 关系悬空：块只剩 `label: "SmartArt"`——没有 `previewText`（TS `...(x ? {} : {})`），没有 `diagramDisplay`。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_smart_art_with_dangling_data_relationship_is_a_bare_chip() {
    let para = format!("<w:p>{}</w:p>", diagram_run(None));
    let v = parsed(&smart_art_docx(&para, false, "diagrams/missing.xml"));
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "SmartArt");
    assert_eq!(b["type"], "passthrough");
    assert!(b.get("previewText").is_none(), "{b}");
    assert!(b.get("diagramDisplay").is_none(), "{b}");
}

// ---- 构造：画布 ----------------------------------------------------------------------------------

fn canvas_sp(
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
    rot: &str,
    geom_fill: &str,
    text: Option<&str>,
) -> String {
    let tx = text.map_or(String::new(), |t| {
        format!(r#"<a:txSp><a:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="1800"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:rPr><a:t>{t}</a:t></a:r></a:p></a:txBody></a:txSp>"#)
    });
    format!(
        r#"<a:sp><a:nvSpPr><a:cNvPr id="9" name="s"/><a:cNvSpPr/></a:nvSpPr><a:spPr><a:xfrm{rot}><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>{geom_fill}</a:spPr>{tx}</a:sp>"#
    )
}

/// 子坐标系是摆放尺寸的 3 倍（extent 2857500×1905000 = 300×200 px，chExt 8572500×5715000，chOff 285750,571500）。
fn canvas_run(anchor: Option<&str>, with_extent: bool) -> String {
    let shapes = [
        // 两个文本形状：框 10×5 px，18 pt 的字每行只装 1 个 → 溢出、分栏、逐字拆
        canvas_sp(857_250, 1_143_000, 285_750, 142_875, "", r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#, Some("ABCD")),
        canvas_sp(2_857_500, 1_143_000, 285_750, 142_875, "", r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#, Some("EF")),
        // 椭圆，主题色 accent1（内建调色板 4472C4）
        canvas_sp(3_714_750, 1_143_000, 1_714_500, 1_714_500, "", r#"<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill>"#, None),
        // 转 45° 的矩形（`rect` 不记 prst），渐变两停靠点等权平均：(FF0000 + 0000FF) / 2 = 800080
        canvas_sp(857_250, 3_000_000, 1_714_500, 1_714_500, r#" rot="2700000""#, r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="FF0000"/></a:gs><a:gs pos="100000"><a:srgbClr val="0000FF"/></a:gs></a:gsLst></a:gradFill>"#, None),
        // 图片：媒体按主 part 的关系解
        r#"<a:pic><a:nvPicPr><a:cNvPr id="900" name="p"/><a:cNvPicPr/></a:nvPicPr><a:blipFill><a:blip r:embed="rIdImg"/></a:blipFill><a:spPr><a:xfrm><a:off x="5715000" y="1143000"/><a:ext cx="1714500" cy="1714500"/></a:xfrm></a:spPr></a:pic>"#.to_string(),
    ]
    .concat();
    let canvas = format!(
        r#"<a:graphic xmlns:a="{A}"><a:graphicData uri="{LC}"><lc:lockedCanvas xmlns:lc="{LC}" xmlns:r="{R}"><a:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2857500" cy="1905000"/><a:chOff x="285750" y="571500"/><a:chExt cx="8572500" cy="5715000"/></a:xfrm></a:grpSpPr>{shapes}</lc:lockedCanvas></a:graphicData></a:graphic>"#
    );
    let extent = if with_extent { r#"<wp:extent cx="2857500" cy="1905000"/>"# } else { "" };
    let body = match anchor {
        None => {
            format!(r#"<wp:inline>{extent}<wp:docPr id="1" name="Canvas"/>{canvas}</wp:inline>"#)
        }
        Some(wrap) => format!(
            r#"<wp:anchor simplePos="0" relativeHeight="1" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>190500</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>285750</wp:posOffset></wp:positionV>{extent}{wrap}<wp:docPr id="1" name="Canvas"/>{canvas}</wp:anchor>"#
        ),
    };
    format!(r#"<w:r><w:drawing xmlns:wp="{WP}">{body}</w:drawing></w:r>"#)
}

fn canvas_docx(run: &str) -> Vec<u8> {
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="{R}/image" Target="media/image1.png"/></Relationships>"#
    );
    let docx = common::docx_with_parts(
        &format!("<w:p>{run}</w:p>"),
        &[("word/_rels/document.xml.rels", rels.as_str())],
    );
    common::with_binary_part(&docx, "word/media/image1.png", &common::b64(common::PNG_1X1))
}

/// 画布：子坐标系缩放、`rect` 不记、渐变平均、图片、原字号文字的分栏与逐字拆分。
///
/// 手算（缩放 1/3，再 ÷ 9525）：文本框 A 在 (857250−285750, 1143000−571500) = (571500, 571500) → (20, 20) px，
/// 285750×142875 → 10×5 px；18 pt = 24 px，每行 floor(10 / (24 × 0.72)) = 0 → 1 个字，"ABCD" 4 行 × 28.8 px
/// = 115.2 > 2 × 5 → 溢出；第一列从 y=0 起，第二列（"EF"，x = (2857500−285750)/3/9525 = 90）从
/// round(0 + ceil(4/2) × 28.8 − 23) = 35 起；两列都是一行一个字 → 拆成逐字形状，行高 ceil(28.8) = 29，
/// A 在 0 / 29 / 58 / 86，E 在 35 / 64；最后按 y 排序。椭圆 (3714750−285750)/3/9525 = 120，60×60；
/// 图片 x = (5715000−285750)/3/9525 = 190；矩形 y = (3000000−571500)/3/9525 = 85。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_canvas_scales_child_geometry_and_stacks_overflowing_text_columns() {
    let v = parsed(&canvas_docx(&canvas_run(None, true)));
    let b = &v["blocks"][0];
    assert_eq!(b["type"], "passthrough");
    assert_eq!(b["label"], "Drawing object");
    assert_eq!(b["previewText"], "A\nB\nE\nC\nF\nD");
    let dd = &b["diagramDisplay"];
    assert_eq!(dd["canvas"], true);
    assert_eq!((dd["widthPx"].as_i64(), dd["heightPx"].as_i64()), (Some(300), Some(200)));
    let shapes = dd["shapes"].as_array().expect("shapes");
    let brief: Vec<(i64, i64, Option<&str>)> = shapes
        .iter()
        .map(|s| (s["yPx"].as_i64().unwrap(), s["xPx"].as_i64().unwrap(), s["texts"][0].as_str()))
        .collect();
    assert_eq!(
        brief,
        vec![
            (0, 20, Some("A")),
            (20, 120, None), // 椭圆
            (20, 190, None), // 图片
            (29, 20, Some("B")),
            (35, 90, Some("E")),
            (58, 20, Some("C")),
            (64, 90, Some("F")),
            (85, 20, None), // 转 45° 的渐变矩形
            (86, 20, Some("D")),
        ],
        "{shapes:#?}"
    );
    // 逐字形状：行高 29、原字号、原颜色，没有填充与几何
    assert_eq!(
        shapes[0],
        json!({ "xPx": 20, "yPx": 0, "wPx": 10, "hPx": 29, "texts": ["A"], "fontSizePt": 18, "textColorHex": "112233" })
    );
    assert_eq!(
        shapes[1],
        json!({ "xPx": 120, "yPx": 20, "wPx": 60, "hPx": 60, "prst": "ellipse", "fillHex": "4472C4" })
    );
    assert!(
        shapes[2]["imageDataUrl"].as_str().is_some_and(|u| u.starts_with("data:image/png")),
        "{}",
        shapes[2]
    );
    assert_eq!(
        shapes[7],
        json!({ "xPx": 20, "yPx": 85, "wPx": 60, "hPx": 60, "rotDeg": 45, "fillHex": "800080" })
    );
    assert!(dd.get("offsetXEmu").is_none() && dd.get("floating").is_none());
}

/// 锚定的画布只带横向偏移（LO 丢竖向偏移）；`wrapNone` → `floating`，`wrapSquare` 不浮。
/// 没有 `wp:extent` 时按子坐标系原尺寸画（TS 退成第一张图，`KNOWN_DIFFS.md`）。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_canvas_anchor_offsets_and_missing_extent() {
    let v = parsed(&canvas_docx(&canvas_run(Some("<wp:wrapNone/>"), true)));
    let dd = &v["blocks"][0]["diagramDisplay"];
    assert_eq!(dd["offsetXEmu"], 190_500);
    assert!(dd.get("offsetYEmu").is_none(), "{dd}");
    assert_eq!(dd["floating"], true);

    let v =
        parsed(&canvas_docx(&canvas_run(Some(r#"<wp:wrapSquare wrapText="bothSides"/>"#), true)));
    let dd = &v["blocks"][0]["diagramDisplay"];
    assert_eq!(dd["offsetXEmu"], 190_500);
    assert!(dd.get("floating").is_none(), "{dd}");

    let v = parsed(&canvas_docx(&canvas_run(None, false)));
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "Drawing object");
    let dd = &b["diagramDisplay"];
    // chExt 8572500×5715000 原尺寸 = 900×600 px，缩放 1：椭圆在 ((3714750−285750)/9525, (1143000−571500)/9525) = (360, 60)
    assert_eq!((dd["widthPx"].as_i64(), dd["heightPx"].as_i64()), (Some(900), Some(600)));
    let ellipse =
        dd["shapes"].as_array().unwrap().iter().find(|s| s["prst"] == "ellipse").expect("ellipse");
    assert_eq!((ellipse["xPx"].as_i64(), ellipse["yPx"].as_i64()), (Some(360), Some(60)));
}

/// 模型侧：R13 / R14 的块与 `siblings`，画布形状留子坐标系原值。
#[test]
fn mod_11_canvas_and_diagram_blocks_in_the_model() {
    let docx = canvas_docx(&canvas_run(None, true));
    let mut pkg = Package::open(&docx).expect("open");
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
    let Some(Block::Protected(pb)) = doc.main.first() else { panic!("{:?}", doc.main.first()) };
    assert_eq!(pb.kind, ProtectedKind::SmartArt);
    let d = pb.display.as_ref().and_then(Display::as_drawing).expect("drawing");
    let c = d.canvas.as_deref().expect("canvas");
    assert_eq!(c.ch_off, Some((285_750, 571_500)));
    assert_eq!(c.ch_ext.map(|e| (e.cx, e.cy)), Some((8_572_500, 5_715_000)));
    assert_eq!(c.shapes.len(), 5);
    assert_eq!(c.shapes[0].off_emu, (857_250, 1_143_000));
    assert_eq!(c.shapes[0].texts, vec!["ABCD".to_string()]);
    assert_eq!(c.shapes[0].font_size_100pt, Some(1800));
    assert!(c.shapes[4].picture.as_ref().is_some_and(|p| p.embed.as_deref() == Some("rIdImg")));
    assert!(pb.siblings.is_empty());
}

/// hostile（`TEST-09`）：成环 / 自指 / 5,000 个点的 `dgm:cxn`，退化的画布（chExt 0 / 负数、坐标 `1e30`、
/// `sz=-5`、没有 blip 的 `a:pic`）：解析成功、投影不崩、局部降级。
#[test]
#[cfg(feature = "compat-ts")]
fn test_09_hostile_diagram_and_canvas_degrade_locally() {
    let cyclic =
        std::fs::read(common::corpus_dir("hostile").join("diagram-cyclic-cxn.docx")).unwrap();
    let v = parsed(&cyclic);
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "SmartArt");
    let preview = b["previewText"].as_str().expect("节点文字");
    let lines: Vec<&str> = preview.lines().collect();
    assert!(lines.len() >= 1000, "{} 行", lines.len());
    let unique: std::collections::BTreeSet<&str> = lines.iter().copied().collect();
    assert_eq!(unique.len(), lines.len(), "成环的点重复出现");

    let degenerate =
        std::fs::read(common::corpus_dir("hostile").join("canvas-degenerate.docx")).unwrap();
    let v = parsed(&degenerate);
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "Drawing object", "{b}");
    assert!(b.get("diagramDisplay").is_none(), "退化的形状全部丢弃：{b}");
    assert_eq!(v["blocks"][1]["runs"][0]["text"], "hello");
}
