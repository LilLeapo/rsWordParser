//! 包写回（`SAVE-01` 步骤 1/5/6、`SAVE-06`、`SAVE-08`）。
//!
//! 遍历原 zip 条目按原顺序：未变 part 用 `raw_copy_file` 直接拷压缩数据；变脏 part 用 Deflate 写新数据。
//! 无脏节点且无新增 part → 直接返回原字节（不变式 1）。
//! 新增 part（`SAVE-05`）追加在末尾，原有条目仍按原顺序原压缩数据拷贝。

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::error::{Error, Result};
use crate::package::ns_context::NamespaceContext;
use crate::package::{Package, PartId};
use crate::save::serialize::serialize_with;
use crate::xml::Dirty;

impl Package {
    /// 有脏节点的 XML part。
    pub fn dirty_parts(&self) -> Vec<PartId> {
        self.parts()
            .iter()
            .filter(|p| p.dom().is_some_and(|d| d.node(d.root()).dirty != Dirty::Clean))
            .map(|p| p.id)
            .collect()
    }

    /// 有脏节点，或有本次会话新建的 part（`SAVE-05`）。
    pub fn is_dirty(&self) -> bool {
        !self.dirty_parts().is_empty() || self.new_parts().next().is_some()
    }

    /// 写回整个包。`SAVE-01` 步骤 1 / 2 / 5 / 6：无脏节点直接返回原字节；校验（`SAVE-02`，调试构建下
    /// `EngineInvariantViolation` 为 `Err`）并补扩展命名空间声明（`SAVE-03`）；序列化脏 part；包写回。
    pub fn save(&mut self) -> Result<Vec<u8>> {
        let mut dirty = self.dirty_parts();
        let fresh: Vec<PartId> = self.new_parts().collect();
        if dirty.is_empty() && fresh.is_empty() {
            return Ok(self.original_bytes().to_vec());
        }
        // 新 part 整份都要写（它的 DOM 是从文本解析出来的，根节点是 `Clean`）
        for id in &fresh {
            if !dirty.contains(id) {
                dirty.push(*id);
            }
        }
        let mut diags = Vec::new();
        for &id in &dirty {
            if let Ok(Some(dom)) = self.dom_mut(id) {
                crate::save::validate::ensure_extension_declarations(dom);
                diags.extend(crate::save::validate::validate_part(dom));
            }
        }
        crate::save::validate::enforce(&diags)?;
        self.push_diagnostics(diags);
        // 先序列化所有脏 part（不可变借用 DOM），再做 zip 写入（可变借用 zip）
        let mut replaced: Vec<(PartId, u32, Vec<u8>)> = Vec::with_capacity(dirty.len());
        for id in dirty {
            let part = self.part(id);
            let dom = part.dom().expect("dirty part has a DOM");
            let ctx = NamespaceContext::from_dom(dom, self.flavor_of(id));
            let bytes = serialize_with(dom, Some(&ctx)).map_err(|e| {
                Error::Invariant(crate::diag::Diagnostic::invariant_violation(
                    id,
                    None,
                    crate::diag::DiagCode::SaveInvariant,
                    format!("serialize {}: {e}", part.uri),
                ))
            })?;
            #[cfg(debug_assertions)]
            check_clean_substrings(dom, &bytes, &part.uri.to_string());
            replaced.push((id, part.zip_index, bytes));
        }
        let appended: Vec<(String, Vec<u8>)> = fresh
            .iter()
            .filter_map(|&id| {
                let bytes = replaced.iter().find(|(p, ..)| *p == id).map(|(.., b)| b.clone())?;
                Some((self.part(id).uri.as_str().to_string(), bytes))
            })
            .collect();

        let entries: Vec<(u32, String, bool)> =
            self.zip().entries().iter().map(|e| (e.index, e.name.clone(), e.is_dir)).collect();
        let mut writer =
            zip::ZipWriter::new(Cursor::new(Vec::with_capacity(self.original_bytes().len())));
        let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (index, name, is_dir) in entries {
            if let Some((.., bytes)) = replaced.iter().find(|(_, i, _)| *i == index) {
                writer
                    .start_file(name.as_str(), deflate)
                    .map_err(|e| Error::Zip(format!("start {name}: {e}")))?;
                writer.write_all(bytes).map_err(|e| Error::Zip(format!("write {name}: {e}")))?;
            } else if is_dir {
                writer
                    .add_directory(name.as_str(), SimpleFileOptions::default())
                    .map_err(|e| Error::Zip(format!("dir {name}: {e}")))?;
            } else {
                self.zip_mut().raw_copy_into(index, &mut writer)?;
            }
        }
        // `SAVE-06`：新 part 追加在末尾
        for (name, bytes) in &appended {
            writer
                .start_file(name.as_str(), deflate)
                .map_err(|e| Error::Zip(format!("start {name}: {e}")))?;
            writer.write_all(bytes).map_err(|e| Error::Zip(format!("write {name}: {e}")))?;
        }
        let cursor = writer.finish().map_err(|e| Error::Zip(format!("finish: {e}")))?;
        Ok(cursor.into_inner())
    }
}

/// `SAVE-08` 不变式 2 自检（调试构建）：脏 part 中 `Clean` 节点的原文子串都出现在输出里（抽样）。
#[cfg(debug_assertions)]
fn check_clean_substrings(dom: &crate::xml::Dom, out: &[u8], uri: &str) {
    use crate::xml::NodeKind;
    let mut checked = 0;
    // 手工前序遍历：不进入 `Deleted` 子树（其中的 `Clean` 后代本来就不输出）
    let mut stack = vec![dom.root()];
    while let Some(id) = stack.pop() {
        if checked >= 64 {
            break;
        }
        let node = dom.node(id);
        if node.dirty == Dirty::Deleted {
            continue;
        }
        stack.extend(dom.children(id).iter().rev());
        if node.dirty != Dirty::Clean || !matches!(node.kind, NodeKind::Element(_)) {
            continue;
        }
        let Some(lex) = &node.lex else { continue };
        let bytes = dom.lex_bytes(&lex.range);
        if bytes.len() < 8 {
            continue;
        }
        checked += 1;
        debug_assert!(
            memchr::memmem::find(out, bytes).is_some(),
            "SAVE-08: clean node {} of {uri} is not a substring of the output",
            id.0
        );
    }
}
