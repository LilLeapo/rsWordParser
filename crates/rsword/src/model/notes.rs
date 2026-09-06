//! 批注与脚注 / 尾注条目（`MOD-10`，任务 2.6）。
//!
//! 一条批注要三个部件合起来才完整：`comments.xml` 给正文、作者与首字母，
//! `commentsExtended.xml` 给回复关系与"已解决"（按最后一段的 `w14:paraId` 关联），
//! `commentsIds.xml` 给 durableId。注释部件里带 `w:type` 的条目（`separator` /
//! `continuationSeparator`）是结构条目，不是正文——保存时要原样留着（`spec/13` 2.6）。
//!
//! 声明值（文字、格式、节点位置）与**内容块**（`Note.blocks` / `Comment.blocks`）都在这里：
//! 条目的内容与页眉页脚、正文同一个构建器（`docs/03` §6.7，任务 5.3）。`text` / `rich` 是 TS 形态的
//! 投影（`COMPAT-02` 的 `footnotes[].richParas`），与 `blocks` 并存——它们随 `compat_ts` 在 M9 一起删。

use std::collections::HashMap;

use crate::diag::Diagnostic;
use crate::model::aux::AuxFlows;
use crate::model::block::Block;
use crate::model::decl::Styles;
use crate::package::{PartId, Rels};
use crate::semantic::props::{RunProps, read_run_props};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

fn w(l: LocalName) -> QName {
    QName::new(NsId::W, l)
}

fn attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|v| v.into_owned())
}

fn flag(v: Option<String>) -> bool {
    matches!(v.as_deref(), Some("1" | "true" | "on"))
}

/// 一个 run 的文字与格式（注释条目的 `richParas` 用）。
#[derive(Debug, Clone, PartialEq)]
pub struct RichRun {
    pub node: NodeId,
    pub text: String,
    pub props: RunProps,
}

/// 一条批注。
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// `w:comment`。
    pub node: NodeId,
    pub id: String,
    pub author: Option<String>,
    pub initials: Option<String>,
    pub date: Option<String>,
    /// 各段文字以 `\n` 连接。
    pub text: String,
    /// **最后一段**的 `w14:paraId`（`commentsExtended` 按它关联）。
    pub para_id: Option<String>,
    /// 回复的父批注 id（由 `w15:paraIdParent` 反查）。
    pub parent_id: Option<String>,
    /// `w15:done`。
    pub done: bool,
    /// `commentsIds.xml` 的 `w16cid:durableId`。
    pub durable_id: Option<String>,
    /// 条目里的 `w:p`（保存时的手术式补丁用）。
    pub paragraphs: Vec<NodeId>,
    pub rich: Vec<Vec<RichRun>>,
    /// 条目内容，与正文同一构建器（`MOD-01`，任务 5.3）。
    pub blocks: Vec<Block>,
}

/// `comments.xml`（+ `commentsExtended.xml` / `commentsIds.xml`）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comments {
    pub part: Option<PartId>,
    pub extended_part: Option<PartId>,
    pub ids_part: Option<PartId>,
    pub items: Vec<Comment>,
    /// `comments.xml` 的三份索引（`SPAN-01`：每条批注是一个独立内容流）。part 缺失时 `None`。
    pub idx: Option<AuxFlows>,
}

impl Comments {
    /// `comments.xml` 缺失时是空集合（`part` 为 `None`，`AddComment` 据此建 part）。
    pub fn from_doms(
        comments: Option<(PartId, &Dom)>,
        extended: Option<(PartId, &Dom)>,
        ids: Option<(PartId, &Dom)>,
        rels: Option<&Rels>,
        styles: Option<&Styles>,
        diags: &mut Vec<Diagnostic>,
    ) -> Comments {
        let mut out = Comments {
            part: comments.map(|(p, _)| p),
            extended_part: extended.map(|(p, _)| p),
            ids_part: ids.map(|(p, _)| p),
            items: Vec::new(),
            idx: None,
        };
        if let Some((part, dom)) = comments {
            let root = dom.root();
            let idx = AuxFlows::build(part, dom, diags);
            for c in dom.semantic_children(root).filter(|&n| dom.is(n, w(LocalName::Comment))) {
                let mut item = read_comment(dom, c, diags);
                if let Some(rels) = rels {
                    item.blocks = idx.blocks_of(dom, rels, styles, c, diags);
                }
                out.items.push(item);
            }
            out.idx = Some(idx);
        }
        out.link_extended(extended.map(|(_, d)| d), ids.map(|(_, d)| d));
        out
    }

    /// `commentsExtended` 的 `done` / `paraIdParent` 与 `commentsIds` 的 durableId。
    fn link_extended(&mut self, extended: Option<&Dom>, ids: Option<&Dom>) {
        let by_para: HashMap<String, String> = self
            .items
            .iter()
            .filter_map(|c| c.para_id.clone().map(|p| (p, c.id.clone())))
            .collect();
        if let Some(dom) = extended {
            let mut done: HashMap<String, bool> = HashMap::new();
            let mut parent: HashMap<String, String> = HashMap::new();
            for e in dom.semantic_children(dom.root()) {
                if !dom.is(e, QName::new(NsId::W15, LocalName::CommentEx)) {
                    continue;
                }
                let Some(pid) = attr(dom, e, NsId::W15, LocalName::ParaId) else { continue };
                if flag(attr(dom, e, NsId::W15, LocalName::Done)) {
                    done.insert(pid.clone(), true);
                }
                if let Some(par) = attr(dom, e, NsId::W15, LocalName::ParaIdParent) {
                    parent.insert(pid, par);
                }
            }
            for c in &mut self.items {
                let Some(pid) = &c.para_id else { continue };
                c.done = done.get(pid).copied().unwrap_or(false);
                c.parent_id = parent.get(pid).and_then(|p| by_para.get(p)).cloned();
            }
        }
        if let Some(dom) = ids {
            let mut durable: HashMap<String, String> = HashMap::new();
            for e in dom.semantic_children(dom.root()) {
                if !dom.is(e, QName::new(NsId::W16Cid, LocalName::CommentId)) {
                    continue;
                }
                if let (Some(pid), Some(d)) = (
                    attr(dom, e, NsId::W16Cid, LocalName::ParaId),
                    attr(dom, e, NsId::W16Cid, LocalName::DurableId),
                ) {
                    durable.insert(pid, d);
                }
            }
            for c in &mut self.items {
                if let Some(pid) = &c.para_id {
                    c.durable_id = durable.get(pid).cloned();
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Comment> {
        self.items.iter().find(|c| c.id == id)
    }

    /// `EDIT-06`：批注 `w:id` 在文档内取最大值 + 1。
    pub fn next_id(&self) -> u32 {
        self.items.iter().filter_map(|c| c.id.trim().parse::<u32>().ok()).max().map_or(1, |m| m + 1)
    }
}

fn read_comment(dom: &Dom, node: NodeId, diags: &mut Vec<Diagnostic>) -> Comment {
    let paragraphs: Vec<NodeId> =
        dom.semantic_children(node).filter(|&n| dom.is(n, w(LocalName::P))).collect();
    let (text, rich) = entry_text(dom, &paragraphs, false, diags);
    Comment {
        node,
        id: attr(dom, node, NsId::W, LocalName::Id).unwrap_or_default(),
        author: attr(dom, node, NsId::W, LocalName::Author),
        initials: attr(dom, node, NsId::W, LocalName::Initials),
        date: attr(dom, node, NsId::W, LocalName::Date),
        text,
        para_id: paragraphs.last().and_then(|&p| attr(dom, p, NsId::W14, LocalName::ParaId)),
        parent_id: None,
        done: false,
        durable_id: None,
        paragraphs,
        rich,
        blocks: Vec::new(),
    }
}

/// 注释条目的种类（`w:type`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteKind {
    /// 正文条目（无 `w:type`）。
    Normal,
    Separator,
    ContinuationSeparator,
    ContinuationNotice,
    Other(String),
}

impl NoteKind {
    pub fn is_normal(&self) -> bool {
        *self == NoteKind::Normal
    }
}

/// 一条脚注 / 尾注。
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// `w:footnote` / `w:endnote`。
    pub node: NodeId,
    pub id: String,
    pub kind: NoteKind,
    /// 各段文字以 `\n` 连接；首段去前导空白（自引用标记后的间隔）。
    pub text: String,
    pub rich: Vec<Vec<RichRun>>,
    /// 条目里没有任何 `w:footnoteRef` / `w:endnoteRef` run。
    pub no_ref_mark: bool,
    /// 首段的 `w:pStyle`（真实 Word 的脚注段落带「脚注文本」样式；TS `footnotes[].styleId`）。
    pub style_id: Option<String>,
    pub paragraphs: Vec<NodeId>,
    /// 条目内容，与正文同一构建器（`MOD-01`，任务 5.3）。
    pub blocks: Vec<Block>,
}

/// `footnotes.xml` 或 `endnotes.xml`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    pub part: Option<PartId>,
    /// 全部条目，含 separator 一类结构条目（保存时要原样保留）。
    pub items: Vec<Note>,
    /// 该 part 的三份索引（`SPAN-01`：每个条目是一个独立内容流）。part 缺失时 `None`。
    pub idx: Option<AuxFlows>,
}

impl Notes {
    pub fn from_dom(
        part: Option<(PartId, &Dom)>,
        entry: LocalName,
        ref_mark: LocalName,
        rels: Option<&Rels>,
        styles: Option<&Styles>,
        diags: &mut Vec<Diagnostic>,
    ) -> Notes {
        let Some((id, dom)) = part else { return Notes::default() };
        let idx = AuxFlows::build(id, dom, diags);
        let entries: Vec<NodeId> =
            dom.semantic_children(dom.root()).filter(|&n| dom.is(n, w(entry))).collect();
        let mut items = Vec::with_capacity(entries.len());
        for n in entries {
            let mut item = read_note(dom, n, ref_mark, diags);
            if let Some(rels) = rels {
                item.blocks = idx.blocks_of(dom, rels, styles, n, diags);
            }
            items.push(item);
        }
        Notes { part: Some(id), items, idx: Some(idx) }
    }

    /// 正文条目（`separator` / `continuationSeparator` 不算）。
    pub fn normal(&self) -> impl Iterator<Item = &Note> + '_ {
        self.items.iter().filter(|n| n.kind.is_normal())
    }

    pub fn get(&self, id: &str) -> Option<&Note> {
        self.items.iter().find(|n| n.id == id)
    }

    /// `EDIT-06`：注释 id 取最大值 + 1（结构条目用 -1 / 0，一并计入）。
    pub fn next_id(&self) -> i64 {
        self.items.iter().filter_map(|n| n.id.trim().parse::<i64>().ok()).max().map_or(1, |m| m + 1)
    }
}

fn read_note(dom: &Dom, node: NodeId, ref_mark: LocalName, diags: &mut Vec<Diagnostic>) -> Note {
    // `w:type`（小写）是 `LocalName::Type`；`UType` 是 `[Content_Types].xml` 里大写的 `Type`
    let kind = match attr(dom, node, NsId::W, LocalName::Type).as_deref() {
        None => NoteKind::Normal,
        Some("separator") => NoteKind::Separator,
        Some("continuationSeparator") => NoteKind::ContinuationSeparator,
        Some("continuationNotice") => NoteKind::ContinuationNotice,
        Some(other) => NoteKind::Other(other.to_string()),
    };
    let paragraphs: Vec<NodeId> =
        dom.semantic_children(node).filter(|&n| dom.is(n, w(LocalName::P))).collect();
    let (text, rich) = entry_text(dom, &paragraphs, true, diags);
    let no_ref_mark = !dom.descendants(node).any(|n| dom.is(n, w(ref_mark)));
    let style_id = paragraphs.first().and_then(|&p| {
        let ppr = dom.semantic_children(p).find(|&c| dom.is(c, w(LocalName::PPr)))?;
        let st = dom.semantic_children(ppr).find(|&c| dom.is(c, w(LocalName::PStyle)))?;
        attr(dom, st, NsId::W, LocalName::Val)
    });
    Note {
        node,
        id: attr(dom, node, NsId::W, LocalName::Id).unwrap_or_default(),
        kind,
        text,
        rich,
        no_ref_mark,
        style_id,
        paragraphs,
        blocks: Vec::new(),
    }
}

/// 条目文字与 run 格式。
///
/// 每段把 `w:t` 拼起来，段间 `\n`。`notes` 为真时（脚注 / 尾注）跳过含自引用标记的 run，
/// 并把**首段的前导空白**吃掉——自引用标记与正文之间那个分隔空格在 Word 里不是内容，
/// TS 的 `text` 与 `richParas` 都不含它，整个 run 只有空白时连 run 一起丢。
fn entry_text(
    dom: &Dom,
    paragraphs: &[NodeId],
    notes: bool,
    diags: &mut Vec<Diagnostic>,
) -> (String, Vec<Vec<RichRun>>) {
    let mut rich: Vec<Vec<RichRun>> = Vec::new();
    for &p in paragraphs {
        let mut runs: Vec<RichRun> = Vec::new();
        // 段内所有 run（含 `w:ins` / `w:hyperlink` 一类包裹里的）
        for r in dom.descendants(p).filter(|&n| dom.is(n, w(LocalName::R))) {
            if notes && is_ref_mark_run(dom, r) {
                continue;
            }
            let mut text = String::new();
            let mut props = RunProps::default();
            for c in dom.semantic_children(r) {
                let Some(name) = dom.name(c) else { continue };
                if name == w(LocalName::RPr) {
                    props = read_run_props(dom, Some(c), diags);
                } else if name == w(LocalName::T)
                    && let Some(t) = dom.children(c).first().and_then(|&t| dom.text(t))
                {
                    text.push_str(&t);
                }
            }
            if text.is_empty() {
                continue;
            }
            runs.push(RichRun { node: r, text, props });
        }
        rich.push(runs);
    }
    if notes && let Some(first) = rich.first_mut() {
        while let Some(run) = first.first_mut() {
            let trimmed = run.text.trim_start().to_string();
            if trimmed.is_empty() {
                first.remove(0);
                continue;
            }
            run.text = trimmed;
            break;
        }
    }
    let text = rich
        .iter()
        .map(|line| line.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    (text, rich)
}

/// 含 `w:footnoteRef` / `w:endnoteRef` 的自引用标记 run。
fn is_ref_mark_run(dom: &Dom, run: NodeId) -> bool {
    dom.semantic_children(run)
        .any(|c| dom.is(c, w(LocalName::FootnoteRef)) || dom.is(c, w(LocalName::EndnoteRef)))
}
