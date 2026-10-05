//! What one agent last saw of each file, so its edits never overwrite a
//! version it hasn't read: a sibling subagent's edit, the user's own edit, or
//! a formatter run in between. Writes to one path are serialized across every
//! session in the process, so a read-modify-write can't interleave with
//! another agent's.
//!
//! The tracker also feeds the undo [`Journal`], shared by a parent and its
//! subagents: the content each file had before a turn first changed it, so
//! the user can revert that turn's edits.

use std::collections::HashMap;
use std::hash::Hasher;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Content hashes of the files one agent has read or written.
#[derive(Debug, Default)]
pub struct FileTracker {
    seen: Mutex<HashMap<PathBuf, u64>>,
    journal: Arc<Mutex<Journal>>,
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stale {
    /// The file exists but this agent never read it.
    Unread,
    /// The file changed after this agent last read or wrote it.
    Changed,
}

impl FileTracker {
    /// An undo journal confined to the workspace in which writes are approved.
    pub fn for_workspace(workspace: &Path) -> Self {
        Self {
            seen: Mutex::default(),
            journal: Arc::new(Mutex::new(Journal {
                workspace: Some(
                    workspace
                        .canonicalize()
                        .unwrap_or_else(|_| workspace.to_path_buf()),
                ),
                ..Journal::default()
            })),
        }
    }

    /// A tracker for a subagent: it knows nothing of the parent's reads, but
    /// writes to the same undo journal.
    pub fn child(&self) -> Self {
        Self {
            seen: Mutex::default(),
            journal: Arc::clone(&self.journal),
        }
    }

    /// The undo journal this tracker writes to.
    pub fn journal(&self) -> &Arc<Mutex<Journal>> {
        &self.journal
    }

    /// Records a write by this agent: it now knows the new content, and the
    /// journal keeps the content from before the turn's first change.
    pub fn wrote(&self, path: &Path, shown: &str, before: Option<&[u8]>, after: &[u8]) {
        self.saw(path, after);
        if let Ok(mut journal) = self.journal.lock() {
            journal.record(path, shown, before, after);
        }
    }

    /// Records the content this agent now knows `path` to have.
    pub fn saw(&self, path: &Path, content: &[u8]) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(path_key(path), hash(content));
        }
    }

    /// Whether `current` is the content this agent last saw. `None` when it
    /// never saw the file.
    pub fn matches(&self, path: &Path, current: &[u8]) -> Option<bool> {
        let seen = self.seen.lock().ok()?;
        seen.get(&path_key(path))
            .map(|known| *known == hash(current))
    }

    /// Checks an overwrite of an existing file. `require_read` refuses files
    /// this agent never read (whole-file writes); edits only refuse a file
    /// that changed since it was seen, as their exact match is a guard too.
    pub fn check(&self, path: &Path, current: &[u8], require_read: bool) -> Result<(), Stale> {
        match self.matches(path, current) {
            Some(true) => Ok(()),
            Some(false) => Err(Stale::Changed),
            None if require_read => Err(Stale::Unread),
            None => Ok(()),
        }
    }
}

/// Turns kept for undo.
const UNDO_TURNS: usize = 20;
/// Files larger than this are not kept for undo.
const UNDO_MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

/// File content from before each recent turn changed it.
#[derive(Debug, Default)]
pub struct Journal {
    turns: Vec<TurnRecord>,
    recording: bool,
    workspace: Option<PathBuf>,
}

#[derive(Debug)]
struct TurnRecord {
    turn_id: u64,
    files: Vec<FileRecord>,
}

#[derive(Debug)]
struct FileRecord {
    path: PathBuf,
    shown: String,
    /// `None`: the turn created the file.
    before: Option<Vec<u8>>,
    /// Hash of the content the turn left.
    after: u64,
    /// The original was too large to keep; undo skips this file.
    too_large: bool,
    /// Identity of the regular file written, not a later link/replacement.
    identity: Option<FileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(not(unix))]
    created: std::time::SystemTime,
}

fn file_identity(metadata: &std::fs::Metadata) -> Option<FileIdentity> {
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(FileIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        Some(FileIdentity {
            created: metadata.created().ok()?,
        })
    }
}

fn unchanged_identity(file: &FileRecord) -> bool {
    file.path.canonicalize().ok().as_ref() == Some(&file.path)
        && std::fs::symlink_metadata(&file.path)
            .ok()
            .as_ref()
            .and_then(file_identity)
            .is_some_and(|identity| Some(identity) == file.identity)
}

/// What an undo did.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoReport {
    pub turn_id: u64,
    /// One diff per restored file (current content -> restored content).
    pub diffs: Vec<crate::protocol::FileDiff>,
    /// Files left alone, with the reason.
    pub skipped: Vec<(String, String)>,
}

impl Journal {
    /// Starts recording changes for a new user turn.
    pub fn begin_turn(&mut self, turn_id: u64) {
        self.turns.push(TurnRecord {
            turn_id,
            files: Vec::new(),
        });
        if self.turns.len() > UNDO_TURNS {
            self.turns.remove(0);
        }
        self.recording = true;
    }

    fn record(&mut self, path: &Path, shown: &str, before: Option<&[u8]>, after: &[u8]) {
        if !self.recording {
            return;
        }
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        let path = path_key(path);
        let identity = std::fs::symlink_metadata(&path)
            .ok()
            .as_ref()
            .and_then(file_identity);
        if let Some(file) = turn.files.iter_mut().find(|f| f.path == path) {
            file.after = hash(after);
            file.identity = identity;
            return;
        }
        let too_large = before.is_some_and(|b| b.len() > UNDO_MAX_FILE_BYTES);
        turn.files.push(FileRecord {
            path,
            shown: shown.to_string(),
            before: if too_large {
                None
            } else {
                before.map(<[u8]>::to_vec)
            },
            after: hash(after),
            too_large,
            identity,
        });
    }

    /// Whether some recent turn changed files that undo could restore.
    pub fn can_undo(&self) -> bool {
        self.turns.iter().any(|t| !t.files.is_empty())
    }

    /// Restores the files the most recent turn with changes edited, except
    /// those changed again since (by the user or a command) or too large to
    /// have been kept. `seen` learns the restored content.
    pub fn undo_last(&mut self, seen: &FileTracker) -> Result<UndoReport, String> {
        let at = self
            .turns
            .iter()
            .rposition(|t| !t.files.is_empty())
            .ok_or("There are no file changes by the agent to undo.")?;
        let turn = self.turns.remove(at);
        let mut report = UndoReport {
            turn_id: turn.turn_id,
            diffs: Vec::new(),
            skipped: Vec::new(),
        };
        for file in turn.files {
            if file.too_large {
                report
                    .skipped
                    .push((file.shown, "the original was too large to keep".into()));
                continue;
            }
            if !self
                .workspace
                .as_ref()
                .is_some_and(|workspace| file.path.starts_with(workspace))
            {
                report.skipped.push((
                    file.shown,
                    "undo path is not confined to its workspace".into(),
                ));
                continue;
            }
            let lock = write_lock(&file.path);
            let Ok(_held) = lock.try_lock() else {
                report
                    .skipped
                    .push((file.shown, "another edit is in progress".into()));
                continue;
            };
            if !unchanged_identity(&file) {
                report.skipped.push((
                    file.shown,
                    "the file or its symlink identity changed".into(),
                ));
                continue;
            }
            // Open without truncation, then revalidate the opened identity.
            // Restoring through this handle cannot follow a later symlink swap.
            let mut opened = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&file.path)
            {
                Ok(opened) => opened,
                Err(err) => {
                    report.skipped.push((file.shown, err.to_string()));
                    continue;
                }
            };
            if opened.metadata().ok().as_ref().and_then(file_identity) != file.identity
                || !unchanged_identity(&file)
            {
                report
                    .skipped
                    .push((file.shown, "the file identity changed during undo".into()));
                continue;
            }
            let mut bytes = Vec::new();
            if Read::by_ref(&mut opened)
                .take(UNDO_MAX_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() > UNDO_MAX_FILE_BYTES
            {
                report
                    .skipped
                    .push((file.shown, "cannot read bounded undo content".into()));
                continue;
            }
            let current = Some(bytes);
            if current.as_deref().map(hash) != Some(file.after) {
                report
                    .skipped
                    .push((file.shown, "it changed after the agent's edit".into()));
                continue;
            }
            let current_text = current
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            let restored = match &file.before {
                Some(before) => opened
                    .rewind()
                    .and_then(|()| opened.set_len(0))
                    .and_then(|()| opened.write_all(before))
                    .map(|()| {
                        seen.saw(&file.path, before);
                        String::from_utf8_lossy(before).into_owned()
                    }),
                None if unchanged_identity(&file) => {
                    std::fs::remove_file(&file.path).map(|()| String::new())
                }
                None => Err(std::io::Error::other(
                    "the file identity changed during undo",
                )),
            };
            match restored {
                Ok(restored) => {
                    let mut diff =
                        super::file_diff(&file.shown, current_text.as_deref(), &restored);
                    if file.before.is_none() {
                        diff.unified.push_str("(file deleted)\n");
                    }
                    report.diffs.push(diff);
                }
                Err(err) => report.skipped.push((file.shown, err.to_string())),
            }
        }
        Ok(report)
    }
}

fn hash(content: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(content);
    hasher.finish()
}

/// Confined tools resolve paths first; canonical keys unify in-workspace aliases.
fn path_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        match (
            path.parent().and_then(|p| p.canonicalize().ok()),
            path.file_name(),
        ) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

/// The process-wide write lock for `path`.
pub fn write_lock(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Drop locks nobody holds so the map stays small in long sessions.
    if locks.len() > 256 {
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    }
    Arc::clone(locks.entry(path_key(path)).or_default())
}
