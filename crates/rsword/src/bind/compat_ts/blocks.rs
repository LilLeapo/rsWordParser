//! `blocks[]` / `extras.elements[]`（`COMPAT-03/04/07`）：body 顶层元素序列（sdt 拆分）、块类型复现、
//! `ParaFormat`、Run 映射与合并。每段规则注明对应的 TS 函数（`docs/01` §6）。
//!
//! M1 范围：文本块完整；表格 / 图片 / 字段 / 公式等保护块只给 `type/label/previewText` 骨架。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use serde_json::{Map, Value, json};

use super::decl::{
    NumberingOut, auto_space, i32_of, jc_align, list_kind, parse_int, strip_hash, tab_stops,
    u32_of, val_text,
};
use super::utf16::Utf16Index;
use crate::model::{
    AtomKind, Block, BreakKind, Document, Inline, LinkTarget, ProtectedKind, Revision,
    RevisionMeta, Run, SegmentKind, StyleType, TextBlock, TextKind,
};
use crate::package::{RelTarget, Rels};
use crate::resolve::{Resolver, rgb_hex};
use crate::semantic::props::{ParaProps, RunProps, UnderlineKind, Val};
use crate::span::field::{FieldId, FieldSpan, FormData, InstrToken, Keyword, read_form_data};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 样式的"显示"信息（TS `StyleInfo.display` 里 run 映射用到的三项）。
#[derive(Debug, Clone, Copy, Default)]
struct StyleDisp {
    rtl: Option<bool>,
    vanish: Option<bool>,
    auto_space: Option<bool>,
}

pub(super) struct Ctx<'a> {
    pub dom: &'a Dom,
    pub doc: &'a Document,
    pub resolver: &'a Resolver<'a>,
    pub idx: &'a Utf16Index,
    pub rels: &'a Rels,
    pub numbering: &'a NumberingOut,
    disp_cache: RefCell<HashMap<(String, StyleType), StyleDisp>>,
}

fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

impl<'a> Ctx<'a> {
    pub fn new(
        dom: &'a Dom,
        doc: &'a Document,
        resolver: &'a Resolver<'a>,
        idx: &'a Utf16Index,
        rels: &'a Rels,
        numbering: &'a NumberingOut,
    ) -> Ctx<'a> {
        Ctx { dom, doc, resolver, idx, rels, numbering, disp_cache: RefCell::new(HashMap::new()) }
    }

    fn slice(&self, r: &Range<u32>) -> &'a str {
        self.dom.lex_str(r)
    }

    fn u16(&self, byte: u32) -> u32 {
        self.idx.at(self.dom.src(), byte)
    }

    fn lex_range(&self, node: NodeId) -> Range<u32> {
        self.dom.node(node).lex.as_ref().map(|l| l.range.clone()).unwrap_or(0..0)
    }

    fn attr(&self, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
        self.dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
    }

    fn style_disp(&self, id: &str, kind: StyleType) -> StyleDisp {
        if let Some(d) = self.disp_cache.borrow().get(&(id.to_string(), kind)) {
            return *d;
        }
        let d = match self.resolver.style_run_props(id, kind) {
            None => StyleDisp::default(),
            Some(rp) => {
                let vanish = if rp.spec_vanish == Some(true) { None } else { rp.vanish };
                let auto_space = if kind == StyleType::Paragraph {
                    self.resolver.style_para_props(id).and_then(|p| auto_space(&p))
                } else {
                    None
                };
                StyleDisp { rtl: rp.rtl, vanish, auto_space }
            }
        };
        self.disp_cache.borrow_mut().insert((id.to_string(), kind), d);
        d
    }

    /// TS `plainText`：`w:t` 文本拼接，`</w:tc>` 边界补一个空格。
    fn plain_text(&self, node: NodeId) -> String {
        let dom = self.dom;
        let mut out = String::new();
        let mut pending_gap = false;
        // 前序遍历，遇到 w:tc 结束（其后第一个文本）补空格：用"上一个 tc 已结束"近似
        let mut stack: Vec<(NodeId, bool)> = vec![(node, false)];
        while let Some((n, is_tc_end)) = stack.pop() {
            if is_tc_end {
                pending_gap = !out.is_empty();
                continue;
            }
            let Some(name) = dom.name(n) else { continue };
            if name == w(LocalName::T) {
                if pending_gap {
                    out.push(' ');
                    pending_gap = false;
                }
                for c in dom.semantic_children(n) {
                    if let Some(t) = dom.text(c) {
                        out.push_str(&t);
                    }
                }
                continue;
            }
            if name == w(LocalName::Tc) {
                stack.push((n, true));
            }
            for &c in dom.children(n).iter().rev() {
                stack.push((c, false));
            }
        }
        out
    }
}

/// TS `INVISIBLE_BODY_MARKERS`。
fn is_ts_invisible_marker(name: QName) -> bool {
    name.ns == NsId::W
        && matches!(
            name.local,
            LocalName::BookmarkStart
                | LocalName::BookmarkEnd
                | LocalName::CommentRangeStart
                | LocalName::CommentRangeEnd
                | LocalName::ProofErr
                | LocalName::PermStart
                | LocalName::PermEnd
                | LocalName::MoveFromRangeStart
                | LocalName::MoveFromRangeEnd
                | LocalName::MoveToRangeStart
                | LocalName::MoveToRangeEnd
                | LocalName::CustomXmlInsRangeStart
                | LocalName::CustomXmlInsRangeEnd
                | LocalName::CustomXmlDelRangeStart
                | LocalName::CustomXmlDelRangeEnd
        )
}

fn revision_info(m: &RevisionMeta) -> Map<String, Value> {
    let mut o = Map::new();
    set(&mut o, "author", m.author.clone().unwrap_or_default());
    if let Some(d) = &m.date {
        set(&mut o, "date", d.clone());
    }
    if let Some(i) = &m.id {
        set(&mut o, "id", i.clone());
    }
    o
}

// ---- body 元素与块（COMPAT-04 / TS parseDocx 主循环 + buildBlock）----------------------------------

pub(super) fn body(ctx: &Ctx<'_>) -> (Vec<Value>, Vec<Value>) {
    let dom = ctx.dom;
    let mut elements = Vec::new();
    let mut blocks = Vec::new();
    let Some(body) = ctx.doc.body else { return (elements, blocks) };
    let by_node: HashMap<NodeId, &Block> = ctx.doc.main.iter().map(|b| (b.node(), b)).collect();
    let mut sdt_group = 0u64;
    for &child in dom.children(body) {
        let Some(name) = dom.name(child) else { continue };
        let lex_name = dom
            .lex_name(child)
            .map(str::to_string)
            .unwrap_or_else(|| name.display(dom.interner()).to_string());
        let range = ctx.lex_range(child);
        if name == w(LocalName::Sdt) {
            sdt_blocks(ctx, child, &range, &by_node, &mut sdt_group, &mut elements, &mut blocks);
            continue;
        }
        let i = elements.len();
        elements.push(element_json(ctx, &lex_name, &range));
        blocks.push(Value::Object(body_block(ctx, child, name, &lex_name, &range, i, &by_node)));
    }
    (elements, blocks)
}

fn element_json(ctx: &Ctx<'_>, name: &str, range: &Range<u32>) -> Value {
    json!({ "name": name, "start": ctx.u16(range.start), "end": ctx.u16(range.end) })
}

fn base(ctx: &Ctx<'_>, i: usize, range: &Range<u32>) -> Map<String, Value> {
    let mut o = Map::new();
    set(&mut o, "id", format!("b{i}"));
    set(&mut o, "docxIndex", i);
    set(&mut o, "originalXml", ctx.slice(range));
    o
}

fn passthrough(mut o: Map<String, Value>, label: &str) -> Map<String, Value> {
    set(&mut o, "type", "passthrough");
    set(&mut o, "label", label);
    o
}

/// `docxIndex → 节点`：与 [`body`] 同一套枚举规则（顶层子元素；多段 sdt 拆成每个 `w:p`/`w:tbl`，
/// 其余 sdt 整体一个元素）。`EDIT-04` 的 `SaveBlock.docxIndex` 用它找回节点。
pub fn element_nodes(dom: &Dom, body: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &child in dom.children(body) {
        if dom.node(child).dirty == crate::xml::Dirty::Deleted || dom.name(child).is_none() {
            continue;
        }
        if dom.is(child, w(LocalName::Sdt)) {
            let mut kids = Vec::new();
            sdt_content_children_of(dom, child, &mut kids);
            let parts: Vec<NodeId> = kids
                .into_iter()
                .filter(|&n| dom.is(n, w(LocalName::P)) || dom.is(n, w(LocalName::Tbl)))
                .collect();
            if parts.len() >= 2 {
                out.extend(parts);
            } else {
                out.push(child);
            }
            continue;
        }
        out.push(child);
    }
    out
}

/// 单段 sdt（TS 一个块 + `sdtShell`）里的那个 `w:p`；多段 / 表格 / 空 sdt 返回 `None`。
pub fn sdt_single_paragraph(dom: &Dom, sdt: NodeId) -> Option<NodeId> {
    let mut kids = Vec::new();
    sdt_content_children_of(dom, sdt, &mut kids);
    let parts: Vec<NodeId> = kids
        .into_iter()
        .filter(|&n| dom.is(n, w(LocalName::P)) || dom.is(n, w(LocalName::Tbl)))
        .collect();
    match parts.as_slice() {
        [p] if dom.is(*p, w(LocalName::P)) => Some(*p),
        _ => None,
    }
}

/// sdt 直接内容子元素：`w:sdtContent` 的子元素，嵌套 sdt 透明（TS `splitSdtParts` 跳过 sdt/sdtContent 标签）。
fn sdt_content_children(ctx: &Ctx<'_>, sdt: NodeId, out: &mut Vec<NodeId>) {
    sdt_content_children_of(ctx.dom, sdt, out);
}

fn sdt_content_children_of(dom: &Dom, sdt: NodeId, out: &mut Vec<NodeId>) {
    for &c in dom.children(sdt) {
        if dom.is(c, w(LocalName::SdtContent)) {
            for &cc in dom.children(c) {
                if dom.name(cc).is_none() {
                    continue;
                }
                if dom.is(cc, w(LocalName::Sdt)) {
                    // 嵌套 sdt：sdtPr 也成为"深度 0 子元素"，但只有 p/tbl 参与拆分
                    for &x in dom.children(cc) {
                        if dom.is(x, w(LocalName::SdtContent)) {
                            sdt_content_children_of(dom, cc, out);
                            break;
                        }
                    }
                } else {
                    out.push(cc);
                }
            }
        }
    }
}

/// TS `sdtMeta`。
fn sdt_meta(ctx: &Ctx<'_>, sdt: NodeId) -> (String, String, &'static str) {
    let dom = ctx.dom;
    let Some(pr) = dom.semantic_children(sdt).find(|&n| dom.is(n, w(LocalName::SdtPr))) else {
        return (String::new(), String::new(), "text");
    };
    let val_of = |local: LocalName| -> Option<String> {
        let n = dom.semantic_children(pr).find(|&n| dom.is(n, w(local)))?;
        ctx.attr(n, NsId::W, LocalName::Val)
    };
    let alias = val_of(LocalName::Alias).unwrap_or_default();
    let tag = val_of(LocalName::UTag).or_else(|| val_of(LocalName::Tag)).unwrap_or_default();
    let has = |local: LocalName| dom.semantic_children(pr).any(|n| dom.is(n, w(local)));
    let control = if has(LocalName::Date) {
        "date"
    } else if has(LocalName::DropDownList) || has(LocalName::ComboBox) {
        "dropdown"
    } else if dom
        .semantic_children(pr)
        .any(|n| dom.name(n).is_some_and(|q| q.local == LocalName::Checkbox))
    {
        "checkbox"
    } else {
        "text"
    };
    (alias, tag, control)
}

#[allow(clippy::too_many_arguments)]
fn sdt_blocks(
    ctx: &Ctx<'_>,
    sdt: NodeId,
    range: &Range<u32>,
    by_node: &HashMap<NodeId, &Block>,
    group_seq: &mut u64,
    elements: &mut Vec<Value>,
    blocks: &mut Vec<Value>,
) {
    let dom = ctx.dom;
    let mut kids = Vec::new();
    sdt_content_children(ctx, sdt, &mut kids);
    let parts: Vec<NodeId> = kids
        .into_iter()
        .filter(|&n| dom.is(n, w(LocalName::P)) || dom.is(n, w(LocalName::Tbl)))
        .collect();
    let (alias, tag, control) = sdt_meta(ctx, sdt);
    let shell = |open: &Range<u32>, close: &Range<u32>, group: Option<u64>| {
        let mut s = Map::new();
        set(&mut s, "alias", alias.clone());
        set(&mut s, "tag", tag.clone());
        set(&mut s, "controlType", control);
        set(&mut s, "openXml", ctx.slice(open));
        set(&mut s, "closeXml", ctx.slice(close));
        if let Some(g) = group {
            set(&mut s, "group", g);
        }
        Value::Object(s)
    };
    let default_label = |o: &mut Map<String, Value>| {
        if !o.contains_key("label") {
            let l = if !alias.is_empty() {
                alias.clone()
            } else if !tag.is_empty() {
                tag.clone()
            } else {
                "Content control".to_string()
            };
            set(o, "label", l);
        }
    };
    if parts.len() >= 2 {
        let group = *group_seq;
        *group_seq += 1;
        let n = parts.len();
        for (k, &child) in parts.iter().enumerate() {
            let cr = ctx.lex_range(child);
            let start = if k == 0 { range.start } else { cr.start };
            let end = if k == n - 1 { range.end } else { ctx.lex_range(parts[k + 1]).start };
            let i = elements.len();
            let name = dom.lex_name(child).unwrap_or("w:p").to_string();
            elements.push(element_json(ctx, &name, &(start..end)));
            let mut o = body_block(ctx, child, dom.name(child).unwrap(), &name, &cr, i, by_node);
            set(&mut o, "originalXml", ctx.slice(&(start..end)));
            set(&mut o, "sdtShell", shell(&(start..cr.start), &(cr.end..end), Some(group)));
            default_label(&mut o);
            blocks.push(Value::Object(o));
        }
        return;
    }
    let i = elements.len();
    elements.push(element_json(ctx, "w:sdt", range));
    let mut o = base(ctx, i, range);
    // TS：内容以表格开头 → 表格块；否则第一个 w:p → 段落块 + sdtShell；都没有 → Content control
    match parts.first() {
        Some(&child) if dom.is(child, w(LocalName::Tbl)) => {
            o = table_block(ctx, child, o);
        }
        Some(&child) => {
            let cr = ctx.lex_range(child);
            o = body_block(ctx, child, dom.name(child).unwrap(), "w:p", &cr, i, by_node);
            set(&mut o, "originalXml", ctx.slice(range));
            set(&mut o, "sdtShell", shell(&(range.start..cr.start), &(cr.end..range.end), None));
            default_label(&mut o);
        }
        None => {
            let preview = ctx.plain_text(sdt);
            let has_gfx = dom
                .descendants(sdt)
                .any(|n| dom.is(n, w(LocalName::Drawing)) || dom.is(n, w(LocalName::Pict)));
            o = passthrough(o, "Content control");
            if preview.trim().is_empty() && !has_gfx {
                set(&mut o, "invisibleMarker", true);
            } else {
                set(&mut o, "previewText", preview);
            }
        }
    }
    blocks.push(Value::Object(o));
}

fn table_block(ctx: &Ctx<'_>, tbl: NodeId, mut o: Map<String, Value>) -> Map<String, Value> {
    let dom = ctx.dom;
    // TS tableSummary：整段 xml 里 <w:tr 的个数 × 第一行 <w:tc 的个数
    let rows = dom.descendants(tbl).filter(|&n| dom.is(n, w(LocalName::Tr))).count();
    let cols = dom
        .descendants(tbl)
        .find(|&n| dom.is(n, w(LocalName::Tr)))
        .map(|tr| dom.descendants(tr).filter(|&n| dom.is(n, w(LocalName::Tc))).count())
        .unwrap_or(0);
    set(&mut o, "type", "table");
    set(&mut o, "label", format!("Table {rows}×{cols}"));
    set(&mut o, "previewText", ctx.plain_text(tbl).chars().take(120).collect::<String>());
    // `table` 模型在 M2（KNOWN_DIFFS）
    o
}

#[allow(clippy::too_many_arguments)]
fn body_block(
    ctx: &Ctx<'_>,
    node: NodeId,
    name: QName,
    lex_name: &str,
    range: &Range<u32>,
    i: usize,
    by_node: &HashMap<NodeId, &Block>,
) -> Map<String, Value> {
    let dom = ctx.dom;
    let o = base(ctx, i, range);
    if name.ns != NsId::W {
        let mut o = passthrough(o, lex_name);
        set(&mut o, "previewText", "");
        return o;
    }
    match name.local {
        LocalName::Ins | LocalName::Del => {
            let inner = dom
                .children(node)
                .iter()
                .copied()
                .find(|&c| dom.is(c, w(LocalName::P)) || dom.is(c, w(LocalName::Tbl)));
            if let Some(inner) = inner {
                let inner_name = dom.name(inner).unwrap();
                let inner_lex = dom.lex_name(inner).unwrap_or("w:p").to_string();
                let mut o = body_block(
                    ctx,
                    inner,
                    inner_name,
                    &inner_lex,
                    &ctx.lex_range(inner),
                    i,
                    by_node,
                );
                set(&mut o, "originalXml", ctx.slice(range));
                let mut rev = revision_info(&RevisionMeta {
                    node,
                    id: ctx.attr(node, NsId::W, LocalName::Id),
                    author: ctx.attr(node, NsId::W, LocalName::Author),
                    date: ctx.attr(node, NsId::W, LocalName::Date),
                });
                set(&mut rev, "kind", if name.local == LocalName::Ins { "ins" } else { "del" });
                set(&mut o, "blockRevision", Value::Object(rev));
                return o;
            }
            let mut o = passthrough(o, lex_name);
            set(&mut o, "previewText", "");
            o
        }
        LocalName::SectPr => {
            let mut o = passthrough(o, "Section properties");
            set(&mut o, "hidden", true);
            o
        }
        LocalName::Tbl => table_block(ctx, node, o),
        _ if is_ts_invisible_marker(name) => {
            let mut o = passthrough(o, lex_name);
            set(&mut o, "invisibleMarker", true);
            o
        }
        LocalName::Br => {
            if ctx.attr(node, NsId::W, LocalName::Type).as_deref() == Some("page") {
                let mut o = passthrough(o, "Page break");
                set(&mut o, "fieldDisplay", json!({ "kind": "pageBreak" }));
                o
            } else {
                let mut o = passthrough(o, lex_name);
                set(&mut o, "invisibleMarker", true);
                o
            }
        }
        LocalName::P => paragraph_block(ctx, node, o, by_node.get(&node).copied()),
        _ => {
            let mut o = passthrough(o, lex_name);
            set(&mut o, "previewText", "");
            o
        }
    }
}

fn paragraph_block(
    ctx: &Ctx<'_>,
    p: NodeId,
    o: Map<String, Value>,
    block: Option<&Block>,
) -> Map<String, Value> {
    match block {
        Some(Block::Text(tb)) if mark_vanish_hidden(ctx, tb) => {
            let mut o = passthrough(o, "Hidden paragraph");
            set(&mut o, "invisibleMarker", true);
            o
        }
        // TS `buildBlock` 规则 2 / 3：字段段落与 TOC 行是只读的 passthrough，不出 runs / format
        Some(Block::Text(tb)) => match ts_field_passthrough(ctx, tb) {
            Some(label) => {
                let mut o = passthrough(o, &label);
                set(&mut o, "previewText", ctx.plain_text(p));
                if let Some(id) = &tb.style_id {
                    set(&mut o, "styleId", id.clone());
                }
                if let Some(fd) = field_display(ctx, p, tb.facts.toc_style_level) {
                    set(&mut o, "fieldDisplay", fd);
                }
                o
            }
            None => text_block(ctx, tb, o),
        },
        Some(Block::Protected(pb)) => match &pb.kind {
            ProtectedKind::Invisible => {
                let mut o = passthrough(o, "Hidden paragraph");
                set(&mut o, "invisibleMarker", true);
                o
            }
            ProtectedKind::SectionBreak => {
                let mut o = passthrough(o, "Section break paragraph");
                set(&mut o, "previewText", "");
                o
            }
            // `COMPAT-03`：`Protected(FieldBlockResult)`（`R09`：Block 策略字段的结果段落）
            // 与字段文本段落同形——TS 那边它们都是同一个 passthrough 分支
            ProtectedKind::FieldBlockResult(_) => {
                let style = para_style_id(ctx, p);
                let toc = style.as_deref().and_then(crate::model::facts::toc_level_of_id);
                // TS 没有 R09：它逐段判定。块字段中间那些**自己不含 fldChar / instrText** 的段落
                // 在 TS 那边落到规则 3（TOC 样式 → `TOC entry`）。既不含字段结构又没有目录样式的
                // 段落 TS 会当普通段落，本引擎按 `FLD-08` 保护整段区间（差异见 `docs/04` §8）。
                let label = if has_field_chars(ctx, p) {
                    field_label(ctx, p)
                } else if toc.is_some() {
                    "TOC entry".to_string()
                } else {
                    "Paragraph".to_string()
                };
                let mut o = passthrough(o, &label);
                set(&mut o, "previewText", ctx.plain_text(p));
                if let Some(id) = &style {
                    set(&mut o, "styleId", id.clone());
                }
                if let Some(fd) = field_display(ctx, p, toc) {
                    set(&mut o, "fieldDisplay", fd);
                }
                o
            }
            kind => {
                let label = match kind {
                    ProtectedKind::Equation => "Equation",
                    ProtectedKind::Chart => "Chart",
                    ProtectedKind::SmartArt => "SmartArt",
                    ProtectedKind::Ole => "Embedded object",
                    ProtectedKind::Rule => "Drawing object",
                    _ => "Paragraph",
                };
                let mut o = passthrough(o, label);
                set(&mut o, "previewText", ctx.plain_text(p));
                o
            }
        },
        Some(Block::Image(_)) => {
            let mut o = o;
            set(&mut o, "type", "image");
            o
        }
        _ => {
            let mut o = passthrough(o, "Paragraph");
            set(&mut o, "previewText", ctx.plain_text(p));
            o
        }
    }
}

// ---- 字段段落（TS `buildBlock` 规则 2 / 3，`docs/01` §6.2）------------------------------------

/// 起点在本段的字段（`MOD-04` 的 `facts.fields`）。
fn para_fields<'a>(ctx: &'a Ctx<'_>, tb: &TextBlock) -> Vec<&'a FieldSpan> {
    tb.facts.fields.iter().filter_map(|&id| ctx.doc.fields.get(id)).collect()
}

/// 段落里有配不上对的 `fldChar` / `instrText`（未闭合、孤立 end）。
///
/// 这种段落 TS 一律走 passthrough：`extractRuns` 折不动半个字段。已识别字段的结构 run 不算——
/// 它们的 run 在索引里查得到。
fn has_stray_field_chars(ctx: &Ctx<'_>, tb: &TextBlock) -> bool {
    tb.inlines.iter().any(|i| match i {
        Inline::Run(r) => {
            r.segments.iter().any(|sg| {
                matches!(
                    sg.kind,
                    SegmentKind::FldChar | SegmentKind::InstrText | SegmentKind::DelInstrText
                )
            }) && ctx.doc.fields.field_of(r.node).is_none()
        }
        _ => false,
    })
}

/// TS `onlyXeFields` 的单字段判定：这些字段会被折成可编辑 run（`COMPAT-07`），其余让整段变
/// passthrough。`w:fldSimple` 一律不折（TS 的 `onlyXeFields` 第一条）。
fn ts_collapsible(ctx: &Ctx<'_>, f: &FieldSpan) -> bool {
    if !f.form.is_complex() {
        return false;
    }
    match f.keyword() {
        Keyword::Xe | Keyword::Ref => true,
        Keyword::FormCheckBox => {
            matches!(read_form_data(ctx.dom, f.ff_data), Some(FormData::CheckBox { .. }))
        }
        Keyword::Hyperlink => ts_convertible_hyperlink(f),
        k => is_simple_inline(k),
    }
}

/// 段落里第一个字段的关键字（文档序）。
///
/// 直接走 DOM：未闭合的字段没有 `FieldSpan`（`FLD-02` 第 6 条），保护块（`R09`）也没有 inlines，
/// 两种情况下指令都只在节点里。连续的 `w:instrText` 先攒起来，遇到 `w:fldChar` 才结算——
/// `PAGE` 被拆成 `PA` + `GE` 也认得出。
fn first_instr_keyword(ctx: &Ctx<'_>, p: NodeId) -> Option<Keyword> {
    let dom = ctx.dom;
    let mut pending = String::new();
    for n in dom.descendants(p) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::W {
            continue;
        }
        match name.local {
            LocalName::InstrText | LocalName::DelInstrText => {
                for c in dom.semantic_children(n) {
                    if let Some(t) = dom.text(c) {
                        pending.push_str(&t);
                    }
                }
            }
            LocalName::FldChar | LocalName::T if !pending.trim().is_empty() => {
                return Some(instr_keyword(&pending));
            }
            _ => {}
        }
    }
    (!pending.trim().is_empty()).then(|| instr_keyword(&pending))
}

fn instr_keyword(raw: &str) -> Keyword {
    crate::span::field::instr::parse(raw, &[]).keyword
}

/// TS `fieldLabel`：段落里第一个字段的关键字决定标签；没有指令（只剩孤立的 `fldChar`）时是
/// "字段结束标记"，段落里还有分页符则再加一句。
fn field_label(ctx: &Ctx<'_>, p: NodeId) -> String {
    match first_instr_keyword(ctx, p) {
        None => {
            if has_page_break(ctx, p) {
                "Field end marker + page break".to_string()
            } else {
                "Field end marker".to_string()
            }
        }
        Some(k) => match k {
            Keyword::Toc => "Auto TOC (updates when opened in Word)".to_string(),
            Keyword::PageRef => "Page reference field".to_string(),
            Keyword::IncludePicture => "Linked picture field".to_string(),
            Keyword::Hyperlink => "Hyperlink field".to_string(),
            Keyword::Seq => "Caption number field".to_string(),
            Keyword::Page => "Page number field".to_string(),
            k => format!("Field ({})", k.as_str()),
        },
    }
}

fn has_page_break(ctx: &Ctx<'_>, p: NodeId) -> bool {
    let dom = ctx.dom;
    dom.descendants(p).any(|n| {
        dom.is(n, w(LocalName::Br))
            && ctx.attr(n, NsId::W, LocalName::Type).as_deref() == Some("page")
    })
}

/// TS `buildBlock` 规则 2 / 3：文本段落是否走 passthrough，返回标签。
///
/// 规则 2：有字段且不是"全部可折叠"→ 字段 passthrough。规则 3：TOC 系列样式的段落 → `TOC entry`。
/// 判定只看 facts 与字段索引，不重新解析 XML（`COMPAT-03`）。
fn ts_field_passthrough(ctx: &Ctx<'_>, tb: &TextBlock) -> Option<String> {
    let fields = para_fields(ctx, tb);
    let stray = has_stray_field_chars(ctx, tb);
    // 有配不上对的结构，或者有字段但不是"全部可折叠"
    if stray || (!fields.is_empty() && !fields.iter().all(|f| ts_collapsible(ctx, f))) {
        return Some(field_label(ctx, tb.node));
    }
    tb.facts.toc_style_level.map(|_| "TOC entry".to_string())
}

/// 段落自身有 `w:fldChar` / `w:instrText` / `w:fldSimple`（TS `hasFields`，逐段判定）。
fn has_field_chars(ctx: &Ctx<'_>, p: NodeId) -> bool {
    let dom = ctx.dom;
    dom.descendants(p).any(|n| {
        dom.name(n).is_some_and(|q| {
            q.ns == NsId::W
                && matches!(
                    q.local,
                    LocalName::FldChar
                        | LocalName::InstrText
                        | LocalName::DelInstrText
                        | LocalName::FldSimple
                )
        })
    })
}

/// 段落的 `w:pStyle`（保护块没有 `TextBlock.style_id`）。
fn para_style_id(ctx: &Ctx<'_>, p: NodeId) -> Option<String> {
    let dom = ctx.dom;
    let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)))?;
    let style = dom.semantic_children(ppr).find(|&n| dom.is(n, w(LocalName::PStyle)))?;
    ctx.attr(style, NsId::W, LocalName::Val)
}

/// 段落里贡献显示文字的 run：`(节点, 文字, 字号)`。`w:tab` 记成 `\t`，指令与 `fldChar` 不算。
fn display_runs(ctx: &Ctx<'_>, p: NodeId) -> Vec<(NodeId, String, Option<i64>)> {
    let dom = ctx.dom;
    let mut out = Vec::new();
    for r in dom.descendants(p).filter(|&n| dom.is(n, w(LocalName::R))) {
        let mut text = String::new();
        for c in dom.semantic_children(r) {
            let Some(name) = dom.name(c) else { continue };
            if name.ns != NsId::W {
                continue;
            }
            match name.local {
                LocalName::T | LocalName::DelText => {
                    for t in dom.semantic_children(c) {
                        if let Some(s) = dom.text(t) {
                            text.push_str(&s);
                        }
                    }
                }
                LocalName::Tab | LocalName::Ptab => text.push('\t'),
                _ => {}
            }
        }
        if text.is_empty() {
            continue;
        }
        let sz = dom
            .semantic_children(r)
            .find(|&n| dom.is(n, w(LocalName::RPr)))
            .and_then(|rpr| dom.semantic_children(rpr).find(|&n| dom.is(n, w(LocalName::Sz))))
            .and_then(|sz| ctx.attr(sz, NsId::W, LocalName::Val))
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|&n| n != 0);
        out.push((r, text, sz));
    }
    out
}

/// TS `fieldDisplayOf`：`pageBreak` / `tocLine` / `text` 三选一，都不成立时没有 `fieldDisplay`。
///
/// `toc_level` 是段落样式的目录级别（`MOD-04` 的 `toc_style_level`）；保护块没有 facts，调用方
/// 从 `styleId` 现算。
fn field_display(ctx: &Ctx<'_>, p: NodeId, toc_level: Option<u8>) -> Option<Value> {
    let runs = display_runs(ctx, p);
    let text: String = runs.iter().map(|(_, t, _)| t.as_str()).collect();
    if text.replace('\t', "").trim().is_empty() {
        return has_page_break(ctx, p).then(|| json!({ "kind": "pageBreak" }));
    }
    match toc_level {
        Some(level) => Some(toc_line_display(ctx, p, &text, &runs, level)),
        None => Some(text_display(ctx, p, &text, &runs)),
    }
}

/// 目录行：按制表符切成 `num? / left / right`。
fn toc_line_display(
    ctx: &Ctx<'_>,
    p: NodeId,
    text: &str,
    runs: &[(NodeId, String, Option<i64>)],
    level: u8,
) -> Value {
    let parts: Vec<&str> = text.split('\t').collect();
    let (num, left, right) = if parts.len() < 2 {
        (None, text.to_string(), String::new())
    } else {
        let right = parts[parts.len() - 1].to_string();
        let head = &parts[..parts.len() - 1];
        if head.len() >= 2 && looks_like_toc_number(head[0]) {
            (Some(head[0].to_string()), head[1..].join(" "), right)
        } else {
            (None, head.join(" "), right)
        }
    };
    let mut o = Map::new();
    if let Some(a) = toc_anchor(ctx, p) {
        set(&mut o, "anchor", a);
    }
    set(&mut o, "kind", "tocLine");
    set(&mut o, "left", left);
    set(&mut o, "level", i64::from(level));
    if let Some(n) = num {
        set(&mut o, "num", n);
    }
    set(&mut o, "right", right);
    if let Some(sz) = runs.first().and_then(|(_, _, sz)| *sz) {
        set(&mut o, "szHalfPoints", sz);
    }
    Value::Object(o)
}

/// `1.1.` 一类的目录编号：只有数字、点、连字符，且至少一个数字。
fn looks_like_toc_number(s: &str) -> bool {
    !s.is_empty()
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-')
}

/// 目录行里的内部链接锚点（`w:hyperlink w:anchor`）。
fn toc_anchor(ctx: &Ctx<'_>, p: NodeId) -> Option<String> {
    let dom = ctx.dom;
    dom.descendants(p)
        .find(|&n| dom.is(n, w(LocalName::Hyperlink)))
        .and_then(|h| ctx.attr(h, NsId::W, LocalName::Anchor))
}

/// 普通字段段落的显示：整段文字 + 段落排版 + 字号 / 字体（不统一时逐 run 给）。
fn text_display(
    ctx: &Ctx<'_>,
    p: NodeId,
    text: &str,
    runs: &[(NodeId, String, Option<i64>)],
) -> Value {
    let dom = ctx.dom;
    let mut o = Map::new();
    let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)));
    let mut warnings = Vec::new();
    let props = crate::semantic::props::read_para_props(dom, ppr, &mut warnings);
    if let Some(Value::Object(f)) = para_format_of_props(ctx, &props, ppr, p, false) {
        for k in ["align", "lineRawTwips", "lineRule", "lineSpacing"] {
            if let Some(v) = f.get(k) {
                o.insert(k.to_string(), v.clone());
            }
        }
    }
    if let Some(font) = runs.first().and_then(|(r, _, _)| {
        let rpr = dom.semantic_children(*r).find(|&n| dom.is(n, w(LocalName::RPr)));
        let rp = crate::semantic::props::read_run_props(dom, rpr, &mut warnings);
        ctx.resolver.fonts(&rp).display().map(str::to_string)
    }) {
        set(&mut o, "fontFamily", font);
    }
    set(&mut o, "kind", "text");
    // TS 的 `left` 是 trim 过的整段文字（语料 `linked-image__003`：`"Company logo: "` → `"Company logo:"`）
    set(&mut o, "left", text.trim().to_string());
    let sizes: Vec<Option<i64>> = runs.iter().map(|(_, _, sz)| *sz).collect();
    let uniform = sizes.windows(2).all(|w| w[0] == w[1]);
    match (uniform, sizes.first().copied().flatten()) {
        (true, Some(sz)) => set(&mut o, "szHalfPoints", sz),
        (true, None) => {}
        (false, _) => {
            let list: Vec<Value> = runs
                .iter()
                .map(|(_, t, sz)| {
                    let mut m = Map::new();
                    if let Some(sz) = sz {
                        set(&mut m, "szHalfPoints", *sz);
                    }
                    set(&mut m, "text", t.clone());
                    Value::Object(m)
                })
                .collect();
            set(&mut o, "runs", Value::Array(list));
        }
    }
    Value::Object(o)
}

/// TS `buildTextParagraph` 的第三条隐藏规则（`docs/01` §6.3）：段落标记 `rPr/vanish` 为真（非 specVanish）、
/// 无可见文本、`staysVanished`、且没有影响排版的无文字内容（br/cr/tab/sym/脚注引用）。
fn mark_vanish_hidden(ctx: &Ctx<'_>, tb: &TextBlock) -> bool {
    let Some(mark) = tb.props.rpr.as_ref() else { return false };
    if mark.vanish != Some(true) || mark.spec_vanish == Some(true) {
        return false;
    }
    let f = &tb.facts;
    if f.visible_text
        || f.unvanish
        || f.has_range_marker
        || !f.drawings.is_empty()
        || !f.picts.is_empty()
        || f.objects > 0
        || f.has_sect_pr
        || tb.props.num.is_some()
    {
        return false;
    }
    if !ctx.plain_text(tb.node).trim().is_empty() {
        return false;
    }
    let layout = tb.inlines.iter().any(|i| match i {
        Inline::Run(r) => r.segments.iter().any(|s| {
            matches!(
                s.kind,
                SegmentKind::Br { .. }
                    | SegmentKind::Cr
                    | SegmentKind::Tab
                    | SegmentKind::Sym { .. }
                    | SegmentKind::FootnoteRef { .. }
                    | SegmentKind::EndnoteRef { .. }
            )
        }),
        Inline::Atom(a) => matches!(a.kind, AtomKind::BareBreak { .. }),
        Inline::Field { .. } => false,
    });
    !layout
}

// ---- 文本块（TS buildTextParagraph）------------------------------------------------------------------

fn text_block(ctx: &Ctx<'_>, tb: &TextBlock, mut o: Map<String, Value>) -> Map<String, Value> {
    let dom = ctx.dom;
    let p = tb.node;
    match &tb.kind {
        TextKind::Paragraph => set(&mut o, "type", "paragraph"),
        TextKind::Heading { level } => {
            set(&mut o, "type", "heading");
            set(&mut o, "level", *level);
        }
        TextKind::ListItem { list } => {
            set(&mut o, "type", "listItem");
            set(
                &mut o,
                "list",
                json!({ "kind": list_kind(ctx.numbering, i64::from(list.num_id), i64::from(list.ilvl)), "numId": list.num_id.to_string(), "ilvl": list.ilvl }),
            );
        }
    }
    if let Some(s) = &tb.style_id {
        set(&mut o, "styleId", s.clone());
    }
    // rawPPr：w:pPr 须是 w:p 的第一个子元素
    let ppr = dom.children(p).iter().copied().find(|&c| dom.name(c).is_some());
    let ppr = ppr.filter(|&c| dom.is(c, w(LocalName::PPr)));
    if let Some(ppr) = ppr {
        set(&mut o, "rawPPr", ctx.slice(&ctx.lex_range(ppr)));
    }
    let runs = runs_json(ctx, tb);
    if let Some(f) = para_format(ctx, tb, ppr, runs.is_empty()) {
        set(&mut o, "format", f);
    }
    let (bookmarks, hidden, cstarts, cends) = markers(ctx, p);
    for (k, v) in [
        ("bookmarks", bookmarks),
        ("hiddenBookmarks", hidden),
        ("commentStarts", cstarts),
        ("commentEnds", cends),
    ] {
        if !v.is_empty() {
            set(&mut o, k, v);
        }
    }
    set(&mut o, "runs", Value::Array(runs.into_iter().map(Value::Object).collect()));
    // revExtras
    if tb.facts.revision.move_from {
        set(&mut o, "moveRevision", "from");
    } else if tb.facts.revision.move_to {
        set(&mut o, "moveRevision", "to");
    }
    for rev in &tb.revisions {
        match rev {
            Revision::ParaPropsChange { meta, old } => {
                let mut info = revision_info(meta);
                let mut old_json = para_format_of_props(ctx, old, None, p, false)
                    .map(|v| match v {
                        Value::Object(m) => m,
                        _ => Map::new(),
                    })
                    .unwrap_or_default();
                if let Some(sid) = &old.style {
                    set(&mut old_json, "styleId", sid.clone());
                }
                let old_num = old.num.as_ref().and_then(|n| n.num_id.as_ref()).map(|v| match v {
                    Val::Value(n) => n.to_string(),
                    Val::Raw(s) => s.clone(),
                });
                if let Some(num_id) = old_num {
                    let ilvl = old.num.as_ref().and_then(|n| i32_of(&n.ilvl)).unwrap_or(0);
                    set(&mut old_json, "type", "docListItem");
                    set(&mut old_json, "numId", num_id.clone());
                    set(&mut old_json, "ilvl", ilvl);
                    set(
                        &mut old_json,
                        "kind",
                        list_kind(ctx.numbering, parse_int(&num_id).unwrap_or(0), ilvl),
                    );
                } else {
                    let level = match &old.outline_lvl {
                        Some(Val::Value(l)) if (0..=8).contains(l) => Some(*l as u8 + 1),
                        Some(_) => None,
                        None => old.style.as_deref().and_then(|s| ctx.resolver.heading_level(s)),
                    };
                    if let Some(l) = level {
                        set(&mut old_json, "type", "docHeading");
                        set(&mut old_json, "level", l);
                    } else if old.style.is_some() {
                        set(&mut old_json, "type", "docParagraph");
                    }
                }
                if !old_json.is_empty() {
                    set(&mut info, "old", Value::Object(old_json));
                }
                set(&mut o, "pPrChangeInfo", Value::Object(info));
            }
            Revision::ParaMarkDelete(meta) => {
                set(&mut o, "paraMarkDel", Value::Object(revision_info(meta)));
            }
            _ => {}
        }
    }
    o
}

/// TS `bookmarkNamesOf` / `crossParaCommentMarkers`（排除文本框内容）。
fn markers(ctx: &Ctx<'_>, p: NodeId) -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) {
    let dom = ctx.dom;
    let mut names: Vec<String> = Vec::new();
    let mut hidden: Vec<String> = Vec::new();
    let mut starts: Vec<String> = Vec::new();
    let mut ends: Vec<String> = Vec::new();
    let mut stack = vec![p];
    while let Some(n) = stack.pop() {
        let Some(name) = dom.name(n) else { continue };
        if name == w(LocalName::TxbxContent) {
            continue;
        }
        if name == w(LocalName::BookmarkStart)
            && let Some(nm) = ctx.attr(n, NsId::W, LocalName::Name)
        {
            let list = if nm.starts_with('_') { &mut hidden } else { &mut names };
            if !list.contains(&nm) {
                list.push(nm);
            }
        } else if name == w(LocalName::CommentRangeStart)
            && let Some(id) = ctx.attr(n, NsId::W, LocalName::Id)
        {
            starts.push(id);
        } else if name == w(LocalName::CommentRangeEnd)
            && let Some(id) = ctx.attr(n, NsId::W, LocalName::Id)
        {
            ends.push(id);
        }
        for &c in dom.children(n).iter().rev() {
            stack.push(c);
        }
    }
    let only_starts: Vec<Value> =
        starts.iter().filter(|s| !ends.contains(s)).map(|s| Value::String(s.clone())).collect();
    let only_ends: Vec<Value> =
        ends.iter().filter(|e| !starts.contains(e)).map(|s| Value::String(s.clone())).collect();
    (
        names.into_iter().map(Value::String).collect(),
        hidden.into_iter().map(Value::String).collect(),
        only_starts,
        only_ends,
    )
}

// ---- ParaFormat（TS extractParaFormat 及 buildTextParagraph 的补充）------------------------------------

fn para_format(
    ctx: &Ctx<'_>,
    tb: &TextBlock,
    ppr: Option<NodeId>,
    runs_empty: bool,
) -> Option<Value> {
    let Value::Object(mut f) = para_format_of_props(ctx, &tb.props, ppr, tb.node, true)
        .unwrap_or(Value::Object(Map::new()))
    else {
        unreachable!()
    };
    // 样式的 autoSpace === false 补到段落
    if !f.contains_key("autoSpace")
        && let Some(sid) = &tb.style_id
        && ctx.style_disp(sid, StyleType::Paragraph).auto_space == Some(false)
    {
        set(&mut f, "autoSpace", false);
    }
    if runs_empty {
        if let Some(sz) = empty_para_size(ctx, tb.node, ppr) {
            set(&mut f, "emptyRunSizeHalfPoints", sz);
        }
        if let Some(font) = empty_para_font(ctx, tb.node, ppr) {
            set(&mut f, "emptyRunFontFamily", font);
        }
    }
    // w:ptab 的显示制表位
    let mut ptabs: Vec<Value> = Vec::new();
    for n in ctx.dom.descendants(tb.node) {
        if ctx.dom.is(n, w(LocalName::Ptab))
            && let Some(al) = ctx.attr(n, NsId::W, LocalName::Alignment)
            && (al == "center" || al == "right")
        {
            let pos = if al == "center" { 50 } else { 100 };
            let mut stop = Map::new();
            set(&mut stop, "pos", pos);
            set(&mut stop, "val", al);
            set(&mut stop, "rel", "margin");
            if let Some(leader) = ctx.attr(n, NsId::W, LocalName::Leader)
                && leader != "none"
                && ["dot", "hyphen", "underscore", "heavy", "middleDot"].contains(&leader.as_str())
            {
                set(&mut stop, "leader", leader);
            }
            ptabs.push(Value::Object(stop));
        }
    }
    if !ptabs.is_empty() {
        let mut stops = f.get("tabStops").and_then(Value::as_array).cloned().unwrap_or_default();
        for st in ptabs {
            let dup = stops.iter().any(|s| {
                s.get("rel").and_then(Value::as_str) == Some("margin")
                    && s.get("pos") == st.get("pos")
            });
            if !dup {
                stops.push(st);
            }
        }
        set(&mut f, "tabStops", Value::Array(stops));
    }
    (!f.is_empty()).then_some(Value::Object(f))
}

/// TS `extractParaFormat(pPr)`：`props` 是声明值；`ppr` 节点用于重复 `w:pBdr` 容器。
fn para_format_of_props(
    ctx: &Ctx<'_>,
    props: &ParaProps,
    ppr: Option<NodeId>,
    _p: NodeId,
    with_borders: bool,
) -> Option<Value> {
    let mut f = Map::new();
    if props.bidi == Some(true) {
        set(&mut f, "bidi", true);
    }
    if let Some(a) = props.jc.as_ref().and_then(jc_align) {
        let a = if props.bidi == Some(true) {
            match a {
                "left" => "right",
                "right" => "left",
                x => x,
            }
        } else {
            a
        };
        set(&mut f, "align", a);
    }
    if let Some(sp) = &props.spacing {
        let rule = super::decl::line_rule(&sp.line_rule);
        // TS lineTwipsOf：只接受 ^-?\d+$
        let line = match &sp.line {
            Some(Val::Value(n)) => Some(i64::from(*n)),
            Some(Val::Raw(s))
                if s.trim() == s
                    && (s.strip_prefix('-').unwrap_or(s)).bytes().all(|b| b.is_ascii_digit())
                    && !s.is_empty() =>
            {
                parse_int(s)
            }
            _ => None,
        };
        match line {
            Some(l) if l > 0 => {
                set(&mut f, "lineRawTwips", l);
                if rule == "auto" {
                    set(&mut f, "lineSpacing", ((l as f64 / 240.0) * 100.0).round() / 100.0);
                    set(&mut f, "lineRule", "auto");
                } else {
                    set(&mut f, "lineRule", rule.clone());
                }
            }
            Some(0) if rule == "atLeast" => {
                set(&mut f, "lineRule", "atLeast");
                set(&mut f, "lineRawTwips", 0);
            }
            _ => {}
        }
        if let Some(b) = sp.before_autospacing {
            set(&mut f, "spaceBeforeAuto", b);
        }
        if let Some(b) = sp.after_autospacing {
            set(&mut f, "spaceAfterAuto", b);
        }
        if let Some(n) = twips_int(&sp.before).filter(|&n| n >= 0) {
            set(&mut f, "spaceBefore", n);
        }
        if let Some(n) = twips_int(&sp.after).filter(|&n| n >= 0) {
            set(&mut f, "spaceAfter", n);
        }
    }
    if let Some(ind) = &props.indent {
        if let Some(l) = twips_int(&ind.start) {
            set(&mut f, "indentLeft", l);
        }
        if let Some(r) = twips_int(&ind.end).filter(|&n| n != 0) {
            set(&mut f, "indentRight", r);
        }
        let hanging = twips_int(&ind.hanging).unwrap_or(0);
        let first = twips_int(&ind.first_line).unwrap_or(0);
        if hanging > 0 {
            set(&mut f, "indentFirstLine", -hanging);
        } else if first > 0 {
            set(&mut f, "indentFirstLine", first);
        }
    }
    if let Some(b) = props.page_break_before {
        set(&mut f, "pageBreakBefore", b);
    }
    if props.keep_next == Some(true) {
        set(&mut f, "keepNext", true);
    }
    if props.keep_lines == Some(true) {
        set(&mut f, "keepLines", true);
    }
    if props.snap_to_grid == Some(false) {
        set(&mut f, "snapToGrid", false);
    }
    if props.widow_control == Some(false) {
        set(&mut f, "widowControl", false);
    }
    if let Some(b) = props.contextual_spacing {
        set(&mut f, "contextualSpacing", b);
    }
    if let Some(b) = auto_space(props) {
        set(&mut f, "autoSpace", b);
    }
    if let Some(shd) = &props.shading {
        // TS 用属性原文（保留大小写）；有 pPr 节点时从 DOM 取
        let raw = ppr.and_then(|pp| {
            ctx.dom.semantic_children(pp).find(|&n| ctx.dom.is(n, w(LocalName::Shd)))
        });
        let (fill_raw, color_raw, val_raw) = match raw {
            Some(n) => (
                ctx.attr(n, NsId::W, LocalName::Fill),
                ctx.attr(n, NsId::W, LocalName::Color),
                ctx.attr(n, NsId::W, LocalName::Val),
            ),
            None => (
                shd.fill.as_ref().map(|v| match v {
                    Val::Value(c) => c.to_xml(),
                    Val::Raw(s) => s.clone(),
                }),
                shd.color.as_ref().map(|v| match v {
                    Val::Value(c) => c.to_xml(),
                    Val::Raw(s) => s.clone(),
                }),
                val_text(&shd.val, |v| v.as_str()),
            ),
        };
        let fill = fill_raw.as_deref().filter(|s| *s != "auto").map(|s| strip_hash(s).to_string());
        if let Some(fl) = &fill {
            set(&mut f, "shadingFill", fl.clone());
        }
        if let Some(d) = super::decl::shd_display_fill_raw(
            val_raw.as_deref(),
            color_raw.as_deref(),
            fill_raw.as_deref(),
        ) && Some(&d) != fill.as_ref()
        {
            set(&mut f, "shadingDisplay", d);
        }
    }
    if with_borders && let Some(ppr) = ppr {
        borders_json(ctx, ppr, &mut f);
    } else if !with_borders && let Some(b) = &props.borders {
        // 旧值快照：只有一份 pBdr，从声明值算
        let mut borders = String::new();
        let mut lines = Map::new();
        for (side, ch) in [(&b.top, "t"), (&b.bottom, "b"), (&b.left, "l"), (&b.right, "r")] {
            let Some(bd) = side else { continue };
            let val = val_text(&bd.val, |v| v.as_str());
            if matches!(val.as_deref(), Some("none" | "nil")) {
                continue;
            }
            borders.push_str(ch);
            let mut line = Map::new();
            if let Some(c) = bd.color.as_ref().map(|v| match v {
                Val::Value(c) => c.to_xml(),
                Val::Raw(s) => s.clone(),
            }) && c != "auto"
            {
                set(&mut line, "color", strip_hash(&c).to_string());
            }
            if let Some(sz) = u32_of(&bd.sz).filter(|&n| n > 0) {
                set(&mut line, "szPt", sz as f64 / 8.0);
            }
            if !line.is_empty() {
                lines.insert(ch.to_string(), Value::Object(line));
            }
        }
        if !borders.is_empty() {
            set(&mut f, "borders", borders);
            if !lines.is_empty() {
                set(&mut f, "borderLines", Value::Object(lines));
            }
        }
    }
    if let Some(stops) = tab_stops(props) {
        set(&mut f, "tabStops", stops);
    }
    if let Some(fr) = &props.frame
        && let Some(dc) = val_text(&fr.drop_cap, |d| d.as_str())
        && (dc == "drop" || dc == "margin")
    {
        let lines = fr.lines.as_ref().map_or(Some(3), |v| match v {
            Val::Value(n) => Some(i64::from(*n)),
            Val::Raw(s) => parse_int(s),
        });
        let lines = lines.filter(|&n| n != 0).unwrap_or(3);
        set(&mut f, "dropCap", json!({ "type": dc, "lines": lines }));
    }
    (!f.is_empty()).then_some(Value::Object(f))
}

/// TS `parseInt(attr, 10)`：`Twips` 值直接用，`Raw` 按前缀整数。
fn twips_int(v: &Option<Val<i32>>) -> Option<i64> {
    match v {
        Some(Val::Value(n)) => Some(i64::from(*n)),
        Some(Val::Raw(s)) => parse_int(s),
        None => None,
    }
}

/// TS `pBdrs`：每边取最后一个含该边的 `w:pBdr` 容器里的元素。
fn borders_json(ctx: &Ctx<'_>, ppr: NodeId, f: &mut Map<String, Value>) {
    let dom = ctx.dom;
    let pbdrs: Vec<NodeId> =
        dom.semantic_children(ppr).filter(|&n| dom.is(n, w(LocalName::PBdr))).collect();
    if pbdrs.is_empty() {
        return;
    }
    let mut borders = String::new();
    let mut lines = Map::new();
    for (side, ch) in [
        (LocalName::Top, "t"),
        (LocalName::Bottom, "b"),
        (LocalName::Left, "l"),
        (LocalName::Right, "r"),
    ] {
        let mut el = None;
        for &pb in &pbdrs {
            if let Some(e) = dom.semantic_children(pb).find(|&n| dom.is(n, w(side))) {
                el = Some(e);
            }
        }
        let Some(el) = el else { continue };
        let val = ctx.attr(el, NsId::W, LocalName::Val);
        if matches!(val.as_deref(), Some("none" | "nil")) {
            continue;
        }
        borders.push_str(ch);
        let mut line = Map::new();
        if let Some(c) = ctx.attr(el, NsId::W, LocalName::Color)
            && c != "auto"
        {
            set(&mut line, "color", strip_hash(&c).to_string());
        }
        if let Some(sz) =
            ctx.attr(el, NsId::W, LocalName::Sz).as_deref().and_then(parse_int).filter(|&n| n > 0)
        {
            set(&mut line, "szPt", sz as f64 / 8.0);
        }
        if !line.is_empty() {
            lines.insert(ch.to_string(), Value::Object(line));
        }
    }
    if !borders.is_empty() {
        set(f, "borders", borders);
        if !lines.is_empty() {
            set(f, "borderLines", Value::Object(lines));
        }
    }
}

/// TS `emptyParaSizeHalfPoints`：段落标记 `pPr/rPr/sz`，否则最后一个直接子 `w:r` 的 `rPr/sz`。
fn empty_para_size(ctx: &Ctx<'_>, p: NodeId, ppr: Option<NodeId>) -> Option<i64> {
    let dom = ctx.dom;
    let sz_of = |rpr: NodeId| -> Option<String> {
        let sz = dom.semantic_children(rpr).find(|&n| dom.is(n, w(LocalName::Sz)))?;
        ctx.attr(sz, NsId::W, LocalName::Val)
    };
    let mark = ppr
        .and_then(|pp| dom.semantic_children(pp).find(|&n| dom.is(n, w(LocalName::RPr))))
        .and_then(sz_of);
    let mut sz = mark;
    if sz.is_none() {
        for r in dom.semantic_children(p).filter(|&n| dom.is(n, w(LocalName::R))) {
            if let Some(rpr) = dom.semantic_children(r).find(|&n| dom.is(n, w(LocalName::RPr)))
                && let Some(v) = sz_of(rpr)
            {
                sz = Some(v);
            }
        }
    }
    parse_int(&sz?).filter(|&n| n > 0)
}

/// TS `emptyParaMarkFont`。
fn empty_para_font(ctx: &Ctx<'_>, p: NodeId, ppr: Option<NodeId>) -> Option<String> {
    let dom = ctx.dom;
    let pick = |rpr: NodeId| -> Option<String> {
        let rf = dom.semantic_children(rpr).find(|&n| dom.is(n, w(LocalName::RFonts)))?;
        ctx.attr(rf, NsId::W, LocalName::Ascii)
            .or_else(|| ctx.attr(rf, NsId::W, LocalName::HAnsi))
            .or_else(|| ctx.attr(rf, NsId::W, LocalName::EastAsia))
    };
    let mut font = ppr
        .and_then(|pp| dom.semantic_children(pp).find(|&n| dom.is(n, w(LocalName::RPr))))
        .and_then(pick);
    if font.is_none() {
        for r in dom.semantic_children(p).filter(|&n| dom.is(n, w(LocalName::R))) {
            if let Some(rpr) = dom.semantic_children(r).find(|&n| dom.is(n, w(LocalName::RPr)))
                && let Some(v) = pick(rpr)
            {
                font = Some(v);
            }
        }
    }
    font
}

// ---- Run（TS extractRuns / buildRun / mergeRuns）--------------------------------------------------------

fn runs_json(ctx: &Ctx<'_>, tb: &TextBlock) -> Vec<Map<String, Value>> {
    let mut para_disp =
        tb.style_id.as_deref().map(|s| ctx.style_disp(s, StyleType::Paragraph)).unwrap_or_default();
    if tb.style_id.is_none()
        && let Some(def) = ctx.resolver.default_style(StyleType::Paragraph).and_then(|s| s.id())
    {
        // TS `defaultParaVanish`：只有 vanish 走默认样式
        para_disp.vanish = ctx.style_disp(def, StyleType::Paragraph).vanish.filter(|&v| v);
    }
    let mut runs: Vec<Map<String, Value>> = Vec::new();
    for inline in &tb.inlines {
        match inline {
            Inline::Run(run) => {
                if let Some(r) = run_json(ctx, run, para_disp) {
                    runs.push(r);
                }
            }
            Inline::Atom(a) => {
                if let AtomKind::BareBreak { kind } = &a.kind {
                    let mut o = Map::new();
                    set(&mut o, "text", break_char(*kind));
                    runs.push(o);
                }
                // Math / Other：TS 的公式 run 需要 OMML token 串（M3）
            }
            Inline::Field { id, result } => {
                if let Some(r) = field_run_json(ctx, *id, result, para_disp) {
                    runs.push(r);
                }
            }
        }
    }
    merge_runs(runs)
}

/// TS 的"可转换 HYPERLINK"（`docs/01` §6.2 `onlyXeFields`）：`HYPERLINK "url"`，最多再带一个
/// `\o "tip"`。其他开关（`\l` 文内锚点等）的 HYPERLINK 字段整段走 passthrough。
fn ts_convertible_hyperlink(f: &FieldSpan) -> bool {
    if *f.keyword() != Keyword::Hyperlink {
        return false;
    }
    let mut target = false;
    for t in &f.instr.tokens {
        match t {
            InstrToken::Word(_) | InstrToken::Quoted(_) if !target => target = true,
            InstrToken::Switch { name, .. } if name.eq_ignore_ascii_case(&'o') => {}
            _ => return false,
        }
    }
    target
}

/// 可转换 HYPERLINK 的目标。
fn hyperlink_href(f: &FieldSpan) -> String {
    f.instr.first_argument().unwrap_or_default().to_string()
}

/// TS 的"简单内联字段"（`docs/01` §6.2 `SIMPLE_INLINE_FIELD_RE`）：结果直接当文字显示。
fn is_simple_inline(k: &Keyword) -> bool {
    matches!(
        k,
        Keyword::Date
            | Keyword::Time
            | Keyword::CreateDate
            | Keyword::SaveDate
            | Keyword::NumPages
            | Keyword::FileName
            | Keyword::Author
            | Keyword::Page
    )
}

/// `COMPAT-07`：原子形态字段折成一个 TS run。
///
/// 形状取自语料（TS 的 `extractRuns` 折叠）：XE → `xeTerm` 且 `text` 为空；REF → `refField` +
/// `refInstr`（指令原文，不 trim），`text` 是结果文字；FORMCHECKBOX → `instrField` + `fldBeginXml`
/// （begin run 的原字节），`text` 是 `☐` / `☒`；简单内联字段 → `instrField` + 结果文字。
/// 其余关键字的字段在 TS 里会让整段变成 passthrough，走不到这里；真走到了就退化成结果文字。
///
/// 格式取第一个非空结果 run；没有结果 run（如未选中的复选框）时不带格式键——语料里这些字段的
/// begin run 都没有 `w:rPr`，TS 的输出也没有格式键，等有反例再从 begin run 取。
fn field_run_json(
    ctx: &Ctx<'_>,
    id: FieldId,
    result: &[Inline],
    para: StyleDisp,
) -> Option<Map<String, Value>> {
    let f = ctx.doc.fields.get(id)?;
    let text = inlines_text(result);
    let first = result.iter().find_map(|i| match i {
        Inline::Run(r) if !run_text(r).is_empty() => Some(r),
        _ => None,
    });
    let mut o = first.and_then(|r| run_json(ctx, r, para)).unwrap_or_default();
    match f.keyword() {
        Keyword::Xe => {
            set(&mut o, "text", "");
            set(&mut o, "xeTerm", f.instr.first_argument().unwrap_or_default().to_string());
        }
        Keyword::Ref => {
            set(&mut o, "text", text);
            set(&mut o, "refField", f.instr.first_argument().unwrap_or_default().to_string());
            set(&mut o, "refInstr", f.instr.raw.clone());
        }
        Keyword::FormCheckBox => {
            let checked = matches!(
                read_form_data(ctx.dom, f.ff_data),
                Some(FormData::CheckBox { checked: true, .. })
            );
            set(&mut o, "text", if checked { "☒" } else { "☐" });
            set(&mut o, "instrField", "FORMCHECKBOX");
            let begin = ctx.lex_range(f.form.head());
            set(&mut o, "fldBeginXml", ctx.slice(&begin).to_string());
        }
        k if is_simple_inline(k) => {
            // 没有结果的简单内联字段（PAGE 常见）：TS 放一个空格占位，run 才不是空的
            set(&mut o, "text", if text.is_empty() { " ".to_string() } else { text });
            set(&mut o, "instrField", k.as_str().to_string());
        }
        _ => {
            if text.is_empty() {
                return None;
            }
            set(&mut o, "text", text);
        }
    }
    Some(o)
}

fn break_char(kind: BreakKind) -> &'static str {
    match kind {
        BreakKind::Page => "\u{0C}",
        BreakKind::Column => "\u{0B}",
        BreakKind::TextWrapping => "\n",
    }
}

/// `COMPAT-07`：run 的坐标流文本按 TS 的控制字符折回（结构段贡献 0）。
fn run_text(run: &Run) -> String {
    let mut text = String::new();
    for seg in &run.segments {
        match &seg.kind {
            SegmentKind::Text | SegmentKind::DelText => text.push_str(run.segment_text(seg)),
            SegmentKind::Tab | SegmentKind::PTab { .. } => text.push('\t'),
            SegmentKind::Br { kind, .. } => text.push_str(break_char(*kind)),
            SegmentKind::Cr => text.push('\n'),
            SegmentKind::NoBreakHyphen => text.push('\u{2011}'),
            SegmentKind::Sym { code: Some(_), .. } => text.push_str(run.segment_text(seg)),
            _ => {}
        }
    }
    text
}

/// 一段 inlines 的文本（字段结果用）。
fn inlines_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Run(r) => s.push_str(&run_text(r)),
            Inline::Field { result, .. } => s.push_str(&inlines_text(result)),
            Inline::Atom(a) => {
                if let AtomKind::BareBreak { kind } = &a.kind {
                    s.push_str(break_char(*kind));
                }
            }
        }
    }
    s
}

/// TS `buildRun`。
fn run_json(ctx: &Ctx<'_>, run: &Run, para: StyleDisp) -> Option<Map<String, Value>> {
    let dom = ctx.dom;
    let r = ctx.resolver;
    let text = run_text(run);
    if text.is_empty() {
        return None;
    }
    let mut o = Map::new();
    set(&mut o, "text", text);
    if let Some(link) = &run.link {
        match link {
            crate::model::Link::Hyperlink { target, tooltip, .. } => {
                let mut l = Map::new();
                match target {
                    LinkTarget::External { rel_id, href } => {
                        let href = href.clone().unwrap_or_else(|| {
                            ctx.rels
                                .by_id(rel_id)
                                .map(|rel| match &rel.target {
                                    RelTarget::Internal(u) => u.to_string(),
                                    RelTarget::External(h) => h.clone(),
                                })
                                .unwrap_or_default()
                        });
                        set(&mut l, "href", href);
                        set(&mut l, "rId", rel_id.clone());
                    }
                    LinkTarget::Internal { anchor } => set(&mut l, "href", format!("#{anchor}")),
                    LinkTarget::Unresolved => set(&mut l, "href", ""),
                }
                if let Some(t) = tooltip {
                    set(&mut l, "tooltip", t.clone());
                }
                set(&mut o, "link", Value::Object(l));
            }
            // `FLD-07` 透明字段：目标来自指令（`HYPERLINK "url" \o "tip"` / `\l anchor`）
            crate::model::Link::Field(id) => {
                if let Some(f) = ctx.doc.fields.get(*id)
                    && ts_convertible_hyperlink(f)
                {
                    let mut l = Map::new();
                    set(&mut l, "href", hyperlink_href(f));
                    if let Some(tip) = f.instr.switch('o').filter(|t| !t.is_empty()) {
                        set(&mut l, "tooltip", tip.to_string());
                    }
                    set(&mut o, "link", Value::Object(l));
                }
            }
        }
    }
    let rpr_node = dom.semantic_children(run.node).find(|&n| dom.is(n, w(LocalName::RPr)));
    let props: &RunProps = &run.props;
    let para_rtl = para.rtl;
    let para_vanish = para.vanish;
    let Some(rpr_node) = rpr_node else {
        if para_rtl == Some(true) {
            set(&mut o, "cs", true);
        }
        if para_vanish == Some(true) {
            set(&mut o, "vanish", true);
        }
        revision_ctx(run, &mut o);
        return Some(o);
    };
    set(&mut o, "rawRPr", ctx.slice(&ctx.lex_range(rpr_node)));
    let r_style = props.style.as_deref().filter(|s| *s != "Hyperlink");
    if let Some(s) = r_style {
        set(&mut o, "styleId", s);
    }
    let char_disp = r_style.map(|s| ctx.style_disp(s, StyleType::Character)).unwrap_or_default();
    let vanish_own = if props.spec_vanish == Some(true) { None } else { props.vanish };
    if vanish_own.or(char_disp.vanish).or(para_vanish) == Some(true) {
        set(&mut o, "vanish", true);
    }
    let inherited_rtl = char_disp.rtl.or(para_rtl);
    let cs = props.rtl.or(inherited_rtl) == Some(true);
    if cs {
        set(&mut o, "cs", true);
    }
    if let Some(b) = if cs { props.bold_cs } else { props.bold } {
        set(&mut o, "bold", b);
    }
    if let Some(b) = if cs { props.italic_cs } else { props.italic } {
        set(&mut o, "italic", b);
    }
    if let Some(u) = props.underline.as_ref().and_then(|u| u.val.as_ref()) {
        if *u != Val::Value(UnderlineKind::None) {
            set(&mut o, "underline", true);
        } else {
            set(&mut o, "underline", false);
        }
    }
    if let Some(b) = props.strike {
        set(&mut o, "strike", b);
    }
    if let Some(c) = props.color.as_ref().and_then(|c| r.color(c)) {
        set(&mut o, "color", rgb_hex(c));
    }
    let sz = if cs { &props.size_cs } else { &props.size };
    if let Some(n) = u32_of(sz).filter(|&n| n != 0) {
        set(&mut o, "sizeHalfPoints", n);
    }
    let fonts = r.fonts(props);
    let font = fonts.display().map(str::to_string);
    if let Some(f) = &font {
        set(&mut o, "font", f.clone());
        if fonts.ea_slot_empty && fonts.east_asia.as_deref() == Some(f) {
            set(&mut o, "eaSlotEmpty", true);
        }
    }
    let font_ascii = fonts.display_ascii().map(str::to_string);
    if let Some(a) = &font_ascii {
        set(&mut o, "fontAscii", a.clone());
    }
    let [t_ascii, t_hansi, t_ea, _] = fonts.themed;
    let font_themed = if fonts.east_asia.is_some() {
        t_ea
    } else if fonts.ascii.is_some() {
        t_ascii
    } else {
        t_hansi
    };
    let ascii_themed = if fonts.ascii.is_some() { t_ascii } else { t_hansi };
    if (font_themed && font.is_some()) || (ascii_themed && font_ascii.is_some()) {
        let mut tr = Map::new();
        if font_themed && let Some(f) = &font {
            set(&mut tr, "font", f.clone());
        }
        if ascii_themed && let Some(a) = &font_ascii {
            set(&mut tr, "fontAscii", a.clone());
        }
        set(&mut o, "themeRFonts", Value::Object(tr));
    }
    if let Some(cs_lit) = props.fonts.as_ref().and_then(|f| f.cs.clone()).filter(|s| !s.is_empty())
    {
        set(&mut o, "fontCs", cs_lit);
    }
    if let Some(cs_font) = &fonts.cs {
        set(&mut o, "csFont", cs_font.clone());
    }
    if let Some(b) = props.rtl {
        set(&mut o, "rtl", b);
    }
    if let Some(sp) = twips_int(&props.spacing).filter(|&n| n != 0) {
        set(&mut o, "charSpacingTwips", sp);
    }
    match (props.caps, props.small_caps) {
        (Some(true), _) => set(&mut o, "caps", "all"),
        (_, Some(true)) => set(&mut o, "caps", "small"),
        (Some(false), _) | (_, Some(false)) => set(&mut o, "caps", "none"),
        _ => {}
    }
    if let Some(sc) = u32_of(&props.scale).filter(|&n| n > 0 && n != 100) {
        set(&mut o, "charScalePct", sc);
    }
    if let Some(h) = val_text(&props.highlight, |h| h.as_str()).filter(|h| h != "none") {
        set(&mut o, "highlight", h);
    }
    if props.shading.is_some() {
        let raw = dom
            .semantic_children(rpr_node)
            .find(|&n| dom.is(n, w(LocalName::Shd)))
            .and_then(|n| ctx.attr(n, NsId::W, LocalName::Fill));
        if let Some(fill) = raw
            && fill != "auto"
        {
            set(&mut o, "shading", strip_hash(&fill).to_string());
        }
    }
    if let Some(va) = val_text(&props.vert_align, |v| v.as_str())
        .filter(|v| v == "superscript" || v == "subscript")
    {
        set(&mut o, "vertAlign", va);
    }
    if let Some(em) = val_text(&props.em, |e| e.as_str()).filter(|e| e != "none") {
        set(&mut o, "em", em);
    }
    if let Some((meta, old)) = run.rev.as_ref().and_then(|rv| rv.props_change.as_ref()) {
        let mut change = revision_info(meta);
        let mut old_json = Map::new();
        let ocs = old.rtl.or(inherited_rtl) == Some(true);
        if (if ocs { old.bold_cs } else { old.bold }) == Some(true) {
            set(&mut old_json, "bold", true);
        }
        if (if ocs { old.italic_cs } else { old.italic }) == Some(true) {
            set(&mut old_json, "italic", true);
        }
        if old
            .underline
            .as_ref()
            .and_then(|u| u.val.as_ref())
            .is_some_and(|u| *u != Val::Value(UnderlineKind::None))
        {
            set(&mut old_json, "underline", true);
        }
        if old.strike == Some(true) {
            set(&mut old_json, "strike", true);
        }
        if let Some(c) = old.color.as_ref().and_then(|c| r.color(c)) {
            set(&mut old_json, "color", rgb_hex(c));
        }
        if let Some(n) = u32_of(if ocs { &old.size_cs } else { &old.size }).filter(|&n| n != 0) {
            set(&mut old_json, "sizeHalfPoints", n);
        }
        if let Some(f) = &old.fonts {
            if let Some(ff) =
                f.east_asia.clone().or_else(|| f.ascii.clone()).or_else(|| f.h_ansi.clone())
            {
                set(&mut old_json, "font", ff);
            }
            if let Some(fa) = f.ascii.clone().or_else(|| f.h_ansi.clone()) {
                set(&mut old_json, "fontAscii", fa);
            }
        }
        if let Some(sp) = twips_int(&old.spacing).filter(|&n| n != 0) {
            set(&mut old_json, "charSpacingTwips", sp);
        }
        if let Some(sc) = u32_of(&old.scale).filter(|&n| n > 0 && n != 100) {
            set(&mut old_json, "charScalePct", sc);
        }
        if let Some(h) = val_text(&old.highlight, |h| h.as_str()).filter(|h| h != "none") {
            set(&mut old_json, "highlight", h);
        }
        if let Some(va) = val_text(&old.vert_align, |v| v.as_str())
            .filter(|v| v == "superscript" || v == "subscript")
        {
            set(&mut old_json, "vertAlign", va);
        }
        if let Some(s) = old.style.as_deref().filter(|s| *s != "Hyperlink") {
            set(&mut old_json, "styleId", s);
        }
        if !old_json.is_empty() {
            set(&mut change, "old", Value::Object(old_json));
        }
        set(&mut o, "rPrChange", Value::Object(change));
    }
    revision_ctx(run, &mut o);
    Some(o)
}

fn revision_ctx(run: &Run, o: &mut Map<String, Value>) {
    if let Some(rv) = &run.rev {
        if let Some(m) = &rv.ins {
            set(o, "ins", Value::Object(revision_info(m)));
        }
        if let Some(m) = &rv.del {
            set(o, "del", Value::Object(revision_info(m)));
        }
    }
}

/// TS `mergeRuns` / `sameStyle`。
fn merge_runs(runs: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for run in runs {
        if let Some(prev) = out.last_mut()
            && same_style(prev, &run)
        {
            let t = format!(
                "{}{}",
                prev.get("text").and_then(Value::as_str).unwrap_or(""),
                run.get("text").and_then(Value::as_str).unwrap_or("")
            );
            set(prev, "text", t);
        } else {
            out.push(run);
        }
    }
    out
}

fn same_style(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    for atomic in ["noteRef", "xeTerm", "refField", "instrField", "math", "ruby", "image"] {
        if a.contains_key(atomic) || b.contains_key(atomic) {
            return false;
        }
    }
    let s = |m: &Map<String, Value>, k: &str| {
        m.get(k).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let truthy =
        |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
    let eq_opt = |k: &str| a.get(k) == b.get(k);
    let link = |m: &Map<String, Value>, k: &str| {
        m.get("link").and_then(|l| l.get(k)).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let comments = |m: &Map<String, Value>| {
        m.get("commentIds")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","))
            .unwrap_or_default()
    };
    let rev = |m: &Map<String, Value>, k: &str| {
        m.get(k).map(|r| (s_of(r, "author"), s_of(r, "date"), s_of(r, "id")))
    };
    s(a, "rawRPr") == s(b, "rawRPr")
        && eq_opt("styleId")
        && truthy(a, "cs") == truthy(b, "cs")
        && truthy(a, "bold") == truthy(b, "bold")
        && truthy(a, "italic") == truthy(b, "italic")
        && truthy(a, "underline") == truthy(b, "underline")
        && truthy(a, "strike") == truthy(b, "strike")
        && eq_opt("color")
        && eq_opt("sizeHalfPoints")
        && eq_opt("font")
        && eq_opt("fontAscii")
        && eq_opt("csFont")
        && eq_opt("highlight")
        && eq_opt("vertAlign")
        && link(a, "href") == link(b, "href")
        && link(a, "rId") == link(b, "rId")
        && comments(a) == comments(b)
        && rev(a, "ins") == rev(b, "ins")
        && rev(a, "del") == rev(b, "del")
}

fn s_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}
