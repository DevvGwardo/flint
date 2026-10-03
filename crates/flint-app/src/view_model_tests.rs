use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn started(call_id: &str, kind: ToolKind, summary: &str) -> AgentEvent {
    AgentEvent::ToolCallStarted {
        call_id: call_id.to_string(),
        name: "run_command".to_string(),
        kind,
        args: json!({}),
        summary: summary.to_string(),
    }
}

#[test]
fn approval_preview_uses_full_arguments_not_a_truncated_summary() {
    let mut view = SessionView::default();
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "c1".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({"command": "printf 'first\\nsecond\\n'", "timeout_secs": 10}),
            summary: "printf 'first".into(),
        },
        secs(0),
    );
    let preview = view.approval_preview("c1", std::path::Path::new("/tmp/work"), true);
    assert_eq!(
        preview.fields,
        vec![
            ("Command", "printf 'first\\nsecond\\n'".into()),
            ("Working directory", "/tmp/work".into())
        ]
    );
    let missing = view.approval_preview("unknown", std::path::Path::new("/tmp/work"), false);
    assert!(missing.fields.is_empty());

    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "e1".into(),
            name: "edit_file".into(),
            kind: ToolKind::Edit,
            args: json!({"path": "src/a.rs", "old_string": "before", "new_string": "after"}),
            summary: "src/a.rs".into(),
        },
        secs(1),
    );
    assert_eq!(
        view.approval_preview("e1", std::path::Path::new("/tmp/work"), true)
            .fields,
        vec![
            ("Affected file", "src/a.rs".into()),
            ("Existing text", "before".into()),
            ("Replacement text", "after".into())
        ]
    );
}

#[test]
fn acp_broad_approval_keeps_other_requests_pending() {
    let mut view = SessionView::default();
    for id in ["a", "b"] {
        view.fold(
            AgentEvent::ApprovalRequested {
                call_id: id.into(),
                kind: ToolKind::Command,
                summary: id.into(),
            },
            secs(0),
        );
    }
    view.resolve_approval_scoped("a", ApprovalDecision::ApproveAlways, false);
    assert_eq!(view.pending_approvals, 1);
    assert_eq!(view.pending_approval().unwrap().0, "b");
}

#[test]
fn streams_reasoning_then_text_into_separate_items() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 1,
        },
        secs(0),
    );
    view.fold(AgentEvent::ReasoningDelta("Look at ".into()), secs(1));
    view.fold(AgentEvent::ReasoningDelta("the tests.".into()), secs(2));
    let change = view.fold(AgentEvent::TextDelta("On it".into()), secs(4));
    view.fold(AgentEvent::TextDelta(".".into()), secs(5));

    assert_eq!(
        change,
        Change {
            appended: 1..2,
            updated: vec![0],
        }
    );
    assert_eq!(
        view.items,
        vec![
            Item::Thinking {
                text: "Look at the tests.".into(),
                started: secs(1),
                duration: Some(secs(3)),
                expanded: false,
            },
            Item::Assistant {
                text: "On it.".into(),
                streaming: true,
            },
        ]
    );
    assert!(view.running);
}

#[test]
fn tool_output_streams_then_finishes_and_failed_commands_open() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TextDelta("Running tests.".into()), secs(0));
    let change = view.fold(started("c1", ToolKind::Command, "npm test"), secs(1));
    assert_eq!(
        change,
        Change {
            appended: 1..2,
            updated: vec![0],
        }
    );
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "c1".into(),
            chunk: "FAIL ".into(),
        },
        secs(2),
    );
    let change = view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "c1".into(),
            output: "FAIL src/a.test.ts".into(),
            exit_code: Some(1),
            success: false,
            diff: None,
            duration_ms: 1200,
        },
        secs(3),
    );

    assert_eq!(change, Change::updated(1));
    assert_eq!(
        view.items[1],
        Item::Tool(Box::new(ToolCall {
            call_id: "c1".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            summary: "npm test".into(),
            args: json!({}),
            output: "FAIL src/a.test.ts".into(),
            terminal_id: None,
            subagent: None,
            started: secs(1),
            result: Some(ToolResult {
                exit_code: Some(1),
                success: false,
                diff: None,
                duration_ms: 1200,
            }),
            expanded: false,
        }))
    );
}

#[test]
fn edits_accumulate_in_changes() {
    let mut view = SessionView::default();
    let diff = |added, removed| FileDiff {
        path: "src/lru.ts".into(),
        unified: format!("+{added} -{removed}"),
        added,
        removed,
        created: false,
    };
    for (id, d) in [("e1", diff(3, 1)), ("e2", diff(2, 2))] {
        view.fold(started(id, ToolKind::Edit, "src/lru.ts"), secs(0));
        view.fold(
            AgentEvent::ToolCallFinished {
                call_id: id.into(),
                output: "ok".into(),
                exit_code: None,
                success: true,
                diff: Some(d),
                duration_ms: 5,
            },
            secs(1),
        );
    }

    assert_eq!(
        view.changes,
        vec![ChangedFile {
            path: "src/lru.ts".into(),
            added: 5,
            removed: 3,
            created: false,
            diffs: vec!["+3 -1".into(), "+2 -2".into()],
            combined: None,
        }]
    );
}

#[test]
fn a_new_edit_invalidates_the_previous_combined_diff() {
    let mut view = SessionView::default();
    for n in 0..2 {
        let id = format!("e{n}");
        view.fold(started(&id, ToolKind::Edit, "a.txt"), secs(0));
        view.fold(
            AgentEvent::ToolCallFinished {
                call_id: id,
                output: "ok".into(),
                exit_code: None,
                success: true,
                diff: Some(FileDiff {
                    path: "a.txt".into(),
                    unified: format!("+edit {n}"),
                    added: 1,
                    removed: 0,
                    created: false,
                }),
                duration_ms: 5,
            },
            secs(1),
        );
        assert_eq!(view.changes[0].combined, None);
        let revision = view.changes_revision;
        view.set_combined("a.txt", format!("+settled {n}"), n + 1, 0);
        assert_ne!(view.changes_revision, revision);
    }
    assert_eq!(view.changes[0].diffs, ["+edit 0", "+edit 1"]);
}

#[test]
fn turn_finish_closes_streams_and_summarizes() {
    let mut view = SessionView::default();
    view.push_user("Fix the LRU eviction bug\nmore detail".into());
    view.fold(AgentEvent::TurnStarted { turn_id: 7 }, secs(10));
    view.fold(
        AgentEvent::StepStarted {
            turn_id: 7,
            step: 3,
        },
        secs(11),
    );
    view.fold(AgentEvent::TextDelta("Done.".into()), secs(12));
    let usage = Usage {
        input_tokens: 1000,
        cached_input_tokens: 800,
        output_tokens: 50,
        reasoning_tokens: 10,
    };
    view.fold(AgentEvent::Usage(usage), secs(12));
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 7,
            reason: TurnEndReason::Completed,
        },
        secs(52),
    );

    assert_eq!(view.title.as_deref(), Some("Fix the LRU eviction bug"));
    assert_eq!(
        view.items[1..],
        [
            Item::Assistant {
                text: "Done.".into(),
                streaming: false,
            },
            Item::TurnSummary {
                reason: TurnEndReason::Completed,
                duration: secs(42),
                steps: 3,
                usage,
            },
        ]
    );
    assert!(!view.running);
    assert_eq!(view.session_usage, usage);
    assert_eq!(view.elapsed(secs(100)), Some(secs(42)));
}

#[test]
fn approvals_and_nudges() {
    let mut view = SessionView::default();
    view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "c9".into(),
            kind: ToolKind::Command,
            summary: "rm -rf build".into(),
        },
        secs(0),
    );
    assert_eq!(view.pending_approvals, 1);
    assert_eq!(
        view.resolve_approval("c9", ApprovalDecision::Deny),
        Change::updated(0)
    );
    assert_eq!(view.pending_approvals, 0);
    assert_eq!(
        view.resolve_approval("c9", ApprovalDecision::Approve),
        Change::default()
    );

    view.fold(
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            message: "run the tests".into(),
        },
        secs(1),
    );
    assert_eq!(
        view.items,
        vec![
            Item::Approval {
                call_id: "c9".into(),
                kind: ToolKind::Command,
                summary: "rm -rf build".into(),
                decision: Some(ApprovalDecision::Deny),
            },
            Item::Nudge {
                reason: NudgeReason::Verify,
                message: "run the tests".into(),
                expanded: false,
            },
        ]
    );
}

#[test]
fn toggles_only_expandable_items() {
    let mut view = SessionView::default();
    view.push_user("hi".into());
    view.fold(AgentEvent::ReasoningDelta("hmm".into()), secs(0));
    assert_eq!(view.toggle_expanded(0), Change::default());
    assert_eq!(view.toggle_expanded(1), Change::updated(1));
    assert!(matches!(
        view.items[1],
        Item::Thinking { expanded: true, .. }
    ));
}

#[test]
fn titles_are_trimmed() {
    assert_eq!(title_from("\n  hello world  \n"), "hello world");
    assert_eq!(title_from(""), "New session");
    let long = "a".repeat(60);
    assert_eq!(title_from(&long), format!("{}…", "a".repeat(48)));
}

#[test]
fn live_output_keeps_only_the_tail() {
    let mut view = SessionView::default();
    view.fold(started("c1", ToolKind::Command, "yes"), secs(0));
    for n in 0..MAX_LIVE_OUTPUT_LINES + 50 {
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "c1".into(),
                chunk: format!("line {n}\n"),
            },
            secs(1),
        );
    }
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(call.output.lines().count(), MAX_LIVE_OUTPUT_LINES);
    assert_eq!(call.output.lines().next(), Some("line 50"));
}

#[test]
fn a_terminals_call_gets_the_terminals_id() {
    let mut view = SessionView::default();
    view.fold(started("c1", ToolKind::Command, "npm test"), secs(0));
    let change = view.fold(
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
        secs(1),
    );
    assert_eq!(change, Change::updated(0));
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(call.terminal_id.as_deref(), Some("t1"));
    // A terminal with no call (an agent's own shell) touches nothing.
    assert_eq!(
        view.fold(
            AgentEvent::TerminalStarted {
                terminal_id: "t2".into(),
                call_id: None,
                label: "claude: zsh".into(),
                cwd: None,
            },
            secs(2),
        ),
        Change::default()
    );
}

#[test]
fn child_events_stay_nested_but_edits_contribute_to_parent_changes() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(started("delegate", ToolKind::Other, "Research"), secs(1));
    view.fold(
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "agent-1".into(),
            model: "child-model".into(),
        },
        secs(1),
    );
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 3,
        },
        AgentEvent::TextDelta("Child answer.".into()),
        started("agent-1:edit", ToolKind::Edit, "a.txt"),
        AgentEvent::ToolCallFinished {
            call_id: "agent-1:edit".into(),
            output: "Created.".into(),
            exit_code: None,
            success: true,
            duration_ms: 2,
            diff: Some(FileDiff {
                path: "a.txt".into(),
                unified: "+hi".into(),
                added: 1,
                removed: 0,
                created: true,
            }),
        },
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        assert_eq!(
            view.fold(
                AgentEvent::SubagentEvent {
                    call_id: "delegate".into(),
                    event: Box::new(event),
                },
                secs(2)
            ),
            Change::updated(0)
        );
    }
    assert!(view.running);
    assert_eq!(view.step, 0);
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.changes[0].path, "a.txt");
    assert_eq!(view.turns[0].files, vec!["a.txt"]);
    let Item::Tool(call) = &view.items[0] else {
        panic!("tool")
    };
    let child = call.subagent.as_ref().expect("child");
    assert_eq!(child.model, "child-model");
    assert!(!child.view.running);
    assert!(child.view.items.iter().any(|item| matches!(item,
        Item::Assistant { text, .. } if text == "Child answer."
    )));
}

#[test]
fn approve_always_resolves_parallel_child_approvals_and_interrupt_expires_them() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    for call_id in ["agent-1:edit", "agent-2:edit"] {
        view.fold(
            AgentEvent::ApprovalRequested {
                call_id: call_id.into(),
                kind: ToolKind::Edit,
                summary: "a.txt".into(),
            },
            secs(1),
        );
    }
    assert_eq!(view.pending_approvals, 2);
    let change = view.resolve_approval("agent-1:edit", ApprovalDecision::ApproveAlways);
    assert_eq!(change.updated, vec![0, 1]);
    assert_eq!(view.pending_approvals, 0);
    assert!(view.pending_approval().is_none());
    view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "agent-3:edit".into(),
            kind: ToolKind::Edit,
            summary: "a.txt".into(),
        },
        secs(2),
    );
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
        secs(3),
    );
    assert_eq!(view.pending_approvals, 0);
    assert!(view.pending_approval().is_none());
}
