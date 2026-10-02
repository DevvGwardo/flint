//! Saved sessions under `~/.flint/sessions/<id>/`: `meta.json` (title,
//! workspace, times), `events.jsonl` (what the UI showed, replayed through the
//! view-model on startup) and the engine's own `history.json`.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use flint_agent::AgentEvent;
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
    std::fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(meta)?)
}

pub fn append(dir: &Path, record: &Logged) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.jsonl"))?;
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    file.write_all(&line)
}

pub fn delete(dir: &Path) -> std::io::Result<()> {
    std::fs::remove_dir_all(dir)
}

/// Every saved session, most recently updated first. Unreadable entries are
/// skipped.
pub fn load_all(home: &Path) -> Vec<(PathBuf, Meta, Vec<Logged>)> {
    let Ok(entries) = std::fs::read_dir(sessions_dir(home)) else {
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

fn load(dir: &Path) -> Option<(Meta, Vec<Logged>)> {
    let meta: Meta = serde_json::from_slice(&std::fs::read(dir.join("meta.json")).ok()?).ok()?;
    let events = std::fs::read_to_string(dir.join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Some((meta, events))
}
