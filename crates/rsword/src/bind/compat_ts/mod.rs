//! `compat_ts` 兼容适配器（`spec/10-compat-ts.md`）。
//!
//! 只读规范状态与投影，输出与今天 TS `ParsedDoc` 字段兼容的 JSON（`COMPAT-02`），并把
//! `SaveBlock[]` 翻译为 `EditOp`（`COMPAT-08`）。所有"半解析"规则封装在此，注释标注 `docs/01`
//! 小节；无法复现的差异登记 `KNOWN_DIFFS.md`。生命周期：M1 建立，M9 删除。
