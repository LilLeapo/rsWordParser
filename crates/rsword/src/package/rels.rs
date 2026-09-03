//! 关系（`PKG-05`、`PKG-07`）。

use std::collections::HashMap;

use crate::diag::{DiagCode, Diagnostic};
use crate::package::uri::{self, PartUri};
use crate::package::{PartFlavor, PartId};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

const T_FAMILY: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const S_FAMILY: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
const PKG_FAMILY: &str = "http://schemas.openxmlformats.org/package/2006/relationships/";
const MS_2007: &str = "http://schemas.microsoft.com/office/2007/relationships/";
const MS_2011: &str = "http://schemas.microsoft.com/office/2011/relationships/";
const MS_2014: &str = "http://schemas.microsoft.com/office/2014/relationships/";
const MS_2016_09: &str = "http://schemas.microsoft.com/office/2016/09/relationships/";
const MS_2018_08: &str = "http://schemas.microsoft.com/office/2018/08/relationships/";

/// 关系类型。Transitional 与 Strict 两族 URI 映射到同一值（`PKG-07`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelType {
    OfficeDocument,
    Styles,
    Numbering,
    Settings,
    WebSettings,
    FontTable,
    Theme,
    Header,
    Footer,
    Footnotes,
    Endnotes,
    Comments,
    CommentsExtended,
    CommentsIds,
    CommentsExtensible,
    People,
    Image,
    Hyperlink,
    Chart,
    ChartEx,
    ChartUserShapes,
    Package,
    DiagramData,
    DiagramLayout,
    DiagramQuickStyle,
    DiagramColors,
    DiagramDrawing,
    CustomXml,
    CustomXmlProps,
    OleObject,
    GlossaryDocument,
    CoreProperties,
    ExtendedProperties,
    CustomProperties,
    Thumbnail,
    Other,
}

/// (类型, Transitional 尾部, Strict 尾部)；Strict 尾部为 `None` 表示与 Transitional 相同。
const DUAL: &[(RelType, &str, Option<&str>)] = &[
    (RelType::OfficeDocument, "officeDocument", None),
    (RelType::Styles, "styles", None),
    (RelType::Numbering, "numbering", None),
    (RelType::Settings, "settings", None),
    (RelType::WebSettings, "webSettings", None),
    (RelType::FontTable, "fontTable", None),
    (RelType::Theme, "theme", None),
    (RelType::Header, "header", None),
    (RelType::Footer, "footer", None),
    (RelType::Footnotes, "footnotes", None),
    (RelType::Endnotes, "endnotes", None),
    (RelType::Comments, "comments", None),
    (RelType::Image, "image", None),
    (RelType::Hyperlink, "hyperlink", None),
    (RelType::Chart, "chart", None),
    (RelType::ChartUserShapes, "chartUserShapes", None),
    (RelType::Package, "package", None),
    (RelType::DiagramData, "diagramData", None),
    (RelType::DiagramLayout, "diagramLayout", None),
    (RelType::DiagramQuickStyle, "diagramQuickStyle", None),
    (RelType::DiagramColors, "diagramColors", None),
    (RelType::CustomXml, "customXml", None),
    (RelType::CustomXmlProps, "customXmlProps", None),
    (RelType::OleObject, "oleObject", None),
    (RelType::GlossaryDocument, "glossaryDocument", None),
    (RelType::ExtendedProperties, "extended-properties", Some("extendedProperties")),
    (RelType::CustomProperties, "custom-properties", Some("customProperties")),
];

/// 单族 URI（包级与微软扩展）。
const SINGLE: &[(RelType, &str, &str)] = &[
    (RelType::CoreProperties, PKG_FAMILY, "metadata/core-properties"),
    (RelType::Thumbnail, PKG_FAMILY, "metadata/thumbnail"),
    (RelType::CommentsExtended, MS_2011, "commentsExtended"),
    (RelType::People, MS_2011, "people"),
    (RelType::CommentsIds, MS_2016_09, "commentsIds"),
    (RelType::CommentsExtensible, MS_2018_08, "commentsExtensible"),
    (RelType::DiagramDrawing, MS_2007, "diagramDrawing"),
    (RelType::ChartEx, MS_2014, "chartEx"),
];

impl RelType {
    /// URI → (类型, 族别)。族别只对双族 URI 有意义。
    pub fn parse(uri: &str) -> (RelType, Option<PartFlavor>) {
        if let Some(tail) = uri.strip_prefix(T_FAMILY) {
            if let Some((t, ..)) = DUAL.iter().find(|(_, tt, _)| *tt == tail) {
                return (*t, Some(PartFlavor::Transitional));
            }
            return (RelType::Other, Some(PartFlavor::Transitional));
        }
        if let Some(tail) = uri.strip_prefix(S_FAMILY) {
            if let Some((t, ..)) = DUAL.iter().find(|(_, tt, st)| st.unwrap_or(tt) == tail) {
                return (*t, Some(PartFlavor::Strict));
            }
            return (RelType::Other, Some(PartFlavor::Strict));
        }
        for (t, prefix, tail) in SINGLE {
            if uri.len() == prefix.len() + tail.len()
                && uri.starts_with(prefix)
                && uri.ends_with(tail)
            {
                return (*t, None);
            }
        }
        (RelType::Other, None)
    }

    /// 生成新关系时的 URI，按目标 part 的 flavor 选族。`Other` 无法生成。
    pub fn uri(self, flavor: PartFlavor) -> Option<String> {
        if let Some((_, tt, st)) = DUAL.iter().find(|(t, ..)| *t == self) {
            return Some(match flavor {
                PartFlavor::Transitional => format!("{T_FAMILY}{tt}"),
                PartFlavor::Strict => format!("{S_FAMILY}{}", st.unwrap_or(tt)),
            });
        }
        SINGLE.iter().find(|(t, ..)| *t == self).map(|(_, p, tail)| format!("{p}{tail}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelTarget {
    Internal(PartUri),
    /// 原文，不做路径归一化。
    External(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub kind: RelType,
    pub target: RelTarget,
    /// `Type` 属性原文。
    pub raw_type: String,
    /// 双族 URI 的族别；包级与厂商类型为 `None`。
    pub family: Option<PartFlavor>,
    /// `.rels` DOM 中的 `Relationship` 节点。
    pub node: NodeId,
}

/// 一个 part 的关系表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rels {
    list: Vec<Relationship>,
    by_id: HashMap<String, usize>,
}

impl Rels {
    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&Relationship> {
        self.by_id.get(id).map(|&i| &self.list[i])
    }

    pub fn of_kind(&self, kind: RelType) -> impl Iterator<Item = &Relationship> {
        self.list.iter().filter(move |r| r.kind == kind)
    }

    /// 内部目标（`Internal`）的 part 路径。
    pub fn target_uri(&self, id: &str) -> Option<&PartUri> {
        match &self.by_id(id)?.target {
            RelTarget::Internal(u) => Some(u),
            RelTarget::External(_) => None,
        }
    }
}

/// 解析 `.rels`。`source` 是关系文件所属的 part（包根为 [`PartUri::ROOT`]）；
/// `exists` 用 zip 条目表回答"此路径是否存在"，供 `../` 写法的回退与大小写不敏感匹配。
pub fn parse_rels(
    dom: &Dom,
    rels_part: PartId,
    source: &PartUri,
    exists: &dyn Fn(&PartUri) -> Option<PartUri>,
    diags: &mut Vec<Diagnostic>,
) -> Rels {
    let id_q = QName::new(NsId::None, LocalName::UId);
    let type_q = QName::new(NsId::None, LocalName::UType);
    let target_q = QName::new(NsId::None, LocalName::Target);
    let mode_q = QName::new(NsId::None, LocalName::TargetMode);
    let mut rels = Rels::default();
    for &child in dom.children(dom.root()) {
        if dom.name(child).map(|q| q.local) != Some(LocalName::Relationship) {
            continue;
        }
        let range = dom.node(child).lex.as_ref().map(|l| l.range.clone());
        let Some(id) = dom.attr_value(child, id_q) else { continue };
        let raw_type = dom.attr_value(child, type_q).map(|s| s.into_owned()).unwrap_or_default();
        let target_raw =
            dom.attr_value(child, target_q).map(|s| s.into_owned()).unwrap_or_default();
        let external_mode =
            dom.attr_value(child, mode_q).is_some_and(|m| m.eq_ignore_ascii_case("External"));
        let (kind, family) = RelType::parse(&raw_type);

        let target = if external_mode {
            RelTarget::External(target_raw)
        } else if looks_external(&target_raw) {
            diags.push(Diagnostic::pre_existing(
                rels_part,
                range.clone(),
                DiagCode::PkgExternalWithoutMode,
                format!("relationship {id} targets {target_raw} without TargetMode=\"External\""),
            ));
            RelTarget::External(target_raw)
        } else {
            match resolve_internal(source, &target_raw, exists) {
                Ok((u, case_fallback)) => {
                    if case_fallback {
                        diags.push(Diagnostic::pre_existing(
                            rels_part,
                            range.clone(),
                            DiagCode::PkgCaseInsensitiveMatch,
                            format!("relationship {id} target {target_raw} matched part {u} case-insensitively"),
                        ));
                    }
                    RelTarget::Internal(u)
                }
                Err(e) => {
                    diags.push(Diagnostic::pre_existing(
                        rels_part,
                        range.clone(),
                        DiagCode::PkgPathEscapesRoot,
                        format!("relationship {id} target {target_raw}: {e}; treated as missing"),
                    ));
                    continue;
                }
            }
        };

        let rel = Relationship { id: id.into_owned(), kind, target, raw_type, family, node: child };
        if let Some(&prev) = rels.by_id.get(&rel.id) {
            diags.push(Diagnostic::pre_existing(
                rels_part,
                range,
                DiagCode::PkgDupRelId,
                format!("duplicate relationship id {}; the later one wins", rel.id),
            ));
            rels.list[prev] = rel;
        } else {
            rels.by_id.insert(rel.id.clone(), rels.list.len());
            rels.list.push(rel);
        }
    }
    rels
}

fn looks_external(target: &str) -> bool {
    let t = target.trim_start();
    let lower: String = t.chars().take(8).collect::<String>().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("file://")
}

/// 先按源 part 目录解析；目标不存在且写法以 `../` 开头时再按关系文件目录解析
///（一些生成器相对 `_rels/` 写目标）。返回 (路径, 是否走了大小写不敏感匹配)。
fn resolve_internal(
    source: &PartUri,
    target: &str,
    exists: &dyn Fn(&PartUri) -> Option<PartUri>,
) -> Result<(PartUri, bool), uri::UriError> {
    let primary = uri::resolve(source, target)?;
    if let Some(found) = exists(&primary) {
        let case = found != primary;
        return Ok((found, case));
    }
    if target.starts_with("../")
        && let Ok(alt) = uri::resolve(&source.rels_uri(), target)
        && let Some(found) = exists(&alt)
    {
        let case = found != alt;
        return Ok((found, case));
    }
    Ok((primary, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId2" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/header" Target="/word/header1.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a/../b" TargetMode="External"/>
<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="http://no-mode.example"/>
<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../../escape.png"/>
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
<Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/Image1.png"/>
<Relationship Id="rId7" Type="http://schemas.microsoft.com/office/2011/relationships/commentsExtended" Target="commentsExtended.xml"/>
<Relationship Id="rId8" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/extendedProperties" Target="../docProps/app.xml"/>
</Relationships>"#;

    fn parse() -> (Rels, Vec<Diagnostic>) {
        let dom = Dom::parse(PartId(1), RELS.as_bytes()).unwrap();
        let existing =
            ["word/styles.xml", "word/header1.xml", "word/media/image1.png", "docProps/app.xml"];
        let exists = |u: &PartUri| -> Option<PartUri> {
            existing
                .iter()
                .find(|e| **e == u.as_str())
                .or_else(|| existing.iter().find(|e| e.eq_ignore_ascii_case(u.as_str())))
                .map(|e| PartUri::from_entry_name(e))
        };
        let mut diags = Vec::new();
        let rels = parse_rels(
            &dom,
            PartId(1),
            &PartUri::from_entry_name("word/document.xml"),
            &exists,
            &mut diags,
        );
        (rels, diags)
    }

    #[test]
    fn pkg_05_targets_modes_and_duplicates() {
        let (rels, diags) = parse();
        let codes: Vec<DiagCode> = diags.iter().map(|d| d.code).collect();
        assert_eq!(
            rels.target_uri("rId1"),
            Some(&PartUri::from_entry_name("word/numbering.xml")),
            "later duplicate wins"
        );
        assert_eq!(rels.by_id("rId1").unwrap().kind, RelType::Numbering);
        assert!(codes.contains(&DiagCode::PkgDupRelId));
        assert_eq!(
            rels.by_id("rId3").unwrap().target,
            RelTarget::External("https://example.com/a/../b".into())
        );
        assert_eq!(
            rels.by_id("rId4").unwrap().target,
            RelTarget::External("http://no-mode.example".into())
        );
        assert!(codes.contains(&DiagCode::PkgExternalWithoutMode));
        assert!(rels.by_id("rId5").is_none(), "escaping target is dropped");
        assert!(codes.contains(&DiagCode::PkgPathEscapesRoot));
        assert_eq!(rels.len(), 7, "missing Id skipped, duplicate merged, escape dropped");
    }

    #[test]
    fn pkg_06_rels_relative_fallback_and_case_insensitive_match() {
        let (rels, diags) = parse();
        // `../media/Image1.png` 相对 word/document.xml 是 media/Image1.png（不存在）→ 回退到相对 _rels/ → word/media/Image1.png → 大小写不敏感命中
        assert_eq!(
            rels.target_uri("rId6"),
            Some(&PartUri::from_entry_name("word/media/image1.png"))
        );
        assert!(diags.iter().any(|d| d.code == DiagCode::PkgCaseInsensitiveMatch));
        assert_eq!(rels.target_uri("rId8"), Some(&PartUri::from_entry_name("docProps/app.xml")));
    }

    #[test]
    fn pkg_07_dual_family_rel_types() {
        let (rels, _) = parse();
        let h = rels.by_id("rId2").unwrap();
        assert_eq!(h.kind, RelType::Header);
        assert_eq!(h.family, Some(PartFlavor::Strict));
        assert_eq!(rels.by_id("rId1").unwrap().family, Some(PartFlavor::Transitional));
        assert_eq!(rels.by_id("rId7").unwrap().kind, RelType::CommentsExtended);
        assert_eq!(rels.by_id("rId7").unwrap().family, None);
        assert_eq!(rels.by_id("rId8").unwrap().kind, RelType::ExtendedProperties);
        assert_eq!(RelType::parse("http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties").0, RelType::ExtendedProperties);
        assert_eq!(RelType::parse("http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties").0, RelType::CoreProperties);
        assert_eq!(RelType::parse("urn:whatever"), (RelType::Other, None));
        assert_eq!(
            RelType::Header.uri(PartFlavor::Strict).unwrap(),
            "http://purl.oclc.org/ooxml/officeDocument/relationships/header"
        );
        assert_eq!(
            RelType::ExtendedProperties.uri(PartFlavor::Transitional).unwrap(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties"
        );
        assert_eq!(RelType::Other.uri(PartFlavor::Transitional), None);
        assert_eq!(rels.of_kind(RelType::Hyperlink).count(), 2);
    }
}
