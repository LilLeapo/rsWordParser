//! `xpath-assert`（`TEST-05`）：对 docx 的一个 part（或独立 XML 文件）求 XPath 子集，支持 `count()`、
//! 属性值、`text()`；`--compare` 比较两份文件同一组 XPath 的结果（`COMPAT-08` 的等价比较），不同则退出码 1。
//!
//! ```text
//! xpath-assert <a.docx|a.xml> [--part word/document.xml] (<xpath> ... | --xpaths FILE)
//! xpath-assert --compare <a.docx|a.xml> <b.docx|b.xml> [--part word/document.xml] (<xpath> ... | --xpaths FILE)
//! xpath-assert --prefixes
//! ```
//!
//! 输出每条 XPath 一行：`<xpath>\t<结果 JSON 数组>`。`--xpaths FILE` 每行一条（`#` 注释）。

use std::path::Path;
use std::process::ExitCode;

use rsword::package::{Package, PartId};
use rsword::xml::xpath::{eval_strings, prefixes};
use rsword::xml::{Dom, XPathError};

fn usage() -> ! {
    eprintln!(
        "用法:\n  xpath-assert <a.docx|a.xml> [--part word/document.xml] (<xpath> ... | --xpaths FILE)\n  xpath-assert --compare <a> <b> [--part word/document.xml] (<xpath> ... | --xpaths FILE)\n  xpath-assert --prefixes"
    );
    std::process::exit(2)
}

/// 读 docx 的一个 part，或独立 XML 文件。
fn load(path: &Path, part: &str) -> Result<Dom, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) || bytes.starts_with(b"<") {
        return Dom::parse(PartId(0), &bytes).map_err(|e| format!("{}: {e}", path.display()));
    }
    let mut pkg = Package::open(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let id = pkg.find_name(part).ok_or_else(|| format!("{}: 没有 part {part}", path.display()))?;
    let dom = pkg
        .dom(id)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .ok_or_else(|| format!("{}: {part} 不是 XML", path.display()))?;
    Ok(dom.clone())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--prefixes") {
        for (p, ns) in prefixes() {
            println!("{p}\t{}", ns.uri(rsword::package::PartFlavor::Transitional).unwrap_or(""));
        }
        return ExitCode::SUCCESS;
    }
    let mut files: Vec<String> = Vec::new();
    let mut xpaths: Vec<String> = Vec::new();
    let mut part = "word/document.xml".to_string();
    let mut compare = false;
    let mut it = argv.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--compare" => compare = true,
            "--part" => part = it.next().unwrap_or_else(|| usage()),
            "--xpaths" => {
                let f = it.next().unwrap_or_else(|| usage());
                let text = std::fs::read_to_string(&f).unwrap_or_else(|e| {
                    eprintln!("{f}: {e}");
                    std::process::exit(2)
                });
                xpaths.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .map(str::to_string),
                );
            }
            "-h" | "--help" => usage(),
            _ => {
                let want_files = if compare { 2 } else { 1 };
                if files.len() < want_files
                    && (a.ends_with(".docx") || a.ends_with(".xml") || Path::new(&a).is_file())
                {
                    files.push(a);
                } else {
                    xpaths.push(a);
                }
            }
        }
    }
    let want_files = if compare { 2 } else { 1 };
    if files.len() != want_files || xpaths.is_empty() {
        usage();
    }
    let doms: Vec<Dom> =
        match files.iter().map(|f| load(Path::new(f), &part)).collect::<Result<_, _>>() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        };
    let mut failures = 0usize;
    for xp in &xpaths {
        let results: Vec<Result<Vec<String>, XPathError>> =
            doms.iter().map(|d| eval_strings(d, xp)).collect();
        if compare {
            match (&results[0], &results[1]) {
                (Ok(a), Ok(b)) if a == b => {
                    println!("{xp}\t{}\tOK", serde_json::to_string(a).unwrap())
                }
                (Ok(a), Ok(b)) => {
                    failures += 1;
                    println!(
                        "{xp}\tDIFF\n  {}: {}\n  {}: {}",
                        files[0],
                        serde_json::to_string(a).unwrap(),
                        files[1],
                        serde_json::to_string(b).unwrap()
                    );
                }
                (Err(e), _) | (_, Err(e)) => {
                    failures += 1;
                    println!("{xp}\tERROR\t{e}");
                }
            }
        } else {
            match &results[0] {
                Ok(v) => println!("{xp}\t{}", serde_json::to_string(v).unwrap()),
                Err(e) => {
                    failures += 1;
                    println!("{xp}\tERROR\t{e}");
                }
            }
        }
    }
    if failures > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
