//! `Document` 与 `Document::rebuild`（`MOD-01`、`MOD-13`，任务 1.5–1.8）：从规范状态（DOM）
//! 完整构建投影。M1 只建正文流：段落 → inlines → run 坐标流；表格 / 图片块占位；
//! `refresh` 在 M2 随编辑引擎加入。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::error::Result;
use crate::model::block::{
    Block, ImageBlock, ListRef, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TextBlock,
};
use crate::model::classify::{
    BodyClass, ParaClass, classify_body_child, classify_paragraph, text_kind,
};
use crate::model::decl::{FontTable, Numbering, Settings, Styles};
use crate::model::facts::ParagraphFacts;
use crate::model::inline::{
    AtomKind, BreakKind, Inline, InlineAtom, Link, LinkTarget, OBJECT_REPLACEMENT, RevisionCtx,
    RevisionMeta, Run, Segment, SegmentKind, utf16_len,
};
use crate::model::notes::{Comments, Notes};
use crate::model::table::{BlockStep, block_at_mut_in};
use crate::model::theme::Theme;
use crate::package::{Package, PartId, RelTarget, RelType, Rels};
use crate::semantic::props::{
    ParaProps, RunProps, read_para_props, read_run_props, read_run_props_change,
};
use crate::span::field::{FieldForm, FieldId, FieldIndex};
use crate::span::{FlowMap, RangeClass, SpanId, SpanIndex, is_range_marker};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 文档模型（`MOD-01`）：DOM + Span 的语义投影。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub main_part: PartId,
    /// `w:body`；缺失时 `None`（`main` 为空，`warnings` 有诊断）。
    pub body: Option<NodeId>,
    pub main: Vec<Block>,
    pub styles: Option<Styles>,
    pub numbering: Option<Numbering>,
    pub theme: Option<Theme>,
    pub settings: Option<Settings>,
    pub font_table: Option<FontTable>,
    /// 主 part 的内容流映射（`SPAN-01`）。
    pub flows: FlowMap,
    /// 主 part 的字段索引（`FLD-02`）。与投影同寿命：`rebuild` / `refresh_paragraphs` 都重建它。
    pub fields: FieldIndex,
    /// 主 part 的范围索引（`SPAN-04`）。**这是投影侧的副本**：编辑期的规范状态在
    /// `EditSession.spans` 里，由 `SPAN-06` 变换维护；这一份只用来读（`Run.comments` 等）。
    pub spans: SpanIndex,
    /// 批注（`comments.xml` + `commentsExtended.xml` + `commentsIds.xml`）。
    pub comments: Comments,
    pub footnotes: Notes,
    pub endnotes: Notes,
    pub warnings: Vec<Diagnostic>,
}

impl Document {
    /// 从包完整构建（`MOD-13`）。辅助 part 先按关系找，找不到再按约定路径
    /// （TS 行为；见 `bind/compat_ts/KNOWN_DIFFS.md`）。
    pub fn rebuild(pkg: &mut Package) -> Result<Document> {
        let main = pkg.main_part();
        let aux = |pkg: &Package, kind: RelType, name: &str| {
            pkg.related(main, kind).next().or_else(|| pkg.find_name(name))
        };
        let styles_id = aux(pkg, RelType::Styles, "word/styles.xml");
        let comments_id = aux(pkg, RelType::Comments, "word/comments.xml");
        let comments_ex_id = aux(pkg, RelType::CommentsExtended, "word/commentsExtended.xml");
        let comments_ids_id = aux(pkg, RelType::CommentsIds, "word/commentsIds.xml");
        let footnotes_id = aux(pkg, RelType::Footnotes, "word/footnotes.xml");
        let endnotes_id = aux(pkg, RelType::Endnotes, "word/endnotes.xml");
        let numbering_id = aux(pkg, RelType::Numbering, "word/numbering.xml");
        let settings_id = aux(pkg, RelType::Settings, "word/settings.xml");
        let theme_id = aux(pkg, RelType::Theme, "word/theme/theme1.xml");
        let font_id = aux(pkg, RelType::FontTable, "word/fontTable.xml");
        // 先确保都已解析，再同时借出
        pkg.dom(main)?;
        for id in [
            styles_id,
            numbering_id,
            settings_id,
            theme_id,
            font_id,
            comments_id,
            comments_ex_id,
            comments_ids_id,
            footnotes_id,
            endnotes_id,
        ]
        .into_iter()
        .flatten()
        {
            let _ = pkg.dom(id);
        }
        let mut warnings = Vec::new();
        let dom_of = |id: Option<PartId>| id.and_then(|id| pkg.part(id).dom());
        let styles = dom_of(styles_id).and_then(|d| Styles::from_dom(d, &mut warnings));
        let numbering = dom_of(numbering_id).and_then(|d| Numbering::from_dom(d, &mut warnings));
        let settings = dom_of(settings_id).and_then(|d| Settings::from_dom(d, &mut warnings));
        let theme = dom_of(theme_id).and_then(Theme::from_dom);
        let font_table = dom_of(font_id).and_then(|d| FontTable::from_dom(d, &mut warnings));
        let with_dom = |id: Option<PartId>| id.and_then(|i| pkg.part(i).dom().map(|d| (i, d)));
        let comments = Comments::from_doms(
            with_dom(comments_id),
            with_dom(comments_ex_id),
            with_dom(comments_ids_id),
            &mut warnings,
        );
        let footnotes = Notes::from_dom(
            with_dom(footnotes_id),
            LocalName::Footnote,
            LocalName::FootnoteRef,
            &mut warnings,
        );
        let endnotes = Notes::from_dom(
            with_dom(endnotes_id),
            LocalName::Endnote,
            LocalName::EndnoteRef,
            &mut warnings,
        );

        let dom = pkg.part(main).dom().expect("main part parsed above");
        let rels = &pkg.part(main).rels;
        let flows = FlowMap::build(dom);
        let mut fields = FieldIndex::build(dom);
        warnings.extend(fields.take_diagnostics());
        let spans = SpanIndex::build(dom);
        let mut b = Builder::new(dom, styles.as_ref(), rels, &fields, &spans, warnings);
        let body = b.find_body();
        let mut blocks = Vec::new();
        if let Some(body) = body {
            b.build_container(body, None, &[], &mut blocks);
        }
        let warnings = b.warnings;
        Ok(Document {
            main_part: main,
            body,
            main: blocks,
            styles,
            numbering,
            theme,
            settings,
            font_table,
            flows,
            fields,
            spans,
            comments,
            footnotes,
            endnotes,
            warnings,
        })
    }

    /// 只建正文（测试与工具用）：`dom` 是主 part。
    pub fn build_main(
        dom: &Dom,
        styles: Option<&Styles>,
        rels: &Rels,
    ) -> (Vec<Block>, Vec<Diagnostic>) {
        let fields = FieldIndex::build(dom);
        let spans = SpanIndex::build(dom);
        let mut b = Builder::new(dom, styles, rels, &fields, &spans, Vec::new());
        let mut blocks = Vec::new();
        if let Some(body) = b.find_body() {
            b.build_container(body, None, &[], &mut blocks);
        }
        (blocks, b.warnings)
    }

    pub fn text_blocks(&self) -> impl Iterator<Item = &TextBlock> {
        self.main.iter().filter_map(Block::as_text)
    }

    /// 容器级刷新（`MOD-13` 的 `refresh`，任务 3.6）：按 [`Document::block_path`] 就地重建给定
    /// `w:p` 的投影——**正文顶层与任意深度的单元格内一视同仁**，保留它的 sdt / 修订上下文。
    /// 返回在投影里找不到的段落（调用方据此退回整体重建）。
    ///
    /// 字段与范围索引是整个 part 的投影，跟着一起重建（只重建主 part；容器级的增量在 M7 随
    /// `TEST-07` 的随机序列一起评估）。
    pub fn refresh_paragraphs(
        &mut self,
        pkg: &mut Package,
        paras: &[NodeId],
    ) -> Result<Vec<NodeId>> {
        let main = self.main_part;
        pkg.dom(main)?;
        let dom = pkg.part(main).dom().expect("main part parsed above");
        let rels = &pkg.part(main).rels;
        let fields = FieldIndex::build(dom);
        let spans = SpanIndex::build(dom);
        let mut missing = Vec::new();
        // 先把路径与上下文取齐，再借出块表——构建器借着 `self.styles`
        let mut work: Vec<RefreshItem> = Vec::new();
        for &p in paras {
            match self.block_path(p).and_then(|path| {
                let blk = self.block_at(&path)?;
                Some((path, blk.sdt().cloned(), blk.revisions().to_vec()))
            }) {
                Some((path, sdt, revs)) => work.push((p, path, sdt, revs)),
                None => missing.push(p),
            }
        }
        let mut b = Builder::new(dom, self.styles.as_ref(), rels, &fields, &spans, Vec::new());
        let mut main = std::mem::take(&mut self.main);
        for (p, path, sdt, revs) in work {
            let rebuilt = b.build_paragraph(p, sdt.as_ref(), &revs);
            match block_at_mut_in(&mut main, &path) {
                Some(slot) => *slot = rebuilt,
                None => missing.push(p),
            }
        }
        self.main = main;
        let warnings = b.warnings;
        self.warnings.extend(warnings);
        self.fields = fields;
        self.spans = spans;
        Ok(missing)
    }

    /// 任意深度的文本段落（含单元格内），按节点找。
    pub fn text_block(&self, para: NodeId) -> Option<&TextBlock> {
        self.blocks().find(|b| b.node() == para).and_then(Block::as_text)
    }
}

/// 正文构建器；表格部分在 `model/table.rs`（同一个类型的另一组方法）。
pub(super) struct Builder<'a> {
    pub(super) dom: &'a Dom,
    styles: Option<&'a Styles>,
    rels: &'a Rels,
    fields: &'a FieldIndex,
    spans: &'a SpanIndex,
    /// `FLD-08`：被 `Block` 策略字段覆盖的段落 → 字段 id。
    block_fields: HashMap<NodeId, FieldId>,
    /// 字段起点所在的段落 → 字段 id（`MOD-04` 的 `facts.fields`）。
    fields_by_para: HashMap<NodeId, Vec<FieldId>>,
    pub(super) warnings: Vec<Diagnostic>,
    /// 当前嵌套的容器层数（body / sdtContent / 修订包裹 / 单元格都算一层，段落内的内联容器也算）；
    /// 块容器超过 [`MAX_CONTAINER_DEPTH`] 层的子树降级为 `TooDeep`（`MOD-07`）。
    pub(super) depth: u32,
    /// 当前段落开始时的 `depth`：内联容器的深度上限相对它计，块的嵌套不占内联的额度
    /// （第 64 层表格里的段落照样要能建 inlines）。
    inline_base: u32,
}

impl<'a> Builder<'a> {
    fn new(
        dom: &'a Dom,
        styles: Option<&'a Styles>,
        rels: &'a Rels,
        fields: &'a FieldIndex,
        spans: &'a SpanIndex,
        warnings: Vec<Diagnostic>,
    ) -> Self {
        let mut fields_by_para: HashMap<NodeId, Vec<FieldId>> = HashMap::new();
        for f in fields.fields() {
            let head = f.form.head();
            if let Some(p) = std::iter::once(head)
                .chain(dom.ancestors(head))
                .find(|&n| dom.is(n, QName::w(LocalName::P)))
            {
                fields_by_para.entry(p).or_default().push(f.id);
            }
        }
        Builder {
            dom,
            styles,
            rels,
            fields,
            spans,
            block_fields: fields.block_result_paragraphs(dom),
            fields_by_para,
            warnings,
            depth: 0,
            inline_base: 0,
        }
    }
}

pub(super) const MAX_CONTAINER_DEPTH: u32 = 64;

/// 一次刷新里要重建的一段：节点、它在块表里的路径、以及要保留的 sdt / 修订上下文。
type RefreshItem = (NodeId, Vec<BlockStep>, Option<SdtInfo>, Vec<Revision>);

fn w(local: LocalName) -> QName {
    QName::w(local)
}

impl<'a> Builder<'a> {
    fn find_body(&mut self) -> Option<NodeId> {
        let dom = self.dom;
        let root = dom.root();
        if !dom.is(root, w(LocalName::Document)) {
            self.warn(root, DiagCode::ModUnparseable, "主 part 根不是 w:document");
            return None;
        }
        let body = dom.semantic_children(root).find(|&n| dom.is(n, w(LocalName::Body)));
        if body.is_none() {
            self.warn(root, DiagCode::ModUnparseable, "w:document 下没有 w:body");
        }
        body
    }

    pub(super) fn warn(&mut self, node: NodeId, code: DiagCode, message: impl Into<String>) {
        let range = self.dom.node(node).lex.as_ref().map(|l| l.range.clone());
        self.warnings.push(Diagnostic::pre_existing(self.dom.part(), range, code, message));
    }

    fn attr(&self, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
        self.dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
    }

    pub(super) fn meta(&self, node: NodeId) -> RevisionMeta {
        RevisionMeta {
            node,
            id: self.attr(node, NsId::W, LocalName::Id),
            author: self.attr(node, NsId::W, LocalName::Author),
            date: self.attr(node, NsId::W, LocalName::Date),
        }
    }

    // ---- 块 ------------------------------------------------------------------------------------

    /// body / sdtContent / 修订包裹 / customXml / 单元格的子节点 → 块（R01–R07）。
    pub(super) fn build_container(
        &mut self,
        container: NodeId,
        sdt: Option<&SdtInfo>,
        revs: &[Revision],
        out: &mut Vec<Block>,
    ) {
        let dom = self.dom;
        if self.depth > MAX_CONTAINER_DEPTH {
            self.warn(container, DiagCode::ModTooDeep, "块容器嵌套过深");
            out.push(Block::Protected(ProtectedBlock {
                node: container,
                kind: ProtectedKind::TooDeep,
                preview: String::new(),
                sdt: sdt.cloned(),
                revisions: revs.to_vec(),
            }));
            return;
        }
        self.depth += 1;
        let children: Vec<NodeId> = dom.semantic_children(container).collect();
        for node in children {
            if dom.name(node).is_none() {
                continue; // 空白文本
            }
            if dom.is(node, w(LocalName::TcPr)) {
                continue; // 单元格属性：`Cell.props` 已读（`MOD-07`）
            }
            let (_rule, class) = classify_body_child(dom, node);
            match class {
                BodyClass::SectionProps => out.push(Block::Protected(ProtectedBlock {
                    node,
                    kind: ProtectedKind::SectionProps,
                    preview: String::new(),
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                })),
                BodyClass::Table => {
                    let block = self.build_table(node, sdt, revs);
                    out.push(block);
                }
                BodyClass::Sdt => {
                    let info = SdtInfo::read(dom, node);
                    let content =
                        dom.semantic_children(node).find(|&n| dom.is(n, w(LocalName::SdtContent)));
                    let before = out.len();
                    if let Some(content) = content {
                        self.build_container(content, Some(&info), revs, out);
                    }
                    if out.len() == before {
                        out.push(Block::Protected(ProtectedBlock {
                            node,
                            kind: ProtectedKind::Invisible,
                            preview: String::new(),
                            sdt: Some(info),
                            revisions: revs.to_vec(),
                        }));
                    }
                }
                BodyClass::RangeMarker => {}
                BodyClass::BodyBreak { page } => out.push(Block::Protected(ProtectedBlock {
                    node,
                    kind: ProtectedKind::BodyBreak { page },
                    preview: String::new(),
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                })),
                BodyClass::InsertWrap
                | BodyClass::DeleteWrap
                | BodyClass::MoveFromWrap
                | BodyClass::MoveToWrap => {
                    let meta = self.meta(node);
                    let rev = match class {
                        BodyClass::InsertWrap => Revision::Insert(meta),
                        BodyClass::DeleteWrap => Revision::Delete(meta),
                        BodyClass::MoveFromWrap => Revision::MoveFrom(meta),
                        _ => Revision::MoveTo(meta),
                    };
                    let mut inner = revs.to_vec();
                    inner.push(rev);
                    self.build_container(node, sdt, &inner, out);
                }
                BodyClass::Transparent => self.build_container(node, sdt, revs, out),
                BodyClass::Unknown(name) => {
                    self.warn(
                        node,
                        DiagCode::ModUnknownBlock,
                        format!("无法分类的块级元素 {}", name.display(dom.interner())),
                    );
                    out.push(Block::Protected(ProtectedBlock {
                        node,
                        kind: ProtectedKind::Unknown(name),
                        preview: self.preview(node),
                        sdt: sdt.cloned(),
                        revisions: revs.to_vec(),
                    }));
                }
                BodyClass::Paragraph => {
                    let block = self.build_paragraph(node, sdt, revs);
                    out.push(block);
                }
            }
        }
        self.depth -= 1;
    }

    pub(super) fn build_paragraph(
        &mut self,
        p: NodeId,
        sdt: Option<&SdtInfo>,
        revs: &[Revision],
    ) -> Block {
        let dom = self.dom;
        self.inline_base = self.depth;
        let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)));
        let props: ParaProps = read_para_props(dom, ppr, &mut self.warnings);
        let mut facts = ParagraphFacts::compute(dom, p, &props, self.styles, sdt.cloned());
        // `MOD-04`：字段事实来自 `FieldIndex`（`FLD-08` 的块字段覆盖段落 → R09）
        facts.fields = self.fields_by_para.get(&p).cloned().unwrap_or_default();
        facts.inside_field_result = self.block_fields.get(&p).copied();
        let (_rule, class) = classify_paragraph(&facts);
        let mut revisions = revs.to_vec();
        // 段落标记修订与 pPrChange（MOD-09）
        if let Some(rpr) = &props.rpr {
            for &n in &rpr.raw_unmodeled {
                match dom.name(n).map(|q| q.local) {
                    Some(LocalName::Ins) if dom.name(n).is_some_and(|q| q.ns == NsId::W) => {
                        revisions.push(Revision::ParaMarkInsert(self.meta(n)));
                    }
                    Some(LocalName::Del) if dom.name(n).is_some_and(|q| q.ns == NsId::W) => {
                        revisions.push(Revision::ParaMarkDelete(self.meta(n)));
                    }
                    _ => {}
                }
            }
        }
        if facts.revision.ppr_change
            && let Some((change, old)) = read_para_props_change_of(dom, ppr, &mut self.warnings)
        {
            revisions
                .push(Revision::ParaPropsChange { meta: self.meta(change), old: Box::new(old) });
        }
        if let Some(num) = &props.num {
            for &n in &num.raw_unmodeled {
                if dom.is(n, w(LocalName::NumberingChange)) {
                    revisions.push(Revision::NumberingChange(self.meta(n)));
                }
            }
        }
        match class {
            ParaClass::Protected(kind) => Block::Protected(ProtectedBlock {
                node: p,
                kind,
                preview: self.preview(p),
                sdt: sdt.cloned(),
                revisions,
            }),
            ParaClass::Image => Block::Image(ImageBlock { node: p, sdt: sdt.cloned(), revisions }),
            ParaClass::Text => {
                let mut inlines = Vec::new();
                self.build_inlines(p, None, None, &mut inlines);
                self.attach_comments(p, &mut inlines);
                Block::Text(Box::new(TextBlock {
                    node: p,
                    kind: text_kind(&facts),
                    style_id: props.style.clone(),
                    props,
                    inlines,
                    sdt: sdt.cloned(),
                    revisions,
                    facts,
                }))
            }
        }
    }

    /// `COMPAT-07` 的模型侧：给 run 挂批注 id。
    ///
    /// 规则同 TS：**起终点都在本段**的批注范围覆盖到的 run 挂它的 id（只有一端在本段的范围
    /// 由块级 `commentStarts` / `commentEnds` 表达，不挂到 run 上）；文件里只有
    /// `w:commentReference` 的批注（`implicit`，LibreOffice 风格）挂到最近的有字 run
    /// ——先往前找，没有再往后找。
    fn attach_comments(&self, para: NodeId, inlines: &mut [Inline]) {
        let dom = self.dom;
        let id_of = |n: NodeId| {
            dom.attr_value(n, w(LocalName::Id)).map(|v| v.into_owned()).unwrap_or_default()
        };
        // 段内的 start / end id：两端都在本段才算覆盖
        let mut starts: Vec<String> = Vec::new();
        let mut ends: Vec<String> = Vec::new();
        let mut has_ref = false;
        let mut stack = vec![para];
        while let Some(n) = stack.pop() {
            let Some(name) = dom.name(n) else { continue };
            if name == w(LocalName::TxbxContent) {
                continue; // 独立内容流
            }
            if name == w(LocalName::CommentRangeStart) {
                starts.push(id_of(n));
            } else if name == w(LocalName::CommentRangeEnd) {
                ends.push(id_of(n));
            } else if name == w(LocalName::CommentReference) {
                has_ref = true;
            }
            for &c in dom.children(n).iter().rev() {
                stack.push(c);
            }
        }
        let both: Vec<&String> = starts.iter().filter(|s| ends.contains(s)).collect();
        if both.is_empty() && !has_ref {
            return;
        }
        // 文档序一遍：跟踪打开的范围，同时记下承载 reference 的 run
        let mut open: Vec<String> = Vec::new();
        let mut cover: HashMap<NodeId, Vec<SpanId>> = HashMap::new();
        let mut refs: Vec<(String, NodeId)> = Vec::new();
        let mut stack = vec![para];
        while let Some(n) = stack.pop() {
            let Some(name) = dom.name(n) else { continue };
            if name == w(LocalName::TxbxContent) {
                continue;
            }
            if name == w(LocalName::CommentRangeStart) {
                let id = id_of(n);
                if both.iter().any(|b| **b == id) {
                    open.push(id);
                }
            } else if name == w(LocalName::CommentRangeEnd) {
                let id = id_of(n);
                open.retain(|x| *x != id);
            } else if name == w(LocalName::R) {
                if !open.is_empty() {
                    let ids: Vec<SpanId> =
                        open.iter().filter_map(|id| self.comment_span(id)).collect();
                    if !ids.is_empty() {
                        cover.insert(n, ids);
                    }
                }
                if let Some(c) =
                    dom.semantic_children(n).find(|&c| dom.is(c, w(LocalName::CommentReference)))
                {
                    refs.push((id_of(c), n));
                }
            }
            for &c in dom.children(n).iter().rev() {
                stack.push(c);
            }
        }
        for (id, run) in refs {
            // 只有 reference 的批注（文件里没有范围标记）才挂最近的 run
            let Some(span) = self.comment_span(&id) else { continue };
            if !self.spans.get(span).is_some_and(|s| s.implicit) {
                continue;
            }
            let Some(i) = inlines.iter().position(|x| x.node() == Some(run)) else { continue };
            let has_text = |x: &Inline| matches!(x, Inline::Run(r) if !r.text.is_empty());
            let target = inlines[..i]
                .iter()
                .rposition(has_text)
                .or_else(|| inlines[i + 1..].iter().position(has_text).map(|k| k + i + 1));
            if let Some(t) = target
                && let Inline::Run(r) = &mut inlines[t]
                && !r.comments.contains(&span)
            {
                r.comments.push(span);
            }
        }
        for inline in inlines.iter_mut() {
            if let Inline::Run(r) = inline
                && let Some(ids) = cover.get(&r.node)
            {
                for id in ids {
                    if !r.comments.contains(id) {
                        r.comments.push(*id);
                    }
                }
            }
        }
    }

    /// 批注 `w:id` → 范围索引里的 `SpanId`。
    fn comment_span(&self, id: &str) -> Option<SpanId> {
        self.spans.find(RangeClass::Comment, id).map(|s| s.id)
    }

    /// 可见文本预览：`w:t` 文本拼接，截到 80 个字符。
    fn preview(&self, node: NodeId) -> String {
        let dom = self.dom;
        let mut s = String::new();
        for n in dom.descendants(node) {
            if dom.is(n, w(LocalName::T)) {
                for c in dom.semantic_children(n) {
                    if let Some(t) = dom.text(c) {
                        s.push_str(&t);
                    }
                }
            }
            if s.chars().count() > 80 {
                break;
            }
        }
        let trimmed = s.trim();
        trimmed.chars().take(80).collect()
    }

    // ---- 内联 ----------------------------------------------------------------------------------

    /// 段落（或透明容器）的子节点 → inlines（`MOD-06`）。内联容器在段落内嵌套超过
    /// [`MAX_CONTAINER_DEPTH`] 层时不再下钻，整个子树作为一个 `Atom(Other)` 占位并记 `MOD_TOO_DEEP`
    /// （病态输入局部降级）；深度相对段落起点计，所以表格嵌套不吃这个额度。
    fn build_inlines(
        &mut self,
        container: NodeId,
        link: Option<&Link>,
        rev: Option<&RevisionCtx>,
        out: &mut Vec<Inline>,
    ) {
        let dom = self.dom;
        if self.depth - self.inline_base > MAX_CONTAINER_DEPTH {
            self.warn(container, DiagCode::ModTooDeep, "内联容器嵌套过深");
            let name = dom.name(container).expect("container is an element");
            out.push(Inline::Atom(InlineAtom {
                node: container,
                kind: AtomKind::Other(name),
                props: RunProps::default(),
            }));
            return;
        }
        self.depth += 1;
        let children: Vec<NodeId> = dom.semantic_children(container).collect();
        self.build_inline_nodes(&children, link, rev, out);
        self.depth -= 1;
    }

    /// 一段兄弟节点 → inlines。字段的结果区也走这里（它是同一个容器里的一段子节点）。
    fn build_inline_nodes(
        &mut self,
        nodes: &[NodeId],
        link: Option<&Link>,
        rev: Option<&RevisionCtx>,
        out: &mut Vec<Inline>,
    ) {
        let dom = self.dom;
        let mut i = 0usize;
        while i < nodes.len() {
            let node = nodes[i];
            i += 1;
            let Some(name) = dom.name(node) else { continue };
            if is_range_marker(name) {
                continue;
            }
            match (name.ns, name.local) {
                (
                    NsId::W,
                    LocalName::PPr
                    | LocalName::ProofErr
                    | LocalName::SdtPr
                    | LocalName::SdtEndPr
                    | LocalName::CustomXmlPr
                    | LocalName::SmartTagPr,
                ) => {}
                (NsId::W, LocalName::R) => {
                    // `FLD-07` 原子形态：begin run 起，整段字段折成一个 `Inline::Field`
                    if let Some(next) = self.atomic_field_at(node, &nodes[i..], link, rev, out) {
                        i += next;
                        continue;
                    }
                    let run = self.build_run(node, link, rev);
                    out.push(Inline::Run(run));
                }
                (NsId::W, LocalName::Hyperlink) => {
                    let target = match (
                        self.attr(node, NsId::R, LocalName::Id),
                        self.attr(node, NsId::W, LocalName::Anchor),
                    ) {
                        (Some(rid), _) => {
                            let href = self.rels.by_id(&rid).and_then(|r| match &r.target {
                                RelTarget::External(h) => Some(h.clone()),
                                RelTarget::Internal(_) => None,
                            });
                            LinkTarget::External { rel_id: rid, href }
                        }
                        (None, Some(anchor)) => LinkTarget::Internal { anchor },
                        (None, None) => LinkTarget::Unresolved,
                    };
                    let l = Link::Hyperlink {
                        node,
                        target,
                        tooltip: self.attr(node, NsId::W, LocalName::Tooltip),
                    };
                    self.build_inlines(node, Some(&l), rev, out);
                }
                (
                    NsId::W,
                    LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo,
                ) => {
                    let meta = self.meta(node);
                    let mut ctx = rev.cloned().unwrap_or_default();
                    match name.local {
                        LocalName::Ins => ctx.ins = Some(meta),
                        LocalName::Del => ctx.del = Some(meta),
                        LocalName::MoveFrom => {
                            ctx.del = Some(meta.clone());
                            ctx.move_from = Some(meta);
                        }
                        _ => {
                            ctx.ins = Some(meta.clone());
                            ctx.move_to = Some(meta);
                        }
                    }
                    self.build_inlines(node, link, Some(&ctx), out);
                }
                (NsId::W, LocalName::FldSimple) => {
                    match self.fields.field_of(node).filter(|f| f.is_atomic()).map(|f| f.id) {
                        Some(id) => {
                            let mut result = Vec::new();
                            self.build_inlines(node, link, rev, &mut result);
                            out.push(Inline::Field { id, result });
                        }
                        None => self.build_inlines(node, link, rev, out),
                    }
                }
                (
                    NsId::W,
                    LocalName::SmartTag
                    | LocalName::Sdt
                    | LocalName::SdtContent
                    | LocalName::CustomXml
                    | LocalName::Dir
                    | LocalName::Bdo,
                ) => self.build_inlines(node, link, rev, out),
                (NsId::M, LocalName::OMath | LocalName::OMathPara) => {
                    out.push(Inline::Atom(InlineAtom {
                        node,
                        kind: AtomKind::Math,
                        props: RunProps::default(),
                    }));
                }
                (NsId::W, LocalName::Br) => {
                    let kind =
                        BreakKind::parse(self.attr(node, NsId::W, LocalName::Type).as_deref());
                    out.push(Inline::Atom(InlineAtom {
                        node,
                        kind: AtomKind::BareBreak { kind },
                        props: RunProps::default(),
                    }));
                }
                _ => out.push(Inline::Atom(InlineAtom {
                    node,
                    kind: AtomKind::Other(name),
                    props: RunProps::default(),
                })),
            }
        }
    }

    /// `node` 是某个原子形态字段的 begin run 且该字段在这一段兄弟节点里闭合时，把整个字段折成
    /// 一个 [`Inline::Field`]，返回要跳过的节点数（含 end run）。
    ///
    /// 结果区是 separate 与 end 之间的节点；没有 separate（XE 一类无结果字段）时结果为空。
    /// 字段没在这一段兄弟节点里闭合（跨段 / 跨容器）时返回 `None`，各 run 照常出现——
    /// 那种字段的策略是 `Block`，段落已经被 R09 保护，不该到这里。
    fn atomic_field_at(
        &mut self,
        node: NodeId,
        rest: &[NodeId],
        link: Option<&Link>,
        rev: Option<&RevisionCtx>,
        out: &mut Vec<Inline>,
    ) -> Option<usize> {
        let f = self.fields.field_of(node).filter(|f| f.form.head() == node && f.is_atomic())?;
        let (id, tail, separate) = match &f.form {
            FieldForm::Complex { end, separate, .. } => (f.id, *end, *separate),
            FieldForm::Simple { .. } => return None,
        };
        let tail_at = rest.iter().position(|&c| c == tail)?;
        let result_from = match separate {
            Some(sep) => rest.iter().position(|&c| c == sep).map_or(tail_at, |k| k + 1),
            None => tail_at,
        };
        let mut result = Vec::new();
        if result_from < tail_at {
            let nodes: Vec<NodeId> = rest[result_from..tail_at].to_vec();
            self.build_inline_nodes(&nodes, link, rev, &mut result);
        }
        out.push(Inline::Field { id, result });
        Some(tail_at + 1)
    }

    /// 一个 `w:r` → `Run`：段与坐标流文本。
    fn build_run(&mut self, r: NodeId, link: Option<&Link>, rev: Option<&RevisionCtx>) -> Run {
        let dom = self.dom;
        let mut text = String::new();
        let mut segments = Vec::new();
        let mut props = RunProps::default();
        let mut rpr_node = None;
        for c in dom.semantic_children(r) {
            let Some(name) = dom.name(c) else { continue };
            if name.ns == NsId::W && name.local == LocalName::RPr {
                rpr_node = Some(c);
                props = read_run_props(dom, Some(c), &mut self.warnings);
                continue;
            }
            let start = text.len() as u32;
            let kind = self.segment(c, name, &mut text);
            let end = text.len() as u32;
            let len = utf16_len(&text[start as usize..end as usize]);
            segments.push(Segment { node: c, kind, text: start..end, utf16_len: len });
        }
        let mut ctx = rev.cloned().unwrap_or_default();
        if let Some(rpr) = rpr_node
            && let Some((change, old)) = read_run_props_change(dom, Some(rpr), &mut self.warnings)
        {
            ctx.props_change = Some((self.meta(change), Box::new(old)));
        }
        let utf16 = utf16_len(&text);
        // `FLD-07` 透明形态（`Link` 策略）：结构 run 与结果 run 都带 `field`，结果 run 另有
        // `Link::Field`（目标来自指令，由 `compat_ts` / 渲染器解析）。外层 `w:hyperlink` 优先。
        let transparent = self.fields.field_of(r).filter(|f| f.is_transparent());
        let link = match (link, transparent) {
            (None, Some(f)) if f.form.result_nodes().contains(&r) => Some(Link::Field(f.id)),
            (l, _) => l.cloned(),
        };
        Run {
            node: r,
            segments,
            text,
            utf16_len: utf16,
            props,
            link,
            field: transparent.map(|f| f.id),
            rev: (!ctx.is_empty()).then_some(ctx),
            comments: Vec::new(),
        }
    }

    /// run 的一个子节点：追加坐标流文本，返回段种类（`MOD-06` 贡献表）。
    fn segment(&self, node: NodeId, name: QName, text: &mut String) -> SegmentKind {
        let dom = self.dom;
        if name.ns != NsId::W {
            text.push(OBJECT_REPLACEMENT);
            return SegmentKind::Other(name);
        }
        match name.local {
            LocalName::T | LocalName::DelText => {
                let mut s = String::new();
                for c in dom.semantic_children(node) {
                    if let Some(t) = dom.text(c) {
                        s.push_str(&t);
                    }
                }
                if xml_space_preserved(dom, node) {
                    text.push_str(&s);
                } else {
                    text.push_str(s.trim_matches([' ', '\t', '\r', '\n']));
                }
                if name.local == LocalName::T { SegmentKind::Text } else { SegmentKind::DelText }
            }
            LocalName::Tab => {
                text.push('\t');
                SegmentKind::Tab
            }
            LocalName::Ptab => {
                text.push('\t');
                SegmentKind::PTab { align: self.attr(node, NsId::W, LocalName::Alignment) }
            }
            LocalName::Br => {
                let kind = BreakKind::parse(self.attr(node, NsId::W, LocalName::Type).as_deref());
                text.push(match kind {
                    BreakKind::TextWrapping => '\n',
                    BreakKind::Page | BreakKind::Column => OBJECT_REPLACEMENT,
                });
                SegmentKind::Br { kind, clear: self.attr(node, NsId::W, LocalName::Clear) }
            }
            LocalName::Cr => {
                text.push('\n');
                SegmentKind::Cr
            }
            LocalName::NoBreakHyphen => {
                text.push('\u{2011}');
                SegmentKind::NoBreakHyphen
            }
            LocalName::SoftHyphen => {
                text.push('\u{00AD}');
                SegmentKind::SoftHyphen
            }
            LocalName::Sym => {
                let font = self.attr(node, NsId::W, LocalName::Font);
                let code = self
                    .attr(node, NsId::W, LocalName::Char)
                    .and_then(|h| u32::from_str_radix(h.trim(), 16).ok());
                // 符号字体映射表在 RES-05（M2）；这里按规范退路：U+F000 + (code & 0xFF)
                let ch = code
                    .and_then(|c| char::from_u32(0xF000 + (c & 0xFF)))
                    .unwrap_or(OBJECT_REPLACEMENT);
                text.push(ch);
                SegmentKind::Sym { font, code }
            }
            LocalName::Drawing => {
                text.push(OBJECT_REPLACEMENT);
                let anchored = dom
                    .semantic_children(node)
                    .any(|c| dom.is(c, QName::new(NsId::Wp, LocalName::Anchor)));
                SegmentKind::Drawing { anchored }
            }
            LocalName::Pict => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::Pict
            }
            LocalName::Object => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::Object
            }
            LocalName::Ruby => {
                text.push(OBJECT_REPLACEMENT);
                let rt = dom
                    .semantic_children(node)
                    .find(|&c| dom.is(c, w(LocalName::Rt)))
                    .map(|rt| self.preview(rt))
                    .unwrap_or_default();
                SegmentKind::Ruby { rt }
            }
            LocalName::FootnoteReference => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::FootnoteRef { id: self.attr(node, NsId::W, LocalName::Id) }
            }
            LocalName::EndnoteReference => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::EndnoteRef { id: self.attr(node, NsId::W, LocalName::Id) }
            }
            LocalName::FootnoteRef => SegmentKind::FootnoteRefMark,
            LocalName::EndnoteRef => SegmentKind::EndnoteRefMark,
            LocalName::Separator => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::Separator
            }
            LocalName::ContinuationSeparator => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::ContinuationSeparator
            }
            LocalName::CommentReference => SegmentKind::CommentRef,
            LocalName::LastRenderedPageBreak => SegmentKind::LastRenderedPageBreak,
            LocalName::FldChar => SegmentKind::FldChar,
            LocalName::InstrText => SegmentKind::InstrText,
            LocalName::DelInstrText => SegmentKind::DelInstrText,
            LocalName::AnnotationRef => SegmentKind::AnnotationRef,
            _ => {
                text.push(OBJECT_REPLACEMENT);
                SegmentKind::Other(name)
            }
        }
    }
}

/// `xml:space` 的有效值（XML 规范：沿祖先继承，最近的声明生效）；没有声明 → Word 行为，trim。
fn xml_space_preserved(dom: &Dom, node: NodeId) -> bool {
    let space = QName::new(NsId::Xml, LocalName::Space);
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let Some(v) = dom.attr_value(n, space) {
            return v.trim() == "preserve";
        }
        cur = dom.parent(n);
    }
    false
}

/// `pPr/pPrChange/pPr` 的旧值快照。
fn read_para_props_change_of(
    dom: &Dom,
    ppr: Option<NodeId>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(NodeId, ParaProps)> {
    crate::semantic::props::read_para_props_change(dom, ppr, diags)
}

impl ListRef {
    pub fn new(num_id: i32, ilvl: i32) -> Self {
        Self { num_id, ilvl, from_style: false }
    }
}
