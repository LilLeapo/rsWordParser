//! `EDIT-02` 位置与定位：`InlinePos { part, para, offset }`，偏移是段坐标流（`MOD-06`）中的
//! UTF-16 code unit。
//!
//! `part` 是**哪个 XML part**（任务 5.5）：`None` = 主 part（正文），`Some` = 页眉页脚 part、
//! 注释 / 批注条目所在的 part。`NodeId` 只在自己 part 的 DOM 里有意义，所以位置必须带上它——
//! 不带的话页眉里的段落节点会被当成正文里的另一个节点（同 `spec/16` 分层决策第 1 条）。

use std::ops::Range;

use crate::diag::DiagCode;
use crate::error::{Error, Result};
use crate::model::Inline;
use crate::model::SegmentKind;
use crate::model::TextBlock;
use crate::package::PartId;
use crate::xml::NodeId;

/// UTF-16 code unit 偏移。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    ::serde::Serialize,
    ::serde::Deserialize,
)]
#[serde(transparent)]
pub struct Utf16Offset(pub u32);

/// 段内位置：`para` 是 `w:p`，`0 ≤ offset ≤ len`；`part` 为 `None` 表示主 part。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InlinePos {
    /// 段落所在的 part；`None` = 主 part。
    pub part: Option<PartId>,
    pub para: NodeId,
    pub offset: Utf16Offset,
}

impl InlinePos {
    /// 主 part（正文）里的位置。
    pub fn new(para: NodeId, offset: u32) -> Self {
        Self { part: None, para, offset: Utf16Offset(offset) }
    }

    /// 指定 part 里的位置（页眉页脚 / 注释 / 批注条目）。
    pub fn in_part(part: PartId, para: NodeId, offset: u32) -> Self {
        Self { part: Some(part), para, offset: Utf16Offset(offset) }
    }

    /// 同一个 part 里的另一个偏移。
    pub fn with_offset(self, offset: u32) -> Self {
        Self { offset: Utf16Offset(offset), ..self }
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
