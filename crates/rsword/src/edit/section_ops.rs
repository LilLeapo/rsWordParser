//! 节与页眉页脚的编辑操作（`EDIT-03`、`SAVE-05`、`SAVE-07`，`spec/16` 任务 5.5）。
//!
//! 五个操作，都走 `plan → validate → commit` 的同一条路（`EDIT-05` 的事务边界在
//! [`EditSession::apply`]），没有旁路：
//!
//! | 操作 | 改什么 |
//! | --- | --- |
//! | [`set_section_props`] | 主 part 的 `w:sectPr`（属性表合并，`PROP-06`） |
//! | [`set_header_footer`] | 页眉页脚 part 的内容（整体替换），或按 `SAVE-05` **新建** part |
//! | [`link_header_footer`] | 主 part `sectPr` 里的一条引用（挂到已有 part） |
//! | [`set_watermark`] | default 页眉里的 VML 水印段落 |
//! | [`set_page_color`] | 主 part 的 `w:background` |
//!
//! **新建 part 的语义**（与 Word / TS 的 `sectionHf` 一致）：这一节自己声明了该变体就改写它引用的
//! part——共享这个 part 的前面各节跟着一起变（Word 的"同前"）；没声明（含从上一节继承）就新建一个
//! part 并把引用插进**这一节**的 `sectPr`，这一节因此独立，前面的节不受影响。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::{HfKind, HfVariant};
use crate::package::{PartFlavor, PartId, RelType};
use crate::semantic::props::{
    NewElement, NodeEdit, SectionPropsPatch, Target, plan_apply_section_props_at,
};
use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

use super::NewBlock;
use super::plan::{MutationPlan, MutationResult};
use super::session::{CT_FOOTER, CT_HEADER, EditSession};

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// 目标是主 part 里活着的 `w:sectPr`。
fn require_sect_pr(dom: &Dom, sect: NodeId) -> Result<()> {
    if (sect.0 as usize) < dom.node_count()
        && dom.node(sect).dirty != Dirty::Deleted
        && dom.is(sect, w(LocalName::SectPr))
    {
        Ok(())
    } else {
        Err(Error::edit(DiagCode::EditBadPosition, format!("节点 {} 不是活的 w:sectPr", sect.0)))
    }
}

/// `EDIT-03 SetSectionProps`。
pub(super) fn set_section_props(
    s: &mut EditSession,
    sect: NodeId,
    patch: &SectionPropsPatch,
) -> Result<MutationResult> {
    let dom = s.dom();
    require_sect_pr(dom, sect)?;
    let mut plan = MutationPlan::new(s.main_part());
    plan.touch(sect);
    plan_apply_section_props_at(
        dom,
        Target::Node(sect),
        Some(sect),
        None,
        patch,
        s.flavor(),
        &mut plan.node_edits,
    );
    s.commit_plan(plan)
}

/// 一个节里某个变体的引用元素（`w:headerReference` / `w:footerReference`）。
fn reference_of(dom: &Dom, sect: NodeId, kind: HfKind, variant: HfVariant) -> Option<NodeId> {
    let elem = match kind {
        HfKind::Header => LocalName::HeaderReference,
        HfKind::Footer => LocalName::FooterReference,
    };
    dom.semantic_children(sect)
        .filter(|&n| dom.is(n, w(elem)) && dom.node(n).dirty != Dirty::Deleted)
        .find(|&n| {
            // `w:type` 缺失与非 schema 的 `odd` 都算 default（`RES-10`）
            let kind = dom.attr_value(n, w(LocalName::Type));
            let v = match kind.as_deref() {
                Some("first") => HfVariant::First,
                Some("even") => HfVariant::Even,
                _ => HfVariant::Default,
            };
            v == variant
        })
}

/// 引用要插在哪儿：`sectPr` 里第一个不是引用的子元素之前（`PROP-05` 的第 0 格）。
fn reference_site(dom: &Dom, sect: NodeId) -> Option<NodeId> {
    dom.semantic_children(sect).filter(|&n| dom.node(n).dirty != Dirty::Deleted).find(|&n| {
        !dom.is(n, w(LocalName::HeaderReference)) && !dom.is(n, w(LocalName::FooterReference))
    })
}

/// 新引用元素。
fn reference_element(kind: HfKind, variant: HfVariant, rid: &str) -> NewElement {
    let elem = match kind {
        HfKind::Header => LocalName::HeaderReference,
        HfKind::Footer => LocalName::FooterReference,
    };
    let mut e = NewElement::new(w(elem));
    e.push_attr(w(LocalName::Type), variant.as_str().to_string());
    e.push_attr(QName::new(NsId::R, LocalName::Id), rid.to_string());
    e
}

/// `LinkHeaderFooter`：把一个**已有** part 的引用挂到这一节（TS 的 `hfAllSections`）。
/// 该节已经声明了这个变体就什么都不做（引用已在，`MutationResult` 为空）。
pub(super) fn link_header_footer(
    s: &mut EditSession,
    sect: NodeId,
    kind: HfKind,
    variant: HfVariant,
    part: PartId,
) -> Result<MutationResult> {
    let main = s.main_part();
    let rid = s.relationship_id(main, part).ok_or_else(|| {
        Error::edit(DiagCode::EditPlanInvalid, "目标 part 与主 part 之间没有关系")
    })?;
    let dom = s.dom();
    require_sect_pr(dom, sect)?;
    let mut plan = MutationPlan::new(main);
    plan.touch(sect);
    if reference_of(dom, sect, kind, variant).is_none() {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(sect),
            before: reference_site(dom, sect),
            node: reference_element(kind, variant, &rid),
        });
    }
    s.commit_plan(plan)
}

/// `SetHeaderFooter`：这一节这个变体的内容整体替换；没声明就新建 part 并插引用。
pub(super) fn set_header_footer(
    s: &mut EditSession,
    sect: NodeId,
    kind: HfKind,
    variant: HfVariant,
    content: Vec<NewBlock>,
) -> Result<MutationResult> {
    require_sect_pr(s.dom(), sect)?;
    let part = ensure_hf_part(s, sect, kind, variant)?;
    replace_part_blocks(s, part, content)
}

/// 这一节这个变体的 part：自己声明了就用它，否则按 `SAVE-05` 新建并把引用插进这一节。
fn ensure_hf_part(
    s: &mut EditSession,
    sect: NodeId,
    kind: HfKind,
    variant: HfVariant,
) -> Result<PartId> {
    let dom = s.dom();
    if let Some(node) = reference_of(dom, sect, kind, variant)
        && let Some(rid) = dom.attr_value(node, QName::new(NsId::R, LocalName::Id))
        && let Some(part) = s.document().hf_by_rel.get(rid.as_ref()).copied()
    {
        return Ok(part);
    }
    // 新建：`word/header{N}.xml`，N 取第一个空闲号
    let (rel, ct, base, root) = match kind {
        HfKind::Header => (RelType::Header, CT_HEADER, "header", "hdr"),
        HfKind::Footer => (RelType::Footer, CT_FOOTER, "footer", "ftr"),
    };
    let mut n = 1usize;
    let uri = loop {
        let uri = format!("word/{base}{n}.xml");
        if s.package().find(&crate::package::PartUri::from_entry_name(&uri)).is_none() {
            break uri;
        }
        n += 1;
    };
    let flavor = s.flavor();
    let w_uri = NsId::W.uri(flavor).expect("w 有两族 URI");
    let r_uri = NsId::R.uri(flavor).expect("r 有两族 URI");
    // 空 part：一个空段落（Word 期待页眉至少有一段）
    let xml = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:{root} xmlns:w="{w}" xmlns:r="{r}"><w:p/></w:{root}>"#
        ),
        root = root,
        w = w_uri,
        r = r_uri
    );
    let main = s.main_part();
    let (part, rid) = s.add_part(main, rel, &uri, ct, &xml)?;
    // 引用插进这一节的 `sectPr`（`PROP-05`：引用组在最前）
    let dom = s.dom();
    let mut plan = MutationPlan::new(main);
    plan.touch(sect);
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(sect),
        before: reference_site(dom, sect),
        node: reference_element(kind, variant, &rid),
    });
    s.commit_plan(plan)?;
    s.rebuild()?;
    Ok(part)
}

/// 一个 part 的内容整体替换：原有块全 `Deleted`，新块按 `NewBlock` 生成。
fn replace_part_blocks(
    s: &mut EditSession,
    part: PartId,
    content: Vec<NewBlock>,
) -> Result<MutationResult> {
    let dom = s.dom_in(Some(part))?;
    let root = dom.root();
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    for c in dom.children(root).iter().copied() {
        if dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some() {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
    }
    for block in content {
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(root),
            before: None,
            node: super::ops::new_block_element(dom, block),
        });
    }
    s.commit_plan(plan)
}

/// `SetPageColor`：`w:background` 是 `w:document` 的第一个子元素（在 `w:body` 之前）。
pub(super) fn set_page_color(s: &mut EditSession, color: Option<String>) -> Result<MutationResult> {
    let dom = s.dom();
    let root = dom.root();
    let existing = dom
        .semantic_children(root)
        .find(|&n| dom.is(n, w(LocalName::Background)) && dom.node(n).dirty != Dirty::Deleted);
    let mut plan = MutationPlan::new(s.main_part());
    match (color, existing) {
        (None, Some(n)) => plan.node_edits.push(NodeEdit::Delete(n)),
        (None, None) => {}
        (Some(c), Some(n)) => plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(n),
            name: w(LocalName::Color),
            value: c,
        }),
        (Some(c), None) => {
            let mut e = NewElement::new(w(LocalName::Background));
            e.push_attr(w(LocalName::Color), c);
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: dom.semantic_children(root).next(),
                node: e,
            });
        }
    }
    s.commit_plan(plan)
}

/// `SetWatermark`：default 页眉里的文字水印（Word 的水印就是页眉里的一个 VML 形状）。
///
/// Strict 包拒绝：VML 不在 Strict 里，DrawingML 水印不在 M5（`docs/03` §14）。
pub(super) fn set_watermark(
    s: &mut EditSession,
    sect: NodeId,
    text: Option<String>,
) -> Result<MutationResult> {
    if text.is_some() && s.flavor() == PartFlavor::Strict {
        return Err(Error::edit(
            DiagCode::EditUnsupported,
            "Strict 包不能写 VML 水印（DrawingML 水印不在 M5）",
        ));
    }
    require_sect_pr(s.dom(), sect)?;
    // 删水印时页眉不存在就什么都不用做
    let existing = {
        let dom = s.dom();
        reference_of(dom, sect, HfKind::Header, HfVariant::Default)
            .and_then(|n| dom.attr_value(n, QName::new(NsId::R, LocalName::Id)))
            .and_then(|rid| s.document().hf_by_rel.get(rid.as_ref()).copied())
    };
    let part = match (existing, &text) {
        (Some(p), _) => p,
        (None, None) => return Ok(MutationResult::default()),
        (None, Some(_)) => ensure_hf_part(s, sect, HfKind::Header, HfVariant::Default)?,
    };
    // 先把只读的活干完（要删哪些段落、插在哪儿），再动可变借用（片段解析要 `&mut Dom`）
    let (root, doomed, before) = {
        let dom = s.dom_in(Some(part))?;
        let root = dom.root();
        let doomed: Vec<NodeId> = dom
            .children(root)
            .iter()
            .copied()
            .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.is(c, w(LocalName::P)))
            .filter(|&c| {
                dom.semantic_descendants(c)
                    .any(|n| dom.is(n, QName::new(NsId::V, LocalName::Textpath)))
            })
            .collect();
        let before = dom.children(root).iter().copied().find(|&c| dom.element(c).is_some());
        (root, doomed, before)
    };
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    // 原有的水印段落（含 `v:textpath` 的 `w:p`）先删掉
    plan.node_edits.extend(doomed.into_iter().map(NodeEdit::Delete));
    if let Some(t) = text {
        // 水印段落放在最前（Word 的位置）；那棵 VML 子树用片段解析，不手拼字符串
        let xml = watermark_paragraph_xml(&t);
        let node = s.new_element_from_xml(part, &xml)?;
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before, node });
    }
    s.commit_plan(plan)
}

/// Word 生成的斜向灰字水印段落（`v:shapetype` 136 = 文字沿路径）。照抄 TS
/// `watermarkParagraphXml`：形状 id / `o:spid` / 样式都按 Word 的写法，编辑器与 Word 都认。
fn watermark_paragraph_xml(text: &str) -> String {
    let escaped =
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    format!(
        concat!(
            r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
            r#" xmlns:v="urn:schemas-microsoft-com:vml""#,
            r#" xmlns:o="urn:schemas-microsoft-com:office:office""#,
            r#" xmlns:w10="urn:schemas-microsoft-com:office:word">"#,
            r#"<w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:pict>"#,
            r#"<v:shapetype id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800""#,
            r#" path="m@7,l@8,m@5,21600l@6,21600e">"#,
            r#"<v:formulas>"#,
            r#"<v:f eqn="sum #0 0 10800"/><v:f eqn="prod #0 2 1"/><v:f eqn="sum 21600 0 @1"/>"#,
            r#"<v:f eqn="sum 0 0 @2"/><v:f eqn="sum 21600 0 @3"/><v:f eqn="if @0 @3 0"/>"#,
            r#"<v:f eqn="if @0 21600 @1"/><v:f eqn="if @0 0 @2"/><v:f eqn="if @0 @4 21600"/>"#,
            r#"<v:f eqn="mid @5 @6"/><v:f eqn="mid @8 @5"/><v:f eqn="mid @7 @8"/>"#,
            r#"<v:f eqn="mid @6 @7"/><v:f eqn="sum @6 0 @5"/>"#,
            r#"</v:formulas>"#,
            r#"<v:path textpathok="t" o:connecttype="custom""#,
            r#" o:connectlocs="@9,0;@10,10800;@11,21600;@12,10800""#,
            r#" o:connectangles="270,180,90,0"/>"#,
            r#"<v:textpath on="t" fitshape="t"/>"#,
            r##"<v:handles><v:h position="#0,bottomRight" xrange="6629,14971"/></v:handles>"##,
            r#"<o:lock v:ext="edit" text="t" shapetype="t"/>"#,
            r#"</v:shapetype>"#,
            r##"<v:shape id="PowerPlusWaterMarkObject1" o:spid="_x0000_s2049" type="#_x0000_t136""##,
            r#" style="position:absolute;left:0;text-align:left;margin-left:0;margin-top:0;"#,
            r#"width:412.4pt;height:247.45pt;rotation:315;z-index:-251656192;"#,
            r#"mso-position-horizontal:center;mso-position-horizontal-relative:margin;"#,
            r#"mso-position-vertical:center;mso-position-vertical-relative:margin""#,
            r#" o:allowincell="f" fillcolor="silver" stroked="f">"#,
            r#"<v:fill opacity=".5"/>"#,
            r#"<v:textpath style="font-family:&quot;DengXian&quot;;font-size:1pt" string="{}"/>"#,
            r#"</v:shape></w:pict></w:r></w:p>"#
        ),
        escaped
    )
}
