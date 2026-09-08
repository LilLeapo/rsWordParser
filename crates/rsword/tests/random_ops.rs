//! `TEST-07`（`spec/18` 门 5）：随机编辑序列。
//!
//! 每条序列 = 一份语料 × 一个种子 × N 步。每一步随机挑一个 `EditOp`、随机开关 `track_changes`
//! （`None` / 作者甲 / 作者乙），断言：
//!
//! - 不 panic；非 `Error::Edit` 的错误一律算失败（编辑层拒绝是合法结果，`EDIT-05` 保证状态没动）
//! - `MOD-13`：增量刷新的投影与从 DOM 整体重建相等
//! - 没有 `EngineInvariantViolation` 诊断
//! - 每 20 步保存一次：`SAVE-02` / `SPAN-09` / `FLD-13` 全过 → 重解析 → `ModelFingerprint`
//!   与**保存之后**的会话相等 → 接着跑。基准取在保存之后是因为范围标记按 `SPAN-08` 在保存时
//!   才物化：保存前 DOM 里那些标记的位置是暂定的，真相在 Span 索引里（不变式 3）
//!
//! 失败时打印 `(文档, 种子, 第几步, 操作)`，那三个数就能复现：
//! `RSWORD_RANDOM_ONLY=<stem> RSWORD_RANDOM_SEED=<seed> cargo test --test random_ops`。
//!
//! 规模由环境变量控制（PR CI 跑缺省的 100 条，nightly 的 `random.yml` 调到 1,000 条）：
//! `RSWORD_RANDOM_SEQUENCES`（缺省 100）、`RSWORD_RANDOM_STEPS`（缺省 100）。

mod common;

use common::Rng;
use common::fingerprint::fingerprint;
use rsword::edit::{
    BlockAt, BlockPos, EditContext, EditOp, EditSession, InlinePos, NewBlock, NewInline, NewRun,
    RevisionAuthor,
};
use rsword::model::{Block, Document, SectionOwner};
use rsword::package::PartId;
use rsword::semantic::props::Change;
use rsword::xml::{LocalName, NodeId, QName};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 一步随机操作。目标不存在（没有表格、没有修订…）时返回 `None`，那一步跳过。
fn random_op(s: &EditSession, rng: &mut Rng) -> Option<EditOp> {
    let doc = s.document();
    match rng.below(100) {
        0..=17 => inline_op(doc, rng),
        18..=27 => block_op(doc, rng),
        28..=37 => table_op(doc, rng),
        38..=45 => range_op(s, doc, rng),
        46..=53 => field_op(doc, rng),
        54..=61 => drawing_op(s, doc, rng),
        62..=69 => note_or_sdt_op(doc, rng),
        70..=79 => revision_op(doc, rng),
        80..=87 => section_op(doc, rng),
        _ => package_op(rng),
    }
}

/// 主 part 与页眉页脚 part 里全部可编辑段落：`(part, 节点, UTF-16 长度)`。
fn paragraphs(doc: &Document) -> Vec<(Option<PartId>, NodeId, u32)> {
    let mut out: Vec<(Option<PartId>, NodeId, u32)> =
        doc.paragraphs().map(|b| (None, b.node, b.utf16_len())).collect();
    // `hf_parts` 是 `HashMap`：迭代序每个进程都不一样，直接用会让"同一个种子复现同一串操作"
    // 失效（同一个种子在 debug 与 release 下跑出过不同的序列）
    let mut parts: Vec<PartId> = doc.hf_parts.keys().copied().collect();
    parts.sort_by_key(|p| p.0);
    for p in parts {
        let hf = &doc.hf_parts[&p];
        out.extend(hf.text_blocks().map(|b| (Some(p), b.node, b.utf16_len())));
    }
    out
}

fn inline_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let paras = paragraphs(doc);
    let &(part, para, len) = rng.pick(&paras)?;
    let a = rng.below(len as usize + 1) as u32;
    let b = rng.below(len as usize + 1) as u32;
    let (from, to) = (a.min(b), a.max(b));
    let pos = |o: u32| InlinePos { part, para, offset: rsword::edit::Utf16Offset(o) };
    Some(match rng.below(8) {
        0 | 1 => EditOp::InsertText {
            at: pos(a),
            text: ["字", "abc", "  ", "第 1 节", "\u{4e00}\u{4e8c}"][rng.below(5)].into(),
            props: None,
        },
        2 => EditOp::DeleteRange { from: pos(from), to: pos(to) },
        3 => EditOp::SetRunProps {
            from: pos(from),
            to: pos(to),
            patch: rsword::semantic::props::RunPropsPatch {
                bold: Change::Set(rng.chance(2)),
                ..Default::default()
            },
        },
        4 => EditOp::SplitParagraph { at: pos(a) },
        5 => EditOp::MergeWithNext { part, para },
        6 => EditOp::SetParaProps {
            part,
            para,
            patch: rsword::semantic::props::ParaPropsPatch {
                jc: Change::Set(rsword::semantic::props::Val::Value(
                    rsword::semantic::props::Jc::Center,
                )),
                ..Default::default()
            },
        },
        _ => EditOp::InsertAtom {
            at: pos(a),
            atom: rsword::edit::NewAtom::Break {
                kind: rsword::model::inline::BreakKind::TextWrapping,
                clear: None,
            },
        },
    })
}

fn block_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let blocks: Vec<NodeId> = doc.main.iter().map(Block::node).collect();
    let &node = rng.pick(&blocks)?;
    let at = BlockPos {
        part: None,
        at: match rng.below(3) {
            0 => BlockAt::Before(node),
            1 => BlockAt::After(node),
            _ => BlockAt::After(node),
        },
    };
    Some(match rng.below(6) {
        0 | 1 => EditOp::InsertBlock {
            at,
            block: NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("新段"))],
            },
        },
        2 => EditOp::InsertBlock {
            at,
            block: NewBlock::Table {
                rows: 1 + rng.below(3) as u32,
                cols: 1 + rng.below(3) as u32,
                widths: None,
                style: None,
                header: rng.chance(2),
            },
        },
        3 => EditOp::InsertBlock {
            at,
            block: NewBlock::Caption { label: "图".into(), text: "说明".into() },
        },
        4 => EditOp::DeleteBlock { part: None, node },
        _ => {
            let &to = rng.pick(&blocks)?;
            EditOp::MoveBlock {
                from: None,
                node,
                to: BlockPos { part: None, at: BlockAt::After(to) },
            }
        }
    })
}

fn table_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let tables: Vec<(NodeId, usize, usize)> = doc
        .tables()
        .map(|t| (t.node, t.rows.len(), t.rows.first().map_or(0, |r| r.cells.len())))
        .collect();
    let &(table, rows, cols) = rng.pick(&tables)?;
    Some(match rng.below(8) {
        0 => EditOp::InsertRow { table, at: rng.below(rows + 1) as u32, template: None },
        1 => EditOp::DeleteRow { table, at: rng.below(rows.max(1)) as u32 },
        2 => EditOp::InsertColumn { table, at: rng.below(cols + 1) as u32, width: 1200 },
        3 => EditOp::DeleteColumn { table, at: rng.below(cols.max(1)) as u32 },
        4 => EditOp::MergeCells {
            table,
            from: (rng.below(rows.max(1)) as u32, rng.below(cols.max(1)) as u32),
            to: (rng.below(rows.max(1)) as u32, rng.below(cols.max(1)) as u32),
        },
        5 => EditOp::SetTableProps {
            table,
            patch: rsword::semantic::props::TablePropsPatch {
                style: Change::Set("TableGrid".into()),
                ..Default::default()
            },
        },
        6 => {
            let t = doc.tables().find(|t| t.node == table)?;
            let row = t.rows.get(rng.below(t.rows.len().max(1)))?;
            EditOp::SetRowProps {
                row: row.node,
                patch: rsword::semantic::props::RowPropsPatch {
                    tbl_header: Change::Set(rng.chance(2)),
                    ..Default::default()
                },
            }
        }
        _ => {
            let t = doc.tables().find(|t| t.node == table)?;
            let row = t.rows.get(rng.below(t.rows.len().max(1)))?;
            let cell = row.cells.get(rng.below(row.cells.len().max(1)))?;
            EditOp::SetCellProps {
                cell: cell.node,
                patch: rsword::semantic::props::CellPropsPatch {
                    v_align: Change::Set(rsword::semantic::props::Val::Value(
                        rsword::semantic::props::VerticalJc::Center,
                    )),
                    ..Default::default()
                },
            }
        }
    })
}

/// 书签与批注（`SPAN-06/07` 的活儿）。
fn range_op(s: &EditSession, doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let paras: Vec<(Option<PartId>, NodeId, u32)> =
        paragraphs(doc).into_iter().filter(|&(p, ..)| p.is_none()).collect();
    let &(_, para, len) = rng.pick(&paras)?;
    let a = rng.below(len as usize + 1) as u32;
    let b = rng.below(len as usize + 1) as u32;
    let pos = |o: u32| InlinePos::new(para, o);
    let (from, to) = (pos(a.min(b)), pos(a.max(b)));
    Some(match rng.below(5) {
        0 => EditOp::AddBookmark { name: format!("bm{}", rng.below(1000)), from, to },
        1 => {
            let names: Vec<String> =
                s.document().comments.items.iter().map(|c| c.id.clone()).collect();
            EditOp::RemoveComment { id: rng.pick(&names)?.clone() }
        }
        2 => EditOp::AddComment {
            from,
            to,
            comment: rsword::edit::NewComment {
                author: "甲".into(),
                initials: Some("A".into()),
                date: Some("2026-01-01T00:00:00Z".into()),
                text: format!("批注 {}", rng.below(100)),
                parent_id: None,
                done: false,
            },
        },
        3 => {
            let names: Vec<String> =
                s.document().comments.items.iter().map(|c| c.id.clone()).collect();
            EditOp::SetCommentText {
                id: rng.pick(&names)?.clone(),
                text: "改过的批注".into(),
                done: Some(rng.chance(2)),
            }
        }
        _ => EditOp::RemoveBookmark { name: format!("bm{}", rng.below(1000)) },
    })
}

fn field_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let fields: Vec<_> = doc.fields.fields().iter().collect();
    let f = rng.pick(&fields)?;
    Some(match rng.below(5) {
        0 => EditOp::ToggleCheckbox { field: f.id },
        1 => EditOp::SetFormText { field: f.id, text: format!("表单 {}", rng.below(100)) },
        2 => EditOp::SetLinkTarget {
            link: rsword::edit::LinkRef::Field(f.id),
            target: rsword::edit::LinkDest::Url("https://example.invalid/".into()),
        },
        3 => EditOp::RegenerateBlockField { field: f.id, options: Default::default() },
        _ => EditOp::UpdateBlockField {
            field: f.id,
            blocks: vec![NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text("新结果"))],
            }],
        },
    })
}

fn drawing_op(s: &EditSession, _doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let dom = s.dom();
    let drawings: Vec<NodeId> = dom
        .descendants(dom.root())
        .filter(|&n| dom.node(n).dirty != rsword::xml::Dirty::Deleted)
        .filter(|&n| dom.is(n, QName::w(LocalName::Drawing)))
        .collect();
    let &drawing = rng.pick(&drawings)?;
    Some(match rng.below(4) {
        0 => EditOp::SetDrawingGeometry {
            drawing,
            geom: rsword::edit::DrawingGeometry {
                extent_emu: Some((100_000 + rng.below(2_000_000) as i64, 100_000)),
                rot_deg: rng.chance(3).then(|| Some(rng.below(360) as i64)),
                ..Default::default()
            },
        },
        1 => EditOp::SetDrawingZOrder { drawing, z: rng.below(64) as i64 },
        2 => EditOp::SetDrawingWrap {
            drawing,
            wrap: rng.chance(4).then_some(
                [
                    rsword::edit::ImageWrap::SquareLeft,
                    rsword::edit::ImageWrap::TightRight,
                    rsword::edit::ImageWrap::TopBottom,
                    rsword::edit::ImageWrap::Behind,
                ][rng.below(4)],
            ),
            pos: None,
            z_order: None,
        },
        _ => {
            let shapes: Vec<NodeId> = dom
                .descendants(drawing)
                .filter(|&n| dom.is(n, QName::new(rsword::xml::NsId::Wps, LocalName::Wsp)))
                .collect();
            EditOp::SetShapeStyle {
                shape: *rng.pick(&shapes)?,
                fill: Some(rng.chance(2).then(|| "FF8800".to_string())),
                outline: None,
            }
        }
    })
}

fn note_or_sdt_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let sdts: Vec<NodeId> = doc.blocks().filter_map(|b| b.sdt().map(|i| i.node)).collect();
    Some(match rng.below(4) {
        0 | 1 => {
            let endnote = rng.chance(2);
            let notes = if endnote { &doc.endnotes } else { &doc.footnotes };
            let ids: Vec<String> = notes.items.iter().map(|n| n.id.clone()).collect();
            let id = rng.pick(&ids)?.clone();
            if rng.chance(3) {
                EditOp::RemoveNote { endnote, id }
            } else {
                EditOp::SetNoteContent {
                    endnote,
                    id,
                    content: vec![vec![NewRun::text("注释正文")]],
                }
            }
        }
        2 => EditOp::RemoveSdtShell { sdt: *rng.pick(&sdts)? },
        _ => EditOp::SetSdtContent {
            sdt: *rng.pick(&sdts)?,
            inlines: vec![NewInline::Run(NewRun::text("控件内容"))],
        },
    })
}

fn revision_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let ids: Vec<_> = doc.revisions.entries().iter().map(|e| e.id).collect();
    Some(match rng.below(6) {
        0 | 1 => EditOp::AcceptRevision { rev: *rng.pick(&ids)? },
        2 | 3 => EditOp::RejectRevision { rev: *rng.pick(&ids)? },
        4 => EditOp::AcceptAll { author: rng.chance(2).then(|| "甲".to_string()) },
        _ => EditOp::RejectAll { author: rng.chance(2).then(|| "乙".to_string()) },
    })
}

fn section_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let sects: Vec<NodeId> = doc.sections.iter().filter_map(|s| s.node).collect();
    let paras: Vec<NodeId> = doc.main.iter().map(Block::node).collect();
    Some(match rng.below(4) {
        0 => EditOp::InsertSectionBreak {
            after: *rng.pick(&paras)?,
            kind: [
                rsword::semantic::props::SectType::NextPage,
                rsword::semantic::props::SectType::Continuous,
                rsword::semantic::props::SectType::EvenPage,
            ][rng.below(3)],
        },
        1 => {
            // body 级的 `sectPr` 删不得，随机挑到就让它自己拒
            EditOp::DeleteSectionBreak { sect: *rng.pick(&sects)? }
        }
        2 => EditOp::SetSectionProps {
            sect: doc.sections.last().filter(|s| s.owner == SectionOwner::Body)?.node?,
            patch: rsword::semantic::props::SectionPropsPatch {
                title_pg: Change::Set(rng.chance(2)),
                ..Default::default()
            },
        },
        _ => EditOp::SetWatermark {
            sect: doc.sections.last().filter(|s| s.owner == SectionOwner::Body)?.node?,
            text: rng.chance(2).then(|| "机密".to_string()),
        },
    })
}

fn package_op(rng: &mut Rng) -> Option<EditOp> {
    Some(match rng.below(2) {
        0 => EditOp::SetPageColor { color: rng.chance(2).then(|| "EEF3FF".to_string()) },
        _ => EditOp::SetDocumentSettings {
            patch: rsword::semantic::props::SettingsPatch {
                even_and_odd_headers: Change::Set(rng.chance(2)),
                ..Default::default()
            },
        },
    })
}

/// `MOD-13`：增量刷新的投影 == 从 DOM 整体重建。
///
/// 块投影整个 `Debug` 打出来有几万字，看不清；这里只报**第一处**不同的块，两边各截一段。
fn refresh_matches_rebuild(s: &mut EditSession) -> Result<(), String> {
    let refreshed = s.document().clone();
    let rebuilt = Document::rebuild(s.package_mut()).map_err(|e| format!("rebuild 失败: {e}"))?;
    let n = refreshed.main.len().max(rebuilt.main.len());
    for i in 0..n {
        let (a, b) = (refreshed.main.get(i), rebuilt.main.get(i));
        // `SpanId` 是**会话内**的身份（`SPAN-04`）：增量索引保号、整体重建重新编号，
        // 两边不该按数字比。比之前先把它换成范围自己的名字
        let (x, y) = (labelled(a, &refreshed), labelled(b, &rebuilt));
        if x == y {
            continue;
        }
        let (dx, dy) =
            common::fingerprint::diff_str(&x, &y).unwrap_or_else(|| (x.clone(), y.clone()));
        return Err(format!(
            "块投影与重建不一致（第 {i} 块，节点 {:?}）\n  刷新: {dx}\n  重建: {dy}",
            a.or(b).map(Block::node)
        ));
    }
    if refreshed.sections.len() != rebuilt.sections.len() {
        return Err(format!(
            "节数与重建不一致：刷新 {} 重建 {}",
            refreshed.sections.len(),
            rebuilt.sections.len()
        ));
    }
    for (i, (a, b)) in refreshed.sections.iter().zip(&rebuilt.sections).enumerate() {
        if a != b {
            let (x, y) = (format!("{a:?}"), format!("{b:?}"));
            let (dx, dy) =
                common::fingerprint::diff_str(&x, &y).unwrap_or_else(|| (x.clone(), y.clone()));
            return Err(format!("第 {i} 节的投影与重建不一致\n  刷新: {dx}\n  重建: {dy}"));
        }
    }
    Ok(())
}

/// 一个块的 `Debug`，里面的 `SpanId(n)` 换成那个范围的名字（书签名 / 批注 id / …）。
fn labelled(b: Option<&Block>, doc: &Document) -> String {
    let mut s = format!("{b:?}");
    if !s.contains("SpanId(") {
        return s;
    }
    // 倒着替换，免得 `SpanId(1)` 撞上 `SpanId(11)`
    let mut ids: Vec<(u32, String)> = doc
        .spans
        .live()
        .map(|sp| {
            let label = sp
                .kind
                .bookmark_name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("{:?}", sp.kind));
            (sp.id.0, label)
        })
        .collect();
    ids.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
    for (n, label) in ids {
        s = s.replace(&format!("SpanId({n})"), &format!("Span[{label}]"));
    }
    s
}

/// 主 part 的字段结构缺陷计数（`FLD-13`）：某个代码变多了就是这一步弄坏的。
fn field_defects(s: &EditSession) -> std::collections::HashMap<rsword::DiagCode, usize> {
    let Ok(dom) = s.dom_in(None) else { return Default::default() };
    rsword::span::field::FieldIndex::build(dom).defect_counts()
}

/// 会话里有没有引擎不变式被破坏的诊断。
fn invariants_clean(s: &EditSession) -> Result<(), String> {
    let bad: Vec<String> = s
        .diagnostics()
        .iter()
        .filter(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation)
        .map(|d| format!("{:?} {}", d.code, d.message))
        .collect();
    if bad.is_empty() { Ok(()) } else { Err(format!("引擎不变式被破坏 {bad:?}")) }
}

/// 主 part 里同类范围重号的对数（`EDIT-06`：`w:id` 在 part 内唯一）。
fn dup_span_ids(s: &mut EditSession) -> usize {
    let part = s.main_part();
    let Ok(idx) = s.spans_of(part) else { return 0 };
    let mut seen: std::collections::HashSet<(rsword::span::RangeClass, String)> =
        Default::default();
    let mut dups = 0;
    for sp in idx.live() {
        if sp.pair_id().is_empty() {
            continue;
        }
        if !seen.insert((sp.class(), sp.pair_id().to_string())) {
            dups += 1;
        }
    }
    dups
}

/// 一步之后的全部不变式。`Err` 里是给人看的说明。
fn check_step(
    s: &mut EditSession,
    prev: &mut std::collections::HashMap<rsword::DiagCode, usize>,
    dups: &mut usize,
) -> Result<(), String> {
    // `EDIT-06`：范围的 `w:id` 在 part 内唯一。输入本来就重的不算，这一步**多**重了才算
    let now = dup_span_ids(s);
    let was = *dups;
    *dups = now;
    if now > was {
        return Err(format!("这一步让范围重号多了 {} 对（EDIT-06）", now - was));
    }
    // `FLD-13`：字段结构的缺陷只许比上一步少，不许多
    let now = field_defects(s);
    let worse: Vec<String> = now
        .iter()
        .filter(|(code, n)| **n > prev.get(code).copied().unwrap_or(0))
        .map(|(code, n)| format!("{code:?} {} → {n}", prev.get(code).copied().unwrap_or(0)))
        .collect();
    *prev = now;
    if !worse.is_empty() {
        return Err(format!("这一步弄坏了字段结构（FLD-13）：{}", worse.join("，")));
    }
    refresh_matches_rebuild(s)?;
    invariants_clean(s)
}

fn ctx_for(rng: &mut Rng) -> EditContext {
    EditContext::default()
        .with_track_changes(match rng.below(3) {
            0 => None,
            1 => Some(RevisionAuthor {
                author: "甲".into(),
                date: Some("2026-01-01T00:00:00Z".into()),
            }),
            _ => Some(RevisionAuthor {
                author: "乙".into(),
                date: Some("2026-02-02T00:00:00Z".into()),
            }),
        })
        .with_mark_updated_fields_dirty(rng.chance(2))
}

/// 只为报错信息好读：操作的名字。
fn op_name(op: &EditOp) -> String {
    let s = format!("{op:?}");
    s.split([' ', '{', '(']).next().unwrap_or("?").to_string()
}

/// 每几步保存一次并重解析。
const SAVE_EVERY: usize = 20;

/// 引擎 panic 也算失败（不变式 4：病态输入只能局部降级，不能炸）。捕获之后当成失败说明，
/// 最小化照跑。默认的 panic 钩子先摘掉：复放会故意撞同一个 panic 几十次，日志刷不完。
fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    out.map_err(|e| {
        let msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "（没有消息）".into());
        format!("引擎 panic：{msg}")
    })
}

/// 把一串**具体**操作放到一份文档上重跑一遍，返回第一处失败的说明（`None` = 全过）。
///
/// 最小化时会照这条路复放子集：被删那一步造出来的节点没了，后面引用它的操作会被
/// `EDIT-05` 拒掉——那正是我们想要的，拒绝不改变状态。
fn replay(bytes: &[u8], ops: &[(EditOp, EditContext)]) -> Option<String> {
    // `RSWORD_RANDOM_TRACE=1`：复放时逐步打印结果，看某一步到底生效了还是被拒了
    let trace = std::env::var("RSWORD_RANDOM_TRACE").is_ok();
    let mut s = EditSession::open(bytes).ok()?;
    let mut defects = field_defects(&s);
    let mut dups = dup_span_ids(&mut s);
    for (i, (op, ctx)) in ops.iter().enumerate() {
        let head = format!("第 {i} 步 {}", op_name(op));
        match guard(|| s.apply(op.clone(), ctx)) {
            Err(why) => return Some(format!("{head}: {why}")),
            Ok(Ok(_)) => {
                if trace {
                    eprintln!("{head}: 生效");
                }
            }
            Ok(Err(rsword::Error::Edit { code, message })) => {
                if trace {
                    eprintln!("{head}: 拒绝 {code:?} {message}");
                }
                continue;
            }
            Ok(Err(e)) => return Some(format!("{head}: 非编辑错误 {e}")),
        }
        match guard(|| check_step(&mut s, &mut defects, &mut dups)) {
            Err(why) | Ok(Err(why)) => return Some(format!("{head}: {why}")),
            Ok(Ok(())) => {}
        }
        // 最后一步也保存一次：不这么做，删掉任何一步都会把保存点挪走，最小化就寸步难行
        if (i + 1).is_multiple_of(SAVE_EVERY) || i + 1 == ops.len() {
            let saved = match s.save() {
                Ok(b) => b,
                Err(e) => return Some(format!("第 {i} 步 {}: 保存失败 {e}", op_name(op))),
            };
            // `save::enforce` 只在调试构建里把引擎不变式当错误；发布构建只记诊断。
            // 这里自己查一遍，两种构建才会在同一处失败
            if let Err(why) = invariants_clean(&s) {
                return Some(format!("第 {i} 步 {}: 保存之后 {why}", op_name(op)));
            }
            // 基准取在**保存之后**：`SPAN-08` 的范围标记在保存时才物化，保存前 DOM 里
            // 那些标记的位置是暂定的（真相在 Span 索引里，不变式 3）
            let before = fingerprint(&s);
            let re = match EditSession::open(&saved) {
                Ok(x) => x,
                Err(e) => {
                    return Some(format!("第 {i} 步 {}: 保存出来的包打不开 {e}", op_name(op)));
                }
            };
            let after = fingerprint(&re);
            if let Some((view, x, y)) = after.diff(&before) {
                return Some(format!(
                    "第 {i} 步 {}: 保存往返后内容变了（视图 {view}）\n  左: {x}\n  右: {y}",
                    op_name(op)
                ));
            }
            s = re;
            defects = field_defects(&s);
            dups = dup_span_ids(&mut s);
        }
    }
    None
}

/// 失败说明的"指纹"：去掉"第几步 什么操作"的前缀，只留原因。最小化时只认**同一种**失败，
/// 不然会收敛到另一个 bug 上去。
fn signature(msg: &str) -> String {
    msg.split_once(": ").map_or(msg, |(_, rest)| rest).chars().take(60).collect()
}

/// 最小化：先二分找最短的失败前缀，再贪心地逐条删掉删了还失败的那些步。
fn minimize(
    bytes: &[u8],
    ops: Vec<(EditOp, EditContext)>,
    want: &str,
) -> Vec<(EditOp, EditContext)> {
    let same = |x: Option<String>| x.is_some_and(|m| signature(&m) == want);
    // ① 最短失败前缀
    let (mut lo, mut hi) = (1usize, ops.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if same(replay(bytes, &ops[..mid])) { hi = mid } else { lo = mid + 1 }
    }
    let mut kept: Vec<(EditOp, EditContext)> = ops[..lo.min(ops.len())].to_vec();
    // ② 逐条尝试删，反复扫到扫不动为止（有些步要等别的步先删掉才删得动）
    loop {
        let before = kept.len();
        let mut i = kept.len();
        while i > 0 {
            i -= 1;
            if i >= kept.len() {
                continue;
            }
            let mut trial = kept.clone();
            trial.remove(i);
            if same(replay(bytes, &trial)) {
                kept = trial;
            }
        }
        if kept.len() == before {
            return kept;
        }
    }
}

struct Stats {
    applied: usize,
    refused: usize,
    saves: usize,
    minimized: usize,
}

/// 一条序列。失败时最小化再 panic，消息里带 `(文档, 种子)` 与最小序列。
fn run_sequence(path: &std::path::Path, seed: u64, steps: usize, st: &mut Stats) {
    let Ok(bytes) = std::fs::read(path) else { return };
    let Ok(mut s) = EditSession::open(&bytes) else { return };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut defects = field_defects(&s);
    let mut dups = dup_span_ids(&mut s);
    let mut trail: Vec<(EditOp, EditContext)> = Vec::new();
    for step in 0..steps {
        let Some(op) = random_op(&s, &mut rng) else { continue };
        let ctx = ctx_for(&mut rng);
        trail.push((op.clone(), ctx.clone()));
        let head = format!(
            "{stem} 种子 {seed} 第 {step} 步（记录第 {}）{}",
            trail.len() - 1,
            op_name(&op)
        );
        match guard(|| s.apply(op, &ctx)) {
            Err(why) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: {why}")),
            Ok(Ok(_)) => st.applied += 1,
            // 编辑层拒绝是合法结果：`EDIT-05` 保证 DOM / Span / Model 一点没动
            Ok(Err(rsword::Error::Edit { .. })) => {
                st.refused += 1;
                continue;
            }
            Ok(Err(e)) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: 非编辑错误 {e}")),
        }
        match guard(|| check_step(&mut s, &mut defects, &mut dups)) {
            Err(why) | Ok(Err(why)) => {
                fail(&bytes, &stem, seed, trail, st, &format!("{head}: {why}"))
            }
            Ok(Ok(())) => {}
        }
        // 保存点按**记录下来的步数**算，不按循环计数——复放时下标才对得上
        if trail.len().is_multiple_of(SAVE_EVERY) {
            let saved = match s.save() {
                Ok(b) => b,
                Err(e) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: 保存失败 {e}")),
            };
            if let Err(why) = invariants_clean(&s) {
                fail(&bytes, &stem, seed, trail, st, &format!("{head}: 保存之后 {why}"));
            }
            // 基准取在保存之后（见 `replay` 的同一处注释）
            let before = fingerprint(&s);
            let re = match EditSession::open(&saved) {
                Ok(x) => x,
                Err(e) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: 重开失败 {e}")),
            };
            let after = fingerprint(&re);
            if let Some((view, x, y)) = after.diff(&before) {
                // `RSWORD_RANDOM_DUMP=<目录>`：把这一步存出来的包落盘，好拿去看 XML
                if let Ok(dir) = std::env::var("RSWORD_RANDOM_DUMP") {
                    let _ = std::fs::write(std::path::Path::new(&dir).join("saved.docx"), &saved);
                }
                fail(
                    &bytes,
                    &stem,
                    seed,
                    trail,
                    st,
                    &format!("{head}: 保存往返后内容变了（视图 {view}）\n  左: {x}\n  右: {y}"),
                );
            }
            st.saves += 1;
            s = re;
            defects = field_defects(&s);
            dups = dup_span_ids(&mut s);
        }
    }
}

/// 最小化之后 panic。
fn fail(
    bytes: &[u8],
    stem: &str,
    seed: u64,
    trail: Vec<(EditOp, EditContext)>,
    st: &mut Stats,
    why: &str,
) -> ! {
    let n = trail.len();
    let small = minimize(bytes, trail, &signature(why));
    st.minimized += 1;
    let list: Vec<String> = small
        .iter()
        .enumerate()
        .map(|(i, (op, ctx))| {
            let track = ctx.track_changes.as_ref().map_or("不追踪", |a| a.author.as_str());
            // 属性补丁的 `Debug` 有几千字，截一段就够定位
            let mut d = format!("{op:?}");
            if d.chars().count() > 200 {
                d = d.chars().take(200).collect::<String>() + " …";
            }
            format!("  {i}. [{track}] {d}")
        })
        .collect();
    panic!(
        "{why}\n\n最小化：{n} 步 → {} 步（`RSWORD_RANDOM_ONLY={stem} RSWORD_RANDOM_SEED={seed}` 复现）\n{}",
        small.len(),
        list.join("\n")
    );
}

#[test]
fn test_07_random_edit_sequences() {
    let sequences = env_usize("RSWORD_RANDOM_SEQUENCES", 100);
    let steps = env_usize("RSWORD_RANDOM_STEPS", 100);
    let only = std::env::var("RSWORD_RANDOM_ONLY").ok();
    let docs: Vec<_> = common::docx_paths("synthetic")
        .into_iter()
        .filter(|p| {
            only.as_deref()
                .is_none_or(|o| p.file_stem().unwrap_or_default().to_string_lossy().contains(o))
        })
        .collect();
    assert!(!docs.is_empty(), "没有语料可跑");
    let mut st = Stats { applied: 0, refused: 0, saves: 0, minimized: 0 };
    let base_seed: u64 = std::env::var("RSWORD_RANDOM_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5DEE_CE66_D3B1_1EAD);
    // 语料轮转 × 种子轮转：`sequences` 条互不相同的 (文档, 种子)
    for i in 0..sequences {
        let path = &docs[i % docs.len()];
        let seed = base_seed.wrapping_add((i / docs.len()) as u64).wrapping_mul(i as u64 + 1);
        run_sequence(path, seed, steps, &mut st);
    }
    eprintln!(
        "TEST-07: {sequences} 条 × {steps} 步 → {} 次生效、{} 次被拒、{} 次保存往返",
        st.applied, st.refused, st.saves
    );
    assert!(st.applied > sequences * 10, "有效操作太少：{}", st.applied);
    assert!(st.saves > 0, "一次保存往返都没跑到");
}
