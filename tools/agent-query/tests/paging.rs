//! AGENT-06：独立拼接 oracle、预算信封、跨接口游标和版本原子性。
#[path = "../../../crates/rsword/tests/common/mod.rs"]
mod common;
use rsword::{
    agent::text::{Scope, project},
    package::Package,
};
use rsword_agent_query::{
    budget::{self, Budget},
    cursor::Registry,
    paging::{self, Unit},
    search::Worker,
    session::{ReadRequest, ReadTool, Sessions},
    transport::common_bytes,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn fixture() -> Vec<u8> {
    common::docx_with_body(&["<w:p><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr><w:r><w:t>标题😀</w:t></w:r></w:p>".repeat(8),"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>cell1</w:t></w:r></w:p><w:p><w:r><w:t>cell2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>OTHER</w:t></w:r></w:p></w:tc></w:tr></w:tbl>".into()].concat())
}
fn req(tool: ReadTool) -> ReadRequest {
    ReadRequest {
        tool,
        options: match tool {
            ReadTool::Document => json!({"blockRange":{"from":0,"to":4}}),
            ReadTool::Find => json!({"pattern":"标题","maxHits":1}),
            _ => json!({}),
        },
    }
}
fn worker() -> Worker {
    Worker::start(
        std::path::Path::new(env!("CARGO_BIN_EXE_rsword-query-worker")),
        &common::repo_root().join("target/m94-worker"),
    )
    .unwrap()
}
fn check(value: &Value, b: Budget) {
    assert!(budget::fits(value, b));
    assert_eq!(value["usage"]["responseBytes"], serde_json::to_vec(value).unwrap().len());
}
#[test]
fn agent_06_all_readers_share_cursor_and_reject_cross_use() {
    let bytes = fixture();
    let mut sessions = Sessions::default();
    let id = sessions.open(&bytes).unwrap();
    let anchor = sessions.anchor(&id, Scope::Main, 0).unwrap();
    let mut requests: Vec<_> =
        [ReadTool::Text, ReadTool::Outline, ReadTool::Find, ReadTool::Context, ReadTool::Document]
            .into_iter()
            .map(req)
            .collect();
    requests[3].options = json!({"anchor":anchor,"before":0,"after":8});
    for origin in &requests {
        let mut first = None;
        for limit in [12, 30, 100, 300, 600, 1000, 2000, 4000] {
            let b = Budget { limit, max_bytes: 10000 };
            if let Ok(value) = sessions.read(
                &id,
                origin,
                Some(b),
                None,
                (origin.tool == ReadTool::Find).then(worker),
            ) && value["truncated"] == true
            {
                check(&value, b);
                first = Some((value, b));
                break;
            }
        }
        let (page, b) = first.unwrap_or_else(|| panic!("{} 必须实际分页", origin.tool.name()));
        let cursor = page["nextCursor"].as_str().unwrap();
        assert!(cursor.starts_with('a'));
        for other in &requests {
            if origin.tool == other.tool {
                continue;
            }
            assert_eq!(
                sessions
                    .read(
                        &id,
                        other,
                        Some(b),
                        Some(cursor),
                        (other.tool == ReadTool::Find).then(worker)
                    )
                    .unwrap_err()
                    .code,
                "AGENT_BAD_CURSOR"
            );
        }
        let repeat = sessions
            .read(&id, origin, Some(b), Some(cursor), (origin.tool == ReadTool::Find).then(worker))
            .unwrap();
        assert_eq!(
            repeat,
            sessions
                .read(
                    &id,
                    origin,
                    Some(b),
                    Some(cursor),
                    (origin.tool == ReadTool::Find).then(worker)
                )
                .unwrap()
        );
        assert_eq!(
            sessions
                .read(
                    &id,
                    origin,
                    Some(Budget { limit: 1, max_bytes: 512 }),
                    Some(cursor),
                    (origin.tool == ReadTool::Find).then(worker)
                )
                .unwrap_err()
                .code,
            "AGENT_BUDGET_TOO_SMALL"
        );
        assert_eq!(
            repeat,
            sessions
                .read(
                    &id,
                    origin,
                    Some(b),
                    Some(cursor),
                    (origin.tool == ReadTool::Find).then(worker)
                )
                .unwrap()
        );
    }
    assert_eq!(sessions.save(&id, None).unwrap(), bytes);
}
#[test]
fn agent_06_version_changes_only_on_successful_writes() {
    let bytes = fixture();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let r = req(ReadTool::Text);
    let b = Budget { limit: 12, max_bytes: 10000 };
    let first = s.read(&id, &r, Some(b), None, None).unwrap();
    let cursor = first["nextCursor"].as_str().unwrap();
    let anchor = s.anchor(&id, Scope::Main, 0).unwrap();
    let before = s.read(&id, &r, Some(b), Some(cursor), None).unwrap();
    assert!(s.edit_native(&id, 0, &[r#"{"op":"notAnOp"}"#.into()], None).is_err());
    assert_eq!(s.snapshot(&id).unwrap()["version"], 0);
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    assert_eq!(before, s.read(&id, &r, Some(b), Some(cursor), None).unwrap());
    s.edit_native(&id, 0, &[], None).unwrap();
    assert_eq!(
        s.read(&id, &r, Some(b), Some(cursor), None).unwrap_err().code,
        "AGENT_STALE_CURSOR"
    );
    assert_eq!(s.validate_anchor(&id, &anchor).unwrap_err().code, "AGENT_STALE_ANCHOR");
    assert_eq!(s.edit_native(&id, 0, &[], None).unwrap_err().code, "AGENT_VERSION_CONFLICT");
    let page = s.read(&id, &r, Some(b), None, None).unwrap();
    s.add_media(&id, 1, &test_media(), "image/png").unwrap();
    assert_eq!(
        s.read(&id, &r, Some(b), page["nextCursor"].as_str(), None).unwrap_err().code,
        "AGENT_STALE_CURSOR"
    );
    s.close(&id);
    assert_eq!(s.read(&id, &r, None, Some(cursor), None).unwrap_err().code, "BIND_NO_SESSION");
}
#[test]
fn agent_06_text_pages_equal_full_projection_and_counts() {
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103);
    let mut opened = 0;
    let mut refused = std::collections::BTreeSet::new();
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let name = path
            .strip_prefix(common::repo_root().join("corpus"))
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let mut pkg = match Package::open(&bytes) {
            Ok(p) => {
                assert!(!common::UNOPENABLE.contains(&name.as_str()));
                p
            }
            Err(_) => {
                assert!(common::UNOPENABLE.contains(&name.as_str()));
                refused.insert(name);
                continue;
            }
        };
        let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
        let p = project(&pkg, &doc, Scope::All, "corpus:1").unwrap();
        let units = paging::text_units(&p, 0..p.anchors.len()).unwrap();
        assert_eq!(
            units.iter().map(|u| u.content.as_str().unwrap()).collect::<String>(),
            p.content,
            "{name}: 单位切分失真"
        );
        let mut totals = BTreeMap::<String, u64>::new();
        for unit in &units {
            for o in &unit.omitted {
                *totals.entry(o["category"].as_str().unwrap().into()).or_default() +=
                    o["count"].as_u64().unwrap();
            }
        }
        let expected: BTreeMap<_, _> = p.omitted["page"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| (o["category"].as_str().unwrap().to_owned(), o["count"].as_u64().unwrap()))
            .collect();
        assert_eq!(totals, expected, "{name}: 省略归属遗漏/重复");
        let counts = units.iter().fold([0; 4], |mut acc, u| {
            for (i, n) in u.counts.iter().enumerate() {
                acc[i] += n;
            }
            acc
        });
        assert_eq!(
            counts,
            [
                p.anchor_counts.source_utf16,
                p.anchor_counts.presentation_utf16,
                p.anchor_counts.source_scalars,
                p.anchor_counts.presentation_scalars
            ],
            "{name}: 锚点覆盖缺失"
        );
        // 对每份文档的前几个完整单位走真实预算路径；完整单位拼接另对全篇断言。
        let sample = &units[..units.len().min(3)];
        let mut registry = Registry::default();
        let mut cursor = None;
        let mut joined = String::new();
        loop {
            let start = joined.encode_utf16().count();
            let unit =
                sample.iter().find(|u| u.range.start as usize >= start).unwrap_or(&sample[0]);
            let single = paging::response(
                "corpus:1",
                std::slice::from_ref(unit),
                true,
                json!("sample"),
                true,
                Some(
                    Registry::default()
                        .candidate("corpus:1", "text", "sample", &json!({}))
                        .as_str(),
                ),
            );
            let b = Budget {
                limit: single["usage"]["contentUtf16"].as_u64().unwrap().max(1) as usize,
                max_bytes: common_bytes(&single, false).max(512),
            };
            if b.limit > 1048576 || b.max_bytes > 4194304 {
                assert_eq!(
                    paging::page(
                        &mut registry,
                        "corpus:1",
                        "text",
                        "sample",
                        sample,
                        true,
                        json!("sample"),
                        Budget { limit: 1048576, max_bytes: 4194304 },
                        cursor.as_deref(),
                        1
                    )
                    .unwrap_err()
                    .code,
                    "AGENT_UNIT_TOO_LARGE"
                );
                break;
            }
            let page = paging::page(
                &mut registry,
                "corpus:1",
                "text",
                "sample",
                sample,
                true,
                json!("sample"),
                b,
                cursor.as_deref(),
                1,
            )
            .unwrap();
            check(&page, b);
            joined.push_str(page["content"].as_str().unwrap());
            let next = page["nextCursor"].as_str().map(str::to_owned);
            assert!(next.is_none() || next != cursor);
            cursor = next;
            if cursor.is_none() {
                assert_eq!(
                    joined,
                    sample.iter().map(|u| u.content.as_str().unwrap()).collect::<String>(),
                    "{name}: 续页拼接失真"
                );
                break;
            }
        }
        opened += 1;
    }
    assert_eq!(opened, 1099);
    assert_eq!(refused, common::UNOPENABLE.into_iter().map(str::to_owned).collect());
}
#[test]
fn agent_06_preflight_and_indivisible_unit_errors() {
    let mut s = Sessions::default();
    assert_eq!(
        s.read(
            "no session",
            &req(ReadTool::Text),
            Some(Budget { limit: 1, max_bytes: 511 }),
            None,
            None
        )
        .unwrap_err()
        .code,
        "BIND_BAD_ARGUMENT"
    );
    let units = vec![Unit::record(json!({"text":"x".repeat(1048577)}), 0)];
    let e = paging::page(
        &mut Registry::default(),
        "v",
        "text",
        "cfg",
        &units,
        false,
        json!(null),
        Budget { limit: 1048576, max_bytes: 4194304 },
        None,
        1,
    )
    .unwrap_err();
    assert_eq!(e.code, "AGENT_UNIT_TOO_LARGE");
    assert!(serde_json::to_vec(&e).unwrap().len() <= 512);
}

#[test]
fn agent_06_file_reopen_fingerprint_and_fresh_anchors() {
    let dir = common::repo_root().join("target/m94-file-cursor");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    let bytes = fixture();
    std::fs::write(&path, &bytes).unwrap();
    let b = Budget { limit: 30, max_bytes: 16000 };
    let request = req(ReadTool::Text);
    let first = Sessions::read_file(&path, &request, Some(b), None, None).unwrap();
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = Sessions::read_file(&path, &request, Some(b), Some(cursor), None).unwrap();
    assert_ne!(first["snapshot"]["sessionId"], second["snapshot"]["sessionId"]);
    assert_eq!(second["anchors"]["snapshot"], second["snapshot"]);
    let repeat = Sessions::read_file(&path, &request, Some(b), Some(cursor), None).unwrap();
    assert_eq!(repeat["content"], second["content"]);
    assert_eq!(repeat["nextCursor"], second["nextCursor"]);
    let mut joined = first["content"].as_str().unwrap().to_owned();
    let mut next = Some(cursor.to_owned());
    while let Some(token) = next {
        let page = Sessions::read_file(&path, &request, Some(b), Some(&token), None).unwrap();
        check(&page, b);
        joined.push_str(page["content"].as_str().unwrap());
        next = page["nextCursor"].as_str().map(str::to_owned);
        assert_ne!(next.as_deref(), Some(token.as_str()));
    }
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    assert_eq!(joined, project(&pkg, &doc, Scope::Main, "oracle").unwrap().content);
    let other = dir.join("other.docx");
    std::fs::write(&other, &bytes).unwrap();
    assert_eq!(
        Sessions::read_file(&other, &request, Some(b), Some(cursor), None).unwrap_err().code,
        "AGENT_BAD_CURSOR"
    );
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    std::fs::write(&path, changed).unwrap();
    assert_eq!(
        Sessions::read_file(&path, &request, Some(b), Some(cursor), None).unwrap_err().code,
        "AGENT_STALE_CURSOR"
    );
    std::fs::write(&path, bytes).unwrap();
}

#[test]
fn agent_06_model_selection_reuses_native_and_hides_outside_indices() {
    let bytes = common::docx_with_body(concat!(
        "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"INSIDE\"/><w:r><w:t>VISIBLE</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>",
        "<w:p><w:bookmarkStart w:id=\"2\" w:name=\"OUTSIDE_SENTINEL\"/><w:r><w:t>SECRET</w:t></w:r><w:bookmarkEnd w:id=\"2\"/></w:p>"
    ));
    let mut native = rsword::bind::native::SessionTable::default();
    let native_id = native.open(&bytes, None).unwrap();
    let whole: Value = serde_json::from_str(&native.document(&native_id, None).unwrap()).unwrap();
    assert!(whole.get("nextCursor").is_none());
    let options = json!({"blockRange":{"from":0,"to":1},"fields":["main"],"depth":0});
    let selected: Value =
        serde_json::from_str(&native.document(&native_id, Some(&options.to_string())).unwrap())
            .unwrap();
    assert!(
        selected.to_string().contains("OUTSIDE_SENTINEL"),
        "原生确实保留全局索引，不能做成空验证"
    );
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let page =
        s.read(&id, &ReadRequest { tool: ReadTool::Document, options }, None, None, None).unwrap();
    let text = page.to_string();
    assert!(!text.contains("OUTSIDE_SENTINEL"));
    assert!(!text.contains("SECRET"));
    assert!(text.contains("INSIDE"));
    let main: Vec<_> = page["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["field"] == "main")
        .map(|r| r["value"].clone())
        .collect();
    assert_eq!(json!(main), selected["main"]);
    assert_eq!(
        whole,
        serde_json::from_str::<Value>(&native.document(&native_id, None).unwrap()).unwrap()
    );
    assert_eq!(s.save(&id, None).unwrap(), bytes);
}

#[test]
fn agent_06_every_read_budget_includes_metadata_and_errors() {
    let huge = json!({"code":"UNKNOWN_CODE","message":"😀\"\\\n".repeat(300),"location":{"part":2,"node":42}});
    for tool in ReadTool::ALL.iter().map(|t| t.name()).chain(["summary", "preview", "diff"]) {
        let units = vec![Unit::record(huge.clone(), 0), Unit::record(json!({"text":"end"}), 1)];
        let mut registry = Registry::default();
        let full = paging::page(
            &mut registry,
            "v",
            tool,
            "c",
            &units,
            false,
            json!(null),
            Budget { limit: 8000, max_bytes: 24000 },
            None,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(full["content"], json!([huge,{"text":"end"}]));
        let exact = Budget {
            limit: full["usage"]["contentUtf16"].as_u64().unwrap() as usize,
            max_bytes: common_bytes(&full, false),
        };
        check(&full, exact);
        assert_eq!(
            full,
            paging::page(
                &mut registry,
                "v",
                tool,
                "c",
                &units,
                false,
                json!(null),
                exact,
                None,
                usize::MAX
            )
            .unwrap()
        );
        let err = paging::page(
            &mut registry,
            "v",
            tool,
            "c",
            &units,
            false,
            json!(null),
            Budget { limit: 1, max_bytes: 512 },
            None,
            usize::MAX,
        )
        .unwrap_err();
        assert_eq!(err.code, "AGENT_BUDGET_TOO_SMALL");
        let serialized = serde_json::to_value(&err).unwrap();
        assert!(serialized.get("content").is_none());
        assert!(serde_json::to_vec(&err).unwrap().len() <= 512);
        assert!(err.details["minLimit"].as_u64().unwrap() > 1);
    }
}

#[test]
fn agent_06_failed_batch_and_duplicate_media_version_boundary() {
    let bytes = fixture();
    let engine = rsword::EditSession::open(&bytes).unwrap();
    let para = engine.document().paragraphs().next().unwrap().node.0;
    let op = json!({"op":"insertText","at":{"para":para,"offset":0},"text":"EDITED "}).to_string();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let original = s.snapshot(&id).unwrap();
    assert!(s.edit_native(&id, 0, &[op.clone(), r#"{"op":"notAnOp"}"#.into()], None).is_err());
    assert_eq!(s.snapshot(&id).unwrap(), original);
    assert_eq!(s.save(&id, None).unwrap(), bytes);
    assert!(s.save(&id, Some(r#"{"removePersonalInfo":"bad"}"#)).is_err());
    assert_eq!(s.snapshot(&id).unwrap(), original);
    s.edit_native(&id, 0, &[op], None).unwrap();
    assert_ne!(s.save(&id, None).unwrap(), bytes);
    assert_eq!(s.snapshot(&id).unwrap()["version"], 1);
    let first = s.add_media(&id, 1, &test_media(), "image/png").unwrap();
    let second = s.add_media(&id, 2, &test_media(), "image/png").unwrap();
    assert_eq!(first["mediaId"], second["mediaId"]);
    assert_eq!(s.snapshot(&id).unwrap()["version"], 3);
    assert!(s.add_media(&id, 3, &test_media(), "not/mime\n").is_err());
    assert_eq!(s.snapshot(&id).unwrap()["version"], 3);
}

#[test]
fn agent_06_file_all_five_tools_share_codec() {
    let dir = common::repo_root().join("target/m94-file-all");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    std::fs::write(&path, fixture()).unwrap();
    for tool in
        [ReadTool::Text, ReadTool::Outline, ReadTool::Find, ReadTool::Context, ReadTool::Document]
    {
        let mut request = req(tool);
        if tool == ReadTool::Context {
            request.options = json!({"anchorOffset":0,"before":0,"after":8});
        }
        let mut paged = None;
        for limit in [12, 30, 100, 300, 600, 1000, 2000, 4000] {
            let b = Budget { limit, max_bytes: 16000 };
            if let Ok(first) = Sessions::read_file(
                &path,
                &request,
                Some(b),
                None,
                (tool == ReadTool::Find).then(worker),
            ) && first["truncated"] == true
            {
                paged = Some((first, b));
                break;
            }
        }
        let (first, b) = paged.unwrap_or_else(|| panic!("{} 必须真实分页", tool.name()));
        let second = Sessions::read_file(
            &path,
            &request,
            Some(b),
            first["nextCursor"].as_str(),
            (tool == ReadTool::Find).then(worker),
        )
        .unwrap();
        assert_ne!(first["snapshot"], second["snapshot"]);
        assert_ne!(first["nextCursor"], second["nextCursor"]);
        check(&second, b);
        let other =
            if tool == ReadTool::Text { req(ReadTool::Outline) } else { req(ReadTool::Text) };
        assert_eq!(
            Sessions::read_file(&path, &other, Some(b), first["nextCursor"].as_str(), None)
                .unwrap_err()
                .code,
            "AGENT_BAD_CURSOR"
        );
    }
}

#[test]
fn agent_06_auxiliary_model_flow_keeps_its_own_indices() {
    let bytes = common::docx_with_parts(
        "<w:p><w:r><w:t>BODY_SENTINEL</w:t></w:r></w:p>",
        &[
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
            ),
            (
                "word/header1.xml",
                r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:bookmarkStart w:id="1" w:name="HF_INSIDE"/><w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple><w:bookmarkEnd w:id="1"/></w:p><w:p><w:r><w:t>HF_OUTSIDE</w:t></w:r></w:p></w:hdr>"#,
            ),
        ],
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let projection = project(&pkg, &doc, Scope::All, "oracle").unwrap();
    let flow = projection.flows.iter().find(|f| f.object.part != doc.main_part.0).unwrap();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let request = ReadRequest {
        tool: ReadTool::Document,
        options: json!({"flow":{"part":flow.object.part,"flow":flow.object.flow},"blockRange":{"from":0,"to":1}}),
    };
    let page = s.read(&id, &request, None, None, None).unwrap();
    let wire = page.to_string();
    assert!(wire.contains("HF_INSIDE"));
    assert!(!wire.contains("BODY_SENTINEL"));
    assert!(!wire.contains("HF_OUTSIDE"));
    let rows = page["content"].as_array().unwrap();
    let fields: Vec<_> = rows.iter().filter(|r| r["field"] == "fields").collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0]["value"]["part"], flow.object.part);
    let spans: Vec<_> = rows.iter().filter(|r| r["field"] == "spans").collect();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0]["value"]["part"], flow.object.part);
}

#[test]
fn agent_06_file_two_processes() {
    if let Some(path) = std::env::var_os("RSWORD_M94_CHILD_INPUT") {
        let cursor = std::env::var("RSWORD_M94_CHILD_CURSOR").ok();
        let page = Sessions::read_file(
            std::path::Path::new(&path),
            &req(ReadTool::Text),
            Some(Budget { limit: 30, max_bytes: 16000 }),
            cursor.as_deref(),
            None,
        )
        .unwrap();
        std::fs::write(
            std::env::var_os("RSWORD_M94_CHILD_OUTPUT").unwrap(),
            serde_json::to_vec(&page).unwrap(),
        )
        .unwrap();
        return;
    }
    let dir = common::repo_root().join("target/m94-process-cursor");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    std::fs::write(&path, fixture()).unwrap();
    let mut pages: Vec<Value> = vec![];
    for index in 0..2 {
        let output = dir.join(format!("{index}.json"));
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "agent_06_file_two_processes"])
            .env("RSWORD_M94_CHILD_INPUT", &path)
            .env("RSWORD_M94_CHILD_OUTPUT", &output);
        if let Some(page) = pages.last() {
            command.env("RSWORD_M94_CHILD_CURSOR", page["nextCursor"].as_str().unwrap());
        }
        let result = command.output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
        pages.push(serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap());
    }
    assert_ne!(pages[0]["snapshot"]["sessionId"], pages[1]["snapshot"]["sessionId"]);
    assert_ne!(
        pages[0]["anchors"]["segments"][0]["range"],
        pages[1]["anchors"]["segments"][0]["range"]
    );
    assert_ne!(pages[0]["nextCursor"], pages[1]["nextCursor"]);
}

#[test]
fn agent_06_continuation_concatenates_independent_text_oracle() {
    let bytes = common::docx_with_body(
        "<w:p><w:r><w:t>FIRST</w:t></w:r></w:p><w:p><w:r><w:t>MIDDLE😀</w:t></w:r></w:p><w:p><w:r><w:t>LAST</w:t></w:r></w:p>",
    );
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    let p = project(&pkg, &doc, Scope::Main, "v").unwrap();
    let units = paging::text_units(&p, 0..p.anchors.len()).unwrap();
    assert_eq!(units.len(), 3);
    let mut registry = Registry::default();
    let mut cursor = None;
    let mut joined = String::new();
    let mut pages = 0;
    loop {
        let page = paging::page(
            &mut registry,
            "v",
            "text",
            "c",
            &units,
            true,
            json!(null),
            Budget { limit: 1000, max_bytes: 16000 },
            cursor.as_deref(),
            1,
        )
        .unwrap();
        joined.push_str(page["content"].as_str().unwrap());
        pages += 1;
        assert!(pages <= 3, "游标未前进");
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(joined, p.content, "续读拼接与一次性规范文本不同");
    assert_eq!(pages, 3);
}

#[test]
fn agent_06_final_page_without_cursor_can_fit_after_rejected_prefix() {
    let dir = common::repo_root().join("target/m94-final-page");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    std::fs::write(
        &path,
        common::docx_with_body(
            "<w:p><w:r><w:t>A</w:t></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>",
        ),
    )
    .unwrap();
    let r = req(ReadTool::Text);
    let full = Sessions::read_file(&path, &r, None, None, None).unwrap();
    assert_eq!(full["content"], "A\nB\n");
    let bytes = common_bytes(&full, false) + 32;
    let prefix =
        Sessions::read_file(&path, &r, Some(Budget { limit: 2, max_bytes: bytes }), None, None)
            .unwrap_err();
    assert_eq!(prefix.code, "AGENT_BUDGET_TOO_SMALL");
    assert!(prefix.details["minBytes"].as_u64().unwrap() > bytes as u64);
    let final_page =
        Sessions::read_file(&path, &r, Some(Budget { limit: 4, max_bytes: bytes }), None, None)
            .unwrap();
    assert_eq!(final_page["content"], full["content"]);
    assert_eq!(final_page["truncated"], false);
    assert!(final_page["nextCursor"].is_null());
}

#[test]
fn agent_06_snapshot_wire_preserves_legacy_strings() {
    let bytes = fixture();
    let mut pkg = Package::open(&bytes).unwrap();
    let doc = rsword::model::Document::rebuild(&mut pkg).unwrap();
    for snapshot in [
        "legacy",
        r#""quoted""#,
        r#"{ "version": 1, "sessionId": "s" }"#,
        r#"{"projectionVersion":"agent/1-unicode17","sessionId":"s","version":1}"#,
    ] {
        let p = project(&pkg, &doc, Scope::Main, snapshot).unwrap();
        let anchor = p.anchors.to_anchor(0, None).unwrap();
        let wire = serde_json::to_value(&anchor).unwrap();
        let restored: rsword::agent::anchors::Anchor =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(restored, anchor, "snapshot 不可归一或剥去引号");
        if snapshot.starts_with("{\"projectionVersion") {
            assert!(wire["snapshot"].is_object());
        } else {
            assert!(wire["snapshot"].is_string());
        }
    }
}

#[test]
fn agent_06_model_block_field_reference_closure() {
    let bytes =
        std::fs::read(common::repo_root().join("corpus/real/fields/fields-toc.docx")).unwrap();
    let mut s = Sessions::default();
    let id = s.open(&bytes).unwrap();
    let page = s
        .read(
            &id,
            &ReadRequest {
                tool: ReadTool::Document,
                options: json!({"blockRange":{"from":2,"to":3}}),
            },
            None,
            None,
            None,
        )
        .unwrap();
    let fields: Vec<_> = page["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["field"] == "fields")
        .map(|v| v["value"].clone())
        .collect();
    assert_eq!(fields.len(), 4, "TOC 块须保留 TOC 与三个 PAGEREF 的引用闭包");
    let keywords: Vec<_> = fields.iter().map(|f| f["instr"]["keyword"].as_str().unwrap()).collect();
    assert_eq!(keywords, ["PAGEREF", "PAGEREF", "PAGEREF", "TOC"]);
    assert!(fields.iter().all(|f| f["id"].as_u64().unwrap() < 4), "范围外 REF/DATE 不得混入");
}

#[test]
fn agent_06_model_content_fields_cannot_bypass_explicit_scope() {
    let mut s = Sessions::default();
    let id = s.open(&fixture()).unwrap();
    for options in [
        json!({"fields":["main"]}),
        json!({"fields":["comments"]}),
        json!({"blockRange":{"from":0,"to":1},"fields":["main","hfParts"]}),
        json!({"scope":"all","fields":["styles"]}),
    ] {
        assert_eq!(
            s.read(&id, &ReadRequest { tool: ReadTool::Document, options }, None, None, None)
                .unwrap_err()
                .code,
            "BIND_BAD_ARGUMENT"
        );
    }
    let declaration = s
        .read(
            &id,
            &ReadRequest {
                tool: ReadTool::Document,
                options: json!({"fields":["styles","settings"]}),
            },
            None,
            None,
            None,
        )
        .unwrap();
    assert!(!declaration.to_string().contains("标题"));
    let scoped = s.read(&id, &req(ReadTool::Document), None, None, None).unwrap();
    assert!(scoped.to_string().contains("标题"));
}
fn test_media() -> Vec<u8> {
    common::part_bytes(
        &std::fs::read(common::repo_root().join("corpus/real/image/image-svg.docx")).unwrap(),
        "word/media/image1.png",
    )
}
#[test]
fn agent_06_cli_cursor_rejected_by_mcp() {
    let dir = common::repo_root().join("target/m97-cursor-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    let bytes = fixture();
    std::fs::write(&path, &bytes).unwrap();
    let r = req(ReadTool::Text);
    let b = Budget { limit: 12, max_bytes: 24000 };
    let file = Sessions::read_file(&path, &r, Some(b), None, None).unwrap();
    let token = file["nextCursor"].as_str().expect("必须实际产生文件游标");
    let mut sessions = Sessions::default();
    let id = sessions.open(&bytes).unwrap();
    let e = sessions.read(&id, &r, Some(b), Some(token), None).unwrap_err();
    assert_eq!(e.code, "AGENT_BAD_CURSOR");
    assert_eq!(e.message, "MCP 需要会话句柄，不能接收 CLI 文件游标");
}
#[test]
fn agent_06_mcp_cursor_rejected_by_cli() {
    let dir = common::repo_root().join("target/m97-cursor-mcp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.docx");
    let bytes = fixture();
    std::fs::write(&path, &bytes).unwrap();
    let r = req(ReadTool::Text);
    let b = Budget { limit: 12, max_bytes: 24000 };
    let mut sessions = Sessions::default();
    let id = sessions.open(&bytes).unwrap();
    let page = sessions.read(&id, &r, Some(b), None, None).unwrap();
    let token = page["nextCursor"].as_str().expect("必须实际产生会话游标");
    let e = Sessions::read_file(&path, &r, Some(b), Some(token), None).unwrap_err();
    assert_eq!(e.code, "AGENT_BAD_CURSOR");
    assert_eq!(e.message, "CLI 需要自包含文件游标，不能接收 MCP 会话句柄");
}
