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
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use gpui_kit::*;

use crate::store;
use crate::store::Logged;
use crate::view_model::Change;
use crate::view_model::Item;
use crate::view_model::SessionView;

pub struct Session {
    /// Stable within this run; event pumps address sessions by it.
    pub uid: u64,
    /// Saved-session directory (`~/.flint/sessions/<id>/`), once it has
    /// sent a message (demo and automation sessions are never saved).
    pub dir: Option<PathBuf>,
    pub view: SessionView,
    pub list: ListState,
    pub subagent_lists: std::collections::HashMap<String, ListState>,
    pub selected_subagent: Option<String>,
    pub subagents_expanded: bool,
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
    /// No project attachment; tools use a private working folder.
    pub general: bool,
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
    pub prompt_queue: crate::prompt_queue::Queue,
    pub queue_scroll: ScrollHandle,
    /// An accepted prompt is waiting for TurnStarted, or a steering ack.
    pub prompt_pending: bool,
    pub steering_pending: Option<u64>,
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
    /// An ACP agent is starting up (its adapter can take 20–50 s).
    Starting,
    Running,
    NeedsApproval,
    /// The last turn failed, hit the step limit, or the agent could not
    /// start. Needs you until `seen`.
    Failed {
        seen: bool,
    },
    /// The user stopped the last turn (or quit while it ran).
    Stopped,
    Unread,
    Done,
}

/// How a status line is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Warning,
    Danger,
}

/// Where a status falls when sessions are grouped or filtered by it; the
/// glyphs still show the finer [`Status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Blocked on an approval, or finished with output not yet read.
    NeedsYou,
    Working,
    /// Finished and already seen.
    Ready,
    /// Nothing has happened yet.
    Inactive,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [
        Bucket::NeedsYou,
        Bucket::Working,
        Bucket::Ready,
        Bucket::Inactive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Bucket::NeedsYou => "Needs you",
            Bucket::Working => "Working",
            Bucket::Ready => "Ready",
            Bucket::Inactive => "Inactive",
        }
    }
}

impl Status {
    pub fn bucket(self) -> Bucket {
        match self {
            Status::NeedsApproval | Status::Unread | Status::Failed { seen: false } => {
                Bucket::NeedsYou
            }
            Status::Running | Status::Starting => Bucket::Working,
            Status::Done | Status::Stopped | Status::Failed { seen: true } => Bucket::Ready,
            Status::Idle => Bucket::Inactive,
        }
    }
}

/// `45s`, `3m 05s`, `1h 12m`: a running turn's elapsed time.
pub fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3_599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3_600, secs % 3_600 / 60),
    }
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
            subagent_lists: Default::default(),
            selected_subagent: None,
            subagents_expanded: true,
            agent: AgentKind::Flint,
            options: Vec::new(),
            agent_ready: false,
            agent_failed: false,
            native_model: None,
            native_approval: None,
            native_allow_all: false,
            workspace,
            general: false,
            created: SystemTime::now(),
            touched: SystemTime::now(),
            unread: false,
            ops: None,
            pump: None,
            submitted_at: None,
            first_token: None,
            first_text: None,
            last_message: None,
            prompt_queue: Default::default(),
            queue_scroll: ScrollHandle::new(),
            prompt_pending: false,
            steering_pending: None,
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
    pub fn apply(&mut self, mut change: Change) {
        for (id, child_change) in std::mem::take(&mut change.children) {
            let list = self.subagent_lists.entry(id).or_insert_with(|| {
                let list = ListState::new(0, ListAlignment::Top, px(1200.));
                list.set_follow_mode(FollowMode::Tail);
                list
            });
            Self::apply_list(list, child_change);
        }
        Self::apply_list(&self.list, change);
    }

    pub(crate) fn apply_list(list: &ListState, mut change: Change) {
        change.updated.sort_unstable();
        change.updated.dedup();
        let mut rows = change.updated.into_iter().peekable();
        while let Some(start) = rows.next() {
            let mut end = start + 1;
            while rows.peek() == Some(&end) {
                rows.next();
                end += 1;
            }
            list.remeasure_items(start..end);
        }
        if !change.appended.is_empty() {
            let at = change.appended.start;
            list.splice(at..at, change.appended.len());
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

    /// What gives the session its colour: stable across restarts for saved
    /// sessions (their folder name), else the run-local uid.
    pub fn color_seed(&self) -> u64 {
        match self.dir.as_ref().and_then(|dir| dir.file_name()) {
            Some(name) => name
                .as_encoded_bytes()
                .iter()
                .fold(0_u64, |acc, byte| acc.rotate_left(5) ^ u64::from(*byte)),
            None => self.uid,
        }
    }

    /// Subagents still working under this session's tool calls.
    pub fn running_subagents(&self) -> usize {
        self.view
            .subagents
            .iter()
            .filter(|child| child.view.running)
            .count()
    }

    /// Tokens used over every finished turn, input and output.
    pub fn total_tokens(&self) -> u64 {
        let usage = &self.view.session_usage;
        usage.input_tokens + usage.output_tokens + usage.reasoning_tokens
    }

    /// The text of the most recent message, either side, for sidebar search.
    pub fn last_message_text(&self) -> Option<&str> {
        self.view.items.iter().rev().find_map(|item| match item {
            Item::User(text) | Item::Assistant { text, .. } if !text.is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
    }

    pub fn status(&self) -> Status {
        if self.view.pending_approvals > 0 {
            Status::NeedsApproval
        } else if self.view.running {
            Status::Running
        } else if self.agent_starting() {
            Status::Starting
        } else if self.failure_text().is_some() {
            Status::Failed { seen: !self.unread }
        } else if self.unread {
            Status::Unread
        } else if self.view.last_reason == Some(TurnEndReason::Interrupted) {
            Status::Stopped
        } else if self.view.turns.is_empty() {
            Status::Idle
        } else {
            Status::Done
        }
    }

    /// Why the session is stuck, when its last turn failed or its agent never
    /// started.
    pub fn failure(&self) -> Option<String> {
        self.failure_text().map(str::to_owned)
    }

    fn failure_text(&self) -> Option<&str> {
        if let Some(error) = &self.view.idle_error {
            return Some(error);
        }
        match &self.view.last_reason {
            Some(TurnEndReason::Failed(error)) => Some(error),
            Some(TurnEndReason::StepLimit) => Some("stopped at the step limit"),
            _ => None,
        }
    }

    /// What the agent is doing right now, for a running session.
    pub fn activity(&self) -> String {
        let subagents = self.running_subagents();
        if subagents > 0 {
            return format!(
                "{subagents} subagent{} working",
                if subagents == 1 { "" } else { "s" }
            );
        }
        let from = self
            .view
            .current_turn
            .and_then(|t| self.view.turns.get(t))
            .map_or(0, |turn| turn.first);
        let items = self.view.items.get(from..).unwrap_or_default();
        // Parallel reads finish out of order: the newest unfinished call wins.
        if let Some(call) = items.iter().rev().find_map(|item| match item {
            Item::Tool(call) if call.result.is_none() => Some(call),
            _ => None,
        }) {
            let what = crate::ui::one_line(&call.summary);
            return match call.kind {
                ToolKind::Command => format!("Running {what}"),
                ToolKind::Edit => format!("Editing {what}"),
                ToolKind::Read => format!("Reading {what}"),
                ToolKind::Search => format!("Searching {what}"),
                ToolKind::Other if call.name == flint_agent::tools::SPAWN_AGENT => {
                    format!("Delegating: {what}")
                }
                ToolKind::Other => format!("Using {}", call.name),
            };
        }
        match items.last() {
            Some(Item::Thinking { duration: None, .. }) => "Thinking…".to_string(),
            Some(Item::Assistant {
                streaming: true, ..
            }) => "Writing…".to_string(),
            Some(Item::Nudge { .. }) => "Checking its work…".to_string(),
            _ => "Working…".to_string(),
        }
    }

    /// How long the running turn has taken, at `now` on the app's clock.
    pub fn running_for(&self, now: Duration) -> Option<Duration> {
        self.view
            .running
            .then_some(self.view.turn_started)
            .flatten()
            .map(|started| now.saturating_sub(started))
    }

    /// The sidebar's second line: what the session is doing or how it ended.
    pub fn status_line(&self, now: Duration) -> (String, Tone) {
        if !self.prompt_queue.items.is_empty()
            && matches!(self.status(), Status::Idle | Status::Done | Status::Stopped)
        {
            return (
                format!(
                    "{} queued · {}",
                    self.prompt_queue.items.len(),
                    if self.prompt_queue.paused {
                        "paused"
                    } else {
                        "up next"
                    }
                ),
                Tone::Muted,
            );
        }
        match self.status() {
            Status::NeedsApproval => (approval_line(&self.view.items), Tone::Warning),
            Status::Starting => (format!("Starting {}…", self.agent.label()), Tone::Muted),
            Status::Running => {
                let mut text = self.activity();
                if let Some(elapsed) = self.running_for(now) {
                    text.push_str(" · ");
                    text.push_str(&clock(elapsed));
                }
                (text, Tone::Muted)
            }
            Status::Failed { .. } => {
                let why = self.failure_text().unwrap_or_default();
                let first = crate::ui::one_line(why.lines().next().unwrap_or_default().trim());
                (format!("Failed: {first}"), Tone::Danger)
            }
            Status::Stopped => match self.last_turn_files() {
                Some(files) => (format!("Stopped · {files}"), Tone::Muted),
                None => ("Stopped".to_string(), Tone::Muted),
            },
            Status::Unread | Status::Done => (
                self.last_turn_files()
                    .unwrap_or_else(|| "Answered".to_string()),
                Tone::Muted,
            ),
            Status::Idle if self.view.items.is_empty() => {
                ("No messages yet".to_string(), Tone::Muted)
            }
            Status::Idle => ("Idle".to_string(), Tone::Muted),
        }
    }

    /// `2 files changed · +10 −3` for the last turn, when it changed files.
    fn last_turn_files(&self) -> Option<String> {
        let turn = self.view.turns.last()?;
        let n = turn.files.len();
        (n > 0).then(|| {
            format!(
                "{n} file{} changed · +{} −{}",
                if n == 1 { "" } else { "s" },
                turn.added,
                turn.removed
            )
        })
    }

    /// Queues an ordered saved event (no-op for unsaved sessions). A flush
    /// barrier is required when the caller needs disk acknowledgment.
    pub(crate) fn log_event(&mut self, event: &AgentEvent) -> std::io::Result<()> {
        // Unsaved sessions discard events, so don't copy their payloads just
        // to drop the owned record. Saved sessions retain the existing writer.
        if self.dir.is_none() {
            return Ok(());
        }
        self.log(Logged::Event(event.clone()))
    }

    /// Queues an owned record, with the same persistence and flush behavior.
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
                general: self.general,
            },
        )
    }

    pub fn workspace_label(&self) -> String {
        if self.general {
            "General agent".into()
        } else {
            folder_name(&self.workspace)
        }
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

fn approval_line(items: &[Item]) -> String {
    let mut waiting = items.iter().filter_map(|item| match item {
        Item::Approval {
            summary,
            decision: None,
            ..
        } => Some(summary.as_str()),
        _ => None,
    });
    let Some(first) = waiting.next() else {
        return "Needs approval".to_string();
    };
    let more = waiting.count();
    let first = crate::ui::one_line(first);
    if more == 0 {
        format!("Approve: {first}")
    } else {
        format!("Approve: {first} (+{more} more)")
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
