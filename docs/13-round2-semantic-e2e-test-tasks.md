# 第二轮：真实 Word 文档的语义读取与公开 API 验收

日期：2026-09-10。执行者：另一台 Mac 上的 Codex + computer use。

## 0. 直接开始执行

用户要求继续全面测试，不限制时间、轮数和 token。本轮要实现工具、创建 Word 语料、执行验证、修复确认的缺陷并留下证据，不只输出计划。继续使用实际分支 `docs/codex-test-handoff`，不要另建一个名为 codex-test 的平行分支。

该分支已同步 main 的 `66b575f`。这个代码基线修复了 native 错误转换的非穷尽匹配，并处理了新 Clippy 对两份测试文件的提示。本机执行过默认 workspace 测试：982 passed、13 ignored、0 failed；fmt 和严格 Clippy 通过。这不是第二轮语义验收结果，也不代替目标 Mac 的干净构建。

先读目标仓库的实际指令，然后检查工作区。干净且在预期分支时执行：

```sh
git fetch origin
git switch docs/codex-test-handoff
git pull --ff-only origin docs/codex-test-handoff
git rev-parse HEAD
git status --short
```

若有未提交工作，先在独立副本留存 patch、未跟踪文件及哈希，或使用独立 worktree；不得 reset/clean 覆盖它们。若分支分叉，检查各自提交后解决，不能强推。以开始执行时实际获取的提交为基线，并确认包含 `66b575f` 或其等价修复。

本轮允许在该测试分支提交并推送测试、修复与证据。不要自动合入 main。原始 Word 文件不可覆盖，用户私有文件不可上传，账号登录和软件付费等边界沿用第一轮要求。

## 1. 第一轮成果与必须纠正的缺口

第一轮入口：`evidence/20260909T052808Z/report.md`、`capability-matrix.csv`、`word-authored/*/read-report.md`。六套 Word 语料、阶段检查点、独立审计、随机参考模型、变形与 fuzz 结果均保留并复用。

本轮针对以下已核实问题：

1. `rsword_e2e_driver --check` 仅检查正文 marker 和无编辑保存字节一致；默认修改模式调用 `Dom::set_text`，文件注释已明确它不能证明公开 EditOp 正确。
2. 第一轮报告把部分 driver 未覆盖的表格内部、批注、文本框等记为 NOT_IMPLEMENTED。当前代码已有更完整的模型入口，必须逐项重新验证，不能直接继承这些结论。
3. `reference_model.rs` 的段落格式模型只有 `jc`，字符格式主要为 bold/italic。两百万随机步不能证明间距、缩进或所有格式正确。
4. 缺少 Word 段前段后、行距、首行两字符、悬挂缩进、样式继承等逐字段端到端断言。
5. 第一轮报告声称构建通过，但提交 `1a05dd7` 在本机不能默认编译。第二轮必须将源码指纹、构建日志和最终报告绑定，验收已提交的代码。

不要删改第一轮历史日志来让其符合新结论。新报告以勘误表记录原结论、当前证据、正确分类和复现方法。

规范仍以 `spec/05-properties.md`、`06-model.md`、`07-resolve.md`、`08-edit.md`、`09-save.md`、`10-compat-ts.md`、`11-testing.md` 及当前 native/Agent 规范为准。旧交接见 docs/11、docs/12；本文只增加本轮执行要求。

## 2. 任务顺序与交付依赖

| 任务 | 内容 | 依赖 | 完成证据 |
| --- | --- | --- | --- |
| R2-00 | 冻结源码、干净构建、记录工具链 | 无 | 基线指纹与退出码 |
| R2-01 | 实现全内容流读取报告和断言驱动 | R2-00 | 驱动测试、实际 JSON、负向自测 |
| R2-02 | 复核六套旧 Word 语料及误分类 | R2-01 | 逐结构勘误表 |
| R2-03 | Word 段落格式专项与继承矩阵 | R2-01 | Word/XML/raw/effective/API 对照 |
| R2-04 | 公开 EditOp 编辑、回滚、Word 回验 | R2-02、R2-03 | 操作回执、语义差异、保真、截图 |
| R2-05 | 扩展参考模型与定向变异 | R2-03 定义的独立语义 | 种子、最小反例、变异结果 |
| R2-06 | final commit 干净验证与报告 | 以上任务 | 可重跑命令、版本映射、最终矩阵 |

先完成一个小用例从 Word 创建到最终报告的闭环，再扩展矩阵。无需把旧版全部长 fuzz 再跑一次才开始 R2-01。修复改变相关逻辑后再补相应压力验证。

## 3. R2-00：可复现基线

记录 macOS、Word build、CPU、字体环境、Rust/Cargo/Clippy、Codex 与 computer use 能力、Git HEAD、源文件清单哈希、Cargo.lock、feature 集合。不要导出完整环境变量或密钥。

在干净源码副本执行并分别保留日志和退出码：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo clippy --workspace --all-targets --features compat-ts -- -D warnings
cargo test --workspace --locked --features compat-ts
```

另外验证仓库声明的最低 Rust 版本，以及目标机器实际开发工具链，分别记录。不因本机较新 lint 修改业务语义。仓库 CI 有额外边界或 schema 门禁时，按实际 workflow 补齐；执行前先读，不凭旧报告猜命令。

编译或测试失败先保存基线，最小修复独立提交，继续任务。不得临时关闭 native/default feature、删除失败测试或扩大已知差异名单来伪造通过。

## 4. R2-01：让读取工具真正读取结构与属性

建议新增 `rsword_semantic_driver` 或等价工具，保留旧 driver 的既有行为。名称和命令行是待实现建议，不是已经存在的命令。新增调用方式必须写进工具 README，并以实际命令执行验证。

优先核对和使用以下当前源码入口，不复制生产实现来计算期望：

| 领域 | 代码入口与注意事项 |
| --- | --- |
| 正文与表格 | `model/table.rs` 的 `Document::blocks()`、`paragraphs()`、`tables()`；`text_blocks()` 仅顶层，不能代表整份文档 |
| 辅助 part | `Document::blocks_of_part()`、`hf_parts`、`comments`、`footnotes`、`endnotes`；读取所属 part URI |
| 文本框 | `model/table.rs::box_flows()`、外部文本框相关入口；核对 Blocks 遍历实现，避免遗漏或重复 |
| 字段与范围 | `fields_in()`、`field_result_text()`、`range_locations_in()`；区分指令、缓存结果、目标关系 |
| 节 | `Document::sections` 与 `model/section.rs`；解析页眉页脚引用和继承，不能只数 sectPr |
| 格式 | 原始 ParaProps/RunProps 与 `Resolver::new()`、`para()`、`run()`；表格上下文按实际接口提供 |
| 编辑 | `EditSession::apply()`、`apply_all()`、`save()`；调用当前 API，不能套旧版签名 |
| 对外输出 | `bind/native/`、Agent 查询工具、显式开启的 `bind/compat_ts/`；分别验证实际产品承诺的字段 |

读取输出至少包括：part URI、内容流种类、稳定定位信息、块类型、文本、原始属性、有效属性与可用 provenance、表格行列/合并/单元格内容、图片资源关系及尺寸、字段、书签、批注正文及范围、节和页眉页脚引用、诊断。

稳定定位使用“part + 结构路径 + 唯一文字锚点/领域 ID”等组合。NodeId 只用于本次会话，不做跨重开的身份依据。相同文字重复出现时必须报歧义或进一步限定；禁止静默选择第一个。

断言驱动必须支持：缺失字段失败、值和单位比较、预期错误码、结构数量和归属、成功读取次数。不能只输出原始 JSON 后让人工扫一眼算通过。不存在、未知、显式零、继承值、Raw 值要分别表示。

建议每条机器结果格式如下，字段名称可调整但信息不可省略：

```json
{
  "case_id": "R2-PARA-SPACE-01",
  "source_sha256": "<actual>",
  "source_commit": "<actual>",
  "location": {"part": "word/document.xml", "anchor": "R2-PARA-SPACE-01"},
  "checks": [{"layer": "effective", "field": "spacing.before", "unit": "twip", "expected": 240, "actual": 240, "status": "PASS"}]
}
```

给驱动写负向自测：锚点在错误 part、重复锚点、错误间距、遗漏单元格、错批注目标、读取输出字段缺失均必须失败。期望文件先从操作计划与独立 XML 建立，不能从内核输出反向自动生成 golden。

## 5. R2-02：复核旧复杂语料

对六份 C05 及有价值的中间检查点执行新驱动。优先选择第一轮被标为 NOT_IMPLEMENTED 的结构：单元格正文、批注正文、文本框、字段、页眉页脚。

逐项判断：模型确实具备且通过；模型具备但 driver 未调用；native/compat 输出缺失；内核实际读取错误；规范尚未承诺的能力。给每项具体 API 路径与断言，不能以类名存在就声称完整支持。

至少检查：单元格文字属于正确行列，横向/纵向合并对应准确；批注正文与起止范围/ID 一致；文本框没有混入正文或被重复计数；header/footer 对应正确 section/variant；字段指令、显示结果及引用目标不混淆。格式不同但文字相同的节点必须区分。

独立 XML 判据覆盖 relationship owner、类型、目标和缺失目标，不只检查目标文件存在。原始语料本身的异常与内核引入的异常分别记录。

## 6. R2-03：Word 段落格式专项

创建新的 `CASE-PARA-FORMAT-02`，必须用 computer use 在 Word UI 中设置真实段落属性。不得把修改 XML 的文件伪装为 Word 创建。程序生成的边界夹具单列。

每组先做短文档，再复制到综合文档、表格单元格、列表段落中交叉验证。通过 Word“段落/样式”实际对话框设置并回读确认，不用空格模拟缩进或空段落模拟段间距。

| ID | Word 设置 / 场景 | 关键断言 |
| --- | --- | --- |
| SPACE-01 | 段前 12 磅、段后 18 磅 | raw/effective/API 的单位与数值；常规 twip 表达为 240/360 |
| SPACE-02 | 显式零与从样式继承非零间距 | 缺省和显式零不同，直接零应按契约覆盖继承 |
| SPACE-03 | 同样式相邻段落，切换“不添加间距” | contextualSpacing 状态与样式归属；不凭视觉反推原始值 |
| SPACE-04 | 自动段前/段后；按行设置的间距（UI 可用时） | 自动标记和 beforeLines/afterLines 不丢失；记录实际优先级 |
| LINE-01 | 单倍、1.5 倍、2 倍 | auto 的原始 line 单位为 1/240 行，不误当 twip；240/360/480 |
| LINE-02 | 固定值 20 磅 | exact 与数值 400 twip；不转换成倍数 |
| LINE-03 | 最小值 20 磅 | atLeast 与数值 400 twip，区别于固定值 |
| LINE-04 | 与文档网格对齐开关 | snapToGrid 与行距分别读取，视觉效应单独验收 |
| INDENT-01 | 首行缩进 2 字符 | 如 Word 持久化 firstLineChars，值应表达百分之一字符；2 字符为 200，不当作 200 twip |
| INDENT-02 | 首行长度缩进（建议可精确换算的 0.5 英寸） | 常规 firstLine 为 720 twip；UI 实际单位和保存值留证 |
| INDENT-03 | 悬挂缩进、左右缩进组合 | hanging 与 firstLine 的区别；兼容层负值约定单独检查 |
| INDENT-04 | 清除直接缩进、恢复样式值 | raw 删除与 effective 恢复，不能统一变零 |
| INDENT-05 | 字符单位和长度单位的合法冲突夹具 | 单列程序生成测试，按规范与 Word 观察确定优先级 |
| INHERIT-01 | 文档默认 → 基础样式 → 派生样式 → 直接格式 | 每层单独覆盖某个子属性，验证复合属性继承 |
| INHERIT-02 | 只覆盖段后，保留继承的段前和行距 | 防止整组 spacing 被替换导致其他值消失 |
| INHERIT-03 | 修改样式后多个引用段落同步变化 | effective 变化正确，无直接属性的段落仍无直接属性 |
| CONTEXT-01 | 列表缩进与直接段落缩进叠加/覆盖 | numbering 层参与，不能用普通段落 oracle 代替 |
| CONTEXT-02 | 单元格内和正文相同段落设置 | 内容流定位正确，表格上下文单独提供 |
| FLOW-01 | 与下段同页、段中不分页、段前分页、孤行控制 | keepNext/keepLines/pageBreakBefore/widowControl 原始与有效值 |

以上数值是选定单位下的可检验期望；Word 可能选择不同合法持久化表达，要解释等价而非盲改 expected。字符缩进到物理长度涉及字体/布局时，不用任意换算系数。若目标 API 只承诺保留字符值而不承诺像素值，应按契约验收，并明确显示层缺口。

每项五层证据：Word 设置截图 → 独立 ZIP/XML（包含 styles/numbering/settings 等来源）→ 内核 raw → resolver effective/provenance → native/compat 实际输出。最后在原 Word 环境打开内核输出回读段落设置；页面渲染不属于内核布局承诺。

auto、字符单位、按行间距等 UI 不可用时记 BLOCKED_UI，继续制作明确标记的程序边界夹具，但不能把它算 Word UI 覆盖。

## 7. R2-04：公开 API 的真实编辑闭环

必须使用公开 EditOp 路径，不用 Dom::set_text 代替。已有低层保真测试保留，分别统计。

从原始 Word 检查点建立独立副本，每类先做单操作：SetParaProps 设置/清除间距与缩进；SetRunProps 局部字符格式；含 emoji 的 InsertText/DeleteRange；跨 run 修改；ReplaceInlines 保留段落属性；支持的表格单元格与辅助 part 编辑。

在有效支持范围内增加 20～50 步声明式操作链，混合格式、文字、保存重开、非法请求。每步输出独立预期、actual、MutationResult、诊断和受影响范围。批处理 apply_all 的失败原子性按当前契约专门验证，不能假定逐个 apply 的多个调用天然是一个事务。

清除、显式 false/zero、Keep 三类 patch 必须分开。不得将未支持返回视为成功，除非该用例预期就是拒绝；失败后比较模型、dirty、保存结果，并继续一次合法操作。

每个主题至少一次公开编辑后 Word 打开验收；格式专项全部关键用例回读 Word 对话框。不得点击修复后算通过。无编辑整包一致；有编辑检查允许变化范围之外的词法片段、条目原始压缩数据及规定元数据。

注意：默认 no-edit save 可能直接返回原始 ZIP，所以它不能独立证明解析结果正确。公开编辑触发保存后的重解析与属性断言不可省略。

## 8. R2-05：扩展参考模型和测试自身验证

在现有 reference_model 基础上增加 spacing、line rule、字符/长度缩进、显式零、复合属性局部覆盖。独立实现一套受限的样式继承 oracle，注明范围，不调用生产 Resolver 计算 expected。

先穷举小状态（缺省、零、非零、清除；基础样式、派生样式、直接值），再跑至少 1,000 seeds × 100 步。分层统计属性组合、错误分支、保存重开次数，再视覆盖扩大；不为追求上一轮数字忽略缺失断言。

保存原始操作序列、种子、生成器版本，失败自动最小化。保留原 Word 文档和缩减后的程序文档各自来源。

定向变异或故障注入至少包括：漏读 firstLineChars、把 auto line 当 twip、把缺省当零、修改段后时清掉段前、错误继承来源、跳过单元格、aux part 混入正文、公开编辑未刷新 effective、失败后保留 dirty。每个变异应由对应独立断言捕获，未捕获必须解释或补测试。

## 9. 结果分类与产物

输出到新的 `evidence/<round2-run-id>/`，不覆写第一轮文件。建议提供：

```text
baseline.json
commands.jsonl
capability-matrix.csv
round1-corrections.csv
word-authored/CASE-PARA-FORMAT-02/
semantic-results/
public-edit-results/
failures/
logs/
progress.md
report.md
reproduce.sh
```

PASS 必须注明具体层（Word 创建、XML 结构、raw、effective、native、compat、公开编辑、保真、Word 回验），不能用单个 PASS 代替全部链路。

允许状态：PASS、FAIL、NOT_TESTED、BLOCKED_UI、NOT_IMPLEMENTED、SKIPPED_WITH_REASON。NOT_IMPLEMENTED 必须给规范范围、实际入口调查及最小例子；driver 缺实现归测试工具缺口。所有计划用例都进入分母，缺文件不能静默跳过。

源码提交与产物关系：先提交最终受测源码，在独立干净 worktree 针对该提交复跑；随后单独提交证据，报告明确 source_commit 与 evidence_commit。避免报告声称验证了一个还未存在的提交，也不要把 dirty 工作区通过等同于已推送代码通过。

保存完整失败命令和退出码，管道输出需保留原命令退出状态。编译缓存可以加速开发，但最终检查必须说明源码干净、feature/工具链明确；如果使用缓存导致版本疑点，再用独立 target 目录复现。

大型截图/二进制遵循仓库现有存储约定，附 SHA-256 与可访问位置。小回归样例应能被普通测试自动发现，不能只留在证据目录而无测试入口。公开共享前检查 Word 作者等个人元数据。

## 10. 验收标准与结束方式

本轮完成必须同时满足：

1. 最终提交在目标 Mac 干净构建；默认与 compat-ts 门禁均有日志，最低工具链和实际工具链状态明确。
2. 新驱动能遍历并定位正文、单元格、辅助 part 和支持的文本框，负向自测证明漏读/错位会失败。
3. 第一轮六份最终 Word 文档完成逐领域重读，原 NOT_IMPLEMENTED 逐项勘误。
4. 段落专项每项都有状态，受支持关键用例有五层断言与 Word 回验；尤其首行两字符、悬挂、段前段后、固定/最小行距、继承覆盖。
5. 公开 EditOp 端到端覆盖与低层 set_text 分开，失败回滚和未编辑保真有实际证据。
6. 新模型和定向变异捕获相关错误，所有缺陷能复现；修复后的测试不是通过删除或放宽断言实现。
7. 新报告列出通过范围、未解决失败、阻塞和产品能力缺口；不能将未完成任务描述为全面通过。

阶段性提交到 docs/codex-test-handoff 并推送。最终交付一段中文摘要：受测源码提交、实际执行规模、修复、剩余问题、复现命令、报告路径。不要自行合入 main。

开始后先给出基线检查结果、当前 Word 能力、R2-01 的具体工具接口，以及首个 SPACE-01 闭环结果。长任务维护 progress.md 和恢复命令，续接时从检查点继续，不重启整轮。

## 11. Suggested skills

按目标环境可用性读取 diagnosing-bugs（缺陷定位）、tdd（回归修复）、pdf（同环境视觉辅助）、handoff（续接）。本轮 Word 真实语料仍由 computer use 创建，不能以程序生成文档技能替代。无需额外审批流程来执行已授权的测试与测试分支提交；外部发送、付费与账号等操作另按实际授权处理。
