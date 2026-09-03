//! zip 读取（`PKG-01`、`PKG-02`、`PKG-11`）。
//!
//! 先在内存副本上把 central directory 中每条记录的 Info-ZIP Unicode Path 字段（`0x7075`）id 改为
//! `0xFFFF`，再交给 `zip` crate：该 crate 会按此字段改写条目名（`read.rs` `UsedExtraField::UnicodePath`），
//! 而 Word 按本地头文件名解析 part 并忽略它。原始字节原样保留，无编辑保存返回的是原始字节。
//! 限额按 central directory 声明的解压大小检查，在解压任何 part 之前完成。

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use zip::CompressionMethod;
use zip::ZipArchive;

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::package::limits;

/// 一个 zip 条目的元数据（来自 central directory）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntryRef {
    /// 条目序号 = zip 中的顺序，写回时保持（`SAVE-06`）。
    pub index: u32,
    pub name: String,
    pub is_dir: bool,
    /// 声明的解压大小。
    pub size: u64,
    pub compressed_size: u64,
    pub method: Compression,
    pub crc32: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Store,
    Deflate,
    /// 其他压缩法：本 crate 的特征集不解压，写回时原样 raw copy。
    Other,
}

impl From<CompressionMethod> for Compression {
    fn from(m: CompressionMethod) -> Self {
        match m {
            CompressionMethod::Stored => Compression::Store,
            CompressionMethod::Deflated => Compression::Deflate,
            _ => Compression::Other,
        }
    }
}

/// 打开的 docx 容器：原始字节 + 中和后的 zip 视图 + 条目表。
pub struct ZipPackage {
    original: Arc<[u8]>,
    archive: ZipArchive<Cursor<Vec<u8>>>,
    entries: Vec<ZipEntryRef>,
    by_name: HashMap<String, u32>,
}

impl std::fmt::Debug for ZipPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipPackage")
            .field("bytes", &self.original.len())
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl ZipPackage {
    /// 读 central directory、中和 `0x7075`、检查限额（`PKG-02`）。不解压任何 part。
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let original: Arc<[u8]> = Arc::from(bytes);
        let neutralized = neutralize_unicode_path(bytes);
        let mut archive =
            ZipArchive::new(Cursor::new(neutralized)).map_err(|e| Error::Zip(e.to_string()))?;

        let mut entries = Vec::with_capacity(archive.len());
        let mut by_name = HashMap::with_capacity(archive.len());
        let mut parts = 0usize;
        let mut total: u64 = 0;
        for i in 0..archive.len() {
            let f = archive.by_index_raw(i).map_err(|e| Error::Zip(format!("entry {i}: {e}")))?;
            let entry = ZipEntryRef {
                index: u32::try_from(i).expect("entry count fits u32"),
                name: f.name().to_string(),
                is_dir: f.is_dir(),
                size: f.size(),
                compressed_size: f.compressed_size(),
                method: f.compression().into(),
                crc32: f.crc32(),
            };
            if !entry.is_dir {
                parts += 1;
                if parts > limits::MAX_PARTS {
                    return Err(limit(
                        DiagCode::PkgTooManyParts,
                        format!("number of parts exceeds {}", limits::MAX_PARTS),
                    ));
                }
                if entry.size > limits::MAX_PART_BYTES {
                    return Err(limit(
                        DiagCode::PkgPartTooLarge,
                        format!(
                            "part {} declares uncompressed size {} > {}",
                            entry.name,
                            entry.size,
                            limits::MAX_PART_BYTES
                        ),
                    ));
                }
                total = total.saturating_add(entry.size);
                if total > limits::MAX_TOTAL_BYTES {
                    return Err(limit(
                        DiagCode::PkgTotalTooLarge,
                        format!("total uncompressed size exceeds {}", limits::MAX_TOTAL_BYTES),
                    ));
                }
            }
            by_name.entry(entry.name.clone()).or_insert(entry.index);
            entries.push(entry);
        }
        Ok(Self { original, archive, entries, by_name })
    }

    /// 输入的原始字节（未中和）。不变式 1 的返回值。
    pub fn original_bytes(&self) -> &Arc<[u8]> {
        &self.original
    }

    pub fn entries(&self) -> &[ZipEntryRef] {
        &self.entries
    }

    pub fn entry(&self, index: u32) -> &ZipEntryRef {
        &self.entries[index as usize]
    }

    /// 精确匹配条目名（重复名取第一个）。
    pub fn find(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// 大小写不敏感匹配（`PKG-06` 的回退路径，调用方记诊断）。
    pub fn find_ignore_case(&self, name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| !e.is_dir && e.name.eq_ignore_ascii_case(name))
            .map(|e| e.index)
    }

    /// 解压一个条目。
    pub fn read(&mut self, index: u32) -> Result<Vec<u8>> {
        let mut f = self
            .archive
            .by_index(index as usize)
            .map_err(|e| Error::Zip(format!("entry {index}: {e}")))?;
        let mut out = Vec::with_capacity(usize::try_from(f.size()).unwrap_or(0));
        f.read_to_end(&mut out)
            .map_err(|e| Error::Zip(format!("entry {index} ({}): {e}", f.name())))?;
        Ok(out)
    }
}

impl ZipPackage {
    /// `SAVE-06`：把条目的压缩数据原样拷进 `writer`（不解压不重压，名字 / 方法 / CRC 不变）。
    pub fn raw_copy_into<W: std::io::Write + std::io::Seek>(
        &mut self,
        index: u32,
        writer: &mut zip::ZipWriter<W>,
    ) -> Result<()> {
        let f = self
            .archive
            .by_index_raw(index as usize)
            .map_err(|e| Error::Zip(format!("entry {index}: {e}")))?;
        writer.raw_copy_file(f).map_err(|e| Error::Zip(format!("raw copy of entry {index}: {e}")))
    }
}

fn limit(code: DiagCode, message: String) -> Error {
    Error::Limit { code, message }
}

const EOCD_SIG: u32 = 0x0605_4b50;
const CDIR_SIG: u32 = 0x0201_4b50;
const UNICODE_PATH: u16 = 0x7075;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// `PKG-01`：返回副本，其中 central directory 每条记录 extra 区里 id 为 `0x7075` 的字段改为 `0xFFFF`。
/// zip64（count 或 offset 为哨兵值）不处理；任何结构不一致处停止改写，剩余部分原样交给 zip 库报错。
pub fn neutralize_unicode_path(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let Some(eocd) = find_eocd(bytes) else { return out };
    let (Some(count), Some(cd_offset)) = (u16_at(bytes, eocd + 10), u32_at(bytes, eocd + 16))
    else {
        return out;
    };
    if count == 0xFFFF || cd_offset == 0xFFFF_FFFF {
        return out; // zip64
    }
    let mut p = cd_offset as usize;
    for _ in 0..count {
        if u32_at(bytes, p) != Some(CDIR_SIG) {
            break;
        }
        let (Some(name_len), Some(extra_len), Some(comment_len)) =
            (u16_at(bytes, p + 28), u16_at(bytes, p + 30), u16_at(bytes, p + 32))
        else {
            break;
        };
        let extra_start = p + 46 + name_len as usize;
        let extra_end = extra_start + extra_len as usize;
        if extra_end > bytes.len() {
            break;
        }
        let mut q = extra_start;
        while q + 4 <= extra_end {
            let (Some(id), Some(len)) = (u16_at(bytes, q), u16_at(bytes, q + 2)) else { break };
            if id == UNICODE_PATH {
                out[q..q + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
            }
            q += 4 + len as usize;
        }
        p = extra_end + comment_len as usize;
    }
    out
}

/// 从尾部向前找 EOCD（注释最长 65535 字节）。
fn find_eocd(b: &[u8]) -> Option<usize> {
    if b.len() < 22 {
        return None;
    }
    let min = b.len().saturating_sub(22 + 65_535);
    (min..=b.len() - 22).rev().find(|&i| u32_at(b, i) == Some(EOCD_SIG))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutralize_is_identity_without_the_field_and_safe_on_garbage() {
        assert_eq!(neutralize_unicode_path(b""), b"");
        assert_eq!(neutralize_unicode_path(b"not a zip at all"), b"not a zip at all");
        // 伪造 EOCD 指向越界的 central directory：不能 panic，原样返回
        let mut fake = vec![0u8; 22];
        fake[..4].copy_from_slice(&EOCD_SIG.to_le_bytes());
        fake[10..12].copy_from_slice(&1u16.to_le_bytes());
        fake[16..20].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes());
        assert_eq!(neutralize_unicode_path(&fake), fake);
    }

    #[test]
    fn open_rejects_non_zip() {
        assert!(matches!(ZipPackage::open(b"hello").unwrap_err(), Error::Zip(_)));
    }
}
