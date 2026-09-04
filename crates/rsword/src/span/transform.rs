//! 编辑期的 Anchor 变换与整体删除策略（`SPAN-06` / `SPAN-07`）。
//!
//! 变换**从 `MutationPlan.node_edits` 推导**，不是每个操作各自计算：内容序列只会因为
//! "插入内容项 / 删除内容项 / 移动内容项" 而变化，这三件事都写在编辑列表里，所以每个操作
//! （包括 compat 路径）自动得到正确的锚点维护，新增操作也不会漏。
//!
//! 下标都在**编辑前**的内容序列里：计划阶段读的是未提交的 DOM，锚点的新下标由"存活项计数 +
//! 落在锚点之前的插入数"算出，这一条同时覆盖 `SPAN-06` 表里插入与删除两行（删除区间内部的
//! 锚点自然落到区间起点，而且对不连续的删除也成立）。

use std::collections::{HashMap, HashSet};

use crate::diag::{DiagCode, Diagnostic};
use crate::xml::{Dirty, Dom, NodeEdit, NodeId, Target};

use super::content::{
    boundary_before, container_of, content_len, is_content_container, is_content_item,
};
use super::index::{Affinity, Anchor, RangeClass, RangeKind, RangeSpan, SpanEnd, SpanIndex};
use super::{SpanId, is_property_element, is_range_marker};

/// 操作对范围的额外要求：编辑列表本身看不出来的那部分语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanPolicy {
    /// `SPAN-07`：整体删除的批注默认连范围与 reference 一起删；置 `true` 则折叠保留。
    pub keep_orphan_comments: bool,
    /// 内容被整体重写的容器（compat 的 `ReplaceInlines`）：提交后按新标记重建这些容器的端点。
    pub rescan: Vec<NodeId>,
    /// 被拆成两半的内容项（`split_run`）。
    ///
    /// 拆分与"在边界插入新内容"在 DOM 上一样（都是在某个边界插入一个元素），语义上不同：
    /// 拆出来的后半是**原内容的延续**，所以该边界上的 `Left` 锚点也要跟着右移，否则在范围
    /// 内部输入会把后半挤到范围外。`SPAN-06` 的插入行只管新内容，这一条是它的补充。
    pub split_items: Vec<NodeId>,
}

impl SpanPolicy {
    pub fn is_default(&self) -> bool {
        !self.keep_orphan_comments && self.rescan.is_empty() && self.split_items.is_empty()
    }
}

/// 一个容器的内容序列变化（下标都在编辑前的序列里）。
#[derive(Debug, Default)]
struct ContainerDelta {
    /// 被删除内容项的下标。
    removed: Vec<u32>,
    /// `(边界, 新增内容项个数)`。
    inserted: Vec<(u32, u32)>,
}

/// 一次编辑对各容器内容序列的影响。
#[derive(Debug, Default)]
struct ContentDelta {
    per_container: HashMap<NodeId, ContainerDelta>,
    /// `(容器, 边界)`：该边界上的插入是"原内容的延续"，`Left` 锚点也要右移。
    continuations: HashSet<(NodeId, u32)>,
    /// 整棵被删除（或被移走）的子树根。
    dead_roots: Vec<NodeId>,
    /// 直接被删除的节点（含标记这类非内容项）。
    dead: HashSet<NodeId>,
    /// 有插入 / 删除 / 移动：`FlowMap` 需要重建。
    structural: bool,
}

impl ContentDelta {
    fn is_empty(&self) -> bool {
        self.per_container.is_empty() && self.dead.is_empty()
    }

    fn at(&self, container: NodeId) -> Option<&ContainerDelta> {
        self.per_container.get(&container)
    }

    fn entry(&mut self, container: NodeId) -> &mut ContainerDelta {
        self.per_container.entry(container).or_default()
    }

    /// 节点是否随本次编辑消失（自身被删，或在被删子树里）。
    fn is_dead(&self, dom: &Dom, node: NodeId) -> bool {
        if self.dead.contains(&node) {
            return true;
        }
        self.dead_roots.iter().any(|&r| dom.is_ancestor_or_self(r, node))
    }

    /// 包含该节点的最外层被删子树根。
    fn outermost_dead_root(&self, dom: &Dom, node: NodeId) -> Option<NodeId> {
        let mut best: Option<NodeId> = None;
        for &r in &self.dead_roots {
            if !dom.is_ancestor_or_self(r, node) {
                continue;
            }
            best = match best {
                Some(b) if dom.is_ancestor_or_self(r, b) => Some(r),
                Some(b) => Some(b),
                None => Some(r),
            };
        }
        best
    }

    fn removed_item(&self, container: NodeId, index: u32) -> bool {
        self.at(container).is_some_and(|d| d.removed.contains(&index))
    }

    fn is_continuation(&self, container: NodeId, boundary: u32) -> bool {
        self.continuations.contains(&(container, boundary))
    }
}

/// `SPAN-06` 的一条结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanAction {
    /// 端点移到新位置（`marker` 已按存活情况修正）。
    Move { span: SpanId, end: SpanEnd, to: Anchor },
    /// 该端连标记一起消失，范围变成半开，交给 `SPAN-09` 在保存前修复。
    Drop { span: SpanId, end: SpanEnd },
    /// `SPAN-07` 整体删除：`nodes` 是要一并删掉的标记与 reference run。
    Remove { span: SpanId, nodes: Vec<NodeId> },
}

/// 计划阶段产出的范围变更（只读）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanUpdate {
    pub actions: Vec<SpanAction>,
    /// 提交后需要按标记重建端点的容器。
    pub rescan: Vec<NodeId>,
    /// 结构变了，`FlowMap` 需要重建。
    pub flows_stale: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl SpanUpdate {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.rescan.is_empty() && !self.flows_stale
    }

    /// `SPAN-07` 要求随范围一起删除的节点。
    pub fn removed_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.actions
            .iter()
            .flat_map(|a| match a {
                SpanAction::Remove { nodes, .. } => nodes.as_slice(),
                _ => &[],
            })
            .copied()
    }
}

/// `SPAN-06`：预测一组 `NodeEdit` 对索引的全部影响。只读，不改 DOM 也不改索引。
pub fn plan_update(
    dom: &Dom,
    index: &SpanIndex,
    edits: &[NodeEdit],
    policy: &SpanPolicy,
) -> SpanUpdate {
    let delta = derive(dom, edits, policy);
    let mut update = SpanUpdate {
        rescan: policy.rescan.clone(),
        flows_stale: delta.structural,
        ..Default::default()
    };
    if delta.is_empty() && policy.rescan.is_empty() {
        return update;
    }
    for span in index.spans() {
        if span.removed {
            continue;
        }
        // 内容被整体重写的容器：两端都在里面 → 整个范围没了；只有一端在里面 → 该端失去
        if !policy.rescan.is_empty() {
            let inside = |a: &Option<Anchor>| {
                a.as_ref().is_some_and(|a| policy.rescan.contains(&a.container))
            };
            let (si, ei) = (inside(&span.start), inside(&span.end));
            if si && ei {
                update.actions.push(SpanAction::Remove { span: span.id, nodes: Vec::new() });
                continue;
            }
            if si || ei {
                // 跨容器范围只有一端落在被重写的容器里：那一端的标记随内容消失。**不动**另一端
                // ——它在没被编辑的容器里，删掉就改写了未编辑内容（不变式 2）。范围变成半开，
                // `SPAN-09` 保存时按"不动 Clean 标记"的规则原样写回并记诊断，物理输出与 TS 一致。
                update.actions.push(SpanAction::Drop {
                    span: span.id,
                    end: if si { SpanEnd::Start } else { SpanEnd::End },
                });
                update.diagnostics.push(Diagnostic::invariant_violation(
                    span.part,
                    None,
                    DiagCode::SpanUnclosed,
                    format!(
                        "{:?} w:id=\"{}\" 跨容器，一端所在容器的内容被整体重写，索引里只剩另一端",
                        span.class(),
                        span.pair_id()
                    ),
                ));
                continue;
            }
        }
        if fully_deleted(dom, &delta, span) && !keep_whole(span, policy) {
            update
                .actions
                .push(SpanAction::Remove { span: span.id, nodes: dead_nodes(dom, &delta, span) });
            continue;
        }
        for end in [SpanEnd::Start, SpanEnd::End] {
            let Some(a) = span.anchor(end) else { continue };
            if policy.rescan.contains(&a.container) {
                continue; // 已经 Drop
            }
            match map_anchor(dom, &delta, a) {
                Some(to) if to != *a => {
                    update.actions.push(SpanAction::Move { span: span.id, end, to })
                }
                Some(_) => {}
                None => update.actions.push(SpanAction::Drop { span: span.id, end }),
            }
        }
    }
    update
}

/// `SPAN-07`：两端都落进被删内容的范围是否保留（折叠）。
fn keep_whole(span: &RangeSpan, policy: &SpanPolicy) -> bool {
    match span.class() {
        // 书签折叠留在删除点：内部的 `_Toc` / `_Ref` 目标仍存在，REF / TOC 不断链
        RangeClass::Bookmark => true,
        RangeClass::Comment => policy.keep_orphan_comments,
        // 权限范围随内容消失；移动与 customXml 修订范围失去内容后没有意义（接受 / 拒绝修订在 M7）
        _ => false,
    }
}

/// 整体删除时要一并删掉的节点：两端的标记与批注的 reference run。
fn dead_nodes(dom: &Dom, delta: &ContentDelta, span: &RangeSpan) -> Vec<NodeId> {
    let mut v = Vec::new();
    let mut push = |n: Option<NodeId>| {
        if let Some(n) = n
            && dom.node(n).dirty != Dirty::Deleted
            && !delta.is_dead(dom, n)
            && !v.contains(&n)
        {
            v.push(n);
        }
    };
    push(span.start.and_then(|a| a.marker));
    push(span.end.and_then(|a| a.marker));
    if let RangeKind::Comment { reference, .. } = &span.kind {
        push(*reference);
    }
    v
}

/// 两端都落进被删内容（或被删容器）。
fn fully_deleted(dom: &Dom, delta: &ContentDelta, span: &RangeSpan) -> bool {
    let (Some(s), Some(e)) = (&span.start, &span.end) else { return false };
    let sd = delta.outermost_dead_root(dom, s.container).is_some();
    let ed = delta.outermost_dead_root(dom, e.container).is_some();
    if sd && ed {
        return true;
    }
    if sd || ed {
        return false;
    }
    if s.container != e.container || s.index >= e.index {
        // 折叠范围与跨容器范围：内容是否被整体删除交给"容器被删"那一支判断
        return false;
    }
    (s.index..e.index).all(|i| delta.removed_item(s.container, i))
}

/// 锚点的新位置。`None` = 连同容器一起消失且外层也没了（该端失去）。
fn map_anchor(dom: &Dom, delta: &ContentDelta, a: &Anchor) -> Option<Anchor> {
    let marker = a.marker.filter(|&m| !delta.is_dead(dom, m));
    let (container, index) = match delta.outermost_dead_root(dom, a.container) {
        // 容器被删：锚点移到"容器所在内容序列"里它原来占的边界（`SPAN-06` 最后两行）
        Some(root) => {
            let outer = container_of(dom, root)?;
            let at = boundary_before(dom, outer, root)?;
            (outer, at)
        }
        None => (a.container, a.index),
    };
    let index = map_boundary(delta, index, a.affinity, dom, container);
    Some(Anchor { container, index, affinity: a.affinity, marker })
}

/// 边界映射：存活项计数 + 落在锚点之前的插入数（`SPAN-06` 插入 / 删除两行）。
fn map_boundary(
    all: &ContentDelta,
    index: u32,
    affinity: Affinity,
    dom: &Dom,
    container: NodeId,
) -> u32 {
    let Some(d) = all.at(container) else { return index };
    let removed_before = d.removed.iter().filter(|&&i| i < index).count() as u32;
    let inserted_before: u32 = d
        .inserted
        .iter()
        .filter(|&&(b, _)| {
            b < index
                || (b == index
                    && (affinity == Affinity::Right || all.is_continuation(container, b)))
        })
        .map(|&(_, c)| c)
        .sum();
    let mapped = index - removed_before.min(index) + inserted_before;
    // 上界防御：内容序列长度在提交后才变，这里按"编辑前长度 - 删除 + 插入"夹逼
    let after_len = content_len(dom, container) + d.inserted.iter().map(|&(_, c)| c).sum::<u32>()
        - (d.removed.len() as u32).min(content_len(dom, container));
    mapped.min(after_len)
}

/// 从编辑列表推导内容序列变化。
fn derive(dom: &Dom, edits: &[NodeEdit], policy: &SpanPolicy) -> ContentDelta {
    let mut d = ContentDelta::default();
    for &item in &policy.split_items {
        if let Some(c) = container_of(dom, item)
            && let Some(i) = boundary_before(dom, c, item)
        {
            d.continuations.insert((c, i + 1));
        }
    }
    for e in edits {
        match e {
            NodeEdit::Delete(n) => {
                d.structural = true;
                d.dead.insert(*n);
                d.dead_roots.push(*n);
                remove_item(dom, &mut d, *n);
            }
            NodeEdit::Insert { parent: Target::Node(p), before, node } => {
                d.structural = true;
                if new_is_content_item(node.name) {
                    insert_item(dom, &mut d, *p, *before);
                }
            }
            NodeEdit::InsertClone { parent: Target::Node(p), before, source } => {
                d.structural = true;
                if is_content_item(dom, *source) {
                    insert_item(dom, &mut d, *p, *before);
                }
            }
            // 原位替换：内容序列不变，但旧节点作为标记的身份消失
            NodeEdit::Replace { old, .. } | NodeEdit::ReplaceClone { old, .. } => {
                d.structural = true;
                d.dead.insert(*old);
            }
            NodeEdit::Move { node, parent, before } => {
                d.structural = true;
                remove_item(dom, &mut d, *node);
                if let Target::Node(p) = parent
                    && is_content_item(dom, *node)
                {
                    insert_item(dom, &mut d, *p, *before);
                }
            }
            // 新建父节点下的插入不影响已有锚点；属性与文本变更不改内容序列
            NodeEdit::Insert { .. }
            | NodeEdit::InsertClone { .. }
            | NodeEdit::SetAttr { .. }
            | NodeEdit::RemoveAttr { .. }
            | NodeEdit::SetText { .. } => {}
        }
    }
    d
}

/// 新元素是否会成为内容项（属性元素与标记不进内容序列）。
fn new_is_content_item(name: crate::xml::QName) -> bool {
    !is_property_element(name) && !is_range_marker(name)
}

fn remove_item(dom: &Dom, d: &mut ContentDelta, node: NodeId) {
    if !is_content_item(dom, node) {
        return;
    }
    let Some(c) = container_of(dom, node) else { return };
    if let Some(i) = boundary_before(dom, c, node) {
        d.entry(c).removed.push(i);
    }
}

fn insert_item(dom: &Dom, d: &mut ContentDelta, parent: NodeId, before: Option<NodeId>) {
    // 只有直接插进内容容器才产生新的内容项（插进 `w:r` 的 `w:t` 不是段落的内容项）
    if !dom.name(parent).is_some_and(is_content_container) {
        return;
    }
    let at = match before {
        Some(b) => boundary_before(dom, parent, b).unwrap_or_else(|| content_len(dom, parent)),
        None => content_len(dom, parent),
    };
    let slot = d.entry(parent);
    match slot.inserted.iter_mut().find(|(b, _)| *b == at) {
        Some((_, n)) => *n += 1,
        None => slot.inserted.push((at, 1)),
    }
}

// ---- 提交后应用 ----

impl SpanIndex {
    /// 机械应用（提交之后调用，`dom` 是提交后的 DOM）。
    ///
    /// `rescan` 的容器按新标记重建端点：内容是被外部描述整体重写的（compat 的 `ReplaceInlines`
    /// 会按 `commentIds` 重发批注标记），这时 DOM 才是那个容器里标记位置的真相。除此之外
    /// **禁止**由标记反推 Anchor（`SPAN-02`）。
    pub fn apply(&mut self, dom: &Dom, update: &SpanUpdate) {
        for action in &update.actions {
            match action {
                SpanAction::Move { span, end, to } => {
                    if let Some(s) = self.get_mut(*span) {
                        match end {
                            SpanEnd::Start => s.start = Some(*to),
                            SpanEnd::End => s.end = Some(*to),
                        }
                    }
                }
                SpanAction::Drop { span, end } => {
                    if let Some(s) = self.get_mut(*span) {
                        match end {
                            SpanEnd::Start => s.start = None,
                            SpanEnd::End => s.end = None,
                        }
                    }
                }
                SpanAction::Remove { span, .. } => {
                    if let Some(s) = self.get_mut(*span) {
                        s.removed = true;
                    }
                }
            }
        }
        self.normalize_collapsed();
        for &container in &update.rescan {
            self.rescan_container(dom, container);
        }
        if update.flows_stale {
            self.refresh_flows(dom);
        }
        if !update.actions.is_empty() || !update.rescan.is_empty() {
            self.reindex_containers();
        }
    }
}
