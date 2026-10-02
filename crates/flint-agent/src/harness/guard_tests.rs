use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn identical_calls_raise_a_suspect_then_a_nudge() {
    let mut guard = TurnGuard::new("fix the build");
    let args = json!({"path": "a.rs"});
    guard.record_tool_call("read_file", ToolKind::Read, &args, None);
    assert_eq!(guard.take_stuck_suspect(), None);
    guard.record_tool_call("read_file", ToolKind::Read, &args, None);
    assert_eq!(
        guard.take_stuck_suspect(),
        Some(r#"read_file({"path":"a.rs"})"#.to_string())
    );
    assert_eq!(guard.take_stuck_nudge(), None);
    guard.record_tool_call("read_file", ToolKind::Read, &args, None);
    assert_eq!(
        guard.take_stuck_nudge(),
        Some(stuck_nudge(r#"read_file({"path":"a.rs"})"#))
    );
}

#[test]
fn repeated_identical_failures_are_stuck() {
    let mut guard = TurnGuard::new("fix the build");
    let args = json!({"command": "cargo test"});
    for _ in 0..2 {
        guard.record_tool_call("run_command", ToolKind::Command, &args, None);
        guard.record_tool_result(
            "run_command",
            ToolKind::Command,
            &args,
            "1 failed",
            Some(101),
            false,
        );
    }
    assert_eq!(guard.take_stuck_nudge(), Some(stuck_nudge("cargo test")));
}

#[test]
fn a_command_that_succeeds_may_be_rerun() {
    let mut guard = TurnGuard::new("fix the build");
    let args = json!({"command": "cargo test"});
    for _ in 0..3 {
        guard.record_tool_call("run_command", ToolKind::Command, &args, None);
        guard.record_tool_result("run_command", ToolKind::Command, &args, "ok", Some(0), true);
    }
    assert_eq!(guard.take_stuck_nudge(), None);
}

#[test]
fn verify_nudge_once_after_unverified_edit() {
    let mut guard = TurnGuard::new("fix the bug in a.rs");
    guard.record_tool_call(
        "edit_file",
        ToolKind::Edit,
        &json!({"path": "a.rs"}),
        Some("a.rs"),
    );
    assert_eq!(
        guard.before_finish(),
        Some((NudgeReason::Verify, VERIFY_NUDGE.to_string()))
    );
    assert_eq!(guard.before_finish(), None);
}

#[test]
fn verified_edit_finishes() {
    let mut guard = TurnGuard::new("fix the bug in a.rs");
    guard.record_tool_call(
        "edit_file",
        ToolKind::Edit,
        &json!({"path": "a.rs"}),
        Some("a.rs"),
    );
    guard.record_tool_call(
        "run_command",
        ToolKind::Command,
        &json!({"command": "cargo test"}),
        None,
    );
    assert_eq!(guard.before_finish(), None);
}

#[test]
fn watchdog_only_for_change_requests() {
    let mut guard = TurnGuard::new("Add a --json flag to the CLI");
    guard.record_tool_call(
        "read_file",
        ToolKind::Read,
        &json!({"path": "cli.rs"}),
        None,
    );
    assert_eq!(
        guard.before_finish(),
        Some((NudgeReason::Watchdog, WATCHDOG_NUDGE.to_string()))
    );
    assert_eq!(guard.before_finish(), None);

    let mut question = TurnGuard::new("how does the router work?");
    question.record_tool_call("read_file", ToolKind::Read, &json!({"path": "r.rs"}), None);
    assert_eq!(question.before_finish(), None);
}

#[test]
fn judge_states_are_capped_and_shaped() {
    let mut guard = TurnGuard::new("fix it");
    guard.record_tool_call(
        "edit_file",
        ToolKind::Edit,
        &json!({"path": "a.rs"}),
        Some("a.rs"),
    );
    let cmd = json!({"command": "ls"});
    guard.record_tool_call("run_command", ToolKind::Command, &cmd, None);
    guard.record_tool_result(
        "run_command",
        ToolKind::Command,
        &cmd,
        "a.rs\n[exit code: 0]",
        Some(0),
        true,
    );
    assert_eq!(
        guard.verify_state("Done."),
        json!({
            "files_edited": ["a.rs"],
            "commands_after_last_edit": [{"command": "ls", "exit_code": 0}],
            "final_message": "Done.",
        })
    );
    for i in 0..20 {
        guard.record_tool_call(
            "read_file",
            ToolKind::Read,
            &json!({"path": format!("f{i}")}),
            None,
        );
    }
    assert_eq!(
        guard.stall_state()["recent_tool_calls"]
            .as_array()
            .map(Vec::len),
        Some(8)
    );
}
