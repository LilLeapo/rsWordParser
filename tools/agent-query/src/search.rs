//! AGENT-04：固定 Unicode 数据，单调源区间映射；仅在可终止 worker 内调用 run。
use crate::{Result, error};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use unicode_normalization::{
    UnicodeNormalization,
    char::{canonical_combining_class, compose, decompose_compatible},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Literal,
    Regex,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub mode: Mode,
    pub insensitive: bool,
    pub fold_width: bool,
    pub collapse_whitespace: bool,
    pub deadline_ms: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Literal,
            insensitive: false,
            fold_width: false,
            collapse_whitespace: false,
            deadline_ms: 250,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputFlow {
    pub part: u32,
    pub flow: u32,
    pub start: u32,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub pattern: String,
    pub options: Options,
    pub flows: Vec<InputFlow>,
    pub max_hits: usize,
    pub position: Position,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub flow: usize,
    pub at: usize,
    pub last_end: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub hits: Vec<Hit>,
    pub next: Option<Position>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub part: u32,
    pub flow: u32,
    pub range: Range<u32>,
    pub normalized_match: String,
    pub original: String,
    pub complete: bool,
    pub next: Position,
}
#[derive(Debug, Clone)]
struct Mapping {
    normalized: Range<usize>,
    source: Range<u32>,
}
#[derive(Debug, Clone)]
struct Normalized {
    text: String,
    map: Vec<Mapping>,
    end: u32,
}
// White_Space 属性显式列出；不使用随 Rust 工具链版本变化的 char::is_whitespace。
fn whitespace(c: char) -> bool {
    matches!(c,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{0085}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}')
}
fn normalize(input: &str, opts: &Options) -> Normalized {
    let mut pieces: Vec<(String, Range<u32>)> = vec![];
    let mut offset = 0;
    for c in input.chars() {
        let end = offset + c.len_utf16() as u32;
        let mut width = String::new();
        if opts.fold_width && (c == '\u{3000}' || ('\u{ff00}'..='\u{ffef}').contains(&c)) {
            decompose_compatible(c, |x| width.push(x));
        } else {
            width.push(c);
        }
        if opts.fold_width
            && let Some((last, range)) = pieces.last_mut()
        {
            let first = width.chars().next().unwrap();
            if canonical_combining_class(first) != 0
                || last.chars().last().and_then(|a| compose(a, first)).is_some()
            {
                last.push_str(&width);
                range.end = end;
                offset = end;
                continue;
            }
        }
        pieces.push((width, offset..end));
        offset = end;
    }
    let mut out = Normalized { text: String::new(), map: vec![], end: offset };
    for (piece, source) in pieces {
        let s = if opts.fold_width { piece.nfc().collect::<String>() } else { piece };
        for c in s.chars() {
            if opts.collapse_whitespace && whitespace(c) {
                if out.text.ends_with(' ')
                    && out.map.last().is_some_and(|m| m.source.end == source.start)
                {
                    out.map.last_mut().unwrap().source.end = source.end;
                    continue;
                }
                let start = out.text.len();
                out.text.push(' ');
                out.map.push(Mapping { normalized: start..out.text.len(), source: source.clone() });
            } else {
                let start = out.text.len();
                out.text.push(c);
                out.map.push(Mapping { normalized: start..out.text.len(), source: source.clone() });
            }
        }
    }
    out
}
impl Normalized {
    fn source(&self, r: Range<usize>) -> (Range<u32>, bool) {
        if r.is_empty() {
            if r.start == self.text.len() {
                return (self.end..self.end, true);
            }
            let i = self.map.partition_point(|m| m.normalized.end <= r.start);
            let m = &self.map[i];
            let complete = m.normalized.start == r.start
                && (i == 0 || self.map[i - 1].source.end <= m.source.start);
            return (m.source.start..m.source.start, complete);
        }
        let a = self.map.partition_point(|m| m.normalized.end <= r.start);
        let b = self.map.partition_point(|m| m.normalized.start < r.end) - 1;
        let complete = (a == 0 || self.map[a - 1].source.end <= self.map[a].source.start)
            && (b + 1 == self.map.len() || self.map[b + 1].source.start >= self.map[b].source.end);
        (self.map[a].source.start..self.map[b].source.end, complete)
    }
}
fn byte(s: &str, n: u32) -> usize {
    let mut off = 0;
    for (i, c) in s.char_indices() {
        if off == n {
            return i;
        }
        off += c.len_utf16() as u32;
    }
    s.len()
}
/// worker-only：外层执行器负责杀死超时进程；不把有限输入误当作 deadline。
pub fn run(request: &Request) -> Result<Batch> {
    let o = &request.options;
    if !(1..=1000).contains(&request.max_hits) {
        return Err(error("BIND_BAD_ARGUMENT", "maxHits 应在 1..=1000 内"));
    }
    if request.pattern.len() > 16384 || request.pattern.encode_utf16().count() > 4096 {
        return Err(error("AGENT_QUERY_TOO_LARGE", "pattern 超限"));
    }
    if !(1..=2000).contains(&o.deadline_ms) {
        return Err(error("BIND_BAD_ARGUMENT", "deadlineMs 应在 1..=2000 内"));
    }
    if o.mode == Mode::Literal && request.pattern.is_empty() {
        return Err(error("BIND_BAD_ARGUMENT", "literal pattern 不能为空"));
    }
    assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0), "Unicode 数据版本漂移");
    let normalized: Vec<_> = request.flows.iter().map(|f| normalize(&f.text, o)).collect();
    if normalized.iter().map(|n| n.text.len()).sum::<usize>() > 1048576 {
        return Err(error(
            "AGENT_QUERY_TOO_LARGE",
            "授权 scope 归一后超过 1048576 B；请显式缩小范围",
        ));
    }
    let pattern = if o.mode == Mode::Literal {
        regex::escape(&normalize(&request.pattern, o).text)
    } else {
        request.pattern.clone()
    };
    let re = RegexBuilder::new(&pattern)
        .case_insensitive(o.insensitive)
        .size_limit(2 * 1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .build()
        .map_err(|e| error("AGENT_BAD_PATTERN", e.to_string()))?;
    let mut out = vec![];
    for (index, (flow, n)) in
        request.flows.iter().zip(normalized).enumerate().skip(request.position.flow)
    {
        let mut at = if index == request.position.flow { request.position.at } else { 0 };
        let mut last_end =
            if index == request.position.flow { request.position.last_end } else { None };
        if at > n.text.len() || !n.text.is_char_boundary(at) {
            return Err(error("AGENT_BAD_CURSOR", "搜索位置越界"));
        }
        // find_at 始终接收完整字符串，不能把后缀当作新的 ^/词边界上下文。
        while let Some(hit) = re.find_at(&n.text, at) {
            if hit.is_empty() && last_end == Some(hit.end()) {
                if hit.end() == n.text.len() {
                    break;
                }
                at = hit.end() + n.text[hit.end()..].chars().next().unwrap().len_utf8();
                continue;
            }
            let next = if hit.is_empty() {
                if hit.end() == n.text.len() {
                    Position { flow: index + 1, at: 0, last_end: None }
                } else {
                    Position {
                        flow: index,
                        at: hit.end() + n.text[hit.end()..].chars().next().unwrap().len_utf8(),
                        last_end: Some(hit.end()),
                    }
                }
            } else {
                Position { flow: index, at: hit.end(), last_end: Some(hit.end()) }
            };
            let (source, complete) = n.source(hit.range());
            let original =
                flow.text[byte(&flow.text, source.start)..byte(&flow.text, source.end)].into();
            out.push(Hit {
                part: flow.part,
                flow: flow.flow,
                range: flow.start + source.start..flow.start + source.end,
                normalized_match: hit.as_str().into(),
                original,
                complete,
                next: next.clone(),
            });
            if out.len() == request.max_hits {
                return Ok(Batch { hits: out, next: Some(next) });
            }
            if next.flow != index {
                break;
            }
            at = next.at;
            last_end = next.last_end;
        }
    }
    Ok(Batch { hits: out, next: None })
}
static NEXT: AtomicU64 = AtomicU64::new(1);
/// 预启动的单次工作单元。初始化不执行查询，查询 deadline 从 submit 开始。
/// 任何出口都通过 Drop 终止并 wait；调用方不能遗留后台计算。
pub struct Worker {
    child: std::process::Child,
    request: PathBuf,
    response: PathBuf,
    ready: PathBuf,
    pending: PathBuf,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for p in [&self.request, &self.response, &self.ready, &self.pending] {
            let _ = std::fs::remove_file(p);
        }
    }
}
impl Worker {
    /// 执行器初始化，独立 2 s 上限；此时没有传入用户 pattern 或文本。
    pub fn start(program: &Path, scratch: &Path) -> Result<Self> {
        let start = Instant::now();
        std::fs::create_dir_all(scratch)
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("query-{}-{id}", std::process::id());
        let request = scratch.join(format!("{name}.request"));
        let response = scratch.join(format!("{name}.response"));
        let ready = scratch.join(format!("{name}.ready"));
        let pending = scratch.join(format!("{name}.pending"));
        let child = Command::new(program)
            .arg(&request)
            .arg(&response)
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        let mut worker = Self { child, request, response, ready, pending };
        while !worker.ready.exists() {
            if start.elapsed() >= Duration::from_secs(2) {
                return Err(error("AGENT_WORKER_FAILED", "worker 初始化超过 2 s；未开始查询"));
            }
            if worker
                .child
                .try_wait()
                .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?
                .is_some()
            {
                return Err(error("AGENT_WORKER_FAILED", "worker 初始化失败"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(worker)
    }
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    /// 包含请求写入、归一化、编译、匹配、响应读取；返回前销毁工作单元。
    pub fn submit(mut self, request: &Request) -> Result<Batch> {
        let start = Instant::now();
        let ms = request.options.deadline_ms;
        if !(1..=2000).contains(&ms) {
            return Err(error("BIND_BAD_ARGUMENT", "deadlineMs 应在 1..=2000 内"));
        }
        let deadline = Duration::from_millis(ms);
        // ready 出现不代表子进程已写完；单独暂存请求，不能把仍打开的握手 inode 改名。
        std::fs::write(&self.pending, serde_json::to_vec(request).unwrap())
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        std::fs::rename(&self.pending, &self.request)
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        loop {
            if start.elapsed() >= deadline {
                return Err(error("AGENT_QUERY_TIMEOUT", "查询超时，worker 已终止并回收"));
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return Err(error("AGENT_WORKER_FAILED", "worker 异常退出"));
                    }
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                Err(e) => return Err(error("AGENT_WORKER_FAILED", e.to_string())),
            }
        }
        let bytes = std::fs::read(&self.response)
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        let result = serde_json::from_slice(&bytes)
            .map_err(|e| error("AGENT_WORKER_FAILED", e.to_string()))?;
        if start.elapsed() >= deadline {
            return Err(error("AGENT_QUERY_TIMEOUT", "响应读取超时"));
        }
        result
    }
}
