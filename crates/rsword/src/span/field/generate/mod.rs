//! 块字段生成器（`FLD-09`，`spec/18` 7.8）：TOC / SEQ 题注 / INDEX。
//!
//! 每个生成器只做一件事——**把已经算好的条目摊成 XML 片段**（TOC 与 INDEX 一条一段、题注一段）。
//! 从文档里收条目（走标题、收 XE 词、数 SEQ）是 `edit::field_ops` 的事：那里才有 `EditSession`。
//! 分开的好处是生成器可以对着 `fixtures/fieldgen/generators.json` 逐字比 TS 的输出。
//!
//! **`ts_shape`**：TS 的形态（制表位固定 9350、页码写纯数字、不发书签与超链接）。缺省是
//! **Word 的形态**：`\h` 时每条包 `w:hyperlink w:anchor`、页码走 `PAGEREF … \h` 字段、
//! 制表位按版心宽算。两者的差异登记在 `docs/04` §8。

pub mod index;
pub mod seq;
pub mod toc;

use crate::xml::entities::escaped_text;

/// `<w:r><w:fldChar w:fldCharType="begin"[ w:dirty="true"]/></w:r>` + 指令 + separate。
pub(crate) fn field_begin(instr: &str, dirty: bool) -> String {
    format!(
        concat!(
            r#"<w:r><w:fldChar w:fldCharType="begin"{dirty}/></w:r>"#,
            r#"<w:r><w:instrText xml:space="preserve">{instr}</w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#
        ),
        dirty = if dirty { r#" w:dirty="true""# } else { "" },
        instr = escaped_text(instr),
    )
}

pub(crate) const FIELD_END: &str = r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#;

/// 一条 `<w:r><w:rPr><w:noProof/></w:rPr>…</w:r>`。
pub(crate) fn no_proof_run(inner: &str) -> String {
    format!("<w:r><w:rPr><w:noProof/></w:rPr>{inner}</w:r>")
}

/// 右对齐点线制表位 + `noProof` 的 `w:pPr`（`style` 为空时不发 `w:pStyle`）。
pub(crate) fn leader_ppr(style: Option<&str>, tab_pos: i64) -> String {
    let p_style =
        style.map_or(String::new(), |s| format!(r#"<w:pStyle w:val="{}"/>"#, escaped_text(s)));
    format!(
        concat!(
            "<w:pPr>{p_style}",
            r#"<w:tabs><w:tab w:val="right" w:leader="dot" w:pos="{pos}"/></w:tabs>"#,
            "<w:rPr><w:noProof/></w:rPr></w:pPr>"
        ),
        p_style = p_style,
        pos = tab_pos,
    )
}

/// TS 固定的制表位（`generateTocFieldXml` 的 `w:pos="9350"`）。
pub const TS_TOC_TAB_POS: i64 = 9350;
/// TS `generateIndexFieldXml` 的制表位。
pub const TS_INDEX_TAB_POS: i64 = 4300;
