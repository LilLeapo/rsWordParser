//! 墨迹批注（`aidocs-ink`）的 JSON 投影（`BIND-02`，`MOD-06` / `MOD-11`）：编辑器存成
//! 浮动图片 run 的手绘笔迹，对分类与坐标流不可见（`Run.segments` 里是长度 0 的
//! `SegmentKind::Ink`），几何与载荷收在 `InkInfo`（`Document.inks`）。

use crate::model::ink::InkInfo;
use crate::xml::NodeId;

use super::model_json;

model_json! {
    /// 一条墨迹批注（`MOD-06`，TS `InkRunMatch` 的模型侧）。
    struct InkInfo(cx) test json_fields_cover_ink_info {
        /// 锚定的段落（`w:p`，可能在单元格里）。
        para => "para", NodeId = para;
        /// 承载它的 `w:r`（`RemoveInks` 删的就是它）。
        run => "run", NodeId = run;
        /// `w:drawing`。
        drawing => "drawing", NodeId = drawing;
        /// `wp:positionH` / `wp:positionV` 的 `wp:posOffset`（EMU），投成 `[x, y]`。
        offset_emu => "offsetEmu", (i64, i64) = offset_emu;
        /// `wp:extent` 的 `cx` / `cy`（EMU），投成 `[w, h]`。
        extent_emu => "extentEmu", (i64, i64) = extent_emu;
        /// 第一个 `a:blip/@r:embed`；没有 → 缺席。
        opt rel_id => "relId", String = rel_id;
        /// `wp:docPr/@descr` 解码后的载荷（不透明）；缺失或空 → 缺席。
        opt payload => "payload", String = payload;
    }
}
