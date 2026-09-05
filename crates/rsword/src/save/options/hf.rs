//! `SAVE-07` 的页眉页脚保存选项 → 5.5 的编辑操作（`spec/16` 任务 5.6b）。
//!
//! 内容本身（`Vec<NewBlock>`）由 `bind/compat_ts` 按 TS `headerFooterPartXml` 的规则算好——
//! `#` 顶替页码、`PAGE` / `NUMPAGES` 标记换字段、样式层的对齐这些都是 TS 的启发式，
//! 按分层决策留在适配器里。这里只管**落在哪个 part、怎么落**：
//!
//! | 情况 | 操作 |
//! | --- | --- |
//! | 这一节没声明该变体 | `SetHeaderFooter`（按 `SAVE-05` 新建 part 并插引用） |
//! | 声明了、part 里只有文本段落 | `SetHeaderFooter`（内容整体替换） |
//! | 声明了、part 里还有别的东西 | **外科合并**：`DeleteBlock` 掉文本段落 + `InsertBlock` 新内容 |
//!
//! 外科合并保留的是"不在文本段落模型里"的东西：表格、`w:sdt`、以及含 `w:drawing` / `w:pict` /
//! `w:object` 的段落（页眉里的 logo）。新内容整体落在**第一个文本段落**的位置。
//!
//! 水印是独立一项：`SetWatermark` 本身就是"只动含 `v:textpath` 的段落"，所以选项里同时给了内容
//! 与水印时先内容后水印（水印段落最终落在最前，同 Word）。选项**没有**给水印时原水印段落
//! 一个字节都不动——TS 会拿解析出来的水印文字重新生成那棵 VML 子树（`docs/04` §8）。

use crate::edit::{BlockAt, BlockPos, EditOp, EditSession, NewBlock};
use crate::model::{Document, HfKind, HfVariant, SectionOwner};
use crate::package::PartId;
use crate::xml::{Dirty, Dom, LocalName, NodeId, QName};

use super::SaveOptions;

/// 六个槽（kind × variant）一张表：一次展开 [`HfSlots`] 的字段、迭代与 TS 键名的对应。
///
/// 同一组六元组在结构体字段、迭代、TS 键解析三处出现，宏一次说清（`spec/16` 的宏计划）。
///
/// ```ignore
/// let mut slots = HfSlots::default();
/// *slots.by_ts_key("headerFirst").unwrap() = Some(blocks);
/// for (kind, variant, blocks) in slots.iter() { … }
/// ```
macro_rules! hf_slots {
    ($( $field:ident : $kind:ident / $variant:ident = $key:literal ),+ $(,)?) => {
        /// 六个页眉页脚槽的内容（`None` = 这一项不动）。
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct HfSlots {
            $(
                #[doc = concat!("TS `SaveOptions.", $key, "`")]
                pub $field: Option<Vec<NewBlock>>,
            )+
        }

        impl HfSlots {
            /// 有内容的槽，按声明顺序（default 在 first / even 之前，与 TS 的调用顺序一致）。
            pub fn iter(&self) -> impl Iterator<Item = (HfKind, HfVariant, &[NewBlock])> {
                [$( (
                    $crate::model::HfKind::$kind,
                    $crate::model::HfVariant::$variant,
                    self.$field.as_deref(),
                ), )+]
                    .into_iter()
                    .filter_map(|(k, v, b)| b.map(|b| (k, v, b)))
            }

            /// TS 键名 → 对应的槽。
            pub fn by_ts_key(&mut self, key: &str) -> Option<&mut Option<Vec<NewBlock>>> {
                match key {
                    $( $key => Some(&mut self.$field), )+
                    _ => None,
                }
            }

            pub fn is_empty(&self) -> bool {
                $( self.$field.is_none() && )+ true
            }
        }
    };
}

hf_slots! {
    header: Header / Default = "header",
    footer: Footer / Default = "footer",
    header_first: Header / First = "headerFirst",
    footer_first: Footer / First = "footerFirst",
    header_even: Header / Even = "headerEven",
    footer_even: Footer / Even = "footerEven",
}

/// 某一节某个变体的内容（TS `sectionHf[]`）。`sect` 由适配器从 `lastBlockIndex` 解析出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHfSave {
    pub sect: NodeId,
    pub kind: HfKind,
    pub variant: HfVariant,
    pub blocks: Vec<NewBlock>,
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// 管这六个槽的那一节：最后一节，且它得是 body 级的 `w:sectPr`（TS 的 trailing hidden sectPr）。
fn trailing_section(doc: &Document) -> Option<NodeId> {
    let last = doc.sections.last()?;
    if last.owner != SectionOwner::Body {
        return None;
    }
    last.node
}

/// 这一节这个变体声明的 part（声明了但 part 不在包里也算没有，同 TS 的 `relTargets` 查不到）。
fn declared_part(doc: &Document, sect: NodeId, kind: HfKind, variant: HfVariant) -> Option<PartId> {
    let info = doc.sections.iter().find(|s| s.node == Some(sect))?;
    doc.hf_by_rel.get(info.hf_ref(kind, variant)?).copied()
}

/// 文本段落：`w:p` 且不含 `w:drawing` / `w:pict` / `w:object`。含 `v:textpath` 的水印段落
/// 因此也算"别的东西"，内容合并时留在原位（它归 `SetWatermark` 管）。
fn is_text_para(dom: &Dom, node: NodeId) -> bool {
    dom.is(node, w(LocalName::P))
        && !dom.semantic_descendants(node).any(|n| {
            dom.is(n, w(LocalName::Drawing))
                || dom.is(n, w(LocalName::Pict))
                || dom.is(n, w(LocalName::Object))
        })
}

/// 一个 part 里的活元素子节点。
fn element_children(dom: &Dom, root: NodeId) -> Vec<NodeId> {
    dom.children(root)
        .iter()
        .copied()
        .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
        .collect()
}

/// `w:sectPr` 的第一个活元素子节点是不是页眉页脚引用（TS `hfAllSections` 的跳过条件：
/// 这一节自己带引用就整节不碰，哪怕带的是另一种引用）。
fn starts_with_reference(dom: &Dom, sect: NodeId) -> bool {
    dom.semantic_children(sect).find(|&n| dom.node(n).dirty != Dirty::Deleted).is_some_and(|n| {
        dom.is(n, w(LocalName::HeaderReference)) || dom.is(n, w(LocalName::FooterReference))
    })
}

/// 一个槽落成操作，并说明这一步是不是**新建** part（`hfAllSections` 只传播新建的）。
fn slot_ops(
    s: &EditSession,
    sect: NodeId,
    kind: HfKind,
    variant: HfVariant,
    blocks: &[NewBlock],
) -> (Vec<EditOp>, bool) {
    let doc = s.document();
    let full = |content: Vec<NewBlock>| EditOp::SetHeaderFooter { sect, kind, variant, content };
    let Some(part) = declared_part(doc, sect, kind, variant) else {
        return (vec![full(blocks.to_vec())], true);
    };
    let Ok(dom) = s.dom_in(Some(part)) else {
        return (vec![full(blocks.to_vec())], false);
    };
    let children = element_children(dom, dom.root());
    // 全是文本段落 → 整体替换（`SetHeaderFooter` 自己做，少一堆 DeleteBlock）
    if children.iter().all(|&c| is_text_para(dom, c)) {
        return (vec![full(blocks.to_vec())], false);
    }
    // 外科合并：删掉文本段落，新内容整体落在第一个文本段落的位置
    let texts: Vec<NodeId> = children.iter().copied().filter(|&c| is_text_para(dom, c)).collect();
    let mut ops = Vec::new();
    let at = match texts.first() {
        Some(&first) => BlockAt::Before(first),
        None => match children.first() {
            Some(&first) => BlockAt::Before(first),
            None => BlockAt::End(dom.root()),
        },
    };
    for block in blocks.iter().cloned() {
        ops.push(EditOp::InsertBlock { at: BlockPos::in_part(part, at), block });
    }
    for node in texts {
        ops.push(EditOp::DeleteBlock { part: Some(part), node });
    }
    (ops, false)
}

/// `SaveOptions` 的页眉页脚部分是不是什么都没要求。
pub(super) fn is_empty(opts: &SaveOptions) -> bool {
    opts.hf.is_empty()
        && opts.watermark.is_none()
        && opts.section_hf.is_empty()
        && !opts.hf_all_sections
}

/// 第一轮：六个槽 + `sectionHf` + 水印。返回操作与"新建了哪些变体"（第二轮 `hfAllSections` 用）。
pub(super) fn content_ops(
    s: &EditSession,
    opts: &SaveOptions,
) -> (Vec<EditOp>, Vec<(HfKind, HfVariant)>) {
    let mut ops = Vec::new();
    let mut created = Vec::new();
    if let Some(sect) = trailing_section(s.document()) {
        for (kind, variant, blocks) in opts.hf.iter() {
            let (mut slot, is_new) = slot_ops(s, sect, kind, variant, blocks);
            ops.append(&mut slot);
            if is_new {
                created.push((kind, variant));
            }
        }
        // 水印在内容之后：`SetWatermark` 把水印段落插在最前
        if let Some(text) = &opts.watermark {
            ops.push(EditOp::SetWatermark { sect, text: text.clone() });
        }
    }
    for e in &opts.section_hf {
        let (mut slot, _) = slot_ops(s, e.sect, e.kind, e.variant, &e.blocks);
        ops.append(&mut slot);
    }
    (ops, created)
}

/// 第二轮：`hfAllSections` 把**新建**的 part 挂到每个自己不带引用的 `w:sectPr` 上。
///
/// 只挂新建的（同 TS）：已有 part 的引用本来就在它该在的节上，向别节传播会把那些节的页眉改掉。
pub(super) fn link_ops(
    s: &EditSession,
    opts: &SaveOptions,
    created: &[(HfKind, HfVariant)],
) -> Vec<EditOp> {
    if !opts.hf_all_sections || created.is_empty() {
        return Vec::new();
    }
    let doc = s.document();
    let Some(trailing) = trailing_section(doc) else { return Vec::new() };
    let dom = s.dom();
    let parts: Vec<(HfKind, HfVariant, PartId)> = created
        .iter()
        .filter_map(|&(k, v)| declared_part(doc, trailing, k, v).map(|p| (k, v, p)))
        .collect();
    let mut ops = Vec::new();
    for info in &doc.sections {
        let Some(sect) = info.node else { continue };
        if starts_with_reference(dom, sect) {
            continue;
        }
        for &(kind, variant, part) in &parts {
            ops.push(EditOp::LinkHeaderFooter { sect, kind, variant, part });
        }
    }
    ops
}
