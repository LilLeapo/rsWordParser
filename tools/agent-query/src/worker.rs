//! AGENT-04 内部工作进程；不是对 Agent 公开的新工具名。
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let request_path = std::path::Path::new(&args[1]);
    std::fs::write(&args[3], b"ready").expect("ready");
    while !request_path.exists() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let request = std::fs::read(request_path).expect("request");
    let request: rsword_agent_query::search::Request =
        serde_json::from_slice(&request).expect("typed request");
    let result = rsword_agent_query::search::run(&request);
    std::fs::write(&args[2], serde_json::to_vec(&result).expect("result JSON")).expect("response");
}
