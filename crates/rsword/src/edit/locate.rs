//! 编辑位置（`EDIT-02`，`docs/03` §8.1）。
//!
//! 偏移单位对外统一为 UTF-16 code unit（`Utf16Offset`）。`InlinePos` 的 `para` 在当前
//! 实现中必须是 `Document::main` 里的可编辑文本段落（`TextBlock`）；页眉页脚 / 单元格 /
//! 脚注段落待 `Document` 扩展后自然接入同一 `locate`。
//!
//! `Boundary.index` 是段内按坐标流展开的边界序号：每个 `Run.segment` 与每个原子 inline
//! 都产生一个尾边界；同一坐标允许有多个边界（零长度结构段），扫描时先到者先得。
//! 后续 `InsertText` 用同一扫描顺序把边界序号映射回 DOM 插入点。

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::Document;
use crate::model::inline::{Inline, SegmentKind};
use crate::package::Package;
use crate::xml::{LocalName, NodeId, QName};

/// 坐标流内的 UTF-16 code unit 偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16Offset(pub u32);

/// 段落内联坐标流中的位置（`EDIT-02`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InlinePos {
    pub para: NodeId,
    pub offset: Utf16Offset,
}

impl InlinePos {
    pub const fn new(para: NodeId, offset: u32) -> Self {
        Self { para, offset: Utf16Offset(offset) }
    }
}

/// 块级位置（`EDIT-02`）。任务 1.11 只定义坐标系，解析在 `InsertBlock` / `DeleteBlock`
/// 实现时接入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockPos {
    Start(NodeId),
    After(NodeId),
    End(NodeId),
}

/// 定位结果（`EDIT-02`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    /// 落在 `Text` / `DelText` 段的内部。
    ///
    /// `byte_offset` 是相对该段在 `Run.text` 中的贡献字符串（`Run::segment_text`）的
    /// UTF-8 字节偏移，**不是**段元素在原 XML part 中的字节偏移。
    InText {
        run: NodeId,
        /// `w:t` 或 `w:delText` 元素（模型 `Segment.node`）。
        segment: NodeId,
        byte_offset: u32,
    },
    /// 落在 inline / segment 边界，`index` 的含义见模块注释。
    Boundary { index: usize },
}

pub(crate) fn locate(pkg: &Package, document: &Document, pos: InlinePos) -> Result<Loc> {
    let dom = pkg.part(document.main_part).dom().expect("main part is parsed when opened");
    if pos.para.0 as usize >= dom.node_count() || !dom.is(pos.para, QName::w(LocalName::P)) {
        return Err(invalid_position(format!("{} is not a paragraph", pos.para.0)));
    }
    let block = document.text_blocks().find(|b| b.node == pos.para).ok_or_else(|| {
        invalid_position(format!(
            "paragraph {} is not an editable text block in the main body",
            pos.para.0
        ))
    })?;
    if pos.offset.0 > block.utf16_len() {
        return Err(invalid_position(format!(
            "offset {} is beyond paragraph length {}",
            pos.offset.0,
            block.utf16_len()
        )));
    }
    locate_inlines(&block.inlines, pos.offset.0)
}

fn locate_inlines(inlines: &[Inline], offset: u32) -> Result<Loc> {
    let mut coord = 0_u32;
    let mut boundary = 0_usize;

    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                let mut rel_cursor = 0_u32;
                for seg in &run.segments {
                    let abs_start = coord + rel_cursor;
                    let abs_end = abs_start + seg.utf16_len;

                    if offset == abs_start {
                        return Ok(Loc::Boundary { index: boundary });
                    }
                    if offset > abs_start && offset < abs_end {
                        let is_text = matches!(seg.kind, SegmentKind::Text | SegmentKind::DelText);
                        let Some(segment_text) = is_text.then(|| run.segment_text(seg)) else {
                            return Err(invalid_position(format!(
                                "offset {offset} points into non-text segment {}",
                                seg.node.0
                            )));
                        };
                        let inner = offset - abs_start;
                        let byte = utf16_to_byte(segment_text, inner)
                            .map_err(|()| split_surrogate(offset))?;
                        return Ok(Loc::InText {
                            run: run.node,
                            segment: seg.node,
                            byte_offset: byte as u32,
                        });
                    }

                    boundary += 1;
                    rel_cursor = abs_end - coord;
                }
                coord += run.utf16_len;
            }
            Inline::Field { .. } | Inline::Atom(_) => {
                if inline.utf16_len() == 0 {
                    boundary += 1;
                    continue;
                }
                let abs_end = coord + inline.utf16_len();
                if offset == coord {
                    return Ok(Loc::Boundary { index: boundary });
                }
                if offset > coord && offset < abs_end {
                    return Err(invalid_position(format!(
                        "offset {offset} points into an atomic inline of length {}",
                        inline.utf16_len()
                    )));
                }
                boundary += 1;
                coord = abs_end;
            }
        }
    }

    if offset == coord {
        return Ok(Loc::Boundary { index: boundary });
    }
    Err(invalid_position(format!("offset {offset} is not on a UTF-16 boundary")))
}

/// UTF-16 偏移 → 字符串内的 UTF-8 字节偏移；目标切进代理对中间返回 `Err(())`。
fn utf16_to_byte(s: &str, target_units: u32) -> std::result::Result<usize, ()> {
    if target_units == 0 {
        return Ok(0);
    }
    let mut units = 0_u32;
    for (byte, ch) in s.char_indices() {
        let next = units + ch.len_utf16() as u32;
        if next > target_units {
            // 只有 target 落在 ch 的两个 UTF-16 单元之间才会出现 next > target 但
            // units < target；此时点到低代理位，属于 EDIT_SPLIT_SURROGATE。
            return Err(());
        }
        if next == target_units {
            return Ok(byte + ch.len_utf8());
        }
        units = next;
    }
    if target_units == utf16_len(s) { Ok(s.len()) } else { Err(()) }
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

fn invalid_position(message: impl Into<String>) -> Error {
    Error::EditPlan { code: DiagCode::EditInvalidPosition, message: message.into() }
}

fn split_surrogate(offset: u32) -> Error {
    Error::EditPlan {
        code: DiagCode::EditSplitSurrogate,
        message: format!("offset {offset} splits a surrogate pair"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_to_byte_handles_surrogate_boundaries() {
        let s = "A😀B";
        assert_eq!(utf16_to_byte(s, 0), Ok(0));
        assert_eq!(utf16_to_byte(s, 1), Ok(1));
        assert_eq!(utf16_to_byte(s, 2), Err(()));
        assert_eq!(utf16_to_byte(s, 3), Ok(5));
        assert_eq!(utf16_to_byte(s, 4), Ok(6));
        assert_eq!(utf16_to_byte(s, 5), Err(()));
    }
}
