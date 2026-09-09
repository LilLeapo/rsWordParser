//! AGENT-06：唯一写入版本边界；原生会话表不可从 Agent 会话取得可写引用。
#[path = "session_commands.rs"]
mod commands;
use crate::{
    QueryError, Result,
    budget::Budget,
    cursor::Registry,
    error,
    find::Finder,
    nav::{self, Selection, Unit as WindowUnit},
    paging::{self, Unit},
    search::{self, Worker},
};
use rsword::{
    agent::{
        anchors::Anchor,
        text::{self, Projection, Scope},
    },
    bind::native::SessionTable,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
macro_rules! read_tools {
    ($($name:ident=>$wire:literal,$budget:ident;)*)=>{
        #[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
        pub enum ReadTool {$(#[serde(rename=$wire)] $name),*}
        impl ReadTool {
            pub const ALL:&[Self]=&[$(Self::$name),*];
            pub fn name(self)->&'static str {match self {$(Self::$name=>$wire),*}}
            pub fn budget(self)->Budget {match self {$(Self::$name=>Budget::$budget),*}}
        }
    }
}
read_tools! {
    Text=>"text",CONTEXT; Outline=>"outline",OUTLINE; Find=>"find",FIND;
    Context=>"context",CONTEXT; Document=>"document",CONTEXT;
    Diagnostics=>"diagnostics",CONTEXT; Media=>"media",CONTEXT;
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {
    pub tool: ReadTool,
    #[serde(default = "empty_options")]
    pub options: Value,
}
fn empty_options() -> Value {
    json!({})
}
struct Session {
    native: SessionTable,
    native_id: String,
    version: u64,
    reports: crate::report::Store,
}
#[derive(Default)]
pub struct Sessions {
    sessions: BTreeMap<String, Session>,
    cursors: Registry,
}
impl Sessions {
    /// 文件工具的读取入口：先验指纹，再重建当前会话；从不沿用旧 native id。
    pub fn read_file(
        path: &std::path::Path,
        request: &ReadRequest,
        budget: Option<Budget>,
        cursor: Option<&str>,
        worker: Option<Worker>,
    ) -> Result<Value> {
        let budget = budget.unwrap_or(request.tool.budget()).validate()?;
        let path =
            path.canonicalize().map_err(|_| error("BIND_BAD_ARGUMENT", "无法定位输入文件"))?;
        let bytes =
            std::fs::read(&path).map_err(|_| error("BIND_BAD_ARGUMENT", "无法读取输入文件"))?;
        let mut semantic = request.options.clone();
        if let Some(o) = semantic.as_object_mut() {
            o.remove("maxHits");
            if let Some(search) = o.get_mut("search").and_then(Value::as_object_mut) {
                search.remove("deadlineMs");
            }
        }
        let binding = json!({"identity":path,"sha256":crate::cursor::hash(&bytes),"request":crate::cursor::hash(semantic.to_string().as_bytes()),"projectionVersion":"agent/1-unicode17"});
        if let Some(token) = cursor {
            crate::cursor::validate_file(token, &binding, request.tool.name())?;
        }
        if request.options.get("anchor").is_some() {
            return Err(error("AGENT_STALE_ANCHOR", "文件调用须以 anchorOffset 在新快照重建锚点"));
        }
        let mut sessions = Self::default();
        let id = sessions.open(&bytes)?;
        sessions.cursors.file = Some(binding);
        let mut request = request.clone();
        if request.tool == ReadTool::Context {
            let offset = request
                .options
                .get("anchorOffset")
                .and_then(Value::as_u64)
                .filter(|n| *n <= u32::MAX as u64)
                .ok_or_else(|| error("AGENT_BAD_OFFSET", "文件 context 需要逻辑 anchorOffset"))?;
            request.options.as_object_mut().unwrap().remove("anchorOffset");
            let scope = if request.options["scope"] == "all" { Scope::All } else { Scope::Main };
            request.options["anchor"] = json!(sessions.anchor(&id, scope, offset as u32)?);
        }
        sessions.read(&id, &request, Some(budget), cursor, worker)
    }
    pub fn open(&mut self, bytes: &[u8]) -> Result<String> {
        let mut native = SessionTable::default();
        let native_id = native.open(bytes, None)?;
        let nonce =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("a{}-{nonce}-{native_id}", std::process::id());
        self.sessions.insert(
            id.clone(),
            Session { native, native_id, version: 0, reports: Default::default() },
        );
        Ok(id)
    }
    pub fn close(&mut self, id: &str) {
        self.sessions.remove(id);
        self.cursors.forget(id);
    }
    fn get(&self, id: &str) -> Result<&Session> {
        self.sessions.get(id).ok_or_else(|| error("BIND_NO_SESSION", "会话不存在"))
    }
    pub fn snapshot(&self, id: &str) -> Result<Value> {
        Ok(
            json!({"sessionId":id,"version":self.get(id)?.version,"projectionVersion":"agent/1-unicode17"}),
        )
    }
    fn expected(&self, id: &str, version: u64) -> Result<()> {
        if self.get(id)?.version != version {
            return Err(error("AGENT_VERSION_CONFLICT", "expectedVersion 不匹配"));
        }
        Ok(())
    }
    /// 一批显式原生调试操作也必须通过此版本边界；文字编辑编译由 9.5 接入。
    pub fn edit_native(
        &mut self,
        id: &str,
        version: u64,
        ops: &[String],
        context: Option<&str>,
    ) -> Result<Value> {
        self.expected(id, version)?;
        let native_id = self.get(id)?.native_id.clone();
        let mut native = self.get(id)?.native.clone();
        let mut results = vec![];
        for op in ops {
            results.push(parse(&native.apply(&native_id, op, context)?)?);
        }
        let state = self.sessions.get_mut(id).unwrap();
        state.native = native;
        state.version += 1;
        Ok(json!({"snapshot":self.snapshot(id)?,"results":results}))
    }
    pub fn add_media(&mut self, id: &str, version: u64, bytes: &[u8], mime: &str) -> Result<Value> {
        self.expected(id, version)?;
        crate::media::verify(bytes, mime)?;
        let state = self.sessions.get_mut(id).unwrap();
        let media = state.native.add_media(&state.native_id, bytes, mime)?;
        state.version += 1;
        Ok(json!({"snapshot":self.snapshot(id)?,"mediaId":media}))
    }
    pub fn save(&self, id: &str, options: Option<&str>) -> Result<Vec<u8>> {
        Ok(self.get(id)?.native.clone().save(&self.get(id)?.native_id, options)?)
    }
    pub fn media(&self, id: &str, media: u32) -> Result<Vec<u8>> {
        Ok(self.get(id)?.native.clone().media(&self.get(id)?.native_id, media)?)
    }
    pub fn validate_anchor(&self, id: &str, anchor: &Anchor) -> Result<()> {
        let snapshot = self.snapshot(id)?.to_string();
        if anchor.snapshot != snapshot {
            return Err(error("AGENT_STALE_ANCHOR", "锚点不属于当前会话版本"));
        }
        Ok(())
    }
    /// AGENT-07/09：所有报告容量及候选操作验证完毕才交换业务状态。
    pub fn edit(
        &mut self,
        id: &str,
        version: u64,
        input: &str,
        preview: Option<&str>,
        worker: Option<&crate::edit::WorkerConfig>,
    ) -> Result<Value> {
        self.expected(id, version)?;
        if let Some(preview) = preview {
            let report = self
                .get(id)?
                .reports
                .get(preview)
                .map_err(|_| error("AGENT_PREVIEW_STALE", "预览不属于此会话"))?;
            let request = serde_json::to_value(crate::edit::Request::parse(input)?).unwrap();
            if !preview.starts_with("preview-")
                || report.before_version != version
                || report.request != request
            {
                return Err(error("AGENT_PREVIEW_STALE", "预览版本、操作或上下文不匹配"));
            }
        }
        let state = self.get(id)?;
        let candidate = crate::report::run(
            &state.native,
            &state.native_id,
            self.snapshot(id)?,
            input,
            worker,
            false,
        )?;
        if let Some(preview) = preview
            && state.reports.get(preview)?.audit.execution_hash
                != candidate.report.audit.execution_hash
        {
            return Err(error("AGENT_PREVIEW_STALE", "实际媒体或执行序列已变化"));
        }
        let receipt = json!({"beforeVersion":version,"afterVersion":version+1,"reportId":candidate.report.id,"counts":candidate.counts});
        let state = self.sessions.get_mut(id).unwrap();
        state.native = candidate.native;
        state.version += 1;
        state.reports.insert(candidate.report, false);
        Ok(receipt)
    }
    /// AGENT-08：共用候选执行；首个完整差异放不下时不登记预览。
    pub fn preview(
        &mut self,
        id: &str,
        version: u64,
        input: &str,
        budget: Budget,
        worker: Option<&crate::edit::WorkerConfig>,
    ) -> Result<Value> {
        budget.validate()?;
        self.expected(id, version)?;
        let state = self.get(id)?;
        let candidate = crate::report::run(
            &state.native,
            &state.native_id,
            self.snapshot(id)?,
            input,
            worker,
            true,
        )?;
        let report = &candidate.report;
        let value = paging::page(
            &mut self.cursors,
            &report.snapshot,
            "summary",
            &report.id,
            &report.units(),
            false,
            json!({"reportId":report.id,"executionHash":report.audit.execution_hash}),
            budget,
            None,
            usize::MAX,
        )?;
        self.sessions.get_mut(id).unwrap().reports.insert(candidate.report, true);
        Ok(value)
    }
    /// 历史游标使用报告快照；后续 edit 不会使该快照变化。
    pub fn summary(
        &mut self,
        id: &str,
        report_id: &str,
        budget: Budget,
        cursor: Option<&str>,
    ) -> Result<Value> {
        budget.validate()?;
        let report = self.get(id)?.reports.get(report_id)?.clone();
        paging::page(
            &mut self.cursors,
            &report.snapshot,
            "summary",
            report_id,
            &report.units(),
            false,
            json!({"reportId":report.id,"executionHash":report.audit.execution_hash}),
            budget,
            cursor,
            usize::MAX,
        )
    }
    /// 只返回内部规范锚点，不能借此取得可写会话。
    pub fn anchor(&self, id: &str, scope: Scope, offset: u32) -> Result<Anchor> {
        let snapshot = self.snapshot(id)?.to_string();
        self.get(id)?.native.inspect(&self.get(id)?.native_id, |s, _| {
            text::project(s.package(), s.document(), scope, &snapshot)?
                .anchors
                .to_anchor(offset, None)
                .map_err(QueryError::from)
        })?
    }
    #[allow(clippy::too_many_arguments)]
    pub fn read(
        &mut self,
        id: &str,
        request: &ReadRequest,
        budget: Option<Budget>,
        cursor: Option<&str>,
        worker: Option<Worker>,
    ) -> Result<Value> {
        let b = budget.unwrap_or(request.tool.budget()).validate()?;
        self.read_inner(id, request, Some(b), cursor, worker).map_err(|mut e| {
            while serde_json::to_vec(&e).unwrap().len() > b.max_bytes && !e.message.is_empty() {
                e.message.pop();
            }
            e
        })
    }
    fn read_inner(
        &mut self,
        id: &str,
        request: &ReadRequest,
        budget: Option<Budget>,
        cursor: Option<&str>,
        worker: Option<Worker>,
    ) -> Result<Value> {
        let b = budget.unwrap_or(request.tool.budget()).validate()?;
        validate_options(request)?;
        let native_id = self.get(id)?.native_id.clone();
        let snapshot = self.snapshot(id)?.to_string();
        let mut semantic = request.options.clone();
        if let Some(o) = semantic.as_object_mut() {
            o.remove("maxHits");
            if let Some(anchor) = o.get_mut("anchor").and_then(Value::as_object_mut) {
                anchor.remove("snapshot");
            }
            if let Some(search) = o.get_mut("search").and_then(Value::as_object_mut) {
                search.remove("deadlineMs");
            }
        }
        let config = semantic.to_string();
        // 非搜索读取在任何模型投影前校验游标；搜索由同一 Registry 校验完整归一配置。
        if request.tool != ReadTool::Find {
            self.cursors.resume(&snapshot, request.tool.name(), &config, cursor)?;
        }
        if request.tool == ReadTool::Document {
            let opts = &request.options;
            if opts.get("blockRange").is_none()
                && opts.get("fields").is_none()
                && opts.get("flow").is_none()
            {
                return Err(error(
                    "BIND_BAD_ARGUMENT",
                    "Agent document 必须显式选 blockRange 或 fields",
                ));
            }
            let mut selected = opts.clone();
            if selected.get("fields").is_none() {
                selected["fields"] = json!(["main"]);
            }
            selected.as_object_mut().unwrap().remove("scope");
            selected.as_object_mut().unwrap().remove("flow");
            let main_part =
                self.get(id)?.native.inspect(&native_id, |s, _| s.document().main_part.0)?;
            let (value, part) = if opts.get("flow").is_some() {
                let p = self.get(id)?.native.inspect(&native_id, |s, _| {
                    text::project(s.package(), s.document(), Scope::All, &snapshot)
                })??;
                let flow = p
                    .flows
                    .iter()
                    .find(|f| {
                        opts["flow"]["part"] == f.object.part
                            && opts["flow"]["flow"] == f.object.flow
                    })
                    .ok_or_else(|| error("AGENT_NOT_PROJECTED", "请求的流未投影"))?;
                let models = self.get(id)?.native.inspect(&native_id, |s, media| {
                    crate::detail::Details::build_with_display(
                        s.package(),
                        s.document(),
                        &p,
                        media,
                        opts["display"] == true,
                    )
                })?;
                let blocks = flow
                    .blocks
                    .iter()
                    .map(|o| {
                        models
                            .get(o.part, o.node)
                            .map(|v| v["model"].clone())
                            .ok_or_else(|| error("AGENT_NOT_PROJECTED", "流块缺少模型"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut value =
                    parse(&self.sessions.get_mut(id).unwrap().native.document(&native_id, None)?)?;
                value["main"] = json!(blocks);
                self.get(id)?.native.inspect(&native_id, |s, _| {
                    use rsword::bind::native::json::{ProjCx, ToJson};
                    let doc = s.document();
                    let part = rsword::package::PartId(flow.object.part);
                    let cx = ProjCx { pkg: s.package(), display: opts["display"] == true };
                    value["fields"] =
                        doc.fields_in(part).map_or_else(|| json!([]), |f| f.fields().to_json(&cx));
                    let aux = doc
                        .hf_parts
                        .get(&part)
                        .map(|h| &h.idx)
                        .or_else(|| doc.aux_flows.get(&part))
                        .or_else(|| {
                            [
                                (&doc.footnotes.part, &doc.footnotes.idx),
                                (&doc.endnotes.part, &doc.endnotes.idx),
                                (&doc.comments.part, &doc.comments.idx),
                            ]
                            .into_iter()
                            .find_map(|(p, i)| (*p == Some(part)).then_some(i.as_ref()).flatten())
                        });
                    value["spans"] = if part == doc.main_part {
                        doc.spans.spans().to_json(&cx)
                    } else {
                        aux.map_or_else(|| json!([]), |a| a.spans.spans().to_json(&cx))
                    };
                })?;
                SessionTable::select_model(&mut value, &selected.to_string())?;
                (value, flow.object.part)
            } else {
                (
                    parse(
                        &self
                            .sessions
                            .get_mut(id)
                            .unwrap()
                            .native
                            .document(&native_id, Some(&selected.to_string()))?,
                    )?,
                    main_part,
                )
            };
            let mut range = opts.clone();
            range["totalBlocks"] = value["totalBlocks"].clone();
            range["selectionTruncated"] = value["truncated"].clone();
            let units = document_units(value, part);
            return paging::page(
                &mut self.cursors,
                &snapshot,
                "document",
                &config,
                &units,
                false,
                range,
                b,
                cursor,
                usize::MAX,
            );
        }
        if matches!(request.tool, ReadTool::Diagnostics | ReadTool::Media) {
            let native = &mut self.sessions.get_mut(id).unwrap().native;
            let mut range = json!({"tool":request.tool.name()});
            let rows = if request.tool == ReadTool::Diagnostics {
                let diagnostics = parse(&native.diagnostics(&native_id)?)?;
                range["xmlEscapeCount"] = diagnostics["xmlEscapeCount"].clone();
                diagnostics["diagnostics"].as_array().cloned().unwrap_or_default()
            } else {
                parse(&native.document(&native_id, Some(r#"{"fields":["media"]}"#))?)?["media"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            };
            let rows: Vec<_> =
                rows.into_iter().enumerate().map(|(i, v)| Unit::record(v, i)).collect();
            return paging::page(
                &mut self.cursors,
                &snapshot,
                request.tool.name(),
                &config,
                &rows,
                false,
                range,
                b,
                cursor,
                usize::MAX,
            );
        }
        let scope = if request.options["scope"] == "all" || request.options.get("flow").is_some() {
            Scope::All
        } else {
            Scope::Main
        };
        let p = self.get(id)?.native.inspect(&self.get(id)?.native_id, |s, _| {
            text::project(s.package(), s.document(), scope, &snapshot)
        })??;
        let selections = selections(&p, &request.options)?;
        if request.tool == ReadTool::Find {
            let pattern = request.options["pattern"]
                .as_str()
                .ok_or_else(|| error("BIND_BAD_ARGUMENT", "缺少 pattern"))?;
            let options: search::Options = serde_json::from_value(
                request.options.get("search").cloned().unwrap_or_else(|| json!({})),
            )
            .map_err(|e| error("BIND_BAD_ARGUMENT", e.to_string()))?;
            let max_hits = request.options["maxHits"].as_u64().unwrap_or(20) as usize;
            let worker = worker.ok_or_else(|| error("AGENT_WORKER_FAILED", "缺少已就绪 worker"))?;
            let mut finder = Finder { registry: std::mem::take(&mut self.cursors) };
            let result =
                finder.find(&p, &selections, pattern, options, max_hits, b, cursor, worker);
            self.cursors = finder.registry;
            return result;
        }
        let mut units = vec![];
        let is_text = request.tool == ReadTool::Text;
        let mut ranges = vec![];
        match request.tool {
            ReadTool::Text => {
                for selection in &selections {
                    let range = selection.range(&p)?;
                    ranges.push(range);
                }
                let mut merged: Vec<std::ops::Range<u32>> = vec![];
                for range in &ranges {
                    if let Some(last) = merged.last_mut()
                        && last.end == range.start
                    {
                        last.end = range.end;
                        continue;
                    }
                    merged.push(range.clone());
                }
                for range in merged {
                    units.extend(paging::text_units(&p, range)?);
                }
                if selections.len() == 1 && ranges[0].is_empty() {
                    let flow = &selections[0].flow;
                    units[0].object = json!(flow);
                    units[0].metadata["caret"] = json!(p.anchors.to_flow_anchor(
                        ranges[0].start,
                        rsword::agent::anchors::Affinity::Left,
                        flow.part,
                        flow.flow,
                    )?);
                }
            }
            ReadTool::Outline => {
                let min = request.options["minLevel"].as_u64().unwrap_or(1) as u8;
                let max = request.options["maxLevel"].as_u64().unwrap_or(9) as u8;
                let rows = self.get(id)?.native.inspect(&self.get(id)?.native_id, |s, _| {
                    nav::outline(&p, s.document(), min..max.saturating_add(1))
                })??;
                for row in rows {
                    let object: rsword::agent::anchors::ObjectRef =
                        serde_json::from_value(row["object"].clone()).unwrap();
                    if selections.iter().any(|s| {
                        s.flow.part == object.part
                            && s.flow.flow == object.flow
                            && row["blockRange"]["start"]
                                .as_u64()
                                .is_some_and(|i| s.blocks.contains(&(i as usize)))
                    }) {
                        units.push(Unit::record(row, units.len()));
                    }
                }
            }
            ReadTool::Context => {
                let anchor: Anchor = serde_json::from_value(request.options["anchor"].clone())
                    .map_err(|e| error("AGENT_BAD_ANCHOR", e.to_string()))?;
                self.validate_anchor(id, &anchor)?;
                let (part, flow) = match &anchor.target {
                    rsword::agent::anchors::Target::Source { part, flow, .. } => (*part, *flow),
                    rsword::agent::anchors::Target::Presentation { owner, .. } => {
                        (owner.part, owner.flow)
                    }
                };
                let selection = selections
                    .iter()
                    .find(|s| s.flow.part == part && s.flow.flow == flow)
                    .ok_or_else(|| error("AGENT_NOT_PROJECTED", "锚点流未授权"))?;
                let before = request.options["before"].as_u64().unwrap_or(1) as u32;
                let after = request.options["after"].as_u64().unwrap_or(1) as u32;
                let window = if request.options["unit"] == "utf16" {
                    WindowUnit::Utf16
                } else {
                    WindowUnit::Blocks
                };
                let value = nav::context(&p, &anchor, selection, before, after, window, false)?;
                let range: std::ops::Range<u32> =
                    serde_json::from_value(value["actualRange"].clone()).unwrap();
                units = paging::text_units(&p, range.clone())?;
                let details = if request.options["detail"] == true {
                    Some(self.get(id)?.native.inspect(&self.get(id)?.native_id, |s, media| {
                        crate::detail::Details::build(s.package(), s.document(), &p, media)
                    })?)
                } else {
                    None
                };
                for unit in &mut units {
                    let detail: Vec<_> = p
                        .objects
                        .values()
                        .filter(|o| {
                            o.object.part == part
                                && o.object.flow == flow
                                && o.range.start >= unit.range.start
                                && o.range.end <= unit.range.end
                        })
                        .filter_map(|o| {
                            details
                                .as_ref()?
                                .get(part, o.object.node)
                                .map(|v| json!({"object":o.object,"value":v}))
                        })
                        .collect();
                    unit.content = json!({"text":unit.content,"object":unit.object,"parent":value["parent"],"flow":value["flow"],"requestedRange":value["requestedRange"],"actualRange":unit.range,"detail":detail});
                }
                ranges.push(range);
            }
            ReadTool::Document | ReadTool::Find | ReadTool::Diagnostics | ReadTool::Media => {
                unreachable!()
            }
        }
        paging::page(
            &mut self.cursors,
            &snapshot,
            request.tool.name(),
            &config,
            &units,
            is_text,
            json!(ranges),
            b,
            cursor,
            usize::MAX,
        )
    }
}
fn parse(s: &str) -> Result<Value> {
    let mut de = serde_json::Deserializer::from_str(s);
    de.disable_recursion_limit();
    Value::deserialize(&mut de).map_err(|e| error("BIND_BAD_ARGUMENT", e.to_string()))
}
fn selections(p: &Projection, options: &Value) -> Result<Vec<Selection>> {
    let mut out = vec![];
    for f in &p.flows {
        if let Some(flow) = options.get("flow")
            && (flow["part"] != f.object.part || flow["flow"] != f.object.flow)
        {
            continue;
        }
        let from = options.get("blockRange").and_then(|r| r["from"].as_u64()).unwrap_or(0) as usize;
        let to = options
            .get("blockRange")
            .and_then(|r| r["to"].as_u64())
            .unwrap_or(f.blocks.len() as u64) as usize;
        if from > to {
            return Err(error("BIND_BAD_ARGUMENT", "blockRange 反向"));
        }
        out.push(Selection {
            flow: f.object.clone(),
            blocks: from.min(f.blocks.len())..to.min(f.blocks.len()),
        });
    }
    if out.is_empty() {
        return Err(error("AGENT_NOT_PROJECTED", "请求的流未投影"));
    }
    Ok(out)
}
fn document_units(mut value: Value, part: u32) -> Vec<Unit> {
    let mut nodes = BTreeSet::new();
    let mut field_ids = BTreeSet::new();
    let mut span_ids = BTreeSet::new();
    let mut stack = vec![&value["main"]];
    while let Some(v) = stack.pop() {
        match v {
            Value::Array(a) => stack.extend(a),
            Value::Object(o) => {
                if let Some(n) = o.get("node").and_then(Value::as_u64) {
                    nodes.insert(n);
                }
                if o.get("kind") == Some(&json!("field"))
                    && let Some(id) = o.get("id").and_then(Value::as_u64)
                {
                    field_ids.insert(id);
                }
                for key in ["field", "fieldId"] {
                    if let Some(id) = o.get(key).and_then(Value::as_u64) {
                        field_ids.insert(id);
                    }
                }
                if let Some(comments) = o.get("comments").and_then(Value::as_array) {
                    span_ids.extend(comments.iter().filter_map(Value::as_u64));
                }
                stack.extend(
                    o.iter()
                        .filter(|(key, _)| {
                            key.as_str() != "content"
                                || o.get("contentPart")
                                    .and_then(Value::as_u64)
                                    .is_none_or(|p| p == part as u64)
                        })
                        .map(|(_, v)| v),
                );
            }
            _ => {}
        }
    }
    // 块/透明字段引用也需闭包；父字段及其嵌套字段不能成为悬空索引。
    if let Some(fields) = value["fields"].as_array() {
        let by_id: BTreeMap<_, _> = fields
            .iter()
            .filter(|f| f["part"] == part)
            .filter_map(|f| Some((f["id"].as_u64()?, f)))
            .collect();
        let mut pending: Vec<_> = field_ids.iter().copied().collect();
        while let Some(id) = pending.pop() {
            if let Some(field) = by_id.get(&id) {
                let next = field["nested"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .chain(field["parent"].as_u64());
                for id in next {
                    if field_ids.insert(id) {
                        pending.push(id);
                    }
                }
            }
        }
    }
    for name in ["spans", "fields", "revisions"] {
        if let Some(a) = value.get_mut(name).and_then(Value::as_array_mut) {
            a.retain(|v| {
                v["part"] == part
                    && match name {
                        "spans" => {
                            v["id"].as_u64().is_some_and(|id| span_ids.contains(&id))
                                || [&v["start"]["container"], &v["end"]["container"]]
                                    .iter()
                                    .any(|v| v.as_u64().is_some_and(|n| nodes.contains(&n)))
                        }
                        "fields" => v["id"].as_u64().is_some_and(|n| field_ids.contains(&n)),
                        _ => {
                            v["owner"]["node"].as_u64().is_some_and(|n| nodes.contains(&n))
                                || v["owner"]["field"]
                                    .as_u64()
                                    .is_some_and(|n| field_ids.contains(&n))
                        }
                    }
            });
        }
    }
    let mut units = vec![];
    for (field, v) in value.as_object_mut().unwrap() {
        if matches!(field.as_str(), "totalBlocks" | "truncated") {
            continue;
        }
        if let Some(a) = v.as_array() {
            for (i, v) in a.iter().enumerate() {
                units.push(Unit::record(json!({"field":field,"index":i,"value":v}), units.len()));
            }
        } else {
            units.push(Unit::record(json!({"field":field,"value":v}), units.len()));
        }
    }
    units
}
pub(crate) fn option_keys(tool: ReadTool) -> (&'static [&'static str], &'static [&'static str]) {
    let common: &[&str] = match tool {
        ReadTool::Document => &["flow", "blockRange"],
        ReadTool::Diagnostics | ReadTool::Media => &[],
        _ => &["scope", "flow", "blockRange"],
    };
    let extra: &[&str] = match tool {
        ReadTool::Text => &[],
        ReadTool::Outline => &["minLevel", "maxLevel"],
        ReadTool::Find => &["pattern", "search", "maxHits"],
        ReadTool::Context => &["anchor", "before", "after", "unit", "detail"],
        ReadTool::Document => &["fields", "depth", "display"],
        ReadTool::Diagnostics | ReadTool::Media => &[],
    };
    (common, extra)
}
fn validate_options(r: &ReadRequest) -> Result<()> {
    let o =
        r.options.as_object().ok_or_else(|| error("BIND_BAD_ARGUMENT", "options 必须为对象"))?;
    let (common, extra) = option_keys(r.tool);
    if o.keys().any(|k| !common.contains(&k.as_str()) && !extra.contains(&k.as_str())) {
        return Err(error("BIND_BAD_ARGUMENT", "未知读取选项"));
    }
    if r.tool == ReadTool::Document
        && let Some(fields) = o.get("fields")
    {
        let fields =
            fields.as_array().ok_or_else(|| error("BIND_BAD_ARGUMENT", "fields 必须是数组"))?;
        let scoped = o.contains_key("blockRange") || o.contains_key("flow");
        let declarations = [
            "styles",
            "numbering",
            "theme",
            "settings",
            "fontTable",
            "sources",
            "sourcesPart",
            "sections",
            "mainPart",
            "body",
            "media",
            "warnings",
            "chartParts",
            "diagramParts",
            "inks",
        ];
        if fields.iter().any(|f| {
            f.as_str().is_none_or(|f| !declarations.contains(&f) && !(scoped && f == "main"))
        }) {
            return Err(error(
                "BIND_BAD_ARGUMENT",
                "内容须用 blockRange/flow 选择；fields 单独只接受声明字段",
            ));
        }
    }
    if let Some(scope) = o.get("scope")
        && !matches!(scope.as_str(), Some("main" | "all"))
    {
        return Err(error("BIND_BAD_ARGUMENT", "scope 非法"));
    }
    for (key, fields) in [("flow", ["part", "flow"]), ("blockRange", ["from", "to"])] {
        if let Some(v) = o.get(key)
            && (v.as_object().is_none_or(|o| o.len() != 2)
                || fields.iter().any(|k| v[*k].as_u64().is_none_or(|v| v > u32::MAX as u64)))
        {
            return Err(error("BIND_BAD_ARGUMENT", "流或块范围参数非法"));
        }
    }
    for key in ["before", "after", "minLevel", "maxLevel", "maxHits"] {
        if let Some(v) = o.get(key)
            && v.as_u64().is_none_or(|n| {
                n > u32::MAX as u64
                    || matches!(key, "minLevel" | "maxLevel") && !(1..=9).contains(&n)
                    || key == "maxHits" && !(1..=1000).contains(&n)
            })
        {
            return Err(error("BIND_BAD_ARGUMENT", "数值选项越界"));
        }
    }
    if let Some(v) = o.get("unit")
        && !matches!(v.as_str(), Some("blocks" | "utf16"))
    {
        return Err(error("BIND_BAD_ARGUMENT", "窗口单位非法"));
    }
    if let Some(v) = o.get("detail")
        && !v.is_boolean()
    {
        return Err(error("BIND_BAD_ARGUMENT", "detail 必须是布尔值"));
    }
    Ok(())
}
