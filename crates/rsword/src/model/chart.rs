//! 图表 part 的显示模型（`MOD-11`，`spec/17` 任务 6.1）。
//!
//! 图表 part（`word/charts/chartN.xml`）是**有自己 DOM 的 XML part**（L1），这里的 [`ChartDisplay`] 是它的投影：
//! 只读 Word 写在数据引用旁边的缓存（`c:strCache` / `c:numCache`），内嵌工作簿从不打开。`SetChartData`（6.6）改的
//! 也是这些缓存的文本节点，所以模型里每个可编辑的值都带着 `NodeId`。
//!
//! 与 TS `chart.ts` 的 `parseChartPartXml` / `parseChartexPartXml` 行为对齐（`docs/01` §8.5），但按 `MOD-11` 分层：
//! 几何留 EMU 原值（宿主 `wp:extent` 在 [`DrawingDisplay`](crate::model::drawing::DrawingDisplay) 上）、px 换算在
//! `compat_ts`；颜色经 `RES-05`（`resolve/drawingml.rs`）解析为 sRGB 并保留原始定义（[`ChartColor`]）；调色板
//! （`c:style` → 主题 accent 的阶梯）是主题的纯函数，这里按文档的配色方案算一次存下来。
//!
//! chartex（`cx:chartSpace`，旭日 / 树状 / 瀑布 / 箱形 / 漏斗 / 帕累托）按 TS 的降级读：数据维度与系列名进同一个
//! [`ChartDisplay`]，`kind` 取最近的经典种类；它的 part 不可编辑（`chartex: true`）。

use std::ops::Range;

use crate::diag::{DiagCode, Diagnostic};
use crate::model::macros::named_enum;
use crate::model::theme::{ColorScheme, ThemeSlot};
use crate::package::PartId;
use crate::resolve::drawingml::{DrawingColor, Rgb, color_in};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

named_enum! {
    /// 图表种类（TS `ChartDisplay.kind`）。三维与环形归并到平面同类；认不出的 `*Chart` 元素是 `Other`。
    pub enum ChartKind {
        Bar = "bar",
        Line = "line",
        Pie = "pie",
        Area = "area",
        Scatter = "scatter",
        Bubble = "bubble",
        Other = "other",
    }
}

named_enum! {
    /// `c:grouping`：只对 bar / area 认堆积的两种（`clustered` / `standard` 不记）。
    pub enum ChartGrouping {
        Stacked = "stacked",
        PercentStacked = "percentStacked",
    }
}

named_enum! {
    /// `c:legend/c:legendPos/@val`；有 `c:legend` 而没写位置时 schema 缺省是右侧。
    pub enum LegendPos {
        Bottom = "b",
        Left = "l",
        Right = "r",
        Top = "t",
        TopRight = "tr",
    }
}

/// 图表 part 元素名 → 种类。一张表同时给出「哪些元素算绘图区里的图」（`plot_kind` 有值）与「归并到哪一类」；
/// ECMA-376 §21.2.2 的 16 种图全部在表里，认不出的种类明说是 `Other`，别的元素（`c:catAx` 等）不是图。
macro_rules! chart_kinds {
    ($($local:ident => $kind:ident),+ $(,)?) => {
        /// 绘图区子元素 → 图表种类；不是图表元素 → `None`。
        pub fn plot_kind(local: LocalName) -> Option<ChartKind> {
            match local {
                $(LocalName::$local => Some(ChartKind::$kind),)+
                _ => None,
            }
        }

        /// 表里全部图表元素（测试用：每一种都要被认出来）。
        pub const PLOT_ELEMENTS: &[LocalName] = &[$(LocalName::$local,)+];
    };
}

chart_kinds! {
    BarChart => Bar,
    Bar3DChart => Bar,
    LineChart => Line,
    Line3DChart => Line,
    PieChart => Pie,
    Pie3DChart => Pie,
    DoughnutChart => Pie,
    OfPieChart => Other,
    AreaChart => Area,
    Area3DChart => Area,
    ScatterChart => Scatter,
    BubbleChart => Bubble,
    RadarChart => Other,
    StockChart => Other,
    SurfaceChart => Other,
    Surface3DChart => Other,
}

/// chartex `cx:series/@layoutId` → 最近的经典种类（TS `CHARTEX_KINDS`：形状不同，数据 / 标签 / 标题照样显示）。
pub fn chartex_kind(layout_id: &str) -> Option<ChartKind> {
    Some(match layout_id {
        "clusteredColumn" | "boxWhisker" | "waterfall" | "funnel" => ChartKind::Bar,
        "paretoLine" => ChartKind::Line,
        "sunburst" | "treemap" => ChartKind::Pie,
        _ => return None,
    })
}

/// 一个图表里的颜色：原始定义（`RES-05` 的 [`DrawingColor`]）与按文档配色方案解出的 sRGB。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartColor {
    pub def: DrawingColor,
    /// 解不出（未知槽位、主题里缺）→ `None`，按「没写颜色」处理。
    pub rgb: Option<Rgb>,
}

/// 一个系列（`c:ser` / `cx:series`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub node: NodeId,
    /// `c:tx`：字面 `c:v` 或缓存的第一个点。
    pub name: Option<String>,
    /// `c:val` / `c:yVal` 缓存；非数字或缺点 → `None`（TS 的 `null`，画成空档）。
    pub values: Vec<Option<f64>>,
    /// `c:spPr/a:solidFill`。
    pub color: Option<ChartColor>,
    /// `c:dPt` 逐点填充，按 `c:idx` 稀疏（饼图扇区、高亮的柱）；一处都没有 → `None`。
    pub point_colors: Option<Vec<Option<ChartColor>>>,
    /// scatter / bubble：`c:xVal` 缓存（全空 → `None`）。
    pub x_values: Option<Vec<Option<f64>>>,
    /// bubble：`c:bubbleSize` 缓存。
    pub sizes: Option<Vec<Option<f64>>>,
    /// scatter：`c:scatterStyle` 含 line / smooth 且系列的 `a:ln` 不是 `a:noFill` → 点之间连线。
    pub line: bool,
}

/// 一个图表 part 的显示模型（TS `ChartDisplay`，`MOD-11`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartDisplay {
    /// `c:chartSpace` / `cx:chartSpace`。
    pub root: NodeId,
    pub kind: ChartKind,
    /// 绘图区里第一个图表元素（组合图以它为准）；chartex 没有。
    pub plot: Option<NodeId>,
    /// bar：`c:barDir val="bar"`（水平条形）。
    pub horizontal: bool,
    pub grouping: Option<ChartGrouping>,
    /// line：`c:marker val="1"`；scatter：`c:scatterStyle` 缺省或含 `marker`。
    pub markers: bool,
    /// doughnut：`c:holeSize`（缺省 50）；`0` 与非环形 → `None`。
    pub hole_pct: Option<u32>,
    pub legend_pos: Option<LegendPos>,
    /// 标题文字：`a:t` 拼接 → `c:strCache/c:v` → 自动标题 `Chart Title`（`autoTitleDeleted` 为真则无）；
    /// 单系列的自动标题按 Office 的做法取系列名。没有 `c:title` 元素 → `None`。
    pub title: Option<String>,
    /// `c:title` 节点（6.6 改标题的落点）。
    pub title_node: Option<NodeId>,
    /// 类别文本：第一个带 `c:cat` / `c:xVal` 的系列；日期格式的序列号换成 `m/d/yyyy`，`xVal` 的长小数四舍五入到 4 位。
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
    /// `c:style/@val`（1–48；`c14:style` 101–148 减 100）。
    pub style_val: Option<u8>,
    /// 系列颜色循环：`c:style` 的列决定灰阶 / 六个 accent / 单色阶梯（[`palette`]）。主题缺 accent → `None`。
    pub palette: Option<[Rgb; 6]>,
    /// `cx:chartSpace`：只有降级读法，`SetChartData` 不接受。
    pub chartex: bool,
}

/// 主 part 引用的一个图表 part。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPart {
    pub part: PartId,
    /// part 解析不了（`Opaque`）时 `None`。
    pub root: Option<NodeId>,
    pub chartex: bool,
    /// 没有带缓存值的系列 → `None`（记 `CHART_NO_SERIES`）。
    pub display: Option<ChartDisplay>,
}

impl ChartPart {
    /// 解析一个图表 part。`dom` 为 `None` = part 是二进制或解析失败（`Package` 已记 `PKG_OPAQUE_PART`）。
    pub fn build(
        part: PartId,
        dom: Option<&Dom>,
        scheme: &ColorScheme,
        warnings: &mut Vec<Diagnostic>,
    ) -> ChartPart {
        let Some(dom) = dom else {
            return ChartPart { part, root: None, chartex: false, display: None };
        };
        let root = dom.root();
        let chartex = is(dom, root, NsId::Cx, LocalName::ChartSpace);
        if !chartex && !is(dom, root, NsId::C, LocalName::ChartSpace) {
            warnings.push(Diagnostic::pre_existing(
                part,
                range_of(dom, root),
                DiagCode::ModUnparseable,
                "图表 part 的根不是 c:chartSpace / cx:chartSpace",
            ));
            return ChartPart { part, root: Some(root), chartex: false, display: None };
        }
        let display =
            if chartex { chartex_display(dom, root) } else { chart_display(dom, root, scheme) };
        if display.is_none() {
            warnings.push(Diagnostic::pre_existing(
                part,
                range_of(dom, root),
                DiagCode::ChartNoSeries,
                "图表 part 里没有带缓存值的系列",
            ));
        }
        ChartPart { part, root: Some(root), chartex, display }
    }
}

// ---- 经典图表（c:）--------------------------------------------------------------------------------

fn chart_display(dom: &Dom, space: NodeId, scheme: &ColorScheme) -> Option<ChartDisplay> {
    let chart = child(dom, space, NsId::C, LocalName::Chart)?;
    let plot_area = child(dom, chart, NsId::C, LocalName::PlotArea)?;
    // 组合图：第一个画出来的图表元素决定种类（主系列）
    let (plot, kind) = dom.semantic_children(plot_area).find_map(|c| {
        let name = dom.name(c)?;
        (dom.is_ns(c, NsId::C, "c")).then_some(())?;
        plot_kind(name.local).map(|k| (c, k))
    })?;
    let val_of = |n: NodeId| attr(dom, n, LocalName::Val);
    let plot_child_val = |l: LocalName| child(dom, plot, NsId::C, l).and_then(val_of);

    let horizontal =
        kind == ChartKind::Bar && plot_child_val(LocalName::BarDir).as_deref() == Some("bar");
    let grouping = match (kind, plot_child_val(LocalName::Grouping).as_deref()) {
        (ChartKind::Bar | ChartKind::Area, Some("stacked")) => Some(ChartGrouping::Stacked),
        (ChartKind::Bar | ChartKind::Area, Some("percentStacked")) => {
            Some(ChartGrouping::PercentStacked)
        }
        _ => None,
    };
    let scatter_style = plot_child_val(LocalName::ScatterStyle);
    let markers = match kind {
        ChartKind::Line => plot_child_val(LocalName::Marker).as_deref() == Some("1"),
        ChartKind::Scatter => {
            scatter_style.as_ref().is_none_or(|s| s.to_lowercase().contains("marker"))
        }
        _ => false,
    };
    let scatter_lines = kind == ChartKind::Scatter
        && scatter_style.as_ref().is_some_and(|s| {
            let s = s.to_lowercase();
            s.contains("line") || s.contains("smooth")
        });
    let hole_pct = dom
        .name(plot)
        .is_some_and(|n| n.local == LocalName::DoughnutChart)
        .then(|| {
            plot_child_val(LocalName::HoleSize).map_or(50, |v| v.trim().parse::<u32>().unwrap_or(0))
        })
        .filter(|&h| h > 0);
    let legend_pos = child(dom, chart, NsId::C, LocalName::Legend).map(|legend| {
        child(dom, legend, NsId::C, LocalName::LegendPos)
            .and_then(val_of)
            .and_then(|v| legend_pos_of(v.trim()))
            .unwrap_or(LegendPos::Right)
    });

    let mut categories: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for ser in children(dom, plot, NsId::C, LocalName::Ser) {
        // scatter / bubble 用 x / y 对，不是类别 / 值
        let Some(val) = child(dom, ser, NsId::C, LocalName::Val)
            .or_else(|| child(dom, ser, NsId::C, LocalName::YVal))
        else {
            continue;
        };
        let values = cache_numbers(dom, val);
        if values.is_empty() {
            continue;
        }
        let cat = child(dom, ser, NsId::C, LocalName::Cat)
            .or_else(|| child(dom, ser, NsId::C, LocalName::XVal));
        if let Some(cat) = cat
            && categories.is_empty()
        {
            categories =
                cache_points(dom, cat).into_iter().map(Option::unwrap_or_default).collect();
            let fmt = cat_format_code(dom, cat);
            if fmt.as_deref().is_some_and(|f| f.contains(['y', 'd', 'Y', 'D'])) {
                // 日期格式的类别缓存是 Excel 序列号，按日期文字显示
                categories =
                    categories.into_iter().map(|v| serial_date_text(&v).unwrap_or(v)).collect();
            } else if dom.name(cat).is_some_and(|n| n.local == LocalName::XVal) {
                // x 缓存是原始双精度文本（0.70000000000000062），显示前修到 4 位
                categories = categories
                    .into_iter()
                    .map(|v| match v.trim().parse::<f64>() {
                        Ok(n) if !v.is_empty() && n.is_finite() => {
                            format!("{}", (n * 10000.0).round() / 10000.0)
                        }
                        _ => v,
                    })
                    .collect();
            }
        }
        let color =
            child(dom, ser, NsId::C, LocalName::SpPr).and_then(|sp| solid_fill(dom, sp, scheme));
        let point_colors = data_point_colors(dom, ser, scheme);
        let (mut x_values, mut sizes, mut line) = (None, None, false);
        if matches!(kind, ChartKind::Scatter | ChartKind::Bubble) {
            x_values = child(dom, ser, NsId::C, LocalName::XVal)
                .map(|n| cache_numbers(dom, n))
                .filter(|xs| xs.iter().any(Option::is_some));
            sizes = child(dom, ser, NsId::C, LocalName::BubbleSize)
                .map(|n| cache_numbers(dom, n))
                .filter(|xs| xs.iter().any(Option::is_some));
            line = scatter_lines && !series_line_hidden(dom, ser);
        }
        series.push(ChartSeries {
            node: ser,
            name: series_name(dom, ser),
            values,
            color,
            point_colors,
            x_values,
            sizes,
            line,
        });
    }
    if series.is_empty() {
        return None;
    }

    let style_val = style_val(dom, space);
    let title_node = child(dom, chart, NsId::C, LocalName::Title);
    let mut title = title_node.and_then(|t| chart_title(dom, chart, t));
    // Office 给单系列图的自动标题取系列名
    if title.as_deref() == Some("Chart Title")
        && series.len() == 1
        && let Some(name) = &series[0].name
    {
        title = Some(name.clone());
    }
    Some(ChartDisplay {
        root: space,
        kind,
        plot: Some(plot),
        horizontal,
        grouping,
        markers,
        hole_pct,
        legend_pos,
        title,
        title_node,
        categories,
        series,
        style_val,
        palette: palette(style_val, scheme),
        chartex: false,
    })
}

fn legend_pos_of(v: &str) -> Option<LegendPos> {
    Some(match v {
        "b" => LegendPos::Bottom,
        "l" => LegendPos::Left,
        "r" => LegendPos::Right,
        "t" => LegendPos::Top,
        "tr" => LegendPos::TopRight,
        _ => return None,
    })
}

/// `c:title` 的文字：`a:t` 拼接 → `c:v` 拼接（`strRef` 标题）→ 自动标题占位（除非 `c:autoTitleDeleted` 为真）。
fn chart_title(dom: &Dom, chart: NodeId, title: NodeId) -> Option<String> {
    let joined = |ns: NsId, local: LocalName| -> String {
        dom.semantic_descendants(title)
            .filter(|&n| is(dom, n, ns, local))
            .map(|n| text_of(dom, n))
            .collect::<String>()
    };
    let rich = joined(NsId::A, LocalName::T);
    if !rich.is_empty() {
        return Some(rich);
    }
    let cached = joined(NsId::C, LocalName::V);
    if !cached.is_empty() {
        return Some(cached);
    }
    // 没有文字的 `c:title` = 自动标题，Word 显示 "Chart Title" 占位；`CT_Boolean`：无 val 与 val="true" 都是真
    let deleted = child(dom, chart, NsId::C, LocalName::AutoTitleDeleted).is_some_and(|d| {
        attr(dom, d, LocalName::Val).is_none_or(|v| matches!(v.trim(), "1" | "true"))
    });
    (!deleted).then(|| "Chart Title".to_string())
}

/// `c:style/@val`（1–48），或 Word 2010 的 `mc:AlternateContent` 包装（`c14:style` 101–148，减 100）。
fn style_val(dom: &Dom, space: NodeId) -> Option<u8> {
    // 语义遍历已经选好 MCE 分支：选中 `c14` 时看到的是 `c14:style`，退到 Fallback 时是 `c:style`
    let node = dom.semantic_children(space).find(|&c| {
        dom.name(c).is_some_and(|n| n.local == LocalName::Style)
            && (dom.is_ns(c, NsId::C, "c") || dom.is_ns(c, NsId::C14, "c14"))
    })?;
    let mut v: i64 = attr(dom, node, LocalName::Val)?.trim().parse().ok()?;
    if v > 100 {
        v -= 100;
    }
    (1..=48).contains(&v).then_some(v as u8)
}

/// Word 灰阶图表样式（样式列 1）的系列色，近似值（TS `GRAYSCALE_PALETTE`）。
pub const GRAYSCALE_PALETTE: [[u8; 3]; 6] = [
    [0x59, 0x59, 0x59],
    [0xD9, 0xD9, 0xD9],
    [0xA6, 0xA6, 0xA6],
    [0x40, 0x40, 0x40],
    [0xBF, 0xBF, 0xBF],
    [0x8C, 0x8C, 0x8C],
];

/// 图表样式的系列颜色循环：`c:style` 1–48 排成 8 列的样式库，列 1 灰阶、列 2 六个 accent 轮转、
/// 列 3–8 单色（一个 accent 的 tint / shade 阶梯）。没有 `c:style` → 列 2。主题缺任一 accent → `None`。
pub fn palette(style_val: Option<u8>, scheme: &ColorScheme) -> Option<[Rgb; 6]> {
    let pos = style_val.map_or(2, |v| (u32::from(v) - 1) % 8 + 1);
    if pos == 1 {
        return Some(GRAYSCALE_PALETTE.map(|c| c.map(f64::from)));
    }
    let accents = [
        ThemeSlot::Accent1,
        ThemeSlot::Accent2,
        ThemeSlot::Accent3,
        ThemeSlot::Accent4,
        ThemeSlot::Accent5,
        ThemeSlot::Accent6,
    ];
    let mut list = [[0.0; 3]; 6];
    for (out, slot) in list.iter_mut().zip(accents) {
        *out = scheme.get(slot)?.map(f64::from);
    }
    if pos == 2 {
        return Some(list);
    }
    let base = list[(pos - 3) as usize];
    let tint = |t: f64| base.map(|c| c * t + 255.0 * (1.0 - t));
    let shade = |s: f64| base.map(|c| c * s);
    Some([base, tint(0.6), shade(0.75), tint(0.3), shade(0.5), tint(0.8)])
}

/// `spPr/a:solidFill` 的颜色（`RES-05`）。
fn solid_fill(dom: &Dom, sp_pr: NodeId, scheme: &ColorScheme) -> Option<ChartColor> {
    let fill = child(dom, sp_pr, NsId::A, LocalName::SolidFill)?;
    let def = color_in(dom, fill)?;
    let rgb = def.to_rgb(scheme);
    Some(ChartColor { def, rgb })
}

/// `c:dPt` 逐点填充，按 `c:idx` 稀疏；一处都没有 → `None`。
fn data_point_colors(
    dom: &Dom,
    ser: NodeId,
    scheme: &ColorScheme,
) -> Option<Vec<Option<ChartColor>>> {
    let mut out: Vec<Option<ChartColor>> = Vec::new();
    for d_pt in children(dom, ser, NsId::C, LocalName::DPt) {
        let Some(idx) = child(dom, d_pt, NsId::C, LocalName::Idx)
            .and_then(|i| attr(dom, i, LocalName::Val))
            .and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let Some(color) =
            child(dom, d_pt, NsId::C, LocalName::SpPr).and_then(|sp| solid_fill(dom, sp, scheme))
        else {
            continue;
        };
        if out.len() <= idx {
            out.resize(idx + 1, None);
        }
        out[idx] = Some(color);
    }
    (!out.is_empty()).then_some(out)
}

/// 系列的 `a:ln` 明写 `a:noFill`（散点只画标记）。
fn series_line_hidden(dom: &Dom, ser: NodeId) -> bool {
    child(dom, ser, NsId::C, LocalName::SpPr)
        .and_then(|sp| child(dom, sp, NsId::A, LocalName::Ln))
        .is_some_and(|ln| child(dom, ln, NsId::A, LocalName::NoFill).is_some())
}

/// `c:tx`：字面 `c:v`，否则缓存的第一个点。
fn series_name(dom: &Dom, ser: NodeId) -> Option<String> {
    let tx = child(dom, ser, NsId::C, LocalName::Tx)?;
    if let Some(v) = child(dom, tx, NsId::C, LocalName::V) {
        return Some(text_of(dom, v));
    }
    cache_points(dom, tx).into_iter().next().flatten()
}

/// `c:cat` / `c:val` / `c:tx` 容器的缓存点文本，按 `idx` 排、`ptCount` 补空。
/// `strRef` / `numRef` 里是 `strCache` / `numCache`，字面量 `strLit` / `numLit` 自己就是缓存。
fn cache_points(dom: &Dom, container: NodeId) -> Vec<Option<String>> {
    let Some(cache) = cache_node(dom, container) else { return Vec::new() };
    let count = child(dom, cache, NsId::C, LocalName::PtCount)
        .and_then(|n| attr(dom, n, LocalName::Val))
        .and_then(|v| v.trim().parse::<usize>().ok());
    let mut points: Vec<Option<String>> = Vec::new();
    for pt in children(dom, cache, NsId::C, LocalName::Pt) {
        let Some(idx) = attr(dom, pt, LocalName::Idx).and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        if points.len() <= idx {
            points.resize(idx + 1, None);
        }
        points[idx] = Some(
            child(dom, pt, NsId::C, LocalName::V).map(|v| text_of(dom, v)).unwrap_or_default(),
        );
    }
    if let Some(n) = count
        && n > points.len()
    {
        points.resize(n, None);
    }
    points
}

fn cache_node(dom: &Dom, container: NodeId) -> Option<NodeId> {
    let reference = child(dom, container, NsId::C, LocalName::StrRef)
        .or_else(|| child(dom, container, NsId::C, LocalName::NumRef));
    match reference {
        Some(r) => child(dom, r, NsId::C, LocalName::StrCache)
            .or_else(|| child(dom, r, NsId::C, LocalName::NumCache)),
        None => child(dom, container, NsId::C, LocalName::StrLit)
            .or_else(|| child(dom, container, NsId::C, LocalName::NumLit)),
    }
}

/// 数值缓存：空白 → `None`，非有限数 → `None`。
fn cache_numbers(dom: &Dom, container: NodeId) -> Vec<Option<f64>> {
    cache_points(dom, container).into_iter().map(|v| v.and_then(|s| parse_number(&s))).collect()
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// 类别缓存的 `c:formatCode`（`numRef/numCache` 或 `numLit`）。
fn cat_format_code(dom: &Dom, container: NodeId) -> Option<String> {
    let cache = match child(dom, container, NsId::C, LocalName::NumRef) {
        Some(r) => child(dom, r, NsId::C, LocalName::NumCache)?,
        None => child(dom, container, NsId::C, LocalName::NumLit)?,
    };
    child(dom, cache, NsId::C, LocalName::FormatCode).map(|n| text_of(dom, n))
}

/// Excel 日期序列号 → `m/d/yyyy`（Word / LibreOffice 画类别轴时显示的是日期不是序列号）。
/// 序列号 0 = 1899-12-30；超出 (0, 80000] 或不是数 → `None`。
pub fn serial_date_text(v: &str) -> Option<String> {
    let n = v.trim().parse::<f64>().ok()?;
    if !n.is_finite() || n <= 0.0 || n > 80000.0 {
        return None;
    }
    // 1899-12-30 距 1970-01-01 是 -25569 天
    let days = n.round() as i64 - 25569;
    let (y, m, d) = civil_from_days(days);
    Some(format!("{m}/{d}/{y}"))
}

/// 自 1970-01-01 起的天数 → 公历 (年, 月, 日)。Howard Hinnant 的算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---- chartex（cx:）--------------------------------------------------------------------------------

/// `cx:chartSpace` 的降级读法：`cx:chartData/cx:data` 的 `strDim` / `numDim` 各一级点缓存，
/// `cx:series` 按 `layoutId` 归并到经典种类。找不到 `cx:chartData` 时接受任何带 `cx:data` 的子元素
/// （消费者必须跳过不认识的元素；测试语料故意改过这个名字）。
fn chartex_display(dom: &Dom, space: NodeId) -> Option<ChartDisplay> {
    let chart_data = child(dom, space, NsId::Cx, LocalName::ChartData).or_else(|| {
        dom.semantic_children(space).find(|&c| child(dom, c, NsId::Cx, LocalName::Data).is_some())
    });
    // data id → (类别, 值)
    let mut data_by_id: Vec<(String, Vec<String>, Vec<Option<f64>>)> = Vec::new();
    for data in chart_data.into_iter().flat_map(|cd| children(dom, cd, NsId::Cx, LocalName::Data)) {
        let id = attr(dom, data, LocalName::Id).unwrap_or_default();
        let (mut cats, mut vals) = (Vec::new(), Vec::new());
        for dim in dom.semantic_children(data) {
            if is(dom, dim, NsId::Cx, LocalName::StrDim) {
                cats =
                    chartex_points(dom, dim).into_iter().map(Option::unwrap_or_default).collect();
            } else if is(dom, dim, NsId::Cx, LocalName::NumDim) {
                vals = chartex_points(dom, dim)
                    .into_iter()
                    .map(|v| v.and_then(|s| parse_number(&s)))
                    .collect();
            }
        }
        data_by_id.push((id, cats, vals));
    }
    let chart = child(dom, space, NsId::Cx, LocalName::Chart);
    let region = chart
        .and_then(|c| child(dom, c, NsId::Cx, LocalName::PlotArea))
        .and_then(|p| child(dom, p, NsId::Cx, LocalName::PlotAreaRegion));
    let mut kind = ChartKind::Other;
    let mut categories: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for ser in region.into_iter().flat_map(|r| children(dom, r, NsId::Cx, LocalName::Series)) {
        if kind == ChartKind::Other
            && let Some(k) =
                attr(dom, ser, LocalName::LayoutId).and_then(|l| chartex_kind(l.trim()))
        {
            kind = k;
        }
        let data_id =
            child(dom, ser, NsId::Cx, LocalName::DataId).and_then(|d| attr(dom, d, LocalName::Val));
        let Some((_, cats, vals)) =
            data_by_id.iter().find(|(id, ..)| Some(id.as_str()) == data_id.as_deref())
        else {
            continue;
        };
        if vals.is_empty() {
            continue;
        }
        if categories.is_empty() {
            categories = cats.clone();
        }
        let name = child(dom, ser, NsId::Cx, LocalName::Tx)
            .and_then(|t| child(dom, t, NsId::Cx, LocalName::TxData))
            .and_then(|t| child(dom, t, NsId::Cx, LocalName::V))
            .map(|v| text_of(dom, v))
            .filter(|s| !s.is_empty());
        series.push(ChartSeries {
            node: ser,
            name,
            values: vals.clone(),
            color: None,
            point_colors: None,
            x_values: None,
            sizes: None,
            line: false,
        });
    }
    if series.is_empty() {
        return None;
    }
    // `cx:title` 与经典图表一样是 `a:t` 富文本
    let title_node = chart.and_then(|c| child(dom, c, NsId::Cx, LocalName::Title));
    let title = title_node
        .map(|t| {
            dom.semantic_descendants(t)
                .filter(|&n| is(dom, n, NsId::A, LocalName::T))
                .map(|n| text_of(dom, n))
                .collect::<String>()
        })
        .filter(|s| !s.is_empty());
    Some(ChartDisplay {
        root: space,
        kind,
        plot: None,
        horizontal: false,
        grouping: None,
        markers: false,
        hole_pct: None,
        legend_pos: None,
        title,
        title_node,
        categories,
        series,
        style_val: None,
        palette: None,
        chartex: true,
    })
}

/// `cx:strDim` / `cx:numDim` 的第一级 `cx:lvl/cx:pt[@idx]`（层级图的叶子标签在第一级）。
fn chartex_points(dom: &Dom, dim: NodeId) -> Vec<Option<String>> {
    let Some(lvl) = child(dom, dim, NsId::Cx, LocalName::Lvl) else { return Vec::new() };
    let mut out: Vec<Option<String>> = Vec::new();
    for pt in children(dom, lvl, NsId::Cx, LocalName::Pt) {
        let Some(idx) = attr(dom, pt, LocalName::Idx).and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        if out.len() <= idx {
            out.resize(idx + 1, None);
        }
        out[idx] = Some(text_of(dom, pt));
    }
    out
}

// ---- DOM 小工具 -------------------------------------------------------------------------------------

/// 命名空间 + 局部名匹配；前缀未绑定时按规范前缀字面量兜底（`Dom::is_ns`）。
fn is(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> bool {
    dom.name(node).is_some_and(|n| n.local == local) && dom.is_ns(node, ns, prefix_of(ns))
}

fn prefix_of(ns: NsId) -> &'static str {
    match ns {
        NsId::C => "c",
        NsId::Cx => "cx",
        NsId::C14 => "c14",
        NsId::A => "a",
        _ => "",
    }
}

fn child(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&c| is(dom, c, ns, local))
}

fn children<'a>(
    dom: &'a Dom,
    node: NodeId,
    ns: NsId,
    local: LocalName,
) -> impl Iterator<Item = NodeId> + 'a {
    dom.semantic_children(node).filter(move |&c| is(dom, c, ns, local))
}

/// 无命名空间属性。
fn attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, local)).map(|s| s.to_string())
}

/// 元素的直接文本内容（`a:t` / `c:v` / `cx:pt`），实体已解码，不 trim。
fn text_of(dom: &Dom, node: NodeId) -> String {
    dom.children(node).iter().filter_map(|&c| dom.text(c)).collect()
}

fn range_of(dom: &Dom, node: NodeId) -> Option<Range<u32>> {
    dom.node(node).lex.as_ref().map(|l| l.range.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_11_chart_kind_table_covers_every_plot_element() {
        // 16 种图（ECMA-376 §21.2.2）都在表里，别的元素不是图
        assert_eq!(PLOT_ELEMENTS.len(), 16);
        for &l in PLOT_ELEMENTS {
            assert!(plot_kind(l).is_some());
        }
        assert_eq!(plot_kind(LocalName::PlotArea), None);
        assert_eq!(plot_kind(LocalName::DoughnutChart), Some(ChartKind::Pie));
        assert_eq!(plot_kind(LocalName::RadarChart), Some(ChartKind::Other));
        assert_eq!(chartex_kind("waterfall"), Some(ChartKind::Bar));
        assert_eq!(chartex_kind("regionMap"), None);
    }

    #[test]
    fn mod_11_excel_serial_dates() {
        assert_eq!(serial_date_text("37377").as_deref(), Some("5/1/2002"));
        assert_eq!(serial_date_text("37408").as_deref(), Some("6/1/2002"));
        assert_eq!(serial_date_text("1").as_deref(), Some("12/31/1899"));
        assert_eq!(serial_date_text("45658.4").as_deref(), Some("1/1/2025"));
        assert_eq!(serial_date_text("0"), None);
        assert_eq!(serial_date_text("80001"), None);
        assert_eq!(serial_date_text("abc"), None);
    }

    #[test]
    fn mod_11_palette_columns() {
        let office = ColorScheme::office_default();
        let hex6 = |p: [Rgb; 6]| p.map(crate::resolve::drawingml::hex);
        // 缺省 / 列 2 = 六个 accent
        assert_eq!(hex6(palette(None, &office).unwrap())[0], "4472C4");
        assert_eq!(hex6(palette(Some(2), &office).unwrap())[5], "70AD47");
        assert_eq!(hex6(palette(Some(10), &office).unwrap())[0], "4472C4");
        // 列 1 灰阶
        assert_eq!(hex6(palette(Some(1), &office).unwrap())[0], "595959");
        assert_eq!(hex6(palette(Some(41), &office).unwrap())[1], "D9D9D9");
        // 列 3–8 单色阶梯：以对应 accent 起头，六个颜色互不相同
        let mono = hex6(palette(Some(5), &office).unwrap());
        assert_eq!(mono[0], "A5A5A5");
        assert_eq!(mono.iter().collect::<std::collections::BTreeSet<_>>().len(), 6);
        assert_eq!(hex6(palette(Some(40), &office).unwrap())[0], "70AD47");
    }
}
