//! 性能记录（`spec/18` 7.9，**非门**）：语料里最大的三份文档上量 `open` / `InsertText` /
//! `AcceptAll` / `save_with` 的耗时。
//!
//! 不引第三方 bench 框架（`harness = false` + `std::time::Instant`）：这里要的是**量级**，
//! 不是统计学。建议观察值 `apply` < 5 ms、`save_with` < 50 ms / MB；数字记进 `docs/05`。
//!
//! 跑法：`cargo bench -p rsword --bench edit`（发布档），或 `cargo run --release --bin …` 之外
//! 直接 `cargo bench`。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::save::SaveOptions;

/// 语料里最大的三份（`find corpus -name '*.docx' | xargs stat -f %z` 排序），外加**带修订的
/// 那份最大的**——不然 `AcceptAll` 那一列没东西可做，恒为 0。
const DOCS: [&str; 4] = [
    "corpus/real/misc/large-report.docx",
    "corpus/real/ole/ole-ppt.docx",
    "corpus/real/chart/chartex-sunburst-2.docx",
    "corpus/real/_round2/_resaved/revisions-comments--comment-resaved-by-word.docx",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

/// 跑 `n` 次取中位数——单次读数受 GC / 页错误影响太大。
fn median(mut f: impl FnMut() -> Duration, n: usize) -> Duration {
    let mut xs: Vec<Duration> = (0..n).map(|_| f()).collect();
    xs.sort();
    xs[xs.len() / 2]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    println!(
        "{:<34} {:>8} {:>9} {:>10} {:>11} {:>10} {:>11}",
        "文档", "KB", "open", "InsertText", "AcceptAll", "save_with", "js::parse"
    );
    for rel in DOCS {
        let path = repo_root().join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            println!("{rel:<34} （读不到，跳过）");
            continue;
        };
        let kb = bytes.len() as f64 / 1024.0;

        let open = median(
            || {
                let t = Instant::now();
                let s = EditSession::open(&bytes).expect("open");
                std::hint::black_box(&s);
                t.elapsed()
            },
            5,
        );

        // 一次内联插入：最常见的编辑，量的是"定位 + 提交 + 增量刷新"这条热路径
        let insert = median(
            || {
                let mut s = EditSession::open(&bytes).expect("open");
                let first = s.document().paragraphs().map(|b| b.node).next();
                let Some(para) = first else { return Duration::ZERO };
                let t = Instant::now();
                let _ = s.apply(
                    EditOp::InsertText {
                        at: InlinePos::new(para, 0),
                        text: "x".into(),
                        props: None,
                    },
                    &EditContext::default(),
                );
                t.elapsed()
            },
            5,
        );

        // 接受全部修订：整份文档的修订解决 + 整体重建
        let accept = median(
            || {
                let mut s = EditSession::open(&bytes).expect("open");
                let t = Instant::now();
                let _ = s.apply(EditOp::AcceptAll { author: None }, &EditContext::default());
                t.elapsed()
            },
            5,
        );

        // 保存：先做一次编辑，逼它走完整序列化（没编辑时会直接返回原字节）
        let save = median(
            || {
                let mut s = EditSession::open(&bytes).expect("open");
                let first = s.document().paragraphs().map(|b| b.node).next();
                if let Some(para) = first {
                    let _ = s.apply(
                        EditOp::InsertText {
                            at: InlinePos::new(para, 0),
                            text: "x".into(),
                            props: None,
                        },
                        &EditContext::default(),
                    );
                }
                let t = Instant::now();
                let out = s.save_with(&SaveOptions::default()).expect("save");
                std::hint::black_box(&out);
                t.elapsed()
            },
            5,
        );

        // JS 绑定那条路（`ParsedDoc` JSON 文本）：M8 里编辑器打开一份文档的真实代价
        let js_parse = median(
            || {
                let t = Instant::now();
                let text = rsword::bind::js::parse(&bytes).expect("js parse");
                std::hint::black_box(&text);
                t.elapsed()
            },
            5,
        );

        let mut stem = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if stem.chars().count() > 34 {
            stem = stem.chars().take(31).collect::<String>() + "…";
        }
        println!(
            "{stem:<34} {kb:>8.0} {:>8.1}ms {:>9.2}ms {:>10.2}ms {:>9.1}ms {:>10.1}ms",
            ms(open),
            ms(insert),
            ms(accept),
            ms(save),
            ms(js_parse)
        );
    }
}
