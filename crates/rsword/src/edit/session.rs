//! `EDIT-01` 会话：`Package`（规范状态）+ `Document`（投影）+ 事务（`EDIT-05`）。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::Document;
use crate::model::block::TextBlock;
use crate::package::{Package, PartFlavor, PartId};
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

/// 事务快照：M1 的操作只碰主 part，快照即主 part DOM 的克隆（投影回滚时整体重建）。
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

    /// 保存（`SAVE-01` 的 M1 版本：校验 → 序列化脏 part → 写回；无脏节点返回原字节）。
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.pkg.save()
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

    /// 一个阶段：`validate` → `commit` → 刷新投影 → 记诊断。
    pub(crate) fn commit_plan(&mut self, plan: MutationPlan) -> Result<MutationResult> {
        let main = self.pkg.main_part();
        if plan.part != main {
            return Err(Error::edit(DiagCode::EditUnsupported, "M1 只编辑主 part"));
        }
        let dom = self.pkg.dom_mut(main)?.expect("main part is parsed");
        plan.validate(dom)?;
        let result = plan.commit(dom);
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if result.structure_changed {
            self.rebuild()?;
        } else if !result.affected_paragraphs.is_empty() {
            // 不在正文顶层的段落（表格内等）M1 不投影，忽略返回的缺失列表
            let _ = self.doc.refresh_paragraphs(&mut self.pkg, &result.affected_paragraphs)?;
        }
        Ok(result)
    }
}
