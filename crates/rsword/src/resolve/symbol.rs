//! 符号字体解码（`RES-05`）。
//!
//! `Symbol` / `Wingdings` / `Wingdings 2` / `Wingdings 3` / `Webdings` 里的字形没有对应的字符语义：
//! 文档里存的是字体私有编码（常见写法是 `0xF000 + 码位` 的 PUA 区间），显示时要按字体的映射表
//! 翻成 Unicode。**规范状态不改写**（`MOD-06` △）：`Run.text` 保持原字符，这里只提供显示视图，
//! `compat_ts` 用它折回 TS 的形态。
//!
//! 表的来源与完整度：`RES-05` 写的是"按 TS `symbol-fonts.ts` 的映射表"，但那份源码不在本仓库。
//! [`SYMBOL`] 是 Adobe Symbol 的标准映射（Unicode 的 `SYMBOL.TXT`），可以照抄；
//! [`WINGDINGS`] 只收了把握得住的常用字形。表外码位解码失败，调用方按 `RES-05` 保留原字符
//! （TS 也是这样，语料 `symbol-fonts__002` 的 `Wingdings 2 F045` 原样留着）。
//! 补表需要证据（语料或 `symbol-fonts.ts`），
//! **不要**凭印象加条目：猜错了比不解码更糟。

/// 视为符号字体的字体名（`RES-05`，大小写不敏感）。
pub fn is_symbol_font(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "symbol" | "wingdings" | "wingdings 2" | "wingdings 3" | "webdings"
    )
}

/// 把字体私有编码解码成 Unicode。`code` 可以是 `0xF000` 偏移过的写法。
///
/// 表外码位返回 `None`。
pub fn decode(font: &str, code: u32) -> Option<char> {
    let table = match font.trim().to_ascii_lowercase().as_str() {
        "symbol" => SYMBOL,
        "wingdings" => WINGDINGS,
        // Wingdings 2 / 3 与 Webdings：还没有可靠的表（语料 `symbol-fonts__002` 里 TS 也没解码
        // `Wingdings 2` 的 `F045`），先按解码失败处理
        "wingdings 2" | "wingdings 3" | "webdings" => &[],
        _ => return None,
    };
    let c = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
    let c = u8::try_from(c).ok()?;
    table.iter().find(|(k, _)| *k == c).map(|(_, v)| *v)
}

/// 一个字符若落在符号字体的 PUA 区间（`U+F000`–`U+F0FF`）就解码。
///
/// 只解码 PUA 区间：符号字体 run 里的普通 ASCII 字母（语料 `symbol-fonts__004` 的 `le`）TS 也不动，
/// 那种文本在没有该字体时本来就按字母显示。
pub fn decode_pua(font: &str, ch: char) -> Option<char> {
    let code = ch as u32;
    (0xF000..=0xF0FF).contains(&code).then(|| decode(font, code)).flatten()
}

/// Adobe Symbol → Unicode（`SYMBOL.TXT`）。
pub const SYMBOL: &[(u8, char)] = &[
    (0x22, '∀'),
    (0x24, '∃'),
    (0x27, '∍'),
    (0x2A, '∗'),
    (0x2D, '−'),
    (0x40, '≅'),
    (0x41, 'Α'),
    (0x42, 'Β'),
    (0x43, 'Χ'),
    (0x44, 'Δ'),
    (0x45, 'Ε'),
    (0x46, 'Φ'),
    (0x47, 'Γ'),
    (0x48, 'Η'),
    (0x49, 'Ι'),
    (0x4A, 'ϑ'),
    (0x4B, 'Κ'),
    (0x4C, 'Λ'),
    (0x4D, 'Μ'),
    (0x4E, 'Ν'),
    (0x4F, 'Ο'),
    (0x50, 'Π'),
    (0x51, 'Θ'),
    (0x52, 'Ρ'),
    (0x53, 'Σ'),
    (0x54, 'Τ'),
    (0x55, 'Υ'),
    (0x56, 'ς'),
    (0x57, 'Ω'),
    (0x58, 'Ξ'),
    (0x59, 'Ψ'),
    (0x5A, 'Ζ'),
    (0x5C, '∴'),
    (0x5E, '⊥'),
    (0x60, '‾'),
    (0x61, 'α'),
    (0x62, 'β'),
    (0x63, 'χ'),
    (0x64, 'δ'),
    (0x65, 'ε'),
    (0x66, 'φ'),
    (0x67, 'γ'),
    (0x68, 'η'),
    (0x69, 'ι'),
    (0x6A, 'ϕ'),
    (0x6B, 'κ'),
    (0x6C, 'λ'),
    (0x6D, 'μ'),
    (0x6E, 'ν'),
    (0x6F, 'ο'),
    (0x70, 'π'),
    (0x71, 'θ'),
    (0x72, 'ρ'),
    (0x73, 'σ'),
    (0x74, 'τ'),
    (0x75, 'υ'),
    (0x76, 'ϖ'),
    (0x77, 'ω'),
    (0x78, 'ξ'),
    (0x79, 'ψ'),
    (0x7A, 'ζ'),
    (0x7E, '∼'),
    (0xA1, 'ϒ'),
    (0xA2, '′'),
    (0xA3, '≤'),
    (0xA4, '⁄'),
    (0xA5, '∞'),
    (0xA6, 'ƒ'),
    (0xA7, '♣'),
    (0xA8, '♦'),
    (0xA9, '♥'),
    (0xAA, '♠'),
    (0xAB, '↔'),
    (0xAC, '←'),
    (0xAD, '↑'),
    (0xAE, '→'),
    (0xAF, '↓'),
    (0xB0, '°'),
    (0xB1, '±'),
    (0xB2, '″'),
    (0xB3, '≥'),
    (0xB4, '×'),
    (0xB5, '∝'),
    (0xB6, '∂'),
    (0xB7, '•'),
    (0xB8, '÷'),
    (0xB9, '≠'),
    (0xBA, '≡'),
    (0xBB, '≈'),
    (0xBC, '…'),
    (0xBF, '↵'),
    (0xC0, 'ℵ'),
    (0xC1, 'ℑ'),
    (0xC2, 'ℜ'),
    (0xC3, '℘'),
    (0xC4, '⊗'),
    (0xC5, '⊕'),
    (0xC6, '∅'),
    (0xC7, '∩'),
    (0xC8, '∪'),
    (0xC9, '⊃'),
    (0xCA, '⊇'),
    (0xCB, '⊄'),
    (0xCC, '⊂'),
    (0xCD, '⊆'),
    (0xCE, '∈'),
    (0xCF, '∉'),
    (0xD0, '∠'),
    (0xD1, '∇'),
    (0xD2, '®'),
    (0xD3, '©'),
    (0xD4, '™'),
    (0xD5, '∏'),
    (0xD6, '√'),
    (0xD7, '⋅'),
    (0xD8, '¬'),
    (0xD9, '∧'),
    (0xDA, '∨'),
    (0xDB, '⇔'),
    (0xDC, '⇐'),
    (0xDD, '⇑'),
    (0xDE, '⇒'),
    (0xDF, '⇓'),
    (0xE0, '◊'),
    (0xE1, '⟨'),
    (0xE2, '®'),
    (0xE3, '©'),
    (0xE4, '™'),
    (0xE5, '∑'),
    (0xF1, '⟩'),
    (0xF2, '∫'),
    (0xF3, '⌠'),
    (0xF5, '⌡'),
];

/// Wingdings → Unicode（常用字形；表外码位不解码，见模块头）。
pub const WINGDINGS: &[(u8, char)] = &[
    (0x21, '✏'),
    (0x22, '✂'),
    (0x28, '☎'),
    (0x2A, '✉'),
    (0x4A, '☺'),
    (0x4B, '😐'),
    (0x4C, '☹'),
    (0x6C, '●'),
    (0x6D, '❍'),
    (0x6E, '■'),
    (0x71, '❑'),
    (0x75, '◆'),
    (0xA7, '▪'),
    (0xFC, '✓'),
    (0xFD, '✔'),
    (0xFE, '✗'),
    (0xFF, '✘'),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `RES-05`：`w:sym` 的 `0xF000` 偏移写法与裸码位都要认。
    #[test]
    fn res_05_decodes_both_spellings() {
        assert_eq!(decode("Wingdings", 0xF0FC), Some('✓'));
        assert_eq!(decode("Wingdings", 0xFC), Some('✓'));
        assert_eq!(decode("wingdings", 0x6C), Some('●'));
        assert_eq!(decode("Symbol", 0xF0B7), Some('•'));
        assert_eq!(decode("Symbol", 0x61), Some('α'));
    }

    /// 表外码位与非符号字体解码失败（调用方按 `RES-05` 保留原字符）。
    #[test]
    fn res_05_unknown_code_points_stay_undecoded() {
        assert_eq!(decode("Wingdings 2", 0xF045), None, "语料 symbol-fonts__002：TS 也没解码");
        assert_eq!(decode("Wingdings", 0x01), None);
        assert_eq!(decode("Times New Roman", 0x41), None);
    }

    /// 符号字体 run 里只解码 PUA 区间：普通 ASCII 字母不动（语料 `symbol-fonts__004`）。
    #[test]
    fn res_05_only_pua_text_is_decoded() {
        assert_eq!(decode_pua("Wingdings", '\u{F0FC}'), Some('✓'));
        assert_eq!(decode_pua("Wingdings", 'l'), None);
        assert_eq!(decode_pua("Symbol", '\u{F0B7}'), Some('•'));
    }

    #[test]
    fn res_05_symbol_font_names() {
        for n in ["Symbol", "wingdings", "Wingdings 2", "Wingdings 3", "Webdings"] {
            assert!(is_symbol_font(n), "{n}");
        }
        assert!(!is_symbol_font("Arial"));
    }
}
