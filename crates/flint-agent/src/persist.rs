//! Saving and restoring a session's history (`<session_dir>/history.json`).
//!
//! Writes are atomic (temp file + rename) and run on a background writer
//! that always writes the latest snapshot and skips stale ones, so a slow
//! disk never blocks the agent loop or the event stream.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::provider::Message;

const FILE_NAME: &str = "history.json";
const VERSION: u32 = 1;

#[derive(Serialize)]
struct SavedRef<'a> {
    version: u32,
    turn_id: u64,
    messages: &'a [Message],
}

#[derive(Deserialize)]
struct Saved {
    version: u32,
    turn_id: u64,
    messages: Vec<Message>,
}

/// A restored conversation (system prompt excluded).
#[derive(Debug, Clone, PartialEq)]
pub struct Restored {
    pub turn_id: u64,
    pub messages: Vec<Message>,
}

/// Serializes the conversation (without the system prompt).
pub fn snapshot(turn_id: u64, messages: &[Message]) -> Vec<u8> {
    let messages = match messages.first() {
        Some(Message::System(_)) => &messages[1..],
        _ => messages,
    };
    serde_json::to_vec(&SavedRef {
        version: VERSION,
        turn_id,
        messages,
    })
    .unwrap_or_default()
}

/// Loads `<dir>/history.json`. `Ok(None)` when there is none; `Err` when it
/// exists but can't be used (the bad file is moved aside so it isn't
/// overwritten).
pub fn load(dir: &Path) -> Result<Option<Restored>, String> {
    let path = dir.join(FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("cannot read {}: {err}", path.display())),
    };
    let parsed = serde_json::from_slice::<Saved>(&bytes)
        .map_err(|err| format!("{} is damaged ({err})", path.display()))
        .and_then(|saved| match saved.version {
            VERSION => Ok(saved),
            other => Err(format!("{} has unknown version {other}", path.display())),
        });
    match parsed {
        Ok(saved) => Ok(Some(Restored {
            turn_id: saved.turn_id,
            messages: repair(saved.messages),
        })),
        Err(message) => {
            let aside = dir.join(format!("{FILE_NAME}.corrupt"));
            let _ = std::fs::rename(&path, &aside);
            Err(format!("{message}; it was moved to {}", aside.display()))
        }
    }
}

/// Drops a stray system prompt and gives every tool call a result, so the
/// restored history is a valid request.
fn repair(messages: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut pending: Vec<String> = Vec::new();
    for message in messages {
        match &message {
            Message::System(_) => continue,
            Message::Tool { call_id, .. } => pending.retain(|id| id != call_id),
            Message::User(_) | Message::Nudge(_) | Message::Assistant { .. } => {
                close_pending(&mut out, &mut pending);
            }
        }
        if let Message::Assistant { tool_calls, .. } = &message {
            pending.extend(tool_calls.iter().map(|c| c.id.clone()));
        }
        out.push(message);
    }
    close_pending(&mut out, &mut pending);
    out
}

fn close_pending(out: &mut Vec<Message>, pending: &mut Vec<String>) {
    for call_id in pending.drain(..) {
        out.push(Message::Tool {
            call_id,
            content: "Interrupted before this ran.".to_string(),
        });
    }
}

/// Background writer for one session directory.
pub struct Saver {
    tx: async_channel::Sender<Vec<u8>>,
    done: tokio::task::JoinHandle<()>,
}

impl Saver {
    pub fn new(dir: PathBuf) -> Self {
        let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
        let done = tokio::spawn(async move {
            while let Ok(mut latest) = rx.recv().await {
                // Skip snapshots that a newer one already replaces.
                while let Ok(newer) = rx.try_recv() {
                    latest = newer;
                }
                let dir = dir.clone();
                let _ = tokio::task::spawn_blocking(move || write_atomic(&dir, &latest)).await;
            }
        });
        Self { tx, done }
    }

    /// Queues a snapshot; returns immediately.
    pub fn save(&self, snapshot: Vec<u8>) {
        let _ = self.tx.try_send(snapshot);
    }

    /// Waits until every queued snapshot is on disk.
    pub async fn flush(self) {
        self.tx.close();
        let _ = self.done.await;
    }
}

/// Writes `history.json` via a temp file and rename.
pub fn write_atomic(dir: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dir.join(FILE_NAME))
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
