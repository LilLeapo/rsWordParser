//! 主题颜色解析（`RES-05`）：槽位映射、shade → tint 的 sRGB 逐通道近似。

use crate::model::theme::{ColorScheme, ThemeSlot};
use crate::semantic::props::{Color, HexColorOrAuto, Val};

/// `c*s/255` 再 `c*t/255 + 255*(1 - t/255)`，逐通道四舍五入。`tint`/`shade` 是 `themeTint`/`themeShade` 的 0–255。
pub fn apply_tint_shade(base: [u8; 3], tint: Option<u8>, shade: Option<u8>) -> [u8; 3] {
    let mut out = [0u8; 3];
    for (o, &c) in out.iter_mut().zip(base.iter()) {
        let mut v = f64::from(c);
        if let Some(s) = shade {
            v = v * f64::from(s) / 255.0;
        }
        if let Some(t) = tint {
            let t = f64::from(t);
            v = v * t / 255.0 + 255.0 * (1.0 - t / 255.0);
        }
        *o = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// 槽位 → 调色板颜色（dk1/lt1 有缺省）→ shade / tint。
pub fn resolve_theme_color(
    palette: &ColorScheme,
    slot: ThemeSlot,
    tint: Option<u8>,
    shade: Option<u8>,
) -> Option<[u8; 3]> {
    let base = palette.get_or_default(slot)?;
    Some(apply_tint_shade(base, tint, shade))
}

/// `w:color` 的有效 sRGB：`themeColor`（可解析且有槽位）优先，否则 `val`；`auto` → `None`（由渲染器决定）。
pub fn resolve_color(c: &Color, palette: &ColorScheme) -> Option<[u8; 3]> {
    if let Some(Val::Value(tc)) = &c.theme_color
        && let Some(slot) = ThemeSlot::from_theme_color(*tc)
        && let Some(rgb) =
            resolve_theme_color(palette, slot, hex2(&c.theme_tint), hex2(&c.theme_shade))
    {
        return Some(rgb);
    }
    match &c.val {
        Some(Val::Value(HexColorOrAuto::Rgb(rgb))) => Some(*rgb),
        Some(Val::Raw(s)) => HexColorOrAuto::parse(s).and_then(HexColorOrAuto::rgb),
        _ => None,
    }
}

fn hex2(v: &Option<Val<u8>>) -> Option<u8> {
    match v {
        Some(Val::Value(n)) => Some(*n),
        Some(Val::Raw(s)) => u8::from_str_radix(s.trim(), 16).ok(),
        None => None,
    }
}

/// 大写 6 位 hex。
pub fn rgb_hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
