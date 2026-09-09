# Agent 预算、截断与游标（9.4）

依据 `spec/22` AGENT-06。实现位于 `tools/agent-query`，由后续 CLI/MCP 共用；
本任务不交付 9.5 的文字编辑编译器、审计报告或 9.6/9.7 的命令与服务。

## 读取与预算

`session::Sessions::read` 接受 `ReadRequest { tool, options }`、可选 `Budget` 与 cursor。
text / outline / find / context / document / diagnostics / media 共用 `budget::longest_prefix`、信封与 `cursor::Registry`；
文本/记录的单位分页由 `paging` 组织。
find 保留完整授权流上的搜索状态与独立 worker；返回记录仍使用相同信封、预算与游标注册表。
旧 `nav::Pages` 也委托同一分页器，不保留旧的游标格式。summary / preview / diff 的完整记录分页构件及预算同表测试已备好，
不把这项构件测试说成 9.5 的预览或报告业务已经实现。

缺省保持 AGENT-06：outline 为 4000 UTF-16 / 16000 B，find 为 4000 / 24000，其他为 8000 / 24000。
字符串 content 按实际 UTF-16 计；结构化 content 按紧凑 JSON 的 UTF-16 计。
完整成功信封含锚点、诊断、省略计数、游标及 usage 本身，responseBytes 与 estimatedTokens 一起求稳定值。
预算低于合法下限时，在查会话或读取输入文件前拒绝；不把内容或游标塞进失败响应。

文本按原投影的完整段落/原子对象切分，表格结构装饰附属相邻单位，续页不重建表头。
文本分页的片段保留全投影 segmentKey 与绝对 UTF-16 区间；页间不重编号。
省略项按对象第一次出现的单位登记，页级字符计数与省略分类可累加；glossary 的不可投影身份、诊断等元数据亦纳入字节预算。
context 把完整窗口拆成同一套段落单位，每条携带 requestedRange / actualRange 及本单位授权对象的详情。

分页取两项预算都容纳的最长完整前缀。末页省掉游标，字节数可能下降，因此不能在前一候选页字节超限时提前停止。
首个单位装不下时返回 `AGENT_BUDGET_TOO_SMALL` 与 object/minLimit/minBytes；单位超过允许硬上限则 `AGENT_UNIT_TOO_LARGE`。
超预算不拆长段、不返回成功空页、不消费游标；错误说明可以收短，错误码与最低重试字段保留。

## 唯一游标与写入边界

所有 Agent 读接口使用同一个 `a1.` 编码器/解析器。该字符串是不透明凭据，不是公开的可写 offset 参数包。
会话模式编码服务端注册句柄；记录绑定 sessionId、version、projectionVersion、工具、语义配置哈希与下一单位位置。
错误、只读与重复读取不消费记录；close 清理所属记录。跨工具/会话/配置为 `AGENT_BAD_CURSOR`，同会话版本变化为 `AGENT_STALE_CURSOR`。
调整 limit/maxBytes/maxHits/deadlineMs 不改变语义配置。锚点的 snapshot 在线型上为结构化对象，内部旧测试字符串仍可往返。

Agent 管理器拥有原生会话与媒体表，外部拿不到可写原生会话引用。
显式原生调试批次 `edit_native` 同样受 expectedVersion 与完整克隆事务约束；批次成功一次推进版本一次，包括成功 no-op。
addMedia 成功一次推进一次，重复登记同一媒体仍算一次成功；失败操作/失败批次/保存/读取不推进。
save 操作克隆。原生表的 `inspect` 是隐藏的只读 Rust 桥，不是新的绑定导出，也不提供会话的可写引用。

`Sessions::read_file` 是文件工具的共享读取构件。游标绑定规范化路径、完整输入 SHA-256、投影版本、语义请求与逻辑单位位置。
重开先核指纹，再建立新会话；不携带旧 nodeId/sessionId 作为跨进程位置。
路径不符报 `AGENT_BAD_CURSOR`，文件字节变化报 `AGENT_STALE_CURSOR`，检查发生在解析 DOCX 之前。
文件 context 用逻辑 `anchorOffset` 在本次快照重新定位；传旧会话 anchor 明确拒绝。
文件游标只是本地只读续页数据，不能作为编辑授权或可信 native 句柄。

## 原生模型契约不变

原生 BIND-10 `document()` 无参仍返回整份，保留 totalBlocks / truncated 与全局索引，不增加 nextCursor。
Agent document 是另一个有界读取入口，显式选择 blockRange/flow 或声明字段；默认 display=false。
`main` 必须伴随显式内容范围，辅助内容经 flow 选择；不能把 `hfParts` / comments 等内容字段伪装成声明字段读取全流。
document 不接受 text 的 scope 参数，诊断/媒体清单也不接受未实现的范围参数，避免接受后静默忽略。
主流直接调用原生 document 的选择器；辅助流从只读模型取得对应块及该 part 的索引，再调用同一个 BIND-10 选择器。
`SessionTable::select_model` 是隐藏的 Rust 复用入口，不是新增协议工具；blockRange / fields / depth 的规则只维护一份。
Agent 才进一步裁剪必要 spans/fields/revisions；按 part 和所选节点/字段引用定位，避免不同 arena 的同号节点混入。
字段引用覆盖行内 Field.id、透明 run.field 与块字段 protectedKind.fieldId，迭代闭包保留父/嵌套引用。
真实 fields/fields-toc.docx 的选中 TOC 块保留 TOC + 三个 PAGEREF，范围外 REF/DATE 不混入。
原生 response 的全局索引不因此改变，Agent 不返回范围外正文哨兵。

## 常驻门与实际破坏

- 全语料精确 1103 份，1099 成功与共享 UNOPENABLE 的 4 份点名拒绝双向锁死。
- 每份完整文本的单位拼接、源/呈现计数与省略分类对独立规范投影；每份抽样前三个完整单位走真实游标分页。
- 五接口都必须产生真实 nextCursor；同游标重读稳定，跨接口误用、预算失败后重读与写入后失效有断言。
- 文件五接口使用同一编码；另起两个独立测试进程验证重开续页与新会话锚点，改变输入一个字节即拒绝。
- 主流/辅助流模型选择与索引分别检查；原生全局哨兵确实存在，Agent 响应不泄漏。
- 非 BMP、JSON 转义、未知诊断、完整元数据、恰好预算、首个单位超限、最大单位超限及七个读接口和未来三类记录的预算共用表。
- 批次先修改后失败仍回到原保存字节/版本；成功修改、no-op、重复 addMedia 与失败 addMedia 的版本分别检查。

实际将生产分页器的下一位置 `end` 改为 `end + 1`，独立拼接断言报红：
实际 `FIRST\nLAST\n`，期望 `FIRST\nMIDDLE😀\nLAST\n`。不是只修改测试期望。
生产文件随后与破坏前备份逐字节比较相同，再运行最终完整检查。
另两处开发期先复现再修：旧 snapshot 字符串不得剥引号或归一格式，TOC 字段引用不能被索引裁剪误删；均有常驻回归。

开发期一轮并发测试有两项在既定 2 s worker 初始化上限失败，尚未开始查询；未改超时或断言，原样重跑通过。
这不是查询性能通过的测量，也未据此推断机器停顿的根因。最终检查数字见 `docs/05-status.md`。

## Worker 握手回归

compat debug 的一轮在既有 worker 恢复测试报异常退出；原运行没有保留 stderr，不能追认该次退出的唯一根因。
排查发现 fc5ada8（9.3）已有可确定复现的竞态：ready 文件出现时子进程可能仍持有写描述符，
父进程却复用该 inode 写请求再 rename；子进程迟到的 ready 写入会覆盖请求 JSON。
9.4 改为独立 pending 文件写完再 rename，Drop 也清理该文件，不修改初始化或查询期限。
常驻脚本持有 ready 的写描述符，故意等提交后再写入；正确协议响应使用 Result 的 Ok 包装。
恢复旧生产暂存方式时测试报 worker 异常退出；独立暂存通过，恢复源码后再跑完整矩阵。
这项测试验证传输交错，不冒充正则执行性能测试。
