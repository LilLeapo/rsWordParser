//! 生成给**真实 Word** 人工核对的样本（`spec/17` 6.6 / 6.8 的「人工核对项」、`docs/07` §4）：本引擎写出的图表 /
//! 图片 / 墨迹文档放进 `corpus/real/_roundtrip/`，由 Windows 侧在桌面 Word 里打开并记录「有无修复提示」。
//!
//! 默认 `#[ignore]`（它写文件）：`cargo test -p rsword --test roundtrip_samples -- --ignored`。每份样本旁边放一份
//! 未改动的源文档（`*-source.docx`），好把「源文档本身在 Word 里就有提示」和「我们改坏了」分开。

mod common;

use std::path::PathBuf;

use rsword::edit::{
    BlockPos, ChartPatch, ChartSeriesPatch, EditContext, EditOp, EditSession, ImageWrap, InkSave,
    NewBlock, NewChart, NewChartKind, NewChartSeries, NewImage, NewInk,
};
use rsword::model::Block;
use rsword::save::options::CompatSaveOptions as SaveOptions;

fn out_dir() -> PathBuf {
    let d = common::repo_root().join("corpus/real/_roundtrip");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 语料里第一份满足 `ok` 的文档：先找真实 Word 写的（`corpus/real`，Word 打开不进兼容模式、结构完整），
/// 再退回合成语料。
fn base(prefix: &str, ok: impl Fn(&EditSession) -> bool) -> (String, Vec<u8>) {
    for p in common::docx_paths("real").into_iter().chain(common::docx_paths("synthetic")) {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.starts_with(prefix) {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap();
        if let Ok(s) = EditSession::open(&bytes)
            && ok(&s)
        {
            return (name, bytes);
        }
    }
    panic!("语料里没有 {prefix}* 满足条件的文档");
}

fn write(name: &str, bytes: &[u8]) {
    std::fs::write(out_dir().join(name), bytes).unwrap();
}

fn text_paras(s: &EditSession) -> Vec<rsword::xml::NodeId> {
    s.document().main.iter().filter(|b| matches!(b, Block::Text(_))).map(Block::node).collect()
}

fn image(bytes: Vec<u8>, wrap: Option<ImageWrap>) -> NewImage {
    NewImage {
        bytes,
        mime: "image/png".into(),
        extent_emu: (1_828_800, 914_400),
        align: Some("center".into()),
        wrap,
        pos_offset_emu: None,
        z_order: Some(1),
        rot_deg: None,
        flip_h: false,
        flip_v: false,
        para_spacing: None,
    }
}

#[test]
#[ignore = "写 corpus/real/_roundtrip/，手动运行"]
fn generate_roundtrip_samples() {
    let ctx = EditContext::default();
    let png = common::b64(common::PNG_1X1);

    // 01 / 06：纯文字文档里插图表（柱形；折线 + 饼）
    let (text_name, text_bytes) =
        base("text-custom-styles", |s| text_paras(s).len() >= 2 && s.document().inks.is_empty());
    write("01-text-source.docx", &text_bytes);
    let mut s = EditSession::open(&text_bytes).unwrap();
    let p = text_paras(&s)[0];
    let bar = NewChart {
        kind: NewChartKind::Bar,
        title: Some("季度销售".into()),
        categories: vec!["Q1".into(), "Q2".into(), "Q3".into()],
        series: vec![
            NewChartSeries {
                name: "华东".into(),
                values: vec![Some(120.0), Some(88.5), Some(96.0)],
            },
            NewChartSeries { name: "华南".into(), values: vec![Some(70.0), None, Some(110.0)] },
        ],
    };
    s.apply(
        EditOp::InsertBlock {
            at: BlockPos::after(p),
            block: NewBlock::Chart { chart: bar, extent_emu: None },
        },
        &ctx,
    )
    .unwrap();
    write("01-chart-insert-bar.docx", &s.save().unwrap());

    let mut s = EditSession::open(&text_bytes).unwrap();
    let p = text_paras(&s)[0];
    for (i, (kind, title)) in
        [(NewChartKind::Line, "趋势"), (NewChartKind::Pie, "占比")].into_iter().enumerate()
    {
        let chart = NewChart {
            kind,
            title: Some(title.into()),
            categories: vec!["一月".into(), "二月".into(), "三月".into()],
            series: vec![NewChartSeries {
                name: format!("系列{}", i + 1),
                values: vec![Some(3.0), Some(5.0), Some(2.0)],
            }],
        };
        s.apply(
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Chart { chart, extent_emu: Some((4_572_000, 2_743_200)) },
            },
            &ctx,
        )
        .unwrap();
    }
    write("06-chart-insert-line-pie.docx", &s.save().unwrap());

    // 02：已有图表改数据（只改缓存文本）
    let (chart_name, chart_bytes) = base("chart-column", |s| !s.document().chart_parts.is_empty());
    write("02-chart-source.docx", &chart_bytes);
    let mut s = EditSession::open(&chart_bytes).unwrap();
    let part = *s.document().chart_parts.keys().next().unwrap();
    s.apply(
        EditOp::SetChartData {
            part,
            patch: ChartPatch {
                title: Some("已改标题 Edited".into()),
                categories: None,
                series: Some(vec![Some(ChartSeriesPatch {
                    name: Some("改名系列".into()),
                    values: Some(vec![Some(9.0), Some(8.0), Some(7.0)]),
                })]),
            },
        },
        &ctx,
    )
    .unwrap();
    write("02-chart-setdata.docx", &s.save().unwrap());

    // 03：插两张图片（随文 + 四周型环绕）
    let mut s = EditSession::open(&text_bytes).unwrap();
    let p = text_paras(&s)[0];
    s.apply_all(
        vec![
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Image(image(png.clone(), None)),
            },
            EditOp::InsertBlock {
                at: BlockPos::after(p),
                block: NewBlock::Image(image(png.clone(), Some(ImageWrap::SquareRight))),
            },
        ],
        &ctx,
    )
    .unwrap();
    write("03-image-insert-inline-and-square.docx", &s.save().unwrap());

    // 04：换掉已有图片的媒体
    let (pic_name, pic_bytes) = base("image-wrap-square", |s| {
        s.document().main.iter().any(|b| matches!(b, Block::Image(_)))
    });
    write("04-image-source.docx", &pic_bytes);
    let mut s = EditSession::open(&pic_bytes).unwrap();
    let drawing = {
        let dom = s.dom();
        dom.descendants(dom.root())
            .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::Drawing)))
            .unwrap()
    };
    s.apply(
        EditOp::ReplaceImageMedia { drawing, bytes: png.clone(), mime: "image/png".into() },
        &ctx,
    )
    .unwrap();
    write("04-image-replace.docx", &s.save().unwrap());

    // 05：墨迹层（两条，锚在第一段）
    let mut s = EditSession::open(&text_bytes).unwrap();
    let p = text_paras(&s)[0];
    let ink = |x: f64, y: f64| NewInk {
        png: png.clone(),
        width_px: 200.0,
        height_px: 80.0,
        offset_x_px: x,
        offset_y_px: y,
        payload: Some(r#"{"strokes":[{"tool":"pen","color":"C00000"}]}"#.into()),
    };
    let out = s
        .save_with_compat(&SaveOptions {
            inks: Some(vec![
                InkSave { para: p, ink: ink(40.0, -10.0) },
                InkSave { para: p, ink: ink(300.0, 20.0) },
            ]),
            ..SaveOptions::default()
        })
        .unwrap();
    write("05-ink-insert.docx", &out);

    eprintln!(
        "bases: text={text_name} chart={chart_name} picture={pic_name} → {}",
        out_dir().display()
    );
}
