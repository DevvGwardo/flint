//! Starts each real ACP adapter and opens a session without sending a
//! prompt (no model usage): proves the adapter launches, the handshake works
//! and the agent is logged in.
//!
//! cargo run -p flint-acp --example acp_probe -- [claude|codex]...

use std::time::Duration;
use std::time::Instant;

use flint_acp::AcpAgent;
use flint_acp::AcpConfig;
use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::Op;

fn main() {
    let wanted: Vec<String> = std::env::args().skip(1).collect();
    for (key, agent) in [("claude", AcpAgent::ClaudeCode), ("codex", AcpAgent::Codex)] {
        if !wanted.is_empty() && !wanted.iter().any(|w| w == key) {
            continue;
        }
        let workspace = tempfile_dir(key);
        let session_dir = workspace.join(".session");
        let started = Instant::now();
        let handle = flint_acp::spawn_acp_session(
            agent.clone(),
            AcpConfig {
                workspace: workspace.clone(),
                session_dir: Some(session_dir.clone()),
                approval: ApprovalMode::AskForChanges,
            },
        );
        let mut result = format!("{}: timed out after 150s", agent.name());
        while started.elapsed() < Duration::from_secs(150) {
            if let Ok(AgentEvent::Error(message)) = handle.events.try_recv() {
                result = format!("{}: ERROR {message}", agent.name());
                break;
            }
            if let Ok(text) = std::fs::read_to_string(session_dir.join("acp.json")) {
                let id = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v["session_id"].as_str().map(str::to_string))
                    .unwrap_or_default();
                result = format!(
                    "{}: session ready in {:.1}s (session id {}…)",
                    agent.name(),
                    started.elapsed().as_secs_f64(),
                    id.chars().take(8).collect::<String>()
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = handle.ops.send_blocking(Op::Shutdown);
        println!("{result}");
        let _ = std::fs::remove_dir_all(&workspace);
    }
}

fn tempfile_dir(key: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("flint-acp-probe-{key}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}
