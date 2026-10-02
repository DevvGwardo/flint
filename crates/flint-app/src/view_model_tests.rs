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
