//! AGENT-06 / docs/21 WP1-A：候选页的 u8 片段账本。
//!
//! `paging::page()` 的前缀选择要为约 12 个候选 `end` 求 `budget::Size`。旧路径每个
//! 探针都重建 `Value` 并整包序列化约 20 次（`envelope` 与 `response` 各一段
//! `measure` 定点，加上 `common_bytes` 里 Text / Structured 两形态各自的定点），
//! large-report 的 text 因此是 13.8 ms，而单位少得多的 outline 只要 3.6 ms。
//!
//! 这里把它拆成两层：每次 `page()` 调用把与 `end` 无关的部分序列化一次
//! （[`Skeleton`]），每个候选单位的正文 / 锚点 / 省略项各留一份 serde_json 产出的
//! compact 字节片段；探针只做 memcpy 拼装与整数运算。拼出来的是**精确字节**，
//! 所以可以整段与 `serde_json::to_vec(&paging::response(..))` 相等断言——调试构建
//! 下每个探针都查（`paging::page`），另有一条全语料逐字节守门测试。
//!
//! 硬规则：**本模块不自己实现 JSON 转义**。所有片段都由 serde_json 产出，这里只做
//! 拼接、计数与 `usage` 定点的算术复现。唯一"读"转义规则的地方是
//! [`escape_extra`]，它只数字节、不写字节，且只在构造片段时跑一次。
use crate::{budget::Size, paging::Unit, transport::Shape};
use serde_json::{Value, json};
use std::sync::OnceLock;

/// `usage` 定点的迭代上限。真实 `measure` 是无上限循环，正常输入两三轮就收敛；
/// 超限说明账本与真实路径的假设已经不符，调用方回退到旧的整包序列化路径。
const FIXPOINT_CAP: usize = 16;

/// serde_json 把一个字节写进 JSON 字符串时新增的字节数。
///
/// 规则取自 serde_json 的 `ESCAPE` 表：`"` / `\` 与 0x08 / 0x09 / 0x0A / 0x0C / 0x0D
/// 写成两字节转义（+1），其余 < 0x20 写成 `\u00XX`（+5），>= 0x20 原样（0）。
/// 非 ASCII 字节不转义，所以按字节统计与按字符统计等价。
const ESCAPE_EXTRA: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 0x20 {
        table[i] = 5;
        i += 1;
    }
    table[0x08] = 1;
    table[0x09] = 1;
    table[0x0A] = 1;
    table[0x0C] = 1;
    table[0x0D] = 1;
    table[0x22] = 1;
    table[0x5C] = 1;
    table
};

/// 这段字节被 serde_json 嵌进 JSON 字符串时会多出多少字节（Text 形态的二次转义）。
fn escape_extra(bytes: &[u8]) -> usize {
    bytes.iter().map(|&b| ESCAPE_EXTRA[b as usize] as usize).sum()
}

/// 一段已由 serde_json 产出的 compact JSON 片段，附带它的二次转义增量。
#[derive(Clone, Debug, Default)]
struct Frag {
    bytes: Vec<u8>,
    escape_extra: usize,
}
impl Frag {
    fn new(bytes: Vec<u8>) -> Self {
        let escape_extra = escape_extra(&bytes);
        Self { bytes, escape_extra }
    }
    fn of(v: &Value) -> Self {
        Self::new(serde_json::to_vec(v).unwrap())
    }
    /// 片段的 UTF-16 单元数。serde_json 默认不转义非 ASCII，所以片段可能含多字节序列。
    fn utf16(&self) -> usize {
        std::str::from_utf8(&self.bytes).unwrap().encode_utf16().count()
    }
}

/// 拼装缓冲：一边写字节，一边累计 Text 形态的二次转义增量。
struct Buf {
    bytes: Vec<u8>,
    escape_extra: usize,
}
impl Buf {
    fn with_capacity(n: usize) -> Self {
        Self { bytes: Vec::with_capacity(n), escape_extra: 0 }
    }
    /// 骨架字面量（键名、括号、逗号）。都很短，逐探针重数的代价可忽略。
    fn lit(&mut self, s: &str) {
        self.escape_extra += escape_extra(s.as_bytes());
        self.bytes.extend_from_slice(s.as_bytes());
    }
    fn frag(&mut self, f: &Frag) {
        self.escape_extra += f.escape_extra;
        self.bytes.extend_from_slice(&f.bytes);
    }
    /// 十进制整数：不含任何可转义字节。
    fn num(&mut self, n: u64) {
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        let mut n = n;
        loop {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.bytes.extend_from_slice(&buf[i..]);
    }
    fn bool(&mut self, b: bool) {
        self.lit(if b { "true" } else { "false" });
    }
}

fn digits(n: usize) -> usize {
    if n == 0 { 1 } else { n.ilog10() as usize + 1 }
}

/// `measure` / `Shape::result` 的定点：`size(rb, et) = k + digits(rb) + digits(et)`。
///
/// 复现真实循环的顺序：先按当前 `(rb, et)` 算长度，命中即停，否则写回再算。起点不同
/// 可能落到不同的不动点（数字位数是阶跃的），所以三段必须按真实顺序串起来跑。
fn fixpoint(k: usize, start: (usize, usize)) -> Option<(usize, usize)> {
    let (mut rb, mut et) = start;
    for _ in 0..FIXPOINT_CAP {
        let size = k + digits(rb) + digits(et);
        let tokens = size.div_ceil(4);
        if rb == size && et == tokens {
            return Some((rb, et));
        }
        rb = size;
        et = tokens;
    }
    None
}

/// Text / Structured 外壳的常量长度：用一个不含可转义字符的占位内层 wrap 一次再减去
/// 占位长度得到，不手写。`json!(0)` 序列化为 `0`：长度 1、无引号、无转义。
fn shells() -> (usize, usize) {
    static SHELLS: OnceLock<(usize, usize)> = OnceLock::new();
    *SHELLS.get_or_init(|| {
        let probe = json!(0);
        let text = Shape::Text.wrap(&probe, false).to_string().len() - 1;
        let structured = Shape::Structured.wrap(&probe, false).to_string().len() - 1;
        // 第二个占位件校验"外壳与内层长度线性可分"这一前提。
        let probe2 = json!(12345);
        debug_assert_eq!(Shape::Text.wrap(&probe2, false).to_string().len() - 5, text);
        debug_assert_eq!(Shape::Structured.wrap(&probe2, false).to_string().len() - 5, structured);
        (text, structured)
    })
}

/// `envelope()` 里 `anchorCounts` 的默认值，逐字节照抄它的 `json!` 字面量经 BTreeMap
/// 排序后的形态；`response()` 只在有锚点时整体替换它。
const DEFAULT_ANCHOR_COUNTS: &str = "\"anchorCounts\":{\"presentationScalars\":0,\"presentationUtf16\":0,\"scope\":\"notApplicable\",\"sourceScalars\":0,\"sourceUtf16\":0}";

/// 单位的一条省略项，拆成"计数前"与"计数后"两段常量片段。
struct Omit {
    category: String,
    /// `{"category":<cat>,"count":`
    prefix: Frag,
    /// `,"reason":<reason>}`
    suffix: Frag,
    count: u64,
}

/// 单个候选单位的账本。
struct Row {
    /// text 页：`content` 字符串序列化后**去掉两侧引号**的字节；记录页：整条 compact JSON。
    body: Frag,
    /// text 页：原串的 UTF-16 单元数；记录页：`body` 片段的 UTF-16 单元数。
    body_utf16: usize,
    anchors: Vec<Frag>,
    counts: [u32; 4],
    omitted: Vec<Omit>,
    projected: bool,
    /// 非空 metadata 的键值片段；`response()` 按单位顺序逐键覆盖写入信封。
    metadata: Vec<(String, Frag)>,
}

/// 与 `end` 无关的页骨架：每次 `page()` 调用构造一次。
pub struct Skeleton {
    text: bool,
    start: usize,
    /// 全部单位数；`more`/`truncated` 由调用方给，这里只用来判断 `projectionKey`。
    snapshot: Frag,
    range: Frag,
    /// `anchors.projectionKey`：`response()` 取页内首个单位的值，即 `units[start]`。
    /// 空页（`end == start`）时 `units.first()` 为 None，写 `null`。
    projection_key: Frag,
    rows: Vec<Row>,
}

/// 一个候选页的拼装结果。
pub struct Assembled {
    /// 与 `serde_json::to_vec(&paging::response(..))` 逐字节相同。
    pub bytes: Vec<u8>,
    pub size: Size,
}

impl Skeleton {
    /// 为 `units[start..last]` 建账。`last` 取 `page()` 的候选上界，超出的单位不进任何候选页。
    ///
    /// 片段在这里算而不是在 `Unit` 构造时算：context 读取在 `text_units` 之后整体替换
    /// `unit.content`（`session.rs` 的 `ReadTool::Context` 分支），构造时算好的片段会失效。
    /// 每个 `units` 只服务一次 `page()`，所以两种时机的总代价相同。
    pub fn new(
        snapshot: &str,
        units: &[Unit],
        start: usize,
        last: usize,
        text: bool,
        range: &Value,
    ) -> Self {
        // 与 `envelope()` 一致：能解析成 JSON 的 snapshot 串换成解析后的值再序列化。
        let snapshot_value =
            serde_json::from_str::<Value>(snapshot).unwrap_or_else(|_| json!(snapshot));
        let rows = units[start..last]
            .iter()
            .map(|u| {
                let (body, body_utf16) = if text {
                    let s = u.content.as_str().expect("text 页的单位正文必须是字符串");
                    let quoted = serde_json::to_vec(&u.content).unwrap();
                    debug_assert!(quoted.len() >= 2 && quoted[0] == b'"');
                    (Frag::new(quoted[1..quoted.len() - 1].to_vec()), s.encode_utf16().count())
                } else {
                    let f = Frag::of(&u.content);
                    let n = f.utf16();
                    (f, n)
                };
                Row {
                    body,
                    body_utf16,
                    anchors: u.anchors.iter().map(Frag::of).collect(),
                    counts: u.counts,
                    omitted: u
                        .omitted
                        .iter()
                        .map(|o| {
                            let mut prefix = b"{\"category\":".to_vec();
                            prefix.extend_from_slice(&serde_json::to_vec(&o["category"]).unwrap());
                            prefix.extend_from_slice(b",\"count\":");
                            let mut suffix = b",\"reason\":".to_vec();
                            suffix.extend_from_slice(&serde_json::to_vec(&o["reason"]).unwrap());
                            suffix.push(b'}');
                            Omit {
                                category: o["category"].as_str().unwrap().to_owned(),
                                prefix: Frag::new(prefix),
                                suffix: Frag::new(suffix),
                                count: o["count"].as_u64().unwrap(),
                            }
                        })
                        .collect(),
                    projected: u.projection_key.is_some(),
                    metadata: u
                        .metadata
                        .as_object()
                        .expect("metadata 必须是对象")
                        .iter()
                        .map(|(k, v)| (k.clone(), Frag::of(v)))
                        .collect(),
                }
            })
            .collect();
        Self {
            text,
            start,
            snapshot: Frag::of(&snapshot_value),
            range: Frag::of(range),
            projection_key: Frag::of(&json!(
                units.get(start).and_then(|u| u.projection_key.as_deref())
            )),
            rows,
        }
    }

    /// `units[start..end]` 里最后一个写了 `key` 的 metadata 值；与 `response()` 的
    /// 「按单位顺序逐键覆盖」等价。
    fn meta(&self, key: &str, end: usize) -> Option<&Frag> {
        self.rows[..end - self.start]
            .iter()
            .rev()
            .find_map(|r| r.metadata.iter().find(|(k, _)| k == key).map(|(_, f)| f))
    }

    /// 拼出与 `paging::response(snapshot, &units[start..end], text, range, more, cursor)`
    /// 逐字节相同的 compact JSON，并给出它的 `budget::Size`。
    ///
    /// `None` 表示 `usage` 定点在 [`FIXPOINT_CAP`] 轮内没收敛，调用方应回退到整包序列化。
    pub fn assemble(&self, end: usize, more: bool, cursor: Option<&str>) -> Option<Assembled> {
        let rows = &self.rows[..end - self.start];
        let anchors_key = self.text || rows.iter().any(|r| r.projected);
        // 预估：正文 + 锚点 + 骨架，避免拼装途中反复扩容。
        let cap = rows.iter().map(|r| r.body.bytes.len() + 64).sum::<usize>()
            + rows.iter().flat_map(|r| &r.anchors).map(|f| f.bytes.len() + 1).sum::<usize>()
            + self.snapshot.bytes.len() * 2
            + self.range.bytes.len()
            + 512;
        let mut b = Buf::with_capacity(cap);
        // 完整对象比"信封态"（envelope 刚构造、response 还没加锚点/元数据/page）多出的字节。
        // 带符号：全零计数的 `anchorCounts` 换掉默认值时会更短（`"page"` 比
        // `"notApplicable"` 少 9 字节），信封态反而比完整对象长。
        let mut env_extra = 0isize;
        b.lit("{");

        // ---- anchorCounts ----
        if anchors_key {
            let mut counts = [0u64; 4];
            for r in rows {
                for (sum, n) in counts.iter_mut().zip(r.counts) {
                    *sum += u64::from(n);
                }
            }
            let at = b.bytes.len();
            b.lit("\"anchorCounts\":{\"presentationScalars\":");
            b.num(counts[3]);
            b.lit(",\"presentationUtf16\":");
            b.num(counts[1]);
            b.lit(",\"scope\":\"page\",\"sourceScalars\":");
            b.num(counts[2]);
            b.lit(",\"sourceUtf16\":");
            b.num(counts[0]);
            b.lit("}");
            env_extra += (b.bytes.len() - at) as isize - DEFAULT_ANCHOR_COUNTS.len() as isize;
        } else {
            b.lit(DEFAULT_ANCHOR_COUNTS);
        }

        // ---- anchors ----
        if anchors_key {
            let at = b.bytes.len();
            b.lit(",\"anchors\":{\"projectionKey\":");
            if rows.is_empty() {
                b.lit("null");
            } else {
                b.frag(&self.projection_key);
            }
            b.lit(",\"segments\":[");
            let mut first = true;
            for f in rows.iter().flat_map(|r| &r.anchors) {
                if !first {
                    b.lit(",");
                }
                first = false;
                b.frag(f);
            }
            b.lit("],\"snapshot\":");
            b.frag(&self.snapshot);
            b.lit("}");
            env_extra += (b.bytes.len() - at) as isize;
        }

        // ---- content ----
        b.lit(",\"content\":");
        let content_utf16;
        let empty;
        if self.text {
            b.lit("\"");
            let mut utf16 = 0usize;
            let mut bytes = 0usize;
            for r in rows {
                b.frag(&r.body);
                utf16 += r.body_utf16;
                bytes += r.body.bytes.len();
            }
            b.lit("\"");
            content_utf16 = utf16;
            empty = bytes == 0;
        } else {
            b.lit("[");
            let mut utf16 = 2usize;
            for (i, r) in rows.iter().enumerate() {
                if i > 0 {
                    b.lit(",");
                    utf16 += 1;
                }
                b.frag(&r.body);
                utf16 += r.body_utf16;
            }
            b.lit("]");
            content_utf16 = utf16;
            empty = rows.is_empty();
        }

        // ---- diagnostics ----
        if let Some(f) = self.meta("diagnostics", end) {
            let at = b.bytes.len();
            b.lit(",\"diagnostics\":");
            b.frag(f);
            env_extra += (b.bytes.len() - at) as isize;
        }

        b.lit(",\"empty\":");
        b.bool(empty);
        b.lit(",\"nextCursor\":");
        b.frag(&Frag::of(&json!(cursor)));

        // ---- omitted ----（键序：addressableNotProjected < caret < complete < page < unrequestedFlows）
        b.lit(",\"omitted\":{");
        for key in ["addressableNotProjected", "caret"] {
            if let Some(f) = self.meta(key, end) {
                let at = b.bytes.len();
                b.lit("\"");
                b.lit(key);
                b.lit("\":");
                b.frag(f);
                b.lit(",");
                env_extra += (b.bytes.len() - at) as isize;
            }
        }
        b.lit("\"complete\":");
        b.bool(!more);
        b.lit(",\"page\":[");
        {
            // 与 response() 的 BTreeMap 聚合一致：按分类累加计数，reason 取首次出现的那条。
            let mut page: std::collections::BTreeMap<&str, (u64, &Omit)> =
                std::collections::BTreeMap::new();
            for o in rows.iter().flat_map(|r| &r.omitted) {
                let e = page.entry(o.category.as_str()).or_insert((0, o));
                e.0 += o.count;
            }
            let at = b.bytes.len();
            for (i, (_, (count, o))) in page.iter().enumerate() {
                if i > 0 {
                    b.lit(",");
                }
                b.frag(&o.prefix);
                b.num(*count);
                b.frag(&o.suffix);
            }
            env_extra += (b.bytes.len() - at) as isize;
        }
        b.lit("]");
        if let Some(f) = self.meta("unrequestedFlows", end) {
            let at = b.bytes.len();
            b.lit(",\"unrequestedFlows\":");
            b.frag(f);
            env_extra += (b.bytes.len() - at) as isize;
        }
        b.lit("}");

        b.lit(",\"range\":");
        b.frag(&self.range);
        b.lit(",\"snapshot\":");
        b.frag(&self.snapshot);
        b.lit(",\"truncated\":");
        b.bool(more);

        // ---- usage ----：两个数字之外全是常量，先算不动点再一次写出。
        let head = b.bytes.len();
        let tail_const = ",\"usage\":{\"contentUtf16\":".len()
            + digits(content_utf16)
            + ",\"estimatedTokens\":".len()
            + ",\"responseBytes\":".len()
            + "}}".len();
        // 尾巴里的 8 个引号（4 个键名）都要计入 Text 的二次转义；数字不转义。
        let escape_total = b.escape_extra + 8;
        let base = head + tail_const;
        let base_env = usize::try_from(base as isize - env_extra).ok()?;

        // 第一段：envelope() 在无锚点/无元数据/page 为空的信封上从 (0,0) 迭代。
        let env = fixpoint(base_env, (0, 0))?;
        // 第二段：response() 加上锚点等之后，从第一段的结果继续迭代。
        let (rb, et) = fixpoint(base, env)?;
        // 第三段：两形态各自的定点（transport::Shape::result），取较大者作共同上界。
        let (text_shell, structured_shell) = shells();
        let (text_rb, _) = fixpoint(text_shell + base + escape_total, (rb, et))?;
        let (structured_rb, _) = fixpoint(structured_shell + base, (rb, et))?;

        b.lit(",\"usage\":{\"contentUtf16\":");
        b.num(content_utf16 as u64);
        b.lit(",\"estimatedTokens\":");
        b.num(et as u64);
        b.lit(",\"responseBytes\":");
        b.num(rb as u64);
        b.lit("}}");
        debug_assert_eq!(b.bytes.len(), base + digits(rb) + digits(et));

        Some(Assembled {
            bytes: b.bytes,
            size: Size { content_utf16, common_bytes: text_rb.max(structured_rb) },
        })
    }
}
