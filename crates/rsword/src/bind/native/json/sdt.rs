//! 内容控件的 JSON 投影（`BIND-02`，`MOD-08`）：`SdtInfo` 是 `w:sdt/w:sdtPr` 的声明值。
//! `SdtControl` / `SdtLock` 是 `sdt_enum!`，`as_str` 即 JSON 值。`SdtRefusal` 是 edit 侧
//! （`EDIT-03`）的拒绝理由，不是模型，不投影。

use crate::model::sdt::{DataBinding, DocPart, SdtControl, SdtInfo, SdtLock};
use crate::xml::NodeId;

use super::{as_str_json, model_json};

as_str_json!(SdtControl, SdtLock);

model_json! {
    /// `w:dataBinding`：控件内容绑定到 customXml part（`MOD-08`；有绑定的第一阶段只读，`EDIT-03`）。
    struct DataBinding(cx) test json_fields_cover_data_binding {
        opt prefix_mappings => "prefixMappings", String = prefix_mappings;
        opt xpath => "xpath", String = xpath;
        opt store_item_id => "storeItemId", String = store_item_id;
    }
}

model_json! {
    /// `w:docPartObj` / `w:docPartList` 的内容（`MOD-08`）。
    struct DocPart(cx) test json_fields_cover_doc_part {
        opt gallery => "gallery", String = gallery;
        opt category => "category", String = category;
        /// `w:docPartUnique`（三态 `OnOff`，缺省 false）。
        flag unique => "unique" = unique;
    }
}

model_json! {
    /// 最近的 `w:sdt` 祖先（`MOD-08`）：`sdtPr` 的声明值。
    struct SdtInfo(cx) test json_fields_cover_sdt_info {
        node => "node", NodeId = node;
        /// `w:alias/@w:val`：给人看的标题。
        opt alias => "alias", String = alias;
        /// `w:tag/@w:val`：给程序用的标签。
        opt tag => "tag", String = tag;
        /// `w:id/@w:val`。
        opt id => "id", i32 = id;
        control => "control", SdtControl = control;
        lock => "lock", SdtLock = lock;
        opt data_binding => "dataBinding", DataBinding = data_binding;
        /// `w:docPartObj` / `w:docPartList` 的内容（控件种类见 `control`）。
        opt doc_part => "docPart", DocPart = doc_part;
        /// `w:placeholder/w:docPart/@w:val`：占位文字所在的构建基块名。
        opt placeholder => "placeholder", String = placeholder;
        /// `w:showingPlcHdr`：当前显示的是占位文字而不是真实内容。
        flag showing_placeholder => "showingPlaceholder" = showing_placeholder;
    }
}
