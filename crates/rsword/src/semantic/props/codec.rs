//! 值编解码（`PROP-02`、`PROP-04`、`PROP-09`）。
//!
//! 每个 codec 是一个实现 [`Codec`] 的类型（内建的是单元结构体，`types.toml` 生成的枚举自身实现）。
//! 解析永不失败：无法理解的文本作为 [`Val::Raw`] 保留并记 `PROP_BAD_VALUE`，写回时原文输出。
//! 度量单位换算：`1in = 72pt`，`1pc = 1pi = 12pt`，`1cm = 72/2.54 pt`，`1mm = 7.2/2.54 pt`，
//! 结果四舍五入到整数。

use std::borrow::Cow;

use crate::package::PartFlavor;
use crate::semantic::props::Val;
use crate::semantic::props::read::{Ctx, lit};

/// 一种属性值的解析与生成。
pub trait Codec {
    /// 解析结果类型（`Val<T>`、`bool`、`String`）。
    type Value: Clone + PartialEq + Eq + std::fmt::Debug;
    /// 诊断里的类型名。
    const NAME: &'static str;

    /// 解析 `w:val`；失败时保留原文并记诊断，不返回错误。
    fn parse(text: &str, ctx: &mut Ctx<'_>) -> Self::Value;

    /// 解析其他属性（诊断里带属性名）。默认与 `parse` 相同。
    fn parse_attr(text: &str, _what: &str, ctx: &mut Ctx<'_>) -> Self::Value {
        Self::parse(text, ctx)
    }

    /// 生成属性值文本，按 flavor（`Strict` 的 `ST_OnOff` 为 `true/false`）。
    fn write(v: &Self::Value, flavor: PartFlavor) -> Cow<'_, str>;

    /// 元素缺 `w:val` 时的值。
    fn missing(ctx: &mut Ctx<'_>) -> Self::Value;

    /// 作为单 `w:val` 元素写出时的属性值；`None` = 不写属性（`OnOff(true)` 的裸元素）。
    fn val_attr(v: &Self::Value, flavor: PartFlavor) -> Option<Cow<'_, str>> {
        Some(Self::write(v, flavor))
    }
}

// ---- 公共辅助（生成代码也用） ------------------------------------------------------------------

/// 记诊断并保留原文。
pub fn raw_val<T>(text: &str, ctx: &mut Ctx<'_>, name: &str) -> Val<T> {
    ctx.bad_value("w:val", name, text);
    Val::Raw(text.to_owned())
}

pub fn raw_attr<T>(text: &str, what: &str, ctx: &mut Ctx<'_>, name: &str) -> Val<T> {
    ctx.bad_value(what, name, text);
    Val::Raw(text.to_owned())
}

/// 元素缺 `w:val`：`Raw("")` + 诊断。
pub fn missing_val<T>(ctx: &mut Ctx<'_>, name: &str) -> Val<T> {
    ctx.missing_value("w:val", name);
    Val::Raw(String::new())
}

/// 枚举字面匹配（生成的枚举 codec 调用）。
pub fn parse_enum<T>(
    text: &str,
    ctx: &mut Ctx<'_>,
    name: &str,
    f: fn(&str) -> Option<T>,
) -> Val<T> {
    match f(text.trim()) {
        Some(v) => Val::Value(v),
        None => raw_val(text, ctx, name),
    }
}

pub fn parse_enum_attr<T>(
    text: &str,
    what: &str,
    ctx: &mut Ctx<'_>,
    name: &str,
    f: fn(&str) -> Option<T>,
) -> Val<T> {
    match f(text.trim()) {
        Some(v) => Val::Value(v),
        None => raw_attr(text, what, ctx, name),
    }
}

// ---- OnOff（PROP-04 三态）------------------------------------------------------------------------

/// `ST_OnOff`。元素缺 `w:val` → `true`；`1/true/on` → `true`；`0/false/off` → `false`；
/// 其他 → `true` + 诊断。生成：Transitional `1/0`，Strict `true/false`；作元素时 `true` 为裸元素。
pub struct OnOff;

impl Codec for OnOff {
    type Value = bool;
    const NAME: &'static str = "ST_OnOff";

    fn parse(text: &str, ctx: &mut Ctx<'_>) -> bool {
        Self::parse_attr(text, "w:val", ctx)
    }

    fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> bool {
        match text.trim() {
            "1" | "true" | "on" => true,
            "0" | "false" | "off" => false,
            other => match other.to_ascii_lowercase().as_str() {
                "true" | "on" => true,
                "false" | "off" => false,
                _ => {
                    ctx.bad_value(what, Self::NAME, text);
                    true
                }
            },
        }
    }

    fn write(v: &bool, flavor: PartFlavor) -> Cow<'_, str> {
        lit(match (v, flavor) {
            (true, PartFlavor::Transitional) => "1",
            (false, PartFlavor::Transitional) => "0",
            (true, PartFlavor::Strict) => "true",
            (false, PartFlavor::Strict) => "false",
        })
    }

    fn missing(_ctx: &mut Ctx<'_>) -> bool {
        true
    }

    fn val_attr(v: &bool, flavor: PartFlavor) -> Option<Cow<'_, str>> {
        if *v { None } else { Some(Self::write(v, flavor)) }
    }
}

// ---- 度量 ----------------------------------------------------------------------------------------

/// 十进制数字面：可选正负号、数字、可选小数部分。不接受指数、`inf`、空串。
fn parse_decimal(s: &str) -> Option<f64> {
    let body = s.strip_prefix('-').or_else(|| s.strip_prefix('+')).unwrap_or(s);
    if body.is_empty() {
        return None;
    }
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<f64>().ok().filter(|f| f.is_finite())
}

/// 单位 → 每单位多少 pt。
fn points_per_unit(unit: &str) -> Option<f64> {
    Some(match unit {
        "pt" => 1.0,
        "in" => 72.0,
        "pc" | "pi" => 12.0,
        "cm" => 72.0 / 2.54,
        "mm" => 7.2 / 2.54,
        _ => return None,
    })
}

/// `ST_UniversalMeasure` 或无单位十进制 → 目标单位整数。`per_point`：目标单位每 pt 的个数
/// （半点 2、twip 20、1/8 pt 8）。无单位时数值直接取整。
pub fn parse_measure(text: &str, per_point: f64) -> Option<i64> {
    let s = text.trim();
    let unit_at = s
        .len()
        .checked_sub(2)
        .filter(|&i| s.is_char_boundary(i) && s[i..].bytes().all(|b| b.is_ascii_alphabetic()));
    let value = match unit_at {
        Some(i) => {
            let per_pt = points_per_unit(&s[i..])?;
            parse_decimal(&s[..i])? * per_pt * per_point
        }
        None => parse_decimal(s)?,
    };
    let rounded = value.round();
    if rounded.abs() > 9.0e15 {
        return None;
    }
    Some(rounded as i64)
}

macro_rules! measure_codec {
    ($(#[$m:meta])* $name:ident, $ty:ty, $per_pt:expr, $xsd:literal) => {
        $(#[$m])*
        pub struct $name;

        impl Codec for $name {
            type Value = Val<$ty>;
            const NAME: &'static str = $xsd;

            fn parse(text: &str, ctx: &mut Ctx<'_>) -> Val<$ty> {
                match parse_measure(text, $per_pt).and_then(|n| <$ty>::try_from(n).ok()) {
                    Some(n) => Val::Value(n),
                    None => raw_val(text, ctx, Self::NAME),
                }
            }

            fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> Val<$ty> {
                match parse_measure(text, $per_pt).and_then(|n| <$ty>::try_from(n).ok()) {
                    Some(n) => Val::Value(n),
                    None => raw_attr(text, what, ctx, Self::NAME),
                }
            }

            fn write(v: &Val<$ty>, _flavor: PartFlavor) -> Cow<'_, str> {
                v.write(|n| Cow::Owned(n.to_string()))
            }

            fn missing(ctx: &mut Ctx<'_>) -> Val<$ty> {
                missing_val(ctx, Self::NAME)
            }
        }
    };
}

measure_codec!(
    /// `ST_HpsMeasure`：半点，无符号。`12pt` → 24。
    HalfPoints, u32, 2.0, "ST_HpsMeasure"
);
measure_codec!(
    /// `ST_SignedHpsMeasure`：半点，有符号（`w:position`）。
    SignedHalfPoints, i32, 2.0, "ST_SignedHpsMeasure"
);
measure_codec!(
    /// `ST_TwipsMeasure` / `ST_SignedTwipsMeasure`：1/20 pt。`1in` → 1440。
    Twips, i32, 20.0, "ST_TwipsMeasure"
);
measure_codec!(
    /// `ST_EighthPointMeasure`：1/8 pt，无符号（边框宽度）。
    EighthPoints, u32, 8.0, "ST_EighthPointMeasure"
);

// ---- 简单标量 ------------------------------------------------------------------------------------

macro_rules! simple_val_codec {
    ($(#[$m:meta])* $name:ident, $ty:ty, $xsd:literal, |$t:ident| $parse:expr, |$v:ident| $write:expr) => {
        $(#[$m])*
        pub struct $name;

        impl Codec for $name {
            type Value = Val<$ty>;
            const NAME: &'static str = $xsd;

            fn parse(text: &str, ctx: &mut Ctx<'_>) -> Val<$ty> {
                let $t = text.trim();
                match $parse {
                    Some(v) => Val::Value(v),
                    None => raw_val(text, ctx, Self::NAME),
                }
            }

            fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> Val<$ty> {
                let $t = text.trim();
                match $parse {
                    Some(v) => Val::Value(v),
                    None => raw_attr(text, what, ctx, Self::NAME),
                }
            }

            fn write(v: &Val<$ty>, _flavor: PartFlavor) -> Cow<'_, str> {
                v.write(|$v| $write)
            }

            fn missing(ctx: &mut Ctx<'_>) -> Val<$ty> {
                missing_val(ctx, Self::NAME)
            }
        }
    };
}

simple_val_codec!(
    /// `ST_TextScale`：整数或 `NN%`；生成整数。
    Percent, u32, "ST_TextScale",
    |t| parse_decimal(t.strip_suffix('%').unwrap_or(t)).map(f64::round).filter(|n| (0.0..=f64::from(u32::MAX)).contains(n)).map(|n| n as u32),
    |n| Cow::Owned(n.to_string())
);
simple_val_codec!(
    /// 两位十六进制（`themeTint` / `themeShade`）；生成大写。
    Hex2, u8, "ST_UcharHexNumber",
    |t| if t.len() == 2 { u8::from_str_radix(t, 16).ok() } else { None },
    |n| Cow::Owned(format!("{n:02X}"))
);
simple_val_codec!(
    /// `ST_DecimalNumber`（有符号整数）。
    Int, i32, "ST_DecimalNumber",
    |t| t.parse::<i32>().ok(),
    |n| Cow::Owned(n.to_string())
);
simple_val_codec!(
    /// 无符号整数（`ST_UnsignedDecimalNumber`）。
    UInt, u32, "ST_UnsignedDecimalNumber",
    |t| t.parse::<u32>().ok(),
    |n| Cow::Owned(n.to_string())
);

/// `ST_HexColor`：`auto` 或 6 位 hex（容忍前导 `#` 与大小写）；生成大写 hex 或 `auto`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HexColorOrAuto {
    Auto,
    Rgb([u8; 3]),
}

impl HexColorOrAuto {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("auto") {
            return Some(HexColorOrAuto::Auto);
        }
        let hex = s.strip_prefix('#').unwrap_or(s);
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let n = u32::from_str_radix(hex, 16).ok()?;
        Some(HexColorOrAuto::Rgb([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
    }

    pub fn to_xml(self) -> String {
        match self {
            HexColorOrAuto::Auto => "auto".to_string(),
            HexColorOrAuto::Rgb([r, g, b]) => format!("{r:02X}{g:02X}{b:02X}"),
        }
    }

    pub fn rgb(self) -> Option<[u8; 3]> {
        match self {
            HexColorOrAuto::Auto => None,
            HexColorOrAuto::Rgb(c) => Some(c),
        }
    }
}

impl Codec for HexColorOrAuto {
    type Value = Val<HexColorOrAuto>;
    const NAME: &'static str = "ST_HexColor";

    fn parse(text: &str, ctx: &mut Ctx<'_>) -> Val<HexColorOrAuto> {
        match HexColorOrAuto::parse(text) {
            Some(c) => Val::Value(c),
            None => raw_val(text, ctx, Self::NAME),
        }
    }

    fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> Val<HexColorOrAuto> {
        match HexColorOrAuto::parse(text) {
            Some(c) => Val::Value(c),
            None => raw_attr(text, what, ctx, Self::NAME),
        }
    }

    fn write(v: &Val<HexColorOrAuto>, _flavor: PartFlavor) -> Cow<'_, str> {
        v.write(|c| Cow::Owned(c.to_xml()))
    }

    fn missing(ctx: &mut Ctx<'_>) -> Val<HexColorOrAuto> {
        missing_val(ctx, Self::NAME)
    }
}

/// `ST_MeasurementOrPercent`（`CT_TblWidth/@w:w`）：无单位十进制数、带单位度量或字面 `NN%`。
/// 无单位数的单位由同元素的 `w:type` 决定（`TblWidth::twips` / `TblWidth::percent`），codec 只区分
/// "数"与"百分数字面"；带单位的度量换算成 twips（只有 `dxa` 才会带单位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Measure {
    /// 无单位数（`type=dxa` 时是 twips，`type=pct` 时是 1/50 百分点）；带单位的度量已换算为 twips。
    Number(i32),
    /// 字面百分数，单位 1/100 百分点：`"50%"` → 5000，`"12.5%"` → 1250。
    Percent(i32),
}

impl Measure {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(pct) = s.strip_suffix('%') {
            let hundredths = (parse_decimal(pct.trim())? * 100.0).round();
            if hundredths.abs() > f64::from(i32::MAX) {
                return None;
            }
            return Some(Measure::Percent(hundredths as i32));
        }
        parse_measure(s, 20.0).and_then(|n| i32::try_from(n).ok()).map(Measure::Number)
    }

    pub fn to_xml(self) -> String {
        match self {
            Measure::Number(n) => n.to_string(),
            Measure::Percent(h) if h % 100 == 0 => format!("{}%", h / 100),
            Measure::Percent(h) => format!("{}%", f64::from(h) / 100.0),
        }
    }
}

/// `Measure` 的 codec（`CT_TblWidth` 各处的 `w:w`）。
pub struct MeasureOrPercent;

impl Codec for MeasureOrPercent {
    type Value = Val<Measure>;
    const NAME: &'static str = "ST_MeasurementOrPercent";

    fn parse(text: &str, ctx: &mut Ctx<'_>) -> Val<Measure> {
        match Measure::parse(text) {
            Some(m) => Val::Value(m),
            None => raw_val(text, ctx, Self::NAME),
        }
    }

    fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> Val<Measure> {
        match Measure::parse(text) {
            Some(m) => Val::Value(m),
            None => raw_attr(text, what, ctx, Self::NAME),
        }
    }

    fn write(v: &Val<Measure>, _flavor: PartFlavor) -> Cow<'_, str> {
        v.write(|m| Cow::Owned(m.to_xml()))
    }

    fn missing(ctx: &mut Ctx<'_>) -> Val<Measure> {
        missing_val(ctx, Self::NAME)
    }
}

/// 原文字符串（`ST_String`）；不会失败。
pub struct Str;

impl Codec for Str {
    type Value = String;
    const NAME: &'static str = "ST_String";

    fn parse(text: &str, _ctx: &mut Ctx<'_>) -> String {
        text.to_owned()
    }

    fn write(v: &String, _flavor: PartFlavor) -> Cow<'_, str> {
        Cow::Borrowed(v)
    }

    fn missing(ctx: &mut Ctx<'_>) -> String {
        ctx.missing_value("w:val", Self::NAME);
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_02_measure_conversions() {
        assert_eq!(parse_measure("24", 2.0), Some(24));
        assert_eq!(parse_measure("12pt", 2.0), Some(24));
        assert_eq!(parse_measure("1in", 20.0), Some(1440));
        assert_eq!(parse_measure("1pc", 20.0), Some(240));
        assert_eq!(parse_measure("1pi", 20.0), Some(240));
        assert_eq!(parse_measure("1cm", 20.0), Some(567));
        assert_eq!(parse_measure("1mm", 20.0), Some(57));
        assert_eq!(parse_measure("-0.5in", 20.0), Some(-720));
        assert_eq!(parse_measure("2.5pt", 8.0), Some(20));
        assert_eq!(parse_measure(" 240 ", 20.0), Some(240));
        assert_eq!(parse_measure("", 20.0), None);
        assert_eq!(parse_measure("abc", 20.0), None);
        assert_eq!(parse_measure("1e3", 20.0), None);
        assert_eq!(parse_measure("12px", 20.0), None);
        assert_eq!(parse_measure("pt", 20.0), None);
    }

    #[test]
    fn prop_02_hex_color() {
        assert_eq!(HexColorOrAuto::parse("FF0000"), Some(HexColorOrAuto::Rgb([255, 0, 0])));
        assert_eq!(HexColorOrAuto::parse("#00ff7f"), Some(HexColorOrAuto::Rgb([0, 255, 127])));
        assert_eq!(HexColorOrAuto::parse("Auto"), Some(HexColorOrAuto::Auto));
        assert_eq!(HexColorOrAuto::parse("FFF"), None);
        assert_eq!(HexColorOrAuto::parse("GG0000"), None);
        assert_eq!(HexColorOrAuto::Rgb([0, 255, 127]).to_xml(), "00FF7F");
        assert_eq!(HexColorOrAuto::Auto.to_xml(), "auto");
    }

    #[test]
    fn prop_02_measure_or_percent() {
        assert_eq!(Measure::parse("2500"), Some(Measure::Number(2500)));
        assert_eq!(Measure::parse("-115"), Some(Measure::Number(-115)));
        assert_eq!(Measure::parse("1in"), Some(Measure::Number(1440)));
        assert_eq!(Measure::parse("50%"), Some(Measure::Percent(5000)));
        assert_eq!(Measure::parse(" 12.5% "), Some(Measure::Percent(1250)));
        assert_eq!(Measure::parse("abc"), None);
        assert_eq!(Measure::parse("%"), None);
        assert_eq!(Measure::Number(2500).to_xml(), "2500");
        assert_eq!(Measure::Percent(5000).to_xml(), "50%");
        assert_eq!(Measure::Percent(1250).to_xml(), "12.5%");
        assert_eq!(Measure::Percent(1234).to_xml(), "12.34%");
    }

    #[test]
    fn prop_02_on_off_write_by_flavor() {
        assert_eq!(OnOff::write(&true, PartFlavor::Transitional), "1");
        assert_eq!(OnOff::write(&false, PartFlavor::Transitional), "0");
        assert_eq!(OnOff::write(&true, PartFlavor::Strict), "true");
        assert_eq!(OnOff::write(&false, PartFlavor::Strict), "false");
        assert_eq!(OnOff::val_attr(&true, PartFlavor::Strict), None);
        assert_eq!(OnOff::val_attr(&false, PartFlavor::Strict).as_deref(), Some("false"));
    }
}
