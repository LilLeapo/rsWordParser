# Agent 文本投影与锚点（9.2）

依据 spec/22 AGENT-01/02。`agent::text::project` 是纯读取的内部索引构件，尚不是 CLI/MCP 的有界读接口；分页、游标、会话版本推进由后续任务接入。调用方传入 snapshot，投影配置进入 projectionKey；同一文档与配置的对象键、分段键确定。

单元格用 ObjectRef 选择规范投影中的绝对子范围，保留原生 FlowId；父表与单元格的联合选择按区间去重。辅助 part 始终带 part，不用正文 arena 的同号节点代替。glossary 构建基块不自动作为当前文档正文输出；每个基块计 glossary，并返回其可寻址身份、段落数与省略原因。

原生流增补 w14:txbx / w:docPartBody：旧根先按文档序编号，新根随后按文档序追加。每个构建基块独立，单元格仍与其所在正文共享流。SPAN_NO_FLOW 报告任何无流归属的内容容器；全语料可解析 XML 的无流段落数必须为 0。

## 分类与 fixture 契约

下表与分类声明宏双向锁死；声明宏生成类别、原因、fixture 与每类测试，模型分派穷尽匹配，不使用遗漏变体的兜底。

```agent-categories
formatting synthetic/anchored-textbox__001.docx
tableGeometry synthetic/balance-dbcs-spacing__002.docx
fieldResult synthetic/bookmarks-crossref__004.docx
image synthetic/bugfix-regressions__006.docx
chart synthetic/chart-edit__001.docx
diagram synthetic/m6-smartart__001.docx
math synthetic/insert-and-layout__001.docx
ink synthetic/m6-ink__002.docx
ole synthetic/emf-image__005.docx
drawingGeometry synthetic/anchor-z-order__001.docx
revisionDetail synthetic/revisions__001.docx
hidden synthetic/raw-rpr__001.docx
structure synthetic/anchor-z-order__001.docx
rangeMetadata synthetic/comments__001.docx
protected synthetic/deep-nested-table__001.docx
unknown synthetic/header-footer-rich__008.docx
glossary real/sdt/content-controls.docx
```

## 诊断说明表 v1

原字段保留缺席状态。表外 code 的 known=false，说明回退原 message，能力影响为 unknown；不将未知诊断解释为无害。说明表、处理分支、测试与本清单双向相等。

```agent-diagnostics-v1
CHART_NO_SERIES|没有带缓存值的系列，不能据此报告完整数据|chartDataIncomplete
XML_UNBOUND_PREFIX|XML 前缀未绑定；原内容保留，对应扩展可能无法解释|partialInterpretation
AGENT_LIST_MARKER_UNRESOLVED|编号声明缺失、损坏或格式尚不支持，不能可靠报告列表标记|listMarkerUnavailable
SPAN_NO_FLOW|内容容器没有原生流身份，无法可靠定位或判断范围是否同流|sourcePositionUnavailable
```

## 编号的降级边界

RES-09 的只读标记计算位于 resolve，Agent 不自建计数器。无法解释的格式须给 list-marker? 与 AGENT_LIST_MARKER_UNRESOLVED，不能猜十进制。当前格式化支持 decimal、decimalZero、upper/lowerLetter、upper/lowerRoman、none 与可解码的 bullet；自定义格式、图片项目符号和其他地区格式明确降级。沿用级别查找的 numStyleLink 防环，计数器按 abstractNumId 累积，startOverride 仅首次应用，尊重 lvlRestart 与 isLgl。

## 必须失败的检查

常驻负例分别删除合成字符映射、篡改 source.part、用合法 source 载荷伪装呈现锚点、定位到 emoji 代理对中点。完整锚点反查还验证 snapshot/projectionKey/segmentKey/affinity 和全部冗余字段；源分段验证原文字、载体归属与 EDIT-02 合法边界。
