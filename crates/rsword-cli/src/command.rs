//! AGENT-10 文件适配：显式路径、完整文件指纹、报告续读与原子发布。
use super::{args::Args, output, read_bounded, read_json};
use rsword_agent_query::{
    Result,
    budget::Budget,
    cursor::hash,
    edit::WorkerConfig,
    error, paging,
    report::Report,
    session::{ReadRequest, Sessions},
    tools::{self, Tool},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
const MEDIA_LIMIT: usize = 16 * 1024 * 1024;
const REPORT_LIMIT: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Archive {
    format: String,
    input_sha256: String,
    request_hash: String,
    native_debug: bool,
    original: Value,
    attachments: Vec<Value>,
    report: Report,
}
fn archive(bytes: &[u8]) -> Result<Archive> {
    let a: Archive = serde_json::from_slice(bytes)
        .map_err(|e| error("AGENT_BAD_ARGUMENT", format!("报告格式: {e}")))?;
    if a.format != "rsword-report/1" {
        return Err(error("AGENT_BAD_ARGUMENT", "报告版本不支持"));
    }
    Ok(a)
}
fn canonical(path: &str) -> Result<PathBuf> {
    Path::new(path).canonicalize().map_err(output::io)
}
fn worker() -> Result<WorkerConfig> {
    Ok(WorkerConfig {
        program: std::env::current_exe().map_err(output::io)?,
        scratch: std::env::temp_dir().join("rsword-query"),
        args: vec!["__query-worker".into()],
    })
}
fn no_cursor(a: &Args) -> Result<()> {
    if a.get("cursor").is_some() {
        Err(error(
            "AGENT_BAD_CURSOR",
            "写请求和固定回执不消费游标；请用 summary REPORT 续读完整报告",
        ))
    } else {
        Ok(())
    }
}
fn page(
    a: &Args,
    identity: Value,
    digest: &str,
    config: &Value,
    rows: Vec<Value>,
) -> Result<Value> {
    tools::file_page(identity, digest, a.tool, config, rows, a.budget, a.get("cursor"))
}
fn guard_input(path: &Path, original: &[u8]) -> Result<()> {
    if fs::read(path).map_err(output::io)? != original {
        return Err(error("AGENT_VERSION_CONFLICT", "输入文件在本次命令期间改变，未发布"));
    }
    Ok(())
}
type Response = (Value, Option<output::Publication>);
pub fn run(a: Args) -> Result<Response> {
    if matches!(a.tool, Tool::Edit | Tool::Preview) {
        return edit(&a);
    }
    if a.tool == Tool::Media && a.get("id").is_some() {
        return export_media(&a);
    }
    let value = match a.tool {
        Tool::Version => {
            no_cursor(&a)?;
            tools::fixed(a.tool, tools::version(), a.budget)
        }
        Tool::Summary => {
            let path = canonical(&a.paths[0])?;
            let bytes = read_bounded(&path, REPORT_LIMIT)?;
            let report = archive(&bytes)?;
            page(&a, json!(path), &hash(&bytes), &json!({}), report.report.rows)
        }
        Tool::Diff => diff(&a),
        Tool::Check => {
            let path = canonical(&a.paths[0])?;
            let bytes = fs::read(&path).map_err(output::io)?;
            let mut sessions = Sessions::default();
            let id = sessions.open(&bytes)?;
            let rows = sessions.check(&id, &bytes)?;
            page(&a, json!(path), &hash(&bytes), &json!({}), rows)
        }
        _ => {
            let tool =
                a.tool.read().ok_or_else(|| error("AGENT_BAD_ARGUMENT", "工具不是文件命令"))?;
            let path = canonical(&a.paths[0])?;
            // 将 IO 失败和业务拒绝分开，不能把文件不存在伪装成参数/模型错误。
            fs::File::open(&path).map_err(output::io)?;
            let w = if a.tool == Tool::Find { Some(worker()?.start()?) } else { None };
            Sessions::read_file(
                &path,
                &ReadRequest { tool, options: a.options.clone() },
                Some(a.budget),
                a.get("cursor"),
                w,
            )
        }
    }?;
    Ok((value, None))
}
fn diff(a: &Args) -> Result<Value> {
    let paths = [canonical(&a.paths[0])?, canonical(&a.paths[1])?];
    let mut texts = vec![];
    let mut hashes = vec![];
    for path in &paths {
        let bytes = fs::read(path).map_err(output::io)?;
        hashes.push(hash(&bytes));
        let mut s = Sessions::default();
        let id = s.open(&bytes)?;
        let p = s.projection(&id)?;
        let units = paging::text_units(&p, 0..p.anchors.len())?;
        texts.push(units.into_iter().map(|u| u.content).collect::<Vec<_>>());
    }
    let rows = tools::diff_rows(&texts[0], &texts[1]);
    page(
        a,
        json!(paths),
        &hash(json!(hashes).to_string().as_bytes()),
        &json!({"alignment":"projectionUnitOrdinal"}),
        rows,
    )
}
fn export_media(a: &Args) -> Result<Response> {
    no_cursor(a)?;
    let media: u32 =
        a.require("id")?.parse().map_err(|_| error("AGENT_BAD_ARGUMENT", "media id 必须是 u32"))?;
    let path = canonical(&a.paths[0])?;
    let bytes = fs::read(&path).map_err(output::io)?;
    let mut sessions = Sessions::default();
    let id = sessions.open(&bytes)?;
    let media = sessions.media(&id, media)?;
    if media.len() > MEDIA_LIMIT {
        return Err(error("AGENT_RESOURCE_LIMIT", "媒体导出超过 16 MiB"));
    }
    let output = PathBuf::from(a.require("output")?);
    let result = tools::fixed(
        a.tool,
        json!({"output":output,"length":media.len(),"sha256":hash(&media)}),
        a.budget,
    )?;
    guard_input(&path, &bytes)?;
    let publication = output::publish(vec![(output, media)], a.flag("overwrite"))?;
    Ok((result, Some(publication)))
}
struct Attachment {
    name: String,
    bytes: Vec<u8>,
    mime: String,
}
fn attachments(raw: &Value, base: &Path) -> Result<(Vec<Attachment>, Vec<Value>)> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        name: String,
        path: PathBuf,
        mime: String,
    }
    let values: Vec<Input> =
        serde_json::from_value(raw.get("attachments").cloned().unwrap_or(json!([])))
            .map_err(|e| error("AGENT_BAD_ARGUMENT", e.to_string()))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut data = vec![];
    let mut metadata = vec![];
    let mut total = 0;
    for v in values {
        if v.name.is_empty() || !seen.insert(v.name.clone()) {
            return Err(error("AGENT_BAD_ARGUMENT", "附件名称为空或重复"));
        }
        let path = base.join(&v.path).canonicalize().map_err(output::io)?;
        let bytes = read_bounded(&path, MEDIA_LIMIT)?;
        total += bytes.len();
        if total > MEDIA_LIMIT {
            return Err(error("AGENT_RESOURCE_LIMIT", "本次附件合计超过 16 MiB"));
        }
        rsword_agent_query::media::verify(&bytes, &v.mime)?;
        metadata.push(json!({"name":v.name,"path":path,"mime":v.mime,"length":bytes.len(),"sha256":hash(&bytes)}));
        data.push(Attachment { name: v.name, bytes, mime: v.mime });
    }
    Ok((data, metadata))
}
fn edit(a: &Args) -> Result<Response> {
    if a.tool == Tool::Edit {
        no_cursor(a)?;
    }
    let path = canonical(&a.paths[0])?;
    let bytes = fs::read(&path).map_err(output::io)?;
    let ops_path = canonical(a.require("ops")?)?;
    let raw = read_json(&ops_path, super::JSON_LIMIT)?;
    let (files, metadata) = attachments(&raw, ops_path.parent().unwrap())?;
    let request_hash = hash(
        json!({"request":raw,"attachments":metadata,"native":a.flag("native-ops")})
            .to_string()
            .as_bytes(),
    );
    let mut request = if raw.is_array() {
        json!({"operations":raw})
    } else if raw.get("action").is_some() || raw.get("op").is_some() {
        json!({"operations":[raw]})
    } else {
        raw.clone()
    };
    if !request.is_object() {
        return Err(error("AGENT_BAD_ARGUMENT", "操作请求必须是对象或数组"));
    }
    request.as_object_mut().unwrap().remove("attachments");
    let input_sha256 = hash(&bytes);
    if a.tool == Tool::Preview
        && a.get("cursor").is_some()
        && let Some(path) = a.get("report")
    {
        let path = canonical(path)?;
        let data = read_bounded(&path, REPORT_LIMIT)?;
        let old = archive(&data)?;
        if old.input_sha256 != input_sha256 || old.request_hash != request_hash {
            return Err(error("AGENT_PREVIEW_STALE", "输入或操作已改变，不能继续旧预览"));
        }
        let digest = hash(&data);
        let meta = json!({"previewId":format!("file-preview:{digest}"),"executionHash":old.report.audit.execution_hash});
        return page(a, json!(path), &digest, &meta, old.report.rows).map(|v| (v, None));
    }
    let mut sessions = Sessions::default();
    let id = sessions.open(&bytes)?;
    let mut media = BTreeMap::new();
    let mut version = 0;
    for file in files {
        let added = sessions.add_media(&id, version, &file.bytes, &file.mime)?;
        version += 1;
        media.insert(file.name, added["mediaId"].clone());
    }
    if let Some(operations) = request["operations"].as_array_mut() {
        for op in operations {
            if let Some(binding) = op.get("mediaId").filter(|v| v.is_object()) {
                let name = binding
                    .get("attachment")
                    .and_then(Value::as_str)
                    .filter(|_| binding.as_object().unwrap().len() == 1)
                    .ok_or_else(|| {
                        error("AGENT_BAD_ARGUMENT", "mediaId 附件绑定必须为 {attachment:名称}")
                    })?;
                op["mediaId"] = media.get(name).cloned().ok_or_else(|| {
                    error("AGENT_ATTACHMENT_MISSING", format!("附件 {name} 不存在"))
                })?;
            }
        }
    }
    let request = request.to_string();
    let worker = worker()?;
    let mut preview = None;
    if let Some(old) = a.get("preview") {
        let old = archive(&read_bounded(&canonical(old)?, REPORT_LIMIT)?)?;
        if old.input_sha256 != input_sha256
            || old.request_hash != request_hash
            || old.native_debug != a.flag("native-ops")
        {
            return Err(error("AGENT_PREVIEW_STALE", "输入字节、操作、上下文或附件与预览不一致"));
        }
        if a.flag("native-ops") {
            return Err(error("AGENT_BAD_ARGUMENT", "原生调试不复用 Agent 预览"));
        }
        let p = sessions.preview(
            &id,
            version,
            &request,
            Budget { limit: 1048576, max_bytes: 4194304 },
            Some(&worker),
        )?;
        let pid = p["range"]["reportId"]
            .as_str()
            .ok_or_else(|| error("AGENT_INTERNAL", "预览缺报告标识"))?;
        if sessions.report(&id, pid)?.audit.execution_hash != old.report.audit.execution_hash {
            return Err(error("AGENT_PREVIEW_STALE", "重新编译的执行序列与预览不同"));
        }
        preview = Some(pid.to_owned());
    }
    let result = if a.tool == Tool::Preview {
        sessions.preview(
            &id,
            version,
            &request,
            Budget { limit: 1048576, max_bytes: 4194304 },
            Some(&worker),
        )?
    } else if a.flag("native-ops") {
        sessions.edit_native_report(&id, version, &request)?
    } else {
        sessions.edit(&id, version, &request, preview.as_deref(), Some(&worker))?
    };
    let report_id = if a.tool == Tool::Preview {
        result["range"]["reportId"].as_str()
    } else {
        result["reportId"].as_str()
    }
    .ok_or_else(|| error("AGENT_INTERNAL", "缺少报告标识"))?;
    let report = sessions.report(&id, report_id)?;
    let archive = Archive {
        format: "rsword-report/1".into(),
        input_sha256,
        request_hash: request_hash.clone(),
        native_debug: a.flag("native-ops"),
        original: raw,
        attachments: metadata,
        report,
    };
    let report_bytes = serde_json::to_vec(&archive).unwrap();
    if report_bytes.len() > REPORT_LIMIT {
        return Err(error("AGENT_REPORT_TOO_LARGE", "文件审计报告超过 16 MiB，未写入输出"));
    }
    let output =
        if a.tool == Tool::Edit { Some(PathBuf::from(a.require("output")?)) } else { None };
    let report_path = a
        .get("report")
        .map(PathBuf::from)
        .or_else(|| output.as_ref().map(|p| PathBuf::from(format!("{}.report.json", p.display()))));
    let value = if a.tool == Tool::Preview {
        let (identity, digest) = if let Some(p) = &report_path {
            let parent = p
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()
                .map_err(output::io)?;
            (
                json!(parent.join(
                    p.file_name().ok_or_else(|| error("AGENT_BAD_ARGUMENT", "报告必须是文件"))?
                )),
                hash(&report_bytes),
            )
        } else {
            (
                json!({"input":path,"request":request_hash}),
                hash(json!([hash(&bytes), request_hash]).to_string().as_bytes()),
            )
        };
        let meta = json!({"previewId":format!("file-preview:{digest}"),"executionHash":archive.report.audit.execution_hash});
        page(a, identity, &digest, &meta, archive.report.rows.clone())?
    } else {
        let mut receipt = result;
        receipt["output"] = json!(output);
        receipt["report"] = json!(report_path);
        tools::fixed(a.tool, receipt, a.budget)?
    };
    let mut outputs = vec![];
    if let Some(out) = output {
        let options =
            a.get("save-options").map(super::json_input).transpose()?.map(|v| v.to_string());
        outputs.push((out, sessions.save(&id, options.as_deref())?));
    }
    if let Some(report) = report_path {
        outputs.push((report, report_bytes));
    }
    guard_input(&path, &bytes)?;
    let publication = output::publish(outputs, a.flag("overwrite"))?;
    sessions.close(&id);
    Ok((value, Some(publication)))
}
