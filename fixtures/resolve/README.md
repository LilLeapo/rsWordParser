# `resolve` 校准 fixture（`RES-12` / `TEST-08`，任务 5.8）

> 还没定死的那一块（`strike` 一族只在 Word 网页版测过）与下一步怎么测，见
> **`docs/06-toggle-open-question.md`**。本文只管 fixture 本身怎么用、怎么填。

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
  且与 Word 版本有关。本引擎**已按实测校准**为 `resolve::toggle::ToggleRule::WordObserved`
  （见下面的实测记录）；`MostSpecificWins`（TS 参考实现的行为）与 `OddParity`（规范字面）
  仍保留在枚举里备查。
- **`RES-10` 节的页眉页脚继承**：一节自己没有 `w:headerReference` 时，Word 到底继续显示上一节的
  页眉，还是显示空的。**实测：继续显示上一节的**，与本引擎一致。

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

## 八份 fixture

| 目录 | 看什么 |
| --- | --- |
| `toggle/para-and-char` | 段落样式 `b` + 字符样式 `b`：奇偶规则说不加粗，最具体胜出说加粗 |
| `toggle/docdefaults-and-para` | `docDefaults` 的 `b` 与段落样式的 `b` 叠加 |
| `toggle/based-on-two-levels` | `basedOn` 链上两层都 `b` |
| `toggle/table-first-row` | 表格样式 `firstRow` 的 `b` 与段落样式的 `b` 叠加（`RES-08` 的表格视图） |
| `toggle/direct-off` | 直接 `w:b w:val="0"` 压住样式的 `b`（两种规则都该说不加粗，用来验对照组） |
| `toggle/docdefaults-and-para-off` | `docDefaults` 的 `b` 与段落样式**显式关掉**的 `b=0`（5.8b 补测） |
| `toggle/other-toggles` | `i` / `strike` / `caps` / `smallCaps` / `dstrike` / `vanish` 各自两层都声明（5.8b 补测，推翻了"九个 toggle 一视同仁"） |
| `sections/inherit-default` | 第二节没有 `headerReference` 时第二页的页眉 |

## 实测记录

**2026-09-06，Word 网页版（office.com / OneDrive，浏览器 Chrome，macOS）。** 八份文档全部上传到
OneDrive 后用 Word 网页版打开，逐段把光标放进去，读功能区"加粗"按钮的按下状态与字体名框
（加粗时显示"宋体 (粗体)"）。两处各复核一次：把光标移开再移回、以及双击选中整个词后重读。

| fixture | 段落 | 声明 | Word |
| --- | --- | --- | --- |
| `toggle/para-and-char` | `para b + char b` | 段落样式 b + 字符样式 b | **不加粗** |
| | `para b only` | 段落样式 b | 加粗 |
| `toggle/docdefaults-and-para` | `docDefaults b + para b` | docDefaults b + 段落样式 b | **不加粗** |
| | `docDefaults b only` | 只有 docDefaults b | **不加粗** |
| `toggle/based-on-two-levels` | `basedOn b + derived b` | basedOn 链上两层都 b | **加粗** |
| | `base b only` | 段落样式 b | 加粗 |
| `toggle/table-first-row` | `table firstRow b + para b` | 表格样式 firstRow b + 段落样式 b | **不加粗** |
| | `table body + para b` | 段落样式 b | 加粗 |
| `toggle/direct-off` | `direct b=0 over style b` | 直接 b=0 压样式 b | 不加粗 |
| | `style b, no direct` | 段落样式 b | 加粗 |
| `sections/inherit-default` | 第二页页眉 | 第二节无 `headerReference` | **显示"第一节页眉"** |
| `toggle/docdefaults-and-para-off` | `docDefaults b + para b=0` | docDefaults b + 段落样式 b=false | **加粗**（段落样式明写"不加粗"，Word 照样加粗） |
| | `docDefaults b only` | 只有 docDefaults b | 不加粗（复现上一条） |
| `toggle/other-toggles` | `i twice` / `i once` | 段落样式 + 字符样式 / 只有段落样式 | **不斜 / 斜**（与 `b` 同规则） |
| | `strike twice` / `strike once` | 同上 | **都有删除线**（**不**异或） |
| | `caps twice` / `caps once` | 同上 | **都是大写**（不异或） |
| | `smallcaps twice` / `smallcaps once` | 同上 | **都是小型大写**（不异或） |
| | `dstrike twice` / `dstrike once` | 同上 | **都有删除线**（不异或） |
| | `vanish twice` / `vanish once` | 同上 | **观察不到**：Word 网页版把隐藏文字照常显示 |

### 结论一：同一份规范里的 toggle，Word 并不同待遇

`b` 与 `i` 按层级异或；`caps` / `smallCaps` / `strike` / `dstrike` **不异或**——两层都声明时
效果照样是开的，也就是"最具体的声明胜出"。所以引擎的规则**按字段选**
（`resolve::toggle` 的 `toggle_fields!` 那张表），没有单一的"当前规则"：

| 字段 | 规则 | 依据 |
| --- | --- | --- |
| `b` / `i` | `WordObserved`（层级异或） | 实测 |
| `bCs` / `iCs` | `WordObserved` | 未单独实测，跟着各自的本体走 |
| `caps` / `smallCaps` / `strike` / `dstrike` | `MostSpecificWins` | 实测 |
| `vanish` | `MostSpecificWins` | 观察不到，按 `strike` 一族处理（也是 TS 的行为） |

### 结论二：`b` / `i` 的层级异或规则

原来激活的"最具体的声明胜出"（也是 TS 参考实现的行为）在前六份里错了三份。实测出来的规则是
`resolve::toggle::ToggleRule::WordObserved`，已激活：

```text
有效值 = docDefaults ⊕ 段落样式层 ⊕ 表格样式层 ⊕ 字符样式层
```

- 奇偶只发生在**层级之间**。层级内部（`basedOn` 链）是普通的"子覆盖父"——`based-on-two-levels`
  两层都 `b=true` 仍然加粗，说明链内不计次数。**这一条与 ECMA-376 §17.7.3 的字面表述不同**：
  规范说的是"层级各样式中为 true 的次数"，实测是"层级数"。属于 [MS-OI29500] 记录的 Word 偏差一类。
- 段落样式层有一处要留神：**链里一处都没声明时，它取 docDefaults 的值**。每个段落都有样式
  （没写 `w:pStyle` 就是 Normal），而样式链的根是 docDefaults，于是 docDefaults 的值在
  "docDefaults 层"与"段落样式层"各出现一次、自己把自己抵消掉。`docDefaults b only` 那一条
  不加粗就是这么来的——两条候选规则都预测加粗，只有这个模型对得上。
- 直接格式一票定音，与两条候选规则一致。

### 来源（`RES-01`）

异或出来的值谁都没单独写过，所以 `Provenance` 是 `Toggle { levels }`，`levels` 按最具体到
最不具体列出参与的层。`expected.toml` 的 `source` 字段把它写成 `Toggle:CharStyle:CBold+ParaStyle:PBold`
这样，一起断言。只有一个层级参与、且有效值就是它写的那个值时才指那一层。

### 还没测到的角

- **`vanish`**：Word 网页版把隐藏文字照常显示，看不出开关状态。引擎按 `strike` 一族处理。
- **`bCs` / `iCs`**：复杂脚本孪生，要 `w:rtl` 的阿拉伯文 / 希伯来文才看得见，没测。
  跟着 `b` / `i` 走。
- **只在 Word 网页版上测过**。桌面版 Word 的渲染是另一套实现，`strike` 一族不异或这条
  尤其值得在桌面版上复核一次——它与 ECMA-376 §17.7.3 的字面表述冲突最大。

### 换规则影响到哪

只影响 `resolve` 这个**公开只读视图**（编辑器消费的那份有效属性）。`bind/compat_ts` 的
`runs[].bold` 发的是 run 自己 `w:rPr` 里的**声明值**（复现 TS 的形态），根本不走
`Resolver::run`；`tests/resolve.rs` 那 86,465 项 `StyleDisplay` 比的是每个样式自己的链合并，
也不走 toggle 规则。所以换规则之后五道差分门与保存语料一个数字都没变——
**这不是"语料证明了新规则安全"**，而是语料压根不覆盖这条路径。语料里没有任何一份文档
在两个不同层级声明同一个 toggle（`docDefaults` 里带 `w:b` 的文档为 0 份）。
