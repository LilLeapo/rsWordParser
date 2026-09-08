//! `TEST-06` / `TEST-07` `fuzz_edit`（`spec/18` 7.9）：把任意字节解释成一串编辑操作，
//! 打在五份内嵌的小语料上。断言三条：
//!
//! 1. **不 panic**（不变式 4：病态输入只能局部降级）；
//! 2. `Err` 之后主 part 的字节**一点没变**（`EDIT-05` 的整体回滚）；
//! 3. 保存出来的包能重新打开（良构）。
//!
//! 操作的形状是 `OpSketch`（种类 + 相对位置 + 短文本），映射到 `EditOp` 时按当前文档的段落
//! 数取模——同一串字节在不同种子文档上会落成不同的操作，覆盖面比写死位置大。
#![no_main]

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;
use rsword::edit::{
    BlockAt, BlockPos, EditContext, EditOp, EditSession, InlinePos, NewBlock, NewInline, NewRun,
    RevisionAuthor,
};
use rsword::model::Block;
use rsword::xml::NodeId;

/// 五份小语料（各 2 KB 上下）：双向文字、表格 + 图、交叉引用字段、修订、表格修订。
const SEEDS: [&[u8]; 5] = [
    include_bytes!("../../corpus/synthetic/bidi__007.docx"),
    include_bytes!("../../corpus/synthetic/bugfix-regressions__006.docx"),
    include_bytes!("../../corpus/synthetic/bookmarks-crossref__006.docx"),
    include_bytes!("../../corpus/synthetic/revisions__001.docx"),
    include_bytes!("../../corpus/synthetic/table-revisions__001.docx"),
];

#[derive(Arbitrary, Debug)]
struct OpSketch {
    kind: u8,
    para: u16,
    from: u16,
    to: u16,
    /// 短文本；插入前会截到 16 个字符。
    text: String,
    /// 0 = 不追踪，其余 = 作者甲 / 乙。
    track: u8,
}

#[derive(Arbitrary, Debug)]
struct Plan {
    seed: u8,
    ops: Vec<OpSketch>,
}

fn ctx_of(track: u8) -> EditContext {
    EditContext {
        track_changes: match track % 3 {
            0 => None,
            1 => Some(RevisionAuthor { author: "甲".into(), date: None }),
            _ => Some(RevisionAuthor { author: "乙".into(), date: Some("2026-01-01T00:00:00Z".into()) }),
        },
        ..Default::default()
    }
}

/// 主 part 序列化后的字节——`EDIT-05` 的"一点没动"就比它。
fn main_bytes(s: &EditSession) -> Vec<u8> {
    let Some(dom) = s.package().part(s.document().main_part).dom() else { return Vec::new() };
    rsword::save::serialize(dom).unwrap_or_default()
}

fn op_of(s: &EditSession, k: &OpSketch) -> Option<EditOp> {
    let doc = s.document();
    let paras: Vec<(NodeId, u32)> = doc.paragraphs().map(|b| (b.node, b.utf16_len())).collect();
    let blocks: Vec<NodeId> = doc.main.iter().map(Block::node).collect();
    let (para, len) = *paras.get(k.para as usize % paras.len().max(1))?;
    let a = u32::from(k.from) % (len + 1);
    let b = u32::from(k.to) % (len + 1);
    let (lo, hi) = (a.min(b), a.max(b));
    let text: String = k.text.chars().take(16).collect();
    let pos = |o: u32| InlinePos::new(para, o);
    Some(match k.kind % 10 {
        0 => EditOp::InsertText { at: pos(a), text, props: None },
        1 => EditOp::DeleteRange { from: pos(lo), to: pos(hi) },
        2 => EditOp::SplitParagraph { at: pos(a) },
        3 => EditOp::MergeWithNext { part: None, para },
        4 => EditOp::AddBookmark { name: text, from: pos(lo), to: pos(hi) },
        5 => EditOp::InsertBlock {
            at: BlockPos {
                part: None,
                at: BlockAt::After(*blocks.get(k.para as usize % blocks.len().max(1))?),
            },
            block: NewBlock::Paragraph {
                props: None,
                inlines: vec![NewInline::Run(NewRun::text(text))],
            },
        },
        6 => EditOp::DeleteBlock {
            part: None,
            node: *blocks.get(k.para as usize % blocks.len().max(1))?,
        },
        7 => {
            let ids: Vec<_> = doc.revisions.entries().iter().map(|e| e.id).collect();
            let id = *ids.get(k.from as usize % ids.len().max(1))?;
            if k.to % 2 == 0 {
                EditOp::AcceptRevision { rev: id }
            } else {
                EditOp::RejectRevision { rev: id }
            }
        }
        8 => EditOp::AcceptAll { author: None },
        _ => EditOp::RejectAll { author: None },
    })
}

fuzz_target!(|plan: Plan| {
    let bytes = SEEDS[plan.seed as usize % SEEDS.len()];
    let Ok(mut s) = EditSession::open(bytes) else { return };
    for k in plan.ops.iter().take(32) {
        let Some(op) = op_of(&s, k) else { continue };
        let before = main_bytes(&s);
        match s.apply(op, &ctx_of(k.track)) {
            Ok(_) => {}
            // `EDIT-05`：拒绝之后 DOM / Span / Model 一点没动
            Err(_) => assert_eq!(main_bytes(&s), before, "被拒的操作动了主 part"),
        }
    }
    // 保存出来的包必须能重新打开
    if let Ok(out) = s.save() {
        EditSession::open(&out).expect("保存出来的包打不开");
    }
});
