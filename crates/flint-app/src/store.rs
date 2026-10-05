//! Saved sessions under `~/.flint/sessions/<id>/`: `meta.json` (title,
//! workspace, times), `events.jsonl` (what the UI showed, replayed through the
//! view-model on startup) and the engine's own `history.json`.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use flint_agent::AgentEvent;
use flint_agent::AgentKind;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: Option<String>,
    pub workspace: PathBuf,
    /// Unix seconds.
    pub created_at: i64,
    pub updated_at: i64,
    /// Which agent runs the session (older sessions are flint's own).
    #[serde(default)]
    pub agent: AgentKind,
    /// Older saved sessions remain attached to their original project.
    #[serde(default)]
    pub general: bool,
}

/// One line of `events.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Logged {
    User(String),
    Event(AgentEvent),
}

pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join("sessions")
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// A new, sortable session id.
pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

pub fn write_meta(dir: &Path, meta: &Meta) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join("meta.json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(meta)?)?;
    std::fs::rename(temporary, dir.join("meta.json"))
}

pub fn append(dir: &Path, record: &Logged) -> std::io::Result<()> {
    append_batch(dir, std::slice::from_ref(record))
}

fn append_batch(dir: &Path, records: &[Logged]) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let mut lines = Vec::new();
    for record in records {
        serde_json::to_writer(&mut lines, record)?;
        lines.push(b'\n');
    }
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.jsonl"))?;
    let length = file.metadata()?.len();
    if let Err(err) = file.write_all(&lines) {
        // A retry must not append behind a partial JSON line.
        file.set_len(length)?;
        return Err(err);
    }
    Ok(())
}

enum WriteCommand {
    Append(Logged),
    Flush(std::sync::mpsc::SyncSender<std::io::Result<()>>),
    Stop,
}

/// One ordered writer per saved session. Failed batches remain in the worker
/// until an explicit flush retries them; later events cannot overtake them.
pub struct EventWriter {
    commands: std::sync::mpsc::Sender<WriteCommand>,
    error: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    failures: async_channel::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl EventWriter {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        let (commands, receiver) = std::sync::mpsc::channel();
        let (failure_tx, failures) = async_channel::unbounded();
        let error = std::sync::Arc::new(std::sync::Mutex::new(None));
        let worker_error = error.clone();
        let thread = std::thread::Builder::new()
            .name("flint-events".into())
            .spawn(move || {
                let mut pending = Vec::new();
                let mut deferred = None;
                let mut failed = false;
                while let Some(command) = deferred.take().or_else(|| receiver.recv().ok()) {
                    let (reply, stop, retry) = match command {
                        WriteCommand::Append(record) => {
                            pending.push(record);
                            // Coalesce only adjacent appends, never across a barrier.
                            while let Ok(next) = receiver.try_recv() {
                                match next {
                                    WriteCommand::Append(record) => pending.push(record),
                                    other => {
                                        deferred = Some(other);
                                        break;
                                    }
                                }
                            }
                            (None, false, false)
                        }
                        WriteCommand::Flush(reply) => (Some(reply), false, true),
                        WriteCommand::Stop => (None, true, true),
                    };
                    let result = if failed && !retry {
                        Err(std::io::Error::other(
                            worker_error.lock().unwrap().clone().unwrap_or_default(),
                        ))
                    } else {
                        append_batch(&dir, &pending)
                    };
                    match &result {
                        Ok(()) => {
                            pending.clear();
                            failed = false;
                            *worker_error.lock().unwrap() = None;
                        }
                        Err(err) => {
                            if !failed {
                                failure_tx.try_send(err.to_string()).ok();
                            }
                            failed = true;
                            *worker_error.lock().unwrap() = Some(err.to_string());
                        }
                    }
                    if let Some(reply) = reply {
                        reply.send(result).ok();
                    }
                    if stop {
                        break;
                    }
                }
            })?;
        Ok(Self {
            commands,
            error,
            failures,
            thread: Some(thread),
        })
    }

    pub fn append(&self, record: Logged) -> std::io::Result<()> {
        self.enqueue(record)
            .map_err(|_| std::io::Error::other("session writer stopped"))?;
        self.check_error()
    }

    pub(crate) fn enqueue(&self, record: Logged) -> Result<(), Box<Logged>> {
        self.commands
            .send(WriteCommand::Append(record))
            .map_err(|err| {
                let WriteCommand::Append(record) = err.0 else {
                    unreachable!()
                };
                Box::new(record)
            })
    }

    pub(crate) fn check_error(&self) -> std::io::Result<()> {
        if let Some(error) = self.error.lock().unwrap().as_ref() {
            return Err(std::io::Error::other(error.clone()));
        }
        Ok(())
    }

    pub(crate) fn failures(&self) -> async_channel::Receiver<String> {
        self.failures.clone()
    }

    pub(crate) fn has_error(&self) -> bool {
        self.error.lock().unwrap().is_some()
    }

    /// A barrier: acknowledges all earlier records, or reports the retained
    /// batch's failure. Used before moving a session directory and at quit.
    pub fn flush(&self) -> std::io::Result<()> {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.commands
            .send(WriteCommand::Flush(reply))
            .map_err(|_| std::io::Error::other("session writer stopped"))?;
        result
            .recv()
            .map_err(|_| std::io::Error::other("session writer stopped"))?
    }
}

impl Drop for EventWriter {
    fn drop(&mut self) {
        self.commands.send(WriteCommand::Stop).ok();
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// Move a saved session out of the active list without deleting its history.
pub fn archive(home: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let name = dir
        .file_name()
        .ok_or_else(|| std::io::Error::other("invalid session directory"))?;
    let destination = home.join("archive").join(name);
    std::fs::create_dir_all(home.join("archive"))?;
    if destination.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "archive already exists",
        ));
    }
    std::fs::rename(dir, &destination)?;
    Ok(destination)
}

pub fn restore(home: &Path, archived: &Path) -> std::io::Result<PathBuf> {
    let name = archived
        .file_name()
        .ok_or_else(|| std::io::Error::other("invalid archive directory"))?;
    let destination = sessions_dir(home).join(name);
    std::fs::create_dir_all(sessions_dir(home))?;
    if destination.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "session already exists",
        ));
    }
    std::fs::rename(archived, &destination)?;
    Ok(destination)
}

/// Every saved session, most recently updated first. Unreadable entries are
/// skipped.
pub fn load_all(home: &Path) -> Vec<(PathBuf, Meta, Vec<Logged>)> {
    load_directory(&sessions_dir(home))
}

pub fn load_archived(home: &Path) -> Vec<(PathBuf, Meta)> {
    let Ok(entries) = std::fs::read_dir(home.join("archive")) else {
        return Vec::new();
    };
    let mut archives: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            load_meta(&dir).map(|meta| (dir, meta))
        })
        .collect();
    archives.sort_by_key(|(_, meta)| std::cmp::Reverse(meta.updated_at));
    archives
}

fn load_directory(root: &Path) -> Vec<(PathBuf, Meta, Vec<Logged>)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut sessions: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            load(&dir).map(|(meta, events)| (dir, meta, events))
        })
        .collect();
    sessions.sort_by_key(|(_, meta, _)| std::cmp::Reverse(meta.updated_at));
    sessions
}

fn load_meta(dir: &Path) -> Option<Meta> {
    serde_json::from_slice(&std::fs::read(dir.join("meta.json")).ok()?).ok()
}

pub(crate) fn load(dir: &Path) -> Option<(Meta, Vec<Logged>)> {
    let meta = load_meta(dir)?;
    let events = std::fs::read_to_string(dir.join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Some((meta, events))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| match serde_json::from_str::<Logged>(line).unwrap() {
                Logged::User(text) => text,
                _ => panic!("unexpected event"),
            })
            .collect()
    }

    #[test]
    fn event_writer_flush_preserves_order_across_barriers() {
        let root = tempfile::tempdir().unwrap();
        let writer = EventWriter::new(root.path().into()).unwrap();
        for n in 0..4_000 {
            writer.append(Logged::User(n.to_string())).unwrap();
            if n % 500 == 0 {
                writer.flush().unwrap();
                assert_eq!(messages(root.path()).len(), n + 1);
            }
        }
        writer.flush().unwrap();
        assert_eq!(
            messages(root.path()),
            (0..4_000).map(|n| n.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn failed_event_writes_retain_records_and_retry_without_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("events.jsonl");
        std::fs::create_dir(&log).unwrap();
        let writer = EventWriter::new(root.path().into()).unwrap();
        writer.append(Logged::User("first".into())).ok();
        assert!(writer.flush().is_err());
        assert!(writer.has_error());
        assert!(writer.failures().try_recv().is_ok());
        assert!(writer.append(Logged::User("second".into())).is_err());
        assert!(writer.flush().is_err());
        std::fs::remove_dir(&log).unwrap();
        writer.flush().unwrap();
        writer.append(Logged::User("third".into())).unwrap();
        writer.flush().unwrap();
        assert!(!writer.has_error());
        assert_eq!(messages(root.path()), ["first", "second", "third"]);
    }

    #[test]
    fn dropping_a_writer_drains_all_accepted_records() {
        let root = tempfile::tempdir().unwrap();
        {
            let writer = EventWriter::new(root.path().into()).unwrap();
            for n in 0..4_000 {
                writer.append(Logged::User(n.to_string())).unwrap();
            }
        }
        assert_eq!(
            messages(root.path()),
            (0..4_000).map(|n| n.to_string()).collect::<Vec<_>>()
        );
    }
}
