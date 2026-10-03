use pretty_assertions::assert_eq;

use super::*;
use crate::provider::RawToolCall;

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
            messages: sample()[1..].to_vec()
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
fn corrupt_or_partial_file_is_moved_aside() {
    let dir = tempfile::tempdir().expect("tempdir");
    let full = snapshot(1, &sample());
    std::fs::write(dir.path().join("history.json"), &full[..full.len() / 2]).expect("write");
    let err = load(dir.path()).expect_err("damaged");
    assert!(err.contains("is damaged"), "{err}");
    assert!(dir.path().join("history.json.corrupt").exists());
    assert_eq!(load(dir.path()), Ok(None));
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
