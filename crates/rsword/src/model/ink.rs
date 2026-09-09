//! 墨迹（`aidocs-ink` 批注层，`MOD-06` / `MOD-11`，`spec/17` 任务 6.8）。
//!
//! 编辑器把手绘笔迹存成**自己写进文档的浮动图片 run**：`w:r/w:drawing/wp:anchor`，`wp:docPr/@name` 以
//! `aidocs-ink` 开头、`@descr` 带笔迹向量（不透明载荷）。Word 把它当普通浮动图片画；只有编辑器把它还原成
//! 可再编辑的笔迹层。所以它对**分类与坐标流都不可见**（TS 在 `detect` 之前 `stripInkRuns`）：被批注的段落
//! 仍是可编辑正文，`Run.segments` 里它是长度 0 的 [`SegmentKind::Ink`]；几何与载荷收在 [`InkInfo`]。

use crate::model::block::Block;
use crate::model::inline::{Inline, SegmentKind};
use crate::model::table::Blocks;
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// `wp:docPr/@name` 的前缀（TS `INK_NAME_PREFIX`）。
pub const INK_NAME_PREFIX: &str = "aidocs-ink";

/// 一条墨迹批注（TS `InkRunMatch` 的模型侧）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InkInfo {
    /// 锚定的段落（`w:p`，可能在单元格里）。
    pub para: NodeId,
    /// 承载它的 `w:r`（`RemoveInks` 删的就是它）。
    pub run: NodeId,
    /// `w:drawing`。
    pub drawing: NodeId,
    /// `wp:positionH` / `wp:positionV` 的 `wp:posOffset`（EMU；缺失或非数字 → 0，TS `parseInt || 0`）。
    pub offset_emu: (i64, i64),
    /// `wp:extent` 的 `cx` / `cy`（EMU；同上）。
    pub extent_emu: (i64, i64),
    /// 第一个 `a:blip/@r:embed`；没有 → `None`（compat 的 `dataUrl: null`）。
    pub rel_id: Option<String>,
    /// `wp:docPr/@descr` 解码后的载荷；缺失或空 → `None`（TS `descr ? … : null`）。
    pub payload: Option<String>,
}

/// TS `parseInt(s, 10) || 0`：可选正负号 + 前导十进制数字，其余忽略；没有数字 → 0。
pub fn lenient_int(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut v: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        v = v.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    if neg { -v } else { v }
}

fn wp(l: LocalName) -> QName {
    QName::new(NsId::Wp, l)
}

fn plain(l: LocalName) -> QName {
    QName::new(NsId::None, l)
}

/// 这个 `w:drawing` 是不是墨迹：`wp:anchor` 里有 `wp:docPr/@name` 以 [`INK_NAME_PREFIX`] 开头
/// （TS `stripInkRuns` / `findInkRuns` 只认 `wp:anchor` 形态的 run）。
pub fn is_ink_drawing(dom: &Dom, drawing: NodeId) -> bool {
    dom.semantic_children(drawing)
        .filter(|&c| dom.is(c, wp(LocalName::Anchor)))
        .any(|anchor| dom.semantic_children(anchor).any(|c| is_ink_doc_pr(dom, c)))
}

fn is_ink_doc_pr(dom: &Dom, node: NodeId) -> bool {
    dom.is(node, wp(LocalName::DocPr))
        && dom
            .attr_value(node, plain(LocalName::Name))
            .is_some_and(|v| v.starts_with(INK_NAME_PREFIX))
}

/// 读一条墨迹的几何与载荷（调用方已确认 [`is_ink_drawing`]）。
pub fn ink_info(dom: &Dom, para: NodeId, run: NodeId, drawing: NodeId) -> InkInfo {
    let mut info = InkInfo {
        para,
        run,
        drawing,
        offset_emu: (0, 0),
        extent_emu: (0, 0),
        rel_id: None,
        payload: None,
    };
    let Some(anchor) = dom.semantic_children(drawing).find(|&c| dom.is(c, wp(LocalName::Anchor)))
    else {
        return info;
    };
    let pos_offset = |n: NodeId| {
        dom.semantic_children(n)
            .find(|&c| dom.is(c, wp(LocalName::PosOffset)))
            .and_then(|c| dom.semantic_children(c).find_map(|t| dom.text(t)))
            .map_or(0, |t| lenient_int(&t))
    };
    let num = |n: NodeId, l: LocalName| dom.attr_value(n, plain(l)).map_or(0, |v| lenient_int(&v));
    for c in dom.semantic_children(anchor) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::Wp {
            continue;
        }
        match name.local {
            LocalName::PositionH => info.offset_emu.0 = pos_offset(c),
            LocalName::PositionV => info.offset_emu.1 = pos_offset(c),
            LocalName::Extent => info.extent_emu = (num(c, LocalName::Cx), num(c, LocalName::Cy)),
            LocalName::DocPr => {
                info.payload = dom
                    .attr_value(c, plain(LocalName::Descr))
                    .filter(|v| !v.is_empty())
                    .map(|v| v.into_owned());
            }
            _ => {}
        }
    }
    info.rel_id = dom
        .semantic_descendants(anchor)
        .find(|&n| dom.is(n, QName::new(NsId::A, LocalName::Blip)))
        .and_then(|b| dom.attr_value(b, QName::new(NsId::R, LocalName::Embed)))
        .map(|v| v.into_owned());
    info
}

/// 主 part 全部块里的墨迹，文档序（`Document.inks`；`rebuild` 与 `refresh_blocks` 都用它重算）。
///
/// 文本块从 `Run.segments` 的 [`SegmentKind::Ink`] 取；图片块 / 只读块没有内联模型，扫子树（TS 用正则扫
/// 每个块的 `originalXml`，表格 / 只读块里的墨迹同样算）。
pub fn collect_inks(dom: &Dom, blocks: &[Block]) -> Vec<InkInfo> {
    let mut out = Vec::new();
    for b in Blocks::over(blocks) {
        match b {
            Block::Text(tb) => {
                let mut stack: Vec<&Inline> = tb.inlines.iter().rev().collect();
                while let Some(inl) = stack.pop() {
                    match inl {
                        Inline::Run(r) => {
                            for seg in &r.segments {
                                if seg.kind == SegmentKind::Ink {
                                    out.push(ink_info(dom, tb.node, r.node, seg.node));
                                }
                            }
                        }
                        Inline::Field { result, .. } => stack.extend(result.iter().rev()),
                        Inline::Atom(_) => {}
                    }
                }
            }
            // 表格块的单元格段落由 `Blocks` 迭代器展开成文本块，这里只剩没有内联模型的块
            Block::Table(_) => {}
            Block::Image(_) | Block::Protected(_) => scan_subtree(dom, b.node(), &mut out),
        }
    }
    out
}

/// 没有内联模型的块：直接在子树里找墨迹 run。
fn scan_subtree(dom: &Dom, root: NodeId, out: &mut Vec<InkInfo>) {
    for n in dom.semantic_descendants(root) {
        if !dom.is(n, QName::w(LocalName::Drawing)) || !is_ink_drawing(dom, n) {
            continue;
        }
        let Some(run) = dom.ancestors(n).find(|&a| dom.is(a, QName::w(LocalName::R))) else {
            continue;
        };
        let Some(para) = dom.ancestors(run).find(|&a| dom.is(a, QName::w(LocalName::P))) else {
            continue;
        };
        out.push(ink_info(dom, para, run, n));
    }
}

#[cfg(test)]
mod tests {
    use super::lenient_int;

    #[test]
    fn lenient_int_follows_parse_int() {
        assert_eq!(lenient_int("381000"), 381_000);
        assert_eq!(lenient_int("-95250"), -95_250);
        assert_eq!(lenient_int("abc"), 0);
        assert_eq!(lenient_int("-abc"), 0);
        assert_eq!(lenient_int("12abc"), 12);
        assert_eq!(lenient_int(" +7"), 7);
        assert_eq!(lenient_int(""), 0);
    }
}
