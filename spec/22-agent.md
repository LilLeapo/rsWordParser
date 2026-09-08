# SPEC 22 · Agent 接口层与文件级工具

> 9.1 关口稿，2026-09-09，**待评审，未实现**。评审通过前禁止开始 9.2。
> 依据：docs/03 v3.3、spec/20、已复核的 docs/12（22 项任务、预算实测）。
> 本稿采用已批准的“源文字 / 呈现”两类锚点。spec/20 门 2 的旧 InlinePos 全覆盖措辞待负责人订正；
> 本文件不修改 spec/18、spec/20、spec/21，也不裁定 M8′ 门 2 / 门 4。

## 共同约定

Agent 层只读取模型、resolve 与内核提供的只读定位结果；不得自行遍历或修改 DOM/Span。修改必须编译为原生 EditOp 后经事务执行。
禁止直接改 DOM/Span、把模型 JSON 写回，或让 Agent 为完成任务调用 XML 逃生口。
文件形态为 A（CLI + MCP）；不决定尚待裁定的 MCP 运行时/传输实现，不提供宿主内实时编辑。

下文接口为逻辑工具契约，不承诺新增同名 Rust 稳定公共项。JSON 字段 camelCase，未知输入字段拒绝，
缺省值逐条规定；未说明的枚举值不得猜测。输出区间均半开，用户指令中的“第 N 个”从 1 起，协议下标从 0 起。
时间为带时区的 ISO 8601 字符串；缺失作者/日期原样报告缺失，不以当前用户或当前时间补齐输入事实。

共同身份：`ObjectRef={part,node,flow,kind}`。part 为会话内 part id，输出必带；输入省略 part 表示主 part（BIND v3.1）。
flow 是同 part 内正文/单元格/文本框/批注/注释等内容流的身份，不用 nodeId 或 partId 代替它。
不存在的 part 或不在该 arena 内的 node 返回 `BIND_ID_UNKNOWN`。id 不跨会话复用。
所有读取遵守 AGENT-06；错误统一为 `{code,message,details}`，原生错误保留原 code，不全部包成 AGENT 错误。

## AGENT-01 文本投影、保真声明与诊断

### 契约

`text(scope, view="marked", budget, cursor)` 返回 AGENT-06 的分页信封。
`content` 是 UTF-8 字符串、LF 换行；另给 `anchors`、`omitted`、`anchorCounts`。默认 scope 为主 part 正文，
不隐式追加页眉、批注等流；`scope=all` 或显式 flow 才读这些流。view 固定为 marked：保留待决插入/删除的文字，
用呈现标记区分，不偷偷接受或拒绝修订。其他 view 值本版返回参数错误。

投影是确定性纯函数；规范化顺序来自文档内容序列、part URI 与流内顺序，不来自 HashMap 迭代。
scope=all 按主流、页眉页脚（part URI 排序）、脚注、尾注、批注及剩余子流的规范顺序输出。表格单元格若已随父表展开，访问集合标记其子流已输出，不再附加一次；只请求该子流时单独输出。
无需反解析轻标记来获得编辑地址：结构化 anchors 才是权威。呈现语法接近 Markdown，但不承诺通用 Markdown 渲染器的视觉效果。
正文中的 `\`、`[`、`]`、`|`、`#` 用反斜线转义；新加的转义符是呈现字符，原字符仍有源文字锚点。

| 内容类别 | 投影与归属 | 保真/省略规则 |
| --- | --- | --- |
| 普通段落、run | 原文字顺序 + 段末 LF；标题前加 level 个 `#` 和空格 | 不归一原始文字；直接样式/字体等不展开，按有格式信息的段落计 formatting |
| 标题、列表 | 标题文字保持；列表按 Resolver::list_markers 的实际标记输出 | 不能猜编号；解析失败用 `[list-marker? #object]` 并报诊断。前缀为呈现字符 |
| 表格 | 表格起止标记、管道行；单元格内段落保持独立单位，行/列坐标进元数据 | grid/span/vMerge/nesting 不靠管道排版表达；分别计 tableGeometry，必要时按需取 cells/table。不复制合并单元格文字 |
| 字段 | `[field KEYWORD #object]` | 字段指令与缓存结果不可当普通可编辑正文；结果摘要只在显式 context/detail 中作为只读呈现文字，计 fieldResult。TOC 条目通过字段详情读取 |
| 图片、图表、SmartArt | `[image #object]`、`[chart #object]`、`[diagram #object]` | 各计 image/chart/diagram；不内联二进制、base64 或猜数据，按需读 display 或媒体 |
| 公式、墨迹、OLE | `[math #object]`、`[ink #object]`、`[ole #object]` | 分别计 math/ink/ole，不伪造 OCR/计算结果 |
| 文本框、画布、组合形状 | 父流 `[textbox #object]` / `[shape #object]`，子流另有标题 | 父流不重复输出子流文字；scope=all 时每个子流只输出一次；几何/形状细节计 drawingGeometry |
| 批注、脚注、尾注 | 引用处 `[comment #object]` / `[footnote #object]` / `[endnote #object]` | 主流只给引用；显式子流包含正文、author/date 等事实，批注关系与解决状态不得从顺序猜 |
| 页眉页脚 | `[flow header/footer part=… variant=…]` 后接子流文字 | 报引用节与 first/even/default，保留 titlePg/evenAndOddHeaders；共享 part 只存一份内容，不把继承当缺失 |
| 修订 | `[ins #object author=…]…[/ins]`、del 同形；属性修订给 `[revision #object]` | 包含标记/作者不等于接受修订；插删标记为呈现字符，合法正文仍为源文字；属性快照详情计 revisionDetail |
| 隐藏文字 | `[hidden #object]` | 默认不泄露隐藏载荷，计 hidden；显式 detail 才给只读呈现文本，不能悄悄算进可见文字 |
| tab、软换行 | 保留 tab / LF 的文本语义 | 仍经 EDIT-02 判断其边界能否编辑，不制造 XML 文本位置 |
| 分节/分页、书签和范围标记 | `[section-break #object]` / `[page-break #object]` / `[range #object]` | 计 structure/rangeMetadata；不假定真实页码；有锚点但无文字的节点也有对象定位 |
| Protected、未知扩展或未支持类型 | `[protected KIND #object]` / `[unknown #object]` | 非空省略计数 protected/unknown，带原诊断；不得消失或伪造成空段落 |

`#object` 为本投影内稳定且带类型的对象键，在 anchors/object 索引中可解析成 ObjectRef；不裸用 mediaId。
同一媒体的两处出现有两个对象键；对象键与 segmentKey 在同 snapshot/config 下不随分页变化。标记里的动态字符串用 JSON 字符串转义，禁止原始换行破坏标记边界。用户文字不能通过伪造占位符获得对象身份。
换行、管道、前缀等每个合成字符都必须有 AGENT-02 呈现归属。

`omitted={scope,page:[{category,count,reason}],complete}`：只统计本页代表的对象省略了哪些细节，
每个对象/类别由其首个投影单位负责计数一次；分页累加不得重复。complete 表示是否已覆盖请求 scope，
不是宣称模型完整理解文档。未请求的流列在范围声明中；分页未返回的内容由 truncated 表示，不能混进“不可支持”。
对缺少可用对象的退化节点也须有 protected/unknown 记录。分类表、投影分派与每类 fixture 来自同一张声明表，
穷尽匹配模型变体；新增变体必须导致未登记分派/fixture 的检查失败。

诊断输出 `DiagnosticView={code,message,origin,part,range,userExplanation,capabilityImpact,known}`，
保留原字段的缺席状态和原始 code/message/定位。只读诊断不得写入或增加会话诊断。
已知 code 的用户说明与能力影响由一份版本化声明表维护，表与处理分支、测试双向锁死。
未知 code：known=false，userExplanation 回退原 message（若缺失则“未知诊断，未提供说明”），
capabilityImpact="unknown"；禁止丢弃或自动归为可忽略。不能因为没有 warnings 就宣称 Word 必定无修复提示。
R10 的 CHART_NO_SERIES 必须说明“没有带缓存值的系列，不能据此报告完整数据”，不推断整个文件打不开。

### 验收

每种表列类别至少一个正例；字面伪占位符、未知变体、深嵌套、同媒体多处引用分别有负例。
两次投影逐字节相同；所有遍历迭代实现。全语料检查分类计数，删除任一分类分派/fixture 必须红。
R3–R6、R8–R10 对独立模型事实逐字段相等；注入未知诊断，原 code/message/定位必须仍在输出且 known=false。

## AGENT-02 双向锚点

### 契约

坐标单位为 **UTF-16 code units**，使用同 scope/config 构成的规范投影流内绝对位置；分页不重置偏移，不与原生子流坐标混用。字符位置指 Unicode scalar 的起点，
另支持末尾 caret；代理对中间不是字符边界，返回 `AGENT_BAD_OFFSET`，禁止切半。
anchors 以不重叠的半开分段覆盖 content 的全部 UTF-16 单位，无洞、无重叠；空文本也有末尾定位。

两个带标签的锚点：

- `source={kind:"source",snapshot,projectionKey,segmentKey,offsetInSegment,part,flow,node,inlinePos:{part,para,offset}}`。
  node 为真实载体，inlinePos 必须经 EDIT-02 验证合法，输出 part 与 inlinePos.part 一致。可编辑仍受操作/保护约束。
- `presentation={kind:"presentation",snapshot,projectionKey,segmentKey,offsetInSegment,owner:ObjectRef,reason}`。
  没有 inlinePos，不借用最近段落位置；标题前缀、分隔符、对象占位符、原子字段结果/保护内容的显示文本均属此类。

projectionKey 绑定 scope/view 与投影配置，服务器核对全部冗余字段和实际分段映射；不能只相信调用方提交的 kind 或 inlinePos。跨投影或伪造字段返回 AGENT_BAD_ANCHOR。
segmentKey 区分同一原生边界在投影中多次呈现的情形，offsetInSegment 为 UTF-16 偏移；
不得以伪造 node 或越界 InlinePos 编码呈现内偏移。字符边界有两侧候选时，toAnchor 默认右侧，流末尾取左侧末端，
可显式 affinity=left/right；返回锚点保留该选择。给定完整锚点的 toTextOffset 必须唯一。
仅给原生 InlinePos 而缺少 segmentKey 时，若有多个投影位置必须返回全部候选或具名歧义，不能猜；该位置未投影（例如未请求的流、隐藏载荷）则 AGENT_NOT_PROJECTED。

`anchorCounts={sourceUtf16,presentationUtf16,sourceScalars,presentationScalars}` 随每页返回；前两者之和为 content UTF-16 长度，
后两者之和为 Unicode scalar 数。保真门枚举所有字符起点及末尾：offset→带标签锚点→offset 恒等；
对 source 再验证真实 InlinePos 的合法性。编辑只接受合法字符边界，不把 UTF-8 byte offset 当 UTF-16。
锚点附 AGENT-06 snapshot；陈旧、跨会话或不属于所给 part/flow 的锚点具名拒绝。

例：测试段落原文 `中😀文` 投为一级标题 `# 中😀文` 加 LF。sourceUtf16=4、presentationUtf16=3，
sourceScalars=3、presentationScalars=3。投影位置 2/3/5 对应原文 UTF-16 位置 0/1/3；
位置 0/1/6 是呈现字符，位置 4 在代理对中间，不能作为编辑端点。两类计数不能按 Rust bytes.len() 计算。

### 验收

全语料 1103 输入，1099 可打开；4 个拒绝名复用 tests/common::UNOPENABLE 且双向锁死。
每个字符两类必居其一且往返相等；覆盖 emoji、组合字符、转义前缀、表格管道、同媒体多处出现、空段落。
删除任一合成字符的映射、给错 part、把呈现伪装 source、将 offset 推进到代理对中间，分别必须失败。
统计 source/presentation 数量并检查总覆盖，不能仅测可编辑文字的子集。

## AGENT-03 大纲

### 契约

`outline(scope=main, levels=[1,9], budget, cursor)` 输出有序记录，
每条 `{kind:"heading",object,level,title,parent,blockRange,blockCount,charCount}`。
标题级别来自已解析/resolve 的段落语义，不能只匹配 style 名字；遇 1→3 保留真实 level=3，parent 指向最近较低级标题，
不能凭空插入二级标题。章范围至下一个同级或更高级标题前，blockCount 包含自身，charCount 为该范围的规范投影 UTF-16 长度。
range 绑定 part/flow；正文顶层用 blockRange，其他流用该流内容序列。数组分页不重复父记录，parent 可引用前页的对象键。

无标题时每 32 个顶层块形成 `kind:"group"`，title 为首个非空段落的首句（句尾 . ! ? 。！？或段末）；
只有非文字时用明确对象描述，空文档返回空记录与 empty=true。group 不假冒 level=1。
不跨页加载全文再客户端过滤；大纲元数据可从内部索引生成，正文载荷不得附带泄漏到返回值。

### 验收

R1/R2/R11：最大真实件 26 个一级标题与独立模型序列相等，第三章 main [41,48)。
266 份真实件都有有界、可读结果；空文档、无标题、跳级、同名标题、多 part 的 parent/range 均检查。
最大件完整 outline 响应 ≤16000 UTF-8 B，代理 token ≤4000；这是实现验收目标，9.1 未声称已测到。

## AGENT-04 定位查询

### 契约

`find(scope=main, pattern, mode="literal", case="sensitive", width="exact", whitespace="exact", maxHits=20, budget, cursor)`。
搜索 AGENT-01 的规范投影，包含呈现字符；结果 `{match,textRange,anchors,editable,context}`，
不能悄悄略掉只读命中。非空命中的起点取 right、终点取 left affinity，零长命中两端使用同一 right affinity。
结果按流顺序、起点、终点排序，采用非重叠最左匹配。

literal 模式先按选项同时归一 pattern 与待搜文字，再把 pattern 当字面；regex 模式 pattern 是作用于归一后文本的正则语法，
**不重写其语法字符**。case=insensitive 使用 Unicode simple case folding，不承诺完整多字符 case folding。
width=fold 应用 Unicode 的 Wide/Narrow 映射后做 NFC 规范组合（含半角浊音组合），不做完整 NFKC；whitespace=collapse 将 Unicode White_Space 连续段变为一个 ASCII 空格，
不删除首尾空白。数据表的 Unicode 版本必须随投影版本固定，不能因平台区域设置改变结果。
归一过程必须保存单调源区间映射；合并/展开的匹配回映到完整原始区间，若端点落在不可分的展开内部则 editable=false。
不能用归一后的长度去执行 DeleteRange。匹配不能跨流；跨段命中可返回，但是否可写由 AGENT-07 判断。

空 literal 拒绝；regex 零长命中允许作为查询结果，按下一个 Unicode scalar 边界推进，末尾最多一次。
regex 使用 regex crate 支持的语法，拒绝回溯引用/look-around 等不支持语法，不降级其他引擎。
一次 regex 搜索对固定 pattern 随文本长度线性；**不宣称枚举全部命中整体必为线性**。
实现必须限制 pattern（4096 UTF-16 单位、16384 UTF-8 B）、编译自动机（2 MiB）、缓存 DFA（2 MiB），
每次查询的完整授权 scope 的归一后文本至多 1048576 UTF-8 B；超限返回 AGENT_QUERY_TOO_LARGE，要求显式缩小 scope，
不自动切段搜索而漏掉跨段匹配。每个 flow 独立搜索完整字符串；分页游标保存 flow、原始搜索起点、零长推进状态和待返回命中。
达到 maxHits 不得跳到下一段，必须保留当前段内剩余命中；find_at 等定位使用完整字符串上下文，不把后缀当新字符串改变 ^/词边界语义。

deadlineMs 缺省 250、允许 1–2000，包含归一、编译和匹配。超时返回 AGENT_QUERY_TIMEOUT，不发布未完成的匹配页或编辑计划。
必须取消计算工作单元（可取消实现或可终止的隔离 worker），不能只让等待的 future 超时而后台继续占 CPU。
输入/自动机上限不是硬超时的替代。regex 依赖与资源执行器放工具侧共享层，不新增核心 rsword 的运行期依赖。
版本依据与复杂度界限：[regex 官方文档](https://docs.rs/regex/1.13.1/regex/#untrusted-input)；实现必须锁定版本并重验限制。

### 验收

全语料每种模式的结果与同配置独立扫描规范 text 的结果一致；正则参考比较不能与被测归一映射共用同一结果缓存。
覆盖全/半角及半角浊音组合、连续空白、emoji、零长、跨段/跨流、合成占位符、多个重复命中及归一后端点不可编辑。
匹配分页拼接等于不限页的有界 oracle；同段超过 maxHits、跨段正则与 ^/词边界的续读分别有断言。
超长 pattern、自动机膨胀及故意超时任务具名拒绝；超时后 worker 不残留，会话逐字节不变。

## AGENT-05 上下文与按需下钻

### 契约

`context(anchor, before=1, after=1, unit="blocks", detail=false, budget, cursor)`。
unit 可为 blocks 或 utf16；utf16 是期望窗口距离，实际内容向外扩至完整段/原子对象边界，再由 AGENT-06 判断能否容纳，
不能为凑 limit 裁半句。返回 requestedRange 与 actualRange，明确边界扩展；不得越过明确的 scope/blockRange 授权范围。
anchor 可为 presentation，以其 owner 定位，只读下钻不等于获得编辑文字位置。

上下文不得跨流猜测邻近段；单元格、文本框、批注等返回 flow 与父对象。detail=true 可取字段结果、图表 display、
表格逻辑坐标、图片引用/媒体元数据、修订/批注事实及诊断，均适用预算；二进制经 media 单独取。
media 清单是资源，不是图片出现计数；图片出现项必须能追到 ObjectRef、所属节、relationship 与 mediaId，外链明确标为外链。

Agent 工具的 `document`（CLI 名 model）是**新增的有界包装**，不得改变 BIND-10 原生响应契约。
必须显式选择 blockRange/flow 或声明型字段；返回只含范围内块及完成该查询必要的引用索引切片，
不能照搬原生 document 所保留的全局 spans/fields/revisions 给调用方后再截断。
内部建立规范模型/只读索引允许；不允许向 Agent 完整预读后客户端过滤，不允许输出范围外正文载荷。

### 验收

R7–R11 的字段/表格/图表/图片/诊断详情逐字段对模型与 resolver 相等。
R11 全调用日志只有 outline 元数据及 [41,48) 内的正文，累计 UTF-8 响应 ≤24000 B；
在原生全局索引中放范围外哨兵文字，Agent 包装不得泄漏。context 的请求窗口、实际边界和预算错误都验证。

## AGENT-06 预算、截断、游标与版本

### 契约

9.0 建议在本关口规范中定为以下默认值，评审通过后对实现生效；不声称现有原生接口已具备。

| 读取 | limit（UTF-16 单位） | maxBytes（完整协议响应 UTF-8） | 其他缺省 |
| --- | --- | --- | --- |
| outline | 4000 | 16000 | 只读标题元数据 |
| text / context | 8000 | 24000 | 主流 / before=after=1 block |
| find | 4000 | 24000 | maxHits=20 |
| document、诊断/媒体清单、summary、preview 的读取结果、diff | 8000 | 24000 | document display=false |

limit 计 content 的 UTF-16 长度；content 为结构化记录时计其规范紧凑 JSON 的 UTF-16 长度。
maxBytes 计完整 JSON 信封，包含 anchors、诊断、游标、计数与转义，不用未转义字符串长度代替。
普通文本 CLI 显示与 JSON 来自同一响应，不能用额外未计费文本绕过预算。传输协议固有 framing 不计入此数，工具内容全部计入。
调用方可提高上限：limit 范围 1–1048576，maxBytes 范围 512–4194304，maxHits 范围 1–1000；越界参数错误。
这是读取响应上限，不授权修改或扩大 scope。`ceil(bytes/4)` 仅作可重复 token 代理，不能称为模型 tokenizer 的硬上界。
真实 Agent 验收须另记 tokenizer 名称/版本与工具描述等完整会话开销。R1 回答和 R11 累计预算继续服从 docs/12。

成功信封：`{snapshot,content,range,truncated,nextCursor,omitted,anchorCounts,usage}`。
usage 含 contentUtf16、responseBytes、estimatedTokens；responseBytes 为最终完整序列化响应大小（含该字段自身，求稳定值），
estimatedTokens=ceil(responseBytes/4)，两个计数字段一起求序列化大小稳定值，不得只报正文大小。非文本工具没有字符锚点时 anchorCounts 为零并标明适用范围，不混充 text 的覆盖率。

分页单位为完整段落、原子对象占位符、outline/find/detail/summary 的完整记录。
表格可在单元格段落边界分页，结构标记附属于相邻单位，不能拆占位符或源文字；续页不重复表头或装饰前缀。
取能装入两个预算的最长单位前缀。若尚无一个单位能装入，返回
`AGENT_BUDGET_TOO_SMALL {object,minLimit,minBytes}`，content 不返回，游标不消费；超过允许最大值则 AGENT_UNIT_TOO_LARGE。
不得拆长段、超预算返回、丢掉对象，或成功返回空页且 nextCursor 不前进。find 全范围无匹配可返回空页，truncated=false；不能把有待返回命中的超预算错误伪装成空页。

分页拼接定义：字符串按原样拼接、数组按记录拼接；不拼接重复的信封元数据。结果等于同 snapshot/config/scope 的一次性规范投影；
omitted.page 与 anchorCounts 逐页累加相等。普通 text 空文档允许空页，truncated=false、nextCursor=null。
字节预算连最小错误信封也不能容纳的参数在开始读取前拒绝；错误说明可缩短，但 code 与可重试所需的最小字段必须保留。

MCP 会话 snapshot=`{sessionId,version,projectionVersion}`。version 为 Agent 层单调计数，
每次成功提交的 edit 批次或 addMedia 增加一次（包括成功 no-op）；失败不增加。save 在克隆上运行、preview/read 不增加。
所有写入口必须统一通过 Agent 会话管理器；不得有绕过版本推进的原生写通道。
游标绑定 snapshot、工具名、scope、投影/查询选项哈希与下一单位位置，不暴露为可修改参数包。
允许续读调整 limit/maxBytes/maxHits/deadlineMs，但不得改变语义查询配置。错误/只读不消费游标，重复请求同游标确定性返回。
版本变更返回 AGENT_STALE_CURSOR；错会话/工具/配置、损坏或伪造游标返回 AGENT_BAD_CURSOR；close 后 BIND_NO_SESSION。
MCP 游标必须验证真实性（服务端记录或认证编码），不能只 base64 任意输入并信任里面的 node/范围。

CLI 单次调用有原生会话，跨调用续读**不复用旧 nodeId/sessionId**：文件游标绑定规范化输入身份、完整输入 SHA-256、
projectionVersion、查询配置和逻辑流/单位序号。重开先核文件字节指纹，再重建位置；相同位置产生新会话的锚点。
文件变化返回 AGENT_STALE_CURSOR，路径指向其他文件返回 AGENT_BAD_CURSOR。文件游标可本地验证但不得作为编辑授权或可信 node 句柄。
CLI 传入旧会话锚点不能自动接着写，须在本次文件快照重新用文字/对象选择器解析。

### 验收

默认值、极小预算、恰好容纳、单段超长、超大元数据/游标/未知诊断、非 BMP、JSON 转义分别测试，最终字节数硬断言。
所有读取（含 summary/preview 结果、诊断清单）进同表预算测试。续读拼接与单次投影相等；省略计数不重算两次。
edit/addMedia/no-op 后旧游标失败；失败 apply、preview、save/read 后旧游标仍可用；错 flow/part/options 不能借游标扩大读取范围。
CLI 两次调用续读成功，文件改一字节即拒绝，重开后不接受旧 native id。长期停留在同一游标的空页测试必须红。

## AGENT-07 文本锚定编辑与编译

### 契约

`edit(expectedVersion, operations, context)` 是原子批次：在当前快照重新解析选择器、预检范围、编译审计序列，
在候选会话按序执行，全部成功才提交。失败恢复文档、媒体、诊断、关系、id 分配、版本与摘要历史，错误带 operationIndex。
原生操作若只支持主 part（例如现有 DeleteColumn），辅助 part 目标必须提前具名拒绝，禁止将同号 node 传入主 arena。
批次后续选择器在前序操作后的候选状态重新解析；由前序产物引用时用批次内符号引用，不预先猜 nodeId。

Agent operations 是以 `action` 为内部标签的 camelCase 枚举（例如 `{"action":"replaceText",...}`），与 native 的 `op` 标签明确区分；payload 由下表统一生成。
选择器包含 scope 与以下一种：本 snapshot 的 source anchor；或 `{find,mode,occurrence,objectKind}`。
缺省 literal/exact；命中为零返回 AGENT_TARGET_NOT_FOUND，多个且未给 occurrence 返回 AGENT_AMBIGUOUS（有界候选列表/总数），禁止选第一个。
occurrence 从 1 起；批量 all 必须显式选择并锁定原 scope 内的匹配集合，按避免偏移漂移的顺序编译。
归一匹配用于定位后必须验证完整源区间和原文前置条件，不以归一字符串直接覆盖源数据。

**任何文字编辑端点落在 presentation，或范围跨越呈现字符，返回 AGENT_NOT_EDITABLE，details 必含 owner:ObjectRef。**
禁止平移端点、删去呈现片段后继续，或用邻近段落猜目标。原子对象的专用操作可用其 ObjectRef，
例如 ReplaceImage 对象选择不等于修改 `[image …]` 这串呈现文字。过期 anchor/version 返回 AGENT_STALE_ANCHOR/AGENT_VERSION_CONFLICT。
跨段或不连续源范围如不能由明定编译规则无损表达，返回 AGENT_UNSUPPORTED_RANGE，不拆成部分成功。

| Agent 操作 | 结构化载荷与原生编译 | 授权边界/任务 |
| --- | --- | --- |
| replaceText | selector + find/replace + occurrence/all；同段 DeleteRange + InsertText，保留未涉及属性 | W1/W11；tracking 走 EditContext.track_changes，不能用不追踪的替换捷径 |
| insertParagraphAfter | anchor + text + 可选 style；InsertBlock(NewBlock::Paragraph) | W2；属性为 patch 线型，不造 XML；不能覆盖后一个段落 |
| setBlockStyle | paragraph selector + styleId + 可选 createStyle 声明；缺失时 UpsertStyle，再 SetParaProps 的 style patch | W6；createStyle 必须完整点名类型/基样式/属性；已存在且冲突返回 AGENT_STYLE_CONFLICT，不擅自 upsert 覆盖；其余 pPr 原样 |
| deleteBlock / moveBlocks | block selector 或 chapter selector + destination；DeleteBlock / 有序 MoveBlock | W10；整章至下个同级/更高级标题前；目的地在自身区间拒绝；分节结构不隐式移动 |
| deleteTableColumn | table selector + 1-based column；DeleteColumn | W4；按逻辑 grid 列预检；不能把非法位置拒绝算成功 |
| addComment | source range + author + text + 可选 date；AddComment | W5；只允许指定字符范围与必要 comment/关系 part，不扩大到整段 |
| acceptRevisions | author 精确字符串 + scope；锁定匹配 revision 集并按引擎规定顺序 AcceptRevision | W3；保留其他作者的待决修订，不用 AcceptAll；空集合报目标不存在 |
| updateToc | field selector，按原指令重算；RegenerateBlockField(Auto {pages}) | W7；保留按目标书签核对的原 PAGEREF 缓存页码（不是新计算值），无法核对则不写页码；报告 pageNumbers="unresolved"。不猜 0/1，不改原指令，点名必要书签变更 |
| setHeaderFooter | section selector + kind + variant + 结构化 paragraphs；SetHeaderFooter | W8；无 part 时经 SAVE-05 创建，不能要求调用方写 w:hdr |
| replaceImage | occurrence selector + 当前会话 mediaId；ReplaceImageMedia | W9；按出现位置找 drawing，不按共享媒体 id 找“第二张”；只改指定引用，禁止原位覆盖共享 part |

TOC 原缓存的提取必须来自内核提供的结构化字段详情，记录来源字段/书签；不能让 Agent 解析 nodeXml 或从条目尾部数字猜页码。
媒体输入经 AGENT-10 addMedia 取得会话句柄，接受实际 bytes/MIME，不要求 Agent 生成 base64 字符串。
MCP 允许宿主附件句柄、CLI 允许文件路径，适配层解析为字节；内容应实测匹配 MIME，不猜扩展名。
addMedia 是显式状态操作，成功推进 version；随后替换必须基于新 version。若上传先成功而后续 edit 失败，
上传仍是先前已提交状态，失败 edit 只须回到它开始时的状态；不把它误报成合并事务回滚。

每个工具操作按同表展开编译分派、载荷 schema、W 任务覆盖清单与审计打印测试；复杂转换用普通函数。
编译时保存**正向线型 EditOpJson**作为审计事实，不依赖 BIND-03 可能具名拒绝的引擎→JSON 反向出口。
用于 hash/审计的线型先按类型校验，再以对象键 UTF-8 字典序、数组原顺序、serde_json 数字格式输出紧凑 JSON；拒绝重复输入键。
不能把 HashMap 的偶然迭代序当作操作身份。媒体内容参与 hash，只有 MIME/长度相同不算同一预览输入。
需要任何 XML 逃生口、不可表示属性或无法打印完整操作序列时，返回 AGENT_UNREPRESENTABLE 并不提交；
不能先改成功后才发现日志打印不了。审计序列包含必要的媒体输入摘要（长度/hash/MIME，不内联字节）。

### 验收

docs/12 W1–W11 的语义与不变式逐项通过，桌面 Word 无修复证据独立记录；xmlEscapeCount 从 0 保持 0。
呈现字符端点、跨只读片段、歧义、过期版本、归一后不可分端点分别具名失败且状态逐字节不变。
W6 断言原 pPr 未涉及字段/原片段保留；W9 两处共享媒体只变第二个引用；W10 七个原块整体移动。
批次中后一步注入错误，前面新增 part/媒体/诊断/id/版本全部回滚；不得只比 document 文本。

## AGENT-08 预览 diff

### 契约

`preview(expectedVersion,operations,context,budget)` 在与 edit 相同的候选快照上解析、编译、执行，
返回不可变 previewId、输入 snapshot、操作摘要/hash、完整审计记录的分页入口，以及按块的 before/after 文本 diff。
呈现插入/删除/移动、格式变更、媒体引用变化、附带 part 变更必须有类型标记；纯格式变化不能因文字不变显示“无变化”。
previewId 同时是带 preview 类型的 reportId，可用 summary 分页读取；预览首个差异单位超预算时返回预算错误且不留下 token，调用方提高预算后重试。
预览的任务工作区不会提交；版本、原生会话字节、媒体句柄、诊断计数均保持。
预览报告是业务状态外的缓存，最多 32 个 / 16 MiB，只能淘汰旧预览，不能因 preview 淘汰已提交编辑的审计报告。

diff 单位是完整的受影响段/对象，不裁半段；长项按 AGENT-06 返回预算错误并可用更大预算重读。
preview 执行与读取分离：成功后保存有界生命周期的不可变报告，报告读取可分页；超时/失败不留下可提交 token。
`edit` 可带 previewId，必须检查 session/version/operations/context hash 全相等，否则 AGENT_PREVIEW_STALE。
不承诺相同包压缩字节或运行时生成日期天然一致；要证明相同输入的预览等价，缺省作者/日期等执行上下文必须在预览时冻结并用于提交。
预览不自动请求额外人工确认，也不自动写盘；工具只陈述影响，保存授权来自调用方任务。

### 验收

preview 前后原生会话、媒体、版本与既有编辑报告快照相等（新建预览缓存除外）；成功 edit 与有效 preview 的语义结果、编译序列与受影响集合相等。
纯格式、共享图片、章移动必须有非空差异类型；过期/异会话 previewId 和 context 改动具名拒绝。
预算过小不能留下半编辑状态；长报告分页全部读完与完整报告相等。

## AGENT-09 变更摘要与审计

### 契约

edit 的成功响应只返回有界回执 `{beforeVersion,afterVersion,reportId,counts}`，完整差异与审计经 summary 读取；不能提交后因即时摘要超预算把 edit 报成失败。
每次成功 edit 生成不可变 reportId，绑定 beforeVersion/afterVersion、操作原始请求、正向 EditOp JSON 序列、
MutationResult、受影响对象/part、before/after 可读文本、修订/批注变化、媒体变更和本次诊断/逃生口计数。
`summary(reportId,budget,cursor)` 为只读分页，记录顺序固定；不能仅输出“修改成功”或用 MutationResult 缺字段推断未改动。
实际变化与请求授权范围分别记录；独立保真 oracle 在测试中检查两者，不用实际变化自证授权。
错误不生成成功报告，不推进 version；错误响应可以给 operationIndex 与具名原因，但不宣称已保存。

报告被后续编辑保留为历史事实，可显式读取旧 reportId，不将其旧锚点冒充当前可写锚点。
报告游标绑定不可变报告版本，不因后续文档编辑失效；此例外仅限历史报告，不适用于 text/find/context 的当前文档游标。
close 后报告不可访问；服务端每会话最多 32 个报告，FIFO 淘汰后 AGENT_REPORT_EXPIRED。总报告存储有界 16 MiB，
新报告超过单会话容量返回 AGENT_REPORT_TOO_LARGE，edit 必须在提交前发现并回滚，不能修改成功却丢审计。
读取后的自然语言建议必须与事实字段可追溯；未知原生诊断沿 AGENT-01 回退。

### 验收

每项 W 操作审计序列可解析且与实际执行线型序列一致；重放到相同初始快照（媒体输入一并提供）得到相同语义结果。
移除审计项、删掉一个附带 part、把失败标成成功时测试必须红。报告分页无重漏，历史报告不可用于当前编辑。
报告容量、淘汰、close 与超大报告回滚有边界测试；逃生口计数与会话 diagnostics 一致。

## AGENT-10 CLI / MCP 工具表与错误

### 契约

同一 `agent_tool!` 表展开工具名、参数 schema、读写分类、默认预算、CLI/MCP 映射及每行端到端测试。
表的成文行集合与注册表双向锁死。除标注的传输差别外，两边调用同一业务处理器，不各写一套编译/预算逻辑。
下表所有路径/附件只由调用方显式提供；二进制读取/上传用独立受限数据通道，不混入 Agent 文字上下文。

| 逻辑工具 | CLI `rsword` | MCP 名 | 主要参数/结果 |
| --- | --- | --- | --- |
| open | 每次文件命令内部 open，不输出可跨进程 native 会话句柄 | open | input 文件/附件；返回 sessionId、version、能力及流数量；流详情按需取，不附完整模型 |
| close | 每次命令 finally 清理 | close | sessionId；幂等，无会话也成功 |
| outline | outline INPUT | outline | scope/levels + Budget/Cursor；AGENT-03 |
| text | text INPUT | text | scope/view + Budget/Cursor；AGENT-01 |
| find | find INPUT | find | pattern/normalization/regex/resource limits + Budget/Cursor |
| context | context INPUT | context | selector/anchor、窗口、detail + Budget/Cursor |
| document | model INPUT | model | blockRange/flow/fields/display + Budget/Cursor；有界包装，不是裸 BIND document |
| preview | preview INPUT --ops OPS [--report REPORT] | preview | operations/context/version；创建报告，读取结果适用预算 |
| edit | ops INPUT --ops OPS --output OUTPUT | edit | 默认 Agent operations；显式 --native-ops 可作低层调试，仍经会话管理器/版本与审计；M9 任务禁止该旁路 |
| save | ops 成功后写 OUTPUT | save | sessionId/options/output；克隆保存，失败会话不变 |
| summary | ops/preview 可输出报告并以报告文件续读；summary REPORT | summary | reportId 或显式报告文件 + Budget/Cursor |
| diff | diff BEFORE AFTER | diff | 两份输入的文本层差异 + Budget/Cursor；分别重建快照，不要求 nodeId 相同 |
| media | media INPUT --list 或 --id ID --output OUTPUT | media | 清单分页；字节按 id 单独传输，默认最大 16 MiB，超限具名拒绝，不 base64 注入 text |
| addMedia | ops 的媒体附件文件绑定 | addMedia | bytes/附件 + MIME，返回句柄/hash/length、新 version |
| check | check INPUT | check | 会话与 package 诊断、可检查不变式 + Budget/Cursor；明确没有 Word 打开证据 |
| version | version | version | 引擎版本、native 协议版本与独立 agent 协议/投影版本；不改 BIND native/0 的时点 |

CLI --json 输出与 MCP 的结构化 content 相等（排除传输和新会话身份）；人类文本模式从同一结果渲染。
固定大小的 open/close/version 回执不分页，仍受响应字节上限；其余只读命令支持 --limit/--max-bytes/--cursor，修改命令的结果读取也遵守同一预算；不能用无法分页的巨大总结绕过限制。
MCP 写请求必须携带 expectedVersion。CLI 在本次 open 后绑定输入完整字节指纹；跨调用使用 `--preview REPORT` 时核对该指纹、operations/context hash 并重新编译文字选择器，不重用旧 native id。
CLI summary 读取报告文件而不重开或修改文档，报告文件游标同样绑定其完整 SHA-256。
默认输出新文件；覆盖显式授权后用同目录临时文件 + 成功原子替换。任何失败保留原输入/既有输出，禁止半文件覆盖。
原始 EditOp 调试入口不算 AGENT-07 文本编译能力，需显式选择且仍不允许绕过事务与版本推进。
媒体上传输入最大 16 MiB；命令行 JSON 输入最大 4 MiB、嵌套深度最大 128，超限错误，不 panic。
MCP 每实例最多 32 个会话、空闲 30 分钟回收；过期等同 close，后续 BIND_NO_SESSION；活跃请求不在执行中回收。
版本响应声明 agent 协议与投影规则版本；本稿不把尚未发布的 native 协议改成 native/1。

错误码最少覆盖：
`AGENT_BAD_ARGUMENT`、`AGENT_BAD_ANCHOR`、`AGENT_BAD_OFFSET`、`AGENT_NOT_PROJECTED`、`AGENT_TARGET_NOT_FOUND`、`AGENT_AMBIGUOUS`、
`AGENT_NOT_EDITABLE`、`AGENT_UNSUPPORTED_RANGE`、`AGENT_UNREPRESENTABLE`、`AGENT_STYLE_CONFLICT`、
`AGENT_BUDGET_TOO_SMALL`、`AGENT_UNIT_TOO_LARGE`、`AGENT_QUERY_TOO_LARGE`、`AGENT_QUERY_TIMEOUT`、
`AGENT_BAD_CURSOR`、`AGENT_STALE_CURSOR`、`AGENT_STALE_ANCHOR`、`AGENT_VERSION_CONFLICT`、
`AGENT_PREVIEW_STALE`、`AGENT_REPORT_EXPIRED`、`AGENT_REPORT_TOO_LARGE`、`AGENT_RESOURCE_LIMIT`。
原生 `BIND_*`/`EDIT_*`/`SAVE_*` 原样传递；未知 code 走 AGENT-01，不改写成 success。
CLI 参数/业务拒绝退出 2，IO/内部错误退出 1，成功退出 0；MCP 以同样的结构化错误返回失败标记，不只写一段自然语言。

### 验收

表中每行至少一条 CLI/MCP 映射与 schema 用例；有 session 的工具均测不存在 id（close 幂等、version/open 除外）。
缺省预算与错误在两个入口一致；CLI 每个真实子命令端到端，macOS/Linux 构建；MCP 真实 Agent 会话完成 docs/12 的三条 W 任务并保存证据。
超限 JSON/媒体/会话、空闲回收、写盘失败、错误输出大小、native 调试写入口的版本推进均测试。
不新增 rsword 核心运行期依赖；compat-ts 默认仍关，原生和兼容回归网继续运行。

## 关口验收与待订正边界

本文件十条验收均为后续实现的硬条件，**9.1 不报告实现通过数**。
任务分母固定为 docs/12 的 22 项；不以跳过超预算、不可编辑或 Word 未验证任务来缩小分母。
spec/20 的门 2 合成字符映射措辞、B/C 宿主能力初判登记 docs/04 §8，待负责人订正；R10 的“只有机器码”是派工口误，不是 spec/20 原文。
本稿评审通过后才开始 9.2；M8′ 门 2 / 门 4 的 spec/18 裁定仍独立等待。
