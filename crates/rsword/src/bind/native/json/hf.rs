//! 页眉页脚 part 的 JSON 投影（`BIND-02`，`MOD-01` 的 `hf_parts`，`docs/03` §6.7）：
//! 内容与正文同一构建器，`blocks` 直接复用块域的表。每 part 的三份索引（`SPAN-01`）
//! 是内部结构，不投影。

use crate::model::Block;
use crate::model::HfKind;
use crate::model::HfPart;
use crate::package::PartId;
use crate::xml::NodeId;

use super::model_json;

model_json! {
    /// 一个页眉或页脚 part（`MOD-01` 的 `hf_parts`）。
    struct HfPart(cx) test json_fields_cover_hf_part {
        part => "part", PartId = part;
        kind => "kind", HfKind = kind;
        /// `w:hdr` / `w:ftr`。
        root => "root", NodeId = root;
        /// 内容，与正文同一构建器（`docs/03` §6.7）。
        blocks => "blocks", Vec<Block> = blocks;
        skip idx, "内部 FieldIndex/SpanIndex/FlowMap 索引（SPAN-01），投影无意义";
        /// 含 `PAGE` 字段（`FLD-11`）或旧式 `w:pgNum` 元素。
        flag has_page_number => "hasPageNumber" = has_page_number;
        /// 含 `NUMPAGES` 字段。
        flag has_num_pages => "hasNumPages" = has_num_pages;
        /// 文字水印：第一个 `v:textpath/@string`（Word 的水印是页眉里的 VML 形状）。
        opt watermark => "watermark", String = watermark;
    }
}
