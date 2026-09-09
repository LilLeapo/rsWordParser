//! 辅助 part 的内容流（`MOD-01`、`docs/03` §6.7，`spec/16` 任务 5.3）。
//!
//! 页眉页脚、脚注尾注、批注的内容都是 `Vec<Block>`，**与正文同一个构建器**。它们与正文的差别只有
//! 两点：块长在别的 part 的 DOM 里，以及每个 part 有自己的三份索引。这里就是那两点的共同部分。
//!
//! `FlowMap` / `FieldIndex` / `SpanIndex` 按 **part** 建一次（`SPAN-01`：`w:hdr` / `w:ftr` /
//! 每个 `w:footnote` / `w:endnote` / `w:comment` 条目各是一个独立内容流，`FlowId` 只在 part 内有
//! 意义），块按**容器**建（注释 part 里一个条目一个容器，页眉 part 整个根就是一个容器）。

use crate::diag::Diagnostic;
use crate::model::block::Block;
use crate::model::build::Builder;
use crate::model::decl::Styles;
use crate::package::{PartId, Rels};
use crate::span::field::FieldIndex;
use crate::span::{FlowMap, SpanIndex};
use crate::xml::{Dom, NodeId};

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
