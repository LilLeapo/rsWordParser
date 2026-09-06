//! `diff-parse`（`TEST-03`）：对每个 `synthetic/*.docx` 运行 `compat_ts` → JSON，与 `*.expected.json` 按
//! `COMPAT-09` 规则差分；输出按 JSON 路径聚合的差异计数与首个样例；`KNOWN_DIFFS.md` 里的模式跳过并单独计数；
//! 有未知差异时退出码 1（CI 门 `TEST-10`）；`--max-unknown N` 是还没关上的门的棘轮：未知差异不超过 N 就放行，
//! 每落地一个任务就把 N 往下拧，归零后删掉参数。
//!
//! ```text
//! diff-parse [--corpus DIR] [--scope text|fields|tables|drawing|hf|embedded|all] [--known FILE] [--doc PREFIX] [--show N] [--json] [--by-doc] [--max-unknown N]
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rsword::bind::compat_ts::{
    Report, diff_json, is_drawing_path, is_embedded_diff, is_hf_path, is_span_field_case,
    is_table_case, is_text_case, known_diffs, on_embedded_block, parse_known_diffs, parsed_doc,
    split_known,
};
use rsword::package::Package;
use serde_json::{Value, json};

/// 差分的取样范围（`TEST-10` 的里程碑门）。
///
/// 两种筛法：`Text` / `Fields` 按**文档**筛（整份文档在不在这个域里），`Drawing` 按**路径**筛
/// （所有文档照跑，只计绘图域的路径）。绘图文档同时背着字段、表格、页眉页脚的差异，按文档筛
/// 那道门永远关不上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// M1 门：纯文本段落用例。
    Text,
    /// M2 门：文本域 + 字段 / 范围标记 / 批注 / 注释（文本域的超集）。
    Fields,
    /// M3 门：再加表格（字段域的超集；单元格里的绘图归 M4）。
    Tables,
    /// M4 门：全部文档，只计绘图域的路径。
    Drawing,
    /// M5 门：全部文档，只计页眉页脚域的路径。
    Hf,
    /// M6 门：全部文档，只计嵌入对象域——路径本身在域内，或落在期望 label / 标志元素判为
    /// 嵌入对象的块上（`compat_ts::is_embedded_diff`）。
    Embedded,
    /// 全部语料。
    All,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Scope::Text => "text",
            Scope::Fields => "fields",
            Scope::Tables => "tables",
            Scope::Drawing => "drawing",
            Scope::Hf => "hf",
            Scope::Embedded => "embedded",
            Scope::All => "all",
        }
    }
}

struct Args {
    corpus: PathBuf,
    scope: Scope,
    known: Option<PathBuf>,
    doc_prefix: Option<String>,
    show: usize,
    json: bool,
    by_doc: bool,
    /// 放行的未知差异上限（棘轮）；缺省 0。
    max_unknown: usize,
}

fn usage() -> ! {
    eprintln!(
        "用法: diff-parse [--corpus DIR] [--scope text|fields|tables|drawing|hf|embedded|all] [--known KNOWN_DIFFS.md] [--doc PREFIX] [--show N] [--json] [--by-doc] [--max-unknown N]\n\
         scope: text = M1 门（纯文本段落），fields = M2 门（再加字段 / 范围 / 批注），\n\
                tables = M3 门（再加表格），\n\
                drawing = M4 门（全部文档，只计绘图域**路径**），\n\
                hf = M5 门（全部文档，只计页眉页脚域**路径**），\n\
                embedded = M6 门（全部文档，只计嵌入对象域：路径或所在块），all = 全部语料\n\
         --max-unknown N: 未知差异不超过 N 就退出码 0（还没关上的门在 CI 里的棘轮；归零后删掉），缺省 0\n\
         缺省 corpus = <仓库根>/corpus/synthetic，scope = all，known = 编进库里的 KNOWN_DIFFS.md，show = 3"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args {
        corpus: repo_root().join("corpus/synthetic"),
        scope: Scope::All,
        known: None,
        doc_prefix: None,
        show: 3,
        json: false,
        by_doc: false,
        max_unknown: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--corpus" => a.corpus = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--scope" => {
                a.scope = match it.next().as_deref() {
                    Some("text") => Scope::Text,
                    Some("fields") => Scope::Fields,
                    Some("tables") => Scope::Tables,
                    Some("drawing") => Scope::Drawing,
                    Some("hf") => Scope::Hf,
                    Some("embedded") => Scope::Embedded,
                    Some("all") => Scope::All,
                    _ => usage(),
                }
            }
            "--known" => a.known = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--doc" => a.doc_prefix = Some(it.next().unwrap_or_else(|| usage())),
            "--show" => a.show = it.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()),
            "--json" => a.json = true,
            "--by-doc" => a.by_doc = true,
            "--max-unknown" => {
                a.max_unknown = it.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| usage())
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }
    a
}

/// 从可执行文件位置或当前目录向上找含 `corpus/` 的仓库根。
fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if dir.join("corpus").is_dir() && dir.join("Cargo.toml").is_file() {
            return dir;
        }
        if !dir.pop() {
            return PathBuf::from(".");
        }
    }
}

fn short(v: &Option<Value>) -> String {
    match v {
        None => "<缺失>".to_string(),
        Some(v) => {
            let s = v.to_string();
            if s.chars().count() > 120 {
                format!("{}…", s.chars().take(120).collect::<String>())
            } else {
                s
            }
        }
    }
}

fn main() -> ExitCode {
    let args = parse_args();
    let known = match &args.known {
        Some(p) => parse_known_diffs(&std::fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("读不到 {}: {e}", p.display());
            std::process::exit(2)
        })),
        None => known_diffs(),
    };
    // 递归：`corpus/real` 按域分目录，`expected.json` 与 docx 同目录
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut stack = vec![args.corpus.clone()];
    if !args.corpus.is_dir() {
        eprintln!("读不到语料目录 {}", args.corpus.display());
        return ExitCode::from(2);
    }
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "docx") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    let mut report = Report::default();
    let mut skipped_scope = 0usize;
    let mut no_expected = 0usize;
    let mut failed_open = 0usize;
    let mut samples: Vec<(String, String, String, String)> = Vec::new();
    let mut by_doc: Vec<(String, usize)> = Vec::new();
    for path in paths {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(p) = &args.doc_prefix
            && !file.starts_with(p)
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            no_expected += 1;
            continue;
        };
        let expected: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{file}: expected.json 解析失败: {e}");
                no_expected += 1;
                continue;
            }
        };
        let in_scope = match args.scope {
            Scope::Text => is_text_case(&expected),
            Scope::Fields => is_span_field_case(&expected),
            Scope::Tables => is_table_case(&expected),
            // 绘图门 / 页眉页脚门 / 嵌入对象门跑全部文档，筛的是路径（或所在块）不是文档
            Scope::Drawing | Scope::Hf | Scope::Embedded | Scope::All => true,
        };
        if !in_scope {
            skipped_scope += 1;
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let actual = match Package::open(&bytes).and_then(|mut pkg| parsed_doc(&mut pkg)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{file}: 打开 / 解析失败: {e}");
                failed_open += 1;
                continue;
            }
        };
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (mut unknown, k) = split_known(diffs, &file, &known);
        // 绘图门只看绘图域路径；别的域各归各的里程碑，混进来这道门永远关不上。
        // 嵌入对象块上的绘图路径差异（墨迹在 TS 里不可见、画布与 chartex 的图片回退、OLE 变体）
        // 属于 M6 的门（`embedded`），不进这两道门。
        if args.scope == Scope::Drawing {
            unknown.retain(|d| is_drawing_path(&d.path) && !on_embedded_block(&d.path, &expected));
        }
        if args.scope == Scope::Hf {
            unknown.retain(|d| is_hf_path(&d.path) && !on_embedded_block(&d.path, &expected));
        }
        if args.scope == Scope::Embedded {
            unknown.retain(|d| is_embedded_diff(&d.path, &expected));
        }
        if !unknown.is_empty() {
            by_doc.push((file.clone(), unknown.len()));
        }
        for d in unknown.iter().take(args.show) {
            samples.push((file.clone(), d.path.clone(), short(&d.expected), short(&d.actual)));
        }
        report.add(&file, unknown, k);
    }

    if args.json {
        let by_path: Vec<Value> = report
            .by_path
            .iter()
            .map(|(k, st)| {
                json!({ "path": k, "count": st.count, "docs": st.docs, "sample": { "doc": st.sample_doc, "path": st.sample_path, "expected": st.sample_expected, "actual": st.sample_actual } })
            })
            .collect();
        let out = json!({
            "corpus": args.corpus.to_string_lossy(),
            "scope": args.scope.as_str(),
            "docs": report.docs, "docsWithUnknown": report.docs_with_unknown,
            "known": report.known, "unknown": report.unknown,
            "skippedByScope": skipped_scope, "noExpected": no_expected, "failedOpen": failed_open,
            "byPath": by_path,
            "byDoc": by_doc.iter().map(|(d, n)| json!({ "doc": d, "count": n })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!(
            "diff-parse: {} 份文档（scope {}），{} 处已知差异，{} 处未知差异（{} 份文档）；范围外 {}，无 expected {}，打开失败 {}",
            report.docs,
            args.scope.as_str(),
            report.known,
            report.unknown,
            report.docs_with_unknown,
            skipped_scope,
            no_expected,
            failed_open
        );
        if !report.by_path.is_empty() {
            println!("\n{:<8} {:<6} 路径", "次数", "文档");
            for (k, st) in &report.by_path {
                println!("{:<8} {:<6} {k}", st.count, st.docs);
                println!(
                    "         首例 {}: {}\n           TS   = {}\n           ours = {}",
                    st.sample_doc,
                    st.sample_path,
                    short(&st.sample_expected),
                    short(&st.sample_actual)
                );
            }
            if args.by_doc {
                by_doc.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                println!("\n按文档（{} 份）：", by_doc.len());
                for (d, n) in &by_doc {
                    println!("{n:<8} {d}");
                }
            }
            if args.show > 0 && !samples.is_empty() {
                println!("\n每份文档前 {} 处：", args.show);
                for (f, p, e, a) in &samples {
                    println!("  {f}: {p}\n    TS   = {e}\n    ours = {a}");
                }
            }
        }
    }
    if report.unknown > args.max_unknown || failed_open > 0 {
        if args.max_unknown > 0 && !args.json {
            println!("diff-parse: 未知差异 {} 超过预算 {}", report.unknown, args.max_unknown);
        }
        ExitCode::from(1)
    } else {
        if args.max_unknown > 0 && !args.json {
            println!(
                "diff-parse: 未知差异 {} 在预算 {} 内（棘轮）",
                report.unknown, args.max_unknown
            );
        }
        ExitCode::SUCCESS
    }
}

#[allow(dead_code)]
fn _assert_path_type(_: &Path) {}
