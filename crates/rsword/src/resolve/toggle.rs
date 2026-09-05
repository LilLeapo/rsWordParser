//! `RES-04` toggle 属性的合成规则（`spec/16` 任务 5.8）。
//!
//! toggle 属性（`b bCs i iCs caps smallCaps strike dstrike vanish`）在 ECMA-376 §17.7.3 里
//! **不是** `child ?? parent`：规范说样式层级里为 `true` 的次数为奇数才是 `true`，再与
//! docDefaults 异或；而 [MS-OI29500] 又记录了 Word 在 docDefaults、表格样式、多层 `basedOn`
//! 上的一串偏差，且与 Word 版本有关。
//!
//! 所以规则在这里**参数化**，由 [`ACTIVE_TOGGLE_RULE`] 选一条，`fixtures/resolve/toggle/*`
//! 用真实 Word 的显示结果校准（`RES-12` / `TEST-08`）。fixture 的观察值填好之前，激活的仍是
//! M1 起用的 [`ToggleRule::MostSpecificWins`]——它与非 toggle 属性同规则，也是 TS `display`
//! 的行为，所以 `tests/resolve.rs` 的八万多项对照保持全等。
//!
//! 换规则只改 [`ACTIVE_TOGGLE_RULE`] 一行：层叠已经按层把各层的声明喂给 [`resolve_toggle`]。

use crate::semantic::props::{RunProps, RunPropsField};

/// 九个 toggle 字段一张表：字段枚举、`RunProps` 的读写、`TOGGLE_FIELDS` 常量一次说清。
///
/// ```ignore
/// for &f in TOGGLE_FIELDS {
///     let v = toggle_of(&props, f);       // Option<bool>
///     set_toggle(&mut props, f, Some(true));
/// }
/// ```
macro_rules! toggle_fields {
    ($( $field:ident => $accessor:ident ),+ $(,)?) => {
        /// `RES-04` 的 toggle 字段（`specVanish` 不是 toggle）。
        pub const TOGGLE_FIELDS: &[RunPropsField] = &[$( RunPropsField::$field ),+];

        /// 某个 toggle 字段的声明值（未声明 = `None`）。
        pub fn toggle_of(p: &RunProps, field: RunPropsField) -> Option<bool> {
            match field {
                $( RunPropsField::$field => p.$accessor, )+
                _ => None,
            }
        }

        /// 写回一个 toggle 字段。
        pub fn set_toggle(p: &mut RunProps, field: RunPropsField, value: Option<bool>) {
            match field {
                $( RunPropsField::$field => p.$accessor = value, )+
                _ => {}
            }
        }
    };
}

toggle_fields! {
    Bold => bold,
    BoldCs => bold_cs,
    Italic => italic,
    ItalicCs => italic_cs,
    Caps => caps,
    SmallCaps => small_caps,
    Strike => strike,
    Dstrike => dstrike,
    Vanish => vanish,
}

/// 可选的 toggle 合成规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleRule {
    /// 最具体的声明胜出（直接 → 字符样式链叶 → 表格样式 → 段落样式链叶 → docDefaults）。
    ///
    /// M1 起的行为，也是 TS `display` 的行为。
    MostSpecificWins,
    /// ECMA-376 §17.7.3 的字面规则：**样式层级**里为 `true` 的次数为奇数则为 `true`，
    /// 再与 docDefaults 异或；直接格式（run 自己的 `rPr`）仍然一票定音。
    OddParity,
}

/// 当前激活的规则。换成 [`ToggleRule::OddParity`] 之前必须有真实 Word 的观察值
/// （`fixtures/resolve/toggle/*/expected.toml` 的 `verified = true`）。
pub const ACTIVE_TOGGLE_RULE: ToggleRule = ToggleRule::MostSpecificWins;

/// 一个 toggle 字段的各层声明。链都是**叶 → 根**（与 `Resolver::chain` 同序）。
#[derive(Debug, Clone, Default)]
pub struct ToggleLayers<'a> {
    /// run 自己的 `rPr`。
    pub direct: Option<bool>,
    /// 字符样式链，叶在前。
    pub char_chain: &'a [Option<bool>],
    /// 表格样式（整表 + 命中的条件格式合成后的一层）。
    pub table: Option<bool>,
    /// 段落样式链，叶在前。
    pub para_chain: &'a [Option<bool>],
    /// `docDefaults/rPrDefault`。
    pub doc_default: Option<bool>,
}

/// 合成一个 toggle 字段的有效值（`None` = 谁都没声明，由渲染器按 `false` 处理）。
pub fn resolve_toggle(rule: ToggleRule, l: &ToggleLayers<'_>) -> Option<bool> {
    // 直接格式在两条规则里都一票定音（`w:b w:val="0"` 压住样式的 b，Word 实测一致）
    if let Some(v) = l.direct {
        return Some(v);
    }
    match rule {
        ToggleRule::MostSpecificWins => l
            .char_chain
            .iter()
            .copied()
            .flatten()
            .next()
            .or(l.table)
            .or_else(|| l.para_chain.iter().copied().flatten().next())
            .or(l.doc_default),
        ToggleRule::OddParity => {
            let styles: Vec<bool> = l
                .char_chain
                .iter()
                .chain(l.para_chain.iter())
                .copied()
                .flatten()
                .chain(l.table)
                .collect();
            if styles.is_empty() {
                return l.doc_default;
            }
            let odd = styles.iter().filter(|&&v| v).count() % 2 == 1;
            Some(odd ^ l.doc_default.unwrap_or(false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers<'a>(
        direct: Option<bool>,
        char_chain: &'a [Option<bool>],
        para_chain: &'a [Option<bool>],
        doc_default: Option<bool>,
    ) -> ToggleLayers<'a> {
        ToggleLayers { direct, char_chain, table: None, para_chain, doc_default }
    }

    #[test]
    fn res_04_direct_formatting_always_wins() {
        for rule in [ToggleRule::MostSpecificWins, ToggleRule::OddParity] {
            let l = layers(Some(false), &[Some(true)], &[Some(true)], Some(true));
            assert_eq!(resolve_toggle(rule, &l), Some(false), "{rule:?}");
        }
    }

    #[test]
    fn res_04_most_specific_wins_layer_order() {
        let rule = ToggleRule::MostSpecificWins;
        // 字符样式链的叶胜过段落样式链
        let l = layers(None, &[Some(false), Some(true)], &[Some(true)], None);
        assert_eq!(resolve_toggle(rule, &l), Some(false));
        // 字符样式链没声明 → 段落样式链的叶
        let l = layers(None, &[None], &[Some(true), Some(false)], None);
        assert_eq!(resolve_toggle(rule, &l), Some(true));
        // 都没声明 → docDefaults
        let l = layers(None, &[], &[], Some(true));
        assert_eq!(resolve_toggle(rule, &l), Some(true));
        // 谁都没声明
        let l = layers(None, &[], &[], None);
        assert_eq!(resolve_toggle(rule, &l), None);
        // 表格样式在段落样式链之后、字符样式链之前
        let mut l = layers(None, &[None], &[Some(false)], None);
        l.table = Some(true);
        assert_eq!(resolve_toggle(rule, &l), Some(true));
    }

    #[test]
    fn res_04_odd_parity_counts_style_layers() {
        let rule = ToggleRule::OddParity;
        // 两层都 true → 偶数 → false
        let l = layers(None, &[Some(true)], &[Some(true)], None);
        assert_eq!(resolve_toggle(rule, &l), Some(false));
        // 一层 true → 奇数 → true
        let l = layers(None, &[], &[Some(true)], None);
        assert_eq!(resolve_toggle(rule, &l), Some(true));
        // 样式层为奇数、docDefaults 也 true → 异或 → false
        let l = layers(None, &[], &[Some(true)], Some(true));
        assert_eq!(resolve_toggle(rule, &l), Some(false));
        // 样式层一层都没声明 → 直接用 docDefaults（不异或自己）
        let l = layers(None, &[None], &[None], Some(true));
        assert_eq!(resolve_toggle(rule, &l), Some(true));
    }

    #[test]
    fn res_04_toggle_field_table_reads_and_writes() {
        let mut p = RunProps::default();
        for &f in TOGGLE_FIELDS {
            assert_eq!(toggle_of(&p, f), None, "{f:?}");
            set_toggle(&mut p, f, Some(true));
            assert_eq!(toggle_of(&p, f), Some(true), "{f:?}");
        }
        assert_eq!(TOGGLE_FIELDS.len(), 9);
        // 非 toggle 字段读不出东西也写不进去
        assert_eq!(toggle_of(&p, RunPropsField::Size), None);
    }
}
