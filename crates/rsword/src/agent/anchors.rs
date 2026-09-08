//! AGENT-02：UTF-16 分段覆盖、两类锚点与严格反向校验。
use crate::{
    edit::{InlinePos, Utf16Offset},
    model::Document,
};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectRef {
    pub part: u32,
    pub node: u32,
    pub flow: u32,
    pub kind: String,
}
impl ObjectRef {
    pub fn key(&self) -> String {
        format!("{}:{}:{}:{}", self.kind, self.part, self.flow, self.node)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Affinity {
    Left,
    Right,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Target {
    Source {
        part: u32,
        flow: u32,
        node: u32,
        #[serde(rename = "inlinePos")]
        inline_pos: InlinePos,
    },
    Presentation {
        owner: ObjectRef,
        reason: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub snapshot: String,
    pub projection_key: String,
    pub segment_key: u32,
    pub offset_in_segment: u32,
    pub affinity: Affinity,
    #[serde(flatten)]
    pub target: Target,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub range: Range<u32>,
    pub target: Target,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorCounts {
    pub source_utf16: u32,
    pub presentation_utf16: u32,
    pub source_scalars: u32,
    pub presentation_scalars: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentError {
    pub code: &'static str,
    pub message: String,
}
impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AgentError {}
pub type Result<T> = std::result::Result<T, AgentError>;
pub(crate) fn err(code: &'static str, message: impl Into<String>) -> AgentError {
    AgentError { code, message: message.into() }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorMap {
    pub snapshot: String,
    pub projection_key: String,
    pub segments: Vec<Segment>,
    pub counts: AnchorCounts,
    // 字符边界同时提供 UTF-8 切片位置；不对每次定位重扫全文。
    #[serde(skip)]
    pub(crate) boundaries: Vec<(u32, usize)>,
    pub(crate) empty: Target,
    #[serde(skip)]
    pub(crate) flow_ends: std::collections::BTreeSet<u32>,
}
impl AnchorMap {
    pub(crate) fn new(snapshot: &str, projection_key: &str, owner: ObjectRef) -> Self {
        Self {
            snapshot: snapshot.into(),
            projection_key: projection_key.into(),
            segments: vec![],
            counts: AnchorCounts::default(),
            boundaries: vec![(0, 0)],
            empty: Target::Presentation { owner, reason: "empty".into() },
            flow_ends: Default::default(),
        }
    }
    pub fn len(&self) -> u32 {
        self.boundaries.last().unwrap().0
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn byte_offset(&self, offset: u32) -> Result<usize> {
        self.boundaries
            .binary_search_by_key(&offset, |b| b.0)
            .map(|i| self.boundaries[i].1)
            .map_err(|_| err("AGENT_BAD_OFFSET", "偏移越界或位于代理对中间"))
    }
    pub(crate) fn push(&mut self, content: &mut String, value: &str, target: Target) {
        if value.is_empty() {
            return;
        }
        let start = self.len();
        let mut offset = start;
        let mut byte = content.len();
        for c in value.chars() {
            offset += c.len_utf16() as u32;
            byte += c.len_utf8();
            self.boundaries.push((offset, byte));
        }
        let scalars = value.chars().count() as u32;
        match &target {
            Target::Source { .. } => {
                self.counts.source_utf16 += offset - start;
                self.counts.source_scalars += scalars;
            }
            Target::Presentation { .. } => {
                self.counts.presentation_utf16 += offset - start;
                self.counts.presentation_scalars += scalars;
            }
        }
        self.segments.push(Segment { range: start..offset, target });
        content.push_str(value);
    }
    pub fn to_anchor(&self, offset: u32, affinity: Option<Affinity>) -> Result<Anchor> {
        self.byte_offset(offset)?;
        let affinity = if offset == self.len() {
            Affinity::Left
        } else if offset == 0 {
            Affinity::Right
        } else {
            affinity.unwrap_or(if self.flow_ends.contains(&offset) {
                Affinity::Left
            } else {
                Affinity::Right
            })
        };
        if self.segments.is_empty() {
            return Ok(Anchor {
                snapshot: self.snapshot.clone(),
                projection_key: self.projection_key.clone(),
                segment_key: 0,
                offset_in_segment: 0,
                affinity,
                target: self.empty.clone(),
            });
        }
        let index = match affinity {
            Affinity::Right => self.segments.partition_point(|s| s.range.end <= offset),
            Affinity::Left => self.segments.partition_point(|s| s.range.end < offset),
        };
        let s = self.segments.get(index).ok_or_else(|| err("AGENT_BAD_ANCHOR", "分段覆盖缺失"))?;
        if offset < s.range.start || offset > s.range.end {
            return Err(err("AGENT_BAD_ANCHOR", "分段覆盖缺失"));
        }
        let delta = offset - s.range.start;
        let mut target = s.target.clone();
        if let Target::Source { inline_pos, .. } = &mut target {
            inline_pos.offset.0 += delta;
        }
        Ok(Anchor {
            snapshot: self.snapshot.clone(),
            projection_key: self.projection_key.clone(),
            segment_key: index as u32,
            offset_in_segment: delta,
            affinity,
            target,
        })
    }
    pub fn to_text_offset(&self, anchor: &Anchor) -> Result<u32> {
        let start = if self.segments.is_empty() {
            0
        } else {
            self.segments
                .get(anchor.segment_key as usize)
                .ok_or_else(|| err("AGENT_BAD_ANCHOR", "未知分段"))?
                .range
                .start
        };
        let offset = start
            .checked_add(anchor.offset_in_segment)
            .ok_or_else(|| err("AGENT_BAD_ANCHOR", "偏移溢出"))?;
        let expected = self
            .to_anchor(offset, Some(anchor.affinity))
            .map_err(|_| err("AGENT_BAD_ANCHOR", "非法锚点位置"))?;
        if &expected != anchor {
            return Err(err("AGENT_BAD_ANCHOR", "锚点身份、part、类型或冗余字段不匹配"));
        }
        Ok(offset)
    }
    pub fn source_candidates(&self, mut pos: InlinePos) -> Result<Vec<u32>> {
        // 投影的空锚点始终记录主 part；缺席 part 与 BIND v3.1 一样指主 part。
        if pos.part.is_none()
            && let Target::Presentation { owner, .. } = &self.empty
        {
            pos.part = Some(crate::package::PartId(owner.part));
        }
        let mut out = Vec::new();
        for s in &self.segments {
            if let Target::Source { inline_pos, .. } = &s.target
                && inline_pos.part == pos.part
                && inline_pos.para == pos.para
                && let Some(delta) = pos.offset.0.checked_sub(inline_pos.offset.0)
                && delta <= s.range.end - s.range.start
                && self.byte_offset(s.range.start + delta).is_ok()
            {
                out.push(s.range.start + delta);
            }
        }
        out.sort_unstable();
        out.dedup();
        if out.is_empty() {
            Err(err("AGENT_NOT_PROJECTED", "原生位置未投影"))
        } else {
            Ok(out)
        }
    }
    pub fn validate(&self, content: &str, doc: &Document) -> Result<()> {
        let mut boundaries = vec![(0, 0)];
        let mut off = 0;
        for (b, c) in content.char_indices() {
            off += c.len_utf16() as u32;
            boundaries.push((off, b + c.len_utf8()));
        }
        if boundaries != self.boundaries {
            return Err(err("AGENT_BAD_ANCHOR", "字符边界表不匹配"));
        }
        let mut end = 0;
        let mut counts = AnchorCounts::default();
        for s in &self.segments {
            if s.range.start != end || s.range.end <= end {
                return Err(err("AGENT_BAD_ANCHOR", "分段有洞或重叠"));
            }
            let a = self.byte_offset(s.range.start)?;
            let b = self.byte_offset(s.range.end)?;
            let scalars = content[a..b].chars().count() as u32;
            match &s.target {
                Target::Source { part, flow, node, inline_pos } => {
                    if inline_pos.part.map(|p| p.0) != Some(*part) {
                        return Err(err("AGENT_BAD_ANCHOR", "source part 不一致"));
                    }
                    let tb = doc
                        .text_block_in(crate::package::PartId(*part), inline_pos.para)
                        .ok_or_else(|| err("AGENT_BAD_ANCHOR", "源段落不存在"))?;
                    let text = tb.text();
                    let begin = crate::edit::pos::utf16_to_byte(&text, inline_pos.offset.0)
                        .map_err(|_| err("AGENT_BAD_ANCHOR", "源起点非法"))?;
                    let finish = crate::edit::pos::utf16_to_byte(
                        &text,
                        inline_pos.offset.0 + s.range.end - s.range.start,
                    )
                    .map_err(|_| err("AGENT_BAD_ANCHOR", "源终点非法"))?;
                    if text.get(begin..finish) != Some(&content[a..b]) {
                        return Err(err("AGENT_BAD_ANCHOR", "源文字与投影不一致"));
                    }
                    if !tb.inlines.iter().any(|i|matches!(i,crate::model::Inline::Run(r) if r.segments.iter().any(|s|s.node.0==*node))) {
                        return Err(err("AGENT_BAD_ANCHOR","源载体不在该段落"));
                    }
                    if super::text::flow_of(
                        doc,
                        crate::package::PartId(*part),
                        crate::xml::NodeId(*node),
                    ) != Some(*flow)
                    {
                        return Err(err("AGENT_BAD_ANCHOR", "源载体不属于所给流"));
                    }
                    for delta in content[a..b]
                        .chars()
                        .scan(0u32, |n, c| {
                            let x = *n;
                            *n += c.len_utf16() as u32;
                            Some(x)
                        })
                        .chain(std::iter::once(s.range.end - s.range.start))
                    {
                        crate::edit::pos::locate(tb, Utf16Offset(inline_pos.offset.0 + delta))
                            .map_err(|_| err("AGENT_BAD_ANCHOR", "源位置不满足 EDIT-02"))?;
                    }
                    counts.source_utf16 += s.range.end - s.range.start;
                    counts.source_scalars += scalars;
                }
                Target::Presentation { .. } => {
                    counts.presentation_utf16 += s.range.end - s.range.start;
                    counts.presentation_scalars += scalars;
                }
            }
            end = s.range.end;
        }
        if end != off || counts != self.counts {
            return Err(err("AGENT_BAD_ANCHOR", "覆盖长度或计数不匹配"));
        }
        Ok(())
    }
}
