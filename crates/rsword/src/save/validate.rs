//! 保存前校验（`SAVE-02`）与命名空间收尾（`SAVE-03` / `XML-14`）。M1 任务 1.14 的第一批：
//!
//! - 前缀绑定：`New` / `SelfDirty` 节点的元素名、属性名不得是 `NsId::Unbound`（`Clean` 节点的未绑定前缀
//!   是解析期已记录的 `PreExistingDamage`，不在此重复）。
//! - `PROP-05` 顺序：属性容器（`semantic::props::TABLES` 里的元素）中 `New` / `SelfDirty` 的子元素，
//!   其 schema 序号必须落在前后已知序号之间。
//! - 扩展命名空间（`w14 w15 w16* wp14`）：`New` / `SelfDirty` 节点用到时，part 根必须声明该命名空间并把
//!   前缀列进 `mc:Ignorable`，否则 Word 会拒绝文件；这是 `XML-14` 允许改动 part 根的唯一情形。
//!
//! 失败来源都是本次编辑造成的（`EngineInvariantViolation`）：调试构建与 CI 下 [`enforce`] 返回
//! `Err(SAVE_INVARIANT)`，发布构建只记诊断继续保存。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic, ValidationOrigin};
use crate::error::{Error, Result};
use crate::semantic::props::{TABLES, TableInfo};
use crate::xml::ns::PrefixUse;
use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

fn is_dirty_self(d: Dirty) -> bool {
    matches!(d, Dirty::New | Dirty::SelfDirty)
}

/// 这次编辑动过表格的**结构**（列、格、跨度、行首尾空档）——只有动过才谈得上是我们把网格弄坏的。
/// 语料里本来就有网格不一致的文档（`table-grid-reconcile__*`），那是 `PreExistingDamage`，
/// 解析时已记 `MOD_TABLE_SHAPE`，保存时不该再拦。
fn grid_structure_touched(dom: &Dom, tbl: NodeId) -> bool {
    dom.descendants(tbl).any(|n| {
        // `DescendantDirty` 只说明"里面有东西变了"（格里改了字），结构没动
        matches!(dom.node(n).dirty, Dirty::New | Dirty::Deleted | Dirty::SelfDirty)
            && dom.name(n).is_some_and(|q| {
                q.ns == NsId::W
                    && matches!(
                        q.local,
                        LocalName::GridCol
                            | LocalName::Tc
                            | LocalName::GridSpan
                            | LocalName::GridBefore
                            | LocalName::GridAfter
                    )
            })
    })
}

/// `SAVE-02`：`w:tbl` 各行的网格宽度（`gridBefore + Σ gridSpan + gridAfter`）应等于 `tblGrid` 的列数。
/// 只在这次编辑动过表格结构时检查。
fn table_grid_mismatch(dom: &Dom, tbl: NodeId) -> Option<String> {
    if !grid_structure_touched(dom, tbl) {
        return None;
    }
    let live = |n: NodeId| dom.element(n).is_some() && dom.node(n).dirty != Dirty::Deleted;
    let kids = |n: NodeId| dom.children(n).iter().copied().filter(|&c| live(c));
    let num = |n: NodeId, name: LocalName| -> u32 {
        kids(n)
            .find(|&c| dom.is(c, QName::w(name)))
            .and_then(|c| dom.attr_value(c, QName::w(LocalName::Val)))
            .and_then(|v| v.trim().parse::<i32>().ok())
            .unwrap_or(0)
            .max(0) as u32
    };
    let cols = kids(tbl)
        .find(|&c| dom.is(c, QName::w(LocalName::TblGrid)))
        .map(|g| kids(g).filter(|&c| dom.is(c, QName::w(LocalName::GridCol))).count() as u32)?;
    if cols == 0 {
        return None;
    }
    let mut bad = Vec::new();
    for (i, tr) in kids(tbl).filter(|&c| dom.is(c, QName::w(LocalName::Tr))).enumerate() {
        let tr_pr = kids(tr).find(|&c| dom.is(c, QName::w(LocalName::TrPr)));
        let (before, after) = tr_pr
            .map(|pr| (num(pr, LocalName::GridBefore), num(pr, LocalName::GridAfter)))
            .unwrap_or((0, 0));
        let spans: u32 = kids(tr)
            .filter(|&c| dom.is(c, QName::w(LocalName::Tc)))
            .map(|tc| {
                kids(tc)
                    .find(|&c| dom.is(c, QName::w(LocalName::TcPr)))
                    .map_or(1, |pr| num(pr, LocalName::GridSpan).max(1))
            })
            .sum();
        let width = before + spans + after;
        if width != cols {
            bad.push((i, width));
        }
    }
    (!bad.is_empty()).then(|| format!("表格 tblGrid 有 {cols} 列，但这些行的网格宽度不符：{bad:?}"))
}

fn violation(dom: &Dom, node: NodeId, message: String) -> Diagnostic {
    let range = dom.node(node).lex.as_ref().map(|l| l.range.clone());
    Diagnostic::invariant_violation(dom.part(), range, DiagCode::SaveInvariant, message)
}

/// 对一个 part 做 `SAVE-02` 的 M1 子集检查。只看 `New` / `SelfDirty` 节点，`Clean` 子树不产生诊断。
pub fn validate_part(dom: &Dom) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let root_dirty = dom.node(dom.root()).dirty;
    if root_dirty == Dirty::Clean {
        return out;
    }
    let tables: HashMap<QName, &'static TableInfo> =
        TABLES.iter().map(|t| (t.element, *t)).collect();
    let interner = dom.interner();
    for node in dom.descendants(dom.root()) {
        let n = dom.node(node);
        if n.dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(node) else { continue };
        if is_dirty_self(n.dirty) {
            if let NsId::Unbound(p) = e.name.ns {
                out.push(violation(
                    dom,
                    node,
                    format!(
                        "元素 `{}:{}` 的前缀未绑定到任何命名空间",
                        interner.resolve(p),
                        e.name.local.as_str(interner)
                    ),
                ));
            }
            for a in &e.attrs {
                if let NsId::Unbound(p) = a.name.ns {
                    out.push(violation(
                        dom,
                        node,
                        format!(
                            "元素 `{}` 的属性 `{}:{}` 前缀未绑定",
                            e.name.display(interner),
                            interner.resolve(p),
                            a.name.local.as_str(interner)
                        ),
                    ));
                }
            }
        }
        // SAVE-02：动过的表格，各行的网格宽度要与 tblGrid 的列数对得上
        if dom.is(node, QName::w(LocalName::Tbl))
            && let Some(msg) = table_grid_mismatch(dom, node)
        {
            let range = dom.node(node).lex.as_ref().map(|l| l.range.clone());
            out.push(Diagnostic::invariant_violation(
                dom.part(),
                range,
                DiagCode::SaveTableGrid,
                msg,
            ));
        }
        // PROP-05：容器里 New/SelfDirty 子元素的序号
        if n.dirty == Dirty::Clean {
            continue;
        }
        let Some(table) = tables.get(&e.name) else { continue };
        let kids: Vec<(NodeId, Option<u16>, Dirty)> = dom
            .semantic_children(node)
            .filter_map(|c| dom.name(c).map(|q| (c, (table.order_index)(q), dom.node(c).dirty)))
            .collect();
        for (i, (child, order, dirty)) in kids.iter().enumerate() {
            let (Some(o), true) = (order, is_dirty_self(*dirty)) else { continue };
            let before_max = kids[..i].iter().filter_map(|k| k.1).max();
            let after_min = kids[i + 1..].iter().filter_map(|k| k.1).min();
            if before_max.is_some_and(|b| b > *o) || after_min.is_some_and(|a| a < *o) {
                out.push(violation(
                    dom,
                    *child,
                    format!(
                        "`{}` 里新写的 `{}` 不在 PROP-05 顺序位置（前面最大序号 {:?}，后面最小序号 {:?}，自身 {o}）",
                        table.name,
                        dom.name(*child).map(|q| q.display(interner).to_string()).unwrap_or_default(),
                        before_max,
                        after_min
                    ),
                ));
            }
        }
    }
    out
}

/// 需要出现在根 `mc:Ignorable` 里的扩展命名空间（`SAVE-03`）。
fn needs_ignorable(ns: NsId) -> bool {
    ns.canonical_prefix().is_some_and(|p| {
        p.starts_with("w14") || p.starts_with("w15") || p.starts_with("w16") || p == "wp14"
    })
}

/// `New` / `SelfDirty` 节点用到的扩展命名空间（去重）。
fn extension_namespaces_in_dirty(dom: &Dom) -> Vec<NsId> {
    let mut out: Vec<NsId> = Vec::new();
    for node in dom.descendants(dom.root()) {
        let n = dom.node(node);
        if !is_dirty_self(n.dirty) {
            continue;
        }
        let Some(e) = dom.element(node) else { continue };
        for ns in std::iter::once(e.name.ns).chain(e.attrs.iter().map(|a| a.name.ns)) {
            if needs_ignorable(ns) && !out.contains(&ns) {
                out.push(ns);
            }
        }
    }
    out
}

/// `XML-14` / `SAVE-03`：把 `New` / `SelfDirty` 节点用到的扩展命名空间声明到 part 根，并补进
/// `mc:Ignorable`（缺 `mc` 声明时一并补）。返回是否改动了根。
pub fn ensure_extension_declarations(dom: &mut Dom) -> bool {
    let used = extension_namespaces_in_dirty(dom);
    if used.is_empty() {
        return false;
    }
    let root = dom.root();
    let mut changed = false;
    let mut prefixes: Vec<String> = Vec::new();
    for ns in used {
        let scope = dom.namespace_scope(root);
        let prefix = match scope.prefix_for(ns) {
            Some(Some(p)) => dom.interner().resolve(p).to_string(),
            _ => {
                let p = dom.pick_prefix(ns, &scope, &[]).expect("known namespace gets a prefix");
                dom.add_declarations(root, &[PrefixUse { prefix: Some(p), ns }]);
                changed = true;
                dom.interner().resolve(p).to_string()
            }
        };
        prefixes.push(prefix);
    }
    // mc:Ignorable
    let ignorable = QName::new(NsId::Mc, LocalName::Ignorable);
    let current = dom.attr_value(root, ignorable).map(|s| s.into_owned()).unwrap_or_default();
    let mut listed: Vec<String> = current.split_whitespace().map(str::to_string).collect();
    let mut grew = false;
    for p in prefixes {
        if !listed.contains(&p) {
            listed.push(p);
            grew = true;
        }
    }
    if grew {
        if dom.namespace_scope(root).prefix_for(NsId::Mc).is_none() {
            let mc = dom.interner_mut().intern("mc");
            dom.add_declarations(root, &[PrefixUse { prefix: Some(mc), ns: NsId::Mc }]);
        }
        dom.set_attr(root, ignorable, listed.join(" "));
        changed = true;
    }
    changed
}

/// `SAVE-02` 的失败处理：调试构建与 CI 下任一 `EngineInvariantViolation` → `Err(SAVE_INVARIANT)`；
/// 发布构建返回 `Ok`，由调用方记诊断。
pub fn enforce(diags: &[Diagnostic]) -> Result<()> {
    if cfg!(debug_assertions)
        && let Some(d) =
            diags.iter().find(|d| d.origin == ValidationOrigin::EngineInvariantViolation)
    {
        return Err(Error::Invariant(d.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PartFlavor, PartId};
    use crate::save::serialize;
    use crate::semantic::props::{Change, RunPropsPatch, plan_apply_run_props};

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
    const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap()
    }

    fn first(dom: &Dom, local: LocalName) -> NodeId {
        dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(local))).unwrap()
    }

    #[test]
    fn save_02_clean_part_has_no_diagnostics_and_plan_apply_keeps_order() {
        let mut dom = parse(&format!(
            r#"<w:r xmlns:w="{W}"><w:rPr><w:sz w:val="24"/></w:rPr><w:t>x</w:t></w:r>"#
        ));
        assert!(validate_part(&dom).is_empty());
        let r = dom.root();
        let rpr = first(&dom, LocalName::RPr);
        let patch = RunPropsPatch { bold: Change::Set(true), ..Default::default() };
        let edits = plan_apply_run_props(&dom, r, Some(rpr), &patch, PartFlavor::Transitional);
        dom.apply_edits(&edits);
        assert!(validate_part(&dom).is_empty(), "plan_apply 的插入满足 PROP-05");
        assert!(enforce(&validate_part(&dom)).is_ok());
    }

    #[test]
    fn save_02_misordered_new_child_is_an_engine_violation() {
        let mut dom =
            parse(&format!(r#"<w:r xmlns:w="{W}"><w:rPr><w:sz w:val="24"/></w:rPr></w:r>"#));
        let rpr = first(&dom, LocalName::RPr);
        let b = dom.new_element(QName::w(LocalName::B));
        dom.append_child(rpr, b); // w:b 排在 w:sz 之后：违反 PROP-05
        let diags = validate_part(&dom);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagCode::SaveInvariant);
        assert_eq!(diags[0].origin, ValidationOrigin::EngineInvariantViolation);
        assert!(
            diags[0].message.contains("RunProps") && diags[0].message.contains("w:b"),
            "{}",
            diags[0].message
        );
        // `SAVE-02`：调试构建 / CI 报错，发布构建只记诊断
        if cfg!(debug_assertions) {
            assert!(matches!(enforce(&diags), Err(Error::Invariant(_))));
        } else {
            assert!(enforce(&diags).is_ok());
        }
        // 同样的乱序若是输入本来如此（Clean），不报
        let clean =
            parse(&format!(r#"<w:r xmlns:w="{W}"><w:rPr><w:sz w:val="24"/><w:b/></w:rPr></w:r>"#));
        assert!(validate_part(&clean).is_empty());
        // 未建模但有序号的 w:bdr 也参与排序判断
        let mut dom =
            parse(&format!(r#"<w:r xmlns:w="{W}"><w:rPr><w:bdr w:val="single"/></w:rPr></w:r>"#));
        let rpr = first(&dom, LocalName::RPr);
        let i = dom.new_element(QName::w(LocalName::I));
        dom.append_child(rpr, i);
        assert_eq!(validate_part(&dom).len(), 1);
    }

    #[test]
    fn save_02_unbound_prefix_on_new_node_is_reported() {
        let mut dom = parse(&format!(r#"<w:r xmlns:w="{W}"><w:t>x</w:t></w:r>"#));
        let zz = dom.interner_mut().intern("zz");
        let bad = dom.new_element(QName::new(NsId::Unbound(zz), LocalName::B));
        let root = dom.root();
        dom.append_child(root, bad);
        let diags = validate_part(&dom);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("zz:b"), "{}", diags[0].message);
    }

    #[test]
    fn save_03_extension_namespace_declared_at_root_and_listed_in_ignorable() {
        // 根已有 mc 与 w15，新写 w14 元素 → 根加 xmlns:w14 并把 w14 追加进 Ignorable
        let mut dom = parse(&format!(
            r#"<w:document xmlns:w="{W}" xmlns:mc="{MC}" mc:Ignorable="w15"><w:body><w:p><w:r><w:rPr/></w:r></w:p></w:body></w:document>"#
        ));
        let rpr = first(&dom, LocalName::RPr);
        let glow = dom.new_element(QName::new(NsId::W14, LocalName::Glow));
        dom.append_child(rpr, glow);
        assert!(ensure_extension_declarations(&mut dom));
        let root = dom.root();
        assert_eq!(
            dom.attr_value(root, QName::new(NsId::Mc, LocalName::Ignorable)).as_deref(),
            Some("w15 w14")
        );
        assert_eq!(dom.node(root).dirty, Dirty::SelfDirty);
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(out.contains(&format!(r#"xmlns:w14="{W14}""#)), "{out}");
        assert!(out.contains(r#"mc:Ignorable="w15 w14""#), "{out}");
        assert!(out.contains("<w14:glow/>"), "{out}");
        // 幂等
        assert!(!ensure_extension_declarations(&mut dom));
        // 重新解析：w14 元素被理解、Ignorable 生效
        let again = Dom::parse(PartId(0), out.as_bytes()).unwrap();
        assert!(
            again
                .descendants(again.root())
                .any(|n| again.is(n, QName::new(NsId::W14, LocalName::Glow)))
        );

        // 根没有 mc：一并声明 mc
        let mut dom = parse(&format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr/></w:r></w:p></w:body></w:document>"#
        ));
        let rpr = first(&dom, LocalName::RPr);
        let glow = dom.new_element(QName::new(NsId::W14, LocalName::Glow));
        dom.append_child(rpr, glow);
        assert!(ensure_extension_declarations(&mut dom));
        let out = String::from_utf8(serialize(&dom).unwrap()).unwrap();
        assert!(
            out.contains(&format!(r#"xmlns:mc="{MC}""#)) && out.contains(r#"mc:Ignorable="w14""#),
            "{out}"
        );
        assert!(validate_part(&dom).is_empty());
        // 没有扩展命名空间的改动：不碰根
        let mut plain =
            parse(&format!(r#"<w:document xmlns:w="{W}"><w:body><w:p/></w:body></w:document>"#));
        let p = first(&plain, LocalName::P);
        let r = plain.new_element(QName::w(LocalName::R));
        plain.append_child(p, r);
        assert!(!ensure_extension_declarations(&mut plain));
        assert_eq!(plain.node(plain.root()).dirty, Dirty::DescendantDirty);
    }
}
