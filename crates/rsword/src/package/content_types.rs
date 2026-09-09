//! `[Content_Types].xml`（`PKG-04`）。

use std::collections::HashMap;

use crate::package::uri::PartUri;
use crate::xml::{Dom, LocalName, NsId, QName};

/// 已知图片扩展名 → MIME（`PKG-04` 判定顺序的第一步）。
const IMAGE_EXT: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("bmp", "image/bmp"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("emf", "image/x-emf"),
    ("wmf", "image/x-wmf"),
    ("emz", "image/x-emz"),
    ("wmz", "image/x-wmz"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentTypes {
    /// 小写扩展名 → 内容类型。
    defaults: HashMap<String, String>,
    /// `/word/document.xml` → 内容类型。
    overrides: HashMap<String, String>,
}

impl ContentTypes {
    /// 从已解析的 `[Content_Types].xml` DOM 读取。未知子元素忽略。
    pub fn from_dom(dom: &Dom) -> Self {
        let mut ct = Self::default();
        let ext_q = QName::new(NsId::None, LocalName::UExtension);
        let ctype_q = QName::new(NsId::None, LocalName::ContentType);
        let part_q = QName::new(NsId::None, LocalName::PartName);
        for &child in dom.children(dom.root()) {
            let Some(name) = dom.name(child) else { continue };
            match name.local {
                LocalName::UDefault => {
                    if let (Some(ext), Some(t)) =
                        (dom.attr_value(child, ext_q), dom.attr_value(child, ctype_q))
                    {
                        ct.defaults.insert(ext.to_ascii_lowercase(), t.into_owned());
                    }
                }
                LocalName::Override => {
                    if let (Some(p), Some(t)) =
                        (dom.attr_value(child, part_q), dom.attr_value(child, ctype_q))
                    {
                        let key = if p.starts_with('/') { p.into_owned() } else { format!("/{p}") };
                        ct.overrides.insert(key, t.into_owned());
                    }
                }
                _ => {}
            }
        }
        ct
    }

    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty() && self.overrides.is_empty()
    }

    pub fn default_for_extension(&self, ext: &str) -> Option<&str> {
        self.defaults.get(&ext.to_ascii_lowercase()).map(String::as_str)
    }

    /// 写侧新增了一条 `Default`（`SAVE-05`）：缓存同步，免得同一会话再补第二条重复的（Word 对重复的
    /// `Default Extension` 弹恢复提示——真实 Word 核对发现，`docs/07` 任务 B）。
    pub(crate) fn add_default(&mut self, ext: &str, content_type: &str) {
        self.defaults.insert(ext.to_ascii_lowercase(), content_type.to_string());
    }

    /// 写侧新增了一条 `Override`：缓存同步。
    pub(crate) fn add_override(&mut self, uri: &PartUri, content_type: &str) {
        self.overrides.insert(uri.override_key(), content_type.to_string());
    }

    /// 写侧删掉了一条 `Override`（资源回收）：缓存同步。
    pub(crate) fn remove_override(&mut self, uri: &PartUri) {
        self.overrides.remove(&uri.override_key());
    }

    pub fn override_for(&self, uri: &PartUri) -> Option<&str> {
        self.overrides.get(&uri.override_key()).map(String::as_str)
    }

    /// part 的内容类型：Override 优先，其次按扩展名的 Default。
    pub fn content_type(&self, uri: &PartUri) -> Option<&str> {
        self.override_for(uri)
            .or_else(|| uri.extension().and_then(|e| self.default_for_extension(e)))
    }

    /// 图片 MIME：扩展名表 → Override → Default；结果必须以 `image/` 开头。
    pub fn image_mime(&self, uri: &PartUri) -> Option<String> {
        let ext = uri.extension().map(str::to_ascii_lowercase);
        let from_ext = ext
            .as_deref()
            .and_then(|e| IMAGE_EXT.iter().find(|(x, _)| *x == e).map(|(_, m)| (*m).to_string()));
        let candidate =
            from_ext.or_else(|| self.override_for(uri).map(str::to_string)).or_else(|| {
                ext.as_deref().and_then(|e| self.default_for_extension(e)).map(str::to_string)
            });
        candidate.filter(|m| m.starts_with("image/"))
    }

    /// 是否按内容类型或扩展名视为 XML part（有 DOM）。
    pub fn is_xml_part(&self, uri: &PartUri) -> bool {
        if let Some(t) = self.content_type(uri) {
            return t.ends_with("+xml") || t == "application/xml" || t == "text/xml";
        }
        matches!(uri.extension().map(|e| e.to_ascii_lowercase()).as_deref(), Some("xml" | "rels"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PartId;

    const CT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="XML" ContentType="application/xml"/>
<Default Extension="bin" ContentType="application/octet-stream"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/media/image1.bin" ContentType="image/png"/>
<Override PartName="/word/media/blob.bin" ContentType="application/vnd.ms-office.blob"/>
</Types>"#;

    fn ct() -> ContentTypes {
        ContentTypes::from_dom(&Dom::parse(PartId(0), CT.as_bytes()).unwrap())
    }

    #[test]
    fn pkg_04_override_beats_default_and_extension_is_case_insensitive() {
        let ct = ct();
        let doc = PartUri::from_entry_name("word/document.xml");
        assert!(ct.content_type(&doc).unwrap().ends_with("document.main+xml"));
        assert_eq!(
            ct.content_type(&PartUri::from_entry_name("word/styles.xml")),
            Some("application/xml")
        );
        assert_eq!(
            ct.content_type(&PartUri::from_entry_name("_rels/.rels")),
            Some("application/vnd.openxmlformats-package.relationships+xml")
        );
        assert!(ct.is_xml_part(&doc));
        assert!(!ct.is_xml_part(&PartUri::from_entry_name("word/media/blob.bin")));
    }

    #[test]
    fn pkg_04_image_mime_order() {
        let ct = ct();
        assert_eq!(
            ct.image_mime(&PartUri::from_entry_name("word/media/image1.bin")).as_deref(),
            Some("image/png")
        );
        assert_eq!(
            ct.image_mime(&PartUri::from_entry_name("word/media/blob.bin")),
            None,
            "non-image override"
        );
        assert_eq!(
            ct.image_mime(&PartUri::from_entry_name("word/media/other.bin")),
            None,
            "Default is octet-stream"
        );
        assert_eq!(
            ct.image_mime(&PartUri::from_entry_name("word/media/a.JPG")).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            ct.image_mime(&PartUri::from_entry_name("word/media/a.emf")).as_deref(),
            Some("image/x-emf")
        );
    }

    #[test]
    fn pkg_04_missing_file_is_empty() {
        let ct = ContentTypes::default();
        assert!(ct.is_empty());
        assert!(
            ct.is_xml_part(&PartUri::from_entry_name("word/document.xml")),
            "extension fallback"
        );
        assert_eq!(ct.content_type(&PartUri::from_entry_name("word/document.xml")), None);
    }
}
