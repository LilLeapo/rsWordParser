//! `INDEX` 字段的结果区（`FLD-09`）：一条索引词一段，按序排好。
//!
//! 词从文档里的 `XE` 字段收（`edit::field_ops`），这里只管去重、排序与摊成 XML。

use crate::xml::entities::escaped_text;

use super::{FIELD_END, TS_INDEX_TAB_POS, field_begin, leader_ppr, no_proof_run};

/// 排序方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collation {
    /// 按 Unicode 码位（缺省）。TS 用 `localeCompare('zh-CN')`（ICU 的拼音序），我们不带 ICU，
    /// 中文的次序会不一样——登记在 `docs/04` §8。
    CodePoint,
    /// 调用方排好的次序：按这个列表出现的先后排，表外的按码位排在后面。
    Given(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexOptions {
    /// `\c "2"`：分几栏。
    pub columns: u32,
    pub collation: Collation,
    /// 右对齐点线制表位（twips）。
    pub tab_pos: Option<i64>,
    /// TS 形态（制表位 4300、指令固定 ` INDEX \c "2" `）。
    pub ts_shape: bool,
    /// 首段发 begin + 指令 + separate、末段发 end；重算既有字段时置 false。
    pub emit_field_structure: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            columns: 2,
            collation: Collation::CodePoint,
            tab_pos: None,
            ts_shape: false,
            emit_field_structure: true,
        }
    }
}

impl IndexOptions {
    pub fn from_instruction(instr: &crate::span::field::Instruction) -> IndexOptions {
        let columns =
            instr.switch('c').and_then(|c| c.trim().parse::<u32>().ok()).unwrap_or(2).max(1);
        IndexOptions { columns, ..Default::default() }
    }

    pub fn instruction(&self) -> String {
        format!(" INDEX \\c \"{}\" ", self.columns)
    }
}

/// 去重（trim 之后）、排序、摊成一条一段。一个词都不剩 → 不生成任何段（TS 同）。
pub fn generate(terms: &[String], opts: &IndexOptions) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for t in terms {
        let t = t.trim();
        if !t.is_empty() && !unique.iter().any(|u| u == t) {
            unique.push(t.to_string());
        }
    }
    if unique.is_empty() {
        return Vec::new();
    }
    match &opts.collation {
        Collation::CodePoint => unique.sort(),
        Collation::Given(order) => unique
            .sort_by_key(|t| (order.iter().position(|o| o == t).unwrap_or(usize::MAX), t.clone())),
    }
    let instr = opts.instruction();
    let tab_pos =
        if opts.ts_shape { TS_INDEX_TAB_POS } else { opts.tab_pos.unwrap_or(TS_INDEX_TAB_POS) };
    let ppr = leader_ppr(None, tab_pos);
    let n = unique.len();
    unique
        .into_iter()
        .enumerate()
        .map(|(i, term)| {
            let structure = opts.emit_field_structure;
            let first = if structure && i == 0 { field_begin(&instr, true) } else { String::new() };
            let last = if structure && i + 1 == n { FIELD_END } else { "" };
            format!(
                "<w:p>{ppr}{first}{text}{tab}{last}</w:p>",
                ppr = ppr,
                first = first,
                text = no_proof_run(&format!(
                    r#"<w:t xml:space="preserve">{}</w:t>"#,
                    escaped_text(&term)
                )),
                tab = no_proof_run("<w:tab/>"),
                last = last,
            )
        })
        .collect()
}
