//! 绘图显示模型（`MOD-11`，`spec/15` 任务 4.3）在语料上的验收。
//!
//! 投影（`imageWidthPx` 等字段真正出现在 `ParsedDoc` 里）是 4.4 的事；这里先验证**事实**：
//! 每个 `w:drawing` 都能建出显示模型，且 `wp:extent` 与 TS 的 `imageWidthPx/HeightPx` 对得上。

mod common;

use std::collections::BTreeMap;

use rsword::model::units::emu_to_px;
use rsword::model::{Block, Display, Document, DrawingKind, Inline, SegmentKind, Wrap};
use rsword::package::Package;
use serde_json::Value;

#[derive(Default)]
struct Stats {
    docs: usize,
    drawings: usize,
    by_kind: BTreeMap<&'static str, usize>,
    anchored: usize,
    by_wrap: BTreeMap<&'static str, usize>,
    with_media: usize,
    compared_extent: usize,
    /// 认不出 `a:graphicData/@uri` 的绘图，按文档聚合（多半是 MCE 退路里的 VML）。
    unknown_docs: BTreeMap<String, usize>,
    mismatches: Vec<String>,
}

fn kind_name(k: DrawingKind) -> &'static str {
    match k {
        DrawingKind::Picture => "picture",
        DrawingKind::Chart => "chart",
        DrawingKind::ChartEx => "chartEx",
        DrawingKind::Diagram => "diagram",
        DrawingKind::LockedCanvas => "lockedCanvas",
        DrawingKind::Shape => "shape",
        DrawingKind::Group => "group",
        DrawingKind::Line => "line",
        DrawingKind::Unknown => "unknown",
    }
}

fn wrap_name(w: &Wrap) -> &'static str {
    match w {
        Wrap::None => "none",
        Wrap::Square { .. } => "square",
        Wrap::Tight { .. } => "tight",
        Wrap::Through { .. } => "through",
        Wrap::TopAndBottom => "topAndBottom",
        Wrap::Unspecified => "unspecified",
    }
}

/// 段落里所有绘图段的显示模型，按文档序。
fn drawings_of(block: &Block) -> Vec<&rsword::model::DrawingDisplay> {
    let Some(tb) = block.as_text() else { return Vec::new() };
    let mut out = Vec::new();
    for i in &tb.inlines {
        let Inline::Run(r) = i else { continue };
        for s in &r.segments {
            if matches!(s.kind, SegmentKind::Drawing { .. })
                && let Some(Display::Drawing(d)) = &s.display
            {
                out.push(&**d);
            }
        }
    }
    out
}

#[test]
fn mod_11_drawing_facts_across_the_corpus() {
    let mut st = Stats::default();
    for path in common::docx_paths("synthetic").into_iter().chain(common::docx_paths("hostile")) {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
        st.docs += 1;

        // TS 的期望值（hostile 语料没有）
        let expected: Option<Value> = std::fs::read_to_string(path.with_extension("expected.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());

        for (bi, block) in doc.main.iter().enumerate() {
            let ds = drawings_of(block);
            for d in &ds {
                st.drawings += 1;
                *st.by_kind.entry(kind_name(d.kind)).or_default() += 1;
                if d.kind == DrawingKind::Unknown {
                    *st.unknown_docs.entry(file.clone()).or_default() += 1;
                }
                match &d.anchor {
                    Some(a) => {
                        st.anchored += 1;
                        *st.by_wrap.entry(wrap_name(&a.wrap)).or_default() += 1;
                    }
                    None => *st.by_wrap.entry("inline").or_default() += 1,
                }
                if let Some(p) = &d.picture
                    && (p.embed.is_some() || p.link.is_some())
                {
                    st.with_media += 1;
                }
                if d.kind == DrawingKind::Picture {
                    assert!(d.picture.is_some(), "{file}: Picture 绘图应当有 pic:pic");
                }
            }

            // 恰好一个绘图的段落：`wp:extent` 应当与 TS 的 imageWidthPx/HeightPx 一致。
            let (Some(e), [d]) = (&expected, ds.as_slice()) else { continue };
            let Some(tb) = e.get("blocks").and_then(Value::as_array).and_then(|a| a.get(bi)) else {
                continue;
            };
            let Some(ext) = d.extent else { continue };
            for (key, emu) in [("imageWidthPx", ext.cx), ("imageHeightPx", ext.cy)] {
                let Some(ts) = tb.get(key).and_then(Value::as_i64) else { continue };
                st.compared_extent += 1;
                let ours = emu_to_px(emu as f64).round() as i64;
                if ours != ts {
                    st.mismatches.push(format!(
                        "{file}: blocks[{bi}].{key} TS={ts} ours={ours}（cx/cy={emu} EMU）"
                    ));
                }
            }
        }
    }

    println!(
        "drawing: {} 份文档，{} 个绘图（{:?}）；锚定 {}，绕排 {:?}；带媒体 {}；对照 extent {} 项",
        st.docs,
        st.drawings,
        st.by_kind,
        st.anchored,
        st.by_wrap,
        st.with_media,
        st.compared_extent
    );
    println!("  认不出种类的绘图：{:?}", st.unknown_docs);
    assert!(st.drawings > 0, "语料里应当有绘图");
    assert!(st.compared_extent > 0, "应当有能与 TS 对照的 extent");
    assert!(st.mismatches.is_empty(), "extent 与 TS 不一致：\n{}", st.mismatches.join("\n"));
}
