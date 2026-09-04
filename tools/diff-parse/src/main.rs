//! `diff-parse`（`TEST-03`）：对每个 `synthetic/*.docx` 运行 `compat_ts` → JSON，与 `*.expected.json` 按
//! `COMPAT-09` 规则差分；输出按 JSON 路径聚合的差异计数与首个样例；`KNOWN_DIFFS.md` 里的模式跳过并单独计数；
//! 有未知差异时退出码 1（CI 门 `TEST-10`）。
//!
//! ```text
//! diff-parse [--corpus DIR] [--scope text|fields|all] [--known FILE] [--doc PREFIX] [--show N] [--json]
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rsword::bind::compat_ts::{
    Report, diff_json, is_span_field_case, is_text_case, known_diffs, parse_known_diffs,
    parsed_doc, split_known,
};
use rsword::package::Package;
use serde_json::{Value, json};

/// 差分的取样范围（`TEST-10` 的里程碑门）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// M1 门：纯文本段落用例。
    Text,
    /// M2 门：文本域 + 字段 / 范围标记 / 批注 / 注释（文本域的超集）。
    Fields,
    /// 全部语料。
    All,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Scope::Text => "text",
            Scope::Fields => "fields",
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
}

fn usage() -> ! {
    eprintln!(
        "用法: diff-parse [--corpus DIR] [--scope text|fields|all] [--known KNOWN_DIFFS.md] [--doc PREFIX] [--show N] [--json]\n\
         scope: text = M1 门（纯文本段落），fields = M2 门（再加字段 / 范围 / 批注），all = 全部语料\n\
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
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--corpus" => a.corpus = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--scope" => {
                a.scope = match it.next().as_deref() {
                    Some("text") => Scope::Text,
                    Some("fields") => Scope::Fields,
                    Some("all") => Scope::All,
                    _ => usage(),
                }
            }
            "--known" => a.known = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--doc" => a.doc_prefix = Some(it.next().unwrap_or_else(|| usage())),
            "--show" => a.show = it.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()),
            "--json" => a.json = true,
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
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(&args.corpus) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "docx"))
            .collect(),
        Err(e) => {
            eprintln!("读不到语料目录 {}: {e}", args.corpus.display());
            return ExitCode::from(2);
        }
    };
    paths.sort();
    let mut report = Report::default();
    let mut skipped_scope = 0usize;
    let mut no_expected = 0usize;
    let mut failed_open = 0usize;
    let mut samples: Vec<(String, String, String, String)> = Vec::new();
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
            Scope::All => true,
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
        let (unknown, k) = split_known(diffs, &file, &known);
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
            if args.show > 0 && !samples.is_empty() {
                println!("\n每份文档前 {} 处：", args.show);
                for (f, p, e, a) in &samples {
                    println!("  {f}: {p}\n    TS   = {e}\n    ours = {a}");
                }
            }
        }
    }
    if report.unknown > 0 || failed_open > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

#[allow(dead_code)]
fn _assert_path_type(_: &Path) {}
