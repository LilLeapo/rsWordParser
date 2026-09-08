//! rsword：高保真 DOCX 编辑内核。
//!
//! 文件是真相：未编辑内容零字节改动，编辑只发生在被标脏的 XML 节点上。
//! 分层与六个核心类型见 `docs/03-architecture-v3.md`；每个模块头部标注对应的 `spec/` 文件，
//! 测试函数名引用规范 ID（例如 `xml_12_dirty_propagation`）。
//!
//! | 模块 | 层 | 规范 |
//! | --- | --- | --- |
//! | [`package`] | L0 包层 | `spec/01-package.md` (`PKG-*`) |
//! | [`xml`] | L1 无损 XML | `spec/02-xml-dom.md` (`XML-*`) |
//! | [`span`] | L2 范围 + 字段 | `spec/03-span.md` (`SPAN-*`)、`spec/04-field.md` (`FLD-*`) |
//! | [`semantic`] | L3 属性表、事实、分类 | `spec/05-properties.md` (`PROP-*`)、`spec/06-model.md` (`MOD-*`) |
//! | [`model`] | L3 文档模型（投影） | `spec/06-model.md` (`MOD-*`) |
//! | [`resolve`] | 有效属性只读视图 | `spec/07-resolve.md` (`RES-*`) |
//! | [`edit`] | L4 编辑引擎 | `spec/08-edit.md` (`EDIT-*`) |
//! | [`save`] | 校验、序列化、包写回 | `spec/09-save.md` (`SAVE-*`) |
//! | [`bind`] | 绑定与 `compat_ts` 适配器 | `spec/10-compat-ts.md` (`COMPAT-*`) |
//!
//! 规范状态 = DOM + Span（`xml` + `span`）；`model` 与 `resolve` 是可重建的投影。

#[cfg(test)]
extern crate self as rsword;

pub mod bind;
pub mod diag;
pub mod edit;
pub mod error;
pub mod model;
pub mod package;
pub mod resolve;
pub mod save;
pub mod semantic;
pub mod span;
pub mod xml;

pub use diag::{DiagCode, Diagnostic, ValidationOrigin};
pub use error::{Error, NotOoxml, Result};
