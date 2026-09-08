//! 声明模型的 JSON 投影（`BIND-02`，`MOD-10`）：styles / numbering / settings / fontTable
//! 的类型本体由属性表生成，投影由 `build/props.rs` 从同一份 TOML 生成（`super` 的
//! `props_gen`）；这里只补手写的 `CompatFacts`。`OwnHeadingLevel` 是 `RES-02` 的内部
//! 辅助，不进模型 JSON。

use crate::model::decl::CompatFacts;
use crate::semantic::props::CompatSetting;
use crate::xml::QName;

use super::model_json;

model_json! {
    /// 兼容事实（`MOD-10`，`docs/03` §6.5）：只记录，`resolve` 与布局层解释。
    /// `flags` 里 `Other` / `Unbound` 的 `QName` 投 `"?"`（`bind::native::json` 模块头约定）。
    struct CompatFacts(cx) test json_fields_cover_compat_facts {
        /// `compatSetting[name=compatibilityMode]/@val`。
        opt mode => "mode", u32 = mode;
        settings => "settings", Vec<CompatSetting> = settings;
        /// `w:compat` 下值为真的布尔子元素。
        flags => "flags", Vec<QName> = flags;
    }
}
