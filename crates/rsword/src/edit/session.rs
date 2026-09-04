//! `EDIT-01` 会话：`Package`（规范状态）+ `Document`（投影）+ 事务（`EDIT-05`）。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::Document;
use crate::model::block::TextBlock;
use crate::package::{Package, PartFlavor, PartId};
use crate::save::SaveOptions;
use crate::xml::{Dom, NodeId};

use super::plan::{MutationPlan, MutationResult};
use super::pos::{InlinePos, Loc, locate};
use super::{EditContext, EditOp, ops};

/// 编辑会话。规范状态是包里各 part 的 DOM；`document()` 是可重建的投影。
pub struct EditSession {
    pkg: Package,
    doc: Document,
    diagnostics: Vec<Diagnostic>,
}

/// 事务快照：M1 的编辑操作只碰主 part，快照即主 part DOM 的克隆（投影回滚时整体重建）。
/// 保存选项不经快照——它们全部先 `validate` 再 `commit`，`commit` 不可失败。
pub(crate) struct Snapshot {
    dom: Dom,
}

impl EditSession {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::open(bytes)?)
    }

    pub fn from_package(mut pkg: Package) -> Result<Self> {
        let doc = Document::rebuild(&mut pkg)?;
        Ok(Self { pkg, doc, diagnostics: Vec::new() })
    }

    /// 投影（`MOD-01`）。
    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// 直接改包（测试与工具用）；之后应调用 [`EditSession::rebuild`]。
    pub fn package_mut(&mut self) -> &mut Package {
        &mut self.pkg
    }

    pub fn main_part(&self) -> PartId {
        self.pkg.main_part()
    }

    /// 主 part 的 DOM。
    pub fn dom(&self) -> &Dom {
        self.pkg.part(self.pkg.main_part()).dom().expect("main part is parsed")
    }

    pub fn flavor(&self) -> PartFlavor {
        self.pkg.flavor_of(self.pkg.main_part())
    }

    /// 编辑阶段累计的诊断（不含包 / 保存阶段的）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// 正文顶层的文本段落块。
    pub fn text_block(&self, para: NodeId) -> Option<&TextBlock> {
        self.doc.main.iter().find(|b| b.node() == para).and_then(|b| b.as_text())
    }

    /// 正文第 `i` 个文本段落（测试便利）。
    pub fn nth_text_block(&self, i: usize) -> Option<&TextBlock> {
        self.doc.text_blocks().nth(i)
    }

    /// `EDIT-02`：定位。
    pub fn locate(&self, pos: InlinePos) -> Result<Loc> {
        let tb = self
            .text_block(pos.para)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "不是正文文本段落"))?;
        locate(tb, pos.offset)
    }

    /// 应用一个操作：失败时会话状态（DOM 与投影）与操作前一致。
    pub fn apply(&mut self, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
        let snap = self.snapshot();
        match ops::run(self, op, ctx) {
            Ok(r) => Ok(r),
            Err(e) => {
                self.restore(snap)?;
                Err(e)
            }
        }
    }

    /// 批量应用：任一失败则整批不生效。
    pub fn apply_all(
        &mut self,
        ops: Vec<EditOp>,
        ctx: &EditContext,
    ) -> Result<Vec<MutationResult>> {
        let snap = self.snapshot();
        let mut results = Vec::with_capacity(ops.len());
        for op in ops {
            match ops::run(self, op, ctx) {
                Ok(r) => results.push(r),
                Err(e) => {
                    self.restore(snap)?;
                    return Err(e);
                }
            }
        }
        Ok(results)
    }

    /// `SAVE-01`：等价于 `save_with(&SaveOptions::default())`。
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.save_with(&SaveOptions::default())
    }

    /// `SAVE-01` 全流程：
    ///
    /// 1. 无脏节点且 `opts` 没有变更请求（`saved_at` 单独设置不算，与 TS `isUnchanged` 一致）
    ///    且文档没有 `w:removePersonalInformation` 标志 → 返回原字节（不变式 1）。
    /// 2. 校验（`SAVE-02`，在 [`Package::save`] 里）。
    /// 3. 物化 Span（`SPAN-08`）：Span 索引在 M2 建立，M1 无操作。
    /// 4. 应用保存选项（`SAVE-07`）：全部先 `validate`（只读）再逐个 `commit`，所以要么全做要么不动。
    /// 5. / 6. 序列化脏 part 并写回（`XML-13` / `SAVE-06`，在 [`Package::save`] 里）。
    pub fn save_with(&mut self, opts: &SaveOptions) -> Result<Vec<u8>> {
        let scrub = opts.remove_personal_info.unwrap_or_else(|| self.remove_personal_info_flag());
        if !self.pkg.is_dirty() && !opts.forces_save() && !scrub {
            return Ok(self.pkg.original_bytes().to_vec());
        }
        // 步骤 3：SPAN-08 物化在 M2（此处无操作，Span 索引尚未建立）。
        let (plans, diags) = crate::save::options::plan_all(&mut self.pkg, opts, scrub)?;
        for plan in &plans {
            let dom = self.pkg.part(plan.part).dom().ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, "保存选项的目标不是 XML part")
            })?;
            plan.validate(dom)?;
        }
        let touches_main = plans.iter().any(|p| p.part == self.pkg.main_part());
        for plan in plans {
            self.commit_plan(plan)?;
        }
        self.diagnostics.extend(diags.iter().cloned());
        self.pkg.push_diagnostics(diags);
        if touches_main {
            self.rebuild()?;
        }
        self.pkg.save()
    }

    /// 文档自带的 `w:removePersonalInformation`（`SAVE-07`：设置或文档标志为真时清洗）。
    pub fn remove_personal_info_flag(&self) -> bool {
        self.doc.settings.as_ref().and_then(|s| s.remove_personal_information) == Some(true)
    }

    /// 投影整体重建。
    pub fn rebuild(&mut self) -> Result<()> {
        self.doc = Document::rebuild(&mut self.pkg)?;
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot { dom: self.dom().clone() }
    }

    pub(crate) fn restore(&mut self, snap: Snapshot) -> Result<()> {
        let main = self.pkg.main_part();
        let dom = self.pkg.dom_mut(main)?.expect("main part is parsed");
        *dom = snap.dom;
        self.rebuild()
    }

    /// 一个阶段：`validate` → `commit` → 刷新投影 → 记诊断。编辑操作只碰主 part；保存选项
    /// （`SAVE-07`）也走这里，可以指向任意 XML part（投影只在主 part 上刷新）。
    pub(crate) fn commit_plan(&mut self, plan: MutationPlan) -> Result<MutationResult> {
        let main = self.pkg.main_part();
        let part = plan.part;
        let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
            Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
        })?;
        plan.validate(dom)?;
        let result = plan.commit(dom);
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if part == main {
            if result.structure_changed {
                self.rebuild()?;
            } else if !result.affected_paragraphs.is_empty() {
                // 不在正文顶层的段落（表格内等）M1 不投影，忽略返回的缺失列表
                let _ = self.doc.refresh_paragraphs(&mut self.pkg, &result.affected_paragraphs)?;
            }
        }
        Ok(result)
    }
}
