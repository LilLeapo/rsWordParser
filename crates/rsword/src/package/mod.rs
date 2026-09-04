//! L0 包层（`spec/01-package.md`，`docs/03` §3）。
//!
//! 把 `.docx` 字节变成 part 图与关系图，判定 flavor，提供每个 part 的 [`NamespaceContext`]。
//! 本层不理解 WordprocessingML 语义。XML part 的 DOM 惰性构建（[`Package::dom`]），主 part 在打开时解析。

pub mod content_types;
pub mod ns_context;
pub mod rels;
pub mod uri;
pub mod zip;

use std::collections::HashMap;
use std::sync::Arc;

pub use content_types::ContentTypes;
pub use ns_context::{NamespaceContext, UNDERSTOOD};
pub use rels::{RelTarget, RelType, Relationship, Rels, parse_rels};
pub use uri::{PartUri, UriError, resolve};
pub use zip::{Compression, ZipEntryRef, ZipPackage, neutralize_unicode_path};

use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, NotOoxml, Result};
use crate::xml::{Dom, NsId, XmlError, sniff_root};

/// part 在会话内的稳定编号（zip 中非目录条目的顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartId(pub u32);

impl PartId {
    fn idx(self) -> usize {
        self.0 as usize
    }
}

/// `PKG-02` 限额：在解压任何 part 之前按 central directory 声明大小检查。
pub mod limits {
    /// part 数上限（不含目录项）。
    pub const MAX_PARTS: usize = 10_000;
    /// 单 part 解压大小上限：512 MiB。
    pub const MAX_PART_BYTES: u64 = 512 * 1024 * 1024;
    /// 总解压大小上限：1.5 GiB。
    pub const MAX_TOTAL_BYTES: u64 = 3 * 512 * 1024 * 1024;
}

/// OOXML 命名空间族（`spec/00` §0.3）。
///
/// 单个 part 的 flavor 由其根元素绑定的 URI 判定（`PKG-08`）；生成新节点时**按目标 part**
/// 的 flavor 工作，不按包。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartFlavor {
    /// `schemas.openxmlformats.org/.../2006/...`
    Transitional,
    /// `purl.oclc.org/ooxml/...`
    Strict,
}

/// 包级 flavor（`docs/03` §3.2、`PKG-08`）。`Mixed` 是防御性分类，产品只承诺前两者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageFlavor {
    Transitional,
    Strict,
    Mixed,
}

#[derive(Debug)]
enum PartDom {
    /// 二进制 part：无 DOM。
    NotXml,
    /// XML part 的原字节，尚未建树。
    Raw(Vec<u8>),
    Parsed(Box<Dom>),
    /// 解析失败：保存原字节（`PKG-11`）。
    Opaque(XmlError),
}

#[derive(Debug)]
pub struct Part {
    pub id: PartId,
    pub uri: PartUri,
    pub zip_index: u32,
    pub content_type: Option<String>,
    pub is_xml: bool,
    /// 根元素命名空间的族别；包级 / 厂商命名空间与二进制 part 为 `None`。
    pub flavor: Option<PartFlavor>,
    /// 本 part 的关系（来自 `<dir>/_rels/<name>.rels`）。
    pub rels: Rels,
    /// 承载 `rels` 的 `.rels` part。
    pub rels_part: Option<PartId>,
    dom: PartDom,
}

impl Part {
    pub fn is_opaque(&self) -> bool {
        matches!(self.dom, PartDom::Opaque(_))
    }

    pub fn is_parsed(&self) -> bool {
        matches!(self.dom, PartDom::Parsed(_))
    }

    /// 解析失败的原因（`Opaque` part）。
    pub fn opaque_error(&self) -> Option<&XmlError> {
        match &self.dom {
            PartDom::Opaque(e) => Some(e),
            _ => None,
        }
    }

    /// 已解析的 DOM（不触发解析）。
    pub fn dom(&self) -> Option<&Dom> {
        match &self.dom {
            PartDom::Parsed(d) => Some(d),
            _ => None,
        }
    }

    pub fn dom_mut(&mut self) -> Option<&mut Dom> {
        match &mut self.dom {
            PartDom::Parsed(d) => Some(d),
            _ => None,
        }
    }
}

/// 打开的 docx 包：part 表、关系图、flavor、内容类型。
#[derive(Debug)]
pub struct Package {
    zip: ZipPackage,
    parts: Vec<Part>,
    by_uri: HashMap<PartUri, PartId>,
    main: PartId,
    flavor: PackageFlavor,
    content_types: ContentTypes,
    content_types_part: Option<PartId>,
    root_rels: Rels,
    root_rels_part: Option<PartId>,
    diagnostics: Vec<Diagnostic>,
}

impl Package {
    /// `PKG-01/02`（zip）→ `PKG-04`（内容类型）→ `PKG-05..07`（关系）→ `PKG-03`（主 part）→ `PKG-08`（flavor）。
    /// 主 part 立即解析（畸形 → `Error::Malformed`，`XML-08`）；其余 XML part 惰性。
    pub fn open(bytes: &[u8]) -> Result<Package> {
        let mut zip = ZipPackage::open(bytes)?;
        let mut diagnostics = Vec::new();

        // part 表
        let mut parts: Vec<Part> = Vec::new();
        let mut by_uri: HashMap<PartUri, PartId> = HashMap::new();
        for e in zip.entries() {
            if e.is_dir {
                continue;
            }
            let id = PartId(u32::try_from(parts.len()).expect("part count fits u32"));
            let uri = PartUri::from_entry_name(&e.name);
            by_uri.entry(uri.clone()).or_insert(id);
            parts.push(Part {
                id,
                uri,
                zip_index: e.index,
                content_type: None,
                is_xml: false,
                flavor: None,
                rels: Rels::default(),
                rels_part: None,
                dom: PartDom::NotXml,
            });
        }

        // [Content_Types].xml
        let ct_uri = PartUri::from_entry_name("[Content_Types].xml");
        let content_types_part = by_uri.get(&ct_uri).copied();
        let mut content_types = ContentTypes::default();
        match content_types_part {
            Some(id) => {
                let bytes = zip.read(parts[id.idx()].zip_index)?;
                match Dom::parse(id, &bytes) {
                    Ok(dom) => {
                        content_types = ContentTypes::from_dom(&dom);
                        diagnostics.extend(dom.diagnostics().iter().cloned());
                        parts[id.idx()].dom = PartDom::Parsed(Box::new(dom));
                    }
                    Err(e) => {
                        diagnostics.push(Diagnostic::pre_existing(
                            id,
                            Some(e.offset..e.offset),
                            DiagCode::PkgOpaquePart,
                            format!("[Content_Types].xml is not parseable: {e}"),
                        ));
                        parts[id.idx()].dom = PartDom::Opaque(e);
                    }
                }
            }
            None => diagnostics.push(Diagnostic::pre_existing(
                PartId(0),
                None,
                DiagCode::PkgNoContentTypes,
                "[Content_Types].xml is missing; part types inferred from extensions",
            )),
        }
        for p in &mut parts {
            p.content_type = content_types.content_type(&p.uri).map(str::to_string);
            p.is_xml = content_types.is_xml_part(&p.uri);
        }

        // 读入全部 XML part 的字节（解析惰性）
        for p in &mut parts {
            if p.is_xml && matches!(p.dom, PartDom::NotXml) {
                p.dom = PartDom::Raw(zip.read(p.zip_index)?);
            }
        }

        // 关系：先解析所有 .rels 的 DOM，再挂到源 part
        let exists = |u: &PartUri| -> Option<PartUri> {
            if by_uri.contains_key(u) {
                return Some(u.clone());
            }
            by_uri.keys().find(|k| k.as_str().eq_ignore_ascii_case(u.as_str())).cloned()
        };
        let mut attach: Vec<(PartId, Option<PartId>, Rels)> = Vec::new(); // (rels part, source part or None=root, rels)
        let mut root_rels = Rels::default();
        let mut root_rels_part = None;
        for part in &mut parts {
            let Some(source) = part.uri.rels_source() else { continue };
            let id = part.id;
            let PartDom::Raw(bytes) = std::mem::replace(&mut part.dom, PartDom::NotXml) else {
                continue;
            };
            match Dom::parse(id, &bytes) {
                Ok(dom) => {
                    let rels = parse_rels(&dom, id, &source, &exists, &mut diagnostics);
                    diagnostics.extend(dom.diagnostics().iter().cloned());
                    part.dom = PartDom::Parsed(Box::new(dom));
                    if source.is_root() {
                        root_rels = rels;
                        root_rels_part = Some(id);
                    } else {
                        attach.push((id, by_uri.get(&source).copied(), rels));
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic::pre_existing(
                        id,
                        Some(e.offset..e.offset),
                        DiagCode::PkgOpaquePart,
                        format!("relationship part {} is not parseable: {e}", part.uri),
                    ));
                    part.dom = PartDom::Opaque(e);
                }
            }
        }
        for (rels_part, source, rels) in attach {
            if let Some(src) = source {
                parts[src.idx()].rels = rels;
                parts[src.idx()].rels_part = Some(rels_part);
            }
        }

        // 主 part（PKG-03）
        let main = match locate_main(&by_uri, &root_rels) {
            Some(m) => m,
            None => {
                let mime = by_uri
                    .get(&PartUri::from_entry_name("mimetype"))
                    .and_then(|id| zip.read(parts[id.idx()].zip_index).ok())
                    .map(|b| String::from_utf8_lossy(&b).trim().to_string());
                return Err(match mime {
                    Some(m) if m.starts_with("application/vnd.oasis.opendocument") => {
                        NotOoxml::OpenDocument(m).into()
                    }
                    _ => NotOoxml::MissingMainPart.into(),
                });
            }
        };

        // flavor（PKG-08）：每个 XML part 的根命名空间族 + 主 part 关系类型的族
        let mut seen: Vec<PartFlavor> = Vec::new();
        for p in &mut parts {
            let bytes = match &p.dom {
                PartDom::Raw(b) => b.as_slice(),
                PartDom::Parsed(d) => d.src_bytes(),
                _ => continue,
            };
            if let Ok(info) = sniff_root(bytes)
                && let Some(uri) = &info.namespace_uri
                && let Some((ns, fl)) = NsId::from_uri(uri)
                && ns.has_strict_uri()
            {
                p.flavor = Some(fl);
                if !seen.contains(&fl) {
                    seen.push(fl);
                }
            }
        }
        for r in parts[main.idx()].rels.iter() {
            if let Some(fl) = r.family
                && !seen.contains(&fl)
            {
                seen.push(fl);
            }
        }
        let flavor = match seen.as_slice() {
            [] | [PartFlavor::Transitional] => PackageFlavor::Transitional,
            [PartFlavor::Strict] => PackageFlavor::Strict,
            _ => {
                diagnostics.push(Diagnostic::pre_existing(
                    main,
                    None,
                    DiagCode::PkgMixedFlavor,
                    "package mixes Strict and Transitional parts; generation follows each part's own flavor",
                ));
                PackageFlavor::Mixed
            }
        };

        let mut pkg = Package {
            zip,
            parts,
            by_uri,
            main,
            flavor,
            content_types,
            content_types_part,
            root_rels,
            root_rels_part,
            diagnostics,
        };
        // 主 part 立即解析；畸形即整体失败（XML-08）
        pkg.dom(main)?;
        Ok(pkg)
    }

    pub fn original_bytes(&self) -> &Arc<[u8]> {
        self.zip.original_bytes()
    }

    pub fn zip(&self) -> &ZipPackage {
        &self.zip
    }

    pub(crate) fn zip_mut(&mut self) -> &mut ZipPackage {
        &mut self.zip
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn part(&self, id: PartId) -> &Part {
        &self.parts[id.idx()]
    }

    /// 可变 part（`EDIT-06` 追加关系后同步内存里的 `Rels`）。
    pub(crate) fn part_mut(&mut self, id: PartId) -> &mut Part {
        &mut self.parts[id.idx()]
    }

    pub fn find(&self, uri: &PartUri) -> Option<PartId> {
        self.by_uri.get(uri).copied()
    }

    pub fn find_name(&self, name: &str) -> Option<PartId> {
        self.find(&PartUri::from_entry_name(name))
    }

    pub fn main_part(&self) -> PartId {
        self.main
    }

    pub fn flavor(&self) -> PackageFlavor {
        self.flavor
    }

    /// 生成新节点时某 part 应使用的 flavor：part 自身的族别，否则包级（`Mixed` 回退 Transitional）。
    pub fn flavor_of(&self, id: PartId) -> PartFlavor {
        self.part(id).flavor.unwrap_or(match self.flavor {
            PackageFlavor::Strict => PartFlavor::Strict,
            _ => PartFlavor::Transitional,
        })
    }

    pub fn content_types(&self) -> &ContentTypes {
        &self.content_types
    }

    pub fn content_types_part(&self) -> Option<PartId> {
        self.content_types_part
    }

    /// `_rels/.rels`。
    pub fn root_rels(&self) -> &Rels {
        &self.root_rels
    }

    pub fn root_rels_part(&self) -> Option<PartId> {
        self.root_rels_part
    }

    /// 保存期新增的诊断（`SAVE-02`）。
    pub(crate) fn push_diagnostics(&mut self, more: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(more);
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// `part` 的关系 `rid` 指向的内部 part（不存在或外部链接 → `None`）。
    pub fn target_part(&self, part: PartId, rid: &str) -> Option<PartId> {
        let uri = self.part(part).rels.target_uri(rid)?;
        self.find(uri)
    }

    /// `part` 的某类关系指向的、存在的内部 part。
    pub fn related(&self, part: PartId, kind: RelType) -> impl Iterator<Item = PartId> + '_ {
        self.part(part).rels.of_kind(kind).filter_map(move |r| match &r.target {
            RelTarget::Internal(u) => self.find(u),
            RelTarget::External(_) => None,
        })
    }

    /// 惰性解析 XML part。`Ok(None)`：二进制 part，或解析失败降级为 `Opaque`（记 `PKG_OPAQUE_PART`，仅首次）。
    /// 主 part 解析失败 → `Err(Malformed)`。
    pub fn dom(&mut self, id: PartId) -> Result<Option<&Dom>> {
        self.ensure_parsed(id)?;
        Ok(self.parts[id.idx()].dom())
    }

    pub fn dom_mut(&mut self, id: PartId) -> Result<Option<&mut Dom>> {
        self.ensure_parsed(id)?;
        Ok(self.parts[id.idx()].dom_mut())
    }

    fn ensure_parsed(&mut self, id: PartId) -> Result<()> {
        let part = &mut self.parts[id.idx()];
        if !matches!(part.dom, PartDom::Raw(_)) {
            return Ok(());
        }
        let PartDom::Raw(bytes) = std::mem::replace(&mut part.dom, PartDom::NotXml) else {
            unreachable!()
        };
        match Dom::parse(id, &bytes) {
            Ok(dom) => {
                self.diagnostics.extend(dom.diagnostics().iter().cloned());
                part.dom = PartDom::Parsed(Box::new(dom));
                Ok(())
            }
            Err(e) => {
                let uri = part.uri.to_string();
                part.dom = PartDom::Opaque(e.clone());
                if id == self.main {
                    return Err(Error::Malformed {
                        part: uri,
                        offset: e.offset,
                        message: e.message,
                    });
                }
                self.diagnostics.push(Diagnostic::pre_existing(
                    id,
                    Some(e.offset..e.offset),
                    DiagCode::PkgOpaquePart,
                    format!("part {uri} is not parseable and is kept as opaque bytes: {e}"),
                ));
                Ok(())
            }
        }
    }

    /// part 的原字节（解压后）。XML part 已解析时给出其源字节（转码过的 part 为转码后字节）。
    pub fn read_bytes(&mut self, id: PartId) -> Result<Vec<u8>> {
        match &self.parts[id.idx()].dom {
            PartDom::Raw(b) => Ok(b.clone()),
            PartDom::Parsed(d) => Ok(d.src_bytes().to_vec()),
            PartDom::NotXml | PartDom::Opaque(_) => self.zip.read(self.parts[id.idx()].zip_index),
        }
    }

    /// `PKG-09`：part 的命名空间上下文（需要 DOM；二进制或 Opaque part → `None`）。
    pub fn namespace_context(&mut self, id: PartId) -> Result<Option<NamespaceContext>> {
        let flavor = self.flavor_of(id);
        Ok(self.dom(id)?.map(|d| NamespaceContext::from_dom(d, flavor)))
    }
}

/// `PKG-03`：`word/document.xml`，否则 `_rels/.rels` 中 `officeDocument` 关系的内部目标。
fn locate_main(by_uri: &HashMap<PartUri, PartId>, root_rels: &Rels) -> Option<PartId> {
    if let Some(id) = by_uri.get(&PartUri::from_entry_name("word/document.xml")) {
        return Some(*id);
    }
    root_rels.of_kind(RelType::OfficeDocument).find_map(|r| match &r.target {
        RelTarget::Internal(u) => by_uri.get(u).copied(),
        RelTarget::External(_) => None,
    })
}
