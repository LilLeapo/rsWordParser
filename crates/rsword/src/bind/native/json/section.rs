//! 节的 JSON 投影（`BIND-02`，`MOD-10`）：`SectionInfo` 是 `w:sectPr` 的**声明值**（继承在
//! `resolve::section`，`RES-10`，不在这里）。`SectionGeom` / `Sections` 是 resolve 派生视图
//! 与内部查询缓存，不投影。`HfKind` / `HfVariant` 是 `named_enum!`，`as_str` 即 JSON 值。

use std::ops::Range;

use crate::model::block::Revision;
use crate::model::{HfKind, HfVariant, SectionInfo, SectionOwner};
use crate::semantic::props::SectionProps;
use crate::xml::NodeId;

use super::{as_str_json, model_json};

as_str_json!(HfKind, HfVariant);

model_json! {
    /// 一个 `w:sectPr` 长在哪儿（`MOD-10`）。
    enum SectionOwner(cx) test json_fields_cover_section_owner {
        /// `w:body` 的末尾子元素：最后一节。
        Body => "body";
        /// 分节段落的 `pPr/sectPr`；`NodeId` 是那个 `w:p`。
        Paragraph(NodeId) as "node" => "paragraph";
        /// 文档里没有任何 `w:sectPr`：隐式节。
        Implicit => "implicit";
    }
}

model_json! {
    /// 一个节（`MOD-10`）。全是**声明值**：继承看 `resolve::section`（`RES-10`）。
    struct SectionInfo(cx) @partial test json_fields_cover_section_info {
        /// `w:sectPr`；隐式节为 `None`。
        opt node => "node", NodeId = node;
        /// 装箱的节属性表（`PROP-01`，`schema/props/section.toml`）。
        props => "props", Box<SectionProps> = props;
        owner => "owner", SectionOwner = owner;
        /// 属于本节的块在 `Document.main` 里的下标区间；分节段落自己算本节的最后一块。
        block_range => "blockRange", Range<usize> = block_range;
        /// `sectPr/sectPrChange` 的旧值快照（`MOD-09`）。
        revisions => "revisions", Vec<Revision> = revisions;
        // `end_offset`：私有字段（`@partial` 解构跳过），是 `Sections::at` / `section_of`
        // 的内部查询键（`w:sectPr` 的结束字节偏移；隐式节为 `u32::MAX`），对投影无意义。
    }
}
