//! 页眉页脚 part 的内容流（`MOD-01` 的 `hf_parts`、`docs/03` §6.7，`spec/16` 任务 5.3）。
//!
//! **复用正文管线**：`w:hdr` / `w:ftr` 里的段落、表格、sdt、修订包裹、文本框都走同一个
//! `Builder::build_container`，所以 `Block` / `Inline` / 显示模型的形状与正文完全一样，
//! 编辑操作也就不需要为页眉另写一份。
//!
//! 每个 part 自带三份索引（`FlowMap` / `FieldIndex` / `SpanIndex`）。它们都是**按 part** 的：
//! `NodeId` 相对该 part 自己的 DOM，`FlowId` 也只在该 part 内有意义（`SPAN-01`：`w:hdr` / `w:ftr`
//! 各是一个独立内容流，范围与字段禁止跨流，也就禁止跨 part）。
//!
//! `has_page_number` / `has_num_pages` 由该 part 的字段索引推导（`FLD-11`），不是扫字符串：
//! `PAGE` / `NUMPAGES` 是原子字段，渲染器看到 `Keyword::Page` 自己替换页码
//! （`docs/03` §5.4 末段——所以模型里没有 `PAGE_MARK` 这类占位符，那只存在于 `compat_ts`）。

use crate::diag::{DiagCode, Diagnostic};
use crate::model::block::Block;
use crate::model::build::Builder;
use crate::model::decl::Styles;
use crate::model::section::HfKind;
use crate::package::{PartId, Rels};
use crate::span::field::{FieldIndex, Keyword};
use crate::span::{FlowMap, SpanIndex};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName};

/// 一个页眉或页脚 part。
#[derive(Debug, Clone, PartialEq)]
pub struct HfPart {
    pub part: PartId,
    pub kind: HfKind,
    /// `w:hdr` / `w:ftr`。
    pub root: NodeId,
    /// 内容，与正文同一构建器（`docs/03` §6.7）。
    pub blocks: Vec<Block>,
    /// 本 part 的内容流映射（`SPAN-01`）。
    pub flows: FlowMap,
    /// 本 part 的字段索引（`FLD-02`）。
    pub fields: FieldIndex,
    /// 本 part 的范围索引（`SPAN-04`）：页眉里的书签与批注标记也要成对认领。
    pub spans: SpanIndex,
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
        let flows = FlowMap::build(dom);
        let mut fields = FieldIndex::build(dom);
        warnings.extend(fields.take_diagnostics());
        let mut spans = SpanIndex::build(dom);
        warnings.extend(spans.take_diagnostics());

        let mut b = Builder::new(dom, styles, rels, &fields, &spans, Vec::new());
        let mut blocks = Vec::new();
        b.build_container(root, None, &[], &mut blocks);
        warnings.append(&mut b.warnings);

        let has = |k: &Keyword| fields.fields().iter().any(|f| f.keyword() == k);
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
            flows,
            has_page_number: has(&Keyword::Page) || legacy_pg_num,
            has_num_pages: has(&Keyword::NumPages),
            watermark: watermark_of(dom),
            fields,
            spans,
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
