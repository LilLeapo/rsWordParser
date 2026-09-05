//! 页眉页脚的 TS 投影（`COMPAT-05`，`docs/01` §9，`spec/16` 任务 5.4）。
//!
//! 三个来源：
//!
//! - `hfParts{rId}`：主 part 每个 header / footer 关系一条（TS `parseAllHfParts`）。
//! - 顶层 `headerText` / `headerParas` / … ：**default 变体**那一个 part（TS `readHeaderFooterPart`）。
//! - `headerFirst` / `footerEven` / … ：对应 `w:type` 的 part。
//!
//! default 变体的选法照 TS：在**整个 `document.xml`** 里按文档序找 `w:headerReference`，
//! 先取 `w:type="default"`，否则 `w:type="odd"`（非 schema，Word 的"缺省页"），否则没有 `w:type` 的。
//! 注意这是"全文第一个"，**不是**按节——多节文档里它来自第一节。模型侧按节记引用
//! （`SectionInfo.hf_ref` + `RES-10` 继承），两者不冲突：这里复现的是 TS 的顶层字段。
//!
//! `text` 是 TS 的 `plainText(cleaned)`，规则与坐标流**不同**，所以这里直接走 part 的 DOM：
//!
//! | TS | 我们 |
//! | --- | --- |
//! | 只取 `w:t`（不含 `w:delText` / `w:instrText`） | 同 |
//! | `w:t` 原文，不按 `xml:space` 去空白 | 同（坐标流会去，这里不去） |
//! | `</w:tc>` 后若还有文字，补一个空格 | 同 |
//! | `PAGE` / `NUMPAGES` 字段 → `PAGE_MARK` / `TOTAL_PAGES_MARK`，丢掉缓存结果 | 同（按字段索引，不是正则） |
//! | 其他字段 → 只留 `separate` 之后的结果 run | 同（跳过 begin..separate） |
//! | 旧式 `w:pgNum` → `PAGE_MARK` | 同 |
//! | `mc:Fallback` 整块删掉 | 语义遍历本来就只走 Choice（`XML-09`） |

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::bind::compat_ts::blocks::set;
use crate::model::{Document, HfKind, HfPart, HfVariant};
use crate::span::field::{FieldForm, Keyword};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// TS `PAGE_MARK`：页码字段的位置。私用区字符，这样 part 里字面的 `#` 不会被当成页码。
pub(super) const PAGE_MARK: char = '\u{E001}';
/// TS `TOTAL_PAGES_MARK`：总页数字段的位置。
pub(super) const TOTAL_PAGES_MARK: char = '\u{E000}';

/// 一个 part 的投影结果（`HfPartInfo`）。
struct PartInfo {
    text: String,
    has_page_number: bool,
    watermark: Option<String>,
}

/// 把页眉页脚的全部顶层键写进 `out`。
pub(super) fn hf_json(
    dom: &Dom,
    doc: &Document,
    hf_doms: &BTreeMap<crate::package::PartId, &Dom>,
    out: &mut Map<String, Value>,
) {
    // 每个 part 投影一次（多个 rId 可能指向同一个 part）
    let infos: BTreeMap<crate::package::PartId, PartInfo> = doc
        .hf_parts
        .iter()
        .filter_map(|(&id, hf)| hf_doms.get(&id).map(|d| (id, part_info(d, hf))))
        .collect();

    let mut parts = Map::new();
    for (rid, part) in &doc.hf_by_rel {
        if let Some(info) = infos.get(part) {
            parts.insert(rid.clone(), info_json(info));
        }
    }
    out.insert("hfParts".into(), Value::Object(parts));
    // `headerParas` / `footerParas` / `headerImages` / `footerImages` 在 5.4b / 5.4c
    for k in ["headerParas", "footerParas", "headerImages", "footerImages"] {
        out.insert(k.into(), Value::Null);
    }

    // 顶层字段：default 变体 + 三种 typed
    for kind in HfKind::ALL {
        let name = |suffix: &str| format!("{}{suffix}", kind.as_str());
        let default = pick(dom, doc, kind, HfVariant::Default).and_then(|p| infos.get(&p));
        // `headerText` / `footerText` 与 `headerHasPageNumber` / `footerHasPageNumber`
        out.insert(name("Text"), default.map_or(Value::Null, |i| Value::String(i.text.clone())));
        out.insert(
            format!("{}HasPageNumber", kind.as_str()),
            Value::Bool(default.is_some_and(|i| i.has_page_number)),
        );
        if kind == HfKind::Header {
            out.insert(
                "watermarkText".into(),
                default.and_then(|i| i.watermark.clone()).map_or(Value::Null, Value::String),
            );
        }
        // `headerFirst` / `headerEven` / `footerFirst` / `footerEven`：整条 `HfPartInfo`
        for variant in [HfVariant::First, HfVariant::Even] {
            let key = format!(
                "{}{}",
                kind.as_str(),
                match variant {
                    HfVariant::First => "First",
                    _ => "Even",
                }
            );
            let v = pick(dom, doc, kind, variant)
                .and_then(|p| infos.get(&p))
                .map_or(Value::Null, info_json);
            out.insert(key, v);
        }
    }
}

fn info_json(i: &PartInfo) -> Value {
    let mut o = Map::new();
    set(&mut o, "text", i.text.clone());
    set(&mut o, "hasPageNumber", i.has_page_number);
    // `paras` 与 `images` 在 5.4b / 5.4c
    o.insert("paras".into(), Value::Array(Vec::new()));
    Value::Object(o)
}

/// TS `readHeaderFooterPart` 的引用选择：全文按文档序的 `w:headerReference` / `w:footerReference`。
fn pick(
    dom: &Dom,
    doc: &Document,
    kind: HfKind,
    variant: HfVariant,
) -> Option<crate::package::PartId> {
    let elem = match kind {
        HfKind::Header => LocalName::HeaderReference,
        HfKind::Footer => LocalName::FooterReference,
    };
    let refs: Vec<NodeId> =
        dom.semantic_descendants(dom.root()).filter(|&n| dom.is(n, QName::w(elem))).collect();
    let type_of = |n: NodeId| dom.attr_value(n, QName::w(LocalName::Type)).map(|v| v.into_owned());
    let find = |want: &str| refs.iter().copied().find(|&n| type_of(n).as_deref() == Some(want));
    let node = match variant {
        HfVariant::First => find("first"),
        HfVariant::Even => find("even"),
        // 缺省页：default → 非 schema 的 odd → 没有 `w:type` 的
        HfVariant::Default => find("default")
            .or_else(|| find("odd"))
            .or_else(|| refs.iter().copied().find(|&n| type_of(n).is_none())),
    }?;
    let rid = dom.attr_value(node, QName::new(NsId::R, LocalName::Id))?;
    doc.hf_by_rel.get(rid.as_ref()).copied()
}

/// 一个 part 的 `text` / `hasPageNumber` / 水印。
fn part_info(dom: &Dom, hf: &HfPart) -> PartInfo {
    PartInfo {
        text: part_text(dom, hf),
        has_page_number: hf.has_page_number,
        watermark: hf.watermark.clone(),
    }
}

/// `text` 的事件：按字节位置排好序再拼。
enum Ev<'a> {
    /// 一个 `w:t` 的原文（未按 `xml:space` 去空白；实体已解码一次，`XML-06`）。
    Text(std::borrow::Cow<'a, str>),
    /// 页码 / 总页数标记。
    Mark(char),
    /// `</w:tc>`：后面还有文字就补一个空格。
    CellEnd,
}

/// TS `plainText(cleaned)`：见模块头的对照表。
pub(super) fn part_text(dom: &Dom, hf: &HfPart) -> String {
    // 跳过区间：PAGE / NUMPAGES 字段整段（连缓存结果），其他字段的 begin..separate（指令区）
    let mut skip: Vec<(u32, u32)> = Vec::new();
    let mut marks: Vec<(u32, char)> = Vec::new();
    for f in hf.idx.fields.fields() {
        let head = f.form.head();
        let (start, end) = (start_of(dom, head), end_of(dom, f.form.tail()));
        match f.keyword() {
            Keyword::Page => {
                marks.push((start, PAGE_MARK));
                skip.push((start, end));
            }
            Keyword::NumPages => {
                marks.push((start, TOTAL_PAGES_MARK));
                skip.push((start, end));
            }
            _ => {
                // 指令区丢掉，`separate` 之后的结果留着
                let instr_end = match &f.form {
                    FieldForm::Complex { separate: Some(s), .. } => end_of(dom, *s),
                    FieldForm::Complex { end, .. } => end_of(dom, *end),
                    // `w:fldSimple` 的 `w:instr` 是属性，不在 `w:t` 里，没什么可跳的
                    FieldForm::Simple { .. } => start,
                };
                if instr_end > start {
                    skip.push((start, instr_end));
                }
            }
        }
    }
    for n in dom.semantic_descendants(hf.root) {
        if dom.is(n, QName::w(LocalName::PgNum)) {
            marks.push((start_of(dom, n), PAGE_MARK));
        }
    }
    skip.sort_unstable();

    let mut events: Vec<(u32, Ev<'_>)> = marks.into_iter().map(|(p, c)| (p, Ev::Mark(c))).collect();
    for n in dom.semantic_descendants(hf.root) {
        if dom.is(n, QName::w(LocalName::T)) {
            let pos = start_of(dom, n);
            if in_skip(&skip, pos) {
                continue;
            }
            for c in dom.children(n).iter().copied() {
                if let Some(t) = dom.text(c) {
                    events.push((pos, Ev::Text(t)));
                }
            }
        } else if dom.is(n, QName::w(LocalName::Tc)) {
            events.push((end_of(dom, n), Ev::CellEnd));
        }
    }
    events.sort_by_key(|(p, _)| *p);

    let mut out = String::new();
    let mut pending_gap = false;
    for (_, ev) in events {
        match ev {
            Ev::CellEnd => pending_gap = !out.is_empty(),
            Ev::Text(t) => {
                if pending_gap {
                    out.push(' ');
                    pending_gap = false;
                }
                out.push_str(&t);
            }
            Ev::Mark(c) => {
                if pending_gap {
                    out.push(' ');
                    pending_gap = false;
                }
                out.push(c);
            }
        }
    }
    out
}

fn start_of(dom: &Dom, n: NodeId) -> u32 {
    dom.node(n).lex.as_ref().map_or(0, |l| l.range.start)
}

fn end_of(dom: &Dom, n: NodeId) -> u32 {
    dom.node(n).lex.as_ref().map_or(0, |l| l.range.end)
}

fn in_skip(skip: &[(u32, u32)], pos: u32) -> bool {
    skip.iter().any(|&(a, b)| pos >= a && pos < b)
}
