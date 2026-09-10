//! 声明模型在语料上的验收（任务 1.4，`MOD-10`）：styles / numbering / settings / theme / fontTable
//! 对全部语料解析无 panic，并与 TS 解析结果（`*.expected.json`）逐字段对照。

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use rsword::model::ColorScheme;
use rsword::model::Theme;
use rsword::model::ThemeSlot;
use rsword::package::{Package, RelType};
use rsword::semantic::props::FontTable;
use rsword::semantic::props::Numbering;
use rsword::semantic::props::Settings;
use rsword::semantic::props::StyleType;
use rsword::semantic::props::Styles;
use rsword::semantic::props::{Codec, DocProtect, Val};
use serde_json::Value;

#[derive(Default)]
struct Stats {
    docs: usize,
    with_expected: usize,
    parts: BTreeMap<&'static str, usize>,
    diags: BTreeMap<&'static str, usize>,
    compared: BTreeMap<&'static str, usize>,
    mismatches: Vec<String>,
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn val_str<T: Copy>(v: &Option<Val<T>>, f: impl Fn(T) -> &'static str) -> Option<String> {
    v.as_ref().map(|x| match x {
        Val::Value(t) => f(*t).to_string(),
        Val::Raw(s) => s.clone(),
    })
}

fn val_i32(v: &Option<Val<i32>>) -> Option<i64> {
    v.as_ref().and_then(|x| x.value().map(|&n| i64::from(n)))
}

/// 已知差异（`src/bind/compat_ts/KNOWN_DIFFS.md`）：(文档名前缀, 字段)。
const KNOWN_DIFFS: &[(&str, &str)] = &[("numbering-defs__012", "level.numFmt")];

fn check(
    st: &mut Stats,
    what: &'static str,
    doc: &Path,
    ok: bool,
    detail: impl FnOnce() -> String,
) {
    *st.compared.entry(what).or_default() += 1;
    let file = doc.file_name().unwrap().to_string_lossy();
    if !ok && KNOWN_DIFFS.iter().any(|(p, w)| file.starts_with(p) && *w == what) {
        *st.compared.entry("known_diff").or_default() += 1;
        return;
    }
    if !ok {
        st.mismatches.push(format!(
            "{}: {what}: {}",
            doc.file_name().unwrap().to_string_lossy(),
            detail()
        ));
    }
}

#[test]
fn mod_10_declarations_parse_on_corpus_and_match_ts() {
    let mut st = Stats::default();
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            st.docs += 1;
            let main = pkg.main_part();
            let mut diags = Vec::new();

            // TS 按固定路径找辅助 part；关系缺失时退回路径（见 bind/compat_ts/KNOWN_DIFFS.md）
            let part_of = |pkg: &Package, kind: RelType, name: &str| {
                pkg.related(main, kind).next().or_else(|| pkg.find_name(name))
            };

            let styles = part_of(&pkg, RelType::Styles, "word/styles.xml").and_then(|id| {
                let dom = pkg.dom(id).ok()??;
                rsword::semantic::props::Styles::from_dom(dom, &mut diags)
            });
            *st.diags.entry("styles").or_default() += diags.len();
            diags.clear();
            let numbering =
                part_of(&pkg, RelType::Numbering, "word/numbering.xml").and_then(|id| {
                    let dom = pkg.dom(id).ok()??;
                    rsword::semantic::props::Numbering::from_dom(dom, &mut diags)
                });
            *st.diags.entry("numbering").or_default() += diags.len();
            for dg in &diags {
                eprintln!(
                    "decl: numbering diag in {}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    dg.message
                );
            }
            diags.clear();
            let settings_part = part_of(&pkg, RelType::Settings, "word/settings.xml");
            let settings = settings_part.and_then(|id| {
                let dom = pkg.dom(id).ok()??;
                rsword::semantic::props::Settings::from_dom(dom, &mut diags)
            });
            *st.diags.entry("settings").or_default() += diags.len();
            diags.clear();
            let theme = part_of(&pkg, RelType::Theme, "word/theme/theme1.xml")
                .and_then(|id| rsword::model::Theme::from_dom(pkg.dom(id).ok()??));
            let font_table =
                part_of(&pkg, RelType::FontTable, "word/fontTable.xml").and_then(|id| {
                    let dom = pkg.dom(id).ok()??;
                    rsword::semantic::props::FontTable::from_dom(dom, &mut diags)
                });
            *st.diags.entry("fontTable").or_default() += diags.len();
            diags.clear();
            for (name, present) in [
                ("styles", styles.is_some()),
                ("numbering", numbering.is_some()),
                ("settings", settings.is_some()),
                ("theme", theme.is_some()),
                ("fontTable", font_table.is_some()),
            ] {
                if present {
                    *st.parts.entry(name).or_default() += 1;
                }
            }

            let expected_path = path.with_extension("expected.json");
            let Ok(text) = std::fs::read_to_string(&expected_path) else { continue };
            let e: Value = serde_json::from_str(&text).unwrap();
            st.with_expected += 1;

            // ---- styles：id / name / type / isDefault（ECMA 归一后）----
            if let Some(es) = e.get("styles").and_then(Value::as_object) {
                let ours = styles.as_ref();
                for (id, s) in es {
                    let Some(our) = ours.and_then(|x| x.get(id)) else {
                        check(&mut st, "style.exists", &path, false, || {
                            format!("TS 有 {id}，本引擎没有")
                        });
                        continue;
                    };
                    check(&mut st, "style.exists", &path, true, String::new);
                    let name = s.get("name").and_then(Value::as_str);
                    check(&mut st, "style.name", &path, our.display_name() == name, || {
                        format!("{id}: name TS={name:?} ours={:?}", our.display_name())
                    });
                    let ty = s.get("type").and_then(Value::as_str);
                    let our_ty = our.kind().map(|k| k.as_str());
                    check(&mut st, "style.type", &path, our_ty == ty, || {
                        format!("{id}: type TS={ty:?} ours={our_ty:?}")
                    });
                    let is_default = s.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
                    let our_default = our
                        .kind()
                        .and_then(|k| ours.unwrap().default_for(k))
                        .is_some_and(|d| d.id() == Some(id));
                    check(&mut st, "style.isDefault", &path, is_default == our_default, || {
                        format!("{id}: isDefault TS={is_default} ours={our_default}")
                    });
                }
                if let Some(ours) = ours {
                    for s in &ours.styles {
                        let Some(id) = s.id() else { continue };
                        let counted = matches!(
                            s.kind(),
                            Some(StyleType::Paragraph | StyleType::Character | StyleType::Table)
                        );
                        if counted {
                            check(&mut st, "style.only_ours", &path, es.contains_key(id), || {
                                format!("本引擎有 {id}（{:?}），TS 没有", s.kind())
                            });
                        }
                    }
                }
            }

            // ---- themeColors：没有 theme part 时 TS 用内建 Office 调色板 ----
            if let Some(ec) = e.get("themeColors").and_then(Value::as_object) {
                let office = rsword::model::ColorScheme::office_default();
                let colors = match &theme {
                    Some(t) => t.colors.as_ref(),
                    None => Some(&office),
                };
                for slot in rsword::model::ThemeSlot::ALL {
                    let exp = ec.get(slot.as_str()).and_then(Value::as_str).map(str::to_string);
                    let ours = colors.and_then(|c| c.get(slot)).map(hex);
                    check(&mut st, "themeColors", &path, exp == ours, || {
                        format!("{}: TS={exp:?} ours={ours:?}", slot.as_str())
                    });
                }
            }

            // ---- themeFonts ----
            if let Some(ef) = e.get("themeFonts").and_then(Value::as_object) {
                let fonts = theme.as_ref().and_then(|t| t.fonts.as_ref());
                let pairs: [(&str, Option<&str>); 6] = [
                    ("major", fonts.and_then(|f| f.major.latin.as_deref())),
                    ("minor", fonts.and_then(|f| f.minor.latin.as_deref())),
                    ("majorEastAsia", fonts.and_then(|f| f.major.ea.as_deref())),
                    ("eastAsia", fonts.and_then(|f| f.minor.ea.as_deref())),
                    ("majorCs", fonts.and_then(|f| f.major.cs.as_deref())),
                    ("minorCs", fonts.and_then(|f| f.minor.cs.as_deref())),
                ];
                for (key, ours) in pairs {
                    let exp = ef.get(key).and_then(Value::as_str);
                    check(&mut st, "themeFonts", &path, exp == ours, || {
                        format!("{key}: TS={exp:?} ours={ours:?}")
                    });
                }
                for (key, slots) in [
                    ("majorScripts", fonts.map(|f| &f.major.scripts)),
                    ("minorScripts", fonts.map(|f| &f.minor.scripts)),
                ] {
                    let exp: BTreeMap<String, String> = ef
                        .get(key)
                        .and_then(Value::as_object)
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    let ours: BTreeMap<String, String> =
                        slots.map(|s| s.iter().cloned().collect()).unwrap_or_default();
                    check(&mut st, "themeFonts.scripts", &path, exp == ours, || {
                        format!("{key}: TS={exp:?} ours={ours:?}")
                    });
                }
                let ea_lang = ef.get("eaLang").and_then(Value::as_str);
                let ours = settings
                    .as_ref()
                    .and_then(|s| s.theme_font_lang.as_ref())
                    .and_then(|l| l.east_asia.as_deref());
                check(&mut st, "themeFonts.eaLang", &path, ea_lang == ours, || {
                    format!("eaLang TS={ea_lang:?} ours={ours:?}")
                });
            }

            // ---- settings 杂项 ----
            let settings_dom = settings_part.and_then(|id| pkg.dom(id).ok().flatten());
            let mode = match (&settings, settings_dom) {
                (Some(s), Some(dom)) => s.compatibility_mode(dom).unwrap_or(0),
                _ => 0,
            };
            if let Some(exp) = e.get("compatibilityMode").and_then(Value::as_u64) {
                check(&mut st, "compatibilityMode", &path, exp == u64::from(mode), || {
                    format!("TS={exp} ours={mode}")
                });
            }
            if let Some(exp) = e.get("evenAndOddHeaders").and_then(Value::as_bool) {
                let ours = settings.as_ref().is_some_and(|s| s.even_and_odd_headers == Some(true));
                check(&mut st, "evenAndOddHeaders", &path, exp == ours, || {
                    format!("TS={exp} ours={ours}")
                });
            }
            if let Some(exp) = e.get("removePersonalInfo").and_then(Value::as_bool) {
                let ours =
                    settings.as_ref().is_some_and(|s| s.remove_personal_information == Some(true));
                check(&mut st, "removePersonalInfo", &path, exp == ours, || {
                    format!("TS={exp} ours={ours}")
                });
            }
            if let Some(exp) = e.get("protection") {
                let ours = settings
                    .as_ref()
                    .and_then(|s| s.document_protection.as_ref())
                    .filter(|p| !matches!(p.edit, None | Some(Val::Value(DocProtect::None))));
                match (exp.as_object(), ours) {
                    (None, None) => check(&mut st, "protection", &path, true, String::new),
                    (Some(o), Some(p)) => {
                        let edit = o.get("edit").and_then(Value::as_str).map(str::to_string);
                        let enforced = o.get("enforced").and_then(Value::as_bool).unwrap_or(false);
                        let our_edit = val_str(&p.edit, DocProtect::as_str);
                        let our_enf = p.enforcement == Some(true);
                        check(
                            &mut st,
                            "protection",
                            &path,
                            edit == our_edit && enforced == our_enf,
                            || format!("TS={exp} ours=edit {our_edit:?} enforced {our_enf}"),
                        );
                    }
                    (o, p) => check(&mut st, "protection", &path, false, || {
                        format!("TS={o:?} ours={p:?}")
                    }),
                }
            }
            if let Some(exp) = e.get("writeProtection") {
                let ours = settings.as_ref().and_then(|s| s.write_protection.as_ref());
                match (exp.as_object(), ours) {
                    (None, None) => check(&mut st, "writeProtection", &path, true, String::new),
                    (Some(o), Some(w)) => {
                        let rec = o.get("recommended").and_then(Value::as_bool).unwrap_or(false);
                        check(
                            &mut st,
                            "writeProtection",
                            &path,
                            rec == (w.recommended == Some(true)),
                            || format!("recommended TS={rec} ours={:?}", w.recommended),
                        );
                    }
                    (o, w) => check(&mut st, "writeProtection", &path, false, || {
                        format!("TS={o:?} ours={w:?}")
                    }),
                }
            }

            // ---- numbering：numId / abstractNumId / 无覆盖时的级别声明 ----
            if let Some(en) = e.get("numbering").and_then(Value::as_object) {
                let ours = numbering.as_ref();
                for (num_id, def) in en {
                    let Ok(id) = num_id.parse::<i32>() else { continue };
                    let Some(num) = ours.and_then(|n| n.num(id)) else {
                        check(&mut st, "num.exists", &path, false, || {
                            format!("TS 有 numId {num_id}，本引擎没有")
                        });
                        continue;
                    };
                    check(&mut st, "num.exists", &path, true, String::new);
                    let abs = def
                        .get("abstractNumId")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<i64>().ok());
                    let our_abs = val_i32(&num.abstract_num_id);
                    check(&mut st, "num.abstractNumId", &path, abs == our_abs, || {
                        format!("{num_id}: TS={abs:?} ours={our_abs:?}")
                    });
                    let Some(abstract_num) =
                        our_abs.and_then(|a| ours.unwrap().abstract_num(a as i32))
                    else {
                        continue;
                    };
                    if !num.overrides.is_empty() || abstract_num.num_style_link.is_some() {
                        continue; // TS 在这里做了合并 / 链解析，属 RES-09
                    }
                    let Some(levels) = def.get("levels").and_then(Value::as_object) else {
                        continue;
                    };
                    for (ilvl, lv) in levels {
                        let Ok(i) = ilvl.parse::<i32>() else { continue };
                        let Some(our_lv) = abstract_num.level(i) else {
                            check(&mut st, "level.exists", &path, false, || {
                                format!("{num_id}/{ilvl}: 本引擎无此级")
                            });
                            continue;
                        };
                        let fmt = lv.get("numFmt").and_then(Value::as_str).map(str::to_string);
                        let our_fmt =
                            our_lv.num_fmt.as_ref().and_then(|f| val_str(&f.val, |v| v.as_str()));
                        check(&mut st, "level.numFmt", &path, fmt == our_fmt, || {
                            format!("{num_id}/{ilvl}: TS={fmt:?} ours={our_fmt:?}")
                        });
                        let text = lv.get("lvlText").and_then(Value::as_str).map(str::to_string);
                        let our_text = our_lv.lvl_text.as_ref().and_then(|t| t.val.clone());
                        check(&mut st, "level.lvlText", &path, text == our_text, || {
                            format!("{num_id}/{ilvl}: TS={text:?} ours={our_text:?}")
                        });
                        let start = lv.get("start").and_then(Value::as_i64);
                        let our_start = i64::from(our_lv.start_or_default());
                        check(&mut st, "level.start", &path, start == Some(our_start), || {
                            format!("{num_id}/{ilvl}: TS={start:?} ours={our_start}")
                        });
                    }
                }
            }
        }
    }

    eprintln!(
        "decl: {} docs ({} with expected.json); parts parsed: {:?}",
        st.docs, st.with_expected, st.parts
    );
    eprintln!("decl: PROP_BAD_VALUE per part kind: {:?}", st.diags);
    eprintln!("decl: comparisons: {:?}", st.compared);
    for m in st.mismatches.iter().take(40) {
        eprintln!("decl: MISMATCH {m}");
    }
    assert!(st.docs > 100 && st.with_expected > 100, "corpus missing?");
    assert!(st.parts["styles"] > 100, "styles parts: {:?}", st.parts);
    assert!(st.mismatches.is_empty(), "{} mismatches against TS", st.mismatches.len());
    let _ = <DocProtect as Codec>::NAME;
}
