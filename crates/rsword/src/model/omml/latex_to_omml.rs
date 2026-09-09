//! LaTeX → OMML（TS `math.ts` 的 `latexToOmml`，`spec/18` 7.5 逐字移植）。
//!
//! 输出与 TS **逐字相等**（`fixtures/fieldgen/` 是对照件）：同样的元素顺序、同样的属性顺序、
//! 同样的转义。这是**用户输入**的解析器，不是文档遍历，所以按 `spec/18` 的约定用递归下降 +
//! 深度上限（256），超限 `Err(EDIT_MATH_TOO_DEEP)` 而不是写显式栈。

use crate::diag::DiagCode;
use crate::error::{Error, Result};

use super::escape_text;

/// 递归深度上限（用户输入，不是文档；`spec/18` 风险 11）。
const MAX_DEPTH: usize = 256;

fn err(msg: impl Into<String>) -> Error {
    Error::edit(DiagCode::EditMathBadLatex, msg)
}

fn too_deep() -> Error {
    Error::edit(DiagCode::EditMathTooDeep, "LaTeX 嵌套超过 256 层")
}

/// TS `escapeXmlAttr`。
fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

struct P<'a> {
    src: &'a [char],
    pos: usize,
    depth: usize,
}

impl P<'_> {
    fn peek(&self) -> char {
        self.src.get(self.pos).copied().unwrap_or('\0')
    }

    fn rest_starts_with(&self, pat: &str) -> bool {
        let p: Vec<char> = pat.chars().collect();
        self.src.len() >= self.pos + p.len() && self.src[self.pos..self.pos + p.len()] == p[..]
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_whitespace() {
            self.pos += 1;
        }
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.src[from.min(self.src.len())..to.min(self.src.len())].iter().collect()
    }

    fn deeper(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(too_deep()) } else { Ok(()) }
    }
}

/// TS `latexToOmml`：整串 LaTeX → `m:oMath` 的**内容**（不含 `m:oMath` 本身）。
pub fn latex_to_omml(latex: &str) -> Result<String> {
    let chars: Vec<char> = latex.chars().collect();
    let mut p = P { src: &chars, pos: 0, depth: 0 };
    let out = parse_sequence(&mut p, &|p: &P<'_>| p.pos >= p.src.len())?;
    if p.pos < p.src.len() {
        return Err(err(format!("Cannot parse: \"{}\"", p.slice(p.pos, p.pos + 12))));
    }
    Ok(out)
}

/// TS `mathParagraphXml`：编辑器新建的独立公式段。
pub fn math_paragraph_xml(omml: &str, align: &str) -> String {
    let jc = if align == "center" {
        String::new()
    } else {
        format!(r#"<w:pPr><w:jc w:val="{}"/></w:pPr>"#, escape_attr(align))
    };
    format!(
        concat!(
            r#"<w:p>{jc}<m:oMathPara><m:oMathParaPr><m:jc m:val="{align}"/></m:oMathParaPr>"#,
            r#"<m:oMath>{omml}</m:oMath></m:oMathPara></w:p>"#
        ),
        jc = jc,
        align = escape_attr(align),
        omml = omml,
    )
}

/// TS `mathRun`。
fn math_run(text: &str, plain: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let rpr = if plain { r#"<m:rPr><m:sty m:val="p"/></m:rPr>"# } else { "" };
    format!(r#"<m:r>{rpr}<m:t xml:space="preserve">{}</m:t></m:r>"#, escape_text(text))
}

/// TS `readControlName`：反斜杠之后的命令名（字母串，否则单个字符）。
fn read_control_name(p: &mut P<'_>) -> String {
    let start = p.pos;
    while p.src.get(p.pos).is_some_and(|c| c.is_ascii_alphabetic()) {
        p.pos += 1;
    }
    if p.pos > start {
        return p.slice(start, p.pos);
    }
    let ch = p.peek();
    p.pos += 1;
    if ch == '\0' { String::new() } else { ch.to_string() }
}

/// TS `parseGroup`：必需的 `{...}`，或者按 LaTeX 语义的**一个** token。
fn parse_group(p: &mut P<'_>) -> Result<String> {
    p.skip_spaces();
    if p.peek() == '{' {
        p.pos += 1;
        p.deeper()?;
        let out = parse_sequence(p, &|p: &P<'_>| p.peek() == '}')?;
        p.depth -= 1;
        if p.peek() != '}' {
            return Err(err("Missing matching }"));
        }
        p.pos += 1;
        return Ok(out);
    }
    if p.peek() == '\\' {
        p.pos += 1;
        p.deeper()?;
        let out = parse_control(p)?;
        p.depth -= 1;
        return Ok(out);
    }
    let ch = p.peek();
    if ch == '\0' || "{}^_&".contains(ch) {
        return Err(err("An argument is required here"));
    }
    p.pos += 1;
    Ok(math_run(&ch.to_string(), false))
}

/// TS `readBraceText`：`{...}` 的原文（`\text` / `\begin` 的名字）。
fn read_brace_text(p: &mut P<'_>) -> Result<String> {
    p.skip_spaces();
    if p.peek() != '{' {
        return Err(err("Expected { here"));
    }
    p.pos += 1;
    let mut depth = 1usize;
    let mut out = String::new();
    while p.pos < p.src.len() {
        let ch = p.src[p.pos];
        p.pos += 1;
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Ok(out);
            }
        }
        if depth > 0 {
            out.push(ch);
        }
    }
    Err(err("Missing matching }"))
}

type Stop<'s> = dyn Fn(&P<'_>) -> bool + 's;

/// TS `parseSequence`：一串原子，`^` / `_` 作用在前一个原子上。
fn parse_sequence(p: &mut P<'_>, stop: &Stop<'_>) -> Result<String> {
    let mut atoms: Vec<String> = Vec::new();
    loop {
        p.skip_spaces();
        if p.pos >= p.src.len() || stop(p) {
            break;
        }
        let ch = p.peek();
        if ch == '^' || ch == '_' {
            p.pos += 1;
            let script = parse_group(p)?;
            let other = p.peek();
            let base = atoms.pop().unwrap_or_else(|| math_run("", false));
            if (other == '^' || other == '_') && other != ch {
                p.pos += 1;
                let second = parse_group(p)?;
                let (sub, sup) = if ch == '_' { (script, second) } else { (second, script) };
                atoms.push(format!(
                    "<m:sSubSup><m:e>{base}</m:e><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup></m:sSubSup>"
                ));
            } else if ch == '^' {
                atoms.push(format!("<m:sSup><m:e>{base}</m:e><m:sup>{script}</m:sup></m:sSup>"));
            } else {
                atoms.push(format!("<m:sSub><m:e>{base}</m:e><m:sub>{script}</m:sub></m:sSub>"));
            }
            continue;
        }
        atoms.push(parse_atom(p)?);
    }
    Ok(atoms.concat())
}

/// TS `parseAtom`。
fn parse_atom(p: &mut P<'_>) -> Result<String> {
    p.skip_spaces();
    let ch = p.peek();
    if ch == '\0' {
        return Ok(String::new());
    }
    if ch == '{' {
        return parse_group(p);
    }
    if ch == '}' {
        return Err(err("Unexpected }"));
    }
    if ch == '\\' {
        p.pos += 1;
        p.deeper()?;
        let out = parse_control(p)?;
        p.depth -= 1;
        return Ok(out);
    }
    let start = p.pos;
    while p.pos < p.src.len() {
        let c = p.src[p.pos];
        if "\\{}^_&".contains(c) || c == '\n' {
            break;
        }
        p.pos += 1;
    }
    let text = p.slice(start, p.pos);
    if text.is_empty() {
        return Err(err(format!("Cannot parse: \"{ch}\"")));
    }
    // 紧跟的上下标只作用在**最后一个字符**上（"ab^2" = a·b²）：退回去让它自成一个原子
    let chars: Vec<char> = text.chars().collect();
    if (p.peek() == '^' || p.peek() == '_') && chars.len() > 1 {
        p.pos -= 1;
        return Ok(math_run(&chars[..chars.len() - 1].iter().collect::<String>(), false));
    }
    Ok(math_run(&text, false))
}

/// TS `naryOmml`。
fn nary_omml(p: &mut P<'_>, chr: &str, lim_loc: &str) -> Result<String> {
    let (mut sub, mut sup) = (String::new(), String::new());
    for _ in 0..2 {
        p.skip_spaces();
        let ch = p.peek();
        if ch == '_' && sub.is_empty() {
            p.pos += 1;
            sub = parse_group(p)?;
        } else if ch == '^' && sup.is_empty() {
            p.pos += 1;
            sup = parse_group(p)?;
        } else {
            break;
        }
    }
    p.skip_spaces();
    let operand = if p.peek() == '{' { parse_group(p)? } else { String::new() };
    let pr = format!(
        r#"<m:naryPr><m:chr m:val="{}"/><m:limLoc m:val="{lim_loc}"/>{}{}</m:naryPr>"#,
        escape_attr(chr),
        if sub.is_empty() { r#"<m:subHide m:val="1"/>"# } else { "" },
        if sup.is_empty() { r#"<m:supHide m:val="1"/>"# } else { "" },
    );
    Ok(format!(
        "<m:nary>{pr}{}{}<m:e>{operand}</m:e></m:nary>",
        if sub.is_empty() { String::new() } else { format!("<m:sub>{sub}</m:sub>") },
        if sup.is_empty() { String::new() } else { format!("<m:sup>{sup}</m:sup>") },
    ))
}

/// TS `matrixOmml`。
fn matrix_omml(p: &mut P<'_>, env: &str) -> Result<String> {
    let delims = matrix_delims(env).expect("caller checked the environment");
    let mut rows: Vec<Vec<String>> = vec![Vec::new()];
    loop {
        let cell = parse_sequence(p, &|p: &P<'_>| {
            p.peek() == '&' || p.rest_starts_with("\\\\") || p.rest_starts_with("\\end")
        })?;
        rows.last_mut().expect("never empty").push(cell);
        if p.peek() == '&' {
            p.pos += 1;
        } else if p.rest_starts_with("\\\\") {
            p.pos += 2;
            rows.push(Vec::new());
        } else if p.rest_starts_with("\\end") {
            p.pos += 4;
            let closing = read_brace_text(p)?;
            if closing != env {
                return Err(err(format!("\\end{{{closing}}} does not match \\begin{{{env}}}")));
            }
            break;
        } else {
            return Err(err(format!("\\begin{{{env}}} is missing \\end{{{env}}}")));
        }
    }
    let body: String = rows
        .iter()
        .filter(|row| row.len() > 1 || row.first().is_some_and(|c| !c.is_empty()))
        .map(|row| {
            let cells: String = row.iter().map(|c| format!("<m:e>{c}</m:e>")).collect();
            format!("<m:mr>{cells}</m:mr>")
        })
        .collect();
    let matrix = format!("<m:m>{body}</m:m>");
    let Some((beg, end)) = delims else { return Ok(matrix) };
    Ok(format!(
        concat!(
            r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
            "</m:dPr><m:e>{matrix}</m:e></m:d>"
        ),
        beg = escape_attr(beg),
        end = escape_attr(end),
        matrix = matrix,
    ))
}

/// TS `readDelimiter`。
fn read_delimiter(p: &mut P<'_>) -> Result<String> {
    p.skip_spaces();
    if p.peek() == '\\' {
        let start = p.pos;
        p.pos += 1;
        let name = read_control_name(p);
        if let Some(ch) = left_right_char(&format!("\\{name}")) {
            return Ok(ch.to_string());
        }
        p.pos = start;
        return Err(err(format!("Unsupported delimiter: \\{name}")));
    }
    let ch = p.peek();
    if let Some(mapped) = left_right_char(&ch.to_string()) {
        p.pos += 1;
        return Ok(mapped.to_string());
    }
    Err(err(format!("Unsupported delimiter: \"{ch}\"")))
}

/// TS `parseControl`。
fn parse_control(p: &mut P<'_>) -> Result<String> {
    let name = read_control_name(p);
    if let Some(ch) = super::latex::symbol_char(&name) {
        return Ok(math_run(&ch.to_string(), false));
    }
    if let Some((chr, lim_loc)) = nary_op(&name) {
        return nary_omml(p, &chr.to_string(), lim_loc);
    }
    if let Some(ch) = super::latex::accent_char(&name) {
        let base = parse_group(p)?;
        return Ok(format!(
            r#"<m:acc><m:accPr><m:chr m:val="{}"/></m:accPr><m:e>{base}</m:e></m:acc>"#,
            escape_attr(&ch.to_string())
        ));
    }
    if super::latex::is_latex_function(&name) {
        return Ok(math_run(&name, true));
    }
    match name.as_str() {
        "frac" | "dfrac" | "tfrac" => {
            let num = parse_group(p)?;
            let den = parse_group(p)?;
            Ok(format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>"))
        }
        "binom" => {
            let top = parse_group(p)?;
            let bottom = parse_group(p)?;
            Ok(format!(
                concat!(
                    r#"<m:d><m:e><m:f><m:fPr><m:type m:val="noBar"/></m:fPr>"#,
                    "<m:num>{top}</m:num><m:den>{bottom}</m:den></m:f></m:e></m:d>"
                ),
                top = top,
                bottom = bottom
            ))
        }
        "sqrt" => {
            p.skip_spaces();
            let mut deg = String::new();
            if p.peek() == '[' {
                // 普通字符串不会在 ']' 停下：把次数的源码单独切出来当一段解析
                p.pos += 1;
                let close = (p.pos..p.src.len()).find(|&i| p.src[i] == ']');
                let Some(close) = close else { return Err(err("Missing matching ]")) };
                let inner: Vec<char> = p.src[p.pos..close].to_vec();
                let mut sub = P { src: &inner, pos: 0, depth: p.depth };
                deg = parse_sequence(&mut sub, &|q: &P<'_>| q.pos >= q.src.len())?;
                p.pos = close + 1;
            }
            let inner = parse_group(p)?;
            if deg.is_empty() {
                return Ok(format!(
                    r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>{inner}</m:e></m:rad>"#
                ));
            }
            Ok(format!("<m:rad><m:deg>{deg}</m:deg><m:e>{inner}</m:e></m:rad>"))
        }
        "overline" => Ok(format!(
            r#"<m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e>{}</m:e></m:bar>"#,
            parse_group(p)?
        )),
        "underline" => Ok(format!(
            r#"<m:bar><m:barPr><m:pos m:val="bot"/></m:barPr><m:e>{}</m:e></m:bar>"#,
            parse_group(p)?
        )),
        "underbrace" => Ok(format!(
            concat!(
                r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏟"/><m:pos m:val="bot"/></m:groupChrPr>"#,
                "<m:e>{}</m:e></m:groupChr>"
            ),
            parse_group(p)?
        )),
        "overbrace" => Ok(format!(
            concat!(
                r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏞"/><m:pos m:val="top"/></m:groupChrPr>"#,
                "<m:e>{}</m:e></m:groupChr>"
            ),
            parse_group(p)?
        )),
        "text" | "mathrm" | "operatorname" => {
            let t = read_brace_text(p)?;
            Ok(math_run(&t, true))
        }
        "lim" => {
            p.skip_spaces();
            if p.peek() == '_' {
                p.pos += 1;
                let lim = parse_group(p)?;
                return Ok(format!(
                    "<m:limLow><m:e>{}</m:e><m:lim>{lim}</m:lim></m:limLow>",
                    math_run("lim", true)
                ));
            }
            Ok(math_run("lim", true))
        }
        "left" => {
            let beg = read_delimiter(p)?;
            p.deeper()?;
            let body = parse_sequence(p, &|p: &P<'_>| p.rest_starts_with("\\right"))?;
            p.depth -= 1;
            if !p.rest_starts_with("\\right") {
                return Err(err("\\left is missing a matching \\right"));
            }
            p.pos += "\\right".chars().count();
            let end = read_delimiter(p)?;
            Ok(format!(
                concat!(
                    r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
                    "</m:dPr><m:e>{body}</m:e></m:d>"
                ),
                beg = escape_attr(&beg),
                end = escape_attr(&end),
                body = body,
            ))
        }
        "begin" => {
            let env = read_brace_text(p)?;
            if matrix_delims(&env).is_none() {
                return Err(err(format!("Unsupported environment: \\begin{{{env}}}")));
            }
            p.deeper()?;
            let out = matrix_omml(p, &env)?;
            p.depth -= 1;
            Ok(out)
        }
        "," | ";" | " " | "quad" | "qquad" => Ok(math_run(" ", false)),
        "\\" => Err(err("\\\\ is only allowed inside matrix environments")),
        "{" => Ok(math_run("{", false)),
        "}" => Ok(math_run("}", false)),
        "%" | "&" | "$" | "#" | "_" | "^" => Ok(math_run(&name, false)),
        other => Err(err(format!("Unsupported command: \\{other}"))),
    }
}

/// TS `NARY_OPS`：符号取自 `latex.rs` 的同一张表（`nary_char`），这里只补 `limLoc`。
fn nary_op(name: &str) -> Option<(char, &'static str)> {
    let chr = super::latex::nary_char(name)?;
    let lim_loc = match name {
        "int" | "iint" | "iiint" | "oint" => "subSup",
        _ => "undOvr",
    };
    Some((chr, lim_loc))
}

/// TS `MATRIX_DELIMS`：外层 `None` = 不是矩阵环境，内层 `None` = 没有定界符。
fn matrix_delims(env: &str) -> Option<Option<(&'static str, &'static str)>> {
    Some(match env {
        "matrix" => None,
        "pmatrix" => Some(("(", ")")),
        "bmatrix" => Some(("[", "]")),
        "Bmatrix" => Some(("{", "}")),
        "vmatrix" => Some(("|", "|")),
        "Vmatrix" => Some(("‖", "‖")),
        "cases" => Some(("{", "")),
        _ => return None,
    })
}

/// TS `LEFT_RIGHT_CHARS`。
fn left_right_char(key: &str) -> Option<&'static str> {
    Some(match key {
        "(" => "(",
        ")" => ")",
        "[" => "[",
        "]" => "]",
        "|" => "|",
        "." => "",
        "\\{" => "{",
        "\\}" => "}",
        "\\|" => "‖",
        "\\langle" => "⟨",
        "\\rangle" => "⟩",
        "\\lfloor" => "⌊",
        "\\rfloor" => "⌋",
        "\\lceil" => "⌈",
        "\\rceil" => "⌉",
        _ => return None,
    })
}
