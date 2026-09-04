//! L4 编辑引擎（`spec/08-edit.md`，`docs/03` §8）。
//!
//! `EditOp → plan（只读）→ validate（只读）→ commit（机械写入，不可失败）→ rebuild`。
//! 任何一步 `Err` 都不留下半修改状态（`EDIT-05`）。偏移单位对外统一为 UTF-16 code unit，
//! 原子为一个 `U+FFFC`（`EDIT-02`）。
//!
//! 本提交完成任务 1.11 的框架：会话、上下文、位置解析与 `MutationPlan` 事务管道；
//! `EditOp` 只声明 M1 的首批变体，具体 planner 在任务 1.12 实现。

pub mod locate;
pub mod plan;

pub use locate::{BlockPos, InlinePos, Loc, Utf16Offset};
pub use plan::{MutationPlan, MutationResult};

#[cfg(test)]
mod tests;

use crate::error::{Error, Result};
use crate::model::Document;
use crate::package::Package;
pub use crate::save::SaveOptions;
use crate::semantic::props::{ParaPropsPatch, RunProps, RunPropsPatch};
use crate::xml::NodeId;

/// 修订作者（`EDIT-01`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionAuthor {
    pub author: String,
    pub date: String,
}

/// 编辑上下文（`EDIT-01`）。`keep_orphan_comments` 与
/// `mark_updated_fields_dirty` 在对应 span / 字段 planner 落地后生效。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditContext {
    pub track_changes: Option<RevisionAuthor>,
    pub default_run_props: Option<RunProps>,
    pub keep_orphan_comments: bool,
    pub mark_updated_fields_dirty: bool,
}

/// M1 操作（`EDIT-03`，`docs/03` §8.2 的前四个内联 / 段落变体）。
///
/// `ReplaceInlines` 依赖 `NewInline` / `NewField` / `NewBlock` 的类型设计，
/// 与任务 1.13 的 `SaveBlock` 映射一起加入。
//`EditOp` 是命令式的单次操作，不批量驻留；大 patch 直接来自调用方。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOp {
    InsertText { at: InlinePos, text: String, props: Option<RunPropsPatch> },
    DeleteRange { from: InlinePos, to: InlinePos },
    SetRunProps { from: InlinePos, to: InlinePos, patch: RunPropsPatch },
    SetParaProps { para: NodeId, patch: ParaPropsPatch },
}

impl EditOp {
    fn name(&self) -> &'static str {
        match self {
            EditOp::InsertText { .. } => "InsertText",
            EditOp::DeleteRange { .. } => "DeleteRange",
            EditOp::SetRunProps { .. } => "SetRunProps",
            EditOp::SetParaProps { .. } => "SetParaProps",
        }
    }
}

/// 会话内媒体句柄（`EDIT-01`）。任务 1.12 / 1.13 的生成器用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaId(pub u32);

/// 一次打开的文档会话。`Package` 是 DOM / 规范状态，`Document` 是可重建投影。
#[derive(Debug)]
pub struct EditSession {
    package: Package,
    document: Document,
}

impl EditSession {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut package = Package::open(bytes)?;
        let document = Document::rebuild(&mut package)?;
        Ok(Self { package, document })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    /// 解析 `InlinePos`（`EDIT-02`）。当前只支持主 part 正文的 `TextBlock` 段落。
    pub fn locate(&self, pos: InlinePos) -> Result<Loc> {
        locate::locate(&self.package, &self.document, pos)
    }

    /// 操作 → 只读变更计划。任务 1.11 尚无操作 planner；任务 1.12 逐个接上。
    pub fn plan(&self, op: &EditOp, _ctx: &EditContext) -> Result<MutationPlan> {
        Err(Error::EditUnsupported { operation: op.name() })
    }

    /// `plan → validate → commit` 的原子入口（`EDIT-05`）。
    pub fn apply(&mut self, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
        let plan = self.plan(&op, ctx)?;
        plan.validate(self)?;
        self.commit_validated(plan)
    }

    /// 批量原子应用：先把所有 op 变成 plan 并全部验证，再开始写入；任一失败则不做任何变更。
    pub fn apply_all(
        &mut self,
        ops: Vec<EditOp>,
        ctx: &EditContext,
    ) -> Result<Vec<MutationResult>> {
        let plans = ops.iter().map(|op| self.plan(op, ctx)).collect::<Result<Vec<_>>>()?;
        self.apply_plans(plans)
    }

    /// 验证并原子提交一组已生成的 plan。`apply_all` 使用它；单元测试用它验证回滚保证。
    pub(crate) fn apply_plans(&mut self, plans: Vec<MutationPlan>) -> Result<Vec<MutationResult>> {
        for plan in &plans {
            plan.validate(self)?;
        }
        let mut results = Vec::with_capacity(plans.len());
        for plan in plans {
            results.push(self.commit_validated(plan)?);
        }
        Ok(results)
    }

    /// 提交一个 `MutationPlan`。公开入口总是先 `validate`，因此机械写入阶段不会因
    /// plan 缺陷失败；失败意味着验证抓到问题，此时保证原状。
    pub fn commit(&mut self, plan: MutationPlan) -> Result<MutationResult> {
        plan.validate(self)?;
        self.commit_validated(plan)
    }

    /// `add_media` 涉及内容类型 / 文件名分配（`EDIT-06`），随首次需要它的 planner 实现。
    pub fn add_media(&mut self, _bytes: Vec<u8>, _mime: &str) -> Result<MediaId> {
        Err(Error::EditUnsupported { operation: "add_media" })
    }

    /// 保存。任务 1.11 只支持默认 `SaveOptions`，等价于 M0 的包写回；
    /// `saved_at` / `remove_personal_info` 在任务 1.14 翻译为 DOM 变更。
    pub fn save(&mut self, opts: SaveOptions) -> Result<Vec<u8>> {
        if opts != SaveOptions::default() {
            return Err(Error::EditUnsupported { operation: "non-default SaveOptions" });
        }
        self.package.save()
    }

    /// 已通过验证的计划：机械写入后整量重建投影。增量 `Document::refresh` 在 M2。
    fn commit_validated(&mut self, plan: MutationPlan) -> Result<MutationResult> {
        let result = plan.apply(&mut self.package)?;
        self.document = Document::rebuild(&mut self.package)?;
        Ok(result)
    }
}
