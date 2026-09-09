//! AGENT-08/09：候选执行与不可变报告；容量检查先于业务提交。
use crate::{
    Result,
    audit::Audit,
    cursor::hash,
    edit::{Compiler, Request, WorkerConfig},
    error,
    paging::Unit,
};
use rsword::{
    agent::text::{self, Projection, Scope},
    bind::native::SessionTable,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
const LIMIT: usize = 16 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub id: String,
    pub snapshot: String,
    pub request: Value,
    pub before_version: u64,
    pub rows: Vec<Value>,
    pub audit: Audit,
    pub bytes: usize,
}
#[derive(Default)]
pub struct Store {
    edits: VecDeque<Report>,
    previews: VecDeque<Report>,
}
impl Store {
    pub fn get(&self, id: &str) -> Result<&Report> {
        self.edits
            .iter()
            .chain(&self.previews)
            .find(|r| r.id == id)
            .ok_or_else(|| error("AGENT_REPORT_EXPIRED", "报告不存在或已淘汰"))
    }
    pub fn insert(&mut self, r: Report, preview: bool) {
        let list = if preview { &mut self.previews } else { &mut self.edits };
        while list.len() >= 32 || list.iter().map(|r| r.bytes).sum::<usize>() + r.bytes > LIMIT {
            list.pop_front();
        }
        list.push_back(r);
    }
}
pub struct Candidate {
    pub native: SessionTable,
    pub report: Report,
    pub counts: Value,
}
fn project(native: &SessionTable, id: &str, snapshot: &str) -> Result<Projection> {
    native.inspect(id, |s, _| {
        text::project(s.package(), s.document(), Scope::All, snapshot)
            .map_err(crate::QueryError::from)
    })?
}
fn parts(native: &mut SessionTable, id: &str) -> Result<BTreeMap<String, String>> {
    let parts = native.inspect(id, |s, _| {
        s.package()
            .parts()
            .iter()
            .filter(|p| !p.deleted)
            .map(|p| (p.id.0, p.uri.to_string()))
            .collect::<Vec<_>>()
    })?;
    parts.into_iter().map(|(part, name)| Ok((name, hash(&native.part_bytes(id, part)?)))).collect()
}
pub fn run(
    native: &SessionTable,
    native_id: &str,
    snapshot: Value,
    input: &str,
    worker: Option<&WorkerConfig>,
    preview: bool,
) -> Result<Candidate> {
    if input.len() > LIMIT {
        return Err(error("AGENT_REPORT_TOO_LARGE", "原始请求超过报告容量"));
    }
    let request = Request::parse(input)?;
    let typed = serde_json::to_value(&request).unwrap();
    let original = crate::audit::parse(input)?;
    let context = serde_json::to_string(&request.context).unwrap();
    let mut candidate = native.clone();
    let before = project(native, native_id, &snapshot.to_string())?;
    let before_parts = parts(&mut candidate, native_id)?;
    let before_diagnostics = candidate.diagnostics(native_id)?;
    let mut executed = vec![];
    let mut results = vec![];

    for (index, action) in request.operations.iter().enumerate() {
        let step = (|| -> Result<()> {
            let protected = if let crate::edit::Action::AcceptRevisions { author, .. } = action {
                candidate.inspect(native_id, |s, _| {
                    s.document()
                        .revisions
                        .entries()
                        .iter()
                        .filter(|r| r.author() != Some(author.as_str()))
                        .cloned()
                        .collect::<Vec<_>>()
                })?
            } else {
                vec![]
            };
            let projection = project(&candidate, native_id, &snapshot.to_string())?;
            let mut cx =
                Compiler { native: &mut candidate, id: native_id, projection: &projection, worker };
            let ops = action.compile(&mut cx)?;
            for op in ops {
                let result = candidate.apply(
                    native_id,
                    &serde_json::to_string(&op).unwrap(),
                    Some(&context),
                )?;
                results.push(serde_json::from_str::<Value>(&result).unwrap());
                executed.push(op);
            }
            if !protected.is_empty() {
                let retained = candidate.inspect(native_id, |s, _| {
                    protected.iter().all(|old| {
                        s.document().revisions.entries().iter().any(|new| {
                            new.part == old.part && new.meta == old.meta && new.kind == old.kind
                        })
                    })
                })?;
                if !retained {
                    return Err(error(
                        "AGENT_UNSUPPORTED_RANGE",
                        "接受修订会删除或改变其他作者的待决记录",
                    ));
                }
            }
            Ok(())
        })();
        if let Err(mut e) = step {
            e.details["operationIndex"] = json!(index);
            return Err(e);
        }
    }
    let after = project(&candidate, native_id, &snapshot.to_string())?;
    let after_parts = parts(&mut candidate, native_id)?;
    let diagnostics: Value = serde_json::from_str(&candidate.diagnostics(native_id)?).unwrap();
    let old: Value = serde_json::from_str(&before_diagnostics).unwrap();
    if diagnostics["xmlEscapeCount"] != old["xmlEscapeCount"] {
        return Err(error("AGENT_UNREPRESENTABLE", "编辑需要 XML 逃生口"));
    }
    let audit = Audit::capture(&executed);
    let mut rows = vec![];
    let keys: BTreeSet<_> = before.objects.keys().chain(after.objects.keys()).collect();
    for key in keys {
        let a = before.objects.get(key);
        let b = after.objects.get(key);
        // 流根覆盖整份正文，不是差异单位；避免一次局部修改被放大成全文 diff。
        if !["paragraph", "table", "image", "chart", "math", "ink", "ole", "protected"]
            .contains(&b.or(a).unwrap().object.kind.as_str())
        {
            continue;
        }
        let a_text = a.map(|o| before.text_range(o.range.clone()).unwrap());
        let b_text = b.map(|o| after.text_range(o.range.clone()).unwrap());
        if a_text != b_text {
            let kind = if a.is_none() {
                "insert"
            } else if b.is_none() {
                "delete"
            } else {
                "text"
            };
            rows.push(json!({"kind":kind,"object":b.or(a).unwrap().object,"before":a_text,"after":b_text}));
        }
    }
    // 文本不变的操作仍输出类型标记和实际 MutationResult，不能误称无变化。
    for (index, action) in request.operations.iter().enumerate() {
        let kind = match action.name() {
            "setBlockStyle" => "format",
            "replaceImage" => "mediaReference",
            "moveBlocks" => "move",
            _ => continue,
        };
        rows.push(json!({"kind":kind,"operationIndex":index,"request":action}));
    }
    let mut changed = 0;
    for uri in before_parts.keys().chain(after_parts.keys()).collect::<BTreeSet<_>>() {
        if before_parts.get(uri) != after_parts.get(uri) {
            changed += 1;
            rows.push(json!({"kind":"part","uri":uri,"beforeHash":before_parts.get(uri),"afterHash":after_parts.get(uri)}));
        }
    }
    let counts = json!({"operations":request.operations.len(),"nativeOperations":executed.len(),"changedParts":changed,"diffUnits":rows.len()});
    rows.push(json!({"kind":"request","original":original,"canonical":typed,"authorized":request.operations,"beforeVersion":snapshot["version"],"afterVersion":snapshot["version"].as_u64().unwrap()+1}));
    for (index, op) in audit.operations.iter().enumerate() {
        rows.push(json!({"kind":"editOp","index":index,"value":op}));
    }
    rows.push(json!({"kind":"attachments","bindings":audit.attachments,"executionHash":audit.execution_hash}));
    for (index, result) in results.into_iter().enumerate() {
        rows.push(json!({"kind":"mutation","index":index,"value":result}));
    }
    rows.push(
        json!({"kind":"diagnostics","before":old,"after":diagnostics,"views":after.diagnostics}),
    );
    let id = format!(
        "{}-{}",
        if preview { "preview" } else { "edit" },
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let version = snapshot["version"].as_u64().unwrap();
    let mut report_snapshot = snapshot;
    report_snapshot["reportId"] = json!(id);
    // 计入报告的所有持有字段，包括内部索引副本，不只计算响应的大小。
    let bytes = serde_json::to_vec(
        &json!({"rows":rows,"request":typed,"audit":audit,"snapshot":report_snapshot,"id":id}),
    )
    .unwrap()
    .len();
    if bytes > LIMIT {
        return Err(error("AGENT_REPORT_TOO_LARGE", "完整审计报告超过 16 MiB，未提交"));
    }
    Ok(Candidate {
        native: candidate,
        report: Report {
            id,
            snapshot: report_snapshot.to_string(),
            request: typed,
            before_version: version,
            rows,
            audit,
            bytes,
        },
        counts,
    })
}
impl Report {
    pub fn units(&self) -> Vec<Unit> {
        self.rows.iter().cloned().enumerate().map(|(i, v)| Unit::record(v, i)).collect()
    }
}
