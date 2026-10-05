use std::time::Duration;

use flint_agent::{AgentEvent, ToolKind, TurnEndReason};
use serde_json::json;

use crate::session::{Session, Status};
use crate::view_model::{Item, SessionView};

fn start(view: &mut SessionView, call: &str, child: &str, message: &str) {
    for event in [
        AgentEvent::ToolCallStarted {
            call_id: call.into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Audit permissions".into(),
            args: json!({"label": "Audit permissions", "message": message}),
        },
        AgentEvent::SubagentStarted {
            call_id: call.into(),
            session_id: child.into(),
            model: "audit-model".into(),
        },
    ] {
        view.fold(event, Duration::ZERO);
    }
}

fn child(view: &mut SessionView, call: &str, event: AgentEvent) {
    view.fold(
        AgentEvent::SubagentEvent {
            call_id: call.into(),
            event: Box::new(event),
        },
        Duration::ZERO,
    );
}

#[test]
fn resumed_subagent_keeps_one_conversation_and_both_turns() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
    for (call, turn, prompt, answer) in [
        ("first", 1, "Inspect permissions.", "First answer."),
        ("resume", 2, "Check the fix.", "Second answer."),
    ] {
        start(&mut view, call, "agent-1", prompt);
        assert!(view.subagent("agent-1").unwrap().queued);
        child(&mut view, call, AgentEvent::TurnStarted { turn_id: turn });
        child(&mut view, call, AgentEvent::TextDelta(answer.into()));
        child(
            &mut view,
            call,
            AgentEvent::TurnFinished {
                turn_id: turn,
                reason: TurnEndReason::Completed,
            },
        );
    }
    assert_eq!(view.subagents.len(), 1);
    let agent = view.subagent("agent-1").unwrap();
    assert_eq!(agent.label, "Audit permissions");
    assert_eq!(agent.view.turns.len(), 2);
    assert_eq!(
        agent
            .view
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) | Item::Assistant { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            "Inspect permissions.",
            "First answer.",
            "Check the fix.",
            "Second answer."
        ]
    );
    assert!(!agent.queued);
    assert_eq!(
        view.items.len(),
        2,
        "child messages must not enter parent context"
    );
}

#[test]
fn duplicate_started_event_does_not_duplicate_child_prompt() {
    let mut view = SessionView::default();
    start(&mut view, "call", "agent-1", "Inspect.");
    let change = view.fold(
        AgentEvent::SubagentStarted {
            call_id: "call".into(),
            session_id: "agent-1".into(),
            model: "audit-model".into(),
        },
        Duration::ZERO,
    );
    assert_eq!(change, crate::view_model::Change::default());
    assert_eq!(view.subagents.len(), 1);
    assert_eq!(
        view.subagents[0].view.items,
        [Item::User("Inspect.".into())]
    );
}

#[test]
fn interrupt_closes_running_and_queued_children_without_touching_finished_children() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
    for id in ["running", "queued", "finished"] {
        start(&mut view, id, id, "Inspect.");
    }
    child(&mut view, "running", AgentEvent::TurnStarted { turn_id: 1 });
    child(
        &mut view,
        "finished",
        AgentEvent::TurnStarted { turn_id: 1 },
    );
    child(
        &mut view,
        "finished",
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
        Duration::ZERO,
    );
    for id in ["running", "queued"] {
        let agent = view.subagent(id).unwrap();
        assert!(!agent.queued && !agent.view.running);
        assert_eq!(agent.status(&view), Status::Stopped);
    }
    assert_eq!(
        view.subagent("finished").unwrap().view.last_reason,
        Some(TurnEndReason::Completed)
    );
}

#[test]
fn session_applies_child_list_changes_in_batch_order() {
    let mut session = Session::new(1, "/workspace".into());
    let mut changes = Vec::new();
    for event in [
        AgentEvent::ToolCallStarted {
            call_id: "call".into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Research".into(),
            args: json!({"message": "Inspect."}),
        },
        AgentEvent::SubagentStarted {
            call_id: "call".into(),
            session_id: "agent-1".into(),
            model: "model".into(),
        },
        AgentEvent::SubagentEvent {
            call_id: "call".into(),
            event: Box::new(AgentEvent::TextDelta("Answer.".into())),
        },
    ] {
        let change = session.view.fold(event, Duration::ZERO);
        changes.extend(change.children);
    }
    session.apply(crate::view_model::Change {
        appended: 0..1,
        updated: Vec::new(),
        children: changes,
    });
    assert_eq!(session.subagent_lists["agent-1"].item_count(), 2);
    assert_eq!(session.list.item_count(), 1);
}
