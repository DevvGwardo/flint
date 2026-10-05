use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn failed_edits_do_not_claim_mutation_but_keep_call_accounting() {
    let mut guard = TurnGuard::new("what happened?");
    let args = json!({"path":"a.rs"});
    guard.record_tool_call("edit_file", ToolKind::Edit, &args, Some("a.rs"));
    guard.record_tool_result("edit_file", ToolKind::Edit, &args, "declined", None, false);
    assert!(!guard.edited_files);
    assert!(guard.edited_paths.is_empty());
    assert_eq!(guard.tool_calls, 1);
}

#[test]
fn read_range_saturates_at_u64_max() {
    let mut guard = TurnGuard::new("read");
    guard.record_tool_call(
        "read_file",
        ToolKind::Read,
        &json!({"path":"a", "offset":u64::MAX, "limit":u64::MAX}),
        None,
    );
    assert_eq!(guard.read_ranges["a"], vec![(u64::MAX, u64::MAX)]);
}

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
    guard.record_tool_result(
        "edit_file",
        ToolKind::Edit,
        &json!({"path":"a.rs"}),
        "edited",
        None,
        true,
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
    guard.record_tool_result(
        "edit_file",
        ToolKind::Edit,
        &json!({"path":"a.rs"}),
        "edited",
        None,
        true,
    );
    let cmd = json!({"command": "cargo test"});
    guard.record_tool_call("run_command", ToolKind::Command, &cmd, None);
    guard.record_tool_result("run_command", ToolKind::Command, &cmd, "ok", Some(0), true);
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
    guard.record_tool_result(
        "edit_file",
        ToolKind::Edit,
        &json!({"path":"a.rs"}),
        "edited",
        None,
        true,
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

fn edit(guard: &mut TurnGuard) {
    guard.record_tool_call(
        "edit_file",
        ToolKind::Edit,
        &json!({"path": "a.rs"}),
        Some("a.rs"),
    );
    guard.record_tool_result(
        "edit_file",
        ToolKind::Edit,
        &json!({"path":"a.rs"}),
        "edited",
        None,
        true,
    );
}

fn command(guard: &mut TurnGuard, cmd: &str, output: &str, exit_code: i32) {
    let args = json!({"command": cmd});
    guard.record_tool_call("run_command", ToolKind::Command, &args, None);
    guard.record_tool_result(
        "run_command",
        ToolKind::Command,
        &args,
        output,
        Some(exit_code),
        exit_code == 0,
    );
}

#[test]
fn inspection_after_an_edit_is_not_verification() {
    let mut guard = TurnGuard::new("fix the bug in a.rs");
    edit(&mut guard);
    command(&mut guard, "ls && cat a.rs | head -5", "ok", 0);
    command(&mut guard, "git diff --stat 2>&1", "1 file", 0);
    assert_eq!(
        guard.before_finish(),
        Some((NudgeReason::Verify, VERIFY_NUDGE.to_string()))
    );
}

#[test]
fn a_failed_check_after_the_edits_gets_one_nudge() {
    let mut guard = TurnGuard::new("fix the bug in a.rs");
    edit(&mut guard);
    command(&mut guard, "cargo test", "1 failed", 101);
    assert_eq!(
        guard.before_finish(),
        Some((
            NudgeReason::Verify,
            failed_check_nudge("cargo test", Some(101))
        ))
    );
    assert_eq!(guard.before_finish(), None);

    // A later passing check clears it.
    let mut guard = TurnGuard::new("fix the bug in a.rs");
    edit(&mut guard);
    command(&mut guard, "cargo test", "1 failed", 101);
    command(&mut guard, "cd crates && cargo test -p x", "ok", 0);
    assert_eq!(guard.before_finish(), None);
}

#[test]
fn inspection_programs_and_real_checks() {
    for cmd in [
        "ls -la",
        "cat a | grep x",
        "git status",
        "FOO=1 head a",
        "cd x; pwd",
        "ls 2>&1",
    ] {
        assert!(is_inspection(cmd), "{cmd}");
    }
    for cmd in [
        "cargo test",
        "cd x && npm test",
        "cat a | python -m json.tool",
        "git commit -m x",
        "env FOO=1 cargo test",
        "test -f a.txt",
        "diff out expected",
    ] {
        assert!(!is_inspection(cmd), "{cmd}");
    }
}

#[test]
fn overlapping_rereads_of_one_file_are_stuck() {
    let mut guard = TurnGuard::new("fix the build");
    let read = |guard: &mut TurnGuard, offset: u64| {
        guard.record_tool_call(
            "read_file",
            ToolKind::Read,
            &json!({"path": "./big.rs", "offset": offset, "limit": 100}),
            None,
        );
    };
    // Paging through a file is not a loop.
    for offset in [1, 101, 201, 301] {
        read(&mut guard, offset);
    }
    assert_eq!(guard.take_stuck_nudge(), None);
    assert_eq!(guard.take_stuck_suspect(), None);
    read(&mut guard, 50);
    read(&mut guard, 150);
    assert_eq!(
        guard.take_stuck_suspect(),
        Some("read_file big.rs".to_string())
    );
    read(&mut guard, 250);
    assert_eq!(
        guard.take_stuck_nudge(),
        Some(stuck_nudge("read_file big.rs"))
    );

    // Editing the file resets its count: reading it back is expected.
    let mut guard = TurnGuard::new("fix the build");
    for _ in 0..2 {
        read(&mut guard, 1);
        guard.record_tool_call(
            "edit_file",
            ToolKind::Edit,
            &json!({"path": "big.rs"}),
            Some("big.rs"),
        );
        guard.record_tool_result(
            "edit_file",
            ToolKind::Edit,
            &json!({"path":"big.rs"}),
            "edited",
            None,
            true,
        );
    }
    read(&mut guard, 1);
    assert_eq!(guard.take_stuck_nudge(), None);
}

#[test]
fn different_commands_failing_the_same_way_are_stuck() {
    let mut guard = TurnGuard::new("fix the build");
    command(&mut guard, "cargo test", "error[E0425]: x", 101);
    assert_eq!(guard.take_stuck_nudge(), None);
    command(&mut guard, "cargo test --all", "error[E0425]: x", 101);
    assert_eq!(
        guard.take_stuck_nudge(),
        Some(stuck_nudge("cargo test --all"))
    );
    command(&mut guard, "cargo check", "error[E0599]: y", 101);
    assert_eq!(guard.take_stuck_nudge(), None);
}

#[test]
fn open_plan_steps_get_one_nudge() {
    let mut guard = TurnGuard::new("how does the router work?");
    guard.record_plan(vec!["Trace the request path".to_string()]);
    assert_eq!(
        guard.before_finish(),
        Some((
            NudgeReason::Watchdog,
            open_plan_nudge(&["Trace the request path".to_string()])
        ))
    );
    assert_eq!(guard.before_finish(), None);
    let mut done = TurnGuard::new("how does the router work?");
    done.record_plan(Vec::new());
    assert_eq!(done.before_finish(), None);
}

#[test]
fn bug_reports_count_as_change_requests() {
    assert!(task_asks_for_changes("the login button is broken"));
    assert!(task_asks_for_changes("tests are failing on main"));
    assert!(!task_asks_for_changes("why is the login button broken?"));
}
