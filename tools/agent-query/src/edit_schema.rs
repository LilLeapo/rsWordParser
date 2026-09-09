//! AGENT-07：操作表的字段类型生成输入 schema；属性对象复用原生属性 schema。
use crate::edit::Selector;
use rsword::{
    agent::anchors::{Anchor, ObjectRef},
    bind::native::{SchemaDefs, ToJson},
    model::{HfKind, HfVariant},
    save::options::decl::StyleUpsertSave,
};
use serde_json::{Value, json};
pub(crate) trait WireSchema {
    fn schema(defs: &mut SchemaDefs) -> Value;
    fn optional() -> bool {
        false
    }
}
pub(crate) fn camel(s: &str) -> String {
    let mut upper = false;
    s.chars()
        .filter_map(|c| {
            if c == '_' {
                upper = true;
                None
            } else {
                let x = if upper { c.to_ascii_uppercase() } else { c };
                upper = false;
                Some(x)
            }
        })
        .collect()
}
impl WireSchema for String {
    fn schema(_: &mut SchemaDefs) -> Value {
        json!({"type":"string"})
    }
}
impl WireSchema for u32 {
    fn schema(_: &mut SchemaDefs) -> Value {
        json!({"type":"integer","minimum":0,"maximum":u32::MAX})
    }
}
impl<T: WireSchema> WireSchema for Option<T> {
    fn schema(d: &mut SchemaDefs) -> Value {
        json!({"oneOf":[{"type":"null"},T::schema(d)]})
    }
    fn optional() -> bool {
        true
    }
}
impl<T: WireSchema> WireSchema for Vec<T> {
    fn schema(d: &mut SchemaDefs) -> Value {
        json!({"type":"array","items":T::schema(d)})
    }
}
impl<T: WireSchema> WireSchema for Box<T> {
    fn schema(d: &mut SchemaDefs) -> Value {
        T::schema(d)
    }
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
impl WireSchema for ObjectRef {
    fn schema(d: &mut SchemaDefs) -> Value {
        d.define("AgentObject",|_|object(json!({"part":{"type":"integer","minimum":0,"maximum":u32::MAX},"node":{"type":"integer","minimum":0,"maximum":u32::MAX},"flow":{"type":"integer","minimum":0,"maximum":u32::MAX},"kind":{"type":"string"}}),&["part","node","flow","kind"]))
    }
}
impl WireSchema for Anchor {
    fn schema(d: &mut SchemaDefs) -> Value {
        d.define("AgentAnchor",|d| {
 let common=json!({"snapshot":{"oneOf":[{"type":"string"},{"type":"object"}]},"projectionKey":{"type":"string"},"segmentKey":<u32 as WireSchema>::schema(d),"offsetInSegment":<u32 as WireSchema>::schema(d),"affinity":{"enum":["left","right"]}});
 let mut source=common.clone();source["kind"]=json!({"const":"source"});source["part"]=<u32 as WireSchema>::schema(d);source["flow"]=<u32 as WireSchema>::schema(d);source["node"]=<u32 as WireSchema>::schema(d);
 source["inlinePos"]=object(json!({"part":<Option<u32> as WireSchema>::schema(d),"para":<u32 as WireSchema>::schema(d),"offset":<u32 as WireSchema>::schema(d)}),&["para","offset"]);
 let mut presentation=common;presentation["kind"]=json!({"const":"presentation"});presentation["owner"]=<ObjectRef as WireSchema>::schema(d);presentation["reason"]=json!({"type":"string"});
 json!({"oneOf":[object(source,&["snapshot","projectionKey","segmentKey","offsetInSegment","affinity","kind","part","flow","node","inlinePos"]),object(presentation,&["snapshot","projectionKey","segmentKey","offsetInSegment","affinity","kind","owner","reason"])]})
})
    }
}
impl WireSchema for Selector {
    fn schema(d: &mut SchemaDefs) -> Value {
        d.define("AgentSelector",|d|object(json!({
 "scope":<Vec<ObjectRef> as WireSchema>::schema(d),"find":<Option<String> as WireSchema>::schema(d),"start":<Option<Anchor> as WireSchema>::schema(d),"end":<Option<Anchor> as WireSchema>::schema(d),"original":<Option<String> as WireSchema>::schema(d),"occurrence":<Option<u32> as WireSchema>::schema(d),"all":{"type":"boolean"},"search":object(json!({"mode":{"enum":["literal","regex"]},"insensitive":{"type":"boolean"},"foldWidth":{"type":"boolean"},"collapseWhitespace":{"type":"boolean"},"deadlineMs":{"type":"integer","minimum":1,"maximum":2000}}),&[])
}),&["scope"]))
    }
}
macro_rules! enums {($($ty:ty => [$($value:literal),*];)*)=>{$(impl WireSchema for $ty {fn schema(_: &mut SchemaDefs)->Value{json!({"enum":[$($value),*]})}})*}}
enums! {HfKind=>["header","footer"]; HfVariant=>["default","first","even"];}
impl WireSchema for StyleUpsertSave {
    fn schema(d: &mut SchemaDefs) -> Value {
        object(
            json!({"styleId":{"type":"string"},"kind":{"type":"string"},"name":{"type":"string"},"basedOn":<Option<String> as WireSchema>::schema(d),"runProps":<Option<rsword::semantic::props::RunProps> as ToJson>::schema(d),"paraProps":<Option<rsword::semantic::props::ParaProps> as ToJson>::schema(d)}),
            &["styleId", "kind", "name"],
        )
    }
}
