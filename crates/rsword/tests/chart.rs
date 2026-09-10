//! 图表 part 的显示模型（`MOD-11`，`spec/17` 任务 6.1）与它的投影（`COMPAT-03`，任务 6.2）在语料上的验收。
//!
//! 6.1 验证**事实**：主 part 引用的每个图表 part 都建出 `ChartDisplay`，且每个字段与 TS golden
//! （`expected.json` 的 `blocks[*].chartDisplay`）逐项对得上。6.2 验证**输出**：`parsed_doc` 的 `chartDisplay` /
//! `previewText` / `extras.chartParts` 与 golden 无差异，chartex 的回退图成为图片块。
//! 语料里 95 份图表（`chart-edit__001` + m6.0a 的 `m6-chart__*` / `m6-chartex__*`）就是 TS 单测夹具的 docx 版。

mod common;

use std::collections::BTreeMap;

#[cfg(feature = "compat-ts")]
use rsword::bind::compat_ts::{
    EmbeddedKind, block_of_path, diff_json, embedded_kind, known_diffs, parsed_doc, split_known,
};
use rsword::diag::DiagCode;
use rsword::model::EMU_PER_PX;
use rsword::model::{Block, ChartColor, ChartDisplay, Display, Document, ProtectedKind};
use rsword::package::Package;
use rsword::resolve::drawingml::{Rgb, hex};
use serde_json::Value;
#[cfg(feature = "compat-ts")]
use serde_json::json;

#[derive(Default)]
struct Stats {
    docs: usize,
    charts: usize,
    compared: BTreeMap<&'static str, usize>,
    near_colors: usize,
    mismatches: Vec<String>,
}

fn hex_of(c: &Option<ChartColor>) -> Option<String> {
    c.as_ref().and_then(|c| c.rgb).map(hex)
}

/// 两个 hex 每通道相差 ≤ 1（`RES-05` 与 TS `lumHex` 的舍入差）。
fn near(a: &str, b: &str) -> bool {
    if a.len() != 6 || b.len() != 6 {
        return false;
    }
    (0..3).all(|i| {
        let x = u8::from_str_radix(&a[2 * i..2 * i + 2], 16).unwrap_or(0);
        let y = u8::from_str_radix(&b[2 * i..2 * i + 2], 16).unwrap_or(0);
        x.abs_diff(y) <= 1
    })
}

fn numbers(v: Option<&Value>) -> Vec<Option<f64>> {
    v.and_then(Value::as_array).map(|a| a.iter().map(Value::as_f64).collect()).unwrap_or_default()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect())
        .unwrap_or_default()
}

fn palette_hex(p: &Option<[Rgb; 6]>) -> Vec<String> {
    p.map(|p| p.iter().copied().map(hex).collect()).unwrap_or_default()
}

fn compare_color(st: &mut Stats, what: String, ours: Option<String>, ts: Option<&str>) {
    *st.compared.entry("color").or_default() += 1;
    match (ours.as_deref(), ts) {
        (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => {}
        (Some(a), Some(b)) if near(a, b) => st.near_colors += 1,
        (None, None) => {}
        _ => st.mismatches.push(format!("{what}: color TS={ts:?} ours={ours:?}")),
    }
}

fn compare(
    st: &mut Stats,
    file: &str,
    d: &ChartDisplay,
    ts: &Value,
    extent_px: Option<(i64, i64)>,
) {
    let s = |k: &str| ts.get(k).and_then(Value::as_str);
    let b = |k: &str| ts.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut check = |what: &'static str, ok: bool, detail: String| {
        *st.compared.entry(what).or_default() += 1;
        if !ok {
            st.mismatches.push(format!("{file}: {what} {detail}"));
        }
    };
    check(
        "kind",
        s("kind") == Some(d.kind.as_str()),
        format!("TS={:?} ours={}", s("kind"), d.kind),
    );
    check(
        "horizontal",
        b("horizontal") == d.horizontal,
        format!("TS={} ours={}", b("horizontal"), d.horizontal),
    );
    check(
        "grouping",
        s("grouping") == d.grouping.map(|g| g.as_str()),
        format!("TS={:?} ours={:?}", s("grouping"), d.grouping),
    );
    check("markers", b("markers") == d.markers, format!("TS={} ours={}", b("markers"), d.markers));
    let ts_hole = ts.get("holePct").and_then(Value::as_u64).map(|h| h as u32);
    check("holePct", ts_hole == d.hole_pct, format!("TS={ts_hole:?} ours={:?}", d.hole_pct));
    check(
        "legendPos",
        s("legendPos") == d.legend_pos.map(|l| l.as_str()),
        format!("TS={:?} ours={:?}", s("legendPos"), d.legend_pos),
    );
    check(
        "title",
        s("title") == d.title.as_deref(),
        format!("TS={:?} ours={:?}", s("title"), d.title),
    );
    let ts_cats = strings(ts.get("categories"));
    check("categories", ts_cats == d.categories, format!("TS={ts_cats:?} ours={:?}", d.categories));
    let ts_series = ts.get("series").and_then(Value::as_array).cloned().unwrap_or_default();
    check(
        "series.len",
        ts_series.len() == d.series.len(),
        format!("TS={} ours={}", ts_series.len(), d.series.len()),
    );
    let ts_palette = strings(ts.get("palette"));
    let ours_palette = palette_hex(&d.palette);
    let palette_ok = ts_palette.len() == ours_palette.len()
        && ts_palette
            .iter()
            .zip(&ours_palette)
            .all(|(a, b)| a.eq_ignore_ascii_case(b) || near(a, b));
    check("palette", palette_ok, format!("TS={ts_palette:?} ours={ours_palette:?}"));
    if let Some((cx, cy)) = extent_px {
        let w = ts.get("widthPx").and_then(Value::as_i64);
        let h = ts.get("heightPx").and_then(Value::as_i64);
        check(
            "extentPx",
            w == Some(cx) && h == Some(cy),
            format!("TS=({w:?},{h:?}) ours=({cx},{cy})"),
        );
    }
    for (i, (ours, ts_s)) in d.series.iter().zip(&ts_series).enumerate() {
        let what = format!("{file}: series[{i}]");
        let s = |k: &str| ts_s.get(k).and_then(Value::as_str);
        *st.compared.entry("series").or_default() += 1;
        if s("name") != ours.name.as_deref() {
            st.mismatches.push(format!("{what}: name TS={:?} ours={:?}", s("name"), ours.name));
        }
        if numbers(ts_s.get("values")) != ours.values {
            st.mismatches.push(format!(
                "{what}: values TS={:?} ours={:?}",
                ts_s.get("values"),
                ours.values
            ));
        }
        let ts_x = ts_s.get("xValues").map(|v| numbers(Some(v)));
        if ts_x != ours.x_values {
            st.mismatches.push(format!("{what}: xValues TS={ts_x:?} ours={:?}", ours.x_values));
        }
        let ts_sizes = ts_s.get("sizes").map(|v| numbers(Some(v)));
        if ts_sizes != ours.sizes {
            st.mismatches.push(format!("{what}: sizes TS={ts_sizes:?} ours={:?}", ours.sizes));
        }
        let ts_line = ts_s.get("line").and_then(Value::as_bool).unwrap_or(false);
        if ts_line != ours.line {
            st.mismatches.push(format!("{what}: line TS={ts_line} ours={}", ours.line));
        }
        compare_color(st, what.clone(), hex_of(&ours.color), s("color"));
        let ts_points: Option<Vec<Option<&str>>> = ts_s
            .get("pointColors")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(Value::as_str).collect());
        let our_points: Option<Vec<Option<String>>> =
            ours.point_colors.as_ref().map(|p| p.iter().map(hex_of).collect());
        match (&ts_points, &our_points) {
            (None, None) => {}
            (Some(t), Some(o)) if t.len() == o.len() => {
                for (j, (tc, oc)) in t.iter().zip(o).enumerate() {
                    compare_color(st, format!("{what}.pointColors[{j}]"), oc.clone(), *tc);
                }
            }
            _ => st
                .mismatches
                .push(format!("{what}: pointColors TS={ts_points:?} ours={our_points:?}")),
        }
    }
}

/// 一个 `Chart` 块：宿主绘图的 `wp:extent`（px）、图表 part 的 display、是否 chartex。
type OurChart<'a> = (Option<(i64, i64)>, Option<&'a ChartDisplay>, bool);

/// 主 part 顶层的 `Chart` 块，文档序。
fn our_charts(doc: &Document) -> Vec<OurChart<'_>> {
    doc.main
        .iter()
        .filter_map(|b| match b {
            rsword::model::Block::Protected(p) if p.kind == rsword::model::ProtectedKind::Chart => {
                Some(p)
            }
            _ => None,
        })
        .map(|p| {
            let drawing = p.display.as_ref().and_then(rsword::model::Display::as_drawing);
            let px = drawing.and_then(|d| d.extent).filter(|e| e.cx > 0 && e.cy > 0).map(|e| {
                (
                    (e.cx as f64 / EMU_PER_PX).round() as i64,
                    (e.cy as f64 / EMU_PER_PX).round() as i64,
                )
            });
            let chart_ref = drawing.and_then(|d| d.chart.as_ref());
            let display = chart_ref
                .and_then(|c| c.rel_id.as_ref())
                .and_then(|rid| doc.chart_by_rel.get(rid))
                .and_then(|part| doc.chart_parts.get(part))
                .and_then(|cp| cp.display.as_ref());
            (px, display, chart_ref.is_some_and(|c| c.chartex))
        })
        .collect()
}

#[test]
fn mod_11_chart_parts_match_ts_across_the_corpus() {
    let mut st = Stats::default();
    let mut ts_docs = 0;
    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let expected_path = path.with_extension("expected.json");
        let Ok(text) = std::fs::read_to_string(&expected_path) else { continue };
        let expected: Value = serde_json::from_str(&text).expect("expected.json");
        let blocks = expected.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
        // TS 顶层 `Chart` 芯片（含没解析出 display 的），文档序
        let ts_charts: Vec<Option<&Value>> = blocks
            .iter()
            .filter(|b| b.get("label").and_then(Value::as_str) == Some("Chart"))
            .map(|b| b.get("chartDisplay"))
            .collect();
        let ts_chart_parts: Vec<String> = expected
            .pointer("/extras/chartParts")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        if ts_charts.is_empty() && ts_chart_parts.is_empty() {
            continue;
        }
        ts_docs += 1;
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).expect("open");
        let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
        st.docs += 1;
        let ours = our_charts(&doc);
        if ours.len() != ts_charts.len() {
            st.mismatches.push(format!(
                "{file}: TS {} 个 Chart 块，本引擎 {} 个",
                ts_charts.len(),
                ours.len()
            ));
            continue;
        }
        for (i, ((px, ours_d, _), ts_d)) in ours.iter().zip(&ts_charts).enumerate() {
            match (ours_d, ts_d) {
                (Some(d), Some(t)) => {
                    st.charts += 1;
                    compare(&mut st, &format!("{file}#{i}"), d, t, *px);
                }
                (None, None) => st.charts += 1,
                (Some(_), None) => {
                    st.mismatches.push(format!("{file}#{i}: TS 没有 chartDisplay，本引擎有"))
                }
                (None, Some(_)) => {
                    st.mismatches.push(format!("{file}#{i}: TS 有 chartDisplay，本引擎没有"))
                }
            }
        }
        // `extras.chartParts`：TS 只存被某个图表块解析成功、且不是 chartex 的 part 原文
        for key in &ts_chart_parts {
            let found = doc.chart_parts.values().any(|cp| {
                pkg.part(cp.part).uri.as_str() == key && !cp.chartex && cp.display.is_some()
            });
            if !found {
                st.mismatches.push(format!(
                    "{file}: TS 的 extras.chartParts 有 {key}，本引擎没有对应的可编辑图表 part"
                ));
            }
        }
        for cp in doc.chart_parts.values() {
            if cp.chartex && ts_chart_parts.iter().any(|k| k == pkg.part(cp.part).uri.as_str()) {
                st.mismatches.push(format!("{file}: chartex part 不该进 extras.chartParts"));
            }
        }
    }
    eprintln!(
        "chart: {} 份带图表的语料，{} 个图表块；逐字段对照 {:?}；颜色 ±1 的 {} 处",
        st.docs, st.charts, st.compared, st.near_colors
    );
    for m in st.mismatches.iter().take(60) {
        eprintln!("chart: MISMATCH {m}");
    }
    assert!(ts_docs >= 90, "语料里带图表的文档应有 90+ 份，实测 {ts_docs}");
    assert!(st.mismatches.is_empty(), "{} mismatches against TS", st.mismatches.len());
}

// ---- 构造用例 -------------------------------------------------------------------------------------

const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

fn chart_paragraph(declare_c: bool) -> String {
    let c_decl = if declare_c { format!(r#" xmlns:c="{C}""#) } else { String::new() };
    format!(
        r#"<w:p><w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}"><wp:inline><wp:extent cx="2857500" cy="1905000"/><wp:docPr id="1" name="Chart 1"/><a:graphic><a:graphicData uri="{C}"><c:chart{c_decl} r:id="rIdChart"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    )
}

fn rels(target: &str) -> String {
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdChart" Type="{R}/chart" Target="{target}"/></Relationships>"#
    )
}

fn chart_space(chart_inner: &str, pre: &str) -> String {
    format!(
        r#"<c:chartSpace xmlns:c="{C}" xmlns:a="{A}" xmlns:r="{R}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">{pre}<c:chart>{chart_inner}</c:chart></c:chartSpace>"#
    )
}

const BAR_PLOT: &str = r#"<c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>S!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>S1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strLit><c:ptCount val="2"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="2"><c:v>x</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea>"#;

fn build(chart_xml: &str, declare_c: bool, target: &str) -> Document {
    let docx = common::docx_with_parts(
        &chart_paragraph(declare_c),
        &[
            ("word/_rels/document.xml.rels", rels(target).as_str()),
            ("word/charts/chart1.xml", chart_xml),
        ],
    );
    let mut pkg = Package::open(&docx).expect("open");
    rsword::model::Document::rebuild(&mut pkg).expect("rebuild")
}

fn display_of(doc: &Document) -> Option<&ChartDisplay> {
    let (_, d, _) = our_charts(doc).into_iter().next().expect("一个 Chart 块");
    d
}

#[test]
fn mod_11_literal_caches_and_auto_title_forms() {
    // `strLit` / `numLit` 字面缓存；`ptCount` 大于点数补空；非数字 → None
    let doc = build(&chart_space(BAR_PLOT, ""), true, "charts/chart1.xml");
    let d = display_of(&doc).expect("display");
    assert_eq!(d.categories, ["A", "B"]);
    assert_eq!(d.series[0].values, [Some(3.0), None, None]);
    assert_eq!(d.series[0].name.as_deref(), Some("S1"));
    assert_eq!(d.title, None, "没有 c:title 元素就没有标题");
    assert_eq!(d.style_val, None);
    assert!(d.palette.is_some(), "没有 theme part 时按内建 Office 调色板");

    // 自动标题：`c:autoTitleDeleted` 缺省 / val="0" → 占位改成单系列的系列名；无 val / "1" / "true" → 无标题
    let title = "<c:title><c:overlay val=\"0\"/></c:title>";
    for (form, want) in [
        ("", Some("S1")),
        ("<c:autoTitleDeleted val=\"0\"/>", Some("S1")),
        ("<c:autoTitleDeleted/>", None),
        ("<c:autoTitleDeleted val=\"1\"/>", None),
        ("<c:autoTitleDeleted val=\"true\"/>", None),
    ] {
        let doc =
            build(&chart_space(&format!("{title}{form}{BAR_PLOT}"), ""), true, "charts/chart1.xml");
        assert_eq!(display_of(&doc).unwrap().title.as_deref(), want, "form {form:?}");
    }
}

#[test]
fn mod_11_style_via_c14_alternate_content() {
    let alt = r#"<mc:AlternateContent><mc:Choice Requires="c14" xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart"><c14:style val="101"/></mc:Choice><mc:Fallback><c:style val="1"/></mc:Fallback></mc:AlternateContent>"#;
    let doc = build(&chart_space(BAR_PLOT, alt), true, "charts/chart1.xml");
    let d = display_of(&doc).expect("display");
    assert_eq!(d.style_val, Some(1));
    assert_eq!(palette_hex(&d.palette)[0], "595959", "样式列 1 是灰阶");
    let doc = build(&chart_space(BAR_PLOT, "<c:style val=\"29\"/>"), true, "charts/chart1.xml");
    let d = display_of(&doc).expect("display");
    assert_eq!(d.style_val, Some(29));
    // 29 → 第 5 列 → accent3（Office 缺省 A5A5A5）打头的单色阶梯
    assert_eq!(palette_hex(&d.palette)[0], "A5A5A5");
}

#[test]
fn mod_11_chart_ref_survives_an_undeclared_prefix_and_dangling_rels_are_diagnosed() {
    // `c` 前缀没声明（`resource-cleanup__008` 的写法）：按前缀字面量认出图表引用
    let doc = build(&chart_space(BAR_PLOT, ""), false, "charts/chart1.xml");
    let (_, d, chartex) = our_charts(&doc).into_iter().next().expect("Chart 块");
    assert!(d.is_some(), "未声明前缀的 c:chart 也要认出并解析到 part");
    assert!(!chartex);
    // 关系指向不存在的 part：display 为空 + PKG_REL_MISSING
    let doc = build(&chart_space(BAR_PLOT, ""), true, "charts/missing.xml");
    assert!(display_of(&doc).is_none());
    assert!(doc.warnings.iter().any(|w| w.code == DiagCode::PkgRelMissing), "{:?}", doc.warnings);
    // 没有任何带缓存值的系列：display 为空 + CHART_NO_SERIES
    let empty = chart_space(
        r#"<c:plotArea><c:barChart><c:ser><c:idx val="0"/><c:val><c:numRef><c:f>S!$A$1</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea>"#,
        "",
    );
    let doc = build(&empty, true, "charts/chart1.xml");
    assert!(display_of(&doc).is_none());
    assert!(doc.warnings.iter().any(|w| w.code == DiagCode::ChartNoSeries), "{:?}", doc.warnings);
}

#[test]
fn test_09_hostile_chart_parts_degrade_locally() {
    // 图表 part 标签不闭合：part 降级 Opaque，图表块留着，display 为空，文档其余部分照常
    let bytes =
        std::fs::read(common::corpus_dir("hostile").join("chart-part-malformed.docx")).unwrap();
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    let cp = doc.chart_parts.values().next().expect("chart part 已登记");
    assert!(cp.root.is_none() && cp.display.is_none());
    assert!(pkg.diagnostics().iter().any(|d| d.code == DiagCode::PkgOpaquePart));
    assert!(
        doc.main.iter().any(|b| matches!(b, Block::Protected(p) if p.kind == ProtectedKind::Chart))
    );
    // `c:chart r:id` 悬空 + `cx:chart` 无 Fallback：两条 PKG_REL_MISSING，都不 panic
    let bytes =
        std::fs::read(common::corpus_dir("hostile").join("chart-missing-rel.docx")).unwrap();
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    let missing = doc.warnings.iter().filter(|w| w.code == DiagCode::PkgRelMissing).count();
    assert!(missing >= 1, "{:?}", doc.warnings);
    assert!(our_charts(&doc).iter().all(|(_, d, _)| d.is_none()));
}

// ---- 6.2 投影 ------------------------------------------------------------------------------------

/// `COMPAT-03`：语料里每个图表文档的 `parsed_doc` 与 TS golden 在图表域（图表块上的一切字段 +
/// `extras.chartParts`）无未知差异——`--scope embedded` 在图表这一块的门。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_chart_projection_matches_ts_across_the_corpus() {
    let known = known_diffs();
    let (mut docs, mut displays, mut parts) = (0, 0, 0);
    let mut unknown: Vec<String> = Vec::new();
    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).expect("expected.json");
        let blocks = expected.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
        let chart_blocks = blocks.iter().filter(|b| embedded_kind(b) == Some(EmbeddedKind::Chart));
        let n_display = chart_blocks.clone().filter(|b| b.get("chartDisplay").is_some()).count();
        let n_parts = expected
            .pointer("/extras/chartParts")
            .and_then(Value::as_object)
            .map_or(0, serde_json::Map::len);
        if chart_blocks.clone().next().is_none() && n_parts == 0 {
            continue;
        }
        docs += 1;
        displays += n_display;
        parts += n_parts;
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).expect("open");
        let actual = parsed_doc(&mut pkg).expect("parsed_doc");
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (diffs, _) = split_known(diffs, &file, &known);
        for d in diffs.into_iter().filter(|d| {
            d.path.starts_with("extras.chartParts")
                || block_of_path(&d.path, &expected)
                    .is_some_and(|b| embedded_kind(b) == Some(EmbeddedKind::Chart))
        }) {
            unknown.push(format!("{file}: {} TS={:?} ours={:?}", d.path, d.expected, d.actual));
        }
    }
    eprintln!(
        "chart: {docs} 份图表文档，{displays} 个 chartDisplay，{parts} 个 extras.chartParts 条目"
    );
    for u in unknown.iter().take(40) {
        eprintln!("chart: DIFF {u}");
    }
    assert!(docs >= 90 && displays >= 85 && parts >= 75, "{docs} / {displays} / {parts}");
    assert!(unknown.is_empty(), "{} 处图表域差异", unknown.len());
}

#[cfg(feature = "compat-ts")]
fn parsed(docx: &[u8]) -> Value {
    let mut pkg = Package::open(docx).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

/// 一份图表块的 `parsed_doc`：`chartDisplay` 的字段换名、`wp:extent` → px、`previewText` = 标题、
/// `extras.chartParts` 是 part 的**原文**（连 XML 声明与空白都一样，不重新序列化）。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_chart_block_fields_and_raw_chart_part() {
    let title = r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>销售</a:t></a:r><a:r><a:t>统计</a:t></a:r></a:p></c:rich></c:tx></c:title>"#;
    let part = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n{}",
        chart_space(&format!("{title}{BAR_PLOT}"), "")
    );
    let docx = common::docx_with_parts(
        &chart_paragraph(true),
        &[
            ("word/_rels/document.xml.rels", rels("charts/chart1.xml").as_str()),
            ("word/charts/chart1.xml", &part),
        ],
    );
    let v = parsed(&docx);
    let b = &v["blocks"][0];
    assert_eq!(b["type"], "passthrough");
    assert_eq!(b["label"], "Chart");
    assert_eq!(b["previewText"], "销售统计");
    let cd = &b["chartDisplay"];
    assert_eq!(cd["partPath"], "word/charts/chart1.xml");
    assert_eq!(cd["kind"], "bar");
    assert_eq!(cd["title"], "销售统计");
    assert_eq!((cd["widthPx"].as_i64(), cd["heightPx"].as_i64()), (Some(300), Some(200)));
    assert_eq!(cd["categories"], json!(["A", "B"]));
    // 数值缓存：缺点与非数字是 null，整数值写成整数
    assert_eq!(cd["series"], json!([{ "name": "S1", "values": [3, null, null] }]));
    for absent in ["horizontal", "grouping", "markers", "holePct", "legendPos"] {
        assert!(cd.get(absent).is_none(), "{absent} 不该出现");
    }
    let parts = v["extras"]["chartParts"].as_object().expect("chartParts");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts["word/charts/chart1.xml"].as_str(), Some(part.as_str()), "原字节");
}

/// 解析不出 display 的图表块（关系悬空 / part 没有带缓存的系列）：只剩 `label: "Chart"`——没有 `chartDisplay`，
/// 也**没有** `previewText`（TS 用 `...(x ? {} : {})` 展开，`undefined` 与 `""` 不等价）；`extras.chartParts` 不收它。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_chart_without_display_has_neither_preview_text_nor_part_entry() {
    let dangling = common::docx_with_parts(
        &chart_paragraph(true),
        &[("word/_rels/document.xml.rels", rels("charts/missing.xml").as_str())],
    );
    let no_series = common::docx_with_parts(
        &chart_paragraph(true),
        &[
            ("word/_rels/document.xml.rels", rels("charts/chart1.xml").as_str()),
            ("word/charts/chart1.xml", &chart_space("<c:plotArea/>", "")),
        ],
    );
    for (what, docx) in [("悬空关系", dangling), ("无系列", no_series)] {
        let v = parsed(&docx);
        let b = &v["blocks"][0];
        assert_eq!(b["label"], "Chart", "{what}");
        assert_eq!(b["type"], "passthrough", "{what}");
        assert!(b.get("chartDisplay").is_none(), "{what}: {b}");
        assert!(b.get("previewText").is_none(), "{what}: {b}");
        assert_eq!(v["extras"]["chartParts"], json!({}), "{what}");
    }
}

#[cfg(feature = "compat-ts")]
const CX: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";
#[cfg(feature = "compat-ts")]
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
#[cfg(feature = "compat-ts")]
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
#[cfg(feature = "compat-ts")]
const CHARTEX_PART: &str = r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chartData><cx:data id="0"><cx:strDim type="cat"><cx:lvl ptCount="2"><cx:pt idx="0">A</cx:pt><cx:pt idx="1">B</cx:pt></cx:lvl></cx:strDim><cx:numDim type="val"><cx:lvl ptCount="2"><cx:pt idx="0">100</cx:pt><cx:pt idx="1">-40</cx:pt></cx:lvl></cx:numDim></cx:data></cx:chartData><cx:chart><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="sunburst"><cx:tx><cx:txData><cx:v>Extended</cx:v></cx:txData></cx:tx><cx:dataId val="0"/></cx:series></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>"#;

/// chartex 绘图的 `w:r`（放在 `mc:Choice` 里或裸放）。
#[cfg(feature = "compat-ts")]
fn chartex_run() -> String {
    format!(
        r#"<w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}"><wp:inline><wp:extent cx="2857500" cy="1905000"/><wp:docPr id="1" name="Graphic 1"/><a:graphic><a:graphicData uri="{CX}"><cx:chart xmlns:cx="{CX}" r:id="rIdCx"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

/// 回退图的 `w:r`：1 英寸的图片，extent 故意与 Choice 不同。
#[cfg(feature = "compat-ts")]
fn fallback_picture_run() -> String {
    format!(
        r#"<w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:r="{R}" xmlns:pic="{PIC}"><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="2" name="Picture 2"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

#[cfg(feature = "compat-ts")]
fn chartex_docx(paragraph: &str) -> Vec<u8> {
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdCx" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="charts/chartEx1.xml"/><Relationship Id="rIdImg" Type="{R}/image" Target="media/image1.png"/></Relationships>"#
    );
    let docx = common::docx_with_parts(
        paragraph,
        &[
            ("word/_rels/document.xml.rels", rels.as_str()),
            ("word/charts/chartEx1.xml", CHARTEX_PART),
        ],
    );
    common::with_binary_part(&docx, "word/media/image1.png", &common::b64(common::PNG_1X1))
}

/// R12 的细化（`spec/06`）：chartex 配了 `mc:Fallback` 回退图 → 图片块。图取 Fallback 的 `a:blip`（媒体预取
/// 对这种 Fallback 放行），尺寸取 Choice 的 `wp:extent`；块上没有 `previewText` / `chartDisplay` / `brokenImage`。
/// 没有回退图的 chartex 仍是 `Chart` 块：`chartDisplay` 走降级读法，`extras.chartParts` 不收 chartex part。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_03_chartex_fallback_picture_becomes_an_image_block() {
    let with_fallback = format!(
        r#"<w:p><mc:AlternateContent xmlns:mc="{MC}" xmlns:cx="{CX}"><mc:Choice Requires="cx">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent></w:p>"#,
        chartex_run(),
        fallback_picture_run()
    );
    let docx = chartex_docx(&with_fallback);
    // 模型：Image 块，显示模型是回退图、extent 是 Choice 的
    let mut pkg = Package::open(&docx).expect("open");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    let Some(rsword::model::Block::Image(img)) = doc.main.first() else {
        panic!("应是 Image 块：{:?}", doc.main.first())
    };
    let d =
        img.display.as_ref().and_then(rsword::model::Display::as_drawing).expect("DrawingDisplay");
    assert!(d.picture().is_some(), "显示模型取自 Fallback 里的图片");
    assert!(d.chart.is_none());
    assert_eq!(d.extent.map(|e| (e.cx, e.cy)), Some((2_857_500, 1_905_000)), "尺寸取 Choice");
    // 投影：TS 的 `type: image` 块
    let v = parsed(&docx);
    let b = &v["blocks"][0];
    assert_eq!(b["type"], "image", "{b}");
    assert_eq!(b["label"], "Image");
    assert!(
        b["imageDataUrl"].as_str().is_some_and(|u| u.starts_with("data:image/png;base64,")),
        "{b}"
    );
    assert_eq!((b["imageWidthPx"].as_i64(), b["imageHeightPx"].as_i64()), (Some(300), Some(200)));
    for absent in ["previewText", "chartDisplay", "brokenImage"] {
        assert!(b.get(absent).is_none(), "{absent} 不该出现：{b}");
    }
    assert_eq!(v["extras"]["chartParts"], json!({}));

    let bare = format!("<w:p>{}</w:p>", chartex_run());
    let v = parsed(&chartex_docx(&bare));
    let b = &v["blocks"][0];
    assert_eq!(b["label"], "Chart", "{b}");
    assert_eq!(b["previewText"], "", "chartex 的降级读法没有标题");
    assert_eq!(b["chartDisplay"]["kind"], "pie", "sunburst → pie（TS `CHARTEX_KINDS`）");
    assert_eq!(b["chartDisplay"]["partPath"], "word/charts/chartEx1.xml");
    assert_eq!(b["chartDisplay"]["series"], json!([{ "name": "Extended", "values": [100, -40] }]));
    assert_eq!(v["extras"]["chartParts"], json!({}), "chartex part 不进 chartParts");
}
