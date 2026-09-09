//! AGENT-03/04/05：独立模型/文本 oracle、预算与范围边界。
#[path = "../../../crates/rsword/tests/common/mod.rs"]
mod common;
use rsword::{
    agent::text::{Projection, Scope, project},
    model::{Block, Document, block::TextKind},
    package::Package,
};
use rsword_agent_query::{
    budget::{self, Budget},
    find::Finder,
    nav::{self, Pages, Selection, Unit},
    search::{self, Mode, Options, Position, Request},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
fn fixture(body: &str) -> (Package, Document, Projection) {
    let mut pkg = Package::open(&common::docx_with_body(body)).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    let p = project(&pkg, &doc, Scope::All, "query:1").unwrap();
    (pkg, doc, p)
}
fn request(text: &str, pattern: &str) -> Request {
    Request {
        pattern: pattern.into(),
        options: Options::default(),
        flows: vec![search::InputFlow { part: 0, flow: 0, start: 0, text: text.into() }],
        max_hits: 1000,
        position: Position::default(),
    }
}
fn all(mut r: Request) -> Vec<search::Hit> {
    let mut out = vec![];
    loop {
        let b = search::run(&r).unwrap();
        out.extend(b.hits);
        if let Some(next) = b.next {
            assert_ne!(next, r.position);
            r.position = next;
        } else {
            return out;
        }
    }
}
fn worker() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_rsword-query-worker"))
}
fn scratch() -> std::path::PathBuf {
    common::repo_root().join("target/agent-query-test")
}
fn selection(p: &Projection) -> Selection {
    Selection { flow: p.flows[0].object.clone(), blocks: 0..p.flows[0].blocks.len() }
}
#[test]
fn agent_03_real_outline_and_budget_measurement() {
    assert_eq!(Budget::OUTLINE.limit, 4000, "不能调大规范缺省预算绕过分页");
    assert_eq!(Budget::OUTLINE.max_bytes, 16000);
    let paths = common::docx_paths("real");
    assert_eq!(paths.len(), 266);
    let largest = paths.iter().max_by_key(|p| std::fs::metadata(p).unwrap().len()).unwrap();
    assert!(largest.ends_with("misc/large-report.docx"));
    for path in paths.iter() {
        let mut pkg = Package::open(&std::fs::read(path).unwrap()).unwrap();
        let doc = Document::rebuild(&mut pkg).unwrap();
        let p = project(&pkg, &doc, Scope::Main, "outline:1").unwrap();
        let rows = nav::outline(&p, &doc, 1..10).unwrap();
        assert!(!rows.is_empty() || doc.main.is_empty(), "{}", path.display());
        let independent: Vec<_> = doc
            .main
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => match t.kind {
                    TextKind::Heading { level } => Some((level, t.text())),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let actual: Vec<_> = rows
            .iter()
            .filter(|r| r["kind"] == "heading")
            .map(|r| (r["level"].as_u64().unwrap() as u8, r["title"].as_str().unwrap().to_owned()))
            .collect();
        assert_eq!(actual, independent);
        let mut pages = Pages::default();
        let mut cursor = None;
        let mut joined = vec![];
        let mut page_count = 0;
        loop {
            let result = pages
                .page(
                    "outline:1",
                    "outline/main/1-9",
                    &rows,
                    json!("main"),
                    Budget::OUTLINE,
                    cursor.as_deref(),
                    usize::MAX,
                )
                .unwrap();
            assert_eq!(
                result["usage"]["responseBytes"],
                serde_json::to_vec(&result).unwrap().len()
            );
            assert!(budget::fits(&result, Budget::OUTLINE));
            joined.extend(result["content"].as_array().unwrap().clone());
            page_count += 1;
            cursor = result["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(joined, rows);
        if path == largest {
            assert_eq!(page_count, 2);
            assert_eq!(rows.len(), 26);
            assert_eq!(rows[2]["blockRange"], json!({"start":41,"end":48}));
            let full = budget::envelope("outline:1", json!(rows), json!("main"), false, None);
            assert!(full["usage"]["responseBytes"].as_u64().unwrap() <= 16000);
            assert!(full["usage"]["contentUtf16"].as_u64().unwrap() > 4000);
            eprintln!("AGENT outline largest: {} pages, usage={}", page_count, full["usage"]);
        }
    }
}
#[test]
fn agent_03_empty_groups_skipped_levels_and_duplicate_titles() {
    let (_, d, p) = fixture("");
    assert!(nav::outline(&p, &d, 1..10).unwrap().is_empty());
    let (_, d, p) = fixture("<w:p><w:r><w:t>首句。后句</w:t></w:r></w:p>");
    let rows = nav::outline(&p, &d, 1..10).unwrap();
    assert_eq!(rows[0]["kind"], "group");
    assert_eq!(rows[0]["title"], "首句。");
    assert!(rows[0].get("level").is_none());
    let body = [0, 2, 2, 0]
        .iter()
        .map(|n| {
            format!(
                "<w:p><w:pPr><w:outlineLvl w:val=\"{n}\"/></w:pPr><w:r><w:t>同名</w:t></w:r></w:p>"
            )
        })
        .collect::<String>();
    let (_, d, p) = fixture(&body);
    let rows = nav::outline(&p, &d, 1..10).unwrap();
    assert_eq!(rows[1]["level"], 3);
    let key =
        serde_json::from_value::<rsword::agent::anchors::ObjectRef>(rows[0]["object"].clone())
            .unwrap()
            .key();
    assert_eq!(rows[1]["parent"], key);
    assert_eq!(rows[2]["parent"], key);
    assert_eq!(rows[3]["parent"], Value::Null);
    assert_eq!(rows[0]["blockCount"], 3);
}
#[test]
fn agent_06_record_budget_and_cursor_integrity() {
    let rows = vec![
        json!({"object":"a","text":"中😀\\\"".repeat(30)}),
        json!({"object":"b","text":"second"}),
        json!({"object":"c","text":"third"}),
    ];
    let mut pages = Pages::default();
    let budget = Budget { limit: 1000, max_bytes: 2000 };
    let first = pages.page("v1", "cfg", &rows, json!(null), budget, None, 1).unwrap();
    let key = first["nextCursor"].as_str().unwrap();
    assert_eq!(
        pages.page("v2", "cfg", &rows, json!(null), budget, Some(key), 1).unwrap_err().code,
        "AGENT_STALE_CURSOR"
    );
    assert_eq!(
        pages.page("v1", "wrong", &rows, json!(null), budget, Some(key), 1).unwrap_err().code,
        "AGENT_BAD_CURSOR"
    );
    assert_eq!(
        pages
            .page(
                "v1",
                "cfg",
                &rows,
                json!(null),
                Budget { limit: 1, max_bytes: 512 },
                Some(key),
                1
            )
            .unwrap_err()
            .code,
        "AGENT_BUDGET_TOO_SMALL"
    );
    let retry = pages.page("v1", "cfg", &rows, json!(null), budget, Some(key), 1).unwrap();
    assert_eq!(retry, pages.page("v1", "cfg", &rows, json!(null), budget, Some(key), 1).unwrap());
    let exact = Budget {
        limit: first["usage"]["contentUtf16"].as_u64().unwrap() as usize,
        max_bytes: rsword_agent_query::transport::common_bytes(&first, false),
    };
    assert!(budget::fits(&first, exact));
    assert!(!budget::fits(&first, Budget { max_bytes: exact.max_bytes - 1, ..exact }));
}
#[test]
fn agent_04_normalization_source_intervals_and_unsupported_regex() {
    let mut r = request("Ｈｅｌｌｏ  \t世界 ｶﾞ e\u{301} ﬁ", "hello 世界 ガ é ﬁ");
    r.options.fold_width = true;
    r.options.collapse_whitespace = true;
    r.options.insensitive = true;
    let hits = all(r);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].original, "Ｈｅｌｌｏ  \t世界 ｶﾞ e\u{301} ﬁ");
    assert!(hits[0].complete);
    let mut r = request("ﬁ", "fi");
    r.options.fold_width = true;
    assert!(all(r).is_empty(), "不能把 width fold 扩成完整 NFKC");
    let mut r = request("q\u{301}", "q");
    r.options.fold_width = true;
    let partial = all(r);
    assert_eq!(partial[0].original, "q\u{301}");
    assert_eq!(partial[0].range, 0..2);
    assert!(!partial[0].complete, "不可分归一簇内部的端点不能伪报可编辑");
    for pattern in [r"(a)\1", r"(?=a)"] {
        let mut r = request("a", pattern);
        r.options.mode = Mode::Regex;
        assert_eq!(search::run(&r).unwrap_err().code, "AGENT_BAD_PATTERN");
    }
    let mut r = request("foo", "[Ａ]");
    r.options.fold_width = true;
    r.options.mode = Mode::Regex;
    assert!(all(r).is_empty());
}
#[test]
fn agent_04_zero_length_and_pagination_keep_full_context() {
    for pattern in [r"(?m)^", r"\b", r"", r"a*", r"a\n.*?b"] {
        let text = "a\nb aa 😀";
        let mut r = request(text, pattern);
        r.options.mode = Mode::Regex;
        r.max_hits = 1;
        let hits = all(r);
        let re = regex::Regex::new(pattern).unwrap();
        let expected: Vec<_> = re
            .find_iter(text)
            .map(|m| {
                let a = text[..m.start()].encode_utf16().count() as u32;
                let b = text[..m.end()].encode_utf16().count() as u32;
                (a..b, m.as_str().to_owned())
            })
            .collect();
        assert_eq!(
            hits.iter().map(|h| (h.range.clone(), h.original.clone())).collect::<Vec<_>>(),
            expected,
            "{pattern}"
        );
    }
    let mut r = request("x", "xy");
    r.flows.push(search::InputFlow { part: 1, flow: 0, start: 1, text: "y".into() });
    assert!(all(r).is_empty(), "不得跨流匹配");
}
#[test]
fn agent_04_worker_limits_timeout_and_recovery() {
    let mut r = request("abc", "a");
    assert_eq!(
        search::Worker::start(worker(), &scratch()).unwrap().submit(&r).unwrap().hits.len(),
        1
    );
    r.pattern = "a".repeat(4097);
    assert_eq!(
        search::Worker::start(worker(), &scratch()).unwrap().submit(&r).unwrap_err().code,
        "AGENT_QUERY_TOO_LARGE"
    );
    r = request(&"a".repeat(1048577), "a");
    r.options.deadline_ms = 2000;
    assert_eq!(
        search::Worker::start(worker(), &scratch()).unwrap().submit(&r).unwrap_err().code,
        "AGENT_QUERY_TOO_LARGE"
    );
    r = request("a", r"\w{100000}");
    r.options.mode = Mode::Regex;
    assert_eq!(
        search::Worker::start(worker(), &scratch()).unwrap().submit(&r).unwrap_err().code,
        "AGENT_BAD_PATTERN"
    );
    r = request(&"a".repeat(1000000), r"(a+)+$");
    r.options.mode = Mode::Regex;
    r.options.deadline_ms = 1;
    assert_eq!(
        search::Worker::start(worker(), &scratch()).unwrap().submit(&r).unwrap_err().code,
        "AGENT_QUERY_TIMEOUT"
    );
    assert_eq!(
        search::Worker::start(worker(), &scratch())
            .unwrap()
            .submit(&request("ok", "ok"))
            .unwrap()
            .hits
            .len(),
        1
    );
}
#[test]
fn agent_04_find_preconditions_budget_and_authorized_context() {
    let (_, _, p) = fixture(
        "<w:p><w:r><w:t>SECRET</w:t></w:r></w:p><w:p><w:r><w:t>ＡＡ　ＢＢ ＡＡ　ＢＢ ＡＡ　ＢＢ</w:t></w:r></w:p><w:p><w:r><w:t>FORBIDDEN</w:t></w:r></w:p>",
    );
    let mut s = selection(&p);
    s.blocks = 1..2;
    let mut finder = Finder::default();
    let opts = Options { fold_width: true, collapse_whitespace: true, ..Default::default() };
    let mut cursor = None;
    let mut rows = vec![];
    loop {
        let result = finder
            .find(
                &p,
                std::slice::from_ref(&s),
                "AA BB",
                opts.clone(),
                1,
                Budget::FIND,
                cursor.as_deref(),
                search::Worker::start(worker(), &scratch()).unwrap(),
            )
            .unwrap();
        let raw = result.to_string();
        assert!(!raw.contains("SECRET") && !raw.contains("FORBIDDEN"));
        for row in result["content"].as_array().unwrap() {
            assert_eq!(row["match"], "ＡＡ　ＢＢ");
            assert_eq!(row["editable"], true);
            rsword_agent_query::find::verify_precondition(&p, row).unwrap();
            let mut bad = row.clone();
            bad["precondition"]["original"] = json!("AA BB");
            assert_eq!(
                rsword_agent_query::find::verify_precondition(&p, &bad).unwrap_err().code,
                "AGENT_PRECONDITION_FAILED"
            );
            rows.push(row.clone());
        }
        cursor = result["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(rows.len(), 3);
    let a =
        p.anchors.to_anchor(rows[0]["textRange"]["start"].as_u64().unwrap() as u32, None).unwrap();
    let c = nav::context(&p, &a, &s, 100, 100, Unit::Utf16, false).unwrap();
    assert_eq!(c["text"], "ＡＡ　ＢＢ ＡＡ　ＢＢ ＡＡ　ＢＢ\n");
    assert!(!c.to_string().contains("SECRET"));
}
#[test]
fn agent_04_corpus_literal_and_regex_independent_oracles() {
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103);
    let mut refused = BTreeSet::new();
    let mut count = 0;
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
        let doc = Document::rebuild(&mut pkg).unwrap();
        let p = project(&pkg, &doc, Scope::All, "corpus:1").unwrap();
        for f in &p.flows {
            let text = p.text_range(f.range.clone()).unwrap();
            if text.len() > 1048576 {
                let mut r = request(text, "a");
                r.options.deadline_ms = 2000;
                assert_eq!(search::run(&r).unwrap_err().code, "AGENT_QUERY_TOO_LARGE");
                continue;
            }
            let pattern = text
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_string())
                .unwrap_or("x".into());
            for mode in [Mode::Literal, Mode::Regex] {
                let mut r = request(text, &pattern);
                r.options.mode = mode;
                r.max_hits = 17;
                let hits = all(r);
                let expected: Vec<_> = text
                    .match_indices(pattern.as_str())
                    .map(|(i, m)| {
                        (
                            text[..i].encode_utf16().count() as u32
                                ..text[..i + m.len()].encode_utf16().count() as u32,
                            m.to_owned(),
                        )
                    })
                    .collect();
                assert_eq!(
                    hits.iter().map(|h| (h.range.clone(), h.original.clone())).collect::<Vec<_>>(),
                    expected,
                    "{name}"
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, 1099);
    assert_eq!(refused, common::UNOPENABLE.into_iter().map(str::to_owned).collect());
}
#[test]
fn agent_05_cell_window_and_surrogate_expansion() {
    let (_, _, p) = fixture(
        "<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>中😀文</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>OTHER CELL</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    let off = p.content[..p.content.find('😀').unwrap()].encode_utf16().count() as u32;
    let a = p.anchors.to_anchor(off, None).unwrap();
    let c = nav::context(&p, &a, &selection(&p), 0, 1, Unit::Utf16, true).unwrap();
    assert_eq!(c["text"], "中😀文\n");
    assert_eq!(c["parent"]["kind"], "cell");
    assert!(!c.to_string().contains("OTHER CELL"));
    let end = c["actualRange"]["end"].as_u64().unwrap() as u32;
    let left = p.anchors.to_anchor(end, Some(rsword::agent::anchors::Affinity::Left)).unwrap();
    for unit in [Unit::Blocks, Unit::Utf16] {
        let tail = nav::context(&p, &left, &selection(&p), 0, 0, unit, true).unwrap();
        assert_eq!(tail["text"], c["text"]);
        assert_eq!(tail["parent"], c["parent"]);
        assert!(!tail.to_string().contains("OTHER CELL"));
    }
}
#[cfg(unix)]
#[test]
fn agent_04_timeout_kills_started_worker_and_reaps_pid() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch().join("timeout-proof");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("busy-worker.sh");
    std::fs::write(&script,b"#!/bin/sh\nprintf ready > \"$3\"\nwhile [ ! -f \"$1\" ]; do :; done\nprintf running > \"$2.started\"\nwhile :; do :; done\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let w = search::Worker::start(&script, &dir).unwrap();
    let pid = w.id();
    let mut r = request("a", "a");
    r.options.deadline_ms = 50;
    assert_eq!(w.submit(&r).unwrap_err().code, "AGENT_QUERY_TIMEOUT");
    assert!(
        !std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "超时 worker 仍存活"
    );
    let started: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "started"))
        .collect();
    assert_eq!(started.len(), 1);
    assert_eq!(std::fs::read(&started[0]).unwrap(), b"running");
    std::fs::remove_file(&started[0]).unwrap();
}
#[test]
fn agent_04_zero_length_middle_and_end_affinities() {
    let (_, _, p) =
        fixture("<w:p><w:r><w:t>body</w:t></w:r></w:p><w:p><w:r><w:t>next</w:t></w:r></w:p>");
    for (blocks, pattern, offset, affinity) in [
        (0..2, r"\b", 0, "right"),
        (0..2, r"\b", 4, "right"),
        (0..2, "$", 10, "left"),
        (0..1, "$", 5, "left"),
    ] {
        let s = Selection { flow: p.flows[0].object.clone(), blocks };
        let range = s.range(&p).unwrap();
        let response = Finder::default()
            .find(
                &p,
                &[s],
                pattern,
                Options { mode: Mode::Regex, ..Default::default() },
                20,
                Budget::FIND,
                None,
                search::Worker::start(worker(), &scratch()).unwrap(),
            )
            .unwrap();
        let row = response["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["textRange"]["start"] == offset)
            .unwrap();
        assert_eq!(row["anchors"]["start"], row["anchors"]["end"]);
        assert_eq!(row["anchors"]["start"]["affinity"], affinity);
        let a = serde_json::from_value(row["anchors"]["start"].clone()).unwrap();
        let back = p.anchors.to_text_offset(&a).unwrap();
        assert_eq!(back, offset);
        assert!(back >= range.start && back <= range.end);
    }
}

#[test]
fn agent_04_empty_and_adjacent_flow_end_ownership() {
    for body in ["", "<w:p><w:r><w:t>body</w:t></w:r></w:p>"] {
        let body = format!(
            "{body}<w:sectPr><w:headerReference xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" w:type=\"default\" r:id=\"rIdHeader\"/></w:sectPr>"
        );
        let bytes = common::docx_with_parts(
            &body,
            &[
                (
                    "word/_rels/document.xml.rels",
                    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
                ),
                (
                    "word/header1.xml",
                    r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>HEADER</w:t></w:r></w:p></w:hdr>"#,
                ),
            ],
        );
        let mut pkg = Package::open(&bytes).unwrap();
        let doc = Document::rebuild(&mut pkg).unwrap();
        let p = project(&pkg, &doc, Scope::All, "flow:1").unwrap();
        assert_eq!(p.flows.len(), 2);
        for f in &p.flows {
            let s = Selection { flow: f.object.clone(), blocks: 0..f.blocks.len() };
            let response = Finder::default()
                .find(
                    &p,
                    std::slice::from_ref(&s),
                    "$",
                    Options { mode: Mode::Regex, ..Default::default() },
                    20,
                    Budget::FIND,
                    None,
                    search::Worker::start(worker(), &scratch()).unwrap(),
                )
                .unwrap();
            let row = &response["content"][0];
            assert_eq!(row["anchors"]["start"], row["anchors"]["end"]);
            let a: rsword::agent::anchors::Anchor =
                serde_json::from_value(row["anchors"]["start"].clone()).unwrap();
            assert_eq!(a.affinity, rsword::agent::anchors::Affinity::Left);
            assert_eq!(p.anchors.to_text_offset(&a).unwrap(), f.range.end);
            let (part, flow) = match &a.target {
                rsword::agent::anchors::Target::Source { part, flow, .. } => (*part, *flow),
                rsword::agent::anchors::Target::Presentation { owner, .. } => {
                    (owner.part, owner.flow)
                }
            };
            assert_eq!((part, flow), (f.object.part, f.object.flow));
            let c = nav::context(&p, &a, &s, 0, 0, Unit::Blocks, true).unwrap();
            assert_eq!(c["flow"], json!(f.object));
        }
        let f = &p.flows[1];
        let a = p
            .anchors
            .to_flow_anchor(
                f.range.start,
                rsword::agent::anchors::Affinity::Right,
                f.object.part,
                f.object.flow,
            )
            .unwrap();
        let c = nav::context(
            &p,
            &a,
            &Selection { flow: f.object.clone(), blocks: 0..f.blocks.len() },
            0,
            0,
            Unit::Blocks,
            true,
        )
        .unwrap();
        assert!(c["text"].as_str().unwrap().contains("HEADER"));
        assert!(!c["text"].as_str().unwrap().contains("body"));
    }
}

#[test]
fn agent_05_field_detail_is_read_only_and_scope_bounded() {
    use rsword_agent_query::detail::{Details, context_page};
    let (mut pkg, doc, p) = fixture(
        r#"<w:p><w:fldSimple w:instr="PAGE"><w:r><w:t>42</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:t>FORBIDDEN_OUTSIDE</w:t></w:r></w:p>"#,
    );
    let before = pkg.save().unwrap();
    let media = rsword::package::media::MediaStore::new();
    let details = Details::build(&pkg, &doc, &p, &media);
    let field = p.objects.values().find(|o| o.object.kind == "field").unwrap();
    assert_eq!(
        details.get(field.object.part, field.object.node).unwrap()["fieldResult"]["text"],
        "42"
    );
    let a = p.anchors.to_anchor(field.range.start, None).unwrap();
    let s = Selection { flow: p.flows[0].object.clone(), blocks: 0..1 };
    let out =
        context_page(&p, &a, &s, 0, 0, Unit::Blocks, Some(&details), Budget::CONTEXT).unwrap();
    assert!(!out.to_string().contains("FORBIDDEN_OUTSIDE"));
    assert!(out.to_string().contains("fieldResult"));
    assert_eq!(
        context_page(
            &p,
            &a,
            &s,
            0,
            0,
            Unit::Blocks,
            Some(&details),
            Budget { limit: 1, ..Budget::CONTEXT }
        )
        .unwrap_err()
        .code,
        "AGENT_BUDGET_TOO_SMALL"
    );
    assert_eq!(pkg.save().unwrap(), before);
}

#[test]
fn agent_05_real_table_chart_and_media_details_match_model() {
    use rsword::{
        bind::native::json::{ProjCx, ToJson},
        package::media::MediaStore,
    };
    use rsword_agent_query::detail::Details;
    for name in ["misc/large-report.docx", "chart/chart-column.docx", "image/image-two-in-run.docx"]
    {
        let bytes = std::fs::read(common::repo_root().join("corpus/real").join(name)).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let doc = Document::rebuild(&mut pkg).unwrap();
        let p = project(&pkg, &doc, Scope::All, "detail:1").unwrap();
        let mut media = MediaStore::new();
        for part in pkg.parts() {
            if pkg.content_types().image_mime(&part.uri).is_some() {
                media.intern_part(&pkg, part.id).unwrap();
            }
        }
        let index = Details::build(&pkg, &doc, &p, &media);
        let cx = ProjCx { pkg: &pkg, display: true };
        if name.contains("large-report") {
            let Block::Table(t) = &doc.main[43] else { panic!("具名第一表") };
            let detail = index.get(doc.main_part.0, t.node.0).unwrap();
            assert_eq!(detail["model"], doc.main[43].to_json(&cx));
            let resolver = rsword::resolve::Resolver::new(&doc);
            let view = resolver.table(pkg.part(doc.main_part).dom().unwrap(), t);
            let cols = view.columns();
            assert_eq!(detail["geometry"]["widthsTwips"], json!(cols.widths_twips));
            assert_eq!(detail["geometry"]["columnCount"], 3);
            assert_eq!(detail["geometry"]["rows"].as_array().unwrap().len(), 3);
        } else if name.contains("chart") {
            let charts: Vec<_> = p.objects.values().filter(|o| o.object.kind == "chart").collect();
            assert_eq!(charts.len(), 1);
            let detail = index.get(charts[0].object.part, charts[0].object.node).unwrap();
            assert_eq!(doc.chart_parts.len(), 1);
            assert_eq!(detail["chart"], doc.chart_parts.values().next().unwrap().to_json(&cx));
        } else {
            let pictures: Vec<_> =
                p.objects.values().filter(|o| o.object.kind == "image").collect();
            assert_eq!(pictures.len(), 2);
            assert_eq!(media.len(), 1);
            for picture in pictures {
                let detail = index.get(picture.object.part, picture.object.node).unwrap();
                assert_eq!(detail["sections"], json!([0]));
                let reference = &detail["media"][0];
                assert_eq!(reference["external"], false);
                let id =
                    rsword::package::media::MediaId(reference["mediaId"].as_u64().unwrap() as u32);
                assert_eq!(reference["partId"], media.get(id).part.0);
                assert_eq!(reference["uri"], media.get(id).uri.as_str());
            }
        }
        assert_eq!(pkg.save().unwrap(), bytes);
    }
}

#[cfg(unix)]
#[test]
fn agent_04_ready_write_cannot_overwrite_request() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch().join("ready-write-race");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("worker.sh");
    // 保持 ready 的打开描述符，直到父进程提交请求才写入，稳定制造握手写入交错。
    std::fs::write(
        &script,
        br#"#!/bin/sh
exec 3>"$3"
while [ ! -f "$1" ]; do :; done
printf ready >&3
IFS= read -r packet < "$1"
case "$packet" in
  \{*) printf '{"Ok":{"hits":[],"next":null}}' > "$2" ;;
  *) exit 17 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result =
        search::Worker::start(&script, &dir).unwrap().submit(&request("body", "absent")).unwrap();
    assert!(result.hits.is_empty());
    assert!(result.next.is_none());
}
