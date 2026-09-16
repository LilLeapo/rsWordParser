//! AGENT-06：所有 Agent 读取共用的完整信封双预算。
use crate::{Result, error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    pub limit: usize,
    pub max_bytes: usize,
}
impl Budget {
    pub const OUTLINE: Self = Self { limit: 4000, max_bytes: 16000 };
    pub const FIND: Self = Self { limit: 4000, max_bytes: 24000 };
    pub const CONTEXT: Self = Self { limit: 8000, max_bytes: 24000 };
    pub fn validate(self) -> Result<Self> {
        if !(1..=1048576).contains(&self.limit) || !(512..=4194304).contains(&self.max_bytes) {
            return Err(error("BIND_BAD_ARGUMENT", "预算超出允许范围"));
        }
        Ok(self)
    }
}
/// nextCursor 为服务端提供的不可伪造句柄；本层不接受客户端提供的 offset。
pub fn envelope(
    snapshot: &str,
    content: Value,
    range: Value,
    truncated: bool,
    cursor: Option<&str>,
) -> Value {
    let content_units = content.as_str().map_or_else(
        || serde_json::to_string(&content).unwrap().encode_utf16().count(),
        |s| s.encode_utf16().count(),
    );
    let empty = content.as_array().is_some_and(Vec::is_empty) || content.as_str() == Some("");
    let mut out = json!({"snapshot":snapshot,"content":content,"empty":empty,"range":range,"truncated":truncated,"nextCursor":cursor,"omitted":{"page":[],"complete":!truncated},"anchorCounts":{"sourceUtf16":0,"presentationUtf16":0,"sourceScalars":0,"presentationScalars":0,"scope":"notApplicable"},"usage":{"contentUtf16":content_units,"responseBytes":0,"estimatedTokens":0}});
    if let Ok(value) = serde_json::from_str::<Value>(snapshot) {
        out["snapshot"] = value;
    }
    measure(&mut out);
    out
}
pub fn measure(out: &mut Value) {
    loop {
        let size = serde_json::to_vec(&out).unwrap().len();
        let tokens = size.div_ceil(4);
        if out["usage"]["responseBytes"] == size && out["usage"]["estimatedTokens"] == tokens {
            break;
        }
        out["usage"]["responseBytes"] = json!(size);
        out["usage"]["estimatedTokens"] = json!(tokens);
    }
}
pub fn fits(v: &Value, b: Budget) -> bool {
    Size::from(v).fits(b)
}
/// 候选页的预算尺寸账（AGENT-06）。
///
/// 前缀选择只需要 `contentUtf16` 与两形态共同上界 `common_bytes`；把这两个数
/// 一次算清后，`fits` 就是纯算术比较。这让选择算法与整包序列化解耦：oracle
/// 测试可以缓存每个 `end` 的 `Size`，不必为每个预算重复序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub content_utf16: usize,
    pub common_bytes: usize,
}
impl Size {
    pub fn fits(&self, b: Budget) -> bool {
        self.content_utf16 <= b.limit && self.common_bytes <= b.max_bytes
    }
}
impl From<&Value> for Size {
    fn from(v: &Value) -> Self {
        Self {
            content_utf16: v["usage"]["contentUtf16"].as_u64().unwrap() as usize,
            common_bytes: crate::transport::common_bytes(v, false),
        }
    }
}
/// 最长完整前缀选择的二分版本。**只有 `paging::page()` 能用它**，
/// 因为单调性是那条路径的结构性质，不是所有读取的通性；`find` 用
/// [`longest_prefix_linear`]。
///
/// `page()` 的候选字节随 `end` 单调不减，来自三条结构事实：
/// 1. 页级元数据（`diagnostics`、`unrequestedFlows`、`caret` 等）只挂在首个单位上
///    （`paging::text_units` 末尾的 `units.first_mut()`），多收一个单位不会新增元数据键；
/// 2. 单位的 UTF-16 区间首尾相接、互不重叠，所以多收一个单位只会往
///    `content` / `anchors` / `omitted.page` 里追加，不会替换已有内容；
/// 3. 游标 position 只含 `unit` / `offset` 两个单调递增的整数与一个对象名，
///    十进制位数不减，因而 `nextCursor` 的长度不减。
///
/// 唯一的例外是末页：`last_is_cursorless` 为真时 `build(last)` 不带 `nextCursor`，
/// 字节可能不升反降，所以把它排除在单调区间外单独评估。
///
/// 选择算法：先求 `first` 的 `Size`（决定 `too_small` 的 `minLimit`/`minBytes`），
/// 再在单调区间上二分找最后一个 fit，最后补评末页。评估次数从 O(n) 降到
/// O(log n)，且结果与线性扫描逐字节相同（`agent_06_prefix_selection_*` 守门）。
/// `size_of` 允许重复调用（同一 `end` 结果必须一致）；`build` 只为最终选中页调用。
pub fn longest_prefix<T>(
    first: usize,
    last: usize,
    last_is_cursorless: bool,
    b: Budget,
    object: Value,
    mut size_of: impl FnMut(usize) -> Size,
    mut build: impl FnMut(usize) -> (Value, T),
) -> Result<(Value, T)> {
    let first_size = size_of(first);
    if !first_size.fits(b) {
        // first 是最小候选，单调区间内不会再有 fit；只有末页可能因省游标而 fit。
        if last_is_cursorless && last > first && size_of(last).fits(b) {
            return Ok(build(last));
        }
        return Err(too_small_size(first_size, object));
    }
    let mono_hi = if last_is_cursorless { last.saturating_sub(1) } else { last };
    let mut best_end = first;
    if mono_hi > first {
        let mut lo = first; // 不变式：fits(lo) 为真，谓词在此区间单调不增
        let mut hi = mono_hi + 1; // 排他上界
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if size_of(mid).fits(b) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        best_end = lo;
    }
    if last_is_cursorless && last > best_end && size_of(last).fits(b) {
        best_end = last;
    }
    Ok(build(best_end))
}
/// 最长完整前缀选择的线性版本：逐个候选评估，取最后一个 fit。
///
/// `find` 用这一版。它的游标 position 是 `search::Position { flow, at, last_end }`：
/// `at` 跨流会重置回小值、`last_end` 会从数字变成 `null`，所以文件模式下多收一条
/// 命中反而可能让 `nextCursor` 变短——[`longest_prefix`] 的第 3 条结构事实在这里
/// 不成立，不能二分。行数由 `maxHits`（≤ 1000，默认 20）封顶，线性代价可忽略。
///
/// 语义与二分版一致：`too_small` 取第一个不 fit 的候选（即 `first`，因为它是最小
/// 候选）；`contentUtf16` 一旦超过 `limit` 就提前停（它在任何路径上都单调不减）。
pub fn longest_prefix_linear<T>(
    first: usize,
    last: usize,
    b: Budget,
    object: Value,
    mut size_of: impl FnMut(usize) -> Size,
    mut build: impl FnMut(usize) -> (Value, T),
) -> Result<(Value, T)> {
    let mut best = None;
    let mut failure = None;
    for end in first..=last {
        let size = size_of(end);
        if size.fits(b) {
            best = Some(end);
        } else {
            if failure.is_none() {
                failure = Some(too_small_size(size, object.clone()));
            }
            if size.content_utf16 > b.limit {
                break;
            }
        }
    }
    let end =
        best.ok_or_else(|| failure.unwrap_or_else(|| error("AGENT_BAD_CURSOR", "没有可返回单位")))?;
    Ok(build(end))
}
pub fn too_small(v: &Value, object: Value) -> crate::QueryError {
    too_small_size(Size::from(v), object)
}
/// `too_small` 的尺寸版本：错误细节只依赖 `Size`，不需要候选页本身。
pub fn too_small_size(size: Size, object: Value) -> crate::QueryError {
    let mut e = error("AGENT_BUDGET_TOO_SMALL", "完整记录无法容纳；未返回内容、未消费游标");
    e.details = json!({"object":object,"minLimit":size.content_utf16,"minBytes":size.common_bytes});
    if size.content_utf16 > 1048576 || size.common_bytes > 4194304 {
        e.code = "AGENT_UNIT_TOO_LARGE".into();
    }
    e
}
