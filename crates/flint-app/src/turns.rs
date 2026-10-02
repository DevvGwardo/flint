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
        turn.header = (turn.first..end)
            .find(|&ix| Some(ix) != turn.final_answer && !matches!(self.items[ix], Item::Error(_)));
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
        } else if matches!(self.items.get(ix), Some(Item::Error(_))) {
            // Failures stay visible when the work collapses.
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
        }
    }

    /// Replaces a file's stats with its combined diff (original -> current).
    pub fn set_combined(&mut self, path: &str, unified: String, added: usize, removed: usize) {
        if let Some(file) = self.changes.iter_mut().find(|f| f.path == path) {
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
        self.items.iter().find_map(|item| match item {
            Item::Approval {
                call_id,
                kind,
                summary,
                decision: None,
            } => Some((call_id.clone(), *kind, summary.clone())),
            _ => None,
        })
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

    /// Commands still running, oldest first (the composer's task tray).
    pub fn running_commands(&self) -> Vec<&ToolCall> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Tool(call) if call.kind == ToolKind::Command && call.result.is_none() => {
                    Some(call.as_ref())
                }
                _ => None,
            })
            .collect()
    }

    /// What the running turn is doing, read from its latest row.
    pub fn activity(&self) -> Activity {
        match self.items.last() {
            Some(Item::Thinking { duration: None, .. }) => Activity::Thinking,
            Some(Item::Assistant {
                streaming: true, ..
            }) => Activity::Writing,
            Some(Item::Tool(call)) if call.result.is_none() => match call.kind {
                ToolKind::Command => Activity::Running(call.summary.clone()),
                ToolKind::Edit => Activity::Editing(call.summary.clone()),
                ToolKind::Read => Activity::Reading(call.summary.clone()),
                ToolKind::Search => Activity::Searching,
                ToolKind::Other => Activity::Working,
            },
            _ => Activity::Working,
        }
    }
}

#[cfg(test)]
#[path = "turns_tests.rs"]
mod tests;
