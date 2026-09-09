//! AGENT-04 内部工作进程；不是对 Agent 公开的新工具名。
fn main() {
    std::process::exit(rsword_agent_query::search::worker_entry(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
