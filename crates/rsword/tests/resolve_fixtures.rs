//! `RES-12` / `TEST-08`：`fixtures/resolve/**` 的 fixture 断言（`spec/16` 任务 5.8）。
//!
//! 每个 fixture 目录一个 `#[test]`（`fixture_tests!` 展开），读同目录的 `expected.toml`。
//!
//! **观察值必须来自真实 Word**：`RES-04` 的 toggle 规则（ECMA-376 §17.7.3 的奇偶叠加）与
//! [MS-OI29500] 记录的 Word 偏差对不上，只有 Word 的显示结果能定案；`RES-10` 的节继承同理。
//! 所以每条断言带一个 `verified` 标志：
//!
//! - `verified = true`：真的断言。不通过就是引擎错了（以 Word 为准，见 `fixtures/resolve/README.md`）。
//! - `verified = false`：**只记不断言**。测试打印引擎当前的答案与文件里的占位值，让"还没校准"
//!   这件事一直看得见——用引擎自己的输出填期望值等于自证，那比没有断言更糟。
//!
//! 填法见 `fixtures/resolve/README.md`。

mod common;

use std::path::{Path, PathBuf};

use rsword::model::{Document, HfKind, HfVariant};
use rsword::package::Package;
use rsword::resolve::Resolver;
use rsword::resolve::section::HfSlot;
use rsword::semantic::props::RunProps;

fn fixture_dir(rel: &str) -> PathBuf {
    common::repo_root().join("fixtures/resolve").join(rel)
}

/// 一条 run 断言：**按文本**找 run（不是按下标——填表的人看到的是那句话，不是第几个 run），
/// 比对 `bold`，`source` 给了就连来源一起比（`RES-01` 的 `Provenance`，如 `ParaStyle:PBold`）。
#[derive(Debug)]
struct RunRow {
    text: String,
    bold: bool,
    source: Option<String>,
    verified: bool,
}

/// 一条节断言：第 `idx` 节的 default 页眉显示什么、继承自哪一节（`-1` = 自己声明的）。
#[derive(Debug)]
struct SectionRow {
    idx: usize,
    header_default: String,
    inherited_from: i64,
    verified: bool,
}

fn rows_of(v: &toml::Value) -> (Vec<RunRow>, Vec<SectionRow>) {
    let runs = v
        .get("run")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| RunRow {
                    text: r["text"].as_str().expect("text").to_string(),
                    bold: r["bold"].as_bool().expect("bold"),
                    source: r.get("source").and_then(toml::Value::as_str).map(str::to_string),
                    verified: r.get("verified").and_then(toml::Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    let sections = v
        .get("section")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| SectionRow {
                    idx: r["idx"].as_integer().expect("idx") as usize,
                    header_default: r["header_default"].as_str().unwrap_or_default().to_string(),
                    inherited_from: r["inherited_from"].as_integer().unwrap_or(-1),
                    verified: r.get("verified").and_then(toml::Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    (runs, sections)
}

/// 引擎对这份 fixture 的回答。
struct Answers {
    /// `(run 文本, 有效 bold, bold 的来源)`，文档序（含表格里的段落）。
    runs: Vec<(String, bool, String)>,
    /// 每节 default 页眉的文本与继承来源。
    sections: Vec<(String, i64)>,
}

fn answers(dir: &Path) -> Answers {
    let bytes = std::fs::read(dir.join("doc.docx"))
        .unwrap_or_else(|e| panic!("{}: {e}", dir.join("doc.docx").display()));
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = Document::rebuild(&mut pkg).expect("rebuild");
    let r = Resolver::new(&doc);
    let mut runs = Vec::new();
    // 表格里的段落要走 `RES-08` 的表格视图，不然 `firstRow` 那一层根本没参与（`RES-03` 第 4 层）
    let dom = pkg.part(doc.main_part).dom().expect("main dom");
    let mut table_rpr: std::collections::HashMap<rsword::xml::NodeId, RunProps> =
        std::collections::HashMap::new();
    for t in doc.tables() {
        let view = r.table(dom, t);
        for (ri, row) in t.rows.iter().enumerate() {
            for (ci, cell) in row.cells.iter().enumerate() {
                let rpr = view.cell(ri, ci).rpr;
                for b in cell.text_blocks() {
                    table_rpr.insert(b.node, rpr.clone());
                }
            }
        }
    }
    for p in doc.paragraphs() {
        let para_style = p.style_id.as_deref();
        let cell_rpr = table_rpr.get(&p.node);
        for inline in &p.inlines {
            let rsword::model::Inline::Run(run) = inline else { continue };
            let text = run.text.trim();
            if text.is_empty() {
                continue;
            }
            let eff = r.run_in_table(cell_rpr, para_style, run.props.style.as_deref(), &run.props);
            let source = provenance_str(&eff.source(rsword::semantic::props::RunPropsField::Bold));
            runs.push((text.to_string(), eff.props.bold == Some(true), source));
        }
    }
    let mut sections = Vec::new();
    let text_of = |rid: &str| -> String {
        doc.hf_by_rel.get(rid).and_then(|p| doc.hf_parts.get(p)).map(hf_text).unwrap_or_default()
    };
    for i in 0..doc.sections.len() {
        let view = r.section(&doc.sections, i).expect("section view");
        let (text, from) = match view.slot(HfKind::Header, HfVariant::Default) {
            HfSlot::Declared(rid) => (text_of(rid), -1),
            HfSlot::Inherited { from, id } => (text_of(id), *from as i64),
            // `-2` = 这个槽在这一节完全没有（既没声明也继承不到）
            HfSlot::Absent => (String::new(), -2),
        };
        sections.push((text, from));
    }
    Answers { runs, sections }
}

/// `Provenance` → `expected.toml` 里的写法（`ParaStyle:PBold` / `Direct` / `DocDefaults` …）。
fn provenance_str(p: &rsword::resolve::Provenance) -> String {
    use rsword::resolve::Provenance as P;
    match p {
        P::Direct => "Direct".into(),
        P::CharStyle(id) => format!("CharStyle:{id}"),
        P::ParaStyle(id) => format!("ParaStyle:{id}"),
        P::TableStyle { .. } => "TableStyle".into(),
        P::NumberingLevel { num_id, ilvl } => format!("NumberingLevel:{num_id}:{ilvl}"),
        P::DocDefaults => "DocDefaults".into(),
        P::Theme => "Theme".into(),
        P::Default => "Default".into(),
    }
}

fn hf_text(hf: &rsword::model::HfPart) -> String {
    hf.text_blocks()
        .map(rsword::model::TextBlock::text)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// 一个 fixture 目录：读 `expected.toml`，`verified` 的断言、没 verified 的只打印。
fn check(rel: &str) {
    let dir = fixture_dir(rel);
    let toml_path = dir.join("expected.toml");
    let text = std::fs::read_to_string(&toml_path)
        .unwrap_or_else(|e| panic!("{}: {e}", toml_path.display()));
    let value: toml::Value =
        toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", toml_path.display()));
    let (runs, sections) = rows_of(&value);
    assert!(!runs.is_empty() || !sections.is_empty(), "{rel}: expected.toml 一条断言都没有");
    let got = answers(&dir);
    let mut pending = 0usize;

    for row in &runs {
        let found = got.runs.iter().find(|(t, _, _)| t == &row.text);
        let (_, bold, source) = found.unwrap_or_else(|| {
            panic!("{rel}: 文档里找不到文本 {:?}；引擎看到的是 {:?}", row.text, got.runs)
        });
        if row.verified {
            assert_eq!(*bold, row.bold, "{rel}: run {:?} 的 bold", row.text);
            if let Some(want) = &row.source {
                assert_eq!(source, want, "{rel}: run {:?} 的 bold 来源", row.text);
            }
        } else {
            pending += 1;
            eprintln!(
                "  [待 Word 校准] {rel} run {:?}: 引擎说 bold={bold}（来源 {source}），文件里占位 {}",
                row.text, row.bold
            );
        }
    }
    for row in &sections {
        let (text, from) = got
            .sections
            .get(row.idx)
            .unwrap_or_else(|| panic!("{rel}: 没有第 {} 节（共 {}）", row.idx, got.sections.len()));
        if row.verified {
            assert_eq!(text, &row.header_default, "{rel}: 第 {} 节的 default 页眉", row.idx);
            assert_eq!(*from, row.inherited_from, "{rel}: 第 {} 节的继承来源", row.idx);
        } else {
            pending += 1;
            eprintln!(
                "  [待 Word 校准] {rel} 第 {} 节: 引擎说 header={text:?} 继承自 {from}，文件里占位 {:?} / {}",
                row.idx, row.header_default, row.inherited_from
            );
        }
    }
    if pending > 0 {
        eprintln!("{rel}: {pending} 条断言等真实 Word 的观察值（见 fixtures/resolve/README.md）");
    }
}

/// 每个 fixture 目录展开一个 `#[test]`：失败信息里直接看得出是哪个 fixture。
macro_rules! fixture_tests {
    ($( $name:ident => $rel:literal ),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                check($rel);
            }
        )+

        /// 目录里的 fixture 都上了表（新加一份 fixture 忘了登记就会失败）。
        #[test]
        fn res_12_every_fixture_directory_is_registered() {
            let listed = [$( $rel ),+];
            let root = fixture_dir("");
            let mut found = Vec::new();
            for area in std::fs::read_dir(&root).expect("fixtures/resolve").flatten() {
                if !area.path().is_dir() {
                    continue;
                }
                for case in std::fs::read_dir(area.path()).expect("area").flatten() {
                    if case.path().join("expected.toml").exists() {
                        let rel = format!(
                            "{}/{}",
                            area.file_name().to_string_lossy(),
                            case.file_name().to_string_lossy()
                        );
                        found.push(rel);
                    }
                }
            }
            found.sort();
            let mut want: Vec<String> = listed.iter().map(|s| s.to_string()).collect();
            want.sort();
            assert_eq!(found, want, "fixtures/resolve 下的目录与登记表不一致");
        }
    };
}

fixture_tests! {
    res_04_para_and_char => "toggle/para-and-char",
    res_04_docdefaults_and_para => "toggle/docdefaults-and-para",
    res_04_based_on_two_levels => "toggle/based-on-two-levels",
    res_04_table_first_row => "toggle/table-first-row",
    res_04_direct_off => "toggle/direct-off",
    res_10_sections_inherit_default => "sections/inherit-default",
}
