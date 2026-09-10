//! `ParsedDoc` 的声明部分（`COMPAT-02`）：`styles` / `docDefaults` / `headingStyleIds` /
//! `listParagraphStyleId` / `numbering` / `themeFonts` / `themeColors` / 保护与 settings 杂项。
//! 每条规则注明对应的 TS 函数（`docs/01` §4.3–4.6）。

use serde_json::{Map, Value, json};

use crate::model::ThemeSlot;
use crate::model::{Document, Level, StyleType};
use crate::resolve::{Resolver, rgb_hex};
use crate::semantic::props::{
    Fonts, Jc, LevelSuffix, LineSpacingRule, NumberFormat, ParaProps, RunProps, Shading, Style,
    UnderlineKind, Val,
};
use crate::xml::Dom;

use crate::bind::native::json::set_some;

pub(super) fn i32_of(v: &Option<Val<i32>>) -> Option<i64> {
    v.as_ref().and_then(|x| x.value().map(|&n| i64::from(n)))
}

pub(super) fn u32_of(v: &Option<Val<u32>>) -> Option<i64> {
    v.as_ref().and_then(|x| x.value().map(|&n| i64::from(n)))
}

/// `Val` 的原文（枚举字面或 Raw）。
pub(super) fn val_text<T: Copy>(
    v: &Option<Val<T>>,
    f: impl Fn(T) -> &'static str,
) -> Option<String> {
    v.as_ref().map(|x| match x {
        Val::Value(t) => f(*t).to_string(),
        Val::Raw(s) => s.clone(),
    })
}

/// TS `stripHash`。
pub(super) fn strip_hash(s: &str) -> &str {
    s.strip_prefix('#').unwrap_or(s)
}

/// TS `parseInt(s, 10)`：前导空白、可选符号、十进制前缀；否则 `None`（NaN）。
pub(super) fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    let n: i64 = digits[..end].parse().ok()?;
    Some(if neg { -n } else { n })
}

fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

// ---- 显示字段（TS styleDisplayOf / buildRun 共用的"半解析"规则）--------------------------------------

/// TS `shdDisplayFill`：`w:shd` 的显示填充色（图案与前景按比例混合）。声明值版本（样式链）。
pub(super) fn shd_display_fill(shd: &Shading) -> Option<String> {
    let fill_raw = shd.fill.as_ref().map(|v| match v {
        Val::Value(c) => c.to_xml(),
        Val::Raw(s) => s.clone(),
    });
    let ink_raw = shd.color.as_ref().map(|v| match v {
        Val::Value(c) => c.to_xml(),
        Val::Raw(s) => s.clone(),
    });
    shd_display_fill_raw(
        val_text(&shd.val, |v| v.as_str()).as_deref(),
        ink_raw.as_deref(),
        fill_raw.as_deref(),
    )
}

/// 原文版本：`val` / `color` / `fill` 是属性原文（保留大小写，与 TS 一致）。
pub(super) fn shd_display_fill_raw(
    val: Option<&str>,
    color: Option<&str>,
    fill_attr: Option<&str>,
) -> Option<String> {
    fn hex6(s: &str) -> Option<String> {
        let s = strip_hash(s);
        (s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_string())
    }
    let fill = fill_attr.filter(|f| *f != "auto").and_then(hex6);
    let val = val.unwrap_or("clear").to_string();
    if val == "clear" || val == "nil" {
        return fill;
    }
    let ink = color.filter(|c| *c != "auto").and_then(hex6);
    if val == "solid" {
        return Some(ink.unwrap_or_else(|| "000000".to_string()));
    }
    let ratio = if let Some(pct) = val.strip_prefix("pct") {
        let r = match pct {
            "12" => 12.5,
            "37" => 37.5,
            "62" => 62.5,
            "87" => 87.5,
            _ => pct.parse::<f64>().unwrap_or(f64::NAN),
        } / 100.0;
        (r > 0.0 && r <= 1.0).then_some(r)
    } else if val.to_ascii_lowercase().contains("stripe")
        || val.to_ascii_lowercase().contains("cross")
    {
        Some(if val.starts_with("thin") { 0.25 } else { 0.5 })
    } else {
        None
    };
    let Some(ratio) = ratio else { return fill };
    let fg = ink.unwrap_or_else(|| "000000".to_string());
    let bg = fill.unwrap_or_else(|| "FFFFFF".to_string());
    let ch = |h: &str, i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64;
    let mix = |i: usize| (ch(&fg, i) * ratio + ch(&bg, i) * (1.0 - ratio)).round() as u8;
    Some(format!("{:02X}{:02X}{:02X}", mix(0), mix(2), mix(4)))
}

/// TS `JC_ALIGN`。
pub(super) fn jc_align(jc: &Val<Jc>) -> Option<&'static str> {
    match jc {
        Val::Value(Jc::Left | Jc::Start) => Some("left"),
        Val::Value(Jc::Center) => Some("center"),
        Val::Value(Jc::Right | Jc::End) => Some("right"),
        Val::Value(Jc::Both) => Some("justify"),
        Val::Value(Jc::Distribute) => Some("distribute"),
        Val::Raw(s) if s == "justify" => Some("justify"),
        _ => None,
    }
}

/// TS `styleDisplayOf` 的 rPr 部分（样式链已合并的属性）。
pub(super) fn style_run_display(r: &Resolver, props: &RunProps, out: &mut Map<String, Value>) {
    if let Some(sz) = u32_of(&props.size).filter(|&n| n != 0) {
        set(out, "sizeHalfPoints", sz);
    }
    if let Some(c) = props.color.as_ref().and_then(|c| r.color(c)) {
        set(out, "color", rgb_hex(c));
    }
    for (k, v) in [
        ("bold", props.bold),
        ("italic", props.italic),
        ("boldCs", props.bold_cs),
        ("italicCs", props.italic_cs),
    ] {
        if let Some(b) = v {
            set(out, k, b);
        }
    }
    if let Some(sz) = u32_of(&props.size_cs).filter(|&n| n != 0) {
        set(out, "sizeCsHalfPoints", sz);
    }
    if let Some(b) = props.rtl {
        set(out, "rtl", b);
    }
    if let Some(u) = props.underline.as_ref().and_then(|u| u.val.as_ref()) {
        set(out, "underline", *u != Val::Value(UnderlineKind::None));
    }
    if let Some(b) = props.strike {
        set(out, "strike", b);
    }
    let f = r.fonts(props);
    let font = f.display().map(str::to_string);
    if let Some(a) = f.display_ascii() {
        set(out, "fontAscii", a);
    }
    if let Some(cs) = &f.cs {
        set(out, "csFont", cs.clone());
    }
    if let Some(font) = &font {
        set(out, "font", font.clone());
        if f.ea_slot_empty && f.east_asia.as_deref() == Some(font) {
            set(out, "eaSlotEmpty", true);
        }
    }
    if let Some(sp) = i32_of(&props.spacing).filter(|&n| n != 0) {
        set(out, "charSpacingTwips", sp);
    }
    match (props.caps, props.small_caps) {
        (Some(true), _) => set(out, "caps", "all"),
        (_, Some(true)) => set(out, "caps", "small"),
        (Some(false), _) | (_, Some(false)) => set(out, "caps", "none"),
        _ => {}
    }
    if let Some(v) = props.vanish
        && props.spec_vanish != Some(true)
    {
        set(out, "vanish", v);
    }
}

/// TS `styleDisplayOf` 的 pPr 部分。
pub(super) fn style_para_display(props: &ParaProps, out: &mut Map<String, Value>) {
    if let Some(sp) = &props.spacing {
        if let Some(line) = i32_of(&sp.line).filter(|&l| l > 0) {
            let rule = line_rule(&sp.line_rule);
            set(out, "lineRule", rule.clone());
            set(out, "lineRawTwips", line);
            if rule == "auto" {
                set(out, "lineSpacing", line as f64 / 240.0);
            }
        }
        if sp.before.is_some() {
            set(out, "spaceBeforeTwips", i32_of(&sp.before).unwrap_or(0));
        }
        if sp.after.is_some() {
            set(out, "spaceAfterTwips", i32_of(&sp.after).unwrap_or(0));
        }
        if let Some(b) = sp.before_autospacing {
            set(out, "spaceBeforeAuto", b);
        }
        if let Some(b) = sp.after_autospacing {
            set(out, "spaceAfterAuto", b);
        }
    }
    if props.keep_next == Some(true) {
        set(out, "keepNext", true);
    }
    if props.keep_lines == Some(true) {
        set(out, "keepLines", true);
    }
    if let Some(b) = props.page_break_before {
        set(out, "pageBreakBefore", b);
    }
    if let Some(b) = props.suppress_auto_hyphens {
        set(out, "suppressAutoHyphens", b);
    }
    if let Some(b) = props.contextual_spacing {
        set(out, "contextualSpacing", b);
    }
    if let Some(b) = auto_space(props) {
        set(out, "autoSpace", b);
    }
    if let Some(jc) = &props.jc {
        match jc_align(jc) {
            Some("distribute") => set(out, "align", "justify"),
            Some(a) => set(out, "align", a),
            None => {}
        }
    }
    if let Some(fill) = props.shading.as_ref().and_then(shd_display_fill) {
        set(out, "shadingFill", fill);
    }
    if let Some(stops) = tab_stops(props) {
        set(out, "tabStops", stops);
    }
    if let Some(ind) = &props.indent {
        if let Some(l) = i32_of(&ind.start).filter(|&n| n != 0) {
            set(out, "indentLeftTwips", l);
        }
        if let Some(r) = i32_of(&ind.end).filter(|&n| n != 0) {
            set(out, "indentRightTwips", r);
        }
        let hanging = i32_of(&ind.hanging).unwrap_or(0);
        let first = i32_of(&ind.first_line).unwrap_or(0);
        if hanging > 0 {
            set(out, "indentFirstLineTwips", -hanging);
        } else if first > 0 {
            set(out, "indentFirstLineTwips", first);
        }
    }
}

pub(super) fn line_rule(v: &Option<Val<LineSpacingRule>>) -> String {
    val_text(v, |r| r.as_str()).unwrap_or_else(|| "auto".to_string())
}

/// TS `autoSpaceOf`。
pub(super) fn auto_space(props: &ParaProps) -> Option<bool> {
    match (props.auto_space_de, props.auto_space_dn) {
        (Some(false), Some(false)) => Some(false),
        (Some(true), _) | (_, Some(true)) => Some(true),
        _ => None,
    }
}

/// TS `tabStopsOf`。
pub(super) fn tab_stops(props: &ParaProps) -> Option<Value> {
    let tabs = props.tabs.as_ref()?;
    let mut stops = Vec::new();
    for t in &tabs.tab {
        let Some(pos) = t.pos.as_ref().and_then(|p| match p {
            Val::Value(n) => Some(i64::from(*n)),
            Val::Raw(s) => parse_int(s),
        }) else {
            continue;
        };
        let val = val_text(&t.val, |v| v.as_str()).unwrap_or_else(|| "left".to_string());
        let val = if ["left", "center", "right", "decimal", "bar", "clear"].contains(&val.as_str())
        {
            val
        } else {
            "left".to_string()
        };
        let mut stop = Map::new();
        set(&mut stop, "pos", pos);
        set(&mut stop, "val", val);
        if let Some(leader) = val_text(&t.leader, |l| l.as_str())
            && ["dot", "hyphen", "underscore", "heavy", "middleDot"].contains(&leader.as_str())
        {
            set(&mut stop, "leader", leader);
        }
        stops.push(Value::Object(stop));
    }
    (!stops.is_empty()).then_some(Value::Array(stops))
}

// ---- styles -------------------------------------------------------------------------------------

pub(super) struct StylesOut {
    pub styles: Value,
    pub doc_defaults: Option<Value>,
    pub heading_style_ids: Value,
    pub list_paragraph_style_id: Option<String>,
}

fn style_type(k: StyleType) -> Option<&'static str> {
    match k {
        StyleType::Paragraph => Some("paragraph"),
        StyleType::Character => Some("character"),
        StyleType::Table => Some("table"),
        StyleType::Numbering => None,
    }
}

/// 样式链上的 `numPr`（TS：自身 numPr 优先，`numId 0` → `'none'` 且阻断继承）。
pub(super) fn style_num_pr(r: &Resolver, id: &str) -> Option<Value> {
    for s in r.chain(id, StyleType::Paragraph) {
        let Some(num) = s.ppr.as_ref().and_then(|p| p.num.as_ref()) else { continue };
        let Some(num_id) = num.num_id.as_ref().map(|v| match v {
            Val::Value(n) => n.to_string(),
            Val::Raw(s) => s.clone(),
        }) else {
            continue;
        };
        if num_id == "0" {
            return Some(Value::String("none".into()));
        }
        let ilvl = num.ilvl.as_ref().and_then(|v| v.value().map(|&n| i64::from(n))).unwrap_or(0);
        return Some(json!({ "numId": num_id, "ilvl": ilvl }));
    }
    None
}

pub(super) fn styles_json(doc: &Document, r: &Resolver) -> StylesOut {
    let mut styles = Map::new();
    let mut heading_ids: Map<String, Value> = Map::new();
    let mut list_para: Option<String> = None;
    let Some(st) = doc.styles.as_ref() else {
        return StylesOut {
            styles: Value::Object(styles),
            doc_defaults: None,
            heading_style_ids: Value::Object(heading_ids),
            list_paragraph_style_id: None,
        };
    };
    // Map 语义：同 id 后者覆盖，但键序按首次出现（normalize 排序键，无影响）
    for s in &st.styles {
        let (Some(id), Some(kind)) = (s.id(), s.kind()) else { continue };
        let Some(ty) = style_type(kind) else { continue };
        // 重复 id：只用最后一个声明（Styles::get 的语义）
        let s: &Style = st.get(id).unwrap_or(s);
        let mut o = Map::new();
        set(&mut o, "styleId", id);
        set(&mut o, "name", s.display_name().unwrap_or(id));
        set(&mut o, "type", ty);
        if kind == StyleType::Paragraph
            && let Some(l) = r.heading_level(id)
        {
            set(&mut o, "headingLevel", l);
        }
        if s.semi_hidden == Some(true) {
            set(&mut o, "semiHidden", true);
        }
        if s.q_format == Some(true) {
            set(&mut o, "qFormat", true);
        }
        if r.is_linked_char_shell(id) {
            set(&mut o, "linkedCharShell", true);
        }
        if kind == StyleType::Table
            && let Some(td) = super::table::table_display(r, id)
        {
            set(&mut o, "tableDisplay", td);
        }
        if kind != StyleType::Table {
            let mut d = Map::new();
            if let Some(rp) = r.style_run_props(id, kind) {
                style_run_display(r, &rp, &mut d);
            }
            if kind == StyleType::Paragraph
                && let Some(pp) = r.style_para_props(id)
            {
                style_para_display(&pp, &mut d);
            }
            if !d.is_empty() {
                set(&mut o, "display", Value::Object(d));
            }
        }
        if kind == StyleType::Paragraph
            && let Some(np) = style_num_pr(r, id)
        {
            set(&mut o, "numPr", np);
        }
        if r.default_style(kind).and_then(Style::id) == Some(id) {
            set(&mut o, "isDefault", true);
        }
        if kind == StyleType::Paragraph
            && let Some(l) = r.heading_level(id)
            && !heading_ids.contains_key(&l.to_string())
        {
            heading_ids.insert(l.to_string(), Value::String(id.to_string()));
        }
        if list_para.is_none() && id.eq_ignore_ascii_case("listparagraph") {
            list_para = Some(id.to_string());
        }
        styles.insert(id.to_string(), Value::Object(o));
    }
    StylesOut {
        styles: Value::Object(styles),
        doc_defaults: doc_defaults_json(doc, r),
        heading_style_ids: Value::Object(heading_ids),
        list_paragraph_style_id: list_para,
    }
}

/// TS `parseStyles` 的 docDefaults（`w:docDefaults` 存在时才有，可为空对象）。
fn doc_defaults_json(doc: &Document, r: &Resolver) -> Option<Value> {
    let st = doc.styles.as_ref()?;
    st.doc_defaults.as_ref()?;
    let mut dd = Map::new();
    let rpr = st.doc_default_rpr();
    if let Some(sz) = rpr.and_then(|x| u32_of(&x.size)).filter(|&n| n != 0) {
        set(&mut dd, "sizeHalfPoints", sz);
    }
    let f = r.doc_default_fonts();
    if let Some(a) = f.display_ascii() {
        set(&mut dd, "asciiFont", a);
    }
    if let Some(ea) = &f.east_asia {
        set(&mut dd, "eastAsiaFont", ea.clone());
        if f.ea_from_lang {
            set(&mut dd, "eaFromLang", true);
            if f.ea_slot_empty {
                set(&mut dd, "eaSlotEmpty", true);
            }
        }
    }
    if let Some(rpr) = rpr {
        // onFlag：元素存在且 val 不是 0/false → true
        if rpr.bold == Some(true) {
            set(&mut dd, "bold", true);
        }
        if rpr.italic == Some(true) {
            set(&mut dd, "italic", true);
        }
        if let Some(c) = rpr.color.as_ref().and_then(|c| r.color(c)) {
            set(&mut dd, "color", rgb_hex(c));
        }
        if let Some(l) = rpr.lang.as_ref().and_then(|l| l.val.clone()) {
            set(&mut dd, "lang", l);
        }
    }
    if let Some(sp) = st.doc_default_ppr().and_then(|p| p.spacing.as_ref()) {
        if let Some(line) = i32_of(&sp.line).filter(|&l| l > 0) {
            let rule = line_rule(&sp.line_rule);
            set(&mut dd, "lineRawTwips", line);
            set(&mut dd, "lineRule", rule.clone());
            if rule == "auto" {
                set(&mut dd, "lineSpacing", line as f64 / 240.0);
            }
        }
        if sp.before.is_some() {
            set(&mut dd, "spaceBeforeTwips", i32_of(&sp.before).unwrap_or(0));
        }
        if sp.after.is_some() {
            set(&mut dd, "spaceAfterTwips", i32_of(&sp.after).unwrap_or(0));
        }
        if let Some(b) = sp.before_autospacing {
            set(&mut dd, "spaceBeforeAuto", b);
        }
        if let Some(b) = sp.after_autospacing {
            set(&mut dd, "spaceAfterAuto", b);
        }
    }
    if st.doc_default_ppr().is_some_and(|p| p.suppress_auto_hyphens == Some(true)) {
        set(&mut dd, "suppressAutoHyphens", true);
    }
    // TS：空对象不输出
    (!dd.is_empty()).then_some(Value::Object(dd))
}

// ---- numbering ----------------------------------------------------------------------------------

/// TS `NumberingDef` 表；`formats` 是 numId → level 0 的 bullet/ordered。
pub(super) struct NumberingOut {
    pub defs: Value,
    /// `(numId, ilvl) → numFmt`
    pub level_fmt: std::collections::HashMap<(i64, i64), String>,
    pub formats: std::collections::HashMap<i64, &'static str>,
}

/// TS `numFmtOfLevel`：直接 `w:numFmt`，或 AlternateContent 里的 Choice/Fallback（MCE 已选分支）；
/// `custom` 须有合法的 `w:format` 枚举项。
fn num_fmt_of_level(l: &Level) -> (Option<String>, Option<String>) {
    let Some(f) = &l.num_fmt else { return (None, None) };
    let val = val_text(&f.val, |v| v.as_str());
    if val.as_deref() == Some("custom") {
        if let Some(fmt) = &f.format
            && custom_enum_items(fmt)
        {
            return (Some("custom".into()), Some(fmt.clone()));
        }
        return (None, None);
    }
    (val, None)
}

/// TS `customEnumItems`：逗号分隔、去掉尾部 `...`/`…`，≥2 项且都非空。
fn custom_enum_items(format: &str) -> bool {
    let mut items: Vec<&str> = format.split(',').map(str::trim).collect();
    while items.last().is_some_and(|s| s.is_empty() || *s == "..." || *s == "…") {
        items.pop();
    }
    items.len() >= 2 && items.iter().all(|s| !s.is_empty())
}

fn level_json(l: &Level) -> Value {
    let mut o = Map::new();
    let (fmt, custom) = num_fmt_of_level(l);
    set(&mut o, "numFmt", fmt.unwrap_or_else(|| NumberFormat::Decimal.as_str().to_string()));
    set(&mut o, "lvlText", l.lvl_text.as_ref().and_then(|t| t.val.clone()).unwrap_or_default());
    let start = l.start.as_ref().map_or(Some(0), |v| match v {
        Val::Value(n) => Some(i64::from(*n)),
        Val::Raw(s) => parse_int(s),
    });
    set(&mut o, "start", start.unwrap_or(0));
    if let Some(c) = custom {
        set(&mut o, "customFormat", c);
    }
    if let Some(Val::Value(s)) = &l.suff {
        set(
            &mut o,
            "suff",
            match s {
                LevelSuffix::Tab => "tab",
                LevelSuffix::Space => "space",
                LevelSuffix::Nothing => "nothing",
            },
        );
    }
    if let Some(ind) = l.ppr.as_ref().and_then(|p| p.indent.as_ref()) {
        let left = i32_of(&ind.start).unwrap_or(0);
        if left > 0 {
            set(&mut o, "indentLeft", left);
        }
        let hanging = i32_of(&ind.hanging).unwrap_or(0);
        if hanging > 0 {
            set(&mut o, "hanging", hanging);
        }
        let first = i32_of(&ind.first_line).unwrap_or(0);
        if hanging <= 0 && first > 0 {
            set(&mut o, "firstLine", first);
        }
    }
    if let Some(rpr) = &l.rpr {
        if let Some(sz) = u32_of(&rpr.size).filter(|&n| n > 0) {
            set(&mut o, "szHalfPoints", sz);
        }
        if let Some(f) = rpr.fonts.as_ref().and_then(|f: &Fonts| {
            f.ascii.clone().or_else(|| f.h_ansi.clone()).or_else(|| f.east_asia.clone())
        }) {
            set(&mut o, "font", f);
        }
    }
    Value::Object(o)
}

pub(super) fn numbering_json(doc: &Document) -> NumberingOut {
    use std::collections::HashMap;
    let mut defs = Map::new();
    let mut level_fmt = HashMap::new();
    let mut formats = HashMap::new();
    let Some(n) = doc.numbering.as_ref() else {
        return NumberingOut { defs: Value::Object(defs), level_fmt, formats };
    };
    // abstractNum → ilvl → level json；numStyleLink 链：终点作底、自身覆盖
    let mut abs_levels: HashMap<i32, Vec<(i64, Value)>> = HashMap::new();
    let mut num_style_links: HashMap<i32, String> = HashMap::new();
    let mut style_link_abs: HashMap<String, i32> = HashMap::new();
    for a in &n.abstract_nums {
        let Some(id) = a.id() else { continue };
        let levels: Vec<(i64, Value)> = a
            .levels
            .iter()
            .filter_map(|l| l.ilvl().map(|i| (i64::from(i), level_json(l))))
            .collect();
        abs_levels.insert(id, levels);
        if let Some(l) = &a.num_style_link {
            num_style_links.insert(id, l.clone());
        }
        if let Some(l) = &a.style_link {
            style_link_abs.insert(l.clone(), id);
        }
    }
    let merged = |levels: &mut Vec<(i64, Value)>, over: &[(i64, Value)]| {
        for (i, v) in over {
            match levels.iter_mut().find(|(j, _)| j == i) {
                Some(slot) => slot.1 = v.clone(),
                None => levels.push((*i, v.clone())),
            }
        }
    };
    for &abs_id in num_style_links.clone().keys() {
        let mut seen = vec![abs_id];
        let mut target = abs_id;
        while let Some(style_id) = num_style_links.get(&target) {
            let Some(&next) = style_link_abs.get(style_id) else { break };
            if seen.contains(&next) {
                break;
            }
            seen.push(next);
            target = next;
        }
        if target != abs_id {
            let mut base = abs_levels.get(&target).cloned().unwrap_or_default();
            let own = abs_levels.get(&abs_id).cloned().unwrap_or_default();
            merged(&mut base, &own);
            abs_levels.insert(abs_id, base);
        }
    }
    for num in &n.nums {
        let (Some(num_id), Some(abs_id)) = (num.id(), num.abstract_id()) else { continue };
        let mut levels = abs_levels.get(&abs_id).cloned().unwrap_or_default();
        let mut start_overrides = Map::new();
        for ov in &num.overrides {
            let Some(ilvl) = i32_of(&ov.ilvl) else { continue };
            if let Some(s) = i32_of(&ov.start_override) {
                start_overrides.insert(ilvl.to_string(), Value::from(s));
            }
            if let Some(l) = &ov.lvl {
                merged(&mut levels, &[(ilvl, level_json(l))]);
            }
        }
        let mut lv = Map::new();
        for (i, v) in &levels {
            if let Some(f) = v.get("numFmt").and_then(Value::as_str) {
                level_fmt.insert((i64::from(num_id), *i), f.to_string());
            }
            lv.insert(i.to_string(), v.clone());
        }
        let fmt0 = levels
            .iter()
            .find(|(i, _)| *i == 0)
            .and_then(|(_, v)| v.get("numFmt"))
            .and_then(Value::as_str);
        formats
            .insert(i64::from(num_id), if fmt0 == Some("bullet") { "bullet" } else { "ordered" });
        defs.insert(
            num_id.to_string(),
            json!({ "numId": num_id.to_string(), "abstractNumId": abs_id.to_string(), "levels": Value::Object(lv), "startOverrides": Value::Object(start_overrides) }),
        );
    }
    NumberingOut { defs: Value::Object(defs), level_fmt, formats }
}

/// TS `listKindOf`。
pub(super) fn list_kind(numbering: &NumberingOut, num_id: i64, ilvl: i64) -> &'static str {
    match numbering.level_fmt.get(&(num_id, ilvl)) {
        Some(f) => {
            if f == "bullet" {
                "bullet"
            } else {
                "ordered"
            }
        }
        None => numbering.formats.get(&num_id).copied().unwrap_or("bullet"),
    }
}

// ---- theme --------------------------------------------------------------------------------------

/// TS `readThemeFonts` + `eaLang`：没有 theme part 或 major/minor 都空 → `null`。
pub(super) fn theme_fonts_json(doc: &Document, r: &Resolver) -> Value {
    let Some(f) = doc.theme.as_ref().and_then(|t| t.fonts.as_ref()) else { return Value::Null };
    let major = f.major.latin.clone().unwrap_or_default();
    let minor = f.minor.latin.clone().unwrap_or_default();
    if major.is_empty() && minor.is_empty() {
        return Value::Null;
    }
    let mut o = Map::new();
    set(&mut o, "major", major);
    set(&mut o, "minor", minor);
    if let Some(ea) = &f.minor.ea {
        set(&mut o, "eastAsia", ea.clone());
    }
    if let Some(ea) = &f.major.ea {
        set(&mut o, "majorEastAsia", ea.clone());
    }
    if let Some(cs) = &f.minor.cs {
        set(&mut o, "minorCs", cs.clone());
    }
    if let Some(cs) = &f.major.cs {
        set(&mut o, "majorCs", cs.clone());
    }
    for (key, slots) in [("majorScripts", &f.major.scripts), ("minorScripts", &f.minor.scripts)] {
        if !slots.is_empty() {
            let mut m = Map::new();
            for (s, t) in slots {
                m.insert(s.clone(), Value::String(t.clone()));
            }
            set(&mut o, key, Value::Object(m));
        }
    }
    if let Some(lang) = r.ea_lang() {
        set(&mut o, "eaLang", lang);
    }
    Value::Object(o)
}

/// TS `parseTheme` 的 colors：没有 theme part → 内建 Office 调色板；有 part 但无 clrScheme → `null`。
pub(super) fn theme_colors_json(doc: &Document) -> Value {
    let palette = match &doc.theme {
        None => crate::model::ColorScheme::office_default(),
        Some(t) => match &t.colors {
            Some(c) => c.clone(),
            None => return Value::Null,
        },
    };
    let mut o = Map::new();
    if let Some(name) = &palette.name
        && doc.theme.is_some()
    {
        set(&mut o, "name", name.clone());
    }
    for slot in ThemeSlot::ALL {
        if let Some(c) = palette.get(slot) {
            set(&mut o, slot.as_str(), rgb_hex(c));
        }
    }
    if o.is_empty() { Value::Null } else { Value::Object(o) }
}

// ---- settings -----------------------------------------------------------------------------------

/// TS `parseProtection`。
pub(super) fn protection_json(doc: &Document) -> Value {
    let Some(p) = doc.settings.as_ref().and_then(|s| s.document_protection.as_ref()) else {
        return Value::Null;
    };
    let Some(edit) = val_text(&p.edit, |e| e.as_str()) else { return Value::Null };
    if edit == "none" {
        return Value::Null;
    }
    let mut o = Map::new();
    set(&mut o, "edit", edit);
    set(&mut o, "enforced", p.enforcement == Some(true));
    if let Some(h) = &p.hash {
        set(&mut o, "hash", h.clone());
    }
    if let Some(s) = &p.salt {
        set(&mut o, "salt", s.clone());
    }
    if let Some(n) = i32_of(&p.crypt_spin_count) {
        set(&mut o, "spinCount", n);
    }
    if let Some(n) = i32_of(&p.crypt_algorithm_sid) {
        set(&mut o, "algorithmSid", n);
    }
    Value::Object(o)
}

/// TS `parseWriteProtection`。
pub(super) fn write_protection_json(doc: &Document) -> Value {
    let Some(w) = doc.settings.as_ref().and_then(|s| s.write_protection.as_ref()) else {
        return Value::Null;
    };
    let recommended = w.recommended == Some(true);
    if !recommended && w.hash.is_none() {
        return Value::Null;
    }
    let mut o = Map::new();
    if recommended {
        set(&mut o, "recommended", true);
    }
    if let Some(h) = &w.hash {
        set(&mut o, "hash", h.clone());
    }
    if let Some(s) = &w.salt {
        set(&mut o, "salt", s.clone());
    }
    if let Some(n) = i32_of(&w.crypt_spin_count) {
        set(&mut o, "spinCount", n);
    }
    if let Some(n) = i32_of(&w.crypt_algorithm_sid) {
        set(&mut o, "algorithmSid", n);
    }
    Value::Object(o)
}

/// TS `xmlFlagOn(document.xml, 'w:titlePg')`：任一 `w:titlePg` 元素且 val 不是 0/false/off。
pub(super) fn title_pg(dom: &Dom) -> bool {
    use crate::xml::{LocalName, QName};
    dom.descendants(dom.root()).any(|n| {
        dom.is(n, QName::w(LocalName::TitlePg))
            && !dom
                .attr_value(n, QName::w(LocalName::Val))
                .is_some_and(|v| matches!(v.trim(), "0" | "false" | "off"))
    })
}

/// TS `parseLayoutSettings` + `parseEvenAndOddHeaders` + `parseCompatibilityMode`。
pub(super) fn settings_json(
    doc: &Document,
    settings_dom: Option<&Dom>,
    out: &mut Map<String, Value>,
) {
    let s = doc.settings.as_ref();
    let mode = match (s, settings_dom) {
        (Some(s), Some(d)) => s.compatibility_mode(d).unwrap_or(0),
        _ => 0,
    };
    set(out, "compatibilityMode", mode);
    set(out, "evenAndOddHeaders", s.is_some_and(|s| s.even_and_odd_headers == Some(true)));
    set(out, "removePersonalInfo", s.is_some_and(|s| s.remove_personal_information == Some(true)));
    if s.is_some_and(|s| s.auto_hyphenation == Some(true)) {
        set(out, "autoHyphenation", true);
    }
    if let Some(t) = s.and_then(|s| i32_of(&s.default_tab_stop)) {
        set(out, "defaultTabStopTwips", t);
    }
    if let (Some(s), Some(d)) = (s, settings_dom) {
        let facts = s.compat_facts(d);
        let flag = |name: crate::xml::LocalName| facts.flags.iter().any(|q| q.local == name);
        if flag(crate::xml::LocalName::BalanceSingleByteDoubleByteWidth) {
            set(out, "balanceDbcsSpacing", true);
        }
        if flag(crate::xml::LocalName::AdjustLineHeightInTable) {
            set(out, "adjustLineHeightInTable", true);
        }
        if val_text(&s.character_spacing_control, |c| c.as_str())
            .is_some_and(|v| v.starts_with("compressPunctuation"))
        {
            set(out, "compressPunctuation", true);
        }
    }
}

/// TS `parseFontTable`（非空才有 `fontTable` 键）。
pub(super) fn font_table_json(doc: &Document) -> Option<Value> {
    let ft = doc.font_table.as_ref()?;
    let mut out = Vec::new();
    for f in &ft.fonts {
        let Some(name) = &f.name else { continue };
        let mut o = Map::new();
        set(&mut o, "name", name.clone());
        if let Some(a) = f.alt_name.as_ref().filter(|s| !s.is_empty()) {
            set(&mut o, "altName", a.clone());
        }
        if let Some(p) = f.panose1.as_ref().filter(|s| !s.is_empty()) {
            set(&mut o, "panose", p.clone());
        }
        if let Some(fam) = val_text(&f.family, |x| x.as_str()).filter(|s| !s.is_empty()) {
            set(&mut o, "family", fam);
        }
        if let Some(p) = val_text(&f.pitch, |x| x.as_str()).filter(|s| !s.is_empty()) {
            set(&mut o, "pitch", p);
        }
        out.push(Value::Object(o));
    }
    (!out.is_empty()).then_some(Value::Array(out))
}

// ---- 批注与注释（`COMPAT-02`，TS `parseComments` / `notes.ts`）--------------------------------

/// `comments[]`：`{id, author, initials, date, text, paraId, parentId, done}`。
/// 缺失的字段不出（TS 只在有值时写）。
pub(super) fn comments_json(doc: &Document) -> Value {
    let mut out = Vec::new();
    for c in &doc.comments.items {
        let mut o = Map::new();
        set(&mut o, "id", c.id.clone());
        if let Some(a) = &c.author {
            set(&mut o, "author", a.clone());
        }
        if let Some(i) = &c.initials {
            set(&mut o, "initials", i.clone());
        }
        if let Some(d) = &c.date {
            set(&mut o, "date", d.clone());
        }
        set(&mut o, "text", c.text.clone());
        if let Some(p) = &c.para_id {
            set(&mut o, "paraId", p.clone());
        }
        if let Some(p) = &c.parent_id {
            set(&mut o, "parentId", p.clone());
        }
        if c.done {
            set(&mut o, "done", true);
        }
        out.push(Value::Object(o));
    }
    Value::Array(out)
}

/// `footnotes[]` / `endnotes[]`：`{id, text, richParas?, noRefMark?}`。
/// 结构条目（`separator` / `continuationSeparator`）不出（TS 跳过带 `w:type` 的条目）。
pub(super) fn notes_json(doc: &Document, r: &Resolver<'_>, endnotes: bool) -> Value {
    let notes = if endnotes { &doc.endnotes } else { &doc.footnotes };
    let mut out = Vec::new();
    for n in notes.normal() {
        let mut o = Map::new();
        set(&mut o, "id", n.id.clone());
        set(&mut o, "text", n.text.clone());
        if let Some(rich) = rich_paras_json(&n.rich, r) {
            set(&mut o, "richParas", rich);
        }
        if n.no_ref_mark {
            set(&mut o, "noRefMark", true);
        }
        set_some!(&mut o, "styleId" => n.style_id.clone());
        out.push(Value::Object(o));
    }
    Value::Array(out)
}

/// `richParas`：每段一个 run 列表。任一 run 都没有格式时整体不出（TS 行为）。
fn rich_paras_json(rich: &[Vec<crate::model::RichRun>], r: &Resolver<'_>) -> Option<Value> {
    let mut any_format = false;
    let mut paras = Vec::new();
    for line in rich {
        let mut runs = Vec::new();
        for run in line {
            let mut o = Map::new();
            set(&mut o, "text", run.text.clone());
            let p = &run.props;
            for (k, v) in
                [("bold", p.bold), ("italic", p.italic), ("strike", p.strike), ("caps", p.caps)]
            {
                if v == Some(true) {
                    set(&mut o, k, true);
                    any_format = true;
                }
            }
            if p.underline.as_ref().is_some_and(|u| {
                u.val.as_ref().and_then(Val::value).is_some_and(|k| *k != UnderlineKind::None)
            }) {
                set(&mut o, "underline", true);
                any_format = true;
            }
            if let Some(c) = p.color.as_ref().and_then(|c| r.color(c)) {
                set(&mut o, "color", rgb_hex(c));
                any_format = true;
            }
            if let Some(sz) = u32_of(&p.size).filter(|&n| n != 0) {
                set(&mut o, "sizeHalfPoints", sz);
                any_format = true;
            }
            runs.push(Value::Object(o));
        }
        paras.push(Value::Array(runs));
    }
    any_format.then_some(Value::Array(paras))
}

/// TS `sources[]`（任务 5.7）：`customXml` 里 `b:Sources` 的条目。
///
/// `publisher` / `url` 缺失时**不给键**（TS 的 `SourceInfo` 里它们是可选的），其余四个字段
/// 总是给——空串也给，因为 TS 的 `??  ''` 把它们兜成了空串。
pub fn sources_json(doc: &Document) -> Value {
    Value::Array(
        doc.sources
            .iter()
            .map(|s| {
                let mut o = Map::new();
                o.insert("tag".into(), Value::String(s.tag.clone()));
                o.insert("type".into(), Value::String(s.kind.clone()));
                o.insert("author".into(), Value::String(s.author.clone()));
                o.insert("title".into(), Value::String(s.title.clone()));
                o.insert("year".into(), Value::String(s.year.clone()));
                set_some!(&mut o, "publisher" => s.publisher.clone(), "url" => s.url.clone());
                Value::Object(o)
            })
            .collect(),
    )
}
