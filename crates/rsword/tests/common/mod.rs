//! 语料发现（`TEST-01` 布局）。集成测试共用。

use std::path::{Path, PathBuf};

/// 仓库根目录（`crates/rsword/../..`）。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

#[allow(dead_code)]
pub fn corpus_dir(kind: &str) -> PathBuf {
    repo_root().join("corpus").join(kind)
}

/// `corpus/<kind>/*.docx`，按文件名排序，保证测试输出稳定。
#[allow(dead_code)]
pub fn docx_paths(kind: &str) -> Vec<PathBuf> {
    let dir = corpus_dir(kind);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "docx"))
        .collect();
    v.sort();
    v
}

/// 最小 docx 加任意辅助 part（`name` 是包内路径，`content` 是整份 XML）。
///
/// `[Content_Types].xml` 用 `Default Extension="xml"`，所以辅助 part 不必逐个声明 Override；
/// 关系也不必写——`Document::rebuild` 找不到关系时按约定路径退路（语料里有这种文档）。
#[allow(dead_code)]
pub fn docx_with_parts(document_body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let base = docx_with_body(document_body);
    let mut zin = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&buf).unwrap();
    }
    for (name, content) in extra {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(content.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// 最小 docx：只有 `[Content_Types].xml` / `_rels/.rels` / `word/document.xml`，
/// `document_body` 是 `w:body` 的内容。集成测试构造精确 XML 用。
#[allow(dead_code)]
pub fn docx_with_body(document_body: &str) -> Vec<u8> {
    use std::io::{Cursor, Write};
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let ct = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
        r#"</Types>"#
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
        r#"</Relationships>"#
    );
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{document_body}</w:body></w:document>"#
    );
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        [("[Content_Types].xml", ct), ("_rels/.rels", rels), ("word/document.xml", doc.as_str())]
    {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// 取 docx 里一个 part 的 DOM（`xpath_asserts!` 用；独立成函数，跳转得到声明处）。
#[allow(dead_code)]
pub fn xpath_dom(docx: &[u8], part: &str) -> rsword::xml::Dom {
    let mut pkg = rsword::package::Package::open(docx).expect("打开 docx");
    let id = pkg.find_name(part).unwrap_or_else(|| panic!("没有 part {part}"));
    pkg.dom(id).expect("解析 part").unwrap_or_else(|| panic!("{part} 不是 XML")).clone()
}

/// `TEST-05` 的一组 XPath 断言：对某个 part 逐条求值，失败信息里带上表达式。
///
/// 期望值写成字符串数组（`eval_strings` 的结果形态：`count()` 是一个数字串，
/// 节点集是各节点的字符串值，`@attr` 是各属性值）。
///
/// ```ignore
/// xpath_asserts!(&saved, "word/document.xml", [
///     ("count(//w:sectPr/w:pgNumType)", ["1"]),
///     ("//w:sectPr/w:pgNumType/@w:fmt", ["upperRoman"]),
///     ("count(//w:hdr)", ["0"]),
/// ]);
/// ```
#[allow(unused_macros)]
macro_rules! xpath_asserts {
    ($docx:expr, $part:expr, [ $( ($expr:expr, $want:expr) ),* $(,)? ]) => {{
        let dom = $crate::common::xpath_dom($docx, $part);
        $({
            let got = rsword::xml::xpath::eval_strings(&dom, $expr)
                .unwrap_or_else(|e| panic!("XPath `{}` 求值失败：{e}", $expr));
            let want: Vec<String> = $want.iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want, "{} 上的 XPath `{}`", $part, $expr);
        })*
    }};
}

#[allow(unused_imports)]
pub(crate) use xpath_asserts;
