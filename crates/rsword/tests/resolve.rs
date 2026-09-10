//! resolve 在语料上的验收（任务 1.9，`RES-02/05/06` DoD "与 TS `StyleDisplay` 对照"）：
//! 每个样式经 basedOn 链 + linked 补缺后的 run / 段落属性，与 TS `styles[id].display`、`headingLevel`、
//! `numPr`、`linkedCharShell` 及 `docDefaults` 逐字段对照。

mod common;

use std::collections::BTreeMap;

use rsword::package::Package;
use rsword::resolve::{Resolver, rgb_hex};
use rsword::semantic::props::StyleType;
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
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
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
            if kind != rsword::semantic::props::StyleType::Table {
                if let Some(rp) = r.style_run_props(id, kind) {
                    run_display(&r, &rp, &mut ours);
                }
                if kind == rsword::semantic::props::StyleType::Paragraph
                    && let Some(pp) = r.style_para_props(id)
                {
                    para_display(&pp, &mut ours);
                }
            }
            let ts_display = ts.get("display").and_then(Value::as_object);
            let keys: Vec<&str> = if kind == rsword::semantic::props::StyleType::Paragraph {
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
            if kind == rsword::semantic::props::StyleType::Paragraph {
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

/// `RES-05` DrawingML 颜色（`spec/15` 任务 4.2）在语料上的普查：正文里每个颜色容器都要能定出 sRGB。
///
/// 这是 4.5 / 4.6 的前置保险——形状的填充与描边全靠它，等到那时才发现「某种底色没实现」就太晚了。
#[test]
fn res_05_drawingml_colors_resolve_across_the_corpus() {
    use rsword::model::ColorScheme;
    use rsword::resolve::drawingml::{color_in, hex};
    use rsword::xml::{LocalName, NsId};

    /// 恰好包一个颜色元素的容器（`a:noFill` / `a:blipFill` 不算）。
    fn is_color_container(local: LocalName) -> bool {
        matches!(
            local,
            LocalName::SolidFill
                | LocalName::Gs
                | LocalName::FillRef
                | LocalName::LnRef
                | LocalName::FgClr
                | LocalName::BgClr
        )
    }

    let mut docs = 0usize;
    let mut containers = 0usize;
    let mut by_hex: BTreeMap<String, usize> = BTreeMap::new();
    let mut unresolved: Vec<String> = Vec::new();

    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
        let palette: ColorScheme = Resolver::new(&doc).palette().clone();
        docs += 1;
        let main = pkg.main_part();
        let Ok(Some(dom)) = pkg.dom(main) else { continue };
        for n in dom.semantic_descendants(dom.root()) {
            let Some(name) = dom.name(n) else { continue };
            if name.ns != NsId::A || !is_color_container(name.local) {
                continue;
            }
            let Some(c) = color_in(dom, n) else { continue };
            containers += 1;
            match c.to_rgb(&palette) {
                Some(rgb) => *by_hex.entry(hex(rgb)).or_default() += 1,
                None => unresolved.push(format!("{file}: {:?}", c.base)),
            }
        }
    }

    println!(
        "drawingml colors: {docs} 份文档，{containers} 个颜色容器，{} 种取值；未定出 {}",
        by_hex.len(),
        unresolved.len()
    );
    assert!(containers > 0, "语料里应当有 DrawingML 颜色");
    assert!(unresolved.is_empty(), "定不出 sRGB 的颜色：\n{}", unresolved.join("\n"));
}

// ---- RES-05 符号字体（任务 2.7）----

/// `RES-05`：`w:sym` 与符号字体 run 的显示文本经映射表解码，表外码位保留原字符。
#[test]
#[cfg(feature = "compat-ts")]
fn res_05_symbol_fonts_decode_for_display() {
    use rsword::bind::compat_ts::parsed_doc;

    let case = |body: &str| -> Vec<serde_json::Value> {
        let bytes = common::docx_with_body(body);
        let mut pkg = rsword::package::Package::open(&bytes).unwrap();
        let v = parsed_doc(&mut pkg).unwrap();
        v["blocks"][0]["runs"].as_array().cloned().unwrap_or_default()
    };

    // w:sym：Wingdings F0FC → ✓、6C → ●（相邻同格式 run 会被合并）
    let runs = case(
        r#"<w:p><w:r><w:t>勾:</w:t></w:r><w:r><w:sym w:font="Wingdings" w:char="F0FC"/></w:r>
           <w:r><w:sym w:font="Wingdings" w:char="6C"/></w:r></w:p>"#,
    );
    assert_eq!(runs[0]["text"], "勾:✓●", "{runs:?}");

    // 表外码位保留原字符（U+F000 + 码位）
    let runs = case(r#"<w:p><w:r><w:sym w:font="Wingdings 2" w:char="F045"/></w:r></w:p>"#);
    assert_eq!(runs[0]["text"], "\u{F045}", "{runs:?}");

    // 符号字体 run 的 PUA 文本解码，`w:rFonts` 随之摘掉（TS 行为）
    let runs = case(
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr><w:t>&#xF0B7;</w:t></w:r></w:p>"#,
    );
    assert_eq!(runs[0]["text"], "•");
    assert_eq!(runs[0]["rawRPr"], "<w:rPr></w:rPr>", "解码后的 run 不再带符号字体");
    assert!(runs[0].get("font").is_none() && runs[0].get("fontAscii").is_none(), "{runs:?}");

    // 符号字体 run 里的普通 ASCII 不解码，字体键照常
    let runs = case(
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Wingdings" w:hAnsi="Wingdings"/></w:rPr><w:t>le</w:t></w:r></w:p>"#,
    );
    assert_eq!(runs[0]["text"], "le");
    assert_eq!(runs[0]["font"], "Wingdings");
    assert!(runs[0]["rawRPr"].as_str().unwrap().contains("w:rFonts"));
}

// ---- RES-04 toggle 歧义的常驻测量（`docs/06` 第 2 件，`spec/18` 7.9）-------------------------

/// 这个 run 的这个 toggle 字段落在**歧义**上没有。
///
/// 歧义 = 「按层级异或」与「最具体的声明胜出」两条规则会给出不同答案的形状，也就是：
/// run 自己的 `w:rPr` **没有**声明它（直接格式在两条规则里都一票定音，有它就没歧义），
/// 而 `docDefaults` / 段落样式链 / 字符样式链里**两个及以上**声明了它
/// （链内部的 `basedOn` 是普通的"子覆盖父"，整条链只算一层）。
///
/// 表格样式那一层这里不算：它要有表格上下文才取得到，语料里带 toggle 的表格样式是 0 份。
fn toggle_ambiguous(
    r: &Resolver<'_>,
    doc_default: Option<&RunProps>,
    para_style: Option<&str>,
    char_style: Option<&str>,
    direct: &RunProps,
    f: rsword::semantic::props::RunPropsField,
) -> bool {
    use rsword::resolve::toggle_of;
    if toggle_of(direct, f).is_some() {
        return false;
    }
    let declared_in = |id: Option<&str>, kind: StyleType| {
        id.is_some_and(|i| {
            r.chain(i, kind)
                .iter()
                .any(|s| s.rpr.as_ref().is_some_and(|p| toggle_of(p, f).is_some()))
        })
    };
    let levels = usize::from(doc_default.is_some_and(|p| toggle_of(p, f).is_some()))
        + usize::from(declared_in(para_style, rsword::semantic::props::StyleType::Paragraph))
        + usize::from(declared_in(char_style, rsword::semantic::props::StyleType::Character));
    levels >= 2
}

/// `docs/06` 第 2 件：toggle 歧义在野外到底被触发没有。遍历 `corpus/synthetic` 与 `corpus/real`，
/// 数有多少 (run, toggle 字段) 落在歧义形状上（判据见 [`toggle_ambiguous`]），按字段给出分布。
///
/// **这是测量，不是判定**：非 0 也不 fail，只把数字打出来（`--nocapture`）。判定阈值写在
/// `docs/06`——只要一直是 0，`RES-04` 那条规则就一直不影响产品；一旦非 0，说明真实用户文档
/// 会走到这条分支，桌面版复核的优先级立刻上升。
#[test]
fn res_04_toggle_ambiguity_probe() {
    use rsword::model::Block;
    let mut runs = 0usize;
    let mut docs = 0usize;
    let mut hits: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_doc: BTreeMap<String, usize> = BTreeMap::new();
    for kind in ["synthetic", "real"] {
        for path in common::docx_paths(kind) {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
            docs += 1;
            let r = Resolver::new(&doc);
            let doc_default = doc.styles.as_ref().and_then(|s| s.doc_default_rpr());
            // 正文 + 页眉页脚 + 脚注尾注 + 批注条目，每一处再下到文本框内容流
            let mut roots: Vec<&[Block]> = vec![&doc.main];
            roots.extend(doc.hf_parts.values().map(|hf| hf.blocks.as_slice()));
            for notes in [&doc.footnotes, &doc.endnotes] {
                roots.extend(notes.items.iter().map(|n| n.blocks.as_slice()));
            }
            roots.extend(doc.comments.items.iter().map(|c| c.blocks.as_slice()));
            let mut queue: Vec<&[Block]> = roots;
            let mut all: Vec<&Block> = Vec::new();
            while let Some(list) = queue.pop() {
                for b in rsword::model::Blocks::over(list) {
                    all.push(b);
                    queue.extend(
                        rsword::model::box_flows(b)
                            .into_iter()
                            .map(|(c, _)| c)
                            .filter(|c| !c.is_empty()),
                    );
                }
            }
            for b in all {
                let rsword::model::Block::Text(tb) = b else { continue };
                let para_style = tb.style_id.as_deref();
                for i in &tb.inlines {
                    let rsword::model::Inline::Run(run) = i else { continue };
                    runs += 1;
                    let char_style = run.props.style.as_deref();
                    for &f in rsword::resolve::TOGGLE_FIELDS {
                        if toggle_ambiguous(&r, doc_default, para_style, char_style, &run.props, f)
                        {
                            *hits.entry(format!("{f:?}")).or_default() += 1;
                            *by_doc
                                .entry(
                                    path.file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string(),
                                )
                                .or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    // 我们自己为 `RES-04` 造的校准件（`fixtures/resolve/toggle` 与它们在真实 Word 里另存的
    // 那几份）本来就是**故意**歧义的，不算"野外撞上"
    let ours = |stem: &str| stem.starts_with("toggle-") || stem.starts_with("c-toggle-");
    let total: usize = hits.values().sum();
    let wild: usize = by_doc.iter().filter(|(d, _)| !ours(d)).map(|(_, n)| n).sum();
    eprintln!(
        "RES-04 toggle 歧义探针：{docs} 份文档、{runs} 个 run，撞上歧义 {total} 次（校准件之外 {wild} 次）"
    );
    if total > 0 {
        eprintln!("  按字段：{hits:?}");
        eprintln!("  按文档：{by_doc:?}");
    }
    if wild > 0 {
        eprintln!(
            "  **校准件之外也撞上了**：按 `docs/06` 第 2 件的阈值，桌面版复核 `RES-04` 的优先级要提上来"
        );
    }
    // 探针要真的扫过语料才有意义（语料 799 + 真实 266）
    assert!(docs > 1000, "扫到的文档太少（{docs}），探针没意义");
    assert!(runs > 2000, "扫到的 run 太少（{runs}），探针没意义");
}
