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
    /// 事务期间每个被写入 part 的写前镜像（`EDIT-05`）。
    txn: Option<Snapshot>,
}

/// 事务快照（`EDIT-05`）：按需记录被写入 part 的 DOM 写前镜像——[`EditSession::commit_plan`] 在
/// 第一次写某个 part 之前克隆它，所以回滚覆盖事务真正碰过的每个 part，而不是只有主 part；
/// 没碰过的 part 不付克隆代价。投影用整体 `rebuild` 恢复。
#[derive(Default)]
pub(crate) struct Snapshot {
    doms: Vec<(PartId, Dom)>,
}

impl Snapshot {
    /// 第一次写 `part` 时记下写前镜像。
    fn remember(&mut self, part: PartId, dom: &Dom) {
        if !self.doms.iter().any(|(p, _)| *p == part) {
            self.doms.push((part, dom.clone()));
        }
    }
}

impl EditSession {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::open(bytes)?)
    }

    pub fn from_package(mut pkg: Package) -> Result<Self> {
        let doc = Document::rebuild(&mut pkg)?;
        Ok(Self { pkg, doc, diagnostics: Vec::new(), txn: None })
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
        self.transaction(|s| ops::run(s, op, ctx))
    }

    /// 批量应用：任一失败则整批不生效。
    pub fn apply_all(
        &mut self,
        ops: Vec<EditOp>,
        ctx: &EditContext,
    ) -> Result<Vec<MutationResult>> {
        self.transaction(|s| {
            let mut results = Vec::with_capacity(ops.len());
            for op in ops {
                results.push(ops::run(s, op, ctx)?);
            }
            Ok(results)
        })
    }

    /// `EDIT-05` 事务边界：`f` 里的每个 plan/commit 阶段共享一个快照，任一阶段 `Err` 就把
    /// 事务碰过的每个 part 恢复到写前镜像并重建投影。事务不可嵌套（内层直接复用外层快照）。
    fn transaction<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.txn.is_some() {
            return f(self); // 已在事务里：外层负责回滚
        }
        self.txn = Some(Snapshot::default());
        match f(self) {
            Ok(v) => {
                self.txn = None;
                Ok(v)
            }
            Err(e) => {
                let snap = self.txn.take().unwrap_or_default();
                self.restore(snap)?;
                Err(e)
            }
        }
    }

    /// `SAVE-01`：等价于 `save_with(&SaveOptions::default())`。
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.save_with(&SaveOptions::default())
    }

    /// `SAVE-01` 全流程：
    ///
    /// 1. 无脏节点且 `opts` 没有变更请求（`saved_at` 单独设置不算，与 TS `isUnchanged` 一致）
    ///    且文档没有 `w:removePersonalInformation` / `w:removeDateAndTime` 标志 → 返回原字节（不变式 1）。
    /// 2. 校验（`SAVE-02`，在 [`Package::save`] 里）。
    /// 3. 物化 Span（`SPAN-08`）：Span 索引在 M2 建立，M1 无操作。
    /// 4. 应用保存选项（`SAVE-07`）：全部先 `validate`（只读）再逐个 `commit`，所以要么全做要么不动。
    /// 5. / 6. 序列化脏 part 并写回（`XML-13` / `SAVE-06`，在 [`Package::save`] 里）。
    pub fn save_with(&mut self, opts: &SaveOptions) -> Result<Vec<u8>> {
        let authors = opts.remove_personal_info.unwrap_or_else(|| self.remove_personal_info_flag());
        let dates = opts.remove_date_and_time.unwrap_or_else(|| self.remove_date_and_time_flag());
        if !self.pkg.is_dirty() && !opts.forces_save() && !authors && !dates {
            return Ok(self.pkg.original_bytes().to_vec());
        }
        // 步骤 3：SPAN-08 物化在 M2（此处无操作，Span 索引尚未建立）。
        let (plans, diags) = crate::save::options::plan_all(&mut self.pkg, opts, authors, dates)?;
        let touches_main = plans.iter().any(|p| p.part == self.pkg.main_part());
        self.transaction(|s| {
            // 先整批只读校验，再逐个提交：提交阶段不可能失败（失败也会被事务回滚）
            for plan in &plans {
                let dom = s.pkg.part(plan.part).dom().ok_or_else(|| {
                    Error::edit(DiagCode::EditPlanInvalid, "保存选项的目标不是 XML part")
                })?;
                plan.validate(dom)?;
            }
            for plan in plans {
                s.commit_plan(plan)?;
            }
            Ok(())
        })?;
        self.diagnostics.extend(diags.iter().cloned());
        self.pkg.push_diagnostics(diags);
        if touches_main {
            self.rebuild()?;
        }
        self.pkg.save()
    }

    /// 文档自带的 `w:removePersonalInformation`（`SAVE-07`：设置或文档标志为真时清洗作者）。
    pub fn remove_personal_info_flag(&self) -> bool {
        self.doc.settings.as_ref().and_then(|s| s.remove_personal_information) == Some(true)
    }

    /// 文档自带的 `w:removeDateAndTime`（设置或文档标志为真时删批注日期）。
    pub fn remove_date_and_time_flag(&self) -> bool {
        self.doc.settings.as_ref().and_then(|s| s.remove_date_and_time) == Some(true)
    }

    /// 投影整体重建。
    pub fn rebuild(&mut self) -> Result<()> {
        self.doc = Document::rebuild(&mut self.pkg)?;
        Ok(())
    }

    /// 把快照里的每个写前镜像放回去，并重建投影。
    fn restore(&mut self, snap: Snapshot) -> Result<()> {
        for (part, image) in snap.doms {
            if let Some(dom) = self.pkg.dom_mut(part)? {
                *dom = image;
            }
        }
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
        if let Some(txn) = &mut self.txn {
            txn.remember(part, dom);
        }
        let result = plan.commit(dom);
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if part == main {
            if result.structure_changed {
                self.rebuild()?;
            } else if !result.affected_paragraphs.is_empty() {
                // 投影里找不到的段落（表格单元格内的，M3 前不投影）→ 整体重建，不留过期投影
                let missing =
                    self.doc.refresh_paragraphs(&mut self.pkg, &result.affected_paragraphs)?;
                if !missing.is_empty() {
                    self.rebuild()?;
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::SaveOptions;
    use crate::semantic::props::{Change, SettingsPatch, plan_apply_settings};
    use std::io::{Cursor, Write};

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// 两个 XML part 的最小 docx（主 part + settings）。
    fn docx() -> Vec<u8> {
        let ct = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
            r#"<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#
        );
        let rels = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
        );
        let doc_rels = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#
        );
        let doc = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
        );
        let settings =
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"/>"#);
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("[Content_Types].xml", ct),
            ("_rels/.rels", rels),
            ("word/_rels/document.xml.rels", doc_rels),
            ("word/document.xml", doc.as_str()),
            ("word/settings.xml", settings.as_str()),
        ] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(bytes.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// `EDIT-05`：事务回滚覆盖它碰过的**每个** part，不只是主 part。
    #[test]
    fn edit_05_transaction_rolls_back_every_touched_part() {
        let bytes = docx();
        let mut s = EditSession::open(&bytes).unwrap();
        let main = s.main_part();
        let settings = s.package().find_name("word/settings.xml").unwrap();
        let err = s
            .transaction(|s| {
                // 阶段 1：写主 part（改字）
                let dom = s.package_mut().dom_mut(main).unwrap().unwrap();
                let t = dom
                    .descendants(dom.root())
                    .find(|&n| dom.is(n, crate::xml::QName::w(crate::xml::LocalName::T)))
                    .unwrap();
                let text = dom.children(t)[0];
                let mut plan = MutationPlan::new(main);
                plan.node_edits
                    .push(crate::xml::NodeEdit::SetText { node: text, text: "y".into() });
                s.commit_plan(plan)?;
                // 阶段 2：写 settings part
                let sdom = s.package().part(settings).dom().unwrap();
                let root = sdom.root();
                let patch = SettingsPatch {
                    remove_personal_information: Change::Set(true),
                    ..Default::default()
                };
                let mut plan = MutationPlan::new(settings);
                plan.node_edits =
                    plan_apply_settings(sdom, root, Some(root), &patch, sdom.flavor());
                s.commit_plan(plan)?;
                assert!(s.package().is_dirty(), "两个 part 都脏了");
                // 阶段 3：失败
                Err::<(), _>(Error::edit(DiagCode::EditUnsupported, "故意失败"))
            })
            .expect_err("事务应失败");
        assert!(matches!(err, Error::Edit { code: DiagCode::EditUnsupported, .. }));
        assert!(!s.package().is_dirty(), "两个 part 都回滚了");
        assert_eq!(s.save_with(&SaveOptions::default()).unwrap(), bytes, "保存回到原字节");
        assert_eq!(s.document().text_blocks().next().unwrap().text(), "x", "投影也回滚");
    }
}
