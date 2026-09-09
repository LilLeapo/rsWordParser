//! `SEQ` 题注段（`FLD-09`）：`<标签> <SEQ 字段> <说明>`，例如「图 1 系统架构」。
//!
//! `SEQ` 的 begin 带 `w:dirty="true"`：Word 一打开就把全篇题注重新编号，静态数字只是那之前
//! 的可见结果。

use crate::xml::entities::escaped_text;

/// 一段题注。`text` 为空时只发标签与编号（TS 同）。
///
/// `ts_shape` 为真时 `w:pPr` 里 `w:jc` 写在 `w:spacing` **之前**——TS 就是那么发的，但那违反
/// `CT_PPr` 的元素次序（`PROP-05`：`spacing` 21 在 `jc` 26 之前），我们自己的保存校验会拦。
/// 缺省按规范次序（`docs/04` §8）。
pub fn caption(label: &str, number: u32, text: &str, ts_shape: bool) -> String {
    const RPR: &str =
        r#"<w:rPr><w:color w:val="44546A"/><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr>"#;
    let run = |inner: &str| format!("<w:r>{RPR}{inner}</w:r>");
    let label_esc = escaped_text(label);
    let tail = if text.is_empty() {
        String::new()
    } else {
        run(&format!(r#"<w:t xml:space="preserve"> {}</w:t>"#, escaped_text(text)))
    };
    let ppr = if ts_shape {
        r#"<w:pPr><w:jc w:val="center"/><w:spacing w:before="80" w:after="200"/></w:pPr>"#
    } else {
        r#"<w:pPr><w:spacing w:before="80" w:after="200"/><w:jc w:val="center"/></w:pPr>"#
    };
    format!(
        "<w:p>{ppr}{label}{begin}{instr}{sep}{cache}{end}{tail}</w:p>",
        ppr = ppr,
        label = run(&format!(r#"<w:t xml:space="preserve">{label_esc} </w:t>"#)),
        begin = run(r#"<w:fldChar w:fldCharType="begin" w:dirty="true"/>"#),
        instr = run(&format!(
            r#"<w:instrText xml:space="preserve"> SEQ {label_esc} \* ARABIC </w:instrText>"#
        )),
        sep = run(r#"<w:fldChar w:fldCharType="separate"/>"#),
        cache = run(&format!("<w:t>{number}</w:t>")),
        end = run(r#"<w:fldChar w:fldCharType="end"/>"#),
        tail = tail,
    )
}
