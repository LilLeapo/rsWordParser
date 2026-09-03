//! L4 编辑引擎（`spec/08-edit.md`，`docs/03` §8）。
//!
//! `EditOp → plan（只读）→ validate（只读）→ commit（机械写入，不可失败）→ model.refresh`。
//! 任何一步 `Err` 都不留下半修改状态（`EDIT-05`）。偏移单位对外统一为 UTF-16 code unit，
//! 原子为一个 `U+FFFC`（`EDIT-02`）。M1 任务 1.11–1.13（`InsertText`/`DeleteRange`/
//! `SetRunProps`/`SetParaProps`/`ReplaceInlines`，不含修订生成）。
