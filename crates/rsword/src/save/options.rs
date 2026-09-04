//! `SAVE-07` 保存选项与 `SAVE-01` 第 4 步 `apply_save_options`。
//!
//! 每项选项都翻译成普通 DOM 变更（经 [`MutationPlan`]，脏标记规则与编辑操作完全一致），没有旁路。
//! M1 实现 `saved_at`（`docProps/core.xml` 的 `dcterms:modified` 与 `cp:revision`）与
//! `remove_personal_info`（`word/settings.xml` 的标志 + 全包清洗）；节 / 页眉页脚 / 页面颜色等
//! 需要新建 part 或 `sectPr` 属性表，属 M5（`SAVE-05`）。
//!
//! 清洗规则与 TS `scrubPersonalMetadata` 对齐：除 `customXml/*` 与 `docProps/custom.xml` 外的每个 XML part 里
//! `w:author`（含无前缀的 `author`）改为 `Author`、`w:initials` 改为 `A`；`core.xml` 的 `dc:creator` 与
//! `cp:lastModifiedBy` 清空；`app.xml` 的 `Manager` / `Company` 清空；`word/people.xml` 的 `w15:person` 整条删除。
//! `w:date` **保留**（TS 行为；`spec/09` 的措辞见 `docs/04` §8）。

use crate::diag::{DiagCode, Diagnostic};
use crate::edit::MutationPlan;
use crate::error::Result;
use crate::package::{Package, PartId, RelType};
use crate::xml::{Dirty, Dom, LocalName, NodeEdit, NodeId, NodeKind, NsId, QName, Target};

/// `SAVE-07`：与 TS `SaveOptions` 对齐的保存选项（M1 子集）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// `docProps/core.xml` 的 `dcterms:modified`（ISO 8601，毫秒被去掉）。
    ///
    /// 只有保存真的进入序列化路径时才落盘：单独设置它**不会**让一份未编辑的文档产生输出
    /// （不变式 1 优先，与 TS 的 `isUnchanged` 判定一致）。
    pub saved_at: Option<String>,
    /// `word/settings.xml` 的 `w:removePersonalInformation`：`Some` 写入该值并按该值决定是否清洗；
    /// `None` 沿用文档已有的标志。
    pub remove_personal_info: Option<bool>,
}

impl SaveOptions {
    pub fn is_empty(&self) -> bool {
        *self == SaveOptions::default()
    }

    /// 是否要求保存必须进入序列化路径（即使没有脏节点）。
    pub fn forces_save(&self) -> bool {
        self.remove_personal_info.is_some()
    }
}

const CORE_PROPS: &str = "docProps/core.xml";
const APP_PROPS: &str = "docProps/app.xml";
const CUSTOM_PROPS: &str = "docProps/custom.xml";
const PEOPLE: &str = "word/people.xml";
const SETTINGS: &str = "word/settings.xml";

fn w(local: LocalName) -> QName {
    QName::w(local)
}

fn live(dom: &Dom, id: NodeId) -> bool {
    dom.node(id).dirty != Dirty::Deleted
}

/// 子树里名字匹配的活元素（含根自身）。
fn elements(dom: &Dom, name: QName) -> Vec<NodeId> {
    dom.descendants(dom.root()).filter(|&n| live(dom, n) && dom.is(n, name)).collect()
}

/// 清空元素内容（保留标签，与 TS `clearQualifiedElements` 一致）。
fn clear_element(dom: &Dom, id: NodeId, plan: &mut MutationPlan) {
    for c in dom.children(id).iter().copied().filter(|&c| live(dom, c)) {
        match &dom.node(c).kind {
            NodeKind::Text(_) => {
                if dom.text(c).is_some_and(|t| !t.is_empty()) {
                    plan.node_edits.push(NodeEdit::SetText { node: c, text: String::new() });
                }
            }
            _ => plan.node_edits.push(NodeEdit::Delete(c)),
        }
    }
}

/// 设置元素的唯一文本子节点；没有文本子节点则不动（TS 不注入缺失的标签）。
fn set_element_text(dom: &Dom, id: NodeId, text: &str, plan: &mut MutationPlan) {
    let Some(node) = dom
        .children(id)
        .iter()
        .copied()
        .find(|&c| live(dom, c) && matches!(dom.node(c).kind, NodeKind::Text(_)))
    else {
        return;
    };
    if dom.text(node).as_deref() != Some(text) {
        plan.node_edits.push(NodeEdit::SetText { node, text: text.to_string() });
    }
}

/// TS `patchCoreProps`：`.mmmZ` → `Z`。
fn normalize_timestamp(ts: &str) -> String {
    let Some(head) = ts.strip_suffix('Z') else { return ts.to_string() };
    match head.rsplit_once('.') {
        Some((before, frac)) if frac.len() == 3 && frac.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{before}Z")
        }
        _ => ts.to_string(),
    }
}

/// `dcterms:modified` ← `iso`，`cp:revision` ← +1（都只在标签存在时）。
fn plan_core_props(dom: &Dom, part: PartId, iso: &str) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    for id in elements(dom, QName::new(NsId::DcTerms, LocalName::Modified)) {
        set_element_text(dom, id, iso, &mut plan);
    }
    for id in elements(dom, QName::new(NsId::Cp, LocalName::Revision)) {
        let next = dom
            .children(id)
            .iter()
            .copied()
            .find(|&c| live(dom, c) && matches!(dom.node(c).kind, NodeKind::Text(_)))
            .and_then(|c| dom.text(c)?.trim().parse::<u64>().ok())
            .map(|n| n + 1);
        if let Some(n) = next {
            set_element_text(dom, id, &n.to_string(), &mut plan);
        }
    }
    plan
}

/// `word/settings.xml` 的 `w:removePersonalInformation`：`on` 时插为第一个子元素（`PROP-05` 的
/// 序号由 `plan_apply_settings` 保证，这里直接用属性表计划），否则删除。
fn plan_settings_flag(dom: &Dom, part: PartId, on: bool) -> MutationPlan {
    use crate::semantic::props::{Change, SettingsPatch, plan_apply_settings};
    let root = dom.root();
    let patch = SettingsPatch {
        remove_personal_information: if on { Change::Set(true) } else { Change::Unset },
        ..Default::default()
    };
    let mut plan = MutationPlan::new(part);
    plan.node_edits = plan_apply_settings(dom, root, Some(root), &patch, dom.flavor());
    plan
}

/// 一个 part 的清洗计划。
fn plan_scrub(dom: &Dom, part: PartId, uri: &str) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let author = w(LocalName::Author);
    let initials = w(LocalName::Initials);
    let bare_author = QName::new(NsId::None, LocalName::Author);
    let bare_initials = QName::new(NsId::None, LocalName::Initials);
    for id in dom.descendants(dom.root()) {
        if !live(dom, id) {
            continue;
        }
        let Some(e) = dom.element(id) else { continue };
        for a in &e.attrs {
            let replacement = if a.name == author || a.name == bare_author {
                "Author"
            } else if a.name == initials || a.name == bare_initials {
                "A"
            } else {
                continue;
            };
            if dom.attr_str(a) != replacement {
                plan.node_edits.push(NodeEdit::SetAttr {
                    node: Target::Node(id),
                    name: a.name,
                    value: replacement.to_string(),
                });
            }
        }
    }
    if uri.eq_ignore_ascii_case(CORE_PROPS) {
        for name in [
            QName::new(NsId::Dc, LocalName::Creator),
            QName::new(NsId::Cp, LocalName::LastModifiedBy),
        ] {
            for id in elements(dom, name) {
                clear_element(dom, id, &mut plan);
            }
        }
    }
    if uri.eq_ignore_ascii_case(APP_PROPS) {
        for local in [LocalName::Manager, LocalName::Company] {
            for id in elements(dom, QName::new(NsId::Ep, local)) {
                clear_element(dom, id, &mut plan);
            }
        }
    }
    if uri.eq_ignore_ascii_case(PEOPLE) {
        for id in elements(dom, QName::new(NsId::W15, LocalName::Person)) {
            plan.node_edits.push(NodeEdit::Delete(id));
        }
    }
    plan
}

/// 该 part 是否参与清洗（`customXml/*` 与自定义属性由 TS 与我们一致地放过）。
fn scrubbable(uri: &str) -> bool {
    !uri.starts_with("customXml/") && !uri.eq_ignore_ascii_case(CUSTOM_PROPS)
}

/// `SAVE-01` 第 4 步：把选项翻译成各 part 的计划（只读产出）+ 计划外的诊断。
pub(crate) fn plan_all(
    pkg: &mut Package,
    opts: &SaveOptions,
    scrub: bool,
) -> Result<(Vec<MutationPlan>, Vec<Diagnostic>)> {
    let mut plans = Vec::new();
    let mut diags = Vec::new();
    let main = pkg.main_part();

    if let Some(ts) = &opts.saved_at {
        let iso = normalize_timestamp(ts);
        if let Some(id) = pkg.find_name(CORE_PROPS) {
            pkg.dom(id)?;
            if let Some(dom) = pkg.part(id).dom() {
                let plan = plan_core_props(dom, id, &iso);
                if !plan.is_empty() {
                    plans.push(plan);
                }
            }
        }
    }

    if let Some(on) = opts.remove_personal_info {
        let settings =
            pkg.related(main, RelType::Settings).next().or_else(|| pkg.find_name(SETTINGS));
        match settings {
            Some(id) => {
                pkg.dom(id)?;
                if let Some(dom) = pkg.part(id).dom() {
                    let plan = plan_settings_flag(dom, id, on);
                    if !plan.is_empty() {
                        plans.push(plan);
                    }
                }
            }
            None => diags.push(Diagnostic::invariant_violation(
                main,
                None,
                DiagCode::EditUnsupported,
                "没有 word/settings.xml，removePersonalInformation 标志未写入（新建 part 属 SAVE-05）"
                    .to_string(),
            )),
        }
    }

    if scrub {
        let ids: Vec<(PartId, String)> = pkg
            .parts()
            .iter()
            .filter(|p| p.is_xml)
            .map(|p| (p.id, p.uri.as_str().to_string()))
            .filter(|(_, uri)| scrubbable(uri))
            .collect();
        for (id, uri) in ids {
            pkg.dom(id)?;
            if let Some(dom) = pkg.part(id).dom() {
                let plan = plan_scrub(dom, id, &uri);
                if !plan.is_empty() {
                    plans.push(plan);
                }
            }
        }
    }
    Ok((plans, diags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_07_timestamp_drops_milliseconds() {
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00.123Z"), "2026-07-28T08:30:00Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00Z"), "2026-07-28T08:30:00Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00.12Z"), "2026-07-28T08:30:00.12Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00+08:00"), "2026-07-28T08:30:00+08:00");
    }

    #[test]
    fn save_07_custom_xml_is_not_scrubbed() {
        assert!(!scrubbable("customXml/item1.xml"));
        assert!(!scrubbable("docProps/custom.xml"));
        assert!(scrubbable("docProps/core.xml"));
        assert!(scrubbable("word/glossary/document.xml"));
    }
}
