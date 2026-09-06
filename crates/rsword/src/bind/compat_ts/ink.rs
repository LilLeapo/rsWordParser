//! `ParsedDoc.inks[]`（TS `findInkRuns` + `parse.ts` 的墨迹段，任务 6.8）。
//!
//! `anchorIndex` 是承载段落所在**块级元素**的 `docxIndex`（单元格里的墨迹算到表格块上）；px 值是
//! `EMU / 9525` **不取整**（TS 浮点；整除时按整数输出）；`dataUrl` 经主 part 的媒体表解析，解析不出 → `null`。

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::blocks::Ctx;
use crate::model::units::EMU_PER_PX;
use crate::xml::NodeId;

/// EMU → px 的 JSON 值：整除给整数，否则给浮点（TS `emu / 9525`）。
fn px(emu: i64) -> Value {
    let per = EMU_PER_PX as i64;
    if emu % per == 0 { json!(emu / per) } else { json!(emu as f64 / EMU_PER_PX) }
}

/// `inks[]`；`elements` 是 `docxIndex → 节点`（[`super::blocks::element_nodes`]）。
pub(super) fn inks_json(ctx: &Ctx<'_>, elements: &[NodeId]) -> Value {
    let dom = ctx.dom;
    let index: HashMap<NodeId, usize> = elements.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let mut out = Vec::new();
    for ink in &ctx.doc.inks {
        let Some(&anchor) =
            std::iter::once(ink.para).chain(dom.ancestors(ink.para)).find_map(|n| index.get(&n))
        else {
            continue;
        };
        let mut o = Map::new();
        o.insert("anchorIndex".into(), json!(anchor));
        o.insert("offsetXPx".into(), px(ink.offset_emu.0));
        o.insert("offsetYPx".into(), px(ink.offset_emu.1));
        o.insert("widthPx".into(), px(ink.extent_emu.0));
        o.insert("heightPx".into(), px(ink.extent_emu.1));
        let data_url = ink
            .rel_id
            .as_deref()
            .and_then(|r| ctx.media.get(r))
            .map_or(Value::Null, |m| Value::String(m.url.clone()));
        o.insert("dataUrl".into(), data_url);
        o.insert("payload".into(), ink.payload.clone().map_or(Value::Null, Value::String));
        out.push(Value::Object(o));
    }
    Value::Array(out)
}
