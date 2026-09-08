//! 修订生成（`EDIT-03` 每个操作的「修订」行；`spec/18` 7.2 / 7.3）。
//!
//! **修订是计划的一部分，不是事后包装**（`spec/18` 分层决策 1）：包裹、改名、快照都是普通
//! [`NodeEdit`]，由 [`Tracker`] 在 plan 阶段生成，`MutationPlan` 不加字段。`w:id` 从会话里全包
//! 修订的最大值 + 1 起顺序发放（`EDIT-06`）；`w:date` 是 [`RevisionAuthor::date`] 的原串——
//! 引擎里没有时钟（不变式 1 与可复现性）。
//!
//! **同作者规则按 Word**（分层决策 2，作者相等 = `w:author` 字符串相等，不看 `w:initials`）：
//! 自己插的可以直接改、直接删；别人插的删了是 `w:ins` 里套 `w:del`；删除区里不能再打字。

use crate::diag::DiagCode;
use crate::error::Error;
use crate::semantic::props::order_index_run_props;
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NewNode, NodeEdit, NodeId, NsId, QName, Target,
};

use super::plan::MutationPlan;
use super::{EditContext, RevisionAuthor};

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// `container_mark` 的落点：属性容器挂在谁下面、容器不存在时插在哪。
#[derive(Debug, Clone, Copy)]
pub(crate) struct MarkSite {
    /// `w:tr` / `w:tc` / `w:tbl`。
    pub owner: NodeId,
    /// `w:trPr` / `w:tcPr` / `w:tblPr`。
    pub container: LocalName,
    /// 容器不存在时插在这个兄弟之前（`None` = 追加到末尾）。
    pub container_before: Option<NodeId>,
}

/// 插入 / 删除位置外面罩着什么修订包裹（只看到段落为止）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackSite {
    /// 没有包裹。
    Clean,
    /// 在**本作者**的 `w:ins` / `w:moveTo` 里（最内层那个）。
    OwnIns(NodeId),
    /// 在**别人**的 `w:ins` / `w:moveTo` 里。
    OtherIns(NodeId),
    /// 在 `w:del` / `w:moveFrom` 里。
    Deleted(NodeId),
}

/// 从 `node` 往上走到 `stop`（段落）为止，最内层的修订包裹是什么。
pub(crate) fn site_of(dom: &Dom, node: NodeId, stop: NodeId, author: &str) -> TrackSite {
    let mut x = Some(node);
    while let Some(n) = x {
        if n == stop {
            break;
        }
        if let Some(name) = dom.name(n)
            && name.ns == NsId::W
        {
            match name.local {
                LocalName::Del | LocalName::MoveFrom => return TrackSite::Deleted(n),
                LocalName::Ins | LocalName::MoveTo => {
                    let same = dom
                        .attr_value(n, w(LocalName::Author))
                        .is_some_and(|a| a.as_ref() == author);
                    return if same { TrackSite::OwnIns(n) } else { TrackSite::OtherIns(n) };
                }
                _ => {}
            }
        }
        x = dom.parent(n);
    }
    TrackSite::Clean
}

/// 落在删除区里不能再打字（Word 的规则）。
pub(crate) fn err_in_deleted() -> Error {
    Error::edit(DiagCode::EditInDeleted, "位置落在已删除的文字里，追踪时不能插入")
}

/// 计划阶段的修订生成器。
#[derive(Debug, Clone)]
pub(crate) struct Tracker {
    pub author: String,
    pub date: Option<String>,
    /// 下一个 `w:id`（`EDIT-06`：全包最大值 + 1 起）。
    next_id: u32,
}

impl Tracker {
    /// `track_changes` 开着才有；`w:id` 的起点来自 [`crate::model::Document::revisions`]
    /// 的全包最大值（任何 part 变脏后索引会重扫，所以每个阶段拿到的都是当前值）。
    pub(crate) fn new(doc: &crate::model::Document, ctx: &EditContext) -> Option<Tracker> {
        let RevisionAuthor { author, date } = ctx.track_changes.as_ref()?;
        Some(Tracker {
            author: author.clone(),
            date: date.clone(),
            next_id: doc.revisions.max_w_id().unwrap_or(0).saturating_add(1),
        })
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// 一个空的修订元素（`w:ins` / `w:del` / `w:moveFrom` / `w:moveTo` / `*PrChange` …），
    /// 带 `w:id` / `w:author` / `w:date`。
    pub(crate) fn marker(&mut self, local: LocalName) -> NewElement {
        let mut e = NewElement::new(w(local));
        e.push_attr(w(LocalName::Id), self.take_id().to_string());
        e.push_attr(w(LocalName::Author), self.author.clone());
        if let Some(d) = &self.date {
            e.push_attr(w(LocalName::Date), d.clone());
        }
        e
    }

    /// 与 `node` 同名同属性、但换一个新 `w:id` 的元素（拆开别人的 `w:ins` 时给右半用）。
    pub(crate) fn clone_marker(&mut self, dom: &Dom, node: NodeId) -> NewElement {
        let name = dom.name(node).expect("clone_marker on an element");
        let mut e = NewElement::new(name);
        e.push_attr(w(LocalName::Id), self.take_id().to_string());
        if let Some(el) = dom.element(node) {
            for a in &el.attrs {
                if a.name != w(LocalName::Id) {
                    e.push_attr(a.name, dom.attr_str(a).into_owned());
                }
            }
        }
        e
    }

    /// 把**一个**内容项原地包进新的 `local`（`w:ins` / `w:del`）里：包裹插在它原来的位置，
    /// 它自己搬进去。返回包裹在 `plan.node_edits` 里的下标。
    ///
    /// 一项一个包裹（Word 会把连着的几个 run 合成一个 `w:del`，我们不合并）：这样容器的
    /// **内容序列长度不变**，范围锚点一个都不用动——`plan.span.rewraps` 让 `SPAN-06`
    /// 的通用推导跳过这两条编辑。形态上多几个包裹，语义完全一样。
    pub(crate) fn wrap_item(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        node: NodeId,
        local: LocalName,
    ) -> Option<usize> {
        let parent = dom.parent(node)?;
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before: Some(node),
            node: self.marker(local),
        });
        plan.node_edits.push(NodeEdit::Move { node, parent: Target::New(k), before: None });
        plan.span.rewraps.push(node);
        Some(k)
    }

    /// 段落标记的插入 / 删除：`pPr/rPr` 里首位放 `w:ins` / `w:del`（`PROP-05`：`run.toml` 的
    /// `order` 把这四个放在最前）。`pPr` / `rPr` 缺就顺手建。
    pub(crate) fn para_mark(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        para: NodeId,
        local: LocalName,
    ) {
        let ppr = super::ops::ppr_of(dom, para);
        let rpr = ppr.and_then(|p| child_named(dom, p, w(LocalName::RPr)));
        // 已经有同种标记就不重复加
        if let Some(r) = rpr
            && live_children(dom, r).any(|c| dom.is(c, w(local)))
        {
            return;
        }
        let marker = self.marker(local);
        match (ppr, rpr) {
            (_, Some(r)) => {
                // `rPr` 已在：插到第一个 order 更靠后的子元素之前
                let before = live_children(dom, r).find(|&c| {
                    dom.name(c)
                        .and_then(order_index_run_props)
                        .is_none_or(|i| i > order_index_run_props(w(local)).unwrap_or(0))
                });
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(r),
                    before,
                    node: marker,
                });
            }
            (Some(p), None) => {
                // `pPr` 在但没有 `rPr`：`w:rPr` 是 `w:pPr` 的**最后**一个子元素（CT_PPr），
                // 只有 `w:sectPr` / `w:pPrChange` 排在它后面
                let before = live_children(dom, p).find(|&c| {
                    dom.is(c, w(LocalName::SectPr)) || dom.is(c, w(LocalName::PPrChange))
                });
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(p),
                    before,
                    node: NewElement::new(w(LocalName::RPr)).with_child(marker),
                });
            }
            (None, None) => {
                // 连 `pPr` 都没有：它是 `w:p` 的第一个子元素
                let before = live_children(dom, para).next();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(para),
                    before,
                    node: NewElement::new(w(LocalName::PPr))
                        .with_child(NewElement::new(w(LocalName::RPr)).with_child(marker)),
                });
            }
        }
    }

    /// 属性容器（`w:trPr` / `w:tcPr` / `w:tblPr`）里的标记：`w:ins` / `w:del` / `w:cellIns` /
    /// `w:cellDel`。容器缺就按 `container_before` 建；已有同种标记就什么都不做。
    ///
    /// `order` 是那张属性表生成的 `order_index_*`（`PROP-05`）：新标记插在第一个 order 更靠后的
    /// 子元素之前。
    pub(crate) fn container_mark(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        at: MarkSite,
        mark: LocalName,
        order: fn(QName) -> Option<u16>,
    ) {
        let MarkSite { owner, container, container_before } = at;
        let existing = child_named(dom, owner, w(container));
        if let Some(c) = existing
            && live_children(dom, c).any(|x| dom.is(x, w(mark)))
        {
            return;
        }
        let marker = self.marker(mark);
        match existing {
            Some(c) => {
                let mine = order(w(mark)).unwrap_or(0);
                // 只看元素：容器里常有缩进用的空白文本节点，`dom.name` 给 `None`，
                // 按"次序未知就插在它前面"会把标记塞到最前面（`PROP-05` 顺序自检会拦下来）
                let before = live_children(dom, c)
                    .filter(|&x| dom.element(x).is_some())
                    .find(|&x| dom.name(x).and_then(order).is_none_or(|i| i > mine));
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(c),
                    before,
                    node: marker,
                });
            }
            None => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(owner),
                before: container_before,
                node: NewElement::new(w(container)).with_child(marker),
            }),
        }
    }

    /// `*PrChange` 旧值快照：`container` 现有子元素的 `Clean` 克隆，排除 `*Change` 自己与
    /// `skip` 列出的字段（`in_change = false`，如 `sectPr` 的页眉页脚引用、`pPr` 里的 `rPr`）。
    /// 插在 `container` 末尾（`PROP-05`：`*Change` 是每张表 `order` 的最后一项）。
    ///
    /// 容器里已经有 `change` 时什么都不做——Word 保留**最早**的那份快照。
    pub(crate) fn snapshot(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        container: NodeId,
        change: LocalName,
        inner: LocalName,
        skip: &[LocalName],
    ) -> Option<usize> {
        if live_children(dom, container).any(|c| dom.is(c, w(change))) {
            return None;
        }
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: None,
            node: self.marker(change),
        });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(k),
            before: None,
            node: NewElement::new(w(inner)),
        });
        for c in live_children(dom, container).collect::<Vec<_>>() {
            let Some(name) = dom.name(c) else { continue };
            if name.ns == NsId::W && (is_change_element(name.local) || skip.contains(&name.local)) {
                continue;
            }
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k + 1),
                before: None,
                source: c,
            });
        }
        Some(k)
    }

    /// `w:delText → w:t`、`w:delInstrText → w:instrText`（拒绝删除修订时改回去，7.4）。
    pub(crate) fn rename_to_live(plan: &mut MutationPlan, dom: &Dom, root: NodeId) {
        rename_text(plan, dom, root, false);
    }

    /// `w:t → w:delText`、`w:instrText → w:delInstrText`（run 进 `w:del` 之后必须改名，
    /// 否则 Word 会把删除的文字当正文显示）。反方向（拒绝修订）用同一张表。
    pub(crate) fn rename_to_deleted(plan: &mut MutationPlan, dom: &Dom, root: NodeId) {
        rename_text(plan, dom, root, true);
    }
}

fn rename_text(plan: &mut MutationPlan, dom: &Dom, root: NodeId, to_deleted: bool) {
    // 反方向（`w:delText → w:t`）由 7.4 的拒绝修订使用，同一张表
    let table: [(LocalName, LocalName); 2] = if to_deleted {
        [(LocalName::T, LocalName::DelText), (LocalName::InstrText, LocalName::DelInstrText)]
    } else {
        [(LocalName::DelText, LocalName::T), (LocalName::DelInstrText, LocalName::InstrText)]
    };
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        stack.extend(e.children.iter().rev());
        if e.name.ns != NsId::W {
            continue;
        }
        if let Some(&(_, to)) = table.iter().find(|(from, _)| *from == e.name.local) {
            plan.node_edits.push(NodeEdit::Rename { node: n, name: w(to) });
        }
    }
}

/// `*Change` 一族（快照里要排除它们自己）。
fn is_change_element(local: LocalName) -> bool {
    matches!(
        local,
        LocalName::RPrChange
            | LocalName::PPrChange
            | LocalName::SectPrChange
            | LocalName::TblPrChange
            | LocalName::TblPrExChange
            | LocalName::TblGridChange
            | LocalName::TrPrChange
            | LocalName::TcPrChange
            | LocalName::NumberingChange
    )
}

fn live_children(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    dom.children(n).iter().copied().filter(|&c| dom.node(c).dirty != Dirty::Deleted)
}

fn child_named(dom: &Dom, parent: NodeId, name: QName) -> Option<NodeId> {
    live_children(dom, parent).find(|&c| dom.is(c, name))
}

/// 把 `marker` 按 `order` 插进一个**还没进 DOM** 的属性容器（`PROP-05`）。
pub(crate) fn insert_ordered(
    container: &mut NewElement,
    marker: NewElement,
    order: fn(QName) -> Option<u16>,
) {
    let mine = order(marker.name).unwrap_or(u16::MAX);
    let at = container
        .children
        .iter()
        .position(|c| match c {
            NewNode::Element(e) => order(e.name).is_none_or(|i| i > mine),
            NewNode::Text(_) => false,
        })
        .unwrap_or(container.children.len());
    container.children.insert(at, NewNode::Element(marker));
}

/// 新建的块（`NewElement`，还没进 DOM）标成"插入"（`spec/18` 7.3）：
///
/// - `NewBlock::Paragraph` → 内容子节点整批进一个 `w:ins`，再给段落标记加 `pPr/rPr/w:ins`；
/// - `NewBlock::Table` → 每个 `w:tr` 加 `trPr/w:ins`（`w:tblPrEx` 仍排在 `w:trPr` 之前）；
/// - `NewBlock::Xml` / `Wrapped`（`opaque`）→ 整个元素包进块级 `w:ins`（TS 的形态，
///   解析器已认）。调用方给的是整段原始 XML，往里面塞标记就等于改写它给的字节。
pub(crate) fn mark_new_block_inserted(
    t: &mut Tracker,
    node: NewElement,
    opaque: bool,
) -> NewElement {
    if opaque {
        return t.marker(LocalName::Ins).with_child(node);
    }
    let is = |e: &NewElement, l: LocalName| e.name == w(l);
    if is(&node, LocalName::P) {
        let mut out = NewElement::new(node.name);
        out.attrs = node.attrs.clone();
        let mut content = t.marker(LocalName::Ins);
        for child in node.children {
            match &child {
                NewNode::Element(e) if e.name == w(LocalName::PPr) => {
                    out.children.push(NewNode::Element(with_para_mark(t, e.clone())));
                }
                _ => content.children.push(child),
            }
        }
        // 没有 `pPr` 时补一个，只为放段落标记的 `w:ins`
        if !out
            .children
            .iter()
            .any(|c| matches!(c, NewNode::Element(e) if e.name == w(LocalName::PPr)))
        {
            let ppr = with_para_mark(t, NewElement::new(w(LocalName::PPr)));
            out.children.insert(0, NewNode::Element(ppr));
        }
        if !content.children.is_empty() {
            out.children.push(NewNode::Element(content));
        }
        return out;
    }
    if is(&node, LocalName::Tbl) {
        let mut out = NewElement::new(node.name);
        out.attrs = node.attrs.clone();
        for child in node.children {
            match child {
                NewNode::Element(e) if e.name == w(LocalName::Tr) => {
                    out.children.push(NewNode::Element(with_row_mark(t, e, LocalName::Ins)));
                }
                other => out.children.push(other),
            }
        }
        return out;
    }
    t.marker(LocalName::Ins).with_child(node)
}

/// `pPr` 里放段落标记的 `w:ins`（`rPr` 缺就建；`w:ins` 是 `rPr` 的第一个子元素，`PROP-05`）。
fn with_para_mark(t: &mut Tracker, ppr: NewElement) -> NewElement {
    let marker = t.marker(LocalName::Ins);
    let mut out = NewElement::new(ppr.name);
    out.attrs = ppr.attrs.clone();
    let mut done = false;
    for child in ppr.children {
        match child {
            NewNode::Element(e) if e.name == w(LocalName::RPr) => {
                let mut rpr = NewElement::new(e.name);
                rpr.attrs = e.attrs.clone();
                rpr.children.push(NewNode::Element(marker.clone()));
                rpr.children.extend(e.children);
                out.children.push(NewNode::Element(rpr));
                done = true;
            }
            other => out.children.push(other),
        }
    }
    if !done {
        // `w:rPr` 是 `w:pPr` 的最后一个子元素（只有 `sectPr` / `pPrChange` 在它后面）
        let at = out
            .children
            .iter()
            .position(|c| {
                matches!(c, NewNode::Element(e)
                    if e.name == w(LocalName::SectPr) || e.name == w(LocalName::PPrChange))
            })
            .unwrap_or(out.children.len());
        out.children
            .insert(at, NewNode::Element(NewElement::new(w(LocalName::RPr)).with_child(marker)));
    }
    out
}

/// `w:tr` 加 `trPr/w:ins` 或 `trPr/w:del`。
fn with_row_mark(t: &mut Tracker, row: NewElement, mark: LocalName) -> NewElement {
    let marker = t.marker(mark);
    let mut out = NewElement::new(row.name);
    out.attrs = row.attrs.clone();
    let mut done = false;
    for child in row.children {
        match child {
            NewNode::Element(e) if e.name == w(LocalName::TrPr) => {
                let mut trpr = NewElement::new(e.name);
                trpr.attrs = e.attrs.clone();
                trpr.children.extend(e.children);
                trpr.children.push(NewNode::Element(marker.clone()));
                out.children.push(NewNode::Element(trpr));
                done = true;
            }
            other => out.children.push(other),
        }
    }
    if !done {
        // `w:trPr` 紧跟 `w:tblPrEx`（如果有），在所有 `w:tc` 之前
        let at = out
            .children
            .iter()
            .position(|c| !matches!(c, NewNode::Element(e) if e.name == w(LocalName::TblPrEx)))
            .unwrap_or(out.children.len());
        out.children
            .insert(at, NewNode::Element(NewElement::new(w(LocalName::TrPr)).with_child(marker)));
    }
    out
}

/// `REV_NOT_TRACKED`：这个操作 Word 也不记修订（或另有机制），照常执行、留一条记录。
/// 有 `MutationPlan` 的地方用 `ops::run` 入口那条集中判定；这里给没有计划的调用点用。
pub(crate) fn not_tracked(_ctx: &EditContext, _what: &str) {}
