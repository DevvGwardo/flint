//! Live end-to-end test against a real OpenAI-compatible endpoint. Runs only
//! with FLINT_LIVE=1; reads FLINT_API_KEY, FLINT_LIVE_BASE_URL and
//! FLINT_LIVE_MODEL (the last two default to the library defaults).

use std::time::Duration;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::Op;
use flint_agent::TurnEndReason;

#[test]
fn live_creates_and_tests_add_py() {
    if std::env::var("FLINT_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set FLINT_LIVE=1 to run against a live endpoint");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = AgentConfig::from_env(dir.path().to_path_buf()).expect("live config");
    if let Ok(base_url) = std::env::var("FLINT_LIVE_BASE_URL") {
        config.base_url = base_url;
    }
    if let Ok(model) = std::env::var("FLINT_LIVE_MODEL") {
        config.model = model;
    }
    let handle = flint_agent::spawn_session(config);
    handle
        .ops
        .send_blocking(Op::UserMessage(
            "Create add.py with a function add(a, b) that returns their sum, and test_add.py with a \
             unittest test for it. Run the tests with python3 -m unittest and make sure they pass."
                .to_string(),
        ))
        .expect("send");
    let mut finished = None;
    let mut ran_command_ok = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    while std::time::Instant::now() < deadline {
        let Ok(event) = handle.events.recv_blocking() else {
            break;
        };
        match &event {
            AgentEvent::ToolCallStarted { name, summary, .. } => {
                eprintln!("call {name}: {summary}")
            }
            AgentEvent::ToolCallFinished {
                success,
                exit_code,
                output,
                ..
            } => {
                eprintln!(
                    "  -> success={success} exit={exit_code:?} {}",
                    output.lines().last().unwrap_or_default()
                );
                if *exit_code == Some(0) {
                    ran_command_ok = true;
                }
            }
            AgentEvent::HarnessNudge { reason, .. } => eprintln!("nudge {reason:?}"),
            AgentEvent::ToolRepaired { tool, detail } => eprintln!("repaired {tool}: {detail}"),
            AgentEvent::Usage(usage) => eprintln!("usage {usage:?}"),
            AgentEvent::Error(message) => eprintln!("error {message}"),
            AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            } => {
                eprintln!("compacted {before_tokens} -> {after_tokens}");
            }
            AgentEvent::SessionOptions(_) => {}
            AgentEvent::TerminalStarted { .. }
            | AgentEvent::TerminalOutput { .. }
            | AgentEvent::TerminalExited { .. } => {}
            AgentEvent::TurnFinished { reason, .. } => {
                eprintln!("finished {reason:?}");
                finished = Some(reason.clone());
                break;
            }
            AgentEvent::TurnStarted { .. }
            | AgentEvent::StepStarted { .. }
            | AgentEvent::ReasoningDelta(_)
            | AgentEvent::TextDelta(_)
            | AgentEvent::ToolOutputDelta { .. }
            | AgentEvent::ApprovalRequested { .. } => {}
        }
    }
    let _ = handle.ops.send_blocking(Op::Shutdown);
    assert_eq!(finished, Some(TurnEndReason::Completed));
    assert!(dir.path().join("add.py").exists(), "add.py was not created");
    assert!(ran_command_ok, "no command succeeded");
    let check = std::process::Command::new("python3")
        .args(["-m", "unittest"])
        .current_dir(dir.path())
        .output()
        .expect("python3");
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}
