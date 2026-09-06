//! 图表 part 的显示模型（`MOD-11`，`spec/17` 任务 6.1）在语料上的验收。
//!
//! 投影（`chartDisplay` 真正出现在 `ParsedDoc` 里）是 6.2 的事；这里验证**事实**：主 part 引用的每个图表 part
//! 都建出 `ChartDisplay`，且每个字段与 TS golden（`expected.json` 的 `blocks[*].chartDisplay`）逐项对得上。
//! 语料里 95 份图表（`chart-edit__001` + m6.0a 的 `m6-chart__*` / `m6-chartex__*`）就是 TS 单测夹具的 docx 版。

mod common;

use std::collections::BTreeMap;

use rsword::diag::DiagCode;
use rsword::model::units::EMU_PER_PX;
use rsword::model::{Block, ChartColor, ChartDisplay, Display, Document, ProtectedKind};
use rsword::package::Package;
use rsword::resolve::drawingml::{Rgb, hex};
use serde_json::Value;

/// TS 与本引擎对不上时先看这里：分类差异（R12 的 chartex Fallback 图片）归 6.2，颜色 ±1 归 `RES-05`。
const KNOWN_CLASS_DIFF: &[&str] = &[
    // A23：chartex 带 Fallback 图片，TS 偏爱图片（`type: image`），R12 的细化在 6.2
    "m6-chartex__008.docx",
];

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
            Block::Protected(p) if p.kind == ProtectedKind::Chart => Some(p),
            _ => None,
        })
        .map(|p| {
            let drawing = p.display.as_ref().and_then(Display::as_drawing);
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
        let doc = Document::rebuild(&mut pkg).expect("rebuild");
        st.docs += 1;
        let ours = our_charts(&doc);
        if ours.len() != ts_charts.len() {
            if !KNOWN_CLASS_DIFF.contains(&file.as_str()) {
                st.mismatches.push(format!(
                    "{file}: TS {} 个 Chart 块，本引擎 {} 个",
                    ts_charts.len(),
                    ours.len()
                ));
            }
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
    Document::rebuild(&mut pkg).expect("rebuild")
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
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
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
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
    let missing = doc.warnings.iter().filter(|w| w.code == DiagCode::PkgRelMissing).count();
    assert!(missing >= 1, "{:?}", doc.warnings);
    assert!(our_charts(&doc).iter().all(|(_, d, _)| d.is_none()));
}
