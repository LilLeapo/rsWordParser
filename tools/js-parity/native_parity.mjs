// BIND-01/05/06/09：真实 wasm 的有状态薄壳；所有输出留内存，不改语料。
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { loadBinding } from './wasm-loader.mjs'

const index = process.argv.indexOf('--pkg')
if (index < 0) throw new Error('缺 --pkg')
const glue = await loadBinding(process.argv[index + 1], true)
const table = new glue.SessionTable()
const missing = [
  ['document', [null]], ['apply', ['{}', null]], ['save', [null]],
  ['media', [0]], ['addMedia', [new Uint8Array(), 'image/png']],
  ['resolveRuns', ['[]', null]], ['resolveParas', ['[]', null]],
  ['resolveCells', ['[]', null]], ['resolveSections', ['[]', null]],
  ['resolveTable', ['[]', null]], ['partBytes', [0]], ['nodeXml', [0, null]],
  ['diagnostics', []],
]
for (const [name, args] of missing) {
  assert.throws(() => table[name]('not-a-session', ...args), e => e.code === 'BIND_NO_SESSION')
}
const bytes = new Uint8Array(readFileSync(new URL('../../corpus/synthetic/bidi__001.docx', import.meta.url)))
const id = table.open(bytes, '{"expectProtocol":"native/0"}')
assert.deepEqual(table.save(id, null), bytes)
const before = table.document(id, null)
assert.throws(() => table.apply(id, 'bad json', null), e => e.code === 'BIND_BAD_ARGUMENT')
assert.throws(() => table.apply(id, JSON.stringify({
  op: 'setHeaderFooter', sect: 0xffffffff, kind: 'header', variant: 'default', content: [],
}), null), e => e.code === 'EDIT_TARGET_MISSING')
assert.equal(table.document(id, null), before)

const doc = JSON.parse(before)
const para = doc.main.find(b => b.kind === 'text').node
const op = JSON.stringify({ op: 'insertText', at: { para, offset: 0 }, text: 'wasm native edit' })
table.apply(id, op, null)
const edited = table.document(id, null)
assert.notEqual(edited, before)
const saved = table.save(id, null)
assert.equal(table.document(id, null), edited)
const reopened = table.open(saved, null)
assert.ok(table.document(reopened, null).includes('wasm native edit'))
const media = table.addMedia(id, new Uint8Array([1, 2, 3]), 'image/png')
assert.equal(table.addMedia(id, new Uint8Array([1, 2, 3]), 'image/png'), media)
assert.deepEqual(table.media(id, media), new Uint8Array([1, 2, 3]))
for (const [name] of missing.filter(([name]) => name.startsWith('resolve'))) {
  assert.equal(table[name](id, '[]', null), '[]')
}
assert.ok(table.partBytes(id, doc.mainPart).length > 0)
assert.ok(table.nodeXml(id, para, null).includes('wasm native edit'))
assert.equal(JSON.parse(table.diagnostics(id)).xmlEscapeCount, 0)
assert.equal(JSON.parse(table.version()).protocol, 'native/0')
if (process.argv.includes('--compat')) {
  assert.equal(JSON.parse(glue.version()).protocol, 'compat/1')
} else {
  for (const name of ['parse', 'parse_diagnostics', 'save', 'blank', 'version']) {
    assert.equal(glue[name], undefined, `default wasm unexpectedly exports ${name}`)
  }
}
table.close(id)
table.close(id)
table.close(reopened)
table.free()
console.error(`native_parity: ${missing.length} missing-session exports; lifecycle, atomic apply/save, media, queries and read-only outlets passed`)
