//! 主题字体解析（`RES-05`）：`rFonts` 的主题属性覆盖同槽字面值；空 EA 槽按 `themeFontLang`。

use crate::model::{FontScheme, FontSlots};
use crate::semantic::props::{Fonts, ThemeFont, Val};

/// `w:rFonts` 解析后的四个槽位。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedFonts {
    pub ascii: Option<String>,
    pub h_ansi: Option<String>,
    pub east_asia: Option<String>,
    pub cs: Option<String>,
    /// 有 `eastAsiaTheme`、主题存在、槽 typeface 为空：`east_asia` 是按语言给的缺省面而非文档选择。
    pub ea_slot_empty: bool,
    /// docDefaults：`east_asia` 是按 `w:lang/@eastAsia` 回填的。
    pub ea_from_lang: bool,
    /// 各槽位是否来自主题（ascii, hAnsi, eastAsia, cs）。
    pub themed: [bool; 4],
}

impl ResolvedFonts {
    /// TS 兼容显示字体：`eastAsia ?? ascii ?? hAnsi`。
    pub fn display(&self) -> Option<&str> {
        self.east_asia.as_deref().or(self.ascii.as_deref()).or(self.h_ansi.as_deref())
    }

    /// `ascii ?? hAnsi`。
    pub fn display_ascii(&self) -> Option<&str> {
        self.ascii.as_deref().or(self.h_ansi.as_deref())
    }
}

/// 主题引用 → 字体方案的槽位。
fn theme_val(scheme: Option<&FontScheme>, r: ThemeFont) -> Option<String> {
    let scheme = scheme?;
    let slots: &FontSlots = match r {
        ThemeFont::MajorAscii
        | ThemeFont::MajorHAnsi
        | ThemeFont::MajorEastAsia
        | ThemeFont::MajorBidi => &scheme.major,
        _ => &scheme.minor,
    };
    let v = match r {
        ThemeFont::MajorAscii
        | ThemeFont::MajorHAnsi
        | ThemeFont::MinorAscii
        | ThemeFont::MinorHAnsi => &slots.latin,
        ThemeFont::MajorEastAsia | ThemeFont::MinorEastAsia => &slots.ea,
        ThemeFont::MajorBidi | ThemeFont::MinorBidi => &slots.cs,
    };
    v.clone().filter(|s| !s.is_empty())
}

fn theme_ref(v: &Option<Val<ThemeFont>>) -> Option<ThemeFont> {
    v.as_ref().and_then(|x| x.value().copied())
}

/// `themeFontLang/@eastAsia` → 主题 `a:font script` 表的 script 名。
fn ea_lang_script(full: &str, lang: &str) -> Option<&'static str> {
    Some(match (full, lang) {
        ("zh-cn" | "zh-sg", _) => "Hans",
        ("zh-tw" | "zh-hk" | "zh-mo", _) => "Hant",
        (_, "ko") => "Hang",
        (_, "ja") => "Jpan",
        (_, "zh") => "Hans",
        _ => return None,
    })
}

/// 空 EA 槽的字体：先查主题 script 表，再按语言的实测缺省（ja → Yu Gothic / Yu Mincho，ko → Malgun Gothic）；
/// 都没有 → `None`（调用方用 DengXian）。
pub fn theme_lang_ea_slot_font(
    scheme: Option<&FontScheme>,
    ea_lang: Option<&str>,
    ea_ref: Option<ThemeFont>,
) -> Option<String> {
    let full = ea_lang?.to_ascii_lowercase();
    let lang = full.split('-').next().unwrap_or("").to_string();
    let major = ea_ref == Some(ThemeFont::MajorEastAsia);
    if let Some(script) = ea_lang_script(&full, &lang)
        && let Some(scheme) = scheme
    {
        let slots = if major { &scheme.major } else { &scheme.minor };
        if let Some(t) = slots.script(script) {
            return Some(t.to_string());
        }
    }
    match lang.as_str() {
        "ja" => Some(if major { "Yu Gothic" } else { "Yu Mincho" }.to_string()),
        "ko" => Some("Malgun Gothic".to_string()),
        _ => None,
    }
}

/// Word 对空 EA 主题槽的通用缺省面。
pub const EMPTY_EA_THEME_FONT: &str = "DengXian";

/// docDefaults 的 EA 回填：`rPrDefault/w:lang/@eastAsia` → 字体。
pub fn ea_lang_default_font(lang: &str) -> Option<&'static str> {
    Some(match lang.to_ascii_lowercase().as_str() {
        "ko" | "ko-kr" => "Malgun Gothic",
        "ja" | "ja-jp" => "MS Mincho",
        "zh-cn" => "SimSun",
        "zh-tw" | "zh-hk" => "PMingLiU",
        _ => return None,
    })
}

/// `RES-05`：主题属性覆盖同槽字面值；解析不到退回字面值；空 EA 槽按语言给缺省并标 `ea_slot_empty`。
pub fn resolve_fonts(
    fonts: Option<&Fonts>,
    scheme: Option<&FontScheme>,
    ea_lang: Option<&str>,
) -> ResolvedFonts {
    let Some(f) = fonts else { return ResolvedFonts::default() };
    let ea_ref = theme_ref(&f.east_asia_theme);
    let themed_ea = ea_ref.and_then(|r| theme_val(scheme, r));
    let ea_slot_empty = themed_ea.is_none()
        && scheme.is_some()
        && matches!(ea_ref, Some(ThemeFont::MajorEastAsia | ThemeFont::MinorEastAsia));
    let themed_ascii = theme_ref(&f.ascii_theme).and_then(|r| theme_val(scheme, r));
    let themed_h_ansi = theme_ref(&f.h_ansi_theme).and_then(|r| theme_val(scheme, r));
    let themed_cs = theme_ref(&f.cs_theme).and_then(|r| theme_val(scheme, r));
    let east_asia = if let Some(t) = themed_ea.clone() {
        Some(t)
    } else if ea_slot_empty {
        Some(
            theme_lang_ea_slot_font(scheme, ea_lang, ea_ref)
                .unwrap_or_else(|| EMPTY_EA_THEME_FONT.to_string()),
        )
    } else {
        f.east_asia.clone()
    };
    ResolvedFonts {
        themed: [
            themed_ascii.is_some(),
            themed_h_ansi.is_some(),
            themed_ea.is_some() || ea_slot_empty,
            themed_cs.is_some(),
        ],
        ascii: themed_ascii.or_else(|| f.ascii.clone()),
        h_ansi: themed_h_ansi.or_else(|| f.h_ansi.clone()),
        east_asia,
        cs: themed_cs.or_else(|| f.cs.clone()),
        ea_slot_empty,
        ea_from_lang: false,
    }
}
