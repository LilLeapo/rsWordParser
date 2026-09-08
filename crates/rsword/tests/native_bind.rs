//! `BIND-02` 模型 JSON 的门测试（`spec/19` M8′ 门 1 与门 6 的体积行，任务 8.2）：
//!
//! 1. 门 1：`document()` 对全部语料（synthetic + real + hostile）过 JSON Schema
//!    （schema 由 `model_json!` 同表生成）；JSON → `Value` → JSON 的 serde 往返逐字节幂等；
//!    `MOD-01`–`MOD-11` 的字段 checklist 不丢字段（独立第二来源：本文件手写 spec 清单）。
//! 2. 门 6（体积）：`display` 关闭时带图语料的 `document()` JSON 体积较
//!    `compat_ts::parsed_doc`（含 dataURL 与 `internal.documentXml`）降 ≥ 50%（聚合）。

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use rsword::bind::native::{DocumentOpts, document_json, document_schema};
use rsword::model::Document;
use rsword::package::Package;
use serde::Deserialize as _;
use serde_json::Value;

fn corpus() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for kind in ["synthetic", "real", "hostile"] {
        v.extend(common::docx_paths(kind));
    }
    v
}

fn project(path: &PathBuf, display: bool) -> (Package, Value) {
    let bytes = std::fs::read(path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc = Document::rebuild(&mut pkg).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let json = document_json(&pkg, &doc, DocumentOpts { display }).0;
    (pkg, json)
}

/// hostile 文档允许打开 / 重建失败（那是设计好的降级：`BIND-01` 里 `open` 返回 `Err`，
/// 根本轮不到 `document()`）；返回 `None`。
fn project_hostile(path: &PathBuf) -> Option<(Package, Value)> {
    let bytes = std::fs::read(path).unwrap();
    let mut pkg = Package::open(&bytes).ok()?;
    let doc = Document::rebuild(&mut pkg).ok()?;
    let json = document_json(&pkg, &doc, DocumentOpts::default()).0;
    Some((pkg, json))
}

/// `MOD-01`–`MOD-11` 的字段 checklist（`BIND-02` 验收：投影不丢字段）。本清单手写自
/// `spec/06` 与 `spec/21`，是 `model_json!` 表之外的**独立来源**：表里整行被删会在这里红。
#[test]
fn bind_02_mod_field_checklist() {
    let schema = document_schema();
    let defs = schema.get("$defs").expect("$defs");

    // (类型, 恒写键, 可选键)——恒写 = schema required，可选 = properties 里有但非 required。
    let cases: &[(&str, &[&str], &[&str])] = &[
        // `MOD-01`（`spec/21` 顶层形态 17 键 + `spec/19` 8.2 的 inks）+ 表内登记的补充键。
        (
            "Document",
            &[
                "main",
                "sections",
                "hfParts",
                "footnotes",
                "endnotes",
                "comments",
                "sources",
                "media",
                "spans",
                "fields",
                "revisions",
                "warnings",
                "inks",
                "mainPart",
                "chartParts",
                "diagramParts",
            ],
            &["styles", "numbering", "theme", "fontTable", "settings", "body", "sourcesPart"],
        ),
        // `MOD-02`（kind 与内标签撞名，改名 textKind / protectedKind，见 docs/04 §8）。
        (
            "TextBlock",
            &["node", "textKind", "props", "inlines", "revisions", "facts"],
            &["styleId", "sdt"],
        ),
        (
            "ProtectedBlock",
            &["node", "protectedKind", "preview", "revisions"],
            &["display", "siblings", "sdt"],
        ),
        // `MOD-06`
        (
            "Run",
            &["node", "segments", "text", "utf16Len", "props", "comments"],
            &["link", "field", "rev"],
        ),
        ("Segment", &["node", "kind", "text", "utf16Len"], &["display"]),
        // `MOD-07`
        ("TableBlock", &["node", "props", "grid", "rows", "revisions"], &["styleId", "sdt"]),
        ("Row", &["node", "props", "cells", "revisions"], &["tblPrEx", "sdt"]),
        ("Cell", &["node", "props", "blocks", "revisions"], &["sdt"]),
        // `MOD-08`
        (
            "SdtInfo",
            &["node", "control", "lock"],
            &["alias", "tag", "id", "dataBinding", "docPart", "placeholder", "showingPlaceholder"],
        ),
        // `MOD-09`
        ("RevisionEntry", &["id", "part", "kind", "meta", "owner", "depth"], &["moveName", "pair"]),
        // `MOD-10`
        ("SectionInfo", &["props", "owner", "blockRange", "revisions"], &["node"]),
        // `BIND-07`
        ("Diagnostic", &["part", "code", "origin", "message"], &["range"]),
        // `BIND-05`
        ("MediaEntry", &["mediaId", "partId", "uri", "mime", "kind"], &[]),
    ];

    for (ty, required, optional) in cases {
        let def = defs.get(ty).unwrap_or_else(|| panic!("$defs 缺 {ty}"));
        let props: BTreeSet<&str> =
            def["properties"].as_object().expect("properties").keys().map(String::as_str).collect();
        let req: BTreeSet<&str> = def["required"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let want_req: BTreeSet<&str> = required.iter().copied().collect();
        let want_all: BTreeSet<&str> = required.iter().chain(optional.iter()).copied().collect();
        assert_eq!(props, want_all, "{ty}: schema 键集与 MOD checklist 不符");
        assert_eq!(req, want_req, "{ty}: schema 必填集与 MOD checklist 不符");
    }
}

/// 门 1：全语料过 JSON Schema；serde 往返逐字节幂等。
#[test]
fn bind_02_corpus_schema_and_roundtrip() {
    let validator = jsonschema::validator_for(&document_schema()).expect("schema 有效");
    let mut docs = 0;
    let mut hostile_unopenable = 0;
    let mut schema_fail = 0;
    for path in corpus() {
        let hostile = path.starts_with(common::corpus_dir("hostile"));
        let (_, json) = if hostile {
            match project_hostile(&path) {
                Some(v) => v,
                None => {
                    hostile_unopenable += 1;
                    continue;
                }
            }
        } else {
            project(&path, false)
        };
        docs += 1;
        // JSON → Value → JSON 逐字节幂等（深度表禁用递归上限：模型把嵌套截断在 64 层
        // （`MOD-07` TooDeep），JSON 深度有界；禁用上限只为深表，不为病态输入放行）
        let s1 = json.to_string();
        let mut de = serde_json::Deserializer::from_str(&s1);
        de.disable_recursion_limit();
        let s2 = serde_json::Value::deserialize(&mut de)
            .unwrap_or_else(|e| panic!("{}: 投影不是合法 JSON: {e}", path.display()))
            .to_string();
        assert_eq!(s1, s2, "{}: serde 往返不幂等", path.display());
        // schema 校验
        if !validator.is_valid(&json) {
            schema_fail += 1;
            if schema_fail <= 5 {
                for e in validator.iter_errors(&json).take(3) {
                    eprintln!("schema FAIL {}: {e} @ {}", path.display(), e.instance_path());
                }
            }
        }
    }
    eprintln!(
        "bind_02: {docs} docs, {schema_fail} schema failures, {hostile_unopenable} hostile 打不开（Err 降级）"
    );
    assert!(docs >= 1090, "语料数不对：{docs}");
    assert_eq!(schema_fail, 0, "{schema_fail} 份文档未过 schema");
}

/// 门 1 的 `display: true` 半边：显示模型（`MOD-11`）的投影同样全语料过 schema + 往返幂等。
#[test]
fn bind_02_corpus_display_schema_and_roundtrip() {
    let validator = jsonschema::validator_for(&document_schema()).expect("schema 有效");
    let mut docs = 0;
    let mut schema_fail = 0;
    for path in corpus() {
        let hostile = path.starts_with(common::corpus_dir("hostile"));
        let json = if hostile {
            match project_hostile(&path) {
                Some(_) => project(&path, true).1,
                None => continue,
            }
        } else {
            project(&path, true).1
        };
        docs += 1;
        let s1 = json.to_string();
        let mut de = serde_json::Deserializer::from_str(&s1);
        de.disable_recursion_limit();
        let s2 = serde_json::Value::deserialize(&mut de)
            .unwrap_or_else(|e| panic!("{}: display 投影不是合法 JSON: {e}", path.display()))
            .to_string();
        assert_eq!(s1, s2, "{}: display serde 往返不幂等", path.display());
        if !validator.is_valid(&json) {
            schema_fail += 1;
            if schema_fail <= 5 {
                for e in validator.iter_errors(&json).take(3) {
                    eprintln!(
                        "display schema FAIL {}: {e} @ {}",
                        path.display(),
                        e.instance_path()
                    );
                }
            }
        }
    }
    eprintln!("bind_02 display: {docs} docs, {schema_fail} schema failures");
    assert!(docs >= 1090, "语料数不对：{docs}");
    assert_eq!(schema_fail, 0, "{schema_fail} 份文档 display 投影未过 schema");
}

/// 门 6（体积）：带图语料 `display` 关闭时的聚合体积较 `compat_ts::parsed_doc` 降 ≥ 50%。
#[test]
fn bind_02_volume_reduction() {
    let mut with_media = 0;
    let mut ours_total = 0usize;
    let mut ts_total = 0usize;
    for path in common::docx_paths("synthetic").into_iter().chain(common::docx_paths("real")) {
        let bytes = std::fs::read(&path).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let has_media = pkg
            .parts()
            .iter()
            .any(|p| !p.deleted && pkg.content_types().image_mime(&p.uri).is_some());
        if !has_media {
            continue;
        }
        with_media += 1;
        let doc = Document::rebuild(&mut pkg).unwrap();
        ours_total += document_json(&pkg, &doc, DocumentOpts::default()).to_string().len();
        ts_total += rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap().to_string().len();
    }
    let pct = 100.0 * (1.0 - ours_total as f64 / ts_total as f64);
    eprintln!(
        "bind_02 volume: {with_media} 带图文档, compat_ts {ts_total}B -> native {ours_total}B (-{pct:.1}%)"
    );
    assert!(with_media >= 50, "带图语料数不对：{with_media}");
    assert!(pct >= 50.0, "体积降幅不足 50%：{pct:.1}%");
}
