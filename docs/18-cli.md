# 18 · 文件级 CLI（M9′ 9.6，AGENT-10）

## 1. 使用

在仓库根目录构建 `cargo build -p rsword-cli --release`，二进制为 `target/release/rsword`。
可用 `cargo run -p rsword-cli --` 代替二进制前缀。核心 rsword 的运行期依赖未增加；
CLI 依赖共享 `rsword-agent-query`，没有将参数解析、进程执行器或 schema 校验库塞进内核。

```sh
rsword outline corpus/real/misc/large-report.docx --json
rsword text corpus/real/misc/large-report.docx --limit 4000 --maxBytes 16000 --json
rsword find corpus/real/misc/large-report.docx --pattern '文档' --json
rsword context corpus/real/misc/large-report.docx --offset 0 --before 0 --after 100 --json
rsword model corpus/real/misc/large-report.docx --options '{"blockRange":{"from":0,"to":1}}' --json
rsword check corpus/real/misc/large-report.docx --json
rsword version --json
```

`find` 可加 `--regex / --ignore-case / --fold-width / --collapse-whitespace`，详细搜索选项走
`--options '{"search":{"deadlineMs":250}}'`。同一业务键不可同时通过快捷参数与 options 指定。
`context --offset` 是本次文件投影的 UTF-16 偏移，文件适配器重建锚点，不复用上次进程的 arena 身份；
`--unit` 取 `utf16` 或 `blocks`。`model` 保留 Agent 显式选择规则，需要 blockRange/flow 或声明 fields，
不以无参完整预读绕过预算。底层 BIND-10 的原生 `document()` 契约没有改动。

## 2. 同表工具与预算

`tools/agent-query/src/tools.rs::agent_tool!` 是 CLI/MCP 的名称、映射、读写分类、预算及读取分派来源。
工具输入 schema 的读取选项键来自运行期校验同一份清单，编辑操作来自 `Action::schema()`；
响应 schema 统一声明完整信封与错误。测试把表与 spec/22 的逻辑工具/MCP 名称表双向比较，
并要求每个实际 CLI 命令都有真实进程测试。MCP 服务器本轮未交付，留给 9.7。

所有命令接受 `--limit / --maxBytes / --cursor / --json`。limit 单位为 UTF-16，maxBytes 计算完整
紧凑 JSON 信封，包括锚点、诊断、游标和转义；JSON stdout 不附加换行，usage.responseBytes 即 stdout 字节数。
人类可读模式仅格式化同一个结果，不走第二条查询路径。参数/业务错误退出 2，IO/内部错误退出 1，成功退出 0。
预算在读取前校验；输入 JSON 上限 4 MiB、深度 128，重复键拒绝；媒体单件及一次上传合计上限 16 MiB。

text/outline/find/context/model/media 清单复用 Sessions::read_file；summary/diff/check/preview 的记录
复用同一个 Registry、file binding 和 paging::page。文件游标绑定规范路径、完整 SHA-256、请求与工具名，
跨工具具名拒绝，文件改变失效；续读必须保持原 options。固定 version、写回执与媒体导出不消费游标，
传 cursor 会具名拒绝，完整编辑结果由 summary 续读，绝不为取下一页重新执行写操作。

## 3. 编辑、预览与审计

```sh
rsword preview input.docx --ops request.json --report preview.json --json
rsword ops input.docx --ops request.json --preview preview.json --output edited.docx --json
rsword summary edited.docx.report.json --json
rsword diff input.docx edited.docx --json
```

request.json 是 9.5 的 `{operations:[...],context:{...}}`，也接受单个操作或操作数组。
默认是文本锚定 action；原生 EditOpJson 必须显式 `--native-ops`，仍经过 Agent 版本检查、整批克隆事务与审计。
`--save-options` 接受 BIND-04 的 JSON。首次读取定位后使用 ObjectRef/原文前置条件编写操作；
start/end 中的会话锚点不跨 CLI 进程重用；文件编辑使用 find + scope + 原文前置条件重新定位，
不静默重绑旧 snapshot。现有 W7、批次符号引用、桌面 Word 未验等边界仍见 docs/12 §8，不因包装 CLI 宣称补齐。

ops 必须指定 output；默认还生成 `OUTPUT.report.json`，可用 --report 指定。报告是 `rsword-report/1`，
保留原请求、输入完整 hash、附件元数据及共享 Report。summary 绑定完整报告文件 hash，不能在分页期间篡改报告。
preview 可显式写报告，也可只返回分页结果；前者续页读原报告，后者重做无副作用候选并核指纹。
跨进程 --preview 重用先校验输入字节、请求/上下文和媒体摘要，再重新编译并比较 executionHash，
不把旧进程的节点身份盲传进新会话。原生调试模式不复用 Agent 预览。

附件通过 request 顶层 `attachments:[{name,path,mime}]` 声明，path 相对请求文件。
replaceImage 的 mediaId 可用 `{"attachment":"name"}`，先经共享 add_media 上传，再绑定本次句柄。
上传推进会话版本。MIME 校验仍是容器签名，非完整图像解码。
审计恢复保留五要素及执行序列总 hash 校验；附件级长度/hash/MIME 不匹配另带
`details:{stage:"attachment",operation,sha256Prefix}`，使用户可定位具体绑定，也能独立测试该守卫。

输出与报告先在各自目标目录 create_new 暂存并 sync，再发布；既有文件默认拒绝覆盖，显式 --overwrite 才允许。
多输出的发布失败会恢复已发布文件；成功回执通过复制的文件描述符直接 write_all 完成前保留备份，stdout 失败也回滚；
不用会吞掉 EBADF 的 Stdout 写路径，只读描述符与 broken pipe 两种失败均有断言。
不把多文件更新声称为断电级原子事务；操作系统崩溃不在进程内回滚保证内。恢复失败时保留备份路径并报 IO 错误。

## 4. 媒体、diff 与 check 的边界

`rsword media INPUT --list --json` 列出媒体，`--id MEDIA_ID --output image.bin` 导出原媒体字节。
id 为响应中的 mediaId；不猜编号，不另建写入 DOCX 的入口。替换媒体须通过 ops。

diff 比较 Scope::All 文本投影中的完整单位，按单位序号对齐，输出不同的 before/after；
不把两份文档的 NodeId 当同一对象，也不宣称最小 LCS diff 或排版差异。比较逻辑在共享 Agent 层。

check 返回会话及包两侧诊断、当前输入无编辑保存字节恒等、已物化 XML 的 Dirty 传播检查、
两侧 EngineInvariantViolation 计数。保存仍调用已有保存校验器；它并非任意干净 XML 的完整 OOXML 验证器。
返回值显式写 `wordOpen:notVerified`，不能把这些检查当作桌面 Word 无修复提示的证据。
无法打开的输入具名错误退出 2，不静默跳过。完整 hostile 分母是 38，4 份既有打开拒绝共用 UNOPENABLE 并双向锁死。

## 5. 验证

每个实际 CLI 命令独立 E2E；响应 schema 拒绝根部额外键；输入 schema 拒绝多余业务键。
跨进程续页、跨工具误用、文件变更失效、预览持久化/重新编译、附件上传/导出、原生调试整批失败、
预算失败前不写、第二个文件发布失败与 stdout broken pipe 后恢复旧输出均有常驻测试。
附件 SHA 条件实际删除后，附件 details 断言报红；恢复后重跑通过。

CI 配置 macOS/Linux 两平台 CLI E2E。本轮 macOS 本机执行 E2E，并用 Rust 1.98.0
交叉构建 aarch64-unknown-linux-musl，产物经 file 确认为静态链接 Linux ELF。Linux 运行测试尚未执行
（未 push；本机容器服务未运行），不把交叉构建写成运行通过。最终两套 feature 与差分数字记在 docs/05。

开发期全测曾两次在共享 worker 的 2 秒初始化守卫报错，独立编辑测试通过；停止改动源码后，
同一批产物的完整默认测试通过。保留失败日志，不以单独通过替代全门，也未增加超时、跳过用例或屏蔽错误。
目前只确认初始化阶段失败，未建立唯一根因，不记为已修复的既有引擎缺陷。
