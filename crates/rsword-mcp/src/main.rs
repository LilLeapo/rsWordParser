//! AGENT-10：原生 Rust MCP stdio 入口；查询 worker 内嵌于同一二进制。
use rsword_agent_query::{edit::WorkerConfig, search, transport::Shape};
use rsword_mcp::{
    Config, Service,
    protocol::{self, Protocol},
};
use std::{io, os::fd::AsFd, time::Duration};
fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut shape = Shape::Text;
    let mut max_sessions = 32;
    let mut idle_timeout = Duration::from_secs(1800);
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if flag == "--help" {
            eprintln!(
                "rsword-mcp [--result-shape text|structured] [--max-sessions 1..32] [--idle-timeout-ms 1..1800000]\nstdio JSON-RPC. 缺省 text；structured 结果需客户端读取 structuredContent，缺省待门 5 实测确认。"
            );
            return Ok(());
        }
        let value = args.next().ok_or("missing option value")?;
        match flag.as_str() {
            "--result-shape" => {
                shape = match value.as_str() {
                    "text" => Shape::Text,
                    "structured" => Shape::Structured,
                    _ => return Err("unknown result shape".into()),
                }
            }
            "--max-sessions" => max_sessions = value.parse()?,
            "--idle-timeout-ms" => idle_timeout = Duration::from_millis(value.parse()?),
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    let worker = WorkerConfig {
        program: std::env::current_exe()?,
        scratch: std::env::temp_dir().join("rsword-query"),
        args: vec!["__query-worker".into()],
    };
    let service = Service::new(Config { shape, max_sessions, idle_timeout, worker })?;
    // 与 CLI 相同：直接使用真实 fd，不能让 Stdout 的 EBADF 吞错留下已发布文件。
    let stdout = std::fs::File::from(io::stdout().as_fd().try_clone_to_owned()?);
    protocol::serve(Protocol::new(service), stdout)?;
    Ok(())
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "__query-worker") {
        std::process::exit(search::worker_entry(&args[1..]));
    }
    if let Err(e) = run(&args) {
        eprintln!("rsword-mcp: {e}");
        std::process::exit(1);
    }
}
