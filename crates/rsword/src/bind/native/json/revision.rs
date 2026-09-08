//! 修订的 JSON 投影（`BIND-02`，`MOD-09`）：`RevisionIndex` 经 `entries()` 投影为
//! `Vec<RevisionEntry>`；`RevKind` 是 `named_enum!`，`as_str` 即 JSON 值。

use crate::model::revision::{RevKind, RevOwner, RevisionEntry};
use crate::span::{FieldId, RevisionMeta};
use crate::xml::NodeId;

use super::{as_str_json, model_json};

as_str_json!(RevKind);

model_json! {
    /// 一条修订的元数据（`w:id` / `w:author` / `w:date`；定义在 L2，`SPAN-03`）。
    struct RevisionMeta(cx) test json_fields_cover_revision_meta {
        node => "node", NodeId = node;
        opt id => "id", String = id;
        opt author => "author", String = author;
        opt date => "date", String = date;
    }
}

model_json! {
    /// 承载修订的宿主（`MOD-09`）。
    enum RevOwner(cx) test json_fields_cover_rev_owner {
        /// 块级包裹所在的块容器。
        Block(NodeId) as "node" => "block";
        /// run 级包裹所在的段落 `w:p`。
        Inline(NodeId) as "node" => "inline";
        /// `w:r/w:rPr/w:rPrChange` 所在的 `w:r`。
        Run(NodeId) as "node" => "run";
        /// 段落标记 / `pPrChange` / `numberingChange` 所在的 `w:p`。
        ParaMark(NodeId) as "node" => "paraMark";
        Row(NodeId) as "node" => "row";
        Cell(NodeId) as "node" => "cell";
        Table(NodeId) as "node" => "table";
        /// `w:sectPr/w:sectPrChange` 所在的 `w:sectPr`。
        Section(NodeId) as "node" => "section";
        /// run 级删除包住字段指令区（`w:delInstrText`）的字段。
        Field(FieldId) as "field" => "field";
    }
}

model_json! {
    /// 一条修订（`MOD-09`，`RevisionIndex` 的条目）。
    struct RevisionEntry(cx) test json_fields_cover_revision_entry {
        id => "id", crate::model::RevisionId = id;
        part => "part", crate::package::PartId = part;
        kind => "kind", RevKind = kind;
        meta => "meta", RevisionMeta = meta;
        owner => "owner", RevOwner = owner;
        depth => "depth", u16 = depth;
        opt move_name => "moveName", String = move_name;
        opt pair => "pair", crate::model::RevisionId = pair;
    }
}
