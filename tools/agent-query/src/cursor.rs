//! AGENT-06：唯一游标编码；会话凭服务端记录，文件凭身份/完整 SHA-256 与逻辑位置。
use crate::{Result, error};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn encode(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap();
    format!("a1.{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}
fn decode(token: &str) -> Result<Value> {
    let s =
        token.strip_prefix("a1.").ok_or_else(|| error("AGENT_BAD_CURSOR", "游标版本或编码非法"))?;
    if s.len() % 2 != 0 || s.len() > 65536 {
        return Err(error("AGENT_BAD_CURSOR", "游标长度非法"));
    }
    let bytes = s
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| std::str::from_utf8(p).ok().and_then(|s| u8::from_str_radix(s, 16).ok()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| error("AGENT_BAD_CURSOR", "游标编码非法"))?;
    serde_json::from_slice(&bytes).map_err(|_| error("AGENT_BAD_CURSOR", "游标内容非法"))
}
#[derive(Clone)]
struct Entry {
    snapshot: String,
    tool: String,
    config: String,
    position: Value,
}
#[derive(Default, Clone)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
    pub(crate) file: Option<Value>,
}
fn session(snapshot: &str) -> Option<String> {
    serde_json::from_str::<Value>(snapshot).ok()?.get("sessionId")?.as_str().map(str::to_owned)
}
pub(crate) fn validate_file(token: &str, file: &Value, tool: &str) -> Result<Value> {
    let wire = decode(token)?;
    if wire["kind"] != "file" {
        return Err(error("AGENT_BAD_CURSOR", "CLI 需要自包含文件游标，不能接收 MCP 会话句柄"));
    }
    if wire["binding"]["identity"] != file["identity"]
        || wire["binding"]["request"] != file["request"]
        || wire["tool"] != tool
    {
        return Err(error("AGENT_BAD_CURSOR", "文件、工具或配置不匹配"));
    }
    if wire["binding"]["sha256"] != file["sha256"]
        || wire["binding"]["projectionVersion"] != file["projectionVersion"]
    {
        return Err(error("AGENT_STALE_CURSOR", "文件字节或投影版本已变化"));
    }
    Ok(wire)
}
impl Registry {
    pub fn resume(
        &self,
        snapshot: &str,
        tool: &str,
        config: &str,
        cursor: Option<&str>,
    ) -> Result<Option<Value>> {
        let Some(token) = cursor else { return Ok(None) };
        let wire = decode(token)?;
        if let Some(file) = &self.file {
            let wire = validate_file(token, file, tool)?;
            if wire["config"] != hash(config.as_bytes()) {
                return Err(error("AGENT_BAD_CURSOR", "文件、工具或配置不匹配"));
            }
            return Ok(Some(wire["position"].clone()));
        }
        if wire["kind"] != "session" {
            return Err(error("AGENT_BAD_CURSOR", "MCP 需要会话句柄，不能接收 CLI 文件游标"));
        }
        let e =
            self.entries.get(token).ok_or_else(|| error("AGENT_BAD_CURSOR", "未知或伪造游标"))?;
        if e.tool != tool
            || e.config != hash(config.as_bytes())
            || session(&e.snapshot) != session(snapshot)
        {
            return Err(error("AGENT_BAD_CURSOR", "游标会话、工具或配置不匹配"));
        }
        if e.snapshot != snapshot {
            return Err(error("AGENT_STALE_CURSOR", "会话版本已变化"));
        }
        Ok(Some(e.position.clone()))
    }
    pub fn candidate(&self, snapshot: &str, tool: &str, config: &str, position: &Value) -> String {
        if let Some(file) = &self.file {
            let mut pos = position.clone();
            if let Some(o) = pos.as_object_mut() {
                o.remove("object");
            }
            return encode(
                &json!({"kind":"file","binding":file,"tool":tool,"config":hash(config.as_bytes()),"position":pos}),
            );
        }
        self.entries.iter().find(|(_,e)|e.snapshot==snapshot && e.tool==tool && e.config==hash(config.as_bytes()) && e.position==*position).map(|(k,_)|k.clone()).unwrap_or_else(||encode(&json!({"kind":"session","handle":format!("{:016x}",NEXT.fetch_add(1,Ordering::Relaxed))})))
    }
    pub fn commit(
        &mut self,
        token: String,
        snapshot: &str,
        tool: &str,
        config: &str,
        position: Value,
    ) {
        if self.file.is_none() {
            self.entries.insert(
                token,
                Entry {
                    snapshot: snapshot.into(),
                    tool: tool.into(),
                    config: hash(config.as_bytes()),
                    position,
                },
            );
        }
    }
    pub(crate) fn forget(&mut self, id: &str) {
        self.entries.retain(|_, e| session(&e.snapshot).as_deref() != Some(id));
    }
}
