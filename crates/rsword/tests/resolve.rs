//! resolve 在语料上的验收（任务 1.9，`RES-02/05/06` DoD "与 TS `StyleDisplay` 对照"）：
//! 每个样式经 basedOn 链 + linked 补缺后的 run / 段落属性，与 TS `styles[id].display`、`headingLevel`、
//! `numPr`、`linkedCharShell` 及 `docDefaults` 逐字段对照。

mod common;

use std::collections::BTreeMap;

use rsword::model::{Document, StyleType};
use rsword::package::Package;
use rsword::resolve::{Resolver, rgb_hex};
use rsword::semantic::props::{LineSpacingRule, ParaProps, RunProps, UnderlineKind, Val};
use serde_json::Value;

/// 已知差异（`src/bind/compat_ts/KNOWN_DIFFS.md`）。
const KNOWN_DIFFS: &[(&str, &str)] = &[];

#[derive(Default)]
struct Stats {
    docs: usize,
    styles: usize,
    compared: BTreeMap<&'static str, usize>,
    mismatches: Vec<String>,
}

fn known(file: &str, what: &str) -> bool {
    KNOWN_DIFFS.iter().any(|(p, w)| file.starts_with(p) && *w == what)
}

fn i32_of(v: &Option<Val<i32>>) -> Option<i64> {
    v.as_ref().and_then(|x| x.value().map(|&n| i64::from(n)))
}

fn u32_of(v: &Option<Val<u32>>) -> Option<i64> {
    v.as_ref().and_then(|x| x.value().map(|&n| i64::from(n)))
}

/// 本引擎按 TS `styleDisplayOf` 的口径投影出的显示字段。
fn run_display(r: &Resolver, props: &RunProps, out: &mut BTreeMap<&'static str, Value>) {
    if let Some(sz) = u32_of(&props.size).filter(|&n| n != 0) {
        out.insert("sizeHalfPoints", sz.into());
    }
    if let Some(c) = props.color.as_ref().and_then(|c| r.color(c)) {
        out.insert("color", rgb_hex(c).into());
    }
    for (key, v) in [
        ("bold", props.bold),
        ("italic", props.italic),
        ("boldCs", props.bold_cs),
        ("italicCs", props.italic_cs),
        ("rtl", props.rtl),
        ("strike", props.strike),
    ] {
        if let Some(b) = v {
            out.insert(key, b.into());
        }
    }
    if let Some(sz) = u32_of(&props.size_cs).filter(|&n| n != 0) {
        out.insert("sizeCsHalfPoints", sz.into());
    }
    if let Some(u) = props.underline.as_ref().and_then(|u| u.val.as_ref()) {
        out.insert("underline", (*u != Val::Value(UnderlineKind::None)).into());
    }
    let f = r.fonts(props);
    if let Some(font) = f.display() {
        out.insert("font", font.into());
        if f.ea_slot_empty && f.east_asia.as_deref() == Some(font) {
            out.insert("eaSlotEmpty", true.into());
        }
    }
    if let Some(a) = f.display_ascii() {
        out.insert("fontAscii", a.into());
    }
    if let Some(cs) = &f.cs {
        out.insert("csFont", cs.clone().into());
    }
    if let Some(sp) = i32_of(&props.spacing).filter(|&n| n != 0) {
        out.insert("charSpacingTwips", sp.into());
    }
    match (props.caps, props.small_caps) {
        (Some(true), _) => {
            out.insert("caps", "all".into());
        }
        (_, Some(true)) => {
            out.insert("caps", "small".into());
        }
        (Some(false), _) | (_, Some(false)) => {
            out.insert("caps", "none".into());
        }
        _ => {}
    }
    if let Some(v) = props.vanish
        && props.spec_vanish != Some(true)
    {
        out.insert("vanish", v.into());
    }
}

fn para_display(props: &ParaProps, out: &mut BTreeMap<&'static str, Value>) {
    if let Some(sp) = &props.spacing {
        if let Some(line) = i32_of(&sp.line).filter(|&l| l > 0) {
            let rule = match sp.line_rule.as_ref().and_then(|r| r.value()) {
                Some(LineSpacingRule::Exact) => "exact",
                Some(LineSpacingRule::AtLeast) => "atLeast",
                _ => "auto",
            };
            out.insert("lineRule", rule.into());
            out.insert("lineRawTwips", line.into());
            if rule == "auto" {
                out.insert("lineSpacing", (line as f64 / 240.0).into());
            }
        }
        if sp.before.is_some() {
            out.insert("spaceBeforeTwips", i32_of(&sp.before).unwrap_or(0).into());
        }
        if sp.after.is_some() {
            out.insert("spaceAfterTwips", i32_of(&sp.after).unwrap_or(0).into());
        }
        if let Some(b) = sp.before_autospacing {
            out.insert("spaceBeforeAuto", b.into());
        }
        if let Some(b) = sp.after_autospacing {
            out.insert("spaceAfterAuto", b.into());
        }
    }
    if props.keep_next == Some(true) {
        out.insert("keepNext", true.into());
    }
    if props.keep_lines == Some(true) {
        out.insert("keepLines", true.into());
    }
    if let Some(b) = props.page_break_before {
        out.insert("pageBreakBefore", b.into());
    }
    if let Some(b) = props.contextual_spacing {
        out.insert("contextualSpacing", b.into());
    }
    match (props.auto_space_de, props.auto_space_dn) {
        (Some(false), Some(false)) => {
            out.insert("autoSpace", false.into());
        }
        (Some(true), _) | (_, Some(true)) => {
            out.insert("autoSpace", true.into());
        }
        _ => {}
    }
    if let Some(jc) = props.jc.as_ref().and_then(|j| j.value()) {
        use rsword::semantic::props::Jc;
        let align = match jc {
            Jc::Center => Some("center"),
            Jc::Right => Some("right"),
            Jc::Left => Some("left"),
            Jc::Both | Jc::Distribute => Some("justify"),
            _ => None,
        };
        if let Some(a) = align {
            out.insert("align", a.into());
        }
    }
    if let Some(ind) = &props.indent {
        if let Some(l) = i32_of(&ind.start).filter(|&n| n != 0) {
            out.insert("indentLeftTwips", l.into());
        }
        if let Some(r) = i32_of(&ind.end).filter(|&n| n != 0) {
            out.insert("indentRightTwips", r.into());
        }
        let hanging = i32_of(&ind.hanging).unwrap_or(0);
        let first = i32_of(&ind.first_line).unwrap_or(0);
        if hanging > 0 {
            out.insert("indentFirstLineTwips", (-hanging).into());
        } else if first > 0 {
            out.insert("indentFirstLineTwips", first.into());
        }
    }
}

/// TS display 里本测试对照的键（其余：tabStops / shadingFill / indentChars / suppressAutoHyphens 在后续任务）。
const RUN_KEYS: &[&str] = &[
    "sizeHalfPoints",
    "color",
    "bold",
    "italic",
    "boldCs",
    "italicCs",
    "sizeCsHalfPoints",
    "rtl",
    "underline",
    "strike",
    "font",
    "fontAscii",
    "csFont",
    "eaSlotEmpty",
    "charSpacingTwips",
    "caps",
    "vanish",
];
const PARA_KEYS: &[&str] = &[
    "lineRule",
    "lineRawTwips",
    "lineSpacing",
    "spaceBeforeTwips",
    "spaceAfterTwips",
    "spaceBeforeAuto",
    "spaceAfterAuto",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "contextualSpacing",
    "autoSpace",
    "align",
    "indentLeftTwips",
    "indentRightTwips",
    "indentFirstLineTwips",
];

fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => {
            (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() < 1e-6
        }
        _ => a == b,
    }
}

#[test]
fn res_02_style_display_matches_ts() {
    let mut st = Stats::default();
    for path in common::docx_paths("synthetic") {
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let e: Value = serde_json::from_str(&text).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
        st.docs += 1;
        let r = Resolver::new(&doc);
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Some(styles) = e.get("styles").and_then(Value::as_object) else { continue };
        for (id, ts) in styles {
            let Some(kind) = doc.styles.as_ref().and_then(|s| s.get(id)).and_then(|s| s.kind())
            else {
                continue;
            };
            st.styles += 1;
            let mut ours: BTreeMap<&'static str, Value> = BTreeMap::new();
            if kind != StyleType::Table {
                if let Some(rp) = r.style_run_props(id, kind) {
                    run_display(&r, &rp, &mut ours);
                }
                if kind == StyleType::Paragraph
                    && let Some(pp) = r.style_para_props(id)
                {
                    para_display(&pp, &mut ours);
                }
            }
            let ts_display = ts.get("display").and_then(Value::as_object);
            let keys: Vec<&str> = if kind == StyleType::Paragraph {
                RUN_KEYS.iter().chain(PARA_KEYS).copied().collect()
            } else {
                RUN_KEYS.to_vec()
            };
            for key in keys {
                let exp = ts_display.and_then(|d| d.get(key));
                let got = ours.get(key);
                *st.compared.entry("display").or_default() += 1;
                if !same(exp, got) && !known(&file, key) {
                    st.mismatches.push(format!("{file}: {id}.{key}: TS={exp:?} ours={got:?}"));
                }
            }
            if kind == StyleType::Paragraph {
                let exp = ts.get("headingLevel").and_then(Value::as_u64);
                let got = r.heading_level(id).map(u64::from);
                *st.compared.entry("headingLevel").or_default() += 1;
                if exp != got {
                    st.mismatches
                        .push(format!("{file}: {id}.headingLevel: TS={exp:?} ours={got:?}"));
                }
            }
            let exp_shell = ts.get("linkedCharShell").and_then(Value::as_bool).unwrap_or(false);
            let got_shell = r.is_linked_char_shell(id);
            *st.compared.entry("linkedCharShell").or_default() += 1;
            if exp_shell != got_shell {
                st.mismatches
                    .push(format!("{file}: {id}.linkedCharShell: TS={exp_shell} ours={got_shell}"));
            }
        }
        // docDefaults
        if let Some(dd) = e.get("docDefaults").and_then(Value::as_object) {
            let f = r.doc_default_fonts();
            let rpr = doc.styles.as_ref().and_then(|s| s.doc_default_rpr());
            let pairs: Vec<(&str, Option<Value>)> = vec![
                ("sizeHalfPoints", rpr.and_then(|r| u32_of(&r.size)).map(Value::from)),
                ("asciiFont", f.display_ascii().map(Value::from)),
                ("eastAsiaFont", f.east_asia.clone().map(Value::from)),
                ("eaFromLang", f.ea_from_lang.then_some(Value::from(true))),
                (
                    "eaSlotEmpty",
                    (f.ea_slot_empty && f.east_asia.is_some()).then_some(Value::from(true)),
                ),
                (
                    "color",
                    rpr.and_then(|x| x.color.as_ref())
                        .and_then(|c| r.color(c))
                        .map(|c| Value::from(rgb_hex(c))),
                ),
                (
                    "lang",
                    rpr.and_then(|x| x.lang.as_ref()).and_then(|l| l.val.clone()).map(Value::from),
                ),
                ("bold", rpr.and_then(|x| x.bold).filter(|&b| b).map(Value::from)),
                ("italic", rpr.and_then(|x| x.italic).filter(|&b| b).map(Value::from)),
            ];
            for (key, got) in pairs {
                *st.compared.entry("docDefaults").or_default() += 1;
                let exp = dd.get(key);
                if !same(exp, got.as_ref()) && !known(&file, key) {
                    st.mismatches
                        .push(format!("{file}: docDefaults.{key}: TS={exp:?} ours={got:?}"));
                }
            }
        }
    }
    eprintln!("resolve: {} docs, {} styles; compared {:?}", st.docs, st.styles, st.compared);
    for m in st.mismatches.iter().take(40) {
        eprintln!("resolve: MISMATCH {m}");
    }
    assert!(st.styles > 1000, "{}", st.styles);
    assert!(st.mismatches.is_empty(), "{} mismatches against TS StyleDisplay", st.mismatches.len());
}
