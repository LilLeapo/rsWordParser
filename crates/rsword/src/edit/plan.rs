//! `EDIT-05`：`MutationPlan`（只读产出）→ `validate`（只读）→ `commit`（机械写入）→ `MutationResult`。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::package::PartId;
use crate::span::SpanPolicy;
use crate::xml::{Dirty, Dom, NodeEdit, NodeId, NodeKind, Target};

use super::pos::Utf16Offset;

/// 一次操作（或操作的一个阶段）对某个 part 的全部变更。
#[derive(Debug, Clone, PartialEq)]
pub struct MutationPlan {
    pub part: PartId,
    pub node_edits: Vec<NodeEdit>,
    /// 需要刷新投影的块（`w:p` 或 `w:tbl`）；`Document::refresh_blocks` 就地重建它们。
    pub affected_blocks: Vec<NodeId>,
    /// 块的增删移：投影整体重建。
    pub structure_changed: bool,
    /// 计划阶段发现、提交后记录到会话的诊断（例如 `EDIT_ANCHOR_UNMOVED`）。
    pub diagnostics: Vec<Diagnostic>,
    /// `(para, from, delta)`：供调用方修正光标。
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
    /// 与范围相关的要求（`SPAN-06/07`）；锚点变换本身由 `commit_plan` 从 `node_edits` 推导。
    pub span: SpanPolicy,
}

/// `commit` 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct MutationResult {
    /// 每条 `NodeEdit` 创建的节点（与 `node_edits` 对齐；不创建节点的为 `None`）。
    pub created: Vec<Option<NodeId>>,
    /// 需要刷新投影的块节点。
    pub affected_blocks: Vec<NodeId>,
    /// 块结构是否发生变化。
    pub structure_changed: bool,
    /// 本次操作产生的诊断。
    pub diagnostics: Vec<Diagnostic>,
    /// 段落、UTF-16 位置与长度变化量。
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl MutationResult {
    /// 合并多阶段结果（后一阶段的 `created` 覆盖）。
    #[doc(hidden)]
    pub fn absorb(&mut self, later: MutationResult) {
        self.created = later.created;
        for p in later.affected_blocks {
            if !self.affected_blocks.contains(&p) {
                self.affected_blocks.push(p);
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
            affected_blocks: Vec::new(),
            structure_changed: false,
            diagnostics: Vec::new(),
            offset_delta: Vec::new(),
            span: SpanPolicy::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.node_edits.is_empty()
    }

    /// 标记一个块（`w:p` / `w:tbl`）需要刷新投影。
    pub fn touch(&mut self, block: NodeId) {
        if !self.affected_blocks.contains(&block) {
            self.affected_blocks.push(block);
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
                NodeEdit::Rename { node, .. } => {
                    if !is_element(*node) {
                        return Err(bad(format!("edit[{i}]: 改名的目标 {} 不是活元素", node.0)));
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
            affected_blocks: self.affected_blocks,
            structure_changed: self.structure_changed,
            diagnostics: self.diagnostics,
            offset_delta: self.offset_delta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::ValidationOrigin;
    use crate::xml::plan::{NewElement, NodeEdit, Target};
    use crate::xml::{Dom, LocalName, QName};

    fn dom(xml: &str) -> Dom {
        Dom::parse(PartId(0), xml.as_bytes()).unwrap()
    }

    fn diag(message: &str) -> Diagnostic {
        Diagnostic::invariant_violation(PartId(0), None, DiagCode::EditPlanInvalid, message)
    }

    fn assert_invalid(plan: &MutationPlan, dom: &Dom) {
        let err = plan.validate(dom).unwrap_err();
        assert!(matches!(err, Error::Edit { code: DiagCode::EditPlanInvalid, .. }));
    }

    fn plan(part: PartId, node_edits: Vec<NodeEdit>) -> MutationPlan {
        let mut plan = MutationPlan::new(part);
        plan.node_edits = node_edits;
        plan
    }

    #[test]
    fn edit_05_result_absorb_and_plan_helpers_cover_all_fields() {
        let a = NodeId(1);
        let b = NodeId(2);
        let mut first = MutationResult {
            created: vec![Some(a)],
            affected_blocks: vec![a],
            structure_changed: false,
            diagnostics: vec![diag("first")],
            offset_delta: vec![(a, Utf16Offset(1), 1)],
        };
        let later = MutationResult {
            created: vec![Some(b)],
            affected_blocks: vec![a, b],
            structure_changed: true,
            diagnostics: vec![diag("later")],
            offset_delta: vec![(b, Utf16Offset(2), -1)],
        };
        first.absorb(later);
        assert_eq!(first.created, vec![Some(b)]);
        assert_eq!(first.affected_blocks, vec![a, b]);
        assert!(first.structure_changed);
        assert_eq!(first.diagnostics, vec![diag("first"), diag("later")]);
        assert_eq!(first.offset_delta, vec![(a, Utf16Offset(1), 1), (b, Utf16Offset(2), -1)]);

        let mut plan = MutationPlan::new(PartId(0));
        assert!(plan.is_empty());
        plan.touch(a);
        plan.touch(a);
        assert_eq!(plan.affected_blocks, vec![a]);
        plan.node_edits.push(NodeEdit::Delete(a));
        assert!(!plan.is_empty());
    }

    #[test]
    fn edit_05_validate_rejects_out_of_range_deleted_and_non_element_targets() {
        let dom = dom("<root><a/><b>text</b></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let text = dom.children(b)[0];

        let out_of_range = plan(
            PartId(0),
            vec![NodeEdit::SetAttr {
                node: Target::Node(NodeId(dom.node_count() as u32)),
                name: QName::w(LocalName::T),
                value: "x".into(),
            }],
        );
        assert_invalid(&out_of_range, &dom);

        let mut deleted_dom = dom.clone();
        deleted_dom.delete(a);
        let deleted = plan(
            PartId(0),
            vec![NodeEdit::SetAttr {
                node: Target::Node(a),
                name: QName::w(LocalName::T),
                value: "x".into(),
            }],
        );
        assert_invalid(&deleted, &deleted_dom);

        let text_target = plan(
            PartId(0),
            vec![NodeEdit::SetAttr {
                node: Target::Node(text),
                name: QName::w(LocalName::T),
                value: "x".into(),
            }],
        );
        assert_invalid(&text_target, &dom);
    }

    #[test]
    fn edit_05_validate_rejects_bad_new_target_and_before() {
        let dom = dom("<root><a/><b/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let new_element = || NewElement::new(QName::w(LocalName::P));

        let self_reference = plan(
            PartId(0),
            vec![NodeEdit::Insert { parent: Target::New(0), before: None, node: new_element() }],
        );
        assert_invalid(&self_reference, &dom);

        let foreign_before = plan(
            PartId(0),
            vec![NodeEdit::Insert {
                parent: Target::Node(a),
                before: Some(b),
                node: new_element(),
            }],
        );
        assert_invalid(&foreign_before, &dom);

        let new_parent_with_before = plan(
            PartId(0),
            vec![
                NodeEdit::Insert { parent: Target::Node(root), before: None, node: new_element() },
                NodeEdit::Insert { parent: Target::New(0), before: Some(a), node: new_element() },
            ],
        );
        assert_invalid(&new_parent_with_before, &dom);
    }

    #[test]
    fn edit_05_validate_rejects_invalid_replace_sources_and_text_targets() {
        let dom = dom("<root><a/><b/><c/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let c = dom.children(root)[2];

        let root_replace = plan(
            PartId(0),
            vec![NodeEdit::Replace { old: root, node: NewElement::new(QName::w(LocalName::P)) }],
        );
        assert_invalid(&root_replace, &dom);

        let mut deleted_old_dom = dom.clone();
        deleted_old_dom.delete(a);
        let deleted_replace_old = plan(
            PartId(0),
            vec![NodeEdit::Replace { old: a, node: NewElement::new(QName::w(LocalName::P)) }],
        );
        assert_invalid(&deleted_replace_old, &deleted_old_dom);

        let mut deleted_source_dom = dom.clone();
        deleted_source_dom.delete(c);
        let deleted_replace_source =
            plan(PartId(0), vec![NodeEdit::ReplaceClone { old: a, source: c }]);
        assert_invalid(&deleted_replace_source, &deleted_source_dom);

        let non_text_set_text =
            plan(PartId(0), vec![NodeEdit::SetText { node: a, text: "x".into() }]);
        assert_invalid(&non_text_set_text, &dom);

        let _ = b;
    }

    #[test]
    fn edit_05_validate_rejects_detached_move_even_with_new_parent() {
        let dom = dom("<root><a/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let plan = plan(
            PartId(0),
            vec![
                NodeEdit::Insert {
                    parent: Target::Node(root),
                    before: None,
                    node: NewElement::new(QName::w(LocalName::P)),
                },
                NodeEdit::Move { node: root, parent: Target::New(0), before: None },
            ],
        );
        assert_invalid(&plan, &dom);
        let _ = a;
    }

    #[test]
    fn edit_05_validate_rejects_invalid_clone_and_move_nodes() {
        let dom = dom("<root><a/></root>");
        let root = dom.root();
        let missing = NodeId(dom.node_count() as u32);
        let invalid_clone = plan(
            PartId(0),
            vec![NodeEdit::InsertClone {
                parent: Target::Node(root),
                before: None,
                source: missing,
            }],
        );
        assert_invalid(&invalid_clone, &dom);

        let invalid_move = plan(
            PartId(0),
            vec![NodeEdit::Move { node: missing, parent: Target::Node(root), before: None }],
        );
        assert_invalid(&invalid_move, &dom);
    }

    #[test]
    fn edit_05_validate_accepts_valid_plan_and_commit_preserves_result() {
        let mut dom = dom("<root><a/></root>");
        let root = dom.root();
        let mut plan = plan(
            PartId(0),
            vec![NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node: NewElement::new(QName::w(LocalName::P)),
            }],
        );
        plan.touch(root);
        plan.structure_changed = true;
        plan.diagnostics.push(diag("commit"));
        plan.offset_delta.push((root, Utf16Offset(0), 1));
        plan.validate(&dom).unwrap();
        let result = plan.commit(&mut dom);
        assert_eq!(result.affected_blocks, vec![root]);
        assert!(result.structure_changed);
        assert_eq!(result.diagnostics, vec![diag("commit")]);
        assert_eq!(result.offset_delta, vec![(root, Utf16Offset(0), 1)]);
        assert_eq!(result.created.len(), 1);
        assert!(result.created[0].is_some());
    }

    #[test]
    fn edit_05_diagnostic_origin_is_preserved_in_plan() {
        let d = diag("origin");
        assert_eq!(d.origin, ValidationOrigin::EngineInvariantViolation);
        let mut plan = MutationPlan::new(PartId(0));
        plan.diagnostics.push(d.clone());
        assert_eq!(plan.diagnostics, vec![d]);
    }
}
