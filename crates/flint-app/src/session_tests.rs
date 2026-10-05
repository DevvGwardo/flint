use super::{Session, combined};
use flint_agent::{AgentEvent, FileDiff, ToolKind, TurnEndReason};
use std::path::PathBuf;
use std::time::Duration;

#[test]
#[ignore = "manual release-mode performance probe"]
fn approval_status_perf_probe() {
    use crate::view_model::Item;
    for (case, summary, count) in [
        ("ascii", "x".repeat(8 * 1024 * 1024), 1),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7), 1),
        ("many", "fixture".into(), 20_000),
    ] {
        let items: Vec<_> = (0..count)
            .map(|n| Item::Approval {
                call_id: n.to_string(),
                kind: ToolKind::Command,
                summary: summary.clone(),
                decision: None,
            })
            .collect();
        let start = std::time::Instant::now();
        let mut capacity = 0;
        for _ in 0..100 {
            let line = super::approval_line(std::hint::black_box(&items));
            capacity = capacity.max(line.capacity());
            std::hint::black_box(line);
        }
        eprintln!(
            "approval_status_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        eprintln!("approval_status_{case}_capacity_bytes={capacity}");
    }
}

#[test]
fn approval_sidebar_projection_is_bounded_and_keeps_full_requests() {
    use super::{Status, Tone};
    use crate::view_model::Item;
    let summary = format!("{}\nORIGINAL_BODY", "界🙂".repeat(512));
    let mut session = Session::new(1, PathBuf::new());
    session.view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "first".into(),
            kind: ToolKind::Command,
            summary: summary.clone(),
        },
        Duration::ZERO,
    );
    let (line, tone) = session.status_line(Duration::ZERO);
    assert_eq!(session.status(), Status::NeedsApproval);
    assert_eq!(tone, Tone::Warning);
    assert!(line.chars().count() <= crate::ui::MAX_HEADER_PREVIEW_CHARS + 64);
    assert!(line.ends_with(" …"));
    assert!(line.capacity() <= crate::ui::MAX_HEADER_PREVIEW_CHARS * 8 + 128);
    assert!(
        matches!(&session.view.items[0], Item::Approval {summary: stored, decision: None, ..} if stored == &summary)
    );
}

#[test]
fn approval_sidebar_counts_only_unanswered_requests_in_original_order() {
    use crate::view_model::Item;
    use flint_agent::ApprovalDecision;
    let mut session = Session::new(1, PathBuf::new());
    session.view.pending_approvals = 1;
    assert_eq!(session.status_line(Duration::ZERO).0, "Needs approval");
    session
        .view
        .items
        .push(Item::User("not an approval".into()));
    for (id, summary, decision) in [
        ("answered", "ignore", Some(ApprovalDecision::Deny)),
        ("first", "first 界", None),
        ("empty", "", None),
        ("last", "last", None),
    ] {
        session.view.items.push(Item::Approval {
            call_id: id.into(),
            kind: ToolKind::Command,
            summary: summary.into(),
            decision,
        });
    }
    assert_eq!(
        session.status_line(Duration::ZERO).0,
        "Approve: first 界 (+2 more)"
    );
    session
        .view
        .resolve_approval("first", ApprovalDecision::Approve);
    session.view.pending_approvals = 2;
    assert_eq!(session.status_line(Duration::ZERO).0, "Approve:  (+1 more)");
    session
        .view
        .resolve_approval("empty", ApprovalDecision::Deny);
    session.view.pending_approvals = 1;
    assert_eq!(session.status_line(Duration::ZERO).0, "Approve: last");
}

#[test]
fn failure_status_borrows_the_preferred_source_and_preserves_owned_failure_api() {
    use super::{Status, Tone};
    let mut session = Session::new(1, PathBuf::new());
    let turn_error = format!("  turn failure 界🙂  \r\n{}", "detail".repeat(128 * 1024));
    session.view.last_reason = Some(TurnEndReason::Failed(turn_error.clone()));
    session.view.idle_error = Some("  startup failure é  \nmore detail".into());
    assert!(std::ptr::eq(
        session.failure_text().unwrap(),
        session.view.idle_error.as_deref().unwrap(),
    ));
    let mut owned = session.failure().unwrap();
    owned.clear();
    assert_eq!(
        session.status_line(Duration::ZERO),
        ("Failed: startup failure é".into(), Tone::Danger)
    );
    session.view.idle_error = None;
    let Some(TurnEndReason::Failed(stored)) = &session.view.last_reason else {
        panic!("expected retained turn failure");
    };
    assert!(std::ptr::eq(
        session.failure_text().unwrap(),
        stored.as_str()
    ));
    assert_eq!(session.failure(), Some(turn_error.clone()));
    assert_eq!(
        session.status_line(Duration::ZERO),
        ("Failed: turn failure 界🙂".into(), Tone::Danger)
    );
    session.unread = true;
    assert_eq!(session.status(), Status::Failed { seen: false });
    assert!(matches!(&session.view.last_reason,
        Some(TurnEndReason::Failed(error)) if error == &turn_error));
    session.view.pending_approvals = 1;
    session.view.running = true;
    assert_eq!(session.status(), Status::NeedsApproval);
    session.view.pending_approvals = 0;
    assert_eq!(session.status(), Status::Running);
    session.view.running = false;
    session.view.last_reason = Some(TurnEndReason::StepLimit);
    assert_eq!(session.failure(), Some("stopped at the step limit".into()));
    assert_eq!(
        session.status_line(Duration::ZERO),
        ("Failed: stopped at the step limit".into(), Tone::Danger)
    );
    session.view.last_reason = Some(TurnEndReason::Completed);
    assert_eq!(session.failure(), None);
    assert_eq!(session.failure_text(), None);
}

#[test]
fn failure_sidebar_preview_is_bounded_without_shortening_the_owned_error() {
    use super::{Status, Tone};
    for source in ["idle", "turn"] {
        for first in ["x".repeat(4096), "界🙂".repeat(2048)] {
            let error = format!("  {first}  \r\nORIGINAL_ERROR_BODY");
            let mut session = Session::new(1, PathBuf::new());
            if source == "idle" {
                session.view.idle_error = Some(error.clone());
            } else {
                session.view.last_reason = Some(TurnEndReason::Failed(error.clone()));
            }
            let (line, tone) = session.status_line(Duration::ZERO);
            assert_eq!(session.status(), Status::Failed { seen: true });
            assert_eq!(tone, Tone::Danger);
            assert!(line.chars().count() <= crate::ui::MAX_HEADER_PREVIEW_CHARS + 10);
            assert!(line.capacity() <= crate::ui::MAX_HEADER_PREVIEW_CHARS * 8 + 128);
            assert!(line.starts_with("Failed: "));
            assert!(line.ends_with(" …"));
            assert!(!line.contains("ORIGINAL_ERROR_BODY"));
            assert_eq!(session.failure_text(), Some(error.as_str()));
            let mut owned = session.failure().unwrap();
            assert_eq!(owned, error);
            owned.clear();
            assert_eq!(session.failure().as_deref(), Some(error.as_str()));
        }
    }
}

#[test]
fn failure_sidebar_preview_preserves_first_line_semantics_at_the_limit() {
    use crate::ui::MAX_HEADER_PREVIEW_CHARS;
    let mut errors: Vec<String> = [
        "",
        "\nBODY",
        "\r\nBODY",
        " \t\u{2003}\nBODY",
        " first \r\nBODY",
        "one\u{2028}two",
        "one\rtwo",
        "é🙂\nBODY",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for ch in ['x', '界', '🙂', 'é'] {
        for len in [0, 1, 239, 240, 241, 512] {
            for suffix in ["", "\nBODY", "\r\nBODY", " \t\nBODY"] {
                errors.push(format!(" \t{}{suffix}", ch.to_string().repeat(len)));
            }
        }
    }
    errors.push(format!("{}\t\u{2003}Z\nBODY", "x".repeat(239)));
    for error in errors {
        let first = error.lines().next().unwrap_or_default().trim();
        let expected = if first.chars().count() <= MAX_HEADER_PREVIEW_CHARS {
            first.to_owned()
        } else {
            let prefix: String = first.chars().take(MAX_HEADER_PREVIEW_CHARS).collect();
            format!("{} …", prefix.trim_end())
        };
        let mut session = Session::new(1, PathBuf::new());
        session.view.idle_error = Some(error.clone());
        assert_eq!(
            session.status_line(Duration::ZERO),
            (format!("Failed: {expected}"), super::Tone::Danger)
        );
        assert_eq!(session.failure().as_deref(), Some(error.as_str()));
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn failure_preview_perf_probe() {
    for (case, error) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("blank_first", format!("\n{}", "x".repeat(8 * 1024 * 1024))),
    ] {
        let mut session = Session::new(1, PathBuf::new());
        session.view.idle_error = Some(error);
        let start = std::time::Instant::now();
        let mut capacity = 0;
        for _ in 0..100 {
            let (line, tone) = std::hint::black_box(&session).status_line(Duration::ZERO);
            capacity = capacity.max(line.capacity());
            std::hint::black_box((line, tone));
        }
        eprintln!(
            "failure_preview_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        eprintln!("failure_preview_{case}_capacity_bytes={capacity}");
    }
}

#[test]
fn failure_status_keeps_empty_and_multiline_first_line_behavior() {
    for error in [
        "",
        "\nsecond",
        " first \r\nsecond",
        "é🙂\nsecond",
        "one\u{2028}two",
    ] {
        let mut session = Session::new(1, PathBuf::new());
        session.view.idle_error = Some(error.into());
        assert_eq!(session.status(), super::Status::Failed { seen: true });
        assert_eq!(
            session.status_line(Duration::ZERO),
            (
                format!(
                    "Failed: {}",
                    error.lines().next().unwrap_or_default().trim()
                ),
                super::Tone::Danger
            )
        );
        assert_eq!(session.failure().as_deref(), Some(error));
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn failure_status_perf_probe() {
    for source in ["idle", "turn"] {
        let mut session = Session::new(1, PathBuf::new());
        let error = format!("  fixture failure 界🙂  \n{}", "x".repeat(8 * 1024 * 1024));
        if source == "idle" {
            session.view.idle_error = Some(error);
        } else {
            session.view.last_reason = Some(TurnEndReason::Failed(error));
        }
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(std::hint::black_box(&session).status());
        }
        eprintln!(
            "failure_{source}_status_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(
                std::hint::black_box(&session).status_line(std::time::Duration::ZERO),
            );
        }
        eprintln!(
            "failure_{source}_line_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        assert_eq!(session.status(), super::Status::Failed { seen: true });
        assert_eq!(
            session.status_line(Duration::ZERO),
            ("Failed: fixture failure 界🙂".into(), super::Tone::Danger)
        );
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn unsaved_event_logging_perf_probe() {
    let mut session = Session::new(1, PathBuf::new());
    let event = AgentEvent::ToolOutputDelta {
        call_id: "controlled-output".into(),
        chunk: "x".repeat(8 * 1024 * 1024),
    };
    let start = std::time::Instant::now();
    for _ in 0..100 {
        std::hint::black_box(&mut session)
            .log_event(std::hint::black_box(&event))
            .unwrap();
    }
    eprintln!(
        "unsaved_event_logging_100_runs_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    assert!(session.dir.is_none());
    assert!(session.event_writer.is_none());
    assert!(session.pending_records.is_empty());
}

#[test]
fn unsaved_event_logging_does_not_start_storage_or_consume_the_event() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(1, root.path().into());
    let event = AgentEvent::TextDelta("unsaved 界🙂".into());
    session.log_event(&event).unwrap();
    session.flush_records().unwrap();
    assert!(session.event_writer.is_none());
    assert!(session.pending_records.is_empty());
    assert!(!session.storage_failed());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(matches!(&event, AgentEvent::TextDelta(text) if text == "unsaved 界🙂"));
}

#[test]
fn saved_event_logging_owns_records_and_keeps_order_across_flushes() {
    use crate::store::Logged;
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(1, root.path().into());
    session.dir = Some(root.path().join("saved"));
    let mut first = AgentEvent::TextDelta("original 界🙂".into());
    session.log(Logged::User("prompt".into())).unwrap();
    session.log_event(&first).unwrap();
    if let AgentEvent::TextDelta(text) = &mut first {
        *text = "mutated after logging".into();
    }
    session.flush_records().unwrap();
    session
        .log_event(&AgentEvent::TextDelta("second".into()))
        .unwrap();
    session.flush_records().unwrap();
    let records: Vec<serde_json::Value> =
        std::fs::read_to_string(session.dir.as_ref().unwrap().join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(
        records,
        vec![
            serde_json::to_value(Logged::User("prompt".into())).unwrap(),
            serde_json::to_value(Logged::Event(AgentEvent::TextDelta("original 界🙂".into())))
                .unwrap(),
            serde_json::to_value(Logged::Event(AgentEvent::TextDelta("second".into()))).unwrap(),
        ]
    );
    assert!(!session.storage_failed());
}

#[test]
fn saved_event_logging_retains_failed_records_and_retries_without_duplicates() {
    use crate::store::Logged;
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(1, root.path().into());
    session.dir = Some(root.path().join("saved"));
    let log = session.dir.as_ref().unwrap().join("events.jsonl");
    std::fs::create_dir_all(&log).unwrap();
    session
        .log_event(&AgentEvent::TextDelta("first".into()))
        .ok();
    assert!(session.flush_records().is_err());
    assert!(session.storage_failed());
    assert!(
        session
            .log_event(&AgentEvent::TextDelta("second".into()))
            .is_err()
    );
    assert!(session.flush_records().is_err());
    std::fs::remove_dir(&log).unwrap();
    session.flush_records().unwrap();
    session
        .log_event(&AgentEvent::TextDelta("third".into()))
        .unwrap();
    session.flush_records().unwrap();
    let records: Vec<String> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| match serde_json::from_str::<Logged>(line).unwrap() {
            Logged::Event(AgentEvent::TextDelta(text)) => text,
            other => panic!("unexpected record: {other:?}"),
        })
        .collect();
    assert_eq!(records, ["first", "second", "third"]);
    assert!(!session.storage_failed());
}

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

#[test]
fn bucket_folds_the_statuses_into_four() {
    use super::{Bucket, Status};
    assert_eq!(Status::NeedsApproval.bucket(), Bucket::NeedsYou);
    assert_eq!(Status::Unread.bucket(), Bucket::NeedsYou);
    assert_eq!(Status::Running.bucket(), Bucket::Working);
    assert_eq!(Status::Done.bucket(), Bucket::Ready);
    assert_eq!(Status::Idle.bucket(), Bucket::Inactive);
}

#[test]
fn last_message_text_is_the_newest_user_or_assistant_text() {
    let mut session = Session::new(1, PathBuf::from("/w"));
    assert_eq!(session.last_message_text(), None);
    let change = session.view.push_user("first question".to_string());
    session.apply(change);
    assert_eq!(session.last_message_text(), Some("first question"));
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
    session
        .view
        .fold(AgentEvent::TextDelta("the answer".into()), Duration::ZERO);
    assert_eq!(session.last_message_text(), Some("the answer"));
}

#[test]
fn color_seed_follows_the_saved_folder_when_there_is_one() {
    let mut a = Session::new(1, PathBuf::from("/w"));
    let mut b = Session::new(2, PathBuf::from("/w"));
    assert_ne!(a.color_seed(), b.color_seed());
    a.dir = Some(PathBuf::from("/home/sessions/abc"));
    b.dir = Some(PathBuf::from("/elsewhere/sessions/abc"));
    assert_eq!(a.color_seed(), b.color_seed());
}

#[test]
fn tint_keeps_lightness_and_alpha_and_is_deterministic() {
    let base = gpui_kit::hsla(0., 0., 0.8, 0.9);
    let one = crate::ui::tinted(base, 7);
    assert_eq!(one, crate::ui::tinted(base, 7));
    assert_eq!((one.l, one.a), (0.8, 0.9));
    let hues: std::collections::HashSet<u32> = (0..12)
        .map(|seed| (crate::ui::tinted(base, seed).h * 360.).round() as u32)
        .collect();
    assert!(hues.len() >= 10, "sequential ids should spread: {hues:?}");
}

#[test]
fn running_subagents_counts_only_children_still_working() {
    let mut session = Session::new(1, PathBuf::from("/w"));
    assert_eq!(session.running_subagents(), 0);
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
    for id in ["a", "b"] {
        session.view.fold(
            AgentEvent::ToolCallStarted {
                call_id: id.into(),
                name: "spawn_agent".into(),
                kind: ToolKind::Other,
                args: serde_json::json!({}),
                summary: id.into(),
            },
            Duration::ZERO,
        );
        session.view.fold(
            AgentEvent::SubagentStarted {
                call_id: id.into(),
                session_id: format!("child-{id}"),
                model: "m".into(),
            },
            Duration::ZERO,
        );
    }
    for id in ["a", "b"] {
        session.view.fold(
            AgentEvent::SubagentEvent {
                call_id: id.into(),
                event: Box::new(AgentEvent::TurnStarted { turn_id: 1 }),
            },
            Duration::ZERO,
        );
    }
    assert_eq!(session.running_subagents(), 2);
    session.view.fold(
        AgentEvent::SubagentEvent {
            call_id: "a".into(),
            event: Box::new(AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            }),
        },
        Duration::ZERO,
    );
    assert_eq!(session.running_subagents(), 1);
}

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn finished(session: &mut Session, reason: TurnEndReason) {
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    session
        .view
        .fold(AgentEvent::TurnFinished { turn_id: 1, reason }, secs(1));
}

#[test]
fn failed_stopped_and_starting_sessions_have_their_own_status() {
    use super::{Bucket, Status, Tone};
    let mut failed = Session::new(1, PathBuf::from("/w"));
    failed.view.push_user("go".into());
    finished(
        &mut failed,
        TurnEndReason::Failed("provider returned 401".into()),
    );
    failed.unread = true;
    assert_eq!(failed.status(), Status::Failed { seen: false });
    assert_eq!(failed.status().bucket(), Bucket::NeedsYou);
    assert_eq!(
        failed.status_line(secs(9)),
        ("Failed: provider returned 401".to_string(), Tone::Danger)
    );
    // Once looked at it no longer needs you, but still reads as failed.
    failed.unread = false;
    assert_eq!(failed.status(), Status::Failed { seen: true });
    assert_eq!(failed.status().bucket(), Bucket::Ready);

    let mut limit = Session::new(2, PathBuf::from("/w"));
    finished(&mut limit, TurnEndReason::StepLimit);
    assert_eq!(
        limit.failure().as_deref(),
        Some("stopped at the step limit")
    );

    let mut stopped = Session::new(3, PathBuf::from("/w"));
    finished(&mut stopped, TurnEndReason::Interrupted);
    assert_eq!(stopped.status(), Status::Stopped);
    assert_eq!(stopped.status_line(secs(0)).0, "Stopped");

    // An engine that fails to start for a sent message.
    let mut broken = Session::new(4, PathBuf::from("/w"));
    broken.view.push_user("hello".into());
    broken
        .view
        .fold(AgentEvent::Error("no API key".into()), secs(0));
    assert_eq!(broken.status(), Status::Failed { seen: true });
    // A new turn clears it.
    finished(&mut broken, TurnEndReason::Completed);
    assert_eq!(broken.status(), Status::Done);

    // Notices don't fail a session: one inside a completed turn, or one
    // after it (e.g. undo had nothing to restore).
    let mut fine = Session::new(5, PathBuf::from("/w"));
    fine.view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    fine.view
        .fold(AgentEvent::Error("effort not supported".into()), secs(0));
    fine.view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
        secs(1),
    );
    fine.view.fold(
        AgentEvent::Error("There are no file changes to undo.".into()),
        secs(2),
    );
    assert_eq!(fine.status(), Status::Done);

    assert_eq!(Status::Starting.bucket(), Bucket::Working);
}

#[test]
fn running_sessions_say_what_they_are_doing_and_for_how_long() {
    use super::Tone;
    let mut session = Session::new(1, PathBuf::from("/w"));
    session
        .view
        .fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(10));
    session
        .view
        .fold(AgentEvent::ReasoningDelta("hmm".into()), secs(11));
    assert_eq!(
        session.status_line(secs(15)),
        ("Thinking… · 5s".to_string(), Tone::Muted)
    );
    let call = |id: &str, name: &str, kind, summary: &str| AgentEvent::ToolCallStarted {
        call_id: id.into(),
        name: name.into(),
        kind,
        args: serde_json::json!({}),
        summary: summary.into(),
    };
    session.view.fold(
        call("e1", "edit_file", ToolKind::Edit, "src/app.rs"),
        secs(12),
    );
    assert_eq!(session.activity(), "Editing src/app.rs");
    // Parallel reads: the newest unfinished one is shown.
    session.view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "e1".into(),
            output: "ok".into(),
            exit_code: None,
            success: true,
            diff: None,
            duration_ms: 1,
        },
        secs(13),
    );
    session
        .view
        .fold(call("r1", "read_file", ToolKind::Read, "a.rs"), secs(13));
    session.view.fold(
        call("r2", "grep", ToolKind::Search, "fn main in ."),
        secs(13),
    );
    assert_eq!(session.activity(), "Searching fn main in .");
    session.view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "c1".into(),
            kind: ToolKind::Command,
            summary: "cargo test".into(),
        },
        secs(14),
    );
    session.view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "c2".into(),
            kind: ToolKind::Command,
            summary: "npm test".into(),
        },
        secs(14),
    );
    assert_eq!(
        session.status_line(secs(200)),
        ("Approve: cargo test (+1 more)".to_string(), Tone::Warning)
    );
}

#[test]
fn clock_reads_seconds_minutes_and_hours() {
    assert_eq!(super::clock(secs(42)), "42s");
    assert_eq!(super::clock(secs(185)), "3m 05s");
    assert_eq!(super::clock(secs(4_380)), "1h 13m");
}
