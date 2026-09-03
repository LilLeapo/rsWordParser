//! 词法区间（`XML-03`）。所有区间是相对 part 原字节（或转码后字节）的 UTF-8 字节偏移（`XML-15`）。

use std::ops::Range;

/// 节点在原文中的位置与写法。
///
/// | 字段 | 元素 | 文本 | Opaque |
/// | --- | --- | --- | --- |
/// | `range` | `<` 到闭标签 `>` 之后（自闭合到 `/>` 之后） | 文本区间 | 整段原文 |
/// | `open` | 开标签 `<…>`；自闭合时等于 `range` | 空 | 空 |
/// | `close` | `</…>`；自闭合时为空 | 空 | 空 |
///
/// 元素与属性的原始限定名分别在 `Element::lex_name` 与 `Attr::lex_name`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lex {
    pub range: Range<u32>,
    pub open: Range<u32>,
    pub close: Range<u32>,
}

impl Lex {
    /// 文本 / Opaque：`open` 与 `close` 为空区间（分别在 `range` 两端）。
    pub fn leaf(range: Range<u32>) -> Self {
        let (start, end) = (range.start, range.end);
        Self { range, open: start..start, close: end..end }
    }

    pub fn is_self_closing(&self) -> bool {
        self.close.is_empty() && self.open == self.range
    }

    /// 子节点必须落在的区间：`[open.end, close.start)`。
    pub fn content(&self) -> Range<u32> {
        if self.is_self_closing() {
            self.range.end..self.range.end
        } else {
            self.open.end..self.close.start
        }
    }
}

pub(crate) fn r32(v: usize) -> u32 {
    u32::try_from(v).expect("offset exceeds u32 (PKG-02 limits guarantee < 512 MiB)")
}

pub(crate) fn urange(r: &Range<u32>) -> Range<usize> {
    r.start as usize..r.end as usize
}
