//! `mc:AlternateContent` 的孪生同步（`spec/18` 7.7）。
//!
//! Word 写文本框与形状时发两份：`mc:Choice Requires="wps"` 里的 DrawingML（新版读这份）与
//! `mc:Fallback` 里的 VML（老版读那份）。两份是同一个对象的两种写法，内容必须一致。
//!
//! 于是编辑落在 Choice 的 `w:txbxContent` 里之后，把 `mc:Fallback` 的**同序** `w:txbxContent`
//! 内容整体换成 Choice 那份的深克隆（旧的 `Deleted`、新的 `New`，`XML-12` 规则 F）。位置直接
//! 落在 `mc:Fallback` 里 → `EDIT_TARGET_FALLBACK`：改那一份下一次同步就被覆盖，没有意义。
//!
//! 孪生只在**内容**上同步。几何与样式（`SetDrawingGeometry` / `SetShapeStyle`）改的是 Choice 的
//! `a:xfrm` / `wps:spPr`，VML 那边对应的是 `v:shape/@style @fillcolor @strokecolor`，
//! 由 [`sync_shape_style`] 单独翻。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::package::PartId;
use crate::xml::{Dirty, Dom, LocalName, NodeEdit, NodeId, NsId, QName, Target};

use super::plan::{MutationPlan, MutationResult};
use super::session::EditSession;

fn mc(local: LocalName) -> QName {
    QName::new(NsId::Mc, local)
}

fn txbx() -> QName {
    QName::new(NsId::W, LocalName::TxbxContent)
}

fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node(n).dirty != Dirty::Deleted
}

/// 一处待同步的孪生：`mc:AlternateContent` 与它下面第 `idx` 个 `w:txbxContent`
/// （一个 `mc:AlternateContent` 里可以有好几个文本框，靠序号配对）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TwinSite {
    pub part: Option<PartId>,
    pub ac: NodeId,
    pub idx: usize,
}

/// 某个分支下的全部 `w:txbxContent`（文档序）。
fn txbx_contents(dom: &Dom, branch: NodeId) -> Vec<NodeId> {
    dom.descendants(branch).filter(|&n| live(dom, n) && dom.is(n, txbx())).collect()
}

/// `mc:AlternateContent` 下第一个活的 `mc:Choice` / `mc:Fallback`。
fn branch(dom: &Dom, ac: NodeId, which: LocalName) -> Option<NodeId> {
    dom.children(ac).iter().copied().find(|&c| live(dom, c) && dom.is(c, mc(which)))
}

/// 编辑目标落在哪些孪生里。落在 `mc:Fallback` 中 → `Err(EDIT_TARGET_FALLBACK)`。
///
/// 一个目标可能牵动**好几处**孪生：套在两层文本框里的段落，里外两层的 Choice 内容都变了；
/// 整体替换（`SetTextboxContent`）的目标又在 `w:txbxContent` 之上。所以既往上收祖先、
/// 也往下收子树里的框，由内到外排——先同步里层，外层再克隆时拿到的就是同步过的里层。
pub(crate) fn sites(
    s: &EditSession,
    targets: &[(Option<PartId>, NodeId)],
) -> Result<Vec<TwinSite>> {
    let mut out: Vec<TwinSite> = Vec::new();
    for &(part, node) in targets {
        let dom = s.dom_in(part)?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            continue;
        }
        if dom.ancestors(node).any(|a| dom.is(a, mc(LocalName::Fallback))) {
            return Err(Error::edit(
                DiagCode::EditTargetFallback,
                "编辑位置在 mc:Fallback 里；那是 mc:Choice 的 VML 孪生，改 Choice 那一份",
            ));
        }
        // 子树里的框（最深的在前）＋ 自己和祖先里的框（由内到外）
        let mut inside: Vec<NodeId> = txbx_contents(dom, node);
        inside.reverse();
        inside.extend(
            std::iter::once(node).chain(dom.ancestors(node)).filter(|&a| dom.is(a, txbx())),
        );
        for t in inside {
            let Some(site) = site_of(dom, part, t) else { continue };
            if !out.contains(&site) {
                out.push(site);
            }
        }
    }
    Ok(out)
}

/// 一个 `w:txbxContent` 属于哪个 `mc:AlternateContent` 的 Choice 分支、排第几。
/// 不在 `mc:Choice` 里（普通文本框、Fallback 已被上面拦下）→ `None`。
fn site_of(dom: &Dom, part: Option<PartId>, txbx: NodeId) -> Option<TwinSite> {
    let choice = dom.ancestors(txbx).find(|&a| dom.is(a, mc(LocalName::Choice)))?;
    let ac = dom.parent(choice).filter(|&p| dom.is(p, mc(LocalName::AlternateContent)))?;
    let idx = txbx_contents(dom, choice).iter().position(|&x| x == txbx)?;
    Some(TwinSite { part, ac, idx })
}

/// 提交之后：每处孪生的 `mc:Fallback` 内容换成 Choice 那份的深克隆。
pub(crate) fn sync(s: &mut EditSession, sites: &[TwinSite]) -> Result<MutationResult> {
    let mut out = MutationResult::default();
    for site in sites {
        let part = site.part.unwrap_or_else(|| s.main_part());
        let dom = s.dom_in(Some(part))?;
        if !live(dom, site.ac) {
            continue;
        }
        let (Some(choice), Some(fallback)) =
            (branch(dom, site.ac, LocalName::Choice), branch(dom, site.ac, LocalName::Fallback))
        else {
            continue;
        };
        let (from, to) = (txbx_contents(dom, choice), txbx_contents(dom, fallback));
        let (Some(&from), Some(&to)) = (from.get(site.idx), to.get(site.idx)) else {
            continue;
        };
        let mut plan = MutationPlan::new(part);
        touch_block(dom, &mut plan, site.ac);
        for c in dom.children(to).iter().copied().filter(|&c| live(dom, c)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
        for c in dom.children(from).iter().copied().filter(|&c| live(dom, c)) {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::Node(to),
                before: None,
                source: c,
            });
        }
        if !plan.is_empty() {
            out.absorb(s.commit_plan(plan)?);
        }
    }
    Ok(out)
}

/// 几何与样式改完之后：把 `mc:Fallback` 里的 VML 形状按 Choice 现在的值改
/// `@style` 的尺寸与位置、`@fillcolor` / `@filled`、`@strokecolor` / `@stroked`。
///
/// 只动这几个键：`@style` 里别的键（`z-index`、`mso-*`、`position`）原样留着——那是 VML 自己的
/// 排版参数，DrawingML 这边没有对应物，猜着改不如不动。
pub(crate) fn sync_shape_style(
    s: &mut EditSession,
    targets: &[(Option<PartId>, NodeId)],
) -> Result<MutationResult> {
    let mut out = MutationResult::default();
    for &(part, node) in targets {
        let p = part.unwrap_or_else(|| s.main_part());
        let dom = s.dom_in(Some(p))?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            continue;
        }
        let Some(ac) = dom.ancestors(node).find(|&a| dom.is(a, mc(LocalName::AlternateContent)))
        else {
            continue;
        };
        let (Some(choice), Some(fallback)) =
            (branch(dom, ac, LocalName::Choice), branch(dom, ac, LocalName::Fallback))
        else {
            continue;
        };
        let Some(shape) = vml_shape(dom, fallback) else { continue };
        let mut plan = MutationPlan::new(p);
        touch_block(dom, &mut plan, ac);
        for (name, value) in vml_attrs(dom, choice, shape) {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(shape),
                name: QName::new(NsId::None, name),
                value,
            });
        }
        if !plan.is_empty() {
            out.absorb(s.commit_plan(plan)?);
        }
    }
    Ok(out)
}

/// 同步是**另一个** plan：投影得跟着刷新，不然宿主段落的投影停在同步之前
/// （`TEST-07` 的随机序列在「插分节符 + 换绕排」两步上抓到过）。
fn touch_block(dom: &Dom, plan: &mut MutationPlan, node: NodeId) {
    let is_block = |n: NodeId| {
        dom.is(n, QName::new(NsId::W, LocalName::P))
            || dom.is(n, QName::new(NsId::W, LocalName::Tbl))
    };
    if let Some(b) = std::iter::once(node).chain(dom.ancestors(node)).find(|&n| is_block(n)) {
        plan.touch(b);
    }
}

/// `mc:Fallback` 里第一个会画东西的 VML 形状。
fn vml_shape(dom: &Dom, fallback: NodeId) -> Option<NodeId> {
    dom.descendants(fallback)
        .find(|&n| live(dom, n) && dom.is_ns(n, NsId::V, "v") && crate::model::drawn_shape(dom, n))
}

/// EMU / pt（VML 的 `@style` 用 pt）。
const EMU_PER_PT: f64 = 12700.0;

/// 从 Choice 现在的 `wp:extent` / `wp:posOffset` / `wps:spPr` 算出 VML 形状该有的属性。
fn vml_attrs(dom: &Dom, choice: NodeId, shape: NodeId) -> Vec<(LocalName, String)> {
    let wp = |l: LocalName| QName::new(NsId::Wp, l);
    let a = |l: LocalName| QName::new(NsId::A, l);
    let find =
        |root: NodeId, q: QName| dom.descendants(root).find(|&n| live(dom, n) && dom.is(n, q));
    let num = |n: NodeId, l: LocalName| -> Option<i64> {
        dom.attr_value(n, QName::new(NsId::None, l))?.trim().parse().ok()
    };
    let pt = |emu: i64| format!("{:.2}pt", emu as f64 / EMU_PER_PT);
    let mut style: Vec<(String, String)> = dom
        .attr_value(shape, QName::new(NsId::None, LocalName::Style))
        .map(|v| {
            v.split(';')
                .filter_map(|kv| kv.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut set = |k: &str, v: String| match style.iter_mut().find(|(x, _)| x == k) {
        Some(slot) => slot.1 = v,
        None => style.push((k.to_string(), v)),
    };
    if let Some(ext) = find(choice, wp(LocalName::Extent)) {
        if let Some(cx) = num(ext, LocalName::Cx) {
            set("width", pt(cx));
        }
        if let Some(cy) = num(ext, LocalName::Cy) {
            set("height", pt(cy));
        }
    }
    for (which, key) in
        [(LocalName::PositionH, "margin-left"), (LocalName::PositionV, "margin-top")]
    {
        let Some(pos) = find(choice, wp(which)) else { continue };
        let Some(off) = find(pos, wp(LocalName::PosOffset)) else { continue };
        let Some(v) = crate::model::text_of(dom, off).and_then(|t| t.trim().parse::<i64>().ok())
        else {
            continue;
        };
        set(key, pt(v));
    }
    let mut out = vec![(
        LocalName::Style,
        style.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(";"),
    )];
    // 填充与描边：`wps:spPr` 直属的 `a:solidFill` / `a:noFill`，以及 `a:ln` 里的那一对
    let sp_pr = find(choice, QName::new(NsId::Wps, LocalName::SpPr));
    let color = |container: Option<NodeId>| -> Option<Option<String>> {
        let c = container?;
        let direct =
            |q: QName| dom.children(c).iter().copied().find(|&x| live(dom, x) && dom.is(x, q));
        if direct(a(LocalName::NoFill)).is_some() {
            return Some(None);
        }
        let fill = direct(a(LocalName::SolidFill))?;
        let clr = find(fill, a(LocalName::SrgbClr))?;
        Some(dom.attr_value(clr, QName::new(NsId::None, LocalName::Val)).map(|v| v.into_owned()))
    };
    if let Some(f) = color(sp_pr) {
        out.push((LocalName::Filled, if f.is_some() { "t".into() } else { "f".into() }));
        if let Some(c) = f {
            out.push((LocalName::Fillcolor, format!("#{c}")));
        }
    }
    let ln = sp_pr.and_then(|n| {
        dom.children(n).iter().copied().find(|&x| live(dom, x) && dom.is(x, a(LocalName::Ln)))
    });
    if let Some(l) = color(ln) {
        out.push((LocalName::Stroked, if l.is_some() { "t".into() } else { "f".into() }));
        if let Some(c) = l {
            out.push((LocalName::Strokecolor, format!("#{c}")));
        }
    }
    out
}
