//! `RES-12` / `TEST-08`：生成 `fixtures/resolve/**/doc.docx`（`spec/16` 任务 5.8）。
//!
//! 这些 fixture 的**文档由我们生成，观察值必须来自真实 Word**：Word 打开后每个 run 到底加不加粗、
//! 第二节页眉显示什么，只有 Word 说了算（ECMA-376 §17.7.3 的奇偶规则与 [MS-OI29500] 记录的
//! Word 偏差对不上，本引擎的 toggle 规则要按实测校准）。每份 fixture 的目录里有：
//!
//! - `doc.docx`：本工具生成，可重复（同样的输入必然同样的字节）。
//! - `expected.toml`：断言表。`verified = false` 的条目 `tests/resolve_fixtures.rs` 只记不断言。
//! - 观察方法与 Word 版本记在 `fixtures/resolve/README.md`。
//!
//! 用法：`cargo run -p gen-fixtures`（写到仓库的 `fixtures/resolve/`；已存在的 `expected.toml`
//! **不覆盖**，免得把填好的观察值冲掉）。

use std::io::Write;
use std::path::{Path, PathBuf};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_HDR: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

/// 一份 fixture：目录名、说明、正文、样式表、额外 part。
struct Fixture {
    dir: &'static str,
    doc: String,
    styles: String,
    extra: Vec<(String, String, &'static str)>,
    expected: String,
}

fn main() {
    let root = repo_root();
    let out = root.join("fixtures/resolve");
    for f in fixtures() {
        let dir = out.join(f.dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let bytes = build(&f);
        write_if_changed(&dir.join("doc.docx"), &bytes);
        let toml = dir.join("expected.toml");
        if toml.exists() {
            println!("keep   {}", toml.display());
        } else {
            std::fs::write(&toml, f.expected.as_bytes()).expect("write toml");
            println!("write  {}", toml.display());
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// 只在内容变了时写：反复运行不应该让 git 里多出改动。
fn write_if_changed(path: &Path, bytes: &[u8]) {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        println!("same   {}", path.display());
        return;
    }
    std::fs::write(path, bytes).expect("write docx");
    println!("write  {}", path.display());
}

fn zip_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // 固定时间戳：生成结果只取决于内容
    let opts = zip::write::SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap());
    for (name, bytes) in files {
        w.start_file(name, opts).expect("start_file");
        w.write_all(bytes).expect("write");
    }
    w.finish().expect("finish").into_inner()
}

fn build(f: &Fixture) -> Vec<u8> {
    let mut overrides = vec![
        ("/word/document.xml".to_string(), CT_MAIN),
        ("/word/styles.xml".to_string(), CT_STYLES),
    ];
    let mut rels = vec![("rIdS".to_string(), format!("{REL}/styles"), "styles.xml".to_string())];
    for (i, (path, _, ct)) in f.extra.iter().enumerate() {
        overrides.push((format!("/{path}"), ct));
        let kind = if *ct == CT_HDR { "header" } else { "footer" };
        let target = path.strip_prefix("word/").unwrap_or(path).to_string();
        rels.push((format!("rIdX{i}"), format!("{REL}/{kind}"), target));
    }
    let ct = format!(
        concat!(
            "{decl}",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            "{ov}</Types>"
        ),
        decl = DECL,
        ov = overrides
            .iter()
            .map(|(p, c)| format!(r#"<Override PartName="{p}" ContentType="{c}"/>"#))
            .collect::<String>()
    );
    let root_rels = format!(
        concat!(
            "{decl}",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="{rel}/officeDocument" Target="word/document.xml"/>"#,
            "</Relationships>"
        ),
        decl = DECL,
        rel = REL
    );
    let doc_rels = format!(
        concat!(
            "{decl}",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            "{items}</Relationships>"
        ),
        decl = DECL,
        items = rels
            .iter()
            .map(|(id, ty, target)| format!(
                r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"/>"#
            ))
            .collect::<String>()
    );
    let mut files = vec![
        ("[Content_Types].xml".to_string(), ct.into_bytes()),
        ("_rels/.rels".to_string(), root_rels.into_bytes()),
        ("word/_rels/document.xml.rels".to_string(), doc_rels.into_bytes()),
        ("word/styles.xml".to_string(), f.styles.clone().into_bytes()),
        ("word/document.xml".to_string(), f.doc.clone().into_bytes()),
    ];
    for (path, xml, _) in &f.extra {
        files.push((path.clone(), xml.clone().into_bytes()));
    }
    zip_of(&files)
}

fn styles(inner: &str) -> String {
    format!(r#"{DECL}<w:styles xmlns:w="{W}">{inner}</w:styles>"#)
}

fn document(body: &str) -> String {
    format!(r#"{DECL}<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}</w:body></w:document>"#)
}

const SECT: &str = concat!(
    r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
    r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#
);

/// 一段：`style` 为段落样式 id，`runs` 是 `(字符样式, 直接 rPr, 文本)`。
fn para(style: Option<&str>, runs: &[(Option<&str>, &str, &str)]) -> String {
    let ppr =
        style.map(|s| format!(r#"<w:pPr><w:pStyle w:val="{s}"/></w:pPr>"#)).unwrap_or_default();
    let body: String = runs
        .iter()
        .map(|(cs, direct, text)| {
            let rstyle = cs.map(|c| format!(r#"<w:rStyle w:val="{c}"/>"#)).unwrap_or_default();
            let rpr = if rstyle.is_empty() && direct.is_empty() {
                String::new()
            } else {
                format!("<w:rPr>{rstyle}{direct}</w:rPr>")
            };
            format!(r#"<w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r>"#)
        })
        .collect();
    format!("<w:p>{ppr}{body}</w:p>")
}

/// `expected.toml` 的骨架：每行一个待观察的断言。
fn expected_runs(doc: &str, rows: &[(&str, &str)]) -> String {
    let mut s = String::new();
    s.push_str("# RES-04 toggle 校准（任务 5.8）。观察值必须来自真实 Word：\n");
    s.push_str("# 在 Word 里打开同目录的 doc.docx，看每段那句话到底加不加粗，把 bold 填成\n");
    s.push_str(
        "# true / false，并把 verified 改成 true。verified = false 的条目测试只记不断言。\n",
    );
    s.push_str("# 观察方法与 Word 版本记在 fixtures/resolve/README.md。\n\n");
    s.push_str(&format!("doc = {doc:?}\n\n"));
    for (para_text, note) in rows {
        s.push_str("[[run]]\n");
        s.push_str(&format!("# {note}\n"));
        s.push_str(&format!("text = {para_text:?}\n"));
        s.push_str("bold = false\n");
        s.push_str("# source 可选：填了就连 bold 的来源一起断言（Direct / ParaStyle:PBold / …）\n");
        s.push_str("# source = \"ParaStyle:PBold\"\n");
        s.push_str("verified = false\n\n");
    }
    s
}

fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();

    // ① 段落样式 b + 字符样式 b：两层都声明 true，奇偶规则说结果是 false，"最具体胜出"说 true
    out.push(Fixture {
        dir: "toggle/para-and-char",
        styles: styles(concat!(
            r#"<w:style w:type="paragraph" w:styleId="PBold"><w:name w:val="P Bold"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
            r#"<w:style w:type="character" w:styleId="CBold"><w:name w:val="C Bold"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
        )),
        doc: document(&format!(
            "{}{}{}",
            para(Some("PBold"), &[(Some("CBold"), "", "para b + char b")]),
            para(Some("PBold"), &[(None, "", "para b only")]),
            SECT
        )),
        extra: Vec::new(),
        expected: expected_runs(
            "toggle/para-and-char",
            &[
                (
                    "para b + char b",
                    "段落样式 b=true 且字符样式 b=true：奇偶规则 → false，最具体胜出 → true",
                ),
                ("para b only", "只有段落样式 b=true：两种规则都说 true"),
            ],
        ),
    });

    // ② docDefaults b + 段落样式 b
    out.push(Fixture {
        dir: "toggle/docdefaults-and-para",
        styles: styles(concat!(
            r#"<w:docDefaults><w:rPrDefault><w:rPr><w:b/></w:rPr></w:rPrDefault></w:docDefaults>"#,
            r#"<w:style w:type="paragraph" w:styleId="PBold"><w:name w:val="P Bold"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
        )),
        doc: document(&format!(
            "{}{}{}",
            para(Some("PBold"), &[(None, "", "docDefaults b + para b")]),
            para(None, &[(None, "", "docDefaults b only")]),
            SECT
        )),
        extra: Vec::new(),
        expected: expected_runs(
            "toggle/docdefaults-and-para",
            &[
                ("docDefaults b + para b", "docDefaults b=true 与段落样式 b=true 异或 → false？"),
                ("docDefaults b only", "只有 docDefaults b=true → true"),
            ],
        ),
    });

    // ③ basedOn 两层都 b
    out.push(Fixture {
        dir: "toggle/based-on-two-levels",
        styles: styles(concat!(
            r#"<w:style w:type="paragraph" w:styleId="Base"><w:name w:val="Base"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
            r#"<w:style w:type="paragraph" w:styleId="Derived"><w:name w:val="Derived"/>"#,
            r#"<w:basedOn w:val="Base"/><w:rPr><w:b/></w:rPr></w:style>"#,
        )),
        doc: document(&format!(
            "{}{}{}",
            para(Some("Derived"), &[(None, "", "basedOn b + derived b")]),
            para(Some("Base"), &[(None, "", "base b only")]),
            SECT
        )),
        extra: Vec::new(),
        expected: expected_runs(
            "toggle/based-on-two-levels",
            &[
                ("basedOn b + derived b", "basedOn 链上两层都 b=true：奇偶规则 → false"),
                ("base b only", "链上一层 b=true → true"),
            ],
        ),
    });

    // ④ 表格样式 firstRow b + 段落样式 b（走 `RES-08` 的表格视图）
    let tbl = format!(
        concat!(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="TBold"/>"#,
            r#"<w:tblLook w:firstRow="1" w:val="0020"/></w:tblPr>"#,
            r#"<w:tblGrid><w:gridCol w:w="4680"/></w:tblGrid>"#,
            "<w:tr><w:tc><w:tcPr><w:tcW w:w=\"4680\" w:type=\"dxa\"/></w:tcPr>{first}</w:tc></w:tr>",
            "<w:tr><w:tc><w:tcPr><w:tcW w:w=\"4680\" w:type=\"dxa\"/></w:tcPr>{rest}</w:tc></w:tr>",
            "</w:tbl>"
        ),
        first = para(Some("PBold"), &[(None, "", "table firstRow b + para b")]),
        rest = para(Some("PBold"), &[(None, "", "table body + para b")]),
    );
    out.push(Fixture {
        dir: "toggle/table-first-row",
        styles: styles(concat!(
            r#"<w:style w:type="paragraph" w:styleId="PBold"><w:name w:val="P Bold"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
            r#"<w:style w:type="table" w:styleId="TBold"><w:name w:val="T Bold"/>"#,
            r#"<w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr></w:style>"#,
        )),
        doc: document(&format!("{tbl}{}{SECT}", para(None, &[(None, "", "after table")]))),
        extra: Vec::new(),
        expected: expected_runs(
            "toggle/table-first-row",
            &[
                ("table firstRow b + para b", "表格样式 firstRow b=true 与段落样式 b=true 叠加"),
                ("table body + para b", "非首行：只有段落样式 b=true → true"),
            ],
        ),
    });

    // ⑤ 直接 `w:b w:val="0"` 覆盖样式
    out.push(Fixture {
        dir: "toggle/direct-off",
        styles: styles(concat!(
            r#"<w:style w:type="paragraph" w:styleId="PBold"><w:name w:val="P Bold"/>"#,
            r#"<w:rPr><w:b/></w:rPr></w:style>"#,
        )),
        doc: document(&format!(
            "{}{}{}",
            para(Some("PBold"), &[(None, r#"<w:b w:val="0"/>"#, "direct b=0 over style b")]),
            para(Some("PBold"), &[(None, "", "style b, no direct")]),
            SECT
        )),
        extra: Vec::new(),
        expected: expected_runs(
            "toggle/direct-off",
            &[
                (
                    "direct b=0 over style b",
                    "直接 w:b w:val=\"0\" 压住样式的 b → false（两种规则一致）",
                ),
                ("style b, no direct", "样式 b=true → true"),
            ],
        ),
    });

    // ⑥ 两节文档，第二节没有页眉引用（`RES-10` 的继承）
    let hdr = format!(
        r#"{DECL}<w:hdr xmlns:w="{W}" xmlns:r="{R}"><w:p><w:r><w:t>第一节页眉</w:t></w:r></w:p></w:hdr>"#
    );
    let body = format!(
        concat!(
            r#"<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdX0"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr>"#,
            r#"<w:r><w:t>第一节正文</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:t>第二节正文</w:t></w:r></w:p>"#,
            "{sect}"
        ),
        sect = SECT
    );
    let mut expected = String::new();
    expected.push_str("# RES-10 节继承校准（任务 5.8）。观察值必须来自真实 Word：\n");
    expected.push_str("# 在 Word 里打开 doc.docx，看**第二页**（第二节）的页眉显示什么。\n");
    expected
        .push_str("# 第二节自己没有 headerReference：Word 会继续显示第一节的页眉，还是空的？\n");
    expected
        .push_str("# 把 header_default 填成看到的文字（空则填 \"\"），verified 改成 true。\n\n");
    expected.push_str("doc = \"sections/inherit-default\"\n\n");
    expected.push_str("[[section]]\nidx = 0\nheader_default = \"第一节页眉\"\ninherited_from = -1\nverified = false\n\n");
    expected.push_str("[[section]]\nidx = 1\nheader_default = \"第一节页眉\"\ninherited_from = 0\nverified = false\n");
    out.push(Fixture {
        dir: "sections/inherit-default",
        styles: styles(""),
        doc: document(&body),
        extra: vec![("word/header1.xml".to_string(), hdr, CT_HDR)],
        expected,
    });

    out
}
