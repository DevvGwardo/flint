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
    pub started: Duration,
    pub result: Option<ToolResult>,
    pub expanded: bool,
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
}

impl Change {
    pub(crate) fn updated(ix: usize) -> Self {
        Self {
            appended: 0..0,
            updated: vec![ix],
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
    pub changes: Vec<ChangedFile>,
    pub pending_approvals: usize,
    /// Every turn so far; see `turns.rs`.
    pub turns: Vec<TurnInfo>,
    /// The turn each item belongs to, parallel to `items`.
    pub turn_of: Vec<Option<usize>>,
    pub current_turn: Option<usize>,
}

impl SessionView {
    /// Records a message the user sent (the engine does not echo it).
    pub fn push_user(&mut self, text: String) -> Change {
        if self.title.is_none() {
            self.title = Some(title_from(&text));
        }
        self.push(Item::User(text))
    }

    pub fn fold(&mut self, event: AgentEvent, now: Duration) -> Change {
        match event {
            AgentEvent::TurnStarted { turn_id } => {
                self.turn_id = turn_id;
                self.running = true;
                self.step = 0;
                self.turn_started = Some(now);
                self.usage = Usage::default();
                self.begin_turn();
                Change::default()
            }
            AgentEvent::StepStarted { step, .. } => {
                self.step = step;
                self.close_streams(now)
            }
            AgentEvent::ReasoningDelta(delta) => self.reasoning_delta(&delta, now),
            AgentEvent::TextDelta(delta) => self.text_delta(&delta, now),
            AgentEvent::ToolCallStarted {
                call_id,
                name,
                kind,
                args,
                summary,
            } => {
                let mut change = self.close_streams(now);
                let pushed = self.push(Item::Tool(Box::new(ToolCall {
                    call_id,
                    name,
                    kind,
                    summary,
                    args,
                    output: String::new(),
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
                    call.output.push_str(&chunk);
                    keep_tail_lines(&mut call.output, MAX_LIVE_OUTPUT_LINES);
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
                    call.result = Some(ToolResult {
                        exit_code,
                        success,
                        diff,
                        duration_ms,
                    });
                }
                Change::updated(ix)
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
                let mut change = self.close_streams(now);
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
            AgentEvent::Error(message) => self.push(Item::Error(message)),
            AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            } => self.push(Item::Compacted {
                before_tokens,
                after_tokens,
            }),
            // Kept on the session (composer chips), not in the transcript.
            AgentEvent::SessionOptions(_) => Change::default(),
        }
    }

    /// Marks an approval card answered; returns its row.
    pub fn resolve_approval(&mut self, call_id: &str, decision: ApprovalDecision) -> Change {
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
        Change::updated(ix)
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
        if let Some(file) = self.changes.iter_mut().find(|f| f.path == diff.path) {
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

/// Trims `output` to its last `max_lines` lines. Cheap for short output: the
/// newline count is only taken once the text could exceed the limit.
fn keep_tail_lines(output: &mut String, max_lines: usize) {
    if output.len() <= max_lines * 8 {
        return;
    }
    let newlines = output.matches('\n').count();
    if newlines <= max_lines {
        return;
    }
    let skip = newlines - max_lines;
    if let Some((cut, _)) = output.match_indices('\n').nth(skip - 1) {
        output.drain(..=cut);
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
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    let mut title: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        title = format!("{}…", title.trim_end());
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
