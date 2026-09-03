//! L3 文档模型（`spec/06-model.md`，`docs/03` §6.3–6.8）。
//!
//! `Document` 是 DOM + Span 的**语义投影**：可增量 `refresh`，但任何时刻
//! `Document::rebuild(&dom, &spans)` 必须与增量结果相等（`MOD-13`，测试用它作 oracle）。
//! `Run` 与物理 `w:r` 一一对应，逻辑 run 合并只发生在投影层（`compat_ts`）。M1 任务 1.5、1.8。
