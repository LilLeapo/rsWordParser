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
    ($( $field:ident => $accessor:ident : $rule:ident ),+ $(,)?) => {
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

        /// 这个字段用哪条规则。**按字段分**是实测逼出来的：见 [`ToggleRule`] 的说明。
        pub fn active_rule(field: RunPropsField) -> ToggleRule {
            match field {
                $( RunPropsField::$field => ToggleRule::$rule, )+
                _ => ToggleRule::MostSpecificWins,
            }
        }
    };
}

toggle_fields! {
    // 桌面 Word 实测（2026-09-07，Office LTSC 2021）：样式层级之间异或，docDefaults 只是无人声明时的底值
    Bold => bold : WordDesktop,
    Italic => italic : WordDesktop,
    // 复杂脚本孪生：没单独实测，跟着各自的本体走（`RES-06` 只决定读哪一个，不决定怎么合成）
    BoldCs => bold_cs : WordDesktop,
    ItalicCs => italic_cs : WordDesktop,
    // 桌面版同样异或（网页版 2026-09-06 读到的是"两层都声明照样开"，与桌面版不同——桌面版为准）
    Caps => caps : WordDesktop,
    SmallCaps => small_caps : WordDesktop,
    Strike => strike : WordDesktop,
    // 桌面版实测：两层都声明**仍然**有双删除线，不异或
    Dstrike => dstrike : MostSpecificWins,
    // 桌面版实测：两层都声明的句子可见、一层的被隐藏 → 异或（网页版观察不到）
    Vanish => vanish : WordDesktop,
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
    /// **Word 实测规则**（2026-09-06，Word 网页版，`fixtures/resolve/toggle/*`）。
    ///
    /// 奇偶只发生在**层级之间**，层级内部（`basedOn` 链）是普通的"子覆盖父"：
    ///
    /// ```text
    /// 有效值 = docDefaults ⊕ 段落样式层 ⊕ 表格样式层 ⊕ 字符样式层
    /// ```
    ///
    /// 段落样式层有一处要留神：**链里一处都没声明时，它取 docDefaults 的值**。理由是每个段落
    /// 都有样式（没写 `w:pStyle` 就是 Normal），而样式链的根是 docDefaults——于是 docDefaults
    /// 的值在"docDefaults 层"与"段落样式层"各出现一次，自己把自己抵消掉。实测正是如此：
    /// 整份文档只有 docDefaults 声明 `b=true` 时，Word 显示**不加粗**。
    ///
    /// 与 ECMA-376 §17.7.3 的差异：规范说奇偶跨越"样式层级中的每一个样式"，实测里
    /// `basedOn` 链上两层都 `b=true` 仍然加粗，说明链内不计次数。这属于
    /// [MS-OI29500] 记录的 Word 偏差一类。
    ///
    /// **只在 Word 网页版观察到**（2026-09-06）。桌面版（2026-09-07）在 docDefaults 的处理上与它不同，
    /// 见 [`ToggleRule::WordDesktop`]；保留备查，不再是任何字段的激活规则。
    WordObserved,
    /// **桌面 Word 实测规则**（2026-09-07，Office LTSC 2021 16.0.14334，`fixtures/resolve/toggle/*` 八份，
    /// 读数与截图在 `corpus/real/_round2/TOGGLE.md`）：
    ///
    /// ```text
    /// 有效值 = 有样式层级声明时：段落样式层 ⊕ 表格样式层 ⊕ 字符样式层（只算声明了的层级）
    ///          一层都没声明时：docDefaults
    /// ```
    ///
    /// 与网页版的差别只在 docDefaults：桌面版里它**不参与异或**，只是没人声明时的底值——
    /// 「只有 docDefaults 写 b」加粗，「docDefaults b + 段落样式 b」加粗，「docDefaults b + 段落样式 b=0」不加粗。
    /// 层级内部（`basedOn` 链）仍是"子覆盖父"，链上两层都 `b` 照样加粗。这与 [MS-OI29500] 对 docDefaults
    /// 的记载一致（docDefaults 的 toggle 不参与奇偶计数）。
    ///
    /// fixture 没有 `settings.xml`，Word 以兼容模式 12 打开；兼容模式 15 下是否相同还没测（`docs/06`）。
    WordDesktop,
}

/// 规则**按字段选**，见 [`active_rule`] 与 `toggle_fields!` 那张表。
///
/// 2026-09-06 的 Word 实测（八份 fixture）发现同一份规范里的 toggle 属性在 Word 里并不同待遇：
/// `b` / `i` 按层级异或，`caps` / `smallCaps` / `strike` / `dstrike` 却是"最具体的声明胜出"。
/// 所以没有单一的"当前规则"。
pub fn rule_of(field: RunPropsField) -> ToggleRule {
    active_rule(field)
}

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
        ToggleRule::WordObserved => {
            // 每个层级先按"子覆盖父"取一个值（链内不计次数），再层级之间异或
            let leaf = |chain: &[Option<bool>]| chain.iter().copied().flatten().next();
            // 段落样式层没声明时取 docDefaults（每个段落都有样式，样式链的根是 docDefaults）
            let para = leaf(l.para_chain).or(l.doc_default);
            let character = leaf(l.char_chain);
            if l.doc_default.is_none() && para.is_none() && character.is_none() && l.table.is_none()
            {
                return None;
            }
            let on = |v: Option<bool>| v.unwrap_or(false);
            Some(on(l.doc_default) ^ on(para) ^ on(l.table) ^ on(character))
        }
        ToggleRule::WordDesktop => {
            let leaf = |chain: &[Option<bool>]| chain.iter().copied().flatten().next();
            let levels = [leaf(l.para_chain), l.table, leaf(l.char_chain)];
            let declared: Vec<bool> = levels.iter().copied().flatten().collect();
            if declared.is_empty() {
                // 一层都没声明：docDefaults 就是普通的底值，不异或自己
                return l.doc_default;
            }
            Some(declared.into_iter().fold(false, |acc, v| acc ^ v))
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

    /// 2026-09-06 在 Word 网页版上实测的十条（`fixtures/resolve/toggle/*`）。
    /// 这一组就是 `WordObserved` 规则的定义式：改规则先过这一关。
    #[test]
    fn res_04_word_observed_matches_the_fixtures() {
        let rule = ToggleRule::WordObserved;
        let t = Some(true);
        // ① 段落样式 b + 字符样式 b → 不加粗；只有段落样式 → 加粗
        assert_eq!(resolve_toggle(rule, &layers(None, &[t], &[t], None)), Some(false));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], None)), Some(true));
        // ② docDefaults b + 段落样式 b → 不加粗；**只有 docDefaults b 也不加粗**
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], t)), Some(false));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[], t)), Some(false));
        // ③ basedOn 链上两层都 b → 仍然加粗（链内不计次数）
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t, t], None)), Some(true));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], None)), Some(true));
        // ④ 表格样式 firstRow b + 段落样式 b → 不加粗；非首行 → 加粗
        let para_only = [t];
        let mut tbl = layers(None, &[], &para_only, None);
        tbl.table = t;
        assert_eq!(resolve_toggle(rule, &tbl), Some(false));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], None)), Some(true));
        // ⑤ 直接 w:b w:val="0" 压住样式的 b → 不加粗；没有直接格式 → 加粗
        assert_eq!(resolve_toggle(rule, &layers(Some(false), &[], &[t], None)), Some(false));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], None)), Some(true));
        // 谁都没声明 → 未声明
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[], None)), None);
    }

    /// 2026-09-07 桌面 Word（Office LTSC 2021）的二十五个测点（`corpus/real/_round2/TOGGLE.md`）。
    /// 这一组就是 `WordDesktop` 规则的定义式：改规则先过这一关。
    #[test]
    fn res_04_word_desktop_matches_the_fixtures() {
        let rule = ToggleRule::WordDesktop;
        let (t, f) = (Some(true), Some(false));
        // 段落样式 b + 字符样式 b → 不加粗；只有段落样式 → 加粗
        assert_eq!(resolve_toggle(rule, &layers(None, &[t], &[t], None)), Some(false));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], None)), Some(true));
        // docDefaults b + 段落样式 b → **加粗**；只有 docDefaults b → **加粗**（与网页版读数相反）
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t], t)), Some(true));
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[], t)), Some(true));
        // docDefaults b + 段落样式 b=0 → 不加粗
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[f], t)), Some(false));
        // basedOn 链上两层都 b → 加粗（链内不计次数）
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[t, t], None)), Some(true));
        // 表格样式 firstRow b + 段落样式 b → 不加粗；非首行 → 加粗
        let para_only = [t];
        let mut tbl = layers(None, &[], &para_only, None);
        tbl.table = t;
        assert_eq!(resolve_toggle(rule, &tbl), Some(false));
        // 直接 w:b w:val="0" 压住样式的 b
        assert_eq!(resolve_toggle(rule, &layers(Some(false), &[], &[t], None)), Some(false));
        // 谁都没声明 → 未声明
        assert_eq!(resolve_toggle(rule, &layers(None, &[], &[], None)), None);
    }

    /// 桌面版实测：同一份规范里的 toggle 在 Word 里并不同待遇——只有 `dstrike` 不异或。
    #[test]
    fn res_04_rule_is_chosen_per_field() {
        for f in [
            RunPropsField::Bold,
            RunPropsField::Italic,
            RunPropsField::BoldCs,
            RunPropsField::ItalicCs,
            RunPropsField::Caps,
            RunPropsField::SmallCaps,
            RunPropsField::Strike,
            RunPropsField::Vanish,
        ] {
            assert_eq!(active_rule(f), ToggleRule::WordDesktop, "{f:?}");
        }
        assert_eq!(active_rule(RunPropsField::Dstrike), ToggleRule::MostSpecificWins);
        // 两层都声明 true：`strike` 抵消掉，`dstrike` 照样是开的
        let t = Some(true);
        let (ch, pa) = ([t], [t]);
        let l = layers(None, &ch, &pa, None);
        assert_eq!(resolve_toggle(active_rule(RunPropsField::Strike), &l), Some(false));
        assert_eq!(resolve_toggle(active_rule(RunPropsField::Dstrike), &l), Some(true));
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
