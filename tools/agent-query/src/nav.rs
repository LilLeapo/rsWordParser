//! AGENT-03/05：仅从规范投影与模型读取，范围在返回载荷前裁剪。
use crate::{Result, budget::Budget, error};
use crate::{cursor::Registry, paging};
use rsword::agent::anchors::Anchor;
use rsword::agent::anchors::ObjectRef;
use rsword::agent::text::FlowRange;
use rsword::agent::text::Projection;
use rsword::model::Document;
use rsword::package::PartId;
use rsword::xml::NodeId;
use serde_json::{Value, json};
use std::ops::Range;
#[derive(Default)]
pub struct Pages {
    registry: Registry,
}
impl Pages {
    #[allow(clippy::too_many_arguments)]
    pub fn page(
        &mut self,
        snapshot: &str,
        config: &str,
        rows: &[Value],
        range: Value,
        budget: Budget,
        cursor: Option<&str>,
        max_rows: usize,
    ) -> Result<Value> {
        let units: Vec<_> =
            rows.iter().cloned().enumerate().map(|(i, v)| paging::Unit::record(v, i)).collect();
        paging::page(
            &mut self.registry,
            snapshot,
            "records",
            config,
            &units,
            false,
            range,
            budget,
            cursor,
            max_rows,
        )
    }
}
fn object_range(p: &Projection, o: &ObjectRef) -> Result<Range<u32>> {
    p.objects
        .get(&o.key())
        .map(|o| o.range.clone())
        .ok_or_else(|| error("AGENT_NOT_PROJECTED", "对象未投影"))
}
fn first_sentence(s: &str) -> String {
    s.split_inclusive(['.', '!', '?', '。', '！', '？', '\n']).next().unwrap_or("").trim().into()
}
/// 所有记录仍留在内部；调用方经 Pages 返回受预算限制的页。
pub fn outline(p: &Projection, doc: &Document, levels: Range<u8>) -> Result<Vec<Value>> {
    if levels.start < 1 || levels.end > 10 || levels.start >= levels.end {
        return Err(error("BIND_BAD_ARGUMENT", "标题级别应在 1..=9 内"));
    }
    let mut out = vec![];
    for f in &p.flows {
        let headings: Vec<_> = f
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                doc.text_block_in(PartId(o.part), NodeId(o.node)).and_then(|t| match t.kind {
                    rsword::model::TextKind::Heading { level } => Some((i, o, t, level)),
                    _ => None,
                })
            })
            .collect();
        if headings.is_empty() {
            for start in (0..f.blocks.len()).step_by(32) {
                let end = (start + 32).min(f.blocks.len());
                let objects = &f.blocks[start..end];
                let mut title = String::new();
                for o in objects {
                    if let Some(t) = doc.text_block_in(PartId(o.part), NodeId(o.node)) {
                        title = first_sentence(&t.text());
                        if !title.is_empty() {
                            break;
                        }
                    }
                }
                if title.is_empty() {
                    title = format!("{} 个内容块（{}）", objects.len(), objects[0].kind);
                }
                let r = object_range(p, &objects[0])?.start
                    ..object_range(p, objects.last().unwrap())?.end;
                out.push(json!({"kind":"group","object":objects[0],"title":title,"parent":null,"blockRange":{"start":start,"end":end},"blockCount":end-start,"charCount":r.end-r.start}));
            }
        } else {
            for (i, o, t, level) in &headings {
                if !levels.contains(level) {
                    continue;
                }
                let end = headings
                    .iter()
                    .find(|(j, _, _, l)| j > i && l <= level)
                    .map_or(f.blocks.len(), |h| h.0);
                let parent = headings
                    .iter()
                    .rev()
                    .find(|(j, _, _, l)| j < i && l < level)
                    .map(|h| h.1.key());
                let chars = object_range(p, &f.blocks[end - 1])?.end - object_range(p, o)?.start;
                out.push(json!({"kind":"heading","object":o,"level":level,"title":t.text(),"parent":parent,"blockRange":{"start":i,"end":end},"blockCount":end-i,"charCount":chars}));
            }
        }
    }
    Ok(out)
}
#[derive(Debug, Clone)]
pub struct Selection {
    pub flow: ObjectRef,
    pub blocks: Range<usize>,
}
impl Selection {
    pub fn flow<'a>(&self, p: &'a Projection) -> Result<&'a FlowRange> {
        let f = p
            .flows
            .iter()
            .find(|f| f.object == self.flow)
            .ok_or_else(|| error("AGENT_NOT_PROJECTED", "流不在投影内"))?;
        if self.blocks.start > self.blocks.end || self.blocks.end > f.blocks.len() {
            return Err(error("BIND_BAD_ARGUMENT", "块范围越界"));
        }
        Ok(f)
    }
    pub fn range(&self, p: &Projection) -> Result<Range<u32>> {
        let f = self.flow(p)?;
        if self.blocks.start == 0 && self.blocks.end == f.blocks.len() {
            return Ok(f.range.clone());
        }
        if self.blocks.is_empty() {
            let at = if let Some(o) = f.blocks.get(self.blocks.start) {
                object_range(p, o)?.start
            } else {
                f.range.end
            };
            return Ok(at..at);
        }
        Ok(object_range(p, &f.blocks[self.blocks.start])?.start
            ..object_range(p, &f.blocks[self.blocks.end - 1])?.end)
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Unit {
    Blocks,
    Utf16,
}
/// 按授权块范围扩展到完整顶层单位；绝不跨流或裁半个段落。
pub fn context(
    p: &Projection,
    anchor: &Anchor,
    selection: &Selection,
    before: u32,
    after: u32,
    unit: Unit,
    detail: bool,
) -> Result<Value> {
    let offset = p.anchors.to_text_offset(anchor)?;
    // 只用于区间归属比较，不切 UTF-16 字符；末端 left 仍属于前一单位。
    let probe = if anchor.affinity == rsword::agent::anchors::Affinity::Left {
        offset.saturating_sub(1)
    } else {
        offset
    };
    let f = selection.flow(p)?;
    let authorized = selection.range(p)?;
    if offset < authorized.start || offset > authorized.end {
        return Err(error("AGENT_NOT_PROJECTED", "锚点不在授权范围内"));
    }
    let owner = match &anchor.target {
        rsword::agent::anchors::Target::Source { part, flow, .. } => (*part, *flow),
        rsword::agent::anchors::Target::Presentation { owner, .. } => (owner.part, owner.flow),
    };
    if owner != (f.object.part, f.object.flow) {
        return Err(error("AGENT_NOT_PROJECTED", "锚点不属于授权流"));
    }
    let parent = p
        .objects
        .values()
        .filter(|o| {
            o.object.part == f.object.part
                && o.object.flow == f.object.flow
                && o.object.kind == "cell"
                && o.range.start <= probe
                && probe < o.range.end
        })
        .min_by_key(|o| o.range.end - o.range.start);
    let container = parent.map_or(authorized.clone(), |o| {
        o.range.start.max(authorized.start)..o.range.end.min(authorized.end)
    });
    let mut candidates: Vec<_> = p
        .objects
        .values()
        .filter(|o| {
            o.object.part == f.object.part
                && o.object.flow == f.object.flow
                && matches!(o.object.kind.as_str(), "paragraph" | "table" | "image" | "protected")
                && o.range.start >= container.start
                && o.range.end <= container.end
        })
        .collect();
    candidates.sort_by_key(|o| (o.range.start, std::cmp::Reverse(o.range.end)));
    let units: Vec<_> = candidates
        .iter()
        .filter(|o| match unit {
            Unit::Blocks => !candidates.iter().any(|other| {
                other.object != o.object
                    && other.range.start <= o.range.start
                    && other.range.end >= o.range.end
                    && (other.range != o.range || other.object.kind == "paragraph")
            }),
            Unit::Utf16 => {
                o.object.kind != "table"
                    && !candidates.iter().any(|other| {
                        other.object != o.object
                            && other.object.kind == "paragraph"
                            && other.range.start <= o.range.start
                            && other.range.end >= o.range.end
                    })
            }
        })
        .copied()
        .collect();
    let center = units.iter().position(|o| probe >= o.range.start && probe < o.range.end);
    let Some(center) = center else {
        // 流框架/范围标记自身没有段落，按明确 owner 下钻，不能猜邻近段落。
        let rsword::agent::anchors::Target::Presentation { owner, .. } = &anchor.target else {
            return Err(error("AGENT_NOT_PROJECTED", "锚点不属于授权内容单位"));
        };
        let o = p
            .objects
            .get(&owner.key())
            .ok_or_else(|| error("AGENT_NOT_PROJECTED", "呈现对象未投影"))?;
        if o.range.start < authorized.start || o.range.end > authorized.end {
            return Err(error("AGENT_NOT_PROJECTED", "呈现对象超出授权范围"));
        }
        return Ok(
            json!({"object":owner,"flow":f.object,"parent":f.object,"requestedRange":{"unit":"object","object":owner},"actualRange":o.range,"blockRange":selection.blocks,"text":p.text_range(o.range.clone())?,"detail":if detail { vec![json!({"object":owner,"metadata":o.metadata})] } else { vec![] }}),
        );
    };
    let (start, end, requested) = match unit {
        Unit::Blocks => {
            let s = center.saturating_sub(before as usize);
            let e = center.saturating_add(after as usize).saturating_add(1).min(units.len());
            (s, e, json!({"unit":"blocks","start":s,"end":e}))
        }
        Unit::Utf16 => {
            let s = offset.saturating_sub(before).max(container.start);
            let e = offset.saturating_add(after).min(container.end);
            let a = units.iter().position(|o| o.range.end > s).unwrap_or(center).min(center);
            let b = units.iter().rposition(|o| o.range.start < e).unwrap_or(center).max(center);
            (a, b + 1, json!({"unit":"utf16","start":s,"end":e}))
        }
    };
    let actual = units[start].range.start..units[end - 1].range.end;
    let details: Vec<_> = if detail {
        p.objects
            .values()
            .filter(|o| {
                o.object.part == f.object.part
                    && o.object.flow == f.object.flow
                    && o.range.start >= actual.start
                    && o.range.end <= actual.end
            })
            .map(|o| json!({"object":o.object,"metadata":o.metadata}))
            .collect()
    } else {
        vec![]
    };
    Ok(
        json!({"object":units[center].object,"flow":f.object,"parent":parent.map_or(&f.object,|o|&o.object),"requestedRange":requested,"actualRange":actual,"blockRange":{"start":start,"end":end},"text":p.text_range(actual)?,"detail":details}),
    )
}
