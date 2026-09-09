//! AGENT-06：完整单位最长前缀；文本和记录使用同一预算与游标注册表。
use crate::{
    Result,
    budget::{self, Budget},
    cursor::Registry,
    error,
};
use rsword::agent::{anchors::Target, text::Projection};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ops::Range};
#[derive(Clone)]
pub struct Unit {
    pub object: Value,
    pub range: Range<u32>,
    pub content: Value,
    pub anchors: Vec<Value>,
    pub omitted: Vec<Value>,
    pub counts: [u32; 4],
    pub projection_key: Option<String>,
    pub metadata: Value,
}
impl Unit {
    pub fn record(value: Value, index: usize) -> Self {
        Self {
            object: value.get("object").cloned().unwrap_or(Value::Null),
            range: index as u32..index as u32 + 1,
            content: value,
            anchors: vec![],
            omitted: vec![],
            counts: [0; 4],
            projection_key: None,
            metadata: json!({}),
        }
    }
}
pub fn text_units(p: &Projection, range: Range<u32>) -> Result<Vec<Unit>> {
    p.text_range(range.clone())?;
    let mut ends: Vec<_> = p
        .objects
        .values()
        .filter(|o| {
            matches!(o.object.kind.as_str(), "paragraph" | "image" | "protected")
                && o.range.start >= range.start
                && o.range.end <= range.end
        })
        .filter(|o| {
            !p.objects.values().any(|other| {
                other.object != o.object
                    && other.object.kind == "paragraph"
                    && other.range.start <= o.range.start
                    && other.range.end >= o.range.end
            })
        })
        .map(|o| (o.range.end, o.object.clone()))
        .collect();
    ends.sort_by_key(|(end, _)| *end);
    ends.dedup_by_key(|(end, _)| *end);
    // 不让落在父段落内部的原子对象把段落切开；结构前后缀附属相邻完整单位。
    ends.retain(|(end, _)| {
        !p.objects
            .values()
            .any(|o| o.object.kind == "paragraph" && o.range.start < *end && *end < o.range.end)
    });
    let fallback = p
        .flows
        .iter()
        .find(|f| f.range.start <= range.start && f.range.end >= range.end)
        .map(|f| f.object.clone())
        .or_else(|| p.flows.first().map(|f| f.object.clone()))
        .ok_or_else(|| error("AGENT_NOT_PROJECTED", "没有投影流"))?;
    if let Some(last) = ends.last_mut() {
        last.0 = range.end;
    } else {
        ends.push((range.end, fallback));
    }
    let mut units = vec![];
    let mut start = range.start;
    for (end, object) in ends {
        if end < start || end > range.end {
            continue;
        }
        let mut u = Unit {
            object: json!(object),
            range: start..end,
            content: json!(p.text_range(start..end)?),
            anchors: vec![],
            omitted: vec![],
            counts: [0; 4],
            projection_key: Some(p.anchors.projection_key.clone()),
            metadata: json!({}),
        };
        for (i, s) in p
            .anchors
            .segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.range.start >= start && s.range.end <= end)
        {
            let scalars = p.text_range(s.range.clone())?.chars().count() as u32;
            let index = if matches!(s.target, Target::Source { .. }) { 0 } else { 1 };
            u.counts[index] += s.range.end - s.range.start;
            u.counts[index + 2] += scalars;
            u.anchors.push(json!({"segmentKey":i,"range":s.range,"target":s.target}));
        }
        units.push(u);
        start = end;
    }
    for (category, owner) in &p.omission_owners {
        let at = p.objects.get(&owner.key()).map(|o| o.range.start);
        let index = match at {
            Some(at) if at >= range.start && at < range.end => {
                units.iter().position(|u| at < u.range.end)
            }
            None if range.start == 0 => Some(0),
            _ => None,
        };
        if let Some(index) = index {
            units[index]
                .omitted
                .push(json!({"category":category.name(),"count":1,"reason":category.reason()}));
        }
    }
    if let Some(first) = units.first_mut() {
        first.metadata["unrequestedFlows"] = p.omitted["unrequestedFlows"].clone();
        if range.start == 0 {
            first.metadata["diagnostics"] = json!(p.diagnostics);
            first.metadata["addressableNotProjected"] =
                p.omitted["addressableNotProjected"].clone();
        }
        if range.is_empty() {
            first.metadata["caret"] = json!(p.anchors.to_anchor(range.start, None)?);
        }
    }
    Ok(units)
}
fn content(units: &[Unit], text: bool) -> Value {
    if text {
        json!(units.iter().map(|u| u.content.as_str().unwrap()).collect::<String>())
    } else {
        json!(units.iter().map(|u| u.content.clone()).collect::<Vec<_>>())
    }
}
pub fn response(
    snapshot: &str,
    units: &[Unit],
    text: bool,
    range: Value,
    more: bool,
    next: Option<&str>,
) -> Value {
    let mut out = budget::envelope(snapshot, content(units, text), range, more, next);
    let mut counts = [0; 4];
    let mut omitted: BTreeMap<String, (u64, Value)> = BTreeMap::new();
    for u in units {
        for (key, value) in u.metadata.as_object().unwrap() {
            if key == "diagnostics" {
                out[key] = value.clone();
            } else {
                out["omitted"][key] = value.clone();
            }
        }
        for (sum, n) in counts.iter_mut().zip(u.counts) {
            *sum += n;
        }
        for o in &u.omitted {
            let x = omitted
                .entry(o["category"].as_str().unwrap().into())
                .or_insert((0, o["reason"].clone()));
            x.0 += o["count"].as_u64().unwrap();
        }
    }
    if text || units.iter().any(|u| u.projection_key.is_some()) {
        out["anchors"] =
            json!({"segments":units.iter().flat_map(|u|u.anchors.iter()).collect::<Vec<_>>()});
        out["anchors"]["snapshot"] = out["snapshot"].clone();
        out["anchors"]["projectionKey"] =
            json!(units.first().and_then(|u| u.projection_key.as_deref()));
        out["anchorCounts"] = json!({"sourceUtf16":counts[0],"presentationUtf16":counts[1],"sourceScalars":counts[2],"presentationScalars":counts[3],"scope":"page"});
    }
    out["omitted"]["page"]=json!(omitted.into_iter().map(|(category,(count,reason))|json!({"category":category,"count":count,"reason":reason})).collect::<Vec<_>>());
    budget::measure(&mut out);
    out
}
#[allow(clippy::too_many_arguments)]
pub fn page(
    registry: &mut Registry,
    snapshot: &str,
    tool: &str,
    config: &str,
    units: &[Unit],
    text: bool,
    range: Value,
    b: Budget,
    cursor: Option<&str>,
    max_rows: usize,
) -> Result<Value> {
    b.validate()?;
    let position = registry.resume(snapshot, tool, config, cursor)?;
    let start = match position {
        Some(ref p) => p["unit"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| error("AGENT_BAD_CURSOR", "单位位置非法"))?,
        None => 0,
    };
    if start > units.len() || max_rows == 0 {
        return Err(error("AGENT_BAD_CURSOR", "单位位置非法"));
    }
    let (out, (more, token, next)) = budget::longest_prefix(
        start + usize::from(start < units.len()),
        units.len().min(start.saturating_add(max_rows)),
        b,
        units.get(start).map(|u| u.object.clone()).unwrap_or(Value::Null),
        |end| {
            let more = end < units.len();
            let next = json!({"unit":end,"object":units.get(end).map(|u|&u.object),"offset":units.get(end).map(|u|u.range.start)});
            let token = registry.candidate(snapshot, tool, config, &next);
            let out = response(
                snapshot,
                &units[start..end],
                text,
                range.clone(),
                more,
                more.then_some(token.as_str()),
            );
            (out, (more, token, next))
        },
    )?;
    if more {
        registry.commit(token, snapshot, tool, config, next);
    }
    Ok(out)
}
