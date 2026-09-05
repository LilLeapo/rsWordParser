//! 页眉页脚的 TS 投影（`COMPAT-05`，`docs/01` §9，`spec/16` 任务 5.4）。
//!
//! 三个来源：
//!
//! - `hfParts{rId}`：主 part 每个 header / footer 关系一条（TS `parseAllHfParts`）。
//! - 顶层 `headerText` / `headerParas` / … ：**default 变体**那一个 part（TS `readHeaderFooterPart`）。
//! - `headerFirst` / `footerEven` / … ：对应 `w:type` 的 part。
//!
//! default 变体的选法照 TS：在**整个 `document.xml`** 里按文档序找 `w:headerReference`，
//! 先取 `w:type="default"`，否则 `w:type="odd"`（非 schema，Word 的"缺省页"），否则没有 `w:type` 的。
//! 注意这是"全文第一个"，**不是**按节——多节文档里它来自第一节。模型侧按节记引用
//! （`SectionInfo.hf_ref` + `RES-10` 继承），两者不冲突：这里复现的是 TS 的顶层字段。
//!
//! `text` 是 TS 的 `plainText(cleaned)`，规则与坐标流**不同**，所以这里直接走 part 的 DOM：
//!
//! | TS | 我们 |
//! | --- | --- |
//! | 只取 `w:t`（不含 `w:delText` / `w:instrText`） | 同 |
//! | `w:t` 原文，不按 `xml:space` 去空白 | 同（坐标流会去，这里不去） |
//! | `</w:tc>` 后若还有文字，补一个空格 | 同 |
//! | `PAGE` / `NUMPAGES` 字段 → `PAGE_MARK` / `TOTAL_PAGES_MARK`，丢掉缓存结果 | 同（按字段索引，不是正则） |
//! | 其他字段 → 只留 `separate` 之后的结果 run | 同（跳过 begin..separate） |
//! | 旧式 `w:pgNum` → `PAGE_MARK` | 同 |
//! | `mc:Fallback` 整块删掉 | 语义遍历本来就只走 Choice（`XML-09`） |

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::bind::compat_ts::blocks::{Ctx, set};
use crate::bind::compat_ts::decl::NumberingOut;
use crate::bind::compat_ts::json::{set_if, set_some};
use crate::bind::compat_ts::{MediaSet, Utf16Index};
use crate::model::{
    Block, Cell, Display, Document, HfKind, HfPart, HfVariant, Inline, TableBlock, TextBlock,
};
use crate::package::{Package, PartId};
use crate::resolve::Resolver;
use crate::span::field::{FieldForm, Keyword};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// TS `PAGE_MARK`：页码字段的位置。私用区字符，这样 part 里字面的 `#` 不会被当成页码。
pub(super) const PAGE_MARK: char = '\u{E001}';
/// TS `TOTAL_PAGES_MARK`：总页数字段的位置。
pub(super) const TOTAL_PAGES_MARK: char = '\u{E000}';

/// 一个 part 的投影结果（`HfPartInfo`）。
struct PartInfo {
    text: String,
    has_page_number: bool,
    paras: Vec<Value>,
    images: Vec<Value>,
    watermark: Option<String>,
}

/// 把页眉页脚的全部顶层键写进 `out`。
pub(super) fn hf_json(
    main: &Ctx<'_>,
    pkg: &Package,
    doc: &Document,
    resolver: &Resolver<'_>,
    numbering: &NumberingOut,
    media: &MediaSet,
    out: &mut Map<String, Value>,
) {
    // 每个 part 一个自己的 `Ctx`（自己的 DOM / rels / 媒体表 / UTF-16 索引），投影一次
    // ——多个 rId 可能指向同一个 part
    let mut infos: BTreeMap<PartId, PartInfo> = BTreeMap::new();
    for (&id, hf) in &doc.hf_parts {
        let Some(hf_dom) = pkg.part(id).dom() else { continue };
        let idx = Utf16Index::new(hf_dom.src());
        let ctx =
            Ctx::new(hf_dom, doc, resolver, &idx, &pkg.part(id).rels, numbering, media.part(id))
                .for_aux(&hf.idx);
        infos.insert(id, part_info(&ctx, hf));
    }

    let mut parts = Map::new();
    for (rid, part) in &doc.hf_by_rel {
        if let Some(info) = infos.get(part) {
            parts.insert(rid.clone(), info_json(info));
        }
    }
    out.insert("hfParts".into(), Value::Object(parts));

    // 顶层字段：default 变体那一个 part 的 `text` / `hasPageNumber` / `paras`（+ 页眉的水印），
    // 加上 first / even 两种 typed 变体的整条 `HfPartInfo`
    for kind in HfKind::ALL {
        let default = pick(main.dom, doc, kind, HfVariant::Default).and_then(|p| infos.get(&p));
        for (suffix, from) in DEFAULT_KEYS {
            if let Some(k) = key_of(kind, suffix) {
                out.insert(k, from(default));
            }
        }
        for (variant, suffix) in [(HfVariant::First, "First"), (HfVariant::Even, "Even")] {
            let v = pick(main.dom, doc, kind, variant)
                .and_then(|p| infos.get(&p))
                .map_or(Value::Null, info_json);
            out.insert(format!("{}{suffix}", kind.as_str()), v);
        }
    }
}

/// default 变体那个 part 贡献的顶层键：TS 的键名后缀与取值。名字集中在这一张表里——
/// 散在几个 `format!` 里迟早写错一个（`watermarkText` 只在页眉出，TS 只读页眉的水印）。
type DefaultKey = (&'static str, fn(Option<&PartInfo>) -> Value);
const DEFAULT_KEYS: [DefaultKey; 5] = [
    ("Text", |i| i.map_or(Value::Null, |i| Value::String(i.text.clone()))),
    ("HasPageNumber", |i| Value::Bool(i.is_some_and(|i| i.has_page_number))),
    ("Paras", |i| i.map_or(Value::Null, |i| Value::Array(i.paras.clone()))),
    ("watermarkText", |i| i.and_then(|i| i.watermark.clone()).map_or(Value::Null, Value::String)),
    // 顶层 `headerImages` / `footerImages`：没有 part 或没有图片都是 `null`
    ("Images", |i| {
        i.map(|i| i.images.clone()).filter(|v| !v.is_empty()).map_or(Value::Null, Value::Array)
    }),
];

/// `(Header, "Text")` → `headerText`；`watermarkText` 是页眉专有的整名。
fn key_of(kind: HfKind, suffix: &str) -> Option<String> {
    match suffix {
        "watermarkText" => (kind == HfKind::Header).then(|| suffix.to_string()),
        _ => Some(format!("{}{suffix}", kind.as_str())),
    }
}

fn info_json(i: &PartInfo) -> Value {
    let mut o = Map::new();
    set(&mut o, "text", i.text.clone());
    set(&mut o, "hasPageNumber", i.has_page_number);
    o.insert("paras".into(), Value::Array(i.paras.clone()));
    // TS 只在非空时给这个键
    if !i.images.is_empty() {
        o.insert("images".into(), Value::Array(i.images.clone()));
    }
    Value::Object(o)
}

/// TS `readHeaderFooterPart` 的引用选择：全文按文档序的 `w:headerReference` / `w:footerReference`。
fn pick(dom: &Dom, doc: &Document, kind: HfKind, variant: HfVariant) -> Option<PartId> {
    let elem = match kind {
        HfKind::Header => LocalName::HeaderReference,
        HfKind::Footer => LocalName::FooterReference,
    };
    let refs: Vec<NodeId> =
        dom.semantic_descendants(dom.root()).filter(|&n| dom.is(n, QName::w(elem))).collect();
    let type_of = |n: NodeId| dom.attr_value(n, QName::w(LocalName::Type)).map(|v| v.into_owned());
    let find = |want: &str| refs.iter().copied().find(|&n| type_of(n).as_deref() == Some(want));
    let node = match variant {
        HfVariant::First => find("first"),
        HfVariant::Even => find("even"),
        // 缺省页：default → 非 schema 的 odd → 没有 `w:type` 的
        HfVariant::Default => find("default")
            .or_else(|| find("odd"))
            .or_else(|| refs.iter().copied().find(|&n| type_of(n).is_none())),
    }?;
    let rid = dom.attr_value(node, QName::new(NsId::R, LocalName::Id))?;
    doc.hf_by_rel.get(rid.as_ref()).copied()
}

/// 一个 part 的 `text` / `hasPageNumber` / `paras` / 水印。
fn part_info(ctx: &Ctx<'_>, hf: &HfPart) -> PartInfo {
    PartInfo {
        text: part_text(ctx.dom, hf),
        has_page_number: hf.has_page_number,
        paras: part_paras(ctx, hf),
        images: part_images(ctx, hf),
        watermark: hf.watermark.clone(),
    }
}

/// `text` 的事件：按字节位置排好序再拼。
enum Ev<'a> {
    /// 一个 `w:t` 的原文（未按 `xml:space` 去空白；实体已解码一次，`XML-06`）。
    Text(std::borrow::Cow<'a, str>),
    /// 页码 / 总页数标记。
    Mark(char),
    /// `</w:tc>`：后面还有文字就补一个空格。
    CellEnd,
}

/// TS `plainText(cleaned)`：见模块头的对照表。
pub(super) fn part_text(dom: &Dom, hf: &HfPart) -> String {
    // 跳过区间：PAGE / NUMPAGES 字段整段（连缓存结果），其他字段的 begin..separate（指令区）
    let mut skip: Vec<(u32, u32)> = Vec::new();
    let mut marks: Vec<(u32, char)> = Vec::new();
    for f in hf.idx.fields.fields() {
        let head = f.form.head();
        let (start, end) = (start_of(dom, head), end_of(dom, f.form.tail()));
        match f.keyword() {
            Keyword::Page => {
                marks.push((start, PAGE_MARK));
                skip.push((start, end));
            }
            Keyword::NumPages => {
                marks.push((start, TOTAL_PAGES_MARK));
                skip.push((start, end));
            }
            _ => {
                // 指令区丢掉，`separate` 之后的结果留着
                let instr_end = match &f.form {
                    FieldForm::Complex { separate: Some(s), .. } => end_of(dom, *s),
                    FieldForm::Complex { end, .. } => end_of(dom, *end),
                    // `w:fldSimple` 的 `w:instr` 是属性，不在 `w:t` 里，没什么可跳的
                    FieldForm::Simple { .. } => start,
                };
                if instr_end > start {
                    skip.push((start, instr_end));
                }
            }
        }
    }
    for n in dom.semantic_descendants(hf.root) {
        if dom.is(n, QName::w(LocalName::PgNum)) {
            marks.push((start_of(dom, n), PAGE_MARK));
        }
    }
    skip.sort_unstable();

    let mut events: Vec<(u32, Ev<'_>)> = marks.into_iter().map(|(p, c)| (p, Ev::Mark(c))).collect();
    for n in dom.semantic_descendants(hf.root) {
        if dom.is(n, QName::w(LocalName::T)) {
            let pos = start_of(dom, n);
            if in_skip(&skip, pos) {
                continue;
            }
            for c in dom.children(n).iter().copied() {
                if let Some(t) = dom.text(c) {
                    events.push((pos, Ev::Text(t)));
                }
            }
        } else if dom.is(n, QName::w(LocalName::Tc)) {
            events.push((end_of(dom, n), Ev::CellEnd));
        }
    }
    events.sort_by_key(|(p, _)| *p);

    let mut out = String::new();
    let mut pending_gap = false;
    for (_, ev) in events {
        match ev {
            Ev::CellEnd => pending_gap = !out.is_empty(),
            Ev::Text(t) => {
                if pending_gap {
                    out.push(' ');
                    pending_gap = false;
                }
                out.push_str(&t);
            }
            Ev::Mark(c) => {
                if pending_gap {
                    out.push(' ');
                    pending_gap = false;
                }
                out.push(c);
            }
        }
    }
    out
}

fn start_of(dom: &Dom, n: NodeId) -> u32 {
    dom.node(n).lex.as_ref().map_or(0, |l| l.range.start)
}

fn end_of(dom: &Dom, n: NodeId) -> u32 {
    dom.node(n).lex.as_ref().map_or(0, |l| l.range.end)
}

fn in_skip(skip: &[(u32, u32)], pos: u32) -> bool {
    skip.iter().any(|&(a, b)| pos >= a && pos < b)
}

// ---- `paras`（TS `hfParagraphs`）---------------------------------------------------------------

/// 一个 part 的 `paras`。
///
/// 与正文块投影的三点不同（都照 TS `hfParagraphs`）：
///
/// 1. **不看分类**：每个 `w:p` 都取 runs。分类成图片 / 保护块的段落（水印、纯图段落）runs 为空，
///    走"框内段落"分支——框里没东西就整段不出，这正是 TS 丢掉水印段落的方式。
/// 2. **字段已被改写**：`text` 那一步把 PAGE / NUMPAGES 换成标记 run、其他字段只留缓存结果，
///    `paras` 看到的是同样的结果（[`hf_runs`]）。
/// 3. **浮动表格延后**：`w:tblpPr` 的表格锚在它后面那个段落上，Word 先画那个段落。
fn part_paras(ctx: &Ctx<'_>, hf: &HfPart) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut deferred: Vec<Value> = Vec::new();
    for b in &hf.blocks {
        match b {
            Block::Table(t) => {
                let rows = table_rows(ctx, t);
                if t.props.position.is_some() {
                    deferred.extend(rows);
                } else {
                    out.extend(rows);
                }
            }
            _ => {
                let node = b.node();
                if !ctx.dom.is(node, QName::w(LocalName::P)) {
                    continue;
                }
                match b {
                    Block::Text(tb) => push_para(ctx, b, tb, &mut out),
                    // 图片 / 保护段落：TS 的 `extractRuns` 给不出 run，直接走框内段落
                    _ => out.extend(textbox_paras(ctx, b)),
                }
                out.append(&mut deferred);
            }
        }
    }
    out.append(&mut deferred);
    // 整个 part 全是空段落 → 一个都不出（TS：`headerParas` 因此可能是 `[]`）
    if out.iter().all(|p| {
        p.get("runs").and_then(Value::as_array).is_none_or(|r| r.is_empty())
            && p.get("cells").is_none()
    }) {
        return Vec::new();
    }
    out
}

/// 一个文本段落 → 一条 `HfParagraph`；runs 为空且段落里有 `w:r` / `w:pict` 时改出框内段落。
fn push_para(ctx: &Ctx<'_>, block: &Block, tb: &TextBlock, out: &mut Vec<Value>) {
    let runs = hf_runs(ctx, tb, false);
    if runs.is_empty() && has_run_or_pict(ctx.dom, tb.node) {
        // 政府公文的页码常放在 VML 文本框里；把框里的段落提出来，别把内容丢了
        out.extend(textbox_paras(ctx, block));
        return;
    }
    let mut p = Map::new();
    // 样式层（Word 内建的 Header / Footer 样式带居中 / 右对齐制表位）；直接格式后写，覆盖它
    let (style_align, style_stops) = style_layer(ctx, tb.style_id.as_deref());
    set_some!(&mut p, "align" => style_align);
    spread_format(&mut p, super::blocks::para_format_json(ctx, tb));
    set_some!(&mut p,
        // 制表位：样式与直接的按位置合并（TS `mergeTabStops`）
        "tabStops" => merge_tab_stops(style_stops, p.remove("tabStops")),
        "ptabAligns" => ptab_aligns(ctx.dom, tb.node),
        "frameXAlign" => frame_x_align(tb),
    );
    set(&mut p, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
    out.push(Value::Object(p));
}

/// 段落里有 `w:r` 或 `w:pict`（TS 的"runs 为空但有内容"判定）。
fn has_run_or_pict(dom: &Dom, p: NodeId) -> bool {
    dom.semantic_children(p)
        .any(|n| dom.is(n, QName::w(LocalName::R)) || dom.is(n, QName::w(LocalName::Pict)))
}

/// TS `textboxParagraphs`：段落里所有 `w:txbxContent` 的段落，runs 非空才出。
///
/// 走模型：框的内容已经由 `Document::rebuild` 建成 `ShapeDisplay.content` / `VmlDisplay.content`
/// （M4 的独立内容流），这里只投影。`wp:anchor` 的绘图与 `position:absolute` 的 VML →
/// `boxAnchored`（框画在锚点上，不占页眉的行高）。
fn textbox_paras(ctx: &Ctx<'_>, block: &Block) -> Vec<Value> {
    let mut out = Vec::new();
    for (blocks, anchored) in box_contents(block) {
        for b in blocks {
            let Block::Text(tb) = b else { continue };
            let runs = hf_runs(ctx, tb, false);
            if runs.is_empty() {
                continue;
            }
            let mut p = Map::new();
            spread_format(&mut p, super::blocks::para_format_json(ctx, tb));
            set(&mut p, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
            set_if!(&mut p, "boxAnchored" => anchored);
            out.push(Value::Object(p));
        }
    }
    out
}

/// 一个块里所有文本框的内容流与"是否浮动"。
fn box_contents(block: &Block) -> Vec<(&[Block], bool)> {
    let mut out = Vec::new();
    let mut displays: Vec<&Display> = Vec::new();
    match block {
        Block::Text(tb) => {
            for i in &tb.inlines {
                let Inline::Run(r) = i else { continue };
                displays.extend(r.segments.iter().filter_map(|seg| seg.display.as_ref()));
            }
        }
        Block::Image(ib) => displays.extend(ib.display.as_ref()),
        Block::Protected(pb) => displays.extend(pb.display.as_ref()),
        Block::Table(_) => {}
    }
    for d in displays {
        match d {
            Display::Drawing(dr) => {
                let anchored = dr.anchor.is_some();
                for sh in &dr.shapes {
                    if !sh.content.is_empty() {
                        out.push((sh.content.as_slice(), anchored));
                    }
                }
            }
            Display::Vml(v) => {
                for sh in &v.shapes {
                    if sh.content.is_empty() {
                        continue;
                    }
                    let abs = sh
                        .style_get("position")
                        .is_some_and(|p| p.trim().eq_ignore_ascii_case("absolute"));
                    out.push((sh.content.as_slice(), abs));
                }
            }
        }
    }
    out
}

// ---- 段落层的小规则 ---------------------------------------------------------------------------

/// 样式层：Word 内建的 `Header` / `Footer` 样式带居中 / 右对齐制表位，有时还带 `w:jc`。
/// 只取 `align`（非 justify）与 `tabStops`（TS 用的是 `styles.*.display`，即样式链解析后的值）。
fn style_layer(ctx: &Ctx<'_>, style_id: Option<&str>) -> (Option<String>, Option<Value>) {
    let Some(id) = style_id else { return (None, None) };
    let Some(props) = ctx.resolver.style_para_props(id) else { return (None, None) };
    let mut m = Map::new();
    super::decl::style_para_display(&props, &mut m);
    let align =
        m.get("align").and_then(Value::as_str).filter(|a| *a != "justify").map(str::to_string);
    (align, m.remove("tabStops"))
}

/// TS `mergeTabStops`：按位置合并，直接格式胜出；`w:val="clear"` 的删掉；同位置只留第一个。
fn merge_tab_stops(style: Option<Value>, direct: Option<Value>) -> Option<Value> {
    let pos_of = |v: &Value| v.get("pos").and_then(Value::as_i64).unwrap_or(0);
    let not_clear = |v: &&Value| v.get("val").and_then(Value::as_str) != Some("clear");
    let arr = |v: Option<Value>| match v {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    let (st, di) = (arr(style), arr(direct));
    if st.is_empty() || di.is_empty() {
        let only: Vec<Value> = st.iter().chain(di.iter()).filter(not_clear).cloned().collect();
        return (!only.is_empty()).then_some(Value::Array(only));
    }
    let mut merged: Vec<Value> = st
        .iter()
        .filter(|s| !di.iter().any(|d| pos_of(d) == pos_of(s)))
        .chain(di.iter())
        .filter(not_clear)
        .cloned()
        .collect();
    merged.sort_by_key(pos_of);
    merged.dedup_by_key(|v| pos_of(v));
    (!merged.is_empty()).then_some(Value::Array(merged))
}

/// TS 的 `ptabAligns`：按**整体制表位顺序**索引，`w:tab` 占一个 `null` 位。
/// 一个 `w:ptab` 都没有就不出这个键（绝对位置制表位自带对齐，不看制表位表）。
fn ptab_aligns(dom: &Dom, para: NodeId) -> Option<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut saw = false;
    let mut stack: Vec<NodeId> = dom.semantic_children(para).collect();
    stack.reverse();
    while let Some(n) = stack.pop() {
        if dom.is(n, QName::w(LocalName::PPr)) {
            continue;
        }
        if dom.is(n, QName::w(LocalName::Tab)) {
            out.push(Value::Null);
        } else if dom.is(n, QName::w(LocalName::Ptab)) {
            saw = true;
            let a = dom.attr_value(n, QName::w(LocalName::Alignment));
            out.push(Value::String(
                match a.as_deref() {
                    Some("center") => "center",
                    Some("right") => "right",
                    _ => "left",
                }
                .to_string(),
            ));
        } else {
            for c in dom.semantic_children(n).collect::<Vec<_>>().into_iter().rev() {
                stack.push(c);
            }
        }
    }
    saw.then_some(Value::Array(out))
}

/// TS 的 `frameXAlign`：`w:framePr/@w:xAlign`（不是首字下沉时才算）。
/// `right` / `outside` → right，`center` → center，`left` / `inside` → left。
fn frame_x_align(tb: &TextBlock) -> Option<&'static str> {
    let f = tb.props.frame.as_ref()?;
    if f.drop_cap.is_some() {
        return None;
    }
    match f.x_align.as_ref()?.value()? {
        crate::semantic::props::XAlign::Right | crate::semantic::props::XAlign::Outside => {
            Some("right")
        }
        crate::semantic::props::XAlign::Center => Some("center"),
        crate::semantic::props::XAlign::Left | crate::semantic::props::XAlign::Inside => {
            Some("left")
        }
    }
}

// ---- 表格（TS `hfTableRowParagraphs` / `hfCellContent`）-----------------------------------------

/// 顶层表格 → 一行一条 `HfParagraph`（`cells` 当列排）。
fn table_rows(ctx: &Ctx<'_>, t: &TableBlock) -> Vec<Value> {
    let grid: Vec<i64> = t
        .grid
        .iter()
        .map(|g| g.w.as_ref().and_then(|v| v.value().copied()).unwrap_or(0).max(0).into())
        .collect();
    let mut out = Vec::new();
    for row in &t.rows {
        let mut widths: Vec<i64> = row
            .cells
            .iter()
            .map(|c| {
                c.props
                    .width
                    .as_ref()
                    .filter(|w| w.percent().is_none())
                    .and_then(|w| w.twips())
                    .map(i64::from)
                    .filter(|&w| w > 0)
                    .unwrap_or(0)
            })
            .collect();
        // 有格子没有 `tcW` 时按 `tblGrid` 与 `gridSpan` 切
        if widths.iter().any(|&w| w <= 0) && grid.iter().any(|&w| w > 0) {
            let mut col = 0usize;
            for (i, c) in row.cells.iter().enumerate() {
                let span =
                    c.props.grid_span.as_ref().and_then(|v| v.value().copied()).unwrap_or(1).max(1)
                        as usize;
                widths[i] = grid.iter().skip(col).take(span).sum();
                col += span;
            }
        }
        let total: i64 = widths.iter().sum();
        let cells: Vec<Value> = row
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut m = cell_content(ctx, c);
                if total > 0 && widths[i] > 0 {
                    #[allow(clippy::cast_precision_loss)]
                    set(&mut m, "widthPct", widths[i] as f64 / total as f64 * 100.0);
                }
                Value::Object(m)
            })
            .collect();
        // 只有底纹没有文字的行也要画（横幅色条）
        let keeps = cells.iter().any(|c| {
            c.get("fill").is_some()
                || c.get("paras").and_then(Value::as_array).is_some_and(|ps| {
                    ps.iter().any(|rs| {
                        rs.as_array().is_some_and(|rs| {
                            rs.iter().any(|r| {
                                r.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty())
                                    || r.get("image").is_some()
                            })
                        })
                    })
                })
        });
        if keeps {
            let mut p = Map::new();
            p.insert("runs".into(), Value::Array(Vec::new()));
            p.insert("cells".into(), Value::Array(cells));
            out.push(Value::Object(p));
        }
    }
    out
}

/// 一个单元格：段落 runs、第一个有 `w:jc` 的段落的对齐、底纹；嵌套表格平铺进来。
fn cell_content(ctx: &Ctx<'_>, cell: &Cell) -> Map<String, Value> {
    let mut paras: Vec<Value> = Vec::new();
    let mut align: Option<String> = None;
    let mut fill = cell.props.shading.as_ref().and_then(super::decl::shd_display_fill);
    let mut saw_nested = false;
    for b in &cell.blocks {
        match b {
            Block::Table(inner) => {
                saw_nested = true;
                for row in &inner.rows {
                    for c in &row.cells {
                        let mut m = cell_content(ctx, c);
                        if let Some(Value::Array(ps)) = m.remove("paras") {
                            paras.extend(ps);
                        }
                        if align.is_none() {
                            align = m.get("align").and_then(Value::as_str).map(str::to_string);
                        }
                        if fill.is_none() {
                            fill = m.get("fill").and_then(Value::as_str).map(str::to_string);
                        }
                    }
                }
            }
            Block::Text(tb) => {
                // 格里带图：`withImages` 开着，但锚定 / 绝对定位的图归 part 级图片列表
                let runs = hf_runs(ctx, tb, true);
                paras.push(Value::Array(runs.into_iter().map(Value::Object).collect()));
                if align.is_none() {
                    align = super::blocks::para_format_json(ctx, tb)
                        .as_ref()
                        .and_then(|f| f.get("align"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            // 纯图段落（`Block::Image`）与保护块：TS 的 `extractRuns(withImages)` 给出一个
            // `text: ""` 的图片 run，显示模型挂在块上（不是段上），所以走 `display_image_run`
            Block::Image(ib) => paras.push(image_para(ctx, ib.display.as_ref())),
            Block::Protected(pb) => paras.push(image_para(ctx, pb.display.as_ref())),
        }
    }
    // 嵌套表格后面那个必有的空段落是排版噪声
    if saw_nested {
        while paras.last().is_some_and(|p| p.as_array().is_some_and(Vec::is_empty)) {
            paras.pop();
        }
    }
    let mut m = Map::new();
    m.insert("paras".into(), Value::Array(paras));
    set_some!(&mut m, "align" => align, "fill" => fill);
    m
}

/// 把 `format` 对象摊进段落对象（`HfParagraph extends ParaFormat`：TS 里格式键是平铺的）。
fn spread_format(p: &mut Map<String, Value>, format: Option<Value>) {
    if let Some(Value::Object(f)) = format {
        for (k, v) in f {
            p.insert(k, v);
        }
    }
}

// ---- run（TS 在 `hfContentFromXml` 里改写完字段之后才 `extractRuns`）---------------------------

/// 页眉页脚的 run 投影。
///
/// 与正文（`COMPAT-07` 的 `runs_json`）差在字段：TS 先在 XML 上把字段整段改写掉，`extractRuns`
/// 看到的已经是普通 run，所以这里
///
/// - `PAGE` / `NUMPAGES` → 一个文字是标记的 run，格式取整段**第一个 `w:rPr`**（TS 的正则就是
///   `/<w:rPr>[\s\S]*?<\/w:rPr>/`，取到哪个就用哪个）；
/// - 其他字段 → 只留 `separate` 之后带 `w:t` 的结果 run（Word 打开时会重算它们）；
/// - 旧式 `w:pgNum` → 该 run 的文字换成标记（run 自己的 `w:rPr` 留着）。
///
/// `with_images`：非表格段落 TS 用 `extractRuns(p, ctx)`（不带图），单元格用
/// `extractRuns(p, ctx, [], [], true)`（带图，但锚定的图剥掉 `image` 字段）——正是
/// [`super::blocks::stray_runs_json`] 的两种模式。
fn hf_runs(ctx: &Ctx<'_>, tb: &TextBlock, with_images: bool) -> Vec<Map<String, Value>> {
    let para = super::blocks::para_disp(ctx, tb);
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for inline in &tb.inlines {
        match inline {
            Inline::Field { id, result } => {
                let Some(f) = ctx.fields.get(*id) else { continue };
                match f.keyword() {
                    Keyword::Page => out.extend(mark_run(ctx, f, para, PAGE_MARK)),
                    Keyword::NumPages => out.extend(mark_run(ctx, f, para, TOTAL_PAGES_MARK)),
                    // 其他字段：缓存结果里的 run 照常投影
                    _ => out.extend(result_runs(ctx, result, para, with_images)),
                }
            }
            // 旧式 `w:pgNum`：TS 把这个元素本身换成 `<w:t>PAGE_MARK</w:t>`，run 与它的 `w:rPr`
            // 都留着。坐标流里它是 `Other` 段（一个 `U+FFFC`），按段重拼文字就能放对位置。
            Inline::Run(run) if pg_num_seg(ctx.dom, run) => {
                let mut o = Map::new();
                set(&mut o, "text", pg_num_text(ctx.dom, run));
                super::blocks::run_format_json(ctx, run.node, &run.props, para, false, &mut o);
                out.push(o);
            }
            Inline::Run(_) | Inline::Atom(_) => {
                out.extend(inline_runs(ctx, std::slice::from_ref(inline), para, with_images));
            }
        }
    }
    super::blocks::merge_runs(out)
}

/// 一个字段折成的标记 run（`PAGE` / `NUMPAGES`）。
fn mark_run(
    ctx: &Ctx<'_>,
    f: &crate::span::field::FieldSpan,
    para: super::blocks::StyleDisp,
    mark: char,
) -> Option<Map<String, Value>> {
    let mut o = Map::new();
    set(&mut o, "text", mark.to_string());
    // 整段第一个带 `w:rPr` 的 run（TS 的正则在 begin..end 的原文里取第一个）
    if let Some(node) = first_rpr_run(ctx.dom, f) {
        let props =
            crate::semantic::props::read_run_props(ctx.dom, rpr_of(ctx.dom, node), &mut Vec::new());
        super::blocks::run_format_json(ctx, node, &props, para, false, &mut o);
    }
    Some(o)
}

/// 字段区间里第一个有 `w:rPr` 的 `w:r`。
fn first_rpr_run(dom: &Dom, f: &crate::span::field::FieldSpan) -> Option<NodeId> {
    let (lo, hi) = (start_of(dom, f.form.head()), end_of(dom, f.form.tail()));
    dom.semantic_descendants(dom.root())
        .filter(|&n| dom.is(n, QName::w(LocalName::R)))
        .filter(|&n| start_of(dom, n) >= lo && end_of(dom, n) <= hi)
        .find(|&n| rpr_of(dom, n).is_some())
}

fn rpr_of(dom: &Dom, run: NodeId) -> Option<NodeId> {
    dom.semantic_children(run).find(|&n| dom.is(n, QName::w(LocalName::RPr)))
}

/// 字段缓存结果里的 run（TS 只留含 `w:t` 的完整 run）。
fn result_runs(
    ctx: &Ctx<'_>,
    result: &[Inline],
    para: super::blocks::StyleDisp,
    with_images: bool,
) -> Vec<Map<String, Value>> {
    inline_runs(ctx, result, para, with_images)
}

/// 一串 inline → run JSON：空文字且没有图的 run 丢掉（TS `extractRuns` 同样丢）。
fn inline_runs(
    ctx: &Ctx<'_>,
    inlines: &[Inline],
    para: super::blocks::StyleDisp,
    with_images: bool,
) -> Vec<Map<String, Value>> {
    super::blocks::inline_runs_json(ctx, inlines, para, with_images)
}

/// run 里有 `w:pgNum` 段。
fn pg_num_seg(dom: &Dom, run: &crate::model::Run) -> bool {
    run.segments.iter().any(|s| is_pg_num(dom, s))
}

fn is_pg_num(dom: &Dom, seg: &crate::model::Segment) -> bool {
    matches!(seg.kind, crate::model::SegmentKind::Other(_))
        && dom.is(seg.node, QName::w(LocalName::PgNum))
}

/// 按段重拼 run 的文字，`w:pgNum` 段换成页码标记。
fn pg_num_text(dom: &Dom, run: &crate::model::Run) -> String {
    let mut out = String::new();
    for seg in &run.segments {
        if is_pg_num(dom, seg) {
            out.push(PAGE_MARK);
        } else {
            out.push_str(&run.text[seg.text.start as usize..seg.text.end as usize]);
        }
    }
    out
}

/// 纯图段落在单元格里的投影：一个 `text: ""` 带 `image` 的 run（没有可解析的图就是空段落）。
fn image_para(ctx: &Ctx<'_>, display: Option<&Display>) -> Value {
    let run = display.and_then(|d| super::image::display_image_run(ctx, d)).map(|img| {
        let mut r = Map::new();
        set(&mut r, "text", "");
        set(&mut r, "image", Value::Object(img));
        Value::Object(r)
    });
    Value::Array(run.into_iter().collect())
}

// ---- `images`（TS `hfImages`）------------------------------------------------------------------

/// 一个 part 的 `images`（part 级的图片列表，显示用；文字编辑不碰它们的字节）。
///
/// 按文档序扫这个 part 的 `w:drawing` 与 `w:pict`。两条跳过规则照 TS：
///
/// - **随文**图片落在顶层 `w:tbl` 区间里 → 跳过（它已经在单元格 run 上了，`hfCellContent`）；
///   浮动的照样进这张表（它要按页面定位）。
/// - `w:pict` 里有 `v:textpath` → 跳过（那是文字水印，走 `watermarkText`）。
///
/// 尺寸：DrawingML 用 `wp:extent`（EMU），VML 用 `v:shape/@style` 的 pt。位置：`wp:anchor` 的
/// `wp:align` / `wp:posOffset`（EMU → px，`relativeFrom` 决定基准），VML 用 `mso-position-*`。
fn part_images(ctx: &Ctx<'_>, hf: &HfPart) -> Vec<Value> {
    let dom = ctx.dom;
    let tables = top_level_table_ranges(dom, hf.root);
    let in_table = |n: NodeId| {
        let at = start_of(dom, n);
        tables.iter().any(|&(a, b)| at > a && at < b)
    };
    let mut out = Vec::new();
    for n in dom.semantic_descendants(hf.root) {
        if dom.is(n, QName::w(LocalName::Drawing)) {
            let d = crate::model::drawing::drawing_display(dom, n);
            let anchored = d.anchor.is_some();
            if !anchored && in_table(n) {
                continue;
            }
            if let Some(img) = drawing_image(ctx, &d, n) {
                out.push(img);
            }
        } else if dom.is(n, QName::w(LocalName::Pict)) || dom.is(n, QName::w(LocalName::Object)) {
            let v = crate::model::vml::vml_display(dom, n);
            // 文字水印不算图片
            if v.shapes.iter().any(|s| s.textpath.is_some()) {
                continue;
            }
            let absolute = v
                .shapes
                .iter()
                .any(|s| s.style_get("position").is_some_and(|p| p.trim() == "absolute"));
            if !absolute && in_table(n) {
                continue;
            }
            if let Some(img) = vml_image(ctx, &v, n, absolute) {
                out.push(img);
            }
        }
    }
    out
}

/// 顶层 `w:tbl` 的字节区间（嵌套的算在外层里）。
fn top_level_table_ranges(dom: &Dom, root: NodeId) -> Vec<(u32, u32)> {
    dom.semantic_children(root)
        .filter(|&n| dom.is(n, QName::w(LocalName::Tbl)))
        .map(|n| (start_of(dom, n), end_of(dom, n)))
        .collect()
}

/// `w:drawing` → 一条 `HfImage`。解析不出媒体就不出这条（TS 同样跳过）。
fn drawing_image(
    ctx: &Ctx<'_>,
    d: &crate::model::drawing::DrawingDisplay,
    node: NodeId,
) -> Option<Value> {
    // 一个 drawing 里可能有几张图（mac Word 的 PDF Choice + PNG Fallback）：取第一张解析得出的
    let found = d
        .pictures
        .iter()
        .find_map(|p| ctx.media.pick(p.embed.as_deref(), p.link.as_deref()).map(|m| (p, m)));
    // 没有位图时：无字的实心矢量装饰合成一张 SVG（TS `hfShapeDrawingSvg`）
    let (pic, url) = match found {
        Some((p, m)) => (Some(p), m.url.clone()),
        None => (None, shape_drawing_svg(ctx, d)?),
    };
    let mut o = Map::new();
    set(&mut o, "dataUrl", url);
    let ext = d.extent.filter(|e| e.cx > 0 || e.cy > 0);
    set_some!(&mut o,
        "widthPx" => ext.filter(|e| e.cx > 0).map(|e| px(e.cx)),
        "heightPx" => ext.filter(|e| e.cy > 0).map(|e| px(e.cy)),
        "crop" => pic.and_then(|p| p.crop).filter(|c| !c.is_zero()).map(|c| Value::Object(crop_json(c))),
    );
    match d.anchor.as_ref() {
        Some(a) => {
            set_if!(&mut o, "floating" => true, "behind" => a.behind_doc);
            set_some!(&mut o, "wrap" => wrap_name(&a.wrap));
            anchor_pos(a, &mut o);
        }
        // 随文图片跟着所在段落的对齐
        None => set_some!(&mut o, "align" => para_align_of(ctx.dom, node)),
    }
    Some(Value::Object(o))
}

/// `w:pict` / `w:object` → 一条 `HfImage`。
fn vml_image(
    ctx: &Ctx<'_>,
    v: &crate::model::VmlDisplay,
    node: NodeId,
    absolute: bool,
) -> Option<Value> {
    let shape = v.shapes.iter().find(|s| s.imagedata.is_some())?;
    let media = ctx.media.get(shape.imagedata.as_deref()?)?;
    let mut o = Map::new();
    set(&mut o, "dataUrl", media.url.clone());
    // VML 的尺寸写在 `style` 里，单位 pt
    set_some!(&mut o,
        "widthPx" => shape.style_len("width").and_then(style_px),
        "heightPx" => shape.style_len("height").and_then(style_px),
    );
    if absolute {
        set_if!(&mut o, "floating" => true);
        // 负 `z-index` = 画在文字下面（图片水印）
        let z = shape.style_get("z-index").map(str::trim).unwrap_or("");
        set_if!(&mut o, "behind" => z.starts_with('-'));
        set_some!(&mut o,
            "posH" => shape.style_get("mso-position-horizontal").and_then(h_align),
            "posV" => shape.style_get("mso-position-vertical").and_then(v_align),
        );
    } else {
        set_some!(&mut o, "align" => para_align_of(ctx.dom, node));
    }
    // `gain` / `blacklevel` 是 Word 的"冲蚀"预设（图片水印）
    set_if!(&mut o, "washout" => washout(ctx.dom, shape.node));
    Some(Value::Object(o))
}

/// `wp:positionH` / `wp:positionV` → `posH` / `posV` 或 `posXPx` / `posYPx` + 基准。
fn anchor_pos(a: &crate::model::drawing::AnchorGeom, o: &mut Map<String, Value>) {
    for (axis_h, pos) in [(true, &a.h), (false, &a.v)] {
        if let Some(align) = pos.align.as_deref() {
            if axis_h {
                set_some!(o, "posH" => h_align(align));
            } else {
                set_some!(o, "posV" => v_align(align));
            }
        } else if let Some(off) = pos.offset_emu {
            let rel = pos.relative_from.as_deref().unwrap_or("");
            if axis_h {
                set(o, "posXPx", px(off));
                set(o, "posHRel", if rel == "page" { "page" } else { "margin" });
            } else {
                set(o, "posYPx", px(off));
                set(
                    o,
                    "posVRel",
                    match rel {
                        "page" => "page",
                        // 竖向的 `paragraph` / `line` 保留原义：正文下推要从页眉带顶开始量
                        "paragraph" | "line" => "paragraph",
                        _ => "margin",
                    },
                );
            }
        }
    }
}

fn wrap_name(w: &crate::model::drawing::Wrap) -> Option<&'static str> {
    use crate::model::drawing::Wrap;
    Some(match w {
        Wrap::None => "none",
        Wrap::Square { .. } => "square",
        Wrap::Tight { .. } => "tight",
        Wrap::Through { .. } => "through",
        Wrap::TopAndBottom => "topBottom",
        Wrap::Unspecified => return None,
    })
}

fn h_align(a: &str) -> Option<&'static str> {
    match a.trim() {
        "left" => Some("left"),
        "center" => Some("center"),
        "right" => Some("right"),
        _ => None,
    }
}

fn v_align(a: &str) -> Option<&'static str> {
    match a.trim() {
        "top" => Some("top"),
        "center" => Some("center"),
        "bottom" => Some("bottom"),
        _ => None,
    }
}

/// EMU → px，四舍五入（`MOD-11` 的单位换算集中在 `model/units.rs`）。
fn px(emu: i64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let v = crate::model::units::emu_to_px(emu as f64).round() as i64;
    v
}

/// VML `style` 里的长度 → px（`width:40pt` → 53）。无单位或非绝对单位 → `None`。
fn style_px(l: crate::model::units::Length) -> Option<i64> {
    #[allow(clippy::cast_possible_truncation)]
    let v = crate::model::units::emu_to_px(l.to_emu()?).round() as i64;
    (v > 0).then_some(v)
}

/// `a:srcRect` → 四边的小数（TS `rectFrac`：千分之一百分比 → 0..1）。
fn crop_json(c: crate::model::drawing::RectFrac) -> Map<String, Value> {
    let mut o = Map::new();
    for (k, v) in [("l", c.l), ("t", c.t), ("r", c.r), ("b", c.b)] {
        #[allow(clippy::cast_precision_loss)]
        set(&mut o, k, v as f64 / 100_000.0);
    }
    o
}

/// `v:imagedata` 上有 `gain` / `blacklevel`（Word 的冲蚀预设）。
fn washout(dom: &Dom, shape: NodeId) -> bool {
    dom.semantic_descendants(shape)
        .filter(|&n| dom.is(n, QName::new(NsId::V, LocalName::Imagedata)))
        .any(|n| {
            [LocalName::Gain, LocalName::Blacklevel]
                .into_iter()
                .any(|a| dom.attr_value(n, QName::new(NsId::None, a)).is_some())
        })
}

/// 图片所在段落的 `w:jc`（随文图片跟着段落对齐）。
fn para_align_of(dom: &Dom, node: NodeId) -> Option<&'static str> {
    let para = std::iter::once(node)
        .chain(dom.ancestors(node))
        .find(|&n| dom.is(n, QName::w(LocalName::P)))?;
    super::image::jc_align(dom, para)
}

// ---- 无字矢量装饰 → 一张 SVG（TS `hfShapeDrawingSvg`）------------------------------------------

/// 一个页眉里的绘图**没有位图**、只有几个实心 `custGeom` 形状（角花之类的装饰）时，TS 把整组
/// 合成**一张 SVG** 当作图片。这是显示层的合成，只出现在适配器里（`COMPAT-01`）。
///
/// 任何一处表达不出来就整张不给（返回 `None`）：有文字、有旋转 / 翻转、缺尺寸、没有实心填充、
/// 几何用了公式或圆弧（`model::custgeom` 给不出路径）。宁可不画，也不能画一张缺了形状的图。
fn shape_drawing_svg(ctx: &Ctx<'_>, d: &crate::model::drawing::DrawingDisplay) -> Option<String> {
    let ext = d.extent.filter(|e| e.cx > 0 && e.cy > 0)?;
    if d.shapes.is_empty() || d.shapes.iter().any(|s| !s.content.is_empty()) {
        return None;
    }
    // 组的子坐标系 → 绘图坐标系的仿射（`a:chOff` / `a:chExt` → `a:off` / `a:ext`）
    let (mut sx, mut sy, mut tx, mut ty) = (1.0f64, 1.0f64, 0.0f64, 0.0f64);
    if let Some(g) = d.shapes.iter().find(|s| s.is_group) {
        let (ge, gce) = (g.ext, g.ch_ext);
        if let (Some(e), Some(ce)) = (ge, gce) {
            #[allow(clippy::cast_precision_loss)]
            if e.cx > 0 && ce.cx > 0 {
                sx = e.cx as f64 / ce.cx as f64;
            }
            #[allow(clippy::cast_precision_loss)]
            if e.cy > 0 && ce.cy > 0 {
                sy = e.cy as f64 / ce.cy as f64;
            }
        }
        let off = g.off.unwrap_or((0, 0));
        let ch = g.ch_off.unwrap_or((0, 0));
        #[allow(clippy::cast_precision_loss)]
        {
            tx = off.0 as f64 - ch.0 as f64 * sx;
            ty = off.1 as f64 - ch.1 as f64 * sy;
        }
    }
    let mut paths = String::new();
    for s in d.shapes.iter().filter(|s| !s.is_group) {
        if s.rot_60k.is_some_and(|r| r != 0) || s.flip_h || s.flip_v {
            return None;
        }
        let e = s.ext.filter(|e| e.cx > 0 && e.cy > 0)?;
        let fill = s
            .fill
            .as_ref()
            .filter(|f| f.kind == crate::model::drawing::FillKind::Solid)
            .and_then(|f| super::box_json::color_hex(ctx, f.node))?;
        let geom = s.geom.as_ref()?;
        let pd = super::box_json::path_data(geom, s.ext)?;
        // 只画填充路径（`path` + `fillPath`），描边路径不参与
        let d_norm: String = ["path", "fillPath"]
            .into_iter()
            .filter_map(|k| pd.get(k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        if d_norm.is_empty() {
            return None;
        }
        let off = s.off.unwrap_or((0, 0));
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (emu_px2(off.0 as f64 * sx + tx), emu_px2(off.1 as f64 * sy + ty));
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (emu_px2(e.cx as f64 * sx), emu_px2(e.cy as f64 * sy));
        let placed = place_path(&d_norm, x, y, w, h);
        paths.push_str(&format!(r##"<path d="{placed}" fill="#{fill}"/>"##));
    }
    if paths.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}">{paths}</svg>"#,
        num2(emu_px2(ext.cx as f64)),
        num2(emu_px2(ext.cy as f64))
    );
    Some(format!("data:image/svg+xml,{}", encode_uri_component(&svg)))
}

/// 0..1 的归一化路径 → 形状矩形里的 px 路径。数字按 x / y 交替换算，遇到命令字母重新计数。
fn place_path(d: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    let mut axis = 0usize;
    d.split(' ')
        .map(|tok| match tok.parse::<f64>() {
            Ok(n) => {
                let v = if axis.is_multiple_of(2) { x + n * w } else { y + n * h };
                axis += 1;
                num2(round2(v))
            }
            Err(_) => {
                axis = 0;
                tok.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// EMU → px，保留两位小数（TS `px()`）。
fn emu_px2(emu: f64) -> f64 {
    round2(crate::model::units::emu_to_px(emu))
}

fn round2(v: f64) -> f64 {
    let r = (v * 100.0).round() / 100.0;
    if r == 0.0 { 0.0 } else { r }
}

/// 数字的 JS `String()` 写法（整数不带小数点，`-0` 归 `0`）。
fn num2(v: f64) -> String {
    format!("{v}")
}

/// `encodeURIComponent`：除 `A-Za-z0-9-_.!~*'()` 外一律按 UTF-8 百分号编码。
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
