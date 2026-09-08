//! 绑定层：对外接口与适配器。`compat_ts`（`spec/10-compat-ts.md`）是**测试专用**的差分对接点
//! （M8′ 起挂 `compat-ts` feature，8.7 落地）；`native`（`spec/21-bind.md`）是唯一对外协议。
//! napi / wasm 外壳是它们的薄壳（`spec/18` 7.10 的分层）。

#[doc(hidden)]
pub mod compat_ts;
#[doc(hidden)]
pub mod js;
pub mod native;
