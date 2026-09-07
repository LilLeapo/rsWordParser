//! `FieldSpan` 的构建与索引（`FLD-01`–`FLD-04`、`FLD-13`）。
//!
//! 复杂字段用栈把 `w:fldChar` 的 begin / separate / end 配对，`w:fldSimple` 进出元素时开合；
//! 两者都产出 [`FieldSpan`]。字段**正确嵌套**（与范围不同），所以栈就够，不需要文档序比较。
//!
//! 索引是 DOM 的**投影**：`FieldSpan` 里的每一条事实都能从节点重新读出来，所以编辑之后重建
//! 即可，不像 `Anchor` 那样必须增量维护（`docs/03` §5.2 / §5.4）。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::package::PartId;
use crate::span::{FlowId, FlowMap, is_flow_root};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

use super::FieldId;
use super::form::{FormData, read_form_data};
use super::instr::{FieldPolicy, Instruction, Keyword, NESTED_PLACEHOLDER, parse as parse_instr};

/// 字段的两种形式（`FLD-01`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldForm {
    /// 复杂字段：三个含 `w:fldChar` 的 `w:r`（`separate` 可缺省）。
    Complex {
        begin: NodeId,
        separate: Option<NodeId>,
        end: NodeId,
        /// begin 与 separate（或 end）之间的 run。
        instr_nodes: Vec<NodeId>,
        /// separate 与 end 之间的 run（跨段时横跨多个段落）。
        result_nodes: Vec<NodeId>,
    },
    /// `w:fldSimple[@w:instr]`，子节点即结果。
    Simple { node: NodeId, result_nodes: Vec<NodeId> },
}

impl FieldForm {
    /// 字段的起始节点（复杂字段是 begin run，简单字段是 `w:fldSimple` 自身）。
    pub fn head(&self) -> NodeId {
        match self {
            Self::Complex { begin, .. } => *begin,
            Self::Simple { node, .. } => *node,
        }
    }

    /// 字段的结束节点。
    pub fn tail(&self) -> NodeId {
        match self {
            Self::Complex { end, .. } => *end,
            Self::Simple { node, .. } => *node,
        }
    }

    pub fn result_nodes(&self) -> &[NodeId] {
        match self {
            Self::Complex { result_nodes, .. } | Self::Simple { result_nodes, .. } => result_nodes,
        }
    }

    /// 结构 run（begin / separate / end）；简单字段没有。
    pub fn structure_nodes(&self) -> Vec<NodeId> {
        match self {
            Self::Complex { begin, separate, end, .. } => {
                let mut v = vec![*begin];
                v.extend(*separate);
                v.push(*end);
                v
            }
            Self::Simple { .. } => Vec::new(),
        }
    }

    pub fn is_complex(&self) -> bool {
        matches!(self, Self::Complex { .. })
    }
}

/// 一个字段（`docs/03` §5.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpan {
    pub id: FieldId,
    pub part: PartId,
    pub flow: FlowId,
    pub form: FieldForm,
    /// 语义视图；保存真相是 `form` 里的节点（`FLD-05`）。
    pub instr: Instruction,
    /// 指令区或结果区内的子字段。
    pub nested: Vec<FieldId>,
    pub parent: Option<FieldId>,
    pub policy: FieldPolicy,
    /// `w:fldChar/@w:fldLock`：`UpdateBlockField` 拒绝（`FLD-07`）。
    pub lock: bool,
    /// `w:fldChar/@w:dirty`：Word 打开时会重算。
    pub dirty_flag: bool,
    /// begin run 里的 `w:ffData`（表单域定义，`FLD-10`）。
    pub ff_data: Option<NodeId>,
    /// 指令里有 `w:delInstrText`：指令被修订删除（`MOD-09`）。
    pub instr_deleted: bool,
    /// begin 与 end 不在同一个 `w:p` 里（`FLD-06` 覆盖规则：一律 `Block`）。
    pub cross_paragraph: bool,
}

impl FieldSpan {
    pub fn keyword(&self) -> &Keyword {
        &self.instr.keyword
    }

    /// 原子形态（坐标流里占 1 个 `U+FFFC`，`FLD-14`）。
    pub fn is_atomic(&self) -> bool {
        matches!(
            self.policy,
            FieldPolicy::Marker
                | FieldPolicy::Atom
                | FieldPolicy::Form
                | FieldPolicy::Picture
                | FieldPolicy::Object
                | FieldPolicy::Unknown
        )
    }

    /// 透明形态：结果 run 直接出现在 inlines 里（`FLD-07` 的 `Link`）。
    pub fn is_transparent(&self) -> bool {
        self.policy == FieldPolicy::Link
    }

    /// 块形态：结果段落只读（`FLD-08`）。
    pub fn is_block(&self) -> bool {
        self.policy == FieldPolicy::Block
    }
}

/// 一个 part 的字段索引（`FLD-02`）。
#[derive(Debug, Clone, PartialEq)]
pub struct FieldIndex {
    part: PartId,
    /// `FieldId` 即下标；顺序是 end 的文档序（嵌套字段排在父字段之前）。
    fields: Vec<FieldSpan>,
    /// 属于字段的节点 → 最内层字段。
    by_node: HashMap<NodeId, FieldId>,
    diagnostics: Vec<Diagnostic>,
}

impl FieldIndex {
    /// `FLD-02`：逐内容流、按文档序扫描 run 与 `w:fldSimple`。
    pub fn build(dom: &Dom) -> FieldIndex {
        Builder::new(dom).run()
    }

    pub fn part(&self) -> PartId {
        self.part
    }

    pub fn fields(&self) -> &[FieldSpan] {
        &self.fields
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn get(&self, id: FieldId) -> Option<&FieldSpan> {
        self.fields.get(id.0 as usize)
    }

    /// 该节点属于哪个（最内层）字段。
    pub fn field_of(&self, node: NodeId) -> Option<&FieldSpan> {
        self.by_node.get(&node).and_then(|&id| self.get(id))
    }

    /// 顶层字段（没有父字段的）。
    pub fn roots(&self) -> impl Iterator<Item = &FieldSpan> + '_ {
        self.fields.iter().filter(|f| f.parent.is_none())
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// 解析期缺陷的计数（按代码）。`FLD-13` 用它区分"输入本来如此"与"编辑造成"：
    /// 保存前重建索引，某个代码的条数变多就是引擎干的。
    pub fn defect_counts(&self) -> HashMap<DiagCode, usize> {
        let mut m = HashMap::new();
        for d in &self.diagnostics {
            *m.entry(d.code).or_insert(0) += 1;
        }
        m
    }

    /// 一个字段占用的全部节点：结构 run、指令 run、结果节点，连同嵌套字段的（递归）。
    ///
    /// `FLD-07` 的"删除原子字段 = 删 begin..end"用它；嵌套字段的 run 记在嵌套字段自己名下，
    /// 所以必须递归，否则会剩下半个内层字段。
    pub fn all_nodes(&self, id: FieldId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let Some(f) = self.get(id) else { continue };
            out.extend(f.form.structure_nodes());
            if let FieldForm::Complex { instr_nodes, .. } = &f.form {
                out.extend(instr_nodes);
            }
            if let FieldForm::Simple { node, .. } = &f.form {
                out.push(*node);
                continue; // `w:fldSimple` 的子节点随它一起删
            }
            out.extend(f.form.result_nodes());
            stack.extend(f.nested.iter().copied());
        }
        out.sort_by_key(|n| n.0);
        out.dedup();
        out
    }

    /// `FLD-08`：`Block` 策略字段覆盖的段落 → 字段 id。
    ///
    /// 头段（begin 所在段落）、尾段（end 所在段落）与其间的所有段落（含表格单元格里的）都算，
    /// 所以按**文档序**取区间，而不是只看 `result_nodes` 落在哪些段落——中间的空段落也要保护。
    /// 嵌套的 `Block` 字段以最外层为准（先标记的胜出）。
    pub fn block_result_paragraphs(&self, dom: &Dom) -> HashMap<NodeId, FieldId> {
        let mut out = HashMap::new();
        if !self.fields.iter().any(|f| f.is_block()) {
            return out;
        }
        // 一次前序遍历取全部段落的文档序
        let paras: Vec<NodeId> =
            dom.descendants(dom.root()).filter(|&n| dom.is(n, w(LocalName::P))).collect();
        let pos: HashMap<NodeId, usize> = paras.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let para_of = |node: NodeId| -> Option<usize> {
            std::iter::once(node).chain(dom.ancestors(node)).find_map(|n| pos.get(&n).copied())
        };
        // 外层字段先标记：`fields` 按 end 的文档序排，嵌套的排在前面，所以倒着来
        for f in self.fields.iter().rev().filter(|f| f.is_block()) {
            let (Some(a), Some(b)) = (para_of(f.form.head()), para_of(f.form.tail())) else {
                continue;
            };
            for &p in &paras[a.min(b)..=a.max(b)] {
                out.insert(p, f.id);
            }
        }
        out
    }

    /// 该 run 是字段的结构 run（`fldChar`）还是指令 run：删除范围覆盖它时要连整个字段一起删。
    pub fn is_structure_run(&self, node: NodeId) -> bool {
        self.field_of(node).is_some_and(|f| match &f.form {
            FieldForm::Complex { begin, separate, end, instr_nodes, .. } => {
                node == *begin
                    || node == *end
                    || Some(node) == *separate
                    || instr_nodes.contains(&node)
            }
            FieldForm::Simple { .. } => false,
        })
    }
}

// ---- 构建 ----

/// 栈上未闭合的字段。
struct Open {
    begin: NodeId,
    /// `w:fldSimple` 的字段没有结构 run。
    simple: bool,
    separate: Option<NodeId>,
    instr_raw: String,
    /// `w:delInstrText` 的文本（`instr_raw` 为空时才当有效指令用，见 `instr_text_of`）。
    instr_raw_deleted: String,
    instr_nodes: Vec<NodeId>,
    result_nodes: Vec<NodeId>,
    nested: Vec<FieldId>,
    /// 指令区里的嵌套字段，按占位符顺序。
    nested_in_instr: Vec<FieldId>,
    instr_deleted: bool,
    lock: bool,
    dirty_flag: bool,
    ff_data: Option<NodeId>,
    flow: FlowId,
}

impl Open {
    /// 还在指令区（`separate` 没出现）。
    fn in_instr(&self) -> bool {
        !self.simple && self.separate.is_none()
    }
}

enum Ev {
    Enter(NodeId),
    Leave(NodeId),
}

struct Builder<'d> {
    dom: &'d Dom,
    part: PartId,
    fields: Vec<FieldSpan>,
    by_node: HashMap<NodeId, FieldId>,
    diagnostics: Vec<Diagnostic>,
    stack: Vec<Open>,
}

impl<'d> Builder<'d> {
    fn new(dom: &'d Dom) -> Self {
        Self {
            dom,
            part: dom.part(),
            fields: Vec::new(),
            by_node: HashMap::new(),
            diagnostics: Vec::new(),
            stack: Vec::new(),
        }
    }

    fn run(mut self) -> FieldIndex {
        let flows = FlowMap::build(self.dom);
        for i in 0..flows.flow_count() {
            let flow = FlowId(i as u32);
            self.walk_flow(flow, flows.root_of(flow));
        }
        FieldIndex {
            part: self.part,
            fields: self.fields,
            by_node: self.by_node,
            diagnostics: self.diagnostics,
        }
    }

    /// 一个内容流的遍历。字段禁止跨流（`FLD-02` 第 7 条）：流结束时栈上剩下的都是未闭合字段。
    fn walk_flow(&mut self, flow: FlowId, root: NodeId) {
        let mut work = vec![Ev::Enter(root)];
        while let Some(ev) = work.pop() {
            match ev {
                Ev::Enter(n) => {
                    if n != root && self.dom.name(n).is_some_and(is_flow_root) {
                        continue; // 独立流，由外层循环单独遍历
                    }
                    if self.dom.is(n, w(LocalName::R)) {
                        self.on_run(flow, n);
                        continue; // run 内部没有字段结构（嵌套文本框是独立流）
                    }
                    if self.dom.is(n, w(LocalName::FldSimple)) {
                        self.open_simple(flow, n);
                    }
                    work.push(Ev::Leave(n));
                    let start = work.len();
                    work.extend(self.dom.semantic_children(n).map(Ev::Enter));
                    work[start..].reverse();
                }
                Ev::Leave(n) => {
                    if self.dom.is(n, w(LocalName::FldSimple))
                        && self.stack.last().is_some_and(|o| o.simple && o.begin == n)
                    {
                        self.close(n);
                    }
                }
            }
        }
        // `FLD-02` 第 6 条：流结束仍未闭合 → 记诊断，不产出字段（begin run 当普通内容，保存原字节）
        while let Some(open) = self.stack.pop() {
            self.diag(
                open.begin,
                DiagCode::FldUnclosed,
                format!("字段 \"{}\" 在内容流结束时仍未闭合", open.instr_raw.trim()),
            );
        }
    }

    /// `FLD-02` 第 1–4 条。
    fn on_run(&mut self, flow: FlowId, run: NodeId) {
        let fld_char =
            self.dom.semantic_children(run).find(|&c| self.dom.is(c, w(LocalName::FldChar)));
        let Some(fc) = fld_char else {
            return self.on_content_run(run);
        };
        match self.dom.attr_value(fc, w(LocalName::FldCharType)).as_deref() {
            Some("begin") => self.open_complex(flow, run, fc),
            Some("separate") => match self.stack.last_mut() {
                Some(open) if open.separate.is_none() => open.separate = Some(run),
                // 重复的 separate：第一个才是结果区的起点，后面的当结果内容
                Some(_) => self.on_content_run(run),
                None => self.diag(
                    run,
                    DiagCode::FldStraySeparate,
                    "fldChar separate 没有对应的 begin".to_string(),
                ),
            },
            Some("end") => {
                if self.stack.is_empty() {
                    self.diag(
                        run,
                        DiagCode::FldStrayEnd,
                        "fldChar end 没有对应的 begin".to_string(),
                    );
                } else {
                    self.close(run);
                }
            }
            // 认不出的 fldCharType：当普通 run
            _ => self.on_content_run(run),
        }
    }

    fn open_complex(&mut self, flow: FlowId, run: NodeId, fld_char: NodeId) {
        let attr = |l: LocalName| self.dom.attr_value(fld_char, w(l)).map(|v| v.into_owned());
        self.stack.push(Open {
            begin: run,
            simple: false,
            separate: None,
            instr_raw: String::new(),
            instr_raw_deleted: String::new(),
            instr_nodes: Vec::new(),
            result_nodes: Vec::new(),
            nested: Vec::new(),
            nested_in_instr: Vec::new(),
            instr_deleted: false,
            lock: on_off(attr(LocalName::FldLock).as_deref()),
            dirty_flag: on_off(attr(LocalName::Dirty).as_deref()),
            ff_data: self
                .dom
                .semantic_children(fld_char)
                .find(|&c| self.dom.is(c, w(LocalName::FfData))),
            flow,
        });
    }

    /// `FLD-02` 第 5 条：`w:fldSimple` 直接开一个字段，其子 run 归入结果。
    fn open_simple(&mut self, flow: FlowId, node: NodeId) {
        let instr = self
            .dom
            .attr_value(node, w(LocalName::Instr))
            .map(|v| v.into_owned())
            .unwrap_or_default();
        self.stack.push(Open {
            begin: node,
            simple: true,
            separate: None,
            instr_raw: instr,
            instr_raw_deleted: String::new(),
            instr_nodes: Vec::new(),
            result_nodes: Vec::new(),
            nested: Vec::new(),
            nested_in_instr: Vec::new(),
            instr_deleted: false,
            lock: false,
            dirty_flag: false,
            ff_data: None,
            flow,
        });
    }

    /// 非 fldChar 的 run：按 `separate` 是否已出现归入指令区或结果区，并拼接指令文本（`FLD-03`）。
    fn on_content_run(&mut self, run: NodeId) {
        let Some(open) = self.stack.last() else { return };
        if open.in_instr() {
            let (text, del_text) = self.instr_text_of(run);
            let open = self.stack.last_mut().expect("just checked");
            open.instr_raw.push_str(&text);
            open.instr_deleted |= !del_text.is_empty();
            open.instr_raw_deleted.push_str(&del_text);
            open.instr_nodes.push(run);
        } else {
            self.stack.last_mut().expect("just checked").result_nodes.push(run);
        }
    }

    /// run 里 `w:instrText` 的文本（`FLD-03`：视同 `xml:space="preserve"`，不 trim）。
    ///
    /// 返回 `(w:instrText 的文本, w:delInstrText 的文本)`。
    ///
    /// **两者分开**（任务 7.2b）：追踪着改字段指令时，旧指令进 `w:del` 并改名成
    /// `w:delInstrText`、新指令进 `w:ins`，两段都在同一个字段里。拼在一起会让字段读成
    /// "旧指令 + 新指令"。有效指令因此**只取活的那部分**；整条指令都被删掉（没有任何
    /// `w:instrText`）时才退回删除的文本——那时它仍是这个字段现在的指令，拒绝修订才会变。
    /// 登记在 `docs/04` §8。
    fn instr_text_of(&self, run: NodeId) -> (String, String) {
        let (mut live, mut deleted) = (String::new(), String::new());
        for c in self.dom.semantic_children(run) {
            let is_del = self.dom.is(c, w(LocalName::DelInstrText));
            if !is_del && !self.dom.is(c, w(LocalName::InstrText)) {
                continue;
            }
            let out = if is_del { &mut deleted } else { &mut live };
            for t in self.dom.semantic_children(c) {
                if let Some(text) = self.dom.text(t) {
                    out.push_str(&text);
                }
            }
        }
        (live, deleted)
    }

    /// 闭合栈顶字段（`tail` 是 end run 或 `w:fldSimple` 自身）。
    fn close(&mut self, tail: NodeId) {
        let Some(open) = self.stack.pop() else { return };
        let id = FieldId(self.fields.len() as u32);
        // 指令整条被追踪删除（一个 `w:instrText` 都没有）：那段旧文本仍是这个字段现在的指令
        let raw = if open.instr_raw.trim().is_empty() && !open.instr_raw_deleted.is_empty() {
            open.instr_raw_deleted.clone()
        } else {
            open.instr_raw.clone()
        };
        let instr = parse_instr(&raw, &open.nested_in_instr);
        let form = if open.simple {
            FieldForm::Simple { node: open.begin, result_nodes: open.result_nodes }
        } else {
            FieldForm::Complex {
                begin: open.begin,
                separate: open.separate,
                end: tail,
                instr_nodes: open.instr_nodes,
                result_nodes: open.result_nodes,
            }
        };
        let cross_paragraph = self.paragraph_of(form.head()) != self.paragraph_of(form.tail());
        let policy = self.policy(&instr.keyword, cross_paragraph, open.ff_data);
        let span = FieldSpan {
            id,
            part: self.part,
            flow: open.flow,
            form,
            instr,
            nested: open.nested.clone(),
            parent: None,
            policy,
            lock: open.lock,
            dirty_flag: open.dirty_flag,
            ff_data: open.ff_data,
            instr_deleted: open.instr_deleted,
            cross_paragraph,
        };
        for &child in &span.nested {
            self.fields[child.0 as usize].parent = Some(id);
        }
        self.register_nodes(&span, id);
        self.fields.push(span);
        // 弹出的字段成为新栈顶的嵌套（`FLD-02` 第 3 条）
        if let Some(parent) = self.stack.last_mut() {
            parent.nested.push(id);
            if parent.in_instr() {
                // 指令区里的嵌套：在指令文本里留一个占位符（`FLD-03`）
                parent.instr_raw.push(NESTED_PLACEHOLDER);
                parent.nested_in_instr.push(id);
            }
        }
    }

    /// `FLD-06` 的两条覆盖规则：跨段一律 `Block`；FORMCHECKBOX 没有 `w:ffData/w:checkBox` 降为 `Unknown`。
    fn policy(
        &self,
        keyword: &Keyword,
        cross_paragraph: bool,
        ff_data: Option<NodeId>,
    ) -> FieldPolicy {
        if cross_paragraph {
            return FieldPolicy::Block;
        }
        let policy = keyword.policy();
        if *keyword == Keyword::FormCheckBox
            && !matches!(read_form_data(self.dom, ff_data), Some(FormData::CheckBox { .. }))
        {
            return FieldPolicy::Unknown;
        }
        policy
    }

    /// 结构 run、指令 run 与结果 run 都记进倒排表；最内层的字段胜出。
    fn register_nodes(&mut self, span: &FieldSpan, id: FieldId) {
        let mut nodes = span.form.structure_nodes();
        if let FieldForm::Complex { instr_nodes, .. } = &span.form {
            nodes.extend(instr_nodes);
        }
        nodes.extend(span.form.result_nodes());
        if let FieldForm::Simple { node, .. } = &span.form {
            nodes.push(*node);
        }
        for n in nodes {
            self.by_node.entry(n).or_insert(id);
        }
    }

    fn paragraph_of(&self, node: NodeId) -> Option<NodeId> {
        std::iter::once(node)
            .chain(self.dom.ancestors(node))
            .find(|&n| self.dom.is(n, w(LocalName::P)))
    }

    fn diag(&mut self, node: NodeId, code: DiagCode, message: String) {
        let range = self.dom.node(node).lex.as_ref().map(|l| l.range.clone());
        self.diagnostics.push(Diagnostic::pre_existing(self.part, range, code, message));
    }
}

fn w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}

/// `w:fldLock` / `w:dirty` 的 `ST_OnOff`：属性存在且不是 `0` / `false` / `off` 即为真。
fn on_off(v: Option<&str>) -> bool {
    match v {
        None => false,
        Some(s) => !matches!(s.trim(), "0" | "false" | "off"),
    }
}
