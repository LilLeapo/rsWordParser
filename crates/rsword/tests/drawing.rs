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
                *st.by_kind.entry(d.kind.as_str()).or_default() += 1;
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
                if let Some(p) = d.picture()
                    && (p.embed.is_some() || p.link.is_some())
                {
                    st.with_media += 1;
                }
                if d.kind == DrawingKind::Picture {
                    assert!(d.picture().is_some(), "{file}: Picture 绘图应当有 pic:pic");
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

/// VML 与嵌入对象（`MOD-11`，`spec/15` 任务 4.5 / 4.7）在语料上的普查。
#[test]
fn mod_11_vml_and_ole_across_the_corpus() {
    use rsword::model::VmlDisplay;

    let mut docs = 0usize;
    let mut picts = 0usize;
    let mut shapes = 0usize;
    let mut by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut rules = 0usize;
    let mut images = 0usize;
    let mut textboxes = 0usize;
    let mut ole = 0usize;
    let mut prog_ids: BTreeMap<String, usize> = BTreeMap::new();

    for path in common::docx_paths("synthetic").into_iter().chain(common::docx_paths("hostile")) {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = Document::rebuild(&mut pkg) else { continue };
        docs += 1;
        let mut seen: Vec<&VmlDisplay> = Vec::new();
        for block in &doc.main {
            match block {
                Block::Protected(b) => seen.extend(b.display.as_ref().and_then(Display::as_vml)),
                Block::Image(b) => seen.extend(b.display.as_ref().and_then(Display::as_vml)),
                Block::Text(tb) => {
                    for i in &tb.inlines {
                        let Inline::Run(r) = i else { continue };
                        for s in &r.segments {
                            if matches!(s.kind, SegmentKind::Pict | SegmentKind::Object) {
                                seen.extend(s.display.as_ref().and_then(Display::as_vml));
                            }
                        }
                    }
                }
                Block::Table(_) => {}
            }
        }
        for v in seen {
            picts += 1;
            shapes += v.shapes.len();
            for s in &v.shapes {
                *by_kind.entry(s.kind.as_str()).or_default() += 1;
            }
            if v.rule().is_some() {
                rules += 1;
            }
            if v.image().is_some() {
                images += 1;
            }
            if v.has_textbox() {
                textboxes += 1;
            }
            if let Some(o) = &v.ole {
                ole += 1;
                if let Some(id) = &o.prog_id {
                    *prog_ids.entry(id.clone()).or_default() += 1;
                }
            }
        }
    }

    println!(
        "vml: {docs} 份文档，{picts} 个 pict/object，{shapes} 个形状（{by_kind:?}）；\
         细横线 {rules}，带图 {images}，带文本框 {textboxes}，嵌入对象 {ole}（{prog_ids:?}）"
    );
    assert!(picts > 0, "语料里应当有 VML");
    assert!(rules > 0, "语料里应当有 v:rect o:hr 细横线");
    assert!(ole > 0, "语料里应当有 w:object 嵌入对象");
    // `resource-cleanup__008` 的 `<o:OLEObject>` 不声明 xmlns:o，靠前缀字面量兜底才认得出来
    assert!(prog_ids.contains_key("Package"), "未绑定前缀的 OLEObject 也要认出 ProgID");
}
