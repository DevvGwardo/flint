use pretty_assertions::assert_eq;
use std::time::Duration;

use super::*;
use crate::provider::RawToolCall;
use crate::task::{TaskOutcome, TaskRecord};

#[test]
fn version_one_history_has_no_retroactive_task_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("history.json"),
        r#"{"version":1,"turn_id":2,"messages":[{"user":"old request"}]}"#,
    )
    .unwrap();
    let restored = load(dir.path()).unwrap().unwrap();
    assert_eq!(restored.turn_id, 2);
    assert!(restored.task.is_none());
}

#[test]
fn task_and_history_roundtrip_in_one_atomic_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = TaskRecord::new(1, "Original goal.\nRequired file: result.c".into(), 0);
    task.assess(crate::protocol::TurnEndReason::Completed, true);
    let mut messages = sample();
    messages.insert(1, Message::TaskContext("untrusted saved host hint".into()));
    write_atomic(dir.path(), &snapshot_with_task(1, &messages, Some(&task))).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(FILE_NAME)).unwrap()).unwrap();
    assert_eq!(saved["messages"], serde_json::json!(&sample()[1..]));
    let restored = load(dir.path()).unwrap().unwrap();
    assert_eq!(restored.task, Some(task));
    assert_eq!(restored.messages, sample()[1..]);
}

#[test]
fn inconsistent_saved_acceptance_is_quarantined_without_overwriting_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = TaskRecord::new(1, "Fix it".into(), 0);
    task.assess(crate::protocol::TurnEndReason::Completed, true);
    task.assessment.as_mut().unwrap().outcome = TaskOutcome::Accepted;
    let bytes = snapshot_with_task(1, &sample(), Some(&task));
    write_atomic(dir.path(), &bytes).unwrap();
    assert!(load(dir.path()).is_err());
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
}

fn saved_review(task: &TaskRecord) -> crate::TaskReview {
    crate::TaskReview {
        turn_id: 7,
        task_id: task.contract.id,
        contract_revision: task.contract.revision,
        work_revision: task.work_revision,
        decision: crate::ReviewDecision::Accept,
    }
}

fn saved_fixture(dir: &Path, workspace: &Path) -> TaskRecord {
    std::fs::write(workspace.join("result.c"), "actual output").unwrap();
    let mut task = TaskRecord::new(1, "Original goal.\nRequired file: result.c".into(), 4);
    task.refresh_files(workspace);
    task.assess(crate::TurnEndReason::Completed, true);
    write_atomic(dir, &snapshot_with_task(7, &sample(), Some(&task))).unwrap();
    task
}

#[test]
fn stopped_native_review_is_persisted_with_history_and_revision_guards() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let task = saved_fixture(dir.path(), workspace.path());
    let reviewed =
        crate::review_saved_task(dir.path(), workspace.path(), &saved_review(&task)).unwrap();
    assert!(reviewed.is_accepted());
    assert_eq!(crate::saved_task(dir.path()).unwrap(), Some((7, reviewed)));
    assert_eq!(load(dir.path()).unwrap().unwrap().messages, sample()[1..]);
    let before = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
    let mut wrong_turn = saved_review(&task);
    wrong_turn.turn_id = 6;
    assert!(crate::review_saved_task(dir.path(), workspace.path(), &wrong_turn).is_err());
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), before);
}

#[test]
fn stopped_native_stale_review_saves_fresh_drift_without_accepting_it() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let task = saved_fixture(dir.path(), workspace.path());
    let old_review = saved_review(&task);
    std::fs::write(workspace.path().join("result.c"), "externally changed").unwrap();
    assert!(
        crate::review_saved_task(dir.path(), workspace.path(), &old_review)
            .unwrap_err()
            .contains("evidence changed")
    );
    let (_, refreshed) = crate::saved_task(dir.path()).unwrap().unwrap();
    assert_eq!(refreshed.work_revision, 5);
    assert!(!refreshed.is_accepted());
    let accepted =
        crate::review_saved_task(dir.path(), workspace.path(), &saved_review(&refreshed)).unwrap();
    assert!(accepted.is_accepted());
}

#[test]
fn stopped_native_scope_edit_keeps_the_original_goal_and_invalidates_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let task = saved_fixture(dir.path(), workspace.path());
    crate::review_saved_task(dir.path(), workspace.path(), &saved_review(&task)).unwrap();
    let edit = crate::TaskEdit {
        task_id: 1,
        expected_revision: 1,
        objective: "A deliberate narrower goal.".into(),
        mode: crate::TaskMode::ReadOnly,
        requirements: task
            .contract
            .requirements
            .iter()
            .map(|requirement| requirement.spec.clone())
            .collect(),
        restrictions: vec!["Don't publish anything.".into()],
        clear_reported_blockers: false,
    };
    let (_, revised) = crate::edit_saved_task(dir.path(), 7, edit.clone()).unwrap();
    assert_eq!(revised.contract.revision, 2);
    assert_eq!(
        revised.contract.original_request,
        task.contract.original_request
    );
    assert_eq!(revised.contract.mode, crate::TaskMode::ReadOnly);
    assert!(!revised.is_accepted());
    let bytes = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
    assert!(crate::edit_saved_task(dir.path(), 7, edit).is_err());
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
}

#[test]
fn stopped_native_review_while_another_writer_owns_history_preserves_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let task = saved_fixture(dir.path(), workspace.path());
    let bytes = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
    let _writer = crate::orchestration::fs::WriterLease::acquire(dir.path()).unwrap();
    assert!(crate::review_saved_task(dir.path(), workspace.path(), &saved_review(&task)).is_err());
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
    assert!(
        !crate::saved_task(dir.path())
            .unwrap()
            .unwrap()
            .1
            .is_accepted()
    );
}

#[test]
fn saved_task_projection_reports_damage_without_moving_or_overwriting_history() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"{partial history";
    std::fs::write(dir.path().join(FILE_NAME), bytes).unwrap();
    assert!(crate::saved_task(dir.path()).is_err());
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
    assert!(!dir.path().join("history.json.corrupt").exists());
}

#[test]
fn stopped_native_controls_refuse_external_history_without_rewriting_it() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let task = TaskRecord::unverified(
        1,
        "External work".into(),
        crate::TurnEndReason::Completed,
        crate::TaskSource::External,
    );
    let bytes = snapshot_with_task(7, &sample(), Some(&task));
    write_atomic(dir.path(), &bytes).unwrap();
    assert!(
        crate::review_saved_task(dir.path(), workspace.path(), &saved_review(&task))
            .unwrap_err()
            .contains("no native task")
    );
    assert!(
        crate::edit_saved_task(
            dir.path(),
            7,
            crate::TaskEdit {
                task_id: 1,
                expected_revision: 1,
                objective: "Don't rewrite external scope".into(),
                mode: crate::TaskMode::ReadOnly,
                requirements: Vec::new(),
                restrictions: Vec::new(),
                clear_reported_blockers: false,
            }
        )
        .unwrap_err()
        .contains("no native task")
    );
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
}

fn sample() -> Vec<Message> {
    vec![
        Message::System("sys".into()),
        Message::User("fix it".into()),
        Message::Assistant {
            content: String::new(),
            reasoning: "hmm".into(),
            tool_calls: vec![RawToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }],
        },
        Message::Tool {
            call_id: "c1".into(),
            content: "ok".into(),
        },
        Message::Nudge("verify".into()),
        Message::Assistant {
            content: "done".into(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        },
    ]
}

#[test]
fn round_trips_without_the_system_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_atomic(dir.path(), &snapshot(7, &sample())).expect("write");
    assert_eq!(
        load(dir.path()),
        Ok(Some(Restored {
            turn_id: 7,
            messages: sample()[1..].to_vec(),
            task: None,
        }))
    );
    assert!(!dir.path().join("history.json.tmp").exists());
}

#[test]
fn missing_file_is_a_fresh_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(load(dir.path()), Ok(None));
}

#[test]
fn corrupt_or_partial_file_remains_in_place_with_repeatable_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let full = snapshot(1, &sample());
    std::fs::write(dir.path().join("history.json"), &full[..full.len() / 2]).expect("write");
    let err = load(dir.path()).expect_err("damaged");
    assert!(err.contains("is damaged"), "{err}");
    assert!(!dir.path().join("history.json.corrupt").exists());
    assert_eq!(
        std::fs::read(dir.path().join(FILE_NAME)).unwrap(),
        full[..full.len() / 2]
    );
    assert!(load(dir.path()).is_err());
}

#[test]
fn dangling_tool_calls_get_results() {
    let mut messages = sample()[..3].to_vec();
    messages.push(Message::User("next".into()));
    let repaired = repair(messages);
    assert_eq!(
        repaired[2..],
        [
            Message::Tool {
                call_id: "c1".into(),
                content: "Interrupted before this ran.".into()
            },
            Message::User("next".into()),
        ]
    );
}

#[tokio::test]
async fn saver_writes_the_latest_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saver = Saver::new(dir.path().to_path_buf());
    for turn in 1..=50 {
        saver.save(snapshot(turn, &sample()));
    }
    saver.flush().await.expect("flush");
    assert_eq!(load(dir.path()).expect("load").map(|r| r.turn_id), Some(50));
}

#[tokio::test]
async fn saver_coalesces_snapshots_before_the_writer_is_polled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saver = Saver::new(dir.path().to_path_buf());
    let messages = [Message::User("x".repeat(65_536))];
    let mut submitted_bytes = 0;
    for turn in 1..=128 {
        let bytes = snapshot(turn, &messages);
        submitted_bytes += bytes.len();
        saver.save(bytes);
    }
    // A current-thread runtime cannot poll the writer until this test yields.
    let queued = saver.tx.len();
    let retained_bytes = saver
        .latest
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .encoded()
        .len();
    assert_eq!(retained_bytes, snapshot(128, &messages).len());
    eprintln!(
        "SNAPSHOT_PRESSURE submitted=128 bytes={submitted_bytes} queued={queued} retained_bytes={retained_bytes}"
    );
    saver.flush().await.expect("flush");
    assert_eq!(
        load(dir.path()).expect("load").map(|r| r.turn_id),
        Some(128)
    );
    assert_eq!(queued, 1, "obsolete snapshots must not accumulate");
}

struct WriteRelease(Option<std::sync::mpsc::Sender<()>>);

impl WriteRelease {
    fn open(&mut self) {
        if let Some(release) = self.0.take() {
            release.send(()).ok();
        }
    }
}

impl Drop for WriteRelease {
    fn drop(&mut self) {
        self.open();
    }
}

struct GatedSaver {
    saver: Saver,
    started: tokio::sync::oneshot::Receiver<()>,
    release: WriteRelease,
    writes: Arc<Mutex<Vec<u64>>>,
}

fn gated_saver(dir: &Path, fail_first: bool) -> GatedSaver {
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = writes.clone();
    let mut started = Some(started);
    let saver = Saver::with_write(dir.to_path_buf(), move |dir, bytes| {
        let saved: Saved = serde_json::from_slice(bytes).unwrap();
        observed.lock().unwrap().push(saved.turn_id);
        if let Some(started) = started.take() {
            started.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            if fail_first {
                return Err(std::io::Error::other("first write failed"));
            }
        }
        write_atomic(dir, bytes)
    });
    GatedSaver {
        saver,
        started: started_rx,
        release: WriteRelease(Some(release)),
        writes,
    }
}

#[tokio::test]
async fn blocked_writer_retains_one_latest_snapshot_and_flush_waits_for_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let GatedSaver {
        saver,
        started,
        mut release,
        writes,
    } = gated_saver(dir.path(), false);
    saver.save(snapshot(1, &sample()));
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    let messages = [Message::User("x".repeat(65_536))];
    for turn in 2..=128 {
        saver.save(snapshot(turn, &messages));
    }
    assert_eq!(
        saver
            .latest
            .lock()
            .unwrap()
            .as_ref()
            .map(|pending| pending.encoded()),
        Some(snapshot(128, &messages))
    );
    assert_eq!(saver.tx.len(), 1);
    let flush = tokio::spawn(saver.flush());
    tokio::task::yield_now().await;
    assert!(
        !flush.is_finished(),
        "flush must wait for the blocked write"
    );
    release.open();
    tokio::time::timeout(Duration::from_secs(5), flush)
        .await
        .unwrap()
        .unwrap()
        .expect("flush");
    assert_eq!(*writes.lock().unwrap(), [1, 128]);
    assert_eq!(
        load(dir.path()).unwrap(),
        Some(Restored {
            turn_id: 128,
            messages: messages.to_vec(),
            task: None,
        })
    );
}

#[tokio::test]
async fn newer_complete_snapshot_recovers_from_a_failed_inflight_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let GatedSaver {
        saver,
        started,
        mut release,
        writes,
    } = gated_saver(dir.path(), true);
    saver.save(snapshot(1, &sample()));
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    for turn in 2..=50 {
        saver.save(snapshot(turn, &sample()));
    }
    release.open();
    saver.flush().await.expect("newest snapshot was saved");
    assert_eq!(*writes.lock().unwrap(), [1, 50]);
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 50);
}

#[tokio::test]
async fn final_write_failure_does_not_acknowledge_a_saved_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_atomic(dir.path(), &snapshot(1, &sample())).unwrap();
    let saver = Saver::with_write(dir.path().to_path_buf(), |_, _| {
        Err(std::io::Error::other("storage unavailable"))
    });
    saver.save(snapshot(2, &sample()));
    let error = saver.flush().await.expect_err("must report failure");
    assert_eq!(error.to_string(), "storage unavailable");
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 1);
}

#[tokio::test]
async fn panicked_history_writer_fails_flush() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saver = Saver::with_write(dir.path().to_path_buf(), |_, _| panic!("writer fixture"));
    saver.save(snapshot(1, &sample()));
    let error = tokio::time::timeout(Duration::from_secs(5), saver.flush())
        .await
        .unwrap()
        .expect_err("must report panic");
    assert_eq!(error.to_string(), "history writer stopped");
    assert!(!dir.path().join(FILE_NAME).exists());
}

#[tokio::test]
async fn unused_saver_flush_does_not_write_an_empty_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saver = Saver::with_write(dir.path().to_path_buf(), |_, _| {
        panic!("an unused saver must not write")
    });
    saver.flush().await.expect("no work to flush");
    assert!(!dir.path().join(FILE_NAME).exists());
}

#[tokio::test]
async fn task_checkpoint_waits_for_the_latest_complete_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let GatedSaver {
        saver,
        started,
        mut release,
        writes,
    } = gated_saver(dir.path(), false);
    saver.save(snapshot(1, &sample()));
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    let mut task = TaskRecord::new(2, "Deliver the actual result.".into(), 0);
    task.assess(crate::TurnEndReason::Completed, true);
    task.review(&crate::TaskReview {
        turn_id: 2,
        task_id: 2,
        contract_revision: 1,
        work_revision: 0,
        decision: crate::ReviewDecision::Accept,
    })
    .unwrap();
    saver.save(snapshot_with_task(2, &sample(), Some(&task)));
    let saver = Arc::new(saver);
    let checkpoint_saver = saver.clone();
    let checkpoint = tokio::spawn(async move { checkpoint_saver.checkpoint().await });
    tokio::task::yield_now().await;
    assert!(
        !checkpoint.is_finished(),
        "acceptance cannot precede the history write"
    );
    assert!(!dir.path().join(FILE_NAME).exists());
    release.open();
    tokio::time::timeout(Duration::from_secs(5), checkpoint)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let restored = load(dir.path()).unwrap().unwrap();
    assert_eq!(restored.turn_id, 2);
    assert_eq!(restored.task, Some(task));
    assert_eq!(*writes.lock().unwrap(), [1, 2]);
    Arc::try_unwrap(saver).ok().unwrap().flush().await.unwrap();
}

#[tokio::test]
async fn task_checkpoint_reports_failed_writes_and_confirms_a_later_recovery() {
    let dir = tempfile::tempdir().unwrap();
    write_atomic(dir.path(), &snapshot(1, &sample())).unwrap();
    let mut fail = true;
    let saver = Saver::with_write(dir.path().into(), move |dir, bytes| {
        if fail {
            fail = false;
            return Err(std::io::Error::other("task checkpoint unavailable"));
        }
        write_atomic(dir, bytes)
    });
    saver.save(snapshot(2, &sample()));
    assert_eq!(
        saver.checkpoint().await.unwrap_err().to_string(),
        "task checkpoint unavailable"
    );
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 1);
    saver.save(snapshot(3, &sample()));
    saver.checkpoint().await.unwrap();
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 3);
    saver.flush().await.unwrap();
}

#[tokio::test]
async fn task_checkpoint_reports_writer_panics_without_hanging() {
    let dir = tempfile::tempdir().unwrap();
    let saver = Saver::with_write(dir.path().into(), |_, _| {
        panic!("checkpoint writer fixture")
    });
    saver.save(snapshot(1, &sample()));
    assert!(
        tokio::time::timeout(Duration::from_secs(5), saver.checkpoint())
            .await
            .unwrap()
            .is_err()
    );
    assert!(saver.flush().await.is_err());
}

#[tokio::test]
async fn dropping_saver_still_writes_the_latest_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (written, completed) = async_channel::unbounded();
    let saver = Saver::with_write(dir.path().to_path_buf(), move |dir, bytes| {
        write_atomic(dir, bytes)?;
        written.try_send(()).unwrap();
        Ok(())
    });
    for turn in 1..=50 {
        saver.save(snapshot(turn, &sample()));
    }
    drop(saver);
    tokio::time::timeout(Duration::from_secs(5), completed.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 50);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), completed.recv())
            .await
            .unwrap()
            .is_err(),
        "the worker should finish when its owner is dropped"
    );
}

#[tokio::test]
async fn blocked_history_writer_does_not_block_another_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let other_dir = tempfile::tempdir().expect("tempdir");
    let GatedSaver {
        saver,
        started,
        mut release,
        writes,
    } = gated_saver(dir.path(), false);
    saver.save(snapshot(1, &sample()));
    tokio::time::timeout(Duration::from_secs(5), started)
        .await
        .unwrap()
        .unwrap();
    let other = Saver::new(other_dir.path().to_path_buf());
    other.save(snapshot(9, &sample()));
    tokio::time::timeout(Duration::from_secs(5), other.flush())
        .await
        .unwrap()
        .expect("independent writer");
    assert_eq!(load(other_dir.path()).unwrap().unwrap().turn_id, 9);
    assert!(!dir.path().join(FILE_NAME).exists());
    assert_eq!(*writes.lock().unwrap(), [1]);
    release.open();
    saver.flush().await.expect("flush");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_snapshot_replacements_do_not_lose_the_final_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saver = Arc::new(Saver::new(dir.path().to_path_buf()));
    let mut producers = Vec::new();
    for producer in 0..4 {
        let saver = saver.clone();
        producers.push(tokio::spawn(async move {
            for turn in 1..=64 {
                saver.save(snapshot(producer * 64 + turn, &sample()));
                tokio::task::yield_now().await;
            }
        }));
    }
    for producer in producers {
        producer.await.unwrap();
    }
    let saver = Arc::try_unwrap(saver).ok().expect("producers finished");
    saver.save(snapshot(1_000, &sample()));
    tokio::time::timeout(Duration::from_secs(5), saver.flush())
        .await
        .unwrap()
        .expect("flush");
    assert_eq!(load(dir.path()).unwrap().unwrap().turn_id, 1_000);
}

fn calls(ids: &[&str]) -> Message {
    Message::Assistant {
        content: String::new(),
        reasoning: "reasoning".into(),
        tool_calls: ids
            .iter()
            .map(|id| RawToolCall {
                id: (*id).into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            })
            .collect(),
    }
}

fn result(id: &str, content: &str) -> Message {
    Message::Tool {
        call_id: id.into(),
        content: content.into(),
    }
}

#[test]
fn orphan_tool_results_are_removed() {
    assert_eq!(
        repair(vec![
            Message::System("old system".into()),
            result("orphan", "unmatched"),
            Message::User("continue".into()),
            result("orphan", "still unmatched"),
        ]),
        [Message::User("continue".into())]
    );
}

#[test]
fn duplicate_tool_results_keep_only_the_first_result() {
    assert_eq!(
        repair(vec![
            calls(&["c1"]),
            result("c1", "first"),
            result("c1", "duplicate"),
        ]),
        [calls(&["c1"]), result("c1", "first")]
    );
}

#[test]
fn late_tool_results_do_not_cross_message_boundaries() {
    for boundary in [
        Message::User("next".into()),
        Message::Nudge("verify".into()),
        Message::Summary("earlier work".into()),
        calls(&[]),
        Message::UserWithImages {
            text: "look".into(),
            images: vec![crate::protocol::ImageAttachment {
                name: "fixture.png".into(),
                mime_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            }],
        },
    ] {
        assert_eq!(
            repair(vec![
                calls(&["c1"]),
                boundary.clone(),
                result("c1", "too late"),
            ]),
            [
                calls(&["c1"]),
                result("c1", "Interrupted before this ran."),
                boundary,
            ]
        );
    }
}

#[test]
fn parallel_tool_results_keep_order_and_fill_only_missing_calls() {
    assert_eq!(
        repair(vec![
            calls(&["c1", "c2", "c3"]),
            result("c3", "third"),
            result("c1", "first"),
            Message::User("next".into()),
        ]),
        [
            calls(&["c1", "c2", "c3"]),
            result("c3", "third"),
            result("c1", "first"),
            result("c2", "Interrupted before this ran."),
            Message::User("next".into()),
        ]
    );
}

#[test]
fn reused_tool_ids_are_matched_with_the_current_call_group() {
    let messages = vec![
        calls(&["c1"]),
        result("c1", "earlier"),
        calls(&["c1", "c2"]),
        result("c2", "second"),
        result("c1", "newer"),
    ];
    assert_eq!(repair(messages.clone()), messages);
}

fn assert_valid_history(messages: &[Message]) {
    let mut pending = std::collections::HashSet::new();
    for message in messages {
        match message {
            Message::System(_) => panic!("restored history must not include a system prompt"),
            Message::Tool { call_id, .. } => {
                assert!(
                    pending.remove(call_id.as_str()),
                    "unmatched result: {call_id}"
                );
            }
            Message::Assistant { tool_calls, .. } => {
                assert!(pending.is_empty(), "unanswered calls before assistant");
                for call in tool_calls {
                    assert!(pending.insert(call.id.as_str()), "duplicate call id");
                }
            }
            _ => assert!(pending.is_empty(), "unanswered calls before user message"),
        }
    }
    assert!(pending.is_empty(), "unanswered calls at end of history");
}

#[test]
fn repaired_histories_are_valid_and_idempotent_for_mixed_interruptions() {
    let variants = [
        Message::System("old system".into()),
        Message::User("next".into()),
        Message::Nudge("verify".into()),
        calls(&[]),
        calls(&["c1"]),
        calls(&["c1", "c2"]),
        result("c1", "first"),
        result("c2", "second"),
        result("orphan", "unmatched"),
    ];
    for mut index in 0..variants.len().pow(4) {
        let messages = (0..4)
            .map(|_| {
                let message = variants[index % variants.len()].clone();
                index /= variants.len();
                message
            })
            .collect();
        let repaired = repair(messages);
        assert_valid_history(&repaired);
        assert_eq!(repair(repaired.clone()), repaired);
    }
}

#[test]
fn loading_repairs_invalid_results_without_changing_the_saved_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bytes = snapshot(
        7,
        &[
            result("orphan", "unmatched"),
            calls(&["c1", "c2"]),
            result("c1", "first"),
            result("c1", "duplicate"),
            Message::User("continue".into()),
            result("c2", "too late"),
        ],
    );
    write_atomic(dir.path(), &bytes).unwrap();
    let restored = load(dir.path()).unwrap().unwrap();
    assert_eq!(restored.turn_id, 7);
    assert_valid_history(&restored.messages);
    assert_eq!(
        restored.messages,
        [
            calls(&["c1", "c2"]),
            result("c1", "first"),
            result("c2", "Interrupted before this ran."),
            Message::User("continue".into()),
        ]
    );
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), bytes);
    let wire: serde_json::Value = serde_json::from_slice(
        &crate::provider::ChatRequest {
            messages: &restored.messages,
            replay_reasoning_from: 0,
            tools_json: "",
            effort: None,
        }
        .body("fixture"),
    )
    .unwrap();
    assert_eq!(wire["messages"][1]["tool_call_id"], "c1");
    assert_eq!(wire["messages"][2]["tool_call_id"], "c2");
    assert_eq!(wire["messages"][3]["role"], "user");
    assert_eq!(wire["messages"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn writer_serialized_snapshots_match_preformatted_bytes() {
    for (bytes_dir, session_dir) in [
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()),
        // The same transcript, saved twice, must not depend on who formats it.
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()),
    ] {
        let mut task = TaskRecord::new(1, "Fix it.\nRequired file: result.c".into(), 0);
        task.assess(crate::protocol::TurnEndReason::Completed, true);
        let messages = sample();
        let bytes_saver = Saver::new(bytes_dir.path().to_path_buf());
        bytes_saver.save(snapshot_with_task(3, &messages, Some(&task)));
        bytes_saver.flush().await.expect("bytes flush");

        let session_saver = Saver::new(session_dir.path().to_path_buf());
        session_saver.save(SaveData::Session(Box::new(SessionSnapshot {
            turn_id: 3,
            messages: Arc::new(messages.clone()),
            task: Some(task.clone()),
        })));
        session_saver.flush().await.expect("session flush");

        assert_eq!(
            std::fs::read(bytes_dir.path().join(FILE_NAME)).unwrap(),
            std::fs::read(session_dir.path().join(FILE_NAME)).unwrap()
        );
        let restored = load(session_dir.path()).unwrap().unwrap();
        assert_eq!(restored.turn_id, 3);
        assert_eq!(restored.task, Some(task));
        assert_eq!(restored.messages, messages[1..]);
    }
}

#[tokio::test]
async fn deferred_serialization_still_keeps_and_checkpoints_the_newest_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let saver = Saver::new(dir.path().to_path_buf());
    let messages = Arc::new(sample());
    // A burst synthesizes one document, and the newest turn must be the one
    // that survives coalescing: serialization is deferred, never dropped.
    for turn in 1..=32 {
        saver.save(SaveData::Session(Box::new(SessionSnapshot {
            turn_id: turn,
            messages: Arc::clone(&messages),
            task: None,
        })));
    }
    saver.checkpoint().await.expect("checkpoint");
    let restored = load(dir.path()).unwrap().unwrap();
    assert_eq!(restored.turn_id, 32);
    saver.flush().await.expect("flush");
    assert_eq!(
        load(dir.path()).unwrap().unwrap().turn_id,
        32,
        "a later flush must not lose the checkpointed snapshot"
    );
}

#[tokio::test]
async fn poisoned_writer_mutex_stays_fail_closed_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let saver = Saver::new(dir.path().to_path_buf());
    // Poison the pending-snapshot mutex by panicking while holding the lock.
    let latest = saver.latest.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = latest.lock().unwrap();
        panic!("intentional poison for regression test");
    }));
    assert!(latest.is_poisoned());
    // Neither the foreground save nor the background writer may panic.
    saver.save(Vec::new());
    saver.checkpoint().await.expect("checkpoint");
    saver.flush().await.expect("flush");
}
