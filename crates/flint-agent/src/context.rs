//! Keeps the model context within a token budget.
//!
//! Size is estimated as chars/4, scaled by how the provider's reported
//! prompt tokens compared to that estimate on the last call. When the
//! estimate passes [`TRIGGER`] of the budget, old turns are compacted in one
//! large chunk down to [`TARGET`], oldest first, so the history (and the
//! provider's prompt cache) changes as rarely as possible:
//!
//! 1. In turns before the last two: tool outputs become short stubs, old
//!    reasoning is dropped, and long tool-call arguments (e.g. whole files
//!    passed to `write_file`) are shortened. User messages and assistant text
//!    stay intact.
//! 2. Still over the trigger: the oldest whole turns are dropped.
//! 3. Still over the target: the older protected turn is compacted too, and
//!    then the current turn's tool outputs except the latest few, so the
//!    next compaction is several steps away.

use serde_json::Value;

use crate::provider::Message;

/// Compact when the estimate passes this share of the budget.
pub const TRIGGER: f64 = 0.8;
/// Compact down to this share of the budget.
pub const TARGET: f64 = 0.5;
/// Turns kept intact.
const PROTECTED_TURNS: usize = 2;
/// Tool outputs shorter than this are left alone.
const STUB_MIN_CHARS: usize = 400;
/// Tool-call arguments longer than this get their long strings shortened.
const ARGS_MAX_CHARS: usize = 600;
/// Latest tool outputs of the current turn never stubbed, even in phase 3.
const KEEP_RECENT_TOOL_OUTPUTS: usize = 4;
/// Per-message overhead in characters (role, ids, JSON punctuation).
const MESSAGE_OVERHEAD_CHARS: usize = 16;

/// Estimates request size and decides when to compact.
#[derive(Debug, Clone)]
pub struct ContextTracker {
    budget: u64,
    /// Reported prompt tokens / chars-based estimate, from the last call.
    ratio: f64,
    /// Characters of the fixed parts of every request (tool specs).
    fixed_chars: usize,
}

impl ContextTracker {
    pub fn new(budget: u64, fixed_chars: usize) -> Self {
        Self {
            budget,
            ratio: 1.0,
            fixed_chars,
        }
    }

    /// Replaces the budget, e.g. once the model's context window is known.
    pub fn set_budget(&mut self, budget: u64) {
        self.budget = budget;
    }

    /// Estimated prompt tokens for `history`.
    pub fn estimate(&self, history: &[Message]) -> u64 {
        (self.raw_estimate(history) as f64 * self.ratio).round() as u64
    }

    fn raw_estimate(&self, history: &[Message]) -> u64 {
        let chars: usize = self.fixed_chars + history.iter().map(message_chars).sum::<usize>();
        (chars / 4) as u64
    }

    /// Calibrates against the prompt tokens the provider reported for a
    /// request built from `history`.
    pub fn observe(&mut self, history: &[Message], reported_prompt_tokens: u64) {
        let raw = self.raw_estimate(history);
        if raw > 0 && reported_prompt_tokens > 0 {
            self.ratio = (reported_prompt_tokens as f64 / raw as f64).clamp(0.5, 2.0);
        }
    }

    /// Compacts `history` when it is over the trigger. Returns the estimates
    /// before and after when something changed.
    pub fn maybe_compact(&self, history: &mut Vec<Message>) -> Option<(u64, u64)> {
        let before = self.estimate(history);
        if (before as f64) < self.budget as f64 * TRIGGER {
            return None;
        }
        let target = (self.budget as f64 * TARGET) as u64;
        let trigger = (self.budget as f64 * TRIGGER) as u64;
        let mut changed = false;

        // Phase 1: stub old turns, oldest first.
        let starts = turn_starts(history);
        let protected_from = protected_start(&starts, history.len());
        for (i, &start) in starts.iter().enumerate() {
            if start >= protected_from || self.estimate(history) <= target {
                break;
            }
            let end = starts
                .get(i + 1)
                .copied()
                .unwrap_or(history.len())
                .min(protected_from);
            for message in &mut history[start..end] {
                changed |= compact_message(message);
            }
        }

        // Phase 2: drop the oldest whole turns.
        while self.estimate(history) > trigger {
            let starts = turn_starts(history);
            let protected_from = protected_start(&starts, history.len());
            let (Some(&first), Some(&second)) = (starts.first(), starts.get(1)) else {
                break;
            };
            if second > protected_from || first >= protected_from {
                break;
            }
            history.drain(first..second);
            changed = true;
        }

        // Phase 3: when the protected turns alone keep us above the target,
        // compact them too: the older protected turn fully, then the current
        // turn's tool outputs except the latest few. Otherwise every
        // following step would re-trigger compaction and break the provider
        // cache each time.
        if self.estimate(history) > target {
            let starts = turn_starts(history);
            let current = starts.last().copied().unwrap_or(0);
            let protected_from = protected_start(&starts, history.len()).min(current);
            for message in &mut history[protected_from..current] {
                changed |= compact_message(message);
            }
            let current_tools: Vec<usize> = (current..history.len())
                .filter(|&i| matches!(history[i], Message::Tool { .. }))
                .collect();
            let stub_until = current_tools.len().saturating_sub(KEEP_RECENT_TOOL_OUTPUTS);
            for &i in &current_tools[..stub_until] {
                if self.estimate(history) <= target {
                    break;
                }
                changed |= compact_message(&mut history[i]);
            }
        }

        changed.then(|| (before, self.estimate(history)))
    }
}

/// Indices of real user messages (turn starts); the system prompt and
/// nudges are not turns.
fn turn_starts(history: &[Message]) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m, Message::User(_)))
        .map(|(i, _)| i)
        .collect()
}

/// First index of the protected (most recent) turns.
fn protected_start(starts: &[usize], len: usize) -> usize {
    starts
        .len()
        .checked_sub(PROTECTED_TURNS)
        .map_or(len.min(starts.first().copied().unwrap_or(len)), |i| {
            starts[i]
        })
}

/// Shrinks one message in place; returns whether it changed.
fn compact_message(message: &mut Message) -> bool {
    match message {
        Message::Tool { content, .. } if content.chars().count() > STUB_MIN_CHARS => {
            *content = stub_for(content);
            true
        }
        Message::Assistant {
            reasoning,
            tool_calls,
            ..
        } => {
            let mut changed = !reasoning.is_empty();
            reasoning.clear();
            for call in tool_calls {
                if call.arguments.len() > ARGS_MAX_CHARS
                    && let Ok(Value::Object(mut args)) =
                        serde_json::from_str::<Value>(&call.arguments)
                {
                    for value in args.values_mut() {
                        if let Value::String(text) = value
                            && text.chars().count() > 200
                        {
                            *text = format!("[trimmed: {} chars]", kchars(text.chars().count()));
                        }
                    }
                    call.arguments = Value::Object(args).to_string();
                    changed = true;
                }
            }
            changed
        }
        Message::System(_) | Message::User(_) | Message::Nudge(_) | Message::Tool { .. } => false,
    }
}

/// `[output trimmed: 4.2k chars, exit 0]`.
fn stub_for(content: &str) -> String {
    let size = kchars(content.chars().count());
    let exit = content
        .trim_end()
        .rsplit('\n')
        .next()
        .and_then(|line| line.strip_prefix("[exit code: "))
        .and_then(|rest| rest.strip_suffix(']'));
    let status = if content.starts_with("Error:") {
        ", error"
    } else {
        ""
    };
    match exit {
        Some(code) => format!("[output trimmed: {size} chars, exit {code}]"),
        None => format!("[output trimmed: {size} chars{status}]"),
    }
}

fn kchars(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn message_chars(message: &Message) -> usize {
    MESSAGE_OVERHEAD_CHARS
        + match message {
            Message::System(text) | Message::User(text) | Message::Nudge(text) => text.len(),
            // Reasoning is replayed on tool-call messages (see session.rs).
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                content.len()
                    + if tool_calls.is_empty() {
                        0
                    } else {
                        reasoning.len()
                    }
                    + tool_calls
                        .iter()
                        .map(|c| {
                            c.id.len() + c.name.len() + c.arguments.len() + MESSAGE_OVERHEAD_CHARS
                        })
                        .sum::<usize>()
            }
            Message::Tool { call_id, content } => call_id.len() + content.len(),
        }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
