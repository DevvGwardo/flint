//! The session view-model: folds the engine's [`AgentEvent`] stream into
//! transcript items the UI renders. Pure data, no GPUI, so it is unit tested.
//!
//! Time is passed in (`now`, measured from an arbitrary origin) instead of read
//! from the clock, so folding is deterministic.

use std::ops::Range;
use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;

pub use crate::turns::Role;
pub use crate::turns::TurnInfo;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    User(String),
    Assistant {
        text: String,
        streaming: bool,
    },
    Thinking {
        text: String,
        started: Duration,
        /// Set once the model moves on from reasoning.
        duration: Option<Duration>,
        expanded: bool,
    },
    Tool(Box<ToolCall>),
    Nudge {
        reason: NudgeReason,
        message: String,
        expanded: bool,
    },
    /// The engine trimmed old history to fit the context budget.
    Compacted {
        before_tokens: u64,
        after_tokens: u64,
    },
    Repair {
        tool: String,
        detail: String,
    },
    Approval {
        call_id: String,
        kind: ToolKind,
        summary: String,
        decision: Option<ApprovalDecision>,
    },
    Error(String),
    /// An undo restored these files and left the skipped ones alone.
    Reverted {
        restored: Vec<String>,
        skipped: Vec<String>,
    },
    TurnSummary {
        reason: TurnEndReason,
        duration: Duration,
        steps: u32,
        usage: Usage,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub kind: ToolKind,
    pub summary: String,
    pub args: serde_json::Value,
    /// Live output while running; replaced by the final output when finished.
    pub output: String,
    /// Incremental count, so each delta only scans newly received output.
    pub(crate) live_output_newlines: usize,
    /// The agent's own terminal for this command, when it reported one; it
    /// has a read-only tab in the terminal dock.
    pub terminal_id: Option<String>,
    pub subagent: Option<SubagentLink>,
    pub started: Duration,
    pub result: Option<ToolResult>,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentLink {
    pub session_id: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentView {
    pub session_id: String,
    pub model: String,
    pub label: String,
    pub queued: bool,
    pub unread: bool,
    pub view: SessionView,
}

/// Details actually received for a request, never reconstructed from its
/// shortened summary or from a different session's workspace.
pub struct ApprovalPreview {
    pub agent: Option<String>,
    pub fields: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub diff: Option<FileDiff>,
    pub duration_ms: u64,
}

/// One file touched this session, with the latest diff stats.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub created: bool,
    /// Every diff applied to this file, oldest first.
    pub diffs: Vec<String>,
    /// One diff from the file's original content to its current content,
    /// when the original could be reconstructed.
    pub combined: Option<String>,
}

/// Which transcript rows a fold touched, so a virtual list can splice and
/// remeasure exactly those.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    /// Rows appended at the end.
    pub appended: Range<usize>,
    /// Existing rows whose content changed.
    pub updated: Vec<usize>,
    /// Ordered changes to independently virtualized child conversations.
    pub children: Vec<(String, Change)>,
}

impl Change {
    pub(crate) fn updated(ix: usize) -> Self {
        Self {
            appended: 0..0,
            updated: vec![ix],
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionView {
    pub title: Option<String>,
    pub items: Vec<Item>,
    pub running: bool,
    pub turn_id: u64,
    pub step: u32,
    pub turn_started: Option<Duration>,
    pub last_turn_duration: Option<Duration>,
    /// Cumulative usage for the running (or last) turn.
    pub usage: Usage,
    /// Usage summed over every finished turn.
    pub session_usage: Usage,
    /// Characters the running turn has streamed (text, reasoning, tool
    /// arguments), to estimate output tokens before usage arrives. ACP agents
    /// report usage only when the turn ends.
    pub streamed_chars: usize,
    pub changes: Vec<ChangedFile>,
    pub(crate) changes_revision: u64,
    pub pending_approvals: usize,
    /// Every turn so far; see `turns.rs`.
    pub turns: Vec<TurnInfo>,
    /// The turn each item belongs to, parallel to `items`.
    pub turn_of: Vec<Option<usize>>,
    pub current_turn: Option<usize>,
    /// How the last turn ended; `None` before the first one finishes.
    pub last_reason: Option<TurnEndReason>,
    /// An error reported while no turn ran (e.g. the engine failed to
    /// start); cleared when a turn starts. Errors inside a turn that still
    /// completes are notices, not failures.
    pub idle_error: Option<String>,
    /// One conversation per child id, in creation order. Delegation cards
    /// reference these instead of owning a fresh transcript on every resume.
    pub subagents: Vec<SubagentView>,
}

impl SessionView {
    pub fn approval_preview(
        &self,
        call_id: &str,
        workspace: &std::path::Path,
        native: bool,
    ) -> ApprovalPreview {
        let mut agent = None;
        let call = self.items.iter().find_map(|item| match item {
            Item::Tool(call) if call.call_id == call_id => Some(call.as_ref()),
            Item::Tool(call) => call.subagent.as_ref().and_then(|link| {
                let child = self.subagent(&link.session_id)?;
                let found = child.view.items.iter().find_map(|item| match item {
                    Item::Tool(tool) if tool.call_id == call_id => Some(tool.as_ref()),
                    _ => None,
                });
                if found.is_some() {
                    agent = Some(child.session_id.clone());
                }
                found
            }),
            _ => None,
        });
        let mut fields = Vec::new();
        if let Some(call) = call {
            if call.kind == ToolKind::Command {
                if let Some(command) = ["command", "cmd", "script"]
                    .iter()
                    .find_map(|key| call.args.get(*key).and_then(|v| v.as_str()))
                    .or_else(|| call.args.as_str())
                {
                    fields.push(("Command", command.to_string()));
                }
                let cwd = ["cwd", "working_directory"]
                    .iter()
                    .find_map(|key| call.args.get(*key).and_then(|v| v.as_str()));
                if let Some(cwd) = cwd {
                    fields.push(("Working directory", cwd.to_string()));
                } else if native && call.name == "run_command" {
                    fields.push(("Working directory", workspace.display().to_string()));
                }
            } else if call.kind == ToolKind::Edit {
                for (key, label) in [
                    ("path", "Affected file"),
                    ("old_string", "Existing text"),
                    ("new_string", "Replacement text"),
                    ("content", "Proposed content"),
                    ("diff", "Proposed diff"),
                ] {
                    if let Some(text) = call.args.get(key).and_then(|v| v.as_str()) {
                        fields.push((label, text.to_string()));
                    }
                }
            }
            if fields.is_empty()
                && !call.args.is_null()
                && let Ok(arguments) = serde_json::to_string_pretty(&call.args)
            {
                fields.push(("Agent-provided arguments", arguments));
            }
        }
        ApprovalPreview { agent, fields }
    }

    /// Records a message the user sent (the engine does not echo it).
    pub fn push_user(&mut self, text: String) -> Change {
        if self.title.is_none() {
            self.title = Some(title_from(&text));
        }
        self.push(Item::User(text))
    }

    pub fn fold(&mut self, event: AgentEvent, now: Duration) -> Change {
        match event {
            AgentEvent::SteeringAccepted { .. } => Change::default(),
            AgentEvent::TurnStarted { turn_id } => {
                self.turn_id = turn_id;
                self.running = true;
                self.step = 0;
                self.turn_started = Some(now);
                self.usage = Usage::default();
                self.streamed_chars = 0;
                self.idle_error = None;
                self.begin_turn();
                Change::default()
            }
            AgentEvent::StepStarted { step, .. } => {
                self.step = step;
                self.close_streams(now)
            }
            AgentEvent::ReasoningDelta(delta) => {
                self.streamed_chars += delta.chars().count();
                self.reasoning_delta(&delta, now)
            }
            AgentEvent::TextDelta(delta) => {
                self.streamed_chars += delta.chars().count();
                self.text_delta(&delta, now)
            }
            AgentEvent::ToolCallStarted {
                call_id,
                name,
                kind,
                args,
                summary,
            } => {
                self.streamed_chars += serialized_chars(&args);
                let mut change = self.close_streams(now);
                let pushed = self.push(Item::Tool(Box::new(ToolCall {
                    call_id,
                    name,
                    kind,
                    summary,
                    args,
                    output: String::new(),
                    live_output_newlines: 0,
                    terminal_id: None,
                    subagent: None,
                    started: now,
                    result: None,
                    expanded: false,
                })));
                change.appended = pushed.appended;
                change
            }
            AgentEvent::ToolOutputDelta { call_id, chunk } => {
                let Some(ix) = self.tool_index(&call_id) else {
                    return Change::default();
                };
                if let Item::Tool(call) = &mut self.items[ix] {
                    if call.result.is_some() {
                        return Change::default();
                    }
                    call.append_live_output(&chunk);
                }
                Change::updated(ix)
            }
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                exit_code,
                success,
                diff,
                duration_ms,
            } => {
                let Some(ix) = self.tool_index(&call_id) else {
                    return Change::default();
                };
                if let Some(diff) = &diff {
                    self.record_change(diff);
                    self.note_turn_file(diff);
                }
                if let Item::Tool(call) = &mut self.items[ix] {
                    call.output = output;
                    call.live_output_newlines = 0;
                    call.result = Some(ToolResult {
                        exit_code,
                        success,
                        diff,
                        duration_ms,
                    });
                }
                Change::updated(ix)
            }
            AgentEvent::SubagentStarted {
                call_id,
                session_id,
                model,
            } => {
                let Some(ix) = self.tool_index(&call_id) else {
                    return Change::default();
                };
                let Item::Tool(call) = &mut self.items[ix] else {
                    return Change::default();
                };
                if call.subagent.is_some() {
                    return Change::default();
                }
                let message = call.args["message"].as_str().map(str::to_owned);
                let label = call.summary.clone();
                call.subagent = Some(SubagentLink {
                    session_id: session_id.clone(),
                    model: model.clone(),
                });
                let child_ix = self
                    .subagents
                    .iter()
                    .position(|child| child.session_id == session_id);
                let child_ix = child_ix.unwrap_or_else(|| {
                    self.subagents.push(SubagentView {
                        session_id: session_id.clone(),
                        model,
                        label,
                        queued: false,
                        unread: false,
                        view: SessionView::default(),
                    });
                    self.subagents.len() - 1
                });
                let child = &mut self.subagents[child_ix];
                child.queued = true;
                let child_change = message
                    .map(|message| child.view.push_user(message))
                    .unwrap_or_default();
                let mut change = Change::updated(ix);
                change.children.push((session_id, child_change));
                change
            }
            AgentEvent::SubagentEvent { call_id, event } => {
                let Some(ix) = self.tool_index(&call_id) else {
                    return Change::default();
                };
                if let AgentEvent::ToolCallFinished {
                    diff: Some(diff), ..
                } = event.as_ref()
                {
                    self.record_change(diff);
                    self.note_turn_file(diff);
                }
                let mut change = Change::updated(ix);
                if let Item::Tool(call) = &self.items[ix]
                    && let Some(link) = &call.subagent
                    && let Some(child) = self
                        .subagents
                        .iter_mut()
                        .find(|child| child.session_id == link.session_id)
                {
                    if matches!(
                        event.as_ref(),
                        AgentEvent::TurnStarted { .. } | AgentEvent::TurnFinished { .. }
                    ) {
                        child.queued = false;
                    }
                    if matches!(event.as_ref(), AgentEvent::TurnFinished { .. }) {
                        child.unread = true;
                    }
                    change
                        .children
                        .push((child.session_id.clone(), child.view.fold(*event, now)));
                }
                change
            }
            AgentEvent::ApprovalRequested {
                call_id,
                kind,
                summary,
            } => {
                self.pending_approvals += 1;
                self.push(Item::Approval {
                    call_id,
                    kind,
                    summary,
                    decision: None,
                })
            }
            AgentEvent::HarnessNudge { reason, message } => {
                self.note_turn_nudge();
                let mut change = self.close_streams(now);
                change.appended = self
                    .push(Item::Nudge {
                        reason,
                        message,
                        expanded: false,
                    })
                    .appended;
                change
            }
            AgentEvent::ToolRepaired { tool, detail } => self.push(Item::Repair { tool, detail }),
            AgentEvent::Usage(usage) => {
                self.usage = usage;
                Change::default()
            }
            AgentEvent::TurnFinished { reason, .. } => {
                self.last_reason = Some(reason.clone());
                let mut change = self.close_streams(now);
                // Interrupted approvals must not remain actionable.
                for (ix, item) in self.items.iter_mut().enumerate() {
                    if let Item::Approval { decision, .. } = item
                        && decision.is_none()
                    {
                        *decision = Some(ApprovalDecision::Deny);
                        change.updated.push(ix);
                    }
                }
                for child in &mut self.subagents {
                    if child.view.running || child.queued {
                        child.queued = false;
                        child.unread = true;
                        let child_change = child.view.fold(
                            AgentEvent::TurnFinished {
                                turn_id: child.view.turn_id,
                                reason: TurnEndReason::Interrupted,
                            },
                            now,
                        );
                        change
                            .children
                            .push((child.session_id.clone(), child_change));
                    }
                }
                self.pending_approvals = 0;
                let duration = self
                    .turn_started
                    .map(|started| now.saturating_sub(started))
                    .unwrap_or_default();
                self.running = false;
                self.last_turn_duration = Some(duration);
                self.turn_started = None;
                add_usage(&mut self.session_usage, &self.usage);
                change.appended = self
                    .push(Item::TurnSummary {
                        reason,
                        duration,
                        steps: self.step,
                        usage: self.usage,
                    })
                    .appended;
                // The turn's work collapses, so every row in it changes height.
                change.updated.extend(self.end_turn(duration));
                change
            }
            AgentEvent::Error(message) => {
                // A message that never got a turn: the engine failed to start.
                if !self.running
                    && (matches!(self.items.last(), Some(Item::User(_)))
                        || self.idle_error.is_some()
                            && matches!(self.items.last(), Some(Item::Error(_))))
                {
                    self.idle_error = Some(message.clone());
                }
                self.push(Item::Error(message))
            }
            AgentEvent::FilesReverted { diffs, skipped, .. } => {
                for diff in &diffs {
                    self.record_change(diff);
                }
                self.push(Item::Reverted {
                    restored: diffs.into_iter().map(|d| d.path).collect(),
                    skipped,
                })
            }
            AgentEvent::SessionStopped { .. } => Change::default(),
            AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            } => self.push(Item::Compacted {
                before_tokens,
                after_tokens,
            }),
            // Kept on the session (composer chips), not in the transcript.
            // Kept on the session or in terminal tabs, not in the transcript.
            AgentEvent::SessionOptions(_)
            | AgentEvent::TerminalOutput { .. }
            | AgentEvent::TerminalExited { .. } => Change::default(),
            // The agent's command has a terminal of its own; remember which,
            // so its card can bring that tab up.
            AgentEvent::TerminalStarted {
                terminal_id,
                call_id,
                ..
            } => {
                let Some(call_id) = call_id else {
                    return Change::default();
                };
                let Some(ix) = self.tool_index(&call_id) else {
                    return Change::default();
                };
                if let Item::Tool(call) = &mut self.items[ix] {
                    call.terminal_id = Some(terminal_id);
                }
                Change::updated(ix)
            }
        }
    }

    /// Marks an approval card answered; returns its row.
    pub fn resolve_approval(&mut self, call_id: &str, decision: ApprovalDecision) -> Change {
        self.resolve_approval_scoped(call_id, decision, true)
    }

    /// ACP's broad choice does not release its other pending requests; its
    /// permission protocol handles each request separately.
    pub fn resolve_approval_scoped(
        &mut self,
        call_id: &str,
        decision: ApprovalDecision,
        release_pending: bool,
    ) -> Change {
        let found = self.items.iter().position(|item| {
            matches!(item, Item::Approval { call_id: id, decision: None, .. } if id == call_id)
        });
        let Some(ix) = found else {
            return Change::default();
        };
        if let Item::Approval { decision: slot, .. } = &mut self.items[ix] {
            *slot = Some(decision);
        }
        self.pending_approvals = self.pending_approvals.saturating_sub(1);
        let mut change = Change::updated(ix);
        if decision == ApprovalDecision::ApproveAlways && release_pending {
            for (other, item) in self.items.iter_mut().enumerate() {
                if let Item::Approval { decision: slot, .. } = item
                    && slot.is_none()
                {
                    *slot = Some(ApprovalDecision::Approve);
                    change.updated.push(other);
                }
            }
            self.pending_approvals = 0;
        }
        change
    }

    /// Flips a thinking block or tool card open/closed.
    pub fn toggle_expanded(&mut self, ix: usize) -> Change {
        match self.items.get_mut(ix) {
            Some(Item::Thinking { expanded, .. }) => *expanded = !*expanded,
            Some(Item::Tool(call)) => call.expanded = !call.expanded,
            Some(Item::Nudge { expanded, .. }) => *expanded = !*expanded,
            _ => return Change::default(),
        }
        Change::updated(ix)
    }

    /// Output tokens for the running (or last) turn: the reported count, or
    /// an estimate from what has streamed (about 4 characters a token) while
    /// that is higher, as it is until the agent reports usage.
    pub fn output_tokens(&self) -> u64 {
        let reported = self.usage.output_tokens + self.usage.reasoning_tokens;
        let estimated = u64::try_from(self.streamed_chars / 4).unwrap_or(u64::MAX);
        reported.max(estimated)
    }

    /// Seconds the running turn has taken so far, or the last turn's length.
    pub fn elapsed(&self, now: Duration) -> Option<Duration> {
        match self.turn_started {
            Some(started) => Some(now.saturating_sub(started)),
            None => self.last_turn_duration,
        }
    }

    fn push(&mut self, item: Item) -> Change {
        let ix = self.items.len();
        self.items.push(item);
        self.turn_of.push(self.current_turn);
        Change {
            appended: ix..ix + 1,
            updated: Vec::new(),
            children: Vec::new(),
        }
    }

    fn reasoning_delta(&mut self, delta: &str, now: Duration) -> Change {
        let last = self.items.len().checked_sub(1);
        if let Some(ix) = last
            && let Item::Thinking {
                text,
                duration: None,
                ..
            } = &mut self.items[ix]
        {
            text.push_str(delta);
            return Change::updated(ix);
        }
        self.push(Item::Thinking {
            text: delta.to_string(),
            started: now,
            duration: None,
            expanded: false,
        })
    }

    fn text_delta(&mut self, delta: &str, now: Duration) -> Change {
        let last = self.items.len().checked_sub(1);
        if let Some(ix) = last
            && let Item::Assistant {
                text,
                streaming: true,
            } = &mut self.items[ix]
        {
            text.push_str(delta);
            return Change::updated(ix);
        }
        let mut change = self.close_streams(now);
        change.appended = self
            .push(Item::Assistant {
                text: delta.to_string(),
                streaming: true,
            })
            .appended;
        change
    }

    /// Ends the open thinking block and assistant message, if any.
    fn close_streams(&mut self, now: Duration) -> Change {
        let mut change = Change::default();
        let Some(ix) = self.items.len().checked_sub(1) else {
            return change;
        };
        match &mut self.items[ix] {
            Item::Thinking {
                started, duration, ..
            } if duration.is_none() => {
                *duration = Some(now.saturating_sub(*started));
                change.updated.push(ix);
            }
            Item::Assistant { streaming, .. } if *streaming => {
                *streaming = false;
                change.updated.push(ix);
            }
            _ => {}
        }
        change
    }

    fn tool_index(&self, call_id: &str) -> Option<usize> {
        self.items
            .iter()
            .rposition(|item| matches!(item, Item::Tool(call) if call.call_id == call_id))
    }

    fn record_change(&mut self, diff: &FileDiff) {
        self.changes_revision = self.changes_revision.wrapping_add(1);
        if let Some(file) = self.changes.iter_mut().find(|f| f.path == diff.path) {
            file.combined = None;
            file.added += diff.added;
            file.removed += diff.removed;
            file.diffs.push(diff.unified.clone());
        } else {
            self.changes.push(ChangedFile {
                path: diff.path.clone(),
                added: diff.added,
                removed: diff.removed,
                created: diff.created,
                diffs: vec![diff.unified.clone()],
                combined: None,
            });
        }
    }
}

/// Live command output kept per card; older lines are dropped from the front.
pub const MAX_LIVE_OUTPUT_LINES: usize = 2_000;
/// A newline-free stream must not bypass the line-based retention limit.
pub const MAX_LIVE_OUTPUT_BYTES: usize = 256 * 1024;

impl ToolCall {
    fn append_live_output(&mut self, chunk: &str) {
        // Bound before copying: draining after appending a huge delta leaves
        // its entire allocation resident, even when only a small tail survives.
        let mut suffix = &chunk[tail_start(chunk, MAX_LIVE_OUTPUT_BYTES)..];
        let mut newlines = suffix.bytes().filter(|&b| b == b'\n').count();
        if newlines > MAX_LIVE_OUTPUT_LINES {
            let cut = suffix
                .match_indices('\n')
                .nth(newlines - MAX_LIVE_OUTPUT_LINES - 1)
                .expect("counted newline")
                .0
                + 1;
            suffix = &suffix[cut..];
            newlines = MAX_LIVE_OUTPUT_LINES;
        }
        if suffix.len() != chunk.len() {
            self.output.clear();
            self.live_output_newlines = 0;
        } else {
            let mut cut = tail_start(&self.output, MAX_LIVE_OUTPUT_BYTES - suffix.len());
            self.live_output_newlines -= self.output[..cut].bytes().filter(|&b| b == b'\n').count();
            let excess =
                (self.live_output_newlines + newlines).saturating_sub(MAX_LIVE_OUTPUT_LINES);
            if excess > 0 {
                cut += self.output[cut..]
                    .match_indices('\n')
                    .nth(excess - 1)
                    .expect("counted retained newline")
                    .0
                    + 1;
                self.live_output_newlines -= excess;
            }
            self.output.drain(..cut);
        }
        self.output.push_str(suffix);
        self.live_output_newlines += newlines;
    }
}

fn tail_start(text: &str, budget: usize) -> usize {
    let mut cut = text.len().saturating_sub(budget);
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    cut
}

fn serialized_chars(value: &serde_json::Value) -> usize {
    let mut count = CharacterCount(0);
    serde_json::to_writer(&mut count, value).expect("JSON values serialize to a counting writer");
    count.0
}

struct CharacterCount(usize);

impl std::io::Write for CharacterCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        // Each UTF-8 scalar starts with a non-continuation byte. This also
        // works when a writer splits a scalar across separate writes.
        self.0 += if bytes.is_ascii() {
            bytes.len()
        } else {
            bytes.iter().filter(|&&byte| byte & 0xc0 != 0x80).count()
        };
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn add_usage(total: &mut Usage, turn: &Usage) {
    total.input_tokens += turn.input_tokens;
    total.cached_input_tokens += turn.cached_input_tokens;
    total.output_tokens += turn.output_tokens;
    total.reasoning_tokens += turn.reasoning_tokens;
}

/// First line of the first message, trimmed to a sidebar-friendly length.
pub fn title_from(text: &str) -> String {
    let mut line = text.trim_start().chars().take_while(|&ch| ch != '\n');
    let mut title: String = line.by_ref().take(48).collect();
    title.truncate(title.trim_end().len());
    // Trailing whitespace alone does not mean visible text was omitted.
    if line.any(|ch| !ch.is_whitespace()) {
        title.push('…');
    }
    if title.is_empty() {
        "New session".to_string()
    } else {
        title
    }
}

#[cfg(test)]
#[path = "view_model_tests.rs"]
mod tests;
