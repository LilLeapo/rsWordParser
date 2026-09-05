# `resolve` 校准 fixture（`RES-12` / `TEST-08`，任务 5.8）

这里的每个目录是一份**最小 docx** 加一张断言表：

```
fixtures/resolve/<域>/<用例>/
  doc.docx        由 `cargo run -p gen-fixtures` 生成，可重复（同输入同字节）
  expected.toml   断言表；`verified = false` 的条目测试只记不断言
```

## 为什么观察值只能来自 Word

两处规则**规范说不清**，只有 Word 的显示结果能定案：

- **`RES-04` toggle 属性**（`b` / `i` / `caps` / `strike` …）。ECMA-376 §17.7.3 说样式层级里为
  `true` 的次数为奇数才是 `true`，再与 `docDefaults` 异或——也就是"段落样式加粗 + 字符样式加粗
  = 不加粗"。[MS-OI29500] 又记录了 Word 在 `docDefaults`、表格样式、多层 `basedOn` 上的一串偏差，
  且与 Word 版本有关。本引擎目前用的是"最具体的声明胜出"（`resolve::toggle::ToggleRule`
  的 `MostSpecificWins`，也是 TS 参考实现的行为）。
- **`RES-10` 节的页眉页脚继承**：一节自己没有 `w:headerReference` 时，Word 到底继续显示上一节的
  页眉，还是显示空的。

**不能用引擎自己的输出去填期望值**——那是自证，比没有断言更糟。所以每条断言带 `verified`：

| `verified` | 测试行为 |
| --- | --- |
| `false` | 只打印"引擎说 X，文件里占位 Y"，不断言。占位值是随手写的，别当结论 |
| `true` | 真的断言。不通过就是引擎错了：**以 Word 为准**改规则，并在下面记下差异 |

## 怎么填

1. 在真实 Word 里打开 `doc.docx`（Windows 或 macOS 版都行，版本记在下面）。
2. 按每个 `[[run]]` 上面的注释看那句话**到底加不加粗**，把 `bold` 改成看到的结果。
   `sections/inherit-default` 看的是**第二页**的页眉显示什么，填 `header_default`。
3. 把该条的 `verified` 改成 `true`。
4. 跑 `cargo test -p rsword --test resolve_fixtures`。
   - 全绿：规则与 Word 一致，`resolve::toggle::ACTIVE_TOGGLE_RULE` 不用动。
   - 有红：以 Word 为准。toggle 就换 `ACTIVE_TOGGLE_RULE`（`OddParity` 已经实现并有单测），
     或者按实测再加一条规则；改完把 `spec/07-resolve.md` 的 `RES-04` 条目按实测改写，
     并在下面的"实测记录"里写清与 ECMA-376 §17.7.3 / [MS-OI29500] 的差异。
5. `tests/resolve.rs` 那 86,465 项 `StyleDisplay` 对照是拿 TS 当参照的（TS 是
   child-overrides-parent）。换规则后若不再全等，**不是回归**：按路径登记到
   `bind/compat_ts/KNOWN_DIFFS.md`，理由写"Word 实测为准"。

## 六份 fixture

| 目录 | 看什么 |
| --- | --- |
| `toggle/para-and-char` | 段落样式 `b` + 字符样式 `b`：奇偶规则说不加粗，最具体胜出说加粗 |
| `toggle/docdefaults-and-para` | `docDefaults` 的 `b` 与段落样式的 `b` 叠加 |
| `toggle/based-on-two-levels` | `basedOn` 链上两层都 `b` |
| `toggle/table-first-row` | 表格样式 `firstRow` 的 `b` 与段落样式的 `b` 叠加（`RES-08` 的表格视图） |
| `toggle/direct-off` | 直接 `w:b w:val="0"` 压住样式的 `b`（两种规则都该说不加粗，用来验对照组） |
| `sections/inherit-default` | 第二节没有 `headerReference` 时第二页的页眉 |

## 实测记录

> 还没有观察值。填的人请补：Word 版本（如 `Microsoft 365 版本 2408，macOS 14.6`）、观察日期、
> 每份 fixture 看到的结果，以及与 ECMA-376 §17.7.3 / [MS-OI29500] 的差异。

- [ ] `toggle/para-and-char`
- [ ] `toggle/docdefaults-and-para`
- [ ] `toggle/based-on-two-levels`
- [ ] `toggle/table-first-row`
- [ ] `toggle/direct-off`
- [ ] `sections/inherit-default`
