//! AGENT-10：真实子进程 stdio 协议，不绕过 tools/call 与会话生命周期。
#[path = "../../rsword/tests/common/mod.rs"]
mod common;
use rsword_agent_query::{tools::Tool, transport::Shape};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Client {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Value>,
    next: u64,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Client {
    fn new(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rsword-mcp"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let v: Value =
                    serde_json::from_str(&line.unwrap()).expect("stdout 只能有 JSON-RPC");
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        let mut c = Self { child, input, output: rx, next: 1 };
        let init = c.rpc("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"rsword-test","version":"1"}}));
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
        c.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        c
    }
    fn send(&mut self, v: Value) {
        writeln!(self.input, "{v}").unwrap();
        self.input.flush().unwrap();
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let v = self.output.recv_timeout(Duration::from_secs(20)).expect("服务端必须有界响应");
        assert_eq!(v["id"], id);
        v
    }
    fn raw(&mut self, tool: Tool, args: Value) -> Value {
        let v = self.rpc("tools/call", json!({"name":tool.mcp(),"arguments":args}));
        assert!(v.get("error").is_none(), "{v}");
        v["result"].clone()
    }
    fn call(&mut self, tool: Tool, args: Value) -> Value {
        let max = args["maxBytes"].as_u64().unwrap_or(tool.budget().max_bytes as u64);
        let raw = self.raw(tool, args);
        assert_eq!(raw["isError"], false, "{raw}");
        let value = unwrap(&raw);
        assert_eq!(value["usage"]["responseBytes"], raw.to_string().len());
        assert!(raw.to_string().len() <= max as usize, "工具信封超预算");
        let schema = tool.response_schema();
        jsonschema::validator_for(&schema).unwrap().validate(&value).unwrap();
        value
    }
    fn open(&mut self, path: &PathBuf) -> String {
        self.call(Tool::Open, json!({"options":{"path":path}}))["content"][0]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}
fn unwrap(v: &Value) -> Value {
    if let Some(value) = v.get("structuredContent") {
        assert_eq!(v["content"], json!([{"type":"text","text":"Read structuredContent."}]));
        value.clone()
    } else {
        serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
fn normalized(mut v: Value) -> Value {
    // 允许差异仅限这两个实际传输计数；不删 contentUtf16 或任何业务/分页字段。
    for field in ["responseBytes", "estimatedTokens"] {
        v["usage"].as_object_mut().unwrap().remove(field);
    }
    v
}
struct Case {
    dir: PathBuf,
    input: PathBuf,
    bytes: Vec<u8>,
}
impl Case {
    fn new() -> Self {
        let dir = common::repo_root().join(format!(
            "target/m97-tests/{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.docx");
        let bytes = common::docx_with_body(
            "<w:p><w:r><w:t>alpha beta</w:t></w:r></w:p><w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p><w:p><w:r><w:t>third row</w:t></w:r></w:p>",
        );
        fs::write(&input, &bytes).unwrap();
        Self { dir, input, bytes }
    }
    fn operations(&self) -> Value {
        let mut p = rsword::package::Package::open(&self.bytes).unwrap();
        let d = rsword::model::Document::rebuild(&mut p).unwrap();
        let text =
            rsword::agent::text::project(&p, &d, rsword::agent::text::Scope::Main, "test").unwrap();
        let scope: Vec<_> = text
            .objects
            .values()
            .filter(|o| o.object.kind == "paragraph")
            .map(|o| json!(o.object))
            .collect();
        json!({"operations":[{"action":"replaceText","selector":{"scope":scope,"find":"alpha"},"text":"delta"}]})
    }
}
#[test]
fn agent_10_mcp_table_schema_and_all_tools_end_to_end() {
    let case = Case::new();
    let mut c = Client::new(&[]);
    let listed = c.rpc("tools/list", json!({}));
    let rows = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(rows.len(), Tool::ALL.len());
    for (row, t) in rows.iter().zip(Tool::ALL) {
        assert_eq!(row["name"], t.mcp());
        assert_eq!(row["inputSchema"], t.mcp_schema());
        assert!(row["description"].as_str().unwrap().contains("先 open，再 outline"));
        assert!(row.get("outputSchema").is_none(), "可返回 text，不能承诺每次 structuredContent");
    }
    let mut visited = std::collections::BTreeSet::new();
    let id = c.open(&case.input);
    visited.insert(Tool::Open.mcp());
    let request = |options: Value| json!({"sessionId":id,"options":options});
    for t in [Tool::Outline, Tool::Text, Tool::Document, Tool::Media, Tool::Check] {
        let options =
            if t == Tool::Document { json!({"blockRange":{"from":0,"to":1}}) } else { json!({}) };
        let value = c.call(t, request(options));
        visited.insert(t.mcp());
        if t == Tool::Text {
            let anchor = value["anchors"]["segments"][0].clone();
            // context 需要具体双向锚点，下面从同一会话的 find 响应取得。
            assert!(!anchor.is_null());
        }
    }
    let found = c.call(Tool::Find, request(json!({"pattern":"alpha"})));
    visited.insert(Tool::Find.mcp());
    let hit = &found["content"][0];
    let anchor = hit["anchors"]["start"].clone();
    // 实际命中结构在断言中打印，防止用猜测字段生成伪锚点。
    assert!(!anchor.is_null(), "{found}");
    c.call(Tool::Context, request(json!({"anchor":anchor,"before":0,"after":0})));
    visited.insert(Tool::Context.mcp());
    let ops = case.operations();
    let preview = c.call(
        Tool::Preview,
        json!({"sessionId":id,"expectedVersion":0,"options":ops,"limit":100000,"maxBytes":1000000}),
    );
    visited.insert(Tool::Preview.mcp());
    let report = preview["range"]["reportId"].clone();
    c.call(Tool::Summary, request(json!({"reportId":report})));
    visited.insert(Tool::Summary.mcp());
    let mut edit = ops.clone();
    edit["previewId"] = report;
    let receipt = c.call(Tool::Edit, json!({"sessionId":id,"expectedVersion":0,"options":edit}));
    visited.insert(Tool::Edit.mcp());
    assert_eq!(receipt["content"][0]["afterVersion"], 1);
    let after = c.call(Tool::Text, request(json!({})));
    assert!(after["content"].as_str().unwrap().contains("delta"));
    let output = case.dir.join("saved.docx");
    c.call(Tool::Save, json!({"sessionId":id,"expectedVersion":1,"options":{"output":output}}));
    visited.insert(Tool::Save.mcp());
    let saved = fs::read(&output).unwrap();
    assert_ne!(saved, case.bytes);
    assert!(
        common::part_bytes(&saved, "word/document.xml")
            .windows(b"UNTOUCHED".len())
            .any(|w| w == b"UNTOUCHED")
    );
    c.call(Tool::Diff, json!({"sessionId":id,"options":{"before":case.input,"after":output}}));
    visited.insert(Tool::Diff.mcp());
    let png = case.dir.join("image.png");
    fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
    let media = c.call(
        Tool::AddMedia,
        json!({"sessionId":id,"expectedVersion":1,"options":{"path":png,"mime":"image/png"}}),
    );
    visited.insert(Tool::AddMedia.mcp());
    let media_id = media["content"][0]["mediaId"].clone();
    let export = case.dir.join("export.png");
    c.call(Tool::Media, request(json!({"id":media_id,"output":export})));
    assert_eq!(fs::read(export).unwrap(), fs::read(png).unwrap());
    c.call(Tool::Version, json!({"options":{}}));
    visited.insert(Tool::Version.mcp());
    for _ in 0..2 {
        c.call(Tool::Close, request(json!({})));
    }
    visited.insert(Tool::Close.mcp());
    assert_eq!(visited, Tool::ALL.iter().map(|t| t.mcp()).collect());
}
#[test]
fn agent_06_mcp_cross_shape_cursor_and_between_costs() {
    let case = Case::new();
    let mut c = Client::new(&[]);
    let id = c.open(&case.input);
    let request = |shape: &str, token: Value| json!({"sessionId":id,"options":{},"limit":12,"maxBytes":24000,"resultShape":shape,"cursor":token});
    let full =
        c.call(Tool::Text, json!({"sessionId":id,"options":{},"maxBytes":1000000,"limit":100000}));
    let mut mixed = String::new();
    let mut one = String::new();
    let mut token = Value::Null;
    let mut pages = 0;
    loop {
        let text = c.call(Tool::Text, request("text", token.clone()));
        let structured = c.call(Tool::Text, request("structured", token.clone()));
        assert_eq!(normalized(text.clone()), normalized(structured.clone()));
        one.push_str(text["content"].as_str().unwrap());
        let page = if pages % 2 == 0 { text } else { structured };
        mixed.push_str(page["content"].as_str().unwrap());
        token = page["nextCursor"].clone();
        pages += 1;
        if token.is_null() {
            break;
        }
    }
    assert!(pages > 1);
    assert_eq!(mixed, one);
    assert_eq!(mixed, full["content"].as_str().unwrap());
    let v = rsword_agent_query::tools::fixed(
        Tool::Version,
        rsword_agent_query::tools::version(),
        rsword_agent_query::budget::Budget::CONTEXT,
    )
    .unwrap();
    let costs = [Shape::Text, Shape::Structured].map(|s| s.result(&v, false).to_string().len());
    assert!(costs[0] != costs[1]);
    let minimum = *costs.iter().max().unwrap();
    let between = *costs.iter().min().unwrap();
    for shape in ["text", "structured"] {
        let raw =
            c.raw(Tool::Version, json!({"options":{},"resultShape":shape,"maxBytes":between}));
        let e = unwrap(&raw);
        assert_eq!(raw["isError"], true);
        assert_eq!(e["code"], "AGENT_BUDGET_TOO_SMALL");
        assert_eq!(e["details"]["minBytes"], minimum);
    }
}
#[test]
fn agent_10_mcp_failed_edit_budget_is_atomic_and_cursor_stales_only_on_success() {
    let case = Case::new();
    let mut c = Client::new(&[]);
    let id = c.open(&case.input);
    let page = c.call(Tool::Text, json!({"sessionId":id,"options":{},"limit":12}));
    let output = case.dir.join("unchanged.docx");
    let mut args =
        json!({"sessionId":id,"expectedVersion":0,"options":case.operations(),"limit":1});
    let failed = c.raw(Tool::Edit, args.clone());
    assert_eq!(unwrap(&failed)["code"], "AGENT_BUDGET_TOO_SMALL");
    c.call(Tool::Save, json!({"sessionId":id,"expectedVersion":0,"options":{"output":output}}));
    assert_eq!(fs::read(output).unwrap(), case.bytes);
    c.call(Tool::Text, json!({"sessionId":id,"options":{},"cursor":page["nextCursor"]}));
    args["limit"] = json!(8000);
    c.call(Tool::Edit, args);
    assert_eq!(
        unwrap(
            &c.raw(Tool::Text, json!({"sessionId":id,"options":{},"cursor":page["nextCursor"]}))
        )["code"],
        "AGENT_STALE_CURSOR"
    );
}
#[test]
fn agent_10_mcp_idle_limit_and_native_debug_version() {
    let case = Case::new();
    let mut c = Client::new(&["--max-sessions", "1", "--idle-timeout-ms", "1000"]);
    let id = c.open(&case.input);
    assert_eq!(
        unwrap(&c.raw(Tool::Open, json!({"options":{"path":case.input}})))["code"],
        "AGENT_RESOURCE_LIMIT"
    );
    let edit = c.call(
        Tool::Edit,
        json!({"sessionId":id,"expectedVersion":0,"options":{"nativeDebug":true,"operations":[]}}),
    );
    assert_eq!(edit["content"][0]["afterVersion"], 1);
    assert_eq!(unwrap(&c.raw(Tool::Save,json!({"sessionId":id,"expectedVersion":0,"options":{"output":case.dir.join("stale.docx")}})))["code"],"AGENT_VERSION_CONFLICT");
    std::thread::sleep(Duration::from_millis(1300));
    assert_eq!(
        unwrap(&c.raw(Tool::Text, json!({"sessionId":id,"options":{}})))["code"],
        "BIND_NO_SESSION"
    );
    c.open(&case.input);
}
#[test]
fn agent_10_mcp_hostile_exact_named_refusals() {
    let mut c = Client::new(&[]);
    let paths = common::docx_paths("hostile");
    assert_eq!(paths.len(), 38);
    let mut rejected = std::collections::BTreeSet::new();
    let mut count = 0;
    for path in paths {
        let relative = path
            .strip_prefix(common::repo_root().join("corpus"))
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let raw = c.raw(Tool::Open, json!({"options":{"path":path}}));
        if raw["isError"] == true {
            assert!(common::UNOPENABLE.contains(&relative.as_str()), "意外拒绝 {relative}: {raw}");
            rejected.insert(relative);
            continue;
        }
        assert!(!common::UNOPENABLE.contains(&relative.as_str()), "必须拒绝 {relative}");
        let id = unwrap(&raw)["content"][0]["sessionId"].as_str().unwrap().to_owned();
        c.call(
            Tool::Check,
            json!({"sessionId":id,"options":{},"maxBytes":4194304,"limit":1048576}),
        );
        c.call(Tool::Close, json!({"sessionId":id,"options":{}}));
        count += 1;
    }
    assert_eq!(count, 34);
    assert_eq!(rejected, common::UNOPENABLE.iter().map(|s| s.to_string()).collect());
}
macro_rules! missing_sessions {
    ($($name:ident: $tool:ident => $options:expr;)*) => {
        const NO_SESSION: &[Tool] = &[$(Tool::$tool),*];
        $(#[test] fn $name() {
            let mut c=Client::new(&[]);
            let mut args=json!({"sessionId":"absent","options":$options});
            if Tool::$tool.needs_version() {args["expectedVersion"]=json!(0);}
            let result=c.raw(Tool::$tool,args);
            assert_eq!(result["isError"],true);
            assert_eq!(unwrap(&result)["code"],"BIND_NO_SESSION");
        })*
    }
}
missing_sessions! {
    agent_10_outline_no_session: Outline => json!({});
    agent_10_text_no_session: Text => json!({});
    agent_10_find_no_session: Find => json!({"pattern":"a"});
    agent_10_context_no_session: Context => json!({});
    agent_10_model_no_session: Document => json!({"blockRange":{"from":0,"to":1}});
    agent_10_preview_no_session: Preview => json!({"operations":[]});
    agent_10_edit_no_session: Edit => json!({"operations":[]});
    agent_10_save_no_session: Save => json!({"output":"unused"});
    agent_10_summary_no_session: Summary => json!({"reportId":"absent"});
    agent_10_media_no_session: Media => json!({});
    agent_10_add_media_no_session: AddMedia => json!({"path":"unused","mime":"image/png"});
    agent_10_check_no_session: Check => json!({});
    agent_10_diff_no_session: Diff => json!({"before":"unused","after":"unused"});
}
#[test]
fn agent_10_missing_session_test_rows_are_exact() {
    assert_eq!(
        NO_SESSION.iter().map(|t| t.mcp()).collect::<std::collections::BTreeSet<_>>(),
        Tool::ALL
            .iter()
            .filter(|t| t.needs_session() && **t != Tool::Close)
            .map(|t| t.mcp())
            .collect()
    );
}
#[test]
fn agent_10_invalid_protocol_and_inputs_do_not_break_connection() {
    let mut c = Client::new(&["--result-shape", "structured"]);
    for invalid in [
        b"{broken}\n".to_vec(),
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"id\":8,\"method\":\"ping\"}\n".to_vec(),
        format!("{}0{}\n", "[".repeat(129), "]".repeat(129)).into_bytes(),
        vec![b'x'; 4 * 1024 * 1024 + 1],
    ] {
        c.input.write_all(&invalid).unwrap();
        if !invalid.ends_with(b"\n") {
            c.input.write_all(b"\n").unwrap();
        }
        c.input.flush().unwrap();
        let v = c.output.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(c.rpc("ping", json!({}))["result"], json!({}));
    }
    let invalid = c.raw(Tool::Open, json!({"options":{"path":"must-not-be-read"},"maxBytes":511}));
    assert_eq!(unwrap(&invalid)["code"], "BIND_BAD_ARGUMENT");
    let invalid = c.raw(Tool::Version, json!({"options":{},"unknown":true}));
    assert_eq!(unwrap(&invalid)["code"], "BIND_BAD_ARGUMENT");
    let v = c.raw(Tool::Version, json!({"options":{}}));
    assert!(v.get("structuredContent").is_some());
}
#[test]
fn agent_10_stdout_failure_restores_saved_file() {
    let case = Case::new();
    let output = case.dir.join("previous.docx");
    fs::write(&output, b"original output").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rsword-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |v: Value| {
        writeln!(input, "{v}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    exchange(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
    );
    writeln!(input, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
    writeln!(input,"{}",json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"open","arguments":{"options":{"path":case.input}}}})).unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let opened: Value = serde_json::from_str(&line).unwrap();
    let id = unwrap(&opened["result"])["content"][0]["sessionId"].clone();
    drop(stdout); // 实际关闭读端，让 save 回执写 stdout 时触发 EPIPE。
    writeln!(input,"{}",json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"save","arguments":{"sessionId":id,"expectedVersion":0,"options":{"output":output,"overwrite":true}}}})).unwrap();
    input.flush().unwrap();
    drop(input);
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("stdout 失败未退出");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(output).unwrap(), b"original output");
}
#[test]
fn agent_10_default_session_limit_is_32_and_reads_honor_version() {
    let case = Case::new();
    let mut c = Client::new(&[]);
    let ids: Vec<_> = (0..32).map(|_| c.open(&case.input)).collect();
    let e = unwrap(&c.raw(Tool::Open, json!({"options":{"path":case.input}})));
    assert_eq!(e["code"], "AGENT_RESOURCE_LIMIT");
    let e =
        unwrap(&c.raw(Tool::Text, json!({"sessionId":ids[0],"expectedVersion":1,"options":{}})));
    assert_eq!(e["code"], "AGENT_VERSION_CONFLICT");
    c.call(Tool::Close, json!({"sessionId":ids[0],"options":{}}));
    c.open(&case.input);
    c.call(Tool::Text, json!({"sessionId":ids[1],"expectedVersion":0,"options":{}}));
}
#[test]
fn agent_10_diff_owns_session_cursor_and_rejects_file_cursor() {
    let case = Case::new();
    let after = case.dir.join("after.docx");
    fs::write(&after,common::docx_with_body("<w:p><w:r><w:t>changed first</w:t></w:r></w:p><w:p><w:r><w:t>changed second</w:t></w:r></w:p><w:p><w:r><w:t>changed third</w:t></w:r></w:p>")).unwrap();
    let mut c = Client::new(&[]);
    let id = c.open(&case.input);
    let mut request =
        json!({"sessionId":id,"options":{"before":case.input,"after":after},"limit":150});
    let page = c.call(Tool::Diff, request.clone());
    let token = page["nextCursor"].as_str().expect("diff 必须实际分页");
    let wire = token
        .strip_prefix("a1.")
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let wire: Value = serde_json::from_slice(&wire).unwrap();
    assert_eq!(wire["kind"], "session");
    assert_eq!(
        wire.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["handle", "kind"],
        "句柄不应包含文件身份/hash"
    );
    let file_registry = rsword_agent_query::cursor::Registry::default();
    // 文件入口实际生成自包含游标；不是随手伪造一段无法解码的字符串。
    let file_page = rsword_agent_query::tools::file_page(
        json!([case.input, after]),
        "digest",
        Tool::Diff,
        &json!({}),
        vec![json!({"text":"a"}), json!({"text":"b"})],
        rsword_agent_query::budget::Budget { limit: 15, max_bytes: 24000 },
        None,
    )
    .unwrap();
    let file_token = file_page["nextCursor"].as_str().expect("必须是实际文件游标");
    request["cursor"] = json!(file_token);
    let rejected = unwrap(&c.raw(Tool::Diff, request.clone()));
    assert_eq!(rejected["code"], "AGENT_BAD_CURSOR");
    assert_eq!(rejected["message"], "MCP 需要会话句柄，不能接收 CLI 文件游标");
    request["cursor"] = json!(token);
    request["limit"] = json!(8000);
    let next = c.call(Tool::Diff, request);
    assert_eq!(next["truncated"], false);
    // 同一 token 也不能脱离服务端 Registry 被当成可重放文件位置。
    assert_eq!(
        file_registry.resume("new", "diff", "{}", Some(token)).unwrap_err().code,
        "AGENT_BAD_CURSOR"
    );
}
#[test]
fn agent_10_integer_parameters_never_fall_back_or_panic() {
    let mut c = Client::new(&[]);
    for field in ["limit", "maxBytes"] {
        let mut args = json!({"options":{}});
        args[field] = json!(512.0);
        let result = c.raw(Tool::Version, args);
        assert_eq!(result["isError"], true, "非整数线型不能回退到默认预算");
        assert_eq!(unwrap(&result)["code"], "BIND_BAD_ARGUMENT");
    }
    let case = Case::new();
    let id = c.open(&case.input);
    for version in [json!(0.0), json!(1e30)] {
        let e = c.raw(
            Tool::Edit,
            json!({"sessionId":id,"expectedVersion":version,"options":{"operations":[]}}),
        );
        assert_eq!(unwrap(&e)["code"], "BIND_BAD_ARGUMENT");
    }
    let e = c.raw(
        Tool::Media,
        json!({"sessionId":id,"options":{"id":0.0,"output":case.dir.join("unused")}}),
    );
    assert_eq!(unwrap(&e)["code"], "BIND_BAD_ARGUMENT");
    c.call(Tool::Text, json!({"sessionId":id,"expectedVersion":0,"options":{}}));
}
