//! 长度单位换算（`spec/15` 任务 4.2）。
//!
//! 模型层一律存 **EMU 原值**（`MOD-11`：显示模型只放文档事实）。px 是显示投影，只有
//! `bind/compat_ts` 用得着——TS 的 `*Px` 字段按 96 dpi 换算。换算集中在这里，免得
//! `9525` 这个魔数散落各处。
//!
//! | 单位 | 每单位 EMU | 出处 |
//! | --- | --- | --- |
//! | 英寸 | 914,400 | ECMA-376 |
//! | 磅 pt | 12,700 | 1 pt = 1/72 in |
//! | 缇 twip | 635 | 1 twip = 1/20 pt |
//! | 像素 px | 9,525 | 96 dpi（TS 用同一个常量） |

/// 1 英寸的 EMU。
pub const EMU_PER_INCH: f64 = 914_400.0;
/// 1 磅的 EMU。
pub const EMU_PER_PT: f64 = 12_700.0;
/// 1 缇（1/20 磅）的 EMU。
pub const EMU_PER_TWIP: f64 = 635.0;
/// 1 像素的 EMU（96 dpi）。**只用于显示投影**。
pub const EMU_PER_PX: f64 = 9_525.0;

pub fn emu_to_px(emu: f64) -> f64 {
    emu / EMU_PER_PX
}

pub fn px_to_emu(px: f64) -> f64 {
    px * EMU_PER_PX
}

pub fn emu_to_pt(emu: f64) -> f64 {
    emu / EMU_PER_PT
}

pub fn pt_to_emu(pt: f64) -> f64 {
    pt * EMU_PER_PT
}

pub fn twips_to_emu(twips: f64) -> f64 {
    twips * EMU_PER_TWIP
}

pub fn emu_to_twips(emu: f64) -> f64 {
    emu / EMU_PER_TWIP
}

/// CSS / VML `style` 里的长度单位。VML 的 `style="width:96pt;margin-left:12.5pt"` 走这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Pt,
    Px,
    In,
    Cm,
    Mm,
    Pc,
    /// 没写单位。VML 里表示「用组的坐标系」，换算要靠上层的组比例（4.5 处理）。
    None,
}

impl LengthUnit {
    /// 该单位一个的 EMU。[`LengthUnit::None`] 没有绝对值。
    pub fn emu(self) -> Option<f64> {
        Some(match self {
            LengthUnit::Pt => EMU_PER_PT,
            LengthUnit::Px => EMU_PER_PX,
            LengthUnit::In => EMU_PER_INCH,
            LengthUnit::Cm => EMU_PER_INCH / 2.54,
            LengthUnit::Mm => EMU_PER_INCH / 25.4,
            // 1 pica = 12 pt
            LengthUnit::Pc => EMU_PER_PT * 12.0,
            LengthUnit::None => return None,
        })
    }
}

/// 一个带单位的长度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: LengthUnit,
}

impl Length {
    /// 绝对长度的 EMU；无单位 → `None`。
    pub fn to_emu(self) -> Option<f64> {
        Some(self.value * self.unit.emu()?)
    }
}

/// 解析 CSS 长度（`96pt`、`-12.5px`、`3.5`、`1in`）。单位大小写不敏感，允许前后空白。
pub fn parse_length(s: &str) -> Option<Length> {
    let s = s.trim();
    let split = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+'))
        .map_or(s.len(), |(i, _)| i);
    let (num, rest) = s.split_at(split);
    let value = num.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    let unit = match rest.trim().to_ascii_lowercase().as_str() {
        "" => LengthUnit::None,
        "pt" => LengthUnit::Pt,
        "px" => LengthUnit::Px,
        "in" => LengthUnit::In,
        "cm" => LengthUnit::Cm,
        "mm" => LengthUnit::Mm,
        "pc" => LengthUnit::Pc,
        _ => return None,
    };
    Some(Length { value, unit })
}

/// 解析 CSS `style` 属性为键值对（分号分隔、冒号赋值），键转小写、值去空白。
///
/// VML 把几何写在 `style` 里（`position:absolute;margin-left:36pt;width:96pt`），`MOD-11` 要求
/// `VmlDisplay` **原样保留**这些键值，所以这里不做语义解释。
pub fn parse_style(style: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if !k.is_empty() {
            out.push((k, v.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_11_unit_conversions_round_trip() {
        assert_eq!(emu_to_px(914_400.0), 96.0);
        assert_eq!(emu_to_pt(914_400.0), 72.0);
        assert_eq!(emu_to_twips(914_400.0), 1440.0);
        assert_eq!(pt_to_emu(72.0), 914_400.0);
        assert_eq!(twips_to_emu(1440.0), 914_400.0);
        assert_eq!(px_to_emu(96.0), 914_400.0);
        // 语料里最常见的一张图：cx=914400 → 96px、cy=457200 → 48px
        assert_eq!(emu_to_px(457_200.0), 48.0);
    }

    #[test]
    fn mod_11_parse_length_units() {
        assert_eq!(parse_length("96pt"), Some(Length { value: 96.0, unit: LengthUnit::Pt }));
        assert_eq!(parse_length(" -12.5px "), Some(Length { value: -12.5, unit: LengthUnit::Px }));
        assert_eq!(parse_length("3.5"), Some(Length { value: 3.5, unit: LengthUnit::None }));
        assert_eq!(parse_length("1IN").unwrap().to_emu(), Some(914_400.0));
        assert_eq!(parse_length("2.54cm").unwrap().to_emu().unwrap().round(), 914_400.0);
        assert_eq!(parse_length("25.4mm").unwrap().to_emu().unwrap().round(), 914_400.0);
        assert_eq!(parse_length("6pc").unwrap().to_emu(), Some(914_400.0));
        assert_eq!(parse_length("3.5").unwrap().to_emu(), None);
        assert_eq!(parse_length("auto"), None);
        assert_eq!(parse_length("10em"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn mod_11_parse_style_keeps_pairs_verbatim() {
        let s = parse_style("position:absolute;MARGIN-LEFT: 36pt ;width:96pt;;bogus");
        assert_eq!(
            s,
            vec![
                ("position".to_string(), "absolute".to_string()),
                ("margin-left".to_string(), "36pt".to_string()),
                ("width".to_string(), "96pt".to_string()),
            ]
        );
        // 值里带冒号（mso-position 之类）只在第一个冒号处切
        assert_eq!(
            parse_style("mso-wrap-style:none:x"),
            vec![("mso-wrap-style".to_string(), "none:x".to_string())]
        );
    }
}
