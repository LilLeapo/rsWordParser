//! 延迟执行的 DOM 变更（`EDIT-05` 的 `MutationPlan.node_edits`，`PROP-07` 的 `plan_apply_*` 输出）。
//!
//! `plan` 阶段只读 DOM、产出 [`NodeEdit`] 列表；`commit` 阶段用 [`Dom::apply_edits`] 机械执行，
//! 每一步只调用 `xml::edit` 的原语，因此脏规则（`XML-12`）自动成立。
//! 同一计划里后面的编辑可以用 [`Target::New`] 指向前面编辑创建的节点。

use crate::xml::Dirty;
use crate::xml::dom::{Dom, NodeId, NodeKind};
use crate::xml::interner::Interner;
use crate::xml::names::{LocalName, NsId, QName};

/// 跨 DOM 的名字映射：已知名原样，`Other` / `Unbound` 按字符串在目标 interner 里重新登记。
pub(crate) fn map_qname(src: &Dom, q: QName, target: &mut Interner) -> QName {
    let si = src.interner();
    let ns = match q.ns {
        NsId::Other(id) => NsId::Other(target.intern(si.resolve(id))),
        NsId::Unbound(id) => NsId::Unbound(target.intern(si.resolve(id))),
        other => other,
    };
    let local = match q.local {
        LocalName::Other(id) => LocalName::Other(target.intern(si.resolve(id))),
        other => other,
    };
    QName::new(ns, local)
}

/// 待创建的元素：与 DOM 无关的描述（codec 的输出），[`NewElement::materialize`] 落成 `New` 子树。
/// 命名空间声明由序列化按作用域补（`XML-14`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewElement {
    pub name: QName,
    /// 按生成顺序；值已是解码后的文本，序列化时再转义。
    pub attrs: Vec<(QName, String)>,
    pub children: Vec<NewNode>,
}

/// 新子树里的一个子节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewNode {
    Element(NewElement),
    Text(String),
}

impl NewElement {
    pub fn new(name: QName) -> Self {
        Self { name, attrs: Vec::new(), children: Vec::new() }
    }

    pub fn push_attr(&mut self, name: QName, value: impl Into<String>) {
        self.attrs.push((name, value.into()));
    }

    pub fn with_attr(mut self, name: QName, value: impl Into<String>) -> Self {
        self.push_attr(name, value);
        self
    }

    pub fn push_child(&mut self, child: NewElement) {
        self.children.push(NewNode::Element(child));
    }

    pub fn with_child(mut self, child: NewElement) -> Self {
        self.push_child(child);
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.children.push(NewNode::Text(text.into()));
        self
    }

    /// 子元素（不含文本）。
    pub fn child_elements(&self) -> impl Iterator<Item = &NewElement> {
        self.children.iter().filter_map(|c| match c {
            NewNode::Element(e) => Some(e),
            NewNode::Text(_) => None,
        })
    }

    /// 把另一棵 DOM（例如 [`crate::xml::fragment::parse_fragment`] 的临时树）里的子树转成与目标
    /// DOM 无关的描述：`Other` 命名空间 / 局部名按字符串重新 intern 到 `target`；`Deleted` 节点、
    /// 注释 / PI 跳过；文本按解码后的内容保留（不 trim）。非元素节点返回 `None`。
    pub fn from_dom(src: &Dom, node: NodeId, target: &mut Interner) -> Option<NewElement> {
        let e = src.element(node)?;
        if src.node(node).dirty == Dirty::Deleted {
            return None;
        }
        let mut out = NewElement::new(map_qname(src, e.name, target));
        for a in &e.attrs {
            out.push_attr(map_qname(src, a.name, target), src.attr_str(a).into_owned());
        }
        for &c in &e.children {
            match &src.node(c).kind {
                NodeKind::Element(_) => {
                    if let Some(child) = NewElement::from_dom(src, c, target) {
                        out.push_child(child);
                    }
                }
                NodeKind::Text(_) => {
                    if src.node(c).dirty != Dirty::Deleted
                        && let Some(t) = src.text(c)
                    {
                        out.children.push(NewNode::Text(t.into_owned()));
                    }
                }
                NodeKind::Opaque => {}
            }
        }
        Some(out)
    }

    /// 在 `dom` 里创建游离的 `New` 子树，返回根节点。
    pub fn materialize(&self, dom: &mut Dom) -> NodeId {
        let id = dom.new_element(self.name);
        for (name, value) in &self.attrs {
            dom.set_attr(id, *name, value.clone());
        }
        for child in &self.children {
            let c = match child {
                NewNode::Element(e) => e.materialize(dom),
                NewNode::Text(t) => dom.new_text(t.clone()),
            };
            dom.append_child(id, c);
        }
        id
    }
}

/// 编辑的目标节点：已有节点，或同一计划里第 `k` 条编辑创建的节点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Node(NodeId),
    /// `edits[k]` 创建的根节点（`Insert` / `InsertClone` / `Replace` / `ReplaceClone`）。
    New(usize),
}

/// 一条机械变更。`before` 是现有的兄弟节点（`None` = 追加到末尾）；用兄弟而不是下标定位，
/// 同一计划里前面的插入 / 删除不会让它失效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEdit {
    /// 把新子树插到 `parent` 的 `before` 之前。
    Insert {
        parent: Target,
        before: Option<NodeId>,
        node: NewElement,
    },
    /// 克隆现有子树 `source` 插到 `parent` 的 `before` 之前（`XML-12` 规则 F）。
    InsertClone {
        parent: Target,
        before: Option<NodeId>,
        source: NodeId,
    },
    /// 用新子树替换 `old`：新节点插在原位置，`old` 变 `Deleted`。
    Replace {
        old: NodeId,
        node: NewElement,
    },
    /// 用 `source` 的克隆替换 `old`。
    ReplaceClone {
        old: NodeId,
        source: NodeId,
    },
    Delete(NodeId),
    /// 设置属性（同名替换；节点变 `SelfDirty`）。
    SetAttr {
        node: Target,
        name: QName,
        value: String,
    },
    /// 删除全部同名属性。
    RemoveAttr {
        node: Target,
        name: QName,
    },
    /// 替换文本节点内容（`Owned`；节点 `SelfDirty`，父 `w:t` 由序列化补 `xml:space`）。
    SetText {
        node: NodeId,
        text: String,
    },
    /// 给现有元素改名（节点变 `SelfDirty`，子树与属性不动）。修订生成用它把
    /// `w:t → w:delText`、`w:instrText → w:delInstrText`（`spec/18` 7.2）。
    Rename {
        node: NodeId,
        name: QName,
    },
    /// 同 part 移动现有子树到 `parent` 的 `before` 之前（`XML-12` 规则 E）。
    Move {
        node: NodeId,
        parent: Target,
        before: Option<NodeId>,
    },
}

impl NodeEdit {
    /// 该编辑是否创建新节点（[`Target::New`] 可以指向它）。
    pub fn creates(&self) -> bool {
        !matches!(
            self,
            NodeEdit::Delete(_)
                | NodeEdit::SetAttr { .. }
                | NodeEdit::RemoveAttr { .. }
                | NodeEdit::SetText { .. }
                | NodeEdit::Rename { .. }
                | NodeEdit::Move { .. }
        )
    }
}

impl Dom {
    /// 机械执行一组编辑，返回每条编辑创建的节点（`Delete` 为 `None`）。
    ///
    /// # Panics
    /// `Target::New(k)` 指向不存在或不创建节点的编辑；`Replace` 的 `old` 没有父节点。
    /// 这些都是 plan 阶段的错误，`validate` 应当在 commit 前拦下。
    pub fn apply_edits(&mut self, edits: &[NodeEdit]) -> Vec<Option<NodeId>> {
        let mut created: Vec<Option<NodeId>> = Vec::with_capacity(edits.len());
        let resolve = |t: Target, created: &[Option<NodeId>]| -> NodeId {
            match t {
                Target::Node(n) => n,
                Target::New(k) => created
                    .get(k)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| panic!("apply_edits: Target::New({k}) 未创建节点")),
            }
        };
        for edit in edits {
            let made = match edit {
                NodeEdit::Insert { parent, before, node } => {
                    let p = resolve(*parent, &created);
                    let idx = self.insert_index(p, *before);
                    let id = node.materialize(self);
                    self.insert_child(p, idx, id);
                    Some(id)
                }
                NodeEdit::InsertClone { parent, before, source } => {
                    let p = resolve(*parent, &created);
                    let idx = self.insert_index(p, *before);
                    Some(self.insert_clone(*source, p, idx))
                }
                NodeEdit::Replace { old, node } => {
                    let p = self.parent(*old).expect("Replace: old 没有父节点");
                    let idx = self.child_index(p, *old).expect("Replace: old 不在父节点的子列表里");
                    let id = node.materialize(self);
                    self.insert_child(p, idx, id);
                    self.delete(*old);
                    Some(id)
                }
                NodeEdit::ReplaceClone { old, source } => {
                    let p = self.parent(*old).expect("ReplaceClone: old 没有父节点");
                    let idx =
                        self.child_index(p, *old).expect("ReplaceClone: old 不在父节点的子列表里");
                    let id = self.insert_clone(*source, p, idx);
                    self.delete(*old);
                    Some(id)
                }
                NodeEdit::Delete(n) => {
                    self.delete(*n);
                    None
                }
                NodeEdit::SetAttr { node, name, value } => {
                    let n = resolve(*node, &created);
                    self.set_attr(n, *name, value.clone());
                    None
                }
                NodeEdit::RemoveAttr { node, name } => {
                    let n = resolve(*node, &created);
                    self.remove_attr(n, *name);
                    None
                }
                NodeEdit::SetText { node, text } => {
                    self.set_text(*node, text.clone());
                    None
                }
                NodeEdit::Rename { node, name } => {
                    self.rename_element(*node, *name);
                    None
                }
                NodeEdit::Move { node, parent, before } => {
                    let p = resolve(*parent, &created);
                    // 先算目标下标：`node` 若已在 `p` 里且位于 `before` 之前，摘下后下标会前移
                    let mut idx = self.insert_index(p, *before);
                    if self.parent(*node) == Some(p)
                        && self.child_index(p, *node).is_some_and(|i| i < idx)
                    {
                        idx -= 1;
                    }
                    self.move_within_part(*node, p, idx);
                    None
                }
            };
            created.push(made);
        }
        created
    }

    fn insert_index(&self, parent: NodeId, before: Option<NodeId>) -> usize {
        match before {
            Some(b) => self.child_index(parent, b).expect("before 不是 parent 的子节点"),
            None => self.children(parent).len(),
        }
    }
}
