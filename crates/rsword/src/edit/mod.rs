//! L4 编辑引擎：单文件实现。编辑语义、事务与协议保持不变。

#[cfg(test)]
use crate::diag::ValidationOrigin;
use crate::diag::{DiagCode, Diagnostic};
use crate::error::{Error, Result};
use crate::model::{
    Block, BreakKind, Cell, Document, EMU_PER_PX, HfKind, HfVariant, INK_NAME_PREFIX, Inline,
    RevKind, RevOwner, RevisionId, Row, Run, SdtInfo, SdtRefusal, Segment, SegmentKind, TableBlock,
    TextBlock, refusing_sdt, utf16_len,
};
use crate::package::ns_context::NamespaceContext;
use crate::package::{Package, PartFlavor, PartId, PartUri, RelTarget, RelType, Relationship};
use crate::resolve::Resolver;
use crate::save::SaveOptions;
#[cfg(test)]
use crate::semantic::props::plan_apply_settings;
use crate::semantic::props::{
    CellPropsPatch, Change, Merge, ParaPropsPatch, RowPropsPatch, RunProps, RunPropsPatch,
    SectionPropsPatch, SettingsPatch, TablePropsPatch, TblWidth, Val, emit_run_props,
    order_index_run_props, plan_apply_cell_props_at, plan_apply_para_props,
    plan_apply_row_props_at, plan_apply_run_props, plan_apply_section_props_at,
};
use crate::span::field::Keyword;
use crate::span::field::generate::index::IndexOptions;
use crate::span::field::generate::toc::{TocEntry, TocOptions};
use crate::span::field::generate::{index as index_gen, seq as seq_gen, toc as toc_gen};
use crate::span::{
    Affinity, Anchor, FieldId, FieldIndex, FlowId, RangeClass, RangeKind, RangeSpan, SpanId,
    SpanIndex, SpanOrigin, SpanPolicy, is_content_item, is_property_element, plan_save,
    plan_update,
};
use crate::xml::dom::{Latex, Omml};
use crate::xml::entities::FragmentText;
use crate::xml::{
    Dirty, Dom, LocalName, NewElement, NewNode, NodeEdit, NodeId, NodeKind, NsId, QName, Target,
    parse_fragment,
};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Write};
use std::ops::Range;
macro_rules! edit_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $($(#[$vmeta])* $variant,)+
        }
        impl From<$name> for &'static str {
            #[inline]
            fn from(value: $name) -> Self {
                match value { $($name::$variant => $text,)+ }
            }
        }
        impl ::core::fmt::Display for $name {
            #[inline]
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(<&str>::from(*self))
            }
        }
        impl ::core::str::FromStr for $name {
            type Err = Error;
            #[inline]
            fn from_str(value: &str) -> Result<Self> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(Error::edit(DiagCode::BindBadArgument,
                        format!("unknown {} value: {value}", stringify!($name)))),
                }
            }
        }
        impl ::serde::Serialize for $name {
            #[inline]
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(<&str>::from(*self))
            }
        }
        impl<'de> ::serde::Deserialize<'de> for $name {
            #[inline]
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::core::result::Result<Self, D::Error> {
                let value = <String as ::serde::Deserialize>::deserialize(deserializer)?;
                value.parse().map_err(|_| ::serde::de::Error::custom("unknown enum value"))
            }
        }
    };
}

macro_rules! context_option {
    ($(#[doc = $doc:expr] $method:ident($field:ident: $ty:ty);)*) => {
        $(
            #[doc = $doc]
            #[inline]
            pub fn $method(mut self, value: $ty) -> Self {
                self.$field = value;
                self
            }
        )*
    };
}
/// 三个表格属性操作同形：定位容器 → `plan_apply_*_at` → 标记所属表格刷新。
///
/// ```ignore
/// table_props_op!(set_cell_props, CellPropsPatch, Tc, TcPr, plan_apply_cell_props_at, "单元格");
/// ```

macro_rules! table_props_op {
    ($name:ident, $patch:ty, $owner:ident, $container:ident, $change:ident, $skip:expr,
     $plan_apply:path, $what:literal) => {
        fn $name(
            s: &mut EditSession,
            node: NodeId,
            patch: &$patch,
            ctx: &EditContext,
        ) -> Result<MutationResult> {
            let dom = s.dom();
            if (node.0 as usize) >= dom.node_count()
                || dom.node(node).dirty == Dirty::Deleted
                || !dom.is(node, QName::w(LocalName::$owner))
            {
                return Err(Error::edit(DiagCode::EditBadPosition, concat!("目标不是", $what)));
            }
            let mut result = MutationResult::default();
            // 追踪：先把旧值快照成 `w:*PrChange`（两个阶段，理由同 `SetRunProps`）
            if let Some(mut t) = Tracker::new(s.document(), ctx) {
                let (container, before) =
                    MutationPlan::props_site(dom, node, LocalName::$container);
                let mut plan = MutationPlan::new(s.main_part());
                if let Some(tbl) = MutationPlan::owning_table(dom, node) {
                    plan.touch(tbl);
                }
                match container {
                    Some(c) => {
                        t.snapshot(
                            &mut plan,
                            dom,
                            c,
                            LocalName::$change,
                            LocalName::$container,
                            $skip,
                        );
                    }
                    None => {
                        // 容器不存在：旧值全是默认，快照是一个空容器
                        let k = plan.node_edits.len();
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(node),
                            before,
                            node: NewElement::new(QName::w(LocalName::$container)),
                        });
                        let change = t
                            .marker(LocalName::$change)
                            .with_child(NewElement::new(QName::w(LocalName::$container)));
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::New(k),
                            before: None,
                            node: change,
                        });
                    }
                }
                if !plan.is_empty() {
                    result.absorb(s.commit_plan(plan)?);
                }
            }
            let dom = s.dom();
            let (container, before) = MutationPlan::props_site(dom, node, LocalName::$container);
            let mut plan = MutationPlan::new(s.main_part());
            $plan_apply(
                dom,
                Target::Node(node),
                container,
                before,
                patch,
                s.flavor(),
                &mut plan.node_edits,
            );
            if let Some(tbl) = MutationPlan::owning_table(dom, node) {
                plan.touch(tbl);
            }
            result.absorb(s.commit_plan(plan)?);
            Ok(result)
        }
    };
}
macro_rules! accept_reject {
    ($($kind:ident => $accept:expr, $reject:expr;)+) => {
        fn actions(kind: RevKind) -> (Act, Act) {
            match kind {
                $(RevKind::$kind => ($accept, $reject),)+
            }
        }
    };
}

// 编辑入口
// L4 编辑引擎（`spec/08-edit.md`，`docs/03` §8）。
//
// `EditOp → plan（只读）→ validate（只读）→ commit（机械写入，不可失败）→ model.refresh`。
// 任何一步 `Err` 都不留下半修改状态（`EDIT-05`）：[`EditSession::apply`] 在操作前对主 part 的 DOM
// 做快照，操作内部可以有多个 plan/commit 阶段（例如先拆 run 再改属性），任一阶段失败整体回滚。
// 偏移单位对外统一为 UTF-16 code unit，原子为一个 `U+FFFC`（`EDIT-02`）。
//
// M1（任务 1.11–1.13）覆盖：`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、
// `ReplaceInlines`、`ReplaceParaProps` 与块级 `InsertBlock` / `DeleteBlock` / `MoveBlock`（compat 路径），
// 不生成修订；范围标记不做 Anchor 变换（`SPAN-06` 在 M2），删除范围覆盖到标记时标记原地保留并记
// `EDIT_ANCHOR_UNMOVED`（`EngineInvariantViolation`）。
/// 修订作者（`track_changes` 开启时写入 `w:author` / `w:date`）。M1 不生成修订，字段保留供 M7。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionAuthor {
    pub author: String,
    pub date: Option<String>,
}
/// `EDIT-01`：一次操作的上下文。
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(default)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct EditContext {
    /// `Some` → 生成修订；`None` → 直接修改（EDIT-01）。
    pub track_changes: Option<RevisionAuthor>,
    /// 新 run 没有可继承的左侧 run 时使用的格式。
    pub default_run_props: Option<RunProps>,
    /// 删除批注范围后保留无引用的批注正文。
    pub keep_orphan_comments: bool,
    /// 更新字段结果后设置重算标志。
    pub mark_updated_fields_dirty: bool,
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditContext {
    context_option! {
        /// 设置修订作者；`None` 表示直接编辑。
        with_track_changes(track_changes: Option<RevisionAuthor>);
        /// 设置新 run 无可继承格式时使用的属性。
        with_default_run_props(default_run_props: Option<RunProps>);
        /// 设置更新字段后的重算标志。
        with_mark_updated_fields_dirty(mark_updated_fields_dirty: bool);
        /// 设置是否保留没有引用的批注正文。
        with_keep_orphan_comments(keep_orphan_comments: bool);
    }
}
/// `EDIT-02`：块位置在容器里的落点。`End(body)` 落在尾部 `w:sectPr` 之前。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BlockAt {
    Start(NodeId),
    Before(NodeId),
    After(NodeId),
    End(NodeId),
}
/// `EDIT-02`：块位置 = 哪个 part + 落点。`part` 为 `None` 表示主 part（任务 5.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockPos {
    pub part: Option<PartId>,
    pub at: BlockAt,
}
impl BlockPos {
    /// 主 part 里的落点。
    pub fn main(at: BlockAt) -> Self {
        Self { part: None, at }
    }

    /// 指定 part 里的落点（页眉页脚 part 等）。
    pub fn in_part(part: PartId, at: BlockAt) -> Self {
        Self { part: Some(part), at }
    }

    pub fn start(container: NodeId) -> Self {
        Self::main(BlockAt::Start(container))
    }

    pub fn before(block: NodeId) -> Self {
        Self::main(BlockAt::Before(block))
    }

    pub fn after(block: NodeId) -> Self {
        Self::main(BlockAt::After(block))
    }

    pub fn end(container: NodeId) -> Self {
        Self::main(BlockAt::End(container))
    }

    /// 落点涉及的节点（容器或参照块）。
    pub fn node(&self) -> NodeId {
        match self.at {
            BlockAt::Start(c) | BlockAt::End(c) => c,
            BlockAt::Before(n) | BlockAt::After(n) => n,
        }
    }
}
/// `InsertBlock` 的内容。
#[derive(Debug, Clone, PartialEq)]
pub enum NewBlock {
    /// 新段落：`props` 为完整的 `w:pPr`（`None` = 无 `pPr`）。
    Paragraph { props: Option<NewElement>, inlines: Vec<NewInline> },
    /// 新表格：`rows` × `cols`，`widths` 是各列宽（twips；缺省等分 9360），`style` 是 `tblStyle`，
    /// `header` 为真时首行带 `w:tblHeader`。每格一个空 `w:p`。
    Table { rows: u32, cols: u32, widths: Option<Vec<i32>>, style: Option<String>, header: bool },
    /// 任意块级片段（`w:p` / `w:tbl` / …），通常来自 [`crate::xml::parse_fragment`]。
    Xml(NewElement),
    /// 外层包裹（compat 侧显式给出的块级 `w:ins` / `w:del`，属性已填好）里放一个块。
    Wrapped { wrapper: NewElement, block: Box<NewBlock> },
    /// 新图表（任务 6.6）：图表 part + 内嵌工作簿 + 关系 + 绘图段落。`extent_emu` 缺省 5486400 × 3200400。
    /// 进入 `InsertBlock` 时先由 `chart_ops::materialize` 建好 part、换成 `Xml` 段落。
    Chart { chart: NewChart, extent_emu: Option<(i64, i64)> },
    /// 新图片（任务 6.7）：媒体 part（相同字节只建一个）+ `image` 关系 + 段落（随文或锚定）。
    Image(NewImage),
    /// 独立公式段（TS `mathParagraphXml`，7.5）：`omml` 是 `m:oMath` 的**内容**，
    /// `align` 是 `left` / `center` / `right`。
    MathPara { omml: NewMath, align: String },
    /// 新建浮动文本框（`spec/18` 7.7）：`wps:wsp` + `w:txbxContent`。Transitional 包发
    /// `mc:AlternateContent`（Choice + VML 孪生），Strict 包只发 Choice。`blocks` 为空时
    /// 放一个空格段（Word 不接受空文本框）。
    Textbox { look: ShapeLook, blocks: Vec<NewBlock> },
    /// 新建形状：`preset` 进 `a:prstGeom/@prst`。`text` 给了就当一个段落放进框里，
    /// 不给就没有 `wps:txbx`（纯图形）。
    Shape { preset: PresetGeom, look: ShapeLook, text: Option<String> },
    /// 新建线条 / 连接符（TS `LINE_KINDS`）：两点（EMU）定位置与大小，只有描边。
    /// 没有 VML 孪生（TS 同），永远浮在文字上（`wrapNone`）。
    Line { kind: LineKind, from: (i64, i64), to: (i64, i64), color: Option<String> },
    /// 新块字段（`FLD-09`，7.8）：条目由当前文档算，一条一段（`FLD-08` / `FLD-12` 的多段字段
    /// 形态：begin + 指令 + separate 在首段开头、end 在末段末尾）。
    Field(NewBlockField),
    /// SEQ 题注段（TS `generateCaptionXml`）：`<标签> <SEQ 字段> <说明>`；
    /// 编号 = 插入位置之前同标签的 `SEQ` 字段数 + 1。
    Caption { label: String, text: String },
    /// 一次插入好几个块（生成器展开成多段时用）。只由 `chart_ops::materialize` 产出。
    Many(Vec<NewBlock>),
}
/// `EDIT-03 InsertAtom` 的内容（`spec/18` 7.5）。每一种在坐标流里都恒占 1 个 UTF-16 单位。
#[derive(Debug, Clone, PartialEq)]
pub enum NewAtom {
    /// `w:r/w:br`；`clear` 只对文字换行有意义。
    Break { kind: crate::model::BreakKind, clear: Option<String> },
    /// `w:r/w:sym`：符号字体里的一个码位（`RES-05` 的反向）。
    Symbol { font: String, code: u32 },
    /// 脚注 / 尾注：新建条目（`w:id` 按 `EDIT-06`，part 不存在按 `SAVE-05` 建）+ 引用 run。
    NoteRef { endnote: bool, content: Vec<Vec<NewRun>> },
    /// 随文图片：run 内一个 `wp:inline`（不另起段落）。
    Image(NewImage),
    /// `m:oMath` 原子。
    Math(NewMath),
}
/// 公式的两种给法。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewMath {
    /// 现成的 OMML（可以是 `<m:oMath>…</m:oMath>`，也可以只给里面的内容）。
    Omml(String),
    /// LaTeX 源码，由 `xml::dom::Omml::try_from` 转成 OMML。
    Latex(String),
}
/// `EDIT-03 AddComment` 的内容。`text` 里的 `\n` 分段。
#[derive(Debug, Clone, Default, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewComment {
    pub author: String,
    pub initials: Option<String>,
    /// ISO 时间戳；`None` 时不写 `w:date`。
    pub date: Option<String>,
    pub text: String,
    /// 回复哪条批注（写 `commentsExtended` 的 `w15:paraIdParent`）。
    pub parent_id: Option<String>,
    pub done: bool,
}
/// 编辑操作枚举（EDIT-03）；各变体注明目标、边界与保真约束。
// 属性补丁（`ParaPropsPatch`）体积大；操作是一次性传入的值，不装箱。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum EditOp {
    /// `EDIT-03 InsertText`：`props == None` 且紧邻 `Text` 段 → 写入该 `w:t`；否则边界插入继承格式的新 run。
    InsertText {
        /// 操作落点；坐标单位和边界见本变体说明。
        at: InlinePos,
        /// 新文本内容。
        text: String,
        /// 新属性或属性补丁，继承及清空规则见本变体说明。
        props: Option<RunPropsPatch>,
    },
    /// `EDIT-03 DeleteRange`（同段）。
    DeleteRange {
        /// 来源或范围起点，语义见本变体说明。
        from: InlinePos,
        /// 目的地或范围终点，语义见本变体说明。
        to: InlinePos,
    },
    /// `EDIT-03 SetRunProps`：两端拆分 run，范围内每个 run 按 `PROP-06` 计划 `rPr` 变更。
    SetRunProps {
        /// 来源或范围起点，语义见本变体说明。
        from: InlinePos,
        /// 目的地或范围终点，语义见本变体说明。
        to: InlinePos,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: RunPropsPatch,
    },
    /// `EDIT-03 ReplaceInlines`（compat 路径）：段落全部内容子节点 `Deleted`，新内容为 `New`；`pPr` 不动。
    ReplaceInlines {
        /// 目标 part；可选值缺席时表示主 part。
        part: Option<PartId>,
        /// 目标段落节点 ID。
        para: NodeId,
        /// 替换后的行内内容序列。
        inlines: Vec<NewInline>,
    },
    /// `EDIT-03 SetParaProps`：`PROP-06` 计划 `pPr` 变更（无 `pPr` → `New` 插为第一子）。
    SetParaProps {
        /// 目标 part；可选值缺席时表示主 part。
        part: Option<PartId>,
        /// 目标段落节点 ID。
        para: NodeId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: ParaPropsPatch,
    },
    /// compat 路径：整个 `w:pPr` 替换为给定片段（`None` = 删除 `pPr`）。`EDIT-04` 的 `rawPPr` 语义。
    ReplaceParaProps {
        /// 目标 part；可选值缺席时表示主 part。
        part: Option<PartId>,
        /// 目标段落节点 ID。
        para: NodeId,
        /// 新属性或属性补丁，继承及清空规则见本变体说明。
        props: Option<NewElement>,
    },
    /// `EDIT-03 SetTableProps`：`w:tblPr` 按 `PROP-06` 合并（容器缺失时插为 `w:tbl` 第一个子元素）。
    SetTableProps {
        /// 目标表格节点。
        table: NodeId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: TablePropsPatch,
    },
    /// `EDIT-03 SetRowProps`：`w:trPr` 按 `PROP-06` 合并（容器缺失时插在 `w:tblPrEx` 之后、首个 `w:tc` 之前）。
    SetRowProps {
        /// 目标行节点。
        row: NodeId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: RowPropsPatch,
    },
    /// `EDIT-03 SetCellProps`：`w:tcPr` 按 `PROP-06` 合并（容器缺失时插为 `w:tc` 第一个子元素）。
    SetCellProps {
        /// 目标单元格节点。
        cell: NodeId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: CellPropsPatch,
    },
    /// `EDIT-03 InsertRow`：在第 `at` 行前插入一行；`template` 缺省取 `at` 的前一行（`at == 0` 取第 0 行），
    /// `trPr` / `tblPrEx` / 各 `tcPr` 字节克隆，内容为一个空 `w:p`（克隆模板格首段的 `pPr`）。
    InsertRow {
        /// 目标表格节点。
        table: NodeId,
        /// 操作落点；坐标单位和边界见本变体说明。
        at: u32,
        /// 用于克隆格式的模板节点。
        template: Option<NodeId>,
    },
    /// `EDIT-03 DeleteRow`：删第 `at` 行；被删行的 `vMerge restart` 会把下一行的 continue 提升为 restart。
    DeleteRow {
        /// 目标表格节点。
        table: NodeId,
        /// 操作落点；坐标单位和边界见本变体说明。
        at: u32,
    },
    /// `EDIT-03 InsertColumn`：在第 `at` 列前插入一列（`width` 是新列宽，twips）。
    InsertColumn {
        /// 目标表格节点。
        table: NodeId,
        /// 操作落点；坐标单位和边界见本变体说明。
        at: u32,
        /// 列宽，单位 twips。
        width: i32,
    },
    /// `EDIT-03 DeleteColumn`：删第 `at` 列。
    DeleteColumn {
        /// 目标表格节点。
        table: NodeId,
        /// 操作落点；坐标单位和边界见本变体说明。
        at: u32,
    },
    /// `EDIT-03 MergeCells`：合并网格坐标闭区间 `from..=to`（行, 列）。
    MergeCells {
        /// 目标表格节点。
        table: NodeId,
        /// 来源或范围起点，语义见本变体说明。
        from: (u32, u32),
        /// 目的地或范围终点，语义见本变体说明。
        to: (u32, u32),
    },
    /// `EDIT-03 InsertBlock`：`New` 子树。
    InsertBlock {
        /// 操作落点；坐标单位和边界见本变体说明。
        at: BlockPos,
        /// 新块内容。
        block: NewBlock,
    },
    /// `EDIT-03 DeleteBlock`：`Deleted`。`part` 为 `None` 表示主 part（任务 5.5）。
    DeleteBlock {
        /// 目标 part；可选值缺席时表示主 part。
        part: Option<PartId>,
        /// 目标节点 ID。
        node: NodeId,
    },
    /// `EDIT-03 MoveBlock`：同 part 走 `move_within_part`；`from` 与 `to.part` 不同的时候
    /// 走 `XML-12` 规则 E′（子树按目标 part 的作用域重解析前缀，`spec/18` 7.6）。
    /// `from` 为 `None` 表示主 part。
    MoveBlock {
        /// 来源或范围起点，语义见本变体说明。
        from: Option<PartId>,
        /// 目标节点 ID。
        node: NodeId,
        /// 目的地或范围终点，语义见本变体说明。
        to: BlockPos,
    },
    /// `EDIT-03 AddComment`（同段）：`comments.xml` 不存在则新建 part（`SAVE-05`），
    /// 正文里插范围标记与 `w:commentReference` run，`w:id` 按 `EDIT-06` 取最大值 + 1。
    AddComment {
        /// 来源或范围起点，语义见本变体说明。
        from: InlinePos,
        /// 目的地或范围终点，语义见本变体说明。
        to: InlinePos,
        /// 新批注的元数据与正文。
        comment: NewComment,
    },
    /// `EDIT-03 RemoveComment`：条目、范围标记与 reference run 一起删。
    RemoveComment {
        /// 目标对象在文档中的 ID。
        id: String,
    },
    /// `EDIT-03 SetCommentText`：改条目正文（保留第一个文字 run 的格式）与 `w15:done`。
    SetCommentText {
        /// 目标对象在文档中的 ID。
        id: String,
        /// 新文本内容。
        text: String,
        /// 批注完成状态；缺席时保留。
        done: Option<bool>,
    },
    /// `EDIT-03 SplitParagraph`：`at` 之后的内容搬进新段落（`pPr` 字节克隆）。
    /// 透明字段会因此跨段 → `Err(EDIT_SPLIT_FIELD)`。
    SplitParagraph {
        /// 操作落点；坐标单位和边界见本变体说明。
        at: InlinePos,
    },
    /// `EDIT-03 MergeWithNext`：下一段内容接到本段末尾，下一段删除（保留**前**段的 `pPr`）。
    MergeWithNext {
        /// 目标 part；可选值缺席时表示主 part。
        part: Option<PartId>,
        /// 目标段落节点 ID。
        para: NodeId,
    },
    /// `EDIT-03 AddBookmark`（同段）：`w:id` 按 `EDIT-06` 取最大值 + 1；名字全文档唯一。
    AddBookmark {
        /// 书签或命名对象的名称。
        name: String,
        /// 来源或范围起点，语义见本变体说明。
        from: InlinePos,
        /// 目的地或范围终点，语义见本变体说明。
        to: InlinePos,
    },
    /// `EDIT-03 RemoveBookmark`：按名字删（标记 `Deleted`，索引里作废）。
    RemoveBookmark {
        /// 书签或命名对象的名称。
        name: String,
    },
    /// `FLD-12 InsertField`：生成 begin / instrText / separate / 结果 / end 五组 run。
    InsertField {
        /// 操作落点；坐标单位和边界见本变体说明。
        at: InlinePos,
        /// 目标字段 ID。
        field: NewField,
    },
    /// `FLD-07 Link`：改链接目标。HYPERLINK 字段只重写 `instrText`（开关原样保留）；
    /// `w:hyperlink` 元素改 `r:id`（外部 URL 按 `EDIT-06` 分配关系）或 `w:anchor`。
    SetLinkTarget {
        /// 目标超链接引用。
        link: LinkRef,
        /// 链接或操作目标。
        target: LinkDest,
    },
    /// `FLD-10`：FORMCHECKBOX 的 `w:checked` 取反。
    ToggleCheckbox {
        /// 目标字段 ID。
        field: FieldId,
    },
    /// `FLD-10`：FORMTEXT 的结果文字。
    SetFormText {
        /// 目标字段 ID。
        field: FieldId,
        /// 新文本内容。
        text: String,
    },
    /// `FLD-07`：字段结果 run 的格式。
    SetFieldResultProps {
        /// 目标字段 ID。
        field: FieldId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: RunPropsPatch,
    },
    /// `FLD-09`：用给定的块替换块字段的 `separate..end`（生成器在 M7；`w:fldLock` 拒绝）。
    UpdateBlockField {
        /// 目标字段 ID。
        field: FieldId,
        /// 按文档顺序排列的新块内容。
        blocks: Vec<NewBlock>,
    },

    // ---- 图表与 part（`EDIT-03`，任务 6.6）------------------------------------------------------
    /// `EDIT-03 SetChartData`：改图表 part 里的缓存文本（标题 / 系列名 / 值 / 类别），结构与引用不动
    /// （`spec/08`「`chart.ts` 补丁语义」）；chartex part → `Err(EDIT_UNSUPPORTED)`。
    SetChartData {
        /// 目标 part；可选值缺席时表示主 part。
        part: PartId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: ChartPatch,
    },
    /// 整个 XML part 换成给定内容（TS `partXml`）。只接受已存在的 XML part（不存在 → `EDIT_TARGET_MISSING`）。
    ReplacePartXml {
        /// 目标 part；可选值缺席时表示主 part。
        part: PartId,
        /// 原始 XML 字符串逃生口。
        xml: String,
    },
    /// 整个 part 换成给定字节（TS `partBinary`）。只接受已存在的 part，主 part 除外。
    ReplacePartBytes {
        /// 目标 part；可选值缺席时表示主 part。
        part: PartId,
        /// 替换或插入的原始二进制字节。
        bytes: Vec<u8>,
    },
    /// `EDIT-04 ReplaceImageMedia`（TS `xml.replaceImage`，任务 6.7）：`drawing` 子树里第一个 `a:blip` 改指新媒体
    /// （字节按内容去重落成媒体 part），删裁剪窗、清填充窗、删 `svgBlip` 扩展。
    ReplaceImageMedia {
        /// 目标绘图节点。
        drawing: NodeId,
        /// 替换或插入的原始二进制字节。
        bytes: Vec<u8>,
        /// 媒体内容类型。
        mime: String,
    },

    // ---- 墨迹（`SAVE-07 inks`，任务 6.8）--------------------------------------------------------
    /// 删掉主 part 里全部 `aidocs-ink` 墨迹 run（`Document.inks`）；它们的媒体与关系随保存时的资源回收消失。
    RemoveInks,
    /// 在段落 `para` 的全部内容之后追加一条墨迹 run（TS `anchoredInkRunXml`；`docPr/@id` 按 `EDIT-06`，
    /// 媒体每条一个 part）。`para` 不是 `w:p`（表格 / sdt 外壳）→ 跳过 + 诊断，不分配媒体与关系。
    InsertInk {
        /// 目标段落节点 ID。
        para: NodeId,
        /// 新墨迹数据。
        ink: NewInk,
    },

    // ---- 节与页眉页脚（`EDIT-03`，任务 5.5）--------------------------------------------------
    /// `EDIT-03 SetSectionProps`：给定 `w:sectPr` 按 `PROP-06` 合并（未建模的子元素原字节不动，
    /// 新元素按 CT_SectPr 顺序插入）。**新建分节符**（给段落加一个 `sectPr`）不在 M5。
    SetSectionProps {
        /// 目标分节属性节点。
        sect: NodeId,
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: SectionPropsPatch,
    },
    /// `EDIT-03 SetHeaderFooter`：这一节这个变体的页眉页脚内容整体替换。
    ///
    /// 该节**自己声明**了这个变体（`RES-10` 的 `Declared`）→ 改写它引用的 part；没声明（含从上一节
    /// 继承）→ 按 `SAVE-05` 新建 `word/header{N}.xml` 并把引用插进这一节的 `sectPr`，这一节因此
    /// 独立、前面的节不受影响（Word 与 TS 的 `sectionHf` 语义）。
    SetHeaderFooter {
        /// 目标分节属性节点。
        sect: NodeId,
        /// 操作对象的种类。
        kind: HfKind,
        /// 页眉页脚槽位：默认、首页或偶数页。
        variant: HfVariant,
        /// 新内容，形状见载荷类型。
        content: Vec<NewBlock>,
    },
    /// 给一个没有该变体引用的节挂上**已有** part 的引用（TS 的 `hfAllSections`）。
    LinkHeaderFooter {
        /// 目标分节属性节点。
        sect: NodeId,
        /// 操作对象的种类。
        kind: HfKind,
        /// 页眉页脚槽位：默认、首页或偶数页。
        variant: HfVariant,
        /// 目标 part；可选值缺席时表示主 part。
        part: PartId,
    },
    /// `SAVE-07 watermark`：这一节 default 页眉里的文字水印。`None` 删掉（连同页眉里所有
    /// 含 `v:textpath` 的段落）；页眉不存在时先建。Strict 包拒绝（VML 不在 Strict 里）。
    SetWatermark {
        /// 目标分节属性节点。
        sect: NodeId,
        /// 新文本内容。
        text: Option<String>,
    },
    /// `SAVE-07 pageColor`：`w:background/@w:color`（`w:document` 的第一个子元素）。`None` 删掉。
    SetPageColor {
        /// 颜色值或清除请求。
        color: Option<String>,
    },
    /// `EDIT-03 SetDocumentSettings`：`word/settings.xml` 按 `PROP-06` 合并（part 不存在就建）。
    SetDocumentSettings {
        /// 已建模属性补丁；未涉及字段保持原值。
        patch: SettingsPatch,
    },
    /// `EDIT-03 InsertAtom`：往坐标流里插一个原子（`spec/18` 7.5）。
    InsertAtom {
        /// 操作落点；坐标单位和边界见本变体说明。
        at: InlinePos,
        /// 新原子内容。
        atom: NewAtom,
    },
    /// `EDIT-03 SetNoteContent`：整条脚注 / 尾注的正文段落换掉（自引用标记 run 保留）。
    SetNoteContent {
        /// 为真时操作尾注，否则为脚注。
        endnote: bool,
        /// 目标对象在文档中的 ID。
        id: String,
        /// 新内容，形状见载荷类型。
        content: Vec<Vec<NewRun>>,
    },
    /// `EDIT-03 RemoveNote`：删条目 + 正文里的引用 run。
    RemoveNote {
        /// 为真时操作尾注，否则为脚注。
        endnote: bool,
        /// 目标对象在文档中的 ID。
        id: String,
    },
    /// `EDIT-03 SetSdtContent`：内联内容控件里的内容整体换掉。
    SetSdtContent {
        /// 目标内容控件节点。
        sdt: NodeId,
        /// 替换后的行内内容序列。
        inlines: Vec<NewInline>,
    },
    /// `EDIT-03 RemoveSdtShell`：Word 的「删除内容控件」——内容留下，`w:sdt` 消失。
    RemoveSdtShell {
        /// 目标内容控件节点。
        sdt: NodeId,
    },
    /// `EDIT-03 SetMathTokens`（TS `patchMathTokens`）：按序替换 `m:oMath` 里每个 `m:t` 的文字；
    /// 个数不等 → `EDIT_MATH_TOKEN_COUNT`。
    SetMathTokens {
        /// 目标公式节点。
        math: NodeId,
        /// 按公式中 m:t 顺序给出的替换文本。
        tokens: Vec<String>,
    },
    /// `EDIT-03 SetDrawingGeometry`（`spec/18` 7.7）：尺寸 / 位置 / 旋转 / 翻转 / 裁剪。
    /// 只改属性，`a:graphic` 子树原字节。
    SetDrawingGeometry {
        /// 目标绘图节点。
        drawing: NodeId,
        /// 绘图几何属性补丁。
        geom: DrawingGeometry,
    },
    /// `EDIT-03 SetDrawingZOrder`：`relativeHeight = 251658240 + z`（只对锚定图片有意义）。
    SetDrawingZOrder {
        /// 目标绘图节点。
        drawing: NodeId,
        /// 绘图层叠顺序。
        z: i64,
    },
    /// `EDIT-03 SetDrawingWrap`（`spec/18` 7.7）：绕排方式。`wrap: None` = 随文（`wp:inline`），
    /// `Some(_)` = 锚定（`wp:anchor`）。壳换了也只重建壳：`wp:extent` / `effectExtent` /
    /// `docPr` / `cNvGraphicFramePr` / `a:graphic` 原字节搬过去（`SAVE-08`）。
    SetDrawingWrap {
        /// 目标绘图节点。
        drawing: NodeId,
        /// 文字环绕方式。
        wrap: Option<ImageWrap>,
        /// 不给时：本来就锚定的保留原 `positionH` / `positionV`，随文转锚定的按绕排方向取缺省。
        /// 位置属性变更。
        pos: Option<AnchorPos>,
        /// 不给时：本来就锚定的保留原 `relativeHeight`，随文转锚定的取基数。
        /// 绘图层叠顺序变更。
        z_order: Option<i64>,
    },
    /// `EDIT-03 SetShapeStyle`：`wps:spPr` 的填充与描边。`None` = 不动，`Some(None)` = 无。
    SetShapeStyle {
        /// 目标形状节点。
        shape: NodeId,
        /// 填充颜色变更。
        fill: Option<Option<String>>,
        /// 轮廓属性变更。
        outline: Option<Option<String>>,
    },
    /// `EDIT-03 RegenerateBlockField`（`spec/18` 7.8）：按生成器重算一个块字段的结果区
    /// （`TOC` / `INDEX`）。走 `UpdateBlockField` 那条机制，`w:fldLock` 一样拒绝。
    RegenerateBlockField {
        /// 目标字段 ID。
        field: crate::span::FieldId,
        /// 生成器选项。
        options: BlockFieldOptions,
    },
    /// `EDIT-03 SetTextboxContent`（`spec/18` 7.7）：一个文本框里的块整体换掉。`textbox` 可以是
    /// `w:txbxContent` 自己，也可以是包着它的 `wps:wsp` / `wps:txbx` / `v:shape` / `v:textbox`。
    /// 落在 `mc:Choice` 里时 `mc:Fallback` 的 VML 孪生跟着同步。
    SetTextboxContent {
        /// 目标文本框节点。
        textbox: NodeId,
        /// 按文档顺序排列的新块内容。
        blocks: Vec<NewBlock>,
    },
    /// `EDIT-03 InsertSectionBreak`（`spec/18` 7.6）：在 `after` 这一段之后断节。
    /// 该段的 `pPr` 里新建一个 `w:sectPr`（原节属性的克隆，含页眉页脚引用），
    /// 原来的 `sectPr` 从此描述**后**一节，它的 `w:type` 换成 `kind`。
    InsertSectionBreak {
        /// 在此段落之后插入分节符。
        after: NodeId,
        /// 操作对象的种类。
        kind: crate::semantic::props::SectType,
    },
    /// `EDIT-03 DeleteSectionBreak`：删掉一个**段落级** `w:sectPr`，这些块并入后一节
    /// （Word 语义：合并后由后一节的页面设置接管）。body 级的 `sectPr` 不能删。
    DeleteSectionBreak {
        /// 目标分节属性节点。
        sect: NodeId,
    },
    /// `EDIT-03 AcceptRevision`：接受一条修订（`Document.revisions` 里的 id）。
    AcceptRevision {
        /// 目标修订 ID。
        rev: crate::model::RevisionId,
    },
    /// `EDIT-03 RejectRevision`：拒绝一条修订。
    RejectRevision {
        /// 目标修订 ID。
        rev: crate::model::RevisionId,
    },
    /// `EDIT-03 AcceptAll`：接受全部修订；`author` 给定时只接受那个作者的
    /// （编辑器按作者接受，`docs/03` §8.2 之外的扩展，登记在 `docs/04` §8）。
    AcceptAll {
        /// 修订作者过滤条件。
        author: Option<String>,
    },
    /// `EDIT-03 RejectAll`：拒绝全部修订；`author` 同上。
    RejectAll {
        /// 修订作者过滤条件。
        author: Option<String>,
    },
    /// `BIND-03 v3`：参考文献权威列表，未变条目保留原字节。
    SetSources {
        /// 参考文献来源的权威列表。
        sources: Vec<crate::save::options::decl::SourceSave>,
    },
    /// `BIND-03 v3`：按 numId 追加，已存在即 no-op。
    AddNumberingDefinition {
        /// 调用方指定 numId 的编号定义。
        definition: crate::save::options::decl::NumberingDefSave,
    },
    /// `BIND-03 v3`：按 numId 追加重启定义，已存在即 no-op。
    RestartNumbering {
        /// 调用方指定 numId 的编号重启声明。
        restart: crate::save::options::decl::RestartNumSave,
    },
    /// `BIND-03 v3`：替换主题字体槽。
    SetThemeFonts {
        /// 主题字体槽位。
        fonts: crate::save::options::decl::ThemeFontsSave,
    },
    /// `BIND-03 v3`：替换主题配色槽。
    SetThemeColors {
        /// 主题配色槽位的权威列表。
        colors: crate::save::options::decl::ThemeColorsSave,
    },
    /// `BIND-03 v3`：按 styleId upsert，相同请求不改状态。
    UpsertStyle {
        /// 以 styleId 为键的样式声明。
        style: crate::save::options::decl::StyleUpsertSave,
    },
}
/// `SetLinkTarget` 要改哪个链接。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LinkRef {
    /// HYPERLINK 字段。
    Field(FieldId),
    /// `w:hyperlink` 元素。
    Element(NodeId),
}
/// 链接目标。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LinkDest {
    /// 外部 URL。`w:hyperlink` 会先按 `EDIT-06` 分配一条外部关系。
    Url(String),
    /// 文内书签（字段写 `\l "name"`，元素写 `w:anchor`）。
    Anchor(String),
    /// 已有的关系 id（只用于 `w:hyperlink`）。
    Rel(String),
}
/// `FLD-12` 的新字段。`instr` 是指令原文（不含首尾空格，生成时补上）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewField {
    pub instr: String,
    /// 结果区内容（空 = 只有 begin..end 的空结果）。
    pub result: Vec<NewInline>,
    /// begin 的 `w:fldChar` 上打 `w:dirty="true"`，Word 打开时重算。
    pub mark_dirty: bool,
}
/// UTF-16 code unit 偏移。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    ::serde::Serialize,
    ::serde::Deserialize,
)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Utf16Offset(pub u32);
/// 段内位置：`para` 是 `w:p`，`0 ≤ offset ≤ len`；`part` 为 `None` 表示主 part。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
#[repr(C)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InlinePos {
    /// 段落所在的 part；`None` = 主 part。
    pub part: Option<PartId>,
    pub para: NodeId,
    pub offset: Utf16Offset,
}
impl InlinePos {
    /// 主 part（正文）里的位置。
    #[inline]
    pub fn new(para: NodeId, offset: u32) -> Self {
        Self { part: None, para, offset: Utf16Offset(offset) }
    }

    /// 指定 part 里的位置（页眉页脚 / 注释 / 批注条目）。
    #[inline]
    pub fn in_part(part: PartId, para: NodeId, offset: u32) -> Self {
        Self { part: Some(part), para, offset: Utf16Offset(offset) }
    }

    /// 同一个 part 里的另一个偏移。
    #[inline]
    pub fn with_offset(self, offset: u32) -> Self {
        Self { offset: Utf16Offset(offset), ..self }
    }
}
/// 定位结果（`EDIT-02` 的 `Loc`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum Loc {
    /// 落在两个 inline 之间：前面有 `index` 个 inline（`0 ≤ index ≤ len`）。
    Boundary { index: usize },
    /// 落在 `inlines[inline]`（Run）内部两段之间：`segment` 是后一段的下标（≥ 1）。
    InRun { inline: usize, segment: usize },
    /// 落在 `Text` / `DelText` 段内部（不含两端），`byte` 是段文本里的 UTF-8 字节偏移。
    InText { inline: usize, segment: usize, byte: usize },
}
impl InlinePos {
    /// 每个 inline 在坐标流中的区间。
    #[inline]
    fn inline_spans(tb: &TextBlock) -> impl Iterator<Item = Range<u32>> + '_ {
        let mut cum = 0u32;
        tb.inlines.iter().map(move |i| {
            let s = cum;
            cum += i.utf16_len();
            s..cum
        })
    }
}
impl Utf16Offset {
    /// `locate(pos)`：顺序累加 inlines 的 UTF-16 长度。偏移指向代理对中间 → `EDIT_SPLIT_SURROGATE`；
    /// 越界或落在非文本原子段内部 → `EDIT_BAD_POSITION`。
    #[inline]
    pub fn locate(self, tb: &TextBlock) -> Result<Loc> {
        let target = self.0;
        let mut cum = 0u32;
        for (i, inline) in tb.inlines.iter().enumerate() {
            if target == cum {
                return Ok(Loc::Boundary { index: i });
            }
            let len = inline.utf16_len();
            if target < cum + len {
                let Inline::Run(run) = inline else {
                    return Err(Error::edit(DiagCode::EditBadPosition, "偏移落在原子内部"));
                };
                let mut scum = cum;
                for (k, seg) in run.segments.iter().enumerate() {
                    if target == scum && k > 0 {
                        return Ok(Loc::InRun { inline: i, segment: k });
                    }
                    if target < scum + seg.utf16_len {
                        if !matches!(seg.kind, SegmentKind::Text | SegmentKind::DelText) {
                            return Err(Error::edit(
                                DiagCode::EditBadPosition,
                                "偏移落在非文本段内部",
                            ));
                        }
                        let byte = usize::try_from(Utf16TextOffset {
                            text: run.segment_text(seg),
                            offset: Self(target - scum),
                        })?;
                        return Ok(Loc::InText { inline: i, segment: k, byte });
                    }
                    scum += seg.utf16_len;
                }
                unreachable!("segments cover the run's coordinate range");
            }
            cum += len;
        }
        if target == cum {
            return Ok(Loc::Boundary { index: tb.inlines.len() });
        }
        Err(Error::edit(DiagCode::EditBadPosition, format!("偏移 {target} 超出段落长度 {cum}")))
    }
}
/// 借用段文本的 UTF-16 位置；转换为 `usize` 时校验并返回 UTF-8 字节偏移。
/// 文本借用绑定到调用方的字符串；代理对内部和越界分别保留原编辑诊断。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct Utf16TextOffset<'a> {
    /// 位置所属的段文本。
    pub text: &'a str,
    /// 文本内 UTF-16 code unit 偏移。
    pub offset: Utf16Offset,
}
impl TryFrom<Utf16TextOffset<'_>> for usize {
    type Error = Error;

    #[inline]
    fn try_from(value: Utf16TextOffset<'_>) -> Result<Self> {
        let Utf16TextOffset { text, offset: Utf16Offset(units) } = value;
        let mut cum = 0u32;
        for (b, c) in text.char_indices() {
            if cum == units {
                return Ok(b);
            }
            let n = c.len_utf16() as u32;
            if cum + n > units {
                return Err(Error::edit(DiagCode::EditSplitSurrogate, "偏移落在代理对中间"));
            }
            cum += n;
        }
        if cum == units {
            return Ok(text.len());
        }
        Err(Error::edit(DiagCode::EditBadPosition, "偏移超出段文本"))
    }
}
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// 操作在正文树上的目标节点：内容控件守卫、`mc:Fallback` 守卫与孪生同步共用。
    ///
    /// 连它所在的 part 一起收集（任务 5.5）：`NodeId` 只在自己 part 的 DOM 里有意义，拿页眉的节点
    /// 去主 part 的树上走祖先会越界。这里**不写通配分支**：新增操作时编译器会提醒你决定它要不要守卫。
    #[inline]
    fn op_targets(
        &self,
        op: &EditOp,
    ) -> impl Iterator<Item = (Option<PartId>, NodeId)> + Clone + use<> {
        let pos = |p: &InlinePos| (p.part, p.para);
        let block_pos = |p: &BlockPos| (p.part, p.node());
        let field = |id: FieldId| self.document().fields.get(id).map(|f| (None, f.form.head()));
        let targets = match op {
            EditOp::InsertText { at, .. }
            | EditOp::SplitParagraph { at }
            | EditOp::InsertAtom { at, .. }
            | EditOp::InsertField { at, .. } => [Some(pos(at)), None],
            EditOp::DeleteRange { from, to }
            | EditOp::SetRunProps { from, to, .. }
            | EditOp::AddComment { from, to, .. }
            | EditOp::AddBookmark { from, to, .. } => [Some(pos(from)), Some(pos(to))],
            EditOp::ReplaceInlines { part, para, .. }
            | EditOp::SetParaProps { part, para, .. }
            | EditOp::ReplaceParaProps { part, para, .. }
            | EditOp::MergeWithNext { part, para } => [Some((*part, *para)), None],
            EditOp::SetTableProps { table: n, .. }
            | EditOp::SetRowProps { row: n, .. }
            | EditOp::SetCellProps { cell: n, .. }
            | EditOp::InsertRow { table: n, .. }
            | EditOp::DeleteRow { table: n, .. }
            | EditOp::InsertColumn { table: n, .. }
            | EditOp::DeleteColumn { table: n, .. }
            | EditOp::MergeCells { table: n, .. } => [Some((None, *n)), None],
            EditOp::InsertBlock { at, .. } => [Some(block_pos(at)), None],
            EditOp::DeleteBlock { part, node } => [Some((*part, *node)), None],
            EditOp::MoveBlock { from, node, to } => [Some((*from, *node)), Some(block_pos(to))],
            EditOp::SetLinkTarget { link, .. } => match link {
                LinkRef::Field(id) => [field(*id), None],
                LinkRef::Element(node) => [Some((None, *node)), None],
            },
            EditOp::ToggleCheckbox { field: id }
            | EditOp::SetFormText { field: id, .. }
            | EditOp::SetFieldResultProps { field: id, .. }
            | EditOp::UpdateBlockField { field: id, .. } => [field(*id), None],
            // 按 id 定位的操作（批注条目、书签名）不在正文树上。这里**不写通配分支**：
            // 新增操作时编译器会提醒你决定它要不要守卫。
            EditOp::RemoveComment { .. }
            | EditOp::SetCommentText { .. }
            | EditOp::RemoveBookmark { .. } => [None, None],
            // 图表 part 与整 part 替换：目标是别的 part，不在正文树上（任务 6.6）
            EditOp::SetSources { .. }
            | EditOp::AddNumberingDefinition { .. }
            | EditOp::RestartNumbering { .. }
            | EditOp::SetThemeFonts { .. }
            | EditOp::SetThemeColors { .. }
            | EditOp::UpsertStyle { .. }
            | EditOp::SetChartData { .. }
            | EditOp::ReplacePartXml { .. }
            | EditOp::ReplacePartBytes { .. } => [None, None],
            EditOp::ReplaceImageMedia { drawing, .. } => [Some((None, *drawing)), None],
            // 墨迹：整层删除不在内容控件里定位；追加落在锚点段落上（任务 6.8）
            EditOp::RemoveInks => [None, None],
            EditOp::InsertInk { para, .. } => [Some((None, *para)), None],
            // 节与页眉页脚：目标是 `w:sectPr` 或整个 part，不在内容控件里（任务 5.5）
            EditOp::SetSectionProps { .. }
            | EditOp::SetHeaderFooter { .. }
            | EditOp::LinkHeaderFooter { .. }
            | EditOp::SetWatermark { .. }
            | EditOp::SetPageColor { .. }
            | EditOp::SetDocumentSettings { .. } => [None, None],
            // 接受 / 拒绝修订按 `RevisionId` 定位；内容控件的锁不该阻止它（改的是修订标记，
            // 不是用户在控件里的输入），逐条落地时该拒的由 `plan_job` 自己拒
            EditOp::AcceptRevision { .. }
            | EditOp::RejectRevision { .. }
            | EditOp::AcceptAll { .. }
            | EditOp::RejectAll { .. } => [None, None],
            // 注释条目按 id 定位、在别的 part 里；内容控件与公式自己做守卫（`sdt_ops::guard`）
            EditOp::SetNoteContent { .. }
            | EditOp::RemoveNote { .. }
            | EditOp::SetSdtContent { .. }
            | EditOp::RemoveSdtShell { .. }
            | EditOp::SetMathTokens { .. } => [None, None],
            // 分节符：目标是段落 / `w:sectPr`，内容控件的锁不该拦它
            EditOp::InsertSectionBreak { .. } | EditOp::DeleteSectionBreak { .. } => [None, None],
            EditOp::SetDrawingGeometry { drawing: n, .. }
            | EditOp::SetDrawingZOrder { drawing: n, .. }
            | EditOp::SetDrawingWrap { drawing: n, .. } => [Some((None, *n)), None],
            EditOp::SetShapeStyle { shape, .. } => [Some((None, *shape)), None],
            EditOp::SetTextboxContent { textbox, .. } => [Some((None, *textbox)), None],
            // 块字段按 `FieldId` 定位，重算走 `UpdateBlockField` 那条路，它自己做守卫
            EditOp::RegenerateBlockField { .. } => [None, None],
        };
        targets.into_iter().flatten()
    }
}

// 原 atom_ops.rs
// `EDIT-03 InsertAtom`：往坐标流里插一个**原子**（`spec/18` 7.5）。
//
// 原子在坐标流里恒占 **1 个 UTF-16 单位**（`EDIT-02`），所以插入前后的偏移差正好是 1。
// 五种原子共用 [`super::ops`] 的边界定位（拆 run → `boundary_site`）与追踪时的
// `w:ins` 包裹（`plan_ins_site`），差别只在 run 里放什么。

// 原 chart_ops.rs
// 图表的写侧（`EDIT-03` / `SAVE-05` / `SAVE-06`，`spec/17` 任务 6.6）：改缓存文本（`set_chart_data`）、
// 新建图表（`materialize`：图表 part + 内嵌工作簿 + 关系 + 绘图段落）。
//
// `SetChartData` 只改缓存文本节点（TS `patchChartPartXml` 的语义）：数据引用 `c:f`、样式、布局一个字节不动，
// 所以保存时只有被改的文本节点脏；锚不到的地方（没有标题、缓存里缺的点）留着不补。新图表的 part 内容按
// TS `buildChartPartXml` / `buildChartWorkbookXlsxBase64` 的模板生成，再解析成 part 的 DOM——之后的编辑与
// 别的 part 一样走 `MutationPlan`。
edit_enum! {
    /// 新图表的种类（TS `NewChart.kind`）：bar / line 带一对轴，pie 没有轴。
    pub enum NewChartKind {
        Bar = "bar",
        Line = "line",
        Pie = "pie",
    }
}
/// 一个新图表的数据（TS `NewChart`）。
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewChart {
    pub kind: NewChartKind,
    pub title: Option<String>,
    pub categories: Vec<String>,
    pub series: Vec<NewChartSeries>,
}
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewChartSeries {
    pub name: String,
    /// 按类别位置；`None` = 空档（缓存里不写这个点）。
    pub values: Vec<Option<f64>>,
}
/// `SetChartData` 的补丁（TS `ChartPatch`）：每一项 `None` = 不动；数组里的 `None` = 那一个不动。
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartPatch {
    pub title: Option<String>,
    /// 与 `ChartDisplay.categories` 对齐。
    pub categories: Option<Vec<Option<String>>>,
    /// 与 `ChartDisplay.series` 对齐。
    pub series: Option<Vec<Option<ChartSeriesPatch>>>,
}
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartSeriesPatch {
    pub name: Option<String>,
    /// 与 `ChartSeries.values` 对齐；`None` = 保留原值。
    pub values: Option<Vec<Option<f64>>>,
}
const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// TS 缺省的图表尺寸：5486400 × 3200400 EMU（576 × 336 px）。
pub const DEFAULT_EXTENT_EMU: (i64, i64) = (5_486_400, 3_200_400);

// ---- 新建 -----------------------------------------------------------------------------------------

// ---- 改数据 ---------------------------------------------------------------------------------------

// 原 diff.rs
// 坐标流 diff（`spec/18` 7.2 的 `ReplaceInlines`）。
//
// Myers 的 O(ND) 版本，先剥掉公共前后缀。**token 就是一个 run**（`spec/18` 7.2：「坐标流 diff …
// 聚到 run 边界」）——一个 run 的文本与它的 `w:rPr` 全同才算相等，改一个字或改一处格式，
// 整个 run 就是删除 + 插入。这正是「接受后 = 不追踪做一遍」要求的：相等段保留原节点时，
// 它的格式必须真的没变。
//
// 新旧两侧的 `w:rPr` 都化成 [`NewElement`] 再比——比较**保守**（属性顺序不同会判成不等），
// 而保守只会让 diff 变粗，不会把不相等的东西判成相等。
/// 坐标流里的一个 token。

/// diff 脚本的一段。

/// token 数超过这个数就不做 Myers（退化成整体替换）——一段里的 run 数正常是几十个。

/// 旧 → 新的编辑脚本。返回 `None` 表示放弃（太大），调用方整体替换。

/// 相邻的同类合并。

/// Myers O(ND)：记录每一轮的 `v`，走完再回溯出脚本。`d` 超过两侧长度之和就放弃。

/// 从 `trace` 反推脚本（Myers 的标准回溯，倒着生成再反转）。

// 原 drawing_ops.rs
// 既有绘图的编辑（`EDIT-03`，`spec/18` 7.7）：尺寸 / 旋转 / 翻转 / 裁剪、z-order、形状样式。
//
// **只改属性**（分层决策 9）：`wp:extent` / `a:ext` / `wp:posOffset` / `relativeHeight` /
// `a:xfrm` 的属性改动让那个元素 `SelfDirty`，`a:graphic` 子树永远原字节。
//
// 形态对照件是 `fixtures/word-ops/{z-order,move-resize}`：Word 自己做同一件事的前后两份。
/// TS `applyImageZOrder` 的基数：`relativeHeight = Z_BASE + z`。
pub const Z_BASE: i64 = Z_ORDER_BASE;

/// 一次几何改动（`None` = 不动这一项）。
#[derive(Debug, Clone, Default, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrawingGeometry {
    /// 显示尺寸（EMU）。
    pub extent_emu: Option<(i64, i64)>,
    /// 锚定位置（EMU）：`(positionH, positionV)` 的 `wp:posOffset`。随文图片没有位置，给了也不动。
    pub pos_offset_emu: Option<(i64, i64)>,
    /// 旋转角（度）；`Some(None)` = 去掉旋转。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::semantic::props::serde::double_option"
    )]
    pub rot_deg: Option<Option<i64>>,
    pub flip_h: Option<bool>,
    pub flip_v: Option<bool>,
    /// 裁剪窗（`a:srcRect` 的四个千分比）；`Some(None)` = 去掉裁剪。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::semantic::props::serde::double_option"
    )]
    pub crop: Option<Option<SrcRect>>,
}
/// `a:srcRect`：四边各裁掉的千分比（0..100000）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SrcRect {
    pub l: i64,
    pub t: i64,
    pub r: i64,
    pub b: i64,
}

/// 一根轴的定位（`wp:positionH` / `wp:positionV`）。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorAxis {
    /// `@relativeFrom`：`column` / `page` / `margin` / `paragraph` / `character` / `line` …
    pub relative_from: String,
    pub pos: AxisPos,
}
/// 轴上的位置：偏移或对齐（`wp:posOffset` / `wp:align`，两者互斥）。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AxisPos {
    /// `wp:posOffset`（EMU）。
    Offset(i64),
    /// `wp:align`：`left` / `center` / `right` / `top` / `bottom` / `inside` / `outside`。
    Align(String),
}
/// 锚定图片的两根轴。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnchorPos {
    pub h: AnchorAxis,
    pub v: AnchorAxis,
}
/// 换壳时**搬进新壳**的子元素，按 `CT_Inline` / `CT_Anchor` 的次序。壳里别的东西
/// （`simplePos` / `positionH` / `positionV` / `wrap*` / `wp14:sizeRel*`）只属于旧壳，跟着它一起走。
const CARRIED: [(NsId, LocalName); 5] = [
    (NsId::Wp, LocalName::Extent),
    (NsId::Wp, LocalName::EffectExtent),
    (NsId::Wp, LocalName::DocPr),
    (NsId::Wp, LocalName::CNvGraphicFramePr),
    (NsId::A, LocalName::Graphic),
];

// 原 field_ops.rs
// 块字段的收集与重算（`FLD-09`，`spec/18` 7.8）。
//
// 生成器（`span::field::generate`）只把算好的条目摊成 XML；从文档里**收**条目在这里——
// 走标题、收 `XE` 词、数 `SEQ`，都要 `EditSession`。
//
// `RegenerateBlockField` = 收条目 → 生成 → 走 `UpdateBlockField` 那条既有机制换结果区，
// 所以 `w:fldLock`、追踪、跨段字段的规则一条都不用重写。
/// `RegenerateBlockField` 的重算方式。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BlockFieldOptions {
    /// 按字段自己的指令开关重算（`TOC` / `INDEX` 各自认得的那些）。
    Auto {
        /// TOC：标题段落 → 页码（调用方的分页结果）。`None` = 不写页码。
        pages: Option<HashMap<NodeId, u32>>,
    },
    Toc {
        opts: Box<TocOptions>,
        pages: Option<HashMap<NodeId, u32>>,
    },
    Index(Box<IndexOptions>),
}
impl Default for BlockFieldOptions {
    fn default() -> Self {
        BlockFieldOptions::Auto { pages: None }
    }
}
/// 新插入的块字段（`NewBlock::Field`）。条目由当前文档算。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewBlockField {
    Toc { opts: Box<TocOptions>, pages: Option<HashMap<NodeId, u32>> },
    Index(Box<IndexOptions>),
}

enum Plan {
    Toc(Box<TocOptions>, Option<HashMap<NodeId, u32>>),
    Index(Box<IndexOptions>),
}

// 原 ink_ops.rs
// 墨迹的写侧（`SAVE-07 inks`、`EDIT-03` / `EDIT-06`，`spec/17` 任务 6.8）。
//
// `inks` 是**权威列表**（与 `comments` 同语义）：`EditSession::remove_inks` 删掉主 part 里全部墨迹 run
// （它们的媒体与关系随保存时的资源回收消失，6.7），再对每条 [`InkSave`] 调 `EditSession::insert_ink`
// ——按 TS `anchoredInkRunXml` 的模板把一条浮动图片 run 追加在段落**全部内容之后**。墨迹的媒体**不去重**
// （每条一个 part，TS 同；两笔画出同一张 PNG 几乎不可能，去重只会让 `r:embed` 与 TS 分叉）。
pub const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
/// 一条要写进文档的墨迹（TS `NewInkImage` 去掉 `blockIndex`）。
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewInk {
    /// PNG 字节。
    pub png: Vec<u8>,
    pub width_px: f64,
    pub height_px: f64,
    /// 相对文字栏左边 / 段落顶部的偏移（px，可为负）。
    pub offset_x_px: f64,
    pub offset_y_px: f64,
    /// 编辑器的笔迹载荷，写进 `wp:docPr/@descr`；`None` 或空 → 不写。
    pub payload: Option<String>,
}
/// `SaveOptions.inks` 的一条：锚点段落 + 墨迹。
#[derive(Debug, Clone, PartialEq)]
pub struct InkSave {
    /// 锚点（`w:p`）。不是段落（表格 / sdt 外壳）→ 跳过 + 诊断，不分配媒体与关系（TS 同）。
    pub para: NodeId,
    pub ink: NewInk,
}
fn attr_escaped(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    crate::xml::entities::escape_attr(s, b'"', &mut out);
    String::from_utf8(out).expect("escape_attr 只输出 UTF-8")
}
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// `EditOp::RemoveInks`：删掉主 part 里全部墨迹 run（`Document.inks`）。
    pub fn remove_inks(&mut self) -> Result<MutationResult> {
        let inks: Vec<(NodeId, NodeId)> =
            self.document().inks.iter().map(|i| (i.para, i.run)).collect();
        if inks.is_empty() {
            return Ok(MutationResult::default());
        }
        let mut plan = MutationPlan::new(self.main_part());
        for (para, run) in inks {
            plan.node_edits.push(NodeEdit::Delete(run));
            plan.touch(para);
        }
        self.commit_plan(plan)
    }

    /// `EditOp::InsertInk`：在 `para` 的全部内容之后追加一条墨迹 run（TS `anchoredInkRunXml`）。
    pub fn insert_ink(&mut self, para: NodeId, ink: &NewInk) -> Result<MutationResult> {
        let main = self.main_part();
        let dom = self.dom();
        if (para.0 as usize) >= dom.node_count() {
            return Err(Error::edit(DiagCode::EditBadPosition, "墨迹的锚点节点不存在"));
        }
        if !dom.is(para, QName::w(LocalName::P)) {
            // TS：`^<w:p[\s/>]` 不匹配（表格 / sdt 外壳）就跳过；先判再分配，不留孤儿媒体与关系
            let diag = Diagnostic::pre_existing(
                main,
                dom.node(para).lex.as_ref().map(|l| l.range.clone()),
                DiagCode::EditBadPosition,
                "墨迹的锚点不是段落，这条墨迹已跳过",
            );
            self.push_diagnostic(diag);
            return Ok(MutationResult::default());
        }
        let rid = self.add_media_with(ink.png.clone(), "image/png", false)?;
        let flavor = self.flavor();
        let dom = self.package_mut().dom_mut(main)?.expect("main part parsed");
        let id = NewImage::media_ops_next_doc_pr_id(dom);
        let ctx = NamespaceContext::from_dom(dom, flavor);
        let (wp, wp_decl) = NewImage::media_ops_prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
        let (r, r_decl) = NewImage::media_ops_prefix_or_decl(&ctx, NsId::R, "r", NS_R);
        let (cx, cy) = (px_to_emu(ink.width_px), px_to_emu(ink.height_px));
        let x = (ink.offset_x_px * EMU_PER_PX).round() as i64;
        let y = (ink.offset_y_px * EMU_PER_PX).round() as i64;
        let name = format!("{INK_NAME_PREFIX} {id}");
        let descr = ink
            .payload
            .as_deref()
            .filter(|p| !p.is_empty())
            .map_or(String::new(), |p| format!(r#" descr="{}""#, attr_escaped(p)));
        let xml = format!(
            concat!(
                r#"<w:r><w:drawing{wp_decl}{r_decl}><{wp}:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" "#,
                r#"relativeHeight="{rh}" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">"#,
                r#"<{wp}:simplePos x="0" y="0"/>"#,
                r#"<{wp}:positionH relativeFrom="column"><{wp}:posOffset>{x}</{wp}:posOffset></{wp}:positionH>"#,
                r#"<{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>{y}</{wp}:posOffset></{wp}:positionV>"#,
                r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="0" t="0" r="0" b="0"/><{wp}:wrapNone/>"#,
                r#"<{wp}:docPr id="{id}" name="{name}"{descr}/><{wp}:cNvGraphicFramePr/>"#,
                r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}"><pic:pic xmlns:pic="{pic}">"#,
                r#"<pic:nvPicPr><pic:cNvPr id="{id}" name="{name}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip {r}:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic></{wp}:anchor></w:drawing></w:r>"#
            ),
            wp_decl = wp_decl,
            r_decl = r_decl,
            wp = wp,
            rh = Z_ORDER_BASE + id,
            x = x,
            y = y,
            cx = cx,
            cy = cy,
            id = id,
            name = name,
            descr = descr,
            a = NS_A,
            pic = NS_PIC,
            r = r,
            rid = rid,
        );
        let run = parse_fragment(dom, &xml)
            .map_err(|e| {
                Error::edit(DiagCode::EditPlanInvalid, format!("墨迹 run 模板不良构: {e}"))
            })?
            .pop()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "墨迹 run 模板为空"))?;
        let mut plan = MutationPlan::new(main);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(para),
            before: None,
            node: run,
        });
        plan.touch(para);
        self.commit_plan(plan)
    }
}

// 原 inline.rs
// 新内容描述（`docs/03` §8.2 的 `NewInline`）与其 `New` 子树生成。
//
// 文本里的控制字符折回段种类（`COMPAT-08`）：`\t` → `w:tab`，`\n` → `w:br`，`\u{0C}` → `w:br w:type="page"`，
// `\u{0B}` → `w:br w:type="column"`，`\r` → `w:cr`；其余进入 `w:t`（`New` 节点序列化时一律带
// `xml:space="preserve"`，`SAVE-03`）。
/// 修订元数据；`id == None` 时按 `EDIT-06` 分配（文档内全部修订 `w:id` 的最大值 + 1）。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRevision {
    pub id: Option<String>,
    pub author: String,
    pub date: Option<String>,
}
/// 超链接目标：已有关系 `r:id`，或文内书签 `w:anchor`。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewLinkTarget {
    Rel(String),
    Anchor(String),
}
/// 一个新 run：`props` 是完整的 `w:rPr`（`None` = 无 `rPr`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub text: String,
    pub props: Option<NewElement>,
}
impl NewRun {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), props: None }
    }
}
/// 范围标记与批注引用（compat 侧重发，`SPAN` 索引在 M2 接管）。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NewMarker {
    BookmarkStart {
        id: String,
        name: String,
    },
    BookmarkEnd {
        id: String,
    },
    CommentRangeStart {
        id: String,
    },
    CommentRangeEnd {
        id: String,
    },
    /// `w:r/w:commentReference`。
    CommentReference {
        id: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewInline {
    Run(NewRun),
    Hyperlink {
        target: NewLinkTarget,
        tooltip: Option<String>,
        inlines: Vec<NewInline>,
    },
    /// `w:ins` 包裹。
    Ins {
        rev: NewRevision,
        inlines: Vec<NewInline>,
    },
    /// `w:del` 包裹：内部 run 的文本写成 `w:delText`。
    Del {
        rev: NewRevision,
        inlines: Vec<NewInline>,
    },
    Marker(NewMarker),
    /// 复杂字段（`FLD-12`）：begin / `w:instrText` / `[separate]` / 结果 / end 五组 run。
    ///
    /// `instr` 是指令原文（生成时 trim 后前后各补一个空格，与 Word 一致）；`separate == false`
    /// 时不发 separate 也不发结果（XE / TA 一类 `Marker` 策略字段就是这个形状）。
    Field {
        instr: String,
        result: Vec<NewInline>,
        separate: bool,
        /// begin 的 `w:fldChar` 上打 `w:dirty="true"`。
        dirty: bool,
        /// 结构 run 的 `w:rPr`（compat 路径不带，`InsertField` 带插入点的继承格式）。
        props: Option<NewElement>,
    },
    /// 任意内联片段（`m:oMath`、带 `w:ruby` / `w:drawing` 的 `w:r` …）。
    Xml(NewElement),
}
impl NewInline {
    /// 复杂字段的便捷构造（`separate` 与结果都有）。
    pub fn field(instr: impl Into<String>, result: Vec<NewInline>) -> Self {
        NewInline::Field { instr: instr.into(), result, separate: true, dirty: false, props: None }
    }

    /// 没有结果区的字段（XE / TA 一类）。
    pub fn marker_field(instr: impl Into<String>) -> Self {
        NewInline::Field {
            instr: instr.into(),
            result: Vec::new(),
            separate: false,
            dirty: false,
            props: None,
        }
    }
}

/// 生成器：负责修订 `w:id` 的连续分配（`EDIT-06`：起点由调用方按文档最大值 + 1 给出）。
pub struct Emitter {
    pub next_revision_id: u32,
}
impl Emitter {
    pub fn new(next_revision_id: u32) -> Self {
        Self { next_revision_id }
    }

    fn revision_attrs(&mut self, e: &mut NewElement, rev: &NewRevision) {
        let id = match &rev.id {
            Some(id) => id.clone(),
            None => {
                let id = self.next_revision_id;
                self.next_revision_id += 1;
                id.to_string()
            }
        };
        e.push_attr(QName::w(LocalName::Id), id);
        e.push_attr(QName::w(LocalName::Author), rev.author.clone());
        if let Some(d) = &rev.date {
            e.push_attr(QName::w(LocalName::Date), d.clone());
        }
    }

    /// 一个 `NewInline` → 顶层 `New` 元素序列（`deleted`：位于 `w:del` 内，文本写 `w:delText`）。
    pub fn emit(&mut self, inline: &NewInline, deleted: bool, out: &mut Vec<NewElement>) {
        match inline {
            NewInline::Run(r) => out.push(Emitter::new_run(&r.text, r.props.clone(), deleted)),
            NewInline::Hyperlink { target, tooltip, inlines } => {
                let mut h = NewElement::new(QName::w(LocalName::Hyperlink));
                match target {
                    NewLinkTarget::Rel(rid) => {
                        h.push_attr(QName::new(NsId::R, LocalName::Id), rid.clone())
                    }
                    NewLinkTarget::Anchor(a) => h.push_attr(QName::w(LocalName::Anchor), a.clone()),
                }
                if let Some(t) = tooltip {
                    h.push_attr(QName::w(LocalName::Tooltip), t.clone());
                }
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, deleted, &mut kids);
                }
                for k in kids {
                    h.push_child(k);
                }
                out.push(h);
            }
            NewInline::Ins { rev, inlines } => {
                let mut e = NewElement::new(QName::w(LocalName::Ins));
                self.revision_attrs(&mut e, rev);
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, deleted, &mut kids);
                }
                for k in kids {
                    e.push_child(k);
                }
                out.push(e);
            }
            NewInline::Del { rev, inlines } => {
                let mut e = NewElement::new(QName::w(LocalName::Del));
                self.revision_attrs(&mut e, rev);
                let mut kids = Vec::new();
                for i in inlines {
                    self.emit(i, true, &mut kids);
                }
                for k in kids {
                    e.push_child(k);
                }
                out.push(e);
            }
            NewInline::Field { instr, result, separate, dirty, props } => {
                let fld = |kind: &str, dirty: bool| {
                    let mut e = NewElement::new(QName::w(LocalName::FldChar))
                        .with_attr(QName::w(LocalName::FldCharType), kind);
                    if dirty {
                        e.push_attr(QName::w(LocalName::Dirty), "true");
                    }
                    e
                };
                let structural = |child: NewElement| {
                    let mut r = NewElement::new(QName::w(LocalName::R));
                    if let Some(p) = props {
                        r.push_child(p.clone());
                    }
                    r.push_child(child);
                    r
                };
                out.push(structural(fld("begin", *dirty)));
                out.push(structural(
                    NewElement::new(QName::w(LocalName::InstrText))
                        .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
                        .with_text(format!(" {} ", instr.trim())),
                ));
                if *separate {
                    out.push(structural(fld("separate", false)));
                    for i in result {
                        self.emit(i, deleted, out);
                    }
                }
                out.push(structural(fld("end", false)));
            }
            NewInline::Marker(m) => out.push(Emitter::marker(m)),
            NewInline::Xml(e) => out.push(e.clone()),
        }
    }
}

// 原 media_ops.rs
// 媒体写侧（`PKG-05` / `EDIT-04` / `EDIT-06` / `SAVE-05`，`spec/17` 任务 6.7）：新图片段落、`ReplaceImageMedia`、
// 按内容去重的媒体 part。资源回收在 `save/prune.rs`。
//
// 新图片的段落按 TS `embedImage` / `applyImageWrap` 的模板生成（`wp:inline`，或带 `wrap` 时 `wp:anchor`），
// 再解析成 `New` 子树——与 `NewBlock::Chart` 同一条路（`chart_ops::materialize`）。`pPr` 的 `w:spacing` / `w:jc`
// 直接写进模板：段落是整棵新建的，`plan_apply_para_props` 的「合并到已有 pPr」在这里没有对象。
edit_enum! {
    /// TS 的九种 `ImageWrap`。
    pub enum ImageWrap {
        SquareLeft = "square-left",
        SquareRight = "square-right",
        TightLeft = "tight-left",
        TightRight = "tight-right",
        ThroughLeft = "through-left",
        ThroughRight = "through-right",
        TopBottom = "topBottom",
        Front = "front",
        Behind = "behind",
    }
}

/// 图片所在段落的 `w:spacing`（TS `paraSpacing`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParaSpacing {
    pub before_twips: Option<i64>,
    pub after_twips: Option<i64>,
    pub line_twips: Option<i64>,
    /// `exact` / `atLeast`。
    pub line_rule: Option<String>,
}
/// 一张新图片（TS `NewImage`）。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewImage {
    pub bytes: Vec<u8>,
    /// `image/png` / `image/jpeg` / `image/gif` …
    pub mime: String,
    /// 显示尺寸（EMU）。
    pub extent_emu: (i64, i64),
    /// `w:jc`：`center` / `right`（`left` 与缺省不写）。
    pub align: Option<String>,
    /// 缺省随文；有 → `wp:anchor`。
    pub wrap: Option<ImageWrap>,
    /// 锚定位置（EMU）：`positionH` 相对栏、`positionV` 相对段落；`page` 为真时两者都相对页面。
    pub pos_offset_emu: Option<PosOffset>,
    /// `relativeHeight = 251658240 + z_order`。
    pub z_order: Option<i64>,
    pub rot_deg: Option<i64>,
    pub flip_h: bool,
    pub flip_v: bool,
    pub para_spacing: Option<ParaSpacing>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PosOffset {
    pub x: i64,
    pub y: i64,
    pub page: bool,
}
/// Word 的 `relativeHeight` 基数。
pub const Z_ORDER_BASE: i64 = 251_658_240;

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// 把图片字节落成主 part 的媒体 part + `image` 关系，返回 `rId`。**相同字节只建一个 part**（同一会话内按
    /// `(mime, 哈希)` 去重，TS 同）；part 名 `word/media/image{N}.{ext}`（第一个空闲 N），`[Content_Types]` 缺该
    /// 扩展名的 `Default` 就补。
    #[doc(hidden)]
    pub fn add_media(&mut self, bytes: Vec<u8>, mime: &str) -> Result<String> {
        self.add_media_with(bytes, mime, true)
    }

    /// 同 [`Self::add_media`]；`dedup = false` 时总是新建一个 part（墨迹：每条一个 part，TS 同，任务 6.8）。
    pub fn add_media_with(&mut self, bytes: Vec<u8>, mime: &str, dedup: bool) -> Result<String> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        let key = (mime.to_string(), h.finish());
        if dedup && let Some(rid) = self.media_by_content.get(&key) {
            return Ok(rid.clone());
        }
        let ext = NewImage::extension_for(mime);
        let main = self.main_part();
        let n = (1u32..)
            .find(|n| {
                let stem = format!("word/media/image{n}.");
                !self
                    .package()
                    .parts()
                    .iter()
                    .any(|p| !p.deleted && p.uri.as_str().starts_with(&stem))
            })
            .expect("总有空闲的编号");
        let (_, rid) = self.add_binary_part(
            main,
            RelType::Image,
            &format!("word/media/image{n}.{ext}"),
            mime,
            bytes,
        )?;
        if dedup {
            self.media_by_content.insert(key, rid.clone());
        }
        Ok(rid)
    }

    /// `EDIT-04 ReplaceImageMedia`（TS `xml.replaceImage`）：`drawing` 子树里第一个 `a:blip` 改指新媒体（`r:link`
    /// 删掉，只有 `r:link` 时改成 `r:embed`）、删第一个 `a:srcRect`、`a:fillRect` 属性清空、删 `asvg:svgBlip` 的
    /// `a:ext` 与空掉的 `a:extLst`。没有 `a:blip` → 不动 + 诊断（hostile）。
    pub fn replace_image_media(
        &mut self,
        drawing: NodeId,
        bytes: Vec<u8>,
        mime: &str,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let main = self.main_part();
        let dom = self.dom();
        if (drawing.0 as usize) >= dom.node_count() {
            return Err(Error::edit(DiagCode::EditBadPosition, "replaceImage 的目标节点不存在"));
        }
        // 追踪：旧 run 进 `w:del`、换了图的克隆 run 进 `w:ins`（`spec/18` 7.3）。
        // 两个阶段：克隆先落到 DOM 里，第二阶段才能定位克隆里的 `a:blip` 去改 `r:embed`
        if let Some(mut t) = Tracker::new(self.document(), ctx) {
            let run = std::iter::once(drawing)
                .chain(dom.ancestors(drawing))
                .find(|&a| dom.is(a, QName::w(LocalName::R)))
                .ok_or_else(|| {
                    Error::edit(DiagCode::EditBadPosition, "replaceImage 的目标不在 run 里")
                })?;
            let parent = dom
                .parent(run)
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "run 没有父节点"))?;
            let mut plan = MutationPlan::new(main);
            if let Some(p) = dom.ancestors(run).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                plan.touch(p);
            }
            // 克隆先插（此时源还没被包起来），再把原 run 包进 `w:del`
            let ins_k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before: Dom::next_live_element_sibling(dom, run),
                node: t.marker(LocalName::Ins),
            });
            let clone_k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(ins_k),
                before: None,
                source: run,
            });
            t.wrap_item(&mut plan, dom, run, LocalName::Del);
            let mut result = self.commit_plan(plan)?;
            let clone = result.created[clone_k]
                .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "克隆的 run 没有创建出来"))?;
            // 第二阶段：在克隆里找同一个绘图，按不追踪的路子换图
            let dom = self.dom();
            let cloned_drawing = std::iter::once(clone)
                .chain(dom.descendants(clone))
                .find(|&n| {
                    dom.is(n, QName::w(LocalName::Drawing)) || dom.is(n, QName::w(LocalName::Pict))
                })
                .unwrap_or(clone);
            let plain = EditContext { track_changes: None, ..Default::default() };
            result.absorb(self.replace_image_media(cloned_drawing, bytes, mime, &plain)?);
            return Ok(result);
        }
        let a = |l: LocalName| QName::new(NsId::A, l);
        let Some(blip) = dom.semantic_descendants(drawing).find(|&n| dom.is(n, a(LocalName::Blip)))
        else {
            let diag = Diagnostic::pre_existing(
                main,
                dom.node(drawing).lex.as_ref().map(|l| l.range.clone()),
                DiagCode::EditUnsupported,
                "replaceImage 的目标里没有 a:blip，图片未替换",
            );
            self.push_diagnostic(diag);
            return Ok(MutationResult::default());
        };
        let rid = self.add_media(bytes, mime)?;
        let dom = self.dom();
        let r_embed = QName::new(NsId::R, LocalName::Embed);
        let r_link = QName::new(NsId::R, LocalName::Link);
        let mut plan = MutationPlan::new(main);
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(blip),
            name: r_embed,
            value: rid,
        });
        if dom.attr(blip, r_link).is_some() {
            plan.node_edits.push(NodeEdit::RemoveAttr { node: Target::Node(blip), name: r_link });
        }
        // 旧的裁剪窗与填充窗会把换进来的字节裁掉一块
        if let Some(src) =
            dom.semantic_descendants(drawing).find(|&n| dom.is(n, a(LocalName::SrcRect)))
        {
            plan.node_edits.push(NodeEdit::Delete(src));
        }
        if let Some(fill) = dom.semantic_descendants(drawing).find(|&n| {
            dom.is(n, a(LocalName::FillRect)) && dom.element(n).is_some_and(|e| !e.attrs.is_empty())
        }) {
            for l in [LocalName::L, LocalName::T, LocalName::R, LocalName::B] {
                if dom.attr(fill, QName::new(NsId::None, l)).is_some() {
                    plan.node_edits.push(NodeEdit::RemoveAttr {
                        node: Target::Node(fill),
                        name: QName::new(NsId::None, l),
                    });
                }
            }
        }
        // 换进来的总是位图；Word 会优先用残留的 Office 2016 `asvg:svgBlip` 扩展，删掉
        if let Some(ext_lst) =
            dom.semantic_children(blip).find(|&n| dom.is(n, a(LocalName::ExtLst)))
        {
            let svg_exts: Vec<NodeId> = dom
                .semantic_children(ext_lst)
                .filter(|&e| {
                    dom.is(e, a(LocalName::Ext))
                        && dom.descendants(e).any(|n| {
                            // 任何前缀（TS `<\w+:svgBlip`）；新插入的片段没有 lex_name，按解析后的本地名判
                            dom.element(n).is_some_and(|el| el.name.local == LocalName::SvgBlip)
                        })
                })
                .collect();
            let remaining =
                dom.semantic_children(ext_lst).filter(|e| !svg_exts.contains(e)).count();
            for e in svg_exts {
                plan.node_edits.push(NodeEdit::Delete(e));
            }
            if remaining == 0 {
                plan.node_edits.push(NodeEdit::Delete(ext_lst));
            }
        }
        // 段落投影要刷新（run 图片的 dataUrl 变了）
        if let Some(p) = std::iter::once(drawing)
            .chain(dom.ancestors(drawing))
            .find(|&n| dom.is(n, QName::w(LocalName::P)))
        {
            plan.touch(p);
        }
        self.commit_plan(plan)
    }
}

/// px → EMU（TS `Math.round(px * 9525)`，至少 1）。
pub fn px_to_emu(px: f64) -> i64 {
    ((px * EMU_PER_PX).round() as i64).max(1)
}

// 原 note_ops.rs
// 注释条目的内容操作（`EDIT-03`，`spec/18` 7.5）：`SetNoteContent` / `RemoveNote`。
//
// 条目内容本身在注释 part 的内容流里，改文字用 `InlinePos { part: 注释 part }` 上的
// `InsertText` / `DeleteRange` 就行（TS 的 `text-patch` 场景就是这么做的，格式与超链接
// 原样保留）。这两个操作是**整体替换**与**整条删除**。

// 原 ops.rs
// `EDIT-03` 操作实现（M1 子集）。每个操作是一个或多个 plan/commit 阶段；事务边界在
// [`EditSession::apply`]（失败整体回滚）。这里的函数只读 DOM 与投影、产出 [`MutationPlan`]，
// 写入全部经 `EditSession::commit_plan`。

// ---- 小工具 ----------------------------------------------------------------------------------

pub(super) fn unsupported(msg: &str) -> Error {
    Error::edit(DiagCode::EditUnsupported, msg)
}

// ---- 拆分 run ---------------------------------------------------------------------------------

// ---- InsertText -------------------------------------------------------------------------------

/// 边界的哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

// ---- DeleteRange ------------------------------------------------------------------------------

// ---- SetRunProps ------------------------------------------------------------------------------

// ---- 段落属性与 compat 路径 ---------------------------------------------------------------------

// ---- 块级 -------------------------------------------------------------------------------------

// ---- 表格属性（`EDIT-03`，任务 3.7）------------------------------------------------------------

table_props_op!(
    set_table_props,
    TablePropsPatch,
    Tbl,
    TblPr,
    TblPrChange,
    &[],
    crate::semantic::props::plan_apply_table_props_at,
    "表格"
);
table_props_op!(
    set_row_props,
    RowPropsPatch,
    Tr,
    TrPr,
    TrPrChange,
    // `in_change = false`（`row.toml`）：整行插入 / 删除的标记不进快照
    &[LocalName::Ins, LocalName::Del],
    crate::semantic::props::plan_apply_row_props_at,
    "表格行"
);
table_props_op!(
    set_cell_props,
    CellPropsPatch,
    Tc,
    TcPr,
    TcPrChange,
    // `in_change = false`（`cell.toml`）
    &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge, LocalName::Headers],
    crate::semantic::props::plan_apply_cell_props_at,
    "单元格"
);

// ---- 批注（`EDIT-03` AddComment / RemoveComment / SetCommentText，任务 2.6）--------------------

/// 条目段落：每段一串 run（`NewRun.props` 是整份 `w:rPr`）。
pub type EntryParas = Vec<Vec<NewRun>>;

// ---- 段落拆分与合并（`EDIT-03`，任务 2.9）------------------------------------------------------

// ---- 书签（`EDIT-03` AddBookmark / RemoveBookmark，任务 2.9）-----------------------------------

// ---- 字段操作（`FLD-09`–`FLD-12`，任务 2.9）----------------------------------------------------

// BIND-03 v3：复用声明 part 计划，事务仍由 apply/apply_all 统一持有。

// 原 plan.rs
// `EDIT-05`：`MutationPlan`（只读产出）→ `validate`（只读）→ `commit`（机械写入）→ `MutationResult`。
/// 一次操作（或操作的一个阶段）对某个 part 的全部变更。
#[derive(Debug, Clone, PartialEq)]
pub struct MutationPlan {
    pub part: PartId,
    pub node_edits: Vec<NodeEdit>,
    /// 需要刷新投影的块（`w:p` 或 `w:tbl`）；`Document::refresh_blocks` 就地重建它们。
    pub affected_blocks: Vec<NodeId>,
    /// 块的增删移：投影整体重建。
    pub structure_changed: bool,
    /// 计划阶段发现、提交后记录到会话的诊断（例如 `EDIT_ANCHOR_UNMOVED`）。
    pub diagnostics: Vec<Diagnostic>,
    /// `(para, from, delta)`：供调用方修正光标。
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
    /// 与范围相关的要求（`SPAN-06/07`）；锚点变换本身由 `commit_plan` 从 `node_edits` 推导。
    pub span: SpanPolicy,
}
/// `commit` 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct MutationResult {
    /// 每条 `NodeEdit` 创建的节点（与 `node_edits` 对齐；不创建节点的为 `None`）。
    pub created: Vec<Option<NodeId>>,
    /// 需要刷新投影的块节点。
    pub affected_blocks: Vec<NodeId>,
    /// 块结构是否发生变化。
    pub structure_changed: bool,
    /// 本次操作产生的诊断。
    pub diagnostics: Vec<Diagnostic>,
    /// 段落、UTF-16 位置与长度变化量。
    pub offset_delta: Vec<(NodeId, Utf16Offset, i32)>,
}
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl MutationResult {
    /// 合并多阶段结果（后一阶段的 `created` 覆盖）。
    #[doc(hidden)]
    pub fn absorb(&mut self, later: MutationResult) {
        self.created = later.created;
        for p in later.affected_blocks {
            if !self.affected_blocks.contains(&p) {
                self.affected_blocks.push(p);
            }
        }
        self.structure_changed |= later.structure_changed;
        self.diagnostics.extend(later.diagnostics);
        self.offset_delta.extend(later.offset_delta);
    }
}
impl MutationPlan {
    pub fn new(part: PartId) -> Self {
        Self {
            part,
            node_edits: Vec::new(),
            affected_blocks: Vec::new(),
            structure_changed: false,
            diagnostics: Vec::new(),
            offset_delta: Vec::new(),
            span: SpanPolicy::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.node_edits.is_empty()
    }

    /// 标记一个块（`w:p` / `w:tbl`）需要刷新投影。
    pub fn touch(&mut self, block: NodeId) {
        if !self.affected_blocks.contains(&block) {
            self.affected_blocks.push(block);
        }
    }

    /// 只读校验：每条编辑引用的节点存在、未删除、类型正确；`before` 是 `parent` 的子节点；
    /// `Target::New(k)` 指向前面一条会创建节点的编辑；`Move` 不把节点搬进自己的子树。
    /// 失败 → `Err(EDIT_PLAN_INVALID)`，DOM 未被触碰。
    pub fn validate(&self, dom: &Dom) -> Result<()> {
        let bad = |msg: String| Error::edit(DiagCode::EditPlanInvalid, msg);
        let exists = |n: NodeId| (n.0 as usize) < dom.node_count();
        let live = |n: NodeId| exists(n) && dom.node(n).dirty != Dirty::Deleted;
        let is_element = |n: NodeId| live(n) && dom.element(n).is_some();
        let check_target = |t: Target, i: usize, what: &str| -> Result<Option<NodeId>> {
            match t {
                Target::Node(n) => {
                    if !is_element(n) {
                        return Err(bad(format!("edit[{i}] {what}: 节点 {} 不是活元素", n.0)));
                    }
                    Ok(Some(n))
                }
                Target::New(k) => {
                    if k >= i || !self.node_edits[k].creates() {
                        return Err(bad(format!(
                            "edit[{i}] {what}: Target::New({k}) 不指向前面的创建"
                        )));
                    }
                    Ok(None)
                }
            }
        };
        let check_before = |parent: Option<NodeId>,
                            before: Option<NodeId>,
                            i: usize|
         -> Result<()> {
            match (parent, before) {
                (_, None) => Ok(()),
                (Some(p), Some(b)) => {
                    if !exists(b) || dom.child_index(p, b).is_none() {
                        return Err(bad(format!("edit[{i}]: before {} 不是 parent 的子节点", b.0)));
                    }
                    Ok(())
                }
                (None, Some(_)) => Err(bad(format!("edit[{i}]: 新建父节点下不能指定 before"))),
            }
        };
        for (i, e) in self.node_edits.iter().enumerate() {
            match e {
                NodeEdit::Insert { parent, before, .. } => {
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                }
                NodeEdit::InsertClone { parent, before, source } => {
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                    if !live(*source) {
                        return Err(bad(format!("edit[{i}]: 克隆源 {} 不存在或已删除", source.0)));
                    }
                }
                NodeEdit::Replace { old, .. } | NodeEdit::ReplaceClone { old, .. } => {
                    if !live(*old) || dom.parent(*old).is_none() {
                        return Err(bad(format!("edit[{i}]: 被替换节点 {} 无效", old.0)));
                    }
                    if let NodeEdit::ReplaceClone { source, .. } = e
                        && !live(*source)
                    {
                        return Err(bad(format!("edit[{i}]: 克隆源 {} 不存在或已删除", source.0)));
                    }
                }
                NodeEdit::Delete(n) => {
                    if !exists(*n) {
                        return Err(bad(format!("edit[{i}]: 删除的节点 {} 不存在", n.0)));
                    }
                }
                NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => {
                    check_target(*node, i, "node")?;
                }
                NodeEdit::SetText { node, .. } => {
                    if !live(*node) || !matches!(dom.node(*node).kind, NodeKind::Text(_)) {
                        return Err(bad(format!(
                            "edit[{i}]: SetText 的目标 {} 不是活文本节点",
                            node.0
                        )));
                    }
                }
                NodeEdit::Rename { node, .. } => {
                    if !is_element(*node) {
                        return Err(bad(format!("edit[{i}]: 改名的目标 {} 不是活元素", node.0)));
                    }
                }
                NodeEdit::Move { node, parent, before } => {
                    if !live(*node) || dom.parent(*node).is_none() {
                        return Err(bad(format!("edit[{i}]: 移动的节点 {} 无效", node.0)));
                    }
                    let p = check_target(*parent, i, "parent")?;
                    check_before(p, *before, i)?;
                    if let Some(p) = p
                        && dom.is_ancestor_or_self(*node, p)
                    {
                        return Err(bad(format!(
                            "edit[{i}]: 不能把节点 {} 搬进自己的子树",
                            node.0
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// 机械写入（只调用 `xml::edit` 原语，`XML-12` 脏规则自动成立）。调用方必须先 `validate`。
    pub fn commit(self, dom: &mut Dom) -> MutationResult {
        let created = dom.apply_edits(&self.node_edits);
        debug_assert!(dom.check_dirty_invariants().is_ok(), "XML-12 脏状态不变式被破坏");
        MutationResult {
            created,
            affected_blocks: self.affected_blocks,
            structure_changed: self.structure_changed,
            diagnostics: self.diagnostics,
            offset_delta: self.offset_delta,
        }
    }
}

// 原 revision_ops.rs
// 接受 / 拒绝修订（`EDIT-03` 的那张表，`spec/18` 7.4）。
//
// **接受 / 拒绝是普通 `EditOp`**（分层决策 4）：走 plan / validate / commit，`AcceptAll` 是**一个
// 事务**（任一步失败整体回滚，`EDIT-05`），事务内部按修订逐条提交——顺序是文档序、
// **先内层后外层**（[`crate::model::RevisionIndex::iter_inner_first`]：`w:ins` 里套 `w:del` 时
// 先处理 `del`，内容先消失再解包空壳）。
//
// 拒绝 `*PrChange` 用快照子元素的**整体克隆**，不走属性补丁：快照里可能有本引擎还没建模的
// 子元素，补丁只会还原建模过的那部分（typed 的 `old` 只服务模型与 compat）。

/// 一条修订在某个方向上要做的事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// 解包：子节点搬到包裹原来的位置，包裹本身删掉。
    Unwrap,
    /// 解包，并把 `w:delText` / `w:delInstrText` 改回 `w:t` / `w:instrText`。
    UnwrapLive,
    /// 整棵子树删掉。
    Drop,
    /// 只删标记元素本身（段落标记的 `w:ins`、`*PrChange` 的接受方向）。
    DropMark,
    /// 删标记，再把这一段与下一段合并（无追踪的 `MergeWithNext`）。
    Merge,
    /// 用 `*Change` 里的快照还原容器：`(容器名, 不动的字段)`。
    Restore(LocalName, RevisionKeep),
    /// 删这个格并收缩网格。
    DropCell,
    /// 这个方向不支持。
    Unsupported,
}
/// `RevKind` → （接受动作, 拒绝动作）。一张表同时给出两个方向，
/// `tests/revisions.rs` 的用例列表按同一张表写。

/// `in_change = false` 的字段不在快照里，还原时不能动它们。
/// 策略按值保存，不借用全局字段表；单字节标签无需 packed 或堆分配。
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevisionKeep {
    None,
    Paragraph,
    Section,
    Row,
    Cell,
}

impl RevisionKeep {
    #[inline]
    fn contains(self, local: LocalName) -> bool {
        match self {
            Self::None => false,
            Self::Paragraph => matches!(local, LocalName::RPr | LocalName::SectPr),
            Self::Section => {
                matches!(local, LocalName::HeaderReference | LocalName::FooterReference)
            }
            Self::Row => matches!(local, LocalName::Ins | LocalName::Del),
            Self::Cell => matches!(
                local,
                LocalName::CellIns | LocalName::CellDel | LocalName::CellMerge | LocalName::Headers
            ),
        }
    }
}
accept_reject! {
    // 内容包裹（`owner` 是 `Row` 时另有一套，见 `row_actions`）
    Insert        => Act::Unwrap,   Act::Drop;
    Delete        => Act::Drop,     Act::UnwrapLive;
    MoveFrom      => Act::Drop,     Act::UnwrapLive;
    MoveTo        => Act::Unwrap,   Act::Drop;
    RunInsert     => Act::Unwrap,   Act::Drop;
    RunDelete     => Act::Drop,     Act::UnwrapLive;
    RunMoveFrom   => Act::Drop,     Act::UnwrapLive;
    RunMoveTo     => Act::Unwrap,   Act::Drop;
    // 段落标记
    ParaMarkInsert   => Act::DropMark, Act::Merge;
    ParaMarkDelete   => Act::Merge,    Act::DropMark;
    ParaMarkMoveFrom => Act::Merge,    Act::DropMark;
    ParaMarkMoveTo   => Act::DropMark, Act::Merge;
    // 属性快照
    RunPropsChange     => Act::DropMark, Act::Restore(LocalName::RPr, RevisionKeep::None);
    ParaPropsChange    => Act::DropMark, Act::Restore(LocalName::PPr, RevisionKeep::Paragraph);
    TablePropsChange   => Act::DropMark, Act::Restore(LocalName::TblPr, RevisionKeep::None);
    TablePropsExChange => Act::DropMark, Act::Restore(LocalName::TblPrEx, RevisionKeep::None);
    SectPropsChange    => Act::DropMark, Act::Restore(LocalName::SectPr, RevisionKeep::Section);
    TableGridChange    => Act::DropMark, Act::Restore(LocalName::TblGrid, RevisionKeep::None);
    RowPropsChange     => Act::DropMark, Act::Restore(LocalName::TrPr, RevisionKeep::Row);
    CellPropsChange    => Act::DropMark, Act::Restore(LocalName::TcPr, RevisionKeep::Cell);
    // `w:numberingChange` 只有 `w:original` 属性、没有内层容器（§17.13.5.14，已废弃），
    // 拒绝无从还原：两个方向都只删标记，登记在 `docs/04` §8
    NumberingChange    => Act::DropMark, Act::DropMark;
    // 单元格
    CellInsert => Act::DropMark, Act::DropCell;
    CellDelete => Act::DropCell, Act::DropMark;
    // `vMergeOrig` 的还原形态待真实 Word 校准（`spec/18`「不在 M7」）。顺带一条实测：
    // Word 的「拒绝所有修订」本来也**不**撤销单元格合并
    CellMerge  => Act::DropMark, Act::Unsupported;
}

/// 空掉之后可以整个去掉的属性容器（`w:tblGrid` / `w:sectPr` 不在其中：它们必须存在）。
///
/// 真实 Word 的对照件是这条的出处：`fixtures/revisions/table-and-move/tracked.docx` 有 6 个
/// `w:tblPrEx`，`accepted.docx` 与 `rejected.docx` **一个都没有**——那些行属性覆盖是跟踪操作
/// 的产物，修订一解决 Word 就把整个容器丢掉。
impl Act {
    #[inline]
    fn droppable_empty(local: LocalName) -> bool {
        matches!(
            local,
            LocalName::RPr
                | LocalName::PPr
                | LocalName::TrPr
                | LocalName::TcPr
                | LocalName::TblPr
                | LocalName::TblPrEx
                | LocalName::NumPr
        )
    }
}
/// 要处理的一条修订（把索引里的数据抄出来：处理过程中索引会重建）。
#[derive(Debug, Clone)]
struct Job {
    part: PartId,
    node: NodeId,
    kind: RevKind,
    owner: RevOwner,
    move_name: Option<String>,
    pair: Option<(PartId, NodeId)>,
}

// 原 sdt_ops.rs
// 内容控件的内容操作（`EDIT-03`，`spec/18` 7.5）：`SetSdtContent` / `RemoveSdtShell`。

// 原 section_ops.rs
// 节与页眉页脚的编辑操作（`EDIT-03`、`SAVE-05`、`SAVE-07`，`spec/16` 任务 5.5）。
//
// 五个操作，都走 `plan → validate → commit` 的同一条路（`EDIT-05` 的事务边界在
// [`EditSession::apply`]），没有旁路：
//
// | 操作 | 改什么 |
// | --- | --- |
// | `set_section_props` | 主 part 的 `w:sectPr`（属性表合并，`PROP-06`） |
// | `set_header_footer` | 页眉页脚 part 的内容（整体替换），或按 `SAVE-05` **新建** part |
// | `link_header_footer` | 主 part `sectPr` 里的一条引用（挂到已有 part） |
// | `set_watermark` | default 页眉里的 VML 水印段落 |
// | `set_page_color` | 主 part 的 `w:background` |
//
// **新建 part 的语义**（与 Word / TS 的 `sectionHf` 一致）：这一节自己声明了该变体就改写它引用的
// part——共享这个 part 的前面各节跟着一起变（Word 的"同前"）；没声明（含从上一节继承）就新建一个
// part 并把引用插进**这一节**的 `sectPr`，这一节因此独立，前面的节不受影响。

/// 这一节这个变体的 part：自己声明了就用它，否则按 `SAVE-05` 新建并把引用插进这一节。
pub fn ensure_hf_part(
    s: &mut EditSession,
    sect: NodeId,
    kind: HfKind,
    variant: HfVariant,
) -> Result<PartId> {
    let dom = s.dom();
    if let Some(node) = MutationPlan::reference_of(dom, sect, kind, variant)
        && let Some(rid) = dom.attr_value(node, QName::new(NsId::R, LocalName::Id))
        && let Some(part) = s.document().hf_by_rel.get(rid.as_ref()).copied()
    {
        return Ok(part);
    }
    // 新建：`word/header{N}.xml`，N 取第一个空闲号
    let (rel, ct, base, root) = match kind {
        HfKind::Header => (RelType::Header, CT_HEADER, "header", "hdr"),
        HfKind::Footer => (RelType::Footer, CT_FOOTER, "footer", "ftr"),
    };
    let mut n = 1usize;
    let uri = loop {
        let uri = format!("word/{base}{n}.xml");
        if s.package().find(&crate::package::PartUri::from_entry_name(&uri)).is_none() {
            break uri;
        }
        n += 1;
    };
    let flavor = s.flavor();
    let w_uri = NsId::W.uri(flavor).expect("w 有两族 URI");
    let r_uri = NsId::R.uri(flavor).expect("r 有两族 URI");
    // 空 part：一个空段落（Word 期待页眉至少有一段）
    let xml = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:{root} xmlns:w="{w}" xmlns:r="{r}"><w:p/></w:{root}>"#
        ),
        root = root,
        w = w_uri,
        r = r_uri
    );
    let main = s.main_part();
    let (part, rid) = s.add_part(main, rel, &uri, ct, &xml)?;
    // 引用插进这一节的 `sectPr`（`PROP-05`：引用组在最前）
    let dom = s.dom();
    let mut plan = MutationPlan::new(main);
    plan.touch(sect);
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(sect),
        before: MutationPlan::reference_site(dom, sect),
        node: MutationPlan::reference_element(kind, variant, &rid),
    });
    s.commit_plan(plan)?;
    s.rebuild()?;
    Ok(part)
}

// ---- 分节符的增删（`spec/18` 7.6）--------------------------------------------------------------

// 原 session.rs
// `EDIT-01` 会话：`Package`（规范状态）+ `Document`（投影）+ 事务（`EDIT-05`）。
/// 编辑会话。规范状态是包里各 part 的 DOM；`document()` 是可重建的投影。
#[derive(Clone)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct EditSession {
    pkg: Package,
    doc: Document,
    /// 规范状态的另一半（`docs/03` §6.8）：每个被编辑过的 part 的范围索引。
    /// 按需在**第一次写该 part 之前**建立（那时 DOM 还没被改，锚点与标记一致），
    /// 之后只由 `SPAN-06` 变换维护，绝不由标记反推（`SPAN-02`）。
    spans: HashMap<PartId, SpanIndex>,
    /// 字段索引（`FLD-02`）。与范围不同，它是 DOM 的**投影**——每条事实都能重新读出来，
    /// 所以编辑后直接作废重建，不增量维护。
    fields: HashMap<PartId, FieldIndex>,
    /// 第一次写某个 part 之前记下的字段缺陷数（按诊断代码）。`FLD-13` 用它区分
    /// "输入本来如此"与"编辑造成"：保存前重建，某个代码多出来的就是引擎干的。
    field_baseline: HashMap<PartId, HashMap<DiagCode, usize>>,
    /// 第一次写某个 part 之前记下的「正文引用的 rId」与「当时已有的关系 id」（资源回收，`save/prune.rs`）：
    /// 保存时只回收本次会话让引用数归零的关系——原本就没人引用的关系不动。
    pub rel_baseline: HashMap<PartId, crate::save::prune::RelBaseline>,
    /// 本次会话按内容去重的媒体：`(mime, 字节哈希)` → 主 part 的 `rId`（`add_media`）。
    pub media_by_content: HashMap<(String, u64), String>,
    diagnostics: Vec<Diagnostic>,
    /// 修订 id 的会话内稳定表（`MOD-13`，任务 7.1）：承载节点 → 它第一次拿到的 [`RevisionId`]。
    /// `refresh` / `rebuild` 之后仍然存在的节点复用旧号（arena 里 `NodeId` 稳定），新节点才取新号。
    rev_ids: BTreeMap<(PartId, NodeId), RevisionId>,
    /// 上面那张表的单调计数器。
    next_rev_id: u32,
    /// 是否位于外层事务中；嵌套操作复用外层的完整写前快照。
    txn: bool,
}
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    /// 打开 DOCX 包并重建只读模型；包损坏或超过限制时返回错误。
    pub fn open(bytes: &[u8]) -> Result<Self> {
        Self::from_package(Package::open(bytes)?)
    }

    /// 新建一份空白文档（`SAVE-05`，`spec/18` 7.8）：一个空段、A4 竖向、标准样式与两条编号
    /// 定义。与 TS `buildBlankDocx` 的六个 part 逐字节相同（`save::blank`）。
    ///
    /// `east_asia_font` 是 `docDefaults` 的 `w:eastAsia`；不给就不写。
    pub fn blank(east_asia_font: Option<&str>) -> Result<Self> {
        Self::open(&crate::save::blank_docx(east_asia_font)?)
    }

    #[doc(hidden)]
    pub fn from_package(mut pkg: Package) -> Result<Self> {
        let doc = Document::rebuild(&mut pkg)?;
        let mut s = Self {
            pkg,
            doc,
            spans: HashMap::new(),
            fields: HashMap::new(),
            field_baseline: HashMap::new(),
            rel_baseline: HashMap::new(),
            media_by_content: HashMap::new(),
            diagnostics: Vec::new(),
            rev_ids: BTreeMap::new(),
            next_rev_id: 0,
            txn: false,
        };
        s.stabilize_revisions();
        Ok(s)
    }

    /// 让 [`Document::revisions`] 的编号在会话内稳定（`MOD-13`）。每次投影重建 / 刷新后调用。
    fn stabilize_revisions(&mut self) {
        self.doc.revisions.stabilize(&mut self.rev_ids, &mut self.next_rev_id);
    }

    /// 投影（`MOD-01`）。
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// 只读包上下文，供原生 JSON 投影使用；Package 的低层接口仍处于观察期。
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// 直接改包（测试与工具用）；之后应调用 [`EditSession::rebuild`]。
    #[doc(hidden)]
    pub fn package_mut(&mut self) -> &mut Package {
        &mut self.pkg
    }

    /// 主文档 part 的会话内 ID。
    pub fn main_part(&self) -> PartId {
        self.pkg.main_part()
    }

    /// 主 part 的 DOM。
    pub fn dom(&self) -> &Dom {
        self.pkg.part(self.pkg.main_part()).dom().expect("main part is parsed")
    }

    #[doc(hidden)]
    pub fn flavor(&self) -> PartFlavor {
        self.pkg.flavor_of(self.pkg.main_part())
    }

    // ---- 按 part 的位置（`EDIT-02`，任务 5.5）--------------------------------------------------

    /// 位置里的 part：`None` → 主 part。
    #[doc(hidden)]
    pub fn part_or_main(&self, part: Option<PartId>) -> PartId {
        part.unwrap_or_else(|| self.pkg.main_part())
    }

    /// 某个 part 的 DOM。part 不存在或不是 XML（二进制 / `Opaque`）→ `EDIT_BAD_POSITION`。
    pub fn dom_in(&self, part: Option<PartId>) -> Result<&Dom> {
        let id = self.part_or_main(part);
        self.pkg.part(id).dom().ok_or_else(|| {
            // `Opaque`（`PKG-11` 解析失败）与"这个 part 压根不是 XML"分开报：前者是可以修的
            // 状况（重新给一份好的 part 字节），后者是调用方指错了地方
            let code = if self.pkg.part(id).is_opaque() {
                DiagCode::EditTargetOpaque
            } else {
                DiagCode::EditBadPosition
            };
            Error::edit(code, format!("part {} 没有可编辑的 XML", id.0))
        })
    }

    /// 某个 part 的 flavor（Strict / Transitional 的写法按 part 定，`PKG-08`）。
    #[doc(hidden)]
    pub fn flavor_in(&self, part: Option<PartId>) -> PartFlavor {
        self.pkg.flavor_of(self.part_or_main(part))
    }

    /// 一段 XML → 那个 part 里的 `NewElement`（`XML-14`：前缀按目标 part 的作用域解析）。
    /// 水印那棵 VML 子树是唯一手写的片段，用它解析而不是拼字符串。
    pub fn new_element_from_xml(&mut self, part: PartId, xml: &str) -> Result<NewElement> {
        let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
            Error::edit(DiagCode::EditBadPosition, format!("part {} 没有可编辑的 XML", part.0))
        })?;
        let frags = crate::xml::parse_fragment(dom, xml)
            .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("片段解析失败: {e}")))?;
        frags
            .into_iter()
            .next()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "片段没有顶层元素"))
    }

    /// `owner` 指向 `target` 的关系 id（`LinkHeaderFooter` 要把已有 part 挂到节上）。
    #[doc(hidden)]
    pub fn relationship_id(&self, owner: PartId, target: PartId) -> Option<String> {
        let uri = &self.pkg.part(target).uri;
        self.pkg
            .part(owner)
            .rels
            .iter()
            .find(|r| matches!(&r.target, crate::package::RelTarget::Internal(u) if u == uri))
            .map(|r| r.id.clone())
    }

    /// 某个 part 里的文本段落投影（页眉页脚 / 注释 / 批注条目 / 正文）。
    #[doc(hidden)]
    pub fn text_block_in(&self, part: Option<PartId>, para: NodeId) -> Option<&TextBlock> {
        self.doc.text_block_in(self.part_or_main(part), para)
    }

    /// 主 part 的范围索引（`SPAN-04`）。第一次调用时建立。
    #[doc(hidden)]
    pub fn spans(&mut self) -> Result<&SpanIndex> {
        let part = self.pkg.main_part();
        self.spans_of(part)
    }

    /// 某个 part 的范围索引；不是 XML part 时 `Err`。
    #[doc(hidden)]
    pub fn spans_of(&mut self, part: PartId) -> Result<&SpanIndex> {
        self.ensure_spans(part)?;
        Ok(self.spans.get(&part).expect("just built"))
    }

    /// 已建立的范围索引（不触发建立）。
    #[doc(hidden)]
    pub fn spans_built(&self, part: PartId) -> Option<&SpanIndex> {
        self.spans.get(&part)
    }

    /// 测试用：直接改索引，往里注入破坏，验证 `SPAN-09` 的自检真的会拦下来。
    #[cfg(test)]
    pub fn spans_mut(&mut self, part: PartId) -> Option<&mut SpanIndex> {
        self.spans.get_mut(&part)
    }

    /// 主 part 的字段索引（`FLD-02`）。
    #[doc(hidden)]
    pub fn fields(&mut self) -> Result<&FieldIndex> {
        let part = self.pkg.main_part();
        self.fields_of(part)
    }

    /// 某个 part 的字段索引；编辑之后第一次调用会重建。
    #[doc(hidden)]
    pub fn fields_of(&mut self, part: PartId) -> Result<&FieldIndex> {
        if !self.fields.contains_key(&part) {
            let index = self.build_fields(part)?;
            self.fields.insert(part, index);
        }
        Ok(self.fields.get(&part).expect("just built"))
    }

    fn build_fields(&mut self, part: PartId) -> Result<FieldIndex> {
        let Some(dom) = self.pkg.dom(part)? else {
            return Err(Error::edit(
                DiagCode::EditPlanInvalid,
                format!("part#{} 不是 XML part", part.0),
            ));
        };
        Ok(FieldIndex::build(dom))
    }

    /// `FLD-13`：在第一次写 `part` 之前记下解析期的字段缺陷，并把诊断报一次。
    /// 资源回收的写前基线（见 `save/prune.rs`）：第一次写 `part` 之前记下它引用的 rId 与它当时的关系 id。
    fn ensure_rel_baseline(&mut self, part: PartId) -> Result<()> {
        if self.rel_baseline.contains_key(&part) {
            return Ok(());
        }
        let rel_ids: std::collections::HashSet<String> =
            self.pkg.part(part).rels.iter().map(|r| r.id.clone()).collect();
        let referenced = match self.pkg.dom(part)? {
            Some(dom) => crate::save::prune::referenced_rids(dom),
            None => std::collections::HashSet::new(),
        };
        self.rel_baseline.insert(part, crate::save::prune::RelBaseline { referenced, rel_ids });
        Ok(())
    }

    /// 记一条诊断（编辑操作里的局部降级）。
    pub fn push_diagnostic(&mut self, diag: Diagnostic) {
        self.record(vec![diag]);
    }

    fn ensure_field_baseline(&mut self, part: PartId) -> Result<()> {
        if self.field_baseline.contains_key(&part) {
            return Ok(());
        }
        let mut index = self.build_fields(part)?;
        self.field_baseline.insert(part, index.defect_counts());
        let diags = index.take_diagnostics();
        self.fields.insert(part, index);
        self.record(diags);
        Ok(())
    }

    /// `FLD-13` 的保存前一半：重建字段索引，比基线多出来的缺陷就是本次编辑造成的。
    fn validate_fields(&mut self) -> Result<()> {
        let parts: Vec<PartId> = self.field_baseline.keys().copied().collect();
        let mut diags = Vec::new();
        for part in parts {
            let index = self.build_fields(part)?;
            let before = self.field_baseline.get(&part).cloned().unwrap_or_default();
            let after = index.defect_counts();
            for (code, n) in after {
                let was = before.get(&code).copied().unwrap_or(0);
                if n > was {
                    diags.push(Diagnostic::invariant_violation(
                        part,
                        None,
                        code,
                        format!("字段结构在本次编辑后新增了 {} 处 {code} 缺陷", n - was),
                    ));
                }
            }
            self.fields.insert(part, index);
        }
        crate::save::enforce(&diags)?;
        self.record(diags);
        Ok(())
    }

    /// `SPAN-04`：在第一次写 `part` 之前建立索引。
    ///
    /// 那一刻 DOM 还没被这个会话改过，所以"由标记建立 Anchor"是合法的（`SPAN-02` 只禁止
    /// 编辑期反推）。已经建立过就直接返回。绕过 `commit_plan` 直接改 DOM（`package_mut`）
    /// 之后再建立索引会读到改后的标记——那条路径要求调用方自己 `rebuild`。
    fn ensure_spans(&mut self, part: PartId) -> Result<()> {
        if self.spans.contains_key(&part) {
            return Ok(());
        }
        let Some(dom) = self.pkg.dom(part)? else {
            return Err(Error::edit(
                DiagCode::EditPlanInvalid,
                format!("part#{} 不是 XML part", part.0),
            ));
        };
        let mut index = SpanIndex::build(dom);
        // `SPAN-10`：端点落在原子字段内部时移到原子边界（与插入侧同一条规则，7.5）
        index.snap_to_field_atoms(dom, &FieldIndex::build(dom));
        let diags = index.take_diagnostics();
        self.spans.insert(part, index);
        self.record(diags);
        Ok(())
    }

    /// `SAVE-05`：批注部件，不存在就建（空 `w:comments` 根，命名空间按目标 part 的 flavor）。
    pub fn ensure_comments_part(&mut self) -> Result<PartId> {
        if let Some(p) = self.doc.comments.part {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let xml = empty_root_xml(self.pkg.flavor_of(main), "comments");
        let (id, _) =
            self.add_part(main, RelType::Comments, "word/comments.xml", CT_COMMENTS, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：`word/settings.xml`，不存在就建（清洗标志要有地方写，`SAVE-07`）。
    pub fn ensure_settings_part(&mut self) -> Result<PartId> {
        let main = self.pkg.main_part();
        if let Some(id) = self
            .pkg
            .related(main, RelType::Settings)
            .next()
            .or_else(|| self.pkg.find_name(SETTINGS))
        {
            return Ok(id);
        }
        let xml = empty_root_xml(self.pkg.flavor_of(main), "settings");
        let (id, _) = self.add_part(main, RelType::Settings, SETTINGS, CT_SETTINGS, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：脚注 / 尾注部件，不存在就建（连 Word 期待的 separator 结构条目一起）。
    pub fn ensure_notes_part(&mut self, endnote: bool) -> Result<PartId> {
        let existing = if endnote { self.doc.endnotes.part } else { self.doc.footnotes.part };
        if let Some(p) = existing {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let flavor = self.pkg.flavor_of(main);
        let w = NsId::W.uri(flavor).expect("w 有两族 URI");
        let (root, entry, mark) = if endnote {
            ("endnotes", "endnote", "continuationSeparator")
        } else {
            ("footnotes", "footnote", "continuationSeparator")
        };
        // Word 期待前两条结构条目（`w:id` 为 -1 / 0）
        let xml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<w:{root} xmlns:w="{w}">"#,
                r#"<w:{entry} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{entry}>"#,
                r#"<w:{entry} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:{mark}/></w:r></w:p></w:{entry}>"#,
                r#"</w:{root}>"#
            ),
            root = root,
            entry = entry,
            mark = mark,
            w = w
        );
        let (kind, uri, ct) = if endnote {
            (RelType::Endnotes, "word/endnotes.xml", CT_ENDNOTES)
        } else {
            (RelType::Footnotes, "word/footnotes.xml", CT_FOOTNOTES)
        };
        let (id, _) = self.add_part(main, kind, uri, ct, &xml)?;
        self.rebuild()?;
        Ok(id)
    }

    /// `SAVE-05`：`commentsExtended` 部件（回复与已解决），不存在就建。
    pub fn ensure_comments_extended_part(&mut self) -> Result<PartId> {
        if let Some(p) = self.doc.comments.extended_part {
            return Ok(p);
        }
        let main = self.pkg.main_part();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w15:commentsEx xmlns:w15="{}"/>"#,
            NsId::W15.uri(PartFlavor::Transitional).expect("w15 有 URI")
        );
        let (id, _) = self.add_part(
            main,
            RelType::CommentsExtended,
            "word/commentsExtended.xml",
            CT_COMMENTS_EXTENDED,
            &xml,
        )?;
        self.rebuild()?;
        Ok(id)
    }

    /// 把一个新范围登记进索引（`AddComment` / `AddBookmark`）。标记节点已经写进 DOM，
    /// 所以 `SPAN-08` 物化时它就在锚点指的位置上，不会重发。
    pub fn push_span(&mut self, part: PartId, span: crate::span::RangeSpan) -> Result<()> {
        self.ensure_spans(part)?;
        let index = self.spans.get_mut(&part).expect("just built");
        index.push_span(span);
        index.reindex_containers();
        Ok(())
    }

    /// `SPAN-07`：把索引里的范围标记为已删除（节点的删除由调用方的计划完成）。
    pub fn drop_span(&mut self, part: PartId, id: crate::span::SpanId) {
        if let Some(index) = self.spans.get_mut(&part)
            && let Some(s) = index.get_mut(id)
        {
            s.removed = true;
        }
    }

    /// `SAVE-05`：新建一个 XML part，接上关系与内容类型 Override，返回 `(part, rId)`。
    ///
    /// 三处改动都走 DOM（新 part 的内容、`.rels` 的一条 `Relationship`、
    /// `[Content_Types].xml` 的一条 `Override`），所以未变部分仍是原字节；新 part 在
    /// `SAVE-06` 里追加到 zip 末尾，其余条目原压缩数据不动。
    ///
    /// `xml` 是新 part 的整份内容。`owner` 必须已经有 `.rels`（新建 `.rels` 目前不支持——
    /// 语料里每个 docx 的主 part 都有）。
    #[doc(hidden)]
    pub fn add_part(
        &mut self,
        owner: PartId,
        kind: RelType,
        uri: &str,
        content_type: &str,
        xml: &str,
    ) -> Result<(PartId, String)> {
        let uri = PartUri::from_entry_name(uri);
        if self.pkg.find(&uri).is_some() {
            return Err(Error::edit(DiagCode::EditPlanInvalid, format!("part {uri} 已存在")));
        }
        let part = self.pkg.register_new_part(uri.clone(), content_type, xml)?;
        // 关系目标是**相对 owner 所在目录**的路径（`PKG-04`）：新 part 不在那个目录底下时
        // 要用 `../` 走出去。写成包根相对的路径 Word 会解析成 `word/customXml/…` 而找不到
        // （门 4 的 part 对照抓到的：`sources` 保存选项建的 `customXml/item1.xml`）
        let owner_dir = self.pkg.part(owner).uri.dir().to_string();
        let target = relative_target(&owner_dir, uri.as_str());
        let rid = self.add_relationship(owner, kind, &target, RelTarget::Internal(uri.clone()))?;
        self.add_content_type_override(&uri, content_type)?;
        Ok((part, rid))
    }

    /// `[Content_Types].xml` 里加一条 `Override`（缺内容类型 part 时只记诊断）。
    pub fn add_content_type_override(&mut self, uri: &PartUri, content_type: &str) -> Result<()> {
        let Some(ct_part) = self.pkg.content_types_part() else {
            self.record(vec![Diagnostic::invariant_violation(
                self.pkg.main_part(),
                None,
                DiagCode::EditUnsupported,
                format!("缺 [Content_Types].xml，{uri} 的内容类型写不进去"),
            )]);
            return Ok(());
        };
        let dom = self
            .pkg
            .dom(ct_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "内容类型不是 XML part"))?;
        let root = dom.root();
        // 名字照抄已有的 `Override`（带着 `[Content_Types].xml` 的默认命名空间）
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::Override))
            .unwrap_or_else(|| {
                QName::new(dom.name(root).map(|q| q.ns).unwrap_or(NsId::None), LocalName::Override)
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let node = NewElement::new(name)
            .with_attr(none(LocalName::PartName), format!("/{}", uri.as_str()))
            .with_attr(none(LocalName::ContentType), content_type);
        let mut plan = MutationPlan::new(ct_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: None, node });
        self.commit_plan(plan)?;
        self.pkg.content_types_mut().add_override(uri, content_type);
        Ok(())
    }

    /// `EDIT-06`：给 `part` 的 `.rels` 追加一条外部关系，返回分配到的 `rId`。
    ///
    /// 走 `commit_plan`，所以它在事务里、可回滚，`.rels` 也按脏节点序列化。
    /// part 没有 `.rels` 时报 `EditUnsupported`——新建 `.rels` 属 `SAVE-05`（2.6）。
    #[doc(hidden)]
    pub fn add_external_relationship(
        &mut self,
        part: PartId,
        kind: RelType,
        target: &str,
    ) -> Result<String> {
        self.add_relationship(part, kind, target, RelTarget::External(target.to_string()))
    }

    /// `SAVE-05`：part 的 `.rels`，没有就建（`<dir>/_rels/<name>.rels`）。
    ///
    /// `.rels` 靠 `[Content_Types].xml` 的 `Default Extension="rels"` 声明类型，缺了就补一条。
    pub fn ensure_rels_part(&mut self, part: PartId) -> Result<PartId> {
        if let Some(p) = self.pkg.part(part).rels_part {
            return Ok(p);
        }
        let uri = self.pkg.part(part).uri.clone();
        let dir = uri.dir();
        let path = if dir.is_empty() {
            format!("_rels/{}.rels", uri.file_name())
        } else {
            format!("{dir}/_rels/{}.rels", uri.file_name())
        };
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{RELS_NS}"/>"#
        );
        let rels_part = self.pkg.register_new_part(
            PartUri::from_entry_name(&path),
            "application/vnd.openxmlformats-package.relationships+xml",
            &xml,
        )?;
        self.pkg.part_mut(part).rels_part = Some(rels_part);
        self.ensure_rels_default_type()?;
        Ok(rels_part)
    }

    /// `[Content_Types].xml` 缺 `Default Extension="rels"` 时补一条。
    fn ensure_rels_default_type(&mut self) -> Result<()> {
        self.ensure_default_type("rels", "application/vnd.openxmlformats-package.relationships+xml")
    }

    /// `[Content_Types].xml` 缺某个扩展名的 `Default` 时补一条（`.rels` / `.xlsx` / 媒体扩展名）。
    pub fn ensure_default_type(&mut self, ext: &str, content_type: &str) -> Result<()> {
        let Some(ct_part) = self.pkg.content_types_part() else { return Ok(()) };
        if self.pkg.content_types().default_for_extension(ext).is_some() {
            return Ok(());
        }
        let dom = self
            .pkg
            .dom(ct_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "内容类型不是 XML part"))?;
        let root = dom.root();
        // 缓存之外再看一眼 DOM（本会话刚补过的也算）：`Default Extension` 重复会让 Word 弹恢复提示
        let already = dom.semantic_children(root).any(|c| {
            dom.name(c).is_some_and(|q| q.local == LocalName::UDefault)
                && dom
                    .attr_value(c, QName::new(NsId::None, LocalName::UExtension))
                    .is_some_and(|v| v.eq_ignore_ascii_case(ext))
        });
        if already {
            self.pkg.content_types_mut().add_default(ext, content_type);
            return Ok(());
        }
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::UDefault))
            .unwrap_or_else(|| {
                QName::new(dom.name(root).map(|q| q.ns).unwrap_or(NsId::None), LocalName::UDefault)
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let node = NewElement::new(name)
            .with_attr(none(LocalName::UExtension), ext)
            .with_attr(none(LocalName::ContentType), content_type);
        let first = dom.children(root).first().copied();
        let mut plan = MutationPlan::new(ct_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: first, node });
        self.commit_plan(plan)?;
        self.pkg.content_types_mut().add_default(ext, content_type);
        Ok(())
    }

    /// 给 `part` 的 `.rels` 追加一条关系（内部或外部），返回分配到的 `rId`。
    pub fn add_relationship(
        &mut self,
        part: PartId,
        kind: RelType,
        target: &str,
        resolved: RelTarget,
    ) -> Result<String> {
        let external = matches!(resolved, RelTarget::External(_));
        let rels_part = self.ensure_rels_part(part)?;
        let id = self.pkg.part(part).rels.next_id();
        let flavor = self.pkg.flavor_of(part);
        let raw_type = kind.uri(flavor).ok_or_else(|| {
            Error::edit(DiagCode::EditUnsupported, format!("关系类型 {kind:?} 没有 URI"))
        })?;
        let dom = self
            .pkg
            .dom(rels_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, ".rels 不是 XML part"))?;
        let root = dom.root();
        // 名字照抄已有的 `Relationship`（它带着 `.rels` 的默认命名空间）；一条都没有时按根元素的
        // 命名空间造一个
        let name = dom
            .children(root)
            .iter()
            .find_map(|&c| dom.name(c).filter(|q| q.local == LocalName::Relationship))
            .unwrap_or_else(|| {
                QName::new(
                    dom.name(root).map(|q| q.ns).unwrap_or(NsId::None),
                    LocalName::Relationship,
                )
            });
        let none = |l: LocalName| QName::new(NsId::None, l);
        let mut node = NewElement::new(name)
            .with_attr(none(LocalName::UId), id.clone())
            .with_attr(none(LocalName::UType), raw_type.clone())
            .with_attr(none(LocalName::Target), target);
        if external {
            node.push_attr(none(LocalName::TargetMode), "External");
        }
        let mut plan = MutationPlan::new(rels_part);
        plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before: None, node });
        let result = self.commit_plan(plan)?;
        let created = result
            .created
            .first()
            .copied()
            .flatten()
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "关系节点没有创建成功"))?;
        let (rel_kind, family) = RelType::parse(&raw_type);
        self.pkg.part_mut(part).rels.push(Relationship {
            id: id.clone(),
            kind: rel_kind,
            target: resolved,
            raw_type,
            family,
            node: created,
        });
        Ok(id)
    }

    /// 记诊断（会话与包各留一份）。
    pub fn record(&mut self, diags: Vec<Diagnostic>) {
        if diags.is_empty() {
            return;
        }
        self.diagnostics.extend(diags.iter().cloned());
        self.pkg.push_diagnostics(diags);
    }

    /// 编辑阶段累计的诊断（不含包 / 保存阶段的）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// 主 part 里的文本段落块，**含表格单元格内任意深度的**（任务 3.6）。
    pub fn text_block(&self, para: NodeId) -> Option<&TextBlock> {
        self.doc.text_block(para)
    }

    /// 正文第 `i` 个文本段落（测试便利）。
    #[doc(hidden)]
    pub fn nth_text_block(&self, i: usize) -> Option<&TextBlock> {
        self.doc.text_blocks().nth(i)
    }

    /// `EDIT-02`：定位。
    #[doc(hidden)]
    pub fn locate(&self, pos: InlinePos) -> Result<Loc> {
        let tb = self
            .text_block(pos.para)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "不是正文文本段落"))?;
        pos.offset.locate(tb)
    }

    /// 应用一个操作：失败时会话状态（DOM 与投影）与操作前一致。
    pub fn apply(&mut self, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
        self.transaction(|s| EditSession::run(s, op, ctx))
    }

    /// 批量应用：任一失败则整批不生效。
    pub fn apply_all(
        &mut self,
        ops: Vec<EditOp>,
        ctx: &EditContext,
    ) -> Result<Vec<MutationResult>> {
        self.transaction(|s| {
            let mut results = Vec::with_capacity(ops.len());
            for op in ops {
                results.push(EditSession::run(s, op, ctx)?);
            }
            Ok(results)
        })
    }

    /// `EDIT-05`：失败恢复完整会话，包括新 part/关系、诊断、修订 ID 分配器与缓存。
    /// 只恢复被写 DOM 再 rebuild 会改变可观察的警告历史、遗漏包级准备操作；嵌套复用外层快照。
    fn transaction<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.txn {
            return f(self);
        }
        let before = self.clone();
        self.txn = true;
        match f(self) {
            Ok(v) => {
                self.txn = false;
                Ok(v)
            }
            Err(e) => {
                *self = before;
                Err(e)
            }
        }
    }

    /// `SAVE-01`：等价于 `save_with(&SaveOptions::default())`。
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.save_with(&SaveOptions::default())
    }

    /// `SAVE-01` 全流程：
    ///
    /// 1. 无脏节点且 `opts` 没有变更请求（`saved_at` 单独设置不算，与 TS `isUnchanged` 一致）
    ///    → 返回原字节（BIND-04 v3：不隐式沿用文档的隐私清洗标志）。
    /// 2. 校验（`SAVE-02`，在 [`Package::save`] 里）。
    /// 3. 物化 Span（`SPAN-08`）与范围校验（`SPAN-09`）：位置没变的标记不动，变了的重发。
    /// 4. 应用保存选项（`SAVE-07`）：全部先 `validate`（只读）再逐个 `commit`，所以要么全做要么不动。
    /// 5. / 6. 序列化脏 part 并写回（`XML-13` / `SAVE-06`，在 [`Package::save`] 里）。
    pub fn save_with(&mut self, opts: &SaveOptions) -> Result<Vec<u8>> {
        let compat = crate::save::options::CompatSaveOptions::from(opts);
        self.save_inner(&compat, opts.remove_personal_info, opts.remove_date_and_time)
    }

    /// 测试专用旧选项入口；保留 TS 宿主保存策略，原生内容修改通过 EditOp。
    #[doc(hidden)]
    pub fn save_with_compat(
        &mut self,
        opts: &crate::save::options::CompatSaveOptions,
    ) -> Result<Vec<u8>> {
        let authors = opts.remove_personal_info.unwrap_or_else(|| self.remove_personal_info_flag());
        let dates = opts.remove_date_and_time.unwrap_or_else(|| self.remove_date_and_time_flag());
        self.save_inner(opts, authors, dates)
    }

    fn save_inner(
        &mut self,
        opts: &crate::save::options::CompatSaveOptions,
        authors: bool,
        dates: bool,
    ) -> Result<Vec<u8>> {
        if !self.pkg.is_dirty() && !opts.forces_save() && !authors && !dates {
            return Ok(self.pkg.original_bytes().to_vec());
        }
        // 要写 `true` 的清洗标志得有地方放：缺 `word/settings.xml` 就按 `SAVE-05` 建一个。
        // 写 `false` 时不建——标志缺失本来就等于 false，凭空造个 part 只是噪音。
        if opts.remove_personal_info == Some(true) || opts.remove_date_and_time == Some(true) {
            self.transaction(|s| s.ensure_settings_part().map(|_| ()))?;
        }
        // `SAVE-07` 的内容类选项（节 / 页眉页脚 / 水印 / 底色 / 保护 / 奇偶页眉）先翻成 5.5 的
        // 编辑操作走一遍 `apply_all`：与手写这些操作完全同一条路（同一套校验、脏标记与新建 part）。
        let (ops, created) = crate::save::options::edit_ops(self, opts);
        if !ops.is_empty() {
            self.apply_all(ops, &EditContext::default())?;
        }
        // `hfAllSections` 要等上一轮把 part 建出来才知道挂哪个，所以分两轮
        let links = crate::save::options::link_ops(self, opts, &created);
        if !links.is_empty() {
            self.apply_all(links, &EditContext::default())?;
        }
        // 5.7：声明 part 的选项要改的 part 缺了就先按 `SAVE-05` 建（`plan_all` 是只读的）
        if self.transaction(|s| crate::save::options::decl::ensure_parts(s, opts))? {
            self.rebuild()?;
        }
        let (plans, diags) = crate::save::options::plan_all(&mut self.pkg, opts, authors, dates)?;
        let mut touches_main = plans.iter().any(|p| p.part == self.pkg.main_part());
        touches_main |= self.transaction(|s| s.materialize_spans())?;
        // `FLD-13`：物化之后字段结构应当仍然完好（物化只动范围标记，不该碰 fldChar）
        self.validate_fields()?;
        self.transaction(|s| {
            // 先整批只读校验，再逐个提交：提交阶段不可能失败（失败也会被事务回滚）
            for plan in &plans {
                let dom = s.pkg.part(plan.part).dom().ok_or_else(|| {
                    Error::edit(DiagCode::EditPlanInvalid, "保存选项的目标不是 XML part")
                })?;
                plan.validate(dom)?;
            }
            for plan in plans {
                s.commit_plan(plan)?;
            }
            Ok(())
        })?;
        self.diagnostics.extend(diags.iter().cloned());
        self.pkg.push_diagnostics(diags);
        // `PROP-05` 的兜底整理：脏了的属性容器按 schema 序重排（`save::plan_reorder_props`）。
        // 同一次提交里好几处各自往同一个容器插子元素时，各自算的插入位置可能排错——在这里收口
        self.transaction(|s| {
            let parts: Vec<PartId> = (0..s.pkg.parts().len()).map(|i| PartId(i as u32)).collect();
            for part in parts {
                let Some(dom) = s.pkg.part(part).dom() else { continue };
                let edits = crate::save::plan_reorder_props(dom);
                if edits.is_empty() {
                    continue;
                }
                let mut plan = MutationPlan::new(part);
                plan.node_edits = edits;
                s.commit_plan(plan)?;
            }
            Ok(())
        })?;
        if touches_main {
            self.rebuild()?;
        }
        // 6.7：回收本次会话让引用数归零的资源（关系 + part 子图 + `[Content_Types]` Override）
        if opts.prune_orphans.unwrap_or(true) {
            self.transaction(|s| s.prune_orphans().map(|_| ()))?;
        }
        self.pkg.save()
    }

    /// `SAVE-01` 步骤 3：把每个 part 的 Anchor 物化成标记（`SPAN-08`），顺带做范围校验
    /// （`SPAN-09`）。返回主 part 是否被改动（需要重建投影）。
    ///
    /// 这个计划**不走**锚点变换：标记是 Anchor 的投影，不能反过来影响它（`SPAN-02`）。
    fn materialize_spans(&mut self) -> Result<bool> {
        let main = self.pkg.main_part();
        let mut touched_main = false;
        let parts: Vec<PartId> = self.spans.keys().copied().collect();
        for part in parts {
            let index = self.spans.get(&part).expect("key came from the map");
            let dom = self.pkg.part(part).dom().ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
            })?;
            let mplan = plan_save(dom, index);
            if mplan.is_empty() {
                continue;
            }
            // `SAVE-02`：范围校验里的 `EngineInvariantViolation`（引擎自己弄丢 / 弄反了端点）
            // 在调试构建与 CI 下是错误。输入本来就损坏的、以及调用方整体重写容器时丢的那一端
            // 记成 `PreExistingDamage`（`SpanOrigin::Damaged`），不在这里拦。
            crate::save::enforce(&mplan.diagnostics)?;
            let mut plan = MutationPlan::new(part);
            plan.node_edits = mplan.edits.clone();
            let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
            })?;
            plan.validate(dom)?;
            let has_edits = !plan.node_edits.is_empty();
            let result = plan.commit(dom);
            let index = self.spans.get_mut(&part).expect("key came from the map");
            crate::span::apply_save(index, &result.created, &mplan);
            self.record(mplan.diagnostics);
            touched_main |= has_edits && part == main;
        }
        Ok(touched_main)
    }

    /// 文档自带的 `w:removePersonalInformation`（`SAVE-07`：设置或文档标志为真时清洗作者）。
    #[doc(hidden)]
    pub fn remove_personal_info_flag(&self) -> bool {
        self.doc.settings.as_ref().and_then(|s| s.remove_personal_information) == Some(true)
    }

    /// 文档自带的 `w:removeDateAndTime`（设置或文档标志为真时删批注日期）。
    #[doc(hidden)]
    pub fn remove_date_and_time_flag(&self) -> bool {
        self.doc.settings.as_ref().and_then(|s| s.remove_date_and_time) == Some(true)
    }

    /// 投影整体重建。
    #[doc(hidden)]
    pub fn rebuild(&mut self) -> Result<()> {
        self.doc = Document::rebuild(&mut self.pkg)?;
        self.stabilize_revisions();
        Ok(())
    }

    /// `ReplacePartXml`：整个 XML part 换成 `xml`（TS `partXml`）。只接受**已存在**的 XML part：
    /// 不存在 → `EDIT_TARGET_MISSING`（TS 静默忽略，`docs/04` §8），二进制 part → `EDIT_TARGET_OPAQUE`。
    /// 新内容经解析成为该 part 的新 DOM（良构校验），关系与内容类型不动；投影整体重建。
    pub fn replace_part_xml(&mut self, part: PartId, xml: &str) -> Result<()> {
        if (part.0 as usize) >= self.pkg.parts().len() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("part#{} 不在包里", part.0),
            ));
        }
        if !self.pkg.part(part).is_xml {
            return Err(Error::edit(
                DiagCode::EditTargetOpaque,
                format!("{} 不是 XML part，不能按 XML 替换", self.pkg.part(part).uri),
            ));
        }
        self.ensure_rel_baseline(part)?;
        self.pkg.replace_part_xml(part, xml)?;
        self.spans.remove(&part);
        self.fields.remove(&part);
        self.rebuild()
    }

    /// `ReplacePartBytes`：整个 part 换成给定字节（TS `partBinary`）。主 part 不能换（它的 DOM 是模型的根）。
    pub fn replace_part_bytes(&mut self, part: PartId, bytes: Vec<u8>) -> Result<()> {
        if (part.0 as usize) >= self.pkg.parts().len() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("part#{} 不在包里", part.0),
            ));
        }
        if part == self.pkg.main_part() {
            return Err(Error::edit(DiagCode::EditUnsupported, "主 part 不能按二进制替换"));
        }
        self.pkg.replace_part_bytes(part, bytes);
        self.spans.remove(&part);
        self.fields.remove(&part);
        self.rebuild()
    }

    /// `SAVE-05`：新建一个二进制 part（内嵌工作簿、媒体），接上关系与按扩展名的 `Default` 内容类型，
    /// 返回 `(part, rId)`。
    #[doc(hidden)]
    pub fn add_binary_part(
        &mut self,
        owner: PartId,
        kind: RelType,
        uri: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(PartId, String)> {
        let uri = PartUri::from_entry_name(uri);
        if self.pkg.find(&uri).is_some() {
            return Err(Error::edit(DiagCode::EditPlanInvalid, format!("part {uri} 已存在")));
        }
        let part = self.pkg.register_new_binary_part(uri.clone(), content_type, bytes)?;
        let owner_dir = self.pkg.part(owner).uri.dir().to_string();
        let target =
            uri.as_str().strip_prefix(&format!("{owner_dir}/")).unwrap_or(uri.as_str()).to_string();
        let rid = self.add_relationship(owner, kind, &target, RelTarget::Internal(uri.clone()))?;
        if let Some(ext) = uri.as_str().rsplit_once('.').map(|(_, e)| e.to_string()) {
            self.ensure_default_type(&ext, content_type)?;
        }
        Ok((part, rid))
    }

    /// 一个阶段：`validate` → `commit` → 刷新投影 → 记诊断。编辑操作只碰主 part；保存选项
    /// （`SAVE-07`）也走这里，可以指向任意 XML part（投影只在主 part 上刷新）。
    pub fn commit_plan(&mut self, mut plan: MutationPlan) -> Result<MutationResult> {
        let main = self.pkg.main_part();
        let part = plan.part;
        self.ensure_spans(part)?;
        self.ensure_field_baseline(part)?;
        self.ensure_rel_baseline(part)?;
        let dom = self.pkg.dom_mut(part)?.ok_or_else(|| {
            Error::edit(DiagCode::EditPlanInvalid, format!("part#{} 不是 XML part", part.0))
        })?;
        let index = self.spans.get(&part).expect("ensure_spans built it");
        // `SPAN-06`：锚点变换从编辑列表推导，每个操作都自动得到维护
        let mut update = plan_update(dom, index, &plan.node_edits, &plan.span);
        // `SPAN-07`：整体删除的范围连标记与 reference run 一起删
        let extra: Vec<NodeId> = update.removed_nodes().collect();
        if !extra.is_empty() {
            let touches_content = extra.iter().any(|&n| is_content_item(dom, n));
            plan.node_edits.extend(extra.into_iter().map(NodeEdit::Delete));
            if touches_content {
                // 删掉的 reference run 是内容项，边界要按最终的编辑列表重算
                update = plan_update(dom, index, &plan.node_edits, &plan.span);
            }
        }
        plan.validate(dom)?;
        let span_diags = std::mem::take(&mut update.diagnostics);
        let result = plan.commit(&mut *dom);
        if !update.is_empty() {
            let index = self.spans.get_mut(&part).expect("ensure_spans built it");
            index.apply(&*dom, &update);
            let more = index.take_diagnostics();
            self.record(more);
        }
        self.record(span_diags);
        // 字段索引是投影：DOM 变了就作废，下次问的时候重建（`FLD-02`）
        self.fields.remove(&part);
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if part == main {
            if result.structure_changed {
                self.rebuild()?;
            } else if !result.affected_blocks.is_empty() {
                // 容器级刷新（`MOD-13`）：单元格内的段落也就地重建。真找不到（投影与 DOM 不同步）
                // 才整体重建——那是兜底，不是正常路径
                let missing = self.doc.refresh_blocks(&mut self.pkg, &result.affected_blocks)?;
                if missing.is_empty() {
                    self.stabilize_revisions();
                } else {
                    self.rebuild()?;
                }
            }
        } else if !result.affected_blocks.is_empty() || result.structure_changed {
            // 辅助 part（页眉页脚 / 注释 / 批注，任务 5.5）：整体重建投影。
            // 这些 part 很小（几 KB），容器级增量不值得；`MOD-13` 的 oracle 照样成立
            // （`refresh` 的结果等于 `rebuild`——这里就是 `rebuild`）。
            self.rebuild()?;
        }
        Ok(result)
    }
}
/// `word/settings.xml` 的约定路径。
const SETTINGS: &str = "word/settings.xml";
/// `.rels` 的根命名空间。
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// `SAVE-05` 的内容类型。
pub const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
pub const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
pub const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
pub const CT_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
pub const CT_HEADER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
pub const CT_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
pub const CT_COMMENTS_EXTENDED: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml";
/// 新建 `w:` 部件的空根：`<w:xxx xmlns:w="…"/>`，URI 按目标 part 的 flavor。
///
/// 只声明用得上的命名空间；`w14:paraId` 一类扩展前缀由 `SAVE-03` 的
/// `ensure_extension_declarations` 在序列化前按需补声明（连 `mc:Ignorable` 一起）。
fn empty_root_xml(flavor: PartFlavor, local: &str) -> String {
    let w = NsId::W.uri(flavor).expect("w 有两族 URI");
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{local} xmlns:w="{w}"/>"#)
}
/// `dir` 目录下的一个 part 指向 `uri` 时该写的相对路径（`PKG-04`）。
///
/// 两边共同的前缀去掉，`dir` 剩下几层就补几个 `../`。`dir` 为空（包根的 part）时就是 `uri` 本身。
fn relative_target(dir: &str, uri: &str) -> String {
    let from: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = uri.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = String::new();
    for _ in common..from.len() {
        out.push_str("../");
    }
    out.push_str(&to[common..].join("/"));
    out
}

// 原 shape_gen.rs
// 新建浮动文本框 / 形状 / 线条（`spec/18` 7.7 的 `NewBlock::Textbox / Shape / Line`）。
//
// 都是 DrawingML 的 `wps:wsp`。Transitional 包按 Word / TS 的形态发一对
// `mc:Choice Requires="wps"` + `mc:Fallback`（VML 孪生）；**Strict 包只发 Choice**——
// Strict 里没有 VML，发了反而是不合法的内容。
//
// `Requires="wps"` 的前缀必须在 `mc:AlternateContent` 那一层能解析出来，不然 Word 会把整份
// 文件报成"内容有问题"（TS `generate.ts` 的同一条注释）。所以 `xmlns:wps` 声明写在
// `mc:AlternateContent` 上，不写在里层的 `wps:wsp` 上。
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const NS_WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const NS_V: &str = "urn:schemas-microsoft-com:vml";
/// EMU / pt（VML 的 `@style` 用 pt）。
pub const EMU_PER_PT: f64 = 12700.0;
/// `a:prstGeom/@prst`（ECMA-376 `ST_ShapeType`）：`rect` / `roundRect` / `ellipse` / `triangle` / …
/// 不是闭集——Word 认的两百来个预置形状都能写，值原样进属性。
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(transparent)]
pub struct PresetGeom(pub String);
impl PresetGeom {
    pub fn rect() -> Self {
        PresetGeom("rect".into())
    }
}
edit_enum! {
    /// 可插入的线条 / 连接符（TS `LINE_KINDS`）。
    pub enum LineKind {
        Line = "line",
        LineArrow = "lineArrow",
        LineArrowDouble = "lineArrowDouble",
        LineBent = "lineBent",
        LineCurved = "lineCurved",
    }
}
impl LineKind {
    /// `(a:prstGeom/@prst, 起点箭头, 终点箭头)`（TS `LINE_KINDS` 的三个字段）。
    fn geom(self) -> (&'static str, bool, bool) {
        match self {
            LineKind::Line => ("line", false, false),
            LineKind::LineArrow => ("straightConnector1", false, true),
            LineKind::LineArrowDouble => ("straightConnector1", true, true),
            LineKind::LineBent => ("bentConnector3", false, false),
            LineKind::LineCurved => ("curvedConnector3", false, false),
        }
    }
}
/// 新建文本框 / 形状共用的外观与位置。
#[derive(Debug, Clone, PartialEq, Eq, Default, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShapeLook {
    /// 显示尺寸（EMU）；缺省 1800000 × 1080000（TS `buildTextboxParagraphXml`）。
    pub extent_emu: Option<(i64, i64)>,
    /// 锚定位置；`None` = 横向居中、纵向贴段落（TS 同）。
    pub pos_offset_emu: Option<PosOffset>,
    /// 绕排；缺省方形。
    pub wrap: Option<ImageWrap>,
    /// `relativeHeight = 251658240 + z_order`。
    pub z_order: Option<i64>,
    /// 填充色（六位十六进制）；`None` = `a:noFill`。
    pub fill: Option<String>,
    /// 描边色；`None` = `a:ln/a:noFill`。
    pub outline: Option<String>,
}
fn hex(v: &str) -> String {
    v.trim_start_matches('#').to_string()
}
/// `wps:spPr`：几何 + 填充 + 描边。
fn sp_pr(preset: &str, cx: i64, cy: i64, look: &ShapeLook) -> String {
    let fill = match &look.fill {
        Some(c) => format!(r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#, hex(c)),
        None => "<a:noFill/>".into(),
    };
    let line = match &look.outline {
        Some(c) => {
            format!(r#"<a:ln><a:solidFill><a:srgbClr val="{}"/></a:solidFill></a:ln>"#, hex(c))
        }
        None => "<a:ln><a:noFill/></a:ln>".into(),
    };
    format!(
        concat!(
            r#"<wps:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
            r#"<a:prstGeom prst="{preset}"><a:avLst/></a:prstGeom>{fill}{line}</wps:spPr>"#
        ),
        cx = cx,
        cy = cy,
        preset = preset,
        fill = fill,
        line = line,
    )
}
/// `wp:anchor` 的开头到 `wp:docPr`（`a:graphic` 由调用方接上）。
fn anchor_open(wp: &str, cx: i64, cy: i64, look: &ShapeLook, id: i64, name: &str) -> String {
    let wrap = look.wrap.unwrap_or(ImageWrap::SquareLeft);
    let behind = if wrap == ImageWrap::Behind { "1" } else { "0" };
    let position = match look.pos_offset_emu {
        Some(p) => {
            let (rh, rv) = if p.page { ("page", "page") } else { ("column", "paragraph") };
            format!(
                concat!(
                    r#"<{wp}:positionH relativeFrom="{rh}"><{wp}:posOffset>{x}</{wp}:posOffset></{wp}:positionH>"#,
                    r#"<{wp}:positionV relativeFrom="{rv}"><{wp}:posOffset>{y}</{wp}:posOffset></{wp}:positionV>"#
                ),
                wp = wp,
                rh = rh,
                rv = rv,
                x = p.x,
                y = p.y
            )
        }
        None => format!(
            concat!(
                r#"<{wp}:positionH relativeFrom="column"><{wp}:align>center</{wp}:align></{wp}:positionH>"#,
                r#"<{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>0</{wp}:posOffset></{wp}:positionV>"#
            ),
            wp = wp
        ),
    };
    let wrap_el = match wrap {
        ImageWrap::TopBottom => format!("<{wp}:wrapTopAndBottom/>"),
        ImageWrap::Front | ImageWrap::Behind => format!("<{wp}:wrapNone/>"),
        _ => format!(r#"<{wp}:wrapSquare wrapText="bothSides"/>"#),
    };
    format!(
        concat!(
            r#"<{wp}:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0""#,
            r#" relativeHeight="{z}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1">"#,
            r#"<{wp}:simplePos x="0" y="0"/>{position}"#,
            r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="0" t="0" r="0" b="0"/>{wrap_el}"#,
            r#"<{wp}:docPr id="{id}" name="{name} {id}"/>"#
        ),
        wp = wp,
        z = Z_ORDER_BASE + look.z_order.unwrap_or(0),
        behind = behind,
        position = position,
        cx = cx,
        cy = cy,
        wrap_el = wrap_el,
        id = id,
        name = name,
    )
}
/// 把 `wps:wsp` 裹成一个 `w:p`：Transitional 发 `mc:AlternateContent`（Choice + VML Fallback），
/// Strict 只发 `w:drawing`。
fn wrap_paragraph(
    flavor: PartFlavor,
    wp: &str,
    wp_decl: &str,
    anchor_head: &str,
    wsp: &str,
    vml: Option<&str>,
) -> String {
    let graphic = format!(
        r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{wps}">{wsp}</a:graphicData></a:graphic>"#,
        a = NS_A,
        wps = NS_WPS,
        wsp = wsp,
    );
    let drawing = format!("<w:drawing{wp_decl}>{anchor_head}{graphic}</{wp}:anchor></w:drawing>");
    match (flavor, vml) {
        (PartFlavor::Strict, _) | (_, None) => format!("<w:p><w:r>{drawing}</w:r></w:p>"),
        (_, Some(vml)) => format!(
            concat!(
                r#"<w:p><w:r><mc:AlternateContent xmlns:mc="{mc}" xmlns:wps="{wps}">"#,
                r#"<mc:Choice Requires="wps">{drawing}</mc:Choice>"#,
                r#"<mc:Fallback><w:pict>{vml}</w:pict></mc:Fallback>"#,
                r#"</mc:AlternateContent></w:r></w:p>"#
            ),
            mc = NS_MC,
            wps = NS_WPS,
            drawing = drawing,
            vml = vml,
        ),
    }
}
/// `NewBlock::Textbox` / `Shape` / `Line` → 段落子树（`chart_ops::materialize` 调）。
///
/// 模板里的 `w:txbxContent` 先留空，解析完再把内容块（以及 VML 孪生那份的克隆）挂进去——
/// 内容是 [`NewBlock`]，拼字符串拼不出来。
pub fn shape_paragraph(s: &mut EditSession, block: NewBlock) -> Result<NewElement> {
    let flavor = s.flavor();
    let main = s.main_part();
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let id = NewImage::media_ops_next_doc_pr_id(dom);
    let ctx = NamespaceContext::from_dom(dom, flavor);
    let (wp, wp_decl) = NewImage::media_ops_prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
    let pt = |emu: i64| format!("{:.2}", emu as f64 / EMU_PER_PT);

    // 框里的内容块先落成 `NewElement`（空的话给一个空格段：Word 不接受空文本框）
    let inner: Vec<NewBlock> = match &block {
        NewBlock::Textbox { blocks, .. } if !blocks.is_empty() => blocks.clone(),
        NewBlock::Textbox { .. } => vec![space_paragraph()],
        NewBlock::Shape { text: Some(t), .. } => vec![text_paragraph(t)],
        _ => Vec::new(),
    };
    let inner = EditSession::materialize_all(s, inner)?;
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let inner: Vec<NewElement> =
        inner.into_iter().map(|b| MutationPlan::new_block_element(dom, b)).collect();
    let has_text = !inner.is_empty();

    let xml = match &block {
        NewBlock::Textbox { look, .. } | NewBlock::Shape { look, .. } => {
            let (preset, name) = match &block {
                NewBlock::Shape { preset, .. } => (preset.0.as_str(), "Shape"),
                _ => ("rect", "TextBox"),
            };
            let (cx, cy) = look.extent_emu.unwrap_or((1_800_000, 1_080_000));
            let (cx, cy) = (cx.max(1), cy.max(1));
            let wsp = format!(
                "<wps:wsp><wps:cNvSpPr{tx}/>{sp}{txbx}<wps:bodyPr/></wps:wsp>",
                tx = if has_text { r#" txBox="1""# } else { "" },
                sp = sp_pr(preset, cx, cy, look),
                txbx = if has_text { "<wps:txbx><w:txbxContent/></wps:txbx>" } else { "" },
            );
            let vml = format!(
                concat!(
                    r#"<v:rect xmlns:v="{v}" style="position:absolute;width:{w}pt;height:{h}pt""#,
                    r#"{fill} stroked="{stroked}">{txbx}</v:rect>"#
                ),
                v = NS_V,
                w = pt(cx),
                h = pt(cy),
                fill = match &look.fill {
                    Some(c) => format!(r##" fillcolor="#{}" filled="t""##, hex(c)),
                    None => r#" filled="f""#.to_string(),
                },
                stroked = if look.outline.is_some() { "t" } else { "f" },
                txbx = if has_text { "<v:textbox><w:txbxContent/></v:textbox>" } else { "" },
            );
            wrap_paragraph(
                flavor,
                &wp,
                &wp_decl,
                &anchor_open(&wp, cx, cy, look, id, name),
                &wsp,
                Some(&vml),
            )
        }
        NewBlock::Line { kind, from, to, color } => {
            let (cx, cy) = ((to.0 - from.0).abs().max(1), (to.1 - from.1).abs().max(1));
            let (prst, head, tail) = kind.geom();
            let color = color.as_deref().map_or_else(|| "000000".to_string(), hex);
            let ln = format!(
                r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill>{h}{t}</a:ln>"#,
                h = if head { r#"<a:headEnd type="triangle"/>"# } else { "" },
                t = if tail { r#"<a:tailEnd type="triangle"/>"# } else { "" },
            );
            let wsp = format!(
                concat!(
                    r#"<wps:wsp><wps:cNvSpPr/><wps:spPr>"#,
                    r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                    r#"<a:prstGeom prst="{prst}"><a:avLst/></a:prstGeom><a:noFill/>{ln}"#,
                    r#"</wps:spPr><wps:bodyPr/></wps:wsp>"#
                ),
                cx = cx,
                cy = cy,
                prst = prst,
                ln = ln,
            );
            // 线条的位置是两点的左上角，大小是两点的差
            let look = ShapeLook {
                extent_emu: Some((cx, cy)),
                pos_offset_emu: Some(PosOffset {
                    x: from.0.min(to.0),
                    y: from.1.min(to.1),
                    page: false,
                }),
                wrap: Some(ImageWrap::Front),
                ..Default::default()
            };
            // 线条没有 VML 孪生：TS 的 `buildLineParagraphXml` 也只发 Choice
            wrap_paragraph(
                flavor,
                &wp,
                &wp_decl,
                &anchor_open(&wp, cx, cy, &look, id, "Line"),
                &wsp,
                None,
            )
        }
        _ => return Err(Error::edit(DiagCode::EditPlanInvalid, "不是形状块")),
    };
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let mut frags = parse_fragment(dom, &xml)
        .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("形状段落解析失败: {e}")))?;
    let mut para =
        frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "形状段落为空"))?;
    fill_txbx(&mut para, &inner);
    Ok(para)
}
fn space_paragraph() -> NewBlock {
    text_paragraph(" ")
}
fn text_paragraph(text: &str) -> NewBlock {
    NewBlock::Paragraph { props: None, inlines: vec![NewInline::Run(NewRun::text(text))] }
}
/// 树里每一处空的 `w:txbxContent` 都填上同一份内容（Choice 一处、VML 孪生一处）。
fn fill_txbx(node: &mut NewElement, blocks: &[NewElement]) {
    if node.name == QName::new(NsId::W, LocalName::TxbxContent) {
        node.children.extend(blocks.iter().cloned().map(crate::xml::NewNode::Element));
        return;
    }
    for c in &mut node.children {
        if let crate::xml::NewNode::Element(e) = c {
            fill_txbx(e, blocks);
        }
    }
}
/// `EDIT-03 SetTextboxContent`：一个 `w:txbxContent` 里的块整体换掉。`box_node` 可以是
/// `w:txbxContent` 自己，也可以是包着它的 `wps:wsp` / `wps:txbx` / `v:shape` / `v:textbox`。
/// 落在 `mc:Fallback` 里 → `EDIT_TARGET_FALLBACK`（守卫在 `ops::run`），孪生由它同步。
pub fn set_textbox_content(
    s: &mut EditSession,
    box_node: NodeId,
    blocks: Vec<NewBlock>,
    ctx: &EditContext,
) -> Result<MutationResult> {
    let blocks = EditSession::materialize_all(s, blocks)?;
    let part = s.main_part();
    let dom = s.dom();
    if (box_node.0 as usize) >= dom.node_count() || dom.node(box_node).dirty == Dirty::Deleted {
        return Err(Error::edit(DiagCode::EditBadPosition, "文本框节点不存在"));
    }
    let txbx = QName::new(NsId::W, LocalName::TxbxContent);
    let container = if dom.is(box_node, txbx) {
        box_node
    } else {
        dom.descendants(box_node)
            .find(|&n| dom.node(n).dirty != Dirty::Deleted && dom.is(n, txbx))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "这里没有 w:txbxContent"))?
    };
    let mut tracker = Tracker::new(s.document(), ctx);
    let mut plan = MutationPlan::new(part);
    plan.structure_changed = true;
    for c in dom.children(container).iter().copied() {
        if dom.node(c).dirty == Dirty::Deleted || dom.element(c).is_none() {
            continue;
        }
        match &mut tracker {
            Some(t) => MutationPlan::plan_delete_block_tracked(&mut plan, dom, t, c),
            None => plan.node_edits.push(NodeEdit::Delete(c)),
        }
    }
    for block in blocks {
        let opaque = matches!(block, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
        let node = MutationPlan::new_block_element(dom, block);
        let node = match &mut tracker {
            Some(t) => t.mark_new_block_inserted(node, opaque),
            None => node,
        };
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: None,
            node,
        });
    }
    s.commit_plan(plan)
}

// 原 table_ops.rs
// 表格的行列结构操作（`EDIT-03` 表格段，任务 3.8）。
//
// **几何以声明网格为准**：列数 = `tblGrid/gridCol` 个数，行宽 = `gridBefore + Σ gridSpan + gridAfter`。
// 任一行的宽度与列数不符时，列操作与 `MergeCells` 直接 `Err(EDIT_TABLE_GRID_INCONSISTENT)`——**不**
// 偷偷修网格，修不修由调用方决定（`spec/14` 风险 3）。折叠、校正一律不做：那是 `resolve` 的视图。

fn geometry_error(msg: impl Into<String>) -> Error {
    Error::edit(DiagCode::EditTableGeometry, msg)
}
/// 合并区里某一行涉及的格：(`w:tc` 节点, 起始列, 跨度)。
type RegionCells = Vec<(NodeId, u32, u32)>;
/// 一行在声明网格上的布局。
#[derive(Debug, Clone)]
pub struct RowGeometry {
    pub node: NodeId,
    /// `gridBefore` / `gridAfter`。
    pub before: u32,
    pub after: u32,
    /// 每个物理格：(`w:tc` 节点, 起始列, 跨度)。
    pub cells: Vec<(NodeId, u32, u32)>,
}
impl RowGeometry {
    /// 这一行占的网格列数。
    pub fn width(&self) -> u32 {
        self.before + self.cells.iter().map(|c| c.2).sum::<u32>() + self.after
    }

    /// 覆盖第 `col` 列的格在 `cells` 里的下标。
    fn cell_at(&self, col: u32) -> Option<usize> {
        self.cells.iter().position(|&(_, start, span)| col >= start && col < start + span)
    }
}
/// 整张表在声明网格上的布局。
#[derive(Debug, Clone)]
pub struct Geometry {
    pub cols: u32,
    pub rows: Vec<RowGeometry>,
    /// `tblGrid/gridCol` 节点与声明宽度。
    pub grid: Vec<(NodeId, i32)>,
}
fn u32_of(v: &Option<Val<i32>>) -> u32 {
    v.as_ref().and_then(Val::value).copied().unwrap_or(0).max(0) as u32
}
pub fn geometry(table: &TableBlock) -> Geometry {
    let rows = table
        .rows
        .iter()
        .map(|r: &Row| {
            let before = u32_of(&r.props.grid_before);
            let mut col = before;
            let cells = r
                .cells
                .iter()
                .map(|c: &Cell| {
                    let span = c.grid_span();
                    let at = col;
                    col += span;
                    (c.node, at, span)
                })
                .collect();
            RowGeometry { node: r.node, before, after: u32_of(&r.props.grid_after), cells }
        })
        .collect();
    Geometry {
        cols: table.grid.len() as u32,
        rows,
        grid: table
            .grid
            .iter()
            .map(|g| (g.node, g.w.as_ref().and_then(Val::value).copied().unwrap_or(0)))
            .collect(),
    }
}
impl Geometry {
    /// 每行的网格宽度都等于列数（列操作的前提）。
    fn consistent(&self) -> bool {
        self.cols > 0 && self.rows.iter().all(|r| r.width() == self.cols)
    }

    pub(super) fn require_consistent(&self) -> Result<()> {
        if self.consistent() {
            return Ok(());
        }
        Err(Error::edit(
            DiagCode::EditTableGridInconsistent,
            format!(
                "表格网格不一致：tblGrid {} 列，各行 {:?}",
                self.cols,
                self.rows.iter().map(RowGeometry::width).collect::<Vec<_>>()
            ),
        ))
    }
}
impl EditSession {
    #[inline]
    /// 会话里的表格块（含嵌套表）。
    fn table_of(&self, node: NodeId) -> Result<&TableBlock> {
        let s = self;

        s.document()
            .tables()
            .find(|t| t.node == node)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "目标不是表格"))
    }
}

/// 空 `w:p`；`clone_ppr` 是要克隆 `w:pPr` 的来源段落。
fn empty_paragraph(dom: &Dom, plan: &mut MutationPlan, parent: Target, clone_ppr: Option<NodeId>) {
    let k = plan.node_edits.len();
    plan.node_edits.push(NodeEdit::Insert {
        parent,
        before: None,
        node: NewElement::new(QName::w(LocalName::P)),
    });
    if let Some(src) =
        clone_ppr.and_then(|p| Dom::live_children_named(dom, p, QName::w(LocalName::PPr)).next())
    {
        plan.node_edits.push(NodeEdit::InsertClone {
            parent: Target::New(k),
            before: None,
            source: src,
        });
    }
}
// ---- InsertRow / DeleteRow ---------------------------------------------------------------------
impl EditSession {
    #[inline]
    fn insert_row(
        &mut self,
        table: NodeId,
        at: u32,
        template: Option<NodeId>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;

        let t = EditSession::table_of(s, table)?;
        let rows: Vec<NodeId> = t.rows.iter().map(|r| r.node).collect();
        if at as usize > rows.len() {
            return Err(geometry_error(format!("行号 {at} 超出 {} 行", rows.len())));
        }
        // 模板：给定的行，否则 at 的前一行（at == 0 时取第一行）
        let tpl_idx = match template {
            Some(n) => rows
                .iter()
                .position(|&r| r == n)
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "模板行不属于这张表"))?,
            None if rows.is_empty() => {
                return Err(geometry_error("空表格没有可用的模板行"));
            }
            None => (at as usize).saturating_sub(1).min(rows.len() - 1),
        };
        let tpl = &t.rows[tpl_idx];
        // `vMerge`：模板是 continue → 新行不带；模板是 restart 且新行插在它与它的 continue 之间 → 新行是 continue
        let next_is_continue = |cell_idx: usize| {
            t.rows
                .get(at as usize)
                .and_then(|r| r.cells.get(cell_idx))
                .is_some_and(Cell::is_vmerge_continue)
        };
        let cells: Vec<(NodeId, Option<NodeId>, Option<NodeId>, VMergeFix)> = tpl
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let fix = if c.is_vmerge_continue() {
                    VMergeFix::Drop
                } else if c.props.v_merge.is_some() && next_is_continue(i) {
                    VMergeFix::Continue
                } else {
                    VMergeFix::Keep
                };
                let dom = s.dom();
                let tc_pr = Dom::live_children_named(dom, c.node, QName::w(LocalName::TcPr)).next();
                let first_para = c
                    .blocks
                    .first()
                    .map(|b| b.node())
                    .filter(|&n| dom.is(n, QName::w(LocalName::P)));
                (c.node, tc_pr, first_para, fix)
            })
            .collect();
        let tr_pr = Dom::live_children_named(s.dom(), tpl.node, QName::w(LocalName::TrPr)).next();
        let tbl_pr_ex =
            Dom::live_children_named(s.dom(), tpl.node, QName::w(LocalName::TblPrEx)).next();
        let before =
            rows.get(at as usize).and_then(|&r| Dom::direct_child_containing(s.dom(), table, r));

        let mut tracker = Tracker::new(s.document(), ctx);
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;
        let row_k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(table),
            before,
            node: NewElement::new(QName::w(LocalName::Tr)),
        });
        if let Some(src) = tbl_pr_ex {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(row_k),
                before: None,
                source: src,
            });
        }
        // 追踪：整行插入 → `trPr/w:ins`（`spec/08`）。`trPr` 是模板行的字节克隆，标记加在克隆之后。
        // 模板行**自己的**修订标记不跟着走：新行是这次插进来的，不是模板那次被删 / 被改的
        // （`TEST-07` 在「先追踪删一行、再照它插一行」上抓到过：`w:ins` 落在 `w:del` 后面，
        // `PROP-05` 的顺序自检当场拦下）
        let src_revs = tr_pr.is_some_and(|src| row_revision_marks(s.dom(), src).next().is_some());
        match (&mut tracker, tr_pr) {
            (None, Some(src)) if !src_revs => plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(row_k),
                before: None,
                source: src,
            }),
            (None, Some(src)) => {
                plan.clone_row_props_without_revisions(s.dom(), row_k, src);
            }
            (None, None) => {}
            (Some(t), Some(src)) => {
                let k = plan.node_edits.len();
                if src_revs {
                    plan.clone_row_props_without_revisions(s.dom(), row_k, src);
                } else {
                    plan.node_edits.push(NodeEdit::InsertClone {
                        parent: Target::New(row_k),
                        before: None,
                        source: src,
                    });
                }
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: t.marker(LocalName::Ins),
                });
            }
            (Some(t), None) => {
                let marker = t.marker(LocalName::Ins);
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(row_k),
                    before: None,
                    node: NewElement::new(QName::w(LocalName::TrPr)).with_child(marker),
                });
            }
        }
        let dom = s.dom();
        for (cell_node, tc_pr, first_para, fix) in cells {
            let cell_k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(row_k),
                before: None,
                node: NewElement::new(QName::w(LocalName::Tc)),
            });
            match (fix, tc_pr) {
                // 需要改 vMerge 时按模型重新生成 tcPr（克隆的子树没法就地改）
                (VMergeFix::Drop | VMergeFix::Continue, _) => {
                    let mut props =
                        crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
                    props.v_merge = match fix {
                        VMergeFix::Continue => Some(Merge::cont()),
                        _ => None,
                    };
                    let node = crate::semantic::props::emit_cell_props(&props, s.flavor());
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::New(cell_k),
                        before: None,
                        node,
                    });
                }
                (VMergeFix::Keep, Some(src)) => plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::New(cell_k),
                    before: None,
                    source: src,
                }),
                (VMergeFix::Keep, None) => {}
            }
            let _ = cell_node;
            empty_paragraph(dom, &mut plan, Target::New(cell_k), first_para);
        }
        plan.touch(table);
        s.commit_plan(plan)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VMergeFix {
    Keep,
    /// 模板格是合并区的非首格：新行该格不带 `vMerge`。
    Drop,
    /// 新行插进了合并区中间：该格是 continue。
    Continue,
}
impl EditSession {
    #[inline]
    fn delete_row(&mut self, table: NodeId, at: u32, ctx: &EditContext) -> Result<MutationResult> {
        let s = self;

        let t = EditSession::table_of(s, table)?;
        let row = t
            .rows
            .get(at as usize)
            .ok_or_else(|| geometry_error(format!("行号 {at} 超出 {} 行", t.rows.len())))?;
        let geo = geometry(t);
        let node = row.node;
        // 追踪：**行留着**，只加 `trPr/w:del`（`spec/08`）。vMerge 的提升不做——内容一点没变
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let dom = s.dom();
            let mut plan = MutationPlan::new(s.main_part());
            plan.touch(table);
            let before = Dom::live_element_children(dom, node)
                .find(|&c| !dom.is(c, QName::w(LocalName::TblPrEx)));
            t.container_mark(
                &mut plan,
                dom,
                MarkSite { owner: node, container: LocalName::TrPr, container_before: before },
                LocalName::Del,
                crate::semantic::props::order_index_row_props,
            );
            return s.commit_plan(plan);
        }
        // 被删行里 vMerge restart 的格：把下一行同列的 continue 提升为 restart
        let mut promote: Vec<NodeId> = Vec::new();
        if let (Some(this), Some(next)) = (geo.rows.get(at as usize), geo.rows.get(at as usize + 1))
        {
            for (i, &(_, col, _)) in this.cells.iter().enumerate() {
                let cell = &t.rows[at as usize].cells[i];
                if !cell.props.v_merge.as_ref().is_some_and(Merge::is_restart) {
                    continue;
                }
                if let Some(j) = next.cell_at(col)
                    && t.rows[at as usize + 1].cells[j].is_vmerge_continue()
                {
                    promote.push(next.cells[j].0);
                }
            }
        }
        let dom = s.dom();
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Delete(node));
        for tc in promote {
            let tc_pr = Dom::live_children_named(dom, tc, QName::w(LocalName::TcPr)).next();
            let patch =
                CellPropsPatch { v_merge: Change::Set(Merge::restart()), ..Default::default() };
            let before = dom.live_element_children(tc).next();
            plan_apply_cell_props_at(
                dom,
                Target::Node(tc),
                tc_pr,
                before,
                &patch,
                s.flavor(),
                &mut plan.node_edits,
            );
        }
        plan.touch(table);
        s.commit_plan(plan)
    }
}
// ---- InsertColumn / DeleteColumn ----------------------------------------------------------------
impl EditSession {
    #[inline]
    /// 行属性里的 `gridBefore` / `gridAfter` 增减。
    fn bump_row_gap(
        &self,
        row: &RowGeometry,
        before: Option<u32>,
        after: Option<u32>,
        plan: &mut MutationPlan,
    ) {
        let s = self;

        let dom = s.dom();
        let patch = RowPropsPatch {
            grid_before: before.map_or(Change::Keep, |v| {
                if v == 0 { Change::Unset } else { Change::Set(Val::Value(v as i32)) }
            }),
            grid_after: after.map_or(Change::Keep, |v| {
                if v == 0 { Change::Unset } else { Change::Set(Val::Value(v as i32)) }
            }),
            ..Default::default()
        };
        let tr_pr = Dom::live_children_named(dom, row.node, QName::w(LocalName::TrPr)).next();
        let anchor = Dom::live_element_children(dom, row.node)
            .find(|&c| !dom.is(c, QName::w(LocalName::TblPrEx)));
        plan_apply_row_props_at(
            dom,
            Target::Node(row.node),
            tr_pr,
            anchor,
            &patch,
            s.flavor(),
            &mut plan.node_edits,
        );
    }
}
impl EditSession {
    #[inline]
    /// 一个格的 `gridSpan` / `tcW` 调整。
    fn patch_cell_span(
        &self,
        cell: NodeId,
        span: Option<u32>,
        width_delta: i32,
        plan: &mut MutationPlan,
    ) {
        let s = self;

        let dom = s.dom();
        let tc_pr = Dom::live_children_named(dom, cell, QName::w(LocalName::TcPr)).next();
        let current = crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
        let mut patch = CellPropsPatch::default();
        if let Some(span) = span {
            patch.grid_span =
                if span <= 1 { Change::Unset } else { Change::Set(Val::Value(span as i32)) };
        }
        if width_delta != 0
            && let Some(old) = current.width.as_ref().and_then(TblWidth::twips)
        {
            patch.width = Change::Set(TblWidth::dxa((old + width_delta).max(0)));
        }
        let before = dom.live_element_children(cell).next();
        plan_apply_cell_props_at(
            dom,
            Target::Node(cell),
            tc_pr,
            before,
            &patch,
            s.flavor(),
            &mut plan.node_edits,
        );
    }
}
impl EditSession {
    #[inline]
    /// 书签 / 权限范围的 `w:colFirst` / `w:colLast` 随列增删移动（`SPAN-03`）。
    fn shift_bookmark_columns(&mut self, at: u32, delta: i32, plan: &mut MutationPlan) {
        let s = self;

        let part = s.main_part();
        let Ok(index) = s.spans_of(part) else { return };
        let mut edits: Vec<(NodeId, LocalName, Option<u32>)> = Vec::new();
        for span in index.spans() {
            let cols = match &span.kind {
                RangeKind::Bookmark { cols: Some(c), .. }
                | RangeKind::Permission { cols: Some(c), .. } => *c,
                _ => continue,
            };
            let Some(start) = span.start.as_ref().and_then(|a| a.marker) else { continue };
            let shift = |v: u32| -> Option<u32> {
                if delta > 0 {
                    Some(if v >= at { v + 1 } else { v })
                } else if v > at {
                    Some(v - 1)
                } else if v == at {
                    None // 该列被删：区间收缩由下面的两端一起决定
                } else {
                    Some(v)
                }
            };
            let (first, last) = (shift(cols.0), shift(cols.1));
            let new = match (first, last) {
                (Some(a), Some(b)) if a <= b => (a, b),
                // 起点落在被删列上：区间从下一列开始
                (None, Some(b)) => (at.min(b), b),
                (Some(a), None) => (a, a),
                _ => continue,
            };
            if new != cols {
                edits.push((start, LocalName::ColFirst, Some(new.0)));
                edits.push((start, LocalName::ColLast, Some(new.1)));
            }
        }
        for (node, name, value) in edits {
            if let Some(v) = value {
                plan.node_edits.push(NodeEdit::SetAttr {
                    node: Target::Node(node),
                    name: QName::w(name),
                    value: v.to_string(),
                });
            }
        }
    }
}
impl EditSession {
    #[inline]
    fn insert_column(
        &mut self,
        table: NodeId,
        at: u32,
        width: i32,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;

        let t = EditSession::table_of(s, table)?;
        let geo = geometry(t);
        geo.require_consistent()?;
        if at > geo.cols {
            return Err(geometry_error(format!("列号 {at} 超出 {} 列", geo.cols)));
        }
        // 每行的落点：拿到需要克隆的模板（左邻格，行首取右邻）
        enum Where {
            Gap { before: Option<u32>, after: Option<u32> },
            Widen(NodeId, u32),
            NewCell { before: Option<NodeId>, template: Option<(NodeId, Option<NodeId>)> },
        }
        let dom = s.dom();
        let mut plans: Vec<(usize, Where)> = Vec::new();
        for (ri, row) in geo.rows.iter().enumerate() {
            let end_of_cells = row.before + row.cells.iter().map(|c| c.2).sum::<u32>();
            let place = if at < row.before {
                Where::Gap { before: Some(row.before + 1), after: None }
            } else if at >= end_of_cells && row.after > 0 {
                Where::Gap { before: None, after: Some(row.after + 1) }
            } else {
                match row.cells.iter().position(|&(_, start, span)| at > start && at < start + span)
                {
                    // 落在某个跨列格中间 → 加宽
                    Some(i) => Where::Widen(row.cells[i].0, row.cells[i].2 + 1),
                    None => {
                        let idx = row.cells.iter().position(|&(_, start, _)| start == at);
                        let template_idx = match idx {
                            Some(0) => row.cells.first(),
                            Some(i) => row.cells.get(i - 1),
                            None => row.cells.last(),
                        };
                        Where::NewCell {
                            before: idx.and_then(|i| {
                                Dom::direct_child_containing(dom, row.node, row.cells[i].0)
                            }),
                            template: template_idx.map(|&(n, _, _)| {
                                let tc_pr =
                                    Dom::live_children_named(dom, n, QName::w(LocalName::TcPr))
                                        .next();
                                let para = Dom::live_element_children(dom, n)
                                    .find(|&c| dom.is(c, QName::w(LocalName::P)));
                                (n, para.and(tc_pr))
                            }),
                        }
                    }
                }
            };
            plans.push((ri, place));
        }

        let mut tracker = Tracker::new(s.document(), ctx);
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;
        // tblGrid
        let grid_parent = Dom::live_children_named(dom, table, QName::w(LocalName::TblGrid)).next();
        match grid_parent {
            Some(g) => {
                // 追踪：网格要变，先把**旧**网格快照进 `w:tblGridChange`（这两条编辑排在新
                // `w:gridCol` 之前，克隆到的就是原来的列）
                if let Some(t) = &mut tracker {
                    t.snapshot(
                        &mut plan,
                        dom,
                        g,
                        LocalName::TblGridChange,
                        LocalName::TblGrid,
                        &[],
                    );
                }
                let before = geo.grid.get(at as usize).map(|&(n, _)| n);
                let mut col = NewElement::new(QName::w(LocalName::GridCol));
                col.push_attr(QName::w(LocalName::W), width.to_string());
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(g),
                    before,
                    node: col,
                });
            }
            None => return Err(geometry_error("表格没有 tblGrid")),
        }
        for (ri, place) in plans {
            let row = &geo.rows[ri];
            match place {
                Where::Gap { before, after } => {
                    // 追踪：`gridBefore` / `gridAfter` 是行属性 → 旧值进 `w:trPrChange`
                    if let Some(t) = &mut tracker
                        && let Some(trpr) =
                            Dom::live_children_named(dom, row.node, QName::w(LocalName::TrPr))
                                .next()
                    {
                        t.snapshot(
                            &mut plan,
                            dom,
                            trpr,
                            LocalName::TrPrChange,
                            LocalName::TrPr,
                            &[LocalName::Ins, LocalName::Del],
                        );
                    }
                    EditSession::bump_row_gap(s, row, before, after, &mut plan)
                }
                Where::Widen(cell, span) => {
                    // 追踪：加宽跨列格改的是 `gridSpan` / `tcW` → 旧值进 `w:tcPrChange`
                    if let Some(t) = &mut tracker
                        && let Some(tcpr) =
                            Dom::live_children_named(dom, cell, QName::w(LocalName::TcPr)).next()
                    {
                        t.snapshot(
                            &mut plan,
                            dom,
                            tcpr,
                            LocalName::TcPrChange,
                            LocalName::TcPr,
                            &[LocalName::CellIns, LocalName::CellDel, LocalName::CellMerge],
                        );
                    }
                    EditSession::patch_cell_span(s, cell, Some(span), width, &mut plan)
                }
                Where::NewCell { before, template } => {
                    let cell_k = plan.node_edits.len();
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(row.node),
                        before,
                        node: NewElement::new(QName::w(LocalName::Tc)),
                    });
                    // 新格的 tcPr：克隆模板但去掉 gridSpan / vMerge，宽度换成新列宽
                    // 新格的 `tcPr`：克隆模板但去掉 gridSpan / vMerge，宽度换成新列宽。
                    // 追踪时 `w:cellIns` 要放进**同一个** `w:tcPr`（两个 `w:tcPr` 不合法）
                    let props = template.map(|(_, tc_pr)| {
                        let mut props =
                            crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new());
                        props.grid_span = None;
                        props.v_merge = None;
                        props.h_merge = None;
                        if props.width.is_some() {
                            props.width = Some(TblWidth::dxa(width));
                        }
                        props
                    });
                    let want_container =
                        tracker.is_some() || props.as_ref().is_some_and(|p| !props_is_empty(p));
                    if want_container {
                        let mut node = match &props {
                            Some(p) => crate::semantic::props::emit_cell_props(p, s.flavor()),
                            None => NewElement::new(QName::w(LocalName::TcPr)),
                        };
                        if let Some(t) = &mut tracker {
                            insert_ordered(
                                &mut node,
                                t.marker(LocalName::CellIns),
                                crate::semantic::props::order_index_cell_props,
                            );
                        }
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::New(cell_k),
                            before: None,
                            node,
                        });
                    }
                    let para = template.and_then(|(tpl, _)| {
                        Dom::live_element_children(dom, tpl)
                            .find(|&c| dom.is(c, QName::w(LocalName::P)))
                    });
                    empty_paragraph(dom, &mut plan, Target::New(cell_k), para);
                }
            }
        }
        EditSession::shift_bookmark_columns(s, at, 1, &mut plan);
        plan.touch(table);
        s.commit_plan(plan)
    }
}
fn props_is_empty(p: &crate::semantic::props::CellProps) -> bool {
    crate::semantic::props::diff_cell_props(&Default::default(), p)
        == crate::semantic::props::CellPropsPatch::default()
}
impl EditSession {
    #[inline]
    fn delete_column(
        &mut self,
        table: NodeId,
        at: u32,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;

        let t = EditSession::table_of(s, table)?;
        let geo = geometry(t);
        geo.require_consistent()?;
        if at >= geo.cols {
            return Err(geometry_error(format!("列号 {at} 超出 {} 列", geo.cols)));
        }
        // 追踪：**格与网格都留着**，只给这一列的格加 `tcPr/w:cellDel`（`spec/08`）。
        // 网格不动，所以**不发** `w:tblGridChange`——收缩发生在接受修订时（7.4），
        // 那时才知道最终的列宽（登记在 `docs/04` §8）
        if let Some(mut tr) = Tracker::new(s.document(), ctx) {
            let dom = s.dom();
            let mut plan = MutationPlan::new(s.main_part());
            plan.touch(table);
            for row in &geo.rows {
                let Some(i) = row.cell_at(at) else { continue };
                let cell = row.cells[i].0;
                let before = dom.live_element_children(cell).next();
                tr.container_mark(
                    &mut plan,
                    dom,
                    MarkSite { owner: cell, container: LocalName::TcPr, container_before: before },
                    LocalName::CellDel,
                    crate::semantic::props::order_index_cell_props,
                );
            }
            return s.commit_plan(plan);
        }
        let width = geo.grid.get(at as usize).map_or(0, |&(_, w)| w);
        // 先看会不会把某一行掏空
        for row in &geo.rows {
            if row.cells.len() == 1
                && let Some(i) = row.cell_at(at)
                && row.cells[i].2 == 1
            {
                return Err(geometry_error(
                    "删掉这一列会让某一行没有单元格；请改用 DeleteBlock 删整表",
                ));
            }
        }
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;
        if let Some(&(node, _)) = geo.grid.get(at as usize) {
            plan.node_edits.push(NodeEdit::Delete(node));
        }
        for row in &geo.rows {
            let end_of_cells = row.before + row.cells.iter().map(|c| c.2).sum::<u32>();
            if at < row.before {
                EditSession::bump_row_gap(s, row, Some(row.before - 1), None, &mut plan);
            } else if at >= end_of_cells {
                if row.after > 0 {
                    EditSession::bump_row_gap(s, row, None, Some(row.after - 1), &mut plan);
                }
            } else if let Some(i) = row.cell_at(at) {
                let (node, _, span) = row.cells[i];
                if span == 1 {
                    plan.node_edits.push(NodeEdit::Delete(node));
                } else {
                    EditSession::patch_cell_span(s, node, Some(span - 1), -width, &mut plan);
                }
            }
        }
        EditSession::shift_bookmark_columns(s, at, -1, &mut plan);
        plan.touch(table);
        s.commit_plan(plan)
    }
}
/// 一个 `w:tc` 在网格里的 `(起始列, 跨度)`（7.4 的格接受 / 拒绝用）。
impl EditSession {
    #[inline]
    fn cell_column(&self, table: NodeId, cell: NodeId) -> Option<(u32, u32)> {
        let table = EditSession::table_of(self, table).ok()?;
        table.rows.iter().find_map(|row| {
            RowGeometry::cell_spans(row)
                .find(|&(node, _, _)| node == cell)
                .map(|(_, start, span)| (start, span))
        })
    }
    /// 一个格被单独删掉时，把它的网格跨度与宽度并进同行的邻格（左邻优先，行首取右邻），
    /// 保住「一行的网格宽度 = `tblGrid` 列数」（`MOD-07` / `SAVE-02` 的 `SAVE_TABLE_GRID`）。
    /// 7.4 拒绝 `cellIns` / 接受 `cellDel` 时，只有一行少一个格的情况走这里。
    #[inline]
    fn absorb_cell_width(&self, table: NodeId, cell: NodeId, plan: &mut MutationPlan) {
        let s = self;
        let Ok(t) = EditSession::table_of(s, table) else { return };
        let geo = geometry(t);
        let Some(row) = geo.rows.iter().find(|r| r.cells.iter().any(|&(n, _, _)| n == cell)) else {
            return;
        };
        let i = row.cells.iter().position(|&(n, _, _)| n == cell).expect("checked above");
        let pick = if i > 0 { row.cells.get(i - 1) } else { row.cells.get(i + 1) };
        let Some(&(neighbour, _, nspan)) = pick else { return };
        let width = {
            let dom = s.dom();
            let tc_pr = Dom::live_children_named(dom, cell, QName::w(LocalName::TcPr)).next();
            crate::semantic::props::read_cell_props(dom, tc_pr, &mut Vec::new())
                .width
                .as_ref()
                .and_then(TblWidth::twips)
                .unwrap_or(0)
        };
        let span = row.cells[i].2;
        EditSession::patch_cell_span(s, neighbour, Some(nspan + span), width, plan);
    }
    /// 每一行在第 `col` 列上的那个 `w:tc`（`(行节点, 格节点)`）。
    #[inline]
    fn column_cells(&self, table: NodeId, col: u32) -> impl Iterator<Item = (NodeId, NodeId)> + '_ {
        EditSession::table_of(self, table).ok().into_iter().flat_map(move |table| {
            table.rows.iter().filter_map(move |row| {
                RowGeometry::cell_spans(row)
                    .find(|&(_, start, span)| col >= start && col < start + span)
                    .map(|(cell, _, _)| (row.node, cell))
            })
        })
    }
    /// `w:tblGrid` 的 `w:gridCol` 节点，按列序。
    #[inline]
    fn grid_cols(&self, table: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        EditSession::table_of(self, table)
            .ok()
            .into_iter()
            .flat_map(|table| table.grid.iter().map(|g| g.node))
    }
}

impl RowGeometry {
    /// 根据原模型声明计算列位置；借用 owner，不构造整张几何表。
    #[inline]
    fn cell_spans(row: &Row) -> impl Iterator<Item = (NodeId, u32, u32)> + '_ {
        row.cells.iter().scan(u32_of(&row.props.grid_before), |col, cell| {
            let start = *col;
            let span = cell.grid_span();
            *col += span;
            Some((cell.node, start, span))
        })
    }
}
// ---- MergeCells ---------------------------------------------------------------------------------
impl EditSession {
    #[inline]
    fn merge_cells(
        &mut self,
        table: NodeId,
        from: (u32, u32),
        to: (u32, u32),
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;

        // 第一阶段：追踪时拒绝（`spec/18`「不在 M7」）。`w:cellMerge` + `vMergeOrig` 的形态要等
        // 真实 Word 的 fixture 校准；顺带一条已知的 Word 行为：Word 的「拒绝所有修订」**不**撤销
        // 单元格合并（`fixtures/revisions/table-and-move/rejected.docx` 首行仍是合并的一格）
        if ctx.track_changes.is_some() {
            return Err(Error::edit(
                DiagCode::EditUnsupportedTrackedMerge,
                "track_changes 开启时不支持 MergeCells（w:cellMerge 的形态待真实 Word 校准）",
            ));
        }
        let t = EditSession::table_of(s, table)?;
        let geo = geometry(t);
        geo.require_consistent()?;
        let (r0, c0) = from;
        let (r1, c1) = to;
        if r0 > r1 || c0 > c1 || r1 as usize >= geo.rows.len() || c1 >= geo.cols {
            return Err(geometry_error(format!("合并区 {from:?}..={to:?} 越界或方向反了")));
        }
        if r0 == r1 && c0 == c1 {
            return s.commit_plan(MutationPlan::new(s.main_part())); // 单格，无事可做
        }
        // 每行的合并区必须正好由整格组成，且不能与既有的纵向合并区交叠
        let mut regions: Vec<(usize, RegionCells)> = Vec::new();
        for r in r0..=r1 {
            let row = &geo.rows[r as usize];
            let inside: RegionCells = row
                .cells
                .iter()
                .copied()
                .filter(|&(_, start, span)| start + span > c0 && start < c1 + 1)
                .collect();
            let Some(&(_, first_start, _)) = inside.first() else {
                return Err(geometry_error(format!("第 {r} 行在合并区里没有单元格")));
            };
            let (_, last_start, last_span) = *inside.last().expect("non-empty");
            if first_start != c0 || last_start + last_span != c1 + 1 {
                return Err(geometry_error(format!(
                    "第 {r} 行的单元格边界与合并区不齐（{first_start}..{} vs {c0}..{}）",
                    last_start + last_span,
                    c1 + 1
                )));
            }
            // 区内的格不能是别的合并区的延续
            for (i, &(node, _, _)) in inside.iter().enumerate() {
                let cell = t.rows[r as usize]
                    .cells
                    .iter()
                    .find(|c| c.node == node)
                    .expect("geometry 与模型同源");
                if cell.is_vmerge_continue() && !(r > r0 && i == 0) {
                    return Err(geometry_error("合并区与既有的纵向合并交叠"));
                }
            }
            regions.push((r as usize, inside));
        }
        let vertical = r1 > r0;
        let span = c1 - c0 + 1;
        let dom = s.dom();
        let top = regions[0].1[0].0;
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;

        for (idx, (_, inside)) in regions.iter().enumerate() {
            let keeper = inside[0].0;
            // ① 先改属性：`plan_apply_*` 的插入锚点是改动前的第一个子元素，内容一搬走它就不在了，
            //    所以属性编辑必须排在搬移之前；跨度与 vMerge 合成一个 patch，免得插出两个 tcPr
            let mut patch = CellPropsPatch::default();
            if span > 1 {
                patch.grid_span = Change::Set(Val::Value(span as i32));
                // 合并后的宽度是区内各格宽度之和（都声明了 dxa 才算）
                let widths: Option<i32> = inside
                    .iter()
                    .map(|&(n, _, _)| {
                        let pr = Dom::live_children_named(dom, n, QName::w(LocalName::TcPr)).next();
                        crate::semantic::props::read_cell_props(dom, pr, &mut Vec::new())
                            .width
                            .as_ref()
                            .and_then(TblWidth::twips)
                    })
                    .sum();
                if let Some(total) = widths {
                    patch.width = Change::Set(TblWidth::dxa(total));
                }
            }
            if vertical {
                patch.v_merge =
                    Change::Set(if idx == 0 { Merge::restart() } else { Merge::cont() });
            }
            if patch != CellPropsPatch::default() {
                let tc_pr = Dom::live_children_named(dom, keeper, QName::w(LocalName::TcPr)).next();
                let before = dom.live_element_children(keeper).next();
                plan_apply_cell_props_at(
                    dom,
                    Target::Node(keeper),
                    tc_pr,
                    before,
                    &patch,
                    s.flavor(),
                    &mut plan.node_edits,
                );
            }
            // ② 再搬内容：纵向合并全都并到左上格，纯横向合并并到本行首格；按文档序
            let target = if vertical { top } else { keeper };
            for (j, &(node, _, _)) in inside.iter().enumerate() {
                if j == 0 && node == target {
                    continue;
                }
                move_cell_content(dom, node, target, &mut plan);
                if j > 0 {
                    plan.node_edits.push(NodeEdit::Delete(node));
                }
            }
            // ③ 被搬空的 continue 格留一个空段落
            if vertical && idx > 0 {
                empty_paragraph(dom, &mut plan, Target::Node(keeper), None);
            }
        }
        plan.touch(table);
        s.commit_plan(plan)
    }
}
/// 把 `from` 格里的内容块（`w:tcPr` 之外的元素）按文档序搬到 `into` 格末尾。
fn move_cell_content(dom: &Dom, from: NodeId, into: NodeId, plan: &mut MutationPlan) {
    for child in Dom::live_element_children(dom, from) {
        if dom.is(child, QName::w(LocalName::TcPr)) {
            continue;
        }
        // 末尾的空段落不搬（Word 合并后不会留下一串空行）
        if dom.is(child, QName::w(LocalName::P)) && crate::span::content_len(dom, child) == 0 {
            continue;
        }
        plan.node_edits.push(NodeEdit::Move {
            node: child,
            parent: Target::Node(into),
            before: None,
        });
    }
}
// ---- NewBlock::Table 生成器 ---------------------------------------------------------------------
/// `rows` × `cols` 的新表格（`EDIT-03 InsertBlock`）。
pub fn new_table(
    rows: u32,
    cols: u32,
    widths: Option<Vec<i32>>,
    style: Option<String>,
    header: bool,
) -> NewElement {
    const BODY_WIDTH: i32 = 9360;
    let cols = cols.max(1);
    let rows = rows.max(1);
    let widths = widths
        .filter(|v| v.len() == cols as usize)
        .unwrap_or_else(|| vec![BODY_WIDTH / cols as i32; cols as usize]);
    let mut tbl = NewElement::new(QName::w(LocalName::Tbl));

    let mut tbl_pr = NewElement::new(QName::w(LocalName::TblPr));
    if let Some(id) = style {
        let mut e = NewElement::new(QName::w(LocalName::TblStyle));
        e.push_attr(QName::w(LocalName::Val), id);
        tbl_pr.push_child(e);
    }
    let mut tbl_w = NewElement::new(QName::w(LocalName::TblW));
    tbl_w.push_attr(QName::w(LocalName::W), "0".to_string());
    tbl_w.push_attr(QName::w(LocalName::UType), "auto".to_string());
    tbl_pr.push_child(tbl_w);
    let mut look = NewElement::new(QName::w(LocalName::TblLook));
    look.push_attr(QName::w(LocalName::Val), "04A0".to_string());
    for (name, on) in [
        (LocalName::FirstRow, "1"),
        (LocalName::LastRow, "0"),
        (LocalName::FirstColumn, "1"),
        (LocalName::LastColumn, "0"),
        (LocalName::NoHBand, "0"),
        (LocalName::NoVBand, "1"),
    ] {
        look.push_attr(QName::w(name), on.to_string());
    }
    tbl_pr.push_child(look);
    tbl.push_child(tbl_pr);

    let mut grid = NewElement::new(QName::w(LocalName::TblGrid));
    for &width in &widths {
        let mut col = NewElement::new(QName::w(LocalName::GridCol));
        col.push_attr(QName::w(LocalName::W), width.max(1).to_string());
        grid.push_child(col);
    }
    tbl.push_child(grid);

    for r in 0..rows {
        let mut tr = NewElement::new(QName::w(LocalName::Tr));
        if header && r == 0 {
            let mut tr_pr = NewElement::new(QName::w(LocalName::TrPr));
            tr_pr.push_child(NewElement::new(QName::w(LocalName::TblHeader)));
            tr.push_child(tr_pr);
        }
        for &width in &widths {
            let mut tc = NewElement::new(QName::w(LocalName::Tc));
            let mut tc_pr = NewElement::new(QName::w(LocalName::TcPr));
            let mut tc_w = NewElement::new(QName::w(LocalName::TcW));
            tc_w.push_attr(QName::w(LocalName::W), width.max(1).to_string());
            tc_w.push_attr(QName::w(LocalName::UType), "dxa".to_string());
            tc_pr.push_child(tc_w);
            tc.push_child(tc_pr);
            tc.push_child(NewElement::new(QName::w(LocalName::P)));
            tr.push_child(tc);
        }
        tbl.push_child(tr);
    }
    tbl
}
/// 行属性里的修订标记（`trPr/w:ins` / `w:del` / `w:trPrChange`）。
fn row_revision_marks(dom: &Dom, tr_pr: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    Dom::live_element_children(dom, tr_pr).filter(move |&c| {
        [LocalName::Ins, LocalName::Del, LocalName::TrPrChange]
            .iter()
            .any(|&l| dom.is(c, QName::w(l)))
    })
}
impl MutationPlan {
    #[inline]
    /// 把模板行的 `trPr` 逐个子元素克隆过去，**跳过修订标记**。子元素各自还是字节克隆
    /// （`XML-12` 规则 F），只是那几个不跟着走。
    fn clone_row_props_without_revisions(&mut self, dom: &Dom, row_k: usize, src: NodeId) {
        let plan = self;

        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(row_k),
            before: None,
            node: NewElement::new(QName::w(LocalName::TrPr)),
        });
        let revs: Vec<NodeId> = row_revision_marks(dom, src).collect();
        for c in Dom::live_element_children(dom, src) {
            if revs.contains(&c) {
                continue;
            }
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k),
                before: None,
                source: c,
            });
        }
    }
}

// 原 track.rs
// 修订生成（`EDIT-03` 每个操作的「修订」行；`spec/18` 7.2 / 7.3）。
//
// **修订是计划的一部分，不是事后包装**（`spec/18` 分层决策 1）：包裹、改名、快照都是普通
// [`NodeEdit`]，由 [`Tracker`] 在 plan 阶段生成，`MutationPlan` 不加字段。`w:id` 从会话里全包
// 修订的最大值 + 1 起顺序发放（`EDIT-06`）；`w:date` 是 [`RevisionAuthor::date`] 的原串——
// 引擎里没有时钟（不变式 1 与可复现性）。
//
// **同作者规则按 Word**（分层决策 2，作者相等 = `w:author` 字符串相等，不看 `w:initials`）：
// 自己插的可以直接改、直接删；别人插的删了是 `w:ins` 里套 `w:del`；删除区里不能再打字。
fn track_w(local: LocalName) -> QName {
    QName::new(NsId::W, local)
}
/// `container_mark` 的落点：属性容器挂在谁下面、容器不存在时插在哪。
#[derive(Debug, Clone, Copy)]
pub struct MarkSite {
    /// `w:tr` / `w:tc` / `w:tbl`。
    pub owner: NodeId,
    /// `w:trPr` / `w:tcPr` / `w:tblPr`。
    pub container: LocalName,
    /// 容器不存在时插在这个兄弟之前（`None` = 追加到末尾）。
    pub container_before: Option<NodeId>,
}
/// 插入 / 删除位置外面罩着什么修订包裹（只看到段落为止）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSite {
    /// 没有包裹。
    Clean,
    /// 在**本作者**的 `w:ins` / `w:moveTo` 里（最内层那个）。
    OwnIns(NodeId),
    /// 在**别人**的 `w:ins` / `w:moveTo` 里。
    OtherIns(NodeId),
    /// 在 `w:del` / `w:moveFrom` 里。
    Deleted(NodeId),
}
/// 从 `node` 往上走到 `stop`（段落）为止，最内层的修订包裹是什么。
pub fn track_site_of(dom: &Dom, node: NodeId, stop: NodeId, author: &str) -> TrackSite {
    let mut x = Some(node);
    while let Some(n) = x {
        if n == stop {
            break;
        }
        if let Some(name) = dom.name(n)
            && name.ns == NsId::W
        {
            match name.local {
                LocalName::Del | LocalName::MoveFrom => return TrackSite::Deleted(n),
                LocalName::Ins | LocalName::MoveTo => {
                    let same = dom
                        .attr_value(n, track_w(LocalName::Author))
                        .is_some_and(|a| a.as_ref() == author);
                    return if same { TrackSite::OwnIns(n) } else { TrackSite::OtherIns(n) };
                }
                _ => {}
            }
        }
        x = dom.parent(n);
    }
    TrackSite::Clean
}
/// 落在删除区里不能再打字（Word 的规则）。
pub fn err_in_deleted() -> Error {
    Error::edit(DiagCode::EditInDeleted, "位置落在已删除的文字里，追踪时不能插入")
}
/// 计划阶段的修订生成器。
#[derive(Debug, Clone)]
pub struct Tracker {
    pub author: String,
    pub date: Option<String>,
    /// 下一个 `w:id`（`EDIT-06`：全包最大值 + 1 起）。
    next_id: u32,
}
impl Tracker {
    /// `track_changes` 开着才有；`w:id` 的起点来自 [`crate::model::Document::revisions`]
    /// 的全包最大值（任何 part 变脏后索引会重扫，所以每个阶段拿到的都是当前值）。
    pub fn new(doc: &crate::model::Document, ctx: &EditContext) -> Option<Tracker> {
        let RevisionAuthor { author, date } = ctx.track_changes.as_ref()?;
        Some(Tracker {
            author: author.clone(),
            date: date.clone(),
            next_id: doc.revisions.max_w_id().unwrap_or(0).saturating_add(1),
        })
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// 一个空的修订元素（`w:ins` / `w:del` / `w:moveFrom` / `w:moveTo` / `*PrChange` …），
    /// 带 `w:id` / `w:author` / `w:date`。
    pub fn marker(&mut self, local: LocalName) -> NewElement {
        let mut e = NewElement::new(track_w(local));
        e.push_attr(track_w(LocalName::Id), self.take_id().to_string());
        e.push_attr(track_w(LocalName::Author), self.author.clone());
        if let Some(d) = &self.date {
            e.push_attr(track_w(LocalName::Date), d.clone());
        }
        e
    }

    /// 与 `node` 同名同属性、但换一个新 `w:id` 的元素（拆开别人的 `w:ins` 时给右半用）。
    pub fn clone_marker(&mut self, dom: &Dom, node: NodeId) -> NewElement {
        let name = dom.name(node).expect("clone_marker on an element");
        let mut e = NewElement::new(name);
        e.push_attr(track_w(LocalName::Id), self.take_id().to_string());
        if let Some(el) = dom.element(node) {
            for a in &el.attrs {
                if a.name != track_w(LocalName::Id) {
                    e.push_attr(a.name, dom.attr_str(a).into_owned());
                }
            }
        }
        e
    }

    /// 把**一个**内容项原地包进新的 `local`（`w:ins` / `w:del`）里：包裹插在它原来的位置，
    /// 它自己搬进去。返回包裹在 `plan.node_edits` 里的下标。
    ///
    /// 一项一个包裹（Word 会把连着的几个 run 合成一个 `w:del`，我们不合并）：这样容器的
    /// **内容序列长度不变**，范围锚点一个都不用动——`plan.span.rewraps` 让 `SPAN-06`
    /// 的通用推导跳过这两条编辑。形态上多几个包裹，语义完全一样。
    pub fn wrap_item(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        node: NodeId,
        local: LocalName,
    ) -> Option<usize> {
        let parent = dom.parent(node)?;
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before: Some(node),
            node: self.marker(local),
        });
        plan.node_edits.push(NodeEdit::Move { node, parent: Target::New(k), before: None });
        plan.span.rewraps.push(node);
        Some(k)
    }

    /// 段落标记的插入 / 删除：`pPr/rPr` 里首位放 `w:ins` / `w:del`（`PROP-05`：`run.toml` 的
    /// `order` 把这四个放在最前）。`pPr` / `rPr` 缺就顺手建。
    pub fn para_mark(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        para: NodeId,
        local: LocalName,
    ) {
        let ppr = MutationPlan::ppr_of(dom, para);
        let rpr =
            ppr.and_then(|p| Dom::live_children_named(dom, p, track_w(LocalName::RPr)).next());
        // 已经有同种标记就不重复加
        if let Some(r) = rpr
            && Dom::live_children(dom, r).any(|c| dom.is(c, track_w(local)))
        {
            return;
        }
        let marker = self.marker(local);
        match (ppr, rpr) {
            (_, Some(r)) => {
                // `rPr` 已在：插到第一个 order 更靠后的子元素之前
                let before = Dom::live_children(dom, r).find(|&c| {
                    dom.name(c)
                        .and_then(order_index_run_props)
                        .is_none_or(|i| i > order_index_run_props(track_w(local)).unwrap_or(0))
                });
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(r),
                    before,
                    node: marker,
                });
            }
            (Some(p), None) => {
                // `pPr` 在但没有 `rPr`：`w:rPr` 是 `w:pPr` 的**最后**一个子元素（CT_PPr），
                // 只有 `w:sectPr` / `w:pPrChange` 排在它后面
                let before = Dom::live_children(dom, p).find(|&c| {
                    dom.is(c, track_w(LocalName::SectPr))
                        || dom.is(c, track_w(LocalName::PPrChange))
                });
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(p),
                    before,
                    node: NewElement::new(track_w(LocalName::RPr)).with_child(marker),
                });
            }
            (None, None) => {
                // 连 `pPr` 都没有：它是 `w:p` 的第一个子元素
                let before = Dom::live_children(dom, para).next();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(para),
                    before,
                    node: NewElement::new(track_w(LocalName::PPr))
                        .with_child(NewElement::new(track_w(LocalName::RPr)).with_child(marker)),
                });
            }
        }
    }

    /// 属性容器（`w:trPr` / `w:tcPr` / `w:tblPr`）里的标记：`w:ins` / `w:del` / `w:cellIns` /
    /// `w:cellDel`。容器缺就按 `container_before` 建；已有同种标记就什么都不做。
    ///
    /// `order` 是那张属性表生成的 `order_index_*`（`PROP-05`）：新标记插在第一个 order 更靠后的
    /// 子元素之前。
    pub fn container_mark(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        at: MarkSite,
        mark: LocalName,
        order: fn(QName) -> Option<u16>,
    ) {
        let MarkSite { owner, container, container_before } = at;
        let existing = Dom::live_children_named(dom, owner, track_w(container)).next();
        if let Some(c) = existing
            && Dom::live_children(dom, c).any(|x| dom.is(x, track_w(mark)))
        {
            return;
        }
        let marker = self.marker(mark);
        match existing {
            Some(c) => {
                let mine = order(track_w(mark)).unwrap_or(0);
                // 只看元素：容器里常有缩进用的空白文本节点，`dom.name` 给 `None`，
                // 按"次序未知就插在它前面"会把标记塞到最前面（`PROP-05` 顺序自检会拦下来）
                let before = Dom::live_children(dom, c)
                    .filter(|&x| dom.element(x).is_some())
                    .find(|&x| dom.name(x).and_then(order).is_none_or(|i| i > mine));
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(c),
                    before,
                    node: marker,
                });
            }
            None => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(owner),
                before: container_before,
                node: NewElement::new(track_w(container)).with_child(marker),
            }),
        }
    }

    /// `*PrChange` 旧值快照：`container` 现有子元素的 `Clean` 克隆，排除 `*Change` 自己与
    /// `skip` 列出的字段（`in_change = false`，如 `sectPr` 的页眉页脚引用、`pPr` 里的 `rPr`）。
    /// 插在 `container` 末尾（`PROP-05`：`*Change` 是每张表 `order` 的最后一项）。
    ///
    /// 容器里已经有 `change` 时什么都不做——Word 保留**最早**的那份快照。
    pub fn snapshot(
        &mut self,
        plan: &mut MutationPlan,
        dom: &Dom,
        container: NodeId,
        change: LocalName,
        inner: LocalName,
        skip: &[LocalName],
    ) -> Option<usize> {
        if Dom::live_children(dom, container).any(|c| dom.is(c, track_w(change))) {
            return None;
        }
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: None,
            node: self.marker(change),
        });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::New(k),
            before: None,
            node: NewElement::new(track_w(inner)),
        });
        for c in Dom::live_children(dom, container).collect::<Vec<_>>() {
            let Some(name) = dom.name(c) else { continue };
            if name.ns == NsId::W && (is_change_element(name.local) || skip.contains(&name.local)) {
                continue;
            }
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k + 1),
                before: None,
                source: c,
            });
        }
        Some(k)
    }

    /// `w:delText → w:t`、`w:delInstrText → w:instrText`（拒绝删除修订时改回去，7.4）。
    pub fn rename_to_live(plan: &mut MutationPlan, dom: &Dom, root: NodeId) {
        rename_text(plan, dom, root, false);
    }

    /// `w:t → w:delText`、`w:instrText → w:delInstrText`（run 进 `w:del` 之后必须改名，
    /// 否则 Word 会把删除的文字当正文显示）。反方向（拒绝修订）用同一张表。
    pub fn rename_to_deleted(plan: &mut MutationPlan, dom: &Dom, root: NodeId) {
        rename_text(plan, dom, root, true);
    }
}
fn rename_text(plan: &mut MutationPlan, dom: &Dom, root: NodeId, to_deleted: bool) {
    // 反方向（`w:delText → w:t`）由 7.4 的拒绝修订使用，同一张表
    let table: [(LocalName, LocalName); 2] = if to_deleted {
        [(LocalName::T, LocalName::DelText), (LocalName::InstrText, LocalName::DelInstrText)]
    } else {
        [(LocalName::DelText, LocalName::T), (LocalName::DelInstrText, LocalName::InstrText)]
    };
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if dom.node(n).dirty == Dirty::Deleted {
            continue;
        }
        let Some(e) = dom.element(n) else { continue };
        stack.extend(e.children.iter().rev());
        if e.name.ns != NsId::W {
            continue;
        }
        if let Some(&(_, to)) = table.iter().find(|(from, _)| *from == e.name.local) {
            plan.node_edits.push(NodeEdit::Rename { node: n, name: track_w(to) });
        }
    }
}
/// `*Change` 一族（快照里要排除它们自己）。
fn is_change_element(local: LocalName) -> bool {
    matches!(
        local,
        LocalName::RPrChange
            | LocalName::PPrChange
            | LocalName::SectPrChange
            | LocalName::TblPrChange
            | LocalName::TblPrExChange
            | LocalName::TblGridChange
            | LocalName::TrPrChange
            | LocalName::TcPrChange
            | LocalName::NumberingChange
    )
}

/// 把 `marker` 按 `order` 插进一个**还没进 DOM** 的属性容器（`PROP-05`）。
pub fn insert_ordered(
    container: &mut NewElement,
    marker: NewElement,
    order: fn(QName) -> Option<u16>,
) {
    let mine = order(marker.name).unwrap_or(u16::MAX);
    let at = container
        .children
        .iter()
        .position(|c| match c {
            NewNode::Element(e) => order(e.name).is_none_or(|i| i > mine),
            NewNode::Text(_) => false,
        })
        .unwrap_or(container.children.len());
    container.children.insert(at, NewNode::Element(marker));
}
impl Tracker {
    #[inline]
    /// 新建的块（`NewElement`，还没进 DOM）标成"插入"（`spec/18` 7.3）：
    ///
    /// - `NewBlock::Paragraph` → 内容子节点整批进一个 `w:ins`，再给段落标记加 `pPr/rPr/w:ins`；
    /// - `NewBlock::Table` → 每个 `w:tr` 加 `trPr/w:ins`（`w:tblPrEx` 仍排在 `w:trPr` 之前）；
    /// - `NewBlock::Xml` / `Wrapped`（`opaque`）→ 整个元素包进块级 `w:ins`（TS 的形态，
    ///   解析器已认）。调用方给的是整段原始 XML，往里面塞标记就等于改写它给的字节。
    fn mark_new_block_inserted(&mut self, node: NewElement, opaque: bool) -> NewElement {
        let t = self;

        if opaque {
            return t.marker(LocalName::Ins).with_child(node);
        }
        let is = |e: &NewElement, l: LocalName| e.name == track_w(l);
        if is(&node, LocalName::P) {
            let mut out = NewElement::new(node.name);
            out.attrs = node.attrs;
            let mut content = t.marker(LocalName::Ins);
            for child in node.children {
                match child {
                    NewNode::Element(e) if e.name == track_w(LocalName::PPr) => {
                        out.children.push(NewNode::Element(t.with_para_mark(e)));
                    }
                    other => content.children.push(other),
                }
            }
            // 没有 `pPr` 时补一个，只为放段落标记的 `w:ins`
            if !out
                .children
                .iter()
                .any(|c| matches!(c, NewNode::Element(e) if e.name == QName::w(LocalName::PPr)))
            {
                let ppr = t.with_para_mark(NewElement::new(track_w(LocalName::PPr)));
                out.children.insert(0, NewNode::Element(ppr));
            }
            if !content.children.is_empty() {
                out.children.push(NewNode::Element(content));
            }
            return out;
        }
        if is(&node, LocalName::Tbl) {
            let mut out = NewElement::new(node.name);
            out.attrs = node.attrs;
            for child in node.children {
                match child {
                    NewNode::Element(e) if e.name == track_w(LocalName::Tr) => {
                        out.children.push(NewNode::Element(t.with_row_mark(e, LocalName::Ins)));
                    }
                    other => out.children.push(other),
                }
            }
            return out;
        }
        t.marker(LocalName::Ins).with_child(node)
    }
}
impl Tracker {
    #[inline]
    /// `pPr` 里放段落标记的 `w:ins`（`rPr` 缺就建；`w:ins` 是 `rPr` 的第一个子元素，`PROP-05`）。
    fn with_para_mark(&mut self, ppr: NewElement) -> NewElement {
        let t = self;

        let marker = t.marker(LocalName::Ins);
        let mut out = NewElement::new(ppr.name);
        out.attrs = ppr.attrs;
        let mut done = false;
        for child in ppr.children {
            match child {
                NewNode::Element(e) if e.name == track_w(LocalName::RPr) => {
                    let mut rpr = NewElement::new(e.name);
                    rpr.attrs = e.attrs;
                    rpr.children.push(NewNode::Element(marker.clone()));
                    rpr.children.extend(e.children);
                    out.children.push(NewNode::Element(rpr));
                    done = true;
                }
                other => out.children.push(other),
            }
        }
        if !done {
            // `w:rPr` 是 `w:pPr` 的最后一个子元素（只有 `sectPr` / `pPrChange` 在它后面）
            let at = out
            .children
            .iter()
            .position(|c| {
                matches!(c, NewNode::Element(e)
                    if e.name == QName::w(LocalName::SectPr) || e.name == QName::w(LocalName::PPrChange))
            })
            .unwrap_or(out.children.len());
            out.children.insert(
                at,
                NewNode::Element(NewElement::new(track_w(LocalName::RPr)).with_child(marker)),
            );
        }
        out
    }
}
impl Tracker {
    #[inline]
    /// `w:tr` 加 `trPr/w:ins` 或 `trPr/w:del`。
    fn with_row_mark(&mut self, row: NewElement, mark: LocalName) -> NewElement {
        let t = self;

        let marker = t.marker(mark);
        let mut out = NewElement::new(row.name);
        out.attrs = row.attrs;
        let mut done = false;
        for child in row.children {
            match child {
                NewNode::Element(e) if e.name == track_w(LocalName::TrPr) => {
                    let mut trpr = NewElement::new(e.name);
                    trpr.attrs = e.attrs;
                    trpr.children.extend(e.children);
                    trpr.children.push(NewNode::Element(marker.clone()));
                    out.children.push(NewNode::Element(trpr));
                    done = true;
                }
                other => out.children.push(other),
            }
        }
        if !done {
            // `w:trPr` 紧跟 `w:tblPrEx`（如果有），在所有 `w:tc` 之前
            let at = out
                .children
                .iter()
                .position(
                    |c| !matches!(c, NewNode::Element(e) if e.name == QName::w(LocalName::TblPrEx)),
                )
                .unwrap_or(out.children.len());
            out.children.insert(
                at,
                NewNode::Element(NewElement::new(track_w(LocalName::TrPr)).with_child(marker)),
            );
        }
        out
    }
}
/// `REV_NOT_TRACKED`：这个操作 Word 也不记修订（或另有机制），照常执行、留一条记录。
/// 有 `MutationPlan` 的地方用 `ops::run` 入口那条集中判定；这里给没有计划的调用点用。
pub fn not_tracked(_ctx: &EditContext, _what: &str) {}

// 原 twin.rs
// `mc:AlternateContent` 的孪生同步（`spec/18` 7.7）。
//
// Word 写文本框与形状时发两份：`mc:Choice Requires="wps"` 里的 DrawingML（新版读这份）与
// `mc:Fallback` 里的 VML（老版读那份）。两份是同一个对象的两种写法，内容必须一致。
//
// 于是编辑落在 Choice 的 `w:txbxContent` 里之后，把 `mc:Fallback` 的**同序** `w:txbxContent`
// 内容整体换成 Choice 那份的深克隆（旧的 `Deleted`、新的 `New`，`XML-12` 规则 F）。位置直接
// 落在 `mc:Fallback` 里 → `EDIT_TARGET_FALLBACK`：改那一份下一次同步就被覆盖，没有意义。
//
// 孪生只在**内容**上同步。几何与样式（`SetDrawingGeometry` / `SetShapeStyle`）改的是 Choice 的
// `a:xfrm` / `wps:spPr`，VML 那边对应的是 `v:shape/@style @fillcolor @strokecolor`，
// 由 [`sync_shape_style`] 单独翻。
fn mc(local: LocalName) -> QName {
    QName::new(NsId::Mc, local)
}
fn txbx() -> QName {
    QName::new(NsId::W, LocalName::TxbxContent)
}
fn live(dom: &Dom, n: NodeId) -> bool {
    dom.node(n).dirty != Dirty::Deleted
}
/// 一处待同步的孪生：`mc:AlternateContent` 与它下面第 `idx` 个 `w:txbxContent`
/// （一个 `mc:AlternateContent` 里可以有好几个文本框，靠序号配对）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwinSite {
    pub part: Option<PartId>,
    pub ac: NodeId,
    pub idx: usize,
}
/// 某个分支下的全部 `w:txbxContent`（文档序）。
fn txbx_contents(dom: &Dom, branch: NodeId) -> Vec<NodeId> {
    dom.descendants(branch).filter(|&n| live(dom, n) && dom.is(n, txbx())).collect()
}
/// `mc:AlternateContent` 下第一个活的 `mc:Choice` / `mc:Fallback`。
fn branch(dom: &Dom, ac: NodeId, which: LocalName) -> Option<NodeId> {
    dom.children(ac).iter().copied().find(|&c| live(dom, c) && dom.is(c, mc(which)))
}
/// 编辑目标落在哪些孪生里。落在 `mc:Fallback` 中 → `Err(EDIT_TARGET_FALLBACK)`。
///
/// 一个目标可能牵动**好几处**孪生：套在两层文本框里的段落，里外两层的 Choice 内容都变了；
/// 整体替换（`SetTextboxContent`）的目标又在 `w:txbxContent` 之上。所以既往上收祖先、
/// 也往下收子树里的框，由内到外排——先同步里层，外层再克隆时拿到的就是同步过的里层。
pub fn sites(
    s: &EditSession,
    targets: impl IntoIterator<Item = (Option<PartId>, NodeId)>,
) -> Result<Vec<TwinSite>> {
    let mut out: Vec<TwinSite> = Vec::new();
    for (part, node) in targets {
        let dom = s.dom_in(part)?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            continue;
        }
        if dom.ancestors(node).any(|a| dom.is(a, mc(LocalName::Fallback))) {
            return Err(Error::edit(
                DiagCode::EditTargetFallback,
                "编辑位置在 mc:Fallback 里；那是 mc:Choice 的 VML 孪生，改 Choice 那一份",
            ));
        }
        // 子树里的框（最深的在前）＋ 自己和祖先里的框（由内到外）
        let mut inside: Vec<NodeId> = txbx_contents(dom, node);
        inside.reverse();
        inside.extend(
            std::iter::once(node).chain(dom.ancestors(node)).filter(|&a| dom.is(a, txbx())),
        );
        for t in inside {
            let Some(site) = twin_site_of(dom, part, t) else { continue };
            if !out.contains(&site) {
                out.push(site);
            }
        }
    }
    Ok(out)
}
/// 一个 `w:txbxContent` 属于哪个 `mc:AlternateContent` 的 Choice 分支、排第几。
/// 不在 `mc:Choice` 里（普通文本框、Fallback 已被上面拦下）→ `None`。
fn twin_site_of(dom: &Dom, part: Option<PartId>, txbx: NodeId) -> Option<TwinSite> {
    let choice = dom.ancestors(txbx).find(|&a| dom.is(a, mc(LocalName::Choice)))?;
    let ac = dom.parent(choice).filter(|&p| dom.is(p, mc(LocalName::AlternateContent)))?;
    let idx = txbx_contents(dom, choice).iter().position(|&x| x == txbx)?;
    Some(TwinSite { part, ac, idx })
}
/// 提交之后：每处孪生的 `mc:Fallback` 内容换成 Choice 那份的深克隆。
pub fn sync(s: &mut EditSession, sites: &[TwinSite]) -> Result<MutationResult> {
    let mut out = MutationResult::default();
    for site in sites {
        let part = site.part.unwrap_or_else(|| s.main_part());
        let dom = s.dom_in(Some(part))?;
        if !live(dom, site.ac) {
            continue;
        }
        let (Some(choice), Some(fallback)) =
            (branch(dom, site.ac, LocalName::Choice), branch(dom, site.ac, LocalName::Fallback))
        else {
            continue;
        };
        let (from, to) = (txbx_contents(dom, choice), txbx_contents(dom, fallback));
        let (Some(&from), Some(&to)) = (from.get(site.idx), to.get(site.idx)) else {
            continue;
        };
        let mut plan = MutationPlan::new(part);
        touch_block(dom, &mut plan, site.ac);
        for c in dom.children(to).iter().copied().filter(|&c| live(dom, c)) {
            plan.node_edits.push(NodeEdit::Delete(c));
        }
        for c in dom.children(from).iter().copied().filter(|&c| live(dom, c)) {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::Node(to),
                before: None,
                source: c,
            });
        }
        if !plan.is_empty() {
            out.absorb(s.commit_plan(plan)?);
        }
    }
    Ok(out)
}
/// 几何与样式改完之后：把 `mc:Fallback` 里的 VML 形状按 Choice 现在的值改
/// `@style` 的尺寸与位置、`@fillcolor` / `@filled`、`@strokecolor` / `@stroked`。
///
/// 只动这几个键：`@style` 里别的键（`z-index`、`mso-*`、`position`）原样留着——那是 VML 自己的
/// 排版参数，DrawingML 这边没有对应物，猜着改不如不动。
pub fn sync_shape_style(
    s: &mut EditSession,
    targets: impl IntoIterator<Item = (Option<PartId>, NodeId)>,
) -> Result<MutationResult> {
    let mut out = MutationResult::default();
    for (part, node) in targets {
        let p = part.unwrap_or_else(|| s.main_part());
        let dom = s.dom_in(Some(p))?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            continue;
        }
        let Some(ac) = dom.ancestors(node).find(|&a| dom.is(a, mc(LocalName::AlternateContent)))
        else {
            continue;
        };
        let (Some(choice), Some(fallback)) =
            (branch(dom, ac, LocalName::Choice), branch(dom, ac, LocalName::Fallback))
        else {
            continue;
        };
        let Some(shape) = vml_shape(dom, fallback) else { continue };
        let mut plan = MutationPlan::new(p);
        touch_block(dom, &mut plan, ac);
        for (name, value) in vml_attrs(dom, choice, shape) {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(shape),
                name: QName::new(NsId::None, name),
                value,
            });
        }
        if !plan.is_empty() {
            out.absorb(s.commit_plan(plan)?);
        }
    }
    Ok(out)
}
/// 同步是**另一个** plan：投影得跟着刷新，不然宿主段落的投影停在同步之前
/// （`TEST-07` 的随机序列在「插分节符 + 换绕排」两步上抓到过）。
fn touch_block(dom: &Dom, plan: &mut MutationPlan, node: NodeId) {
    let is_block = |n: NodeId| {
        dom.is(n, QName::new(NsId::W, LocalName::P))
            || dom.is(n, QName::new(NsId::W, LocalName::Tbl))
    };
    if let Some(b) = std::iter::once(node).chain(dom.ancestors(node)).find(|&n| is_block(n)) {
        plan.touch(b);
    }
}
/// `mc:Fallback` 里第一个会画东西的 VML 形状。
fn vml_shape(dom: &Dom, fallback: NodeId) -> Option<NodeId> {
    dom.descendants(fallback)
        .find(|&n| live(dom, n) && dom.is_ns(n, NsId::V, "v") && crate::model::drawn_shape(dom, n))
}
/// EMU / pt（VML 的 `@style` 用 pt）。
/// 从 Choice 现在的 `wp:extent` / `wp:posOffset` / `wps:spPr` 算出 VML 形状该有的属性。
fn vml_attrs(dom: &Dom, choice: NodeId, shape: NodeId) -> Vec<(LocalName, String)> {
    let wp = |l: LocalName| QName::new(NsId::Wp, l);
    let a = |l: LocalName| QName::new(NsId::A, l);
    let find =
        |root: NodeId, q: QName| dom.descendants(root).find(|&n| live(dom, n) && dom.is(n, q));
    let num = |n: NodeId, l: LocalName| -> Option<i64> {
        dom.attr_value(n, QName::new(NsId::None, l))?.trim().parse().ok()
    };
    let pt = |emu: i64| format!("{:.2}pt", emu as f64 / EMU_PER_PT);
    let mut style: Vec<(String, String)> = dom
        .attr_value(shape, QName::new(NsId::None, LocalName::Style))
        .map(|v| {
            v.split(';')
                .filter_map(|kv| kv.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut set = |k: &str, v: String| match style.iter_mut().find(|(x, _)| x == k) {
        Some(slot) => slot.1 = v,
        None => style.push((k.to_string(), v)),
    };
    if let Some(ext) = find(choice, wp(LocalName::Extent)) {
        if let Some(cx) = num(ext, LocalName::Cx) {
            set("width", pt(cx));
        }
        if let Some(cy) = num(ext, LocalName::Cy) {
            set("height", pt(cy));
        }
    }
    for (which, key) in
        [(LocalName::PositionH, "margin-left"), (LocalName::PositionV, "margin-top")]
    {
        let Some(pos) = find(choice, wp(which)) else { continue };
        let Some(off) = find(pos, wp(LocalName::PosOffset)) else { continue };
        let Some(v) = crate::model::text_of(dom, off).and_then(|t| t.trim().parse::<i64>().ok())
        else {
            continue;
        };
        set(key, pt(v));
    }
    let mut out = vec![(
        LocalName::Style,
        style.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(";"),
    )];
    // 填充与描边：`wps:spPr` 直属的 `a:solidFill` / `a:noFill`，以及 `a:ln` 里的那一对
    let sp_pr = find(choice, QName::new(NsId::Wps, LocalName::SpPr));
    let color = |container: Option<NodeId>| -> Option<Option<String>> {
        let c = container?;
        let direct =
            |q: QName| dom.children(c).iter().copied().find(|&x| live(dom, x) && dom.is(x, q));
        if direct(a(LocalName::NoFill)).is_some() {
            return Some(None);
        }
        let fill = direct(a(LocalName::SolidFill))?;
        let clr = find(fill, a(LocalName::SrgbClr))?;
        Some(dom.attr_value(clr, QName::new(NsId::None, LocalName::Val)).map(|v| v.into_owned()))
    };
    if let Some(f) = color(sp_pr) {
        out.push((LocalName::Filled, if f.is_some() { "t".into() } else { "f".into() }));
        if let Some(c) = f {
            out.push((LocalName::Fillcolor, format!("#{c}")));
        }
    }
    let ln = sp_pr.and_then(|n| {
        dom.children(n).iter().copied().find(|&x| live(dom, x) && dom.is(x, a(LocalName::Ln)))
    });
    if let Some(l) = color(ln) {
        out.push((LocalName::Stroked, if l.is_some() { "t".into() } else { "f".into() }));
        if let Some(c) = l {
            out.push((LocalName::Strokecolor, format!("#{c}")));
        }
    }
    out
}

/// 坐标流里的一个 token。
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
enum Tok<'a> {
    /// 一个纯文本 run：文本 + `w:rPr`。
    Run(&'a str, Option<Box<NewElement>>),
    /// 非纯文本的内联（图片 / 字段 / 制表符 / 超链接 …）：按整体比较，永远只当一个 token。
    Atom(String),
}

/// diff 脚本的一段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
enum Step {
    /// 两侧各前进 `n` 个 token。
    Equal(usize),
    /// 旧侧删掉 `n` 个。
    Delete(usize),
    /// 新侧插入 `n` 个。
    Insert(usize),
}

/// 超过此 token 数时放弃 Myers 并按原契约整体替换。
#[repr(usize)]
#[derive(Clone, Copy)]
enum DiffLimit {
    Tokens = 4_000,
}
impl From<DiffLimit> for usize {
    #[inline]
    fn from(value: DiffLimit) -> Self {
        value as Self
    }
}

/// 按 run 边界比较旧模型与新输入；借用只持续到产生拥有型编辑脚本。
/// 属性比较保守地包含 XML 属性顺序，相等段保留原节点及原字节。
#[repr(C)]
struct InlineDiff<'a> {
    old: &'a [Tok<'a>],
    new: &'a [Tok<'a>],
}
impl<'a> InlineDiff<'a> {
    /// 旧 → 新的编辑脚本。返回 `None` 表示放弃（太大），调用方整体替换。
    #[inline]
    fn steps(self) -> Option<Vec<Step>> {
        let Self { old, new } = self;
        if old.len() > usize::from(DiffLimit::Tokens) || new.len() > usize::from(DiffLimit::Tokens)
        {
            return None;
        }
        let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
        let rest_old = &old[pre..];
        let rest_new = &new[pre..];
        let suf = rest_old
            .iter()
            .rev()
            .zip(rest_new.iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
            .min(rest_old.len())
            .min(rest_new.len());
        let a = &rest_old[..rest_old.len() - suf];
        let b = &rest_new[..rest_new.len() - suf];
        let mut steps = Vec::new();
        if pre > 0 {
            steps.push(Step::Equal(pre));
        }
        steps.extend(Self { old: a, new: b }.myers()?);
        if suf > 0 {
            steps.push(Step::Equal(suf));
        }
        Some(Step::merge(steps))
    }

    /// Myers O(ND)：记录每一轮的 `v`，走完再回溯出脚本。`d` 超过两侧长度之和就放弃。
    #[inline]
    fn myers(self) -> Option<Vec<Step>> {
        let Self { old: a, new: b } = self;
        let (n, m) = (a.len(), b.len());
        if n == 0 {
            return Some(vec![Step::Insert(m)]);
        }
        if m == 0 {
            return Some(vec![Step::Delete(n)]);
        }
        let max = n + m;
        let offset = max as isize;
        let mut v = vec![0usize; 2 * max + 1];
        let mut trace: Vec<Vec<usize>> = Vec::with_capacity(max + 1);
        for d in 0..=max as isize {
            trace.push(v.clone());
            let mut k = -d;
            while k <= d {
                let idx = (k + offset) as usize;
                let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                    v[idx + 1]
                } else {
                    v[idx - 1] + 1
                };
                let mut y = (x as isize - k) as usize;
                while x < n && y < m && a[x] == b[y] {
                    x += 1;
                    y += 1;
                }
                v[idx] = x;
                if x >= n && y >= m {
                    return Some(Self::backtrack(&trace, n, m, offset));
                }
                k += 2;
            }
        }
        None
    }

    /// 从 `trace` 反推脚本（Myers 的标准回溯，倒着生成再反转）。
    #[inline]
    fn backtrack(trace: &[Vec<usize>], n: usize, m: usize, offset: isize) -> Vec<Step> {
        let mut steps: Vec<Step> = Vec::new();
        let (mut x, mut y) = (n as isize, m as isize);
        for (d, v) in trace.iter().enumerate().rev() {
            let d = d as isize;
            let k = x - y;
            let idx = (k + offset) as usize;
            let prev_k = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) { k + 1 } else { k - 1 };
            let prev_x = v[(prev_k + offset) as usize] as isize;
            let prev_y = prev_x - prev_k;
            while x > prev_x && y > prev_y {
                steps.push(Step::Equal(1));
                x -= 1;
                y -= 1;
            }
            if d > 0 {
                if x == prev_x {
                    steps.push(Step::Insert(1));
                } else {
                    steps.push(Step::Delete(1));
                }
            }
            x = prev_x;
            y = prev_y;
        }
        steps.reverse();
        steps
    }
}
impl Step {
    /// 相邻的同类合并。
    #[inline]
    fn merge(steps: Vec<Step>) -> Vec<Step> {
        let mut out: Vec<Step> = Vec::with_capacity(steps.len());
        for s in steps {
            match (out.last_mut(), s) {
                (Some(Step::Equal(n)), Step::Equal(m)) => *n += m,
                (Some(Step::Delete(n)), Step::Delete(m)) => *n += m,
                (Some(Step::Insert(n)), Step::Insert(m)) => *n += m,
                _ if matches!(s, Step::Equal(0) | Step::Delete(0) | Step::Insert(0)) => {}
                _ => out.push(s),
            }
        }
        out
    }
}

impl Emitter {
    #[inline]
    /// XML 1.0 允许的字符，加上折回 `w:br` 的 `\u{0B}` / `\u{0C}`。
    fn is_allowed_char(c: char) -> bool {
        matches!(c, '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}')
            || ('\u{20}'..='\u{D7FF}').contains(&c)
            || ('\u{E000}'..='\u{FFFD}').contains(&c)
            || c >= '\u{10000}'
    }
    #[inline]
    /// 剔除 XML 非法字符；剔除了任何字符时记一条 `EDIT_BAD_TEXT`。
    fn sanitize_text(text: &str, part: PartId, diags: &mut Vec<Diagnostic>) -> String {
        if text.chars().all(Emitter::is_allowed_char) {
            return text.to_string();
        }
        let removed = text.chars().filter(|c| !Emitter::is_allowed_char(*c)).count();
        diags.push(Diagnostic::invariant_violation(
            part,
            None,
            DiagCode::EditBadText,
            format!("文本含 {removed} 个 XML 非法字符，已剔除"),
        ));
        text.chars().filter(|c| Emitter::is_allowed_char(*c)).collect()
    }
    #[inline]
    /// 文本是否含需要折回元素的控制字符（不能直接写进现有 `w:t`）。
    fn has_control_chars(text: &str) -> bool {
        text.chars().any(|c| matches!(c, '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}'))
    }
    #[inline]
    fn text_element(deleted: bool, text: &str) -> NewElement {
        NewElement::new(QName::w(if deleted { LocalName::DelText } else { LocalName::T }))
            .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
            .with_text(text)
    }
    #[inline]
    /// 文本 → run 的子节点序列（不含 `rPr`）。
    fn text_segments(text: &str, deleted: bool) -> Vec<NewElement> {
        let mut out = Vec::new();
        let mut buf = String::new();
        let flush = |buf: &mut String, out: &mut Vec<NewElement>| {
            if !buf.is_empty() {
                out.push(Emitter::text_element(deleted, buf));
                buf.clear();
            }
        };
        for c in text.chars() {
            match c {
                '\t' => {
                    flush(&mut buf, &mut out);
                    out.push(NewElement::new(QName::w(LocalName::Tab)));
                }
                '\n' => {
                    flush(&mut buf, &mut out);
                    out.push(NewElement::new(QName::w(LocalName::Br)));
                }
                '\u{0C}' => {
                    flush(&mut buf, &mut out);
                    out.push(
                        NewElement::new(QName::w(LocalName::Br))
                            .with_attr(QName::w(LocalName::Type), "page"),
                    );
                }
                '\u{0B}' => {
                    flush(&mut buf, &mut out);
                    out.push(
                        NewElement::new(QName::w(LocalName::Br))
                            .with_attr(QName::w(LocalName::Type), "column"),
                    );
                }
                '\r' => {
                    flush(&mut buf, &mut out);
                    out.push(NewElement::new(QName::w(LocalName::Cr)));
                }
                _ => buf.push(c),
            }
        }
        flush(&mut buf, &mut out);
        out
    }
    #[inline]
    /// `w:r`：`rPr` + 文本段。
    fn new_run(text: &str, props: Option<NewElement>, deleted: bool) -> NewElement {
        let mut r = NewElement::new(QName::w(LocalName::R));
        if let Some(p) = props {
            r.push_child(p);
        }
        for seg in Emitter::text_segments(text, deleted) {
            r.push_child(seg);
        }
        r
    }
    #[inline]
    fn marker(m: &NewMarker) -> NewElement {
        match m {
            NewMarker::BookmarkStart { id, name } => {
                NewElement::new(QName::w(LocalName::BookmarkStart))
                    .with_attr(QName::w(LocalName::Id), id.clone())
                    .with_attr(QName::w(LocalName::Name), name.clone())
            }
            NewMarker::BookmarkEnd { id } => NewElement::new(QName::w(LocalName::BookmarkEnd))
                .with_attr(QName::w(LocalName::Id), id.clone()),
            NewMarker::CommentRangeStart { id } => {
                NewElement::new(QName::w(LocalName::CommentRangeStart))
                    .with_attr(QName::w(LocalName::Id), id.clone())
            }
            NewMarker::CommentRangeEnd { id } => {
                NewElement::new(QName::w(LocalName::CommentRangeEnd))
                    .with_attr(QName::w(LocalName::Id), id.clone())
            }
            NewMarker::CommentReference { id } => NewElement::new(QName::w(LocalName::R))
                .with_child(
                    NewElement::new(QName::w(LocalName::CommentReference))
                        .with_attr(QName::w(LocalName::Id), id.clone()),
                ),
        }
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `EDIT-06`：注释条目的下一个 `w:id`（0 / -1 是 separator 一族的保留号）。
    fn next_note_id(&self, endnote: bool) -> i64 {
        let s = self;
        let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
        notes.items.iter().filter_map(|n| n.id.trim().parse::<i64>().ok()).max().unwrap_or(0).max(0)
            + 1
    }
    #[inline]
    fn insert_atom(
        &mut self,
        at: InlinePos,
        atom: &NewAtom,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.part_or_main(at.part);
        let mut result = MutationResult::default();
        // 注释条目先建（它在另一个 part 里，`w:id` 要先定下来才能发引用 run）
        let note_ref = match atom {
            NewAtom::NoteRef { endnote, content } => {
                if at.part.is_some() {
                    return Err(unsupported("注释引用只能插在主 part"));
                }
                let id = EditSession::next_note_id(s, *endnote).to_string();
                result.absorb(EditSession::upsert_note_entry(s, *endnote, &id, content)?);
                Some((*endnote, id))
            }
            _ => None,
        };
        // 图片要先分配媒体关系（`&mut` 的活儿都在建计划之前做完）
        let image_run = match atom {
            NewAtom::Image(img) => Some(EditSession::image_run(s, img)?),
            _ => None,
        };
        let math_run = match atom {
            NewAtom::Math(m) => Some(EditSession::math_element(s, m)?),
            _ => None,
        };

        let tb = EditSession::require_text_block(s, at.part, at.para)?;
        let loc = at.offset.locate(tb)?;
        let (parent, before, inherit) = match EditSession::split_at(s, at, loc, &mut result)? {
            Some((left, right)) => (
                s.dom_in(at.part)?.parent(left).expect("run has a parent"),
                Some(right),
                Some(left),
            ),
            None => {
                let Loc::Boundary { index } = loc else {
                    unreachable!("split_at handles the rest")
                };
                EditSession::boundary_site_public(s, at.part, at.para, index)?
            }
        };
        let dom = s.dom_in(at.part)?;
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        let (run_parent, run_before) = match &mut Tracker::new(s.document(), ctx) {
            None => (Target::Node(parent), before),
            Some(t) => MutationPlan::plan_ins_site(&mut plan, dom, t, at.para, parent, before)?,
        };
        // `m:oMath` 不是 run：它自己就是段落的内容项
        if let Some(math) = math_run {
            plan.node_edits.push(NodeEdit::Insert {
                parent: run_parent,
                before: run_before,
                node: math,
            });
            plan.offset_delta.push((at.para, at.offset, 1));
            result.absorb(s.commit_plan(plan)?);
            return Ok(result);
        }
        let k = plan.node_edits.len();
        match image_run {
            // 图片的 run 是整份生成好的（含 `w:drawing` 与命名空间声明）
            Some(run) => plan.node_edits.push(NodeEdit::Insert {
                parent: run_parent,
                before: run_before,
                node: run,
            }),
            None => {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: run_parent,
                    before: run_before,
                    node: NewElement::new(QName::w(LocalName::R)),
                });
                // 继承左邻 run 的 `w:rPr`（与 `InsertText` 同一条）
                if let Some(rpr) = inherit.and_then(|r| MutationPlan::rpr_of(dom, r)) {
                    plan.node_edits.push(NodeEdit::InsertClone {
                        parent: Target::New(k),
                        before: None,
                        source: rpr,
                    });
                }
                let child = match atom {
                    NewAtom::Break { kind, clear } => {
                        NewAtom::break_element(*kind, clear.as_deref())
                    }
                    NewAtom::Symbol { font, code } => NewAtom::symbol_element(font, *code),
                    NewAtom::NoteRef { .. } => {
                        let (endnote, id) = note_ref.as_ref().expect("built above");
                        NewAtom::note_ref_element(*endnote, id)
                    }
                    NewAtom::Image(_) | NewAtom::Math(_) => unreachable!("handled above"),
                };
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: child,
                });
            }
        }
        plan.offset_delta.push((at.para, at.offset, 1));
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    /// `m:oMath`：OMML 直接解析，LaTeX 先转 OMML（7.5b 的 `Omml::try_from`）。
    fn math_element(&mut self, m: &NewMath) -> Result<NewElement> {
        let s = self;
        let omml = match m {
            NewMath::Omml(x) => x.clone(),
            NewMath::Latex(tex) => String::from(Omml::try_from(Latex::from(tex.as_str()))?),
        };
        let main = s.part_or_main(None);
        let dom = s
            .package_mut()
            .dom_mut(main)?
            .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
        let xml = if omml.trim_start().starts_with("<m:oMath") {
            omml
        } else {
            format!(r#"<m:oMath xmlns:m="{}">{omml}</m:oMath>"#, crate::model::NS_M)
        };
        let mut frags = crate::xml::parse_fragment(dom, &xml)
            .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("OMML 解析失败: {e}")))?;
        frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "OMML 片段为空"))
    }
}

impl NewAtom {
    #[inline]
    /// `w:br`：`type` 缺省是文字换行；`clear` 只对文字换行有意义。
    fn break_element(kind: BreakKind, clear: Option<&str>) -> NewElement {
        let mut e = NewElement::new(QName::w(LocalName::Br));
        match kind {
            BreakKind::Page => e.push_attr(QName::w(LocalName::Type), "page"),
            BreakKind::Column => e.push_attr(QName::w(LocalName::Type), "column"),
            BreakKind::TextWrapping => {
                if let Some(c) = clear {
                    e.push_attr(QName::w(LocalName::Clear), c.to_string());
                }
            }
        }
        e
    }
    #[inline]
    /// `w:sym`：`w:char` 是四位大写十六进制（`RES-05` 的反向；PUA 码位写低四位）。
    fn symbol_element(font: &str, code: u32) -> NewElement {
        let mut e = NewElement::new(QName::w(LocalName::Sym));
        e.push_attr(QName::w(LocalName::Font), font.to_string());
        e.push_attr(QName::w(LocalName::Char), format!("{:04X}", code & 0xFFFF));
        e
    }
    #[inline]
    fn note_ref_element(endnote: bool, id: &str) -> NewElement {
        let local =
            if endnote { LocalName::EndnoteReference } else { LocalName::FootnoteReference };
        NewElement::new(QName::w(local)).with_attr(QName::w(LocalName::Id), id.to_string())
    }
}

impl NewChart {
    #[inline]
    fn c(local: LocalName) -> QName {
        QName::new(NsId::C, local)
    }
    #[inline]
    fn chart_ops_a(local: LocalName) -> QName {
        QName::new(NsId::A, local)
    }
    #[inline]
    /// 命名空间在主 part 里的前缀；没声明 → 用缺省前缀并给出要补在 `w:drawing` 上的声明。
    fn chart_ops_prefix_or_decl(
        ctx: &NamespaceContext,
        ns: NsId,
        default: &str,
        uri: &str,
    ) -> (String, String) {
        match ctx.prefix_for(ns) {
            Some(p) if !p.is_empty() => (p.to_string(), String::new()),
            _ => (default.to_string(), format!(r#" xmlns:{default}="{uri}""#)),
        }
    }
    #[inline]
    /// `EDIT-06`：主 part 里全部 `wp:docPr/@id` 的最大值 + 1（TS 从 8000 起计数，差分容忍）。
    fn chart_ops_next_doc_pr_id(dom: &Dom) -> i64 {
        NewImage::media_ops_next_doc_pr_id(dom)
    }
    #[inline]
    /// JS `String(number)` 的数字写法：整数不带小数点。
    fn num(v: f64) -> String {
        if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
    }
    #[inline]
    /// Excel 列名：A、B、C …（系列 i 在 B 起的第 i 列）。
    fn col_letter(i: usize) -> char {
        (b'A' + i as u8) as char
    }
    #[inline]
    fn str_cache(values: &[String], f: &str) -> String {
        let pts: String = values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#,
                    String::from(FragmentText::from(v.as_str()))
                )
            })
            .collect();
        format!(
            r#"<c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val="{}"/>{pts}</c:strCache></c:strRef>"#,
            String::from(FragmentText::from(f)),
            values.len()
        )
    }
    #[inline]
    fn num_cache(values: &[Option<f64>], f: &str) -> String {
        let pts: String = values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.map(|v| format!(r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#, NewChart::num(v)))
            })
            .collect();
        format!(
            r#"<c:numRef><c:f>{}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>{pts}</c:numCache></c:numRef>"#,
            String::from(FragmentText::from(f)),
            values.len()
        )
    }
    #[inline]
    /// 整个图表 part（TS `buildChartPartXml`）。`external_data_rid` 是工作簿关系（`c:externalData`）。
    fn chart_part_xml(chart: &NewChart, external_data_rid: Option<&str>) -> String {
        let rows = chart.categories.len();
        let sers: String = chart
        .series
        .iter()
        .enumerate()
        .map(|(i, ser)| {
            let col = NewChart::col_letter(i + 1);
            let values: Vec<Option<f64>> = ser.values.iter().copied().take(rows).collect();
            format!(
                r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/><c:tx>{}</c:tx><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
                NewChart::str_cache(std::slice::from_ref(&ser.name), &format!("Sheet1!${col}$1")),
                NewChart::str_cache(&chart.categories, &format!("Sheet1!$A$2:$A${}", rows + 1)),
                NewChart::num_cache(&values, &format!("Sheet1!${col}$2:${col}${}", rows + 1)),
            )
        })
        .collect();
        let plot = match chart.kind {
            NewChartKind::Pie => {
                format!(
                    r#"<c:pieChart><c:varyColors val="1"/>{sers}<c:firstSliceAng val="0"/></c:pieChart>"#
                )
            }
            kind => {
                let axes = concat!(
                    r#"<c:catAx><c:axId val="111111111"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
                    r#"<c:delete val="0"/><c:axPos val="b"/><c:crossAx val="222222222"/></c:catAx>"#,
                    r#"<c:valAx><c:axId val="222222222"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
                    r#"<c:delete val="0"/><c:axPos val="l"/><c:crossAx val="111111111"/></c:valAx>"#
                );
                let inner = if kind == NewChartKind::Bar {
                    format!(
                        r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>{sers}<c:axId val="111111111"/><c:axId val="222222222"/></c:barChart>"#
                    )
                } else {
                    format!(
                        r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>{sers}<c:marker val="1"/><c:axId val="111111111"/><c:axId val="222222222"/></c:lineChart>"#
                    )
                };
                format!("{inner}{axes}")
            }
        };
        let title = chart.title.as_deref().map_or(String::new(), |t| {
        format!(
            r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>"#,
            String::from(FragmentText::from(t))
        )
    });
        let external = external_data_rid.map_or(String::new(), |rid| {
            format!(r#"<c:externalData r:id="{rid}"><c:autoUpdate val="0"/></c:externalData>"#)
        });
        let out = format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
                r#"<c:chartSpace xmlns:c="{c}" xmlns:a="{a}" xmlns:r="{r}">"#,
                r#"<c:chart>{title}<c:plotArea><c:layout/>{plot}</c:plotArea>"#,
                r#"<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart>{external}</c:chartSpace>"#
            ),
            c = NS_C,
            a = NS_A,
            r = NS_R,
            title = title,
            plot = plot,
            external = external
        );
        out
    }
    #[inline]
    /// 最小但合法的 xlsx（TS `buildChartWorkbookXlsxBase64`）：一张 `Sheet1`，A 列类别、B/C… 列系列，首行系列名，
    /// 文本走 `sharedStrings`（按出现顺序去重）。
    fn workbook_xlsx(categories: &[String], series: &[NewChartSeries]) -> Vec<u8> {
        const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
        let mut strings: Vec<String> = Vec::new();
        let mut si = |s: &str| -> usize {
            match strings.iter().position(|x| x == s) {
                Some(i) => i,
                None => {
                    strings.push(s.to_string());
                    strings.len() - 1
                }
            }
        };
        let mut header = format!(r#"<c r="A1" t="s"><v>{}</v></c>"#, si(""));
        for (j, ser) in series.iter().enumerate() {
            header.push_str(&format!(
                r#"<c r="{}1" t="s"><v>{}</v></c>"#,
                NewChart::col_letter(j + 1),
                si(&ser.name)
            ));
        }
        let mut rows = String::new();
        for (i, cat) in categories.iter().enumerate() {
            let row = i + 2;
            let mut cells = format!(r#"<c r="A{row}" t="s"><v>{}</v></c>"#, si(cat));
            for (j, ser) in series.iter().enumerate() {
                if let Some(Some(v)) = ser.values.get(i) {
                    cells.push_str(&format!(
                        r#"<c r="{}{row}"><v>{}</v></c>"#,
                        NewChart::col_letter(j + 1),
                        NewChart::num(*v)
                    ));
                }
            }
            rows.push_str(&format!(r#"<row r="{row}">{cells}</row>"#));
        }
        let sheet = format!(
            r#"{DECL}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">{header}</row>{rows}</sheetData></worksheet>"#
        );
        let sst: String = strings
            .iter()
            .map(|s| format!("<si><t>{}</t></si>", String::from(FragmentText::from(s.as_str()))))
            .collect();
        let shared = format!(
            r#"{DECL}<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{n}" uniqueCount="{n}">{sst}</sst>"#,
            n = strings.len()
        );
        let workbook = format!(
            r#"{DECL}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="{NS_R}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
        );
        let workbook_rels = format!(
            concat!(
                "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                r#"<Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/>"#,
                r#"<Relationship Id="rId2" Type="{NS_R}/sharedStrings" Target="sharedStrings.xml"/>"#,
                "</Relationships>"
            ),
            DECL = DECL,
            NS_R = NS_R
        );
        let top_rels = format!(
            concat!(
                "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                r#"<Relationship Id="rId1" Type="{NS_R}/officeDocument" Target="xl/workbook.xml"/>"#,
                "</Relationships>"
            ),
            DECL = DECL,
            NS_R = NS_R
        );
        let content_types = format!(
            concat!(
                "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
                r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
                r#"<Default Extension="xml" ContentType="application/xml"/>"#,
                r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
                r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
                r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
                "</Types>"
            ),
            DECL = DECL
        );
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", top_rels),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/sharedStrings.xml", shared),
        ] {
            w.start_file(name, opts).expect("zip in memory");
            w.write_all(body.as_bytes()).expect("zip in memory");
        }
        w.finish().expect("zip in memory").into_inner()
    }

    #[inline]
    /// 容器（`c:val` / `c:cat`）里每个带 `idx` 的缓存点：`texts[idx]` 有值就改它的第一个 `c:v`。
    fn point_edits(
        dom: &Dom,
        container: NodeId,
        texts: &[Option<String>],
        plan: &mut MutationPlan,
    ) {
        for pt in
            dom.semantic_descendants(container).filter(|&n| dom.is(n, NewChart::c(LocalName::Pt)))
        {
            let Some(idx) = dom
                .attr_value(pt, QName::new(NsId::None, LocalName::Idx))
                .and_then(|v| v.trim().parse::<usize>().ok())
            else {
                continue;
            };
            let Some(Some(text)) = texts.get(idx) else { continue };
            if let Some(v) =
                dom.semantic_descendants(pt).find(|&n| dom.is(n, NewChart::c(LocalName::V)))
            {
                MutationPlan::set_segment_text(dom, v, text, plan);
            }
        }
    }
    #[inline]
    /// 整个 `c:title` 元素都不存在：新建一个带文字的标题，按 `CT_Chart` 的顺序插在 `c:chart` 的最前面。
    fn new_title(dom: &Dom, root: NodeId, text: &str, plan: &mut MutationPlan) {
        let Some(chart) =
            dom.semantic_descendants(root).find(|&n| dom.is(n, NewChart::c(LocalName::Chart)))
        else {
            return;
        };
        let title = NewElement::new(NewChart::c(LocalName::Title))
            .with_child(
                NewElement::new(NewChart::c(LocalName::Tx)).with_child(
                    NewElement::new(NewChart::c(LocalName::Rich))
                        .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::BodyPr)))
                        .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::LstStyle)))
                        .with_child(
                            NewElement::new(NewChart::chart_ops_a(LocalName::P)).with_child(
                                NewElement::new(NewChart::chart_ops_a(LocalName::R)).with_child(
                                    NewElement::new(NewChart::chart_ops_a(LocalName::T))
                                        .with_text(text),
                                ),
                            ),
                        ),
                ),
            )
            .with_child(
                NewElement::new(NewChart::c(LocalName::Overlay))
                    .with_attr(QName::new(NsId::None, LocalName::Val), "0"),
            );
        let first = dom.semantic_children(chart).next();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(chart),
            before: first,
            node: title,
        });
    }
    #[inline]
    /// 没有文字的标题（自动标题 / 无缓存的 strRef）：给它一个带文字的 run。
    fn inject_title(dom: &Dom, title: NodeId, text: &str, plan: &mut MutationPlan) {
        let run = NewElement::new(NewChart::chart_ops_a(LocalName::R))
            .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::T)).with_text(text));
        let tx = Dom::children_named(dom, title, NewChart::c(LocalName::Tx)).next();
        let p = tx.and_then(|tx| {
            dom.semantic_descendants(tx).find(|&n| dom.is(n, NewChart::chart_ops_a(LocalName::P)))
        });
        if let Some(p) = p {
            // Word 的自动标题带一个只有 a:endParaRPr 的空 c:tx/c:rich 段落：run 按 schema 顺序插在它之前
            let end_pr =
                Dom::children_named(dom, p, NewChart::chart_ops_a(LocalName::EndParaRPr)).next();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(p),
                before: end_pr,
                node: run,
            });
            return;
        }
        let rich = NewElement::new(NewChart::c(LocalName::Tx)).with_child(
            NewElement::new(NewChart::c(LocalName::Rich))
                .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::BodyPr)))
                .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::LstStyle)))
                .with_child(NewElement::new(NewChart::chart_ops_a(LocalName::P)).with_child(run)),
        );
        match tx {
            // 无缓存的 strRef（没有 a:p、没有 c:v）：没地方注入，整个 c:tx 换成 rich body（CT_Tx 是二选一）
            Some(tx) => plan.node_edits.push(NodeEdit::Replace { old: tx, node: rich }),
            None => {
                let first = dom.semantic_children(title).next();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(title),
                    before: first,
                    node: rich,
                });
            }
        }
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `NewBlock::Chart`（含包在 `Wrapped` 里的）→ 建好 part 后的 `NewBlock::Xml` 绘图段落；其他块原样返回。
    /// 每个接收 `NewBlock` 的入口（`InsertBlock` / `UpdateBlockField` / 页眉页脚内容）都先过这里。
    fn materialize(&mut self, block: NewBlock) -> Result<NewBlock> {
        let s = self;
        EditSession::materialize_at(s, block, None)
    }
    #[inline]
    /// 同 `materialize`，但知道这块要落在哪儿——`NewBlock::Caption` 的编号是「位置之前同标签的
    /// `SEQ` 数 + 1」，非知道不可。
    fn materialize_at(&mut self, block: NewBlock, anchor: Option<BlockAt>) -> Result<NewBlock> {
        let s = self;
        Ok(match block {
            NewBlock::Chart { chart, extent_emu } => {
                NewBlock::Xml(EditSession::insert_chart_parts(s, &chart, extent_emu)?)
            }
            NewBlock::Image(img) => NewBlock::Xml(EditSession::image_paragraph(s, &img)?),
            // 独立公式段：先把 LaTeX 转成 OMML，再按 TS `mathParagraphXml` 生成整段
            NewBlock::MathPara { omml, align } => {
                let body = match omml {
                    NewMath::Omml(x) => Omml::from(x),
                    NewMath::Latex(t) => Omml::try_from(Latex::from(t.as_str()))?,
                };
                let xml = body.paragraph(&align);
                let main = s.main_part();
                let w_uri = NsId::W.uri(s.flavor()).expect("w 有两族 URI");
                let m_uri = crate::model::NS_M;
                let xml = xml.replacen(
                    "<w:p>",
                    &format!(r#"<w:p xmlns:w="{w_uri}" xmlns:m="{m_uri}">"#),
                    1,
                );
                let dom = s
                    .package_mut()
                    .dom_mut(main)?
                    .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
                let mut frags = crate::xml::parse_fragment(dom, &xml).map_err(|e| {
                    Error::edit(DiagCode::EditPlanInvalid, format!("公式段落解析失败: {e}"))
                })?;
                NewBlock::Xml(
                    frags
                        .pop()
                        .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "公式段落为空"))?,
                )
            }
            NewBlock::Wrapped { wrapper, block } => NewBlock::Wrapped {
                wrapper,
                block: Box::new(EditSession::materialize_at(s, *block, anchor)?),
            },
            // 7.8：块字段与题注
            NewBlock::Field(f) => NewBlock::Many(EditSession::materialize_field(s, f)?),
            NewBlock::Caption { label, text } => {
                EditSession::materialize_caption(s, &label, &text, anchor)?
            }
            NewBlock::Many(v) => NewBlock::Many(
                v.into_iter()
                    .map(|b| EditSession::materialize_at(s, b, anchor))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flat_map(|b| match b {
                        NewBlock::Many(inner) => inner,
                        b => vec![b],
                    })
                    .collect(),
            ),
            // 7.7：新建文本框 / 形状 / 线条
            b @ (NewBlock::Textbox { .. } | NewBlock::Shape { .. } | NewBlock::Line { .. }) => {
                NewBlock::Xml(shape_paragraph(s, b)?)
            }
            other => other,
        })
    }
    #[inline]
    /// 一批块：`NewBlock::Many` 就地摊平（生成器可以把一块展开成好几段）。
    fn materialize_all(&mut self, blocks: Vec<NewBlock>) -> Result<Vec<NewBlock>> {
        let s = self;
        let mut out = Vec::with_capacity(blocks.len());
        for b in blocks {
            match EditSession::materialize(s, b)? {
                NewBlock::Many(v) => out.extend(v),
                b => out.push(b),
            }
        }
        Ok(out)
    }
    #[inline]
    /// 建图表 part、内嵌工作簿、两个 `.rels` 里的关系与内容类型，返回引用它的绘图段落。
    ///
    /// part 名 `word/charts/chart{N}.xml` 取第一个空闲的 N（同一事务里刚建的也已登记在包里，所以也算）；
    /// 工作簿 `word/charts/embeddings/workbook{N}.xlsx`（`Default Extension="xlsx"`）；图表 part 的 `.rels`
    /// 第一条关系就是工作簿（`c:externalData r:id`）；主 part 的 `chart` 型关系；`wp:docPr/@id` 按 `EDIT-06`。
    fn insert_chart_parts(
        &mut self,
        chart: &NewChart,
        extent_emu: Option<(i64, i64)>,
    ) -> Result<NewElement> {
        let s = self;
        let main = s.main_part();
        let n = (1u32..)
            .find(|n| s.package().find_name(&format!("word/charts/chart{n}.xml")).is_none())
            .expect("总有空闲的编号");
        let xml = NewChart::chart_part_xml(chart, Some("rId1"));
        let (chart_part, chart_rid) =
            s.add_part(main, RelType::Chart, &format!("word/charts/chart{n}.xml"), CT_CHART, &xml)?;
        let xlsx = NewChart::workbook_xlsx(&chart.categories, &chart.series);
        let (_, wb_rid) = s.add_binary_part(
            chart_part,
            RelType::Package,
            &format!("word/charts/embeddings/workbook{n}.xlsx"),
            CT_XLSX,
            xlsx,
        )?;
        if wb_rid != "rId1" {
            // 图表 part 的 `.rels` 里已经有别的关系（不该发生：part 是刚建的）：把 `c:externalData` 指过去
            let dom = s.dom_in(Some(chart_part))?;
            let ext = dom
                .semantic_descendants(dom.root())
                .find(|&x| dom.is(x, NewChart::c(LocalName::ExternalData)))
                .ok_or_else(|| {
                    Error::edit(DiagCode::EditPlanInvalid, "新图表 part 里没有 c:externalData")
                })?;
            let mut plan = MutationPlan::new(chart_part);
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(ext),
                name: QName::new(NsId::R, LocalName::Id),
                value: wb_rid,
            });
            s.commit_plan(plan)?;
        }

        let (cx, cy) = extent_emu.unwrap_or(DEFAULT_EXTENT_EMU);
        let (cx, cy) = (cx.max(1), cy.max(1));
        let flavor = s.flavor();
        let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
        let doc_pr_id = NewChart::chart_ops_next_doc_pr_id(dom);
        let ctx = NamespaceContext::from_dom(dom, flavor);
        // `wp` / `r` 借主 part 的声明（缺了就在 `w:drawing` 上补）；`a` / `c` 与 TS 一样就地声明
        let (wp, wp_decl) = NewChart::chart_ops_prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
        let (r, r_decl) = NewChart::chart_ops_prefix_or_decl(&ctx, NsId::R, "r", NS_R);
        let para = format!(
            concat!(
                r#"<w:p><w:r><w:drawing{wp_decl}{r_decl}><{wp}:inline distT="0" distB="0" distL="0" distR="0">"#,
                r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:docPr id="{id}" name="Chart {id}"/>"#,
                r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{c_ns}">"#,
                r#"<c:chart xmlns:c="{c_ns}" {r}:id="{rid}"/></a:graphicData></a:graphic></{wp}:inline></w:drawing></w:r></w:p>"#
            ),
            wp_decl = wp_decl,
            r_decl = r_decl,
            wp = wp,
            cx = cx,
            cy = cy,
            id = doc_pr_id,
            a = NS_A,
            c_ns = NS_C,
            r = r,
            rid = chart_rid,
        );
        let mut frags = parse_fragment(dom, &para).map_err(|e| {
            Error::edit(DiagCode::EditPlanInvalid, format!("图表绘图段落解析失败: {e}"))
        })?;
        frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "图表绘图段落为空"))
    }
    #[inline]
    /// `EDIT-03 SetChartData`：按 TS `patchChartPartXml` 的锚定规则改缓存文本。
    ///
    /// 标题：`c:title` 里第一个 `a:t` 改、其余 `a:t` 清空；没有 `a:t` 则 `c:strCache/c:v`；两者都没有（自动标题）→
    /// 在 `c:tx/c:rich/a:p` 的 `a:endParaRPr` 之前注入 `a:r/a:t`，`c:tx` 是无缓存 `strRef` → 整个换成 rich body，
    /// 没有 `c:tx` → rich body 插为 `c:title` 第一个子元素。系列名 → `c:ser/c:tx` 下第一个 `c:v`；值 → `c:val`
    /// 缓存点按 `idx` 改文本，缺的点不补；类别 → **每个**系列的 `c:cat` 都改（缓存按系列各存一份）。
    fn set_chart_data(&mut self, part: PartId, patch: &ChartPatch) -> Result<MutationResult> {
        let s = self;
        let cp = s.document().chart_parts.get(&part).ok_or_else(|| {
            Error::edit(
                DiagCode::EditPlanInvalid,
                format!("part#{} 不是主 part 引用的图表 part", part.0),
            )
        })?;
        if cp.chartex {
            // TS 静默 no-op；这里明说（`docs/04` §8）
            return Err(Error::edit(
                DiagCode::EditUnsupported,
                "chartex（cx:chartSpace）图表只有降级显示，SetChartData 不支持",
            ));
        }
        let dom = s.dom_in(Some(part))?;
        let root = dom.root();
        let mut plan = MutationPlan::new(part);
        if let Some(title) = &patch.title {
            match dom.semantic_descendants(root).find(|&n| dom.is(n, NewChart::c(LocalName::Title)))
            {
                Some(t) => {
                    let mut texts: Vec<NodeId> = dom
                        .semantic_descendants(t)
                        .filter(|&n| dom.is(n, NewChart::chart_ops_a(LocalName::T)))
                        .collect();
                    if texts.is_empty() {
                        // strRef 标题把文字放在 c:strCache/c:v 里
                        texts = dom
                            .semantic_descendants(t)
                            .filter(|&n| dom.is(n, NewChart::c(LocalName::V)))
                            .collect();
                    }
                    if texts.is_empty() {
                        NewChart::inject_title(dom, t, title, &mut plan);
                    } else {
                        for (i, tn) in texts.into_iter().enumerate() {
                            let text = if i == 0 { title.as_str() } else { "" };
                            MutationPlan::set_segment_text(dom, tn, text, &mut plan);
                        }
                    }
                }
                // 整个 `c:title` 都没有（Word 的「无标题」图表就是删掉这个元素，`corpus/real/chart/chart-no-title`）：
                // 按 `CT_Chart` 的顺序建一个插在 `c:chart` 的最前面。TS 在这里什么都不做，请求被静默丢弃（`docs/04` §8）
                None => NewChart::new_title(dom, root, title, &mut plan),
            }
        }
        let sers: Vec<NodeId> = dom
            .semantic_descendants(root)
            .filter(|&n| dom.is(n, NewChart::c(LocalName::Ser)))
            .collect();
        for (i, ser) in sers.into_iter().enumerate() {
            let sp = patch.series.as_ref().and_then(|v| v.get(i)).and_then(Option::as_ref);
            if let Some(sp) = sp {
                if let Some(name) = &sp.name
                    && let Some(tx) =
                        Dom::children_named(dom, ser, NewChart::c(LocalName::Tx)).next()
                    && let Some(v) =
                        dom.semantic_descendants(tx).find(|&n| dom.is(n, NewChart::c(LocalName::V)))
                {
                    MutationPlan::set_segment_text(dom, v, name, &mut plan);
                }
                // 散点 / 气泡图把 y 值放在 `c:yVal`（`ChartPart::build` 的读侧同样是 `c:val ?? c:yVal`）：
                // 只认 `c:val` 会让「读得出来的值改不动」（`corpus/real/chart/chart-scatter` 等，`docs/04` §8）
                if let Some(values) = &sp.values
                    && let Some(val) =
                        Dom::children_named(dom, ser, NewChart::c(LocalName::Val)).next().or_else(
                            || Dom::children_named(dom, ser, NewChart::c(LocalName::YVal)).next(),
                        )
                {
                    let texts: Vec<Option<String>> =
                        values.iter().map(|v| v.map(NewChart::num)).collect();
                    NewChart::point_edits(dom, val, &texts, &mut plan);
                }
            }
            if let Some(cats) = &patch.categories
                && let Some(cat) = Dom::children_named(dom, ser, NewChart::c(LocalName::Cat)).next()
            {
                NewChart::point_edits(dom, cat, cats, &mut plan);
            }
        }
        if plan.node_edits.is_empty() {
            return Ok(MutationResult::default());
        }
        // 图表 part 不在正文投影里：让 `commit_plan` 整体重建，`Document.chart_parts` 才会跟上
        plan.structure_changed = true;
        s.commit_plan(plan)
    }
}

impl DrawingGeometry {
    #[inline]
    fn drawing_ops_a(local: LocalName) -> QName {
        QName::new(NsId::A, local)
    }
    #[inline]
    fn wp(local: LocalName) -> QName {
        QName::new(NsId::Wp, local)
    }

    #[inline]
    /// `w:drawing` 下的 `wp:inline` 或 `wp:anchor`。
    fn shell(dom: &Dom, drawing: NodeId) -> Result<NodeId> {
        Dom::live_children(dom, drawing)
            .find(|&c| {
                dom.is(c, DrawingGeometry::wp(LocalName::Inline))
                    || dom.is(c, DrawingGeometry::wp(LocalName::Anchor))
            })
            .ok_or_else(|| {
                Error::edit(DiagCode::EditBadPosition, "w:drawing 里没有 inline / anchor")
            })
    }
    #[inline]
    /// 这次改完之后的旋转角（度）：这次给了就用这次的，否则读现有的 `a:xfrm/@rot`。
    fn current_rot(dom: &Dom, drawing: NodeId, geom: &DrawingGeometry) -> i64 {
        if let Some(r) = geom.rot_deg {
            return r.unwrap_or(0);
        }
        DrawingGeometry::xfrms(dom, drawing)
            .into_iter()
            .find_map(|x| {
                dom.attr_value(x, QName::new(NsId::None, LocalName::Rot))?
                    .trim()
                    .parse::<i64>()
                    .ok()
            })
            .map_or(0, |v| v / 60_000)
    }
    #[inline]
    /// 绘图里全部 `a:xfrm`（图片的 `pic:spPr` 与形状的 `wps:spPr` 都算）。
    fn xfrms(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
        dom.descendants(drawing)
            .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
            .filter(|&n| dom.is(n, DrawingGeometry::drawing_ops_a(LocalName::Xfrm)))
            .collect()
    }
    #[inline]
    /// 每个 `a:xfrm` 下的 `a:ext`。
    fn xfrm_exts(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
        DrawingGeometry::xfrms(dom, drawing)
            .into_iter()
            .filter_map(|x| {
                Dom::live_children(dom, x)
                    .find(|&c| dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::Ext)))
            })
            .collect()
    }
    #[inline]
    fn blip_fills(dom: &Dom, drawing: NodeId) -> Vec<NodeId> {
        dom.descendants(drawing)
            .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
            .filter(|&n| {
                dom.is(n, DrawingGeometry::drawing_ops_a(LocalName::BlipFill))
                    || dom.is(n, QName::new(NsId::Pic, LocalName::BlipFill))
            })
            .collect()
    }
    #[inline]
    /// 容器里的 `a:solidFill` / `a:noFill` 换成新的。
    fn replace_fill(
        dom: &Dom,
        plan: &mut MutationPlan,
        container: NodeId,
        before: Option<NodeId>,
        color: Option<&str>,
    ) {
        let old: Vec<NodeId> = Dom::live_children(dom, container)
            .filter(|&c| {
                dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::SolidFill))
                    || dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::NoFill))
                    || dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::GradFill))
                    || dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::BlipFill))
            })
            .collect();
        let at = old.first().copied().or(before);
        for n in &old {
            plan.node_edits.push(NodeEdit::Delete(*n));
        }
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: at.filter(|n| !old.contains(n)).or(before),
            node: DrawingGeometry::fill_element(color),
        });
    }
    #[inline]
    fn fill_element(color: Option<&str>) -> NewElement {
        match color {
            Some(rgb) => NewElement::new(DrawingGeometry::drawing_ops_a(LocalName::SolidFill))
                .with_child(
                    NewElement::new(DrawingGeometry::drawing_ops_a(LocalName::SrgbClr)).with_attr(
                        QName::new(NsId::None, LocalName::Val),
                        rgb.trim_start_matches('#'),
                    ),
                ),
            None => NewElement::new(DrawingGeometry::drawing_ops_a(LocalName::NoFill)),
        }
    }
    #[inline]
    fn is_wrap_element(dom: &Dom, n: NodeId) -> bool {
        [
            LocalName::WrapNone,
            LocalName::WrapSquare,
            LocalName::WrapTight,
            LocalName::WrapThrough,
            LocalName::WrapTopAndBottom,
        ]
        .iter()
        .any(|&l| dom.is(n, DrawingGeometry::wp(l)))
    }
    #[inline]
    /// 锚定壳的就地改写。
    fn in_place_anchor(
        dom: &Dom,
        plan: &mut MutationPlan,
        anchor: NodeId,
        wrap: ImageWrap,
        pos: Option<&AnchorPos>,
        z_order: Option<i64>,
    ) {
        let none = |l: LocalName| QName::new(NsId::None, l);
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(anchor),
            name: none(LocalName::BehindDoc),
            value: if wrap == ImageWrap::Behind { "1".into() } else { "0".into() },
        });
        if let Some(z) = z_order {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(anchor),
                name: none(LocalName::RelativeHeight),
                value: (Z_BASE + z).max(0).to_string(),
            });
        }
        // 绕排元素：旧的删掉，新的插在原位（没有旧的就插在 `docPr` 之前）
        let old_wrap =
            Dom::live_children(dom, anchor).find(|&c| DrawingGeometry::is_wrap_element(dom, c));
        let polygon = old_wrap
            .filter(|_| DrawingGeometry::keeps_polygon(dom, old_wrap, wrap))
            .and_then(|w| {
                Dom::live_children(dom, w)
                    .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::WrapPolygon)))
            });
        let before = old_wrap.or_else(|| {
            Dom::live_children(dom, anchor)
                .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::DocPr)))
        });
        let at = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(anchor),
            before,
            node: DrawingGeometry::wrap_element(wrap, polygon.is_none()),
        });
        if let Some(poly) = polygon {
            plan.node_edits.push(NodeEdit::Move {
                node: poly,
                parent: Target::New(at),
                before: None,
            });
        }
        if let Some(w) = old_wrap {
            plan.node_edits.push(NodeEdit::Delete(w));
        }
        let Some(p) = pos else {
            // 没给位置：`square-left` ↔ `square-right` 说的正是图靠哪一边，所以横轴**用对齐写着**
            // 的时候跟着绕排走；写着明确偏移的（用户摆过位置）不动。
            if let Some(align) = Dom::live_children(dom, anchor)
                .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::PositionH)))
                .and_then(|h| {
                    Dom::live_children(dom, h)
                        .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::Align)))
                })
            {
                MutationPlan::set_segment_text(dom, align, NewImage::default_align(wrap), plan);
            }
            return;
        };
        for (which, axis) in [(LocalName::PositionH, &p.h), (LocalName::PositionV, &p.v)] {
            let e = DrawingGeometry::position_element(which, axis);
            match Dom::live_children(dom, anchor).find(|&c| dom.is(c, DrawingGeometry::wp(which))) {
                Some(old) => plan.node_edits.push(NodeEdit::Replace { old, node: e }),
                None => plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(anchor),
                    before: Dom::live_children(dom, anchor)
                        .find(|&c| !dom.is(c, DrawingGeometry::wp(LocalName::SimplePos))),
                    node: e,
                }),
            }
        }
    }
    #[inline]
    /// 同类绕排（紧密 ↔ 穿越）之间保留原来的 `wp:wrapPolygon`，别的情况重新生成矩形。
    fn keeps_polygon(dom: &Dom, old_wrap: Option<NodeId>, next: ImageWrap) -> bool {
        let polygonal = matches!(
            next,
            ImageWrap::TightLeft
                | ImageWrap::TightRight
                | ImageWrap::ThroughLeft
                | ImageWrap::ThroughRight
        );
        polygonal
            && old_wrap.is_some_and(|w| {
                dom.is(w, DrawingGeometry::wp(LocalName::WrapTight))
                    || dom.is(w, DrawingGeometry::wp(LocalName::WrapThrough))
            })
    }
    #[inline]
    /// 重建外壳：新壳插在旧壳之前，要保的子元素搬进去，旧壳删掉。
    fn rebuild_shell(
        dom: &Dom,
        plan: &mut MutationPlan,
        drawing: NodeId,
        old: NodeId,
        wrap: Option<ImageWrap>,
        pos: Option<&AnchorPos>,
        z_order: Option<i64>,
    ) {
        let shell_at = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(drawing),
            before: Some(old),
            node: DrawingGeometry::shell_element(wrap, pos, z_order),
        });
        let carry = |plan: &mut MutationPlan, upto: usize| {
            for &(ns, local) in &CARRIED[..upto] {
                if let Some(n) =
                    Dom::live_children(dom, old).find(|&c| dom.is(c, QName::new(ns, local)))
                {
                    plan.node_edits.push(NodeEdit::Move {
                        node: n,
                        parent: Target::New(shell_at),
                        before: None,
                    });
                }
            }
        };
        // `wp:extent` / `effectExtent` 在绕排元素之前，`docPr` 之后的三个在它之后
        carry(plan, 2);
        if let Some(w) = wrap {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(shell_at),
                before: None,
                node: DrawingGeometry::wrap_element(w, true),
            });
        }
        for &(ns, local) in &CARRIED[2..] {
            if let Some(n) =
                Dom::live_children(dom, old).find(|&c| dom.is(c, QName::new(ns, local)))
            {
                plan.node_edits.push(NodeEdit::Move {
                    node: n,
                    parent: Target::New(shell_at),
                    before: None,
                });
            }
        }
        plan.node_edits.push(NodeEdit::Delete(old));
    }
    #[inline]
    /// 新的 `wp:inline` / `wp:anchor` 外壳（不含要搬进去的子元素与绕排元素）。
    fn shell_element(
        wrap: Option<ImageWrap>,
        pos: Option<&AnchorPos>,
        z_order: Option<i64>,
    ) -> NewElement {
        let none = |l: LocalName| QName::new(NsId::None, l);
        let Some(wrap) = wrap else {
            let mut e = NewElement::new(DrawingGeometry::wp(LocalName::Inline));
            for l in [LocalName::DistT, LocalName::DistB, LocalName::DistL, LocalName::DistR] {
                e.push_attr(none(l), "0");
            }
            return e;
        };
        let mut e = NewElement::new(DrawingGeometry::wp(LocalName::Anchor));
        for (l, v) in [
            (LocalName::DistT, "0"),
            (LocalName::DistB, "0"),
            (LocalName::DistL, "114300"),
            (LocalName::DistR, "114300"),
            (LocalName::SimplePos, "0"),
        ] {
            e.push_attr(none(l), v);
        }
        e.push_attr(
            none(LocalName::RelativeHeight),
            (Z_BASE + z_order.unwrap_or(0)).max(0).to_string(),
        );
        e.push_attr(none(LocalName::BehindDoc), if wrap == ImageWrap::Behind { "1" } else { "0" });
        for (l, v) in [
            (LocalName::Locked, "0"),
            (LocalName::LayoutInCell, "1"),
            (LocalName::AllowOverlap, "1"),
        ] {
            e.push_attr(none(l), v);
        }
        e.push_child(
            NewElement::new(DrawingGeometry::wp(LocalName::SimplePos))
                .with_attr(none(LocalName::X), "0")
                .with_attr(none(LocalName::Y), "0"),
        );
        let default = DrawingGeometry::default_pos(wrap);
        let pos = pos.unwrap_or(&default);
        e.push_child(DrawingGeometry::position_element(LocalName::PositionH, &pos.h));
        e.push_child(DrawingGeometry::position_element(LocalName::PositionV, &pos.v));
        e
    }
    #[inline]
    /// 没给位置时的缺省（TS `applyImageWrap`：横向按绕排方向对齐、纵向贴段落）。
    fn default_pos(wrap: ImageWrap) -> AnchorPos {
        AnchorPos {
            h: AnchorAxis {
                relative_from: "column".into(),
                pos: AxisPos::Align(NewImage::default_align(wrap).into()),
            },
            v: AnchorAxis { relative_from: "paragraph".into(), pos: AxisPos::Offset(0) },
        }
    }
    #[inline]
    fn position_element(which: LocalName, axis: &AnchorAxis) -> NewElement {
        let mut e = NewElement::new(DrawingGeometry::wp(which))
            .with_attr(QName::new(NsId::None, LocalName::RelativeFrom), axis.relative_from.clone());
        e.push_child(match &axis.pos {
            AxisPos::Offset(v) => {
                NewElement::new(DrawingGeometry::wp(LocalName::PosOffset)).with_text(v.to_string())
            }
            AxisPos::Align(a) => {
                NewElement::new(DrawingGeometry::wp(LocalName::Align)).with_text(a.clone())
            }
        });
        e
    }
    #[inline]
    /// 绕排元素。`fresh_polygon` 为真时给紧密 / 穿越配一个矩形多边形（否则等着把旧的搬进来）。
    fn wrap_element(wrap: ImageWrap, fresh_polygon: bool) -> NewElement {
        let none = |l: LocalName| QName::new(NsId::None, l);
        let both = |e: NewElement| e.with_attr(none(LocalName::WrapText), "bothSides");
        match wrap {
            ImageWrap::Front | ImageWrap::Behind => {
                NewElement::new(DrawingGeometry::wp(LocalName::WrapNone))
            }
            ImageWrap::TopBottom => {
                NewElement::new(DrawingGeometry::wp(LocalName::WrapTopAndBottom))
            }
            ImageWrap::SquareLeft | ImageWrap::SquareRight => {
                both(NewElement::new(DrawingGeometry::wp(LocalName::WrapSquare)))
            }
            ImageWrap::TightLeft
            | ImageWrap::TightRight
            | ImageWrap::ThroughLeft
            | ImageWrap::ThroughRight => {
                let name = if matches!(wrap, ImageWrap::TightLeft | ImageWrap::TightRight) {
                    LocalName::WrapTight
                } else {
                    LocalName::WrapThrough
                };
                let mut e = both(NewElement::new(DrawingGeometry::wp(name)));
                if fresh_polygon {
                    e.push_child(DrawingGeometry::rect_polygon());
                }
                e
            }
        }
    }
    #[inline]
    /// 整幅图的矩形多边形（21600 = 一幅图的宽 / 高，OOXML 的相对坐标）。
    fn rect_polygon() -> NewElement {
        let none = |l: LocalName| QName::new(NsId::None, l);
        let pt = |name: LocalName, x: &str, y: &str| {
            NewElement::new(DrawingGeometry::wp(name))
                .with_attr(none(LocalName::X), x)
                .with_attr(none(LocalName::Y), y)
        };
        let mut e = NewElement::new(DrawingGeometry::wp(LocalName::WrapPolygon))
            .with_attr(none(LocalName::Edited), "0")
            .with_child(pt(LocalName::Start, "0", "0"));
        for (x, y) in [("0", "21600"), ("21600", "21600"), ("21600", "0"), ("0", "0")] {
            e.push_child(pt(LocalName::LineTo, x, y));
        }
        e
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    fn require_drawing(&self, drawing: NodeId) -> Result<()> {
        let s = self;
        let dom = s.dom();
        if (drawing.0 as usize) >= dom.node_count()
            || dom.node(drawing).dirty == Dirty::Deleted
            || !dom.is(drawing, QName::w(LocalName::Drawing))
        {
            return Err(Error::edit(DiagCode::EditBadPosition, "目标不是活的 w:drawing"));
        }
        Ok(())
    }
    #[inline]
    /// `SetDrawingGeometry`。
    fn set_geometry(&mut self, drawing: NodeId, geom: &DrawingGeometry) -> Result<MutationResult> {
        let s = self;
        EditSession::require_drawing(s, drawing)?;
        let part = s.main_part();
        let dom = s.dom();
        let shell = DrawingGeometry::shell(dom, drawing)?;
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        let set = |plan: &mut MutationPlan, node: NodeId, name: QName, v: String| {
            plan.node_edits.push(NodeEdit::SetAttr { node: Target::Node(node), name, value: v });
        };
        // ① `wp:extent` 与每个 `a:ext`（图片自己的 `a:xfrm/a:ext`）
        if let Some((cx, cy)) = geom.extent_emu {
            let (cx, cy) = (cx.max(1), cy.max(1));
            let none = |l: LocalName| QName::new(NsId::None, l);
            if let Some(ext) = Dom::live_children(dom, shell)
                .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::Extent)))
            {
                set(&mut plan, ext, none(LocalName::Cx), cx.to_string());
                set(&mut plan, ext, none(LocalName::Cy), cy.to_string());
            }
            // 旋转后的外接框（与 6.7 新建图片同一条公式）
            let rot = DrawingGeometry::current_rot(dom, drawing, geom).rem_euclid(360);
            let rad = rot as f64 * std::f64::consts::PI / 180.0;
            let bw = (cx as f64 * rad.cos()).abs() + (cy as f64 * rad.sin()).abs();
            let bh = (cx as f64 * rad.sin()).abs() + (cy as f64 * rad.cos()).abs();
            let ex = (((bw - cx as f64) / 2.0).round() as i64).max(0);
            let ey = (((bh - cy as f64) / 2.0).round() as i64).max(0);
            if let Some(ee) = Dom::live_children(dom, shell)
                .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::EffectExtent)))
            {
                for (n, v) in
                    [(LocalName::L, ex), (LocalName::T, ey), (LocalName::R, ex), (LocalName::B, ey)]
                {
                    set(&mut plan, ee, none(n), v.to_string());
                }
            }
            for ext in DrawingGeometry::xfrm_exts(dom, drawing) {
                set(&mut plan, ext, none(LocalName::Cx), cx.to_string());
                set(&mut plan, ext, none(LocalName::Cy), cy.to_string());
            }
        }
        // ② 锚定位置
        if let Some((x, y)) = geom.pos_offset_emu {
            for (which, v) in [(LocalName::PositionH, x), (LocalName::PositionV, y)] {
                let Some(pos) =
                    Dom::live_children(dom, shell).find(|&c| dom.is(c, DrawingGeometry::wp(which)))
                else {
                    continue;
                };
                match Dom::live_children(dom, pos)
                    .find(|&c| dom.is(c, DrawingGeometry::wp(LocalName::PosOffset)))
                {
                    Some(off) => {
                        MutationPlan::set_segment_text(dom, off, &v.to_string(), &mut plan)
                    }
                    None => plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(pos),
                        before: None,
                        node: NewElement::new(DrawingGeometry::wp(LocalName::PosOffset))
                            .with_text(v.to_string()),
                    }),
                }
            }
        }
        // ③ `a:xfrm` 的旋转 / 翻转
        for xfrm in DrawingGeometry::xfrms(dom, drawing) {
            let none = |l: LocalName| QName::new(NsId::None, l);
            if let Some(rot) = geom.rot_deg {
                match rot {
                    Some(d) => set(&mut plan, xfrm, none(LocalName::Rot), (d * 60_000).to_string()),
                    None => plan.node_edits.push(NodeEdit::RemoveAttr {
                        node: Target::Node(xfrm),
                        name: none(LocalName::Rot),
                    }),
                }
            }
            for (flag, name) in [(geom.flip_h, LocalName::FlipH), (geom.flip_v, LocalName::FlipV)] {
                match flag {
                    Some(true) => set(&mut plan, xfrm, none(name), "1".into()),
                    Some(false) => plan
                        .node_edits
                        .push(NodeEdit::RemoveAttr { node: Target::Node(xfrm), name: none(name) }),
                    None => {}
                }
            }
        }
        // ④ 裁剪窗
        if let Some(crop) = geom.crop {
            for fill in DrawingGeometry::blip_fills(dom, drawing) {
                let existing = Dom::live_children(dom, fill)
                    .find(|&c| dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::SrcRect)));
                match (crop, existing) {
                    (None, Some(n)) => plan.node_edits.push(NodeEdit::Delete(n)),
                    (None, None) => {}
                    (Some(r), old) => {
                        let mut e =
                            NewElement::new(DrawingGeometry::drawing_ops_a(LocalName::SrcRect));
                        let none = |l: LocalName| QName::new(NsId::None, l);
                        for (n, v) in [
                            (LocalName::L, r.l),
                            (LocalName::T, r.t),
                            (LocalName::R, r.r),
                            (LocalName::B, r.b),
                        ] {
                            if v != 0 {
                                e.push_attr(none(n), v.to_string());
                            }
                        }
                        match old {
                            Some(n) => plan.node_edits.push(NodeEdit::Replace { old: n, node: e }),
                            // `a:srcRect` 是 `a:blipFill` 的第一个子元素（在 `a:stretch` 之前）
                            None => plan.node_edits.push(NodeEdit::Insert {
                                parent: Target::Node(fill),
                                before: Dom::live_children(dom, fill).find(|&c| {
                                    !dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::Blip))
                                }),
                                node: e,
                            }),
                        }
                    }
                }
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetDrawingZOrder`：`relativeHeight = Z_BASE + z`（TS `applyImageZOrder`）。随文图片没有
    /// z-order，给了也不动（`wp:inline` 上没有这个属性）。
    fn set_z_order(&mut self, drawing: NodeId, z: i64) -> Result<MutationResult> {
        let s = self;
        EditSession::require_drawing(s, drawing)?;
        let part = s.main_part();
        let dom = s.dom();
        let shell = DrawingGeometry::shell(dom, drawing)?;
        if !dom.is(shell, DrawingGeometry::wp(LocalName::Anchor)) {
            return Err(unsupported("随文图片没有 z-order；先用 SetDrawingWrap 改成锚定"));
        }
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(shell),
            name: QName::new(NsId::None, LocalName::RelativeHeight),
            value: (Z_BASE + z).max(0).to_string(),
        });
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetShapeStyle`：`wps:spPr` 的填充与描边。`None` = 不动，`Some(None)` = 无填充 / 无描边。
    fn set_shape_style(
        &mut self,
        shape: NodeId,
        fill: Option<Option<String>>,
        outline: Option<Option<String>>,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let dom = s.dom();
        if (shape.0 as usize) >= dom.node_count() || dom.node(shape).dirty == Dirty::Deleted {
            return Err(Error::edit(DiagCode::EditBadPosition, "形状节点不存在"));
        }
        // 目标可以是 `wps:wsp` 自己，也可以是它的 `wps:spPr`
        let sp_pr = if dom.is(shape, QName::new(NsId::Wps, LocalName::SpPr)) {
            shape
        } else {
            Dom::live_children(dom, shape)
                .find(|&c| dom.is(c, QName::new(NsId::Wps, LocalName::SpPr)))
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "形状里没有 wps:spPr"))?
        };
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(sp_pr).find(|&x| dom.is(x, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        if let Some(f) = fill {
            // `a:solidFill` / `a:noFill` 在 `a:prstGeom` 之后、`a:ln` 之前（CT_ShapeProperties）
            let before = Dom::live_children(dom, sp_pr)
                .find(|&c| dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::Ln)));
            DrawingGeometry::replace_fill(dom, &mut plan, sp_pr, before, f.as_deref());
        }
        if let Some(o) = outline {
            let ln = Dom::live_children(dom, sp_pr)
                .find(|&c| dom.is(c, DrawingGeometry::drawing_ops_a(LocalName::Ln)));
            match ln {
                Some(ln) => DrawingGeometry::replace_fill(dom, &mut plan, ln, None, o.as_deref()),
                None => {
                    let mut e = NewElement::new(DrawingGeometry::drawing_ops_a(LocalName::Ln));
                    e.push_child(DrawingGeometry::fill_element(o.as_deref()));
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(sp_pr),
                        before: None,
                        node: e,
                    });
                }
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetDrawingWrap`：`wp:inline ↔ wp:anchor` 的绕排切换。
    ///
    /// 壳的种类不变（锚定 → 锚定）时**就地改**：只换绕排元素、`behindDoc` / `relativeHeight`、
    /// 给了 `pos` 才动 `positionH` / `positionV`。种类变了才重建外壳，`wp:extent` /
    /// `effectExtent` / `docPr` / `cNvGraphicFramePr` / `a:graphic` 用 `move_within_part`
    /// 搬进去，原字节保住（`SAVE-08`）。
    fn set_wrap(
        &mut self,
        drawing: NodeId,
        wrap: Option<ImageWrap>,
        pos: Option<&AnchorPos>,
        z_order: Option<i64>,
    ) -> Result<MutationResult> {
        let s = self;
        EditSession::require_drawing(s, drawing)?;
        let part = s.main_part();
        let dom = s.dom();
        let old = DrawingGeometry::shell(dom, drawing)?;
        let was_anchor = dom.is(old, DrawingGeometry::wp(LocalName::Anchor));
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(drawing).find(|&x| dom.is(x, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        match (was_anchor, wrap) {
            // 锚定 → 锚定：就地改
            (true, Some(new_wrap)) => {
                DrawingGeometry::in_place_anchor(dom, &mut plan, old, new_wrap, pos, z_order)
            }
            // 随文 → 随文：绕排本来就没有，只有 `pos` / `z` 无处可放
            (false, None) => {}
            // 换壳
            (_, next) => {
                DrawingGeometry::rebuild_shell(dom, &mut plan, drawing, old, next, pos, z_order)
            }
        }
        s.commit_plan(plan)
    }
}

impl NewBlockField {
    #[inline]
    /// 段落的标题级别：`\t` 的自定义样式优先，然后是样式链给的级别（`RES-02`），
    /// `\u` 时还认段落自己的 `outlineLvl`。不是标题 → `None`。
    fn level_of(tb: &TextBlock, r: &Resolver<'_>, opts: &TocOptions) -> Option<u8> {
        // `\t` 给的自定义样式优先
        if let Some(id) = tb.style_id.as_deref()
            && let Some((_, l)) = opts.styles.iter().find(|(n, _)| n == id)
        {
            return Some(*l);
        }
        // `\u`：`facts.outline_level` 已经是 1 起的级别（段落自己的 `outlineLvl` + 1，
        // 没有就退回样式链）。不带 `\u` 时只认样式链给的标题级别（`RES-02`）。
        if opts.use_outline
            && let Some(l) = tb.facts.outline_level
        {
            return Some(l);
        }
        tb.style_id.as_deref().and_then(|id| r.heading_level(id))
    }
    #[inline]
    /// 目录条目的文字：坐标流去掉字段结构、脚注 / 尾注引用与图形占位（TS 的 `blocks[].runs` 同样不含它们）。
    fn entry_text(tb: &TextBlock) -> String {
        let mut out = String::new();
        for i in &tb.inlines {
            let Inline::Run(r) = i else { continue };
            for seg in &r.segments {
                let skip = matches!(
                    seg.kind,
                    SegmentKind::DelText
                        | SegmentKind::Drawing { .. }
                        | SegmentKind::Pict
                        | SegmentKind::Object
                        | SegmentKind::Ink
                        | SegmentKind::FootnoteRef { .. }
                        | SegmentKind::EndnoteRef { .. }
                        | SegmentKind::FootnoteRefMark
                        | SegmentKind::EndnoteRefMark
                );
                // 字段的结构 run（begin / 指令 / separate / end）在坐标流里长度为 0，
                // 但指令文本不该进目录：只取结果区与普通文字
                if skip {
                    continue;
                }
                out.push_str(&r.text[seg.text.start as usize..seg.text.end as usize]);
            }
        }
        out.trim().to_string()
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// 正文里按文档序的目录条目（含表格单元格内的段落）。
    fn toc_entries(
        &mut self,
        opts: &TocOptions,
        pages: Option<&HashMap<NodeId, u32>>,
    ) -> Result<Vec<TocEntry>> {
        let s = self;
        let bookmarks = EditSession::existing_toc_bookmarks(s)?;
        let doc = s.document();
        let r = Resolver::new(doc);
        Ok(doc
            .blocks()
            .filter_map(Block::as_text)
            .filter_map(|tb| {
                let level = NewBlockField::level_of(tb, &r, opts)?;
                if level < opts.levels.0 || level > opts.levels.1 {
                    return None;
                }
                let text = NewBlockField::entry_text(tb);
                if text.is_empty() {
                    return None;
                }
                Some(TocEntry {
                    level,
                    text,
                    page_no: pages.and_then(|m| m.get(&tb.node).copied()),
                    bookmark: bookmarks.get(&tb.node).cloned(),
                })
            })
            .collect())
    }
    #[inline]
    /// 已经挂在段落上的 `_Toc…` 书签（重算时不重新铸名字）。
    fn existing_toc_bookmarks(&mut self) -> Result<HashMap<NodeId, String>> {
        let s = self;
        let part = s.main_part();
        let idx = s.spans_of(part)?;
        let mut out = HashMap::new();
        for sp in idx.live() {
            let Some(name) = sp.kind.bookmark_name() else { continue };
            if !name.starts_with("_Toc") {
                continue;
            }
            if let Some(a) = sp.start.as_ref() {
                out.entry(a.container).or_insert_with(|| name.to_string());
            }
        }
        Ok(out)
    }
    #[inline]
    /// 主 part 里全部 `XE` 字段的词（第一个实参），文档序。
    fn index_terms(&self) -> Vec<String> {
        let s = self;
        let Some(idx) = s.document().fields_in(s.main_part()) else { return Vec::new() };
        idx.fields()
            .iter()
            .filter(|f| f.instr.keyword == Keyword::Xe)
            .filter_map(|f| f.instr.first_argument().map(str::to_string))
            .collect()
    }
    #[inline]
    /// 这个落点之前同标签的 `SEQ` 字段数 + 1（TS `generateCaptionXml` 的编号来源）。
    ///
    /// 「之前」按落点算：`After` / `End` 连锚点**整棵子树**一起算进去（题注插在它后面），
    /// `Before` / `Start` 只算到锚点为止。
    fn next_seq_number(&self, label: &str, at: BlockAt) -> u32 {
        let s = self;

        let Some(idx) = s.document().fields_in(s.main_part()) else { return 1 };
        let dom = s.dom();
        let order: std::collections::HashMap<NodeId, usize> =
            dom.descendants(dom.root()).enumerate().map(|(i, n)| (n, i)).collect();
        let cutoff = match at {
            BlockAt::After(n) | BlockAt::End(n) => {
                order.get(&n).map_or(usize::MAX, |&i| i + dom.descendants(n).count())
            }
            BlockAt::Before(n) | BlockAt::Start(n) => order.get(&n).copied().unwrap_or(0),
        };
        let n = idx
            .fields()
            .iter()
            .filter(|f| f.instr.keyword == Keyword::Seq)
            .filter(|f| f.instr.first_argument() == Some(label))
            .filter(|f| order.get(&f.form.head()).is_some_and(|&i| i < cutoff))
            .count();
        n as u32 + 1
    }
    #[inline]
    /// `\h`：给每个目录条目对应的标题段落补一个隐藏书签 `_Toc{9 位}`（已经有的不动），
    /// 回填进 `entries[].bookmark`。TS 不做这一步（`docs/04` §8：我们更强）。
    fn ensure_toc_bookmarks(&mut self, entries: &mut [TocEntry]) -> Result<MutationResult> {
        let s = self;
        let mut out = MutationResult::default();
        let opts = TocOptions::default();
        // 条目与段落的对应要重算一遍：`toc_entries` 只带回了文字
        let paras: Vec<NodeId> = {
            let doc = s.document();
            let r = Resolver::new(doc);
            doc.blocks()
                .filter_map(Block::as_text)
                .filter(|tb| {
                    NewBlockField::level_of(tb, &r, &opts).is_some()
                        && !NewBlockField::entry_text(tb).is_empty()
                })
                .map(|tb| tb.node)
                .collect()
        };
        let mut next = 100_000_000u32;
        for (e, para) in entries.iter_mut().zip(paras) {
            if e.bookmark.is_some() {
                continue;
            }
            let name = loop {
                let name = format!("_Toc{next:09}");
                next += 1;
                let taken = s
                    .spans_of(s.main_part())?
                    .live()
                    .any(|sp| sp.kind.bookmark_name() == Some(name.as_str()));
                if !taken {
                    break name;
                }
            };
            let len = s
                .text_block_in(None, para)
                .map(|tb| tb.inlines.iter().map(Inline::utf16_len).sum::<u32>())
                .unwrap_or(0);
            out.absorb(EditSession::add_bookmark(
                s,
                &name,
                InlinePos::new(para, 0),
                InlinePos::new(para, len),
            )?);
            e.bookmark = Some(name);
        }
        Ok(out)
    }
    #[inline]
    /// 一批生成好的段落 XML → `NewBlock::Xml`。
    fn parse_blocks(&mut self, xml: Vec<String>) -> Result<Vec<NewBlock>> {
        let s = self;
        let main = s.main_part();
        let flavor = s.flavor();
        let w_uri = crate::xml::NsId::W.uri(flavor).expect("w 有两族 URI");
        let dom = s
            .package_mut()
            .dom_mut(main)?
            .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
        // part 本来就把 `w` 绑在这个 URI 上就不用再声明一遍（`XML-14`：序列化按作用域补）
        let ctx = crate::package::ns_context::NamespaceContext::from_dom(dom, flavor);
        let decl = match ctx.prefix_for(crate::xml::NsId::W) {
            Some("w") => String::new(),
            _ => format!(r#" xmlns:w="{w_uri}""#),
        };
        xml.into_iter()
            .map(|x| {
                let x = x.replacen("<w:p>", &format!("<w:p{decl}>"), 1);
                let mut frags = crate::xml::parse_fragment(dom, &x).map_err(|e| {
                    Error::edit(DiagCode::EditPlanInvalid, format!("生成的段落解析失败: {e}"))
                })?;
                frags
                    .pop()
                    .map(NewBlock::Xml)
                    .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "生成的段落为空"))
            })
            .collect()
    }
    #[inline]
    /// `NewBlock::Field` → 生成好的段落（`chart_ops::materialize` 调）。
    fn materialize_field(&mut self, f: NewBlockField) -> Result<Vec<NewBlock>> {
        let s = self;
        let xml = match f {
            NewBlockField::Toc { opts, pages } => {
                let mut entries = EditSession::toc_entries(s, &opts, pages.as_ref())?;
                if opts.hyperlinks && !opts.ts_shape() {
                    EditSession::ensure_toc_bookmarks(s, &mut entries)?;
                }
                toc_gen::generate(&entries, &opts)
            }
            NewBlockField::Index(opts) => index_gen::generate(&EditSession::index_terms(s), &opts),
        };
        EditSession::parse_blocks(s, xml)
    }
    #[inline]
    /// `NewBlock::Caption` → 一段题注（编号 = 位置之前同标签的 `SEQ` 数 + 1）。
    fn materialize_caption(
        &mut self,
        label: &str,
        text: &str,
        at: Option<BlockAt>,
    ) -> Result<NewBlock> {
        let s = self;
        let n = match at {
            Some(at) => EditSession::next_seq_number(s, label, at),
            // 不知道落点（不经 `InsertBlock` 的路）：全文数一遍
            None => EditSession::next_seq_number(s, label, BlockAt::End(s.dom().root())),
        };
        let xml = seq_gen::caption(label, n, text, false);
        Ok(EditSession::parse_blocks(s, vec![xml])?.pop().expect("一段"))
    }
    #[inline]
    /// `EDIT-03 RegenerateBlockField`：按生成器重算一个块字段的结果区。
    ///
    /// 走 `UpdateBlockField` 那条既有机制换结果区，所以 `w:fldLock`、追踪、跨段字段的规则一条都不用
    /// 重写。`\h` 模式会先给标题段落补书签——那会重建索引，所以字段用 begin 的 `NodeId` 重新认
    /// （`FieldId` 是下标，可能已经变了）。
    fn regenerate(
        &mut self,
        field: FieldId,
        options: BlockFieldOptions,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let main = s.main_part();
        let (instr, begin) = s
            .document()
            .fields_in(main)
            .and_then(|i| i.get(field))
            .map(|f| (f.instr.clone(), f.form.head()))
            .ok_or_else(|| Error::edit(DiagCode::EditTargetMissing, "没有这个字段"))?;
        let plan = match options {
            BlockFieldOptions::Toc { mut opts, pages } => {
                opts.emit_field_structure = false;
                Plan::Toc(opts, pages)
            }
            BlockFieldOptions::Index(mut opts) => {
                opts.emit_field_structure = false;
                Plan::Index(opts)
            }
            BlockFieldOptions::Auto { pages } => match instr.keyword {
                Keyword::Toc => Plan::Toc(
                    Box::new(TocOptions {
                        emit_field_structure: false,
                        ..TocOptions::from_instruction(&instr)
                    }),
                    pages,
                ),
                Keyword::Index => Plan::Index(Box::new(IndexOptions {
                    emit_field_structure: false,
                    ..IndexOptions::from_instruction(&instr)
                })),
                ref k => {
                    return Err(unsupported(&format!("{k:?} 没有生成器；能重算的是 TOC 与 INDEX")));
                }
            },
        };
        let mut result = MutationResult::default();
        let blocks = match plan {
            Plan::Toc(opts, pages) => {
                let mut entries = EditSession::toc_entries(s, &opts, pages.as_ref())?;
                if opts.hyperlinks && !opts.ts_shape() {
                    result.absorb(EditSession::ensure_toc_bookmarks(s, &mut entries)?);
                }
                EditSession::parse_blocks(s, toc_gen::generate(&entries, &opts))?
            }
            Plan::Index(opts) => EditSession::parse_blocks(
                s,
                index_gen::generate(&EditSession::index_terms(s), &opts),
            )?,
        };
        // 补书签重建过索引：按 begin 的节点重新认这个字段
        let field = s
            .document()
            .fields_in(main)
            .and_then(|i| i.fields().iter().find(|f| f.form.head() == begin))
            .map(|f| f.id)
            .ok_or_else(|| {
                Error::edit(DiagCode::EditTargetMissing, "补书签之后找不到这个字段了")
            })?;
        result.absorb(EditSession::update_block_field(s, field, blocks, ctx)?);
        result.absorb(EditSession::relocate_structure(s, begin)?);
        Ok(result)
    }
    #[inline]
    /// `FLD-12` 的多段形态：begin + 指令 + separate 在**首段**开头、end 在**末段**末尾。
    ///
    /// `UpdateBlockField` 只换结果区，结构 run 还留在原来那两段里；重算之后那两段常常就空了
    /// （目录自己生成的字段，begin 就在第一条条目那一段）。把结构 run 搬进新的首 / 末段
    /// （`move_within_part`，原字节保住），空掉的段落删掉。
    fn relocate_structure(&mut self, begin: NodeId) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let Some(f) = s
            .document()
            .fields_in(part)
            .and_then(|i| i.fields().iter().find(|f| f.form.head() == begin))
        else {
            return Ok(MutationResult::default());
        };
        let crate::span::FieldForm::Complex { begin, separate, end, instr_nodes, .. } = &f.form
        else {
            return Ok(MutationResult::default());
        };
        let (begin, end) = (*begin, *end);
        let structure: Vec<NodeId> =
            std::iter::once(begin).chain(instr_nodes.iter().copied()).chain(*separate).collect();
        let dom = s.dom();
        let para_of = |n: NodeId| {
            std::iter::once(n)
                .chain(dom.ancestors(n))
                .find(|&a| dom.is(a, crate::xml::QName::w(crate::xml::LocalName::P)))
        };
        let (Some(begin_para), Some(end_para)) = (para_of(begin), para_of(end)) else {
            return Ok(MutationResult::default());
        };
        if begin_para == end_para {
            return Ok(MutationResult::default());
        }
        // 新条目段落 = begin 段与 end 段之间的兄弟
        let Some(parent) = dom.parent(begin_para) else { return Ok(MutationResult::default()) };
        let between: Vec<NodeId> = dom
            .children(parent)
            .iter()
            .copied()
            .filter(|&c| dom.node(c).dirty != crate::xml::Dirty::Deleted)
            .skip_while(|&c| c != begin_para)
            .skip(1)
            .take_while(|&c| c != end_para)
            .collect();
        let (Some(&first), Some(&last)) = (between.first(), between.last()) else {
            return Ok(MutationResult::default());
        };
        let content = |p: NodeId| {
            dom.children(p)
                .iter()
                .copied()
                .filter(|&c| dom.node(c).dirty != crate::xml::Dirty::Deleted)
                .filter(|&c| dom.element(c).is_some())
                .filter(|&c| !dom.is(c, crate::xml::QName::w(crate::xml::LocalName::PPr)))
                .collect::<Vec<_>>()
        };
        // 结构 run 之外还有别的内容就别动：那一段不是纯结构段
        if content(begin_para).iter().any(|c| !structure.contains(c))
            || content(end_para) != vec![end]
        {
            return Ok(MutationResult::default());
        }
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        let head = content(first).first().copied();
        for n in structure {
            plan.node_edits.push(crate::xml::NodeEdit::Move {
                node: n,
                parent: crate::xml::Target::Node(first),
                before: head,
            });
        }
        plan.node_edits.push(crate::xml::NodeEdit::Move {
            node: end,
            parent: crate::xml::Target::Node(last),
            before: None,
        });
        plan.node_edits.push(crate::xml::NodeEdit::Delete(begin_para));
        plan.node_edits.push(crate::xml::NodeEdit::Delete(end_para));
        s.commit_plan(plan)
    }
}

impl NewImage {
    #[inline]
    /// MIME → 媒体 part 扩展名（TS `IMAGE_EXT`；别的 `image/*` 取子类型）。
    fn extension_for(mime: &str) -> String {
        match mime {
            "image/png" => "png".into(),
            "image/jpeg" | "image/jpg" => "jpg".into(),
            "image/gif" => "gif".into(),
            "image/bmp" => "bmp".into(),
            "image/tiff" => "tiff".into(),
            "image/svg+xml" => "svg".into(),
            other => other.rsplit('/').next().unwrap_or("bin").trim_start_matches("x-").to_string(),
        }
    }
    #[inline]
    /// 没给位置时 `wp:positionH` 的对齐（TS `applyImageWrap`）：向右绕排的贴右、上下型居中、其余贴左。
    fn default_align(wrap: ImageWrap) -> &'static str {
        match wrap {
            ImageWrap::SquareRight | ImageWrap::TightRight | ImageWrap::ThroughRight => "right",
            ImageWrap::TopBottom => "center",
            _ => "left",
        }
    }
    #[inline]
    /// `wp:anchor` 的三段：开标签、`位置 \0 绕排元素`（绕排元素要放在 extent / effectExtent 之后、docPr 之前）、闭标签
    /// （TS `applyImageWrap`）。
    fn anchor_parts(
        wp: &str,
        wrap: ImageWrap,
        pos: Option<PosOffset>,
        z_order: Option<i64>,
    ) -> (String, String, String) {
        let behind = if wrap == ImageWrap::Behind { "1" } else { "0" };
        let open = format!(
            r#"<{wp}:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="{}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1">"#,
            Z_ORDER_BASE + z_order.unwrap_or(0)
        );
        let position = match pos {
            Some(p) => {
                let (rel_h, rel_v) =
                    if p.page { ("page", "page") } else { ("column", "paragraph") };
                format!(
                    r#"<{wp}:simplePos x="0" y="0"/><{wp}:positionH relativeFrom="{rel_h}"><{wp}:posOffset>{}</{wp}:posOffset></{wp}:positionH><{wp}:positionV relativeFrom="{rel_v}"><{wp}:posOffset>{}</{wp}:posOffset></{wp}:positionV>"#,
                    p.x, p.y
                )
            }
            None => {
                let h = NewImage::default_align(wrap);
                format!(
                    r#"<{wp}:simplePos x="0" y="0"/><{wp}:positionH relativeFrom="column"><{wp}:align>{h}</{wp}:align></{wp}:positionH><{wp}:positionV relativeFrom="paragraph"><{wp}:posOffset>0</{wp}:posOffset></{wp}:positionV>"#
                )
            }
        };
        let wrap_el = match wrap {
            ImageWrap::SquareLeft
            | ImageWrap::SquareRight
            | ImageWrap::TightLeft
            | ImageWrap::TightRight
            | ImageWrap::ThroughLeft
            | ImageWrap::ThroughRight => format!(r#"<{wp}:wrapSquare wrapText="bothSides"/>"#),
            ImageWrap::TopBottom => format!("<{wp}:wrapTopAndBottom/>"),
            ImageWrap::Front | ImageWrap::Behind => format!("<{wp}:wrapNone/>"),
        };
        (open, format!("{position}\u{0}{wrap_el}"), format!("</{wp}:anchor>"))
    }
    #[inline]
    fn media_ops_prefix_or_decl(
        ctx: &NamespaceContext,
        ns: NsId,
        default: &str,
        uri: &str,
    ) -> (String, String) {
        match ctx.prefix_for(ns) {
            Some(p) if !p.is_empty() => (p.to_string(), String::new()),
            _ => (default.to_string(), format!(r#" xmlns:{default}="{uri}""#)),
        }
    }
    #[inline]
    /// `EDIT-06`：主 part 里全部 `wp:docPr/@id` 的最大值 + 1。
    ///
    /// 扫**全部**未删节点，包括本引擎不理解的 `mc:Choice` 分支与 `mc:Fallback`：id 的唯一性是整个 part 的事，
    /// 与 MCE 选哪支无关。Word 原生墨迹（`Requires="wpi"`）的 `wp:docPr id="1"` 就藏在语义遍历看不见的分支里，
    /// 与新图片撞号后 Word 弹恢复提示（真实 Word 第二轮核对，`corpus/real/_round2/EDITED.md`）。
    fn media_ops_next_doc_pr_id(dom: &Dom) -> i64 {
        let mut max = 0i64;
        let mut stack = vec![dom.root()];
        while let Some(n) = stack.pop() {
            if dom.node(n).dirty == crate::xml::Dirty::Deleted {
                continue;
            }
            let Some(e) = dom.element(n) else { continue };
            stack.extend(e.children.iter().rev());
            if e.name.local != LocalName::DocPr || !dom.is_ns(n, NsId::Wp, "wp") {
                continue;
            }
            if let Some(v) = dom.attr_value(n, QName::new(NsId::None, LocalName::Id))
                && let Ok(id) = v.trim().parse::<i64>()
            {
                max = max.max(id);
            }
        }
        max + 1
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `NewBlock::Image` → 建媒体 part 后的绘图段落（`chart_ops::materialize` 调）。
    fn image_paragraph(&mut self, img: &NewImage) -> Result<NewElement> {
        let s = self;
        let rid = s.add_media(img.bytes.clone(), &img.mime)?;
        let (cx, cy) = (img.extent_emu.0.max(1), img.extent_emu.1.max(1));
        // Word 按未旋转的 wp:extent + wp:effectExtent 排版：转过的非正方形图片要把外接框的溢出记进去
        let rot = img.rot_deg.map_or(0, |d| d.rem_euclid(360));
        let rad = rot as f64 * std::f64::consts::PI / 180.0;
        let bw = (cx as f64 * rad.cos()).abs() + (cy as f64 * rad.sin()).abs();
        let bh = (cx as f64 * rad.sin()).abs() + (cy as f64 * rad.cos()).abs();
        let ee_x = (((bw - cx as f64) / 2.0).round() as i64).max(0);
        let ee_y = (((bh - cy as f64) / 2.0).round() as i64).max(0);
        let flavor = s.flavor();
        let main = s.main_part();
        let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
        let id = NewImage::media_ops_next_doc_pr_id(dom);
        let ctx = NamespaceContext::from_dom(dom, flavor);
        let (wp, wp_decl) = NewImage::media_ops_prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
        let (r, r_decl) = NewImage::media_ops_prefix_or_decl(&ctx, NsId::R, "r", NS_R);

        let mut spacing = Vec::new();
        if let Some(ps) = &img.para_spacing {
            if let Some(b) = ps.before_twips.filter(|&v| v > 0) {
                spacing.push(format!(r#"w:before="{b}""#));
            }
            if let Some(a) = ps.after_twips.filter(|&v| v >= 0) {
                spacing.push(format!(r#"w:after="{a}""#));
            }
            if let (Some(l), Some(rule)) =
                (ps.line_twips.filter(|&v| v != 0), ps.line_rule.as_deref())
            {
                spacing.push(format!(r#"w:line="{l}" w:lineRule="{rule}""#));
            }
        }
        let spacing = if spacing.is_empty() {
            String::new()
        } else {
            format!("<w:spacing {}/>", spacing.join(" "))
        };
        let jc = img
            .align
            .as_deref()
            .filter(|a| *a != "left")
            .map_or(String::new(), |a| format!(r#"<w:jc w:val="{a}"/>"#));
        let ppr = if spacing.is_empty() && jc.is_empty() {
            String::new()
        } else {
            format!("<w:pPr>{spacing}{jc}</w:pPr>")
        };
        let xfrm_attrs = format!(
            "{}{}{}",
            if rot != 0 { format!(r#" rot="{}""#, rot * 60_000) } else { String::new() },
            if img.flip_h { r#" flipH="1""# } else { "" },
            if img.flip_v { r#" flipV="1""# } else { "" }
        );
        let (open, position_wrap, close) = match img.wrap {
            None => (
                format!(r#"<{wp}:inline distT="0" distB="0" distL="0" distR="0">"#),
                String::new(),
                format!("</{wp}:inline>"),
            ),
            Some(wrap) => NewImage::anchor_parts(&wp, wrap, img.pos_offset_emu, img.z_order),
        };
        let (position, wrap_el) = match position_wrap.split_once('\u{0}') {
            Some((p, w)) => (p.to_string(), w.to_string()),
            None => (String::new(), String::new()),
        };
        let para = format!(
            concat!(
                r#"<w:p>{ppr}<w:r><w:drawing{wp_decl}{r_decl}>{open}{position}"#,
                r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:effectExtent l="{eex}" t="{eey}" r="{eex}" b="{eey}"/>{wrap_el}"#,
                r#"<{wp}:docPr id="{id}" name="Picture {id}"/>"#,
                r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}"><pic:pic xmlns:pic="{pic}">"#,
                r#"<pic:nvPicPr><pic:cNvPr id="{id}" name="Picture {id}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip {r}:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm{xfrm}><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>{close}</w:drawing></w:r></w:p>"#
            ),
            ppr = ppr,
            wp_decl = wp_decl,
            r_decl = r_decl,
            open = open,
            position = position,
            wp = wp,
            cx = cx,
            cy = cy,
            eex = ee_x,
            eey = ee_y,
            wrap_el = wrap_el,
            id = id,
            a = NS_A,
            pic = NS_PIC,
            r = r,
            rid = rid,
            xfrm = xfrm_attrs,
            close = close,
        );
        let mut frags = parse_fragment(dom, &para).map_err(|e| {
            Error::edit(DiagCode::EditPlanInvalid, format!("图片段落解析失败: {e}"))
        })?;
        frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "图片段落为空"))
    }
    #[inline]
    /// 随文图片的 **run**（`InsertAtom::Image`，7.5）：生成整段再把里面的 `w:r` 取出来，
    /// 与 `NewBlock::Image` 共用同一套模板。
    fn image_run(&mut self, img: &NewImage) -> Result<NewElement> {
        let s = self;
        let para = EditSession::image_paragraph(s, img)?;
        para.children
            .into_iter()
            .find_map(|c| match c {
                crate::xml::NewNode::Element(e) if e.name == QName::new(NsId::W, LocalName::R) => {
                    Some(e)
                }
                _ => None,
            })
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "图片段落里没有 run"))
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `SetNoteContent`：整条条目的正文段落换成 `content`（自引用标记 run 保留）。
    fn set_note_content(
        &mut self,
        endnote: bool,
        id: &str,
        content: &[Vec<NewRun>],
    ) -> Result<MutationResult> {
        let s = self;
        let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
        if notes.get(id).is_none() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("没有 id 为 {id:?} 的{}", if endnote { "尾注" } else { "脚注" }),
            ));
        }
        let paras: Vec<Vec<NewRun>> =
            if content.is_empty() { vec![vec![NewRun::text("")]] } else { content.to_vec() };
        EditSession::upsert_note_entry(s, endnote, id, &paras)
    }
    #[inline]
    /// `RemoveNote`：删条目 + 删正文里的引用 run（run 里只剩引用时整 run 删，否则只删引用元素）。
    fn remove_note(&mut self, endnote: bool, id: &str) -> Result<MutationResult> {
        let s = self;
        let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
        if notes.get(id).is_none() {
            return Err(Error::edit(
                DiagCode::EditTargetMissing,
                format!("没有 id 为 {id:?} 的{}", if endnote { "尾注" } else { "脚注" }),
            ));
        }
        let mut result = EditSession::drop_references(s, endnote, id)?;
        result.absorb(EditSession::remove_note_entry(s, endnote, id)?);
        Ok(result)
    }
    #[inline]
    /// 正文里指向这条注释的引用 run（`w:footnoteReference` / `w:endnoteReference`）。
    fn drop_references(&mut self, endnote: bool, id: &str) -> Result<MutationResult> {
        let s = self;
        let refname =
            if endnote { LocalName::EndnoteReference } else { LocalName::FootnoteReference };
        let main = s.main_part();
        let dom = s.dom();
        let mut plan = MutationPlan::new(main);
        let mut stack = vec![dom.root()];
        while let Some(n) = stack.pop() {
            if dom.node(n).dirty == Dirty::Deleted {
                continue;
            }
            let Some(e) = dom.element(n) else { continue };
            stack.extend(e.children.iter().rev());
            if !dom.is(n, QName::w(refname))
                || dom.attr_value(n, QName::w(LocalName::Id)).as_deref() != Some(id)
            {
                continue;
            }
            let Some(run) = dom.parent(n).filter(|&r| dom.is(r, QName::w(LocalName::R))) else {
                plan.node_edits.push(NodeEdit::Delete(n));
                continue;
            };
            // run 里除 `w:rPr` 之外只有这个引用 → 整 run 删
            let others = dom
                .children(run)
                .iter()
                .copied()
                .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
                .filter(|&c| c != n && !dom.is(c, QName::w(LocalName::RPr)))
                .count();
            let victim: NodeId = if others == 0 { run } else { n };
            plan.node_edits.push(NodeEdit::Delete(victim));
            if let Some(p) = dom.ancestors(victim).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                plan.touch(p);
            }
        }
        if plan.is_empty() {
            return Ok(MutationResult::default());
        }
        s.commit_plan(plan)
    }
}

impl Job {
    #[inline]
    /// `trPr/w:ins|w:del`：标记在行属性里，动的是**整行**。
    fn row_actions(kind: RevKind) -> Option<(Act, Act)> {
        match kind {
            RevKind::Insert => Some((Act::DropMark, Act::Drop)),
            RevKind::Delete => Some((Act::Drop, Act::DropMark)),
            _ => None,
        }
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `AcceptRevision` / `RejectRevision`。
    fn one(&mut self, rev: RevisionId, accept: bool) -> Result<MutationResult> {
        let s = self;
        let e = s.document().revisions.get(rev).ok_or_else(|| {
            Error::edit(DiagCode::EditPlanInvalid, format!("修订 {} 不在索引里", rev.0))
        })?;
        // 一个字段、一个表格的列改动都要整个一起解决（见 `field_group` / `table_group`）
        let mut group = EditSession::field_group(s, e);
        for id in EditSession::table_group(s, e) {
            if !group.contains(&id) {
                group.push(id);
            }
        }
        group.sort_by_key(|&id| {
            // 网格快照最后还原：它整块替换 `w:tblGrid`，前面掉格时删掉的 `w:gridCol` 会被它盖掉
            let last = s
                .document()
                .revisions
                .get(id)
                .is_some_and(|x| x.kind == crate::model::RevKind::TableGridChange);
            (last, id.0)
        });
        let jobs: Vec<Job> = group
            .into_iter()
            .filter_map(|id| s.document().revisions.get(id))
            .map(|e| EditSession::job_of(s, e))
            .collect();
        EditSession::apply_jobs(s, jobs, accept)
    }
    #[inline]
    /// 与这条修订同属**一次列改动**的那几条：一张表的 `tblGridChange` 与它的 `cellIns` / `cellDel`
    /// 是同一次操作的两面，单独解决一面就会让网格与行里的格数对不上（`SAVE_TABLE_GRID`）。
    fn table_group(&self, e: &crate::model::RevisionEntry) -> Vec<RevisionId> {
        let s = self;
        if !matches!(e.kind, RevKind::TableGridChange | RevKind::CellInsert | RevKind::CellDelete) {
            return Vec::new();
        }
        let Ok(dom) = s.dom_in(Some(e.part)) else { return Vec::new() };
        let table_of = |n: NodeId| dom.ancestors(n).find(|&a| dom.is(a, QName::w(LocalName::Tbl)));
        let Some(tbl) = table_of(e.meta.node) else { return Vec::new() };
        s.document()
            .revisions
            .entries()
            .iter()
            .filter(|x| x.part == e.part && x.author() == e.author())
            .filter(|x| {
                matches!(
                    x.kind,
                    RevKind::TableGridChange | RevKind::CellInsert | RevKind::CellDelete
                )
            })
            .filter(|x| table_of(x.meta.node) == Some(tbl))
            .map(|x| x.id)
            .collect()
    }
    #[inline]
    /// 与这条修订同属一个字段的那几条（含它自己），按索引序。
    ///
    /// 追踪删除时**每个内容项各包一层** `w:del`（7.2：一个包裹顶替一个内容项，锚点才不动），
    /// 于是一个字段的 begin / 指令 / separate / 结果 / end 分在好几条修订里。单独接受其中一条就
    /// 丢了半个字段，另一半成孤儿——`FLD-13` 从此每次保存都失败（`TEST-07` 用两步就抓到了：
    /// 追踪删一段盖住 `REF` 字段 → 接受其中一条）。
    fn field_group(&self, e: &crate::model::RevisionEntry) -> Vec<RevisionId> {
        let s = self;
        let Some(fields) = s.document().fields_in(e.part) else { return vec![e.id] };
        let Ok(dom) = s.dom_in(Some(e.part)) else { return vec![e.id] };
        let ids_under = |node: NodeId| -> Vec<crate::span::FieldId> {
            let mut out: Vec<crate::span::FieldId> = Vec::new();
            for n in dom.descendants(node) {
                if dom.node(n).dirty == Dirty::Deleted {
                    continue;
                }
                if let Some(f) = fields.field_of(n)
                    && !out.contains(&f.id)
                {
                    out.push(f.id);
                }
            }
            out
        };
        let mine = ids_under(e.meta.node);
        if mine.is_empty() {
            return vec![e.id];
        }
        s.document()
            .revisions
            .entries()
            .iter()
            .filter(|x| x.part == e.part && x.kind == e.kind && x.author() == e.author())
            .filter(|x| x.id == e.id || ids_under(x.meta.node).iter().any(|f| mine.contains(f)))
            .map(|x| x.id)
            .collect()
    }
    #[inline]
    /// `AcceptAll` / `RejectAll`（`author` 给定时只处理那个作者的）。
    fn all(&mut self, author: Option<&str>, accept: bool) -> Result<MutationResult> {
        let s = self;
        let ordered = s.document().revisions.iter_inner_first();
        let picked: Vec<Job> = ordered
            .into_iter()
            .filter(|e| author.is_none_or(|a| e.author() == Some(a)))
            .map(|e| EditSession::job_of(s, e))
            .collect();
        // **段落标记放最后，而且倒着来**。放最后：解决它可能是"与下一段合并"，那要等这一段的
        // 内容先处理完（接受一个被搬走的段落 = 内容消失 + 标记合并 = 整段没了；反过来做会留下空段）。
        // 倒着来：连续几段都被删时，从后往前解决，每一段看到的"下一段"都已经定型了——顺着来的话
        // 第一段会先与还没消失的第二段合并，第三段就并不进来了。
        // 内容那一组保持 `iter_inner_first` 的次序，`w:ins` 套 `w:del` 的内外顺序不受影响
        let (mut marks, content): (Vec<Job>, Vec<Job>) =
            picked.into_iter().partition(|j| j.kind.is_para_mark());
        marks.reverse();
        // 网格标记排最后：先让掉格阶段同步删除当前 gridCol，再决定是否仍需恢复属性快照。
        // 反过来先缩网格再掉格，会把同一列删两遍。
        let (grid, content): (Vec<Job>, Vec<Job>) =
            content.into_iter().partition(|j| j.kind == crate::model::RevKind::TableGridChange);
        let jobs: Vec<Job> = content.into_iter().chain(grid).chain(marks).collect();
        EditSession::apply_jobs(s, jobs, accept)
    }
    #[inline]
    fn job_of(&self, e: &crate::model::RevisionEntry) -> Job {
        let s = self;
        Job {
            part: e.part,
            node: e.node(),
            kind: e.kind,
            owner: e.owner,
            move_name: e.move_name.clone(),
            pair: e.pair.and_then(|p| s.document().revisions.get(p)).map(|t| (t.part, t.node())),
        }
    }
    #[inline]
    /// 逐条处理。整批在**一个事务**里（调用方 `EditSession::apply` 已经开了），任一步 `Err`
    /// 就把每个碰过的 part 恢复到写前镜像。
    fn apply_jobs(&mut self, jobs: Vec<Job>, accept: bool) -> Result<MutationResult> {
        let s = self;
        // 记录本事务开始时的网格节点。掉格阶段已删掉对应 gridCol 时，不能再用旧快照
        // 覆盖它：存活列可能包含后来未追踪插入的列及其宽度。
        let mut grids = Vec::new();
        if !accept {
            for job in &jobs {
                if job.kind != RevKind::TableGridChange
                    || !EditSession::alive(s, job.part, job.node)?
                {
                    continue;
                }
                let dom = s.dom_in(Some(job.part))?;
                if let Some(grid) = dom.parent(job.node)
                    && let Some(table) =
                        dom.ancestors(grid).find(|&n| dom.is(n, QName::w(LocalName::Tbl)))
                {
                    let cols: Vec<_> = Dom::live_children(dom, grid)
                        .filter(|&n| dom.is(n, QName::w(LocalName::GridCol)))
                        .collect();
                    grids.push((job.part, job.node, table, cols));
                }
            }
        }
        let mut result = MutationResult::default();
        let mut done: Vec<(PartId, NodeId)> = Vec::new();
        for job in jobs {
            if done.contains(&(job.part, job.node)) {
                continue;
            }
            // 前面的步骤可能已经把它连着的子树删了（`w:ins` 里套 `w:del`，拒绝外层时内层随之消失）
            if !EditSession::alive(s, job.part, job.node)? {
                continue;
            }
            done.push((job.part, job.node));
            // 搬移的两半一起处理：孪生的方向相反
            let twin = job.pair.filter(|&(p, n)| !done.contains(&(p, n)));
            if let Some((tp, tn)) = twin {
                done.push((tp, tn));
            }
            let mut dead_spans: Vec<(PartId, SpanId)> = Vec::new();
            let reconciled_grid = grids
                .iter()
                .find(|(p, n, _, _)| (*p, *n) == (job.part, job.node))
                .is_some_and(|(_, _, _, cols)| {
                    let dom = s.dom_in(Some(job.part)).expect("已验证 part");
                    cols.iter().any(|&n| dom.node(n).dirty == Dirty::Deleted)
                });
            for plan in EditSession::plan_job(s, &job, accept, reconciled_grid, &mut dead_spans)? {
                result.absorb(s.commit_plan(plan)?);
            }
            if let Some((tp, tn)) = twin
                && EditSession::alive(s, tp, tn)?
            {
                let kind =
                    s.document().revisions.by_node(tp, tn).map(|e| e.kind).unwrap_or(job.kind);
                let twin_job = Job { part: tp, node: tn, kind, ..job.clone() };
                for plan in EditSession::plan_job(s, &twin_job, accept, false, &mut dead_spans)? {
                    result.absorb(s.commit_plan(plan)?);
                }
            }
            // 范围本身要从索引里摘掉，否则 `SPAN-09` 会在保存时按索引把标记重新物化出来
            for (part, span) in dead_spans {
                s.drop_span(part, span);
            }
        }
        // 外层事务提交前检查最终几何；包含 Dirty::New 的行，不依赖保存校验的跳过规则。
        // 任一失败由 EditSession 的完整检查点回滚，非法中间态不会发布给调用方。
        for (part, _, table, _) in grids {
            if !EditSession::alive(s, part, table)? {
                continue;
            }
            let mut pending = s.document().blocks_of_part(part).unwrap_or_default();
            let mut found = false;
            while let Some(block) = pending.pop() {
                if let crate::model::Block::Table(t) = block {
                    if t.node == table {
                        geometry(t).require_consistent()?;
                        found = true;
                        break;
                    }
                    pending.extend(t.rows.iter().flat_map(|r| &r.cells).flat_map(|c| &c.blocks));
                }
                for (blocks, here) in crate::model::box_flows(block) {
                    if here.is_none_or(|p| p == part) {
                        pending.extend(blocks);
                    }
                }
            }
            if !found {
                return Err(Error::edit(
                    DiagCode::EditTableGridInconsistent,
                    "无法验证还原后的表格网格",
                ));
            }
        }
        Ok(result)
    }
    #[inline]
    fn alive(&self, part: PartId, node: NodeId) -> Result<bool> {
        let s = self;
        let dom = s.dom_in(Some(part))?;
        Ok((node.0 as usize) < dom.node_count()
            && dom.node(node).dirty != Dirty::Deleted
            && dom.ancestors(node).all(|a| dom.node(a).dirty != Dirty::Deleted))
    }
    #[inline]
    /// 一条修订的计划（合并段落要两个阶段：先删标记再合并）。
    fn plan_job(
        &mut self,
        job: &Job,
        accept: bool,
        reconciled_grid: bool,
        dead_spans: &mut Vec<(PartId, SpanId)>,
    ) -> Result<Vec<MutationPlan>> {
        let s = self;
        let (a, r) = match (Job::row_actions(job.kind), job.owner) {
            (Some(row), RevOwner::Row(_)) => row,
            _ => actions(job.kind),
        };
        let act = if accept { a } else { r };
        // 行级的 `Drop` 动的是整行，不是标记本身
        let target = match (act, job.owner) {
            (Act::Drop, RevOwner::Row(row)) => row,
            _ => job.node,
        };
        let part = Some(job.part);
        let dom = s.dom_in(part)?;
        let mut plan = MutationPlan::new(job.part);
        match dom.ancestors(job.node).find(|&x| dom.is(x, QName::w(LocalName::P))) {
            Some(p) => plan.touch(p),
            // 不在段落里的修订（body 级 `w:sectPr` 的 `sectPrChange`、行 / 格标记…）：
            // 没有块可以刷，整体重建。解决修订不是热路径，稳比快要紧（`TEST-07` 抓到的）
            None => plan.structure_changed = true,
        }
        match act {
            Act::Unsupported => {
                return Err(Error::edit(
                    DiagCode::EditUnsupported,
                    format!("{} 的{}方向暂不支持", job.kind, if accept { "接受" } else { "拒绝" }),
                ));
            }
            Act::Unwrap | Act::UnwrapLive => {
                if act == Act::UnwrapLive {
                    Tracker::rename_to_live(&mut plan, dom, target);
                }
                MutationPlan::unwrap(&mut plan, dom, target);
            }
            Act::Drop => {
                plan.structure_changed = true;
                plan.node_edits.push(NodeEdit::Delete(target));
                if let Some(parent) = dom.parent(target) {
                    MutationPlan::keep_cell_paragraph(dom, parent, Some(target), &mut plan);
                    // 表格的最后一行也走了 → 整张表跟着走（没有行的 `w:tbl` 不合法）
                    if dom.is(target, QName::w(LocalName::Tr))
                        && Dom::live_children(dom, parent)
                            .filter(|&c| dom.is(c, QName::w(LocalName::Tr)))
                            .all(|c| c == target)
                    {
                        plan.node_edits.push(NodeEdit::Delete(parent));
                    }
                }
            }
            Act::DropMark => {
                plan.node_edits.push(NodeEdit::Delete(job.node));
                MutationPlan::drop_empty_containers(&mut plan, dom, job.node);
            }
            Act::Restore(container, keep) => {
                let c =
                    dom.parent(job.node).filter(|&c| dom.is(c, QName::w(container))).ok_or_else(
                        || Error::edit(DiagCode::EditPlanInvalid, "快照不在预期的容器里"),
                    )?;
                if container == LocalName::TblGrid && reconciled_grid {
                    // plan_drop_cells 已按当前列位置同时删除格与 gridCol；只摘掉历史标记。
                    plan.node_edits.push(NodeEdit::Delete(job.node));
                } else {
                    MutationPlan::restore(&mut plan, dom, c, job.node, container, keep);
                }
            }
            Act::Merge => {
                plan.node_edits.push(NodeEdit::Delete(job.node));
                MutationPlan::drop_empty_containers(&mut plan, dom, job.node);
                let para = dom
                    .ancestors(job.node)
                    .find(|&x| dom.is(x, QName::w(LocalName::P)))
                    .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "段落标记不在段落里"))?;
                // 内容也没剩下 → **整段消失**，而不是与下一段合并。
                //
                // 这两件事不一样：合并保留**本段**的属性（下一段的样式会丢），而"这一段整个被
                // 删掉 / 整个是插进来的"应该让下一段原样留着。段落标记排在内容之后处理
                // （见 `all`），所以这里看到的就是内容解决之后的样子。
                let empty = Dom::live_children(dom, para).all(|c| {
                    dom.name(c).is_none_or(|q| {
                        crate::span::is_property_element(q) || crate::span::is_range_marker(q)
                    })
                });
                if empty {
                    plan.structure_changed = true;
                    plan.node_edits.push(NodeEdit::Delete(para));
                    if let Some(parent) = dom.parent(para) {
                        MutationPlan::keep_cell_paragraph(dom, parent, Some(para), &mut plan);
                    }
                    return Ok(vec![plan]);
                }
                let merge = EditSession::plan_merge_with_next(s, part, para)?;
                return Ok(match merge {
                    Some(m) => vec![plan, m],
                    // 后面没有同容器的段落：只把标记去掉（Word 也只能这样）
                    None => {
                        plan.diagnostics.push(Diagnostic::pre_existing(
                            job.part,
                            None,
                            DiagCode::EditUnsupported,
                            "段落标记的修订没有可合并的下一段，只去掉标记",
                        ));
                        vec![plan]
                    }
                });
            }
            Act::DropCell => return EditSession::plan_drop_cells(s, job),
        }
        // 搬移的范围随内容一起消失（要 `&mut` 拿范围索引，所以放在 `dom` 的借用之后）
        EditSession::drop_move_range(s, &mut plan, job, dead_spans)?;
        Ok(vec![plan])
    }
    #[inline]
    /// 搬移的范围（`w:moveFromRangeStart` / `End` / `moveToRange*`）随内容一起消失：标记节点删掉，
    /// **范围本身也要从索引里摘掉**——否则 `SPAN-09` 会在保存时按索引把标记重新物化出来。
    fn drop_move_range(
        &mut self,
        plan: &mut MutationPlan,
        job: &Job,
        dead_spans: &mut Vec<(PartId, SpanId)>,
    ) -> Result<()> {
        let s = self;
        if !job.kind.is_move() {
            return Ok(());
        }
        let Some(name) = job.move_name.clone() else { return Ok(()) };
        let mut victims: Vec<NodeId> = Vec::new();
        {
            let index = s.spans_of(job.part)?;
            for sp in index.live() {
                let same = match &sp.kind {
                    RangeKind::MoveFrom { name: n, .. } | RangeKind::MoveTo { name: n, .. } => {
                        *n == name
                    }
                    _ => false,
                };
                if !same || dead_spans.iter().any(|&(p, id)| p == job.part && id == sp.id) {
                    continue;
                }
                dead_spans.push((job.part, sp.id));
                victims.extend(sp.start.and_then(|a| a.marker));
                victims.extend(sp.end.and_then(|a| a.marker));
            }
        }
        let dom = s.dom_in(Some(job.part))?;
        for m in victims {
            if dom.node(m).dirty != Dirty::Deleted {
                plan.node_edits.push(NodeEdit::Delete(m));
            }
        }
        Ok(())
    }
    #[inline]
    /// `CellInsert` 拒绝 / `CellDelete` 接受：删这个格；整列的格都带同一种标记时连
    /// `w:gridCol` 一起删，否则把左邻格加宽，保住"行的网格宽度 = `tblGrid` 列数"。
    fn plan_drop_cells(&mut self, job: &Job) -> Result<Vec<MutationPlan>> {
        let s = self;
        let part = Some(job.part);
        let dom = s.dom_in(part)?;
        let cell = dom
            .parent(job.node)
            .and_then(|tcpr| dom.parent(tcpr))
            .filter(|&c| dom.is(c, QName::w(LocalName::Tc)))
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "格标记不在 w:tc/w:tcPr 里"))?;
        let mark = dom.name(job.node).map(|q| q.local).unwrap_or(LocalName::CellIns);
        let table = dom
            .ancestors(cell)
            .find(|&a| dom.is(a, QName::w(LocalName::Tbl)))
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "格不在表格里"))?;
        let Some((col, span)) = s.cell_column(table, cell) else {
            let mut plan = MutationPlan::new(job.part);
            plan.structure_changed = true;
            plan.node_edits.push(NodeEdit::Delete(cell));
            return Ok(vec![plan]);
        };
        // 这一列上带同种标记的格；有一行没标记 → 网格不能收缩
        let mut column = s.column_cells(table, col).peekable();
        let dom = s.dom_in(part)?;
        let marked = |c: NodeId| {
            Dom::live_children(dom, c).find(|&x| dom.is(x, QName::w(LocalName::TcPr))).is_some_and(
                |tcpr| Dom::live_children(dom, tcpr).any(|m| dom.is(m, QName::w(mark))),
            )
        };
        let whole_column = column.peek().is_some() && column.all(|(_, c)| marked(c));
        let mut plan = MutationPlan::new(job.part);
        plan.structure_changed = true;
        plan.touch(table);
        if whole_column {
            // 尚未提交，模型保持不变；重新借用同一列，避免保存临时目标集合。
            for (_, c) in s.column_cells(table, col) {
                plan.node_edits.push(NodeEdit::Delete(c));
            }
            if span == 1 {
                if let Some(g) = s.grid_cols(table).nth(col as usize) {
                    plan.node_edits.push(NodeEdit::Delete(g));
                }
            }
        } else {
            // 只这一行少一个格：把它的宽度并进邻格，行的网格宽度才还对得上 `tblGrid`
            plan.node_edits.push(NodeEdit::Delete(cell));
            s.absorb_cell_width(table, cell, &mut plan);
        }
        Ok(vec![plan])
    }
}

impl MutationPlan {
    #[inline]
    /// 解包：子节点按原顺序搬到包裹的位置，包裹删掉。内容序列从 1 项变成 N 项，
    /// `SPAN-06` 的通用推导正好算得出（`Move` 插入 N、`Delete` 移除 1）。
    fn unwrap(&mut self, dom: &Dom, wrapper: NodeId) {
        let plan = self;
        let Some(parent) = dom.parent(wrapper) else { return };
        if let Some(offset) = crate::span::boundary_before(dom, parent, wrapper) {
            // 包裹本身也是内容容器；其内部边界须平移到父序列，不能按整棵删除折叠。
            plan.span.merges.push(crate::span::ContainerMerge {
                source: wrapper,
                into: parent,
                offset,
            });
        }
        plan.structure_changed = true;
        for c in dom.live_children(wrapper) {
            plan.node_edits.push(NodeEdit::Move {
                node: c,
                parent: Target::Node(parent),
                before: Some(wrapper),
            });
        }
        plan.node_edits.push(NodeEdit::Delete(wrapper));
    }
    #[inline]
    /// 删掉 `marker` 之后空掉的属性容器一路往上也删（`w:rPr` → `w:pPr`、`w:tblPrEx` …）。
    /// 那正是真实 Word 的形态，见 [`Act::droppable_empty`]。
    fn drop_empty_containers(&mut self, dom: &Dom, marker: NodeId) {
        let plan = self;
        let mut gone = vec![marker];
        let mut cur = dom.parent(marker);
        while let Some(c) = cur {
            let Some(name) = dom.name(c) else { break };
            if name.ns != NsId::W || !Act::droppable_empty(name.local) {
                break;
            }
            if Dom::live_children(dom, c).any(|x| !gone.contains(&x)) {
                break;
            }
            plan.node_edits.push(NodeEdit::Delete(c));
            gone.push(c);
            cur = dom.parent(c);
        }
    }
    #[inline]
    /// 用 `*Change` 里的快照还原容器：容器现有子元素（除 `keep` 与 `*Change` 自己）全删，
    /// 快照内层容器的子元素**整体克隆**进来。容器因此空掉时整个去掉。
    fn restore(
        &mut self,
        dom: &Dom,
        container: NodeId,
        change: NodeId,
        inner: LocalName,
        keep: RevisionKeep,
    ) {
        let plan = self;
        let mut kept = 0usize;
        for c in dom.live_children(container) {
            if c == change {
                continue;
            }
            let Some(name) = dom.name(c) else { continue };
            if name.ns == NsId::W && keep.contains(name.local) {
                kept += 1;
                continue;
            }
            plan.node_edits.push(NodeEdit::Delete(c));
        }
        let snapshot = Dom::live_children(dom, change).find(|&c| dom.is(c, QName::w(inner)));
        let mut restored = 0usize;
        if let Some(snapshot) = snapshot {
            // 还原的子元素要与**留下来的**那些排在一起（`PROP-05`）：`w:pPr` 里 `w:rPr`（33）不在
            // 快照里、原地不动，还原的 `w:ind`（22）就得插在它前面。一律插在 `*Change` 之前会排到
            // 它后面去，保存时的顺序自检当场拦下（`TEST-07` 五步就抓到：追踪改两次段落属性 +
            // 中间拒绝一次）
            let order = crate::semantic::props::TABLES
                .iter()
                .find(|t| dom.name(container) == Some(t.element))
                .map(|t| t.order_index);
            for c in dom.live_children(snapshot) {
                let before = order
                    .zip(dom.name(c).and_then(|q| order.and_then(|f| f(q))))
                    .and_then(|(f, mine)| {
                        Dom::live_children(dom, container)
                            .filter(|&k| k != change)
                            .find(|&k| dom.name(k).and_then(f).is_some_and(|i| i > mine))
                    })
                    .unwrap_or(change);
                plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::Node(container),
                    before: Some(before),
                    source: c,
                });
                restored += 1;
            }
        }
        plan.node_edits.push(NodeEdit::Delete(change));
        // 旧值是"什么都没有"→ 容器整个去掉（Word 的形态，见 `Act::droppable_empty`）
        if kept == 0
            && restored == 0
            && dom.name(container).is_some_and(|q| Act::droppable_empty(q.local))
        {
            plan.node_edits.push(NodeEdit::Delete(container));
        }
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// 这个 `w:sdt` 自己拒不拒绝内容编辑（`MOD-08` 的四态锁与数据绑定）。
    fn guard(&self, sdt: NodeId) -> Result<NodeId> {
        let s = self;
        let dom = s.dom();
        if (sdt.0 as usize) >= dom.node_count()
            || dom.node(sdt).dirty == Dirty::Deleted
            || !dom.is(sdt, QName::w(LocalName::Sdt))
        {
            return Err(Error::edit(DiagCode::EditBadPosition, "目标不是活的 w:sdt"));
        }
        let info = SdtInfo::read(dom, sdt);
        match info.refusal() {
            Some(SdtRefusal::Locked) => {
                Err(Error::edit(DiagCode::EditSdtLocked, "内容控件锁定了内容，拒绝编辑"))
            }
            Some(SdtRefusal::Bound) => {
                Err(Error::edit(DiagCode::EditSdtBound, "内容控件有数据绑定，第一阶段只读"))
            }
            None => dom
                .semantic_children(sdt)
                .find(|&c| dom.is(c, QName::w(LocalName::SdtContent)))
                .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "内容控件没有 w:sdtContent")),
        }
    }
    #[inline]
    /// `SetSdtContent`：`w:sdtContent` 里的内联整体换掉。控件里装的是块（段落 / 表格）时拒绝
    /// ——那要走 `ReplaceInlines` / `InsertBlock` 一族按块操作。
    fn set_sdt_content(
        &mut self,
        sdt: NodeId,
        inlines: &[NewInline],
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let content = EditSession::guard(s, sdt)?;
        let dom = s.dom();
        if dom
            .semantic_children(content)
            .any(|c| dom.is(c, QName::w(LocalName::P)) || dom.is(c, QName::w(LocalName::Tbl)))
        {
            return Err(unsupported("这个内容控件装的是块级内容；请对里面的段落用 ReplaceInlines"));
        }
        let para = dom
            .ancestors(content)
            .find(|&a| dom.is(a, QName::w(LocalName::P)))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "内联控件不在段落里"))?;
        EditSession::replace_container_inlines(s, para, content, inlines, ctx)
    }
    #[inline]
    /// `RemoveSdtShell`：Word 的「删除内容控件」——内容搬到父节点，`w:sdt` 本身消失。
    fn remove_sdt_shell(&mut self, sdt: NodeId) -> Result<MutationResult> {
        let s = self;
        let content = EditSession::guard(s, sdt)?;
        let dom = s.dom();
        let parent = dom
            .parent(sdt)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "w:sdt 没有父节点"))?;
        let mut plan = MutationPlan::new(s.main_part());
        plan.structure_changed = true;
        if let Some(p) = dom.ancestors(sdt).find(|&a| dom.is(a, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        for c in dom.children(content).iter().copied() {
            if dom.node(c).dirty == Dirty::Deleted || dom.element(c).is_none() {
                continue;
            }
            plan.node_edits.push(NodeEdit::Move {
                node: c,
                parent: Target::Node(parent),
                before: Some(sdt),
            });
        }
        plan.node_edits.push(NodeEdit::Delete(sdt));
        s.commit_plan(plan)
    }
}

impl MutationPlan {
    #[inline]
    /// 目标是主 part 里活着的 `w:sectPr`。
    ///
    /// "活着"要看**整条祖先链**：删掉一个段落时它的 `pPr/sectPr` 自己的 `Dirty` 不变，只有段落是
    /// `Deleted`。放过这种节点的话，后面的插入会落进一棵已经死掉的子树里，静默地什么都不发生
    /// （`compat_ts` 把 `sectionHf` 的 `lastBlockIndex` 在块操作之前解析成节点时踩到过）。
    fn require_sect_pr(dom: &Dom, sect: NodeId) -> Result<()> {
        let live = (sect.0 as usize) < dom.node_count()
            && dom.is(sect, QName::w(LocalName::SectPr))
            && dom.node(sect).dirty != Dirty::Deleted
            && dom.ancestors(sect).all(|a| dom.node(a).dirty != Dirty::Deleted);
        if live {
            Ok(())
        } else {
            Err(Error::edit(
                DiagCode::EditBadPosition,
                format!("节点 {} 不是活的 w:sectPr", sect.0),
            ))
        }
    }
    #[inline]
    /// 一个节里某个变体的引用元素（`w:headerReference` / `w:footerReference`）。
    fn reference_of(dom: &Dom, sect: NodeId, kind: HfKind, variant: HfVariant) -> Option<NodeId> {
        let elem = match kind {
            HfKind::Header => LocalName::HeaderReference,
            HfKind::Footer => LocalName::FooterReference,
        };
        dom.semantic_children(sect)
            .filter(|&n| dom.is(n, QName::w(elem)) && dom.node(n).dirty != Dirty::Deleted)
            .find(|&n| {
                // `w:type` 缺失与非 schema 的 `odd` 都算 default（`RES-10`）
                let kind = dom.attr_value(n, QName::w(LocalName::Type));
                let v = match kind.as_deref() {
                    Some("first") => HfVariant::First,
                    Some("even") => HfVariant::Even,
                    _ => HfVariant::Default,
                };
                v == variant
            })
    }
    #[inline]
    /// 引用要插在哪儿：`sectPr` 里第一个不是引用的子元素之前（`PROP-05` 的第 0 格）。
    fn reference_site(dom: &Dom, sect: NodeId) -> Option<NodeId> {
        dom.semantic_children(sect).filter(|&n| dom.node(n).dirty != Dirty::Deleted).find(|&n| {
            !dom.is(n, QName::w(LocalName::HeaderReference))
                && !dom.is(n, QName::w(LocalName::FooterReference))
        })
    }
    #[inline]
    /// 新引用元素。
    fn reference_element(kind: HfKind, variant: HfVariant, rid: &str) -> NewElement {
        let elem = match kind {
            HfKind::Header => LocalName::HeaderReference,
            HfKind::Footer => LocalName::FooterReference,
        };
        let mut e = NewElement::new(QName::w(elem));
        e.push_attr(QName::w(LocalName::Type), variant.as_str().to_string());
        e.push_attr(QName::new(NsId::R, LocalName::Id), rid.to_string());
        e
    }
    #[inline]
    /// Word 生成的斜向灰字水印段落（`v:shapetype` 136 = 文字沿路径）。照抄 TS
    /// `watermarkParagraphXml`：形状 id / `o:spid` / 样式都按 Word 的写法，编辑器与 Word 都认。
    fn watermark_paragraph_xml(text: &str) -> String {
        let escaped = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        format!(
            concat!(
                r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
                r#" xmlns:v="urn:schemas-microsoft-com:vml""#,
                r#" xmlns:o="urn:schemas-microsoft-com:office:office""#,
                r#" xmlns:w10="urn:schemas-microsoft-com:office:word">"#,
                r#"<w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:pict>"#,
                r#"<v:shapetype id="_x0000_t136" coordsize="21600,21600" o:spt="136" adj="10800""#,
                r#" path="m@7,l@8,m@5,21600l@6,21600e">"#,
                r#"<v:formulas>"#,
                r#"<v:f eqn="sum #0 0 10800"/><v:f eqn="prod #0 2 1"/><v:f eqn="sum 21600 0 @1"/>"#,
                r#"<v:f eqn="sum 0 0 @2"/><v:f eqn="sum 21600 0 @3"/><v:f eqn="if @0 @3 0"/>"#,
                r#"<v:f eqn="if @0 21600 @1"/><v:f eqn="if @0 0 @2"/><v:f eqn="if @0 @4 21600"/>"#,
                r#"<v:f eqn="mid @5 @6"/><v:f eqn="mid @8 @5"/><v:f eqn="mid @7 @8"/>"#,
                r#"<v:f eqn="mid @6 @7"/><v:f eqn="sum @6 0 @5"/>"#,
                r#"</v:formulas>"#,
                r#"<v:path textpathok="t" o:connecttype="custom""#,
                r#" o:connectlocs="@9,0;@10,10800;@11,21600;@12,10800""#,
                r#" o:connectangles="270,180,90,0"/>"#,
                r#"<v:textpath on="t" fitshape="t"/>"#,
                r##"<v:handles><v:h position="#0,bottomRight" xrange="6629,14971"/></v:handles>"##,
                r#"<o:lock v:ext="edit" text="t" shapetype="t"/>"#,
                r#"</v:shapetype>"#,
                r##"<v:shape id="PowerPlusWaterMarkObject1" o:spid="_x0000_s2049" type="#_x0000_t136""##,
                r#" style="position:absolute;left:0;text-align:left;margin-left:0;margin-top:0;"#,
                r#"width:412.4pt;height:247.45pt;rotation:315;z-index:-251656192;"#,
                r#"mso-position-horizontal:center;mso-position-horizontal-relative:margin;"#,
                r#"mso-position-vertical:center;mso-position-vertical-relative:margin""#,
                r#" o:allowincell="f" fillcolor="silver" stroked="f">"#,
                r#"<v:fill opacity=".5"/>"#,
                r#"<v:textpath style="font-family:&quot;DengXian&quot;;font-size:1pt" string="{}"/>"#,
                r#"</v:shape></w:pict></w:r></w:p>"#
            ),
            escaped
        )
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `EDIT-03 SetSectionProps`。
    fn set_section_props(
        &mut self,
        sect: NodeId,
        patch: &SectionPropsPatch,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        MutationPlan::require_sect_pr(s.dom(), sect)?;
        let mut result = MutationResult::default();
        // 追踪：旧值快照进 `w:sectPrChange`（`spec/08`）。快照**不含页眉页脚引用**——
        // `w:sectPrChange` 里的旧值是 CT_SectPrBase，没有那两个元素（`section.toml` `in_change = false`）
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let dom = s.dom();
            let mut plan = MutationPlan::new(s.main_part());
            plan.touch(sect);
            t.snapshot(
                &mut plan,
                dom,
                sect,
                LocalName::SectPrChange,
                LocalName::SectPr,
                &[LocalName::HeaderReference, LocalName::FooterReference],
            );
            if !plan.is_empty() {
                result.absorb(s.commit_plan(plan)?);
            }
        }
        let dom = s.dom();
        let mut plan = MutationPlan::new(s.main_part());
        plan.touch(sect);
        plan_apply_section_props_at(
            dom,
            Target::Node(sect),
            Some(sect),
            None,
            patch,
            s.flavor(),
            &mut plan.node_edits,
        );
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    /// `LinkHeaderFooter`：把一个**已有** part 的引用挂到这一节（TS 的 `hfAllSections`）。
    /// 该节已经声明了这个变体就什么都不做（引用已在，`MutationResult` 为空）。
    fn link_header_footer(
        &mut self,
        sect: NodeId,
        kind: HfKind,
        variant: HfVariant,
        part: PartId,
    ) -> Result<MutationResult> {
        let s = self;
        let main = s.main_part();
        let rid = s.relationship_id(main, part).ok_or_else(|| {
            Error::edit(DiagCode::EditPlanInvalid, "目标 part 与主 part 之间没有关系")
        })?;
        let dom = s.dom();
        MutationPlan::require_sect_pr(dom, sect)?;
        let mut plan = MutationPlan::new(main);
        plan.touch(sect);
        if MutationPlan::reference_of(dom, sect, kind, variant).is_none() {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(sect),
                before: MutationPlan::reference_site(dom, sect),
                node: MutationPlan::reference_element(kind, variant, &rid),
            });
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetHeaderFooter`：这一节这个变体的内容整体替换；没声明就新建 part 并插引用。
    fn set_header_footer(
        &mut self,
        sect: NodeId,
        kind: HfKind,
        variant: HfVariant,
        content: Vec<NewBlock>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        MutationPlan::require_sect_pr(s.dom(), sect)?;
        let part = ensure_hf_part(s, sect, kind, variant)?;
        EditSession::replace_part_blocks(s, part, content, ctx)
    }
    #[inline]
    /// 一个 part 的内容整体替换：原有块全 `Deleted`，新块按 `NewBlock` 生成。
    fn replace_part_blocks(
        &mut self,
        part: PartId,
        content: Vec<NewBlock>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let content = EditSession::materialize_all(s, content)?;
        let mut tracker = Tracker::new(s.document(), ctx);
        let dom = s.dom_in(Some(part))?;
        let root = dom.root();
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        for c in dom.children(root).iter().copied() {
            if dom.node(c).dirty == Dirty::Deleted || dom.element(c).is_none() {
                continue;
            }
            match &mut tracker {
                // 追踪：这个 part 里按段落规则标删（`spec/18` 7.3），原块留着
                Some(t) => MutationPlan::plan_delete_block_tracked(&mut plan, dom, t, c),
                None => plan.node_edits.push(NodeEdit::Delete(c)),
            }
        }
        for block in content {
            let opaque = matches!(block, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
            let node = MutationPlan::new_block_element(dom, block);
            let node = match &mut tracker {
                Some(t) => t.mark_new_block_inserted(node, opaque),
                None => node,
            };
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node,
            });
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetPageColor`：`w:background` 是 `w:document` 的第一个子元素（在 `w:body` 之前）。
    fn set_page_color(&mut self, color: Option<String>) -> Result<MutationResult> {
        let s = self;
        let dom = s.dom();
        let root = dom.root();
        let existing = dom.semantic_children(root).find(|&n| {
            dom.is(n, QName::w(LocalName::Background)) && dom.node(n).dirty != Dirty::Deleted
        });
        let mut plan = MutationPlan::new(s.main_part());
        match (color, existing) {
            (None, Some(n)) => plan.node_edits.push(NodeEdit::Delete(n)),
            (None, None) => {}
            (Some(c), Some(n)) => plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(n),
                name: QName::w(LocalName::Color),
                value: c,
            }),
            (Some(c), None) => {
                let mut e = NewElement::new(QName::w(LocalName::Background));
                e.push_attr(QName::w(LocalName::Color), c);
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(root),
                    before: dom.semantic_children(root).next(),
                    node: e,
                });
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `SetWatermark`：default 页眉里的文字水印（Word 的水印就是页眉里的一个 VML 形状）。
    ///
    /// Strict 包拒绝：VML 不在 Strict 里，DrawingML 水印不在 M5（`docs/03` §14）。
    fn set_watermark(&mut self, sect: NodeId, text: Option<String>) -> Result<MutationResult> {
        let s = self;
        if text.is_some() && s.flavor() == PartFlavor::Strict {
            return Err(Error::edit(
                DiagCode::EditUnsupported,
                "Strict 包不能写 VML 水印（DrawingML 水印不在 M5）",
            ));
        }
        MutationPlan::require_sect_pr(s.dom(), sect)?;
        // 删水印时页眉不存在就什么都不用做
        let existing = {
            let dom = s.dom();
            MutationPlan::reference_of(dom, sect, HfKind::Header, HfVariant::Default)
                .and_then(|n| dom.attr_value(n, QName::new(NsId::R, LocalName::Id)))
                .and_then(|rid| s.document().hf_by_rel.get(rid.as_ref()).copied())
        };
        let part = match (existing, &text) {
            (Some(p), _) => p,
            (None, None) => return Ok(MutationResult::default()),
            (None, Some(_)) => ensure_hf_part(s, sect, HfKind::Header, HfVariant::Default)?,
        };
        // 先把只读的活干完（要删哪些段落、插在哪儿），再动可变借用（片段解析要 `&mut Dom`）
        let (root, doomed, before) = {
            let dom = s.dom_in(Some(part))?;
            let root = dom.root();
            let doomed: Vec<NodeId> = dom
                .children(root)
                .iter()
                .copied()
                .filter(|&c| {
                    dom.node(c).dirty != Dirty::Deleted && dom.is(c, QName::w(LocalName::P))
                })
                .filter(|&c| {
                    dom.semantic_descendants(c)
                        .any(|n| dom.is(n, QName::new(NsId::V, LocalName::Textpath)))
                })
                .collect();
            let before = dom.children(root).iter().copied().find(|&c| dom.element(c).is_some());
            (root, doomed, before)
        };
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        // 原有的水印段落（含 `v:textpath` 的 `w:p`）先删掉
        plan.node_edits.extend(doomed.into_iter().map(NodeEdit::Delete));
        if let Some(t) = text {
            // 水印段落放在最前（Word 的位置）；那棵 VML 子树用片段解析，不手拼字符串
            let xml = MutationPlan::watermark_paragraph_xml(&t);
            let node = s.new_element_from_xml(part, &xml)?;
            plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(root), before, node });
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `InsertSectionBreak`：在 `after` 这一段之后断节。
    ///
    /// 形态与真实 Word 一致（`fixtures/word-ops/insert-next-page`）：段落 `pPr` 里新建的
    /// `w:sectPr` 是**原节属性的克隆**（含页眉页脚引用——第一节因此保住自己的页眉），
    /// 原来那个 `sectPr` 从此描述后一节。`w:type` 只在不是缺省的 `nextPage` 时才写
    /// （Word 也不写缺省值）。
    fn insert_section_break(
        &mut self,
        after: NodeId,
        kind: crate::semantic::props::SectType,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let main = s.main_part();
        let dom = s.dom();
        if (after.0 as usize) >= dom.node_count()
            || dom.node(after).dirty == Dirty::Deleted
            || !dom.is(after, QName::w(LocalName::P))
        {
            return Err(Error::edit(DiagCode::EditBadPosition, "分节符要加在一个活的 w:p 之后"));
        }
        // 段落必须是块容器的直接子节点：Word 也不允许在单元格里分节
        let parent = dom
            .parent(after)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "段落没有父节点"))?;
        if !dom.is(parent, QName::w(LocalName::Body))
            && !dom.is(parent, QName::w(LocalName::SdtContent))
        {
            return Err(Error::edit(
                DiagCode::EditBadPosition,
                "只能在正文（或内容控件）的直接子段落之后分节；单元格里不能分节",
            ));
        }
        // 管辖这一段的节的活 `sectPr`
        let idx = s
            .document()
            .section_of(dom, after)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "找不到管辖这一段的节"))?;
        let source = s.document().sections[idx]
            .node
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "这一节是隐式节，没有 sectPr"))?;
        if dom.ancestors(source).any(|a| a == after) {
            return Err(Error::edit(DiagCode::EditBadPosition, "这一段已经是分节段落了"));
        }
        not_tracked(ctx, "InsertSectionBreak");
        let ppr = MutationPlan::ppr_of(dom, after);
        let mut plan = MutationPlan::new(main);
        plan.structure_changed = true;
        plan.touch(after);
        // `PROP-05`：`w:sectPr` 在 `w:rPr` 之后、`w:pPrChange` 之前
        let target = match ppr {
            Some(p) => {
                let before = dom
                    .semantic_children(p)
                    .filter(|&c| dom.node(c).dirty != Dirty::Deleted)
                    .find(|&c| dom.is(c, QName::w(LocalName::PPrChange)));
                plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::Node(p),
                    before,
                    source,
                });
                None
            }
            None => {
                let first = dom
                    .children(after)
                    .iter()
                    .copied()
                    .find(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some());
                let k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(after),
                    before: first,
                    node: NewElement::new(QName::w(LocalName::PPr)),
                });
                plan.node_edits.push(NodeEdit::InsertClone {
                    parent: Target::New(k),
                    before: None,
                    source,
                });
                Some(k)
            }
        };
        let _ = target;
        let mut result = s.commit_plan(plan)?;
        // 原来的 `sectPr` 现在描述**后**一节：它的 `w:type` 就是这次断节的方式
        if kind != crate::semantic::props::SectType::NextPage {
            let patch = crate::semantic::props::SectionPropsPatch {
                kind: crate::semantic::props::Change::Set(crate::semantic::props::Val::Value(kind)),
                ..Default::default()
            };
            result.absorb(EditSession::set_section_props(
                s,
                source,
                &patch,
                &EditContext::default(),
            )?);
        }
        Ok(result)
    }
    #[inline]
    /// `DeleteSectionBreak`：删掉一个段落级 `w:sectPr`。
    fn delete_section_break(&mut self, sect: NodeId, ctx: &EditContext) -> Result<MutationResult> {
        let s = self;
        let main = s.main_part();
        let dom = s.dom();
        MutationPlan::require_sect_pr(dom, sect)?;
        let ppr = dom
            .parent(sect)
            .filter(|&p| dom.is(p, QName::w(LocalName::PPr)))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "body 级的 sectPr 不能删"))?;
        not_tracked(ctx, "DeleteSectionBreak");
        let mut plan = MutationPlan::new(main);
        plan.structure_changed = true;
        if let Some(p) = dom.parent(ppr) {
            plan.touch(p);
        }
        plan.node_edits.push(NodeEdit::Delete(sect));
        let live = |n: NodeId| {
            dom.children(n)
                .iter()
                .copied()
                .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
        };
        // 段落**没有内容**（只有一个 `w:pPr`）→ 整段消失。那正是 Word 的形态：
        // `fixtures/word-ops/delete-break` 的 `after.docx` 比 `before.docx` 少一个 `w:p`、文字一个不少
        // ——分节符那一行本来就是一个只带 `sectPr` 的空段（它的 `pPr` 里还有段落标记的 `rPr`，
        // 所以判据看的是**段落有没有内容**，不是 `pPr` 空不空）。
        // 段落里还有内容时只去掉 `sectPr`，内容留给后一节（不做破坏性的合并）。
        if let Some(para) = dom.parent(ppr).filter(|&x| dom.is(x, QName::w(LocalName::P)))
            && live(para).all(|c| c == ppr)
        {
            plan.node_edits.push(NodeEdit::Delete(para));
        } else if live(ppr).all(|c| c == sect) {
            plan.node_edits.push(NodeEdit::Delete(ppr));
        }
        s.commit_plan(plan)
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    fn run(&mut self, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
        let s = self;
        EditSession::guard_sdt(s, &op)?;
        EditSession::guard_main_only(s, &op)?;
        // 7.7：位置落在 `mc:Fallback` 里直接拒；落在 `mc:Choice` 的文本框里的记下来，提交后同步孪生
        let targets = s.op_targets(&op);
        let twins = sites(s, targets.clone())?;
        // 几何与样式不改内容，改的是 VML 那边的 `@style` / `@fillcolor` / `@strokecolor`
        let styled = matches!(op, EditOp::SetDrawingGeometry { .. } | EditOp::SetShapeStyle { .. })
            .then_some(targets);
        // `spec/18` 7.3：Word 自己也不把这些记成修订（或另有机制）。照常执行，留一条
        // `REV_NOT_TRACKED`——编辑器开着修订时改页面颜色不该失败（分层决策 5）
        if ctx.track_changes.is_some()
            && let Some(what) = EditOp::not_tracked_name(&op)
        {
            let part = s.main_part();
            s.record(vec![Diagnostic::pre_existing(
                part,
                None,
                DiagCode::RevNotTracked,
                format!("{what} 不产生修订（Word 也不记，或另有机制）"),
            )]);
        }
        let mut result = EditSession::dispatch(s, op, ctx)?;
        if !twins.is_empty() {
            result.absorb(sync(s, &twins)?);
        }
        if let Some(targets) = styled {
            result.absorb(sync_shape_style(s, targets)?);
        }
        Ok(result)
    }
    #[inline]
    fn dispatch(&mut self, op: EditOp, ctx: &EditContext) -> Result<MutationResult> {
        let s = self;
        match op {
            EditOp::SetSources { sources } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    sources: Some(sources),
                    ..Default::default()
                },
            ),
            EditOp::AddNumberingDefinition { definition } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    numbering_new_defs: vec![definition],
                    ..Default::default()
                },
            ),
            EditOp::RestartNumbering { restart } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    numbering_restart_nums: vec![restart],
                    ..Default::default()
                },
            ),
            EditOp::SetThemeFonts { fonts } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    theme_fonts: Some(fonts),
                    ..Default::default()
                },
            ),
            EditOp::SetThemeColors { colors } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    theme_colors: Some(colors),
                    ..Default::default()
                },
            ),
            EditOp::UpsertStyle { style } => EditSession::declarations(
                s,
                crate::save::options::CompatSaveOptions {
                    style_upserts: vec![style],
                    ..Default::default()
                },
            ),
            EditOp::InsertText { at, text, props } => {
                EditSession::insert_text(s, at, &text, props, ctx)
            }
            EditOp::DeleteRange { from, to } => EditSession::delete_range(s, from, to, ctx),
            EditOp::SetRunProps { from, to, patch } => {
                EditSession::set_run_props(s, from, to, &patch, ctx)
            }
            EditOp::ReplaceInlines { part, para, inlines } => {
                EditSession::replace_inlines(s, part, para, &inlines, ctx)
            }
            EditOp::SetParaProps { part, para, patch } => {
                EditSession::set_para_props(s, part, para, &patch, ctx)
            }
            EditOp::ReplaceParaProps { part, para, props } => {
                EditSession::replace_para_props(s, part, para, props, ctx)
            }
            EditOp::InsertRow { table, at, template } => {
                EditSession::insert_row(s, table, at, template, ctx)
            }
            EditOp::DeleteRow { table, at } => EditSession::delete_row(s, table, at, ctx),
            EditOp::InsertColumn { table, at, width } => {
                EditSession::insert_column(s, table, at, width, ctx)
            }
            EditOp::DeleteColumn { table, at } => EditSession::delete_column(s, table, at, ctx),
            EditOp::MergeCells { table, from, to } => {
                EditSession::merge_cells(s, table, from, to, ctx)
            }
            EditOp::SetTableProps { table, patch } => set_table_props(s, table, &patch, ctx),
            EditOp::SetRowProps { row, patch } => set_row_props(s, row, &patch, ctx),
            EditOp::SetCellProps { cell, patch } => set_cell_props(s, cell, &patch, ctx),
            EditOp::InsertBlock { at, block } => EditSession::insert_block(s, at, block, ctx),
            EditOp::DeleteBlock { part, node } => EditSession::delete_block(s, part, node, ctx),
            EditOp::MoveBlock { from, node, to } => EditSession::move_block(s, from, node, to, ctx),
            EditOp::AddComment { from, to, comment } => {
                EditSession::add_comment(s, from, to, &comment)
            }
            EditOp::RemoveComment { id } => EditSession::remove_comment(s, &id),
            EditOp::SetCommentText { id, text, done } => {
                EditSession::set_comment_text(s, &id, &text, done)
            }
            EditOp::SplitParagraph { at } => EditSession::split_paragraph(s, at, ctx),
            EditOp::MergeWithNext { part, para } => {
                EditSession::merge_with_next(s, part, para, ctx)
            }
            EditOp::AddBookmark { name, from, to } => EditSession::add_bookmark(s, &name, from, to),
            EditOp::RemoveBookmark { name } => EditSession::remove_bookmark(s, &name),
            EditOp::InsertField { at, field } => EditSession::insert_field(s, at, &field, ctx),
            EditOp::SetLinkTarget { link, target } => {
                EditSession::set_link_target(s, link, &target, ctx)
            }
            EditOp::ToggleCheckbox { field } => EditSession::toggle_checkbox(s, field),
            EditOp::SetFormText { field, text } => EditSession::set_form_text(s, field, &text, ctx),
            EditOp::SetFieldResultProps { field, patch } => {
                EditSession::set_field_result_props(s, field, &patch, ctx)
            }
            EditOp::UpdateBlockField { field, blocks } => {
                EditSession::update_block_field(s, field, blocks, ctx)
            }
            EditOp::SetSectionProps { sect, patch } => {
                EditSession::set_section_props(s, sect, &patch, ctx)
            }
            EditOp::SetHeaderFooter { sect, kind, variant, content } => {
                EditSession::set_header_footer(s, sect, kind, variant, content, ctx)
            }
            EditOp::LinkHeaderFooter { sect, kind, variant, part } => {
                EditSession::link_header_footer(s, sect, kind, variant, part)
            }
            EditOp::SetWatermark { sect, text } => EditSession::set_watermark(s, sect, text),
            EditOp::SetPageColor { color } => EditSession::set_page_color(s, color),
            EditOp::SetDocumentSettings { patch } => EditSession::set_document_settings(s, &patch),
            EditOp::SetChartData { part, patch } => EditSession::set_chart_data(s, part, &patch),
            EditOp::ReplacePartXml { part, xml } => {
                s.replace_part_xml(part, &xml)?;
                Ok(MutationResult::default())
            }
            EditOp::ReplacePartBytes { part, bytes } => {
                s.replace_part_bytes(part, bytes)?;
                Ok(MutationResult::default())
            }
            EditOp::ReplaceImageMedia { drawing, bytes, mime } => {
                s.replace_image_media(drawing, bytes, &mime, ctx)
            }
            EditOp::RemoveInks => s.remove_inks(),
            EditOp::InsertInk { para, ink } => s.insert_ink(para, &ink),
            EditOp::InsertAtom { at, atom } => EditSession::insert_atom(s, at, &atom, ctx),
            EditOp::SetNoteContent { endnote, id, content } => {
                EditSession::set_note_content(s, endnote, &id, &content)
            }
            EditOp::RemoveNote { endnote, id } => EditSession::remove_note(s, endnote, &id),
            EditOp::SetSdtContent { sdt, inlines } => {
                EditSession::set_sdt_content(s, sdt, &inlines, ctx)
            }
            EditOp::RemoveSdtShell { sdt } => EditSession::remove_sdt_shell(s, sdt),
            EditOp::SetMathTokens { math, tokens } => {
                EditSession::set_math_tokens(s, math, &tokens)
            }
            EditOp::SetDrawingGeometry { drawing, geom } => {
                EditSession::set_geometry(s, drawing, &geom)
            }
            EditOp::SetDrawingZOrder { drawing, z } => EditSession::set_z_order(s, drawing, z),
            EditOp::SetDrawingWrap { drawing, wrap, pos, z_order } => {
                EditSession::set_wrap(s, drawing, wrap, pos.as_ref(), z_order)
            }
            EditOp::SetShapeStyle { shape, fill, outline } => {
                EditSession::set_shape_style(s, shape, fill, outline)
            }
            EditOp::SetTextboxContent { textbox, blocks } => {
                set_textbox_content(s, textbox, blocks, ctx)
            }
            EditOp::RegenerateBlockField { field, options } => {
                EditSession::regenerate(s, field, options, ctx)
            }
            EditOp::InsertSectionBreak { after, kind } => {
                EditSession::insert_section_break(s, after, kind, ctx)
            }
            EditOp::DeleteSectionBreak { sect } => EditSession::delete_section_break(s, sect, ctx),
            EditOp::AcceptRevision { rev } => EditSession::one(s, rev, true),
            EditOp::RejectRevision { rev } => EditSession::one(s, rev, false),
            EditOp::AcceptAll { author } => EditSession::all(s, author.as_deref(), true),
            EditOp::RejectAll { author } => EditSession::all(s, author.as_deref(), false),
        }
    }
    #[inline]
    /// `EDIT-03 SetDocumentSettings`：`word/settings.xml` 按 `PROP-06` 合并；part 不存在就按
    /// `SAVE-05` 建（`evenAndOddHeaders` / 保护标志要有地方写）。
    fn set_document_settings(
        &mut self,
        patch: &crate::semantic::props::SettingsPatch,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.ensure_settings_part()?;
        let dom = s.dom_in(Some(part))?;
        let root = dom.root();
        let mut plan = MutationPlan::new(part);
        plan.node_edits = crate::semantic::props::plan_apply_settings(
            dom,
            root,
            Some(root),
            patch,
            s.flavor_in(Some(part)),
        );
        let result = s.commit_plan(plan)?;
        // 声明属性的 SetAttr 没有 affected_blocks；不能依赖块刷新更新 settings。
        s.rebuild()?;
        Ok(result)
    }
    #[inline]
    /// 只支持主 part 的操作（书签 / 批注 / 字段：它们的索引与 id 都只对主 part 建过）。位置带别的
    /// part 时明确拒绝，而不是悄悄去改主 part 的同号节点（任务 5.5）。
    fn guard_main_only(&self, op: &EditOp) -> Result<()> {
        let s = self;
        let main = s.main_part();
        let foreign = |p: &Option<PartId>| p.is_some_and(|x| x != main);
        let bad = match op {
            EditOp::AddBookmark { from, to, .. } | EditOp::AddComment { from, to, .. } => {
                foreign(&from.part) || foreign(&to.part)
            }
            EditOp::InsertField { at, .. } => foreign(&at.part),
            _ => false,
        };
        if bad {
            return Err(Error::edit(
                DiagCode::EditUnsupported,
                "该操作暂只支持主 part（书签 / 批注 / 字段的索引只对正文建）",
            ));
        }
        Ok(())
    }
    #[inline]
    /// `EDIT-03` / `MOD-08`：编辑目标落在只读（`contentLocked` / `sdtContentLocked`）或数据绑定的内容
    /// 控件里 → 整体拒绝，状态不变（`EDIT-05`）。第一阶段绑定控件一律只读：显示文字只是 customXml 的
    /// 缓存，改了 Word 重开会刷回去。
    fn guard_sdt(&self, op: &EditOp) -> Result<()> {
        let s = self;
        for (part, node) in s.op_targets(op) {
            let dom = s.dom_in(part)?;
            // 目标节点根本不在这个 part 里（调用方给了别的 part 的 `NodeId`，或者早就被删了）：
            // 这里不能走祖先链——那会越界 panic（不变式 4）。让操作自己去拒
            if (node.0 as usize) >= dom.node_count() {
                continue;
            }
            let Some((info, why)) = refusing_sdt(dom, node) else { continue };
            let what = info
                .alias
                .clone()
                .or_else(|| info.tag.clone())
                .unwrap_or_else(|| info.control.as_str().to_string());
            return Err(match why {
                SdtRefusal::Locked => Error::edit(
                    DiagCode::EditSdtLocked,
                    format!("内容控件 `{what}` 的 w:lock 是 {}，内容只读", info.lock),
                ),
                SdtRefusal::Bound => Error::edit(
                    DiagCode::EditSdtBound,
                    format!("内容控件 `{what}` 绑定了 customXml 数据，第一阶段不可编辑"),
                ),
            });
        }
        Ok(())
    }
    #[inline]
    /// 某个 part 里的文本段落投影（`None` = 主 part，任务 5.5）。
    fn require_text_block(&self, part: Option<PartId>, para: NodeId) -> Result<&TextBlock> {
        let s = self;
        s.text_block_in(part, para).ok_or_else(|| {
            Error::edit(
                DiagCode::EditBadPosition,
                format!("节点 {} 在 part {} 里不是文本段落", para.0, s.part_or_main(part).0),
            )
        })
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditOp {
    #[inline]
    /// 追踪时**不产生修订**的操作（`spec/18` 7.3 的清单）：书签、批注、复选框、页眉链接、
    /// 水印、页面底色、文档设置、图表数据、part 替换、墨迹。绘图几何与样式（7.7）到时候一起加。
    fn not_tracked_name(&self) -> Option<&'static str> {
        let op = self;
        Some(match op {
            EditOp::AddBookmark { .. } => "AddBookmark",
            EditOp::RemoveBookmark { .. } => "RemoveBookmark",
            EditOp::AddComment { .. } => "AddComment",
            EditOp::RemoveComment { .. } => "RemoveComment",
            EditOp::SetCommentText { .. } => "SetCommentText",
            EditOp::ToggleCheckbox { .. } => "ToggleCheckbox",
            EditOp::LinkHeaderFooter { .. } => "LinkHeaderFooter",
            EditOp::SetWatermark { .. } => "SetWatermark",
            EditOp::SetPageColor { .. } => "SetPageColor",
            EditOp::SetDocumentSettings { .. } => "SetDocumentSettings",
            // 7.7：Word 不把图片的格式改动记成修订
            EditOp::SetDrawingGeometry { .. } => "SetDrawingGeometry",
            EditOp::SetDrawingZOrder { .. } => "SetDrawingZOrder",
            EditOp::SetDrawingWrap { .. } => "SetDrawingWrap",
            EditOp::SetShapeStyle { .. } => "SetShapeStyle",
            EditOp::InsertSectionBreak { .. } => "InsertSectionBreak",
            EditOp::DeleteSectionBreak { .. } => "DeleteSectionBreak",
            EditOp::SetSources { .. } => "SetSources",
            EditOp::AddNumberingDefinition { .. } => "AddNumberingDefinition",
            EditOp::RestartNumbering { .. } => "RestartNumbering",
            EditOp::SetThemeFonts { .. } => "SetThemeFonts",
            EditOp::SetThemeColors { .. } => "SetThemeColors",
            EditOp::UpsertStyle { .. } => "UpsertStyle",
            EditOp::SetChartData { .. } => "SetChartData",
            EditOp::ReplacePartXml { .. } => "ReplacePartXml",
            EditOp::ReplacePartBytes { .. } => "ReplacePartBytes",
            EditOp::InsertInk { .. } => "InsertInk",
            EditOp::RemoveInks => "RemoveInks",
            _ => return None,
        })
    }
}

impl MutationPlan {
    #[inline]
    fn rpr_of(dom: &Dom, run: NodeId) -> Option<NodeId> {
        Dom::live_children_named(dom, run, QName::w(LocalName::RPr)).next()
    }
    #[inline]
    fn ppr_of(dom: &Dom, para: NodeId) -> Option<NodeId> {
        Dom::live_children_named(dom, para, QName::w(LocalName::PPr)).next()
    }
}

impl MutationPlan {
    #[inline]
    fn clone_attrs(dom: &Dom, from: NodeId, to: &mut NewElement) {
        if let Some(e) = dom.element(from) {
            for a in &e.attrs {
                to.push_attr(a.name, dom.attr_str(a).into_owned());
            }
        }
    }
    #[inline]
    /// 段文本设为 `text`：提交时即保留边界空白，不能等到保存才补 preserve。
    fn set_segment_text(dom: &Dom, seg_node: NodeId, text: &str, plan: &mut MutationPlan) {
        let preserve = dom.name(seg_node).is_some_and(|name| {
            name.ns == NsId::W
                && matches!(
                    name.local,
                    LocalName::T
                        | LocalName::DelText
                        | LocalName::InstrText
                        | LocalName::DelInstrText
                )
        });
        match Dom::sole_live_text_child(dom, seg_node) {
            Some(tn) => {
                plan.node_edits.push(NodeEdit::SetText { node: tn, text: text.to_string() });
                // 否则连续编辑间的模型重建会裁掉新增的首尾空白，令后续 UTF-16 坐标漂移。
                if preserve {
                    plan.node_edits.push(NodeEdit::SetAttr {
                        node: Target::Node(seg_node),
                        name: QName::new(NsId::Xml, LocalName::Space),
                        value: "preserve".into(),
                    });
                }
            }
            None => {
                let name = dom.name(seg_node).expect("segment is an element");
                let mut e = NewElement::new(name);
                MutationPlan::clone_attrs(dom, seg_node, &mut e);
                if preserve {
                    e.attrs.retain(|(name, _)| *name != QName::new(NsId::Xml, LocalName::Space));
                    e.push_attr(QName::new(NsId::Xml, LocalName::Space), "preserve");
                }
                plan.node_edits.push(NodeEdit::Replace { old: seg_node, node: e.with_text(text) });
            }
        }
    }
    #[inline]
    /// 把另一份计划的编辑追加进来：`Target::New(k)` 按偏移重定位。
    fn append_edits(&mut self, mut edits: Vec<NodeEdit>) {
        let plan = self;
        let base = plan.node_edits.len();
        let shift = |t: &mut Target| {
            if let Target::New(k) = t {
                *k += base;
            }
        };
        for e in &mut edits {
            match e {
                NodeEdit::Insert { parent, .. }
                | NodeEdit::InsertClone { parent, .. }
                | NodeEdit::Move { parent, .. } => shift(parent),
                NodeEdit::SetAttr { node, .. } | NodeEdit::RemoveAttr { node, .. } => shift(node),
                NodeEdit::Replace { .. }
                | NodeEdit::ReplaceClone { .. }
                | NodeEdit::Delete(_)
                | NodeEdit::Rename { .. }
                | NodeEdit::SetText { .. } => {}
            }
        }
        plan.node_edits.extend(edits);
    }
    #[inline]
    /// `EDIT-06`：文档内全部修订 `w:id` 的最大值 + 1（M1 只看主 part）。
    fn next_revision_id(dom: &Dom) -> u32 {
        let mut max = 0u32;
        for n in dom.descendants(dom.root()) {
            let node = dom.node(n);
            if node.dirty == Dirty::Deleted {
                continue;
            }
            let NodeKind::Element(e) = &node.kind else { continue };
            if e.name.ns != NsId::W
                || !matches!(
                    e.name.local,
                    LocalName::Ins
                        | LocalName::Del
                        | LocalName::MoveFrom
                        | LocalName::MoveTo
                        | LocalName::RPrChange
                        | LocalName::PPrChange
                        | LocalName::SectPrChange
                        | LocalName::TblPrChange
                        | LocalName::TblGridChange
                        | LocalName::TcPrChange
                        | LocalName::TblPrExChange
                        | LocalName::NumberingChange
                        | LocalName::CellIns
                        | LocalName::CellDel
                        | LocalName::CellMerge
                        | LocalName::MoveFromRangeStart
                        | LocalName::MoveToRangeStart
                )
            {
                continue;
            }
            if let Some(v) = dom.attr_value(n, QName::w(LocalName::Id))
                && let Ok(id) = v.trim().parse::<u32>()
            {
                max = max.max(id);
            }
        }
        max + 1
    }
    #[inline]
    /// 追踪时新 run 该落在哪里（`spec/18` 7.2「同作者规则」）：
    ///
    /// - 本作者自己的 `w:ins` 里 → 直接插，不套第二层；
    /// - 别人的 `w:ins` 里 → **拆开**外层（属性克隆、换新 `w:id`），把新的 `w:ins` 夹在中间——
    ///   否则按作者拒绝时会把两个人的字一起撤掉；
    /// - 其余 → 新建一个 `w:ins` 包住。
    fn plan_ins_site(
        &mut self,
        dom: &Dom,
        t: &mut Tracker,
        para: NodeId,
        parent: NodeId,
        before: Option<NodeId>,
    ) -> Result<(Target, Option<NodeId>)> {
        let plan = self;
        let wrap = |plan: &mut MutationPlan, t: &mut Tracker, parent: NodeId, before| {
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before,
                node: t.marker(LocalName::Ins),
            });
            (Target::New(k), None)
        };
        match track_site_of(dom, parent, para, &t.author) {
            TrackSite::Deleted(_) => Err(err_in_deleted()),
            TrackSite::OwnIns(_) => Ok((Target::Node(parent), before)),
            // 位置嵌在别人 `w:ins` 内更深的容器里（超链接、smartTag …）：拆不动外层，
            // 退化成内层再套一个 `w:ins`（形态合法，接受 / 拒绝都正确，只是按作者拒绝外层会连带）
            TrackSite::OtherIns(ins) if ins != parent => Ok(wrap(plan, t, parent, before)),
            TrackSite::OtherIns(ins) => {
                let gp = dom.parent(ins).ok_or_else(|| unsupported("w:ins 没有父节点"))?;
                let kids: Vec<NodeId> = Dom::live_children(dom, ins).collect();
                let cut = before.and_then(|b| kids.iter().position(|&k| k == b));
                match cut {
                    // 落在末尾：整个 `w:ins` 之后另起一个
                    None => Ok(wrap(plan, t, gp, Dom::next_live_sibling(dom, ins))),
                    // 落在开头：整个 `w:ins` 之前另起一个
                    Some(0) => Ok(wrap(plan, t, gp, Some(ins))),
                    Some(i) => {
                        let after = Dom::next_live_sibling(dom, ins);
                        let (target, _) = wrap(plan, t, gp, after);
                        // 右半：同名同属性、新 `w:id`；插在同一个 `before` 上 → 落在我们这一段之后
                        let k2 = plan.node_edits.len();
                        let right = t.clone_marker(dom, ins);
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(gp),
                            before: after,
                            node: right,
                        });
                        for &c in &kids[i..] {
                            plan.node_edits.push(NodeEdit::Move {
                                node: c,
                                parent: Target::New(k2),
                                before: None,
                            });
                        }
                        Ok((target, None))
                    }
                }
            }
            TrackSite::Clean => Ok(wrap(plan, t, parent, before)),
        }
    }
}

impl Tracker {
    #[inline]
    /// 字段 / 批注 / 脚注结构段：删除范围覆盖时原地保留（M2 由 `FieldSpan` 接管）。
    /// 字段结构段（`fldChar` / 指令文本）：`is_structural` 的子集。
    fn is_field_structure(kind: &SegmentKind) -> bool {
        matches!(kind, SegmentKind::FldChar | SegmentKind::InstrText | SegmentKind::DelInstrText)
    }
    #[inline]
    fn is_structural(kind: &SegmentKind) -> bool {
        matches!(
            kind,
            SegmentKind::FldChar
                | SegmentKind::InstrText
                | SegmentKind::DelInstrText
                | SegmentKind::CommentRef
                | SegmentKind::AnnotationRef
                | SegmentKind::FootnoteRefMark
                | SegmentKind::EndnoteRefMark
                | SegmentKind::Separator
                | SegmentKind::ContinuationSeparator
        )
    }
    #[inline]
    fn in_deleted_run(run: &Run) -> bool {
        run.rev.as_ref().is_some_and(|r| r.del.is_some())
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// 在 `run.segments[seg]` 的 `byte` 处拆分：左半留在原 run（文本 `Owned` 截断），右半为 `New` run，
    /// `rPr` 为字节克隆（`XML-12` 规则 F），其后的段整段克隆过去、原节点 `Deleted`。
    /// `byte == 0` → 段 `seg` 整段归右；`byte ≥ len` → 归左。新 run 是 `created[0]`。
    fn split_run(
        &self,
        part: Option<PartId>,
        para: NodeId,
        run: &Run,
        seg: usize,
        byte: usize,
    ) -> MutationPlan {
        let s = self;
        let dom = s.dom_in(part).expect("caller resolved the part");
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        // 后半是原 run 的延续：该边界上的 `Left` 锚点也要右移（`SPAN-06` 的补充，见 `SpanPolicy`）
        plan.span.split_items.push(run.node);
        let parent = dom.parent(run.node).expect("run has a parent");
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before: Dom::next_live_sibling(dom, run.node),
            node: NewElement::new(QName::w(LocalName::R)),
        });
        if let Some(rpr) = MutationPlan::rpr_of(dom, run.node) {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k),
                before: None,
                source: rpr,
            });
        }
        let segs = &run.segments;
        let text = run.segment_text(&segs[seg]);
        let move_from = if byte == 0 {
            seg
        } else if byte >= text.len() {
            seg + 1
        } else {
            MutationPlan::set_segment_text(dom, segs[seg].node, &text[..byte], &mut plan);
            let name = dom.name(segs[seg].node).expect("segment is an element");
            let mut e = NewElement::new(name);
            MutationPlan::clone_attrs(dom, segs[seg].node, &mut e);
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: e.with_text(&text[byte..]),
            });
            seg + 1
        };
        for sg in &segs[move_from.min(segs.len())..] {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k),
                before: None,
                source: sg.node,
            });
            plan.node_edits.push(NodeEdit::Delete(sg.node));
        }
        plan
    }
    #[inline]
    /// 位置若在某个 run 内部（段间或文本段内），先拆分；返回 `(左 run, 右 run)`。边界位置返回 `None`。
    fn split_at(
        &mut self,
        at: InlinePos,
        loc: Loc,
        result: &mut MutationResult,
    ) -> Result<Option<(NodeId, NodeId)>> {
        let s = self;
        EditSession::split_at_maybe_deleted(s, at, loc, result, false)
    }
    #[inline]
    /// 同上；`in_deleted` 为真时允许在已删除的文字里拆分（追踪路径要把已删区间也切齐）。
    fn split_at_maybe_deleted(
        &mut self,
        at: InlinePos,
        loc: Loc,
        result: &mut MutationResult,
        in_deleted: bool,
    ) -> Result<Option<(NodeId, NodeId)>> {
        let s = self;
        let (inline, segment, byte) = match loc {
            Loc::Boundary { .. } => return Ok(None),
            Loc::InRun { inline, segment } => (inline, segment, 0),
            Loc::InText { inline, segment, byte } => (inline, segment, byte),
        };
        let para = at.para;
        let tb = EditSession::require_text_block(s, at.part, para)?;
        let Inline::Run(run) = &tb.inlines[inline] else {
            unreachable!("InRun/InText point at runs")
        };
        if !in_deleted
            && (Tracker::in_deleted_run(run) || run.segments[segment].kind == SegmentKind::DelText)
        {
            return Err(unsupported("位置在已删除文本内（不追踪时不能在删除区里编辑）"));
        }
        let run_node = run.node;
        let plan = EditSession::split_run(s, at.part, para, run, segment, byte);
        let r = s.commit_plan(plan)?;
        let right = r.created[0].expect("split creates the right run");
        result.absorb(r);
        Ok(Some((run_node, right)))
    }
    #[inline]
    /// 边界一侧的 inline 对应的 DOM 节点。
    ///
    /// 字段原子（`Inline::Field`）自己没有单一节点，取它靠着边界的那一端：左邻取 `tail`
    /// （end run / `w:fldSimple`）、右邻取 `head`（begin run）。插入点因此落在原子**之外**，
    /// 与 `SPAN-10`"端点落在字段原子内部时移到原子边界"是同一条道理。
    fn boundary_node(
        &self,
        part: Option<PartId>,
        tb: &TextBlock,
        i: &Inline,
        side: Side,
    ) -> Result<NodeId> {
        let s = self;
        if let Some(n) = i.node() {
            return Ok(n);
        }
        let Inline::Field { id, .. } = i else {
            return Err(unsupported("inline 没有对应节点"));
        };
        // 字段索引是**按 part** 的（`FLD-02`）：页眉里的字段在那个 part 自己的索引里
        let f = s
            .document()
            .fields_in(s.part_or_main(part))
            .and_then(|idx| idx.get(*id))
            .ok_or_else(|| unsupported("字段不在索引里（投影过期）"))?;
        let n = match side {
            Side::Left => f.form.tail(),
            Side::Right => f.form.head(),
        };
        // 跨段字段（`FLD-06` 的 `Block`）另一端在别的段落里，结果段落只读
        if !s.dom_in(part)?.ancestors(n).any(|a| a == tb.node) {
            return Err(unsupported("跨段字段的边界（Block 字段的结果段落只读）"));
        }
        Ok(n)
    }
    #[inline]
    /// 边界插入点：`(parent, before, 继承格式的 run)`。
    fn boundary_site(
        &self,
        part: Option<PartId>,
        tb: &TextBlock,
        index: usize,
    ) -> Result<(NodeId, Option<NodeId>, Option<NodeId>)> {
        let s = self;
        let dom = s.dom_in(part)?;
        let para = tb.node;
        let left = (index > 0)
            .then(|| EditSession::boundary_node(s, part, tb, &tb.inlines[index - 1], Side::Left))
            .transpose()?;
        let right = (index < tb.inlines.len())
            .then(|| EditSession::boundary_node(s, part, tb, &tb.inlines[index], Side::Right))
            .transpose()?;
        // 继承格式的 run：先看平铺的 run，再看字段结果里的 run（紧邻字段插字沿用结果的格式）
        let run_node = |i: &Inline| match i {
            Inline::Run(r) => Some(r.node),
            Inline::Field { result, .. } => result.iter().rev().find_map(|r| match r {
                Inline::Run(r) => Some(r.node),
                _ => None,
            }),
            Inline::Atom(_) => None,
        };
        let inherit = tb.inlines[..index]
            .iter()
            .rev()
            .find_map(run_node)
            .or_else(|| tb.inlines[index..].iter().find_map(run_node));
        let (parent, before) = match (left, right) {
            (Some(l), Some(r)) => InlinePos::common_site(dom, para, l, r),
            (Some(_), None) | (None, None) => (para, None),
            (None, Some(r)) => (para, Some(InlinePos::top_child(dom, para, r))),
        };
        Ok((parent, before, inherit))
    }
    #[inline]
    fn insert_text(
        &mut self,
        at: InlinePos,
        text: &str,
        props: Option<RunPropsPatch>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.part_or_main(at.part);
        s.spans_of(part)?;
        let mut diags = Vec::new();
        let text = Emitter::sanitize_text(text, part, &mut diags);
        if text.is_empty() {
            return Err(Error::edit(DiagCode::EditBadText, "插入文本为空"));
        }
        let delta = utf16_len(&text) as i32;
        let tb = EditSession::require_text_block(s, at.part, at.para)?;
        let loc = at.offset.locate(tb)?;

        // 追踪时先看位置落在什么修订包裹里（`spec/18` 7.2 的同作者规则）
        let mut tracker = Tracker::new(s.document(), ctx);
        let dom0 = s.dom_in(at.part)?;
        if let Some(t) = &tracker {
            let probe = match loc {
                Loc::Boundary { .. } => at.para,
                Loc::InRun { inline, .. } | Loc::InText { inline, .. } => {
                    tb.inlines[inline].node().unwrap_or(at.para)
                }
            };
            if matches!(track_site_of(dom0, probe, at.para, &t.author), TrackSite::Deleted(_)) {
                return Err(err_in_deleted());
            }
        }
        // 路径 1：紧邻 / 落在 Text 段 → 直接写该 w:t 的文本节点。
        // 追踪时只有落在**本作者自己的** `w:ins` 里才能这么做（Word：自己插的可以接着改）
        // 范围端点处必须插独立内容项，让 SPAN-06 按 affinity 移动锚点；
        // 直接扩写原 run 会吞掉这个边界，使追踪与不追踪的批注覆盖范围不同。
        let own_ins = |seg: NodeId| {
            tracker.as_ref().is_none_or(|t| {
                matches!(track_site_of(dom0, seg, at.para, &t.author), TrackSite::OwnIns(_))
            })
        };
        if props.is_none()
            && !Emitter::has_control_chars(&text)
            && let Some((seg_node, seg_text, byte)) = InlinePos::direct_text_target(tb, &loc)
            && own_ins(seg_node)
            && !s.spans_built(part).is_some_and(|index| {
                index.live().any(|span| {
                    [span.start, span.end].into_iter().flatten().any(|a| {
                        crate::span::content::item_containing(dom0, a.container, seg_node)
                            .is_some_and(|i| {
                                (byte == 0 && a.index == i)
                                    || (byte == seg_text.len() && a.index == i + 1)
                            })
                    })
                })
            })
        {
            let new_text = format!("{}{}{}", &seg_text[..byte], text, &seg_text[byte..]);
            let mut plan = MutationPlan::new(part);
            plan.touch(at.para);
            plan.diagnostics = diags;
            MutationPlan::set_segment_text(s.dom_in(at.part)?, seg_node, &new_text, &mut plan);
            plan.offset_delta.push((at.para, at.offset, delta));
            return s.commit_plan(plan);
        }

        // 路径 2：边界插入 New run（继承左侧 rPr 或 default_run_props）
        let mut result = MutationResult::default();
        let (parent, before, inherit) = match EditSession::split_at(s, at, loc, &mut result)? {
            Some((left, right)) => (
                s.dom_in(at.part)?.parent(left).expect("run has a parent"),
                Some(right),
                Some(left),
            ),
            None => {
                let Loc::Boundary { index } = loc else {
                    unreachable!("split_at handles the rest")
                };
                EditSession::boundary_site(
                    s,
                    at.part,
                    EditSession::require_text_block(s, at.part, at.para)?,
                    index,
                )?
            }
        };
        let dom = s.dom_in(at.part)?;
        let flavor = s.flavor_in(at.part);
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        plan.diagnostics = diags;
        // 追踪：新 run 进 `w:ins`（同作者规则见 `plan_ins_site`）
        let (run_parent, run_before) = match &mut tracker {
            None => (Target::Node(parent), before),
            Some(t) => MutationPlan::plan_ins_site(&mut plan, dom, t, at.para, parent, before)?,
        };
        let k = plan.node_edits.len();
        plan.node_edits.push(NodeEdit::Insert {
            parent: run_parent,
            before: run_before,
            node: NewElement::new(QName::w(LocalName::R)),
        });
        match inherit.and_then(|r| MutationPlan::rpr_of(dom, r)) {
            Some(rpr) => plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(k),
                before: None,
                source: rpr,
            }),
            None => {
                if let Some(d) = &ctx.default_run_props {
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::New(k),
                        before: None,
                        node: emit_run_props(d, flavor),
                    });
                }
            }
        }
        for seg in Emitter::text_segments(&text, false) {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: seg,
            });
        }
        plan.offset_delta.push((at.para, at.offset, delta));
        let r = s.commit_plan(plan)?;
        let new_run = r.created[k].expect("insert creates the run");
        result.absorb(r);

        // 路径 2c：合并 props
        if let Some(patch) = props {
            let dom = s.dom_in(at.part)?;
            let edits = plan_apply_run_props(
                dom,
                new_run,
                MutationPlan::rpr_of(dom, new_run),
                &patch,
                flavor,
            );
            if !edits.is_empty() {
                let mut plan = MutationPlan::new(part);
                plan.touch(at.para);
                plan.node_edits = edits;
                result.absorb(s.commit_plan(plan)?);
            }
        }
        Ok(result)
    }
    #[inline]
    /// `boundary_site` 的对外壳（`atom_ops` 用）。
    fn boundary_site_public(
        &self,
        part: Option<PartId>,
        para: NodeId,
        index: usize,
    ) -> Result<(NodeId, Option<NodeId>, Option<NodeId>)> {
        let s = self;
        EditSession::boundary_site(s, part, EditSession::require_text_block(s, part, para)?, index)
    }
}

impl InlinePos {
    #[inline]
    /// `props == None` 时可直接写入的 `Text` 段：`(段节点, 段文本, 字节偏移)`。
    fn direct_text_target<'a>(tb: &'a TextBlock, loc: &Loc) -> Option<(NodeId, &'a str, usize)> {
        let text_seg = |run: &'a Run, seg: &'a Segment| -> Option<(NodeId, &'a str)> {
            (seg.kind == SegmentKind::Text && !Tracker::in_deleted_run(run))
                .then(|| (seg.node, run.segment_text(seg)))
        };
        let run_at = |i: usize| match &tb.inlines[i] {
            Inline::Run(r) => Some(r),
            _ => None,
        };
        match *loc {
            Loc::InText { inline, segment, byte } => {
                let run = run_at(inline)?;
                text_seg(run, &run.segments[segment]).map(|(n, t)| (n, t, byte))
            }
            Loc::InRun { inline, segment } => {
                let run = run_at(inline)?;
                if let Some((n, t)) = text_seg(run, &run.segments[segment - 1]) {
                    return Some((n, t, t.len()));
                }
                text_seg(run, &run.segments[segment]).map(|(n, t)| (n, t, 0))
            }
            Loc::Boundary { index } => {
                if index > 0
                    && let Some(run) = run_at(index - 1)
                    && let Some(seg) = run.segments.last()
                    && let Some((n, t)) = text_seg(run, seg)
                {
                    return Some((n, t, t.len()));
                }
                if index < tb.inlines.len()
                    && let Some(run) = run_at(index)
                    && let Some(seg) = run.segments.first()
                    && let Some((n, t)) = text_seg(run, seg)
                {
                    return Some((n, t, 0));
                }
                None
            }
        }
    }
    #[inline]
    /// `n` 的祖先或自身中直接挂在 `para` 下的那个。
    fn top_child(dom: &Dom, para: NodeId, n: NodeId) -> NodeId {
        let mut x = n;
        while dom.parent(x).is_some_and(|p| p != para) {
            x = dom.parent(x).expect("checked");
        }
        x
    }
    #[inline]
    /// 左右两个 inline 节点的最深公共容器与插入锚点（`before` = 含右节点的那个子节点）。
    fn common_site(
        dom: &Dom,
        para: NodeId,
        left: NodeId,
        right: NodeId,
    ) -> (NodeId, Option<NodeId>) {
        let anc_left: Vec<NodeId> =
            std::iter::successors(Some(left), |&x| dom.parent(x).filter(|&p| p != para)).collect();
        let mut x = right;
        loop {
            let p = dom.parent(x).expect("inline is inside the paragraph");
            if p == para || anc_left.contains(&p) {
                return (p, Some(x));
            }
            x = p;
        }
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    fn delete_range(
        &mut self,
        from: InlinePos,
        to: InlinePos,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        if from.part != to.part {
            return Err(Error::edit(DiagCode::EditBadPosition, "DeleteRange 两端不在同一个 part"));
        }
        if from.para != to.para {
            return EditSession::delete_range_cross(s, from, to, ctx);
        }
        let (a, b) = (from.offset.0, to.offset.0);
        if a > b {
            return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
        }
        let part = s.part_or_main(from.part);
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        from.offset.locate(tb)?;
        to.offset.locate(tb)?;
        let mut plan = MutationPlan::new(part);
        plan.span.keep_orphan_comments = ctx.keep_orphan_comments;
        plan.touch(from.para);
        if a == b {
            return s.commit_plan(plan);
        }
        if ctx.track_changes.is_some() {
            return EditSession::delete_range_tracked(s, from, to, ctx);
        }
        let dom = s.dom_in(from.part)?;
        let fields = &s.document().fields;
        let spans = InlinePos::inline_spans(tb);
        let mut kept_structure = 0usize;
        for (inline, span) in tb.inlines.iter().zip(spans) {
            if span.end <= a || span.start >= b {
                continue;
            }
            match inline {
                Inline::Atom(atom) => plan.node_edits.push(NodeEdit::Delete(atom.node)),
                // `FLD-07`：原子形态字段被覆盖 → 整个字段（begin..end，含嵌套）一起删。
                // 原子只占 1 个坐标单位，区间与它相交就必然把它整个盖住。
                Inline::Field { id, .. } => {
                    for n in fields.all_nodes(*id) {
                        plan.node_edits.push(NodeEdit::Delete(n));
                    }
                }
                Inline::Run(run) => {
                    let fully = span.start >= a && span.end <= b;
                    let structural = run.segments.iter().any(|sg| Tracker::is_structural(&sg.kind));
                    if fully && !structural {
                        plan.node_edits.push(NodeEdit::Delete(run.node));
                        continue;
                    }
                    // 属于已识别字段的结构 run 原地保留是**正确**行为：透明字段（`Link`）的结果可以
                    // 正常编辑，字段本身不该跟着消失。剩下的（畸形 / 未闭合字段的 fldChar）才是缺陷。
                    if structural
                        && run.field.is_none()
                        && run.segments.iter().any(|sg| {
                            Tracker::is_field_structure(&sg.kind)
                                && fields.field_of(run.node).is_none()
                        })
                    {
                        kept_structure += 1;
                    }
                    let mut ss = span.start;
                    for seg in &run.segments {
                        let se = ss + seg.utf16_len;
                        let inside =
                            if seg.utf16_len == 0 { ss > a && ss < b } else { se > a && ss < b };
                        if inside {
                            match seg.kind {
                                SegmentKind::Text | SegmentKind::DelText => {
                                    let c0 = a.max(ss) - ss;
                                    let c1 = b.min(se) - ss;
                                    if c0 == 0 && c1 == seg.utf16_len {
                                        plan.node_edits.push(NodeEdit::Delete(seg.node));
                                    } else {
                                        let t = run.segment_text(seg);
                                        let b0 = usize::try_from(Utf16TextOffset {
                                            text: t,
                                            offset: Utf16Offset(c0),
                                        })?;
                                        let b1 = usize::try_from(Utf16TextOffset {
                                            text: t,
                                            offset: Utf16Offset(c1),
                                        })?;
                                        let nt = format!("{}{}", &t[..b0], &t[b1..]);
                                        MutationPlan::set_segment_text(
                                            dom, seg.node, &nt, &mut plan,
                                        );
                                    }
                                }
                                _ if seg.utf16_len > 0 => {
                                    plan.node_edits.push(NodeEdit::Delete(seg.node))
                                }
                                ref k if Tracker::is_structural(k) => {}
                                _ => plan.node_edits.push(NodeEdit::Delete(seg.node)),
                            }
                        }
                        ss = se;
                    }
                }
            }
        }
        // 范围标记不再原地"漏"着：删除内容项后标记物理上就落在删除点，正好是 `SPAN-06`
        // 把锚点算到的位置（`commit_plan` 统一变换）。剩下的只有字段结构，等 2.4 的 `FieldSpan`。
        if kept_structure > 0 {
            plan.diagnostics.push(Diagnostic::invariant_violation(
                part,
                None,
                DiagCode::EditAnchorUnmoved,
                format!("删除范围内有 {kept_structure} 个未闭合 / 畸形字段的结构 run 原地保留"),
            ));
        }
        plan.offset_delta.push((from.para, from.offset, -((b - a) as i32)));
        // 覆盖到的注释引用：条目跟着走（`EDIT-03`；追踪时不删——那是接受修订那一刻的事，7.4）
        let notes = MutationPlan::covered_note_refs(dom, tb, a, b);
        let mut result = s.commit_plan(plan)?;
        for (endnote, id) in notes {
            result.absorb(EditSession::drop_references(s, endnote, &id)?);
            result.absorb(EditSession::remove_note_entry(s, endnote, &id)?);
        }
        Ok(result)
    }
    #[inline]
    /// 跨段 `DeleteRange`（`spec/18` 7.5）：两端在**同一个内容容器**里时，拆成
    /// 首段尾部删除 + 中间块 `DeleteBlock` + 末段头部删除 + `MergeWithNext` 四步，
    /// 都在调用方那一个事务里（任一步失败整体回滚）。跨容器 → `Err(EDIT_CROSS_CONTAINER)`。
    ///
    /// 追踪时这四步各自按 7.2 / 7.3 的规则留痕：三段都还在，中段与两头的内容带 `w:del`，
    /// 首段的段落标记带 `w:del`（接受之后才真的并成一段）。
    fn delete_range_cross(
        &mut self,
        from: InlinePos,
        to: InlinePos,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let dom = s.dom_in(from.part)?;
        let container = |p: NodeId| crate::span::container_of(dom, p);
        let (Some(c1), Some(c2)) = (container(from.para), container(to.para)) else {
            return Err(Error::edit(DiagCode::EditBadPosition, "段落不在内容容器里"));
        };
        if c1 != c2 {
            return Err(Error::edit(
                DiagCode::EditCrossContainer,
                "DeleteRange 两端不在同一个内容容器里",
            ));
        }
        // 容器里从首段到末段之间的块（含末段，不含首段）
        let items: Vec<NodeId> = crate::span::content_children(dom, c1);
        let (Some(i1), Some(i2)) = (
            items.iter().position(|&n| n == InlinePos::top_child(dom, c1, from.para)),
            items.iter().position(|&n| n == InlinePos::top_child(dom, c1, to.para)),
        ) else {
            return Err(Error::edit(DiagCode::EditBadPosition, "段落不是容器的直接内容项"));
        };
        if i1 >= i2 {
            return Err(Error::edit(DiagCode::EditBadPosition, "from 段落在 to 段落之后"));
        }
        let middles: Vec<NodeId> = items[i1 + 1..i2].to_vec();
        let first_len =
            EditSession::require_text_block(s, from.part, from.para)?.text().encode_utf16().count()
                as u32;
        let mut result = MutationResult::default();
        // ① 首段：从 `from` 删到段尾
        if from.offset.0 < first_len {
            let end =
                InlinePos { part: from.part, para: from.para, offset: Utf16Offset(first_len) };
            result.absorb(EditSession::delete_range(s, from, end, ctx)?);
        }
        // ② 中间的块整块删（表格 / 段落都走 `DeleteBlock` 的规则）
        for m in middles {
            result.absorb(EditSession::delete_block(s, from.part, m, ctx)?);
        }
        // ③ 末段：从段首删到 `to`
        if to.offset.0 > 0 {
            let start = InlinePos { part: to.part, para: to.para, offset: Utf16Offset(0) };
            result.absorb(EditSession::delete_range(s, start, to, ctx)?);
        }
        // ④ 两段并一段（追踪时只在首段的标记上打 `w:del`）
        result.absorb(EditSession::merge_with_next(s, from.part, from.para, ctx)?);
        Ok(result)
    }
    #[inline]
    /// 追踪时的 `DeleteRange`（`spec/18` 7.2）：**内容不删**，覆盖到的每个内容项原地包进
    /// `w:del`，`w:t → w:delText`、`w:instrText → w:delInstrText`。
    ///
    /// 三条与不追踪相反的性质：坐标流长度不变（`w:delText` 照样占位）、`offset_delta` 为 0、
    /// 范围标记一个都不动（`SPAN-06` 的删除规则**不**调用，见 `SpanPolicy::rewraps`）。
    ///
    /// 同作者规则：本作者自己插的（`w:ins` 在本作者名下）真删；别人插的 → `w:del` 嵌在那个
    /// `w:ins` 里（包裹插在 run 原来的位置，父节点就是 `w:ins`，天然嵌进去）；已经在 `w:del`
    /// 里的不动。
    fn delete_range_tracked(
        &mut self,
        from: InlinePos,
        to: InlinePos,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.part_or_main(from.part);
        let (a, b) = (from.offset.0, to.offset.0);
        let mut result = MutationResult::default();
        // 两端先拆 run（拆分不改坐标），之后区间里的 run 要么整个在内要么整个在外
        for off in [to.offset, from.offset] {
            let loc = off.locate(EditSession::require_text_block(s, from.part, from.para)?)?;
            EditSession::split_at_maybe_deleted(s, from, loc, &mut result, true)?;
        }
        let mut t = Tracker::new(s.document(), ctx).expect("调用方已确认在追踪");
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let spans = InlinePos::inline_spans(tb);
        let dom = s.dom_in(from.part)?;
        let fields =
            s.document().fields_in(part).ok_or_else(|| unsupported("这个 part 没有字段索引"))?;
        let mut plan = MutationPlan::new(part);
        plan.span.keep_orphan_comments = ctx.keep_orphan_comments;
        plan.touch(from.para);
        let mut kept_structure = 0usize;
        // `(节点, 真删还是标删)`，文档序
        let mut items: Vec<(NodeId, bool)> = Vec::new();
        for (inline, span) in tb.inlines.iter().zip(spans) {
            if span.end <= a || span.start >= b {
                continue;
            }
            match inline {
                Inline::Atom(atom) => items.push((atom.node, false)),
                // `FLD-07`：原子形态字段被覆盖 → begin..end 整段进 `w:del`（条目 / 结果都留着）
                Inline::Field { id, .. } => {
                    for n in fields.all_nodes(*id) {
                        items.push((n, false));
                    }
                }
                Inline::Run(run) => {
                    // 已经在删除区里：不动（不套第二层）
                    if matches!(
                        track_site_of(dom, run.node, from.para, &t.author),
                        TrackSite::Deleted(_)
                    ) {
                        continue;
                    }
                    // 结构 run（`fldChar` / `instrText` / 批注引用 / 注释分隔符）一律**原地保留**，
                    // 与不追踪那条路同一条规则（`EDIT-03`）。追踪时更要紧：本作者自己插的内容会被
                    // 真删，真删掉半个字段另一半就成了孤儿，`FLD-13` 从此每次保存都失败
                    // （`TEST-07` 在「追踪插题注 → 追踪删它一段」上抓到的）
                    if run.segments.iter().any(|sg| Tracker::is_structural(&sg.kind)) {
                        if run.field.is_none()
                            && run.segments.iter().any(|sg| {
                                Tracker::is_field_structure(&sg.kind)
                                    && fields.field_of(run.node).is_none()
                            })
                        {
                            kept_structure += 1;
                        }
                        continue;
                    }
                    // 本作者自己插的 → 真删（Word：自己插的字删掉就没了）
                    let own = matches!(
                        track_site_of(dom, run.node, from.para, &t.author),
                        TrackSite::OwnIns(_)
                    );
                    items.push((run.node, own));
                }
            }
        }
        // 真删掉自己插的内容之后，空掉的 `w:ins` 壳一起删（Word 不留空包裹）
        let dropped: Vec<NodeId> = items.iter().filter(|(_, d)| *d).map(|&(n, _)| n).collect();
        let mut empty_wrappers: Vec<NodeId> = Vec::new();
        for &n in &dropped {
            if let TrackSite::OwnIns(ins) = track_site_of(dom, n, from.para, &t.author)
                && !empty_wrappers.contains(&ins)
                && Dom::live_children(dom, ins).all(|c| dropped.contains(&c))
            {
                empty_wrappers.push(ins);
            }
        }
        for (node, drop_it) in items {
            if drop_it {
                // 壳整个删掉就不用再删它的孩子
                let covered = empty_wrappers
                    .iter()
                    .any(|&ins| dom.is_ancestor_or_self(ins, node) && ins != node);
                if !covered {
                    plan.node_edits.push(NodeEdit::Delete(node));
                }
                continue;
            }
            t.wrap_item(&mut plan, dom, node, LocalName::Del);
            Tracker::rename_to_deleted(&mut plan, dom, node);
        }
        for ins in empty_wrappers {
            plan.node_edits.push(NodeEdit::Delete(ins));
        }
        if kept_structure > 0 {
            plan.diagnostics.push(Diagnostic::invariant_violation(
                part,
                None,
                DiagCode::EditAnchorUnmoved,
                format!("删除范围内有 {kept_structure} 个未闭合 / 畸形字段的结构 run 原地保留"),
            ));
        }
        // `offset_delta` 不写：追踪时内容还在坐标流里
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    fn set_run_props(
        &mut self,
        from: InlinePos,
        to: InlinePos,
        patch: &RunPropsPatch,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        if from.part != to.part {
            return Err(Error::edit(DiagCode::EditBadPosition, "SetRunProps 两端不在同一个 part"));
        }
        if from.para != to.para {
            return Err(Error::edit(DiagCode::EditCrossParagraph, "SetRunProps 两端不在同一段落"));
        }
        let (a, b) = (from.offset.0, to.offset.0);
        if a > b {
            return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
        }
        let part = s.part_or_main(from.part);
        let mut result = MutationResult::default();
        // 阶段 A/B：先在 to、再在 from 处拆分（拆分不改变坐标）
        for off in [to.offset, from.offset] {
            let loc = off.locate(EditSession::require_text_block(s, from.part, from.para)?)?;
            EditSession::split_at(s, from, loc, &mut result)?;
        }
        // 阶段 C0（追踪）：范围内每个还没有 `rPrChange` 的 run 记下旧格式（`spec/08`）。
        // 单独一个阶段：补丁要看到已经存在的 `w:rPrChange`，才能把新元素放在它**前面**（`PROP-05`）
        if let Some(plan) = EditSession::plan_run_props_change(s, from, a, b, ctx)? {
            result.absorb(s.commit_plan(plan)?);
        }
        // 阶段 C：范围内的每个非零宽 run 按 PROP-06 计划 rPr 变更
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let spans = InlinePos::inline_spans(tb);
        let dom = s.dom_in(from.part)?;
        let flavor = s.flavor_in(from.part);
        let mut plan = MutationPlan::new(part);
        plan.touch(from.para);
        for (inline, span) in tb.inlines.iter().zip(spans) {
            if span.start < a || span.end > b || span.start == span.end {
                continue;
            }
            if let Inline::Run(run) = inline {
                let edits = plan_apply_run_props(
                    dom,
                    run.node,
                    MutationPlan::rpr_of(dom, run.node),
                    patch,
                    flavor,
                );
                MutationPlan::append_edits(&mut plan, edits);
            }
        }
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    /// 追踪时 `[a, b)` 里每个 run 的 `w:rPrChange` 旧值快照。不追踪 → `None`。
    fn plan_run_props_change(
        &self,
        from: InlinePos,
        a: u32,
        b: u32,
        ctx: &EditContext,
    ) -> Result<Option<MutationPlan>> {
        let s = self;
        let Some(mut t) = Tracker::new(s.document(), ctx) else { return Ok(None) };
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let spans = InlinePos::inline_spans(tb);
        let dom = s.dom_in(from.part)?;
        let mut plan = MutationPlan::new(s.part_or_main(from.part));
        plan.touch(from.para);
        for (inline, span) in tb.inlines.iter().zip(spans) {
            if span.start < a || span.end > b || span.start == span.end {
                continue;
            }
            let Inline::Run(run) = inline else { continue };
            MutationPlan::snapshot_run_props(&mut plan, dom, &mut t, run.node);
        }
        Ok((!plan.is_empty()).then_some(plan))
    }
    #[inline]
    fn set_para_props(
        &mut self,
        part: Option<PartId>,
        para: NodeId,
        patch: &ParaPropsPatch,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        MutationPlan::require_paragraph(s.dom_in(part)?, para)?;
        let mut result = MutationResult::default();
        // 追踪：先把旧值快照成 `w:pPrChange`（`SAVE-04`），再打补丁——两个阶段，补丁看到的
        // `pPr` 里已经有 `pPrChange`，`order` 会把新元素放在它前面（`PROP-05`）
        if let Some(plan) = EditSession::plan_para_props_change(s, part, para, ctx)? {
            result.absorb(s.commit_plan(plan)?);
        }
        let dom = s.dom_in(part)?;
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        plan.node_edits = plan_apply_para_props(
            dom,
            para,
            MutationPlan::ppr_of(dom, para),
            patch,
            s.flavor_in(part),
        );
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    /// 追踪时段落属性变更的旧值快照（`w:pPrChange`）。不追踪 → `None`。
    fn plan_para_props_change(
        &self,
        part: Option<PartId>,
        para: NodeId,
        ctx: &EditContext,
    ) -> Result<Option<MutationPlan>> {
        let s = self;
        let Some(mut t) = Tracker::new(s.document(), ctx) else { return Ok(None) };
        let dom = s.dom_in(part)?;
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        match MutationPlan::ppr_of(dom, para) {
            Some(ppr) => {
                t.snapshot(
                    &mut plan,
                    dom,
                    ppr,
                    LocalName::PPrChange,
                    LocalName::PPr,
                    // `in_change = false`：段落标记的 `rPr` 与段落级 `sectPr` 不进快照（`para.toml`）
                    &[LocalName::RPr, LocalName::SectPr],
                );
            }
            None => {
                // 没有 `pPr`：旧值全是默认，快照是一个空的 `w:pPr`
                let before = Dom::live_children(dom, para).next();
                let k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(para),
                    before,
                    node: NewElement::new(QName::w(LocalName::PPr)),
                });
                let change = t
                    .marker(LocalName::PPrChange)
                    .with_child(NewElement::new(QName::w(LocalName::PPr)));
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: change,
                });
            }
        }
        Ok((!plan.is_empty()).then_some(plan))
    }
    #[inline]
    fn replace_inlines(
        &mut self,
        part: Option<PartId>,
        para: NodeId,
        inlines: &[NewInline],
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let dom = s.dom_in(part)?;
        MutationPlan::require_paragraph(dom, para)?;
        if ctx.track_changes.is_some() {
            return EditSession::replace_inlines_tracked(s, part, para, inlines, ctx);
        }
        EditSession::replace_container_inlines_in(s, part, para, para, inlines)
    }
    #[inline]
    /// 主 part 里某个内联容器（段落自己、或段落里的 `w:sdtContent`）的内容整体重写。
    /// `SetSdtContent` 与 `ReplaceInlines` 共用；追踪时走 `ReplaceInlines` 的 diff。
    fn replace_container_inlines(
        &mut self,
        para: NodeId,
        container: NodeId,
        inlines: &[NewInline],
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        if ctx.track_changes.is_some() && container == para {
            return EditSession::replace_inlines_tracked(s, None, para, inlines, ctx);
        }
        EditSession::replace_container_inlines_in(s, None, para, container, inlines)
    }
    #[inline]
    fn replace_container_inlines_in(
        &mut self,
        part: Option<PartId>,
        para: NodeId,
        container: NodeId,
        inlines: &[NewInline],
    ) -> Result<MutationResult> {
        let s = self;
        let dom = s.dom_in(part)?;
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        // 内容（含范围标记）被外部描述整体重写：提交后按新标记重建这个容器的端点（`SPAN-06` rescan）
        plan.span.rescan.push(container);
        for c in Dom::live_children(dom, container) {
            if !dom.is(c, QName::w(LocalName::PPr)) {
                plan.node_edits.push(NodeEdit::Delete(c));
            }
        }
        for e in MutationPlan::emit_inlines(dom, inlines) {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(container),
                before: None,
                node: e,
            });
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `EDIT-03 SetMathTokens`（TS `patchMathTokens`）：按序替换 `m:oMath` 里每个 `m:t` 的文字。
    fn set_math_tokens(&mut self, math: NodeId, tokens: &[String]) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let dom = s.dom();
        if (math.0 as usize) >= dom.node_count()
            || dom.node(math).dirty == Dirty::Deleted
            || !dom.is(math, QName::new(NsId::M, LocalName::OMath))
        {
            return Err(Error::edit(DiagCode::EditBadPosition, "目标不是活的 m:oMath"));
        }
        let slots: Vec<NodeId> = dom
            .descendants(math)
            .filter(|&n| dom.node(n).dirty != Dirty::Deleted)
            .filter(|&n| dom.is(n, QName::new(NsId::M, LocalName::T)))
            .collect();
        if slots.len() != tokens.len() {
            return Err(Error::edit(
                DiagCode::EditMathTokenCount,
                format!("公式有 {} 个 m:t，给了 {} 个 token", slots.len(), tokens.len()),
            ));
        }
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(math).find(|&a| dom.is(a, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        for (node, text) in slots.into_iter().zip(tokens) {
            MutationPlan::set_segment_text(dom, node, text, &mut plan);
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// 追踪时的 `ReplaceInlines`（`spec/18` 7.2）：坐标流 diff **聚到 run 边界**——
    /// 相等的 run 保留原节点（原字节），删掉的进 `w:del`，新增的进 `w:ins`。
    ///
    /// 相等的判据是「文本与 `w:rPr` 全同」，所以相等段保留原节点之后，接受视图与不追踪做一遍
    /// 完全一致。两侧的 `w:rPr` 都化成 `NewElement` 再比，比较保守（属性顺序不同判成不等），
    /// 保守只让 diff 变粗、不会误判相等。
    ///
    /// **范围标记不动**（Word：在书签里替换文字，书签还在），而不追踪那条路是按调用方的描述
    /// 整体重发标记——这条差异登记在 `docs/04` §8。
    fn replace_inlines_tracked(
        &mut self,
        part: Option<PartId>,
        para: NodeId,
        inlines: &[NewInline],
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let mut t = Tracker::new(s.document(), ctx).expect("调用方已确认在追踪");
        let tb = EditSession::require_text_block(s, part, para)?;
        let dom = s.dom_in(part)?;
        let mut interner = crate::xml::Interner::new();
        // 旧侧：每个 inline 一个 token；只有"纯文本 run 且是段落的直接子节点"才可能相等
        let mut old_toks: Vec<Tok> = Vec::new();
        let mut old_nodes: Vec<NodeId> = Vec::new();
        for inline in &tb.inlines {
            let node = match inline.node() {
                Some(n) => n,
                // 字段原子没有单一节点：整体当一个不可匹配的 token（下面会退化成整体替换）
                None => {
                    old_toks.push(Tok::Atom(format!("{:?}", std::ptr::from_ref(inline))));
                    old_nodes.push(para);
                    continue;
                }
            };
            let plain = match inline {
                Inline::Run(r) => {
                    r.segments.iter().all(|sg| sg.kind == SegmentKind::Text)
                        && dom.parent(node) == Some(para)
                }
                _ => false,
            };
            if plain {
                let props = MutationPlan::rpr_of(dom, node)
                    .and_then(|n| NewElement::from_dom(dom, n, &mut interner))
                    .map(Box::new);
                let text = match inline {
                    Inline::Run(r) => r.text.as_str(),
                    _ => unreachable!("plain 只对 Run 成立"),
                };
                old_toks.push(Tok::Run(text, props));
            } else {
                old_toks.push(Tok::Atom(crate::xml::canonical(
                    dom,
                    node,
                    &crate::xml::CanonOptions::default(),
                )));
            }
            old_nodes.push(node);
        }
        // 新侧：每个 `NewInline` 一个 token，同时记下它展开成的元素
        let mut new_toks: Vec<Tok> = Vec::new();
        let mut new_nodes: Vec<Vec<NewElement>> = Vec::new();
        let mut has_marker = false;
        for i in inlines {
            let mut em = Emitter::new(MutationPlan::next_revision_id(dom));
            let mut out = Vec::new();
            em.emit(i, false, &mut out);
            match i {
                NewInline::Run(r) => {
                    new_toks.push(Tok::Run(&r.text, r.props.clone().map(Box::new)))
                }
                other => {
                    has_marker |= matches!(other, NewInline::Marker(_));
                    new_toks.push(Tok::Atom(format!("{other:?}")));
                }
            }
            new_nodes.push(out);
        }
        // 标记要按新描述重发时没法只做局部 diff：退化成"旧内容整体标删 + 新内容整体标插"
        let script =
            (!has_marker).then(|| InlineDiff { old: &old_toks, new: &new_toks }.steps()).flatten();
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        match script {
            Some(steps) => {
                let (mut oi, mut ni) = (0usize, 0usize);
                for step in steps {
                    match step {
                        Step::Equal(n) => {
                            oi += n;
                            ni += n;
                        }
                        Step::Delete(n) => {
                            for &node in &old_nodes[oi..oi + n] {
                                t.wrap_item(&mut plan, dom, node, LocalName::Del);
                                Tracker::rename_to_deleted(&mut plan, dom, node);
                            }
                            oi += n;
                        }
                        Step::Insert(n) => {
                            let before = old_nodes.get(oi).copied();
                            let k = plan.node_edits.len();
                            plan.node_edits.push(NodeEdit::Insert {
                                parent: Target::Node(para),
                                before,
                                node: t.marker(LocalName::Ins),
                            });
                            for e in new_nodes[ni..ni + n].iter().flatten() {
                                plan.node_edits.push(NodeEdit::Insert {
                                    parent: Target::New(k),
                                    before: None,
                                    node: e.clone(),
                                });
                            }
                            ni += n;
                        }
                    }
                }
            }
            None => {
                for &node in &old_nodes {
                    if dom.parent(node) == Some(para) {
                        t.wrap_item(&mut plan, dom, node, LocalName::Del);
                        Tracker::rename_to_deleted(&mut plan, dom, node);
                    }
                }
                let k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(para),
                    before: None,
                    node: t.marker(LocalName::Ins),
                });
                for e in new_nodes.iter().flatten() {
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::New(k),
                        before: None,
                        node: e.clone(),
                    });
                }
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    fn replace_para_props(
        &mut self,
        part: Option<PartId>,
        para: NodeId,
        props: Option<NewElement>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        MutationPlan::require_paragraph(s.dom_in(part)?, para)?;
        let mut result = MutationResult::default();
        // 追踪：快照先做进**旧**的 `pPr`，第二阶段再把那个 `w:pPrChange` 搬进新的 `pPr`——
        // 整份替换会把旧容器删掉，克隆源必须在它还活着的时候取
        if let Some(plan) = EditSession::plan_para_props_change(s, part, para, ctx)? {
            result.absorb(s.commit_plan(plan)?);
        }
        let tracked = ctx.track_changes.is_some();
        let dom = s.dom_in(part)?;
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.touch(para);
        let first = Dom::live_children(dom, para).next();
        let old_ppr = MutationPlan::ppr_of(dom, para);
        let change = old_ppr.filter(|_| tracked).and_then(|p| {
            Dom::live_children(dom, p).find(|&c| dom.is(c, QName::w(LocalName::PPrChange)))
        });
        let k = plan.node_edits.len();
        match (props, change) {
            (Some(p), _) => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(para),
                before: first,
                node: p,
            }),
            // 追踪时即使调用方要求"没有 pPr"，也得留一个装快照的空壳
            (None, Some(_)) => plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(para),
                before: first,
                node: NewElement::new(QName::w(LocalName::PPr)),
            }),
            (None, None) => {}
        }
        if let Some(c) = change {
            plan.node_edits.push(NodeEdit::Move { node: c, parent: Target::New(k), before: None });
        }
        for c in Dom::live_children(dom, para) {
            if dom.is(c, QName::w(LocalName::PPr)) {
                plan.node_edits.push(NodeEdit::Delete(c));
            }
        }
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
}

impl MutationPlan {
    #[inline]
    /// `[a, b)` 覆盖到的 `w:footnoteReference` / `w:endnoteReference` 的 `(是尾注, id)`。
    fn covered_note_refs(dom: &Dom, tb: &TextBlock, a: u32, b: u32) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for (inline, span) in tb.inlines.iter().zip(InlinePos::inline_spans(tb)) {
            if span.end <= a || span.start >= b {
                continue;
            }
            let Some(node) = inline.node() else { continue };
            for n in dom.descendants(node) {
                if dom.node(n).dirty == Dirty::Deleted {
                    continue;
                }
                let endnote = if dom.is(n, QName::w(LocalName::FootnoteReference)) {
                    false
                } else if dom.is(n, QName::w(LocalName::EndnoteReference)) {
                    true
                } else {
                    continue;
                };
                if let Some(id) = dom.attr_value(n, QName::w(LocalName::Id)) {
                    let entry = (endnote, id.into_owned());
                    if !out.contains(&entry) {
                        out.push(entry);
                    }
                }
            }
        }
        out
    }
    #[inline]
    /// 一个 run 的 `w:rPrChange` 旧值快照；没有 `w:rPr` 就先建一个空的（旧格式全是继承来的）。
    fn snapshot_run_props(&mut self, dom: &Dom, t: &mut Tracker, run: NodeId) {
        let plan = self;
        match MutationPlan::rpr_of(dom, run) {
            Some(rpr) => {
                t.snapshot(plan, dom, rpr, LocalName::RPrChange, LocalName::RPr, &[]);
            }
            None => {
                let k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(run),
                    before: Dom::live_children(dom, run).next(),
                    node: NewElement::new(QName::w(LocalName::RPr)),
                });
                let change = t
                    .marker(LocalName::RPrChange)
                    .with_child(NewElement::new(QName::w(LocalName::RPr)));
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: change,
                });
            }
        }
    }
    #[inline]
    fn require_paragraph(dom: &Dom, para: NodeId) -> Result<()> {
        if (para.0 as usize) < dom.node_count()
            && dom.node(para).dirty != Dirty::Deleted
            && dom.is(para, QName::w(LocalName::P))
        {
            Ok(())
        } else {
            Err(Error::edit(DiagCode::EditBadPosition, format!("节点 {} 不是活的 w:p", para.0)))
        }
    }
    #[inline]
    fn emit_inlines(dom: &Dom, inlines: &[NewInline]) -> Vec<NewElement> {
        let mut em = Emitter::new(MutationPlan::next_revision_id(dom));
        let mut out = Vec::new();
        for i in inlines {
            em.emit(i, false, &mut out);
        }
        out
    }
}

impl MutationPlan {
    #[inline]
    /// `BlockPos` 的落点 → `(parent, before)`。`End(c)` 落在尾部 `w:sectPr` 之前。
    fn block_site(dom: &Dom, at: BlockAt) -> Result<(NodeId, Option<NodeId>)> {
        let live_elem = |c: NodeId| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some();
        let ok = |n: NodeId| (n.0 as usize) < dom.node_count() && live_elem(n);
        match at {
            BlockAt::Start(c) => {
                if !ok(c) {
                    return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
                }
                Ok((c, dom.children(c).iter().copied().find(|&k| live_elem(k))))
            }
            BlockAt::Before(n) | BlockAt::After(n) => {
                if !ok(n) {
                    return Err(Error::edit(DiagCode::EditBadPosition, "锚点块无效"));
                }
                let parent = dom
                    .parent(n)
                    .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "锚点块没有父节点"))?;
                let before = if matches!(at, BlockAt::Before(_)) {
                    Some(n)
                } else {
                    Dom::next_live_sibling(dom, n)
                };
                Ok((parent, before))
            }
            BlockAt::End(c) => {
                if !ok(c) {
                    return Err(Error::edit(DiagCode::EditBadPosition, "容器无效"));
                }
                let last = dom.children(c).iter().copied().rev().find(|&k| live_elem(k));
                let before = last.filter(|&l| dom.is(l, QName::w(LocalName::SectPr)));
                Ok((c, before))
            }
        }
    }
    #[inline]
    fn new_block_element(dom: &Dom, block: NewBlock) -> NewElement {
        match block {
            NewBlock::Xml(e) => e,
            // 每个接收 `NewBlock` 的入口都先过 `chart_ops::materialize`（建 part、换成 `Xml`）
            NewBlock::Chart { .. }
            | NewBlock::Image(_)
            | NewBlock::MathPara { .. }
            | NewBlock::Textbox { .. }
            | NewBlock::Shape { .. }
            | NewBlock::Line { .. }
            | NewBlock::Field(_)
            | NewBlock::Caption { .. }
            | NewBlock::Many(_) => {
                unreachable!("这些块必须先经 chart_ops::materialize")
            }
            NewBlock::Table { rows, cols, widths, style, header } => {
                new_table(rows, cols, widths, style, header)
            }
            NewBlock::Paragraph { props, inlines } => {
                let mut p = NewElement::new(QName::w(LocalName::P));
                if let Some(pp) = props {
                    p.push_child(pp);
                }
                for e in MutationPlan::emit_inlines(dom, &inlines) {
                    p.push_child(e);
                }
                p
            }
            NewBlock::Wrapped { mut wrapper, block } => {
                // 块级 w:ins / w:del：空的 w:id 占位 → EDIT-06 分配
                if let Some(id) = wrapper
                    .attrs
                    .iter_mut()
                    .find(|(n, v)| *n == QName::w(LocalName::Id) && v.is_empty())
                {
                    id.1 = MutationPlan::next_revision_id(dom).to_string();
                }
                let inner = MutationPlan::new_block_element(dom, *block);
                wrapper.push_child(inner);
                wrapper
            }
        }
    }
    #[inline]
    /// `node` 所属的最内层 `w:tbl`（投影刷新的单位）。
    fn owning_table(dom: &Dom, node: NodeId) -> Option<NodeId> {
        std::iter::once(node)
            .chain(dom.ancestors(node))
            .find(|&n| dom.is(n, QName::w(LocalName::Tbl)))
    }
    #[inline]
    /// 属性容器与"缺失时插在谁之前"。`w:tblPr` / `w:tcPr` 是第一个子元素；`w:trPr` 在 `w:tblPrEx`
    /// 之后、第一个 `w:tc` 之前（`PROP-05` 的 `w:tr` 子元素顺序）。
    fn props_site(
        dom: &Dom,
        parent: NodeId,
        container: LocalName,
    ) -> (Option<NodeId>, Option<NodeId>) {
        let live = |c: NodeId| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some();
        let kids: Vec<NodeId> = dom.children(parent).iter().copied().filter(|&c| live(c)).collect();
        let existing = kids.iter().copied().find(|&c| dom.is(c, QName::w(container)));
        let before = if container == LocalName::TrPr {
            kids.iter().copied().find(|&c| !dom.is(c, QName::w(LocalName::TblPrEx)))
        } else {
            kids.first().copied()
        };
        (existing, before)
    }
    #[inline]
    /// 追踪时删一个块（`spec/18` 7.3）：段落 → 内容逐项 `w:del` + 段落标记 `w:del`（段落保留）；
    /// 表格 → 每行 `trPr/w:del`（行保留）；其他 → 整块包一层块级 `w:del`。
    fn plan_delete_block_tracked(&mut self, dom: &Dom, t: &mut Tracker, node: NodeId) {
        let plan = self;
        plan.touch(node);
        if dom.is(node, QName::w(LocalName::P)) {
            // 段落标记**先**打：`para_mark` 在没有 `pPr` 时要插在第一个内容子节点之前，
            // 而下面的包裹会把那个子节点搬进 `w:del`，`before` 就不再是段落的子节点了
            t.para_mark(plan, dom, node, LocalName::Del);
            for c in Dom::live_children(dom, node).collect::<Vec<_>>() {
                let Some(name) = dom.name(c) else { continue };
                if is_property_element(name) || crate::span::is_range_marker(name) {
                    continue;
                }
                t.wrap_item(plan, dom, c, LocalName::Del);
                Tracker::rename_to_deleted(plan, dom, c);
            }
        } else if dom.is(node, QName::w(LocalName::Tbl)) {
            plan.structure_changed = true;
            for row in Dom::live_children(dom, node).collect::<Vec<_>>() {
                if !dom.is(row, QName::w(LocalName::Tr)) {
                    continue;
                }
                let (_, before) = MutationPlan::props_site(dom, row, LocalName::TrPr);
                t.container_mark(
                    plan,
                    dom,
                    MarkSite { owner: row, container: LocalName::TrPr, container_before: before },
                    LocalName::Del,
                    crate::semantic::props::order_index_row_props,
                );
            }
        } else {
            plan.structure_changed = true;
            t.wrap_item(plan, dom, node, LocalName::Del);
        }
    }
    #[inline]
    /// `EDIT-03` 表格通则：**单元格最后一个块必须是 `w:p`**（Word 的约束）。计划生效后 `container`
    /// （只管 `w:tc`）的末尾不是段落时，追加一个 `New` 空 `w:p`。`removed` 是这次计划里要删除 / 搬走的节点。
    fn keep_cell_paragraph(
        dom: &Dom,
        container: NodeId,
        removed: Option<NodeId>,
        plan: &mut MutationPlan,
    ) {
        if !dom.is(container, QName::w(LocalName::Tc)) {
            return;
        }
        let last = dom
            .children(container)
            .iter()
            .copied()
            .rev()
            .filter(|&c| dom.node(c).dirty != Dirty::Deleted && dom.element(c).is_some())
            .find(|&c| Some(c) != removed);
        if last.is_some_and(|n| dom.is(n, QName::w(LocalName::P))) {
            return;
        }
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(container),
            before: None,
            node: NewElement::new(QName::w(LocalName::P)),
        });
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    fn insert_block(
        &mut self,
        at: BlockPos,
        block: NewBlock,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        // 新图表先建 part（图表 / 工作簿 / 关系），块本身换成绘图段落（任务 6.6）
        let block = EditSession::materialize_at(s, block, Some(at.at))?;
        // 生成器（TOC / INDEX）展开成好几段：按序全插进去
        let blocks = match block {
            NewBlock::Many(v) => v,
            b => vec![b],
        };
        let part = s.part_or_main(at.part);
        let dom = s.dom_in(at.part)?;
        let (parent, before) = MutationPlan::block_site(dom, at.at)?;
        let mut tracker = Tracker::new(s.document(), ctx);
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        let mut last_is_para = true;
        for block in blocks {
            last_is_para = matches!(block, NewBlock::Paragraph { .. } | NewBlock::Xml(_));
            let opaque = matches!(block, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
            let node = MutationPlan::new_block_element(dom, block);
            // 追踪：段落的内容进 `w:ins` 且段落标记标插入；表格每行 `trPr/w:ins`；其他整块包 `w:ins`
            let node = match &mut tracker {
                None => node,
                Some(t) => t.mark_new_block_inserted(node, opaque),
            };
            plan.node_edits.push(NodeEdit::Insert { parent: Target::Node(parent), before, node });
        }
        // 插在格尾的非段落块（表格等）后面要补一个空段落
        if before.is_none() && !last_is_para {
            MutationPlan::keep_cell_paragraph(dom, parent, None, &mut plan);
        }
        s.commit_plan(plan)
    }
    #[inline]
    fn delete_block(
        &mut self,
        part: Option<PartId>,
        node: NodeId,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let dom = s.dom_in(part)?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            return Err(Error::edit(DiagCode::EditBadPosition, "块不存在或已删除"));
        }
        // `FLD-13`：这个块里带着某个字段的一端、另一端在块外（TOC / INDEX / BIBLIOGRAPHY 这类跨段的块字段
        // 最常见）。删掉它会把另一端留成孤儿，`FLD_STRAY_END` 是引擎自己造成的缺陷，于是**每次**保存都失败、
        // 整个会话再也存不下去。在这里拒绝，`EDIT-05` 保证状态一点没动；要删整个字段请走 `UpdateBlockField`。
        // （真实 Word 语料 `fields-toc-stale` 撞到的，`docs/09` 第三轮。）
        if let Some(idx) = s.document().fields_in(s.part_or_main(part)) {
            let inside = |n: NodeId| n == node || dom.ancestors(n).any(|a| a == node);
            if let Some(f) =
                idx.fields().iter().find(|f| inside(f.form.head()) != inside(f.form.tail()))
            {
                return Err(Error::edit(
                    DiagCode::EditSplitField,
                    format!(
                        "这个块只含 {:?} 字段的一端，删掉它会让另一端变成孤儿；要删整个字段请用 UpdateBlockField",
                        f.keyword()
                    ),
                ));
            }
        }
        // 追踪：**块留着**（`spec/18` 7.3）
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let mut plan = MutationPlan::new(s.part_or_main(part));
            MutationPlan::plan_delete_block_tracked(&mut plan, dom, &mut t, node);
            return s.commit_plan(plan);
        }
        let mut plan = MutationPlan::new(s.part_or_main(part));
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Delete(node));
        if let Some(parent) = dom.parent(node) {
            MutationPlan::keep_cell_paragraph(dom, parent, Some(node), &mut plan);
        }
        s.commit_plan(plan)
    }
    #[inline]
    fn move_block(
        &mut self,
        from: Option<PartId>,
        node: NodeId,
        to: BlockPos,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        // `docs/03` §8.2 第一阶段：追踪时不生成 `moveFrom` / `moveTo`（调用方用 Delete + Insert）
        if ctx.track_changes.is_some() {
            return Err(Error::edit(
                DiagCode::EditUnsupportedTrackedMove,
                "track_changes 开启时不支持 MoveBlock；请用 DeleteBlock + InsertBlock",
            ));
        }
        if s.part_or_main(from) != s.part_or_main(to.part) {
            return EditSession::move_block_cross_part(s, from, node, to, ctx);
        }
        let dom = s.dom_in(to.part)?;
        let (parent, before) = MutationPlan::block_site(dom, to.at)?;
        let mut plan = MutationPlan::new(s.part_or_main(to.part));
        if before == Some(node) {
            return s.commit_plan(plan); // 已在目标位置
        }
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Move { node, parent: Target::Node(parent), before });
        // 搬出单元格后原格可能空了；搬进格尾的非段落块后面要补段落
        if let Some(from) = dom.parent(node).filter(|&f| f != parent) {
            MutationPlan::keep_cell_paragraph(dom, from, Some(node), &mut plan);
        }
        if before.is_none() && !dom.is(node, QName::w(LocalName::P)) {
            MutationPlan::keep_cell_paragraph(dom, parent, None, &mut plan);
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// 跨 part 的 `MoveBlock`（`XML-12` 规则 E′，`spec/18` 7.6）。
    ///
    /// 子树在两个 part 之间搬家时前缀不能照抄：目标 part 的命名空间声明是另一套。做法是把子树
    /// 连同**源处作用域里的全部有效声明**序列化成一段 XML，再用目标 part 的 `parse_fragment`
    /// 读进去——能绑到同 URI 的复用目标前缀，绑不上的在子树根上声明（`Dom::parse_fragment`
    /// 与序列化器已经保证这一条）。
    ///
    /// 范围：目标容器 `rescan`（搬过去的标记在新 part 里重新成范围），源那边照 `SPAN-06/07`
    /// 走删除（整个落在被搬块内的范围随之消失）。块字段被劈开 → `Err(EDIT_SPLIT_FIELD)`。
    fn move_block_cross_part(
        &mut self,
        from: Option<PartId>,
        node: NodeId,
        to: BlockPos,
        _ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let src_part = s.part_or_main(from);
        let dst_part = s.part_or_main(to.part);
        let dom = s.dom_in(from)?;
        if (node.0 as usize) >= dom.node_count() || dom.node(node).dirty == Dirty::Deleted {
            return Err(Error::edit(DiagCode::EditBadPosition, "块不存在或已删除"));
        }
        // 与 `DeleteBlock` 同一条：块里只有块字段的一端 → 拒绝（另一端会变孤儿）
        if let Some(idx) = s.document().fields_in(src_part) {
            let inside = |n: NodeId| n == node || dom.ancestors(n).any(|a| a == node);
            if idx.fields().iter().any(|f| inside(f.form.head()) != inside(f.form.tail())) {
                return Err(Error::edit(
                    DiagCode::EditSplitField,
                    "这个块只含某个块字段的一端，搬走会让另一端变成孤儿",
                ));
            }
        }
        // ① 源处：子树 + 作用域里的有效声明 → 一段自足的 XML
        let mut bytes = Vec::new();
        crate::save::serialize_subtree(dom, node, &mut bytes)
            .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("子树序列化失败: {e}")))?;
        let body = String::from_utf8(bytes)
            .map_err(|_| Error::edit(DiagCode::EditPlanInvalid, "子树不是 UTF-8"))?;
        let flavor = s.flavor_in(from);
        let scope = dom.namespace_scope(node);
        let mut decls = String::new();
        for (prefix, ns) in scope.effective() {
            let Some(uri) = ns.uri(flavor) else { continue };
            match prefix.map(|p| dom.interner().resolve(p).to_string()) {
                Some(p) => decls.push_str(&format!(r#" xmlns:{p}="{uri}""#)),
                None => decls.push_str(&format!(r#" xmlns="{uri}""#)),
            }
        }
        let wrapped = format!("<rsword-move{decls}>{body}</rsword-move>");
        // ② 目标 part：重解析（前缀按目标作用域重新落）
        let dst_dom = s
            .package_mut()
            .dom_mut(dst_part)?
            .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "目标 part 没有 DOM"))?;
        let frag = crate::xml::parse_fragment(dst_dom, &wrapped)
            .map_err(|e| Error::edit(DiagCode::EditPlanInvalid, format!("子树重解析失败: {e}")))?;
        let moved = frag
            .into_iter()
            .next()
            .and_then(|e| {
                e.children.into_iter().find_map(|c| match c {
                    crate::xml::NewNode::Element(x) => Some(x),
                    crate::xml::NewNode::Text(_) => None,
                })
            })
            .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "重解析后子树为空"))?;
        let dst_dom = s.dom_in(to.part)?;
        let (parent, before) = MutationPlan::block_site(dst_dom, to.at)?;
        let mut plan = MutationPlan::new(dst_part);
        plan.structure_changed = true;
        // 搬过去的内容里可能带范围标记：按新 part 的 DOM 重建这个容器的端点
        plan.span.rescan.push(parent);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before,
            node: moved,
        });
        if before.is_none() {
            MutationPlan::keep_cell_paragraph(dst_dom, parent, None, &mut plan);
        }
        let mut result = s.commit_plan(plan)?;
        // ③ 源处删掉。整个落在被搬块内的范围**从源索引里摘掉**：内容不是被销毁而是搬走了，
        // 标记已经跟着到了目标 part。不摘的话 `SPAN-07` 会把书签折叠留在删除点，
        // `SPAN-09` 再物化出一个同名标记——同一个书签就在两个 part 里各有一份了
        let ends: Vec<(crate::span::SpanId, Option<NodeId>, Option<NodeId>)> = {
            let index = s.spans_of(src_part)?;
            index
                .live()
                .map(|sp| (sp.id, sp.start.and_then(|a| a.marker), sp.end.and_then(|a| a.marker)))
                .collect()
        };
        let dead: Vec<crate::span::SpanId> = {
            let dom = s.dom_in(from)?;
            let inside = |n: NodeId| n == node || dom.ancestors(n).any(|a| a == node);
            ends.into_iter()
                .filter(|&(_, a, b)| a.is_some_and(inside) && b.is_some_and(inside))
                .map(|(id, _, _)| id)
                .collect()
        };
        let dom = s.dom_in(from)?;
        let mut plan = MutationPlan::new(src_part);
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Delete(node));
        if let Some(p) = dom.parent(node) {
            MutationPlan::keep_cell_paragraph(dom, p, Some(node), &mut plan);
        }
        result.absorb(s.commit_plan(plan)?);
        for span in dead {
            s.drop_span(src_part, span);
        }
        Ok(result)
    }
}

impl MutationPlan {
    #[inline]
    /// 段落里第 `boundary` 个内容项（`None` = 边界在末尾，插入时追加）。
    fn content_site(dom: &Dom, para: NodeId, boundary: u32) -> Option<NodeId> {
        crate::span::content_children(dom, para).get(boundary as usize).copied()
    }
    #[inline]
    /// `w:commentReference` run（带 `CommentReference` 字符样式，与 Word 一致）。
    fn comment_reference_run(id: &str) -> NewElement {
        let rpr = NewElement::new(QName::w(LocalName::RPr)).with_child(
            NewElement::new(QName::w(LocalName::RStyle))
                .with_attr(QName::w(LocalName::Val), "CommentReference"),
        );
        NewElement::new(QName::w(LocalName::R)).with_child(rpr).with_child(
            NewElement::new(QName::w(LocalName::CommentReference))
                .with_attr(QName::w(LocalName::Id), id),
        )
    }
    #[inline]
    /// 纯文本按 `\n` 分段，每段一个 run（可带一份共用的 `rPr`）。
    fn text_entry_paras(text: &str, rpr: Option<&NewElement>) -> EntryParas {
        let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.split('\n').collect() };
        lines
            .into_iter()
            .map(|line| vec![NewRun { text: line.to_string(), props: rpr.cloned() }])
            .collect()
    }
    #[inline]
    /// 一个 `w:r`：`props` 是整份 `w:rPr`，控制字符按 `MOD-06` 折回 `w:tab` / `w:br`。
    fn entry_run(r: &NewRun) -> NewElement {
        let mut e = NewElement::new(QName::w(LocalName::R));
        if let Some(p) = &r.props {
            e.push_child(p.clone());
        }
        for seg in Emitter::text_segments(&r.text, false) {
            e.push_child(seg);
        }
        e
    }
    #[inline]
    /// 批注 / 注释条目的段落：首段可带一个引导 run（批注的 `w:annotationRef`、注释的
    /// `w:footnoteRef`），末段带 `w14:paraId`（`commentsExtended` 按它关联）。
    fn entry_paragraphs(
        paras: &EntryParas,
        para_id: Option<&str>,
        lead: Option<NewElement>,
    ) -> Vec<NewElement> {
        let w14 = |l: LocalName| QName::new(NsId::W14, l);
        let last = paras.len().saturating_sub(1);
        paras
            .iter()
            .enumerate()
            .map(|(i, runs)| {
                let mut p = NewElement::new(QName::w(LocalName::P));
                if i == last
                    && let Some(pid) = para_id
                {
                    p.push_attr(w14(LocalName::ParaId), pid.to_string());
                }
                if i == 0
                    && let Some(l) = &lead
                {
                    p.push_child(l.clone());
                }
                for r in runs {
                    p.push_child(MutationPlan::entry_run(r));
                }
                p
            })
            .collect()
    }
    #[inline]
    /// 批注条目首段的引用标记 run（`CommentReference` 样式 + `w:annotationRef`）。
    fn annotation_ref_run() -> NewElement {
        let rpr = NewElement::new(QName::w(LocalName::RPr)).with_child(
            NewElement::new(QName::w(LocalName::RStyle))
                .with_attr(QName::w(LocalName::Val), "CommentReference"),
        );
        NewElement::new(QName::w(LocalName::R))
            .with_child(rpr)
            .with_child(NewElement::new(QName::w(LocalName::AnnotationRef)))
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// `InlinePos` → 段落内容序列的边界。位置必须已经在 inline 边界上（先 `split_at`）。
    fn content_boundary(&self, para: NodeId, at: InlinePos) -> Result<u32> {
        let s = self;
        let tb = EditSession::require_text_block(s, at.part, para)?;
        let dom = s.dom_in(at.part)?;
        match at.offset.locate(tb)? {
            Loc::Boundary { index } => {
                let len = crate::span::content_len(dom, para);
                match tb.inlines.get(index) {
                    // 段落层的内容项：inline 可能在 `w:hyperlink` / `w:ins` 里，取它在段落下的那一层
                    Some(inline) => {
                        let n = EditSession::boundary_node(s, at.part, tb, inline, Side::Right)?;
                        Ok(crate::span::boundary_before(
                            dom,
                            para,
                            InlinePos::top_child(dom, para, n),
                        )
                        .unwrap_or(len))
                    }
                    None => Ok(len),
                }
            }
            _ => Err(unsupported("批注端点没落在 inline 边界上（内部错误）")),
        }
    }
    #[inline]
    /// 会话内唯一的 `w14:paraId`（8 位十六进制，避开已用的）。
    fn fresh_para_id(&self, seed: u32) -> String {
        let s = self;
        let used: Vec<&str> =
            s.document().comments.items.iter().filter_map(|c| c.para_id.as_deref()).collect();
        let mut n = 0x1000_0000u32.wrapping_add(seed.wrapping_mul(0x9E37_79B9));
        loop {
            let candidate = format!("{n:08X}");
            if !used.contains(&candidate.as_str()) {
                return candidate;
            }
            n = n.wrapping_add(1);
        }
    }
    #[inline]
    fn add_comment(
        &mut self,
        from: InlinePos,
        to: InlinePos,
        c: &NewComment,
    ) -> Result<MutationResult> {
        let s = self;
        if from.para != to.para {
            return Err(Error::edit(
                DiagCode::EditCrossParagraph,
                "AddComment 两端不在同一段落（M2 只支持同段）",
            ));
        }
        if from.offset > to.offset {
            return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
        }
        let part = s.main_part();
        // 位置先落到 inline 边界（拆 run 是独立阶段，失败由事务回滚）
        let mut result = MutationResult::default();
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let loc_to = to.offset.locate(tb)?;
        EditSession::split_at(s, to, loc_to, &mut result)?;
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let loc_from = from.offset.locate(tb)?;
        EditSession::split_at(s, from, loc_from, &mut result)?;

        // 端点可能位于受保护的 inline 内；必须在创建 part/关系之前拒绝。
        let a = EditSession::content_boundary(s, from.para, from)?;
        let b = EditSession::content_boundary(s, to.para, to)?;

        // 批注部件与条目（`SAVE-05` + `EDIT-06`）
        let comments_part = s.ensure_comments_part()?;
        // `w:id` 要在**范围索引**里也没人用过：条目删了、范围还留在索引里等物化时，只看
        // `comments.xml` 会把那个号再发一次（保存时 `SPAN_DUP_START`，`TEST-07` 抓到的）
        let id = {
            let from_entries = s.document().comments.next_id();
            let part = s.main_part();
            let from_index = s
                .spans_of(part)
                .map(|idx| {
                    idx.live()
                        .filter(|sp| sp.class() == crate::span::RangeClass::Comment)
                        .filter_map(|sp| sp.pair_id().trim().parse::<u32>().ok())
                        .max()
                        .map_or(1, |m| m + 1)
                })
                .unwrap_or(1);
            from_entries.max(from_index).to_string()
        };
        let para_id = EditSession::fresh_para_id(s, s.document().comments.items.len() as u32 + 1);
        let mut entry = NewElement::new(QName::w(LocalName::Comment))
            .with_attr(QName::w(LocalName::Id), id.clone());
        entry.push_attr(QName::w(LocalName::Author), c.author.clone());
        if let Some(i) = &c.initials {
            entry.push_attr(QName::w(LocalName::Initials), i.clone());
        }
        if let Some(d) = &c.date {
            entry.push_attr(QName::w(LocalName::Date), d.clone());
        }
        for p in MutationPlan::entry_paragraphs(
            &MutationPlan::text_entry_paras(&c.text, None),
            Some(&para_id),
            Some(MutationPlan::annotation_ref_run()),
        ) {
            entry.push_child(p);
        }
        let cdom = s.package().part(comments_part).dom().expect("comments part is parsed");
        let croot = cdom.root();
        let mut cplan = MutationPlan::new(comments_part);
        cplan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(croot),
            before: None,
            node: entry,
        });
        result.absorb(s.commit_plan(cplan)?);

        // 正文：范围标记 + reference run
        let dom = s.dom();
        let start_before = MutationPlan::content_site(dom, from.para, a);
        let end_before = MutationPlan::content_site(dom, to.para, b);
        let marker = |local: LocalName| {
            NewElement::new(QName::w(local)).with_attr(QName::w(LocalName::Id), id.clone())
        };
        let mut plan = MutationPlan::new(part);
        plan.touch(from.para);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(from.para),
            before: start_before,
            node: marker(LocalName::CommentRangeStart),
        });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(to.para),
            before: end_before,
            node: marker(LocalName::CommentRangeEnd),
        });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(to.para),
            before: end_before,
            node: MutationPlan::comment_reference_run(&id),
        });
        let r = s.commit_plan(plan)?;
        let (start_marker, end_marker, ref_run) = (
            r.created[0].ok_or_else(|| unsupported("范围起点没创建"))?,
            r.created[1].ok_or_else(|| unsupported("范围终点没创建"))?,
            r.created[2].ok_or_else(|| unsupported("reference run 没创建"))?,
        );
        result.absorb(r);

        // 登记范围：标记已经在 DOM 里，物化时正好在锚点位置上（不会重发）
        let dom = s.dom();
        let anchor = |node: NodeId, aff: Affinity| {
            Anchor::at(
                from.para,
                crate::span::boundary_before(dom, from.para, node).unwrap_or(0),
                aff,
                node,
            )
        };
        let span = RangeSpan {
            id: SpanId(0),
            part,
            flow: s.document().flows.flow_of(from.para).unwrap_or(FlowId(0)),
            kind: RangeKind::Comment { id: id.clone(), reference: Some(ref_run) },
            origin: SpanOrigin::New,
            implicit: false,
            removed: false,
            start: Some(anchor(start_marker, Affinity::Right)),
            end: Some(anchor(end_marker, Affinity::Left)),
        };
        s.push_span(part, span)?;
        // 回复与已解决在 `commentsExtended`
        if c.parent_id.is_some() || c.done {
            let parent_para = c
                .parent_id
                .as_deref()
                .and_then(|pid| s.document().comments.get(pid))
                .and_then(|p| p.para_id.clone());
            result.absorb(EditSession::set_comment_ex(
                s,
                &para_id,
                parent_para.as_deref(),
                c.done,
            )?);
        }
        s.rebuild()?;
        Ok(result)
    }
    #[inline]
    /// `commentsExtended` 里的一条 `w15:commentEx`（没有就建 part / 建条目）。
    fn set_comment_ex(
        &mut self,
        para_id: &str,
        parent_para: Option<&str>,
        done: bool,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.ensure_comments_extended_part()?;
        let w15 = |l: LocalName| QName::new(NsId::W15, l);
        let dom = s.package().part(part).dom().expect("commentsExtended is parsed");
        let root = dom.root();
        let existing = dom.semantic_children(root).find(|&n| {
            dom.is(n, w15(LocalName::CommentEx))
                && dom.attr_value(n, w15(LocalName::ParaId)).as_deref() == Some(para_id)
        });
        let mut plan = MutationPlan::new(part);
        if let Some(node) = existing {
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(node),
                name: w15(LocalName::Done),
                value: if done { "1".into() } else { "0".into() },
            });
            match parent_para {
                Some(p) => plan.node_edits.push(NodeEdit::SetAttr {
                    node: Target::Node(node),
                    name: w15(LocalName::ParaIdParent),
                    value: p.to_string(),
                }),
                None => plan.node_edits.push(NodeEdit::RemoveAttr {
                    node: Target::Node(node),
                    name: w15(LocalName::ParaIdParent),
                }),
            }
        } else {
            let mut e = NewElement::new(w15(LocalName::CommentEx))
                .with_attr(w15(LocalName::ParaId), para_id);
            if let Some(p) = parent_para {
                e.push_attr(w15(LocalName::ParaIdParent), p);
            }
            e.push_attr(w15(LocalName::Done), if done { "1" } else { "0" });
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(root),
                before: None,
                node: e,
            });
        }
        s.commit_plan(plan)
    }
    #[inline]
    fn remove_comment(&mut self, id: &str) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let entry = s.document().comments.get(id).map(|c| (c.node, c.para_id.clone())).ok_or_else(
            || Error::edit(DiagCode::EditBadPosition, format!("没有 id 为 {id} 的批注")),
        )?;
        let comments_part =
            s.document().comments.part.ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, "批注条目在的 part 找不到")
            })?;
        // 正文：范围标记与 reference run
        let mut victims: Vec<NodeId> = Vec::new();
        let mut spans: Vec<SpanId> = Vec::new();
        {
            // 索引可能还没建（这是本次会话第一次写主 part）
            let index = s.spans_of(part)?;
            for span in index.live() {
                if span.class() != RangeClass::Comment || span.pair_id() != id {
                    continue;
                }
                spans.push(span.id);
                victims.extend(span.start.and_then(|a| a.marker));
                victims.extend(span.end.and_then(|a| a.marker));
                if let RangeKind::Comment { reference: Some(r), .. } = &span.kind {
                    victims.push(*r);
                }
            }
        }
        let dom = s.dom();
        victims.retain(|&n| dom.node(n).dirty != Dirty::Deleted);
        if !victims.is_empty() {
            let mut plan = MutationPlan::new(part);
            for n in &victims {
                if let Some(p) = dom.ancestors(*n).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                    plan.touch(p);
                }
                plan.node_edits.push(NodeEdit::Delete(*n));
            }
            s.commit_plan(plan)?;
        }
        for span in spans {
            s.drop_span(part, span);
        }
        // 条目与 `commentsExtended` 条目
        let mut plan = MutationPlan::new(comments_part);
        plan.node_edits.push(NodeEdit::Delete(entry.0));
        let mut result = s.commit_plan(plan)?;
        if let (Some(ex_part), Some(pid)) = (s.document().comments.extended_part, entry.1) {
            let dom = s.package().part(ex_part).dom().expect("commentsExtended is parsed");
            let w15 = |l: LocalName| QName::new(NsId::W15, l);
            let victim = dom.semantic_children(dom.root()).find(|&n| {
                dom.is(n, w15(LocalName::CommentEx))
                    && dom.attr_value(n, w15(LocalName::ParaId)).as_deref() == Some(pid.as_str())
            });
            if let Some(node) = victim {
                let mut plan = MutationPlan::new(ex_part);
                plan.node_edits.push(NodeEdit::Delete(node));
                result.absorb(s.commit_plan(plan)?);
            }
        }
        s.rebuild()?;
        Ok(result)
    }
    #[inline]
    fn set_comment_text(
        &mut self,
        id: &str,
        text: &str,
        done: Option<bool>,
    ) -> Result<MutationResult> {
        let s = self;
        let (node, para_id, first_rpr) = {
            let c = s.document().comments.get(id).ok_or_else(|| {
                Error::edit(DiagCode::EditBadPosition, format!("没有 id 为 {id} 的批注"))
            })?;
            let dom = s.package().part(s.document().comments.part.expect("有条目就有 part")).dom();
            // 保留第一个有字 run 的格式（加粗 / 颜色的批注改字后不变素）
            let rpr = dom.and_then(|d| {
                c.rich
                    .iter()
                    .flatten()
                    .next()
                    .and_then(|r| {
                        d.semantic_children(r.node).find(|&n| d.is(n, QName::w(LocalName::RPr)))
                    })
                    .and_then(|n| NewElement::from_dom(d, n, &mut crate::xml::Interner::new()))
            });
            (c.node, c.para_id.clone(), rpr)
        };
        let comments_part =
            s.document().comments.part.ok_or_else(|| unsupported("批注条目在的 part 找不到"))?;
        let para_id = para_id.unwrap_or_else(|| EditSession::fresh_para_id(s, 1));
        let dom = s.package().part(comments_part).dom().expect("comments part is parsed");
        let mut plan = MutationPlan::new(comments_part);
        for c in dom.semantic_children(node) {
            if dom.is(c, QName::w(LocalName::P)) {
                plan.node_edits.push(NodeEdit::Delete(c));
            }
        }
        let paras = MutationPlan::text_entry_paras(text, first_rpr.as_ref());
        for p in MutationPlan::entry_paragraphs(
            &paras,
            Some(&para_id),
            Some(MutationPlan::annotation_ref_run()),
        ) {
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(node),
                before: None,
                node: p,
            });
        }
        let mut result = s.commit_plan(plan)?;
        if let Some(done) = done {
            let parent = s.document().comments.get(id).and_then(|c| c.parent_id.clone());
            let parent_para = parent
                .as_deref()
                .and_then(|pid| s.document().comments.get(pid))
                .and_then(|p| p.para_id.clone());
            result.absorb(EditSession::set_comment_ex(s, &para_id, parent_para.as_deref(), done)?);
        }
        s.rebuild()?;
        Ok(result)
    }
    #[inline]
    /// compat 的权威列表路径用的批注条目 upsert（`COMPAT-04` 的 `SaveOptions.comments`）。
    ///
    /// 条目在就改（正文重写、属性按需改），不在就新建；**不动正文里的范围标记**——标记的位置由
    /// 块的 `commentStarts` / `commentEnds` / `commentIds` 决定。
    #[cfg(feature = "compat-ts")]
    fn upsert_comment_entry(
        &mut self,
        id: &str,
        c: &NewComment,
        paras: &EntryParas,
    ) -> Result<MutationResult> {
        let s = self;
        let (author, initials, date) = (
            (!c.author.is_empty()).then_some(c.author.as_str()),
            c.initials.as_deref(),
            c.date.as_deref(),
        );
        let (parent_id, done) = (c.parent_id.as_deref(), c.done);
        let part = s.ensure_comments_part()?;
        let existing = s.document().comments.get(id).map(|c| (c.node, c.para_id.clone()));
        let mut result = MutationResult::default();
        let para_id = match &existing {
            Some((_, Some(pid))) => pid.clone(),
            _ => EditSession::fresh_para_id(s, id.len() as u32 + 1),
        };
        let body = MutationPlan::entry_paragraphs(
            paras,
            Some(&para_id),
            Some(MutationPlan::annotation_ref_run()),
        );
        let mut plan = MutationPlan::new(part);
        match existing {
            Some((node, _)) => {
                let dom = s.package().part(part).dom().expect("comments part is parsed");
                for c in dom.semantic_children(node) {
                    if dom.is(c, QName::w(LocalName::P)) {
                        plan.node_edits.push(NodeEdit::Delete(c));
                    }
                }
                for (name, value) in [
                    (LocalName::Author, author),
                    (LocalName::Initials, initials),
                    (LocalName::Date, date),
                ] {
                    match value {
                        Some(v) => plan.node_edits.push(NodeEdit::SetAttr {
                            node: Target::Node(node),
                            name: QName::w(name),
                            value: v.to_string(),
                        }),
                        None => plan.node_edits.push(NodeEdit::RemoveAttr {
                            node: Target::Node(node),
                            name: QName::w(name),
                        }),
                    }
                }
                for p in body {
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(node),
                        before: None,
                        node: p,
                    });
                }
            }
            None => {
                let dom = s.package().part(part).dom().expect("comments part is parsed");
                let root = dom.root();
                let mut entry = NewElement::new(QName::w(LocalName::Comment))
                    .with_attr(QName::w(LocalName::Id), id);
                if let Some(a) = author {
                    entry.push_attr(QName::w(LocalName::Author), a);
                }
                if let Some(i) = initials {
                    entry.push_attr(QName::w(LocalName::Initials), i);
                }
                if let Some(d) = date {
                    entry.push_attr(QName::w(LocalName::Date), d);
                }
                for p in body {
                    entry.push_child(p);
                }
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(root),
                    before: None,
                    node: entry,
                });
            }
        }
        result.absorb(s.commit_plan(plan)?);
        if parent_id.is_some() || done {
            let parent_para = parent_id
                .and_then(|pid| s.document().comments.get(pid))
                .and_then(|p| p.para_id.clone());
            result.absorb(EditSession::set_comment_ex(s, &para_id, parent_para.as_deref(), done)?);
        }
        s.rebuild()?;
        Ok(result)
    }
    #[inline]
    /// 注释条目 upsert（`SaveOptions.footnotes` / `endnotes`）。
    ///
    /// 条目在就只重写正文段落、**保留自引用标记 run**（`w:footnoteRef` 是编号，不能丢）；
    /// 不在就新建条目。结构条目（`separator` 一类）一个字节不动。
    fn upsert_note_entry(
        &mut self,
        endnote: bool,
        id: &str,
        paras: &EntryParas,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.ensure_notes_part(endnote)?;
        let (entry_name, ref_name) = if endnote {
            (LocalName::Endnote, LocalName::EndnoteRef)
        } else {
            (LocalName::Footnote, LocalName::FootnoteRef)
        };
        let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
        let existing = notes.get(id).map(|n| n.node);
        let mut plan = MutationPlan::new(part);
        let dom = s.package().part(part).dom().expect("notes part is parsed");
        match existing {
            Some(node) => {
                // 只重发正文段落：把段落里除自引用标记 run 之外的内容换掉
                let ref_run = dom
                    .descendants(node)
                    .find(|&n| dom.is(n, QName::w(ref_name)))
                    .and_then(|m| dom.ancestors(m).find(|&a| dom.is(a, QName::w(LocalName::R))));
                let lead = ref_run
                    .and_then(|r| NewElement::from_dom(dom, r, &mut crate::xml::Interner::new()));
                for c in dom.semantic_children(node) {
                    if dom.is(c, QName::w(LocalName::P)) {
                        plan.node_edits.push(NodeEdit::Delete(c));
                    }
                }
                for p in MutationPlan::entry_paragraphs(paras, None, lead) {
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: Target::Node(node),
                        before: None,
                        node: p,
                    });
                }
            }
            None => {
                let root = dom.root();
                let lead = NewElement::new(QName::w(LocalName::R))
                    .with_child(NewElement::new(QName::w(ref_name)));
                let mut entry =
                    NewElement::new(QName::w(entry_name)).with_attr(QName::w(LocalName::Id), id);
                for p in MutationPlan::entry_paragraphs(paras, None, Some(lead)) {
                    entry.push_child(p);
                }
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(root),
                    before: None,
                    node: entry,
                });
            }
        }
        let r = s.commit_plan(plan)?;
        s.rebuild()?;
        Ok(r)
    }
    #[inline]
    /// 从注释部件里删掉一条正文条目（结构条目不动）。
    fn remove_note_entry(&mut self, endnote: bool, id: &str) -> Result<MutationResult> {
        let s = self;
        let notes = if endnote { &s.document().endnotes } else { &s.document().footnotes };
        let Some(part) = notes.part else { return Ok(MutationResult::default()) };
        let Some(node) = notes.get(id).map(|n| n.node) else {
            return Ok(MutationResult::default());
        };
        let mut plan = MutationPlan::new(part);
        plan.node_edits.push(NodeEdit::Delete(node));
        let r = s.commit_plan(plan)?;
        s.rebuild()?;
        Ok(r)
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditSession {
    #[inline]
    /// 段落里横跨内容边界 `k` 的字段：拆分会让它跨段（`FLD-08` 里那就变成 `Block` 策略）。
    ///
    /// 判定按段落层的内容项下标：字段的 head 与 tail 在段落下各属一个内容项（可能是同一个），
    /// 边界落在两者**之间**就是横跨。原子形态字段的内部位置在 `locate` 那一步就被拒了。
    fn field_across(&self, para: NodeId, k: u32) -> Option<crate::span::FieldId> {
        let s = self;
        let dom = s.dom();
        let fields = &s.document().fields;
        for f in fields.fields() {
            let (head, tail) = (f.form.head(), f.form.tail());
            if !dom.ancestors(head).any(|a| a == para) && head != para {
                continue;
            }
            let item_of = |n: NodeId| {
                crate::span::boundary_before(dom, para, InlinePos::top_child(dom, para, n))
            };
            let (Some(h), Some(t)) = (item_of(head), item_of(tail)) else { continue };
            if k > h && k <= t {
                return Some(f.id);
            }
        }
        None
    }
    #[inline]
    /// `Block` 策略字段的结果段落只读（`FLD-07`）。
    fn refuse_block_field_result(&self, para: NodeId) -> Result<()> {
        let s = self;
        if s.document().fields.block_result_paragraphs(s.dom()).contains_key(&para) {
            return Err(unsupported("Block 字段（TOC 等）的结果段落只读"));
        }
        Ok(())
    }
    #[inline]
    fn split_paragraph(&mut self, at: InlinePos, ctx: &EditContext) -> Result<MutationResult> {
        let s = self;
        let part = s.part_or_main(at.part);
        // 块字段与它的结果段落只在主 part 有索引（`FLD-08`）
        if at.part.is_none() {
            EditSession::refuse_block_field_result(s, at.para)?;
        }
        let tb = EditSession::require_text_block(s, at.part, at.para)?;
        let loc = at.offset.locate(tb)?;
        // 位置落在 run 内部 → 先拆 run（原子内部的位置 `locate` 已经拒了）
        let mut result = MutationResult::default();
        EditSession::split_at(s, at, loc, &mut result)?;
        // `k` 是内容序列里的边界下标（`SPAN-06` 的拆分规则要用）；字段跨段的检查只在主 part
        let k = EditSession::content_boundary(s, at.para, at)?;
        if at.part.is_none()
            && let Some(id) = EditSession::field_across(s, at.para, k)
        {
            return Err(Error::edit(
                DiagCode::EditSplitField,
                format!("拆分会让字段 {} 跨段（FLD-08）", id.0),
            ));
        }
        let dom = s.dom_in(at.part)?;
        let parent = dom.parent(at.para).ok_or_else(|| unsupported("段落没有父节点"))?;
        let ppr = MutationPlan::ppr_of(dom, at.para);
        let after = Dom::next_live_sibling(dom, at.para);
        // 阶段 1：建新段落（`pPr` 字节克隆，`XML-12` 规则 F），插在原段之后
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(parent),
            before: after,
            node: NewElement::new(QName::w(LocalName::P)),
        });
        if let Some(ppr) = ppr {
            plan.node_edits.push(NodeEdit::InsertClone {
                parent: Target::New(0),
                before: None,
                source: ppr,
            });
        }
        let r = s.commit_plan(plan)?;
        let tail = r.created[0].ok_or_else(|| unsupported("新段落没创建"))?;
        result.absorb(r);

        // 阶段 2：把边界之后的内容项与范围标记搬进新段落（`SPAN-06` 拆分规则由 `SpanPolicy` 表达）
        let dom = s.dom_in(at.part)?;
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        plan.span.splits.push(crate::span::ContainerSplit { source: at.para, boundary: k, tail });
        let mut index = 0u32;
        for c in dom.semantic_children(at.para).collect::<Vec<_>>() {
            let Some(name) = dom.name(c) else { continue };
            if is_property_element(name) {
                continue;
            }
            if crate::span::is_range_marker(name) {
                // 标记跟着它右边的内容走：边界处的标记留在前段（终点）或跟去后段（起点），
                // 物化会按锚点摆正，这里只要不把它落在错误的段里
                if index > k {
                    plan.node_edits.push(NodeEdit::Move {
                        node: c,
                        parent: Target::Node(tail),
                        before: None,
                    });
                }
                continue;
            }
            if index >= k {
                plan.node_edits.push(NodeEdit::Move {
                    node: c,
                    parent: Target::Node(tail),
                    before: None,
                });
            }
            index += 1;
        }
        result.absorb(s.commit_plan(plan)?);

        // 阶段 3（追踪）：拆出来的**前**段的段落标记是新加的 → `pPr/rPr/w:ins`。
        // 必须在阶段 1 克隆 `pPr` 之后做，否则后段会跟着带上这个标记
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let dom = s.dom_in(at.part)?;
            let mut plan = MutationPlan::new(part);
            plan.touch(at.para);
            t.para_mark(&mut plan, dom, at.para, LocalName::Ins);
            if !plan.is_empty() {
                result.absorb(s.commit_plan(plan)?);
            }
        }
        result.structure_changed = true;
        Ok(result)
    }
    #[inline]
    fn merge_with_next(
        &mut self,
        at: Option<PartId>,
        para: NodeId,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.part_or_main(at);
        MutationPlan::require_paragraph(s.dom_in(at)?, para)?;
        // 块字段的结果段落只读（`FLD-08`）；字段索引只对主 part 建了，别的 part 里没有块字段的概念
        if at.is_none() {
            EditSession::refuse_block_field_result(s, para)?;
        }
        let dom = s.dom_in(at)?;
        let next = Dom::next_live_element_sibling(dom, para)
            .filter(|&n| dom.is(n, QName::w(LocalName::P)))
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "下一个块不是段落"))?;
        if at.is_none() {
            EditSession::refuse_block_field_result(s, next)?;
        }
        // 追踪：**不合并**，只把本段的段落标记标成删除（`spec/08`；接受后才真的合并）
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let mut plan = MutationPlan::new(part);
            plan.touch(para);
            t.para_mark(&mut plan, dom, para, LocalName::Del);
            return s.commit_plan(plan);
        }
        let _ = next;
        let plan = EditSession::plan_merge_with_next(s, at, para)?
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, "下一个块不是段落"))?;
        s.commit_plan(plan)
    }
    #[inline]
    /// 无追踪的"与下一段合并"计划（`SPAN-06` 合并行）。下一个块不是同容器的段落 → `None`。
    /// 7.4 接受段落标记的删除时复用它。
    fn plan_merge_with_next(
        &self,
        at: Option<PartId>,
        para: NodeId,
    ) -> Result<Option<MutationPlan>> {
        let s = self;
        let part = s.part_or_main(at);
        let dom = s.dom_in(at)?;
        let Some(next) = Dom::next_live_element_sibling(dom, para)
            .filter(|&n| dom.is(n, QName::w(LocalName::P)))
        else {
            return Ok(None);
        };
        let offset = crate::span::content_len(dom, para);
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        plan.touch(para);
        // `SPAN-06` 合并行：`next` 里的锚点整体搬到 `para`，下标加上原有内容项数
        plan.span.merges.push(crate::span::ContainerMerge { source: next, into: para, offset });
        // 内容项与范围标记按原顺序接到 `para` 末尾；`next` 的 `pPr` 随它一起消失
        // （Word 语义：合并后保留**前**段属性）
        for c in dom.semantic_children(next).collect::<Vec<_>>() {
            let Some(name) = dom.name(c) else { continue };
            if is_property_element(name) {
                continue;
            }
            plan.node_edits.push(NodeEdit::Move {
                node: c,
                parent: Target::Node(para),
                before: None,
            });
        }
        plan.node_edits.push(NodeEdit::Delete(next));
        // 格里的最后一段被合走后要补一个空 `w:p`（表格通则）
        if let Some(parent) = dom.parent(next) {
            MutationPlan::keep_cell_paragraph(dom, parent, Some(next), &mut plan);
        }
        Ok(Some(plan))
    }
    #[inline]
    /// `EDIT-06`：书签 `w:id` 在 part 内取最大值 + 1。
    fn next_bookmark_id(&mut self) -> u32 {
        let s = self;
        let part = s.main_part();
        // 索引里的书签也要算：标记被某次编辑从 DOM 里摘掉、范围还留着等 `SPAN-09` 重新物化时，
        // 只看 DOM 就会把那个号再发一次，保存时撞成 `SPAN_DUP_START`（`TEST-07` 抓到的）
        let mut max = s
            .spans_of(part)
            .map(|idx| {
                idx.live()
                    .filter(|sp| sp.kind.bookmark_name().is_some())
                    .filter_map(|sp| sp.pair_id().trim().parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let dom = s.dom();
        for n in dom.descendants(dom.root()) {
            if dom.node(n).dirty == Dirty::Deleted {
                continue;
            }
            let is_marker = dom.is(n, QName::w(LocalName::BookmarkStart))
                || dom.is(n, QName::w(LocalName::BookmarkEnd));
            if is_marker
                && let Some(v) = dom.attr_value(n, QName::w(LocalName::Id))
                && let Ok(id) = v.trim().parse::<u32>()
            {
                max = max.max(id);
            }
        }
        max + 1
    }
    #[inline]
    fn add_bookmark(
        &mut self,
        name: &str,
        from: InlinePos,
        to: InlinePos,
    ) -> Result<MutationResult> {
        let s = self;
        if name.is_empty() {
            return Err(Error::edit(DiagCode::EditBadPosition, "书签名为空"));
        }
        if from.para != to.para {
            return Err(Error::edit(
                DiagCode::EditCrossParagraph,
                "AddBookmark 两端不在同一段落（M2 只支持同段）",
            ));
        }
        if from.offset > to.offset {
            return Err(Error::edit(DiagCode::EditBadPosition, "from 在 to 之后"));
        }
        let part = s.main_part();
        // 名字全文档唯一（`EDIT-03`）
        if s.spans_of(part)?.live().any(|sp| sp.kind.bookmark_name() == Some(name)) {
            return Err(Error::edit(DiagCode::EditBadPosition, format!("书签名 {name:?} 已存在")));
        }
        // 两端落到 inline 边界
        let mut result = MutationResult::default();
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let loc_to = to.offset.locate(tb)?;
        EditSession::split_at(s, to, loc_to, &mut result)?;
        let tb = EditSession::require_text_block(s, from.part, from.para)?;
        let loc_from = from.offset.locate(tb)?;
        EditSession::split_at(s, from, loc_from, &mut result)?;

        let id = EditSession::next_bookmark_id(s).to_string();
        let a = EditSession::content_boundary(s, from.para, from)?;
        let b = EditSession::content_boundary(s, to.para, to)?;
        let dom = s.dom();
        let start_before = MutationPlan::content_site(dom, from.para, a);
        let end_before = MutationPlan::content_site(dom, to.para, b);
        let mut plan = MutationPlan::new(part);
        plan.touch(from.para);
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(from.para),
            before: start_before,
            node: NewElement::new(QName::w(LocalName::BookmarkStart))
                .with_attr(QName::w(LocalName::Id), id.clone())
                .with_attr(QName::w(LocalName::Name), name),
        });
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(to.para),
            before: end_before,
            node: NewElement::new(QName::w(LocalName::BookmarkEnd))
                .with_attr(QName::w(LocalName::Id), id.clone()),
        });
        let r = s.commit_plan(plan)?;
        let (start_marker, end_marker) = (
            r.created[0].ok_or_else(|| unsupported("书签起点没创建"))?,
            r.created[1].ok_or_else(|| unsupported("书签终点没创建"))?,
        );
        result.absorb(r);
        let dom = s.dom();
        let anchor = |node: NodeId, aff: Affinity| {
            Anchor::at(
                from.para,
                crate::span::boundary_before(dom, from.para, node).unwrap_or(0),
                aff,
                node,
            )
        };
        let (mut start, mut end) =
            (anchor(start_marker, Affinity::Right), anchor(end_marker, Affinity::Left));
        // 空书签两端同向（`SPAN-02` 例外）
        if start.same_place(&end) {
            start.affinity = Affinity::Right;
            end.affinity = Affinity::Right;
        }
        s.push_span(
            part,
            RangeSpan {
                id: SpanId(0),
                part,
                flow: s.document().flows.flow_of(from.para).unwrap_or(FlowId(0)),
                kind: RangeKind::Bookmark {
                    id,
                    name: name.to_string(),
                    hidden: name.starts_with('_'),
                    cols: None,
                },
                origin: SpanOrigin::New,
                implicit: false,
                removed: false,
                start: Some(start),
                end: Some(end),
            },
        )?;
        Ok(result)
    }
    #[inline]
    fn remove_bookmark(&mut self, name: &str) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let mut victims: Vec<NodeId> = Vec::new();
        let mut spans: Vec<SpanId> = Vec::new();
        {
            let index = s.spans_of(part)?;
            for sp in index.live() {
                if sp.kind.bookmark_name() != Some(name) {
                    continue;
                }
                spans.push(sp.id);
                victims.extend(sp.start.and_then(|a| a.marker));
                victims.extend(sp.end.and_then(|a| a.marker));
            }
        }
        if spans.is_empty() {
            return Err(Error::edit(
                DiagCode::EditBadPosition,
                format!("没有名为 {name:?} 的书签"),
            ));
        }
        let dom = s.dom();
        victims.retain(|&n| dom.node(n).dirty != Dirty::Deleted);
        let mut plan = MutationPlan::new(part);
        for n in &victims {
            if let Some(p) = dom.ancestors(*n).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                plan.touch(p);
            }
            plan.node_edits.push(NodeEdit::Delete(*n));
        }
        let r = s.commit_plan(plan)?;
        for span in spans {
            s.drop_span(part, span);
        }
        Ok(r)
    }
    #[inline]
    fn field_of(&self, id: crate::span::FieldId) -> Result<&crate::span::FieldSpan> {
        let s = self;
        s.document()
            .fields
            .get(id)
            .ok_or_else(|| Error::edit(DiagCode::EditBadPosition, format!("没有字段 {}", id.0)))
    }
    #[inline]
    fn insert_field(
        &mut self,
        at: InlinePos,
        field: &NewField,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        EditSession::refuse_block_field_result(s, at.para)?;
        let part = s.main_part();
        let mut result = MutationResult::default();
        let tb = EditSession::require_text_block(s, at.part, at.para)?;
        let loc = at.offset.locate(tb)?;
        let (parent, before, inherit) = match EditSession::split_at(s, at, loc, &mut result)? {
            Some((left, right)) => {
                (s.dom().parent(left).expect("run has a parent"), Some(right), Some(left))
            }
            None => {
                let Loc::Boundary { index } = loc else {
                    unreachable!("split_at handles the rest")
                };
                EditSession::boundary_site(
                    s,
                    at.part,
                    EditSession::require_text_block(s, at.part, at.para)?,
                    index,
                )?
            }
        };
        let flavor = s.flavor();
        let dom = s.dom();
        // 继承格式：优先左侧 run 的 `rPr` 字节克隆，其次 `default_run_props`
        let rpr = match inherit.and_then(|r| MutationPlan::rpr_of(dom, r)) {
            Some(node) => NewElement::from_dom(dom, node, &mut crate::xml::Interner::new()),
            None => ctx.default_run_props.as_ref().map(|d| emit_run_props(d, flavor)),
        };
        let mut plan = MutationPlan::new(part);
        plan.touch(at.para);
        let inline = NewInline::Field {
            instr: field.instr.clone(),
            result: field.result.clone(),
            separate: true,
            dirty: field.mark_dirty,
            props: rpr,
        };
        // 追踪：整套结构 run 一起进 `w:ins`（`spec/08`）
        let (fparent, fbefore) = match &mut Tracker::new(s.document(), ctx) {
            None => (Target::Node(parent), before),
            Some(t) => MutationPlan::plan_ins_site(&mut plan, dom, t, at.para, parent, before)?,
        };
        for node in MutationPlan::emit_inlines(dom, std::slice::from_ref(&inline)) {
            plan.node_edits.push(NodeEdit::Insert { parent: fparent, before: fbefore, node });
        }
        result.absorb(s.commit_plan(plan)?);
        // 字段索引是投影：提交后已经重建，新字段按 `FLD-06` 归策略
        Ok(result)
    }
    #[inline]
    /// `FLD-07 Link`：改链接目标。
    fn set_link_target(
        &mut self,
        link: LinkRef,
        target: &LinkDest,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        match link {
            LinkRef::Field(id) => EditSession::set_field_link_target(s, id, target, ctx),
            // `w:hyperlink` 的 `r:id` / `w:anchor` 是元素属性，Word 不把它记成修订
            LinkRef::Element(node) => EditSession::set_hyperlink_target(s, node, target),
        }
    }
    #[inline]
    /// `w:hyperlink` 元素：外部 URL 先按 `EDIT-06` 分配关系，再改 `r:id`；书签改 `w:anchor`。
    /// 两个属性互斥（Word 只认一个），所以设一个就删另一个。
    fn set_hyperlink_target(&mut self, node: NodeId, target: &LinkDest) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        if !s.dom().is(node, QName::w(LocalName::Hyperlink)) {
            return Err(Error::edit(DiagCode::EditBadPosition, "节点不是 w:hyperlink"));
        }
        let rid_q = QName::new(NsId::R, LocalName::Id);
        let anchor_q = QName::w(LocalName::Anchor);
        let (set, remove, value) = match target {
            LinkDest::Url(url) => {
                let rid = s.add_external_relationship(part, RelType::Hyperlink, url)?;
                (rid_q, anchor_q, rid)
            }
            LinkDest::Rel(rid) => (rid_q, anchor_q, rid.clone()),
            LinkDest::Anchor(name) => (anchor_q, rid_q, name.clone()),
        };
        let dom = s.dom();
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(node).find(|&a| dom.is(a, QName::w(LocalName::P))) {
            plan.touch(p);
        }
        plan.node_edits.push(NodeEdit::SetAttr { node: Target::Node(node), name: set, value });
        if dom.attr(node, remove).is_some() {
            plan.node_edits.push(NodeEdit::RemoveAttr { node: Target::Node(node), name: remove });
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// HYPERLINK 字段：只重写 `instrText` 的文本，第一个参数之后的开关原文保留。
    fn set_field_link_target(
        &mut self,
        id: crate::span::FieldId,
        target: &LinkDest,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let f = EditSession::field_of(s, id)?;
        if *f.keyword() != crate::span::field::Keyword::Hyperlink {
            return Err(unsupported("SetLinkTarget 只用于 HYPERLINK 字段"));
        }
        let crate::span::FieldForm::Complex { instr_nodes, .. } = &f.form else {
            return Err(unsupported("SetLinkTarget 暂不支持 w:fldSimple（改 @w:instr 属性）"));
        };
        if instr_nodes.is_empty() {
            return Err(unsupported("字段没有指令 run"));
        }
        let instr_nodes = instr_nodes.clone();
        let raw = f.instr.raw.clone();
        let dom = s.dom();
        // 保留除第一个参数之外的全部原文（开关 `\o "tip"` 等）
        let rest = MutationPlan::remaining_after_first_argument(&raw);
        let head = match target {
            LinkDest::Url(url) => format!("HYPERLINK \"{url}\""),
            // 文内链接：Word 写 `HYPERLINK \l "bookmark"`
            LinkDest::Anchor(name) => format!("HYPERLINK \\l \"{name}\""),
            LinkDest::Rel(_) => {
                return Err(unsupported("字段形式的链接没有关系 id（用 LinkDest::Url）"));
            }
        };
        let rest = if matches!(target, LinkDest::Anchor(_)) {
            // `\l` 自己就是开关，去掉原来的 `\l`
            rest.split_whitespace().collect::<Vec<_>>().join(" ").replace("\\l ", "")
        } else {
            rest
        };
        let text = if rest.is_empty() { format!(" {head} ") } else { format!(" {head} {rest} ") };
        let mut plan = MutationPlan::new(part);
        if let Some(p) = dom.ancestors(instr_nodes[0]).find(|&a| dom.is(a, QName::w(LocalName::P)))
        {
            plan.touch(p);
        }
        // 追踪：旧指令 run 进 `w:del` 并改名 `w:delInstrText`，新指令 run 进 `w:ins`（Word 形态）
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let has_instr = instr_nodes.iter().any(|&r| {
                dom.semantic_children(r).any(|c| dom.is(c, QName::w(LocalName::InstrText)))
            });
            if !has_instr {
                return Err(unsupported("字段没有 w:instrText 可改"));
            }
            let last = *instr_nodes.last().expect("checked above");
            let parent = dom.parent(last).ok_or_else(|| unsupported("指令 run 没有父节点"))?;
            let k = plan.node_edits.len();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(parent),
                before: Dom::next_live_sibling(dom, last),
                node: t.marker(LocalName::Ins),
            });
            let mut run = NewElement::new(QName::w(LocalName::R));
            if let Some(rpr) = MutationPlan::rpr_of(dom, instr_nodes[0])
                && let Some(e) = NewElement::from_dom(dom, rpr, &mut crate::xml::Interner::new())
            {
                run.push_child(e);
            }
            run.push_child(
                NewElement::new(QName::w(LocalName::InstrText))
                    .with_attr(QName::new(NsId::Xml, LocalName::Space), "preserve")
                    .with_text(text),
            );
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::New(k),
                before: None,
                node: run,
            });
            for &r in &instr_nodes {
                t.wrap_item(&mut plan, dom, r, LocalName::Del);
                Tracker::rename_to_deleted(&mut plan, dom, r);
            }
            return s.commit_plan(plan);
        }
        // 指令拆在多个 `w:instrText` 里时（`FLD-03`）：第一个写全量，其余清空
        let mut first = true;
        for run in &instr_nodes {
            for seg in dom.semantic_children(*run).collect::<Vec<_>>() {
                if !dom.is(seg, QName::w(LocalName::InstrText)) {
                    continue;
                }
                let value = if first { text.clone() } else { String::new() };
                first = false;
                MutationPlan::set_segment_text(dom, seg, &value, &mut plan);
            }
        }
        if first {
            return Err(unsupported("字段没有 w:instrText 可改"));
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `FLD-10`：`w:ffData` 里 `w:checkBox` 的 `w:checked` 取反（不存在则按顺序插在 `w:default` 之后）。
    fn toggle_checkbox(&mut self, id: crate::span::FieldId) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let f = EditSession::field_of(s, id)?;
        let dom = s.dom();
        let data = crate::span::field::read_form_data(dom, f.ff_data)
            .ok_or_else(|| unsupported("字段没有 w:ffData 表单定义（FLD-10）"))?;
        let crate::span::FormData::CheckBox { node, checked, .. } = data else {
            return Err(unsupported("这个字段不是复选框"));
        };
        let want = !checked;
        let existing =
            dom.semantic_children(node).find(|&c| dom.is(c, QName::w(LocalName::Checked)));
        let mut plan = MutationPlan::new(part);
        MutationPlan::touch_field_paragraphs(dom, &mut plan, f);
        match existing {
            Some(c) => plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(c),
                name: QName::w(LocalName::Val),
                value: if want { "1".into() } else { "0".into() },
            }),
            None => {
                // 顺序：`w:size`|`w:sizeAuto`, `w:default`, `w:checked`（`FLD-10`）→ 追加在末尾即可
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(node),
                    before: None,
                    node: NewElement::new(QName::w(LocalName::Checked))
                        .with_attr(QName::w(LocalName::Val), if want { "1" } else { "0" }),
                });
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `FLD-10`：FORMTEXT 的结果文字。结果 run 只留一个，文本为 `text`（格式沿用第一个结果 run）。
    fn set_form_text(
        &mut self,
        id: crate::span::FieldId,
        text: &str,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let f = EditSession::field_of(s, id)?;
        let results: Vec<NodeId> = f.form.result_nodes().to_vec();
        let tracked = ctx.track_changes.is_some();
        let dom = s.dom();
        // 只有一个结果 run 且它有唯一 `w:t` → 直接改文本（最小脏化）。追踪时不走这条：
        // 结果 run 要按 `DeleteRange` + `InsertText` 的规则留下痕迹
        if !tracked
            && results.len() == 1
            && let Some(t) =
                dom.semantic_children(results[0]).find(|&c| dom.is(c, QName::w(LocalName::T)))
        {
            let mut plan = MutationPlan::new(part);
            MutationPlan::touch_field_paragraphs(dom, &mut plan, f);
            MutationPlan::set_segment_text(dom, t, text, &mut plan);
            return s.commit_plan(plan);
        }
        let rpr = results
            .first()
            .and_then(|&r| MutationPlan::rpr_of(dom, r))
            .and_then(|n| NewElement::from_dom(dom, n, &mut crate::xml::Interner::new()));
        let (parent, before) = match results.first() {
            Some(&first) => (dom.parent(first).expect("run has a parent"), Some(first)),
            None => {
                // 没有结果区：插在 end run 之前
                let end = f.form.tail();
                (dom.parent(end).expect("run has a parent"), Some(end))
            }
        };
        let mut plan = MutationPlan::new(part);
        MutationPlan::touch_field_paragraphs(dom, &mut plan, f);
        let new_run = {
            let mut r = NewElement::new(QName::w(LocalName::R));
            if let Some(p) = &rpr {
                r.push_child(p.clone());
            }
            for seg in Emitter::text_segments(text, false) {
                r.push_child(seg);
            }
            r
        };
        match Tracker::new(s.document(), ctx) {
            None => {
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(parent),
                    before,
                    node: new_run,
                });
                for old in &results {
                    plan.node_edits.push(NodeEdit::Delete(*old));
                }
            }
            Some(mut t) => {
                let k = plan.node_edits.len();
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(parent),
                    before,
                    node: t.marker(LocalName::Ins),
                });
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::New(k),
                    before: None,
                    node: new_run,
                });
                for &old in &results {
                    t.wrap_item(&mut plan, dom, old, LocalName::Del);
                    Tracker::rename_to_deleted(&mut plan, dom, old);
                }
            }
        }
        s.commit_plan(plan)
    }
    #[inline]
    /// `FLD-07`：字段结果 run 的格式（原子字段不能按 `SetRunProps` 那样定位，所以单列一个操作）。
    fn set_field_result_props(
        &mut self,
        id: crate::span::FieldId,
        patch: &RunPropsPatch,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let part = s.main_part();
        let f = EditSession::field_of(s, id)?;
        let results: Vec<NodeId> = f.form.result_nodes().to_vec();
        let head = f.form.head();
        if results.is_empty() {
            return Err(unsupported("字段没有结果区可改格式"));
        }
        let mut result = MutationResult::default();
        // 追踪：与 `SetRunProps` 同规则，先快照 `w:rPrChange`（两个阶段，理由同上）
        if let Some(mut t) = Tracker::new(s.document(), ctx) {
            let dom = s.dom();
            let mut plan = MutationPlan::new(part);
            if let Some(p) = dom.ancestors(head).find(|&a| dom.is(a, QName::w(LocalName::P))) {
                plan.touch(p);
            }
            for r in &results {
                if !dom.is(*r, QName::w(LocalName::R)) {
                    continue;
                }
                MutationPlan::snapshot_run_props(&mut plan, dom, &mut t, *r);
            }
            if !plan.is_empty() {
                result.absorb(s.commit_plan(plan)?);
            }
        }
        let dom = s.dom();
        let f = EditSession::field_of(s, id)?;
        let mut plan = MutationPlan::new(part);
        MutationPlan::touch_field_paragraphs(dom, &mut plan, f);
        let flavor = s.flavor();
        for r in &results {
            if !dom.is(*r, QName::w(LocalName::R)) {
                continue;
            }
            let rpr = MutationPlan::rpr_of(dom, *r);
            plan.node_edits.extend(plan_apply_run_props(dom, *r, rpr, patch, flavor));
        }
        result.absorb(s.commit_plan(plan)?);
        Ok(result)
    }
    #[inline]
    /// `FLD-09`：块字段更新——用给定的块替换 `separate..end` 之间的全部节点。
    ///
    /// 生成器（TOC 重算等）在 M7；这里是机制：调用方给内容，`w:fldLock` 的字段拒绝（`FLD_LOCKED`）。
    /// 结构 run（begin / 指令 / separate / end）与外层容器都保留。跨段字段的结果区里，
    /// 中间的整段直接删，begin / end 所在段落里只删属于结果的 run。
    fn update_block_field(
        &mut self,
        id: crate::span::FieldId,
        blocks: Vec<NewBlock>,
        ctx: &EditContext,
    ) -> Result<MutationResult> {
        let s = self;
        let blocks = EditSession::materialize_all(s, blocks)?;
        let part = s.main_part();
        let f = EditSession::field_of(s, id)?;
        if f.lock {
            return Err(Error::edit(DiagCode::FldLocked, "字段带 w:fldLock，拒绝更新"));
        }
        let crate::span::FieldForm::Complex { separate, end, result_nodes, begin, .. } = &f.form
        else {
            return Err(unsupported("w:fldSimple 没有 separate..end 区间"));
        };
        if separate.is_none() {
            return Err(unsupported("字段没有 separate，无法定位结果区"));
        }
        let (end, begin) = (*end, *begin);
        let old: Vec<NodeId> = result_nodes.clone();
        let dom = s.dom();
        let para_of = |n: NodeId| {
            std::iter::once(n).chain(dom.ancestors(n)).find(|&a| dom.is(a, QName::w(LocalName::P)))
        };
        let end_para = para_of(end).ok_or_else(|| unsupported("字段 end 不在段落里"))?;
        let begin_para = para_of(begin).ok_or_else(|| unsupported("字段 begin 不在段落里"))?;
        let cross = end_para != begin_para;
        let mut tracker = Tracker::new(s.document(), ctx);
        let mut plan = MutationPlan::new(part);
        plan.structure_changed = true;
        plan.touch(begin_para);
        plan.touch(end_para);
        if cross {
            // 段落级：新块插在 end 所在段落之前
            let parent = dom.parent(end_para).ok_or_else(|| unsupported("段落没有父节点"))?;
            for b in blocks {
                let opaque = matches!(b, NewBlock::Xml(_) | NewBlock::Wrapped { .. });
                let node = MutationPlan::new_block_element(dom, b);
                // 追踪：新结果块按 `InsertBlock` 规则、旧结果块按 `DeleteBlock` 规则（`spec/18` 7.3）
                let node = match &mut tracker {
                    Some(t) => t.mark_new_block_inserted(node, opaque),
                    None => node,
                };
                plan.node_edits.push(NodeEdit::Insert {
                    parent: Target::Node(parent),
                    before: Some(end_para),
                    node,
                });
            }
        } else {
            // 同段：新块的 inline 直接插在 end run 之前（段落里不能塞段落）
            let parent = dom.parent(end).ok_or_else(|| unsupported("字段 end 没有父节点"))?;
            for b in blocks {
                // 段落的 inline 直接内联；生成器给的是整段 `w:p`，取它 `pPr` 之外的子元素
                let nodes: Vec<NewElement> = match b {
                    NewBlock::Paragraph { inlines, .. } => {
                        MutationPlan::emit_inlines(dom, &inlines)
                    }
                    NewBlock::Xml(e) if e.name == QName::w(LocalName::P) => e
                        .children
                        .into_iter()
                        .filter_map(|c| match c {
                            crate::xml::NewNode::Element(e)
                                if e.name != QName::w(LocalName::PPr) =>
                            {
                                Some(e)
                            }
                            _ => None,
                        })
                        .collect(),
                    _ => {
                        return Err(unsupported(
                            "同段块字段的新内容只能是段落（它的 inline 会内联进去）",
                        ));
                    }
                };
                let (iparent, ibefore) = match &mut tracker {
                    None => (Target::Node(parent), Some(end)),
                    Some(t) => {
                        let k = plan.node_edits.len();
                        plan.node_edits.push(NodeEdit::Insert {
                            parent: Target::Node(parent),
                            before: Some(end),
                            node: t.marker(LocalName::Ins),
                        });
                        (Target::New(k), None)
                    }
                };
                for node in nodes {
                    plan.node_edits.push(NodeEdit::Insert {
                        parent: iparent,
                        before: ibefore,
                        node,
                    });
                }
            }
        }
        // 旧结果：中间整段删掉，begin / end 所在段落里只删结果 run
        let mut victims: Vec<NodeId> = Vec::new();
        for n in old {
            let owner = para_of(n);
            let victim = match owner {
                Some(p) if p == end_para || p == begin_para => n,
                Some(p) => p,
                None => n,
            };
            if victim == end || victim == begin || victim == end_para || victim == begin_para {
                continue;
            }
            if !victims.contains(&victim) {
                victims.push(victim);
            }
        }
        // 结果 run 常裹在 `w:hyperlink`（目录条目）或 `w:ins` 里：整包都成废墟就连壳一起删。
        // 不然留下一个空 `w:hyperlink`，重算一次多一个空壳（生成的目录第一条与最后一条就在
        // begin / end 所在的段落里）。
        loop {
            let grown: Vec<NodeId> = victims
                .iter()
                .filter_map(|&v| dom.parent(v))
                .filter(|&p| p != begin_para && p != end_para && !dom.is(p, QName::w(LocalName::P)))
                .filter(|&p| !victims.contains(&p))
                .filter(|&p| Dom::live_children(dom, p).all(|c| victims.contains(&c)))
                .collect();
            if grown.is_empty() {
                break;
            }
            for p in grown {
                if !victims.contains(&p) {
                    victims.push(p);
                }
            }
        }
        for v in victims {
            match &mut tracker {
                Some(t)
                    if dom.is(v, QName::w(LocalName::P)) || dom.is(v, QName::w(LocalName::Tbl)) =>
                {
                    MutationPlan::plan_delete_block_tracked(&mut plan, dom, t, v);
                }
                Some(t) => {
                    t.wrap_item(&mut plan, dom, v, LocalName::Del);
                    Tracker::rename_to_deleted(&mut plan, dom, v);
                }
                None => plan.node_edits.push(NodeEdit::Delete(v)),
            }
        }
        if ctx.mark_updated_fields_dirty
            && let Some(fld) =
                dom.semantic_children(begin).find(|&c| dom.is(c, QName::w(LocalName::FldChar)))
        {
            // begin run 的 `w:fldChar` 上打 `w:dirty="true"`，Word 打开时重算
            plan.node_edits.push(NodeEdit::SetAttr {
                node: Target::Node(fld),
                name: QName::w(LocalName::Dirty),
                value: "true".into(),
            });
        }
        s.commit_plan(plan)
    }
    #[inline]
    fn declarations(
        &mut self,
        opts: crate::save::options::CompatSaveOptions,
    ) -> Result<MutationResult> {
        let s = self;
        crate::save::options::decl::ensure_parts(s, &opts)?;
        let (plans, diagnostics) =
            crate::save::options::plan_all(s.package_mut(), &opts, false, false)?;
        for plan in &plans {
            plan.validate(s.package().part(plan.part).dom().ok_or_else(|| {
                Error::edit(DiagCode::EditTargetOpaque, "declaration part has no DOM")
            })?)?;
        }
        let mut result = MutationResult::default();
        for plan in plans {
            result.absorb(s.commit_plan(plan)?);
        }
        result.diagnostics.extend(diagnostics.iter().cloned());
        s.record(diagnostics);
        s.rebuild()?;
        Ok(result)
    }
}

impl MutationPlan {
    #[inline]
    /// 指令原文里第一个参数之后的部分（开关等），已 trim。
    fn remaining_after_first_argument(raw: &str) -> String {
        let t = raw.trim();
        let after_keyword = t.split_once(char::is_whitespace).map(|(_, r)| r.trim()).unwrap_or("");
        if after_keyword.is_empty() {
            return String::new();
        }
        let rest = if let Some(stripped) = after_keyword.strip_prefix('"') {
            stripped.split_once('"').map(|(_, r)| r).unwrap_or("")
        } else {
            after_keyword.split_once(char::is_whitespace).map(|(_, r)| r).unwrap_or("")
        };
        rest.trim().to_string()
    }
    #[inline]
    /// 字段涉及的每个段落都要刷新投影（`MOD-13`）：跨段字段的结果区横跨好几段，只刷 begin
    /// 那一段的话，别的段落的投影就停在编辑之前（`TEST-07` 一步就抓到：`SetFormText` 改完，
    /// 另一段的 `FieldBlockResult` 预览还是旧的）。
    fn touch_field_paragraphs(dom: &Dom, plan: &mut MutationPlan, f: &crate::span::FieldSpan) {
        let para_of = |n: NodeId| {
            std::iter::once(n).chain(dom.ancestors(n)).find(|&a| dom.is(a, QName::w(LocalName::P)))
        };
        let nodes = f
            .form
            .structure_nodes()
            .into_iter()
            .chain(f.form.result_nodes().iter().copied())
            .chain(std::iter::once(f.form.head()))
            .chain(std::iter::once(f.form.tail()));
        for n in nodes {
            if let Some(p) = para_of(n) {
                plan.touch(p);
            }
        }
    }
}

#[cfg(test)]
mod test_edit {
    #[test]
    fn enum_traits_preserve_wire_spelling_and_unknown_value_errors() {
        for (value, text) in [
            (super::ImageWrap::SquareLeft, "square-left"),
            (super::ImageWrap::SquareRight, "square-right"),
            (super::ImageWrap::TightLeft, "tight-left"),
            (super::ImageWrap::TightRight, "tight-right"),
            (super::ImageWrap::ThroughLeft, "through-left"),
            (super::ImageWrap::ThroughRight, "through-right"),
            (super::ImageWrap::TopBottom, "topBottom"),
            (super::ImageWrap::Front, "front"),
            (super::ImageWrap::Behind, "behind"),
        ] {
            assert_eq!(<&str>::from(value), text);
            assert_eq!(value.to_string(), text);
            assert_eq!(text.parse::<super::ImageWrap>().unwrap(), value);
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(json, text);
            assert_eq!(serde_json::from_value::<super::ImageWrap>(json).unwrap(), value);
        }
        assert!("top-bottom".parse::<super::ImageWrap>().is_err());
        assert!("unknown".parse::<super::NewChartKind>().is_err());
        assert!("unknown".parse::<super::LineKind>().is_err());
        assert_eq!(
            serde_json::from_value::<super::ImageWrap>(serde_json::json!("unknown"))
                .unwrap_err()
                .to_string(),
            "unknown enum value",
        );
    }

    #[test]
    fn dom_queries_preserve_raw_order_namespace_and_deleted_filtering() {
        let mut dom = super::Dom::parse(super::PartId(0),
            br#"<r xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:t>A</w:t> <w:t>B</w:t><w:p/></r>"#,
        ).unwrap();
        let root = dom.root();
        let first = dom.children(root)[0];
        let space = dom.children(root)[1];
        let second = dom.children(root)[2];
        let last = dom.children(root)[3];
        let name = super::QName::w(super::LocalName::T);
        assert!(dom.live_children_named(root, name).eq([first, second]));
        assert!(dom.live_element_children(root).eq([first, second, last]));
        assert_eq!(dom.direct_child_containing(root, dom.children(first)[0]), Some(first));
        assert_eq!(dom.direct_child_containing(root, first), Some(first));
        assert_eq!(dom.direct_child_containing(first, last), None);
        assert_eq!(dom.direct_child_containing(root, root), None);
        assert_eq!(dom.next_live_sibling(first), Some(space));
        assert_eq!(dom.next_live_element_sibling(first), Some(second));
        assert_eq!(dom.sole_live_text_child(first), Some(dom.children(first)[0]));
        assert_eq!(dom.sole_live_text_child(root), None);
        assert_eq!(dom.sole_live_text_child(last), None);
        dom.delete(second);
        assert!(dom.live_element_children(root).eq([first, last]));
        assert!(dom.live_children(root).eq([first, space, last]));
        assert!(dom.live_children_named(root, name).eq([first]));
        assert_eq!(dom.next_live_element_sibling(first), Some(last));
        assert_eq!(
            dom.live_children_named(
                root,
                super::QName::new(super::NsId::None, super::LocalName::T)
            )
            .count(),
            0
        );
    }

    #[test]
    fn diff_preserves_common_runs_and_falls_back_at_the_existing_limit() {
        let first = String::from("a");
        let last = String::from("b");
        let inserted = String::from("c");
        let old = [super::Tok::Run(&first, None), super::Tok::Run(&last, None)];
        let new = [
            super::Tok::Run(&first, None),
            super::Tok::Run(&inserted, None),
            super::Tok::Run(&last, None),
        ];
        assert_eq!(
            super::InlineDiff { old: &old, new: &new }.steps().unwrap(),
            [super::Step::Equal(1), super::Step::Insert(1), super::Step::Equal(1)],
        );
        assert!(super::InlineDiff { old: &[], new: &[] }.steps().unwrap().is_empty());
        let limit = vec![super::Tok::Run(&first, None); 4_000];
        assert_eq!(
            super::InlineDiff { old: &limit, new: &limit }.steps().unwrap(),
            [super::Step::Equal(4_000)],
        );
        let oversized = vec![super::Tok::Run(&first, None); 4_001];
        assert!(super::InlineDiff { old: &oversized, new: &[] }.steps().is_none());
        assert!(super::InlineDiff { old: &[], new: &oversized }.steps().is_none());
    }
    #[test]
    fn targets_preserve_parts_duplicates_order_and_snapshot_ownership() {
        let session = super::EditSession::blank(None).unwrap();
        let header = super::InlinePos::in_part(super::PartId(7), super::NodeId(11), 0);
        let op = super::EditOp::DeleteRange { from: header, to: header.with_offset(1) };
        let targets = session.op_targets(&op);
        assert!(targets.clone().eq([
            (Some(super::PartId(7)), super::NodeId(11)),
            (Some(super::PartId(7)), super::NodeId(11)),
        ]));
        // 迭代器拥有轻量 ID，跨提交保存的目标不借用会话或操作，也不会在消费时重新查询。
        drop(op);
        drop(session);
        assert_eq!(targets.count(), 2);
        let session = super::EditSession::blank(None).unwrap();
        let op = super::EditOp::MoveBlock {
            from: Some(super::PartId(7)),
            node: super::NodeId(11),
            to: super::BlockPos::end(super::NodeId(13)),
        };
        assert!(
            session
                .op_targets(&op)
                .eq([(Some(super::PartId(7)), super::NodeId(11)), (None, super::NodeId(13)),])
        );
        assert_eq!(session.op_targets(&super::EditOp::RemoveInks).count(), 0);
    }
    // 原 locate.rs 未被加载；保留其独有的逐个 UTF-16 边界断言，并校验当前诊断契约。
    #[test]
    fn position_utf16_text_conversion_preserves_boundaries_and_errors() {
        let text = String::from("A😀B");
        for (units, bytes) in [(0, 0), (1, 1), (3, 5), (4, 6)] {
            assert_eq!(
                usize::try_from(super::Utf16TextOffset {
                    text: &text,
                    offset: super::Utf16Offset(units),
                })
                .unwrap(),
                bytes,
            );
        }
        for (units, expected_code, expected_message) in [
            (2, super::DiagCode::EditSplitSurrogate, "偏移落在代理对中间"),
            (5, super::DiagCode::EditBadPosition, "偏移超出段文本"),
        ] {
            let error = usize::try_from(super::Utf16TextOffset {
                text: &text,
                offset: super::Utf16Offset(units),
            })
            .unwrap_err();
            let _: &dyn std::error::Error = &error;
            assert!(matches!(error, super::Error::Edit { code, message }
                if code == expected_code && message == expected_message));
        }
        assert_eq!(
            usize::try_from(super::Utf16TextOffset { text: "", offset: super::Utf16Offset(0) })
                .unwrap(),
            0,
        );
    }
    #[test]
    fn position_json_protocol_is_unchanged() {
        let position = super::InlinePos::in_part(super::PartId(7), super::NodeId(11), 3);
        let value = serde_json::to_value(position).unwrap();
        assert_eq!(value, serde_json::json!({"part": 7, "para": 11, "offset": 3}));
        assert_eq!(serde_json::from_value::<super::InlinePos>(value).unwrap(), position);
        let main = super::InlinePos::new(super::NodeId(11), 0);
        assert_eq!(
            serde_json::to_value(main).unwrap(),
            serde_json::json!({"part": null, "para": 11, "offset": 0}),
        );
        assert_eq!(serde_json::to_value(super::Utf16Offset(3)).unwrap(), 3);
    }
    #[test]
    fn edit_03_text_segments_fold_control_chars() {
        let segs = super::Emitter::text_segments("a\tb\nc\u{0C}d\u{0B}e", false);
        let names: Vec<super::LocalName> = segs.iter().map(|e| e.name.local).collect();
        assert_eq!(
            names,
            [
                super::LocalName::T,
                super::LocalName::Tab,
                super::LocalName::T,
                super::LocalName::Br,
                super::LocalName::T,
                super::LocalName::Br,
                super::LocalName::T,
                super::LocalName::Br,
                super::LocalName::T
            ]
        );
        assert_eq!(
            segs[5].attrs,
            vec![(super::QName::w(super::LocalName::Type), "page".to_string())]
        );
        assert_eq!(
            segs[7].attrs,
            vec![(super::QName::w(super::LocalName::Type), "column".to_string())]
        );
        assert_eq!(segs[0].children, vec![super::NewNode::Text("a".into())]);
        let del = super::Emitter::text_segments("x", true);
        assert_eq!(del[0].name.local, super::LocalName::DelText);
        let mut diags = Vec::new();
        assert_eq!(
            super::Emitter::sanitize_text("a\u{0}b\u{1F}c", super::PartId(0), &mut diags),
            "abc"
        );
        assert_eq!(diags.len(), 1);
    }
    impl PlanTestsFixture {
        #[inline]
        fn dom(xml: &str) -> super::Dom {
            super::Dom::parse(super::PartId(0), xml.as_bytes()).unwrap()
        }
    }
    impl PlanTestsFixture {
        #[inline]
        fn diag(message: &str) -> super::Diagnostic {
            super::Diagnostic::invariant_violation(
                super::PartId(0),
                None,
                super::DiagCode::EditPlanInvalid,
                message,
            )
        }
    }
    impl PlanTestsFixture {
        #[inline]
        fn assert_invalid(plan: &super::MutationPlan, dom: &super::Dom) {
            let err = plan.validate(dom).unwrap_err();
            assert!(matches!(
                err,
                super::Error::Edit { code: super::DiagCode::EditPlanInvalid, .. }
            ));
        }
    }
    impl PlanTestsFixture {
        #[inline]
        fn plan(part: super::PartId, node_edits: Vec<super::NodeEdit>) -> super::MutationPlan {
            let mut plan = super::MutationPlan::new(part);
            plan.node_edits = node_edits;
            plan
        }
    }
    #[test]
    fn edit_05_result_absorb_and_plan_helpers_cover_all_fields() {
        let a = super::NodeId(1);
        let b = super::NodeId(2);
        let mut first = super::MutationResult {
            created: vec![Some(a)],
            affected_blocks: vec![a],
            structure_changed: false,
            diagnostics: vec![PlanTestsFixture::diag("first")],
            offset_delta: vec![(a, super::Utf16Offset(1), 1)],
        };
        let later = super::MutationResult {
            created: vec![Some(b)],
            affected_blocks: vec![a, b],
            structure_changed: true,
            diagnostics: vec![PlanTestsFixture::diag("later")],
            offset_delta: vec![(b, super::Utf16Offset(2), -1)],
        };
        first.absorb(later);
        assert_eq!(first.created, vec![Some(b)]);
        assert_eq!(first.affected_blocks, vec![a, b]);
        assert!(first.structure_changed);
        assert_eq!(
            first.diagnostics,
            vec![PlanTestsFixture::diag("first"), PlanTestsFixture::diag("later")]
        );
        assert_eq!(
            first.offset_delta,
            vec![(a, super::Utf16Offset(1), 1), (b, super::Utf16Offset(2), -1)]
        );

        let mut plan = super::MutationPlan::new(super::PartId(0));
        assert!(plan.is_empty());
        plan.touch(a);
        plan.touch(a);
        assert_eq!(plan.affected_blocks, vec![a]);
        plan.node_edits.push(super::NodeEdit::Delete(a));
        assert!(!plan.is_empty());
    }
    #[test]
    fn edit_05_validate_rejects_out_of_range_deleted_and_non_element_targets() {
        let dom = PlanTestsFixture::dom("<root><a/><b>text</b></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let text = dom.children(b)[0];

        let out_of_range = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::SetAttr {
                node: super::Target::Node(super::NodeId(dom.node_count() as u32)),
                name: super::QName::w(super::LocalName::T),
                value: "x".into(),
            }],
        );
        PlanTestsFixture::assert_invalid(&out_of_range, &dom);

        let mut deleted_dom = dom.clone();
        deleted_dom.delete(a);
        let deleted = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::SetAttr {
                node: super::Target::Node(a),
                name: super::QName::w(super::LocalName::T),
                value: "x".into(),
            }],
        );
        PlanTestsFixture::assert_invalid(&deleted, &deleted_dom);

        let text_target = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::SetAttr {
                node: super::Target::Node(text),
                name: super::QName::w(super::LocalName::T),
                value: "x".into(),
            }],
        );
        PlanTestsFixture::assert_invalid(&text_target, &dom);
    }
    #[test]
    fn edit_05_validate_rejects_bad_new_target_and_before() {
        let dom = PlanTestsFixture::dom("<root><a/><b/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let new_element = || super::NewElement::new(super::QName::w(super::LocalName::P));

        let self_reference = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Insert {
                parent: super::Target::New(0),
                before: None,
                node: new_element(),
            }],
        );
        PlanTestsFixture::assert_invalid(&self_reference, &dom);

        let foreign_before = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Insert {
                parent: super::Target::Node(a),
                before: Some(b),
                node: new_element(),
            }],
        );
        PlanTestsFixture::assert_invalid(&foreign_before, &dom);

        let new_parent_with_before = PlanTestsFixture::plan(
            super::PartId(0),
            vec![
                super::NodeEdit::Insert {
                    parent: super::Target::Node(root),
                    before: None,
                    node: new_element(),
                },
                super::NodeEdit::Insert {
                    parent: super::Target::New(0),
                    before: Some(a),
                    node: new_element(),
                },
            ],
        );
        PlanTestsFixture::assert_invalid(&new_parent_with_before, &dom);
    }
    #[test]
    fn edit_05_validate_rejects_invalid_replace_sources_and_text_targets() {
        let dom = PlanTestsFixture::dom("<root><a/><b/><c/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let b = dom.children(root)[1];
        let c = dom.children(root)[2];

        let root_replace = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Replace {
                old: root,
                node: super::NewElement::new(super::QName::w(super::LocalName::P)),
            }],
        );
        PlanTestsFixture::assert_invalid(&root_replace, &dom);

        let mut deleted_old_dom = dom.clone();
        deleted_old_dom.delete(a);
        let deleted_replace_old = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Replace {
                old: a,
                node: super::NewElement::new(super::QName::w(super::LocalName::P)),
            }],
        );
        PlanTestsFixture::assert_invalid(&deleted_replace_old, &deleted_old_dom);

        let mut deleted_source_dom = dom.clone();
        deleted_source_dom.delete(c);
        let deleted_replace_source = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::ReplaceClone { old: a, source: c }],
        );
        PlanTestsFixture::assert_invalid(&deleted_replace_source, &deleted_source_dom);

        let non_text_set_text = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::SetText { node: a, text: "x".into() }],
        );
        PlanTestsFixture::assert_invalid(&non_text_set_text, &dom);

        let _ = b;
    }
    #[test]
    fn edit_05_validate_rejects_detached_move_even_with_new_parent() {
        let dom = PlanTestsFixture::dom("<root><a/></root>");
        let root = dom.root();
        let a = dom.children(root)[0];
        let plan = PlanTestsFixture::plan(
            super::PartId(0),
            vec![
                super::NodeEdit::Insert {
                    parent: super::Target::Node(root),
                    before: None,
                    node: super::NewElement::new(super::QName::w(super::LocalName::P)),
                },
                super::NodeEdit::Move { node: root, parent: super::Target::New(0), before: None },
            ],
        );
        PlanTestsFixture::assert_invalid(&plan, &dom);
        let _ = a;
    }
    #[test]
    fn edit_05_validate_rejects_invalid_clone_and_move_nodes() {
        let dom = PlanTestsFixture::dom("<root><a/></root>");
        let root = dom.root();
        let missing = super::NodeId(dom.node_count() as u32);
        let invalid_clone = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::InsertClone {
                parent: super::Target::Node(root),
                before: None,
                source: missing,
            }],
        );
        PlanTestsFixture::assert_invalid(&invalid_clone, &dom);

        let invalid_move = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Move {
                node: missing,
                parent: super::Target::Node(root),
                before: None,
            }],
        );
        PlanTestsFixture::assert_invalid(&invalid_move, &dom);
    }
    #[test]
    fn edit_05_validate_accepts_valid_plan_and_commit_preserves_result() {
        let mut dom = PlanTestsFixture::dom("<root><a/></root>");
        let root = dom.root();
        let mut plan = PlanTestsFixture::plan(
            super::PartId(0),
            vec![super::NodeEdit::Insert {
                parent: super::Target::Node(root),
                before: None,
                node: super::NewElement::new(super::QName::w(super::LocalName::P)),
            }],
        );
        plan.touch(root);
        plan.structure_changed = true;
        plan.diagnostics.push(PlanTestsFixture::diag("commit"));
        plan.offset_delta.push((root, super::Utf16Offset(0), 1));
        plan.validate(&dom).unwrap();
        let result = plan.commit(&mut dom);
        assert_eq!(result.affected_blocks, vec![root]);
        assert!(result.structure_changed);
        assert_eq!(result.diagnostics, vec![PlanTestsFixture::diag("commit")]);
        assert_eq!(result.offset_delta, vec![(root, super::Utf16Offset(0), 1)]);
        assert_eq!(result.created.len(), 1);
        assert!(result.created[0].is_some());
    }
    #[test]
    fn edit_05_diagnostic_origin_is_preserved_in_plan() {
        let d = PlanTestsFixture::diag("origin");
        assert_eq!(d.origin, super::ValidationOrigin::EngineInvariantViolation);
        let mut plan = super::MutationPlan::new(super::PartId(0));
        plan.diagnostics.push(d.clone());
        assert_eq!(plan.diagnostics, vec![d]);
    }
    #[repr(C)]
    struct PlanTestsFixture;
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    /// 两个 XML part 的最小 docx（主 part + settings）。

    impl SessionTestsFixture {
        #[inline]
        fn docx() -> Vec<u8> {
            SessionTestsFixture::docx_with(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#)
        }
    }
    /// 同上，正文由调用方给。

    impl SessionTestsFixture {
        #[inline]
        fn docx_with(body: &str) -> Vec<u8> {
            let ct = concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
                r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
                r#"<Default Extension="xml" ContentType="application/xml"/>"#,
                r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
                r#"<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#
            );
            let rels = concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
            );
            let doc_rels = concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#
            );
            let doc = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#
            );
            let settings =
                format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"/>"#);
            let mut w = zip::ZipWriter::new(super::Cursor::new(Vec::new()));
            for (name, bytes) in [
                ("[Content_Types].xml", ct),
                ("_rels/.rels", rels),
                ("word/_rels/document.xml.rels", doc_rels),
                ("word/document.xml", doc.as_str()),
                ("word/settings.xml", settings.as_str()),
            ] {
                w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                super::Write::write_all(&mut w, bytes.as_bytes()).unwrap();
            }
            w.finish().unwrap().into_inner()
        }
    }
    /// `EDIT-05`：事务回滚覆盖它碰过的**每个** part，不只是主 part。

    #[test]
    fn edit_05_transaction_rolls_back_every_touched_part() {
        let bytes = SessionTestsFixture::docx();
        let mut s = super::EditSession::open(&bytes).unwrap();
        let main = s.main_part();
        let settings = s.package().find_name("word/settings.xml").unwrap();
        let err = s
            .transaction(|s| {
                // 阶段 1：写主 part（改字）
                let dom = s.package_mut().dom_mut(main).unwrap().unwrap();
                let t = dom
                    .descendants(dom.root())
                    .find(|&n| dom.is(n, crate::xml::QName::w(crate::xml::LocalName::T)))
                    .unwrap();
                let text = dom.children(t)[0];
                let mut plan = super::MutationPlan::new(main);
                plan.node_edits
                    .push(crate::xml::NodeEdit::SetText { node: text, text: "y".into() });
                s.commit_plan(plan)?;
                // 阶段 2：写 settings part
                let sdom = s.package().part(settings).dom().unwrap();
                let root = sdom.root();
                let patch = super::SettingsPatch {
                    remove_personal_information: super::Change::Set(true),
                    ..Default::default()
                };
                let mut plan = super::MutationPlan::new(settings);
                plan.node_edits =
                    super::plan_apply_settings(sdom, root, Some(root), &patch, sdom.flavor());
                s.commit_plan(plan)?;
                assert!(s.package().is_dirty(), "两个 part 都脏了");
                // 阶段 3：失败
                Err::<(), _>(super::Error::edit(super::DiagCode::EditUnsupported, "故意失败"))
            })
            .expect_err("事务应失败");
        assert!(matches!(err, super::Error::Edit { code: super::DiagCode::EditUnsupported, .. }));
        assert!(!s.package().is_dirty(), "两个 part 都回滚了");
        assert_eq!(s.save_with(&super::SaveOptions::default()).unwrap(), bytes, "保存回到原字节");
        assert_eq!(s.document().text_blocks().next().unwrap().text(), "x", "投影也回滚");
    }
    /// `SPAN-09` / `SAVE-02`：引擎自己弄丢一端的范围在调试构建下让保存失败，发布构建只记诊断。

    ///

    /// 索引没有对外的可变入口，破坏只能从 crate 内部注入——这条自检就是为了让"变换弄丢锚点"

    /// 这类缺陷在 CI 里当场暴露，而不是悄悄写出一份半开的范围。

    #[test]
    fn span_09_engine_broken_range_fails_the_save_in_debug_builds() {
        let bytes = SessionTestsFixture::docx_with(
            r#"<w:p><w:bookmarkStart w:id="1" w:name="a"/><w:r><w:t>x</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
        );
        let mut s = super::EditSession::open(&bytes).unwrap();
        let para = s.nth_text_block(0).expect("text block").node;
        // 一次正常编辑：建立索引并让主 part 变脏（否则保存直接返回原字节）
        s.apply(
            super::EditOp::InsertText {
                at: super::InlinePos::new(para, 0),
                text: "y".into(),
                props: None,
            },
            &super::EditContext::default(),
        )
        .expect("插入成功");
        let main = s.main_part();
        let index = s.spans_mut(main).expect("索引已建立");
        let span = index.live().next().expect("书签范围").id;
        index.get_mut(span).expect("范围还在").end = None; // 注入破坏：终点不见了
        let saved = s.save();
        if cfg!(debug_assertions) {
            match saved {
                Err(super::Error::Invariant(d)) => {
                    assert_eq!(d.code, super::DiagCode::SpanUnclosed);
                    assert_eq!(d.origin, crate::diag::ValidationOrigin::EngineInvariantViolation);
                }
                other => panic!("调试构建下应 Err(SAVE_INVARIANT)：{other:?}"),
            }
        } else {
            assert!(saved.is_ok(), "发布构建只记诊断");
        }
    }
    #[repr(C)]
    struct SessionTestsFixture;
    #[test]
    fn pkg_04_relative_targets() {
        assert_eq!(super::relative_target("word", "word/media/image1.png"), "media/image1.png");
        assert_eq!(super::relative_target("word", "customXml/item1.xml"), "../customXml/item1.xml");
        assert_eq!(super::relative_target("word", "docProps/core.xml"), "../docProps/core.xml");
        assert_eq!(super::relative_target("", "word/document.xml"), "word/document.xml");
        assert_eq!(
            super::relative_target("word/charts", "word/charts/embeddings/wb.xlsx"),
            "embeddings/wb.xlsx"
        );
        assert_eq!(super::relative_target("word/charts", "word/media/i.png"), "../media/i.png");
    }
}
