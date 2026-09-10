//! `TEST-07` 的嵌入对象子集（`spec/17` 任务 6.9）：`SetChartData` / `InsertBlock{Chart}` / `InsertBlock{Image}` /
//! `ReplaceImageMedia` / `InsertInk` / `RemoveInks` / `DeleteBlock` / `InsertText` 混合的随机序列，100 步 × 10 份带图片或
//! 图表的语料。每步 `refresh == rebuild`（块与墨迹表）、`SAVE-02` 无 `EngineInvariantViolation`；每 20 步保存 + 重解析，
//! 检查包里没有**新的**悬空关系与孤儿 part（受管目录下没人引用的 part），接着在重开的会话上继续。

mod common;

use std::collections::BTreeSet;

use rsword::edit::{
    BlockPos, ChartPatch, ChartSeriesPatch, EditContext, EditOp, EditSession, InlinePos, NewBlock,
    NewChart, NewChartKind, NewChartSeries, NewImage, NewInk,
};
use rsword::model::{Block, Document, ProtectedKind};
use rsword::package::{Package, RelTarget};
use rsword::xml::{LocalName, NodeId, NsId, QName};

const GIF_1X1: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

fn image(rng: &mut common::Rng) -> NewImage {
    let gif = rng.below(2) == 0;
    NewImage {
        bytes: common::b64(if gif { GIF_1X1 } else { common::PNG_1X1 }),
        mime: if gif { "image/gif" } else { "image/png" }.into(),
        extent_emu: (609_600, 304_800),
        align: None,
        wrap: None,
        pos_offset_emu: None,
        z_order: None,
        rot_deg: None,
        flip_h: false,
        flip_v: false,
        para_spacing: None,
    }
}

fn chart(step: usize) -> NewChart {
    NewChart {
        kind: [NewChartKind::Bar, NewChartKind::Line, NewChartKind::Pie][step % 3],
        title: Some(format!("C{step}")),
        categories: vec!["a".into(), "b".into()],
        series: vec![NewChartSeries { name: format!("s{step}"), values: vec![Some(1.0), None] }],
    }
}

fn ink(step: usize) -> NewInk {
    NewInk {
        png: common::b64(common::PNG_1X1),
        width_px: 20.0 + step as f64,
        height_px: 10.0,
        offset_x_px: step as f64,
        offset_y_px: -1.0,
        payload: Some(format!("{{\"s\":{step}}}")),
    }
}

/// 正文顶层的内容块（不含尾部 `w:sectPr`）。
fn content_blocks(doc: &Document) -> Vec<NodeId> {
    doc.main
        .iter()
        .filter(|b| !matches!(b, Block::Protected(p) if p.kind == ProtectedKind::SectionProps))
        .map(rsword::model::Block::node)
        .collect()
}

fn text_paragraphs(doc: &Document) -> Vec<NodeId> {
    doc.blocks().filter_map(|b| b.as_text()).map(|t| t.node).collect()
}

fn drawings_with_blip(s: &EditSession) -> Vec<NodeId> {
    let dom = s.dom();
    dom.semantic_descendants(dom.root())
        .filter(|&n| {
            dom.is(n, QName::w(LocalName::Drawing))
                && dom
                    .semantic_descendants(n)
                    .any(|c| dom.is(c, QName::new(NsId::A, LocalName::Blip)))
        })
        .collect()
}

fn random_op(s: &EditSession, rng: &mut common::Rng, step: usize) -> Option<EditOp> {
    let doc = s.document();
    let blocks = content_blocks(doc);
    let paras = text_paragraphs(doc);
    let pick = |rng: &mut common::Rng, v: &[NodeId]| v.get(rng.below(v.len())).copied();
    let at = |rng: &mut common::Rng| {
        pick(rng, &blocks)
            .map(BlockPos::after)
            .unwrap_or_else(|| BlockPos::after(doc.main.last().expect("至少有 sectPr").node()))
    };
    Some(match rng.below(9) {
        0 => {
            let parts: Vec<_> = doc.chart_parts.keys().copied().collect();
            let part = *parts.get(rng.below(parts.len()))?;
            EditOp::SetChartData {
                part,
                patch: ChartPatch {
                    title: Some(format!("T{step}")),
                    categories: None,
                    series: Some(vec![Some(ChartSeriesPatch {
                        name: Some(format!("n{step}")),
                        values: Some(vec![Some(step as f64), None, Some(0.5)]),
                    })]),
                },
            }
        }
        1 => EditOp::InsertBlock {
            at: at(rng),
            block: NewBlock::Chart { chart: chart(step), extent_emu: None },
        },
        2 => EditOp::InsertBlock { at: at(rng), block: NewBlock::Image(image(rng)) },
        3 => {
            let d = drawings_with_blip(s);
            let drawing = pick(rng, &d)?;
            let img = image(rng);
            EditOp::ReplaceImageMedia { drawing, bytes: img.bytes, mime: img.mime }
        }
        4 => EditOp::InsertInk { para: pick(rng, &paras)?, ink: ink(step) },
        5 => EditOp::RemoveInks,
        6 => EditOp::DeleteBlock { part: None, node: pick(rng, &blocks)? },
        _ => {
            let p = pick(rng, &paras)?;
            let len = doc.text_block(p)?.utf16_len();
            EditOp::InsertText {
                at: InlinePos::new(p, rng.below(len as usize + 1) as u32),
                text: format!("t{step}"),
                props: None,
            }
        }
    })
}

/// 包的一致性：主 part 里 `r:` 命名空间引用的关系 id 都存在；受管目录下的 part 都有关系指向它。
/// 返回 (悬空的 rId, 孤儿 part)——与源文档的基线比较，只许不新增。
fn dangling_and_orphans(bytes: &[u8]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut pkg = Package::open(bytes).expect("reopen");
    let main = pkg.main_part();
    let mut dangling = BTreeSet::new();
    {
        let dom = pkg.dom(main).expect("parse").expect("xml").clone();
        let rels = &pkg.part(main).rels;
        for n in dom.semantic_descendants(dom.root()) {
            let Some(e) = dom.element(n) else { continue };
            for a in &e.attrs {
                if a.name.ns == NsId::R {
                    let v = dom.attr_str(a).trim().to_string();
                    if !v.is_empty() && rels.by_id(&v).is_none() {
                        dangling.insert(v);
                    }
                }
            }
        }
    }
    let referenced: BTreeSet<String> = pkg
        .parts()
        .iter()
        .flat_map(|p| {
            p.rels.iter().filter_map(|r| match &r.target {
                RelTarget::Internal(u) => Some(u.as_str().to_string()),
                RelTarget::External(_) => None,
            })
        })
        .collect();
    let orphans = pkg
        .parts()
        .iter()
        .map(|p| p.uri.as_str().to_string())
        .filter(|u| {
            (u.starts_with("word/media/")
                || u.starts_with("word/charts/")
                || u.starts_with("word/embeddings/"))
                && !u.ends_with(".rels")
                && !referenced.contains(u)
        })
        .collect();
    (dangling, orphans)
}

#[test]
fn test_07_random_embedded_edit_sequences() {
    const STEPS: usize = 100;
    let mut docs: Vec<_> = common::docx_paths("synthetic")
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_str().unwrap().starts_with("m6-chart__"))
        .take(5)
        .collect();
    docs.extend(
        common::docx_paths("synthetic")
            .into_iter()
            .filter(|p| p.file_name().unwrap().to_str().unwrap().starts_with("m6-image__"))
            .take(5),
    );
    assert_eq!(docs.len(), 10, "语料里应有 5 份图表 + 5 份图片文档");

    let (mut applied, mut rejected, mut saves) = (0usize, 0usize, 0usize);
    for (di, path) in docs.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let (base_dangling, base_orphans) = dangling_and_orphans(&bytes);
        let mut s = EditSession::open(&bytes).unwrap();
        let mut rng = common::Rng(0xD1B5_4A32_D192_ED03 ^ (di as u64 + 1));
        for step in 0..STEPS {
            let Some(op) = random_op(&s, &mut rng, step) else { continue };
            let what = format!("{}: step {step} {op:?}", path.display());
            match s.apply(op, &EditContext::default()) {
                Ok(_) => applied += 1,
                Err(rsword::Error::Edit { .. }) => {
                    rejected += 1;
                    continue; // 拒绝是合法结果；EDIT-05 保证状态没动
                }
                Err(e) => panic!("{what}: 非编辑错误 {e}"),
            }
            // MOD-13：投影 == 重建（块与墨迹表）
            let refreshed = s.document().clone();
            let rebuilt = rsword::model::Document::rebuild(s.package_mut()).unwrap();
            assert_eq!(refreshed.main, rebuilt.main, "{what}: refresh != rebuild");
            assert_eq!(refreshed.inks, rebuilt.inks, "{what}: 墨迹表 refresh != rebuild");
            assert!(
                !s.diagnostics()
                    .iter()
                    .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
                "{what}: {:?}",
                s.diagnostics()
            );
            if step % 20 == 19 {
                let saved = s.save().unwrap_or_else(|e| panic!("{what}: save {e}"));
                saves += 1;
                let (dangling, orphans) = dangling_and_orphans(&saved);
                assert!(
                    dangling.is_subset(&base_dangling),
                    "{what}: 新的悬空关系 {:?}",
                    dangling.difference(&base_dangling).collect::<Vec<_>>()
                );
                assert!(
                    orphans.is_subset(&base_orphans),
                    "{what}: 新的孤儿 part {:?}",
                    orphans.difference(&base_orphans).collect::<Vec<_>>()
                );
                let re = EditSession::open(&saved).unwrap_or_else(|e| panic!("{what}: reopen {e}"));
                assert_eq!(
                    re.document().inks.len(),
                    s.document().inks.len(),
                    "{what}: 保存往返后墨迹数变了"
                );
                assert_eq!(
                    content_blocks(re.document()).len(),
                    content_blocks(s.document()).len(),
                    "{what}: 保存往返后块数变了"
                );
                s = re;
            }
        }
    }
    eprintln!("random embedded ops: {applied} 次生效，{rejected} 次被拒，{saves} 次保存");
    assert!(applied > 300, "有效操作太少：{applied}");
}
