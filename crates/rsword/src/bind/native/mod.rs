//! 原生协议（`spec/21-bind.md`，前缀 `BIND`）：rsword 独立交付的**唯一对外协议**
//! `open → document / resolve / media → apply / save → close`。
//!
//! | 模块 | 内容 |
//! | --- | --- |
//! | [`json`] | `BIND-02` 模型 JSON 投影（`model_json!` 同表展开实现 / schema / 覆盖测试） |
//! | [`schema`] | 由同一张表生成的 JSON Schema 片段构造器（门 1：全语料过校验） |
//!
//! [`SessionTable`] 提供全部会话导出（`BIND-01/05/06/09`），JSON 参数按 `spec/21`；
//! 未提供的选项传 `None`，节点查询缺省主 part。`document()` 单向输出，写入只经 `apply()`。
//! `part_bytes()` / `node_xml()` 是只读调试设施。

pub mod edit;
mod error;
mod exports;
mod query;
mod selection;
mod session;
pub use session::{SessionId, SessionTable};
pub mod json;
pub mod schema;

pub use crate::bind_export;
pub use error::ApiError;

pub use edit::{
    EditJsonError, EditOpJson, apply_edit_json, edit_op_from_json, edit_op_to_json,
    xml_escape_count,
};
pub use json::{DocumentJson, DocumentOpts, ProjCx, ToJson, document_json};
pub use schema::{SchemaDefs, document_schema};
