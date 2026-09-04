//! 声明模型（`MOD-10`，任务 1.4）：styles / numbering / settings / fontTable 的入口与查找辅助。
//!
//! 类型本身由属性表生成（`schema/props/{styles,numbering,settings,font_table}.toml`），
//! 这里只加"从 part 读取"和只读查找；样式链、编号覆盖合并等解释在 `resolve`。

use crate::diag::Diagnostic;
pub use crate::semantic::props::{
    AbstractNum, Compat, CompatSetting, DocDefaults, Font, FontTable, Level, LevelOverride, Num,
    Numbering, ParaProps, RunProps, Settings, Style, StyleType, TableStylePr, TblStyleOverrideType,
};
use crate::semantic::props::{
    Val, codec::OnOff, read_font_table, read_numbering, read_settings, read_styles,
};
use crate::xml::{Dom, LocalName, NodeId, QName};

fn root_if(dom: &Dom, name: QName) -> Option<NodeId> {
    let root = dom.root();
    dom.is(root, name).then_some(root)
}

fn val_i32(v: &Option<Val<i32>>) -> Option<i32> {
    v.as_ref().and_then(|x| x.value().copied())
}

// ---- Styles -------------------------------------------------------------------------------------

impl Styles {
    /// 根须是 `w:styles`，否则 `None`。
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Styles> {
        let root = root_if(dom, QName::w(LocalName::Styles))?;
        Some(read_styles(dom, Some(root), diags))
    }

    /// 按 `styleId` 查找（第一个匹配）。
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.id() == Some(id))
    }

    /// 某类型的默认样式：该类型最后一个 `w:default="1|true"`；没有声明的 → 该类型中 styleId 或
    /// name 为 `Normal`（不分大小写）的第一个；再没有 → `None`（只剩 docDefaults）。
    ///
    /// 与 `RES-02` 引用的 ECMA-376 §17.7.4.17 "取该类型第一个样式"不同：Word 实测不用
    /// first-of-type 规则（TS `parseStyles` 注释与差分语料），这里按 Word 行为。
    pub fn default_for(&self, kind: StyleType) -> Option<&Style> {
        let of_kind = || self.styles.iter().filter(move |s| s.kind() == Some(kind));
        of_kind().rfind(|s| s.is_default == Some(true)).or_else(|| {
            of_kind().find(|s| {
                s.id().is_some_and(|i| i.eq_ignore_ascii_case("normal"))
                    || s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case("normal"))
            })
        })
    }

    pub fn doc_default_rpr(&self) -> Option<&RunProps> {
        self.doc_defaults.as_ref()?.rpr_default.as_ref()?.rpr.as_ref()
    }

    pub fn doc_default_ppr(&self) -> Option<&ParaProps> {
        self.doc_defaults.as_ref()?.ppr_default.as_ref()?.ppr.as_ref()
    }

    /// 段落样式自身给出的标题级别；不沿 basedOn 继承（`RES-02` 做继承）。
    pub fn own_heading_level(style: &Style) -> OwnHeadingLevel {
        if let Some(l) = style.name.as_deref().and_then(heading_level_of_name) {
            return OwnHeadingLevel::Level(l);
        }
        if let Some(l) = style.id().and_then(heading_level_of_id) {
            return OwnHeadingLevel::Level(l);
        }
        match style.ppr.as_ref().and_then(|p| val_i32(&p.outline_lvl)) {
            Some(l @ 0..=8) => OwnHeadingLevel::Level(l as u8 + 1),
            Some(_) => OwnHeadingLevel::Blocked,
            None => OwnHeadingLevel::Inherit,
        }
    }
}

/// [`Styles::own_heading_level`] 的结果（`RES-02` heading_level）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnHeadingLevel {
    /// 名字 / id 匹配 `heading N`，或 `outlineLvl` 0–8。
    Level(u8),
    /// `outlineLvl == 9`：正文级，阻断 basedOn 继承（`TOCHeading basedOn Heading1`）。
    Blocked,
    /// 未指定，沿 basedOn 继承。
    Inherit,
}

/// `/^heading\s*([1-9])$/i`
fn heading_level_of_name(name: &str) -> Option<u8> {
    let rest = name.get(..7).filter(|p| p.eq_ignore_ascii_case("heading")).map(|_| &name[7..])?;
    let rest = rest.trim_start();
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !('1'..='9').contains(&d) {
        return None;
    }
    Some(d as u8 - b'0')
}

/// `/^Heading([1-9])$/`
fn heading_level_of_id(id: &str) -> Option<u8> {
    let rest = id.strip_prefix("Heading")?;
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !('1'..='9').contains(&d) {
        return None;
    }
    Some(d as u8 - b'0')
}

impl Style {
    pub fn id(&self) -> Option<&str> {
        self.style_id.as_deref()
    }

    pub fn kind(&self) -> Option<StyleType> {
        self.kind.as_ref().and_then(|v| v.value().copied())
    }

    /// 显示名；缺省用 styleId（TS 行为）。
    pub fn display_name(&self) -> Option<&str> {
        self.name.as_deref().or(self.id())
    }

    pub fn is_default_flag(&self) -> bool {
        self.is_default == Some(true)
    }
}

// ---- Numbering ----------------------------------------------------------------------------------

impl Numbering {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Numbering> {
        let root = root_if(dom, QName::w(LocalName::Numbering))?;
        Some(read_numbering(dom, Some(root), diags))
    }

    pub fn abstract_num(&self, id: i32) -> Option<&AbstractNum> {
        self.abstract_nums.iter().find(|a| val_i32(&a.abstract_num_id) == Some(id))
    }

    pub fn num(&self, num_id: i32) -> Option<&Num> {
        self.nums.iter().find(|n| val_i32(&n.num_id) == Some(num_id))
    }
}

impl AbstractNum {
    pub fn id(&self) -> Option<i32> {
        val_i32(&self.abstract_num_id)
    }

    pub fn level(&self, ilvl: i32) -> Option<&Level> {
        self.levels.iter().find(|l| l.ilvl() == Some(ilvl))
    }
}

impl Level {
    pub fn ilvl(&self) -> Option<i32> {
        val_i32(&self.ilvl)
    }

    /// `w:start`；缺省 0（ECMA-376 §17.9.25；Word 显示 "0."）。
    pub fn start_or_default(&self) -> i32 {
        val_i32(&self.start).unwrap_or(0)
    }
}

impl Num {
    pub fn id(&self) -> Option<i32> {
        val_i32(&self.num_id)
    }

    pub fn abstract_id(&self) -> Option<i32> {
        val_i32(&self.abstract_num_id)
    }

    pub fn override_for(&self, ilvl: i32) -> Option<&LevelOverride> {
        self.overrides.iter().find(|o| val_i32(&o.ilvl) == Some(ilvl))
    }
}

// ---- Settings -----------------------------------------------------------------------------------

/// 兼容事实（`docs/03` §6.5）：只记录，`resolve` 与布局层解释。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatFacts {
    /// `compatSetting[name=compatibilityMode]/@val`。
    pub mode: Option<u32>,
    pub settings: Vec<CompatSetting>,
    /// `w:compat` 下值为真的布尔子元素。
    pub flags: Vec<QName>,
}

impl Settings {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Settings> {
        let root = root_if(dom, QName::w(LocalName::Settings))?;
        Some(read_settings(dom, Some(root), diags))
    }

    pub fn compat_facts(&self, dom: &Dom) -> CompatFacts {
        let Some(compat) = &self.compat else { return CompatFacts::default() };
        let settings = compat.settings.clone();
        let mode = settings
            .iter()
            .find(|s| s.name.as_deref() == Some("compatibilityMode"))
            .and_then(|s| s.val.as_deref()?.trim().parse().ok());
        let mut flags = Vec::new();
        let mut sink = Vec::new();
        let mut ctx = crate::semantic::props::Ctx::new(dom, &mut sink);
        for &n in &compat.raw_unmodeled {
            let Some(name) = dom.name(n) else { continue };
            ctx.enter(n);
            let on = match dom.attr_value(n, QName::w(LocalName::Val)) {
                Some(v) => <OnOff as crate::semantic::props::Codec>::parse(&v, &mut ctx),
                None => true,
            };
            if on {
                flags.push(name);
            }
        }
        CompatFacts { mode, settings, flags }
    }

    pub fn compatibility_mode(&self, dom: &Dom) -> Option<u32> {
        self.compat_facts(dom).mode
    }

    /// `w:defaultTabStop`，twip；缺省 720（Word）。
    pub fn default_tab_stop_or_default(&self) -> i32 {
        val_i32(&self.default_tab_stop).unwrap_or(720)
    }
}

// ---- FontTable ----------------------------------------------------------------------------------

impl FontTable {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<FontTable> {
        let root = root_if(dom, QName::w(LocalName::Fonts))?;
        Some(read_font_table(dom, Some(root), diags))
    }

    pub fn get(&self, name: &str) -> Option<&Font> {
        self.fonts.iter().find(|f| f.name.as_deref() == Some(name))
    }
}

pub use crate::semantic::props::Styles;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;
    use crate::semantic::props::{
        CharacterSpacing, DocProtect, FontFamily, FontPitch, Jc, MultiLevelType, NumberFormat,
    };

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn dom(xml: &str) -> Dom {
        Dom::parse(PartId(0), xml.as_bytes()).unwrap()
    }

    #[test]
    fn mod_10_styles_defaults_and_heading_levels() {
        let d = dom(&format!(
            r#"<w:styles xmlns:w="{W}">
              <w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault>
                <w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
              <w:latentStyles w:count="1"><w:lsdException w:name="Normal"/></w:latentStyles>
              <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
              <w:style w:type="paragraph" w:default="1" w:styleId="Body"><w:name w:val="Body Text"/><w:pPr><w:jc w:val="both"/></w:pPr></w:style>
              <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:link w:val="Heading1Char"/>
                <w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
              <w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Heading1"/><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>
              <w:style w:type="paragraph" w:styleId="MyH"><w:name w:val="Custom"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
              <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:semiHidden/><w:unhideWhenUsed/></w:style>
              <w:style w:type="character" w:styleId="Heading1Char"><w:name w:val="Heading 1 Char"/><w:link w:val="Heading1"/><w:rPr><w:b/></w:rPr></w:style>
              <w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/></w:tblBorders></w:tblPr>
                <w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="D9D9D9"/></w:tcPr></w:tblStylePr></w:style>
            </w:styles>"#
        ));
        let mut diags = Vec::new();
        let s = Styles::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(s.styles.len(), 8);
        assert!(s.latent_styles.is_some());
        assert_eq!(s.doc_default_rpr().unwrap().size, Some(Val::Value(22)));
        assert_eq!(
            s.doc_default_ppr().unwrap().spacing.as_ref().unwrap().after,
            Some(Val::Value(160))
        );
        // 默认样式：最后一个 default 胜出；无声明取第一个
        assert_eq!(s.default_for(StyleType::Paragraph).unwrap().id(), Some("Body"));
        assert_eq!(s.default_for(StyleType::Character).unwrap().id(), Some("DefaultParagraphFont"));
        assert_eq!(
            s.default_for(StyleType::Table),
            None,
            "无声明且无 Normal → 无默认（Word 行为）"
        );
        assert_eq!(s.default_for(StyleType::Numbering), None);
        // 无声明时退到 Normal
        let d2 = dom(&format!(
            r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/></w:style>
               <w:style w:type="paragraph" w:styleId="a"><w:name w:val="Normal"/></w:style></w:styles>"#
        ));
        let s2 = Styles::from_dom(&d2, &mut Vec::new()).unwrap();
        assert_eq!(s2.default_for(StyleType::Paragraph).unwrap().id(), Some("a"));
        let h1 = s.get("Heading1").unwrap();
        assert_eq!(h1.kind(), Some(StyleType::Paragraph));
        assert_eq!(h1.based_on.as_deref(), Some("Normal"));
        assert_eq!(h1.link.as_deref(), Some("Heading1Char"));
        assert_eq!(h1.ui_priority, Some(Val::Value(9)));
        assert_eq!(h1.q_format, Some(true));
        assert_eq!(h1.ppr.as_ref().unwrap().keep_next, Some(true));
        assert_eq!(h1.rpr.as_ref().unwrap().size, Some(Val::Value(32)));
        assert_eq!(Styles::own_heading_level(h1), OwnHeadingLevel::Level(1));
        assert_eq!(
            Styles::own_heading_level(s.get("TOCHeading").unwrap()),
            OwnHeadingLevel::Blocked
        );
        assert_eq!(Styles::own_heading_level(s.get("MyH").unwrap()), OwnHeadingLevel::Level(3));
        assert_eq!(Styles::own_heading_level(s.get("Normal").unwrap()), OwnHeadingLevel::Inherit);
        assert_eq!(heading_level_of_name("Heading 3"), Some(3));
        assert_eq!(heading_level_of_name("heading3"), Some(3));
        assert_eq!(heading_level_of_name("Heading 10"), None);
        assert_eq!(heading_level_of_id("Heading9"), Some(9));
        assert_eq!(heading_level_of_id("Heading1Char"), None);
        let dpf = s.get("DefaultParagraphFont").unwrap();
        assert_eq!(dpf.semi_hidden, Some(true));
        assert_eq!(dpf.unhide_when_used, Some(true));
        let tg = s.get("TableGrid").unwrap();
        assert!(tg.tbl_pr.is_some());
        assert_eq!(tg.conditional.len(), 1);
        assert_eq!(tg.conditional[0].kind, Some(Val::Value(TblStyleOverrideType::FirstRow)));
        assert_eq!(tg.conditional[0].rpr.as_ref().unwrap().bold, Some(true));
        assert!(tg.conditional[0].tc_pr.is_some());
    }

    #[test]
    fn mod_10_numbering_declarations() {
        let d = dom(&format!(
            r#"<w:numbering xmlns:w="{W}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
                 xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14">
              <w:abstractNum w:abstractNumId="0"><w:nsid w:val="0ABC1234"/><w:multiLevelType w:val="hybridMultilevel"/>
                <w:lvl w:ilvl="0" w:tplc="04090001"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/><w:lvlJc w:val="left"/>
                  <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol" w:hint="default"/></w:rPr></w:lvl>
                <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/><w:lvlRestart w:val="0"/><w:isLgl/></w:lvl>
              </w:abstractNum>
              <w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="ListNumber"/>
                <w:lvl w:ilvl="0"><w:start w:val="1"/>
                  <mc:AlternateContent><mc:Choice Requires="w14"><w:numFmt w:val="custom" w:format="001, 002, 003, ..."/></mc:Choice><mc:Fallback><w:numFmt w:val="decimal"/></mc:Fallback></mc:AlternateContent>
                  <w:lvlText w:val="%1)"/></w:lvl>
              </w:abstractNum>
              <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
              <w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride>
                <w:lvlOverride w:ilvl="1"><w:lvl w:ilvl="1"><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%2"/></w:lvl></w:lvlOverride></w:num>
            </w:numbering>"#
        ));
        let mut diags = Vec::new();
        let n = Numbering::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(n.abstract_nums.len(), 2);
        assert_eq!(n.nums.len(), 2);
        let a0 = n.abstract_num(0).unwrap();
        assert_eq!(a0.nsid.as_deref(), Some("0ABC1234"));
        assert_eq!(a0.multi_level_type, Some(Val::Value(MultiLevelType::HybridMultilevel)));
        let l0 = a0.level(0).unwrap();
        assert_eq!(l0.tplc.as_deref(), Some("04090001"));
        assert_eq!(l0.start_or_default(), 1);
        assert_eq!(l0.num_fmt.as_ref().unwrap().val, Some(Val::Value(NumberFormat::Bullet)));
        assert_eq!(l0.lvl_text.as_ref().unwrap().val.as_deref(), Some("\u{F0B7}"));
        assert_eq!(l0.lvl_jc, Some(Val::Value(Jc::Left)));
        assert_eq!(
            l0.ppr.as_ref().unwrap().indent.as_ref().unwrap().hanging,
            Some(Val::Value(360))
        );
        assert_eq!(
            l0.rpr.as_ref().unwrap().fonts.as_ref().unwrap().ascii.as_deref(),
            Some("Symbol")
        );
        let l1 = a0.level(1).unwrap();
        assert_eq!(l1.start_or_default(), 0, "缺 w:start 从 0 起");
        assert_eq!(l1.lvl_restart, Some(Val::Value(0)));
        assert_eq!(l1.is_lgl, Some(true));
        // w14 自定义格式：MCE 选中 Choice 分支
        let a1 = n.abstract_num(1).unwrap();
        assert_eq!(a1.num_style_link.as_deref(), Some("ListNumber"));
        let f = a1.level(0).unwrap().num_fmt.as_ref().unwrap();
        assert_eq!(f.val, Some(Val::Value(NumberFormat::Custom)));
        assert_eq!(f.format.as_deref(), Some("001, 002, 003, ..."));
        // num 与覆盖
        assert_eq!(n.num(1).unwrap().abstract_id(), Some(0));
        let n2 = n.num(2).unwrap();
        assert_eq!(n2.overrides.len(), 2);
        assert_eq!(n2.override_for(0).unwrap().start_override, Some(Val::Value(5)));
        let ov = n2.override_for(1).unwrap().lvl.as_ref().unwrap();
        assert_eq!(ov.num_fmt.as_ref().unwrap().val, Some(Val::Value(NumberFormat::UpperRoman)));
        assert!(n.num(3).is_none());
    }

    #[test]
    fn mod_10_settings_compat_facts_and_font_table() {
        let d = dom(&format!(
            r#"<w:settings xmlns:w="{W}" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">
              <w:writeProtection w:recommended="1" w:algorithmName="SHA-512" w:hashValue="abc=" w:saltValue="s=" w:spinCount="100000"/>
              <w:zoom w:percent="100"/><w:removePersonalInformation/><w:trackRevisions/>
              <w:documentProtection w:edit="readOnly" w:enforcement="1"/>
              <w:defaultTabStop w:val="420"/><w:autoHyphenation w:val="0"/><w:evenAndOddHeaders/>
              <w:characterSpacingControl w:val="compressPunctuation"/>
              <w:compat><w:spaceForUL/><w:balanceSingleByteDoubleByteWidth/><w:doNotLeaveBackslashAlone w:val="0"/>
                <w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
                <w:compatSetting w:name="overrideTableStyleFontSizeAndJustification" w:uri="http://schemas.microsoft.com/office/word" w:val="1"/></w:compat>
              <w:rsids><w:rsidRoot w:val="00A1"/></w:rsids>
              <w:themeFontLang w:val="en-US" w:eastAsia="zh-CN"/><w:decimalSymbol w:val="."/><w:listSeparator w:val=","/>
              <w15:chartTrackingRefBased/>
            </w:settings>"#
        ));
        let mut diags = Vec::new();
        let s = Settings::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        let wp = s.write_protection.as_ref().unwrap();
        assert_eq!(wp.recommended, Some(true));
        assert_eq!(wp.spin_count, Some(Val::Value(100_000)));
        assert_eq!(s.zoom.as_ref().unwrap().percent, Some(Val::Value(100)));
        assert_eq!(s.remove_personal_information, Some(true));
        assert_eq!(s.track_revisions, Some(true));
        let dp = s.document_protection.as_ref().unwrap();
        assert_eq!(dp.edit, Some(Val::Value(DocProtect::ReadOnly)));
        assert_eq!(dp.enforcement, Some(true));
        assert_eq!(s.default_tab_stop_or_default(), 420);
        assert_eq!(s.auto_hyphenation, Some(false));
        assert_eq!(s.even_and_odd_headers, Some(true));
        assert_eq!(
            s.character_spacing_control,
            Some(Val::Value(CharacterSpacing::CompressPunctuation))
        );
        assert!(s.rsids.is_some());
        assert_eq!(s.theme_font_lang.as_ref().unwrap().east_asia.as_deref(), Some("zh-CN"));
        assert_eq!(s.chart_tracking_ref_based, Some(true));
        let facts = s.compat_facts(&d);
        assert_eq!(facts.mode, Some(15));
        assert_eq!(facts.settings.len(), 2);
        let flags: Vec<String> =
            facts.flags.iter().map(|q| q.display(d.interner()).to_string()).collect();
        assert_eq!(
            flags,
            ["w:spaceForUL", "w:balanceSingleByteDoubleByteWidth"],
            "val=0 的开关不算"
        );

        let d = dom(&format!(
            r#"<w:fonts xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
              <w:font w:name="Calibri"><w:panose1 w:val="020F0502020204030204"/><w:charset w:val="00"/><w:family w:val="swiss"/><w:pitch w:val="variable"/>
                <w:sig w:usb0="E0002AFF" w:usb1="C000247B" w:usb2="00000009" w:usb3="00000000" w:csb0="000001FF" w:csb1="00000000"/>
                <w:embedRegular r:id="rId1" w:fontKey="{{ABC}}" w:subsetted="1"/></w:font>
              <w:font w:name="宋体"><w:altName w:val="SimSun"/><w:family w:val="auto"/><w:pitch w:val="default"/></w:font>
            </w:fonts>"#
        ));
        let ft = FontTable::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(ft.fonts.len(), 2);
        let c = ft.get("Calibri").unwrap();
        assert_eq!(c.family, Some(Val::Value(FontFamily::Swiss)));
        assert_eq!(c.pitch, Some(Val::Value(FontPitch::Variable)));
        assert_eq!(c.sig.as_ref().unwrap().usb0.as_deref(), Some("E0002AFF"));
        let e = c.embed_regular.as_ref().unwrap();
        assert_eq!(e.id.as_deref(), Some("rId1"));
        assert_eq!(e.subsetted, Some(true));
        assert_eq!(ft.get("宋体").unwrap().alt_name.as_deref(), Some("SimSun"));
        assert!(Settings::from_dom(&d, &mut diags).is_none(), "根不是 w:settings");
    }
}
