//! 批注与脚注 / 尾注条目的 JSON 投影（`BIND-02`，`MOD-10`）：条目内容与正文同一构建器
//! （`blocks`，`docs/03` §6.7）。`text` / `rich` / `paragraphs` 是 TS 形态的半解析残留
//! （`COMPAT-02` 的 `richParas`），BIND-02 禁止项，M9 随 compat_ts 删除——跳过；`RichRun`
//! 只被 `rich` 用，不投影。`Comments` / `Notes` 容器不写表：`Document` 顶层直接投影 `items`。

use crate::model::block::Block;
use crate::model::notes::{Comment, Note, NoteKind};
use crate::xml::NodeId;

use super::model_json;

model_json! {
    /// 注释条目的种类（`w:type`；`MOD-10`）。带载荷的 `Other` 走内标签对象，
    /// `value` 是认不出的原字面。
    enum NoteKind(cx) test json_fields_cover_note_kind {
        /// 正文条目（无 `w:type`）。
        Normal => "normal";
        Separator => "separator";
        ContinuationSeparator => "continuationSeparator";
        ContinuationNotice => "continuationNotice";
        Other(String) as "value" => "other";
    }
}

model_json! {
    /// 一条脚注 / 尾注（`MOD-10`）。
    struct Note(cx) test json_fields_cover_note {
        /// `w:footnote` / `w:endnote`。
        node => "node", NodeId = node;
        id => "id", String = id;
        kind => "kind", NoteKind = kind;
        skip text, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        skip rich, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        /// 条目里没有任何 `w:footnoteRef` / `w:endnoteRef` run。
        flag no_ref_mark => "noRefMark" = no_ref_mark;
        /// 首段的 `w:pStyle`（真实 Word 的注释段落带「脚注文本」样式）。
        opt style_id => "styleId", String = style_id;
        skip paragraphs, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        /// 条目内容，与正文同一构建器（`MOD-01`）。
        blocks => "blocks", Vec<Block> = blocks;
    }
}

model_json! {
    /// 一条批注（`MOD-10`）：`comments.xml` 的声明值，已合并 `commentsExtended.xml` 的
    /// 回复关系与「已解决」、`commentsIds.xml` 的 durableId。
    struct Comment(cx) test json_fields_cover_comment {
        /// `w:comment`。
        node => "node", NodeId = node;
        id => "id", String = id;
        opt author => "author", String = author;
        opt initials => "initials", String = initials;
        opt date => "date", String = date;
        skip text, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        /// **最后一段**的 `w14:paraId`（`commentsExtended` 按它关联）。
        opt para_id => "paraId", String = para_id;
        /// 回复的父批注 id（由 `w15:paraIdParent` 反查）。
        opt parent_id => "parentId", String = parent_id;
        /// `w15:done`。
        flag done => "done" = done;
        /// `commentsIds.xml` 的 `w16cid:durableId`。
        opt durable_id => "durableId", String = durable_id;
        skip paragraphs, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        skip rich, "TS 形态半解析字段，BIND-02 禁止项；M9 随 compat_ts 删除";
        /// 条目内容，与正文同一构建器（`MOD-01`）。
        blocks => "blocks", Vec<Block> = blocks;
    }
}
