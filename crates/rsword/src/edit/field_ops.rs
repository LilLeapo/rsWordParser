//! 块字段的收集与重算（`FLD-09`，`spec/18` 7.8）。
//!
//! 生成器（`span::field::generate`）只把算好的条目摊成 XML；从文档里**收**条目在这里——
//! 走标题、收 `XE` 词、数 `SEQ`，都要 `EditSession`。
//!
//! `RegenerateBlockField` = 收条目 → 生成 → 走 `UpdateBlockField` 那条既有机制换结果区，
//! 所以 `w:fldLock`、追踪、跨段字段的规则一条都不用重写。

use std::collections::HashMap;

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::Inline;
use crate::model::SegmentKind;
use crate::model::{Block, TextBlock};
use crate::resolve::Resolver;
use crate::span::FieldId;
use crate::span::field::Keyword;
use crate::span::field::generate::index::IndexOptions;
use crate::span::field::generate::toc::{TocEntry, TocOptions};
use crate::span::field::generate::{index as index_gen, seq as seq_gen, toc as toc_gen};
use crate::xml::NodeId;

use super::plan::MutationResult;
use super::pos::InlinePos;
use super::session::EditSession;
use super::{EditContext, NewBlock};

/// `RegenerateBlockField` 的重算方式。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BlockFieldOptions {
    /// 按字段自己的指令开关重算（`TOC` / `INDEX` 各自认得的那些）。
    Auto {
        /// TOC：标题段落 → 页码（调用方的分页结果）。`None` = 不写页码。
        pages: Option<HashMap<NodeId, u32>>,
    },
    Toc {
        opts: Box<TocOptions>,
        pages: Option<HashMap<NodeId, u32>>,
    },
    Index(Box<IndexOptions>),
}

impl Default for BlockFieldOptions {
    fn default() -> Self {
        BlockFieldOptions::Auto { pages: None }
    }
}

/// 新插入的块字段（`NewBlock::Field`）。条目由当前文档算。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewBlockField {
    Toc { opts: Box<TocOptions>, pages: Option<HashMap<NodeId, u32>> },
    Index(Box<IndexOptions>),
}

/// 段落的标题级别：`\t` 的自定义样式优先，然后是样式链给的级别（`RES-02`），
/// `\u` 时还认段落自己的 `outlineLvl`。不是标题 → `None`。
fn level_of(tb: &TextBlock, r: &Resolver<'_>, opts: &TocOptions) -> Option<u8> {
    // `\t` 给的自定义样式优先
    if let Some(id) = tb.style_id.as_deref()
        && let Some((_, l)) = opts.styles.iter().find(|(n, _)| n == id)
    {
        return Some(*l);
    }
    // `\u`：`facts.outline_level` 已经是 1 起的级别（段落自己的 `outlineLvl` + 1，
    // 没有就退回样式链）。不带 `\u` 时只认样式链给的标题级别（`RES-02`）。
    if opts.use_outline
        && let Some(l) = tb.facts.outline_level
    {
        return Some(l);
    }
    tb.style_id.as_deref().and_then(|id| r.heading_level(id))
}

/// 目录条目的文字：坐标流去掉字段结构、脚注 / 尾注引用与图形占位（TS 的 `blocks[].runs` 同样不含它们）。
fn entry_text(tb: &TextBlock) -> String {
    let mut out = String::new();
    for i in &tb.inlines {
        let Inline::Run(r) = i else { continue };
        for seg in &r.segments {
            let skip = matches!(
                seg.kind,
                SegmentKind::DelText
                    | SegmentKind::Drawing { .. }
                    | SegmentKind::Pict
                    | SegmentKind::Object
                    | SegmentKind::Ink
                    | SegmentKind::FootnoteRef { .. }
                    | SegmentKind::EndnoteRef { .. }
                    | SegmentKind::FootnoteRefMark
                    | SegmentKind::EndnoteRefMark
            );
            // 字段的结构 run（begin / 指令 / separate / end）在坐标流里长度为 0，
            // 但指令文本不该进目录：只取结果区与普通文字
            if skip {
                continue;
            }
            out.push_str(&r.text[seg.text.start as usize..seg.text.end as usize]);
        }
    }
    out.trim().to_string()
}

/// 正文里按文档序的目录条目（含表格单元格内的段落）。
pub fn toc_entries(
    s: &mut EditSession,
    opts: &TocOptions,
    pages: Option<&HashMap<NodeId, u32>>,
) -> Result<Vec<TocEntry>> {
    let bookmarks = existing_toc_bookmarks(s)?;
    let doc = s.document();
    let r = Resolver::new(doc);
    Ok(doc
        .blocks()
        .filter_map(Block::as_text)
        .filter_map(|tb| {
            let level = level_of(tb, &r, opts)?;
            if level < opts.levels.0 || level > opts.levels.1 {
                return None;
            }
            let text = entry_text(tb);
            if text.is_empty() {
                return None;
            }
            Some(TocEntry {
                level,
                text,
                page_no: pages.and_then(|m| m.get(&tb.node).copied()),
                bookmark: bookmarks.get(&tb.node).cloned(),
            })
        })
        .collect())
}

/// 已经挂在段落上的 `_Toc…` 书签（重算时不重新铸名字）。
fn existing_toc_bookmarks(s: &mut EditSession) -> Result<HashMap<NodeId, String>> {
    let part = s.main_part();
    let idx = s.spans_of(part)?;
    let mut out = HashMap::new();
    for sp in idx.live() {
        let Some(name) = sp.kind.bookmark_name() else { continue };
        if !name.starts_with("_Toc") {
            continue;
        }
        if let Some(a) = sp.start.as_ref() {
            out.entry(a.container).or_insert_with(|| name.to_string());
        }
    }
    Ok(out)
}

/// 主 part 里全部 `XE` 字段的词（第一个实参），文档序。
pub fn index_terms(s: &EditSession) -> Vec<String> {
    let Some(idx) = s.document().fields_in(s.main_part()) else { return Vec::new() };
    idx.fields()
        .iter()
        .filter(|f| f.instr.keyword == Keyword::Xe)
        .filter_map(|f| f.instr.first_argument().map(str::to_string))
        .collect()
}

/// 这个落点之前同标签的 `SEQ` 字段数 + 1（TS `generateCaptionXml` 的编号来源）。
///
/// 「之前」按落点算：`After` / `End` 连锚点**整棵子树**一起算进去（题注插在它后面），
/// `Before` / `Start` 只算到锚点为止。
pub fn next_seq_number(s: &EditSession, label: &str, at: crate::edit::BlockAt) -> u32 {
    use crate::edit::BlockAt;
    let Some(idx) = s.document().fields_in(s.main_part()) else { return 1 };
    let dom = s.dom();
    let order: std::collections::HashMap<NodeId, usize> =
        dom.descendants(dom.root()).enumerate().map(|(i, n)| (n, i)).collect();
    let cutoff = match at {
        BlockAt::After(n) | BlockAt::End(n) => {
            order.get(&n).map_or(usize::MAX, |&i| i + dom.descendants(n).count())
        }
        BlockAt::Before(n) | BlockAt::Start(n) => order.get(&n).copied().unwrap_or(0),
    };
    let n = idx
        .fields()
        .iter()
        .filter(|f| f.instr.keyword == Keyword::Seq)
        .filter(|f| f.instr.first_argument() == Some(label))
        .filter(|f| order.get(&f.form.head()).is_some_and(|&i| i < cutoff))
        .count();
    n as u32 + 1
}

/// `\h`：给每个目录条目对应的标题段落补一个隐藏书签 `_Toc{9 位}`（已经有的不动），
/// 回填进 `entries[].bookmark`。TS 不做这一步（`docs/04` §8：我们更强）。
fn ensure_toc_bookmarks(s: &mut EditSession, entries: &mut [TocEntry]) -> Result<MutationResult> {
    let mut out = MutationResult::default();
    let opts = TocOptions::default();
    // 条目与段落的对应要重算一遍：`toc_entries` 只带回了文字
    let paras: Vec<NodeId> = {
        let doc = s.document();
        let r = Resolver::new(doc);
        doc.blocks()
            .filter_map(Block::as_text)
            .filter(|tb| level_of(tb, &r, &opts).is_some() && !entry_text(tb).is_empty())
            .map(|tb| tb.node)
            .collect()
    };
    let mut next = 100_000_000u32;
    for (e, para) in entries.iter_mut().zip(paras) {
        if e.bookmark.is_some() {
            continue;
        }
        let name = loop {
            let name = format!("_Toc{next:09}");
            next += 1;
            let taken = s
                .spans_of(s.main_part())?
                .live()
                .any(|sp| sp.kind.bookmark_name() == Some(name.as_str()));
            if !taken {
                break name;
            }
        };
        let len = s
            .text_block_in(None, para)
            .map(|tb| tb.inlines.iter().map(Inline::utf16_len).sum::<u32>())
            .unwrap_or(0);
        out.absorb(super::ops::add_bookmark(
            s,
            &name,
            InlinePos::new(para, 0),
            InlinePos::new(para, len),
        )?);
        e.bookmark = Some(name);
    }
    Ok(out)
}

/// 一批生成好的段落 XML → `NewBlock::Xml`。
fn parse_blocks(s: &mut EditSession, xml: Vec<String>) -> Result<Vec<NewBlock>> {
    let main = s.main_part();
    let flavor = s.flavor();
    let w_uri = crate::xml::NsId::W.uri(flavor).expect("w 有两族 URI");
    let dom = s
        .package_mut()
        .dom_mut(main)?
        .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
    // part 本来就把 `w` 绑在这个 URI 上就不用再声明一遍（`XML-14`：序列化按作用域补）
    let ctx = crate::package::ns_context::NamespaceContext::from_dom(dom, flavor);
    let decl = match ctx.prefix_for(crate::xml::NsId::W) {
        Some("w") => String::new(),
        _ => format!(r#" xmlns:w="{w_uri}""#),
    };
    xml.into_iter()
        .map(|x| {
            let x = x.replacen("<w:p>", &format!("<w:p{decl}>"), 1);
            let mut frags = crate::xml::parse_fragment(dom, &x).map_err(|e| {
                Error::edit(DiagCode::EditPlanInvalid, format!("生成的段落解析失败: {e}"))
            })?;
            frags
                .pop()
                .map(NewBlock::Xml)
                .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "生成的段落为空"))
        })
        .collect()
}

/// `NewBlock::Field` → 生成好的段落（`chart_ops::materialize` 调）。
pub(crate) fn materialize_field(s: &mut EditSession, f: NewBlockField) -> Result<Vec<NewBlock>> {
    let xml = match f {
        NewBlockField::Toc { opts, pages } => {
            let mut entries = toc_entries(s, &opts, pages.as_ref())?;
            if opts.hyperlinks && !opts.ts_shape() {
                ensure_toc_bookmarks(s, &mut entries)?;
            }
            toc_gen::generate(&entries, &opts)
        }
        NewBlockField::Index(opts) => index_gen::generate(&index_terms(s), &opts),
    };
    parse_blocks(s, xml)
}

/// `NewBlock::Caption` → 一段题注（编号 = 位置之前同标签的 `SEQ` 数 + 1）。
pub(crate) fn materialize_caption(
    s: &mut EditSession,
    label: &str,
    text: &str,
    at: Option<crate::edit::BlockAt>,
) -> Result<NewBlock> {
    let n = match at {
        Some(at) => next_seq_number(s, label, at),
        // 不知道落点（不经 `InsertBlock` 的路）：全文数一遍
        None => next_seq_number(s, label, crate::edit::BlockAt::End(s.dom().root())),
    };
    let xml = seq_gen::caption(label, n, text, false);
    Ok(parse_blocks(s, vec![xml])?.pop().expect("一段"))
}

/// `EDIT-03 RegenerateBlockField`：按生成器重算一个块字段的结果区。
///
/// 走 `UpdateBlockField` 那条既有机制换结果区，所以 `w:fldLock`、追踪、跨段字段的规则一条都不用
/// 重写。`\h` 模式会先给标题段落补书签——那会重建索引，所以字段用 begin 的 `NodeId` 重新认
/// （`FieldId` 是下标，可能已经变了）。
pub(crate) fn regenerate(
    s: &mut EditSession,
    field: FieldId,
    options: BlockFieldOptions,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let main = s.main_part();
    let (instr, begin) = s
        .document()
        .fields_in(main)
        .and_then(|i| i.get(field))
        .map(|f| (f.instr.clone(), f.form.head()))
        .ok_or_else(|| Error::edit(DiagCode::EditTargetMissing, "没有这个字段"))?;
    let plan = match options {
        BlockFieldOptions::Toc { mut opts, pages } => {
            opts.emit_field_structure = false;
            Plan::Toc(opts, pages)
        }
        BlockFieldOptions::Index(mut opts) => {
            opts.emit_field_structure = false;
            Plan::Index(opts)
        }
        BlockFieldOptions::Auto { pages } => match instr.keyword {
            Keyword::Toc => Plan::Toc(
                Box::new(TocOptions {
                    emit_field_structure: false,
                    ..TocOptions::from_instruction(&instr)
                }),
                pages,
            ),
            Keyword::Index => Plan::Index(Box::new(IndexOptions {
                emit_field_structure: false,
                ..IndexOptions::from_instruction(&instr)
            })),
            ref k => {
                return Err(super::ops::unsupported(&format!(
                    "{k:?} 没有生成器；能重算的是 TOC 与 INDEX"
                )));
            }
        },
    };
    let mut result = MutationResult::default();
    let blocks = match plan {
        Plan::Toc(opts, pages) => {
            let mut entries = toc_entries(s, &opts, pages.as_ref())?;
            if opts.hyperlinks && !opts.ts_shape() {
                result.absorb(ensure_toc_bookmarks(s, &mut entries)?);
            }
            parse_blocks(s, toc_gen::generate(&entries, &opts))?
        }
        Plan::Index(opts) => parse_blocks(s, index_gen::generate(&index_terms(s), &opts))?,
    };
    // 补书签重建过索引：按 begin 的节点重新认这个字段
    let field = s
        .document()
        .fields_in(main)
        .and_then(|i| i.fields().iter().find(|f| f.form.head() == begin))
        .map(|f| f.id)
        .ok_or_else(|| Error::edit(DiagCode::EditTargetMissing, "补书签之后找不到这个字段了"))?;
    result.absorb(super::ops::update_block_field(s, field, blocks, ctx)?);
    result.absorb(relocate_structure(s, begin)?);
    Ok(result)
}

/// `FLD-12` 的多段形态：begin + 指令 + separate 在**首段**开头、end 在**末段**末尾。
///
/// `UpdateBlockField` 只换结果区，结构 run 还留在原来那两段里；重算之后那两段常常就空了
/// （目录自己生成的字段，begin 就在第一条条目那一段）。把结构 run 搬进新的首 / 末段
/// （`move_within_part`，原字节保住），空掉的段落删掉。
fn relocate_structure(s: &mut EditSession, begin: NodeId) -> Result<MutationResult> {
    let part = s.main_part();
    let Some(f) = s
        .document()
        .fields_in(part)
        .and_then(|i| i.fields().iter().find(|f| f.form.head() == begin))
    else {
        return Ok(MutationResult::default());
    };
    let crate::span::FieldForm::Complex { begin, separate, end, instr_nodes, .. } = &f.form else {
        return Ok(MutationResult::default());
    };
    let (begin, end) = (*begin, *end);
    let structure: Vec<NodeId> =
        std::iter::once(begin).chain(instr_nodes.iter().copied()).chain(*separate).collect();
    let dom = s.dom();
    let para_of = |n: NodeId| {
        std::iter::once(n)
            .chain(dom.ancestors(n))
            .find(|&a| dom.is(a, crate::xml::QName::w(crate::xml::LocalName::P)))
    };
    let (Some(begin_para), Some(end_para)) = (para_of(begin), para_of(end)) else {
        return Ok(MutationResult::default());
    };
    if begin_para == end_para {
        return Ok(MutationResult::default());
    }
    // 新条目段落 = begin 段与 end 段之间的兄弟
    let Some(parent) = dom.parent(begin_para) else { return Ok(MutationResult::default()) };
    let between: Vec<NodeId> = dom
        .children(parent)
        .iter()
        .copied()
        .filter(|&c| dom.node(c).dirty != crate::xml::Dirty::Deleted)
        .skip_while(|&c| c != begin_para)
        .skip(1)
        .take_while(|&c| c != end_para)
        .collect();
    let (Some(&first), Some(&last)) = (between.first(), between.last()) else {
        return Ok(MutationResult::default());
    };
    let content = |p: NodeId| {
        dom.children(p)
            .iter()
            .copied()
            .filter(|&c| dom.node(c).dirty != crate::xml::Dirty::Deleted)
            .filter(|&c| dom.element(c).is_some())
            .filter(|&c| !dom.is(c, crate::xml::QName::w(crate::xml::LocalName::PPr)))
            .collect::<Vec<_>>()
    };
    // 结构 run 之外还有别的内容就别动：那一段不是纯结构段
    if content(begin_para).iter().any(|c| !structure.contains(c)) || content(end_para) != vec![end]
    {
        return Ok(MutationResult::default());
    }
    let mut plan = crate::edit::plan::MutationPlan::new(part);
    plan.structure_changed = true;
    let head = content(first).first().copied();
    for n in structure {
        plan.node_edits.push(crate::xml::NodeEdit::Move {
            node: n,
            parent: crate::xml::Target::Node(first),
            before: head,
        });
    }
    plan.node_edits.push(crate::xml::NodeEdit::Move {
        node: end,
        parent: crate::xml::Target::Node(last),
        before: None,
    });
    plan.node_edits.push(crate::xml::NodeEdit::Delete(begin_para));
    plan.node_edits.push(crate::xml::NodeEdit::Delete(end_para));
    s.commit_plan(plan)
}

enum Plan {
    Toc(Box<TocOptions>, Option<HashMap<NodeId, u32>>),
    Index(Box<IndexOptions>),
}
