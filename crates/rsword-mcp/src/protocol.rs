//! MCP 2025-11-25 stdio：逐行 JSON-RPC，stdout 不写日志，业务拒绝为 isError。
use crate::Service;
use rsword_agent_query::output::Publication;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    sync::mpsc,
    time::Duration,
};
const MAX_LINE: usize = 4 * 1024 * 1024;
pub struct Protocol {
    pub service: Service,
    initialized: bool,
    ready: bool,
}
pub struct Response {
    pub value: Value,
    pub publication: Option<Publication>,
}
fn failure(id: Value, code: i32, message: &str) -> Response {
    Response {
        value: json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
        publication: None,
    }
}
impl Protocol {
    pub fn new(service: Service) -> Self {
        Self { service, initialized: false, ready: false }
    }
    pub fn handle(&mut self, line: &[u8]) -> Option<Response> {
        let v: Value = match std::str::from_utf8(line)
            .ok()
            .and_then(|s| rsword_agent_query::audit::parse(s).ok())
        {
            Some(v) => v,
            None => {
                return Some(failure(
                    Value::Null,
                    -32700,
                    "Invalid JSON, duplicate keys or excessive depth",
                ));
            }
        };
        if !v.is_object()
            || v["jsonrpc"] != "2.0"
            || !v["method"].is_string()
            || v.get("id").is_some_and(|id| !id.is_string() && !id.is_number())
        {
            return Some(failure(Value::Null, -32600, "Invalid Request"));
        }
        let method = v["method"].as_str().unwrap();
        let Some(id) = v.get("id").cloned() else {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        };
        let params = &v["params"];
        let mut publication = None;
        let result = match method {
            "initialize" => {
                if self.initialized {
                    return Some(failure(id, -32600, "Already initialized"));
                }
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(failure(id, -32602, "Invalid initialize parameters"));
                }
                let requested = params["protocolVersion"].as_str().unwrap();
                let version = match requested {
                    "2025-11-25" | "2025-06-18" | "2025-03-26" | "2024-11-05" => requested,
                    _ => "2025-11-25",
                };
                self.initialized = true;
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"rsword-mcp","version":env!("CARGO_PKG_VERSION")},"instructions":"先 open，再 outline，按范围用 text/model/context 下钻。只有一份结果载荷；structured 模式请读取 structuredContent。编辑携带 expectedVersion；完成后 save 到新文件并 close。"})
            }
            "ping" => json!({}),
            _ if !self.ready => {
                return Some(failure(
                    id,
                    -32000,
                    "Initialize and send notifications/initialized first",
                ));
            }
            "tools/list" => {
                if params.get("cursor").is_some() {
                    return Some(failure(id, -32602, "Tool list is not paginated"));
                }
                self.service.list()
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(failure(id, -32602, "Missing tool name"));
                };
                if !params["arguments"].is_object() {
                    return Some(failure(id, -32602, "Missing arguments object"));
                }
                let reply = self.service.call(name, params["arguments"].clone());
                publication = reply.publication;
                reply.value
            }
            _ => return Some(failure(id, -32601, "Method not found")),
        };
        Some(Response { value: json!({"jsonrpc":"2.0","id":id,"result":result}), publication })
    }
}
/// 超长行丢弃至换行，绝不为一行无上限分配；队列容量一，避免慢工具前堆积请求。
fn line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut data = vec![];
    let mut oversized = false;
    loop {
        let part = reader.fill_buf()?;
        if part.is_empty() {
            return Ok(if data.is_empty() && !oversized { None } else { Some(data) });
        }
        let n = part.iter().position(|b| *b == b'\n').map_or(part.len(), |i| i + 1);
        let end = part[n - 1] == b'\n';
        if data.len() + n > MAX_LINE {
            oversized = true;
            data.clear();
        }
        if !oversized {
            data.extend_from_slice(&part[..n]);
        }
        reader.consume(n);
        if end {
            return Ok(Some(data));
        }
    }
}
pub fn serve(mut protocol: Protocol, mut output: impl Write) -> io::Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            match line(&mut reader) {
                Ok(Some(data)) => {
                    if tx.send(Ok(data)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            }
        }
    });
    loop {
        let input = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(input) => input?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                protocol.service.expire(std::time::Instant::now());
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        if let Some(response) = protocol.handle(&input) {
            let mut bytes = serde_json::to_vec(&response.value)?;
            bytes.push(b'\n');
            if let Err(e) = output.write_all(&bytes).and_then(|_| output.flush()) {
                if let Some(publication) = response.publication {
                    publication.rollback().map_err(io::Error::other)?;
                }
                return Err(e);
            }
            if let Some(publication) = response.publication {
                publication.commit();
            }
        }
    }
}
