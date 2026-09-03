//! 包内路径与唯一的路径解析函数（`PKG-06`）。

use std::fmt;

/// 规范化的包内路径：无前导 `/`，`/` 分隔，已做 `.`/`..` 归一与百分号解码。例：`word/document.xml`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartUri(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriError {
    /// `..` 越过包根（`PKG_PATH_ESCAPES_ROOT`）。
    EscapesRoot,
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EscapesRoot => f.write_str("target escapes the package root"),
        }
    }
}

impl PartUri {
    /// 包根（用于解析 `_rels/.rels` 中的目标）。
    pub const ROOT: PartUri = PartUri(String::new());

    /// 直接由 zip 条目名构造（不做归一化：条目名就是真相）。
    pub fn from_entry_name(name: &str) -> Self {
        Self(name.trim_start_matches('/').to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// 所在目录，不含末尾 `/`；根目录下的 part 与包根都返回 `""`。
    pub fn dir(&self) -> &str {
        self.0.rfind('/').map_or("", |i| &self.0[..i])
    }

    pub fn file_name(&self) -> &str {
        self.0.rfind('/').map_or(self.0.as_str(), |i| &self.0[i + 1..])
    }

    /// 扩展名（不含点，原始大小写）。`.rels` 的扩展名是 `rels`。
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rfind('.').map(|i| &name[i + 1..]).filter(|e| !e.is_empty())
    }

    /// 该 part 的关系文件：`<dir>/_rels/<name>.rels`；包根为 `_rels/.rels`。
    pub fn rels_uri(&self) -> PartUri {
        if self.is_root() {
            return PartUri("_rels/.rels".into());
        }
        let dir = self.dir();
        if dir.is_empty() {
            PartUri(format!("_rels/{}.rels", self.file_name()))
        } else {
            PartUri(format!("{dir}/_rels/{}.rels", self.file_name()))
        }
    }

    /// 若本 part 是关系文件，返回其源 part（`_rels/.rels` → 包根）。
    pub fn rels_source(&self) -> Option<PartUri> {
        let name = self.file_name().strip_suffix(".rels")?;
        let dir = self.dir().strip_suffix("_rels")?;
        if name.is_empty() && dir.is_empty() {
            return Some(PartUri::ROOT);
        }
        let dir = dir.strip_suffix('/').unwrap_or(dir);
        Some(if dir.is_empty() {
            PartUri(name.to_string())
        } else {
            PartUri(format!("{dir}/{name}"))
        })
    }

    /// `PKG-04` 的 Override 键形式：`/word/document.xml`。
    pub fn override_key(&self) -> String {
        format!("/{}", self.0)
    }
}

impl fmt::Display for PartUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `PKG-06`：**唯一**的路径解析函数。
///
/// 1. `target` 以 `/` 开头 → 相对包根；否则相对 `base` 所在目录。
/// 2. 按 `/` 分段：空段与 `.` 丢弃，`..` 弹出上一段，弹空 → `EscapesRoot`。
/// 3. 每段百分号解码（`%20` 等；非法序列原样保留）。
///
/// 存在性检查、大小写不敏感回退（`PKG_CASE_INSENSITIVE_MATCH`）由调用方在 zip 条目表上完成。
pub fn resolve(base: &PartUri, target: &str) -> Result<PartUri, UriError> {
    let mut segs: Vec<String> = Vec::new();
    let rest = if let Some(abs) = target.strip_prefix('/') {
        abs
    } else {
        segs.extend(base.dir().split('/').filter(|s| !s.is_empty()).map(str::to_string));
        target
    };
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if segs.pop().is_none() {
                    return Err(UriError::EscapesRoot);
                }
            }
            s => segs.push(percent_decode(s)),
        }
    }
    Ok(PartUri(segs.join("/")))
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PartUri {
        PartUri::from_entry_name(s)
    }

    #[test]
    fn pkg_06_three_spellings_resolve_to_the_same_part() {
        let base = p("word/document.xml");
        let a = resolve(&base, "media/x.png").unwrap();
        let b = resolve(&base, "/word/media/x.png").unwrap();
        let c = resolve(&p("word/_rels/document.xml.rels"), "../media/x.png").unwrap();
        assert_eq!(a, p("word/media/x.png"));
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(resolve(&PartUri::ROOT, "word/document.xml").unwrap(), p("word/document.xml"));
        assert_eq!(resolve(&PartUri::ROOT, "/word/document.xml").unwrap(), p("word/document.xml"));
        assert_eq!(resolve(&base, "./styles.xml").unwrap(), p("word/styles.xml"));
        assert_eq!(resolve(&base, "..//docProps/core.xml").unwrap(), p("docProps/core.xml"));
    }

    #[test]
    fn pkg_06_escape_root_and_percent_decoding() {
        assert_eq!(resolve(&p("word/document.xml"), "../../x").unwrap_err(), UriError::EscapesRoot);
        assert_eq!(resolve(&PartUri::ROOT, "..").unwrap_err(), UriError::EscapesRoot);
        assert_eq!(
            resolve(&p("word/document.xml"), "media/my%20image.png").unwrap(),
            p("word/media/my image.png")
        );
        assert_eq!(
            resolve(&p("word/document.xml"), "media/100%.png").unwrap(),
            p("word/media/100%.png")
        );
    }

    #[test]
    fn pkg_05_rels_uri_and_source() {
        assert_eq!(p("word/document.xml").rels_uri(), p("word/_rels/document.xml.rels"));
        assert_eq!(PartUri::ROOT.rels_uri(), p("_rels/.rels"));
        assert_eq!(p("mimetype").rels_uri(), p("_rels/mimetype.rels"));
        assert_eq!(p("word/_rels/document.xml.rels").rels_source(), Some(p("word/document.xml")));
        assert_eq!(p("_rels/.rels").rels_source(), Some(PartUri::ROOT));
        assert_eq!(p("word/document.xml").rels_source(), None);
        assert_eq!(p("word/media/image1.PNG").extension(), Some("PNG"));
        assert_eq!(p("word/document.xml").override_key(), "/word/document.xml");
        assert_eq!(p("word/document.xml").dir(), "word");
        assert_eq!(p("mimetype").dir(), "");
    }
}
