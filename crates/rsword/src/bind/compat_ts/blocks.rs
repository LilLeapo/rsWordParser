//! `blocks[]` / `extras.elements[]`（`COMPAT-03/04/07`）：body 顶层元素序列（sdt 拆分）、块类型复现、
//! `ParaFormat`、Run 映射与合并。每段规则注明对应的 TS 函数（`docs/01` §6）。
//!
//! M1 范围：文本块完整；表格 / 图片 / 字段 / 公式等保护块只给 `type/label/previewText` 骨架。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use serde_json::{Map, Value, json};

use super::chart;
use super::decl::{
    NumberingOut, auto_space, i32_of, jc_align, list_kind, parse_int, strip_hash, tab_stops,
    u32_of, val_text,
};
use super::diagram;
use super::image;
use super::math;
use super::media::MediaMap;
use super::textbox;
use super::utf16::Utf16Index;
use crate::bind::native::json::set_some;
use crate::model::Sections;
use crate::model::vml::vml_display;
use crate::model::{
    AtomKind, Block, BreakKind, Display, Document, Inline, LinkTarget, ProtectedKind, Revision,
    RevisionMeta, Run, SdtControl, SdtInfo, SegmentKind, StyleType, TextBlock, TextKind,
};
use crate::package::{PartId, RelTarget, Rels};
use crate::resolve::{Resolver, rgb_hex};
use crate::semantic::props::{ParaProps, RunProps, UnderlineKind, Val};
use crate::span::field::{FieldId, FieldSpan, FormData, InstrToken, Keyword, read_form_data};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 样式的"显示"信息（TS `StyleInfo.display` 里 run 映射用到的三项）。
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct StyleDisp {
    rtl: Option<bool>,
    vanish: Option<bool>,
    auto_space: Option<bool>,
}

/// 另一个 part 的投影上下文来源（外部文本框 part 等）。
pub(super) struct AuxProj<'a> {
    pub dom: &'a Dom,
    pub rels: &'a Rels,
    pub idx: &'a Utf16Index,
    pub flows: &'a crate::model::AuxFlows,
    pub media: &'a MediaMap,
}

/// `PartId` → 那个 part 的投影来源。
pub(super) type AuxProjMap<'a> = std::collections::BTreeMap<PartId, AuxProj<'a>>;

pub(super) struct Ctx<'a> {
    pub dom: &'a Dom,
    pub doc: &'a Document,
    /// **当前 part** 的字段索引（`FLD-02`）。页眉页脚投影时是那个 part 的，不是主 part 的
    /// ——`FieldId` 只在自己的索引里有意义，用错了会读到另一个 part 的字段（任务 5.4）。
    pub fields: &'a crate::span::field::FieldIndex,
    /// **当前 part** 的范围索引（`SPAN-04`）。`commentIds` 要用。
    pub spans: &'a crate::span::SpanIndex,
    pub resolver: &'a Resolver<'a>,
    pub idx: &'a Utf16Index,
    pub rels: &'a Rels,
    pub numbering: &'a NumberingOut,
    /// 主 part 的媒体预取表（`bind::compat_ts::media`）。
    pub media: &'a MediaMap,
    /// 主 part 的节页面几何（锚定绘图定位要用）。
    pub sections: Sections,
    /// 文档里第一处分页的字节位置（`None` = 整篇都在首页）。
    first_page_break: Option<u32>,
    /// 正文引用的其他 part（外部文本框 part）：[`Ctx::switch`] 用。
    aux: &'a AuxProjMap<'a>,
    /// 主 part 的图表关系表（任务 6.2）；页眉页脚 / 外部 part 的上下文里是空表——关系 id 是按 part 的。
    pub charts: &'a chart::ChartMap<'a>,
    /// 主 part 的 SmartArt 关系表（任务 6.3），同上。
    pub diagrams: &'a diagram::DiagramMap<'a>,
    disp_cache: RefCell<HashMap<(String, StyleType), StyleDisp>>,
}

pub(super) fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

impl<'a> Ctx<'a> {
    /// TS `noteNumbers`：正文条目按 part 顺序 1..n；查不到写 `*`。
    pub fn note_number(&self, endnote: bool, id: Option<&str>) -> String {
        let notes = if endnote { &self.doc.endnotes } else { &self.doc.footnotes };
        let Some(id) = id else { return "*".to_string() };
        notes
            .normal()
            .position(|n| n.id == id)
            .map_or_else(|| "*".to_string(), |i| (i + 1).to_string())
    }

    pub fn new(
        dom: &'a Dom,
        doc: &'a Document,
        resolver: &'a Resolver<'a>,
        idx: &'a Utf16Index,
        rels: &'a Rels,
        numbering: &'a NumberingOut,
        media: &'a MediaMap,
    ) -> Ctx<'a> {
        // 节几何取自 `doc.sections`（任务 5.2 起是唯一来源）；`Document::build_main` 之类没建节的
        // 路径退回直接扫 DOM
        let sections = if doc.sections.is_empty() {
            Sections::build(dom)
        } else {
            Sections::from_sections(&doc.sections)
        };
        let first_page_break = first_page_break_at(dom.src());
        Ctx {
            dom,
            doc,
            fields: &doc.fields,
            spans: &doc.spans,
            resolver,
            idx,
            rels,
            numbering,
            media,
            sections,
            first_page_break,
            aux: empty_aux(),
            charts: chart::empty_charts(),
            diagrams: diagram::empty_diagrams(),
            disp_cache: RefCell::new(HashMap::new()),
        }
    }

    /// 挂上"正文引用的其他 part"表（`parsed_doc_of` 建）。
    pub(super) fn with_aux(mut self, aux: &'a AuxProjMap<'a>) -> Ctx<'a> {
        self.aux = aux;
        self
    }

    /// 挂上主 part 的图表关系表（`parsed_doc_of` 建）。
    pub(super) fn with_charts(mut self, charts: &'a chart::ChartMap<'a>) -> Ctx<'a> {
        self.charts = charts;
        self
    }

    /// 挂上主 part 的 SmartArt 关系表（`parsed_doc_of` 建）。
    pub(super) fn with_diagrams(mut self, diagrams: &'a diagram::DiagramMap<'a>) -> Ctx<'a> {
        self.diagrams = diagrams;
        self
    }

    /// 切到另一个 part 的投影上下文：`dom` / `rels` / 媒体表 / UTF-16 索引与两个索引全换掉，
    /// 声明模型（样式 / 编号 / 主题）与 `doc` 共用。
    ///
    /// 外部文本框 part（`wps:txbx/@r:txbx`）的块的 `NodeId` 属于那个 part，投影必须换 DOM，
    /// 否则 `rawRPr` 之类的原字节切片会切到主 part 上去（任务 5.4d）。
    pub(super) fn switch(&self, part: PartId) -> Option<Ctx<'a>> {
        let a = self.aux.get(&part)?;
        Some(Ctx {
            dom: a.dom,
            doc: self.doc,
            fields: &a.flows.fields,
            spans: &a.flows.spans,
            resolver: self.resolver,
            idx: a.idx,
            rels: a.rels,
            numbering: self.numbering,
            media: a.media,
            sections: self.sections.clone(),
            first_page_break: None,
            aux: self.aux,
            charts: chart::empty_charts(),
            diagrams: diagram::empty_diagrams(),
            disp_cache: RefCell::new(HashMap::new()),
        })
    }

    /// 换成另一个 part 的索引：`dom` 是那个 part 的 DOM 时，字段与范围索引也必须跟着换
    /// （页眉页脚 / 注释 / 批注的投影，任务 5.4）。
    pub(super) fn for_aux(mut self, aux: &'a crate::model::AuxFlows) -> Ctx<'a> {
        self.fields = &aux.fields;
        self.spans = &aux.spans;
        self
    }

    pub(super) fn slice(&self, r: &Range<u32>) -> &'a str {
        self.dom.lex_str(r)
    }

    /// 一个节点的原字节（`COMPAT-04`：TS 的各种 `xml` 字段都是原文切片）。
    pub(super) fn node_xml(&self, node: NodeId) -> &'a str {
        self.slice(&self.lex_range(node))
    }

    /// 管辖某个节点的节几何。
    pub(super) fn section_at(&self, node: NodeId) -> Option<&crate::model::SectionGeom> {
        let start = self.lex_range(node).start;
        self.sections.at(start)
    }

    /// TS `opts.firstPage`：这个块的锚定绘图能不能按页面原始坐标钉住。
    ///
    /// 要求块**不是**第一个——首个块的锚点本来就在正文顶上，按段落原点算已经准了——
    /// 且块起点在首个分页之前。
    pub(super) fn first_page(&self, node: NodeId, docx_index: usize) -> bool {
        docx_index > 0 && self.first_page_break.is_none_or(|b| self.lex_range(node).start < b)
    }

    /// `w:instrText` 的文本内容。
    fn plain_instr(&self, node: NodeId) -> String {
        let mut s = String::new();
        for c in self.dom.semantic_children(node) {
            if let Some(t) = self.dom.text(c) {
                s.push_str(&t);
            }
        }
        s
    }

    fn u16(&self, byte: u32) -> u32 {
        self.idx.at(self.dom.src(), byte)
    }

    pub(super) fn lex_range(&self, node: NodeId) -> Range<u32> {
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
    pub(super) fn plain_text(&self, node: NodeId) -> String {
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

pub(super) fn revision_info(m: &RevisionMeta) -> Map<String, Value> {
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
    image::normalize_z_orders(&mut blocks);
    (elements, blocks)
}

/// TS `ctx.firstPageBreakAt`：第一处分页的字节位置。显式分页符、`w:pageBreakBefore`、
/// Word 记录的渲染分页提示，以及第一个节的结束，取最靠前的那个。
///
/// 误报只会把「钉页」关掉，是保守方向；漏报会把第二页的封面图钉到第一页上。
fn first_page_break_at(src: &str) -> Option<u32> {
    /// `open` 开头的标签里，第一个满足 `ok` 的标签起点。
    fn scan(src: &str, open: &str, ok: impl Fn(&str) -> bool) -> Option<usize> {
        let mut from = 0;
        while let Some(rel) = src[from..].find(open) {
            let at = from + rel;
            let end = src[at..].find('>').map_or(src.len(), |e| at + e + 1);
            if ok(&src[at..end]) {
                return Some(at);
            }
            from = end;
        }
        None
    }
    [
        scan(src, "<w:br ", |t| t.contains("w:type=\"page\"")),
        scan(src, "<w:pageBreakBefore", |t| t.ends_with("/>")),
        scan(src, "<w:lastRenderedPageBreak", |t| t.ends_with("/>")),
        src.find("</w:sectPr>"),
    ]
    .into_iter()
    .flatten()
    .min()
    .map(|i| i as u32)
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

/// TS `sdtMeta`：从模型的 [`SdtInfo`]（`MOD-08`）投影出 TS 的三个字段。
/// TS 的 `controlType` 只有四值，别的控件种类一律 `text`（`COMPAT-02`）。
fn sdt_meta(ctx: &Ctx<'_>, sdt: NodeId) -> (String, String, &'static str) {
    let info = SdtInfo::read(ctx.dom, sdt);
    let control = match info.control {
        SdtControl::Date => "date",
        SdtControl::DropDownList | SdtControl::ComboBox => "dropdown",
        SdtControl::Checkbox => "checkbox",
        _ => "text",
    };
    (info.alias.unwrap_or_default(), info.tag.unwrap_or_default(), control)
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
            o = table_block(ctx, child, o, by_node.get(&child).copied());
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

fn table_block(
    ctx: &Ctx<'_>,
    tbl: NodeId,
    mut o: Map<String, Value>,
    block: Option<&Block>,
) -> Map<String, Value> {
    let (rows, cols) = ts_table_summary(ctx.slice(&ctx.lex_range(tbl)));
    set(&mut o, "type", "table");
    set(&mut o, "label", format!("Table {rows}×{cols}"));
    set(&mut o, "previewText", ctx.plain_text(tbl).chars().take(120).collect::<String>());
    let docx_index = o.get("docxIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
    if let Some(Block::Table(t)) = block
        && let Some(model) = super::table::table_json(ctx, t, 1, docx_index)
    {
        set(&mut o, "table", model);
    }
    o
}

/// TS `tableSummary`：`label` 的行列数是在**原字节**上数出来的——行数是整段 XML 里 `<w:tr` 的个数
/// （嵌套表的行也算），列数是"第一行"里 `<w:tc` 的个数，而那个"第一行"止于**第一个** `</w:tr>`，
/// 于是首格里的嵌套表会把自己的行尾借给外层（`table-display__003`：外层 1 行 2 格 + 嵌套 1 行 2 格
/// → `Table 2×3`）。这是 TS 正则切片的产物，照数才对得上；`table` 模型走的是真正的行列。
fn ts_table_summary(xml: &str) -> (usize, usize) {
    fn count(hay: &str, tag: &str) -> usize {
        let mut n = 0;
        let mut from = 0;
        while let Some(i) = hay[from..].find(tag) {
            let at = from + i + tag.len();
            if hay[at..].starts_with(|c: char| c == '>' || c.is_ascii_whitespace()) {
                n += 1;
            }
            from = at;
        }
        n
    }
    let rows = count(xml, "<w:tr");
    let first_row = xml
        .find("<w:tr")
        .and_then(|start| xml[start..].find("</w:tr>").map(|end| &xml[start..start + end + 7]))
        .unwrap_or("");
    (rows, count(first_row, "<w:tc"))
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
        LocalName::Tbl => table_block(ctx, node, o, by_node.get(&node).copied()),
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

/// 嵌入对象的显示信息该不该输出（TS `onlyOleFields`，`docs/01` §6.2.2）。
///
/// TS 的决策树里字段分支在 `w:object` 之前：段落里只要还有别的字段，这一段就归字段管
/// （`smartart-ole__013` 的 `EMBED` 后面跟了个 `TOC`，标签是 `Field (EMBED)`）。只有当段落
/// 没有字段、或者所有指令都是 `EMBED` / `LINK` 时，才走嵌入对象这条路。
fn ole_display_applies(ctx: &Ctx<'_>, p: NodeId) -> bool {
    let dom = ctx.dom;
    let mut instrs = 0usize;
    let mut all_ole = true;
    for n in dom.semantic_descendants(p) {
        // 文本框里的字段不算（TS 的 `fieldDetect` 先剥掉文本框）
        if dom.is(n, w(LocalName::TxbxContent)) {
            continue;
        }
        if dom.is(n, w(LocalName::FldSimple)) {
            return false;
        }
        if dom.is(n, w(LocalName::InstrText)) {
            instrs += 1;
            let text = ctx.plain_instr(n);
            let head = text.trim_start();
            if !(head.starts_with("EMBED") || head.starts_with("LINK")) {
                all_ole = false;
            }
        }
    }
    instrs == 0 || all_ole
}

/// 段落里每个 `w:object` 的 `v:imagedata` 预览图都能解析。
fn ole_previews_resolve(ctx: &Ctx<'_>, tb: &TextBlock) -> bool {
    tb.facts.objects.iter().all(|&n| {
        vml_display(ctx.dom, n)
            .image()
            .and_then(|s| s.imagedata.as_deref())
            .is_some_and(|r| ctx.media.get(r).is_some())
    })
}

/// 绘图分支（`spec/15` 4.6），落空就是普通文本段落。
fn drawing_or_text_block(
    ctx: &Ctx<'_>,
    p: NodeId,
    tb: &TextBlock,
    o: Map<String, Value>,
) -> Map<String, Value> {
    // 段落里既有文字又有 `w:object`，但预览图解析不出来：TS 退成 `Embedded object`
    // 只读块（否则整段会只画一张画不出来的预览图，把文字吃掉）。预览图都解析得出来时
    // 留在带图的文本段落路径上，`w:object` 的原字节照样往返（`docs/01` §6.2.6）。
    if !tb.facts.objects.is_empty() && !ole_previews_resolve(ctx, tb) {
        let mut o = passthrough(o, "Embedded object");
        set(&mut o, "previewText", ctx.plain_text(p));
        let v = vml_display(ctx.dom, tb.facts.objects[0]);
        image::ole_display(ctx, p, &v, &mut o);
        return o;
    }
    // 文本框 / 绘图对象的分类整个在投影层，模型里这仍是可编辑段落。
    match textbox::drawing_block(ctx, p, tb, o.clone()) {
        Some(o) => o,
        None => text_block(ctx, tb, o),
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
        // TS `buildBlock` 规则 2 / 3：字段段落与 TOC 行是只读的 passthrough，不出 runs / format。
        // 顺序照 TS 的决策树：**字段在绘图之前**——文本框里的字段不算数（`para_fields` /
        // `has_stray_field_chars` 都只看宿主段落自己的 inline），所以带字段的文本框段落照样
        // 走得到下面的绘图分支（`vml-textbox__007`）。
        Some(Block::Text(tb)) => match ts_field_passthrough(ctx, tb) {
            // TS：`{ EMBED … }` / `{ LINK … }` 包着 `w:object`、段落里没有别的字段 → 走嵌入对象那条路，
            // 预览图与声明尺寸留住，而不是一个光秃秃的 `Field (EMBED)` 芯片（任务 6.4，`m6-ole__007`）。
            Some(_)
                if !tb.facts.objects.is_empty()
                    && has_field_chars(ctx, p)
                    && ole_display_applies(ctx, p) =>
            {
                let mut o = passthrough(o, "Embedded object");
                set(&mut o, "previewText", ctx.plain_text(p));
                let v = vml_display(ctx.dom, tb.facts.objects[0]);
                image::ole_display(ctx, p, &v, &mut o);
                o
            }
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
            None => drawing_or_text_block(ctx, p, tb, o),
        },
        Some(Block::Protected(pb)) => match &pb.kind {
            ProtectedKind::Invisible => {
                // R08（样式链 vanish）→ TS `Hidden paragraph`；R16（只有画不出来的 VML：仅 shapetype /
                // 隐藏形状）→ TS `isInvisibleVmlPict` 的 `Drawing object`（`wordart-vml__006`，任务 6.9）
                let style_vanish = para_style_id(ctx, p)
                    .is_some_and(|s| ctx.style_disp(&s, StyleType::Paragraph).vanish == Some(true));
                let has_pict = ctx.dom.descendants(p).any(|n| ctx.dom.is(n, w(LocalName::Pict)));
                let label =
                    if !style_vanish && has_pict { "Drawing object" } else { "Hidden paragraph" };
                let mut o = passthrough(o, label);
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
            // `COMPAT-03` 图表块（任务 6.2）：`chartDisplay` 与 `previewText` 只在 part 解析出 display 时有；
            // 没有的块只剩 `label: "Chart"`（`chart::chart_block`）。TS 对图表块不取段落文字。
            ProtectedKind::Chart => {
                let mut o = passthrough(o, "Chart");
                chart::chart_block(ctx, pb, &mut o);
                o
            }
            // `COMPAT-03` SmartArt 与画布（任务 6.3）：R13 / R14 都是 `SmartArt` 种类，按绘图分——段落里
            // 有 `lc:lockedCanvas` 的走画布（TS 的 `<lc:lockedCanvas` 分支，label `Drawing object`），
            // 否则是图示（`previewText` / `diagramDisplay` / 同段其他绘图的 `textboxes`）。
            ProtectedKind::SmartArt => {
                let canvas = std::iter::once(pb.display.as_ref())
                    .flatten()
                    .chain(pb.siblings.iter())
                    .filter_map(Display::as_drawing)
                    .find(|d| d.canvas.is_some());
                match canvas {
                    Some(d) => {
                        let mut o = passthrough(o, "Drawing object");
                        diagram::canvas_block(ctx, d, &mut o);
                        o
                    }
                    None => {
                        let mut o = passthrough(o, "SmartArt");
                        diagram::smart_art_block(ctx, p, pb, &mut o);
                        o
                    }
                }
            }
            // TS 的决策树里字段分支在 `w:object` 之前：段落里除 `EMBED` / `LINK` 外还有别的字段，
            // 哪怕一个字都没有（R18 归了 `Ole`），也是字段芯片（任务 6.4；有字的版本走上面的文本分支）
            ProtectedKind::Ole if has_field_chars(ctx, p) && !ole_display_applies(ctx, p) => {
                let style = para_style_id(ctx, p);
                let toc = style.as_deref().and_then(crate::model::facts::toc_level_of_id);
                let mut o = passthrough(o, &field_label(ctx, p));
                set(&mut o, "previewText", ctx.plain_text(p));
                set_some!(&mut o, "styleId" => style, "fieldDisplay" => field_display(ctx, p, toc));
                o
            }
            // `COMPAT-07` 公式块（任务 6.5）：`previewText` 是 token 拼接，不是段落文字
            ProtectedKind::Equation => {
                let mut o = passthrough(o, "Equation");
                math::formula_block(ctx, pb, &mut o);
                o
            }
            kind => {
                let label = match kind {
                    ProtectedKind::Ole => "Embedded object",
                    ProtectedKind::Rule => "Drawing object",
                    _ => "Paragraph",
                };
                let mut o = passthrough(o, label);
                // TS 的 `w:pict` 细横线分支只出 `decorative` / `rule*`，没有 `previewText`（`smartart-ole__005`）
                if !matches!(kind, ProtectedKind::Rule) {
                    set(&mut o, "previewText", ctx.plain_text(p));
                }
                let vml = pb.display.as_ref().and_then(Display::as_vml);
                match (kind, vml) {
                    // HTML `<hr>` 导入的细横线：Word 按声明高度画一条线，画成绘图对象芯片会
                    // 既画错又白吃掉一行版面（`docs/01` §6.2.6）。
                    (ProtectedKind::Rule, Some(v)) => image::vml_rule(v, &mut o),
                    (ProtectedKind::Ole, Some(v)) if ole_display_applies(ctx, p) => {
                        image::ole_display(ctx, p, v, &mut o);
                    }
                    _ => {}
                }
                o
            }
        },
        Some(Block::Image(b)) => {
            let mut o = o;
            let drawing = b.display.as_ref().and_then(Display::as_drawing);
            let media = drawing
                .and_then(|d| d.picture())
                .and_then(|p| ctx.media.pick(p.embed.as_deref(), p.link.as_deref()));
            match media {
                Some(m) => {
                    set(&mut o, "type", "image");
                    set(&mut o, "label", "Image");
                    set(&mut o, "imageDataUrl", m.url.clone());
                }
                // 媒体解析不出来：TS 退成只读的 `Image` 块并标 brokenImage，预览文字取 docPr。
                // 只对 DrawingML 图片这么做——VML 图片（`w:pict`）的显示模型在 4.5，
                // 那之前它没有 `display`，不能据此断定媒体坏了。
                None if drawing.is_some_and(|d| d.picture().is_some()) => {
                    o = passthrough(o, "Image");
                    set(&mut o, "brokenImage", true);
                    let preview = drawing
                        .map(|d| &d.doc_pr)
                        .and_then(|d| d.descr.clone().or_else(|| d.name.clone()))
                        .unwrap_or_default();
                    set(&mut o, "previewText", preview);
                }
                None => set(&mut o, "type", "image"),
            }
            if let Some(d) = drawing {
                image::image_meta(ctx, Some(p), d, &mut o);
            }
            // TS `applyProtectedLeadingBreaks`：图片块吞掉了段落的 run，写在图形之前的分页 run 会无声
            // 消失，而 Word 在那里翻页——等价地记成段落级 `pageBreakBefore`（只对 `image` 块，任务 6.9）
            if o.get("type").and_then(Value::as_str) == Some("image") && leading_page_break(ctx, p)
            {
                set(&mut o, "format", json!({ "pageBreakBefore": true }));
            }
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
    tb.facts.fields.iter().filter_map(|&id| ctx.fields.get(id)).collect()
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
            }) && ctx.fields.field_of(r.node).is_none()
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
            // `w:fldSimple/@w:instr`（TS `fieldLabel` 的第二个来源，`vml-textbox__008`）
            LocalName::FldSimple if pending.trim().is_empty() => {
                if let Some(instr) = ctx.attr(n, NsId::W, LocalName::Instr)
                    && !instr.trim().is_empty()
                {
                    return Some(instr_keyword(&instr));
                }
            }
            _ => {}
        }
    }
    (!pending.trim().is_empty()).then(|| instr_keyword(&pending))
}

/// `w14:textFill` → 6 位 hex（TS `w14TextFillHex`）：`w14:solidFill` 的颜色，或 `w14:gradFill/w14:gsLst` 各停靠点
/// 的等权平均；颜色语法与 DrawingML 相同（`srgbClr` / `schemeClr` + `lumMod` / `lumOff` / `shade` / `tint` / `satMod`）。
fn text_fill_hex(ctx: &Ctx<'_>, tf: NodeId) -> Option<String> {
    use crate::resolve::drawingml::{average, color_in_ns, hex};
    let dom = ctx.dom;
    let palette = ctx.resolver.palette();
    let w14 = |l: LocalName| QName::new(NsId::W14, l);
    if let Some(solid) = dom.semantic_children(tf).find(|&c| dom.is(c, w14(LocalName::SolidFill))) {
        return color_in_ns(dom, solid, NsId::W14).and_then(|c| c.to_rgb(palette)).map(hex);
    }
    let grad = dom.semantic_children(tf).find(|&c| dom.is(c, w14(LocalName::GradFill)))?;
    let gs_lst = dom.semantic_children(grad).find(|&c| dom.is(c, w14(LocalName::GsLst)))?;
    let stops: Vec<_> = dom
        .semantic_children(gs_lst)
        .filter(|&g| dom.is(g, w14(LocalName::Gs)))
        .filter_map(|g| color_in_ns(dom, g, NsId::W14).and_then(|c| c.to_rgb(palette)))
        .collect();
    average(&stops).map(hex)
}

/// TS `hostPageBreak`：宿主段落自己（不算文本框内容）有没有分页 `w:br`。
pub(super) fn host_page_break(ctx: &Ctx<'_>, p: NodeId) -> bool {
    let dom = ctx.dom;
    let mut stack = vec![p];
    while let Some(n) = stack.pop() {
        if dom.is(n, w(LocalName::TxbxContent)) {
            continue;
        }
        if dom.is(n, w(LocalName::Br))
            && ctx.attr(n, NsId::W, LocalName::Type).as_deref() == Some("page")
        {
            return true;
        }
        stack.extend(dom.children(n).iter().rev());
    }
    false
}

/// TS `applyProtectedLeadingBreaks` 的判据：第一个图形（`w:drawing` / `w:pict` / `w:object`）之前有分页
/// `w:br`，且分页之前没有可见文字。
fn leading_page_break(ctx: &Ctx<'_>, p: NodeId) -> bool {
    let dom = ctx.dom;
    let mut saw_break = false;
    let mut stack = vec![p];
    while let Some(n) = stack.pop() {
        let Some(name) = dom.name(n) else {
            if !saw_break && dom.text(n).is_some_and(|t| !t.trim().is_empty()) {
                // 文字节点：分页之前有字就不算（`w:t` 之外的文字，如 instrText，TS 也不数——只看 w:t）
            }
            continue;
        };
        if name.ns == NsId::W {
            match name.local {
                LocalName::Drawing | LocalName::Pict | LocalName::Object => return saw_break,
                LocalName::Br
                    if ctx.attr(n, NsId::W, LocalName::Type).as_deref() == Some("page") =>
                {
                    saw_break = true;
                    continue;
                }
                LocalName::T if !saw_break => {
                    if dom
                        .semantic_children(n)
                        .any(|t| dom.text(t).is_some_and(|s| !s.trim().is_empty()))
                    {
                        return false;
                    }
                    continue;
                }
                _ => {}
            }
        }
        stack.extend(dom.children(n).iter().rev());
    }
    false
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
        || !f.objects.is_empty()
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
    let mut runs = runs_json(ctx, tb);
    image::resolve_run_overlap(&mut runs);
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

pub(super) fn para_format(
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
/// 文本框段落用的扁平 `format`（TS `txbxContentParas` 把 `extractParaFormat` 的字段直接摊在
/// 段落对象上，而不是包进 `format`）。
pub(super) fn para_format_json(ctx: &Ctx<'_>, tb: &TextBlock) -> Option<Value> {
    let ppr = ctx.dom.semantic_children(tb.node).find(|&n| ctx.dom.is(n, w(LocalName::PPr)));
    para_format_of_props(ctx, &tb.props, ppr, tb.node, true)
}

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
pub(super) fn empty_para_size(ctx: &Ctx<'_>, p: NodeId, ppr: Option<NodeId>) -> Option<i64> {
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
pub(super) fn empty_para_font(ctx: &Ctx<'_>, p: NodeId, ppr: Option<NodeId>) -> Option<String> {
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

/// TS `strayParaRuns`：形状之外的那些 run。
///
/// `with_images` 打开时保留**随文**图片的 run；锚定的绘图不给图——它已经作为框画出来了，
/// 再当 run 里的图给一遍就是画两遍。关掉时一张图都不给。空 run 一律丢掉。
pub(super) fn stray_runs_json(
    ctx: &Ctx<'_>,
    tb: &TextBlock,
    with_images: bool,
) -> Vec<Map<String, Value>> {
    let anchored = |run: &Run| {
        run.segments.iter().any(|seg| {
            seg.display.as_ref().and_then(Display::as_drawing).is_some_and(|d| d.anchor.is_some())
        })
    };
    let mut out = Vec::new();
    for inline in &tb.inlines {
        let Inline::Run(run) = inline else { continue };
        for mut r in run_jsons(ctx, run, para_disp(ctx, tb)) {
            if !with_images || anchored(run) {
                r.remove("image");
            }
            let has_text = r.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty());
            if has_text || r.contains_key("image") {
                out.push(r);
            }
        }
    }
    merge_runs(out)
}

/// 段落级的显示属性（`vanish` / `rtl` / 自动间距），run 投影要用。
pub(super) fn para_disp(ctx: &Ctx<'_>, tb: &TextBlock) -> StyleDisp {
    let mut d =
        tb.style_id.as_deref().map(|s| ctx.style_disp(s, StyleType::Paragraph)).unwrap_or_default();
    if tb.style_id.is_none()
        && let Some(def) = ctx.resolver.default_style(StyleType::Paragraph).and_then(|s| s.id())
    {
        // TS `defaultParaVanish`：只有 vanish 走默认样式
        d.vanish = ctx.style_disp(def, StyleType::Paragraph).vanish.filter(|&v| v);
    }
    d
}

/// 一串 inline → TS run 列表，可选带不带图（`COMPAT-05` 的页眉页脚投影用：TS 的
/// `extractRuns` 在非表格段落上不带图，在单元格里带图但剥掉锚定图）。
///
/// 与 [`runs_json`] 的区别只有两点：不做字段折叠（页眉页脚的字段由调用方按 `COMPAT-05` 改写），
/// 以及 `with_images` 可关。空文字且没有图的 run 一律丢掉。
pub(super) fn inline_runs_json(
    ctx: &Ctx<'_>,
    inlines: &[Inline],
    para: StyleDisp,
    with_images: bool,
) -> Vec<Map<String, Value>> {
    let anchored = |run: &Run| {
        run.segments.iter().any(|seg| {
            seg.display.as_ref().and_then(Display::as_drawing).is_some_and(|d| d.anchor.is_some())
        })
    };
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                for mut r in run_jsons(ctx, run, para) {
                    if !with_images || anchored(run) {
                        r.remove("image");
                    }
                    let has_text =
                        r.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty());
                    if has_text || r.contains_key("image") {
                        out.push(r);
                    }
                }
            }
            Inline::Atom(a) => {
                if let AtomKind::BareBreak { kind } = &a.kind {
                    let mut o = Map::new();
                    set(&mut o, "text", break_char(*kind));
                    out.push(o);
                }
            }
            // 字段：调用方决定（页眉页脚按 `COMPAT-05` 改写，正文走 `runs_json`）
            Inline::Field { .. } => {}
        }
    }
    out
}

pub(super) fn runs_json(ctx: &Ctx<'_>, tb: &TextBlock) -> Vec<Map<String, Value>> {
    let para_disp = para_disp(ctx, tb);
    let mut runs: Vec<Map<String, Value>> = Vec::new();
    for inline in &tb.inlines {
        match inline {
            Inline::Run(run) => runs.extend(run_jsons(ctx, run, para_disp)),
            Inline::Atom(a) => match &a.kind {
                AtomKind::BareBreak { kind } => {
                    let mut o = Map::new();
                    set(&mut o, "text", break_char(*kind));
                    runs.push(o);
                }
                // 文字夹公式（R19）：每个 `m:oMath` 是一个原子 run（任务 6.5）
                AtomKind::Math => runs.push(math::math_run(ctx, a.node)),
                _ => {}
            },
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
    let f = ctx.fields.get(id)?;
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
            // TS `pushRun({ text: fieldCached || ' ', instrField: fieldInstr.trim() })`：只有文字与**整条**指令
            // （含开关，`DATE  \* MERGEFORMAT`），不带结果 run 的格式键（真实 Word 的 `fields-toc`）；
            // 没有结果的简单内联字段（PAGE 常见）放一个空格占位，run 才不是空的
            let comments = o.remove("commentIds");
            o = Map::new();
            set(&mut o, "text", if text.is_empty() { " ".to_string() } else { text });
            set(&mut o, "instrField", f.instr.raw.trim().to_string());
            if let Some(c) = comments {
                o.insert("commentIds".into(), c);
            }
            let _ = k;
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

/// `rawRPr` 的字节；`drop_fonts` 时把 `w:rFonts` 子元素从原字节里剪掉（`RES-05`：解码过的符号
/// run 在 TS 那边是 `<w:rPr></w:rPr>`）。按节点区间剪，不做字符串匹配。
fn raw_rpr(ctx: &Ctx<'_>, rpr: NodeId, drop_fonts: bool) -> String {
    let dom = ctx.dom;
    let whole = ctx.lex_range(rpr);
    if !drop_fonts {
        return ctx.slice(&whole).to_string();
    }
    let Some(fonts) = dom.semantic_children(rpr).find(|&n| dom.is(n, w(LocalName::RFonts))) else {
        return ctx.slice(&whole).to_string();
    };
    let cut = ctx.lex_range(fonts);
    let src = ctx.dom.src();
    let (a, b) = (whole.start as usize, whole.end as usize);
    let (c, d) = (cut.start as usize, cut.end as usize);
    if c < a || d > b {
        return ctx.slice(&whole).to_string();
    }
    format!("{}{}", &src[a..c], &src[d..b])
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

/// `RES-05`：符号字体 run 的显示文本。
///
/// `w:sym` 按字体表解码，表外的保留原字符（`U+F000 + 码位`，与 TS 一致，语料
/// `symbol-fonts__002`）；符号字体 run 的
/// `w:t` 只解码 PUA 区间的字符。返回值第二项表示"文本段被解码过"——TS 那边这种 run 的 `w:rFonts`
/// 会被摘掉（字形已经变成真正的 Unicode，再带符号字体反而显示不出来）。
fn symbol_text(ctx: &Ctx<'_>, run: &Run, segs: &[crate::model::Segment]) -> (String, bool) {
    let font = ctx.resolver.fonts(&run.props).display_ascii().map(str::to_string);
    let symbol_run = font.as_deref().is_some_and(crate::resolve::is_symbol_font);
    let mut text = String::new();
    let mut decoded_text = false;
    for seg in segs {
        match &seg.kind {
            SegmentKind::Text | SegmentKind::DelText if symbol_run => {
                let f = font.as_deref().unwrap_or_default();
                for c in run.segment_text(seg).chars() {
                    match crate::resolve::decode_pua(f, c) {
                        Some(d) => {
                            text.push(d);
                            decoded_text = true;
                        }
                        None => text.push(c),
                    }
                }
            }
            SegmentKind::Text | SegmentKind::DelText => text.push_str(run.segment_text(seg)),
            SegmentKind::Tab | SegmentKind::PTab { .. } => text.push('\t'),
            SegmentKind::Br { kind, .. } => text.push_str(break_char(*kind)),
            SegmentKind::Cr => text.push('\n'),
            SegmentKind::NoBreakHyphen => text.push('\u{2011}'),
            // 解码失败保留原字符（`RES-05`）：模型里放的就是 `U+F000 + 码位`，TS 也是这样
            SegmentKind::Sym { font, code: Some(_) } => {
                match font.as_deref().and_then(|f| {
                    let SegmentKind::Sym { code: Some(c), .. } = &seg.kind else { return None };
                    crate::resolve::decode_symbol(f, *c)
                }) {
                    Some(d) => text.push(d),
                    None => text.push_str(run.segment_text(seg)),
                }
            }
            _ => {}
        }
    }
    (text, decoded_text)
}

/// 一个模型 run → 零到多个 TS run。
///
/// TS `splitImageRun`：一个 run 里有不止一个图形子元素（`w:drawing` / `w:pict` / `w:object`）时按图形拆开——
/// `Run.image` 只有一个位置，不拆的话第一张之后的图全丢、一编辑就从文件里消失。每一段收到它**前面**的文字，
/// 最后剩下的文字单独成段（`smartart-ole__017`：`w:object` + 文字 + 空 `w:pict` → 图片 run + 文字 run；
/// 任务 6.4）。只有一个图形的 run 不拆，图片与文字同在一个 run 上。
fn run_jsons(ctx: &Ctx<'_>, run: &Run, para: StyleDisp) -> Vec<Map<String, Value>> {
    let is_graphic = |seg: &crate::model::Segment| {
        matches!(seg.kind, SegmentKind::Drawing { .. } | SegmentKind::Pict | SegmentKind::Object)
    };
    if run.segments.iter().filter(|s| is_graphic(s)).count() <= 1 {
        return run_json(ctx, run, para).into_iter().collect();
    }
    let mut parts: Vec<std::ops::Range<usize>> = Vec::new();
    let mut start = 0;
    for (i, seg) in run.segments.iter().enumerate() {
        if is_graphic(seg) {
            parts.push(start..i + 1);
            start = i + 1;
        }
    }
    if start < run.segments.len() {
        parts.push(start..run.segments.len());
    }
    parts.into_iter().filter_map(|r| run_json_segs(ctx, run, &run.segments[r], para)).collect()
}

/// TS `buildRun`。
fn run_json(ctx: &Ctx<'_>, run: &Run, para: StyleDisp) -> Option<Map<String, Value>> {
    run_json_segs(ctx, run, &run.segments, para)
}

/// [`run_json`] 的一部分 run：`segs` 是 `run.segments` 的一个连续切片（[`run_jsons`] 拆图形用）。
fn run_json_segs(
    ctx: &Ctx<'_>,
    run: &Run,
    segs: &[crate::model::Segment],
    para: StyleDisp,
) -> Option<Map<String, Value>> {
    // `COMPAT-07`：脚注 / 尾注引用是原子 run，`text` 是显示编号，其余字段一概不出（TS 行为）
    if let Some((endnote, id)) = note_ref_of(run) {
        let mut o = Map::new();
        set(&mut o, "text", ctx.note_number(endnote, id.as_deref()));
        let mut nr = Map::new();
        if let Some(id) = id {
            set(&mut nr, "id", id);
        }
        set(&mut nr, "kind", if endnote { "endnote" } else { "footnote" });
        set(&mut o, "noteRef", Value::Object(nr));
        comment_ids(ctx, run, &mut o);
        return Some(o);
    }
    // `w:ruby`：TS 见到它就只出 `{ text: 被注正文, ruby }`，同 run 的其他子节点与 `w:rPr` 都不看（任务 6.5）
    if let Some((node, rt, base)) = segs.iter().find_map(|s| match &s.kind {
        SegmentKind::Ruby { rt, base } => Some((s.node, rt, base)),
        _ => None,
    }) {
        let mut o = math::ruby_run(ctx, node, rt, base);
        comment_ids(ctx, run, &mut o);
        return Some(o);
    }
    let (text, symbol_decoded) = symbol_text(ctx, run, segs);
    // TS `buildRun(withImages)`：run 里的图片成为一个 `text: ""` 的原子 run。
    let image = segs.iter().find_map(|s| image::run_image(ctx, s));
    if text.is_empty() && image.is_none() {
        return None;
    }
    let mut o = Map::new();
    set(&mut o, "text", text);
    if let Some(img) = image {
        set(&mut o, "image", Value::Object(img));
    }
    comment_ids(ctx, run, &mut o);
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
                if let Some(f) = ctx.fields.get(*id)
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
    let inherited_rtl = run_format_json(ctx, run.node, &run.props, para, symbol_decoded, &mut o);
    rpr_change_json(ctx, run, inherited_rtl, &mut o);
    revision_ctx(run, &mut o);
    Some(o)
}

/// run 的格式键（`COMPAT-07` 的 `rawRPr` 与半解析字段）。
///
/// 从 [`run_json`] 里拆出来是给页眉页脚投影用的：TS 的 `hfContentFromXml` 把 PAGE 字段整段换成
/// 一个"带同一份 `w:rPr`、文字是标记"的合成 run（`COMPAT-05`），所以格式要能脱离 run 的文字单独算。
pub(super) fn run_format_json(
    ctx: &Ctx<'_>,
    node: NodeId,
    props: &RunProps,
    para: StyleDisp,
    symbol_decoded: bool,
    o: &mut Map<String, Value>,
) -> Option<bool> {
    let dom = ctx.dom;
    let r = ctx.resolver;
    let rpr_node = dom.semantic_children(node).find(|&n| dom.is(n, w(LocalName::RPr)));
    let para_rtl = para.rtl;
    let para_vanish = para.vanish;
    let Some(rpr_node) = rpr_node else {
        if para_rtl == Some(true) {
            set(o, "cs", true);
        }
        if para_vanish == Some(true) {
            set(o, "vanish", true);
        }
        return para_rtl;
    };
    set(o, "rawRPr", raw_rpr(ctx, rpr_node, symbol_decoded));
    let r_style = props.style.as_deref().filter(|s| *s != "Hyperlink");
    if let Some(s) = r_style {
        set(o, "styleId", s);
    }
    let char_disp = r_style.map(|s| ctx.style_disp(s, StyleType::Character)).unwrap_or_default();
    let vanish_own = if props.spec_vanish == Some(true) { None } else { props.vanish };
    if vanish_own.or(char_disp.vanish).or(para_vanish) == Some(true) {
        set(o, "vanish", true);
    }
    let inherited_rtl = char_disp.rtl.or(para_rtl);
    let cs = props.rtl.or(inherited_rtl) == Some(true);
    if cs {
        set(o, "cs", true);
    }
    if let Some(b) = if cs { props.bold_cs } else { props.bold } {
        set(o, "bold", b);
    }
    if let Some(b) = if cs { props.italic_cs } else { props.italic } {
        set(o, "italic", b);
    }
    if let Some(u) = props.underline.as_ref().and_then(|u| u.val.as_ref()) {
        if *u != Val::Value(UnderlineKind::None) {
            set(o, "underline", true);
        } else {
            set(o, "underline", false);
        }
    }
    if let Some(b) = props.strike {
        set(o, "strike", b);
    }
    if let Some(c) = props.color.as_ref().and_then(|c| r.color(c)) {
        set(o, "color", rgb_hex(c));
    } else if let Some(hex) = props.text_fill.and_then(|tf| text_fill_hex(ctx, tf)) {
        // TS `w14TextFillHex`：WordArt 文字填充（`w14:textFill`）当作颜色——实心直接取，渐变取停靠点的
        // 等权平均（显示近似；`wordart-vml__012/013`，任务 6.9）
        set(o, "color", hex);
    }
    let sz = if cs { &props.size_cs } else { &props.size };
    if let Some(n) = u32_of(sz).filter(|&n| n != 0) {
        set(o, "sizeHalfPoints", n);
    }
    let fonts = r.fonts(props);
    // 解码过的符号 run：TS 连 `w:rFonts` 一起摘掉，`font` / `fontAscii` / `themeRFonts` 都不出
    let fonts = if symbol_decoded { Default::default() } else { fonts };
    let font = fonts.display().map(str::to_string);
    if let Some(f) = &font {
        set(o, "font", f.clone());
        if fonts.ea_slot_empty && fonts.east_asia.as_deref() == Some(f) {
            set(o, "eaSlotEmpty", true);
        }
    }
    let font_ascii = fonts.display_ascii().map(str::to_string);
    if let Some(a) = &font_ascii {
        set(o, "fontAscii", a.clone());
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
        set(o, "themeRFonts", Value::Object(tr));
    }
    if let Some(cs_lit) = props.fonts.as_ref().and_then(|f| f.cs.clone()).filter(|s| !s.is_empty())
    {
        set(o, "fontCs", cs_lit);
    }
    if let Some(cs_font) = &fonts.cs {
        set(o, "csFont", cs_font.clone());
    }
    if let Some(b) = props.rtl {
        set(o, "rtl", b);
    }
    if let Some(sp) = twips_int(&props.spacing).filter(|&n| n != 0) {
        set(o, "charSpacingTwips", sp);
    }
    match (props.caps, props.small_caps) {
        (Some(true), _) => set(o, "caps", "all"),
        (_, Some(true)) => set(o, "caps", "small"),
        (Some(false), _) | (_, Some(false)) => set(o, "caps", "none"),
        _ => {}
    }
    if let Some(sc) = u32_of(&props.scale).filter(|&n| n > 0 && n != 100) {
        set(o, "charScalePct", sc);
    }
    if let Some(h) = val_text(&props.highlight, |h| h.as_str()).filter(|h| h != "none") {
        set(o, "highlight", h);
    }
    if props.shading.is_some() {
        let raw = dom
            .semantic_children(rpr_node)
            .find(|&n| dom.is(n, w(LocalName::Shd)))
            .and_then(|n| ctx.attr(n, NsId::W, LocalName::Fill));
        if let Some(fill) = raw
            && fill != "auto"
        {
            set(o, "shading", strip_hash(&fill).to_string());
        }
    }
    if let Some(va) = val_text(&props.vert_align, |v| v.as_str())
        .filter(|v| v == "superscript" || v == "subscript")
    {
        set(o, "vertAlign", va);
    }
    if let Some(em) = val_text(&props.em, |e| e.as_str()).filter(|e| e != "none") {
        set(o, "em", em);
    }
    inherited_rtl
}

/// `COMPAT-07` 的 `rPrChange`：旧属性快照。`inherited_rtl` 决定读 `b` 还是 `bCs`（`RES-06`）。
fn rpr_change_json(
    ctx: &Ctx<'_>,
    run: &Run,
    inherited_rtl: Option<bool>,
    o: &mut Map<String, Value>,
) {
    let r = ctx.resolver;
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
        set(o, "rPrChange", Value::Object(change));
    }
}

/// run 里的脚注 / 尾注引用段（`w:footnoteReference` / `w:endnoteReference`）。
fn note_ref_of(run: &Run) -> Option<(bool, Option<String>)> {
    run.segments.iter().find_map(|s| match &s.kind {
        SegmentKind::FootnoteRef { id } => Some((false, id.clone())),
        SegmentKind::EndnoteRef { id } => Some((true, id.clone())),
        _ => None,
    })
}

/// `COMPAT-07` `commentIds`：起止都在本段的批注范围覆盖到的 run，加上只有 `commentReference`
/// 的批注（模型侧已按 TS 规则挂到最近的有字 run，见 `Document` 的 `attach_comments`）。
fn comment_ids(ctx: &Ctx<'_>, run: &Run, o: &mut Map<String, Value>) {
    if run.comments.is_empty() {
        return;
    }
    let ids: Vec<Value> = run
        .comments
        .iter()
        .filter_map(|s| ctx.spans.get(*s))
        .map(|s| Value::String(s.pair_id().to_string()))
        .collect();
    if !ids.is_empty() {
        set(o, "commentIds", Value::Array(ids));
    }
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
pub(super) fn merge_runs(runs: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
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

/// 空的 aux 表（多数文档没有外部 part）。
fn empty_aux() -> &'static AuxProjMap<'static> {
    static EMPTY: std::sync::LazyLock<AuxProjMap<'static>> =
        std::sync::LazyLock::new(AuxProjMap::new);
    &EMPTY
}

#[cfg(test)]
mod tests {
    use super::first_page_break_at;

    /// 首页判定：谁在最前面就是分页点，找不到就整篇算首页。
    #[test]
    fn compat_03_first_page_break_takes_the_earliest_marker() {
        let at = |x: &str| first_page_break_at(x);
        assert_eq!(at("<w:p/><w:p/>"), None);
        assert_eq!(at(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#), Some(10));
        // 节结束也算分页
        assert_eq!(at("<w:p/></w:sectPr>"), Some(6));
        // 取最靠前的那个：这里 br 在 sectPr 之前
        let both = r#"<w:br w:type="page"/><w:p/></w:sectPr>"#;
        assert_eq!(at(both), Some(0));
        // Word 记录的渲染分页提示；不自闭合的标签不算
        assert_eq!(at("<w:p/><w:lastRenderedPageBreak/>"), Some(6));
        assert_eq!(at("<w:p/><w:lastRenderedPageBreak></w:lastRenderedPageBreak>"), None);
        // `<w:br/>` 没有 w:type="page" 就不是分页
        assert_eq!(at(r#"<w:br w:type="textWrapping"/>"#), None);
    }
}
