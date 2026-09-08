// blank 的 node 侧（M8′ 8.0②，`SAVE-05` 的绑定等价门）：对每种 UI 语言给一个
// `eastAsiaFont`，用 wasm 绑定的 `blank` 产出空白文档，落 `<out>/<名字>.docx` +
// manifest.json（名字 → 字体）。Rust 侧（`tests/js_binding.rs` 的 `RSWORD_JS_BLANK_DIR`
// 模式）对同一组字体跑原生 `blank_docx` 比字节。字体清单按调用方需要给（CI 用四种
// UI 语言的典型字体）。
//
// 用法: node blank_parity.mjs --pkg DIR --out DIR --fonts "SimSun,Yu Gothic,PingFang SC"
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { emit, loadBinding } from './wasm-loader.mjs'

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  if (i < 0) throw new Error(`缺 --${name}`)
  return process.argv[i + 1]
}

const pkg = arg('pkg')
const out = arg('out')
const fonts = (arg('fonts') ?? '').split(',').map((f) => f.trim()).filter((f) => f.length > 0)
const glue = await loadBinding(pkg)

const slug = (s) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')
const manifest = []
for (const font of [null, ...fonts]) {
  const name = font === null ? 'none.docx' : `font-${slug(font)}.docx`
  const status = emit(out, name, () => glue.blank(font === null ? null : JSON.stringify({ eastAsiaFont: font })))
  if (status !== 'ok') throw new Error(`blank(${font}) 失败`)
  manifest.push({ file: name, eastAsiaFont: font })
}
writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2))
console.error(`blank_parity: ${manifest.length} 份（none + ${fonts.length} 种字体）`)
