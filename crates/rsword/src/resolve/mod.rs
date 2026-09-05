//! 有效属性只读视图（`spec/07-resolve.md`，`docs/03` §7）。
//!
//! 从声明值计算编辑器需要的有效值，带来源（`RES-01`）；不修改规范状态，不做排版。
//! M1 首版（任务 1.9）：样式链与 linked（`RES-02`）、run 层叠（`RES-03`）、toggle 占位（`RES-04`）、
//! 主题字体 / 颜色（`RES-05`）、Cs 选择（`RES-06`）、段落层叠的编号缩进（`RES-07`/`RES-09` 级别查找）。
//! 表格（`RES-08`）、编号标记（`RES-09`）、节（`RES-10`）在 M2+。

use crate::model::block::ListRef;
use crate::model::decl::{Numbering, Settings, Styles};
use crate::model::facts::heading_level_of_chain;
use crate::model::theme::{ColorScheme, Theme};
use crate::model::{Document, Level, Style};
use crate::semantic::props::{
    Color, ParaProps, ParaPropsField, RunProps, RunPropsField, StyleType, merge_para_props,
    merge_run_props,
};

pub mod color;
pub mod drawingml;
pub mod fonts;
pub mod symbol;
pub mod table;

pub use color::{resolve_theme_color, rgb_hex};
pub use drawingml::{ColorBase, ColorTransform, DrawingColor, Rgb};
pub use fonts::ResolvedFonts;
pub use symbol::{decode as decode_symbol, decode_pua, is_symbol_font};
pub use table::{
    ColumnSource, ColumnView, EffectiveCellProps, TableStyleLayer, TableStyleView, TableView,
    TblLookFlags, ViewCell,
};

/// 有效值的来源（`RES-01`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    Direct,
    CharStyle(String),
    ParaStyle(String),
    NumberingLevel {
        num_id: i32,
        ilvl: i32,
    },
    TableStyle {
        style: String,
        /// 命中的条件格式（`None` = 整表层）。
        cond: Option<crate::semantic::props::TblStyleOverrideType>,
    },
    DocDefaults,
    Theme,
    /// 未声明，取 Word 缺省。
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective<T> {
    pub value: T,
    pub source: Provenance,
}

/// 解析上下文：文档的声明模型。缓存键（样式表版本号）在 M2 随编辑引擎加入。
pub struct Resolver<'a> {
    pub styles: Option<&'a Styles>,
    pub numbering: Option<&'a Numbering>,
    pub theme: Option<&'a Theme>,
    pub settings: Option<&'a Settings>,
    /// 没有 theme part 时的内建 Office 调色板。
    office: ColorScheme,
}

/// 样式一层层叠后的 run 属性，与来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRunProps {
    pub props: RunProps,
    sources: Vec<Option<Provenance>>,
    /// `RES-06`：run 的复杂文种状态。
    pub cs: Effective<bool>,
}

impl EffectiveRunProps {
    pub fn source(&self, field: RunPropsField) -> Provenance {
        self.sources[field as usize].clone().unwrap_or(Provenance::Default)
    }

    /// `RES-06`：`cs` 时读 `bCs`，否则读 `b`，无交叉回退。
    pub fn bold(&self) -> Option<bool> {
        if self.cs.value { self.props.bold_cs } else { self.props.bold }
    }

    pub fn italic(&self) -> Option<bool> {
        if self.cs.value { self.props.italic_cs } else { self.props.italic }
    }

    /// 半点。
    pub fn size(&self) -> Option<u32> {
        let v = if self.cs.value { &self.props.size_cs } else { &self.props.size };
        v.as_ref().and_then(|x| x.value().copied())
    }
}

/// 段落层叠后的属性，与来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveParaProps {
    pub props: ParaProps,
    sources: Vec<Option<Provenance>>,
}

impl EffectiveParaProps {
    pub fn source(&self, field: ParaPropsField) -> Provenance {
        self.sources[field as usize].clone().unwrap_or(Provenance::Default)
    }
}

/// `RES-04` placeholder：toggle 属性（b/bCs/i/iCs/caps/smallCaps/strike/dstrike/vanish）暂按
/// "最具体的声明胜出"处理，与非 toggle 属性相同。规范的奇偶叠加规则与 Word 偏差待
/// `fixtures/resolve/toggle/*` 校准（M5），到时只改这里。
pub const TOGGLE_FIELDS: &[RunPropsField] = &[
    RunPropsField::Bold,
    RunPropsField::BoldCs,
    RunPropsField::Italic,
    RunPropsField::ItalicCs,
    RunPropsField::Caps,
    RunPropsField::SmallCaps,
    RunPropsField::Strike,
    RunPropsField::Dstrike,
    RunPropsField::Vanish,
];

impl<'a> Resolver<'a> {
    pub fn new(doc: &'a Document) -> Resolver<'a> {
        Self::from_parts(
            doc.styles.as_ref(),
            doc.numbering.as_ref(),
            doc.theme.as_ref(),
            doc.settings.as_ref(),
        )
    }

    pub fn from_parts(
        styles: Option<&'a Styles>,
        numbering: Option<&'a Numbering>,
        theme: Option<&'a Theme>,
        settings: Option<&'a Settings>,
    ) -> Resolver<'a> {
        Resolver { styles, numbering, theme, settings, office: ColorScheme::office_default() }
    }

    // ---- RES-02 样式 -------------------------------------------------------------------------

    /// basedOn 链，叶 → 根（防环，类型一致）。
    pub fn chain(&self, id: &str, kind: StyleType) -> Vec<&'a Style> {
        self.styles.map(|s| s.chain(id, kind)).unwrap_or_default()
    }

    pub fn default_style(&self, kind: StyleType) -> Option<&'a Style> {
        self.styles.and_then(|s| s.default_for(kind))
    }

    /// 样式的 run 属性：basedOn 链根 → 叶层叠，再由 `w:link` 对端补缺失项（TS 行为，`RES-02`）。
    /// 返回 `None` 表示样式不存在。
    pub fn style_run_props(&self, id: &str, kind: StyleType) -> Option<RunProps> {
        let chain = self.chain(id, kind);
        let leaf = *chain.first()?;
        let mut props = merge_chain_rpr(&chain);
        if let Some(link) = leaf.link.as_deref() {
            let other_kind = match kind {
                StyleType::Paragraph => StyleType::Character,
                _ => StyleType::Paragraph,
            };
            let other = self.chain(link, other_kind);
            if !other.is_empty() {
                let mut filled = merge_chain_rpr(&other);
                merge_run_props(&mut filled, &props); // 自身声明优先
                props = filled;
            }
        }
        Some(props)
    }

    /// 样式的段落属性：链根 → 叶层叠。
    pub fn style_para_props(&self, id: &str) -> Option<ParaProps> {
        let chain = self.chain(id, StyleType::Paragraph);
        chain.first()?;
        let mut props = ParaProps::default();
        for s in chain.iter().rev() {
            if let Some(p) = &s.ppr {
                merge_para_props(&mut props, p);
            }
        }
        // 段落样式的 rPr 不属于段落属性视图（它走 run 层叠）
        props.rpr = None;
        Some(props)
    }

    /// 样式链给出的标题级别（`RES-02` heading_level）。
    pub fn heading_level(&self, id: &str) -> Option<u8> {
        heading_level_of_chain(&self.chain(id, StyleType::Paragraph), Some(id))
    }

    /// 字符 / 段落样式是否是 linked 对的字符侧（TS `linkedCharShell`）。
    pub fn is_linked_char_shell(&self, id: &str) -> bool {
        let Some(styles) = self.styles else { return false };
        let Some(s) = styles.get(id) else { return false };
        s.kind() == Some(StyleType::Character)
            && s.link.as_deref().is_some_and(|l| {
                styles.get(l).is_some_and(|p| p.kind() == Some(StyleType::Paragraph))
            })
    }

    // ---- RES-03 / RES-06 run -----------------------------------------------------------------

    /// run 的有效属性。`para_style` / `char_style` 是 styleId；`direct` 是 run 自身的 `rPr`。
    /// 覆盖顺序：docDefaults → 段落样式链 → 字符样式链（含 linked 补缺）→ 直接；`None` 不覆盖。
    /// 编号级别 rPr 只用于列表标记（`RES-03` 第 3 条）。表格内的 run 用 [`Resolver::run_in_table`]。
    pub fn run(
        &self,
        para_style: Option<&str>,
        char_style: Option<&str>,
        direct: &RunProps,
    ) -> EffectiveRunProps {
        self.run_in_table(None, para_style, char_style, direct)
    }

    /// `RES-03` 第 4 层：表格样式（整表 + 命中的条件格式，见 [`TableView::cell`] 的 `rpr`）在段落样式链
    /// 之后、字符样式链之前生效。`table` 为 `None` 时与 [`Resolver::run`] 等价。
    pub fn run_in_table(
        &self,
        table: Option<&RunProps>,
        para_style: Option<&str>,
        char_style: Option<&str>,
        direct: &RunProps,
    ) -> EffectiveRunProps {
        let n = RunPropsField::ALL.len();
        let mut props = RunProps::default();
        let mut sources: Vec<Option<Provenance>> = vec![None; n];
        let mut apply = |props: &mut RunProps, over: &RunProps, prov: Provenance| {
            for f in merge_run_props(props, over) {
                sources[f as usize] = Some(prov.clone());
            }
        };
        if let Some(dd) = self.styles.and_then(Styles::doc_default_rpr) {
            apply(&mut props, dd, Provenance::DocDefaults);
        }
        let para_chain =
            para_style.map(|id| self.chain(id, StyleType::Paragraph)).unwrap_or_default();
        for s in para_chain.iter().rev() {
            if let Some(r) = &s.rpr {
                apply(&mut props, r, Provenance::ParaStyle(s.id().unwrap_or_default().to_string()));
            }
        }
        if let Some(t) = table {
            // 具体是哪张表的哪一层由调用方（`TableView::cell`）知道，这里只标"来自表格样式"
            apply(&mut props, t, Provenance::TableStyle { style: String::new(), cond: None });
        }
        let char_chain =
            char_style.map(|id| self.chain(id, StyleType::Character)).unwrap_or_default();
        for s in char_chain.iter().rev() {
            if let Some(r) = &s.rpr {
                apply(&mut props, r, Provenance::CharStyle(s.id().unwrap_or_default().to_string()));
            }
        }
        // linked 补缺：字符样式链没有声明、其 w:link 段落样式链声明了的项
        if let (Some(leaf), Some(id)) = (char_chain.first(), char_style)
            && let Some(link) = leaf.link.as_deref()
        {
            let own = merge_chain_rpr(&char_chain);
            let linked = merge_chain_rpr(&self.chain(link, StyleType::Paragraph));
            let mut fill = RunProps::default();
            merge_run_props(&mut fill, &linked);
            // 只补自身链没有的
            let mut probe = own.clone();
            let missing = merge_run_props(&mut probe, &fill);
            let mut layer = RunProps::default();
            for f in missing {
                copy_field(&mut layer, &fill, f);
            }
            apply(&mut props, &layer, Provenance::CharStyle(id.to_string()));
        }
        apply(&mut props, direct, Provenance::Direct);

        // RES-06：cs = 直接 rtl ?? 字符样式链 rtl ?? 段落样式链 rtl ?? false
        let cs = if let Some(v) = direct.rtl {
            Effective { value: v, source: Provenance::Direct }
        } else if let Some((s, v)) =
            char_chain.iter().find_map(|s| s.rpr.as_ref()?.rtl.map(|v| (s, v)))
        {
            Effective {
                value: v,
                source: Provenance::CharStyle(s.id().unwrap_or_default().to_string()),
            }
        } else if let Some((s, v)) =
            para_chain.iter().find_map(|s| s.rpr.as_ref()?.rtl.map(|v| (s, v)))
        {
            Effective {
                value: v,
                source: Provenance::ParaStyle(s.id().unwrap_or_default().to_string()),
            }
        } else {
            Effective { value: false, source: Provenance::Default }
        };
        EffectiveRunProps { props, sources, cs }
    }

    // ---- RES-07 段落 -------------------------------------------------------------------------

    /// 段落有效属性：docDefaults → 段落样式链 → 编号级别（仅 `ind`，且段落自身无 `ind`）→ 直接。
    pub fn para(
        &self,
        style: Option<&str>,
        list: Option<&ListRef>,
        direct: &ParaProps,
    ) -> EffectiveParaProps {
        let n = ParaPropsField::ALL.len();
        let mut props = ParaProps::default();
        let mut sources: Vec<Option<Provenance>> = vec![None; n];
        let mut apply = |props: &mut ParaProps, over: &ParaProps, prov: Provenance| {
            for f in merge_para_props(props, over) {
                sources[f as usize] = Some(prov.clone());
            }
        };
        if let Some(dd) = self.styles.and_then(Styles::doc_default_ppr) {
            apply(&mut props, dd, Provenance::DocDefaults);
        }
        if let Some(id) = style {
            for s in self.chain(id, StyleType::Paragraph).iter().rev() {
                if let Some(p) = &s.ppr {
                    apply(
                        &mut props,
                        p,
                        Provenance::ParaStyle(s.id().unwrap_or_default().to_string()),
                    );
                }
            }
        }
        if let Some(l) = list
            && direct.indent.is_none()
            && let Some(level) = self.level(l.num_id, l.ilvl)
            && let Some(ind) = level.ppr.as_ref().and_then(|p| p.indent.as_ref())
        {
            let layer = ParaProps { indent: Some(ind.clone()), ..Default::default() };
            apply(
                &mut props,
                &layer,
                Provenance::NumberingLevel { num_id: l.num_id, ilvl: l.ilvl },
            );
        }
        apply(&mut props, direct, Provenance::Direct);
        props.rpr = direct.rpr.clone();
        EffectiveParaProps { props, sources }
    }

    // ---- RES-09 级别查找 ---------------------------------------------------------------------

    /// `num → abstractNum`；`numStyleLink → 样式.numPr.numId → 其 abstractNum`（防环）；
    /// `num.overrides[ilvl].lvl` 覆盖整级。
    pub fn level(&self, num_id: i32, ilvl: i32) -> Option<&'a Level> {
        let numbering = self.numbering?;
        let num = numbering.num(num_id)?;
        if let Some(ov) = num.override_for(ilvl)
            && let Some(lvl) = &ov.lvl
        {
            return Some(lvl);
        }
        let mut abs = numbering.abstract_num(num.abstract_id()?)?;
        let mut hops = 0;
        while let Some(link) = abs.num_style_link.as_deref() {
            hops += 1;
            if hops > 16 {
                return None;
            }
            let style = self.styles?.get(link)?;
            let linked_num = style.ppr.as_ref()?.num.as_ref()?.num_id.as_ref()?.value().copied()?;
            let target = numbering.num(linked_num)?;
            abs = numbering.abstract_num(target.abstract_id()?)?;
        }
        abs.level(ilvl)
    }

    // ---- RES-05 ------------------------------------------------------------------------------

    /// 有效颜色（sRGB）：`themeColor` 存在 → 按调色板（无 theme part 时用内建 Office 调色板）
    /// 解析并施加 shade / tint；否则 `val`；`auto` / 解析失败 → `None`。
    pub fn color(&self, c: &Color) -> Option<[u8; 3]> {
        color::resolve_color(c, self.palette())
    }

    pub fn palette(&self) -> &ColorScheme {
        self.theme.and_then(|t| t.colors.as_ref()).unwrap_or(&self.office)
    }

    /// `w:rFonts` 经主题解析（`RES-05`）。
    pub fn fonts(&self, props: &RunProps) -> ResolvedFonts {
        fonts::resolve_fonts(
            props.fonts.as_ref(),
            self.theme.and_then(|t| t.fonts.as_ref()),
            self.ea_lang(),
        )
    }

    /// docDefaults 的字体，含 EA 槽空时按 `rPrDefault/w:lang/@eastAsia` 的回填（`RES-05`）。
    pub fn doc_default_fonts(&self) -> ResolvedFonts {
        let dd = self.styles.and_then(Styles::doc_default_rpr);
        let mut f = fonts::resolve_fonts(
            dd.and_then(|r| r.fonts.as_ref()),
            self.theme.and_then(|t| t.fonts.as_ref()),
            self.ea_lang(),
        );
        if f.ea_slot_empty {
            f.east_asia = None;
        }
        if f.east_asia.is_none()
            && let Some(lang) =
                dd.and_then(|r| r.lang.as_ref()).and_then(|l| l.east_asia.as_deref())
            && let Some(default) = fonts::ea_lang_default_font(lang)
        {
            let by_theme = if f.ea_slot_empty {
                fonts::theme_lang_ea_slot_font(
                    self.theme.and_then(|t| t.fonts.as_ref()),
                    self.ea_lang(),
                    dd.and_then(|r| r.fonts.as_ref())
                        .and_then(|x| x.east_asia_theme.as_ref())
                        .and_then(|v| v.value().copied()),
                )
            } else {
                None
            };
            f.east_asia = Some(by_theme.unwrap_or_else(|| default.to_string()));
            f.ea_from_lang = true;
        }
        f
    }

    /// `settings/themeFontLang/@eastAsia`。
    pub fn ea_lang(&self) -> Option<&'a str> {
        self.settings?.theme_font_lang.as_ref()?.east_asia.as_deref()
    }
}

/// 链根 → 叶层叠 rPr。
fn merge_chain_rpr(chain: &[&Style]) -> RunProps {
    let mut props = RunProps::default();
    for s in chain.iter().rev() {
        if let Some(r) = &s.rpr {
            merge_run_props(&mut props, r);
        }
    }
    props
}

/// 把 `from` 的一个字段复制到 `to`（用于 linked 补缺层）。
fn copy_field(to: &mut RunProps, from: &RunProps, f: RunPropsField) {
    macro_rules! cp {
        ($($v:ident => $field:ident),* $(,)?) => {
            match f { $(RunPropsField::$v => to.$field = from.$field.clone(),)* }
        };
    }
    cp!(
        Style => style, Fonts => fonts, Bold => bold, BoldCs => bold_cs, Italic => italic, ItalicCs => italic_cs,
        Caps => caps, SmallCaps => small_caps, Strike => strike, Dstrike => dstrike, Vanish => vanish,
        Color => color, Spacing => spacing, Scale => scale, Kern => kern, Position => position, Size => size,
        SizeCs => size_cs, Highlight => highlight, Underline => underline, Shading => shading,
        VertAlign => vert_align, Rtl => rtl, Cs => cs, Em => em, Lang => lang, SpecVanish => spec_vanish,
        TextFill => text_fill,
    );
}

#[cfg(test)]
mod tests;
