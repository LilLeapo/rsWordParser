# 20. 模板填充作业法（真实标书实战总结，2026-09-10）

面向**用 rsword 往 Word 模板里填内容**的调用方（Agent 或人）。全部规则都来自一次真实作业
（滨海新区数字化转型申报书，1485 段、37 个 zip 条目）踩出来的坑，每条都在那份文件上验证过。

前置：`docs/18-cli.md`（CLI）、`docs/19-mcp.md`（MCP）、`docs/17-agent-edit.md`（编辑操作）。

---

## 0. 一句话

**文字对不对，和格式对不对，是两件独立的事，必须分别验。**

那次作业写了 394 条断言，全部在查文字，结果 94 个段落丢了 `w:rPr`、2 处下划线蔓延，
一条断言都没报警，直到人看截图才发现。**格式漂移不会自己报错。**

---

## 1. 参数分两层（最常见的报错）

| 层级 | 放什么 |
| --- | --- |
| **顶层** | `sessionId`、`limit`、`maxBytes`、`cursor`、`expectedVersion`、`resultShape` |
| **`options` 里** | 该工具自己的业务参数（`text` 是 `scope`/`blockRange`/`flow`，`open` 是 `path`） |

`options` 声明了 `additionalProperties: false`，把预算参数塞进去会被拒：

```
BIND_BAD_ARGUMENT  Additional properties are not allowed ('limit' was unexpected)
```

见到 `BIND_BAD_ARGUMENT` 往**参数结构**上找；见到 `AGENT_BUDGET_TOO_SMALL` 才是真的超预算，
而且它会把 `minBytes` / `minLimit` 直接告诉你，照填即可。

默认值很小：`limit` 8000、`maxBytes` 24000。稍大的文档必须显式调。

**分页不是可选的**：`hasMore=true` / `truncated=true` 时必须用 `nextCursor` 续读。
`range.totalBlocks` 会给总数，可以用来自查有没有读全。那次作业漏掉最后一张表，就是
读到 `truncated: true` 却没续读。

---

## 2. 写之前：格式基准要三源交叉

**空段落的格式在模板里是欠定的。** 实测那份模板的三个候选基准互相矛盾：

| 候选基准 | 在 `0B9900EA`（项目效益预期）上给出 | 判定 |
| --- | --- | --- |
| 段落标记 `w:pPr/w:rPr` | `黑体` | **错**（编辑残留） |
| 同 `pStyle` 的邻居 | 后面 4 个同 `pStyle=2` 的邻居也全是空的黑体 | 问不出 |
| 全文同 `pStyle` 的众数 | 黑体 13 / 楷体_GB2312 9 / 仿宋 4 / 方正小标宋 4 | 没有众数 |
| **同构位置的已填样例** | 「（七）项目进度」的正文 = `仿宋_GB2312 sz32` | **对** |

**段落标记的 `rPr` 是模板作者敲过又删掉的残留，不是他对正文的意图。** 但换成 pStyle 也不行——
正确答案要靠**结构平行**推：「（八）」的正文格式抄「（七）」的正文。

### 作业规则

1. 对每个待填位置，取三个来源：**段落标记 rPr**、**同构已填样例的 run props**、**前后最近非空正文的 run props**。
2. **三者一致 → 自动填。** 那份模板 58 个空段里 44 个属于这类。
3. **不一致 → 停下来问人，不要自己挑一个。** 剩下 14 个（24%）属于这类，工作量完全可控。

这些数据一次扫描就能拿到，成本很低：`model` 的输出里每段都带 `props.rpr`（段标记）和
`inlines[].props`（每个 run 的字体/字号/语言），读 8 个相邻段落的完整格式约 **2000 tokens**。

### 模板复用的话，先洗一遍

同一套模板要反复填，最划算的是**一次性把空段的段落标记格式规范化**（按同构位置统一成正文格式），
洗完以后每次填充都不用再判断。比每次填都做 14 次人工裁定省得多。

---

## 3. 写的时候：避开 `replaceText` 的整 run 陷阱

**`replaceText` 在 `find` 恰好等于某个 run 的全部文本时会静默改版式**（`docs/04` §8.0.1，引擎缺陷）：

| 命中方式 | 结果 |
| --- | --- |
| 部分命中（`find` 是 run 文本的子串） | `w:rPr` 完整保留，就地改 `w:t` ✅ |
| 整 run 命中，段内无其他 run | `w:rPr` **整块丢失**，退到 `docDefaults` |
| 整 run 命中，段内有相邻 run | 文本落进**邻居的 `w:t`**，冒用邻居格式，两个 run **被合并** |

危险在于**调用方无法预知哪个 `find` 会正好等于某个 run 的全文**——取决于模板作者怎么切 run。
同一个 `find` 在一份文档里安全、在另一份里毁版式，不报错、不回滚。

### 作业规则

- **写入一律走原生三连**，不要依赖 `replaceText` 的隐式格式继承：

  ```
  deleteRange(from, to)
  insertText(at, text)
  setRunProps(from, from+len(text), <第 2 节选定的基准>)
  ```

  `setRunProps` 的 patch 直接用模型里的 run props 形态（`{"fonts":{"eastAsia":"仿宋_GB2312"},"size":24}`）。

- **`patch` 的三个臂**（`spec/21-bind.md`）：`Keep` = 键缺席，`Unset` = **`null`**，`Set(v)` = 值本身。
  要**删掉**某个属性（例如去掉下划线）写 `{"underline": null}`，不要写 `{"underline":{"val":"none"}}`——
  后者会留下 `<w:u w:val="none"/>`，和模板里「根本没有 `w:u`」不是一回事。

- **空单元格**用该段 `w:pPr/w:rPr` 只能当兜底，且必须过第 2 节的三源交叉。

---

## 4. 写完必须扫格式漂移

### `preview` 挡不住

`preview` 给的是**文本 diff**（`before`/`after` 文字 + part 哈希），**对格式变化是瞎的**。
那三处版式问题 preview 全都显示"正常"。它能挡文字写错，不能挡格式漂移。

### 有效的扫法：清空文字后 diff 骨架

把两份文件的 `w:t` / `w:instrText` / `w:delText` 文本全部清空、去掉 `xml:space`，再 diff
整个 `document.xml`。**剩下的任何差异必然是格式或结构差异。** 一次就能把全部问题捞出来：

```python
def skeleton(path):
    r = ET.fromstring(zipfile.ZipFile(path).read("word/document.xml"))
    for t in r.iter():
        if t.tag in (W+"t", W+"instrText", W+"delText"): t.text = ""
        t.attrib.pop("{http://www.w3.org/XML/1998/namespace}space", None)
    # 递归输出 <标签 属性排序> 每行一个元素
```

那次一跑就出来：`+299 rPr / +298 sz`（正常，新 run）、`-11 u / +1 u`（**下划线净减 10**，
两处蔓延）、`-1 sym`（一处符号元素丢失）、`+6 b`（粗体，来自模板段标记）。

### 逐段配对用 `w14:paraId`

Word 给每个段落分配的 `w14:paraId` 全局唯一、编辑后不变，**是唯一可靠的配对键**。
按它配对后逐段比 `pPr` 和每个 run 的 `rPr`。新建的段落没有 paraId，单独看。

### 必查的五项

| 项 | 为什么 |
| --- | --- |
| **裸 run 数**（无 `rPr`） | 抓 `replaceText` 丢格式 |
| **run 数变化** | 减少 = run 被合并，格式可能被邻居吞掉 |
| **`w:u` / `w:b` / `w:highlight` 全局计数** | 一个数字就能暴露蔓延；净减不代表变少，可能是合并后蔓延 |
| **`pPr` 是否变化** | 段落级格式不该被文字填充碰到 |
| **zip 条目 CRC** | 只有被编辑的 part 该变；其余必须逐字节相同 |

最后一项用 `rsword check` 加 zip 对照：

```sh
rsword check out.docx --json --limit 100000 --maxBytes 400000
# noEditSaveIdentity / dirtyPropagation / engineInvariantDiagnostics 必须全 true
```

---

## 5. 填完了，版面还得看

有些问题不是格式漂移，是**模板设计遇上真实内容**。引擎和调用方都没错，但页面就是不能看。
这三类要靠人（或渲染截图）发现，XML 层面查不出来：

| 症状 | 机理 | 修法 |
| --- | --- | --- |
| 表格文字被裁 | 单元格有 `w:noWrap` + 固定 `tcW`，内容比列宽长就裁掉（**文字在 XML 里是完整的**） | 去 `noWrap` 允许折行（首选，标书最怕内容看起来缺失）；或那几列降一号字；或从宽列借宽度 |
| 短行莫名换行 | 模板用 `w:ind w:firstLineChars` 硬缩进做"右对齐"，填的内容比占位符长就溢出 | 去掉数字两侧的空格；或降 `firstLineChars`；或改成真正的 `w:jc="right"` |
| 某段看起来加粗 | 不是 `w:b`，是**字体是 `黑体`**（来自模板段标记），周围正文是 `仿宋_GB2312` | 改成与周围正文一致 |

**中文标书的排版惯例**：数字与「年月日」「万元」「%」之间**不加空格**。
那次作业到处写成 `2024 年 5 月 20 日`、`3200 万元`、`96.2%`，多出来的空格直接导致了换行溢出。

---

## 6. 检查清单

写之前：

- [ ] 参数分层对了（预算在顶层，业务参数在 `options`）
- [ ] 读全了（`truncated` / `hasMore` 都续读到底，块数对上 `totalBlocks`）
- [ ] 每个待填位置做了三源交叉，分歧项已人工裁定
- [ ] 写入用原生三连，不靠 `replaceText` 的隐式继承

写之后：

- [ ] 骨架 diff（清空文字后比 `document.xml`），差异逐条有解释
- [ ] 裸 run 数、run 数变化、`w:u`/`w:b` 计数、`pPr` 变化 —— 四项都核过
- [ ] `rsword check` 三条不变式全 true
- [ ] zip 只有被编辑的 part 的 CRC 变化
- [ ] **人眼看过渲染结果**（第 5 节那三类只能这样发现）

---

## 附：这次实战的账

| | |
| --- | --- |
| 文档规模 | 1485 段、181 个顶层块、37 个 zip 条目、698 KB |
| 写入 | 291 个空单元格 + 119 处片段替换 + 1 个新建段落 |
| 第一轮交付的缺陷 | 94 段丢 `w:rPr`（只验文字没验格式，全部漏过） |
| 修复后残留 | 2 处下划线蔓延（run 合并所致，报告作者未发现，骨架 diff 一次捞出） |
| 模板自身的坑 | 58 个空段里 14 个段标记格式与上下文矛盾；703 处 `noWrap`；多处硬编码 `firstLineChars` |
| 引擎真缺陷 | 1 个（`InsertText` 边界插入不继承被删 run 的 `rPr`，见 `docs/04` §8.0.1） |

**引擎缺陷只有 1 个，其余全是调用方法与模板适配问题**——这也是本文存在的理由。
