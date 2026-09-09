//! BIND-11：下游视角的根部 API 与协议生命周期；默认 compat_ts 排除另由 tools/ci/check-native-default.sh 验证。
use rsword::bind::native::{SessionTable, edit_op_from_json};
use rsword::{EditContext, EditOp, EditSession};
use serde_json::{Value, json};

#[test]
fn bind_11_public_protocol_and_core_api_save_identically() {
    let bytes = EditSession::blank(None).unwrap().save().unwrap();
    let mut table = SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    let model: Value = serde_json::from_str(&table.document(&id, None).unwrap()).unwrap();
    let operation = json!({"op":"replaceInlines", "para":model["main"][0]["node"],
        "inlines":[{"kind":"run","value":{"text":"公共 API","props":null}}]})
    .to_string();
    let mut core = EditSession::open(&bytes).unwrap();
    let op: EditOp = edit_op_from_json(&operation, &mut core.dom().clone()).unwrap();
    let result: rsword::MutationResult = core.apply(op, &EditContext::default()).unwrap();
    assert!(!result.affected_blocks.is_empty());
    table.apply(&id, &operation, None).unwrap();
    let output = table.save(&id, None).unwrap();
    assert_eq!(output, core.save().unwrap());
    let reopened = table.open(&output, None).unwrap();
    let model: Value = serde_json::from_str(&table.document(&reopened, None).unwrap()).unwrap();
    assert_eq!(model["main"][0]["inlines"][0]["text"], "公共 API");
    table.close(&reopened);
    table.close(&id);
}

// BIND-11：这是发布时已有的代码表。新增变体不用改此表；旧变体及机器码不许变。
// 禁止从当前 DiagCode 实现重新生成测试期望，避免改名后门跟着漂。
#[test]
fn bind_11_diagnostic_codes_are_append_only() {
    for (code, expected) in RELEASED_CODES {
        assert_eq!(code.as_str(), *expected);
    }
}

const RELEASED_CODES: &[(rsword::DiagCode, &str)] = &[
    (rsword::DiagCode::PkgTooManyParts, "PKG_TOO_MANY_PARTS"),
    (rsword::DiagCode::PkgPartTooLarge, "PKG_PART_TOO_LARGE"),
    (rsword::DiagCode::PkgTotalTooLarge, "PKG_TOTAL_TOO_LARGE"),
    (rsword::DiagCode::PkgNoContentTypes, "PKG_NO_CONTENT_TYPES"),
    (rsword::DiagCode::PkgExternalWithoutMode, "PKG_EXTERNAL_WITHOUT_MODE"),
    (rsword::DiagCode::PkgDupRelId, "PKG_DUP_REL_ID"),
    (rsword::DiagCode::PkgPathEscapesRoot, "PKG_PATH_ESCAPES_ROOT"),
    (rsword::DiagCode::PkgCaseInsensitiveMatch, "PKG_CASE_INSENSITIVE_MATCH"),
    (rsword::DiagCode::PkgMixedFlavor, "PKG_MIXED_FLAVOR"),
    (rsword::DiagCode::PkgOpaquePart, "PKG_OPAQUE_PART"),
    (rsword::DiagCode::PkgRelMissing, "PKG_REL_MISSING"),
    (rsword::DiagCode::XmlTranscoded, "XML_TRANSCODED"),
    (rsword::DiagCode::XmlDupAttr, "XML_DUP_ATTR"),
    (rsword::DiagCode::XmlUnboundPrefix, "XML_UNBOUND_PREFIX"),
    (rsword::DiagCode::XmlBadEntity, "XML_BAD_ENTITY"),
    (rsword::DiagCode::XmlMalformed, "XML_MALFORMED"),
    (rsword::DiagCode::XmlTooDeep, "XML_TOO_DEEP"),
    (rsword::DiagCode::XmlMustUnderstand, "XML_MUST_UNDERSTAND"),
    (rsword::DiagCode::XmlNoActiveBranch, "XML_NO_ACTIVE_BRANCH"),
    (rsword::DiagCode::SpanOrphanEnd, "SPAN_ORPHAN_END"),
    (rsword::DiagCode::SpanUnclosed, "SPAN_UNCLOSED"),
    (rsword::DiagCode::SpanDupStart, "SPAN_DUP_START"),
    (rsword::DiagCode::SpanCrossFlow, "SPAN_CROSS_FLOW"),
    (rsword::DiagCode::FldStraySeparate, "FLD_STRAY_SEPARATE"),
    (rsword::DiagCode::FldStrayEnd, "FLD_STRAY_END"),
    (rsword::DiagCode::FldUnclosed, "FLD_UNCLOSED"),
    (rsword::DiagCode::FldLocked, "FLD_LOCKED"),
    (rsword::DiagCode::PropBadValue, "PROP_BAD_VALUE"),
    (rsword::DiagCode::ModTooDeep, "MOD_TOO_DEEP"),
    (rsword::DiagCode::ModUnknownBlock, "MOD_UNKNOWN_BLOCK"),
    (rsword::DiagCode::ModUnparseable, "MOD_UNPARSEABLE"),
    (rsword::DiagCode::ModTableShape, "MOD_TABLE_SHAPE"),
    (rsword::DiagCode::ChartNoSeries, "CHART_NO_SERIES"),
    (rsword::DiagCode::RevUnpairedMove, "REV_UNPAIRED_MOVE"),
    (rsword::DiagCode::EditInDeleted, "EDIT_IN_DELETED"),
    (rsword::DiagCode::EditMathBadLatex, "EDIT_MATH_BAD_LATEX"),
    (rsword::DiagCode::EditMathTooDeep, "EDIT_MATH_TOO_DEEP"),
    (rsword::DiagCode::EditMathTokenCount, "EDIT_MATH_TOKEN_COUNT"),
    (rsword::DiagCode::RevNotTracked, "REV_NOT_TRACKED"),
    (rsword::DiagCode::ResStyleCycle, "RES_STYLE_CYCLE"),
    (rsword::DiagCode::ResBasedOnTypeMismatch, "RES_BASED_ON_TYPE_MISMATCH"),
    (rsword::DiagCode::EditSplitSurrogate, "EDIT_SPLIT_SURROGATE"),
    (rsword::DiagCode::EditBadPosition, "EDIT_BAD_POSITION"),
    (rsword::DiagCode::EditCrossParagraph, "EDIT_CROSS_PARAGRAPH"),
    (rsword::DiagCode::EditCrossContainer, "EDIT_CROSS_CONTAINER"),
    (rsword::DiagCode::EditTargetFallback, "EDIT_TARGET_FALLBACK"),
    (rsword::DiagCode::EditBadText, "EDIT_BAD_TEXT"),
    (rsword::DiagCode::EditAnchorUnmoved, "EDIT_ANCHOR_UNMOVED"),
    (rsword::DiagCode::EditPlanInvalid, "EDIT_PLAN_INVALID"),
    (rsword::DiagCode::EditUnsupported, "EDIT_UNSUPPORTED"),
    (rsword::DiagCode::EditTargetOpaque, "EDIT_TARGET_OPAQUE"),
    (rsword::DiagCode::EditTargetMissing, "EDIT_TARGET_MISSING"),
    (rsword::DiagCode::EditTableGridInconsistent, "EDIT_TABLE_GRID_INCONSISTENT"),
    (rsword::DiagCode::EditTableGeometry, "EDIT_TABLE_GEOMETRY"),
    (rsword::DiagCode::EditSplitField, "EDIT_SPLIT_FIELD"),
    (rsword::DiagCode::EditUnsupportedTrackedMove, "EDIT_UNSUPPORTED_TRACKED_MOVE"),
    (rsword::DiagCode::EditUnsupportedTrackedMerge, "EDIT_UNSUPPORTED_TRACKED_MERGE"),
    (rsword::DiagCode::EditSdtLocked, "EDIT_SDT_LOCKED"),
    (rsword::DiagCode::EditSdtBound, "EDIT_SDT_BOUND"),
    (rsword::DiagCode::SaveInvariant, "SAVE_INVARIANT"),
    (rsword::DiagCode::SaveStrictNoVml, "SAVE_STRICT_NO_VML"),
    (rsword::DiagCode::SaveTableGrid, "SAVE_TABLE_GRID"),
    (rsword::DiagCode::BindBadArgument, "BIND_BAD_ARGUMENT"),
    (rsword::DiagCode::BindXmlEscape, "BIND_XML_ESCAPE"),
    (rsword::DiagCode::BindNoSession, "BIND_NO_SESSION"),
    (rsword::DiagCode::BindIdUnknown, "BIND_ID_UNKNOWN"),
    (rsword::DiagCode::BindProtocolMismatch, "BIND_PROTOCOL_MISMATCH"),
];
