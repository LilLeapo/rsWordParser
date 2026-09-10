//! AGENT-05：授权对象的模型/字段/绘图详情；媒体句柄必须来自调用方会话表。
use crate::{
    Result,
    budget::{self, Budget},
    error,
    nav::{self, Selection, Unit},
};
use rsword::agent::anchors::Anchor;
use rsword::agent::text::Projection;
use rsword::bind::native::json::ProjCx;
use rsword::bind::native::json::ToJson;
use rsword::model::Block;
use rsword::model::Blocks;
use rsword::model::Display;
use rsword::model::Document;
use rsword::model::Inline;
use rsword::model::box_flows;
use rsword::package::Package;
use rsword::package::PartId;
use rsword::package::RelTarget;
use rsword::package::media::MediaStore;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
/// 只读索引，构建时允许读取完整模型；返回时只从授权对象集合中取值。
pub struct Details {
    entries: BTreeMap<(u32, u32), Value>,
}
impl Details {
    pub fn build(pkg: &Package, doc: &Document, p: &Projection, media: &MediaStore) -> Self {
        Self::build_with_display(pkg, doc, p, media, true)
    }
    pub fn build_with_display(
        pkg: &Package,
        doc: &Document,
        p: &Projection,
        media: &MediaStore,
        display: bool,
    ) -> Self {
        let cx = ProjCx { pkg, display };
        let mut entries = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut parts = vec![doc.main_part];
        parts.extend(doc.hf_parts.keys().copied());
        parts.extend(
            [doc.footnotes.part, doc.endnotes.part, doc.comments.part].into_iter().flatten(),
        );
        let mut stack = vec![];
        for part in parts {
            if let Some(blocks) = doc.blocks_of_part(part) {
                stack.extend(blocks.into_iter().map(|b| (part, b)));
            }
        }
        while let Some((part, root)) = stack.pop() {
            for b in rsword::model::Blocks::over(std::slice::from_ref(root)) {
                if !seen.insert((part, b.node())) {
                    continue;
                }
                let mut data = json!({"model":b.to_json(&cx)});
                if let rsword::model::Block::Table(t) = b
                    && let Some(dom) = pkg.part(part).dom()
                {
                    let view = rsword::resolve::Resolver::new(doc).table(dom, t);
                    let cols = view.columns();
                    data["geometry"] = json!({"widthsTwips":cols.widths_twips,"widthsPct":cols.widths_pct,"columnCount":cols.column_count(),"rows":cols.rows.iter().map(|r|r.iter().map(|c|json!({"cell":c.cell,"span":c.span,"gap":c.gap})).collect::<Vec<_>>()).collect::<Vec<_>>()});
                }
                entries.insert((part.0, b.node().0), data);
                for (content, child_part) in box_flows(b) {
                    stack.extend(content.iter().map(|b| (child_part.unwrap_or(part), b)));
                }
                let displays: Vec<_> = match b {
                    rsword::model::Block::Text(t) => t
                        .inlines
                        .iter()
                        .flat_map(|i| match i {
                            rsword::model::Inline::Run(r) => r
                                .segments
                                .iter()
                                .filter_map(|s| s.display.as_ref())
                                .collect::<Vec<_>>(),
                            rsword::model::Inline::Atom(_) | rsword::model::Inline::Field { id: _, result: _ } => vec![],
                        })
                        .collect(),
                    rsword::model::Block::Image(i) => i.display.iter().collect(),
                    rsword::model::Block::Protected(b) => b.display.iter().chain(b.siblings.iter()).collect(),
                    rsword::model::Block::Table(_) => vec![],
                };
                for display in displays {
                    match display {
                        rsword::model::Display::Drawing(d) => {
                            for pic in &d.pictures {
                                entries.insert((part.0,pic.node.unwrap_or(d.node).0),json!({"display":pic.to_json(&cx),"media":pic.embed.iter().chain(pic.link.iter()).map(|rid|media_ref(pkg,media,part,rid)).collect::<Vec<_>>() }));
                            }
                            if let Some(chart) = &d.chart {
                                let target = chart
                                    .rel_id
                                    .as_deref()
                                    .and_then(|id| pkg.part(part).rels.target_uri(id))
                                    .and_then(|uri| pkg.find(uri));
                                entries.insert((part.0,chart.node.0),json!({"reference":chart.to_json(&cx),"chart":target.and_then(|id|doc.chart_parts.get(&id)).map(|c|c.to_json(&cx))}));
                            }
                            if let Some(diagram) = &d.diagram {
                                let target = diagram
                                    .rel_id
                                    .as_deref()
                                    .and_then(|id| pkg.part(part).rels.target_uri(id))
                                    .and_then(|uri| pkg.find(uri));
                                entries.insert((part.0,diagram.node.0),json!({"reference":diagram.to_json(&cx),"diagram":target.and_then(|id|doc.diagram_parts.get(&id)).map(|d|d.to_json(&cx))}));
                            }
                        }
                        rsword::model::Display::Vml(v) => {
                            for shape in &v.shapes {
                                if let Some(rid) = &shape.imagedata {
                                    entries.insert((part.0,shape.node.0),json!({"display":shape.to_json(&cx),"media":[media_ref(pkg,media,part,rid)]}));
                                }
                            }
                        }
                        rsword::model::Display::Formula(_) => {}
                    }
                }
            }
        }
        for o in p.objects.values() {
            if o.object.kind == "field"
                && let Some(id) = o.metadata["id"].as_u64()
                && let Some(text) = doc.field_result_text(
                    pkg,
                    PartId(o.object.part),
                    rsword::span::FieldId(id as u32),
                )
            {
                entries.entry((o.object.part, o.object.node)).or_insert_with(|| json!({}))["fieldResult"] =
                    json!({"text":text,"readOnly":true,"keyword":o.metadata["keyword"]});
            }
            if let Some(data) = entries.get_mut(&(o.object.part, o.object.node)) {
                let sections: Vec<_> = if o.object.part == doc.main_part.0 {
                    doc.sections
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            p.flows
                                .iter()
                                .find(|f| {
                                    f.object.part == doc.main_part.0 && f.object.kind == "main"
                                })
                                .is_some_and(|f| {
                                    f.blocks.get(s.block_range.clone()).unwrap_or(&[]).iter().any(
                                        |b| {
                                            p.objects.get(&b.key()).is_some_and(|r| {
                                                r.range.start <= o.range.start
                                                    && r.range.end >= o.range.end
                                            })
                                        },
                                    )
                                })
                        })
                        .map(|(i, _)| i)
                        .collect()
                } else if doc.hf_parts.contains_key(&PartId(o.object.part)) {
                    use rsword::model::HfKind;
use rsword::model::HfVariant;
                    let resolver = rsword::resolve::Resolver::new(doc);
                    (0..doc.sections.len())
                        .filter(|&i| {
                            let section = resolver.section(&doc.sections, i).unwrap();
                            rsword::model::HfKind::ALL.into_iter().any(|kind| {
                                rsword::model::HfVariant::ALL.into_iter().any(|variant| {
                                    section
                                        .slot(kind, variant)
                                        .rel_id()
                                        .and_then(|rid| {
                                            pkg.part(doc.main_part).rels.target_uri(rid)
                                        })
                                        .and_then(|uri| pkg.find(uri))
                                        == Some(PartId(o.object.part))
                                })
                            })
                        })
                        .collect()
                } else {
                    vec![]
                };
                data["sections"] = json!(sections);
            }
        }
        Self { entries }
    }
    pub fn get(&self, part: u32, node: u32) -> Option<&Value> {
        self.entries.get(&(part, node))
    }
}
fn media_ref(pkg: &Package, media: &MediaStore, part: PartId, rid: &str) -> Value {
    let mut value = json!({"relationship":rid});
    match pkg.part(part).rels.by_id(rid).map(|r| &r.target) {
        Some(RelTarget::External(uri)) => {
            value["external"] = json!(true);
            value["uri"] = json!(uri);
        }
        Some(RelTarget::Internal(uri)) => {
            value["external"] = json!(false);
            value["uri"] = json!(uri.as_str());
            if let Some(id) = pkg.find(uri) {
                value["partId"] = json!(id.0);
                value["mediaId"] = json!(media.id_for_part(id).map(|id| id.0));
                if media.id_for_part(id).is_none() {
                    value["unavailable"] = json!("mediaNotRegistered");
                }
            } else {
                value["unavailable"] = json!("partMissing");
            }
        }
        None => {
            value["missing"] = json!(true);
        }
    };
    value
}
/// 包含完整窗口/详情记录的有界响应；不能先提交内容再发现预算失败。
#[allow(clippy::too_many_arguments)]
pub fn context_page(
    p: &Projection,
    anchor: &Anchor,
    selection: &Selection,
    before: u32,
    after: u32,
    unit: Unit,
    details: Option<&Details>,
    budget: Budget,
) -> Result<Value> {
    budget.validate()?;
    let mut value = nav::context(p, anchor, selection, before, after, unit, details.is_some())?;
    if let Some(details) = details {
        for entry in value["detail"].as_array_mut().unwrap() {
            let object = &entry["object"];
            if let Some(data) = details.get(
                object["part"].as_u64().unwrap() as u32,
                object["node"].as_u64().unwrap() as u32,
            ) {
                entry["value"] = data.clone();
            }
        }
    }
    let out = budget::envelope(
        &p.anchors.snapshot,
        json!([value]),
        json!(selection.range(p)?),
        false,
        None,
    );
    if !budget::fits(&out, budget) {
        return Err(budget::too_small(&out, json!(selection.flow)));
    }
    if out["content"][0]["detail"].as_array().is_none() {
        return Err(error("AGENT_NOT_PROJECTED", "详情索引不可用"));
    }
    Ok(out)
}
