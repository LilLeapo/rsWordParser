//! 公式与 ruby（`MOD-11` / `COMPAT-07` / `EDIT-02`，`spec/17` 任务 6.5）。
//!
//! 语料级：公式 / ruby 文档在本域的差异为 0（单元格里的两条 TS 缺陷已按路径登记）。夹具级：TS `math.test.ts`
//! 的 `ommlToMathML` 七例与 `ommlToLatex` 三例照搬（含 Word 生成的带属性包的 OMML、子集之外返回 `None`、特殊字符
//! 转义），外加逐字的 MathML 期望；hostile `omml-deep`（3,000 层）不爆栈；公式原子前后 `InsertText` 后 `m:oMath`
//! 字节原样。

mod common;

#[cfg(feature = "compat-ts")]
use rsword::bind::compat_ts::{
    EmbeddedKind, block_of_path, diff_json, embedded_kind, known_diffs, parsed_doc, split_known,
};
#[cfg(feature = "compat-ts")]
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::model::fragments;
use rsword::model::latex;
use rsword::model::mathml;
#[cfg(feature = "compat-ts")]
use rsword::package::Package;
use rsword::xml::Dom;
#[cfg(feature = "compat-ts")]
use serde_json::{Value, json};

const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const FRACTION: &str = "<m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>";

/// 把一段 OMML 包进带 `m:` 声明的段落里解析；返回 DOM 与其中的 `m:oMath` 片段。
fn omath(inner: &str) -> (Dom, Vec<rsword::xml::NodeId>) {
    let src = format!(
        r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="{M}">{inner}</w:p>"#
    );
    let dom = Dom::parse(rsword::package::PartId(0), src.as_bytes()).expect("parse");
    let frags = fragments(&dom, dom.root());
    (dom, frags)
}

fn to_mathml(inner: &str) -> String {
    let (dom, frags) = omath(inner);
    frags.iter().map(|&f| rsword::model::to_mathml(&dom, f)).collect()
}

fn to_latex(inner: &str) -> Option<String> {
    let (dom, frags) = omath(inner);
    let [only] = frags.as_slice() else { return None };
    rsword::model::to_latex(&dom, *only)
}

#[cfg(feature = "compat-ts")]
fn parsed(bytes: &[u8]) -> Value {
    let mut pkg = Package::open(bytes).expect("open");
    parsed_doc(&mut pkg).expect("parsed_doc")
}

#[cfg(feature = "compat-ts")]
fn body_docx(body: &str) -> Vec<u8> {
    // `docx_with_body` 只声明 `w`；公式与 ruby 段落自己带 `xmlns:m`
    common::docx_with_body(body)
}

/// `COMPAT-07`：语料里每个公式 / ruby 文档在本域的差异为 0。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_07_formula_and_ruby_projection_match_ts_across_the_corpus() {
    let known = known_diffs();
    let (mut docs, mut formulas, mut unknown) = (0, 0, Vec::new());
    for path in common::docx_paths("synthetic") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(path.with_extension("expected.json")) else {
            continue;
        };
        let expected: Value = serde_json::from_str(&text).expect("expected.json");
        let blocks = expected.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
        let mine = |b: &Value| {
            matches!(embedded_kind(b), Some(EmbeddedKind::Formula | EmbeddedKind::Ruby))
        };
        if !blocks.iter().any(mine) {
            continue;
        }
        docs += 1;
        formulas += blocks.iter().filter(|b| b.get("formulaDisplay").is_some()).count();
        let bytes = std::fs::read(&path).unwrap();
        let actual = parsed(&bytes);
        let mut diffs = Vec::new();
        diff_json(&expected, &actual, &mut diffs);
        let (diffs, _) = split_known(diffs, &file, &known);
        for d in diffs.into_iter().filter(|d| block_of_path(&d.path, &expected).is_some_and(mine)) {
            unknown.push(format!("{file}: {} TS={:?} ours={:?}", d.path, d.expected, d.actual));
        }
    }
    eprintln!("math: {docs} 份文档，{formulas} 个 formulaDisplay");
    for u in unknown.iter().take(20) {
        eprintln!("math: DIFF {u}");
    }
    assert!(docs >= 50 && formulas >= 40, "{docs} / {formulas}");
    assert!(unknown.is_empty(), "{} 处差异", unknown.len());
}

/// TS `math.test.ts` 的 `ommlToMathML` 七例（断言与 TS 相同，再钉住逐字的输出）。
#[test]
fn mod_11_omml_to_mathml_fixtures() {
    // 分式
    let frac = to_mathml(FRACTION);
    assert_eq!(
        frac,
        r#"<math display="block"><mrow><mfrac><mrow><mi>a</mi></mrow><mrow><mi>b</mi></mrow></mfrac></mrow></math>"#
    );
    // 上标、根式、定界符
    let m = to_mathml(concat!(
        "<m:oMath>",
        "<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>",
        r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>y</m:t></m:r></m:e></m:rad>"#,
        "<m:d><m:e><m:r><m:t>z</m:t></m:r></m:e></m:d>",
        "</m:oMath>"
    ));
    for needle in ["<msup>", "<mn>2</mn>", "<msqrt>", r#"<mo stretchy="true">(</mo>"#] {
        assert!(m.contains(needle), "{needle} in {m}");
    }
    assert_eq!(
        m,
        r#"<math display="block"><mrow><msup><mrow><mi>x</mi></mrow><mrow><mn>2</mn></mrow></msup><msqrt><mrow><mi>y</mi></mrow></msqrt><mrow><mo stretchy="true">(</mo><mrow><mi>z</mi></mrow><mo stretchy="true">)</mo></mrow></mrow></math>"#
    );
    // n 元运算符，上下限
    let m = to_mathml(concat!(
        r#"<m:oMath><m:nary><m:naryPr><m:chr m:val="∑"/><m:limLoc m:val="undOvr"/></m:naryPr>"#,
        "<m:sub><m:r><m:t>k=0</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>",
        "<m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary></m:oMath>"
    ));
    assert!(m.contains("<munderover>") && m.contains("∑"), "{m}");
    assert_eq!(
        m,
        r#"<math display="block"><mrow><mrow><munderover><mo stretchy="false">∑</mo><mrow><mi>k</mi><mo>=</mo><mn>0</mn></mrow><mrow><mi>n</mi></mrow></munderover><mrow><mi>k</mi></mrow></mrow></mrow></math>"#
    );
    // 矩阵
    let m = to_mathml(concat!(
        "<m:oMath><m:m>",
        "<m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>0</m:t></m:r></m:e></m:mr>",
        "<m:mr><m:e><m:r><m:t>0</m:t></m:r></m:e><m:e><m:r><m:t>1</m:t></m:r></m:e></m:mr>",
        "</m:m></m:oMath>"
    ));
    assert_eq!(m.matches("<mtr>").count(), 2);
    assert_eq!(m.matches("<mtd>").count(), 4);
    // mn / mi / mo 分类
    let m = to_mathml("<m:oMath><m:r><m:t>2x+1</m:t></m:r></m:oMath>");
    assert_eq!(
        m,
        r#"<math display="block"><mrow><mn>2</mn><mi>x</mi><mo>+</mo><mn>1</mn></mrow></math>"#
    );
    // 同段多个片段 → 多个 <math>
    let m = to_mathml(&format!("{FRACTION}{FRACTION}"));
    assert_eq!(m.matches("<math ").count(), 2);
    // oMathPara 展开
    let (dom, frags) = omath(&format!("<m:oMathPara>{FRACTION}</m:oMathPara>"));
    assert_eq!(frags.len(), 1);
    assert!(mathml::to_mathml(&dom, frags[0]).contains("<mfrac>"));
    // 普通文字 run 整段是 mi；括号在普通 run 里不伸缩；空格跳过；未知字符是 mtext；实体转义
    let m = to_mathml(
        r#"<m:oMath><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r><m:r><m:t>(a &lt; b) ^</m:t></m:r></m:oMath>"#,
    );
    assert_eq!(
        m,
        r#"<math display="block"><mrow><mi>sin</mi><mo stretchy="false">(</mo><mi>a</mi><mo>&lt;</mo><mi>b</mi><mo stretchy="false">)</mo><mtext>^</mtext></mrow></math>"#
    );
    assert_eq!(to_mathml("<m:oMath/>"), "", "没有内容 → 空串");
}

/// TS `math.test.ts` 的 `ommlToLatex` 三例 + 子集内的其他结构。
#[test]
fn mod_11_omml_to_latex_fixtures() {
    // Word 生成的带属性包的 OMML
    let word = concat!(
        "<m:oMath><m:sSup><m:sSupPr><m:ctrlPr/></m:sSupPr>",
        r#"<m:e><m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>x</m:t></m:r></m:e>"#,
        "<m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>",
        "<m:r><m:t>+1=0</m:t></m:r></m:oMath>"
    );
    assert_eq!(to_latex(word).as_deref(), Some("{x}^{2}+1=0"));
    // 子集之外：sPre；两个片段
    assert_eq!(
        to_latex(
            "<m:oMath><m:sPre><m:sub><m:r><m:t>a</m:t></m:r></m:sub><m:e><m:r><m:t>X</m:t></m:r></m:e></m:sPre></m:oMath>"
        ),
        None
    );
    assert_eq!(to_latex(&format!("{FRACTION}{FRACTION}")), None);
    // 特殊字符转义（`100%_x` → 编译回去 token 不变）
    assert_eq!(
        to_latex("<m:oMath><m:r><m:t>100%_x</m:t></m:r></m:oMath>").as_deref(),
        Some("100\\% \\_ x")
    );
    // 分式 / 根式 / 矩阵环境 / \left \right / n 元 / 函数 / lim / 重音 / 括线 / 花括号 / 符号
    assert_eq!(to_latex(FRACTION).as_deref(), Some("\\frac{a}{b}"));
    assert_eq!(
        to_latex("<m:oMath><m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath>").as_deref(),
        Some("\\sqrt[3]{x}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:m><m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>0</m:t></m:r></m:e></m:mr><m:mr><m:e><m:r><m:t>0</m:t></m:r></m:e><m:e><m:r><m:t>1</m:t></m:r></m:e></m:mr></m:m></m:e></m:d></m:oMath>"#).as_deref(),
        Some("\\begin{pmatrix} 1 & 0 \\\\ 0 & 1 \\end{pmatrix}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:d><m:dPr><m:begChr m:val="{"/><m:endChr m:val=""/></m:dPr><m:e><m:eqArr><m:e><m:r><m:t>x</m:t></m:r></m:e><m:e><m:r><m:t>-x</m:t></m:r></m:e></m:eqArr></m:e></m:d></m:oMath>"#).as_deref(),
        Some("\\begin{cases} x \\\\ -x \\end{cases}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="⌉"/></m:dPr><m:e><m:r><m:t>y</m:t></m:r></m:e></m:d></m:oMath>"#).as_deref(),
        Some("\\left[ y \\right\\rceil")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:nary><m:naryPr><m:chr m:val="∑"/></m:naryPr><m:sub><m:r><m:t>k=0</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary></m:oMath>"#).as_deref(),
        Some("\\sum_{k=0}^{n} {k}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:func><m:fName><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>α</m:t></m:r></m:e></m:func></m:oMath>"#).as_deref(),
        Some("\\sin {\\alpha }")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:limLow><m:e><m:r><m:t>lim</m:t></m:r></m:e><m:lim><m:r><m:t>x→0</m:t></m:r></m:lim></m:limLow></m:oMath>"#).as_deref(),
        Some("\\lim_{x\\to 0}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:acc><m:accPr><m:chr m:val="̂"/></m:accPr><m:e><m:r><m:t>y</m:t></m:r></m:e></m:acc><m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e><m:r><m:t>z</m:t></m:r></m:e></m:bar><m:groupChr><m:e><m:r><m:t>a+b</m:t></m:r></m:e></m:groupChr></m:oMath>"#).as_deref(),
        Some("\\hat{y}\\overline{z}\\underbrace{a+b}")
    );
    assert_eq!(
        to_latex(r#"<m:oMath><m:d><m:e><m:f><m:fPr><m:type m:val="noBar"/></m:fPr><m:num><m:r><m:t>n</m:t></m:r></m:num><m:den><m:r><m:t>k</m:t></m:r></m:den></m:f></m:e></m:d></m:oMath>"#).as_deref(),
        Some("\\binom{n}{k}")
    );
    // 普通文字 run：函数名 / lim / \text；含花括号或反斜杠 → 子集之外
    assert_eq!(
        to_latex(r#"<m:oMath><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>速度</m:t></m:r><m:r><m:t>=</m:t></m:r></m:oMath>"#).as_deref(),
        Some("\\text{速度}=")
    );
    assert_eq!(
        to_latex(
            r#"<m:oMath><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>a{b}</m:t></m:r></m:oMath>"#
        ),
        None
    );
    assert_eq!(to_latex(r#"<m:oMath><m:r><m:t>a\b</m:t></m:r></m:oMath>"#), None);
    // 认不出的 n 元运算符 / 重音 / 定界符 → None
    assert_eq!(
        to_latex(
            r#"<m:oMath><m:nary><m:naryPr><m:chr m:val="⨁"/></m:naryPr><m:e/></m:nary></m:oMath>"#
        ),
        None
    );
    assert_eq!(
        to_latex(
            r#"<m:oMath><m:d><m:dPr><m:begChr m:val="«"/></m:dPr><m:e><m:r><m:t>y</m:t></m:r></m:e></m:d></m:oMath>"#
        ),
        None
    );
}

/// 公式块与文字夹公式的投影；`mathml` 只在没有可见正文时给；`omml` 是原字节；多片段没有 `latex`。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_07_formula_blocks_and_inline_math_runs() {
    let m_decl = format!(r#"xmlns:m="{M}""#);
    let body = format!(
        concat!(
            r#"<w:p {m}>{frac}</w:p>"#,
            r#"<w:p {m}><m:oMathPara>{frac}</m:oMathPara><w:r><w:t xml:space="preserve"> 其中 a≠0</w:t></w:r></w:p>"#,
            r#"<w:p {m}><w:r><w:t xml:space="preserve">see </w:t></w:r>{frac}<w:r><w:t xml:space="preserve"> here</w:t></w:r></w:p>"#,
            r#"<w:p {m}>{frac}{frac}</w:p>"#
        ),
        m = m_decl,
        frac = FRACTION
    );
    let v = parsed(&body_docx(&body));
    let b = &v["blocks"][0];
    assert_eq!(b["type"], "passthrough");
    assert_eq!(b["label"], "Equation");
    assert_eq!(b["previewText"], "ab");
    assert_eq!(b["formulaDisplay"]["tokens"], json!(["a", "b"]));
    assert_eq!(b["formulaDisplay"]["omml"], FRACTION);
    assert_eq!(b["formulaDisplay"]["latex"], "\\frac{a}{b}");
    assert!(b["formulaDisplay"]["mathml"].as_str().is_some_and(|m| m.contains("<mfrac>")));
    // oMathPara 旁还有正文：仍是公式块，但没有 mathml（TS 保留平铺 token 条）
    let b = &v["blocks"][1];
    assert_eq!(b["label"], "Equation");
    assert_eq!(b["previewText"], "ab");
    assert!(b["formulaDisplay"].get("mathml").is_none(), "{b}");
    assert_eq!(b["formulaDisplay"]["latex"], "\\frac{a}{b}");
    // 文字夹公式：三个 run
    let b = &v["blocks"][2];
    assert_eq!(b["type"], "paragraph");
    let runs = b["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0], json!({ "text": "see " }));
    assert_eq!(runs[1], json!({ "text": "ab", "math": { "omml": FRACTION } }));
    assert_eq!(runs[2], json!({ "text": " here" }));
    // 两个片段：mathml 两个 <math>，omml 拼接，没有 latex
    let fd = &v["blocks"][3]["formulaDisplay"];
    assert_eq!(fd["tokens"], json!(["a", "b", "a", "b"]));
    assert_eq!(fd["omml"], format!("{FRACTION}{FRACTION}"));
    assert_eq!(fd["mathml"].as_str().map(|m| m.matches("<math ").count()), Some(2));
    assert!(fd.get("latex").is_none(), "{fd}");
}

/// ruby run：`text` 是被注正文，`rt` 不进正文，`xml` 是整个 `w:ruby` 原字节，不带格式键。
#[test]
#[cfg(feature = "compat-ts")]
fn compat_07_ruby_runs_carry_rt_and_raw_xml() {
    let ruby = |base: &str, rt: &str| {
        format!(
            r#"<w:ruby><w:rubyPr><w:rubyAlign w:val="center"/><w:hps w:val="10"/></w:rubyPr><w:rt><w:r><w:rPr><w:sz w:val="10"/></w:rPr><w:t>{rt}</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>{base}</w:t></w:r></w:rubyBase></w:ruby>"#
        )
    };
    let body = format!(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr>{}</w:r><w:r>{}</w:r><w:r><w:t>,</w:t></w:r></w:p>"#,
        ruby("床", "chuáng"),
        ruby("前", "qián")
    );
    let v = parsed(&body_docx(&body));
    let runs = v["blocks"][0]["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0]["text"], "床");
    assert_eq!(runs[0]["ruby"]["rt"], "chuáng");
    assert_eq!(runs[0]["ruby"]["xml"], ruby("床", "chuáng"));
    assert!(
        runs[0].get("bold").is_none() && runs[0].get("rawRPr").is_none(),
        "TS 的 ruby run 不带格式：{}",
        runs[0]
    );
    assert_eq!(runs[1]["text"], "前");
    assert_eq!(runs[2], json!({ "text": "," }));
    // 段落文字（TS plainText）仍含注音：那是 w:t 的原文
    let mut pkg = Package::open(&body_docx(&body)).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let tb = doc.main.iter().find_map(|b| b.as_text()).expect("text block");
    assert_eq!(tb.text(), "\u{FFFC}\u{FFFC},", "坐标流里每个 ruby 是 1 个原子");
}

/// hostile `omml-deep`（3,000 层嵌套的分式）：解析、投影、两个转换器都不爆栈；结果的深度与输入一致。
#[test]
#[cfg(feature = "compat-ts")]
fn test_09_deeply_nested_omml_converts_iteratively() {
    let bytes = std::fs::read(common::corpus_dir("hostile").join("omml-deep.docx")).unwrap();
    let v = parsed(&bytes);
    let b = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b.get("formulaDisplay").is_some())
        .expect("公式块");
    let mathml = b["formulaDisplay"]["mathml"].as_str().expect("mathml");
    assert!(mathml.matches("<mfrac").count() >= 1000, "{}", mathml.len());
    // 构造 4,000 层：转换器自己的栈
    let depth = 4000;
    let mut s = String::new();
    for _ in 0..depth {
        s.push_str("<m:f><m:num>");
    }
    s.push_str("<m:r><m:t>x</m:t></m:r>");
    for _ in 0..depth {
        s.push_str("</m:num><m:den><m:r><m:t>1</m:t></m:r></m:den></m:f>");
    }
    let (dom, frags) = omath(&format!("<m:oMath>{s}</m:oMath>"));
    let m = rsword::model::to_mathml(&dom, frags[0]);
    assert_eq!(m.matches("<mfrac>").count(), depth);
    let l = rsword::model::to_latex(&dom, frags[0]).expect("latex");
    assert_eq!(l.matches("\\frac").count(), depth);
}

/// `EDIT-02`：文字夹公式的段落里，`m:oMath` 是 1 个原子；在它前后插字后保存，公式字节原样。
#[test]
#[cfg(feature = "compat-ts")]
fn edit_02_text_edits_step_around_the_math_atom() {
    let body = format!(
        r#"<w:p xmlns:m="{M}"><w:r><w:t>ab</w:t></w:r>{FRACTION}<w:r><w:t>cd</w:t></w:r></w:p>"#
    );
    let bytes = body_docx(&body);
    let mut s = EditSession::open(&bytes).expect("open");
    let p = s.nth_text_block(0).expect("paragraph").node;
    assert_eq!(s.nth_text_block(0).unwrap().text(), "ab\u{FFFC}cd");
    let ctx = EditContext::default();
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 2), text: "X".into(), props: None }, &ctx)
        .unwrap();
    s.apply(EditOp::InsertText { at: InlinePos::new(p, 4), text: "Y".into(), props: None }, &ctx)
        .unwrap();
    assert_eq!(s.nth_text_block(0).unwrap().text(), "abX\u{FFFC}Ycd");
    let saved = s.save().expect("save");
    let mut pkg = Package::open(&saved).expect("reopen");
    let main = pkg.main_part();
    let xml = pkg.dom(main).unwrap().unwrap().src().to_string();
    assert!(xml.contains(FRACTION), "m:oMath 原字节原样：{xml}");
    let v = parsed(&saved);
    let runs = v["blocks"][0]["runs"].as_array().unwrap();
    let texts: Vec<&str> = runs.iter().map(|r| r["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["abX", "ab", "Ycd"]);
    assert_eq!(runs[1]["math"]["omml"], FRACTION);
}
