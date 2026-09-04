//! 绘图显示模型（`MOD-11`，`spec/15` 任务 4.3）在语料上的验收。
//!
//! 投影（`imageWidthPx` 等字段真正出现在 `ParsedDoc` 里）是 4.4 的事；这里先验证**事实**：
//! 每个 `w:drawing` 都能建出显示模型，且 `wp:extent` 与 TS 的 `imageWidthPx/HeightPx` 对得上。

mod common;

use std::collections::BTreeMap;

use rsword::bind::compat_ts::parsed_doc;
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

/// `TEST-09` 恶意绘图树：深嵌套 / 退化的组 / 悬空关系 / 畸形 `style`。
///
/// 四份用例都在正常段落旁边放一个病态绘图，验收线是「局部降级」：整份文档照样解析、
/// 正常那段的字一个不少、未编辑保存字节不变（后者由 `save_01_no_edit_returns_original_bytes_for_all_corpus`
/// 全语料覆盖），病态的那部分给不出就不给，不猜、不 panic、不爆栈。
#[test]
fn test_09_hostile_drawing_trees_degrade_locally() {
    /// 块里所有 run 的文字。
    fn texts(v: &Value) -> String {
        v.get("blocks")
            .and_then(Value::as_array)
            .map(|bs| {
                bs.iter()
                    .filter_map(|b| b.get("runs").and_then(Value::as_array))
                    .flatten()
                    .filter_map(|r| r.get("text").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// 所有块的 `textboxes[]` 摊平。
    fn boxes(v: &Value) -> Vec<&Value> {
        v.get("blocks")
            .and_then(Value::as_array)
            .map(|bs| {
                bs.iter()
                    .filter_map(|b| b.get("textboxes").and_then(Value::as_array))
                    .flatten()
                    .collect()
            })
            .unwrap_or_default()
    }
    /// 每个数字字段都必须是有限值——`NaN` / `Infinity` 连 JSON 都序列化不出来。
    fn all_numbers_finite(v: &Value) -> bool {
        match v {
            Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
            Value::Array(a) => a.iter().all(all_numbers_finite),
            Value::Object(o) => o.values().all(all_numbers_finite),
            _ => true,
        }
    }

    let dir = common::corpus_dir("hostile");
    let load = |name: &str| -> Value {
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut pkg = Package::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let v = parsed_doc(&mut pkg).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(texts(&v).contains("hello"), "{name}: 旁边那段正常文字必须留住");
        assert!(all_numbers_finite(&v), "{name}: 投影里出现了非有限的数");
        v
    };

    // 3000 层 wpg 组套娃：遍历是迭代的所以不爆栈，深度上限之外的形状认不到，
    // 段落于是没有框可提取——但它仍然是一个完整的块。
    let deep = load("drawing-deep-groups.docx");
    assert!(boxes(&deep).is_empty(), "超过深度上限的形状不该冒出框来");
    assert_eq!(deep["blocks"][0]["label"], "Drawing object", "整段仍是一个完整的块");

    // 退化的画布：`coordsize="0,0"` 定不出缩放、坐标离谱、`v:group` 与 `v:shapetype` 同 id。
    // 组链靠下标向上走（孩子的下标恒大于组），构造不出环，所以这里只验证不循环也不出脏数。
    let cyclic = load("drawing-cyclic-group.docx");
    let cyclic_boxes = boxes(&cyclic);
    assert_eq!(cyclic_boxes.len(), 1, "画布里那个带字的形状仍要成框");
    assert_eq!(cyclic["blocks"][0]["previewText"], "canvas child");
    for b in &cyclic_boxes {
        // `coordsize="0,0"` 定不出缩放，`width:1e400` 不是有限值：这些尺寸一律不给
        for k in ["widthPx", "heightPx"] {
            assert!(b.get(k).is_none(), "定不出的 {k} 不该猜一个出来：{b:?}");
        }
    }

    // 悬空关系：图、VML 预览图、外部文本框 part 的 r:id 全都指向不存在的关系。
    let missing = load("drawing-missing-rels.docx");
    // 原字节按 `COMPAT-04` 原样带出，所以只看**投影出来**的字段
    let mut urls = Vec::new();
    collect_by_key(&missing, "DataUrl", &mut urls);
    assert!(urls.is_empty(), "解析不出来的媒体不该编出 dataURL：{urls:?}");
    assert!(
        missing["blocks"][0]["originalXml"].as_str().is_some_and(|x| x.contains("rIdGone1")),
        "悬空关系的原字节要原样留着"
    );

    // 畸形 `style` / `coordsize` / `path` / 颜色：认不出的值一律不给，不猜。
    let junk = load("drawing-bad-style.docx");
    let junk_boxes = boxes(&junk);
    assert_eq!(junk_boxes.len(), 1, "框里的字要留住");
    assert_eq!(junk["blocks"][0]["previewText"], "junk");
    for b in &junk_boxes {
        // `width:--3pt` / `height:0pt` / `margin-left:NaNpt` 都不是长度
        for k in ["widthPx", "heightPx", "fill", "borderColor"] {
            assert!(b.get(k).is_none(), "认不出的 {k} 不该给：{b:?}");
        }
        // `path="m0,0c1"` 是曲线命令，转不出来就整条不给——绝不退化成一个实心包围盒
        assert!(b.get("pathData").is_none(), "转不出的 VML 路径不该给 pathData");
        // `fillcolor="#zzzzzz"` 不是颜色
        assert_ne!(b.get("fill").and_then(Value::as_str), Some("zzzzzz"));
    }
}

/// 收集所有键名以 `suffix` 结尾的字符串值。
fn collect_by_key<'a>(v: &'a Value, suffix: &str, out: &mut Vec<&'a str>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k.ends_with(suffix)
                    && let Some(s) = x.as_str()
                {
                    out.push(s);
                }
                collect_by_key(x, suffix, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_by_key(x, suffix, out)),
        _ => {}
    }
}
