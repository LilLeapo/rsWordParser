//! 主题声明值（`MOD-10`）：`a:theme/a:themeElements` 的字体方案与颜色方案。
//! 只记录声明；主题字体 / 颜色的解析规则（槽位映射、tint/shade、空 EA 槽）在 `RES-05`。

use crate::semantic::props::{HexColorOrAuto, ThemeColor};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 颜色方案的 12 个槽位（`a:clrScheme` 子元素名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThemeSlot {
    Dk1,
    Lt1,
    Dk2,
    Lt2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hlink,
    FolHlink,
}

impl ThemeSlot {
    pub const ALL: [ThemeSlot; 12] = [
        ThemeSlot::Dk1,
        ThemeSlot::Lt1,
        ThemeSlot::Dk2,
        ThemeSlot::Lt2,
        ThemeSlot::Accent1,
        ThemeSlot::Accent2,
        ThemeSlot::Accent3,
        ThemeSlot::Accent4,
        ThemeSlot::Accent5,
        ThemeSlot::Accent6,
        ThemeSlot::Hlink,
        ThemeSlot::FolHlink,
    ];

    /// `a:clrScheme` 里的元素局部名。
    pub const fn local(self) -> LocalName {
        match self {
            ThemeSlot::Dk1 => LocalName::Dk1,
            ThemeSlot::Lt1 => LocalName::Lt1,
            ThemeSlot::Dk2 => LocalName::Dk2,
            ThemeSlot::Lt2 => LocalName::Lt2,
            ThemeSlot::Accent1 => LocalName::Accent1,
            ThemeSlot::Accent2 => LocalName::Accent2,
            ThemeSlot::Accent3 => LocalName::Accent3,
            ThemeSlot::Accent4 => LocalName::Accent4,
            ThemeSlot::Accent5 => LocalName::Accent5,
            ThemeSlot::Accent6 => LocalName::Accent6,
            ThemeSlot::Hlink => LocalName::Hlink,
            ThemeSlot::FolHlink => LocalName::FolHlink,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ThemeSlot::Dk1 => "dk1",
            ThemeSlot::Lt1 => "lt1",
            ThemeSlot::Dk2 => "dk2",
            ThemeSlot::Lt2 => "lt2",
            ThemeSlot::Accent1 => "accent1",
            ThemeSlot::Accent2 => "accent2",
            ThemeSlot::Accent3 => "accent3",
            ThemeSlot::Accent4 => "accent4",
            ThemeSlot::Accent5 => "accent5",
            ThemeSlot::Accent6 => "accent6",
            ThemeSlot::Hlink => "hlink",
            ThemeSlot::FolHlink => "folHlink",
        }
    }

    /// `w:themeColor` 的槽位映射（`RES-05`）：`dark1/text1 → dk1` 等；`none` 无槽位。
    pub const fn from_theme_color(c: ThemeColor) -> Option<ThemeSlot> {
        Some(match c {
            ThemeColor::Dark1 | ThemeColor::Text1 => ThemeSlot::Dk1,
            ThemeColor::Light1 | ThemeColor::Background1 => ThemeSlot::Lt1,
            ThemeColor::Dark2 | ThemeColor::Text2 => ThemeSlot::Dk2,
            ThemeColor::Light2 | ThemeColor::Background2 => ThemeSlot::Lt2,
            ThemeColor::Accent1 => ThemeSlot::Accent1,
            ThemeColor::Accent2 => ThemeSlot::Accent2,
            ThemeColor::Accent3 => ThemeSlot::Accent3,
            ThemeColor::Accent4 => ThemeSlot::Accent4,
            ThemeColor::Accent5 => ThemeSlot::Accent5,
            ThemeColor::Accent6 => ThemeSlot::Accent6,
            ThemeColor::Hyperlink => ThemeSlot::Hlink,
            ThemeColor::FollowedHyperlink => ThemeSlot::FolHlink,
            ThemeColor::None => return None,
        })
    }

    /// DrawingML `a:schemeClr/@val` 的名字（含别名 `tx1→dk1, bg1→lt1, tx2→dk2, bg2→lt2`）。
    pub fn from_scheme_name(s: &str) -> Option<ThemeSlot> {
        Some(match s {
            "dk1" | "tx1" => ThemeSlot::Dk1,
            "lt1" | "bg1" => ThemeSlot::Lt1,
            "dk2" | "tx2" => ThemeSlot::Dk2,
            "lt2" | "bg2" => ThemeSlot::Lt2,
            "accent1" => ThemeSlot::Accent1,
            "accent2" => ThemeSlot::Accent2,
            "accent3" => ThemeSlot::Accent3,
            "accent4" => ThemeSlot::Accent4,
            "accent5" => ThemeSlot::Accent5,
            "accent6" => ThemeSlot::Accent6,
            "hlink" => ThemeSlot::Hlink,
            "folHlink" => ThemeSlot::FolHlink,
            _ => return None,
        })
    }
}

/// 一组字体（`a:majorFont` / `a:minorFont`）：三个脚本槽位与 `a:font script→typeface` 表。空串视为无。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontSlots {
    pub node: Option<NodeId>,
    pub latin: Option<String>,
    pub ea: Option<String>,
    pub cs: Option<String>,
    /// `(script, typeface)`，按出现顺序（`Jpan`、`Hang`、`Hans`、`Hant`……）。
    pub scripts: Vec<(String, String)>,
}

impl FontSlots {
    pub fn script(&self, script: &str) -> Option<&str> {
        self.scripts.iter().find(|(s, _)| s == script).map(|(_, t)| t.as_str())
    }

    fn read(dom: &Dom, node: NodeId) -> FontSlots {
        let mut out = FontSlots { node: Some(node), ..Default::default() };
        for child in dom.semantic_children(node) {
            let Some(name) = dom.name(child) else { continue };
            if name.ns != NsId::A {
                continue;
            }
            let typeface = dom
                .attr_value(child, QName::new(NsId::None, LocalName::Typeface))
                .map(|s| s.into_owned())
                .filter(|s| !s.is_empty());
            match name.local {
                LocalName::Latin => out.latin = typeface,
                LocalName::Ea => out.ea = typeface,
                LocalName::Cs => out.cs = typeface,
                LocalName::Font => {
                    if let (Some(script), Some(t)) =
                        (dom.attr_value(child, QName::new(NsId::None, LocalName::Script)), typeface)
                    {
                        out.scripts.push((script.into_owned(), t));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// `a:fontScheme`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontScheme {
    pub node: NodeId,
    pub name: Option<String>,
    pub major: FontSlots,
    pub minor: FontSlots,
}

/// `a:clrScheme`：12 个槽位的 sRGB（`a:srgbClr/@val`，或 `a:sysClr/@lastClr`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    /// 内建调色板（[`ColorScheme::office_default`]）没有节点。
    pub node: Option<NodeId>,
    pub name: Option<String>,
    colors: [Option<[u8; 3]>; 12],
}

/// Word 内建 Office 调色板：文档没有 theme part 时，`schemeClr` / `themeColor` 仍按它解析
/// （`RES-05`；TS `DEFAULT_THEME_COLORS`）。顺序同 [`ThemeSlot::ALL`]。
pub const OFFICE_DEFAULT_COLORS: [[u8; 3]; 12] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x44, 0x54, 0x6A],
    [0xE7, 0xE6, 0xE6],
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
    [0x05, 0x63, 0xC1],
    [0x95, 0x4F, 0x72],
];

impl ColorScheme {
    /// 内建 Office 调色板。
    pub fn office_default() -> ColorScheme {
        ColorScheme {
            node: None,
            name: Some("Office".into()),
            colors: OFFICE_DEFAULT_COLORS.map(Some),
        }
    }

    pub fn get(&self, slot: ThemeSlot) -> Option<[u8; 3]> {
        self.colors[slot as usize]
    }

    /// 缺省值：`dk1 → 000000`、`lt1 → FFFFFF`（`RES-05`），其余槽位无缺省。
    pub fn get_or_default(&self, slot: ThemeSlot) -> Option<[u8; 3]> {
        self.get(slot).or(match slot {
            ThemeSlot::Dk1 => Some([0, 0, 0]),
            ThemeSlot::Lt1 => Some([0xFF, 0xFF, 0xFF]),
            _ => None,
        })
    }

    fn read(dom: &Dom, node: NodeId) -> ColorScheme {
        let mut colors = [None; 12];
        for child in dom.semantic_children(node) {
            let Some(name) = dom.name(child) else { continue };
            if name.ns != NsId::A {
                continue;
            }
            let Some(slot) = ThemeSlot::ALL.iter().copied().find(|s| s.local() == name.local)
            else {
                continue;
            };
            colors[slot as usize] = read_color(dom, child);
        }
        ColorScheme { node: Some(node), name: attr_name(dom, node), colors }
    }
}

/// 颜色槽位下第一个 `a:srgbClr`（取 `val`）或 `a:sysClr`（取 `lastClr`）。
fn read_color(dom: &Dom, slot: NodeId) -> Option<[u8; 3]> {
    for c in dom.semantic_children(slot) {
        let Some(name) = dom.name(c) else { continue };
        let attr = match (name.ns, name.local) {
            (NsId::A, LocalName::SrgbClr) => LocalName::Val,
            (NsId::A, LocalName::SysClr) => LocalName::LastClr,
            _ => continue,
        };
        let text = dom.attr_value(c, QName::new(NsId::None, attr))?;
        return HexColorOrAuto::parse(&text).and_then(HexColorOrAuto::rgb);
    }
    None
}

fn attr_name(dom: &Dom, node: NodeId) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, LocalName::Name)).map(|s| s.into_owned())
}

/// `a:theme` 的声明值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub node: NodeId,
    pub name: Option<String>,
    pub fonts: Option<FontScheme>,
    pub colors: Option<ColorScheme>,
}

impl Theme {
    /// 根须是 `a:theme`，否则 `None`。缺 `a:themeElements` 时两个方案都为 `None`。
    pub fn from_dom(dom: &Dom) -> Option<Theme> {
        let root = dom.root();
        if !dom.is(root, QName::new(NsId::A, LocalName::Theme)) {
            return None;
        }
        let mut theme = Theme { node: root, name: attr_name(dom, root), fonts: None, colors: None };
        let elements = dom
            .semantic_children(root)
            .find(|&n| dom.is(n, QName::new(NsId::A, LocalName::ThemeElements)));
        let Some(elements) = elements else { return Some(theme) };
        for child in dom.semantic_children(elements) {
            let Some(name) = dom.name(child) else { continue };
            match (name.ns, name.local) {
                (NsId::A, LocalName::ClrScheme) if theme.colors.is_none() => {
                    theme.colors = Some(ColorScheme::read(dom, child));
                }
                (NsId::A, LocalName::FontScheme) if theme.fonts.is_none() => {
                    let mut major = FontSlots::default();
                    let mut minor = FontSlots::default();
                    for g in dom.semantic_children(child) {
                        match dom.name(g).map(|q| (q.ns, q.local)) {
                            Some((NsId::A, LocalName::MajorFont)) => {
                                major = FontSlots::read(dom, g)
                            }
                            Some((NsId::A, LocalName::MinorFont)) => {
                                minor = FontSlots::read(dom, g)
                            }
                            _ => {}
                        }
                    }
                    theme.fonts =
                        Some(FontScheme { node: child, name: attr_name(dom, child), major, minor });
                }
                _ => {}
            }
        }
        Some(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    #[test]
    fn mod_10_theme_fonts_and_colors() {
        let xml = format!(
            r#"<a:theme xmlns:a="{A}" name="Office Theme"><a:themeElements>
              <a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
                <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2>
                <a:accent1><a:srgbClr val="4472c4"/></a:accent1><a:hlink><a:srgbClr val="0563C1"/></a:hlink></a:clrScheme>
              <a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/>
                <a:font script="Jpan" typeface="游ゴシック Light"/><a:font script="Hans" typeface="等线 Light"/></a:majorFont>
                <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface="宋体"/><a:cs typeface="Arial"/></a:minorFont></a:fontScheme>
            </a:themeElements></a:theme>"#
        );
        let dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
        let t = Theme::from_dom(&dom).unwrap();
        assert_eq!(t.name.as_deref(), Some("Office Theme"));
        let c = t.colors.as_ref().unwrap();
        assert_eq!(c.name.as_deref(), Some("Office"));
        assert_eq!(c.get(ThemeSlot::Dk1), Some([0, 0, 0]));
        assert_eq!(c.get(ThemeSlot::Dk2), Some([0x44, 0x54, 0x6A]));
        assert_eq!(c.get(ThemeSlot::Accent1), Some([0x44, 0x72, 0xC4]));
        assert_eq!(c.get(ThemeSlot::Accent2), None);
        assert_eq!(c.get(ThemeSlot::Lt2), None);
        assert_eq!(c.get_or_default(ThemeSlot::Lt1), Some([0xFF, 0xFF, 0xFF]));
        let f = t.fonts.as_ref().unwrap();
        assert_eq!(f.major.latin.as_deref(), Some("Calibri Light"));
        assert_eq!(f.major.ea, None, "空串视为无");
        assert_eq!(f.major.script("Hans"), Some("等线 Light"));
        assert_eq!(f.minor.ea.as_deref(), Some("宋体"));
        assert_eq!(f.minor.cs.as_deref(), Some("Arial"));
        assert_eq!(ThemeSlot::from_theme_color(ThemeColor::Text1), Some(ThemeSlot::Dk1));
        assert_eq!(ThemeSlot::from_theme_color(ThemeColor::None), None);
        assert_eq!(ThemeSlot::from_scheme_name("bg2"), Some(ThemeSlot::Lt2));
        let office = ColorScheme::office_default();
        assert_eq!(office.get(ThemeSlot::Accent1), Some([0x44, 0x72, 0xC4]));
        assert_eq!(office.get(ThemeSlot::FolHlink), Some([0x95, 0x4F, 0x72]));
        assert!(office.node.is_none());
    }

    #[test]
    fn mod_10_theme_wrong_root_is_none() {
        let dom = Dom::parse(PartId(0), format!(r#"<a:foo xmlns:a="{A}"/>"#).as_bytes()).unwrap();
        assert!(Theme::from_dom(&dom).is_none());
    }
}
