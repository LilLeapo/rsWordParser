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
//!
//! # 稳定面（BIND-11）
//! 使用 [`bind::native`] 的会话协议，或本页列出的核心类型。隐藏模块保留旧路径供一版
//! 观察期使用；下游仍可调用，因此本版尚未缩小实际公共面。
//! 破坏性变更仅在 crate minor 版本发生，变更记录必须给出迁移路径；协议版本独立演进。

#![warn(missing_docs)]

#[cfg(test)]
extern crate self as rsword;

#[macro_use]
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod model;
#[doc(hidden)]
pub mod agent;
pub mod bind;
// audit 构建取消祖先的隐藏，让稳定定义及固有 impl 上的 deny(missing_docs) 生效。
// 观察项仍不要求文档；注解位置由 BIND-11 成文清单和源码扫描双向锁定。
// mod 声明保留在源码中，让 cargo fmt 能继续发现并检查各层文件。
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod diag;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod edit;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod error;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod package;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod resolve;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod save;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod semantic;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod span;
#[cfg_attr(not(rsword_api_docs), doc(hidden))]
#[cfg_attr(rsword_api_docs, allow(missing_docs))]
pub mod xml;

#[doc(inline)]
pub use diag::DiagCode;
#[doc(hidden)]
pub use diag::{Diagnostic, ValidationOrigin};
#[doc(inline)]
pub use edit::{EditContext, EditOp, EditSession, MutationResult};
#[doc(inline)]
pub use error::Error;
#[doc(hidden)]
pub use error::Result;
#[doc(inline)]
pub use span::field::FieldSpan;
#[doc(inline)]
pub use span::{Anchor, RangeSpan};
#[doc(inline)]
pub use xml::{Dirty, Node};
