//! `MutationPlan` / `MutationResult`（`EDIT-05`，`docs/03` §8.3）。
//!
//! `plan` 只读 DOM、产出跨 part 的机械编辑；`validate` 不改状态，检查所有 part / 节点 /
//! `Target::New` 引用与 `before` 父子关系；`commit` 在验证后才调用 `Dom::apply_edits`，
//! 因此任何错误都出现在写入之前，不会留下半修改状态。
//!
//! 任务 1.11 只实现框架。`EditSession::plan` 当前对全部 `EditOp` 返回
//! [`Error::EditUnsupported`]；[`MutationPlan::for_part`] 是公开的低层测试 / 工具入口，
//! 正常编辑路径由任务 1.12 的各操作 planner 构造。

use crate::diag::DiagCode;
use crate::edit::{EditSession, Utf16Offset};
use crate::error::{Error, Result};
use crate::package::{Package, PartId};
use crate::span::SpanId;
use crate::xml::names::NsId;
use crate::xml::plan::{NewElement, NewNode, NodeEdit, Target};
use crate::xml::{Dirty, Dom, NodeId};

/// 一次原子提交的结果（`EDIT-05`）。
///
/// `offset_delta` 供调用方把旧 `InlinePos` 映射到提交后的坐标；
/// `dirty_nodes` / `affected_containers` 目前是 plan 触及节点与脏祖先的保守上界，
/// 任务 1.12 实现增量 `Document::refresh` 后改为精确的刷新范围。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationResult {
    pub dirty_nodes: Vec<NodeId>,
    pub affected_containers: Vec<NodeId>,
    pub affected_spans: Vec<SpanId>,
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}

/// 延迟执行、可验证、可原子提交的 DOM 变更（`EDIT-05`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationPlan {
    /// 按 part 分组，组间顺序即 plan 阶段给出的顺序。
    pub(crate) edits: Vec<(PartId, Vec<NodeEdit>)>,
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}

impl MutationPlan {
    /// 单 part 的机械变更。正常编辑路径由任务 1.12 的 planner 构造；
    /// 本公开入口供测试与低层工具复用，commit 前仍会走完整 validate。
    pub fn for_part(part: PartId, edits: Vec<NodeEdit>) -> Self {
        Self { edits: vec![(part, edits)], offset_delta: Vec::new() }
    }

    pub fn with_offset_delta(mut self, offset_delta: Vec<(NodeId, Utf16Offset, i32)>) -> Self {
        self.offset_delta = offset_delta;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() || self.edits.iter().all(|(_, edits)| edits.is_empty())
    }

    /// 只读验证。任何 `Err` 都保证尚未对 [`Package`] 做任何修改。
    pub fn validate(&self, session: &EditSession) -> Result<()> {
        for (part, edits) in &self.edits {
            let Some(part_ref) = session.package().parts().get(part.0 as usize) else {
                return Err(invalid_plan(format!("part #{} does not exist", part.0)));
            };
            let Some(dom) = part_ref.dom() else {
                return Err(invalid_plan(format!(
                    "part {} has no parsed DOM; plan must ensure it is parsed first",
                    part_ref.uri
                )));
            };

            // Target::New(k) 指向后续 commit 才会创建的节点；这里只验证 k 确实引用
            // 前序创建型编辑。新子树自身的结构由 validate_new_element 检查。
            let mut created: Vec<bool> = Vec::with_capacity(edits.len());
            for (index, edit) in edits.iter().enumerate() {
                validate_edit(dom, edit, index, &created)?;
                created.push(edit.creates());
            }
        }
        Ok(())
    }

    /// 机械写入。仅在 `validate` 通过后调用；此时除 OOM 外不会失败。
    pub(crate) fn apply(&self, pkg: &mut Package) -> Result<MutationResult> {
        let mut result =
            MutationResult { offset_delta: self.offset_delta.clone(), ..MutationResult::default() };

        for (part, edits) in &self.edits {
            let dom = pkg
                .dom_mut(*part)?
                .ok_or_else(|| invalid_plan(format!("part #{} has no DOM", part.0)))?;
            let made = dom.apply_edits(edits);
            for root in made.iter().flatten() {
                dom.declare_for_new_subtree(*root);
            }
            let touched = plan_touched(dom, edits, &made);
            result.dirty_nodes.extend(touched.iter().copied());
            for &node in &touched {
                if dom.element(node).is_some() {
                    result.affected_containers.push(node);
                }
                for ancestor in dom.ancestors(node) {
                    if dom.node(ancestor).dirty != Dirty::Clean {
                        result.dirty_nodes.push(ancestor);
                    }
                }
            }
        }

        result.dirty_nodes.sort_unstable();
        result.dirty_nodes.dedup();
        result.affected_containers.sort_unstable();
        result.affected_containers.dedup();
        Ok(result)
    }
}

enum ValidatedTarget {
    Node(NodeId),
    /// `Target::New(k)`：此刻没有真实 DOM 节点，只验证 k 合法。
    New,
}

fn validated_target(
    dom: &Dom,
    target: Target,
    index: usize,
    created: &[bool],
) -> Result<ValidatedTarget> {
    match target {
        Target::Node(id) => {
            if id.0 >= dom.node_count() as u32 || dom.node(id).dirty == Dirty::Deleted {
                return Err(invalid_plan(format!(
                    "edit #{index}: target node {} does not exist or is deleted",
                    id.0
                )));
            }
            Ok(ValidatedTarget::Node(id))
        }
        Target::New(k) => {
            if k >= index || !created.get(k).copied().unwrap_or(false) {
                return Err(invalid_plan(format!(
                    "edit #{index}: Target::New({k}) does not refer to an earlier edit that creates a node"
                )));
            }
            Ok(ValidatedTarget::New)
        }
    }
}

fn validate_edit(dom: &Dom, edit: &NodeEdit, index: usize, created: &[bool]) -> Result<()> {
    let node_exists = |id: NodeId| -> bool {
        id.0 < dom.node_count() as u32 && dom.node(id).dirty != Dirty::Deleted
    };
    let check_node = |id: NodeId, what: &str| -> Result<()> {
        if !node_exists(id) {
            return Err(invalid_plan(format!(
                "edit #{index}: {what} node {} does not exist or is deleted",
                id.0
            )));
        }
        Ok(())
    };
    let parent_is_element = |p: NodeId| -> Result<()> {
        if dom.element(p).is_none() {
            return Err(invalid_plan(format!(
                "edit #{index}: parent node {} is not an element",
                p.0
            )));
        }
        Ok(())
    };
    let before_is_child = |p: NodeId, before: NodeId| -> Result<()> {
        if before.0 >= dom.node_count() as u32 || !dom.children(p).contains(&before) {
            return Err(invalid_plan(format!(
                "edit #{index}: `before` node {} is not a child of parent {}",
                before.0, p.0
            )));
        }
        Ok(())
    };

    match edit {
        NodeEdit::Insert { parent, before, node } => {
            match validated_target(dom, *parent, index, created)? {
                ValidatedTarget::Node(p) => {
                    parent_is_element(p)?;
                    if let Some(b) = before {
                        before_is_child(p, *b)?;
                    }
                }
                // 新 parent 的子节点关系由 planner 负责；生成器当前只有 before=None 走这里。
                ValidatedTarget::New => {}
            }
            validate_new_element(node, 0)?;
        }
        NodeEdit::InsertClone { parent, before, source } => {
            match validated_target(dom, *parent, index, created)? {
                ValidatedTarget::Node(p) => {
                    parent_is_element(p)?;
                    if let Some(b) = before {
                        before_is_child(p, *b)?;
                    }
                }
                ValidatedTarget::New => {}
            }
            check_node(*source, "clone source")?;
        }
        NodeEdit::Replace { old, node } => {
            check_node(*old, "replace target")?;
            if dom.parent(*old).is_none() {
                return Err(invalid_plan(format!(
                    "edit #{index}: node {} to replace has no parent",
                    old.0
                )));
            }
            validate_new_element(node, 0)?;
        }
        NodeEdit::ReplaceClone { old, source } => {
            check_node(*old, "replace target")?;
            if dom.parent(*old).is_none() {
                return Err(invalid_plan(format!(
                    "edit #{index}: node {} to replace has no parent",
                    old.0
                )));
            }
            check_node(*source, "clone source")?;
        }
        NodeEdit::Delete(node) => {
            check_node(*node, "delete target")?;
            if dom.parent(*node).is_none() {
                return Err(invalid_plan(format!(
                    "edit #{index}: node {} to delete has no parent",
                    node.0
                )));
            }
        }
        NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => {
            match validated_target(dom, *node, index, created)? {
                ValidatedTarget::Node(n) => {
                    if dom.element(n).is_none() {
                        return Err(invalid_plan(format!(
                            "edit #{index}: attribute target {} is not an element",
                            n.0
                        )));
                    }
                }
                ValidatedTarget::New => {}
            }
        }
    }
    Ok(())
}

fn validate_new_element(node: &NewElement, depth: u32) -> Result<()> {
    if depth > crate::xml::MAX_DEPTH {
        return Err(invalid_plan("new subtree is deeper than XML_MAX_DEPTH"));
    }
    if matches!(node.name.ns, NsId::Unbound(_)) {
        return Err(invalid_plan("new element uses an unbound namespace"));
    }
    for (attr, _) in &node.attrs {
        if matches!(attr.ns, NsId::Unbound(_)) {
            return Err(invalid_plan("new element has an attribute in an unbound namespace"));
        }
    }
    for child in &node.children {
        if let NewNode::Element(element) = child {
            validate_new_element(element, depth + 1)?;
        }
    }
    Ok(())
}

/// plan 直接或通过 `Target::New` 触及的节点（提交后存在性由 `Dom::apply_edits` 保证）。
fn plan_touched(dom: &Dom, edits: &[NodeEdit], created: &[Option<NodeId>]) -> Vec<NodeId> {
    let resolve = |t: Target| match t {
        Target::Node(id) => Some(id),
        Target::New(k) => created.get(k).copied().flatten(),
    };
    let mut out = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        match edit {
            NodeEdit::Insert { parent, before, .. }
            | NodeEdit::InsertClone { parent, before, .. } => {
                out.extend(resolve(*parent));
                out.extend(*before);
            }
            NodeEdit::Replace { old, .. } | NodeEdit::ReplaceClone { old, .. } => {
                out.push(*old);
                out.extend(dom.parent(*old));
            }
            NodeEdit::Delete(node) => {
                out.push(*node);
                out.extend(dom.parent(*node));
            }
            NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => {
                out.extend(resolve(*node));
            }
        }
        if edit.creates() {
            out.extend(created.get(index).copied().flatten());
        }
    }
    out.retain(|&id| id.0 < dom.node_count() as u32);
    out
}

fn invalid_plan(message: impl Into<String>) -> Error {
    Error::EditPlan { code: DiagCode::EditInvalidPlan, message: message.into() }
}
