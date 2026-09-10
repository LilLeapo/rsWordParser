//! 参考文献源的 JSON 投影（`BIND-02`，`MOD-10`）：`Source` 只收 TS `SourceInfo` 的六个字段
//! 加 `node`；未建模的 `b:*` 域留在 DOM 里，写回时原字节不动（`SAVE-07` 的权威列表只重建
//! 变了的条目）。

use crate::model::Source;
use crate::xml::NodeId;

use super::model_json;

model_json! {
    /// 一条文献源（`MOD-10`，TS `SourceInfo`）。
    struct Source(cx) test json_fields_cover_source {
        /// `b:Tag`：引文里引用它的短标签，也是权威列表的键。
        tag => "tag", String = tag;
        /// `b:SourceType`（`JournalArticle` / `Book` / `InternetSite` …）；缺失按 TS 记 `Misc`。
        kind => "kind", String = kind;
        /// `b:Corporate`，否则第一个 `b:Person` 的 `"Last, First"`（两者都缺就是空串）。
        author => "author", String = author;
        title => "title", String = title;
        year => "year", String = year;
        /// `b:Publisher` → `b:JournalName` → `b:InternetSiteTitle`，取第一个有的。
        opt publisher => "publisher", String = publisher;
        opt url => "url", String = url;
        /// 这条 `b:Source` 元素本身（写回时未变的条目原字节保留）。
        node => "node", NodeId = node;
    }
}
