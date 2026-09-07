# 09 · 桌面 Word 第三轮（收尾轮）任务

> **读者**：前两轮的 Windows 侧代理。这份文档自含。**产出**：`real-word-round3-<yyyymmdd>.zip`。
> **这是最后一轮**，目标是把所有"只有真实 Word 能回答"的问题一次问完。
> 环境仍用 **Office LTSC 2021**（前两轮同一台）；**不做 Microsoft 365 对照**（项目负责人决定：这个版本跑通即可）。
> 有疑问按字面做，写进 `README.md` 的「未完成 / 存疑」，不要自行改规格。

## 0. 前两轮的结果，以及这一轮为什么这么排

两轮交付都已接入（`corpus/real` 现有 194 份真实文档，九道差分门全 0，502 个测试通过）。第二轮你们发现的问题**全部确认是本引擎的真 bug 并已修好**：

| 你们的发现 | 根因 | 已修 |
| --- | --- | --- |
| 9 份编辑后文档弹恢复提示（全是原生墨迹底稿） | 新绘图的 `wp:docPr/@id` 只在"生效的 MCE 分支"里找最大值，撞上原生墨迹藏在 `mc:Choice Requires="wpi"` 里的 `id="1"` | 改成扫全部节点；本轮请复验 |
| 散点 / 气泡图 `chartdata` 值没变 | 那些图表的值在 `c:yVal` 里，我们的写侧只认 `c:val` | 已补 `c:yVal` |
| 无标题图表加不上标题 | 那份图表连 `c:title` 元素都没有，标题请求被静默丢弃 | 现在会新建一个 |
| 新图实际 108×54 pt，清单写 144×72 pt | **我们清单写错了** | 清单已改 |
| `mergecells` 前后 XML 相同 | **我们挑的那一行本来就已合并** | 改成挑未合并的行 |

`dstrike` 两层都开这条反常读数，我核对了你们的字体对话框截图，也验证了 fixture 本身确实两层都声明——**结论采信**，规则里 `dstrike` 单独走"最具体胜出"。

这一轮四件事，按价值排：**A** 是新的重头（下一个里程碑 M7 的验收门直接依赖它），**B** 是复验 + 新覆盖，**C** 关掉 toggle 最后一个未决项，**D** 补两个小缺口。

## 1. 交付物

```
real-word-round3-<yyyymmdd>/
  README.md          Word 精确版本、Windows 版本、日期、方法；未完成 / 存疑
  REVISIONS.md       任务 A：每个 case 一段，写清做了什么、Word 显示什么（§2.3 的表头）
  revfix/            任务 A 的文档（§2 的目录结构）
  EDITED3.md         任务 B：1544 份的读数（§3.2 的表头）；另附 edited3-results.json
  TOGGLE15.md        任务 C：兼容模式 15 下的 25 个测点（§4 的表头）
  ink2/  comments2/  任务 D 的文档
  _resaved/  screenshots/  _scripts/  _previews/（只放 PDF）
```

## 2. 任务 A · 让 Word 自己做「接受 / 拒绝全部修订」等操作（最重要）

**为什么**：下一个里程碑要实现"生成修订"与"接受 / 拒绝修订"。判断我们做得对不对，最硬的标准是
**拿 Word 自己的结果做对照**：同一份带修订的文档，Word 点「接受所有修订」得到什么，我们的引擎就该得到什么。
你们只要把 Word 的结果**另存出来**，我们这边就能自动比对，不需要再来一轮人工核对。

### 2.1 修订三件套（至少这 3 个 case，每个 4 份文件）

每个 case 一个目录 `revfix/<case>/`，里面 4 份：

| 文件 | 怎么来 |
| --- | --- |
| `base.docx` | **先关掉修订**，把"原始正文"打好，保存。这是没有任何修订的底稿 |
| `tracked.docx` | 打开 `base.docx`，**开启修订**（审阅 → 修订），做下表的操作，**另存为**这个名字。不要接受也不要拒绝 |
| `accepted.docx` | 打开 `tracked.docx`，审阅 → 接受 → **接受所有修订**，**另存为**这个名字 |
| `rejected.docx` | 重新打开 `tracked.docx`，审阅 → 拒绝 → **拒绝所有修订**，**另存为**这个名字 |

> 关键：`accepted` / `rejected` 必须从**同一份** `tracked.docx` 分别得到（不要在同一个窗口里先接受再撤销再拒绝）。
> 每份都是 Word 保存的，保存后不要再打开保存。作者名用「作者甲」；`revfix/tracked-two-authors/` 那个 case 用两个作者。

| case 目录 | `base.docx` 的正文 | 在 `tracked.docx` 里做什么 |
| --- | --- | --- |
| `revfix/run-edits/` | `before 前文` / `第一句原文。第二句原文。第三句原文。` / `after 后文` | ① 在第一句后插入 `新增的一句。` ② 删掉「第二句原文。」 ③ 把「第三句」三个字改成加粗 + 红色 |
| `revfix/para-split-merge/` | `before 前文` / `甲段落的文字。` / `乙段落的文字。` / `after 后文` | ① 把「甲段落」在中间某处按回车拆成两段 ② 把「乙段落」与它下一段合并（在乙段末尾按 Delete） ③ 把拆出来的第二段设为居中并加 2 字符首行缩进 |
| `revfix/table-and-move/` | `before 前文` / 一个 2 列 × 3 行表格（格子写 A1…B3） / `可移动段落。` / `after 后文` | ① 表格第 2 行前插一行 ② 删掉最后一行 ③ 合并第 1 行两格 ④ **开启「跟踪移动」**（审阅 → 修订选项里确认），把「可移动段落。」整段剪切并粘贴到 `after 后文` 之前 |
| `revfix/tracked-two-authors/` | `before 前文` / `原始正文。` / `after 后文` | 作者甲插入一句；改用户名为作者乙，再插入一句并删掉「原始正文。」的后半句 |

自检（复制一份解包看，别动原件）：`tracked.docx` 里应有 `<w:ins`、`<w:del`（内含 `w:delText`）、`<w:rPrChange>` / `<w:pPrChange>`、
表格 case 有 `<w:trPr>` 里的 `<w:ins>` / `<w:del>` 与 `<w:tcPrChange>`、移动 case 有 `<w:moveFrom>` / `<w:moveTo>` 与
`moveFromRangeStart` / `moveToRangeStart`；`accepted.docx` / `rejected.docx` 里这些标记应当**一个都不剩**。

### 2.2 「Word 自己做这个操作」的前后对照（4 组，每组 2 份）

同样的道理：M7 要实现"插入 / 删除分节符"与"改绘图叠放次序、移动缩放绘图"。与其等我们写完再请你们看，不如现在
把 **Word 做同一件事的前后两份**留下来，我们直接比对形态。**这几组不要开修订。**

| 目录 | `before.docx` | 在 `after.docx` 里做什么 |
| --- | --- | --- |
| `revfix/sect-insert/` | `before 前文` / `第一节正文。` / `第二节正文。` / `after 后文`（单节） | 光标放在「第二节正文。」开头，布局 → 分隔符 → **下一页**；再把新的第 2 节设为横向 |
| `revfix/sect-delete/` | 两节：第 1 节纵向、第 2 节横向（用下一页分节符），各一段正文 | 在草稿视图里选中那条分节符标记，按 Delete 删掉它 |
| `revfix/z-order/` | 三张同一张小图的浮动图片，错位叠放（同第二轮 `image2/image-z-order.docx` 的做法） | 把最底下那张「置于顶层」 |
| `revfix/move-resize/` | 一张四周型环绕的浮动图片 + `before` / `after` 两段 | 把图片往右下拖约 2 cm，再把宽高缩到原来的一半（按住 Shift 拖角点保持比例） |

自检：`sect-insert/after.docx` 应比 `before` 多一个 `<w:sectPr>` 且新节带 `w:orient="landscape"`；`sect-delete/after.docx` 应只剩一个
`<w:sectPr>`；`z-order/after.docx` 的三个 `relativeHeight` 次序应与视觉一致；`move-resize/after.docx` 的 `wp:posOffset` 与
`wp:extent` 都变了。

### 2.3 `REVISIONS.md` 表头

```
| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
```

`accepted` / `rejected` 两份请特别写清**正文最终是什么样**——那是我们比对的依据。例如 `run-edits`：
接受后应是「第一句原文。新增的一句。第三句原文。」（第三句加粗红色），拒绝后应回到 `base.docx` 的三句原文。

## 3. 任务 B · 复验 + 新覆盖：Word 打开本引擎写出的 1544 份

输入 zip 的 `edited/` 有 **1544 份**（12 种编辑 × 127 份真实底稿，比第二轮多了 17 份 M7 底稿派生的那批）与
`MANIFEST.md` / `manifest.json`。做法与第二轮完全相同（对象模型批量开 + 抽检目视），结果写进 `EDITED3.md` /
`edited3-results.json`。

### 3.1 必须单独确认的两组

- **第二轮失败的 9 份**：`ink-pen--{newimage,newchart,ink}`、`ink-highlighter--{newimage,newchart,ink}`、
  `ink-to-shape--{newimage,newchart,ink}`。**这一轮它们应当正常打开、没有恢复提示**；请在 UI 里逐份打开确认并截图。
  若仍有提示，那是我们没修对，请务必写清提示原文。
- **第二轮的 4 份图表 mismatch**：`chart-no-title--chartdata`（现在应能看到标题「标题已改 rsword」）、
  `chart-scatter--chartdata` / `chart-scatter-lines--chartdata` / `chart-bubble--chartdata`
  （第一系列的值现在应是 11 / 21 / 31）。同样 UI 逐份确认。

### 3.2 `EDITED3.md` 表头

与第二轮一致，末尾加一列：

```
| 文件 | open | compat | 恢复提示 | marker / shapes / chart 读数 | 抽检：看到什么（未抽检写 -） | 与期望是否一致 | 与第二轮相比（新增 / 修复 / 仍失败 / 不变） |
```

抽检范围：每种编辑 3 份（尽量覆盖不同域），**外加 §3.1 的 13 份**，**外加**新增的那 17 份 M7 底稿派生的样本里每种编辑各 1 份。

## 4. 任务 C · 兼容模式 15 下重测 toggle（关掉最后一个未决项）

第二轮你们测的八份 fixture 没有 `settings.xml`，Word 以**兼容模式 12** 打开。真实文档都是 15，所以必须确认
模式不影响结论。输入 zip 的 `fixtures/` 里现在每份有两个文件：

- `<名字>.docx`：与第二轮**逐字节相同**的那份（兼容模式 12）。
- `<名字>-compat15.docx`：只多了一个声明 `compatibilityMode = 15` 的 `settings.xml`，其余完全一样。

请对 **`-compat15` 那八份**重跑第二轮 `TOGGLE.md` 的**同样 25 个测点**，方法照旧（对象模型读两次 + 页面/功能区证据；
`strike twice` / `dstrike twice` / `vanish once` 三处仍请附字体对话框截图）。结果写 `TOGGLE15.md`：

```
| 文件 | 句子 | 属性 | 兼容模式 15 下开/关 | 与第二轮（模式 12）是否一致 |
```

**另外做一次交叉验证**：把 `toggle-other-toggles.docx`（模式 12 那份）在 Word 里用「文件 → 信息 → 转换」升级到最新格式，
另存为 `_resaved/toggle-other-toggles-converted.docx`，再读一遍那 12 个测点，看与 `-compat15` 的读数是否一致。
如果三者（模式 12 / 我们造的模式 15 / Word 转换出来的）读数一致，这条就彻底关掉了。

## 5. 任务 D · 两个补缺

| 目录 / 文件 | 怎么做 | 自检 |
| --- | --- | --- |
| `ink2/ink-to-shape-2.docx` | 绘图选项卡 → 打开「墨迹转形状」→ **一笔**画一个闭合的圆（尽量圆、一笔到底不要抬笔），等 Word 自动转成形状 → 保存 | 应出现 `<wps:wsp>` 且 `prst` 是 `ellipse` / `flowChartConnector` 之类，**没有** `w14:contentPart`。一笔画不成就多试几次；确实转不出来就保留试件并写明（第二轮八笔转成了八边形，这次务必**一笔** ） |
| `comments2/comment-nesting.docx` | 三条根批注；对第一条**在 Word 界面里**点批注气泡上的「答复」按钮加一条回复，再对**那条回复**点「答复」加第二层回复；把整个线程标为已解决 | `commentsExtended.xml` 里应有**两层不同**的 `w15:paraIdParent`（回复 2 指向回复 1，而不是都指向根）。第二轮用 COM 的 `Replies.Add` 做不出两层，所以这次请用界面按钮 |

## 6. 顺序与完成标准

顺序：**A → B 的 §3.1 十三份 → C → B 的其余 → D**。任务 A 最重要，做不完其余的也要先保证 A 完整。

完成标准：`revfix/` 每个 case 的文件齐全并通过自检、`REVISIONS.md` 写清接受 / 拒绝后的最终正文；`EDITED3.md` 1544 行齐全
（对象模型那一层每份必有）且 §3.1 的 13 份有 UI 结论；`TOGGLE15.md` 25 行 + 交叉验证结论；任务 D 两份或写明未达成；
`README.md` 有精确版本与未完成清单；zip 里没有 `~$` 锁文件。
