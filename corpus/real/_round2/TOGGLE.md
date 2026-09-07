# 桌面 Word Toggle 复核

日期：2026-09-07，Asia/Shanghai。Word / Office LTSC Professional Plus 2021 x64，16.0.14334.20848；当前环境详情见 `environment.json`。本表覆盖 8 份 fixture 的 25 个测点。

本轮 fixture 在 Word 中以兼容模式打开。七份分文件 JSON 实际记录 `Document.CompatibilityMode=12`；`toggle-para-and-char.docx` 的早期读数脚本未写出该数值，其页面截图标题栏明确显示“兼容性模式”，这里不补造精确数值。这些 fixture 的模式与 B2-a 真 Word 底稿的 15 不同；本次未转换兼容模式后重测，结论应保留版本和模式范围。

每个测点保留两次真实对象模型读数，并结合实际 Word 页面、功能区及关键字体对话框核实。一般字体属性在句子内部放置折叠光标后读取 `Selection.Font`；脚本在各次读数前移开选区。Hidden 属性使用完整句子选区读取两次，保持 `ShowHiddenText=false`、`ShowAll=false`。页眉测点定位实际第 2 页后读取第二节默认页眉两次。

方法与任务书的差别：两次重复读数主要通过对象模型完成；并未对每一句手动打开两次字体对话框，也没有为每一次按钮状态单独保存截图。字体对话框截图只用于明确列出的 `strike twice`、`dstrike twice`、`vanish once`。页面与功能区的检查和截图作为辅助 UI 证据。下表的“开/关”同时有实际 Word 读数及所列 UI 证据。

| 文件 | 句子 | 属性 | 桌面 Word 里开/关（或页眉文字） | 与网页版结论是否一致 |
| --- | --- | --- | --- | --- |
| `toggle-other-toggles.docx` | `i twice` | 倾斜 i | 关；两次 0 / 0；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `i once` | 倾斜 i | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `strike twice` | 删除线 strike | 关；两次 0 / 0；[页面](screenshots/toggle-other-render-0.jpg)、[字体对话框](screenshots/toggle-strike-twice-font-1-1.jpg) | 不一致；网页版为开 |
| `toggle-other-toggles.docx` | `strike once` | 删除线 strike | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `caps twice` | 全部大写 caps | 关；两次 0 / 0；[页面](screenshots/toggle-other-render-0.jpg) | 不一致；网页版为开 |
| `toggle-other-toggles.docx` | `caps once` | 全部大写 caps | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `smallcaps twice` | 小型大写 smallCaps | 关；两次 0 / 0；[页面](screenshots/toggle-other-render-0.jpg) | 不一致；网页版为开 |
| `toggle-other-toggles.docx` | `smallcaps once` | 小型大写 smallCaps | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `dstrike twice` | 双删除线 dstrike | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg)、[字体对话框](screenshots/toggle-dstrike-twice-font-1.jpg) | 一致 |
| `toggle-other-toggles.docx` | `dstrike once` | 双删除线 dstrike | 开；两次 -1 / -1；[页面](screenshots/toggle-other-render-0.jpg) | 一致 |
| `toggle-other-toggles.docx` | `vanish twice` | 隐藏 vanish | 关；两次 0 / 0；句子仍可见；[页面](screenshots/toggle-other-render-0.jpg) | 网页版未知，不能判一致性 |
| `toggle-other-toggles.docx` | `vanish once` | 隐藏 vanish | 开；两次 -1 / -1；句子被隐藏；[页面](screenshots/toggle-other-render-0.jpg)、[字体对话框](screenshots/toggle-vanish-once-font-1.jpg) | 网页版未知，不能判一致性 |
| `toggle-para-and-char.docx` | `para b + char b` | 加粗 b | 关；两次 0 / 0；[页面](screenshots/toggle-para-and-char-0.jpg) | 一致 |
| `toggle-para-and-char.docx` | `para b only` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-para-and-char-0.jpg) | 一致 |
| `toggle-docdefaults-and-para.docx` | `docDefaults b + para b` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-docdefaults-and-para-0.jpg) | 不一致；网页版为关 |
| `toggle-docdefaults-and-para.docx` | `docDefaults b only` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-docdefaults-and-para-0.jpg) | 不一致；网页版为关 |
| `toggle-docdefaults-and-para-off.docx` | `docDefaults b + para b=0` | 加粗 b | 关；两次 0 / 0；[页面](screenshots/toggle-docdefaults-and-para-off-0.jpg) | 不一致；网页版为开 |
| `toggle-docdefaults-and-para-off.docx` | `docDefaults b only` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-docdefaults-and-para-off-0.jpg) | 不一致；网页版为关 |
| `toggle-based-on-two-levels.docx` | `basedOn b + derived b` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-based-on-two-levels-0.jpg) | 一致 |
| `toggle-based-on-two-levels.docx` | `base b only` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-based-on-two-levels-0.jpg) | 一致 |
| `toggle-table-first-row.docx` | `table firstRow b + para b` | 加粗 b | 关；两次 0 / 0；[页面](screenshots/toggle-table-first-row-0.jpg) | 一致 |
| `toggle-table-first-row.docx` | `table body + para b` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-table-first-row-0.jpg) | 一致 |
| `toggle-direct-off.docx` | `direct b=0 over style b` | 加粗 b | 关；两次 0 / 0；[页面](screenshots/toggle-direct-off-0.jpg) | 一致 |
| `toggle-direct-off.docx` | `style b, no direct` | 加粗 b | 开；两次 -1 / -1；[页面](screenshots/toggle-direct-off-0.jpg) | 一致 |
| `sections-inherit-default.docx` | `第二页页眉` | 第二节默认页眉 | 第一节页眉；两次一致；LinkToPrevious=true；[页面](screenshots/toggle-sections-inherit-default-0.jpg) | 一致 |

## 读数边界

`caps once` 的常规查找遇到 `Word Find returned an unexpected range`。补测直接使用 Word 中已定位的 Range 48..57，实际返回文本为 `CAPS ONCE`，在位置 50 折叠光标两次读到 `AllCaps=-1`。原始补测为 `_scripts/toggle-caps-direct.json`；分文件 JSON 保留原查找错误及补测来源，没有把错误当成关闭。

`vanish once` 不能用隐藏句子内的折叠光标值定案：本次曾观察到折叠光标返回 0，而完整选区 126..137 的 `Selection.Font.Hidden` 为 -1。最终表使用重新执行的两次整句选区读数，且字体对话框“隐藏”复选框已勾选、关闭隐藏文字显示后页面不显示该句；`vanish twice` 的整句两次读数均为 0，页面仍显示它。

第 2 页的页眉两次均为“第一节页眉”。截图同时显示“页眉 - 第 2 节”和“与上一节相同”，对象模型 `LinkToPrevious=true`，与网页版记录一致。

## 与网页版比较

25 个测点中，16 个与已记录网页版结果一致，7 个不同，2 个 Hidden 测点没有可比较的网页版结论。不同项是 `strike twice`、`caps twice`、`smallcaps twice`，以及两份 docDefaults fixture 的四句。桌面本次 `dstrike twice` 仍开启，不能把删除线一族全部概括成同一规则；basedOn、docDefaults 与段落/字符层的 b 也必须按逐句结果保留。

原始与补充读数保存在 `_scripts/toggle-read/round2-20260907-live/*.json`；每行有 `samples`、`readMethod`、`uiObservation` 和实际 `uiEvidence` 路径。该目录现已补入最终 UI 证据，`uiVerified=true` 的含义是已有页面/功能区或指定字体对话框证据支持该值，并不表示逐句进行了两次人工对话框检查。网页版参照取自输入的 `fixtures/README-fixtures.md` 逐句实测表，而不是任务书的简写概括。
