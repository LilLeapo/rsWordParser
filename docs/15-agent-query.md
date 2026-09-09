# Agent 大纲、定位与上下文（9.3）

本页保留 9.3 的接口与测量记录。9.4 已统一会话版本、分页预算和游标，并修复 worker 握手暂存竞态，见 [16-agent-budget.md](16-agent-budget.md)。

依据 `spec/22` AGENT-03/04/05，复用 9.2 的规范文本、UTF-16 分段与 ObjectRef。
实现位于工具侧 workspace 成员 `tools/agent-query`；核心只增加导航索引、空流呈现位置和字段缓存文字的只读访问。
`regex` 与 Unicode 归一依赖、可终止进程执行器不进入核心 rsword 的运行期依赖。
这是后续 CLI/MCP 共用的构件，不是 9.4 的完整会话管理器或 9.6/9.7 的工具交付。

## 大纲与预算

`nav::outline` 按流输出标题记录，parent 指向最近的较低级别标题；同名标题不合并，跳级不补虚构父节点。
section 范围截至下一同级或更高级标题，字符数按规范投影 UTF-16 计，块数按该流顶层块计。
无标题时每 32 块一组，取首个有文字段落的首句；非文字组明确说明对象，空文档返回空记录。
`Pages` 把记录按双预算分页，cursor 在服务端绑定 snapshot、调用配置与记录位置；预算失败不消费原游标。

最大真实件 `corpus/real/misc/large-report.docx` 的 26 条标题记录：content 序列化为 5052 UTF-16，
完整信封 5415 B，按 ceil(B/4) 为 1354 代理 token（不是模型 tokenizer 实测）。
默认 outline **limit=4000 UTF-16 / maxBytes=16000** 保持不变，返回 **2 页**。
不能把 1354 代理 token 与 limit 的 UTF-16 单位混用。测量来自 `agent_03_real_outline_and_budget_measurement --nocapture`。

`budget` 对 content JSON 计 UTF-16，对完整成功信封计 UTF-8 字节；usage 自身反复计入直到稳定。
导航记录没有 text 的逐字符分段载荷，anchorCounts 明写 notApplicable；不把索引覆盖数伪装成当前页字符数。
`context_page` 当前把完整请求窗口视为一条记录，超预算具名拒绝、报告最低预算，不裁半段；
跨接口统一会话版本推进、通用读取游标及窗口分段续读由 9.4 接入，不能宣称这些已经交付。

## 搜索与原文前置条件

锁定 regex 1.13.1、unicode-normalization 0.1.25 / Unicode 17.0.0；区域设置不参与归一。
字面模式同时归一输入与 pattern；正则模式不改写 pattern 的语法。
宽度只映射 Wide/Narrow 后 NFC，保留普通兼容字形（例如 ﬁ 不变成 fi）；Unicode White_Space 连续段折成一个空格，保留首尾。
Unicode simple case folding 由 regex 完成，不承诺完整多字符折叠。
每个归一字符保留原始 UTF-16 区间；命中返回 original、normalizedMatch、原文前置条件与可编辑性，
后续编译必须再调用 `verify_precondition`，不能按归一长度修改源数据。

搜索完整授权流，禁止把不连续区间拼接成伪造的连续文本；不同流永不合并。
分页保存 flow、归一搜索位置及零长推进状态，继续使用完整字符串调用 find_at，保留 ^/词边界语义。
零长中间两端 right，流末或授权范围末端两端 left；两端同一锚点。
空流与后一流共享文本偏移时，用独立呈现位置保留流身份，不虚构 InlinePos，不借用下一流。

资源界限：pattern 4096 UTF-16 / 16384 B，编译自动机与 DFA 缓存各 2 MiB，完整授权 scope 归一后至多 1048576 B。
默认 deadline 250 ms，允许 1–2000 ms；输入上限不能替代超时。
`Worker::start` 是不接收 pattern/text 的执行器初始化，另有 2 s 保护；`submit` 开始查询时钟，
计入请求写入、归一、编译、匹配与响应读取。查询失败或超时均 kill + wait，不能留下继续计算的后台线程。
该区分来自本机开发测试的 pre-main dyld 启动停顿观察，不是引擎性能缺陷，也不声称冷启动加查询总耗时小于 250 ms。
常驻破坏用例使用已经写出 running 标记的忙循环 worker，检查 timeout 后 PID 不存在，排除“还没启动就超时”的空验证。

## 授权详情

context 按共享锚点定位，在明确授权流/块范围中扩展至完整单位；单元格窗口保留 cell 父对象，不能带出另一格。
呈现字符按 owner 下钻，流框架没有段落时读取其明确对象，禁止猜附近源文字。
详情索引可内部读取全模型，但返回只取实际窗口内的对象；范围外正文哨兵测试锁住这个边界。
字段缓存由核心 `Document::field_result_text` 只读提取，Agent 不解析 nodeXml；表格几何走 Resolver，图表 display 走原生投影。
图片保留发生位置 ObjectRef、关系、资源 part/mediaId 和节；页眉页脚按 resolver 继承引用列出相关节，外链单列。
资源缺失或未登记媒体句柄明确标为 unavailable，不能杜撰一个句柄；二进制不进入详情。

## 常驻验证

- 全部 266 份真实件：标题与原生模型逐项对比，分页拼接回完整记录；空文档、无标题、跳级、同名标题另有定点用例。
- 全语料 1103 份，1099 成功投影与 4 份具名拒绝；字面与正则用直接扫描规范文本的独立 oracle，分页不漏重复命中。
- 全半角、半角浊音、空白、emoji、零长两侧、跨段/跨流、源前置条件、正则拒绝与资源上限有定点断言。
- 字段缓存、真实图表与表格、两处图片共用一媒体：逐字段对模型/resolver/媒体表，读取前后保存字节相同。
- reason 非空且属于 CATEGORIES；清空或未知值必须返回 AGENT_BAD_ANCHOR，全语料另作独立分类断言。

本轮另实际修改生产转换分支作两次破坏验证：将中间默认改 left，测试报告 left != right；
将授权范围末端改 right，测试报告 right != left。两次均为断言失败而非编译失败；之后逐字节恢复源码再跑全套。

9.3 不判定 22 项 Agent 端到端任务，也不声称桌面 Word 已验证；M8′ 待裁定门状态不变。
