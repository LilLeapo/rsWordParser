# corpus/real · 真实 Word 语料

制作说明（给 Windows 侧代理的自含文档）在 **`docs/07-real-word-corpus.md`**：为什么要、怎么做、放哪、记什么、怎么自检。

布局（与 `docs/07` §2 一致）：

```
corpus/real/
  README.md                 本文件
  OBSERVED.md               每份文档一行：Word 版本 / 制作方式 / 步骤要点 / 在 Word 里看到什么（人眼 oracle）
  ROUNDTRIP.md              任务 B：用 Word 打开 _roundtrip/ 里本引擎写出的文件，记有无修复提示
  <域>/<名字>.docx          源文件（Word 写出的原件，随仓库提交，之后不再改动）
  <域>/<名字>.expected.json 由 tools/export-golden 用 TS parseDocx 录制（macOS 侧生成，参考不是权威）
  _roundtrip/               本引擎生成、等 Word 核对的样本（tests/roundtrip_samples.rs 产出）
  _scripts/                 Windows 侧用过的 COM / PowerShell 脚本（可复现）
```

接入（macOS 侧）：`cargo run -p diff-parse -- --corpus corpus/real`（参考差分），`tests/save.rs` 的往返与编辑保真扫描覆盖
`corpus/real/**/*.docx`（字节相同、其他条目 CRC 不变——这两条是门）。
