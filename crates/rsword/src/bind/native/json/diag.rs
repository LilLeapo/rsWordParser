//! 诊断的 JSON 投影（`BIND-07`，`spec/00` §0.5）。
//!
//! 形态 `{ part, range?: [start, end], code, origin, message }`：`code` 是稳定契约
//! （`DiagCode::as_str` 的字串，调用方可依赖），`message` 只给人看（**禁止**调用方解析）；
//! `origin` 区分「输入文件本来如此」与「本次编辑造成」（`SAVE-02`：后者在调试构建与 CI 下
//! 是错误）。`range` 是相对 part 原字节的 UTF-8 字节区间（`XML-15`），投成 `[start, end]`。

use std::ops::Range;

use crate::diag::{DiagCode, Diagnostic, ValidationOrigin};
use crate::package::PartId;

use super::{as_str_json, json_str_enum, model_json};

as_str_json!(DiagCode);

json_str_enum! {
    /// 缺陷来源（`docs/03` §9.1、`SAVE-02`）。
    ValidationOrigin test json_fields_cover_validation_origin {
        PreExistingDamage => "preExistingDamage";
        EngineInvariantViolation => "engineInvariantViolation";
    }
}

model_json! {
    /// 一条诊断（`BIND-07`；`spec/00` §0.5）。
    struct Diagnostic(cx) test json_fields_cover_diagnostic {
        part => "part", PartId = part;
        opt range => "range", Range<u32> = range;
        code => "code", DiagCode = code;
        origin => "origin", ValidationOrigin = origin;
        message => "message", String = message;
    }
}
