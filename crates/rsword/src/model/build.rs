//! `Document` 与 `Document::rebuild`（`MOD-01`、`MOD-13`，任务 1.5–1.8）：从规范状态（DOM）
//! 完整构建投影。M1 只建正文流：段落 → inlines → run 坐标流；表格 / 图片块占位；
//! `refresh` 在 M2 随编辑引擎加入。

use crate::diag::{DiagCode, Diagnostic};
use crate::error::Result;
use crate::model::block::{
    Block, ImageBlock, ListRef, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TableBlock,
    TextBlock,
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
use crate::model::theme::Theme;
use crate::package::{Package, PartId, RelTarget, RelType, Rels};
use crate::semantic::props::{
    ParaProps, RunProps, read_para_props, read_run_props, read_run_props_change,
};
use crate::span::{FlowMap, is_range_marker};
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
        let numbering_id = aux(pkg, RelType::Numbering, "word/numbering.xml");
        let settings_id = aux(pkg, RelType::Settings, "word/settings.xml");
        let theme_id = aux(pkg, RelType::Theme, "word/theme/theme1.xml");
        let font_id = aux(pkg, RelType::FontTable, "word/fontTable.xml");
        // 先确保都已解析，再同时借出
        pkg.dom(main)?;
        for id in [styles_id, numbering_id, settings_id, theme_id, font_id].into_iter().flatten() {
            let _ = pkg.dom(id);
        }
        let mut warnings = Vec::new();
        let dom_of = |id: Option<PartId>| id.and_then(|id| pkg.part(id).dom());
        let styles = dom_of(styles_id).and_then(|d| Styles::from_dom(d, &mut warnings));
        let numbering = dom_of(numbering_id).and_then(|d| Numbering::from_dom(d, &mut warnings));
        let settings = dom_of(settings_id).and_then(|d| Settings::from_dom(d, &mut warnings));
        let theme = dom_of(theme_id).and_then(Theme::from_dom);
        let font_table = dom_of(font_id).and_then(|d| FontTable::from_dom(d, &mut warnings));

        let dom = pkg.part(main).dom().expect("main part parsed above");
        let rels = &pkg.part(main).rels;
        let flows = FlowMap::build(dom);
        let mut b = Builder { dom, styles: styles.as_ref(), rels, warnings, depth: 0 };
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
            warnings,
        })
    }

    /// 只建正文（测试与工具用）：`dom` 是主 part。
    pub fn build_main(
        dom: &Dom,
        styles: Option<&Styles>,
        rels: &Rels,
    ) -> (Vec<Block>, Vec<Diagnostic>) {
        let mut b = Builder { dom, styles, rels, warnings: Vec::new(), depth: 0 };
        let mut blocks = Vec::new();
        if let Some(body) = b.find_body() {
            b.build_container(body, None, &[], &mut blocks);
        }
        (blocks, b.warnings)
    }

    pub fn text_blocks(&self) -> impl Iterator<Item = &TextBlock> {
        self.main.iter().filter_map(Block::as_text)
    }

    /// 局部刷新（`MOD-13` 的 `refresh`，M1 版本）：重建给定 `w:p` 在正文块表里的投影，
    /// 保留其 sdt / 修订上下文；返回不在正文顶层的段落（表格内等，M1 不投影）。
    pub fn refresh_paragraphs(
        &mut self,
        pkg: &mut Package,
        paras: &[NodeId],
    ) -> Result<Vec<NodeId>> {
        let main = self.main_part;
        pkg.dom(main)?;
        let dom = pkg.part(main).dom().expect("main part parsed above");
        let rels = &pkg.part(main).rels;
        let mut b =
            Builder { dom, styles: self.styles.as_ref(), rels, warnings: Vec::new(), depth: 0 };
        let mut missing = Vec::new();
        for &p in paras {
            match self.main.iter().position(|blk| blk.node() == p) {
                Some(i) => {
                    let sdt = self.main[i].sdt().cloned();
                    let revs = self.main[i].revisions().to_vec();
                    self.main[i] = b.build_paragraph(p, sdt.as_ref(), &revs);
                }
                None => missing.push(p),
            }
        }
        let warnings = b.warnings;
        self.warnings.extend(warnings);
        Ok(missing)
    }
}

struct Builder<'a> {
    dom: &'a Dom,
    styles: Option<&'a Styles>,
    rels: &'a Rels,
    warnings: Vec<Diagnostic>,
    depth: u32,
}

const MAX_CONTAINER_DEPTH: u32 = 64;

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

    fn warn(&mut self, node: NodeId, code: DiagCode, message: impl Into<String>) {
        let range = self.dom.node(node).lex.as_ref().map(|l| l.range.clone());
        self.warnings.push(Diagnostic::pre_existing(self.dom.part(), range, code, message));
    }

    fn attr(&self, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
        self.dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
    }

    fn meta(&self, node: NodeId) -> RevisionMeta {
        RevisionMeta {
            node,
            id: self.attr(node, NsId::W, LocalName::Id),
            author: self.attr(node, NsId::W, LocalName::Author),
            date: self.attr(node, NsId::W, LocalName::Date),
        }
    }

    // ---- 块 ------------------------------------------------------------------------------------

    /// body / sdtContent / 修订包裹 / customXml 的子节点 → 块（R01–R07）。
    fn build_container(
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
            let (_rule, class) = classify_body_child(dom, node);
            match class {
                BodyClass::SectionProps => out.push(Block::Protected(ProtectedBlock {
                    node,
                    kind: ProtectedKind::SectionProps,
                    preview: String::new(),
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                })),
                BodyClass::Table => out.push(Block::Table(TableBlock {
                    node,
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                })),
                BodyClass::Sdt => {
                    let info = SdtInfo { node };
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

    fn build_paragraph(&mut self, p: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Block {
        let dom = self.dom;
        let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)));
        let props: ParaProps = read_para_props(dom, ppr, &mut self.warnings);
        let facts = ParagraphFacts::compute(dom, p, &props, self.styles, sdt.cloned());
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

    /// 段落（或透明容器）的子节点 → inlines（`MOD-06`）。内联容器嵌套超过 [`MAX_CONTAINER_DEPTH`]
    /// 时不再下钻，整个子树作为一个 `Atom(Other)` 占位并记 `MOD_TOO_DEEP`（病态输入局部降级）。
    fn build_inlines(
        &mut self,
        container: NodeId,
        link: Option<&Link>,
        rev: Option<&RevisionCtx>,
        out: &mut Vec<Inline>,
    ) {
        let dom = self.dom;
        if self.depth > MAX_CONTAINER_DEPTH {
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
        for node in children {
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
                (
                    NsId::W,
                    LocalName::SmartTag
                    | LocalName::Sdt
                    | LocalName::SdtContent
                    | LocalName::CustomXml
                    | LocalName::FldSimple
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
        self.depth -= 1;
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
        Run {
            node: r,
            segments,
            text,
            utf16_len: utf16,
            props,
            link: link.cloned(),
            field: None,
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
