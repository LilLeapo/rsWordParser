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

/// sdt 直接内容子元素：`w:sdtContent` 的子元素，嵌套 sdt 透明（TS `splitSdtParts` 跳过 sdt/sdtContent 标签）。
fn sdt_content_children(ctx: &Ctx<'_>, sdt: NodeId, out: &mut Vec<NodeId>) {
    let dom = ctx.dom;
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
                            sdt_content_children(ctx, cc, out);
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
        Some(Block::Text(tb)) => text_block(ctx, tb, o),
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
            Inline::Field { .. } => {}
        }
    }
    merge_runs(runs)
}

fn break_char(kind: BreakKind) -> &'static str {
    match kind {
        BreakKind::Page => "\u{0C}",
        BreakKind::Column => "\u{0B}",
        BreakKind::TextWrapping => "\n",
    }
}

/// TS `buildRun`。
fn run_json(ctx: &Ctx<'_>, run: &Run, para: StyleDisp) -> Option<Map<String, Value>> {
    let dom = ctx.dom;
    let r = ctx.resolver;
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
            crate::model::Link::Field(_) => {}
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
