//! One conversation: its view-model, its virtual-list state, its workspace,
//! its link to the engine (or the demo player), and its saved copy on disk.
//! Sessions run independently, so several can work at once in the background.

use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use flint_agent::AgentEvent;
use flint_agent::AgentKind;
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
    pub last_message: Option<(String, String)>,
    /// Every edit this session, oldest first: (turn index, diff).
    diff_log: Vec<(usize, FileDiff)>,
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
            diff_log: Vec::new(),
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

    /// Appends to the saved event log (no-op for unsaved sessions).
    pub fn log(&self, record: Logged) {
        if let Some(dir) = &self.dir {
            store::append(dir, &record).ok();
        }
    }

    pub fn save_meta(&self) {
        let Some(dir) = &self.dir else {
            return;
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
        .ok();
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
        self.diff_log.push((turn, diff));
    }

    /// At the end of a turn, when the files on disk are final for it, rebuilds
    /// each edited file's content before the turn (and before the session) by
    /// reverse-applying its recorded edits newest-first, and replaces the
    /// summed per-edit stats with one combined diff per file. Reading during
    /// the turn would race the engine's next edit.
    pub fn settle_changes(&mut self) {
        let Some(turn) = self.view.turns.len().checked_sub(1) else {
            return;
        };
        let mut paths: Vec<String> = Vec::new();
        for (t, diff) in &self.diff_log {
            if *t == turn && !paths.contains(&diff.path) {
                paths.push(diff.path.clone());
            }
        }
        for path in paths {
            let current = std::fs::read_to_string(self.workspace.join(&path)).unwrap_or_default();
            let edits: Vec<&(usize, FileDiff)> = self
                .diff_log
                .iter()
                .filter(|(_, d)| d.path == path)
                .collect();
            let turn_edits: Vec<&FileDiff> = edits
                .iter()
                .filter(|(t, _)| *t == turn)
                .map(|(_, d)| d)
                .collect();
            if let Some(before_turn) = rewind(&current, &turn_edits) {
                let (_, added, removed) = combined(&before_turn, &current, &path);
                self.view.set_turn_file_stats(turn, &path, added, removed);
            }
            let all: Vec<&FileDiff> = edits.iter().map(|(_, d)| d).collect();
            if let Some(original) = rewind(&current, &all) {
                let (unified, added, removed) = combined(&original, &current, &path);
                self.view.set_combined(&path, unified, added, removed);
            }
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
