//! DrawingML 颜色（`RES-05`；`spec/15` 任务 4.2）。
//!
//! 一个「颜色容器」（`a:solidFill` / `a:gs` / `a:fillRef` / `a:lnRef` / `a:fgClr` …）里恰有一个颜色
//! 元素，颜色元素下面挂零到多个变换子元素。这里把它解析成 [`DrawingColor`]——**保留原始定义**
//! （`MOD-11`），需要 sRGB 时再调 [`DrawingColor::to_rgb`]。
//!
//! ## 色彩空间（实现选择，`docs/04` §8 有记录）
//!
//! - `lumMod` / `lumOff` / `shade` / `tint`：sRGB 逐通道（`c*mod`、`c + 255*off`、`c*s`、
//!   `c*t + 255*(1-t)`）。
//! - `satMod` / `hueMod`：只能在 HSL 里做。
//!
//! ECMA-376 对 `lum*` 的措辞指向线性化色彩空间。实际比过两处：语料 `bugfix-regressions__025` 的
//! `ED7D31` + `lumMod 40%` + `lumOff 60%`，以及 Office 调色板 `4472C4` 的「淡色 80%」。两处逐通道
//! 与 HSL 给出同一个结果，且与 Word 公布的值差 ≤ 1/255——正是 `RES-05` 验收允许的误差；逐通道又与
//! TS 一致，能让绘图域差分为 0。真要按线性空间做，先补 Word 实测 fixture（同 `RES-04` 的 toggle）。
//!
//! ## 变换顺序
//!
//! 按**文档顺序**施加（ECMA 的规定）。TS 用固定顺序 `lumMod → lumOff → shade → tint`；语料里两者
//! 一致（只出现 `lumMod`+`lumOff` 与单独的 `shade`）。
//!
//! ## 与 TS 的一处刻意不同
//!
//! TS 的 `gradStopRgb` 只在底色来自 `a:schemeClr` 时施加变换，`<a:srgbClr><a:lumMod/></a:srgbClr>`
//! 的变换被丢掉。那是 TS 的缺陷（`docs/04` §8「TS 的缺陷不跟随」），这里一律施加。语料里没有这种
//! 写法，所以不影响差分。

use crate::model::{ColorScheme, ThemeSlot};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// sRGB 三通道，0–255，**保留小数**：多段渐变要先平均再取整（TS `gradFillApproxHex` 同序）。
pub type Rgb = [f64; 3];

/// 颜色元素的底色定义。
#[derive(Debug, Clone, PartialEq)]
pub enum ColorBase {
    /// `a:srgbClr/@val`
    Srgb([u8; 3]),
    /// `a:sysClr`：Word 会把解析后的系统色写进 `lastClr`。
    Sys { val: Option<String>, last: Option<[u8; 3]> },
    /// `a:prstClr/@val`
    Prst { name: String, rgb: Option<[u8; 3]> },
    /// `a:schemeClr/@val`（含别名 `tx1/bg1/tx2/bg2`）
    Scheme { name: String, slot: Option<ThemeSlot> },
    /// `a:scrgbClr`：三个百分比。
    Scrgb(Rgb),
    /// `a:hslClr`：`hue` 单位是 1/60000 度，`sat`/`lum` 是千分之一百分比。
    Hsl { hue: f64, sat: f64, lum: f64 },
}

/// 颜色变换子元素。数值已归一（百分比 → 0.0–1.0，色相 → 度）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorTransform {
    LumMod(f64),
    LumOff(f64),
    Shade(f64),
    Tint(f64),
    SatMod(f64),
    HueMod(f64),
    /// 不参与 sRGB 计算，只记录（`compat_ts` 的显示字段没有 alpha 通道）。
    Alpha(f64),
}

/// 一个 DrawingML 颜色：底色 + 按文档顺序排列的变换。
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingColor {
    pub node: NodeId,
    pub base: ColorBase,
    pub transforms: Vec<ColorTransform>,
}

impl DrawingColor {
    /// 解析成 sRGB。底色定不下来（未知 `schemeClr` 槽位、槽位在主题里缺失且无缺省、未知预设名）
    /// → `None`，调用方按「没写颜色」处理。
    pub fn to_rgb(&self, palette: &ColorScheme) -> Option<Rgb> {
        let mut rgb = self.base_rgb(palette)?;
        for t in &self.transforms {
            rgb = apply(rgb, *t);
        }
        Some(rgb)
    }

    /// `a:alpha`（0.0–1.0）。没写 → `None`（不透明）。
    pub fn alpha(&self) -> Option<f64> {
        self.transforms.iter().find_map(|t| match t {
            ColorTransform::Alpha(a) => Some(*a),
            _ => None,
        })
    }

    fn base_rgb(&self, palette: &ColorScheme) -> Option<Rgb> {
        let c = match &self.base {
            ColorBase::Srgb(c) => *c,
            ColorBase::Sys { val, last } => match last {
                Some(c) => *c,
                // Word 没写 lastClr 时按 TS 的退路：windowText 黑、其余白。
                None if val.as_deref() == Some("windowText") => [0, 0, 0],
                None => [0xFF, 0xFF, 0xFF],
            },
            ColorBase::Prst { rgb, .. } => (*rgb)?,
            ColorBase::Scheme { slot, .. } => palette.get_or_default((*slot)?)?,
            ColorBase::Scrgb(c) => return Some(*c),
            ColorBase::Hsl { hue, sat, lum } => return Some(hsl_to_rgb(*hue, *sat, *lum)),
        };
        Some([f64::from(c[0]), f64::from(c[1]), f64::from(c[2])])
    }
}

fn apply(rgb: Rgb, t: ColorTransform) -> Rgb {
    match t {
        ColorTransform::LumMod(m) => rgb.map(|c| c * m),
        ColorTransform::LumOff(o) => rgb.map(|c| c + 255.0 * o),
        ColorTransform::Shade(s) => rgb.map(|c| c * s),
        ColorTransform::Tint(t) => rgb.map(|c| c * t + 255.0 * (1.0 - t)),
        ColorTransform::SatMod(m) => {
            let (h, s, l) = rgb_to_hsl(rgb);
            hsl_to_rgb(h, (s * m).clamp(0.0, 1.0), l)
        }
        ColorTransform::HueMod(m) => {
            let (h, s, l) = rgb_to_hsl(rgb);
            hsl_to_rgb((h * m).rem_euclid(360.0), s, l)
        }
        ColorTransform::Alpha(_) => rgb,
    }
}

// ---- 解析 ---------------------------------------------------------------------------------------

/// 容器里的第一个颜色元素（`a:solidFill` / `a:gs` / `a:fillRef` / `a:lnRef` / `a:fgClr` …）。
pub fn color_in(dom: &Dom, container: NodeId) -> Option<DrawingColor> {
    color_in_ns(dom, container, NsId::A)
}

/// 同 [`color_in`]，颜色元素在 `ns` 命名空间下（`w14:solidFill` / `w14:gs` 里的 `w14:srgbClr` /
/// `w14:schemeClr` 与 DrawingML 同一套语法，任务 6.9）。
pub fn color_in_ns(dom: &Dom, container: NodeId, ns: NsId) -> Option<DrawingColor> {
    dom.semantic_children(container).find_map(|c| parse_color_ns(dom, c, ns))
}

/// 颜色元素本身（`a:srgbClr` / `a:sysClr` / `a:prstClr` / `a:schemeClr` / `a:scrgbClr` / `a:hslClr`）。
pub fn parse_color(dom: &Dom, node: NodeId) -> Option<DrawingColor> {
    parse_color_ns(dom, node, NsId::A)
}

/// 同 [`parse_color`]，元素与它的变换子元素都在 `ns` 命名空间下。
pub fn parse_color_ns(dom: &Dom, node: NodeId, ns: NsId) -> Option<DrawingColor> {
    let name = dom.name(node)?;
    if name.ns != ns {
        return None;
    }
    let val = attr(dom, node, LocalName::Val);
    let base = match name.local {
        LocalName::SrgbClr => ColorBase::Srgb(parse_hex(val.as_deref()?)?),
        LocalName::SysClr => ColorBase::Sys {
            val,
            last: attr(dom, node, LocalName::LastClr).as_deref().and_then(parse_hex),
        },
        LocalName::PrstClr => {
            let name = val?;
            ColorBase::Prst { rgb: preset_rgb(&name), name }
        }
        LocalName::SchemeClr => {
            let name = val?;
            ColorBase::Scheme { slot: ThemeSlot::from_scheme_name(&name), name }
        }
        LocalName::ScrgbClr => ColorBase::Scrgb([
            pct(dom, node, LocalName::R)? * 255.0,
            pct(dom, node, LocalName::G)? * 255.0,
            pct(dom, node, LocalName::B)? * 255.0,
        ]),
        LocalName::HslClr => ColorBase::Hsl {
            // `@hue` 是 1/60000 度。
            hue: num(dom, node, LocalName::Hue)? / 60000.0,
            sat: pct(dom, node, LocalName::Sat)?,
            lum: pct(dom, node, LocalName::Lum)?,
        },
        _ => return None,
    };
    Some(DrawingColor { node, base, transforms: transforms_of(dom, node, ns) })
}

fn transforms_of(dom: &Dom, color: NodeId, ns: NsId) -> Vec<ColorTransform> {
    let mut out = Vec::new();
    for c in dom.semantic_children(color) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != ns {
            continue;
        }
        let Some(v) = attr_pct(dom, c) else { continue };
        out.push(match name.local {
            LocalName::LumMod => ColorTransform::LumMod(v),
            LocalName::LumOff => ColorTransform::LumOff(v),
            LocalName::Shade => ColorTransform::Shade(v),
            LocalName::Tint => ColorTransform::Tint(v),
            LocalName::SatMod => ColorTransform::SatMod(v),
            LocalName::HueMod => ColorTransform::HueMod(v),
            LocalName::Alpha => ColorTransform::Alpha(v),
            _ => continue,
        });
    }
    out
}

/// 无前缀的属性；没有时再试元素自己的命名空间——DrawingML 写 `<a:srgbClr val=…>`，Word 2010 的
/// `w14:*` 颜色写 `<w14:srgbClr w14:val=…>`（任务 6.9）。
fn attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, local))
        .or_else(|| dom.attr_value(node, QName::new(dom.name(node)?.ns, local)))
        .map(|s| s.trim().to_string())
}

fn num(dom: &Dom, node: NodeId, local: LocalName) -> Option<f64> {
    attr(dom, node, local)?.parse::<f64>().ok()
}

/// `ST_Percentage`：千分之一百分比（`40000` = 40%），也接受 ISO 的 `40%` 写法。
fn parse_pct(s: &str) -> Option<f64> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|v| v / 100.0),
        None => s.parse::<f64>().ok().map(|v| v / 100_000.0),
    }
}

fn pct(dom: &Dom, node: NodeId, local: LocalName) -> Option<f64> {
    parse_pct(&attr(dom, node, local)?)
}

fn attr_pct(dom: &Dom, node: NodeId) -> Option<f64> {
    parse_pct(&attr(dom, node, LocalName::Val)?)
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// `ST_PresetColorVal` 的常用子集（X11 / SVG 取值）。表外的名字返回 `None`，按「没写颜色」处理。
fn preset_rgb(name: &str) -> Option<[u8; 3]> {
    let hex = match name {
        "black" => "000000",
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "gray" | "grey" => "808080",
        "dkGray" | "darkGray" => "A9A9A9",
        "ltGray" | "lightGray" => "D3D3D3",
        "dkRed" | "darkRed" => "8B0000",
        "dkGreen" | "darkGreen" => "006400",
        "dkBlue" | "darkBlue" => "00008B",
        "lime" => "00FF00",
        "navy" => "000080",
        "olive" => "808000",
        "purple" => "800080",
        "silver" => "C0C0C0",
        "teal" => "008080",
        "maroon" => "800000",
        "orange" => "FFA500",
        "brown" => "A52A2A",
        "pink" => "FFC0CB",
        _ => return None,
    };
    parse_hex(hex)
}

// ---- 输出 ---------------------------------------------------------------------------------------

/// 大写 6 位 hex，最后一步才 clamp + 四舍五入（中间计算保留小数）。
pub fn hex(rgb: Rgb) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("{:02X}{:02X}{:02X}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

/// 多个渐变停靠点的等权平均（`a:gradFill` 的单色近似，显示用）。
///
/// 第一个停靠点常常是白色，只取它会把可见的颜色丢光，所以取平均——与 TS `gradFillApproxHex` 一致。
pub fn average(stops: &[Rgb]) -> Option<Rgb> {
    if stops.is_empty() {
        return None;
    }
    let n = stops.len() as f64;
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = stops.iter().map(|s| s[i]).sum::<f64>() / n;
    }
    Some(out)
}

// ---- HSL ----------------------------------------------------------------------------------------

/// sRGB(0–255) → (色相度, 饱和度 0–1, 亮度 0–1)。
fn rgb_to_hsl(rgb: Rgb) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(|c| c.clamp(0.0, 255.0) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    if s <= 0.0 {
        let v = l * 255.0;
        return [v, v, v];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hk = h.rem_euclid(360.0) / 360.0;
    [
        hue_to_channel(p, q, hk + 1.0 / 3.0) * 255.0,
        hue_to_channel(p, q, hk) * 255.0,
        hue_to_channel(p, q, hk - 1.0 / 3.0) * 255.0,
    ]
}

fn hue_to_channel(p: f64, q: f64, t: f64) -> f64 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;
    use crate::xml::Dom;

    fn dom(src: &str) -> Dom {
        Dom::parse(PartId(0), src.as_bytes()).expect("dom")
    }

    /// 文档根下第一个颜色容器。
    fn color(src: &str) -> DrawingColor {
        let d = dom(src);
        color_in(&d, d.root()).expect("color")
    }

    const A: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

    #[test]
    fn res_05_srgb_and_transform_order() {
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="ED7D31"><a:lumMod val="40000"/><a:lumOff val="60000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(c.base, ColorBase::Srgb([0xED, 0x7D, 0x31]));
        assert_eq!(c.transforms, vec![ColorTransform::LumMod(0.4), ColorTransform::LumOff(0.6)]);
        // 0.4c + 153，与 Word 的「accent1 淡色 60%」F8CBAD 一致
        assert_eq!(hex(c.to_rgb(&ColorScheme::office_default()).unwrap()), "F8CBAD");
    }

    #[test]
    fn res_05_scheme_slot_aliases_and_theme_lookup() {
        let p = ColorScheme::office_default();
        for (name, want) in
            [("accent1", "4472C4"), ("tx1", "000000"), ("bg1", "FFFFFF"), ("tx2", "44546A")]
        {
            let c =
                color(&format!(r#"<a:solidFill {A}><a:schemeClr val="{name}"/></a:solidFill>"#));
            assert_eq!(hex(c.to_rgb(&p).unwrap()), want, "{name}");
        }
        // 未知槽位名 → 定不下来
        let c = color(&format!(r#"<a:solidFill {A}><a:schemeClr val="phon"/></a:solidFill>"#));
        assert!(c.to_rgb(&p).is_none());
    }

    #[test]
    fn res_05_shade_and_tint() {
        let p = ColorScheme::office_default();
        // accent1 4472C4 shade 50% → 223962（语料 themeless-shapes-external-txbx__001 的边框色）
        let c = color(&format!(
            r#"<a:solidFill {A}><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "223962");
        // tint 25%：c*0.25 + 191.25
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="000000"><a:tint val="25000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "BFBFBF");
    }

    #[test]
    fn res_05_gradient_average_rounds_last() {
        let p = ColorScheme::office_default();
        // 语料 bugfix-regressions__025：ED7D31 lumMod40/lumOff60 与 lt1 的两段渐变 → FBE5D6。
        // 该文档的主题把 accent1 定成 ED7D31；内建调色板里同一个值在 accent2 槽，这里借用它。
        // 先平均再取整才对：两段各自取整后再平均会得到 FCE5D6。
        let stop1 = color(&format!(
            r#"<a:gs {A}><a:schemeClr val="accent2"><a:lumMod val="40000"/><a:lumOff val="60000"/></a:schemeClr></a:gs>"#
        ))
        .to_rgb(&p)
        .unwrap();
        let stop2 =
            color(&format!(r#"<a:gs {A}><a:schemeClr val="lt1"/></a:gs>"#)).to_rgb(&p).unwrap();
        assert_eq!(hex(average(&[stop1, stop2]).unwrap()), "FBE5D6");
        assert!(average(&[]).is_none());
    }

    #[test]
    fn res_05_sys_and_preset_colors() {
        let p = ColorScheme::office_default();
        // Word 把解析后的系统色写进 lastClr
        let c = color(&format!(
            r#"<a:solidFill {A}><a:sysClr val="window" lastClr="FFFFFF"/></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "FFFFFF");
        // 没有 lastClr 时的退路
        let c = color(&format!(r#"<a:solidFill {A}><a:sysClr val="windowText"/></a:solidFill>"#));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "000000");
        let c = color(&format!(r#"<a:solidFill {A}><a:prstClr val="black"/></a:solidFill>"#));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "000000");
        let c = color(&format!(r#"<a:solidFill {A}><a:prstClr val="nosuch"/></a:solidFill>"#));
        assert!(c.to_rgb(&p).is_none());
    }

    #[test]
    fn res_05_sat_and_hue_go_through_hsl() {
        let p = ColorScheme::office_default();
        // 灰度没有饱和度可调
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="808080"><a:satMod val="200000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "808080");
        // 饱和度翻倍后仍然 clamp 在 1.0：纯红不变
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="FF0000"><a:satMod val="200000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "FF0000");
        // 半饱和的红提到 200% → 纯红
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="BF4040"><a:satMod val="200000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "FF0000");
        // hueMod 120% 把 0° 的红转到 0°*1.2 = 0°（红是不动点），用 60° 的黄验证
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="FFFF00"><a:hueMod val="200000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "00FF00");
    }

    #[test]
    fn res_05_scrgb_hsl_alpha_and_iso_percent() {
        let p = ColorScheme::office_default();
        let c = color(&format!(
            r#"<a:solidFill {A}><a:scrgbClr r="100000" g="0" b="0"/></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "FF0000");
        let c = color(&format!(
            r#"<a:solidFill {A}><a:hslClr hue="3600000" sat="100000" lum="50000"/></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "FFFF00");
        // ISO 写法的百分比
        let c = color(&format!(
            r#"<a:solidFill {A}><a:srgbClr val="000000"><a:tint val="25%"/><a:alpha val="50000"/></a:srgbClr></a:solidFill>"#
        ));
        assert_eq!(hex(c.to_rgb(&p).unwrap()), "BFBFBF");
        assert_eq!(c.alpha(), Some(0.5));
    }
}
