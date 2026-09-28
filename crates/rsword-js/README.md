# `rsword-js` · JS 绑定（wasm-bindgen）/ npm `@lilleapo/rs-word-parser`

`rsword` 的 wasm 入口，也是 GitHub Packages 上 npm 包 `@lilleapo/rs-word-parser`
的全部来源：wasm + wasm-bindgen 胶水（`--target web`）+ 类型声明，不含 CLI 与原生产物。
高保真 DOCX 读写：未编辑的内容保留原字节，编辑只补丁被改动的 XML；不做布局与渲染。

```js
import init, { SessionTable } from '@lilleapo/rs-word-parser'

await init() // 浏览器 / 打包器：按 import.meta.url 取同目录的 rsword_js_bg.wasm
// Node：import { initSync } …; initSync({ module: fs.readFileSync(wasm 路径) })

const table = new SessionTable()
const id = table.open(docxBytes, JSON.stringify({ expectProtocol: 'native/0' }))
const model = JSON.parse(table.document(id))
table.apply(id, JSON.stringify({ op: 'insertText', at: { para: model.main[0].node, offset: 0 }, text: 'Hi' }))
const saved = table.save(id) // Uint8Array
table.close(id)
```

## 面

**原生协议 `native/0`**（`spec/21`，稳定面）：`SessionTable` 的有状态会话——`open` /
`document` / `apply` / `save` / `diagnostics` / `media` / `addMedia` / `resolve*` /
`nodeXml` / `partBytes` / `close` / `version`。

**兼容面 `compat/1`**（`compat-ts` feature；npm 包带上，默认构建不带）：

```ts
version(): string                             // { version, git, protocol } 的 JSON
parse(bytes: Uint8Array): string              // TS ParsedDoc JSON 文本
parse_diagnostics(bytes: Uint8Array): string  // Document.warnings 的 JSON 数组
save(bytes: Uint8Array, blocksJson: string, optionsJson: string): Uint8Array
blank(optionsJson?: string): Uint8Array       // BlankDocxOptions：{ eastAsiaFont?: string }
```

与 GenOffice TS 引擎的对应：`parse` ↔ `parseDocx`、`save` ↔ `saveDocx`、`blank` ↔ `buildBlankDocx`。
这组入口是差分测试适配器（`spec/10`），形状跟随 TS，**不承诺跨版本稳定**。

- `blocksJson` 是 `finalBlocks` 数组的 JSON，**必填**——空的 `[]` 语义是「正文清空」，漏传参数
  不该悄悄走到那一步，所以空串 / `null` 直接报 `BIND_BAD_ARGUMENT`。
- `optionsJson` 是 `SaveOptions` 的 JSON，空串 / `null` 当 `{}`。

两套面出错都抛 JS `Error`（`RswordError`），带 `code`（`EDIT_BAD_POSITION` 一类的诊断码，
稳定可依赖）与 `message`（给人看的）。JS 侧照 `code` 分支，别去 parse `message`。

## 构建

```sh
tools/build-js.sh                          # 默认面：wasm32 + wasm-release → wasm-bindgen → pkg/
tools/build-js.sh --features compat-ts     # npm 发布件：再带兼容面
```

`wasm-release` 是给 wasm 用的档（体积优先 + LTO + 去符号）：普通 `release` 带 `debug = 1`，
在 wasm 里是几十 MB。`wasm-bindgen-cli` 必须与 Cargo.lock 里的 `wasm-bindgen` 精确同版本
（见仓库根的 `TOOLS.md`）。

## 安装与发布

包在 GitHub Packages（`npm.pkg.github.com`），安装需要带 `read:packages` 的 GitHub token：

```ini
# .npmrc
@lilleapo:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}
```


包的元数据在本目录 `package.json`（不写 `version`）；**版本号就是本 crate `Cargo.toml` 的
`version`**。`.github/workflows/npm.yml` 在 `main` 每次推送时检查 `@lilleapo/rs-word-parser@<版本>`：
注册表上已有则跳过，没有则按 release 的门验默认构建（`tools/js-parity/native_parity.mjs`），
再带 `compat-ts` 重建、组装、冒烟并发布。改版本号即发版；带 `-` 的预发布版本发到 dist-tag
`next`。用 workflow 自带的 `GITHUB_TOKEN` 发布，不需要额外 secret。

本地组装与冒烟（不发布）：

```sh
tools/build-js.sh --features compat-ts
node crates/rsword-js/pack.mjs pack        # pkg/ 成为完整的包目录，并跑 smoke.mjs
```

`pack.mjs` 往 `pkg/` 写 `package.json`、补本 README 与仓库根的许可证；`smoke.mjs` 从包目录加载
wasm，验原生会话无编辑保存逐字节相同（不变式 1）与兼容面的解析 / 保存。

## 这个 crate 里没有逻辑

真正的实现在 `rsword::bind::js` 与 `rsword::bind::native`：函数、错误映射、JSON 形态全在那儿，
这里的 `wasm_export!` 表只做类型转换，并顺带展开「非法输入 → 错误码」的原生单测。绑定层因此能在
**原生构建**里对着全语料验（`cargo test --workspace`），再由 node 侧对真实 wasm 产物复核：

- `cargo run -p diff-parse -- --scope all --via-js`（绑定输出与原生逐字节相同，两个语料）
- `cargo test -p rsword --test js_binding`（原生等价 + blank 门控比对）
- `cargo test -p rsword --test save_blocks js_binding_save_bytes_parity`（208 份保存用例
  经绑定与原生字节相同；门控见 `tools/js-parity/save_parity.mjs` 头注释）

License: MIT OR Apache-2.0.
