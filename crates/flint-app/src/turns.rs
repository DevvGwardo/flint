//! Turn structure on top of the flat transcript: which rows are the work of a
//! turn (collapsed behind "Worked for …" once it finishes), which row is the
//! final answer, and what the turn changed. Also derives the live activity
//! shown in the status line.

use std::ops::Range;
use std::time::Duration;

use flint_agent::FileDiff;
use flint_agent::ToolKind;

use crate::view_model::Change;
use crate::view_model::Item;
use crate::view_model::SessionView;
use crate::view_model::ToolCall;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnInfo {
    /// First item index belonging to the turn.
    pub first: usize,
    /// The summary row, once the turn has finished.
    pub end: Option<usize>,
    /// The last assistant message, promoted out of the collapsed work.
    pub final_answer: Option<usize>,
    /// The row that carries the "Worked for …" disclosure.
    pub header: Option<usize>,
    /// Whether the finished turn's work is shown.
    pub expanded: bool,
    pub nudges: usize,
    pub duration: Duration,
    /// Files edited in this turn, in first-edit order.
    pub files: Vec<String>,
    pub added: usize,
    pub removed: usize,
    /// Net per-file stats for this turn (original vs. current content), set
    /// by the app once it has read the files; overrides the summed edits.
    pub file_stats: Vec<(String, usize, usize)>,
    /// Thumbs up (true) / down (false), kept locally.
    pub feedback: Option<bool>,
}

/// How a transcript row renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Outside any turn (the user's message, stray errors).
    Plain,
    /// Part of a running turn: the live activity stream.
    Live,
    /// First work row of a finished turn: draws the "Worked for" line, then
    /// the item itself when expanded.
    WorkHeader,
    /// Work of a finished, expanded turn.
    Work,
    /// Work of a finished, collapsed turn: renders nothing.
    Hidden,
    /// The final answer of a finished turn.
    Answer,
    /// The turn's closing row: files-changed card and actions.
    Summary,
}

/// What the agent is doing right now, for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    Thinking,
    Writing,
    Running(String),
    Editing(String),
    Reading(String),
    Searching,
    Working,
}

impl SessionView {
    pub(crate) fn begin_turn(&mut self) {
        self.turns.push(TurnInfo {
            first: self.items.len(),
            ..TurnInfo::default()
        });
        self.current_turn = Some(self.turns.len() - 1);
    }

    pub(crate) fn note_turn_file(&mut self, diff: &FileDiff) {
        let Some(turn) = self.current_turn.and_then(|t| self.turns.get_mut(t)) else {
            return;
        };
        if !turn.files.contains(&diff.path) {
            turn.files.push(diff.path.clone());
        }
        turn.added += diff.added;
        turn.removed += diff.removed;
    }

    pub(crate) fn note_turn_nudge(&mut self) {
        if let Some(turn) = self.current_turn.and_then(|t| self.turns.get_mut(t)) {
            turn.nudges += 1;
        }
    }

    /// Closes the current turn (its summary row was just pushed) and returns
    /// the rows whose rendering changed.
    pub(crate) fn end_turn(&mut self, duration: Duration) -> Range<usize> {
        let Some(t) = self.current_turn.take() else {
            return 0..0;
        };
        let end = self.items.len() - 1;
        let turn = &mut self.turns[t];
        turn.end = Some(end);
        turn.duration = duration;
        turn.final_answer = (turn.first..end)
            .rev()
            .find(|&ix| matches!(self.items[ix], Item::Assistant { .. }));
        turn.header = (turn.first..end).find(|&ix| {
            Some(ix) != turn.final_answer
                && !matches!(self.items[ix], Item::Error(_) | Item::User(_))
        });
        turn.first..end
    }

    pub fn role(&self, ix: usize) -> Role {
        let Some(turn) = self
            .turn_of
            .get(ix)
            .copied()
            .flatten()
            .and_then(|t| self.turns.get(t))
        else {
            return Role::Plain;
        };
        if turn.end.is_none() {
            Role::Live
        } else if matches!(
            self.items.get(ix),
            Some(Item::Error(_) | Item::User(_) | Item::Reverted { .. })
        ) {
            // Failures, messages the user sent mid-turn and undo results
            // stay visible when the work collapses.
            Role::Plain
        } else if turn.end == Some(ix) {
            Role::Summary
        } else if turn.final_answer == Some(ix) {
            Role::Answer
        } else if turn.header == Some(ix) {
            Role::WorkHeader
        } else if turn.expanded {
            Role::Work
        } else {
            Role::Hidden
        }
    }

    /// The turn an item belongs to.
    pub fn turn_at(&self, ix: usize) -> Option<&TurnInfo> {
        self.turn_of
            .get(ix)
            .copied()
            .flatten()
            .and_then(|t| self.turns.get(t))
    }

    /// Opens or closes a finished turn's work.
    pub fn toggle_work(&mut self, ix: usize) -> Change {
        let Some(t) = self.turn_of.get(ix).copied().flatten() else {
            return Change::default();
        };
        let turn = &mut self.turns[t];
        let Some(end) = turn.end else {
            return Change::default();
        };
        turn.expanded = !turn.expanded;
        Change {
            appended: 0..0,
            updated: (turn.first..end).collect(),
            children: Vec::new(),
        }
    }

    /// Replaces a file's stats with its combined diff (original -> current).
    pub fn set_combined(&mut self, path: &str, unified: String, added: usize, removed: usize) {
        if let Some(file) = self.changes.iter_mut().find(|f| f.path == path) {
            self.changes_revision = self.changes_revision.wrapping_add(1);
            file.combined = Some(unified);
            file.added = added;
            file.removed = removed;
        }
    }

    /// Sets one file's net stats for a turn and re-totals the turn.
    pub fn set_turn_file_stats(&mut self, turn: usize, path: &str, added: usize, removed: usize) {
        let Some(turn) = self.turns.get_mut(turn) else {
            return;
        };
        match turn.file_stats.iter_mut().find(|(p, _, _)| p == path) {
            Some(entry) => *entry = (path.to_string(), added, removed),
            None => turn.file_stats.push((path.to_string(), added, removed)),
        }
        turn.added = turn.file_stats.iter().map(|(_, a, _)| a).sum();
        turn.removed = turn.file_stats.iter().map(|(_, _, r)| r).sum();
    }

    /// The oldest approval still waiting: (call id, kind, summary).
    pub fn pending_approval(&self) -> Option<(String, ToolKind, String)> {
        self.pending_approval_ref()
            .map(|(call_id, kind, summary)| (call_id.to_owned(), kind, summary.to_owned()))
    }

    pub(crate) fn pending_approval_ref(&self) -> Option<(&str, ToolKind, &str)> {
        self.items.iter().find_map(|item| match item {
            Item::Approval {
                call_id,
                kind,
                summary,
                decision: None,
            } => Some((call_id.as_str(), *kind, summary.as_str())),
            _ => None,
        })
    }

    pub(crate) fn has_pending_approval(&self) -> bool {
        self.pending_approval_ref().is_some()
    }

    pub fn set_feedback(&mut self, ix: usize, positive: bool) -> Change {
        let Some(t) = self.turn_of.get(ix).copied().flatten() else {
            return Change::default();
        };
        let turn = &mut self.turns[t];
        turn.feedback = if turn.feedback == Some(positive) {
            None
        } else {
            Some(positive)
        };
        Change::updated(ix)
    }

    /// Count and borrowed commands for the composer's task tray, oldest first.
    pub(crate) fn running_command_projection(&self) -> (usize, impl Iterator<Item = &ToolCall>) {
        let mut commands = self.running_commands_iter();
        // Small trays need one scan even when completed tools precede them.
        let inline = std::array::from_fn::<_, 8, _>(|_| commands.next());
        let count = inline.iter().flatten().count() + commands.clone().count();
        (count, inline.into_iter().flatten().chain(commands))
    }

    /// Commands still running, oldest first (the composer's task tray).
    pub fn running_commands(&self) -> Vec<&ToolCall> {
        self.running_commands_iter().collect()
    }

    fn running_commands_iter(&self) -> impl Iterator<Item = &ToolCall> + Clone {
        let items = self
            .current_turn
            .and_then(|ix| self.turns.get(ix))
            .map_or(&[][..], |turn| &self.items[turn.first..]);
        items.iter().filter_map(|item| match item {
            Item::Tool(call) if call.kind == ToolKind::Command && call.result.is_none() => {
                Some(call.as_ref())
            }
            _ => None,
        })
    }

    /// What the running turn is doing, read from its latest row.
    pub fn activity(&self) -> Activity {
        match self.items.last() {
            Some(Item::Thinking { duration: None, .. }) => Activity::Thinking,
            Some(Item::Assistant {
                streaming: true, ..
            }) => Activity::Writing,
            Some(Item::Tool(call)) if call.result.is_none() => match call.kind {
                ToolKind::Command => Activity::Running(crate::ui::one_line(&call.summary)),
                ToolKind::Edit => Activity::Editing(crate::ui::one_line(&call.summary)),
                ToolKind::Read => Activity::Reading(crate::ui::one_line(&call.summary)),
                ToolKind::Search => Activity::Searching,
                ToolKind::Other => Activity::Working,
            },
            _ => Activity::Working,
        }
    }
}

/// Characters of a command's output quoted back to the agent.
const SENT_CHARS: usize = 8_000;
/// Terminal lines quoted back to the agent (the most recent ones).
const SENT_TERMINAL_LINES: usize = 200;

/// What a command card's "Send to agent" says and sends: the command's
/// output, bounded, in a fenced block.
pub fn command_message(call: &ToolCall) -> (String, String) {
    let shown = format!("Here's the output of `{}`:", call.summary);
    let body = flint_agent::tools::head_tail(call.output.trim_end(), SENT_CHARS);
    (shown.clone(), format!("{shown}\n\n```\n{body}\n```"))
}

/// What a terminal tab's "Send to agent" says and sends: the last lines of
/// the terminal's buffer.
pub fn terminal_message(label: &str, text: &str) -> (String, String) {
    let shown = format!("Here's what my terminal ({label}) shows:");
    let mut lines: Vec<&str> = text.lines().rev().take(SENT_TERMINAL_LINES).collect();
    lines.reverse();
    let tail = lines.join("\n");
    let body = flint_agent::tools::head_tail(tail.trim_end(), SENT_CHARS);
    (shown.clone(), format!("{shown}\n\n```\n{body}\n```"))
}

#[cfg(test)]
#[path = "turns_tests.rs"]
mod tests;
