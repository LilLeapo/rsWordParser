//! `BIND-03` 间接包含 NewElement 的闭包；递归字段通过显式后序工作栈转换。
use super::codec::*;
use crate::edit::*;
use crate::semantic::props::{ParaPropsPatch, RunPropsPatch};
use crate::xml::Dom;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 文本 run 的结构化输入，可由 JSON 解码构造。
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct NewRunJson {
    /// run 的文本。
    pub text: String,
    /// run 属性补丁；缺席时由编辑操作决定继承格式。
    pub props: Option<RunPropsPatch>,
}
pub struct RunCodec;
impl Codec for RunCodec {
    type Engine = NewRun;
    type Wire = NewRunJson;
    fn encode(v: &NewRun, dom: &Dom) -> Result<NewRunJson> {
        let NewRun { text, props } = v;
        Ok(NewRunJson { text: text.clone(), props: Optional::<RunPropsCodec>::encode(props, dom)? })
    }
    fn decode(v: &NewRunJson, cx: &mut DecodeCx<'_>) -> Result<NewRun> {
        let NewRunJson { text, props } = v;
        Ok(NewRun { text: text.clone(), props: Optional::<RunPropsCodec>::decode(props, cx)? })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[non_exhaustive]
/// 行内内容的协议线型，嵌套包装仍使用结构化行内内容。
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum NewInlineJson {
    /// 文本 run。
    Run(NewRunJson),
    /// 超链接。
    Hyperlink {
        /// 外部 URL 或文档内部锚点。
        target: NewLinkTarget,
        /// 鼠标提示文字。
        tooltip: Option<String>,
        /// 链接内的内容。
        inlines: Vec<NewInlineJson>,
    },
    /// 插入修订包装。
    Ins {
        /// 修订作者与时间。
        rev: NewRevision,
        /// 被插入的内容。
        inlines: Vec<NewInlineJson>,
    },
    /// 删除修订包装。
    Del {
        /// 修订作者与时间。
        rev: NewRevision,
        /// 被删除的内容。
        inlines: Vec<NewInlineJson>,
    },
    /// 范围标记。
    Marker(NewMarker),
    /// 复杂字段。
    Field {
        /// 字段指令。
        instr: String,
        /// 缓存结果内容。
        result: Vec<NewInlineJson>,
        /// 是否生成 separate 标记。
        separate: bool,
        /// 是否要求宿主重算。
        dirty: bool,
        /// 字段 run 属性补丁。
        props: Option<RunPropsPatch>,
    },
    /// 原始 XML 逃生口，成功 apply 后记录诊断和计数。
    Xml(String),
}

fn postorder<N, O>(
    root: &N,
    children: impl Fn(&N) -> Vec<&N>,
    mut build: impl FnMut(&N, &mut BTreeMap<*const N, O>) -> Result<O>,
) -> Result<O> {
    let mut work = vec![(root, false)];
    let mut done = BTreeMap::new();
    while let Some((node, visited)) = work.pop() {
        if visited {
            let out = build(node, &mut done)?;
            done.insert(node as *const N, out);
        } else {
            work.push((node, true));
            work.extend(children(node).into_iter().rev().map(|c| (c, false)));
        }
    }
    Ok(done.remove(&(root as *const N)).expect("postorder root"))
}
fn take<N, O>(nodes: &[N], done: &mut BTreeMap<*const N, O>) -> Vec<O> {
    nodes.iter().map(|n| done.remove(&(n as *const N)).expect("postorder child")).collect()
}

pub struct InlineCodec;
impl Codec for InlineCodec {
    type Engine = NewInline;
    type Wire = NewInlineJson;
    fn encode(v: &NewInline, dom: &Dom) -> Result<NewInlineJson> {
        postorder(
            v,
            |n| match n {
                NewInline::Hyperlink { inlines, .. }
                | NewInline::Ins { inlines, .. }
                | NewInline::Del { inlines, .. } => inlines.iter().collect(),
                NewInline::Field { result, .. } => result.iter().collect(),
                NewInline::Run(_) | NewInline::Marker(_) | NewInline::Xml(_) => vec![],
            },
            |n, done| {
                Ok(match n {
                    NewInline::Run(run) => NewInlineJson::Run(RunCodec::encode(run, dom)?),
                    NewInline::Hyperlink { target, tooltip, inlines } => NewInlineJson::Hyperlink {
                        target: target.clone(),
                        tooltip: tooltip.clone(),
                        inlines: take(inlines, done),
                    },
                    NewInline::Ins { rev, inlines } => {
                        NewInlineJson::Ins { rev: rev.clone(), inlines: take(inlines, done) }
                    }
                    NewInline::Del { rev, inlines } => {
                        NewInlineJson::Del { rev: rev.clone(), inlines: take(inlines, done) }
                    }
                    NewInline::Marker(marker) => NewInlineJson::Marker(marker.clone()),
                    NewInline::Field { instr, result, separate, dirty, props } => {
                        NewInlineJson::Field {
                            instr: instr.clone(),
                            result: take(result, done),
                            separate: *separate,
                            dirty: *dirty,
                            props: Optional::<RunPropsCodec>::encode(props, dom)?,
                        }
                    }
                    NewInline::Xml(xml) => NewInlineJson::Xml(XmlElement::encode(xml, dom)?),
                })
            },
        )
    }
    fn decode(v: &NewInlineJson, cx: &mut DecodeCx<'_>) -> Result<NewInline> {
        postorder(
            v,
            |n| match n {
                NewInlineJson::Hyperlink { inlines, .. }
                | NewInlineJson::Ins { inlines, .. }
                | NewInlineJson::Del { inlines, .. } => inlines.iter().collect(),
                NewInlineJson::Field { result, .. } => result.iter().collect(),
                NewInlineJson::Run(_) | NewInlineJson::Marker(_) | NewInlineJson::Xml(_) => vec![],
            },
            |n, done| {
                Ok(match n {
                    NewInlineJson::Run(run) => NewInline::Run(RunCodec::decode(run, cx)?),
                    NewInlineJson::Hyperlink { target, tooltip, inlines } => NewInline::Hyperlink {
                        target: target.clone(),
                        tooltip: tooltip.clone(),
                        inlines: take(inlines, done),
                    },
                    NewInlineJson::Ins { rev, inlines } => {
                        NewInline::Ins { rev: rev.clone(), inlines: take(inlines, done) }
                    }
                    NewInlineJson::Del { rev, inlines } => {
                        NewInline::Del { rev: rev.clone(), inlines: take(inlines, done) }
                    }
                    NewInlineJson::Marker(marker) => NewInline::Marker(marker.clone()),
                    NewInlineJson::Field { instr, result, separate, dirty, props } => {
                        NewInline::Field {
                            instr: instr.clone(),
                            result: take(result, done),
                            separate: *separate,
                            dirty: *dirty,
                            props: Optional::<RunPropsCodec>::decode(props, cx)?,
                        }
                    }
                    NewInlineJson::Xml(xml) => NewInline::Xml(XmlElement::decode(xml, cx)?),
                })
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 字段输入，可由 JSON 解码构造。
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct NewFieldJson {
    /// 字段指令。
    pub instr: String,
    /// 缓存结果。
    pub result: Vec<NewInlineJson>,
    /// 是否要求宿主重算。
    pub mark_dirty: bool,
}
pub struct FieldCodec;
impl Codec for FieldCodec {
    type Engine = NewField;
    type Wire = NewFieldJson;
    fn encode(v: &NewField, dom: &Dom) -> Result<NewFieldJson> {
        let NewField { instr, result, mark_dirty } = v;
        Ok(NewFieldJson {
            instr: instr.clone(),
            result: List::<InlineCodec>::encode(result, dom)?,
            mark_dirty: *mark_dirty,
        })
    }
    fn decode(v: &NewFieldJson, cx: &mut DecodeCx<'_>) -> Result<NewField> {
        let NewFieldJson { instr, result, mark_dirty } = v;
        Ok(NewField {
            instr: instr.clone(),
            result: List::<InlineCodec>::decode(result, cx)?,
            mark_dirty: *mark_dirty,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[non_exhaustive]
/// 占一个内容坐标的原子输入。
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum NewAtomJson {
    /// 换行、分页或分栏。
    Break {
        /// 换行种类。
        kind: crate::model::inline::BreakKind,
        /// 绕排清除方式。
        clear: Option<String>,
    },
    /// 字体符号。
    Symbol {
        /// 符号字体。
        font: String,
        /// 字符码。
        code: u32,
    },
    /// 脚注或尾注引用，同时提供注释正文。
    NoteRef {
        /// 为真时创建尾注，否则为脚注。
        endnote: bool,
        /// 按段落分组的 run。
        content: Vec<Vec<NewRunJson>>,
    },
    /// 图片。
    Image(NewImage),
    /// 行内公式。
    Math(NewMath),
}
pub struct AtomCodec;
impl Codec for AtomCodec {
    type Engine = NewAtom;
    type Wire = NewAtomJson;
    fn encode(v: &NewAtom, dom: &Dom) -> Result<NewAtomJson> {
        Ok(match v {
            NewAtom::Break { kind, clear } => {
                NewAtomJson::Break { kind: *kind, clear: clear.clone() }
            }
            NewAtom::Symbol { font, code } => {
                NewAtomJson::Symbol { font: font.clone(), code: *code }
            }
            NewAtom::NoteRef { endnote, content } => NewAtomJson::NoteRef {
                endnote: *endnote,
                content: List::<List<RunCodec>>::encode(content, dom)?,
            },
            NewAtom::Image(image) => NewAtomJson::Image(image.clone()),
            NewAtom::Math(math) => NewAtomJson::Math(math.clone()),
        })
    }
    fn decode(v: &NewAtomJson, cx: &mut DecodeCx<'_>) -> Result<NewAtom> {
        Ok(match v {
            NewAtomJson::Break { kind, clear } => {
                NewAtom::Break { kind: *kind, clear: clear.clone() }
            }
            NewAtomJson::Symbol { font, code } => {
                NewAtom::Symbol { font: font.clone(), code: *code }
            }
            NewAtomJson::NoteRef { endnote, content } => NewAtom::NoteRef {
                endnote: *endnote,
                content: List::<List<RunCodec>>::decode(content, cx)?,
            },
            NewAtomJson::Image(image) => NewAtom::Image(image.clone()),
            NewAtomJson::Math(math) => NewAtom::Math(math.clone()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[non_exhaustive]
/// 块内容输入；XML 逃生口与结构化段落属性分开编码。
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum NewBlockJson {
    /// 段落。
    Paragraph {
        /// 段落属性补丁。
        props: Box<Option<ParaPropsPatch>>,
        /// 行内内容。
        inlines: Vec<NewInlineJson>,
    },
    /// 空表格。
    Table {
        /// 行数。
        rows: u32,
        /// 列数。
        cols: u32,
        /// 各列宽度，单位 twips。
        widths: Option<Vec<i32>>,
        /// 表格样式 ID。
        style: Option<String>,
        /// 首行是否为表头。
        header: bool,
    },
    /// 原始块 XML 逃生口。
    Xml(String),
    /// XML 包装器逃生口，内部块仍可结构化。
    Wrapped {
        /// 包装元素 XML。
        wrapper: String,
        /// 包装内容。
        block: Box<NewBlockJson>,
    },
    /// 图表。
    Chart {
        /// 图表内容。
        chart: NewChart,
        /// 宽高，单位 EMU。
        extent_emu: Option<(i64, i64)>,
    },
    /// 图片段落。
    Image(NewImage),
    /// 独立公式段落。
    MathPara {
        /// 公式内容。
        omml: NewMath,
        /// 段落对齐方式。
        align: String,
    },
    /// 文本框。
    Textbox {
        /// 外观。
        look: ShapeLook,
        /// 文本框内容。
        blocks: Vec<NewBlockJson>,
    },
    /// 预设形状。
    Shape {
        /// 几何预设。
        preset: PresetGeom,
        /// 外观。
        look: ShapeLook,
        /// 可选文字。
        text: Option<String>,
    },
    /// 线条。
    Line {
        /// 线条种类。
        kind: LineKind,
        /// 起点，单位 EMU。
        from: (i64, i64),
        /// 终点，单位 EMU。
        to: (i64, i64),
        /// 可选颜色。
        color: Option<String>,
    },
    /// 块字段生成器。
    Field(NewBlockField),
    /// 题注。
    Caption {
        /// 编号标签。
        label: String,
        /// 题注正文。
        text: String,
    },
    /// 顺序插入多个块。
    Many(Vec<NewBlockJson>),
}
pub struct BlockCodec;
impl Codec for BlockCodec {
    type Engine = NewBlock;
    type Wire = NewBlockJson;
    fn encode(v: &NewBlock, dom: &Dom) -> Result<NewBlockJson> {
        postorder(
            v,
            |n| match n {
                NewBlock::Paragraph { props: _, inlines: _ } => vec![],
                NewBlock::Table { rows: _, cols: _, widths: _, style: _, header: _ } => vec![],
                NewBlock::Xml(_) => vec![],
                NewBlock::Wrapped { wrapper: _, block } => vec![block.as_ref()],
                NewBlock::Chart { chart: _, extent_emu: _ } => vec![],
                NewBlock::Image(_) => vec![],
                NewBlock::MathPara { omml: _, align: _ } => vec![],
                NewBlock::Textbox { look: _, blocks } => blocks.iter().collect(),
                NewBlock::Shape { preset: _, look: _, text: _ } => vec![],
                NewBlock::Line { kind: _, from: _, to: _, color: _ } => vec![],
                NewBlock::Field(_) => vec![],
                NewBlock::Caption { label: _, text: _ } => vec![],
                NewBlock::Many(value) => value.iter().collect(),
            },
            |n, done| {
                Ok(match n {
                    NewBlock::Paragraph { props, inlines } => NewBlockJson::Paragraph {
                        props: Box::new(Optional::<ParaPropsCodec>::encode(props, dom)?),
                        inlines: List::<InlineCodec>::encode(inlines, dom)?,
                    },
                    NewBlock::Table { rows, cols, widths, style, header } => NewBlockJson::Table {
                        rows: *rows,
                        cols: *cols,
                        widths: widths.clone(),
                        style: style.clone(),
                        header: *header,
                    },
                    NewBlock::Xml(value) => NewBlockJson::Xml(XmlElement::encode(value, dom)?),
                    NewBlock::Wrapped { wrapper, block } => NewBlockJson::Wrapped {
                        wrapper: XmlElement::encode(wrapper, dom)?,
                        block: Box::new(
                            done.remove(&(block.as_ref() as *const NewBlock))
                                .expect("postorder child"),
                        ),
                    },
                    NewBlock::Chart { chart, extent_emu } => {
                        NewBlockJson::Chart { chart: chart.clone(), extent_emu: *extent_emu }
                    }
                    NewBlock::Image(value) => NewBlockJson::Image(value.clone()),
                    NewBlock::MathPara { omml, align } => {
                        NewBlockJson::MathPara { omml: omml.clone(), align: align.clone() }
                    }
                    NewBlock::Textbox { look, blocks } => {
                        NewBlockJson::Textbox { look: look.clone(), blocks: take(blocks, done) }
                    }
                    NewBlock::Shape { preset, look, text } => NewBlockJson::Shape {
                        preset: preset.clone(),
                        look: look.clone(),
                        text: text.clone(),
                    },
                    NewBlock::Line { kind, from, to, color } => NewBlockJson::Line {
                        kind: *kind,
                        from: *from,
                        to: *to,
                        color: color.clone(),
                    },
                    NewBlock::Field(value) => NewBlockJson::Field(value.clone()),
                    NewBlock::Caption { label, text } => {
                        NewBlockJson::Caption { label: label.clone(), text: text.clone() }
                    }
                    NewBlock::Many(value) => NewBlockJson::Many(take(value, done)),
                })
            },
        )
    }
    fn decode(v: &NewBlockJson, cx: &mut DecodeCx<'_>) -> Result<NewBlock> {
        postorder(
            v,
            |n| match n {
                NewBlockJson::Paragraph { props: _, inlines: _ } => vec![],
                NewBlockJson::Table { rows: _, cols: _, widths: _, style: _, header: _ } => vec![],
                NewBlockJson::Xml(_) => vec![],
                NewBlockJson::Wrapped { wrapper: _, block } => vec![block.as_ref()],
                NewBlockJson::Chart { chart: _, extent_emu: _ } => vec![],
                NewBlockJson::Image(_) => vec![],
                NewBlockJson::MathPara { omml: _, align: _ } => vec![],
                NewBlockJson::Textbox { look: _, blocks } => blocks.iter().collect(),
                NewBlockJson::Shape { preset: _, look: _, text: _ } => vec![],
                NewBlockJson::Line { kind: _, from: _, to: _, color: _ } => vec![],
                NewBlockJson::Field(_) => vec![],
                NewBlockJson::Caption { label: _, text: _ } => vec![],
                NewBlockJson::Many(value) => value.iter().collect(),
            },
            |n, done| {
                Ok(match n {
                    NewBlockJson::Paragraph { props, inlines } => NewBlock::Paragraph {
                        props: Optional::<ParaPropsCodec>::decode(props, cx)?,
                        inlines: List::<InlineCodec>::decode(inlines, cx)?,
                    },
                    NewBlockJson::Table { rows, cols, widths, style, header } => NewBlock::Table {
                        rows: *rows,
                        cols: *cols,
                        widths: widths.clone(),
                        style: style.clone(),
                        header: *header,
                    },
                    NewBlockJson::Xml(value) => NewBlock::Xml(XmlElement::decode(value, cx)?),
                    NewBlockJson::Wrapped { wrapper, block } => NewBlock::Wrapped {
                        wrapper: XmlElement::decode(wrapper, cx)?,
                        block: Box::new(
                            done.remove(&(block.as_ref() as *const NewBlockJson))
                                .expect("postorder child"),
                        ),
                    },
                    NewBlockJson::Chart { chart, extent_emu } => {
                        NewBlock::Chart { chart: chart.clone(), extent_emu: *extent_emu }
                    }
                    NewBlockJson::Image(value) => NewBlock::Image(value.clone()),
                    NewBlockJson::MathPara { omml, align } => {
                        NewBlock::MathPara { omml: omml.clone(), align: align.clone() }
                    }
                    NewBlockJson::Textbox { look, blocks } => {
                        NewBlock::Textbox { look: look.clone(), blocks: take(blocks, done) }
                    }
                    NewBlockJson::Shape { preset, look, text } => NewBlock::Shape {
                        preset: preset.clone(),
                        look: look.clone(),
                        text: text.clone(),
                    },
                    NewBlockJson::Line { kind, from, to, color } => {
                        NewBlock::Line { kind: *kind, from: *from, to: *to, color: color.clone() }
                    }
                    NewBlockJson::Field(value) => NewBlock::Field(value.clone()),
                    NewBlockJson::Caption { label, text } => {
                        NewBlock::Caption { label: label.clone(), text: text.clone() }
                    }
                    NewBlockJson::Many(value) => NewBlock::Many(take(value, done)),
                })
            },
        )
    }
}
