//! Saving and restoring a session's history (`<session_dir>/history.json`).
//!
//! Writes are atomic (temp file + rename) and run on a background writer
//! that always writes the latest snapshot and skips stale ones, so a slow
//! disk never blocks the agent loop or the event stream.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;

use crate::provider::Message;
use crate::task::TaskRecord;

const FILE_NAME: &str = "history.json";
const VERSION: u32 = 2;

#[derive(Serialize)]
struct SavedRef<'a> {
    version: u32,
    turn_id: u64,
    messages: &'a [&'a Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<&'a TaskRecord>,
}

#[derive(Deserialize)]
struct Saved {
    version: u32,
    turn_id: u64,
    messages: Vec<Message>,
    #[serde(default)]
    task: Option<TaskRecord>,
}

/// A restored conversation (system prompt excluded).
#[derive(Debug, Clone, PartialEq)]
pub struct Restored {
    pub turn_id: u64,
    pub messages: Vec<Message>,
    pub task: Option<TaskRecord>,
}

/// Serializes the conversation (without the system prompt).
#[cfg(test)]
pub fn snapshot(turn_id: u64, messages: &[Message]) -> Vec<u8> {
    snapshot_with_task(turn_id, messages, None)
}

pub fn snapshot_with_task(
    turn_id: u64,
    messages: &[Message],
    task: Option<&TaskRecord>,
) -> Vec<u8> {
    let messages: Vec<_> = messages
        .iter()
        .filter(|message| !matches!(message, Message::System(_) | Message::TaskContext(_)))
        .collect();
    serde_json::to_vec(&SavedRef {
        version: VERSION,
        turn_id,
        messages: &messages,
        task,
    })
    .unwrap_or_default()
}

/// Loads `<dir>/history.json`. `Ok(None)` when there is none; `Err` when it
/// exists but can't be used. Damaged and unknown versions stay in place.
pub fn load(dir: &Path) -> Result<Option<Restored>, String> {
    let directory = match crate::orchestration::fs::Directory::open(dir) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read saved history. Existing history has been kept.".into()),
    };
    let Some(document) = crate::orchestration::store::read_document(&directory)
        .map_err(|error| format!("Saved history is damaged or unsupported: {error}"))?
    else {
        return Ok(None);
    };
    let saved: Saved = serde_json::from_value(document)
        .map_err(|_| "Saved history is damaged. Existing history has been kept.")?;
    if !matches!(saved.version, 1..=3) {
        return Err("Unsupported history version. Existing history has been kept.".into());
    }
    Ok(Some(Restored {
        turn_id: saved.turn_id,
        messages: repair(saved.messages),
        task: saved.task,
    }))
}

/// Drops stray system prompts and unmatched tool results, and gives every
/// pending tool call a result, so the restored history is a valid request.
fn repair(messages: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut pending: Vec<String> = Vec::new();
    for message in messages {
        match &message {
            Message::System(_) | Message::TaskContext(_) => continue,
            Message::Tool { call_id, .. } => {
                let Some(ix) = pending.iter().position(|id| id == call_id) else {
                    continue;
                };
                pending.remove(ix);
            }
            Message::User(_)
            | Message::UserWithImages { .. }
            | Message::Nudge(_)
            | Message::Summary(_)
            | Message::Assistant { .. } => {
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

/// Background writer for one session directory. Only the newest pending
/// complete snapshot is retained, in addition to an in-flight write.
pub struct Saver {
    tx: async_channel::Sender<SaveCommand>,
    latest: Arc<Mutex<Option<Vec<u8>>>>,
    done: tokio::task::JoinHandle<std::io::Result<()>>,
}

enum SaveCommand {
    Wake,
    Checkpoint(tokio::sync::oneshot::Sender<std::io::Result<()>>),
}

impl Saver {
    #[cfg(test)]
    pub fn new(dir: PathBuf) -> Self {
        match Self::try_new(dir.clone()) {
            Ok(saver) => saver,
            Err(error) => {
                let message = error.to_string();
                Self::with_write(dir, move |_, _| Err(std::io::Error::other(message.clone())))
            }
        }
    }

    pub(crate) fn try_new(dir: PathBuf) -> std::io::Result<Self> {
        let lease = crate::orchestration::fs::WriterLease::acquire(&dir)?;
        let document = crate::orchestration::store::read_document(&lease.directory)
            .map_err(std::io::Error::other)?;
        if document.is_some_and(|document| document["version"] == 3) {
            return Err(std::io::Error::other(
                "Legacy execution cannot schedule a v3 orchestration session in B1. History has been kept.",
            ));
        }
        Ok(Self::with_write(dir, move |_, bytes| {
            write_legacy_locked(&lease, bytes)
        }))
    }

    fn with_write(
        dir: PathBuf,
        mut write: impl FnMut(&Path, &[u8]) -> std::io::Result<()> + Send + 'static,
    ) -> Self {
        // This channel carries wakeups, not snapshots or transcript events.
        let (tx, rx) = async_channel::bounded(1);
        let latest = Arc::new(Mutex::new(None::<Vec<u8>>));
        let pending = latest.clone();
        let done = tokio::spawn(async move {
            let mut result = Ok(());
            while let Ok(command) = rx.recv().await {
                let latest = pending.lock().unwrap().take();
                if let Some(latest) = latest {
                    let dir = dir.clone();
                    let completed = tokio::task::spawn_blocking(move || {
                        let result = write(&dir, &latest);
                        (write, result)
                    })
                    .await;
                    let (writer, written) = match completed {
                        Ok(completed) => completed,
                        Err(_) => {
                            // async-channel retains queued values while a
                            // sender exists. Drain checkpoint replies before
                            // exiting, or their callers can wait forever.
                            rx.close();
                            while let Ok(command) = rx.try_recv() {
                                if let SaveCommand::Checkpoint(reply) = command {
                                    let _ = reply
                                        .send(Err(std::io::Error::other("history writer stopped")));
                                }
                            }
                            return Err(std::io::Error::other("history writer stopped"));
                        }
                    };
                    write = writer;
                    result = written;
                }
                if let SaveCommand::Checkpoint(reply) = command {
                    let confirmed = result
                        .as_ref()
                        .copied()
                        .map_err(|error| std::io::Error::new(error.kind(), error.to_string()));
                    let _ = reply.send(confirmed);
                }
            }
            result
        });
        Self { tx, latest, done }
    }

    #[cfg(test)]
    pub(crate) fn failing_for_tests(dir: PathBuf) -> Self {
        Self::with_write(dir, |_, _| {
            Err(std::io::Error::other("injected history write failure"))
        })
    }

    /// Replaces the pending complete snapshot without waiting for storage.
    pub fn save(&self, snapshot: Vec<u8>) {
        let previous = self.latest.lock().unwrap().replace(snapshot);
        drop(previous);
        // A full channel already has a wakeup for the shared latest snapshot.
        let _ = self.tx.try_send(SaveCommand::Wake);
    }

    /// Explicit task controls wait for durable history before announcing a
    /// scope or acceptance change. Ordinary turn snapshots remain coalesced.
    pub async fn checkpoint(&self) -> std::io::Result<()> {
        let (reply, confirmed) = tokio::sync::oneshot::channel();
        self.tx
            .send(SaveCommand::Checkpoint(reply))
            .await
            .map_err(|_| std::io::Error::other("history writer stopped"))?;
        confirmed
            .await
            .map_err(|_| std::io::Error::other("history writer stopped"))?
    }

    /// Waits for the newest snapshot to finish writing, reporting its result.
    pub async fn flush(self) -> std::io::Result<()> {
        self.tx.close();
        self.done
            .await
            .unwrap_or_else(|_| Err(std::io::Error::other("history writer stopped")))
    }
}

/// Writes `history.json` via a temp file and rename.
#[cfg(test)]
pub fn write_atomic(dir: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let lease = crate::orchestration::fs::WriterLease::acquire(dir)?;
    write_legacy_locked(&lease, bytes)
}

fn write_legacy_locked(
    lease: &crate::orchestration::fs::WriterLease,
    bytes: &[u8],
) -> std::io::Result<()> {
    if bytes.len() > crate::orchestration::store::MAX_SNAPSHOT_BYTES {
        return Err(std::io::Error::other("History exceeds its byte limit."));
    }
    let existing = crate::orchestration::store::read_document(&lease.directory)
        .map_err(std::io::Error::other)?;
    if existing.is_some_and(|document| document["version"] == 3) {
        return Err(std::io::Error::other(
            "Refusing a legacy write over v3 history.",
        ));
    }
    lease.directory.replace(FILE_NAME, bytes)
}

/// Stopped-task mutations hold the same lease from load through commit and
/// replace only the task field, retaining history and all v3/extension fields.
pub(crate) fn save_task_locked(
    lease: &crate::orchestration::fs::WriterLease,
    document: &mut serde_json::Value,
    task: &TaskRecord,
) -> std::io::Result<()> {
    document["task"] = serde_json::to_value(task)?;
    if document["version"] == 3 {
        let revision = document["orchestration"]["revision"]
            .as_u64()
            .and_then(|revision| revision.checked_add(1))
            .ok_or_else(|| std::io::Error::other("Invalid orchestration revision."))?;
        document["orchestration"]["revision"] = serde_json::json!(revision);
    }
    let bytes = serde_json::to_vec(document)?;
    if bytes.len() > crate::orchestration::store::MAX_SNAPSHOT_BYTES {
        return Err(std::io::Error::other("History exceeds its byte limit."));
    }
    lease.directory.replace(FILE_NAME, &bytes)
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
