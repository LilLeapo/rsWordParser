//! 绑定层：对外接口与适配器。`compat_ts`（`spec/10-compat-ts.md`）在 M1 建立、M9 删除；
//! napi / wasm 绑定在 M8 接入编辑器时加入。

pub mod compat_ts;
pub mod js;
