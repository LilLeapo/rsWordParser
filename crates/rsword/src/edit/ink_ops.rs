//! 墨迹的写侧（`SAVE-07 inks`、`EDIT-03` / `EDIT-06`，`spec/17` 任务 6.8）。
//!
//! `inks` 是**权威列表**（与 `comments` 同语义）：`EditSession::remove_inks` 删掉主 part 里全部墨迹 run
//! （它们的媒体与关系随保存时的资源回收消失，6.7），再对每条 [`InkSave`] 调 `EditSession::insert_ink`
//! ——按 TS `anchoredInkRunXml` 的模板把一条浮动图片 run 追加在段落**全部内容之后**。墨迹的媒体**不去重**
//! （每条一个 part，TS 同；两笔画出同一张 PNG 几乎不可能，去重只会让 `r:embed` 与 TS 分叉）。

use crate::diag::{DiagCode, Diagnostic};
use crate::edit::EditSession;
use crate::edit::media_ops::{NS_R, NS_WP, next_doc_pr_id, prefix_or_decl, px_to_emu};
use crate::edit::plan::{MutationPlan, MutationResult};
use crate::error::{Error, Result};
use crate::model::EMU_PER_PX;
use crate::model::ink::INK_NAME_PREFIX;
use crate::package::ns_context::NamespaceContext;
use crate::xml::plan::{NodeEdit, Target};
use crate::xml::{LocalName, NodeId, NsId, QName, parse_fragment};

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// 一条要写进文档的墨迹（TS `NewInkImage` 去掉 `blockIndex`）。
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewInk {
    /// PNG 字节。
    pub png: Vec<u8>,
    pub width_px: f64,
    pub height_px: f64,
    /// 相对文字栏左边 / 段落顶部的偏移（px，可为负）。
    pub offset_x_px: f64,
    pub offset_y_px: f64,
    /// 编辑器的笔迹载荷，写进 `wp:docPr/@descr`；`None` 或空 → 不写。
    pub payload: Option<String>,
}

/// `SaveOptions.inks` 的一条：锚点段落 + 墨迹。
#[derive(Debug, Clone, PartialEq)]
pub struct InkSave {
    /// 锚点（`w:p`）。不是段落（表格 / sdt 外壳）→ 跳过 + 诊断，不分配媒体与关系（TS 同）。
    pub para: NodeId,
    pub ink: NewInk,
}

fn attr_escaped(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    crate::xml::entities::escape_attr(s, b'"', &mut out);
    String::from_utf8(out).expect("escape_attr 只输出 UTF-8")
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// `EditOp::RemoveInks`：删掉主 part 里全部墨迹 run（`Document.inks`）。
    pub(crate) fn remove_inks(&mut self) -> Result<MutationResult> {
        let inks: Vec<(NodeId, NodeId)> =
            self.document().inks.iter().map(|i| (i.para, i.run)).collect();
        if inks.is_empty() {
            return Ok(MutationResult::default());
        }
        let mut plan = MutationPlan::new(self.main_part());
        for (para, run) in inks {
            plan.node_edits.push(NodeEdit::Delete(run));
            plan.touch(para);
        }
        self.commit_plan(plan)
    }

    /// `EditOp::InsertInk`：在 `para` 的全部内容之后追加一条墨迹 run（TS `anchoredInkRunXml`）。
    pub(crate) fn insert_ink(&mut self, para: NodeId, ink: &NewInk) -> Result<MutationResult> {
        let main = self.main_part();
        let dom = self.dom();
        if (para.0 as usize) >= dom.node_count() {
            return Err(Error::edit(DiagCode::EditBadPosition, "墨迹的锚点节点不存在"));
        }
        if !dom.is(para, QName::w(LocalName::P)) {
            // TS：`^<w:p[\s/>]` 不匹配（表格 / sdt 外壳）就跳过；先判再分配，不留孤儿媒体与关系
            let diag = Diagnostic::pre_existing(
                main,
                dom.node(para).lex.as_ref().map(|l| l.range.clone()),
                DiagCode::EditBadPosition,
                "墨迹的锚点不是段落，这条墨迹已跳过",
            );
            self.push_diagnostic(diag);
            return Ok(MutationResult::default());
        }
        let rid = self.add_media_with(ink.png.clone(), "image/png", false)?;
        let flavor = self.flavor();
        let dom = self.package_mut().dom_mut(main)?.expect("main part parsed");
        let id = next_doc_pr_id(dom);
        let ctx = NamespaceContext::from_dom(dom, flavor);
        let (wp, wp_decl) = prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
        let (r, r_decl) = prefix_or_decl(&ctx, NsId::R, "r", NS_R);
        let (cx, cy) = (px_to_emu(ink.width_px), px_to_emu(ink.height_px));
        let x = (ink.offset_x_px * EMU_PER_PX).round() as i64;
        let y = (ink.offset_y_px * EMU_PER_PX).round() as i64;
        let name = format!("{INK_NAME_PREFIX} {id}");
        let descr = ink
            .payload
            .as_deref()
            .filter(|p| !p.is_empty())
            .map_or(String::new(), |p| format!(r#" descr="{}""#, attr_escaped(p)));
        let xml = format!(
            concat!(
                r#"<w:r><w:drawing{wp_decl}{r_decl}><{wp}:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" "#,
                r#"relativeHeight="{rh}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
                r#"<{wp}:simplePos x="0" y="0"/>"#,
                r#"<{wp}:positionH relativeFrom="column"><{wp}:posOffset>{x}</{wp}:posOffset></{wp}:positionH>"#,
                r#"<{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>{y}</{wp}:posOffset></{wp}:positionV>"#,
                r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="0" t="0" r="0" b="0"/><{wp}:wrapNone/>"#,
                r#"<{wp}:docPr id="{id}" name="{name}"{descr}/><{wp}:cNvGraphicFramePr/>"#,
                r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}"><pic:pic xmlns:pic="{pic}">"#,
                r#"<pic:nvPicPr><pic:cNvPr id="{id}" name="{name}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip {r}:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic></{wp}:anchor></w:drawing></w:r>"#
            ),
            wp_decl = wp_decl,
            r_decl = r_decl,
            wp = wp,
            rh = crate::edit::media_ops::Z_ORDER_BASE + id,
            x = x,
            y = y,
            cx = cx,
            cy = cy,
            id = id,
            name = name,
            descr = descr,
            a = NS_A,
            pic = NS_PIC,
            r = r,
            rid = rid,
        );
        let run = parse_fragment(dom, &xml)
            .map_err(|e| {
                Error::edit(DiagCode::EditPlanInvalid, format!("墨迹 run 模板不良构: {e}"))
            })?
            .pop()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "墨迹 run 模板为空"))?;
        let mut plan = MutationPlan::new(main);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: None,
            node: run,
        });
        plan.touch(para);
        self.commit_plan(plan)
    }
}
