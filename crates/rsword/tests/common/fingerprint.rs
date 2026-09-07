//! `ModelFingerprint`（`spec/18` 分层决策 3）：M7 的「等价」定义。
//!
//! 门 1 的三条 oracle、门 3 的 Word 对照与 `TEST-07` 共用它。指纹**忽略**：`NodeId` /
//! `RevisionId` / `w:rsid*` / `w14:paraId` / run 的切分位置（相邻同格式的 run 合并后再比，
//! 所以拆 run 不算差异——Word 拒绝一处 `rPrChange` 之后并不会把 run 合回去，
//! `fixtures/revisions/README.md`）。
//!
//! 每份文档算**两个视图**：
//!
//! | 视图 | 内容 | `*PrChange` | 段落标记 |
//! | --- | --- | --- | --- |
//! | `accept` | 含 `w:ins`、不含 `w:del` | 用当前值（去掉 `*Change`） | `w:del` 的标记 → 与下一段合并 |
//! | `reject` | 含 `w:del`、不含 `w:ins` | 用 `*Change` 里的旧值快照 | `w:ins` 的标记 → 与下一段合并 |
//!
//! 没有修订的文档两个视图相同。有了这两个视图，**在 `AcceptAll` / `RejectAll` 落地之前**就能
//! 验「追踪一遍 = 不追踪做一遍」（比 accept 视图）与「拒绝能回到原样」（比 reject 视图）。

#![allow(dead_code)]

use rsword::edit::EditSession;
use rsword::package::PartId;
use rsword::xml::{CanonOptions, Dirty, Dom, LocalName, NodeId, NsId, QName, canonical};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFingerprint {
    pub accept: String,
    pub reject: String,
}

impl ModelFingerprint {
    /// 两个视图的第一处差异（断言失败时打印）。
    pub fn diff(&self, other: &Self) -> Option<(&'static str, String, String)> {
        for (name, a, b) in
            [("accept", &self.accept, &other.accept), ("reject", &self.reject, &other.reject)]
        {
            if let Some((x, y)) = diff_str(a, b) {
                return Some((name, x, y));
            }
        }
        None
    }
}

/// 断言两份指纹相等，不等时打印第一处差异。
#[macro_export]
macro_rules! assert_fingerprint_eq {
    ($a:expr, $b:expr, $($msg:tt)*) => {{
        let (a, b) = (&$a, &$b);
        if let Some((view, x, y)) = a.diff(b) {
            panic!("{}\n  视图 {view}\n  左: {x}\n  右: {y}", format_args!($($msg)*));
        }
    }};
}

/// 两个字符串的第一处差异，各截一段上下文。
pub fn diff_str(a: &str, b: &str) -> Option<(String, String)> {
    if a == b {
        return None;
    }
    let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let start = a[..common].char_indices().rev().nth(60).map_or(0, |(i, _)| i);
    let cut = |s: &str| {
        let from = s.char_indices().map(|(i, _)| i).find(|&i| i >= start).unwrap_or(s.len());
        let end = s[from..].char_indices().nth(160).map_or(s.len(), |(i, _)| from + i);
        s[from..end].to_string()
    };
    Some((cut(a), cut(b)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Accept,
    Reject,
}

pub fn fingerprint(s: &EditSession) -> ModelFingerprint {
    ModelFingerprint { accept: render(s, View::Accept), reject: render(s, View::Reject) }
}

/// 参与指纹的 part，文档序（与 `Document.revisions` 的 part 顺序一致）。
fn parts(s: &EditSession) -> Vec<PartId> {
    let doc = s.document();
    let mut v = vec![doc.main_part];
    v.extend(doc.hf_parts.keys().copied());
    v.extend([doc.footnotes.part, doc.endnotes.part, doc.comments.part].into_iter().flatten());
    v.extend(doc.aux_flows.keys().copied());
    v
}

fn render(s: &EditSession, view: View) -> String {
    let mut out = String::new();
    for part in parts(s) {
        let Some(dom) = s.package().part(part).dom() else { continue };
        out.push_str("PART\n");
        let mut w = Walker { dom, view, out: &mut out, para: Para::default() };
        w.walk(dom.root());
        w.flush_para();
    }
    out
}

/// 累积中的段落（段落标记被本视图判为"已删"时不 flush，接着往下一段攒——那正是"接受合并"）。
#[derive(Default)]
struct Para {
    open: bool,
    ppr: String,
    text: String,
    /// `(UTF-16 长度, rPr 规范化)`，相邻相同的已合并。
    runs: Vec<(u32, String)>,
    /// `(种类, 名字 / id, 视图文本里的 UTF-16 偏移)`。
    marks: Vec<(String, String, u32)>,
    /// 字段指令（`w:instrText` / `w:delInstrText` 的文本，按视图过滤）。
    instr: Vec<String>,
}

struct Walker<'a> {
    dom: &'a Dom,
    view: View,
    out: &'a mut String,
    para: Para,
}

fn wq(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// `w:rsid*` / `w14:*` / `w15:*` 是版本噪音，不进指纹。
fn ignore_attr(dom: &Dom, _node: NodeId, attr: QName) -> bool {
    if matches!(attr.ns, NsId::W14 | NsId::W15) {
        return true;
    }
    attr.ns == NsId::W
        && matches!(
            attr.local,
            LocalName::RsidR
                | LocalName::RsidRDefault
                | LocalName::RsidP
                | LocalName::RsidRPr
                | LocalName::RsidDel
                | LocalName::RsidTr
                | LocalName::RsidSect
        )
        && {
            let _ = dom;
            true
        }
}

fn canon(dom: &Dom, n: NodeId) -> String {
    canonical(dom, n, &CanonOptions { ignore_attr: &ignore_attr })
}

fn live_children(dom: &Dom, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    dom.children(n).iter().copied().filter(|&c| dom.node(c).dirty != Dirty::Deleted)
}

fn child(dom: &Dom, n: NodeId, name: LocalName) -> Option<NodeId> {
    live_children(dom, n).find(|&c| dom.is(c, wq(name)))
}

fn is_change(local: LocalName) -> bool {
    matches!(
        local,
        LocalName::RPrChange
            | LocalName::PPrChange
            | LocalName::SectPrChange
            | LocalName::TblPrChange
            | LocalName::TblPrExChange
            | LocalName::TblGridChange
            | LocalName::TrPrChange
            | LocalName::TcPrChange
            | LocalName::NumberingChange
    )
}

fn is_mark_element(local: LocalName) -> bool {
    matches!(local, LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo)
}

/// 属性容器的规范化：跳过 `*Change`、跳过 `skip` 列出的、跳过段落标记的四个修订元素。
fn props_canon(dom: &Dom, container: NodeId, skip: &[LocalName]) -> String {
    let mut out = String::new();
    for c in live_children(dom, container) {
        let Some(q) = dom.name(c) else { continue };
        if q.ns == NsId::W
            && (is_change(q.local) || is_mark_element(q.local) || skip.contains(&q.local))
        {
            continue;
        }
        out.push_str(&canon(dom, c));
    }
    out
}

/// 视图里这个属性容器的有效内容：`reject` 且有 `*Change` → 用快照里的旧值。
fn props_in_view(
    dom: &Dom,
    container: Option<NodeId>,
    view: View,
    change: LocalName,
    inner: LocalName,
    skip: &[LocalName],
) -> String {
    let Some(container) = container else { return String::new() };
    if view == View::Reject
        && let Some(ch) = child(dom, container, change)
    {
        return match child(dom, ch, inner) {
            Some(old) => props_canon(dom, old, skip),
            // 空的 `*Change`（hostile `rev-change-empty`）：旧值就是"全默认"
            None => String::new(),
        };
    }
    props_canon(dom, container, skip)
}

impl Walker<'_> {
    /// 本视图是否要跳过整棵 `w:ins` / `w:del` 子树。
    fn skips(&self, local: LocalName) -> bool {
        match self.view {
            View::Accept => matches!(local, LocalName::Del | LocalName::MoveFrom),
            View::Reject => matches!(local, LocalName::Ins | LocalName::MoveTo),
        }
    }

    fn push_text(&mut self, t: &str) {
        self.para.text.push_str(t);
    }

    fn text_len(&self) -> u32 {
        self.para.text.encode_utf16().count() as u32
    }

    fn push_run(&mut self, len: u32, rpr: String) {
        if len == 0 {
            return;
        }
        match self.para.runs.last_mut() {
            Some((n, p)) if *p == rpr => *n += len,
            _ => self.para.runs.push((len, rpr)),
        }
    }

    fn flush_para(&mut self) {
        if !self.para.open {
            return;
        }
        let p = std::mem::take(&mut self.para);
        self.out.push_str("P|");
        self.out.push_str(&p.ppr);
        self.out.push_str("|T=");
        self.out.push_str(&p.text.replace('\n', "\\n"));
        self.out.push_str("|R=");
        for (n, r) in &p.runs {
            self.out.push_str(&format!("{n}:{r};"));
        }
        self.out.push_str("|M=");
        for (k, id, at) in &p.marks {
            self.out.push_str(&format!("{k}/{id}@{at};"));
        }
        self.out.push_str("|F=");
        for i in &p.instr {
            self.out.push_str(i);
            self.out.push(';');
        }
        self.out.push('\n');
    }

    fn walk(&mut self, node: NodeId) {
        if self.dom.node(node).dirty == Dirty::Deleted {
            return;
        }
        let Some(q) = self.dom.name(node) else { return };
        if q.ns == NsId::W {
            match q.local {
                _ if is_mark_element(q.local) => {
                    if self.skips(q.local) {
                        return;
                    }
                    self.descend(node);
                    return;
                }
                LocalName::P => return self.paragraph(node),
                LocalName::Tbl => {
                    self.flush_para();
                    self.out.push_str("TBL{\n");
                    self.descend(node);
                    self.flush_para();
                    self.out.push_str("}TBL\n");
                    return;
                }
                LocalName::Tr => {
                    // 整行插入 / 删除（`trPr/w:ins|w:del`）
                    if let Some(trpr) = child(self.dom, node, LocalName::TrPr) {
                        for m in [LocalName::Ins, LocalName::Del] {
                            if child(self.dom, trpr, m).is_some() && self.skips(m) {
                                return;
                            }
                        }
                    }
                    self.flush_para();
                    self.out.push_str("TR{\n");
                    self.descend(node);
                    self.flush_para();
                    self.out.push_str("}TR\n");
                    return;
                }
                LocalName::Tc => {
                    // 单元格插入 / 删除（`tcPr/w:cellIns|w:cellDel`）
                    if let Some(tcpr) = child(self.dom, node, LocalName::TcPr) {
                        let ins = child(self.dom, tcpr, LocalName::CellIns).is_some();
                        let del = child(self.dom, tcpr, LocalName::CellDel).is_some();
                        if (ins && self.view == View::Reject) || (del && self.view == View::Accept)
                        {
                            return;
                        }
                    }
                    self.flush_para();
                    let tcpr = child(self.dom, node, LocalName::TcPr);
                    let props = props_in_view(
                        self.dom,
                        tcpr,
                        self.view,
                        LocalName::TcPrChange,
                        LocalName::TcPr,
                        &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge],
                    );
                    self.out.push_str(&format!("TC{{{props}\n"));
                    self.descend(node);
                    self.flush_para();
                    self.out.push_str("}TC\n");
                    return;
                }
                LocalName::SectPr => {
                    self.flush_para();
                    let props = props_in_view(
                        self.dom,
                        Some(node),
                        self.view,
                        LocalName::SectPrChange,
                        LocalName::SectPr,
                        &[LocalName::HeaderReference, LocalName::FooterReference],
                    );
                    self.out.push_str(&format!("SECT|{props}\n"));
                    return;
                }
                LocalName::R => return self.run(node),
                // 属性容器不产生文本
                LocalName::PPr | LocalName::RPr | LocalName::TblPr | LocalName::TblGrid => return,
                _ if rsword::span::is_range_marker(q) => return self.marker(node, q),
                _ => {}
            }
        }
        self.descend(node);
    }

    fn descend(&mut self, node: NodeId) {
        for c in self.dom.children(node).to_vec() {
            self.walk(c);
        }
    }

    fn paragraph(&mut self, p: NodeId) {
        let dom = self.dom;
        let ppr = child(dom, p, LocalName::PPr);
        // 段落标记的 `rPr` 里的 `w:ins` / `w:del`：本视图判定这个标记还在不在
        let mark_rpr = ppr.and_then(|x| child(dom, x, LocalName::RPr));
        let mark_gone = mark_rpr.is_some_and(|r| {
            live_children(dom, r).any(|c| {
                dom.name(c).is_some_and(|q| {
                    q.ns == NsId::W && is_mark_element(q.local) && self.skips(q.local)
                })
            })
        });
        // 段落属性：`w:rPr`（段落标记的格式）不进指纹——合并 / 拆分时它归谁是 Word 的细节
        let props = props_in_view(
            dom,
            ppr,
            self.view,
            LocalName::PPrChange,
            LocalName::PPr,
            &[LocalName::RPr, LocalName::SectPr],
        );
        if !self.para.open {
            self.para.open = true;
            self.para.ppr = props;
        }
        for c in dom.children(p).to_vec() {
            self.walk(c);
        }
        // 段落级 `sectPr` 在 `walk` 里已经作为 `SECT` 输出（它是 `pPr` 的子节点，
        // 而 `pPr` 整体被跳过——这里补一条）
        if let Some(x) = ppr
            && let Some(sect) = child(dom, x, LocalName::SectPr)
        {
            let s = props_in_view(
                dom,
                Some(sect),
                self.view,
                LocalName::SectPrChange,
                LocalName::SectPr,
                &[LocalName::HeaderReference, LocalName::FooterReference],
            );
            self.para.text.push('\u{2029}');
            self.para.instr.push(format!("SECT:{s}"));
        }
        if !mark_gone {
            self.flush_para();
        }
    }
}

/// `w:r` 与范围标记等叶子的处理挂在 `walk` 的默认分支上。
impl Walker<'_> {
    fn run(&mut self, r: NodeId) {
        let dom = self.dom;
        let rpr = child(dom, r, LocalName::RPr);
        let props = props_in_view(dom, rpr, self.view, LocalName::RPrChange, LocalName::RPr, &[]);
        let before = self.text_len();
        for c in live_children(dom, r).collect::<Vec<_>>() {
            let Some(q) = dom.name(c) else { continue };
            if q.ns == NsId::M {
                self.push_text("\u{FFFC}");
                continue;
            }
            if q.ns != NsId::W {
                continue;
            }
            match q.local {
                LocalName::RPr => {}
                LocalName::T => {
                    let t = dom.text(c).unwrap_or_default().into_owned();
                    self.push_text(&t);
                }
                LocalName::DelText => {
                    if self.view == View::Reject {
                        let t = dom.text(c).unwrap_or_default().into_owned();
                        self.push_text(&t);
                    }
                }
                LocalName::InstrText | LocalName::DelInstrText => {
                    let keep = q.local == LocalName::InstrText || self.view == View::Reject;
                    if keep {
                        let t = dom.text(c).unwrap_or_default().into_owned();
                        self.para.instr.push(t);
                    }
                }
                LocalName::Tab => self.push_text("\t"),
                LocalName::Br | LocalName::Cr => self.push_text("\n"),
                LocalName::NoBreakHyphen => self.push_text("-"),
                LocalName::SoftHyphen => self.push_text("\u{00AD}"),
                LocalName::FldChar => {
                    let ty = dom
                        .attr_value(c, wq(LocalName::FldCharType))
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    self.para.instr.push(format!("fld:{ty}"));
                }
                LocalName::Sym
                | LocalName::Drawing
                | LocalName::Object
                | LocalName::Pict
                | LocalName::Ruby => self.push_text("\u{FFFC}"),
                LocalName::FootnoteReference | LocalName::EndnoteReference => {
                    self.push_text("\u{FFFC}")
                }
                _ => {}
            }
        }
        let len = self.text_len() - before;
        self.push_run(len, props);
    }

    fn marker(&mut self, n: NodeId, q: QName) {
        let dom = self.dom;
        let kind = format!("{:?}", q.local);
        let id = dom
            .attr_value(n, wq(LocalName::Name))
            .or_else(|| dom.attr_value(n, wq(LocalName::Id)))
            .map(|v| v.into_owned())
            .unwrap_or_default();
        let at = self.text_len();
        self.para.marks.push((kind, id, at));
    }
}
