// `diff-parse --via-js` 的 node 侧（M8′ 8.0②，`COMPAT-02` 的绑定等价门）：对清单里的每份
// docx 跑 wasm 绑定的 `parse`，把输出**原样**写到 `<out>/<序号>.json`——一个字节都不加工，
// Rust 侧按序号比对。绑定抛错（不该发生在语料上）写 `<序号>.err`，Rust 侧按逐字节差异报告。
//
// 用法: node parse_parity.mjs --pkg DIR --out DIR --docs-file LIST
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { ensureDir, loadBinding } from './wasm-loader.mjs'

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  if (i < 0) throw new Error(`缺 --${name}`)
  return process.argv[i + 1]
}

const pkg = arg('pkg')
const out = arg('out')
ensureDir(out)
const docs = readFileSync(arg('docs-file'), 'utf8').split('\n').filter((l) => l.length > 0)
const glue = await loadBinding(pkg)

docs.forEach((path, i) => {
  const bytes = new Uint8Array(readFileSync(path))
  try {
    writeFileSync(join(out, `${i}.json`), glue.parse(bytes), 'utf8')
  } catch (e) {
    writeFileSync(join(out, `${i}.err`), JSON.stringify({ code: e.code ?? null, message: String(e) }))
  }
})
