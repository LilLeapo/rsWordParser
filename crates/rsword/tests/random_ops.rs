//! `TEST-07`（`spec/18` 门 5）：随机编辑序列。
//!
//! 每条序列 = 一份语料 × 一个种子 × N 步。每一步随机挑一个 `EditOp`，编码并记录 JSON，
//! 经 `edit_op_from_json` / `SessionTable::apply` 执行；随机开关 `track_changes`
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
use rsword::bind::native::edit::edit_diagnostics_json;
use rsword::bind::native::{
    DocumentOpts, SessionTable, document_json, edit_op_from_json, edit_op_to_json,
};
use rsword::edit::{
    BlockAt, BlockPos, EditContext, EditOp, EditSession, InlinePos, NewBlock, NewInline, NewRun,
    RevisionAuthor,
};
use rsword::model::{Block, Document};
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
                kind: rsword::model::BreakKind::TextWrapping,
                clear: None,
            },
        },
    })
}

fn block_op(doc: &Document, rng: &mut Rng) -> Option<EditOp> {
    let blocks: Vec<NodeId> = doc.main.iter().map(rsword::model::Block::node).collect();
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
    let paras: Vec<NodeId> = doc.main.iter().map(rsword::model::Block::node).collect();
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
            sect: doc
                .sections
                .last()
                .filter(|s| s.owner == rsword::model::SectionOwner::Body)?
                .node?,
            patch: rsword::semantic::props::SectionPropsPatch {
                title_pg: Change::Set(rng.chance(2)),
                ..Default::default()
            },
        },
        _ => EditOp::SetWatermark {
            sect: doc
                .sections
                .last()
                .filter(|s| s.owner == rsword::model::SectionOwner::Body)?
                .node?,
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
fn refresh_matches_rebuild(s: &EditSession) -> Result<(), String> {
    let refreshed = s.document().clone();
    // 独立 oracle 的惰性解析等副作用不得进入活会话。
    let rebuilt = rsword::model::Document::rebuild(&mut s.package().clone())
        .map_err(|e| format!("rebuild 失败: {e}"))?;
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
        .chain(s.package().diagnostics())
        .filter(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation)
        .map(|d| format!("{:?} {}", d.code, d.message))
        .collect();
    if bad.is_empty() { Ok(()) } else { Err(format!("引擎不变式被破坏 {bad:?}")) }
}

/// 主 part 里同类范围重号的对数（`EDIT-06`：`w:id` 在 part 内唯一）。
fn dup_span_ids(s: &EditSession) -> usize {
    let part = s.main_part();
    // 查询可能建立索引并记诊断；oracle 不得推进被比较的原生会话。
    let mut scratch = s.clone();
    let Ok(idx) = scratch.spans_of(part) else { return 0 };
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
    s: &EditSession,
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
fn op_name(op: &str) -> String {
    serde_json::from_str::<serde_json::Value>(op).unwrap()["op"].as_str().unwrap().to_owned()
}

/// 每几步保存一次并重解析。
const SAVE_EVERY: usize = 20;

/// 协议会话执行记录下来的 JSON；原生会话只作为增量模型、Span 与两视图的 oracle。
/// 保存后两边同时重开，保留原 TEST-07 的 arena/保存点与最小化语义。
struct ProtocolSequence {
    engine: EditSession,
    table: SessionTable,
    id: String,
}
impl std::ops::Deref for ProtocolSequence {
    type Target = EditSession;
    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}
impl ProtocolSequence {
    fn open(bytes: &[u8]) -> rsword::error::Result<Self> {
        Ok(Self::reopened(EditSession::open(bytes)?, bytes))
    }
    fn reopened(engine: EditSession, bytes: &[u8]) -> Self {
        let mut table = SessionTable::default();
        let id = table.open(bytes, None).expect("协议与原生打开一致");
        Self { engine, table, id }
    }
    fn observed(&mut self) -> (String, String, Result<Vec<u8>, String>) {
        (
            self.table.document(&self.id, None).unwrap(),
            self.table.diagnostics(&self.id).unwrap(),
            self.table.save(&self.id, None).map_err(|e| e.to_string()),
        )
    }
    fn apply_json(&mut self, json: &str, ctx: &EditContext) -> rsword::error::Result<()> {
        let before = self.observed();
        // 本生成器不生成 XML 逃生口或原始属性；解码不得产生新的驻留名。
        // 将来扩展生成器时此断言会阻止把新 interner 索引带入另一个 Dom。
        let mut scratch = self.dom().clone();
        let scratch_before = format!("{scratch:?}");
        let op = edit_op_from_json(json, &mut scratch).expect("生成的 JSON 必须能解码");
        assert_eq!(format!("{scratch:?}"), scratch_before, "生成器引入了需专属 Dom 的 XML");
        let context = serde_json::to_string(ctx).unwrap();
        let protocol = self.table.apply(&self.id, json, Some(&context));
        let native = self.engine.apply(op, ctx);
        assert_eq!(
            protocol.as_ref().err().map(|e| e.code.as_str()),
            native.as_ref().err().map(|e| match e {
                rsword::Error::Edit { code, .. } => code.as_str(),
                _ => panic!("非编辑错误 {e}"),
            }),
            "协议/原生错误码不同: {json}"
        );
        assert_eq!(
            protocol.as_ref().err().map(|e| e.message.clone()),
            native.as_ref().err().map(|e| e.to_string()),
            "协议/原生错误说明不同: {json}"
        );
        if protocol.is_err() {
            assert_eq!(self.observed(), before, "协议 Err 改变状态");
        }
        let mut expected =
            document_json(self.package(), self.document(), DocumentOpts { display: false }).0;
        expected["totalBlocks"] = self.document().main.len().into();
        expected["truncated"] = false.into();
        assert_eq!(
            self.table.document(&self.id, None).unwrap(),
            expected.to_string(),
            "协议/原生模型不同: {json}"
        );
        assert_eq!(
            self.table.diagnostics(&self.id).unwrap(),
            edit_diagnostics_json(&self.engine).to_string(),
            "协议/原生诊断不同: {json}"
        );
        native.map(|_| ())
    }
    fn save(&mut self) -> rsword::error::Result<Vec<u8>> {
        let protocol = self.table.save(&self.id, None);
        let native = self.engine.save();
        assert_eq!(
            protocol.as_ref().err().map(|e| e.message.clone()),
            native.as_ref().err().map(|e| e.to_string()),
            "协议/原生保存错误不同"
        );
        if let (Ok(a), Ok(b)) = (&protocol, &native) {
            assert_eq!(a, b, "协议/原生保存字节不同");
        }
        native
    }
}

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
fn replay(bytes: &[u8], ops: &[(String, EditContext)]) -> Option<String> {
    // `RSWORD_RANDOM_TRACE=1`：复放时逐步打印结果，看某一步到底生效了还是被拒了
    let trace = std::env::var("RSWORD_RANDOM_TRACE").is_ok();
    let mut s = ProtocolSequence::open(bytes).ok()?;
    let mut defects = field_defects(&s);
    let mut dups = dup_span_ids(&s);
    for (i, (op, ctx)) in ops.iter().enumerate() {
        let head = format!("第 {i} 步 {}", op_name(op));
        match guard(|| s.apply_json(op, ctx)) {
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
            }
            Ok(Err(e)) => return Some(format!("{head}: 非编辑错误 {e}")),
        }
        match guard(|| check_step(&s, &mut defects, &mut dups)) {
            Err(why) | Ok(Err(why)) => return Some(format!("{head}: {why}")),
            Ok(Ok(())) => {}
        }
        // 最后一步也保存一次：不这么做，删掉任何一步都会把保存点挪走，最小化就寸步难行
        if (i + 1).is_multiple_of(SAVE_EVERY) || i + 1 == ops.len() {
            let saved = match guard(|| s.save()).and_then(|r| r.map_err(|e| e.to_string())) {
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
            s = ProtocolSequence::reopened(re, &saved);
            defects = field_defects(&s);
            dups = dup_span_ids(&s);
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
    ops: Vec<(String, EditContext)>,
    want: &str,
) -> Vec<(String, EditContext)> {
    let same = |x: Option<String>| x.is_some_and(|m| signature(&m) == want);
    // ① 最短失败前缀
    let (mut lo, mut hi) = (1usize, ops.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if same(replay(bytes, &ops[..mid])) { hi = mid } else { lo = mid + 1 }
    }
    let mut kept: Vec<(String, EditContext)> = ops[..lo.min(ops.len())].to_vec();
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
    if std::env::var_os("RSWORD_RANDOM_TRACE").is_some() {
        eprintln!("sequence {} seed={seed}", path.display());
    }
    let bytes = std::fs::read(path).expect("随机序列语料必须可读");
    let mut s = ProtocolSequence::open(&bytes).expect("synthetic 必须可打开，不得漏跑序列");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut defects = field_defects(&s);
    let mut dups = dup_span_ids(&s);
    let mut trail: Vec<(String, EditContext)> = Vec::new();
    for step in 0..steps {
        let Some(op) = random_op(&s, &mut rng) else { continue };
        let op = edit_op_to_json(&op, s.dom()).expect("生成器的结构化操作必须可表示，禁止跳过");
        let ctx = ctx_for(&mut rng);
        trail.push((op.clone(), ctx.clone()));
        let head = format!(
            "{stem} 种子 {seed} 第 {step} 步（记录第 {}）{}",
            trail.len() - 1,
            op_name(&op)
        );
        match guard(|| s.apply_json(&op, &ctx)) {
            Err(why) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: {why}")),
            Ok(Ok(_)) => st.applied += 1,
            // 编辑层拒绝是合法结果：`EDIT-05` 保证 DOM / Span / Model 一点没动
            Ok(Err(rsword::Error::Edit { .. })) => {
                st.refused += 1;
            }
            Ok(Err(e)) => fail(&bytes, &stem, seed, trail, st, &format!("{head}: 非编辑错误 {e}")),
        }
        match guard(|| check_step(&s, &mut defects, &mut dups)) {
            Err(why) | Ok(Err(why)) => {
                fail(&bytes, &stem, seed, trail, st, &format!("{head}: {why}"))
            }
            Ok(Ok(())) => {}
        }
        // 保存点按**记录下来的步数**算，不按循环计数——复放时下标才对得上
        if trail.len().is_multiple_of(SAVE_EVERY) {
            let saved = match guard(|| s.save()).and_then(|r| r.map_err(|e| e.to_string())) {
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
            s = ProtocolSequence::reopened(re, &saved);
            defects = field_defects(&s);
            dups = dup_span_ids(&s);
        }
    }
}

/// 最小化之后 panic。
fn fail(
    bytes: &[u8],
    stem: &str,
    seed: u64,
    trail: Vec<(String, EditContext)>,
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
            format!("  {i}. [{track}] {op} context={}", serde_json::to_string(ctx).unwrap())
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

#[test]
fn test_07_settings_projection_survives_rejected_edit() {
    let bytes = common::docx_with_parts(
        "<w:p/>",
        &[(
            "word/settings.xml",
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:evenAndOddHeaders/></w:settings>"#,
        )],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    s.apply(
        EditOp::SetDocumentSettings {
            patch: rsword::semantic::props::SettingsPatch {
                even_and_odd_headers: Change::Set(false),
                ..Default::default()
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(
        s.document().settings.as_ref().unwrap().even_and_odd_headers,
        Some(false),
        "成功编辑必须刷新 settings 投影"
    );
    let before =
        document_json(s.package(), s.document(), DocumentOpts { display: false }).to_string();
    assert!(
        s.apply(EditOp::DeleteSectionBreak { sect: NodeId(u32::MAX) }, &EditContext::default())
            .is_err()
    );
    assert_eq!(
        document_json(s.package(), s.document(), DocumentOpts { display: false }).to_string(),
        before
    );
}

#[test]
fn test_07_refused_edit_preserves_warning_projection() {
    let bytes = common::docx_with_body(
        r#"<w:p><w:r><w:t>a</w:t><w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent><w:tbl><w:tr/></w:tbl></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let para = s.document().paragraphs().next().unwrap().node;
    s.apply(
        EditOp::InsertText { at: InlinePos::new(para, 0), text: "x".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    let before =
        document_json(s.package(), s.document(), DocumentOpts { display: false }).to_string();
    assert!(!s.document().warnings.is_empty(), "样例必须包含模型警告");
    assert!(
        s.apply(EditOp::DeleteSectionBreak { sect: NodeId(u32::MAX) }, &EditContext::default())
            .is_err()
    );
    assert_eq!(
        document_json(s.package(), s.document(), DocumentOpts { display: false }).to_string(),
        before
    );
}

#[test]
fn test_07_refused_comment_does_not_create_parts() {
    let bytes = include_bytes!("../../../corpus/synthetic/decorated-paragraphs__001.docx");
    let mut s = EditSession::open(bytes).unwrap();
    // 原 [7,17) 是合法范围，只因拆文本后空白被裁掉而误拒绝；9.5 修复后另测它必须成功。
    let op = edit_op_from_json(r#"{"op":"addComment","from":{"para":2,"offset":7},"to":{"para":2,"offset":4294967295},"comment":{"author":"A","text":"rejected","done":false}}"#, &mut s.dom().clone()).unwrap();
    let error = s.apply(op, &EditContext::default()).unwrap_err();
    eprintln!("refused comment: {error}");
    assert_eq!(s.save().unwrap(), bytes.as_slice(), "拒绝批注不得留下新 part 或关系");
}

#[test]
fn test_07_valid_comment_preserves_boundary_whitespace() {
    let bytes = include_bytes!("../../../corpus/synthetic/decorated-paragraphs__001.docx");
    let mut s = EditSession::open(bytes).unwrap();
    let before = s.document().paragraphs().map(|p| p.text()).collect::<Vec<_>>();
    let op=edit_op_from_json(r#"{"op":"addComment","from":{"para":2,"offset":7},"to":{"para":2,"offset":17},"comment":{"author":"A","text":"valid","done":false}}"#,&mut s.dom().clone()).unwrap();
    s.apply(op, &EditContext::default()).expect("合法批注不能因拆分后丢空白而被拒绝");
    assert_eq!(s.document().paragraphs().map(|p| p.text()).collect::<Vec<_>>(), before);
    let saved = s.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    assert_eq!(reopened.document().paragraphs().map(|p| p.text()).collect::<Vec<_>>(), before);
}

#[test]
fn test_07_rejected_edit_does_not_consume_revision_ids() {
    let bytes = include_bytes!("../../../corpus/synthetic/smartart-ole__017.docx");
    let tracked = |author: &str| {
        EditContext::default().with_track_changes(Some(RevisionAuthor {
            author: author.into(),
            date: Some("2026-01-01T00:00:00Z".into()),
        }))
    };
    let ops = vec![
        (r#"{"op":"setRunProps","from":{"para":2,"offset":0},"to":{"para":2,"offset":9},"patch":{"bold":true}}"#.into(), tracked("乙")),
        (r#"{"op":"splitParagraph","at":{"para":2,"offset":5}}"#.into(), EditContext::default()),
        (r#"{"op":"setRunProps","from":{"para":2,"offset":8},"to":{"para":2,"offset":9},"patch":{"bold":true}}"#.into(), tracked("甲")),
    ];
    assert_eq!(replay(bytes, &ops), None);
}

#[test]
fn test_07_fingerprint_deep_wrappers_keep_both_views() {
    let body = "<w:p><w:r><w:t>deep</w:t></w:r></w:p>";
    let plain = EditSession::open(&common::docx_with_body(body)).unwrap();
    let wrapped = format!("{}{body}{}", "<w:smartTag>".repeat(5000), "</w:smartTag>".repeat(5000));
    let deep = EditSession::open(&common::docx_with_body(&wrapped)).unwrap();
    assert_eq!(fingerprint(&deep), fingerprint(&plain));
}

/// TEST-07：seed 10201016167453558198，40 → 26 步；保留第 20 步保存重开。
#[test]
fn test_07_reject_grid_keeps_later_untracked_column() {
    let bytes = include_bytes!("../../../corpus/synthetic/m6-chart__070.docx");
    let ops: Vec<(serde_json::Value, EditContext)> = serde_json::from_str(include_str!(
        "../../../fixtures/regressions/table-revision-10201016167453558198.json"
    ))
    .unwrap();
    assert_eq!(ops.len(), 26);
    let mut s = ProtocolSequence::open(bytes).unwrap();
    for (i, (op, ctx)) in ops.iter().enumerate() {
        let result = s.apply_json(&op.to_string(), ctx);
        if i == 25 {
            result.expect("拒绝追踪插列必须成功，不能靠新增拒绝过门");
            let table = s.document().tables().next().unwrap();
            assert_eq!(table.grid.len(), 4);
            assert_eq!(
                table
                    .rows
                    .iter()
                    .map(|r| r.cells.iter().map(|c| c.grid_span()).sum::<u32>())
                    .collect::<Vec<_>>(),
                [4, 4, 4]
            );
        }
        if (i + 1).is_multiple_of(SAVE_EVERY) {
            s = ProtocolSequence::open(&s.save().unwrap()).unwrap();
        }
    }
    s.save().unwrap();
    invariants_clean(&s).unwrap();
    let wire = ops.into_iter().map(|(op, ctx)| (op.to_string(), ctx)).collect::<Vec<_>>();
    assert_eq!(replay(bytes, &wire), None);
}

/// TEST-07 的门自检：release 的 SAVE-02 诊断在 Package，不在会话诊断列表。
#[test]
fn test_07_invariant_gate_reads_package_diagnostics() {
    let body = "<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>";
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let col = s.document().tables().next().unwrap().grid[0].node;
    let part = s.main_part();
    // 故意绕过编辑器制造非法几何，只用于检验门本身能否发现包级诊断。
    s.package_mut().dom_mut(part).unwrap().unwrap().delete(col);
    let saved = s.save();
    if cfg!(debug_assertions) {
        assert!(matches!(saved, Err(rsword::Error::Invariant(_))));
    } else {
        saved.unwrap();
        assert!(
            !s.diagnostics()
                .iter()
                .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation)
        );
        assert!(
            s.package().diagnostics().iter().any(|d| d.code == rsword::DiagCode::SaveTableGrid)
        );
        assert!(invariants_clean(&s).unwrap_err().contains("SaveTableGrid"));
    }
}
