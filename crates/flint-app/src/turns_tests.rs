use std::time::Duration;

use flint_agent::AgentEvent;
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
