//! `COMPAT-10` 的定点验收（任务 3.5）：全语料的逐字段对照由 `diff-parse --scope tables` 做
//! （M3 门第 1 条），这里钉住几条聚合数字看不出来的规则。

mod common;

use rsword::bind::compat_ts::parsed_doc;
use rsword::package::Package;
use serde_json::Value;

fn doc(name: &str) -> Value {
    let bytes =
        std::fs::read(common::corpus_dir("synthetic").join(format!("{name}.docx"))).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    parsed_doc(&mut pkg).unwrap()
}

fn tables(d: &Value) -> Vec<&Value> {
    d["blocks"].as_array().unwrap().iter().filter_map(|b| b.get("table")).collect()
}

/// 嵌套到第 8 层就整棵扁平化成 1×1（TS `flattenedTableModel`）：模型在 64 层才截断，扁平化必须
/// 直接读 DOM，否则拿不到更深的段落。
#[test]
fn compat_10_deep_nesting_flattens_at_depth_eight() {
    let d = doc("deep-nested-table__001");
    let mut t = tables(&d)[0].clone();
    // 逐层下钻：前 8 层都是正常表格，第 9 层是扁平化的结果
    for level in 1..=8 {
        let cell = &t["rows"][0][0];
        let nested =
            cell["nestedTables"].as_array().unwrap_or_else(|| panic!("第 {level} 层没有嵌套表"));
        assert_eq!(nested.len(), 1, "第 {level} 层");
        t = nested[0].clone();
    }
    // 第 8 层的子表是扁平的 1×1，autoLayout，且带着它以下**全部**段落的文字
    assert_eq!(t["autoLayout"], Value::Bool(true));
    assert_eq!(t["rows"].as_array().unwrap().len(), 1);
    assert_eq!(t["rows"][0].as_array().unwrap().len(), 1);
    let cell = &t["rows"][0][0];
    let paras = cell["paras"].as_array().unwrap();
    assert!(paras.len() > 100, "扁平表应收下深处的全部段落，实得 {}", paras.len());
    assert!(cell.get("nestedTables").is_none(), "扁平表没有嵌套");
    // richParas 与 paras 一一对应，非空段落一个纯文本 run
    let rich = cell["richParas"].as_array().unwrap();
    assert_eq!(rich.len(), paras.len());
    let non_empty = paras.iter().position(|p| p.as_str() != Some("")).unwrap();
    assert_eq!(rich[non_empty]["runs"].as_array().unwrap().len(), 1);
    assert_eq!(rich[non_empty]["runs"][0]["text"], paras[non_empty]);
}

/// TS `attachRawTablePr` 的"宁可不挂也不挂错"：某行的直接 `w:tc` 数与折叠后的真实格数不符时，
/// 那一行不给 `rawTcPr`（行属性照给）。`table-display__005` 的第一行有 `hMerge` 折叠。
#[test]
fn compat_10_raw_tc_pr_is_dropped_when_folding_changed_the_cell_count() {
    let d = doc("table-display__005");
    let t = tables(&d)[0];
    let row = t["rows"][0].as_array().unwrap();
    assert_eq!(row.len(), 2, "三个 w:tc 里的 hMerge continue 折进了左格");
    assert_eq!(row[0]["colSpan"], Value::from(2));
    assert_eq!(row[0]["hMerge"], Value::from("restart"));
    assert!(row[0].get("rawTcPr").is_none(), "格数对不上，整行不挂 rawTcPr");
    assert!(row[1].get("rawTcPr").is_none());
    // 对照：没有折叠的表格照常挂
    let d = doc("table-display__001");
    let t = tables(&d)[0];
    assert!(t["rows"][0][0].get("rawTcPr").is_some());
}

/// `gridBefore` / `gridAfter` 的显示占位：不是 `w:tc`，没有 `rawTcPr`，跨度按声明值。
#[test]
fn compat_10_grid_gap_placeholders() {
    let d = doc("table-grid-reconcile__004");
    let t = tables(&d)[0];
    let mut gaps = 0;
    for row in t["rows"].as_array().unwrap() {
        for cell in row.as_array().unwrap() {
            if cell.get("gridGap").is_some() {
                gaps += 1;
                assert_eq!(cell["paras"].as_array().unwrap().len(), 0);
                assert!(cell.get("rawTcPr").is_none());
            }
        }
    }
    assert!(gaps >= 2, "语料这份有行首与行尾的占位，实得 {gaps}");
}

/// `styles.*.tableDisplay`：条件格式与 basedOn 链的深合并。
#[test]
fn compat_10_table_display_from_the_style_chain() {
    let d = doc("table-style__001");
    let td = &d["styles"]["GridBlue"]["tableDisplay"];
    assert!(td.is_object(), "GridBlue 应有 tableDisplay：{}", d["styles"]["GridBlue"]);
    assert!(td.get("firstRow").is_some() || td.get("band1Fill").is_some(), "{td}");
    // 基样式的层通过 basedOn 继承（table-style__004 是链的用例）
    let d = doc("table-style__004");
    let styles = d["styles"].as_object().unwrap();
    let child = styles
        .values()
        .find(|s| s["type"] == "table" && s.get("tableDisplay").is_some_and(Value::is_object))
        .expect("应有带 tableDisplay 的表格样式");
    assert!(!child["tableDisplay"].as_object().unwrap().is_empty());
}

/// 表格块的 `label` 与 `previewText`（TS `tableSummary`：按原字节数，嵌套表也算）。
#[test]
fn compat_10_table_summary_counts_nested_rows() {
    let d = doc("table-display__003");
    let b = d["blocks"].as_array().unwrap().iter().find(|b| b["type"] == "table").unwrap();
    assert_eq!(b["label"], Value::from("Table 2×3"), "外层 1 行 2 格 + 嵌套 1 行 2 格");
    assert!(b["previewText"].as_str().unwrap().contains("外层"));
}

/// 单元格里的锚定形状：挂在格上（不像正文段落那样把整块降级成 `Text box`），锚点记它前面有几段，
/// 格内文字要把框的内容与 `wp:posOffset` 的数字剥掉（`COMPAT-10`；M3 与 M4 的交叉点）。
#[test]
fn compat_10_cell_anchored_boxes() {
    let d = doc("cell-anchored-boxes__001");
    let t = tables(&d)[0];
    let cell = &t["rows"][0][0];
    let boxes = cell["anchoredBoxes"].as_array().expect("格里的锚定框");
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0]["prst"], Value::from("triangle"));
    assert_eq!(boxes[0]["floating"], Value::Bool(true));
    assert_eq!(cell["anchoredBoxAnchors"], serde_json::json!([0]), "锚在第 0 段");
    assert_eq!(cell["paras"], serde_json::json!(["cell text"]), "框与偏移量不进格内文字");
    // 块本身仍是表格，没有被降级
    let b = d["blocks"].as_array().unwrap().iter().find(|b| b["type"] == "table").unwrap();
    assert!(b.get("textboxes").is_none(), "格里的框不该冒到块上");

    // 多段的格：锚点是框所在段落的下标
    let d = doc("cell-anchored-boxes__004");
    let cell = &tables(&d)[0]["rows"][0][0];
    assert_eq!(cell["anchoredBoxAnchors"], serde_json::json!([1]));
    assert_eq!(cell["anchoredBoxes"].as_array().unwrap().len(), 1);
}

/// 格里只有一张图的段落：`MOD-05` R15 把它分成图片块，但 TS 在格里一律当普通段落，
/// `richParas` 要给出那个图片 run。
#[test]
fn compat_10_cell_image_paragraph_still_has_a_run() {
    let d = doc("bugfix-regressions__006");
    let cell = &tables(&d)[0]["rows"][0][0];
    let runs = cell["richParas"][0]["runs"].as_array().expect("图片段落的 runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["text"], Value::from(""));
    assert!(
        runs[0]["image"]["dataUrl"].as_str().is_some_and(|u| u.starts_with("data:image/")),
        "{}",
        runs[0]
    );
}
