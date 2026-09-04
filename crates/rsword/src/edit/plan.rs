//! `EDIT-05`：`MutationPlan`（只读产出）→ `validate`（只读）→ `commit`（机械写入）→ `MutationResult`。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::package::PartId;
use crate::xml::{Dirty, Dom, NodeEdit, NodeId, NodeKind, Target};

use super::pos::Utf16Offset;

/// 一次操作（或操作的一个阶段）对某个 part 的全部变更。
#[derive(Debug, Clone, PartialEq)]
pub struct MutationPlan {
    pub part: PartId,
    pub node_edits: Vec<NodeEdit>,
    /// 需要刷新投影的段落（`w:p`）。
    pub affected_paragraphs: Vec<NodeId>,
    /// 块的增删移：投影整体重建。
    pub structure_changed: bool,
    /// 计划阶段发现、提交后记录到会话的诊断（例如 `EDIT_ANCHOR_UNMOVED`）。
    pub diagnostics: Vec<Diagnostic>,
    /// `(para, from, delta)`：供调用方修正光标。
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}

/// `commit` 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MutationResult {
    /// 每条 `NodeEdit` 创建的节点（与 `node_edits` 对齐；不创建节点的为 `None`）。
    pub created: Vec<Option<NodeId>>,
    pub affected_paragraphs: Vec<NodeId>,
    pub structure_changed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}

impl MutationResult {
    /// 合并多阶段结果（后一阶段的 `created` 覆盖）。
    pub fn absorb(&mut self, later: MutationResult) {
        self.created = later.created;
        for p in later.affected_paragraphs {
            if !self.affected_paragraphs.contains(&p) {
                self.affected_paragraphs.push(p);
            }
        }
        self.structure_changed |= later.structure_changed;
        self.diagnostics.extend(later.diagnostics);
        self.offset_delta.extend(later.offset_delta);
    }
}

impl MutationPlan {
    pub fn new(part: PartId) -> Self {
        Self {
            part,
            node_edits: Vec::new(),
            affected_paragraphs: Vec::new(),
            structure_changed: false,
            diagnostics: Vec::new(),
            offset_delta: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.node_edits.is_empty()
    }

    pub fn touch(&mut self, para: NodeId) {
        if !self.affected_paragraphs.contains(&para) {
            self.affected_paragraphs.push(para);
        }
    }

    /// 只读校验：每条编辑引用的节点存在、未删除、类型正确；`before` 是 `parent` 的子节点；
    /// `Target::New(k)` 指向前面一条会创建节点的编辑；`Move` 不把节点搬进自己的子树。
    /// 失败 → `Err(EDIT_PLAN_INVALID)`，DOM 未被触碰。
    pub fn validate(&self, dom: &Dom) -> Result<()> {
        let bad = |msg: String| Error::edit(DiagCode::EditPlanInvalid, msg);
        let exists = |n: NodeId| (n.0 as usize) < dom.node_count();
        let live = |n: NodeId| exists(n) && dom.node(n).dirty != Dirty::Deleted;
        let is_element = |n: NodeId| live(n) && dom.element(n).is_some();
        let check_target = |t: Target, i: usize, what: &str| -> Result<Option<NodeId>> {
            match t {
                Target::Node(n) => {
                    if !is_element(n) {
                        return Err(bad(format!("edit[{i}] {what}: 节点 {} 不是活元素", n.0)));
                    }
                    Ok(Some(n))
                }
                Target::New(k) => {
                    if k >= i || !self.node_edits[k].creates() {
                        return Err(bad(format!(
                            "edit[{i}] {what}: Target::New({k}) 不指向前面的创建"
                        )));
                    }
                    Ok(None)
                }
            }
        };
        let check_before = |parent: Option<NodeId>,
                            before: Option<NodeId>,
                            i: usize|
         -> Result<()> {
            match (parent, before) {
                (_, None) => Ok(()),
                (Some(p), Some(b)) => {
                    if !exists(b) || dom.child_index(p, b).is_none() {
                        return Err(bad(format!("edit[{i}]: before {} 不是 parent 的子节点", b.0)));
                    }
                    Ok(())
                }
                (None, Some(_)) => Err(bad(format!("edit[{i}]: 新建父节点下不能指定 before"))),
            }
        };
        for (i, e) in self.node_edits.iter().enumerate() {
            match e {
                NodeEdit::Insert { parent, before, .. } => {
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                }
                NodeEdit::InsertClone { parent, before, source } => {
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                    if !live(*source) {
                        return Err(bad(format!("edit[{i}]: 克隆源 {} 不存在或已删除", source.0)));
                    }
                }
                NodeEdit::Replace { old, .. } | NodeEdit::ReplaceClone { old, .. } => {
                    if !live(*old) || dom.parent(*old).is_none() {
                        return Err(bad(format!("edit[{i}]: 被替换节点 {} 无效", old.0)));
                    }
                    if let NodeEdit::ReplaceClone { source, .. } = e
                        && !live(*source)
                    {
                        return Err(bad(format!("edit[{i}]: 克隆源 {} 不存在或已删除", source.0)));
                    }
                }
                NodeEdit::Delete(n) => {
                    if !exists(*n) {
                        return Err(bad(format!("edit[{i}]: 删除的节点 {} 不存在", n.0)));
                    }
                }
                NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => {
                    check_target(*node, i, "node")?;
                }
                NodeEdit::SetText { node, .. } => {
                    if !live(*node) || !matches!(dom.node(*node).kind, NodeKind::Text(_)) {
                        return Err(bad(format!(
                            "edit[{i}]: SetText 的目标 {} 不是活文本节点",
                            node.0
                        )));
                    }
                }
                NodeEdit::Move { node, parent, before } => {
                    if !live(*node) || dom.parent(*node).is_none() {
                        return Err(bad(format!("edit[{i}]: 移动的节点 {} 无效", node.0)));
                    }
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                    if let Some(p) = p
                        && dom.is_ancestor_or_self(*node, p)
                    {
                        return Err(bad(format!(
                            "edit[{i}]: 不能把节点 {} 搬进自己的子树",
                            node.0
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// 机械写入（只调用 `xml::edit` 原语，`XML-12` 脏规则自动成立）。调用方必须先 `validate`。
    pub fn commit(self, dom: &mut Dom) -> MutationResult {
        let created = dom.apply_edits(&self.node_edits);
        debug_assert!(dom.check_dirty_invariants().is_ok(), "XML-12 脏状态不变式被破坏");
        MutationResult {
            created,
            affected_paragraphs: self.affected_paragraphs,
            structure_changed: self.structure_changed,
            diagnostics: self.diagnostics,
            offset_delta: self.offset_delta,
        }
    }
}
