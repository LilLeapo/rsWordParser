//! `Anchor` / `RangeSpan` 与范围索引（`SPAN-02`–`SPAN-05`）。
//!
//! 索引是平铺的 [`RangeSpan`] 列表加一份按容器的倒排表；范围之间允许任意交叠，没有树。
//! 构建后 **Anchor 是事实、标记是投影**（`docs/03` §5.2）：编辑期只变换 Anchor（`SPAN-06`），
//! 保存时按 Anchor 物化标记（`SPAN-08`）；禁止反向由标记推导 Anchor。

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::package::PartId;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

use super::content::{is_content_container, item_containing};
use super::{
    FlowId, FlowMap, RevisionMeta, SpanId, is_flow_root, is_property_element, is_range_marker,
};

/// 锚点在边界处插入内容时的去向（`SPAN-02`）。
///
/// `Left` 吸附左侧内容：在该边界插入的内容落在锚点**之后**（`index` 不动）。
/// `Right` 吸附右侧内容：插入的内容落在锚点**之前**（`index` 前移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Affinity {
    Left,
    Right,
}

/// 附着在 DOM 上的位置（`SPAN-02`）：容器 + 内容序列边界 + affinity。
///
/// `marker` 指向物理标记元素；新建范围与字段边界为 `None`（`FLD`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub container: NodeId,
    /// 内容序列边界，`0..=content_len(container)`；标记自身不计入。
    pub index: u32,
    pub affinity: Affinity,
    pub marker: Option<NodeId>,
}

impl Anchor {
    pub fn new(container: NodeId, index: u32, affinity: Affinity) -> Self {
        Self { container, index, affinity, marker: None }
    }

    pub fn at(container: NodeId, index: u32, affinity: Affinity, marker: NodeId) -> Self {
        Self { container, index, affinity, marker: Some(marker) }
    }

    /// 同一位置（容器与边界都相同；affinity 不参与）。
    pub fn same_place(&self, other: &Anchor) -> bool {
        self.container == other.container && self.index == other.index
    }
}

/// 范围的哪一端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpanEnd {
    Start,
    End,
}

/// 范围种类（不带数据），用于起终点配对与倒排查询。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RangeClass {
    Bookmark,
    Comment,
    Permission,
    MoveFrom,
    MoveTo,
    CustomXmlIns,
    CustomXmlDel,
    CustomXmlMoveFrom,
    CustomXmlMoveTo,
}

/// 范围种类与它携带的文档事实（`SPAN-03`，`docs/03` §5.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeKind {
    Bookmark {
        id: String,
        name: String,
        /// `_` 前缀（`_GoBack` / `_Toc…` / `_Ref…`）：Word 不在书签列表里显示。
        hidden: bool,
        /// `w:colFirst` / `w:colLast`：范围在表格中覆盖的列区间。
        cols: Option<(u32, u32)>,
    },
    Comment {
        id: String,
        /// 承载 `w:commentReference` 的 run。
        reference: Option<NodeId>,
    },
    Permission {
        id: String,
        editor: Option<String>,
        group: Option<String>,
        cols: Option<(u32, u32)>,
    },
    MoveFrom {
        id: String,
        name: String,
        meta: RevisionMeta,
    },
    MoveTo {
        id: String,
        name: String,
        meta: RevisionMeta,
    },
    CustomXmlIns {
        id: String,
        meta: RevisionMeta,
    },
    CustomXmlDel {
        id: String,
        meta: RevisionMeta,
    },
    CustomXmlMoveFrom {
        id: String,
        meta: RevisionMeta,
    },
    CustomXmlMoveTo {
        id: String,
        meta: RevisionMeta,
    },
}

impl RangeKind {
    pub fn class(&self) -> RangeClass {
        match self {
            Self::Bookmark { .. } => RangeClass::Bookmark,
            Self::Comment { .. } => RangeClass::Comment,
            Self::Permission { .. } => RangeClass::Permission,
            Self::MoveFrom { .. } => RangeClass::MoveFrom,
            Self::MoveTo { .. } => RangeClass::MoveTo,
            Self::CustomXmlIns { .. } => RangeClass::CustomXmlIns,
            Self::CustomXmlDel { .. } => RangeClass::CustomXmlDel,
            Self::CustomXmlMoveFrom { .. } => RangeClass::CustomXmlMoveFrom,
            Self::CustomXmlMoveTo { .. } => RangeClass::CustomXmlMoveTo,
        }
    }

    /// 配对键（`w:id` 原值）。
    pub fn pair_id(&self) -> &str {
        match self {
            Self::Bookmark { id, .. }
            | Self::Comment { id, .. }
            | Self::Permission { id, .. }
            | Self::MoveFrom { id, .. }
            | Self::MoveTo { id, .. }
            | Self::CustomXmlIns { id, .. }
            | Self::CustomXmlDel { id, .. }
            | Self::CustomXmlMoveFrom { id, .. }
            | Self::CustomXmlMoveTo { id, .. } => id,
        }
    }

    pub fn bookmark_name(&self) -> Option<&str> {
        match self {
            Self::Bookmark { name, .. }
            | Self::MoveFrom { name, .. }
            | Self::MoveTo { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// 范围是解析出来的还是本次会话新建的。
///
/// 决定 `SPAN-09` 校验失败时的 `origin`：解析时就有的缺陷是 `PreExistingDamage`，
/// 本次编辑造成的是 `EngineInvariantViolation`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanOrigin {
    /// 解析出来的完好范围。
    Parsed,
    /// 已损坏，且不是引擎的缺陷：解析时就孤儿 / 未闭合 / 跨流，或调用方把容器内容整体重写时
    /// 丢掉了一端（compat 的 `ReplaceInlines`）。`SPAN-09` 按 `PreExistingDamage` 处理，
    /// 因此不会让调试构建的保存失败——引擎自己弄丢的端点才会（`SAVE-02`）。
    Damaged,
    /// 本次会话新建。
    New,
}

/// 一个范围（`SPAN-03`）。
///
/// `start` / `end` 为 `None` 表示该端在 part 内缺失（损坏输入：孤儿终点 / 未闭合起点），
/// 由 `SPAN-09` 在保存前按 `PreExistingDamage` 修复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeSpan {
    pub id: SpanId,
    pub part: PartId,
    pub flow: FlowId,
    pub kind: RangeKind,
    pub origin: SpanOrigin,
    /// 文件里没有物理标记元素（只有 `commentReference` 的批注就是这样）。
    /// `SPAN-08` 物化**不得**为它插入标记，否则未编辑内容会被改写（不变式 1/2）。
    /// 与"标记本来有、被这次编辑删掉了"（`marker` 变 `None`）不是一回事：后者要重新物化。
    pub implicit: bool,
    /// 本次会话按 `SPAN-07` 整体删除。保留在索引里供撤销；物化与校验跳过。
    pub removed: bool,
    pub start: Option<Anchor>,
    pub end: Option<Anchor>,
}

impl RangeSpan {
    pub fn class(&self) -> RangeClass {
        self.kind.class()
    }

    pub fn pair_id(&self) -> &str {
        self.kind.pair_id()
    }

    pub fn anchor(&self, end: SpanEnd) -> Option<&Anchor> {
        match end {
            SpanEnd::Start => self.start.as_ref(),
            SpanEnd::End => self.end.as_ref(),
        }
    }

    pub fn anchor_mut(&mut self, end: SpanEnd) -> Option<&mut Anchor> {
        match end {
            SpanEnd::Start => self.start.as_mut(),
            SpanEnd::End => self.end.as_mut(),
        }
    }

    /// 空范围：两端在同一位置。
    pub fn is_collapsed(&self) -> bool {
        match (&self.start, &self.end) {
            (Some(s), Some(e)) => s.same_place(e),
            _ => false,
        }
    }

    /// 两端都在（不是损坏输入）。
    pub fn is_paired(&self) -> bool {
        self.start.is_some() && self.end.is_some()
    }
}

/// 一个 part 的范围索引（`SPAN-04`）。
#[derive(Debug, Clone, PartialEq)]
pub struct SpanIndex {
    part: PartId,
    flows: FlowMap,
    spans: Vec<RangeSpan>,
    by_container: HashMap<NodeId, Vec<(SpanId, SpanEnd)>>,
    diagnostics: Vec<Diagnostic>,
}

impl SpanIndex {
    /// `SPAN-04`：按内容流、按文档序扫描所有容器的语义子节点建立索引。
    pub fn build(dom: &Dom) -> SpanIndex {
        Builder::new(dom).run()
    }

    /// `SPAN-10` 的另一半：端点落在**原子形态字段的内部**（begin..end 之间）时移到原子边界
    /// ——起点移到字段之前、终点移到字段之后。与插入侧（`edit::ops::boundary_node`）同一条规则：
    /// 原子字段在坐标流里只占一个单位，端点停在它中间既表达不出来、物化时也放不回原处。
    ///
    /// 在建完索引之后跑一次（`Document::rebuild` 与 `EditSession::ensure_spans` 各一处）。
    /// 只改索引里的锚点；DOM 里的标记不动——`SPAN-09` 只物化脏容器，未编辑的文档保存仍然字节相同。
    pub fn snap_to_field_atoms(&mut self, dom: &Dom, fields: &crate::span::field::FieldIndex) {
        // `(容器, 字段首项下标, 字段末项下标)`
        let atoms: Vec<(NodeId, u32, u32)> = fields
            .fields()
            .iter()
            .filter(|f| f.is_atomic())
            .filter_map(|f| {
                let (head, tail) = (f.form.head(), f.form.tail());
                let c = crate::span::container_of(dom, head)?;
                if crate::span::container_of(dom, tail) != Some(c) {
                    return None;
                }
                let hi = crate::span::content_index_of(dom, c, head)?;
                let ti = crate::span::content_index_of(dom, c, tail)?;
                (hi < ti).then_some((c, hi, ti))
            })
            .collect();
        if atoms.is_empty() {
            return;
        }
        for span in &mut self.spans {
            for end in [SpanEnd::Start, SpanEnd::End] {
                let Some(a) = span.anchor_mut(end) else { continue };
                let Some(&(_, hi, ti)) = atoms
                    .iter()
                    .find(|&&(c, hi, ti)| c == a.container && a.index > hi && a.index <= ti)
                else {
                    continue;
                };
                a.index = if end == SpanEnd::Start { hi } else { ti + 1 };
            }
        }
    }

    pub fn part(&self) -> PartId {
        self.part
    }

    pub fn flows(&self) -> &FlowMap {
        &self.flows
    }

    pub fn spans(&self) -> &[RangeSpan] {
        &self.spans
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn get(&self, id: SpanId) -> Option<&RangeSpan> {
        self.spans.get(id.0 as usize)
    }

    pub fn get_mut(&mut self, id: SpanId) -> Option<&mut RangeSpan> {
        self.spans.get_mut(id.0 as usize)
    }

    /// 落在该容器里的所有端点（`SPAN-06` 变换的入口）。
    pub fn at_container(&self, container: NodeId) -> &[(SpanId, SpanEnd)] {
        self.by_container.get(&container).map_or(&[], |v| v.as_slice())
    }

    /// 有端点落在容器里的所有容器。
    pub fn containers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.by_container.keys().copied()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// 未被 `SPAN-07` 删除的范围。
    pub fn live(&self) -> impl Iterator<Item = &RangeSpan> + '_ {
        self.spans.iter().filter(|s| !s.removed)
    }

    pub fn iter_class(&self, class: RangeClass) -> impl Iterator<Item = &RangeSpan> + '_ {
        self.spans.iter().filter(move |s| !s.removed && s.class() == class)
    }

    /// 追加一个范围，返回它的 id。
    pub(crate) fn push_span(&mut self, span: RangeSpan) -> SpanId {
        let id = SpanId(self.spans.len() as u32);
        self.spans.push(RangeSpan { id, ..span });
        id
    }

    /// 结构变化后重建流映射（`SPAN-01`：子树移动跨越流根时缓存必须失效）。
    pub(crate) fn refresh_flows(&mut self, dom: &Dom) {
        self.flows = FlowMap::build(dom);
        for s in &mut self.spans {
            if let Some(f) = s.start.or(s.end).and_then(|a| self.flows.flow_of(a.container)) {
                s.flow = f;
            }
        }
    }

    /// 重建按容器的倒排表。
    pub(crate) fn reindex_containers(&mut self) {
        self.by_container = invert(&self.spans);
    }

    /// 按 `w:id` 找范围（同 id 重复时返回第一个；跳过已删除的）。
    pub fn find(&self, class: RangeClass, id: &str) -> Option<&RangeSpan> {
        self.spans.iter().find(|s| !s.removed && s.class() == class && s.pair_id() == id)
    }

    pub fn flow_of(&self, anchor: &Anchor) -> Option<FlowId> {
        self.flows.flow_of(anchor.container)
    }

    /// `SPAN-05` 文档序。`None` = 不可比（跨流，或容器不在同一棵可达子树上）。
    pub fn compare(&self, dom: &Dom, a: &Anchor, b: &Anchor) -> Option<Ordering> {
        compare(dom, &self.flows, a, b)
    }

    /// 起点是否不在终点之后（`SPAN-05`）。缺端点的范围返回 `false`。
    ///
    /// 两端在同一位置（空范围）一律算有序：affinity 的 `Left < Right` 只用来给同一边界上的
    /// 不同范围排序，不该把空范围判成反序（`SPAN-02` 的空范围例外）。
    pub fn is_ordered(&self, dom: &Dom, span: &RangeSpan) -> bool {
        let (Some(s), Some(e)) = (&span.start, &span.end) else { return false };
        if s.same_place(e) {
            return self.flow_of(s).is_some() && self.flow_of(s) == self.flow_of(e);
        }
        matches!(self.compare(dom, s, e), Some(Ordering::Less | Ordering::Equal))
    }
}

/// `SPAN-05`：Anchor 的文档序。
///
/// 1. 跨流（或任一端不在任何流里）→ `None`（`SPAN_CROSS_FLOW`）。
/// 2. 同容器：比较 `index`，相等则 `Left < Right`。
/// 3. 不同容器：一侧是另一侧的祖先时，用祖先侧的 `index` 与"包含另一侧的内容项"下标比较；
///    否则比较两条路径在最近公共祖先里的分叉子序号。
pub fn compare(dom: &Dom, flows: &FlowMap, a: &Anchor, b: &Anchor) -> Option<Ordering> {
    let (fa, fb) = (flows.flow_of(a.container), flows.flow_of(b.container));
    match (fa, fb) {
        (Some(x), Some(y)) if x == y => {}
        _ => return None,
    }
    if a.container == b.container {
        return Some(a.index.cmp(&b.index).then_with(|| affinity_rank(a).cmp(&affinity_rank(b))));
    }
    let pa = path(dom, a.container);
    let pb = path(dom, b.container);
    let mut l = 0;
    while l < pa.len() && l < pb.len() && pa[l] == pb[l] {
        l += 1;
    }
    if l == 0 {
        return None; // 不在同一棵树上
    }
    if l == pa.len() {
        // a 的容器是 b 的容器的祖先：b 落在 a 容器的某个内容项内部
        let j = item_containing(dom, a.container, b.container)?;
        return Some(if a.index <= j { Ordering::Less } else { Ordering::Greater });
    }
    if l == pb.len() {
        let j = item_containing(dom, b.container, a.container)?;
        return Some(if b.index <= j { Ordering::Greater } else { Ordering::Less });
    }
    let common = pa[l - 1];
    let kids = dom.children(common);
    let ia = kids.iter().position(|&x| x == pa[l])?;
    let ib = kids.iter().position(|&x| x == pb[l])?;
    Some(ia.cmp(&ib))
}

fn affinity_rank(a: &Anchor) -> u8 {
    match a.affinity {
        Affinity::Left => 0,
        Affinity::Right => 1,
    }
}

/// 根到 `node` 的路径（含两端）。
fn path(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = std::iter::once(node).chain(dom.ancestors(node)).collect();
    v.reverse();
    v
}

/// `w:id`（配对键与修订 id）。
pub(crate) fn w_id() -> QName {
    QName::new(NsId::W, LocalName::Id)
}

/// `w:commentReference`。
pub(crate) fn comment_ref_name() -> QName {
    QName::new(NsId::W, LocalName::CommentReference)
}

/// 元素名 → （种类, 哪一端）；不是范围标记则 `None`。
pub(crate) fn classify(name: QName) -> Option<(RangeClass, SpanEnd)> {
    (name.ns == NsId::W).then(|| classify_marker(name.local)).flatten()
}

fn attr_of(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::W, local)).map(|v| v.into_owned())
}

fn attr_u32_of(dom: &Dom, node: NodeId, local: LocalName) -> Option<u32> {
    attr_of(dom, node, local)?.trim().parse().ok()
}

/// 从标记元素的属性读出 `RangeKind`（`SPAN-03`）。
pub(crate) fn read_kind_of(dom: &Dom, class: RangeClass, marker: NodeId, id: String) -> RangeKind {
    let attr = |l: LocalName| attr_of(dom, marker, l);
    let cols = || match (
        attr_u32_of(dom, marker, LocalName::ColFirst),
        attr_u32_of(dom, marker, LocalName::ColLast),
    ) {
        (Some(a), Some(b)) => Some((a, b)),
        (Some(a), None) => Some((a, a)),
        (None, Some(b)) => Some((b, b)),
        (None, None) => None,
    };
    let meta = || RevisionMeta {
        node: marker,
        id: attr(LocalName::Id),
        author: attr(LocalName::Author),
        date: attr(LocalName::Date),
    };
    let name = || attr(LocalName::Name).unwrap_or_default();
    match class {
        RangeClass::Bookmark => {
            let name = name();
            RangeKind::Bookmark { id, hidden: name.starts_with('_'), name, cols: cols() }
        }
        RangeClass::Comment => RangeKind::Comment { id, reference: None },
        RangeClass::Permission => RangeKind::Permission {
            id,
            editor: attr(LocalName::Ed),
            group: attr(LocalName::EdGrp),
            cols: cols(),
        },
        RangeClass::MoveFrom => RangeKind::MoveFrom { id, name: name(), meta: meta() },
        RangeClass::MoveTo => RangeKind::MoveTo { id, name: name(), meta: meta() },
        RangeClass::CustomXmlIns => RangeKind::CustomXmlIns { id, meta: meta() },
        RangeClass::CustomXmlDel => RangeKind::CustomXmlDel { id, meta: meta() },
        RangeClass::CustomXmlMoveFrom => RangeKind::CustomXmlMoveFrom { id, meta: meta() },
        RangeClass::CustomXmlMoveTo => RangeKind::CustomXmlMoveTo { id, meta: meta() },
    }
}

/// 标记元素 → （种类, 哪一端）。
fn classify_marker(local: LocalName) -> Option<(RangeClass, SpanEnd)> {
    use LocalName as L;
    use RangeClass as C;
    use SpanEnd::{End, Start};
    Some(match local {
        L::BookmarkStart => (C::Bookmark, Start),
        L::BookmarkEnd => (C::Bookmark, End),
        L::CommentRangeStart => (C::Comment, Start),
        L::CommentRangeEnd => (C::Comment, End),
        L::PermStart => (C::Permission, Start),
        L::PermEnd => (C::Permission, End),
        L::MoveFromRangeStart => (C::MoveFrom, Start),
        L::MoveFromRangeEnd => (C::MoveFrom, End),
        L::MoveToRangeStart => (C::MoveTo, Start),
        L::MoveToRangeEnd => (C::MoveTo, End),
        L::CustomXmlInsRangeStart => (C::CustomXmlIns, Start),
        L::CustomXmlInsRangeEnd => (C::CustomXmlIns, End),
        L::CustomXmlDelRangeStart => (C::CustomXmlDel, Start),
        L::CustomXmlDelRangeEnd => (C::CustomXmlDel, End),
        L::CustomXmlMoveFromRangeStart => (C::CustomXmlMoveFrom, Start),
        L::CustomXmlMoveFromRangeEnd => (C::CustomXmlMoveFrom, End),
        L::CustomXmlMoveToRangeStart => (C::CustomXmlMoveTo, Start),
        L::CustomXmlMoveToRangeEnd => (C::CustomXmlMoveTo, End),
        _ => return None,
    })
}

// ---- 构建 ----

struct OpenStart {
    flow: FlowId,
    span: SpanId,
}

struct CommentRef {
    id: String,
    container: NodeId,
    index: u32,
    run: NodeId,
}

/// 遍历栈的一层。
enum Frame {
    /// 容器：按 `kids`（语义子节点，含标记）推进，`index` 是下一个内容项的下标。
    Container { container: NodeId, kids: Vec<NodeId>, next: usize, index: u32 },
    /// 内容项子树扫描：找下一层容器（`w:sdt` / `w:tbl` / `w:hyperlink` … 都可能包着容器）。
    Scan { pending: Vec<NodeId> },
}

/// 一步动作：先从栈顶算出来，再执行，避免同时可变借用栈。
enum Step {
    Pop,
    Skip,
    /// (容器, 边界, 标记)
    Marker(NodeId, u32, NodeId),
    /// (容器, 边界, 内容项)
    Item(NodeId, u32, NodeId),
    /// 扫描到的节点
    Visit(NodeId),
}

struct Builder<'d> {
    dom: &'d Dom,
    part: PartId,
    spans: Vec<RangeSpan>,
    diagnostics: Vec<Diagnostic>,
    open: HashMap<(RangeClass, String), Vec<OpenStart>>,
    refs: Vec<CommentRef>,
}

impl<'d> Builder<'d> {
    fn new(dom: &'d Dom) -> Self {
        Self {
            dom,
            part: dom.part(),
            spans: Vec::new(),
            diagnostics: Vec::new(),
            open: HashMap::new(),
            refs: Vec::new(),
        }
    }

    fn run(mut self) -> SpanIndex {
        let flows = FlowMap::build(self.dom);
        for i in 0..flows.flow_count() {
            let flow = FlowId(i as u32);
            self.walk_flow(flow, flows.root_of(flow));
        }
        self.close_unclosed();
        self.attach_comment_refs(&flows);
        let by_container = invert(&self.spans);
        SpanIndex {
            part: self.part,
            flows,
            spans: self.spans,
            by_container,
            diagnostics: self.diagnostics,
        }
    }

    /// 一个流的文档序遍历（迭代：语料里有几千层嵌套）。
    fn walk_flow(&mut self, flow: FlowId, root: NodeId) {
        let mut stack = vec![self.container_frame(root)];
        while !stack.is_empty() {
            let step = match stack.last_mut().expect("stack is not empty") {
                Frame::Container { container, kids, next, index } => {
                    if *next >= kids.len() {
                        Step::Pop
                    } else {
                        let kid = kids[*next];
                        *next += 1;
                        let (c, i) = (*container, *index);
                        match self.dom.name(kid) {
                            Some(q) if is_range_marker(q) => Step::Marker(c, i, kid),
                            Some(q) if is_property_element(q) => Step::Skip,
                            Some(_) => {
                                *index += 1;
                                Step::Item(c, i, kid)
                            }
                            // 文本 / Opaque 不是内容项
                            None => Step::Skip,
                        }
                    }
                }
                Frame::Scan { pending } => match pending.pop() {
                    Some(n) => Step::Visit(n),
                    None => Step::Pop,
                },
            };
            match step {
                Step::Pop => {
                    stack.pop();
                }
                Step::Skip => {}
                Step::Marker(c, i, m) => self.on_marker(flow, c, i, m),
                Step::Item(c, i, node) => {
                    self.note_comment_reference(c, i, node);
                    stack.push(Frame::Scan { pending: vec![node] });
                }
                Step::Visit(n) => {
                    let name = self.dom.name(n);
                    if name.is_some_and(is_flow_root) {
                        continue; // 独立流，由外层循环单独遍历
                    }
                    if name.is_some_and(is_content_container) {
                        stack.push(self.container_frame(n));
                    } else if let Some(Frame::Scan { pending }) = stack.last_mut() {
                        let start = pending.len();
                        pending.extend(self.dom.semantic_children(n));
                        pending[start..].reverse();
                    }
                }
            }
        }
    }

    fn container_frame(&self, container: NodeId) -> Frame {
        Frame::Container {
            container,
            kids: self.dom.semantic_children(container).collect(),
            next: 0,
            index: 0,
        }
    }

    fn on_marker(&mut self, flow: FlowId, container: NodeId, index: u32, marker: NodeId) {
        let Some(q) = self.dom.name(marker) else { return };
        let Some((class, which)) = classify_marker(q.local) else { return };
        let key = attr_of(self.dom, marker, LocalName::Id).unwrap_or_default();
        match which {
            SpanEnd::Start => {
                let id = SpanId(self.spans.len() as u32);
                let kind = self.read_kind(class, marker, key.clone());
                self.spans.push(RangeSpan {
                    id,
                    part: self.part,
                    flow,
                    kind,
                    origin: SpanOrigin::Parsed,
                    implicit: false,
                    removed: false,
                    start: Some(Anchor::at(container, index, Affinity::Right, marker)),
                    end: None,
                });
                if self.open.get(&(class, key.clone())).is_some_and(|s| !s.is_empty()) {
                    self.diag(
                        marker,
                        DiagCode::SpanDupStart,
                        format!("{class:?} w:id=\"{key}\" 的起点重复出现，后者按新范围处理"),
                    );
                }
                self.open.entry((class, key)).or_default().push(OpenStart { flow, span: id });
            }
            SpanEnd::End => {
                let anchor = Anchor::at(container, index, Affinity::Left, marker);
                if let Some(slot) = self.open.get_mut(&(class, key.clone())) {
                    if let Some(pos) = slot.iter().rposition(|o| o.flow == flow) {
                        let open = slot.remove(pos);
                        self.close(open.span, anchor);
                        return;
                    }
                    if !slot.is_empty() {
                        // 起点在另一个内容流里：范围禁止跨流，两端都按损坏处理
                        self.diag(
                            marker,
                            DiagCode::SpanCrossFlow,
                            format!("{class:?} w:id=\"{key}\" 的起点在另一个内容流里"),
                        );
                        self.orphan_end(flow, class, key, marker, anchor);
                        return;
                    }
                }
                self.diag(
                    marker,
                    DiagCode::SpanOrphanEnd,
                    format!("{class:?} w:id=\"{key}\" 的终点找不到起点"),
                );
                self.orphan_end(flow, class, key, marker, anchor);
            }
        }
    }

    /// 闭合一个起点。两端落在同一位置（空范围）时把终点的 affinity 也设为 `Right`：
    /// 否则 `SPAN-05` 的 `Left < Right` 会判定"起在终后"，而且边界插入会把空范围拆反
    /// （起点右移、终点不动）。空范围整体吸附右侧内容，与"起点默认 `Right`"一致。
    fn close(&mut self, span: SpanId, mut end: Anchor) {
        let s = &mut self.spans[span.0 as usize];
        if s.start.as_ref().is_some_and(|st| st.same_place(&end)) {
            end.affinity = Affinity::Right;
        }
        s.end = Some(end);
    }

    fn orphan_end(
        &mut self,
        flow: FlowId,
        class: RangeClass,
        key: String,
        marker: NodeId,
        anchor: Anchor,
    ) {
        let id = SpanId(self.spans.len() as u32);
        let kind = self.read_kind(class, marker, key);
        self.spans.push(RangeSpan {
            id,
            part: self.part,
            flow,
            kind,
            origin: SpanOrigin::Damaged,
            implicit: false,
            removed: false,
            start: None,
            end: Some(anchor),
        });
    }

    /// part 扫完后仍未闭合的起点（`SPAN-04` 第 3 条）。
    ///
    /// 规范写的是"流结束"，实现推迟到 part 结束：这样后面流里出现的同 id 终点还能被识别为
    /// 跨流配对（`SPAN_CROSS_FLOW`），而不是退化成一对"未闭合 + 孤儿终点"。范围集合不变。
    fn close_unclosed(&mut self) {
        let mut pending: Vec<(RangeClass, String, SpanId)> = Vec::new();
        for ((class, key), slot) in &self.open {
            for open in slot {
                pending.push((*class, key.clone(), open.span));
            }
        }
        pending.sort_by_key(|(_, _, span)| span.0);
        for (class, key, span) in pending {
            self.spans[span.0 as usize].origin = SpanOrigin::Damaged;
            let marker = self.spans[span.0 as usize].start.and_then(|a| a.marker);
            let node = marker.unwrap_or(self.dom.root());
            self.diag(
                node,
                DiagCode::SpanUnclosed,
                format!("{class:?} w:id=\"{key}\" 的起点在 part 结束时仍未闭合"),
            );
        }
        self.open.clear();
    }

    /// 记下承载 `w:commentReference` 的 run 及其内容坐标（`SPAN-03` / `SPAN-04` 第 5 条）。
    fn note_comment_reference(&mut self, container: NodeId, index: u32, item: NodeId) {
        for c in self.dom.semantic_children(item) {
            if self.dom.is(c, QName::w(LocalName::CommentReference)) {
                let id = attr_of(self.dom, c, LocalName::Id).unwrap_or_default();
                self.refs.push(CommentRef { id, container, index, run: item });
            }
        }
    }

    /// 把 reference run 挂到批注范围上；只有 reference 没有范围标记的批注生成折叠范围。
    fn attach_comment_refs(&mut self, flows: &FlowMap) {
        let refs = std::mem::take(&mut self.refs);
        for r in refs {
            let existing = self.spans.iter_mut().find(|s| {
                matches!(&s.kind, RangeKind::Comment { id, reference } if *id == r.id && reference.is_none())
            });
            if let Some(span) = existing {
                if let RangeKind::Comment { reference, .. } = &mut span.kind {
                    *reference = Some(r.run);
                }
                continue;
            }
            if self.spans.iter().any(
                |s| matches!(&s.kind, RangeKind::Comment { id, reference } if *id == r.id && *reference == Some(r.run)),
            ) {
                continue;
            }
            // LibreOffice 风格：没有 commentRangeStart/End，只有 reference run。
            // 折叠范围放在 reference run 之前的边界；两端 `marker: None`，物化不得为它补标记。
            let anchor = Anchor::new(r.container, r.index, Affinity::Right);
            let id = SpanId(self.spans.len() as u32);
            self.spans.push(RangeSpan {
                id,
                part: self.part,
                flow: flows.flow_of(r.container).unwrap_or(FlowId(0)),
                kind: RangeKind::Comment { id: r.id, reference: Some(r.run) },
                origin: SpanOrigin::Parsed,
                implicit: true,
                removed: false,
                start: Some(anchor),
                end: Some(anchor),
            });
        }
    }

    fn read_kind(&self, class: RangeClass, marker: NodeId, id: String) -> RangeKind {
        read_kind_of(self.dom, class, marker, id)
    }

    fn diag(&mut self, node: NodeId, code: DiagCode, message: String) {
        let range = self.dom.node(node).lex.as_ref().map(|l| l.range.clone());
        self.diagnostics.push(Diagnostic::pre_existing(self.part, range, code, message));
    }
}

fn invert(spans: &[RangeSpan]) -> HashMap<NodeId, Vec<(SpanId, SpanEnd)>> {
    let mut map: HashMap<NodeId, Vec<(SpanId, SpanEnd)>> = HashMap::new();
    for s in spans {
        for which in [SpanEnd::Start, SpanEnd::End] {
            if let Some(a) = s.anchor(which) {
                map.entry(a.container).or_default().push((s.id, which));
            }
        }
    }
    map
}

impl SpanIndex {
    /// 按容器里现有的标记重建该容器的端点（`SPAN-06` 的 `rescan`）。
    ///
    /// 只用于内容被外部描述**整体重写**的容器（compat 的 `ReplaceInlines` 会按 `commentIds`
    /// 重发批注标记）：这时那个容器里标记的位置才是真相。其余情形一律禁止由标记反推 Anchor
    /// （`SPAN-02`）。容器里配不上对的标记先尝试**认领**索引里刚刚失去这一端的跨容器范围
    /// （调用方对那些范围先做了 `Drop`），认领不到才留成半开范围并记诊断。
    pub(crate) fn rescan_container(&mut self, dom: &Dom, container: NodeId) {
        let part = self.part;
        let flow = self.flows.flow_of(container).unwrap_or(FlowId(0));
        let mut index = 0u32;
        let mut open: Vec<(RangeClass, String, SpanId)> = Vec::new();
        let mut orphans: Vec<(RangeClass, String, SpanId)> = Vec::new();
        let mut refs: Vec<(String, NodeId)> = Vec::new();
        for c in dom.semantic_children(container).collect::<Vec<_>>() {
            let Some(q) = dom.name(c) else { continue };
            if is_property_element(q) {
                continue;
            }
            let Some((class, which)) = classify(q) else {
                // 内容项：记下承载 commentReference 的 run，再前进一个边界
                if let Some(r) = dom.semantic_children(c).find(|&g| dom.is(g, comment_ref_name())) {
                    refs.push((
                        dom.attr_value(r, w_id()).map(|v| v.into_owned()).unwrap_or_default(),
                        c,
                    ));
                }
                index += 1;
                continue;
            };
            let id = dom.attr_value(c, w_id()).map(|v| v.into_owned()).unwrap_or_default();
            let kind = read_kind_of(dom, class, c, id.clone());
            match which {
                SpanEnd::Start => {
                    let span = self.push_span(RangeSpan {
                        id: SpanId(0),
                        part,
                        flow,
                        kind,
                        origin: SpanOrigin::New,
                        implicit: false,
                        removed: false,
                        start: Some(Anchor::at(container, index, Affinity::Right, c)),
                        end: None,
                    });
                    open.push((class, id, span));
                }
                SpanEnd::End => {
                    let mut anchor = Anchor::at(container, index, Affinity::Left, c);
                    match open.iter().rposition(|(k, i, _)| *k == class && *i == id) {
                        Some(pos) => {
                            let (_, _, span) = open.remove(pos);
                            let s = &mut self.spans[span.0 as usize];
                            if s.start.is_some_and(|st| st.same_place(&anchor)) {
                                anchor.affinity = Affinity::Right;
                            }
                            s.end = Some(anchor);
                        }
                        None => {
                            let span = self.push_span(RangeSpan {
                                id: SpanId(0),
                                part,
                                flow,
                                kind,
                                origin: SpanOrigin::New,
                                implicit: false,
                                removed: false,
                                start: None,
                                end: Some(anchor),
                            });
                            orphans.push((class, id, span));
                        }
                    }
                }
            }
        }
        for (class, id, span) in open {
            self.settle_unpaired(container, span, SpanEnd::Start, class, &id);
        }
        for (class, id, span) in orphans {
            self.settle_unpaired(container, span, SpanEnd::End, class, &id);
        }
        for (id, run) in refs {
            let hit = self.spans.iter_mut().find(|s| {
                !s.removed
                    && s.start.is_some_and(|a| a.container == container)
                    && matches!(&s.kind, RangeKind::Comment { id: cid, .. } if *cid == id)
            });
            if let Some(s) = hit
                && let RangeKind::Comment { reference, .. } = &mut s.kind
            {
                *reference = Some(run);
            }
        }
    }

    /// 重写容器里配不上对的标记：先认领索引里刚失去这一端的跨容器范围，认领不到就记诊断。
    fn settle_unpaired(
        &mut self,
        container: NodeId,
        local: SpanId,
        which: SpanEnd,
        class: RangeClass,
        id: &str,
    ) {
        let anchor = self.spans[local.0 as usize].anchor(which).copied();
        let target = anchor.and_then(|_| {
            self.spans
                .iter()
                .position(|s| {
                    !s.removed
                        && s.id != local
                        && s.class() == class
                        && s.pair_id() == id
                        && s.anchor(which).is_none()
                        && s.anchor(other_end(which)).is_some_and(|o| o.container != container)
                })
                .map(|i| SpanId(self.spans[i].id.0))
        });
        match (target, anchor) {
            (Some(t), Some(a)) => {
                // 跨容器范围复原：重发的标记就是它这一端
                let s = &mut self.spans[t.0 as usize];
                match which {
                    SpanEnd::Start => s.start = Some(a),
                    SpanEnd::End => s.end = Some(a),
                }
                self.spans[local.0 as usize].removed = true;
            }
            _ => {
                let code = match which {
                    SpanEnd::Start => DiagCode::SpanUnclosed,
                    SpanEnd::End => DiagCode::SpanOrphanEnd,
                };
                // 调用方重发的标记配不上对：是那份描述的缺陷，不是引擎的（`SAVE-02` 因此
                // 不该让调试构建的保存失败）。范围标成 `Damaged`，物化时按半开处理。
                self.spans[local.0 as usize].origin = SpanOrigin::Damaged;
                self.diagnostics.push(Diagnostic::pre_existing(
                    self.part,
                    None,
                    code,
                    format!("重写的容器里 {class:?} w:id=\"{id}\" 的标记配不上对"),
                ));
            }
        }
    }
}

fn other_end(end: SpanEnd) -> SpanEnd {
    match end {
        SpanEnd::Start => SpanEnd::End,
        SpanEnd::End => SpanEnd::Start,
    }
}

impl SpanIndex {
    /// 变换后两端落到同一位置的范围，affinity 统一为 `Right`（`SPAN-02` 的空范围例外）。
    ///
    /// 否则 `Left < Right` 会把空范围判成"起在终后"，而且在该边界插入内容会把它拆反
    /// （起点右移、终点不动）。
    pub(crate) fn normalize_collapsed(&mut self) {
        for s in &mut self.spans {
            if s.removed {
                continue;
            }
            let (Some(a), Some(b)) = (s.start, s.end) else { continue };
            if a.same_place(&b) && (a.affinity != Affinity::Right || b.affinity != Affinity::Right)
            {
                s.start = Some(Anchor { affinity: Affinity::Right, ..a });
                s.end = Some(Anchor { affinity: Affinity::Right, ..b });
            }
        }
    }
}
