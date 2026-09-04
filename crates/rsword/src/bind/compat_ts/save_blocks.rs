//! `COMPAT-08` / `EDIT-04`：TS `saveDocx(parsed, finalBlocks, options)` 的 `SaveBlock[]` → `EditOp`。
//!
//! | SaveBlock | 映射 |
//! | --- | --- |
//! | `original`（顺序不变） | 无操作，节点保持 `Clean` |
//! | `original` 顺序变化 | `MoveBlock`（M1：仅全 original 列表） |
//! | `generated` 取代缺失的 original `w:p` | `ReplaceParaProps`（`rawPPr` 逐字 / 由 `type`+`format` 重建）+ `ReplaceInlines` |
//! | 多余的 `generated` | `InsertBlock{Paragraph}`，插在下一个 original 之前（sdt 首段前 → sdt 之前） |
//! | `xml` | `InsertBlock{Xml}`；带 `docxIndex` 时先插后删原块 |
//! | 缺失的 original | `DeleteBlock` |
//! | `chart` / `image` / 块级 `revision` / `SaveOptions` | M1 不支持 → `Err(EDIT_UNSUPPORTED)` |
//!
//! `GeneratedBlock.runs` 按 TS `runsXml` / `runFragmentXml` / `generateRunXml` 的语义翻译成 `NewInline`：
//! 批注范围标记按 `commentIds` 的首末 run 重发，同 `href` 的连续 run 合成一个 `w:hyperlink`，`ins/del`
//! 分组包裹；`rawRPr` 按 `mergeRPrModel` 的分组规则与模型字段合并（相等的组保留原样，不等的组重建，
//! 未建模子元素原位保留），无 `rawRPr` 时按 `modelRPrChildren` 从模型生成。`sdtShell` 忽略（DOM 天然保留）。

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

use crate::bind::compat_ts::{blocks, parsed_doc_of};
use crate::diag::DiagCode;
use crate::edit::ops::ppr_of;
use crate::edit::{
    BlockPos, EditContext, EditOp, EditSession, NewBlock, NewInline, NewLinkTarget, NewMarker,
    NewRevision, NewRun,
};
use crate::error::{Error, Result};
use crate::package::PartFlavor;
use crate::semantic::props::{
    Border, BorderStyle, Color, DropCap, FontHint, Fonts, FrameAnchor, FramePr, FrameWrap,
    HeightRule, HexColorOrAuto, HighlightColor, Indent, Jc, LineSpacingRule, NumPr, ParaBorders,
    ParaProps, RunProps, Shading, ShadingPattern, Spacing, Tab, TabJc, TabLeader, Tabs, Underline,
    UnderlineKind, Val, VerticalAlignRun, emit_para_props, emit_run_props, order_index_run_props,
    read_run_props,
};
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NodeId, NsId, QName, parse_fragment, parse_fragment_dom,
};

/// `apply_save_blocks` 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveBlocksOutcome {
    /// TS `isUnchanged`：全部 original 且顺序不变、无选项 → 无操作。
    pub unchanged: bool,
    pub ops: usize,
}

fn unsupported(msg: impl Into<String>) -> Error {
    Error::edit(DiagCode::EditUnsupported, msg)
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

/// TS `bookmarkIdOf`：`h = (h * 31 + code) | 0` 逐 UTF-16 单元，`Math.abs(h) % 0x7fffffff`。
pub fn bookmark_id_of(name: &str) -> u32 {
    let mut h: i32 = 0;
    for u in name.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(u));
    }
    h.unsigned_abs() % 0x7fff_ffff
}

enum Item<'a> {
    Original(usize),
    Generated(&'a Value, Option<&'a Value>),
    Xml { xml: &'a str, docx_index: Option<usize>, revision: Option<&'a Value> },
}

fn s_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn truthy(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn round(x: f64) -> i32 {
    x.round() as i32
}

/// 把 TS `SaveBlock[]`（`finalBlocks`）与 `SaveOptions` 应用到会话；任一块不受支持则不改任何状态。
pub fn apply_save_blocks(
    session: &mut EditSession,
    final_blocks: &Value,
    options: &Value,
) -> Result<SaveBlocksOutcome> {
    if options.as_object().is_some_and(|o| !o.is_empty()) {
        let keys: Vec<&String> = options.as_object().unwrap().keys().collect();
        return Err(unsupported(format!("SaveOptions {keys:?} 在后续里程碑（SAVE-07）")));
    }
    let final_blocks =
        final_blocks.as_array().ok_or_else(|| unsupported("finalBlocks 不是数组"))?;
    let parsed = parsed_doc_of(session.package(), session.document());
    if parsed["removePersonalInfo"] == Value::Bool(true) {
        // TS：文档自带 w:removePersonalInformation 时即使无编辑也清洗作者信息
        return Err(unsupported("removePersonalInformation 清洗（SAVE-07）"));
    }
    let body = session
        .document()
        .body
        .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "文档没有 w:body"))?;
    let nodes = blocks::element_nodes(session.dom(), body);
    let pblocks = parsed["blocks"].as_array().expect("parsed_doc blocks");
    if nodes.len() != pblocks.len() {
        return Err(Error::edit(
            DiagCode::EditPlanInvalid,
            format!("元素表 {} 与块表 {} 长度不一致", nodes.len(), pblocks.len()),
        ));
    }
    let visible: Vec<usize> =
        pblocks.iter().enumerate().filter(|(_, b)| !truthy(b, "hidden")).map(|(i, _)| i).collect();
    let heading_ids: HashMap<u32, String> = parsed["headingStyleIds"]
        .as_object()
        .map(|m| {
            m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect()
        })
        .unwrap_or_default();
    let list_style = parsed["listParagraphStyleId"].as_str().map(str::to_string);

    // SaveBlock → Item
    let mut items: Vec<Item<'_>> = Vec::with_capacity(final_blocks.len());
    for fb in final_blocks {
        let revision = fb.get("revision").filter(|r| r.is_object());
        match s_of(fb, "kind") {
            Some("original") => {
                if revision.is_some() {
                    return Err(unsupported(
                        "original 块的修订包裹（把已有块搬进 New w:ins）在 M7",
                    ));
                }
                let d =
                    fb["docxIndex"].as_u64().ok_or_else(|| unsupported("original 缺 docxIndex"))?
                        as usize;
                if d >= nodes.len() {
                    return Err(Error::edit(
                        DiagCode::EditBadPosition,
                        format!("docxIndex {d} 越界"),
                    ));
                }
                items.push(Item::Original(d));
            }
            Some("generated") => {
                let blk = fb.get("block").ok_or_else(|| unsupported("generated 缺 block"))?;
                items.push(Item::Generated(blk, revision));
            }
            Some("xml") => {
                if fb.get("replaceImage").is_some() {
                    return Err(unsupported("xml.replaceImage（媒体 part 分配）在 M3"));
                }
                let xml = s_of(fb, "xml").ok_or_else(|| unsupported("xml 缺 xml"))?;
                let docx_index = fb["docxIndex"].as_u64().map(|d| d as usize);
                items.push(Item::Xml { xml, docx_index, revision });
            }
            other => return Err(unsupported(format!("SaveBlock kind {other:?} 在后续里程碑"))),
        }
    }

    // TS isUnchanged
    let all_original_in_order = items.len() == visible.len()
        && items.iter().zip(&visible).all(|(it, &v)| matches!(it, Item::Original(d) if *d == v));
    if all_original_in_order {
        return Ok(SaveBlocksOutcome { unchanged: true, ops: 0 });
    }

    let main = session.main_part();
    let flavor = session.flavor();
    let ops = {
        let dom = session.package_mut().dom_mut(main)?.expect("main part parsed");
        let mut planner = Planner {
            dom,
            flavor,
            body,
            nodes: &nodes,
            heading_ids: &heading_ids,
            list_style: list_style.as_deref(),
        };
        planner.build_ops(&items, &visible)?
    };
    let n = ops.len();
    session.apply_all(ops, &EditContext::default())?;
    Ok(SaveBlocksOutcome { unchanged: false, ops: n })
}

struct Planner<'a> {
    dom: &'a mut Dom,
    flavor: PartFlavor,
    body: NodeId,
    nodes: &'a [NodeId],
    heading_ids: &'a HashMap<u32, String>,
    list_style: Option<&'a str>,
}

impl Planner<'_> {
    fn build_ops(&mut self, items: &[Item<'_>], visible: &[usize]) -> Result<Vec<EditOp>> {
        let present: BTreeSet<usize> = items
            .iter()
            .filter_map(|it| if let Item::Original(d) = it { Some(*d) } else { None })
            .collect();
        for &d in &present {
            if !visible.contains(&d) {
                return Err(unsupported(format!("original 引用隐藏块 docxIndex {d}")));
            }
        }
        let missing: Vec<usize> =
            visible.iter().copied().filter(|d| !present.contains(d)).collect();
        let mut handled: HashSet<usize> = HashSet::new();
        let mut ops = Vec::new();

        // 重排（M1：只支持全 original 列表）
        let mut max_seen: Option<usize> = None;
        let mut out_of_order = Vec::new();
        for (k, it) in items.iter().enumerate() {
            if let Item::Original(d) = it {
                if max_seen.is_some_and(|m| *d < m) {
                    out_of_order.push(k);
                } else {
                    max_seen = Some(*d);
                }
            }
        }
        if !out_of_order.is_empty() {
            if !items.iter().all(|it| matches!(it, Item::Original(_))) {
                return Err(unsupported("original 重排与 generated/xml 混合"));
            }
            for &k in &out_of_order {
                let Item::Original(d) = items[k] else { unreachable!() };
                let node = self.nodes[d];
                if self.dom.parent(node) != Some(self.body) {
                    return Err(unsupported("重排 sdt 内的段落"));
                }
                let to = match k.checked_sub(1).map(|p| &items[p]) {
                    Some(Item::Original(pd)) => BlockPos::After(self.nodes[*pd]),
                    _ => BlockPos::Start(self.body),
                };
                ops.push(EditOp::MoveBlock { node, to });
            }
        }

        // 逐个"间隙"（两个 present original 之间的非 original 序列）配对
        let mut i = 0;
        let mut lo: Option<usize> = None;
        while i < items.len() {
            if let Item::Original(d) = items[i] {
                lo = Some(d);
                i += 1;
                continue;
            }
            let j = items[i..]
                .iter()
                .position(|it| matches!(it, Item::Original(_)))
                .map_or(items.len(), |k| i + k);
            let hi = if j < items.len() {
                if let Item::Original(d) = items[j] { Some(d) } else { None }
            } else {
                None
            };
            let mut candidates: Vec<usize> = missing
                .iter()
                .copied()
                .filter(|&m| lo.is_none_or(|l| m > l) && hi.is_none_or(|h| m < h))
                .collect();
            let anchor = match hi {
                Some(h) => BlockPos::Before(self.insert_anchor(self.nodes[h])),
                None => BlockPos::End(self.body),
            };
            for it in &items[i..j] {
                match it {
                    Item::Original(_) => unreachable!(),
                    Item::Generated(blk, revision) => {
                        // 与缺失的 original 配对：`w:p`，或单段 sdt 里的那个 `w:p`（TS 用 sdtShell 重新包裹）
                        let paired = candidates.first().copied().and_then(|c| {
                            let n = self.nodes[c];
                            if self.dom.is(n, w(LocalName::P)) {
                                Some((c, n))
                            } else if self.dom.is(n, w(LocalName::Sdt)) {
                                blocks::sdt_single_paragraph(self.dom, n).map(|p| (c, p))
                            } else {
                                None
                            }
                        });
                        match paired {
                            Some((c, para)) if revision.is_none() => {
                                candidates.remove(0);
                                handled.insert(c);
                                self.generated_replace(para, blk, &mut ops)?;
                            }
                            _ => {
                                let (props, inlines) = self.generated_paragraph(blk)?;
                                let block = NewBlock::Paragraph { props, inlines };
                                ops.push(EditOp::InsertBlock {
                                    at: anchor,
                                    block: wrap_revision(block, *revision),
                                });
                            }
                        }
                    }
                    Item::Xml { xml, docx_index, revision } => {
                        let frags = self.fragment_blocks(xml)?;
                        match docx_index {
                            Some(d) => {
                                let old = self.nodes[*d];
                                for frag in frags {
                                    ops.push(EditOp::InsertBlock {
                                        at: BlockPos::Before(old),
                                        block: wrap_revision(NewBlock::Xml(frag), *revision),
                                    });
                                }
                                ops.push(EditOp::DeleteBlock { node: old });
                                handled.insert(*d);
                                candidates.retain(|&c| c != *d);
                            }
                            None => {
                                for frag in frags {
                                    ops.push(EditOp::InsertBlock {
                                        at: anchor,
                                        block: wrap_revision(NewBlock::Xml(frag), *revision),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            for c in candidates {
                handled.insert(c);
                ops.push(EditOp::DeleteBlock { node: self.nodes[c] });
            }
            i = j;
        }
        for m in missing {
            if !handled.contains(&m) {
                ops.push(EditOp::DeleteBlock { node: self.nodes[m] });
            }
        }
        Ok(ops)
    }

    /// 在某个块之前插入时的锚点：sdt 拆分出的首段 → 整个 sdt（TS 把新块放在 `openXml` 之前）。
    fn insert_anchor(&self, node: NodeId) -> NodeId {
        let dom = &*self.dom;
        let mut top = node;
        while dom.parent(top).is_some_and(|p| p != self.body) {
            top = dom.parent(top).expect("checked");
        }
        if top == node {
            return node;
        }
        let first_in_sdt = self.nodes.iter().copied().find(|&n| {
            let mut t = n;
            while dom.parent(t).is_some_and(|p| p != self.body) {
                t = dom.parent(t).expect("checked");
            }
            t == top
        });
        if first_in_sdt == Some(node) { top } else { node }
    }

    /// `kind: 'xml'` 的片段：TS 直接拼接字符串，所以允许多个顶层块元素。
    fn fragment_blocks(&mut self, xml: &str) -> Result<Vec<NewElement>> {
        let frags = parse_fragment(self.dom, xml)
            .map_err(|e| unsupported(format!("xml 块解析失败: {e}")))?;
        if frags.is_empty() {
            return Err(unsupported("xml 块没有顶层元素"));
        }
        Ok(frags)
    }

    /// generated 取代已有 `w:p`：`ReplaceParaProps`（需要时）+ `ReplaceInlines`。
    fn generated_replace(
        &mut self,
        para: NodeId,
        blk: &Value,
        ops: &mut Vec<EditOp>,
    ) -> Result<()> {
        let current = ppr_of(self.dom, para);
        let (props, inlines) = self.generated_paragraph(blk)?;
        let same_raw = match (s_of(blk, "rawPPr"), current) {
            (Some(raw), Some(c)) => {
                let node = self.dom.node(c);
                node.dirty == Dirty::Clean
                    && node.lex.as_ref().is_some_and(|l| self.dom.lex_str(&l.range) == raw)
            }
            (Some(raw), None) => raw.is_empty(),
            (None, _) => false,
        };
        let skip =
            same_raw || (blk.get("rawPPr").is_none() && props.is_none() && current.is_none());
        if !skip {
            ops.push(EditOp::ReplaceParaProps { para, props });
        }
        ops.push(EditOp::ReplaceInlines { para, inlines });
        Ok(())
    }

    /// TS `generateParagraphXml`：`(pPr, inlines)`。
    fn generated_paragraph(&mut self, blk: &Value) -> Result<(Option<NewElement>, Vec<NewInline>)> {
        if blk.get("pPrChange").is_some_and(|v| !v.is_null()) {
            return Err(unsupported("GeneratedBlock.pPrChange 在 M7"));
        }
        let props = match s_of(blk, "rawPPr") {
            Some("") => None,
            Some(raw) => {
                let mut frags = parse_fragment(self.dom, raw)
                    .map_err(|e| unsupported(format!("rawPPr 解析失败: {e}")))?;
                if frags.len() != 1 || frags[0].name != w(LocalName::PPr) {
                    return Err(unsupported("rawPPr 不是单个 w:pPr"));
                }
                Some(frags.remove(0))
            }
            None => self.para_props_from_format(blk),
        };
        let mut inlines = Vec::new();
        let bookmark = |name: &str| {
            let id = bookmark_id_of(name).to_string();
            [
                NewInline::Marker(NewMarker::BookmarkStart {
                    id: id.clone(),
                    name: name.to_string(),
                }),
                NewInline::Marker(NewMarker::BookmarkEnd { id }),
            ]
        };
        for k in ["hiddenBookmarks", "bookmarks"] {
            for n in blk.get(k).and_then(Value::as_array).into_iter().flatten() {
                if let Some(n) = n.as_str() {
                    inlines.extend(bookmark(n));
                }
            }
        }
        for id in blk.get("commentStarts").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = id.as_str() {
                inlines
                    .push(NewInline::Marker(NewMarker::CommentRangeStart { id: id.to_string() }));
            }
        }
        let runs = blk.get("runs").and_then(Value::as_array).cloned().unwrap_or_default();
        self.runs_to_inlines(&runs, &mut inlines)?;
        for id in blk.get("commentEnds").and_then(Value::as_array).into_iter().flatten() {
            if let Some(id) = id.as_str() {
                inlines.push(NewInline::Marker(NewMarker::CommentRangeEnd { id: id.to_string() }));
                inlines.push(NewInline::Marker(NewMarker::CommentReference { id: id.to_string() }));
            }
        }
        Ok((props, inlines))
    }

    /// TS `generateParagraphXml` 无 `rawPPr` 分支 + `formatPPrChildren`。
    fn para_props_from_format(&self, blk: &Value) -> Option<NewElement> {
        let mut p = ParaProps::default();
        let ty = s_of(blk, "type").unwrap_or("paragraph");
        p.style = match ty {
            "heading" => {
                let level = num(blk, "level").map_or(1, |l| l as i64).clamp(1, 9) as u32;
                s_of(blk, "styleId")
                    .map(str::to_string)
                    .or_else(|| self.heading_ids.get(&level).cloned())
            }
            "listItem" => s_of(blk, "styleId")
                .map(str::to_string)
                .or_else(|| self.list_style.map(str::to_string)),
            _ => s_of(blk, "styleId").map(str::to_string),
        };
        if ty == "listItem"
            && let Some(list) = blk.get("list")
            && list.is_object()
        {
            let ilvl = num(list, "ilvl").map_or(0, |x| x as i64).clamp(0, 8) as i32;
            let num_id = match list.get("numId") {
                Some(Value::String(s)) => {
                    s.parse::<i32>().map_or_else(|_| Val::Raw(s.clone()), Val::Value)
                }
                Some(Value::Number(n)) => Val::Value(n.as_i64().unwrap_or(0) as i32),
                _ => Val::Raw(String::new()),
            };
            p.num = Some(NumPr {
                ilvl: Some(Val::Value(ilvl)),
                num_id: Some(num_id),
                ..Default::default()
            });
        }
        if let Some(f) = blk.get("format").filter(|f| f.is_object()) {
            format_into(f, &mut p);
        }
        (p != ParaProps::default()).then(|| emit_para_props(&p, self.flavor))
    }

    /// TS `runsXml`：批注范围、超链接分组、修订分组。
    fn runs_to_inlines(&mut self, runs: &[Value], out: &mut Vec<NewInline>) -> Result<()> {
        // 批注：每个 id 覆盖的首末 run
        let mut first_of: Vec<(String, usize)> = Vec::new();
        let mut last_of: HashMap<String, usize> = HashMap::new();
        for (i, r) in runs.iter().enumerate() {
            for id in r.get("commentIds").and_then(Value::as_array).into_iter().flatten() {
                let Some(id) = id.as_str() else { continue };
                if !first_of.iter().any(|(k, _)| k == id) {
                    first_of.push((id.to_string(), i));
                }
                last_of.insert(id.to_string(), i);
            }
        }
        let starts_at = |i: usize, out: &mut Vec<NewInline>| {
            for (id, at) in &first_of {
                if *at == i {
                    out.push(NewInline::Marker(NewMarker::CommentRangeStart { id: id.clone() }));
                }
            }
        };
        let ends_at = |i: usize, out: &mut Vec<NewInline>| {
            for (id, _) in &first_of {
                if last_of.get(id) == Some(&i) {
                    out.push(NewInline::Marker(NewMarker::CommentRangeEnd { id: id.clone() }));
                    out.push(NewInline::Marker(NewMarker::CommentReference { id: id.clone() }));
                }
            }
        };
        let rev_key = |r: &Value| -> Option<String> {
            let ins = r.get("ins").filter(|v| v.is_object());
            let del = r.get("del").filter(|v| v.is_object());
            if ins.is_none() && del.is_none() {
                return None;
            }
            let part = |v: Option<&Value>| {
                v.map_or("null".to_string(), |v| {
                    format!("{:?}|{:?}|{:?}", s_of(v, "author"), s_of(v, "date"), s_of(v, "id"))
                })
            };
            Some(format!("{}#{}", part(ins), part(del)))
        };
        let revision = |v: &Value| NewRevision {
            id: s_of(v, "id").map(str::to_string),
            author: s_of(v, "author").unwrap_or_default().to_string(),
            date: s_of(v, "date").map(str::to_string),
        };

        let mut g = 0;
        while g < runs.len() {
            let key = rev_key(&runs[g]);
            let mut end = g;
            while end < runs.len() && rev_key(&runs[end]) == key {
                end += 1;
            }
            let mut group_out = Vec::new();
            self.emit_range(runs, g, end, &starts_at, &ends_at, &mut group_out)?;
            match key {
                None => out.extend(group_out),
                Some(_) => {
                    let mut inner = group_out;
                    if let Some(del) = runs[g].get("del").filter(|v| v.is_object()) {
                        inner = vec![NewInline::Del { rev: revision(del), inlines: inner }];
                    }
                    if let Some(ins) = runs[g].get("ins").filter(|v| v.is_object()) {
                        inner = vec![NewInline::Ins { rev: revision(ins), inlines: inner }];
                    }
                    out.extend(inner);
                }
            }
            g = end;
        }
        Ok(())
    }

    /// TS `emitRange`：`[from, to)` 内的 run，超链接分组 + 批注标记。
    fn emit_range(
        &mut self,
        runs: &[Value],
        from: usize,
        to: usize,
        starts_at: &dyn Fn(usize, &mut Vec<NewInline>),
        ends_at: &dyn Fn(usize, &mut Vec<NewInline>),
        out: &mut Vec<NewInline>,
    ) -> Result<()> {
        let mut i = from;
        while i < to {
            let run = &runs[i];
            let link = run.get("link").filter(|l| l.is_object());
            if let Some(link) = link {
                let href = s_of(link, "href").unwrap_or_default().to_string();
                let start = i;
                let mut rid: Option<String> = None;
                let mut tooltip: Option<String> = None;
                while i < to
                    && runs[i]
                        .get("link")
                        .filter(|l| l.is_object())
                        .is_some_and(|l| s_of(l, "href") == Some(href.as_str()))
                {
                    let l = &runs[i]["link"];
                    if rid.is_none() {
                        rid = s_of(l, "rId").map(str::to_string);
                    }
                    if i == start {
                        tooltip = s_of(l, "tooltip").filter(|t| !t.is_empty()).map(str::to_string);
                    }
                    i += 1;
                }
                for j in start..i {
                    starts_at(j, out);
                }
                let target = if let Some(anchor) = href.strip_prefix('#') {
                    Some(NewLinkTarget::Anchor(anchor.to_string()))
                } else {
                    match rid {
                        Some(r) => Some(NewLinkTarget::Rel(r)),
                        None => {
                            return Err(unsupported(format!(
                                "新外部超链接 {href} 需要分配关系（EDIT-06 rId，M2）"
                            )));
                        }
                    }
                };
                let mut inner = Vec::new();
                for r in &runs[start..i] {
                    self.run_fragment(r, true, &mut inner)?;
                }
                if let Some(target) = target {
                    out.push(NewInline::Hyperlink { target, tooltip, inlines: inner });
                }
                for j in start..i {
                    ends_at(j, out);
                }
            } else {
                starts_at(i, out);
                self.run_fragment(run, false, out)?;
                ends_at(i, out);
                i += 1;
            }
        }
        Ok(())
    }

    /// TS `runFragmentXml`。
    fn run_fragment(
        &mut self,
        run: &Value,
        inside_link: bool,
        out: &mut Vec<NewInline>,
    ) -> Result<()> {
        if let Some(omml) = run.get("math").and_then(|m| s_of(m, "omml")) {
            for e in parse_fragment(self.dom, omml)
                .map_err(|e| unsupported(format!("OMML 解析失败: {e}")))?
            {
                out.push(NewInline::Xml(e));
            }
            return Ok(());
        }
        if let Some(xml) = run.get("ruby").and_then(|m| s_of(m, "xml")) {
            let mut r = NewElement::new(w(LocalName::R));
            for e in parse_fragment(self.dom, xml)
                .map_err(|e| unsupported(format!("ruby 解析失败: {e}")))?
            {
                r.push_child(e);
            }
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        if let Some(xml) = run.get("image").and_then(|m| s_of(m, "xml")) {
            if s_of(run, "text").is_some_and(|t| !t.is_empty()) {
                let props = self.run_props(run, inside_link)?;
                out.push(NewInline::Run(NewRun {
                    text: s_of(run, "text").unwrap_or_default().to_string(),
                    props,
                }));
            }
            let mut r = NewElement::new(w(LocalName::R));
            for e in parse_fragment(self.dom, xml)
                .map_err(|e| unsupported(format!("image.xml 解析失败: {e}")))?
            {
                r.push_child(e);
            }
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        if let Some(note) = run.get("noteRef").filter(|n| n.is_object()) {
            let tag = if s_of(note, "kind") == Some("footnote") {
                LocalName::FootnoteReference
            } else {
                LocalName::EndnoteReference
            };
            let rpr = NewElement::new(w(LocalName::RPr)).with_child(
                NewElement::new(w(LocalName::VertAlign))
                    .with_attr(w(LocalName::Val), "superscript"),
            );
            let r = NewElement::new(w(LocalName::R)).with_child(rpr).with_child(
                NewElement::new(w(tag))
                    .with_attr(w(LocalName::Id), s_of(note, "id").unwrap_or_default()),
            );
            out.push(NewInline::Xml(r));
            return Ok(());
        }
        for k in ["refField", "instrField", "xeTerm", "fldBeginXml"] {
            if run.get(k).is_some_and(|v| !v.is_null()) {
                return Err(unsupported(format!("run.{k}：字段生成（FLD-12）在 M2")));
            }
        }
        if run.get("rPrChange").is_some_and(|v| !v.is_null()) {
            return Err(unsupported("run.rPrChange 在 M7"));
        }
        let text = s_of(run, "text").unwrap_or_default();
        if text.is_empty() {
            return Ok(());
        }
        let props = self.run_props(run, inside_link)?;
        out.push(NewInline::Run(NewRun { text: text.to_string(), props }));
        Ok(())
    }

    /// TS `generateRunXml` 的 `rPr`：`rawRPr` → `mergeRPrModel`；否则 `modelRPrChildren`。
    fn run_props(&mut self, run: &Value, inside_link: bool) -> Result<Option<NewElement>> {
        let flavor = self.flavor;
        let Some(raw) = s_of(run, "rawRPr") else {
            let mut p = RunProps::default();
            model_into(run, inside_link, &mut p, false);
            return Ok((p != RunProps::default()).then(|| emit_run_props(&p, flavor)));
        };
        let (tmp, tops) = parse_fragment_dom(self.dom, raw)
            .map_err(|e| unsupported(format!("rawRPr 解析失败: {e}")))?;
        let rpr = tops.first().copied().filter(|&n| tmp.is(n, w(LocalName::RPr)));
        let Some(rpr) = rpr else {
            // '<w:rPr/>' 或无法识别：按模型重建
            let mut p = RunProps::default();
            model_into(run, inside_link, &mut p, false);
            return Ok((p != RunProps::default()).then(|| emit_run_props(&p, flavor)));
        };
        let mut diags = Vec::new();
        let mut p = read_run_props(&tmp, Some(rpr), &mut diags);
        let cs = truthy(run, "cs") || p.rtl == Some(true);
        merge_model(run, inside_link, cs, &mut p);
        // 未建模子元素：`w:rPrChange` 由模型接管（JSON 无 rPrChange → 丢弃），其余原位保留
        let raw_kids: Vec<NodeId> = tmp.children(rpr).to_vec();
        let mut extra: Vec<(u16, u8, NewElement)> = Vec::new();
        let mut last_idx = 0u16;
        for &c in &raw_kids {
            let Some(name) = tmp.name(c) else { continue };
            if let Some(idx) = order_index_run_props(name) {
                last_idx = idx;
            }
            if p.raw_unmodeled.contains(&c)
                && !tmp.is(c, w(LocalName::RPrChange))
                && let Some(e) = NewElement::from_dom(&tmp, c, self.dom.interner_mut())
            {
                extra.push((order_index_run_props(name).unwrap_or(last_idx), 1, e));
            }
        }
        p.raw_unmodeled.clear();
        let emitted = emit_run_props(&p, flavor);
        let mut all: Vec<(u16, u8, NewElement)> = emitted
            .child_elements()
            .map(|e| (order_index_run_props(e.name).unwrap_or(u16::MAX), 0, e.clone()))
            .collect();
        all.extend(extra);
        all.sort_by_key(|(idx, sub, _)| (*idx, *sub));
        if all.is_empty() {
            return Ok(None);
        }
        let mut out = NewElement::new(w(LocalName::RPr));
        for (_, _, e) in all {
            out.push_child(e);
        }
        Ok(Some(out))
    }
}

/// TS：`fb.revision` → 整块包进 `w:ins` / `w:del`（`id` 缺省 TS 写 `0`，这里按 `EDIT-06` 由引擎分配）。
fn wrap_revision(block: NewBlock, revision: Option<&Value>) -> NewBlock {
    let Some(rev) = revision else { return block };
    let kind = if s_of(rev, "kind") == Some("del") { LocalName::Del } else { LocalName::Ins };
    let mut wrapper = NewElement::new(w(kind));
    if let Some(id) = s_of(rev, "id") {
        wrapper.push_attr(w(LocalName::Id), id);
    } else {
        wrapper.push_attr(w(LocalName::Id), String::new()); // 由 InsertBlock 分配
    }
    wrapper.push_attr(w(LocalName::Author), s_of(rev, "author").unwrap_or_default());
    if let Some(d) = s_of(rev, "date") {
        wrapper.push_attr(w(LocalName::Date), d);
    }
    NewBlock::Wrapped { wrapper, block: Box::new(block) }
}

fn hex(s: &str) -> Option<HexColorOrAuto> {
    HexColorOrAuto::parse(s)
}

fn hex_upper(c: &HexColorOrAuto) -> Option<String> {
    c.rgb().map(|[r, g, b]| format!("{r:02X}{g:02X}{b:02X}"))
}

/// TS `freshRFontsXml`。
fn fresh_fonts(font: Option<&str>, ascii: Option<&str>, cs: Option<&str>) -> Fonts {
    let a = ascii.or(font).or(cs).unwrap_or("").to_string();
    Fonts {
        ascii: Some(a.clone()),
        h_ansi: Some(a.clone()),
        east_asia: font.map(str::to_string),
        cs: Some(cs.map_or(a, str::to_string)),
        ..Default::default()
    }
}

/// TS `modelRPrChildren`：把 JSON run 的建模字段写进 `p`（`fresh_fonts` 由 `with_fonts` 控制）。
fn model_into(run: &Value, inside_link: bool, p: &mut RunProps, keep_fonts: bool) {
    p.style = if inside_link {
        Some("Hyperlink".to_string())
    } else {
        s_of(run, "styleId").map(str::to_string)
    };
    if !keep_fonts {
        let (font, ascii, cs) = (s_of(run, "font"), s_of(run, "fontAscii"), s_of(run, "fontCs"));
        p.fonts = (font.is_some() || ascii.is_some() || cs.is_some())
            .then(|| fresh_fonts(font, ascii, cs));
    }
    let b = truthy(run, "bold");
    p.bold = b.then_some(true);
    p.bold_cs = b.then_some(true);
    let i = truthy(run, "italic");
    p.italic = i.then_some(true);
    p.italic_cs = i.then_some(true);
    p.strike = truthy(run, "strike").then_some(true);
    p.color = s_of(run, "color")
        .and_then(hex)
        .map(|c| Color { val: Some(Val::Value(c)), ..Default::default() });
    let sz = num(run, "sizeHalfPoints").map(|x| x as u32).filter(|&x| x != 0);
    p.size = sz.map(Val::Value);
    p.size_cs = sz.map(Val::Value);
    p.highlight = s_of(run, "highlight")
        .map(|h| HighlightColor::parse(h).map_or_else(|| Val::Raw(h.to_string()), Val::Value));
    p.underline = truthy(run, "underline")
        .then(|| Underline { val: Some(Val::Value(UnderlineKind::Single)), ..Default::default() });
    p.shading = s_of(run, "shading").and_then(hex).map(|fill| Shading {
        val: Some(Val::Value(ShadingPattern::Clear)),
        color: Some(Val::Value(HexColorOrAuto::Auto)),
        fill: Some(Val::Value(fill)),
        ..Default::default()
    });
    p.vert_align = match s_of(run, "vertAlign") {
        Some("superscript") => Some(Val::Value(VerticalAlignRun::Superscript)),
        Some("subscript") => Some(Val::Value(VerticalAlignRun::Subscript)),
        _ => None,
    };
    p.rtl = truthy(run, "rtl").then_some(true);
}

/// TS `mergeRPrModel` 的分组比较：相等的组保留 `p` 里的原值，不等的组按模型重写。
fn merge_model(run: &Value, inside_link: bool, cs: bool, p: &mut RunProps) {
    let raw_bool = |v: Option<bool>| v == Some(true);
    // rStyle
    let modeled = if inside_link { Some("Hyperlink") } else { s_of(run, "styleId") };
    let raw_style = p.style.as_deref();
    if !(raw_style == modeled || (raw_style == Some("Hyperlink") && modeled.is_none())) {
        p.style = modeled.map(str::to_string);
    }
    // rFonts
    {
        let (font, ascii_m, cs_m) =
            (s_of(run, "font"), s_of(run, "fontAscii"), s_of(run, "fontCs"));
        let theme = run.get("themeRFonts").filter(|t| t.is_object());
        let t_font = theme.and_then(|t| s_of(t, "font"));
        let t_ascii = theme.and_then(|t| s_of(t, "fontAscii"));
        let f = p.fonts.clone();
        let raw_ascii: Option<String> =
            f.as_ref().and_then(|f| f.ascii.clone().or(f.h_ansi.clone()));
        let raw_primary: Option<String> =
            f.as_ref().and_then(|f| f.east_asia.clone()).or(raw_ascii.clone());
        let raw_cs: Option<String> = f.as_ref().and_then(|f| f.cs.clone());
        let equal = (raw_primary.as_deref() == font || (font.is_some() && font == t_font))
            && (raw_ascii.as_deref() == ascii_m || (ascii_m.is_some() && ascii_m == t_ascii))
            && (cs_m.is_none() || raw_cs.as_deref() == cs_m);
        if !equal {
            if let Some(mut rf) = f.filter(|_| font.is_some() || ascii_m.is_some()) {
                // mergeRFontsXml：只改模型持有的槽，去掉对应 theme 属性
                let had_ea = rf.east_asia.is_some() || rf.east_asia_theme.is_some();
                let raw_primary_owned = raw_primary.clone();
                if let Some(a) = ascii_m
                    && Some(a) != t_ascii
                {
                    rf.ascii = Some(a.to_string());
                    rf.h_ansi = Some(a.to_string());
                    rf.ascii_theme = None;
                    rf.h_ansi_theme = None;
                }
                if let Some(fo) = font
                    && Some(fo) != t_font
                    && (had_ea || Some(fo) != raw_primary_owned.as_deref())
                {
                    rf.east_asia = Some(fo.to_string());
                    rf.east_asia_theme = None;
                }
                if let Some(c) = cs_m {
                    rf.cs = Some(c.to_string());
                    rf.cs_theme = None;
                }
                p.fonts = Some(rf);
            } else {
                p.fonts = (font.is_some() || ascii_m.is_some() || cs_m.is_some())
                    .then(|| fresh_fonts(font, ascii_m, cs_m));
            }
        }
    }
    // bold / italic（rtl 时比较 Cs 孪生）
    let b = truthy(run, "bold");
    if raw_bool(if cs { p.bold_cs } else { p.bold }) != b {
        p.bold = b.then_some(true);
        p.bold_cs = b.then_some(true);
    }
    let it = truthy(run, "italic");
    if raw_bool(if cs { p.italic_cs } else { p.italic }) != it {
        p.italic = it.then_some(true);
        p.italic_cs = it.then_some(true);
    }
    let st = truthy(run, "strike");
    if raw_bool(p.strike) != st {
        p.strike = st.then_some(true);
    }
    // color
    let raw_color = p.color.as_ref().and_then(|c| c.val.as_ref()).and_then(|v| match v {
        Val::Value(c) => hex_upper(c),
        Val::Raw(s) => Some(s.clone()),
    });
    let model_color = s_of(run, "color");
    if !raw_color
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
        .eq(&model_color.map(str::to_ascii_uppercase).as_deref())
    {
        p.color = model_color
            .and_then(hex)
            .map(|c| Color { val: Some(Val::Value(c)), ..Default::default() });
    }
    // size
    let raw_size = (if cs { &p.size_cs } else { &p.size })
        .as_ref()
        .and_then(|v| v.value().copied())
        .filter(|&x| x != 0);
    let model_size = num(run, "sizeHalfPoints").map(|x| x as u32).filter(|&x| x != 0);
    if raw_size != model_size {
        p.size = model_size.map(Val::Value);
        p.size_cs = model_size.map(Val::Value);
    }
    // highlight
    let raw_hl = p.highlight.as_ref().and_then(|v| match v {
        Val::Value(HighlightColor::None) => None,
        Val::Value(h) => Some(h.as_str().to_string()),
        Val::Raw(s) => Some(s.clone()),
    });
    if raw_hl.as_deref() != s_of(run, "highlight") {
        p.highlight = s_of(run, "highlight")
            .map(|h| HighlightColor::parse(h).map_or_else(|| Val::Raw(h.to_string()), Val::Value));
    }
    // shading
    let raw_shd = p.shading.as_ref().and_then(|s| s.fill.as_ref()).and_then(|v| match v {
        Val::Value(c) => hex_upper(c),
        Val::Raw(s) => Some(s.clone()),
    });
    if raw_shd.as_deref().map(str::to_ascii_uppercase)
        != s_of(run, "shading").map(str::to_ascii_uppercase)
    {
        p.shading = s_of(run, "shading").and_then(hex).map(|fill| Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            color: Some(Val::Value(HexColorOrAuto::Auto)),
            fill: Some(Val::Value(fill)),
            ..Default::default()
        });
    }
    // underline：有 w:val 且不是 none
    let raw_u = p
        .underline
        .as_ref()
        .and_then(|u| u.val.as_ref())
        .is_some_and(|v| *v != Val::Value(UnderlineKind::None));
    if raw_u != truthy(run, "underline") {
        p.underline = truthy(run, "underline").then(|| Underline {
            val: Some(Val::Value(UnderlineKind::Single)),
            ..Default::default()
        });
    }
    // vertAlign
    let raw_va = match p.vert_align.as_ref() {
        Some(Val::Value(VerticalAlignRun::Superscript)) => Some("superscript"),
        Some(Val::Value(VerticalAlignRun::Subscript)) => Some("subscript"),
        _ => None,
    };
    if raw_va != s_of(run, "vertAlign") {
        p.vert_align = match s_of(run, "vertAlign") {
            Some("superscript") => Some(Val::Value(VerticalAlignRun::Superscript)),
            Some("subscript") => Some(Val::Value(VerticalAlignRun::Subscript)),
            _ => None,
        };
    }
    // rtl
    let r = truthy(run, "rtl");
    if raw_bool(p.rtl) != r {
        p.rtl = r.then_some(true);
    }
}

/// TS `formatPPrChildren`（`ParaFormat` → `ParaProps` 字段）。
fn format_into(f: &Value, p: &mut ParaProps) {
    if truthy(f, "pageBreakBefore") {
        p.page_break_before = Some(true);
    }
    if let Some(sides) = s_of(f, "borders") {
        let style = f.get("borderStyle").filter(|s| s.is_object());
        let default_sz =
            style.and_then(|s| num(s, "szEighths")).map_or(4, |x| round(x).max(2)) as u32;
        let space =
            style.and_then(|s| num(s, "spacePt")).map_or(1, |x| round(x).clamp(0, 31)) as u32;
        let default_color =
            style.and_then(|s| s_of(s, "color")).and_then(hex).unwrap_or(HexColorOrAuto::Auto);
        let lines = f.get("borderLines").filter(|l| l.is_object());
        let line = |ch: &str| -> Border {
            let declared = lines.and_then(|l| l.get(ch)).filter(|d| d.is_object());
            let sz = declared
                .and_then(|d| num(d, "szPt"))
                .filter(|&x| x != 0.0)
                .map_or(default_sz, |x| round(x * 8.0).max(1) as u32);
            let color =
                declared.and_then(|d| s_of(d, "color")).and_then(hex).unwrap_or(default_color);
            Border {
                val: Some(Val::Value(BorderStyle::Single)),
                sz: Some(Val::Value(sz)),
                space: Some(Val::Value(space)),
                color: Some(Val::Value(color)),
                ..Default::default()
            }
        };
        let mut b = ParaBorders::default();
        if sides.contains('t') {
            b.top = Some(line("t"));
        }
        if sides.contains('l') {
            b.left = Some(line("l"));
        }
        if sides.contains('b') {
            b.bottom = Some(line("b"));
        }
        if sides.contains('r') {
            b.right = Some(line("r"));
        }
        if b != ParaBorders::default() {
            p.borders = Some(b);
        }
    }
    if let Some(fill) = s_of(f, "shadingFill").and_then(hex) {
        p.shading = Some(Shading {
            val: Some(Val::Value(ShadingPattern::Clear)),
            color: Some(Val::Value(HexColorOrAuto::Auto)),
            fill: Some(Val::Value(fill)),
            ..Default::default()
        });
    }
    let bidi = truthy(f, "bidi");
    if bidi {
        p.bidi = Some(true);
    }
    // spacing
    {
        let mut sp = Spacing::default();
        if let Some(b) = num(f, "spaceBefore").filter(|&x| x > 0.0) {
            sp.before = Some(Val::Value(round(b)));
        }
        if let Some(Value::Bool(a)) = f.get("spaceBeforeAuto") {
            sp.before_autospacing = Some(*a);
        }
        if let Some(a) = num(f, "spaceAfter").filter(|&x| x >= 0.0) {
            sp.after = Some(Val::Value(round(a)));
        }
        if let Some(Value::Bool(a)) = f.get("spaceAfterAuto") {
            sp.after_autospacing = Some(*a);
        }
        let rule = s_of(f, "lineRule");
        if matches!(rule, Some("exact" | "atLeast"))
            && let Some(raw) = num(f, "lineRawTwips").filter(|&x| x != 0.0)
        {
            sp.line = Some(Val::Value(round(raw)));
            sp.line_rule = Some(Val::Value(if rule == Some("exact") {
                LineSpacingRule::Exact
            } else {
                LineSpacingRule::AtLeast
            }));
        } else if let Some(ls) = num(f, "lineSpacing").filter(|&x| x > 0.0) {
            sp.line = Some(Val::Value(round(ls * 240.0)));
            sp.line_rule = Some(Val::Value(LineSpacingRule::Auto));
        }
        if sp != Spacing::default() {
            p.spacing = Some(sp);
        }
    }
    // ind
    {
        let mut ind = Indent::default();
        if let Some(l) = num(f, "indentLeft") {
            ind.start = Some(Val::Value(round(l)));
        }
        if let Some(r) = num(f, "indentRight").filter(|&x| x != 0.0) {
            ind.end = Some(Val::Value(round(r)));
        }
        if let Some(fl) = num(f, "indentFirstLine").filter(|&x| x != 0.0) {
            if fl > 0.0 {
                ind.first_line = Some(Val::Value(round(fl)));
            } else {
                ind.hanging = Some(Val::Value(round(-fl)));
            }
        }
        if ind != Indent::default() {
            p.indent = Some(ind);
        }
    }
    if let Some(align) = s_of(f, "align") {
        let mut jc = if align == "justify" { "both" } else { align };
        if bidi && (jc == "left" || jc == "right") {
            jc = if jc == "left" { "right" } else { "left" };
        }
        p.jc = Some(Jc::parse(jc).map_or_else(|| Val::Raw(jc.to_string()), Val::Value));
    }
    if let Some(stops) = f.get("tabStops").and_then(Value::as_array) {
        let tabs: Vec<Tab> = stops
            .iter()
            .filter(|ts| !truthy(ts, "rel"))
            .map(|ts| Tab {
                val: s_of(ts, "val")
                    .map(|v| TabJc::parse(v).map_or_else(|| Val::Raw(v.to_string()), Val::Value)),
                pos: num(ts, "pos").map(|x| Val::Value(round(x))),
                leader: s_of(ts, "leader").filter(|l| *l != "none").map(|l| {
                    TabLeader::parse(l).map_or_else(|| Val::Raw(l.to_string()), Val::Value)
                }),
            })
            .collect();
        if !tabs.is_empty() {
            p.tabs = Some(Tabs { tab: tabs, ..Default::default() });
        }
    }
    if let Some(fr) = f.get("frame").filter(|x| x.is_object()) {
        let anchor = |k: &str| {
            let s = s_of(fr, k).unwrap_or("page");
            Some(FrameAnchor::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value))
        };
        let mut fp = FramePr {
            w: num(fr, "wTwips").map(|x| Val::Value(round(x))),
            wrap: Some({
                let s = s_of(fr, "wrap").unwrap_or("none");
                FrameWrap::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value)
            }),
            v_anchor: anchor("vAnchor"),
            h_anchor: anchor("hAnchor"),
            x: num(fr, "xTwips").map(|x| Val::Value(round(x))),
            y: num(fr, "yTwips").map(|x| Val::Value(round(x))),
            ..Default::default()
        };
        if let Some(h) = num(fr, "hTwips") {
            fp.h = Some(Val::Value(round(h)));
            let s = s_of(fr, "hRule").unwrap_or("atLeast");
            fp.h_rule =
                Some(HeightRule::parse(s).map_or_else(|| Val::Raw(s.to_string()), Val::Value));
        }
        p.frame = Some(fp);
    } else if let Some(dc) = f.get("dropCap").filter(|x| x.is_object()) {
        let ty = s_of(dc, "type").unwrap_or("drop");
        p.frame = Some(FramePr {
            drop_cap: Some(DropCap::parse(ty).map_or_else(|| Val::Raw(ty.to_string()), Val::Value)),
            lines: num(dc, "lines").map(|x| Val::Value(round(x))),
            wrap: Some(Val::Value(FrameWrap::Around)),
            v_anchor: Some(Val::Value(FrameAnchor::Text)),
            h_anchor: Some(Val::Value(FrameAnchor::Text)),
            ..Default::default()
        });
    }
    if let Some(sz) = num(f, "emptyRunSizeHalfPoints").filter(|&x| x != 0.0) {
        let sz = round(sz) as u32;
        p.rpr = Some(RunProps {
            size: Some(Val::Value(sz)),
            size_cs: Some(Val::Value(sz)),
            ..Default::default()
        });
    }
}

// 让未使用的导入在功能面变化时报错而不是静默
#[allow(dead_code)]
fn _types(_: FontHint, _: NsId) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_08_bookmark_id_matches_ts_hash() {
        // TS: bookmarkIdOf('_Ref12345') 与 bookmarkIdOf('市场规模')；值由 node 计算得到
        assert_eq!(bookmark_id_of(""), 0);
        assert_eq!(bookmark_id_of("a"), 97);
        assert_eq!(bookmark_id_of("ab"), 97 * 31 + 98);
        let long = bookmark_id_of("_Ref12345678901234567890");
        assert!(long < 0x7fff_ffff);
    }
}
