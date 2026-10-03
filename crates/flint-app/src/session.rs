//! One conversation: its view-model, its virtual-list state, its workspace,
//! its link to the engine (or the demo player), and its saved copy on disk.
//! Sessions run independently, so several can work at once in the background.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use flint_agent::AgentEvent;
use flint_agent::AgentKind;
use flint_agent::ApprovalMode;
use flint_agent::FileDiff;
use flint_agent::Op;
use gpui_kit::*;

use crate::store;
use crate::store::Logged;
use crate::view_model::Change;
use crate::view_model::SessionView;

pub struct Session {
    /// Stable within this run; event pumps address sessions by it.
    pub uid: u64,
    /// Saved-session directory (`~/.flint/sessions/<id>/`), once it has
    /// sent a message (demo and automation sessions are never saved).
    pub dir: Option<PathBuf>,
    pub view: SessionView,
    pub list: ListState,
    /// flint's engine or an ACP agent; fixed once the session has started.
    pub agent: AgentKind,
    /// The agent's own options (model, reasoning, mode, …), as last reported.
    pub options: Vec<flint_agent::SessionOption>,
    /// The ACP agent has opened its session (it reported its options).
    pub agent_ready: bool,
    /// The ACP agent failed before it was ready.
    pub agent_failed: bool,
    /// Config captured when a native engine starts; settings edits cannot
    /// reconfigure an already-running engine.
    pub native_model: Option<String>,
    pub native_approval: Option<ApprovalMode>,
    pub native_allow_all: bool,
    pub workspace: PathBuf,
    pub created: SystemTime,
    /// Last activity, for ordering and the sidebar's relative time.
    pub touched: SystemTime,
    /// A turn finished while the session was not on screen.
    pub unread: bool,
    /// Engine ops channel, once the session has started an engine.
    pub ops: Option<async_channel::Sender<Op>>,
    /// Events pump (engine) or demo playback; dropping it stops either.
    pub pump: Option<Task<()>>,
    /// When the user last sent a message, for time-to-first-token.
    pub submitted_at: Option<Instant>,
    /// Time from sending to the first streamed token (reasoning or text).
    pub first_token: Option<Duration>,
    /// Time from sending to the first answer text.
    pub first_text: Option<Duration>,
    /// The last message as shown and as sent (with attachments), for Retry.
    pub last_message: Option<(String, String, Vec<flint_agent::ImageAttachment>)>,
    /// Failed UI writes are retained in order and retried before archiving.
    pub(crate) pending_records: VecDeque<Logged>,
    pub(crate) event_writer: Option<store::EventWriter>,
    pub(crate) persistence_task: Option<Task<()>>,
    /// Shutdown has been requested; submissions and retries must wait.
    pub stopping: bool,
    pub(crate) history_save_failed: bool,
    pub(crate) engine_shutdown_verified: bool,
    /// Every edit this session, oldest first: (turn index, diff).
    diff_log: Vec<(usize, Arc<FileDiff>)>,
    pub(crate) changes_task: Option<Task<()>>,
}

/// The sidebar's live status glyph for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running,
    NeedsApproval,
    Unread,
    Done,
}

impl Session {
    pub fn new(uid: u64, workspace: PathBuf) -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            uid,
            dir: None,
            view: SessionView::default(),
            list,
            agent: AgentKind::Flint,
            options: Vec::new(),
            agent_ready: false,
            agent_failed: false,
            native_model: None,
            native_approval: None,
            native_allow_all: false,
            workspace,
            created: SystemTime::now(),
            touched: SystemTime::now(),
            unread: false,
            ops: None,
            pump: None,
            submitted_at: None,
            first_token: None,
            first_text: None,
            last_message: None,
            pending_records: VecDeque::new(),
            event_writer: None,
            persistence_task: None,
            stopping: false,
            history_save_failed: false,
            engine_shutdown_verified: true,
            diff_log: Vec::new(),
            changes_task: None,
        }
    }

    /// Keeps the virtual list in step with a view-model change.
    pub fn apply(&self, change: Change) {
        for ix in change.updated {
            self.list.remeasure_items(ix..ix + 1);
        }
        if !change.appended.is_empty() {
            let at = change.appended.start;
            self.list.splice(at..at, change.appended.len());
        }
    }

    /// An ACP agent is starting up (its adapter can take 20–50 s).
    pub fn agent_starting(&self) -> bool {
        self.agent != flint_agent::AgentKind::Flint
            && self.ops.is_some()
            && !self.agent_ready
            && !self.agent_failed
    }

    pub fn title(&self) -> String {
        self.view
            .title
            .clone()
            .unwrap_or_else(|| "New session".to_string())
    }

    pub fn status(&self) -> Status {
        if self.view.pending_approvals > 0 {
            Status::NeedsApproval
        } else if self.view.running {
            Status::Running
        } else if self.unread {
            Status::Unread
        } else if self.view.turns.is_empty() {
            Status::Idle
        } else {
            Status::Done
        }
    }

    /// Queues an ordered saved event (no-op for unsaved sessions). A flush
    /// barrier is required when the caller needs disk acknowledgment.
    pub fn log(&mut self, record: Logged) -> std::io::Result<()> {
        if let Some(dir) = &self.dir {
            self.pending_records.push_back(record);
            if self.event_writer.is_none() {
                self.event_writer = Some(store::EventWriter::new(dir.clone())?);
            }
            self.enqueue_pending()?;
            self.event_writer.as_ref().unwrap().check_error()?;
        }
        Ok(())
    }

    fn enqueue_pending(&mut self) -> std::io::Result<()> {
        let writer = self.event_writer.as_ref().unwrap();
        while let Some(record) = self.pending_records.pop_front() {
            if let Err(record) = writer.enqueue(record) {
                self.pending_records.push_front(*record);
                return Err(std::io::Error::other("session writer stopped"));
            }
        }
        Ok(())
    }

    pub(crate) fn storage_failed(&self) -> bool {
        !self.pending_records.is_empty()
            || self
                .event_writer
                .as_ref()
                .is_some_and(store::EventWriter::has_error)
    }

    pub fn flush_records(&mut self) -> std::io::Result<()> {
        if self.event_writer.is_some() {
            self.enqueue_pending()?;
            return self.event_writer.as_ref().unwrap().flush();
        }
        if let Some(dir) = &self.dir {
            while let Some(record) = self.pending_records.front() {
                store::append(dir, record)?;
                self.pending_records.pop_front();
            }
        }
        Ok(())
    }

    pub fn save_meta(&self) -> std::io::Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        store::write_meta(
            dir,
            &store::Meta {
                id,
                title: self.view.title.clone(),
                workspace: self.workspace.clone(),
                created_at: unix_secs(self.created),
                updated_at: unix_secs(self.touched),
                agent: self.agent,
            },
        )
    }

    /// Records time to first token / first text for the last message.
    pub fn note_timing(&mut self, event: &AgentEvent) {
        let Some(sent) = self.submitted_at else {
            return;
        };
        let elapsed = sent.elapsed();
        match event {
            AgentEvent::ReasoningDelta(_) => {
                self.first_token.get_or_insert(elapsed);
            }
            AgentEvent::TextDelta(_) => {
                self.first_token.get_or_insert(elapsed);
                self.first_text.get_or_insert(elapsed);
            }
            _ => {}
        }
    }

    /// Remembers an edit; stats are settled at the end of the turn.
    pub fn record_edit(&mut self, diff: FileDiff) {
        let turn = self.view.turns.len().saturating_sub(1);
        self.diff_log.push((turn, Arc::new(diff)));
    }

    pub(crate) fn record_event_edit(&mut self, event: &AgentEvent) {
        let event = match event {
            AgentEvent::SubagentEvent { event, .. } => event.as_ref(),
            event => event,
        };
        if let AgentEvent::ToolCallFinished {
            diff: Some(diff), ..
        } = event
        {
            self.record_edit(diff.clone());
        }
    }

    /// At the end of a turn, when the files on disk are final for it, rebuilds
    /// each edited file's content before the turn (and before the session) by
    /// reverse-applying its recorded edits newest-first, and replaces the
    /// summed per-edit stats with one combined diff per file. Reading during
    /// the turn would race the engine's next edit.
    pub fn settle_changes(&mut self) {
        if let Some(snapshot) = self.changes_snapshot() {
            self.apply_settled_changes(snapshot.compute());
        }
    }

    /// Cheap owned input for background work. Patch text is shared, not copied.
    pub fn changes_snapshot(&self) -> Option<ChangesSnapshot> {
        let turn = self.view.turns.len().checked_sub(1)?;
        let mut paths = std::collections::HashSet::new();
        for (t, diff) in &self.diff_log {
            if *t == turn {
                paths.insert(diff.path.clone());
            }
        }
        paths.extend(
            self.view
                .changes
                .iter()
                .filter(|file| file.combined.is_none())
                .map(|file| file.path.clone()),
        );
        if paths.is_empty() {
            return None;
        }
        Some(ChangesSnapshot {
            uid: self.uid,
            workspace: self.workspace.clone(),
            turn,
            revision: self.view.changes_revision,
            turn_stats: self
                .diff_log
                .iter()
                .filter(|(t, diff)| {
                    *t == turn
                        || !self.view.turns[*t]
                            .file_stats
                            .iter()
                            .any(|(path, _, _)| *path == diff.path)
                })
                .map(|(t, diff)| (*t, diff.path.clone()))
                .collect(),
            edits: self
                .diff_log
                .iter()
                .filter(|(_, d)| paths.contains(&d.path))
                .cloned()
                .collect(),
        })
    }

    /// Results never overwrite newer edits, another turn, or another workspace.
    pub fn apply_settled_changes(&mut self, settled: SettledChanges) -> bool {
        if self.uid != settled.uid
            || self.workspace != settled.workspace
            || self.view.running
            || self.view.turns.len().checked_sub(1) != Some(settled.turn)
            || self.view.changes_revision != settled.revision
        {
            return false;
        }
        for file in settled.files {
            for (turn, added, removed) in file.turn_stats {
                self.view
                    .set_turn_file_stats(turn, &file.path, added, removed);
                if let Some(end) = self.view.turns[turn].end {
                    self.list.remeasure_items(end..end + 1);
                }
            }
            if let Some((unified, added, removed)) = file.combined {
                self.view.set_combined(&file.path, unified, added, removed);
            }
        }
        true
    }
}

pub struct ChangesSnapshot {
    uid: u64,
    workspace: PathBuf,
    turn: usize,
    revision: u64,
    turn_stats: std::collections::HashSet<(usize, String)>,
    edits: Vec<(usize, Arc<FileDiff>)>,
}

pub struct SettledChanges {
    uid: u64,
    workspace: PathBuf,
    turn: usize,
    revision: u64,
    files: Vec<SettledFile>,
}

struct SettledFile {
    path: String,
    turn_stats: Vec<(usize, usize, usize)>,
    combined: Option<(String, usize, usize)>,
}

impl ChangesSnapshot {
    /// All filesystem access, patch reversal, and diff generation happens here.
    pub fn compute(self) -> SettledChanges {
        let mut by_path: std::collections::BTreeMap<&str, Vec<(usize, &FileDiff)>> =
            std::collections::BTreeMap::new();
        for (turn, diff) in &self.edits {
            by_path
                .entry(&diff.path)
                .or_default()
                .push((*turn, diff.as_ref()));
        }
        let mut files = Vec::new();
        for (path, edits) in by_path {
            let current = match std::fs::read_to_string(self.workspace.join(path)) {
                Ok(current) => current,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
                // An unreadable file is not evidence of a deletion.
                Err(_) => continue,
            };
            let mut turn_stats = Vec::new();
            let mut after = current.clone();
            let mut end = edits.len();
            // Walk turn boundaries backwards once, using each turn's contents
            // rather than today's file for older summaries.
            while end > 0 {
                let turn = edits[end - 1].0;
                let start = edits[..end].partition_point(|(t, _)| *t < turn);
                let group: Vec<_> = edits[start..end].iter().map(|(_, diff)| *diff).collect();
                let Some(before) = rewind(&after, &group) else {
                    break;
                };
                if self.turn_stats.contains(&(turn, path.to_string())) {
                    let (_, added, removed) = combined(&before, &after, path);
                    turn_stats.push((turn, added, removed));
                }
                after = before;
                end = start;
            }
            let all: Vec<&FileDiff> = edits.iter().map(|(_, diff)| *diff).collect();
            let net = rewind(&current, &all).map(|original| combined(&original, &current, path));
            files.push(SettledFile {
                path: path.to_string(),
                turn_stats,
                combined: net,
            });
        }
        SettledChanges {
            uid: self.uid,
            workspace: self.workspace,
            turn: self.turn,
            revision: self.revision,
            files,
        }
    }
}

/// The content before `edits` (oldest first), by reverse-applying them to
/// `current` newest-first; `None` when a diff doesn't apply.
fn rewind(current: &str, edits: &[&FileDiff]) -> Option<String> {
    let mut content = current.to_string();
    for diff in edits.iter().rev() {
        if diff.created {
            return Some(String::new());
        }
        let patch = diffy::Patch::from_str(&diff.unified).ok()?;
        content = diffy::apply(&content, &patch.reverse()).ok()?;
    }
    Some(content)
}

pub fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Unified diff `original` -> `current` with line stats.
pub fn combined(original: &str, current: &str, path: &str) -> (String, usize, usize) {
    let patch = diffy::create_patch(original, current);
    let (mut added, mut removed) = (0, 0);
    for hunk in patch.hunks() {
        for line in hunk.lines() {
            match line {
                diffy::Line::Insert(_) => added += 1,
                diffy::Line::Delete(_) => removed += 1,
                diffy::Line::Context(_) => {}
            }
        }
    }
    let body = patch.to_string();
    let body = body
        .lines()
        .skip_while(|line| line.starts_with("---") || line.starts_with("+++"))
        .collect::<Vec<_>>()
        .join("\n");
    (
        format!("--- a/{path}\n+++ b/{path}\n{body}\n"),
        added,
        removed,
    )
}

pub fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
