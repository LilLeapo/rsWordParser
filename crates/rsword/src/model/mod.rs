//! L3 文档模型（`spec/06-model.md`，`docs/03` §6.3–6.8）。
//!
//! `Document` 是 DOM + Span 的**语义投影**：可增量 `refresh`（M2），但任何时刻
//! `Document::rebuild(&pkg)` 必须与增量结果相等（`MOD-13`，测试用它作 oracle）。
//! `Run` 与物理 `w:r` 一一对应，逻辑 run 合并只发生在投影层（`compat_ts`）。
//!
//! | 模块 | 内容 |
//! | --- | --- |
//! | [`inline`] | `Inline` / `Run` / `Segment` 与坐标流（`MOD-06`） |
//! | [`block`] | `Block` / `TextBlock` / `ProtectedBlock` / `Revision`（`MOD-02/08/09`） |
//! | [`facts`] | `ParagraphFacts`（`MOD-04`） |
//! | [`classify`] | 分类规则表与 `TextKind` 判定（`MOD-05/03`） |
//! | [`build`] | `Document` 与 `rebuild`（`MOD-01/13`） |
//! | [`decl`] / [`theme`] | 声明模型（`MOD-10`） |

pub mod block;
pub mod build;
pub mod classify;
pub mod decl;
pub mod drawing;
pub mod facts;
pub mod inline;
pub mod theme;
pub mod units;

pub use block::{
    Block, ImageBlock, ListRef, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TableBlock,
    TextBlock, TextKind,
};
pub use build::Document;
pub use classify::{BodyClass, ParaClass, classify_body_child, classify_paragraph, text_kind};
pub use decl::{
    AbstractNum, Compat, CompatFacts, CompatSetting, DocDefaults, Font, FontTable, Level,
    LevelOverride, Num, Numbering, OwnHeadingLevel, Settings, Style, StyleType, Styles,
    TableStylePr,
};
pub use drawing::{
    AnchorGeom, Display, Dist, DocPr, DrawingDisplay, Extent, ImageDisplay, LineDisplay, Position,
    RectFrac, Wrap,
};
pub use facts::{
    DrawingFacts, DrawingKind, MathFacts, ParagraphFacts, PictFacts, PictKind, RevisionFacts,
};
pub use inline::{
    AtomKind, BreakKind, Inline, InlineAtom, Link, LinkTarget, OBJECT_REPLACEMENT, RevisionCtx,
    RevisionMeta, Run, Segment, SegmentKind,
};
pub use theme::{ColorScheme, FontScheme, FontSlots, Theme, ThemeSlot};

#[cfg(test)]
mod tests;
