//! 新内容描述（`docs/03` §8.2 的 `NewInline`）与其 `New` 子树生成。
//!
//! 文本里的控制字符折回段种类（`COMPAT-08`）：`\t` → `w:tab`，`\n` → `w:br`，`\u{0C}` → `w:br w:type="page"`，
//! `\u{0B}` → `w:br w:type="column"`，`\r` → `w:cr`；其余进入 `w:t`（`New` 节点序列化时一律带
//! `xml:space="preserve"`，`SAVE-03`）。

use crate::diag::{DiagCode, Diagnostic};
use crate::package::PartId;
use crate::xml::{LocalName, NewElement, NsId, QName};

/// 修订元数据；`id == None` 时按 `EDIT-06` 分配（文档内全部修订 `w:id` 的最大值 + 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRevision {
    pub id: Option<String>,
    pub author: String,
    pub date: Option<String>,
}

/// 超链接目标：已有关系 `r:id`，或文内书签 `w:anchor`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewLinkTarget {
    Rel(String),
    Anchor(String),
}

/// 一个新 run：`props` 是完整的 `w:rPr`（`None` = 无 `rPr`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub text: String,
    pub props: Option<NewElement>,
}

impl NewRun {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), props: None }
    }
}

/// 范围标记与批注引用（compat 侧重发，`SPAN` 索引在 M2 接管）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewMarker {
    BookmarkStart {
        id: String,
        name: String,
    },
    BookmarkEnd {
        id: String,
    },
    CommentRangeStart {
        id: String,
    },
    CommentRangeEnd {
        id: String,
    },
    /// `w:r/w:commentReference`。
    CommentReference {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewInline {
    Run(NewRun),
    Hyperlink {
        target: NewLinkTarget,
        tooltip: Option<String>,
        inlines: Vec<NewInline>,
    },
    /// `w:ins` 包裹。
    Ins {
        rev: NewRevision,
        inlines: Vec<NewInline>,
    },
    /// `w:del` 包裹：内部 run 的文本写成 `w:delText`。
    Del {
        rev: NewRevision,
        inlines: Vec<NewInline>,
    },
    Marker(NewMarker),
    /// 复杂字段（`FLD-12`）：begin / `w:instrText` / [separate] / 结果 / end 五组 run。
    ///
    /// `instr` 是指令原文（生成时 trim 后前后各补一个空格，与 Word 一致）；`separate == false`
    /// 时不发 separate 也不发结果（XE / TA 一类 `Marker` 策略字段就是这个形状）。
    Field {
        instr: String,
        result: Vec<NewInline>,
        separate: bool,
        /// begin 的 `w:fldChar` 上打 `w:dirty="true"`。
        dirty: bool,
        /// 结构 run 的 `w:rPr`（compat 路径不带，`InsertField` 带插入点的继承格式）。
        props: Option<NewElement>,
    },
    /// 任意内联片段（`m:oMath`、带 `w:ruby` / `w:drawing` 的 `w:r` …）。
    Xml(NewElement),
}

impl NewInline {
    /// 复杂字段的便捷构造（`separate` 与结果都有）。
    pub fn field(instr: impl Into<String>, result: Vec<NewInline>) -> Self {
        NewInline::Field { instr: instr.into(), result, separate: true, dirty: false, props: None }
    }

    /// 没有结果区的字段（XE / TA 一类）。
    pub fn marker_field(instr: impl Into<String>) -> Self {
        NewInline::Field {
            instr: instr.into(),
            result: Vec::new(),
            separate: false,
            dirty: false,
            props: None,
        }
    }
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// XML 1.0 允许的字符，加上折回 `w:br` 的 `\u{0B}` / `\u{0C}`。
pub fn is_allowed_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}')
        || ('\u{20}'..='\u{D7FF}').contains(&c)
        || ('\u{E000}'..='\u{FFFD}').contains(&c)
        || c >= '\u{10000}'
}

/// 剔除 XML 非法字符；剔除了任何字符时记一条 `EDIT_BAD_TEXT`。
pub fn sanitize_text(text: &str, part: PartId, diags: &mut Vec<Diagnostic>) -> String {
    if text.chars().all(is_allowed_char) {
        return text.to_string();
    }
    let removed = text.chars().filter(|c| !is_allowed_char(*c)).count();
    diags.push(Diagnostic::invariant_violation(
        part,
        None,
        DiagCode::EditBadText,
        format!("文本含 {removed} 个 XML 非法字符，已剔除"),
    ));
    text.chars().filter(|c| is_allowed_char(*c)).collect()
}

/// 文本是否含需要折回元素的控制字符（不能直接写进现有 `w:t`）。
pub fn has_control_chars(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}'))
}

fn text_element(deleted: bool, text: &str) -> NewElement {
    NewElement::new(w(if deleted { LocalName::DelText } else { LocalName::T }))
        .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
        .with_text(text)
}

/// 文本 → run 的子节点序列（不含 `rPr`）。
pub fn text_segments(text: &str, deleted: bool) -> Vec<NewElement> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<NewElement>| {
        if !buf.is_empty() {
            out.push(text_element(deleted, buf));
            buf.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\t' => {
                flush(&mut buf, &mut out);
                out.push(NewElement::new(w(LocalName::Tab)));
            }
            '\n' => {
                flush(&mut buf, &mut out);
                out.push(NewElement::new(w(LocalName::Br)));
            }
            '\u{0C}' => {
                flush(&mut buf, &mut out);
                out.push(NewElement::new(w(LocalName::Br)).with_attr(w(LocalName::Type), "page"));
            }
            '\u{0B}' => {
                flush(&mut buf, &mut out);
                out.push(NewElement::new(w(LocalName::Br)).with_attr(w(LocalName::Type), "column"));
            }
            '\r' => {
                flush(&mut buf, &mut out);
                out.push(NewElement::new(w(LocalName::Cr)));
            }
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// `w:r`：`rPr` + 文本段。
pub fn new_run(text: &str, props: Option<NewElement>, deleted: bool) -> NewElement {
    let mut r = NewElement::new(w(LocalName::R));
    if let Some(p) = props {
        r.push_child(p);
    }
    for seg in text_segments(text, deleted) {
        r.push_child(seg);
    }
    r
}

fn marker(m: &NewMarker) -> NewElement {
    match m {
        NewMarker::BookmarkStart { id, name } => NewElement::new(w(LocalName::BookmarkStart))
            .with_attr(w(LocalName::Id), id.clone())
            .with_attr(w(LocalName::Name), name.clone()),
        NewMarker::BookmarkEnd { id } => {
            NewElement::new(w(LocalName::BookmarkEnd)).with_attr(w(LocalName::Id), id.clone())
        }
        NewMarker::CommentRangeStart { id } => {
            NewElement::new(w(LocalName::CommentRangeStart)).with_attr(w(LocalName::Id), id.clone())
        }
        NewMarker::CommentRangeEnd { id } => {
            NewElement::new(w(LocalName::CommentRangeEnd)).with_attr(w(LocalName::Id), id.clone())
        }
        NewMarker::CommentReference { id } => NewElement::new(w(LocalName::R)).with_child(
            NewElement::new(w(LocalName::CommentReference)).with_attr(w(LocalName::Id), id.clone()),
        ),
    }
}

/// 生成器：负责修订 `w:id` 的连续分配（`EDIT-06`：起点由调用方按文档最大值 + 1 给出）。
pub struct Emitter {
    pub next_revision_id: u32,
}

impl Emitter {
    pub fn new(next_revision_id: u32) -> Self {
        Self { next_revision_id }
    }

    fn revision_attrs(&mut self, e: &mut NewElement, rev: &NewRevision) {
        let id = match &rev.id {
            Some(id) => id.clone(),
            None => {
                let id = self.next_revision_id;
                self.next_revision_id += 1;
                id.to_string()
            }
        };
        e.push_attr(w(LocalName::Id), id);
        e.push_attr(w(LocalName::Author), rev.author.clone());
        if let Some(d) = &rev.date {
            e.push_attr(w(LocalName::Date), d.clone());
        }
    }

    /// 一个 `NewInline` → 顶层 `New` 元素序列（`deleted`：位于 `w:del` 内，文本写 `w:delText`）。
    pub fn emit(&mut self, inline: &NewInline, deleted: bool, out: &mut Vec<NewElement>) {
        match inline {
            NewInline::Run(r) => out.push(new_run(&r.text, r.props.clone(), deleted)),
            NewInline::Hyperlink { target, tooltip, inlines } => {
                let mut h = NewElement::new(w(LocalName::Hyperlink));
                match target {
                    NewLinkTarget::Rel(rid) => {
                        h.push_attr(QName::new(NsId::R, LocalName::Id), rid.clone())
                    }
                    NewLinkTarget::Anchor(a) => h.push_attr(w(LocalName::Anchor), a.clone()),
                }
                if let Some(t) = tooltip {
                    h.push_attr(w(LocalName::Tooltip), t.clone());
                }
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, deleted, &mut kids);
                }
                for k in kids {
                    h.push_child(k);
                }
                out.push(h);
            }
            NewInline::Ins { rev, inlines } => {
                let mut e = NewElement::new(w(LocalName::Ins));
                self.revision_attrs(&mut e, rev);
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, deleted, &mut kids);
                }
                for k in kids {
                    e.push_child(k);
                }
                out.push(e);
            }
            NewInline::Del { rev, inlines } => {
                let mut e = NewElement::new(w(LocalName::Del));
                self.revision_attrs(&mut e, rev);
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, true, &mut kids);
                }
                for k in kids {
                    e.push_child(k);
                }
                out.push(e);
            }
            NewInline::Field { instr, result, separate, dirty, props } => {
                let fld = |kind: &str, dirty: bool| {
                    let mut e = NewElement::new(w(LocalName::FldChar))
                        .with_attr(w(LocalName::FldCharType), kind);
                    if dirty {
                        e.push_attr(w(LocalName::Dirty), "true");
                    }
                    e
                };
                let structural = |child: NewElement| {
                    let mut r = NewElement::new(w(LocalName::R));
                    if let Some(p) = props {
                        r.push_child(p.clone());
                    }
                    r.push_child(child);
                    r
                };
                out.push(structural(fld("begin", *dirty)));
                out.push(structural(
                    NewElement::new(w(LocalName::InstrText))
                        .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
                        .with_text(format!(" {} ", instr.trim())),
                ));
                if *separate {
                    out.push(structural(fld("separate", false)));
                    for i in result {
                        self.emit(i, deleted, out);
                    }
                }
                out.push(structural(fld("end", false)));
            }
            NewInline::Marker(m) => out.push(marker(m)),
            NewInline::Xml(e) => out.push(e.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::NewNode;

    #[test]
    fn edit_03_text_segments_fold_control_chars() {
        let segs = text_segments("a\tb\nc\u{0C}d\u{0B}e", false);
        let names: Vec<LocalName> = segs.iter().map(|e| e.name.local).collect();
        assert_eq!(
            names,
            [
                LocalName::T,
                LocalName::Tab,
                LocalName::T,
                LocalName::Br,
                LocalName::T,
                LocalName::Br,
                LocalName::T,
                LocalName::Br,
                LocalName::T
            ]
        );
        assert_eq!(segs[5].attrs, vec![(w(LocalName::Type), "page".to_string())]);
        assert_eq!(segs[7].attrs, vec![(w(LocalName::Type), "column".to_string())]);
        assert_eq!(segs[0].children, vec![NewNode::Text("a".into())]);
        let del = text_segments("x", true);
        assert_eq!(del[0].name.local, LocalName::DelText);
        let mut diags = Vec::new();
        assert_eq!(sanitize_text("a\u{0}b\u{1F}c", PartId(0), &mut diags), "abc");
        assert_eq!(diags.len(), 1);
    }
}
