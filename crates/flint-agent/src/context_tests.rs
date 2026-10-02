use pretty_assertions::assert_eq;

use super::*;
use crate::provider::RawToolCall;

fn turn(i: usize, output_chars: usize) -> Vec<Message> {
    vec![
        Message::User(format!("task {i}")),
        Message::Assistant {
            content: String::new(),
            reasoning: "thinking ".repeat(50),
            tool_calls: vec![RawToolCall {
                id: format!("c{i}"),
                name: "write_file".into(),
                arguments:
                    serde_json::json!({"path": format!("f{i}.py"), "content": "x".repeat(2000)})
                        .to_string(),
            }],
        },
        Message::Tool {
            call_id: format!("c{i}"),
            content: format!("{}\n[exit code: 0]", "o".repeat(output_chars)),
        },
        Message::Nudge("verify please".into()),
        Message::Assistant {
            content: format!("answer {i}"),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        },
    ]
}

fn session(turns: usize, output_chars: usize) -> Vec<Message> {
    let mut history = vec![Message::System("sys".into())];
    for i in 0..turns {
        history.extend(turn(i, output_chars));
    }
    history
}

#[test]
fn under_the_trigger_nothing_changes() {
    let tracker = ContextTracker::new(100_000, 0);
    let mut history = session(5, 1000);
    let before = history.clone();
    assert_eq!(tracker.maybe_compact(&mut history), None);
    assert_eq!(history, before);
}

#[test]
fn long_session_compacts_old_turns_in_one_chunk_and_keeps_the_last_two() {
    let tracker = ContextTracker::new(20_000, 0);
    let mut history = session(40, 4200);
    let (before, after) = tracker.maybe_compact(&mut history).expect("compacted");
    assert!(before > 16_000, "before={before}");
    assert!(after <= 10_000, "after={after}");

    // Every user message and assistant answer survives phase 1.
    let users = history
        .iter()
        .filter(|m| matches!(m, Message::User(_)))
        .count();
    assert_eq!(users, 40);
    // The oldest tool output is a stub with its size and exit code.
    assert_eq!(
        history[3],
        Message::Tool {
            call_id: "c0".into(),
            content: "[output trimmed: 4.2k chars, exit 0]".into()
        }
    );
    // Old reasoning is dropped, old file contents are shortened.
    let Message::Assistant {
        reasoning,
        tool_calls,
        ..
    } = &history[2]
    else {
        panic!("assistant")
    };
    assert_eq!(reasoning, "");
    // Parsed: key order depends on serde_json's `preserve_order`, which
    // other workspace crates may enable.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&tool_calls[0].arguments).ok(),
        Some(serde_json::json!({"content": "[trimmed: 2.0k chars]", "path": "f0.py"}))
    );
    // The last two turns are untouched.
    let tail = session(40, 4200).split_off(history.len() - 10);
    assert_eq!(history[history.len() - 10..].to_vec(), tail);

    // Compacting again right away is a no-op: changes come in big chunks.
    let settled = history.clone();
    assert_eq!(tracker.maybe_compact(&mut history), None);
    assert_eq!(history, settled);
}

#[test]
fn growing_session_compacts_rarely() {
    let tracker = ContextTracker::new(30_000, 0);
    let mut history = vec![Message::System("sys".into())];
    let mut compacted_at = Vec::new();
    for i in 0..200 {
        history.extend(turn(i, 4200));
        if tracker.maybe_compact(&mut history).is_some() {
            compacted_at.push(i);
        }
        assert!(
            tracker.estimate(&history) <= 24_000,
            "turn {i} over the trigger"
        );
    }
    // Each turn adds ~1.6k tokens; compacting down to 50% buys ~5 turns of
    // headroom, so compactions come in chunks, never on consecutive turns.
    assert!(!compacted_at.is_empty());
    let min_gap = compacted_at
        .windows(2)
        .map(|w| w[1] - w[0])
        .min()
        .unwrap_or(usize::MAX);
    assert!(min_gap >= 4, "compactions at {compacted_at:?}");
}

#[test]
fn drops_whole_turns_when_stubbing_is_not_enough() {
    let tracker = ContextTracker::new(2_000, 0);
    let mut history = vec![Message::System("sys".into())];
    for i in 0..10 {
        history.push(Message::User(format!(
            "{i} {}",
            "long request ".repeat(100)
        )));
        history.push(Message::Assistant {
            content: "ok".into(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        });
    }
    tracker.maybe_compact(&mut history).expect("compacted");
    assert!(tracker.estimate(&history) <= 1_600);
    assert_eq!(history[0], Message::System("sys".into()));
    assert!(matches!(history.last(), Some(Message::Assistant { .. })));
}

#[test]
fn calibrates_against_reported_tokens() {
    let mut tracker = ContextTracker::new(100_000, 0);
    let history = session(3, 1000);
    let raw = tracker.estimate(&history);
    tracker.observe(&history, raw * 3 / 2);
    assert_eq!(
        tracker.estimate(&history),
        (raw as f64 * 1.5).round() as u64
    );
}

#[test]
fn stub_text() {
    assert_eq!(
        stub_for(&format!("{}\n[exit code: 2]", "x".repeat(900))),
        "[output trimmed: 915 chars, exit 2]"
    );
    assert_eq!(
        stub_for(&format!("Error: {}", "x".repeat(900))),
        "[output trimmed: 907 chars, error]"
    );
}

#[test]
fn large_recent_turns_do_not_recompact_every_turn() {
    // Mirrors the live run: each turn is ~1/4 of the budget, so the two
    // protected turns alone exceed the 50% target.
    let tracker = ContextTracker::new(14_000, 6_000);
    let mut history = vec![Message::System("sys".into())];
    let mut compacted_at = Vec::new();
    for i in 0..30 {
        history.extend(turn(i, 13_000));
        if tracker.maybe_compact(&mut history).is_some() {
            compacted_at.push(i);
        }
    }
    let min_gap = compacted_at
        .windows(2)
        .map(|w| w[1] - w[0])
        .min()
        .unwrap_or(usize::MAX);
    assert!(min_gap >= 2, "compactions at {compacted_at:?}");
}
