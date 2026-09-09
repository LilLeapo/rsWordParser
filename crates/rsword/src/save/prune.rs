//! 资源回收（`SAVE-07` `prune_orphans`，`spec/17` 任务 6.7）：保存前删掉**本次会话**让引用数归零的关系，
//! 以及因此再没人引用的 part 子图。
//!
//! 规则（TS `cleanupDocxOwnedResources` 的语义，多一条「只动本次会话造成的」）：
//! - 只看写过的内容 part（`EditSession::rel_baseline` 有基线的那些）里 TS `DOCUMENT_OWNED_REL_TYPES` 那几种关系
//!   （image / chart / diagram × 4 / hyperlink / oleObject）；
//! - 一条关系被回收，须是**现在**没人引用、且（写之前有人引用，或是本次会话新加的）——原本就是孤儿的关系不动，
//!   文件是真相；
//! - 被回收关系的内部目标若在受管目录（`word/media` / `charts` / `embeddings` / `diagrams`）下且整个包里再没有别的
//!   关系指向它，就删掉它和它的 `.rels`，并沿它自己的关系递归（图表 → 工作簿；图示 → 五个 part）；别处仍引用的留着；
//! - `[Content_Types].xml` 里对应的 `Override` 一起删，`Default` 不动。

use std::collections::{HashMap, HashSet};

use crate::diag::DiagCode;
use crate::edit::EditSession;
use crate::edit::plan::MutationPlan;
use crate::error::{Error, Result};
use crate::package::{PartId, RelTarget, RelType};
use crate::xml::plan::NodeEdit;
use crate::xml::{Dirty, Dom, LocalName, NsId};

/// 一个 part 第一次被写之前的引用状态。
#[derive(Debug, Clone, Default)]
pub struct RelBaseline {
    /// 正文里被引用的 `rId`。
    pub referenced: HashSet<String>,
    /// 当时 `.rels` 里有的关系 id。
    pub rel_ids: HashSet<String>,
}

/// TS `DOCUMENT_OWNED_REL_TYPES`：引用直接写在文档 XML 里、可以由本引擎管生命周期的关系类型。
const OWNED_REL_TYPES: &[RelType] = &[
    RelType::Image,
    RelType::Chart,
    RelType::ChartEx,
    RelType::DiagramData,
    RelType::DiagramLayout,
    RelType::DiagramQuickStyle,
    RelType::DiagramColors,
    RelType::DiagramDrawing,
    RelType::Hyperlink,
    RelType::OleObject,
];

/// 受管目录（TS `isOwnedPart`）：只有这些目录下的 part 会被删。
fn is_owned_part(uri: &str) -> bool {
    uri.starts_with("word/media/")
        || uri.starts_with("word/charts/")
        || uri.starts_with("word/embeddings/")
        || uri.starts_with("word/diagrams/")
}

/// 一个 part 的 DOM 里被引用的关系 id：`r:` 命名空间的全部属性（`r:embed` / `r:link` / `r:id` / `r:dm` / …），
/// 加上任何值形如 `rId…` 的属性（TS 的保守兜底：老 VML 会把关系 id 写在 `o:relid` 一类别的命名空间里）。
///
/// 跳过 `Deleted` 子树（删掉的段落不再算引用），但**不**跳过不活跃的 `mc:Fallback`——它还在文件里，老 Word 会用。
pub fn referenced_rids(dom: &Dom) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut stack = vec![dom.root()];
    while let Some(n) = stack.pop() {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        for a in &e.attrs {
            let v = dom.attr_str(a);
            let v = v.trim();
            if (a.name.ns == NsId::R || v.starts_with("rId")) && !v.is_empty() {
                out.insert(v.to_string());
            }
        }
        stack.extend(e.children.iter().rev());
    }
    out
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// 回收本次会话让引用数归零的资源。返回删掉的 part 数。
    pub(crate) fn prune_orphans(&mut self) -> Result<usize> {
        // 1. 每个写过的内容 part：哪些受管关系现在没人引用、而且是本次会话造成的
        let parts: Vec<PartId> = self.rel_baseline.keys().copied().collect();
        let mut roots: Vec<PartId> = Vec::new();
        for part in parts {
            if self.package().part(part).deleted {
                continue;
            }
            let Some(dom) = self.package().part(part).dom() else { continue };
            let now = referenced_rids(dom);
            let base = self.rel_baseline[&part].clone();
            let stale: Vec<(String, Option<PartId>)> = self
                .package()
                .part(part)
                .rels
                .iter()
                .filter(|r| OWNED_REL_TYPES.contains(&r.kind))
                .filter(|r| !now.contains(&r.id))
                .filter(|r| base.referenced.contains(&r.id) || !base.rel_ids.contains(&r.id))
                .map(|r| {
                    let target = match &r.target {
                        RelTarget::Internal(u) => self.package().find(u),
                        RelTarget::External(_) => None,
                    };
                    (r.id.clone(), target)
                })
                .collect();
            for (id, target) in stale {
                self.remove_relationship(part, &id)?;
                if let Some(t) = target
                    && is_owned_part(self.package().part(t).uri.as_str())
                {
                    roots.push(t);
                }
            }
        }
        if roots.is_empty() {
            return Ok(0);
        }
        // 2. 候选子图：从根出发沿关系走到的受管 part
        let outgoing = self.outgoing_edges();
        let mut candidates: HashSet<PartId> = HashSet::new();
        let mut pending = roots;
        while let Some(p) = pending.pop() {
            if !candidates.insert(p) {
                continue;
            }
            for &t in outgoing.get(&p).map(Vec::as_slice).unwrap_or(&[]) {
                if is_owned_part(self.package().part(t).uri.as_str()) {
                    pending.push(t);
                }
            }
        }
        // 3. 候选之外仍有关系指进来的目标是共享的：留下它和它能走到的一切
        let mut shared: Vec<PartId> = Vec::new();
        for (&src, targets) in &outgoing {
            if candidates.contains(&src) {
                continue;
            }
            shared.extend(targets.iter().copied().filter(|t| candidates.contains(t)));
        }
        let mut retained: HashSet<PartId> = HashSet::new();
        while let Some(p) = shared.pop() {
            if !candidates.contains(&p) || !retained.insert(p) {
                continue;
            }
            shared.extend(outgoing.get(&p).map(Vec::as_slice).unwrap_or(&[]).iter().copied());
        }
        // 4. 删 part（连它的 `.rels`）与 Override
        let mut removed = 0usize;
        let mut doomed: Vec<PartId> = candidates.difference(&retained).copied().collect();
        doomed.sort_by_key(|p| p.0);
        for p in doomed {
            let rels_part = self.package().part(p).rels_part;
            self.remove_content_type_override(p)?;
            self.package_mut().remove_part(p);
            if let Some(r) = rels_part {
                self.package_mut().remove_part(r);
            }
            removed += 1;
        }
        if removed > 0 {
            self.rebuild()?;
        }
        Ok(removed)
    }

    /// 全部未删 part 的内部关系边（源 part → 目标 part）。
    fn outgoing_edges(&self) -> HashMap<PartId, Vec<PartId>> {
        let pkg = self.package();
        let mut out: HashMap<PartId, Vec<PartId>> = HashMap::new();
        for p in pkg.parts().iter().filter(|p| !p.deleted) {
            for r in p.rels.iter() {
                if let RelTarget::Internal(u) = &r.target
                    && let Some(t) = pkg.find(u)
                {
                    out.entry(p.id).or_default().push(t);
                }
            }
        }
        out
    }

    /// 删掉 `part` 的一条关系：`.rels` DOM 里的节点 `Deleted`（走 `commit_plan`，在事务里），内存视图同步。
    fn remove_relationship(&mut self, part: PartId, id: &str) -> Result<()> {
        let Some(rels_part) = self.package().part(part).rels_part else { return Ok(()) };
        let Some(rel) = self.package().part(part).rels.by_id(id) else { return Ok(()) };
        let node = rel.node;
        let mut plan = MutationPlan::new(rels_part);
        plan.node_edits.push(NodeEdit::Delete(node));
        self.commit_plan(plan)?;
        self.package_mut().part_mut(part).rels.remove(id);
        Ok(())
    }

    /// `[Content_Types].xml` 里 `PartName="/<uri>"` 的 `Override` 删掉（没有就算了：靠 `Default` 声明的 part）。
    fn remove_content_type_override(&mut self, part: PartId) -> Result<()> {
        let Some(ct_part) = self.package().content_types_part() else { return Ok(()) };
        let uri = self.package().part(part).uri.clone();
        let want = format!("/{}", uri.as_str());
        let dom = self
            .package()
            .part(ct_part)
            .dom()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "内容类型不是 XML part"))?;
        let root = dom.root();
        let hit = dom.children(root).iter().copied().find(|&c| {
            dom.name(c).is_some_and(|q| q.local == LocalName::Override)
                && dom
                    .attr_value(c, crate::xml::QName::new(NsId::None, LocalName::PartName))
                    .is_some_and(|v| v == want)
        });
        if let Some(node) = hit {
            let mut plan = MutationPlan::new(ct_part);
            plan.node_edits.push(NodeEdit::Delete(node));
            self.commit_plan(plan)?;
            self.package_mut().content_types_mut().remove_override(&uri);
        }
        Ok(())
    }
}
