# `rsword-js` · JS 绑定（wasm-bindgen）

`rsword` 的浏览器 / Electron 渲染进程入口，与 TS `docx-engine` 同一个位置。M8「编辑器切换到
Rust 引擎」用它。

## 面

```ts
version(): string
parse(bytes: Uint8Array): string              // ParsedDoc JSON 文本
save(bytes: Uint8Array, blocksJson: string, optionsJson: string): Uint8Array
blank(eastAsiaFont?: string): Uint8Array
```

与 TS 的对应：`parse` ↔ `parseDocx`、`save` ↔ `saveDocx`、`blank` ↔ `buildBlankDocx`。

- `blocksJson` 是 `finalBlocks` 数组的 JSON，**必填**——空的 `[]` 语义是「正文清空」，漏传参数
  不该悄悄走到那一步，所以空串 / `null` 直接报错。
- `optionsJson` 是 `SaveOptions` 的 JSON，空串 / `null` 当 `{}`。
- 出错抛 JS `Error`，带两个属性：`code`（`EDIT_BAD_POSITION` 一类的诊断码，稳定可依赖）与
  `message`（给人看的）。JS 侧照 `code` 分支，别去 parse `message`。

## 构建

```sh
cargo build -p rsword-js --profile wasm-release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir pkg \
  target/wasm32-unknown-unknown/wasm-release/rsword_js.wasm
```

`wasm-release` 是给 wasm 用的档（体积优先 + LTO + 去符号）：普通 `release` 带 `debug = 1`，
在 wasm 里是几十 MB。CI 只跑到 `cargo build` 这一步（`wasm-bindgen` 的后处理在发布流程里）。

## 这个 crate 里没有逻辑

真正的实现在 `rsword::bind::js`：四个函数、错误映射、JSON 形态全在那儿，这里只做类型转换。
这么分是为了让绑定层能在**原生构建**里对着全语料验：

- `cargo run -p diff-parse -- --scope all --via js`（两个语料都是 0 处未知差异）
- `cargo test -p rsword --test js_binding`（`parse` 与原生投影逐字节相同、`save` 对 208 份
  保存用例字节相同、错误码稳定）

不用起 node 就能守住这两条。
