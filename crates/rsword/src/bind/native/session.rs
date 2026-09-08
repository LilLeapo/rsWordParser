//! 会话生命周期、媒体句柄与只读调试出口（`BIND-01/04/05/07/08/09`）。
//! 保存只操作克隆；媒体登记只在成功写入后推进，失败请求不改变规范状态。

use super::{ApiError, DocumentOpts, ProjCx, ToJson, apply_edit_json, document_json};
use crate::edit::{EditContext, EditSession};
use crate::package::media::{MediaId, MediaStore};
use crate::package::{Package, PartId};
use crate::save::SaveOptions;
use crate::xml::{Dirty, Dom, NodeId};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// 不透明会话句柄；只在当前进程内有效。
pub type SessionId = String;
const PROTOCOL: &str = "native/0";
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// 同进程的会话表。语言外壳持有此表，禁止暴露可写的引擎引用。
#[derive(Default)]
pub struct SessionTable {
    sessions: BTreeMap<SessionId, EditSession>,
    media: BTreeMap<SessionId, MediaStore>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct OpenOptions {
    expect_protocol: Option<String>,
}

pub(super) fn bad(message: impl Into<String>) -> ApiError {
    ApiError::new("BIND_BAD_ARGUMENT", message)
}
pub(super) fn unknown() -> ApiError {
    ApiError::new("BIND_ID_UNKNOWN", "句柄不在所选会话 / part 中")
}
pub(super) fn decode<T: serde::de::DeserializeOwned + Default>(
    json: Option<&str>,
) -> Result<T, ApiError> {
    match json {
        None => Ok(T::default()),
        Some(s) => serde_json::from_str(s).map_err(|e| bad(e.to_string())),
    }
}

impl SessionTable {
    /// `BIND-01/08`：成功解析才分配会话；版本不匹配在打开文件之前拒绝。
    pub fn open(&mut self, bytes: &[u8], options: Option<&str>) -> Result<SessionId, ApiError> {
        let options: OpenOptions = decode(options)?;
        if options.expect_protocol.as_deref().is_some_and(|p| p != PROTOCOL) {
            return Err(ApiError::new("BIND_PROTOCOL_MISMATCH", "期望协议与 native/0 不匹配"));
        }
        let session = EditSession::open(bytes)?;
        let mut media = MediaStore::new();
        register_media(&mut media, session.package());
        let id = format!("s{}", NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
        self.sessions.insert(id.clone(), session);
        self.media.insert(id.clone(), media);
        Ok(id)
    }

    /// `BIND-01`：幂等清理；不存在的会话也成功。
    pub fn close(&mut self, id: &str) {
        self.sessions.remove(id);
        self.media.remove(id);
    }

    /// `BIND-08`：构建版本、提交与协议版本。
    pub fn version() -> Value {
        serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "git": env!("RSWORD_GIT_SHA"), "protocol": PROTOCOL})
    }

    fn require_session(&self, id: &str) -> Result<(), ApiError> {
        self.sessions
            .contains_key(id)
            .then_some(())
            .ok_or_else(|| ApiError::new("BIND_NO_SESSION", "会话不存在或已关闭"))
    }

    fn document_inner(&self, id: &str, options: Option<&str>) -> Result<String, ApiError> {
        let opts: super::selection::Selection = decode(options)?;
        let s = &self.sessions[id];
        let mut value =
            document_json(s.package(), s.document(), DocumentOpts { display: opts.display }).0;
        // MediaStore 只追加，删除较早的 part 不得重排后续句柄。
        for entry in value["media"].as_array_mut().expect("model media array") {
            let part = PartId(entry["partId"].as_u64().expect("part id") as u32);
            entry["mediaId"] =
                Value::from(self.media[id].id_for_part(part).expect("registered media").0);
        }
        opts.select(&mut value)?;
        Ok(value.to_string())
    }

    fn apply_inner(
        &mut self,
        id: &str,
        op: &str,
        context: Option<&str>,
    ) -> Result<String, ApiError> {
        let context: EditContext = decode(context)?;
        let s = self.sessions.get_mut(id).expect("checked session");
        let result = apply_edit_json(s, op, &context)?;
        register_media(self.media.get_mut(id).expect("session media"), s.package());
        Ok(result.to_json(&ProjCx { pkg: s.package(), display: false }).to_string())
    }

    fn save_inner(&self, id: &str, options: Option<&str>) -> Result<Vec<u8>, ApiError> {
        let options: SaveOptions = decode(options)?;
        Ok(self.sessions[id].clone().save_with(&options)?)
    }

    fn media_inner(&self, id: &str, media: u32) -> Result<Vec<u8>, ApiError> {
        let store = &self.media[id];
        let part = store.try_get(MediaId(media)).ok_or_else(unknown)?.part;
        current_part_bytes(self.sessions[id].package(), part)
    }

    fn add_media_inner(&mut self, id: &str, bytes: &[u8], mime: &str) -> Result<u32, ApiError> {
        let s = &self.sessions[id];
        for part in s.package().parts().iter().filter(|p| !p.deleted) {
            if s.package().content_types().image_mime(&part.uri).as_deref() == Some(mime)
                && current_part_bytes(s.package(), part.id)? == bytes
            {
                return self.media[id].id_for_part(part.id).map(|m| m.0).ok_or_else(unknown);
            }
        }
        let mut candidate = s.clone();
        // 上面已逐字节去重；引擎的哈希缓存可能指向被 ReplacePartBytes 换过内容的 part。
        let rel = candidate.add_media_with(bytes.to_vec(), mime, false)?;
        let part =
            candidate.package().target_part(candidate.main_part(), &rel).ok_or_else(unknown)?;
        let store = self.media.get_mut(id).expect("session media");
        let media =
            store.intern_part(candidate.package(), part).map_err(|_| bad("媒体 MIME 不受支持"))?;
        self.sessions.insert(id.into(), candidate);
        Ok(media.0)
    }

    fn part_bytes_inner(&self, id: &str, part: u32) -> Result<Vec<u8>, ApiError> {
        current_part_bytes(self.sessions[id].package(), PartId(part))
    }

    fn node_xml_inner(&self, id: &str, node: u32, part: Option<u32>) -> Result<String, ApiError> {
        let s = &self.sessions[id];
        let dom = query_dom(s, part)?;
        let node = NodeId(node);
        if node.0 as usize >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            return Err(unknown());
        }
        let mut scratch = dom.clone();
        if scratch.element(node).is_some() {
            let decls = scratch
                .namespace_scope(node)
                .effective()
                .into_iter()
                .map(|(prefix, ns)| crate::xml::ns::PrefixUse { prefix, ns })
                .collect::<Vec<_>>();
            scratch.add_declarations(node, &decls);
        }
        let mut bytes = Vec::new();
        crate::save::serialize_subtree(&scratch, node, &mut bytes)
            .map_err(|e| ApiError::new("SAVE_INVARIANT", e.to_string()))?;
        String::from_utf8(bytes).map_err(|e| ApiError::new("SAVE_INVARIANT", e.to_string()))
    }

    fn diagnostics_inner(&self, id: &str) -> Result<String, ApiError> {
        Ok(super::edit::edit_diagnostics_json(&self.sessions[id]).to_string())
    }
}

fn register_media(store: &mut MediaStore, pkg: &Package) {
    for p in pkg.parts().iter().filter(|p| !p.deleted) {
        if pkg.content_types().image_mime(&p.uri).is_some() {
            let _ = store.intern_part(pkg, p.id);
        }
    }
}

pub(super) fn query_dom(s: &EditSession, part: Option<u32>) -> Result<&Dom, ApiError> {
    let part = PartId(part.unwrap_or(s.main_part().0));
    s.package()
        .parts()
        .get(part.0 as usize)
        .filter(|p| !p.deleted)
        .and_then(|p| p.dom())
        .ok_or_else(unknown)
}

/// 调试用途（`BIND-09`）。Clean XML 从 ZIP 读原编码，不能用转码后的 Dom 源文本。
pub(super) fn current_part_bytes(pkg: &Package, id: PartId) -> Result<Vec<u8>, ApiError> {
    let p = pkg.parts().get(id.0 as usize).filter(|p| !p.deleted).ok_or_else(unknown)?;
    if let Some(bytes) = p.owned_bytes() {
        return Ok(bytes.to_vec());
    }
    if let Some(dom) = p.dom()
        && (p.is_new() || p.replaced || dom.node(dom.root()).dirty != Dirty::Clean)
    {
        let cx = crate::package::ns_context::NamespaceContext::from_dom(dom, pkg.flavor_of(id));
        return crate::save::serialize::serialize_with(dom, Some(&cx))
            .map_err(|e| ApiError::new("SAVE_INVARIANT", e.to_string()));
    }
    if !p.is_new() {
        return Ok(pkg.zip().clone().read(p.zip_index)?);
    }
    Ok(pkg.clone().read_bytes(id)?)
}

crate::bind_export! { sessions SessionTable;
    /// `BIND-02/10`：按需获取单向模型 JSON。
    document(options: Option<&str> = None) -> String => document_inner, test bind_01_document_no_session;
    /// `BIND-03`：原子应用一条 EditOp JSON。
    apply(op: &str = "{}", context: Option<&str> = None) -> String => apply_inner, test bind_01_apply_no_session;
    /// `BIND-04`：在克隆上保存，成功与失败均不改变会话。
    save(options: Option<&str> = None) -> Vec<u8> => save_inner, test bind_01_save_no_session;
    /// `BIND-05`：按稳定句柄读取媒体原字节。
    media(media: u32 = 0) -> Vec<u8> => media_inner, test bind_01_media_no_session;
    /// `BIND-05`：去重后加入媒体，返回稳定句柄。
    add_media(bytes: &[u8] = b"", mime: &str = "image/png") -> u32 => add_media_inner, test bind_01_add_media_no_session;
    /// `BIND-09`：只读调试出口，读取当前 part 字节。
    part_bytes(part: u32 = 0) -> Vec<u8> => part_bytes_inner, test bind_01_part_bytes_no_session;
    /// `BIND-09`：只读调试出口，读取子树 XML，缺省主 part。
    node_xml(node: u32 = 0, part: Option<u32> = None) -> String => node_xml_inner, test bind_01_node_xml_no_session;
    /// `BIND-07`：累计诊断与成功提交的 XML 逃生口次数。
    diagnostics() -> String => diagnostics_inner, test bind_01_diagnostics_no_session;
}

macro_rules! session_queries {
    ($($name:ident, $inner:ident, $test:ident;)*) => {
        impl SessionTable {$(
            fn $inner(&self, id: &str, ids: &str, part: Option<u32>) -> Result<String, ApiError> {
                super::query::$name(&self.sessions[id], ids, part)
            }
        )*}
        crate::bind_export! { sessions SessionTable;
            $(
                /// `BIND-06`：批量只读查询；错误节点逐项返回，缺省主 part。
                $name(ids: &str = "[]", part: Option<u32> = None) -> String => $inner, test $test;
            )*
        }
    };
}
session_queries! {
    resolve_runs, runs_inner, bind_01_resolve_runs_no_session;
    resolve_paras, paras_inner, bind_01_resolve_paras_no_session;
    resolve_cells, cells_inner, bind_01_resolve_cells_no_session;
    resolve_sections, sections_inner, bind_01_resolve_sections_no_session;
    resolve_table, table_inner, bind_01_resolve_table_no_session;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(table: &SessionTable, id: &str) -> String {
        let s = &table.sessions[id];
        format!(
            "{:?}{:?}{:?}{:?}",
            s.package(),
            s.document(),
            s.diagnostics(),
            table.media[id].iter().collect::<Vec<_>>()
        )
    }
    #[test]
    fn bind_01_failed_open_leaves_no_table_entry() {
        let mut table = SessionTable::default();
        assert!(table.open(b"not a ZIP", None).is_err());
        assert!(table.sessions.is_empty());
        assert!(table.media.is_empty());
    }
    #[test]
    fn bind_01_failed_apply_preserves_interners_and_escape_count() {
        let bytes = crate::save::blank_docx(None).unwrap();
        let mut table = SessionTable::default();
        let id = table.open(&bytes, None).unwrap();
        let root = table.sessions[&id].dom().root().0;
        let op=serde_json::json!({"op":"replaceInlines","para":root,"inlines":[{"kind":"xml","value":"<zz:new xmlns:zz='urn:must-not-intern'/>"}]}).to_string();
        let before = state(&table, &id);
        assert!(table.apply(&id, &op, None).is_err());
        assert_eq!(state(&table, &id), before);
        assert_eq!(table.diagnostics(&id).unwrap(), "{\"diagnostics\":[],\"xmlEscapeCount\":0}");
        assert_eq!(table.save(&id, None).unwrap(), bytes);
    }
    #[test]
    fn bind_01_engine_save_failure_is_functional() {
        let bytes = crate::save::blank_docx(None).unwrap();
        let mut table = SessionTable::default();
        let id = table.open(&bytes, None).unwrap();
        // 仅测试注入序列化故障：保存会先在候选克隆中执行隐私选项，再在未绑定 QName 处失败。
        let s = table.sessions.get_mut(&id).unwrap();
        let part = s.main_part();
        let dom = s.package_mut().dom_mut(part).unwrap().unwrap();
        let prefix = dom.interner_mut().intern("cannotSerialize");
        let node = dom.new_element(crate::xml::QName::new(
            crate::xml::NsId::Unbound(prefix),
            crate::xml::LocalName::P,
        ));
        dom.append_child(dom.root(), node);
        let before = state(&table, &id);
        let projection = table.document(&id, None).unwrap();
        assert!(table.save(&id, Some(r#"{"removePersonalInfo":true}"#)).is_err());
        assert_eq!(state(&table, &id), before);
        assert_eq!(table.document(&id, None).unwrap(), projection);
    }
    #[test]
    fn bind_05_removing_earlier_part_does_not_renumber_media() {
        let mut table = SessionTable::default();
        let id = table.open(&crate::save::blank_docx(None).unwrap(), None).unwrap();
        let a = table.add_media(&id, b"first", "image/png").unwrap();
        let b = table.add_media(&id, b"second", "image/png").unwrap();
        let part = table.media[&id].get(MediaId(a)).part;
        table.sessions.get_mut(&id).unwrap().package_mut().part_mut(part).deleted = true;
        let document: Value = serde_json::from_str(&table.document(&id, None).unwrap()).unwrap();
        assert_eq!(document["media"][0]["mediaId"], b);
        assert_eq!(table.media(&id, b).unwrap(), b"second");
        assert_eq!(table.media(&id, a).unwrap_err().code, "BIND_ID_UNKNOWN");
    }
}
