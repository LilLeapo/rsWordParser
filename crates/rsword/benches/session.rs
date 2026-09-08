//! BIND-01：选择最大真实文档，单独计量会话克隆；释放不计入 clone。
use rsword::edit::EditSession;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut pending = vec![root.join("corpus/real")];
    let mut files = vec![];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "edited") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "docx") {
                files.push((path.metadata().unwrap().len(), path));
            }
        }
    }
    files.sort();
    let (size, path) = files.last().expect("real corpus");
    let bytes = std::fs::read(path).unwrap();
    let session = EditSession::open(&bytes).unwrap();
    drop(black_box(session.clone()));
    let mut times = vec![];
    for _ in 0..31 {
        let start = Instant::now();
        let cloned = black_box(session.clone());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        drop(cloned);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "BIND-01 clone: {} real documents, largest {} ({} bytes); samples={}, median={:.3} ms, p95={:.3} ms, max={:.3} ms",
        files.len(),
        path.strip_prefix(&root).unwrap().display(),
        size,
        times.len(),
        times[15],
        times[29],
        times[30]
    );
    let runs: Vec<u32> = session
        .document()
        .paragraphs()
        .flat_map(|p| p.inlines.iter())
        .filter_map(|i| if let rsword::model::Inline::Run(r) = i { Some(r.node.0) } else { None })
        .collect();
    assert!(!runs.is_empty());
    let ids = serde_json::to_string(&(0..1000).map(|i| runs[i % runs.len()]).collect::<Vec<_>>())
        .unwrap();
    let mut table = rsword::bind::native::SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    black_box(table.resolve_runs(&id, &ids, None).unwrap());
    let mut queries = vec![];
    for _ in 0..31 {
        let start = Instant::now();
        let output = black_box(table.resolve_runs(&id, &ids, None).unwrap());
        queries.push(start.elapsed().as_secs_f64() * 1000.0);
        drop(output);
    }
    queries.sort_by(f64::total_cmp);
    println!(
        "BIND-06 resolveRuns: 1000 ids, samples={}, median={:.3} ms, p95={:.3} ms, max={:.3} ms",
        queries.len(),
        queries[15],
        queries[29],
        queries[30]
    );
    assert!(queries[29] < 50.0, "BIND-06 p95 must be below 50 ms");
}
