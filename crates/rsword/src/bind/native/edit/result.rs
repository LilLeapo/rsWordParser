//! `BIND-03/07` 操作结果投影；复用模型 JSON 表，新增引擎字段必须补投影。
use crate::bind::native::{ProjCx, SchemaDefs, ToJson};
use crate::diag::Diagnostic;
use crate::edit::{MutationResult, Utf16Offset};
use crate::xml::NodeId;
use serde_json::{Value, json};

impl<A: ToJson, B: ToJson, C: ToJson> ToJson for (A, B, C) {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(vec![self.0.to_json(cx), self.1.to_json(cx), self.2.to_json(cx)])
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        json!({"type": "array", "prefixItems": [A::schema(defs), B::schema(defs), C::schema(defs)], "minItems": 3, "maxItems": 3})
    }
}

crate::bind::native::json::model_json! {
    /// `EDIT-05` 操作结果五字段，created 中不创建节点的位置保留 null。
    struct MutationResult(cx) test bind_03_mutation_result_fields {
        created => "created", Vec<Option<NodeId>> = created;
        affected_blocks => "affectedBlocks", Vec<NodeId> = affected_blocks;
        structure_changed => "structureChanged", bool = structure_changed;
        diagnostics => "diagnostics", Vec<Diagnostic> = diagnostics;
        offset_delta => "offsetDelta", Vec<(NodeId, Utf16Offset, i32)> = offset_delta;
    }
}
