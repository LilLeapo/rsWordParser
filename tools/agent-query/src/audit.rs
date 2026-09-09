//! AGENT-07/09：正向线型的无损媒体外置；还原全部验证成功后才允许重放。
use crate::{Result, cursor::hash, error};
use rsword::bind::native::EditOpJson;
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub operation: usize,
    pub path: String,
    pub sha256: String,
    pub length: usize,
    pub mime: String,
}
#[derive(Debug, Clone)]
pub struct Attachment {
    pub bytes: Vec<u8>,
    pub mime: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Audit {
    pub operations: Vec<Value>,
    pub attachments: Vec<Binding>,
    pub execution_hash: String,
}
pub fn canonical(ops: &[EditOpJson]) -> Vec<u8> {
    // Value 的 BTreeMap 明确固定键序；不能直接依赖结构体字段声明顺序。
    serde_json::to_vec(&serde_json::to_value(ops).unwrap()).unwrap()
}
impl Audit {
    pub fn capture(ops: &[EditOpJson]) -> Self {
        let mut operations = serde_json::to_value(ops).unwrap().as_array().unwrap().clone();
        let mut attachments = vec![];
        for (operation, value) in operations.iter_mut().enumerate() {
            if value["op"] == "replaceImageMedia" {
                let bytes: Vec<u8> = serde_json::from_value(value["bytes"].clone()).unwrap();
                let sha256 = hash(&bytes);
                attachments.push(Binding {
                    operation,
                    path: "/bytes".into(),
                    sha256: sha256.clone(),
                    length: bytes.len(),
                    mime: value["mime"].as_str().unwrap().into(),
                });
                value["bytes"] = json!({"$attachment":sha256});
            }
        }
        Self { operations, attachments, execution_hash: hash(&canonical(ops)) }
    }
    pub fn restore(&self, media: &BTreeMap<String, Attachment>) -> Result<Vec<EditOpJson>> {
        let bad = || error("AGENT_ATTACHMENT_MISMATCH", "附件、摘要或执行序列不匹配");
        let mut operations = self.operations.clone();
        let mut seen = BTreeSet::new();
        for binding in &self.attachments {
            let Some(op) = operations.get_mut(binding.operation) else { return Err(bad()) };
            if !seen.insert(binding.operation)
                || binding.path != "/bytes"
                || op["op"] != "replaceImageMedia"
                || op["mime"] != binding.mime
                || op["bytes"] != json!({"$attachment":binding.sha256})
            {
                return Err(bad());
            }
            let source = media.get(&binding.sha256).ok_or_else(|| {
                error("AGENT_ATTACHMENT_MISSING", format!("缺少附件 {}", binding.sha256))
            })?;
            if source.bytes.len() != binding.length
                || hash(&source.bytes) != binding.sha256
                || source.mime != binding.mime
            {
                let mut e = bad();
                e.details = json!({"stage":"attachment","operation":binding.operation,"sha256Prefix":binding.sha256.chars().take(12).collect::<String>()});
                return Err(e);
            }
            op["bytes"] = json!(source.bytes);
        }
        if operations
            .iter()
            .enumerate()
            .any(|(i, o)| o["op"] == "replaceImageMedia" && !seen.contains(&i))
        {
            return Err(bad());
        }
        let ops: Vec<EditOpJson> = serde_json::from_value(json!(operations)).map_err(|_| bad())?;
        if hash(&canonical(&ops)) != self.execution_hash {
            return Err(bad());
        }
        Ok(ops)
    }
}
// 输入先保留重复键信息再转 Value；Value::deserialize 会静默接受最后一个值。
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: de::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("无重复键的 JSON")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Unique, M::Error> {
                let mut out = serde_json::Map::new();
                while let Some((key, Unique(value))) = map.next_entry::<String, Unique>()? {
                    if out.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!("重复键 {key}")));
                    }
                }
                Ok(Unique(Value::Object(out)))
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Unique, S::Error> {
                let mut out = vec![];
                while let Some(Unique(v)) = seq.next_element()? {
                    out.push(v)
                }
                Ok(Unique(Value::Array(out)))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
        }
        de.deserialize_any(V)
    }
}
pub fn parse(input: &str) -> Result<Value> {
    serde_json::from_str::<Unique>(input)
        .map(|v| v.0)
        .map_err(|e| error("BIND_BAD_ARGUMENT", e.to_string()))
}
