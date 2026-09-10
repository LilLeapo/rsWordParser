//! `BIND-02` 模型 JSON 的门测试（`spec/19` M8′ 门 1 与门 6 的体积行，任务 8.2）：
//!
//! 1. 门 1：`document()` 对全部语料（synthetic + real + hostile）过 JSON Schema
//!    （schema 由 `model_json!` 同表生成）；投影确定性、重建稳定性、键集严格性与深度护栏；
//!    `MOD-01`–`MOD-11` 的字段 checklist 不丢字段（独立第二来源：本文件手写 spec 清单）。
//! 2. 门 6（体积）：`display` 关闭时带图语料的 `document()` JSON 体积较
//!    `compat_ts::parsed_doc`（含 dataURL 与 `internal.documentXml`）降 ≥ 50%（聚合）。

mod common;

use std::collections::{BTreeMap, BTreeSet};
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

// 四份必须在 Package::open 阶段拒绝；其余文件（包括 hostile）必须打开并重建成功。
const UNOPENABLE: [&str; 4] = [
    "xml-unbalanced-main.docx",
    "zip-part-too-large.docx",
    "zip-too-many-parts.docx",
    "zip-total-too-large.docx",
];

// 全语料实测最深为 deep-nested-table__001 的 392 层；xml-deep-table 为 388 层，
// hf-deep-txbx 开 display 为 87 层。上限留 56 层（约 14%）余量。
// 这是投影门的预算，不假定 MOD-07 的 64 层限制在各嵌套轴之间如何共享。
const MAX_JSON_DEPTH: usize = 448;

/// 扫描容器括号，跳过字符串和转义；在反序列化及 schema 遍历之前执行。
fn json_depth(json: &str) -> usize {
    let (mut depth, mut max, mut string, mut escape) = (0usize, 0, false, false);
    for byte in json.bytes() {
        if string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    max = max.max(depth);
                }
                b'}' | b']' => depth -= 1,
                _ => {}
            }
        }
    }
    max
}

/// 沿 schema 的类型边走实例，不以实例已有键猜类型（否则多余键会使类型匹配落空）。
/// allOf 先合并声明键；oneOf / anyOf 只选匹配分支；动态 map 的值继续检查。
/// 两层工作栈均为迭代，避免深表在测试辅助代码里耗尽调用栈。
struct KeyChecker<'a> {
    root: &'a Value,
    branches: BTreeMap<*const Value, jsonschema::Validator>,
}

impl<'a> KeyChecker<'a> {
    fn new(root: &'a Value) -> Self {
        Self { root, branches: BTreeMap::new() }
    }

    fn check(&mut self, json: &Value) -> Result<(), String> {
        let mut pending = vec![(json, vec![self.root], String::from("$"))];
        while let Some((value, mut schemas, path)) = pending.pop() {
            let mut props: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
            let mut maps = Vec::new();
            let mut items = Vec::new();
            let mut prefixes = Vec::new();
            let mut fixed_object = false;
            while let Some(schema) = schemas.pop() {
                if let Some(reference) = schema["$ref"].as_str() {
                    let pointer = reference.strip_prefix('#').expect("只允许本地 schema 引用");
                    schemas.push(self.root.pointer(pointer).expect("schema 引用必须存在"));
                }
                if let Some(arms) = schema["allOf"].as_array() {
                    schemas.extend(arms);
                }
                for keyword in ["oneOf", "anyOf"] {
                    if let Some(arms) = schema[keyword].as_array() {
                        let mut matched = false;
                        for arm in arms {
                            let validator =
                                self.branches.entry(arm as *const Value).or_insert_with(|| {
                                    let mut standalone = arm.clone();
                                    standalone["$defs"] = self.root["$defs"].clone();
                                    jsonschema::validator_for(&standalone)
                                        .expect("分支 schema 有效")
                                });
                            if validator.is_valid(value) {
                                // 当前投影变体应能唯一识别；禁止借多个宽松分支的键集并集过门。
                                if matched {
                                    return Err(format!("{path}: {keyword} 匹配多个分支"));
                                }
                                schemas.push(arm);
                                matched = true;
                            }
                        }
                        if !matched {
                            return Err(format!("{path}: {keyword} 没有匹配分支"));
                        }
                    }
                }
                if let Some(properties) = schema["properties"].as_object() {
                    fixed_object = true;
                    for (key, child) in properties {
                        props.entry(key).or_default().push(child);
                    }
                }
                if let Some(map) = schema.get("additionalProperties") {
                    if map == &Value::Bool(false) {
                        fixed_object = true;
                    } else {
                        assert!(map.is_object(), "新增 schema 形态需补充键集遍历");
                        maps.push(map);
                    }
                }
                if let Some(item) = schema.get("items") {
                    items.push(item);
                }
                if let Some(prefix) = schema["prefixItems"].as_array() {
                    prefixes.push(prefix);
                }
            }
            if let Some(object) = value.as_object() {
                for (key, child) in object {
                    let child_path = format!("{path}/{key}");
                    if let Some(child_schemas) = props.get(key.as_str()) {
                        pending.push((child, child_schemas.clone(), child_path));
                    } else if !maps.is_empty() {
                        pending.push((child, maps.clone(), child_path));
                    } else if fixed_object {
                        return Err(format!("{child_path}: schema 未声明的键"));
                    }
                }
            } else if let Some(array) = value.as_array() {
                for (index, child) in array.iter().enumerate() {
                    let mut child_schemas = items.clone();
                    child_schemas.extend(prefixes.iter().filter_map(|p| p.get(index)));
                    if !child_schemas.is_empty() {
                        pending.push((child, child_schemas, format!("{path}/{index}")));
                    }
                }
            }
        }
        Ok(())
    }
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

/// 门 1：两种 display 模式均对全语料执行相同的严格检查。
fn check_corpus(display: bool) {
    let schema = document_schema();
    let validator = jsonschema::validator_for(&schema).expect("schema 有效");
    let mut keys = KeyChecker::new(&schema);
    let mut docs = 0;
    let mut rejected = BTreeSet::new();
    let mut max_depth = 0;
    let mut deepest = String::new();
    let paths = corpus();
    assert_eq!(paths.len(), 1103, "语料总数变化");
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let must_reject =
            path.starts_with(common::corpus_dir("hostile")) && UNOPENABLE.contains(&name);
        let opened = Package::open(&bytes);
        if must_reject {
            assert!(opened.is_err(), "{}: 必须在 Package::open 阶段拒绝", path.display());
            assert!(rejected.insert(name.to_owned()), "重复失败语料：{name}");
            continue;
        }
        let mut pkg = opened.unwrap_or_else(|e| panic!("{}: 意外打开失败: {e}", path.display()));
        let doc = rsword::model::Document::rebuild(&mut pkg)
            .unwrap_or_else(|e| panic!("{}: 意外重建失败: {e}", path.display()));
        let opts = DocumentOpts { display };
        let json = document_json(&pkg, &doc, opts).0;
        let first = json.to_string();
        let depth = json_depth(&first);
        assert!(
            depth <= MAX_JSON_DEPTH,
            "{}: JSON 深度 {depth} 超过 {MAX_JSON_DEPTH}",
            path.display()
        );
        if depth > max_depth {
            max_depth = depth;
            deepest = path.display().to_string();
        }
        if ["xml-deep-table.docx", "hf-deep-txbx.docx"].contains(&name) {
            eprintln!("bind_02 display={display}: {name} JSON depth {depth}");
        }
        assert_eq!(
            first,
            document_json(&pkg, &doc, opts).to_string(),
            "{}: 投影不确定",
            path.display()
        );
        let mut rebuilt_pkg = Package::open(&bytes).unwrap();
        let rebuilt_doc = rsword::model::Document::rebuild(&mut rebuilt_pkg).unwrap();
        assert_eq!(
            first,
            document_json(&rebuilt_pkg, &rebuilt_doc, opts).to_string(),
            "{}: 重建投影不稳定",
            path.display()
        );

        // 仅检查输出语法，不把 Value 的自往返当作投影性质；深度预算已先检查。
        let mut de = serde_json::Deserializer::from_str(&first);
        de.disable_recursion_limit();
        let _parsed = Value::deserialize(&mut de)
            .unwrap_or_else(|e| panic!("{}: 非法 JSON: {e}", path.display()));
        de.end().expect("JSON 尾部无多余内容");
        if let Err(e) = validator.validate(&json) {
            panic!("{}: schema: {e} @ {}", path.display(), e.instance_path());
        }
        keys.check(&json).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        docs += 1;
    }
    assert_eq!(docs, 1099, "成功投影语料数变化");
    assert_eq!(rejected, UNOPENABLE.into_iter().map(String::from).collect());
    eprintln!(
        "bind_02 display={display}: {docs} docs, {} named open failures, max JSON depth {max_depth} ({deepest})",
        rejected.len()
    );
}

#[test]
fn bind_02_corpus_schema_and_stability() {
    check_corpus(false);
}

#[test]
fn bind_02_corpus_display_schema_and_stability() {
    check_corpus(true);
}

#[test]
fn bind_02_key_checker_rejects_extra_keys() {
    // 小型 schema 同时覆盖引用、flatten、动态 map、元组、可空值和显示模型分支。
    let schema = serde_json::json!({
        "$ref": "#/$defs/Root",
        "$defs": {
            "Root": {"properties": {
                "blocks": {"type": "array", "items": {"oneOf": [
                    {"allOf": [{"$ref": "#/$defs/Payload"}, {"properties": {"kind": {"const": "text"}}, "required": ["kind"]}]},
                    {"properties": {"kind": {"const": "empty"}}, "required": ["kind"]}
                ]}},
                "parts": {"additionalProperties": {"$ref": "#/$defs/Payload"}},
                "pair": {"prefixItems": [{"$ref": "#/$defs/Payload"}]},
                "display": {"anyOf": [{"type": "null"}, {"$ref": "#/$defs/Payload"},
                    {"type": "object", "properties": {"raw": {"type": "string"}}, "required": ["raw"]}]}
            }},
            "Payload": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}
        }
    });
    let valid = serde_json::json!({"blocks": [{"kind": "text", "text": "ok"}], "parts": {"42": {"text": "ok"}}, "pair": [{"text": "ok"}], "display": {"text": "ok"}});
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut checker = KeyChecker::new(&schema);
    checker.check(&valid).unwrap();
    for display in [Value::Null, serde_json::json!({"raw": "unparsed"})] {
        let mut alternative = valid.clone();
        alternative["display"] = display;
        checker.check(&alternative).unwrap();
    }
    for pointer in ["", "/blocks/0", "/parts/42", "/pair/0", "/display"] {
        let mut bad = valid.clone();
        bad.pointer_mut(pointer).unwrap()["unexpected"] = Value::Bool(true);
        assert!(validator.is_valid(&bad), "普通 schema 无法抓住多余键");
        let error = checker.check(&bad).expect_err("多余键必须失败");
        assert!(error.contains("unexpected"), "{error}");
    }
    let mut sibling_key = valid.clone();
    sibling_key["blocks"][0] = serde_json::json!({"kind": "empty", "text": "wrong arm"});
    assert!(checker.check(&sibling_key).is_err(), "不能合并未匹配变体的键");
    let mut ambiguous = valid.clone();
    ambiguous["display"]["raw"] = Value::from("混入另一分支");
    assert!(validator.is_valid(&ambiguous));
    assert!(checker.check(&ambiguous).is_err(), "不能合并多个匹配分支的键");
}

#[test]
fn bind_02_json_depth_ignores_escaped_strings() {
    assert_eq!(json_depth(r#"{"text":"[{}]\\\"[", "nested":[{}]}"#), 3);
    assert_eq!(
        json_depth(&format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        )),
        MAX_JSON_DEPTH + 1
    );
}

/// 门 6（体积）：带图语料 `display` 关闭时的聚合体积较 `compat_ts::parsed_doc` 降 ≥ 50%。
#[test]
#[cfg(feature = "compat-ts")]
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
        let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
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

#[test]
fn bind_03_patch_keys_are_declared_and_checked() {
    use rsword::bind::native::{SchemaDefs, ToJson};
    use rsword::semantic::props::{RunProps, RunPropsPatch, TableChange};
    let mut defs = SchemaDefs::default();
    let mut schema = <TableChange<RunProps, RunPropsPatch>>::schema(&mut defs);
    schema["$defs"] = Value::Object(defs.into_map());
    let mut checker = KeyChecker::new(&schema);
    checker.check(&serde_json::json!({"$patch": {}})).unwrap();
    checker.check(&serde_json::json!({"$patch": {"bold": false}})).unwrap();
    let error = checker.check(&serde_json::json!({"$patch": {"zzSabotage": true}})).unwrap_err();
    assert!(error.contains("zzSabotage: schema 未声明的键"), "{error}");
}
