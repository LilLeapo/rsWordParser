//! Test-only GUI driver: create deterministic edited DOCX copies for Word acceptance.
//!
//! This intentionally uses the low-level `Dom::set_text` path and does not claim
//! coverage of the public `EditOp` planner.

use std::{fs, path::Path, path::PathBuf};

use rsword::model::Document;
use rsword::package::{Package, PartId};
use rsword::xml::{Dom, LocalName, NodeId, NodeKind, QName};
use serde_json::json;

const MARKER: &str = "RSWORD-E2E 中文 😀 &<>";

fn first_text(pkg: &mut Package) -> Option<(PartId, NodeId)> {
    let main = pkg.main_part();
    let dom = pkg.dom(main).ok()??;
    let t = QName::w(LocalName::T);
    for id in dom.descendants(dom.root()) {
        if !dom.is(id, t) || inside_table(dom, id) {
            continue;
        }
        for &child in dom.children(id) {
            let has_text = matches!(dom.node(child).kind, NodeKind::Text(_))
                && dom.text(child).is_some_and(|s| !s.trim().is_empty());
            if has_text {
                return Some((main, child));
            }
        }
    }
    None
}

fn inside_table(dom: &Dom, mut node: NodeId) -> bool {
    let table = QName::w(LocalName::Tbl);
    while let Some(parent) = dom.parent(node) {
        if dom.is(parent, table) {
            return true;
        }
        node = parent;
    }
    false
}

fn contains_marker(pkg: &mut Package, marker: &str) -> Result<bool, String> {
    let document = Document::rebuild(pkg).map_err(|e| e.to_string())?;
    Ok(document.text_blocks().any(|block| block.text().contains(marker)))
}

fn run_case(source: &Path, out_dir: &Path) -> Result<serde_json::Value, String> {
    let source = source.canonicalize().map_err(|e| format!("{}: {e}", source.display()))?;
    let bytes = fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let mut package = Package::open(&bytes).map_err(|e| format!("{}: {e}", source.display()))?;
    let no_edit = package.save().map_err(|e| format!("{}: save: {e}", source.display()))?;
    if no_edit != bytes {
        return Err(format!("{}: no-edit save changed bytes", source.display()));
    }

    let (main, text) = first_text(&mut package)
        .ok_or_else(|| format!("{}: no editable first w:t text", source.display()))?;
    let old_text = package
        .dom(main)
        .map_err(|e| e.to_string())?
        .and_then(|dom| dom.text(text).map(|s| s.into_owned()))
        .ok_or_else(|| format!("{}: first text disappeared", source.display()))?;
    package.dom_mut(main).map_err(|e| e.to_string())?.unwrap().set_text(text, MARKER);
    let dirty_parts = package.dirty_parts().len();
    let saved = package.save().map_err(|e| format!("{}: save: {e}", source.display()))?;

    let stem = source.file_stem().and_then(|s| s.to_str()).ok_or("non-UTF8 source stem")?;
    let output = out_dir.join(format!("{stem}.edited.docx"));
    fs::write(&output, &saved).map_err(|e| format!("{}: {e}", output.display()))?;
    let mut reopened =
        Package::open(&saved).map_err(|e| format!("{}: reopen: {e}", source.display()))?;
    if !contains_marker(&mut reopened, MARKER)? {
        return Err(format!("{}: marker not visible after reopen", source.display()));
    }

    Ok(json!({
        "source": source,
        "output": output,
        "operation": "Dom::set_text",
        "old_text": old_text,
        "new_text": MARKER,
        "no_edit_save_byte_identical": true,
        "dirty_parts": dirty_parts,
        "reopen_model_contains_marker": true,
    }))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next().unwrap_or_default();
    if first == "--check" {
        let Some(path) = args.next().map(PathBuf::from) else {
            eprintln!("usage: rsword_e2e_driver --check <file.docx>");
            std::process::exit(2);
        };
        let marker = args.next().unwrap_or_else(|| MARKER.to_string());
        let bytes = fs::read(&path).expect("read check input");
        let mut package = Package::open(&bytes).expect("open check input");
        let visible = contains_marker(&mut package, &marker).expect("check model");
        let saved = package.save().expect("check no-edit save");
        let byte_identical = saved == bytes;
        println!(
            "{}",
            json!({ "file": path, "marker": marker, "contains_marker": visible, "no_edit_save_byte_identical": byte_identical })
        );
        if !visible || !byte_identical {
            std::process::exit(1);
        }
        return;
    }
    let out_dir = PathBuf::from(first);
    let sources: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if sources.is_empty() {
        eprintln!("usage: rsword_e2e_driver <output-dir> <source.docx>...");
        std::process::exit(2);
    }
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir).expect("create output dir");
    }
    let mut cases = Vec::new();
    let mut failures = Vec::new();
    for source in &sources {
        match run_case(source, &out_dir) {
            Ok(case) => cases.push(case),
            Err(err) => failures.push(json!({ "source": source, "error": err })),
        }
    }

    let manifest = json!({ "cases": cases, "failures": failures });
    let manifest_path = out_dir.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap())
        .expect("write manifest");
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
