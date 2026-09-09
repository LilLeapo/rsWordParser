//! AGENT-10：会话生命周期与共享工具分派；空闲回收只在请求之间运行。
use rsword_agent_query::{
    Result,
    budget::Budget,
    cursor::hash,
    edit::WorkerConfig,
    error,
    output::{self, Publication},
    paging,
    session::{ReadRequest, Sessions},
    tools::{self, Tool},
    transport::Shape,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Config {
    pub shape: Shape,
    pub max_sessions: usize,
    pub idle_timeout: Duration,
    pub worker: WorkerConfig,
}
pub struct Service {
    config: Config,
    sessions: Sessions,
    touched: BTreeMap<String, Instant>,
    schemas: BTreeMap<&'static str, jsonschema::Validator>,
}
pub struct Reply {
    pub value: Value,
    pub publication: Option<Publication>,
}
impl Service {
    pub fn new(config: Config) -> Result<Self> {
        if config.max_sessions == 0
            || config.max_sessions > 32
            || config.idle_timeout.is_zero()
            || config.idle_timeout > Duration::from_secs(1800)
        {
            return Err(error("BIND_BAD_ARGUMENT", "会话上限须为 1..32，空闲时间须为 (0,1800] 秒"));
        }
        let schemas = Tool::ALL
            .iter()
            .map(|t| {
                jsonschema::validator_for(&t.mcp_schema())
                    .map(|v| (t.mcp(), v))
                    .map_err(|e| error("AGENT_INTERNAL", e.to_string()))
            })
            .collect::<Result<_>>()?;
        Ok(Self { config, sessions: Sessions::default(), touched: BTreeMap::new(), schemas })
    }
    pub fn list(&self) -> Value {
        json!({"tools": Tool::ALL.iter().map(|t| {
            // 单请求可切 text，不能声明要求所有结果含 structuredContent 的 outputSchema。
            json!({"name":t.mcp(),"description":t.description(),"inputSchema":t.mcp_schema()})
        }).collect::<Vec<_>>()})
    }
    pub fn session_count(&self) -> usize {
        self.touched.len()
    }
    pub fn expire(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .touched
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= self.config.idle_timeout)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.sessions.close(&id);
            self.touched.remove(&id);
        }
    }
    pub fn call(&mut self, name: &str, arguments: Value) -> Reply {
        let shape = match arguments["resultShape"].as_str() {
            Some("text") => Shape::Text,
            Some("structured") => Shape::Structured,
            _ => self.config.shape,
        };
        let maximum = arguments
            .get("maxBytes")
            .and_then(Value::as_u64)
            .filter(|n| (512..=4194304).contains(n))
            .unwrap_or(24000) as usize;
        self.expire(Instant::now());
        let id = arguments["sessionId"].as_str().map(str::to_owned);
        let outcome = self.execute(name, arguments);
        // 请求处理期间不回收；完成时重新计时，包括现存会话上的具名业务拒绝。
        if let Some(id) = id
            && let Some(t) = self.touched.get_mut(&id)
        {
            *t = Instant::now();
        }
        match outcome {
            Ok((value, publication)) => Reply { value: shape.result(&value, false), publication },
            Err(e) => Reply {
                value: shape.result(&json!(tools::bounded_error(e, maximum)), true),
                publication: None,
            },
        }
    }
    fn execute(&mut self, name: &str, args: Value) -> Result<(Value, Option<Publication>)> {
        let tool = Tool::from_mcp(name).ok_or_else(|| error("BIND_BAD_ARGUMENT", "未知工具"))?;
        if let Err(e) = self.schemas[tool.mcp()].validate(&args) {
            return Err(error("BIND_BAD_ARGUMENT", e.to_string()));
        }
        let b = Budget {
            limit: unsigned(&args, "limit")?.unwrap_or(tool.budget().limit as u64) as usize,
            max_bytes: unsigned(&args, "maxBytes")?.unwrap_or(tool.budget().max_bytes as u64)
                as usize,
        }
        .validate()?;
        let id = args["sessionId"].as_str().unwrap_or("");
        let opts = &args["options"];
        let cursor = args["cursor"].as_str();
        if tool.needs_session() && tool != Tool::Close {
            self.sessions.snapshot(id)?;
        }
        let version = unsigned(&args, "expectedVersion")?;
        if tool.needs_session()
            && tool != Tool::Close
            && let Some(version) = version
            && self.sessions.snapshot(id)?["version"] != version
        {
            return Err(error("AGENT_VERSION_CONFLICT", "expectedVersion 不匹配"));
        }
        if matches!(
            tool,
            Tool::Open | Tool::Close | Tool::Version | Tool::Edit | Tool::Save | Tool::AddMedia
        ) && cursor.is_some()
        {
            return Err(error("AGENT_BAD_CURSOR", "固定回执或写请求不消费游标"));
        }
        if tool == Tool::Media && opts.get("id").is_some() {
            if cursor.is_some() {
                return Err(error("AGENT_BAD_CURSOR", "媒体导出不消费游标"));
            }
            let bytes = self.sessions.media(id, unsigned(opts, "id")?.unwrap() as u32)?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err(error("AGENT_RESOURCE_LIMIT", "媒体超过 16 MiB"));
            }
            let path = string(opts, "output")?;
            let value = tools::fixed(
                tool,
                json!({"id":opts["id"],"output":path,"length":bytes.len(),"sha256":hash(&bytes)}),
                b,
            )?;
            return Ok((
                value,
                Some(output::publish(
                    vec![(PathBuf::from(path), bytes)],
                    opts["overwrite"] == true,
                )?),
            ));
        }
        if let Some(read) = tool.read() {
            let mut options = opts.clone();
            if tool == Tool::Media
                && (opts.get("output").is_some() || opts.get("overwrite").is_some())
            {
                return Err(error("BIND_BAD_ARGUMENT", "导出媒体需要 id"));
            }
            if tool == Tool::Context && options.get("anchorOffset").is_some() {
                return Err(error(
                    "BIND_BAD_ARGUMENT",
                    "MCP context 使用当前会话的 anchor，不接受文件专用 anchorOffset",
                ));
            }
            if tool == Tool::Media {
                options.as_object_mut().unwrap().remove("id");
            }
            let worker = if tool == Tool::Find { Some(self.config.worker.start()?) } else { None };
            return Ok((
                self.sessions.read(
                    id,
                    &ReadRequest { tool: read, options },
                    Some(b),
                    cursor,
                    worker,
                )?,
                None,
            ));
        }
        let value = match tool {
            Tool::Open => {
                if self.touched.len() >= self.config.max_sessions {
                    return Err(error("AGENT_RESOURCE_LIMIT", "会话数达到上限，请 close 后重试"));
                }
                let bytes = read_file(Path::new(string(opts, "path")?), 256 * 1024 * 1024)?;
                let id = self.sessions.open(&bytes)?;
                let receipt = tools::fixed(tool, self.sessions.snapshot(&id)?, b);
                match receipt {
                    Ok(v) => {
                        self.touched.insert(id, Instant::now());
                        v
                    }
                    Err(e) => {
                        self.sessions.close(&id);
                        return Err(e);
                    }
                }
            }
            Tool::Close => {
                let v = tools::fixed(tool, json!({"closed":true}), b)?;
                self.sessions.close(id);
                self.touched.remove(id);
                v
            }
            Tool::Version => tools::fixed(tool, tools::version(), b)?,
            Tool::Edit => {
                let mut input = opts.clone();
                let native =
                    input.as_object_mut().unwrap().remove("nativeDebug") == Some(json!(true));
                let preview = input.as_object_mut().unwrap().remove("previewId");
                let worker = &self.config.worker;
                self.sessions.transaction(id, |sessions| {
                    let receipt = if native {
                        if preview.is_some() {
                            return Err(error("BIND_BAD_ARGUMENT", "原生调试不消费 Agent 预览"));
                        }
                        sessions.edit_native_report(id, version.unwrap(), &input.to_string())?
                    } else {
                        sessions.edit(
                            id,
                            version.unwrap(),
                            &input.to_string(),
                            preview.as_ref().and_then(Value::as_str),
                            Some(worker),
                        )?
                    };
                    tools::fixed(tool, receipt, b)
                })?
            }
            Tool::Preview => {
                if cursor.is_some() {
                    return Err(error("AGENT_BAD_CURSOR", "预览续读请用 summary 和 reportId"));
                }
                self.sessions.preview(
                    id,
                    version.unwrap(),
                    &opts.to_string(),
                    b,
                    Some(&self.config.worker),
                )?
            }
            Tool::Summary => self.sessions.summary(id, string(opts, "reportId")?, b, cursor)?,
            Tool::AddMedia => {
                let bytes = read_file(Path::new(string(opts, "path")?), 16 * 1024 * 1024)?;
                let mime = string(opts, "mime")?;
                self.sessions.transaction(id, |sessions| {
                    let mut receipt = sessions.add_media(id, version.unwrap(), &bytes, mime)?;
                    receipt["sha256"] = json!(hash(&bytes));
                    receipt["length"] = json!(bytes.len());
                    receipt["mime"] = json!(mime);
                    tools::fixed(tool, receipt, b)
                })?
            }
            Tool::Save => {
                let options = opts.get("saveOptions").map(Value::to_string);
                let bytes = self.sessions.save(id, options.as_deref())?;
                let path = string(opts, "output")?;
                let value = tools::fixed(
                    tool,
                    json!({"snapshot":self.sessions.snapshot(id)?,"output":path,"length":bytes.len(),"sha256":hash(&bytes)}),
                    b,
                )?;
                return Ok((
                    value,
                    Some(output::publish(
                        vec![(PathBuf::from(path), bytes)],
                        opts["overwrite"] == true,
                    )?),
                ));
            }
            Tool::Check => {
                let rows = self.sessions.check_current(id)?;
                self.sessions.records(id, tool.logical(), opts, rows, b, cursor)?
            }
            Tool::Diff => {
                let paths = [
                    Path::new(string(opts, "before")?).canonicalize().map_err(output::io)?,
                    Path::new(string(opts, "after")?).canonicalize().map_err(output::io)?,
                ];
                let mut projections = vec![];
                let mut hashes = vec![];
                for path in &paths {
                    let bytes = read_file(path, 256 * 1024 * 1024)?;
                    hashes.push(hash(&bytes));
                    let mut s = Sessions::default();
                    let id = s.open(&bytes)?;
                    let p = s.projection(&id)?;
                    projections.push(
                        paging::text_units(&p, 0..p.content.encode_utf16().count() as u32)?
                            .into_iter()
                            .map(|u| u.content)
                            .collect::<Vec<_>>(),
                    );
                }
                self.sessions.records(
                    id,
                    tool.logical(),
                    &json!({"paths":paths,"sha256":hashes}),
                    tools::diff_rows(&projections[0], &projections[1]),
                    b,
                    cursor,
                )?
            }
            Tool::Outline
            | Tool::Text
            | Tool::Find
            | Tool::Context
            | Tool::Document
            | Tool::Media => unreachable!("读取已分派"),
        };
        Ok((value, None))
    }
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or_else(|| error("BIND_BAD_ARGUMENT", format!("缺少 {key}")))
}
fn unsigned(v: &Value, key: &str) -> Result<Option<u64>> {
    v.get(key)
        .map(|n| {
            n.as_u64().ok_or_else(|| error("BIND_BAD_ARGUMENT", format!("{key} 需要 u64 整数线型")))
        })
        .transpose()
}
fn read_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(output::io)?;
    let mut bytes = vec![];
    file.take(max as u64 + 1).read_to_end(&mut bytes).map_err(output::io)?;
    if bytes.len() > max {
        return Err(error("AGENT_RESOURCE_LIMIT", "输入文件超出容量上限"));
    }
    Ok(bytes)
}
