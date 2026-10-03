//! Disposable, provider-free persistence and combined-diff profiling.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use flint_agent::{AgentEvent, FileDiff, TurnEndReason};
use flint_app::session::{Session, combined};
use flint_app::store::Logged;
use serde_json::json;

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("pass a fresh absolute output directory"),
    );
    assert!(
        root.is_absolute() && !root.exists(),
        "output directory must be fresh and absolute"
    );
    std::fs::create_dir_all(&root).unwrap();
    let mut persistence = Vec::new();
    for run in 0..5 {
        let mut session = Session::new(run, root.clone());
        session.dir = Some(root.join(format!("events-{run}")));
        let start = Instant::now();
        let mut batches = Vec::new();
        for _ in 0..250 {
            let batch_start = Instant::now();
            for _ in 0..16 {
                session
                    .log(Logged::Event(AgentEvent::TextDelta(
                        "synthetic token ".into(),
                    )))
                    .unwrap();
            }
            batches.push(batch_start.elapsed().as_secs_f64() * 1000.);
        }
        batches.sort_by(f64::total_cmp);
        let enqueue_ms = start.elapsed().as_secs_f64() * 1000.;
        let flush_start = Instant::now();
        session.flush_records().unwrap();
        let flush_ms = flush_start.elapsed().as_secs_f64() * 1000.;
        let records = std::fs::read_to_string(session.dir.as_ref().unwrap().join("events.jsonl"))
            .unwrap()
            .lines()
            .count();
        assert_eq!(records, 4000);
        persistence.push(json!({
            "records": 4000,
            "total_ms": start.elapsed().as_secs_f64() * 1000.,
            "enqueue_ms": enqueue_ms,
            "flush_ms": flush_ms,
            "batch_p95_ms": batches[237],
            "batch_max_ms": batches[249],
        }));
    }
    let mut diffs = Vec::new();
    for lines in [10_000usize, 100_000] {
        let original: String = (0..lines)
            .map(|n| format!("original line {n:06}\n"))
            .collect();
        for run in 0..3 {
            let workspace = root.join(format!("diff-{lines}-{run}"));
            std::fs::create_dir(&workspace).unwrap();
            let mut session = Session::new(run, workspace.clone());
            session.view.push_user("synthetic edits".into());
            session
                .view
                .fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
            let mut current = original.clone();
            for edit in 0..20 {
                let next = current.replacen(
                    &format!("original line {:06}", edit * (lines / 20)),
                    &format!("changed line {:06}", edit * (lines / 20)),
                    1,
                );
                let (unified, added, removed) = combined(&current, &next, "large.txt");
                let diff = FileDiff {
                    path: "large.txt".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                };
                session.view.fold(
                    AgentEvent::ToolCallStarted {
                        call_id: format!("edit-{edit}"),
                        name: "edit".into(),
                        kind: flint_agent::ToolKind::Edit,
                        args: json!({}),
                        summary: "large.txt".into(),
                    },
                    Duration::ZERO,
                );
                session.view.fold(
                    AgentEvent::ToolCallFinished {
                        call_id: format!("edit-{edit}"),
                        output: "ok".into(),
                        success: true,
                        exit_code: Some(0),
                        duration_ms: 1,
                        diff: Some(diff.clone()),
                    },
                    Duration::ZERO,
                );
                session.record_edit(diff);
                current = next;
            }
            std::fs::write(workspace.join("large.txt"), &current).unwrap();
            session.view.fold(
                AgentEvent::TurnFinished {
                    turn_id: 1,
                    reason: TurnEndReason::Completed,
                },
                Duration::ZERO,
            );
            let start = Instant::now();
            let snapshot = session.changes_snapshot().unwrap();
            let snapshot_ms = start.elapsed().as_secs_f64() * 1000.;
            let start = Instant::now();
            let result = snapshot.compute();
            let compute_ms = start.elapsed().as_secs_f64() * 1000.;
            let start = Instant::now();
            assert!(session.apply_settled_changes(result));
            let apply_ms = start.elapsed().as_secs_f64() * 1000.;
            diffs.push(json!({
                "lines": lines, "edits": 20,
                "settle_ms": snapshot_ms + compute_ms + apply_ms,
                "snapshot_ms": snapshot_ms, "compute_ms": compute_ms, "apply_ms": apply_ms,
            }));
        }
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "persistence": persistence, "combined_diff": diffs,
        "scope": "enqueue/flush and snapshot/compute/apply costs; fixtures and patch preparation excluded",
    })).unwrap());
}
