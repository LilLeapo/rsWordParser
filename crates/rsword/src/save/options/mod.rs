//! `SAVE-07` 保存选项与 `SAVE-01` 第 4 步 `apply_save_options`。
//!
//! 每项选项都翻译成普通 DOM 变更，没有旁路。两条路：
//!
//! - **元数据与清洗**（`saved_at` / `remove_personal_info` / `remove_date_and_time`）走
//!   [`plan_all`]，直接产出各 part 的 [`MutationPlan`]。它们不是"编辑"，没有对应的 `EditOp`。
//! - **文档内容**（节 / 页眉页脚 / 水印 / 页面底色 / 保护 / 奇偶页眉）走 [`edit_ops`]，翻成
//!   5.5 的编辑操作再由 `EditSession::apply_all` 执行。同一条路意味着同一套校验、同一套脏标记、
//!   同一套 `SAVE-05` 新建 part（`spec/16` 任务 5.6 的"没有旁路"）。
//!
//! 作者清洗规则与 TS `scrubPersonalMetadata` 对齐：除 `customXml/*` 与 `docProps/custom.xml` 外的每个 XML part 里
//! `w:author`（含无前缀的 `author`）改为 `Author`、`w:initials` 改为 `A`；`core.xml` 的 `dc:creator` 与
//! `cp:lastModifiedBy` 清空；`app.xml` 的 `Manager` / `Company` 清空；`word/people.xml` 的 `w15:person` 整条删除。
//!
//! 日期清洗是独立的一项（OOXML 的 `w:removeDateAndTime`，TS 没有这个能力）：批注元素（带 `w:author` 的
//! 修订与批注）上的 `w:date` 删除。`remove_personal_info` 单独开启时日期保留，与 TS 一致。

pub mod decl;
pub mod hf;
pub mod section;
pub mod settings;

pub use decl::{
    NumberingDefSave, NumberingLevelSave, RestartNumSave, SourceSave, StyleUpsertSave,
    ThemeColorsSave, ThemeFontsSave,
};
pub use hf::{HfSlots, SectionHfSave};
pub use section::{PgNumTypeOption, ProtectionOption, SectionSaveSettings, WriteProtectionOption};

use crate::diag::Diagnostic;
use crate::edit::{EditOp, InkSave, MutationPlan};
use crate::error::Result;
use crate::model::SectionOwner;
use crate::package::{Package, PartFlavor, PartId, RelType};
use crate::semantic::props::{PropsPatch, SectType, SettingsPatch};
use crate::xml::{Dirty, Dom, LocalName, NodeEdit, NodeId, NodeKind, NsId, QName, Target};

/// `BIND-04` 原生保存选项：只有包级五项，内容修改必须通过 EditOp。
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveOptions {
    /// 修改时间；单独设置不强制保存。
    pub saved_at: Option<String>,
    /// 缺省 false；文档标志只作为事实读取，不自动清洗。
    pub remove_personal_info: bool,
    /// 缺省 false；仅显式 true 才清洗。
    pub remove_date_and_time: bool,
    /// None 等价于 true，仅回收本次会话造成的孤儿。
    pub prune_orphans: Option<bool>,
    /// 缺省 false。
    pub normalize_z_order: bool,
}

impl From<&SaveOptions> for CompatSaveOptions {
    fn from(value: &SaveOptions) -> Self {
        let SaveOptions {
            saved_at,
            remove_personal_info,
            remove_date_and_time,
            prune_orphans,
            normalize_z_order,
        } = value;
        Self {
            saved_at: saved_at.clone(),
            remove_personal_info: remove_personal_info.then_some(true),
            remove_date_and_time: remove_date_and_time.then_some(true),
            prune_orphans: *prune_orphans,
            normalize_z_order: *normalize_z_order,
            ..Default::default()
        }
    }
}

/// `SAVE-07`：与 TS `CompatSaveOptions` 对齐的保存选项（M1 子集）。
#[derive(Debug, Clone, Default, PartialEq)]
#[doc(hidden)]
pub struct CompatSaveOptions {
    /// `docProps/core.xml` 的 `dcterms:modified`（ISO 8601，毫秒被去掉）。
    ///
    /// 只有保存真的进入序列化路径时才落盘：单独设置它**不会**让一份未编辑的文档产生输出
    /// （不变式 1 优先，与 TS 的 `isUnchanged` 判定一致）。
    pub saved_at: Option<String>,
    /// `word/settings.xml` 的 `w:removePersonalInformation`：`Some` 写入该值并按该值决定是否清洗作者；
    /// `None` 沿用文档已有的标志。
    pub remove_personal_info: Option<bool>,
    /// `word/settings.xml` 的 `w:removeDateAndTime`：`Some` 写入该值并按该值决定是否删除批注 / 修订上的
    /// `w:date`；`None` 沿用文档已有的标志。TS `CompatSaveOptions` 没有这一项（`docs/04` §8）。
    pub remove_date_and_time: Option<bool>,

    // ---- 5.6：落在**最后一节**（body 级 `w:sectPr`）的四项 ----
    /// 页面设置（`w:pgSz` / `w:pgMar` / `w:pgBorders` / `w:cols` / `w:bidi`）。
    pub section: Option<SectionSaveSettings>,
    /// 本节相对上一节的起始方式（`w:type`）。
    pub section_start_type: Option<SectType>,
    /// 页码格式与起始值（`w:pgNumType`）；两个字段都缺表示删掉该元素。
    pub pg_num_type: Option<PgNumTypeOption>,
    /// 首页页眉页脚不同（`w:titlePg`）。
    pub title_pg: Option<bool>,

    // ---- 5.6：包级 ----
    /// `w:document/w:background` 的页面底色（六位十六进制）；`Some(None)` 删除，`None` 不动。
    /// 写颜色时一并打开 `w:displayBackgroundShape`（Word 只在那时才画背景）。
    pub page_color: Option<Option<String>>,
    /// `w:documentProtection`；`Some(None)` 删除，`None` 不动。
    pub protection: Option<Option<ProtectionOption>>,
    /// `w:writeProtection`；`Some(None)` 删除，`None` 不动。
    pub write_protection: Option<Option<WriteProtectionOption>>,
    /// `w:evenAndOddHeaders`。
    pub even_and_odd_headers: Option<bool>,

    // ---- 5.6b：页眉页脚 ----
    /// 六个槽的内容（kind × variant）。内容按 TS `headerFooterPartXml` 的规则由适配器算好。
    pub hf: HfSlots,
    /// 文字水印（default 页眉）；`Some(None)` 删除，`None` 不动。
    pub watermark: Option<Option<String>>,
    /// 逐节的页眉页脚（TS `sectionHf[]`）。
    pub section_hf: Vec<SectionHfSave>,
    /// 把新建的页眉页脚 part 挂到每个自己不带引用的 `w:sectPr` 上（TS `hfAllSections`）。
    pub hf_all_sections: bool,

    // ---- 5.7：声明 part ----
    /// 参考文献的**权威列表**（`None` = 不动）：列表外的条目删掉，字段没变的原字节不动。
    pub sources: Option<Vec<SourceSave>>,
    /// 新编号定义（只追加）。
    pub numbering_new_defs: Vec<NumberingDefSave>,
    /// 重新起编号的 `w:num`（只追加）。
    pub numbering_restart_nums: Vec<RestartNumSave>,
    /// 主题字体。
    pub theme_fonts: Option<ThemeFontsSave>,
    /// 主题配色。
    pub theme_colors: Option<ThemeColorsSave>,
    /// 样式 upsert。
    pub style_upserts: Vec<StyleUpsertSave>,

    // ---- 6.8：墨迹 ----
    /// 墨迹批注的**权威列表**（TS `inks`）：`Some` → 删掉全部已有墨迹 run，再按列表逐条追加
    /// （`EditOp::RemoveInks` + `InsertInk`；旧媒体与关系随资源回收消失）；`Some(vec![])` 只删；`None` 不动。
    pub inks: Option<Vec<InkSave>>,

    // ---- 7.7：z 序归一 ----
    /// 主 part 的浮动对象按 z 序稳定重排成 `251658240 + 0..n`（TS `normalizeImageZOrders`）。
    /// 缺省 **false**：没人要求就不动未编辑的字节（不变式 1）。开着时也只在文档里真有
    /// 「野值」（某个 `|z| > 10000`，LibreOffice 一类的产出）时才动手——与投影层的闸门同一条。
    pub normalize_z_order: bool,

    // ---- 6.7：资源回收 ----
    /// 保存时回收**本次会话**让引用数归零的图片 / 图表 / 图示 / OLE / 超链接关系与它们的 part 子图
    /// （`save/prune.rs`）。`None` = 开（缺省）；原本就是孤儿的 part 一个字节不动（TS 会一并删掉，`docs/04` §8）。
    pub prune_orphans: Option<bool>,
}

impl CompatSaveOptions {
    pub fn is_empty(&self) -> bool {
        *self == CompatSaveOptions::default()
    }

    /// 是否要求保存必须进入序列化路径（即使没有脏节点）。
    ///
    /// `saved_at` **不算**（单独设置它不该让一份未编辑的文档产生输出，同 TS `isUnchanged`）；
    /// 其余每一项都是对文档的修改请求，即使最终算出来是空补丁也要走完流程。
    pub fn forces_save(&self) -> bool {
        self.remove_personal_info.is_some()
            || self.remove_date_and_time.is_some()
            || self.section.is_some()
            || self.section_start_type.is_some()
            || self.pg_num_type.is_some()
            || self.title_pg.is_some()
            || self.page_color.is_some()
            || self.protection.is_some()
            || self.write_protection.is_some()
            || self.even_and_odd_headers.is_some()
            || !hf::is_empty(self)
            || self.sources.is_some()
            || !self.numbering_new_defs.is_empty()
            || !self.numbering_restart_nums.is_empty()
            || self.theme_fonts.is_some()
            || self.theme_colors.is_some()
            || !self.style_upserts.is_empty()
            || self.inks.is_some()
            || self.normalize_z_order
    }
}

/// `SAVE-07` 第二条路：内容类选项 → 5.5 的编辑操作（`spec/16` 任务 5.6）。
///
/// 顺序有意义：节属性先落，再是页面底色，最后是 `settings.xml`（后两者互不相干，但把包级的放在
/// 后面让失败时的诊断更好读）。返回空表示这批选项对这份文档没有要改的东西。
///
/// 节的四项只作用在**最后一节且它是 body 级的** `w:sectPr` 上（TS 的 trailing hidden sectPr）：
/// 文档里连一个 `w:sectPr` 都没有时 TS 什么都不做，我们也不凭空造一个（新建分节属性容器
/// 就是新建分节符，见 `docs/04` §8）。
pub(crate) fn edit_ops(
    s: &crate::edit::EditSession,
    opts: &CompatSaveOptions,
) -> (Vec<EditOp>, Vec<(crate::model::HfKind, crate::model::HfVariant)>) {
    let doc = s.document();
    let mut ops = Vec::new();
    if let Some(last) = doc.sections.last()
        && last.owner == SectionOwner::Body
        && let Some(sect) = last.node
    {
        let patch = section_patch(&last.props, opts);
        if !patch.is_empty() {
            ops.push(EditOp::SetSectionProps { sect, patch });
        }
    }
    let (mut hf_ops, created) = hf::content_ops(s, opts);
    ops.append(&mut hf_ops);
    if let Some(color) = &opts.page_color {
        ops.push(EditOp::SetPageColor { color: color.clone() });
    }
    let patch = settings_patch(opts);
    if !patch.is_empty() {
        ops.push(EditOp::SetDocumentSettings { patch });
    }
    if opts.normalize_z_order {
        ops.extend(z_order_ops(s));
    }
    // 6.8：墨迹层整体重发（TS 对每个最终块 `stripInkRuns` 再注入）
    if let Some(inks) = &opts.inks {
        ops.push(EditOp::RemoveInks);
        ops.extend(inks.iter().map(|e| EditOp::InsertInk { para: e.para, ink: e.ink.clone() }));
    }
    (ops, created)
}

/// z 序归一（TS `normalizeImageZOrders` 的写回方向，`docs/01` §13.7）：主 part 的每个
/// `wp:anchor` 按 `relativeHeight - 251658240` 稳定排序（同值按文档序），名次就是新的 z。
///
/// 闸门与投影层同一条：全部 `|z| <= 10000` 说明这份文档的 z 序本来就是规矩的，一个字节都不动。
/// 名次已经对上的那些也不发操作（`SetDrawingZOrder` 会让节点 `SelfDirty`）。
fn z_order_ops(s: &crate::edit::EditSession) -> Vec<EditOp> {
    const Z_BASE: i64 = crate::edit::media_ops::Z_ORDER_BASE;
    let main = s.document().main_part;
    let Some(dom) = s.package().part(main).dom() else { return Vec::new() };
    let mut anchored: Vec<(NodeId, i64)> = Vec::new();
    for n in dom.descendants(dom.root()) {
        if dom.node(n).dirty == Dirty::Deleted
            || !dom.is(n, QName::new(NsId::Wp, LocalName::Anchor))
        {
            continue;
        }
        let z = dom
            .attr_value(n, QName::new(NsId::None, LocalName::RelativeHeight))
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map_or(0, |h| h - Z_BASE);
        let Some(drawing) = dom.ancestors(n).find(|&a| dom.is(a, w(LocalName::Drawing))) else {
            continue;
        };
        anchored.push((drawing, z));
    }
    if !anchored.iter().any(|&(_, z)| z.abs() > 10_000) {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..anchored.len()).collect();
    order.sort_by_key(|&i| (anchored[i].1, i));
    order
        .into_iter()
        .enumerate()
        .filter(|&(rank, i)| anchored[i].1 != rank as i64)
        .map(|(rank, i)| EditOp::SetDrawingZOrder { drawing: anchored[i].0, z: rank as i64 })
        .collect()
}

/// `SAVE-07` 第二轮：`hfAllSections` 要等第一轮把 part 建出来才知道挂哪个。
pub(crate) fn link_ops(
    s: &crate::edit::EditSession,
    opts: &CompatSaveOptions,
    created: &[(crate::model::HfKind, crate::model::HfVariant)],
) -> Vec<EditOp> {
    hf::link_ops(s, opts, created)
}

/// 节的四项合成一个补丁（它们碰的字段互不相交）。
fn section_patch(
    current: &crate::semantic::props::SectionProps,
    opts: &CompatSaveOptions,
) -> crate::semantic::props::SectionPropsPatch {
    let mut patch = match &opts.section {
        Some(s) => section::settings_patch(current, s),
        None => Default::default(),
    };
    if let Some(kind) = opts.section_start_type {
        patch.kind = section::start_type_patch(kind).kind;
    }
    if let Some(p) = &opts.pg_num_type {
        patch.page_numbers = section::pg_num_type_patch(p).page_numbers;
    }
    if let Some(on) = opts.title_pg {
        patch.title_pg = section::title_pg_patch(on).title_pg;
    }
    patch
}

/// `settings.xml` 的三项合成一个补丁。页面底色写值时顺带打开 `w:displayBackgroundShape`
/// （Word 只在这个开关打开时才画 `w:background`；删底色时不去关它，同 TS）。
fn settings_patch(opts: &CompatSaveOptions) -> SettingsPatch {
    let background = matches!(&opts.page_color, Some(Some(_))).then_some(true);
    let mut patch = settings::flags_patch(opts.even_and_odd_headers, background);
    if let Some(p) = &opts.protection {
        patch.document_protection = settings::protection_patch(p.as_ref()).document_protection;
    }
    if let Some(p) = &opts.write_protection {
        patch.write_protection = settings::write_protection_patch(p.as_ref()).write_protection;
    }
    patch
}

const CORE_PROPS: &str = "docProps/core.xml";
const APP_PROPS: &str = "docProps/app.xml";
const CUSTOM_PROPS: &str = "docProps/custom.xml";
const PEOPLE: &str = "word/people.xml";
const SETTINGS: &str = "word/settings.xml";

fn w(local: LocalName) -> QName {
    QName::w(local)
}

fn live(dom: &Dom, id: NodeId) -> bool {
    dom.node(id).dirty != Dirty::Deleted
}

/// 子树里名字匹配的活元素（含根自身）。
fn elements(dom: &Dom, name: QName) -> Vec<NodeId> {
    dom.descendants(dom.root()).filter(|&n| live(dom, n) && dom.is(n, name)).collect()
}

/// 清空元素内容（保留标签，与 TS `clearQualifiedElements` 一致）。
fn clear_element(dom: &Dom, id: NodeId, plan: &mut MutationPlan) {
    for c in dom.children(id).iter().copied().filter(|&c| live(dom, c)) {
        match &dom.node(c).kind {
            NodeKind::Text(_) => {
                if dom.text(c).is_some_and(|t| !t.is_empty()) {
                    plan.node_edits.push(NodeEdit::SetText { node: c, text: String::new() });
                }
            }
            _ => plan.node_edits.push(NodeEdit::Delete(c)),
        }
    }
}

/// 设置元素的唯一文本子节点；没有文本子节点则不动（TS 不注入缺失的标签）。
fn set_element_text(dom: &Dom, id: NodeId, text: &str, plan: &mut MutationPlan) {
    let Some(node) = dom
        .children(id)
        .iter()
        .copied()
        .find(|&c| live(dom, c) && matches!(dom.node(c).kind, NodeKind::Text(_)))
    else {
        return;
    };
    if dom.text(node).as_deref() != Some(text) {
        plan.node_edits.push(NodeEdit::SetText { node, text: text.to_string() });
    }
}

/// TS `patchCoreProps`：`.mmmZ` → `Z`。
fn normalize_timestamp(ts: &str) -> String {
    let Some(head) = ts.strip_suffix('Z') else { return ts.to_string() };
    match head.rsplit_once('.') {
        Some((before, frac)) if frac.len() == 3 && frac.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{before}Z")
        }
        _ => ts.to_string(),
    }
}

/// `dcterms:modified` ← `iso`，`cp:revision` ← +1（都只在标签存在时）。
fn plan_core_props(dom: &Dom, part: PartId, iso: &str) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    for id in elements(dom, QName::new(NsId::DcTerms, LocalName::Modified)) {
        set_element_text(dom, id, iso, &mut plan);
    }
    for id in elements(dom, QName::new(NsId::Cp, LocalName::Revision)) {
        let next = dom
            .children(id)
            .iter()
            .copied()
            .find(|&c| live(dom, c) && matches!(dom.node(c).kind, NodeKind::Text(_)))
            .and_then(|c| dom.text(c)?.trim().parse::<u64>().ok())
            .map(|n| n + 1);
        if let Some(n) = next {
            set_element_text(dom, id, &n.to_string(), &mut plan);
        }
    }
    plan
}

/// `word/settings.xml` 的两个清洗标志（位置与顺序由 `plan_apply_settings` 按 `PROP-05` 保证）。
fn plan_settings_flags(
    dom: &Dom,
    part: PartId,
    personal: Option<bool>,
    dates: Option<bool>,
) -> MutationPlan {
    use crate::semantic::props::{Change, SettingsPatch, plan_apply_settings};
    let flag = |v: Option<bool>| match v {
        None => Change::Keep,
        Some(true) => Change::Set(true),
        Some(false) => Change::Unset,
    };
    let root = dom.root();
    let patch = SettingsPatch {
        remove_personal_information: flag(personal),
        remove_date_and_time: flag(dates),
        ..Default::default()
    };
    let mut plan = MutationPlan::new(part);
    plan.node_edits = plan_apply_settings(dom, root, Some(root), &patch, dom.flavor());
    plan
}

/// 一个 part 的清洗计划：`authors` 改作者与缩写，`dates` 删批注元素上的 `w:date`。
fn plan_scrub(dom: &Dom, part: PartId, uri: &str, authors: bool, dates: bool) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let author = w(LocalName::Author);
    let initials = w(LocalName::Initials);
    let date = w(LocalName::Date);
    let bare_author = QName::new(NsId::None, LocalName::Author);
    let bare_initials = QName::new(NsId::None, LocalName::Initials);
    let bare_date = QName::new(NsId::None, LocalName::Date);
    for id in dom.descendants(dom.root()) {
        if !live(dom, id) {
            continue;
        }
        let Some(e) = dom.element(id) else { continue };
        // 批注元素 = 带作者属性的元素（修订、`w:comment`、`w15:person` 之外的注释类）
        let annotated = e.attrs.iter().any(|a| a.name == author || a.name == bare_author);
        for a in &e.attrs {
            if dates && annotated && (a.name == date || a.name == bare_date) {
                plan.node_edits.push(NodeEdit::RemoveAttr { node: Target::Node(id), name: a.name });
                continue;
            }
            if !authors {
                continue;
            }
            let replacement = if a.name == author || a.name == bare_author {
                "Author"
            } else if a.name == initials || a.name == bare_initials {
                "A"
            } else {
                continue;
            };
            if dom.attr_str(a) != replacement {
                plan.node_edits.push(NodeEdit::SetAttr {
                    node: Target::Node(id),
                    name: a.name,
                    value: replacement.to_string(),
                });
            }
        }
    }
    if !authors {
        return plan;
    }
    if uri.eq_ignore_ascii_case(CORE_PROPS) {
        for name in [
            QName::new(NsId::Dc, LocalName::Creator),
            QName::new(NsId::Cp, LocalName::LastModifiedBy),
        ] {
            for id in elements(dom, name) {
                clear_element(dom, id, &mut plan);
            }
        }
    }
    if uri.eq_ignore_ascii_case(APP_PROPS) {
        for local in [LocalName::Manager, LocalName::Company] {
            for id in elements(dom, QName::new(NsId::Ep, local)) {
                clear_element(dom, id, &mut plan);
            }
        }
    }
    if uri.eq_ignore_ascii_case(PEOPLE) {
        for id in elements(dom, QName::new(NsId::W15, LocalName::Person)) {
            plan.node_edits.push(NodeEdit::Delete(id));
        }
    }
    plan
}

/// 该 part 是否参与清洗（`customXml/*` 与自定义属性由 TS 与我们一致地放过）。
fn scrubbable(uri: &str) -> bool {
    !uri.starts_with("customXml/") && !uri.eq_ignore_ascii_case(CUSTOM_PROPS)
}

/// `SAVE-01` 第 4 步：把选项翻译成各 part 的计划（只读产出）+ 计划外的诊断。
pub(crate) fn plan_all(
    pkg: &mut Package,
    opts: &CompatSaveOptions,
    scrub_authors: bool,
    scrub_dates: bool,
) -> Result<(Vec<MutationPlan>, Vec<Diagnostic>)> {
    let mut plans = Vec::new();
    let diags = Vec::new();
    let main = pkg.main_part();

    if let Some(ts) = &opts.saved_at {
        let iso = normalize_timestamp(ts);
        if let Some(id) = pkg.find_name(CORE_PROPS) {
            pkg.dom(id)?;
            if let Some(dom) = pkg.part(id).dom() {
                let plan = plan_core_props(dom, id, &iso);
                if !plan.is_empty() {
                    plans.push(plan);
                }
            }
        }
    }

    if opts.remove_personal_info.is_some() || opts.remove_date_and_time.is_some() {
        let settings =
            pkg.related(main, RelType::Settings).next().or_else(|| pkg.find_name(SETTINGS));
        // 缺 part 时什么都不用做：只有"要写 true"才需要 part，那种情况 `save_with` 已经按
        // `SAVE-05` 建好了；写 false 时标志缺失本来就等于 false
        if let Some(id) = settings {
            pkg.dom(id)?;
            if let Some(dom) = pkg.part(id).dom() {
                let plan = plan_settings_flags(
                    dom,
                    id,
                    opts.remove_personal_info,
                    opts.remove_date_and_time,
                );
                if !plan.is_empty() {
                    plans.push(plan);
                }
            }
        }
    }

    // 5.7：声明 part 的四项（part 由 `decl::ensure_parts` 在此之前建好）
    let decl_targets: Vec<(Option<PartId>, u8)> = vec![
        (pkg.find_name("word/styles.xml"), 0),
        (
            pkg.related(main, RelType::Numbering)
                .next()
                .or_else(|| pkg.find_name("word/numbering.xml")),
            1,
        ),
        (
            pkg.related(main, RelType::Theme)
                .next()
                .or_else(|| pkg.find_name("word/theme/theme1.xml")),
            2,
        ),
    ];
    let want_decl = [
        !opts.style_upserts.is_empty(),
        !opts.numbering_new_defs.is_empty() || !opts.numbering_restart_nums.is_empty(),
        opts.theme_fonts.is_some() || opts.theme_colors.is_some(),
    ];
    for (id, which) in decl_targets {
        if !want_decl[which as usize] {
            continue;
        }
        let Some(id) = id else { continue };
        pkg.dom(id)?;
        let flavor = pkg.part(id).flavor.unwrap_or(PartFlavor::Transitional);
        if let Some(dom) = pkg.part(id).dom() {
            let plan = match which {
                0 => decl::styles_plan(dom, id, flavor, &opts.style_upserts),
                1 => decl::numbering_plan(dom, id, opts),
                _ => decl::theme_plan(dom, id, opts),
            };
            if !plan.is_empty() {
                plans.push(plan);
            }
        }
    }
    if let Some(want) = &opts.sources
        && let Some(id) = crate::model::sources::find_part(pkg)
    {
        pkg.dom(id)?;
        if let Some(dom) = pkg.part(id).dom() {
            let current = crate::model::sources::read(dom);
            let plan = decl::sources_plan(dom, id, &current, want);
            if !plan.is_empty() {
                plans.push(plan);
            }
        }
    }

    if scrub_authors || scrub_dates {
        let ids: Vec<(PartId, String)> = pkg
            .parts()
            .iter()
            .filter(|p| p.is_xml)
            .map(|p| (p.id, p.uri.as_str().to_string()))
            .filter(|(_, uri)| scrubbable(uri))
            .collect();
        for (id, uri) in ids {
            pkg.dom(id)?;
            if let Some(dom) = pkg.part(id).dom() {
                let plan = plan_scrub(dom, id, &uri, scrub_authors, scrub_dates);
                if !plan.is_empty() {
                    plans.push(plan);
                }
            }
        }
    }
    Ok((plans, diags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_07_timestamp_drops_milliseconds() {
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00.123Z"), "2026-07-28T08:30:00Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00Z"), "2026-07-28T08:30:00Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00.12Z"), "2026-07-28T08:30:00.12Z");
        assert_eq!(normalize_timestamp("2026-07-28T08:30:00+08:00"), "2026-07-28T08:30:00+08:00");
    }

    #[test]
    fn save_07_custom_xml_is_not_scrubbed() {
        assert!(!scrubbable("customXml/item1.xml"));
        assert!(!scrubbable("docProps/custom.xml"));
        assert!(scrubbable("docProps/core.xml"));
        assert!(scrubbable("word/glossary/document.xml"));
    }
}
