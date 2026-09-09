//! AGENT-10：薄参数适配，业务配置交给共享查询/操作类型校验。
use rsword_agent_query::{Result, budget::Budget, error, tools::Tool};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub struct Args {
    pub tool: Tool,
    pub paths: Vec<String>,
    pub flags: BTreeMap<String, String>,
    pub options: Value,
    pub budget: Budget,
}
impl Args {
    pub fn parse(args: &[String]) -> Result<Self> {
        let tool = args
            .first()
            .and_then(|s| Tool::from_cli(s))
            .ok_or_else(|| error("AGENT_BAD_ARGUMENT", "需要子命令；使用 --help 查看"))?;
        let mut flags = BTreeMap::new();
        let mut paths = vec![];
        let mut i = 1;
        while i < args.len() {
            let arg = &args[i];
            i += 1;
            if let Some(key) = arg.strip_prefix("--") {
                let key = if key == "maxBytes" { "max-bytes" } else { key };
                let switch = matches!(
                    key,
                    "json"
                        | "overwrite"
                        | "native-ops"
                        | "list"
                        | "regex"
                        | "ignore-case"
                        | "fold-width"
                        | "collapse-whitespace"
                );
                if !switch
                    && !matches!(
                        key,
                        "options"
                            | "limit"
                            | "max-bytes"
                            | "cursor"
                            | "ops"
                            | "output"
                            | "report"
                            | "preview"
                            | "pattern"
                            | "scope"
                            | "offset"
                            | "before"
                            | "after"
                            | "unit"
                            | "id"
                            | "save-options"
                    )
                {
                    return Err(error("AGENT_BAD_ARGUMENT", format!("未知参数 --{key}")));
                }
                let value = if switch {
                    "true".into()
                } else {
                    let v = args
                        .get(i)
                        .ok_or_else(|| error("AGENT_BAD_ARGUMENT", format!("--{key} 缺少值")))?
                        .clone();
                    i += 1;
                    v
                };
                if flags.insert(key.to_owned(), value).is_some() {
                    return Err(error("AGENT_BAD_ARGUMENT", format!("重复参数 --{key}")));
                }
            } else {
                paths.push(arg.clone());
            }
        }
        let n = match tool {
            Tool::Version => 0,
            Tool::Diff => 2,
            _ => 1,
        };
        if paths.len() != n {
            return Err(error("AGENT_BAD_ARGUMENT", format!("{} 需要 {n} 个路径", tool.cli())));
        }
        let mut options = match flags.get("options") {
            Some(s) => super::json_input(s)?,
            None => json!({}),
        };
        if !options.is_object() {
            return Err(error("AGENT_BAD_ARGUMENT", "--options 必须是对象"));
        }
        for key in ["pattern", "scope", "unit"] {
            if let Some(v) = flags.get(key) {
                if options.get(key).is_some() {
                    return Err(error("AGENT_BAD_ARGUMENT", format!("重复业务选项 {key}")));
                }
                options[key] = json!(v);
            }
        }
        for (key, wire) in [("offset", "anchorOffset"), ("before", "before"), ("after", "after")] {
            if let Some(v) = flags.get(key) {
                let n: u32 = v
                    .parse()
                    .map_err(|_| error("AGENT_BAD_ARGUMENT", format!("--{key} 必须是 u32")))?;
                if options.get(wire).is_some() {
                    return Err(error("AGENT_BAD_ARGUMENT", format!("重复业务选项 {wire}")));
                }
                options[wire] = json!(n);
            }
        }
        for (key, wire) in [
            ("regex", "mode"),
            ("ignore-case", "insensitive"),
            ("fold-width", "foldWidth"),
            ("collapse-whitespace", "collapseWhitespace"),
        ] {
            if flags.contains_key(key) {
                if options.get("search").is_none() {
                    options["search"] = json!({});
                }
                if !options["search"].is_object() {
                    return Err(error("AGENT_BAD_ARGUMENT", "search 必须是对象"));
                }
                if options["search"].get(wire).is_some() {
                    return Err(error("AGENT_BAD_ARGUMENT", format!("重复搜索选项 {wire}")));
                }
                options["search"][wire] = if key == "regex" { json!("regex") } else { json!(true) };
            }
        }
        let mut budget = tool.budget();
        if let Some(v) = flags.get("limit") {
            budget.limit =
                v.parse().map_err(|_| error("AGENT_BAD_ARGUMENT", "limit 必须是整数"))?;
        }
        if let Some(v) = flags.get("max-bytes") {
            budget.max_bytes =
                v.parse().map_err(|_| error("AGENT_BAD_ARGUMENT", "maxBytes 必须是整数"))?;
        }
        budget.validate()?;
        let a = Self { tool, paths, flags, options, budget };
        a.validate_flags()?;
        Ok(a)
    }
    fn validate_flags(&self) -> Result<()> {
        for key in self.flags.keys() {
            let allowed = match key.as_str() {
                "json" | "limit" | "max-bytes" | "cursor" => true,
                "options" | "scope" => self.tool.read().is_some(),
                "pattern" | "regex" | "ignore-case" | "fold-width" | "collapse-whitespace" => {
                    self.tool == Tool::Find
                }
                "offset" | "before" | "after" | "unit" => self.tool == Tool::Context,
                "ops" | "report" => matches!(self.tool, Tool::Edit | Tool::Preview),
                "native-ops" | "preview" | "save-options" => self.tool == Tool::Edit,
                "output" => matches!(self.tool, Tool::Edit | Tool::Media),
                "overwrite" => {
                    matches!(self.tool, Tool::Edit | Tool::Preview | Tool::Media)
                }
                "id" | "list" => self.tool == Tool::Media,
                _ => false,
            };
            if !allowed {
                return Err(error(
                    "AGENT_BAD_ARGUMENT",
                    format!("{} 不接受 --{key}", self.tool.cli()),
                ));
            }
        }
        if self.tool == Tool::Media && self.get("output").is_some() && self.get("id").is_none() {
            return Err(error("AGENT_BAD_ARGUMENT", "媒体导出需要 --id，不能忽略 --output"));
        }
        if self.tool == Tool::Media && (self.flag("list") && self.get("id").is_some()) {
            return Err(error("AGENT_BAD_ARGUMENT", "--list 与 --id 互斥"));
        }
        Ok(())
    }
    pub fn flag(&self, key: &str) -> bool {
        self.flags.contains_key(key)
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.flags.get(key).map(String::as_str)
    }
    pub fn require(&self, key: &str) -> Result<&str> {
        self.get(key).ok_or_else(|| error("AGENT_BAD_ARGUMENT", format!("需要 --{key}")))
    }
}
