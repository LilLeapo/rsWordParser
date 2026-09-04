//! 表单域定义 `w:ffData` 的读取（`FLD-10` 读侧）。
//!
//! 写侧（`ToggleCheckbox` / `SetFormText` / 下拉选择）在任务 2.9；这里只把状态读出来，
//! 供 `FLD-06` 的策略降级（FORMCHECKBOX 没有 `w:checkBox` → `Unknown`）与 2.5 的显示使用。

use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// `w:ffData` 里那个决定形态的子元素（三者其一，`FLD-10`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormData {
    /// `w:checkBox`。
    CheckBox {
        node: NodeId,
        /// `checked ?? default`（`FLD-10`）。
        checked: bool,
        default: Option<bool>,
        /// `w:size`（半点）；`w:sizeAuto` 时为 `None`。
        size: Option<u32>,
    },
    /// `w:ddList`。
    DropDown {
        node: NodeId,
        /// `w:result`：选中的 `w:listEntry` 下标。
        result: Option<u32>,
        default: Option<u32>,
        entries: Vec<String>,
    },
    /// `w:textInput`。
    TextInput {
        node: NodeId,
        /// `w:type`：`regular` / `number` / `date` / `currentTime` / `currentDate` / `calculated`。
        kind: Option<String>,
        default: Option<String>,
        max_length: Option<u32>,
        format: Option<String>,
    },
}

impl FormData {
    /// 下拉当前选中的条目文本。
    pub fn selected_entry(&self) -> Option<&str> {
        match self {
            Self::DropDown { result, default, entries, .. } => {
                let i = result.or(*default).unwrap_or(0) as usize;
                entries.get(i).map(String::as_str)
            }
            _ => None,
        }
    }
}

/// 读 `w:ffData`（`None` 或没有三种子元素时返回 `None`）。
pub fn read_form_data(dom: &Dom, ff_data: Option<NodeId>) -> Option<FormData> {
    let ff = ff_data?;
    for c in dom.semantic_children(ff) {
        match dom.name(c)?.local {
            LocalName::CheckBox if dom.is(c, w(LocalName::CheckBox)) => {
                let default = child_on_off(dom, c, LocalName::Default);
                let checked = child_on_off(dom, c, LocalName::Checked);
                return Some(FormData::CheckBox {
                    node: c,
                    checked: checked.or(default).unwrap_or(false),
                    default,
                    size: child_u32(dom, c, LocalName::Size),
                });
            }
            LocalName::DdList if dom.is(c, w(LocalName::DdList)) => {
                let entries = dom
                    .semantic_children(c)
                    .filter(|&e| dom.is(e, w(LocalName::ListEntry)))
                    .map(|e| val(dom, e).unwrap_or_default())
                    .collect();
                return Some(FormData::DropDown {
                    node: c,
                    result: child_u32(dom, c, LocalName::Result),
                    default: child_u32(dom, c, LocalName::Default),
                    entries,
                });
            }
            LocalName::TextInput if dom.is(c, w(LocalName::TextInput)) => {
                return Some(FormData::TextInput {
                    node: c,
                    kind: child_val(dom, c, LocalName::Type),
                    default: child_val(dom, c, LocalName::Default),
                    max_length: child_u32(dom, c, LocalName::MaxLength),
                    format: child_val(dom, c, LocalName::Format),
                });
            }
            _ => {}
        }
    }
    None
}

/// `w:name`（表单域名字，`FLD-10`）。
pub fn form_name(dom: &Dom, ff_data: Option<NodeId>) -> Option<String> {
    let ff = ff_data?;
    dom.semantic_children(ff).find(|&c| dom.is(c, w(LocalName::Name))).and_then(|c| val(dom, c))
}

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

fn val(dom: &Dom, node: NodeId) -> Option<String> {
    dom.attr_value(node, w(LocalName::Val)).map(|v| v.into_owned())
}

fn child(dom: &Dom, parent: NodeId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(parent).find(|&c| dom.is(c, w(local)))
}

fn child_val(dom: &Dom, parent: NodeId, local: LocalName) -> Option<String> {
    val(dom, child(dom, parent, local)?)
}

fn child_u32(dom: &Dom, parent: NodeId, local: LocalName) -> Option<u32> {
    child_val(dom, parent, local)?.trim().parse().ok()
}

/// `FLD-10`：元素存在而无 `w:val` → true；`w:val ∈ {1, true, on}` → true；元素不存在 → `None`。
fn child_on_off(dom: &Dom, parent: NodeId, local: LocalName) -> Option<bool> {
    let node = child(dom, parent, local)?;
    Some(match val(dom, node) {
        None => true,
        Some(v) => matches!(v.trim(), "1" | "true" | "on" | "True" | "TRUE"),
    })
}
