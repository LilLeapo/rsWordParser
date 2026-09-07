# `RES-04` toggle 规则的未决部分（2026-09-06 暂存）

> **一句话**：不挡进度，也改不坏文件。真正该做的下一步只有一件——**在桌面版 Word 上复核
> `strike` 一族**。本文把已知、未知、以及怎么测记下来，等有空再动手。
>
> 相关：`spec/07-resolve.md` 的 `RES-04`、`fixtures/resolve/README.md`（实测方法与完整读数）、
> `crates/rsword/src/resolve/toggle.rs`（那张按字段的规则表）。

## 1. 问题是什么

ECMA-376 §17.7.3 把十三个属性归为 "toggle"，规定它们**不按** `child ?? parent` 合并，而是
按层级里为 `true` 的次数取奇偶。[MS-OI29500] 又记了一串 Word 的偏差，且与 Word 版本有关。
也就是说规范本身说不清，只能拿 Word 的显示结果定案。

2026-09-06 用八份最小 docx 在 **Word 网页版**上实测（方法见 `fixtures/resolve/README.md`），
结论比预期复杂：**同一份规范里的 toggle，Word 并不同待遇。**

| 属性 | 段落样式 + 字符样式都声明 `true` 时 | 规则 |
| --- | --- | --- |
| `b` | 不加粗（抵消） | 层级异或 |
| `i` | 不斜体（抵消） | 层级异或 |
| `strike` | **仍有删除线** | 最具体的声明胜出 |
| `caps` | **仍全大写** | 最具体的声明胜出 |
| `smallCaps` | **仍小型大写** | 最具体的声明胜出 |
| `dstrike` | **仍有删除线** | 最具体的声明胜出 |
| `vanish` | **观察不到**（网页版把隐藏文字照常显示） | 跟 `strike` 一族 |
| `bCs` / `iCs` | **没测**（要 RTL 文本） | 跟各自本体 |

异或那条的完整形态（`b` / `i`）：

```text
有效值 = docDefaults ⊕ 段落样式层 ⊕ 表格样式层 ⊕ 字符样式层
```

层级**之间**才异或，层级内部的 `basedOn` 链是普通的"子覆盖父"。段落样式层在链里一处都没声明
时取 docDefaults 的值——每个段落都有样式（没写 `w:pStyle` 就是 Normal），样式链的根是
docDefaults，于是 docDefaults 自己抵消自己。两个已实测的推论：

- 整份文档只有 docDefaults 声明 `b=true` → **不加粗**。
- docDefaults 写 `b=true`、段落样式写 `w:b w:val="0"` → **加粗**（段落样式明说"不加粗"却是粗的）。

## 2. 现在的实现

规则**按字段选**，表在 `resolve::toggle` 的 `toggle_fields!` 宏里，一处定义同时展开字段列表、
`RunProps` 读写与规则映射。三条规则都实现了并有单测：

| `ToggleRule` | 含义 | 谁在用 |
| --- | --- | --- |
| `WordObserved` | 层级异或（上面那条） | `b` / `i` / `bCs` / `iCs` |
| `MostSpecificWins` | 最具体的声明胜出（M1 起的行为，也是 TS 的行为） | `caps` / `smallCaps` / `strike` / `dstrike` / `vanish` |
| `OddParity` | ECMA-376 §17.7.3 的字面规则 | 没人用，备查 |

来源（`RES-01`）：异或出来的值谁都没单独写过，所以是 `Provenance::Toggle { levels }`；只有一个
层级参与、且有效值就是它写的那个值时才指那一层；直接格式一票定音时是 `Direct`。

钉住它的东西：八份 fixture（`fixtures/resolve/toggle/*` 七份 + `sections/*` 一份，全部
`verified = true`）、`resolve::toggle` 的六个单测、以及 `tests/resolve_fixtures.rs` 里
"目录都登记了"的检查。想悄悄改回统一规则会直接红。

## 3. 为什么不挡进度

**实测频率（2026-09-06）**

| | |
| --- | --- |
| 语料 573 份 + 真实 Word 文档 66 份的 run 总数 | 24,177 |
| 撞上歧义的 run（两个及以上层级声明同一个 toggle） | **0** |
| `docDefaults` 里带 toggle 的真实文档 | 0 / 66 |
| 生产代码里调用 `Resolver::run` 的地方 | **0**（只有测试在用） |

三层保险：

1. **改不坏文件。** 不变式 3 禁止把投影当真相写回，`resolve` 是只读视图。最坏后果是某个 run
   显示成粗体而 Word 显示细体，动不了一个字节。
2. **现在还没接上。** `compat_ts` 发给编辑器的 `runs[].bold` 是 run 自己 `w:rPr` 里的**声明值**
   （复现 TS 的 `ParsedDoc` 形态），不走 `Resolver::run`；`tests/resolve.rs` 那 86,465 项
   `StyleDisplay` 比的是每个样式自己的链合并，也不走 toggle 规则。所以换规则前后五道差分门与
   保存语料一个数字都没变——**这不是"语料证明了规则对"，而是语料压根不覆盖这条路径。**
3. **真实文档几乎不产生这个形状。** Word 自己写文档时不会在两个层级重复声明同一个 toggle，
   这是个互操作性怪癖，主要出现在别的生成器或手搭的文档里。

**什么时候会开始影响**

- **M7 把编辑器接到 `resolve` 时**。那是这条规则第一次对用户可见。
- **编辑器自己可能造出这个形状**：用户在已加粗的段落样式上再套一个加粗的字符样式就撞上了；
  `styleUpserts` 保存选项也能写出这种样式。所以"真实文档里 0 次"不等于"永远 0 次"。

## 4. 怎么测（按价值排）

### ~~第 1 件：桌面版 Word 复核 `strike` 一族~~ —— **已完成（2026-09-07）**

> **结果：网页版的读数在 7 个测点上是错的，规则已按桌面版改写。** Office LTSC 2021（16.0.14334）实测：
> `strike` / `caps` / `smallCaps` / `vanish` 与 `b` / `i` 一样**按层级异或**，只有 `dstrike` 例外（两层都声明
> 照样画双删除线）；`docDefaults` **不参与异或**，只是没人声明时的底值——所以"整份文档只有 docDefaults
> 写 `b`"是**加粗**的（网页版读到的是不加粗）。规则表见 `resolve::toggle` 的 `ToggleRule::WordDesktop`，
> 逐句读数与截图见 `fixtures/resolve/README.md` 与 `corpus/real/_round2/TOGGLE.md`。
>
> **新的未决项**：那八份 fixture 没有 `settings.xml`，Word 以**兼容模式 12** 打开；兼容模式 15 下是否相同没测。
> 这是现在这条线上最该补的一件（做法：给 fixture 加一份带 `<w:compat><w:compatSetting … w:val="15"/>` 的
> `settings.xml`，或用 Word 另存一次再复读，然后重跑同样的二十五个测点）。
>
> 下面是当时写的步骤，留作复核方法的记录。

<details><summary>原步骤（2026-09-06 写）</summary>


`strike` / `caps` / `smallCaps` / `dstrike` 不抵消这条，与 §17.7.3 的字面冲突最大，而它来自
**Word 网页版**——那是一套独立的渲染实现，未必等于桌面版。

步骤（十分钟）：

1. 桌面版 Word 打开 `fixtures/resolve/toggle/other-toggles/doc.docx`（OneDrive 上也有一份，
   名为 `8-toggle-other-toggles.docx`）。
2. 光标放进 `strike twice`，看功能区"删除线"按不按下；再放进 `strike once` 对照。
3. 同样看 `caps twice` / `caps once` 是不是都大写、`smallcaps` / `dstrike` 两条。
4. 顺手把 `vanish twice` / `vanish once` 看一眼——桌面版可能真的把隐藏文字藏起来，那就补上了
   网页版测不到的那个角。
5. 记下 Word 版本与结果，填进 `fixtures/resolve/README.md` 的实测记录。

**判定**：

- 与网页版一致 → 把"仅在网页版测过"这条警告去掉，规则表不动。
- 不一致 → 是实现分裂。产品应对齐**桌面版**（作者用的是它），改 `toggle_fields!` 那张表，
  并把两边的差异登记进 `docs/04` §8。
- **下一次改规则应以桌面版为准，不要再拿网页版读数改了。**

</details>

### 第 2 件：把歧义频率做成常驻测量

fixture 已经把规则钉住了，缺的是"这条分支在野外到底被触发没有"。做法：把本文第 3 节那个探针
固化成一个测试，遍历 `corpus/synthetic` 与 `corpus/real`，统计有多少 run 的某个 toggle 被两个
及以上层级声明，按属性给出分布。`corpus/real` 现在是空的；往里放真实文档后这个数字会自己说话。

判定阈值：只要它一直是 0，这条规则就一直不影响产品，可以继续放着；一旦非 0，说明真实用户
文档会走到这条分支，第 1 件事的优先级立刻上升。

### 第 3 件：等编辑器接上时补对照

M7 把编辑器接到 `resolve` 的那个提交里，加一道"`resolve` 的答案与投影不会悄悄分叉"的对照。
现在加没有意义（两边根本不共用这条路径）。

### 第 4 件：让歧义在运行时可见

`Provenance::Toggle` 已经标出"这个值是异或来的"。可以再往上记一条诊断，真实用户文档撞上时
我们能知道，而不是默默渲染成 Word 不会显示的样子。

### 先别做的

- 不要再加更多 toggle 组合的 fixture。规则已经钉住，加组合只是重复。
- `vanish`（网页版观察不到）与 `bCs` / `iCs`（要 RTL 文本）先放着，等有真实需求再测。
- 不要拿网页版的新读数再改规则。

## 5. 已知未测到的角

| 角 | 为什么没测 | 现在怎么处理 |
| --- | --- | --- |
| ~~`vanish`~~ | ~~网页版观察不到~~ | **已测**（桌面版 2026-09-07：两层可见、一层隐藏 → 异或）。但 `expected.toml` 断言不了：这种段落被 R08 整段判成隐藏块，模型里没有 run |
| `bCs` / `iCs` | 要带 `w:rtl` 的阿拉伯文 / 希伯来文才看得见 | 跟各自本体 `b` / `i` 走 |
| ~~桌面版 Word~~ | ~~只在网页版测过~~ | **已测**（2026-09-07，LTSC 2021）；规则已按它改写 |
| **兼容模式 15** | 八份 fixture 没有 `settings.xml`，Word 以兼容模式 12 打开 | **未测**，现在这条线上最该补的一件 |
| Microsoft 365 | 两轮实测都是 LTSC 2021 | 未测 |
| 表格样式的条件格式 × 字符样式 | 只测了表格样式 × 段落样式 | 按同一条异或规则处理 |
