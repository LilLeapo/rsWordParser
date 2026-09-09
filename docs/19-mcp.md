# 19 · 原生 MCP server（AGENT-10）

9.7 提供 `crates/rsword-mcp`，复用 `tools/agent-query` 的工具声明、查询、预算、版本与编辑事务。
原生 Rust 形态**按 spec/20 建议执行、待追认**。本文记录可连接的实现，不是 docs/12 的真实 Agent 三项 W 任务验收；
该会话由评审者发起并记录，缺省结果形态也待门 5 客户端实测确认。

## 连接

```sh
cargo build -p rsword-mcp --release
./target/release/rsword-mcp --help
```

在支持 stdio MCP 的客户端配置一个服务（command 使用本机工作树的绝对路径）：

```json
{
  "mcpServers": {
    "rsword": {
      "command": "/Users/lilleap/code/rsWordParser-m8j/target/release/rsword-mcp",
      "args": ["--result-shape", "text"]
    }
  }
}
```

工具涉及的 path/output 是 **server 本机文件路径**；推荐绝对路径，不假设客户端与 server 的工作目录相同。
默认新文件输出，只有 `overwrite:true` 才替换既有文件。会话修改留在内存，须显式 save 才落盘。
stdout 仅逐行 UTF-8 JSON-RPC；日志写 stderr。实现 initialize / initialized、ping、tools/list、tools/call，
不宣称 HTTP、远程文件上传、MCP tasks 或实时取消支持。查找计算仍由可终止 worker 的 deadline 约束。

工具名称、inputSchema、默认预算与描述来自 9.6 的同一张 `agent_tool!` 表，当前包括
open/close/outline/text/find/context/model/preview/edit/save/summary/diff/media/addMedia/check/version。
CLI 名与 MCP 名也在该表，不在两个传输层分别登记。

## 调用顺序

以下是 tools/call 的 params，sessionId、version 和 reportId 必须取自当前响应，不能照抄示例值：

```json
{"name":"open","arguments":{"options":{"path":"/absolute/input.docx"}}}
```

open 的业务载荷 `content[0]` 返回 sessionId/version/projectionVersion。先 outline，再按返回范围下钻：

```json
{"name":"outline","arguments":{"sessionId":"CURRENT_ID","options":{}}}
{"name":"text","arguments":{"sessionId":"CURRENT_ID","options":{"blockRange":{"from":0,"to":2}},"limit":8000,"maxBytes":24000}}
{"name":"find","arguments":{"sessionId":"CURRENT_ID","options":{"pattern":"目标文字"}}}
```

find 返回 `content[].anchors.start/end`，context 可使用该锚点；MCP 不接受 CLI 文件入口专用的 anchorOffset。
Agent 编辑的 operations 形状见 [17-agent-edit.md](17-agent-edit.md)；其作用域使用实际返回的 ObjectRef。
preview 需 expectedVersion，在克隆上运行；返回的 reportId 可交 summary 续读，或作为 edit.options.previewId。
edit 成功回执只有版本、reportId 与计数；完整审计经 summary 获取，媒体审计使用外置摘要绑定。

```json
{"name":"summary","arguments":{"sessionId":"CURRENT_ID","options":{"reportId":"CURRENT_REPORT"}}}
{"name":"save","arguments":{"sessionId":"CURRENT_ID","expectedVersion":1,"options":{"output":"/absolute/output.docx"}}}
{"name":"close","arguments":{"sessionId":"CURRENT_ID","options":{}}}
```

edit/addMedia/save/preview 必须提供 expectedVersion；读取可提供该前置条件。close 不要求版本且幂等。
diff 的 before/after 为两个本机输入路径，MCP 同时提供 sessionId 持有结果游标，不能沿用 CLI 文件游标。
显式 nativeDebug 调试编辑也通过版本、审计及原子事务，不计入 Agent 任务集的文本编译通过率。
addMedia 接收本机 path 与 MIME，最大 16 MiB；沿用容器签名校验，不做完整图像解码。media 指定 id/output 可导出文件，字节不内联进工具文本。
check 包含会话和 package 诊断、保存校验、Dirty 等内核可检查项；编辑后不再判“无编辑保存恒等”，并明确无桌面 Word 证据。

## 三个维度与计费

- **形态**：`--result-shape text|structured`，缺省 text；单请求 resultShape 可覆盖缺省。在同一 MCP 会话，两形态共用分页边界、nextCursor、truncated；游标可在两形态间续读。
- **接口**：同一传输的读取共用游标编码；游标仍绑定工具及查询，跨工具误用 AGENT_BAD_CURSOR，不意味着一个 text 游标可以拿去查 outline。
- **传输**：CLI 自包含文件身份/hash/位置，MCP 使用会话短句柄。两侧保证相同逻辑区间与完整续读终态的业务结果，允许相同 maxBytes 下页边界不同；两类游标错投均具名拒绝。

只发送一份业务载荷：text 为 `content:[{type:"text",text:"业务 JSON"}]`，没有 structuredContent；
structured 为 structuredContent，content 仅一行 `Read structuredContent.`。这行说明也计费。
由于请求可切回 text，tools/list 不声明会强制所有结果带 structuredContent 的 outputSchema；
业务响应 schema 仍来自共享 Tool::response_schema，并在测试中校验。

每个候选页按 **两形态成本较大值** 判断 maxBytes，minBytes 同样是两者均可容纳的最低预算。
`usage.responseBytes` 则报告实际形态的 CallToolResult 紧凑 JSON 大小（含 usage 自身求稳定值、转义、说明与 isError），
不计外层 JSON-RPC id/result framing。estimatedTokens=ceil(responseBytes/4)，不是模型 tokenizer 硬上界。
CLI 报实际业务 JSON 大小；跨传输断言明确排除两个 usage 传输字段、快照身份与对应游标标识，不排除业务内容或 contentUtf16。

预算不足不消费游标；编辑回执超预算也必须恢复会话和审计。文件先暂存、原子发布，stdout 写失败恢复已发布输出。
未知参数、重复 JSON 键、过深或超长输入拒绝；stdio 输入行上限 4 MiB。错误保留 code/message/details，并返回 isError=true。

## 生命周期与验证

每实例默认最多 32 个会话，空闲 30 分钟回收，正在执行的请求不回收；过期后 BIND_NO_SESSION。
`--max-sessions` 与 `--idle-timeout-ms` 只允许收紧这些上限，便于受限部署和确定性生命周期测试。

```sh
cargo test -p rsword-mcp
cargo test -p rsword-mcp --test memory -- --nocapture
cargo build -p rsword-cli -p rsword-mcp
node tools/ci/check-agent-transports.mjs target/debug/rsword target/debug/rsword-mcp
```

memory 在独立测试进程用 stats_alloc 计实际在用堆字节（分配减释放，重分配已计入），
预热后反复打开最大真实件、建立文本游标和审计，再 close 或超时回收。服务实例本身仍存活；
必须释放至少 90% 的会话增量，残余不超过 16 KiB，五轮不得累积泄漏。它不把 RSS 的分配器缓存当泄漏。
真实进程测试覆盖工具表/schema、hostile 点名拒绝、会话上限、空闲、失败原子性、stdout 断管恢复文件、
跨形态及跨传输游标；CI 在 macOS/Linux 跑，当前本机执行证据与最终数字见 [docs/05](05-status.md)。

协议依据：[MCP stdio](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、
[生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)、
[工具结果与 outputSchema](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。
MCP 的兼容建议允许同时提供文本与结构化副本，本项目经评审选择单份载荷加显式说明，以满足 AGENT-06 完整计费。
