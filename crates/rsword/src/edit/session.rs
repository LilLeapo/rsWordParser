//! `EDIT-01` 会话：`Package`（规范状态）+ `Document`（投影）+ 事务（`EDIT-05`）。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::Document;
use crate::model::block::TextBlock;
use crate::package::{
    Package, PartFlavor, PartId, PartImage, PartUri, RelTarget, RelType, Relationship,
};
use crate::save::SaveOptions;
use crate::span::{FieldIndex, SpanIndex, is_content_item, plan_save, plan_update};
use crate::xml::{Dom, LocalName, NewElement, NodeEdit, NodeId, NsId, QName, Target};

use super::plan::{MutationPlan, MutationResult};
use super::pos::{InlinePos, Loc, locate};
use super::{EditContext, EditOp, ops};

/// 编辑会话。规范状态是包里各 part 的 DOM；`document()` 是可重建的投影。
pub struct EditSession {
    pkg: Package,
    doc: Document,
    /// 规范状态的另一半（`docs/03` §6.8）：每个被编辑过的 part 的范围索引。
    /// 按需在**第一次写该 part 之前**建立（那时 DOM 还没被改，锚点与标记一致），
    /// 之后只由 `SPAN-06` 变换维护，绝不由标记反推（`SPAN-02`）。
    spans: HashMap<PartId, SpanIndex>,
    /// 字段索引（`FLD-02`）。与范围不同，它是 DOM 的**投影**——每条事实都能重新读出来，
    /// 所以编辑后直接作废重建，不增量维护。
    fields: HashMap<PartId, FieldIndex>,
    /// 第一次写某个 part 之前记下的字段缺陷数（按诊断代码）。`FLD-13` 用它区分
    /// "输入本来如此"与"编辑造成"：保存前重建，某个代码多出来的就是引擎干的。
    field_baseline: HashMap<PartId, HashMap<DiagCode, usize>>,
    /// 第一次写某个 part 之前记下的「正文引用的 rId」与「当时已有的关系 id」（资源回收，`save/prune.rs`）：
    /// 保存时只回收本次会话让引用数归零的关系——原本就没人引用的关系不动。
    pub(crate) rel_baseline: HashMap<PartId, crate::save::prune::RelBaseline>,
    /// 本次会话按内容去重的媒体：`(mime, 字节哈希)` → 主 part 的 `rId`（`add_media`）。
    pub(crate) media_by_content: HashMap<(String, u64), String>,
    diagnostics: Vec<Diagnostic>,
    /// 事务期间每个被写入 part 的写前镜像（`EDIT-05`）。
    txn: Option<Snapshot>,
}

/// 事务快照（`EDIT-05`）：按需记录被写入 part 的 DOM 写前镜像——[`EditSession::commit_plan`] 在
/// 第一次写某个 part 之前克隆它，所以回滚覆盖事务真正碰过的每个 part，而不是只有主 part；
/// 没碰过的 part 不付克隆代价。投影用整体 `rebuild` 恢复。
#[derive(Default)]
pub(crate) struct Snapshot {
    images: Vec<(PartId, Image)>,
}

/// 一个 part 的写前镜像：节点级编辑记 DOM（与范围索引），整体替换记整个 part。
enum Image {
    Dom(Box<Dom>, Option<SpanIndex>),
    Part(PartImage),
}

impl Snapshot {
    /// 第一次写 `part` 时记下写前镜像（DOM 与范围索引一起，它们合起来是规范状态）。
    fn remember(&mut self, part: PartId, dom: &Dom, spans: Option<&SpanIndex>) {
        if !self.has(part) {
            self.images.push((part, Image::Dom(Box::new(dom.clone()), spans.cloned())));
        }
    }

    /// 第一次整体替换 `part` 之前记下整个 part（`ReplacePartXml` / `ReplacePartBytes`）。
    fn remember_part(&mut self, part: PartId, image: PartImage) {
        if !self.has(part) {
            self.images.push((part, Image::Part(image)));
        }
    }

    fn has(&self, part: PartId) -> bool {
        self.images.iter().any(|(p, _)| *p == part)
    }
}

impl EditSession {
    pub fn open(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::open(bytes)?)
    }

    pub fn from_package(mut pkg: Package) -> Result<Self> {
        let doc = Document::rebuild(&mut pkg)?;
        Ok(Self {
            pkg,
            doc,
            spans: HashMap::new(),
            fields: HashMap::new(),
            field_baseline: HashMap::new(),
            rel_baseline: HashMap::new(),
            media_by_content: HashMap::new(),
            diagnostics: Vec::new(),
            txn: None,
        })
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

    // ---- 按 part 的位置（`EDIT-02`，任务 5.5）--------------------------------------------------

    /// 位置里的 part：`None` → 主 part。
    pub fn part_or_main(&self, part: Option<PartId>) -> PartId {
        part.unwrap_or_else(|| self.pkg.main_part())
    }

    /// 某个 part 的 DOM。part 不存在或不是 XML（二进制 / `Opaque`）→ `EDIT_BAD_POSITION`。
    pub fn dom_in(&self, part: Option<PartId>) -> Result<&Dom> {
        let id = self.part_or_main(part);
        self.pkg.part(id).dom().ok_or_else(|| {
            // `Opaque`（`PKG-11` 解析失败）与"这个 part 压根不是 XML"分开报：前者是可以修的
            // 状况（重新给一份好的 part 字节），后者是调用方指错了地方
            let code = if self.pkg.part(id).is_opaque() {
                DiagCode::EditTargetOpaque
            } else {
                DiagCode::EditBadPosition
            };
            Error::edit(code, format!("part {} 没有可编辑的 XML", id.0))
        })
    }

    /// 某个 part 的 flavor（Strict / Transitional 的写法按 part 定，`PKG-08`）。
    pub fn flavor_in(&self, part: Option<PartId>) -> PartFlavor {
        self.pkg.flavor_of(self.part_or_main(part))
    }

    /// 一段 XML → 那个 part 里的 `NewElement`（`XML-14`：前缀按目标 part 的作用域解析）。
    /// 水印那棵 VML 子树是唯一手写的片段，用它解析而不是拼字符串。
    pub(crate) fn new_element_from_xml(&mut self, part: PartId, xml: &str) -> Result<NewElement> {
        let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
            Error::edit(DiagCode::EditBadPosition, format!("part {} 没有可编辑的 XML", part.0))
        })?;
        let frags = crate::xml::parse_fragment(dom, xml)
            .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("片段解析失败: {e}")))?;
        frags
            .into_iter()
            .next()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "片段没有顶层元素"))
    }

    /// `owner` 指向 `target` 的关系 id（`LinkHeaderFooter` 要把已有 part 挂到节上）。
    pub fn relationship_id(&self, owner: PartId, target: PartId) -> Option<String> {
        let uri = &self.pkg.part(target).uri;
        self.pkg
            .part(owner)
            .rels
            .iter()
            .find(|r| matches!(&r.target, crate::package::RelTarget::Internal(u) if u == uri))
            .map(|r| r.id.clone())
    }

    /// 某个 part 里的文本段落投影（页眉页脚 / 注释 / 批注条目 / 正文）。
    pub fn text_block_in(&self, part: Option<PartId>, para: NodeId) -> Option<&TextBlock> {
        self.doc.text_block_in(self.part_or_main(part), para)
    }

    /// 主 part 的范围索引（`SPAN-04`）。第一次调用时建立。
    pub fn spans(&mut self) -> Result<&SpanIndex> {
        let part = self.pkg.main_part();
        self.spans_of(part)
    }

    /// 某个 part 的范围索引；不是 XML part 时 `Err`。
    pub fn spans_of(&mut self, part: PartId) -> Result<&SpanIndex> {
        self.ensure_spans(part)?;
        Ok(self.spans.get(&part).expect("just built"))
    }

    /// 已建立的范围索引（不触发建立）。
    pub fn spans_built(&self, part: PartId) -> Option<&SpanIndex> {
        self.spans.get(&part)
    }

    /// 测试用：直接改索引，往里注入破坏，验证 `SPAN-09` 的自检真的会拦下来。
    #[cfg(test)]
    pub(crate) fn spans_mut(&mut self, part: PartId) -> Option<&mut SpanIndex> {
        self.spans.get_mut(&part)
    }

    /// 主 part 的字段索引（`FLD-02`）。
    pub fn fields(&mut self) -> Result<&FieldIndex> {
        let part = self.pkg.main_part();
        self.fields_of(part)
    }

    /// 某个 part 的字段索引；编辑之后第一次调用会重建。
    pub fn fields_of(&mut self, part: PartId) -> Result<&FieldIndex> {
        if !self.fields.contains_key(&part) {
            let index = self.build_fields(part)?;
            self.fields.insert(part, index);
        }
        Ok(self.fields.get(&part).expect("just built"))
    }

    fn build_fields(&mut self, part: PartId) -> Result<FieldIndex> {
        let Some(dom) = self.pkg.dom(part)? else {
            return Err(Error::edit(
                DiagCode::EditPlanInvalid,
                format!("part#{} 不是 XML part", part.0),
            ));
        };
        Ok(FieldIndex::build(dom))
    }

    /// `FLD-13`：在第一次写 `part` 之前记下解析期的字段缺陷，并把诊断报一次。
    /// 资源回收的写前基线（见 `save/prune.rs`）：第一次写 `part` 之前记下它引用的 rId 与它当时的关系 id。
    fn ensure_rel_baseline(&mut self, part: PartId) -> Result<()> {
        if self.rel_baseline.contains_key(&part) {
            return Ok(());
        }
        let rel_ids: std::collections::HashSet<String> =
            self.pkg.part(part).rels.iter().map(|r| r.id.clone()).collect();
        let referenced = match self.pkg.dom(part)? {
            Some(dom) => crate::save::prune::referenced_rids(dom),
            None => std::collections::HashSet::new(),
        };
        self.rel_baseline.insert(part, crate::save::prune::RelBaseline { referenced, rel_ids });
        Ok(())
    }

    /// 记一条诊断（编辑操作里的局部降级）。
    pub(crate) fn push_diagnostic(&mut self, diag: Diagnostic) {
        self.record(vec![diag]);
    }

    fn ensure_field_baseline(&mut self, part: PartId) -> Result<()> {
        if self.field_baseline.contains_key(&part) {
            return Ok(());
        }
        let mut index = self.build_fields(part)?;
        self.field_baseline.insert(part, index.defect_counts());
        let diags = index.take_diagnostics();
        self.fields.insert(part, index);
        self.record(diags);
        Ok(())
    }

    /// `FLD-13` 的保存前一半：重建字段索引，比基线多出来的缺陷就是本次编辑造成的。
    fn validate_fields(&mut self) -> Result<()> {
        let parts: Vec<PartId> = self.field_baseline.keys().copied().collect();
        let mut diags = Vec::new();
        for part in parts {
            let index = self.build_fields(part)?;
            let before = self.field_baseline.get(&part).cloned().unwrap_or_default();
            let after = index.defect_counts();
            for (code, n) in after {
                let was = before.get(&code).copied().unwrap_or(0);
                if n > was {
                    diags.push(Diagnostic::invariant_violation(
                        part,
                        None,
                        code,
                        format!("字段结构在本次编辑后新增了 {} 处 {code} 缺陷", n - was),
                    ));
                }
            }
            self.fields.insert(part, index);
        }
        crate::save::enforce(&diags)?;
        self.record(diags);
        Ok(())
    }

    /// `SPAN-04`：在第一次写 `part` 之前建立索引。
    ///
    /// 那一刻 DOM 还没被这个会话改过，所以"由标记建立 Anchor"是合法的（`SPAN-02` 只禁止
    /// 编辑期反推）。已经建立过就直接返回。绕过 `commit_plan` 直接改 DOM（`package_mut`）
    /// 之后再建立索引会读到改后的标记——那条路径要求调用方自己 `rebuild`。
    fn ensure_spans(&mut self, part: PartId) -> Result<()> {
        if self.spans.contains_key(&part) {
            return Ok(());
        }
        let Some(dom) = self.pkg.dom(part)? else {
            return Err(Error::edit(
                DiagCode::EditPlanInvalid,
                format!("part#{} 不是 XML part", part.0),
            ));
        };
        let mut index = SpanIndex::build(dom);
        let diags = index.take_diagnostics();
        self.spans.insert(part, index);
        self.record(diags);
        Ok(())
    }

    /// `SAVE-05`：批注部件，不存在就建（空 `w:comments` 根，命名空间按目标 part 的 flavor）。
    pub(crate) fn ensure_comments_part(&mut self) -> Result<PartId> {
        if let Some(p) = self.doc.comments.part {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let xml = empty_root_xml(self.pkg.flavor_of(main), "comments");
        let (id, _) =
            self.add_part(main, RelType::Comments, "word/comments.xml", CT_COMMENTS, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：`word/settings.xml`，不存在就建（清洗标志要有地方写，`SAVE-07`）。
    pub(crate) fn ensure_settings_part(&mut self) -> Result<PartId> {
        let main = self.pkg.main_part();
        if let Some(id) = self
            .pkg
            .related(main, RelType::Settings)
            .next()
            .or_else(|| self.pkg.find_name(SETTINGS))
        {
            return Ok(id);
        }
        let xml = empty_root_xml(self.pkg.flavor_of(main), "settings");
        let (id, _) = self.add_part(main, RelType::Settings, SETTINGS, CT_SETTINGS, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：脚注 / 尾注部件，不存在就建（连 Word 期待的 separator 结构条目一起）。
    pub(crate) fn ensure_notes_part(&mut self, endnote: bool) -> Result<PartId> {
        let existing = if endnote { self.doc.endnotes.part } else { self.doc.footnotes.part };
        if let Some(p) = existing {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let flavor = self.pkg.flavor_of(main);
        let w = NsId::W.uri(flavor).expect("w 有两族 URI");
        let (root, entry, mark) = if endnote {
            ("endnotes", "endnote", "continuationSeparator")
        } else {
            ("footnotes", "footnote", "continuationSeparator")
        };
        // Word 期待前两条结构条目（`w:id` 为 -1 / 0）
        let xml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<w:{root} xmlns:w="{w}">"#,
                r#"<w:{entry} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{entry}>"#,
                r#"<w:{entry} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:{mark}/></w:r></w:p></w:{entry}>"#,
                r#"</w:{root}>"#
            ),
            root = root,
            entry = entry,
            mark = mark,
            w = w
        );
        let (kind, uri, ct) = if endnote {
            (RelType::Endnotes, "word/endnotes.xml", CT_ENDNOTES)
        } else {
            (RelType::Footnotes, "word/footnotes.xml", CT_FOOTNOTES)
        };
        let (id, _) = self.add_part(main, kind, uri, ct, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：`commentsExtended` 部件（回复与已解决），不存在就建。
    pub(crate) fn ensure_comments_extended_part(&mut self) -> Result<PartId> {
        if let Some(p) = self.doc.comments.extended_part {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w15:commentsEx xmlns:w15="{}"/>"#,
            NsId::W15.uri(PartFlavor::Transitional).expect("w15 有 URI")
        );
        let (id, _) = self.add_part(
            main,
            RelType::CommentsExtended,
            "word/commentsExtended.xml",
            CT_COMMENTS_EXTENDED,
            &xml,
        )?;
        self.rebuild()?;
        Ok(id)
    }

    /// 把一个新范围登记进索引（`AddComment` / `AddBookmark`）。标记节点已经写进 DOM，
    /// 所以 `SPAN-08` 物化时它就在锚点指的位置上，不会重发。
    pub(crate) fn push_span(&mut self, part: PartId, span: crate::span::RangeSpan) -> Result<()> {
        self.ensure_spans(part)?;
        let index = self.spans.get_mut(&part).expect("just built");
        index.push_span(span);
        index.reindex_containers();
        Ok(())
    }

    /// `SPAN-07`：把索引里的范围标记为已删除（节点的删除由调用方的计划完成）。
    pub(crate) fn drop_span(&mut self, part: PartId, id: crate::span::SpanId) {
        if let Some(index) = self.spans.get_mut(&part)
            && let Some(s) = index.get_mut(id)
        {
            s.removed = true;
        }
    }

    /// `SAVE-05`：新建一个 XML part，接上关系与内容类型 Override，返回 `(part, rId)`。
    ///
    /// 三处改动都走 DOM（新 part 的内容、`.rels` 的一条 `Relationship`、
    /// `[Content_Types].xml` 的一条 `Override`），所以未变部分仍是原字节；新 part 在
    /// `SAVE-06` 里追加到 zip 末尾，其余条目原压缩数据不动。
    ///
    /// `xml` 是新 part 的整份内容。`owner` 必须已经有 `.rels`（新建 `.rels` 目前不支持——
    /// 语料里每个 docx 的主 part 都有）。
    pub fn add_part(
        &mut self,
        owner: PartId,
        kind: RelType,
        uri: &str,
        content_type: &str,
        xml: &str,
    ) -> Result<(PartId, String)> {
        let uri = PartUri::from_entry_name(uri);
        if self.pkg.find(&uri).is_some() {
            return Err(Error::edit(DiagCode::EditPlanInvalid, format!("part {uri} 已存在")));
        }
        let part = self.pkg.register_new_part(uri.clone(), content_type, xml)?;
        // 关系目标是相对 owner 所在目录的路径
        let owner_dir = self.pkg.part(owner).uri.dir().to_string();
        let target =
            uri.as_str().strip_prefix(&format!("{owner_dir}/")).unwrap_or(uri.as_str()).to_string();
        let rid = self.add_relationship(owner, kind, &target, RelTarget::Internal(uri.clone()))?;
        self.add_content_type_override(&uri, content_type)?;
        Ok((part, rid))
    }

    /// `[Content_Types].xml` 里加一条 `Override`（缺内容类型 part 时只记诊断）。
    pub(crate) fn add_content_type_override(
        &mut self,
        uri: &PartUri,
        content_type: &str,
    ) -> Result<()> {
        let Some(ct_part) = self.pkg.content_types_part() else {
            self.record(vec![Diagnostic::invariant_violation(
                self.pkg.main_part(),
                None,
                DiagCode::EditUnsupported,
                format!("缺 [Content_Types].xml，{uri} 的内容类型写不进去"),
            )]);
            return Ok(());
        };
        let dom = self
            .pkg
            .dom(ct_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "内容类型不是 XML part"))?;
        let root = dom.root();
        // 名字照抄已有的 `Override`（带着 `[Content_Types].xml` 的默认命名空间）
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::Override))
            .unwrap_or_else(|| {
                QName::new(dom.name(root).map(|q| q.ns).unwrap_or(NsId::None), LocalName::Override)
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let node = NewElement::new(name)
            .with_attr(none(LocalName::PartName), format!("/{}", uri.as_str()))
            .with_attr(none(LocalName::ContentType), content_type);
        let mut plan = MutationPlan::new(ct_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: None, node });
        self.commit_plan(plan)?;
        self.pkg.content_types_mut().add_override(uri, content_type);
        Ok(())
    }

    /// `EDIT-06`：给 `part` 的 `.rels` 追加一条外部关系，返回分配到的 `rId`。
    ///
    /// 走 `commit_plan`，所以它在事务里、可回滚，`.rels` 也按脏节点序列化。
    /// part 没有 `.rels` 时报 `EditUnsupported`——新建 `.rels` 属 `SAVE-05`（2.6）。
    pub fn add_external_relationship(
        &mut self,
        part: PartId,
        kind: RelType,
        target: &str,
    ) -> Result<String> {
        self.add_relationship(part, kind, target, RelTarget::External(target.to_string()))
    }

    /// `SAVE-05`：part 的 `.rels`，没有就建（`<dir>/_rels/<name>.rels`）。
    ///
    /// `.rels` 靠 `[Content_Types].xml` 的 `Default Extension="rels"` 声明类型，缺了就补一条。
    pub(crate) fn ensure_rels_part(&mut self, part: PartId) -> Result<PartId> {
        if let Some(p) = self.pkg.part(part).rels_part {
            return Ok(p);
        }
        let uri = self.pkg.part(part).uri.clone();
        let dir = uri.dir();
        let path = if dir.is_empty() {
            format!("_rels/{}.rels", uri.file_name())
        } else {
            format!("{dir}/_rels/{}.rels", uri.file_name())
        };
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{}"/>"#,
            RELS_NS
        );
        let rels_part = self.pkg.register_new_part(
            PartUri::from_entry_name(&path),
            "application/vnd.openxmlformats-package.relationships+xml",
            &xml,
        )?;
        self.pkg.part_mut(part).rels_part = Some(rels_part);
        self.ensure_rels_default_type()?;
        Ok(rels_part)
    }

    /// `[Content_Types].xml` 缺 `Default Extension="rels"` 时补一条。
    fn ensure_rels_default_type(&mut self) -> Result<()> {
        self.ensure_default_type("rels", "application/vnd.openxmlformats-package.relationships+xml")
    }

    /// `[Content_Types].xml` 缺某个扩展名的 `Default` 时补一条（`.rels` / `.xlsx` / 媒体扩展名）。
    pub(crate) fn ensure_default_type(&mut self, ext: &str, content_type: &str) -> Result<()> {
        let Some(ct_part) = self.pkg.content_types_part() else { return Ok(()) };
        if self.pkg.content_types().default_for_extension(ext).is_some() {
            return Ok(());
        }
        let dom = self
            .pkg
            .dom(ct_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "内容类型不是 XML part"))?;
        let root = dom.root();
        // 缓存之外再看一眼 DOM（本会话刚补过的也算）：`Default Extension` 重复会让 Word 弹恢复提示
        let already = dom.semantic_children(root).any(|c| {
            dom.name(c).is_some_and(|q| q.local == LocalName::UDefault)
                && dom
                    .attr_value(c, QName::new(NsId::None, LocalName::UExtension))
                    .is_some_and(|v| v.eq_ignore_ascii_case(ext))
        });
        if already {
            self.pkg.content_types_mut().add_default(ext, content_type);
            return Ok(());
        }
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::UDefault))
            .unwrap_or_else(|| {
                QName::new(dom.name(root).map(|q| q.ns).unwrap_or(NsId::None), LocalName::UDefault)
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let node = NewElement::new(name)
            .with_attr(none(LocalName::UExtension), ext)
            .with_attr(none(LocalName::ContentType), content_type);
        let first = dom.children(root).first().copied();
        let mut plan = MutationPlan::new(ct_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: first, node });
        self.commit_plan(plan)?;
        self.pkg.content_types_mut().add_default(ext, content_type);
        Ok(())
    }

    /// 给 `part` 的 `.rels` 追加一条关系（内部或外部），返回分配到的 `rId`。
    pub(crate) fn add_relationship(
        &mut self,
        part: PartId,
        kind: RelType,
        target: &str,
        resolved: RelTarget,
    ) -> Result<String> {
        let external = matches!(resolved, RelTarget::External(_));
        let rels_part = self.ensure_rels_part(part)?;
        let id = self.pkg.part(part).rels.next_id();
        let flavor = self.pkg.flavor_of(part);
        let raw_type = kind.uri(flavor).ok_or_else(|| {
            Error::edit(DiagCode::EditUnsupported, format!("关系类型 {kind:?} 没有 URI"))
        })?;
        let dom = self
            .pkg
            .dom(rels_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, ".rels 不是 XML part"))?;
        let root = dom.root();
        // 名字照抄已有的 `Relationship`（它带着 `.rels` 的默认命名空间）；一条都没有时按根元素的
        // 命名空间造一个
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::Relationship))
            .unwrap_or_else(|| {
                QName::new(
                    dom.name(root).map(|q| q.ns).unwrap_or(NsId::None),
                    LocalName::Relationship,
                )
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let mut node = NewElement::new(name)
            .with_attr(none(LocalName::UId), id.clone())
            .with_attr(none(LocalName::UType), raw_type.clone())
            .with_attr(none(LocalName::Target), target);
        if external {
            node.push_attr(none(LocalName::TargetMode), "External");
        }
        let mut plan = MutationPlan::new(rels_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: None, node });
        let result = self.commit_plan(plan)?;
        let created = result
            .created
            .first()
            .copied()
            .flatten()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "关系节点没有创建成功"))?;
        let (rel_kind, family) = RelType::parse(&raw_type);
        self.pkg.part_mut(part).rels.push(Relationship {
            id: id.clone(),
            kind: rel_kind,
            target: resolved,
            raw_type,
            family,
            node: created,
        });
        Ok(id)
    }

    /// 记诊断（会话与包各留一份）。
    fn record(&mut self, diags: Vec<Diagnostic>) {
        if diags.is_empty() {
            return;
        }
        self.diagnostics.extend(diags.iter().cloned());
        self.pkg.push_diagnostics(diags);
    }

    /// 编辑阶段累计的诊断（不含包 / 保存阶段的）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// 主 part 里的文本段落块，**含表格单元格内任意深度的**（任务 3.6）。
    pub fn text_block(&self, para: NodeId) -> Option<&TextBlock> {
        self.doc.text_block(para)
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
    /// 3. 物化 Span（`SPAN-08`）与范围校验（`SPAN-09`）：位置没变的标记不动，变了的重发。
    /// 4. 应用保存选项（`SAVE-07`）：全部先 `validate`（只读）再逐个 `commit`，所以要么全做要么不动。
    /// 5. / 6. 序列化脏 part 并写回（`XML-13` / `SAVE-06`，在 [`Package::save`] 里）。
    pub fn save_with(&mut self, opts: &SaveOptions) -> Result<Vec<u8>> {
        let authors = opts.remove_personal_info.unwrap_or_else(|| self.remove_personal_info_flag());
        let dates = opts.remove_date_and_time.unwrap_or_else(|| self.remove_date_and_time_flag());
        if !self.pkg.is_dirty() && !opts.forces_save() && !authors && !dates {
            return Ok(self.pkg.original_bytes().to_vec());
        }
        // 要写 `true` 的清洗标志得有地方放：缺 `word/settings.xml` 就按 `SAVE-05` 建一个。
        // 写 `false` 时不建——标志缺失本来就等于 false，凭空造个 part 只是噪音。
        if opts.remove_personal_info == Some(true) || opts.remove_date_and_time == Some(true) {
            self.transaction(|s| s.ensure_settings_part().map(|_| ()))?;
        }
        // `SAVE-07` 的内容类选项（节 / 页眉页脚 / 水印 / 底色 / 保护 / 奇偶页眉）先翻成 5.5 的
        // 编辑操作走一遍 `apply_all`：与手写这些操作完全同一条路（同一套校验、脏标记与新建 part）。
        let (ops, created) = crate::save::options::edit_ops(self, opts);
        if !ops.is_empty() {
            self.apply_all(ops, &EditContext::default())?;
        }
        // `hfAllSections` 要等上一轮把 part 建出来才知道挂哪个，所以分两轮
        let links = crate::save::options::link_ops(self, opts, &created);
        if !links.is_empty() {
            self.apply_all(links, &EditContext::default())?;
        }
        // 5.7：声明 part 的选项要改的 part 缺了就先按 `SAVE-05` 建（`plan_all` 是只读的）
        if self.transaction(|s| crate::save::options::decl::ensure_parts(s, opts))? {
            self.rebuild()?;
        }
        let (plans, diags) = crate::save::options::plan_all(&mut self.pkg, opts, authors, dates)?;
        let mut touches_main = plans.iter().any(|p| p.part == self.pkg.main_part());
        touches_main |= self.transaction(|s| s.materialize_spans())?;
        // `FLD-13`：物化之后字段结构应当仍然完好（物化只动范围标记，不该碰 fldChar）
        self.validate_fields()?;
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
        // 6.7：回收本次会话让引用数归零的资源（关系 + part 子图 + `[Content_Types]` Override）
        if opts.prune_orphans.unwrap_or(true) {
            self.transaction(|s| s.prune_orphans().map(|_| ()))?;
        }
        self.pkg.save()
    }

    /// `SAVE-01` 步骤 3：把每个 part 的 Anchor 物化成标记（`SPAN-08`），顺带做范围校验
    /// （`SPAN-09`）。返回主 part 是否被改动（需要重建投影）。
    ///
    /// 这个计划**不走**锚点变换：标记是 Anchor 的投影，不能反过来影响它（`SPAN-02`）。
    fn materialize_spans(&mut self) -> Result<bool> {
        let main = self.pkg.main_part();
        let mut touched_main = false;
        let parts: Vec<PartId> = self.spans.keys().copied().collect();
        for part in parts {
            let index = self.spans.get(&part).expect("key came from the map");
            let dom = self.pkg.part(part).dom().ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
            })?;
            let mplan = plan_save(dom, index);
            if mplan.is_empty() {
                continue;
            }
            // `SAVE-02`：范围校验里的 `EngineInvariantViolation`（引擎自己弄丢 / 弄反了端点）
            // 在调试构建与 CI 下是错误。输入本来就损坏的、以及调用方整体重写容器时丢的那一端
            // 记成 `PreExistingDamage`（`SpanOrigin::Damaged`），不在这里拦。
            crate::save::enforce(&mplan.diagnostics)?;
            let mut plan = MutationPlan::new(part);
            plan.node_edits = mplan.edits.clone();
            let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
            })?;
            plan.validate(dom)?;
            if let Some(txn) = &mut self.txn {
                txn.remember(part, dom, self.spans.get(&part));
            }
            let has_edits = !plan.node_edits.is_empty();
            let result = plan.commit(dom);
            let index = self.spans.get_mut(&part).expect("key came from the map");
            crate::span::apply_save(index, &result.created, &mplan);
            self.record(mplan.diagnostics);
            touched_main |= has_edits && part == main;
        }
        Ok(touched_main)
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
        for (part, image) in snap.images {
            match image {
                Image::Dom(image, spans) => {
                    if let Some(dom) = self.pkg.dom_mut(part)? {
                        *dom = *image;
                    }
                    match spans {
                        Some(idx) => {
                            self.spans.insert(part, idx);
                        }
                        None => {
                            self.spans.remove(&part);
                        }
                    }
                }
                Image::Part(image) => {
                    self.pkg.restore_part(part, image);
                    self.spans.remove(&part);
                }
            }
            self.fields.remove(&part); // 投影，重建即可
        }
        self.rebuild()
    }

    /// `ReplacePartXml`：整个 XML part 换成 `xml`（TS `partXml`）。只接受**已存在**的 XML part：
    /// 不存在 → `EDIT_TARGET_MISSING`（TS 静默忽略，`docs/04` §8），二进制 part → `EDIT_TARGET_OPAQUE`。
    /// 新内容经解析成为该 part 的新 DOM（良构校验），关系与内容类型不动；投影整体重建。
    pub(crate) fn replace_part_xml(&mut self, part: PartId, xml: &str) -> Result<()> {
        if (part.0 as usize) >= self.pkg.parts().len() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("part#{} 不在包里", part.0),
            ));
        }
        if !self.pkg.part(part).is_xml {
            return Err(Error::edit(
                DiagCode::EditTargetOpaque,
                format!("{} 不是 XML part，不能按 XML 替换", self.pkg.part(part).uri),
            ));
        }
        self.ensure_rel_baseline(part)?;
        let image = self.pkg.snapshot_part(part);
        if let Some(txn) = &mut self.txn {
            txn.remember_part(part, image);
        }
        self.pkg.replace_part_xml(part, xml)?;
        self.spans.remove(&part);
        self.fields.remove(&part);
        self.rebuild()
    }

    /// `ReplacePartBytes`：整个 part 换成给定字节（TS `partBinary`）。主 part 不能换（它的 DOM 是模型的根）。
    pub(crate) fn replace_part_bytes(&mut self, part: PartId, bytes: Vec<u8>) -> Result<()> {
        if (part.0 as usize) >= self.pkg.parts().len() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("part#{} 不在包里", part.0),
            ));
        }
        if part == self.pkg.main_part() {
            return Err(Error::edit(DiagCode::EditUnsupported, "主 part 不能按二进制替换"));
        }
        let image = self.pkg.snapshot_part(part);
        if let Some(txn) = &mut self.txn {
            txn.remember_part(part, image);
        }
        self.pkg.replace_part_bytes(part, bytes);
        self.spans.remove(&part);
        self.fields.remove(&part);
        self.rebuild()
    }

    /// `SAVE-05`：新建一个二进制 part（内嵌工作簿、媒体），接上关系与按扩展名的 `Default` 内容类型，
    /// 返回 `(part, rId)`。
    pub fn add_binary_part(
        &mut self,
        owner: PartId,
        kind: RelType,
        uri: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(PartId, String)> {
        let uri = PartUri::from_entry_name(uri);
        if self.pkg.find(&uri).is_some() {
            return Err(Error::edit(DiagCode::EditPlanInvalid, format!("part {uri} 已存在")));
        }
        let part = self.pkg.register_new_binary_part(uri.clone(), content_type, bytes)?;
        let owner_dir = self.pkg.part(owner).uri.dir().to_string();
        let target =
            uri.as_str().strip_prefix(&format!("{owner_dir}/")).unwrap_or(uri.as_str()).to_string();
        let rid = self.add_relationship(owner, kind, &target, RelTarget::Internal(uri.clone()))?;
        if let Some(ext) = uri.as_str().rsplit_once('.').map(|(_, e)| e.to_string()) {
            self.ensure_default_type(&ext, content_type)?;
        }
        Ok((part, rid))
    }

    /// 一个阶段：`validate` → `commit` → 刷新投影 → 记诊断。编辑操作只碰主 part；保存选项
    /// （`SAVE-07`）也走这里，可以指向任意 XML part（投影只在主 part 上刷新）。
    pub(crate) fn commit_plan(&mut self, mut plan: MutationPlan) -> Result<MutationResult> {
        let main = self.pkg.main_part();
        let part = plan.part;
        self.ensure_spans(part)?;
        self.ensure_field_baseline(part)?;
        self.ensure_rel_baseline(part)?;
        let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
            Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
        })?;
        let index = self.spans.get(&part).expect("ensure_spans built it");
        // `SPAN-06`：锚点变换从编辑列表推导，每个操作都自动得到维护
        let mut update = plan_update(dom, index, &plan.node_edits, &plan.span);
        // `SPAN-07`：整体删除的范围连标记与 reference run 一起删
        let extra: Vec<NodeId> = update.removed_nodes().collect();
        if !extra.is_empty() {
            let touches_content = extra.iter().any(|&n| is_content_item(dom, n));
            plan.node_edits.extend(extra.into_iter().map(NodeEdit::Delete));
            if touches_content {
                // 删掉的 reference run 是内容项，边界要按最终的编辑列表重算
                update = plan_update(dom, index, &plan.node_edits, &plan.span);
            }
        }
        plan.validate(dom)?;
        if let Some(txn) = &mut self.txn {
            txn.remember(part, dom, self.spans.get(&part));
        }
        let span_diags = std::mem::take(&mut update.diagnostics);
        let result = plan.commit(&mut *dom);
        if !update.is_empty() {
            let index = self.spans.get_mut(&part).expect("ensure_spans built it");
            index.apply(&*dom, &update);
            let more = index.take_diagnostics();
            self.record(more);
        }
        self.record(span_diags);
        // 字段索引是投影：DOM 变了就作废，下次问的时候重建（`FLD-02`）
        self.fields.remove(&part);
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if part == main {
            if result.structure_changed {
                self.rebuild()?;
            } else if !result.affected_blocks.is_empty() {
                // 容器级刷新（`MOD-13`）：单元格内的段落也就地重建。真找不到（投影与 DOM 不同步）
                // 才整体重建——那是兜底，不是正常路径
                let missing = self.doc.refresh_blocks(&mut self.pkg, &result.affected_blocks)?;
                if !missing.is_empty() {
                    self.rebuild()?;
                }
            }
        } else if !result.affected_blocks.is_empty() || result.structure_changed {
            // 辅助 part（页眉页脚 / 注释 / 批注，任务 5.5）：整体重建投影。
            // 这些 part 很小（几 KB），容器级增量不值得；`MOD-13` 的 oracle 照样成立
            // （`refresh` 的结果等于 `rebuild`——这里就是 `rebuild`）。
            self.rebuild()?;
        }
        Ok(result)
    }
}

/// `word/settings.xml` 的约定路径。
const SETTINGS: &str = "word/settings.xml";

/// `.rels` 的根命名空间。
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// `SAVE-05` 的内容类型。
pub(crate) const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
pub(crate) const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
pub(crate) const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
pub(crate) const CT_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
pub(crate) const CT_HEADER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
pub(crate) const CT_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
pub(crate) const CT_COMMENTS_EXTENDED: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml";

/// 新建 `w:` 部件的空根：`<w:xxx xmlns:w="…"/>`，URI 按目标 part 的 flavor。
///
/// 只声明用得上的命名空间；`w14:paraId` 一类扩展前缀由 `SAVE-03` 的
/// `ensure_extension_declarations` 在序列化前按需补声明（连 `mc:Ignorable` 一起）。
fn empty_root_xml(flavor: PartFlavor, local: &str) -> String {
    let w = NsId::W.uri(flavor).expect("w 有两族 URI");
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{local} xmlns:w="{w}"/>"#)
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
        docx_with(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#)
    }

    /// 同上，正文由调用方给。
    fn docx_with(body: &str) -> Vec<u8> {
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
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#
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

    /// `SPAN-09` / `SAVE-02`：引擎自己弄丢一端的范围在调试构建下让保存失败，发布构建只记诊断。
    ///
    /// 索引没有对外的可变入口，破坏只能从 crate 内部注入——这条自检就是为了让"变换弄丢锚点"
    /// 这类缺陷在 CI 里当场暴露，而不是悄悄写出一份半开的范围。
    #[test]
    fn span_09_engine_broken_range_fails_the_save_in_debug_builds() {
        let bytes = docx_with(
            r#"<w:p><w:bookmarkStart w:id="1" w:name="a"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
        );
        let mut s = EditSession::open(&bytes).unwrap();
        let para = s.nth_text_block(0).expect("text block").node;
        // 一次正常编辑：建立索引并让主 part 变脏（否则保存直接返回原字节）
        s.apply(
            EditOp::InsertText { at: InlinePos::new(para, 0), text: "y".into(), props: None },
            &EditContext::default(),
        )
        .expect("插入成功");
        let main = s.main_part();
        let index = s.spans_mut(main).expect("索引已建立");
        let span = index.live().next().expect("书签范围").id;
        index.get_mut(span).expect("范围还在").end = None; // 注入破坏：终点不见了
        let saved = s.save();
        if cfg!(debug_assertions) {
            match saved {
                Err(Error::Invariant(d)) => {
                    assert_eq!(d.code, DiagCode::SpanUnclosed);
                    assert_eq!(d.origin, crate::diag::ValidationOrigin::EngineInvariantViolation);
                }
                other => panic!("调试构建下应 Err(SAVE_INVARIANT)：{other:?}"),
            }
        } else {
            assert!(saved.is_ok(), "发布构建只记诊断");
        }
    }
}
