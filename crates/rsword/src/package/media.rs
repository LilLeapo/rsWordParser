//! 媒体解析（`docs/03` §3.5、`PKG-05`；`spec/15` 任务 4.1）。
//!
//! [`MediaStore`] 把「某个 part 里的关系 id」解析成包内媒体 part 或外部 URL，缓存 MIME 与字节，
//! 并给 `compat_ts` 提供 dataURL。
//!
//! 三条规则来自 `docs/01` §4.2 与 §8.4，实测语料依赖它们：
//!
//! 1. **按 part 自己的 rels 解析**。页眉页脚、SmartArt 的 drawing part 各有自己的 `_rels/*.rels`，
//!    同一个 `rId10` 在不同 part 指向不同图片。路径里的 `..` 段由 [`crate::package::resolve`]
//!    在解析 `.rels` 时归一化。
//! 2. **MIME 判定顺序**：扩展名表 → `Override` → `Default`，且必须以 `image/` 开头
//!    （[`ContentTypes::image_mime`]）。语料里有 `Default` 把 `png` 声明成
//!    `application/octet-stream` 的文档。
//! 3. **`TargetMode="External"` 与 `http(s)://` 目标直出 URL**，由显示端自己去取。
//!
//! EMF / WMF / EMZ / WMZ 与 TIFF **不在 Rust 侧转换**（`docs/03` §3.5 冻结）：只标
//! [`MediaKind::Metafile`] / [`MediaKind::Tiff`]，由调用方决定是交给外部转换器还是原样给出。
//!
//! [`ContentTypes::image_mime`]: crate::package::ContentTypes::image_mime

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::Result;
use crate::package::{Package, PartId, PartUri, RelTarget};

/// 媒体在会话内的稳定编号（[`MediaStore`] 内的登记顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaId(pub u32);

impl MediaId {
    fn idx(self) -> usize {
        self.0 as usize
    }
}

/// 媒体的显示能力分类。浏览器能直接画的是 [`Raster`](MediaKind::Raster) 与
/// [`Svg`](MediaKind::Svg)，其余需要外部转换（`docs/03` §3.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// PNG / JPEG / GIF / BMP / WEBP：可直接内联。
    Raster,
    Svg,
    /// EMF / WMF / EMZ / WMZ：矢量元文件，Rust 侧不渲染。
    Metafile,
    Tiff,
    /// 其余 `image/*`。
    Other,
}

impl MediaKind {
    pub fn from_mime(mime: &str) -> MediaKind {
        match mime.trim().to_ascii_lowercase().as_str() {
            "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/bmp" | "image/webp" => {
                MediaKind::Raster
            }
            "image/svg+xml" => MediaKind::Svg,
            "image/x-emf" | "image/emf" | "image/x-wmf" | "image/wmf" | "image/x-emz"
            | "image/x-wmz" => MediaKind::Metafile,
            "image/tiff" => MediaKind::Tiff,
            _ => MediaKind::Other,
        }
    }

    /// 是否需要外部转换才能显示。
    pub fn needs_conversion(self) -> bool {
        matches!(self, MediaKind::Metafile | MediaKind::Tiff)
    }
}

/// 一个包内媒体 part。字节惰性读取（见 [`MediaStore::bytes`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub part: PartId,
    pub uri: PartUri,
    pub mime: String,
    pub kind: MediaKind,
}

/// 关系解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaRef {
    /// 包内媒体 part。
    Media(MediaId),
    /// 外部目标（`TargetMode="External"` 或 `http(s)://`）的原始 URL。
    External(String),
}

/// 解析不到媒体的原因。驱动 `compat_ts` 的 `brokenImage`（`COMPAT-03`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaMiss {
    /// 该 part 的 rels 里没有这个 id。
    NoRel,
    /// 关系存在、目标是包内路径，但 zip 里没有这个条目。
    PartMissing,
    /// part 存在但内容类型不是 `image/*`。
    NotAnImage,
}

impl MediaMiss {
    pub const fn as_str(self) -> &'static str {
        match self {
            MediaMiss::NoRel => "no relationship",
            MediaMiss::PartMissing => "relationship target is not in the package",
            MediaMiss::NotAnImage => "content type is not an image",
        }
    }
}

/// 媒体表（`docs/03` §3.5）。按 part 去重：同一张图被多处引用只登记一次。
#[derive(Debug, Default)]
pub struct MediaStore {
    media: Vec<Media>,
    by_part: HashMap<PartId, MediaId>,
    /// 与 `media` 同序的字节缓存。
    bytes: Vec<Option<Arc<[u8]>>>,
}

impl MediaStore {
    pub fn new() -> MediaStore {
        MediaStore::default()
    }

    pub fn len(&self) -> usize {
        self.media.len()
    }

    pub fn is_empty(&self) -> bool {
        self.media.is_empty()
    }

    pub fn get(&self, id: MediaId) -> &Media {
        &self.media[id.idx()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (MediaId, &Media)> {
        self.media.iter().enumerate().map(|(i, m)| (MediaId(i as u32), m))
    }

    /// 解析 `part` 的 rels 里 `rid` 指向的媒体（规则见模块文档）。
    pub fn resolve(
        &mut self,
        pkg: &Package,
        part: PartId,
        rid: &str,
    ) -> std::result::Result<MediaRef, MediaMiss> {
        let rel = pkg.part(part).rels.by_id(rid).ok_or(MediaMiss::NoRel)?;
        let uri = match &rel.target {
            RelTarget::External(t) => return Ok(MediaRef::External(t.clone())),
            RelTarget::Internal(u) => u.clone(),
        };
        // `PKG-05`：没标 External 的 http(s) 目标也当外链（诊断在包层已记 PKG_EXTERNAL_WITHOUT_MODE）。
        if is_http(uri.as_str()) {
            return Ok(MediaRef::External(uri.as_str().to_string()));
        }
        let target = pkg.find(&uri).ok_or(MediaMiss::PartMissing)?;
        Ok(MediaRef::Media(self.intern(pkg, target)))
    }

    /// 直接按 part 登记（媒体 part 已知时用，例如遍历某个关系类型）。
    pub fn intern_part(
        &mut self,
        pkg: &Package,
        part: PartId,
    ) -> std::result::Result<MediaId, MediaMiss> {
        if pkg.content_types().image_mime(&pkg.part(part).uri).is_none() {
            return Err(MediaMiss::NotAnImage);
        }
        Ok(self.intern(pkg, part))
    }

    /// 登记（或复用）一个媒体 part。非图片 part 也会登记，`mime` 取内容类型或空串——
    /// 调用方要先用 [`MediaStore::intern_part`] 过滤；[`MediaStore::resolve`] 不过滤是因为
    /// `v:imagedata` 指向 OLE 预览等非 `image/*` 内容时 TS 仍然显示。
    fn intern(&mut self, pkg: &Package, part: PartId) -> MediaId {
        if let Some(&id) = self.by_part.get(&part) {
            return id;
        }
        let uri = pkg.part(part).uri.clone();
        let mime = pkg
            .content_types()
            .image_mime(&uri)
            .or_else(|| pkg.content_types().content_type(&uri).map(str::to_string))
            .unwrap_or_default();
        let id = MediaId(self.media.len() as u32);
        self.media.push(Media { part, uri, kind: MediaKind::from_mime(&mime), mime });
        self.bytes.push(None);
        self.by_part.insert(part, id);
        id
    }

    /// 媒体字节。首次调用解压并缓存。
    pub fn bytes(&mut self, pkg: &mut Package, id: MediaId) -> Result<Arc<[u8]>> {
        if let Some(b) = &self.bytes[id.idx()] {
            return Ok(Arc::clone(b));
        }
        let part = self.media[id.idx()].part;
        let bytes: Arc<[u8]> = Arc::from(pkg.read_bytes(part)?.into_boxed_slice());
        self.bytes[id.idx()] = Some(Arc::clone(&bytes));
        Ok(bytes)
    }

    /// `data:<mime>;base64,…`。MIME 为空时按 `application/octet-stream`。
    ///
    /// 需要外部转换的媒体（[`MediaKind::needs_conversion`]）**照样**输出原始字节的 dataURL：
    /// 转换不在 Rust 侧（`docs/03` §3.5），调用方看 [`Media::kind`] 决定要不要送去转换。
    pub fn data_url(&mut self, pkg: &mut Package, id: MediaId) -> Result<String> {
        let bytes = self.bytes(pkg, id)?;
        let mime = self.media[id.idx()].mime.clone();
        let mime = if mime.is_empty() { "application/octet-stream" } else { &mime };
        let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4 + mime.len() + 16);
        s.push_str("data:");
        s.push_str(mime);
        s.push_str(";base64,");
        base64_into(&bytes, &mut s);
        Ok(s)
    }
}

fn is_http(target: &str) -> bool {
    let t = target.trim_start();
    let lower = t.get(..8).unwrap_or(t).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64 解码（忽略空白与 `=` 填充；碰到表外字符 → `None`）。TS `partBinary` / `image.base64` 用。
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = T.iter().position(|&t| t == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// 标准 base64（带 `=` 填充）。自己写是为了不给 L0 引第三方依赖。
fn base64_into(bytes: &[u8], out: &mut String) {
    let (chunks, rest) = bytes.as_chunks::<3>();
    for c in chunks {
        let n = (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
        for shift in [18, 12, 6, 0] {
            out.push(B64[(n >> shift) as usize & 0x3f] as char);
        }
    }
    match rest {
        [a] => {
            let n = u32::from(*a) << 16;
            out.push(B64[(n >> 18) as usize & 0x3f] as char);
            out.push(B64[(n >> 12) as usize & 0x3f] as char);
            out.push_str("==");
        }
        [a, b] => {
            let n = (u32::from(*a) << 16) | (u32::from(*b) << 8);
            for shift in [18, 12, 6] {
                out.push(B64[(n >> shift) as usize & 0x3f] as char);
            }
            out.push('=');
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(bytes: &[u8]) -> String {
        let mut s = String::new();
        base64_into(bytes, &mut s);
        s
    }

    #[test]
    fn pkg_05_base64_padding() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foob"), "Zm9vYg==");
        assert_eq!(b64(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        // 高位字节走满 6 位分组
        assert_eq!(b64(&[0xff, 0xff, 0xff]), "////");
        assert_eq!(b64(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn pkg_05_media_kind_from_mime() {
        assert_eq!(MediaKind::from_mime("image/png"), MediaKind::Raster);
        assert_eq!(MediaKind::from_mime("IMAGE/JPEG"), MediaKind::Raster);
        assert_eq!(MediaKind::from_mime("image/svg+xml"), MediaKind::Svg);
        assert_eq!(MediaKind::from_mime("image/x-emf"), MediaKind::Metafile);
        assert_eq!(MediaKind::from_mime("image/x-wmz"), MediaKind::Metafile);
        assert_eq!(MediaKind::from_mime("image/tiff"), MediaKind::Tiff);
        assert_eq!(MediaKind::from_mime("image/heic"), MediaKind::Other);
        assert!(MediaKind::Metafile.needs_conversion());
        assert!(MediaKind::Tiff.needs_conversion());
        assert!(!MediaKind::Raster.needs_conversion());
        assert!(!MediaKind::Svg.needs_conversion());
    }

    #[test]
    fn pkg_05_http_target_is_external() {
        assert!(is_http("http://example.org/a.png"));
        assert!(is_http("HTTPS://example.org/a.png"));
        assert!(!is_http("media/image1.png"));
        assert!(!is_http("httpx://x"));
        assert!(!is_http("ht"));
    }
}
