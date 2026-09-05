//! `SAVE-07` 的 `word/settings.xml` 保存选项 → `SettingsPatch`（`spec/16` 任务 5.6）。
//!
//! 元素在 `w:settings` 里的位置由 `plan_apply_settings` 按 `PROP-05` 决定。TS 每个 `apply*` 都插在
//! 根标签之后、再靠"倒着调用"凑出 schema 顺序；我们不需要这个技巧。

use crate::semantic::props::{Change, DocProtection, SettingsPatch, Val, WriteProtection};

use super::section::{ProtectionOption, WriteProtectionOption};

/// 三个开关的同一形状：`Some(true)` 确保元素存在、`Some(false)` 删掉、`None` 不动。
///
/// ```ignore
/// let patch = SettingsPatch {
///     even_and_odd_headers: settings_flag!(opts.even_and_odd_headers),
///     ..Default::default()
/// };
/// ```
macro_rules! settings_flag {
    ($v:expr) => {
        match $v {
            None => $crate::semantic::props::Change::Keep,
            Some(true) => $crate::semantic::props::Change::Set(true),
            Some(false) => $crate::semantic::props::Change::Unset,
        }
    };
}

/// Word 2013+ 的口令散列属性组：`w:documentProtection` 与 `w:writeProtection` 上一模一样的七个
/// 属性（TS 的字面值）。`hash` 缺失时一个都不写——只有 `recommended` / `edit` 的保护是无口令的。
///
/// ```ignore
/// let mut p = DocProtection::default();
/// crypt_attrs!(p, opt);   // opt 有 hash 时填七个属性，否则什么都不做
/// ```
macro_rules! crypt_attrs {
    ($target:expr, $opt:expr) => {
        if let Some(hash) = $opt.hash.clone() {
            $target.crypt_provider_type = Some("rsaAES".to_string());
            $target.crypt_algorithm_class = Some("hash".to_string());
            $target.crypt_algorithm_type = Some("typeAny".to_string());
            $target.crypt_algorithm_sid = Some(Val::Value($opt.algorithm_sid.unwrap_or(14)));
            $target.crypt_spin_count = Some(Val::Value($opt.spin_count.unwrap_or(100_000)));
            $target.hash = Some(hash);
            $target.salt = $opt.salt.clone();
        }
    };
}

/// `w:documentProtection`：`None` 删掉。
pub fn protection_patch(p: Option<&ProtectionOption>) -> SettingsPatch {
    let document_protection = match p {
        None => Change::Unset,
        Some(p) => {
            let mut out = DocProtection {
                edit: Some(Val::Value(p.edit)),
                enforcement: p.enforced.then_some(true),
                ..Default::default()
            };
            crypt_attrs!(out, p);
            Change::Set(out)
        }
    };
    SettingsPatch { document_protection, ..Default::default() }
}

/// `w:writeProtection`：`None`，或既不 `recommended` 也没有口令 → 删掉（同 TS）。
pub fn write_protection_patch(p: Option<&WriteProtectionOption>) -> SettingsPatch {
    let write_protection = match p {
        Some(p) if p.recommended || p.hash.is_some() => {
            let mut out = WriteProtection {
                recommended: p.recommended.then_some(true),
                ..Default::default()
            };
            crypt_attrs!(out, p);
            Change::Set(out)
        }
        _ => Change::Unset,
    };
    SettingsPatch { write_protection, ..Default::default() }
}

/// 两个布尔开关：奇偶页眉，以及页面底色要求的 `w:displayBackgroundShape`
/// （Word 只在这个开关打开时才画 `w:background`）。
pub fn flags_patch(even_and_odd_headers: Option<bool>, background: Option<bool>) -> SettingsPatch {
    SettingsPatch {
        even_and_odd_headers: settings_flag!(even_and_odd_headers),
        display_background_shape: settings_flag!(background),
        ..Default::default()
    }
}
