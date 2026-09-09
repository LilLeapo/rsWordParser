# 17 · 文本锚定编辑、预览与审计

对应 `spec/22` AGENT-07/08/09，任务 9.5。接口位于工具层 `rsword-agent-query`，不是新增原生协议导出。

## 1. 输入与事务

`Sessions::edit(id, expectedVersion, input, previewId, workerConfig)`；input 为
`{"operations":[{"action":"replaceText","selector":{"scope":[ObjectRef],"find":"原文","all":true},"text":"新文"}],"context":{}}`。
`context` 使用原生 EditContext 的结构化形态。时间缺席保留 None；核心不补当前时间，预览与提交使用同一份类型化上下文。
原始请求保留，操作身份由类型化后的紧凑 JSON 固定键序确定；输入任何层的重复键拒绝。

选择器必须显式 scope；文字使用 find（默认 literal/exact，可选 search 配置）或 start/end 源锚点与 original 前置条件。
occurrence 从 1 开始，与 all 互斥。没有命中报 TARGET_NOT_FOUND，歧义给最多 8 个有定位的候选与总数。
find 复用可终止 worker，完整收集分页命中后倒序编译；单个编译集合上限 100,000，超过整批拒绝，不提交前缀。
呈现端点或跨呈现片段具名拒绝并带 owner；中间源段逐段验证 part、段号与 UTF-16 连续性。
块、表格、图片操作使用显式 ObjectRef。moveBlocks 接受按原序连续的完整主块，不隐式携带分节属性。
原生仅支持主 part 的操作先查 part，不能把辅助 arena 的同号节点当正文目标。

候选会话整批克隆。后续选择器读取候选状态；任一步失败不交换原生状态，不推进版本，不登记编辑报告。
批次内符号引用尚未实现，不能猜新节点 id；需要前一步产物的任务分批读取新锚点后再编辑。

## 2. 同表清单与当前边界

下列成文清单由常规测试与 Action 操作表双向对照。`supported` 表示编译与原生执行已测，不等于全部端到端任务/桌面 Word 已验收。

```agent-actions
replaceText W1/W11 supported
insertParagraphAfter W2 supported
setBlockStyle W6 supported
deleteBlock W10 supported
moveBlocks W10 supported
deleteTableColumn W4 supported
addComment W5 supported
acceptRevisions W3 supported
updateToc W7 pending
setHeaderFooter W8 supported
replaceImage W9 supported
```

每个动作由表展开载荷、编译分派、schema、覆盖清单与一条审计测试。复杂校验放普通函数。
updateToc 当前返回 AGENT_UNSUPPORTED_RANGE：缺页码来源书签核对与生成报告，不猜页码，不算通过。
acceptRevisions 只处理可映回授权对象的宿主；无法定位或跨作者配对拒绝，提交前检查其他作者记录仍在。
setBlockStyle 可创建 paragraph 样式；同声明重试不 upsert，冲突拒绝，正文只打 style patch。
replaceImage 按绘图出现对象替换引用。addMedia 检查 PNG/JPEG/GIF/BMP/WebP/TIFF 容器签名与 MIME；
这是类型校验而非完整图像解码。SVG/EMF 等尚无已交付的内容验证器，当前具名拒绝，不猜扩展名。

## 3. 预览与报告

preview 与 edit 共用候选执行。预览首次分页成功后才登记 previewId；版本和业务状态不提交。
previewId 绑定会话、版本、类型化请求/上下文与实际执行序列 hash；不匹配返回 AGENT_PREVIEW_STALE。
编辑成功只回 `{beforeVersion,afterVersion,reportId,counts}`，摘要不能反过来使已提交编辑报失败。
summary 使用 AGENT-06 唯一游标实现，但绑定不可变报告快照；后续编辑不使历史报告游标失效。
close 后不可访问；编辑报告与预览缓存分开，各最多 32 份 / 16 MiB（按持有 JSON 载荷及内部索引副本序列化大小计），FIFO 淘汰。
单份报告超容量在候选提交前返回 AGENT_REPORT_TOO_LARGE。测试以小于输入容量但产生超容量报告的真实编辑验证回滚。

报告保留原始请求及独立授权描述、实际 MutationResult、块前后文字、格式/移动/媒体引用类型、部件前后 hash、诊断及 XML 逃生口计数。
完整记录按不可拆单位分页；读取时遇预算不足不消费游标。自然语言措辞生成仍在后续 Agent 工具/模型层，当前输出结构化事实。

## 4. 媒体外置与重放

执行只用正向 EditOpJson。`Audit::capture` 将 ReplaceImageMedia.bytes 外置，记录操作序号、JSON Pointer `/bytes`、SHA-256、长度和 MIME。
审计占位为 `{"$attachment":"sha256"}`，它不是原生 EditOpJson。附件由调用方在重放时另供。
`Audit::restore` 在返回任何操作前校验所有绑定及附件，再恢复完整线型并比对执行序列 hash。
缺失为 AGENT_ATTACHMENT_MISSING；内容、绑定、长度、MIME 或完整序列不匹配为 AGENT_ATTACHMENT_MISMATCH。
测试逐字节比较还原前后规范序列，并分别篡改附件、操作、五项绑定和删除附件。打印审计不内联图片字节。

## 5. 本轮发现的旧门洞与修复

- set_segment_text 原先等保存才补 xml:space；连续删除/插入及批注拆分之间的重建会裁边界空白。现在提交时即保留，原生偏移测试直接复现。
- styles 声明写成 w:Type，重建读不到 paragraph 类型；改为 w:type。同声明重试检查暴露并锁住该问题。
- refresh_blocks 更新了块与字段却未更新 FlowMap；追踪创建的 run 无流身份。现在同时重建映射，编号策略不改。
- ModelFingerprint 对 w:t/w:delText/w:instrText 元素调用仅接受文本节点的 Dom::text，导致 T/F 空串。改读活文本子节点；FIRST 与 OTHER 原先双视图相同的反例先红后绿。
- diff_str 的共同字节前缀可能落在 UTF-8 字符内部；“甲/申”差异现在按完整字符报告，避免诊断代码自身 panic 干扰失败签名。
- 指纹原来只读物理标记，忽略完整范围待物化的逻辑锚点。现在独立按 Span 内容边界生成遍历事件，不调用保存/物化。半开孤儿按 SPAN-09 保留物理字节，两条常驻测试覆盖待物化批注与孤儿书签，建立索引前后结果相同。
- 修正指纹后，image-cropped--comment-resaved-by-word 的 InsertText@0 暴露不追踪快路径绕过范围 affinity，批注起点为 0，而追踪后为 1。范围端点禁用直接扩写 run，沿正常内容项插入路径更新锚点；不改变既定 affinity。
- 随机序列 cjk-layout__002 / 429126359206643922 缩成 22 步，暴露修订解包误把已搬出的后代判死，书签端点折成反序并重发同号标记。ContentDelta 区分搬往存活父节点的子树，unwrap 用 ContainerMerge 平移内部边界；短用例覆盖块级/行内包裹，断言精确边界、物理身份与重开仅一对。
- 扩到 1,000 条后，layout-fidelity__001 / 7912911188902404317 缩成 23 步，暴露新建空批注仍用 Right/Left；下一次插入拆反端点。公共登记入口立即统一空范围 affinity 为 Right，遵循 SPAN-02；独立短用例验证创建时两端、插入后的精确字符位置及重开不重复。
- m6-chart__042 / 5148378436044499658 暴露指纹把段外标记挂到待删除的空段，制造了不存在的块。段外 Span 另记规范块流的结构边界（以规范前缀长度标识），名称、种类、位置仍全部比较；末端边界在不占内容下标的 sectPr 之前。常驻测试要求 reject 不凭空多段、书签不丢失、改名仍改变指纹，且逻辑/物理投影相同。
- dropcap__001 / 18351572306291724201 再次暴露懒索引分支的不一致：索引未建时直接读物理标记，已有索引时按逻辑边界。现在未建时只读解析临时 SpanIndex，所有完整范围统一按内容边界定位；测试将同一下标的物理标记放到属性两侧，指纹必须相同。不调用保存/物化作为 oracle。
- m6-image__020 / 16588334391582655360 缩成 10 步，暴露 AddComment 把无单一节点的字段原子误判为段尾，创建时端点已经反序。content_boundary 复用既有 boundary_node 的字段首尾解析；短用例在创建、插入和重开三个阶段检查端点及批注数量，不靠最终保存修正位置。
- random_ops 的旧“拒绝批注”输入 [7,17) 本来合法，依赖裁空白错误才失败；保留为新增必须成功测试，原拒绝回滚门改用明确越界端点。没有把失败改成可跳过。

上述修复不更改 spec/18 的待裁定语义，也不宣称 M8′ 门 2/4 已获裁定。

开发期也出现并修复了一处本轮引入的回归：最初给所有 set_segment_text 目标加 preserve，误及图表 c:v，
既有 chart_ops 散点数值测试报红。最终只处理 w:t/w:delText/w:instrText/w:delInstrText；不把这次开发错误记成旧引擎缺陷。

另在写侧复核时发现零长匹配曾使用整份投影的默认 affinity，而读侧已实现授权范围末端取 left。
两者现在共用 hit_anchors；只授权首段时末端呈现字符必须具名拒绝，段首仍可插入，次段原字节不变。
实际将末端条件破坏后，用例因得到成功回执而报红；恢复源码后重跑。此处是本轮编译器缺陷，不归因于旧读侧。

## 6. 完整请求样例（每个 action 一份，常规进程测试执行）

以下 ID 只适用于点名的未编辑 corpus 文件，不是可跨文档照抄的通用 ID。每个请求独立从原件开始，
不要把这些步骤串接在同一份已修改文档上。真实输入用下节的 find 锚点通路解析对象。

从仓库根目录构建 CLI 后，可以直接执行任一完整样例：

```sh
cargo build -p rsword-cli
node tools/agent-edit-example.mjs addComment
```

运行器把下方 JSON 原样提取到本树 target 下的独立临时目录并调用 `rsword ops INPUT --ops REQUEST.json --output NEW.docx --json`，
输入 corpus 不修改；输出路径随回执打印。也可复制对应 JSON 为 REQUEST.json，以上述命令执行。
replaceImage 的 replacement.png 与 REQUEST.json 同目录，先用 `rsword media corpus/real/image/image-svg.docx --id 0 --output replacement.png --json` 导出。
updateToc 是明确失败样例，其他样例必须执行成功；这不是 Word 或真实 Agent 的任务通过率。

### replaceText

输入：`corpus/real/text/text-basic.docx`。替换唯一命中；全部替换用 all:true，不是 occurrence:"all"。

<!-- agent-example replaceText text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "replaceText",
      "selector": {
        "scope": [
          {
            "part": 2,
            "flow": 0,
            "node": 27,
            "kind": "paragraph"
          }
        ],
        "find": "Mixed English",
        "occurrence": 1
      },
      "text": "已核对"
    }
  ],
  "context": {}
}
```

### insertParagraphAfter

输入：`corpus/real/text/text-basic.docx`。在二级标题后插段。

<!-- agent-example insertParagraphAfter text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "insertParagraphAfter",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 19,
        "kind": "paragraph"
      },
      "text": "本节摘要。",
      "style": null
    }
  ],
  "context": {}
}
```

### setBlockStyle

输入：`corpus/real/text/text-basic.docx`。创建并应用样式；start 是整数。createStyle.paraProps 是样式属性值，正文仅打 style patch，保留直接字号。

<!-- agent-example setBlockStyle text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "setBlockStyle",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 27,
        "kind": "paragraph"
      },
      "styleId": "AgentQuote",
      "createStyle": {
        "styleId": "AgentQuote",
        "kind": "paragraph",
        "name": "AgentQuote",
        "basedOn": null,
        "runProps": null,
        "paraProps": {
          "indent": {
            "start": 720
          }
        }
      }
    }
  ],
  "context": {}
}
```

### deleteBlock

输入：`corpus/real/text/text-basic.docx`。只删除指定正文段落。

<!-- agent-example deleteBlock text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "deleteBlock",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 27,
        "kind": "paragraph"
      }
    }
  ],
  "context": {}
}
```

### moveBlocks

输入：`corpus/real/text/text-basic.docx`。把完整正文段移到一级标题前。

<!-- agent-example moveBlocks text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "moveBlocks",
      "targets": [
        {
          "part": 2,
          "flow": 0,
          "node": 27,
          "kind": "paragraph"
        }
      ],
      "before": {
        "part": 2,
        "flow": 0,
        "node": 11,
        "kind": "paragraph"
      }
    }
  ],
  "context": {}
}
```

### deleteTableColumn

输入：`corpus/real/table/table-styled.docx`。column 从 0 开始；删除第三逻辑列。

<!-- agent-example deleteTableColumn table/table-styled.docx ok -->
```json
{
  "operations": [
    {
      "action": "deleteTableColumn",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 11,
        "kind": "table"
      },
      "column": 2
    }
  ],
  "context": {}
}
```

### addComment

输入：`corpus/real/text/text-basic.docx`。author 必填；date:null 保留未提供时间，不自动补当前时间。

<!-- agent-example addComment text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "addComment",
      "selector": {
        "scope": [
          {
            "part": 2,
            "flow": 0,
            "node": 27,
            "kind": "paragraph"
          }
        ],
        "find": "Mixed English",
        "occurrence": 1
      },
      "author": "Agent",
      "text": "需要复核",
      "date": null
    }
  ],
  "context": {}
}
```

### acceptRevisions

输入：`corpus/real/revisions2/rev-insert-delete.docx`。只接受授权段落内该作者的修订。

<!-- agent-example acceptRevisions revisions2/rev-insert-delete.docx ok -->
```json
{
  "operations": [
    {
      "action": "acceptRevisions",
      "scope": [
        {
          "part": 2,
          "flow": 0,
          "node": 2,
          "kind": "paragraph"
        },
        {
          "part": 2,
          "flow": 0,
          "node": 11,
          "kind": "paragraph"
        },
        {
          "part": 2,
          "flow": 0,
          "node": 26,
          "kind": "paragraph"
        },
        {
          "part": 2,
          "flow": 0,
          "node": 36,
          "kind": "paragraph"
        },
        {
          "part": 2,
          "flow": 0,
          "node": 51,
          "kind": "paragraph"
        }
      ],
      "author": "作者甲"
    }
  ],
  "context": {}
}
```

### updateToc

输入：`corpus/real/fields2/fields-toc-stale.docx`。这是可运行的拒绝样例：必须返回 AGENT_UNSUPPORTED_RANGE；不能生成成功输出。

<!-- agent-example updateToc fields2/fields-toc-stale.docx AGENT_UNSUPPORTED_RANGE -->
```json
{
  "operations": [
    {
      "action": "updateToc",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 18,
        "kind": "field"
      }
    }
  ],
  "context": {}
}
```

### setHeaderFooter

输入：`corpus/real/text/text-basic.docx`。section.node 从 model sections 取得；part/flow 保留其所属正文身份。

<!-- agent-example setHeaderFooter text/text-basic.docx ok -->
```json
{
  "operations": [
    {
      "action": "setHeaderFooter",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 185,
        "kind": "section"
      },
      "kind": "header",
      "variant": "default",
      "paragraphs": [
        "Agent 审阅稿"
      ]
    }
  ],
  "context": {}
}
```

### replaceImage

输入：`corpus/real/image/image-two-in-run.docx`。CLI 附件绑定；精确替换第二个出现位置。运行器先从 image-svg 导出 PNG 为 replacement.png。MCP 用 addMedia 返回的数值 mediaId，并携带更新后的版本。

<!-- agent-example replaceImage image/image-two-in-run.docx ok -->
```json
{
  "operations": [
    {
      "action": "replaceImage",
      "target": {
        "part": 2,
        "flow": 0,
        "node": 69,
        "kind": "image"
      },
      "mediaId": {
        "attachment": "replacement"
      }
    }
  ],
  "context": {},
  "attachments": [
    {
      "name": "replacement",
      "path": "replacement.png",
      "mime": "image/png"
    }
  ]
}
```

## 7. 从 find 锚点取得编辑授权（含无标题文档）

outline 的 group 是导航摘要，不是 paragraph ObjectRef。扁平文档可从 group.blockRange 下钻，
或直接用 find 找源文字；**不要把 group 对象作为 selector.scope**。
find 的 `pageHits` 是本页数，`hasMore=true` 必须按 nextCursor 续读，直到 false 才可统计总数。

下面完整流程从真实 find 响应生成 addComment 请求；它没有硬编码 nodeId：

```sh
mkdir -p target/agent-find-scope
./target/debug/rsword find corpus/real/text/text-basic.docx --pattern 'Mixed English' --json > target/agent-find-scope/hits.json
node --input-type=module <<'JS'
import {readFileSync, writeFileSync} from 'node:fs';
const page = JSON.parse(readFileSync('target/agent-find-scope/hits.json', 'utf8'));
if (page.hasMore || page.content.length !== 1) throw new Error('须先完整续读并明确选择命中');
const hit = page.content[0];
const anchor = hit.anchors.start;
if (!hit.editable || anchor.kind !== 'source') throw new Error('呈现字符不能作为文字编辑授权');
const scope = [{part:anchor.part, flow:anchor.flow, kind:'paragraph', node:anchor.inlinePos.para}];
writeFileSync('target/agent-find-scope/request.json', JSON.stringify({operations:[{
  action:'addComment', selector:{scope,find:hit.match,occurrence:1}, author:'Agent',text:'需要复核',date:null
}],context:{}}, null, 2));
JS
./target/debug/rsword ops corpus/real/text/text-basic.docx --ops target/agent-find-scope/request.json --output target/agent-find-scope/commented.docx --json
```

此示例只授权命中所在段落，不是授权整份文档；若要全部匹配，必须先续读、收集并核对全部授权段落，
按 `(part,flow,kind,node)` 去重后显式 `all:true`。跨段范围须覆盖两端及中间的授权对象；不要仅取起点猜测。
`anchor.node` 是源文字节点，**不是**段落节点；`inlinePos.para` 才是 paragraph 的 node。
呈现锚点的 owner 可用于相应对象操作，但不能因此绕过文字编辑的 AGENT_NOT_EDITABLE。

当前寻址形状并排如下（本轮不改执行契约）：

| action 类别 | 寻址字段 | 值形状 |
| --- | --- | --- |
| replaceText / addComment | selector.scope + find 或 start/end/original | ObjectRef 数组；occurrence 是从 1 开始的 u32，全部选用 all:true，两者互斥 |
| insertParagraphAfter / setBlockStyle / deleteBlock / deleteTableColumn / updateToc / setHeaderFooter / replaceImage | target | 单个 ObjectRef，kind 必须对应操作对象 |
| moveBlocks | targets + before | 原序连续 ObjectRef 数组 + 单个目的 ObjectRef |
| acceptRevisions | scope + author | ObjectRef 数组 + 精确作者字符串 |

样式的 createStyle 使用已声明的 StyleUpsertSave 值对象；没有 indentStartTwips 字段。
`paraProps.indent.start` 的值是整数，例如 720，不能写 `{value:720}`。
正文 setBlockStyle 只编译 style patch；一般属性 patch 的 Keep/Unset/Set 分别是缺席/null/值本身，
不是额外包一层 map。两种寻址风格尚未统一，以上完整请求是当前可执行契约。
