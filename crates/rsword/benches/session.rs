//! BIND-01：选择最大真实文档，单独计量会话克隆；释放不计入 clone。
use rsword::edit::EditSession;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

// 同形采样统一：预热一次，准备状态与释放不计入计时；apply 内部的事务克隆计入。
macro_rules! measure {
    ($label:literal, $setup:expr, $run:expr) => {{
        let mut samples = Vec::new();
        for sample in 0..32 {
            let mut state = $setup;
            let start = Instant::now();
            let output = black_box(($run)(&mut state));
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            drop(output);
            if sample != 0 {
                samples.push(ms);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{}: samples={}, median={:.3} ms, p95={:.3} ms, max={:.3} ms",
            $label,
            samples.len(),
            samples[15],
            samples[29],
            samples[30]
        );
        samples[29]
    }};
}

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
    println!(
        "BIND-01 corpus: {} real documents, largest {} ({} bytes)",
        files.len(),
        path.strip_prefix(&root).unwrap().display(),
        size
    );
    let clone_p95 = measure!("BIND-01 clone", (), |_: &mut ()| session.clone());
    assert!(clone_p95 < 50.0, "BIND-01 clone p95 must be below 50 ms");
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
    let query_p95 = measure!("BIND-06 resolveRuns (1000 ids)", (), |_: &mut ()| table
        .resolve_runs(&id, &ids, None)
        .unwrap());
    assert!(query_p95 < 50.0, "BIND-06 p95 must be below 50 ms");
    let para = session.document().paragraphs().next().unwrap().node;
    let op = rsword::edit::EditOp::InsertText {
        at: rsword::edit::InlinePos::new(para, 0),
        text: "bench".into(),
        props: None,
    };
    let json = rsword::bind::native::edit_op_to_json(&op, session.dom()).unwrap();
    let native_p95 = measure!(
        "EDIT-05 apply with complete rollback snapshot",
        session.clone(),
        |s: &mut EditSession| s.apply(op.clone(), &rsword::EditContext::default()).unwrap()
    );
    let protocol_p95 = measure!(
        "BIND-01 protocol apply",
        {
            let mut t = rsword::bind::native::SessionTable::default();
            let id = t.open(&bytes, None).unwrap();
            (t, id)
        },
        |s: &mut (rsword::bind::native::SessionTable, String)| s
            .0
            .apply(&s.1, &json, None)
            .unwrap()
    );
    assert!(native_p95 < 5.0 && protocol_p95 < 5.0, "apply p95 must be below 5 ms");
}
