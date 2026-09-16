#!/usr/bin/env node
// docs/21 WP1-2 D-3：两个 rsword-mcp 二进制逐字节对照 JSON-RPC 响应行。
//
//   node tools/perf/compare-mcp-bytes.mjs <baseline-bin> <new-bin> [选项]
//     --corpus DIR   语料根目录（默认：本仓库的 corpus/）
//     --docs N       只取前 N 份（默认全部）
//     --pages N      跟随 nextCursor 的最大页数（默认 3）
//     --verbose      打印每条差异的前 400 字节
//
// CLI 那条对照（compare-cli-bytes.mjs）跑不到 `transport::Shape`：文件模式不包 MCP
// 外壳。Text / Structured 两形态的 `usage` 定点与二次转义只在这里才被真正比较。
//
// 两个 MCP 进程各自常驻，按同样的顺序收同样的请求。`snapshot.sessionId` 掩成定长
// 占位；MCP 的 sessionId 由服务端自增（`a{pid}-{nonce}-{id}`），pid 位数不同会改
// responseBytes，所以同样要求两侧长度一致。
import { spawn } from "node:child_process";
import { readdir, stat } from "node:fs/promises";
import { createInterface } from "node:readline";
import path from "node:path";
import process from "node:process";

const args = process.argv.slice(2);
const positional = args.filter((a) => !a.startsWith("--"));
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] !== undefined ? args[i + 1] : fallback;
};
const [baseBin, newBin] = positional;
if (!baseBin || !newBin) {
  console.error("用法: compare-mcp-bytes.mjs <baseline-bin> <new-bin> [--corpus DIR] [--docs N]");
  process.exit(2);
}
const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const corpusRoot = path.resolve(flag("corpus", path.join(repoRoot, "corpus")));
const maxDocs = Number(flag("docs", "0"));
const maxPages = Number(flag("pages", "3"));
const verbose = args.includes("--verbose");

async function docxPaths(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await docxPaths(full)));
    else if (entry.name.endsWith(".docx")) out.push(full);
  }
  return out.sort();
}

/** 常驻一个 MCP 进程，按行收发；返回未解析的响应行（就是要逐字节比的东西）。 */
function server(bin) {
  const child = spawn(bin, [], { cwd: repoRoot, stdio: ["pipe", "pipe", "ignore"] });
  const pending = [];
  createInterface({ input: child.stdout }).on("line", (line) => pending.shift()?.(line));
  let seq = 0;
  const rpc = (method, params) =>
    new Promise((done, fail) => {
      const timer = setTimeout(() => fail(new Error(`${bin} 超时: ${method}`)), 30000);
      pending.push((line) => {
        clearTimeout(timer);
        done(line);
      });
      child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: ++seq, method, params }) + "\n");
    });
  const notify = (method) =>
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method }) + "\n");
  return { child, rpc, notify };
}

/// 会话标识的字面形状：`a{pid}-{nonce}-s{n}`。按**值**匹配而不是按 `"sessionId":`
/// 键匹配：MCP 的 Text 形态会把整个内层再转义一遍，键在响应里长成 `\"sessionId\":`，
/// 甚至两层（锚点里的 snapshot），按键匹配会漏掉。nonce 是纳秒时间戳，语料正文里
/// 不可能出现这种形状。
const SESSION = /a\d{1,7}-\d{15,22}-s\d{1,6}/g;
/// `a1.` 游标。会话模式的载荷只有一个自增句柄（`{:016x}`，定长），它取自全局计数器，
/// 消耗多少个取决于前缀选择评估了多少个候选——换算法就会变，且 `docs/16` 明确它是
/// **不透明凭据**，`tools/ci/check-agent-transports.mjs` 也把 `/nextCursor` 列为允许
/// 差异。所以这里把 handle 归一掉，但把**其余载荷**留着比：文件模式的游标带
/// binding / position，一个字节都不许变，归一不会碰它。
const CURSOR = /a1\.(?:[0-9a-f]{2})+/g;
function normalizeCursor(token) {
  try {
    const wire = JSON.parse(Buffer.from(token.slice(3), "hex").toString("utf8"));
    if (wire.kind !== "session") return token;
    delete wire.handle;
    return `<cursor:session:${token.length}:${JSON.stringify(wire)}>`;
  } catch {
    return token;
  }
}
/** 掩掉会话标识与会话句柄，返回 [归一后的文本, 出现过的各标识长度]。 */
function mask(text) {
  const lengths = [];
  const masked = text
    .replace(SESSION, (id) => {
      lengths.push(id.length);
      return "<session>";
    })
    .replace(CURSOR, normalizeCursor);
  return [masked, lengths];
}
/** 响应行里唯一允许不同的就是 sessionId 本身；id 序号两侧同步递增，不必掩。 */
function body(line) {
  const r = JSON.parse(line).result;
  return r.structuredContent ?? JSON.parse(r.content[0].text);
}

const shapes = ["text", "structured"];
function calls(sessionId, pattern) {
  const out = [];
  for (const resultShape of shapes) {
    const common = { sessionId, resultShape };
    out.push(["text", { ...common, options: {} }]);
    out.push(["text", { ...common, options: {}, limit: 300, maxBytes: 3000 }]);
    out.push(["outline", { ...common, options: {} }]);
    out.push(["find", { ...common, options: { pattern } }]);
    out.push(["text", { ...common, options: { scope: "all" } }]);
    out.push(["summary", { ...common, options: {} }]);
    out.push(["check", { ...common, options: {} }]);
  }
  return out;
}

const started = Date.now();
for (const bin of [baseBin, newBin]) await stat(bin);
let docs = await docxPaths(corpusRoot);
if (maxDocs > 0) docs = docs.slice(0, maxDocs);
const a = server(baseBin);
const b = server(newBin);
for (const s of [a, b]) {
  await s.rpc("initialize", {
    protocolVersion: "2025-11-25",
    capabilities: {},
    clientInfo: { name: "perf-parity", version: "1" },
  });
  s.notify("notifications/initialized");
}
console.error(`语料 ${docs.length} 份；基线 ${baseBin}；对照 ${newBin}`);

const diffs = [];
let compared = 0;
let unopened = 0;
let done = 0;
for (const doc of docs) {
  const openArgs = { options: { path: doc } };
  const [oa, ob] = await Promise.all([
    a.rpc("tools/call", { name: "open", arguments: openArgs }),
    b.rpc("tools/call", { name: "open", arguments: openArgs }),
  ]);
  let ida;
  let idb;
  try {
    ida = body(oa).content[0].sessionId;
    idb = body(ob).content[0].sessionId;
  } catch {
    // 打不开的语料两侧都应当同样打不开：直接比原始行。
    if (mask(oa)[0] !== mask(ob)[0]) {
      diffs.push({ doc: path.relative(repoRoot, doc), why: "open 响应不同" });
    }
    unopened += 1;
    if (/AGENT_RESOURCE_LIMIT/.test(oa)) throw new Error(`会话未回收: ${oa.slice(0, 200)}`);
    compared += 1;
    done += 1;
    continue;
  }
  // 两个进程必须收到**完全相同**的请求序列：JSON-RPC 的 id 是各自自增的，只给
  // 其中一个多发一条探针，之后每一行的 id 都会差 1，整行比较就全是假差异。
  let pattern = "a";
  try {
    const [pa] = await Promise.all([
      a.rpc("tools/call", { name: "text", arguments: { sessionId: ida, options: {}, resultShape: "structured" } }),
      b.rpc("tools/call", { name: "text", arguments: { sessionId: idb, options: {}, resultShape: "structured" } }),
    ]);
    for (const ch of String(body(pa).content ?? "")) {
      if (/[\p{L}\p{N}]/u.test(ch)) {
        pattern = ch;
        break;
      }
    }
  } catch {
    /* 用默认 pattern */
  }
  for (const [name, argsA] of calls(ida, pattern)) {
    const argsB = { ...argsA, sessionId: idb };
    let cursorA = null;
    let cursorB = null;
    for (let page = 0; page < maxPages; page += 1) {
      const [la, lb] = await Promise.all([
        a.rpc("tools/call", { name, arguments: cursorA ? { ...argsA, cursor: cursorA } : argsA }),
        b.rpc("tools/call", { name, arguments: cursorB ? { ...argsB, cursor: cursorB } : argsB }),
      ]);
      const [ma, lena] = mask(la);
      const [mb, lenb] = mask(lb);
      compared += 1;
      if (lena.join() !== lenb.join()) {
        diffs.push({ doc, name, page, why: `sessionId 长度不同 ${lena} vs ${lenb}` });
        break;
      }
      if (ma !== mb) {
        let at = 0;
        while (at < ma.length && at < mb.length && ma[at] === mb[at]) at += 1;
        diffs.push({
          doc: path.relative(repoRoot, doc),
          name,
          page,
          why: `响应行第 ${at} 字符起不同`,
          base: verbose ? ma.slice(at, at + 400) : undefined,
          next: verbose ? mb.slice(at, at + 400) : undefined,
        });
        break;
      }
      let next = null;
      try {
        next = body(la).nextCursor ?? null;
      } catch {
        break;
      }
      if (!next) break;
      cursorA = next;
      cursorB = body(lb).nextCursor;
    }
  }
  // 必须真的关掉：MCP 有并发会话上限，close 失败会让后面的 open 全部 AGENT_RESOURCE_LIMIT。
  for (const [s, id] of [
    [a, ida],
    [b, idb],
  ]) {
    const line = await s.rpc("tools/call", { name: "close", arguments: { sessionId: id, options: {} } });
    if (JSON.parse(line).result?.isError) throw new Error(`close 失败: ${line.slice(0, 200)}`);
  }
  done += 1;
  if (done % 50 === 0) console.error(`  ${done}/${docs.length} …`);
}
a.child.kill();
b.child.kill();
console.log(
  JSON.stringify(
    {
      docs: docs.length,
      comparisons: compared,
      unopened,
      seconds: Number(((Date.now() - started) / 1000).toFixed(1)),
      differences: diffs.length,
      sample: diffs.slice(0, 10),
    },
    null,
    2,
  ),
);
process.exit(diffs.length === 0 ? 0 : 1);
