#!/usr/bin/env node
// docs/21 WP1/WP2：两个 rsword CLI 二进制在**同一批绝对路径**上跑全语料，逐字节
// 对照 stdout 与退出码。性能改动只允许更快，不允许换一个字节。
//
//   node tools/perf/compare-cli-bytes.mjs <baseline-bin> <new-bin> [选项]
//     --corpus DIR   语料根目录（默认：本仓库的 corpus/）
//     --jobs N       并发度（默认：CPU 核数）
//     --pages N      跟随 nextCursor 的最大页数（默认 6）
//     --docs N       只取前 N 份文档（冒烟用；默认全部）
//     --verbose      打印每条差异的前 400 字节
//
// 两个二进制必须跑同一批绝对路径：文件游标的 binding 里带 `identity`（绝对路径），
// 路径不同则游标不同，后续页会跟着全部不同。
//
// `snapshot.sessionId` 是 `a{pid}-{nonce}-{id}`，每个进程都不一样，而它进信封，
// 会改变 responseBytes / estimatedTokens。比较前掩成定长占位，并要求两侧的
// sessionId **长度一致**——长度不同则字节账本来就会差，那样的比较无意义。
// macOS 的 pid 位数在 3–6 位之间跳，所以长度不一致会零星发生；碰到就把整条配置
// 两边**重跑**（最多 RETRIES 次），只比较长度对齐的那一对，绝不跨长度比。
import { spawn } from "node:child_process";
import { readdir, stat } from "node:fs/promises";
import { cpus } from "node:os";
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
  console.error("用法: compare-cli-bytes.mjs <baseline-bin> <new-bin> [--corpus DIR] [--jobs N]");
  process.exit(2);
}
const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const corpusRoot = path.resolve(flag("corpus", path.join(repoRoot, "corpus")));
const jobs = Number(flag("jobs", String(Math.max(1, cpus().length))));
const maxPages = Number(flag("pages", "6"));
const maxDocs = Number(flag("docs", "0"));
const verbose = args.includes("--verbose");
/** sessionId 长度不齐时，整条配置两边重跑的上限。 */
const RETRIES = 12;

/** 预算档：默认 + 两档小预算。小预算专门逼出 AGENT_BUDGET_TOO_SMALL 与多页路径。 */
const BUDGETS = [[], ["--limit", "300", "--maxBytes", "3000"], ["--limit", "8", "--maxBytes", "600"]];

async function docxPaths(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await docxPaths(full)));
    else if (entry.name.endsWith(".docx")) out.push(full);
  }
  return out.sort();
}

function run(bin, argv) {
  return new Promise((resolve) => {
    const child = spawn(bin, argv, { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"] });
    const out = [];
    const err = [];
    child.stdout.on("data", (c) => out.push(c));
    child.stderr.on("data", (c) => err.push(c));
    child.on("error", (e) => resolve({ code: -1, stdout: "", stderr: String(e) }));
    child.on("close", (code) =>
      resolve({
        code,
        stdout: Buffer.concat(out).toString("utf8"),
        stderr: Buffer.concat(err).toString("utf8"),
      }),
    );
  });
}

const SESSION = /"sessionId":\s*"([^"]*)"/g;
/** 掩掉 sessionId，返回 [掩码后的文本, 出现过的各 sessionId 长度]。 */
function mask(text) {
  const lengths = [];
  const masked = text.replace(SESSION, (_m, id) => {
    lengths.push(id.length);
    return '"sessionId":"<id>"';
  });
  return [masked, lengths];
}

/** 跑一条配置（跟随 nextCursor 到 maxPages 页），返回逐页的 {code, stdout}。 */
async function pages(bin, argv) {
  const out = [];
  let cursor = null;
  for (let i = 0; i < maxPages; i += 1) {
    const full = cursor ? [...argv, "--cursor", cursor] : argv;
    const r = await run(bin, full);
    out.push(r);
    if (r.code !== 0) break;
    let next = null;
    try {
      next = JSON.parse(r.stdout).nextCursor ?? null;
    } catch {
      break;
    }
    if (!next) break;
    cursor = next;
  }
  return out;
}

/** find 的 pattern 取自本文档 text 首屏里的第一个字母/数字，保证真的有命中。 */
function patternFrom(stdout) {
  try {
    const content = JSON.parse(stdout).content;
    if (typeof content === "string") {
      for (const ch of content) if (/[\p{L}\p{N}]/u.test(ch)) return ch;
    }
  } catch {
    /* 解析失败就退回默认 pattern */
  }
  return "a";
}

function configs(doc, pattern) {
  const out = [];
  for (const budget of BUDGETS) {
    out.push(["text", doc, "--json", ...budget]);
    out.push(["outline", doc, "--json", ...budget]);
    out.push(["find", doc, "--json", "--pattern", pattern, ...budget]);
    out.push(["context", doc, "--json", "--offset", "0", "--before", "40", "--after", "40", ...budget]);
  }
  // scope=all 各一档：把页眉页脚 / 批注等辅助流一起拉进投影。
  const all = ["--options", '{"scope":"all"}'];
  out.push(["text", doc, "--json", ...all]);
  out.push(["outline", doc, "--json", ...all]);
  out.push(["find", doc, "--json", "--pattern", pattern, ...all]);
  out.push(["context", doc, "--json", "--offset", "0", "--before", "40", "--after", "40", ...all]);
  return out;
}

async function compareDoc(doc) {
  const probe = await run(newBin, ["text", doc, "--json"]);
  const pattern = patternFrom(probe.stdout);
  const diffs = [];
  let invocations = 0;
  let retries = 0;
  for (const argv of configs(doc, pattern)) {
    let a;
    let b;
    let aligned = false;
    for (let attempt = 0; attempt <= RETRIES; attempt += 1) {
      [a, b] = await Promise.all([pages(baseBin, argv), pages(newBin, argv)]);
      invocations += a.length + b.length;
      aligned =
        a.length === b.length &&
        a.every((_, i) => mask(a[i].stdout)[1].join() === mask(b[i].stdout)[1].join());
      if (aligned) break;
      retries += 1;
    }
    if (a.length !== b.length) {
      diffs.push({ argv, why: `页数不同 ${a.length} vs ${b.length}` });
      continue;
    }
    if (!aligned) {
      diffs.push({ argv, why: `sessionId 长度重跑 ${RETRIES} 次仍不齐（字节账无从比较）` });
      continue;
    }
    for (let i = 0; i < a.length; i += 1) {
      if (a[i].code !== b[i].code) {
        diffs.push({ argv, page: i, why: `退出码 ${a[i].code} vs ${b[i].code}` });
        continue;
      }
      const [ma] = mask(a[i].stdout);
      const [mb] = mask(b[i].stdout);
      if (ma !== mb) {
        let at = 0;
        while (at < ma.length && at < mb.length && ma[at] === mb[at]) at += 1;
        diffs.push({
          argv,
          page: i,
          why: `stdout 第 ${at} 字符起不同`,
          base: verbose ? ma.slice(at, at + 400) : undefined,
          next: verbose ? mb.slice(at, at + 400) : undefined,
        });
      }
    }
  }
  return { diffs, invocations, retries };
}

const started = Date.now();
let docs = await docxPaths(corpusRoot);
if (maxDocs > 0) docs = docs.slice(0, maxDocs);
for (const bin of [baseBin, newBin]) await stat(bin);
console.error(`语料 ${docs.length} 份；并发 ${jobs}；基线 ${baseBin}；对照 ${newBin}`);

let done = 0;
let invocations = 0;
let retries = 0;
const allDiffs = [];
let cursor = 0;
await Promise.all(
  Array.from({ length: jobs }, async () => {
    for (;;) {
      const i = cursor;
      cursor += 1;
      if (i >= docs.length) return;
      const r = await compareDoc(docs[i]);
      invocations += r.invocations;
      retries += r.retries;
      for (const d of r.diffs) allDiffs.push({ doc: path.relative(repoRoot, docs[i]), ...d });
      done += 1;
      if (done % 50 === 0) console.error(`  ${done}/${docs.length} …`);
    }
  }),
);

const seconds = ((Date.now() - started) / 1000).toFixed(1);
console.log(
  JSON.stringify(
    {
      docs: docs.length,
      invocations,
      sessionIdRetries: retries,
      seconds: Number(seconds),
      differences: allDiffs.length,
      sample: allDiffs.slice(0, 10),
    },
    null,
    2,
  ),
);
process.exit(allDiffs.length === 0 ? 0 : 1);
