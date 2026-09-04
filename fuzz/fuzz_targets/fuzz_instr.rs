//! TEST-06 `fuzz_instr`：任意字符串当字段指令。tokenizer 不 panic，且不越过原文。
//!
//! `FLD-05` 只做 tokenization 与关键字提取，任何输入都要有结果（认不出的字符进 `Word`）；
//! 保存真相是 `instr_nodes` 的原字节，所以这里额外断言 `raw` 一字不改地留在结果里。
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsword::span::FieldId;
use rsword::span::field::{InstrToken, Keyword, NESTED_PLACEHOLDER, instr};

/// token 里的文本总量不超过原文（tokenizer 不得凭空造字符）。
fn text_len(t: &InstrToken) -> usize {
    match t {
        InstrToken::Word(s) | InstrToken::Quoted(s) => s.chars().count(),
        InstrToken::Switch { arg, .. } => arg.as_deref().map_or(0, text_len),
        InstrToken::GeneralFormat { arg, .. } => arg.chars().count(),
        InstrToken::Nested(_) => 1,
    }
}

fn check(t: &InstrToken, nested: &[FieldId], depth: usize) {
    assert!(depth < 8, "开关参数不该无限嵌套");
    match t {
        InstrToken::Nested(id) => assert!(nested.contains(id), "Nested 只能引用给定的字段"),
        InstrToken::Switch { name, arg } => {
            assert!(
                name.is_ascii_alphabetic() || matches!(name, '*' | '#' | '@' | '!'),
                "开关字符超出 FLD-05 的集合: {name:?}"
            );
            if let Some(a) = arg {
                check(a, nested, depth + 1);
            }
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let raw = String::from_utf8_lossy(data).into_owned();
    // 嵌套字段：`NESTED_PLACEHOLDER` 的个数决定能引用多少个 id（`FLD-05` 的 NESTED 分支）
    let holes = raw.chars().filter(|&c| c == NESTED_PLACEHOLDER).count();
    let nested: Vec<FieldId> = (0..holes as u32).map(FieldId).collect();
    let parsed = instr::parse(&raw, &nested);

    assert_eq!(parsed.raw, raw, "raw 必须是原文");
    let total: usize = parsed.tokens.iter().map(text_len).sum();
    assert!(total <= raw.chars().count(), "token 文本 {total} 超过原文 {}", raw.chars().count());
    for t in &parsed.tokens {
        check(t, &nested, 0);
    }
    // 关键字与策略表查找：任何输入都有结果
    let _ = parsed.keyword.policy();
    if let Keyword::Unknown(u) = &parsed.keyword {
        assert_eq!(*u, u.to_ascii_uppercase(), "Unknown 关键字规范化为大写");
    }
    // 只读访问器不 panic
    let _ = parsed.first_argument();
    let _ = parsed.switch('h');
    let _ = parsed.general_format('*');
    let _ = parsed.nested().count();
});
