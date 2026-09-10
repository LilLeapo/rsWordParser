//! `COMPAT-08` / `EDIT-04`：TS `saveDocx(parsed, finalBlocks, options)` 的 `SaveBlock[]` → `EditOp`。
//!
//! | SaveBlock | 映射 |
//! | --- | --- |
//! | `original`（顺序不变） | 无操作，节点保持 `Clean` |
//! | `original` 顺序变化 | `MoveBlock`（M1：仅全 original 列表） |
//! | `generated` 取代缺失的 original `w:p` | `ReplaceParaProps`（`rawPPr` 逐字 / 由 `type`+`format` 重建）+ `ReplaceInlines` |
//! | 多余的 `generated` | `InsertBlock{Paragraph}`，插在下一个 original 之前（sdt 首段前 → sdt 之前） |
//! | `xml` | `InsertBlock{Xml}`；带 `docxIndex` 时先插后删原块 |
//! | 缺失的 original | `DeleteBlock` |
//! | `chart` | `InsertBlock{NewBlock::Chart}`（图表 part + 工作簿 + 关系 + 绘图段落，6.6） |
//! | `image` | `InsertBlock{NewBlock::Image}`（媒体 part 按内容去重 + `image` 关系 + 随文 / 锚定段落，6.7） |
//! | `xml` + `replaceImage` | 先 `InsertBlock{Xml}`，再对新块 `ReplaceImageMedia`（6.7） |
//! | `SaveOptions.partXml` / `partBinary` | `ReplacePartXml` / `ReplacePartBytes`（只接受已存在的 part，6.6） |
//! | `SaveOptions.savedAt` / `removePersonalInfo` | 翻成 [`SaveOptions`]，由 `save_with` 执行（`SAVE-07`） |
//! | 其余 `SaveOptions` | M1 不支持 → `Err(EDIT_UNSUPPORTED)` |
//!
//! `GeneratedBlock.runs` 按 TS `runsXml` / `runFragmentXml` / `generateRunXml` 的语义翻译成 `NewInline`：
//! 批注范围标记按 `commentIds` 的首末 run 重发，同 `href` 的连续 run 合成一个 `w:hyperlink`，`ins/del`
//! 分组包裹；`rawRPr` 按 `mergeRPrModel` 的分组规则与模型字段合并（相等的组保留原样，不等的组重建，
//! 未建模子元素原位保留），无 `rawRPr` 时按 `modelRPrChildren` 从模型生成。`sdtShell` 忽略（DOM 天然保留）。

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

use crate::bind::compat_ts::{MediaSet, blocks, parsed_doc_of};
use crate::diag::DiagCode;
use crate::edit::{
    BlockPos, EditContext, EditOp, EditSession, EntryParas, EntryReconciliation, ImageExtentEmu,
    ImageExtentPx, ImageWrap, InkSave, NewBlock, NewChart, NewChartKind, NewChartSeries,
    NewComment, NewImage, NewInk, NewInline, NewLinkTarget, NewMarker, NewRevision, NewRun,
    ParaSpacing, PosOffset,
};
use crate::error::{Error, Result};
use crate::model::{HfKind, HfVariant};
use crate::package::{PartFlavor, RelType};
use crate::save::options::CompatSaveOptions as SaveOptions;
use crate::save::options::{
    NumberingDefSave, NumberingLevelSave, PgNumTypeOption, ProtectionOption, RestartNumSave,
    SectionHfSave, SectionSaveSettings, SourceSave, StyleUpsertSave, ThemeColorsSave,
    ThemeFontsSave, WriteProtectionOption,
};
use crate::semantic::props::{
    Border, BorderStyle, Color, DropCap, FontHint, Fonts, FrameAnchor, FramePr, FrameWrap,
    HeightRule, HexColorOrAuto, HighlightColor, Indent, Jc, LineSpacingRule, NumPr, ParaBorders,
    ParaProps, RunProps, Shading, ShadingPattern, Spacing, Tab, TabJc, TabLeader, Tabs, Underline,
    UnderlineKind, Val, VerticalAlignRun, emit_para_props, emit_run_props, order_index_run_props,
    read_run_props,
};
use crate::semantic::props::{DocProtect, NumberFormat, SectType};
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NodeId, NsId, QName, parse_fragment, parse_fragment_dom,
};

/// `apply_save_blocks` 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SaveBlocksOutcome {
    /// 全部 original 且顺序不变 → 没有产生任何 `EditOp`（是否真的返回原字节还要看 `save_options`
    /// 与文档的 `w:removePersonalInformation` 标志，由 [`crate::edit::EditSession::save_with`] 判定）。
    pub unchanged: bool,
    pub ops: usize,
    /// `SaveOptions.savedAt` / `removePersonalInfo` 的翻译结果，交给 `save_with`。
    pub save_options: SaveOptions,
}

fn unsupported(msg: impl Into<String>) -> Error {
    Error::edit(DiagCode::EditUnsupported, msg)
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// TS `bookmarkIdOf`：`h = (h * 31 + code) | 0` 逐 UTF-16 单元，`Math.abs(h) % 0x7fffffff`。
pub fn bookmark_id_of(name: &str) -> u32 {
    let mut h: i32 = 0;
    for u in name.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(u));
    }
    h.unsigned_abs() % 0x7fff_ffff
}

enum Item<'a> {
    Original(usize),
    Generated(&'a Value, Option<&'a Value>),
    Xml {
        xml: &'a str,
        docx_index: Option<usize>,
        revision: Option<&'a Value>,
        /// TS `replaceImage {base64, mime}`：块插好后把它第一个 `a:blip` 换成新媒体（任务 6.7）。
        replace_image: Option<&'a Value>,
    },
    /// TS `kind:"chart"`：新图表（任务 6.6）。
    Chart {
        chart: &'a Value,
        extent: Option<&'a Value>,
        revision: Option<&'a Value>,
    },
    /// TS `kind:"image"`：新图片（任务 6.7）。
    Image {
        image: &'a Value,
        revision: Option<&'a Value>,
    },
}

fn s_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn truthy(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn round(x: f64) -> i32 {
    x.round() as i32
}

/// TS `SaveOptions` JSON → [`SaveOptions`]（M1 支持的两项）；其余键属后续里程碑。
/// `SaveOptions` 里的"权威条目列表"：批注与脚注 / 尾注。
///
/// TS 的语义是**整份替换**：列表里没有的条目连正文里的标记一起删掉，列表里有的按内容改或新建。
#[derive(Debug, Clone, Default)]
struct EntryLists {
    comments: Option<Vec<Value>>,
    footnotes: Option<Vec<Value>>,
    endnotes: Option<Vec<Value>>,
    /// TS `partXml`：zip 路径 → 整份 XML（任务 6.6）。
    part_xml: Vec<(String, String)>,
    /// TS `partBinary`：zip 路径 → base64 字节。
    part_binary: Vec<(String, String)>,
    /// TS `inks`（权威列表，任务 6.8）：`blockIndex` 是 finalBlocks 下标，块操作落定后才能解析成节点。
    inks: Option<Vec<Value>>,
}

/// 页眉页脚选项的原始 JSON：内容要用 `Planner`（借着主 part 的 DOM）才能翻成 `NewBlock`，
/// 所以先原样收着，等 `Planner` 建好再翻（见 [`apply_save_blocks`]）。
#[derive(Debug, Clone, Default)]
struct HfOptionJson {
    /// `(TS 键名, HeaderFooter)`。
    slots: Vec<(String, Value)>,
    /// `sectionHf[]` 原样。
    section_hf: Vec<Value>,
}

fn save_options_of(options: &Value) -> Result<(SaveOptions, EntryLists, HfOptionJson)> {
    let mut out = SaveOptions::default();
    let mut lists = EntryLists::default();
    let mut hfs = HfOptionJson::default();
    let Some(map) = options.as_object() else { return Ok((out, lists, hfs)) };
    let arr = |v: &Value, k: &str| -> Result<Vec<Value>> {
        v.as_array().cloned().ok_or_else(|| unsupported(format!("SaveOptions {k:?} 不是数组")))
    };
    for (k, v) in map {
        match k.as_str() {
            "savedAt" => out.saved_at = v.as_str().map(str::to_string),
            "removePersonalInfo" => out.remove_personal_info = v.as_bool(),
            "comments" => lists.comments = Some(arr(v, "comments")?),
            "footnotes" => lists.footnotes = Some(arr(v, "footnotes")?),
            "endnotes" => lists.endnotes = Some(arr(v, "endnotes")?),
            "inks" => lists.inks = Some(arr(v, "inks")?),
            "partXml" | "partBinary" => {
                let obj = v
                    .as_object()
                    .ok_or_else(|| unsupported(format!("SaveOptions {k:?} 不是对象")))?;
                let target =
                    if k == "partXml" { &mut lists.part_xml } else { &mut lists.part_binary };
                for (path, content) in obj {
                    let text = content.as_str().ok_or_else(|| {
                        unsupported(format!("SaveOptions {k:?}[{path}] 不是字符串"))
                    })?;
                    target.push((path.clone(), text.to_string()));
                }
            }
            // ---- 5.6：节与包级选项 ----
            "section" => out.section = Some(section_settings_of(v)?),
            "sectionStartType" => out.section_start_type = Some(sect_type_of(v)?),
            "pgNumType" => out.pg_num_type = Some(pg_num_type_of(v)?),
            "titlePg" => out.title_pg = v.as_bool(),
            "pageColor" => out.page_color = Some(page_color_of(v)?),
            "evenAndOddHeaders" => out.even_and_odd_headers = v.as_bool(),
            "protection" => out.protection = Some(protection_of(v)?),
            "writeProtection" => out.write_protection = Some(write_protection_of(v)?),
            // ---- 5.6b：页眉页脚 ----
            "watermark" => {
                out.watermark = Some(match v {
                    Value::Null => None,
                    Value::String(t) if t.is_empty() => None,
                    Value::String(t) => Some(t.clone()),
                    other => return Err(unsupported(format!("watermark 不是字符串: {other}"))),
                })
            }
            "hfAllSections" => out.hf_all_sections = v.as_bool().unwrap_or(false),
            // ---- 5.7：声明 part ----
            "sources" => {
                out.sources = Some(
                    arr(v, "sources")?.iter().map(source_save_of).collect::<Result<Vec<_>>>()?,
                )
            }
            "numbering" => numbering_of(v, &mut out)?,
            "themeFonts" => out.theme_fonts = Some(theme_fonts_of(v)),
            "themeColors" => out.theme_colors = Some(theme_colors_of(v)),
            "styleUpserts" => {
                out.style_upserts =
                    arr(v, "styleUpserts")?.iter().map(style_upsert_of).collect::<Result<_>>()?
            }
            "sectionHf" => hfs.section_hf = arr(v, "sectionHf")?,
            k if HF_SLOT_KEYS.contains(&k) => {
                if !v.is_object() {
                    return Err(unsupported(format!("SaveOptions {k:?} 不是对象")));
                }
                hfs.slots.push((k.to_string(), v.clone()));
            }
            other => {
                return Err(unsupported(format!("SaveOptions {other:?} 在后续里程碑（SAVE-07）")));
            }
        }
    }
    Ok((out, lists, hfs))
}

/// 六个槽的 TS 键名（`HfSlots::by_ts_key` 认的那些）。
const HF_SLOT_KEYS: &[&str] =
    &["header", "footer", "headerFirst", "footerFirst", "headerEven", "footerEven"];

/// 六位十六进制的页面底色；`null` = 删除。
fn page_color_of(v: &Value) -> Result<Option<String>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(Some(s.clone()))
        }
        other => Err(unsupported(format!("pageColor 不是六位十六进制: {other}"))),
    }
}

fn i32_of(v: &Value, k: &str) -> Result<i32> {
    v.get(k)
        .and_then(Value::as_i64)
        .map(|n| n as i32)
        .ok_or_else(|| unsupported(format!("SectionSettings 缺 {k:?}")))
}

/// TS `SectionSettings` → 写侧子集（读侧的只读字段不参与保存）。
fn section_settings_of(v: &Value) -> Result<SectionSaveSettings> {
    let opt_i32 = |k: &str| v.get(k).and_then(Value::as_i64).map(|n| n as i32);
    let widths = v
        .get("colWidths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).map(|n| n as i32).collect::<Vec<_>>());
    Ok(SectionSaveSettings {
        page_width: i32_of(v, "pageWidth")?,
        page_height: i32_of(v, "pageHeight")?,
        landscape: v.get("orientation").and_then(Value::as_str) == Some("landscape"),
        margin_top: i32_of(v, "marginTop")?,
        margin_right: i32_of(v, "marginRight")?,
        margin_bottom: i32_of(v, "marginBottom")?,
        margin_left: i32_of(v, "marginLeft")?,
        header_dist: opt_i32("headerDist"),
        footer_dist: opt_i32("footerDist"),
        page_border: v.get("pageBorder").and_then(Value::as_bool).unwrap_or(false),
        columns: opt_i32("columns").unwrap_or(1),
        col_space: opt_i32("colSpace"),
        col_widths: widths,
        bidi: v.get("bidi").and_then(Value::as_bool),
    })
}

fn sect_type_of(v: &Value) -> Result<SectType> {
    let s = v.as_str().ok_or_else(|| unsupported("sectionStartType 不是字符串"))?;
    SectType::parse(s).ok_or_else(|| unsupported(format!("sectionStartType {s:?} 不是合法值")))
}

fn pg_num_type_of(v: &Value) -> Result<PgNumTypeOption> {
    let fmt = match v.get("fmt").and_then(Value::as_str) {
        None => None,
        Some(s) => Some(
            NumberFormat::parse(s)
                .ok_or_else(|| unsupported(format!("pgNumType.fmt {s:?} 不是合法值")))?,
        ),
    };
    Ok(PgNumTypeOption { fmt, start: v.get("start").and_then(Value::as_i64).map(|n| n as i32) })
}

fn protection_of(v: &Value) -> Result<Option<ProtectionOption>> {
    if v.is_null() {
        return Ok(None);
    }
    let edit = v.get("edit").and_then(Value::as_str).unwrap_or("none");
    let edit = DocProtect::parse(edit)
        .ok_or_else(|| unsupported(format!("protection.edit {edit:?} 不是合法值")))?;
    Ok(Some(ProtectionOption {
        edit,
        enforced: v.get("enforced").and_then(Value::as_bool).unwrap_or(false),
        hash: v.get("hash").and_then(Value::as_str).map(str::to_string),
        salt: v.get("salt").and_then(Value::as_str).map(str::to_string),
        spin_count: v.get("spinCount").and_then(Value::as_i64).map(|n| n as i32),
        algorithm_sid: v.get("algorithmSid").and_then(Value::as_i64).map(|n| n as i32),
    }))
}

fn write_protection_of(v: &Value) -> Result<Option<WriteProtectionOption>> {
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(WriteProtectionOption {
        recommended: v.get("recommended").and_then(Value::as_bool).unwrap_or(false),
        hash: v.get("hash").and_then(Value::as_str).map(str::to_string),
        salt: v.get("salt").and_then(Value::as_str).map(str::to_string),
        spin_count: v.get("spinCount").and_then(Value::as_i64).map(|n| n as i32),
        algorithm_sid: v.get("algorithmSid").and_then(Value::as_i64).map(|n| n as i32),
    }))
}

/// `richParas` 的一个 run → `w:rPr`（TS 的八个字段）。
fn rich_run_props(r: &Value) -> Option<NewElement> {
    let mut rpr = NewElement::new(w(LocalName::RPr));
    let on = |k: &str| r.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut any = false;
    for (k, local) in
        [("bold", LocalName::B), ("italic", LocalName::I), ("strike", LocalName::Strike)]
    {
        if on(k) {
            rpr.push_child(NewElement::new(w(local)));
            any = true;
        }
    }
    if on("caps") {
        rpr.push_child(NewElement::new(w(LocalName::Caps)));
        any = true;
    }
    if on("underline") {
        rpr.push_child(NewElement::new(w(LocalName::U)).with_attr(w(LocalName::Val), "single"));
        any = true;
    }
    if let Some(c) = r.get("color").and_then(Value::as_str) {
        rpr.push_child(NewElement::new(w(LocalName::Color)).with_attr(w(LocalName::Val), c));
        any = true;
    }
    if let Some(sz) = r.get("sizeHalfPoints").and_then(Value::as_i64) {
        rpr.push_child(
            NewElement::new(w(LocalName::Sz)).with_attr(w(LocalName::Val), sz.to_string()),
        );
        any = true;
    }
    any.then_some(rpr)
}

/// `{text, richParas?}` → 条目段落。`richParas` 在就照它的 run 与格式发，否则按 `\n` 分段。
fn entry_paras_of(entry: &Value) -> EntryParas {
    if let Some(paras) = entry.get("richParas").and_then(Value::as_array) {
        let out: EntryParas = paras
            .iter()
            .map(|line| {
                line.as_array()
                    .map(|runs| {
                        runs.iter()
                            .map(|r| NewRun {
                                text: r
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                props: rich_run_props(r),
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        if !out.is_empty() {
            return out;
        }
    }
    entry
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split('\n')
        .map(|line| vec![NewRun { text: line.to_owned(), props: None }])
        .collect()
}

impl From<&EntryLists> for EntryReconciliation {
    #[inline]
    fn from(lists: &EntryLists) -> Self {
        let comments = lists.comments.as_ref().map(|list| {
            list.iter()
                .filter_map(|c| {
                    let id = c.get("id").and_then(Value::as_str)?;
                    let paras = entry_paras_of(c);
                    let meta = NewComment {
                        author: c
                            .get("author")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        initials: c.get("initials").and_then(Value::as_str).map(str::to_string),
                        date: c.get("date").and_then(Value::as_str).map(str::to_string),
                        text: String::new(), // 正文用 `paras`（`richParas` 可能带格式）
                        parent_id: c.get("parentId").and_then(Value::as_str).map(str::to_string),
                        done: c.get("done").and_then(Value::as_bool).unwrap_or(false),
                    };
                    Some((id.to_owned(), meta, paras))
                })
                .collect()
        });
        let notes = |list: &Option<Vec<Value>>| {
            list.as_ref().map(|list| {
                list.iter()
                    .filter_map(|n| Some((n.get("id")?.as_str()?.to_owned(), entry_paras_of(n))))
                    .collect()
            })
        };
        Self { comments, footnotes: notes(&lists.footnotes), endnotes: notes(&lists.endnotes) }
    }
}

/// 把 TS `SaveBlock[]`（`finalBlocks`）与 `SaveOptions` 应用到会话；任一块不受支持则不改任何状态。
/// 返回的 `save_options` 要传给 [`crate::edit::EditSession::save_with`]（`SAVE-01` 第 4 步）。
pub fn apply_save_blocks(
    session: &mut EditSession,
    final_blocks: &Value,
    options: &Value,
) -> Result<SaveBlocksOutcome> {
    let (mut save_options, lists, hf_json) = save_options_of(options)?;
    // BIND-04 v3：沿用文档标志是 TS 宿主策略，原生内核不再隐式清洗。
    if save_options.remove_personal_info.is_none() && session.remove_personal_info_flag() {
        save_options.remove_personal_info = Some(true);
    }
    if save_options.remove_date_and_time.is_none() && session.remove_date_and_time_flag() {
        save_options.remove_date_and_time = Some(true);
    }
    let final_blocks =
        final_blocks.as_array().ok_or_else(|| unsupported("finalBlocks 不是数组"))?;
    // 7.7：投影层判过这份文档的 z 序是野值（`imageZOrderNormalized`），块表带着归一后的名次
    // 回来——那就把 z 序也写回 XML，不然屏幕上的叠放次序与文件里的对不上。
    save_options.normalize_z_order |=
        final_blocks.iter().any(|b| truthy(b, "imageZOrderNormalized"));
    // 保存路径按 docxIndex / 原字节匹配块，用不到图片 dataURL，给一张空的媒体表即可。
    let parsed = parsed_doc_of(session.package(), session.document(), &MediaSet::default());
    let body = session
        .document()
        .body
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "文档没有 w:body"))?;
    let nodes = blocks::element_nodes(session.dom(), body);
    let pblocks = parsed["blocks"].as_array().expect("parsed_doc blocks");
    if nodes.len() != pblocks.len() {
        return Err(Error::edit(
            DiagCode::EditPlanInvalid,
            format!("元素表 {} 与块表 {} 长度不一致", nodes.len(), pblocks.len()),
        ));
    }
    let visible: Vec<usize> =
        pblocks.iter().enumerate().filter(|(_, b)| !truthy(b, "hidden")).map(|(i, _)| i).collect();
    let heading_ids: HashMap<u32, String> = parsed["headingStyleIds"]
        .as_object()
        .map(|m| {
            m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect()
        })
        .unwrap_or_default();
    let list_style = parsed["listParagraphStyleId"].as_str().map(str::to_string);

    // 页眉页脚选项的内容（TS `headerFooterPartXml` 的规则）要借主 part 的 DOM 翻，而且必须在
    // isUnchanged 短路**之前**——这些用例的正文块本来就没动，改的只有页眉页脚
    let mut pending_section_hf = Vec::new();
    if !hf_json.slots.is_empty() || !hf_json.section_hf.is_empty() {
        let flavor = session.flavor();
        let main = session.main_part();
        let no_rels = HashMap::new();
        let dom = session.package_mut().dom_mut(main)?.expect("main part parsed");
        let mut planner = Planner {
            dom,
            flavor,
            body,
            nodes: &nodes,
            heading_ids: &heading_ids,
            list_style: list_style.as_deref(),
            link_rels: &no_rels,
            replace_images: Vec::new(),
        };
        for (key, hf) in &hf_json.slots {
            let blocks = planner.hf_blocks(hf)?;
            let slot = save_options.hf.by_ts_key(key).expect("键已经过滤过");
            *slot = Some(blocks);
        }
        for e in &hf_json.section_hf {
            pending_section_hf.push(planner.section_hf_of(e)?);
        }
    }

    // SaveBlock → Item
    let mut items: Vec<Item<'_>> = Vec::with_capacity(final_blocks.len());
    for fb in final_blocks {
        let revision = fb.get("revision").filter(|r| r.is_object());
        match s_of(fb, "kind") {
            Some("original") => {
                if revision.is_some() {
                    return Err(unsupported(
                        "original 块的修订包裹（把已有块搬进 New w:ins）在 M7",
                    ));
                }
                let d =
                    fb["docxIndex"].as_u64().ok_or_else(|| unsupported("original 缺 docxIndex"))?
                        as usize;
                if d >= nodes.len() {
                    return Err(Error::edit(
                        DiagCode::EditBadPosition,
                        format!("docxIndex {d} 越界"),
                    ));
                }
                items.push(Item::Original(d));
            }
            Some("generated") => {
                let blk = fb.get("block").ok_or_else(|| unsupported("generated 缺 block"))?;
                items.push(Item::Generated(blk, revision));
            }
            Some("xml") => {
                let xml = s_of(fb, "xml").ok_or_else(|| unsupported("xml 缺 xml"))?;
                let docx_index = fb["docxIndex"].as_u64().map(|d| d as usize);
                let replace_image = fb.get("replaceImage").filter(|r| r.is_object());
                items.push(Item::Xml { xml, docx_index, revision, replace_image });
            }
            Some("image") => {
                let image = fb.get("image").ok_or_else(|| unsupported("image 缺 image"))?;
                items.push(Item::Image { image, revision });
            }
            Some("chart") => {
                let chart = fb.get("chart").ok_or_else(|| unsupported("chart 缺 chart"))?;
                items.push(Item::Chart { chart, extent: fb.get("extentPx"), revision });
            }
            other => return Err(unsupported(format!("SaveBlock kind {other:?} 在后续里程碑"))),
        }
    }

    // TS isUnchanged
    let all_original_in_order = items.len() == visible.len()
        && items.iter().zip(&visible).all(|(it, &v)| matches!(it, Item::Original(d) if *d == v));
    if all_original_in_order {
        // 块没动，但权威条目列表可能要删 / 改条目，整 part 替换也照样做（TS 的 isUnchanged 也看它们）
        let extra = session.reconcile_entries(EntryReconciliation::from(&lists))?
            + apply_part_replacements(session, &lists)?;
        save_options.section_hf = resolve_section_hf(session, pending_section_hf)?;
        save_options.inks = resolve_inks(session, lists.inks.as_deref())?;
        let unchanged = extra == 0 && !save_options.forces_save();
        return Ok(SaveBlocksOutcome { unchanged, ops: extra, save_options });
    }

    let main = session.main_part();
    // `EDIT-06`：generated 块里没有 `rId` 的新外链先分配关系（`.rels` 也进同一个事务）
    let mut link_rels: HashMap<String, String> = HashMap::new();
    for href in new_external_links(final_blocks) {
        let rid = session.add_external_relationship(main, RelType::Hyperlink, &href)?;
        link_rels.insert(href, rid);
    }
    let flavor = session.flavor();
    let ops = {
        let dom = session.package_mut().dom_mut(main)?.expect("main part parsed");
        let mut planner = Planner {
            dom,
            flavor,
            body,
            nodes: &nodes,
            heading_ids: &heading_ids,
            list_style: list_style.as_deref(),
            link_rels: &link_rels,
            replace_images: Vec::new(),
        };
        let ops = planner.build_ops(&items, &visible)?;
        (ops, std::mem::take(&mut planner.replace_images))
    };
    let (ops, replace_images) = ops;
    let n = ops.len();
    let results = session.apply_all(ops, &EditContext::default())?;
    // `replaceImage`：块插好了，找到它第一个带 `a:blip` 的新块换媒体（TS 在插入前改字符串；我们改 DOM）
    let mut extra_ops = Vec::new();
    for (op_indices, (bytes, mime)) in replace_images {
        let dom = session.dom();
        let target = op_indices.iter().find_map(|&i| {
            let node = results.get(i)?.created.first().copied().flatten()?;
            dom.semantic_descendants(node)
                .any(|n| dom.is(n, QName::new(NsId::A, LocalName::Blip)))
                .then_some(node)
        });
        match target {
            Some(drawing) => extra_ops.push(EditOp::ReplaceImageMedia { drawing, bytes, mime }),
            None => {
                return Err(Error::edit(
                    DiagCode::EditPlanInvalid,
                    "replaceImage：插入的 xml 块里没有 a:blip",
                ));
            }
        }
    }
    let n = n + extra_ops.len();
    if !extra_ops.is_empty() {
        session.apply_all(extra_ops, &EditContext::default())?;
    }
    // 条目列表在块之后应用：删掉的批注要连"块重发出来的"标记一起清掉
    let extra = session.reconcile_entries(EntryReconciliation::from(&lists))?
        + apply_part_replacements(session, &lists)?;
    save_options.section_hf = resolve_section_hf(session, pending_section_hf)?;
    save_options.inks = resolve_inks(session, lists.inks.as_deref())?;
    Ok(SaveBlocksOutcome { unchanged: false, ops: n + extra, save_options })
}

/// 一个 `replaceImage`：产生它的 `InsertBlock` 在 op 列表里的下标们 + 新媒体（字节、mime）。
type ReplaceImageJob = (Vec<usize>, (Vec<u8>, String));

struct Planner<'a> {
    dom: &'a mut Dom,
    flavor: PartFlavor,
    body: NodeId,
    nodes: &'a [NodeId],
    heading_ids: &'a HashMap<u32, String>,
    list_style: Option<&'a str>,
    /// 新外链的 `href` → 刚分配的 `rId`（`EDIT-06`）。
    link_rels: &'a HashMap<String, String>,
    /// `xml` 块的 `replaceImage`：这些下标的 `InsertBlock` 建出的块里，第一个带 `a:blip` 的换成新媒体
    /// （块插好、拿到节点之后才能做，`apply_save_blocks` 收尾）。
    replace_images: Vec<ReplaceImageJob>,
}

/// generated 块里需要新建关系的外部链接：有 `href`、不是文内锚点、没带 `rId`。
///
/// 顺序即出现顺序，去重；`rId` 在建 `Planner` 之前分配，那时还能借用 `EditSession`。
fn new_external_links(final_blocks: &[Value]) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                if let Some(Value::Object(l)) = m.get("link")
                    && let Some(href) = l.get("href").and_then(Value::as_str)
                    && !href.is_empty()
                    && !href.starts_with('#')
                    && l.get("rId").and_then(Value::as_str).is_none_or(str::is_empty)
                    && !out.iter().any(|h| h == href)
                {
                    out.push(href.to_string());
                }
                for (_, x) in m {
                    walk(x, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for b in final_blocks {
        walk(b, &mut out);
    }
    out
}

impl Planner<'_> {
    fn build_ops(&mut self, items: &[Item<'_>], visible: &[usize]) -> Result<Vec<EditOp>> {
        let present: BTreeSet<usize> = items
            .iter()
            .filter_map(|it| if let Item::Original(d) = it { Some(*d) } else { None })
            .collect();
        for &d in &present {
            if !visible.contains(&d) {
                return Err(unsupported(format!("original 引用隐藏块 docxIndex {d}")));
            }
        }
        let missing: Vec<usize> =
            visible.iter().copied().filter(|d| !present.contains(d)).collect();
        let mut handled: HashSet<usize> = HashSet::new();
        let mut ops = Vec::new();

        // 重排（M1：只支持全 original 列表）
        let mut max_seen: Option<usize> = None;
        let mut out_of_order = Vec::new();
        for (k, it) in items.iter().enumerate() {
            if let Item::Original(d) = it {
                if max_seen.is_some_and(|m| *d < m) {
                    out_of_order.push(k);
                } else {
                    max_seen = Some(*d);
                }
            }
        }
        if !out_of_order.is_empty() {
            if !items.iter().all(|it| matches!(it, Item::Original(_))) {
                return Err(unsupported("original 重排与 generated/xml 混合"));
            }
            for &k in &out_of_order {
                let Item::Original(d) = items[k] else { unreachable!() };
                let node = self.nodes[d];
                if self.dom.parent(node) != Some(self.body) {
                    return Err(unsupported("重排 sdt 内的段落"));
                }
                let to = match k.checked_sub(1).map(|p| &items[p]) {
                    Some(Item::Original(pd)) => BlockPos::after(self.nodes[*pd]),
                    _ => BlockPos::start(self.body),
                };
                ops.push(EditOp::MoveBlock { from: None, node, to });
            }
        }

        // 逐个"间隙"（两个 present original 之间的非 original 序列）配对
        let mut i = 0;
        let mut lo: Option<usize> = None;
        while i < items.len() {
            if let Item::Original(d) = items[i] {
                lo = Some(d);
                i += 1;
                continue;
            }
            let j = items[i..]
                .iter()
                .position(|it| matches!(it, Item::Original(_)))
                .map_or(items.len(), |k| i + k);
            let hi = if j < items.len() {
                if let Item::Original(d) = items[j] { Some(d) } else { None }
            } else {
                None
            };
            let mut candidates: Vec<usize> = missing
                .iter()
                .copied()
                .filter(|&m| lo.is_none_or(|l| m > l) && hi.is_none_or(|h| m < h))
                .collect();
            let anchor = match hi {
                Some(h) => BlockPos::before(self.insert_anchor(self.nodes[h])),
                None => BlockPos::end(self.body),
            };
            for it in &items[i..j] {
                match it {
                    Item::Original(_) => unreachable!(),
                    Item::Generated(blk, revision) => {
                        // 与缺失的 original 配对：`w:p`，或单段 sdt 里的那个 `w:p`（TS 用 sdtShell 重新包裹）
                        let paired = candidates.first().copied().and_then(|c| {
                            let n = self.nodes[c];
                            if self.dom.is(n, w(LocalName::P)) {
                                Some((c, n))
                            } else if self.dom.is(n, w(LocalName::Sdt)) {
                                blocks::sdt_single_paragraph(self.dom, n).map(|p| (c, p))
                            } else {
                                None
                            }
                        });
                        match paired {
                            Some((c, para)) if revision.is_none() => {
                                candidates.remove(0);
                                handled.insert(c);
                                self.generated_replace(para, blk, &mut ops)?;
                            }
                            _ => {
                                let (props, inlines) = self.generated_paragraph(blk)?;
                                let block = NewBlock::Paragraph { props, inlines };
                                ops.push(EditOp::InsertBlock {
                                    at: anchor,
                                    block: wrap_revision(block, *revision),
                                });
                            }
                        }
                    }
                    Item::Xml { xml, docx_index, revision, replace_image } => {
                        let frags = self.fragment_blocks(xml)?;
                        let first_op = ops.len();
                        match docx_index {
                            Some(d) => {
                                let old = self.nodes[*d];
                                for frag in frags {
                                    ops.push(EditOp::InsertBlock {
                                        at: BlockPos::before(old),
                                        block: wrap_revision(NewBlock::Xml(frag), *revision),
                                    });
                                }
                                let inserted: Vec<usize> = (first_op..ops.len()).collect();
                                ops.push(EditOp::DeleteBlock { part: None, node: old });
                                handled.insert(*d);
                                candidates.retain(|&c| c != *d);
                                if let Some(ri) = replace_image {
                                    self.replace_images.push((inserted, replace_media_of(ri)?));
                                }
                            }
                            None => {
                                for frag in frags {
                                    ops.push(EditOp::InsertBlock {
                                        at: anchor,
                                        block: wrap_revision(NewBlock::Xml(frag), *revision),
                                    });
                                }
                                if let Some(ri) = replace_image {
                                    let inserted: Vec<usize> = (first_op..ops.len()).collect();
                                    self.replace_images.push((inserted, replace_media_of(ri)?));
                                }
                            }
                        }
                    }
                    Item::Chart { chart, extent, revision } => ops.push(EditOp::InsertBlock {
                        at: anchor,
                        block: wrap_revision(chart_block(chart, *extent)?, *revision),
                    }),
                    Item::Image { image, revision } => ops.push(EditOp::InsertBlock {
                        at: anchor,
                        block: wrap_revision(image_block(image)?, *revision),
                    }),
                }
            }
            for c in candidates {
                handled.insert(c);
                ops.push(EditOp::DeleteBlock { part: None, node: self.nodes[c] });
            }
            i = j;
        }
        for m in missing {
            if !handled.contains(&m) {
                ops.push(EditOp::DeleteBlock { part: None, node: self.nodes[m] });
            }
        }
        Ok(ops)
    }

    /// 在某个块之前插入时的锚点：sdt 拆分出的首段 → 整个 sdt（TS 把新块放在 `openXml` 之前）。
    fn insert_anchor(&self, node: NodeId) -> NodeId {
        let dom = &*self.dom;
        let mut top = node;
        while dom.parent(top).is_some_and(|p| p != self.body) {
            top = dom.parent(top).expect("checked");
        }
        if top == node {
            return node;
        }
        let first_in_sdt = self.nodes.iter().copied().find(|&n| {
            let mut t = n;
            while dom.parent(t).is_some_and(|p| p != self.body) {
                t = dom.parent(t).expect("checked");
            }
            t == top
        });
        if first_in_sdt == Some(node) { top } else { node }
    }

    /// `kind: 'xml'` 的片段：TS 直接拼接字符串，所以允许多个顶层块元素。
    fn fragment_blocks(&mut self, xml: &str) -> Result<Vec<NewElement>> {
        let frags = parse_fragment(self.dom, xml)
            .map_err(|e| unsupported(format!("xml 块解析失败: {e}")))?;
        if frags.is_empty() {
            return Err(unsupported("xml 块没有顶层元素"));
        }
        Ok(frags)
    }

    /// generated 取代已有 `w:p`：`ReplaceParaProps`（需要时）+ `ReplaceInlines`。
    fn generated_replace(
        &mut self,
        para: NodeId,
        blk: &Value,
        ops: &mut Vec<EditOp>,
    ) -> Result<()> {
        let current = self.dom.live_children_named(para, QName::w(LocalName::PPr)).next();
        let (props, inlines) = self.generated_paragraph(blk)?;
        let same_raw = match (s_of(blk, "rawPPr"), current) {
            (Some(raw), Some(c)) => {
                let node = self.dom.node(c);
                node.dirty == Dirty::Clean
                    && node.lex.as_ref().is_some_and(|l| self.dom.lex_str(&l.range) == raw)
            }
            (Some(raw), None) => raw.is_empty(),
            (None, _) => false,
        };
        let skip =
            same_raw || (blk.get("rawPPr").is_none() && props.is_none() && current.is_none());
        if !skip {
            ops.push(EditOp::ReplaceParaProps { part: None, para, props });
        }
        ops.push(EditOp::ReplaceInlines { part: None, para, inlines });
        Ok(())
    }

    /// TS `generateParagraphXml`：`(pPr, inlines)`。
    fn generated_paragraph(&mut self, blk: &Value) -> Result<(Option<NewElement>, Vec<NewInline>)> {
        if blk.get("pPrChange").is_some_and(|v| !v.is_null()) {
            return Err(unsupported("GeneratedBlock.pPrChange 在 M7"));
        }
        let props = match s_of(blk, "rawPPr") {
            Some("") => None,
            Some(raw) => {
                let mut frags = parse_fragment(self.dom, raw)
                    .map_err(|e| unsupported(format!("rawPPr 解析失败: {e}")))?;
                if frags.len() != 1 || frags[0].name != w(LocalName::PPr) {
                    return Err(unsupported("rawPPr 不是单个 w:pPr"));
                }
                Some(frags.remove(0))
            }
            None => self.para_props_from_format(blk),
        };
        let mut inlines = Vec::new();
        let bookmark = |name: &str| {
            let id = bookmark_id_of(name).to_string();
            [
                NewInline::Marker(NewMarker::BookmarkStart {
                    id: id.clone(),
                    name: name.to_string(),
                }),
                NewInline::Marker(NewMarker::BookmarkEnd { id }),
            ]
        };
        for k in ["hiddenBookmarks", "bookmarks"] {
            for n in blk.get(k).and_then(Value::as_array).into_iter().flatten() {
                if let Some(n) = n.as_str() {
                    inlines.extend(bookmark(n));
                }
            }
        }
        for id in blk.get("commentStarts").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = id.as_str() {
                inlines
                    .push(NewInline::Marker(NewMarker::CommentRangeStart { id: id.to_string() }));
            }
        }
        let runs = blk.get("runs").and_then(Value::as_array).cloned().unwrap_or_default();
        self.runs_to_inlines(&runs, &mut inlines)?;
        for id in blk.get("commentEnds").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = id.as_str() {
                inlines.push(NewInline::Marker(NewMarker::CommentRangeEnd { id: id.to_string() }));
                inlines.push(NewInline::Marker(NewMarker::CommentReference { id: id.to_string() }));
            }
        }
        Ok((props, inlines))
    }

    /// TS `generateParagraphXml` 无 `rawPPr` 分支 + `formatPPrChildren`。
    fn para_props_from_format(&self, blk: &Value) -> Option<NewElement> {
        let mut p = ParaProps::default();
        let ty = s_of(blk, "type").unwrap_or("paragraph");
        p.style = match ty {
            "heading" => {
                let level = num(blk, "level").map_or(1, |l| l as i64).clamp(1, 9) as u32;
                s_of(blk, "styleId")
                    .map(str::to_string)
                    .or_else(|| self.heading_ids.get(&level).cloned())
            }
            "listItem" => s_of(blk, "styleId")
                .map(str::to_string)
                .or_else(|| self.list_style.map(str::to_string)),
            _ => s_of(blk, "styleId").map(str::to_string),
        };
        if ty == "listItem"
            && let Some(list) = blk.get("list")
            && list.is_object()
        {
            let ilvl = num(list, "ilvl").map_or(0, |x| x as i64).clamp(0, 8) as i32;
            let num_id = match list.get("numId") {
                Some(Value::String(s)) => {
                    s.parse::<i32>().map_or_else(|_| Val::Raw(s.clone()), Val::Value)
                }
                Some(Value::Number(n)) => Val::Value(n.as_i64().unwrap_or(0) as i32),
                _ => Val::Raw(String::new()),
            };
            p.num = Some(NumPr {
                ilvl: Some(Val::Value(ilvl)),
                num_id: Some(num_id),
                ..Default::default()
            });
        }
        if let Some(f) = blk.get("format").filter(|f| f.is_object()) {
            format_into(f, &mut p);
        }
        (p != ParaProps::default()).then(|| emit_para_props(&p, self.flavor))
    }

    /// TS `runsXml`：批注范围、超链接分组、修订分组。
    fn runs_to_inlines(&mut self, runs: &[Value], out: &mut Vec<NewInline>) -> Result<()> {
        // 批注：每个 id 覆盖的首末 run
        let mut first_of: Vec<(String, usize)> = Vec::new();
        let mut last_of: HashMap<String, usize> = HashMap::new();
        for (i, r) in runs.iter().enumerate() {
            for id in r.get("commentIds").and_then(Value::as_array).into_iter().flatten() {
                let Some(id) = id.as_str() else { continue };
                if !first_of.iter().any(|(k, _)| k == id) {
                    first_of.push((id.to_string(), i));
                }
                last_of.insert(id.to_string(), i);
            }
        }
        let starts_at = |i: usize, out: &mut Vec<NewInline>| {
            for (id, at) in &first_of {
                if *at == i {
                    out.push(NewInline::Marker(NewMarker::CommentRangeStart { id: id.clone() }));
                }
            }
        };
        let ends_at = |i: usize, out: &mut Vec<NewInline>| {
            for (id, _) in &first_of {
                if last_of.get(id) == Some(&i) {
                    out.push(NewInline::Marker(NewMarker::CommentRangeEnd { id: id.clone() }));
                    out.push(NewInline::Marker(NewMarker::CommentReference { id: id.clone() }));
                }
            }
        };
        let rev_key = |r: &Value| -> Option<String> {
            let ins = r.get("ins").filter(|v| v.is_object());
            let del = r.get("del").filter(|v| v.is_object());
            if ins.is_none() && del.is_none() {
                return None;
            }
            let part = |v: Option<&Value>| {
                v.map_or("null".to_string(), |v| {
                    format!("{:?}|{:?}|{:?}", s_of(v, "author"), s_of(v, "date"), s_of(v, "id"))
                })
            };
            Some(format!("{}#{}", part(ins), part(del)))
        };
        let revision = |v: &Value| NewRevision {
            id: s_of(v, "id").map(str::to_string),
            author: s_of(v, "author").unwrap_or_default().to_string(),
            date: s_of(v, "date").map(str::to_string),
        };

        let mut g = 0;
        while g < runs.len() {
            let key = rev_key(&runs[g]);
            let mut end = g;
            while end < runs.len() && rev_key(&runs[end]) == key {
                end += 1;
            }
            let mut group_out = Vec::new();
            self.emit_range(runs, g, end, &starts_at, &ends_at, &mut group_out)?;
            match key {
                None => out.extend(group_out),
                Some(_) => {
                    let mut inner = group_out;
                    if let Some(del) = runs[g].get("del").filter(|v| v.is_object()) {
                        inner = vec![NewInline::Del { rev: revision(del), inlines: inner }];
                    }
                    if let Some(ins) = runs[g].get("ins").filter(|v| v.is_object()) {
                        inner = vec![NewInline::Ins { rev: revision(ins), inlines: inner }];
                    }
                    out.extend(inner);
                }
            }
            g = end;
        }
        Ok(())
    }

    /// TS `emitRange`：`[from, to)` 内的 run，超链接分组 + 批注标记。
    fn emit_range(
        &mut self,
        runs: &[Value],
        from: usize,
        to: usize,
        starts_at: &dyn Fn(usize, &mut Vec<NewInline>),
        ends_at: &dyn Fn(usize, &mut Vec<NewInline>),
        out: &mut Vec<NewInline>,
    ) -> Result<()> {
        let mut i = from;
        while i < to {
            let run = &runs[i];
            let link = run.get("link").filter(|l| l.is_object());
            if let Some(link) = link {
                let href = s_of(link, "href").unwrap_or_default().to_string();
                let start = i;
                let mut rid: Option<String> = None;
                let mut tooltip: Option<String> = None;
                while i < to
                    && runs[i]
                        .get("link")
                        .filter(|l| l.is_object())
                        .is_some_and(|l| s_of(l, "href") == Some(href.as_str()))
                {
                    let l = &runs[i]["link"];
                    if rid.is_none() {
                        rid = s_of(l, "rId").map(str::to_string);
                    }
                    if i == start {
                        tooltip = s_of(l, "tooltip").filter(|t| !t.is_empty()).map(str::to_string);
                    }
                    i += 1;
                }
                for j in start..i {
                    starts_at(j, out);
                }
                let target = if let Some(anchor) = href.strip_prefix('#') {
                    Some(NewLinkTarget::Anchor(anchor.to_string()))
                } else {
                    match rid.or_else(|| self.link_rels.get(&href).cloned()) {
                        Some(r) => Some(NewLinkTarget::Rel(r)),
                        None => {
                            return Err(unsupported(format!("新外部超链接 {href} 没有可用的关系")));
                        }
                    }
                };
                let mut inner = Vec::new();
                for r in &runs[start..i] {
                    self.run_fragment(r, true, &mut inner)?;
                }
                if let Some(target) = target {
                    out.push(NewInline::Hyperlink { target, tooltip, inlines: inner });
                }
                for j in start..i {
                    ends_at(j, out);
                }
            } else {
                starts_at(i, out);
                self.run_fragment(run, false, out)?;
                ends_at(i, out);
                i += 1;
            }
        }
        Ok(())
    }

    /// TS `runFragmentXml`。
    fn run_fragment(
        &mut self,
        run: &Value,
        inside_link: bool,
        out: &mut Vec<NewInline>,
    ) -> Result<()> {
        if let Some(omml) = run.get("math").and_then(|m| s_of(m, "omml")) {
            for e in parse_fragment(self.dom, omml)
                .map_err(|e| unsupported(format!("OMML 解析失败: {e}")))?
            {
                out.push(NewInline::Xml(e));
            }
            return Ok(());
        }
        if let Some(xml) = run.get("ruby").and_then(|m| s_of(m, "xml")) {
            let mut r = NewElement::new(w(LocalName::R));
            for e in parse_fragment(self.dom, xml)
                .map_err(|e| unsupported(format!("ruby 解析失败: {e}")))?
            {
                r.push_child(e);
            }
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        if let Some(xml) = run.get("image").and_then(|m| s_of(m, "xml")) {
            if s_of(run, "text").is_some_and(|t| !t.is_empty()) {
                let props = self.run_props(run, inside_link)?;
                out.push(NewInline::Run(NewRun {
                    text: s_of(run, "text").unwrap_or_default().to_string(),
                    props,
                }));
            }
            let mut r = NewElement::new(w(LocalName::R));
            for e in parse_fragment(self.dom, xml)
                .map_err(|e| unsupported(format!("image.xml 解析失败: {e}")))?
            {
                r.push_child(e);
            }
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        if let Some(note) = run.get("noteRef").filter(|n| n.is_object()) {
            let tag = if s_of(note, "kind") == Some("footnote") {
                LocalName::FootnoteReference
            } else {
                LocalName::EndnoteReference
            };
            let rpr = NewElement::new(w(LocalName::RPr)).with_child(
                NewElement::new(w(LocalName::VertAlign))
                    .with_attr(w(LocalName::Val), "superscript"),
            );
            let r = NewElement::new(w(LocalName::R)).with_child(rpr).with_child(
                NewElement::new(w(tag))
                    .with_attr(w(LocalName::Id), s_of(note, "id").unwrap_or_default()),
            );
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        // `FLD-12`：字段类 run 重新发成 begin / instrText / [separate] / 结果 / end
        if let Some(term) = s_of(run, "xeTerm") {
            // XE 是 `Marker` 策略：没有 separate 也没有结果
            out.push(NewInline::marker_field(format!(r#"XE "{term}""#)));
            return Ok(());
        }
        if run.get("refField").is_some_and(|v| !v.is_null()) {
            // 指令原文照发（`\r` `\h` 等开关必须逐字保留，`docs/03` §13）
            let instr = s_of(run, "refInstr")
                .map(str::to_string)
                .unwrap_or_else(|| format!("REF {}", s_of(run, "refField").unwrap_or_default()));
            let text = s_of(run, "text").unwrap_or_default();
            let props = self.run_props(run, inside_link)?;
            let result = if text.is_empty() {
                Vec::new()
            } else {
                vec![NewInline::Run(NewRun { text: text.to_string(), props })]
            };
            out.push(NewInline::field(instr, result));
            return Ok(());
        }
        for k in ["instrField", "fldBeginXml"] {
            if run.get(k).is_some_and(|v| !v.is_null()) {
                return Err(unsupported(format!(
                    "run.{k}：表单域 / 简单内联字段的重发要 begin run 原字节（M7）"
                )));
            }
        }
        let text = s_of(run, "text").unwrap_or_default();
        if text.is_empty() {
            return Ok(());
        }
        let props = self.run_props(run, inside_link)?;
        out.push(NewInline::Run(NewRun { text: text.to_string(), props }));
        Ok(())
    }

    /// TS `generateRunXml` 的 `rPr`：`rawRPr` → `mergeRPrModel`；否则 `modelRPrChildren`。
    fn run_props(&mut self, run: &Value, inside_link: bool) -> Result<Option<NewElement>> {
        let flavor = self.flavor;
        let Some(raw) = s_of(run, "rawRPr") else {
            let mut p = RunProps::default();
            model_into(run, inside_link, &mut p, false);
            let change = rpr_change_element(run);
            if p == RunProps::default() && change.is_none() {
                return Ok(None);
            }
            let mut out = emit_run_props(&p, flavor);
            if let Some(c) = change {
                out.push_child(c);
            }
            return Ok(Some(out));
        };
        let (tmp, tops) = parse_fragment_dom(self.dom, raw)
            .map_err(|e| unsupported(format!("rawRPr 解析失败: {e}")))?;
        let rpr = tops.first().copied().filter(|&n| tmp.is(n, w(LocalName::RPr)));
        let Some(rpr) = rpr else {
            // '<w:rPr/>' 或无法识别：按模型重建
            let mut p = RunProps::default();
            model_into(run, inside_link, &mut p, false);
            let change = rpr_change_element(run);
            if p == RunProps::default() && change.is_none() {
                return Ok(None);
            }
            let mut out = emit_run_props(&p, flavor);
            if let Some(c) = change {
                out.push_child(c);
            }
            return Ok(Some(out));
        };
        let mut diags = Vec::new();
        let mut p = read_run_props(&tmp, Some(rpr), &mut diags);
        let cs = truthy(run, "cs") || p.rtl == Some(true);
        merge_model(run, inside_link, cs, &mut p);
        // 未建模子元素：`w:rPrChange` 由模型接管（JSON 无 rPrChange → 丢弃），其余原位保留
        let raw_kids: Vec<NodeId> = tmp.children(rpr).to_vec();
        let mut extra: Vec<(u16, u8, NewElement)> = Vec::new();
        let mut last_idx = 0u16;
        for &c in &raw_kids {
            let Some(name) = tmp.name(c) else { continue };
            if let Some(idx) = order_index_run_props(name) {
                last_idx = idx;
            }
            if p.raw_unmodeled.contains(&c)
                && !tmp.is(c, w(LocalName::RPrChange))
                && let Some(e) = NewElement::from_dom(&tmp, c, self.dom.interner_mut())
            {
                extra.push((order_index_run_props(name).unwrap_or(last_idx), 1, e));
            }
        }
        p.raw_unmodeled.clear();
        let emitted = emit_run_props(&p, flavor);
        let mut all: Vec<(u16, u8, NewElement)> = emitted
            .child_elements()
            .map(|e| (order_index_run_props(e.name).unwrap_or(u16::MAX), 0, e.clone()))
            .collect();
        all.extend(extra);
        if let Some(change) = rpr_change_element(run) {
            all.push((order_index_run_props(change.name).unwrap_or(u16::MAX), 2, change));
        }
        all.sort_by_key(|(idx, sub, _)| (*idx, *sub));
        if all.is_empty() {
            return Ok(None);
        }
        let mut out = NewElement::new(w(LocalName::RPr));
        for (_, _, e) in all {
            out.push_child(e);
        }
        Ok(Some(out))
    }
}

/// `runs[].rPrChange` → `w:rPrChange`（TS `revisionRPrChangeXml`，任务 7.2b）。
///
/// 内层 `w:rPr` 只写 `old` 里建模的那几项，**不发 `bCs` / `iCs` 孪生**：`old.bold` 分不出
/// `w:b` 与 `w:b + w:bCs`，补上就是凭空造一个文档从来没有的复杂文种标志（TS 的注释同此）。
/// `w:id` 缺省仍按 `EDIT-06` 由引擎分配（TS 写 `0`，`INTENTIONAL` 已登记这条差异）。
fn rpr_change_element(run: &Value) -> Option<NewElement> {
    let change = run.get("rPrChange").filter(|v| v.is_object())?;
    let old = change.get("old").and_then(Value::as_object);
    let g = |k: &str| old.and_then(|o| o.get(k));
    let s_old = |k: &str| g(k).and_then(Value::as_str);
    let i_old = |k: &str| g(k).and_then(Value::as_i64);
    let b_old = |k: &str| g(k).and_then(Value::as_bool) == Some(true);
    let mut inner = NewElement::new(w(LocalName::RPr));
    fn val(inner: &mut NewElement, local: LocalName, v: &str) {
        inner.push_child(
            NewElement::new(w(local)).with_attr(QName::w(LocalName::Val), v.to_string()),
        );
    }
    if let Some(style) = s_old("styleId") {
        val(&mut inner, LocalName::RStyle, style);
    }
    let (font, ascii) = (s_old("font"), s_old("fontAscii"));
    if font.is_some() || ascii.is_some() {
        let a = ascii.or(font).unwrap_or_default().to_string();
        let mut f = NewElement::new(w(LocalName::RFonts));
        f.push_attr(QName::w(LocalName::Ascii), a.clone());
        if let Some(ea) = font {
            f.push_attr(QName::w(LocalName::EastAsia), ea.to_string());
        }
        f.push_attr(QName::w(LocalName::HAnsi), a.clone());
        f.push_attr(QName::w(LocalName::Cs), a);
        inner.push_child(f);
    }
    for (k, local) in
        [("bold", LocalName::B), ("italic", LocalName::I), ("strike", LocalName::Strike)]
    {
        if b_old(k) {
            inner.push_child(NewElement::new(w(local)));
        }
    }
    if let Some(c) = s_old("color") {
        val(&mut inner, LocalName::Color, c);
    }
    if let Some(n) = i_old("charSpacingTwips").filter(|&n| n != 0) {
        val(&mut inner, LocalName::Spacing, &n.to_string());
    }
    if let Some(n) = i_old("charScalePct").filter(|&n| n != 0) {
        val(&mut inner, LocalName::W, &n.to_string());
    }
    if let Some(n) = i_old("sizeHalfPoints").filter(|&n| n != 0) {
        val(&mut inner, LocalName::Sz, &n.to_string());
        val(&mut inner, LocalName::SzCs, &n.to_string());
    }
    if let Some(h) = s_old("highlight") {
        val(&mut inner, LocalName::Highlight, h);
    }
    if b_old("underline") {
        val(&mut inner, LocalName::U, "single");
    }
    if let Some(v) = s_old("vertAlign") {
        val(&mut inner, LocalName::VertAlign, v);
    }
    let mut e = NewElement::new(w(LocalName::RPrChange));
    if let Some(id) = change.get("id").and_then(Value::as_str) {
        e.push_attr(QName::w(LocalName::Id), id.to_string());
    }
    e.push_attr(
        QName::w(LocalName::Author),
        change.get("author").and_then(Value::as_str).unwrap_or_default().to_string(),
    );
    if let Some(d) = change.get("date").and_then(Value::as_str) {
        e.push_attr(QName::w(LocalName::Date), d.to_string());
    }
    e.push_child(inner);
    Some(e)
}

/// TS：`fb.revision` → 整块包进 `w:ins` / `w:del`（`id` 缺省 TS 写 `0`，这里按 `EDIT-06` 由引擎分配）。
/// TS `NewChart` → [`NewBlock::Chart`]。`extentPx {w, h}` → EMU（× 9525，至少 1）。
fn chart_block(chart: &Value, extent: Option<&Value>) -> Result<NewBlock> {
    let kind = match s_of(chart, "kind") {
        Some("bar") | None => NewChartKind::Bar,
        Some("line") => NewChartKind::Line,
        Some("pie") => NewChartKind::Pie,
        Some(other) => return Err(unsupported(format!("chart.kind {other:?}"))),
    };
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect())
            .unwrap_or_default()
    };
    let series = chart
        .get("series")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|ser| NewChartSeries {
                    name: s_of(ser, "name").unwrap_or_default().to_string(),
                    values: ser
                        .get("values")
                        .and_then(Value::as_array)
                        .map(|vs| vs.iter().map(Value::as_f64).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let extent_emu = extent.and_then(|e| {
        let px = |k: &str| e.get(k).and_then(Value::as_f64);
        Some((
            (px("w")? * 9525.0).round().max(1.0) as i64,
            (px("h")? * 9525.0).round().max(1.0) as i64,
        ))
    });
    Ok(NewBlock::Chart {
        chart: NewChart {
            kind,
            title: s_of(chart, "title").map(str::to_string),
            categories: strings(chart.get("categories")),
            series,
        },
        extent_emu,
    })
}

/// TS `replaceImage {base64, mime}` → 字节与 MIME。
fn replace_media_of(v: &Value) -> Result<(Vec<u8>, String)> {
    let b64 = s_of(v, "base64").ok_or_else(|| unsupported("replaceImage 缺 base64"))?;
    let bytes = crate::package::media::base64_decode(b64)
        .ok_or_else(|| unsupported("replaceImage.base64 不是 base64"))?;
    Ok((bytes, s_of(v, "mime").unwrap_or("image/png").to_string()))
}

/// TS `NewImage` → [`NewBlock::Image`]：px → EMU（× 9525），九种 `wrap`，`posOffsetEmu`，`zOrder`，旋转 / 翻转，`paraSpacing`。
fn image_block(v: &Value) -> Result<NewBlock> {
    let (bytes, mime) = replace_media_of(v)?;
    let num = |k: &str| v.get(k).and_then(Value::as_f64);
    let (w, h) = (num("widthPx").unwrap_or(1.0), num("heightPx").unwrap_or(1.0));
    let wrap = match s_of(v, "wrap") {
        None => None,
        Some(w) => {
            Some(w.parse::<ImageWrap>().map_err(|_| unsupported(format!("image.wrap {w:?}")))?)
        }
    };
    let pos_offset_emu = v.get("posOffsetEmu").filter(|p| p.is_object()).map(|p| PosOffset {
        x: p.get("x").and_then(Value::as_f64).unwrap_or(0.0).round() as i64,
        y: p.get("y").and_then(Value::as_f64).unwrap_or(0.0).round() as i64,
        page: s_of(p, "relativeTo") == Some("page"),
    });
    let para_spacing = v.get("paraSpacing").filter(|p| p.is_object()).map(|p| ParaSpacing {
        before_twips: p.get("beforeTwips").and_then(Value::as_f64).map(|x| x.round() as i64),
        after_twips: p.get("afterTwips").and_then(Value::as_f64).map(|x| x.round() as i64),
        line_twips: p.get("lineTwips").and_then(Value::as_f64).map(|x| x.round() as i64),
        line_rule: s_of(p, "lineRule").map(str::to_string),
    });
    Ok(NewBlock::Image(NewImage {
        bytes,
        mime,
        extent_emu: (
            i64::from(ImageExtentEmu::from(ImageExtentPx(w))),
            i64::from(ImageExtentEmu::from(ImageExtentPx(h))),
        ),
        align: s_of(v, "align").map(str::to_string),
        wrap,
        pos_offset_emu,
        z_order: num("zOrder").map(|z| z.round() as i64),
        rot_deg: num("rotDeg").map(|d| d.round() as i64),
        flip_h: truthy(v, "flipH"),
        flip_v: truthy(v, "flipV"),
        para_spacing,
    }))
}

/// TS `partXml` / `partBinary`：按 zip 路径整体替换 part。TS 对不存在的路径静默忽略；这里是
/// `EDIT_TARGET_MISSING`（`docs/04` §8）。
fn apply_part_replacements(session: &mut EditSession, lists: &EntryLists) -> Result<usize> {
    let mut ops = Vec::new();
    for (path, xml) in &lists.part_xml {
        let part = session.package().find_name(path).ok_or_else(|| {
            Error::edit(DiagCode::EditTargetMissing, format!("partXml: {path} 不在包里"))
        })?;
        ops.push(EditOp::ReplacePartXml { part, xml: xml.clone() });
    }
    for (path, b64) in &lists.part_binary {
        let part = session.package().find_name(path).ok_or_else(|| {
            Error::edit(DiagCode::EditTargetMissing, format!("partBinary: {path} 不在包里"))
        })?;
        let bytes = crate::package::media::base64_decode(b64)
            .ok_or_else(|| unsupported(format!("partBinary: {path} 不是 base64")))?;
        ops.push(EditOp::ReplacePartBytes { part, bytes });
    }
    let n = ops.len();
    if n > 0 {
        session.apply_all(ops, &EditContext::default())?;
    }
    Ok(n)
}

fn wrap_revision(block: NewBlock, revision: Option<&Value>) -> NewBlock {
    let Some(rev) = revision else { return block };
    let kind = if s_of(rev, "kind") == Some("del") { LocalName::Del } else { LocalName::Ins };
    let mut wrapper = NewElement::new(w(kind));
    if let Some(id) = s_of(rev, "id") {
        wrapper.push_attr(w(LocalName::Id), id);
    } else {
        wrapper.push_attr(w(LocalName::Id), String::new()); // 由 InsertBlock 分配
    }
    wrapper.push_attr(w(LocalName::Author), s_of(rev, "author").unwrap_or_default());
    if let Some(d) = s_of(rev, "date") {
        wrapper.push_attr(w(LocalName::Date), d);
    }
    NewBlock::Wrapped { wrapper, block: Box::new(block) }
}

fn hex(s: &str) -> Option<HexColorOrAuto> {
    HexColorOrAuto::parse(s)
}

fn hex_upper(c: &HexColorOrAuto) -> Option<String> {
    c.rgb().map(|[r, g, b]| format!("{r:02X}{g:02X}{b:02X}"))
}

/// TS `freshRFontsXml`。
fn fresh_fonts(font: Option<&str>, ascii: Option<&str>, cs: Option<&str>) -> Fonts {
    let a = ascii.or(font).or(cs).unwrap_or("").to_string();
    Fonts {
        ascii: Some(a.clone()),
        h_ansi: Some(a.clone()),
        east_asia: font.map(str::to_string),
        cs: Some(cs.map_or(a, str::to_string)),
        ..Default::default()
    }
}

/// TS `modelRPrChildren`：把 JSON run 的建模字段写进 `p`（`fresh_fonts` 由 `with_fonts` 控制）。
fn model_into(run: &Value, inside_link: bool, p: &mut RunProps, keep_fonts: bool) {
    p.style = if inside_link {
        Some("Hyperlink".to_string())
    } else {
        s_of(run, "styleId").map(str::to_string)
    };
    if !keep_fonts {
        let (font, ascii, cs) = (s_of(run, "font"), s_of(run, "fontAscii"), s_of(run, "fontCs"));
        p.fonts = (font.is_some() || ascii.is_some() || cs.is_some())
            .then(|| fresh_fonts(font, ascii, cs));
    }
    let b = truthy(run, "bold");
    p.bold = b.then_some(true);
    p.bold_cs = b.then_some(true);
    let i = truthy(run, "italic");
    p.italic = i.then_some(true);
    p.italic_cs = i.then_some(true);
    p.strike = truthy(run, "strike").then_some(true);
    p.color = s_of(run, "color")
        .and_then(hex)
        .map(|c| Color { val: Some(Val::Value(c)), ..Default::default() });
    let sz = num(run, "sizeHalfPoints").map(|x| x as u32).filter(|&x| x != 0);
    p.size = sz.map(Val::Value);
    p.size_cs = sz.map(Val::Value);
    p.highlight = s_of(run, "highlight")
        .map(|h| HighlightColor::parse(h).map_or_else(|| Val::Raw(h.to_string()), Val::Value));
    p.underline = truthy(run, "underline")
        .then(|| Underline { val: Some(Val::Value(UnderlineKind::Single)), ..Default::default() });
    p.shading = s_of(run, "shading").and_then(hex).map(|fill| Shading {
        val: Some(Val::Value(ShadingPattern::Clear)),
        color: Some(Val::Value(HexColorOrAuto::Auto)),
        fill: Some(Val::Value(fill)),
        ..Default::default()
    });
    p.vert_align = match s_of(run, "vertAlign") {
        Some("superscript") => Some(Val::Value(VerticalAlignRun::Superscript)),
        Some("subscript") => Some(Val::Value(VerticalAlignRun::Subscript)),
        _ => None,
    };
    p.rtl = truthy(run, "rtl").then_some(true);
}

/// TS `mergeRPrModel` 的分组比较：相等的组保留 `p` 里的原值，不等的组按模型重写。
fn merge_model(run: &Value, inside_link: bool, cs: bool, p: &mut RunProps) {
    let raw_bool = |v: Option<bool>| v == Some(true);
    // rStyle
    let modeled = if inside_link { Some("Hyperlink") } else { s_of(run, "styleId") };
    let raw_style = p.style.as_deref();
    if !(raw_style == modeled || (raw_style == Some("Hyperlink") && modeled.is_none())) {
        p.style = modeled.map(str::to_string);
    }
    // rFonts
    {
        let (font, ascii_m, cs_m) =
            (s_of(run, "font"), s_of(run, "fontAscii"), s_of(run, "fontCs"));
        let theme = run.get("themeRFonts").filter(|t| t.is_object());
        let t_font = theme.and_then(|t| s_of(t, "font"));
        let t_ascii = theme.and_then(|t| s_of(t, "fontAscii"));
        let f = p.fonts.clone();
        let raw_ascii: Option<String> =
            f.as_ref().and_then(|f| f.ascii.clone().or(f.h_ansi.clone()));
        let raw_primary: Option<String> =
            f.as_ref().and_then(|f| f.east_asia.clone()).or(raw_ascii.clone());
        let raw_cs: Option<String> = f.as_ref().and_then(|f| f.cs.clone());
        let equal = (raw_primary.as_deref() == font || (font.is_some() && font == t_font))
            && (raw_ascii.as_deref() == ascii_m || (ascii_m.is_some() && ascii_m == t_ascii))
            && (cs_m.is_none() || raw_cs.as_deref() == cs_m);
        if !equal {
            if let Some(mut rf) = f.filter(|_| font.is_some() || ascii_m.is_some()) {
                // mergeRFontsXml：只改模型持有的槽，去掉对应 theme 属性
                let had_ea = rf.east_asia.is_some() || rf.east_asia_theme.is_some();
                let raw_primary_owned = raw_primary.clone();
                if let Some(a) = ascii_m
                    && Some(a) != t_ascii
                {
                    rf.ascii = Some(a.to_string());
                    rf.h_ansi = Some(a.to_string());
                    rf.ascii_theme = None;
                    rf.h_ansi_theme = None;
                }
                if let Some(fo) = font
                    && Some(fo) != t_font
                    && (had_ea || Some(fo) != raw_primary_owned.as_deref())
                {
                    rf.east_asia = Some(fo.to_string());
                    rf.east_asia_theme = None;
                }
                if let Some(c) = cs_m {
                    rf.cs = Some(c.to_string());
                    rf.cs_theme = None;
                }
                p.fonts = Some(rf);
            } else {
                p.fonts = (font.is_some() || ascii_m.is_some() || cs_m.is_some())
                    .then(|| fresh_fonts(font, ascii_m, cs_m));
            }
        }
    }
    // bold / italic（rtl 时比较 Cs 孪生）
    let b = truthy(run, "bold");
    if raw_bool(if cs { p.bold_cs } else { p.bold }) != b {
        p.bold = b.then_some(true);
        p.bold_cs = b.then_some(true);
    }
    let it = truthy(run, "italic");
    if raw_bool(if cs { p.italic_cs } else { p.italic }) != it {
        p.italic = it.then_some(true);
        p.italic_cs = it.then_some(true);
    }
    let st = truthy(run, "strike");
    if raw_bool(p.strike) != st {
        p.strike = st.then_some(true);
    }
    // color
    let raw_color = p.color.as_ref().and_then(|c| c.val.as_ref()).and_then(|v| match v {
        Val::Value(c) => hex_upper(c),
        Val::Raw(s) => Some(s.clone()),
    });
    let model_color = s_of(run, "color");
    if !raw_color
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
        .eq(&model_color.map(str::to_ascii_uppercase).as_deref())
    {
        p.color = model_color
            .and_then(hex)
            .map(|c| Color { val: Some(Val::Value(c)), ..Default::default() });
    }
    // size
    let raw_size = (if cs { &p.size_cs } else { &p.size })
        .as_ref()
        .and_then(|v| v.value().copied())
        .filter(|&x| x != 0);
    let model_size = num(run, "sizeHalfPoints").map(|x| x as u32).filter(|&x| x != 0);
    if raw_size != model_size {
        p.size = model_size.map(Val::Value);
        p.size_cs = model_size.map(Val::Value);
    }
    // highlight
    let raw_hl = p.highlight.as_ref().and_then(|v| match v {
        Val::Value(HighlightColor::None) => None,
        Val::Value(h) => Some(h.as_str().to_string()),
        Val::Raw(s) => Some(s.clone()),
    });
    if raw_hl.as_deref() != s_of(run, "highlight") {
        p.highlight = s_of(run, "highlight")
            .map(|h| HighlightColor::parse(h).map_or_else(|| Val::Raw(h.to_string()), Val::Value));
    }
    // shading
    let raw_shd = p.shading.as_ref().and_then(|s| s.fill.as_ref()).and_then(|v| match v {
        Val::Value(c) => hex_upper(c),
        Val::Raw(s) => Some(s.clone()),
    });
    if raw_shd.as_deref().map(str::to_ascii_uppercase)
        != s_of(run, "shading").map(str::to_ascii_uppercase)
    {
        p.shading = s_of(run, "shading").and_then(hex).map(|fill| Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            color: Some(Val::Value(HexColorOrAuto::Auto)),
            fill: Some(Val::Value(fill)),
            ..Default::default()
        });
    }
    // underline：有 w:val 且不是 none
    let raw_u = p
        .underline
        .as_ref()
        .and_then(|u| u.val.as_ref())
        .is_some_and(|v| *v != Val::Value(UnderlineKind::None));
    if raw_u != truthy(run, "underline") {
        p.underline = truthy(run, "underline").then(|| Underline {
            val: Some(Val::Value(UnderlineKind::Single)),
            ..Default::default()
        });
    }
    // vertAlign
    let raw_va = match p.vert_align.as_ref() {
        Some(Val::Value(VerticalAlignRun::Superscript)) => Some("superscript"),
        Some(Val::Value(VerticalAlignRun::Subscript)) => Some("subscript"),
        _ => None,
    };
    if raw_va != s_of(run, "vertAlign") {
        p.vert_align = match s_of(run, "vertAlign") {
            Some("superscript") => Some(Val::Value(VerticalAlignRun::Superscript)),
            Some("subscript") => Some(Val::Value(VerticalAlignRun::Subscript)),
            _ => None,
        };
    }
    // rtl
    let r = truthy(run, "rtl");
    if raw_bool(p.rtl) != r {
        p.rtl = r.then_some(true);
    }
}

/// TS `formatPPrChildren`（`ParaFormat` → `ParaProps` 字段）。
fn format_into(f: &Value, p: &mut ParaProps) {
    if truthy(f, "pageBreakBefore") {
        p.page_break_before = Some(true);
    }
    if let Some(sides) = s_of(f, "borders") {
        let style = f.get("borderStyle").filter(|s| s.is_object());
        let default_sz =
            style.and_then(|s| num(s, "szEighths")).map_or(4, |x| round(x).max(2)) as u32;
        let space =
            style.and_then(|s| num(s, "spacePt")).map_or(1, |x| round(x).clamp(0, 31)) as u32;
        let default_color =
            style.and_then(|s| s_of(s, "color")).and_then(hex).unwrap_or(HexColorOrAuto::Auto);
        let lines = f.get("borderLines").filter(|l| l.is_object());
        let line = |ch: &str| -> Border {
            let declared = lines.and_then(|l| l.get(ch)).filter(|d| d.is_object());
            let sz = declared
                .and_then(|d| num(d, "szPt"))
                .filter(|&x| x != 0.0)
                .map_or(default_sz, |x| round(x * 8.0).max(1) as u32);
            let color =
                declared.and_then(|d| s_of(d, "color")).and_then(hex).unwrap_or(default_color);
            Border {
                val: Some(Val::Value(BorderStyle::Single)),
                sz: Some(Val::Value(sz)),
                space: Some(Val::Value(space)),
                color: Some(Val::Value(color)),
                ..Default::default()
            }
        };
        let mut b = ParaBorders::default();
        if sides.contains('t') {
            b.top = Some(line("t"));
        }
        if sides.contains('l') {
            b.left = Some(line("l"));
        }
        if sides.contains('b') {
            b.bottom = Some(line("b"));
        }
        if sides.contains('r') {
            b.right = Some(line("r"));
        }
        if b != ParaBorders::default() {
            p.borders = Some(b);
        }
    }
    if let Some(fill) = s_of(f, "shadingFill").and_then(hex) {
        p.shading = Some(Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            color: Some(Val::Value(HexColorOrAuto::Auto)),
            fill: Some(Val::Value(fill)),
            ..Default::default()
        });
    }
    let bidi = truthy(f, "bidi");
    if bidi {
        p.bidi = Some(true);
    }
    // spacing
    {
        let mut sp = Spacing::default();
        if let Some(b) = num(f, "spaceBefore").filter(|&x| x > 0.0) {
            sp.before = Some(Val::Value(round(b)));
        }
        if let Some(Value::Bool(a)) = f.get("spaceBeforeAuto") {
            sp.before_autospacing = Some(*a);
        }
        if let Some(a) = num(f, "spaceAfter").filter(|&x| x >= 0.0) {
            sp.after = Some(Val::Value(round(a)));
        }
        if let Some(Value::Bool(a)) = f.get("spaceAfterAuto") {
            sp.after_autospacing = Some(*a);
        }
        let rule = s_of(f, "lineRule");
        if matches!(rule, Some("exact" | "atLeast"))
            && let Some(raw) = num(f, "lineRawTwips").filter(|&x| x != 0.0)
        {
            sp.line = Some(Val::Value(round(raw)));
            sp.line_rule = Some(Val::Value(if rule == Some("exact") {
                LineSpacingRule::Exact
            } else {
                LineSpacingRule::AtLeast
            }));
        } else if let Some(ls) = num(f, "lineSpacing").filter(|&x| x > 0.0) {
            sp.line = Some(Val::Value(round(ls * 240.0)));
            sp.line_rule = Some(Val::Value(LineSpacingRule::Auto));
        }
        if sp != Spacing::default() {
            p.spacing = Some(sp);
        }
    }
    // ind
    {
        let mut ind = Indent::default();
        if let Some(l) = num(f, "indentLeft") {
            ind.start = Some(Val::Value(round(l)));
        }
        if let Some(r) = num(f, "indentRight").filter(|&x| x != 0.0) {
            ind.end = Some(Val::Value(round(r)));
        }
        if let Some(fl) = num(f, "indentFirstLine").filter(|&x| x != 0.0) {
            if fl > 0.0 {
                ind.first_line = Some(Val::Value(round(fl)));
            } else {
                ind.hanging = Some(Val::Value(round(-fl)));
            }
        }
        if ind != Indent::default() {
            p.indent = Some(ind);
        }
    }
    if let Some(align) = s_of(f, "align") {
        let mut jc = if align == "justify" { "both" } else { align };
        if bidi && (jc == "left" || jc == "right") {
            jc = if jc == "left" { "right" } else { "left" };
        }
        p.jc = Some(Jc::parse(jc).map_or_else(|| Val::Raw(jc.to_string()), Val::Value));
    }
    if let Some(stops) = f.get("tabStops").and_then(Value::as_array) {
        let tabs: Vec<Tab> = stops
            .iter()
            .filter(|ts| !truthy(ts, "rel"))
            .map(|ts| Tab {
                val: s_of(ts, "val")
                    .map(|v| TabJc::parse(v).map_or_else(|| Val::Raw(v.to_string()), Val::Value)),
                pos: num(ts, "pos").map(|x| Val::Value(round(x))),
                leader: s_of(ts, "leader").filter(|l| *l != "none").map(|l| {
                    TabLeader::parse(l).map_or_else(|| Val::Raw(l.to_string()), Val::Value)
                }),
            })
            .collect();
        if !tabs.is_empty() {
            p.tabs = Some(Tabs { tab: tabs, ..Default::default() });
        }
    }
    if let Some(fr) = f.get("frame").filter(|x| x.is_object()) {
        let anchor = |k: &str| {
            let s = s_of(fr, k).unwrap_or("page");
            Some(FrameAnchor::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value))
        };
        let mut fp = FramePr {
            w: num(fr, "wTwips").map(|x| Val::Value(round(x))),
            wrap: Some({
                let s = s_of(fr, "wrap").unwrap_or("none");
                FrameWrap::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value)
            }),
            v_anchor: anchor("vAnchor"),
            h_anchor: anchor("hAnchor"),
            x: num(fr, "xTwips").map(|x| Val::Value(round(x))),
            y: num(fr, "yTwips").map(|x| Val::Value(round(x))),
            ..Default::default()
        };
        if let Some(h) = num(fr, "hTwips") {
            fp.h = Some(Val::Value(round(h)));
            let s = s_of(fr, "hRule").unwrap_or("atLeast");
            fp.h_rule =
                Some(HeightRule::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value));
        }
        p.frame = Some(fp);
    } else if let Some(dc) = f.get("dropCap").filter(|x| x.is_object()) {
        let ty = s_of(dc, "type").unwrap_or("drop");
        p.frame = Some(FramePr {
            drop_cap: Some(DropCap::parse(ty).map_or_else(|| Val::Raw(ty.to_string()), Val::Value)),
            lines: num(dc, "lines").map(|x| Val::Value(round(x))),
            wrap: Some(Val::Value(FrameWrap::Around)),
            v_anchor: Some(Val::Value(FrameAnchor::Text)),
            h_anchor: Some(Val::Value(FrameAnchor::Text)),
            ..Default::default()
        });
    }
    if let Some(sz) = num(f, "emptyRunSizeHalfPoints").filter(|&x| x != 0.0) {
        let sz = round(sz) as u32;
        p.rpr = Some(RunProps {
            size: Some(Val::Value(sz)),
            size_cs: Some(Val::Value(sz)),
            ..Default::default()
        });
    }
}

// 让未使用的导入在功能面变化时报错而不是静默
#[allow(dead_code)]
fn _types(_: FontHint, _: NsId) {}
// ---------------------------------------------------------------------------
// 页眉页脚保存选项：TS `HeaderFooter` → `Vec<NewBlock>`（`spec/16` 任务 5.6b）
// ---------------------------------------------------------------------------

/// TS `headerFooterPartXml` 的 PAGE / NUMPAGES 字段：五个 run，结果缓存写 `1`。
fn page_field(keyword: &str) -> NewInline {
    NewInline::Field {
        instr: keyword.to_string(),
        result: vec![NewInline::Run(NewRun::text("1"))],
        separate: true,
        dirty: false,
        props: None,
    }
}

/// 一段文本按 `TOTAL_PAGES_MARK` 切开，段间插 NUMPAGES 字段（TS `textWithTotal`）。
fn text_with_total(text: &str, props: Option<&NewElement>, out: &mut Vec<NewInline>) {
    for (i, seg) in text.split(super::hf::TOTAL_PAGES_MARK).enumerate() {
        if i > 0 {
            out.push(page_field("NUMPAGES"));
        }
        if !seg.is_empty() {
            out.push(NewInline::Run(NewRun { text: seg.to_string(), props: props.cloned() }));
        }
    }
}

/// 居中的一段（TS 的 `<w:p><w:pPr><w:jc w:val="center"/></w:pPr>…</w:p>`）。
fn centered_para(inlines: Vec<NewInline>) -> NewBlock {
    let props = NewElement::new(w(LocalName::PPr))
        .with_child(NewElement::new(w(LocalName::Jc)).with_attr(w(LocalName::Val), "center"));
    NewBlock::Paragraph { props: Some(props), inlines }
}

/// `HeaderFooter` 里有没有真的页码标记（含表格行的段落，TS `hasPageMark`）。
fn has_page_mark(paras: &[Value]) -> bool {
    let run_has = |runs: Option<&Vec<Value>>| {
        runs.into_iter()
            .flatten()
            .any(|r| s_of(r, "text").is_some_and(|t| t.contains(super::hf::PAGE_MARK)))
    };
    paras.iter().any(|p| {
        run_has(p.get("runs").and_then(Value::as_array))
            || p.get("cells").and_then(Value::as_array).into_iter().flatten().any(|c| {
                c.get("paras")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|cp| run_has(cp.get("runs").and_then(Value::as_array)))
            })
    })
}

impl Planner<'_> {
    /// TS `headerFooterPartXml` 的内容部分 → `Vec<NewBlock>`（外层的 part 合并在 `save/options/hf.rs`）。
    ///
    /// 两个分支与 TS 同：给了 `paras` 就逐条发（带 `cells` 的**跳过**——那是表格行的显示形态，
    /// 原 `w:tbl` 字节由外科合并保留），没给就一段居中的 `text` + 可选页码。
    fn hf_blocks(&mut self, hf: &Value) -> Result<Vec<NewBlock>> {
        let text = s_of(hf, "text").unwrap_or_default().to_string();
        let page_number = truthy(hf, "pageNumber");
        let Some(paras) = hf.get("paras").and_then(Value::as_array) else {
            let mut inlines = Vec::new();
            if text.contains(super::hf::PAGE_MARK) {
                for (i, seg) in text.split(super::hf::PAGE_MARK).enumerate() {
                    if i > 0 {
                        inlines.push(page_field("PAGE"));
                    }
                    text_with_total(seg, None, &mut inlines);
                }
            } else if page_number && text.contains('#') {
                let (before, rest) = text.split_once('#').expect("含 #");
                text_with_total(before, None, &mut inlines);
                inlines.push(page_field("PAGE"));
                text_with_total(rest, None, &mut inlines);
            } else {
                if !text.is_empty() {
                    // TS：有页码时正文后补一个空格再接字段
                    let t = if page_number { format!("{text} ") } else { text };
                    text_with_total(&t, None, &mut inlines);
                }
                if page_number {
                    inlines.push(page_field("PAGE"));
                }
            }
            return Ok(vec![centered_para(inlines)]);
        };

        // `pageNumber` 且一个真标记都没有时，第一个字面 `#` 顶替页码（只顶替第一个）
        let mut page_emitted = !page_number || has_page_mark(paras);
        let mut out = Vec::new();
        for para in paras.iter().filter(|p| p.get("cells").is_none_or(Value::is_null)) {
            let mut p = ParaProps::default();
            format_into(para, &mut p);
            let props = (p != ParaProps::default()).then(|| emit_para_props(&p, self.flavor));
            let mut inlines = Vec::new();
            for run in para.get("runs").and_then(Value::as_array).into_iter().flatten() {
                let t = s_of(run, "text").unwrap_or_default();
                let marked =
                    t.contains(super::hf::TOTAL_PAGES_MARK) || t.contains(super::hf::PAGE_MARK);
                if !marked && (page_emitted || !t.contains('#')) {
                    self.runs_to_inlines(std::slice::from_ref(run), &mut inlines)?;
                    continue;
                }
                let rpr = rich_run_props(run);
                for (k, seg) in t.split(super::hf::TOTAL_PAGES_MARK).enumerate() {
                    if k > 0 {
                        inlines.push(page_field("NUMPAGES"));
                    }
                    if seg.contains(super::hf::PAGE_MARK) {
                        for (j, piece) in seg.split(super::hf::PAGE_MARK).enumerate() {
                            if j > 0 {
                                inlines.push(page_field("PAGE"));
                            }
                            if !piece.is_empty() {
                                inlines.push(NewInline::Run(NewRun {
                                    text: piece.to_string(),
                                    props: rpr.clone(),
                                }));
                            }
                        }
                    } else if !page_emitted && seg.contains('#') {
                        let (before, rest) = seg.split_once('#').expect("含 #");
                        if !before.is_empty() {
                            inlines.push(NewInline::Run(NewRun {
                                text: before.to_string(),
                                props: rpr.clone(),
                            }));
                        }
                        inlines.push(page_field("PAGE"));
                        if !rest.is_empty() {
                            inlines.push(NewInline::Run(NewRun {
                                text: rest.to_string(),
                                props: rpr.clone(),
                            }));
                        }
                        page_emitted = true;
                    } else if !seg.is_empty() {
                        inlines.push(NewInline::Run(NewRun {
                            text: seg.to_string(),
                            props: rpr.clone(),
                        }));
                    }
                }
            }
            out.push(NewBlock::Paragraph { props, inlines });
        }
        // 一条都没发出页码：补一段居中的纯页码（同 TS）
        if !page_emitted {
            out.push(centered_para(vec![page_field("PAGE")]));
        }
        Ok(out)
    }
}

impl Planner<'_> {
    /// TS `sectionHf[]` 的一条：内容 + 它落在第几个块（`lastBlockIndex`）。
    ///
    /// 节点**不在这里**解析：块操作可能整段重发，那时早先算出的 `w:sectPr` 节点已经在一棵
    /// 删掉的子树里了。索引 → 节点留到所有块操作之后（见 [`resolve_section_hf`]）。
    ///
    /// 变体固定是 default——TS 找引用时也只认 `default` / 非 schema 的 `odd` / 无 `w:type`
    /// （`sectionHf` 表达不了 first / even）。
    fn section_hf_of(&mut self, e: &Value) -> Result<(usize, HfKind, Vec<NewBlock>)> {
        let idx = e
            .get("lastBlockIndex")
            .and_then(Value::as_u64)
            .ok_or_else(|| unsupported("sectionHf 条目缺 lastBlockIndex"))?
            as usize;
        let kind = match s_of(e, "kind") {
            Some("footer") => HfKind::Footer,
            _ => HfKind::Header,
        };
        let hf = e.get("hf").ok_or_else(|| unsupported("sectionHf 条目缺 hf"))?;
        Ok((idx, kind, self.hf_blocks(hf)?))
    }
}

/// 块操作之后把 `lastBlockIndex` 解析成 `w:sectPr` 节点。
fn resolve_section_hf(
    session: &EditSession,
    pending: Vec<(usize, HfKind, Vec<NewBlock>)>,
) -> Result<Vec<SectionHfSave>> {
    if pending.is_empty() {
        return Ok(Vec::new());
    }
    let body = session
        .document()
        .body
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "文档没有 w:body"))?;
    let nodes = blocks::element_nodes(session.dom(), body);
    pending
        .into_iter()
        .map(|(idx, kind, blocks)| {
            let node = *nodes
                .get(idx)
                .ok_or_else(|| unsupported(format!("sectionHf.lastBlockIndex {idx} 越界")))?;
            let sect = sect_pr_in(session.dom(), node)
                .ok_or_else(|| unsupported(format!("第 {idx} 块里没有 w:sectPr")))?;
            Ok(SectionHfSave { sect, kind, variant: HfVariant::Default, blocks })
        })
        .collect()
}

/// TS `inks`：块操作之后把每条的 `blockIndex`（finalBlocks 下标）解析成锚点节点（TS 逐个最终块注入）。
/// 锚点不是段落的条目照样传下去——`InsertInk` 自己跳过并记诊断，不分配媒体（TS `^<w:p` 检查在分配之前）。
fn resolve_inks(session: &EditSession, inks: Option<&[Value]>) -> Result<Option<Vec<InkSave>>> {
    let Some(list) = inks else { return Ok(None) };
    let body = session
        .document()
        .body
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "文档没有 w:body"))?;
    let nodes = blocks::element_nodes(session.dom(), body);
    list.iter()
        .map(|v| {
            let idx =
                v["blockIndex"].as_u64().ok_or_else(|| unsupported("inks 条目缺 blockIndex"))?
                    as usize;
            let para = *nodes.get(idx).ok_or_else(|| {
                Error::edit(DiagCode::EditBadPosition, format!("inks.blockIndex {idx} 越界"))
            })?;
            let b64 = s_of(v, "base64").ok_or_else(|| unsupported("inks 条目缺 base64"))?;
            let png = crate::package::media::base64_decode(b64)
                .ok_or_else(|| unsupported("inks.base64 不是合法的 base64"))?;
            let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
            Ok(InkSave {
                para,
                ink: NewInk {
                    png,
                    width_px: f("widthPx"),
                    height_px: f("heightPx"),
                    offset_x_px: f("offsetXPx"),
                    offset_y_px: f("offsetYPx"),
                    payload: v.get("payload").and_then(Value::as_str).map(str::to_string),
                },
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// 一个块级元素里的 `w:sectPr`：自己就是，或者在 `w:pPr` 里。
fn sect_pr_in(dom: &Dom, node: NodeId) -> Option<NodeId> {
    if dom.is(node, w(LocalName::SectPr)) {
        return Some(node);
    }
    let ppr = dom.semantic_children(node).find(|&c| dom.is(c, w(LocalName::PPr)))?;
    dom.semantic_children(ppr).find(|&c| dom.is(c, w(LocalName::SectPr)))
}
// ---------------------------------------------------------------------------
// 声明 part 的保存选项：TS JSON → `SaveOptions`（`spec/16` 任务 5.7）
// ---------------------------------------------------------------------------

/// TS `SourceInfo` → [`SourceSave`]。
fn source_save_of(v: &Value) -> Result<SourceSave> {
    let tag = s_of(v, "tag").ok_or_else(|| unsupported("sources 条目缺 tag"))?;
    Ok(SourceSave {
        tag: tag.to_string(),
        kind: s_of(v, "type").unwrap_or("Misc").to_string(),
        author: s_of(v, "author").unwrap_or_default().to_string(),
        title: s_of(v, "title").unwrap_or_default().to_string(),
        year: s_of(v, "year").unwrap_or_default().to_string(),
        publisher: s_of(v, "publisher").map(str::to_string),
        url: s_of(v, "url").map(str::to_string),
    })
}

/// TS `numbering` → 两张只追加的表。`numId` 由调用方给（正文的 `w:numPr` 已经在引用它）。
fn numbering_of(v: &Value, out: &mut SaveOptions) -> Result<()> {
    for def in v.get("newDefs").and_then(Value::as_array).into_iter().flatten() {
        let num_id = num_id_of(def).ok_or_else(|| unsupported("numbering.newDefs 缺 numId"))?;
        let levels = def
            .get("levels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|l| NumberingLevelSave {
                num_fmt: s_of(l, "numFmt").unwrap_or("decimal").to_string(),
                lvl_text: s_of(l, "lvlText").unwrap_or_default().to_string(),
                indent_left: num(l, "indentLeft").map_or(720, round),
                hanging: num(l, "hanging").map(round),
                start: num(l, "start").map(round),
            })
            .collect();
        out.numbering_new_defs.push(NumberingDefSave {
            num_id,
            bullet: s_of(def, "kind") == Some("bullet"),
            levels,
        });
    }
    for r in v.get("restartNums").and_then(Value::as_array).into_iter().flatten() {
        let num_id = num_id_of(r).ok_or_else(|| unsupported("numbering.restartNums 缺 numId"))?;
        let abstract_num_id = r
            .get("abstractNumId")
            .and_then(json_id)
            .ok_or_else(|| unsupported("numbering.restartNums 缺 abstractNumId"))?;
        let start_overrides = r
            .get("startOverrides")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.parse::<i32>().ok()?, v.as_i64()? as i32)))
                    .collect()
            })
            .unwrap_or_default();
        out.numbering_restart_nums.push(RestartNumSave {
            num_id,
            abstract_num_id,
            start_overrides,
        });
    }
    Ok(())
}

/// TS 里这些 id 有时是字符串、有时是数字。
fn json_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn num_id_of(v: &Value) -> Option<String> {
    v.get("numId").and_then(json_id)
}

fn theme_fonts_of(v: &Value) -> ThemeFontsSave {
    ThemeFontsSave {
        major: s_of(v, "major").unwrap_or_default().to_string(),
        minor: s_of(v, "minor").unwrap_or_default().to_string(),
        east_asia: s_of(v, "eastAsia").map(str::to_string),
    }
}

/// TS `ThemeColors`：`name` 之外的键都是槽名，只收可写的那八个（`dk1` / `lt1` / `hlink` /
/// `folHlink` 是只读槽，TS 的 `applyThemeColors` 也不改）。
fn theme_colors_of(v: &Value) -> ThemeColorsSave {
    let mut out = ThemeColorsSave { name: s_of(v, "name").map(str::to_string), slots: Vec::new() };
    for slot in crate::save::options::decl::THEME_COLOR_SLOTS {
        if let Some(hex) = s_of(v, slot) {
            out.slots.push((slot.to_string(), hex.to_string()));
        }
    }
    out
}

/// TS `StyleUpsert` → [`StyleUpsertSave`]：`rPr` / `pPr` 进属性表结构体，emit 由 `PROP-05` 排序。
fn style_upsert_of(v: &Value) -> Result<StyleUpsertSave> {
    let style_id = s_of(v, "styleId").ok_or_else(|| unsupported("styleUpserts 条目缺 styleId"))?;
    let mut run_props = RunProps::default();
    if let Some(r) = v.get("rPr").filter(|r| r.is_object()) {
        if let Some(f) = s_of(r, "font") {
            run_props.fonts = Some(Fonts {
                ascii: Some(f.to_string()),
                h_ansi: Some(f.to_string()),
                east_asia: Some(f.to_string()),
                ..Default::default()
            });
        }
        if truthy(r, "bold") {
            run_props.bold = Some(true);
        }
        if truthy(r, "italic") {
            run_props.italic = Some(true);
        }
        if truthy(r, "strike") {
            run_props.strike = Some(true);
        }
        if let Some(c) = s_of(r, "color").and_then(hex) {
            run_props.color = Some(Color { val: Some(Val::Value(c)), ..Default::default() });
        }
        if let Some(sz) = num(r, "sizeHalfPoints").filter(|&x| x > 0.0) {
            let v = Val::Value(round(sz) as u32);
            run_props.size = Some(v.clone());
            run_props.size_cs = Some(v);
        }
        if truthy(r, "underline") {
            run_props.underline = Some(Underline {
                val: Some(Val::Value(UnderlineKind::Single)),
                ..Default::default()
            });
        }
    }
    let mut para_props = ParaProps::default();
    if let Some(p) = v.get("pPr").filter(|p| p.is_object()) {
        let mut sp = Spacing::default();
        if let Some(b) = num(p, "spaceBeforeTwips") {
            sp.before = Some(Val::Value(round(b)));
        }
        if let Some(af) = num(p, "spaceAfterTwips") {
            sp.after = Some(Val::Value(round(af)));
        }
        if let Some(ls) = num(p, "lineSpacing") {
            sp.line = Some(Val::Value(round(ls * 240.0)));
            sp.line_rule = Some(Val::Value(LineSpacingRule::Auto));
        }
        if sp != Spacing::default() {
            para_props.spacing = Some(sp);
        }
        if let Some(align) = s_of(p, "align") {
            let jc = if align == "justify" { "both" } else { align };
            para_props.jc =
                Some(Jc::parse(jc).map_or_else(|| Val::Raw(jc.to_string()), Val::Value));
        }
    }
    Ok(StyleUpsertSave {
        style_id: style_id.to_string(),
        kind: s_of(v, "type").unwrap_or("paragraph").to_string(),
        name: s_of(v, "name").unwrap_or(style_id).to_string(),
        based_on: s_of(v, "basedOn").map(str::to_string),
        run_props: (run_props != RunProps::default()).then_some(run_props),
        para_props: (para_props != ParaProps::default()).then_some(para_props),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_08_bookmark_id_matches_ts_hash() {
        // TS: bookmarkIdOf('_Ref12345') 与 bookmarkIdOf('市场规模')；值由 node 计算得到
        assert_eq!(bookmark_id_of(""), 0);
        assert_eq!(bookmark_id_of("a"), 97);
        assert_eq!(bookmark_id_of("ab"), 97 * 31 + 98);
        let long = bookmark_id_of("_Ref12345678901234567890");
        assert!(long < 0x7fff_ffff);
    }
}
