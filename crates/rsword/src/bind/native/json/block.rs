//! 块模型的 JSON 投影（`BIND-02`；`MOD-02` 块、`MOD-03` 段落分类、`MOD-05` 图片块、
//! `MOD-08` 只读块、`MOD-09` 块级 / 段落标记修订）。
//!
//! `Block` 四变体全部平铺：载荷对象的键铺开后再写内标签 `kind`（`flatten_variant_json`
//! 后写覆盖先写），所以载荷自己的 `kind` 字段（`TextBlock.kind` / `ProtectedBlock.kind`）
//! 改名为 `textKind` / `protectedKind`——不改名的话平铺会丢段落分类与保护原因。
//! `ImageBlock.display` 与 `ProtectedBlock.display` / `siblings` 属显示模型（`MOD-11`），
//! `display: true` 才投影（决策 4）。

use crate::model::Display;
use crate::model::Inline;
use crate::model::ParagraphFacts;
use crate::model::table::TableBlock;
use crate::model::{
    Block, ImageBlock, ListRef, ProtectedBlock, ProtectedKind, Revision, SdtInfo, TextBlock,
    TextKind,
};
use crate::semantic::props::{CellProps, ParaProps, RowProps, SectionProps, TableProps};
use crate::span::{FieldId, RevisionMeta};
use crate::xml::{NodeId, QName};

use super::model_json;

model_json! {
    /// 文档块（`MOD-02`）；平铺变体，`kind` 是块别。
    enum Block(cx) test json_fields_cover_block {
        /// 可编辑段落（装箱：`TextBlock` 比其他变体大几十倍）。
        Text(Box<TextBlock>) => "text";
        Table(TableBlock) => "table";
        /// 只含一张图片的段落（`MOD-05` R15）。
        Image(ImageBlock) => "image";
        /// 只读块。
        Protected(ProtectedBlock) => "protected";
    }
}

model_json! {
    /// 可编辑段落（`MOD-02`）。
    struct TextBlock(cx) test json_fields_cover_text_block {
        node => "node", NodeId = node;
        ~ kind => "textKind", TextKind = kind, "平铺进 Block 时会被内标签键 kind 覆盖（flatten_variant_json 后写覆盖先写），改名保住段落分类";
        opt style_id => "styleId", String = style_id;
        /// 声明值（`w:pPr`），段落标记 rPr 在 `props.rpr`。
        props => "props", ParaProps = props;
        inlines => "inlines", Vec<Inline> = inlines;
        opt sdt => "sdt", SdtInfo = sdt;
        /// 块级修订：`w:ins/w:del` 包裹、段落标记 ins/del、`pPrChange`。
        revisions => "revisions", Vec<Revision> = revisions;
        facts => "facts", ParagraphFacts = facts;
    }
}

model_json! {
    /// 段落分类（`MOD-03`）。
    enum TextKind(cx) test json_fields_cover_text_kind {
        Paragraph => "paragraph";
        Heading { level } => "heading" {
            level => "level", u8 = level;
        };
        ListItem { list } => "listItem" {
            list => "list", ListRef = list;
        };
    }
}

model_json! {
    /// 编号引用（`MOD-03`）。
    struct ListRef(cx) test json_fields_cover_list_ref {
        num_id => "numId", i32 = num_id;
        ilvl => "ilvl", i32 = ilvl;
        /// 来自段落样式链而非直接 `w:numPr`。
        flag from_style => "fromStyle" = from_style;
    }
}

model_json! {
    /// 只含一张图片的段落（`MOD-05` R15）。
    struct ImageBlock(cx) test json_fields_cover_image_block {
        node => "node", NodeId = node;
        /// 该段唯一那个绘图的显示模型（`MOD-11`）；`display: true` 才投影。
        raw opt display => "display", Display = display.as_ref().filter(|_| cx.display).map(|d| d.to_json(cx));
        opt sdt => "sdt", SdtInfo = sdt;
        revisions => "revisions", Vec<Revision> = revisions;
    }
}

model_json! {
    /// 只读块（`MOD-08`）。
    struct ProtectedBlock(cx) test json_fields_cover_protected_block {
        node => "node", NodeId = node;
        ~ kind => "protectedKind", ProtectedKind = kind, "平铺进 Block 时会被内标签键 kind 覆盖，改名保住保护原因";
        /// 可见文本预览（最多 80 个字符），供编辑器显示占位。
        preview => "preview", String = preview;
        /// 段落里第一个图形的显示载荷（`MOD-11`）；`display: true` 才投影。
        raw opt display => "display", Display = display.as_ref().filter(|_| cx.display).map(|d| d.to_json(cx));
        /// 段落里其余顶层绘图的显示模型（`R13`，文档序）；`display: true` 且非空才投影。
        raw opt siblings => "siblings", Vec<Display> = if cx.display && !siblings.is_empty() { Some(siblings.to_json(cx)) } else { None };
        opt sdt => "sdt", SdtInfo = sdt;
        revisions => "revisions", Vec<Revision> = revisions;
    }
}

model_json! {
    /// 保护原因（`MOD-08`）。显示载荷挂在 `ProtectedBlock.display`。
    enum ProtectedKind(cx) test json_fields_cover_protected_kind {
        /// 块级字段结果。
        FieldBlockResult(FieldId) as "fieldId" => "fieldBlockResult";
        Equation => "equation";
        Chart => "chart";
        SmartArt => "smartArt";
        Ole => "ole";
        Rule => "rule";
        Invisible => "invisible";
        SectionBreak => "sectionBreak";
        SectionProps => "sectionProps";
        BodyBreak { page } => "bodyBreak" {
            flag page => "page" = page;
        };
        Unknown(QName) as "name" => "unknown";
        TooDeep => "tooDeep";
        Unparseable => "unparseable";
    }
}

model_json! {
    /// 块级 / 段落标记修订（`MOD-09`）。run 级修订在 `RevisionCtx`（inline 域）。
    enum Revision(cx) test json_fields_cover_revision {
        /// 顶层 `w:ins` 包裹的块；也用于 `trPr/ins`（整行插入，挂在 `Row.revisions`）。
        Insert(RevisionMeta) => "insert";
        /// 顶层 `w:del` 包裹的块；也用于 `trPr/del`（整行删除）。
        Delete(RevisionMeta) => "delete";
        MoveFrom(RevisionMeta) => "moveFrom";
        MoveTo(RevisionMeta) => "moveTo";
        /// `pPr/rPr/ins`：段落标记被插入。
        ParaMarkInsert(RevisionMeta) => "paraMarkInsert";
        /// `pPr/rPr/del`：段落标记被删除（与下一段合并）。
        ParaMarkDelete(RevisionMeta) => "paraMarkDelete";
        /// `pPrChange`：旧值快照。
        ParaPropsChange { meta, old } => "paraPropsChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", Box<ParaProps> = old;
        };
        /// `numPr/numberingChange`。
        NumberingChange(RevisionMeta) => "numberingChange";
        /// `tblPr/tblPrChange`：表格属性旧值（`TableBlock.revisions`）。
        TablePropsChange { meta, old } => "tablePropsChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", Box<TableProps> = old;
        };
        /// `sectPr/sectPrChange`：旧值快照（`SectionInfo.revisions`）。
        SectPropsChange { meta, old } => "sectPropsChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", Box<SectionProps> = old;
        };
        /// `tblGrid/tblGridChange`：旧网格；`old` 是快照里的 `w:tblGrid`（没有就是 change 元素本身）。
        TableGridChange { meta, old } => "tableGridChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", NodeId = old;
        };
        /// `trPr/trPrChange`（`Row.revisions`）。
        RowPropsChange { meta, old } => "rowPropsChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", Box<RowProps> = old;
        };
        /// `tcPr/tcPrChange`（`Cell.revisions`）。
        CellPropsChange { meta, old } => "cellPropsChange" {
            meta => "meta", RevisionMeta = meta;
            old => "old", Box<CellProps> = old;
        };
        /// `tcPr/cellIns`。
        CellInsert(RevisionMeta) => "cellInsert";
        /// `tcPr/cellDel`。
        CellDelete(RevisionMeta) => "cellDelete";
        /// `tcPr/cellMerge`。
        CellMerge(RevisionMeta) => "cellMerge";
    }
}
