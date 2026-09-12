//! AGENT-10：真实进程边界、schema、文件副作用及全 hostile 分母。
#[path = "../../rsword/tests/common/mod.rs"]
mod common;
use rsword_agent_query::tools::Tool;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Case {
    dir: PathBuf,
    input: PathBuf,
    ops: PathBuf,
    output: PathBuf,
    report: PathBuf,
    bytes: Vec<u8>,
}
impl Case {
    fn new() -> Self {
        let dir = common::repo_root().join(format!(
            "target/m96-tests/{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.docx");
        let ops = dir.join("ops.json");
        let output = dir.join("output.docx");
        let report = dir.join("report.json");
        let bytes = common::docx_with_body(
            "<w:p><w:r><w:t>alpha beta</w:t></w:r></w:p><w:p><w:r><w:t>UNTOUCHED</w:t></w:r></w:p>",
        );
        fs::write(&input, &bytes).unwrap();
        let mut p = rsword::package::Package::open(&bytes).unwrap();
        let d = rsword::model::Document::rebuild(&mut p).unwrap();
        let projection =
            rsword::agent::text::project(&p, &d, rsword::agent::text::Scope::Main, "test").unwrap();
        let scope: Vec<_> = projection
            .objects
            .values()
            .filter(|o| o.object.kind == "paragraph")
            .map(|o| json!(o.object))
            .collect();
        fs::write(&ops,json!({"operations":[{"action":"replaceText","selector":{"scope":scope,"find":"alpha"},"text":"delta"}]}).to_string()).unwrap();
        Self { dir, input, ops, output, report, bytes }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rsword"))
            .args(args)
            .env("TMPDIR", &self.dir)
            .output()
            .unwrap()
    }
    fn call(&self, args: &[&str], tool: Tool) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}: {} / {}",
            tool.cli(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        validate(tool, &v);
        assert_eq!(v["usage"]["responseBytes"], out.stdout.len());
        v
    }
    fn preview(&self) -> Value {
        self.call(
            &[
                "preview",
                s(&self.input),
                "--ops",
                s(&self.ops),
                "--report",
                s(&self.report),
                "--json",
                "--limit",
                "100000",
                "--maxBytes",
                "1000000",
            ],
            Tool::Preview,
        )
    }
}
fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn validate(tool: Tool, v: &Value) {
    let validator = jsonschema::validator_for(&tool.response_schema()).unwrap();
    assert!(
        validator.is_valid(v),
        "{} schema: {:?}\n{v}",
        tool.cli(),
        validator.iter_errors(v).map(|e| e.to_string()).collect::<Vec<_>>()
    );
    let mut wrong = v.clone();
    wrong["zzSabotage"] = json!(true);
    assert!(!validator.is_valid(&wrong), "根部额外键不能通过 schema");
}
macro_rules! cli_commands {($($name:ident=>$tool:ident;)*)=>{$(#[test]fn $name(){exercise(Tool::$tool);})* const COMMANDS:&[Tool]=&[$(Tool::$tool),*];}}
cli_commands! {
 agent_10_cli_outline=>Outline;agent_10_cli_text=>Text;agent_10_cli_find=>Find;agent_10_cli_context=>Context;
 agent_10_cli_model=>Document;agent_10_cli_ops=>Edit;agent_10_cli_preview=>Preview;agent_10_cli_summary=>Summary;
 agent_10_cli_diff=>Diff;agent_10_cli_media=>Media;agent_10_cli_check=>Check;agent_10_cli_version=>Version;
}
fn exercise(tool: Tool) {
    let c = Case::new();
    let v = match tool {
        Tool::Version => c.call(&["version", "--json"], tool),
        Tool::Find => c.call(&["find", s(&c.input), "--pattern", "alpha", "--json"], tool),
        Tool::Context => c.call(&["context", s(&c.input), "--offset", "0", "--json"], tool),
        Tool::Document => c.call(
            &["model", s(&c.input), "--options", r#"{"blockRange":{"from":0,"to":1}}"#, "--json"],
            tool,
        ),
        Tool::Edit => {
            let v = c.call(
                &["ops", s(&c.input), "--ops", s(&c.ops), "--output", s(&c.output), "--json"],
                tool,
            );
            let xml = common::part_bytes(&fs::read(&c.output).unwrap(), "word/document.xml");
            assert!(String::from_utf8(xml).unwrap().contains("delta beta"));
            assert!(PathBuf::from(format!("{}.report.json", c.output.display())).exists());
            v
        }
        Tool::Preview => c.preview(),
        Tool::Summary => {
            c.preview();
            c.call(
                &["summary", s(&c.report), "--json", "--limit", "100000", "--maxBytes", "1000000"],
                tool,
            )
        }
        Tool::Diff => {
            fs::write(&c.output, common::docx_with_body("<w:p><w:r><w:t>other</w:t></w:r></w:p>"))
                .unwrap();
            c.call(&["diff", s(&c.input), s(&c.output), "--json"], tool)
        }
        Tool::Media => {
            let source = common::repo_root().join("corpus/real/image/image-two-in-run.docx");
            c.call(&["media", s(&source), "--list", "--json"], tool)
        }
        _ => c.call(&[tool.cli(), s(&c.input), "--json"], tool),
    };
    match tool {
        Tool::Text => assert!(v["content"].as_str().unwrap().contains("alpha beta")),
        Tool::Find => assert_eq!(v["content"][0]["match"], "alpha"),
        Tool::Diff => assert!(v["content"][0]["before"].as_str().unwrap().contains("alpha")),
        Tool::Check => {
            assert!(
                v["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["name"] == "noEditSaveIdentity" && r["passed"] == true)
            );
            assert!(
                v["content"].as_array().unwrap().iter().any(|r| r["kind"] == "packageDiagnostics")
            );
        }
        _ => assert!(!v["content"].as_array().unwrap().is_empty()),
    };
    assert_eq!(fs::read(&c.input).unwrap(), c.bytes);
}
#[test]
fn agent_10_tools_match_spec_and_cli_test_rows() {
    let spec = include_str!("../../../spec/22-agent.md").replace("\r\n", "\n");
    let start = spec.find("| 逻辑工具 | CLI").unwrap();
    let table = spec[start..].split("\n\n").next().unwrap();
    let expected: std::collections::BTreeSet<_> = table
        .lines()
        .skip(2)
        .map(|line| {
            let cols: Vec<_> = line.split('|').map(str::trim).collect();
            (cols[1], cols[3])
        })
        .collect();
    let actual: std::collections::BTreeSet<_> =
        Tool::ALL.iter().map(|t| (t.logical(), t.mcp())).collect();
    assert_eq!(actual, expected);
    let commands: std::collections::BTreeSet<_> = COMMANDS.iter().map(|t| t.cli()).collect();
    let registered: std::collections::BTreeSet<_> =
        Tool::ALL.iter().filter_map(|t| Tool::from_cli(t.cli())).map(|t| t.cli()).collect();
    assert_eq!(commands, registered);
    let c = Case::new();
    for t in Tool::ALL {
        let options = match t {
            Tool::Find => json!({"pattern":"alpha"}),
            Tool::Context => json!({"anchorOffset":0}),
            Tool::Edit | Tool::Preview => {
                serde_json::from_slice(&fs::read(&c.ops).unwrap()).unwrap()
            }
            _ => json!({}),
        };
        let mut input =
            json!({"options":options,"limit":t.budget().limit,"maxBytes":t.budget().max_bytes});
        let validator = jsonschema::validator_for(&t.input_schema()).unwrap();
        assert!(validator.is_valid(&input), "{}: {input}", t.logical());
        input["options"]["zzSabotage"] = json!(true);
        assert!(!validator.is_valid(&input), "{}: 多余业务键未拒绝", t.logical());
    }
}
#[test]
fn agent_10_hostile_exact_corpus_and_named_refusals() {
    let c = Case::new();
    let paths = common::docx_paths("hostile");
    assert_eq!(paths.len(), 38);
    let mut refused = std::collections::BTreeSet::new();
    for path in paths {
        let name = format!("hostile/{}", path.file_name().unwrap().to_str().unwrap());
        let out =
            c.run(&["check", s(&path), "--json", "--limit", "100000", "--maxBytes", "1000000"]);
        let value: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!("{name}: {:?} {:?}", out.status, String::from_utf8_lossy(&out.stderr))
        });
        validate(Tool::Check, &value);
        if common::UNOPENABLE.contains(&name.as_str()) {
            assert_eq!(out.status.code(), Some(2), "{name}");
            refused.insert(name.to_owned());
        } else {
            assert!(out.status.success(), "{name}: {value}");
        }
    }
    assert_eq!(refused, common::UNOPENABLE.iter().map(|s| s.to_string()).collect());
}
#[test]
fn agent_10_paging_cross_process_and_changed_file() {
    let c = Case::new();
    let body =
        (0..40).map(|i| format!("<w:p><w:r><w:t>ROW{i}</w:t></w:r></w:p>")).collect::<String>();
    fs::write(&c.input, common::docx_with_body(&body)).unwrap();
    let full = c.call(
        &["text", s(&c.input), "--json", "--limit", "100000", "--maxBytes", "1000000"],
        Tool::Text,
    );
    let mut cursor: Option<String> = None;
    let mut content = String::new();
    let mut pages = 0;
    loop {
        let mut args = vec!["text", s(&c.input), "--json", "--limit", "20", "--maxBytes", "24000"];
        if let Some(ref token) = cursor {
            args.extend(["--cursor", token]);
        }
        let v = c.call(&args, Tool::Text);
        content.push_str(v["content"].as_str().unwrap());
        pages += 1;
        cursor = v["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    assert!(pages > 1);
    assert_eq!(content, full["content"]);
    let page = c.call(&["text", s(&c.input), "--json", "--limit", "20"], Tool::Text);
    let token = page["nextCursor"].as_str().unwrap();
    let out = c.run(&["outline", s(&c.input), "--json", "--cursor", token]);
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["code"], "AGENT_BAD_CURSOR");
    fs::write(&c.input, &c.bytes).unwrap();
    let out = c.run(&["text", s(&c.input), "--json", "--cursor", token]);
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["code"], "AGENT_STALE_CURSOR");
}
#[test]
fn agent_10_write_failures_preserve_input_and_outputs() {
    let c = Case::new();
    fs::write(&c.output, b"existing").unwrap();
    let out = c.run(&["ops", s(&c.input), "--ops", s(&c.ops), "--output", s(&c.output), "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(&c.output).unwrap(), b"existing");
    let dir = c.dir.join("directory");
    fs::create_dir(&dir).unwrap();
    let out = c.run(&[
        "ops",
        s(&c.input),
        "--ops",
        s(&c.ops),
        "--output",
        s(&c.output),
        "--report",
        s(&dir),
        "--overwrite",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(&c.output).unwrap(), b"existing");
    let out = c.run(&[
        "ops",
        s(&c.input),
        "--ops",
        s(&c.ops),
        "--output",
        s(&c.output),
        "--overwrite",
        "--limit",
        "1",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(&c.output).unwrap(), b"existing");
    assert_eq!(fs::read(&c.input).unwrap(), c.bytes);
    let out = c.run(&["check", s(&c.dir.join("missing.docx")), "--json"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn agent_10_preview_report_continuation_and_commit_fingerprint() {
    let c = Case::new();
    let first = c.call(
        &[
            "preview",
            s(&c.input),
            "--ops",
            s(&c.ops),
            "--report",
            s(&c.report),
            "--limit",
            "1000",
            "--maxBytes",
            "8000",
            "--json",
        ],
        Tool::Preview,
    );
    let mut rows = first["content"].as_array().unwrap().clone();
    let mut cursor = first["nextCursor"].as_str().map(String::from);
    assert!(cursor.is_some());
    let original_report = fs::read(&c.report).unwrap();
    while let Some(token) = cursor {
        let p = c.call(
            &[
                "preview",
                s(&c.input),
                "--ops",
                s(&c.ops),
                "--report",
                s(&c.report),
                "--limit",
                "100000",
                "--maxBytes",
                "8000",
                "--json",
                "--cursor",
                &token,
            ],
            Tool::Preview,
        );
        rows.extend(p["content"].as_array().unwrap().clone());
        cursor = p["nextCursor"].as_str().map(String::from);
    }
    let stored: Value = serde_json::from_slice(&original_report).unwrap();
    assert_eq!(json!(rows), stored["report"]["rows"]);
    assert_eq!(fs::read(&c.report).unwrap(), original_report);
    c.call(
        &[
            "ops",
            s(&c.input),
            "--ops",
            s(&c.ops),
            "--preview",
            s(&c.report),
            "--output",
            s(&c.output),
            "--json",
        ],
        Tool::Edit,
    );
    fs::write(&c.input, common::docx_with_body("<w:p><w:r><w:t>changed</w:t></w:r></w:p>"))
        .unwrap();
    let out = c.run(&[
        "ops",
        s(&c.input),
        "--ops",
        s(&c.ops),
        "--preview",
        s(&c.report),
        "--output",
        s(&c.output),
        "--overwrite",
        "--json",
    ]);
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["code"],
        "AGENT_PREVIEW_STALE"
    );
}
#[test]
fn agent_10_native_debug_is_audited_and_atomic() {
    let c = Case::new();
    let mut p = rsword::package::Package::open(&c.bytes).unwrap();
    let d = rsword::model::Document::rebuild(&mut p).unwrap();
    let para = d.text_blocks().next().unwrap().node.0;
    let request =
        json!({"operations":[{"op":"insertText","at":{"para":para,"offset":0},"text":"NATIVE"}]});
    fs::write(&c.ops, request.to_string()).unwrap();
    let receipt = c.call(
        &[
            "ops",
            s(&c.input),
            "--ops",
            s(&c.ops),
            "--output",
            s(&c.output),
            "--native-ops",
            "--json",
        ],
        Tool::Edit,
    );
    assert_eq!(receipt["content"][0]["beforeVersion"], 0);
    assert_eq!(receipt["content"][0]["afterVersion"], 1);
    let report: Value =
        serde_json::from_slice(&fs::read(format!("{}.report.json", c.output.display())).unwrap())
            .unwrap();
    assert_eq!(report["nativeDebug"], true);
    assert_eq!(report["report"]["audit"]["operations"].as_array().unwrap().len(), 1);
    let audit: rsword_agent_query::audit::Audit =
        serde_json::from_value(report["report"]["audit"].clone()).unwrap();
    assert!(audit.restore(&Default::default()).is_ok());
    let mut bad = request;
    bad["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"op":"insertText","at":{"para":4294967295_u32,"offset":0},"text":"bad"}));
    fs::write(&c.ops, bad.to_string()).unwrap();
    let saved = fs::read(&c.output).unwrap();
    let out = c.run(&[
        "ops",
        s(&c.input),
        "--ops",
        s(&c.ops),
        "--output",
        s(&c.output),
        "--native-ops",
        "--overwrite",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(&c.output).unwrap(), saved);
    assert_eq!(fs::read(&c.input).unwrap(), c.bytes);
}
#[test]
fn agent_10_json_limits_fail_before_writing() {
    let c = Case::new();
    for bytes in [
        vec![b' '; 4 * 1024 * 1024 + 1],
        format!("{}0{}", "[".repeat(130), "]".repeat(130)).into_bytes(),
        br#"{"operations":[],"operations":[]}"#.to_vec(),
    ] {
        fs::write(&c.ops, bytes).unwrap();
        let out = c.run(&[
            "ops",
            s(&c.input),
            "--ops",
            s(&c.ops),
            "--output",
            s(&c.output),
            "--json",
            "--maxBytes",
            "512",
        ]);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.len() <= 512);
        assert!(!c.output.exists());
        validate(Tool::Edit, &serde_json::from_slice::<Value>(&out.stdout).unwrap());
    }
    let out = c.run(&["check", s(&c.dir.join("missing")), "--maxBytes", "1", "--json"]);
    assert_eq!(out.status.code(), Some(2), "预算校验必须先于读取文件");
}
#[test]
fn agent_10_media_export_and_attachment_binding() {
    let c = Case::new();
    let invalid = c.run(&["media", s(&c.input), "--output", s(&c.output), "--json"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(!c.output.exists());

    let image = common::repo_root().join("corpus/real/image/image-two-in-run.docx");
    let bytes = fs::read(&image).unwrap();
    let listed = c.call(&["media", s(&image), "--list", "--json"], Tool::Media);
    let id = listed["content"][0]["mediaId"].to_string();
    c.call(&["media", s(&image), "--id", &id, "--output", s(&c.output), "--json"], Tool::Media);
    let mut native = rsword::bind::native::SessionTable::default();
    let sid = native.open(&bytes, None).unwrap();
    assert_eq!(fs::read(&c.output).unwrap(), native.media(&sid, id.parse().unwrap()).unwrap());
    let source = common::repo_root().join("corpus/real/image/image-svg.docx");
    let media = common::part_bytes(&fs::read(source).unwrap(), "word/media/image1.png");
    let attachment = c.dir.join("replacement.png");
    fs::write(&attachment, &media).unwrap();
    let mut p = rsword::package::Package::open(&bytes).unwrap();
    let d = rsword::model::Document::rebuild(&mut p).unwrap();
    let projection =
        rsword::agent::text::project(&p, &d, rsword::agent::text::Scope::Main, "test").unwrap();
    let target = projection
        .objects
        .values()
        .filter(|o| o.object.kind == "image")
        .nth(1)
        .unwrap()
        .object
        .clone();
    fs::write(&c.ops,json!({"attachments":[{"name":"replacement","path":attachment,"mime":"image/png"}],"operations":[{"action":"replaceImage","target":target,"mediaId":{"attachment":"replacement"}}]}).to_string()).unwrap();
    let out = c.dir.join("image-edited.docx");
    let receipt =
        c.call(&["ops", s(&image), "--ops", s(&c.ops), "--output", s(&out), "--json"], Tool::Edit);
    assert_eq!(receipt["content"][0]["beforeVersion"], 1);
    assert_eq!(receipt["content"][0]["afterVersion"], 2);
    let report: Value =
        serde_json::from_slice(&fs::read(format!("{}.report.json", out.display())).unwrap())
            .unwrap();
    assert_eq!(
        report["report"]["audit"]["attachments"][0]["sha256"],
        rsword_agent_query::cursor::hash(&media)
    );
    fs::write(&attachment, vec![0; 16 * 1024 * 1024 + 1]).unwrap();
    let error = c.run(&["ops", s(&image), "--ops", s(&c.ops), "--output", s(&out), "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&error.stdout).unwrap()["code"],
        "AGENT_RESOURCE_LIMIT"
    );
}

#[test]
fn agent_10_stdout_failure_rolls_back_published_files() {
    let c = Case::new();
    fs::write(&c.output, b"old output").unwrap();
    fs::write(&c.report, b"old report").unwrap();
    let read_only = c.dir.join("read-only-stdout");
    fs::write(&read_only, b"untouched").unwrap();
    let outputs = vec![std::process::Stdio::from(fs::File::open(&read_only).unwrap())];
    #[cfg(unix)]
    let outputs = {
        let mut outputs = outputs;
        let (stdout, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        drop(peer);
        outputs.push(std::process::Stdio::from(std::os::fd::OwnedFd::from(stdout)));
        outputs
    };
    for stdout in outputs {
        let status = Command::new(env!("CARGO_BIN_EXE_rsword"))
            .args([
                "ops",
                s(&c.input),
                "--ops",
                s(&c.ops),
                "--output",
                s(&c.output),
                "--report",
                s(&c.report),
                "--overwrite",
                "--json",
            ])
            .env("TMPDIR", &c.dir)
            .stdout(stdout)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(1));
        assert_eq!(fs::read(&c.output).unwrap(), b"old output");
        assert_eq!(fs::read(&c.report).unwrap(), b"old report");
        assert_eq!(fs::read(&c.input).unwrap(), c.bytes);
        assert_eq!(fs::read(&read_only).unwrap(), b"untouched");
    }
}

#[test]
fn agent_10_large_error_and_ambiguous_candidates_are_bounded() {
    let c = Case::new();
    fs::write(&c.ops, json!({"z".repeat(200_000):true}).to_string()).unwrap();
    let out = c.run(&[
        "ops",
        s(&c.input),
        "--ops",
        s(&c.ops),
        "--output",
        s(&c.output),
        "--json",
        "--maxBytes",
        "512",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.len() <= 512);
    validate(Tool::Edit, &serde_json::from_slice(&out.stdout).unwrap());
    assert!(!c.output.exists());
    let mut e = rsword_agent_query::error("AGENT_AMBIGUOUS", "多个命中");
    e.details = json!({"total":800,"candidates":(0..8).map(|i|json!({"part":u32::MAX,"flow":u32::MAX,"range":{"start":u32::MAX-i,"end":u32::MAX}})).collect::<Vec<_>>()});
    let e = rsword_agent_query::tools::bounded_error(e, 512);
    assert_eq!(e.code, "AGENT_AMBIGUOUS");
    assert_eq!(e.details["total"], 800);
    assert!(!e.details["candidates"].as_array().unwrap().is_empty());
    assert!(serde_json::to_vec(&e).unwrap().len() <= 512);
}

#[test]
fn agent_04_find_page_counts_are_explicit_and_complete() {
    let c = Case::new();
    let input = common::repo_root().join("corpus/real/hf/hf-variants.docx");
    for (scope, pattern, expected, pages) in
        [("main", "页", 3, 2), ("all", "页", 15, 8), ("main", "zzNoSuchText", 0, 1)]
    {
        let mut cursor = None;
        let mut total = 0;
        let mut count = 0;
        loop {
            let mut args = vec![
                "find",
                s(&input),
                "--pattern",
                pattern,
                "--scope",
                scope,
                "--limit",
                "2000",
                "--json",
            ];
            if let Some(token) = cursor.as_deref() {
                args.extend(["--cursor", token]);
            }
            let v = c.call(&args, Tool::Find);
            let hits = v["content"].as_array().unwrap().len();
            assert_eq!(v["pageHits"], hits);
            assert_eq!(v["hasMore"], v["truncated"]);
            assert_eq!(v["hasMore"], v["nextCursor"].is_string());
            assert_eq!(v["usage"]["responseBytes"], v.to_string().len());
            total += hits;
            count += 1;
            cursor = v["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
            assert!(count <= pages, "游标必须推进");
        }
        assert_eq!((total, count), (expected, pages));
    }
    assert!(Tool::Find.description().starts_with("find：pageHits 只是本页命中数，不是总数"));
}

#[test]
fn agent_07_every_documented_request_runs_in_cli() {
    use std::collections::BTreeSet;
    let docs = include_str!("../../../docs/17-agent-edit.md").replace("\r\n", "\n");
    let names: Vec<_> = docs
        .lines()
        .filter_map(|line| {
            line.strip_prefix("<!-- agent-example ")
                .map(|tail| tail.split_whitespace().next().unwrap())
        })
        .collect();
    let expected: BTreeSet<_> =
        rsword_agent_query::edit::Action::COVERAGE.iter().map(|row| row.0).collect();
    assert_eq!(names.len(), expected.len(), "每个 action 恰有一份完整请求");
    assert_eq!(names.iter().copied().collect::<BTreeSet<_>>(), expected);
    for name in names {
        let out = Command::new("node")
            .current_dir(common::repo_root())
            .args(["tools/agent-edit-example.mjs", name, env!("CARGO_BIN_EXE_rsword")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        if name == "updateToc" {
            assert_eq!(v["code"], "AGENT_UNSUPPORTED_RANGE");
        } else {
            assert!(v["content"].is_array());
        }
    }
}

#[test]
fn agent_07_documented_find_anchor_scope_pipeline_runs() {
    let c = Case::new();
    // 文档同时含 shell 与 JS 路径；用仓库相对路径避开 Windows 反斜杠转义。
    let dir = c.dir.strip_prefix(common::repo_root()).unwrap().to_string_lossy().replace('\\', "/");
    let binary = env!("CARGO_BIN_EXE_rsword").replace('\\', "/").replace('\'', "'\\''");
    let docs = include_str!("../../../docs/17-agent-edit.md").replace("\r\n", "\n");
    let section = docs.split("## 7. 从 find 锚点").nth(1).unwrap();
    let script = section
        .split("```sh\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap()
        .replace("./target/debug/rsword", &format!("'{binary}'"))
        .replace("target/agent-find-scope", &dir);
    let out = Command::new("bash")
        .current_dir(common::repo_root())
        .args(["-eu", "-c", &script])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let request: Value =
        serde_json::from_slice(&fs::read(c.dir.join("request.json")).unwrap()).unwrap();
    assert_eq!(
        request["operations"][0]["selector"]["scope"],
        json!([{"part":2,"flow":0,"kind":"paragraph","node":27}])
    );
    let saved = fs::read(c.dir.join("commented.docx")).unwrap();
    let session = rsword::EditSession::open(&saved).unwrap();
    assert_eq!(session.document().comments.items.len(), 1);
}
