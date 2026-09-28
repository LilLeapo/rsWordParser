// 把本 crate 的 wasm-bindgen 产物（`pkg/`，--target web）组装成 npm 包，就地发布 `pkg/`。
//
// 用法：
//   node <crate>/pack.mjs version   打印将要发布的版本（本 crate Cargo.toml 的 version）
//   node <crate>/pack.mjs pack      往 pkg/ 写 package.json、补 README / 许可证，再跑 smoke.mjs
//
// 包的元数据在同目录 package.json（不写 version，打包时取 Cargo.toml）；`files` 里的文件
// 依次从 pkg/、本目录、仓库根查找。
import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const crate = dirname(fileURLToPath(import.meta.url))
const root = resolve(crate, '../..')
const pkg = join(crate, 'pkg')

/** `[package]` 段的 version；`version.workspace = true` 时取根 `[workspace.package]`。 */
function crateVersion() {
  const section = (file, name) =>
    readFileSync(file, 'utf8')
      .split(/^\[/m)
      .find((s) => s.startsWith(`${name}]`)) ?? ''
  const own = section(join(crate, 'Cargo.toml'), 'package')
  const found =
    own.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ??
    (/^version\.workspace\s*=\s*true/m.test(own)
      ? section(join(root, 'Cargo.toml'), 'workspace.package').match(
          /^version\s*=\s*"([^"]+)"/m,
        )?.[1]
      : undefined)
  if (!found) throw new Error('Cargo.toml 没有 [package] version')
  return found
}

const version = crateVersion()
const command = process.argv[2]
if (command === 'version') {
  console.log(version)
} else if (command === 'pack') {
  const manifest = { ...JSON.parse(readFileSync(join(crate, 'package.json'), 'utf8')), version }
  for (const name of manifest.files) {
    const from = [join(pkg, name), join(crate, name), join(root, name)].find(existsSync)
    if (!from) throw new Error(`找不到要发布的文件 ${name}（先构建 pkg/，见本 crate README）`)
    if (from !== join(pkg, name)) copyFileSync(from, join(pkg, name))
  }
  writeFileSync(join(pkg, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  execFileSync(process.execPath, [join(crate, 'smoke.mjs'), pkg], { stdio: 'inherit' })
  console.log(`${manifest.name}@${version} -> ${pkg}`)
} else {
  throw new Error('用法：node <crate>/pack.mjs version | pack')
}
