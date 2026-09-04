//! `EDIT-02` 位置与定位：`InlinePos { para, offset }`，偏移是段坐标流（`MOD-06`）中的 UTF-16 code unit。

use std::ops::Range;

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::block::TextBlock;
use crate::model::inline::{Inline, SegmentKind};
use crate::xml::NodeId;

/// UTF-16 code unit 偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Utf16Offset(pub u32);

/// 段内位置：`para` 是 `w:p`，`0 ≤ offset ≤ len`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InlinePos {
    pub para: NodeId,
    pub offset: Utf16Offset,
}

impl InlinePos {
    pub fn new(para: NodeId, offset: u32) -> Self {
        Self { para, offset: Utf16Offset(offset) }
    }
}

/// 定位结果（`EDIT-02` 的 `Loc`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    /// 落在两个 inline 之间：前面有 `index` 个 inline（`0 ≤ index ≤ len`）。
    Boundary { index: usize },
    /// 落在 `inlines[inline]`（Run）内部两段之间：`segment` 是后一段的下标（≥ 1）。
    InRun { inline: usize, segment: usize },
    /// 落在 `Text` / `DelText` 段内部（不含两端），`byte` 是段文本里的 UTF-8 字节偏移。
    InText { inline: usize, segment: usize, byte: usize },
}

/// 每个 inline 在坐标流中的区间。
pub fn inline_spans(tb: &TextBlock) -> Vec<Range<u32>> {
    let mut cum = 0u32;
    tb.inlines
        .iter()
        .map(|i| {
            let s = cum;
            cum += i.utf16_len();
            s..cum
        })
        .collect()
}

/// `locate(pos)`：顺序累加 inlines 的 UTF-16 长度。偏移指向代理对中间 → `EDIT_SPLIT_SURROGATE`；
/// 越界或落在非文本原子段内部 → `EDIT_BAD_POSITION`。
pub fn locate(tb: &TextBlock, offset: Utf16Offset) -> Result<Loc> {
    let target = offset.0;
    let mut cum = 0u32;
    for (i, inline) in tb.inlines.iter().enumerate() {
        if target == cum {
            return Ok(Loc::Boundary { index: i });
        }
        let len = inline.utf16_len();
        if target < cum + len {
            let Inline::Run(run) = inline else {
                return Err(Error::edit(DiagCode::EditBadPosition, "偏移落在原子内部"));
            };
            let mut scum = cum;
            for (k, seg) in run.segments.iter().enumerate() {
                if target == scum && k > 0 {
                    return Ok(Loc::InRun { inline: i, segment: k });
                }
                if target < scum + seg.utf16_len {
                    if !matches!(seg.kind, SegmentKind::Text | SegmentKind::DelText) {
                        return Err(Error::edit(DiagCode::EditBadPosition, "偏移落在非文本段内部"));
                    }
                    let byte = utf16_to_byte(run.segment_text(seg), target - scum)?;
                    return Ok(Loc::InText { inline: i, segment: k, byte });
                }
                scum += seg.utf16_len;
            }
            unreachable!("segments cover the run's coordinate range");
        }
        cum += len;
    }
    if target == cum {
        return Ok(Loc::Boundary { index: tb.inlines.len() });
    }
    Err(Error::edit(DiagCode::EditBadPosition, format!("偏移 {target} 超出段落长度 {cum}")))
}

/// 段文本里 UTF-16 偏移 → 字节偏移；落在代理对中间 → `EDIT_SPLIT_SURROGATE`。
pub fn utf16_to_byte(text: &str, units: u32) -> Result<usize> {
    let mut cum = 0u32;
    for (b, c) in text.char_indices() {
        if cum == units {
            return Ok(b);
        }
        let n = c.len_utf16() as u32;
        if cum + n > units {
            return Err(Error::edit(DiagCode::EditSplitSurrogate, "偏移落在代理对中间"));
        }
        cum += n;
    }
    if cum == units {
        return Ok(text.len());
    }
    Err(Error::edit(DiagCode::EditBadPosition, "偏移超出段文本"))
}
