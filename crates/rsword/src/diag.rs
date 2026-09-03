//! 诊断（`spec/00-overview.md` §0.5）。
//!
//! 所有层共用同一 [`Diagnostic`]。`code` 是稀疏枚举，按 spec 前缀分组；
//! `origin` 区分"输入文件本来如此"与"本次编辑造成"（`SAVE-02`）：后者在调试构建与 CI 下是错误。

use std::fmt;
use std::ops::Range;

use crate::package::PartId;

/// 缺陷来源（`docs/03` §9.1、`SAVE-02`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationOrigin {
    /// 输入文件本来如此：容错修复并记诊断。
    PreExistingDamage,
    /// 本次编辑造成：调试构建与 CI 下报错，发布构建下修复并记诊断，绝不静默。
    EngineInvariantViolation,
}

/// 诊断代码。每个 spec 定义自己的前缀；这里只收录规范文本中点名的代码，
/// 新代码随实现追加（一经发布不改名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagCode {
    // ---- PKG（spec/01）----
    /// `PKG-02`：part 数超过 10,000。
    PkgTooManyParts,
    /// `PKG-02`：单 part 解压大小超过 512 MiB。
    PkgPartTooLarge,
    /// `PKG-02`：总解压大小超过 1.5 GiB。
    PkgTotalTooLarge,
    /// `PKG-04`：缺少 `[Content_Types].xml`，解析继续。
    PkgNoContentTypes,
    /// `PKG-05`：`http(s)://` 目标未标 `TargetMode="External"`。
    PkgExternalWithoutMode,
    /// `PKG-05`：同一 `.rels` 中重复的关系 `Id`，后者覆盖。
    PkgDupRelId,
    /// `PKG-06`：`..` 越过包根。
    PkgPathEscapesRoot,
    /// `PKG-06`：目标只能按大小写不敏感匹配到 zip 条目。
    PkgCaseInsensitiveMatch,
    /// `PKG-08`：包内 part 的 flavor 不一致，包为 `Mixed`。
    PkgMixedFlavor,
    /// `PKG-11`：非主 XML part 解析失败，标记为 `Opaque`。
    PkgOpaquePart,

    // ---- XML（spec/02）----
    /// `XML-01`：part 非 UTF-8，已转码，无法逐字节保真。
    XmlTranscoded,
    /// `XML-04`：重复属性名，全部保留、语义取第一个。
    XmlDupAttr,
    /// `XML-05`：前缀未绑定，节点保留并可原样写回。
    XmlUnboundPrefix,
    /// `XML-06`：非法字符引用或未知命名实体，保留原文。
    XmlBadEntity,
    /// `XML-08`：标签不平衡等畸形输入（主 part 时为整体 `Err`）。
    XmlMalformed,
    /// `XML-08`：嵌套深度超过 100,000。
    XmlTooDeep,
    /// `XML-09`：`mc:MustUnderstand` 命中未理解的命名空间。
    XmlMustUnderstand,
    /// `XML-09`：`mc:AlternateContent` 既无可选 `Choice` 也无 `Fallback`。
    XmlNoActiveBranch,

    // ---- SPAN（spec/03）----
    /// `SPAN-04`：终点标记找不到起点。
    SpanOrphanEnd,
    /// `SPAN-04`：流结束时起点未闭合。
    SpanUnclosed,
    /// `SPAN-04`：同一 `id` 重复起点。
    SpanDupStart,
    /// `SPAN-05`：范围两端不在同一内容流。
    SpanCrossFlow,

    // ---- FLD（spec/04）----
    /// `FLD-02`：栈空时遇到 `separate`。
    FldStraySeparate,
    /// `FLD-02`：栈空时遇到 `end`。
    FldStrayEnd,
    /// `FLD-02`：流结束时字段未闭合。
    FldUnclosed,
    /// `FLD-07`：对 `w:fldLock` 字段执行 `UpdateBlockField`。
    FldLocked,

    // ---- PROP（spec/05）----
    /// `PROP-02`/`PROP-09`：枚举或颜色等值无法识别，保留为 `Raw`。
    PropBadValue,

    // ---- MOD（spec/06）----
    /// `MOD-07`/`MOD-12`：嵌套表格深度超过 64，降级为 `Protected(TooDeep)`。
    ModTooDeep,
    /// `MOD-05` R07/`MOD-12`：body 子节点无法分类，降级为 `Protected(Unknown)`。
    ModUnknownBlock,
    /// `MOD-12`：块降级为 `Protected(Unparseable)`。
    ModUnparseable,

    // ---- RES（spec/07）----
    /// `RES-02`：`basedOn` 链成环。
    ResStyleCycle,
    /// `RES-02`：`basedOn` 指向不同类型的样式，忽略。
    ResBasedOnTypeMismatch,

    // ---- EDIT（spec/08）----
    /// `EDIT-02`：偏移落在代理对中间。
    EditSplitSurrogate,
    /// `EDIT-03`：拆分段落会让透明字段跨段。
    EditSplitField,
    /// `EDIT-03`：第一阶段 `track_changes` 下不支持 `MoveBlock`。
    EditUnsupportedTrackedMove,
    /// `EDIT-03`：sdt 为 `ContentLocked`/`SdtContentLocked`。
    EditSdtLocked,
    /// `EDIT-03`：sdt 带 `dataBinding`，第一阶段只读。
    EditSdtBound,

    // ---- SAVE（spec/09）----
    /// `SAVE-02`：调试构建与 CI 下的 `EngineInvariantViolation`。
    SaveInvariant,
    /// `SAVE-03`：Strict part 中禁止生成 VML。
    SaveStrictNoVml,
}

impl DiagCode {
    /// 规范文本中的大写下划线写法（`XML_UNBOUND_PREFIX`），用于日志与差分工具输出。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PkgTooManyParts => "PKG_TOO_MANY_PARTS",
            Self::PkgPartTooLarge => "PKG_PART_TOO_LARGE",
            Self::PkgTotalTooLarge => "PKG_TOTAL_TOO_LARGE",
            Self::PkgNoContentTypes => "PKG_NO_CONTENT_TYPES",
            Self::PkgExternalWithoutMode => "PKG_EXTERNAL_WITHOUT_MODE",
            Self::PkgDupRelId => "PKG_DUP_REL_ID",
            Self::PkgPathEscapesRoot => "PKG_PATH_ESCAPES_ROOT",
            Self::PkgCaseInsensitiveMatch => "PKG_CASE_INSENSITIVE_MATCH",
            Self::PkgMixedFlavor => "PKG_MIXED_FLAVOR",
            Self::PkgOpaquePart => "PKG_OPAQUE_PART",
            Self::XmlTranscoded => "XML_TRANSCODED",
            Self::XmlDupAttr => "XML_DUP_ATTR",
            Self::XmlUnboundPrefix => "XML_UNBOUND_PREFIX",
            Self::XmlBadEntity => "XML_BAD_ENTITY",
            Self::XmlMalformed => "XML_MALFORMED",
            Self::XmlTooDeep => "XML_TOO_DEEP",
            Self::XmlMustUnderstand => "XML_MUST_UNDERSTAND",
            Self::XmlNoActiveBranch => "XML_NO_ACTIVE_BRANCH",
            Self::SpanOrphanEnd => "SPAN_ORPHAN_END",
            Self::SpanUnclosed => "SPAN_UNCLOSED",
            Self::SpanDupStart => "SPAN_DUP_START",
            Self::SpanCrossFlow => "SPAN_CROSS_FLOW",
            Self::FldStraySeparate => "FLD_STRAY_SEPARATE",
            Self::FldStrayEnd => "FLD_STRAY_END",
            Self::FldUnclosed => "FLD_UNCLOSED",
            Self::FldLocked => "FLD_LOCKED",
            Self::PropBadValue => "PROP_BAD_VALUE",
            Self::ModTooDeep => "MOD_TOO_DEEP",
            Self::ModUnknownBlock => "MOD_UNKNOWN_BLOCK",
            Self::ModUnparseable => "MOD_UNPARSEABLE",
            Self::ResStyleCycle => "RES_STYLE_CYCLE",
            Self::ResBasedOnTypeMismatch => "RES_BASED_ON_TYPE_MISMATCH",
            Self::EditSplitSurrogate => "EDIT_SPLIT_SURROGATE",
            Self::EditSplitField => "EDIT_SPLIT_FIELD",
            Self::EditUnsupportedTrackedMove => "EDIT_UNSUPPORTED_TRACKED_MOVE",
            Self::EditSdtLocked => "EDIT_SDT_LOCKED",
            Self::EditSdtBound => "EDIT_SDT_BOUND",
            Self::SaveInvariant => "SAVE_INVARIANT",
            Self::SaveStrictNoVml => "SAVE_STRICT_NO_VML",
        }
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一条诊断。`range` 是相对 part 原字节的 UTF-8 字节区间（`XML-15`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub part: PartId,
    pub range: Option<Range<u32>>,
    pub code: DiagCode,
    pub origin: ValidationOrigin,
    pub message: String,
}

impl Diagnostic {
    /// 解析阶段的诊断：来源恒为 [`ValidationOrigin::PreExistingDamage`]。
    pub fn pre_existing(
        part: PartId,
        range: Option<Range<u32>>,
        code: DiagCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            part,
            range,
            code,
            origin: ValidationOrigin::PreExistingDamage,
            message: message.into(),
        }
    }

    /// 编辑或保存阶段新出现的缺陷：来源为 [`ValidationOrigin::EngineInvariantViolation`]。
    pub fn invariant_violation(
        part: PartId,
        range: Option<Range<u32>>,
        code: DiagCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            part,
            range,
            code,
            origin: ValidationOrigin::EngineInvariantViolation,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] part#{}", self.code, self.part.0)?;
        if let Some(r) = &self.range {
            write!(f, " @{}..{}", r.start, r.end)?;
        }
        if matches!(self.origin, ValidationOrigin::EngineInvariantViolation) {
            f.write_str(" (engine invariant violation)")?;
        }
        write!(f, ": {}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_code_as_str_matches_spec_spelling() {
        assert_eq!(DiagCode::XmlUnboundPrefix.as_str(), "XML_UNBOUND_PREFIX");
        assert_eq!(DiagCode::FldUnclosed.to_string(), "FLD_UNCLOSED");
    }

    #[test]
    fn diagnostic_display_carries_origin() {
        let d = Diagnostic::invariant_violation(
            PartId(0),
            Some(10..20),
            DiagCode::SaveInvariant,
            "orphan bookmarkEnd",
        );
        let s = d.to_string();
        assert!(s.starts_with("[SAVE_INVARIANT] part#0 @10..20"));
        assert!(s.contains("engine invariant violation"));
        assert_eq!(
            Diagnostic::pre_existing(PartId(1), None, DiagCode::XmlDupAttr, "x").origin,
            ValidationOrigin::PreExistingDamage
        );
    }
}
