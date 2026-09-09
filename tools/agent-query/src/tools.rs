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
        let size = serde_json::to_vec(&e).unwrap().len();
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
