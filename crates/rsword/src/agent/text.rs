//! AGENT-01：模型的确定性只读文本；所有树遍历使用显式任务栈。
//! 投影索引是内部构件，不是 AGENT-06 的有界工具响应。
use super::{
    anchors::{AnchorMap, ObjectRef, Result, Target, err},
    diagnostics::diagnostic_view,
};
use crate::{
    bind::native::json::{ProjCx, ToJson},
    edit::InlinePos,
    model::{Block, Display, Document, Inline, TextBlock},
    model::{
        block::{ProtectedKind, TextKind},
        inline::{AtomKind, SegmentKind},
    },
    package::{Package, PartId},
    resolve::Resolver,
    xml::NodeId,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

macro_rules! categories {
    ($($name:ident => ($wire:literal,$reason:literal,$test:ident,$fixture:literal);)*) => {
        #[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Serialize)]
        pub enum Category { $(#[serde(rename=$wire)] $name),* }
        impl Category {
            pub fn name(self)->&'static str { match self { $(Self::$name=>$wire),* } }
            pub fn reason(self)->&'static str { match self { $(Self::$name=>$reason),* } }
        }
        pub const CATEGORIES:&[Category]=&[$(Category::$name),*];
        pub const CATEGORY_FIXTURES:&[(&str,&str)]=&[$(($wire,$fixture)),*];
        $(#[cfg(test)] #[test] fn $test() {
            let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus").join($fixture);
            let mut pkg=Package::open(&std::fs::read(path).unwrap()).unwrap();
            let doc=Document::rebuild(&mut pkg).unwrap();
            let p=project(&pkg,&doc,Scope::All,"fixture:1").unwrap();
            assert!(p.omitted["page"].as_array().unwrap().iter().any(|x|x["category"]==$wire && x["count"].as_u64().unwrap()>0),"分类分派或 fixture 漂移: {}",$wire);
        })*
    }
}
categories! {
    Formatting => ("formatting","文字保留，样式细节按需读取",agent_01_formatting,"synthetic/anchored-textbox__001.docx");
    TableGeometry => ("tableGeometry","管道表不表达网格、合并与嵌套几何",agent_01_table,"synthetic/balance-dbcs-spacing__002.docx");
    FieldResult => ("fieldResult","字段指令及缓存结果仅在显式详情中只读展示",agent_01_field,"synthetic/bookmarks-crossref__004.docx");
    Image => ("image","图片二进制及显示属性按需读取",agent_01_image,"synthetic/bugfix-regressions__006.docx");
    Chart => ("chart","图表数据及显示属性按需读取",agent_01_chart,"synthetic/chart-edit__001.docx");
    Diagram => ("diagram","SmartArt 图形语义按需读取",agent_01_diagram,"synthetic/m6-smartart__001.docx");
    Math => ("math","公式不伪造计算或 OCR 结果",agent_01_math,"synthetic/insert-and-layout__001.docx");
    Ink => ("ink","墨迹不伪造识别结果",agent_01_ink,"synthetic/m6-ink__002.docx");
    Ole => ("ole","嵌入对象载荷按需读取",agent_01_ole,"synthetic/emf-image__005.docx");
    DrawingGeometry => ("drawingGeometry","形状几何按需读取，独立文字流不在父流重复",agent_01_drawing,"synthetic/anchor-z-order__001.docx");
    RevisionDetail => ("revisionDetail","修订快照及关联按需读取，未接受或拒绝修订",agent_01_revision,"synthetic/revisions__001.docx");
    Hidden => ("hidden","隐藏载荷默认不披露",agent_01_hidden,"synthetic/raw-rpr__001.docx");
    Structure => ("structure","结构标记不代表真实分页",agent_01_structure,"synthetic/anchor-z-order__001.docx");
    RangeMetadata => ("rangeMetadata","范围的完整元数据按需读取",agent_01_range,"synthetic/comments__001.docx");
    Protected => ("protected","保护内容只给类型与定位，不伪造可编辑文字",agent_01_protected,"synthetic/deep-nested-table__001.docx");
    Unknown => ("unknown","未知内容保留原文档，不静默丢弃",agent_01_unknown,"synthetic/header-footer-rich__008.docx");
    Glossary => ("glossary","构建基块可寻址，但不是当前文档内容；不自动展开",agent_01_glossary,"real/sdt/content-controls.docx");
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Omission {
    pub category: Category,
    pub count: usize,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectRange {
    pub object: ObjectRef,
    pub range: Range<u32>,
    pub metadata: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub content: String,
    pub anchors: AnchorMap,
    pub anchor_counts: super::anchors::AnchorCounts,
    pub objects: BTreeMap<String, ObjectRange>,
    pub omitted: Value,
    pub diagnostics: Vec<Value>,
}
impl Projection {
    /// 使用对象索引选择同一绝对区间；重叠选择合并，不复制父表内已经展开的单元格。
    pub fn select_objects(&self, objects: &[ObjectRef]) -> Result<Vec<Range<u32>>> {
        let mut ranges = Vec::new();
        for object in objects {
            let hit = self
                .objects
                .get(&object.key())
                .filter(|x| &x.object == object)
                .ok_or_else(|| err("AGENT_NOT_PROJECTED", "对象不在本投影"))?;
            ranges.push(hit.range.clone());
        }
        ranges.sort_by_key(|r| (r.start, r.end));
        let mut out: Vec<Range<u32>> = Vec::new();
        for r in ranges {
            if let Some(last) = out.last_mut()
                && r.start <= last.end
            {
                last.end = last.end.max(r.end);
            } else {
                out.push(r);
            }
        }
        Ok(out)
    }
    pub fn text_range(&self, range: Range<u32>) -> Result<&str> {
        let start = self.anchors.byte_offset(range.start)?;
        let end = self.anchors.byte_offset(range.end)?;
        self.content.get(start..end).ok_or_else(|| err("AGENT_BAD_OFFSET", "反向区间"))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Main,
    All,
}
pub(crate) fn flow_of(doc: &Document, part: PartId, node: NodeId) -> Option<u32> {
    doc.flow_of_in(part, node).map(|f| f.0)
}
struct Flow<'a> {
    part: PartId,
    root: NodeId,
    kind: &'static str,
    blocks: &'a [Block],
    metadata: Value,
}
enum Task<'a> {
    Block(&'a Block, Option<(&'a crate::model::TableBlock, usize, usize)>),
    Cell(&'a crate::model::TableBlock, usize, usize),
    End(ObjectRef, u32, Value),
    Mark(ObjectRef, String),
}
struct Builder<'a> {
    doc: &'a Document,
    pkg: &'a Package,
    out: Projection,
    counts: BTreeMap<Category, BTreeSet<String>>,
    seen: BTreeSet<(u32, u32)>,
    pending: VecDeque<Flow<'a>>,
    markers: BTreeMap<(u32, u32), Option<String>>,
}
impl<'a> Builder<'a> {
    fn revisions(
        &mut self,
        part: PartId,
        revisions: &[crate::model::block::Revision],
    ) -> Result<Vec<(ObjectRef, String)>> {
        use crate::model::block::Revision as R;
        let mut closes = Vec::new();
        for rev in revisions {
            let (meta, tag) = match rev {
                R::Insert(m) | R::MoveTo(m) => (m, "ins"),
                R::Delete(m) | R::MoveFrom(m) => (m, "del"),
                R::ParaMarkInsert(m)
                | R::ParaMarkDelete(m)
                | R::NumberingChange(m)
                | R::CellInsert(m)
                | R::CellDelete(m)
                | R::CellMerge(m) => (m, "revision"),
                R::ParaPropsChange { meta, old: _ }
                | R::TablePropsChange { meta, old: _ }
                | R::SectPropsChange { meta, old: _ }
                | R::TableGridChange { meta, old: _ }
                | R::RowPropsChange { meta, old: _ }
                | R::CellPropsChange { meta, old: _ } => (meta, "revision"),
            };
            let owner = self.object(part, meta.node, "revision")?;
            let start = self.out.anchors.len();
            self.mark(
                &owner,
                &format!(
                    "[{tag} #{} author={} date={}]",
                    owner.key(),
                    json!(meta.author),
                    json!(meta.date)
                ),
            );
            self.count(&owner, Category::RevisionDetail);
            self.record(
                owner.clone(),
                start,
                json!({"id":meta.id,"author":meta.author,"date":meta.date,"tag":tag}),
            );
            if tag != "revision" {
                closes.push((owner, format!("[/{tag}]")));
            }
        }
        Ok(closes)
    }
    fn object(&self, part: PartId, node: NodeId, kind: &str) -> Result<ObjectRef> {
        Ok(ObjectRef {
            part: part.0,
            node: node.0,
            flow: flow_of(self.doc, part, node).ok_or_else(|| {
                err("BIND_ID_UNKNOWN", format!("节点 {}:{} 不属于原生流", part.0, node.0))
            })?,
            kind: kind.into(),
        })
    }
    fn mark(&mut self, owner: &ObjectRef, value: &str) {
        self.out.anchors.push(
            &mut self.out.content,
            value,
            Target::Presentation { owner: owner.clone(), reason: owner.kind.clone() },
        );
    }
    fn count(&mut self, owner: &ObjectRef, c: Category) {
        self.counts.entry(c).or_default().insert(owner.key());
    }
    fn placeholder(&mut self, owner: &ObjectRef, tag: &str, c: Category) {
        let start = self.out.anchors.len();
        self.mark(owner, &format!("[{tag} #{}]", owner.key()));
        self.count(owner, c);
        self.record(owner.clone(), start, json!({}));
    }
    fn record(&mut self, object: ObjectRef, start: u32, metadata: Value) {
        let end = self.out.anchors.len();
        self.out
            .objects
            .entry(object.key())
            .and_modify(|r| {
                if start <= r.range.start {
                    r.range = start..end;
                }
                if metadata != json!({}) {
                    r.metadata = metadata.clone();
                }
            })
            .or_insert(ObjectRange { object, range: start..end, metadata });
    }
    fn source(&mut self, owner: &ObjectRef, para: NodeId, offset: u32, text: &str) {
        // 只在转义字符处分段；普通长文本不为每个字符分配锚点记录。
        let mut begin = 0;
        let mut units = offset;
        for (byte, c) in text.char_indices() {
            if matches!(c, '\\' | '[' | ']' | '|' | '#') {
                if begin < byte {
                    self.source_piece(owner, para, units, &text[begin..byte]);
                    units += text[begin..byte].encode_utf16().count() as u32;
                }
                self.mark(owner, "\\");
                self.source_piece(owner, para, units, &text[byte..byte + c.len_utf8()]);
                units += c.len_utf16() as u32;
                begin = byte + c.len_utf8();
            }
        }
        if begin < text.len() {
            self.source_piece(owner, para, units, &text[begin..]);
        }
    }
    fn source_piece(&mut self, owner: &ObjectRef, para: NodeId, offset: u32, text: &str) {
        self.out.anchors.push(
            &mut self.out.content,
            text,
            Target::Source {
                part: owner.part,
                flow: owner.flow,
                node: owner.node,
                inline_pos: InlinePos::in_part(PartId(owner.part), para, offset),
            },
        );
    }
    fn field(
        &mut self,
        part: PartId,
        id: crate::span::FieldId,
        fallback: &ObjectRef,
    ) -> Result<()> {
        if let Some((node, keyword)) = self.doc.field_projection_label(part, id) {
            let owner = self.object(part, node, "field")?;
            let key = owner.key();
            if !self.out.objects.contains_key(&key) {
                self.placeholder(
                    &owner,
                    &format!("field {}", json!(keyword)),
                    Category::FieldResult,
                );
                self.out.objects.get_mut(&key).expect("刚写入字段").metadata =
                    json!({"id":id.0,"keyword":keyword});
            }
        } else {
            self.placeholder(fallback, "field ?", Category::FieldResult);
        }
        Ok(())
    }
    fn paragraph(
        &mut self,
        part: PartId,
        t: &'a TextBlock,
        owner: &ObjectRef,
        context: Option<(&crate::model::TableBlock, usize, usize)>,
    ) -> Result<()> {
        match &t.kind {
            TextKind::Paragraph => {}
            TextKind::Heading { level } => {
                self.mark(owner, &format!("{} ", "#".repeat(*level as usize)))
            }
            TextKind::ListItem { list } => {
                if list.num_id != 0 {
                    match self.markers.get(&(part.0, t.node.0)).cloned().flatten() {
                        Some(m) => {
                            if !m.is_empty() {
                                self.mark(owner, &format!("{m} "));
                            }
                        }
                        None => {
                            self.mark(owner, &format!("[list-marker? #{}] ", owner.key()));
                            self.out.diagnostics.push(diagnostic_view(&json!({"code":"AGENT_LIST_MARKER_UNRESOLVED","part":part.0,"node":t.node.0})));
                        }
                    }
                }
            }
        }
        if t.props != Default::default() || !t.props.raw_unmodeled.is_empty()
            || t.inlines.iter().any(|i| matches!(i,Inline::Run(r) if r.props!=Default::default() || !r.props.raw_unmodeled.is_empty()))
        {
            self.count(owner, Category::Formatting);
        }
        let resolver = Resolver::new(self.doc);
        let cell = context.map(|(table, row, col)| {
            resolver
                .table(self.pkg.part(part).dom().expect("模型所属 part 已解析"), table)
                .cell(row, col)
        });
        let mut offset = 0;
        for inline in &t.inlines {
            match inline {
                Inline::Field { id, result: _ } => self.field(part, *id, owner)?,
                Inline::Atom(a) => {
                    let o = self.object(part, a.node, "atom")?;
                    match a.kind {
                        AtomKind::Math => self.placeholder(&o, "math", Category::Math),
                        AtomKind::BareBreak {
                            kind: crate::model::inline::BreakKind::TextWrapping,
                        } => self.mark(&o, "\n"),
                        AtomKind::BareBreak { .. } => {
                            self.placeholder(&o, "page-break", Category::Structure)
                        }
                        AtomKind::Other(_) => self.placeholder(&o, "unknown", Category::Unknown),
                    }
                }
                Inline::Run(r) => {
                    let o = self.object(part, r.node, "run")?;
                    if let Some(id) = r.field {
                        self.field(part, id, &o)?;
                        offset += inline.utf16_len();
                        continue;
                    }
                    if resolver
                        .run_in_table(
                            cell.as_ref().map(|c| &c.rpr),
                            t.style_id.as_deref(),
                            r.props.style.as_deref(),
                            &r.props,
                        )
                        .props
                        .vanish
                        == Some(true)
                    {
                        self.placeholder(&o, "hidden", Category::Hidden);
                        offset += inline.utf16_len();
                        continue;
                    }
                    let mut closes = Vec::new();
                    if let Some(rev) = &r.rev {
                        for (tag, meta) in [("ins", rev.ins.as_ref()), ("del", rev.del.as_ref())] {
                            if let Some(meta) = meta {
                                let ro = self.object(part, meta.node, "revision")?;
                                self.mark(
                                    &ro,
                                    &format!("[{tag} #{} author={}]", ro.key(), json!(meta.author)),
                                );
                                self.count(&ro, Category::RevisionDetail);
                                closes.push((ro, format!("[/{tag}]")));
                            }
                        }
                        if let Some((meta, _)) = &rev.props_change {
                            let ro = self.object(part, meta.node, "revision")?;
                            self.placeholder(&ro, "revision", Category::RevisionDetail);
                        }
                    }
                    let mut segment_offset = offset;
                    for seg in &r.segments {
                        let so = self.object(part, seg.node, "segment")?;
                        match &seg.kind {
                            SegmentKind::Text
                            | SegmentKind::DelText
                            | SegmentKind::Tab
                            | SegmentKind::Cr
                            | SegmentKind::NoBreakHyphen
                            | SegmentKind::SoftHyphen => {
                                self.source(&so, t.node, segment_offset, r.segment_text(seg))
                            }
                            SegmentKind::Br {
                                kind: crate::model::inline::BreakKind::TextWrapping,
                                ..
                            } => self.source(&so, t.node, segment_offset, r.segment_text(seg)),
                            SegmentKind::Br { .. } => {
                                self.placeholder(&so, "page-break", Category::Structure)
                            }
                            SegmentKind::PTab { .. } | SegmentKind::Sym { .. } => {
                                self.mark(&so, r.segment_text(seg))
                            }
                            SegmentKind::Drawing { .. }
                            | SegmentKind::Pict
                            | SegmentKind::Object => {
                                if let Some(d) = &seg.display {
                                    self.display(part, d, &so)?;
                                } else {
                                    self.placeholder(&so, "unknown", Category::Unknown);
                                }
                            }
                            SegmentKind::Ink => self.placeholder(&so, "ink", Category::Ink),
                            SegmentKind::FootnoteRef { id } => {
                                self.placeholder(&so, "footnote", Category::RangeMetadata);
                                self.out.objects.get_mut(&so.key()).unwrap().metadata =
                                    json!({"id":id});
                            }
                            SegmentKind::EndnoteRef { id } => {
                                self.placeholder(&so, "endnote", Category::RangeMetadata);
                                self.out.objects.get_mut(&so.key()).unwrap().metadata =
                                    json!({"id":id});
                            }
                            SegmentKind::CommentRef => {
                                self.placeholder(&so, "comment", Category::RangeMetadata)
                            }
                            SegmentKind::Ruby { .. } => {
                                self.placeholder(&so, "protected ruby", Category::Protected)
                            }
                            SegmentKind::Other(_) => {
                                self.placeholder(&so, "unknown", Category::Unknown)
                            }
                            SegmentKind::FldChar
                            | SegmentKind::InstrText
                            | SegmentKind::DelInstrText => self.placeholder(
                                &so,
                                "protected field-structure",
                                Category::FieldResult,
                            ),
                            SegmentKind::FootnoteRefMark
                            | SegmentKind::EndnoteRefMark
                            | SegmentKind::Separator
                            | SegmentKind::ContinuationSeparator
                            | SegmentKind::LastRenderedPageBreak
                            | SegmentKind::AnnotationRef => {
                                self.placeholder(&so, "range", Category::RangeMetadata)
                            }
                        }
                        segment_offset += seg.utf16_len;
                    }
                    for (ro, close) in closes.into_iter().rev() {
                        self.mark(&ro, &close);
                    }
                }
            }
            offset += inline.utf16_len();
        }
        self.mark(owner, "\n");
        Ok(())
    }
    fn display(&mut self, part: PartId, d: &'a Display, fallback: &ObjectRef) -> Result<()> {
        match d {
            Display::Formula(_) => self.placeholder(fallback, "math", Category::Math),
            Display::Drawing(d) => {
                let mut any = false;
                for pic in &d.pictures {
                    let o = self.object(part, pic.node.unwrap_or(d.node), "image")?;
                    self.placeholder(&o, "image", Category::Image);
                    any = true;
                }
                if let Some(chart) = &d.chart {
                    let o = self.object(part, chart.node, "chart")?;
                    self.placeholder(&o, "chart", Category::Chart);
                    any = true;
                }
                if let Some(diagram) = &d.diagram {
                    let o = self.object(part, diagram.node, "diagram")?;
                    self.placeholder(&o, "diagram", Category::Diagram);
                    any = true;
                }
                for shape in &d.shapes {
                    let o = self.object(part, shape.node, "shape")?;
                    self.placeholder(
                        &o,
                        if shape.txbx.is_some() || shape.txbx_rel.is_some() {
                            "textbox"
                        } else {
                            "shape"
                        },
                        Category::DrawingGeometry,
                    );
                    let child_part = shape.content_part.unwrap_or(part);
                    let root = shape.content.first().map(Block::node).or(shape.txbx);
                    if let Some(root) = root {
                        self.pending.push_back(Flow {
                            part: child_part,
                            root,
                            kind: "textbox",
                            blocks: &shape.content,
                            metadata: json!({"parent":o}),
                        });
                    }
                    any = true;
                }
                if d.canvas.is_some() {
                    self.placeholder(fallback, "shape", Category::DrawingGeometry);
                    any = true;
                }
                if !any {
                    self.placeholder(fallback, "unknown", Category::Unknown);
                }
            }
            Display::Vml(v) => {
                if v.ole.is_some() {
                    self.placeholder(fallback, "ole", Category::Ole);
                }
                for shape in &v.shapes {
                    if shape.nested {
                        continue;
                    }
                    let o = self.object(part, shape.node, "shape")?;
                    let (tag, category) = if shape.imagedata.is_some() {
                        ("image", Category::Image)
                    } else if shape.has_textbox {
                        ("textbox", Category::DrawingGeometry)
                    } else {
                        ("shape", Category::DrawingGeometry)
                    };
                    self.placeholder(&o, tag, category);
                    if let Some(root) = shape.txbx {
                        self.pending.push_back(Flow {
                            part,
                            root,
                            kind: "textbox",
                            blocks: &shape.content,
                            metadata: json!({"parent":o}),
                        });
                    }
                }
                if v.shapes.is_empty() && v.ole.is_none() {
                    self.placeholder(fallback, "unknown", Category::Unknown);
                }
            }
        }
        Ok(())
    }
    fn flow(&mut self, f: Flow<'a>, header: bool) -> Result<()> {
        let owner = self.object(f.part, f.root, f.kind)?;
        if !self.seen.insert((owner.part, owner.flow)) {
            return Ok(());
        }
        let start = self.out.anchors.len();
        if header {
            self.mark(
                &owner,
                &format!("[flow {} part={} metadata={}]\n", f.kind, f.part.0, f.metadata),
            );
        }
        for range in
            self.doc.range_locations_in(f.part).into_iter().filter(|r| r.flow.0 == owner.flow)
        {
            let object = self.object(f.part, range.node, &format!("range/{}", range.id))?;
            self.placeholder(
                &object,
                if range.comment { "comment" } else { "range" },
                Category::RangeMetadata,
            );
            self.out.objects.get_mut(&object.key()).expect("刚写入范围").metadata = json!({"id":range.pair_id,"start":range.start.map(|(n,i)|(n.0,i)),"end":range.end.map(|(n,i)|(n.0,i))});
            self.mark(&object, "\n");
        }
        let lists: Vec<_> = crate::model::Blocks::over(f.blocks)
            .filter_map(|b| match b {
                Block::Text(t) => match &t.kind {
                    TextKind::ListItem { list } => Some((t.node, list.clone())),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let markers = Resolver::new(self.doc)
            .list_markers(&lists.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>());
        for ((node, _), marker) in lists.into_iter().zip(markers) {
            self.markers.insert((f.part.0, node.0), marker);
        }
        let mut stack: Vec<_> = f.blocks.iter().rev().map(|b| Task::Block(b, None)).collect();
        while let Some(task) = stack.pop() {
            match task {
                Task::End(o, start, metadata) => self.record(o, start, metadata),
                Task::Mark(o, s) => self.mark(&o, &s),
                Task::Cell(table, row, col) => {
                    let cell = &table.rows[row].cells[col];
                    let o = self.object(f.part, cell.node, "cell")?;
                    let start = self.out.anchors.len();
                    stack.push(Task::End(o, start, json!({"row":row,"column":col})));
                    for (owner, close) in self.revisions(f.part, &cell.revisions)? {
                        stack.push(Task::Mark(owner, close));
                    }
                    stack.extend(
                        cell.blocks.iter().rev().map(|b| Task::Block(b, Some((table, row, col)))),
                    );
                }
                Task::Block(block, context) => {
                    let kind = match block {
                        Block::Text(_) => "paragraph",
                        Block::Table(_) => "table",
                        Block::Image(_) => "image",
                        Block::Protected(_) => "protected",
                    };
                    let o = self.object(f.part, block.node(), kind)?;
                    let start = self.out.anchors.len();
                    stack.push(Task::End(o.clone(), start, json!({})));
                    for (owner, close) in self.revisions(f.part, block.revisions())? {
                        stack.push(Task::Mark(owner, close));
                    }
                    match block {
                        Block::Text(t) => self.paragraph(f.part, t, &o, context)?,
                        Block::Table(t) => {
                            self.count(&o, Category::TableGeometry);
                            self.mark(&o, &format!("[table #{}]\n", o.key()));
                            stack.push(Task::Mark(o.clone(), "[/table]\n".into()));
                            for (row, r) in t.rows.iter().enumerate().rev() {
                                stack.push(Task::Mark(o.clone(), "\n".into()));
                                for col in (0..r.cells.len()).rev() {
                                    stack.push(Task::Mark(o.clone(), "|".into()));
                                    stack.push(Task::Cell(t, row, col));
                                }
                                stack.push(Task::Mark(o.clone(), "|".into()));
                            }
                        }
                        Block::Image(b) => {
                            if let Some(d) = &b.display {
                                self.display(f.part, d, &o)?;
                            } else {
                                self.placeholder(&o, "image", Category::Image);
                            }
                            self.mark(&o, "\n");
                        }
                        Block::Protected(b) => {
                            match &b.kind {
                                ProtectedKind::FieldBlockResult(id) => {
                                    self.field(f.part, *id, &o)?
                                }
                                ProtectedKind::Equation => {
                                    self.placeholder(&o, "math", Category::Math)
                                }
                                ProtectedKind::Chart
                                | ProtectedKind::SmartArt
                                | ProtectedKind::Ole => {
                                    if let Some(d) = &b.display {
                                        self.display(f.part, d, &o)?;
                                    } else {
                                        let (tag, c) = match b.kind {
                                            ProtectedKind::Chart => ("chart", Category::Chart),
                                            ProtectedKind::SmartArt => {
                                                ("diagram", Category::Diagram)
                                            }
                                            _ => ("ole", Category::Ole),
                                        };
                                        self.placeholder(&o, tag, c);
                                    }
                                }
                                ProtectedKind::Rule | ProtectedKind::Invisible => {
                                    if let Some(d) = &b.display {
                                        self.display(f.part, d, &o)?;
                                    } else {
                                        self.placeholder(&o, "protected", Category::Protected);
                                    }
                                }
                                ProtectedKind::SectionBreak | ProtectedKind::SectionProps => {
                                    self.placeholder(&o, "section-break", Category::Structure)
                                }
                                ProtectedKind::BodyBreak { .. } => {
                                    self.placeholder(&o, "page-break", Category::Structure)
                                }
                                ProtectedKind::Unknown(_) => {
                                    self.placeholder(&o, "unknown", Category::Unknown)
                                }
                                ProtectedKind::TooDeep | ProtectedKind::Unparseable => self
                                    .placeholder(
                                        &o,
                                        &format!("protected {}", b.kind.key()),
                                        Category::Protected,
                                    ),
                            }
                            for d in &b.siblings {
                                self.display(f.part, d, &o)?;
                            }
                            self.mark(&o, "\n");
                        }
                    }
                }
            }
        }
        if f.part == self.doc.main_part {
            for ink in self
                .doc
                .inks
                .iter()
                .filter(|i| flow_of(self.doc, f.part, i.drawing) == Some(owner.flow))
            {
                let object = self.object(f.part, ink.drawing, "segment")?;
                if !self.out.objects.contains_key(&object.key()) {
                    self.placeholder(&object, "ink", Category::Ink);
                    self.mark(&object, "\n");
                }
            }
        }
        // Run.rev 是压平上下文；完整索引还含深层包裹和属性修订，不能静默漏掉。
        for revision in self.doc.revisions.entries().iter().filter(|r| {
            r.part == f.part && flow_of(self.doc, f.part, r.meta.node) == Some(owner.flow)
        }) {
            let object = self.object(f.part, revision.meta.node, "revision")?;
            if !self.out.objects.contains_key(&object.key()) {
                self.placeholder(&object, "revision", Category::RevisionDetail);
                self.mark(&object, "\n");
            }
            self.out.objects.get_mut(&object.key()).expect("修订对象存在").metadata = json!({"id":revision.id.0,"wId":revision.meta.id,"author":revision.meta.author,"date":revision.meta.date,"kind":revision.kind.as_str()});
        }
        self.out.anchors.flow_ends.insert(self.out.anchors.len());
        self.record(owner, start, f.metadata);
        Ok(())
    }
}
/// 纯读取 Document 及其包的 URI/诊断事实；snapshot 由后续 Agent 会话管理器传入。
pub fn project(pkg: &Package, doc: &Document, scope: Scope, snapshot: &str) -> Result<Projection> {
    let body = doc.body.ok_or_else(|| err("BIND_ID_UNKNOWN", "文档没有正文流"))?;
    let owner = ObjectRef {
        part: doc.main_part.0,
        node: body.0,
        flow: flow_of(doc, doc.main_part, body)
            .ok_or_else(|| err("BIND_ID_UNKNOWN", "正文没有流身份"))?,
        kind: "main".into(),
    };
    let cx = ProjCx { pkg, display: false };
    let mut b = Builder {
        doc,
        pkg,
        out: Projection {
            content: String::new(),
            anchor_counts: Default::default(),
            anchors: AnchorMap::new(
                snapshot,
                match scope {
                    Scope::Main => "text/1:main:marked",
                    Scope::All => "text/1:all:marked",
                },
                owner,
            ),
            objects: BTreeMap::new(),
            omitted: Value::Null,
            diagnostics: doc.warnings.iter().map(|d| diagnostic_view(&d.to_json(&cx))).collect(),
        },
        counts: BTreeMap::new(),
        seen: BTreeSet::new(),
        pending: VecDeque::new(),
        markers: BTreeMap::new(),
    };
    b.flow(
        Flow {
            part: doc.main_part,
            root: body,
            kind: "main",
            blocks: &doc.main,
            metadata: json!({}),
        },
        false,
    )?;
    if scope == Scope::All {
        let mut hf: Vec<_> = doc.hf_parts.values().collect();
        hf.sort_by_key(|h| pkg.part(h.part).uri.as_str());
        for h in hf {
            let resolver = Resolver::new(doc);
            let mut references = Vec::new();
            for index in 0..doc.sections.len() {
                let effective = resolver.section(&doc.sections, index).expect("节下标来自模型");
                for variant in crate::model::HfVariant::ALL {
                    let slot = effective.slot(h.kind, variant);
                    if slot.rel_id().and_then(|rid| doc.hf_by_rel.get(rid)).copied() == Some(h.part)
                    {
                        references.push(json!({"sectionIndex":index,"variant":variant.as_str(),"inherited":!slot.is_declared(),"titlePg":effective.title_pg,"evenAndOddHeaders":effective.even_and_odd}));
                    }
                }
            }
            b.flow(
                Flow {
                    part: h.part,
                    root: h.root,
                    kind: match h.kind {
                        crate::model::HfKind::Header => "header",
                        crate::model::HfKind::Footer => "footer",
                    },
                    blocks: &h.blocks,
                    metadata: json!({"uri":pkg.part(h.part).uri.as_str(),"references":references}),
                },
                true,
            )?;
        }
        for (kind, notes) in [("footnote", &doc.footnotes), ("endnote", &doc.endnotes)] {
            if let Some(part) = notes.part {
                for n in &notes.items {
                    b.flow(
                        Flow {
                            part,
                            root: n.node,
                            kind,
                            blocks: &n.blocks,
                            metadata: json!({"id":n.id}),
                        },
                        true,
                    )?;
                }
            }
        }
        if let Some(part) = doc.comments.part {
            for c in &doc.comments.items {
                b.flow(Flow{part,root:c.node,kind:"comment",blocks:&c.blocks,metadata:json!({"id":c.id,"author":c.author,"date":c.date,"parentId":c.parent_id,"done":c.done})},true)?;
            }
        }
        while let Some(flow) = b.pending.pop_front() {
            b.flow(flow, true)?;
        }
    }
    let mut excluded = Vec::new();
    for (part, node, flow, paragraphs) in crate::model::table::glossary_flows(pkg)
        .map_err(|e| err("AGENT_PROJECTION_FAILED", e.to_string()))?
    {
        let object =
            ObjectRef { part: part.0, node: node.0, flow: flow.0, kind: "docPartBody".into() };
        b.count(&object, Category::Glossary);
        excluded.push(
            json!({"object":object,"paragraphs":paragraphs,"reason":Category::Glossary.reason()}),
        );
    }
    let page: Vec<_> = b
        .counts
        .iter()
        .map(|(c, objects)| Omission {
            category: *c,
            count: objects.len(),
            reason: c.reason().into(),
        })
        .collect();
    b.out.omitted = json!({"scope":match scope{Scope::Main=>"main",Scope::All=>"all"},"page":page,"complete":true,"unrequestedFlows":if scope==Scope::Main{json!(["headers","footers","footnotes","endnotes","comments","textboxes"])}else{json!([])}});
    b.out.omitted["addressableNotProjected"] = json!(excluded);
    b.out.anchor_counts = b.out.anchors.counts.clone();
    Ok(b.out)
}
