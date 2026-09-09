//! 属性表（`spec/05-properties.md`，`docs/03` §6.1）。
//!
//! `RunProps`、`ParaProps` 等类型与 `read_* / diff_* / emit_* / order_index_*` 函数由 `build.rs`
//! 从 `schema/props/*.toml` 生成（`PROP-01`、`PROP-07`），本文件只放手写的公共类型：
//!
//! - [`Val`]：解析失败时保留原文的值（`PROP-09`）；
//! - [`Change`] / [`TableChange`]：patch 的字段变更（`PROP-06`）；
//! - [`NewElement`] / [`NodeEdit`]（定义在 `xml::plan`）：codec 生成的新元素描述与 `plan_apply_*`
//!   产出的机械变更，`Dom::apply_edits` 执行；
//! - [`FieldInfo`] / [`TableInfo`] / [`AttrInfo`]：表元数据，供合并算法与工具泛型使用。
//!
//! 与 `PROP-07` 措辞的差别：`read_*` 多一个 `&mut Vec<Diagnostic>` 参数收集 `PROP_BAD_VALUE`；
//! `order_index_*` 的参数按值传 `QName`（它是 `Copy`）。

use std::borrow::Cow;

use crate::diag::Diagnostic;
use crate::package::PartFlavor;
use crate::xml::dom::{Dom, NodeId};
use crate::xml::names::{LocalName, NsId, QName};
pub use crate::xml::plan::{NewElement, NewNode, NodeEdit, Target};

pub mod codec;
mod read;
pub(crate) mod serde;
mod table;

pub use codec::{Codec, HexColorOrAuto, Measure};
pub use read::{Ctx, emit_val, read_attr, read_val, spell};
use read::{plan_multi, plan_raw, plan_single};

/// 解析结果（`PROP-09`）：能理解的值，或原文。比较按 `derive` 语义（`Raw` 与任何 `Value` 不等），
/// 写回时 `Raw` 原文输出，保证读到什么就能写回什么。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Val<T> {
    Value(T),
    /// codec 无法解析的原文，已记 `PROP_BAD_VALUE`。
    Raw(String),
}

impl<T> Val<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Val::Value(v) => Some(v),
            Val::Raw(_) => None,
        }
    }

    pub fn raw(&self) -> Option<&str> {
        match self {
            Val::Value(_) => None,
            Val::Raw(s) => Some(s),
        }
    }

    pub fn is_raw(&self) -> bool {
        matches!(self, Val::Raw(_))
    }

    /// 写回：`Value` 交给 `f`，`Raw` 原文。
    pub fn write<'s>(&'s self, f: impl FnOnce(&'s T) -> Cow<'s, str>) -> Cow<'s, str> {
        match self {
            Val::Value(v) => f(v),
            Val::Raw(s) => Cow::Borrowed(s),
        }
    }
}

impl<T> From<T> for Val<T> {
    fn from(v: T) -> Self {
        Val::Value(v)
    }
}

/// patch 里一个字段的变更（`PROP-06`）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Change<T> {
    #[default]
    Keep,
    /// 删除该字段的元素。
    Unset,
    /// 替换为新值（`multi` 字段：完整的新列表，替换整个子列表）。
    Set(T),
}

/// [`Change::kind`] / [`TableChange::kind`] 的无载荷视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Keep,
    Unset,
    Set,
    /// 嵌套表的局部变更。
    Patch,
}

impl<T: Clone + PartialEq> Change<T> {
    /// 单值字段：相等 `Keep`，`b` 缺席 `Unset`，否则 `Set(b)`。
    pub fn diff(a: &Option<T>, b: &Option<T>) -> Change<T> {
        match (a, b) {
            (x, y) if x == y => Change::Keep,
            (_, None) => Change::Unset,
            (_, Some(v)) => Change::Set(v.clone()),
        }
    }
}

impl<T: Clone + PartialEq> Change<Vec<T>> {
    /// `multi` 字段：列表相等 `Keep`，`b` 为空 `Unset`，否则 `Set(整表)`。
    pub fn diff_multi(a: &[T], b: &[T]) -> Change<Vec<T>> {
        if a == b {
            Change::Keep
        } else if b.is_empty() {
            Change::Unset
        } else {
            Change::Set(b.to_vec())
        }
    }
}

impl<T> Change<T> {
    pub fn is_keep(&self) -> bool {
        matches!(self, Change::Keep)
    }

    pub fn kind(&self) -> ChangeKind {
        match self {
            Change::Keep => ChangeKind::Keep,
            Change::Unset => ChangeKind::Unset,
            Change::Set(_) => ChangeKind::Set,
        }
    }
}

/// 生成的 `XxxPatch` 都实现它，让 [`TableChange`] 能判断嵌套 patch 是否为空。
pub trait PropsPatch {
    fn is_empty(&self) -> bool;
}

/// 嵌套表字段（`pPr/rPr`、`pPr/pBdr`）的变更：可整体设置，也可只给子 patch。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TableChange<T, P> {
    #[default]
    Keep,
    Unset,
    /// 整体替换；容器已存在时按 `diff(现值, T)` 递归合并，未建模子元素仍原位保留。
    Set(T),
    /// 只改子 patch 里给出的字段。
    Patch(P),
}

impl<T: Clone + PartialEq, P: PropsPatch> TableChange<T, P> {
    pub fn diff(a: &Option<T>, b: &Option<T>, diff: impl FnOnce(&T, &T) -> P) -> Self {
        match (a, b) {
            (x, y) if x == y => TableChange::Keep,
            (_, None) => TableChange::Unset,
            (None, Some(v)) => TableChange::Set(v.clone()),
            (Some(x), Some(y)) => TableChange::Patch(diff(x, y)),
        }
    }
}

impl<T, P: PropsPatch> TableChange<T, P> {
    /// `Keep`，或内容为空的 `Patch`。
    pub fn is_keep(&self) -> bool {
        match self {
            TableChange::Keep => true,
            TableChange::Patch(p) => p.is_empty(),
            _ => false,
        }
    }

    pub fn kind(&self) -> ChangeKind {
        match self {
            TableChange::Keep => ChangeKind::Keep,
            TableChange::Unset => ChangeKind::Unset,
            TableChange::Set(_) => ChangeKind::Set,
            TableChange::Patch(p) if p.is_empty() => ChangeKind::Keep,
            TableChange::Patch(_) => ChangeKind::Patch,
        }
    }
}

/// 字段的建模方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// 单 `w:val` 属性的元素。
    Scalar,
    /// 带多个属性的元素（`types.toml` 的 `struct`）。
    Struct,
    /// 嵌套容器（另一张表）。
    Table,
    /// 整个元素按 DOM 保留，字段值是 `NodeId`。
    Raw,
}

/// 一行属性表（`PROP-01` 的列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Rust 字段名。
    pub name: &'static str,
    /// 子元素 QName（Strict 拼写）。
    pub element: QName,
    /// Transitional 拼写（`w:left` 对 `w:start`），解析两者都接受。
    pub legacy: Option<QName>,
    /// 在容器 `order` 列表中的序号（`PROP-05`）。
    pub order: u16,
    pub kind: FieldKind,
    /// 允许多次出现（`Vec<T>`）。
    pub multi: bool,
    /// 出现在 `*PrChange` 旧值快照里。
    pub in_change: bool,
    /// Cs 孪生字段名（`PROP-03`），只作元数据。
    pub cs_twin: Option<&'static str>,
}

/// 一张属性表。
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: &'static str,
    pub element: QName,
    /// 修订快照容器（`w:rPrChange`）。
    pub change: Option<QName>,
    /// 容器元素自身的属性（`w:lvl/@ilvl`）。
    pub attrs: &'static [AttrInfo],
    pub fields: &'static [FieldInfo],
    /// 容器任一子元素名 → schema 序号；表外为 `None`。
    pub order_index: fn(QName) -> Option<u16>,
}

impl TableInfo {
    pub fn field(&self, name: &str) -> Option<&'static FieldInfo> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// `struct` 类型的一个属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrInfo {
    pub name: &'static str,
    pub attr: QName,
    pub legacy: Option<QName>,
}

include!(concat!(env!("OUT_DIR"), "/props.rs"));

#[cfg(test)]
mod tests;
