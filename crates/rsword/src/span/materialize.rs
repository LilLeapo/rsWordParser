//! 物化与保存前校验（`SPAN-08` / `SPAN-09`）。
//!
//! 保存时 Anchor 是事实、标记是投影：位置没变的标记一个字节都不动（`Clean` 拷字节），位置变了
//! 的旧标记 `Deleted`、新位置插 `New`，本来就没有标记的（新建范围、容器被删后搬出来的锚点）
//! 按 `RangeKind` 生成。**只有** `implicit` 的范围（文件里本来只有 `commentReference` 的批注）
//! 永远不写标记。
//!
//! 校验（`SPAN-09`）在物化之前：半开、跨流、起在终后的范围不物化，按来源记诊断；修复（删掉
//! 落单的标记）只在**脏容器**里做——干净容器里的解析期缺陷保持原字节，否则"编辑一段不动别处"
//! 的不变式 2 会被破坏。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic, ValidationOrigin};
use crate::xml::{Dirty, Dom, LocalName, NewElement, NodeEdit, NodeId, QName, Target};

use super::SpanId;
use super::content::{boundary_before, is_content_item};
use super::index::{Anchor, RangeClass, RangeKind, RangeSpan, SpanEnd, SpanIndex, SpanOrigin};
use super::is_property_element;

/// 一次保存的范围计划。
#[derive(Debug, Clone, Default)]
pub struct MaterializePlan {
    pub edits: Vec<NodeEdit>,
    /// `edits[k]` 创建的标记属于哪个端点：提交后把新 `NodeId` 回填到 `marker`。
    pub markers: Vec<(usize, SpanId, SpanEnd)>,
    /// `SPAN-09` 修复掉的范围。
    pub removed: Vec<SpanId>,
    pub diagnostics: Vec<Diagnostic>,
}

impl MaterializePlan {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.removed.is_empty() && self.diagnostics.is_empty()
    }
}

/// `SPAN-08` + `SPAN-09`：产出保存前要做的标记变更（只读）。
pub fn plan_save(dom: &Dom, index: &SpanIndex) -> MaterializePlan {
    let mut plan = MaterializePlan::default();
    let mut seen: HashMap<(RangeClass, &str), SpanId> = HashMap::new();
    for span in index.live() {
        if span.implicit {
            continue;
        }
        if !span.pair_id().is_empty() {
            let key = (span.class(), span.pair_id());
            if let Some(prev) = seen.insert(key, span.id) {
                plan.diagnostics.push(diag(
                    dom,
                    span,
                    DiagCode::SpanDupStart,
                    format!(
                        "{:?} w:id=\"{}\" 在 part 内不唯一（另一个是 span {}）",
                        span.class(),
                        span.pair_id(),
                        prev.0
                    ),
                ));
            }
        }
        match (span.start, span.end) {
            (Some(s), Some(e)) => {
                if index.compare(dom, &s, &e).is_none() {
                    plan.diagnostics.push(diag(
                        dom,
                        span,
                        DiagCode::SpanCrossFlow,
                        "两端不在同一内容流，不物化".to_string(),
                    ));
                    continue;
                }
                if !index.is_ordered(dom, span) {
                    plan.diagnostics.push(diag(
                        dom,
                        span,
                        DiagCode::SpanOrphanEnd,
                        "起点在终点之后，不物化".to_string(),
                    ));
                    continue;
                }
                materialize_pair(dom, span, &mut plan);
            }
            // 半开：`SPAN-09` 的成对修复
            (Some(a), None) => repair_half_open(dom, span, &a, DiagCode::SpanUnclosed, &mut plan),
            (None, Some(a)) => repair_half_open(dom, span, &a, DiagCode::SpanOrphanEnd, &mut plan),
            (None, None) => plan.removed.push(span.id),
        }
    }
    plan
}

/// 提交之后把新标记的 `NodeId` 回填到锚点，并落实 `SPAN-09` 的修复。
pub fn apply_save(index: &mut SpanIndex, created: &[Option<NodeId>], plan: &MaterializePlan) {
    for &(k, span, end) in &plan.markers {
        let Some(Some(node)) = created.get(k) else { continue };
        if let Some(s) = index.get_mut(span)
            && let Some(a) = s.anchor_mut(end)
        {
            a.marker = Some(*node);
        }
    }
    for &span in &plan.removed {
        if let Some(s) = index.get_mut(span) {
            s.removed = true;
        }
    }
}

/// 两端都在：各自按需要物化。
fn materialize_pair(dom: &Dom, span: &RangeSpan, plan: &mut MaterializePlan) {
    let (Some(s), Some(e)) = (span.start, span.end) else { return };
    let s_ok = marker_in_place(dom, &s);
    let e_ok = marker_in_place(dom, &e);
    if s_ok && e_ok {
        return;
    }
    // 空范围两端都要重发时放在同一个位置、起点在前，否则物理上会成为反序的一对（`SPAN-08` 例外）
    if span.is_collapsed() && !s_ok && !e_ok {
        let site = insert_site(dom, s.container, s.index, SpanEnd::Start);
        emit(dom, span, SpanEnd::Start, &s, site, plan);
        emit(dom, span, SpanEnd::End, &e, site, plan);
        return;
    }
    if !s_ok {
        let site = insert_site(dom, s.container, s.index, SpanEnd::Start);
        emit(dom, span, SpanEnd::Start, &s, site, plan);
    }
    if !e_ok {
        let site = insert_site(dom, e.container, e.index, SpanEnd::End);
        emit(dom, span, SpanEnd::End, &e, site, plan);
    }
}

/// `SPAN-09`：半开范围的成对修复。
///
/// **不动 `Clean` 标记**：它的字节没变过，删掉就等于改写未编辑的内容（不变式 1 / 2）。
/// 解析期就损坏的输入因此原样写回，只记诊断——`docs/03` §5.6 的"成对删除"是安全网，
/// 不能反过来破坏字节保真。落单的标记已经不在（引擎把另一端删了）或已经被改写过时才作废。
fn repair_half_open(
    dom: &Dom,
    span: &RangeSpan,
    alive: &Anchor,
    code: DiagCode,
    plan: &mut MaterializePlan,
) {
    let alive_marker = alive.marker.filter(|&m| dom.node(m).dirty != Dirty::Deleted);
    match alive_marker {
        Some(m) if dom.node(m).dirty == Dirty::Clean => {
            plan.diagnostics.push(diag(
                dom,
                span,
                code,
                "范围缺一端；落单的标记未被改写过，保持原字节".into(),
            ));
        }
        Some(m) => {
            plan.edits.push(NodeEdit::Delete(m));
            plan.removed.push(span.id);
            plan.diagnostics.push(diag(dom, span, code, "范围缺一端，落单的标记已删除".into()));
        }
        None => {
            plan.removed.push(span.id);
            plan.diagnostics.push(diag(dom, span, code, "范围缺一端，标记已不在".into()));
        }
    }
}

/// 生成一条标记变更。
fn emit(
    dom: &Dom,
    span: &RangeSpan,
    end: SpanEnd,
    anchor: &Anchor,
    site: (NodeId, Option<NodeId>),
    plan: &mut MaterializePlan,
) {
    if let Some(old) = anchor.marker.filter(|&m| dom.node(m).dirty != Dirty::Deleted) {
        plan.edits.push(NodeEdit::Delete(old));
    }
    let node = marker_element(dom, span, end, anchor);
    plan.markers.push((plan.edits.len(), span.id, end));
    plan.edits.push(NodeEdit::Insert { parent: Target::Node(site.0), before: site.1, node });
}

/// 标记还在锚点指的位置上。
fn marker_in_place(dom: &Dom, a: &Anchor) -> bool {
    let Some(m) = a.marker else { return false };
    if dom.node(m).dirty == Dirty::Deleted {
        return false;
    }
    boundary_before(dom, a.container, m) == Some(a.index)
}

/// 标记元素：有旧标记就照抄属性（保住 `w:colFirst` / `w:displacedByCustomXml` 一类未建模的），
/// 否则按 `RangeKind` 生成。
fn marker_element(dom: &Dom, span: &RangeSpan, end: SpanEnd, anchor: &Anchor) -> NewElement {
    let name = QName::w(marker_name(span.class(), end));
    if let Some(old) = anchor.marker
        && dom.element(old).is_some()
    {
        let mut e = NewElement::new(name);
        if let Some(el) = dom.element(old) {
            for a in &el.attrs {
                e.push_attr(a.name, dom.attr_str(a).into_owned());
            }
        }
        return e;
    }
    let mut e = NewElement::new(name);
    let w = |l: LocalName| QName::w(l);
    e.push_attr(w(LocalName::Id), span.pair_id().to_string());
    if end == SpanEnd::Start {
        match &span.kind {
            RangeKind::Bookmark { name, cols, .. } => {
                e.push_attr(w(LocalName::Name), name.clone());
                if let Some((a, b)) = cols {
                    e.push_attr(w(LocalName::ColFirst), a.to_string());
                    e.push_attr(w(LocalName::ColLast), b.to_string());
                }
            }
            RangeKind::Permission { editor, group, cols, .. } => {
                if let Some(ed) = editor {
                    e.push_attr(w(LocalName::Ed), ed.clone());
                }
                if let Some(g) = group {
                    e.push_attr(w(LocalName::EdGrp), g.clone());
                }
                if let Some((a, b)) = cols {
                    e.push_attr(w(LocalName::ColFirst), a.to_string());
                    e.push_attr(w(LocalName::ColLast), b.to_string());
                }
            }
            RangeKind::MoveFrom { name, meta, .. } | RangeKind::MoveTo { name, meta, .. } => {
                e.push_attr(w(LocalName::Name), name.clone());
                push_meta(&mut e, meta);
            }
            RangeKind::CustomXmlIns { meta, .. }
            | RangeKind::CustomXmlDel { meta, .. }
            | RangeKind::CustomXmlMoveFrom { meta, .. }
            | RangeKind::CustomXmlMoveTo { meta, .. } => push_meta(&mut e, meta),
            RangeKind::Comment { .. } => {}
        }
    }
    e
}

fn push_meta(e: &mut NewElement, meta: &super::RevisionMeta) {
    if let Some(a) = &meta.author {
        e.push_attr(QName::w(LocalName::Author), a.clone());
    }
    if let Some(d) = &meta.date {
        e.push_attr(QName::w(LocalName::Date), d.clone());
    }
}

fn marker_name(class: RangeClass, end: SpanEnd) -> LocalName {
    use LocalName as L;
    let start = end == SpanEnd::Start;
    match class {
        RangeClass::Bookmark => {
            if start {
                L::BookmarkStart
            } else {
                L::BookmarkEnd
            }
        }
        RangeClass::Comment => {
            if start {
                L::CommentRangeStart
            } else {
                L::CommentRangeEnd
            }
        }
        RangeClass::Permission => {
            if start {
                L::PermStart
            } else {
                L::PermEnd
            }
        }
        RangeClass::MoveFrom => {
            if start {
                L::MoveFromRangeStart
            } else {
                L::MoveFromRangeEnd
            }
        }
        RangeClass::MoveTo => {
            if start {
                L::MoveToRangeStart
            } else {
                L::MoveToRangeEnd
            }
        }
        RangeClass::CustomXmlIns => {
            if start {
                L::CustomXmlInsRangeStart
            } else {
                L::CustomXmlInsRangeEnd
            }
        }
        RangeClass::CustomXmlDel => {
            if start {
                L::CustomXmlDelRangeStart
            } else {
                L::CustomXmlDelRangeEnd
            }
        }
        RangeClass::CustomXmlMoveFrom => {
            if start {
                L::CustomXmlMoveFromRangeStart
            } else {
                L::CustomXmlMoveFromRangeEnd
            }
        }
        RangeClass::CustomXmlMoveTo => {
            if start {
                L::CustomXmlMoveToRangeStart
            } else {
                L::CustomXmlMoveToRangeEnd
            }
        }
    }
}

/// 边界 `k` 上的插入位置（`SPAN-08` 顺序：同一边界先终点标记、后起点标记）。
///
/// 返回 `(parent, before)`。属性元素不能被跨过：`w:pPr` 一类领头的必须在插入点之前，
/// `w:sectPr` 一类收尾的必须在插入点之后。
fn insert_site(dom: &Dom, container: NodeId, k: u32, end: SpanEnd) -> (NodeId, Option<NodeId>) {
    let kids = dom.children(container);
    // 边界 k 的区间 [lo, hi)：第 k-1 个内容项之后、第 k 个内容项之前
    let mut items = 0u32;
    let mut lo = 0usize;
    let mut hi = kids.len();
    for (i, &c) in kids.iter().enumerate() {
        if dom.node(c).dirty == Dirty::Deleted {
            continue;
        }
        if is_content_item(dom, c) {
            if items == k {
                hi = i;
                break;
            }
            items += 1;
            lo = i + 1;
        }
    }
    if items < k {
        // 边界在最后一个内容项之后
        hi = kids.len();
    }
    // 领头的属性元素（w:pPr / w:tblPr …）之后
    while lo < hi
        && dom
            .name(kids[lo])
            .is_some_and(|q| is_property_element(q) && q.local != LocalName::SectPr)
    {
        lo += 1;
    }
    // 收尾的属性元素（w:sectPr）之前
    while hi > lo && dom.name(kids[hi - 1]).is_some_and(is_property_element) {
        hi -= 1;
    }
    let pos = match end {
        SpanEnd::End => lo,
        SpanEnd::Start => hi,
    };
    (container, kids.get(pos).copied())
}

fn diag(dom: &Dom, span: &RangeSpan, code: DiagCode, message: String) -> Diagnostic {
    let node = span.start.or(span.end).and_then(|a| a.marker);
    let range = node.and_then(|n| dom.node(n).lex.as_ref().map(|l| l.range.clone()));
    let origin = match span.origin {
        // 解析时就损坏的是输入的缺陷；本来完好的范围出问题就是引擎干的
        SpanOrigin::ParsedDamaged => ValidationOrigin::PreExistingDamage,
        SpanOrigin::Parsed | SpanOrigin::New => ValidationOrigin::EngineInvariantViolation,
    };
    Diagnostic { part: span.part, range, code, origin, message }
}
