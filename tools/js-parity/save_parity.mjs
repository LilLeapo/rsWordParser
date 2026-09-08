// save_blocks 的 node 侧（M8′ 8.0②，`COMPAT-08` 的绑定等价门）：对全部 `*.save.<k>.json`
// 用例，用 wasm 绑定的 `save` 产出保存结果——成功写 `<out>/<用例文件名>.docx`，被拒写
// `<out>/<用例文件名>.err`（`{ code, message }`）。Rust 侧（`tests/save_blocks.rs` 的
// `RSWORD_JS_SAVE_DIR` 模式）对同一批用例跑原生路径，按文件名逐一比字节。
//
// 用法: node save_parity.mjs --pkg DIR --corpus DIR --out DIR
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { collectFiles, emit, ensureDir, loadBinding, stringifyValue } from './wasm-loader.mjs'

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  if (i < 0) throw new Error(`缺 --${name}`)
  return process.argv[i + 1]
}

const pkg = arg('pkg')
const corpus = arg('corpus')
const out = arg('out')
ensureDir(out)
const glue = await loadBinding(pkg)

// 与 tests/save_blocks.rs 相同的发现规则：文件名含 `.save.`，按文件名排序
const cases = collectFiles(corpus, '.save.')
let ok = 0
let rejected = 0
for (const casePath of cases) {
  const name = casePath.split('/').pop()
  const stem = name.split('.save.')[0]
  const caseJson = JSON.parse(readFileSync(casePath, 'utf8'))
  const status = emit(out, `${name}.docx`, () => {
    const bytes = new Uint8Array(readFileSync(join(casePath, '..', `${stem}.docx`)))
    return glue.save(bytes, stringifyValue(caseJson.blocks), stringifyValue(caseJson.options))
  })
  status === 'ok' ? (ok += 1) : (rejected += 1)
}
console.error(`save_parity: ${cases.length} 份用例，${ok} 保存、${rejected} 被拒`)
