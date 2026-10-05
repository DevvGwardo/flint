use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn task_fixture(commands: usize, finished: usize) -> SessionView {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    for ix in 0..commands + finished {
        let call_id = format!("command-{ix}");
        view.fold(
            AgentEvent::ToolCallStarted {
                call_id: call_id.clone(),
                name: "run_command".into(),
                kind: ToolKind::Command,
                args: json!({}),
                summary: format!("task {ix} 界🙂"),
            },
            secs(1),
        );
        if ix < finished {
            view.fold(
                AgentEvent::ToolCallFinished {
                    call_id,
                    output: "completed".into(),
                    exit_code: Some(0),
                    success: true,
                    diff: None,
                    duration_ms: 1,
                },
                secs(2),
            );
        }
    }
    view
}

fn running_command_reference(view: &SessionView) -> Vec<&ToolCall> {
    let Some(turn) = view.current_turn.and_then(|ix| view.turns.get(ix)) else {
        return Vec::new();
    };
    view.items[turn.first..]
        .iter()
        .filter_map(|item| match item {
            Item::Tool(call) if call.kind == ToolKind::Command && call.result.is_none() => {
                Some(call.as_ref())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn running_command_projection_matches_reference_and_borrows_original_calls() {
    for commands in [0, 1, 8, 9, 64] {
        for completed in [0, 1, 16] {
            let mut view = task_fixture(commands, completed);
            view.fold(
                AgentEvent::ToolCallStarted {
                    call_id: "read".into(),
                    name: "read_file".into(),
                    kind: ToolKind::Read,
                    args: json!({}),
                    summary: "not a command".into(),
                },
                secs(3),
            );
            let expected = running_command_reference(&view);
            let (count, projected) = view.running_command_projection();
            let projected: Vec<_> = projected.collect();
            assert_eq!(count, commands);
            assert_eq!(projected.len(), expected.len());
            for (actual, reference) in projected.into_iter().zip(expected) {
                assert!(std::ptr::eq(actual, reference));
                assert_eq!(actual.summary, reference.summary);
            }
            assert_eq!(view.running_commands(), running_command_reference(&view));
        }
    }
}

#[test]
fn running_command_projection_preserves_turn_boundaries_and_invalid_turn_behavior() {
    let mut view = task_fixture(3, 0);
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
        secs(4),
    );
    assert_eq!(view.running_command_projection().0, 0);
    assert_eq!(view.running_command_projection().1.count(), 0);
    view.fold(AgentEvent::TurnStarted { turn_id: 2 }, secs(5));
    assert_eq!(view.running_command_projection().0, 0);
    view.current_turn = Some(usize::MAX);
    assert!(view.running_command_projection().1.next().is_none());
    assert!(view.running_commands().is_empty());
    view.current_turn = None;
    assert!(view.running_command_projection().1.next().is_none());
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn running_command_projection_perf_probe() {
    for (case, live, completed) in [
        ("one", 1, 0),
        ("eight", 8, 0),
        ("many", 512, 0),
        ("mixed", 8, 2048),
        ("completed", 0, 2048),
    ] {
        let view = task_fixture(live, completed);
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let (projected_count, commands) =
                std::hint::black_box(&view).running_command_projection();
            let mut count = 0;
            let mut bytes = 0;
            for call in commands {
                count += 1;
                bytes += std::hint::black_box(call).call_id.len();
            }
            assert_eq!(std::hint::black_box(count), live);
            assert_eq!(std::hint::black_box(projected_count), live);
            std::hint::black_box(bytes);
        }
        eprintln!(
            "running_command_projection_{case}_1000_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        eprintln!(
            "running_command_public_vec_{case}_capacity_bytes={}",
            view.running_commands().capacity() * std::mem::size_of::<&ToolCall>()
        );
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn pending_approval_presence_perf_probe() {
    for (case, summary) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("short", "fixture approval".into()),
    ] {
        let mut view = SessionView::default();
        view.fold(
            AgentEvent::ApprovalRequested {
                call_id: "fixture-request".into(),
                kind: ToolKind::Command,
                summary,
            },
            Duration::ZERO,
        );
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(std::hint::black_box(&view).has_pending_approval());
        }
        eprintln!(
            "pending_approval_presence_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        assert!(view.has_pending_approval());
    }
}

#[test]
fn pending_approval_borrows_original_fields_and_keeps_owned_api_independent() {
    let mut view = SessionView::default();
    let summary = format!("{}\nORIGINAL_BODY", "界🙂".repeat(4096));
    view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "original 界".into(),
            kind: ToolKind::Command,
            summary: summary.clone(),
        },
        Duration::ZERO,
    );
    let Item::Approval {
        call_id: stored_id,
        summary: stored_summary,
        ..
    } = &view.items[0]
    else {
        panic!("expected approval");
    };
    let (id, kind, borrowed_summary) = view.pending_approval_ref().unwrap();
    assert!(std::ptr::eq(id, stored_id.as_str()));
    assert!(std::ptr::eq(borrowed_summary, stored_summary.as_str()));
    assert_eq!(kind, ToolKind::Command);
    assert!(view.has_pending_approval());
    let (mut owned_id, owned_kind, mut owned_summary) = view.pending_approval().unwrap();
    assert_eq!(
        (owned_id.as_str(), owned_kind, owned_summary.as_str()),
        ("original 界", ToolKind::Command, summary.as_str())
    );
    owned_id.clear();
    owned_summary.clear();
    assert_eq!(
        view.pending_approval_ref().unwrap(),
        ("original 界", ToolKind::Command, summary.as_str())
    );
}

#[test]
fn pending_approval_projection_preserves_oldest_order_and_ignores_counter_and_answered_rows() {
    let mut view = SessionView {
        pending_approvals: 99,
        ..SessionView::default()
    };
    assert!(!view.has_pending_approval());
    assert_eq!(view.pending_approval_ref(), None);
    view.items.push(Item::User("not a request".into()));
    for (id, kind, summary, decision) in [
        (
            "answered",
            ToolKind::Command,
            "ignore",
            Some(ApprovalDecision::Deny),
        ),
        ("first", ToolKind::Edit, "", None),
        ("second", ToolKind::Read, "full second", None),
    ] {
        view.items.push(Item::Approval {
            call_id: id.into(),
            kind,
            summary: summary.into(),
            decision,
        });
    }
    view.pending_approvals = 0;
    assert_eq!(
        view.pending_approval_ref(),
        Some(("first", ToolKind::Edit, ""))
    );
    assert!(view.has_pending_approval());
    view.resolve_approval("first", ApprovalDecision::Approve);
    assert_eq!(
        view.pending_approval_ref(),
        Some(("second", ToolKind::Read, "full second"))
    );
    view.resolve_approval("second", ApprovalDecision::Deny);
    assert_eq!(view.pending_approval_ref(), None);
    assert_eq!(view.pending_approval(), None);
    assert!(!view.has_pending_approval());
}

/// A finished turn: thinking, an edit, an intermediate message, a nudge,
/// and a final answer.
fn finished_turn() -> SessionView {
    let mut view = SessionView::default();
    view.push_user("fix it".into());
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(AgentEvent::ReasoningDelta("hmm".into()), secs(1));
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "e1".into(),
            name: "edit_file".into(),
            kind: ToolKind::Edit,
            args: json!({}),
            summary: "a.ts".into(),
        },
        secs(2),
    );
    view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "e1".into(),
            output: "ok".into(),
            exit_code: None,
            success: true,
            diff: Some(FileDiff {
                path: "a.ts".into(),
                unified: String::new(),
                added: 4,
                removed: 1,
                created: false,
            }),
            duration_ms: 3,
        },
        secs(3),
    );
    view.fold(AgentEvent::TextDelta("Edited.".into()), secs(4));
    view.fold(
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            message: "verify".into(),
        },
        secs(5),
    );
    view.fold(AgentEvent::TextDelta("All done.".into()), secs(6));
    view
}

#[test]
fn running_turn_rows_are_live() {
    let view = finished_turn();
    assert_eq!(
        (0..view.items.len())
            .map(|ix| view.role(ix))
            .collect::<Vec<_>>(),
        vec![
            Role::Plain,
            Role::Live,
            Role::Live,
            Role::Live,
            Role::Live,
            Role::Live
        ]
    );
    assert_eq!(view.activity(), Activity::Writing);
}

#[test]
fn finishing_collapses_work_and_promotes_the_answer() {
    let mut view = finished_turn();
    let change = view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
        secs(72),
    );

    assert_eq!(change.appended, 6..7);
    assert_eq!(
        view.turns,
        vec![TurnInfo {
            first: 1,
            end: Some(6),
            final_answer: Some(5),
            header: Some(1),
            expanded: false,
            nudges: 1,
            duration: secs(72),
            files: vec!["a.ts".into()],
            added: 4,
            removed: 1,
            file_stats: vec![],
            feedback: None,
        }]
    );
    assert_eq!(
        (0..view.items.len())
            .map(|ix| view.role(ix))
            .collect::<Vec<_>>(),
        vec![
            Role::Plain,
            Role::WorkHeader,
            Role::Hidden,
            Role::Hidden,
            Role::Hidden,
            Role::Answer,
            Role::Summary,
        ]
    );

    assert_eq!(view.toggle_work(3).updated, vec![1, 2, 3, 4, 5]);
    assert_eq!(view.role(2), Role::Work);
    assert_eq!(view.set_feedback(6, true), Change::updated(6));
    assert_eq!(view.turns[0].feedback, Some(true));
    view.set_feedback(6, true);
    assert_eq!(view.turns[0].feedback, None);
}

#[test]
fn running_commands_feed_the_task_tray() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "c1".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({}),
            summary: "npm test".into(),
        },
        secs(1),
    );
    assert_eq!(
        view.running_commands()
            .iter()
            .map(|c| c.summary.as_str())
            .collect::<Vec<_>>(),
        vec!["npm test"]
    );
    assert_eq!(view.activity(), Activity::Running("npm test".into()));
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn terminal_forwarding_perf_probe() {
    let text = "terminal output\n".repeat(1_000_000);
    let start = std::time::Instant::now();
    for _ in 0..20 {
        std::hint::black_box(terminal_message("probe", &text));
    }
    eprintln!(
        "terminal_forwarding_20_runs_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
}

#[test]
fn terminal_forwarding_matches_the_last_lines_for_lf_crlf_and_unicode() {
    for ending in ["\n", "\r\n"] {
        for trailing in [false, true] {
            let text = (0..220)
                .map(|n| format!("line {n} 界🙂"))
                .collect::<Vec<_>>()
                .join(ending)
                + if trailing { ending } else { "" };
            let (_, message) = terminal_message("shell", &text);
            assert!(!message.contains("line 19 "));
            assert!(message.contains("line 20 界🙂"));
            assert!(message.contains("line 219 界🙂"));
            assert!(!message.contains('\r'));
        }
    }
    assert_eq!(
        terminal_message("shell", "").1,
        "Here's what my terminal (shell) shows:\n\n```\n\n```"
    );
}

#[test]
fn interrupted_commands_do_not_leak_into_the_next_turns_task_tray() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "old".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({}),
            summary: "old command".into(),
        },
        secs(1),
    );
    assert_eq!(view.running_commands().len(), 1);
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
        secs(2),
    );
    assert!(view.running_commands().is_empty());
    view.fold(AgentEvent::TurnStarted { turn_id: 2 }, secs(3));
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "new".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({}),
            summary: "new command".into(),
        },
        secs(4),
    );
    assert_eq!(
        view.running_commands()
            .iter()
            .map(|call| call.call_id.as_str())
            .collect::<Vec<_>>(),
        ["new"]
    );
}

#[test]
fn messages_sent_mid_turn_and_undo_results_stay_visible() {
    let mut view = SessionView::default();
    view.push_user("fix it".into());
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(AgentEvent::TextDelta("Working.".into()), secs(1));
    view.push_user("also update the docs".into());
    view.fold(
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 1,
        },
        secs(2),
    );
    view.fold(AgentEvent::TextDelta("Done.".into()), secs(3));
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
        secs(4),
    );
    let steered = view
        .items
        .iter()
        .position(|item| matches!(item, Item::User(text) if text == "also update the docs"))
        .expect("steered message");
    assert_eq!(view.role(steered), Role::Plain);
    assert_eq!(view.role(steered - 1), Role::WorkHeader);

    view.fold(
        AgentEvent::FilesReverted {
            turn_id: 1,
            diffs: vec![FileDiff {
                path: "a.ts".into(),
                unified: "-new\n+old\n".into(),
                added: 1,
                removed: 1,
                created: false,
            }],
            skipped: vec!["b.ts: it changed after the agent's edit".into()],
        },
        secs(5),
    );
    assert_eq!(
        view.items.last(),
        Some(&Item::Reverted {
            restored: vec!["a.ts".into()],
            skipped: vec!["b.ts: it changed after the agent's edit".into()],
        })
    );
    assert!(view.changes.iter().any(|f| f.path == "a.ts"));
}
