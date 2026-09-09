//! AGENT-10：CLI/MCP 共用工具声明；传输层不复制名称、分类、预算或响应 schema。
use crate::{
    Result,
    budget::{self, Budget},
    cursor::{self, Registry},
    paging::{self, Unit},
    session::ReadTool,
};
use serde_json::{Value, json};
macro_rules! agent_tool {
    ($($variant:ident, $logical:literal, $cli:literal, $mcp:literal, $write:literal, $budget:ident, $read:expr;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Tool { $($variant),* }
        impl Tool {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
            pub fn logical(self)-> &'static str {match self {$(Self::$variant=>$logical),*}}
            pub fn cli(self)-> &'static str {match self {$(Self::$variant=>$cli),*}}
            pub fn mcp(self)-> &'static str {match self {$(Self::$variant=>$mcp),*}}
            pub fn writes(self)->bool {match self {$(Self::$variant=>$write),*}}
            pub fn budget(self)->Budget {match self {$(Self::$variant=>Budget::$budget),*}}
            pub fn read(self)->Option<ReadTool> {match self {$(Self::$variant=>$read),*}}
            pub fn from_cli(name:&str)->Option<Self> {Self::ALL.iter().copied().find(|t| t.cli()==name && !matches!(t,Self::Open|Self::Close|Self::Save|Self::AddMedia))}
        }
    }
}
agent_tool! {
    Open,"open","ops","open",true,CONTEXT,None;
    Close,"close","ops","close",true,CONTEXT,None;
    Outline,"outline","outline","outline",false,OUTLINE,Some(ReadTool::Outline);
    Text,"text","text","text",false,CONTEXT,Some(ReadTool::Text);
    Find,"find","find","find",false,FIND,Some(ReadTool::Find);
    Context,"context","context","context",false,CONTEXT,Some(ReadTool::Context);
    Document,"document","model","model",false,CONTEXT,Some(ReadTool::Document);
    Preview,"preview","preview","preview",false,CONTEXT,None;
    Edit,"edit","ops","edit",true,CONTEXT,None;
    Save,"save","ops","save",true,CONTEXT,None;
    Summary,"summary","summary","summary",false,CONTEXT,None;
    Diff,"diff","diff","diff",false,CONTEXT,None;
    Media,"media","media","media",false,CONTEXT,Some(ReadTool::Media);
    AddMedia,"addMedia","ops","addMedia",true,CONTEXT,None;
    Check,"check","check","check",false,CONTEXT,None;
    Version,"version","version","version",false,CONTEXT,None;
}
impl Tool {
    pub fn from_mcp(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.mcp() == name)
    }
    pub fn needs_session(self) -> bool {
        !matches!(self, Self::Open | Self::Version)
    }
    pub fn needs_version(self) -> bool {
        matches!(self, Self::Edit | Self::AddMedia | Self::Save | Self::Preview)
    }
    pub fn description(self) -> String {
        let purpose = match self {
            Self::Open => "打开本机 path 指定的 DOCX，返回 sessionId/version；不修改输入文件。",
            Self::Close => "释放会话、报告和游标；重复关闭成功，不要求 expectedVersion。",
            Self::Outline => "读取标题层级、文字和可下钻的块范围，不预读完整正文。",
            Self::Text => "读取可读文本、占位符和双向锚点；呈现锚点不能直接编辑。",
            Self::Find => "字面/正则定位，支持归一化；返回原文前置条件、锚点与有界上下文。",
            Self::Context => "以当前会话 anchor 按段落或 UTF-16 取窗口，不越过授权范围。",
            Self::Document => {
                "有界模型查询，须选择 blockRange/flow 或声明字段；非原生全量 document。"
            }
            Self::Preview => {
                "克隆执行 operations，保存预览报告，不改文档；后续用 summary/reportId 读取。"
            }
            Self::Edit => {
                "编译并原子执行 Agent operations，返回 reportId；歧义拒绝不猜，可用 previewId 核预览。nativeDebug 只作显式低层调试。"
            }
            Self::Save => {
                "在克隆上保存到本机 output；已存在输出需 overwrite=true；失败保留原文件和会话。"
            }
            Self::Summary => {
                "按 reportId 分页读原始请求、文本差异及可还原审计；报告有容量与淘汰上限。"
            }
            Self::Diff => {
                "比较本机 before/after 两份 DOCX 的完整文本单位，不按 nodeId 配对，不宣称最小 diff。"
            }
            Self::Media => {
                "分页列媒体；指定 id/output 可导出本机文件（上限 16 MiB），不把字节内联进文本。"
            }
            Self::AddMedia => {
                "上传本机 path 的媒体并校验 MIME，返回 mediaId/hash/length，成功推进版本。"
            }
            Self::Check => "检查会话和包诊断、保存校验与可检查不变式；不声称已在桌面 Word 验证。",
            Self::Version => "返回引擎、原生协议与 Agent 投影规则版本，不要求会话。",
        };
        format!(
            "{}：{}先 open，再 outline 获取标题与范围，随后用 text/model/context 按需下钻。limit 计 UTF-16，maxBytes 计完整工具响应；默认 {} / {}。按两形态较大成本共同分页，超预算不截断段落，重试可增预算；cursor 绑定会话版本和查询。{}",
            self.logical(),
            purpose,
            self.budget().limit,
            self.budget().max_bytes,
            if self.needs_version() {
                "必须携带 expectedVersion；失败不改变会话。"
            } else {
                "close 幂等；会话默认空闲 30 分钟回收。"
            }
        )
    }
    /// MCP 的路径/会话路由扩展仍在共享声明侧，不在服务端复制工具表。
    pub fn mcp_schema(self) -> Value {
        let mut schema = self.input_schema();
        schema["properties"]["resultShape"] = json!({"enum":["text","structured"],"description":"只改变传输信封；可在同一会话逐请求切换，游标仍通用"});
        if self == Self::Context {
            schema["properties"]["options"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove("anchorOffset");
        }
        let string = json!({"type":"string","minLength":1});
        if self.needs_session() {
            schema["properties"]["sessionId"] = string.clone();
            schema["required"].as_array_mut().unwrap().push(json!("sessionId"));
        }
        if self.needs_version() {
            schema["required"].as_array_mut().unwrap().push(json!("expectedVersion"));
        }
        let opts = &mut schema["properties"]["options"];
        let (extra, required) = match self {
            Self::Open => (json!({"path":string}), vec!["path"]),
            Self::Save => (
                json!({"output":string,"overwrite":{"type":"boolean"},"saveOptions":{"type":"object"}}),
                vec!["output"],
            ),
            Self::Summary => (json!({"reportId":string}), vec!["reportId"]),
            Self::Diff => (json!({"before":string,"after":string}), vec!["before", "after"]),
            Self::AddMedia => (json!({"path":string,"mime":string}), vec!["path", "mime"]),
            Self::Media => (
                json!({"id":{"type":"integer","minimum":0,"maximum":4294967295u32},"output":string,"overwrite":{"type":"boolean"}}),
                vec![],
            ),
            Self::Edit => (json!({"previewId":string,"nativeDebug":{"type":"boolean"}}), vec![]),
            _ => (json!({}), vec![]),
        };
        opts["properties"].as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        opts["required"].as_array_mut().unwrap().extend(required.into_iter().map(|s| json!(s)));
        if self == Self::Edit {
            // 原生调试仍显式选择，运行时由 BIND-03 的正向线型严格解析。
            opts["if"] =
                json!({"properties":{"nativeDebug":{"const":true}},"required":["nativeDebug"]});
            opts["then"] =
                json!({"properties":{"operations":{"type":"array","items":{"type":"object"}}}});
            let agent_operations =
                opts["properties"].as_object_mut().unwrap().remove("operations").unwrap();
            opts["properties"]["operations"] = json!({"type":"array"});
            opts["else"] = json!({"properties":{"operations":agent_operations}});
        }
        schema
    }
    /// 业务选项的键与运行期校验共用清单；操作字段复用正向编译表的 schema。
    pub fn input_schema(self) -> Value {
        use crate::edit_schema::WireSchema;
        let mut defs = rsword::bind::native::SchemaDefs::default();
        let mut properties = json!({});
        let mut required: Vec<&str> = vec![];
        if let Some(read) = self.read() {
            let (common, extra) = crate::session::option_keys(read);
            for key in common.iter().chain(extra) {
                properties[*key] = match *key {
                    "scope" => json!({"enum":["main","all"]}),
                    "flow" | "blockRange" => {
                        let keys = if *key == "flow" { ["part", "flow"] } else { ["from", "to"] };
                        let mut p = json!({});
                        for k in keys {
                            p[k] = <u32 as WireSchema>::schema(&mut defs);
                        }
                        json!({"type":"object","properties":p,"required":keys,"additionalProperties":false})
                    }
                    "pattern" => json!({"type":"string","maxLength":4096}),
                    "search" => {
                        json!({"type":"object","properties":{"mode":{"enum":["literal","regex"]},"insensitive":{"type":"boolean"},"foldWidth":{"type":"boolean"},"collapseWhitespace":{"type":"boolean"},"deadlineMs":{"type":"integer","minimum":1,"maximum":2000}},"additionalProperties":false})
                    }
                    "anchor" => <rsword::agent::anchors::Anchor as WireSchema>::schema(&mut defs),
                    "fields" => json!({"type":"array","items":{"type":"string"}}),
                    "display" | "detail" => json!({"type":"boolean"}),
                    "unit" => json!({"enum":["utf16","blocks"]}),
                    "minLevel" | "maxLevel" => json!({"type":"integer","minimum":1,"maximum":9}),
                    "maxHits" => json!({"type":"integer","minimum":1,"maximum":1000}),
                    "before" | "after" | "depth" => <u32 as WireSchema>::schema(&mut defs),
                    _ => panic!("新增读取选项必须补 schema: {key}"),
                };
            }
            match read {
                ReadTool::Find => required.push("pattern"),
                ReadTool::Context => {
                    properties["anchorOffset"] = <u32 as WireSchema>::schema(&mut defs);
                }
                _ => {}
            }
        }
        let mut definitions = defs.into_map();
        if matches!(self, Self::Edit | Self::Preview) {
            let mut action = crate::edit::Action::schema();
            if let Some(d) = action.as_object_mut().unwrap().remove("$defs") {
                definitions.extend(d.as_object().unwrap().clone());
            }
            properties["operations"] = json!({"type":"array","items":action});
            properties["context"] = json!({"type":"object"});
            required.push("operations");
        }
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":definitions,"type":"object","properties":{"options":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"limit":{"type":"integer","minimum":1,"maximum":1048576,"default":self.budget().limit},"maxBytes":{"type":"integer","minimum":512,"maximum":4194304,"default":self.budget().max_bytes},"cursor":{"type":["string","null"]},"expectedVersion":{"type":"integer","minimum":0}},"required":["options"],"additionalProperties":false})
    }
    pub fn response_schema(self) -> Value {
        let content = if self == Self::Text {
            json!({"type":"string"})
        } else {
            json!({"type":"array","items":{"type":"object"}})
        };
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","oneOf":[
          {"type":"object","required":["content","snapshot","empty","range","truncated","nextCursor","omitted","anchorCounts","usage"],"properties":{
           "content":content,"anchors":{"type":"object","required":["segments","snapshot","projectionKey"]},"diagnostics":{"type":"array"},"snapshot":{"type":["object","string"]},"empty":{"type":"boolean"},"range":{},"truncated":{"type":"boolean"},"nextCursor":{"type":["string","null"]},"omitted":{"type":"object","required":["page","complete"]},"anchorCounts":{"type":"object","required":["sourceUtf16","presentationUtf16","sourceScalars","presentationScalars"]},"usage":{"type":"object","required":["contentUtf16","responseBytes","estimatedTokens"],"properties":{"contentUtf16":{"type":"integer","minimum":0},"responseBytes":{"type":"integer","minimum":0},"estimatedTokens":{"type":"integer","minimum":0}},"additionalProperties":false}},"additionalProperties":false},
          {"type":"object","required":["code","message","details"],"properties":{"code":{"type":"string","minLength":1},"message":{"type":"string"},"details":{"type":"object"}},"additionalProperties":false}
        ]})
    }
}
/// 报告、双文件 diff 与 check 也使用 9.4 的文件游标，不增设编码。
#[allow(clippy::too_many_arguments)]
pub fn file_page(
    identity: Value,
    digest: &str,
    tool: Tool,
    config: &Value,
    rows: Vec<Value>,
    b: Budget,
    token: Option<&str>,
) -> Result<Value> {
    let b = b.validate()?;
    let binding = json!({"identity":identity,"sha256":digest,"request":cursor::hash(config.to_string().as_bytes()),"projectionVersion":"agent/1-unicode17"});
    if let Some(token) = token {
        cursor::validate_file(token, &binding, tool.logical())?;
    }
    let mut registry = Registry::default();
    registry.file = Some(binding.clone());
    let units: Vec<_> = rows.into_iter().enumerate().map(|(i, r)| Unit::record(r, i)).collect();
    paging::page(
        &mut registry,
        &binding.to_string(),
        tool.logical(),
        &config.to_string(),
        &units,
        false,
        json!({"tool":tool.logical(),"result":config}),
        b,
        token,
        usize::MAX,
    )
}
pub fn fixed(tool: Tool, value: Value, b: Budget) -> Result<Value> {
    let b = b.validate()?;
    let v =
        budget::envelope("agent/1", json!([value]), json!({"tool":tool.logical()}), false, None);
    if budget::fits(&v, b) { Ok(v) } else { Err(budget::too_small(&v, Value::Null)) }
}
pub fn version() -> Value {
    json!({"engine":rsword::bind::native::SessionTable::version(),"agentProtocol":"agent/1","projectionVersion":"agent/1-unicode17"})
}
/// 按完整投影单位比较，不把不同文件的 arena id 当成相同对象；不声称最小 diff。
pub fn diff_rows(before: &[Value], after: &[Value]) -> Vec<Value> {
    (0..before.len().max(after.len()))
        .filter(|&i| before.get(i) != after.get(i))
        .map(|i| json!({"kind":"text","unitIndex":i,"before":before.get(i),"after":after.get(i)}))
        .collect()
}
/// 完整错误信封同样有界；收缩候选保留总数，消息按比例收缩避免平方级重序列化。
pub fn bounded_error(mut e: crate::QueryError, max_bytes: usize) -> crate::QueryError {
    loop {
        let size = crate::transport::common_bytes(&serde_json::to_value(&e).unwrap(), true);
        if size <= max_bytes {
            return e;
        }
        if let Some(candidates) = e.details.get_mut("candidates").and_then(Value::as_array_mut)
            && !candidates.is_empty()
        {
            candidates.pop();
        } else if !e.message.is_empty() {
            let mut end = e.message.len() / 2;
            while !e.message.is_char_boundary(end) {
                end -= 1;
            }
            e.message.truncate(end);
        } else {
            let mut budget = crate::error("AGENT_BUDGET_TOO_SMALL", "错误定位超出响应预算");
            budget.details = json!({"minBytes":size,"minLimit":0,"object":null});
            return budget;
        }
    }
}
