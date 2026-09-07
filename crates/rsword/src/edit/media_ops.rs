//! 媒体写侧（`PKG-05` / `EDIT-04` / `EDIT-06` / `SAVE-05`，`spec/17` 任务 6.7）：新图片段落、`ReplaceImageMedia`、
//! 按内容去重的媒体 part。资源回收在 `save/prune.rs`。
//!
//! 新图片的段落按 TS `embedImage` / `applyImageWrap` 的模板生成（`wp:inline`，或带 `wrap` 时 `wp:anchor`），
//! 再解析成 `New` 子树——与 `NewBlock::Chart` 同一条路（`chart_ops::materialize`）。`pPr` 的 `w:spacing` / `w:jc`
//! 直接写进模板：段落是整棵新建的，`plan_apply_para_props` 的「合并到已有 pPr」在这里没有对象。

use std::hash::{Hash, Hasher};

use crate::diag::{DiagCode, Diagnostic};
use crate::edit::EditSession;
use crate::edit::plan::{MutationPlan, MutationResult};
use crate::error::{Error, Result};
use crate::model::macros::named_enum;
use crate::model::units::EMU_PER_PX;
use crate::package::RelType;
use crate::package::ns_context::NamespaceContext;
use crate::xml::plan::{NewElement, NodeEdit, Target};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName, parse_fragment};

named_enum! {
    /// TS 的九种 `ImageWrap`。
    pub enum ImageWrap {
        SquareLeft = "square-left",
        SquareRight = "square-right",
        TightLeft = "tight-left",
        TightRight = "tight-right",
        ThroughLeft = "through-left",
        ThroughRight = "through-right",
        TopBottom = "topBottom",
        Front = "front",
        Behind = "behind",
    }
}

impl ImageWrap {
    pub fn parse(s: &str) -> Option<ImageWrap> {
        Some(match s {
            "square-left" => ImageWrap::SquareLeft,
            "square-right" => ImageWrap::SquareRight,
            "tight-left" => ImageWrap::TightLeft,
            "tight-right" => ImageWrap::TightRight,
            "through-left" => ImageWrap::ThroughLeft,
            "through-right" => ImageWrap::ThroughRight,
            "topBottom" => ImageWrap::TopBottom,
            "front" => ImageWrap::Front,
            "behind" => ImageWrap::Behind,
            _ => return None,
        })
    }
}

/// 图片所在段落的 `w:spacing`（TS `paraSpacing`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaSpacing {
    pub before_twips: Option<i64>,
    pub after_twips: Option<i64>,
    pub line_twips: Option<i64>,
    /// `exact` / `atLeast`。
    pub line_rule: Option<String>,
}

/// 一张新图片（TS `NewImage`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewImage {
    pub bytes: Vec<u8>,
    /// `image/png` / `image/jpeg` / `image/gif` …
    pub mime: String,
    /// 显示尺寸（EMU）。
    pub extent_emu: (i64, i64),
    /// `w:jc`：`center` / `right`（`left` 与缺省不写）。
    pub align: Option<String>,
    /// 缺省随文；有 → `wp:anchor`。
    pub wrap: Option<ImageWrap>,
    /// 锚定位置（EMU）：`positionH` 相对栏、`positionV` 相对段落；`page` 为真时两者都相对页面。
    pub pos_offset_emu: Option<PosOffset>,
    /// `relativeHeight = 251658240 + z_order`。
    pub z_order: Option<i64>,
    pub rot_deg: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub para_spacing: Option<ParaSpacing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosOffset {
    pub x: i64,
    pub y: i64,
    pub page: bool,
}

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
pub(crate) const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const NS_WP: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// Word 的 `relativeHeight` 基数。
pub const Z_ORDER_BASE: i64 = 251_658_240;

/// MIME → 媒体 part 扩展名（TS `IMAGE_EXT`；别的 `image/*` 取子类型）。
pub fn extension_for(mime: &str) -> String {
    match mime {
        "image/png" => "png".into(),
        "image/jpeg" | "image/jpg" => "jpg".into(),
        "image/gif" => "gif".into(),
        "image/bmp" => "bmp".into(),
        "image/tiff" => "tiff".into(),
        "image/svg+xml" => "svg".into(),
        other => other.rsplit('/').next().unwrap_or("bin").trim_start_matches("x-").to_string(),
    }
}

impl EditSession {
    /// 把图片字节落成主 part 的媒体 part + `image` 关系，返回 `rId`。**相同字节只建一个 part**（同一会话内按
    /// `(mime, 哈希)` 去重，TS 同）；part 名 `word/media/image{N}.{ext}`（第一个空闲 N），`[Content_Types]` 缺该
    /// 扩展名的 `Default` 就补。
    pub fn add_media(&mut self, bytes: Vec<u8>, mime: &str) -> Result<String> {
        self.add_media_with(bytes, mime, true)
    }

    /// 同 [`Self::add_media`]；`dedup = false` 时总是新建一个 part（墨迹：每条一个 part，TS 同，任务 6.8）。
    pub(crate) fn add_media_with(
        &mut self,
        bytes: Vec<u8>,
        mime: &str,
        dedup: bool,
    ) -> Result<String> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        let key = (mime.to_string(), h.finish());
        if dedup && let Some(rid) = self.media_by_content.get(&key) {
            return Ok(rid.clone());
        }
        let ext = extension_for(mime);
        let main = self.main_part();
        let n = (1u32..)
            .find(|n| {
                let stem = format!("word/media/image{n}.");
                !self
                    .package()
                    .parts()
                    .iter()
                    .any(|p| !p.deleted && p.uri.as_str().starts_with(&stem))
            })
            .expect("总有空闲的编号");
        let (_, rid) = self.add_binary_part(
            main,
            RelType::Image,
            &format!("word/media/image{n}.{ext}"),
            mime,
            bytes,
        )?;
        if dedup {
            self.media_by_content.insert(key, rid.clone());
        }
        Ok(rid)
    }

    /// `EDIT-04 ReplaceImageMedia`（TS `xml.replaceImage`）：`drawing` 子树里第一个 `a:blip` 改指新媒体（`r:link`
    /// 删掉，只有 `r:link` 时改成 `r:embed`）、删第一个 `a:srcRect`、`a:fillRect` 属性清空、删 `asvg:svgBlip` 的
    /// `a:ext` 与空掉的 `a:extLst`。没有 `a:blip` → 不动 + 诊断（hostile）。
    pub(crate) fn replace_image_media(
        &mut self,
        drawing: NodeId,
        bytes: Vec<u8>,
        mime: &str,
        ctx: &crate::edit::EditContext,
    ) -> Result<MutationResult> {
        let main = self.main_part();
        let dom = self.dom();
        if (drawing.0 as usize) >= dom.node_count() {
            return Err(Error::edit(DiagCode::EditBadPosition, "replaceImage 的目标节点不存在"));
        }
        // 追踪：旧 run 进 `w:del`、换了图的克隆 run 进 `w:ins`（`spec/18` 7.3）。
        // 两个阶段：克隆先落到 DOM 里，第二阶段才能定位克隆里的 `a:blip` 去改 `r:embed`
        if let Some(mut t) = crate::edit::track::Tracker::new(self.document(), ctx) {
            let run = std::iter::once(drawing)
                .chain(dom.ancestors(drawing))
                .find(|&a| dom.is(a, QName::w(LocalName::R)))
                .ok_or_else(|| {
                    Error::edit(DiagCode::EditBadPosition, "replaceImage 的目标不在 run 里")
                })?;
            let parent = dom
                .parent(run)
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "run 没有父节点"))?;
            let mut plan = MutationPlan::new(main);
            if let Some(p) = dom.ancestors(run).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                plan.touch(p);
            }
            // 克隆先插（此时源还没被包起来），再把原 run 包进 `w:del`
            let ins_k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before: crate::edit::ops::next_element_sibling(dom, run),
                node: t.marker(LocalName::Ins),
            });
            let clone_k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(ins_k),
                before: None,
                source: run,
            });
            t.wrap_item(&mut plan, dom, run, LocalName::Del);
            let mut result = self.commit_plan(plan)?;
            let clone = result.created[clone_k]
                .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "克隆的 run 没有创建出来"))?;
            // 第二阶段：在克隆里找同一个绘图，按不追踪的路子换图
            let dom = self.dom();
            let cloned_drawing = std::iter::once(clone)
                .chain(dom.descendants(clone))
                .find(|&n| {
                    dom.is(n, QName::w(LocalName::Drawing)) || dom.is(n, QName::w(LocalName::Pict))
                })
                .unwrap_or(clone);
            let plain = crate::edit::EditContext { track_changes: None, ..Default::default() };
            result.absorb(self.replace_image_media(cloned_drawing, bytes, mime, &plain)?);
            return Ok(result);
        }
        let a = |l: LocalName| QName::new(NsId::A, l);
        let Some(blip) = dom.semantic_descendants(drawing).find(|&n| dom.is(n, a(LocalName::Blip)))
        else {
            let diag = Diagnostic::pre_existing(
                main,
                dom.node(drawing).lex.as_ref().map(|l| l.range.clone()),
                DiagCode::EditUnsupported,
                "replaceImage 的目标里没有 a:blip，图片未替换",
            );
            self.push_diagnostic(diag);
            return Ok(MutationResult::default());
        };
        let rid = self.add_media(bytes, mime)?;
        let dom = self.dom();
        let r_embed = QName::new(NsId::R, LocalName::Embed);
        let r_link = QName::new(NsId::R, LocalName::Link);
        let mut plan = MutationPlan::new(main);
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(blip),
            name: r_embed,
            value: rid,
        });
        if dom.attr(blip, r_link).is_some() {
            plan.node_edits.push(NodeEdit::RemoveAttr { node: Target::Node(blip), name: r_link });
        }
        // 旧的裁剪窗与填充窗会把换进来的字节裁掉一块
        if let Some(src) =
            dom.semantic_descendants(drawing).find(|&n| dom.is(n, a(LocalName::SrcRect)))
        {
            plan.node_edits.push(NodeEdit::Delete(src));
        }
        if let Some(fill) = dom.semantic_descendants(drawing).find(|&n| {
            dom.is(n, a(LocalName::FillRect)) && dom.element(n).is_some_and(|e| !e.attrs.is_empty())
        }) {
            for l in [LocalName::L, LocalName::T, LocalName::R, LocalName::B] {
                if dom.attr(fill, QName::new(NsId::None, l)).is_some() {
                    plan.node_edits.push(NodeEdit::RemoveAttr {
                        node: Target::Node(fill),
                        name: QName::new(NsId::None, l),
                    });
                }
            }
        }
        // 换进来的总是位图；Word 会优先用残留的 Office 2016 `asvg:svgBlip` 扩展，删掉
        if let Some(ext_lst) =
            dom.semantic_children(blip).find(|&n| dom.is(n, a(LocalName::ExtLst)))
        {
            let svg_exts: Vec<NodeId> = dom
                .semantic_children(ext_lst)
                .filter(|&e| {
                    dom.is(e, a(LocalName::Ext))
                        && dom.descendants(e).any(|n| {
                            // 任何前缀（TS `<\w+:svgBlip`）；新插入的片段没有 lex_name，按解析后的本地名判
                            dom.element(n).is_some_and(|el| el.name.local == LocalName::SvgBlip)
                        })
                })
                .collect();
            let remaining =
                dom.semantic_children(ext_lst).filter(|e| !svg_exts.contains(e)).count();
            for e in svg_exts {
                plan.node_edits.push(NodeEdit::Delete(e));
            }
            if remaining == 0 {
                plan.node_edits.push(NodeEdit::Delete(ext_lst));
            }
        }
        // 段落投影要刷新（run 图片的 dataUrl 变了）
        if let Some(p) = std::iter::once(drawing)
            .chain(dom.ancestors(drawing))
            .find(|&n| dom.is(n, QName::w(LocalName::P)))
        {
            plan.touch(p);
        }
        self.commit_plan(plan)
    }
}

/// `NewBlock::Image` → 建媒体 part 后的绘图段落（`chart_ops::materialize` 调）。
pub(crate) fn image_paragraph(s: &mut EditSession, img: &NewImage) -> Result<NewElement> {
    let rid = s.add_media(img.bytes.clone(), &img.mime)?;
    let (cx, cy) = (img.extent_emu.0.max(1), img.extent_emu.1.max(1));
    // Word 按未旋转的 wp:extent + wp:effectExtent 排版：转过的非正方形图片要把外接框的溢出记进去
    let rot = img.rot_deg.map_or(0, |d| d.rem_euclid(360));
    let rad = rot as f64 * std::f64::consts::PI / 180.0;
    let bw = (cx as f64 * rad.cos()).abs() + (cy as f64 * rad.sin()).abs();
    let bh = (cx as f64 * rad.sin()).abs() + (cy as f64 * rad.cos()).abs();
    let ee_x = (((bw - cx as f64) / 2.0).round() as i64).max(0);
    let ee_y = (((bh - cy as f64) / 2.0).round() as i64).max(0);
    let flavor = s.flavor();
    let main = s.main_part();
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let id = next_doc_pr_id(dom);
    let ctx = NamespaceContext::from_dom(dom, flavor);
    let (wp, wp_decl) = prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
    let (r, r_decl) = prefix_or_decl(&ctx, NsId::R, "r", NS_R);

    let mut spacing = Vec::new();
    if let Some(ps) = &img.para_spacing {
        if let Some(b) = ps.before_twips.filter(|&v| v > 0) {
            spacing.push(format!(r#"w:before="{b}""#));
        }
        if let Some(a) = ps.after_twips.filter(|&v| v >= 0) {
            spacing.push(format!(r#"w:after="{a}""#));
        }
        if let (Some(l), Some(rule)) = (ps.line_twips.filter(|&v| v != 0), ps.line_rule.as_deref())
        {
            spacing.push(format!(r#"w:line="{l}" w:lineRule="{rule}""#));
        }
    }
    let spacing = if spacing.is_empty() {
        String::new()
    } else {
        format!("<w:spacing {}/>", spacing.join(" "))
    };
    let jc = img
        .align
        .as_deref()
        .filter(|a| *a != "left")
        .map_or(String::new(), |a| format!(r#"<w:jc w:val="{a}"/>"#));
    let ppr = if spacing.is_empty() && jc.is_empty() {
        String::new()
    } else {
        format!("<w:pPr>{spacing}{jc}</w:pPr>")
    };
    let xfrm_attrs = format!(
        "{}{}{}",
        if rot != 0 { format!(r#" rot="{}""#, rot * 60_000) } else { String::new() },
        if img.flip_h { r#" flipH="1""# } else { "" },
        if img.flip_v { r#" flipV="1""# } else { "" }
    );
    let (open, position_wrap, close) = match img.wrap {
        None => (
            format!(r#"<{wp}:inline distT="0" distB="0" distL="0" distR="0">"#),
            String::new(),
            format!("</{wp}:inline>"),
        ),
        Some(wrap) => anchor_parts(&wp, wrap, img.pos_offset_emu, img.z_order),
    };
    let (position, wrap_el) = match position_wrap.split_once('\u{0}') {
        Some((p, w)) => (p.to_string(), w.to_string()),
        None => (String::new(), String::new()),
    };
    let para = format!(
        concat!(
            r#"<w:p>{ppr}<w:r><w:drawing{wp_decl}{r_decl}>{open}{position}"#,
            r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="{eex}" t="{eey}" r="{eex}" b="{eey}"/>{wrap_el}"#,
            r#"<{wp}:docPr id="{id}" name="Picture {id}"/>"#,
            r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}"><pic:pic xmlns:pic="{pic}">"#,
            r#"<pic:nvPicPr><pic:cNvPr id="{id}" name="Picture {id}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
            r#"<pic:blipFill><a:blip {r}:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
            r#"<pic:spPr><a:xfrm{xfrm}><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
            r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>{close}</w:drawing></w:r></w:p>"#
        ),
        ppr = ppr,
        wp_decl = wp_decl,
        r_decl = r_decl,
        open = open,
        position = position,
        wp = wp,
        cx = cx,
        cy = cy,
        eex = ee_x,
        eey = ee_y,
        wrap_el = wrap_el,
        id = id,
        a = NS_A,
        pic = NS_PIC,
        r = r,
        rid = rid,
        xfrm = xfrm_attrs,
        close = close,
    );
    let mut frags = parse_fragment(dom, &para)
        .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("图片段落解析失败: {e}")))?;
    frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "图片段落为空"))
}

/// `wp:anchor` 的三段：开标签、`位置 \0 绕排元素`（绕排元素要放在 extent / effectExtent 之后、docPr 之前）、闭标签
/// （TS `applyImageWrap`）。
fn anchor_parts(
    wp: &str,
    wrap: ImageWrap,
    pos: Option<PosOffset>,
    z_order: Option<i64>,
) -> (String, String, String) {
    let behind = if wrap == ImageWrap::Behind { "1" } else { "0" };
    let open = format!(
        r#"<{wp}:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="{}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1">"#,
        Z_ORDER_BASE + z_order.unwrap_or(0)
    );
    let position = match pos {
        Some(p) => {
            let (rel_h, rel_v) = if p.page { ("page", "page") } else { ("column", "paragraph") };
            format!(
                r#"<{wp}:simplePos x="0" y="0"/><{wp}:positionH relativeFrom="{rel_h}"><{wp}:posOffset>{}</{wp}:posOffset></{wp}:positionH><{wp}:positionV relativeFrom="{rel_v}"><{wp}:posOffset>{}</{wp}:posOffset></{wp}:positionV>"#,
                p.x, p.y
            )
        }
        None => {
            let h = match wrap {
                ImageWrap::SquareRight | ImageWrap::TightRight | ImageWrap::ThroughRight => "right",
                ImageWrap::TopBottom => "center",
                _ => "left",
            };
            format!(
                r#"<{wp}:simplePos x="0" y="0"/><{wp}:positionH relativeFrom="column"><{wp}:align>{h}</{wp}:align></{wp}:positionH><{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>0</{wp}:posOffset></{wp}:positionV>"#
            )
        }
    };
    let wrap_el = match wrap {
        ImageWrap::SquareLeft
        | ImageWrap::SquareRight
        | ImageWrap::TightLeft
        | ImageWrap::TightRight
        | ImageWrap::ThroughLeft
        | ImageWrap::ThroughRight => format!(r#"<{wp}:wrapSquare wrapText="bothSides"/>"#),
        ImageWrap::TopBottom => format!("<{wp}:wrapTopAndBottom/>"),
        ImageWrap::Front | ImageWrap::Behind => format!("<{wp}:wrapNone/>"),
    };
    (open, format!("{position}\u{0}{wrap_el}"), format!("</{wp}:anchor>"))
}

pub(crate) fn prefix_or_decl(
    ctx: &NamespaceContext,
    ns: NsId,
    default: &str,
    uri: &str,
) -> (String, String) {
    match ctx.prefix_for(ns) {
        Some(p) if !p.is_empty() => (p.to_string(), String::new()),
        _ => (default.to_string(), format!(r#" xmlns:{default}="{uri}""#)),
    }
}

/// `EDIT-06`：主 part 里全部 `wp:docPr/@id` 的最大值 + 1。
///
/// 扫**全部**未删节点，包括本引擎不理解的 `mc:Choice` 分支与 `mc:Fallback`：id 的唯一性是整个 part 的事，
/// 与 MCE 选哪支无关。Word 原生墨迹（`Requires="wpi"`）的 `wp:docPr id="1"` 就藏在语义遍历看不见的分支里，
/// 与新图片撞号后 Word 弹恢复提示（真实 Word 第二轮核对，`corpus/real/_round2/EDITED.md`）。
pub(crate) fn next_doc_pr_id(dom: &Dom) -> i64 {
    let mut max = 0i64;
    let mut stack = vec![dom.root()];
    while let Some(n) = stack.pop() {
        if dom.node(n).dirty == crate::xml::Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        stack.extend(e.children.iter().rev());
        if e.name.local != LocalName::DocPr || !dom.is_ns(n, NsId::Wp, "wp") {
            continue;
        }
        if let Some(v) = dom.attr_value(n, QName::new(NsId::None, LocalName::Id))
            && let Ok(id) = v.trim().parse::<i64>()
        {
            max = max.max(id);
        }
    }
    max + 1
}

/// px → EMU（TS `Math.round(px * 9525)`，至少 1）。
pub fn px_to_emu(px: f64) -> i64 {
    ((px * EMU_PER_PX).round() as i64).max(1)
}
