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

/// 「模型结构体 → TS JSON 对象」的字段表（`spec/17` 的 `display_json!`）：每行一个键，展开成一个
/// `Map<String, Value>`。三种行：
///
/// - `"key" => expr` 恒写；
/// - `opt "key" => expr` 的 `expr` 是 `Option`，有值才写（[`set_some!`]）；
/// - `flag "key" => expr` 的 `expr` 是 `bool`，为真才写 `true`（[`set_if!`]）。
///
/// TS 的显示结构体（`ChartDisplay` / `ChartSeries` / `DiagramDisplay` / `FormulaDisplay` …）全是
/// 「几个必填 + 一堆 `?:` 可选」，用这张表读起来就是 `types.ts` 里的接口声明本身。
///
/// ```ignore
/// let o = display_json! {
///     "partPath" => path,
///     "kind" => d.kind.as_str(),
///     flag "horizontal" => d.horizontal,
///     opt "grouping" => d.grouping.map(|g| g.as_str()),
/// };
/// ```
macro_rules! display_json {
    (@field $o:ident, opt $key:literal, $val:expr) => {
        $crate::bind::compat_ts::json::set_some!(&mut $o, $key => $val)
    };
    (@field $o:ident, flag $key:literal, $val:expr) => {
        $crate::bind::compat_ts::json::set_if!(&mut $o, $key => $val)
    };
    (@field $o:ident, $key:literal, $val:expr) => {
        $crate::bind::compat_ts::json::set(&mut $o, $key, $val)
    };
    ($($($tag:ident)? $key:literal => $val:expr),+ $(,)?) => {{
        let mut o = ::serde_json::Map::new();
        $( $crate::bind::compat_ts::json::display_json!(@field o, $($tag)? $key, $val); )+
        o
    }};
}

pub(super) use {display_json, set_if, set_some};

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    #[test]
    fn display_json_three_row_kinds() {
        let none: Option<&str> = None;
        let o = display_json! {
            "always" => 1,
            opt "present" => Some("x"),
            opt "absent" => none,
            flag "on" => true,
            flag "off" => false,
        };
        assert_eq!(Value::Object(o), json!({ "always": 1, "present": "x", "on": true }));
    }
}
