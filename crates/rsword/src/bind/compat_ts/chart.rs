//! 图表块的投影（`COMPAT-03`，任务 6.2）：TS `extractChart` 的三样产物——块上的 `chartDisplay`、
//! `previewText`（标题，没标题给 `""`）、`extras.chartParts[path]`（可编辑图表 part 的原文）。
//!
//! 模型在 `model/chart.rs`（6.1）；这里只做字段换名与单位换算：颜色 → 无 `#` 的大写 hex，
//! 宿主绘图的 `wp:extent` → 取整的 px，整数值的 `f64` 写成 JSON 整数（TS 的 `JSON.stringify` 就是这样）。

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::model::DrawingDisplay;
use crate::model::emu_to_px;
use crate::model::{Block, Display, Document, ProtectedBlock, ProtectedKind};
use crate::model::{ChartColor, ChartDisplay, ChartPart, ChartSeries};
use crate::package::Package;
use crate::resolve::drawingml::hex;
use crate::xml::Dom;

use super::blocks::Ctx;
use crate::bind::native::json::{display_json, set};

/// 主 part 的一个图表关系：zip 路径、part 模型、原文。
pub(super) struct ChartEntry<'a> {
    /// zip 路径（TS `partPath` 与 `extras.chartParts` 的键），如 `word/charts/chart1.xml`。
    pub path: &'a str,
    pub part: &'a ChartPart,
    /// part 原文（解析成功时）；`extras.chartParts` 原样输出，不重新序列化。
    pub src: Option<&'a str>,
}

/// 关系 id → 图表 part。`parsed_doc_of` 建一次，挂在 `Ctx::charts`。
pub(super) type ChartMap<'a> = BTreeMap<String, ChartEntry<'a>>;

pub(super) fn chart_map<'a>(pkg: &'a Package, doc: &'a Document) -> ChartMap<'a> {
    doc.chart_by_rel
        .iter()
        .filter_map(|(rid, &id)| {
            let part = doc.chart_parts.get(&id)?;
            let p = pkg.part(id);
            Some((
                rid.clone(),
                ChartEntry { path: p.uri.as_str(), part, src: p.dom().map(Dom::src) },
            ))
        })
        .collect()
}

/// 空表：没挂图表表的上下文（页眉页脚 part、外部文本框 part）。
pub(super) fn empty_charts() -> &'static ChartMap<'static> {
    static EMPTY: std::sync::LazyLock<ChartMap<'static>> = std::sync::LazyLock::new(ChartMap::new);
    &EMPTY
}

/// 图表块上的两个字段。TS：`...(chartDisplay ? { chartDisplay, previewText: chartDisplay.title ?? '' } : {})`
/// ——没有 display 的块（关系悬空、part 缺失或解析不出、没有带缓存的系列）什么都不写，连 `previewText`
/// 都没有：`undefined` 与 `""` 在差分里不等价。
pub(super) fn chart_block(ctx: &Ctx<'_>, pb: &ProtectedBlock, o: &mut Map<String, Value>) {
    let Some(drawing) = pb.display.as_ref().and_then(Display::as_drawing) else { return };
    let Some((entry, d)) = resolve(ctx, drawing) else { return };
    set(o, "chartDisplay", Value::Object(display_json(entry.path, d, drawing)));
    set(o, "previewText", d.title.clone().unwrap_or_default());
}

/// `extras.chartParts`：TS 在建块时把每个**顶层**图表块解析成功、且不是 chartex 的 part 原文按 zip 路径
/// 存下来（chartex 的 `SetChartData` 补丁不认，存了会把编辑变成破坏）。表格单元格里的图表 TS 不建块，
/// 也就不进这张表；同一 part 被两个块引用只存一份。
pub(super) fn chart_parts_json(ctx: &Ctx<'_>) -> Map<String, Value> {
    let mut out = Map::new();
    for b in &ctx.doc.main {
        let Block::Protected(pb) = b else { continue };
        if !matches!(pb.kind, ProtectedKind::Chart) {
            continue;
        }
        let Some(drawing) = pb.display.as_ref().and_then(Display::as_drawing) else { continue };
        let Some((entry, _)) = resolve(ctx, drawing) else { continue };
        if entry.part.chartex {
            continue;
        }
        if let Some(src) = entry.src {
            out.entry(entry.path.to_string()).or_insert_with(|| Value::String(src.to_string()));
        }
    }
    out
}

/// 宿主绘图 → 它引用的图表 part 与 display。
fn resolve<'a>(
    ctx: &Ctx<'a>,
    drawing: &DrawingDisplay,
) -> Option<(&'a ChartEntry<'a>, &'a ChartDisplay)> {
    let rid = drawing.chart.as_ref()?.rel_id.as_deref()?;
    let entry = ctx.charts.get(rid)?;
    Some((entry, entry.part.display.as_ref()?))
}

/// TS `ChartDisplay`（`types.ts`）：一张 14 行的表。`palette` 只在 6.1 能定出时给；`widthPx / heightPx`
/// 取宿主 `wp:extent`，0 或缺失不给。
fn display_json(path: &str, d: &ChartDisplay, drawing: &DrawingDisplay) -> Map<String, Value> {
    let ext = drawing.extent;
    display_json! {
        "partPath" => path,
        "kind" => d.kind.as_str(),
        flag "horizontal" => d.horizontal,
        opt "grouping" => d.grouping.map(|g| g.as_str()),
        flag "markers" => d.markers,
        opt "holePct" => d.hole_pct,
        opt "legendPos" => d.legend_pos.map(|l| l.as_str()),
        opt "title" => d.title.clone(),
        "categories" => d.categories.clone(),
        "series" => d.series.iter().map(series_json).collect::<Vec<Value>>(),
        opt "palette" => d.palette.map(|p| p.iter().copied().map(hex).collect::<Vec<String>>()),
        opt "widthPx" => ext.map(|e| e.cx).filter(|&cx| cx > 0).map(px),
        opt "heightPx" => ext.map(|e| e.cy).filter(|&cy| cy > 0).map(px),
    }
}

/// TS `ChartSeries`：7 行。
fn series_json(s: &ChartSeries) -> Value {
    Value::Object(display_json! {
        opt "name" => s.name.clone(),
        "values" => numbers(&s.values),
        opt "color" => color_hex(s.color.as_ref()),
        opt "pointColors" => s
            .point_colors
            .as_ref()
            .map(|pc| pc.iter().map(|c| color_hex(c.as_ref()).map_or(Value::Null, Value::String)).collect::<Vec<Value>>()),
        opt "xValues" => s.x_values.as_deref().map(numbers),
        opt "sizes" => s.sizes.as_deref().map(numbers),
        flag "line" => s.line,
    })
}

/// 解出 sRGB 的颜色 → 大写 hex；解不出（未知槽位、主题缺项）按没写颜色处理。
fn color_hex(c: Option<&ChartColor>) -> Option<String> {
    c.and_then(|c| c.rgb).map(hex)
}

/// 缓存数列 → JSON 数组：缺点 / 非数字是 `null`；整数值写成整数。
fn numbers(v: &[Option<f64>]) -> Value {
    Value::Array(v.iter().map(|x| x.map_or(Value::Null, number)).collect())
}

fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 { Value::from(n as i64) } else { Value::from(n) }
}

/// EMU → 取整的 px（TS `Math.round(cx / EMU_PER_PX)`）。
fn px(emu: i64) -> i64 {
    emu_to_px(emu as f64).round() as i64
}
