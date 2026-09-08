//! `BIND-02` 的 JSON Schema（`spec/21-bind.md`，任务 8.2）：由 `model_json!` 表与
//! `build/props.rs` 同表生成，本文件只有片段构造器；整份 schema 的装配（`document_schema()`）
//! 随 `Document` 表一起落地。门 1：全语料 `document()` 输出通过该校验。
//!
//! 说明：
//! - 命名类型进 `$defs`、引用处返回 `$ref`（`Block` / `Inline` 有递归，内联不可能）。
//! - 不用 `additionalProperties: false`：带载荷枚举的 `{"kind": …, …}` 是 `allOf` 拼出来的，
//!   `false` 会把拼进分支的 `kind` 键判掉；键集正确性由 `model_json!` 的覆盖测试保证。
//! - 元组 / 区间用 `prefixItems`（draft 2020-12）。

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// `$defs` 收集器：同名只注册一次（先占位打断递归，再回填）。
#[derive(Debug, Default)]
pub struct SchemaDefs {
    map: BTreeMap<&'static str, Value>,
}

impl SchemaDefs {
    /// 注册并返回 `{"$ref": "#/$defs/<name>"}`；递归引用在占位期命中已有条目，直接返回 `$ref`。
    pub fn define(
        &mut self,
        name: &'static str,
        build: impl FnOnce(&mut SchemaDefs) -> Value,
    ) -> Value {
        if !self.map.contains_key(name) {
            self.map.insert(name, Value::Null);
            let v = build(self);
            self.map.insert(name, v);
        }
        let mut o = Map::new();
        o.insert("$ref".to_string(), Value::from(format!("#/$defs/{name}")));
        Value::Object(o)
    }

    /// 已注册的片段（覆盖测试用）。
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.map.get(name)
    }

    /// 全部注册（`document_schema()` 装配用）。
    pub fn into_map(self) -> Map<String, Value> {
        self.map.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }
}

fn typed(t: &str) -> Value {
    let mut o = Map::new();
    o.insert("type".to_string(), Value::from(t));
    Value::Object(o)
}

/// `{"type":"integer"}`。
pub fn int_schema() -> Value {
    typed("integer")
}

/// `{"type":"number"}`。
pub fn num_schema() -> Value {
    typed("number")
}

/// `{"type":"boolean"}`。
pub fn bool_schema() -> Value {
    typed("boolean")
}

/// `{"type":"string"}`。
pub fn str_schema() -> Value {
    typed("string")
}

/// `{"type":"null"}`。
pub fn null_schema() -> Value {
    typed("null")
}

/// `Val::Raw` 的形态：`{"raw": "原文"}`。
pub fn raw_schema() -> Value {
    let mut p = Map::new();
    p.insert("raw".to_string(), str_schema());
    obj_schema(p, vec!["raw"])
}

/// 对象：`{"type":"object","properties":…,"required":[…]}`（`required` 为空则省略）。
pub fn obj_schema(props: Map<String, Value>, required: Vec<&str>) -> Value {
    let mut o = Map::new();
    o.insert("type".to_string(), Value::from("object"));
    o.insert("properties".to_string(), Value::Object(props));
    if !required.is_empty() {
        o.insert(
            "required".to_string(),
            Value::Array(required.into_iter().map(Value::from).collect()),
        );
    }
    Value::Object(o)
}

/// 数组：`{"type":"array","items":…}`。
pub fn arr_schema(items: Value) -> Value {
    let mut o = Map::new();
    o.insert("type".to_string(), Value::from("array"));
    o.insert("items".to_string(), items);
    Value::Object(o)
}

/// 以 id 字串为键的对象：`{"type":"object","additionalProperties":…}`。
pub fn map_schema(value: Value) -> Value {
    let mut o = Map::new();
    o.insert("type".to_string(), Value::from("object"));
    o.insert("additionalProperties".to_string(), value);
    Value::Object(o)
}

/// 二元数组（区间 / 元组）：`{"type":"array","prefixItems":[a,b],"minItems":2,"maxItems":2}`。
pub fn pair_schema(a: Value, b: Value) -> Value {
    let mut o = Map::new();
    o.insert("type".to_string(), Value::from("array"));
    o.insert("prefixItems".to_string(), Value::Array(vec![a, b]));
    o.insert("minItems".to_string(), Value::from(2));
    o.insert("maxItems".to_string(), Value::from(2));
    Value::Object(o)
}

/// `{"anyOf": […]}`。
pub fn any_of_schema(arms: Vec<Value>) -> Value {
    let mut o = Map::new();
    o.insert("anyOf".to_string(), Value::Array(arms));
    Value::Object(o)
}

/// `{"oneOf": […]}`。
pub fn one_of_schema(arms: Vec<Value>) -> Value {
    let mut o = Map::new();
    o.insert("oneOf".to_string(), Value::Array(arms));
    Value::Object(o)
}

/// 字串枚举：`{"enum": […]}`。
pub fn enum_str_schema(values: &[&str]) -> Value {
    let mut o = Map::new();
    o.insert("enum".to_string(), Value::Array(values.iter().map(|v| Value::from(*v)).collect()));
    Value::Object(o)
}

/// 枚举变体（内标签 `kind`）的 schema：`{"kind": <const>, …props}`，`kind` 与 `required` 恒必填。
pub fn variant_schema(kind: &str, mut props: Map<String, Value>, required: Vec<&str>) -> Value {
    let mut k = Map::new();
    k.insert("const".to_string(), Value::from(kind));
    props.insert("kind".to_string(), Value::Object(k));
    let mut req = vec!["kind"];
    req.extend(required);
    obj_schema(props, req)
}

/// flatten 变体（`Block::Text(TextBlock)` 这类：载荷对象的字段平铺进变体对象）的 schema：
/// `allOf[载荷 schema, {"kind": <const>}]`。
pub fn flatten_variant_schema(kind: &str, inner: Value) -> Value {
    let mut o = Map::new();
    o.insert(
        "allOf".to_string(),
        Value::Array(vec![inner, variant_schema(kind, Map::new(), Vec::new())]),
    );
    Value::Object(o)
}

/// flatten 变体的投影：载荷对象平铺 + `kind` 键。
pub fn flatten_variant_json(v: Value, kind: &str) -> Value {
    let Value::Object(mut o) = v else {
        panic!("model_json!: flatten 变体的载荷必须投影成对象（kind = {kind}）")
    };
    assert!(
        !o.contains_key("kind"),
        "model_json!: flatten 变体的载荷已有 kind 键（{kind}），载荷字段要用 `~` 改名行"
    );
    o.insert("kind".to_string(), Value::from(kind));
    Value::Object(o)
}

/// `document()` 输出的整份 JSON Schema（`BIND-02` 顶层形态为根，命名类型全进 `$defs`）。
pub fn document_schema() -> Value {
    let mut defs = SchemaDefs::default();
    let mut root = <crate::model::Document as crate::bind::native::json::ToJson>::schema(&mut defs);
    let mut o = Map::new();
    o.insert("$schema".to_string(), Value::from("https://json-schema.org/draft/2020-12/schema"));
    let mut defs = defs.into_map();
    // BIND-10：顶层可裁剪，跨块索引仍必填；元数据不属于模型字段。
    let mut document = defs["Document"].clone();
    document["required"] = serde_json::json!(["spans", "fields", "revisions"]);
    document["properties"]["totalBlocks"] = int_schema();
    document["properties"]["truncated"] = bool_schema();
    defs.insert("DocumentResponse".into(), document);
    root["$ref"] = Value::from("#/$defs/DocumentResponse");
    o.insert("$defs".to_string(), Value::Object(defs));

    if let Value::Object(r) = root {
        o.extend(r);
    }
    Value::Object(o)
}
