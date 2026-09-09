# `fixtures/revisions` · 修订的 Word 对照件（`spec/18` M7 门第 3 条）

每个目录是**同一份文档的四态**，全部由桌面 Word 自己保存（2026-09-07，Office LTSC 2021 16.0.14334，
制作与逐句观察见 `corpus/real/_round3/REVISIONS.md`）：

| 文件 | 是什么 |
| --- | --- |
| `base.docx` | 关掉修订打好的原始正文 |
| `tracked.docx` | 在 `base` 上**开着修订**做完操作后另存；带 `w:ins` / `w:del` / `*Change` 标记 |
| `accepted.docx` | 打开 `tracked` 后 Word「接受所有修订」再另存 |
| `rejected.docx` | 从**同一份** `tracked` 出发，Word「拒绝所有修订」再另存 |

M7 拿它做 oracle：我们对 `tracked.docx` 做 `AcceptAll` / `RejectAll`，`ModelFingerprint` 应分别与
`accepted.docx` / `rejected.docx` 相等。

| 目录 | `tracked` 里做了什么 | 覆盖的修订种类 |
| --- | --- | --- |
| `run-edits` | 插一句、删一句、把一处改成加粗红色 | `w:ins`、`w:del`、`w:rPrChange` |
| `para-split-merge` | 拆一段、合并两段、给拆出来的段落设居中 + 首行缩进 | `w:ins`、`w:del`、`w:pPrChange` |
| `table-and-move` | 表格插一行、删一行、合并首行两格；跟踪移动整段 | `w:trPr/w:ins`、`w:trPr/w:del`、`w:tcPrChange`、`w:tblGridChange`、`w:tblPrExChange`、`w:moveFrom` / `w:moveTo` + 成对范围标记 |
| `tracked-two-authors` | 作者甲插一句；作者乙插一句并删半句 | 两个 `w:author` 的 `w:ins` / `w:del` |

## 已复算的性质（写测试时可以直接当断言）

- 四个 case 的 `rejected.docx` 与 `base.docx` **逐段可见文字相同**；`accepted.docx` 与 `base` 不同；
  `accepted` / `rejected` 里**一个修订标记都不剩**。
- **`ModelFingerprint` 必须忽略 run 边界**：`run-edits` 拒绝格式修订后，Word 把那一段留成
  `第一句原文。第二句原文。` + `第三句` + `原文。` 三个 run（字符与 `base` 完全一致，只是没有合并回去）。
  按 run 逐个比较会误判。
- **Word 的「拒绝所有修订」不会撤销单元格合并**：`table-and-move/rejected.docx` 行数与文字都回到了
  `base`（3 行、A1…B3），但首行仍然是合并的一格（`w:gridSpan="2"`）。这是 Word 的实际行为，M7 要么照做、
  要么把差异登记进 `docs/04` §8。
