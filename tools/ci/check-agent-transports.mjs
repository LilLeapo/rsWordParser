// AGENT-06/10：两个真实二进制；跨传输同区间/续读终态等价，游标必须互斥。
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';
import { resolve } from 'node:path';
const [cli, mcp] = process.argv.slice(2);
assert(cli && mcp, 'usage: node check-agent-transports.mjs CLI_BINARY MCP_BINARY');
const input = resolve('corpus/synthetic/word-basics__001.docx');
const child = spawn(mcp, [], { stdio: ['pipe', 'pipe', 'inherit'] });
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', line => { assert(pending.length, '未请求的 stdout 输出'); pending.shift()(JSON.parse(line)); });
let sequence = 0;
function rpc(method, params) {
  return new Promise((done, reject) => {
    const timer = setTimeout(() => reject(new Error('MCP response timeout')), 10000);
    pending.push(value => { clearTimeout(timer); done(value); });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: ++sequence, method, params }) + '\n');
  });
}
function unwrap(response) {
  assert(!response.error, JSON.stringify(response));
  const r = response.result;
  if (r.structuredContent) {
    assert.deepEqual(r.content, [{ type: 'text', text: 'Read structuredContent.' }]);
    return r.structuredContent;
  }
  assert.equal(r.content.length, 1);
  return JSON.parse(r.content[0].text);
}
function fileRead(limit, cursor) {
  const args = ['text', input, '--json', '--limit', String(limit), '--maxBytes', '4194304'];
  if (cursor) args.push('--cursor', cursor);
  const r = spawnSync(cli, args, { encoding: 'utf8' });
  assert([0, 2].includes(r.status), r.stderr);
  return JSON.parse(r.stdout);
}
function business(value) {
  const v = structuredClone(value);
  // 穷举允许差异：实际传输计数、快照身份及其游标标识。其余字段必须逐字段相等。
  for (const pointer of ['/usage/responseBytes', '/usage/estimatedTokens', '/snapshot', '/anchors/snapshot', '/nextCursor']) {
    const keys = pointer.slice(1).split('/');
    let object = v;
    for (const key of keys.slice(0, -1)) object = object[key];
    assert(Object.hasOwn(object, keys.at(-1)), `允许差异字段漂移: ${pointer}`);
    delete object[keys.at(-1)];
  }
  return v;
}
try {
  await rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'transport-parity', version: '1' } });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  const opened = unwrap(await rpc('tools/call', { name: 'open', arguments: { options: { path: input } } }));
  const sessionId = opened.content[0].sessionId;
  const read = async (limit, cursor, resultShape = 'text') => unwrap(await rpc('tools/call', {
    name: 'text', arguments: { sessionId, options: {}, limit, maxBytes: 4194304, cursor, resultShape },
  }));
  const fullFile = fileRead(1048576);
  const fullSession = await read(1048576, null);
  assert.deepEqual(business(fullFile), business(fullSession));
  assert.equal(fullFile.truncated, false);
  assert.equal(fullSession.truncated, false);
  const firstLimit = fullFile.content.indexOf('\n') + 1;
  assert(firstLimit > 0 && firstLimit < fullFile.content.length);
  const fileFirst = fileRead(firstLimit);
  const sessionFirst = await read(firstLimit, null);
  assert(fileFirst.nextCursor && sessionFirst.nextCursor, '两侧必须真有续页');
  assert.deepEqual(business(fileFirst), business(sessionFirst));
  // 两个负例使用各自真实返回的游标，不能拿伪造字符串代替错传输。
  const intoMcp = await read(1048576, fileFirst.nextCursor);
  assert.equal(intoMcp.code, 'AGENT_BAD_CURSOR');
  assert.match(intoMcp.message, /MCP.*CLI/);
  const intoCli = fileRead(1048576, sessionFirst.nextCursor);
  assert.equal(intoCli.code, 'AGENT_BAD_CURSOR');
  assert.match(intoCli.message, /CLI.*MCP/);
  // 错投没有消费原游标；各用自己的游标继续，MCP 同时切到 structured。
  let file = fileFirst, session = sessionFirst;
  let fileText = file.content, sessionText = session.content;
  let pages = 1;
  while (file.nextCursor || session.nextCursor) {
    assert(file.nextCursor && session.nextCursor);
    file = fileRead(1048576, file.nextCursor);
    session = await read(1048576, session.nextCursor, 'structured');
    assert.deepEqual(business(file), business(session));
    fileText += file.content; sessionText += session.content;
    assert(++pages <= 10, '游标未前进');
  }
  assert.equal(fileText, fullFile.content);
  assert.equal(sessionText, fullSession.content);
  assert.equal(fileText, sessionText);
  console.log(`AGENT-06/10: CLI/MCP business and continuation parity; ${pages} pages; both wrong-transport cursors rejected`);
} finally {
  child.stdin.end();
  child.kill();
  lines.close();
}
