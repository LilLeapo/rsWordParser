//! `ParsedDoc` JSON 字段的小工具（`COMPAT-02`）。
//!
//! 投影层几乎每个函数都在做同一件事：算出一个可选值，有值就写进对象。散着写就是几十行
//! `if let Some(x) = … { set(&mut o, "k", x) }`；宏把它压成一张表，读起来就是「有哪些键、
//! 各自从哪来」，而不是一堆控制流。

use serde_json::{Map, Value};

/// 写一个字段。
pub(super) fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

/// 有值才写。
///
/// ```ignore
/// set_some!(o,
///     "widthPx" => ext.map(|e| px(e.cx)),
///     "rotDeg" => s.rot_60k.filter(|&r| r != 0),
/// );
/// ```
macro_rules! set_some {
    ($o:expr, $($key:literal => $val:expr),+ $(,)?) => {$(
        if let ::core::option::Option::Some(v) = $val {
            $crate::bind::compat_ts::json::set($o, $key, v);
        }
    )+};
}

/// 条件为真才写，写进去的值恒为 `true`——投影里的布尔字段一律「要么是 true，要么不出现」。
///
/// ```ignore
/// set_if!(o, "floating" => grouped, "behind" => a.behind_doc);
/// ```
macro_rules! set_if {
    ($o:expr, $($key:literal => $cond:expr),+ $(,)?) => {$(
        if $cond {
            $crate::bind::compat_ts::json::set($o, $key, true);
        }
    )+};
}

pub(super) use {set_if, set_some};
