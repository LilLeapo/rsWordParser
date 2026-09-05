//! `SAVE-07` 的声明 part 保存选项（`spec/16` 任务 5.7）：参考文献、编号追加、主题、样式 upsert。
//!
//! 这四项都改**声明 part**，不是正文，所以走 [`plan_all`](super::plan_all) 的 `MutationPlan` 路
//! 而不是编辑操作——它们没有对应的 `EditOp`（正文里没有任何位置可以指）。缺 part 时先按
//! `SAVE-05` 建（[`ensure_parts`]），再翻成计划。
//!
//! | 选项 | part | 做法 |
//! | --- | --- | --- |
//! | `sources` | `customXml/item{N}.xml` | 权威列表：字段没变的条目**原字节不动**，变了的重建，列表外的删掉 |
//! | `numbering` | `word/numbering.xml` | **只追加**：新 `abstractNum` + `num`；既有条目不动 |
//! | `theme_fonts` / `theme_colors` | `word/theme/theme1.xml` | 只改 `@typeface` / 槽里的颜色元素 |
//! | `style_upserts` | `word/styles.xml` | 同 `styleId` 的整条替换，否则追加 |

use crate::edit::MutationPlan;
use crate::error::Result;
use crate::model::sources::publisher_element;
use crate::package::{PartFlavor, PartId, RelType};
use crate::semantic::props::{NewElement, NodeEdit, Target};
use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

use super::SaveOptions;

/// 一条要写的文献源（TS `SourceInfo`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSave {
    pub tag: String,
    /// `b:SourceType`；空串按 TS 写 `Misc`。
    pub kind: String,
    /// `"Last, First"` 或团体名。
    pub author: String,
    pub title: String,
    pub year: String,
    pub publisher: Option<String>,
    pub url: Option<String>,
}

/// `numbering.newDefs` 的一条：`kind` 决定用哪套缺省级别，`levels` 给了就按它生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberingDefSave {
    /// `w:num/@w:numId`（由调用方指定：它是正文 `w:numPr` 已经引用的号）。
    pub num_id: String,
    /// `true` = 项目符号，`false` = 有序（十进制）。
    pub bullet: bool,
    /// 自定义级别；空表示用缺省的 5 级模板。
    pub levels: Vec<NumberingLevelSave>,
}

/// 一级自定义编号（TS `CustomNumberingLevel`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberingLevelSave {
    pub num_fmt: String,
    pub lvl_text: String,
    pub indent_left: i32,
    /// 缺省 360。
    pub hanging: Option<i32>,
    /// 缺省 1。
    pub start: Option<i32>,
}

/// `numbering.restartNums` 的一条：指向已有 `abstractNum` 的新 `w:num` + 起始值覆盖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartNumSave {
    pub num_id: String,
    pub abstract_num_id: String,
    /// `(ilvl, startOverride)`，按 ilvl 升序写。
    pub start_overrides: Vec<(i32, i32)>,
}

/// 主题字体：`a:majorFont` / `a:minorFont` 的拉丁字体，以及可选的东亚字体（两组都写）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeFontsSave {
    pub major: String,
    pub minor: String,
    pub east_asia: Option<String>,
}

/// 主题配色：只有这八个槽可写（`dk1` / `lt1` 常是 `a:sysClr`，Word 自己也不让改）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeColorsSave {
    /// `a:clrScheme/@name`。
    pub name: Option<String>,
    /// `(槽名, 六位十六进制)`；槽名取 `dk2` / `lt2` / `accent1`…`accent6`。
    pub slots: Vec<(String, String)>,
}

/// 可写的配色槽（同 TS `COLOR_TAGS`）。
pub const THEME_COLOR_SLOTS: [&str; 8] =
    ["dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"];

/// 一条样式 upsert（TS `StyleUpsert`）。`r_pr` / `p_pr` 用属性表生成，不手写 XML。
#[derive(Debug, Clone, PartialEq)]
pub struct StyleUpsertSave {
    pub style_id: String,
    /// `w:style/@w:type`（`paragraph` / `character` / `table` / `numbering`）。
    pub kind: String,
    pub name: String,
    pub based_on: Option<String>,
    pub run_props: Option<crate::semantic::props::RunProps>,
    pub para_props: Option<crate::semantic::props::ParaProps>,
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

fn b(local: LocalName) -> QName {
    QName::new(NsId::B, local)
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node(n).dirty != Dirty::Deleted
}

/// `<b:X>文本</b:X>`；值为空时不发这个元素（同 TS 的 `t()`）。
fn b_field(local: LocalName, value: &str) -> Option<NewElement> {
    (!value.is_empty()).then(|| NewElement::new(b(local)).with_text(value))
}

/// 一条 `b:Source`（TS `sourceEntryXml`）：`"Last, First"` 拆成 `b:Last` / `b:First`，
/// 逗号都没有就整串当 `b:Last`。
fn source_element(s: &SourceSave) -> NewElement {
    let kind = if s.kind.is_empty() { "Misc" } else { s.kind.as_str() };
    let mut e = NewElement::new(b(LocalName::Source));
    if let Some(c) = b_field(LocalName::UTag, &s.tag) {
        e.push_child(c);
    }
    if let Some(c) = b_field(LocalName::SourceType, kind) {
        e.push_child(c);
    }
    if !s.author.is_empty() {
        let (last, first) = match s.author.split_once(',') {
            Some((l, f)) => (l.trim(), f.trim()),
            None => (s.author.as_str(), ""),
        };
        let mut person = NewElement::new(b(LocalName::UPerson));
        if let Some(c) = b_field(LocalName::Last, last) {
            person.push_child(c);
        }
        if let Some(c) = b_field(LocalName::First, first) {
            person.push_child(c);
        }
        // TS 的嵌套：`b:Author/b:Author/b:NameList/b:Person`（外层是 CT_Contributor 的容器）
        let list = NewElement::new(b(LocalName::NameList)).with_child(person);
        let inner = NewElement::new(b(LocalName::UAuthor)).with_child(list);
        e.push_child(NewElement::new(b(LocalName::UAuthor)).with_child(inner));
    }
    if let Some(c) = b_field(LocalName::UTitle, &s.title) {
        e.push_child(c);
    }
    if let Some(c) = b_field(LocalName::Year, &s.year) {
        e.push_child(c);
    }
    if let Some(p) = &s.publisher
        && let Some(c) = b_field(publisher_element(kind), p)
    {
        e.push_child(c);
    }
    if let Some(u) = &s.url
        && let Some(c) = b_field(LocalName::URL, u)
    {
        e.push_child(c);
    }
    e
}

/// 现值与请求值是否一致（不一致才重建那条，未建模的域因此保住）。
fn source_unchanged(cur: &crate::model::Source, want: &SourceSave) -> bool {
    let kind = if want.kind.is_empty() { "Misc" } else { want.kind.as_str() };
    cur.kind == kind
        && cur.author == want.author
        && cur.title == want.title
        && cur.year == want.year
        && cur.publisher.as_deref().unwrap_or_default()
            == want.publisher.as_deref().unwrap_or_default()
        && cur.url.as_deref().unwrap_or_default() == want.url.as_deref().unwrap_or_default()
}

/// `sources`：权威列表。顺序按请求；未变的条目原位不动，变了的替换，列表外的删掉。
pub fn sources_plan(
    dom: &Dom,
    part: PartId,
    current: &[crate::model::Source],
    want: &[SourceSave],
) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let root = dom.root();
    let mut keep: Vec<NodeId> = Vec::new();
    // 列表外的删掉
    for cur in current {
        if !want.iter().any(|s| s.tag == cur.tag) {
            plan.node_edits.push(NodeEdit::Delete(cur.node));
        }
    }
    for s in want {
        match current.iter().find(|c| c.tag == s.tag) {
            Some(cur) if source_unchanged(cur, s) => keep.push(cur.node),
            Some(cur) => {
                plan.node_edits.push(NodeEdit::Replace { old: cur.node, node: source_element(s) });
                keep.push(cur.node);
            }
            None => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node: source_element(s),
            }),
        }
    }
    let _ = keep;
    plan
}

/// TS `bulletLevels` / `decimalLevels`：缺省 5 级。照抄 `blank.ts`（出处见模块头）。
fn default_levels(bullet: bool) -> Vec<NewElement> {
    (0..5)
        .map(|ilvl: i32| {
            let mut lvl =
                NewElement::new(w(LocalName::Lvl)).with_attr(w(LocalName::Ilvl), ilvl.to_string());
            lvl.push_child(NewElement::new(w(LocalName::Start)).with_attr(w(LocalName::Val), "1"));
            lvl.push_child(
                NewElement::new(w(LocalName::NumFmt))
                    .with_attr(w(LocalName::Val), if bullet { "bullet" } else { "decimal" }),
            );
            lvl.push_child(NewElement::new(w(LocalName::LvlText)).with_attr(
                w(LocalName::Val),
                if bullet { "\u{F0B7}".to_string() } else { format!("%{}.", ilvl + 1) },
            ));
            lvl.push_child(
                NewElement::new(w(LocalName::LvlJc)).with_attr(w(LocalName::Val), "left"),
            );
            let ind = NewElement::new(w(LocalName::Ind))
                .with_attr(w(LocalName::Left), (720 * (ilvl + 1)).to_string())
                .with_attr(w(LocalName::Hanging), "360");
            lvl.push_child(NewElement::new(w(LocalName::PPr)).with_child(ind));
            if bullet {
                let fonts = NewElement::new(w(LocalName::RFonts))
                    .with_attr(w(LocalName::Ascii), "Symbol")
                    .with_attr(w(LocalName::HAnsi), "Symbol")
                    .with_attr(w(LocalName::Hint), "default");
                lvl.push_child(NewElement::new(w(LocalName::RPr)).with_child(fonts));
            }
            lvl
        })
        .collect()
}

/// 自定义级别（TS `customLevels`）。
fn custom_levels(levels: &[NumberingLevelSave]) -> Vec<NewElement> {
    levels
        .iter()
        .enumerate()
        .map(|(ilvl, l)| {
            let mut lvl =
                NewElement::new(w(LocalName::Lvl)).with_attr(w(LocalName::Ilvl), ilvl.to_string());
            lvl.push_child(
                NewElement::new(w(LocalName::Start))
                    .with_attr(w(LocalName::Val), l.start.unwrap_or(1).to_string()),
            );
            lvl.push_child(
                NewElement::new(w(LocalName::NumFmt)).with_attr(w(LocalName::Val), &l.num_fmt),
            );
            lvl.push_child(
                NewElement::new(w(LocalName::LvlText)).with_attr(w(LocalName::Val), &l.lvl_text),
            );
            lvl.push_child(
                NewElement::new(w(LocalName::LvlJc)).with_attr(w(LocalName::Val), "left"),
            );
            let ind = NewElement::new(w(LocalName::Ind))
                .with_attr(w(LocalName::Left), l.indent_left.to_string())
                .with_attr(w(LocalName::Hanging), l.hanging.unwrap_or(360).to_string());
            lvl.push_child(NewElement::new(w(LocalName::PPr)).with_child(ind));
            lvl
        })
        .collect()
}

/// `numbering`：**只追加**。新 `abstractNum` 插在第一个 `w:num` 之前（schema 顺序），
/// 新 `w:num` 追加在末尾；既有条目一个字节都不动。
pub fn numbering_plan(dom: &Dom, part: PartId, opts: &SaveOptions) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let root = dom.root();
    let kids: Vec<NodeId> = dom.semantic_children(root).filter(|&n| live(dom, n)).collect();
    let first_num = kids.iter().copied().find(|&n| dom.is(n, w(LocalName::Num)));
    // 新 `abstractNumId` 从现有最大值 +1 起（`w:num` 只引用 abstract，号由引擎分配）
    let next_abs = kids
        .iter()
        .filter(|&&n| dom.is(n, w(LocalName::AbstractNum)))
        .filter_map(|&n| dom.attr_value(n, w(LocalName::AbstractNumId)))
        .filter_map(|v| v.trim().parse::<i64>().ok())
        .max()
        .map_or(0, |m| m + 1);
    for (i, def) in opts.numbering_new_defs.iter().enumerate() {
        let next_abs = next_abs + i as i64;
        let levels = if def.levels.is_empty() {
            default_levels(def.bullet)
        } else {
            custom_levels(&def.levels)
        };
        let mut abs = NewElement::new(w(LocalName::AbstractNum))
            .with_attr(w(LocalName::AbstractNumId), next_abs.to_string());
        for l in levels {
            abs.push_child(l);
        }
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(root),
            before: first_num,
            node: abs,
        });
        let num = NewElement::new(w(LocalName::Num))
            .with_attr(w(LocalName::NumId), &def.num_id)
            .with_child(
                NewElement::new(w(LocalName::AbstractNumId))
                    .with_attr(w(LocalName::Val), next_abs.to_string()),
            );
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(root),
            before: None,
            node: num,
        });
    }
    for r in &opts.numbering_restart_nums {
        let mut num = NewElement::new(w(LocalName::Num))
            .with_attr(w(LocalName::NumId), &r.num_id)
            .with_child(
                NewElement::new(w(LocalName::AbstractNumId))
                    .with_attr(w(LocalName::Val), &r.abstract_num_id),
            );
        let mut overrides: Vec<(i32, i32)> = r.start_overrides.clone();
        overrides.sort_by_key(|(ilvl, _)| *ilvl);
        for (ilvl, start) in overrides {
            num.push_child(
                NewElement::new(w(LocalName::LvlOverride))
                    .with_attr(w(LocalName::Ilvl), ilvl.to_string())
                    .with_child(
                        NewElement::new(w(LocalName::StartOverride))
                            .with_attr(w(LocalName::Val), start.to_string()),
                    ),
            );
        }
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(root),
            before: None,
            node: num,
        });
    }
    plan
}

/// 主题里某个字体组（`a:majorFont` / `a:minorFont`）的 `a:latin` / `a:ea`。
fn font_group(dom: &Dom, root: NodeId, group: LocalName) -> Option<NodeId> {
    dom.semantic_descendants(root).find(|&n| live(dom, n) && dom.is(n, a(group)))
}

fn set_typeface(dom: &Dom, group: NodeId, script: LocalName, face: &str, plan: &mut MutationPlan) {
    if let Some(n) = dom.semantic_children(group).find(|&n| live(dom, n) && dom.is(n, a(script))) {
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(n),
            name: QName::new(NsId::None, LocalName::Typeface),
            value: face.to_string(),
        });
    }
}

/// `themeFonts` / `themeColors`：只改用得上的属性，其余原字节不动。
pub fn theme_plan(dom: &Dom, part: PartId, opts: &SaveOptions) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let root = dom.root();
    if let Some(f) = &opts.theme_fonts {
        for (group, face) in [(LocalName::MajorFont, &f.major), (LocalName::MinorFont, &f.minor)] {
            let Some(g) = font_group(dom, root, group) else { continue };
            set_typeface(dom, g, LocalName::Latin, face, &mut plan);
            if let Some(ea) = &f.east_asia {
                set_typeface(dom, g, LocalName::Ea, ea, &mut plan);
            }
        }
    }
    if let Some(c) = &opts.theme_colors {
        let Some(scheme) = dom
            .semantic_descendants(root)
            .find(|&n| live(dom, n) && dom.is(n, a(LocalName::ClrScheme)))
        else {
            return plan;
        };
        if let Some(name) = &c.name {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(scheme),
                name: QName::new(NsId::None, LocalName::Name),
                value: name.clone(),
            });
        }
        for (slot, hex) in &c.slots {
            let Some(node) = dom.semantic_children(scheme).find(|&n| {
                live(dom, n) && dom.name(n).is_some_and(|q| q.local.as_str(dom.interner()) == slot)
            }) else {
                continue;
            };
            let srgb = NewElement::new(a(LocalName::SrgbClr))
                .with_attr(QName::new(NsId::None, LocalName::Val), hex);
            // 槽里原来是 `a:sysClr`（windowText 一类）时也换成 `a:srgbClr`：不然请求会被静默丢掉
            match dom.semantic_children(node).find(|&n| live(dom, n)) {
                Some(old) => plan.node_edits.push(NodeEdit::Replace { old, node: srgb }),
                None => plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(node),
                    before: None,
                    node: srgb,
                }),
            }
        }
    }
    plan
}

/// 一条 `w:style`（TS `buildStyleXml`；`rPr` / `pPr` 走属性表的 emit，顺序由 `PROP-05` 保证）。
fn style_element(up: &StyleUpsertSave, flavor: PartFlavor) -> NewElement {
    let mut e = NewElement::new(w(LocalName::Style))
        .with_attr(w(LocalName::UType), &up.kind)
        .with_attr(w(LocalName::StyleId), &up.style_id)
        .with_attr(w(LocalName::CustomStyle), "1");
    e.push_child(NewElement::new(w(LocalName::Name)).with_attr(w(LocalName::Val), &up.name));
    if let Some(base) = &up.based_on {
        e.push_child(NewElement::new(w(LocalName::BasedOn)).with_attr(w(LocalName::Val), base));
    }
    e.push_child(NewElement::new(w(LocalName::QFormat)));
    if let Some(p) = &up.para_props {
        e.push_child(crate::semantic::props::emit_para_props(p, flavor));
    }
    if let Some(r) = &up.run_props {
        e.push_child(crate::semantic::props::emit_run_props(r, flavor));
    }
    e
}

/// `styleUpserts`：同 `styleId` 的整条替换，否则追加在末尾。
pub fn styles_plan(
    dom: &Dom,
    part: PartId,
    flavor: PartFlavor,
    upserts: &[StyleUpsertSave],
) -> MutationPlan {
    let mut plan = MutationPlan::new(part);
    let root = dom.root();
    for up in upserts {
        let node = style_element(up, flavor);
        let existing = dom.semantic_children(root).filter(|&n| live(dom, n)).find(|&n| {
            dom.is(n, w(LocalName::Style))
                && dom.attr_value(n, w(LocalName::StyleId)).as_deref() == Some(up.style_id.as_str())
        });
        match existing {
            Some(old) => plan.node_edits.push(NodeEdit::Replace { old, node }),
            None => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node,
            }),
        }
    }
    plan
}

/// 这批选项要碰的 part，缺的按 `SAVE-05` 建出来（`plan_all` 是只读的，建 part 得先做）。
///
/// 返回是否真的建了什么（调用方据此决定要不要重建投影）。
pub fn ensure_parts(s: &mut crate::edit::EditSession, opts: &SaveOptions) -> Result<bool> {
    let main = s.main_part();
    let flavor = s.flavor_in(Some(main));
    let mut created = false;
    if !opts.style_upserts.is_empty() && s.package().find_name("word/styles.xml").is_none() {
        let xml = empty_w_part(flavor, "styles");
        s.add_part(main, RelType::Styles, "word/styles.xml", CT_STYLES, &xml)?;
        created = true;
    }
    if (!opts.numbering_new_defs.is_empty() || !opts.numbering_restart_nums.is_empty())
        && s.package().find_name("word/numbering.xml").is_none()
    {
        let xml = empty_w_part(flavor, "numbering");
        s.add_part(main, RelType::Numbering, "word/numbering.xml", CT_NUMBERING, &xml)?;
        created = true;
    }
    if (opts.theme_fonts.is_some() || opts.theme_colors.is_some())
        && s.package().find_name("word/theme/theme1.xml").is_none()
    {
        let xml = theme_template(flavor);
        s.add_part(main, RelType::Theme, "word/theme/theme1.xml", CT_THEME, &xml)?;
        created = true;
    }
    if opts.sources.is_some() && s.document().sources_part.is_none() {
        create_sources_part(s)?;
        created = true;
    }
    Ok(created)
}

/// 空的 `w:` 声明 part。
fn empty_w_part(flavor: PartFlavor, local: &str) -> String {
    let uri = NsId::W.uri(flavor).expect("w 有两族 URI");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{local} xmlns:w="{uri}"></w:{local}>"#
    )
}

/// 最小可用主题：Word 要求 `a:themeElements` 下三块齐全，缺 `a:fmtScheme` 会报文件损坏。
/// 字体与配色用内建 Office 值（同 `resolve` 无 theme part 时的调色板），随后被选项改写。
fn theme_template(flavor: PartFlavor) -> String {
    let uri = NsId::A.uri(flavor).expect("a 有两族 URI");
    let font_group = |tag: &str| {
        format!(
            r#"<a:{tag}><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:{tag}>"#
        )
    };
    let slot = |tag: &str, hex: &str| format!(r#"<a:{tag}><a:srgbClr val="{hex}"/></a:{tag}>"#);
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<a:theme xmlns:a="{uri}" name="Office"><a:themeElements>"#,
            r#"<a:clrScheme name="Office">{colors}</a:clrScheme>"#,
            r#"<a:fontScheme name="Office">{major}{minor}</a:fontScheme>"#,
            r#"<a:fmtScheme name="Office">"#,
            r#"<a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst>"#,
            r#"<a:lnStyleLst><a:ln w="6350">{fill}</a:ln><a:ln w="12700">{fill}</a:ln>"#,
            r#"<a:ln w="19050">{fill}</a:ln></a:lnStyleLst>"#,
            r#"<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle>"#,
            r#"<a:effectStyle><a:effectLst/></a:effectStyle>"#,
            r#"<a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>"#,
            r#"<a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst>"#,
            r#"</a:fmtScheme></a:themeElements></a:theme>"#
        ),
        uri = uri,
        colors = [
            slot("dk1", "000000"),
            slot("lt1", "FFFFFF"),
            slot("dk2", "44546A"),
            slot("lt2", "E7E6E6"),
            slot("accent1", "4472C4"),
            slot("accent2", "ED7D31"),
            slot("accent3", "A5A5A5"),
            slot("accent4", "FFC000"),
            slot("accent5", "5B9BD5"),
            slot("accent6", "70AD47"),
            slot("hlink", "0563C1"),
            slot("folHlink", "954F72"),
        ]
        .concat(),
        major = font_group("majorFont"),
        minor = font_group("minorFont"),
        fill = fill,
    )
}

/// `SAVE-05` 扩到 customXml：`customXml/item{N}.xml` + `itemProps{N}.xml` + 两条关系。
fn create_sources_part(s: &mut crate::edit::EditSession) -> Result<()> {
    let main = s.main_part();
    let mut n = 1usize;
    let uri = loop {
        let item = format!("customXml/item{n}.xml");
        let props = format!("customXml/itemProps{n}.xml");
        if s.package().find_name(&item).is_none() && s.package().find_name(&props).is_none() {
            break item;
        }
        n += 1;
    };
    let sources_ns = NsId::B.uri(PartFlavor::Transitional).expect("b 有两族 URI");
    // 只声明 `xmlns:b`（不像 TS 再绑一个同 URI 的默认命名空间）：那样新加的子元素会落到
    // 默认绑定上、序列化成不带前缀的 `<Source>`。同一个命名空间，Word 都认，但带前缀更好读
    let item_xml = format!(
        r#"<b:Sources SelectedStyle="\APASixthEditionOfficeOnline.xsl" StyleName="APA" Version="6" xmlns:b="{sources_ns}"></b:Sources>"#
    );
    let (item_part, _) = s.add_part(main, RelType::CustomXml, &uri, CT_CUSTOM_XML, &item_xml)?;
    // `itemProps` 挂在 item 自己的 `.rels` 上（Word 就是这么组织的）
    let ds = NsId::Ds.uri(PartFlavor::Transitional).expect("ds 有两族 URI");
    let props_xml = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<ds:datastoreItem ds:itemID="{{4A8D2934-D5B1-4C15-A6F4-7A1B2C3D4E5F}}" xmlns:ds="{ds}">"#,
            r#"<ds:schemaRefs><ds:schemaRef ds:uri="{ns}"/></ds:schemaRefs></ds:datastoreItem>"#
        ),
        ds = ds,
        ns = sources_ns
    );
    let props_uri = format!("customXml/itemProps{n}.xml");
    s.add_part(item_part, RelType::CustomXmlProps, &props_uri, CT_CUSTOM_XML_PROPS, &props_xml)?;
    Ok(())
}

const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CT_CUSTOM_XML: &str = "application/xml";
const CT_CUSTOM_XML_PROPS: &str =
    "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
