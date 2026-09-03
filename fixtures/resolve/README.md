# fixtures/resolve

`resolve/` 的 Word 实测校准 fixture（`spec/07-resolve.md` RES-12、`spec/11-testing.md` TEST-08）。

```
fixtures/resolve/<area>/<case>/
  doc.docx            # 真实 Word 文档，尽量最小
  expected.toml       # [[run]] para = 3, run = 1, bold = true, source = "ParaStyle:Heading1"
  README.md           # Word 版本、观察方法（截图 / 属性面板）、来源
```

每条 `RES-*` 至少一个 fixture，`RES-04` toggle 至少五个。fixture 与规则冲突时以 Word 为准修改规则并在 README 记录。M5 前建齐。
