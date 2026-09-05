//! 指令 tokenization、关键字与策略表（`FLD-05` / `FLD-06`）。
//!
//! **只做 tokenization**：不解释 `IF` 的条件、不求值 `=SUM(ABOVE)`、不解析 `MERGEFIELD` 的数据源。
//! `Instruction` 是视图，保存真相永远是 `instr_nodes` 的原字节（`docs/03` §13），
//! 因此这里**禁止**用解析结果重新序列化指令——开关（`\r` `\h` `\* MERGEFORMAT`）会丢。
//!
//! tokenizer 对任何输入都不失败：认不出来的字符归入 `Word`。

use super::FieldId;

/// 嵌套字段在指令文本里的占位符（`FLD-03`）。
pub const NESTED_PLACEHOLDER: char = '\u{FFFC}';

/// 指令的语义视图（`docs/03` §5.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub keyword: Keyword,
    pub tokens: Vec<InstrToken>,
    /// `instr_nodes` 里所有 `w:instrText` / `w:delInstrText` 的文本按序拼接，不 trim。
    pub raw: String,
}

impl Instruction {
    /// 空指令（`w:fldSimple` 没有 `w:instr` 时也走这里）。
    pub fn empty() -> Self {
        Self { keyword: Keyword::Unknown(String::new()), tokens: Vec::new(), raw: String::new() }
    }

    /// 第一个非开关实参（`REF bookmark`、`HYPERLINK "url"` 的目标）。
    pub fn first_argument(&self) -> Option<&str> {
        self.tokens.iter().find_map(|t| match t {
            InstrToken::Word(s) | InstrToken::Quoted(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// 某个开关的实参（`\o "1-3"` → `Some("1-3")`）；开关存在但无实参 → `Some("")`。
    pub fn switch(&self, name: char) -> Option<&str> {
        let want = name.to_ascii_lowercase();
        self.tokens.iter().find_map(|t| match t {
            InstrToken::Switch { name, arg } if name.to_ascii_lowercase() == want => Some(
                arg.as_deref()
                    .and_then(|a| match a {
                        InstrToken::Word(s) | InstrToken::Quoted(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .unwrap_or(""),
            ),
            _ => None,
        })
    }

    pub fn has_switch(&self, name: char) -> bool {
        self.switch(name).is_some()
    }

    /// 通用格式开关（`\*` `\#` `\@` `\!`）的实参。
    pub fn general_format(&self, kind: char) -> Option<&str> {
        self.tokens.iter().find_map(|t| match t {
            InstrToken::GeneralFormat { kind: k, arg } if *k == kind => Some(arg.as_str()),
            _ => None,
        })
    }

    pub fn nested(&self) -> impl Iterator<Item = FieldId> + '_ {
        self.tokens.iter().filter_map(|t| match t {
            InstrToken::Nested(id) => Some(*id),
            _ => None,
        })
    }
}

/// 指令里的一项（`docs/03` §5.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstrToken {
    /// 裸词（`bare`）。
    Word(String),
    /// 引号串，已去掉外层引号并解掉 `\"` / `\\`。
    Quoted(String),
    /// `\h` `\o "1-3"` …；`arg` 是紧随其后的实参（下一项是开关时为 `None`）。
    Switch { name: char, arg: Option<Box<InstrToken>> },
    /// 通用格式开关 `\*` `\#` `\@` `\!`（`FLD-05`）。
    GeneralFormat { kind: char, arg: String },
    /// 嵌套字段占位（`U+FFFC`）。
    Nested(FieldId),
}

impl InstrToken {
    /// 实参的文本（`Switch` / `Nested` 没有）。
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Word(s) | Self::Quoted(s) => Some(s),
            _ => None,
        }
    }
}

/// 策略（`FLD-06` / `FLD-07`）：决定显示形态、可编辑性与保存行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldPolicy {
    /// 不可见零宽（XE / TC / SET …）。
    Marker,
    /// 结果作为一个不可键入的内联原子。
    Atom,
    /// 透明：结果 run 正常显示与编辑，带链接目标。
    Link,
    /// 表单域（复选框 / 文本框 / 下拉）。
    Form,
    Picture,
    Object,
    /// 结果是多个段落，整体只读。
    Block,
    /// 表外关键字：显示与编辑同 `Atom`，保存原字节。
    Unknown,
}

/// 关键字表：变体名、规范写法（大写）、策略。一处定义，`parse` / `as_str` / `policy` 都由它生成。
macro_rules! keywords {
    ($($variant:ident => $text:literal, $policy:ident;)*) => {
        /// 字段关键字（`FLD-06`）。比较不区分大小写，规范化为大写。
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Keyword {
            $($variant,)*
            /// 表外关键字（厂商私有等），保存原文（已大写）。
            Unknown(String),
        }

        impl Keyword {
            /// 规范写法；`Unknown` 返回原文。
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $text,)*
                    Self::Unknown(s) => s.as_str(),
                }
            }

            /// 不区分大小写地识别；表外 → `Unknown`（大写形式）。
            pub fn parse(word: &str) -> Keyword {
                let upper = word.to_ascii_uppercase();
                match upper.as_str() {
                    $($text => Self::$variant,)*
                    _ => Self::Unknown(upper),
                }
            }

            /// `FLD-06` 的策略表（不含"跨段一律 Block"与表单域降级两条覆盖规则）。
            pub fn policy(&self) -> FieldPolicy {
                match self {
                    $(Self::$variant => FieldPolicy::$policy,)*
                    Self::Unknown(_) => FieldPolicy::Unknown,
                }
            }
        }
    };
}

keywords! {
    // Marker
    Xe => "XE", Marker;
    Ta => "TA", Marker;
    Tc => "TC", Marker;
    Rd => "RD", Marker;
    Private => "PRIVATE", Marker;
    Set => "SET", Marker;
    // Atom
    Page => "PAGE", Atom;
    NumPages => "NUMPAGES", Atom;
    Section => "SECTION", Atom;
    SectionPages => "SECTIONPAGES", Atom;
    NumWords => "NUMWORDS", Atom;
    NumChars => "NUMCHARS", Atom;
    Date => "DATE", Atom;
    Time => "TIME", Atom;
    CreateDate => "CREATEDATE", Atom;
    SaveDate => "SAVEDATE", Atom;
    PrintDate => "PRINTDATE", Atom;
    EditTime => "EDITTIME", Atom;
    Author => "AUTHOR", Atom;
    Title => "TITLE", Atom;
    Subject => "SUBJECT", Atom;
    Keywords => "KEYWORDS", Atom;
    Comments => "COMMENTS", Atom;
    LastSavedBy => "LASTSAVEDBY", Atom;
    FileName => "FILENAME", Atom;
    FileSize => "FILESIZE", Atom;
    Template => "TEMPLATE", Atom;
    DocProperty => "DOCPROPERTY", Atom;
    DocVariable => "DOCVARIABLE", Atom;
    UserName => "USERNAME", Atom;
    UserInitials => "USERINITIALS", Atom;
    UserAddress => "USERADDRESS", Atom;
    Seq => "SEQ", Atom;
    StyleRef => "STYLEREF", Atom;
    PageRef => "PAGEREF", Atom;
    NoteRef => "NOTEREF", Atom;
    Ref => "REF", Atom;
    Quote => "QUOTE", Atom;
    Symbol => "SYMBOL", Atom;
    ListNum => "LISTNUM", Atom;
    AutoNum => "AUTONUM", Atom;
    AutoNumLgl => "AUTONUMLGL", Atom;
    AutoNumOut => "AUTONUMOUT", Atom;
    RevNum => "REVNUM", Atom;
    Info => "INFO", Atom;
    Formula => "=", Atom;
    MergeField => "MERGEFIELD", Atom;
    MergeRec => "MERGEREC", Atom;
    MergeSeq => "MERGESEQ", Atom;
    Next => "NEXT", Atom;
    NextIf => "NEXTIF", Atom;
    SkipIf => "SKIPIF", Atom;
    If => "IF", Atom;
    Compare => "COMPARE", Atom;
    Advance => "ADVANCE", Atom;
    Eq => "EQ", Atom;
    GotoButton => "GOTOBUTTON", Atom;
    MacroButton => "MACROBUTTON", Atom;
    Citation => "CITATION", Atom;
    FillIn => "FILLIN", Atom;
    Ask => "ASK", Atom;
    GreetingLine => "GREETINGLINE", Atom;
    AddressBlock => "ADDRESSBLOCK", Atom;
    AutoText => "AUTOTEXT", Atom;
    AutoTextList => "AUTOTEXTLIST", Atom;
    BidiOutline => "BIDIOUTLINE", Atom;
    // Link / Form / Picture / Object
    Hyperlink => "HYPERLINK", Link;
    FormCheckBox => "FORMCHECKBOX", Form;
    FormText => "FORMTEXT", Form;
    FormDropDown => "FORMDROPDOWN", Form;
    IncludePicture => "INCLUDEPICTURE", Picture;
    Embed => "EMBED", Object;
    Link => "LINK", Object;
    // Block
    Toc => "TOC", Block;
    Index => "INDEX", Block;
    Bibliography => "BIBLIOGRAPHY", Block;
    IncludeText => "INCLUDETEXT", Block;
    Database => "DATABASE", Block;
}

/// `FLD-05`：把指令原文切成 token。`nested` 按出现顺序供占位符消费。
///
/// 不失败：认不出的字符进 `Word`。
pub fn parse(raw: &str, nested: &[FieldId]) -> Instruction {
    let mut p = Parser { src: raw.as_bytes(), chars: raw.chars().collect(), i: 0, nested, n: 0 };
    let keyword = p.keyword();
    let mut tokens = Vec::new();
    while let Some(t) = p.item() {
        tokens.push(t);
    }
    Instruction { keyword, tokens, raw: raw.to_string() }
}

struct Parser<'a> {
    #[allow(dead_code)]
    src: &'a [u8],
    chars: Vec<char>,
    i: usize,
    nested: &'a [FieldId],
    /// 已消费的嵌套占位符个数。
    n: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.i += 1;
        }
    }

    /// `keyword := '=' | ident`。`=`（公式）不需要后随空白。
    fn keyword(&mut self) -> Keyword {
        self.skip_ws();
        match self.peek() {
            Some('=') => {
                self.i += 1;
                Keyword::Formula
            }
            Some(_) => {
                let start = self.i;
                while matches!(self.peek(), Some(c) if !c.is_whitespace() && c != NESTED_PLACEHOLDER)
                {
                    self.i += 1;
                }
                let word: String = self.chars[start..self.i].iter().collect();
                if word.is_empty() {
                    // 指令以嵌套字段开头：没有可识别的关键字
                    Keyword::Unknown(String::new())
                } else {
                    Keyword::parse(&word)
                }
            }
            None => Keyword::Unknown(String::new()),
        }
    }

    /// `item := switch | argument`。
    fn item(&mut self) -> Option<InstrToken> {
        self.skip_ws();
        match self.peek()? {
            '\\' => Some(self.switch()),
            _ => self.argument(),
        }
    }

    /// `switch := '\' switch-char argument?`。
    fn switch(&mut self) -> InstrToken {
        self.i += 1; // '\'
        let Some(c) = self.peek() else {
            return InstrToken::Word("\\".to_string());
        };
        if !(c.is_ascii_alphabetic() || matches!(c, '*' | '#' | '@' | '!')) {
            // 认不出的开关字符：连反斜杠一起当裸词，别丢信息
            self.i += 1;
            return InstrToken::Word(format!("\\{c}"));
        }
        self.i += 1;
        if matches!(c, '*' | '#' | '@' | '!') {
            let arg = self.optional_argument().and_then(|t| t.text().map(str::to_string));
            return InstrToken::GeneralFormat { kind: c, arg: arg.unwrap_or_default() };
        }
        InstrToken::Switch { name: c, arg: self.optional_argument().map(Box::new) }
    }

    /// 紧随开关的实参：下一项是开关或到头时没有。
    fn optional_argument(&mut self) -> Option<InstrToken> {
        let save = self.i;
        self.skip_ws();
        match self.peek() {
            Some('\\') | None => {
                self.i = save;
                None
            }
            _ => self.argument(),
        }
    }

    /// `argument := quoted | bare | NESTED`。
    fn argument(&mut self) -> Option<InstrToken> {
        match self.peek()? {
            '"' => Some(self.quoted()),
            NESTED_PLACEHOLDER => {
                self.i += 1;
                let id = self.nested.get(self.n).copied();
                self.n += 1;
                // 占位符多于已知嵌套字段（畸形输入）：当成裸词，不丢字符
                Some(id.map_or_else(
                    || InstrToken::Word(NESTED_PLACEHOLDER.to_string()),
                    InstrToken::Nested,
                ))
            }
            _ => Some(self.bare()),
        }
    }

    /// `quoted := '"' ( '\' any | [^"\\] )* '"'`，`\"` 与 `\\` 是转义。未闭合到结尾为止。
    fn quoted(&mut self) -> InstrToken {
        self.i += 1; // 开引号
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '"' => return InstrToken::Quoted(s),
                // 只有 `\"` 与 `\\` 是转义；别的 `\x` 原样留着——引号里的反斜杠是字面量
                // （Windows 路径 `"file:///C:\Users\u\x"`，吃掉就成了 `C:Usersux`）。
                // 开关只在引号**外**有意义。
                '\\' => match self.peek() {
                    Some(esc @ ('"' | '\\')) => {
                        self.i += 1;
                        s.push(esc);
                    }
                    Some(_) | None => s.push('\\'),
                },
                _ => s.push(c),
            }
        }
        InstrToken::Quoted(s)
    }

    /// `bare := [^ \t\r\n"\\]+`，另外在嵌套占位符处断开。
    fn bare(&mut self) -> InstrToken {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '"' || c == '\\' || c == NESTED_PLACEHOLDER {
                break;
            }
            self.i += 1;
        }
        if start == self.i {
            // 兜底：一个字符也没吃到时强制前进，避免死循环
            self.i += 1;
            return InstrToken::Word(self.chars[start].to_string());
        }
        InstrToken::Word(self.chars[start..self.i].iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &str) -> Instruction {
        parse(s, &[])
    }

    /// `FLD-05` 验收行：`HYPERLINK "http://x" \o "tip"`。
    #[test]
    fn fld_05_hyperlink_with_quoted_target_and_switch() {
        let i = tokens(r#" HYPERLINK "http://x" \o "tip" "#);
        assert_eq!(i.keyword, Keyword::Hyperlink);
        assert_eq!(i.keyword.policy(), FieldPolicy::Link);
        assert_eq!(
            i.tokens,
            vec![
                InstrToken::Quoted("http://x".into()),
                InstrToken::Switch {
                    name: 'o',
                    arg: Some(Box::new(InstrToken::Quoted("tip".into())))
                },
            ]
        );
        assert_eq!(i.first_argument(), Some("http://x"));
        assert_eq!(i.switch('o'), Some("tip"));
        assert_eq!(i.switch('l'), None);
    }

    /// `FLD-05` 验收行：`=SUM(ABOVE) \# "0.00"` → keyword `=`，GeneralFormat{#}。
    #[test]
    fn fld_05_formula_keyword_and_general_format() {
        let i = tokens(r#"=SUM(ABOVE) \# "0.00""#);
        assert_eq!(i.keyword, Keyword::Formula);
        assert_eq!(i.keyword.as_str(), "=");
        assert_eq!(
            i.tokens,
            vec![
                InstrToken::Word("SUM(ABOVE)".into()),
                InstrToken::GeneralFormat { kind: '#', arg: "0.00".into() },
            ]
        );
        assert_eq!(i.general_format('#'), Some("0.00"));
    }

    /// `FLD-05` 验收行：引号内的 `\"`。
    #[test]
    fn fld_05_escapes_inside_quotes() {
        let i = tokens(r#"QUOTE "a \"b\" c\\d""#);
        assert_eq!(i.keyword, Keyword::Quote);
        assert_eq!(i.tokens, vec![InstrToken::Quoted(r#"a "b" c\d"#.into())]);
    }

    #[test]
    fn fld_05_switch_without_argument_and_merge_format() {
        let i = tokens(r#"PAGE \* MERGEFORMAT"#);
        assert_eq!(i.keyword, Keyword::Page);
        assert_eq!(
            i.tokens,
            vec![InstrToken::GeneralFormat { kind: '*', arg: "MERGEFORMAT".into() }]
        );
        let i = tokens(r#"REF _Ref1 \h \* MERGEFORMAT"#);
        assert_eq!(i.keyword, Keyword::Ref);
        assert_eq!(
            i.tokens,
            vec![
                InstrToken::Word("_Ref1".into()),
                InstrToken::Switch { name: 'h', arg: None },
                InstrToken::GeneralFormat { kind: '*', arg: "MERGEFORMAT".into() },
            ]
        );
        assert!(i.has_switch('h'));
        assert_eq!(i.switch('h'), Some(""));
    }

    #[test]
    fn fld_05_toc_switches() {
        let i = tokens(r#"TOC \o "1-3" \h \z \u"#);
        assert_eq!(i.keyword, Keyword::Toc);
        assert_eq!(i.keyword.policy(), FieldPolicy::Block);
        assert_eq!(i.switch('o'), Some("1-3"));
        assert!(i.has_switch('z') && i.has_switch('u'));
    }

    /// 嵌套字段以 `U+FFFC` 出现在 `raw` 里，token 是 `Nested`。
    #[test]
    fn fld_03_nested_placeholder_becomes_a_token() {
        let raw = format!("IF {NESTED_PLACEHOLDER} = 1 \"yes\" \"no\"");
        let i = parse(&raw, &[FieldId(7)]);
        assert_eq!(i.keyword, Keyword::If);
        assert_eq!(i.tokens[0], InstrToken::Nested(FieldId(7)));
        assert_eq!(i.nested().collect::<Vec<_>>(), vec![FieldId(7)]);
    }

    /// tokenizer 对任何输入都不失败（`FLD-05`）。
    #[test]
    fn fld_05_tokenizer_never_fails() {
        for s in [
            "",
            " ",
            "\\",
            "\\\\",
            "\"",
            "\"unterminated",
            "\\9 weird",
            "\u{FFFC}",
            "PAGE\u{FFFC}",
            "= ",
            "\\* ",
            "MERGEFIELD \\b \"a\\",
        ] {
            let i = tokens(s);
            assert_eq!(i.raw, s);
        }
    }

    #[test]
    fn fld_06_policy_table_and_case_insensitive_keywords() {
        assert_eq!(Keyword::parse("page"), Keyword::Page);
        assert_eq!(Keyword::parse("PaGeReF"), Keyword::PageRef);
        assert_eq!(Keyword::parse("XE").policy(), FieldPolicy::Marker);
        assert_eq!(Keyword::parse("INCLUDEPICTURE").policy(), FieldPolicy::Picture);
        assert_eq!(Keyword::parse("EMBED").policy(), FieldPolicy::Object);
        assert_eq!(Keyword::parse("FORMCHECKBOX").policy(), FieldPolicy::Form);
        assert_eq!(Keyword::parse("BIBLIOGRAPHY").policy(), FieldPolicy::Block);
        let unknown = Keyword::parse("acmeprivate");
        assert_eq!(unknown, Keyword::Unknown("ACMEPRIVATE".into()));
        assert_eq!(unknown.policy(), FieldPolicy::Unknown);
        assert_eq!(unknown.as_str(), "ACMEPRIVATE");
    }
}
