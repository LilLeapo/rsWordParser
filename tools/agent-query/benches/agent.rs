//! AGENT-03/04/06 / TEST-10：最大三份真实件，已打开会话的默认首屏。
//! setup/open 不计时；find 包含工作进程启动；响应序列化计时，销毁不计时。
use rsword_agent_query::{
    search::Worker,
    session::{ReadRequest, ReadTool, Sessions},
    transport::Shape,
};
use serde_json::json;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let worker = std::env::var_os("RSWORD_BENCH_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/release/rsword-query-worker"));
    assert!(worker.is_file(), "先 cargo build -p rsword-agent-query --release --bins");
    let mut pending = vec![root.join("corpus/real")];
    let mut paths = vec![];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "edited") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|n| n == "docx") {
                paths.push((path.metadata().unwrap().len(), path));
            }
        }
    }
    assert_eq!(paths.len(), 266, "真实语料集合漂移");
    paths.sort();
    for (zip_bytes, path) in paths.into_iter().rev().take(3) {
        let bytes = std::fs::read(&path).unwrap();
        let mut probe = Sessions::default();
        let id = probe.open(&bytes).unwrap();
        let projection = probe.projection(&id).unwrap();
        // 实际投影中的可搜索字符；输出查询词，避免空命中基准冒充定位成本。
        let pattern = projection.content.chars().find(|c| c.is_alphanumeric()).unwrap().to_string();
        for tool in [ReadTool::Text, ReadTool::Outline, ReadTool::Find] {
            let request = ReadRequest {
                tool,
                options: if tool == ReadTool::Find {
                    json!({"scope":"all", "pattern":pattern})
                } else {
                    json!({})
                },
            };
            let mut samples = vec![];
            let mut sizes = None;
            let mut sessions = Sessions::default();
            let id = sessions.open(&bytes).unwrap();
            for i in 0..32 {
                let start = Instant::now();
                let worker = (tool == ReadTool::Find).then(|| {
                    Worker::start(&worker, &root.join("target/agent-bench-worker")).unwrap()
                });
                let value = sessions.read(&id, &request, None, None, worker).unwrap();
                let encoded = black_box(serde_json::to_vec(&value).unwrap());
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(value["usage"]["responseBytes"], encoded.len());
                assert!(!value["content"].is_null());
                if tool == ReadTool::Find {
                    assert!(!value["content"].as_array().unwrap().is_empty());
                }
                let row = json!({"jsonBytes":encoded.len(),
                    "mcpTextBytes":Shape::Text.result(&value,false).to_string().len(),
                    "mcpStructuredBytes":Shape::Structured.result(&value,false).to_string().len(),
                    "contentUtf16":value["usage"]["contentUtf16"],"truncated":value["truncated"]});
                if let Some(previous) = &sizes {
                    assert_eq!(&row, previous, "首屏体积/分页必须确定");
                }
                sizes = Some(row);
                if i != 0 {
                    samples.push(ms);
                }
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{}",
                json!({"path":path.strip_prefix(&root).unwrap(),
                "zipBytes":zip_bytes,"tool":tool.name(),"request":request.options,
                "limit":tool.budget().limit,"maxBytes":tool.budget().max_bytes,
                "samples":samples.len(),"medianMs":samples[15],"p95Ms":samples[29],
                "maxMs":samples[30],"firstPage":sizes})
            );
        }
    }
}
