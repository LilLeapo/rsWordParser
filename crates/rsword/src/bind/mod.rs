//! 绑定层：对外接口与适配器。`compat_ts`（`spec/10-compat-ts.md`）是**测试专用**的差分对接点
//! （已挂 `compat-ts` feature，默认不编译）；`native`（`spec/21-bind.md`）是唯一对外协议。
//! napi / wasm 外壳是它们的薄壳（`spec/18` 7.10 的分层）。

#[cfg(feature = "compat-ts")]
#[doc(hidden)]
pub mod compat_ts;
#[cfg(feature = "compat-ts")]
#[doc(hidden)]
pub mod js;
pub mod native;
