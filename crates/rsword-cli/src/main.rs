//! AGENT-10 文件级 CLI；所有业务在共享 Agent 层，stdout 只写一个结果。
mod args;
mod command;
mod output;
use rsword_agent_query::{Result, error};
use serde_json::Value;
#[cfg(unix)]
use std::os::fd::AsFd;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
const JSON_LIMIT: usize = 4 * 1024 * 1024;
fn json_input(s: &str) -> Result<Value> {
    if s.len() > JSON_LIMIT {
        return Err(error("AGENT_RESOURCE_LIMIT", "JSON 输入超过 4 MiB"));
    }
    rsword_agent_query::audit::parse(s)
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut data = vec![];
    File::open(path)
        .map_err(output::io)?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(output::io)?;
    if data.len() > limit {
        return Err(error("AGENT_RESOURCE_LIMIT", "文件超过该输入通道的字节上限"));
    }
    Ok(data)
}
fn read_json(path: &Path, limit: usize) -> Result<Value> {
    let b = read_bounded(path, limit)?;
    let s =
        std::str::from_utf8(&b).map_err(|_| error("AGENT_BAD_ARGUMENT", "JSON 必须是 UTF-8"))?;
    rsword_agent_query::audit::parse(s)
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "__query-worker") {
        let code = rsword_agent_query::search::worker_entry(&args[1..]);
        std::process::exit(code);
    }
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!(
            "rsword <outline|text|find|context|model|ops|preview|summary|diff|media|check|version> [INPUT] [AFTER]\n通用: --json --limit N --maxBytes N --cursor TOKEN --options JSON\nfind: --pattern TEXT [--regex --ignore-case --fold-width --collapse-whitespace]\ncontext: --offset UTF16 [--before N --after N --unit utf16|blocks]\nops: --ops REQUEST.json --output NEW.docx [--report REPORT.json --preview REPORT.json --native-ops --overwrite]\npreview: --ops REQUEST.json [--report REPORT.json]\nsummary REPORT.json; media INPUT [--list | --id ID --output FILE]\n写入默认拒绝覆盖；只读预算单位为 UTF-16，maxBytes 计算完整 JSON 信封。"
        );
        return;
    }
    let json = args.iter().any(|a| a == "--json");
    let parsed = args::Args::parse(&args);
    let cap = parsed.as_ref().map(|a| a.budget.max_bytes).unwrap_or(512);
    let result = parsed.and_then(command::run);
    let (value, publication, exit) = match result {
        Ok((v, publication)) => (v, publication, 0),
        Err(e) => {
            let e = rsword_agent_query::tools::bounded_error(e, cap);
            let code =
                if matches!(e.code.as_str(), "AGENT_IO" | "AGENT_INTERNAL" | "AGENT_WORKER_FAILED")
                {
                    1
                } else {
                    2
                };
            (serde_json::to_value(e).unwrap(), None, code)
        }
    };
    // 只序列化一次，直接落在字节缓冲上（docs/21 WP1-2 D-2）：`to_string` 会在
    // `to_vec` 之后再做一趟 UTF-8 校验，而下游只需要字节。整块缓冲不能省——
    // 下面对 stdout 的写入要么整体成功要么整体失败，才能如实回滚已发布的文件。
    let mut text: Vec<u8> = Vec::new();
    if !json && let Some(content) = value.get("content") {
        match content.as_str() {
            Some(s) => text.extend_from_slice(s.as_bytes()),
            None => serde_json::to_writer(&mut text, content).unwrap(),
        }
        if let Some(c) = value["nextCursor"].as_str() {
            text.extend_from_slice(b"\nnextCursor: ");
            text.extend_from_slice(c.as_bytes());
        }
    } else {
        serde_json::to_writer(&mut text, &value).unwrap();
    }
    // Stdout 对 EBADF 特殊返回成功；直接写复制的描述符，才能如实报告回执丢失。
    #[cfg(unix)]
    let written = std::io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .and_then(|fd| File::from(fd).write_all(&text));
    // Windows 使用标准输出的控制台编码处理，同时保留管道/文件写入失败回滚。
    #[cfg(windows)]
    let written = {
        let mut stdout = std::io::stdout().lock();
        // 响应没有末尾换行；必须在提交文件前刷新行缓冲，才能捕获真实写入错误。
        // process::exit 不会替我们刷新标准输出。
        stdout.write_all(&text).and_then(|()| stdout.flush())
    };
    if written.is_err() {
        if let Some(publication) = publication
            && let Err(e) = publication.rollback()
        {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
    if let Some(publication) = publication {
        publication.commit();
    }
    std::process::exit(exit);
}
