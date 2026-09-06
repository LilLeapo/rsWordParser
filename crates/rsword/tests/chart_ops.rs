//! 图表的写侧（`EDIT-03` / `SAVE-05` / `SAVE-06` / `SAVE-08`，`spec/17` 任务 6.6）。
//!
//! `NewBlock::Chart`：part / 工作簿 / 两个 `.rels` / 内容类型都在，重解析的 `chartDisplay` 与输入相等，xlsx 能被
//! `zip` 重新打开且 `sheet1.xml` 单元格与数据一致，其他 zip 条目 CRC 不变；`SetChartData`：TS `patchChartPartXml`
//! 的两例 + 三种没有文字的标题的注入形态，part 里未改的字节原样；`ReplacePartXml` / `ReplacePartBytes`：字节即
//! 内容、不存在的 part 报 `EDIT_TARGET_MISSING`、事务失败回滚。

mod common;

use std::io::Read;

use rsword::bind::compat_ts::parsed_doc;
use rsword::diag::DiagCode;
use rsword::edit::{
    BlockPos, ChartPatch, ChartSeriesPatch, EditContext, EditOp, EditSession, NewBlock, NewChart,
    NewChartKind, NewChartSeries,
};
use rsword::error::Error;
use rsword::package::Package;
use serde_json::{Value, json};

fn spec() -> NewChart {
    NewChart {
        kind: NewChartKind::Bar,
        title: Some("季度销售".into()),
        categories: vec!["Q1".into(), "Q2".into(), "Q3".into()],
        series: vec![
            NewChartSeries {
                name: "华东".into(),
                values: vec![Some(120.0), Some(88.5), Some(96.0)],
            },
            NewChartSeries { name: "华南".into(), values: vec![Some(70.0), None, Some(110.0)] },
        ],
    }
}

fn open(bytes: &[u8]) -> EditSession {
    EditSession::open(bytes).expect("open")
}

fn zip_entries(bytes: &[u8]) -> Vec<(String, u32)> {
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

fn first_para(s: &EditSession) -> rsword::xml::NodeId {
    s.nth_text_block(0).expect("paragraph").node
}

/// 新建图表：part、`.rels`、工作簿、内容类型、绘图段落；重解析的显示模型与输入相等；工作簿是合法的 xlsx。
#[test]
fn edit_03_new_chart_creates_parts_and_round_trips() {
    let src = common::docx_with_body(r#"<w:p><w:r><w:t>前文</w:t></w:r></w:p>"#);
    let before = zip_entries(&src);
    let mut s = open(&src);
    let p = first_para(&s);
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::after(p),
            block: NewBlock::Chart { chart: spec(), extent_emu: None },
        },
        &EditContext::default(),
    )
    .expect("insert chart");
    let saved = s.save().expect("save");
    // 包：新 part 追加在末尾，其他条目 CRC 不变（`SAVE-06`）
    let after = zip_entries(&saved);
    for (name, crc) in &before {
        if !matches!(
            name.as_str(),
            "word/document.xml" | "word/_rels/document.xml.rels" | "[Content_Types].xml"
        ) {
            assert_eq!(
                after.iter().find(|(n, _)| n == name).map(|(_, c)| *c),
                Some(*crc),
                "{name} 的 CRC 变了"
            );
        }
    }
    for name in [
        "word/charts/chart1.xml",
        "word/charts/_rels/chart1.xml.rels",
        "word/charts/embeddings/workbook1.xlsx",
    ] {
        assert!(after.iter().any(|(n, _)| n == name), "缺 {name}: {after:?}");
    }
    let ct = text(&saved, "[Content_Types].xml");
    assert!(ct.contains(r#"PartName="/word/charts/chart1.xml""#), "{ct}");
    assert!(ct.contains(r#"Extension="xlsx""#), "{ct}");
    let rels = text(&saved, "word/_rels/document.xml.rels");
    assert!(rels.contains(r#"Target="charts/chart1.xml""#), "{rels}");
    let chart_rels = text(&saved, "word/charts/_rels/chart1.xml.rels");
    assert!(
        chart_rels.contains(r#"Id="rId1""#) && chart_rels.contains("embeddings/workbook1.xlsx"),
        "{chart_rels}"
    );
    let chart_xml = text(&saved, "word/charts/chart1.xml");
    assert!(
        chart_xml.contains(r#"<c:externalData r:id="rId1">"#) && chart_xml.contains("<c:catAx>"),
        "{chart_xml}"
    );
    // 工作簿：合法 xlsx，A 列类别、B/C 列系列，首行系列名
    let xlsx = entry(&saved, "word/charts/embeddings/workbook1.xlsx");
    let sheet = text(&xlsx, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(r#"<c r="B2"><v>120</v></c>"#)
            && sheet.contains(r#"<c r="B3"><v>88.5</v></c>"#),
        "{sheet}"
    );
    assert!(!sheet.contains(r#"<c r="C3""#), "空档不写单元格：{sheet}");
    let shared = text(&xlsx, "xl/sharedStrings.xml");
    for s in ["Q1", "华东", "华南"] {
        assert!(shared.contains(s), "{shared}");
    }
    assert!(text(&xlsx, "xl/workbook.xml").contains(r#"<sheet name="Sheet1""#));
    // 重解析：显示模型与输入相等，绘图段落尺寸是缺省值
    let v = parsed(&saved);
    let b = &v["blocks"][1];
    assert_eq!(b["label"], "Chart", "{b}");
    let cd = &b["chartDisplay"];
    assert_eq!(cd["kind"], "bar");
    assert_eq!(cd["title"], "季度销售");
    assert_eq!(cd["categories"], json!(["Q1", "Q2", "Q3"]));
    assert_eq!(
        cd["series"],
        json!([{ "name": "华东", "values": [120, 88.5, 96] }, { "name": "华南", "values": [70, null, 110] }])
    );
    assert_eq!((cd["widthPx"].as_i64(), cd["heightPx"].as_i64()), (Some(576), Some(336)));
    assert_eq!(v["extras"]["chartParts"].as_object().map(|m| m.len()), Some(1));
}

/// 同一次保存两个图表：part 名 chart1 / chart2，`docPr/@id` 递增，饼图没有轴，`extent_emu` 生效。
#[test]
fn edit_03_two_charts_get_distinct_parts_and_ids() {
    let src = common::docx_with_body(r#"<w:p><w:r><w:t>前文</w:t></w:r></w:p>"#);
    let mut s = open(&src);
    let p = first_para(&s);
    let pie = NewChart { kind: NewChartKind::Pie, title: None, ..spec() };
    s.apply_all(
        vec![
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Chart { chart: spec(), extent_emu: None },
            },
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Chart { chart: pie, extent_emu: Some((2_857_500, 1_905_000)) },
            },
        ],
        &EditContext::default(),
    )
    .expect("two charts");
    let saved = s.save().expect("save");
    let names: Vec<String> = zip_entries(&saved).into_iter().map(|(n, _)| n).collect();
    for n in [
        "word/charts/chart1.xml",
        "word/charts/chart2.xml",
        "word/charts/embeddings/workbook1.xlsx",
        "word/charts/embeddings/workbook2.xlsx",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
    let doc = text(&saved, "word/document.xml");
    assert!(
        doc.contains(r#"<wp:docPr id="1" name="Chart 1"/>"#)
            && doc.contains(r#"<wp:docPr id="2" name="Chart 2"/>"#),
        "{doc}"
    );
    let v = parsed(&saved);
    let kinds: Vec<&str> = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["chartDisplay"]["kind"].as_str())
        .collect();
    assert_eq!(kinds, vec!["pie", "bar"]);
    let pie_block = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["chartDisplay"]["kind"] == "pie")
        .unwrap();
    assert_eq!(
        (
            pie_block["chartDisplay"]["widthPx"].as_i64(),
            pie_block["chartDisplay"]["heightPx"].as_i64()
        ),
        (Some(300), Some(200))
    );
    assert!(pie_block["chartDisplay"].get("title").is_none());
    let chart2 = text(&saved, "word/charts/chart2.xml");
    let pie_xml = if chart2.contains("<c:pieChart>") {
        chart2
    } else {
        text(&saved, "word/charts/chart1.xml")
    };
    assert!(pie_xml.contains("<c:pieChart>") && !pie_xml.contains("<c:catAx>"), "{pie_xml}");
}

/// 一份带图表 part 的文档（`chart-edit__001`），以及它的图表 part。
fn chart_edit_docx() -> Vec<u8> {
    std::fs::read(common::corpus_dir("synthetic").join("chart-edit__001.docx")).unwrap()
}

fn chart_part_of(s: &EditSession) -> rsword::package::PartId {
    *s.document().chart_parts.keys().next().expect("chart part")
}

/// TS `patchChartPartXml` 第一例：标题 / 系列名 / 值 / 类别都改，引用与结构原样，未改的字节一个不动。
#[test]
fn edit_03_set_chart_data_patches_cached_texts_only() {
    let src = chart_edit_docx();
    let original_part = text(&src, "word/charts/chart1.xml");
    let mut s = open(&src);
    let part = chart_part_of(&s);
    s.apply(
        EditOp::SetChartData {
            part,
            patch: ChartPatch {
                title: Some("2026 营收 & 目标".into()),
                categories: Some(vec![None, Some("2月".into()), None]),
                series: Some(vec![
                    Some(ChartSeriesPatch {
                        name: Some("华东大区".into()),
                        values: Some(vec![None, Some(99.0), None]),
                    }),
                    None,
                ]),
            },
        },
        &EditContext::default(),
    )
    .expect("patch");
    let saved = s.save().expect("save");
    let patched = text(&saved, "word/charts/chart1.xml");
    for keep in [
        "<c:f>Sheet1!$B$2:$B$4</c:f>",
        "<c:formatCode>General</c:formatCode>",
        r#"<c:barDir val="col"/>"#,
    ] {
        assert!(patched.contains(keep), "{keep} 应原样：{patched}");
    }
    assert!(patched.contains("<a:t>2026 营收 &amp; 目标</a:t>"), "{patched}");
    for got in [
        "<c:v>华东大区</c:v>",
        "<c:v>华南</c:v>",
        "<c:v>99</c:v>",
        "<c:v>120</c:v>",
        "<c:v>2月</c:v>",
    ] {
        assert!(patched.contains(got), "{got} in {patched}");
    }
    assert!(!patched.contains("<c:v>88.5</c:v>"));
    // 除了被改的文本，字节原样（`SAVE-08` 抽样）：把新文本换回旧文本应恢复原 part
    let back = patched
        // 原标题是两个 run「销售」「统计」：整个标题进第一个 run、第二个清空（TS 同）
        .replace(
            "<a:t>2026 营收 &amp; 目标</a:t></a:r><a:r><a:t></a:t>",
            "<a:t>销售</a:t></a:r><a:r><a:t>统计</a:t>",
        )
        .replace("<c:v>华东大区</c:v>", "<c:v>华东</c:v>")
        .replace("<c:v>99</c:v>", "<c:v>88.5</c:v>")
        .replacen("<c:v>2月</c:v>", "<c:v>二月</c:v>", 2);
    assert_eq!(back, original_part, "只有缓存文本变了");
    let v = parsed(&saved);
    let cd = &v["blocks"][0]["chartDisplay"];
    assert_eq!(cd["title"], "2026 营收 & 目标");
    assert_eq!(cd["categories"], json!(["一月", "2月", "三月"]));
    assert_eq!(cd["series"][0], json!({ "name": "华东大区", "values": [120, 99, 96] }));
    assert_eq!(cd["series"][1]["name"], "华南");
    // 主 part 与样式 part 原字节
    assert_eq!(entry(&saved, "word/document.xml"), entry(&src, "word/document.xml"));
    assert_eq!(entry(&saved, "word/styles.xml"), entry(&src, "word/styles.xml"));
}

/// TS 第二例：锚不到的（缓存里缺的点、没有标题的 part）与空补丁都不产生变化。
#[test]
fn edit_03_set_chart_data_ignores_what_it_cannot_anchor() {
    let src = chart_edit_docx();
    let mut s = open(&src);
    let part = chart_part_of(&s);
    // 华南系列的第 1 个点在缓存里就是空档：补丁值落空，其他点没给 → 一个字节不变
    s.apply(
        EditOp::SetChartData {
            part,
            patch: ChartPatch {
                series: Some(vec![
                    None,
                    Some(ChartSeriesPatch {
                        name: None,
                        values: Some(vec![None, Some(42.0), None]),
                    }),
                ]),
                ..ChartPatch::default()
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    s.apply(EditOp::SetChartData { part, patch: ChartPatch::default() }, &EditContext::default())
        .unwrap();
    assert_eq!(s.save().unwrap(), src, "没有可锚定的编辑 → 原字节");
}

/// 图表 part 的段落 `chart_inner` 包成一份最小文档。
fn docx_with_chart(chart_inner: &str) -> Vec<u8> {
    const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    let para = format!(
        r#"<w:p><w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}"><wp:inline><wp:extent cx="2857500" cy="1905000"/><wp:docPr id="1" name="Chart 1"/><a:graphic><a:graphicData uri="{C}"><c:chart xmlns:c="{C}" r:id="rIdChart"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdChart" Type="{R}/chart" Target="charts/chart1.xml"/></Relationships>"#
    );
    let part = format!(
        r#"<c:chartSpace xmlns:c="{C}" xmlns:a="{A}" xmlns:r="{R}"><c:chart>{chart_inner}</c:chart></c:chartSpace>"#
    );
    common::docx_with_parts(
        &para,
        &[
            ("word/_rels/document.xml.rels", rels.as_str()),
            ("word/charts/chart1.xml", part.as_str()),
        ],
    )
}

const PLOT: &str = r#"<c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>S!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>S1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>3</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea>"#;

fn set_title(bytes: &[u8], title: &str) -> String {
    let mut s = open(bytes);
    let part = chart_part_of(&s);
    s.apply(
        EditOp::SetChartData {
            part,
            patch: ChartPatch { title: Some(title.into()), ..ChartPatch::default() },
        },
        &EditContext::default(),
    )
    .expect("set title");
    text(&s.save().unwrap(), "word/charts/chart1.xml")
}

/// 没有文字的标题的三种形态（TS `chart-insert.test.ts` 的标题补丁）：自动标题的空 rich 段落 → run 插在
/// `a:endParaRPr` 之前；无缓存的 `strRef` → 整个 `c:tx` 换成 rich body；没有 `c:tx` → rich body 插为第一个子元素。
/// strRef 有缓存 → 改 `c:v`。
#[test]
fn edit_03_set_chart_data_injects_text_into_textless_titles() {
    let auto = docx_with_chart(&format!(
        r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr/></a:pPr><a:endParaRPr lang="en-US"/></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>{PLOT}"#
    ));
    let out = set_title(&auto, "新标题");
    assert!(out.contains(r#"<a:pPr><a:defRPr/></a:pPr><a:r><a:t>新标题</a:t></a:r><a:endParaRPr lang="en-US"/></a:p>"#), "{out}");

    let str_ref = docx_with_chart(&format!(
        r#"<c:title><c:tx><c:strRef><c:f>Sheet1!$A$1</c:f></c:strRef></c:tx><c:overlay val="0"/></c:title>{PLOT}"#
    ));
    let out = set_title(&str_ref, "新标题");
    assert!(out.contains(r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>新标题</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>"#), "{out}");
    assert!(!out.contains("<c:strRef><c:f>Sheet1!$A$1"), "无缓存的 strRef 整个换掉：{out}");

    let bare = docx_with_chart(&format!(r#"<c:title><c:overlay val="0"/></c:title>{PLOT}"#));
    let out = set_title(&bare, "新标题");
    assert!(out.contains(r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>新标题</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>"#), "{out}");

    let cached = docx_with_chart(&format!(
        r#"<c:title><c:tx><c:strRef><c:f>Sheet1!$A$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>旧</c:v></c:pt></c:strCache></c:strRef></c:tx></c:title>{PLOT}"#
    ));
    let out = set_title(&cached, "新标题");
    assert!(out.contains("<c:v>新标题</c:v>") && !out.contains("<c:v>旧</c:v>"), "{out}");
    // 重解析看得到
    let mut s = open(&cached);
    let part = chart_part_of(&s);
    s.apply(
        EditOp::SetChartData {
            part,
            patch: ChartPatch { title: Some("新标题".into()), ..ChartPatch::default() },
        },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(parsed(&s.save().unwrap())["blocks"][0]["chartDisplay"]["title"], "新标题");
}

/// chartex part 的 `SetChartData` 明确拒绝（TS 静默 no-op）；事务失败时前面的编辑回滚。
#[test]
fn edit_03_set_chart_data_rejects_chartex_and_rolls_back() {
    let src = std::fs::read(common::corpus_dir("synthetic").join("m6-chartex__001.docx")).unwrap();
    let mut s = open(&src);
    let part = chart_part_of(&s);
    let err = s
        .apply(
            EditOp::SetChartData {
                part,
                patch: ChartPatch { title: Some("x".into()), ..ChartPatch::default() },
            },
            &EditContext::default(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Edit { code: DiagCode::EditUnsupported, .. }), "{err}");
    // 一批里 chartex 失败 → 前面的整 part 替换也回滚
    let main = s.main_part();
    let chart_xml = text(&src, "word/charts/chartEx1.xml");
    let res = s.apply_all(
        vec![
            EditOp::ReplacePartXml { part, xml: chart_xml.replace("Extended", "Changed") },
            EditOp::SetChartData {
                part,
                patch: ChartPatch { title: Some("x".into()), ..ChartPatch::default() },
            },
        ],
        &EditContext::default(),
    );
    assert!(res.is_err());
    let _ = main;
    assert_eq!(s.save().unwrap(), src, "回滚后保存返回原字节");
}

/// `ReplacePartXml` / `ReplacePartBytes`：字节即内容；只接受已存在的 part；替换过的 part 重解析生效。
#[test]
fn edit_03_replace_part_xml_and_bytes() {
    let src = chart_edit_docx();
    let mut s = open(&src);
    let part = chart_part_of(&s);
    let original = text(&src, "word/charts/chart1.xml");
    let new_xml = original
        .replace("<a:t>销售</a:t>", "<a:t>更新后的标题</a:t>")
        .replace("<a:t>统计</a:t>", "<a:t></a:t>")
        .replace("<c:v>120</c:v>", "<c:v>200</c:v>");
    s.apply(EditOp::ReplacePartXml { part, xml: new_xml.clone() }, &EditContext::default())
        .expect("replace xml");
    assert_eq!(
        s.document().chart_parts[&part].display.as_ref().and_then(|d| d.title.clone()).as_deref(),
        Some("更新后的标题"),
        "模型立刻跟上"
    );
    let saved = s.save().unwrap();
    assert_eq!(text(&saved, "word/charts/chart1.xml"), new_xml, "part 字节就是给定内容");
    assert_eq!(entry(&saved, "word/document.xml"), entry(&src, "word/document.xml"));
    let v = parsed(&saved);
    assert_eq!(v["blocks"][0]["chartDisplay"]["series"][0]["values"], json!([200, 88.5, 96]));
    assert_eq!(v["extras"]["chartParts"]["word/charts/chart1.xml"], new_xml);

    // 二进制：换掉内嵌工作簿
    let mut s = open(&src);
    let wb =
        s.package().find_name("word/charts/embeddings/Microsoft_Excel_Worksheet.xlsx").or_else(
            || s.package().parts().iter().find(|p| p.uri.as_str().ends_with(".xlsx")).map(|p| p.id),
        );
    if let Some(wb) = wb {
        let name = s.package().part(wb).uri.as_str().to_string();
        s.apply(
            EditOp::ReplacePartBytes { part: wb, bytes: b"M6 replaced workbook bytes".to_vec() },
            &EditContext::default(),
        )
        .unwrap();
        let saved = s.save().unwrap();
        assert_eq!(entry(&saved, &name), b"M6 replaced workbook bytes");
    }
    // 不存在的 part
    let missing = rsword::package::PartId(9_999);
    let err = s
        .apply(
            EditOp::ReplacePartXml { part: missing, xml: "<a/>".into() },
            &EditContext::default(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Edit { code: DiagCode::EditTargetMissing, .. }), "{err}");
    // 非良构 → 错误，part 不动
    let err = s
        .apply(
            EditOp::ReplacePartXml { part, xml: "<c:chartSpace>".into() },
            &EditContext::default(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Malformed { .. }), "{err}");
}
