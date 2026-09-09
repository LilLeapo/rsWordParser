//! AGENT-04：命中映回原始区间；响应预算只接收完整命中记录。
use crate::cursor::Registry;
use crate::{
    Result,
    budget::{self, Budget},
    error,
    nav::Selection,
    search::{self, Options, Request},
};
use rsword::agent::{
    anchors::{Affinity, Target},
    text::Projection,
};
use serde_json::{Value, json};
#[derive(Default)]
pub struct Finder {
    pub(crate) registry: Registry,
}
impl Finder {
    #[allow(clippy::too_many_arguments)]
    pub fn find(
        &mut self,
        p: &Projection,
        selection: &[Selection],
        pattern: &str,
        options: Options,
        max_hits: usize,
        budget: Budget,
        cursor: Option<&str>,
        worker: search::Worker,
    ) -> Result<Value> {
        budget.validate()?;
        if !(1..=1000).contains(&max_hits) {
            return Err(error("BIND_BAD_ARGUMENT", "maxHits 应在 1..=1000 内"));
        }
        let mut ranges = vec![];
        for s in selection {
            let r = s.range(p)?;
            ranges.push((s.flow.clone(), r));
        }
        ranges.sort_by_key(|(_, r)| r.start);
        // 授权同一流的分离范围不能拼接，避免伪造跨缺口匹配。
        if ranges.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(error("BIND_BAD_ARGUMENT", "同一流请提供一个连续授权范围"));
        }
        let flows = ranges
            .iter()
            .map(|(o, r)| {
                Ok(search::InputFlow {
                    part: o.part,
                    flow: o.flow,
                    start: r.start,
                    text: p.text_range(r.clone())?.into(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let config=serde_json::to_string(&json!({"projection":p.anchors.projection_key,"scope":ranges,"pattern":pattern,"mode":options.mode,"insensitive":options.insensitive,"width":options.fold_width,"whitespace":options.collapse_whitespace})).unwrap();
        let position = self
            .registry
            .resume(&p.anchors.snapshot, "find", &config, cursor)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| error("AGENT_BAD_CURSOR", "搜索位置非法"))?
            .unwrap_or_default();
        let request = Request { pattern: pattern.into(), options, flows, max_hits, position };
        let batch = worker.submit(&request)?;
        let rows = batch
            .hits
            .iter()
            .map(|h| {
                let scope = ranges
                    .iter()
                    .find(|(o, _)| o.part == h.part && o.flow == h.flow)
                    .expect("worker 的流来自授权请求");
                record(p, h, &scope.1)
            })
            .collect::<Result<Vec<_>>>()?;
        let (value, (next, token)) = budget::longest_prefix(
            usize::from(!rows.is_empty()),
            rows.len(),
            budget,
            Value::Null,
            |end| {
                let next = if end < rows.len() {
                    Some(batch.hits[end - 1].next.clone())
                } else {
                    batch.next.clone()
                };
                let token = self.registry.candidate(
                    &p.anchors.snapshot,
                    "find",
                    &config,
                    &serde_json::to_value(&next).unwrap(),
                );
                let value = budget::envelope(
                    &p.anchors.snapshot,
                    json!(&rows[..end]),
                    json!(ranges),
                    next.is_some(),
                    next.as_ref().map(|_| token.as_str()),
                );
                (value, (next, token))
            },
        )?;
        if let Some(position) = next {
            self.registry.commit(
                token,
                &p.anchors.snapshot,
                "find",
                &config,
                serde_json::to_value(position).unwrap(),
            );
        }
        Ok(value)
    }
}
fn record(p: &Projection, h: &search::Hit, authorized: &std::ops::Range<u32>) -> Result<Value> {
    let original = p.text_range(h.range.clone())?;
    if original != h.original {
        return Err(error("AGENT_BAD_ANCHOR", "归一匹配回映的原文前置条件不一致"));
    }
    let affinity = if h.range.is_empty() && h.range.end == authorized.end {
        Affinity::Left
    } else {
        Affinity::Right
    };
    let start = p.anchors.to_flow_anchor(h.range.start, affinity, h.part, h.flow)?;
    let end = if h.range.is_empty() {
        start.clone()
    } else {
        p.anchors.to_flow_anchor(h.range.end, Affinity::Left, h.part, h.flow)?
    };
    let source_only = p
        .anchors
        .segments
        .iter()
        .filter(|s| s.range.start < h.range.end && s.range.end > h.range.start)
        .all(|s| matches!(s.target, Target::Source { .. }));
    let editable = h.complete
        && source_only
        && matches!(start.target, Target::Source { .. })
        && matches!(end.target, Target::Source { .. });
    if h.range.start < authorized.start || h.range.end > authorized.end {
        return Err(error("AGENT_NOT_PROJECTED", "命中超出授权范围"));
    }
    for anchor in [&start, &end] {
        let (part, flow) = match &anchor.target {
            Target::Source { part, flow, .. } => (*part, *flow),
            Target::Presentation { owner, .. } => (owner.part, owner.flow),
        };
        if (part, flow) != (h.part, h.flow) {
            return Err(error("AGENT_BAD_ANCHOR", "命中锚点归属越过授权流"));
        }
    }
    let mut left = h.range.start.saturating_sub(40).max(authorized.start);
    while p.anchors.byte_offset(left).is_err() {
        left -= 1;
    }
    let mut right = h.range.end.saturating_add(40).min(authorized.end);
    while p.anchors.byte_offset(right).is_err() {
        right += 1;
    }
    Ok(
        json!({"match":original,"normalizedMatch":h.normalized_match,"textRange":h.range,"anchors":{"start":start,"end":end},"editable":editable,"precondition":{"original":original,"range":h.range},"context":{"range":{"start":left,"end":right},"text":p.text_range(left..right)?}}),
    )
}
/// 编译编辑前调用：只验证真实源区间及原文，不用归一文字或归一长度替代它。
pub fn verify_precondition(p: &Projection, record: &Value) -> Result<()> {
    let start: rsword::agent::anchors::Anchor =
        serde_json::from_value(record["anchors"]["start"].clone())
            .map_err(|_| error("AGENT_BAD_ANCHOR", "起点载荷非法"))?;
    let end: rsword::agent::anchors::Anchor =
        serde_json::from_value(record["anchors"]["end"].clone())
            .map_err(|_| error("AGENT_BAD_ANCHOR", "终点载荷非法"))?;
    let range = p.anchors.to_text_offset(&start)?..p.anchors.to_text_offset(&end)?;
    if record["precondition"]["range"] != json!(range)
        || record["precondition"]["original"] != p.text_range(range)?
    {
        return Err(error("AGENT_PRECONDITION_FAILED", "源区间或原文已变化"));
    }
    Ok(())
}
