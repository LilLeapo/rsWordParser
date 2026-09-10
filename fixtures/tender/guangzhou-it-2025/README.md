# 广州 IT 运维招标文件格式参考

来源：用户于 2026-09-10 提供的《广州市公安局2025年-2026年信息通信运维项目之信息中心IT设备运维项目招标文件（2025052601）.docx》。

- 原文件大小：53,721 bytes。
- SHA-256：`b93295ed1c7a25269ce806e52e38e7d26a2430c839b1d55400beb1dec64103df`。
- 检查方法：只读 ZIP + 独立 XML 解析，未在本机渲染或进行 Word 视觉验收。
- 原文件没有提交到仓库。以下夹具由格式事实重新构造，正文、表格内容是合成占位内容，不是原始招标文件摘录，也不是 Word UI 创建的文件。
- 未声明原文件的再分发许可。这个目录不是 corpus/real，不能标记为真实文件全量验证通过。

## 实际提取的结构事实

以下计数限定在原文件 `word/document.xml`，包括表格内段落；不是渲染页数。

| 项目 | 观察值 |
| --- | --- |
| 段落 / run | 1,381 / 1,324 |
| 段落样式引用 | 1,114 个，全部引用 `null3` |
| 直接首行缩进 | 511 个 `w:ind w:firstLine="480"` |
| 直接段落 spacing / numPr / tabs | 均未出现；不代表有效间距必为零，也不能排除文本模拟编号或制表效果 |
| run 显式字号 | 半磅值 24×97、28×40、48×6、36×6、7×4 |
| 加粗 / 单下划线 | 158 个 b 元素 / 41 个 u=single |
| 直接对齐 | left×87、center×61、right×5 |
| 表格 / 行 / 单元格 | 19 / 145 / 622 |
| 表宽 | 19 个 tblW(type=auto,w=0)，没有显式 tblLayout |
| 横向合并 | gridSpan=2×45、3×1、5×1 |
| 纵向合并 | vMerge=restart×2、无 val 的 vMerge×34 |
| 重复表头标记 | 未出现 tblHeader |
| 节 / 页面 | 1 节，pgSz=11906×16838 twip |
| 页边距 | 上下 1440、左右 1800、header 851、footer 992、gutter 0 twip |
| 文档网格 | type=lines、linePitch=312、charSpace=0 |
| 字段指令 | 主文档没有 instrText；不能据此声称其他 part 也无字段 |

19 张表格的（物理行数，gridCol 数）依次为：

```text
(2,7), (9,2), (2,9), (3,3), (22,3), (3,5), (8,3),
(2,3), (40,4), (2,5), (2,10), (2,10), (7,8), (7,4),
(6,6), (8,9), (11,7), (5,4), (4,2)
```

styles.xml 中默认段落样式 ID 为 `1`、名称 Normal，声明 both 对齐、widowControl=false、sz=21、szCs=24 和主题字体；被大量引用的自定义样式 `null3` 是 hidden，没有 basedOn，只声明字体 hint 和语言。不能因为样式名不规则就忽略它，也不能凭名称把它推断为标题。

## 夹具与已自动化的断言

- `document-body.xml`：首行 480、无直接 spacing、居中标题、单下划线栏、自动宽度表格、横向合并和省略 val 的纵向续接、页面/边距/网格。
- `styles.xml`：重构默认样式和稀疏的 null3 样式；无主题 part 的最小夹具不能作为主题字体实际解析的完整 oracle。
- `crates/rsword/tests/tender_styles.rs`：测试时使用项目 helper 打包最小 DOCX。三项测试覆盖首行长度/字符单位区分及缺省 spacing、表格内容流与合并、公开 InsertText 后段落和节属性不变。

```sh
cargo test -p rsword --locked --test tender_styles
```

本最小夹具的表格行数、列宽、占位文字是测试设计选择，不能当原文件完整副本。它不证明真实长表分页、字体呈现或整份招标文件均已通过。

Word UI 复现和面向投标编写的后续矩阵见 `docs/14-tender-style-test-tasks.md`。
