//! 原生协议（`spec/21-bind.md`，前缀 `BIND`）：rsword 独立交付的**唯一对外协议**
//! `open → document / resolve / media → apply / save → close`。
//!
//! | 模块 | 内容 |
//! | --- | --- |
//! | [`json`] | `BIND-02` 模型 JSON 投影（`model_json!` 同表展开实现 / schema / 覆盖测试） |
//! | [`schema`] | 由同一张表生成的 JSON Schema 片段构造器（门 1：全语料过校验） |
//!
//! 会话、媒体句柄与导出（`BIND-01/05/06/09`）在任务 8.4 落地；`EditOp` JSON（`BIND-03`）在 8.3。

pub mod edit;
pub mod json;
pub mod schema;

pub use edit::{
    EditJsonError, EditOpJson, apply_edit_json, edit_op_from_json, edit_op_to_json,
    xml_escape_count,
};
pub use json::{DocumentJson, DocumentOpts, ProjCx, ToJson, document_json};
pub use schema::{SchemaDefs, document_schema};
