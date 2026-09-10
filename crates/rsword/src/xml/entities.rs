//! 实体与字符引用（`XML-06`）。
//!
//! 解码只在从 `Raw` 取值时发生一次；`Owned` 值视为已解码。写回 `Owned` 时转义 `& < >`
//! （属性值再按引号转义 `"` 或 `'`），并去除 XML 1.0 非法控制字符。

use std::borrow::Cow;

/// Borrowed text for generated formula and chart XML fragments.
///
/// Preserves the TS `escapeXmlText` contract: escape `& < >` and discard forbidden
/// C0 controls. Tabs, line breaks, and U+FFFE/U+FFFF retain their original behavior;
/// the general XML serializer performs its own stricter character validation.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct FragmentText<'a>(&'a str);

impl<'a> From<&'a str> for FragmentText<'a> {
    #[inline]
    fn from(value: &'a str) -> Self {
        Self(value)
    }
}

impl From<FragmentText<'_>> for String {
    #[inline]
    fn from(value: FragmentText<'_>) -> Self {
        let mut out = String::with_capacity(value.0.len());
        for ch in value.0.chars() {
            match ch {
                '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' => {}
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                c => out.push(c),
            }
        }
        out
    }
}

/// 解码失败的位置（相对输入串的字节偏移）与原因；原文保留。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadEntity {
    pub offset: usize,
    pub raw: String,
}

/// 解析 `&…;`，`s[0] == '&'`。返回 (解码字符, 消耗字节数)。
fn parse_ref(s: &str) -> Option<(char, usize)> {
    let semi = s[1..].find(';')? + 1;
    if semi > 12 {
        return None;
    }
    let body = &s[1..semi];
    let c = match body {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let digits = body.strip_prefix('#')?;
            let cp = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
                if hex.is_empty() {
                    return None;
                }
                u32::from_str_radix(hex, 16).ok()?
            } else {
                if digits.is_empty() {
                    return None;
                }
                digits.parse::<u32>().ok()?
            };
            // 代理区与超范围码点非法；U+0000 也非法
            if cp == 0 {
                return None;
            }
            char::from_u32(cp)?
        }
    };
    Some((c, semi + 1))
}

/// 解码文本或属性值中的实体。无 `&` 时零拷贝。
pub fn decode(s: &str) -> Cow<'_, str> {
    let Some(first) = s.find('&') else { return Cow::Borrowed(s) };
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..first]);
    let mut rest = &s[first..];
    loop {
        // rest 以 '&' 开头
        match parse_ref(rest) {
            Some((c, n)) => {
                out.push(c);
                rest = &rest[n..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
        match rest.find('&') {
            Some(i) => {
                out.push_str(&rest[..i]);
                rest = &rest[i..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    Cow::Owned(out)
}

/// 只检查不解码：返回第一个非法引用（用于解析期诊断 `XML_BAD_ENTITY`）。
pub fn first_bad(s: &str) -> Option<BadEntity> {
    let mut from = 0;
    while let Some(i) = s[from..].find('&') {
        let at = from + i;
        match parse_ref(&s[at..]) {
            Some((_, n)) => from = at + n,
            None => {
                let mut end = s[at..].find(';').map_or(s.len(), |k| (at + k + 1).min(s.len()));
                end = end.min(at + 16);
                // 截断点必须落在字符边界上（fuzz_xml 发现：`&` 后紧跟多字节字符时切进字符中间会 panic）
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                return Some(BadEntity { offset: at, raw: s[at..end].to_string() });
            }
        }
    }
    None
}

/// XML 1.0 非法字符：C0 控制字符除 TAB/LF/CR，以及 U+FFFE/U+FFFF。
fn is_illegal(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// 文本节点转义：`& < >`。
pub fn escape_text(s: &str, out: &mut Vec<u8>) {
    for c in s.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            c if is_illegal(c) => {}
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

/// 文本节点转义，直接给 `String`（拼 XML 片段的生成器用；同 TS `escapeXmlText`）。
pub fn escaped_text(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    escape_text(s, &mut out);
    String::from_utf8(out).expect("escape_text 只产出合法 UTF-8")
}

/// 双引号属性值转义，直接给 `String`（拼 XML 片段的生成器用）。
pub fn escaped_attr(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    escape_attr(s, b'"', &mut out);
    String::from_utf8(out).expect("escape_attr 只产出合法 UTF-8")
}

/// 属性值转义：`& < >` 加上所用引号。
pub fn escape_attr(s: &str, quote: u8, out: &mut Vec<u8>) {
    for c in s.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '"' if quote == b'"' => out.extend_from_slice(b"&quot;"),
            '\'' if quote == b'\'' => out.extend_from_slice(b"&apos;"),
            '\t' => out.extend_from_slice(b"&#9;"),
            '\n' => out.extend_from_slice(b"&#10;"),
            '\r' => out.extend_from_slice(b"&#13;"),
            c if is_illegal(c) => {}
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_06_decode_once() {
        assert_eq!(decode("a&amp;lt;b"), "a&lt;b");
        assert_eq!(decode("&lt;&gt;&quot;&apos;&amp;"), "<>\"'&");
        assert_eq!(decode("&#65;&#x42;&#x1F600;"), "AB😀");
        assert!(matches!(decode("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn xml_06_bad_entities_kept_verbatim() {
        assert_eq!(decode("x &foo; y & z &#xD800; &#; &#x;"), "x &foo; y & z &#xD800; &#; &#x;");
        let bad = first_bad("ok &amp; then &nope; end").unwrap();
        assert_eq!(bad.offset, 14);
        assert_eq!(bad.raw, "&nope;");
        assert!(first_bad("&lt;&#10;").is_none());
        assert_eq!(first_bad("a & b").unwrap().raw, "& b");
    }

    #[test]
    fn xml_06_bad_entity_snippet_respects_char_boundaries() {
        // fuzz_xml 回归：`&` 后 16 字节内含多字节字符
        let s = "&ééééééééééééééé";
        let bad = first_bad(s).unwrap();
        assert_eq!(bad.offset, 0);
        assert!(s.is_char_boundary(bad.raw.len()));
        assert!(first_bad("&😀😀😀😀😀😀😀😀").is_some());
        assert_eq!(decode("&😀"), "&😀");
    }

    /// Generated fragments preserve their established C0-only filtering contract.
    #[test]
    fn fragment_text_preserves_character_and_entity_contract() {
        let input = String::from("α<&>\"'\0\u{8}\u{b}\u{c}\u{e}\u{1f}\t\n\r\u{fffe}\u{ffff}");
        assert_eq!(
            String::from(super::FragmentText::from(input.as_str())),
            "α&lt;&amp;&gt;\"'\t\n\r\u{fffe}\u{ffff}"
        );
        assert_eq!(String::from(super::FragmentText::from("&amp;")), "&amp;amp;");
        assert_eq!(String::from(super::FragmentText::from("")), "");
        assert_eq!(super::escaped_text("\u{fffe}\u{ffff}"), "");
    }

    #[test]
    fn xml_06_escape_text_and_attr() {
        let mut out = Vec::new();
        escape_text("a<b>&c\u{1}", &mut out);
        assert_eq!(out, b"a&lt;b&gt;&amp;c");
        out.clear();
        escape_attr("say \"hi\" 'yo'", b'"', &mut out);
        assert_eq!(out, b"say &quot;hi&quot; 'yo'");
        out.clear();
        escape_attr("say \"hi\" 'yo'\n", b'\'', &mut out);
        assert_eq!(out, b"say \"hi\" &apos;yo&apos;&#10;");
    }
}
