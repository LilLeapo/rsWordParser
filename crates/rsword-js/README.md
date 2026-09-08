# `rsword-js` · JS 绑定（wasm-bindgen）

`rsword` 的 wasm 入口。M8′ 的原生协议（`spec/21`，会话与模型 JSON）落地前，这份绑定面是
`compat_ts` 的五函数面；差分工具（`diff-parse --via-js`、`tools/js-parity/`）只走这几个入口。

## 面

```ts
version(): string                             // { version, git, protocol } 的 JSON
parse(bytes: Uint8Array): string              // ParsedDoc JSON 文本
parseDiagnostics(bytes: Uint8Array): string   // Document.warnings 的 JSON 数组
save(bytes: Uint8Array, blocksJson: string, optionsJson: string): Uint8Array
blank(optionsJson?: string): Uint8Array       // BlankDocxOptions：{ eastAsiaFont?: string }
```

与 TS 的对应：`parse` ↔ `parseDocx`、`save` ↔ `saveDocx`、`blank` ↔ `buildBlankDocx`。

- `blocksJson` 是 `finalBlocks` 数组的 JSON，**必填**——空的 `[]` 语义是「正文清空」，漏传参数
  不该悄悄走到那一步，所以空串 / `null` 直接报 `BIND_BAD_ARGUMENT`。
- `optionsJson` 是 `SaveOptions` 的 JSON，空串 / `null` 当 `{}`。
- 出错抛 JS `Error`（`RswordError`），带两个属性：`code`（`EDIT_BAD_POSITION` 一类的诊断码，
  稳定可依赖）与 `message`（给人看的）。JS 侧照 `code` 分支，别去 parse `message`。

## 构建

```sh
tools/build-js.sh   # wasm32 + wasm-release 构建 → wasm-bindgen → crates/rsword-js/pkg/
```

`wasm-release` 是给 wasm 用的档（体积优先 + LTO + 去符号）：普通 `release` 带 `debug = 1`，
在 wasm 里是几十 MB。`wasm-bindgen-cli` 必须与 Cargo.lock 里的 `wasm-bindgen` 精确同版本
（见仓库根的 `TOOLS.md`）。

## 这个 crate 里没有逻辑

真正的实现在 `rsword::bind::js`：五个函数、错误映射、JSON 形态全在那儿，这里的
`wasm_export!` 表只做类型转换，并顺带展开「非法输入 → 错误码」的原生单测。绑定层因此能在
**原生构建**里对着全语料验（`cargo test --workspace`），再由 node 侧对真实 wasm 产物复核：

- `cargo run -p diff-parse -- --scope all --via-js`（绑定输出与原生逐字节相同，两个语料）
- `cargo test -p rsword --test js_binding`（原生等价 + blank 门控比对）
- `cargo test -p rsword --test save_blocks js_binding_save_bytes_parity`（208 份保存用例
  经绑定与原生字节相同；门控见 `tools/js-parity/save_parity.mjs` 头注释）
