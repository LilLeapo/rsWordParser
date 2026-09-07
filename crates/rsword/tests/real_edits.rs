//! 给桌面 Word 验收的**编辑后真实文档**（`docs/08`，第二轮）：对 `corpus/real` 里每一份 Word 写出的文档做几种编辑并保存，
//! 写到 `corpus/real/_roundtrip/edited/`（不进仓库，`.gitignore`），连同一份清单 `MANIFEST.md` / `manifest.json`，
//! 由 Windows 侧逐份用 Word 打开：有无修复提示、看到的是不是清单里写的东西。
//!
//! 默认 `#[ignore]`：`cargo test -p rsword --test real_edits -- --ignored --nocapture`。

mod common;

use std::path::{Path, PathBuf};

use rsword::edit::{
    BlockPos, ChartPatch, ChartSeriesPatch, EditContext, EditOp, EditSession, ImageWrap, InkSave,
    InlinePos, NewBlock, NewChart, NewChartKind, NewChartSeries, NewComment, NewImage, NewInk,
    NewInline, NewRun,
};
use rsword::model::{Block, HfKind, HfVariant, ProtectedKind, SectionOwner};
use rsword::save::SaveOptions;
use rsword::xml::NodeId;

fn out_dir() -> PathBuf {
    let d = common::repo_root().join("corpus/real/_roundtrip/edited");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 正文顶层的文本段落。
fn top_text_paras(s: &EditSession) -> Vec<(NodeId, u32)> {
    s.document().main.iter().filter_map(|b| b.as_text()).map(|t| (t.node, t.utf16_len())).collect()
}

fn content_blocks(s: &EditSession) -> Vec<NodeId> {
    s.document()
        .main
        .iter()
        .filter(|b| !matches!(b, Block::Protected(p) if p.kind == ProtectedKind::SectionProps))
        .map(Block::node)
        .collect()
}

fn image(bytes: Vec<u8>) -> NewImage {
    NewImage {
        bytes,
        mime: "image/png".into(),
        extent_emu: (1_371_600, 685_800),
        align: None,
        wrap: Some(ImageWrap::SquareRight),
        pos_offset_emu: None,
        z_order: Some(2),
        rot_deg: None,
        flip_h: false,
        flip_v: false,
        para_spacing: None,
    }
}

struct Row {
    file: String,
    base: String,
    op: &'static str,
    expect: String,
    status: String,
}

/// 一种编辑：`f` 返回「Word 里应看到什么」；`Err` 记为跳过（拒绝是合法结果）。
fn variant(
    rows: &mut Vec<Row>,
    base: &Path,
    bytes: &[u8],
    op: &'static str,
    f: impl FnOnce(&mut EditSession) -> Result<Option<String>, rsword::Error>,
) {
    let stem = base.file_stem().unwrap().to_string_lossy().to_string();
    let file = format!("{stem}--{op}.docx");
    let base_name = format!(
        "{}/{}",
        base.parent().unwrap().file_name().unwrap().to_string_lossy(),
        base.file_name().unwrap().to_string_lossy()
    );
    let mut s = match EditSession::open(bytes) {
        Ok(s) => s,
        Err(e) => {
            rows.push(Row {
                file,
                base: base_name,
                op,
                expect: String::new(),
                status: format!("open failed: {e}"),
            });
            return;
        }
    };
    match f(&mut s) {
        Ok(Some(expect)) => match s.save() {
            Ok(out) => {
                std::fs::write(out_dir().join(&file), &out).unwrap();
                rows.push(Row { file, base: base_name, op, expect, status: "generated".into() });
            }
            Err(e) => rows.push(Row {
                file,
                base: base_name,
                op,
                expect,
                status: format!("save failed: {e}"),
            }),
        },
        Ok(None) => {}
        Err(e) => rows.push(Row {
            file,
            base: base_name,
            op,
            expect: String::new(),
            status: format!("skipped: {}", e.to_string().lines().next().unwrap_or("")),
        }),
    }
}

#[test]
#[ignore = "写 corpus/real/_roundtrip/edited/，手动运行"]
fn generate_edited_real_documents() {
    let ctx = EditContext::default();
    let png = common::b64(common::PNG_1X1);
    let mut rows: Vec<Row> = Vec::new();
    let docs: Vec<PathBuf> = common::docx_paths("real")
        .into_iter()
        .filter(|p| !p.to_string_lossy().contains("/_roundtrip/"))
        .collect();
    for path in &docs {
        let bytes = std::fs::read(path).unwrap();
        // 1. 首段开头插字
        variant(&mut rows, path, &bytes, "insert", |s| {
            let Some(&(p, _)) = top_text_paras(s).first() else { return Ok(None) };
            s.apply(
                EditOp::InsertText {
                    at: InlinePos::new(p, 0),
                    text: "rsword✎ ".into(),
                    props: None,
                },
                &ctx,
            )?;
            Ok(Some("第一个文字段落以「rsword✎ 」开头，其余一切不变".into()))
        });
        // 2. 首段后插一张浮动图片
        variant(&mut rows, path, &bytes, "newimage", |s| {
            let Some(&(p, _)) = top_text_paras(s).first() else { return Ok(None) };
            s.apply(
                EditOp::InsertBlock {
                    at: BlockPos::after(p),
                    block: NewBlock::Image(image(png.clone())),
                },
                &ctx,
            )?;
            Ok(Some("第一个文字段落之后多了一块 108 × 54 pt 的纯色小图，四周型环绕靠右".into()))
        });
        // 3. 首段后插一张柱形图
        variant(&mut rows, path, &bytes, "newchart", |s| {
            let Some(&(p, _)) = top_text_paras(s).first() else { return Ok(None) };
            let chart = NewChart {
                kind: NewChartKind::Bar,
                title: Some("rsword 新图表".into()),
                categories: vec!["甲".into(), "乙".into(), "丙".into()],
                series: vec![NewChartSeries {
                    name: "系列 A".into(),
                    values: vec![Some(3.0), Some(1.0), Some(2.0)],
                }],
            };
            s.apply(
                EditOp::InsertBlock {
                    at: BlockPos::after(p),
                    block: NewBlock::Chart { chart, extent_emu: Some((4_572_000, 2_743_200)) },
                },
                &ctx,
            )?;
            Ok(Some("第一个文字段落之后多了一张簇状柱形图「rsword 新图表」（甲 3 / 乙 1 / 丙 2）；右键「编辑数据」能打开内嵌工作簿".into()))
        });
        // 4. 首段上加一条墨迹（浮动图片）
        variant(&mut rows, path, &bytes, "ink", |s| {
            let Some(&(p, _)) = top_text_paras(s).first() else { return Ok(None) };
            let ink = NewInk {
                png: png.clone(),
                width_px: 160.0,
                height_px: 40.0,
                offset_x_px: 20.0,
                offset_y_px: -5.0,
                payload: Some(r#"{"strokes":[]}"#.into()),
            };
            let out = s.save_with(&SaveOptions {
                inks: Some(vec![InkSave { para: p, ink }]),
                ..SaveOptions::default()
            })?;
            std::fs::write(
                out_dir()
                    .join(format!("{}--ink.docx", path.file_stem().unwrap().to_string_lossy())),
                &out,
            )
            .unwrap();
            Ok(None) // 已自己写文件；下面补一行清单
        });
        if !top_text_paras(&EditSession::open(&bytes).unwrap()).is_empty() {
            rows.push(Row {
                file: format!("{}--ink.docx", path.file_stem().unwrap().to_string_lossy()),
                base: format!("{}/{}", path.parent().unwrap().file_name().unwrap().to_string_lossy(), path.file_name().unwrap().to_string_lossy()),
                op: "ink",
                expect: "第一个文字段落**前方**压着一块 120 × 30 pt 的纯色浮动图片（偏移 15 / −3.75 pt），文字不被挤开".into(),
                status: "generated".into(),
            });
        }
        // 5. 删掉第二个内容块（特征块多半在这里；它引用的媒体 / part 应随之回收）
        variant(&mut rows, path, &bytes, "deleteblock", |s| {
            let blocks = content_blocks(s);
            if blocks.len() < 3 {
                return Ok(None);
            }
            s.apply(EditOp::DeleteBlock { part: None, node: blocks[1] }, &ctx)?;
            Ok(Some("正文第二个块（多半是特征本身）整块消失，前后文照旧；文件里它独占的媒体 / 图表 part 一起消失".into()))
        });
        // 6. 图表数据（只改缓存文本）
        variant(&mut rows, path, &bytes, "chartdata", |s| {
            let Some(&part) = s.document().chart_parts.keys().next() else { return Ok(None) };
            s.apply(
                EditOp::SetChartData {
                    part,
                    patch: ChartPatch {
                        title: Some("标题已改 rsword".into()),
                        categories: None,
                        series: Some(vec![Some(ChartSeriesPatch {
                            name: Some("改名系列".into()),
                            values: Some(vec![Some(11.0), Some(21.0), Some(31.0)]),
                        })]),
                    },
                },
                &ctx,
            )?;
            Ok(Some("图表标题变成「标题已改 rsword」，第一系列改名「改名系列」、值 11 / 21 / 31（内嵌工作簿**未改**——请记录「编辑数据」后 Word 是否用工作簿旧值刷回图表）".into()))
        });
        // 7. 换掉第一张图片的媒体
        variant(&mut rows, path, &bytes, "replaceimage", |s| {
            let drawing = {
                let dom = s.dom();
                let pic = s
                    .document()
                    .main
                    .iter()
                    .find(|b| matches!(b, Block::Image(_)))
                    .map(Block::node);
                let Some(p) = pic else { return Ok(None) };
                dom.descendants(p)
                    .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::Drawing)))
            };
            let Some(drawing) = drawing else { return Ok(None) };
            s.apply(
                EditOp::ReplaceImageMedia { drawing, bytes: png.clone(), mime: "image/png".into() },
                &ctx,
            )?;
            Ok(Some("那张图片位置、大小、环绕不变，内容变成一块纯色；原来的裁剪被取消".into()))
        });
        // 8. 表格：第 1 行前插一行；合并首行前两格
        variant(&mut rows, path, &bytes, "insertrow", |s| {
            let Some(t) = s.document().tables().next().map(|t| t.node) else { return Ok(None) };
            s.apply(EditOp::InsertRow { table: t, at: 1, template: None }, &ctx)?;
            Ok(Some("第一张表格在第 1 行之后多了一行空格子（格式照第 1 行）".into()))
        });
        variant(&mut rows, path, &bytes, "mergecells", |s| {
            let Some(t) = s.document().tables().next() else { return Ok(None) };
            // 找第一行前两格还没合并的行（首行常是已合并的标题行，合并已合并的格是幂等操作，看不出变化）
            let Some(row) = t.rows.iter().position(|r| {
                r.cells.len() >= 2 && r.cells.iter().take(2).all(|c| c.grid_span() == 1)
            }) else {
                return Ok(None);
            };
            let node = t.node;
            s.apply(
                EditOp::MergeCells { table: node, from: (row as u32, 0), to: (row as u32, 1) },
                &ctx,
            )?;
            Ok(Some(format!("第一张表格第 {} 行的前两格合并成一格（文字连在一起）", row + 1)))
        });
        // 9. 页眉：默认页眉整体替换
        variant(&mut rows, path, &bytes, "header", |s| {
            let sect = s
                .document()
                .sections
                .last()
                .filter(|x| x.owner == SectionOwner::Body)
                .and_then(|x| x.node);
            let Some(sect) = sect else { return Ok(None) };
            s.apply(
                EditOp::SetHeaderFooter {
                    sect,
                    kind: HfKind::Header,
                    variant: HfVariant::Default,
                    content: vec![NewBlock::Paragraph {
                        props: None,
                        inlines: vec![NewInline::Run(NewRun::text("rsword 页眉"))],
                    }],
                },
                &ctx,
            )?;
            Ok(Some("最后一节的默认页眉只剩一行「rsword 页眉」（原有页眉内容被替换；首页 / 偶数页页眉若有则不动）".into()))
        });
        // 10. 批注 + 拆段
        variant(&mut rows, path, &bytes, "comment", |s| {
            let Some(&(p, len)) = top_text_paras(s).iter().find(|(_, l)| *l >= 2) else {
                return Ok(None);
            };
            s.apply(
                EditOp::AddComment {
                    from: InlinePos::new(p, 0),
                    to: InlinePos::new(p, len.min(4)),
                    comment: NewComment {
                        author: "rsword".into(),
                        initials: Some("rs".into()),
                        date: Some("2026-09-07T00:00:00Z".into()),
                        text: "rsword 批注".into(),
                        ..Default::default()
                    },
                },
                &ctx,
            )?;
            Ok(Some("第一个有字的段落开头几个字上有一条作者 rsword 的批注「rsword 批注」".into()))
        });
        variant(&mut rows, path, &bytes, "split", |s| {
            let Some(&(p, len)) = top_text_paras(s).iter().find(|(_, l)| *l >= 4) else {
                return Ok(None);
            };
            let _ = len;
            s.apply(EditOp::SplitParagraph { at: InlinePos::new(p, 2) }, &ctx)?;
            Ok(Some("第一个够长的段落在第 2 个字符后被拆成两段（第二段继承同样的段落格式）".into()))
        });
    }

    // 清单
    let mut md = String::from(
        "# `_roundtrip/edited/` 清单（由 `cargo test -p rsword --test real_edits -- --ignored` 生成）\n\n每份 = 一份真实 Word 文档 + 一种编辑 + 本引擎保存。请逐份用 Word 打开，按「期望」核对，结果写进 `EDITED.md`。\n\n| 文件 | 底稿 | 编辑 | 期望在 Word 里看到 | 生成状态 |\n| --- | --- | --- | --- | --- |\n",
    );
    let mut json = Vec::new();
    for r in &rows {
        md.push_str(&format!(
            "| `{}` | `{}` | {} | {} | {} |\n",
            r.file, r.base, r.op, r.expect, r.status
        ));
        json.push(serde_json::json!({ "file": r.file, "base": r.base, "op": r.op, "expect": r.expect, "status": r.status }));
    }
    std::fs::write(out_dir().join("MANIFEST.md"), md).unwrap();
    std::fs::write(out_dir().join("manifest.json"), serde_json::to_string_pretty(&json).unwrap())
        .unwrap();
    let generated = rows.iter().filter(|r| r.status == "generated").count();
    let skipped = rows.len() - generated;
    eprintln!(
        "edited real documents: {generated} generated, {skipped} skipped/failed → {}",
        out_dir().display()
    );
    assert!(generated > 200);
}
