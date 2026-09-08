// 共享的 wasm 绑定装载（node 用 initSync；浏览器分支不属于本仓库的工具面）。
// `--target web` 的 glue 在 node 里不能走 fetch/URL，直接喂 wasm 字节。
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

export async function loadBinding(pkgDir) {
  const glue = await import(pathToFileURL(join(pkgDir, 'rsword_js.js')).href)
  glue.initSync({ module: readFileSync(join(pkgDir, 'rsword_js_bg.wasm')) })
  const version = JSON.parse(glue.version())
  console.error(`js-parity: 绑定 ${version.version} (git ${version.git}, ${version.protocol})`)
  return glue
}

/** `--out` 目录由 runner 自建（干净机器 / CI 首跑没有任何预建目录；评审复盘：漏了它 CI 必红）。 */
export function ensureDir(dir) {
  mkdirSync(dir, { recursive: true })
}

/** 递归收集目录下名字含 `needle` 的文件，排序——顺序必须与 Rust 测试一致（`fs::read_dir` 后 sort）。 */
export function collectFiles(root, needle) {
  const out = []
  const stack = [root]
  while (stack.length > 0) {
    const dir = stack.pop()
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name)
      if (e.isDirectory()) stack.push(p)
      else if (e.name.includes(needle)) out.push(p)
    }
  }
  return out.sort()
}

/** 传给 save 的 blocks / options 字符串；值层面的形状与 Rust 测试读到的 `serde_json::Value` 一致。 */
export function stringifyValue(v) {
  return JSON.stringify(v)
}

/**
 * 跑一个用例并把结果落到 `<out>/<name>`：成功写 `name`，绑定抛错写 `name.err`
 * （`{ code, message }`）。三个 runner 共用这一个出口，错误形态保持一致。
 * `outDir` 在这里兜底自建——runner 入口已 `ensureDir`，这层保证任何未来调用方也安全。
 */
export function emit(outDir, name, run) {
  ensureDir(outDir)
  try {
    const bytes = run()
    writeFileSync(join(outDir, name), bytes)
    return 'ok'
  } catch (e) {
    writeFileSync(join(outDir, `${name}.err`), JSON.stringify({ code: e.code ?? null, message: String(e) }))
    return 'err'
  }
}
