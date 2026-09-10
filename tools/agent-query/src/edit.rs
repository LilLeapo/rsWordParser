//! AGENT-07：源范围预检、正向线型编译；不写 DOM，不依赖反向编码。
use crate::{Result, error, search};
use rsword::{
    agent::{
        anchors::{Anchor, ObjectRef, Target},
        text::Projection,
    },
    bind::native::{EditOpJson, SessionTable},
    edit::InlinePos,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{ops::Range, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selector {
    pub scope: Vec<ObjectRef>,
    pub find: Option<String>,
    #[serde(default)]
    pub search: search::Options,
    pub start: Option<Anchor>,
    pub end: Option<Anchor>,
    pub original: Option<String>,
    pub occurrence: Option<u32>,
    #[serde(default)]
    pub all: bool,
}
#[derive(Clone)]
pub struct WorkerConfig {
    pub program: PathBuf,
    pub scratch: PathBuf,
    pub args: Vec<String>,
}
impl WorkerConfig {
    pub fn start(&self) -> Result<search::Worker> {
        search::Worker::start_with_args(&self.program, &self.scratch, &self.args)
    }
}
macro_rules! actions {
    ($($variant:ident => $wire:literal, $helper:ident, $task:literal, $supported:literal { $($field:ident:$ty:ty),* };)*) => {
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(tag="action",rename_all_fields="camelCase",deny_unknown_fields)]
        pub enum Action { $(#[serde(rename=$wire)] $variant { $($field:$ty),* }),* }
        impl Action {
            pub const COVERAGE: &'static [(&'static str,&'static str,bool)] = &[$(($wire,$task,$supported)),*];
            pub fn name(&self)-> &'static str { match self { $(Self::$variant{..}=>$wire),* } }
            pub fn compile(&self,cx:&mut Compiler<'_>)->Result<Vec<EditOpJson>> {
                match self { $(Self::$variant{$($field),*}=>cx.$helper($($field),*)),* }
            }
            pub fn schema()->Value {
                let mut defs=rsword::bind::native::SchemaDefs::default();
                let mut variants=vec![];
                $(let mut properties=json!({"action":{"const":$wire}});let mut required=vec!["action".to_owned()];
                  $(let name=crate::edit_schema::camel(stringify!($field));properties[&name]=<$ty as crate::edit_schema::WireSchema>::schema(&mut defs);if !<$ty as crate::edit_schema::WireSchema>::optional(){required.push(name);})*
                  variants.push(json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}));)*
                json!({"$schema":"https://json-schema.org/draft/2020-12/schema","oneOf":variants,"$defs":defs.into_map()})
            }
        }
        #[cfg(test)] mod agent_07_audit_tests {
            $(#[test] fn $helper(){super::tests::check($wire);})*
        }
    }
}
#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
actions! {
    ReplaceText=>"replaceText",replace,"W1/W11", true {selector:Selector,text:String};
    InsertParagraphAfter=>"insertParagraphAfter",insert,"W2", true {target:ObjectRef,text:String,style:Option<String>};
    SetBlockStyle=>"setBlockStyle",style,"W6", true {target:ObjectRef,style_id:String,create_style:Option<Box<rsword::save::options::decl::StyleUpsertSave>>};
    DeleteBlock=>"deleteBlock",delete,"W10", true {target:ObjectRef};
    MoveBlocks=>"moveBlocks",move_blocks,"W10", true {targets:Vec<ObjectRef>,before:ObjectRef};
    DeleteTableColumn=>"deleteTableColumn",column,"W4", true {target:ObjectRef,column:u32};
    AddComment=>"addComment",comment,"W5", true {selector:Selector,author:String,text:String,date:Option<String>};
    AcceptRevisions=>"acceptRevisions",revisions,"W3", true {scope:Vec<ObjectRef>,author:String};
    UpdateToc=>"updateToc",toc,"W7", false {target:ObjectRef};
    SetHeaderFooter=>"setHeaderFooter",header,"W8", true {target:ObjectRef,kind:rsword::model::HfKind,variant:rsword::model::HfVariant,paragraphs:Vec<String>};
    ReplaceImage=>"replaceImage",image,"W9", true {target:ObjectRef,media_id:u32};
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub operations: Vec<Action>,
    #[serde(default)]
    pub context: rsword::EditContext,
}
impl Request {
    pub fn parse(input: &str) -> Result<Self> {
        serde_json::from_value(crate::audit::parse(input)?)
            .map_err(|e| error("BIND_BAD_ARGUMENT", e.to_string()))
    }
}
pub struct Compiler<'a> {
    pub native: &'a mut SessionTable,
    pub id: &'a str,
    pub projection: &'a Projection,
    pub worker: Option<&'a WorkerConfig>,
}
fn wire(value: Value) -> Result<EditOpJson> {
    serde_json::from_value(value).map_err(|e| error("AGENT_UNREPRESENTABLE", e.to_string()))
}
fn not_editable(owner: &ObjectRef) -> crate::QueryError {
    let mut e = error("AGENT_NOT_EDITABLE", "范围包含呈现字符");
    e.details = json!({"owner":owner});
    e
}
fn paragraph(text: &str, style: Option<&str>) -> Value {
    json!({"kind":"paragraph","value":{"props":style.map(|s|json!({"style":s})),"inlines":[{"kind":"run","value":{"text":text,"props":null}}]}})
}
impl Compiler<'_> {
    fn object(&self, o: &ObjectRef, kind: Option<&str>) -> Result<Range<u32>> {
        let object = self
            .projection
            .objects
            .get(&o.key())
            .ok_or_else(|| error("AGENT_TARGET_NOT_FOUND", "对象不在当前投影中"))?;
        if kind.is_some_and(|k| k != o.kind) {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "对象类型不适用于操作"));
        }
        Ok(object.range.clone())
    }
    fn main(&self, o: &ObjectRef) -> Result<()> {
        let part = self.native.inspect(self.id, |s, _| s.main_part().0)?;
        if o.part != part {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "该原生操作仅支持主 part"));
        }
        Ok(())
    }
    fn source(
        &self,
        start: &Anchor,
        end: &Anchor,
        original: &str,
    ) -> Result<(InlinePos, InlinePos)> {
        let p = self.projection;
        if start.snapshot != p.anchors.snapshot || end.snapshot != p.anchors.snapshot {
            return Err(error("AGENT_STALE_ANCHOR", "锚点版本已失效"));
        }
        let range = p.anchors.to_text_offset(start)?..p.anchors.to_text_offset(end)?;
        if range.start > range.end {
            return Err(error("AGENT_BAD_ANCHOR", "范围倒置"));
        }
        for t in [&start.target, &end.target].into_iter().chain(
            p.anchors
                .segments
                .iter()
                .filter(|s| s.range.start < range.end && s.range.end > range.start)
                .map(|s| &s.target),
        ) {
            if let Target::Presentation { owner, .. } = t {
                return Err(not_editable(owner));
            }
        }
        let (Target::Source { inline_pos: from, .. }, Target::Source { inline_pos: to, .. }) =
            (&start.target, &end.target)
        else {
            unreachable!()
        };
        if from.part != to.part
            || from.para != to.para
            || to.offset.0 < from.offset.0
            || to.offset.0 - from.offset.0 != range.end - range.start
        {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "源区间跨段或不连续"));
        }
        for segment in p
            .anchors
            .segments
            .iter()
            .filter(|s| s.range.start < range.end && s.range.end > range.start)
        {
            if let Target::Source { inline_pos, .. } = &segment.target {
                let at = segment.range.start.max(range.start);
                if inline_pos.part != from.part
                    || inline_pos.para != from.para
                    || inline_pos.offset.0 + (at - segment.range.start)
                        != from.offset.0 + (at - range.start)
                {
                    return Err(error("AGENT_UNSUPPORTED_RANGE", "中间源片段不连续"));
                }
            }
        }
        if p.text_range(range)? != original {
            return Err(error("AGENT_PRECONDITION_FAILED", "原文前置条件不匹配"));
        }
        Ok((*from, *to))
    }
    fn matches(&self, s: &Selector) -> Result<Vec<(InlinePos, InlinePos)>> {
        if s.scope.is_empty() || (s.all && s.occurrence.is_some()) {
            return Err(error("BIND_BAD_ARGUMENT", "必须明确 scope，all 与 occurrence 互斥"));
        }
        let ranges = self.projection.select_objects(&s.scope)?;
        if s.find.is_none() {
            if s.all || s.occurrence.is_some() {
                return Err(error("BIND_BAD_ARGUMENT", "锚点选择不能同时指定匹配序号"));
            }
            let start = s.start.as_ref().ok_or_else(|| error("BIND_BAD_ARGUMENT", "缺少起点"))?;
            let end = s.end.as_ref().unwrap_or(start);
            let original = s
                .original
                .as_deref()
                .ok_or_else(|| error("BIND_BAD_ARGUMENT", "锚点编辑必须提供 original"))?;
            let positions = self.source(start, end, original)?;
            let a = self.projection.anchors.to_text_offset(start)?;
            let b = self.projection.anchors.to_text_offset(end)?;
            if !ranges.iter().any(|r| r.start <= a && b <= r.end) {
                return Err(error("AGENT_NOT_PROJECTED", "范围不在授权 scope 内"));
            }
            return Ok(vec![positions]);
        }
        if s.start.is_some() || s.end.is_some() {
            return Err(error("BIND_BAD_ARGUMENT", "find 与锚点互斥"));
        }
        let worker = self
            .worker
            .ok_or_else(|| error("BIND_BAD_ARGUMENT", "find 选择器需要可终止 worker 配置"))?;
        let mut flows = vec![];
        // 对象范围可能跨流；逐流交集保持授权，绝不拼接不同流。
        for flow in &self.projection.flows {
            for range in &ranges {
                let r = range.start.max(flow.range.start)..range.end.min(flow.range.end);
                if r.start < r.end {
                    flows.push(search::InputFlow {
                        part: flow.object.part,
                        flow: flow.object.flow,
                        start: r.start,
                        text: self.projection.text_range(r)?.into(),
                    })
                }
            }
        }
        let mut position = Default::default();
        let mut hits = vec![];
        loop {
            let batch = worker.start()?.submit(&search::Request {
                pattern: s.find.clone().unwrap(),
                options: s.search.clone(),
                flows: flows.clone(),
                max_hits: 1000,
                position,
            })?;
            hits.extend(batch.hits);
            let Some(next) = batch.next else { break };
            if hits.len() >= 100_000 {
                return Err(error("AGENT_UNIT_TOO_LARGE", "匹配集合超过编译容量，未部分执行"));
            }
            position = next;
        }
        if hits.is_empty() {
            return Err(error("AGENT_TARGET_NOT_FOUND", "没有匹配"));
        }
        if !s.all {
            if s.occurrence.is_none() && hits.len() > 1 {
                let mut e = error("AGENT_AMBIGUOUS", "多个匹配，必须指定 occurrence 或 all");
                e.details = json!({"total":hits.len(),"candidates":hits.iter().take(8).map(|h|json!({"part":h.part,"flow":h.flow,"range":h.range})).collect::<Vec<_>>()});
                return Err(e);
            }
            let index = s.occurrence.unwrap_or(1) as usize;
            if index == 0 || index > hits.len() {
                return Err(error("AGENT_TARGET_NOT_FOUND", "匹配序号不存在"));
            }
            hits = vec![hits.remove(index - 1)];
        }
        let mut out = vec![];
        for hit in hits.iter().rev() {
            if !hit.complete {
                return Err(error("AGENT_UNSUPPORTED_RANGE", "归一命中不能映回完整源区间"));
            }
            if s.original.as_deref().is_some_and(|v| v != hit.original) {
                return Err(error("AGENT_PRECONDITION_FAILED", "命中原文不匹配"));
            }
            let authorized = flows
                .iter()
                .find_map(|f| {
                    let range = f.start..f.start + f.text.encode_utf16().count() as u32;
                    (f.part == hit.part
                        && f.flow == hit.flow
                        && range.start <= hit.range.start
                        && hit.range.end <= range.end)
                        .then_some(range)
                })
                .ok_or_else(|| error("AGENT_NOT_PROJECTED", "命中不在授权流内"))?;
            let (start, end) = crate::find::hit_anchors(self.projection, hit, &authorized)?;
            out.push(self.source(&start, &end, &hit.original)?);
        }
        Ok(out)
    }
    fn replace(&mut self, s: &Selector, text: &str) -> Result<Vec<EditOpJson>> {
        let mut ops = vec![];
        for (from, to) in self.matches(s)? {
            ops.push(wire(json!({"op":"deleteRange","from":from,"to":to}))?);
            ops.push(wire(json!({"op":"insertText","at":from,"text":text}))?);
        }
        Ok(ops)
    }
    fn comment(
        &mut self,
        s: &Selector,
        author: &str,
        text: &str,
        date: &Option<String>,
    ) -> Result<Vec<EditOpJson>> {
        let matches = self.matches(s)?;
        let main = self.native.inspect(self.id, |s, _| s.main_part())?;
        if matches.iter().any(|(from, to)| {
            from.part.is_some_and(|p| p != main) || to.part.is_some_and(|p| p != main)
        }) {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "原生批注操作仅支持主 part"));
        }
        matches.into_iter().map(|(from,to)|wire(json!({"op":"addComment","from":from,"to":to,"comment":{"author":author,"text":text,"date":date,"initials":null,"done":false}}))).collect()
    }
    fn insert(
        &mut self,
        o: &ObjectRef,
        text: &str,
        style: &Option<String>,
    ) -> Result<Vec<EditOpJson>> {
        self.object(o, Some("paragraph"))?;
        Ok(vec![wire(
            json!({"op":"insertBlock","at":{"part":o.part,"at":{"after":o.node}},"block":paragraph(text,style.as_deref())}),
        )?])
    }
    fn delete(&mut self, o: &ObjectRef) -> Result<Vec<EditOpJson>> {
        self.object(o, None)?;
        if !["paragraph", "table"].contains(&o.kind.as_str()) {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "只能删除完整块"));
        }
        Ok(vec![wire(json!({"op":"deleteBlock","part":o.part,"node":o.node}))?])
    }
    fn column(&mut self, o: &ObjectRef, column: &u32) -> Result<Vec<EditOpJson>> {
        self.main(o)?;
        self.object(o, Some("table"))?;
        if *column == 0 {
            return Err(error("AGENT_TARGET_NOT_FOUND", "列号从 1 开始"));
        }
        Ok(vec![wire(json!({"op":"deleteColumn","table":o.node,"at":column-1}))?])
    }
    fn style(
        &mut self,
        o: &ObjectRef,
        id: &str,
        create: &Option<Box<rsword::save::options::decl::StyleUpsertSave>>,
    ) -> Result<Vec<EditOpJson>> {
        self.object(o, Some("paragraph"))?;
        let existing = self.native.inspect(self.id, |s, _| {
            s.document().styles.as_ref().and_then(|styles| styles.get(id)).cloned()
        })?;
        let mut ops = vec![];
        if existing.as_ref().is_some_and(|style| {
            style.kind() != Some(rsword::semantic::props::StyleType::Paragraph)
        }) {
            return Err(error("AGENT_STYLE_CONFLICT", "setBlockStyle 只接受段落样式"));
        }
        if let Some(create) = create {
            if create.style_id != id || create.kind != "paragraph" {
                return Err(error("AGENT_STYLE_CONFLICT", "样式声明 id/type 不匹配"));
            }
            if let Some(existing) = &existing {
                if existing.kind() != Some(rsword::semantic::props::StyleType::Paragraph)
                    || existing.name.as_deref() != Some(create.name.as_str())
                    || existing.based_on != create.based_on
                    || existing.ppr.as_ref() != create.para_props.as_ref()
                    || existing.rpr.as_ref() != create.run_props.as_ref()
                {
                    let mut e = error("AGENT_STYLE_CONFLICT", "已有样式与 createStyle 声明冲突");
                    e.details = json!({"styleId":id});
                    return Err(e);
                }
            } else {
                ops.push(wire(json!({"op":"upsertStyle","style":create}))?);
            }
        } else if existing.is_none() {
            return Err(error("AGENT_TARGET_NOT_FOUND", "样式不存在，须提供 createStyle"));
        }
        ops.push(wire(
            json!({"op":"setParaProps","part":o.part,"para":o.node,"patch":{"style":id}}),
        )?);
        Ok(ops)
    }
    fn image(&mut self, o: &ObjectRef, media: &u32) -> Result<Vec<EditOpJson>> {
        self.main(o)?;
        self.object(o, Some("image"))?;
        let bytes = self.native.media(self.id, *media)?;
        let mime = self
            .native
            .inspect(self.id, |_, store| {
                store.try_get(rsword::package::media::MediaId(*media)).map(|m| m.mime.clone())
            })?
            .ok_or_else(|| error("BIND_ID_UNKNOWN", "媒体不存在"))?;
        crate::media::verify(&bytes, &mime)?;
        Ok(vec![wire(
            json!({"op":"replaceImageMedia","drawing":o.node,"bytes":bytes,"mime":mime}),
        )?])
    }
    fn header(
        &mut self,
        target: &ObjectRef,
        kind: &rsword::model::HfKind,
        variant: &rsword::model::HfVariant,
        paragraphs: &[String],
    ) -> Result<Vec<EditOpJson>> {
        self.main(target)?;
        if target.kind != "section"
            || !self.native.inspect(self.id, |s, _| {
                s.document()
                    .sections
                    .iter()
                    .any(|section| section.node.is_some_and(|n| n.0 == target.node))
            })?
        {
            return Err(error("AGENT_TARGET_NOT_FOUND", "当前主 part 中没有此节"));
        }
        Ok(vec![wire(
            json!({"op":"setHeaderFooter","sect":target.node,"kind":kind,"variant":variant,"content":paragraphs.iter().map(|p|paragraph(p,None)).collect::<Vec<_>>()}),
        )?])
    }
    fn move_blocks(
        &mut self,
        targets: &[ObjectRef],
        before: &ObjectRef,
    ) -> Result<Vec<EditOpJson>> {
        self.main(before)?;
        self.object(before, None)?;
        for target in targets {
            self.main(target)?;
            self.object(target, None)?;
        }
        let (nodes, section_nodes) = self.native.inspect(self.id, |s, _| {
            (
                s.document().main.iter().map(|b| b.node().0).collect::<Vec<_>>(),
                s.document()
                    .sections
                    .iter()
                    .filter_map(|section| {
                        if let rsword::model::SectionOwner::Paragraph(n) = section.owner {
                            Some(n.0)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })?;
        if !nodes.contains(&before.node) || targets.is_empty() {
            return Err(error("AGENT_TARGET_NOT_FOUND", "移动目标或目的地不在主块序列"));
        }
        let mut indices = targets
            .iter()
            .map(|o| {
                nodes
                    .iter()
                    .position(|n| *n == o.node)
                    .ok_or_else(|| error("AGENT_UNSUPPORTED_RANGE", "移动只接受完整主块"))
            })
            .collect::<Result<Vec<_>>>()?;
        indices.sort_unstable();
        if indices.windows(2).any(|w| w[1] != w[0] + 1)
            || targets.iter().any(|o| o.node == before.node || section_nodes.contains(&o.node))
        {
            return Err(error("AGENT_UNSUPPORTED_RANGE", "块集合不连续、目的地在自身或含分节结构"));
        }
        indices.into_iter().map(|i|wire(json!({"op":"moveBlock","from":before.part,"node":nodes[i],"to":{"part":before.part,"at":{"before":before.node}}}))).collect()
    }
    fn revisions(&mut self, scope: &[ObjectRef], author: &str) -> Result<Vec<EditOpJson>> {
        if scope.is_empty() {
            return Err(error("BIND_BAD_ARGUMENT", "必须指定修订 scope"));
        }
        let ranges = self.projection.select_objects(scope)?;
        let revisions = self.native.inspect(self.id, |s, _| {
            s.document().revisions.iter_inner_first().into_iter().cloned().collect::<Vec<_>>()
        })?;
        let mut selected = vec![];
        for rev in revisions.iter().filter(|r| r.author() == Some(author)) {
            let owner = rev.owner.node().and_then(|n| {
                self.projection
                    .objects
                    .values()
                    .find(|o| o.object.part == rev.part.0 && o.object.node == n.0)
            });
            let Some(owner) = owner else {
                return Err(error(
                    "AGENT_UNSUPPORTED_RANGE",
                    "该修订宿主不能映回授权对象，不猜测范围",
                ));
            };
            if !ranges.iter().any(|r| r.start <= owner.range.start && owner.range.end <= r.end) {
                continue;
            }
            if rev.pair.is_some_and(|pair| {
                revisions.iter().any(|r| r.id == pair && r.author() != Some(author))
            }) {
                return Err(error("AGENT_UNSUPPORTED_RANGE", "配对修订跨作者，不能扩大授权"));
            }
            selected.push(wire(json!({"op":"acceptRevision","rev":rev.id}))?);
        }
        if selected.is_empty() {
            return Err(error("AGENT_TARGET_NOT_FOUND", "授权范围内没有该作者的修订"));
        }
        Ok(selected)
    }
    fn toc(&mut self, _target: &ObjectRef) -> Result<Vec<EditOpJson>> {
        Err(error("AGENT_UNSUPPORTED_RANGE", "TOC 页码来源核对尚未交付，不猜页码"))
    }
}
