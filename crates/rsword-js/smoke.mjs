// 发布前冒烟：从组装好的包目录加载 wasm，走一遍原生会话与 compat 面。
// 用法：node crates/rsword-js/smoke.mjs <包目录>（pack.mjs pack 会对 pkg/ 调用）
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const dir = resolve(process.argv[2])
const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'))
const js = await import(pathToFileURL(join(dir, manifest.main)).href)
js.initSync({ module: readFileSync(join(dir, 'rsword_js_bg.wasm')) })

// 原生协议：空白模板开会话，无编辑保存必须与输入逐字节相同（不变式 1）
const blank = js.blank()
const table = new js.SessionTable()
assert.equal(JSON.parse(table.version()).protocol, 'native/0')
const id = table.open(blank, JSON.stringify({ expectProtocol: 'native/0' }))
assert.ok(JSON.parse(table.document(id)).main.length > 0)
assert.deepEqual(table.save(id), blank)
table.close(id)
table.free()

// compat 面：ParsedDoc JSON 与 SaveBlock[] 保存
assert.equal(JSON.parse(js.version()).protocol, 'compat/1')
const parsed = JSON.parse(js.parse(blank))
const blocks = parsed.blocks
  .filter((b) => !b.hidden)
  .map((b) => ({ kind: 'original', docxIndex: b.docxIndex }))
assert.deepEqual(js.save(blank, JSON.stringify(blocks), '{}'), blank)
console.log(`smoke ok: ${manifest.name}@${manifest.version}`)
