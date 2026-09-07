//! 跨 part 的修订索引（`MOD-09`、`EDIT-06`；`spec/18` 7.1）。
//!
//! `Block.revisions` / `Run.rev` / `Row` / `Cell` / `SectionInfo` 上的修订是**投影**：run 级的
//! [`crate::model::RevisionCtx`] 每种只留一格，包裹套娃会被压平，内联容器超过深度上限的整段还会降级。
//! 接受 / 拒绝修订要动的是每一层承载元素本身，所以这张索引直接走 DOM：
//!
//! - **扫全部未删节点**，包括 `mc:Choice` / `mc:Fallback` 两支——修订 `w:id` 的唯一性是整个包的事，
//!   与 MCE 选哪支无关（与 [`crate::edit::media_ops`] 的 `wp:docPr/@id` 同一条理由）。
//! - **迭代遍历**（`rev-nested-wrappers` 是 500 层 `w:ins` / `w:del` 交替）。
//! - 文档序 = part 顺序（主 part → 页眉页脚 → 脚注 → 尾注 → 批注 → 外部文本框）内各自的前序。

use std::collections::BTreeMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::model::RevisionMeta;
use crate::model::macros::named_enum;
use crate::package::PartId;
use crate::span::field::{FieldId, FieldIndex};
use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

/// 修订的会话内稳定 id（`MOD-13`）。
///
/// [`crate::model::Document::rebuild`] 从 0 起按文档序编号；[`crate::edit::EditSession`] 随后把仍然
/// 存在的承载节点换回它上次拿到的 id（arena 里 `NodeId` 稳定），新节点才拿新号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RevisionId(pub u32);

named_enum! {
    /// 修订种类：`MOD-09` 的 16 种 + run 级 5 种 + `w:tblPrExChange`。
    ///
    /// `w:tblPrExChange` 不在 `MOD-09` 的清单里，但真实 Word 的表格修订会写它
    /// （`fixtures/revisions/table-and-move/tracked.docx` 有 3 处），接受 / 拒绝必须认它，
    /// 否则门第 3 条过不去。登记在 `docs/04` §8。
    pub enum RevKind {
        /// 块级 `w:ins`（含 `trPr/w:ins` 的整行插入）。
        Insert = "insert",
        /// 块级 `w:del`（含 `trPr/w:del`）。
        Delete = "delete",
        MoveFrom = "moveFrom",
        MoveTo = "moveTo",
        /// `pPr/rPr/w:ins`。
        ParaMarkInsert = "paraMarkInsert",
        /// `pPr/rPr/w:del`。
        ParaMarkDelete = "paraMarkDelete",
        /// `pPr/rPr/w:moveFrom`：段落标记随内容被搬走（接受后与下一段合并，同 `ParaMarkDelete`，
        /// 但要与 `ParaMarkMoveTo` 配对）。
        ParaMarkMoveFrom = "paraMarkMoveFrom",
        /// `pPr/rPr/w:moveTo`。
        ParaMarkMoveTo = "paraMarkMoveTo",
        ParaPropsChange = "paraPropsChange",
        NumberingChange = "numberingChange",
        TablePropsChange = "tablePropsChange",
        /// `w:tr/w:tblPrEx/w:tblPrExChange`。
        TablePropsExChange = "tablePropsExChange",
        SectPropsChange = "sectPropsChange",
        TableGridChange = "tableGridChange",
        RowPropsChange = "rowPropsChange",
        CellPropsChange = "cellPropsChange",
        CellInsert = "cellInsert",
        CellDelete = "cellDelete",
        CellMerge = "cellMerge",
        /// run 级 `w:ins`。
        RunInsert = "runInsert",
        /// run 级 `w:del`。
        RunDelete = "runDelete",
        RunMoveFrom = "runMoveFrom",
        RunMoveTo = "runMoveTo",
        /// `w:rPrChange`（段落标记的 `rPr` 里的那个也算这一种，owner 不同）。
        RunPropsChange = "runPropsChange",
    }
}

impl RevKind {
    /// 是不是内联（run 级）包裹。
    pub const fn is_run_level(self) -> bool {
        matches!(self, Self::RunInsert | Self::RunDelete | Self::RunMoveFrom | Self::RunMoveTo)
    }

    /// 是不是「内容包裹」（`w:ins` / `w:del` / `w:moveFrom` / `w:moveTo`，块级或 run 级）。
    pub const fn is_wrapper(self) -> bool {
        matches!(
            self,
            Self::Insert
                | Self::Delete
                | Self::MoveFrom
                | Self::MoveTo
                | Self::RunInsert
                | Self::RunDelete
                | Self::RunMoveFrom
                | Self::RunMoveTo
        )
    }

    /// 搬移的**内容**一半（要配对）。
    ///
    /// 段落标记上的 `w:moveFrom` / `w:moveTo` 不算：真实 Word 把它写在范围标记**之外**
    /// （`corpus/real/revisions2/rev-move.docx` 的 `w:moveFrom w:id="0"` 在
    /// `w:moveFromRangeStart` 之前），按 `@w:name` 配不上；而且标记的接受 / 拒绝
    /// 与 `ParaMarkDelete` / `ParaMarkInsert` 完全一样，本来就不需要孪生。
    pub const fn is_move(self) -> bool {
        matches!(self, Self::MoveFrom | Self::MoveTo | Self::RunMoveFrom | Self::RunMoveTo)
    }

    /// 搬移的**来源**半边。
    pub const fn is_move_from(self) -> bool {
        matches!(self, Self::MoveFrom | Self::RunMoveFrom)
    }

    /// 段落标记上的修订（`pPr/rPr` 里那一层）。搬移配对时内容与标记各配各的。
    pub const fn is_para_mark(self) -> bool {
        matches!(
            self,
            Self::ParaMarkInsert
                | Self::ParaMarkDelete
                | Self::ParaMarkMoveFrom
                | Self::ParaMarkMoveTo
        )
    }

    /// `*PrChange` 一族：内层是旧值快照容器。
    pub const fn is_props_change(self) -> bool {
        matches!(
            self,
            Self::ParaPropsChange
                | Self::RunPropsChange
                | Self::TablePropsChange
                | Self::TablePropsExChange
                | Self::SectPropsChange
                | Self::RowPropsChange
                | Self::CellPropsChange
                | Self::TableGridChange
                | Self::NumberingChange
        )
    }
}

/// 承载修订的宿主。`kind` 已经说明是哪种修订，这里给的是接受 / 拒绝时要动的那个上层节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevOwner {
    /// 块级包裹：包着 `w:p` / `w:tbl` 的那层壳所在的块容器（`w:body` / `w:tc` / `w:sdtContent` …）。
    Block(NodeId),
    /// run 级包裹：所在段落 `w:p`。
    Inline(NodeId),
    /// `w:r/w:rPr/w:rPrChange`：那个 `w:r`。
    Run(NodeId),
    /// 段落标记（`pPr/rPr/w:ins|w:del`、`pPr/rPr/w:rPrChange`）、`pPrChange`、`numberingChange`：`w:p`。
    ParaMark(NodeId),
    Row(NodeId),
    Cell(NodeId),
    Table(NodeId),
    /// `w:sectPr/w:sectPrChange`：那个 `w:sectPr`。
    Section(NodeId),
    /// run 级删除包住的是字段指令区（`w:delInstrText`）：那个字段。
    Field(FieldId),
}

impl RevOwner {
    /// 宿主节点（`Field` 没有节点）。
    pub const fn node(self) -> Option<NodeId> {
        match self {
            Self::Block(n)
            | Self::Inline(n)
            | Self::Run(n)
            | Self::ParaMark(n)
            | Self::Row(n)
            | Self::Cell(n)
            | Self::Table(n)
            | Self::Section(n) => Some(n),
            Self::Field(_) => None,
        }
    }
}

/// 一条修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionEntry {
    pub id: RevisionId,
    pub part: PartId,
    pub kind: RevKind,
    /// 承载元素（`w:ins` / `w:rPrChange` / …）与它的 `w:id` / `w:author` / `w:date`。
    pub meta: RevisionMeta,
    pub owner: RevOwner,
    /// 外层修订包裹的层数，最外层为 0。
    pub depth: u16,
    /// `w:moveFromRangeStart` / `w:moveToRangeStart` 的 `@w:name`（只有搬移有；没被范围罩住则 `None`）。
    pub move_name: Option<String>,
    /// moveFrom ↔ moveTo 的孪生（`REV_UNPAIRED_MOVE` 时 `None`）。
    pub pair: Option<RevisionId>,
}

impl RevisionEntry {
    /// 承载元素。
    pub fn node(&self) -> NodeId {
        self.meta.node
    }

    pub fn author(&self) -> Option<&str> {
        self.meta.author.as_deref()
    }

    /// `w:id` 的数值形态（`EDIT-06` 取全局最大值用）。非数字的原串保留在 `meta.id`，不参与比较。
    pub fn w_id(&self) -> Option<u32> {
        self.meta.id.as_deref()?.trim().parse().ok()
    }
}

/// 全包的修订表（`MOD-09`），文档序。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionIndex {
    entries: Vec<RevisionEntry>,
    by_id: BTreeMap<RevisionId, usize>,
    by_node: BTreeMap<(PartId, NodeId), usize>,
}

impl RevisionIndex {
    pub fn entries(&self) -> &[RevisionEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: RevisionId) -> Option<&RevisionEntry> {
        self.by_id.get(&id).map(|&i| &self.entries[i])
    }

    /// 承载元素 → 条目。
    pub fn by_node(&self, part: PartId, node: NodeId) -> Option<&RevisionEntry> {
        self.by_node.get(&(part, node)).map(|&i| &self.entries[i])
    }

    pub fn of_part(&self, part: PartId) -> impl Iterator<Item = &RevisionEntry> {
        self.entries.iter().filter(move |e| e.part == part)
    }

    /// 某个作者的修订（`w:author` 字符串相等，不看 `w:initials`）。
    pub fn by_author<'a>(&'a self, author: &'a str) -> impl Iterator<Item = &'a RevisionEntry> {
        self.entries.iter().filter(move |e| e.author() == Some(author))
    }

    /// 出现过的作者，去重后按名字排序。
    pub fn authors(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entries.iter().filter_map(RevisionEntry::author).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// 全包最大的修订 `w:id`（`EDIT-06`：新修订从它 + 1 起编号）。
    pub fn max_w_id(&self) -> Option<u32> {
        self.entries.iter().filter_map(RevisionEntry::w_id).max()
    }

    /// **先内层后外层**的文档序（`spec/18` 7.4：`w:ins` 套 `w:del` 时先处理 `del`）。
    ///
    /// 条目是前序收集的，祖先一定排在后代之前，`depth` 相邻两条最多差 1，所以一遍栈就能转成后序。
    pub fn iter_inner_first(&self) -> Vec<&RevisionEntry> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut stack: Vec<&RevisionEntry> = Vec::new();
        for e in &self.entries {
            while stack.last().is_some_and(|t| t.depth >= e.depth) {
                out.push(stack.pop().expect("checked by last()"));
            }
            stack.push(e);
        }
        while let Some(t) = stack.pop() {
            out.push(t);
        }
        out
    }

    /// 会话内稳定编号（`MOD-13`）：仍然存在的承载节点复用旧 id，新节点从 `next` 取号。
    pub(crate) fn stabilize(
        &mut self,
        known: &mut BTreeMap<(PartId, NodeId), RevisionId>,
        next: &mut u32,
    ) {
        // `pair` 存的是重编号**之前**的 id，先记下它指向哪个节点
        let was: BTreeMap<RevisionId, (PartId, NodeId)> =
            self.entries.iter().map(|e| (e.id, (e.part, e.node()))).collect();
        for e in &mut self.entries {
            let key = (e.part, e.node());
            let id = *known.entry(key).or_insert_with(|| {
                let id = RevisionId(*next);
                *next = next.saturating_add(1);
                id
            });
            e.id = id;
        }
        let now: BTreeMap<(PartId, NodeId), RevisionId> =
            self.entries.iter().map(|e| ((e.part, e.node()), e.id)).collect();
        for e in &mut self.entries {
            e.pair = e.pair.and_then(|old| was.get(&old)).and_then(|k| now.get(k)).copied();
        }
        self.reindex();
    }

    fn reindex(&mut self) {
        self.by_id = self.entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
        self.by_node =
            self.entries.iter().enumerate().map(|(i, e)| ((e.part, e.node()), i)).collect();
    }
}

// ---- 构建 --------------------------------------------------------------------------------------

/// 一个 part 的输入：DOM 与（有的话）它的字段索引。
pub(crate) struct RevPart<'a> {
    pub part: PartId,
    pub dom: &'a Dom,
    pub fields: Option<&'a FieldIndex>,
}

/// 遍历时随节点下传的上下文。
#[derive(Clone, Copy, Default)]
struct Ctx {
    /// 最近的 `w:p`（进内容流根与 `w:tc` 时清空——文本框里的段落不算外层段落的一部分）。
    para: Option<NodeId>,
    run: Option<NodeId>,
    row: Option<NodeId>,
    cell: Option<NodeId>,
    table: Option<NodeId>,
    sect: Option<NodeId>,
    /// 最近的块容器（块级包裹的 owner）。
    container: Option<NodeId>,
    /// 在 `w:pPr` 里（它下面的 `w:rPr` 是段落标记的）。
    in_ppr: bool,
    /// 在 `w:trPr` 里（它下面的 `w:ins` / `w:del` 是整行插入 / 删除）。
    in_trpr: bool,
    depth: u16,
}

impl RevisionIndex {
    /// 扫一批 part，按给定顺序拼成文档序的索引。
    pub(crate) fn build(parts: &[RevPart<'_>], warnings: &mut Vec<Diagnostic>) -> RevisionIndex {
        let mut idx = RevisionIndex::default();
        for p in parts {
            let base = idx.entries.len() as u32;
            idx.entries.extend(scan_part(p, base));
        }
        idx.pair_moves(parts, warnings);
        idx.reindex();
        idx
    }

    /// 按 `@w:name` 把 moveFrom 与 moveTo 配对，落单的记 `REV_UNPAIRED_MOVE`。
    fn pair_moves(&mut self, parts: &[RevPart<'_>], warnings: &mut Vec<Diagnostic>) {
        let (pairs, mut lonely) = self.group_moves();
        for (f, t) in pairs {
            let (fid, tid) = (self.entries[f].id, self.entries[t].id);
            self.entries[f].pair = Some(tid);
            self.entries[t].pair = Some(fid);
        }
        lonely.sort_unstable();
        lonely.dedup();
        for i in lonely {
            let e = &self.entries[i];
            let dom = parts.iter().find(|p| p.part == e.part).map(|p| p.dom);
            let range = dom.and_then(|d| d.node(e.node()).lex.as_ref().map(|l| l.range.clone()));
            warnings.push(Diagnostic::pre_existing(
                e.part,
                range,
                DiagCode::RevUnpairedMove,
                match &e.move_name {
                    Some(n) => format!("{} 的孪生（w:name = {n}）不存在", e.kind),
                    None => format!("{} 不在任何 move 范围标记里", e.kind),
                },
            ));
        }
    }

    /// 按 `@w:name` 分组配对，返回（成对的下标, 落单的下标）。
    fn group_moves(&self) -> (Vec<(usize, usize)>, Vec<usize>) {
        let mut from: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut to: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut lonely: Vec<usize> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if !e.kind.is_move() {
                continue;
            }
            match e.move_name.as_deref() {
                Some(name) => {
                    let side = if e.kind.is_move_from() { &mut from } else { &mut to };
                    side.entry(name).or_default().push(i);
                }
                None => lonely.push(i),
            }
        }
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for (name, fs) in &from {
            let ts = to.get(name).map(Vec::as_slice).unwrap_or(&[]);
            for (k, &f) in fs.iter().enumerate() {
                match ts.get(k) {
                    Some(&t) => pairs.push((f, t)),
                    None => lonely.push(f),
                }
            }
            if ts.len() > fs.len() {
                lonely.extend(ts[fs.len()..].iter().copied());
            }
        }
        for (name, ts) in &to {
            if !from.contains_key(name) {
                lonely.extend(ts.iter().copied());
            }
        }
        (pairs, lonely)
    }
}

/// 扫一个 part，条目按前序（= 文档序）返回，`id` 先按序号临时给。
fn scan_part(p: &RevPart<'_>, id_base: u32) -> Vec<RevisionEntry> {
    let dom = p.dom;
    let mut out: Vec<RevisionEntry> = Vec::new();
    // 打开着的 move 范围：`(w:id, w:name)`，最内层在末尾
    let mut open_from: Vec<(String, String)> = Vec::new();
    let mut open_to: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(NodeId, Ctx)> = vec![(dom.root(), Ctx::default())];
    while let Some((n, ctx)) = stack.pop() {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        let name = e.name;
        // 范围标记先处理：它们是兄弟节点，按文档序开合
        if name.ns == NsId::W {
            match name.local {
                LocalName::MoveFromRangeStart => push_range(dom, n, &mut open_from),
                LocalName::MoveToRangeStart => push_range(dom, n, &mut open_to),
                LocalName::MoveFromRangeEnd => pop_range(dom, n, &mut open_from),
                LocalName::MoveToRangeEnd => pop_range(dom, n, &mut open_to),
                _ => {}
            }
        }
        let hit = classify(dom, n, name, &ctx);
        let mut child = ctx;
        if let Some((kind, owner)) = hit {
            let move_name = kind.is_move().then(|| {
                let open = if kind.is_move_from() { &open_from } else { &open_to };
                open.last().map(|(_, name)| name.clone())
            });
            out.push(RevisionEntry {
                id: RevisionId(id_base + out.len() as u32),
                part: p.part,
                kind,
                meta: meta_of(dom, n),
                owner: refine_owner(dom, n, kind, owner, p.fields),
                depth: ctx.depth,
                move_name: move_name.flatten(),
                pair: None,
            });
            child.depth = ctx.depth.saturating_add(1);
        }
        descend(n, name, &mut child);
        stack.extend(dom.children(n).iter().rev().map(|&c| (c, child)));
    }
    out
}

fn meta_of(dom: &Dom, node: NodeId) -> RevisionMeta {
    let a = |l: LocalName| dom.attr_value(node, QName::new(NsId::W, l)).map(|v| v.into_owned());
    RevisionMeta {
        node,
        id: a(LocalName::Id),
        author: a(LocalName::Author),
        date: a(LocalName::Date),
    }
}

fn push_range(dom: &Dom, n: NodeId, open: &mut Vec<(String, String)>) {
    let a = |l: LocalName| dom.attr_value(n, QName::new(NsId::W, l)).map(|v| v.into_owned());
    open.push((a(LocalName::Id).unwrap_or_default(), a(LocalName::Name).unwrap_or_default()));
}

fn pop_range(dom: &Dom, n: NodeId, open: &mut Vec<(String, String)>) {
    let id = dom
        .attr_value(n, QName::new(NsId::W, LocalName::Id))
        .map(|v| v.into_owned())
        .unwrap_or_default();
    match open.iter().rposition(|(i, _)| *i == id) {
        Some(k) => {
            open.remove(k);
        }
        // `w:id` 对不上（`rev-move-unpaired`）：按最内层关掉，不让范围一直挂着
        None => {
            open.pop();
        }
    }
}

/// 元素名 + 上下文 → 修订种类与宿主；不是承载元素则 `None`。
fn classify(dom: &Dom, n: NodeId, name: QName, ctx: &Ctx) -> Option<(RevKind, RevOwner)> {
    if name.ns != NsId::W {
        return None;
    }
    let block = || RevOwner::Block(ctx.container.or(dom.parent(n)).unwrap_or(n));
    let owner_or = |slot: Option<NodeId>, f: fn(NodeId) -> RevOwner| {
        slot.map_or_else(|| RevOwner::Block(dom.parent(n).unwrap_or(n)), f)
    };
    let para_mark = || owner_or(ctx.para, RevOwner::ParaMark);
    Some(match name.local {
        LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
            let ins = matches!(name.local, LocalName::Ins | LocalName::MoveTo);
            let mv = matches!(name.local, LocalName::MoveFrom | LocalName::MoveTo);
            if ctx.in_ppr && dom.parent(n).is_some_and(|p| dom.is(p, QName::w(LocalName::RPr))) {
                // 段落标记。`w:moveFrom` / `w:moveTo` 对标记的作用与 `w:del` / `w:ins` 相同
                // （接受 moveFrom = 与下一段合并），但要与另一半配对，所以另立种类
                let kind = match (mv, ins) {
                    (true, true) => RevKind::ParaMarkMoveTo,
                    (true, false) => RevKind::ParaMarkMoveFrom,
                    (false, true) => RevKind::ParaMarkInsert,
                    (false, false) => RevKind::ParaMarkDelete,
                };
                (kind, para_mark())
            } else if ctx.in_trpr {
                let kind = if ins { RevKind::Insert } else { RevKind::Delete };
                (kind, owner_or(ctx.row, RevOwner::Row))
            } else if ctx.para.is_some() {
                let kind = match (mv, ins) {
                    (true, true) => RevKind::RunMoveTo,
                    (true, false) => RevKind::RunMoveFrom,
                    (false, true) => RevKind::RunInsert,
                    (false, false) => RevKind::RunDelete,
                };
                (kind, owner_or(ctx.para, RevOwner::Inline))
            } else {
                let kind = match (mv, ins) {
                    (true, true) => RevKind::MoveTo,
                    (true, false) => RevKind::MoveFrom,
                    (false, true) => RevKind::Insert,
                    (false, false) => RevKind::Delete,
                };
                (kind, block())
            }
        }
        LocalName::RPrChange => {
            let owner = if ctx.in_ppr { para_mark() } else { owner_or(ctx.run, RevOwner::Run) };
            (RevKind::RunPropsChange, owner)
        }
        LocalName::PPrChange => (RevKind::ParaPropsChange, para_mark()),
        LocalName::NumberingChange => (RevKind::NumberingChange, para_mark()),
        LocalName::SectPrChange => {
            (RevKind::SectPropsChange, owner_or(ctx.sect, RevOwner::Section))
        }
        LocalName::TblPrChange => (RevKind::TablePropsChange, owner_or(ctx.table, RevOwner::Table)),
        LocalName::TblPrExChange => (RevKind::TablePropsExChange, owner_or(ctx.row, RevOwner::Row)),
        LocalName::TblGridChange => {
            (RevKind::TableGridChange, owner_or(ctx.table, RevOwner::Table))
        }
        LocalName::TrPrChange => (RevKind::RowPropsChange, owner_or(ctx.row, RevOwner::Row)),
        LocalName::TcPrChange => (RevKind::CellPropsChange, owner_or(ctx.cell, RevOwner::Cell)),
        LocalName::CellIns => (RevKind::CellInsert, owner_or(ctx.cell, RevOwner::Cell)),
        LocalName::CellDel => (RevKind::CellDelete, owner_or(ctx.cell, RevOwner::Cell)),
        LocalName::CellMerge => (RevKind::CellMerge, owner_or(ctx.cell, RevOwner::Cell)),
        _ => return None,
    })
}

/// run 级删除 / 插入包住的全是字段指令区的 run 时，宿主记成那个字段（`FLD-10` / 7.4 的
/// `FieldInstrDelete`）。查不到字段索引就保持原来的宿主。
fn refine_owner(
    dom: &Dom,
    n: NodeId,
    kind: RevKind,
    owner: RevOwner,
    fields: Option<&FieldIndex>,
) -> RevOwner {
    if !kind.is_run_level() {
        return owner;
    }
    let Some(fields) = fields else { return owner };
    let mut found = None;
    let mut instr = false;
    let mut other = false;
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        if dom.node(x).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(x) else { continue };
        stack.extend(e.children.iter().rev());
        if e.name.ns != NsId::W {
            continue;
        }
        match e.name.local {
            LocalName::InstrText | LocalName::DelInstrText => instr = true,
            LocalName::T | LocalName::DelText => other = true,
            LocalName::R => {
                if let Some(f) = fields.field_of(x) {
                    found.get_or_insert(f.id);
                }
            }
            _ => {}
        }
    }
    match (instr && !other, found) {
        (true, Some(id)) => RevOwner::Field(id),
        _ => owner,
    }
}

/// 进入 `node` 的子树前更新上下文。
fn descend(n: NodeId, name: QName, ctx: &mut Ctx) {
    if name.ns != NsId::W {
        return;
    }
    match name.local {
        // 内容流根：里面的段落与外层无关（`SPAN-01`）
        LocalName::Body
        | LocalName::TxbxContent
        | LocalName::Hdr
        | LocalName::Ftr
        | LocalName::Footnote
        | LocalName::Endnote
        | LocalName::Comment => {
            ctx.para = None;
            ctx.run = None;
            ctx.container = Some(n);
        }
        LocalName::Tbl => ctx.table = Some(n),
        LocalName::Tr => ctx.row = Some(n),
        LocalName::Tc => {
            ctx.cell = Some(n);
            ctx.para = None;
            ctx.run = None;
            ctx.container = Some(n);
        }
        LocalName::SdtContent | LocalName::CustomXml => {
            if ctx.para.is_none() {
                ctx.container = Some(n);
            }
        }
        LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
            if ctx.para.is_none() && !ctx.in_ppr && !ctx.in_trpr {
                ctx.container = Some(n);
            }
        }
        LocalName::P => ctx.para = Some(n),
        LocalName::R => ctx.run = Some(n),
        LocalName::PPr => ctx.in_ppr = true,
        LocalName::TrPr => ctx.in_trpr = true,
        LocalName::SectPr => ctx.sect = Some(n),
        _ => {}
    }
}
