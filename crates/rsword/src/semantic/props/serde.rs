//! `BIND-03` 属性变更线型：缺席 / null / 值 / {"$patch": …}。
//! `$patch` 的规范登记见 `docs/04` §8；不能使用把空 Patch 当 Keep 的 is_keep。

use ::serde::de::DeserializeOwned;
use ::serde::ser::SerializeMap;
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{Change, TableChange, Val};

impl<T> Change<T> {
    /// serde 只省略真正的 Keep。
    pub fn wire_is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl<T, P> TableChange<T, P> {
    /// diff 在相等时先返回 Keep；空 Patch 是防御性编码，不能被省略成另一分支。
    pub fn wire_is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl<T: Serialize> Serialize for Change<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Keep => {
                Err(::serde::ser::Error::custom("Keep must be omitted by its containing field"))
            }
            Self::Unset => s.serialize_none(),
            Self::Set(v) => v.serialize(s),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Change<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            None => Self::Unset,
            Some(v) => Self::Set(v),
        })
    }
}

impl<T: Serialize, P: Serialize> Serialize for TableChange<T, P> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Keep => {
                Err(::serde::ser::Error::custom("Keep must be omitted by its containing field"))
            }
            Self::Unset => s.serialize_none(),
            Self::Set(v) => v.serialize(s),
            Self::Patch(p) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("$patch", p)?;
                m.end()
            }
        }
    }
}

impl<'de, T: DeserializeOwned, P: DeserializeOwned> Deserialize<'de> for TableChange<T, P> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut v = Value::deserialize(d)?;
        if v.is_null() {
            return Ok(Self::Unset);
        }
        if let Some(m) = v.as_object_mut()
            && m.contains_key("$patch")
        {
            if m.len() != 1 {
                return Err(::serde::de::Error::custom("$patch cannot have sibling keys"));
            }
            return serde_json::from_value(m.remove("$patch").unwrap())
                .map(Self::Patch)
                .map_err(::serde::de::Error::custom);
        }
        serde_json::from_value(v).map(Self::Set).map_err(::serde::de::Error::custom)
    }
}

impl<T: Serialize> Serialize for Val<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(v) => v.serialize(s),
            Self::Raw(raw) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("raw", raw)?;
                m.end()
            }
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Val<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if let Some(m) = v.as_object()
            && m.len() == 1
            && let Some(raw) = m.get("raw")
        {
            return raw
                .as_str()
                .map(|s| Self::Raw(s.to_owned()))
                .ok_or_else(|| ::serde::de::Error::custom("raw must be a string"));
        }
        serde_json::from_value(v).map(Self::Value).map_err(::serde::de::Error::custom)
    }
}

impl Serialize for super::HexColorOrAuto {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_xml())
    }
}
impl<'de> Deserialize<'de> for super::HexColorOrAuto {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).ok_or_else(|| ::serde::de::Error::custom("invalid hex color"))
    }
}

use crate::bind::native::json::{ProjCx, SchemaDefs, ToJson};
use crate::bind::native::schema;

impl<T: ToJson + Serialize> ToJson for Change<T> {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        serde_json::to_value(self).expect("non-Keep field")
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        schema::one_of_schema(vec![schema::null_schema(), T::schema(defs)])
    }
}
impl<T: ToJson + Serialize, P: ToJson + Serialize> ToJson for TableChange<T, P> {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        serde_json::to_value(self).expect("non-Keep field")
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        let set =
            serde_json::json!({"allOf": [T::schema(defs), {"not": {"required": ["$patch"]}}]});
        let patch = serde_json::json!({"type": "object", "properties": {"$patch": P::schema(defs)}, "required": ["$patch"], "additionalProperties": false});
        schema::one_of_schema(vec![schema::null_schema(), set, patch])
    }
}

/// 字段缺席由 default 产生 None；显式 null 必须保留为 Some(None)。
pub(crate) fn double_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}
