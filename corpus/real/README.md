# corpus/real

每个用例一个目录：`<source>/<case>/doc.docx` 与 `case.toml`：

```toml
source = "libreoffice sw/qa/extras/ooxmlexport/data/tdf123456.docx"
license = "MPL-2.0"
word_version = "Word 365 16.80 (mac)"
assertions = [
  "blocks[0].type == 'paragraph'",
]
```

每个用例至少一个断言与一次往返（TEST-04）。LibreOffice 语料需确认许可允许复制后再加入。
