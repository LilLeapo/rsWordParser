//! TS `TableModel` 的投影（`COMPAT-10`，任务 3.5）。
//!
//! 输入是模型的 [`TableBlock`] 与 `resolve` 的 [`TableView`]（`RES-08`）：折叠 `hMerge`、补
//! `gridBefore`/`gridAfter` 占位、列宽的四条启发式、条件格式都在视图里做完了，这里只负责摆成
//! TS 的形状。唯一直接读 DOM 的地方是 `textOf`（TS 的段落纯文本）与深度 ≥ 8 的扁平化——那时模型
//! 已经在 64 层截断了，拿不到更深的段落。

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::model::table::{Cell, TableBlock};
use crate::model::{Block, Revision, RevisionMeta, Style, StyleType, TextBlock};
use crate::resolve::rgb_hex;
use crate::resolve::{Resolver, TableView, ViewCell};
use crate::semantic::props::{
    Border, CellProps, HeightRule, ParaProps, RunProps, TableProps, TblBorders, TblCellMar,
    TblWidth, TcBorders, TcMar, Val,
};
use crate::xml::{Dom, LocalName, NodeId, QName};

use super::blocks::{
    Ctx, empty_para_font, empty_para_size, para_format, revision_info, runs_json, set,
};
use super::decl::{list_kind, shd_display_fill};

/// TS `MAX_TABLE_NEST_DEPTH`：这一层的表格深度 ≥ 8 时，它的子表整棵扁平化成 1×1。
const MAX_TABLE_NEST_DEPTH: usize = 8;

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// `blocks[*].table`：`depth` 是这张表的嵌套层数（最外层为 1），`docx_index` 是这张表所在块的下标
/// （单元格里的锚定框要用它判首页）。
pub(super) fn table_json(
    ctx: &Ctx<'_>,
    t: &TableBlock,
    depth: usize,
    docx_index: usize,
) -> Option<Value> {
    let view = ctx.resolver.table(ctx.dom, t);
    let cols = view.columns();
    // TS 丢掉没有格的行；每行的附属数组都跟着这个过滤
    let kept: Vec<usize> = (0..t.rows.len()).filter(|&r| !t.rows[r].cells.is_empty()).collect();
    if kept.is_empty() {
        return None;
    }
    let mut o = Map::new();

    // TS `attachRawTablePr`：只给顶层表，且直接子节点的行 / 格数必须对得上，否则宁可不挂
    let raw = raw_table_pr(ctx, t, cols, &kept, depth);
    let mut rows = Vec::with_capacity(kept.len());
    for (k, &r) in kept.iter().enumerate() {
        let mut cells = Vec::new();
        let mut real = 0usize;
        for vc in &cols.rows[r] {
            let raw_tc = if vc.gap {
                None
            } else {
                let v = raw.tc.get(k).and_then(|row| row.get(real).cloned().flatten());
                real += 1;
                v
            };
            cells.push(cell_json(ctx, &view, r, vc, depth, docx_index, raw_tc));
        }
        rows.push(Value::Array(cells));
    }
    set(&mut o, "rows", Value::Array(rows));

    if !cols.widths_pct.is_empty() {
        set(
            &mut o,
            "colWidthsPct",
            Value::Array(cols.widths_pct.iter().map(|&p| json_num(p)).collect()),
        );
    }
    if !cols.widths_twips.is_empty() {
        set(
            &mut o,
            "colWidthsTwips",
            Value::Array(cols.widths_twips.iter().map(|&w| Value::from(w)).collect()),
        );
    }

    let props: &TableProps = &t.props;
    let width_pct =
        props.width.as_ref().and_then(TblWidth::percent).filter(|p| *p > 0.0 && *p <= 100.0);
    if let Some(p) = width_pct {
        set(&mut o, "widthPct", json_num(p));
    }
    // autoLayout / autoFit / fixedLayout
    let fixed = matches!(
        props.layout.as_ref().and_then(|l| l.kind.as_ref()).and_then(Val::value),
        Some(crate::semantic::props::TblLayoutType::Fixed)
    );
    let auto_width = match props.width.as_ref() {
        None => true,
        Some(w) => match w.kind.as_ref().and_then(Val::value) {
            Some(crate::semantic::props::TblWidthType::Auto) => true,
            Some(crate::semantic::props::TblWidthType::Dxa) | None => {
                !w.twips().is_some_and(|v| v > 0)
            }
            _ => false,
        },
    };
    if !fixed && (auto_width || width_pct.is_some()) {
        set(&mut o, "autoLayout", true);
    }
    let auto_fit = if fixed || (!auto_width && width_pct.is_none()) {
        "fixed"
    } else if width_pct == Some(100.0) {
        "window"
    } else {
        "contents"
    };
    set(&mut o, "autoFit", auto_fit);
    if fixed {
        set(&mut o, "fixedLayout", true);
    }
    if let Some(m) = view.cell_margins().map(|e| e.value).as_ref().and_then(margins_json) {
        set(&mut o, "cellMarTwips", m);
    }
    // cellSpacing：表格级，缺省看第一行
    let spacing =
        props.cell_spacing.as_ref().and_then(TblWidth::twips).filter(|&v| v > 0).or_else(|| {
            t.rows
                .first()
                .and_then(|r| r.props.cell_spacing.as_ref())
                .and_then(TblWidth::twips)
                .filter(|&v| v > 0)
        });
    if let Some(v) = spacing {
        set(&mut o, "cellSpacingTwips", v);
    }
    if let Some(fill) = props.shading.as_ref().and_then(shd_display_fill) {
        set(&mut o, "fill", fill);
    }
    if let Some(b) = view.borders().map(|e| e.value).as_ref().and_then(tbl_borders_json) {
        set(&mut o, "borders", b);
    }
    if let Some(a) = props.jc.as_ref().and_then(Val::value).and_then(|j| match j {
        crate::semantic::props::JcTable::Center => Some("center"),
        crate::semantic::props::JcTable::Right | crate::semantic::props::JcTable::End => {
            Some("right")
        }
        _ => None,
    }) {
        set(&mut o, "align", a);
    }
    if let Some((side, pos)) = float_json(props) {
        set(&mut o, "floatSide", side);
        set(&mut o, "floatPos", pos);
    }
    if let Some(ind) = props.indent.as_ref().and_then(TblWidth::twips).filter(|&v| v != 0) {
        set(&mut o, "indentTwips", ind);
    }
    // 每行对齐的数组
    let mut heights = Vec::new();
    let mut rules = Vec::new();
    let mut headers = Vec::new();
    let mut revs = Vec::new();
    for &r in &kept {
        match view.row_height(r) {
            Some((h, rule)) => {
                heights.push(Value::from(h));
                rules.push(Value::from(match rule {
                    HeightRule::Exact => "exact",
                    _ => "atLeast",
                }));
            }
            None => {
                heights.push(Value::Null);
                rules.push(Value::Null);
            }
        }
        headers.push(Value::from(t.rows[r].props.tbl_header == Some(true)));
        revs.push(row_revision(&t.rows[r].revisions));
    }
    if heights.iter().any(|v| !v.is_null()) {
        set(&mut o, "rowHeightsTwips", Value::Array(heights));
        set(&mut o, "rowHeightRules", Value::Array(rules));
    }
    set(&mut o, "repeatHeaderRows", Value::Array(headers));
    if raw.tr.iter().any(Option::is_some) {
        set(
            &mut o,
            "rawTrPrs",
            Value::Array(
                raw.tr.iter().map(|v| v.clone().map_or(Value::Null, Value::from)).collect(),
            ),
        );
    }
    if revs.iter().any(|v| !v.is_null()) {
        set(&mut o, "rowRevisions", Value::Array(revs));
    }
    if let Some(id) = &t.style_id {
        set(&mut o, "tblStyleId", id.clone());
    }
    let look = view.look();
    set(
        &mut o,
        "tableLook",
        serde_json::json!({
            "firstRow": look.first_row,
            "lastRow": look.last_row,
            "firstColumn": look.first_column,
            "lastColumn": look.last_column,
            "bandedRows": look.banded_rows,
            "bandedColumns": look.banded_columns,
        }),
    );
    if props.bidi_visual == Some(true) {
        set(&mut o, "bidiVisual", true);
    }
    Some(Value::Object(o))
}

/// TS `attachRawTablePr` 的结果：每行的 `w:trPr` 原字节，以及每行每个真实格的 `w:tcPr` 原字节。
#[derive(Default)]
struct RawTablePr {
    tr: Vec<Option<String>>,
    tc: Vec<Vec<Option<String>>>,
}

/// TS `attachRawTablePr`：**只给顶层表**（嵌套表走 `extractTableModel`，没有这一步），而且宁可不挂也
/// 不挂错——`w:tbl` 的**直接** `w:tr` 子节点数与行数不符（行被 sdt 包着就会这样）→ 整张表都不挂；
/// 某行的直接 `w:tc` 数与该行的真实格数不符（`hMerge` 折叠过）→ 那一行不挂格属性，但行属性照挂。
fn raw_table_pr(
    ctx: &Ctx<'_>,
    t: &TableBlock,
    cols: &crate::resolve::ColumnView,
    kept: &[usize],
    depth: usize,
) -> RawTablePr {
    let mut raw = RawTablePr { tr: vec![None; kept.len()], tc: vec![Vec::new(); kept.len()] };
    if depth != 1 {
        return raw;
    }
    let dom = ctx.dom;
    let trs: Vec<NodeId> =
        dom.children(t.node).iter().copied().filter(|&n| dom.is(n, w(LocalName::Tr))).collect();
    if trs.len() != kept.len() {
        return raw;
    }
    for (k, &tr) in trs.iter().enumerate() {
        if let Some(pr) = dom.children(tr).iter().copied().find(|&n| dom.is(n, w(LocalName::TrPr)))
        {
            raw.tr[k] = Some(ctx.slice(&ctx.lex_range(pr)).to_string());
        }
        let tcs: Vec<NodeId> =
            dom.children(tr).iter().copied().filter(|&n| dom.is(n, w(LocalName::Tc))).collect();
        let real = cols.rows[kept[k]].iter().filter(|c| !c.gap).count();
        if tcs.len() != real {
            continue;
        }
        raw.tc[k] = tcs
            .iter()
            .map(|&tc| {
                dom.children(tc)
                    .iter()
                    .copied()
                    .find(|&n| dom.is(n, w(LocalName::TcPr)))
                    .map(|pr| ctx.slice(&ctx.lex_range(pr)).to_string())
            })
            .collect();
    }
    raw
}

/// 整行的插入 / 删除修订（`trPr/ins|del`，`MOD-09`）。
fn row_revision(revs: &[Revision]) -> Value {
    for r in revs {
        let (kind, meta): (&str, &RevisionMeta) = match r {
            Revision::Insert(m) => ("ins", m),
            Revision::Delete(m) => ("del", m),
            _ => continue,
        };
        let mut info = revision_info(meta);
        set(&mut info, "kind", kind);
        return Value::Object(info);
    }
    Value::Null
}

#[allow(clippy::too_many_arguments)]
fn cell_json(
    ctx: &Ctx<'_>,
    view: &TableView<'_>,
    row: usize,
    vc: &ViewCell,
    depth: usize,
    docx_index: usize,
    raw_tc_pr: Option<String>,
) -> Value {
    let mut o = Map::new();
    let Some(idx) = vc.cell else {
        // gridBefore / gridAfter 的显示占位
        set(&mut o, "paras", Value::Array(Vec::new()));
        set(&mut o, "gridGap", true);
        if vc.span > 1 {
            set(&mut o, "colSpan", vc.span);
        }
        return Value::Object(o);
    };
    let cell = &view.table().rows[row].cells[idx];
    let props: &CellProps = &cell.props;

    let mut paras: Vec<Value> = Vec::new();
    let mut rich: Vec<Value> = Vec::new();
    let mut nested: Vec<Value> = Vec::new();
    let mut anchors: Vec<usize> = Vec::new();
    let mut boxes: Vec<Value> = Vec::new();
    let mut box_anchors: Vec<usize> = Vec::new();
    let mut jcs: BTreeSet<String> = BTreeSet::new();
    let mut saw_bold = false;
    let mut saw_non_bold = false;
    let mut colors: BTreeSet<String> = BTreeSet::new();

    for b in &cell.blocks {
        if let Block::Table(inner) = b {
            let model = if depth >= MAX_TABLE_NEST_DEPTH {
                flattened(ctx.dom, inner.node)
            } else {
                table_json(ctx, inner, depth + 1, docx_index)
            };
            if let Some(m) = model {
                nested.push(m);
                anchors.push(paras.len());
            }
            continue;
        }
        let node = b.node();
        if !ctx.dom.is(node, w(LocalName::P)) {
            continue;
        }
        // 格里的锚定形状：Word 画在格内并把行撑高，所以挂在格上而不是把整段降级成 `Text box`
        let (found, stripped) = match b.as_text() {
            Some(tb) => super::textbox::anchored_boxes_in_cell(ctx, node, tb, docx_index),
            None => (Vec::new(), Vec::new()),
        };
        if !found.is_empty() {
            box_anchors.extend(std::iter::repeat_n(paras.len(), found.len()));
            boxes.extend(found.into_iter().map(|b| Value::Object(b.json)));
        }
        let text = text_of_skipping(ctx.dom, node, &stripped);
        paras.push(Value::from(text.clone()));
        rich.push(rich_para(ctx, b, node, &text));
        if !text.is_empty() {
            jcs.insert(declared_jc(ctx.dom, node).unwrap_or_default());
        }
        for r in ctx.dom.semantic_children(node).filter(|&n| ctx.dom.is(n, w(LocalName::R))) {
            let rpr = ctx.dom.semantic_children(r).find(|&n| ctx.dom.is(n, w(LocalName::RPr)));
            let bold = rpr.is_some_and(|n| bool_prop(ctx.dom, n, LocalName::B));
            if bold {
                saw_bold = true;
            } else {
                saw_non_bold = true;
            }
            if !text_of(ctx.dom, r).is_empty() {
                colors.insert(run_color(ctx, rpr).unwrap_or_else(|| "none".to_string()));
            }
        }
    }
    let para_count = paras.len();
    set(&mut o, "paras", Value::Array(paras));
    set(&mut o, "richParas", Value::Array(rich));
    if !boxes.is_empty() {
        set(&mut o, "anchoredBoxes", Value::Array(boxes));
        set(
            &mut o,
            "anchoredBoxAnchors",
            Value::Array(box_anchors.into_iter().map(Value::from).collect()),
        );
    }
    if !nested.is_empty() {
        set(&mut o, "nestedTables", Value::Array(nested));
        set(
            &mut o,
            "nestedTableAnchors",
            Value::Array(anchors.iter().map(|&a| Value::from(a.min(para_count))).collect()),
        );
    }
    if let Some(m) = props.margins.as_ref().and_then(tc_margins_json) {
        set(&mut o, "cellMarTwips", m);
    }
    if vc.span > 1 {
        set(&mut o, "colSpan", vc.span);
    }
    if let Some(m) = &props.v_merge {
        set(&mut o, "vMerge", if m.is_restart() { "restart" } else { "continue" });
    }
    if let Some(m) = &props.h_merge {
        set(&mut o, "hMerge", if m.is_restart() { "restart" } else { "continue" });
    }
    // 自身底纹优先；没有就用条件格式补（TS applyTableStyleDisplay）
    let eff = view.cell(row, idx);
    let own_fill = props.shading.as_ref().and_then(shd_display_fill);
    let fill = own_fill.or_else(|| eff.props.shading.as_ref().and_then(shd_display_fill));
    if let Some(f) = fill {
        set(&mut o, "fill", f);
    }
    // TS：格内每个 run 都加粗 → `bold: true`；否则条件格式 / 整表的加粗补上（只补 true，不写 false）
    if (saw_bold && !saw_non_bold) || eff.rpr.bold == Some(true) {
        set(&mut o, "bold", true);
    }
    let own_color =
        (colors.len() == 1).then(|| colors.iter().next().cloned().unwrap()).filter(|c| c != "none");
    let color = own_color
        .or_else(|| eff.rpr.color.as_ref().and_then(|c| ctx.resolver.color(c)).map(rgb_hex));
    if let Some(c) = color {
        set(&mut o, "color", c);
    }
    if jcs.len() == 1
        && let Some(jc) = jcs.iter().next()
        && ["center", "right", "left", "justify"].contains(&jc.as_str())
    {
        set(&mut o, "align", jc.clone());
    }
    if let Some(v) = props.v_align.as_ref().and_then(Val::value) {
        use crate::semantic::props::VerticalJc as V;
        if let Some(s) = match v {
            V::Top => Some("top"),
            V::Center => Some("center"),
            V::Bottom => Some("bottom"),
            V::Both => None,
        } {
            set(&mut o, "vAlign", s);
        }
    }
    if let Some(d) = props.text_direction.as_ref().and_then(Val::value) {
        use crate::semantic::props::TextDirection as D;
        if let Some(s) = match d {
            D::TbRl | D::TbRlV => Some("tbRl"),
            D::BtLr | D::LrTbV => Some("btLr"),
            _ => None,
        } {
            set(&mut o, "textDirection", s);
        }
    }
    if let Some(b) = view.cell_borders(row, idx).as_ref().and_then(tc_borders_json) {
        set(&mut o, "borders", b);
    }
    if let Some(raw) = raw_tc_pr {
        set(&mut o, "rawTcPr", raw);
    }
    for r in &cell.revisions {
        let (kind, meta) = match r {
            Revision::CellInsert(m) => ("ins", m),
            Revision::CellDelete(m) => ("del", m),
            _ => continue,
        };
        let mut info = revision_info(meta);
        set(&mut info, "kind", kind);
        set(&mut o, "cellRevision", Value::Object(info));
        break;
    }
    Value::Object(o)
}

/// 一段的 `richParas` 条目。非文本块（图片 / 保护块）退化成纯文本 run。
fn rich_para(ctx: &Ctx<'_>, b: &Block, node: NodeId, text: &str) -> Value {
    let mut p = Map::new();
    let ppr = ctx.dom.semantic_children(node).find(|&n| ctx.dom.is(n, w(LocalName::PPr)));
    let tb: Option<&TextBlock> = b.as_text();
    let runs = match tb {
        Some(tb) => runs_json(ctx, tb),
        // 非文本块：格里只有一张图的段落是 `MOD-05` R15 的图片块，格里的保护块同理——
        // TS 在单元格里一律当普通段落，所以图片补成一个原子 run，其余退化成纯文本 run
        None => match block_display(b).and_then(|d| super::image::block_image_run(ctx, d)) {
            Some(r) => vec![r],
            None if text.is_empty() => Vec::new(),
            None => {
                let mut r = Map::new();
                set(&mut r, "text", text.to_string());
                vec![r]
            }
        },
    };
    if let Some(tb) = tb
        && let Some(Value::Object(f)) = para_format(ctx, tb, ppr, runs.is_empty())
    {
        for (k, v) in f {
            p.insert(k, v);
        }
    }
    if let Some(pr) = ppr
        && let Some(style) = ctx
            .dom
            .semantic_children(pr)
            .find(|&n| ctx.dom.is(n, w(LocalName::PStyle)))
            .and_then(|n| ctx.dom.attr_value(n, w(LocalName::Val)))
    {
        set(&mut p, "styleId", style.into_owned());
    }
    if runs.is_empty() {
        if let Some(sz) = empty_para_size(ctx, node, ppr) {
            set(&mut p, "emptyRunSizeHalfPoints", sz);
        }
        if let Some(font) = empty_para_font(ctx, node, ppr) {
            set(&mut p, "emptyRunFontFamily", font);
        }
    }
    if let Some(crate::model::TextKind::ListItem { list }) = tb.map(|t| &t.kind) {
        set(
            &mut p,
            "list",
            serde_json::json!({
                "kind": list_kind(ctx.numbering, i64::from(list.num_id), i64::from(list.ilvl)),
                "numId": list.num_id.to_string(),
                "ilvl": list.ilvl,
            }),
        );
    }
    set(&mut p, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
    Value::Object(p)
}

/// 块上挂的显示模型（图片块 / 保护块）。
fn block_display(b: &Block) -> Option<&crate::model::Display> {
    match b {
        Block::Image(i) => i.display.as_ref(),
        Block::Protected(p) => p.display.as_ref(),
        _ => None,
    }
}

/// TS `flattenedTableModel`：整棵子树按段落收集纯文本，成一个 1×1 的只读表。
/// 迭代实现——这里的子树可能有几千层（模型在 64 层就截断了，所以只能直接读 DOM）。
fn flattened(dom: &Dom, tbl: NodeId) -> Option<Value> {
    enum Step {
        Node(NodeId),
        ParaEnd,
    }
    let mut paras: Vec<String> = Vec::new();
    let mut buf: Option<String> = None;
    let mut stack = vec![Step::Node(tbl)];
    while let Some(step) = stack.pop() {
        let n = match step {
            Step::ParaEnd => {
                paras.push(buf.take().unwrap_or_default());
                continue;
            }
            Step::Node(n) => n,
        };
        if let Some(t) = dom.text(n) {
            if let Some(b) = &mut buf {
                b.push_str(&t);
            }
            continue;
        }
        if buf.is_none() && dom.is(n, w(LocalName::P)) {
            buf = Some(String::new());
            stack.push(Step::ParaEnd);
        }
        for &c in dom.children(n).iter().rev() {
            stack.push(Step::Node(c));
        }
    }
    if paras.is_empty() {
        return None;
    }
    let rich: Vec<Value> = paras
        .iter()
        .map(|text| {
            let runs = if text.is_empty() {
                Vec::new()
            } else {
                vec![serde_json::json!({ "text": text })]
            };
            serde_json::json!({ "runs": runs })
        })
        .collect();
    Some(serde_json::json!({
        "rows": [[{ "paras": paras, "richParas": rich }]],
        "autoLayout": true,
    }))
}

/// TS `textOf`：子树里所有文本节点拼接（含 `w:delText` / `w:instrText`）。
fn text_of(dom: &Dom, node: NodeId) -> String {
    text_of_skipping(dom, node, &[])
}

/// 同上，但跳过 `skip` 里那些子树——TS 在取单元格文字前会把锚定形状从段落里删掉再解析，
/// 不跳的话框里的文字与 `wp:posOffset` 的数字会漏进单元格文本（`COMPAT-10`）。
///
/// 走**语义**子节点：MCE 的非活动分支不算数（TS 没有 MCE，靠正则删 `mc:Fallback` 达到同样效果，
/// 但两边选的分支可能不同——见 `KNOWN_DIFFS` 的 `Requires` 前缀未声明那条）。
fn text_of_skipping(dom: &Dom, node: NodeId, skip: &[NodeId]) -> String {
    let mut out = String::new();
    let mut stack = vec![node];
    let mut order = Vec::new();
    while let Some(n) = stack.pop() {
        if skip.contains(&n) {
            continue;
        }
        order.push(n);
        let kids: Vec<NodeId> = dom.semantic_children(n).collect();
        for &c in kids.iter().rev() {
            stack.push(c);
        }
    }
    for n in order {
        if let Some(t) = dom.text(n) {
            out.push_str(&t);
        }
    }
    out
}

/// 段落自己声明的 `w:jc`（原文，不解析）。
fn declared_jc(dom: &Dom, p: NodeId) -> Option<String> {
    let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)))?;
    let jc = dom.semantic_children(ppr).find(|&n| dom.is(n, w(LocalName::Jc)))?;
    Some(dom.attr_value(jc, w(LocalName::Val))?.into_owned())
}

/// TS `boolProp`：元素存在即 true，除非 `w:val` 明确关掉。
fn bool_prop(dom: &Dom, parent: NodeId, local: LocalName) -> bool {
    let Some(n) = dom.semantic_children(parent).find(|&n| dom.is(n, w(local))) else {
        return false;
    };
    !matches!(
        dom.attr_value(n, w(LocalName::Val)).as_deref(),
        Some("0") | Some("false") | Some("off")
    )
}

/// TS `colorFrom(rPr)`：run 的显示色。
fn run_color(ctx: &Ctx<'_>, rpr: Option<NodeId>) -> Option<String> {
    let rpr = rpr?;
    let mut diags = Vec::new();
    let props: RunProps = crate::semantic::props::read_run_props(ctx.dom, Some(rpr), &mut diags);
    props.color.as_ref().and_then(|c| ctx.resolver.color(c)).map(rgb_hex)
}

fn border_json(b: &Border) -> Option<Value> {
    let style = b.val.as_ref()?.write_str();
    let mut o = Map::new();
    set(&mut o, "style", style);
    if let Some(sz) = b.sz.as_ref().and_then(Val::value).copied().filter(|&s| s != 0) {
        set(&mut o, "szEighths", sz);
    }
    if let Some(c) = b.color.as_ref() {
        set(&mut o, "color", c.write_str());
    }
    Some(Value::Object(o))
}

fn tbl_borders_json(b: &TblBorders) -> Option<Value> {
    let mut o = Map::new();
    for (k, v) in [
        ("top", &b.top),
        ("left", &b.start),
        ("bottom", &b.bottom),
        ("right", &b.end),
        ("insideH", &b.inside_h),
        ("insideV", &b.inside_v),
    ] {
        if let Some(j) = v.as_ref().and_then(border_json) {
            set(&mut o, k, j);
        }
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

fn tc_borders_json(b: &TcBorders) -> Option<Value> {
    let mut o = Map::new();
    for (k, v) in [("top", &b.top), ("left", &b.start), ("bottom", &b.bottom), ("right", &b.end)] {
        if let Some(j) = v.as_ref().and_then(border_json) {
            set(&mut o, k, j);
        }
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

fn margins_json(m: &TblCellMar) -> Option<Value> {
    sides_json([("top", &m.top), ("left", &m.start), ("bottom", &m.bottom), ("right", &m.end)])
}

fn tc_margins_json(m: &TcMar) -> Option<Value> {
    sides_json([("top", &m.top), ("left", &m.start), ("bottom", &m.bottom), ("right", &m.end)])
}

fn sides_json(sides: [(&str, &Option<TblWidth>); 4]) -> Option<Value> {
    let mut o = Map::new();
    for (k, v) in sides {
        if let Some(w) = v.as_ref().and_then(TblWidth::twips).filter(|&w| w >= 0) {
            set(&mut o, k, w);
        }
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

/// 浮动表格（`w:tblpPr`）：环绕侧与定位。显示启发式，只出现在这里（`MOD-11`）。
fn float_json(props: &TableProps) -> Option<(&'static str, Value)> {
    let p = props.position.as_ref()?;
    use crate::semantic::props::XAlign;
    let x_spec = p.tblp_x_spec.as_ref().and_then(Val::value).copied();
    let x = p.tblp_x.as_ref().and_then(Val::value).copied();
    let side = if matches!(x_spec, Some(XAlign::Right | XAlign::Outside))
        || (x_spec.is_none() && x.is_some_and(|v| v > 4680))
    {
        "right"
    } else {
        "left"
    };
    let mut pos = Map::new();
    set(&mut pos, "xTwips", x.unwrap_or(if side == "right" { 9360 } else { 0 }));
    set(&mut pos, "yTwips", p.tblp_y.as_ref().and_then(Val::value).copied().unwrap_or(0));
    let anchor = |v: Option<&Val<crate::semantic::props::FrameAnchor>>| -> Option<&'static str> {
        use crate::semantic::props::FrameAnchor as A;
        match v?.value()? {
            A::Page => Some("page"),
            A::Margin => Some("margin"),
            A::Text => Some("text"),
        }
    };
    if let Some(a) = anchor(p.horz_anchor.as_ref()) {
        set(&mut pos, "horzAnchor", a);
    }
    if let Some(a) = anchor(p.vert_anchor.as_ref()) {
        set(&mut pos, "vertAnchor", a);
    }
    let mut dist = Map::new();
    for (k, v) in [
        ("top", &p.top_from_text),
        ("right", &p.right_from_text),
        ("bottom", &p.bottom_from_text),
        ("left", &p.left_from_text),
    ] {
        if let Some(n) = v.as_ref().and_then(Val::value).copied().filter(|&n| n >= 0) {
            set(&mut dist, k, n);
        }
    }
    if !dist.is_empty() {
        set(&mut pos, "distanceTwips", Value::Object(dist));
    }
    Some((side, Value::Object(pos)))
}

fn json_num(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

// ---- styles.*.tableDisplay（TS tableStyleDisplayOf + mergeTableDisplay）--------------------------

/// 表格样式的显示模型：先按样式各自算一份，再沿 basedOn 链根 → 叶合并（六个子对象逐字段深合并，
/// 其余整体覆盖），与 TS `mergeTableDisplay` 一致。
pub(super) fn table_display(r: &Resolver<'_>, id: &str) -> Option<Value> {
    let chain = r.chain(id, StyleType::Table);
    if chain.is_empty() {
        return None;
    }
    let mut out = Map::new();
    for s in chain.iter().rev() {
        let one = style_display(r, s);
        merge_display(&mut out, one);
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

const DEEP_KEYS: &[&str] =
    &["wholeTable", "firstRow", "firstCol", "lastCol", "lastRow", "paraSpacing"];

fn merge_display(base: &mut Map<String, Value>, over: Map<String, Value>) {
    for (k, v) in over {
        if DEEP_KEYS.contains(&k.as_str())
            && let (Some(Value::Object(b)), Value::Object(o)) = (base.get_mut(&k), &v)
        {
            for (kk, vv) in o {
                b.insert(kk.clone(), vv.clone());
            }
            continue;
        }
        base.insert(k, v);
    }
}

fn style_display(r: &Resolver<'_>, s: &Style) -> Map<String, Value> {
    let mut o = Map::new();
    if let Some(fill) = s.tc_pr.as_ref().and_then(|p| p.shading.as_ref()).and_then(shd_display_fill)
    {
        set(&mut o, "fill", fill);
    }
    if let Some(rpr) = &s.rpr
        && let Some(v) = run_display(r, rpr)
    {
        set(&mut o, "wholeTable", v);
    }
    if let Some(tbl) = &s.tbl_pr {
        if let Some(b) = tbl.borders.as_ref().and_then(tbl_borders_json) {
            set(&mut o, "borders", b);
        }
        if let Some(m) = tbl.cell_margins.as_ref().and_then(margins_json) {
            set(&mut o, "cellMarTwips", m);
        }
    }
    if let Some(ppr) = &s.ppr {
        if let Some(jc) = ppr.jc.as_ref() {
            set(&mut o, "paraJc", jc.write_str());
        }
        if let Some(sp) = para_spacing(ppr) {
            set(&mut o, "paraSpacing", sp);
        }
    }
    use crate::semantic::props::TblStyleOverrideType as T;
    for c in &s.conditional {
        let Some(kind) = c.kind.as_ref().and_then(Val::value).copied() else { continue };
        let key = match kind {
            T::FirstRow => "firstRow",
            T::LastRow => "lastRow",
            T::FirstCol => "firstCol",
            T::LastCol => "lastCol",
            T::Band1Horz => "band1Fill",
            T::Band2Horz => "band2Fill",
            _ => continue,
        };
        let fill = c.tc_pr.as_ref().and_then(|p| p.shading.as_ref()).and_then(shd_display_fill);
        if matches!(kind, T::Band1Horz | T::Band2Horz) {
            if let Some(f) = fill {
                set(&mut o, key, f);
            }
            continue;
        }
        let mut cond =
            c.rpr.as_ref().and_then(|rpr| run_display(r, rpr)).map_or_else(Map::new, |v| match v {
                Value::Object(m) => m,
                _ => Map::new(),
            });
        if let Some(f) = fill {
            set(&mut cond, "fill", f);
        }
        if !cond.is_empty() {
            set(&mut o, key, Value::Object(cond));
        }
    }
    o
}

/// 条件格式 / 整表的 run 显示：TS 只取这四项。
fn run_display(r: &Resolver<'_>, rpr: &RunProps) -> Option<Value> {
    let mut o = Map::new();
    if let Some(c) = rpr.color.as_ref().and_then(|c| r.color(c)) {
        set(&mut o, "color", rgb_hex(c));
    }
    if let Some(b) = rpr.bold {
        set(&mut o, "bold", b);
    }
    if let Some(i) = rpr.italic {
        set(&mut o, "italic", i);
    }
    if let Some(sz) = rpr.size.as_ref().and_then(Val::value).copied().filter(|&n| n != 0) {
        set(&mut o, "sizeHalfPoints", sz);
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

fn para_spacing(ppr: &ParaProps) -> Option<Value> {
    let sp = ppr.spacing.as_ref()?;
    let mut o = Map::new();
    if let Some(v) = sp.before.as_ref().and_then(Val::value) {
        set(&mut o, "beforeTwips", *v);
    }
    if let Some(v) = sp.after.as_ref().and_then(Val::value) {
        set(&mut o, "afterTwips", *v);
    }
    if let Some(v) = sp.line.as_ref().and_then(Val::value) {
        set(&mut o, "lineRawTwips", *v);
        use crate::semantic::props::LineSpacingRule as R;
        let rule = sp.line_rule.as_ref().and_then(Val::value).copied().unwrap_or(R::Auto);
        set(
            &mut o,
            "lineRule",
            match rule {
                R::Exact => "exact",
                R::AtLeast => "atLeast",
                R::Auto => "auto",
            },
        );
        if rule == R::Auto {
            set(&mut o, "lineSpacing", json_num(f64::from(*v) / 240.0));
        }
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

/// `Val<T>` 的原文（枚举写字面，`Raw` 写原文）。
trait WriteStr {
    fn write_str(&self) -> String;
}

impl<T: AsStr> WriteStr for Val<T> {
    fn write_str(&self) -> String {
        match self {
            Val::Value(v) => v.as_str().to_string(),
            Val::Raw(s) => s.clone(),
        }
    }
}

/// 有 `as_str` 的值类型（生成的枚举与颜色）。
trait AsStr {
    fn as_str(&self) -> String;
}

impl AsStr for crate::semantic::props::BorderStyle {
    fn as_str(&self) -> String {
        crate::semantic::props::BorderStyle::as_str(*self).to_string()
    }
}

impl AsStr for crate::semantic::props::HexColorOrAuto {
    fn as_str(&self) -> String {
        self.to_xml()
    }
}

impl AsStr for crate::semantic::props::Jc {
    fn as_str(&self) -> String {
        crate::semantic::props::Jc::as_str(*self).to_string()
    }
}

impl AsStr for crate::semantic::props::JcTable {
    fn as_str(&self) -> String {
        crate::semantic::props::JcTable::as_str(*self).to_string()
    }
}

/// 让 `Cell` 的类型出现在文档里（模块只读它）。
#[allow(dead_code)]
fn _cell_type(_: &Cell) {}
