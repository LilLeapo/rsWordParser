//! 语料发现（`TEST-01` 布局）。集成测试共用。

pub mod fingerprint;

use std::path::{Path, PathBuf};

/// 确定性伪随机（xorshift64*）：随机序列测试失败时靠种子复现。
#[allow(dead_code)]
pub struct Rng(pub u64);

#[allow(dead_code)]
impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `0..n`（`n == 0` 时给 0）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    /// 从切片里随机取一个。
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        xs.get(self.below(xs.len()))
    }

    /// `n` 分之一的概率为真。
    pub fn chance(&mut self, n: usize) -> bool {
        self.below(n.max(1)) == 0
    }
}

/// 仓库根目录（`crates/rsword/../..`）。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

#[allow(dead_code)]
pub fn corpus_dir(kind: &str) -> PathBuf {
    repo_root().join("corpus").join(kind)
}

/// `corpus/<kind>/**/*.docx`（递归：`corpus/real` 按域分目录），按路径排序，保证测试输出稳定。
/// 名为 `edited` 的目录跳过：那是本引擎写出来等 Word 验收的产物（`tests/real_edits.rs`），不是语料。
#[allow(dead_code)]
pub fn docx_paths(kind: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![corpus_dir(kind)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "edited") {
                    continue;
                }
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "docx") {
                v.push(p);
            }
        }
    }
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

/// 1×1 的 PNG（base64）：测试里塞真实图片字节用，配 [`b64`] / [`with_binary_part`]。
#[allow(dead_code)]
pub const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// base64 解码（测试里塞真实 PNG 字节用）。
#[allow(dead_code)]
pub fn b64(s: &str) -> Vec<u8> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes().filter(|&c| c != b'=') {
        let Some(v) = T.iter().position(|&t| t == c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// 往一份 docx 里追加一个二进制 part。
#[allow(dead_code)]
pub fn with_binary_part(docx: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zin = zip::ZipArchive::new(Cursor::new(docx.to_vec())).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let n = f.name().to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
        w.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&buf).unwrap();
    }
    w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
    w.write_all(data).unwrap();
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

/// zip 里一个条目的原字节（没有这个条目时是空）。
#[allow(dead_code)]
pub fn part_bytes(docx: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(docx)).expect("zip");
    match zip.by_name(name) {
        Ok(mut f) => {
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut out).expect("read");
            out
        }
        Err(_) => Vec::new(),
    }
}

/// `--via js` 的产物目录门控（M8′ 8.0②）：node 侧脚本（`tools/js-parity/`）先把绑定输出
/// 落到 `$RSWORD_JS_SAVE_DIR` / `$RSWORD_JS_BLANK_DIR`，Rust 测试只比字节。缺省的
/// `cargo test` 不依赖 node：没设变量就跳过**并打印一行**，绝不无声 PASS。
/// CI 的门控步骤额外设 `RSWORD_JS_PARITY_REQUIRED=1`——它置位而产物变量缺失时直接断言失败，
/// 防止「配置丢了、门却绿着」（评审复盘：门不该能悄悄通过）。
#[allow(unused_macros)]
macro_rules! via_js_dir {
    ($var:literal) => {
        match std::env::var_os($var) {
            Some(v) if !v.is_empty() => std::path::PathBuf::from(v),
            _ => {
                assert!(
                    std::env::var_os("RSWORD_JS_PARITY_REQUIRED").is_none(),
                    "RSWORD_JS_PARITY_REQUIRED 置位但 {} 没设——node 等价门不该静默跳过",
                    $var
                );
                eprintln!(
                    "via_js_dir: 跳过（未设 {}；先跑 tools/js-parity/ 落产物再设变量才执行这门）",
                    $var
                );
                return;
            }
        }
    };
}

#[allow(unused_imports)]
pub(crate) use via_js_dir;

/// 保存差分用例：`corpus/synthetic` 下文件名含 `.save.` 的 JSON，按文件名排序。
/// `tests/save_blocks.rs` 的原生差分与绑定差分共用这一份发现逻辑。
#[allow(dead_code)]
pub fn save_cases() -> Vec<PathBuf> {
    let dir = corpus_dir("synthetic");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus/synthetic")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(".save.")))
        .collect();
    files.sort();
    files
}

/// 绑定产物（成功路径）的字节；没有就说明两侧行为不一致，直接 panic。
#[allow(dead_code)]
pub fn binding_bytes(dir: &Path, name: &str) -> Vec<u8> {
    match std::fs::read(dir.join(name)) {
        Ok(b) => b,
        Err(_) => {
            let err = std::fs::read_to_string(dir.join(format!("{name}.err")))
                .unwrap_or_else(|_| "<连 .err 都没有>".to_string());
            panic!("绑定产物 {name} 缺失（绑定侧: {err}）")
        }
    }
}

/// 绑定产物（被拒路径）的错误码；产物居然是字节就 panic。
#[allow(dead_code)]
pub fn binding_error_code(dir: &Path, name: &str) -> String {
    let err = std::fs::read_to_string(dir.join(format!("{name}.err")))
        .unwrap_or_else(|_| panic!("用例 {name} 原生被拒，绑定侧却没写 .err"));
    serde_json::from_str::<serde_json::Value>(&err)
        .unwrap_or_else(|e| panic!(".err 不是 JSON: {e}: {err}"))["code"]
        .as_str()
        .expect("code 是字符串")
        .to_string()
}
