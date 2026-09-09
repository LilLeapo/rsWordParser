//! AGENT-10：独立测试进程测真实在用堆字节，避免 RSS 的 allocator 缓存冒充泄漏。
use rsword_agent_query::{edit::WorkerConfig, tools::Tool, transport::Shape};
use rsword_mcp::{Config, Service};
use serde_json::{Value, json};
use stats_alloc::{INSTRUMENTED_SYSTEM, StatsAlloc};
use std::{
    alloc::System,
    path::PathBuf,
    time::{Duration, Instant},
};
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;
fn live() -> i128 {
    let s = GLOBAL.stats();
    s.bytes_allocated as i128 - s.bytes_deallocated as i128
}
fn open(s: &mut Service, path: &PathBuf) -> String {
    let r = s.call(Tool::Open.mcp(), json!({"options":{"path":path}}));
    assert_eq!(r.value["isError"], false, "{}", r.value);
    r.value["structuredContent"]["content"][0]["sessionId"].as_str().unwrap().to_owned()
}
fn close(s: &mut Service, id: &str) {
    let r = s.call(Tool::Close.mcp(), json!({"sessionId":id,"options":{}}));
    assert_eq!(r.value["isError"], false);
}
#[test]
fn agent_10_close_and_idle_expiry_release_real_heap_and_reports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let input = root.join("corpus/real/misc/large-report.docx");
    let mut s = Service::new(Config {
        shape: Shape::Structured,
        max_sessions: 32,
        idle_timeout: Duration::from_secs(1800),
        worker: WorkerConfig {
            program: PathBuf::from(env!("CARGO_BIN_EXE_rsword-mcp")),
            scratch: root.join("target/m97-memory-worker"),
            args: vec!["__query-worker".into()],
        },
    })
    .unwrap();
    let id = open(&mut s, &input);
    close(&mut s, &id);
    drop(id);
    let baseline = live();
    let mut measurements = vec![];
    for cycle in 0..5 {
        let id = open(&mut s, &input);
        // 同时建游标与报告，不能只验证裸文档 Drop 而漏会话附属缓存。
        let r = s.call("text", json!({"sessionId":id,"options":{},"limit":4000}));
        assert_eq!(r.value["isError"], false, "{}", r.value);
        assert!(r.value["structuredContent"]["nextCursor"].is_string());
        drop(r);
        let r=s.call("edit",json!({"sessionId":id,"expectedVersion":0,"options":{"nativeDebug":true,"operations":[]}}));
        assert_eq!(r.value["isError"], false, "{}", r.value);
        drop(r);
        let opened = live();
        if cycle % 2 == 0 {
            close(&mut s, &id);
        } else {
            s.expire(Instant::now() + Duration::from_secs(1801));
        }
        drop(id);
        let closed = live();
        assert!(opened - baseline > 100_000, "必须实际保有文档，而不是空会话测试");
        assert!(
            closed - baseline < 16 * 1024,
            "close 后仍在用 {} B，疑似会话/报告/游标泄漏",
            closed - baseline
        );
        assert!(opened - closed > (opened - baseline) * 9 / 10, "内存未回落至少 90%");
        assert_eq!(s.session_count(), 0);
        measurements.push((opened - baseline, closed - baseline));
    }
    println!("AGENT-10 heap live deltas (open, close), bytes: {measurements:?}");
    let gone = s.call("text", json!({"sessionId":"missing","options":{}}));
    assert_eq!(gone.value["structuredContent"]["code"], Value::from("BIND_NO_SESSION"));
}
