//! L3 文档模型（`spec/06-model.md`，`docs/03` §6.3–6.8）。
//!
//! `Document` 是 DOM + Span 的**语义投影**：可增量 `refresh`（M2），但任何时刻
//! `Document::rebuild(&pkg)` 必须与增量结果相等（`MOD-13`，测试用它作 oracle）。
//! `Run` 与物理 `w:r` 一一对应，逻辑 run 合并只发生在投影层（`compat_ts`）。
//!
//! | 主要模型项 | 内容 |
//! | --- | --- |
//! | [`Inline`] | `Inline` / `Run` / `Segment` 与坐标流（`MOD-06`） |
//! | [`Block`] | `Block` / `TextBlock` / `ProtectedBlock` / `Revision`（`MOD-02/08/09`） |
//! | [`table`] | `TableBlock` / `Row` / `Cell` 与跨表格的块遍历（`MOD-07`） |
//! | [`SdtInfo`] | `SdtInfo`：内容控件的种类 / 锁 / 数据绑定（`MOD-08`） |
//! | [`ParagraphFacts`] | `ParagraphFacts`（`MOD-04`） |
//! | [`classify_paragraph`] | 分类规则表与 `TextKind` 判定（`MOD-05/03`） |
//! | [`build`] | `Document` 与 `rebuild`（`MOD-01/13`） |
//! | [`Styles`] / [`Theme`] / [`Notes`] | 声明模型（`MOD-10`）：样式 / 编号 / 主题 / 设置 / 批注 / 注释 |

// 块模型（`MOD-02`、`MOD-03`、`MOD-08`、`MOD-09`，`docs/03` §6.3）。

pub use crate::model::table::TableBlock;
use crate::semantic::props::{CellProps, RowProps, TableProps};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 装箱：`TextBlock`（含 `ParaProps` 与 facts）比其他变体大几十倍。
    Text(Box<TextBlock>),
    Table(TableBlock),
    Image(ImageBlock),
    Protected(ProtectedBlock),
}

impl Block {
    /// 块对应的节点（`w:p` / `w:tbl` / `w:sectPr` / 未知元素）。
    pub fn node(&self) -> NodeId {
        match self {
            Block::Text(b) => b.node,
            Block::Table(b) => b.node,
            Block::Image(b) => b.node,
            Block::Protected(b) => b.node,
        }
    }

    pub fn sdt(&self) -> Option<&SdtInfo> {
        match self {
            Block::Text(b) => b.sdt.as_ref(),
            Block::Table(b) => b.sdt.as_ref(),
            Block::Image(b) => b.sdt.as_ref(),
            Block::Protected(b) => b.sdt.as_ref(),
        }
    }

    pub fn revisions(&self) -> &[Revision] {
        match self {
            Block::Text(b) => &b.revisions,
            Block::Table(b) => &b.revisions,
            Block::Image(b) => &b.revisions,
            Block::Protected(b) => &b.revisions,
        }
    }

    pub fn as_text(&self) -> Option<&TextBlock> {
        match self {
            Block::Text(b) => Some(b),
            _ => None,
        }
    }
}

/// 可编辑段落。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub node: NodeId,
    pub kind: TextKind,
    pub style_id: Option<String>,
    /// 声明值（`w:pPr`），段落标记 rPr 在 `props.rpr`。
    pub props: ParaProps,
    pub inlines: Vec<Inline>,
    pub sdt: Option<SdtInfo>,
    /// 块级修订：`w:ins/w:del` 包裹、段落标记 ins/del、`pPrChange`。
    pub revisions: Vec<Revision>,
    pub facts: ParagraphFacts,
}

impl TextBlock {
    /// 坐标流文本（`MOD-06`）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        for i in &self.inlines {
            i.append_text(&mut s);
        }
        s
    }

    /// 坐标流长度（UTF-16 单位）。
    pub fn utf16_len(&self) -> u32 {
        self.inlines.iter().map(Inline::utf16_len).sum()
    }

    pub fn para_mark_props(&self) -> Option<&RunProps> {
        self.props.rpr.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextKind {
    Paragraph,
    Heading { level: u8 },
    ListItem { list: ListRef },
}

/// 编号引用（`MOD-03`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRef {
    pub num_id: i32,
    pub ilvl: i32,
    /// 来自段落样式链而非直接 `w:numPr`。
    pub from_style: bool,
}

/// 只含一张图片的段落（`MOD-05` R15）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBlock {
    pub node: NodeId,
    /// 该段唯一那个绘图的显示模型（`MOD-11`）。VML 图片（`w:pict`）的显示模型在 4.5。
    pub display: Option<Display>,
    pub sdt: Option<SdtInfo>,
    pub revisions: Vec<Revision>,
}

/// 只读块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedBlock {
    pub node: NodeId,
    pub kind: ProtectedKind,
    /// 可见文本预览（最多 80 个字符），供编辑器显示占位。
    pub preview: String,
    /// 显示载荷（`MOD-11`）：段落里第一个图形——细横线 / 嵌入对象的 VML、图表 / SmartArt / 画布的绘图。
    pub display: Option<Display>,
    /// 段落里**其余**顶层绘图的显示模型（`R13`：SmartArt 旁的照片 / 形状各有自己的锚点），文档序。
    /// 只有段落分类建的保护块会填；表格 / 节属性 / 过深等结构块恒为空。
    pub siblings: Vec<Display>,
    pub sdt: Option<SdtInfo>,
    pub revisions: Vec<Revision>,
}

/// 保护原因。显示载荷挂在 [`ProtectedBlock::display`]；图表 / SmartArt 的载荷在 M6。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedKind {
    FieldBlockResult(FieldId),
    Equation,
    Chart,
    SmartArt,
    Ole,
    Rule,
    Invisible,
    SectionBreak,
    SectionProps,
    BodyBreak { page: bool },
    Unknown(QName),
    TooDeep,
    Unparseable,
}

impl ProtectedKind {
    /// i18n key（`docs/03` §6.3："`label` 变为 i18n key"）。
    pub fn key(&self) -> &'static str {
        match self {
            ProtectedKind::FieldBlockResult(_) => "protected.field_block_result",
            ProtectedKind::Equation => "protected.equation",
            ProtectedKind::Chart => "protected.chart",
            ProtectedKind::SmartArt => "protected.smart_art",
            ProtectedKind::Ole => "protected.ole",
            ProtectedKind::Rule => "protected.rule",
            ProtectedKind::Invisible => "protected.invisible",
            ProtectedKind::SectionBreak => "protected.section_break",
            ProtectedKind::SectionProps => "protected.section_props",
            ProtectedKind::BodyBreak { .. } => "protected.body_break",
            ProtectedKind::Unknown(_) => "protected.unknown",
            ProtectedKind::TooDeep => "protected.too_deep",
            ProtectedKind::Unparseable => "protected.unparseable",
        }
    }
}

/// 块级 / 段落标记修订（`MOD-09`）。run 级修订在 [`crate::model::RevisionCtx`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// 顶层 `w:ins` 包裹的块；也用于 `trPr/ins`（整行插入，挂在 `Row.revisions`）。
    Insert(RevisionMeta),
    /// 顶层 `w:del` 包裹的块；也用于 `trPr/del`（整行删除）。
    Delete(RevisionMeta),
    MoveFrom(RevisionMeta),
    MoveTo(RevisionMeta),
    /// `pPr/rPr/ins`：段落标记被插入。
    ParaMarkInsert(RevisionMeta),
    /// `pPr/rPr/del`：段落标记被删除（与下一段合并）。
    ParaMarkDelete(RevisionMeta),
    /// `pPrChange`：旧值快照。
    ParaPropsChange {
        meta: RevisionMeta,
        old: Box<ParaProps>,
    },
    /// `numPr/numberingChange`。
    NumberingChange(RevisionMeta),
    /// `tblPr/tblPrChange`：表格属性旧值（`TableBlock.revisions`）。
    TablePropsChange {
        meta: RevisionMeta,
        old: Box<TableProps>,
    },
    /// `sectPr/sectPrChange`：旧值快照（`SectionInfo.revisions`，任务 5.2）。
    ///
    /// 与 `TablePropsChange` 一族同形（typed + `Box`）而不是 `spec/06` 早先写的 `old: NodeId`：
    /// 四个 `*PrChange` 同一形状，M7 的 Accept / Reject 就能共用一条 `plan_apply_*` 路径。
    /// 快照元素本身仍能从 `meta.node`（`w:sectPrChange`）一步走到，信息没丢。
    SectPropsChange {
        meta: RevisionMeta,
        old: Box<crate::semantic::props::SectionProps>,
    },
    /// `tblGrid/tblGridChange`：旧网格；`old` 是快照里的 `w:tblGrid`（没有就是 change 元素本身）。
    TableGridChange {
        meta: RevisionMeta,
        old: NodeId,
    },
    /// `trPr/trPrChange`（`Row.revisions`）。
    RowPropsChange {
        meta: RevisionMeta,
        old: Box<RowProps>,
    },
    /// `tcPr/tcPrChange`（`Cell.revisions`）。
    CellPropsChange {
        meta: RevisionMeta,
        old: Box<CellProps>,
    },
    /// `tcPr/cellIns`。
    CellInsert(RevisionMeta),
    /// `tcPr/cellDel`。
    CellDelete(RevisionMeta),
    /// `tcPr/cellMerge`。
    CellMerge(RevisionMeta),
}

pub mod build {
    //! `Document` 与 `Document::rebuild`（`MOD-01`、`MOD-13`，任务 1.5–1.8）：从规范状态（DOM）
    //! 完整构建投影。M1 只建正文流：段落 → inlines → run 坐标流；表格 / 图片块占位；
    //! `refresh` 在 M2 随编辑引擎加入。

    use std::collections::{BTreeMap, HashMap};

    use crate::diag::{DiagCode, Diagnostic};
    use crate::error::Result;
    use crate::model::AtomKind;
    use crate::model::AuxFlows;
    use crate::model::BreakKind;
    use crate::model::ChartPart;
    use crate::model::DiagramPart;
    use crate::model::HfPart;
    use crate::model::Inline;
    use crate::model::InlineAtom;
    use crate::model::Link;
    use crate::model::LinkTarget;
    use crate::model::OBJECT_REPLACEMENT;
    use crate::model::ParagraphFacts;
    use crate::model::RevisionCtx;
    use crate::model::RevisionMeta;
    use crate::model::Run;
    use crate::model::Segment;
    use crate::model::SegmentKind;
    use crate::model::Theme;
    use crate::model::table::{BlockStep, Blocks, block_at_mut_in};
    use crate::model::utf16_len;
    use crate::model::vml_display;
    use crate::model::{
        Block, ImageBlock, ListRef, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TextBlock,
    };
    use crate::model::{BodyClass, ParaClass, classify_body_child, classify_paragraph, text_kind};
    use crate::model::{Comments, Notes};
    use crate::model::{Display, drawing_display};
    use crate::model::{FontTable, Numbering, Settings, Styles};
    use crate::model::{HfKind, SectionInfo};
    use crate::package::{Package, PartId, RelTarget, RelType, Rels};
    use crate::semantic::props::{
        ParaProps, RunProps, read_para_props, read_run_props, read_run_props_change,
    };
    use crate::span::field::{FieldForm, FieldId, FieldIndex};
    use crate::span::{FlowMap, RangeClass, SpanId, SpanIndex, is_range_marker};
    use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

    /// 文档模型（`MOD-01`）：DOM + Span 的语义投影。
    #[derive(Debug, Clone, PartialEq)]
    pub struct Document {
        pub main_part: PartId,
        /// `w:body`；缺失时 `None`（`main` 为空，`warnings` 有诊断）。
        pub body: Option<NodeId>,
        pub main: Vec<Block>,
        pub styles: Option<Styles>,
        pub numbering: Option<Numbering>,
        pub theme: Option<Theme>,
        pub settings: Option<Settings>,
        pub font_table: Option<FontTable>,
        /// 正文的节序列（`MOD-10`，任务 5.2）。至少一个（没有 `w:sectPr` 时是隐式节）。
        pub sections: Vec<SectionInfo>,
        /// 页眉页脚 part（`MOD-01`，任务 5.3）：主 part 关系里 type 以 `/header` / `/footer` 结尾的**全部**
        /// part，含没被任何 `sectPr` 引用的孤儿（TS `parseAllHfParts` 也输出它们）。
        pub hf_parts: BTreeMap<PartId, HfPart>,
        /// 关系 id → 页眉页脚 part。`sectPr` 的引用与 `SectionInfo.hf_ref` 都是 `rId`，查 part 走这里。
        pub hf_by_rel: BTreeMap<String, PartId>,
        /// 正文引用的其他 part 的内容流索引（目前只有外部文本框 part，`wps:txbx/@r:txbx`）。
        /// 那些 part 的块挂在 `ShapeDisplay.content` 上、`content_part` 指回这里的键。
        pub aux_flows: BTreeMap<PartId, AuxFlows>,
        /// 主 part 关系里的图表 part（`chart` / `chartEx` 型，M6 6.1），含没被任何绘图引用的。
        /// 绘图侧的引用是 `DrawingDisplay.chart`（`rel_id`），经 [`Document::chart_by_rel`] 到这里。
        pub chart_parts: BTreeMap<PartId, ChartPart>,
        pub chart_by_rel: BTreeMap<String, PartId>,
        /// 主 part 里的墨迹批注（`aidocs-ink` 浮动图片 run，任务 6.8），文档序；对分类与坐标流不可见。
        pub inks: Vec<crate::model::InkInfo>,
        /// 主 part 引用的 SmartArt（按**数据 part** 的 `PartId`，任务 6.3）。绘图侧的引用是
        /// `DrawingDisplay.diagram`（`@r:dm`），经 [`Document::diagram_by_rel`] 到这里。
        pub diagram_parts: BTreeMap<PartId, DiagramPart>,
        pub diagram_by_rel: BTreeMap<String, PartId>,
        /// 主 part 的内容流映射（`SPAN-01`）。
        pub flows: FlowMap,
        /// 主 part 的字段索引（`FLD-02`）。与投影同寿命：`rebuild` / `refresh_blocks` 都重建它。
        pub fields: FieldIndex,
        /// 主 part 的范围索引（`SPAN-04`）。**这是投影侧的副本**：编辑期的规范状态在
        /// `EditSession.spans` 里，由 `SPAN-06` 变换维护；这一份只用来读（`Run.comments` 等）。
        pub spans: SpanIndex,
        /// 参考文献源（`customXml` 里的 `b:Sources`，任务 5.7）与它所在的 part。
        pub sources: Vec<crate::model::Source>,
        pub sources_part: Option<PartId>,
        /// 全包的修订表（`MOD-09`，任务 7.1）：`Block.revisions` / `Run.rev` 是压平过的投影，
        /// 这一份直接扫 DOM，每一层承载元素一条，接受 / 拒绝与 `EDIT-06` 的 `w:id` 分配都读它。
        pub revisions: crate::model::revision::RevisionIndex,
        /// 批注（`comments.xml` + `commentsExtended.xml` + `commentsIds.xml`）。
        pub comments: Comments,
        pub footnotes: Notes,
        pub endnotes: Notes,
        pub warnings: Vec<Diagnostic>,
    }

    impl Document {
        /// 从包完整构建（`MOD-13`）。辅助 part 先按关系找，找不到再按约定路径
        /// （TS 行为；见 `bind/compat_ts/KNOWN_DIFFS.md`）。
        pub fn rebuild(pkg: &mut Package) -> Result<Document> {
            let main = pkg.main_part();
            let aux = |pkg: &Package, kind: RelType, name: &str| {
                pkg.related(main, kind).next().or_else(|| pkg.find_name(name))
            };
            // 文献源 part 按根元素找（customXml 的关系类型对每个 item 都一样）；要在下面借出
            // 各 part 的 DOM 之前做完，它自己要 `&mut pkg`
            let sources_part = crate::model::find_part(pkg);
            let styles_id = aux(pkg, RelType::Styles, "word/styles.xml");
            let comments_id = aux(pkg, RelType::Comments, "word/comments.xml");
            let comments_ex_id = aux(pkg, RelType::CommentsExtended, "word/commentsExtended.xml");
            let comments_ids_id = aux(pkg, RelType::CommentsIds, "word/commentsIds.xml");
            let footnotes_id = aux(pkg, RelType::Footnotes, "word/footnotes.xml");
            let endnotes_id = aux(pkg, RelType::Endnotes, "word/endnotes.xml");
            let numbering_id = aux(pkg, RelType::Numbering, "word/numbering.xml");
            let settings_id = aux(pkg, RelType::Settings, "word/settings.xml");
            let theme_id = aux(pkg, RelType::Theme, "word/theme/theme1.xml");
            let font_id = aux(pkg, RelType::FontTable, "word/fontTable.xml");
            // 先确保都已解析，再同时借出
            pkg.dom(main)?;
            for id in [
                styles_id,
                numbering_id,
                settings_id,
                theme_id,
                font_id,
                comments_id,
                comments_ex_id,
                comments_ids_id,
                footnotes_id,
                endnotes_id,
            ]
            .into_iter()
            .flatten()
            {
                let _ = pkg.dom(id);
            }
            // 页眉页脚 part：先把 (rId, PartId, kind) 抄出来（关系表的借用要在 `pkg.dom` 之前结束），
            // 再逐个解析。type 以 `/header` / `/footer` 结尾的关系**全都**要，包括没被任何 `sectPr`
            // 引用的孤儿 part（TS `parseAllHfParts` 同样输出它们）
            let hf_rels: Vec<(String, PartId, HfKind)> =
                [(RelType::Header, HfKind::Header), (RelType::Footer, HfKind::Footer)]
                    .into_iter()
                    .flat_map(|(rel, kind)| {
                        pkg.part(main).rels.of_kind(rel).filter_map(move |r| {
                            let RelTarget::Internal(u) = &r.target else { return None };
                            Some((r.id.clone(), u.clone(), kind))
                        })
                    })
                    .filter_map(|(id, uri, kind)| pkg.find(&uri).map(|p| (id, p, kind)))
                    .collect();
            for (_, id, _) in &hf_rels {
                let _ = pkg.dom(*id);
            }

            // 外部文本框 part（`wps:txbx/@r:txbx` → `word/txbx*.xml`，任务 5.4d）：同页眉页脚，
            // 关系表的借用要在 `pkg.dom` 之前结束
            let txbx_rels: Vec<(String, PartId)> = pkg
                .part(main)
                .rels
                .of_kind(RelType::Txbx)
                .filter_map(|r| match &r.target {
                    RelTarget::Internal(u) => Some((r.id.clone(), u.clone())),
                    RelTarget::External(_) => None,
                })
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|(id, uri)| pkg.find(&uri).map(|p| (id, p)))
                .collect();
            for (_, id) in &txbx_rels {
                let _ = pkg.dom(*id);
            }
            // 图表 part（`c:chart r:id` / `cx:chart r:id`，任务 6.1）：同样先抄关系再解析。
            // 关系指向的 part 不在包里 → 记 `PKG_REL_MISSING`（悬空的 `r:id` 在块建好之后另查）
            let mut chart_rels: Vec<(String, PartId)> = Vec::new();
            let mut chart_rels_missing: Vec<String> = Vec::new();
            for kind in [RelType::Chart, RelType::ChartEx] {
                for r in pkg.part(main).rels.of_kind(kind) {
                    let RelTarget::Internal(u) = &r.target else { continue };
                    match pkg.find(u) {
                        Some(p) => chart_rels.push((r.id.clone(), p)),
                        None => chart_rels_missing.push(format!("{}（{}）", r.id, u.as_str())),
                    }
                }
            }
            for (_, id) in &chart_rels {
                let _ = pkg.dom(*id);
            }
            // SmartArt（`dgm:relIds/@r:dm`，任务 6.3）：数据 part 走关系；绘图 part 优先走数据 part 自己的
            // `diagramDrawing` 关系（ECMA-376 没有它，是 Word 2007 的扩展，真实文档都写），没有再按 TS 的
            // 路径约定 `data{N}.xml → drawing{N}.xml`。两个 part 都在这里解析。
            let mut diagram_rels: Vec<(String, PartId, Option<PartId>)> = Vec::new();
            let mut diagram_rels_missing: Vec<String> = Vec::new();
            for r in pkg.part(main).rels.of_kind(RelType::DiagramData) {
                let RelTarget::Internal(u) = &r.target else { continue };
                match pkg.find(u) {
                    Some(data) => {
                        let by_rel =
                            pkg.part(data).rels.of_kind(RelType::DiagramDrawing).find_map(|d| {
                                match &d.target {
                                    RelTarget::Internal(u) => pkg.find(u),
                                    RelTarget::External(_) => None,
                                }
                            });
                        let drawing = by_rel.or_else(|| {
                            let name = u.as_str();
                            let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
                            let n = file.strip_prefix("data")?.strip_suffix(".xml")?;
                            if !n.chars().all(|c| c.is_ascii_digit()) {
                                return None;
                            }
                            let sep = if dir.is_empty() { "" } else { "/" };
                            pkg.find_name(&format!("{dir}{sep}drawing{n}.xml"))
                        });
                        diagram_rels.push((r.id.clone(), data, drawing));
                    }
                    None => diagram_rels_missing.push(format!("{}（{}）", r.id, u.as_str())),
                }
            }
            for (_, data, drawing) in &diagram_rels {
                let _ = pkg.dom(*data);
                if let Some(d) = drawing {
                    let _ = pkg.dom(*d);
                }
            }

            let mut warnings = Vec::new();
            let dom_of = |id: Option<PartId>| id.and_then(|id| pkg.part(id).dom());
            let styles = dom_of(styles_id).and_then(|d| Styles::from_dom(d, &mut warnings));
            let numbering =
                dom_of(numbering_id).and_then(|d| Numbering::from_dom(d, &mut warnings));
            let settings = dom_of(settings_id).and_then(|d| Settings::from_dom(d, &mut warnings));
            let theme = dom_of(theme_id).and_then(Theme::from_dom);
            for rid in chart_rels_missing {
                warnings.push(Diagnostic::pre_existing(
                    main,
                    None,
                    DiagCode::PkgRelMissing,
                    format!("图表关系 {rid} 指向的 part 不在包里"),
                ));
            }
            for rid in diagram_rels_missing {
                warnings.push(Diagnostic::pre_existing(
                    main,
                    None,
                    DiagCode::PkgRelMissing,
                    format!("SmartArt 数据关系 {rid} 指向的 part 不在包里"),
                ));
            }
            let mut diagram_parts = BTreeMap::new();
            let mut diagram_by_rel = BTreeMap::new();
            for (rel_id, data, drawing) in diagram_rels {
                diagram_by_rel.insert(rel_id, data);
                if diagram_parts.contains_key(&data) {
                    continue;
                }
                let dp = DiagramPart::build(
                    data,
                    pkg.part(data).dom(),
                    drawing.map(|d| (d, pkg.part(d).dom())),
                    &mut warnings,
                );
                diagram_parts.insert(data, dp);
            }
            // 图表颜色按文档的配色方案解（没有 theme part 时按内建 Office 调色板，`RES-05`）
            let scheme = theme
                .as_ref()
                .and_then(|t| t.colors.clone())
                .unwrap_or_else(crate::model::ColorScheme::office_default);
            let mut chart_parts = BTreeMap::new();
            let mut chart_by_rel = BTreeMap::new();
            for (rel_id, id) in chart_rels {
                chart_by_rel.insert(rel_id, id);
                if chart_parts.contains_key(&id) {
                    continue;
                }
                let cp = ChartPart::build(id, pkg.part(id).dom(), &scheme, &mut warnings);
                chart_parts.insert(id, cp);
            }
            let sources = dom_of(sources_part).map(crate::model::read).unwrap_or_default();
            let font_table = dom_of(font_id).and_then(|d| FontTable::from_dom(d, &mut warnings));
            let with_dom = |id: Option<PartId>| id.and_then(|i| pkg.part(i).dom().map(|d| (i, d)));
            // 条目内容复用正文管线（任务 5.3）：要那个 part 自己的 rels（条目里的图片 / 链接按 part 解析）
            let rels_of = |id: Option<PartId>| id.map(|i| &pkg.part(i).rels);
            let comments = Comments::from_doms(
                with_dom(comments_id),
                with_dom(comments_ex_id),
                with_dom(comments_ids_id),
                rels_of(comments_id),
                styles.as_ref(),
                &mut warnings,
            );
            let footnotes = Notes::from_dom(
                with_dom(footnotes_id),
                LocalName::Footnote,
                LocalName::FootnoteRef,
                rels_of(footnotes_id),
                styles.as_ref(),
                &mut warnings,
            );
            let endnotes = Notes::from_dom(
                with_dom(endnotes_id),
                LocalName::Endnote,
                LocalName::EndnoteRef,
                rels_of(endnotes_id),
                styles.as_ref(),
                &mut warnings,
            );

            let dom = pkg.part(main).dom().expect("main part parsed above");
            let rels = &pkg.part(main).rels;
            let flows = FlowMap::build(dom);
            let mut fields = FieldIndex::build(dom);
            warnings.extend(fields.take_diagnostics());
            let mut spans = SpanIndex::build(dom);
            warnings.extend(
                spans.diagnostics().iter().filter(|d| d.code == DiagCode::SpanNoFlow).cloned(),
            );
            // `SPAN-10`：端点落在原子字段内部时移到原子边界（7.5）
            spans.snap_to_field_atoms(dom, &fields);
            let ext_txbx: crate::model::ExtTxbxMap<'_> = txbx_rels
                .iter()
                .filter_map(|(rid, id)| {
                    let d = pkg.part(*id).dom()?;
                    let idx = AuxFlows::build(*id, d, &mut warnings);
                    Some((
                        rid.clone(),
                        crate::model::ExtTxbxPart {
                            part: *id,
                            dom: d,
                            rels: &pkg.part(*id).rels,
                            idx,
                        },
                    ))
                })
                .collect();
            let mut b = Builder::new(dom, styles.as_ref(), rels, &fields, &spans, warnings)
                .with_ext_txbx(&ext_txbx);
            let body = b.find_body();
            let mut blocks = Vec::new();
            if let Some(body) = body {
                b.build_container(body, None, &[], &mut blocks);
            }
            let mut warnings = b.warnings;
            let aux_flows: BTreeMap<PartId, AuxFlows> =
                ext_txbx.into_values().map(|e| (e.part, e.idx)).collect();
            let sections = crate::model::build_sections(dom, &blocks, &mut warnings);
            // 页眉页脚 part：同一个构建器，各自的 DOM 与 rels（`SPAN-01` 独立内容流）
            let mut hf_parts = BTreeMap::new();
            let mut hf_by_rel = BTreeMap::new();
            for (rel_id, id, kind) in hf_rels {
                hf_by_rel.insert(rel_id, id);
                if hf_parts.contains_key(&id) {
                    continue; // 多个 rId 指向同一个 part（Word 的"同前"）
                }
                let Some(hf_dom) = pkg.part(id).dom() else { continue };
                if let Some(hf) = HfPart::build(
                    id,
                    hf_dom,
                    kind,
                    styles.as_ref(),
                    &pkg.part(id).rels,
                    &mut warnings,
                ) {
                    hf_parts.insert(id, hf);
                }
            }
            // 悬空的 `w:headerReference` / `w:footerReference`：`.rels` 里没有那个 `r:id`。
            // 那个槽读成"没声明"（`RES-10` 会继续往上一节继承），但要留一条诊断——
            // 编辑器据此能告诉用户"这一节的页眉丢了"，而不是默默显示上一节的
            for info in &sections {
                for (kind, variant, rid) in info.declared_refs() {
                    if !hf_by_rel.contains_key(rid) {
                        let range = info
                            .node
                            .and_then(|n| dom.node(n).lex.as_ref().map(|l| l.range.clone()));
                        warnings.push(Diagnostic::pre_existing(
                            main,
                            range,
                            DiagCode::PkgRelMissing,
                            format!("{kind}/{variant} 引用的关系 {rid} 不存在"),
                        ));
                    }
                }
            }
            // 绘图里 `c:chart r:id` 悬空（关系表里没有那个 id）：TS 解析不出图表、块仍是 `Chart` 芯片；
            // 留一条诊断让编辑器能说"这张图表的数据丢了"
            for b in Blocks::over(&blocks) {
                let Block::Protected(pb) = b else { continue };
                let Some(d) = pb.display.as_ref().and_then(Display::as_drawing) else { continue };
                if let Some(chart) = d.chart.as_ref()
                    && chart.rel_id.as_ref().is_none_or(|rid| !chart_by_rel.contains_key(rid))
                {
                    warnings.push(Diagnostic::pre_existing(
                        main,
                        dom.node(chart.node).lex.as_ref().map(|l| l.range.clone()),
                        DiagCode::PkgRelMissing,
                        match &chart.rel_id {
                            Some(rid) => format!("图表引用的关系 {rid} 不存在"),
                            None => "图表引用没有 r:id".to_string(),
                        },
                    ));
                }
                // SmartArt 的 `@r:dm` 悬空同理（`smartart-ole__006`）：块仍是 `SmartArt` 芯片，没有文字与形状
                if let Some(dg) = d.diagram.as_ref()
                    && dg.rel_id.as_ref().is_none_or(|rid| !diagram_by_rel.contains_key(rid))
                {
                    warnings.push(Diagnostic::pre_existing(
                        main,
                        dom.node(dg.node).lex.as_ref().map(|l| l.range.clone()),
                        DiagCode::PkgRelMissing,
                        match &dg.rel_id {
                            Some(rid) => format!("SmartArt 引用的关系 {rid} 不存在"),
                            None => "SmartArt 引用没有 r:dm".to_string(),
                        },
                    ));
                }
            }
            let inks = crate::model::collect_inks(dom, &blocks);
            let mut doc = Document {
                main_part: main,
                body,
                main: blocks,
                inks,
                sections,
                hf_parts,
                hf_by_rel,
                aux_flows,
                chart_parts,
                chart_by_rel,
                diagram_parts,
                diagram_by_rel,
                styles,
                numbering,
                theme,
                settings,
                font_table,
                flows,
                fields,
                spans,
                sources,
                sources_part,
                comments,
                footnotes,
                endnotes,
                revisions: crate::model::revision::RevisionIndex::default(),
                warnings,
            };
            doc.rebuild_revisions(pkg);
            Ok(doc)
        }

        /// 重扫全包的修订表（`MOD-09`）。part 顺序 = 主 part → 页眉页脚 → 脚注 → 尾注 → 批注 →
        /// 外部文本框，各自内部前序，合起来就是文档序。
        pub(crate) fn rebuild_revisions(&mut self, pkg: &Package) {
            use crate::model::revision::{RevPart, RevisionIndex};
            let mut parts: Vec<RevPart<'_>> = Vec::new();
            if let Some(d) = pkg.part(self.main_part).dom() {
                parts.push(RevPart { part: self.main_part, dom: d, fields: Some(&self.fields) });
            }
            for hf in self.hf_parts.values() {
                if let Some(d) = pkg.part(hf.part).dom() {
                    parts.push(RevPart { part: hf.part, dom: d, fields: Some(&hf.idx.fields) });
                }
            }
            for notes in [&self.footnotes, &self.endnotes] {
                if let (Some(id), Some(idx)) = (notes.part, notes.idx.as_ref())
                    && let Some(d) = pkg.part(id).dom()
                {
                    parts.push(RevPart { part: id, dom: d, fields: Some(&idx.fields) });
                }
            }
            if let (Some(id), Some(idx)) = (self.comments.part, self.comments.idx.as_ref())
                && let Some(d) = pkg.part(id).dom()
            {
                parts.push(RevPart { part: id, dom: d, fields: Some(&idx.fields) });
            }
            for (id, aux) in &self.aux_flows {
                if let Some(d) = pkg.part(*id).dom() {
                    parts.push(RevPart { part: *id, dom: d, fields: Some(&aux.fields) });
                }
            }
            let mut warnings = Vec::new();
            let index = RevisionIndex::build(&parts, &mut warnings);
            drop(parts);
            self.revisions = index;
            self.warnings.extend(warnings);
        }

        /// 只建正文（测试与工具用）：`dom` 是主 part。
        pub fn build_main(
            dom: &Dom,
            styles: Option<&Styles>,
            rels: &Rels,
        ) -> (Vec<Block>, Vec<Diagnostic>) {
            let fields = FieldIndex::build(dom);
            let spans = SpanIndex::build(dom);
            let mut b = Builder::new(dom, styles, rels, &fields, &spans, Vec::new());
            let mut blocks = Vec::new();
            if let Some(body) = b.find_body() {
                b.build_container(body, None, &[], &mut blocks);
            }
            (blocks, b.warnings)
        }

        pub fn text_blocks(&self) -> impl Iterator<Item = &TextBlock> {
            self.main.iter().filter_map(Block::as_text)
        }

        /// 容器级刷新（`MOD-13` 的 `refresh`，任务 3.6 / 3.7）：按 [`Document::block_path`] 就地重建给定
        /// 的块——`w:p` 走段落构建、`w:tbl` 走表格构建，**正文顶层与任意深度的单元格内一视同仁**，
        /// 保留它的 sdt / 修订上下文。返回在投影里找不到的节点（调用方据此退回整体重建）。
        ///
        /// 字段与范围索引是整个 part 的投影，跟着一起重建（只重建主 part；容器级的增量在 M7 随
        /// `TEST-07` 的随机序列一起评估）。
        pub fn refresh_blocks(
            &mut self,
            pkg: &mut Package,
            blocks: &[NodeId],
        ) -> Result<Vec<NodeId>> {
            let main = self.main_part;
            pkg.dom(main)?;
            let dom = pkg.part(main).dom().expect("main part parsed above");
            let rels = &pkg.part(main).rels;
            let fields = FieldIndex::build(dom);
            let mut spans = SpanIndex::build(dom);
            spans.snap_to_field_atoms(dom, &fields);
            // `SpanId` 是按文档序编的号（`SPAN-04`）。中间多出或少掉一个范围，它后面的全部改号——
            // 而增量刷新只重建被碰过的块，没刷的块里 `Run.comments` 还存着旧号，那号现在指着**别的**
            // 范围。识别到改号就整体重建（`TEST-07` 在「先加批注、后加书签」上抓到的）
            if span_identities(&self.spans) != span_identities(&spans)
                || field_identities(&self.fields) != field_identities(&fields)
            {
                return Ok(blocks.to_vec());
            }
            let mut missing = Vec::new();
            // 先把路径与上下文取齐，再借出块表——构建器借着 `self.styles`
            let mut work: Vec<RefreshItem> = Vec::new();
            for &p in blocks {
                match self.block_path(p).and_then(|path| {
                    let blk = self.block_at(&path)?;
                    Some((path, blk.sdt().cloned(), wrapper_revisions(blk)))
                }) {
                    Some((path, sdt, revs)) => work.push((p, path, sdt, revs)),
                    None => missing.push(p),
                }
            }
            let mut b = Builder::new(dom, self.styles.as_ref(), rels, &fields, &spans, Vec::new());
            let mut main = std::mem::take(&mut self.main);
            for (p, path, sdt, revs) in work {
                // body 级 `w:sectPr` 也是一个块（`MOD-01` 的 R01），但它没有段落 / 表格的内容流，
                // 交给 `build_paragraph` 会得出一个假段落，节投影随后就崩（`MOD-10` 的 owner 断言）。
                // 整体重建时 `depth` 是一路 `build_container` 累出来的（正文 1、每进一层单元格 /
                // 文本框 +1），`MOD-07` 的 `TooDeep` 就看它。增量刷新直接从这一块开始，得把那个
                // 层数补回来，不然深层的嵌套表在这里会被完整建出来、与重建不一致
                b.depth = 1 + dom
                    .ancestors(p)
                    .filter(|&a| {
                        dom.is(a, QName::w(LocalName::Tc))
                            || dom.is(a, QName::w(LocalName::TxbxContent))
                    })
                    .count() as u32;
                let rebuilt = if dom.is(p, QName::w(LocalName::Tbl)) {
                    b.build_table(p, sdt.as_ref(), &revs)
                } else if dom.is(p, QName::w(LocalName::SectPr)) {
                    section_props_block(p, sdt.as_ref(), &revs)
                } else {
                    b.build_paragraph(p, sdt.as_ref(), &revs)
                };
                // 内容在别的 part 里的文本框：增量建不出来（外部 part 的索引只在整体重建时装好）
                if crate::model::table::has_external_textbox(&rebuilt) {
                    missing.push(p);
                }
                match block_at_mut_in(&mut main, &path) {
                    Some(slot) => *slot = rebuilt,
                    None => missing.push(p),
                }
            }
            self.main = main;
            let mut warnings = b.warnings;
            // 节是块序的投影：刷新一个段落可能加上或去掉它的 `pPr/sectPr`，所以一起重算
            // （块数不变，`block_range` 的下标还有效；块增删走 `structure_changed` 的整体重建）
            self.sections = crate::model::build_sections(dom, &self.main, &mut warnings);
            // 墨迹表同理是块的投影：刷新的段落可能多了或少了墨迹 run
            self.inks = crate::model::collect_inks(dom, &self.main);
            self.warnings.extend(warnings);
            self.fields = fields;
            // 内容编辑也会新建 run / 修订节点；块数不变不代表 arena 的流映射不变。
            self.flows = FlowMap::build(dom);
            self.spans = spans;
            // 修订表是 DOM 的投影，和字段索引一样整体重建（辅助 part 走整体 `rebuild`，这里只有主 part 变了，
            // 但重扫全部 part 才能让 `EDIT-06` 的全局 `w:id` 最大值始终正确）
            self.rebuild_revisions(pkg);
            Ok(missing)
        }

        /// 管辖某个节点的节下标（`RES-10` 的 `section_of`）。
        // （`wrapper_revisions` 见文件末尾）
        pub fn section_of(&self, dom: &Dom, node: NodeId) -> Option<usize> {
            crate::model::section_of(dom, &self.sections, node)
        }

        /// 任意深度的文本段落（含单元格内），按节点找。
        pub fn text_block(&self, para: NodeId) -> Option<&TextBlock> {
            self.blocks().find(|b| b.node() == para).and_then(Block::as_text)
        }
    }

    /// body 级 `w:sectPr` 的块（`MOD-01` R01）：没有内容流，只占一个块位，让节投影能定位它。
    fn section_props_block(node: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Block {
        Block::Protected(ProtectedBlock {
            node,
            kind: ProtectedKind::SectionProps,
            preview: String::new(),
            display: None,
            siblings: Vec::new(),
            sdt: sdt.cloned(),
            revisions: revs.to_vec(),
        })
    }

    /// 正文构建器；表格部分在 `model/table.rs`（同一个类型的另一组方法）。
    pub(super) struct Builder<'a> {
        pub(super) dom: &'a Dom,
        styles: Option<&'a Styles>,
        rels: &'a Rels,
        fields: &'a FieldIndex,
        spans: &'a SpanIndex,
        /// `FLD-08`：被 `Block` 策略字段覆盖的段落 → 字段 id。
        block_fields: HashMap<NodeId, FieldId>,
        /// 字段起点所在的段落 → 字段 id（`MOD-04` 的 `facts.fields`）。
        fields_by_para: HashMap<NodeId, Vec<FieldId>>,
        /// 外部文本框 part（`wps:txbx/@r:txbx`）：空表表示这份文档没有（多数情况）。
        ext_txbx: &'a crate::model::ExtTxbxMap<'a>,
        pub(super) warnings: Vec<Diagnostic>,
        /// 当前嵌套的容器层数（body / sdtContent / 修订包裹 / 单元格都算一层，段落内的内联容器也算）；
        /// 块容器超过 [`MAX_CONTAINER_DEPTH`] 层的子树降级为 `TooDeep`（`MOD-07`）。
        pub(super) depth: u32,
        /// 当前嵌在第几层框里（`w:txbxContent`）。
        ///
        /// 框里的段落走的是"容器 → 段落 → 内联 → 框内容 → 容器"这条**递归**，每一层都在栈上压一组
        /// 属性结构体（`ParaProps` / `RunProps` 几 KB）。块容器的 64 层预算换算成框大约 33 层，
        /// 那已经够把测试线程的 2 MiB 栈用光（同 M3 在嵌套表格上踩过的那一条）。框套框在真实文档里
        /// 最多两层，所以给框单独一个小预算，超过就整段降级为 `TooDeep`。
        box_depth: u32,
        /// `w:txbxContent` → 它的块（每个 part 一份，随 `Builder` 同寿命）。
        ///
        /// `vml_display` 会把整棵 `w:pict` 里的形状**摊平**成一张表，别人框里的形状也在表里
        /// （compat 要按 TS 的形态把它们当只读的兄弟框输出）。于是同一段 `w:txbxContent` 会被
        /// 摊平表里的每个外层形状各建一次——套娃 n 层就是 2^n 次。记忆化让每段内容只建一次。
        box_content: HashMap<NodeId, Vec<Block>>,
        /// 当前段落开始时的 `depth`：内联容器的深度上限相对它计，块的嵌套不占内联的额度
        /// （第 64 层表格里的段落照样要能建 inlines）。
        inline_base: u32,
        /// 本段（或它里面嵌着的段落）的内联容器嵌套超过了上限：整段降级为 `TooDeep`（TS 解析失败时整段
        /// passthrough；我们不能一边丢掉深处的文字一边让段落可编辑，任务 6.9）。
        inline_too_deep: bool,
    }

    impl<'a> Builder<'a> {
        pub(super) fn new(
            dom: &'a Dom,
            styles: Option<&'a Styles>,
            rels: &'a Rels,
            fields: &'a FieldIndex,
            spans: &'a SpanIndex,
            warnings: Vec<Diagnostic>,
        ) -> Self {
            let mut fields_by_para: HashMap<NodeId, Vec<FieldId>> = HashMap::new();
            for f in fields.fields() {
                let head = f.form.head();
                if let Some(p) = std::iter::once(head)
                    .chain(dom.ancestors(head))
                    .find(|&n| dom.is(n, QName::w(LocalName::P)))
                {
                    fields_by_para.entry(p).or_default().push(f.id);
                }
            }
            Builder {
                dom,
                styles,
                rels,
                ext_txbx: crate::model::empty_ext_txbx(),
                fields,
                spans,
                block_fields: fields.block_result_paragraphs(dom),
                fields_by_para,
                warnings,
                depth: 0,
                box_depth: 0,
                box_content: HashMap::new(),
                inline_base: 0,
                inline_too_deep: false,
            }
        }
    }

    pub(super) const MAX_CONTAINER_DEPTH: u32 = 64;

    /// 一次刷新里要重建的一段：节点、它在块表里的路径、以及要保留的 sdt / 修订上下文。
    type RefreshItem = (NodeId, Vec<BlockStep>, Option<SdtInfo>, Vec<Revision>);

    fn w(local: LocalName) -> QName {
        QName::w(local)
    }

    impl<'a> Builder<'a> {
        /// 挂上外部文本框 part 表（`Document::rebuild` 用；别的入口没有别的 part 可给）。
        pub(super) fn with_ext_txbx(
            mut self,
            map: &'a crate::model::ExtTxbxMap<'a>,
        ) -> Builder<'a> {
            self.ext_txbx = map;
            self
        }

        fn find_body(&mut self) -> Option<NodeId> {
            let dom = self.dom;
            let root = dom.root();
            if !dom.is(root, w(LocalName::Document)) {
                self.warn(root, DiagCode::ModUnparseable, "主 part 根不是 w:document");
                return None;
            }
            let body = dom.semantic_children(root).find(|&n| dom.is(n, w(LocalName::Body)));
            if body.is_none() {
                self.warn(root, DiagCode::ModUnparseable, "w:document 下没有 w:body");
            }
            body
        }

        pub(super) fn warn(&mut self, node: NodeId, code: DiagCode, message: impl Into<String>) {
            let range = self.dom.node(node).lex.as_ref().map(|l| l.range.clone());
            self.warnings.push(Diagnostic::pre_existing(self.dom.part(), range, code, message));
        }

        fn attr(&self, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
            self.dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
        }

        pub(super) fn meta(&self, node: NodeId) -> RevisionMeta {
            RevisionMeta {
                node,
                id: self.attr(node, NsId::W, LocalName::Id),
                author: self.attr(node, NsId::W, LocalName::Author),
                date: self.attr(node, NsId::W, LocalName::Date),
            }
        }

        // ---- 块 ------------------------------------------------------------------------------------

        /// body / sdtContent / 修订包裹 / customXml / 单元格的子节点 → 块（R01–R07）。
        pub(super) fn build_container(
            &mut self,
            container: NodeId,
            sdt: Option<&SdtInfo>,
            revs: &[Revision],
            out: &mut Vec<Block>,
        ) {
            let dom = self.dom;
            if self.depth > MAX_CONTAINER_DEPTH {
                self.warn(container, DiagCode::ModTooDeep, "块容器嵌套过深");
                out.push(Block::Protected(ProtectedBlock {
                    node: container,
                    kind: ProtectedKind::TooDeep,
                    preview: String::new(),
                    display: None,
                    siblings: Vec::new(),
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                }));
                return;
            }
            self.depth += 1;
            let children: Vec<NodeId> = dom.semantic_children(container).collect();
            for node in children {
                if dom.name(node).is_none() {
                    continue; // 空白文本
                }
                if dom.is(node, w(LocalName::TcPr)) {
                    continue; // 单元格属性：`Cell.props` 已读（`MOD-07`）
                }
                let (_rule, class) = classify_body_child(dom, node);
                match class {
                    BodyClass::SectionProps => out.push(section_props_block(node, sdt, revs)),
                    BodyClass::Table => {
                        let block = self.build_table(node, sdt, revs);
                        out.push(block);
                    }
                    BodyClass::Sdt => {
                        let info = SdtInfo::read(dom, node);
                        let content = dom
                            .semantic_children(node)
                            .find(|&n| dom.is(n, w(LocalName::SdtContent)));
                        let before = out.len();
                        if let Some(content) = content {
                            self.build_container(content, Some(&info), revs, out);
                        }
                        if out.len() == before {
                            out.push(Block::Protected(ProtectedBlock {
                                node,
                                kind: ProtectedKind::Invisible,
                                preview: String::new(),
                                display: None,
                                siblings: Vec::new(),
                                sdt: Some(info),
                                revisions: revs.to_vec(),
                            }));
                        }
                    }
                    BodyClass::RangeMarker => {}
                    BodyClass::BodyBreak { page } => out.push(Block::Protected(ProtectedBlock {
                        node,
                        kind: ProtectedKind::BodyBreak { page },
                        preview: String::new(),
                        display: None,
                        siblings: Vec::new(),
                        sdt: sdt.cloned(),
                        revisions: revs.to_vec(),
                    })),
                    BodyClass::InsertWrap
                    | BodyClass::DeleteWrap
                    | BodyClass::MoveFromWrap
                    | BodyClass::MoveToWrap => {
                        let meta = self.meta(node);
                        let rev = match class {
                            BodyClass::InsertWrap => Revision::Insert(meta),
                            BodyClass::DeleteWrap => Revision::Delete(meta),
                            BodyClass::MoveFromWrap => Revision::MoveFrom(meta),
                            _ => Revision::MoveTo(meta),
                        };
                        let mut inner = revs.to_vec();
                        inner.push(rev);
                        self.build_container(node, sdt, &inner, out);
                    }
                    BodyClass::Transparent => self.build_container(node, sdt, revs, out),
                    BodyClass::Unknown(name) => {
                        self.warn(
                            node,
                            DiagCode::ModUnknownBlock,
                            format!("无法分类的块级元素 {}", name.display(dom.interner())),
                        );
                        out.push(Block::Protected(ProtectedBlock {
                            node,
                            kind: ProtectedKind::Unknown(name),
                            preview: self.preview(node),
                            display: None,
                            siblings: Vec::new(),
                            sdt: sdt.cloned(),
                            revisions: revs.to_vec(),
                        }));
                    }
                    BodyClass::Paragraph => {
                        let block = self.build_paragraph(node, sdt, revs);
                        out.push(block);
                    }
                }
            }
            self.depth -= 1;
        }

        pub(super) fn build_paragraph(
            &mut self,
            p: NodeId,
            sdt: Option<&SdtInfo>,
            revs: &[Revision],
        ) -> Block {
            let dom = self.dom;
            // 内联深度上限相对本段起点计；退出时还原——文本框里的段落会在外层段落的 `build_inlines`
            // 中途嵌套进来（M4 的 `txbxContent` 走同一套段落管线），不还原的话外层会拿到更深的基线
            let outer_inline_base = self.inline_base;
            self.inline_base = self.depth;
            // 外层段落（文本框宿主）的标志先收起来：里面这段太深，外层随后也会被判太深（它包着这一段）
            let outer_too_deep = std::mem::replace(&mut self.inline_too_deep, false);
            let ppr = dom.semantic_children(p).find(|&n| dom.is(n, w(LocalName::PPr)));
            let props: ParaProps = read_para_props(dom, ppr, &mut self.warnings);
            let mut facts = ParagraphFacts::compute(dom, p, &props, self.styles, sdt.cloned());
            // `MOD-04`：字段事实来自 `FieldIndex`（`FLD-08` 的块字段覆盖段落 → R09）
            facts.fields = self.fields_by_para.get(&p).cloned().unwrap_or_default();
            facts.inside_field_result = self.block_fields.get(&p).copied();
            let (_rule, class) = classify_paragraph(&facts);
            let mut revisions = revs.to_vec();
            // 段落标记修订与 pPrChange（MOD-09）
            if let Some(rpr) = &props.rpr {
                for &n in &rpr.raw_unmodeled {
                    match dom.name(n).map(|q| q.local) {
                        Some(LocalName::Ins) if dom.name(n).is_some_and(|q| q.ns == NsId::W) => {
                            revisions.push(Revision::ParaMarkInsert(self.meta(n)));
                        }
                        Some(LocalName::Del) if dom.name(n).is_some_and(|q| q.ns == NsId::W) => {
                            revisions.push(Revision::ParaMarkDelete(self.meta(n)));
                        }
                        _ => {}
                    }
                }
            }
            if facts.revision.ppr_change
                && let Some((change, old)) = read_para_props_change_of(dom, ppr, &mut self.warnings)
            {
                revisions.push(Revision::ParaPropsChange {
                    meta: self.meta(change),
                    old: Box::new(old),
                });
            }
            if let Some(num) = &props.num {
                for &n in &num.raw_unmodeled {
                    if dom.is(n, w(LocalName::NumberingChange)) {
                        revisions.push(Revision::NumberingChange(self.meta(n)));
                    }
                }
            }
            let block = match class {
                ParaClass::Protected(kind) => Block::Protected(ProtectedBlock {
                    node: p,
                    kind: kind.clone(),
                    preview: self.preview(p),
                    // 公式段落的载荷是公式本身（6.5）；其余保护块是段落里第一个图形
                    display: match kind {
                        ProtectedKind::Equation => Some(Display::Formula(Box::new(
                            crate::model::formula_display(dom, p, facts.visible_text),
                        ))),
                        _ => graphic_display(dom, &facts),
                    },
                    siblings: graphic_siblings(dom, &facts),
                    sdt: sdt.cloned(),
                    revisions,
                }),
                // R15 保证该段恰有一个绘图或一个 VML 图片。
                ParaClass::Image => Block::Image(ImageBlock {
                    node: p,
                    display: graphic_display(dom, &facts),
                    sdt: sdt.cloned(),
                    revisions,
                }),
                ParaClass::Text => {
                    let mut inlines = Vec::new();
                    self.build_inlines(p, None, None, &mut inlines);
                    self.attach_comments(p, &mut inlines);
                    if self.inline_too_deep {
                        // 深处的内容没有进内联模型：整段只读，字节原样（`MOD-07`；TS 同样整段 passthrough）
                        Block::Protected(ProtectedBlock {
                            node: p,
                            kind: ProtectedKind::TooDeep,
                            preview: self.preview(p),
                            display: None,
                            siblings: Vec::new(),
                            sdt: sdt.cloned(),
                            revisions,
                        })
                    } else {
                        Block::Text(Box::new(TextBlock {
                            node: p,
                            kind: text_kind(&facts),
                            style_id: props.style.clone(),
                            props,
                            inlines,
                            sdt: sdt.cloned(),
                            revisions,
                            facts,
                        }))
                    }
                }
            };
            self.inline_base = outer_inline_base;
            self.inline_too_deep |= outer_too_deep;
            block
        }

        /// `COMPAT-07` 的模型侧：给 run 挂批注 id。
        ///
        /// 规则同 TS：**起终点都在本段**的批注范围覆盖到的 run 挂它的 id（只有一端在本段的范围
        /// 由块级 `commentStarts` / `commentEnds` 表达，不挂到 run 上）；文件里只有
        /// `w:commentReference` 的批注（`implicit`，LibreOffice 风格）挂到最近的有字 run
        /// ——先往前找，没有再往后找。
        fn attach_comments(&self, para: NodeId, inlines: &mut [Inline]) {
            let dom = self.dom;
            let id_of = |n: NodeId| {
                dom.attr_value(n, w(LocalName::Id)).map(|v| v.into_owned()).unwrap_or_default()
            };
            // 段内的 start / end id：两端都在本段才算覆盖
            let mut starts: Vec<String> = Vec::new();
            let mut ends: Vec<String> = Vec::new();
            let mut has_ref = false;
            let mut stack = vec![para];
            while let Some(n) = stack.pop() {
                let Some(name) = dom.name(n) else { continue };
                if name == w(LocalName::TxbxContent) {
                    continue; // 独立内容流
                }
                if name == w(LocalName::CommentRangeStart) {
                    starts.push(id_of(n));
                } else if name == w(LocalName::CommentRangeEnd) {
                    ends.push(id_of(n));
                } else if name == w(LocalName::CommentReference) {
                    has_ref = true;
                }
                for &c in dom.children(n).iter().rev() {
                    stack.push(c);
                }
            }
            let both: Vec<&String> = starts.iter().filter(|s| ends.contains(s)).collect();
            if both.is_empty() && !has_ref {
                return;
            }
            // 文档序一遍：跟踪打开的范围，同时记下承载 reference 的 run
            let mut open: Vec<String> = Vec::new();
            let mut cover: HashMap<NodeId, Vec<SpanId>> = HashMap::new();
            let mut refs: Vec<(String, NodeId)> = Vec::new();
            let mut stack = vec![para];
            while let Some(n) = stack.pop() {
                let Some(name) = dom.name(n) else { continue };
                if name == w(LocalName::TxbxContent) {
                    continue;
                }
                if name == w(LocalName::CommentRangeStart) {
                    let id = id_of(n);
                    if both.iter().any(|b| **b == id) {
                        open.push(id);
                    }
                } else if name == w(LocalName::CommentRangeEnd) {
                    let id = id_of(n);
                    open.retain(|x| *x != id);
                } else if name == w(LocalName::R) {
                    if !open.is_empty() {
                        let ids: Vec<SpanId> =
                            open.iter().filter_map(|id| self.comment_span(id)).collect();
                        if !ids.is_empty() {
                            cover.insert(n, ids);
                        }
                    }
                    if let Some(c) = dom
                        .semantic_children(n)
                        .find(|&c| dom.is(c, w(LocalName::CommentReference)))
                    {
                        refs.push((id_of(c), n));
                    }
                }
                for &c in dom.children(n).iter().rev() {
                    stack.push(c);
                }
            }
            for (id, run) in refs {
                // 只有 reference 的批注（文件里没有范围标记）才挂最近的 run
                let Some(span) = self.comment_span(&id) else { continue };
                if !self.spans.get(span).is_some_and(|s| s.implicit) {
                    continue;
                }
                let Some(i) = inlines.iter().position(|x| x.node() == Some(run)) else { continue };
                let has_text = |x: &Inline| matches!(x, Inline::Run(r) if !r.text.is_empty());
                let target = inlines[..i]
                    .iter()
                    .rposition(has_text)
                    .or_else(|| inlines[i + 1..].iter().position(has_text).map(|k| k + i + 1));
                if let Some(t) = target
                    && let Inline::Run(r) = &mut inlines[t]
                    && !r.comments.contains(&span)
                {
                    r.comments.push(span);
                }
            }
            for inline in inlines.iter_mut() {
                if let Inline::Run(r) = inline
                    && let Some(ids) = cover.get(&r.node)
                {
                    for id in ids {
                        if !r.comments.contains(id) {
                            r.comments.push(*id);
                        }
                    }
                }
            }
        }

        /// 批注 `w:id` → 范围索引里的 `SpanId`。
        fn comment_span(&self, id: &str) -> Option<SpanId> {
            self.spans.find(RangeClass::Comment, id).map(|s| s.id)
        }

        /// 可见文本预览：`w:t` 文本拼接，截到 80 个字符。已删除的子树不算——那些字节还在 DOM 里
        /// （`XML-12` 的 `Deleted` 是标记不是移除），但它们不再是可见文本。
        fn preview(&self, node: NodeId) -> String {
            let dom = self.dom;
            let mut s = String::new();
            for n in dom.descendants(node) {
                if dom.node(n).dirty == crate::xml::Dirty::Deleted {
                    continue;
                }
                if dom.is(n, w(LocalName::T)) {
                    for c in dom.semantic_children(n) {
                        if let Some(t) = dom.text(c) {
                            s.push_str(&t);
                        }
                    }
                }
                if s.chars().count() > 80 {
                    break;
                }
            }
            let trimmed = s.trim();
            trimmed.chars().take(80).collect()
        }

        // ---- 内联 ----------------------------------------------------------------------------------

        /// 段落（或透明容器）的子节点 → inlines（`MOD-06`）。内联容器在段落内嵌套超过
        /// [`MAX_CONTAINER_DEPTH`] 层时不再下钻，整个子树作为一个 `Atom(Other)` 占位并记 `MOD_TOO_DEEP`
        /// （病态输入局部降级）；深度相对段落起点计，所以表格嵌套不吃这个额度。
        fn build_inlines(
            &mut self,
            container: NodeId,
            link: Option<&Link>,
            rev: Option<&RevisionCtx>,
            out: &mut Vec<Inline>,
        ) {
            let dom = self.dom;
            if self.depth.saturating_sub(self.inline_base) > MAX_CONTAINER_DEPTH {
                self.warn(container, DiagCode::ModTooDeep, "内联容器嵌套过深");
                self.inline_too_deep = true;
                let name = dom.name(container).expect("container is an element");
                out.push(Inline::Atom(InlineAtom {
                    node: container,
                    kind: AtomKind::Other(name),
                    props: RunProps::default(),
                }));
                return;
            }
            self.depth += 1;
            let children: Vec<NodeId> = dom.semantic_children(container).collect();
            self.build_inline_nodes(&children, link, rev, out);
            self.depth -= 1;
        }

        /// 一段兄弟节点 → inlines。字段的结果区也走这里（它是同一个容器里的一段子节点）。
        fn build_inline_nodes(
            &mut self,
            nodes: &[NodeId],
            link: Option<&Link>,
            rev: Option<&RevisionCtx>,
            out: &mut Vec<Inline>,
        ) {
            let dom = self.dom;
            let mut i = 0usize;
            while i < nodes.len() {
                let node = nodes[i];
                i += 1;
                let Some(name) = dom.name(node) else { continue };
                if is_range_marker(name) {
                    continue;
                }
                match (name.ns, name.local) {
                    (
                        NsId::W,
                        LocalName::PPr
                        | LocalName::ProofErr
                        | LocalName::SdtPr
                        | LocalName::SdtEndPr
                        | LocalName::CustomXmlPr
                        | LocalName::SmartTagPr,
                    ) => {}
                    (NsId::W, LocalName::R) => {
                        // `FLD-07` 原子形态：begin run 起，整段字段折成一个 `Inline::Field`
                        if let Some(next) = self.atomic_field_at(node, &nodes[i..], link, rev, out)
                        {
                            i += next;
                            continue;
                        }
                        let run = self.build_run(node, link, rev);
                        out.push(Inline::Run(run));
                    }
                    (NsId::W, LocalName::Hyperlink) => {
                        let target = match (
                            self.attr(node, NsId::R, LocalName::Id),
                            self.attr(node, NsId::W, LocalName::Anchor),
                        ) {
                            (Some(rid), _) => {
                                let href = self.rels.by_id(&rid).and_then(|r| match &r.target {
                                    RelTarget::External(h) => Some(h.clone()),
                                    RelTarget::Internal(_) => None,
                                });
                                LinkTarget::External { rel_id: rid, href }
                            }
                            (None, Some(anchor)) => LinkTarget::Internal { anchor },
                            (None, None) => LinkTarget::Unresolved,
                        };
                        let l = Link::Hyperlink {
                            node,
                            target,
                            tooltip: self.attr(node, NsId::W, LocalName::Tooltip),
                        };
                        self.build_inlines(node, Some(&l), rev, out);
                    }
                    (
                        NsId::W,
                        LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo,
                    ) => {
                        let meta = self.meta(node);
                        let mut ctx = rev.cloned().unwrap_or_default();
                        match name.local {
                            LocalName::Ins => ctx.ins = Some(meta),
                            LocalName::Del => ctx.del = Some(meta),
                            LocalName::MoveFrom => {
                                ctx.del = Some(meta.clone());
                                ctx.move_from = Some(meta);
                            }
                            _ => {
                                ctx.ins = Some(meta.clone());
                                ctx.move_to = Some(meta);
                            }
                        }
                        self.build_inlines(node, link, Some(&ctx), out);
                    }
                    (NsId::W, LocalName::FldSimple) => {
                        match self.fields.field_of(node).filter(|f| f.is_atomic()).map(|f| f.id) {
                            Some(id) => {
                                let mut result = Vec::new();
                                self.build_inlines(node, link, rev, &mut result);
                                out.push(Inline::Field { id, result });
                            }
                            None => self.build_inlines(node, link, rev, out),
                        }
                    }
                    (
                        NsId::W,
                        LocalName::SmartTag
                        | LocalName::Sdt
                        | LocalName::SdtContent
                        | LocalName::CustomXml
                        | LocalName::Dir
                        | LocalName::Bdo,
                    ) => self.build_inlines(node, link, rev, out),
                    (NsId::M, LocalName::OMath | LocalName::OMathPara) => {
                        out.push(Inline::Atom(InlineAtom {
                            node,
                            kind: AtomKind::Math,
                            props: RunProps::default(),
                        }));
                    }
                    (NsId::W, LocalName::Br) => {
                        let kind =
                            BreakKind::parse(self.attr(node, NsId::W, LocalName::Type).as_deref());
                        out.push(Inline::Atom(InlineAtom {
                            node,
                            kind: AtomKind::BareBreak { kind },
                            props: RunProps::default(),
                        }));
                    }
                    _ => out.push(Inline::Atom(InlineAtom {
                        node,
                        kind: AtomKind::Other(name),
                        props: RunProps::default(),
                    })),
                }
            }
        }

        /// `node` 是某个原子形态字段的 begin run 且该字段在这一段兄弟节点里闭合时，把整个字段折成
        /// 一个 [`Inline::Field`]，返回要跳过的节点数（含 end run）。
        ///
        /// 结果区是 separate 与 end 之间的节点；没有 separate（XE 一类无结果字段）时结果为空。
        /// 字段没在这一段兄弟节点里闭合（跨段 / 跨容器）时返回 `None`，各 run 照常出现——
        /// 那种字段的策略是 `Block`，段落已经被 R09 保护，不该到这里。
        fn atomic_field_at(
            &mut self,
            node: NodeId,
            rest: &[NodeId],
            link: Option<&Link>,
            rev: Option<&RevisionCtx>,
            out: &mut Vec<Inline>,
        ) -> Option<usize> {
            let f =
                self.fields.field_of(node).filter(|f| f.form.head() == node && f.is_atomic())?;
            let (id, tail, separate) = match &f.form {
                FieldForm::Complex { end, separate, .. } => (f.id, *end, *separate),
                FieldForm::Simple { .. } => return None,
            };
            let tail_at = rest.iter().position(|&c| c == tail)?;
            let result_from = match separate {
                Some(sep) => rest.iter().position(|&c| c == sep).map_or(tail_at, |k| k + 1),
                None => tail_at,
            };
            let mut result = Vec::new();
            if result_from < tail_at {
                let nodes: Vec<NodeId> = rest[result_from..tail_at].to_vec();
                self.build_inline_nodes(&nodes, link, rev, &mut result);
            }
            out.push(Inline::Field { id, result });
            Some(tail_at + 1)
        }

        /// 一个 `w:r` → `Run`：段与坐标流文本。
        fn build_run(&mut self, r: NodeId, link: Option<&Link>, rev: Option<&RevisionCtx>) -> Run {
            let dom = self.dom;
            let mut text = String::new();
            let mut segments = Vec::new();
            let mut props = RunProps::default();
            let mut rpr_node = None;
            for c in dom.semantic_children(r) {
                let Some(name) = dom.name(c) else { continue };
                if name.ns == NsId::W && name.local == LocalName::RPr {
                    rpr_node = Some(c);
                    props = read_run_props(dom, Some(c), &mut self.warnings);
                    continue;
                }
                let start = text.len() as u32;
                let kind = self.segment(c, name, &mut text);
                let end = text.len() as u32;
                let len = utf16_len(&text[start as usize..end as usize]);
                // `MOD-11` 显示模型：绘图段带 `DrawingDisplay`，`w:pict` / `w:object` 段带 `VmlDisplay`。
                let display = match kind {
                    SegmentKind::Drawing { .. } => {
                        let mut d = drawing_display(dom, c);
                        self.fill_shape_content(&mut d.shapes);
                        Some(Display::Drawing(Box::new(d)))
                    }
                    SegmentKind::Pict | SegmentKind::Object => {
                        let mut v = vml_display(dom, c);
                        if v.too_deep {
                            self.warn(c, DiagCode::ModTooDeep, "VML 框套得过深，摊平表已截断");
                        }
                        // 只给**最外层**的框建内容：别人框里的形状在那个框自己的投影里已经建过一遍
                        // （摊平表见 `vml_display`）。给它们各建一份会让内容树的规模随嵌套层数指数增长
                        // ——`corpus/hostile/hf-deep-txbx.docx` 就是这么把投影卡死的
                        self.fill_box_content(
                            v.shapes
                                .iter_mut()
                                .filter(|s| !s.nested)
                                .map(|s| (s.txbx, &mut s.content)),
                        );
                        Some(Display::Vml(Box::new(v)))
                    }
                    _ => None,
                };
                segments.push(Segment { node: c, kind, text: start..end, utf16_len: len, display });
            }
            let mut ctx = rev.cloned().unwrap_or_default();
            if let Some(rpr) = rpr_node
                && let Some((change, old)) =
                    read_run_props_change(dom, Some(rpr), &mut self.warnings)
            {
                ctx.props_change = Some((self.meta(change), Box::new(old)));
            }
            let utf16 = utf16_len(&text);
            // `FLD-07` 透明形态（`Link` 策略）：结构 run 与结果 run 都带 `field`，结果 run 另有
            // `Link::Field`（目标来自指令，由 `compat_ts` / 渲染器解析）。外层 `w:hyperlink` 优先。
            let transparent = self.fields.field_of(r).filter(|f| f.is_transparent());
            let link = match (link, transparent) {
                (None, Some(f)) if f.form.result_nodes().contains(&r) => Some(Link::Field(f.id)),
                (l, _) => l.cloned(),
            };
            Run {
                node: r,
                segments,
                text,
                utf16_len: utf16,
                props,
                link,
                field: transparent.map(|f| f.id),
                rev: (!ctx.is_empty()).then_some(ctx),
                comments: Vec::new(),
            }
        }

        /// 文本框内容流（`w:txbxContent`）复用段落管线建块（`MOD-11` 的 `content`）。
        ///
        /// 框里是**独立内容流**：它的段落有自己的 run、自己的图，和宿主段落的坐标流无关。
        fn fill_box_content<'b>(
            &mut self,
            boxes: impl Iterator<Item = (Option<NodeId>, &'b mut Vec<Block>)>,
        ) {
            for (txbx, content) in boxes {
                let Some(txbx) = txbx else { continue };
                *content = self.box_blocks(txbx);
            }
        }

        /// 一段 `w:txbxContent` 的块，记忆化（见 [`Builder::box_content`]）。
        fn box_blocks(&mut self, txbx: NodeId) -> Vec<Block> {
            if let Some(hit) = self.box_content.get(&txbx) {
                return hit.clone();
            }
            if self.box_depth >= crate::model::MAX_BOX_NESTING {
                self.warn(txbx, DiagCode::ModTooDeep, "框套得过深");
                return vec![Block::Protected(ProtectedBlock {
                    node: txbx,
                    kind: ProtectedKind::TooDeep,
                    preview: String::new(),
                    display: None,
                    siblings: Vec::new(),
                    sdt: None,
                    revisions: Vec::new(),
                })];
            }
            let mut blocks = Vec::new();
            self.box_depth += 1;
            self.build_container(txbx, None, &[], &mut blocks);
            self.box_depth -= 1;
            self.box_content.insert(txbx, blocks.clone());
            blocks
        }

        /// DrawingML 形状的内容流：本 part 的 `w:txbxContent`，或**外部文本框 part**
        /// （`wps:txbx/@r:txbx` → `word/txbx1.xml`，任务 5.4d）。
        ///
        /// 外部 part 的块用**那个 part 的** DOM / rels / 索引建（`NodeId` 因此属于它，投影靠
        /// `content_part` 换 DOM）。这种框是只读的：内容不在本 part 里，重写本 part 的段落列表
        /// 救不了它（TS 同样把它排除在保存序号之外并标 `readOnly`）。
        fn fill_shape_content(&mut self, shapes: &mut [crate::model::ShapeDisplay]) {
            for s in shapes {
                if let Some(txbx) = s.txbx {
                    s.content = self.box_blocks(txbx);
                    continue;
                }
                let Some(rid) = s.txbx_rel.as_deref() else { continue };
                let Some(ext) = self.ext_txbx.get(rid) else { continue };
                s.content = ext.idx.blocks_of(
                    ext.dom,
                    ext.rels,
                    self.styles,
                    ext.dom.root(),
                    &mut self.warnings,
                );
                s.content_part = Some(ext.part);
            }
        }

        /// run 的一个子节点：追加坐标流文本，返回段种类（`MOD-06` 贡献表）。
        fn segment(&self, node: NodeId, name: QName, text: &mut String) -> SegmentKind {
            let dom = self.dom;
            if name.ns != NsId::W {
                text.push(OBJECT_REPLACEMENT);
                return SegmentKind::Other(name);
            }
            match name.local {
                LocalName::T | LocalName::DelText => {
                    let mut s = String::new();
                    for c in dom.semantic_children(node) {
                        if let Some(t) = dom.text(c) {
                            s.push_str(&t);
                        }
                    }
                    if xml_space_preserved(dom, node) {
                        text.push_str(&s);
                    } else {
                        text.push_str(s.trim_matches([' ', '\t', '\r', '\n']));
                    }
                    if name.local == LocalName::T {
                        SegmentKind::Text
                    } else {
                        SegmentKind::DelText
                    }
                }
                LocalName::Tab => {
                    text.push('\t');
                    SegmentKind::Tab
                }
                LocalName::Ptab => {
                    text.push('\t');
                    SegmentKind::PTab { align: self.attr(node, NsId::W, LocalName::Alignment) }
                }
                LocalName::Br => {
                    let kind =
                        BreakKind::parse(self.attr(node, NsId::W, LocalName::Type).as_deref());
                    text.push(match kind {
                        BreakKind::TextWrapping => '\n',
                        BreakKind::Page | BreakKind::Column => OBJECT_REPLACEMENT,
                    });
                    SegmentKind::Br { kind, clear: self.attr(node, NsId::W, LocalName::Clear) }
                }
                LocalName::Cr => {
                    text.push('\n');
                    SegmentKind::Cr
                }
                LocalName::NoBreakHyphen => {
                    text.push('\u{2011}');
                    SegmentKind::NoBreakHyphen
                }
                LocalName::SoftHyphen => {
                    text.push('\u{00AD}');
                    SegmentKind::SoftHyphen
                }
                LocalName::Sym => {
                    let font = self.attr(node, NsId::W, LocalName::Font);
                    let code = self
                        .attr(node, NsId::W, LocalName::Char)
                        .and_then(|h| u32::from_str_radix(h.trim(), 16).ok());
                    // 符号字体映射表在 RES-05（M2）；这里按规范退路：U+F000 + (code & 0xFF)
                    let ch = code
                        .and_then(|c| char::from_u32(0xF000 + (c & 0xFF)))
                        .unwrap_or(OBJECT_REPLACEMENT);
                    text.push(ch);
                    SegmentKind::Sym { font, code }
                }
                LocalName::Drawing => {
                    // 墨迹对坐标流不可见：长度 0 的段（TS 在 detect 前把整个 run 剥掉，任务 6.8）
                    if crate::model::is_ink_drawing(dom, node) {
                        return SegmentKind::Ink;
                    }
                    text.push(OBJECT_REPLACEMENT);
                    let anchored = dom
                        .semantic_children(node)
                        .any(|c| dom.is(c, QName::new(NsId::Wp, LocalName::Anchor)));
                    SegmentKind::Drawing { anchored }
                }
                LocalName::Pict => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::Pict
                }
                LocalName::Object => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::Object
                }
                LocalName::Ruby => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::Ruby {
                        rt: crate::model::ruby_part_text(dom, node, LocalName::Rt),
                        base: crate::model::ruby_part_text(dom, node, LocalName::RubyBase),
                    }
                }
                LocalName::FootnoteReference => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::FootnoteRef { id: self.attr(node, NsId::W, LocalName::Id) }
                }
                LocalName::EndnoteReference => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::EndnoteRef { id: self.attr(node, NsId::W, LocalName::Id) }
                }
                LocalName::FootnoteRef => SegmentKind::FootnoteRefMark,
                LocalName::EndnoteRef => SegmentKind::EndnoteRefMark,
                LocalName::Separator => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::Separator
                }
                LocalName::ContinuationSeparator => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::ContinuationSeparator
                }
                LocalName::CommentReference => SegmentKind::CommentRef,
                LocalName::LastRenderedPageBreak => SegmentKind::LastRenderedPageBreak,
                LocalName::FldChar => SegmentKind::FldChar,
                LocalName::InstrText => SegmentKind::InstrText,
                LocalName::DelInstrText => SegmentKind::DelInstrText,
                LocalName::AnnotationRef => SegmentKind::AnnotationRef,
                _ => {
                    text.push(OBJECT_REPLACEMENT);
                    SegmentKind::Other(name)
                }
            }
        }
    }

    /// `xml:space` 的有效值（XML 规范：沿祖先继承，最近的声明生效）；没有声明 → Word 行为，trim。
    /// 段落唯一那个图形的显示模型（`MOD-11`）。同时有多种时按 drawing → pict → object 取第一个：
    /// 只有 R15 / R17 / R18 这些「段里就一个图形」的分类会用到它。
    fn graphic_display(dom: &Dom, facts: &ParagraphFacts) -> Option<Display> {
        if let Some(d) = facts.drawings.first() {
            let Some(fallback) = d.fallback_picture else {
                return Some(Display::Drawing(Box::new(drawing_display(dom, d.node))));
            };
            // chartex 的回退图（R12 → Image）：图取 `mc:Fallback`，尺寸取 `mc:Choice`——Word 排版占的是
            // 图表的位置，回退图只是替身（真实 Word 文件里两者的 extent 相同；TS 同样读第一个 `wp:extent`）。
            let mut display = drawing_display(dom, fallback);
            if let Some(ext) = drawing_display(dom, d.node).extent {
                display.extent = Some(ext);
            }
            return Some(Display::Drawing(Box::new(display)));
        }
        let vml = facts.picts.first().map(|p| p.node).or_else(|| facts.objects.first().copied())?;
        Some(Display::Vml(Box::new(vml_display(dom, vml))))
    }

    /// 段落里第一个之外的顶层绘图（`ProtectedBlock::siblings`）：SmartArt / 画布旁边的照片与形状。
    fn graphic_siblings(dom: &Dom, facts: &ParagraphFacts) -> Vec<Display> {
        facts
            .drawings
            .iter()
            .skip(1)
            .map(|d| Display::Drawing(Box::new(drawing_display(dom, d.node))))
            .collect()
    }

    fn xml_space_preserved(dom: &Dom, node: NodeId) -> bool {
        let space = QName::new(NsId::Xml, LocalName::Space);
        let mut cur = Some(node);
        while let Some(n) = cur {
            if let Some(v) = dom.attr_value(n, space) {
                return v.trim() == "preserve";
            }
            cur = dom.parent(n);
        }
        false
    }

    /// `pPr/pPrChange/pPr` 的旧值快照。
    fn read_para_props_change_of(
        dom: &Dom,
        ppr: Option<NodeId>,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<(NodeId, ParaProps)> {
        crate::semantic::props::read_para_props_change(dom, ppr, diags)
    }

    impl ListRef {
        pub fn new(num_id: i32, ilvl: i32) -> Self {
            Self { num_id, ilvl, from_style: false }
        }
    }

    /// 块**外面**那层修订（`w:ins` / `w:del` / `moveFrom` / `moveTo` 包着整块）。
    ///
    /// 增量刷新（`MOD-13`）只把这些带回给 `build_*`：段落标记、`pPrChange`、`tblPrChange`、
    /// `tblGridChange` 都由构建器自己从 DOM 再读一遍，带回去就成了两份——`TEST-07` 的随机序列
    /// 在「同一个表格连改两次属性」上抓到过。
    fn wrapper_revisions(blk: &Block) -> Vec<Revision> {
        blk.revisions()
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    Revision::Insert(_)
                        | Revision::Delete(_)
                        | Revision::MoveFrom(_)
                        | Revision::MoveTo(_)
                )
            })
            .cloned()
            .collect()
    }

    /// 范围索引的编号身份：`(SpanId, 类别, 配对 id)` 的序列。两次构建这个序列相同，就说明
    /// 已有范围的 `SpanId` 一个都没变，未刷新的块里存着的号还指着同一个范围。
    fn span_identities(idx: &SpanIndex) -> Vec<(u32, crate::span::RangeClass, String)> {
        idx.live().map(|s| (s.id.0, s.class(), s.pair_id().to_string())).collect()
    }

    /// 同理的 `FieldId`（`FLD-02` 也是按文档序编号）：`(FieldId, begin 节点)` 的序列。
    fn field_identities(idx: &FieldIndex) -> Vec<(u32, NodeId)> {
        idx.fields().iter().map(|f| (f.id.0, f.form.head())).collect()
    }
}

// 分类规则表（`MOD-05`）与 `TextKind` 判定（`MOD-03`）：都是 facts 的纯函数，按优先级首条命中。
//
// 与 TS 的 `buildBlock` 决策树不同处标 △（见 spec）。M1 只需 R01/R02(占位)/R07/R08/R10/R19，
// 其余规则已按 facts 写出，但 M1 的 facts 里字段事实为空，R09 不会命中。

use crate::span::{is_property_element, is_range_marker};

/// body（或 sdtContent / 修订包裹）直接子节点的分类（R01–R07）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyClass {
    /// R01
    SectionProps,
    /// R02
    Table,
    /// R03：递归分类 `sdtContent` 的子节点
    Sdt,
    /// R04：不产生 Block
    RangeMarker,
    /// R05
    BodyBreak {
        page: bool,
    },
    /// R06：递归并附 `Revision`
    InsertWrap,
    DeleteWrap,
    MoveFromWrap,
    MoveToWrap,
    /// `w:customXml` / `w:smartTag` 块级包裹：透明递归（spec 未列；见 docs/04 §8）
    Transparent,
    /// 其他非 `w:p`（R07）
    Unknown(QName),
    /// `w:p`：继续按 facts 分类
    Paragraph,
}

pub fn classify_body_child(dom: &Dom, node: NodeId) -> (&'static str, BodyClass) {
    // 文本节点（缩进空白）不产生块
    let Some(name) = dom.name(node) else { return ("R04", BodyClass::RangeMarker) };
    if is_range_marker(name) || (name.ns == NsId::W && name.local == LocalName::ProofErr) {
        return ("R04", BodyClass::RangeMarker);
    }
    if name.ns != NsId::W {
        return ("R07", BodyClass::Unknown(name));
    }
    match name.local {
        LocalName::SectPr => ("R01", BodyClass::SectionProps),
        LocalName::Tbl => ("R02", BodyClass::Table),
        LocalName::Sdt => ("R03", BodyClass::Sdt),
        LocalName::Br => {
            let page =
                dom.attr_value(node, QName::w(LocalName::Type)).is_some_and(|t| t.trim() == "page");
            ("R05", BodyClass::BodyBreak { page })
        }
        LocalName::Ins => ("R06", BodyClass::InsertWrap),
        LocalName::Del => ("R06", BodyClass::DeleteWrap),
        LocalName::MoveFrom => ("R06", BodyClass::MoveFromWrap),
        LocalName::MoveTo => ("R06", BodyClass::MoveToWrap),
        LocalName::CustomXml | LocalName::SmartTag => ("R06", BodyClass::Transparent),
        LocalName::P => ("R19", BodyClass::Paragraph),
        _ if is_property_element(name) => ("R07", BodyClass::Unknown(name)),
        _ => ("R07", BodyClass::Unknown(name)),
    }
}

/// `w:p` 的分类结果（R08–R19）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParaClass {
    Protected(ProtectedKind),
    Image,
    Text,
}

/// 一条段落规则：命中返回结果。
pub type ParaRule = fn(&ParagraphFacts) -> Option<ParaClass>;

/// 按优先级排列的段落规则表；`classify_paragraph` 顺序求值，首条命中即结束。
pub const PARA_RULES: &[(&str, ParaRule)] = &[
    ("R08", r08_style_vanish),
    ("R09", r09_field_block_result),
    ("R10", r10_section_break),
    ("R11", r11_equation),
    ("R12", r12_chart),
    ("R13", r13_smart_art),
    ("R14", r14_locked_canvas),
    ("R15", r15_image),
    ("R16", r16_invisible_shapes),
    ("R17", r17_rule),
    ("R18", r18_ole),
    ("R19", r19_text),
];

pub fn classify_paragraph(f: &ParagraphFacts) -> (&'static str, ParaClass) {
    for (id, rule) in PARA_RULES {
        if let Some(c) = rule(f) {
            return (id, c);
        }
    }
    ("R19", ParaClass::Text)
}

pub fn r08_style_vanish(f: &ParagraphFacts) -> Option<ParaClass> {
    f.style_vanish.then_some(ParaClass::Protected(ProtectedKind::Invisible))
}

pub fn r09_field_block_result(f: &ParagraphFacts) -> Option<ParaClass> {
    f.inside_field_result.map(|id| ParaClass::Protected(ProtectedKind::FieldBlockResult(id)))
}

pub fn r10_section_break(f: &ParagraphFacts) -> Option<ParaClass> {
    (f.has_sect_pr && !f.visible_text).then_some(ParaClass::Protected(ProtectedKind::SectionBreak))
}

pub fn r11_equation(f: &ParagraphFacts) -> Option<ParaClass> {
    (f.math.omath_para || (f.math.count > 0 && !f.visible_text))
        .then_some(ParaClass::Protected(ProtectedKind::Equation))
}

pub fn r12_chart(f: &ParagraphFacts) -> Option<ParaClass> {
    let mut chart = false;
    for d in &f.drawings {
        match d.kind {
            // chartex（旭日图 / 瀑布图 …）配了预渲染的回退图：Word 之外的渲染器画的就是这张图，
            // 数据模型的降级读法只留给没有回退图的 part。`graphic_display` 取回退图的显示模型。
            DrawingKind::ChartEx if d.fallback_picture.is_some() => {
                return Some(ParaClass::Image);
            }
            DrawingKind::Chart | DrawingKind::ChartEx => chart = true,
            _ => {}
        }
    }
    chart.then_some(ParaClass::Protected(ProtectedKind::Chart))
}

pub fn r13_smart_art(f: &ParagraphFacts) -> Option<ParaClass> {
    f.drawings
        .iter()
        .any(|d| d.kind == DrawingKind::Diagram)
        .then_some(ParaClass::Protected(ProtectedKind::SmartArt))
}

pub fn r14_locked_canvas(f: &ParagraphFacts) -> Option<ParaClass> {
    f.drawings
        .iter()
        .any(|d| d.kind == DrawingKind::LockedCanvas)
        .then_some(ParaClass::Protected(ProtectedKind::SmartArt))
}

pub fn r15_image(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || !f.objects.is_empty() || f.math.count != 0 {
        return None;
    }
    let single_picture =
        f.drawings.len() == 1 && f.picts.is_empty() && f.drawings[0].kind == DrawingKind::Picture;
    let single_imagedata =
        f.picts.len() == 1 && f.drawings.is_empty() && f.picts[0].kind == PictKind::ImageData;
    (single_picture || single_imagedata).then_some(ParaClass::Image)
}

pub fn r16_invisible_shapes(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || f.picts.is_empty() || !f.drawings.is_empty() || !f.objects.is_empty() {
        return None;
    }
    f.picts
        .iter()
        .all(|p| matches!(p.kind, PictKind::Hidden | PictKind::ShapeTypeOnly))
        .then_some(ParaClass::Protected(ProtectedKind::Invisible))
}

pub fn r17_rule(f: &ParagraphFacts) -> Option<ParaClass> {
    if f.visible_text || f.picts.is_empty() || !f.drawings.is_empty() || !f.objects.is_empty() {
        return None;
    }
    f.picts
        .iter()
        .all(|p| p.kind == PictKind::Hr)
        .then_some(ParaClass::Protected(ProtectedKind::Rule))
}

pub fn r18_ole(f: &ParagraphFacts) -> Option<ParaClass> {
    (!f.visible_text && !f.objects.is_empty() && f.drawings.is_empty() && f.picts.is_empty())
        .then_some(ParaClass::Protected(ProtectedKind::Ole))
}

pub fn r19_text(_: &ParagraphFacts) -> Option<ParaClass> {
    Some(ParaClass::Text)
}

/// `MOD-03`：ListRef 存在 → ListItem；否则 Heading；否则 Paragraph。
pub fn text_kind(f: &ParagraphFacts) -> TextKind {
    if let Some(list) = &f.numbering_ref {
        return TextKind::ListItem { list: list.clone() };
    }
    if let Some(level) = f.outline_level {
        return TextKind::Heading { level };
    }
    TextKind::Paragraph
}

// 模型层的声明宏。
//
// 显示模型里有一批「无字段枚举 + 一个稳定短名字」的类型：种类、绕排、填充方式……名字用在
// 诊断、语料普查的输出、以后的 i18n key 上。枚举写一遍、名字表再写一遍，迟早对不上——尤其是
// 加变体的时候编译器不会提醒你去补名字表。宏把两者绑在同一处声明里。

/// 声明一个无字段枚举，并生成 `as_str`（名字表跟着变体走，漏了编译不过）。
///
/// ```ignore
/// named_enum! {
///     /// VML 元素种类。
///     pub enum VmlKind {
///         /// `v:shape`
///         Shape = "shape",
///         Group = "group",
///     }
/// }
/// ```
macro_rules! named_enum {
        (
            $(#[$meta:meta])*
            $vis:vis enum $name:ident {
                $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)?
            }
        ) => {
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            $vis enum $name {
                $($(#[$vmeta])* $variant,)+
            }

            impl $name {
                /// 稳定的短名字。
                pub const fn as_str(self) -> &'static str {
                    match self {
                        $(Self::$variant => $text,)+
                    }
                }
            }

            impl ::core::fmt::Display for $name {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    f.write_str(self.as_str())
                }
            }
        };
    }

pub(crate) use named_enum;

// 辅助 part 的内容流（`MOD-01`、`docs/03` §6.7，`spec/16` 任务 5.3）。
//
// 页眉页脚、脚注尾注、批注的内容都是 `Vec<Block>`，**与正文同一个构建器**。它们与正文的差别只有
// 两点：块长在别的 part 的 DOM 里，以及每个 part 有自己的三份索引。这里就是那两点的共同部分。
//
// `FlowMap` / `FieldIndex` / `SpanIndex` 按 **part** 建一次（`SPAN-01`：`w:hdr` / `w:ftr` /
// 每个 `w:footnote` / `w:endnote` / `w:comment` 条目各是一个独立内容流，`FlowId` 只在 part 内有
// 意义），块按**容器**建（注释 part 里一个条目一个容器，页眉 part 整个根就是一个容器）。

use crate::diag::Diagnostic;

use crate::model::build::Builder;

use crate::package::{PartId, Rels};
use crate::span::field::FieldIndex;
use crate::span::{FlowMap, SpanIndex};
use crate::xml::Dom;

/// 一个辅助 XML part 的内容流索引。
#[derive(Debug, Clone, PartialEq)]
pub struct AuxFlows {
    pub part: PartId,
    pub flows: FlowMap,
    pub fields: FieldIndex,
    pub spans: SpanIndex,
}

impl AuxFlows {
    /// 建这个 part 的三份索引，诊断进 `warnings`。
    pub fn build(part: PartId, dom: &Dom, warnings: &mut Vec<Diagnostic>) -> AuxFlows {
        let flows = FlowMap::build(dom);
        let mut fields = FieldIndex::build(dom);
        warnings.extend(fields.take_diagnostics());
        let mut spans = SpanIndex::build(dom);
        warnings.extend(spans.take_diagnostics());
        AuxFlows { part, flows, fields, spans }
    }

    /// 用这份索引给一个容器建块（复用正文管线：段落 / 表格 / sdt / 修订包裹 / 文本框）。
    pub fn blocks_of(
        &self,
        dom: &Dom,
        rels: &Rels,
        styles: Option<&Styles>,
        container: NodeId,
        warnings: &mut Vec<Diagnostic>,
    ) -> Vec<Block> {
        let mut b = Builder::new(dom, styles, rels, &self.fields, &self.spans, Vec::new());
        let mut blocks = Vec::new();
        b.build_container(container, None, &[], &mut blocks);
        warnings.append(&mut b.warnings);
        blocks
    }
}

/// 一个外部文本框 part（`wps:txbx/@r:txbx` → `word/txbx1.xml`，根是 `w14:txbx`）。
///
/// 形状的内容不在本 part 里时才用它。`Document::rebuild` 先把这些 part 解析好放进一张
/// `rel id → ExtTxbxPart` 的表，构建器遇到 `txbx_rel` 就从表里取——构建器只持有主 part 的 DOM，
/// 没法自己去解析别的 part。
pub struct ExtTxbxPart<'a> {
    pub part: PartId,
    pub dom: &'a Dom,
    pub rels: &'a Rels,
    pub idx: AuxFlows,
}

/// 外部文本框 part 表（按关系 id）。
pub type ExtTxbxMap<'a> = std::collections::BTreeMap<String, ExtTxbxPart<'a>>;

/// 空表（没有外部文本框 part 的文档，以及 `build_main` 之类的入口）。
pub(crate) fn empty_ext_txbx() -> &'static ExtTxbxMap<'static> {
    static EMPTY: std::sync::LazyLock<ExtTxbxMap<'static>> =
        std::sync::LazyLock::new(ExtTxbxMap::new);
    &EMPTY
}

// 图表 part 的显示模型（`MOD-11`，`spec/17` 任务 6.1）。
//
// 图表 part（`word/charts/chartN.xml`）是**有自己 DOM 的 XML part**（L1），这里的 [`ChartDisplay`] 是它的投影：
// 只读 Word 写在数据引用旁边的缓存（`c:strCache` / `c:numCache`），内嵌工作簿从不打开。`SetChartData`（6.6）改的
// 也是这些缓存的文本节点，所以模型里每个可编辑的值都带着 `NodeId`。
//
// 与 TS `chart.ts` 的 `parseChartPartXml` / `parseChartexPartXml` 行为对齐（`docs/01` §8.5），但按 `MOD-11` 分层：
// 几何留 EMU 原值（宿主 `wp:extent` 在 [`DrawingDisplay`](crate::model::DrawingDisplay) 上）、px 换算在
// `compat_ts`；颜色经 `RES-05`（`resolve/drawingml.rs`）解析为 sRGB 并保留原始定义（[`ChartColor`]）；调色板
// （`c:style` → 主题 accent 的阶梯）是主题的纯函数，这里按文档的配色方案算一次存下来。
//
// chartex（`cx:chartSpace`，旭日 / 树状 / 瀑布 / 箱形 / 漏斗 / 帕累托）按 TS 的降级读：数据维度与系列名进同一个
// [`ChartDisplay`]，`kind` 取最近的经典种类；它的 part 不可编辑（`chartex: true`）。

use crate::resolve::drawingml::{DrawingColor, Rgb, color_in};

named_enum! {
    /// 图表种类（TS `ChartDisplay.kind`）。三维与环形归并到平面同类；认不出的 `*Chart` 元素是 `Other`。
    pub enum ChartKind {
        Bar = "bar",
        Line = "line",
        Pie = "pie",
        Area = "area",
        Scatter = "scatter",
        Bubble = "bubble",
        Other = "other",
    }
}

named_enum! {
    /// `c:grouping`：只对 bar / area 认堆积的两种（`clustered` / `standard` 不记）。
    pub enum ChartGrouping {
        Stacked = "stacked",
        PercentStacked = "percentStacked",
    }
}

named_enum! {
    /// `c:legend/c:legendPos/@val`；有 `c:legend` 而没写位置时 schema 缺省是右侧。
    pub enum LegendPos {
        Bottom = "b",
        Left = "l",
        Right = "r",
        Top = "t",
        TopRight = "tr",
    }
}

/// 图表 part 元素名 → 种类。一张表同时给出「哪些元素算绘图区里的图」（`plot_kind` 有值）与「归并到哪一类」；
/// ECMA-376 §21.2.2 的 16 种图全部在表里，认不出的种类明说是 `Other`，别的元素（`c:catAx` 等）不是图。
macro_rules! chart_kinds {
        ($($local:ident => $kind:ident),+ $(,)?) => {
            /// 绘图区子元素 → 图表种类；不是图表元素 → `None`。
            pub fn plot_kind(local: LocalName) -> Option<ChartKind> {
                match local {
                    $(LocalName::$local => Some(ChartKind::$kind),)+
                    _ => None,
                }
            }

            /// 表里全部图表元素（测试用：每一种都要被认出来）。
            pub const PLOT_ELEMENTS: &[LocalName] = &[$(LocalName::$local,)+];
        };
    }

chart_kinds! {
    BarChart => Bar,
    Bar3DChart => Bar,
    LineChart => Line,
    Line3DChart => Line,
    PieChart => Pie,
    Pie3DChart => Pie,
    DoughnutChart => Pie,
    OfPieChart => Other,
    AreaChart => Area,
    Area3DChart => Area,
    ScatterChart => Scatter,
    BubbleChart => Bubble,
    RadarChart => Other,
    StockChart => Other,
    SurfaceChart => Other,
    Surface3DChart => Other,
}

/// chartex `cx:series/@layoutId` → 最近的经典种类（TS `CHARTEX_KINDS`：形状不同，数据 / 标签 / 标题照样显示）。
pub fn chartex_kind(layout_id: &str) -> Option<ChartKind> {
    Some(match layout_id {
        "clusteredColumn" | "boxWhisker" | "waterfall" | "funnel" => ChartKind::Bar,
        "paretoLine" => ChartKind::Line,
        "sunburst" | "treemap" => ChartKind::Pie,
        _ => return None,
    })
}

/// 一个图表里的颜色：原始定义（`RES-05` 的 [`DrawingColor`]）与按文档配色方案解出的 sRGB。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartColor {
    pub def: DrawingColor,
    /// 解不出（未知槽位、主题里缺）→ `None`，按「没写颜色」处理。
    pub rgb: Option<Rgb>,
}

/// 一个系列（`c:ser` / `cx:series`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub node: NodeId,
    /// `c:tx`：字面 `c:v` 或缓存的第一个点。
    pub name: Option<String>,
    /// `c:val` / `c:yVal` 缓存；非数字或缺点 → `None`（TS 的 `null`，画成空档）。
    pub values: Vec<Option<f64>>,
    /// `c:spPr/a:solidFill`。
    pub color: Option<ChartColor>,
    /// `c:dPt` 逐点填充，按 `c:idx` 稀疏（饼图扇区、高亮的柱）；一处都没有 → `None`。
    pub point_colors: Option<Vec<Option<ChartColor>>>,
    /// scatter / bubble：`c:xVal` 缓存（全空 → `None`）。
    pub x_values: Option<Vec<Option<f64>>>,
    /// bubble：`c:bubbleSize` 缓存。
    pub sizes: Option<Vec<Option<f64>>>,
    /// scatter：`c:scatterStyle` 含 line / smooth 且系列的 `a:ln` 不是 `a:noFill` → 点之间连线。
    pub line: bool,
}

/// 一个图表 part 的显示模型（TS `ChartDisplay`，`MOD-11`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartDisplay {
    /// `c:chartSpace` / `cx:chartSpace`。
    pub root: NodeId,
    pub kind: ChartKind,
    /// 绘图区里第一个图表元素（组合图以它为准）；chartex 没有。
    pub plot: Option<NodeId>,
    /// bar：`c:barDir val="bar"`（水平条形）。
    pub horizontal: bool,
    pub grouping: Option<ChartGrouping>,
    /// line：`c:marker val="1"`；scatter：`c:scatterStyle` 缺省或含 `marker`。
    pub markers: bool,
    /// doughnut：`c:holeSize`（缺省 50）；`0` 与非环形 → `None`。
    pub hole_pct: Option<u32>,
    pub legend_pos: Option<LegendPos>,
    /// 标题文字：`a:t` 拼接 → `c:strCache/c:v` → 自动标题 `Chart Title`（`autoTitleDeleted` 为真则无）；
    /// 单系列的自动标题按 Office 的做法取系列名。没有 `c:title` 元素 → `None`。
    pub title: Option<String>,
    /// `c:title` 节点（6.6 改标题的落点）。
    pub title_node: Option<NodeId>,
    /// 类别文本：第一个带 `c:cat` / `c:xVal` 的系列；日期格式的序列号换成 `m/d/yyyy`，`xVal` 的长小数四舍五入到 4 位。
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
    /// `c:style/@val`（1–48；`c14:style` 101–148 减 100）。
    pub style_val: Option<u8>,
    /// 系列颜色循环：`c:style` 的列决定灰阶 / 六个 accent / 单色阶梯（[`palette`]）。主题缺 accent → `None`。
    pub palette: Option<[Rgb; 6]>,
    /// `cx:chartSpace`：只有降级读法，`SetChartData` 不接受。
    pub chartex: bool,
}

/// 主 part 引用的一个图表 part。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPart {
    pub part: PartId,
    /// part 解析不了（`Opaque`）时 `None`。
    pub root: Option<NodeId>,
    pub chartex: bool,
    /// 没有带缓存值的系列 → `None`（记 `CHART_NO_SERIES`）。
    pub display: Option<ChartDisplay>,
}

impl ChartPart {
    /// 解析一个图表 part。`dom` 为 `None` = part 是二进制或解析失败（`Package` 已记 `PKG_OPAQUE_PART`）。
    pub fn build(
        part: PartId,
        dom: Option<&Dom>,
        scheme: &ColorScheme,
        warnings: &mut Vec<Diagnostic>,
    ) -> ChartPart {
        let Some(dom) = dom else {
            return ChartPart { part, root: None, chartex: false, display: None };
        };
        let root = dom.root();
        let chartex = is(dom, root, NsId::Cx, LocalName::ChartSpace);
        if !chartex && !is(dom, root, NsId::C, LocalName::ChartSpace) {
            warnings.push(Diagnostic::pre_existing(
                part,
                range_of(dom, root),
                DiagCode::ModUnparseable,
                "图表 part 的根不是 c:chartSpace / cx:chartSpace",
            ));
            return ChartPart { part, root: Some(root), chartex: false, display: None };
        }
        let display =
            if chartex { chartex_display(dom, root) } else { chart_display(dom, root, scheme) };
        if display.is_none() {
            warnings.push(Diagnostic::pre_existing(
                part,
                range_of(dom, root),
                DiagCode::ChartNoSeries,
                "图表 part 里没有带缓存值的系列",
            ));
        }
        ChartPart { part, root: Some(root), chartex, display }
    }
}

// ---- 经典图表（c:）--------------------------------------------------------------------------------

fn chart_display(dom: &Dom, space: NodeId, scheme: &ColorScheme) -> Option<ChartDisplay> {
    let chart = chart_child(dom, space, NsId::C, LocalName::Chart)?;
    let plot_area = chart_child(dom, chart, NsId::C, LocalName::PlotArea)?;
    // 组合图：第一个画出来的图表元素决定种类（主系列）
    let (plot, kind) = dom.semantic_children(plot_area).find_map(|c| {
        let name = dom.name(c)?;
        (dom.is_ns(c, NsId::C, "c")).then_some(())?;
        plot_kind(name.local).map(|k| (c, k))
    })?;
    let val_of = |n: NodeId| chart_attr(dom, n, LocalName::Val);
    let plot_child_val = |l: LocalName| chart_child(dom, plot, NsId::C, l).and_then(val_of);

    let horizontal =
        kind == ChartKind::Bar && plot_child_val(LocalName::BarDir).as_deref() == Some("bar");
    let grouping = match (kind, plot_child_val(LocalName::Grouping).as_deref()) {
        (ChartKind::Bar | ChartKind::Area, Some("stacked")) => Some(ChartGrouping::Stacked),
        (ChartKind::Bar | ChartKind::Area, Some("percentStacked")) => {
            Some(ChartGrouping::PercentStacked)
        }
        _ => None,
    };
    let scatter_style = plot_child_val(LocalName::ScatterStyle);
    let markers = match kind {
        ChartKind::Line => plot_child_val(LocalName::Marker).as_deref() == Some("1"),
        ChartKind::Scatter => {
            scatter_style.as_ref().is_none_or(|s| s.to_lowercase().contains("marker"))
        }
        _ => false,
    };
    let scatter_lines = kind == ChartKind::Scatter
        && scatter_style.as_ref().is_some_and(|s| {
            let s = s.to_lowercase();
            s.contains("line") || s.contains("smooth")
        });
    let hole_pct = dom
        .name(plot)
        .is_some_and(|n| n.local == LocalName::DoughnutChart)
        .then(|| {
            plot_child_val(LocalName::HoleSize).map_or(50, |v| v.trim().parse::<u32>().unwrap_or(0))
        })
        .filter(|&h| h > 0);
    let legend_pos = chart_child(dom, chart, NsId::C, LocalName::Legend).map(|legend| {
        chart_child(dom, legend, NsId::C, LocalName::LegendPos)
            .and_then(val_of)
            .and_then(|v| legend_pos_of(v.trim()))
            .unwrap_or(LegendPos::Right)
    });

    let mut categories: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for ser in children(dom, plot, NsId::C, LocalName::Ser) {
        // scatter / bubble 用 x / y 对，不是类别 / 值
        let Some(val) = chart_child(dom, ser, NsId::C, LocalName::Val)
            .or_else(|| chart_child(dom, ser, NsId::C, LocalName::YVal))
        else {
            continue;
        };
        let values = cache_numbers(dom, val);
        if values.is_empty() {
            continue;
        }
        let cat = chart_child(dom, ser, NsId::C, LocalName::Cat)
            .or_else(|| chart_child(dom, ser, NsId::C, LocalName::XVal));
        if let Some(cat) = cat
            && categories.is_empty()
        {
            categories =
                cache_points(dom, cat).into_iter().map(Option::unwrap_or_default).collect();
            let fmt = cat_format_code(dom, cat);
            if fmt.as_deref().is_some_and(|f| f.contains(['y', 'd', 'Y', 'D'])) {
                // 日期格式的类别缓存是 Excel 序列号，按日期文字显示
                categories =
                    categories.into_iter().map(|v| serial_date_text(&v).unwrap_or(v)).collect();
            } else if dom.name(cat).is_some_and(|n| n.local == LocalName::XVal) {
                // x 缓存是原始双精度文本（0.70000000000000062），显示前修到 4 位
                categories = categories
                    .into_iter()
                    .map(|v| match v.trim().parse::<f64>() {
                        Ok(n) if !v.is_empty() && n.is_finite() => {
                            format!("{}", (n * 10000.0).round() / 10000.0)
                        }
                        _ => v,
                    })
                    .collect();
            }
        }
        let color = chart_child(dom, ser, NsId::C, LocalName::SpPr)
            .and_then(|sp| solid_fill(dom, sp, scheme));
        let point_colors = data_point_colors(dom, ser, scheme);
        let (mut x_values, mut sizes, mut line) = (None, None, false);
        if matches!(kind, ChartKind::Scatter | ChartKind::Bubble) {
            x_values = chart_child(dom, ser, NsId::C, LocalName::XVal)
                .map(|n| cache_numbers(dom, n))
                .filter(|xs| xs.iter().any(Option::is_some));
            sizes = chart_child(dom, ser, NsId::C, LocalName::BubbleSize)
                .map(|n| cache_numbers(dom, n))
                .filter(|xs| xs.iter().any(Option::is_some));
            line = scatter_lines && !series_line_hidden(dom, ser);
        }
        series.push(ChartSeries {
            node: ser,
            name: series_name(dom, ser),
            values,
            color,
            point_colors,
            x_values,
            sizes,
            line,
        });
    }
    if series.is_empty() {
        return None;
    }

    let style_val = style_val(dom, space);
    let title_node = chart_child(dom, chart, NsId::C, LocalName::Title);
    let mut title = title_node.and_then(|t| chart_title(dom, chart, t));
    // Office 给单系列图的自动标题取系列名
    if title.as_deref() == Some("Chart Title")
        && series.len() == 1
        && let Some(name) = &series[0].name
    {
        title = Some(name.clone());
    }
    Some(ChartDisplay {
        root: space,
        kind,
        plot: Some(plot),
        horizontal,
        grouping,
        markers,
        hole_pct,
        legend_pos,
        title,
        title_node,
        categories,
        series,
        style_val,
        palette: palette(style_val, scheme),
        chartex: false,
    })
}

fn legend_pos_of(v: &str) -> Option<LegendPos> {
    Some(match v {
        "b" => LegendPos::Bottom,
        "l" => LegendPos::Left,
        "r" => LegendPos::Right,
        "t" => LegendPos::Top,
        "tr" => LegendPos::TopRight,
        _ => return None,
    })
}

/// `c:title` 的文字：`a:t` 拼接 → `c:v` 拼接（`strRef` 标题）→ 自动标题占位（除非 `c:autoTitleDeleted` 为真）。
fn chart_title(dom: &Dom, chart: NodeId, title: NodeId) -> Option<String> {
    let joined = |ns: NsId, local: LocalName| -> String {
        dom.semantic_descendants(title)
            .filter(|&n| is(dom, n, ns, local))
            .map(|n| chart_text_of(dom, n))
            .collect::<String>()
    };
    let rich = joined(NsId::A, LocalName::T);
    if !rich.is_empty() {
        return Some(rich);
    }
    let cached = joined(NsId::C, LocalName::V);
    if !cached.is_empty() {
        return Some(cached);
    }
    // 没有文字的 `c:title` = 自动标题，Word 显示 "Chart Title" 占位；`CT_Boolean`：无 val 与 val="true" 都是真
    let deleted = chart_child(dom, chart, NsId::C, LocalName::AutoTitleDeleted).is_some_and(|d| {
        chart_attr(dom, d, LocalName::Val).is_none_or(|v| matches!(v.trim(), "1" | "true"))
    });
    (!deleted).then(|| "Chart Title".to_string())
}

/// `c:style/@val`（1–48），或 Word 2010 的 `mc:AlternateContent` 包装（`c14:style` 101–148，减 100）。
fn style_val(dom: &Dom, space: NodeId) -> Option<u8> {
    // 语义遍历已经选好 MCE 分支：选中 `c14` 时看到的是 `c14:style`，退到 Fallback 时是 `c:style`
    let node = dom.semantic_children(space).find(|&c| {
        dom.name(c).is_some_and(|n| n.local == LocalName::Style)
            && (dom.is_ns(c, NsId::C, "c") || dom.is_ns(c, NsId::C14, "c14"))
    })?;
    let mut v: i64 = chart_attr(dom, node, LocalName::Val)?.trim().parse().ok()?;
    if v > 100 {
        v -= 100;
    }
    (1..=48).contains(&v).then_some(v as u8)
}

/// Word 灰阶图表样式（样式列 1）的系列色，近似值（TS `GRAYSCALE_PALETTE`）。
pub const GRAYSCALE_PALETTE: [[u8; 3]; 6] = [
    [0x59, 0x59, 0x59],
    [0xD9, 0xD9, 0xD9],
    [0xA6, 0xA6, 0xA6],
    [0x40, 0x40, 0x40],
    [0xBF, 0xBF, 0xBF],
    [0x8C, 0x8C, 0x8C],
];

/// 图表样式的系列颜色循环：`c:style` 1–48 排成 8 列的样式库，列 1 灰阶、列 2 六个 accent 轮转、
/// 列 3–8 单色（一个 accent 的 tint / shade 阶梯）。没有 `c:style` → 列 2。主题缺任一 accent → `None`。
pub fn palette(style_val: Option<u8>, scheme: &ColorScheme) -> Option<[Rgb; 6]> {
    let pos = style_val.map_or(2, |v| (u32::from(v) - 1) % 8 + 1);
    if pos == 1 {
        return Some(GRAYSCALE_PALETTE.map(|c| c.map(f64::from)));
    }
    let accents = [
        ThemeSlot::Accent1,
        ThemeSlot::Accent2,
        ThemeSlot::Accent3,
        ThemeSlot::Accent4,
        ThemeSlot::Accent5,
        ThemeSlot::Accent6,
    ];
    let mut list = [[0.0; 3]; 6];
    for (out, slot) in list.iter_mut().zip(accents) {
        *out = scheme.get(slot)?.map(f64::from);
    }
    if pos == 2 {
        return Some(list);
    }
    let base = list[(pos - 3) as usize];
    let tint = |t: f64| base.map(|c| c * t + 255.0 * (1.0 - t));
    let shade = |s: f64| base.map(|c| c * s);
    Some([base, tint(0.6), shade(0.75), tint(0.3), shade(0.5), tint(0.8)])
}

/// `spPr/a:solidFill` 的颜色（`RES-05`）。
fn solid_fill(dom: &Dom, sp_pr: NodeId, scheme: &ColorScheme) -> Option<ChartColor> {
    let fill = chart_child(dom, sp_pr, NsId::A, LocalName::SolidFill)?;
    let def = color_in(dom, fill)?;
    let rgb = def.to_rgb(scheme);
    Some(ChartColor { def, rgb })
}

/// `c:dPt` 逐点填充，按 `c:idx` 稀疏；一处都没有 → `None`。
fn data_point_colors(
    dom: &Dom,
    ser: NodeId,
    scheme: &ColorScheme,
) -> Option<Vec<Option<ChartColor>>> {
    let mut out: Vec<Option<ChartColor>> = Vec::new();
    for d_pt in children(dom, ser, NsId::C, LocalName::DPt) {
        let Some(idx) = chart_child(dom, d_pt, NsId::C, LocalName::Idx)
            .and_then(|i| chart_attr(dom, i, LocalName::Val))
            .and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let Some(color) = chart_child(dom, d_pt, NsId::C, LocalName::SpPr)
            .and_then(|sp| solid_fill(dom, sp, scheme))
        else {
            continue;
        };
        if out.len() <= idx {
            out.resize(idx + 1, None);
        }
        out[idx] = Some(color);
    }
    (!out.is_empty()).then_some(out)
}

/// 系列的 `a:ln` 明写 `a:noFill`（散点只画标记）。
fn series_line_hidden(dom: &Dom, ser: NodeId) -> bool {
    chart_child(dom, ser, NsId::C, LocalName::SpPr)
        .and_then(|sp| chart_child(dom, sp, NsId::A, LocalName::Ln))
        .is_some_and(|ln| chart_child(dom, ln, NsId::A, LocalName::NoFill).is_some())
}

/// `c:tx`：字面 `c:v`，否则缓存的第一个点。
fn series_name(dom: &Dom, ser: NodeId) -> Option<String> {
    let tx = chart_child(dom, ser, NsId::C, LocalName::Tx)?;
    if let Some(v) = chart_child(dom, tx, NsId::C, LocalName::V) {
        return Some(chart_text_of(dom, v));
    }
    cache_points(dom, tx).into_iter().next().flatten()
}

/// `c:cat` / `c:val` / `c:tx` 容器的缓存点文本，按 `idx` 排、`ptCount` 补空。
/// `strRef` / `numRef` 里是 `strCache` / `numCache`，字面量 `strLit` / `numLit` 自己就是缓存。
fn cache_points(dom: &Dom, container: NodeId) -> Vec<Option<String>> {
    let Some(cache) = cache_node(dom, container) else { return Vec::new() };
    let count = chart_child(dom, cache, NsId::C, LocalName::PtCount)
        .and_then(|n| chart_attr(dom, n, LocalName::Val))
        .and_then(|v| v.trim().parse::<usize>().ok());
    let mut points: Vec<Option<String>> = Vec::new();
    for pt in children(dom, cache, NsId::C, LocalName::Pt) {
        let Some(idx) =
            chart_attr(dom, pt, LocalName::Idx).and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        if points.len() <= idx {
            points.resize(idx + 1, None);
        }
        points[idx] = Some(
            chart_child(dom, pt, NsId::C, LocalName::V)
                .map(|v| chart_text_of(dom, v))
                .unwrap_or_default(),
        );
    }
    if let Some(n) = count
        && n > points.len()
    {
        points.resize(n, None);
    }
    points
}

fn cache_node(dom: &Dom, container: NodeId) -> Option<NodeId> {
    let reference = chart_child(dom, container, NsId::C, LocalName::StrRef)
        .or_else(|| chart_child(dom, container, NsId::C, LocalName::NumRef));
    match reference {
        Some(r) => chart_child(dom, r, NsId::C, LocalName::StrCache)
            .or_else(|| chart_child(dom, r, NsId::C, LocalName::NumCache)),
        None => chart_child(dom, container, NsId::C, LocalName::StrLit)
            .or_else(|| chart_child(dom, container, NsId::C, LocalName::NumLit)),
    }
}

/// 数值缓存：空白 → `None`，非有限数 → `None`。
fn cache_numbers(dom: &Dom, container: NodeId) -> Vec<Option<f64>> {
    cache_points(dom, container).into_iter().map(|v| v.and_then(|s| parse_number(&s))).collect()
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// 类别缓存的 `c:formatCode`（`numRef/numCache` 或 `numLit`）。
fn cat_format_code(dom: &Dom, container: NodeId) -> Option<String> {
    let cache = match chart_child(dom, container, NsId::C, LocalName::NumRef) {
        Some(r) => chart_child(dom, r, NsId::C, LocalName::NumCache)?,
        None => chart_child(dom, container, NsId::C, LocalName::NumLit)?,
    };
    chart_child(dom, cache, NsId::C, LocalName::FormatCode).map(|n| chart_text_of(dom, n))
}

/// Excel 日期序列号 → `m/d/yyyy`（Word / LibreOffice 画类别轴时显示的是日期不是序列号）。
/// 序列号 0 = 1899-12-30；超出 (0, 80000] 或不是数 → `None`。
pub fn serial_date_text(v: &str) -> Option<String> {
    let n = v.trim().parse::<f64>().ok()?;
    if !n.is_finite() || n <= 0.0 || n > 80000.0 {
        return None;
    }
    // 1899-12-30 距 1970-01-01 是 -25569 天
    let days = n.round() as i64 - 25569;
    let (y, m, d) = civil_from_days(days);
    Some(format!("{m}/{d}/{y}"))
}

/// 自 1970-01-01 起的天数 → 公历 (年, 月, 日)。Howard Hinnant 的算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---- chartex（cx:）--------------------------------------------------------------------------------

/// `cx:chartSpace` 的降级读法：`cx:chartData/cx:data` 的 `strDim` / `numDim` 各一级点缓存，
/// `cx:series` 按 `layoutId` 归并到经典种类。找不到 `cx:chartData` 时接受任何带 `cx:data` 的子元素
/// （消费者必须跳过不认识的元素；测试语料故意改过这个名字）。
fn chartex_display(dom: &Dom, space: NodeId) -> Option<ChartDisplay> {
    let chart_data = chart_child(dom, space, NsId::Cx, LocalName::ChartData).or_else(|| {
        dom.semantic_children(space)
            .find(|&c| chart_child(dom, c, NsId::Cx, LocalName::Data).is_some())
    });
    // data id → (类别, 值)
    let mut data_by_id: Vec<(String, Vec<String>, Vec<Option<f64>>)> = Vec::new();
    for data in chart_data.into_iter().flat_map(|cd| children(dom, cd, NsId::Cx, LocalName::Data)) {
        let id = chart_attr(dom, data, LocalName::Id).unwrap_or_default();
        let (mut cats, mut vals) = (Vec::new(), Vec::new());
        for dim in dom.semantic_children(data) {
            if is(dom, dim, NsId::Cx, LocalName::StrDim) {
                cats =
                    chartex_points(dom, dim).into_iter().map(Option::unwrap_or_default).collect();
            } else if is(dom, dim, NsId::Cx, LocalName::NumDim) {
                vals = chartex_points(dom, dim)
                    .into_iter()
                    .map(|v| v.and_then(|s| parse_number(&s)))
                    .collect();
            }
        }
        data_by_id.push((id, cats, vals));
    }
    let chart = chart_child(dom, space, NsId::Cx, LocalName::Chart);
    let region = chart
        .and_then(|c| chart_child(dom, c, NsId::Cx, LocalName::PlotArea))
        .and_then(|p| chart_child(dom, p, NsId::Cx, LocalName::PlotAreaRegion));
    let mut kind = ChartKind::Other;
    let mut categories: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for ser in region.into_iter().flat_map(|r| children(dom, r, NsId::Cx, LocalName::Series)) {
        if kind == ChartKind::Other
            && let Some(k) =
                chart_attr(dom, ser, LocalName::LayoutId).and_then(|l| chartex_kind(l.trim()))
        {
            kind = k;
        }
        let data_id = chart_child(dom, ser, NsId::Cx, LocalName::DataId)
            .and_then(|d| chart_attr(dom, d, LocalName::Val));
        let Some((_, cats, vals)) =
            data_by_id.iter().find(|(id, ..)| Some(id.as_str()) == data_id.as_deref())
        else {
            continue;
        };
        if vals.is_empty() {
            continue;
        }
        if categories.is_empty() {
            categories = cats.clone();
        }
        let name = chart_child(dom, ser, NsId::Cx, LocalName::Tx)
            .and_then(|t| chart_child(dom, t, NsId::Cx, LocalName::TxData))
            .and_then(|t| chart_child(dom, t, NsId::Cx, LocalName::V))
            .map(|v| chart_text_of(dom, v))
            .filter(|s| !s.is_empty());
        series.push(ChartSeries {
            node: ser,
            name,
            values: vals.clone(),
            color: None,
            point_colors: None,
            x_values: None,
            sizes: None,
            line: false,
        });
    }
    if series.is_empty() {
        return None;
    }
    // `cx:title` 与经典图表一样是 `a:t` 富文本
    let title_node = chart.and_then(|c| chart_child(dom, c, NsId::Cx, LocalName::Title));
    let title = title_node
        .map(|t| {
            dom.semantic_descendants(t)
                .filter(|&n| is(dom, n, NsId::A, LocalName::T))
                .map(|n| chart_text_of(dom, n))
                .collect::<String>()
        })
        .filter(|s| !s.is_empty());
    Some(ChartDisplay {
        root: space,
        kind,
        plot: None,
        horizontal: false,
        grouping: None,
        markers: false,
        hole_pct: None,
        legend_pos: None,
        title,
        title_node,
        categories,
        series,
        style_val: None,
        palette: None,
        chartex: true,
    })
}

/// `cx:strDim` / `cx:numDim` 的第一级 `cx:lvl/cx:pt[@idx]`（层级图的叶子标签在第一级）。
fn chartex_points(dom: &Dom, dim: NodeId) -> Vec<Option<String>> {
    let Some(lvl) = chart_child(dom, dim, NsId::Cx, LocalName::Lvl) else { return Vec::new() };
    let mut out: Vec<Option<String>> = Vec::new();
    for pt in children(dom, lvl, NsId::Cx, LocalName::Pt) {
        let Some(idx) =
            chart_attr(dom, pt, LocalName::Idx).and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        if out.len() <= idx {
            out.resize(idx + 1, None);
        }
        out[idx] = Some(chart_text_of(dom, pt));
    }
    out
}

// ---- DOM 小工具 -------------------------------------------------------------------------------------

/// 命名空间 + 局部名匹配；前缀未绑定时按规范前缀字面量兜底（`Dom::is_ns`）。
fn is(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> bool {
    dom.name(node).is_some_and(|n| n.local == local) && dom.is_ns(node, ns, prefix_of(ns))
}

fn prefix_of(ns: NsId) -> &'static str {
    match ns {
        NsId::C => "c",
        NsId::Cx => "cx",
        NsId::C14 => "c14",
        NsId::A => "a",
        _ => "",
    }
}

fn chart_child(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&c| is(dom, c, ns, local))
}

fn children<'a>(
    dom: &'a Dom,
    node: NodeId,
    ns: NsId,
    local: LocalName,
) -> impl Iterator<Item = NodeId> + 'a {
    dom.semantic_children(node).filter(move |&c| is(dom, c, ns, local))
}

/// 无命名空间属性。
fn chart_attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, local)).map(|s| s.to_string())
}

/// 元素的直接文本内容（`a:t` / `c:v` / `cx:pt`），实体已解码，不 trim。
fn chart_text_of(dom: &Dom, node: NodeId) -> String {
    dom.children(node).iter().filter_map(|&c| dom.text(c)).collect()
}

fn range_of(dom: &Dom, node: NodeId) -> Option<Range<u32>> {
    dom.node(node).lex.as_ref().map(|l| l.range.clone())
}

// `a:custGeom` 自定义几何（`MOD-11`；`spec/15` 任务 4.6d）。
//
// 只记录路径命令与每条 `a:path` 的声明尺寸；归一化到 0..1 与拼成 SVG 路径串是显示投影，
// 在 `bind/compat_ts`。
//
// ## 只做能如实表达的那部分
//
// OOXML 的自定义几何可以在 `a:avLst` / `a:gdLst` 里写公式（`gd/@fmla`），坐标写成引导名而不是
// 数字，还能用 `a:arcTo` 画椭圆弧。公式求值器与弧转贝塞尔不在 M4 范围里，所以**遇到这些就整条
// 几何返回 `None`**——宁可不给路径，也不能给一条少了几段、或者坐标当成 0 的错路径。
//
// 语料里的四份 `shape-extraction__*` 都是 `moveTo` + 三段 `lnTo` + `close` 的直边矩形，落在
// 支持范围内。

/// 一个 `a:custGeom` 的全部路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomGeom {
    pub node: NodeId,
    pub paths: Vec<GeomPath>,
}

/// 一条 `a:path`。坐标在这条路径自己的坐标系里（`@w` / `@h`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeomPath {
    /// `@w` / `@h`：这条路径的坐标空间；缺省时用形状的 `a:ext`。
    pub w: Option<i64>,
    pub h: Option<i64>,
    /// `@fill="none"`。
    pub fill_none: bool,
    /// `@stroke="0" | "false" | "none"`。
    pub stroke_none: bool,
    pub cmds: Vec<GeomCmd>,
}

/// 路径命令。点是路径坐标系里的绝对坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomCmd {
    MoveTo([i64; 2]),
    LineTo([i64; 2]),
    /// `a:quadBezTo`：控制点 + 终点。
    QuadTo([[i64; 2]; 2]),
    /// `a:cubicBezTo`：两个控制点 + 终点。
    CubicTo([[i64; 2]; 3]),
    Close,
}

impl GeomCmd {
    /// SVG 的命令字母。
    pub fn letter(&self) -> char {
        match self {
            GeomCmd::MoveTo(_) => 'M',
            GeomCmd::LineTo(_) => 'L',
            GeomCmd::QuadTo(_) => 'Q',
            GeomCmd::CubicTo(_) => 'C',
            GeomCmd::Close => 'Z',
        }
    }

    /// 命令带的点。
    pub fn points(&self) -> &[[i64; 2]] {
        match self {
            GeomCmd::MoveTo(p) | GeomCmd::LineTo(p) => std::slice::from_ref(p),
            GeomCmd::QuadTo(p) => p,
            GeomCmd::CubicTo(p) => p,
            GeomCmd::Close => &[],
        }
    }
}

/// 解析 `a:custGeom`。用到公式或圆弧时返回 `None`（见模块文档）。
pub fn custom_geom(dom: &Dom, cust_geom: NodeId) -> Option<CustomGeom> {
    // 有公式的引导表就整条放弃：坐标可能引用引导名，我们求不了值。
    for c in dom.semantic_descendants(cust_geom) {
        if dom.is(c, a(LocalName::Gd)) {
            return None;
        }
    }
    let path_lst = dom.semantic_children(cust_geom).find(|&c| dom.is(c, a(LocalName::PathLst)))?;
    let mut paths = Vec::new();
    for p in dom.semantic_children(path_lst) {
        if !dom.is(p, a(LocalName::Path)) {
            continue;
        }
        paths.push(geom_path(dom, p)?);
    }
    (!paths.is_empty()).then_some(CustomGeom { node: cust_geom, paths })
}

fn geom_path(dom: &Dom, path: NodeId) -> Option<GeomPath> {
    let mut out = GeomPath {
        w: custgeom_num(dom, path, LocalName::W),
        h: custgeom_num(dom, path, LocalName::H),
        fill_none: custgeom_attr(dom, path, LocalName::Fill).as_deref() == Some("none"),
        stroke_none: matches!(
            custgeom_attr(dom, path, LocalName::Stroke).as_deref(),
            Some("0") | Some("false") | Some("none")
        ),
        cmds: Vec::new(),
    };
    for c in dom.semantic_children(path) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        let need = match name.local {
            LocalName::MoveTo | LocalName::LnTo => 1,
            LocalName::QuadBezTo => 2,
            LocalName::CubicBezTo => 3,
            LocalName::Close => {
                out.cmds.push(GeomCmd::Close);
                continue;
            }
            // 圆弧要转成贝塞尔，M4 不做
            LocalName::ArcTo => return None,
            _ => continue,
        };
        let pts: Vec<[i64; 2]> = dom
            .semantic_children(c)
            .filter(|&n| dom.is(n, a(LocalName::Pt)))
            .map(|n| point(dom, n))
            .collect::<Option<Vec<_>>>()?;
        if pts.len() < need {
            return None;
        }
        out.cmds.push(match need {
            1 if name.local == LocalName::MoveTo => GeomCmd::MoveTo(pts[0]),
            1 => GeomCmd::LineTo(pts[0]),
            2 => GeomCmd::QuadTo([pts[0], pts[1]]),
            _ => GeomCmd::CubicTo([pts[0], pts[1], pts[2]]),
        });
    }
    Some(out)
}

/// `a:pt`。`ST_AdjCoordinate` 要么是整数，要么是引导名——引导名我们求不了值，整条几何作废。
fn point(dom: &Dom, pt: NodeId) -> Option<[i64; 2]> {
    let coord = |l: LocalName| -> Option<i64> {
        let v = custgeom_attr(dom, pt, l)?.parse::<f64>().ok()?;
        (v.is_finite() && v.fract() == 0.0).then_some(v as i64)
    };
    Some([coord(LocalName::X)?, coord(LocalName::Y)?])
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

fn custgeom_attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, local)).map(|s| s.trim().to_string())
}

fn custgeom_num(dom: &Dom, node: NodeId, local: LocalName) -> Option<i64> {
    custgeom_attr(dom, node, local)?.parse().ok()
}

// 声明模型（`MOD-10`，任务 1.4）：styles / numbering / settings / fontTable 的入口与查找辅助。
//
// 类型本身由属性表生成（`schema/props/{styles,numbering,settings,font_table}.toml`），
// 这里只加"从 part 读取"和只读查找；样式链、编号覆盖合并等解释在 `resolve`。

pub use crate::semantic::props::{
    AbstractNum, Compat, CompatSetting, DocDefaults, Font, FontTable, Level, LevelOverride, Num,
    Numbering, ParaProps, RunProps, Settings, Style, StyleType, TableStylePr, TblStyleOverrideType,
};
use crate::semantic::props::{
    Val, codec::OnOff, read_font_table, read_numbering, read_settings, read_styles,
};

fn root_if(dom: &Dom, name: QName) -> Option<NodeId> {
    let root = dom.root();
    dom.is(root, name).then_some(root)
}

fn val_i32(v: &Option<Val<i32>>) -> Option<i32> {
    v.as_ref().and_then(|x| x.value().copied())
}

// ---- Styles -------------------------------------------------------------------------------------

impl Styles {
    /// 根须是 `w:styles`，否则 `None`。
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Styles> {
        let root = root_if(dom, QName::w(LocalName::Styles))?;
        Some(read_styles(dom, Some(root), diags))
    }

    /// 按 `styleId` 查找。重复的 styleId（语料里有）取**最后一个**声明，与 TS 的 `Map` 语义一致。
    pub fn get(&self, id: &str) -> Option<&Style> {
        self.styles.iter().rfind(|s| s.id() == Some(id))
    }

    /// 某类型的默认样式：该类型最后一个 `w:default="1|true"`；没有声明的 → 该类型中 styleId 或
    /// name 为 `Normal`（不分大小写）的第一个；再没有 → `None`（只剩 docDefaults）。
    ///
    /// 与 `RES-02` 引用的 ECMA-376 §17.7.4.17 "取该类型第一个样式"不同：Word 实测不用
    /// first-of-type 规则（TS `parseStyles` 注释与差分语料），这里按 Word 行为。
    pub fn default_for(&self, kind: StyleType) -> Option<&Style> {
        let of_kind = || self.styles.iter().filter(move |s| s.kind() == Some(kind));
        of_kind().rfind(|s| s.is_default == Some(true)).or_else(|| {
            of_kind().find(|s| {
                s.id().is_some_and(|i| i.eq_ignore_ascii_case("normal"))
                    || s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case("normal"))
            })
        })
    }

    pub fn doc_default_rpr(&self) -> Option<&RunProps> {
        self.doc_defaults.as_ref()?.rpr_default.as_ref()?.rpr.as_ref()
    }

    pub fn doc_default_ppr(&self) -> Option<&ParaProps> {
        self.doc_defaults.as_ref()?.ppr_default.as_ref()?.ppr.as_ref()
    }

    /// 段落样式自身给出的标题级别；不沿 basedOn 继承（`RES-02` 做继承）。
    pub fn own_heading_level(style: &Style) -> OwnHeadingLevel {
        if let Some(l) = style.name.as_deref().and_then(heading_level_of_name) {
            return OwnHeadingLevel::Level(l);
        }
        if let Some(l) = style.id().and_then(heading_level_of_id) {
            return OwnHeadingLevel::Level(l);
        }
        match style.ppr.as_ref().and_then(|p| val_i32(&p.outline_lvl)) {
            Some(l @ 0..=8) => OwnHeadingLevel::Level(l as u8 + 1),
            Some(_) => OwnHeadingLevel::Blocked,
            None => OwnHeadingLevel::Inherit,
        }
    }
}

/// [`Styles::own_heading_level`] 的结果（`RES-02` heading_level）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnHeadingLevel {
    /// 名字 / id 匹配 `heading N`，或 `outlineLvl` 0–8。
    Level(u8),
    /// `outlineLvl == 9`：正文级，阻断 basedOn 继承（`TOCHeading basedOn Heading1`）。
    Blocked,
    /// 未指定，沿 basedOn 继承。
    Inherit,
}

/// `/^heading\s*([1-9])$/i`
pub(in crate::model) fn heading_level_of_name(name: &str) -> Option<u8> {
    let rest = name.get(..7).filter(|p| p.eq_ignore_ascii_case("heading")).map(|_| &name[7..])?;
    let rest = rest.trim_start();
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !('1'..='9').contains(&d) {
        return None;
    }
    Some(d as u8 - b'0')
}

/// `/^Heading([1-9])$/`
pub(in crate::model) fn heading_level_of_id(id: &str) -> Option<u8> {
    let rest = id.strip_prefix("Heading")?;
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !('1'..='9').contains(&d) {
        return None;
    }
    Some(d as u8 - b'0')
}

impl Style {
    pub fn id(&self) -> Option<&str> {
        self.style_id.as_deref()
    }

    pub fn kind(&self) -> Option<StyleType> {
        self.kind.as_ref().and_then(|v| v.value().copied())
    }

    /// 显示名；缺省用 styleId（TS 行为）。
    pub fn display_name(&self) -> Option<&str> {
        self.name.as_deref().or(self.id())
    }

    pub fn is_default_flag(&self) -> bool {
        self.is_default == Some(true)
    }
}

// ---- Numbering ----------------------------------------------------------------------------------

impl Numbering {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Numbering> {
        let root = root_if(dom, QName::w(LocalName::Numbering))?;
        Some(read_numbering(dom, Some(root), diags))
    }

    pub fn abstract_num(&self, id: i32) -> Option<&AbstractNum> {
        self.abstract_nums.iter().find(|a| val_i32(&a.abstract_num_id) == Some(id))
    }

    pub fn num(&self, num_id: i32) -> Option<&Num> {
        self.nums.iter().find(|n| val_i32(&n.num_id) == Some(num_id))
    }
}

impl AbstractNum {
    pub fn id(&self) -> Option<i32> {
        val_i32(&self.abstract_num_id)
    }

    pub fn level(&self, ilvl: i32) -> Option<&Level> {
        self.levels.iter().find(|l| l.ilvl() == Some(ilvl))
    }
}

impl Level {
    pub fn ilvl(&self) -> Option<i32> {
        val_i32(&self.ilvl)
    }

    /// `w:start`；缺省 0（ECMA-376 §17.9.25；Word 显示 "0."）。
    pub fn start_or_default(&self) -> i32 {
        val_i32(&self.start).unwrap_or(0)
    }
}

impl Num {
    pub fn id(&self) -> Option<i32> {
        val_i32(&self.num_id)
    }

    pub fn abstract_id(&self) -> Option<i32> {
        val_i32(&self.abstract_num_id)
    }

    pub fn override_for(&self, ilvl: i32) -> Option<&LevelOverride> {
        self.overrides.iter().find(|o| val_i32(&o.ilvl) == Some(ilvl))
    }
}

// ---- Settings -----------------------------------------------------------------------------------

/// 兼容事实（`docs/03` §6.5）：只记录，`resolve` 与布局层解释。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatFacts {
    /// `compatSetting[name=compatibilityMode]/@val`。
    pub mode: Option<u32>,
    pub settings: Vec<CompatSetting>,
    /// `w:compat` 下值为真的布尔子元素。
    pub flags: Vec<QName>,
}

impl Settings {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<Settings> {
        let root = root_if(dom, QName::w(LocalName::Settings))?;
        Some(read_settings(dom, Some(root), diags))
    }

    pub fn compat_facts(&self, dom: &Dom) -> CompatFacts {
        let Some(compat) = &self.compat else { return CompatFacts::default() };
        let settings = compat.settings.clone();
        let mode = settings
            .iter()
            .find(|s| s.name.as_deref() == Some("compatibilityMode"))
            .and_then(|s| s.val.as_deref()?.trim().parse().ok());
        let mut flags = Vec::new();
        let mut sink = Vec::new();
        let mut ctx = crate::semantic::props::Ctx::new(dom, &mut sink);
        for &n in &compat.raw_unmodeled {
            let Some(name) = dom.name(n) else { continue };
            ctx.enter(n);
            let on = match dom.attr_value(n, QName::w(LocalName::Val)) {
                Some(v) => <OnOff as crate::semantic::props::Codec>::parse(&v, &mut ctx),
                None => true,
            };
            if on {
                flags.push(name);
            }
        }
        CompatFacts { mode, settings, flags }
    }

    pub fn compatibility_mode(&self, dom: &Dom) -> Option<u32> {
        self.compat_facts(dom).mode
    }

    /// `w:defaultTabStop`，twip；缺省 720（Word）。
    pub fn default_tab_stop_or_default(&self) -> i32 {
        val_i32(&self.default_tab_stop).unwrap_or(720)
    }
}

// ---- FontTable ----------------------------------------------------------------------------------

impl FontTable {
    pub fn from_dom(dom: &Dom, diags: &mut Vec<Diagnostic>) -> Option<FontTable> {
        let root = root_if(dom, QName::w(LocalName::Fonts))?;
        Some(read_font_table(dom, Some(root), diags))
    }

    pub fn get(&self, name: &str) -> Option<&Font> {
        self.fonts.iter().find(|f| f.name.as_deref() == Some(name))
    }
}

pub use crate::semantic::props::Styles;

// SmartArt 与绘图画布的模型（`MOD-11`，`spec/17` 任务 6.3）。
//
// SmartArt 有两个 part：**数据 part**（`dgm:dataModel`，主 part 里 `dgm:relIds/@r:dm` 指向）给节点文字，
// **绘图 part**（`dsp:drawing`，Word 保存下来的已排版结果）给形状。两者都是有自己 DOM 的 XML part（L1），
// 这里只读**事实**：EMU、1/60000 度、颜色的原始定义；px 换算、画布缩放与排版启发式全在
// `bind/compat_ts/diagram.rs`。画布（`lc:lockedCanvas`，R14）在主 part 里，形状用同一个
// [`DiagramShape`]，外加子坐标系（[`CanvasDisplay`]）。

use std::collections::HashSet;

/// 一个形状：绘图 part 的 `dsp:sp`，或画布里的 `a:sp` / `a:pic`。几何是原值（EMU、1/60000 度）；
/// 画布形状的几何在**子坐标系**里（[`CanvasDisplay::ch_off`] / `ch_ext`），缩放在投影层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramShape {
    pub node: NodeId,
    /// `a:xfrm/a:off`。
    pub off_emu: (i64, i64),
    /// `a:xfrm/a:ext`。连线（`prst=line` / `*Connector*`）可以一边为 0。
    pub ext_emu: Extent,
    /// `a:xfrm/@rot`。
    pub rot_60k: Option<i64>,
    /// `a:prstGeom/@prst`。
    pub prst: Option<String>,
    /// `a:noFill`。
    pub no_fill: bool,
    /// `a:solidFill` 容器节点。颜色留原始定义，解析成 sRGB 走 [`crate::resolve::drawingml::color_in`]
    /// （与 M4 的 [`crate::model::FillDisplay`] 同一约定；节点属于形状所在的 part）。
    pub fill: Option<NodeId>,
    /// `a:gradFill` 节点（投影层取各停靠点的等权平均）。
    pub gradient: Option<NodeId>,
    /// `a:ln`（有 `a:noFill` 的线不记）。
    pub line: Option<DiagramLine>,
    /// `a:blipFill`（`dsp:spPr` 里的图片填充，或 `a:pic` 自己的图）。
    pub picture: Option<DiagramPicture>,
    /// `txBody` 各段文字（`a:r/a:t` 拼接，空段不记）。
    pub texts: Vec<String>,
    /// 第一个带 `sz` 的 `a:rPr`：字号，1/100 pt 原值。
    pub font_size_100pt: Option<i64>,
    /// 同一轮里读到的 `a:rPr/a:solidFill` 容器节点。
    pub text_color: Option<NodeId>,
}

/// `a:ln`：颜色容器（`a:solidFill`）与 `@w`（EMU）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramLine {
    pub node: NodeId,
    pub color: Option<NodeId>,
    pub width_emu: Option<i64>,
}

/// 图片填充：`a:blip/@r:embed`（按**所在 part** 的关系解）与 `a:stretch/a:fillRect`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramPicture {
    pub embed: Option<String>,
    pub fill_rect: Option<RectFrac>,
}

/// 主 part 引用的一个 SmartArt：数据 part 与（可能没有的）绘图 part。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramPart {
    pub data: PartId,
    /// 绘图 part：数据 part 的 `diagramDrawing` 关系，找不到时按 TS 的路径约定 `data{N}.xml → drawing{N}.xml`。
    pub drawing: Option<PartId>,
    /// 数据 part 的节点文字（树序，`\n` 连接）；一个字都没有、part 缺失或不是 `dgm:dataModel` → `None`。
    pub text: Option<String>,
    /// 绘图 part 的形状；part 缺失 / 解析不了 / 一个形状都没有 → `None`。
    pub shapes: Option<Vec<DiagramShape>>,
}

impl DiagramPart {
    /// `data_dom` / 绘图 part 的 DOM 为 `None` = part 是二进制或解析失败（`Package` 已记 `PKG_OPAQUE_PART`）。
    pub fn build(
        data: PartId,
        data_dom: Option<&Dom>,
        drawing: Option<(PartId, Option<&Dom>)>,
        warnings: &mut Vec<Diagnostic>,
    ) -> DiagramPart {
        let text = data_dom.and_then(|dom| {
            let root = dom.root();
            if !dom.is(root, QName::new(NsId::Dgm, LocalName::DataModel)) {
                warnings.push(Diagnostic::pre_existing(
                    data,
                    dom.node(root).lex.as_ref().map(|l| l.range.clone()),
                    DiagCode::ModUnparseable,
                    "SmartArt 数据 part 的根不是 dgm:dataModel",
                ));
                return None;
            }
            diagram_text(dom)
        });
        let shapes = drawing.and_then(|(_, dom)| dom).map(diagram_shapes).filter(|s| !s.is_empty());
        DiagramPart { data, drawing: drawing.map(|(id, _)| id), text, shapes }
    }
}

/// 数据 part 的节点文字，按内容树的先序（TS `extractDiagramText`）。
///
/// `dgm:pt` 里 `type ∈ {pres, parTrans, sibTrans}` 的是排版点，不算；文字是 `a:t` 拼接后 trim。
/// `dgm:cxn` 没写 `type` 或 `type="parOf"` 的是父子边，按 `srcOrd` 排；根 = 出现过做源点、没有父的点，
/// 按首次出现的顺序各走一遍先序（显式栈 + `seen`，成环 / 自指不会死循环）；没进树的点按文件序追加。
pub fn diagram_text(dom: &Dom) -> Option<String> {
    let root = dom.root();
    // (modelId, 文字)，文件序
    let mut points: Vec<(String, String)> = Vec::new();
    let mut text_of_id: HashMap<String, usize> = HashMap::new();
    let mut src_order: Vec<String> = Vec::new();
    let mut children: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    let mut has_parent: HashSet<String> = HashSet::new();
    for n in dom.semantic_descendants(root) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::Dgm {
            continue;
        }
        match name.local {
            LocalName::Pt => {
                let Some(id) = attr(dom, n, NsId::None, LocalName::ModelId) else { continue };
                if attr(dom, n, NsId::None, LocalName::Type)
                    .is_some_and(|t| matches!(t.as_str(), "pres" | "parTrans" | "sibTrans"))
                {
                    continue;
                }
                let mut s = String::new();
                for t in dom.semantic_descendants(n) {
                    if dom.is(t, QName::new(NsId::A, LocalName::T))
                        && let Some(x) = text_of(dom, t)
                    {
                        s.push_str(&x);
                    }
                }
                let s = s.trim();
                if s.is_empty() || text_of_id.contains_key(&id) {
                    continue;
                }
                text_of_id.insert(id.clone(), points.len());
                points.push((id, s.to_string()));
            }
            LocalName::Cxn => {
                if attr(dom, n, NsId::None, LocalName::Type).is_some_and(|t| t != "parOf") {
                    continue;
                }
                let (Some(src), Some(dst)) = (
                    attr(dom, n, NsId::None, LocalName::SrcId),
                    attr(dom, n, NsId::None, LocalName::DestId),
                ) else {
                    continue;
                };
                let ord = num(dom, n, LocalName::SrcOrd).unwrap_or(0);
                if !children.contains_key(&src) {
                    src_order.push(src.clone());
                }
                children.entry(src).or_default().push((ord, dst.clone()));
                has_parent.insert(dst);
            }
            _ => {}
        }
    }
    for kids in children.values_mut() {
        kids.sort_by_key(|(ord, _)| *ord);
    }
    let mut texts: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = Vec::new();
    for root_id in src_order.iter().filter(|s| !has_parent.contains(*s)) {
        stack.push(root_id);
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(&i) = text_of_id.get(id) {
                texts.push(&points[i].1);
            }
            if let Some(kids) = children.get(id) {
                stack.extend(kids.iter().rev().map(|(_, d)| d.as_str()));
            }
        }
    }
    for (id, t) in &points {
        if !seen.contains(id.as_str()) {
            texts.push(t);
        }
    }
    (!texts.is_empty()).then(|| texts.join("\n"))
}

/// 绘图 part 里全部 `dsp:sp`（含 `dsp:grpSp` 里的），文档序；没有 `dsp:spPr` 或 `a:xfrm` 不全的跳过。
pub fn diagram_shapes(dom: &Dom) -> Vec<DiagramShape> {
    let mut out = Vec::new();
    for sp in dom.semantic_descendants(dom.root()) {
        if !dom.is(sp, QName::new(NsId::Dsp, LocalName::Sp)) {
            continue;
        }
        let Some(sp_pr) = diagram_child(dom, sp, NsId::Dsp, LocalName::SpPr) else { continue };
        let tx_body = diagram_child(dom, sp, NsId::Dsp, LocalName::TxBody);
        if let Some(s) = read_shape(dom, sp, sp_pr, None, tx_body) {
            out.push(s);
        }
    }
    out
}

/// 一个 `lc:lockedCanvas`：子坐标系与直接子元素里的 `a:sp` / `a:pic`（TS 不下钻 `a:grpSp`，这里也不）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasDisplay {
    pub node: NodeId,
    /// `a:grpSpPr/a:xfrm/a:chOff`（缺省 0,0）。
    pub ch_off: Option<(i64, i64)>,
    /// `a:grpSpPr/a:xfrm/a:chExt`（缺省 = 宿主 `wp:extent`）。
    pub ch_ext: Option<Extent>,
    pub shapes: Vec<DiagramShape>,
}

pub fn canvas_display(dom: &Dom, lc: NodeId) -> CanvasDisplay {
    let mut c = CanvasDisplay { node: lc, ch_off: None, ch_ext: None, shapes: Vec::new() };
    for n in dom.semantic_children(lc) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::GrpSpPr => {
                if let Some(xfrm) = diagram_child(dom, n, NsId::A, LocalName::Xfrm) {
                    c.ch_off = diagram_child(dom, xfrm, NsId::A, LocalName::ChOff)
                        .and_then(|g| xy(dom, g));
                    c.ch_ext = diagram_child(dom, xfrm, NsId::A, LocalName::ChExt)
                        .and_then(|g| extent_of(dom, g));
                }
            }
            LocalName::Sp => {
                let Some(sp_pr) = diagram_child(dom, n, NsId::A, LocalName::SpPr) else { continue };
                let tx_body = diagram_child(dom, n, NsId::A, LocalName::TxSp)
                    .and_then(|t| diagram_child(dom, t, NsId::A, LocalName::TxBody));
                if let Some(s) = read_shape(dom, n, sp_pr, None, tx_body) {
                    c.shapes.push(s);
                }
            }
            LocalName::Pic => {
                let Some(sp_pr) = diagram_child(dom, n, NsId::A, LocalName::SpPr) else { continue };
                let blip = diagram_child(dom, n, NsId::A, LocalName::BlipFill);
                if let Some(s) = read_shape(dom, n, sp_pr, blip, None) {
                    c.shapes.push(s);
                }
            }
            _ => {}
        }
    }
    c
}

/// `spPr`（几何 / 填充 / 线）+ 可选的独立 `a:blipFill`（`a:pic`）+ 可选的文字体 → 形状。
fn read_shape(
    dom: &Dom,
    node: NodeId,
    sp_pr: NodeId,
    blip_fill: Option<NodeId>,
    tx_body: Option<NodeId>,
) -> Option<DiagramShape> {
    let xfrm = diagram_child(dom, sp_pr, NsId::A, LocalName::Xfrm)?;
    let off = diagram_child(dom, xfrm, NsId::A, LocalName::Off).and_then(|g| xy(dom, g))?;
    let ext = diagram_child(dom, xfrm, NsId::A, LocalName::Ext).and_then(|g| extent_of(dom, g))?;
    let mut s = DiagramShape {
        node,
        off_emu: off,
        ext_emu: ext,
        rot_60k: num(dom, xfrm, LocalName::Rot),
        prst: None,
        no_fill: false,
        fill: None,
        gradient: None,
        line: None,
        picture: None,
        texts: Vec::new(),
        font_size_100pt: None,
        text_color: None,
    };
    for c in dom.semantic_children(sp_pr) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::PrstGeom => s.prst = attr(dom, c, NsId::None, LocalName::Prst),
            LocalName::NoFill => s.no_fill = true,
            LocalName::SolidFill if s.fill.is_none() => s.fill = Some(c),
            LocalName::GradFill if s.gradient.is_none() => s.gradient = Some(c),
            LocalName::BlipFill if s.picture.is_none() => s.picture = Some(picture_of(dom, c)),
            LocalName::Ln
                if s.line.is_none()
                    && diagram_child(dom, c, NsId::A, LocalName::NoFill).is_none() =>
            {
                s.line = Some(DiagramLine {
                    node: c,
                    color: diagram_child(dom, c, NsId::A, LocalName::SolidFill),
                    width_emu: num(dom, c, LocalName::W),
                });
            }
            _ => {}
        }
    }
    if let Some(b) = blip_fill {
        s.picture = Some(picture_of(dom, b));
    }
    if let Some(body) = tx_body {
        read_text_body(dom, body, &mut s);
    }
    Some(s)
}

fn picture_of(dom: &Dom, blip_fill: NodeId) -> DiagramPicture {
    let mut p = DiagramPicture { embed: None, fill_rect: None };
    for n in dom.semantic_descendants(blip_fill) {
        let Some(name) = dom.name(n) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::Blip if p.embed.is_none() => {
                p.embed = attr(dom, n, NsId::R, LocalName::Embed)
            }
            LocalName::FillRect if p.fill_rect.is_none() => p.fill_rect = Some(rect_frac(dom, n)),
            _ => {}
        }
    }
    p
}

/// `txBody/a:p/a:r`：段文字、第一个带 `sz` 的 `a:rPr` 的字号与颜色（TS 的读法：字号没定下来之前
/// 每个 `a:rPr` 都看一眼，颜色跟着最后看的那个）。
fn read_text_body(dom: &Dom, body: NodeId, s: &mut DiagramShape) {
    for p in dom.semantic_children(body) {
        if !dom.is(p, QName::new(NsId::A, LocalName::P)) {
            continue;
        }
        let mut text = String::new();
        for r in dom.semantic_children(p) {
            if !dom.is(r, QName::new(NsId::A, LocalName::R)) {
                continue;
            }
            if let Some(t) = diagram_child(dom, r, NsId::A, LocalName::T)
                && let Some(x) = text_of(dom, t)
            {
                text.push_str(&x);
            }
            if s.font_size_100pt.is_none()
                && let Some(rpr) = diagram_child(dom, r, NsId::A, LocalName::RPr)
            {
                s.font_size_100pt = num(dom, rpr, LocalName::Sz).filter(|&v| v > 0);
                if let Some(c) = diagram_child(dom, rpr, NsId::A, LocalName::SolidFill) {
                    s.text_color = Some(c);
                }
            }
        }
        if !text.trim().is_empty() {
            s.texts.push(text);
        }
    }
}

fn diagram_child(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&c| dom.is(c, QName::new(ns, local)))
}

// 绘图显示模型（`MOD-11`；`spec/15` 任务 4.3）。
//
// 一个 `w:drawing` 的**文档事实**：锚定几何、`wp:extent`、`wp:docPr`，以及 `pic:pic` 的图片信息。
// 这里**没有**任何由排版决定的字段（px、band、猜出来的浮动方向）——那些是 `bind/compat_ts` 的
// 投影（`spec/15` 分层决策）。长度一律 EMU 原值，角度一律 1/60000 度原值。
//
// ## 遍历边界
//
// `w:txbxContent` 是**独立内容流**：文本框里的段落有自己的 run 与自己的图。所以扫一个 drawing 时
// 不下钻进 `txbxContent`，也不下钻进嵌套的 `w:drawing`——否则文本框里的图会被当成段落级图片，
// 分类全错（`spec/15` 风险 3，对应 TS `topLevelDrawings` 的平衡匹配）。
//
// 遍历是迭代的，带深度上限：语料里有几千层嵌套的恶意输入。

/// 绘图子树的深度上限，与 `MOD-07` 的块嵌套上限同值。
const DRAWING_MAX_DEPTH: u32 = 64;

/// `Segment.display` / `ProtectedBlock.display` / `ImageBlock.display`：显示载荷（`MOD-11`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Display {
    /// `w:drawing`
    Drawing(Box<DrawingDisplay>),
    /// `w:pict` / `w:object`（含 OLE 信息）
    Vml(Box<VmlDisplay>),
    /// 公式段落（R11）的 `m:oMath` 片段、token、MathML / LaTeX（M6 6.5，`model::math`）。
    Formula(Box<FormulaDisplay>),
}

impl Display {
    pub fn as_drawing(&self) -> Option<&DrawingDisplay> {
        match self {
            Display::Drawing(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_vml(&self) -> Option<&VmlDisplay> {
        match self {
            Display::Vml(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_formula(&self) -> Option<&FormulaDisplay> {
        match self {
            Display::Formula(f) => Some(f),
            _ => None,
        }
    }
}

/// 一个 `w:drawing`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingDisplay {
    /// `w:drawing` 节点。
    pub node: NodeId,
    pub kind: DrawingKind,
    /// `wp:anchor` 的锚定几何；`wp:inline` → `None`（随文）。
    pub anchor: Option<AnchorGeom>,
    /// `wp:extent`（EMU）。
    pub extent: Option<Extent>,
    pub doc_pr: DocPr,
    /// 全部 `pic:pic`，文档序。段落级图片取第一个（[`DrawingDisplay::picture`]）；
    /// 组里的图片各有自己的位置与所属组。图表 / SmartArt 的载荷在 M6。
    pub pictures: Vec<ImageDisplay>,
    /// `wps:wsp` 形状与 `wpg` 组，文档序；组内形状排在组之后，`group` 指回组。
    pub shapes: Vec<ShapeDisplay>,
    /// `a:graphicData` 里的 `c:chart` / `cx:chart`：图表 part 的引用（M6 6.1）。part 本身在
    /// `Document.chart_parts`，按 `rel_id` 经 `Document.chart_by_rel` 找。
    pub chart: Option<ChartRef>,
    /// `dgm:relIds`：SmartArt 两个 part 的引用（M6 6.3）。part 在 `Document.diagram_parts`，
    /// 按 `rel_id`（`@r:dm`）经 `Document.diagram_by_rel` 找。
    pub diagram: Option<DiagramRef>,
    /// `lc:lockedCanvas`：绘图画布的子坐标系与形状（M6 6.3；`model::diagram`）。
    pub canvas: Option<Box<CanvasDisplay>>,
}

/// `dgm:relIds`：一个绘图对 SmartArt part 的引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramRef {
    pub node: NodeId,
    /// `@r:dm`（数据 part）；写丢了 → `None`（TS 同样解析不出图示）。
    pub rel_id: Option<String>,
}

/// `c:chart r:id` / `cx:chart r:id`：一个绘图对图表 part 的引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartRef {
    pub node: NodeId,
    /// `r:id`；写丢了 → `None`（TS 同样解析不出图表）。
    pub rel_id: Option<String>,
    /// `cx:chart`（2014 chartex）。
    pub chartex: bool,
}

/// 一个 `wps:wsp` 形状，或一个 `wpg:wgp` / `wpg:grpSp` 组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeDisplay {
    pub node: NodeId,
    /// 组本身（`wpg`）不画东西，只提供子坐标系。
    pub is_group: bool,
    /// `wps:cNvPr/@id`：保存路径要靠它往形状里塞新的 `wps:txbx`。
    pub cnv_id: Option<String>,
    /// `a:prstGeom/@prst`：预设几何名（`rect` / `line` / `straightConnector1` …）。
    pub prst: Option<String>,
    /// 有 `a:custGeom`：自定义路径几何。
    pub cust_geom: bool,
    /// `a:custGeom` 的路径；用到公式或圆弧时为 `None`（`model::custgeom`）。
    pub geom: Option<CustomGeom>,
    /// `a:xfrm/a:ext`（EMU）。
    pub ext: Option<Extent>,
    /// `a:xfrm/a:off`（EMU）。
    pub off: Option<(i64, i64)>,
    /// 组的子坐标系原点与尺寸（`a:chOff` / `a:chExt`），用来算组内形状的仿射。
    pub ch_off: Option<(i64, i64)>,
    pub ch_ext: Option<Extent>,
    /// `a:xfrm/@rot`，1/60000 度。
    pub rot_60k: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    /// `spPr` 的填充。
    pub fill: Option<FillDisplay>,
    /// `spPr/a:ln`。
    pub line: Option<LineDisplay>,
    /// `wps:style/a:fillRef` / `a:lnRef`：主题引用，`idx > 0` 时补缺省颜色。
    pub fill_ref: Option<StyleRef>,
    pub line_ref: Option<StyleRef>,
    /// `wps:style/a:fontRef`：图库形状的文字颜色出处（缺省蓝形状引用 `lt1`，所以 Word 里
    /// 不写任何 run 颜色也显示白字）。
    pub font_ref: Option<StyleRef>,
    /// `spPr/a:effectLst` 里有内容（阴影等）。空形状判定要看它。
    pub has_effects: bool,
    /// `wps:bodyPr`。
    pub body: Option<BodyPr>,
    /// `wps:txbx/w:txbxContent`：框里的独立内容流。
    pub txbx: Option<NodeId>,
    /// `wps:txbx/@r:txbx`：框的内容在**另一个 part** 里（`word/txbx1.xml`，根是 `w14:txbx`）。
    /// 与 `txbx` 互斥：本 part 里没有 `w:txbxContent` 时才看它。
    pub txbx_rel: Option<String>,
    /// 框里内容流的块（`MOD-11` 的 `content`）。由 `Document::rebuild` 复用段落管线构建。
    pub content: Vec<Block>,
    /// `content` 里的 `NodeId` 属于哪个 part。`None` = 本 part（`txbx`）；
    /// `Some` = 外部文本框 part（`txbx_rel`），投影要换那个 part 的 DOM。
    pub content_part: Option<crate::package::PartId>,
    /// 所属组在 `shapes` 里的下标。
    pub group: Option<usize>,
}

impl ShapeDisplay {
    /// 有可见的填充、描边或图片——空形状据此判断要不要留下（TS `buildWpsBox`）。
    pub fn has_paint(&self) -> bool {
        self.fill.as_ref().is_some_and(|f| f.kind != FillKind::None)
            || self.line.as_ref().is_some_and(|l| !l.no_fill && l.fill.is_some())
            || self.fill_ref.is_some()
    }
}

/// `spPr` 的填充种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillKind {
    /// `a:noFill`
    None,
    Solid,
    Gradient,
    Pattern,
    /// `a:blipFill`：图片填充。
    Blip,
    /// `a:grpFill`：继承所在 `wpg` 组的填充。
    Group,
}

/// 填充。颜色留原始定义（容器节点），解析成 sRGB 走 [`crate::resolve::drawingml`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillDisplay {
    pub node: NodeId,
    pub kind: FillKind,
    /// `a:blipFill/a:blip/@r:embed`。
    pub blip: Option<String>,
    /// `a:blipFill/a:tile`：平铺而非拉伸。
    pub tile: bool,
}

/// `a:fillRef` / `a:lnRef`：主题样式引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRef {
    pub node: NodeId,
    /// `@idx`：0 表示「无」。
    pub idx: Option<i64>,
}

/// `wps:bodyPr`：文字框的内边距与对齐。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BodyPr {
    /// `@lIns` / `@tIns` / `@rIns` / `@bIns`（EMU）。缺省值由投影层按 OOXML 补。
    pub l_ins: Option<i64>,
    pub t_ins: Option<i64>,
    pub r_ins: Option<i64>,
    pub b_ins: Option<i64>,
    /// `@anchor`：`t` / `ctr` / `b`。
    pub anchor: Option<Anchor>,
    /// 有 `a:spAutoFit`：框高随文字自适应。
    pub auto_fit: bool,
}

/// `wps:bodyPr/@anchor`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Center,
    Bottom,
}

impl DrawingDisplay {
    /// 段落级图片：第一个 `pic:pic`。
    pub fn picture(&self) -> Option<&ImageDisplay> {
        self.pictures.first()
    }
}

/// `wp:extent` / `a:ext`：EMU 宽高。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub cx: i64,
    pub cy: i64,
}

/// `wp:docPr`：无障碍与标识信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocPr {
    pub id: Option<String>,
    pub name: Option<String>,
    /// `@descr`：替代文字。
    pub descr: Option<String>,
    pub title: Option<String>,
    pub hidden: bool,
}

/// `wp:anchor` 的锚定几何。布尔属性的缺省值按 ECMA-376。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorGeom {
    pub node: NodeId,
    /// 绘在正文文字下面（只影响绘制次序，不等于不绕排）。
    pub behind_doc: bool,
    /// `@allowOverlap`，缺省 `true`。
    pub allow_overlap: bool,
    pub locked: bool,
    /// `@layoutInCell`，缺省 `true`。
    pub layout_in_cell: bool,
    pub simple_pos: bool,
    /// `@relativeHeight` 原值。z 序是它减去 Word 的基数，换算在投影层。
    pub relative_height: Option<i64>,
    /// `@distT/@distB/@distL/@distR`（EMU）。
    pub dist: Dist,
    pub h: Position,
    pub v: Position,
    pub wrap: Wrap,
}

/// 绕排边距（EMU）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dist {
    pub top: Option<i64>,
    pub bottom: Option<i64>,
    pub left: Option<i64>,
    pub right: Option<i64>,
}

/// `wp:positionH` / `wp:positionV`。三种定位写法互斥，但畸形文档可能都写，全都记下来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    /// `@relativeFrom`（`margin` / `page` / `column` / `paragraph` / `line` …）。
    pub relative_from: Option<String>,
    /// `wp:align` 的文本（`left` / `center` / `right` / `top` / `bottom` / `inside` / `outside`）。
    pub align: Option<String>,
    /// `wp:posOffset`（EMU）。
    pub offset_emu: Option<i64>,
    /// `wp14:pctPosHOffset` / `wp14:pctPosVOffset`（千分之一百分比原值）。
    pub pct: Option<i64>,
}

/// 绕排方式（`wp:wrap*`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrap {
    /// `wp:wrapNone`：不绕排，浮在文字上/下。
    None,
    /// `wp:wrapSquare`，`@wrapText` 说文字走哪一侧。
    Square {
        text: Option<String>,
    },
    Tight {
        text: Option<String>,
    },
    Through {
        text: Option<String>,
    },
    TopAndBottom,
    /// 随文（`wp:inline`），或 anchor 里没写绕排元素。
    Unspecified,
}

impl Wrap {
    /// `@wrapText`（`bothSides` / `left` / `right` / `largest`）。
    pub fn text(&self) -> Option<&str> {
        match self {
            Wrap::Square { text } | Wrap::Tight { text } | Wrap::Through { text } => {
                text.as_deref()
            }
            _ => None,
        }
    }
}

/// `pic:pic`：一张图片。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageDisplay {
    /// `pic:pic` 节点。
    pub node: Option<NodeId>,
    /// `pic:spPr/a:xfrm` 的 `a:off` / `a:ext`（EMU）。组里的图片靠它定位。
    pub off: Option<(i64, i64)>,
    pub ext: Option<Extent>,
    /// 所属 `wpg` 组在 `shapes` 里的下标。
    pub group: Option<usize>,
    /// `a:blip/@r:embed`：包内媒体的关系 id。
    pub embed: Option<String>,
    /// `a:blip/@r:link`：外链媒体的关系 id。
    pub link: Option<String>,
    /// `a:srcRect`：源图裁剪，四边各千分之一百分比。
    pub crop: Option<RectFrac>,
    /// `a:stretch/a:fillRect`：填充矩形。
    pub fill_rect: Option<RectFrac>,
    /// `pic:spPr/a:xfrm/@rot`，1/60000 度原值。
    pub rot_60k: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    /// `pic:spPr/a:ln`：图片边框。
    pub border: Option<LineDisplay>,
}

/// `a:srcRect` / `a:fillRect` 的四边，千分之一百分比原值（`10000` = 10%）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RectFrac {
    pub l: i64,
    pub t: i64,
    pub r: i64,
    pub b: i64,
}

impl RectFrac {
    pub fn is_zero(&self) -> bool {
        self.l == 0 && self.t == 0 && self.r == 0 && self.b == 0
    }
}

/// `a:ln`：线条。颜色留原始定义，解析成 sRGB 走 [`crate::resolve::drawingml`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDisplay {
    pub node: NodeId,
    /// `@w`（EMU）。
    pub width_emu: Option<i64>,
    /// `a:noFill` → 没有线。
    pub no_fill: bool,
    /// 颜色容器节点（`a:solidFill` 等），供 `resolve::drawingml::color_in`。
    pub fill: Option<NodeId>,
    /// `a:prstDash/@val`。
    pub dash: Option<String>,
    /// `a:headEnd/@type` / `a:tailEnd/@type`（`none` 视为没有箭头）。
    pub head_end: Option<String>,
    pub tail_end: Option<String>,
}

impl LineDisplay {
    /// 两端任一有箭头。
    pub fn arrowed(&self) -> bool {
        [&self.head_end, &self.tail_end].iter().any(|e| e.as_deref().is_some_and(|t| t != "none"))
    }
}

// ---- 解析 ---------------------------------------------------------------------------------------

/// 建一个 `w:drawing` 的显示模型。
pub fn drawing_display(dom: &Dom, drawing: NodeId) -> DrawingDisplay {
    let mut d = DrawingDisplay {
        node: drawing,
        kind: DrawingKind::Unknown,
        anchor: None,
        extent: None,
        doc_pr: DocPr::default(),
        pictures: Vec::new(),
        shapes: Vec::new(),
        chart: None,
        diagram: None,
        canvas: None,
    };
    let mut pic_nodes: Vec<(NodeId, Option<usize>)> = Vec::new();
    // 组的下标要在遍历时跟着走，所以这里用带父组的显式栈，而不是 `walk`。
    let mut stack: Vec<(NodeId, u32, Option<usize>)> = vec![(drawing, 0, None)];
    let mut scratch: Vec<NodeId> = Vec::new();
    while let Some((n, depth, parent)) = stack.pop() {
        let mut group = parent;
        if let Some(name) = dom.name(n) {
            match (eff_ns(dom, n), name.local) {
                (NsId::Wp, LocalName::Anchor) => d.anchor = Some(anchor_geom(dom, n)),
                (NsId::Wp, LocalName::Extent) if d.extent.is_none() => d.extent = extent_of(dom, n),
                (NsId::Wp, LocalName::DocPr) => d.doc_pr = doc_pr(dom, n),
                (NsId::A, LocalName::GraphicData) if d.kind == DrawingKind::Unknown => {
                    d.kind = crate::model::graphic_data_kind(dom, n);
                }
                (NsId::Pic, LocalName::Pic) => pic_nodes.push((n, parent)),
                (ns @ (NsId::C | NsId::Cx), LocalName::Chart) if d.chart.is_none() => {
                    d.chart = Some(ChartRef {
                        node: n,
                        rel_id: attr(dom, n, NsId::R, LocalName::Id),
                        chartex: ns == NsId::Cx,
                    });
                }
                (NsId::Dgm, LocalName::RelIds) if d.diagram.is_none() => {
                    d.diagram =
                        Some(DiagramRef { node: n, rel_id: attr(dom, n, NsId::R, LocalName::Dm) });
                }
                (NsId::Lc, LocalName::LockedCanvas) if d.canvas.is_none() => {
                    d.canvas = Some(Box::new(canvas_display(dom, n)));
                }
                (NsId::Wps, LocalName::Wsp) => d.shapes.push(shape_display(dom, n, false, parent)),
                (NsId::Wpg, LocalName::Wgp | LocalName::GrpSp) => {
                    d.shapes.push(shape_display(dom, n, true, parent));
                    group = Some(d.shapes.len() - 1);
                }
                // 绘图画布（真实 Word 的 `wpc:wpc`）：子形状的 `a:off` 以画布左上为原点、单位就是 EMU，
                // 等价于一个 `chOff = 0`、`chExt = ext = wp:extent` 的组（`corpus/real/canvas-*`）
                (NsId::Wpc, LocalName::Wpc) => {
                    let mut g = shape_display(dom, n, true, parent);
                    g.off = Some((0, 0));
                    g.ch_off = Some((0, 0));
                    g.ext = d.extent;
                    g.ch_ext = d.extent;
                    d.shapes.push(g);
                    group = Some(d.shapes.len() - 1);
                }
                _ => {}
            }
        }
        if depth < DRAWING_MAX_DEPTH {
            scratch.clear();
            scratch.extend(dom.semantic_children(n).filter(|&c| !is_own_flow(dom, c)));
            stack.extend(scratch.iter().rev().map(|&c| (c, depth + 1, group)));
        }
    }
    for (pic, group) in pic_nodes {
        let mut img = image_display(dom, pic);
        img.group = group;
        d.pictures.push(img);
    }
    d
}

/// `wps:wsp` / `wpg:wgp` / `wpg:grpSp` → [`ShapeDisplay`]。只看形状自己的属性子树，
/// 不下钻进 `txbxContent`（框里的内容是独立内容流）。
fn shape_display(dom: &Dom, node: NodeId, is_group: bool, group: Option<usize>) -> ShapeDisplay {
    let mut s = ShapeDisplay {
        node,
        is_group,
        cnv_id: None,
        prst: None,
        cust_geom: false,
        geom: None,
        ext: None,
        off: None,
        ch_off: None,
        ch_ext: None,
        rot_60k: None,
        flip_h: false,
        flip_v: false,
        fill: None,
        line: None,
        fill_ref: None,
        line_ref: None,
        font_ref: None,
        has_effects: false,
        body: None,
        txbx: None,
        txbx_rel: None,
        content: Vec::new(),
        content_part: None,
        group,
    };
    // 只走形状自己的属性容器：`spPr` / `grpSpPr` / `style` / `bodyPr` / `txbx`。
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        match (eff_ns(dom, c), name.local) {
            (NsId::Wps, LocalName::SpPr) | (NsId::Wpg, LocalName::GrpSpPr) => sp_pr(dom, c, &mut s),
            (NsId::Wps, LocalName::Style) => {
                for r in dom.semantic_children(c) {
                    let Some(n) = dom.name(r) else { continue };
                    let sr = StyleRef { node: r, idx: num(dom, r, LocalName::Idx) };
                    match (n.ns, n.local) {
                        (NsId::A, LocalName::FillRef) => s.fill_ref = Some(sr),
                        (NsId::A, LocalName::LnRef) => s.line_ref = Some(sr),
                        (NsId::A, LocalName::FontRef) => s.font_ref = Some(sr),
                        _ => {}
                    }
                }
            }
            (NsId::Wps, LocalName::BodyPr) => s.body = Some(body_pr(dom, c)),
            (NsId::Wps, LocalName::CNvPr) => s.cnv_id = attr(dom, c, NsId::None, LocalName::Id),
            (NsId::Wps, LocalName::Txbx) => {
                s.txbx_rel =
                    dom.attr_value(c, QName::new(NsId::R, LocalName::Txbx)).map(|v| v.into_owned());
                s.txbx = dom
                    .semantic_children(c)
                    .find(|&t| dom.is(t, QName::new(NsId::W, LocalName::TxbxContent)));
            }
            _ => {}
        }
    }
    s
}

fn sp_pr(dom: &Dom, sp_pr: NodeId, s: &mut ShapeDisplay) {
    for c in dom.semantic_children(sp_pr) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::Xfrm => {
                s.rot_60k = num(dom, c, LocalName::Rot);
                s.flip_h = drawing_flag(dom, c, LocalName::FlipH).unwrap_or(false);
                s.flip_v = drawing_flag(dom, c, LocalName::FlipV).unwrap_or(false);
                for g in dom.semantic_children(c) {
                    let Some(gn) = dom.name(g) else { continue };
                    match gn.local {
                        LocalName::Off => s.off = xy(dom, g),
                        LocalName::Ext => s.ext = extent_of(dom, g),
                        LocalName::ChOff => s.ch_off = xy(dom, g),
                        LocalName::ChExt => s.ch_ext = extent_of(dom, g),
                        _ => {}
                    }
                }
            }
            LocalName::PrstGeom => s.prst = attr(dom, c, NsId::None, LocalName::Prst),
            LocalName::CustGeom => {
                s.cust_geom = true;
                s.geom = custom_geom(dom, c);
            }
            LocalName::GrpFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Group, blip: None, tile: false })
            }
            LocalName::NoFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::None, blip: None, tile: false })
            }
            LocalName::SolidFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Solid, blip: None, tile: false })
            }
            LocalName::GradFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Gradient, blip: None, tile: false })
            }
            LocalName::PattFill if s.fill.is_none() => {
                s.fill =
                    Some(FillDisplay { node: c, kind: FillKind::Pattern, blip: None, tile: false })
            }
            LocalName::BlipFill if s.fill.is_none() => {
                let mut blip = None;
                let mut tile = false;
                for b in dom.semantic_children(c) {
                    match dom.name(b).map(|n| n.local) {
                        Some(LocalName::Blip) => blip = attr(dom, b, NsId::R, LocalName::Embed),
                        Some(LocalName::Tile) => tile = true,
                        _ => {}
                    }
                }
                s.fill = Some(FillDisplay { node: c, kind: FillKind::Blip, blip, tile });
            }
            LocalName::Ln if s.line.is_none() => s.line = Some(line_display(dom, c)),
            LocalName::EffectLst => {
                s.has_effects = dom.semantic_children(c).next().is_some();
            }
            _ => {}
        }
    }
}

fn body_pr(dom: &Dom, node: NodeId) -> BodyPr {
    BodyPr {
        l_ins: num(dom, node, LocalName::LIns),
        t_ins: num(dom, node, LocalName::TIns),
        r_ins: num(dom, node, LocalName::RIns),
        b_ins: num(dom, node, LocalName::BIns),
        anchor: match attr(dom, node, NsId::None, LocalName::Anchor).as_deref() {
            Some("t") => Some(Anchor::Top),
            Some("ctr") => Some(Anchor::Center),
            Some("b") => Some(Anchor::Bottom),
            _ => None,
        },
        auto_fit: dom
            .semantic_children(node)
            .any(|c| dom.is(c, QName::new(NsId::A, LocalName::SpAutoFit))),
    }
}

pub(crate) fn xy(dom: &Dom, node: NodeId) -> Option<(i64, i64)> {
    Some((num(dom, node, LocalName::X)?, num(dom, node, LocalName::Y)?))
}

/// `pic:pic` → [`ImageDisplay`]。
pub fn image_display(dom: &Dom, pic: NodeId) -> ImageDisplay {
    let mut img = ImageDisplay { node: Some(pic), ..ImageDisplay::default() };
    let mut in_sp_pr = false;
    for n in walk(dom, pic) {
        let Some(name) = dom.name(n) else { continue };
        match (eff_ns(dom, n), name.local) {
            (NsId::Pic, LocalName::SpPr) => in_sp_pr = true,
            (NsId::A, LocalName::Blip) if img.embed.is_none() && img.link.is_none() => {
                img.embed = attr(dom, n, NsId::R, LocalName::Embed);
                img.link = attr(dom, n, NsId::R, LocalName::Link);
            }
            (NsId::A, LocalName::SrcRect) if img.crop.is_none() => {
                img.crop = Some(rect_frac(dom, n));
            }
            (NsId::A, LocalName::FillRect) if img.fill_rect.is_none() => {
                img.fill_rect = Some(rect_frac(dom, n));
            }
            // 旋转与翻转只认 `pic:spPr` 自己的 `a:xfrm`：锚定文本框兄弟有它自己的 `wps` xfrm。
            (NsId::A, LocalName::Xfrm) if in_sp_pr && img.rot_60k.is_none() => {
                img.rot_60k = num(dom, n, LocalName::Rot);
                img.flip_h = drawing_flag(dom, n, LocalName::FlipH).unwrap_or(false);
                img.flip_v = drawing_flag(dom, n, LocalName::FlipV).unwrap_or(false);
                for g in dom.semantic_children(n) {
                    match dom.name(g).map(|q| q.local) {
                        Some(LocalName::Off) => img.off = xy(dom, g),
                        Some(LocalName::Ext) => img.ext = extent_of(dom, g),
                        _ => {}
                    }
                }
            }
            (NsId::A, LocalName::Ln) if in_sp_pr && img.border.is_none() => {
                img.border = Some(line_display(dom, n));
            }
            _ => {}
        }
    }
    img
}

/// `a:ln` → [`LineDisplay`]。
pub fn line_display(dom: &Dom, ln: NodeId) -> LineDisplay {
    let mut l = LineDisplay {
        node: ln,
        width_emu: num(dom, ln, LocalName::W),
        no_fill: false,
        fill: None,
        dash: None,
        head_end: None,
        tail_end: None,
    };
    for c in dom.semantic_children(ln) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::A {
            continue;
        }
        match name.local {
            LocalName::NoFill => l.no_fill = true,
            LocalName::PrstDash => l.dash = attr(dom, c, NsId::None, LocalName::Val),
            // 注意是小写的 `type`（`LocalName::Type`）；大写 `Type` 是 `UType`
            LocalName::HeadEnd => l.head_end = attr(dom, c, NsId::None, LocalName::Type),
            LocalName::TailEnd => l.tail_end = attr(dom, c, NsId::None, LocalName::Type),
            LocalName::SolidFill | LocalName::GradFill | LocalName::PattFill
                if l.fill.is_none() =>
            {
                l.fill = Some(c);
            }
            _ => {}
        }
    }
    l
}

fn anchor_geom(dom: &Dom, anchor: NodeId) -> AnchorGeom {
    let mut g = AnchorGeom {
        node: anchor,
        behind_doc: drawing_flag(dom, anchor, LocalName::BehindDoc).unwrap_or(false),
        allow_overlap: drawing_flag(dom, anchor, LocalName::AllowOverlap).unwrap_or(true),
        locked: drawing_flag(dom, anchor, LocalName::Locked).unwrap_or(false),
        layout_in_cell: drawing_flag(dom, anchor, LocalName::LayoutInCell).unwrap_or(true),
        simple_pos: drawing_flag(dom, anchor, LocalName::SimplePos).unwrap_or(false),
        relative_height: num(dom, anchor, LocalName::RelativeHeight),
        dist: Dist {
            top: num(dom, anchor, LocalName::DistT),
            bottom: num(dom, anchor, LocalName::DistB),
            left: num(dom, anchor, LocalName::DistL),
            right: num(dom, anchor, LocalName::DistR),
        },
        h: Position::default(),
        v: Position::default(),
        wrap: Wrap::Unspecified,
    };
    // 只看 anchor 的直接子节点：位置与绕排是 anchor 自己的属性，图形内部的同名元素不算。
    for c in dom.semantic_children(anchor) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::Wp {
            continue;
        }
        match name.local {
            LocalName::PositionH => g.h = position(dom, c, LocalName::PctPosHOffset),
            LocalName::PositionV => g.v = position(dom, c, LocalName::PctPosVOffset),
            LocalName::WrapNone => g.wrap = Wrap::None,
            LocalName::WrapSquare => g.wrap = Wrap::Square { text: wrap_text(dom, c) },
            LocalName::WrapTight => g.wrap = Wrap::Tight { text: wrap_text(dom, c) },
            LocalName::WrapThrough => g.wrap = Wrap::Through { text: wrap_text(dom, c) },
            LocalName::WrapTopAndBottom => g.wrap = Wrap::TopAndBottom,
            _ => {}
        }
    }
    g
}

fn wrap_text(dom: &Dom, node: NodeId) -> Option<String> {
    attr(dom, node, NsId::None, LocalName::WrapText)
}

fn position(dom: &Dom, node: NodeId, pct: LocalName) -> Position {
    let mut p = Position {
        relative_from: attr(dom, node, NsId::None, LocalName::RelativeFrom),
        ..Position::default()
    };
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        match (name.ns, name.local) {
            (NsId::Wp, LocalName::Align) => p.align = text_of(dom, c),
            (NsId::Wp, LocalName::PosOffset) => {
                p.offset_emu = text_of(dom, c).and_then(|s| s.trim().parse().ok());
            }
            (NsId::Wp14, l) if l == pct => {
                p.pct = text_of(dom, c).and_then(|s| s.trim().parse().ok());
            }
            _ => {}
        }
    }
    p
}

fn doc_pr(dom: &Dom, node: NodeId) -> DocPr {
    DocPr {
        id: attr(dom, node, NsId::None, LocalName::Id),
        name: attr(dom, node, NsId::None, LocalName::Name),
        descr: attr(dom, node, NsId::None, LocalName::Descr),
        title: attr(dom, node, NsId::None, LocalName::Title),
        hidden: drawing_flag(dom, node, LocalName::Hidden).unwrap_or(false),
    }
}

pub(crate) fn extent_of(dom: &Dom, node: NodeId) -> Option<Extent> {
    Some(Extent { cx: num(dom, node, LocalName::Cx)?, cy: num(dom, node, LocalName::Cy)? })
}

pub(crate) fn rect_frac(dom: &Dom, node: NodeId) -> RectFrac {
    let side = |l: LocalName| num(dom, node, l).unwrap_or(0);
    RectFrac {
        l: side(LocalName::L),
        t: side(LocalName::T),
        r: side(LocalName::R),
        b: side(LocalName::B),
    }
}

pub(crate) fn attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.trim().to_string())
}

pub(crate) fn num(dom: &Dom, node: NodeId, local: LocalName) -> Option<i64> {
    attr(dom, node, NsId::None, local)?.parse().ok()
}

/// OOXML 布尔属性：`1` / `true` / `on` 为真，`0` / `false` / `off` 为假。
fn drawing_flag(dom: &Dom, node: NodeId, local: LocalName) -> Option<bool> {
    match attr(dom, node, NsId::None, local)?.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

pub(crate) fn text_of(dom: &Dom, node: NodeId) -> Option<String> {
    let mut s = String::new();
    for c in dom.semantic_children(node) {
        if let Some(t) = dom.text(c) {
            s.push_str(&t);
        }
    }
    (!s.is_empty()).then_some(s)
}

/// 绘图子树的语义前序遍历，遇到独立内容流（`w:txbxContent`）与嵌套 `w:drawing` 就不再下钻。
fn walk(dom: &Dom, root: NodeId) -> Walk<'_> {
    Walk { dom, stack: vec![(root, 0)], scratch: Vec::new() }
}

struct Walk<'a> {
    dom: &'a Dom,
    stack: Vec<(NodeId, u32)>,
    scratch: Vec<NodeId>,
}

impl Iterator for Walk<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let (id, depth) = self.stack.pop()?;
        if depth < DRAWING_MAX_DEPTH {
            let dom = self.dom;
            self.scratch.clear();
            self.scratch.extend(dom.semantic_children(id).filter(|&c| !is_own_flow(dom, c)));
            self.stack.extend(self.scratch.iter().rev().map(|&c| (c, depth + 1)));
        }
        Some(id)
    }
}

/// 该节点是否开启了一条独立内容流（不属于当前 drawing 的几何）。
/// 节点的**有效**命名空间：前缀绑不上时按字面量认。
///
/// 语料里有一批合成文档只在根上声明了 `w` / `wp` / `a` / `pic`，`wps` 与 `wpg` 一个都没声明
/// （`field-display__015`）。TS 用字符串匹配 `<wps:wsp`，压根不看声明；我们走 DOM，就得在
/// 这里补一条：绑不上的前缀按字面量认，其余照旧按 URI。
fn eff_ns(dom: &Dom, node: NodeId) -> NsId {
    let Some(name) = dom.name(node) else { return NsId::None };
    if !matches!(name.ns, NsId::Unbound(_)) {
        return name.ns;
    }
    match dom.lex_name(node).and_then(|q| q.split_once(':')).map(|(p, _)| p) {
        Some("wps") => NsId::Wps,
        Some("wpg") => NsId::Wpg,
        Some("wp") => NsId::Wp,
        Some("c") => NsId::C,
        Some("cx") => NsId::Cx,
        Some("dgm") => NsId::Dgm,
        Some("lc") => NsId::Lc,
        Some("pic") => NsId::Pic,
        Some("a") => NsId::A,
        _ => name.ns,
    }
}

fn is_own_flow(dom: &Dom, node: NodeId) -> bool {
    dom.name(node).is_some_and(|n| {
        n.ns == NsId::W && matches!(n.local, LocalName::TxbxContent | LocalName::Drawing)
    })
}

// `ParagraphFacts`（`MOD-04`，`docs/03` §6.2）：对 `w:p` 一次遍历得到的事实，分类（`MOD-05`）
// 与 `TextKind` 判定（`MOD-03`）都是它的纯函数。
//
// M1 范围：文本、sectPr、样式、编号、outline、公式与修订计数、绘图 / VML 的粗事实
// （种类按 `graphicData/@uri` 与 VML 子元素判定）。字段事实（`fields` / `inside_field_result`）在 M2。

use crate::xml::MceRole;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParagraphFacts {
    pub has_sect_pr: bool,
    /// 任一 `w:t`/`w:delText` trim 后非空（不含 `w:txbxContent` 内）。
    pub visible_text: bool,
    /// 同上，且排除所有绘图 / VML / 对象内容。
    pub visible_text_outside_boxes: bool,
    pub fields: Vec<FieldId>,
    pub inside_field_result: Option<FieldId>,
    pub drawings: Vec<DrawingFacts>,
    pub picts: Vec<PictFacts>,
    /// `w:object` 节点（文档序）。
    pub objects: Vec<NodeId>,
    pub math: MathFacts,
    pub revision: RevisionFacts,
    pub style_id: Option<String>,
    /// 样式链 `vanish == true` 且段落里没有把它关掉、没有必须显示的内容。
    pub style_vanish: bool,
    /// styleId 匹配 `^TOC ?([1-9])$`。
    pub toc_style_level: Option<u8>,
    pub numbering_ref: Option<ListRef>,
    /// `MOD-03`：直接 `outlineLvl` 0–8 → +1（9 → `None`，不再看样式）；否则样式链；否则 styleId 匹配。
    pub outline_level: Option<u8>,
    pub sdt: Option<SdtInfo>,
    /// 段落里有 `w:vanish w:val="0|false|off"`（把样式的隐藏关掉）。
    pub unvanish: bool,
    /// 段落里有书签起点或批注范围标记（TS `staysVanished` 的排除项）。
    pub has_range_marker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingFacts {
    pub node: NodeId,
    pub kind: DrawingKind,
    /// `wp:anchor`（否则 `wp:inline`）。
    pub anchored: bool,
    pub has_txbx_text: bool,
    pub has_blip: bool,
    /// `wp:docPr/@name` 以 `aidocs-ink` 开头。
    pub is_ink: bool,
    /// chartex 绘图配的预渲染图：本绘图在 `mc:Choice` 里，同一 `mc:AlternateContent` 的 `mc:Fallback`
    /// 里那个带 `a:blip` 的 `w:drawing`（R12 据此把段落归为图片；Word 2016+ 给每个 chartex 都配一张）。
    /// 只对 `ChartEx` 算；其他种类恒为 `None`。
    pub fallback_picture: Option<NodeId>,
}

named_enum! {
    /// 按 `a:graphicData/@uri` 判定；`@uri` 缺失时退回看 `a:graphicData` 的子元素命名空间。
    pub enum DrawingKind {
        Picture = "picture",
        Chart = "chart",
        ChartEx = "chartEx",
        Diagram = "diagram",
        LockedCanvas = "lockedCanvas",
        Shape = "shape",
        Group = "group",
        Line = "line",
        Unknown = "unknown",
    }
}

impl DrawingKind {
    pub fn from_uri(uri: &str) -> DrawingKind {
        match uri {
            "http://schemas.openxmlformats.org/drawingml/2006/picture" => DrawingKind::Picture,
            "http://schemas.openxmlformats.org/drawingml/2006/chart" => DrawingKind::Chart,
            "http://schemas.microsoft.com/office/drawing/2014/chartex" => DrawingKind::ChartEx,
            "http://schemas.openxmlformats.org/drawingml/2006/diagram" => DrawingKind::Diagram,
            "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas" => {
                DrawingKind::LockedCanvas
            }
            "http://schemas.microsoft.com/office/word/2010/wordprocessingShape" => {
                DrawingKind::Shape
            }
            "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" => {
                DrawingKind::Group
            }
            // 真实 Word 的绘图画布（`wpc:wpc`）：子形状与组一样按容器处理（`corpus/real/canvas-*`）
            "http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas" => {
                DrawingKind::Group
            }
            _ => DrawingKind::Unknown,
        }
    }

    /// `@uri` 缺失或不认识时的退路：看 `a:graphicData` 里放的是什么。
    ///
    /// 语料里有 `<a:graphicData><c:chart r:id=.../></a:graphicData>`（`resource-cleanup__008`）
    /// 这种不写 `@uri` 的写法；TS 是按 XML 里出现的 `<c:chart` / `r:dm=` / `<dgm:` 等标记判的，
    /// 所以它认得出来。只看命名空间，不看具体元素名。
    pub fn from_graphic_child_ns(ns: NsId) -> DrawingKind {
        match ns {
            NsId::Pic => DrawingKind::Picture,
            NsId::C => DrawingKind::Chart,
            NsId::Cx => DrawingKind::ChartEx,
            NsId::Dgm => DrawingKind::Diagram,
            NsId::Lc => DrawingKind::LockedCanvas,
            NsId::Wps => DrawingKind::Shape,
            NsId::Wpg | NsId::Wpc => DrawingKind::Group,
            _ => DrawingKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictFacts {
    pub node: NodeId,
    pub kind: PictKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictKind {
    ImageData,
    TextBox,
    WordArt,
    /// `v:rect[@o:hr]`。
    Hr,
    ShapeTypeOnly,
    /// `visibility:hidden`。
    Hidden,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathFacts {
    /// 段落直接内容里的 `m:oMath` 数（不含绘图 / 文本框内）。
    pub count: u32,
    pub omath_para: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionFacts {
    pub run_ins: bool,
    pub run_del: bool,
    pub move_from: bool,
    pub move_to: bool,
    pub del_instr_text: bool,
    pub para_mark_ins: bool,
    pub para_mark_del: bool,
    pub ppr_change: bool,
}

impl RevisionFacts {
    pub fn any(&self) -> bool {
        self.run_ins
            || self.run_del
            || self.move_from
            || self.move_to
            || self.del_instr_text
            || self.para_mark_ins
            || self.para_mark_del
            || self.ppr_change
    }
}

fn is_w(name: QName, local: LocalName) -> bool {
    name.ns == NsId::W && name.local == local
}

fn facts_attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.into_owned())
}

/// 元素下所有文本节点拼接后 trim 是否非空。
fn has_visible_text(dom: &Dom, node: NodeId) -> bool {
    dom.semantic_children(node)
        .any(|c| dom.text(c).is_some_and(|t| !t.trim_matches([' ', '\t', '\r', '\n']).is_empty()))
}

impl ParagraphFacts {
    /// 一次遍历；`props` 是已读出的 `w:pPr`（可缺）。
    pub fn compute(
        dom: &Dom,
        p: NodeId,
        props: &ParaProps,
        styles: Option<&Styles>,
        sdt: Option<SdtInfo>,
    ) -> ParagraphFacts {
        let mut f = ParagraphFacts { sdt, ..Default::default() };
        f.has_sect_pr = props.sect_pr.is_some();
        f.style_id = props.style.clone();
        f.toc_style_level = f.style_id.as_deref().and_then(toc_level_of_id);
        for &n in &props.raw_unmodeled {
            if dom.is(n, QName::w(LocalName::PPrChange)) {
                f.revision.ppr_change = true;
            }
        }
        if let Some(rpr) = &props.rpr {
            for &n in &rpr.raw_unmodeled {
                match dom.name(n) {
                    Some(q) if is_w(q, LocalName::Ins) => f.revision.para_mark_ins = true,
                    Some(q) if is_w(q, LocalName::Del) => f.revision.para_mark_del = true,
                    _ => {}
                }
            }
        }

        // 内容遍历：(节点, 在文本框内, 在绘图/VML/对象内)
        let mut unvanish = false;
        let mut has_marker = false;
        let mut stack: Vec<(NodeId, bool, bool)> = dom
            .semantic_children(p)
            .filter(|&c| !dom.is(c, QName::w(LocalName::PPr)))
            .map(|c| (c, false, false))
            .collect();
        stack.reverse();
        while let Some((node, in_txbx, in_gfx)) = stack.pop() {
            let Some(name) = dom.name(node) else {
                continue;
            };
            let (mut txbx, mut gfx) = (in_txbx, in_gfx);
            match (name.ns, name.local) {
                (NsId::W, LocalName::T | LocalName::DelText) => {
                    if !in_txbx && has_visible_text(dom, node) {
                        f.visible_text = true;
                        if !in_gfx {
                            f.visible_text_outside_boxes = true;
                        }
                    }
                    continue;
                }
                (NsId::W, LocalName::TxbxContent) => txbx = true,
                (NsId::W, LocalName::Drawing) if !in_gfx => {
                    let df = drawing_facts(dom, node);
                    // 墨迹对分类不可见（TS 在 detect 前 `stripInkRuns`）：被批注的段落仍是可编辑正文
                    if !df.is_ink {
                        f.drawings.push(df);
                    }
                    gfx = true;
                }
                (NsId::W, LocalName::Pict) if !in_gfx => {
                    f.picts.push(PictFacts { node, kind: pict_kind(dom, node) });
                    gfx = true;
                }
                (NsId::W, LocalName::Object) if !in_gfx => {
                    f.objects.push(node);
                    gfx = true;
                }
                (NsId::M, LocalName::OMath) if !in_gfx && !in_txbx => f.math.count += 1,
                (NsId::M, LocalName::OMathPara) if !in_gfx && !in_txbx => f.math.omath_para = true,
                (NsId::W, LocalName::Ins) => f.revision.run_ins = true,
                (NsId::W, LocalName::Del) => f.revision.run_del = true,
                (NsId::W, LocalName::MoveFrom) => f.revision.move_from = true,
                (NsId::W, LocalName::MoveTo) => f.revision.move_to = true,
                (NsId::W, LocalName::DelInstrText) => f.revision.del_instr_text = true,
                (NsId::W, LocalName::Vanish) if !in_txbx => {
                    if let Some(v) = facts_attr(dom, node, NsId::W, LocalName::Val)
                        && matches!(v.trim(), "0" | "false" | "off")
                    {
                        unvanish = true;
                    }
                }
                (
                    NsId::W,
                    LocalName::BookmarkStart
                    | LocalName::CommentRangeStart
                    | LocalName::CommentRangeEnd,
                ) => {
                    has_marker = true;
                }
                _ => {}
            }
            for &c in dom.children(node).iter().rev() {
                stack.push((c, txbx, gfx));
            }
        }

        f.unvanish = unvanish;
        f.has_range_marker = has_marker;
        // MOD-03：编号与标题级别
        let chain: Vec<&Style> = match (styles, f.style_id.as_deref()) {
            (Some(s), Some(id)) => s.chain(id, StyleType::Paragraph),
            _ => Vec::new(),
        };
        f.numbering_ref = list_ref(props, &chain);
        f.outline_level = outline_level(props, &chain, f.style_id.as_deref());
        // style_vanish（TS `staysVanished`）
        // 无 pStyle 时看默认段落样式链（TS `defaultParaVanish`）
        let vanish_chain: Vec<&Style> = if f.style_id.is_none() {
            styles
                .and_then(|s| {
                    s.default_for(StyleType::Paragraph)
                        .and_then(Style::id)
                        .map(|id| s.chain(id, StyleType::Paragraph))
                })
                .unwrap_or_default()
        } else {
            chain.clone()
        };
        let chain_vanish = vanish_chain
            .iter()
            .find_map(|s| s.rpr.as_ref().and_then(|r| r.vanish))
            .unwrap_or(false);
        f.style_vanish = chain_vanish
            && !unvanish
            && !has_marker
            && f.drawings.is_empty()
            && f.picts.is_empty()
            && f.objects.is_empty()
            && !f.has_sect_pr
            && props.num.is_none();
        f
    }
}

/// `MOD-03` `ListRef`：直接 `w:numPr`（numId 0 → 无编号；无 numId 时用样式链的，ilvl 缺省用样式的再缺省 0）。
fn list_ref(props: &ParaProps, chain: &[&Style]) -> Option<ListRef> {
    let direct = props.num.as_ref();
    let direct_num = direct.and_then(|n| n.num_id.as_ref()).and_then(|v| v.value().copied());
    let direct_ilvl = direct.and_then(|n| n.ilvl.as_ref()).and_then(|v| v.value().copied());
    if let Some(id) = direct_num {
        if id == 0 {
            return None;
        }
        let ilvl = direct_ilvl.or_else(|| style_list(chain).map(|(_, l)| l)).unwrap_or(0);
        return Some(ListRef { num_id: id, ilvl, from_style: false });
    }
    let (num_id, style_ilvl) = style_list(chain)?;
    Some(ListRef { num_id, ilvl: direct_ilvl.unwrap_or(style_ilvl), from_style: true })
}

/// 样式链上第一个 `numPr`：numId 0 为显式取消（返回 `None`）。
fn style_list(chain: &[&Style]) -> Option<(i32, i32)> {
    for s in chain {
        let Some(num) = s.ppr.as_ref().and_then(|p| p.num.as_ref()) else { continue };
        let id = num.num_id.as_ref().and_then(|v| v.value().copied());
        match id {
            Some(0) => return None,
            Some(id) => {
                let ilvl = num.ilvl.as_ref().and_then(|v| v.value().copied()).unwrap_or(0);
                return Some((id, ilvl));
            }
            None => continue,
        }
    }
    None
}

/// `MOD-03` 标题级别。
fn outline_level(props: &ParaProps, chain: &[&Style], style_id: Option<&str>) -> Option<u8> {
    if let Some(Val::Value(l)) = &props.outline_lvl {
        return match *l {
            0..=8 => Some(*l as u8 + 1),
            _ => None,
        };
    }
    heading_level_of_chain(chain, style_id)
}

/// 样式链（叶 → 根）给出的标题级别（`RES-02` heading_level）：叶起第一个 `Level`；`Blocked` 终止；
/// 链为空时按文档未定义的内建样式 id `^Heading([1-9])$`（忽略大小写）。
pub fn heading_level_of_chain(chain: &[&Style], style_id: Option<&str>) -> Option<u8> {
    for s in chain {
        match Styles::own_heading_level(s) {
            OwnHeadingLevel::Level(l) => return Some(l),
            OwnHeadingLevel::Blocked => return None,
            OwnHeadingLevel::Inherit => {}
        }
    }
    if chain.is_empty() {
        let id = style_id?;
        let rest = id.get(..7).filter(|p| p.eq_ignore_ascii_case("heading")).map(|_| &id[7..])?;
        let mut it = rest.chars();
        let d = it.next()?;
        if it.next().is_none() && d.is_ascii_digit() && d != '0' {
            return Some(d as u8 - b'0');
        }
    }
    None
}

/// `^TOC ?([1-9])$`
/// `MOD-04` 的 `toc_style_level`：`TOC1` / `TOC 1` → 级别；图表目录 / 引文目录样式 → 1。
///
/// 后一类（`TableofFigures` / `TableofAuthorities`，Word 的"图表目录""引文目录"）也是目录行，
/// TS 同样给它们 `TOC entry` + `tocLine`（语料 `field-display__010`）。
pub(crate) fn toc_level_of_id(id: &str) -> Option<u8> {
    let squashed: String = id.chars().filter(|c| !c.is_whitespace()).collect();
    if squashed.eq_ignore_ascii_case("TableofFigures")
        || squashed.eq_ignore_ascii_case("TableofAuthorities")
    {
        return Some(1);
    }
    let rest = id.strip_prefix("TOC")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let mut it = rest.chars();
    let d = it.next()?;
    if it.next().is_some() || !d.is_ascii_digit() || d == '0' {
        return None;
    }
    Some(d as u8 - b'0')
}

fn drawing_facts(dom: &Dom, drawing: NodeId) -> DrawingFacts {
    let mut f = DrawingFacts {
        node: drawing,
        kind: DrawingKind::Unknown,
        anchored: false,
        has_txbx_text: false,
        has_blip: false,
        is_ink: false,
        fallback_picture: None,
    };
    for n in dom.descendants(drawing) {
        let Some(name) = dom.name(n) else { continue };
        match (name.ns, name.local) {
            (NsId::Wp, LocalName::Anchor) => f.anchored = true,
            (NsId::Wp, LocalName::DocPr) => {
                if facts_attr(dom, n, NsId::None, LocalName::Name)
                    .is_some_and(|s| s.starts_with("aidocs-ink"))
                {
                    f.is_ink = true;
                }
            }
            (NsId::A, LocalName::GraphicData) if f.kind == DrawingKind::Unknown => {
                f.kind = graphic_data_kind(dom, n);
            }
            (NsId::A, LocalName::Blip) => f.has_blip = true,
            (NsId::W, LocalName::T) if has_visible_text(dom, n) => f.has_txbx_text = true,
            _ => {}
        }
    }
    if f.kind == DrawingKind::ChartEx {
        f.fallback_picture = fallback_picture(dom, drawing);
    }
    // TS 只认 `wp:anchor` 形态的墨迹 run（`stripInkRuns` 的正则）
    f.is_ink &= f.anchored;
    f
}

/// 承载 `drawing` 的 `mc:Choice` 的兄弟 `mc:Fallback` 里第一个带 `a:blip` 的 `w:drawing`。
///
/// Fallback 不是 active 分支，语义遍历看不见它，这里按原始子树找；往上找 Choice 时到段落就停
/// （`mc:AlternateContent` 只会出现在段落内容里）。
fn fallback_picture(dom: &Dom, drawing: NodeId) -> Option<NodeId> {
    let mut cur = dom.parent(drawing)?;
    let choice = loop {
        if dom.is(cur, QName::w(LocalName::P)) {
            return None;
        }
        match dom.element(cur)?.mce.role {
            MceRole::Choice => break cur,
            MceRole::AlternateContent | MceRole::Fallback => return None,
            MceRole::None => cur = dom.parent(cur)?,
        }
    };
    let alternate = dom.parent(choice)?;
    let fallback = dom
        .children(alternate)
        .iter()
        .copied()
        .find(|&c| dom.element(c).is_some_and(|e| e.mce.role == MceRole::Fallback))?;
    let blip = QName::new(NsId::A, LocalName::Blip);
    dom.descendants(fallback).find(|&n| {
        dom.is(n, QName::w(LocalName::Drawing)) && dom.descendants(n).any(|b| dom.is(b, blip))
    })
}

/// `a:graphicData` 的种类：`@uri` 优先，缺失或不认识时看第一个子元素的命名空间。
pub(crate) fn graphic_data_kind(dom: &Dom, graphic_data: NodeId) -> DrawingKind {
    if let Some(uri) = facts_attr(dom, graphic_data, NsId::None, LocalName::Uri) {
        let kind = DrawingKind::from_uri(&uri);
        if kind != DrawingKind::Unknown {
            return kind;
        }
    }
    dom.semantic_children(graphic_data)
        .map(|c| graphic_child_kind(dom, c))
        .find(|&k| k != DrawingKind::Unknown)
        .unwrap_or(DrawingKind::Unknown)
}

/// `a:graphicData` 的一个子元素说明这是什么图。
fn graphic_child_kind(dom: &Dom, child: NodeId) -> DrawingKind {
    let Some(name) = dom.name(child) else { return DrawingKind::Unknown };
    let kind = DrawingKind::from_graphic_child_ns(name.ns);
    if kind != DrawingKind::Unknown {
        return kind;
    }
    // 前缀未绑定（已记 `XML_UNBOUND_PREFIX`）时按前缀字面量兜底。语料里有既不写 `@uri`
    // 也不声明 `c` / `dgm` / `wps` 前缀的文档（`resource-cleanup__008`、`field-display__015`），
    // 这时字面量是唯一还剩的信息；宁可按它分类，也好过整段降级成"认不出的绘图"。
    if !matches!(name.ns, NsId::Unbound(_)) {
        return DrawingKind::Unknown;
    }
    match dom.lex_name(child).and_then(|q| q.split_once(':')).map(|(p, _)| p) {
        Some("pic") => DrawingKind::Picture,
        Some("c") => DrawingKind::Chart,
        Some("cx") => DrawingKind::ChartEx,
        Some("dgm") => DrawingKind::Diagram,
        Some("lc") => DrawingKind::LockedCanvas,
        Some("wps") => DrawingKind::Shape,
        Some("wpg") | Some("wpc") => DrawingKind::Group,
        _ => DrawingKind::Unknown,
    }
}

/// 一个 `w:pict` 是哪一类（`MOD-05` 的 R15–R18 要用）。
///
/// 优先级照抄 TS 的 `w:pict` 决策树，**不是**文档序：文本框 / WordArt → 图片 → 隐藏形状 →
/// 细横线。一个画布里既有 `v:imagedata` 又有 `v:textbox` 时（`wordart-vml__015`），它是文本框
/// 而不是图片——按文档序谁先谁赢的话，同一份文档换个形状顺序就换一种分类。
fn pict_kind(dom: &Dom, pict: NodeId) -> PictKind {
    let mut only_shapetype = true;
    for c in dom.semantic_children(pict) {
        let Some(name) = dom.name(c) else { continue };
        if !(name.ns == NsId::V && name.local == LocalName::Shapetype) {
            only_shapetype = false;
        }
    }
    if only_shapetype && dom.semantic_children(pict).next().is_some() {
        return PictKind::ShapeTypeOnly;
    }
    let (mut textbox, mut wordart, mut image, mut hidden, mut hr) =
        (false, false, false, false, false);
    for n in dom.descendants(pict) {
        let Some(name) = dom.name(n) else { continue };
        match (name.ns, name.local) {
            (NsId::V, LocalName::Imagedata) => image = true,
            (NsId::V, LocalName::Textbox) => textbox = true,
            (NsId::V, LocalName::Textpath)
                if facts_attr(dom, n, NsId::None, LocalName::String).is_some() =>
            {
                wordart = true;
            }
            (NsId::V, LocalName::Rect)
                if dom.attr(n, QName::new(NsId::O, LocalName::Hr)).is_some() =>
            {
                hr = true;
            }
            (
                NsId::V,
                LocalName::Shape
                | LocalName::Rect
                | LocalName::Oval
                | LocalName::Roundrect
                | LocalName::Line,
            ) if facts_attr(dom, n, NsId::None, LocalName::Style)
                .is_some_and(|s| s.contains("visibility:hidden")) =>
            {
                hidden = true;
            }
            _ => {}
        }
    }
    match (textbox, wordart, image, hidden, hr) {
        (true, ..) => PictKind::TextBox,
        (_, true, ..) => PictKind::WordArt,
        (_, _, true, ..) => PictKind::ImageData,
        (_, _, _, true, _) => PictKind::Hidden,
        (.., true) => PictKind::Hr,
        _ => PictKind::Other,
    }
}

impl Styles {
    /// basedOn 链（叶 → 根），带环检测；类型不一致的 basedOn 视为链结束（`RES-02`）。
    pub fn chain(&self, id: &str, kind: StyleType) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(id) = cur {
            let Some(s) = self.get(&id) else { break };
            if s.kind() != Some(kind)
                || out.iter().any(|x: &&Style| std::ptr::eq(*x, s))
                || out.len() > 64
            {
                break;
            }
            out.push(s);
            cur = s.based_on.clone();
        }
        out
    }
}

// 页眉页脚 part 的内容流（`MOD-01` 的 `hf_parts`、`docs/03` §6.7，`spec/16` 任务 5.3）。
//
// **复用正文管线**：`w:hdr` / `w:ftr` 里的段落、表格、sdt、修订包裹、文本框都走同一个
// `Builder::build_container`，所以 `Block` / `Inline` / 显示模型的形状与正文完全一样，
// 编辑操作也就不需要为页眉另写一份。
//
// 每个 part 自带三份索引（`FlowMap` / `FieldIndex` / `SpanIndex`）。它们都是**按 part** 的：
// `NodeId` 相对该 part 自己的 DOM，`FlowId` 也只在该 part 内有意义（`SPAN-01`：`w:hdr` / `w:ftr`
// 各是一个独立内容流，范围与字段禁止跨流，也就禁止跨 part）。
//
// `has_page_number` / `has_num_pages` 由该 part 的字段索引推导（`FLD-11`），不是扫字符串：
// `PAGE` / `NUMPAGES` 是原子字段，渲染器看到 `Keyword::Page` 自己替换页码
// （`docs/03` §5.4 末段——所以模型里没有 `PAGE_MARK` 这类占位符，那只存在于 `compat_ts`）。

use crate::diag::DiagCode;

use crate::span::field::Keyword;
use crate::xml::{LocalName, NsId};

/// 一个页眉或页脚 part。
#[derive(Debug, Clone, PartialEq)]
pub struct HfPart {
    pub part: PartId,
    pub kind: HfKind,
    /// `w:hdr` / `w:ftr`。
    pub root: NodeId,
    /// 内容，与正文同一构建器（`docs/03` §6.7）。
    pub blocks: Vec<Block>,
    /// 本 part 的三份索引（`SPAN-01` / `FLD-02` / `SPAN-04`）。
    pub idx: AuxFlows,
    /// 含 `PAGE` 字段（`FLD-11`）或旧式 `w:pgNum` 元素。
    pub has_page_number: bool,
    /// 含 `NUMPAGES` 字段。
    pub has_num_pages: bool,
    /// 文字水印：第一个 `v:textpath/@string`（Word 的水印是页眉里的 VML 形状）。
    /// 页脚里也照读——判"只有页眉算水印"是投影层的事（`COMPAT-05`）。
    pub watermark: Option<String>,
}

impl HfPart {
    /// 解析一个页眉页脚 part。根不是 `w:hdr` / `w:ftr` → `None` + 一条诊断
    /// （part 本身解析不了的情况在 `Package::dom` 就降级成 `Opaque` 了，走不到这里）。
    pub fn build(
        part: PartId,
        dom: &Dom,
        kind: HfKind,
        styles: Option<&Styles>,
        rels: &Rels,
        warnings: &mut Vec<Diagnostic>,
    ) -> Option<HfPart> {
        let root = dom.root();
        let (want, want_name) = match kind {
            HfKind::Header => (LocalName::Hdr, "w:hdr"),
            HfKind::Footer => (LocalName::Ftr, "w:ftr"),
        };
        if !dom.is(root, QName::w(want)) {
            warnings.push(Diagnostic::pre_existing(
                part,
                dom.node(root).lex.as_ref().map(|l| l.range.clone()),
                DiagCode::ModUnparseable,
                format!("{} part 的根不是 {want_name}", kind.as_str()),
            ));
            return None;
        }
        let idx = AuxFlows::build(part, dom, warnings);
        let blocks = idx.blocks_of(dom, rels, styles, root, warnings);
        let has = |k: &Keyword| idx.fields.fields().iter().any(|f| f.keyword() == k);
        // `w:pgNum` 是 Word 6.0/95 的旧式页码：一个 run 子元素，不是字段，但语义就是"这里放页码"
        // （TS `hfContentFromXml` 把它换成 `PAGE_MARK` 并置 `hasPageNumber`）。
        // 坐标流里它是 `SegmentKind::Other`，与原子字段一样占 1 个单位，所以偏移不受影响。
        let legacy_pg_num =
            dom.semantic_descendants(root).any(|n| dom.is(n, QName::w(LocalName::PgNum)));
        Some(HfPart {
            part,
            kind,
            root,
            blocks,
            has_page_number: has(&Keyword::Page) || legacy_pg_num,
            has_num_pages: has(&Keyword::NumPages),
            watermark: watermark_of(dom),
            idx,
        })
    }

    /// 全部文本块（与 `Document::text_blocks` 同义，只是限在本 part）。
    pub fn text_blocks(&self) -> impl Iterator<Item = &crate::model::TextBlock> {
        self.blocks.iter().filter_map(Block::as_text)
    }
}

/// 第一个 `v:textpath/@string`（无前缀属性）。实体在读取时已解码一次（`XML-06`）。
///
/// 空串按"没有水印"处理（同 TS `readWatermarkText`：`return text || null`）。
fn watermark_of(dom: &Dom) -> Option<String> {
    dom.semantic_descendants(dom.root())
        .filter(|&n| dom.is(n, QName::new(NsId::V, LocalName::Textpath)))
        .find_map(|n| dom.attr_value(n, QName::new(NsId::None, LocalName::String)))
        .map(|v| v.into_owned())
        .filter(|s| !s.is_empty())
}

// 墨迹（`aidocs-ink` 批注层，`MOD-06` / `MOD-11`，`spec/17` 任务 6.8）。
//
// 编辑器把手绘笔迹存成**自己写进文档的浮动图片 run**：`w:r/w:drawing/wp:anchor`，`wp:docPr/@name` 以
// `aidocs-ink` 开头、`@descr` 带笔迹向量（不透明载荷）。Word 把它当普通浮动图片画；只有编辑器把它还原成
// 可再编辑的笔迹层。所以它对**分类与坐标流都不可见**（TS 在 `detect` 之前 `stripInkRuns`）：被批注的段落
// 仍是可编辑正文，`Run.segments` 里它是长度 0 的 [`SegmentKind::Ink`]；几何与载荷收在 [`InkInfo`]。

/// `wp:docPr/@name` 的前缀（TS `INK_NAME_PREFIX`）。
pub const INK_NAME_PREFIX: &str = "aidocs-ink";

/// 一条墨迹批注（TS `InkRunMatch` 的模型侧）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InkInfo {
    /// 锚定的段落（`w:p`，可能在单元格里）。
    pub para: NodeId,
    /// 承载它的 `w:r`（`RemoveInks` 删的就是它）。
    pub run: NodeId,
    /// `w:drawing`。
    pub drawing: NodeId,
    /// `wp:positionH` / `wp:positionV` 的 `wp:posOffset`（EMU；缺失或非数字 → 0，TS `parseInt || 0`）。
    pub offset_emu: (i64, i64),
    /// `wp:extent` 的 `cx` / `cy`（EMU；同上）。
    pub extent_emu: (i64, i64),
    /// 第一个 `a:blip/@r:embed`；没有 → `None`（compat 的 `dataUrl: null`）。
    pub rel_id: Option<String>,
    /// `wp:docPr/@descr` 解码后的载荷；缺失或空 → `None`（TS `descr ? … : null`）。
    pub payload: Option<String>,
}

/// TS `parseInt(s, 10) || 0`：可选正负号 + 前导十进制数字，其余忽略；没有数字 → 0。
pub fn lenient_int(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut v: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        v = v.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    if neg { -v } else { v }
}

fn wp(l: LocalName) -> QName {
    QName::new(NsId::Wp, l)
}

fn plain(l: LocalName) -> QName {
    QName::new(NsId::None, l)
}

/// 这个 `w:drawing` 是不是墨迹：`wp:anchor` 里有 `wp:docPr/@name` 以 [`INK_NAME_PREFIX`] 开头
/// （TS `stripInkRuns` / `findInkRuns` 只认 `wp:anchor` 形态的 run）。
pub fn is_ink_drawing(dom: &Dom, drawing: NodeId) -> bool {
    dom.semantic_children(drawing)
        .filter(|&c| dom.is(c, wp(LocalName::Anchor)))
        .any(|anchor| dom.semantic_children(anchor).any(|c| is_ink_doc_pr(dom, c)))
}

fn is_ink_doc_pr(dom: &Dom, node: NodeId) -> bool {
    dom.is(node, wp(LocalName::DocPr))
        && dom
            .attr_value(node, plain(LocalName::Name))
            .is_some_and(|v| v.starts_with(INK_NAME_PREFIX))
}

/// 读一条墨迹的几何与载荷（调用方已确认 [`is_ink_drawing`]）。
pub fn ink_info(dom: &Dom, para: NodeId, run: NodeId, drawing: NodeId) -> InkInfo {
    let mut info = InkInfo {
        para,
        run,
        drawing,
        offset_emu: (0, 0),
        extent_emu: (0, 0),
        rel_id: None,
        payload: None,
    };
    let Some(anchor) = dom.semantic_children(drawing).find(|&c| dom.is(c, wp(LocalName::Anchor)))
    else {
        return info;
    };
    let pos_offset = |n: NodeId| {
        dom.semantic_children(n)
            .find(|&c| dom.is(c, wp(LocalName::PosOffset)))
            .and_then(|c| dom.semantic_children(c).find_map(|t| dom.text(t)))
            .map_or(0, |t| lenient_int(&t))
    };
    let num = |n: NodeId, l: LocalName| dom.attr_value(n, plain(l)).map_or(0, |v| lenient_int(&v));
    for c in dom.semantic_children(anchor) {
        let Some(name) = dom.name(c) else { continue };
        if name.ns != NsId::Wp {
            continue;
        }
        match name.local {
            LocalName::PositionH => info.offset_emu.0 = pos_offset(c),
            LocalName::PositionV => info.offset_emu.1 = pos_offset(c),
            LocalName::Extent => info.extent_emu = (num(c, LocalName::Cx), num(c, LocalName::Cy)),
            LocalName::DocPr => {
                info.payload = dom
                    .attr_value(c, plain(LocalName::Descr))
                    .filter(|v| !v.is_empty())
                    .map(|v| v.into_owned());
            }
            _ => {}
        }
    }
    info.rel_id = dom
        .semantic_descendants(anchor)
        .find(|&n| dom.is(n, QName::new(NsId::A, LocalName::Blip)))
        .and_then(|b| dom.attr_value(b, QName::new(NsId::R, LocalName::Embed)))
        .map(|v| v.into_owned());
    info
}

/// 主 part 全部块里的墨迹，文档序（`Document.inks`；`rebuild` 与 `refresh_blocks` 都用它重算）。
///
/// 文本块从 `Run.segments` 的 [`SegmentKind::Ink`] 取；图片块 / 只读块没有内联模型，扫子树（TS 用正则扫
/// 每个块的 `originalXml`，表格 / 只读块里的墨迹同样算）。
pub fn collect_inks(dom: &Dom, blocks: &[Block]) -> Vec<InkInfo> {
    let mut out = Vec::new();
    for b in Blocks::over(blocks) {
        match b {
            Block::Text(tb) => {
                let mut stack: Vec<&Inline> = tb.inlines.iter().rev().collect();
                while let Some(inl) = stack.pop() {
                    match inl {
                        Inline::Run(r) => {
                            for seg in &r.segments {
                                if seg.kind == SegmentKind::Ink {
                                    out.push(ink_info(dom, tb.node, r.node, seg.node));
                                }
                            }
                        }
                        Inline::Field { result, .. } => stack.extend(result.iter().rev()),
                        Inline::Atom(_) => {}
                    }
                }
            }
            // 表格块的单元格段落由 `Blocks` 迭代器展开成文本块，这里只剩没有内联模型的块
            Block::Table(_) => {}
            Block::Image(_) | Block::Protected(_) => scan_subtree(dom, b.node(), &mut out),
        }
    }
    out
}

/// 没有内联模型的块：直接在子树里找墨迹 run。
fn scan_subtree(dom: &Dom, root: NodeId, out: &mut Vec<InkInfo>) {
    for n in dom.semantic_descendants(root) {
        if !dom.is(n, QName::w(LocalName::Drawing)) || !is_ink_drawing(dom, n) {
            continue;
        }
        let Some(run) = dom.ancestors(n).find(|&a| dom.is(a, QName::w(LocalName::R))) else {
            continue;
        };
        let Some(para) = dom.ancestors(run).find(|&a| dom.is(a, QName::w(LocalName::P))) else {
            continue;
        };
        out.push(ink_info(dom, para, run, n));
    }
}

// 内联模型与坐标流（`MOD-06`，`docs/03` §6.3、§8.1）。
//
// `Run` 与物理 `w:r` 一一对应；`segments` 覆盖 run 的全部子节点、按顺序、区间不重叠，
// 给出坐标流中的文本偏移到子节点的映射。偏移单位对外是 UTF-16 code unit，
// 内部字符串是 UTF-8，`utf16_len` 缓存每段长度。

use std::ops::Range;

use crate::span::{FieldId, SpanId};
use crate::xml::{NodeId, QName};

/// 坐标流里代表一个原子（图片、字段、公式、分页符……）的字符，占 1 个 UTF-16 单位。
pub const OBJECT_REPLACEMENT: char = '\u{FFFC}';

#[derive(Debug, Clone, PartialEq, Eq)]
// `Run` 是绝对多数，装箱只会多一次分配；`Atom` / `Field` 少见，接受尺寸差。
#[allow(clippy::large_enum_variant)]
pub enum Inline {
    Run(Run),
    /// 原子形态的字段（`FLD-07`）：坐标流中 1 个 `U+FFFC`，`result` 不参与坐标。M2 建立。
    Field {
        id: FieldId,
        result: Vec<Inline>,
    },
    /// 段落级非 `w:r` 子节点（公式、裸 `w:br`、未知元素）。
    Atom(InlineAtom),
}

impl Inline {
    /// 坐标流贡献（UTF-16 单位）。
    pub fn utf16_len(&self) -> u32 {
        match self {
            Inline::Run(r) => r.utf16_len,
            Inline::Field { .. } | Inline::Atom(_) => 1,
        }
    }

    pub fn node(&self) -> Option<NodeId> {
        match self {
            Inline::Run(r) => Some(r.node),
            Inline::Field { .. } => None,
            Inline::Atom(a) => Some(a.node),
        }
    }

    /// 坐标流文本追加到 `out`。
    pub fn append_text(&self, out: &mut String) {
        match self {
            Inline::Run(r) => out.push_str(&r.text),
            Inline::Field { .. } | Inline::Atom(_) => out.push(OBJECT_REPLACEMENT),
        }
    }
}

/// 一个物理 `w:r`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub node: NodeId,
    /// 覆盖全部子节点（`w:rPr` 除外），按顺序，`text` 区间不重叠。
    pub segments: Vec<Segment>,
    /// 坐标流中该 run 的文本。
    pub text: String,
    pub utf16_len: u32,
    /// 声明值（`w:rPr`）。
    pub props: RunProps,
    /// 所在的 `w:hyperlink`，或透明字段。
    pub link: Option<Link>,
    /// 透明字段（`Link` 策略）的 id；结构 run 也带它，段长度为 0。M2 建立。
    pub field: Option<FieldId>,
    /// 祖先 `w:ins/w:del/w:moveFrom/w:moveTo` 与自身 `rPrChange`。
    pub rev: Option<RevisionCtx>,
    /// 覆盖该 run 的批注范围（由 Span 索引反查，M2）。
    pub comments: Vec<SpanId>,
}

impl Run {
    /// `text` 中某段的字符串。
    pub fn segment_text(&self, seg: &Segment) -> &str {
        &self.text[seg.text.start as usize..seg.text.end as usize]
    }
}

/// run 的一个子节点在坐标流中的投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub node: NodeId,
    pub kind: SegmentKind,
    /// 在 `Run.text` 中的字节区间（长度 0 的段也有位置）。
    pub text: Range<u32>,
    pub utf16_len: u32,
    /// 显示模型（`MOD-11`）：绘图 / VML / OLE 段才有。
    pub display: Option<Display>,
}

/// `w:br/@w:type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    TextWrapping,
    Page,
    Column,
}

impl BreakKind {
    pub fn parse(s: Option<&str>) -> BreakKind {
        match s.map(str::trim) {
            Some("page") => BreakKind::Page,
            Some("column") => BreakKind::Column,
            _ => BreakKind::TextWrapping,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKind {
    Text,
    DelText,
    Tab,
    PTab {
        align: Option<String>,
    },
    Br {
        kind: BreakKind,
        clear: Option<String>,
    },
    Cr,
    NoBreakHyphen,
    SoftHyphen,
    /// `w:sym`：`font` 与 `w:char` 的十六进制码；显示解码在 `RES-05`。
    Sym {
        font: Option<String>,
        code: Option<u32>,
    },
    Drawing {
        anchored: bool,
    },
    Pict,
    Object,
    /// `w:ruby`：注音文字与被注的正文（各取直接 `w:r/w:t`，TS `rubyPartText`）。坐标流里是 1 个原子。
    Ruby {
        rt: String,
        base: String,
    },
    /// `aidocs-ink` 墨迹批注的浮动图片（任务 6.8）：对坐标流不可见（长度 0）、对分类不可见；
    /// 几何与载荷见 [`crate::model::InkInfo`]（`Document.inks`）。
    Ink,
    FootnoteRef {
        id: Option<String>,
    },
    EndnoteRef {
        id: Option<String>,
    },
    /// 脚注 / 尾注正文里的编号标记（`w:footnoteRef`）。
    FootnoteRefMark,
    EndnoteRefMark,
    Separator,
    ContinuationSeparator,
    CommentRef,
    LastRenderedPageBreak,
    FldChar,
    InstrText,
    DelInstrText,
    AnnotationRef,
    Other(QName),
}

impl SegmentKind {
    /// 该段是否在坐标流里占位（长度可能为 0 的段：结构标记）。
    pub fn is_zero_width(&self) -> bool {
        matches!(
            self,
            SegmentKind::FldChar
                | SegmentKind::InstrText
                | SegmentKind::DelInstrText
                | SegmentKind::CommentRef
                | SegmentKind::LastRenderedPageBreak
                | SegmentKind::AnnotationRef
                | SegmentKind::FootnoteRefMark
                | SegmentKind::EndnoteRefMark
        )
    }
}

/// 段落级非 `w:r` 子节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAtom {
    pub node: NodeId,
    pub kind: AtomKind,
    /// 用于新输入继承格式；M1 为默认。
    pub props: RunProps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomKind {
    /// `m:oMath`（R19 文字夹公式的段落里的一个公式原子；整段公式是 R11 的保护块）。
    Math,
    /// run 外的 `w:br`。
    BareBreak {
        kind: BreakKind,
    },
    Other(QName),
}

/// 超链接来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Hyperlink { node: NodeId, target: LinkTarget, tooltip: Option<String> },
    Field(FieldId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// `w:anchor`：文内书签。
    Internal { anchor: String },
    /// `r:id`：`href` 是关系的外部目标（关系缺失或不是外部目标时为 `None`）。
    External { rel_id: String, href: Option<String> },
    /// 两者都没有。
    Unresolved,
}

/// 一条修订的元数据（`w:id` / `w:author` / `w:date`）。定义在 L2（范围标记用同一组属性）。
pub use crate::span::RevisionMeta;

/// run 的修订上下文（`MOD-06`）：`w:moveFrom` 同时计入 `del`，`w:moveTo` 同时计入 `ins`（TS 语义），
/// `move_*` 保留精确信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionCtx {
    pub ins: Option<RevisionMeta>,
    pub del: Option<RevisionMeta>,
    pub move_from: Option<RevisionMeta>,
    pub move_to: Option<RevisionMeta>,
    /// 自身 `w:rPrChange`：元数据与旧值快照。
    pub props_change: Option<(RevisionMeta, Box<RunProps>)>,
}

impl RevisionCtx {
    pub fn is_empty(&self) -> bool {
        self.ins.is_none()
            && self.del.is_none()
            && self.move_from.is_none()
            && self.move_to.is_none()
            && self.props_change.is_none()
    }
}

/// UTF-16 长度。
pub fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

// 公式与 ruby 的模型（`MOD-11`，`spec/17` 任务 6.5）。
//
// 公式段落（R11：`m:oMathPara` 或没有可见正文的公式段）挂 [`FormulaDisplay`]：片段节点、可编辑的 token、
// MathML 与 LaTeX（转换器在 [`crate::model::omml`]）。文字夹公式的段落（R19）里每个 `m:oMath` 是一个
// `Inline::Atom(Math)`，投影时按需算 token。原字节（TS 的 `omml`）在投影层按 `lex.range` 切，与 `rawRPr` 同一做法。

use crate::model::omml::{latex, mathml};

/// 一个公式段落的显示模型（TS `FormulaDisplay`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaDisplay {
    /// 段落里的 `m:oMath`，文档序（`m:oMathPara` 展开）。
    pub fragments: Vec<NodeId>,
    /// 全部 `m:t` 文本，文档序：可编辑的 token 串。
    pub tokens: Vec<String>,
    /// MathML Core，各片段拼接。段落有可见正文时不算（TS：oMathPara 旁还有普通 run 的段落只保留平铺的 token 条），
    /// 一个片段都转不出内容也没有。
    pub mathml: Option<String>,
    /// LaTeX 子集：只有一个片段、且全在子集之内才有。
    pub latex: Option<String>,
}

/// 一个段落的公式显示模型。`visible_text`：段落有可见正文（`ParagraphFacts::visible_text`）。
pub fn formula_display(dom: &Dom, para: NodeId, visible_text: bool) -> FormulaDisplay {
    let fragments = omml::fragments(dom, para);
    let tokens: Vec<String> = fragments.iter().flat_map(|&f| omml::tokens(dom, f)).collect();
    let mathml = (!visible_text)
        .then(|| fragments.iter().map(|&f| mathml::to_mathml(dom, f)).collect::<String>())
        .filter(|s| !s.is_empty());
    let latex = match fragments.as_slice() {
        [only] => latex::to_latex(dom, *only).filter(|s| !s.is_empty()),
        _ => None,
    };
    FormulaDisplay { fragments, tokens, mathml, latex }
}

/// 一个 `m:oMath` 原子的 token（R19 的公式 run：`text` = token 拼接）。
pub fn math_tokens(dom: &Dom, omath: NodeId) -> Vec<String> {
    omml::tokens(dom, omath)
}

/// `w:ruby` 的一半（`w:rt` / `w:rubyBase`）的文字：直接 `w:r` 子节点的直接 `w:t` 子节点拼接（TS `rubyPartText`）。
pub fn ruby_part_text(dom: &Dom, ruby: NodeId, part: LocalName) -> String {
    let Some(part) = dom.semantic_children(ruby).find(|&c| dom.is(c, QName::w(part))) else {
        return String::new();
    };
    let mut out = String::new();
    for r in dom.semantic_children(part).filter(|&r| dom.is(r, QName::w(LocalName::R))) {
        for t in dom.semantic_children(r).filter(|&t| dom.is(t, QName::w(LocalName::T))) {
            out.push_str(&omml::text_of(dom, t));
        }
    }
    out
}

// 批注与脚注 / 尾注条目（`MOD-10`，任务 2.6）。
//
// 一条批注要三个部件合起来才完整：`comments.xml` 给正文、作者与首字母，
// `commentsExtended.xml` 给回复关系与"已解决"（按最后一段的 `w14:paraId` 关联），
// `commentsIds.xml` 给 durableId。注释部件里带 `w:type` 的条目（`separator` /
// `continuationSeparator`）是结构条目，不是正文——保存时要原样留着（`spec/13` 2.6）。
//
// 声明值（文字、格式、节点位置）与**内容块**（`Note.blocks` / `Comment.blocks`）都在这里：
// 条目的内容与页眉页脚、正文同一个构建器（`docs/03` §6.7，任务 5.3）。`text` / `rich` 是 TS 形态的
// 投影（`COMPAT-02` 的 `footnotes[].richParas`），与 `blocks` 并存——它们随 `compat_ts` 在 M9 一起删。

use std::collections::HashMap;

use crate::semantic::props::read_run_props;

fn notes_w(l: LocalName) -> QName {
    QName::new(NsId::W, l)
}

fn notes_attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|v| v.into_owned())
}

fn notes_flag(v: Option<String>) -> bool {
    matches!(v.as_deref(), Some("1" | "true" | "on"))
}

/// 一个 run 的文字与格式（注释条目的 `richParas` 用）。
#[derive(Debug, Clone, PartialEq)]
pub struct RichRun {
    pub node: NodeId,
    pub text: String,
    pub props: RunProps,
}

/// 一条批注。
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// `w:comment`。
    pub node: NodeId,
    pub id: String,
    pub author: Option<String>,
    pub initials: Option<String>,
    pub date: Option<String>,
    /// 各段文字以 `\n` 连接。
    pub text: String,
    /// **最后一段**的 `w14:paraId`（`commentsExtended` 按它关联）。
    pub para_id: Option<String>,
    /// 回复的父批注 id（由 `w15:paraIdParent` 反查）。
    pub parent_id: Option<String>,
    /// `w15:done`。
    pub done: bool,
    /// `commentsIds.xml` 的 `w16cid:durableId`。
    pub durable_id: Option<String>,
    /// 条目里的 `w:p`（保存时的手术式补丁用）。
    pub paragraphs: Vec<NodeId>,
    pub rich: Vec<Vec<RichRun>>,
    /// 条目内容，与正文同一构建器（`MOD-01`，任务 5.3）。
    pub blocks: Vec<Block>,
}

/// `comments.xml`（+ `commentsExtended.xml` / `commentsIds.xml`）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comments {
    pub part: Option<PartId>,
    pub extended_part: Option<PartId>,
    pub ids_part: Option<PartId>,
    pub items: Vec<Comment>,
    /// `comments.xml` 的三份索引（`SPAN-01`：每条批注是一个独立内容流）。part 缺失时 `None`。
    pub idx: Option<AuxFlows>,
}

impl Comments {
    /// `comments.xml` 缺失时是空集合（`part` 为 `None`，`AddComment` 据此建 part）。
    pub fn from_doms(
        comments: Option<(PartId, &Dom)>,
        extended: Option<(PartId, &Dom)>,
        ids: Option<(PartId, &Dom)>,
        rels: Option<&Rels>,
        styles: Option<&Styles>,
        diags: &mut Vec<Diagnostic>,
    ) -> Comments {
        let mut out = Comments {
            part: comments.map(|(p, _)| p),
            extended_part: extended.map(|(p, _)| p),
            ids_part: ids.map(|(p, _)| p),
            items: Vec::new(),
            idx: None,
        };
        if let Some((part, dom)) = comments {
            let root = dom.root();
            let idx = AuxFlows::build(part, dom, diags);
            for c in dom.semantic_children(root).filter(|&n| dom.is(n, notes_w(LocalName::Comment)))
            {
                let mut item = read_comment(dom, c, diags);
                if let Some(rels) = rels {
                    item.blocks = idx.blocks_of(dom, rels, styles, c, diags);
                }
                out.items.push(item);
            }
            out.idx = Some(idx);
        }
        out.link_extended(extended.map(|(_, d)| d), ids.map(|(_, d)| d));
        out
    }

    /// `commentsExtended` 的 `done` / `paraIdParent` 与 `commentsIds` 的 durableId。
    fn link_extended(&mut self, extended: Option<&Dom>, ids: Option<&Dom>) {
        let by_para: HashMap<String, String> = self
            .items
            .iter()
            .filter_map(|c| c.para_id.clone().map(|p| (p, c.id.clone())))
            .collect();
        if let Some(dom) = extended {
            let mut done: HashMap<String, bool> = HashMap::new();
            let mut parent: HashMap<String, String> = HashMap::new();
            for e in dom.semantic_children(dom.root()) {
                if !dom.is(e, QName::new(NsId::W15, LocalName::CommentEx)) {
                    continue;
                }
                let Some(pid) = notes_attr(dom, e, NsId::W15, LocalName::ParaId) else { continue };
                if notes_flag(notes_attr(dom, e, NsId::W15, LocalName::Done)) {
                    done.insert(pid.clone(), true);
                }
                if let Some(par) = notes_attr(dom, e, NsId::W15, LocalName::ParaIdParent) {
                    parent.insert(pid, par);
                }
            }
            for c in &mut self.items {
                let Some(pid) = &c.para_id else { continue };
                c.done = done.get(pid).copied().unwrap_or(false);
                c.parent_id = parent.get(pid).and_then(|p| by_para.get(p)).cloned();
            }
        }
        if let Some(dom) = ids {
            let mut durable: HashMap<String, String> = HashMap::new();
            for e in dom.semantic_children(dom.root()) {
                if !dom.is(e, QName::new(NsId::W16Cid, LocalName::CommentId)) {
                    continue;
                }
                if let (Some(pid), Some(d)) = (
                    notes_attr(dom, e, NsId::W16Cid, LocalName::ParaId),
                    notes_attr(dom, e, NsId::W16Cid, LocalName::DurableId),
                ) {
                    durable.insert(pid, d);
                }
            }
            for c in &mut self.items {
                if let Some(pid) = &c.para_id {
                    c.durable_id = durable.get(pid).cloned();
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Comment> {
        self.items.iter().find(|c| c.id == id)
    }

    /// `EDIT-06`：批注 `w:id` 在文档内取最大值 + 1。
    pub fn next_id(&self) -> u32 {
        self.items.iter().filter_map(|c| c.id.trim().parse::<u32>().ok()).max().map_or(1, |m| m + 1)
    }
}

fn read_comment(dom: &Dom, node: NodeId, diags: &mut Vec<Diagnostic>) -> Comment {
    let paragraphs: Vec<NodeId> =
        dom.semantic_children(node).filter(|&n| dom.is(n, notes_w(LocalName::P))).collect();
    let (text, rich) = entry_text(dom, &paragraphs, false, diags);
    Comment {
        node,
        id: notes_attr(dom, node, NsId::W, LocalName::Id).unwrap_or_default(),
        author: notes_attr(dom, node, NsId::W, LocalName::Author),
        initials: notes_attr(dom, node, NsId::W, LocalName::Initials),
        date: notes_attr(dom, node, NsId::W, LocalName::Date),
        text,
        para_id: paragraphs.last().and_then(|&p| notes_attr(dom, p, NsId::W14, LocalName::ParaId)),
        parent_id: None,
        done: false,
        durable_id: None,
        paragraphs,
        rich,
        blocks: Vec::new(),
    }
}

/// 注释条目的种类（`w:type`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteKind {
    /// 正文条目（无 `w:type`）。
    Normal,
    Separator,
    ContinuationSeparator,
    ContinuationNotice,
    Other(String),
}

impl NoteKind {
    pub fn is_normal(&self) -> bool {
        *self == NoteKind::Normal
    }
}

/// 一条脚注 / 尾注。
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// `w:footnote` / `w:endnote`。
    pub node: NodeId,
    pub id: String,
    pub kind: NoteKind,
    /// 各段文字以 `\n` 连接；首段去前导空白（自引用标记后的间隔）。
    pub text: String,
    pub rich: Vec<Vec<RichRun>>,
    /// 条目里没有任何 `w:footnoteRef` / `w:endnoteRef` run。
    pub no_ref_mark: bool,
    /// 首段的 `w:pStyle`（真实 Word 的脚注段落带「脚注文本」样式；TS `footnotes[].styleId`）。
    pub style_id: Option<String>,
    pub paragraphs: Vec<NodeId>,
    /// 条目内容，与正文同一构建器（`MOD-01`，任务 5.3）。
    pub blocks: Vec<Block>,
}

/// `footnotes.xml` 或 `endnotes.xml`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    pub part: Option<PartId>,
    /// 全部条目，含 separator 一类结构条目（保存时要原样保留）。
    pub items: Vec<Note>,
    /// 该 part 的三份索引（`SPAN-01`：每个条目是一个独立内容流）。part 缺失时 `None`。
    pub idx: Option<AuxFlows>,
}

impl Notes {
    pub fn from_dom(
        part: Option<(PartId, &Dom)>,
        entry: LocalName,
        ref_mark: LocalName,
        rels: Option<&Rels>,
        styles: Option<&Styles>,
        diags: &mut Vec<Diagnostic>,
    ) -> Notes {
        let Some((id, dom)) = part else { return Notes::default() };
        let idx = AuxFlows::build(id, dom, diags);
        let entries: Vec<NodeId> =
            dom.semantic_children(dom.root()).filter(|&n| dom.is(n, notes_w(entry))).collect();
        let mut items = Vec::with_capacity(entries.len());
        for n in entries {
            let mut item = read_note(dom, n, ref_mark, diags);
            if let Some(rels) = rels {
                item.blocks = idx.blocks_of(dom, rels, styles, n, diags);
            }
            items.push(item);
        }
        Notes { part: Some(id), items, idx: Some(idx) }
    }

    /// 正文条目（`separator` / `continuationSeparator` 不算）。
    pub fn normal(&self) -> impl Iterator<Item = &Note> + '_ {
        self.items.iter().filter(|n| n.kind.is_normal())
    }

    pub fn get(&self, id: &str) -> Option<&Note> {
        self.items.iter().find(|n| n.id == id)
    }

    /// `EDIT-06`：注释 id 取最大值 + 1（结构条目用 -1 / 0，一并计入）。
    pub fn next_id(&self) -> i64 {
        self.items.iter().filter_map(|n| n.id.trim().parse::<i64>().ok()).max().map_or(1, |m| m + 1)
    }
}

fn read_note(dom: &Dom, node: NodeId, ref_mark: LocalName, diags: &mut Vec<Diagnostic>) -> Note {
    // `w:type`（小写）是 `LocalName::Type`；`UType` 是 `[Content_Types].xml` 里大写的 `Type`
    let kind = match notes_attr(dom, node, NsId::W, LocalName::Type).as_deref() {
        None => NoteKind::Normal,
        Some("separator") => NoteKind::Separator,
        Some("continuationSeparator") => NoteKind::ContinuationSeparator,
        Some("continuationNotice") => NoteKind::ContinuationNotice,
        Some(other) => NoteKind::Other(other.to_string()),
    };
    let paragraphs: Vec<NodeId> =
        dom.semantic_children(node).filter(|&n| dom.is(n, notes_w(LocalName::P))).collect();
    let (text, rich) = entry_text(dom, &paragraphs, true, diags);
    let no_ref_mark = !dom.descendants(node).any(|n| dom.is(n, notes_w(ref_mark)));
    let style_id = paragraphs.first().and_then(|&p| {
        let ppr = dom.semantic_children(p).find(|&c| dom.is(c, notes_w(LocalName::PPr)))?;
        let st = dom.semantic_children(ppr).find(|&c| dom.is(c, notes_w(LocalName::PStyle)))?;
        notes_attr(dom, st, NsId::W, LocalName::Val)
    });
    Note {
        node,
        id: notes_attr(dom, node, NsId::W, LocalName::Id).unwrap_or_default(),
        kind,
        text,
        rich,
        no_ref_mark,
        style_id,
        paragraphs,
        blocks: Vec::new(),
    }
}

/// 条目文字与 run 格式。
///
/// 每段把 `w:t` 拼起来，段间 `\n`。`notes` 为真时（脚注 / 尾注）跳过含自引用标记的 run，
/// 并把**首段的前导空白**吃掉——自引用标记与正文之间那个分隔空格在 Word 里不是内容，
/// TS 的 `text` 与 `richParas` 都不含它，整个 run 只有空白时连 run 一起丢。
fn entry_text(
    dom: &Dom,
    paragraphs: &[NodeId],
    notes: bool,
    diags: &mut Vec<Diagnostic>,
) -> (String, Vec<Vec<RichRun>>) {
    let mut rich: Vec<Vec<RichRun>> = Vec::new();
    for &p in paragraphs {
        let mut runs: Vec<RichRun> = Vec::new();
        // 段内所有 run（含 `w:ins` / `w:hyperlink` 一类包裹里的）
        for r in dom.descendants(p).filter(|&n| dom.is(n, notes_w(LocalName::R))) {
            if notes && is_ref_mark_run(dom, r) {
                continue;
            }
            let mut text = String::new();
            let mut props = RunProps::default();
            for c in dom.semantic_children(r) {
                let Some(name) = dom.name(c) else { continue };
                if name == notes_w(LocalName::RPr) {
                    props = read_run_props(dom, Some(c), diags);
                } else if name == notes_w(LocalName::T)
                    && let Some(t) = dom.children(c).first().and_then(|&t| dom.text(t))
                {
                    text.push_str(&t);
                }
            }
            if text.is_empty() {
                continue;
            }
            runs.push(RichRun { node: r, text, props });
        }
        rich.push(runs);
    }
    if notes && let Some(first) = rich.first_mut() {
        while let Some(run) = first.first_mut() {
            let trimmed = run.text.trim_start().to_string();
            if trimmed.is_empty() {
                first.remove(0);
                continue;
            }
            run.text = trimmed;
            break;
        }
    }
    let text = rich
        .iter()
        .map(|line| line.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    (text, rich)
}

/// 含 `w:footnoteRef` / `w:endnoteRef` 的自引用标记 run。
fn is_ref_mark_run(dom: &Dom, run: NodeId) -> bool {
    dom.semantic_children(run).any(|c| {
        dom.is(c, notes_w(LocalName::FootnoteRef)) || dom.is(c, notes_w(LocalName::EndnoteRef))
    })
}

pub mod omml {
    //! OMML（Office Math，`m:` 命名空间）的读法与两个转换器（`spec/17` 任务 6.5）。
    //!
    //! [`mathml`] 与 [`latex`] 是 TS `math.ts` 的 `ommlToMathML` / `ommlToLatex` 的逐字移植——差分按字符串比较，
    //! `mn / mi / mo` 分类、运算符集、函数名表、转义规则都必须一样。两个转换器都是**迭代**实现（显式任务栈 +
    //! 结果栈）：语料 `corpus/hostile/omml-deep.docx` 有 3,000 层嵌套，递归会把测试线程的栈吃光。
    //! 这里放两者共用的小工具：语义子节点查找、属性包读取、run 文字、XML 转义。

    pub mod latex {
        //! OMML → LaTeX 子集（TS `ommlToLatex`，`math.ts` 502–723 的逐字移植）。
        //!
        //! 子集之外的结构（`m:sPre`、`m:limUpp`、认不出的 n 元运算符 / 重音 / 定界符、`\` 与换行）→ `None`，
        //! 调用方只保留 token 级编辑。与 [`super::mathml`] 同一套迭代求值骨架，错误一路短路。

        use super::{
            child, children_named, content_children, is_plain_run, plain_text_of_runs, prop_on,
            prop_val, run_text, text_of,
        };
        use crate::xml::{Dom, LocalName, NodeId, NsId};

        struct Unsupported;

        /// 一个 `m:oMath` → LaTeX；子集之外 → `None`。结果 trim 并把连续空白压成一个空格。
        pub fn to_latex(dom: &Dom, omath: NodeId) -> Option<String> {
            let raw = eval(dom, Item::Seq(omath)).ok()?;
            let mut out = String::with_capacity(raw.len());
            let mut ws = 0usize;
            for ch in raw.trim().chars() {
                if ch.is_whitespace() {
                    ws += 1;
                    if ws == 1 {
                        out.push(ch);
                    } else if ws == 2 {
                        out.pop();
                        out.push(' ');
                    }
                } else {
                    ws = 0;
                    out.push(ch);
                }
            }
            Some(out)
        }

        #[derive(Clone)]
        enum Item {
            Node(NodeId),
            /// `parent/m:<name>` 的内容（缺失 → `""`）。
            Slot(NodeId, LocalName),
            /// 内容子节点直接拼接。
            Seq(NodeId),
            /// `\binom{num}{den}`（`(` `)` 包着的单个 noBar 分式）。
            Binom(NodeId),
            /// `m:m` / `m:eqArr` 的行体；`env` 是环境名（`matrix` / `pmatrix` / … / `cases`）。
            Matrix {
                node: NodeId,
                env: String,
            },
            /// 一行 `m:mr`：各格 ` & ` 连接。
            MatrixRow(NodeId),
            /// `\left<beg> … \right<end>`。
            LeftRight {
                beg: String,
                end: String,
                slot: NodeId,
            },
        }

        enum Task {
            Eval(Item),
            Finish(Item, usize),
        }

        fn eval(dom: &Dom, root: Item) -> Result<String, Unsupported> {
            let mut tasks = vec![Task::Eval(root)];
            let mut results: Vec<String> = Vec::new();
            while let Some(task) = tasks.pop() {
                match task {
                    Task::Eval(item) => {
                        if let Some(subs) = expand(dom, &item, &mut results)? {
                            tasks.push(Task::Finish(item, subs.len()));
                            tasks.extend(subs.into_iter().rev().map(Task::Eval));
                        }
                    }
                    Task::Finish(item, arity) => {
                        let at = results.len() - arity;
                        let parts: Vec<String> = results.drain(at..).collect();
                        results.push(finish(dom, &item, parts)?);
                    }
                }
            }
            Ok(results.pop().unwrap_or_default())
        }

        fn nodes(dom: &Dom, n: NodeId) -> Vec<Item> {
            content_children(dom, n).into_iter().map(Item::Node).collect()
        }

        /// 矩阵的行：有 `m:mr` 就按行 / 格，否则每个 `m:e` 一行。
        fn matrix_rows(dom: &Dom, node: NodeId) -> Vec<Item> {
            let mrs = children_named(dom, node, LocalName::Mr);
            if mrs.is_empty() {
                children_named(dom, node, LocalName::E).into_iter().map(Item::Seq).collect()
            } else {
                mrs.into_iter().map(Item::MatrixRow).collect()
            }
        }

        fn expand(
            dom: &Dom,
            item: &Item,
            results: &mut Vec<String>,
        ) -> Result<Option<Vec<Item>>, Unsupported> {
            let slot = |n: NodeId, l: LocalName| Item::Slot(n, l);
            Ok(match item {
                Item::Slot(parent, name) => match child(dom, *parent, *name) {
                    None => {
                        results.push(String::new());
                        None
                    }
                    Some(s) => Some(nodes(dom, s)),
                },
                Item::Seq(n) => Some(nodes(dom, *n)),
                Item::Binom(f) => Some(vec![slot(*f, LocalName::Num), slot(*f, LocalName::Den)]),
                Item::Matrix { node, .. } => Some(matrix_rows(dom, *node)),
                Item::MatrixRow(mr) => Some(
                    children_named(dom, *mr, LocalName::E).into_iter().map(Item::Seq).collect(),
                ),
                Item::LeftRight { slot, .. } => Some(nodes(dom, *slot)),
                Item::Node(n) => {
                    let n = *n;
                    let Some(name) = dom.name(n) else {
                        results.push(String::new());
                        return Ok(None);
                    };
                    if name.ns != NsId::M {
                        return Err(Unsupported);
                    }
                    Some(match name.local {
                        LocalName::R => {
                            results.push(run_to_latex(dom, n)?);
                            return Ok(None);
                        }
                        LocalName::T => {
                            results.push(chars_to_latex(&text_of(dom, n))?);
                            return Ok(None);
                        }
                        LocalName::F => {
                            // 裸的 noBar 分式只出现在 \binom 的 m:d 包里（那边处理）；别的分式样式在子集之外
                            if prop_val(dom, n, LocalName::FPr, LocalName::Type)
                                .is_some_and(|t| t != "bar")
                            {
                                return Err(Unsupported);
                            }
                            vec![slot(n, LocalName::Num), slot(n, LocalName::Den)]
                        }
                        LocalName::SSup => vec![slot(n, LocalName::E), slot(n, LocalName::Sup)],
                        LocalName::SSub => vec![slot(n, LocalName::E), slot(n, LocalName::Sub)],
                        LocalName::SSubSup => {
                            vec![
                                slot(n, LocalName::E),
                                slot(n, LocalName::Sub),
                                slot(n, LocalName::Sup),
                            ]
                        }
                        LocalName::Rad => {
                            if prop_on(dom, n, LocalName::RadPr, LocalName::DegHide)
                                || child(dom, n, LocalName::Deg).is_none()
                            {
                                vec![slot(n, LocalName::E)]
                            } else {
                                vec![slot(n, LocalName::Deg), slot(n, LocalName::E)]
                            }
                        }
                        LocalName::D => return delimiter(dom, n).map(|it| Some(vec![it])),
                        LocalName::Nary => {
                            let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                                .unwrap_or_else(|| "∫".into());
                            if nary_command(&chr).is_none() {
                                return Err(Unsupported);
                            }
                            vec![
                                slot(n, LocalName::Sub),
                                slot(n, LocalName::Sup),
                                slot(n, LocalName::E),
                            ]
                        }
                        LocalName::Func => {
                            let name = plain_text_of_runs(dom, child(dom, n, LocalName::FName));
                            let name = name.trim();
                            if !(LATEX_FUNCTIONS.contains(&name)
                                || name == "lim"
                                || (!name.is_empty()
                                    && name.chars().all(|c| c.is_ascii_alphabetic())))
                            {
                                return Err(Unsupported);
                            }
                            vec![slot(n, LocalName::E)]
                        }
                        LocalName::LimLow => {
                            if plain_text_of_runs(dom, child(dom, n, LocalName::E)).trim() != "lim"
                            {
                                return Err(Unsupported);
                            }
                            vec![slot(n, LocalName::Lim)]
                        }
                        LocalName::Acc => {
                            let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{0302}".into());
                            if accent_command(&chr).is_none() {
                                return Err(Unsupported);
                            }
                            vec![slot(n, LocalName::E)]
                        }
                        LocalName::Bar => vec![slot(n, LocalName::E)],
                        LocalName::GroupChr => {
                            let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{23DF}".into());
                            if chr != "\u{23DF}" && chr != "\u{23DE}" {
                                return Err(Unsupported);
                            }
                            vec![slot(n, LocalName::E)]
                        }
                        LocalName::M => {
                            return Ok(Some(vec![Item::Matrix { node: n, env: "matrix".into() }]));
                        }
                        LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                            vec![slot(n, LocalName::E)]
                        }
                        _ => return Err(Unsupported),
                    })
                }
            })
        }

        fn finish(dom: &Dom, item: &Item, parts: Vec<String>) -> Result<String, Unsupported> {
            let p = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
            Ok(match item {
                Item::Slot(..) | Item::Seq(_) => parts.concat(),
                Item::Binom(_) => format!("\\binom{{{}}}{{{}}}", p(0), p(1)),
                Item::Matrix { env, .. } => {
                    format!("\\begin{{{env}}} {} \\end{{{env}}}", parts.join(" \\\\ "))
                }
                Item::MatrixRow(_) => parts.join(" & "),
                Item::LeftRight { beg, end, .. } => {
                    format!("\\left{beg} {} \\right{end}", parts.concat())
                }
                Item::Node(n) => {
                    let n = *n;
                    let Some(name) = dom.name(n) else { return Ok(String::new()) };
                    match name.local {
                        LocalName::F => format!("\\frac{{{}}}{{{}}}", p(0), p(1)),
                        LocalName::SSup => format!("{{{}}}^{{{}}}", p(0), p(1)),
                        LocalName::SSub => format!("{{{}}}_{{{}}}", p(0), p(1)),
                        LocalName::SSubSup => format!("{{{}}}_{{{}}}^{{{}}}", p(0), p(1), p(2)),
                        LocalName::Rad => {
                            if parts.len() == 1 {
                                format!("\\sqrt{{{}}}", p(0))
                            } else {
                                format!("\\sqrt[{}]{{{}}}", p(0), p(1))
                            }
                        }
                        LocalName::D => parts.concat(),
                        LocalName::Nary => {
                            let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                                .unwrap_or_else(|| "∫".into());
                            let command = nary_command(&chr).ok_or(Unsupported)?;
                            let sub = if prop_on(dom, n, LocalName::NaryPr, LocalName::SubHide) {
                                String::new()
                            } else {
                                format!("_{{{}}}", p(0))
                            };
                            let sup = if prop_on(dom, n, LocalName::NaryPr, LocalName::SupHide) {
                                String::new()
                            } else {
                                format!("^{{{}}}", p(1))
                            };
                            format!("\\{command}{sub}{sup} {{{}}}", p(2))
                        }
                        LocalName::Func => {
                            let name = plain_text_of_runs(dom, child(dom, n, LocalName::FName));
                            let name = name.trim();
                            let arg = format!("{{{}}}", p(0));
                            if LATEX_FUNCTIONS.contains(&name) || name == "lim" {
                                format!("\\{name} {arg}")
                            } else {
                                format!("\\operatorname{{{name}}} {arg}")
                            }
                        }
                        LocalName::LimLow => format!("\\lim_{{{}}}", p(0)),
                        LocalName::Acc => {
                            let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{0302}".into());
                            format!("\\{}{{{}}}", accent_command(&chr).ok_or(Unsupported)?, p(0))
                        }
                        LocalName::Bar => {
                            let top = prop_val(dom, n, LocalName::BarPr, LocalName::Pos).as_deref()
                                == Some("top");
                            format!("\\{}{{{}}}", if top { "overline" } else { "underline" }, p(0))
                        }
                        LocalName::GroupChr => {
                            let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{23DF}".into());
                            format!(
                                "\\{}{{{}}}",
                                if chr == "\u{23DE}" { "overbrace" } else { "underbrace" },
                                p(0)
                            )
                        }
                        LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                            p(0).to_string()
                        }
                        // `m:m` 展开成一个 `Matrix` 项，结果就是它
                        LocalName::M => p(0).to_string(),
                        _ => return Err(Unsupported),
                    }
                }
            })
        }

        /// `m:d`（TS `delimiterToLatex`）：`\binom`、矩阵环境、`\left … \right` 三种形态之一。
        fn delimiter(dom: &Dom, d: NodeId) -> Result<Item, Unsupported> {
            let beg =
                prop_val(dom, d, LocalName::DPr, LocalName::BegChr).unwrap_or_else(|| "(".into());
            let end =
                prop_val(dom, d, LocalName::DPr, LocalName::EndChr).unwrap_or_else(|| ")".into());
            let slots = children_named(dom, d, LocalName::E);
            let [slot] = slots.as_slice() else { return Err(Unsupported) };
            let inner = content_children(dom, *slot);
            if let [only] = inner.as_slice() {
                let only = *only;
                if beg == "("
                    && end == ")"
                    && dom.is(only, super::m(LocalName::F))
                    && prop_val(dom, only, LocalName::FPr, LocalName::Type).as_deref()
                        == Some("noBar")
                {
                    return Ok(Item::Binom(only));
                }
                if (dom.is(only, super::m(LocalName::M))
                    || dom.is(only, super::m(LocalName::EqArr)))
                    && let Some(env) = matrix_env(&beg, &end)
                {
                    return Ok(Item::Matrix { node: only, env: env.to_string() });
                }
            }
            let beg_tok = delim_token(&beg).ok_or(Unsupported)?;
            let end_tok = delim_token(&end).ok_or(Unsupported)?;
            Ok(Item::LeftRight { beg: beg_tok.to_string(), end: end_tok.to_string(), slot: *slot })
        }

        fn run_to_latex(dom: &Dom, run: NodeId) -> Result<String, Unsupported> {
            let text = run_text(dom, run);
            if !is_plain_run(dom, run) {
                return chars_to_latex(&text);
            }
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(" ".into());
            }
            if LATEX_FUNCTIONS.contains(&trimmed) {
                return Ok(format!("\\{trimmed} "));
            }
            if trimmed == "lim" {
                return Ok("\\lim ".into());
            }
            if text.contains(['{', '}', '\\']) {
                return Err(Unsupported);
            }
            Ok(format!("\\text{{{text}}}"))
        }

        /// 普通数学文字：解析器的特殊字符转义，符号换成 `\命令 `（TS `charsToLatex`）。
        fn chars_to_latex(text: &str) -> Result<String, Unsupported> {
            let mut out = String::new();
            for ch in text.chars() {
                if ch == '\\' || ch == '\n' {
                    return Err(Unsupported);
                }
                if let Some(esc) = char_escape(ch) {
                    out.push_str(esc);
                    continue;
                }
                match symbol_command(ch) {
                    Some(cmd) => {
                        out.push('\\');
                        out.push_str(cmd);
                        out.push(' ');
                    }
                    None => out.push(ch),
                }
            }
            Ok(out)
        }

        fn char_escape(ch: char) -> Option<&'static str> {
            Some(match ch {
                '{' => "\\{ ",
                '}' => "\\} ",
                '_' => "\\_ ",
                '^' => "\\^ ",
                '&' => "\\& ",
                '%' => "\\% ",
                '$' => "\\$ ",
                '#' => "\\# ",
                _ => return None,
            })
        }

        /// 三张 TS 表的反查：符号 / n 元运算符 / 重音 → 命令名（同一字符有多个名字时**第一个**赢）。
        macro_rules! latex_symbols {
            ($fn:ident / $rev:ident: $($name:literal => $ch:literal),+ $(,)?) => {
                /// 字符 → `\命令`（表序，别名取第一个）。
                fn $fn(ch: char) -> Option<&'static str> {
                    $( if ch == $ch { return Some($name); } )+
                    None
                }

                /// `\命令` → 字符（同一张表的反方向，`latex_to_omml` 用）。
                pub(super) fn $rev(name: &str) -> Option<char> {
                    $( if name == $name { return Some($ch); } )+
                    None
                }
            };
        }

        latex_symbols! { symbol_command / symbol_char:
            "alpha" => 'α', "beta" => 'β', "gamma" => 'γ', "delta" => 'δ', "epsilon" => 'ε', "zeta" => 'ζ',
            "eta" => 'η', "theta" => 'θ', "vartheta" => 'ϑ', "iota" => 'ι', "kappa" => 'κ', "lambda" => 'λ',
            "mu" => 'μ', "nu" => 'ν', "xi" => 'ξ', "pi" => 'π', "rho" => 'ρ', "sigma" => 'σ', "tau" => 'τ',
            "upsilon" => 'υ', "phi" => 'φ', "varphi" => 'ϕ', "chi" => 'χ', "psi" => 'ψ', "omega" => 'ω',
            "Gamma" => 'Γ', "Delta" => 'Δ', "Theta" => 'Θ', "Lambda" => 'Λ', "Xi" => 'Ξ', "Pi" => 'Π',
            "Sigma" => 'Σ', "Upsilon" => 'Υ', "Phi" => 'Φ', "Psi" => 'Ψ', "Omega" => 'Ω',
            "infty" => '∞', "pm" => '±', "mp" => '∓', "times" => '×', "div" => '÷', "cdot" => '⋅', "ast" => '*',
            "le" => '≤', "ge" => '≥', "ne" => '≠', "approx" => '≈', "equiv" => '≡', "sim" => '∼', "propto" => '∝',
            "to" => '→', "leftarrow" => '←', "leftrightarrow" => '↔', "Rightarrow" => '⇒', "Leftarrow" => '⇐',
            "Leftrightarrow" => '⇔', "partial" => '∂', "nabla" => '∇', "in" => '∈', "notin" => '∉',
            "subset" => '⊂', "supset" => '⊃', "subseteq" => '⊆', "supseteq" => '⊇', "cup" => '∪', "cap" => '∩',
            "forall" => '∀', "exists" => '∃', "wedge" => '∧', "vee" => '∨', "neg" => '¬', "angle" => '∠',
            "perp" => '⊥', "parallel" => '∥', "ldots" => '…', "cdots" => '⋯', "vdots" => '⋮', "ddots" => '⋱',
            "prime" => '′', "circ" => '∘', "degree" => '°', "bullet" => '∙', "star" => '⋆', "emptyset" => '∅',
            "hbar" => 'ℏ', "ell" => 'ℓ', "Re" => 'ℜ', "Im" => 'ℑ', "aleph" => 'ℵ', "therefore" => '∴', "because" => '∵',
        }

        latex_symbols! { accent_char_command / accent_char:
            "hat" => '\u{0302}', "bar" => '\u{0304}', "vec" => '\u{20D7}', "dot" => '\u{0307}', "ddot" => '\u{0308}',
            "tilde" => '\u{0303}', "check" => '\u{030C}', "breve" => '\u{0306}',
        }

        latex_symbols! { nary_char_command / nary_char:
            "sum" => '∑', "prod" => '∏', "coprod" => '∐', "bigcup" => '⋃', "bigcap" => '⋂', "int" => '∫',
            "iint" => '∬', "iiint" => '∭', "oint" => '∮',
        }

        fn single(s: &str) -> Option<char> {
            let mut it = s.chars();
            let c = it.next()?;
            it.next().is_none().then_some(c)
        }

        fn nary_command(chr: &str) -> Option<&'static str> {
            single(chr).and_then(nary_char_command)
        }

        fn accent_command(chr: &str) -> Option<&'static str> {
            single(chr).and_then(accent_char_command)
        }

        /// TS `LATEX_FUNCTIONS.has(name)`（`latex_to_omml` 用）。
        pub(super) fn is_latex_function(name: &str) -> bool {
            LATEX_FUNCTIONS.contains(&name)
        }

        /// TS `LATEX_FUNCTIONS`。
        const LATEX_FUNCTIONS: &[&str] = &[
            "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin",
            "arccos", "arctan", "ln", "log", "exp", "max", "min", "sup", "inf", "arg", "det",
            "gcd", "deg", "dim", "ker", "mod",
        ];

        /// 定界字符 → `\left` / `\right` 后面的 token（TS `LEFT_RIGHT_CHARS` 的反查；`""` → `.`）。
        fn delim_token(ch: &str) -> Option<&'static str> {
            Some(match ch {
                "" => ".",
                "(" => "(",
                ")" => ")",
                "[" => "[",
                "]" => "]",
                "|" => "|",
                "{" => "\\{",
                "}" => "\\}",
                "‖" => "\\|",
                "⟨" => "\\langle",
                "⟩" => "\\rangle",
                "⌊" => "\\lfloor",
                "⌋" => "\\rfloor",
                "⌈" => "\\lceil",
                "⌉" => "\\rceil",
                _ => return None,
            })
        }

        /// 定界符对 → 矩阵环境（TS `MATRIX_DELIMS`；`cases` 是 `{` 配空的右侧）。
        fn matrix_env(beg: &str, end: &str) -> Option<&'static str> {
            Some(match (beg, end) {
                ("(", ")") => "pmatrix",
                ("[", "]") => "bmatrix",
                ("{", "}") => "Bmatrix",
                ("|", "|") => "vmatrix",
                ("‖", "‖") => "Vmatrix",
                ("{", "") => "cases",
                _ => return None,
            })
        }
    }
    pub mod latex_to_omml {
        //! LaTeX → OMML（TS `math.ts` 的 `latexToOmml`，`spec/18` 7.5 逐字移植）。
        //!
        //! 输出与 TS **逐字相等**（`fixtures/fieldgen/` 是对照件）：同样的元素顺序、同样的属性顺序、
        //! 同样的转义。这是**用户输入**的解析器，不是文档遍历，所以按 `spec/18` 的约定用递归下降 +
        //! 深度上限（256），超限 `Err(EDIT_MATH_TOO_DEEP)` 而不是写显式栈。

        use crate::diag::DiagCode;
        use crate::error::{Error, Result};

        use super::escape_text;

        /// 递归深度上限（用户输入，不是文档；`spec/18` 风险 11）。
        const MAX_DEPTH: usize = 256;

        fn err(msg: impl Into<String>) -> Error {
            Error::edit(DiagCode::EditMathBadLatex, msg)
        }

        fn too_deep() -> Error {
            Error::edit(DiagCode::EditMathTooDeep, "LaTeX 嵌套超过 256 层")
        }

        /// TS `escapeXmlAttr`。
        fn escape_attr(s: &str) -> String {
            escape_text(s).replace('"', "&quot;")
        }

        struct P<'a> {
            src: &'a [char],
            pos: usize,
            depth: usize,
        }

        impl P<'_> {
            fn peek(&self) -> char {
                self.src.get(self.pos).copied().unwrap_or('\0')
            }

            fn rest_starts_with(&self, pat: &str) -> bool {
                let p: Vec<char> = pat.chars().collect();
                self.src.len() >= self.pos + p.len()
                    && self.src[self.pos..self.pos + p.len()] == p[..]
            }

            fn skip_spaces(&mut self) {
                while self.peek().is_whitespace() {
                    self.pos += 1;
                }
            }

            fn slice(&self, from: usize, to: usize) -> String {
                self.src[from.min(self.src.len())..to.min(self.src.len())].iter().collect()
            }

            fn deeper(&mut self) -> Result<()> {
                self.depth += 1;
                if self.depth > MAX_DEPTH { Err(too_deep()) } else { Ok(()) }
            }
        }

        /// TS `latexToOmml`：整串 LaTeX → `m:oMath` 的**内容**（不含 `m:oMath` 本身）。
        pub fn latex_to_omml(latex: &str) -> Result<String> {
            let chars: Vec<char> = latex.chars().collect();
            let mut p = P { src: &chars, pos: 0, depth: 0 };
            let out = parse_sequence(&mut p, &|p: &P<'_>| p.pos >= p.src.len())?;
            if p.pos < p.src.len() {
                return Err(err(format!("Cannot parse: \"{}\"", p.slice(p.pos, p.pos + 12))));
            }
            Ok(out)
        }

        /// TS `mathParagraphXml`：编辑器新建的独立公式段。
        pub fn math_paragraph_xml(omml: &str, align: &str) -> String {
            let jc = if align == "center" {
                String::new()
            } else {
                format!(r#"<w:pPr><w:jc w:val="{}"/></w:pPr>"#, escape_attr(align))
            };
            format!(
                concat!(
                    r#"<w:p>{jc}<m:oMathPara><m:oMathParaPr><m:jc m:val="{align}"/></m:oMathParaPr>"#,
                    r#"<m:oMath>{omml}</m:oMath></m:oMathPara></w:p>"#
                ),
                jc = jc,
                align = escape_attr(align),
                omml = omml,
            )
        }

        /// TS `mathRun`。
        fn math_run(text: &str, plain: bool) -> String {
            if text.is_empty() {
                return String::new();
            }
            let rpr = if plain { r#"<m:rPr><m:sty m:val="p"/></m:rPr>"# } else { "" };
            format!(r#"<m:r>{rpr}<m:t xml:space="preserve">{}</m:t></m:r>"#, escape_text(text))
        }

        /// TS `readControlName`：反斜杠之后的命令名（字母串，否则单个字符）。
        fn read_control_name(p: &mut P<'_>) -> String {
            let start = p.pos;
            while p.src.get(p.pos).is_some_and(|c| c.is_ascii_alphabetic()) {
                p.pos += 1;
            }
            if p.pos > start {
                return p.slice(start, p.pos);
            }
            let ch = p.peek();
            p.pos += 1;
            if ch == '\0' { String::new() } else { ch.to_string() }
        }

        /// TS `parseGroup`：必需的 `{...}`，或者按 LaTeX 语义的**一个** token。
        fn parse_group(p: &mut P<'_>) -> Result<String> {
            p.skip_spaces();
            if p.peek() == '{' {
                p.pos += 1;
                p.deeper()?;
                let out = parse_sequence(p, &|p: &P<'_>| p.peek() == '}')?;
                p.depth -= 1;
                if p.peek() != '}' {
                    return Err(err("Missing matching }"));
                }
                p.pos += 1;
                return Ok(out);
            }
            if p.peek() == '\\' {
                p.pos += 1;
                p.deeper()?;
                let out = parse_control(p)?;
                p.depth -= 1;
                return Ok(out);
            }
            let ch = p.peek();
            if ch == '\0' || "{}^_&".contains(ch) {
                return Err(err("An argument is required here"));
            }
            p.pos += 1;
            Ok(math_run(&ch.to_string(), false))
        }

        /// TS `readBraceText`：`{...}` 的原文（`\text` / `\begin` 的名字）。
        fn read_brace_text(p: &mut P<'_>) -> Result<String> {
            p.skip_spaces();
            if p.peek() != '{' {
                return Err(err("Expected { here"));
            }
            p.pos += 1;
            let mut depth = 1usize;
            let mut out = String::new();
            while p.pos < p.src.len() {
                let ch = p.src[p.pos];
                p.pos += 1;
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                }
                if depth > 0 {
                    out.push(ch);
                }
            }
            Err(err("Missing matching }"))
        }

        type Stop<'s> = dyn Fn(&P<'_>) -> bool + 's;

        /// TS `parseSequence`：一串原子，`^` / `_` 作用在前一个原子上。
        fn parse_sequence(p: &mut P<'_>, stop: &Stop<'_>) -> Result<String> {
            let mut atoms: Vec<String> = Vec::new();
            loop {
                p.skip_spaces();
                if p.pos >= p.src.len() || stop(p) {
                    break;
                }
                let ch = p.peek();
                if ch == '^' || ch == '_' {
                    p.pos += 1;
                    let script = parse_group(p)?;
                    let other = p.peek();
                    let base = atoms.pop().unwrap_or_else(|| math_run("", false));
                    if (other == '^' || other == '_') && other != ch {
                        p.pos += 1;
                        let second = parse_group(p)?;
                        let (sub, sup) =
                            if ch == '_' { (script, second) } else { (second, script) };
                        atoms.push(format!(
                            "<m:sSubSup><m:e>{base}</m:e><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup></m:sSubSup>"
                        ));
                    } else if ch == '^' {
                        atoms.push(format!(
                            "<m:sSup><m:e>{base}</m:e><m:sup>{script}</m:sup></m:sSup>"
                        ));
                    } else {
                        atoms.push(format!(
                            "<m:sSub><m:e>{base}</m:e><m:sub>{script}</m:sub></m:sSub>"
                        ));
                    }
                    continue;
                }
                atoms.push(parse_atom(p)?);
            }
            Ok(atoms.concat())
        }

        /// TS `parseAtom`。
        fn parse_atom(p: &mut P<'_>) -> Result<String> {
            p.skip_spaces();
            let ch = p.peek();
            if ch == '\0' {
                return Ok(String::new());
            }
            if ch == '{' {
                return parse_group(p);
            }
            if ch == '}' {
                return Err(err("Unexpected }"));
            }
            if ch == '\\' {
                p.pos += 1;
                p.deeper()?;
                let out = parse_control(p)?;
                p.depth -= 1;
                return Ok(out);
            }
            let start = p.pos;
            while p.pos < p.src.len() {
                let c = p.src[p.pos];
                if "\\{}^_&".contains(c) || c == '\n' {
                    break;
                }
                p.pos += 1;
            }
            let text = p.slice(start, p.pos);
            if text.is_empty() {
                return Err(err(format!("Cannot parse: \"{ch}\"")));
            }
            // 紧跟的上下标只作用在**最后一个字符**上（"ab^2" = a·b²）：退回去让它自成一个原子
            let chars: Vec<char> = text.chars().collect();
            if (p.peek() == '^' || p.peek() == '_') && chars.len() > 1 {
                p.pos -= 1;
                return Ok(math_run(&chars[..chars.len() - 1].iter().collect::<String>(), false));
            }
            Ok(math_run(&text, false))
        }

        /// TS `naryOmml`。
        fn nary_omml(p: &mut P<'_>, chr: &str, lim_loc: &str) -> Result<String> {
            let (mut sub, mut sup) = (String::new(), String::new());
            for _ in 0..2 {
                p.skip_spaces();
                let ch = p.peek();
                if ch == '_' && sub.is_empty() {
                    p.pos += 1;
                    sub = parse_group(p)?;
                } else if ch == '^' && sup.is_empty() {
                    p.pos += 1;
                    sup = parse_group(p)?;
                } else {
                    break;
                }
            }
            p.skip_spaces();
            let operand = if p.peek() == '{' { parse_group(p)? } else { String::new() };
            let pr = format!(
                r#"<m:naryPr><m:chr m:val="{}"/><m:limLoc m:val="{lim_loc}"/>{}{}</m:naryPr>"#,
                escape_attr(chr),
                if sub.is_empty() { r#"<m:subHide m:val="1"/>"# } else { "" },
                if sup.is_empty() { r#"<m:supHide m:val="1"/>"# } else { "" },
            );
            Ok(format!(
                "<m:nary>{pr}{}{}<m:e>{operand}</m:e></m:nary>",
                if sub.is_empty() { String::new() } else { format!("<m:sub>{sub}</m:sub>") },
                if sup.is_empty() { String::new() } else { format!("<m:sup>{sup}</m:sup>") },
            ))
        }

        /// TS `matrixOmml`。
        fn matrix_omml(p: &mut P<'_>, env: &str) -> Result<String> {
            let delims = matrix_delims(env).expect("caller checked the environment");
            let mut rows: Vec<Vec<String>> = vec![Vec::new()];
            loop {
                let cell = parse_sequence(p, &|p: &P<'_>| {
                    p.peek() == '&' || p.rest_starts_with("\\\\") || p.rest_starts_with("\\end")
                })?;
                rows.last_mut().expect("never empty").push(cell);
                if p.peek() == '&' {
                    p.pos += 1;
                } else if p.rest_starts_with("\\\\") {
                    p.pos += 2;
                    rows.push(Vec::new());
                } else if p.rest_starts_with("\\end") {
                    p.pos += 4;
                    let closing = read_brace_text(p)?;
                    if closing != env {
                        return Err(err(format!(
                            "\\end{{{closing}}} does not match \\begin{{{env}}}"
                        )));
                    }
                    break;
                } else {
                    return Err(err(format!("\\begin{{{env}}} is missing \\end{{{env}}}")));
                }
            }
            let body: String = rows
                .iter()
                .filter(|row| row.len() > 1 || row.first().is_some_and(|c| !c.is_empty()))
                .map(|row| {
                    let cells: String = row.iter().map(|c| format!("<m:e>{c}</m:e>")).collect();
                    format!("<m:mr>{cells}</m:mr>")
                })
                .collect();
            let matrix = format!("<m:m>{body}</m:m>");
            let Some((beg, end)) = delims else { return Ok(matrix) };
            Ok(format!(
                concat!(
                    r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
                    "</m:dPr><m:e>{matrix}</m:e></m:d>"
                ),
                beg = escape_attr(beg),
                end = escape_attr(end),
                matrix = matrix,
            ))
        }

        /// TS `readDelimiter`。
        fn read_delimiter(p: &mut P<'_>) -> Result<String> {
            p.skip_spaces();
            if p.peek() == '\\' {
                let start = p.pos;
                p.pos += 1;
                let name = read_control_name(p);
                if let Some(ch) = left_right_char(&format!("\\{name}")) {
                    return Ok(ch.to_string());
                }
                p.pos = start;
                return Err(err(format!("Unsupported delimiter: \\{name}")));
            }
            let ch = p.peek();
            if let Some(mapped) = left_right_char(&ch.to_string()) {
                p.pos += 1;
                return Ok(mapped.to_string());
            }
            Err(err(format!("Unsupported delimiter: \"{ch}\"")))
        }

        /// TS `parseControl`。
        fn parse_control(p: &mut P<'_>) -> Result<String> {
            let name = read_control_name(p);
            if let Some(ch) = super::latex::symbol_char(&name) {
                return Ok(math_run(&ch.to_string(), false));
            }
            if let Some((chr, lim_loc)) = nary_op(&name) {
                return nary_omml(p, &chr.to_string(), lim_loc);
            }
            if let Some(ch) = super::latex::accent_char(&name) {
                let base = parse_group(p)?;
                return Ok(format!(
                    r#"<m:acc><m:accPr><m:chr m:val="{}"/></m:accPr><m:e>{base}</m:e></m:acc>"#,
                    escape_attr(&ch.to_string())
                ));
            }
            if super::latex::is_latex_function(&name) {
                return Ok(math_run(&name, true));
            }
            match name.as_str() {
                "frac" | "dfrac" | "tfrac" => {
                    let num = parse_group(p)?;
                    let den = parse_group(p)?;
                    Ok(format!("<m:f><m:num>{num}</m:num><m:den>{den}</m:den></m:f>"))
                }
                "binom" => {
                    let top = parse_group(p)?;
                    let bottom = parse_group(p)?;
                    Ok(format!(
                        concat!(
                            r#"<m:d><m:e><m:f><m:fPr><m:type m:val="noBar"/></m:fPr>"#,
                            "<m:num>{top}</m:num><m:den>{bottom}</m:den></m:f></m:e></m:d>"
                        ),
                        top = top,
                        bottom = bottom
                    ))
                }
                "sqrt" => {
                    p.skip_spaces();
                    let mut deg = String::new();
                    if p.peek() == '[' {
                        // 普通字符串不会在 ']' 停下：把次数的源码单独切出来当一段解析
                        p.pos += 1;
                        let close = (p.pos..p.src.len()).find(|&i| p.src[i] == ']');
                        let Some(close) = close else { return Err(err("Missing matching ]")) };
                        let inner: Vec<char> = p.src[p.pos..close].to_vec();
                        let mut sub = P { src: &inner, pos: 0, depth: p.depth };
                        deg = parse_sequence(&mut sub, &|q: &P<'_>| q.pos >= q.src.len())?;
                        p.pos = close + 1;
                    }
                    let inner = parse_group(p)?;
                    if deg.is_empty() {
                        return Ok(format!(
                            r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e>{inner}</m:e></m:rad>"#
                        ));
                    }
                    Ok(format!("<m:rad><m:deg>{deg}</m:deg><m:e>{inner}</m:e></m:rad>"))
                }
                "overline" => Ok(format!(
                    r#"<m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e>{}</m:e></m:bar>"#,
                    parse_group(p)?
                )),
                "underline" => Ok(format!(
                    r#"<m:bar><m:barPr><m:pos m:val="bot"/></m:barPr><m:e>{}</m:e></m:bar>"#,
                    parse_group(p)?
                )),
                "underbrace" => Ok(format!(
                    concat!(
                        r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏟"/><m:pos m:val="bot"/></m:groupChrPr>"#,
                        "<m:e>{}</m:e></m:groupChr>"
                    ),
                    parse_group(p)?
                )),
                "overbrace" => Ok(format!(
                    concat!(
                        r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏞"/><m:pos m:val="top"/></m:groupChrPr>"#,
                        "<m:e>{}</m:e></m:groupChr>"
                    ),
                    parse_group(p)?
                )),
                "text" | "mathrm" | "operatorname" => {
                    let t = read_brace_text(p)?;
                    Ok(math_run(&t, true))
                }
                "lim" => {
                    p.skip_spaces();
                    if p.peek() == '_' {
                        p.pos += 1;
                        let lim = parse_group(p)?;
                        return Ok(format!(
                            "<m:limLow><m:e>{}</m:e><m:lim>{lim}</m:lim></m:limLow>",
                            math_run("lim", true)
                        ));
                    }
                    Ok(math_run("lim", true))
                }
                "left" => {
                    let beg = read_delimiter(p)?;
                    p.deeper()?;
                    let body = parse_sequence(p, &|p: &P<'_>| p.rest_starts_with("\\right"))?;
                    p.depth -= 1;
                    if !p.rest_starts_with("\\right") {
                        return Err(err("\\left is missing a matching \\right"));
                    }
                    p.pos += "\\right".chars().count();
                    let end = read_delimiter(p)?;
                    Ok(format!(
                        concat!(
                            r#"<m:d><m:dPr><m:begChr m:val="{beg}"/><m:endChr m:val="{end}"/>"#,
                            "</m:dPr><m:e>{body}</m:e></m:d>"
                        ),
                        beg = escape_attr(&beg),
                        end = escape_attr(&end),
                        body = body,
                    ))
                }
                "begin" => {
                    let env = read_brace_text(p)?;
                    if matrix_delims(&env).is_none() {
                        return Err(err(format!("Unsupported environment: \\begin{{{env}}}")));
                    }
                    p.deeper()?;
                    let out = matrix_omml(p, &env)?;
                    p.depth -= 1;
                    Ok(out)
                }
                "," | ";" | " " | "quad" | "qquad" => Ok(math_run(" ", false)),
                "\\" => Err(err("\\\\ is only allowed inside matrix environments")),
                "{" => Ok(math_run("{", false)),
                "}" => Ok(math_run("}", false)),
                "%" | "&" | "$" | "#" | "_" | "^" => Ok(math_run(&name, false)),
                other => Err(err(format!("Unsupported command: \\{other}"))),
            }
        }

        /// TS `NARY_OPS`：符号取自 `latex.rs` 的同一张表（`nary_char`），这里只补 `limLoc`。
        fn nary_op(name: &str) -> Option<(char, &'static str)> {
            let chr = super::latex::nary_char(name)?;
            let lim_loc = match name {
                "int" | "iint" | "iiint" | "oint" => "subSup",
                _ => "undOvr",
            };
            Some((chr, lim_loc))
        }

        /// TS `MATRIX_DELIMS`：外层 `None` = 不是矩阵环境，内层 `None` = 没有定界符。
        fn matrix_delims(env: &str) -> Option<Option<(&'static str, &'static str)>> {
            Some(match env {
                "matrix" => None,
                "pmatrix" => Some(("(", ")")),
                "bmatrix" => Some(("[", "]")),
                "Bmatrix" => Some(("{", "}")),
                "vmatrix" => Some(("|", "|")),
                "Vmatrix" => Some(("‖", "‖")),
                "cases" => Some(("{", "")),
                _ => return None,
            })
        }

        /// TS `LEFT_RIGHT_CHARS`。
        fn left_right_char(key: &str) -> Option<&'static str> {
            Some(match key {
                "(" => "(",
                ")" => ")",
                "[" => "[",
                "]" => "]",
                "|" => "|",
                "." => "",
                "\\{" => "{",
                "\\}" => "}",
                "\\|" => "‖",
                "\\langle" => "⟨",
                "\\rangle" => "⟩",
                "\\lfloor" => "⌊",
                "\\rfloor" => "⌋",
                "\\lceil" => "⌈",
                "\\rceil" => "⌉",
                _ => return None,
            })
        }
    }
    pub mod mathml {
        //! OMML → MathML Core（TS `ommlToMathML`，`math.ts` 55–376 的逐字移植）。
        //!
        //! 迭代求值：`Item` 是一个待求值的项（元素 / 槽位 / 行 …），任务栈里 `Eval(item)` 展开出子项与一个
        //! `Finish(item, arity)`；`Finish` 从结果栈取回 `arity` 个子结果拼成自己的字串。一个 3,000 层的公式
        //! 只是一个长一点的栈。

        use super::{
            child, children_named, content_children, escape_text, is_plain_run, prop_on, prop_val,
            text_of,
        };
        use crate::xml::{Dom, LocalName, NodeId, NsId};

        /// 一个 `m:oMath` → `<math display="block"><mrow>…</mrow></math>`；一个内容都没有 → `""`。
        pub fn to_mathml(dom: &Dom, omath: NodeId) -> String {
            let body = eval(dom, Item::Seq(omath));
            if body.is_empty() {
                String::new()
            } else {
                format!("<math display=\"block\"><mrow>{body}</mrow></math>")
            }
        }

        #[derive(Clone, Copy)]
        enum Item {
            /// 一个 OMML 元素。
            Node(NodeId),
            /// `parent/m:<name>` 槽位 → `<mrow>内容</mrow>`；缺失 → `<mrow></mrow>`。
            Slot(NodeId, LocalName),
            /// 某元素的内容子节点包成 `<mrow>`（`m:d` / `m:m` 的 `m:e`）。
            Row(NodeId),
            /// `m:mr` → `<mtr><mtd>…</mtd>…</mtr>`。
            Cells(NodeId),
            /// `m:eqArr/m:e` → `<mtr><mtd><mrow>…</mrow></mtd></mtr>`。
            EqRow(NodeId),
            /// 内容子节点直接拼接，不包。
            Seq(NodeId),
        }

        enum Task {
            Eval(Item),
            Finish(Item, usize),
        }

        fn mo(ch: &str, extra: &str) -> String {
            format!("<mo{extra}>{}</mo>", escape_text(ch))
        }

        fn eval(dom: &Dom, root: Item) -> String {
            let mut tasks = vec![Task::Eval(root)];
            let mut results: Vec<String> = Vec::new();
            while let Some(task) = tasks.pop() {
                match task {
                    Task::Eval(item) => {
                        let subs = expand(dom, item, &mut results);
                        if let Some(subs) = subs {
                            tasks.push(Task::Finish(item, subs.len()));
                            tasks.extend(subs.into_iter().rev().map(Task::Eval));
                        }
                    }
                    Task::Finish(item, arity) => {
                        let at = results.len() - arity;
                        let parts: Vec<String> = results.drain(at..).collect();
                        results.push(finish(dom, item, parts));
                    }
                }
            }
            results.pop().unwrap_or_default()
        }

        /// 展开一个项：叶子直接把结果压栈并返回 `None`；否则返回子项。
        fn expand(dom: &Dom, item: Item, results: &mut Vec<String>) -> Option<Vec<Item>> {
            let slot = |n: NodeId, l: LocalName| Item::Slot(n, l);
            let rows = |n: NodeId| -> Vec<Item> {
                content_children(dom, n).into_iter().map(Item::Node).collect()
            };
            match item {
                Item::Slot(parent, name) => match child(dom, parent, name) {
                    None => {
                        results.push("<mrow></mrow>".to_string());
                        None
                    }
                    Some(s) => Some(rows(s)),
                },
                Item::Row(n) | Item::Seq(n) => Some(rows(n)),
                Item::Cells(mr) => {
                    Some(children_named(dom, mr, LocalName::E).into_iter().map(Item::Row).collect())
                }
                Item::EqRow(e) => Some(vec![Item::Row(e)]),
                Item::Node(n) => {
                    let Some(name) = dom.name(n) else {
                        results.push(String::new());
                        return None;
                    };
                    if name.ns != NsId::M {
                        // 不认识的结构：渲染它的内容子节点，别让东西凭空消失
                        return Some(rows(n));
                    }
                    Some(match name.local {
                        LocalName::R => {
                            results.push(run_to_mml(dom, n));
                            return None;
                        }
                        LocalName::T => {
                            results.push(run_text_to_mml(&text_of(dom, n), false));
                            return None;
                        }
                        LocalName::F => vec![slot(n, LocalName::Num), slot(n, LocalName::Den)],
                        LocalName::SSup => vec![slot(n, LocalName::E), slot(n, LocalName::Sup)],
                        LocalName::SSub => vec![slot(n, LocalName::E), slot(n, LocalName::Sub)],
                        LocalName::SSubSup | LocalName::SPre => {
                            vec![
                                slot(n, LocalName::E),
                                slot(n, LocalName::Sub),
                                slot(n, LocalName::Sup),
                            ]
                        }
                        LocalName::Rad => {
                            if prop_on(dom, n, LocalName::RadPr, LocalName::DegHide)
                                || child(dom, n, LocalName::Deg).is_none()
                            {
                                vec![slot(n, LocalName::E)]
                            } else {
                                vec![slot(n, LocalName::E), slot(n, LocalName::Deg)]
                            }
                        }
                        LocalName::D => children_named(dom, n, LocalName::E)
                            .into_iter()
                            .map(Item::Row)
                            .collect(),
                        LocalName::Nary => {
                            vec![
                                slot(n, LocalName::Sub),
                                slot(n, LocalName::Sup),
                                slot(n, LocalName::E),
                            ]
                        }
                        LocalName::Func => vec![slot(n, LocalName::FName), slot(n, LocalName::E)],
                        LocalName::LimLow | LocalName::LimUpp => {
                            vec![slot(n, LocalName::E), slot(n, LocalName::Lim)]
                        }
                        LocalName::Acc | LocalName::Bar | LocalName::GroupChr => {
                            vec![slot(n, LocalName::E)]
                        }
                        LocalName::M => children_named(dom, n, LocalName::Mr)
                            .into_iter()
                            .map(Item::Cells)
                            .collect(),
                        LocalName::EqArr => children_named(dom, n, LocalName::E)
                            .into_iter()
                            .map(Item::EqRow)
                            .collect(),
                        LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                            vec![slot(n, LocalName::E)]
                        }
                        _ => rows(n),
                    })
                }
            }
        }

        fn finish(dom: &Dom, item: Item, parts: Vec<String>) -> String {
            let joined = || parts.concat();
            let p = |i: usize| parts.get(i).map(String::as_str).unwrap_or("");
            match item {
                Item::Slot(..) | Item::Row(_) => format!("<mrow>{}</mrow>", joined()),
                Item::Seq(_) => joined(),
                Item::Cells(_) => {
                    let cells: String = parts.iter().map(|c| format!("<mtd>{c}</mtd>")).collect();
                    format!("<mtr>{cells}</mtr>")
                }
                Item::EqRow(_) => format!("<mtr><mtd>{}</mtd></mtr>", p(0)),
                Item::Node(n) => {
                    let Some(name) = dom.name(n) else { return String::new() };
                    if name.ns != NsId::M {
                        return joined();
                    }
                    match name.local {
                        LocalName::F => {
                            let attrs = match prop_val(dom, n, LocalName::FPr, LocalName::Type)
                                .as_deref()
                            {
                                Some("noBar") => " linethickness=\"0\"",
                                Some("lin" | "skw") => " bevelled=\"true\"",
                                _ => "",
                            };
                            format!("<mfrac{attrs}>{}{}</mfrac>", p(0), p(1))
                        }
                        LocalName::SSup => format!("<msup>{}{}</msup>", p(0), p(1)),
                        LocalName::SSub => format!("<msub>{}{}</msub>", p(0), p(1)),
                        LocalName::SSubSup => {
                            format!("<msubsup>{}{}{}</msubsup>", p(0), p(1), p(2))
                        }
                        LocalName::SPre => {
                            format!(
                                "<mmultiscripts>{}<mprescripts/>{}{}</mmultiscripts>",
                                p(0),
                                p(1),
                                p(2)
                            )
                        }
                        LocalName::Rad => {
                            if parts.len() == 1 {
                                format!("<msqrt>{}</msqrt>", p(0))
                            } else {
                                format!("<mroot>{}{}</mroot>", p(0), p(1))
                            }
                        }
                        LocalName::D => {
                            let beg = prop_val(dom, n, LocalName::DPr, LocalName::BegChr)
                                .unwrap_or_else(|| "(".into());
                            let end = prop_val(dom, n, LocalName::DPr, LocalName::EndChr)
                                .unwrap_or_else(|| ")".into());
                            let sep = prop_val(dom, n, LocalName::DPr, LocalName::SepChr)
                                .unwrap_or_else(|| "|".into());
                            let sep_mo = if sep.is_empty() { String::new() } else { mo(&sep, "") };
                            let body = parts.join(&sep_mo);
                            let open = if beg.is_empty() {
                                String::new()
                            } else {
                                mo(&beg, " stretchy=\"true\"")
                            };
                            let close = if end.is_empty() {
                                String::new()
                            } else {
                                mo(&end, " stretchy=\"true\"")
                            };
                            format!("<mrow>{open}{body}{close}</mrow>")
                        }
                        LocalName::Nary => {
                            let chr = prop_val(dom, n, LocalName::NaryPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{222B}".into());
                            let lim_loc = prop_val(dom, n, LocalName::NaryPr, LocalName::LimLoc)
                                .unwrap_or_else(|| {
                                    if chr == "\u{222B}" {
                                        "subSup".into()
                                    } else {
                                        "undOvr".into()
                                    }
                                });
                            let sub_hide = prop_on(dom, n, LocalName::NaryPr, LocalName::SubHide);
                            let sup_hide = prop_on(dom, n, LocalName::NaryPr, LocalName::SupHide);
                            let op = mo(&chr, " stretchy=\"false\"");
                            let und_ovr = lim_loc == "undOvr";
                            let scripted = match (sub_hide, sup_hide) {
                                (false, false) => {
                                    let tag = if und_ovr { "munderover" } else { "msubsup" };
                                    format!("<{tag}>{op}{}{}</{tag}>", p(0), p(1))
                                }
                                (false, true) => {
                                    let tag = if und_ovr { "munder" } else { "msub" };
                                    format!("<{tag}>{op}{}</{tag}>", p(0))
                                }
                                (true, false) => {
                                    let tag = if und_ovr { "mover" } else { "msup" };
                                    format!("<{tag}>{op}{}</{tag}>", p(1))
                                }
                                (true, true) => op,
                            };
                            format!("<mrow>{scripted}{}</mrow>", p(2))
                        }
                        LocalName::Func => {
                            format!("<mrow>{}<mo>\u{2061}</mo>{}</mrow>", p(0), p(1))
                        }
                        LocalName::LimLow => format!("<munder>{}{}</munder>", p(0), p(1)),
                        LocalName::LimUpp => format!("<mover>{}{}</mover>", p(0), p(1)),
                        LocalName::Acc => {
                            let chr = prop_val(dom, n, LocalName::AccPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{0302}".into());
                            format!("<mover accent=\"true\">{}{}</mover>", p(0), mo(&chr, ""))
                        }
                        LocalName::Bar => {
                            let top = prop_val(dom, n, LocalName::BarPr, LocalName::Pos).as_deref()
                                == Some("top");
                            let (tag, line) =
                                if top { ("mover", "\u{00AF}") } else { ("munder", "\u{005F}") };
                            format!("<{tag}>{}{}</{tag}>", p(0), mo(line, " stretchy=\"true\""))
                        }
                        LocalName::GroupChr => {
                            let chr = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Chr)
                                .unwrap_or_else(|| "\u{23DF}".into());
                            let top = prop_val(dom, n, LocalName::GroupChrPr, LocalName::Pos)
                                .as_deref()
                                == Some("top");
                            let tag = if top { "mover" } else { "munder" };
                            format!("<{tag}>{}{}</{tag}>", p(0), mo(&chr, " stretchy=\"true\""))
                        }
                        LocalName::M | LocalName::EqArr => format!("<mtable>{}</mtable>", joined()),
                        LocalName::Box | LocalName::BorderBox | LocalName::Phant => {
                            p(0).to_string()
                        }
                        _ => joined(),
                    }
                }
            }
        }

        /// `m:r` → 各 `m:t` 分类后的 token 串（`sty="p"` / `m:nor` 的 run 整段是 `<mi>`）。
        fn run_to_mml(dom: &Dom, run: NodeId) -> String {
            let plain = is_plain_run(dom, run);
            children_named(dom, run, LocalName::T)
                .iter()
                .map(|&t| run_text_to_mml(&text_of(dom, t), plain))
                .collect()
        }

        /// TS `OPERATOR_CHARS`。
        const OPERATOR_CHARS: &str = "+-−=<>±∓×÷·⋅∙*/!%&|,;:()[]{}′″∞→←↔⇒⇐⇔∈∉⊂⊃∪∩∀∃∧∨¬≤≥≠≈≡∼∝⊥∥°∂∇";

        fn is_letter(ch: char) -> bool {
            ch.is_ascii_alphabetic()
                || ('\u{0370}'..='\u{03FF}').contains(&ch)
                || ('\u{1D400}'..='\u{1D7FF}').contains(&ch)
        }

        /// 一段 run 文字 → `mn / mi / mo / mtext`（TS `runTextToMml`）。
        pub(crate) fn run_text_to_mml(text: &str, plain: bool) -> String {
            if plain {
                return if text.is_empty() {
                    String::new()
                } else {
                    format!("<mi>{}</mi>", escape_text(text))
                };
            }
            let chars: Vec<char> = text.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            while i < chars.len() {
                let ch = chars[i];
                if ch.is_ascii_digit() || ch == '.' {
                    let mut num = String::new();
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                        num.push(chars[i]);
                        i += 1;
                    }
                    out.push_str(&format!("<mn>{num}</mn>"));
                } else if is_letter(ch) {
                    out.push_str(&format!("<mi>{}</mi>", escape_text(&ch.to_string())));
                    i += 1;
                } else if ch == ' ' {
                    i += 1;
                } else if OPERATOR_CHARS.contains(ch) {
                    // 普通 run 里的括号是字面字符，只有 m:d 包的定界符才可伸缩
                    let s = ch.to_string();
                    out.push_str(&if "()[]{}|".contains(ch) {
                        mo(&s, " stretchy=\"false\"")
                    } else {
                        mo(&s, "")
                    });
                    i += 1;
                } else {
                    out.push_str(&format!("<mtext>{}</mtext>", escape_text(&ch.to_string())));
                    i += 1;
                }
            }
            out
        }
    }

    use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

    pub(crate) fn m(local: LocalName) -> QName {
        QName::new(NsId::M, local)
    }

    /// 第一个名为 `m:<local>` 的语义子节点。
    pub(crate) fn child(dom: &Dom, node: NodeId, local: LocalName) -> Option<NodeId> {
        dom.semantic_children(node).find(|&c| dom.is(c, m(local)))
    }

    /// 全部名为 `m:<local>` 的语义子节点，文档序。
    pub(crate) fn children_named(dom: &Dom, node: NodeId, local: LocalName) -> Vec<NodeId> {
        dom.semantic_children(node).filter(|&c| dom.is(c, m(local))).collect()
    }

    /// 内容子节点：元素，且名字不以 `Pr` 结尾（TS `contentChildren`：属性包不是内容）。
    pub(crate) fn content_children(dom: &Dom, node: NodeId) -> Vec<NodeId> {
        dom.semantic_children(node)
            .filter(|&c| dom.name(c).is_some())
            .filter(|&c| !dom.lex_name(c).is_some_and(|q| q.ends_with("Pr")))
            .collect()
    }

    /// `node/m:<pr>/m:<child>/@m:val`（TS `propVal`）。
    pub(crate) fn prop_val(
        dom: &Dom,
        node: NodeId,
        pr: LocalName,
        name: LocalName,
    ) -> Option<String> {
        let pr = child(dom, node, pr)?;
        let c = child(dom, pr, name)?;
        dom.attr_value(c, m(LocalName::Val)).map(|v| v.into_owned())
    }

    /// 属性存在且不是 `0` / `false` / `off`（TS `propOn`）。
    pub(crate) fn prop_on(dom: &Dom, node: NodeId, pr: LocalName, name: LocalName) -> bool {
        prop_val(dom, node, pr, name)
            .is_some_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
    }

    /// 一个 `m:t` 的文本（实体已解码）。
    pub(crate) fn text_of(dom: &Dom, node: NodeId) -> String {
        let mut s = String::new();
        for c in dom.semantic_children(node) {
            if let Some(t) = dom.text(c) {
                s.push_str(&t);
            }
        }
        s
    }

    /// 一个 `m:r` 的全部 `m:t` 文本拼接。
    pub(crate) fn run_text(dom: &Dom, run: NodeId) -> String {
        children_named(dom, run, LocalName::T).iter().map(|&t| text_of(dom, t)).collect()
    }

    /// `m:rPr/m:sty = "p"` 或有 `m:rPr/m:nor`：普通文字（不按数学斜体分类）。
    pub(crate) fn is_plain_run(dom: &Dom, run: NodeId) -> bool {
        let sty = prop_val(dom, run, LocalName::RPr, LocalName::Sty);
        sty.as_deref() == Some("p")
            || child(dom, run, LocalName::RPr)
                .is_some_and(|pr| child(dom, pr, LocalName::Nor).is_some())
    }

    /// 容器下全部 `m:r` 的文字拼接（TS `plainTextOfRuns`：函数名 / `lim`）。
    pub(crate) fn plain_text_of_runs(dom: &Dom, node: Option<NodeId>) -> String {
        let Some(node) = node else { return String::new() };
        children_named(dom, node, LocalName::R).iter().map(|&r| run_text(dom, r)).collect()
    }

    /// TS `escapeXmlText`：去掉 XML 1.0 不允许的控制字符，转义 `& < >`。
    pub(crate) fn escape_text(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' => {}
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                c => out.push(c),
            }
        }
        out
    }

    /// 一个节点下全部 `m:oMath` 片段，文档序（`m:oMathPara` 展开；`m:oMath` 不嵌套）。
    pub fn fragments(dom: &Dom, node: NodeId) -> Vec<NodeId> {
        dom.semantic_descendants(node).filter(|&n| dom.is(n, m(LocalName::OMath))).collect()
    }

    /// 片段里全部 `m:t` 的文本，文档序（TS `mathTokens`：可编辑的公式 token）。
    pub fn tokens(dom: &Dom, omath: NodeId) -> Vec<String> {
        dom.semantic_descendants(omath)
            .filter(|&n| dom.is(n, m(LocalName::T)))
            .map(|t| text_of(dom, t))
            .collect()
    }

    pub use latex_to_omml::{latex_to_omml, math_paragraph_xml};

    /// OMML 的命名空间 URI（Transitional 与 Strict 相同）。
    pub const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
}
pub mod revision {
    //! 跨 part 的修订索引（`MOD-09`、`EDIT-06`；`spec/18` 7.1）。
    //!
    //! `Block.revisions` / `Run.rev` / `Row` / `Cell` / `SectionInfo` 上的修订是**投影**：run 级的
    //! [`crate::model::RevisionCtx`] 每种只留一格，包裹套娃会被压平，内联容器超过深度上限的整段还会降级。
    //! 接受 / 拒绝修订要动的是每一层承载元素本身，所以这张索引直接走 DOM：
    //!
    //! - **扫全部未删节点**，包括 `mc:Choice` / `mc:Fallback` 两支——修订 `w:id` 的唯一性是整个包的事，
    //!   与 MCE 选哪支无关（与 [`crate::edit::media_ops`] 的 `wp:docPr/@id` 同一条理由）。
    //! - **迭代遍历**（`rev-nested-wrappers` 是 500 层 `w:ins` / `w:del` 交替）。
    //! - 文档序 = part 顺序（主 part → 页眉页脚 → 脚注 → 尾注 → 批注 → 外部文本框）内各自的前序。

    use std::collections::BTreeMap;

    use crate::diag::{DiagCode, Diagnostic};
    use crate::model::RevisionMeta;
    use crate::model::named_enum;
    use crate::package::PartId;
    use crate::span::field::{FieldId, FieldIndex};
    use crate::xml::{Dirty, Dom, LocalName, NodeId, NsId, QName};

    /// 修订的会话内稳定 id（`MOD-13`）。
    ///
    /// [`crate::model::Document::rebuild`] 从 0 起按文档序编号；[`crate::edit::EditSession`] 随后把仍然
    /// 存在的承载节点换回它上次拿到的 id（arena 里 `NodeId` 稳定），新节点才拿新号。
    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Hash,
        PartialOrd,
        Ord,
        ::serde::Serialize,
        ::serde::Deserialize,
    )]
    #[serde(transparent)]
    pub struct RevisionId(pub u32);

    named_enum! {
        /// 修订种类：`MOD-09` 的 16 种 + run 级 5 种 + `w:tblPrExChange`。
        ///
        /// `w:tblPrExChange` 不在 `MOD-09` 的清单里，但真实 Word 的表格修订会写它
        /// （`fixtures/revisions/table-and-move/tracked.docx` 有 3 处），接受 / 拒绝必须认它，
        /// 否则门第 3 条过不去。登记在 `docs/04` §8。
        pub enum RevKind {
            /// 块级 `w:ins`（含 `trPr/w:ins` 的整行插入）。
            Insert = "insert",
            /// 块级 `w:del`（含 `trPr/w:del`）。
            Delete = "delete",
            MoveFrom = "moveFrom",
            MoveTo = "moveTo",
            /// `pPr/rPr/w:ins`。
            ParaMarkInsert = "paraMarkInsert",
            /// `pPr/rPr/w:del`。
            ParaMarkDelete = "paraMarkDelete",
            /// `pPr/rPr/w:moveFrom`：段落标记随内容被搬走（接受后与下一段合并，同 `ParaMarkDelete`，
            /// 但要与 `ParaMarkMoveTo` 配对）。
            ParaMarkMoveFrom = "paraMarkMoveFrom",
            /// `pPr/rPr/w:moveTo`。
            ParaMarkMoveTo = "paraMarkMoveTo",
            ParaPropsChange = "paraPropsChange",
            NumberingChange = "numberingChange",
            TablePropsChange = "tablePropsChange",
            /// `w:tr/w:tblPrEx/w:tblPrExChange`。
            TablePropsExChange = "tablePropsExChange",
            SectPropsChange = "sectPropsChange",
            TableGridChange = "tableGridChange",
            RowPropsChange = "rowPropsChange",
            CellPropsChange = "cellPropsChange",
            CellInsert = "cellInsert",
            CellDelete = "cellDelete",
            CellMerge = "cellMerge",
            /// run 级 `w:ins`。
            RunInsert = "runInsert",
            /// run 级 `w:del`。
            RunDelete = "runDelete",
            RunMoveFrom = "runMoveFrom",
            RunMoveTo = "runMoveTo",
            /// `w:rPrChange`（段落标记的 `rPr` 里的那个也算这一种，owner 不同）。
            RunPropsChange = "runPropsChange",
        }
    }

    impl RevKind {
        /// 是不是内联（run 级）包裹。
        pub const fn is_run_level(self) -> bool {
            matches!(self, Self::RunInsert | Self::RunDelete | Self::RunMoveFrom | Self::RunMoveTo)
        }

        /// 是不是「内容包裹」（`w:ins` / `w:del` / `w:moveFrom` / `w:moveTo`，块级或 run 级）。
        pub const fn is_wrapper(self) -> bool {
            matches!(
                self,
                Self::Insert
                    | Self::Delete
                    | Self::MoveFrom
                    | Self::MoveTo
                    | Self::RunInsert
                    | Self::RunDelete
                    | Self::RunMoveFrom
                    | Self::RunMoveTo
            )
        }

        /// 搬移的**内容**一半（要配对）。
        ///
        /// 段落标记上的 `w:moveFrom` / `w:moveTo` 不算：真实 Word 把它写在范围标记**之外**
        /// （`corpus/real/revisions2/rev-move.docx` 的 `w:moveFrom w:id="0"` 在
        /// `w:moveFromRangeStart` 之前），按 `@w:name` 配不上；而且标记的接受 / 拒绝
        /// 与 `ParaMarkDelete` / `ParaMarkInsert` 完全一样，本来就不需要孪生。
        pub const fn is_move(self) -> bool {
            matches!(self, Self::MoveFrom | Self::MoveTo | Self::RunMoveFrom | Self::RunMoveTo)
        }

        /// 搬移的**来源**半边。
        pub const fn is_move_from(self) -> bool {
            matches!(self, Self::MoveFrom | Self::RunMoveFrom)
        }

        /// 段落标记上的修订（`pPr/rPr` 里那一层）。搬移配对时内容与标记各配各的。
        pub const fn is_para_mark(self) -> bool {
            matches!(
                self,
                Self::ParaMarkInsert
                    | Self::ParaMarkDelete
                    | Self::ParaMarkMoveFrom
                    | Self::ParaMarkMoveTo
            )
        }

        /// `*PrChange` 一族：内层是旧值快照容器。
        pub const fn is_props_change(self) -> bool {
            matches!(
                self,
                Self::ParaPropsChange
                    | Self::RunPropsChange
                    | Self::TablePropsChange
                    | Self::TablePropsExChange
                    | Self::SectPropsChange
                    | Self::RowPropsChange
                    | Self::CellPropsChange
                    | Self::TableGridChange
                    | Self::NumberingChange
            )
        }
    }

    /// 承载修订的宿主。`kind` 已经说明是哪种修订，这里给的是接受 / 拒绝时要动的那个上层节点。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RevOwner {
        /// 块级包裹：包着 `w:p` / `w:tbl` 的那层壳所在的块容器（`w:body` / `w:tc` / `w:sdtContent` …）。
        Block(NodeId),
        /// run 级包裹：所在段落 `w:p`。
        Inline(NodeId),
        /// `w:r/w:rPr/w:rPrChange`：那个 `w:r`。
        Run(NodeId),
        /// 段落标记（`pPr/rPr/w:ins|w:del`、`pPr/rPr/w:rPrChange`）、`pPrChange`、`numberingChange`：`w:p`。
        ParaMark(NodeId),
        Row(NodeId),
        Cell(NodeId),
        Table(NodeId),
        /// `w:sectPr/w:sectPrChange`：那个 `w:sectPr`。
        Section(NodeId),
        /// run 级删除包住的是字段指令区（`w:delInstrText`）：那个字段。
        Field(FieldId),
    }

    impl RevOwner {
        /// 宿主节点（`Field` 没有节点）。
        pub const fn node(self) -> Option<NodeId> {
            match self {
                Self::Block(n)
                | Self::Inline(n)
                | Self::Run(n)
                | Self::ParaMark(n)
                | Self::Row(n)
                | Self::Cell(n)
                | Self::Table(n)
                | Self::Section(n) => Some(n),
                Self::Field(_) => None,
            }
        }
    }

    /// 一条修订。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RevisionEntry {
        pub id: RevisionId,
        pub part: PartId,
        pub kind: RevKind,
        /// 承载元素（`w:ins` / `w:rPrChange` / …）与它的 `w:id` / `w:author` / `w:date`。
        pub meta: RevisionMeta,
        pub owner: RevOwner,
        /// 外层修订包裹的层数，最外层为 0。
        pub depth: u16,
        /// `w:moveFromRangeStart` / `w:moveToRangeStart` 的 `@w:name`（只有搬移有；没被范围罩住则 `None`）。
        pub move_name: Option<String>,
        /// moveFrom ↔ moveTo 的孪生（`REV_UNPAIRED_MOVE` 时 `None`）。
        pub pair: Option<RevisionId>,
    }

    impl RevisionEntry {
        /// 承载元素。
        pub fn node(&self) -> NodeId {
            self.meta.node
        }

        pub fn author(&self) -> Option<&str> {
            self.meta.author.as_deref()
        }

        /// `w:id` 的数值形态（`EDIT-06` 取全局最大值用）。非数字的原串保留在 `meta.id`，不参与比较。
        pub fn w_id(&self) -> Option<u32> {
            self.meta.id.as_deref()?.trim().parse().ok()
        }
    }

    /// 全包的修订表（`MOD-09`），文档序。
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct RevisionIndex {
        entries: Vec<RevisionEntry>,
        by_id: BTreeMap<RevisionId, usize>,
        by_node: BTreeMap<(PartId, NodeId), usize>,
    }

    impl RevisionIndex {
        pub fn entries(&self) -> &[RevisionEntry] {
            &self.entries
        }

        pub fn len(&self) -> usize {
            self.entries.len()
        }

        pub fn is_empty(&self) -> bool {
            self.entries.is_empty()
        }

        pub fn get(&self, id: RevisionId) -> Option<&RevisionEntry> {
            self.by_id.get(&id).map(|&i| &self.entries[i])
        }

        /// 承载元素 → 条目。
        pub fn by_node(&self, part: PartId, node: NodeId) -> Option<&RevisionEntry> {
            self.by_node.get(&(part, node)).map(|&i| &self.entries[i])
        }

        pub fn of_part(&self, part: PartId) -> impl Iterator<Item = &RevisionEntry> {
            self.entries.iter().filter(move |e| e.part == part)
        }

        /// 某个作者的修订（`w:author` 字符串相等，不看 `w:initials`）。
        pub fn by_author<'a>(&'a self, author: &'a str) -> impl Iterator<Item = &'a RevisionEntry> {
            self.entries.iter().filter(move |e| e.author() == Some(author))
        }

        /// 出现过的作者，去重后按名字排序。
        pub fn authors(&self) -> Vec<&str> {
            let mut v: Vec<&str> = self.entries.iter().filter_map(RevisionEntry::author).collect();
            v.sort_unstable();
            v.dedup();
            v
        }

        /// 全包最大的修订 `w:id`（`EDIT-06`：新修订从它 + 1 起编号）。
        pub fn max_w_id(&self) -> Option<u32> {
            self.entries.iter().filter_map(RevisionEntry::w_id).max()
        }

        /// **先内层后外层**的文档序（`spec/18` 7.4：`w:ins` 套 `w:del` 时先处理 `del`）。
        ///
        /// 条目是前序收集的，祖先一定排在后代之前，`depth` 相邻两条最多差 1，所以一遍栈就能转成后序。
        pub fn iter_inner_first(&self) -> Vec<&RevisionEntry> {
            let mut out = Vec::with_capacity(self.entries.len());
            let mut stack: Vec<&RevisionEntry> = Vec::new();
            for e in &self.entries {
                while stack.last().is_some_and(|t| t.depth >= e.depth) {
                    out.push(stack.pop().expect("checked by last()"));
                }
                stack.push(e);
            }
            while let Some(t) = stack.pop() {
                out.push(t);
            }
            out
        }

        /// 会话内稳定编号（`MOD-13`）：仍然存在的承载节点复用旧 id，新节点从 `next` 取号。
        pub(crate) fn stabilize(
            &mut self,
            known: &mut BTreeMap<(PartId, NodeId), RevisionId>,
            next: &mut u32,
        ) {
            // `pair` 存的是重编号**之前**的 id，先记下它指向哪个节点
            let was: BTreeMap<RevisionId, (PartId, NodeId)> =
                self.entries.iter().map(|e| (e.id, (e.part, e.node()))).collect();
            for e in &mut self.entries {
                let key = (e.part, e.node());
                let id = *known.entry(key).or_insert_with(|| {
                    let id = RevisionId(*next);
                    *next = next.saturating_add(1);
                    id
                });
                e.id = id;
            }
            let now: BTreeMap<(PartId, NodeId), RevisionId> =
                self.entries.iter().map(|e| ((e.part, e.node()), e.id)).collect();
            for e in &mut self.entries {
                e.pair = e.pair.and_then(|old| was.get(&old)).and_then(|k| now.get(k)).copied();
            }
            self.reindex();
        }

        fn reindex(&mut self) {
            self.by_id = self.entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
            self.by_node =
                self.entries.iter().enumerate().map(|(i, e)| ((e.part, e.node()), i)).collect();
        }
    }

    // ---- 构建 --------------------------------------------------------------------------------------

    /// 一个 part 的输入：DOM 与（有的话）它的字段索引。
    pub(crate) struct RevPart<'a> {
        pub part: PartId,
        pub dom: &'a Dom,
        pub fields: Option<&'a FieldIndex>,
    }

    /// 遍历时随节点下传的上下文。
    #[derive(Clone, Copy, Default)]
    struct Ctx {
        /// 最近的 `w:p`（进内容流根与 `w:tc` 时清空——文本框里的段落不算外层段落的一部分）。
        para: Option<NodeId>,
        run: Option<NodeId>,
        row: Option<NodeId>,
        cell: Option<NodeId>,
        table: Option<NodeId>,
        sect: Option<NodeId>,
        /// 最近的块容器（块级包裹的 owner）。
        container: Option<NodeId>,
        /// 在 `w:pPr` 里（它下面的 `w:rPr` 是段落标记的）。
        in_ppr: bool,
        /// 在 `w:trPr` 里（它下面的 `w:ins` / `w:del` 是整行插入 / 删除）。
        in_trpr: bool,
        depth: u16,
    }

    impl RevisionIndex {
        /// 扫一批 part，按给定顺序拼成文档序的索引。
        pub(crate) fn build(
            parts: &[RevPart<'_>],
            warnings: &mut Vec<Diagnostic>,
        ) -> RevisionIndex {
            let mut idx = RevisionIndex::default();
            for p in parts {
                let base = idx.entries.len() as u32;
                idx.entries.extend(scan_part(p, base));
            }
            idx.pair_moves(parts, warnings);
            idx.reindex();
            idx
        }

        /// 按 `@w:name` 把 moveFrom 与 moveTo 配对，落单的记 `REV_UNPAIRED_MOVE`。
        fn pair_moves(&mut self, parts: &[RevPart<'_>], warnings: &mut Vec<Diagnostic>) {
            let (pairs, mut lonely) = self.group_moves();
            for (f, t) in pairs {
                let (fid, tid) = (self.entries[f].id, self.entries[t].id);
                self.entries[f].pair = Some(tid);
                self.entries[t].pair = Some(fid);
            }
            lonely.sort_unstable();
            lonely.dedup();
            for i in lonely {
                let e = &self.entries[i];
                let dom = parts.iter().find(|p| p.part == e.part).map(|p| p.dom);
                let range =
                    dom.and_then(|d| d.node(e.node()).lex.as_ref().map(|l| l.range.clone()));
                warnings.push(Diagnostic::pre_existing(
                    e.part,
                    range,
                    DiagCode::RevUnpairedMove,
                    match &e.move_name {
                        Some(n) => format!("{} 的孪生（w:name = {n}）不存在", e.kind),
                        None => format!("{} 不在任何 move 范围标记里", e.kind),
                    },
                ));
            }
        }

        /// 按 `@w:name` 分组配对，返回（成对的下标, 落单的下标）。
        fn group_moves(&self) -> (Vec<(usize, usize)>, Vec<usize>) {
            let mut from: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            let mut to: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            let mut lonely: Vec<usize> = Vec::new();
            for (i, e) in self.entries.iter().enumerate() {
                if !e.kind.is_move() {
                    continue;
                }
                match e.move_name.as_deref() {
                    Some(name) => {
                        let side = if e.kind.is_move_from() { &mut from } else { &mut to };
                        side.entry(name).or_default().push(i);
                    }
                    None => lonely.push(i),
                }
            }
            let mut pairs: Vec<(usize, usize)> = Vec::new();
            for (name, fs) in &from {
                let ts = to.get(name).map(Vec::as_slice).unwrap_or(&[]);
                for (k, &f) in fs.iter().enumerate() {
                    match ts.get(k) {
                        Some(&t) => pairs.push((f, t)),
                        None => lonely.push(f),
                    }
                }
                if ts.len() > fs.len() {
                    lonely.extend(ts[fs.len()..].iter().copied());
                }
            }
            for (name, ts) in &to {
                if !from.contains_key(name) {
                    lonely.extend(ts.iter().copied());
                }
            }
            (pairs, lonely)
        }
    }

    /// 扫一个 part，条目按前序（= 文档序）返回，`id` 先按序号临时给。
    fn scan_part(p: &RevPart<'_>, id_base: u32) -> Vec<RevisionEntry> {
        let dom = p.dom;
        let mut out: Vec<RevisionEntry> = Vec::new();
        // 打开着的 move 范围：`(w:id, w:name)`，最内层在末尾
        let mut open_from: Vec<(String, String)> = Vec::new();
        let mut open_to: Vec<(String, String)> = Vec::new();
        let mut stack: Vec<(NodeId, Ctx)> = vec![(dom.root(), Ctx::default())];
        while let Some((n, ctx)) = stack.pop() {
            if dom.node(n).dirty == Dirty::Deleted {
                continue;
            }
            let Some(e) = dom.element(n) else { continue };
            let name = e.name;
            // 范围标记先处理：它们是兄弟节点，按文档序开合
            if name.ns == NsId::W {
                match name.local {
                    LocalName::MoveFromRangeStart => push_range(dom, n, &mut open_from),
                    LocalName::MoveToRangeStart => push_range(dom, n, &mut open_to),
                    LocalName::MoveFromRangeEnd => pop_range(dom, n, &mut open_from),
                    LocalName::MoveToRangeEnd => pop_range(dom, n, &mut open_to),
                    _ => {}
                }
            }
            let hit = classify(dom, n, name, &ctx);
            let mut child = ctx;
            if let Some((kind, owner)) = hit {
                let move_name = kind.is_move().then(|| {
                    let open = if kind.is_move_from() { &open_from } else { &open_to };
                    open.last().map(|(_, name)| name.clone())
                });
                out.push(RevisionEntry {
                    id: RevisionId(id_base + out.len() as u32),
                    part: p.part,
                    kind,
                    meta: meta_of(dom, n),
                    owner: refine_owner(dom, n, kind, owner, p.fields),
                    depth: ctx.depth,
                    move_name: move_name.flatten(),
                    pair: None,
                });
                child.depth = ctx.depth.saturating_add(1);
            }
            descend(n, name, &mut child);
            stack.extend(dom.children(n).iter().rev().map(|&c| (c, child)));
        }
        out
    }

    fn meta_of(dom: &Dom, node: NodeId) -> RevisionMeta {
        let a = |l: LocalName| dom.attr_value(node, QName::new(NsId::W, l)).map(|v| v.into_owned());
        RevisionMeta {
            node,
            id: a(LocalName::Id),
            author: a(LocalName::Author),
            date: a(LocalName::Date),
        }
    }

    fn push_range(dom: &Dom, n: NodeId, open: &mut Vec<(String, String)>) {
        let a = |l: LocalName| dom.attr_value(n, QName::new(NsId::W, l)).map(|v| v.into_owned());
        open.push((a(LocalName::Id).unwrap_or_default(), a(LocalName::Name).unwrap_or_default()));
    }

    fn pop_range(dom: &Dom, n: NodeId, open: &mut Vec<(String, String)>) {
        let id = dom
            .attr_value(n, QName::new(NsId::W, LocalName::Id))
            .map(|v| v.into_owned())
            .unwrap_or_default();
        match open.iter().rposition(|(i, _)| *i == id) {
            Some(k) => {
                open.remove(k);
            }
            // `w:id` 对不上（`rev-move-unpaired`）：按最内层关掉，不让范围一直挂着
            None => {
                open.pop();
            }
        }
    }

    /// 元素名 + 上下文 → 修订种类与宿主；不是承载元素则 `None`。
    fn classify(dom: &Dom, n: NodeId, name: QName, ctx: &Ctx) -> Option<(RevKind, RevOwner)> {
        if name.ns != NsId::W {
            return None;
        }
        let block = || RevOwner::Block(ctx.container.or(dom.parent(n)).unwrap_or(n));
        let owner_or = |slot: Option<NodeId>, f: fn(NodeId) -> RevOwner| {
            slot.map_or_else(|| RevOwner::Block(dom.parent(n).unwrap_or(n)), f)
        };
        let para_mark = || owner_or(ctx.para, RevOwner::ParaMark);
        Some(match name.local {
            LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
                let ins = matches!(name.local, LocalName::Ins | LocalName::MoveTo);
                let mv = matches!(name.local, LocalName::MoveFrom | LocalName::MoveTo);
                if ctx.in_ppr && dom.parent(n).is_some_and(|p| dom.is(p, QName::w(LocalName::RPr)))
                {
                    // 段落标记。`w:moveFrom` / `w:moveTo` 对标记的作用与 `w:del` / `w:ins` 相同
                    // （接受 moveFrom = 与下一段合并），但要与另一半配对，所以另立种类
                    let kind = match (mv, ins) {
                        (true, true) => RevKind::ParaMarkMoveTo,
                        (true, false) => RevKind::ParaMarkMoveFrom,
                        (false, true) => RevKind::ParaMarkInsert,
                        (false, false) => RevKind::ParaMarkDelete,
                    };
                    (kind, para_mark())
                } else if ctx.in_trpr {
                    let kind = if ins { RevKind::Insert } else { RevKind::Delete };
                    (kind, owner_or(ctx.row, RevOwner::Row))
                } else if ctx.para.is_some() {
                    let kind = match (mv, ins) {
                        (true, true) => RevKind::RunMoveTo,
                        (true, false) => RevKind::RunMoveFrom,
                        (false, true) => RevKind::RunInsert,
                        (false, false) => RevKind::RunDelete,
                    };
                    (kind, owner_or(ctx.para, RevOwner::Inline))
                } else {
                    let kind = match (mv, ins) {
                        (true, true) => RevKind::MoveTo,
                        (true, false) => RevKind::MoveFrom,
                        (false, true) => RevKind::Insert,
                        (false, false) => RevKind::Delete,
                    };
                    (kind, block())
                }
            }
            LocalName::RPrChange => {
                let owner = if ctx.in_ppr { para_mark() } else { owner_or(ctx.run, RevOwner::Run) };
                (RevKind::RunPropsChange, owner)
            }
            LocalName::PPrChange => (RevKind::ParaPropsChange, para_mark()),
            LocalName::NumberingChange => (RevKind::NumberingChange, para_mark()),
            LocalName::SectPrChange => {
                (RevKind::SectPropsChange, owner_or(ctx.sect, RevOwner::Section))
            }
            LocalName::TblPrChange => {
                (RevKind::TablePropsChange, owner_or(ctx.table, RevOwner::Table))
            }
            LocalName::TblPrExChange => {
                (RevKind::TablePropsExChange, owner_or(ctx.row, RevOwner::Row))
            }
            LocalName::TblGridChange => {
                (RevKind::TableGridChange, owner_or(ctx.table, RevOwner::Table))
            }
            LocalName::TrPrChange => (RevKind::RowPropsChange, owner_or(ctx.row, RevOwner::Row)),
            LocalName::TcPrChange => (RevKind::CellPropsChange, owner_or(ctx.cell, RevOwner::Cell)),
            LocalName::CellIns => (RevKind::CellInsert, owner_or(ctx.cell, RevOwner::Cell)),
            LocalName::CellDel => (RevKind::CellDelete, owner_or(ctx.cell, RevOwner::Cell)),
            LocalName::CellMerge => (RevKind::CellMerge, owner_or(ctx.cell, RevOwner::Cell)),
            _ => return None,
        })
    }

    /// run 级删除 / 插入包住的全是字段指令区的 run 时，宿主记成那个字段（`FLD-10` / 7.4 的
    /// `FieldInstrDelete`）。查不到字段索引就保持原来的宿主。
    fn refine_owner(
        dom: &Dom,
        n: NodeId,
        kind: RevKind,
        owner: RevOwner,
        fields: Option<&FieldIndex>,
    ) -> RevOwner {
        if !kind.is_run_level() {
            return owner;
        }
        let Some(fields) = fields else { return owner };
        let mut found = None;
        let mut instr = false;
        let mut other = false;
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            if dom.node(x).dirty == Dirty::Deleted {
                continue;
            }
            let Some(e) = dom.element(x) else { continue };
            stack.extend(e.children.iter().rev());
            if e.name.ns != NsId::W {
                continue;
            }
            match e.name.local {
                LocalName::InstrText | LocalName::DelInstrText => instr = true,
                LocalName::T | LocalName::DelText => other = true,
                LocalName::R => {
                    if let Some(f) = fields.field_of(x) {
                        found.get_or_insert(f.id);
                    }
                }
                _ => {}
            }
        }
        match (instr && !other, found) {
            (true, Some(id)) => RevOwner::Field(id),
            _ => owner,
        }
    }

    /// 进入 `node` 的子树前更新上下文。
    fn descend(n: NodeId, name: QName, ctx: &mut Ctx) {
        if name.ns != NsId::W {
            return;
        }
        match name.local {
            // 内容流根：里面的段落与外层无关（`SPAN-01`）
            LocalName::Body
            | LocalName::TxbxContent
            | LocalName::Hdr
            | LocalName::Ftr
            | LocalName::Footnote
            | LocalName::Endnote
            | LocalName::Comment => {
                ctx.para = None;
                ctx.run = None;
                ctx.container = Some(n);
            }
            LocalName::Tbl => ctx.table = Some(n),
            LocalName::Tr => ctx.row = Some(n),
            LocalName::Tc => {
                ctx.cell = Some(n);
                ctx.para = None;
                ctx.run = None;
                ctx.container = Some(n);
            }
            LocalName::SdtContent | LocalName::CustomXml => {
                if ctx.para.is_none() {
                    ctx.container = Some(n);
                }
            }
            LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
                if ctx.para.is_none() && !ctx.in_ppr && !ctx.in_trpr {
                    ctx.container = Some(n);
                }
            }
            LocalName::P => ctx.para = Some(n),
            LocalName::R => ctx.run = Some(n),
            LocalName::PPr => ctx.in_ppr = true,
            LocalName::TrPr => ctx.in_trpr = true,
            LocalName::SectPr => ctx.sect = Some(n),
            _ => {}
        }
    }
}

// 内容控件（`MOD-08`，任务 3.3）：`w:sdt` 的 `sdtPr` 读成 [`SdtInfo`]。
//
// 块级与 run 级 sdt 用同一个读取器。控件种类按 `sdtPr` 里第一个可识别的控件元素判定，**只看局部名**
// ——复选框在 `w14`、重复节在 `w15`，Word 各版本的前缀不一样（TS 也是这么认的）。
// 编辑策略在 `EDIT-03`：[`refusing_sdt`] 给出拒绝理由，`ContentLocked` / `SdtContentLocked` 只读，
// 有 `data_binding` 的第一阶段也只读（显示文字只是绑定数据的缓存，Word 重开会从 customXml 刷回）。

/// 无字段枚举 + `as_str` + `parse`：把「变体 ↔ XML 字面」的名字表写成一张表，
/// 免得枚举、匹配、测试各抄一遍（通用枚举宏见 `named_enum!`，这里只服务 sdt）。
///
/// ```ignore
/// sdt_enum! {
///     /// 文档注释
///     pub enum SdtLock { Unlocked => "unlocked", SdtLocked => "sdtLocked" }
/// }
/// ```
macro_rules! sdt_enum {
        ($(#[$m:meta])* pub enum $name:ident { $($(#[$vm:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
            $(#[$m])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum $name {
                $($(#[$vm])* $variant,)+
            }

            impl $name {
                /// 全部变体，声明顺序。
                pub const ALL: &[$name] = &[$($name::$variant,)+];

                /// XML 字面。
                pub const fn as_str(self) -> &'static str {
                    match self {
                        $($name::$variant => $text,)+
                    }
                }

                /// 精确匹配 XML 字面。
                pub fn parse(s: &str) -> Option<$name> {
                    match s {
                        $($text => Some($name::$variant),)+
                        _ => None,
                    }
                }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(self.as_str())
                }
            }
        };
    }

sdt_enum! {
    /// 控件种类（`sdtPr` 里的控件元素）。识别不出来（只有 `w:id` / `w:tag` 一类）→ [`SdtControl::Unknown`]。
    pub enum SdtControl {
        /// `w:richText`：富文本（Word 的缺省内容控件）
        RichText => "richText",
        /// `w:text`：纯文本
        PlainText => "text",
        /// `w:picture`
        Picture => "picture",
        /// `w:comboBox`：可输入的下拉框
        ComboBox => "comboBox",
        /// `w:dropDownList`：只能选的下拉框
        DropDownList => "dropDownList",
        /// `w:date`
        Date => "date",
        /// `w14:checkbox`（Word 2013）
        Checkbox => "checkbox",
        /// `w:group`：成组的只读区
        Group => "group",
        /// `w:citation`：引文
        Citation => "citation",
        /// `w:bibliography`：参考文献
        Bibliography => "bibliography",
        /// `w:docPartObj`：构建基块（封面等）
        DocPartObj => "docPartObj",
        /// `w:docPartList`
        DocPartList => "docPartList",
        /// `w:equation`
        Equation => "equation",
        /// `w15:repeatingSection`
        RepeatingSection => "repeatingSection",
        /// `w15:repeatingSectionItem`
        RepeatingSectionItem => "repeatingSectionItem",
        /// 没有可识别的控件元素
        Unknown => "unknown",
    }
}

sdt_enum! {
    /// `w:lock/@w:val`。缺 `w:lock` 或字面不认识 → [`SdtLock::Unlocked`]。
    pub enum SdtLock {
        /// 都能改
        Unlocked => "unlocked",
        /// 控件本身不能删，内容可改
        SdtLocked => "sdtLocked",
        /// 内容只读，控件可删
        ContentLocked => "contentLocked",
        /// 都不行
        SdtContentLocked => "sdtContentLocked",
    }
}

impl SdtLock {
    /// 内容只读（`EDIT-03`：编辑落在这种 sdt 内 → `Err(EDIT_SDT_LOCKED)`）。
    pub const fn content_locked(self) -> bool {
        matches!(self, SdtLock::ContentLocked | SdtLock::SdtContentLocked)
    }

    /// 控件本身不可删除（`w:sdt` 元素受保护）。M3 没有删除整个 sdt 的操作，只建模。
    pub const fn sdt_locked(self) -> bool {
        matches!(self, SdtLock::SdtLocked | SdtLock::SdtContentLocked)
    }
}

/// `w:dataBinding`：控件内容绑定到 customXml part。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBinding {
    pub prefix_mappings: Option<String>,
    pub xpath: Option<String>,
    pub store_item_id: Option<String>,
}

/// `w:docPartObj` / `w:docPartList` 的内容。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocPart {
    pub gallery: Option<String>,
    pub category: Option<String>,
    /// `w:docPartUnique`（三态 `OnOff`，缺省 false）。
    pub unique: bool,
}

/// 最近的 `w:sdt` 祖先（`MOD-08`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdtInfo {
    pub node: NodeId,
    /// `w:alias/@w:val`：给人看的标题。
    pub alias: Option<String>,
    /// `w:tag/@w:val`：给程序用的标签。
    pub tag: Option<String>,
    /// `w:id/@w:val`。
    pub id: Option<i32>,
    pub control: SdtControl,
    pub lock: SdtLock,
    pub data_binding: Option<DataBinding>,
    /// `w:docPartObj` / `w:docPartList` 的内容（控件种类见 `control`）。
    pub doc_part: Option<DocPart>,
    /// `w:placeholder/w:docPart/@w:val`：占位文字所在的构建基块名。
    pub placeholder: Option<String>,
    /// `w:showingPlcHdr`：当前显示的是占位文字而不是真实内容。
    pub showing_placeholder: bool,
}

fn sdt_w(local: LocalName) -> QName {
    QName::w(local)
}

impl SdtInfo {
    /// 读 `w:sdt` 的 `w:sdtPr`；没有 `sdtPr` 时除 `node` 外全是缺省值。
    pub fn read(dom: &Dom, sdt: NodeId) -> SdtInfo {
        let mut info = SdtInfo {
            node: sdt,
            alias: None,
            tag: None,
            id: None,
            control: SdtControl::Unknown,
            lock: SdtLock::Unlocked,
            data_binding: None,
            doc_part: None,
            placeholder: None,
            showing_placeholder: false,
        };
        let Some(pr) = dom.semantic_children(sdt).find(|&n| dom.is(n, sdt_w(LocalName::SdtPr)))
        else {
            return info;
        };
        let val = |n: NodeId| dom.attr_value(n, sdt_w(LocalName::Val)).map(|v| v.into_owned());
        let attr =
            |n: NodeId, local: LocalName| dom.attr_value(n, sdt_w(local)).map(|v| v.into_owned());
        // `OnOff` 元素：存在即 true，除非 `w:val` 明确关掉（`PROP-02`）
        let on = |n: NodeId| !matches!(val(n).as_deref(), Some("0" | "false" | "off"));
        for child in dom.semantic_children(pr) {
            let Some(name) = dom.name(child) else { continue };
            // 控件元素只看局部名：checkbox 在 w14、repeatingSection* 在 w15
            if info.control == SdtControl::Unknown
                && let Some(kind) =
                    name.local.known_str().and_then(SdtControl::parse).filter(|c| c.is_control())
            {
                info.control = kind;
                if matches!(kind, SdtControl::DocPartObj | SdtControl::DocPartList) {
                    info.doc_part = Some(read_doc_part(dom, child));
                }
                continue;
            }
            match name.local {
                LocalName::Alias => info.alias = val(child),
                LocalName::UTag | LocalName::Tag => {
                    if info.tag.is_none() {
                        info.tag = val(child);
                    }
                }
                LocalName::Id => info.id = val(child).and_then(|v| v.trim().parse().ok()),
                LocalName::Lock => {
                    info.lock =
                        val(child).as_deref().and_then(SdtLock::parse).unwrap_or(SdtLock::Unlocked);
                }
                LocalName::DataBinding => {
                    info.data_binding = Some(DataBinding {
                        prefix_mappings: attr(child, LocalName::PrefixMappings),
                        xpath: attr(child, LocalName::Xpath),
                        store_item_id: attr(child, LocalName::StoreItemID),
                    });
                }
                LocalName::Placeholder => {
                    info.placeholder = dom
                        .semantic_children(child)
                        .find(|&n| dom.is(n, sdt_w(LocalName::DocPart)))
                        .and_then(val);
                }
                LocalName::ShowingPlcHdr => info.showing_placeholder = on(child),
                _ => {}
            }
        }
        info
    }

    /// 内容只读（`w:lock`）。
    pub fn content_locked(&self) -> bool {
        self.lock.content_locked()
    }

    /// 绑定到 customXml：第一阶段不可编辑（`EDIT-03`）。
    pub fn is_bound(&self) -> bool {
        self.data_binding.is_some()
    }

    /// 拒绝编辑的理由；两条都不成立时 `None`。
    pub fn refusal(&self) -> Option<SdtRefusal> {
        if self.content_locked() {
            Some(SdtRefusal::Locked)
        } else if self.is_bound() {
            Some(SdtRefusal::Bound)
        } else {
            None
        }
    }
}

impl SdtControl {
    /// 这个变体对应一个真正的控件元素（`Unknown` 不是）。
    const fn is_control(self) -> bool {
        !matches!(self, SdtControl::Unknown)
    }
}

fn read_doc_part(dom: &Dom, node: NodeId) -> DocPart {
    let mut dp = DocPart::default();
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        let val = || dom.attr_value(c, sdt_w(LocalName::Val)).map(|v| v.into_owned());
        match name.local {
            LocalName::DocPartGallery => dp.gallery = val(),
            LocalName::DocPartCategory => dp.category = val(),
            LocalName::DocPartUnique => {
                dp.unique = !matches!(val().as_deref(), Some("0" | "false" | "off"));
            }
            _ => {}
        }
    }
    dp
}

/// 为什么拒绝编辑（`EDIT-03`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdtRefusal {
    /// `w:lock` 为 `contentLocked` / `sdtContentLocked` → `EDIT_SDT_LOCKED`。
    Locked,
    /// 有 `w:dataBinding` → `EDIT_SDT_BOUND`（第一阶段）。
    Bound,
}

/// `node`（含自身）的祖先里第一个拒绝内容编辑的 `w:sdt`；从最近的祖先往外找。
pub fn refusing_sdt(dom: &Dom, node: NodeId) -> Option<(SdtInfo, SdtRefusal)> {
    std::iter::once(node)
        .chain(dom.ancestors(node))
        .filter(|&n| dom.is(n, sdt_w(LocalName::Sdt)))
        .find_map(|n| {
            let info = SdtInfo::read(dom, n);
            info.refusal().map(|r| (info, r))
        })
}

// 节模型（`MOD-10` 的 `SectionInfo`、`spec/16` 任务 5.1 / 5.2）与锚定绘图要的页面几何
// （`spec/15` 任务 4.6c）。
//
// 两层：
//
// - [`SectionInfo`]：一个节的声明值。`props` 走节属性表（`schema/props/section.toml`，`PROP-01`），
//   所以 `ST_TwipsMeasure` 的单位与容错、`Val::Raw` 降级都是共用的一份 codec。节的**继承**
//   （页眉页脚引用缺失时沿用上一节）不在这里，在 `resolve::section`（`RES-10`）——模型只存声明值。
// - [`SectionGeom`] / [`Sections`]：从 `props` 里挑出页宽页高、四边页边距、栏数的小结构，按
//   字节偏移查询。`wp:anchor` 相对 `page` / `margin` 对齐时要拿它解横向位置（TS
//   `resolveAnchorPagePos`），否则浮动框的 `offsetXEmu` / `pageRelX` / `pagePinned` 都定不下来。
//
// 节的边界：每个 `w:sectPr` **结束**它所在的节（最后一节的 `sectPr` 是 `w:body` 的末尾子元素，
// 分节段落的写在自己的 `pPr` 里）。所以「管辖某个位置的节」= 第一个结束位置在它之后的
// `sectPr`（TS `sectionAt` 同义）。一份 `w:sectPr` 都没有的文档给一个隐式节（`node: None`，
// 全部取缺省，同 TS `DEFAULT_SECTION`）。

use crate::semantic::props::{
    SectType, SectionProps, read_section_props, read_section_props_change,
};

/// 缺省节：US Letter 竖排、四边 1 英寸（同 TS `DEFAULT_SECTION`）。
pub const DEFAULT_PAGE_WIDTH: i64 = 12_240;
pub const DEFAULT_PAGE_HEIGHT: i64 = 15_840;
pub const DEFAULT_MARGIN: i64 = 1_440;

named_enum! {
    /// 页眉还是页脚。名字是 TS 的字面值。
    pub enum HfKind {
        Header = "header",
        Footer = "footer",
    }
}

named_enum! {
    /// 页眉页脚的三种变体（`ST_HdrFtr`）。非 schema 的 `odd` 归 `Default`（`RES-10`）。
    pub enum HfVariant {
        Default = "default",
        First = "first",
        Even = "even",
    }
}

impl HfKind {
    pub const ALL: [HfKind; 2] = [HfKind::Header, HfKind::Footer];
}

impl HfVariant {
    pub const ALL: [HfVariant; 3] = [HfVariant::Default, HfVariant::First, HfVariant::Even];

    /// `w:type` 的建模值 → 变体。缺失与认不出的都算 `default`（Word 行为，`docs/01` §12）。
    pub fn of(kind: Option<&Val<crate::semantic::props::HdrFtrType>>) -> HfVariant {
        use crate::semantic::props::HdrFtrType as T;
        match kind {
            Some(Val::Value(T::First)) => HfVariant::First,
            Some(Val::Value(T::Even)) => HfVariant::Even,
            // default / odd（非 schema）/ Raw / 缺失
            _ => HfVariant::Default,
        }
    }
}

/// 一个节的 `w:sectPr` 长在哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionOwner {
    /// `w:body` 的末尾子元素：最后一节。
    Body,
    /// 分节段落的 `pPr/sectPr`；`NodeId` 是那个 `w:p`。
    Paragraph(NodeId),
    /// 文档里没有任何 `w:sectPr`：隐式节。
    Implicit,
}

/// 一个节（`MOD-10`）。全是**声明值**：继承看 `resolve::section`（`RES-10`）。
///
/// `PartialEq` 是 `MOD-13` 的 oracle 要的（`refresh == rebuild`）；生成的 `SectionProps` 的
/// `PartialEq` 不比 `raw_unmodeled`（未建模子元素的 `NodeId`），所以比较只看建模字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    /// `w:sectPr`；隐式节为 `None`。
    pub node: Option<NodeId>,
    /// 装箱：`SectionProps` 有几 KB，节列表按值传会把栈帧撑大（同 `TableBlock.props`）。
    pub props: Box<SectionProps>,
    pub owner: SectionOwner,
    /// 属于本节的块在 `Document.main` 里的下标区间；分节段落自己算本节的最后一块。
    pub block_range: Range<usize>,
    /// `sectPr/sectPrChange` 的旧值快照（`MOD-09`）。
    pub revisions: Vec<Revision>,
    /// `w:sectPr` 的结束字节偏移，`Sections::at` 用；隐式节为 `u32::MAX`。
    end_offset: u32,
}

impl SectionInfo {
    /// `w:type`：本节相对上一节如何开始。缺省 `nextPage`；第一节无意义。
    pub fn start_type(&self) -> SectType {
        match self.props.kind.as_ref() {
            Some(Val::Value(t)) => *t,
            _ => SectType::NextPage,
        }
    }

    /// `w:titlePg`：本节首页用 `first` 变体。
    pub fn title_pg(&self) -> bool {
        self.props.title_pg == Some(true)
    }

    /// 本节**声明**的某个槽的关系 id；没声明返回 `None`（继承在 `RES-10`）。
    ///
    /// 同一变体重复声明时取第一个（Word 读第一个）。
    pub fn hf_ref(&self, kind: HfKind, variant: HfVariant) -> Option<&str> {
        let list = match kind {
            HfKind::Header => &self.props.header_references,
            HfKind::Footer => &self.props.footer_references,
        };
        list.iter()
            .find(|r| HfVariant::of(r.kind.as_ref()) == variant)
            .and_then(|r| r.id.as_deref())
    }

    /// 本节声明的全部槽（供 `resolve::section` 做继承）。
    pub fn declared_refs(&self) -> impl Iterator<Item = (HfKind, HfVariant, &str)> + '_ {
        HfKind::ALL.into_iter().flat_map(move |k| {
            HfVariant::ALL.into_iter().filter_map(move |v| self.hf_ref(k, v).map(|id| (k, v, id)))
        })
    }

    /// 页面几何。
    pub fn geom(&self) -> SectionGeom {
        geom_of(self.node, &self.props)
    }
}

/// 一个 `w:sectPr` 的页面几何。长度单位是缇。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionGeom {
    /// `w:sectPr`；隐式节为 `None`。
    pub node: Option<NodeId>,
    /// `w:pgSz/@w:w` / `@w:h`。
    pub page_width: i64,
    pub page_height: i64,
    /// `w:pgMar` 四边。
    pub margin_top: i64,
    pub margin_right: i64,
    pub margin_bottom: i64,
    pub margin_left: i64,
    /// `w:cols/@w:num`，缺省 1。
    pub columns: i64,
}

impl SectionGeom {
    /// 正文可用宽度（页宽减左右页边距）。
    pub fn body_width(&self) -> i64 {
        self.page_width - self.margin_left - self.margin_right
    }
}

/// 按文档序的节几何，附各自的结束偏移。投影侧的查询缓存。
#[derive(Debug, Clone, Default)]
pub struct Sections {
    list: Vec<(u32, SectionGeom)>,
}

impl Sections {
    /// 从已建好的节列表取几何（`Document::rebuild` 之后的正路，不重复读属性）。
    pub fn from_sections(sections: &[SectionInfo]) -> Sections {
        Sections { list: sections.iter().map(|s| (s.end_offset, s.geom())).collect() }
    }

    /// 直接扫一个 part 里的全部 `w:sectPr`（没有 `Document` 时的退路，例如辅助 part）。
    pub fn build(dom: &Dom) -> Sections {
        let mut list = Vec::new();
        for n in dom.semantic_descendants(dom.root()) {
            if !dom.is(n, QName::w(LocalName::SectPr)) {
                continue;
            }
            let mut diags = Vec::new();
            let props = read_section_props(dom, Some(n), &mut diags);
            list.push((end_offset(dom, n), geom_of(Some(n), &props)));
        }
        list.sort_by_key(|&(end, _)| end);
        Sections { list }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 管辖某个字节偏移的节：**第一个结束位置在它之后**的 `w:sectPr`；都在它之前就取最后一个。
    pub fn at(&self, offset: u32) -> Option<&SectionGeom> {
        self.list
            .iter()
            .find(|&&(end, _)| end > offset)
            .or_else(|| self.list.last())
            .map(|(_, g)| g)
    }
}

/// 建模值 → 缇；`Val::Raw`（认不出的字面）与缺失都按 `default` 处理。
fn twips(v: Option<&Val<i32>>, default: i64) -> i64 {
    match v {
        Some(Val::Value(n)) => i64::from(*n),
        _ => default,
    }
}

fn section_attr(dom: &Dom, node: NodeId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::w(local)).map(|v| v.into_owned())
}

fn end_offset(dom: &Dom, node: NodeId) -> u32 {
    dom.node(node).lex.as_ref().map_or(0, |l| l.range.end)
}

fn geom_of(node: Option<NodeId>, p: &SectionProps) -> SectionGeom {
    let (sz, mar) = (p.page_size.as_ref(), p.page_margins.as_ref());
    // 纸张尺寸必须是正数（`ST_TwipsMeasure` 是无符号的）。`w:h="-1"` 这种畸形值解析成 -1
    // 是对的（DOM 是真相，写回时原字节不动），但**几何**不能用它——回退到缺省纸张，
    // 不然所有按页宽算的东西（列宽启发式、图片缩放）都跟着变成负数
    let positive = |v: i64, fallback: i64| if v > 0 { v } else { fallback };
    SectionGeom {
        node,
        page_width: positive(
            twips(sz.and_then(|s| s.w.as_ref()), DEFAULT_PAGE_WIDTH),
            DEFAULT_PAGE_WIDTH,
        ),
        page_height: positive(
            twips(sz.and_then(|s| s.h.as_ref()), DEFAULT_PAGE_HEIGHT),
            DEFAULT_PAGE_HEIGHT,
        ),
        margin_top: twips(mar.and_then(|m| m.top.as_ref()), DEFAULT_MARGIN),
        margin_right: twips(mar.and_then(|m| m.right.as_ref()), DEFAULT_MARGIN),
        margin_bottom: twips(mar.and_then(|m| m.bottom.as_ref()), DEFAULT_MARGIN),
        margin_left: twips(mar.and_then(|m| m.left.as_ref()), DEFAULT_MARGIN),
        columns: twips(p.columns.as_ref().and_then(|c| c.num.as_ref()), 1).max(1),
    }
}

/// 一个顶层块里的 `w:sectPr`：body 级的 `w:sectPr` 自己，或分节段落 `pPr` 里的那个。
///
/// **不下钻**表格与文本框：框里的段落是另一个内容流，它的 `sectPr` 不结束正文的节
/// （TS 按 `originalXml` 里有没有 `<w:sectPr` 判，会把框里的算上；那是它的缺陷）。
fn sect_pr_of(dom: &Dom, block: &Block) -> Option<(NodeId, SectionOwner)> {
    let node = block.node();
    if dom.is(node, QName::w(LocalName::SectPr)) {
        return Some((node, SectionOwner::Body));
    }
    if !dom.is(node, QName::w(LocalName::P)) {
        return None;
    }
    let ppr = dom.semantic_children(node).find(|&c| dom.is(c, QName::w(LocalName::PPr)))?;
    let sect = dom.semantic_children(ppr).find(|&c| dom.is(c, QName::w(LocalName::SectPr)))?;
    Some((sect, SectionOwner::Paragraph(node)))
}

/// 读一个 `w:sectPr` 建出 `SectionInfo`。
///
/// `#[inline(never)]`：`SectionProps` 几 KB，读进来立刻装箱，调用方的栈帧只留一个指针
/// （同 `model/table.rs` 的 `boxed_reader!`）。
#[inline(never)]
fn info_of(
    dom: &Dom,
    node: Option<NodeId>,
    owner: SectionOwner,
    block_range: Range<usize>,
    diags: &mut Vec<Diagnostic>,
) -> SectionInfo {
    let props = Box::new(match node {
        Some(n) => read_section_props(dom, Some(n), diags),
        None => SectionProps::default(),
    });
    let mut revisions = Vec::new();
    if let Some(n) = node
        && let Some((change, old)) = read_section_props_change(dom, Some(n), diags)
    {
        revisions.push(Revision::SectPropsChange {
            meta: RevisionMeta {
                node: change,
                id: section_attr(dom, change, LocalName::Id),
                author: section_attr(dom, change, LocalName::Author),
                date: section_attr(dom, change, LocalName::Date),
            },
            old: Box::new(old),
        });
    }
    SectionInfo {
        node,
        props,
        owner,
        block_range,
        revisions,
        end_offset: node.map_or(u32::MAX, |n| end_offset(dom, n)),
    }
}

/// 正文的节序列（`MOD-10`）：按块序走一遍，每个 `w:sectPr` 结束它所在的节。
///
/// - 一个 `w:sectPr` 都没有 → 一个隐式节（全缺省）覆盖全部块，同 TS `readSections`。
/// - 最后一个 `sectPr` 之后还有块（畸形文档，`w:sectPr` 本该是 body 末尾）→ 并进最后一节，
///   与 `Sections::at` 的"都在它之前就取最后一个"一致。
pub fn build_sections(
    dom: &Dom,
    blocks: &[Block],
    diags: &mut Vec<Diagnostic>,
) -> Vec<SectionInfo> {
    let mut out: Vec<SectionInfo> = Vec::new();
    let mut first = 0usize;
    for (i, b) in blocks.iter().enumerate() {
        // 隐藏的 body 级 sectPr 块与分节段落都算；其余块不看
        let is_props_block =
            matches!(b, Block::Protected(p) if p.kind == ProtectedKind::SectionProps);
        let Some((node, owner)) = sect_pr_of(dom, b) else { continue };
        debug_assert!(is_props_block || matches!(owner, SectionOwner::Paragraph(_)));
        out.push(info_of(dom, Some(node), owner, first..i + 1, diags));
        first = i + 1;
    }
    match out.last_mut() {
        None => out.push(info_of(dom, None, SectionOwner::Implicit, 0..blocks.len(), diags)),
        Some(last) if first < blocks.len() => last.block_range.end = blocks.len(),
        Some(_) => {}
    }
    out
}

/// 管辖某个节点的节下标：第一个结束位置在它之后的 `sectPr`；都在它之前就取最后一个。
pub fn section_of(dom: &Dom, sections: &[SectionInfo], node: NodeId) -> Option<usize> {
    if sections.is_empty() {
        return None;
    }
    let start = dom.node(node).lex.as_ref().map_or(0, |l| l.range.start);
    Some(sections.iter().position(|s| s.end_offset > start).unwrap_or(sections.len() - 1))
}

// 参考文献源（`MOD-10`，`spec/16` 任务 5.7）。
//
// Word 把文献源放在一个 `customXml/item{N}.xml` part 里，根元素是 bibliography 命名空间的
// `b:Sources`——放在这儿 Word 自己的"管理源"对话框才认。所以这份数据既不在主 part 里，
// 也不在任何 `w:` 关系上，只能按"根元素叫什么"去找（`find_part`）。
//
// 读的是**投影**：`Source` 只收 TS `SourceInfo` 的六个字段，未建模的域（`b:Editor` /
// `b:Volume` / `b:Pages` / 多作者列表…）留在 DOM 里，写回时原字节不动（`SAVE-07` 的权威列表
// 只重建变了的条目）。

use crate::package::Package;
use crate::xml::Dirty;

/// 一条文献源（TS `SourceInfo`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// `b:Tag`：引文里引用它的短标签，也是权威列表的键。
    pub tag: String,
    /// `b:SourceType`（`JournalArticle` / `Book` / `InternetSite` …）；缺失按 TS 记 `Misc`。
    pub kind: String,
    /// `b:Corporate`，否则第一个 `b:Person` 的 `"Last, First"`（两者都缺就是空串）。
    pub author: String,
    pub title: String,
    pub year: String,
    /// `b:Publisher` → `b:JournalName` → `b:InternetSiteTitle`，取第一个有的。
    pub publisher: Option<String>,
    pub url: Option<String>,
    /// 这条 `b:Source` 元素本身（写回时未变的条目原字节保留）。
    pub node: NodeId,
}

fn b(local: LocalName) -> QName {
    QName::new(NsId::B, local)
}

fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node(n).dirty != Dirty::Deleted
}

/// 子树里第一个该名字元素的文本（trim 过）。TS 用正则取"整条 `b:Source` 里第一个"，
/// 所以多作者时只看第一个 `b:Person`——这里按同一规则走文档序。
fn field(dom: &Dom, source: NodeId, local: LocalName) -> Option<String> {
    let n = dom.semantic_descendants(source).find(|&n| live(dom, n) && dom.is(n, b(local)))?;
    let text = crate::xml::xpath::string_value(dom, n);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// 一个 part 的 `b:Sources` 根元素。
fn sources_root(dom: &Dom) -> Option<NodeId> {
    let root = dom.root();
    dom.is(root, b(LocalName::Sources)).then_some(root)
}

/// 包里承载 `b:Sources` 的 part：`customXml/item{N}.xml` 里根元素是 `b:Sources` 的那个。
///
/// 按**根元素**找而不是按关系：customXml 的关系类型对每个 item 都一样，Word 也是这么找的
/// （TS `findSourcesPart` 用"文件名匹配 + 内容里出现命名空间"，同一件事）。
pub fn find_part(pkg: &mut Package) -> Option<PartId> {
    let candidates: Vec<PartId> = pkg
        .parts()
        .iter()
        .filter(|p| p.is_xml && is_custom_xml_item(p.uri.as_str()))
        .map(|p| p.id)
        .collect();
    candidates.into_iter().find(|&id| pkg.dom(id).ok().flatten().and_then(sources_root).is_some())
}

/// `customXml/item1.xml`（不含 `itemProps1.xml`）。
pub(in crate::model) fn is_custom_xml_item(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("customXml/item") else { return false };
    let Some(digits) = rest.strip_suffix(".xml") else { return false };
    !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
}

/// 一个 `b:Sources` part 的全部条目（文档序）。`b:Tag` 缺失的条目跳过（同 TS：标签是键）。
pub fn read(dom: &Dom) -> Vec<Source> {
    let Some(root) = sources_root(dom) else { return Vec::new() };
    dom.semantic_children(root)
        .filter(|&n| live(dom, n) && dom.is(n, b(LocalName::Source)))
        .filter_map(|n| {
            let tag = field(dom, n, LocalName::UTag)?;
            let author = field(dom, n, LocalName::Corporate).unwrap_or_else(|| {
                let last = field(dom, n, LocalName::Last).unwrap_or_default();
                let first = field(dom, n, LocalName::First).unwrap_or_default();
                [last, first]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            Some(Source {
                tag,
                kind: field(dom, n, LocalName::SourceType).unwrap_or_else(|| "Misc".into()),
                author,
                title: field(dom, n, LocalName::UTitle).unwrap_or_default(),
                year: field(dom, n, LocalName::Year).unwrap_or_default(),
                publisher: field(dom, n, LocalName::Publisher)
                    .or_else(|| field(dom, n, LocalName::JournalName))
                    .or_else(|| field(dom, n, LocalName::InternetSiteTitle)),
                url: field(dom, n, LocalName::URL),
                node: n,
            })
        })
        .collect()
}

/// `b:SourceType` → 出版方字段的元素名（TS `sourceEntryXml` 的三分支）。
pub fn publisher_element(kind: &str) -> LocalName {
    match kind {
        "JournalArticle" => LocalName::JournalName,
        "InternetSite" => LocalName::InternetSiteTitle,
        _ => LocalName::Publisher,
    }
}

pub mod table {
    //! 表格模型（`MOD-07`，`MOD-09` 的表格部分，任务 3.2）。
    //!
    //! 模型只存**声明值**：`grid` 是 `gridCol/@w:w` 原值（允许 0 与缺失），`hMerge` 不折叠、`trHeight`
    //! 不截、`tcW` 不校正——折叠与校正在 `resolve`（`RES-08`）与 `compat_ts`（`COMPAT-10`）。
    //! 行 / 格穿透 `w:sdt`、`w:customXml` 与修订包裹取得；单元格内容复用正文构建器
    //! （`Builder::build_container`），所以嵌套表、sdt、修订包裹在格里和在正文里一个样。
    //! 嵌套超过 `MAX_CONTAINER_DEPTH` 层的子表降级为 `Protected(TooDeep)`（语料有 2,000 层、hostile 有
    //! 5,000 层的文档，深度上限就是栈的保险）。
    //!
    //! 另外给 [`Document`] 补跨表格的遍历：[`Document::blocks`] / [`Document::paragraphs`] 深入单元格，
    //! [`Document::block_path`] 给任意块的祖先路径（`MOD-13` 的容器级刷新与 `EDIT-02` 的定位用）。

    use crate::diag::{DiagCode, Diagnostic};
    use crate::model::build::{Builder, Document, MAX_CONTAINER_DEPTH};
    use crate::model::{Block, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TextBlock};
    use crate::package::PartId;
    use crate::semantic::props::codec::Twips;
    use crate::semantic::props::{
        CellProps, Ctx, RowProps, TableProps, Val, read_attr, read_cell_props,
        read_cell_props_change, read_row_props, read_row_props_change, read_table_props,
        read_table_props_change,
    };
    use crate::span::is_range_marker;
    use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

    /// `w:tbl`（`MOD-07`）。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct TableBlock {
        pub node: NodeId,
        /// `w:tblPr` 声明值（装箱：三张表格属性表都有几 KB，嵌套 64 层时栈帧要小）。
        pub props: Box<TableProps>,
        /// `w:tblGrid/w:gridCol` 声明值。
        pub grid: Vec<GridCol>,
        pub rows: Vec<Row>,
        /// `tblPr/tblStyle`。
        pub style_id: Option<String>,
        pub sdt: Option<SdtInfo>,
        /// 块级包裹修订 + `TablePropsChange` / `TableGridChange`。
        pub revisions: Vec<Revision>,
    }

    /// `w:gridCol`：`w` 是声明值，可以是 0、`Raw`，也可以缺失。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GridCol {
        pub node: NodeId,
        pub w: Option<Val<i32>>,
    }

    /// `w:tr`。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Row {
        pub node: NodeId,
        pub props: Box<RowProps>,
        /// `w:tblPrEx`：行级表格属性例外，该行优先于 `tblPr`（`RES-08`）。
        pub tbl_pr_ex: Option<Box<TableProps>>,
        pub cells: Vec<Cell>,
        /// 包裹这一行的 `w:sdt`（研究报告模板把 tr 包在 sdt 里）。
        pub sdt: Option<SdtInfo>,
        /// 包裹修订 + `trPr/ins|del`（`Insert` / `Delete`，整行）+ `RowPropsChange`。
        pub revisions: Vec<Revision>,
    }

    /// `w:tc`。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Cell {
        pub node: NodeId,
        pub props: Box<CellProps>,
        /// 单元格内容，与正文同一构建器；最后一个块应是 `w:p`（Word 约束，缺了记 `MOD_TABLE_SHAPE`）。
        pub blocks: Vec<Block>,
        pub sdt: Option<SdtInfo>,
        /// 包裹修订 + `CellInsert` / `CellDelete` / `CellMerge` + `CellPropsChange`。
        pub revisions: Vec<Revision>,
    }

    impl TableBlock {
        /// 声明网格的列数（`tblGrid` 缺失时为 0）。
        pub fn column_count(&self) -> usize {
            self.grid.len()
        }

        /// `(r, c)` 的物理单元格（不折叠 `hMerge`，不按网格坐标）。
        pub fn cell(&self, row: usize, cell: usize) -> Option<&Cell> {
            self.rows.get(row)?.cells.get(cell)
        }

        /// 每一行的网格宽度都等于列数（`EDIT-03` 表格通则的前提；`tblGrid` 缺失时不算一致）。
        pub fn grid_consistent(&self) -> bool {
            let cols = self.grid.len() as i64;
            cols > 0 && self.rows.iter().all(|r| r.grid_width() == cols)
        }
    }

    impl Row {
        /// `gridBefore + Σ gridSpan + gridAfter`：这一行在声明网格里占的列数。
        pub fn grid_width(&self) -> i64 {
            let n =
                |v: &Option<Val<i32>>| v.as_ref().and_then(Val::value).copied().unwrap_or(0).max(0);
            i64::from(n(&self.props.grid_before))
                + self.cells.iter().map(|c| i64::from(c.grid_span())).sum::<i64>()
                + i64::from(n(&self.props.grid_after))
        }
    }

    impl Cell {
        /// `gridSpan`，缺省 1；非正数或 `Raw` 也按 1。
        pub fn grid_span(&self) -> u32 {
            self.props
                .grid_span
                .as_ref()
                .and_then(Val::value)
                .copied()
                .filter(|n| *n > 0)
                .map_or(1, |n| n as u32)
        }

        /// 是否是纵向合并区的非首格（`vMerge` 存在且不是 restart）。
        pub fn is_vmerge_continue(&self) -> bool {
            self.props.v_merge.as_ref().is_some_and(|m| !m.is_restart())
        }

        /// 是否是旧式横向合并的非首格（`hMerge` 存在且不是 restart）；`resolve` 把它折叠进左格。
        pub fn is_hmerge_continue(&self) -> bool {
            self.props.h_merge.as_ref().is_some_and(|m| !m.is_restart())
        }

        /// 格内直接的文本块（不进嵌套表）。
        pub fn text_blocks(&self) -> impl Iterator<Item = &TextBlock> {
            self.blocks.iter().filter_map(Block::as_text)
        }
    }

    fn w(local: LocalName) -> QName {
        QName::w(local)
    }

    /// 读容器属性并装箱。**必须**是独立且不内联的函数：`TableProps` 2.1 KB、`CellProps` 2.4 KB，
    /// 快照元组同样大；留在 `build_table` / `build_row` / `build_cell` 的栈帧里，它们会一直活到递归
    /// 返回（debug 构建按帧分配临时值），64 层嵌套就把 2 MiB 的测试线程栈撑爆。装进 `Box` 之后每层
    /// 只留一个指针，2000 层的语料与 5000 层的 hostile 文档都能在默认栈上跑完。
    macro_rules! boxed_reader {
        ($(#[$m:meta])* $name:ident, $read:path, $props:ty) => {
            $(#[$m])*
            #[inline(never)]
            fn $name(dom: &Dom, container: Option<NodeId>, diags: &mut Vec<Diagnostic>) -> Box<$props> {
                Box::new($read(dom, container, diags))
            }
        };
    }

    /// 同上，读 `*PrChange` 的旧值快照。
    macro_rules! boxed_change_reader {
        ($(#[$m:meta])* $name:ident, $read:path, $props:ty) => {
            $(#[$m])*
            #[inline(never)]
            fn $name(
                dom: &Dom,
                container: Option<NodeId>,
                diags: &mut Vec<Diagnostic>,
            ) -> Option<(NodeId, Box<$props>)> {
                $read(dom, container, diags).map(|(n, v)| (n, Box::new(v)))
            }
        };
    }

    boxed_reader!(
        /// `w:tblPr`（也用于 `w:tblPrEx`）。
        boxed_table_props, read_table_props, TableProps
    );
    boxed_reader!(
        /// `w:trPr`。
        boxed_row_props, read_row_props, RowProps
    );
    boxed_reader!(
        /// `w:tcPr`。
        boxed_cell_props, read_cell_props, CellProps
    );
    boxed_change_reader!(
        /// `w:tblPrChange`。
        boxed_table_props_change, read_table_props_change, TableProps
    );
    boxed_change_reader!(
        /// `w:trPrChange`。
        boxed_row_props_change, read_row_props_change, RowProps
    );
    boxed_change_reader!(
        /// `w:tcPrChange`。
        boxed_cell_props_change, read_cell_props_change, CellProps
    );

    /// 行 / 格收集时的包裹上下文：穿透 sdt 与修订包裹要带着它们往下走。
    struct Wrap {
        node: NodeId,
        sdt: Option<SdtInfo>,
        revs: Vec<Revision>,
    }

    impl<'a> Builder<'a> {
        /// `w:tbl` → [`Block::Table`]；嵌套过深 → `Protected(TooDeep)`（`MOD-07`）。
        pub(super) fn build_table(
            &mut self,
            tbl: NodeId,
            sdt: Option<&SdtInfo>,
            revs: &[Revision],
        ) -> Block {
            let dom = self.dom;
            if self.depth > MAX_CONTAINER_DEPTH {
                self.warn(tbl, DiagCode::ModTooDeep, "表格嵌套过深，子表按只读保留");
                return Block::Protected(ProtectedBlock {
                    node: tbl,
                    kind: ProtectedKind::TooDeep,
                    preview: String::new(),
                    display: None,
                    siblings: Vec::new(),
                    sdt: sdt.cloned(),
                    revisions: revs.to_vec(),
                });
            }
            let tbl_pr = dom.semantic_children(tbl).find(|&n| dom.is(n, w(LocalName::TblPr)));
            let props = boxed_table_props(dom, tbl_pr, &mut self.warnings);
            let mut revisions = revs.to_vec();
            if let Some((change, old)) = boxed_table_props_change(dom, tbl_pr, &mut self.warnings) {
                revisions.push(Revision::TablePropsChange { meta: self.meta(change), old });
            }
            let mut grid = Vec::new();
            if let Some(g) = dom.semantic_children(tbl).find(|&n| dom.is(n, w(LocalName::TblGrid)))
            {
                for c in dom.semantic_children(g) {
                    if dom.is(c, w(LocalName::GridCol)) {
                        let mut ctx = Ctx::new(dom, &mut self.warnings);
                        ctx.enter(c);
                        let width = read_attr::<Twips>(c, w(LocalName::W), None, &mut ctx);
                        grid.push(GridCol { node: c, w: width });
                    } else if dom.is(c, w(LocalName::TblGridChange)) {
                        let old = dom
                            .semantic_children(c)
                            .find(|&n| dom.is(n, w(LocalName::TblGrid)))
                            .unwrap_or(c);
                        revisions.push(Revision::TableGridChange { meta: self.meta(c), old });
                    }
                }
            }
            let mut rows = Vec::new();
            self.collect_rows(tbl, &mut rows);
            let table = TableBlock {
                node: tbl,
                style_id: props.style.clone(),
                props,
                grid,
                rows,
                sdt: sdt.cloned(),
                revisions,
            };
            if !table.grid.is_empty() && !table.grid_consistent() {
                self.warn(
                    tbl,
                    DiagCode::ModTableShape,
                    format!(
                        "行的网格宽度与 tblGrid 的 {} 列不一致：{:?}",
                        table.grid.len(),
                        table.rows.iter().map(Row::grid_width).collect::<Vec<_>>()
                    ),
                );
            }
            Block::Table(table)
        }

        /// `w:tbl` 的行：穿透 `w:sdt/w:sdtContent`、`w:customXml` 与 `w:ins/w:del/w:moveFrom/w:moveTo`
        /// 包裹（带着 sdt / 修订上下文），跳过 `tblPr` / `tblGrid` / 范围标记。迭代实现，包裹层数不限。
        fn collect_rows(&mut self, tbl: NodeId, out: &mut Vec<Row>) {
            let dom = self.dom;
            let mut stack: Vec<Wrap> = vec![Wrap { node: tbl, sdt: None, revs: Vec::new() }];
            // 先进后出：一个包裹展开后，它的子节点要按文档序处理，所以逆序压栈
            while let Some(Wrap { node, sdt, revs }) = stack.pop() {
                if dom.is(node, w(LocalName::Tr)) {
                    let row = self.build_row(node, sdt.as_ref(), &revs);
                    out.push(row);
                    continue;
                }
                let content = self.wrapped_children(node, &sdt, &revs, "表格");
                for wrap in content.into_iter().rev() {
                    stack.push(wrap);
                }
            }
        }

        /// `w:tr` 的格：同 [`Self::collect_rows`]，跳过 `trPr` / `tblPrEx`。
        fn collect_cells(&mut self, tr: NodeId, out: &mut Vec<Cell>) {
            let dom = self.dom;
            let mut stack: Vec<Wrap> = vec![Wrap { node: tr, sdt: None, revs: Vec::new() }];
            while let Some(Wrap { node, sdt, revs }) = stack.pop() {
                if dom.is(node, w(LocalName::Tc)) {
                    let cell = self.build_cell(node, sdt.as_ref(), &revs);
                    out.push(cell);
                    continue;
                }
                let content = self.wrapped_children(node, &sdt, &revs, "表格行");
                for wrap in content.into_iter().rev() {
                    stack.push(wrap);
                }
            }
        }

        /// 把 `node`（tbl / tr / sdt / sdtContent / customXml / 修订包裹）的子节点变成待处理项：
        /// `tr` / `tc` 原样返回，包裹元素带上新的上下文，属性元素与范围标记丢弃，其他记 `MOD_UNKNOWN_BLOCK`。
        fn wrapped_children(
            &mut self,
            node: NodeId,
            sdt: &Option<SdtInfo>,
            revs: &[Revision],
            where_: &str,
        ) -> Vec<Wrap> {
            let dom = self.dom;
            let mut out = Vec::new();
            for child in dom.semantic_children(node) {
                let Some(name) = dom.name(child) else { continue };
                if is_range_marker(name) {
                    continue;
                }
                if name.ns != NsId::W {
                    self.warn(
                        child,
                        DiagCode::ModUnknownBlock,
                        format!("{where_}里无法分类的元素 {}", name.display(dom.interner())),
                    );
                    continue;
                }
                match name.local {
                    LocalName::Tr | LocalName::Tc => {
                        out.push(Wrap { node: child, sdt: sdt.clone(), revs: revs.to_vec() });
                    }
                    LocalName::Sdt => {
                        let info = SdtInfo::read(dom, child);
                        if let Some(content) = dom
                            .semantic_children(child)
                            .find(|&n| dom.is(n, w(LocalName::SdtContent)))
                        {
                            out.push(Wrap { node: content, sdt: Some(info), revs: revs.to_vec() });
                        }
                    }
                    LocalName::SdtContent | LocalName::CustomXml | LocalName::SmartTag => {
                        out.push(Wrap { node: child, sdt: sdt.clone(), revs: revs.to_vec() });
                    }
                    LocalName::Ins | LocalName::Del | LocalName::MoveFrom | LocalName::MoveTo => {
                        let meta = self.meta(child);
                        let rev = match name.local {
                            LocalName::Ins => Revision::Insert(meta),
                            LocalName::Del => Revision::Delete(meta),
                            LocalName::MoveFrom => Revision::MoveFrom(meta),
                            _ => Revision::MoveTo(meta),
                        };
                        let mut inner = revs.to_vec();
                        inner.push(rev);
                        out.push(Wrap { node: child, sdt: sdt.clone(), revs: inner });
                    }
                    LocalName::TblPr
                    | LocalName::TblGrid
                    | LocalName::TrPr
                    | LocalName::TblPrEx
                    | LocalName::TcPr
                    | LocalName::SdtPr
                    | LocalName::SdtEndPr
                    | LocalName::CustomXmlPr
                    | LocalName::SmartTagPr
                    | LocalName::ProofErr => {}
                    _ => self.warn(
                        child,
                        DiagCode::ModUnknownBlock,
                        format!("{where_}里无法分类的元素 {}", name.display(dom.interner())),
                    ),
                }
            }
            out
        }

        fn build_row(&mut self, tr: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Row {
            let dom = self.dom;
            let tr_pr = dom.semantic_children(tr).find(|&n| dom.is(n, w(LocalName::TrPr)));
            let props = boxed_row_props(dom, tr_pr, &mut self.warnings);
            let tbl_pr_ex = dom
                .semantic_children(tr)
                .find(|&n| dom.is(n, w(LocalName::TblPrEx)))
                .map(|ex| boxed_table_props(dom, Some(ex), &mut self.warnings));
            let mut revisions = revs.to_vec();
            if let Some(pr) = tr_pr {
                for n in dom.semantic_children(pr) {
                    if dom.is(n, w(LocalName::Ins)) {
                        revisions.push(Revision::Insert(self.meta(n)));
                    } else if dom.is(n, w(LocalName::Del)) {
                        revisions.push(Revision::Delete(self.meta(n)));
                    }
                }
            }
            if let Some((change, old)) = boxed_row_props_change(dom, tr_pr, &mut self.warnings) {
                revisions.push(Revision::RowPropsChange { meta: self.meta(change), old });
            }
            let mut cells = Vec::new();
            self.collect_cells(tr, &mut cells);
            if cells.is_empty() {
                self.warn(tr, DiagCode::ModTableShape, "表格行没有单元格");
            }
            Row { node: tr, props, tbl_pr_ex, cells, sdt: sdt.cloned(), revisions }
        }

        fn build_cell(&mut self, tc: NodeId, sdt: Option<&SdtInfo>, revs: &[Revision]) -> Cell {
            let dom = self.dom;
            let tc_pr = dom.semantic_children(tc).find(|&n| dom.is(n, w(LocalName::TcPr)));
            let props = boxed_cell_props(dom, tc_pr, &mut self.warnings);
            let mut revisions = revs.to_vec();
            if let Some(pr) = tc_pr {
                for n in dom.semantic_children(pr) {
                    let Some(name) = dom.name(n) else { continue };
                    if name.ns != NsId::W {
                        continue;
                    }
                    match name.local {
                        LocalName::CellIns => revisions.push(Revision::CellInsert(self.meta(n))),
                        LocalName::CellDel => revisions.push(Revision::CellDelete(self.meta(n))),
                        LocalName::CellMerge => revisions.push(Revision::CellMerge(self.meta(n))),
                        _ => {}
                    }
                }
            }
            if let Some((change, old)) = boxed_cell_props_change(dom, tc_pr, &mut self.warnings) {
                revisions.push(Revision::CellPropsChange { meta: self.meta(change), old });
            }
            // 格内内容：不带外层的 sdt / 修订上下文——它们属于格与行，段落自己的包裹在格里另算
            let mut blocks = Vec::new();
            self.build_container(tc, None, &[], &mut blocks);
            let ends_with_paragraph = blocks.last().is_some_and(|b| {
                dom.is(b.node(), w(LocalName::P))
                    || matches!(b, Block::Protected(p) if p.kind == ProtectedKind::TooDeep)
            });
            if !ends_with_paragraph {
                self.warn(tc, DiagCode::ModTableShape, "单元格不以 w:p 结尾");
            }
            Cell { node: tc, props, blocks, sdt: sdt.cloned(), revisions }
        }
    }

    // ---- 跨表格的块遍历（MOD-13 / EDIT-02 用）--------------------------------------------------------

    /// [`Document::block_at_mut`] 的实现：只借块表，不借整个 `Document`（刷新时构建器同时借着样式）。
    pub fn block_at_mut_in<'a>(main: &'a mut [Block], path: &[BlockStep]) -> Option<&'a mut Block> {
        let (first, rest) = path.split_first()?;
        let BlockStep::Main(i) = first else { return None };
        let mut cur = main.get_mut(*i)?;
        for step in rest {
            let BlockStep::Cell { row, cell, block } = step else { return None };
            let Block::Table(t) = cur else { return None };
            cur = t.rows.get_mut(*row)?.cells.get_mut(*cell)?.blocks.get_mut(*block)?;
        }
        Some(cur)
    }

    /// 从顶层块到某个块的一步（`Document::block_path`）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum BlockStep {
        /// `Document.main[i]`。
        Main(usize),
        /// 上一步是表格：`rows[row].cells[cell].blocks[block]`。
        Cell { row: usize, cell: usize, block: usize },
    }

    /// 深度优先、文档序的块迭代器：进单元格，不进文本框（那是别的内容流）。
    pub struct Blocks<'a> {
        stack: Vec<&'a Block>,
    }

    impl<'a> Iterator for Blocks<'a> {
        type Item = &'a Block;

        fn next(&mut self) -> Option<&'a Block> {
            let b = self.stack.pop()?;
            if let Block::Table(t) = b {
                for row in t.rows.iter().rev() {
                    for cell in row.cells.iter().rev() {
                        self.stack.extend(cell.blocks.iter().rev());
                    }
                }
            }
            Some(b)
        }
    }

    /// 一个块直接挂着的文本框内容流：`(块列表, 这些 `NodeId` 属于哪个 part)`；`None` = 与宿主同 part。
    pub fn box_flows(block: &Block) -> Vec<(&[Block], Option<PartId>)> {
        use crate::model::Display;
        let mut out: Vec<(&[Block], Option<PartId>)> = Vec::new();
        fn push<'b>(out: &mut Vec<(&'b [Block], Option<PartId>)>, d: Option<&'b Display>) {
            match d {
                Some(Display::Drawing(d)) => {
                    out.extend(d.shapes.iter().map(|s| (s.content.as_slice(), s.content_part)));
                }
                Some(Display::Vml(v)) => {
                    out.extend(v.shapes.iter().map(|s| (s.content.as_slice(), None)));
                }
                Some(Display::Formula(_)) | None => {}
            }
        }
        match block {
            Block::Text(t) => {
                for i in &t.inlines {
                    let crate::model::Inline::Run(r) = i else { continue };
                    for seg in &r.segments {
                        push(&mut out, seg.display.as_ref());
                    }
                }
            }
            Block::Image(b) => push(&mut out, b.display.as_ref()),
            Block::Protected(b) => push(&mut out, b.display.as_ref()),
            Block::Table(_) => {}
        }
        out.retain(|(blocks, _)| !blocks.is_empty());
        out
    }

    /// 块里有没有内容在**别的 part** 里的文本框（`wps:txbx/@r:txbx` → `word/txbx1.xml`）。
    ///
    /// 增量刷新（`MOD-13`）建不出那种内容：外部 part 的 DOM 与索引只在整体 `rebuild` 时装好。
    /// 碰上就退回整体重建（`TEST-07` 一步就抓到：`SetShapeStyle` 之后外部文本框的内容空了）。
    pub fn has_external_textbox(block: &Block) -> bool {
        use crate::model::Display;
        let external = |d: Option<&Display>| match d {
            Some(Display::Drawing(d)) => d.shapes.iter().any(|s| s.txbx_rel.is_some()),
            _ => false,
        };
        for b in Blocks::over(std::slice::from_ref(block)) {
            let hit = match b {
                Block::Text(t) => t.inlines.iter().any(|i| {
                    let crate::model::Inline::Run(r) = i else { return false };
                    r.segments.iter().any(|seg| external(seg.display.as_ref()))
                }),
                Block::Image(x) => external(x.display.as_ref()),
                Block::Protected(x) => external(x.display.as_ref()),
                Block::Table(_) => false,
            };
            if hit {
                return true;
            }
            for (content, _) in box_flows(b) {
                if content.iter().any(has_external_textbox) {
                    return true;
                }
            }
        }
        false
    }

    /// 深搜（表格 → 单元格，文本框 → 内容流）找 `part` 里的段落 `para`。`here` 是 `blocks` 所属的 part。
    fn text_block_deep(
        blocks: &[Block],
        here: PartId,
        part: PartId,
        para: NodeId,
    ) -> Option<&TextBlock> {
        for b in Blocks::over(blocks) {
            if here == part
                && b.node() == para
                && let Some(t) = b.as_text()
            {
                return Some(t);
            }
            for (content, cpart) in box_flows(b) {
                let hit = text_block_deep(content, cpart.unwrap_or(here), part, para);
                if hit.is_some() {
                    return hit;
                }
            }
        }
        None
    }

    impl<'a> Blocks<'a> {
        /// 任意块列表的深度遍历（页眉页脚 part、注释 / 批注条目、文本框内容流都用它）。
        pub fn over(blocks: &'a [Block]) -> Blocks<'a> {
            Blocks { stack: blocks.iter().rev().collect() }
        }
    }

    impl Document {
        /// 全部块，文档序，深入单元格（嵌套表也算）。
        pub fn blocks(&self) -> Blocks<'_> {
            Blocks::over(&self.main)
        }

        /// 某个 part 的顶层块列表：主 part 是正文，其余是页眉页脚 part 或注释 / 批注条目
        /// （一个 part 里所有条目的块按文档序接起来）。找不到这个 part → `None`。
        pub fn blocks_of_part(&self, part: PartId) -> Option<Vec<&Block>> {
            if part == self.main_part {
                return Some(self.main.iter().collect());
            }
            if let Some(hf) = self.hf_parts.get(&part) {
                return Some(hf.blocks.iter().collect());
            }
            for notes in [&self.footnotes, &self.endnotes] {
                if notes.part == Some(part) {
                    return Some(notes.items.iter().flat_map(|n| n.blocks.iter()).collect());
                }
            }
            if self.comments.part == Some(part) {
                return Some(self.comments.items.iter().flat_map(|c| c.blocks.iter()).collect());
            }
            None
        }

        /// 某个 part 的字段索引（`FLD-02`）：主 part 是 `fields`，辅助 part 在它自己的
        /// `AuxFlows` 里（页眉页脚 / 注释 / 批注 / 外部文本框 part）。找不到 → `None`。
        pub fn fields_in(&self, part: PartId) -> Option<&crate::span::field::FieldIndex> {
            if part == self.main_part {
                return Some(&self.fields);
            }
            if let Some(hf) = self.hf_parts.get(&part) {
                return Some(&hf.idx.fields);
            }
            for notes in [&self.footnotes, &self.endnotes] {
                if notes.part == Some(part) {
                    return notes.idx.as_ref().map(|i| &i.fields);
                }
            }
            if self.comments.part == Some(part) {
                return self.comments.idx.as_ref().map(|i| &i.fields);
            }
            self.aux_flows.get(&part).map(|i| &i.fields)
        }

        /// 任意 part 里的文本段落（含单元格内任意深度、**文本框内容流**），按 part + 节点找
        /// （`EDIT-02`）。文本框里的段落是独立内容流，不在 [`Blocks`] 的平铺里，所以要单独下去
        /// （`spec/18` 7.7：`InlinePos.para` 任意深度）。
        pub fn text_block_in(&self, part: PartId, para: NodeId) -> Option<&TextBlock> {
            // 先走平铺（正文 / 单元格）：绝大多数位置在这里就命中，代价与文本框那条路无关
            if let Some(tops) = self.blocks_of_part(part)
                && let Some(hit) = tops
                    .into_iter()
                    .find_map(|b| Blocks::over(std::slice::from_ref(b)).find(|x| x.node() == para))
                    .and_then(Block::as_text)
            {
                return Some(hit);
            }
            // 没命中才下到框里。宿主块可能在别的 part（页眉里的文本框），所以每个 part 都走一遍
            let mut parts = vec![self.main_part];
            parts.extend(self.hf_parts.keys().copied());
            parts.extend(
                [self.footnotes.part, self.endnotes.part, self.comments.part].iter().flatten(),
            );
            for here in parts {
                let Some(tops) = self.blocks_of_part(here) else { continue };
                for b in tops {
                    for (content, cpart) in box_flows(b) {
                        let hit = text_block_deep(content, cpart.unwrap_or(here), part, para);
                        if hit.is_some() {
                            return hit;
                        }
                    }
                    if let Block::Table(_) = b {
                        for x in Blocks::over(std::slice::from_ref(b)) {
                            for (content, cpart) in box_flows(x) {
                                let hit =
                                    text_block_deep(content, cpart.unwrap_or(here), part, para);
                                if hit.is_some() {
                                    return hit;
                                }
                            }
                        }
                    }
                }
            }
            None
        }

        /// 全部可编辑段落，含单元格内任意深度的。`text_blocks()` 仍只给顶层的。
        pub fn paragraphs(&self) -> impl Iterator<Item = &TextBlock> {
            self.blocks().filter_map(Block::as_text)
        }

        /// 全部表格，含嵌套表。
        pub fn tables(&self) -> impl Iterator<Item = &TableBlock> {
            self.blocks().filter_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
        }

        /// 从顶层到 `node` 所在块的路径；`node` 不是任何块的节点时 `None`。
        pub fn block_path(&self, node: NodeId) -> Option<Vec<BlockStep>> {
            let mut stack: Vec<(&Block, Vec<BlockStep>)> = self
                .main
                .iter()
                .enumerate()
                .rev()
                .map(|(i, b)| (b, vec![BlockStep::Main(i)]))
                .collect();
            while let Some((b, path)) = stack.pop() {
                if b.node() == node {
                    return Some(path);
                }
                if let Block::Table(t) = b {
                    for (ri, row) in t.rows.iter().enumerate().rev() {
                        for (ci, cell) in row.cells.iter().enumerate().rev() {
                            for (bi, inner) in cell.blocks.iter().enumerate().rev() {
                                let mut p = path.clone();
                                p.push(BlockStep::Cell { row: ri, cell: ci, block: bi });
                                stack.push((inner, p));
                            }
                        }
                    }
                }
            }
            None
        }

        /// 按路径取块。
        pub fn block_at(&self, path: &[BlockStep]) -> Option<&Block> {
            let (first, rest) = path.split_first()?;
            let BlockStep::Main(i) = first else { return None };
            let mut cur = self.main.get(*i)?;
            for step in rest {
                let BlockStep::Cell { row, cell, block } = step else { return None };
                let Block::Table(t) = cur else { return None };
                cur = t.rows.get(*row)?.cells.get(*cell)?.blocks.get(*block)?;
            }
            Some(cur)
        }

        /// 按路径取块（可变）。
        pub fn block_at_mut(&mut self, path: &[BlockStep]) -> Option<&mut Block> {
            block_at_mut_in(&mut self.main, path)
        }
    }

    impl Document {
        /// AGENT-02 的只读定位：复用原生流索引，单元格不另建流。
        pub fn flow_of_in(&self, part: PartId, node: NodeId) -> Option<crate::span::FlowId> {
            if part == self.main_part {
                return self.flows.flow_of(node);
            }
            if let Some(hf) = self.hf_parts.get(&part) {
                return hf.idx.flows.flow_of(node);
            }
            for notes in [&self.footnotes, &self.endnotes] {
                if notes.part == Some(part) {
                    return notes.idx.as_ref()?.flows.flow_of(node);
                }
            }
            if self.comments.part == Some(part) {
                return self.comments.idx.as_ref()?.flows.flow_of(node);
            }
            self.aux_flows.get(&part)?.flows.flow_of(node)
        }
        /// AGENT-01 的只读字段定位；Agent 无需遍历字段索引。
        pub fn field_projection_label(
            &self,
            part: PartId,
            id: crate::span::FieldId,
        ) -> Option<(NodeId, &str)> {
            let f = self.fields_in(part)?.get(id)?;
            Some((f.form.head(), f.instr.keyword.as_str()))
        }
        /// AGENT-05：字段缓存结果的只读文字；调用方无需自行遍历 DOM/Span。
        pub fn field_result_text(
            &self,
            pkg: &crate::package::Package,
            part: PartId,
            id: crate::span::FieldId,
        ) -> Option<String> {
            let field = self.fields_in(part)?.get(id)?;
            let dom = pkg.parts().get(part.0 as usize)?.dom()?;
            let mut seen = std::collections::BTreeSet::new();
            let mut out = String::new();
            let mut paragraph = None;
            for &root in field.form.result_nodes() {
                for n in dom.descendants(root) {
                    if !seen.insert(n) {
                        continue;
                    }
                    if dom.is(n, QName::w(LocalName::T)) || dom.is(n, QName::w(LocalName::DelText))
                    {
                        let here = dom.ancestors(n).find(|&n| dom.is(n, QName::w(LocalName::P)));
                        if paragraph.is_some() && here != paragraph {
                            out.push('\n');
                        }
                        paragraph = here;
                        for &c in dom.children(n) {
                            if let Some(text) = dom.text(c) {
                                out.push_str(&text);
                            }
                        }
                    } else if dom.is(n, QName::w(LocalName::Tab)) {
                        out.push('\t');
                    } else if dom.is(n, QName::w(LocalName::Br))
                        || dom.is(n, QName::w(LocalName::Cr))
                    {
                        out.push('\n');
                    }
                }
            }
            Some(out)
        }
    }

    /// AGENT-01：构建基块的可寻址身份与省略数量，正文模型不自动展开声明 part。
    /// 在包副本上惰性解析 glossary；调用方包的缓存、诊断与规范状态均不改变。
    pub fn glossary_flows(
        pkg: &crate::package::Package,
    ) -> crate::error::Result<Vec<(PartId, NodeId, crate::span::FlowId, usize)>> {
        use crate::package::RelType;
        let mut ids: std::collections::BTreeSet<_> =
            pkg.parts().iter().flat_map(|p| pkg.related(p.id, RelType::GlossaryDocument)).collect();
        ids.extend(
            pkg.parts()
                .iter()
                .filter(|p| {
                    p.content_type
                        .as_deref()
                        .is_some_and(|t| t.ends_with("wordprocessingml.document.glossary+xml"))
                })
                .map(|p| p.id),
        );
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut scratch = pkg.clone();
        let mut out = Vec::new();
        for id in ids {
            let Some(dom) = scratch.dom(id)? else {
                continue;
            };
            let flows = crate::span::FlowMap::build(dom);
            for &root in flows.roots() {
                if dom.is(root, QName::w(LocalName::DocPartBody)) {
                    let flow = flows.flow_of(root).expect("已枚举的流根有身份");
                    let paragraphs = dom
                        .descendants(root)
                        .filter(|&n| {
                            dom.is(n, QName::w(LocalName::P)) && flows.flow_of(n) == Some(flow)
                        })
                        .count();
                    out.push((id, root, flow, paragraphs));
                }
            }
        }
        Ok(out)
    }

    /// AGENT-01 使用的只读范围定位摘要；不向 Agent 暴露可变 Span 索引。
    pub struct RangeLocation {
        pub node: NodeId,
        pub id: u32,
        pub flow: crate::span::FlowId,
        pub pair_id: String,
        pub comment: bool,
        pub start: Option<(NodeId, u32)>,
        pub end: Option<(NodeId, u32)>,
    }
    impl Document {
        /// 既有范围的模型摘要，包含无可见文字的标记。
        pub fn range_locations_in(&self, part: PartId) -> Vec<RangeLocation> {
            let idx = if part == self.main_part {
                Some(&self.spans)
            } else if let Some(h) = self.hf_parts.get(&part) {
                Some(&h.idx.spans)
            } else if self.footnotes.part == Some(part) {
                self.footnotes.idx.as_ref().map(|i| &i.spans)
            } else if self.endnotes.part == Some(part) {
                self.endnotes.idx.as_ref().map(|i| &i.spans)
            } else if self.comments.part == Some(part) {
                self.comments.idx.as_ref().map(|i| &i.spans)
            } else {
                self.aux_flows.get(&part).map(|i| &i.spans)
            };
            let Some(idx) = idx else { return Vec::new() };
            idx.spans()
                .iter()
                .filter(|s| !s.removed)
                .map(|s| {
                    let anchor = s.start.as_ref().or(s.end.as_ref());
                    let node = anchor
                        .map(|a| a.marker.unwrap_or(a.container))
                        .unwrap_or_else(|| idx.flows().root_of(s.flow));
                    RangeLocation {
                        node,
                        id: s.id.0,
                        flow: s.flow,
                        pair_id: s.pair_id().to_owned(),
                        comment: s.class() == crate::span::RangeClass::Comment,
                        start: s.start.as_ref().map(|a| (a.container, a.index)),
                        end: s.end.as_ref().map(|a| (a.container, a.index)),
                    }
                })
                .collect()
        }
    }
}

// 主题声明值（`MOD-10`）：`a:theme/a:themeElements` 的字体方案与颜色方案。
// 只记录声明；主题字体 / 颜色的解析规则（槽位映射、tint/shade、空 EA 槽）在 `RES-05`。

use crate::semantic::props::{HexColorOrAuto, ThemeColor};

/// 颜色方案的 12 个槽位（`a:clrScheme` 子元素名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThemeSlot {
    Dk1,
    Lt1,
    Dk2,
    Lt2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hlink,
    FolHlink,
}

impl ThemeSlot {
    pub const ALL: [ThemeSlot; 12] = [
        ThemeSlot::Dk1,
        ThemeSlot::Lt1,
        ThemeSlot::Dk2,
        ThemeSlot::Lt2,
        ThemeSlot::Accent1,
        ThemeSlot::Accent2,
        ThemeSlot::Accent3,
        ThemeSlot::Accent4,
        ThemeSlot::Accent5,
        ThemeSlot::Accent6,
        ThemeSlot::Hlink,
        ThemeSlot::FolHlink,
    ];

    /// `a:clrScheme` 里的元素局部名。
    pub const fn local(self) -> LocalName {
        match self {
            ThemeSlot::Dk1 => LocalName::Dk1,
            ThemeSlot::Lt1 => LocalName::Lt1,
            ThemeSlot::Dk2 => LocalName::Dk2,
            ThemeSlot::Lt2 => LocalName::Lt2,
            ThemeSlot::Accent1 => LocalName::Accent1,
            ThemeSlot::Accent2 => LocalName::Accent2,
            ThemeSlot::Accent3 => LocalName::Accent3,
            ThemeSlot::Accent4 => LocalName::Accent4,
            ThemeSlot::Accent5 => LocalName::Accent5,
            ThemeSlot::Accent6 => LocalName::Accent6,
            ThemeSlot::Hlink => LocalName::Hlink,
            ThemeSlot::FolHlink => LocalName::FolHlink,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ThemeSlot::Dk1 => "dk1",
            ThemeSlot::Lt1 => "lt1",
            ThemeSlot::Dk2 => "dk2",
            ThemeSlot::Lt2 => "lt2",
            ThemeSlot::Accent1 => "accent1",
            ThemeSlot::Accent2 => "accent2",
            ThemeSlot::Accent3 => "accent3",
            ThemeSlot::Accent4 => "accent4",
            ThemeSlot::Accent5 => "accent5",
            ThemeSlot::Accent6 => "accent6",
            ThemeSlot::Hlink => "hlink",
            ThemeSlot::FolHlink => "folHlink",
        }
    }

    /// `w:themeColor` 的槽位映射（`RES-05`）：`dark1/text1 → dk1` 等；`none` 无槽位。
    pub const fn from_theme_color(c: ThemeColor) -> Option<ThemeSlot> {
        Some(match c {
            ThemeColor::Dark1 | ThemeColor::Text1 => ThemeSlot::Dk1,
            ThemeColor::Light1 | ThemeColor::Background1 => ThemeSlot::Lt1,
            ThemeColor::Dark2 | ThemeColor::Text2 => ThemeSlot::Dk2,
            ThemeColor::Light2 | ThemeColor::Background2 => ThemeSlot::Lt2,
            ThemeColor::Accent1 => ThemeSlot::Accent1,
            ThemeColor::Accent2 => ThemeSlot::Accent2,
            ThemeColor::Accent3 => ThemeSlot::Accent3,
            ThemeColor::Accent4 => ThemeSlot::Accent4,
            ThemeColor::Accent5 => ThemeSlot::Accent5,
            ThemeColor::Accent6 => ThemeSlot::Accent6,
            ThemeColor::Hyperlink => ThemeSlot::Hlink,
            ThemeColor::FollowedHyperlink => ThemeSlot::FolHlink,
            ThemeColor::None => return None,
        })
    }

    /// DrawingML `a:schemeClr/@val` 的名字（含别名 `tx1→dk1, bg1→lt1, tx2→dk2, bg2→lt2`）。
    pub fn from_scheme_name(s: &str) -> Option<ThemeSlot> {
        Some(match s {
            "dk1" | "tx1" => ThemeSlot::Dk1,
            "lt1" | "bg1" => ThemeSlot::Lt1,
            "dk2" | "tx2" => ThemeSlot::Dk2,
            "lt2" | "bg2" => ThemeSlot::Lt2,
            "accent1" => ThemeSlot::Accent1,
            "accent2" => ThemeSlot::Accent2,
            "accent3" => ThemeSlot::Accent3,
            "accent4" => ThemeSlot::Accent4,
            "accent5" => ThemeSlot::Accent5,
            "accent6" => ThemeSlot::Accent6,
            "hlink" => ThemeSlot::Hlink,
            "folHlink" => ThemeSlot::FolHlink,
            _ => return None,
        })
    }
}

/// 一组字体（`a:majorFont` / `a:minorFont`）：三个脚本槽位与 `a:font script→typeface` 表。空串视为无。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontSlots {
    pub node: Option<NodeId>,
    pub latin: Option<String>,
    pub ea: Option<String>,
    pub cs: Option<String>,
    /// `(script, typeface)`，按出现顺序（`Jpan`、`Hang`、`Hans`、`Hant`……）。
    pub scripts: Vec<(String, String)>,
}

impl FontSlots {
    pub fn script(&self, script: &str) -> Option<&str> {
        self.scripts.iter().find(|(s, _)| s == script).map(|(_, t)| t.as_str())
    }

    fn read(dom: &Dom, node: NodeId) -> FontSlots {
        let mut out = FontSlots { node: Some(node), ..Default::default() };
        for child in dom.semantic_children(node) {
            let Some(name) = dom.name(child) else { continue };
            if name.ns != NsId::A {
                continue;
            }
            let typeface = dom
                .attr_value(child, QName::new(NsId::None, LocalName::Typeface))
                .map(|s| s.into_owned())
                .filter(|s| !s.is_empty());
            match name.local {
                LocalName::Latin => out.latin = typeface,
                LocalName::Ea => out.ea = typeface,
                LocalName::Cs => out.cs = typeface,
                LocalName::Font => {
                    if let (Some(script), Some(t)) =
                        (dom.attr_value(child, QName::new(NsId::None, LocalName::Script)), typeface)
                    {
                        out.scripts.push((script.into_owned(), t));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// `a:fontScheme`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontScheme {
    pub node: NodeId,
    pub name: Option<String>,
    pub major: FontSlots,
    pub minor: FontSlots,
}

/// `a:clrScheme`：12 个槽位的 sRGB（`a:srgbClr/@val`，或 `a:sysClr/@lastClr`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    /// 内建调色板（[`ColorScheme::office_default`]）没有节点。
    pub node: Option<NodeId>,
    pub name: Option<String>,
    colors: [Option<[u8; 3]>; 12],
}

/// Word 内建 Office 调色板：文档没有 theme part 时，`schemeClr` / `themeColor` 仍按它解析
/// （`RES-05`；TS `DEFAULT_THEME_COLORS`）。顺序同 [`ThemeSlot::ALL`]。
pub const OFFICE_DEFAULT_COLORS: [[u8; 3]; 12] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x44, 0x54, 0x6A],
    [0xE7, 0xE6, 0xE6],
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
    [0x05, 0x63, 0xC1],
    [0x95, 0x4F, 0x72],
];

impl ColorScheme {
    /// 内建 Office 调色板。
    pub fn office_default() -> ColorScheme {
        ColorScheme {
            node: None,
            name: Some("Office".into()),
            colors: OFFICE_DEFAULT_COLORS.map(Some),
        }
    }

    pub fn get(&self, slot: ThemeSlot) -> Option<[u8; 3]> {
        self.colors[slot as usize]
    }

    /// 缺省值：`dk1 → 000000`、`lt1 → FFFFFF`（`RES-05`），其余槽位无缺省。
    pub fn get_or_default(&self, slot: ThemeSlot) -> Option<[u8; 3]> {
        self.get(slot).or(match slot {
            ThemeSlot::Dk1 => Some([0, 0, 0]),
            ThemeSlot::Lt1 => Some([0xFF, 0xFF, 0xFF]),
            _ => None,
        })
    }

    fn read(dom: &Dom, node: NodeId) -> ColorScheme {
        let mut colors = [None; 12];
        for child in dom.semantic_children(node) {
            let Some(name) = dom.name(child) else { continue };
            if name.ns != NsId::A {
                continue;
            }
            let Some(slot) = ThemeSlot::ALL.iter().copied().find(|s| s.local() == name.local)
            else {
                continue;
            };
            colors[slot as usize] = read_color(dom, child);
        }
        ColorScheme { node: Some(node), name: attr_name(dom, node), colors }
    }
}

/// 颜色槽位下第一个 `a:srgbClr`（取 `val`）或 `a:sysClr`（取 `lastClr`）。
fn read_color(dom: &Dom, slot: NodeId) -> Option<[u8; 3]> {
    for c in dom.semantic_children(slot) {
        let Some(name) = dom.name(c) else { continue };
        let attr = match (name.ns, name.local) {
            (NsId::A, LocalName::SrgbClr) => LocalName::Val,
            (NsId::A, LocalName::SysClr) => LocalName::LastClr,
            _ => continue,
        };
        let text = dom.attr_value(c, QName::new(NsId::None, attr))?;
        return HexColorOrAuto::parse(&text).and_then(HexColorOrAuto::rgb);
    }
    None
}

fn attr_name(dom: &Dom, node: NodeId) -> Option<String> {
    dom.attr_value(node, QName::new(NsId::None, LocalName::Name)).map(|s| s.into_owned())
}

/// `a:theme` 的声明值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub node: NodeId,
    pub name: Option<String>,
    pub fonts: Option<FontScheme>,
    pub colors: Option<ColorScheme>,
}

impl Theme {
    /// 根须是 `a:theme`，否则 `None`。缺 `a:themeElements` 时两个方案都为 `None`。
    pub fn from_dom(dom: &Dom) -> Option<Theme> {
        let root = dom.root();
        if !dom.is(root, QName::new(NsId::A, LocalName::Theme)) {
            return None;
        }
        let mut theme = Theme { node: root, name: attr_name(dom, root), fonts: None, colors: None };
        let elements = dom
            .semantic_children(root)
            .find(|&n| dom.is(n, QName::new(NsId::A, LocalName::ThemeElements)));
        let Some(elements) = elements else { return Some(theme) };
        for child in dom.semantic_children(elements) {
            let Some(name) = dom.name(child) else { continue };
            match (name.ns, name.local) {
                (NsId::A, LocalName::ClrScheme) if theme.colors.is_none() => {
                    theme.colors = Some(ColorScheme::read(dom, child));
                }
                (NsId::A, LocalName::FontScheme) if theme.fonts.is_none() => {
                    let mut major = FontSlots::default();
                    let mut minor = FontSlots::default();
                    for g in dom.semantic_children(child) {
                        match dom.name(g).map(|q| (q.ns, q.local)) {
                            Some((NsId::A, LocalName::MajorFont)) => {
                                major = FontSlots::read(dom, g)
                            }
                            Some((NsId::A, LocalName::MinorFont)) => {
                                minor = FontSlots::read(dom, g)
                            }
                            _ => {}
                        }
                    }
                    theme.fonts =
                        Some(FontScheme { node: child, name: attr_name(dom, child), major, minor });
                }
                _ => {}
            }
        }
        Some(theme)
    }
}

// 长度单位换算（`spec/15` 任务 4.2）。
//
// 模型层一律存 **EMU 原值**（`MOD-11`：显示模型只放文档事实）。px 是显示投影，只有
// `bind/compat_ts` 用得着——TS 的 `*Px` 字段按 96 dpi 换算。换算集中在这里，免得
// `9525` 这个魔数散落各处。
//
// | 单位 | 每单位 EMU | 出处 |
// | --- | --- | --- |
// | 英寸 | 914,400 | ECMA-376 |
// | 磅 pt | 12,700 | 1 pt = 1/72 in |
// | 缇 twip | 635 | 1 twip = 1/20 pt |
// | 像素 px | 9,525 | 96 dpi（TS 用同一个常量） |

/// 1 英寸的 EMU。
pub const EMU_PER_INCH: f64 = 914_400.0;
/// 1 磅的 EMU。
pub const EMU_PER_PT: f64 = 12_700.0;
/// 1 缇（1/20 磅）的 EMU。
pub const EMU_PER_TWIP: f64 = 635.0;
/// 1 像素的 EMU（96 dpi）。**只用于显示投影**。
pub const EMU_PER_PX: f64 = 9_525.0;

pub fn emu_to_px(emu: f64) -> f64 {
    emu / EMU_PER_PX
}

pub fn px_to_emu(px: f64) -> f64 {
    px * EMU_PER_PX
}

pub fn emu_to_pt(emu: f64) -> f64 {
    emu / EMU_PER_PT
}

pub fn pt_to_emu(pt: f64) -> f64 {
    pt * EMU_PER_PT
}

pub fn twips_to_emu(twips: f64) -> f64 {
    twips * EMU_PER_TWIP
}

pub fn emu_to_twips(emu: f64) -> f64 {
    emu / EMU_PER_TWIP
}

/// CSS / VML `style` 里的长度单位。VML 的 `style="width:96pt;margin-left:12.5pt"` 走这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Pt,
    Px,
    In,
    Cm,
    Mm,
    Pc,
    /// 没写单位。VML 里表示「用组的坐标系」，换算要靠上层的组比例（4.5 处理）。
    None,
}

impl LengthUnit {
    /// 该单位一个的 EMU。[`LengthUnit::None`] 没有绝对值。
    pub fn emu(self) -> Option<f64> {
        Some(match self {
            LengthUnit::Pt => EMU_PER_PT,
            LengthUnit::Px => EMU_PER_PX,
            LengthUnit::In => EMU_PER_INCH,
            LengthUnit::Cm => EMU_PER_INCH / 2.54,
            LengthUnit::Mm => EMU_PER_INCH / 25.4,
            // 1 pica = 12 pt
            LengthUnit::Pc => EMU_PER_PT * 12.0,
            LengthUnit::None => return None,
        })
    }
}

/// 一个带单位的长度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: LengthUnit,
}

impl Length {
    /// 绝对长度的 EMU；无单位 → `None`。
    pub fn to_emu(self) -> Option<f64> {
        Some(self.value * self.unit.emu()?)
    }
}

/// 解析 CSS 长度（`96pt`、`-12.5px`、`3.5`、`1in`）。单位大小写不敏感，允许前后空白。
pub fn parse_length(s: &str) -> Option<Length> {
    let s = s.trim();
    let split = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+'))
        .map_or(s.len(), |(i, _)| i);
    let (num, rest) = s.split_at(split);
    let value = num.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    let unit = match rest.trim().to_ascii_lowercase().as_str() {
        "" => LengthUnit::None,
        "pt" => LengthUnit::Pt,
        "px" => LengthUnit::Px,
        "in" => LengthUnit::In,
        "cm" => LengthUnit::Cm,
        "mm" => LengthUnit::Mm,
        "pc" => LengthUnit::Pc,
        _ => return None,
    };
    Some(Length { value, unit })
}

/// 解析 CSS `style` 属性为键值对（分号分隔、冒号赋值），键转小写、值去空白。
///
/// VML 把几何写在 `style` 里（`position:absolute;margin-left:36pt;width:96pt`），`MOD-11` 要求
/// `VmlDisplay` **原样保留**这些键值，所以这里不做语义解释。
pub fn parse_style(style: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if !k.is_empty() {
            out.push((k, v.to_string()));
        }
    }
    out
}

// VML 显示模型（`MOD-11`；`spec/15` 任务 4.5 / 4.7）。
//
// `w:pict` 与 `w:object` 里装的都是 VML：形状把几何写在 `style` 属性里（CSS 语法），填充与描边
// 写在 `fillcolor` / `strokecolor` 上。`MOD-11` 要求 `style` **原样保留**，所以这里不做语义解释，
// 只把键值对拆出来；要用的地方自己按 [`crate::model::parse_length`] 取长度。
//
// 组（`v:group`）用 `coordsize` 定义子坐标系，子形状的 `style` 里是组坐标不是绝对长度。这里
// 只记录关系（`VmlShape::parent`）与 `coordsize`，缩放留给用的人（4.6 的文本框投影）。
//
// 遍历是迭代的、带深度上限，和绘图那边同一条规矩。

/// VML 子树的深度上限，与绘图同值。
const VML_MAX_DEPTH: u32 = 64;

/// 摊平表最多穿几层**框**（`w:txbxContent`）。
///
/// `vml_display` 把整棵 `w:pict` 里的形状摊平成一张表，别人框里的形状也在表里（compat 要按
/// TS 的形态把它们当只读的兄弟框输出）。表里每一层都会重复列出更深的层，所以**表的规模**随
/// 嵌套层数是 O(n²)；语料里框套框最多 2 层（`textbox-edit__012`），Word 的界面根本做不出更深的。
/// 超过这个数的层不进表（内容仍在 DOM 里，`too_deep` 记 `MOD_TOO_DEEP`），
/// 这样 `corpus/hostile/hf-deep-txbx.docx` 那种 3000 层套娃不会把投影拖死。
pub(crate) const MAX_BOX_NESTING: u32 = 8;

/// 一个 `w:pict` / `w:object` 的 VML 内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmlDisplay {
    /// `w:pict` 或 `w:object` 节点。
    pub node: NodeId,
    /// 文档序的形状表；组内形状排在组之后，`parent` 指回组。
    pub shapes: Vec<VmlShape>,
    /// `w:object` 的嵌入对象信息。
    pub ole: Option<OleInfo>,
    /// 框套得比 `MAX_BOX_NESTING` 还深，摊平表在那一层截断（`MOD_TOO_DEEP`）。
    pub too_deep: bool,
}

impl VmlDisplay {
    /// 第一个带 `o:hr="t"` 的形状（HTML `<hr>` 导入的细横线）。
    pub fn rule(&self) -> Option<&VmlShape> {
        self.shapes.iter().find(|s| s.hr)
    }

    /// 第一个带 `v:imagedata` 的形状。
    pub fn image(&self) -> Option<&VmlShape> {
        self.shapes.iter().find(|s| s.imagedata.is_some())
    }

    pub fn has_textbox(&self) -> bool {
        self.shapes.iter().any(|s| s.has_textbox)
    }
}

named_enum! {
    /// VML 元素种类（`v:` 命名空间下的元素名）。
    pub enum VmlKind {
        Shape = "shape",
        Rect = "rect",
        RoundRect = "roundRect",
        Oval = "oval",
        Line = "line",
        Group = "group",
        /// `v:shapetype`：只是形状模板，不画东西。
        ShapeType = "shapeType",
        Other = "other",
    }
}

impl VmlKind {
    fn from_local(local: LocalName) -> Option<VmlKind> {
        Some(match local {
            LocalName::Shape => VmlKind::Shape,
            LocalName::Rect => VmlKind::Rect,
            LocalName::Roundrect => VmlKind::RoundRect,
            LocalName::Oval => VmlKind::Oval,
            LocalName::Line => VmlKind::Line,
            LocalName::Group => VmlKind::Group,
            LocalName::Shapetype => VmlKind::ShapeType,
            _ => return None,
        })
    }

    /// 会画出东西的形状（`v:shapetype` 只是模板）。
    pub fn is_drawn(self) -> bool {
        !matches!(self, VmlKind::ShapeType | VmlKind::Other)
    }
}

/// 这个节点是会画出东西的 VML 形状（`v:shapetype` 一类的模板不算）。
pub fn drawn_shape(dom: &Dom, node: NodeId) -> bool {
    dom.name(node).and_then(|n| VmlKind::from_local(n.local)).is_some_and(VmlKind::is_drawn)
}

/// 一个 VML 形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmlShape {
    pub node: NodeId,
    pub kind: VmlKind,
    /// `@style` 的键值对，键小写、值原样（`MOD-11`：不做语义解释）。
    pub style: Vec<(String, String)>,
    /// `@fillcolor`，归一成 6 位大写 hex；认不出的颜色写法 → `None`。
    pub fill_color: Option<String>,
    /// `@filled`（`f` / `false` 为假）。
    pub filled: Option<bool>,
    pub stroke_color: Option<String>,
    pub stroked: Option<bool>,
    /// `@coordsize`：组的子坐标系尺寸（整数对）。
    pub coordsize: Option<(i64, i64)>,
    /// `@coordorigin`：组子坐标系的原点（整数对）。画布里的孩子从它量起，不是从 0,0。
    pub coordorigin: Option<(i64, i64)>,
    /// `@path`：VML 自定义路径，坐标在 `coordsize` 定的空间里。
    pub path: Option<String>,
    /// `@type`：引用 `v:shapetype` 的 id（`#_x0000_t202` 是文本框、`t75` 是图片）。
    pub shape_type: Option<String>,
    /// `@o:spt`：形状类型号。`75` 同样是图片。
    pub spt: Option<String>,
    /// `v:imagedata/@r:id`。
    pub imagedata: Option<String>,
    /// `v:textpath/@string`（WordArt 文字）。
    pub textpath: Option<String>,
    /// `v:textpath/@style`：WordArt 的字体与字号写在这里。
    pub textpath_style: Option<String>,
    /// `@strokeweight`（多半带 `pt`）。
    pub stroke_weight: Option<String>,
    /// `v:fill` 元素上的颜色。WordArt 拿它当**文字**颜色。
    pub fill: Option<VmlFill>,
    /// `@o:hr="t"`：HTML `<hr>` 导入的细横线。
    pub hr: bool,
    /// 直接挂着 `v:textbox`。
    pub has_textbox: bool,
    /// `v:textbox/w:txbxContent`：框里的独立内容流。
    pub txbx: Option<NodeId>,
    /// 框里内容流的块（`MOD-11` 的 `content`）。由 `Document::rebuild` 复用段落管线构建。
    pub content: Vec<Block>,
    /// 所属 `v:group` 在 `shapes` 里的下标。
    pub parent: Option<usize>,
    /// 这个形状躺在别的形状的 `w:txbxContent` 里：它不是页面上的兄弟框，
    /// 也不占保存路径的序号。
    pub nested: bool,
}

impl VmlShape {
    /// `style` 里某个键的原值。
    pub fn style_get(&self, key: &str) -> Option<&str> {
        self.style.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// `style` 里某个键当长度解析（`width:96pt` 之类）。
    pub fn style_len(&self, key: &str) -> Option<Length> {
        parse_length(self.style_get(key)?)
    }

    /// `position:absolute`：脱离文字流。
    pub fn is_absolute(&self) -> bool {
        self.style_get("position").is_some_and(|v| v.eq_ignore_ascii_case("absolute"))
    }

    /// `visibility:hidden`。
    pub fn is_hidden(&self) -> bool {
        self.style_get("visibility").is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
    }

    /// 图片形状类型（`o:spt="75"` 或 `type="#_x0000_t75"`）。图解析不出来时它什么都不画。
    pub fn is_picture_type(&self) -> bool {
        self.spt.as_deref() == Some("75")
            || self.shape_type.as_deref().is_some_and(|t| t.contains("_x0000_t75"))
    }
}

/// `v:fill` 元素上的颜色。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmlFill {
    pub color: Option<String>,
    pub color2: Option<String>,
}

/// `w:object` 的嵌入对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OleInfo {
    /// `o:OLEObject` 节点。
    pub node: NodeId,
    /// `@ProgID`：`Excel.Sheet.12` 等。
    pub prog_id: Option<String>,
    /// `@Type`：`Embed` / `Link`。
    pub kind: Option<String>,
    /// `@DrawAspect`：`Content` / `Icon`。
    pub draw_aspect: Option<String>,
    /// `@r:id`：内嵌二进制 part（`embeddings/oleObject1.bin`）或外链的关系；删掉对象后它成为编辑引起的孤儿（6.7 回收）。
    pub rel_id: Option<String>,
    /// `w:object/@w:dxaOrig`（缇），预览图的声明宽度。
    pub dxa_orig: Option<i64>,
    pub dya_orig: Option<i64>,
}

/// 建一个 `w:pict` / `w:object` 的 VML 显示模型。
pub fn vml_display(dom: &Dom, node: NodeId) -> VmlDisplay {
    let mut shapes: Vec<VmlShape> = Vec::new();
    let mut ole = None;
    // (节点, 深度, 所属组下标, 穿过了几层别人的 txbxContent)
    let mut stack: Vec<(NodeId, u32, Option<usize>, u32)> = vec![(node, 0, None, 0)];
    let mut scratch: Vec<NodeId> = Vec::new();
    let mut too_deep = false;
    while let Some((n, depth, parent, boxes)) = stack.pop() {
        let nested = boxes > 0;
        let mut group = parent;
        if let Some(name) = dom.name(n) {
            if dom.is_ns(n, NsId::V, "v")
                && let Some(kind) = VmlKind::from_local(name.local)
            {
                shapes.push(shape(dom, n, kind, parent, nested));
                if kind == VmlKind::Group {
                    group = Some(shapes.len() - 1);
                }
            }
            if name.local == LocalName::OLEObject && ole.is_none() && dom.is_ns(n, NsId::O, "o") {
                ole = Some(OleInfo {
                    node: n,
                    prog_id: vml_attr(dom, n, NsId::None, LocalName::ProgID),
                    kind: vml_attr(dom, n, NsId::None, LocalName::UType),
                    draw_aspect: vml_attr(dom, n, NsId::None, LocalName::DrawAspect),
                    rel_id: vml_attr(dom, n, NsId::R, LocalName::Id),
                    dxa_orig: vml_num(dom, node, NsId::W, LocalName::DxaOrig),
                    dya_orig: vml_num(dom, node, NsId::W, LocalName::DyaOrig),
                });
            }
        }
        if depth >= VML_MAX_DEPTH {
            continue;
        }
        let boxes = boxes + u32::from(dom.is(n, QName::new(NsId::W, LocalName::TxbxContent)));
        if boxes > MAX_BOX_NESTING {
            too_deep = true;
            continue;
        }
        scratch.clear();
        scratch.extend(dom.semantic_children(n));
        stack.extend(scratch.iter().rev().map(|&c| (c, depth + 1, group, boxes)));
    }
    VmlDisplay { node, shapes, ole, too_deep }
}

fn shape(dom: &Dom, n: NodeId, kind: VmlKind, parent: Option<usize>, nested: bool) -> VmlShape {
    let mut s = VmlShape {
        node: n,
        kind,
        style: vml_attr(dom, n, NsId::None, LocalName::Style)
            .map(|v| parse_style(&v))
            .unwrap_or_default(),
        fill_color: vml_attr(dom, n, NsId::None, LocalName::Fillcolor).and_then(|v| vml_color(&v)),
        filled: vml_flag(dom, n, NsId::None, LocalName::Filled),
        stroke_color: vml_attr(dom, n, NsId::None, LocalName::Strokecolor)
            .and_then(|v| vml_color(&v)),
        stroked: vml_flag(dom, n, NsId::None, LocalName::Stroked),
        coordsize: vml_attr(dom, n, NsId::None, LocalName::Coordsize).and_then(|v| pair(&v)),
        coordorigin: vml_attr(dom, n, NsId::None, LocalName::Coordorigin).and_then(|v| pair(&v)),
        path: vml_attr(dom, n, NsId::None, LocalName::Path),
        shape_type: vml_attr(dom, n, NsId::None, LocalName::Type),
        spt: vml_attr(dom, n, NsId::O, LocalName::Spt),
        imagedata: None,
        textpath: None,
        textpath_style: None,
        stroke_weight: vml_attr(dom, n, NsId::None, LocalName::Strokeweight),
        fill: None,
        hr: dom.attr(n, QName::new(NsId::O, LocalName::Hr)).is_some_and(|_| {
            vml_attr(dom, n, NsId::O, LocalName::Hr).is_some_and(|v| v == "t" || v == "true")
        }),
        has_textbox: false,
        txbx: None,
        content: Vec::new(),
        parent,
        nested,
    };
    // 直接子节点上的图片 / WordArt 文字 / 文本框
    for c in dom.semantic_children(n) {
        let Some(name) = dom.name(c) else { continue };
        if !dom.is_ns(c, NsId::V, "v") {
            continue;
        }
        match name.local {
            LocalName::Imagedata if s.imagedata.is_none() => {
                s.imagedata = vml_attr(dom, c, NsId::R, LocalName::Id);
            }
            LocalName::Textpath if s.textpath.is_none() => {
                s.textpath = vml_attr(dom, c, NsId::None, LocalName::String);
                s.textpath_style = vml_attr(dom, c, NsId::None, LocalName::Style);
            }
            LocalName::Fill if s.fill.is_none() => {
                s.fill = Some(VmlFill {
                    color: vml_attr(dom, c, NsId::None, LocalName::Color),
                    color2: vml_attr(dom, c, NsId::None, LocalName::Color2),
                });
            }
            LocalName::Textbox => {
                s.has_textbox = true;
                s.txbx = dom
                    .semantic_children(c)
                    .find(|&t| dom.is(t, QName::new(NsId::W, LocalName::TxbxContent)));
            }
            _ => {}
        }
    }
    s
}

fn vml_attr(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<String> {
    dom.attr_value(node, QName::new(ns, local)).map(|s| s.trim().to_string())
}

fn vml_num(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<i64> {
    vml_attr(dom, node, ns, local)?.parse().ok()
}

/// VML 布尔属性：`f` / `false` 为假，其余（`t` / `true`）为真。
fn vml_flag(dom: &Dom, node: NodeId, ns: NsId, local: LocalName) -> Option<bool> {
    let v = vml_attr(dom, node, ns, local)?.to_ascii_lowercase();
    Some(!(v == "f" || v == "false"))
}

/// HTML 颜色名，VML 属性里常见（TS `VML_NAMED_COLORS`）。
const NAMED: &[(&str, &str)] = &[
    ("black", "000000"),
    ("white", "FFFFFF"),
    ("red", "FF0000"),
    ("green", "008000"),
    ("blue", "0000FF"),
    ("yellow", "FFFF00"),
    ("silver", "C0C0C0"),
    ("gray", "808080"),
    ("grey", "808080"),
    ("maroon", "800000"),
    ("olive", "808000"),
    ("navy", "000080"),
    ("purple", "800080"),
    ("teal", "008080"),
    ("fuchsia", "FF00FF"),
    ("lime", "00FF00"),
    ("aqua", "00FFFF"),
    ("cyan", "00FFFF"),
    ("orange", "FFA500"),
];

/// VML 颜色属性 → 6 位 hex（不带 `#`）。`#dbe5f1` / `#aaa` / `#dbe5f1 [3204]` / `silver` 都认。
///
/// **保留原大小写**：TS 的 VML 路径直接把 `fillcolor` 去掉 `#` 就用（`vml-textbox__002` 的
/// `borderColor` 是小写），只有细横线那条路会 `toUpperCase`。
pub fn vml_color(v: &str) -> Option<String> {
    let s = v.trim();
    let body = s.trim_start_matches('#');
    let head: String = body.chars().take(6).collect();
    if head.len() == 6 && head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(head);
    }
    // `#abc` 简写（后面不能再跟十六进制位）
    if let Some(short) = s.strip_prefix('#') {
        let three: String = short.chars().take(3).collect();
        if three.len() == 3
            && three.bytes().all(|b| b.is_ascii_hexdigit())
            && !short.chars().nth(3).is_some_and(|c| c.is_ascii_hexdigit())
        {
            return Some(three.chars().flat_map(|c| [c, c]).collect());
        }
    }
    let name = s.split([' ', '[']).next().unwrap_or("").to_ascii_lowercase();
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, h)| (*h).to_string())
}

/// `coordsize="2000,1000"`。
fn pair(v: &str) -> Option<(i64, i64)> {
    let (a, b) = v.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

pub use build::Document;

pub use omml::fragments;
pub use omml::latex::to_latex;
pub use omml::latex_to_omml::{latex_to_omml, math_paragraph_xml};
pub use omml::mathml::to_mathml;
pub use revision::{RevKind, RevOwner, RevisionEntry, RevisionId, RevisionIndex};

pub use table::{BlockStep, Blocks, Cell, GridCol, Row, box_flows, glossary_flows};

#[cfg(test)]
mod test_model {
    use super::custom_geom;
    use super::diagram_text;
    use super::drawing_display;
    use super::lenient_int;
    use super::*;
    use super::{DEFAULT_MARGIN, DEFAULT_PAGE_HEIGHT, DEFAULT_PAGE_WIDTH};
    use super::{PLOT_ELEMENTS, chartex_kind, palette, plot_kind, serial_date_text};
    use super::{heading_level_of_id, heading_level_of_name};
    use super::{is_custom_xml_item, publisher_element};
    use super::{vml_color, vml_display};
    use crate::resolve::drawingml::Rgb;
    use crate::semantic::props::{TblStyleOverrideType, ThemeColor};

    #[test]
    fn mod_11_chart_kind_table_covers_every_plot_element() {
        // 16 种图（ECMA-376 §21.2.2）都在表里，别的元素不是图
        assert_eq!(PLOT_ELEMENTS.len(), 16);
        for &l in PLOT_ELEMENTS {
            assert!(plot_kind(l).is_some());
        }
        assert_eq!(plot_kind(LocalName::PlotArea), None);
        assert_eq!(plot_kind(LocalName::DoughnutChart), Some(ChartKind::Pie));
        assert_eq!(plot_kind(LocalName::RadarChart), Some(ChartKind::Other));
        assert_eq!(chartex_kind("waterfall"), Some(ChartKind::Bar));
        assert_eq!(chartex_kind("regionMap"), None);
    }

    #[test]
    fn mod_11_excel_serial_dates() {
        assert_eq!(serial_date_text("37377").as_deref(), Some("5/1/2002"));
        assert_eq!(serial_date_text("37408").as_deref(), Some("6/1/2002"));
        assert_eq!(serial_date_text("1").as_deref(), Some("12/31/1899"));
        assert_eq!(serial_date_text("45658.4").as_deref(), Some("1/1/2025"));
        assert_eq!(serial_date_text("0"), None);
        assert_eq!(serial_date_text("80001"), None);
        assert_eq!(serial_date_text("abc"), None);
    }

    #[test]
    fn mod_11_palette_columns() {
        let office = ColorScheme::office_default();
        let hex6 = |p: [Rgb; 6]| p.map(crate::resolve::drawingml::hex);
        // 缺省 / 列 2 = 六个 accent
        assert_eq!(hex6(palette(None, &office).unwrap())[0], "4472C4");
        assert_eq!(hex6(palette(Some(2), &office).unwrap())[5], "70AD47");
        assert_eq!(hex6(palette(Some(10), &office).unwrap())[0], "4472C4");
        // 列 1 灰阶
        assert_eq!(hex6(palette(Some(1), &office).unwrap())[0], "595959");
        assert_eq!(hex6(palette(Some(41), &office).unwrap())[1], "D9D9D9");
        // 列 3–8 单色阶梯：以对应 accent 起头，六个颜色互不相同
        let mono = hex6(palette(Some(5), &office).unwrap());
        assert_eq!(mono[0], "A5A5A5");
        assert_eq!(mono.iter().collect::<std::collections::BTreeSet<_>>().len(), 6);
        assert_eq!(hex6(palette(Some(40), &office).unwrap())[0], "70AD47");
    }

    const CUSTGEOM_NS: &str = r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

    fn parse_custgeom(inner: &str) -> (Dom, Option<CustomGeom>) {
        let src = format!("<a:custGeom{CUSTGEOM_NS}>{inner}</a:custGeom>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let g = custom_geom(&dom, root);
        (dom, g)
    }

    const RECT: &str = concat!(
        r#"<a:avLst/><a:gdLst/><a:pathLst><a:path w="952500" h="476250">"#,
        r#"<a:moveTo><a:pt x="0" y="476250"/></a:moveTo>"#,
        r#"<a:lnTo><a:pt x="952500" y="476250"/></a:lnTo>"#,
        r#"<a:lnTo><a:pt x="952500" y="0"/></a:lnTo>"#,
        r#"<a:lnTo><a:pt x="0" y="0"/></a:lnTo>"#,
        r#"<a:close/></a:path></a:pathLst>"#,
    );

    #[test]
    fn mod_11_custgeom_straight_edges() {
        let (_, g) = parse_custgeom(RECT);
        let g = g.expect("geom");
        assert_eq!(g.paths.len(), 1);
        let p = &g.paths[0];
        assert_eq!((p.w, p.h), (Some(952_500), Some(476_250)));
        assert!(!p.fill_none && !p.stroke_none);
        assert_eq!(
            p.cmds,
            vec![
                GeomCmd::MoveTo([0, 476_250]),
                GeomCmd::LineTo([952_500, 476_250]),
                GeomCmd::LineTo([952_500, 0]),
                GeomCmd::LineTo([0, 0]),
                GeomCmd::Close,
            ]
        );
    }

    #[test]
    fn mod_11_custgeom_bails_on_formulas_and_arcs() {
        // 引导公式：坐标可能写成引导名，求不了值 → 整条作废
        let (_, g) = parse_custgeom(&format!(
            r#"<a:gdLst><a:gd name="adj" fmla="val 50000"/></a:gdLst>{RECT}"#
        ));
        assert!(g.is_none(), "有 gd 公式就不给路径");
        // 圆弧要转贝塞尔，不做
        let (_, g) = parse_custgeom(concat!(
            r#"<a:pathLst><a:path w="100" h="100">"#,
            r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo>"#,
            r#"<a:arcTo wR="50" hR="50" stAng="0" swAng="5400000"/>"#,
            r#"</a:path></a:pathLst>"#,
        ));
        assert!(g.is_none(), "有 arcTo 就不给路径");
        // 坐标是引导名而不是数字
        let (_, g) = parse_custgeom(concat!(
            r#"<a:pathLst><a:path w="100" h="100">"#,
            r#"<a:moveTo><a:pt x="hc" y="0"/></a:moveTo></a:path></a:pathLst>"#,
        ));
        assert!(g.is_none(), "非数字坐标就不给路径");
        // 没有 pathLst
        let (_, g) = parse_custgeom("<a:avLst/>");
        assert!(g.is_none());
    }

    #[test]
    fn mod_11_custgeom_fill_and_stroke_flags() {
        let (_, g) = parse_custgeom(concat!(
            r#"<a:pathLst><a:path w="10" h="10" fill="none" stroke="0">"#,
            r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo>"#,
            r#"<a:cubicBezTo><a:pt x="1" y="2"/><a:pt x="3" y="4"/><a:pt x="5" y="6"/></a:cubicBezTo>"#,
            r#"</a:path></a:pathLst>"#,
        ));
        let g = g.expect("geom");
        let p = &g.paths[0];
        assert!(p.fill_none && p.stroke_none);
        assert_eq!(p.cmds[1], GeomCmd::CubicTo([[1, 2], [3, 4], [5, 6]]));
        assert_eq!(p.cmds[1].letter(), 'C');
        assert_eq!(p.cmds[1].points().len(), 3);
    }

    use crate::semantic::props::{
        CharacterSpacing, DocProtect, FontFamily, FontPitch, Jc, MultiLevelType, NumberFormat,
    };

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn dom(xml: &str) -> Dom {
        Dom::parse(PartId(0), xml.as_bytes()).unwrap()
    }

    #[test]
    fn mod_10_styles_defaults_and_heading_levels() {
        let d = dom(&format!(
            r#"<w:styles xmlns:w="{W}">
                      <w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault>
                        <w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
                      <w:latentStyles w:count="1"><w:lsdException w:name="Normal"/></w:latentStyles>
                      <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
                      <w:style w:type="paragraph" w:default="1" w:styleId="Body"><w:name w:val="Body Text"/><w:pPr><w:jc w:val="both"/></w:pPr></w:style>
                      <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:link w:val="Heading1Char"/>
                        <w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
                      <w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Heading1"/><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>
                      <w:style w:type="paragraph" w:styleId="MyH"><w:name w:val="Custom"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
                      <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:semiHidden/><w:unhideWhenUsed/></w:style>
                      <w:style w:type="character" w:styleId="Heading1Char"><w:name w:val="Heading 1 Char"/><w:link w:val="Heading1"/><w:rPr><w:b/></w:rPr></w:style>
                      <w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4"/></w:tblBorders></w:tblPr>
                        <w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr><w:tcPr><w:shd w:val="clear" w:fill="D9D9D9"/></w:tcPr></w:tblStylePr></w:style>
                    </w:styles>"#
        ));
        let mut diags = Vec::new();
        let s = Styles::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(s.styles.len(), 8);
        assert!(s.latent_styles.is_some());
        assert_eq!(s.doc_default_rpr().unwrap().size, Some(Val::Value(22)));
        assert_eq!(
            s.doc_default_ppr().unwrap().spacing.as_ref().unwrap().after,
            Some(Val::Value(160))
        );
        // 默认样式：最后一个 default 胜出；无声明取第一个
        assert_eq!(s.default_for(StyleType::Paragraph).unwrap().id(), Some("Body"));
        assert_eq!(s.default_for(StyleType::Character).unwrap().id(), Some("DefaultParagraphFont"));
        assert_eq!(
            s.default_for(StyleType::Table),
            None,
            "无声明且无 Normal → 无默认（Word 行为）"
        );
        assert_eq!(s.default_for(StyleType::Numbering), None);
        // 无声明时退到 Normal
        let d2 = dom(&format!(
            r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/></w:style>
                       <w:style w:type="paragraph" w:styleId="a"><w:name w:val="Normal"/></w:style></w:styles>"#
        ));
        let s2 = Styles::from_dom(&d2, &mut Vec::new()).unwrap();
        assert_eq!(s2.default_for(StyleType::Paragraph).unwrap().id(), Some("a"));
        let h1 = s.get("Heading1").unwrap();
        assert_eq!(h1.kind(), Some(StyleType::Paragraph));
        assert_eq!(h1.based_on.as_deref(), Some("Normal"));
        assert_eq!(h1.link.as_deref(), Some("Heading1Char"));
        assert_eq!(h1.ui_priority, Some(Val::Value(9)));
        assert_eq!(h1.q_format, Some(true));
        assert_eq!(h1.ppr.as_ref().unwrap().keep_next, Some(true));
        assert_eq!(h1.rpr.as_ref().unwrap().size, Some(Val::Value(32)));
        assert_eq!(Styles::own_heading_level(h1), OwnHeadingLevel::Level(1));
        assert_eq!(
            Styles::own_heading_level(s.get("TOCHeading").unwrap()),
            OwnHeadingLevel::Blocked
        );
        assert_eq!(Styles::own_heading_level(s.get("MyH").unwrap()), OwnHeadingLevel::Level(3));
        assert_eq!(Styles::own_heading_level(s.get("Normal").unwrap()), OwnHeadingLevel::Inherit);
        assert_eq!(heading_level_of_name("Heading 3"), Some(3));
        assert_eq!(heading_level_of_name("heading3"), Some(3));
        assert_eq!(heading_level_of_name("Heading 10"), None);
        assert_eq!(heading_level_of_id("Heading9"), Some(9));
        assert_eq!(heading_level_of_id("Heading1Char"), None);
        let dpf = s.get("DefaultParagraphFont").unwrap();
        assert_eq!(dpf.semi_hidden, Some(true));
        assert_eq!(dpf.unhide_when_used, Some(true));
        let tg = s.get("TableGrid").unwrap();
        assert!(tg.tbl_pr.is_some());
        assert_eq!(tg.conditional.len(), 1);
        assert_eq!(tg.conditional[0].kind, Some(Val::Value(TblStyleOverrideType::FirstRow)));
        assert_eq!(tg.conditional[0].rpr.as_ref().unwrap().bold, Some(true));
        assert!(tg.conditional[0].tc_pr.is_some());
    }

    #[test]
    fn mod_10_numbering_declarations() {
        let d = dom(&format!(
            r#"<w:numbering xmlns:w="{W}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
                         xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14">
                      <w:abstractNum w:abstractNumId="0"><w:nsid w:val="0ABC1234"/><w:multiLevelType w:val="hybridMultilevel"/>
                        <w:lvl w:ilvl="0" w:tplc="04090001"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/><w:lvlJc w:val="left"/>
                          <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol" w:hint="default"/></w:rPr></w:lvl>
                        <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/><w:lvlRestart w:val="0"/><w:isLgl/></w:lvl>
                      </w:abstractNum>
                      <w:abstractNum w:abstractNumId="1"><w:numStyleLink w:val="ListNumber"/>
                        <w:lvl w:ilvl="0"><w:start w:val="1"/>
                          <mc:AlternateContent><mc:Choice Requires="w14"><w:numFmt w:val="custom" w:format="001, 002, 003, ..."/></mc:Choice><mc:Fallback><w:numFmt w:val="decimal"/></mc:Fallback></mc:AlternateContent>
                          <w:lvlText w:val="%1)"/></w:lvl>
                      </w:abstractNum>
                      <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
                      <w:num w:numId="2"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride>
                        <w:lvlOverride w:ilvl="1"><w:lvl w:ilvl="1"><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%2"/></w:lvl></w:lvlOverride></w:num>
                    </w:numbering>"#
        ));
        let mut diags = Vec::new();
        let n = Numbering::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(n.abstract_nums.len(), 2);
        assert_eq!(n.nums.len(), 2);
        let a0 = n.abstract_num(0).unwrap();
        assert_eq!(a0.nsid.as_deref(), Some("0ABC1234"));
        assert_eq!(a0.multi_level_type, Some(Val::Value(MultiLevelType::HybridMultilevel)));
        let l0 = a0.level(0).unwrap();
        assert_eq!(l0.tplc.as_deref(), Some("04090001"));
        assert_eq!(l0.start_or_default(), 1);
        assert_eq!(l0.num_fmt.as_ref().unwrap().val, Some(Val::Value(NumberFormat::Bullet)));
        assert_eq!(l0.lvl_text.as_ref().unwrap().val.as_deref(), Some("\u{F0B7}"));
        assert_eq!(l0.lvl_jc, Some(Val::Value(Jc::Left)));
        assert_eq!(
            l0.ppr.as_ref().unwrap().indent.as_ref().unwrap().hanging,
            Some(Val::Value(360))
        );
        assert_eq!(
            l0.rpr.as_ref().unwrap().fonts.as_ref().unwrap().ascii.as_deref(),
            Some("Symbol")
        );
        let l1 = a0.level(1).unwrap();
        assert_eq!(l1.start_or_default(), 0, "缺 w:start 从 0 起");
        assert_eq!(l1.lvl_restart, Some(Val::Value(0)));
        assert_eq!(l1.is_lgl, Some(true));
        // w14 自定义格式：MCE 选中 Choice 分支
        let a1 = n.abstract_num(1).unwrap();
        assert_eq!(a1.num_style_link.as_deref(), Some("ListNumber"));
        let f = a1.level(0).unwrap().num_fmt.as_ref().unwrap();
        assert_eq!(f.val, Some(Val::Value(NumberFormat::Custom)));
        assert_eq!(f.format.as_deref(), Some("001, 002, 003, ..."));
        // num 与覆盖
        assert_eq!(n.num(1).unwrap().abstract_id(), Some(0));
        let n2 = n.num(2).unwrap();
        assert_eq!(n2.overrides.len(), 2);
        assert_eq!(n2.override_for(0).unwrap().start_override, Some(Val::Value(5)));
        let ov = n2.override_for(1).unwrap().lvl.as_ref().unwrap();
        assert_eq!(ov.num_fmt.as_ref().unwrap().val, Some(Val::Value(NumberFormat::UpperRoman)));
        assert!(n.num(3).is_none());
    }

    #[test]
    fn mod_10_settings_compat_facts_and_font_table() {
        let d = dom(&format!(
            r#"<w:settings xmlns:w="{W}" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">
                      <w:writeProtection w:recommended="1" w:algorithmName="SHA-512" w:hashValue="abc=" w:saltValue="s=" w:spinCount="100000"/>
                      <w:zoom w:percent="100"/><w:removePersonalInformation/><w:trackRevisions/>
                      <w:documentProtection w:edit="readOnly" w:enforcement="1"/>
                      <w:defaultTabStop w:val="420"/><w:autoHyphenation w:val="0"/><w:evenAndOddHeaders/>
                      <w:characterSpacingControl w:val="compressPunctuation"/>
                      <w:compat><w:spaceForUL/><w:balanceSingleByteDoubleByteWidth/><w:doNotLeaveBackslashAlone w:val="0"/>
                        <w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
                        <w:compatSetting w:name="overrideTableStyleFontSizeAndJustification" w:uri="http://schemas.microsoft.com/office/word" w:val="1"/></w:compat>
                      <w:rsids><w:rsidRoot w:val="00A1"/></w:rsids>
                      <w:themeFontLang w:val="en-US" w:eastAsia="zh-CN"/><w:decimalSymbol w:val="."/><w:listSeparator w:val=","/>
                      <w15:chartTrackingRefBased/>
                    </w:settings>"#
        ));
        let mut diags = Vec::new();
        let s = Settings::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        let wp = s.write_protection.as_ref().unwrap();
        assert_eq!(wp.recommended, Some(true));
        assert_eq!(wp.spin_count, Some(Val::Value(100_000)));
        assert_eq!(s.zoom.as_ref().unwrap().percent, Some(Val::Value(100)));
        assert_eq!(s.remove_personal_information, Some(true));
        assert_eq!(s.track_revisions, Some(true));
        let dp = s.document_protection.as_ref().unwrap();
        assert_eq!(dp.edit, Some(Val::Value(DocProtect::ReadOnly)));
        assert_eq!(dp.enforcement, Some(true));
        assert_eq!(s.default_tab_stop_or_default(), 420);
        assert_eq!(s.auto_hyphenation, Some(false));
        assert_eq!(s.even_and_odd_headers, Some(true));
        assert_eq!(
            s.character_spacing_control,
            Some(Val::Value(CharacterSpacing::CompressPunctuation))
        );
        assert!(s.rsids.is_some());
        assert_eq!(s.theme_font_lang.as_ref().unwrap().east_asia.as_deref(), Some("zh-CN"));
        assert_eq!(s.chart_tracking_ref_based, Some(true));
        let facts = s.compat_facts(&d);
        assert_eq!(facts.mode, Some(15));
        assert_eq!(facts.settings.len(), 2);
        let flags: Vec<String> =
            facts.flags.iter().map(|q| q.display(d.interner()).to_string()).collect();
        assert_eq!(
            flags,
            ["w:spaceForUL", "w:balanceSingleByteDoubleByteWidth"],
            "val=0 的开关不算"
        );

        let d = dom(&format!(
            r#"<w:fonts xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                      <w:font w:name="Calibri"><w:panose1 w:val="020F0502020204030204"/><w:charset w:val="00"/><w:family w:val="swiss"/><w:pitch w:val="variable"/>
                        <w:sig w:usb0="E0002AFF" w:usb1="C000247B" w:usb2="00000009" w:usb3="00000000" w:csb0="000001FF" w:csb1="00000000"/>
                        <w:embedRegular r:id="rId1" w:fontKey="{{ABC}}" w:subsetted="1"/></w:font>
                      <w:font w:name="宋体"><w:altName w:val="SimSun"/><w:family w:val="auto"/><w:pitch w:val="default"/></w:font>
                    </w:fonts>"#
        ));
        let ft = FontTable::from_dom(&d, &mut diags).unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(ft.fonts.len(), 2);
        let c = ft.get("Calibri").unwrap();
        assert_eq!(c.family, Some(Val::Value(FontFamily::Swiss)));
        assert_eq!(c.pitch, Some(Val::Value(FontPitch::Variable)));
        assert_eq!(c.sig.as_ref().unwrap().usb0.as_deref(), Some("E0002AFF"));
        let e = c.embed_regular.as_ref().unwrap();
        assert_eq!(e.id.as_deref(), Some("rId1"));
        assert_eq!(e.subsetted, Some(true));
        assert_eq!(ft.get("宋体").unwrap().alt_name.as_deref(), Some("SimSun"));
        assert!(Settings::from_dom(&d, &mut diags).is_none(), "根不是 w:settings");
    }

    const DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    fn data(pts: &str, cxns: &str) -> Dom {
        let src = format!(
            r#"<dgm:dataModel xmlns:dgm="{DGM}" xmlns:a="{A}"><dgm:ptLst>{pts}</dgm:ptLst><dgm:cxnLst>{cxns}</dgm:cxnLst></dgm:dataModel>"#
        );
        Dom::parse(PartId(0), src.as_bytes()).expect("parse")
    }

    fn pt(id: &str, text: &str, ty: &str) -> String {
        let ty = if ty.is_empty() { String::new() } else { format!(r#" type="{ty}""#) };
        format!(
            r#"<dgm:pt modelId="{id}"{ty}><dgm:t><a:p><a:r><a:t>{text}</a:t></a:r></a:p></dgm:t></dgm:pt>"#
        )
    }

    fn cxn(src: &str, dst: &str, ord: &str, ty: &str) -> String {
        let ty = if ty.is_empty() { String::new() } else { format!(r#" type="{ty}""#) };
        let ord = if ord.is_empty() { String::new() } else { format!(r#" srcOrd="{ord}""#) };
        format!(r#"<dgm:cxn modelId="c"{ty} srcId="{src}" destId="{dst}"{ord}/>"#)
    }

    #[test]
    fn tree_order_then_isolated_points() {
        let dom = data(
            &[
                pt("root", "Root", ""),
                pt("later", "Later", ""),
                pt("first", "First", ""),
                pt("leaf", "Leaf", ""),
                pt("alone", "Alone", ""),
                pt("pres", "IGNORED", "pres"),
            ]
            .concat(),
            &[
                cxn("root", "later", "9", "parOf"),
                cxn("root", "first", "1", ""),
                cxn("first", "leaf", "0", ""),
                cxn("root", "alone", "0", "presOf"),
            ]
            .concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("Root\nFirst\nLeaf\nLater\nAlone"));
    }

    #[test]
    fn cycles_self_loops_and_missing_src_ord_terminate() {
        // root ↔ later 成环、first 自指：没有根，全部按文件序当孤立点
        let dom = data(
            &[pt("root", "Root", ""), pt("later", "Later", ""), pt("first", "First", "")].concat(),
            &[
                cxn("root", "later", "", ""),
                cxn("later", "root", "", ""),
                cxn("first", "first", "", ""),
            ]
            .concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("Root\nLater\nFirst"));
        // 有根、子树里成环：环上的点各出现一次
        let dom = data(
            &[pt("a", "A", ""), pt("b", "B", ""), pt("c", "C", "")].concat(),
            &[cxn("a", "b", "", ""), cxn("b", "c", "", ""), cxn("c", "b", "", "")].concat(),
        );
        assert_eq!(diagram_text(&dom).as_deref(), Some("A\nB\nC"));
        assert_eq!(diagram_text(&data("", "")), None);
    }

    const DRAWING_NS: &str = concat!(
        r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
        r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
        r#" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
        r#" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing""#,
        r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
        r#" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#,
    );

    fn parse_drawing(inner: &str) -> (Dom, DrawingDisplay) {
        let src = format!("<w:drawing{DRAWING_NS}>{inner}</w:drawing>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let d = drawing_display(&dom, root);
        (dom, d)
    }

    const PIC: &str = concat!(
        r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
        r#"<pic:pic><pic:blipFill><a:blip r:embed="rId7"/></pic:blipFill></pic:pic>"#,
        r#"</a:graphicData></a:graphic>"#,
    );

    #[test]
    fn mod_11_inline_picture_facts() {
        let (_, d) = parse_drawing(&format!(
            r#"<wp:inline distT="0" distB="0"><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Logo" descr="a photo"/>{PIC}</wp:inline>"#
        ));
        assert_eq!(d.kind, DrawingKind::Picture);
        assert!(d.anchor.is_none(), "wp:inline 是随文，没有锚定几何");
        assert_eq!(d.extent, Some(Extent { cx: 914_400, cy: 457_200 }));
        assert_eq!(d.doc_pr.name.as_deref(), Some("Logo"));
        assert_eq!(d.doc_pr.descr.as_deref(), Some("a photo"));
        let p = d.picture().expect("picture").clone();
        assert_eq!(p.embed.as_deref(), Some("rId7"));
        assert!(p.link.is_none());
    }

    #[test]
    fn mod_11_anchor_geometry_defaults_and_values() {
        let (_, d) = parse_drawing(concat!(
            r#"<wp:anchor behindDoc="1" allowOverlap="0" distT="10" distB="20" distL="30" distR="40" relativeHeight="251658242">"#,
            r#"<wp:positionH relativeFrom="page"><wp:posOffset>-1270</wp:posOffset></wp:positionH>"#,
            r#"<wp:positionV relativeFrom="margin"><wp:align>center</wp:align><wp14:pctPosVOffset>25000</wp14:pctPosVOffset></wp:positionV>"#,
            r#"<wp:wrapSquare wrapText="left"/>"#,
            r#"<wp:extent cx="100" cy="200"/>"#,
            r#"</wp:anchor>"#,
        ));
        let a = d.anchor.expect("anchor");
        assert!(a.behind_doc);
        assert!(!a.allow_overlap, "allowOverlap=0");
        assert!(a.layout_in_cell, "没写 layoutInCell 时缺省为 true");
        assert!(!a.locked);
        assert_eq!(a.relative_height, Some(251_658_242));
        assert_eq!(
            a.dist,
            Dist { top: Some(10), bottom: Some(20), left: Some(30), right: Some(40) }
        );
        assert_eq!(a.h.relative_from.as_deref(), Some("page"));
        assert_eq!(a.h.offset_emu, Some(-1270));
        assert_eq!(a.v.relative_from.as_deref(), Some("margin"));
        assert_eq!(a.v.align.as_deref(), Some("center"));
        assert_eq!(a.v.pct, Some(25000));
        assert_eq!(a.wrap, Wrap::Square { text: Some("left".into()) });
        assert_eq!(a.wrap.text(), Some("left"));
    }

    #[test]
    fn mod_11_picture_crop_rotation_and_border() {
        let (_, d) = parse_drawing(concat!(
            r#"<wp:inline><wp:extent cx="100" cy="100"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic>"#,
            r#"<pic:blipFill><a:blip r:link="rId9"/><a:srcRect l="5000" b="10000"/>"#,
            r#"<a:stretch><a:fillRect t="1000"/></a:stretch></pic:blipFill>"#,
            r#"<pic:spPr><a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/></a:xfrm>"#,
            r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:prstDash val="dash"/></a:ln>"#,
            r#"</pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline>"#,
        ));
        let p = d.picture().expect("picture").clone();
        assert_eq!(p.link.as_deref(), Some("rId9"));
        assert!(p.embed.is_none());
        assert_eq!(p.crop, Some(RectFrac { l: 5000, t: 0, r: 0, b: 10000 }));
        assert_eq!(p.fill_rect, Some(RectFrac { l: 0, t: 1000, r: 0, b: 0 }));
        assert_eq!(p.rot_60k, Some(5_400_000));
        assert!(p.flip_h && !p.flip_v);
        let b = p.border.expect("border");
        assert_eq!(b.width_emu, Some(12700));
        assert!(!b.no_fill);
        assert_eq!(b.dash.as_deref(), Some("dash"));
        assert!(b.fill.is_some(), "颜色容器节点要留给 resolve::drawingml");
    }

    #[test]
    fn mod_11_textbox_content_is_not_this_drawings_geometry() {
        // 文本框里的图属于框内段落，不能被宿主 drawing 认领（`spec/15` 风险 3）。
        let (_, d) = parse_drawing(concat!(
            r#"<wp:anchor><wp:extent cx="100" cy="100"/><wp:docPr id="1" name="Box"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
            r#"<w:txbxContent><w:p><w:r><w:drawing><wp:inline><wp:extent cx="999" cy="888"/>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
            r#"<pic:pic><pic:blipFill><a:blip r:embed="rIdInner"/></pic:blipFill></pic:pic>"#,
            r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:txbxContent>"#,
            r#"</a:graphicData></a:graphic></wp:anchor>"#,
        ));
        assert_eq!(d.extent, Some(Extent { cx: 100, cy: 100 }), "取宿主的 extent");
        assert_eq!(d.kind, DrawingKind::Shape);
        assert!(d.picture().is_none(), "框里的 pic:pic 不属于宿主 drawing");
    }

    #[test]
    fn mod_11_deep_nesting_terminates() {
        // 恶意输入：深嵌套不能栈溢出，也不能死循环。
        let deep = format!("{}{}", "<a:grpSp>".repeat(500), "</a:grpSp>".repeat(500));
        let (_, d) =
            parse_drawing(&format!(r#"<wp:inline><wp:extent cx="1" cy="2"/>{deep}</wp:inline>"#));
        assert_eq!(d.extent, Some(Extent { cx: 1, cy: 2 }));
    }

    #[test]
    fn lenient_int_follows_parse_int() {
        assert_eq!(lenient_int("381000"), 381_000);
        assert_eq!(lenient_int("-95250"), -95_250);
        assert_eq!(lenient_int("abc"), 0);
        assert_eq!(lenient_int("-abc"), 0);
        assert_eq!(lenient_int("12abc"), 12);
        assert_eq!(lenient_int(" +7"), 7);
        assert_eq!(lenient_int(""), 0);
    }

    const SECTION_NS: &str =
        r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn mod_10_section_geometry_and_defaults() {
        let src = format!(
            concat!(
                "<w:body{}>",
                r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
                r#"<w:pgMar w:top="100" w:right="200" w:bottom="300" w:left="400"/>"#,
                r#"<w:cols w:num="2"/></w:sectPr></w:pPr></w:p>"#,
                "<w:p/>",
                "<w:sectPr/>",
                "</w:body>"
            ),
            SECTION_NS
        );
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let s = Sections::build(&dom);
        assert!(!s.is_empty());

        // 第一段（偏移落在第一个 sectPr 之前）归第一节
        let first = s.at(10).expect("section");
        assert_eq!(first.page_width, 11906);
        assert_eq!((first.margin_top, first.margin_left), (100, 400));
        assert_eq!(first.columns, 2);
        assert_eq!(first.body_width(), 11906 - 400 - 200);

        // 落在两者之间的偏移归正文末尾那个空 sectPr：一切取缺省
        let last = s.at(u32::MAX - 1).expect("section");
        assert_eq!((last.page_width, last.page_height), (DEFAULT_PAGE_WIDTH, DEFAULT_PAGE_HEIGHT));
        assert_eq!(last.margin_left, DEFAULT_MARGIN);
        assert_eq!(last.columns, 1);
    }

    /// 任务 5.1 / 5.9：几何走属性表之后，认不出的字面（`Val::Raw`）、缺失、以及**不是正数的
    /// 尺寸**都退到缺省，不会让 `body_width` 变成负数（`PROP-09` + hostile `sectpr-bad-values`）。
    ///
    /// 5.1 时 `w:h="-1"` 是照原值给的（"能解析的数就照给"）；5.9 改成也退缺省：`ST_TwipsMeasure`
    /// 本来就是无符号的，而 `SectionGeom` 的每个消费者都拿它做版面算术。声明值仍是 -1
    /// （`props` 里原样保留，写回不受影响），只有几何视图回退（`docs/04` §8）。
    #[test]
    fn prop_09_bad_section_values_fall_back_to_defaults() {
        let src = format!(
            concat!(
                "<w:body{}>",
                r#"<w:sectPr><w:pgSz w:w="abc" w:h="-1"/>"#,
                r#"<w:pgMar w:top="x" w:right="200" w:bottom="y" w:left="400"/>"#,
                r#"<w:cols w:num="0"/></w:sectPr>"#,
                "</w:body>"
            ),
            SECTION_NS
        );
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let g = *Sections::build(&dom).at(0).expect("section");
        assert_eq!(g.page_width, DEFAULT_PAGE_WIDTH, "w=\"abc\" 退到缺省");
        assert_eq!(g.page_height, DEFAULT_PAGE_HEIGHT, "h=\"-1\" 不是正数，退到缺省");
        assert_eq!(g.margin_top, DEFAULT_MARGIN);
        assert_eq!(g.margin_right, 200);
        assert_eq!(g.margin_bottom, DEFAULT_MARGIN);
        assert_eq!(g.margin_left, 400);
        assert_eq!(g.columns, 1, "num=0 至少一栏");
    }

    #[test]
    fn mod_10_custom_xml_item_paths() {
        assert!(is_custom_xml_item("customXml/item1.xml"));
        assert!(is_custom_xml_item("customXml/item12.xml"));
        assert!(!is_custom_xml_item("customXml/itemProps1.xml"));
        assert!(!is_custom_xml_item("customXml/item.xml"));
        assert!(!is_custom_xml_item("word/document.xml"));
    }

    #[test]
    fn mod_10_publisher_element_per_source_type() {
        assert_eq!(publisher_element("JournalArticle"), LocalName::JournalName);
        assert_eq!(publisher_element("InternetSite"), LocalName::InternetSiteTitle);
        assert_eq!(publisher_element("Book"), LocalName::Publisher);
    }

    #[test]
    fn mod_10_theme_fonts_and_colors() {
        let xml = format!(
            r#"<a:theme xmlns:a="{A}" name="Office Theme"><a:themeElements>
                      <a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
                        <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2>
                        <a:accent1><a:srgbClr val="4472c4"/></a:accent1><a:hlink><a:srgbClr val="0563C1"/></a:hlink></a:clrScheme>
                      <a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/>
                        <a:font script="Jpan" typeface="游ゴシック Light"/><a:font script="Hans" typeface="等线 Light"/></a:majorFont>
                        <a:minorFont><a:latin typeface="Calibri"/><a:ea typeface="宋体"/><a:cs typeface="Arial"/></a:minorFont></a:fontScheme>
                    </a:themeElements></a:theme>"#
        );
        let dom = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
        let t = Theme::from_dom(&dom).unwrap();
        assert_eq!(t.name.as_deref(), Some("Office Theme"));
        let c = t.colors.as_ref().unwrap();
        assert_eq!(c.name.as_deref(), Some("Office"));
        assert_eq!(c.get(ThemeSlot::Dk1), Some([0, 0, 0]));
        assert_eq!(c.get(ThemeSlot::Dk2), Some([0x44, 0x54, 0x6A]));
        assert_eq!(c.get(ThemeSlot::Accent1), Some([0x44, 0x72, 0xC4]));
        assert_eq!(c.get(ThemeSlot::Accent2), None);
        assert_eq!(c.get(ThemeSlot::Lt2), None);
        assert_eq!(c.get_or_default(ThemeSlot::Lt1), Some([0xFF, 0xFF, 0xFF]));
        let f = t.fonts.as_ref().unwrap();
        assert_eq!(f.major.latin.as_deref(), Some("Calibri Light"));
        assert_eq!(f.major.ea, None, "空串视为无");
        assert_eq!(f.major.script("Hans"), Some("等线 Light"));
        assert_eq!(f.minor.ea.as_deref(), Some("宋体"));
        assert_eq!(f.minor.cs.as_deref(), Some("Arial"));
        assert_eq!(ThemeSlot::from_theme_color(ThemeColor::Text1), Some(ThemeSlot::Dk1));
        assert_eq!(ThemeSlot::from_theme_color(ThemeColor::None), None);
        assert_eq!(ThemeSlot::from_scheme_name("bg2"), Some(ThemeSlot::Lt2));
        let office = ColorScheme::office_default();
        assert_eq!(office.get(ThemeSlot::Accent1), Some([0x44, 0x72, 0xC4]));
        assert_eq!(office.get(ThemeSlot::FolHlink), Some([0x95, 0x4F, 0x72]));
        assert!(office.node.is_none());
    }

    #[test]
    fn mod_10_theme_wrong_root_is_none() {
        let dom = Dom::parse(PartId(0), format!(r#"<a:foo xmlns:a="{A}"/>"#).as_bytes()).unwrap();
        assert!(Theme::from_dom(&dom).is_none());
    }

    #[test]
    fn mod_11_unit_conversions_round_trip() {
        assert_eq!(emu_to_px(914_400.0), 96.0);
        assert_eq!(emu_to_pt(914_400.0), 72.0);
        assert_eq!(emu_to_twips(914_400.0), 1440.0);
        assert_eq!(pt_to_emu(72.0), 914_400.0);
        assert_eq!(twips_to_emu(1440.0), 914_400.0);
        assert_eq!(px_to_emu(96.0), 914_400.0);
        // 语料里最常见的一张图：cx=914400 → 96px、cy=457200 → 48px
        assert_eq!(emu_to_px(457_200.0), 48.0);
    }

    #[test]
    fn mod_11_parse_length_units() {
        assert_eq!(parse_length("96pt"), Some(Length { value: 96.0, unit: LengthUnit::Pt }));
        assert_eq!(parse_length(" -12.5px "), Some(Length { value: -12.5, unit: LengthUnit::Px }));
        assert_eq!(parse_length("3.5"), Some(Length { value: 3.5, unit: LengthUnit::None }));
        assert_eq!(parse_length("1IN").unwrap().to_emu(), Some(914_400.0));
        assert_eq!(parse_length("2.54cm").unwrap().to_emu().unwrap().round(), 914_400.0);
        assert_eq!(parse_length("25.4mm").unwrap().to_emu().unwrap().round(), 914_400.0);
        assert_eq!(parse_length("6pc").unwrap().to_emu(), Some(914_400.0));
        assert_eq!(parse_length("3.5").unwrap().to_emu(), None);
        assert_eq!(parse_length("auto"), None);
        assert_eq!(parse_length("10em"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn mod_11_parse_style_keeps_pairs_verbatim() {
        let s = parse_style("position:absolute;MARGIN-LEFT: 36pt ;width:96pt;;bogus");
        assert_eq!(
            s,
            vec![
                ("position".to_string(), "absolute".to_string()),
                ("margin-left".to_string(), "36pt".to_string()),
                ("width".to_string(), "96pt".to_string()),
            ]
        );
        // 值里带冒号（mso-position 之类）只在第一个冒号处切
        assert_eq!(
            parse_style("mso-wrap-style:none:x"),
            vec![("mso-wrap-style".to_string(), "none:x".to_string())]
        );
    }

    const VML_NS: &str = concat!(
        r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
        r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
        r#" xmlns:v="urn:schemas-microsoft-com:vml""#,
        r#" xmlns:o="urn:schemas-microsoft-com:office:office""#,
    );

    fn parse_vml(inner: &str) -> (Dom, VmlDisplay) {
        let src = format!("<w:pict{VML_NS}>{inner}</w:pict>");
        let dom = Dom::parse(PartId(0), src.as_bytes()).expect("dom");
        let root = dom.root();
        let v = vml_display(&dom, root);
        (dom, v)
    }

    #[test]
    fn mod_11_vml_horizontal_rule() {
        // 语料 smartart-ole__005：HTML <hr> 导入的细横线
        let (_, v) = parse_vml(
            r##"<v:rect id="_x0000_i1026" style="width:0;height:1.5pt" o:hralign="center" o:hr="t" fillcolor="#aca899" stroked="f"/>"##,
        );
        let r = v.rule().expect("hr");
        assert_eq!(r.kind, VmlKind::Rect);
        assert_eq!(r.fill_color.as_deref(), Some("aca899"), "去掉 # 但保留原大小写");
        assert_eq!(r.stroked, Some(false));
        assert_eq!(r.style_len("height").unwrap().to_emu(), Some(1.5 * 12700.0));
        // width:0 是「铺满可用宽度」，不是 0 宽
        assert_eq!(r.style_get("width"), Some("0"));
    }

    #[test]
    fn mod_11_vml_group_records_coordsize_and_parent() {
        let (_, v) = parse_vml(concat!(
            r#"<v:group style="width:150pt;height:75pt" coordsize="2000,1000">"#,
            r#"<v:shape id="pic" style="position:absolute;width:200;height:100"><v:imagedata r:id="rId10"/></v:shape>"#,
            r#"<v:shape id="tb" style="position:absolute"><v:textbox><w:txbxContent/></v:textbox></v:shape>"#,
            r#"</v:group>"#,
        ));
        assert_eq!(v.shapes.len(), 3);
        assert_eq!(v.shapes[0].kind, VmlKind::Group);
        assert_eq!(v.shapes[0].coordsize, Some((2000, 1000)));
        assert_eq!(v.shapes[1].parent, Some(0));
        assert_eq!(v.shapes[2].parent, Some(0));
        assert_eq!(v.image().and_then(|s| s.imagedata.as_deref()), Some("rId10"));
        assert!(v.shapes[1].is_absolute());
        assert!(v.has_textbox());
        // 组坐标里的长度没有单位，换算要靠组比例（4.6）
        assert_eq!(v.shapes[1].style_len("width").unwrap().to_emu(), None);
    }

    #[test]
    fn mod_11_vml_wordart_and_hidden() {
        let (_, v) = parse_vml(
            r#"<v:shape style="visibility:hidden"><v:textpath string="HELLO"/></v:shape>"#,
        );
        assert_eq!(v.shapes[0].textpath.as_deref(), Some("HELLO"));
        assert!(v.shapes[0].is_hidden());
        assert!(v.rule().is_none());
    }

    #[test]
    fn mod_11_vml_color_forms() {
        assert_eq!(vml_color("#ACA899"), Some("ACA899".into()));
        assert_eq!(vml_color("aca899"), Some("aca899".into()));
        assert_eq!(vml_color("#ffffff [65535]"), Some("ffffff".into()));
        // HTML 颜色名与 `#abc` 简写也认（TS `vmlColorHex`）
        assert_eq!(vml_color("red"), Some("FF0000".into()));
        assert_eq!(vml_color("Silver [2]"), Some("C0C0C0".into()));
        assert_eq!(vml_color("#abc"), Some("aabbcc".into()));
        assert_eq!(vml_color("window"), None);
    }
    // 模型验收（`spec/06` 验收清单 MOD-03 / 05 / 06，任务 1.5–1.8）。

    use crate::diag::DiagCode;
    use crate::package::{PartId, Rels};
    use crate::semantic::props::Val;
    use crate::xml::{Dom, LocalName, QName};

    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";

    const V: &str = "urn:schemas-microsoft-com:vml";

    fn doc(body: &str) -> Dom {
        let xml = format!(
            r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:m="{M}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:v="{V}"><w:body>{body}</w:body></w:document>"#
        );
        Dom::parse(PartId(0), xml.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{xml}"))
    }

    fn styles(xml: &str) -> Styles {
        let d = Dom::parse(
            PartId(0),
            format!(r#"<w:styles xmlns:w="{W}">{xml}</w:styles>"#).as_bytes(),
        )
        .unwrap();
        Styles::from_dom(&d, &mut Vec::new()).unwrap()
    }

    fn build(body: &str) -> (Vec<Block>, Vec<crate::diag::Diagnostic>) {
        let d = doc(body);
        Document::build_main(&d, None, &Rels::default())
    }

    fn build_with(body: &str, s: &Styles) -> Vec<Block> {
        let d = doc(body);
        Document::build_main(&d, Some(s), &Rels::default()).0
    }

    fn text_of(b: &Block) -> String {
        b.as_text().expect("text block").text()
    }

    #[test]
    fn mod_06_coordinate_flow_acceptance() {
        // "Hello" + <w:tab/> + "World" → Hello\tWorld
        let (blocks, _) =
            build(r#"<w:p><w:r><w:t>Hello</w:t><w:tab/><w:t>World</w:t></w:r></w:p>"#);
        assert_eq!(text_of(&blocks[0]), "Hello\tWorld");
        let tb = blocks[0].as_text().unwrap();
        assert_eq!(tb.utf16_len(), 11);
        let Inline::Run(run) = &tb.inlines[0] else { panic!() };
        assert_eq!(run.segments.len(), 3);
        assert_eq!(run.segments[1].kind, SegmentKind::Tab);
        assert_eq!(run.segments[1].text, 5..6);
        assert_eq!(run.segment_text(&run.segments[2]), "World");

        // 含图片 run 的段落坐标流含 1 个 U+FFFC
        let (blocks, _) = build(
            r#"<w:p><w:r><w:t>A</w:t></w:r><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"/></a:graphic></wp:inline></w:drawing></w:r><w:r><w:t>B</w:t></w:r></w:p>"#,
        );
        let t = text_of(&blocks[0]);
        assert_eq!(t, format!("A{OBJECT_REPLACEMENT}B"));
        assert_eq!(t.chars().filter(|&c| c == OBJECT_REPLACEMENT).count(), 1);
        assert_eq!(blocks[0].as_text().unwrap().utf16_len(), 3);

        // 无 preserve 的 <w:t> x </w:t> 文本为 x；有 preserve 原样；xml:space 沿祖先继承、default 复位
        let (blocks, _) =
            build(r#"<w:p><w:r><w:t> x </w:t><w:t xml:space="preserve"> y </w:t></w:r></w:p>"#);
        assert_eq!(text_of(&blocks[0]), "x y ");
        let (blocks, _) = build(
            r#"<w:p xml:space="preserve"><w:r><w:t> a </w:t><w:t xml:space="default"> b </w:t></w:r></w:p>"#,
        );
        assert_eq!(text_of(&blocks[0]), " a b");

        // 换行 / 分页 / 连字符 / 代理对
        let (blocks, _) = build(
            r#"<w:p><w:r><w:t>a</w:t><w:br/><w:t>b</w:t><w:br w:type="page"/><w:cr/><w:noBreakHyphen/><w:softHyphen/><w:t>😀</w:t><w:lastRenderedPageBreak/><w:fldChar w:fldCharType="begin"/></w:r></w:p>"#,
        );
        let tb = blocks[0].as_text().unwrap();
        assert_eq!(tb.text(), format!("a\nb{OBJECT_REPLACEMENT}\n\u{2011}\u{00AD}😀"));
        assert_eq!(tb.utf16_len(), 9, "😀 占 2 个 UTF-16 单位");
        let Inline::Run(run) = &tb.inlines[0] else { panic!() };
        let kinds: Vec<&SegmentKind> = run.segments.iter().map(|s| &s.kind).collect();
        assert!(matches!(kinds[1], SegmentKind::Br { kind: BreakKind::TextWrapping, .. }));
        assert!(matches!(kinds[3], SegmentKind::Br { kind: BreakKind::Page, .. }));
        assert_eq!(kinds[8], &SegmentKind::LastRenderedPageBreak);
        assert_eq!(run.segments[8].utf16_len, 0);
        assert_eq!(run.segments[9].kind, SegmentKind::FldChar);
        assert_eq!(run.segments[7].utf16_len, 2);
        // 段区间覆盖且不重叠
        let mut pos = 0;
        for s in &run.segments {
            assert_eq!(s.text.start, pos);
            pos = s.text.end;
        }
        assert_eq!(pos as usize, run.text.len());
    }

    #[test]
    fn mod_06_symbols_atoms_and_run_props() {
        let (blocks, _) = build(
            r#"<w:p><w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:sym w:font="Wingdings" w:char="F0FC"/></w:r>
                   <m:oMath><m:r><m:t>x</m:t></m:r></m:oMath><w:br w:type="page"/>
                   <w:r><w:ruby><w:rt><w:r><w:t>rt</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>base</w:t></w:r></w:rubyBase></w:ruby></w:r>
                   <w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
        );
        let tb = blocks[0].as_text().unwrap();
        assert_eq!(tb.inlines.len(), 5);
        let Inline::Run(run) = &tb.inlines[0] else { panic!() };
        assert_eq!(run.props.bold, Some(true));
        assert_eq!(run.props.size, Some(Val::Value(28)));
        assert_eq!(run.text, "\u{F0FC}", "符号字体映射表在 M2，先按 U+F000 + (code & 0xFF)");
        assert!(
            matches!(&run.segments[0].kind, SegmentKind::Sym { font: Some(f), code: Some(0xF0FC) } if f == "Wingdings")
        );
        assert!(matches!(&tb.inlines[1], Inline::Atom(InlineAtom { kind: AtomKind::Math, .. })));
        assert!(matches!(
            &tb.inlines[2],
            Inline::Atom(InlineAtom { kind: AtomKind::BareBreak { kind: BreakKind::Page }, .. })
        ));
        let Inline::Run(ruby) = &tb.inlines[3] else { panic!() };
        assert!(matches!(&ruby.segments[0].kind, SegmentKind::Ruby { rt, .. } if rt == "rt"));
        let Inline::Run(fn_ref) = &tb.inlines[4] else { panic!() };
        assert!(
            matches!(&fn_ref.segments[0].kind, SegmentKind::FootnoteRef { id: Some(id) } if id == "1")
        );
        assert_eq!(tb.text(), format!("\u{F0FC}{o}{o}{o}{o}", o = OBJECT_REPLACEMENT));
        assert_eq!(tb.utf16_len(), 5);
    }

    #[test]
    fn mod_06_hyperlink_revisions_and_transparent_containers() {
        let (blocks, _) = build(
            r#"<w:p><w:hyperlink r:id="rId9" w:tooltip="tip"><w:r><w:t>link</w:t></w:r></w:hyperlink>
                   <w:hyperlink w:anchor="bm1"><w:r><w:t>in</w:t></w:r></w:hyperlink>
                   <w:ins w:id="3" w:author="A" w:date="2024-01-01T00:00:00Z"><w:r><w:t>new</w:t></w:r></w:ins>
                   <w:moveFrom w:id="4" w:author="B" w:date="2024-01-02T00:00:00Z"><w:r><w:delText>gone</w:delText></w:r></w:moveFrom>
                   <w:sdt><w:sdtPr/><w:sdtContent><w:r><w:t>sdt</w:t></w:r></w:sdtContent></w:sdt>
                   <w:bookmarkStart w:id="0" w:name="bm1"/><w:proofErr w:type="spellStart"/>
                   <w:r><w:rPr><w:rPrChange w:id="7" w:author="C" w:date="2024-01-03T00:00:00Z"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>chg</w:t></w:r>
                   <w:bookmarkEnd w:id="0"/></w:p>"#,
        );
        let tb = blocks[0].as_text().unwrap();
        assert_eq!(tb.text(), "linkinnewgonesdtchg", "范围标记与 proofErr 不占位");
        let runs: Vec<&Run> = tb
            .inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Run(r) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 6);
        assert!(
            matches!(&runs[0].link, Some(Link::Hyperlink { target: LinkTarget::External { rel_id, href: None }, tooltip: Some(t), .. }) if rel_id == "rId9" && t == "tip")
        );
        assert!(
            matches!(&runs[1].link, Some(Link::Hyperlink { target: LinkTarget::Internal { anchor }, .. }) if anchor == "bm1")
        );
        let ins = runs[2].rev.as_ref().unwrap();
        assert_eq!(ins.ins.as_ref().unwrap().author.as_deref(), Some("A"));
        assert!(ins.del.is_none());
        let mv = runs[3].rev.as_ref().unwrap();
        assert!(
            mv.del.is_some() && mv.move_from.is_some() && mv.ins.is_none(),
            "moveFrom 同时计入 del"
        );
        assert_eq!(runs[3].segments[0].kind, SegmentKind::DelText);
        assert!(runs[4].rev.is_none() && runs[4].link.is_none());
        let chg = runs[5].rev.as_ref().unwrap();
        let (meta, old) = chg.props_change.as_ref().unwrap();
        assert_eq!(meta.id.as_deref(), Some("7"));
        assert_eq!(old.italic, Some(true));
    }

    #[test]
    fn mod_03_text_kind_acceptance() {
        let s = styles(
            r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
                   <w:style w:type="paragraph" w:styleId="Sub"><w:name w:val="Sub"/><w:basedOn w:val="Heading1"/></w:style>
                   <w:style w:type="paragraph" w:styleId="ListPara"><w:name w:val="List Para"/><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="5"/></w:numPr></w:pPr></w:style>
                   <w:style w:type="paragraph" w:styleId="NoList"><w:name w:val="No List"/><w:basedOn w:val="ListPara"/><w:pPr><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr></w:style>
                   <w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Heading1"/><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>"#,
        );
        let body = r#"
              <w:p><w:pPr><w:pStyle w:val="Heading1"/><w:outlineLvl w:val="9"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="Sub"/></w:pPr><w:r><w:t>b</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="TOCHeading"/></w:pPr><w:r><w:t>c</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="ListPara"/></w:pPr><w:r><w:t>d</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="NoList"/></w:pPr><w:r><w:t>e</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="ListPara"/><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr><w:r><w:t>f</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="ListPara"/><w:numPr><w:ilvl w:val="2"/></w:numPr></w:pPr><w:r><w:t>g</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="Heading1"/><w:numPr><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>h</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>i</w:t></w:r></w:p>
              <w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>j</w:t></w:r></w:p>"#;
        let blocks = build_with(body, &s);
        let kinds: Vec<&TextKind> = blocks.iter().map(|b| &b.as_text().unwrap().kind).collect();
        assert_eq!(kinds[0], &TextKind::Paragraph, "outlineLvl=9 且样式为 Heading1 → Paragraph");
        assert_eq!(kinds[1], &TextKind::Heading { level: 1 }, "basedOn 继承标题级别");
        assert_eq!(kinds[2], &TextKind::Paragraph, "outlineLvl 9 阻断继承");
        assert_eq!(
            kinds[3],
            &TextKind::ListItem { list: ListRef { num_id: 5, ilvl: 1, from_style: true } }
        );
        assert_eq!(kinds[4], &TextKind::Paragraph, "样式 numId 0 取消继承编号");
        assert_eq!(kinds[5], &TextKind::Paragraph, "直接 numId 0 → 无编号");
        assert_eq!(
            kinds[6],
            &TextKind::ListItem { list: ListRef { num_id: 5, ilvl: 2, from_style: true } },
            "ilvl 直接、numId 来自样式"
        );
        assert_eq!(
            kinds[7],
            &TextKind::ListItem { list: ListRef { num_id: 9, ilvl: 0, from_style: false } },
            "ListRef 优先于 Heading"
        );
        assert_eq!(kinds[8], &TextKind::Heading { level: 3 }, "文档未定义的内建 Heading3");
        assert_eq!(kinds[9], &TextKind::Heading { level: 3 }, "直接 outlineLvl");
    }

    #[test]
    fn mod_05_body_rules() {
        let s = styles(
            r#"<w:style w:type="paragraph" w:styleId="Hidden"><w:name w:val="Hidden"/><w:rPr><w:vanish/></w:rPr></w:style>"#,
        );
        let body = r#"
              <w:p><w:r><w:t>text</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>
              <w:sdt><w:sdtContent><w:p><w:r><w:t>s1</w:t></w:r></w:p><w:p><w:r><w:t>s2</w:t></w:r></w:p></w:sdtContent></w:sdt>
              <w:sdt><w:sdtContent/></w:sdt>
              <w:bookmarkStart w:id="1" w:name="x"/><w:bookmarkEnd w:id="1"/>
              <w:br w:type="page"/>
              <w:ins w:id="2" w:author="A" w:date="2024-01-01T00:00:00Z"><w:p><w:r><w:t>inserted</w:t></w:r></w:p></w:ins>
              <w:altChunk r:id="rId5"/>
              <w:p><w:pPr><w:pStyle w:val="Hidden"/></w:pPr><w:r><w:t>hidden</w:t></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="Hidden"/></w:pPr><w:r><w:rPr><w:vanish w:val="0"/></w:rPr><w:t>shown</w:t></w:r></w:p>
              <w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p>
              <w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>with text</w:t></w:r></w:p>
              <w:p><m:oMathPara><m:oMath/></m:oMathPara></w:p>
              <w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><a:blip/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
              <w:p><w:r><w:drawing><wp:anchor><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></wp:anchor></w:drawing></w:r></w:p>
              <w:p><w:r><w:pict><v:rect o:hr="t" xmlns:o="urn:schemas-microsoft-com:office:office"/></w:pict></w:r></w:p>
              <w:p><w:r><w:object><v:shape/></w:object></w:r></w:p>
              <w:p><w:r><w:t>text</w:t><w:drawing><wp:inline><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
              <w:sectPr><w:pgSz w:w="11906"/></w:sectPr>"#;
        let d = doc(body);
        let (blocks, warnings) = Document::build_main(&d, Some(&s), &Rels::default());
        let kinds: Vec<String> = blocks
            .iter()
            .map(|b| match b {
                Block::Text(t) => format!("Text:{}", t.text()),
                Block::Table(_) => "Table".into(),
                Block::Image(_) => "Image".into(),
                Block::Protected(p) => format!("Protected:{}", p.kind.key()),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "Text:text",
                "Table",
                "Text:s1",
                "Text:s2",
                "Protected:protected.invisible",
                "Protected:protected.body_break",
                "Text:inserted",
                "Protected:protected.unknown",
                "Protected:protected.invisible",
                "Text:shown",
                "Protected:protected.section_break",
                "Text:with text",
                "Protected:protected.equation",
                "Image",
                "Protected:protected.chart",
                "Protected:protected.rule",
                "Protected:protected.ole",
                &format!("Text:text{OBJECT_REPLACEMENT}"),
                "Protected:protected.section_props",
            ]
        );
        // sdt 信息、修订包裹、预览、诊断
        assert!(
            blocks[2].sdt().is_some() && blocks[3].sdt().is_some() && blocks[0].sdt().is_none()
        );
        assert!(
            matches!(blocks[6].revisions(), [Revision::Insert(m)] if m.author.as_deref() == Some("A"))
        );
        let Block::Protected(unknown) = &blocks[7] else { panic!() };
        assert!(
            matches!(&unknown.kind, ProtectedKind::Unknown(q) if q.local == LocalName::AltChunk)
        );
        let Block::Protected(hidden) = &blocks[8] else { panic!() };
        assert_eq!(hidden.preview, "hidden");
        assert!(
            matches!(&blocks[10], Block::Protected(p) if p.kind == ProtectedKind::SectionBreak)
        );
        assert_eq!(warnings.iter().filter(|d| d.code == DiagCode::ModUnknownBlock).count(), 1);
        // 每条规则可单测
        let f = ParagraphFacts { has_sect_pr: true, visible_text: false, ..Default::default() };
        assert_eq!(
            classify_paragraph(&f),
            ("R10", ParaClass::Protected(ProtectedKind::SectionBreak))
        );
        let f = ParagraphFacts { has_sect_pr: true, visible_text: true, ..Default::default() };
        assert_eq!(classify_paragraph(&f), ("R19", ParaClass::Text));
        let (rule, class) = classify_body_child(&d, blocks[1].node());
        assert_eq!((rule, class), ("R02", BodyClass::Table));
    }

    #[test]
    fn mod_04_paragraph_facts() {
        let s = styles(
            r#"<w:style w:type="paragraph" w:styleId="TOC 2"><w:name w:val="toc 2"/></w:style>"#,
        );
        let d = doc(
            r#"<w:p><w:pPr><w:pStyle w:val="TOC 2"/><w:rPr><w:del w:id="1" w:author="a" w:date="2024-01-01T00:00:00Z"/></w:rPr>
                     <w:pPrChange w:id="2" w:author="a" w:date="2024-01-01T00:00:00Z"><w:pPr/></w:pPrChange></w:pPr>
                   <w:r><w:t> </w:t></w:r>
                   <w:r><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r>
                   <w:del w:id="3" w:author="a" w:date="2024-01-01T00:00:00Z"><w:r><w:delText>gone</w:delText></w:r></w:del>
                   <m:oMath/></w:p>"#,
        );
        let blocks = Document::build_main(&d, Some(&s), &Rels::default()).0;
        let tb = blocks[0].as_text().unwrap();
        let f = &tb.facts;
        assert!(f.visible_text, "delText 也算可见文本");
        assert!(f.visible_text_outside_boxes);
        assert_eq!(f.toc_style_level, Some(2));
        assert_eq!(f.picts.len(), 1);
        assert_eq!(f.picts[0].kind, PictKind::TextBox);
        assert_eq!(f.math.count, 1);
        assert!(f.revision.run_del && f.revision.para_mark_del && f.revision.ppr_change);
        assert!(!f.revision.run_ins);
        assert!(matches!(
            tb.revisions.as_slice(),
            [Revision::ParaMarkDelete(_), Revision::ParaPropsChange { .. }]
        ));
        // 只有文本框里有字：visible_text 为假
        let d2 = doc(
            r#"<w:p><w:r><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
        );
        let blocks = Document::build_main(&d2, None, &Rels::default()).0;
        let f = &blocks[0].as_text().unwrap().facts;
        assert!(!f.visible_text && !f.visible_text_outside_boxes);
    }

    #[test]
    fn mod_01_document_without_body_warns() {
        let xml = format!(r#"<w:document xmlns:w="{W}"/>"#);
        let d = Dom::parse(PartId(0), xml.as_bytes()).unwrap();
        let (blocks, warnings) = Document::build_main(&d, None, &Rels::default());
        assert!(blocks.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, DiagCode::ModUnparseable);
        let _ = QName::w(LocalName::Body);
    }
}
