//! BIND-01/05 / TEST-10：最大三份真实文档的协议耗时与 JSON 体积。
use rsword::EditSession;
use rsword::bind::native::SessionTable;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn measure<S, O>(
    name: &str,
    mut setup: impl FnMut() -> S,
    mut run: impl FnMut(&mut S) -> O,
) -> f64 {
    let mut samples = Vec::new();
    for i in 0..32 {
        let mut state = setup();
        let start = Instant::now();
        let result = black_box(run(&mut state));
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        drop(result);
        if i > 0 {
            samples.push(elapsed);
        }
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{name}: median={:.6} ms p95={:.6} ms max={:.6} ms",
        samples[15], samples[29], samples[30]
    );
    samples[29]
}
fn opened(bytes: &[u8]) -> (SessionTable, String) {
    let mut table = SessionTable::default();
    let id = table.open(bytes, None).unwrap();
    (table, id)
}
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut pending = vec![root.join("corpus/real")];
    let mut paths = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "edited") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|x| x == "docx") {
                paths.push((path.metadata().unwrap().len(), path));
            }
        }
    }
    assert_eq!(paths.len(), 266);
    paths.sort();
    for (size, path) in paths.into_iter().rev().take(3) {
        let bytes = std::fs::read(&path).unwrap();
        let engine = EditSession::open(&bytes).unwrap();
        let para = engine.document().paragraphs().next().unwrap().node.0;
        let op =
            serde_json::json!({"op":"insertText", "at":{"para":para,"offset":0}, "text":"bench"})
                .to_string();
        let (mut table, id) = opened(&bytes);
        let json = table.document(&id, None).unwrap();
        let model: serde_json::Value = serde_json::from_str(&json).unwrap();
        println!(
            "{}: {size} B, native JSON {} B; 31 samples after warmup",
            path.strip_prefix(&root).unwrap().display(),
            json.len()
        );
        #[cfg(feature = "compat-ts")]
        {
            let mut pkg = rsword::package::Package::open(&bytes).unwrap();
            let compat = rsword::bind::compat_ts::parsed_doc(&mut pkg).unwrap().to_string().len();
            println!(
                "JSON: compat {compat} B -> native {} B ({:.1}% reduction)",
                json.len(),
                100.0 * (1.0 - json.len() as f64 / compat as f64)
            );
        }
        measure("open", SessionTable::default, |t| t.open(&bytes, None).unwrap());
        measure("document", || opened(&bytes), |(t, id)| t.document(id, None).unwrap());
        let apply = measure("apply", || opened(&bytes), |(t, id)| t.apply(id, &op, None).unwrap());
        let save = measure(
            "save edited",
            || {
                let (mut t, id) = opened(&bytes);
                t.apply(&id, &op, None).unwrap();
                (t, id)
            },
            |(t, id)| t.save(id, None).unwrap(),
        );
        println!(
            "save: {:.6} ms / MB of input ZIP (decimal MB)",
            save / (size as f64 / 1_000_000.0)
        );
        assert!(apply < 5.0, "apply p95 >= 5 ms");
        assert!(save / (size as f64 / 1_000_000.0) < 50.0, "save p95 >= 50 ms/MB");
        if let Some(media) = model["media"].as_array().unwrap().first() {
            let media = media["mediaId"].as_u64().unwrap() as u32;
            println!("media payload: {} B", table.media(&id, media).unwrap().len());
            measure("media", || opened(&bytes), |(t, id)| t.media(id, media).unwrap());
        } else {
            // 文档本身没有媒体时不伪造源文档的媒体读取成绩；显式标明测新增句柄。
            println!("media: source has none; measuring an added 1024-byte image handle");
            measure(
                "media added",
                || {
                    let (mut t, id) = opened(&bytes);
                    let media = t.add_media(&id, &[0; 1024], "image/png").unwrap();
                    (t, id, media)
                },
                |(t, id, media)| t.media(id, *media).unwrap(),
            );
        }
    }
}
