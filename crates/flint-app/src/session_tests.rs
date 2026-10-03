use super::{Session, combined};
use flint_agent::{AgentEvent, FileDiff, ToolKind, TurnEndReason};
use std::path::PathBuf;
use std::time::Duration;

fn edited_session(workspace: PathBuf) -> Session {
    let mut session = Session::new(1, workspace);
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
    add_edit(&mut session, "one\n", "two\n");
    session.view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
        Duration::ZERO,
    );
    session
}

fn add_edit(session: &mut Session, before: &str, after: &str) {
    let (unified, added, removed) = combined(before, after, "a.txt");
    let diff = FileDiff {
        path: "a.txt".into(),
        unified,
        added,
        removed,
        created: false,
    };
    session.view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "edit".into(),
            name: "edit".into(),
            kind: ToolKind::Edit,
            args: serde_json::json!({}),
            summary: "a.txt".into(),
        },
        Duration::ZERO,
    );
    session.view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "edit".into(),
            output: "ok".into(),
            exit_code: Some(0),
            success: true,
            duration_ms: 1,
            diff: Some(diff.clone()),
        },
        Duration::ZERO,
    );
    session.record_edit(diff);
}

#[test]
fn settled_changes_apply_only_to_the_matching_session_and_revision() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("a.txt"), "two\n").unwrap();
    let mut session = edited_session(workspace.path().into());
    let snapshot = session.changes_snapshot().unwrap();
    assert!(session.apply_settled_changes(snapshot.compute()));
    assert_eq!(
        (
            session.view.changes[0].added,
            session.view.changes[0].removed
        ),
        (1, 1)
    );
    assert!(session.view.changes[0].combined.is_some());

    let result = session.changes_snapshot().unwrap().compute();
    session.uid += 1;
    assert!(!session.apply_settled_changes(result));

    let result = session.changes_snapshot().unwrap().compute();
    add_edit(&mut session, "two\n", "three\n");
    assert!(!session.apply_settled_changes(result));
    assert!(session.view.changes[0].combined.is_none());
}

#[test]
fn a_new_turn_or_workspace_rejects_old_combined_diff_results() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("a.txt"), "two\n").unwrap();
    let mut session = edited_session(workspace.path().into());
    let result = session.changes_snapshot().unwrap().compute();
    let after_turn_result = session.changes_snapshot().unwrap().compute();
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 2 }, Duration::ZERO);
    assert!(!session.apply_settled_changes(result));
    session.view.fold(
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Completed,
        },
        Duration::ZERO,
    );
    assert!(!session.apply_settled_changes(after_turn_result));

    let mut session = edited_session(workspace.path().into());
    let result = session.changes_snapshot().unwrap().compute();
    session.workspace = workspace.path().join("other");
    assert!(!session.apply_settled_changes(result));
}

#[test]
fn historical_turn_counts_use_their_own_contents_and_skip_edit_free_turns() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("a.txt"), "one\n").unwrap();
    let mut session = edited_session(workspace.path().into());
    add_edit(&mut session, "two\n", "three\n");
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 2 }, Duration::ZERO);
    add_edit(&mut session, "three\n", "one\n");
    session.view.fold(
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Completed,
        },
        Duration::ZERO,
    );
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 3 }, Duration::ZERO);
    session.view.fold(
        AgentEvent::TurnFinished {
            turn_id: 3,
            reason: TurnEndReason::Completed,
        },
        Duration::ZERO,
    );
    session.settle_changes();
    assert_eq!(
        session
            .view
            .turns
            .iter()
            .map(|turn| (turn.added, turn.removed))
            .collect::<Vec<_>>(),
        [(1, 1), (1, 1), (0, 0)]
    );
    assert!(session.view.turns[2].file_stats.is_empty());
    assert_eq!(
        (
            session.view.changes[0].added,
            session.view.changes[0].removed
        ),
        (0, 0)
    );
}

#[test]
fn unreadable_files_do_not_turn_into_phantom_deletions() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("a.txt")).unwrap();
    let mut session = edited_session(workspace.path().into());
    session.settle_changes();
    assert!(session.view.changes[0].combined.is_none());
    assert_eq!(
        (
            session.view.changes[0].added,
            session.view.changes[0].removed
        ),
        (1, 1)
    );
}
