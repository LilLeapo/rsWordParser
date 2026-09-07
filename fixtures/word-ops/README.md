# `fixtures/word-ops` · Word 自己做同一个操作的前后对照

`before.docx` → Word 里做一件事 → `after.docx`，两份都由桌面 Word 保存（2026-09-07，Office LTSC 2021
16.0.14334；操作记录见 `corpus/real/_round3/REVISIONS.md`）。M7 实现对应的编辑操作后，拿我们的输出与
`after.docx` 比形态，不需要再请人打开看一眼。**这几份没有开修订。**

| 目录 | 操作 | 已复算的形态变化 |
| --- | --- | --- |
| `insert-next-page` | 在第二段开头插「下一页」分节符，并把新的第 2 节设为横向 | `w:sectPr` 1 → 2；新节 `w:orient="landscape"`、`w:pgSz` 16838 × 11906 |
| `delete-break` | 删掉两节之间的分节符 | `w:sectPr` 2 → 1，**留下的是后一节的页面设置**（横向 16838 × 11906）——合并后由后一节的属性接管 |
| `z-order` | 把最底下那张浮动图片「置于顶层」 | 该锚的 `relativeHeight` 251658240 → 251661312（升到最高）；三张图的 `posOffset` 与 `extent` 都没动 |
| `move-resize` | 把浮动图片往右下移约 2 cm，再等比缩到一半 | `posOffset` (2512, 314346) → (721360, 1033780)；`extent` 1524000 × 762000 → 762000 × 381000 |
