//! `BIND-03` 操作表：线型、双向穷尽转换、逐变体测试和清单同源。
use super::codec::Codec;
use super::codec::*;
use super::payload::*;
use crate::edit::*;
#[cfg(test)]
use crate::model::RevisionId;
use crate::model::{HfKind, HfVariant};
use crate::package::PartId;
use crate::save::options::decl::*;
use crate::semantic::props::*;
use crate::span::FieldId;
use crate::xml::{Dom, NodeId};

macro_rules! edit_op_json {
    (($dom:ident) $($variant:ident { $($(#[$attr:meta])* $field:ident : $codec:ty = $sample:expr),* $(,)? } context ($part:expr) test $test:ident $outcome:ident;)+) => {
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
        #[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
        #[allow(clippy::large_enum_variant)]
        /// BIND-03 编辑线型；语义及拒绝条件见对应的引擎操作。
        #[non_exhaustive]
        #[cfg_attr(rsword_api_docs, deny(missing_docs))]
        pub enum EditOpJson {
            $(#[doc = concat!("对应 [`EditOp::", stringify!($variant), "`]；字段采用 BIND-03 线型。")]
            $variant { $($(#[$attr])* #[doc = concat!("操作的 `", stringify!($field), "` 参数，语义见对应引擎变体。")]
                $field: <$codec as Codec>::Wire),* },)+
        }
        #[cfg_attr(rsword_api_docs, deny(missing_docs))]
        impl EditOpJson {
            /// 在指定 part 的 DOM 上转换，用于调试和审计；不能无损表示时返回具名错误。
            pub fn from_engine(op: &EditOp, dom: &Dom) -> Result<Self> {
                Ok(match op {
                    $(EditOp::$variant { $($field),* } => Self::$variant {
                        $($field: <$codec>::encode($field, dom).map_err(|e| e.at(concat!(stringify!($variant), ".", stringify!($field))))?),*
                    },)+
                })
            }
            pub(crate) fn to_engine(&self, cx: &mut DecodeCx<'_>) -> Result<EditOp> {
                Ok(match self {
                    $(Self::$variant { $($field),* } => EditOp::$variant {
                        $($field: <$codec>::decode($field, cx).map_err(|e| e.at(concat!(stringify!($variant), ".", stringify!($field))))?),*
                    },)+
                })
            }
            #[allow(unused_variables)]
            pub(crate) fn context_part(&self) -> Option<PartId> {
                match self { $(Self::$variant { $($field),* } => $part,)+ }
            }
            /// 同表生成的完整变体清单，名称为 Rust 变体名。
            pub const VARIANTS: &'static [&'static str] = &[$(stringify!($variant)),+];
        }
        #[cfg(test)]
        pub(super) fn examples($dom: &mut Dom) -> Vec<(&'static str, EditOp, bool)> {
            vec![$((stringify!($variant), EditOp::$variant { $($field: $sample),* }, edit_op_json!(@status $outcome))),+]
        }
        $(#[cfg(test)] #[test]
        fn $test() {
            let mut scratch = super::fixtures::scratch(); let $dom = &mut scratch;
            let op = EditOp::$variant { $($field: $sample),* };
            super::fixtures::check(stringify!($variant), op, $dom, edit_op_json!(@status $outcome));
        })+
    };
    (@status lossless) => { true };
    (@status refused) => { false };
}

edit_op_json! { (dom)
    SetSources { sources: Shared<Vec<SourceSave>> = vec![SourceSave { tag: "audit".into(), title: "source".into(), ..Default::default() }], } context (None) test bind_03_set_sources_roundtrip lossless;
    AddNumberingDefinition { definition: Shared<NumberingDefSave> = NumberingDefSave { num_id: "42".into(), bullet: false, levels: vec![] }, } context (None) test bind_03_add_numbering_definition_roundtrip lossless;
    RestartNumbering { restart: Shared<RestartNumSave> = RestartNumSave { num_id: "43".into(), abstract_num_id: "0".into(), start_overrides: vec![(0, 2)] }, } context (None) test bind_03_restart_numbering_roundtrip lossless;
    SetThemeFonts { fonts: Shared<ThemeFontsSave> = ThemeFontsSave { major: "Arial".into(), minor: "Calibri".into(), east_asia: None }, } context (None) test bind_03_set_theme_fonts_roundtrip lossless;
    SetThemeColors { colors: Shared<ThemeColorsSave> = ThemeColorsSave { name: Some("Audit".into()), slots: vec![("accent1".into(), "123456".into())] }, } context (None) test bind_03_set_theme_colors_roundtrip lossless;
    UpsertStyle { style: Shared<StyleUpsertSave> = StyleUpsertSave { style_id: "Audit".into(), kind: "paragraph".into(), name: "Audit".into(), based_on: None, run_props: None, para_props: None }, } context (None) test bind_03_upsert_style_roundtrip lossless;
    InsertText {
        at: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        text: Shared<String> = "sample".to_owned(),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Shared<Option<RunPropsPatch>> = None,
    } context (at.part) test bind_03_insert_text_roundtrip lossless;
    DeleteRange {
        from: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        to: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
    } context (from.part) test bind_03_delete_range_roundtrip lossless;
    SetRunProps {
        from: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        to: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        patch: Shared<RunPropsPatch> = Default::default(),
    } context (from.part) test bind_03_set_run_props_roundtrip lossless;
    ReplaceInlines {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Shared<Option<PartId>> = None,
        para: Shared<NodeId> = NodeId(2),
        inlines: List<InlineCodec> = vec![NewInline::Run(super::fixtures::bad_run(dom))],
    } context (*part) test bind_03_replace_inlines_roundtrip refused;
    SetParaProps {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Shared<Option<PartId>> = None,
        para: Shared<NodeId> = NodeId(2),
        patch: Shared<ParaPropsPatch> = Default::default(),
    } context (*part) test bind_03_set_para_props_roundtrip lossless;
    ReplaceParaProps {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Shared<Option<PartId>> = None,
        para: Shared<NodeId> = NodeId(2),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Optional<XmlElement> = Some(super::fixtures::xml(dom)),
    } context (*part) test bind_03_replace_para_props_roundtrip lossless;
    SetTableProps {
        table: Shared<NodeId> = NodeId(2),
        patch: Shared<TablePropsPatch> = Default::default(),
    } context (None) test bind_03_set_table_props_roundtrip lossless;
    SetRowProps {
        row: Shared<NodeId> = NodeId(2),
        patch: Shared<RowPropsPatch> = Default::default(),
    } context (None) test bind_03_set_row_props_roundtrip lossless;
    SetCellProps {
        cell: Shared<NodeId> = NodeId(2),
        patch: Shared<CellPropsPatch> = Default::default(),
    } context (None) test bind_03_set_cell_props_roundtrip lossless;
    InsertRow {
        table: Shared<NodeId> = NodeId(2),
        at: Shared<u32> = 1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Shared<Option<NodeId>> = None,
    } context (None) test bind_03_insert_row_roundtrip lossless;
    DeleteRow {
        table: Shared<NodeId> = NodeId(2),
        at: Shared<u32> = 1,
    } context (None) test bind_03_delete_row_roundtrip lossless;
    InsertColumn {
        table: Shared<NodeId> = NodeId(2),
        at: Shared<u32> = 1,
        width: Shared<i32> = 240,
    } context (None) test bind_03_insert_column_roundtrip lossless;
    DeleteColumn {
        table: Shared<NodeId> = NodeId(2),
        at: Shared<u32> = 1,
    } context (None) test bind_03_delete_column_roundtrip lossless;
    MergeCells {
        table: Shared<NodeId> = NodeId(2),
        from: Shared<(u32, u32)> = (0, 1),
        to: Shared<(u32, u32)> = (0, 1),
    } context (None) test bind_03_merge_cells_roundtrip lossless;
    InsertBlock {
        at: Shared<BlockPos> = BlockPos::end(NodeId(1)),
        block: BlockCodec = super::fixtures::bad_block(dom),
    } context (at.part) test bind_03_insert_block_roundtrip refused;
    DeleteBlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Shared<Option<PartId>> = None,
        node: Shared<NodeId> = NodeId(2),
    } context (*part) test bind_03_delete_block_roundtrip lossless;
    MoveBlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Shared<Option<PartId>> = None,
        node: Shared<NodeId> = NodeId(2),
        to: Shared<BlockPos> = BlockPos::end(NodeId(1)),
    } context (None) test bind_03_move_block_roundtrip lossless;
    AddComment {
        from: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        to: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        comment: Shared<NewComment> = NewComment { author: "author".into(), text: "comment".into(), ..Default::default() },
    } context (from.part) test bind_03_add_comment_roundtrip lossless;
    RemoveComment {
        id: Shared<String> = "sample".to_owned(),
    } context (None) test bind_03_remove_comment_roundtrip lossless;
    SetCommentText {
        id: Shared<String> = "sample".to_owned(),
        text: Shared<String> = "sample".to_owned(),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        done: Shared<Option<bool>> = None,
    } context (None) test bind_03_set_comment_text_roundtrip lossless;
    SplitParagraph {
        at: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
    } context (at.part) test bind_03_split_paragraph_roundtrip lossless;
    MergeWithNext {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Shared<Option<PartId>> = None,
        para: Shared<NodeId> = NodeId(2),
    } context (*part) test bind_03_merge_with_next_roundtrip lossless;
    AddBookmark {
        name: Shared<String> = "sample".to_owned(),
        from: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        to: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
    } context (from.part) test bind_03_add_bookmark_roundtrip lossless;
    RemoveBookmark {
        name: Shared<String> = "sample".to_owned(),
    } context (None) test bind_03_remove_bookmark_roundtrip lossless;
    InsertField {
        at: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        field: FieldCodec = super::fixtures::bad_field(dom),
    } context (at.part) test bind_03_insert_field_roundtrip refused;
    SetLinkTarget {
        link: Shared<LinkRef> = LinkRef::Element(NodeId(2)),
        target: Shared<LinkDest> = LinkDest::Url("https://example.test".into()),
    } context (None) test bind_03_set_link_target_roundtrip lossless;
    ToggleCheckbox {
        field: Shared<FieldId> = FieldId(0),
    } context (None) test bind_03_toggle_checkbox_roundtrip lossless;
    SetFormText {
        field: Shared<FieldId> = FieldId(0),
        text: Shared<String> = "sample".to_owned(),
    } context (None) test bind_03_set_form_text_roundtrip lossless;
    SetFieldResultProps {
        field: Shared<FieldId> = FieldId(0),
        patch: Shared<RunPropsPatch> = Default::default(),
    } context (None) test bind_03_set_field_result_props_roundtrip lossless;
    UpdateBlockField {
        field: Shared<FieldId> = FieldId(0),
        blocks: List<BlockCodec> = vec![super::fixtures::bad_block(dom)],
    } context (None) test bind_03_update_block_field_roundtrip refused;
    SetChartData {
        part: Shared<PartId> = PartId(0),
        patch: Shared<ChartPatch> = Default::default(),
    } context (Some(*part)) test bind_03_set_chart_data_roundtrip lossless;
    ReplacePartXml {
        part: Shared<PartId> = PartId(0),
        xml: PartXml = "sample".to_owned(),
    } context (Some(*part)) test bind_03_replace_part_xml_roundtrip lossless;
    ReplacePartBytes {
        part: Shared<PartId> = PartId(0),
        bytes: Binary = vec![0, 1, 254, 255],
    } context (Some(*part)) test bind_03_replace_part_bytes_roundtrip lossless;
    ReplaceImageMedia {
        drawing: Shared<NodeId> = NodeId(2),
        bytes: Shared<Vec<u8>> = vec![0, 1, 254, 255],
        mime: Shared<String> = "sample".to_owned(),
    } context (None) test bind_03_replace_image_media_roundtrip lossless;
    RemoveInks {
    } context (None) test bind_03_remove_inks_roundtrip lossless;
    InsertInk {
        para: Shared<NodeId> = NodeId(2),
        ink: Shared<NewInk> = NewInk { png: vec![1, 2, 3], width_px: 10.0, height_px: 12.0, offset_x_px: 1.0, offset_y_px: -2.0, payload: Some("ink".into()) },
    } context (None) test bind_03_insert_ink_roundtrip lossless;
    SetSectionProps {
        sect: Shared<NodeId> = NodeId(2),
        patch: Shared<SectionPropsPatch> = Default::default(),
    } context (None) test bind_03_set_section_props_roundtrip lossless;
    SetHeaderFooter {
        sect: Shared<NodeId> = NodeId(2),
        kind: Shared<HfKind> = HfKind::Header,
        variant: Shared<HfVariant> = HfVariant::Default,
        content: List<BlockCodec> = vec![super::fixtures::bad_block(dom)],
    } context (None) test bind_03_set_header_footer_roundtrip refused;
    LinkHeaderFooter {
        sect: Shared<NodeId> = NodeId(2),
        kind: Shared<HfKind> = HfKind::Header,
        variant: Shared<HfVariant> = HfVariant::Default,
        part: Shared<PartId> = PartId(0),
    } context (Some(*part)) test bind_03_link_header_footer_roundtrip lossless;
    SetWatermark {
        sect: Shared<NodeId> = NodeId(2),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Shared<Option<String>> = None,
    } context (None) test bind_03_set_watermark_roundtrip lossless;
    SetPageColor {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Shared<Option<String>> = None,
    } context (None) test bind_03_set_page_color_roundtrip lossless;
    SetDocumentSettings {
        patch: Shared<SettingsPatch> = Default::default(),
    } context (None) test bind_03_set_document_settings_roundtrip lossless;
    InsertAtom {
        at: Shared<InlinePos> = InlinePos::new(NodeId(2), 0),
        atom: AtomCodec = NewAtom::NoteRef { endnote: false, content: vec![vec![super::fixtures::bad_run(dom)]] },
    } context (at.part) test bind_03_insert_atom_roundtrip refused;
    SetNoteContent {
        endnote: Shared<bool> = false,
        id: Shared<String> = "sample".to_owned(),
        content: List<List<RunCodec>> = vec![vec![super::fixtures::bad_run(dom)]],
    } context (None) test bind_03_set_note_content_roundtrip refused;
    RemoveNote {
        endnote: Shared<bool> = false,
        id: Shared<String> = "sample".to_owned(),
    } context (None) test bind_03_remove_note_roundtrip lossless;
    SetSdtContent {
        sdt: Shared<NodeId> = NodeId(2),
        inlines: List<InlineCodec> = vec![NewInline::Run(super::fixtures::bad_run(dom))],
    } context (None) test bind_03_set_sdt_content_roundtrip refused;
    RemoveSdtShell {
        sdt: Shared<NodeId> = NodeId(2),
    } context (None) test bind_03_remove_sdt_shell_roundtrip lossless;
    SetMathTokens {
        math: Shared<NodeId> = NodeId(2),
        tokens: Shared<Vec<String>> = vec!["a".into(), "b".into()],
    } context (None) test bind_03_set_math_tokens_roundtrip lossless;
    SetDrawingGeometry {
        drawing: Shared<NodeId> = NodeId(2),
        geom: Shared<DrawingGeometry> = Default::default(),
    } context (None) test bind_03_set_drawing_geometry_roundtrip lossless;
    SetDrawingZOrder {
        drawing: Shared<NodeId> = NodeId(2),
        z: Shared<i64> = 2,
    } context (None) test bind_03_set_drawing_zorder_roundtrip lossless;
    SetDrawingWrap {
        drawing: Shared<NodeId> = NodeId(2),
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wrap: Shared<Option<media_ops::ImageWrap>> = None,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pos: Shared<Option<AnchorPos>> = None,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z_order: Shared<Option<i64>> = None,
    } context (None) test bind_03_set_drawing_wrap_roundtrip lossless;
    SetShapeStyle {
        shape: Shared<NodeId> = NodeId(2),
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::semantic::props::serde::double_option")]
        fill: Shared<Option<Option<String>>> = Some(None),
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::semantic::props::serde::double_option")]
        outline: Shared<Option<Option<String>>> = Some(None),
    } context (None) test bind_03_set_shape_style_roundtrip lossless;
    RegenerateBlockField {
        field: Shared<crate::span::FieldId> = FieldId(0),
        options: Shared<BlockFieldOptions> = Default::default(),
    } context (None) test bind_03_regenerate_block_field_roundtrip lossless;
    SetTextboxContent {
        textbox: Shared<NodeId> = NodeId(2),
        blocks: List<BlockCodec> = vec![super::fixtures::bad_block(dom)],
    } context (None) test bind_03_set_textbox_content_roundtrip refused;
    InsertSectionBreak {
        after: Shared<NodeId> = NodeId(2),
        kind: Shared<crate::semantic::props::SectType> = SectType::NextPage,
    } context (None) test bind_03_insert_section_break_roundtrip lossless;
    DeleteSectionBreak {
        sect: Shared<NodeId> = NodeId(2),
    } context (None) test bind_03_delete_section_break_roundtrip lossless;
    AcceptRevision {
        rev: Shared<crate::model::RevisionId> = RevisionId(0),
    } context (None) test bind_03_accept_revision_roundtrip lossless;
    RejectRevision {
        rev: Shared<crate::model::RevisionId> = RevisionId(0),
    } context (None) test bind_03_reject_revision_roundtrip lossless;
    AcceptAll {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Shared<Option<String>> = None,
    } context (None) test bind_03_accept_all_roundtrip lossless;
    RejectAll {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Shared<Option<String>> = None,
    } context (None) test bind_03_reject_all_roundtrip lossless;
}
