//! `TOC` 字段的结果区（`FLD-09`）。
//!
//! 一条目录条目一段：`pStyle TOC{级别}` + 右对齐点线制表位 + `noProof`，正文是标题文字、
//! 一个制表符、页码。字段结构（begin + 指令 + separate）在**首段**开头、end 在**末段**末尾
//! （`FLD-08` / `FLD-12` 的多段字段形态）。
//!
//! begin 带 `w:dirty="true"`：静态文字只是打开前的可见结果，Word 一打开就自己重算。

use crate::xml::entities::escaped_text;

use super::{FIELD_END, TS_TOC_TAB_POS, field_begin, leader_ppr, no_proof_run};

/// 一条目录条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// 标题级别 1–9。
    pub level: u8,
    pub text: String,
    /// 分页算出来的页码（调用方给；`None` = 不写）。
    pub page_no: Option<u32>,
    /// `\h` 模式下这条目指向的书签名（`_Toc` + 9 位数字）。
    pub bookmark: Option<String>,
}

/// 生成选项。缺省是 **Word 的形态**；`ts_shape` 切到 TS 的。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TocOptions {
    /// `\o "a-b"` 的级别范围（含两端）。
    pub levels: (u8, u8),
    /// `\u`：也认段落自己的 `outlineLvl`，不只是标题样式。
    pub use_outline: bool,
    /// `\t "样式,级别,…"`：自定义样式到级别的映射。
    pub styles: Vec<(String, u8)>,
    /// `\h`：条目做成指向标题书签的超链接。
    pub hyperlinks: bool,
    /// `\n`：不写页码。
    pub no_page_numbers: bool,
    /// `\z` / `\p` / `\w`：记下来原样发回指令，不影响条目。
    pub passthrough: Vec<String>,
    /// 右对齐点线制表位（twips）；`None` → 版心宽由调用方算好之前用 TS 的 9350。
    pub tab_pos: Option<i64>,
    /// TS 形态：制表位 9350、页码写纯数字、不发书签与超链接、指令固定
    /// ` TOC \o "1-{最大级别}" \h \z \u `。
    pub ts_shape: bool,
    /// 首段发 begin + 指令 + separate、末段发 end。**重算既有字段时置 false**：
    /// 结构 run 是原来那几个（`UpdateBlockField` 保留它们），再发一份就重了。
    pub emit_field_structure: bool,
}

impl Default for TocOptions {
    fn default() -> Self {
        Self {
            levels: (1, 9),
            use_outline: true,
            styles: Vec::new(),
            hyperlinks: true,
            no_page_numbers: false,
            passthrough: Vec::new(),
            tab_pos: None,
            ts_shape: false,
            emit_field_structure: true,
        }
    }
}

impl TocOptions {
    /// 从既有 `TOC` 字段的指令里读出开关（`RegenerateBlockField` 用）。
    pub fn from_instruction(instr: &crate::span::field::Instruction) -> TocOptions {
        let mut o = TocOptions { hyperlinks: instr.has_switch('h'), ..Default::default() };
        if let Some(range) = instr.switch('o')
            && let Some((a, b)) = range.split_once('-')
            && let (Ok(a), Ok(b)) = (a.trim().parse::<u8>(), b.trim().parse::<u8>())
        {
            o.levels = (a.clamp(1, 9), b.clamp(1, 9));
        }
        o.use_outline = instr.has_switch('u');
        if let Some(list) = instr.switch('t') {
            let parts: Vec<&str> = list.split(',').map(str::trim).collect();
            for pair in parts.chunks(2) {
                if let [name, level] = pair
                    && let Ok(l) = level.parse::<u8>()
                    && !name.is_empty()
                {
                    o.styles.push(((*name).to_string(), l.clamp(1, 9)));
                }
            }
        }
        o.no_page_numbers = instr.has_switch('n');
        for sw in ['z', 'p', 'w'] {
            if instr.has_switch(sw) {
                o.passthrough.push(format!("\\{sw}"));
            }
        }
        o
    }

    /// 这次要发的指令文本（两端各一个空格，与 Word / TS 同）。
    pub fn instruction(&self, entries: &[TocEntry]) -> String {
        if self.ts_shape {
            let max = entries.iter().map(|e| e.level).max().unwrap_or(1).clamp(1, 9);
            return format!(" TOC \\o \"1-{max}\" \\h \\z \\u ");
        }
        let mut s = format!(" TOC \\o \"{}-{}\"", self.levels.0, self.levels.1);
        if !self.styles.is_empty() {
            let list: Vec<String> = self.styles.iter().map(|(n, l)| format!("{n},{l}")).collect();
            s.push_str(&format!(" \\t \"{}\"", list.join(",")));
        }
        if self.hyperlinks {
            s.push_str(" \\h");
        }
        for p in &self.passthrough {
            s.push(' ');
            s.push_str(p);
        }
        if self.use_outline {
            s.push_str(" \\u");
        }
        if self.no_page_numbers {
            s.push_str(" \\n");
        }
        s.push(' ');
        s
    }
}

/// 结果区：一条目录条目一段。条目为空 → 不生成任何段（TS 同）。
pub fn generate(entries: &[TocEntry], opts: &TocOptions) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }
    let instr = opts.instruction(entries);
    let tab_pos =
        if opts.ts_shape { TS_TOC_TAB_POS } else { opts.tab_pos.unwrap_or(TS_TOC_TAB_POS) };
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let style = format!("TOC{}", e.level.clamp(1, 9));
            let structure = opts.emit_field_structure;
            let first = if structure && i == 0 { field_begin(&instr, true) } else { String::new() };
            let last = if structure && i + 1 == entries.len() { FIELD_END } else { "" };
            format!(
                "<w:p>{ppr}{first}{body}{last}</w:p>",
                ppr = leader_ppr(Some(&style), tab_pos),
                first = first,
                body = entry_body(e, opts),
                last = last,
            )
        })
        .collect()
}

/// 一条目录条目的正文：文字 + 制表符 + 页码。Word 形态下这三样一起包进 `w:hyperlink`。
fn entry_body(e: &TocEntry, opts: &TocOptions) -> String {
    let text =
        no_proof_run(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, escaped_text(&e.text)));
    let tab = no_proof_run("<w:tab/>");
    let page = match (opts.no_page_numbers, e.page_no) {
        (true, _) | (_, None) => String::new(),
        (false, Some(n)) if opts.ts_shape => no_proof_run(&format!("<w:t>{n}</w:t>")),
        // Word 写的是 `PAGEREF <书签> \h` 字段，静态数字是它的缓存结果
        (false, Some(n)) => match &e.bookmark {
            Some(b) => format!(
                "{begin}{cache}{end}",
                begin = field_begin(&format!(" PAGEREF {b} \\h "), false),
                cache = no_proof_run(&format!("<w:t>{n}</w:t>")),
                end = FIELD_END,
            ),
            None => no_proof_run(&format!("<w:t>{n}</w:t>")),
        },
    };
    let body = format!("{text}{tab}{page}");
    match (&e.bookmark, opts.ts_shape || !opts.hyperlinks) {
        (Some(b), false) => format!(
            r#"<w:hyperlink w:anchor="{}">{body}</w:hyperlink>"#,
            crate::xml::entities::escaped_attr(b),
        ),
        _ => body,
    }
}
