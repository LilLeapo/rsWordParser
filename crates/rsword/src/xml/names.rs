//! `NsId` / `LocalName` / `QName`（`XML-05`）。枚举体由 `build.rs` 从 `schema/` 生成。

use std::fmt;

use crate::package::PartFlavor;
use crate::xml::interner::{Interned, Interner};

include!(concat!(env!("OUT_DIR"), "/names.rs"));

/// 语义身份：命名空间 + 局部名。原始写法（前缀）在 `Lex.name` / `Attr.lex_name`，不在这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    pub ns: NsId,
    pub local: LocalName,
}

impl QName {
    pub const fn new(ns: NsId, local: LocalName) -> Self {
        Self { ns, local }
    }

    /// `w:p` 一类的常量写法：`QName::w(LocalName::P)`。
    pub const fn w(local: LocalName) -> Self {
        Self { ns: NsId::W, local }
    }

    /// 可读形式（`w:p`、`{uri}local`、`local`），用于诊断与测试。
    pub fn display<'a>(&'a self, interner: &'a Interner) -> QNameDisplay<'a> {
        QNameDisplay { q: self, interner }
    }
}

impl LocalName {
    /// 查表，表外则 intern 为 `Other`。
    pub fn intern(s: &str, interner: &mut Interner) -> LocalName {
        LocalName::known(s).unwrap_or_else(|| LocalName::Other(interner.intern(s)))
    }

    pub fn as_str<'a>(&self, interner: &'a Interner) -> &'a str {
        match self {
            LocalName::Other(id) => interner.resolve(*id),
            known => known.known_str().expect("known local name has a str"),
        }
    }
}

impl NsId {
    /// URI → 身份：表内直接命中，表外 intern 为 `Other`。
    pub fn intern_uri(uri: &str, interner: &mut Interner) -> (NsId, Option<PartFlavor>) {
        match NsId::from_uri(uri) {
            Some((id, flavor)) => (id, Some(flavor)),
            None => (NsId::Other(interner.intern(uri)), None),
        }
    }

    /// 可读形式：已知命名空间用规范前缀，`Other` 用 URI，`Unbound` 用前缀。
    pub fn describe<'a>(&self, interner: &'a Interner) -> std::borrow::Cow<'a, str> {
        match self {
            NsId::None => "".into(),
            NsId::Unbound(p) => format!("?{}", interner.resolve(*p)).into(),
            NsId::Other(u) => format!("{{{}}}", interner.resolve(*u)).into(),
            known => known.canonical_prefix().unwrap_or("").into(),
        }
    }
}

pub struct QNameDisplay<'a> {
    q: &'a QName,
    interner: &'a Interner,
}

impl fmt::Display for QNameDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ns = self.q.ns.describe(self.interner);
        if ns.is_empty() {
            f.write_str(self.q.local.as_str(self.interner))
        } else if ns.starts_with('{') {
            write!(f, "{ns}{}", self.q.local.as_str(self.interner))
        } else {
            write!(f, "{ns}:{}", self.q.local.as_str(self.interner))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_05_strict_and_transitional_same_nsid() {
        let t = NsId::from_uri("http://schemas.openxmlformats.org/wordprocessingml/2006/main");
        let s = NsId::from_uri("http://purl.oclc.org/ooxml/wordprocessingml/main");
        assert_eq!(t, Some((NsId::W, PartFlavor::Transitional)));
        assert_eq!(s, Some((NsId::W, PartFlavor::Strict)));
        assert_eq!(
            NsId::W.uri(PartFlavor::Strict),
            Some("http://purl.oclc.org/ooxml/wordprocessingml/main")
        );
        assert_eq!(NsId::Mc.uri(PartFlavor::Strict), NsId::Mc.uri(PartFlavor::Transitional));
        assert!(NsId::W.has_strict_uri());
        assert!(!NsId::W14.has_strict_uri());
    }

    #[test]
    fn xml_05_xml_and_xmlns_are_distinct() {
        assert_ne!(NsId::Xml, NsId::Xmlns);
        assert_eq!(
            NsId::from_uri("http://www.w3.org/XML/1998/namespace").map(|x| x.0),
            Some(NsId::Xml)
        );
        assert_eq!(NsId::from_uri("http://www.w3.org/2000/xmlns/").map(|x| x.0), Some(NsId::Xmlns));
        assert_eq!(NsId::Ct.canonical_prefix(), Some(""));
        assert_eq!(NsId::W.canonical_prefix(), Some("w"));
    }

    #[test]
    fn xml_05_local_names_roundtrip_and_other() {
        assert_eq!(LocalName::known("pPr"), Some(LocalName::PPr));
        assert_eq!(LocalName::PPr.known_str(), Some("pPr"));
        assert_eq!(LocalName::known("Default"), Some(LocalName::UDefault));
        assert_eq!(LocalName::known("default"), Some(LocalName::Default));
        let known = LocalName::KNOWN_COUNT;
        assert!(known > 600, "name table shrank to {known}");
        let mut i = Interner::new();
        let o = LocalName::intern("veryUnknownName", &mut i);
        assert!(matches!(o, LocalName::Other(_)));
        assert_eq!(o.as_str(&i), "veryUnknownName");
        let q = QName::new(NsId::W, LocalName::P);
        assert_eq!(q.display(&i).to_string(), "w:p");
        let (other_ns, fl) = NsId::intern_uri("urn:example", &mut i);
        assert_eq!(fl, None);
        assert_eq!(QName::new(other_ns, o).display(&i).to_string(), "{urn:example}veryUnknownName");
    }
}
