//! Per-turn guardrails for the agent loop. Deliberately simple rules:
//!
//! - Stuck: the same tool call (normalized args) three times, or the same
//!   command failing twice in a row with identical output -> a nudge to step
//!   back. A call seen twice becomes a *suspect* the JEV judge may confirm.
//! - Verify before done: files were edited and no command ran after the last
//!   edit when the model tries to finish -> one nudge to run tests/build.
//! - Zero-edit watchdog: the task asks for changes, the model used tools but
//!   edited nothing, and it tries to finish -> one nudge to do the work.
//!
//! The guard also keeps a small, capped record of the turn that the JEV
//! judges read (see the `*_state` methods).

use std::collections::BTreeMap;
use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;

use crate::protocol::NudgeReason;
use crate::protocol::ToolKind;

/// Identical calls before the hard stuck trigger fires.
pub const STUCK_REPEAT_TRIGGER: u32 = 3;

pub const VERIFY_NUDGE: &str = "You modified files during this turn but haven't run any \
    verification commands. Run the relevant tests/build/lint (or explain why they can't be run) \
    and continue instead of ending the turn.";

pub const WATCHDOG_NUDGE: &str = "You inspected files and gathered context, but haven't modified \
    any files or completed the requested changes yet. Implement the solution and verify it \
    before concluding.";

/// The stuck nudge for a repeated call or command.
pub fn stuck_nudge(call: &str) -> String {
    format!(
        "You appear to be looping on `{call}`. Step back, re-read the error or output carefully, \
         and try a different approach instead of repeating the same action."
    )
}

// Caps that keep every JEV state small no matter how large the arguments or
// outputs are (small fixed values).
const JEV_TRAILING_TOOL_CALLS: usize = 8;
const JEV_MAX_ARGS_CHARS: usize = 240;
const JEV_MAX_OUTPUT_CHARS: usize = 200;
const JEV_MAX_EDITED_FILES: usize = 20;
const JEV_MAX_PATH_CHARS: usize = 160;
const JEV_MAX_COMMANDS: usize = 20;
const JEV_MAX_MESSAGE_CHARS: usize = 600;
const JEV_MAX_TASK_CHARS: usize = 1000;

/// Which end-of-turn check applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishCheck {
    Verify,
    Watchdog,
}

#[derive(Debug, Clone)]
struct TraceEntry {
    tool: String,
    args: String,
    exit_code: Option<i32>,
    output: String,
}

#[derive(Debug, Clone)]
struct CommandEntry {
    command: String,
    exit_code: Option<i32>,
}

/// Tracks one user turn.
#[derive(Debug)]
pub struct TurnGuard {
    user_message: String,
    call_counts: HashMap<String, u32>,
    last_failure: HashMap<String, String>,
    failure_streak: HashMap<String, u32>,
    pending_stuck: Option<String>,
    stuck_suspect: Option<String>,
    tool_calls: u32,
    edited_files: bool,
    shell_after_last_edit: bool,
    verify_nudged: bool,
    watchdog_nudged: bool,
    trace: Vec<TraceEntry>,
    edited_paths: Vec<String>,
    commands_after_last_edit: Vec<CommandEntry>,
}

impl TurnGuard {
    pub fn new(user_message: &str) -> Self {
        Self {
            user_message: user_message.to_string(),
            call_counts: HashMap::new(),
            last_failure: HashMap::new(),
            failure_streak: HashMap::new(),
            pending_stuck: None,
            stuck_suspect: None,
            tool_calls: 0,
            edited_files: false,
            shell_after_last_edit: false,
            verify_nudged: false,
            watchdog_nudged: false,
            trace: Vec::new(),
            edited_paths: Vec::new(),
            commands_after_last_edit: Vec::new(),
        }
    }

    /// Records a call before it runs. `path` is the edited file for edits.
    pub fn record_tool_call(
        &mut self,
        name: &str,
        kind: ToolKind,
        args: &Value,
        path: Option<&str>,
    ) {
        self.tool_calls += 1;
        match kind {
            ToolKind::Edit => {
                self.edited_files = true;
                self.shell_after_last_edit = false;
                self.commands_after_last_edit.clear();
                if let Some(path) = path {
                    let path = truncate(path, JEV_MAX_PATH_CHARS);
                    if !self.edited_paths.contains(&path)
                        && self.edited_paths.len() < JEV_MAX_EDITED_FILES
                    {
                        self.edited_paths.push(path);
                    }
                }
            }
            ToolKind::Command => {
                self.shell_after_last_edit = true;
                if self.commands_after_last_edit.len() < JEV_MAX_COMMANDS {
                    self.commands_after_last_edit.push(CommandEntry {
                        command: truncate(&command_of(name, kind, args), JEV_MAX_ARGS_CHARS),
                        exit_code: None,
                    });
                }
            }
            ToolKind::Read | ToolKind::Search | ToolKind::Other => {}
        }
        let key = call_key(name, args);
        self.trace.push(TraceEntry {
            tool: name.to_string(),
            args: truncate(&key, JEV_MAX_ARGS_CHARS),
            exit_code: None,
            output: String::new(),
        });
        if self.trace.len() > JEV_TRAILING_TOOL_CALLS {
            self.trace.remove(0);
        }
        let count = self.call_counts.entry(key.clone()).or_insert(0);
        *count += 1;
        if *count == STUCK_REPEAT_TRIGGER - 1 {
            self.stuck_suspect = Some(key.clone());
        }
        if *count == STUCK_REPEAT_TRIGGER {
            self.pending_stuck = Some(stuck_nudge(&key));
            self.stuck_suspect = None;
        }
    }

    /// Records a finished call.
    pub fn record_tool_result(
        &mut self,
        name: &str,
        kind: ToolKind,
        args: &Value,
        output: &str,
        exit_code: Option<i32>,
        success: bool,
    ) {
        let key = truncate(&call_key(name, args), JEV_MAX_ARGS_CHARS);
        if let Some(entry) = self
            .trace
            .iter_mut()
            .rev()
            .find(|e| e.args == key && e.exit_code.is_none() && e.output.is_empty())
        {
            entry.exit_code = Some(exit_code.unwrap_or(if success { 0 } else { 1 }));
            entry.output = first_meaningful_line(output);
        }
        if kind == ToolKind::Command
            && let Some(last) = self.commands_after_last_edit.last_mut()
            && last.exit_code.is_none()
        {
            last.exit_code = Some(exit_code.unwrap_or(if success { 0 } else { 1 }));
        }
        let command = command_of(name, kind, args);
        if success {
            self.last_failure.remove(&command);
            self.failure_streak.remove(&command);
            // A command that now succeeds may legitimately be re-run.
            if kind == ToolKind::Command {
                self.call_counts.remove(&call_key(name, args));
            }
            return;
        }
        let normalized = output.trim().to_string();
        if self.last_failure.get(&command) == Some(&normalized) {
            let streak = self.failure_streak.entry(command.clone()).or_insert(1);
            *streak += 1;
            if *streak == 2 {
                self.pending_stuck = Some(stuck_nudge(&command));
            }
        } else {
            self.last_failure.insert(command.clone(), normalized);
            self.failure_streak.insert(command, 1);
        }
    }

    /// Nudge to inject before the next model call, if the turn looks stuck.
    pub fn take_stuck_nudge(&mut self) -> Option<String> {
        self.pending_stuck.take()
    }

    /// A loop the heuristic suspects but has not confirmed (for the stall judge).
    pub fn take_stuck_suspect(&mut self) -> Option<String> {
        self.stuck_suspect.take()
    }

    /// The stall judge agreed: raise the same nudge the hard trigger would.
    pub fn confirm_stuck(&mut self, suspect: &str) {
        if self.pending_stuck.is_none() {
            self.pending_stuck = Some(stuck_nudge(suspect));
        }
    }

    /// Which finish check applies, by preconditions only.
    pub fn finish_check(&self) -> Option<FinishCheck> {
        if self.edited_files {
            return (!self.verify_nudged).then_some(FinishCheck::Verify);
        }
        (self.tool_calls > 0 && !self.watchdog_nudged).then_some(FinishCheck::Watchdog)
    }

    /// Marks a finish nudge as sent (each fires at most once per turn).
    pub fn mark_nudged(&mut self, check: FinishCheck) {
        match check {
            FinishCheck::Verify => self.verify_nudged = true,
            FinishCheck::Watchdog => self.watchdog_nudged = true,
        }
    }

    /// The heuristic finish rules: a nudge to keep going, or `None` to finish.
    pub fn before_finish(&mut self) -> Option<(NudgeReason, String)> {
        if self.edited_files && !self.shell_after_last_edit && !self.verify_nudged {
            self.verify_nudged = true;
            return Some((NudgeReason::Verify, VERIFY_NUDGE.to_string()));
        }
        if !self.edited_files
            && self.tool_calls > 0
            && !self.watchdog_nudged
            && task_asks_for_changes(&self.user_message)
        {
            self.watchdog_nudged = true;
            return Some((NudgeReason::Watchdog, WATCHDOG_NUDGE.to_string()));
        }
        None
    }

    /// State for the stall judge.
    pub fn stall_state(&self) -> Value {
        json!({
            "tool_call_count": self.tool_calls,
            "files_edited": self.edited_paths,
            "recent_tool_calls": self.trace_json(),
        })
    }

    /// State for the verify judge.
    pub fn verify_state(&self, final_message: &str) -> Value {
        let commands: Vec<Value> = self
            .commands_after_last_edit
            .iter()
            .map(|c| json!({"command": c.command, "exit_code": c.exit_code}))
            .collect();
        json!({
            "files_edited": self.edited_paths,
            "commands_after_last_edit": commands,
            "final_message": truncate(final_message, JEV_MAX_MESSAGE_CHARS),
        })
    }

    /// State for the watchdog judge.
    pub fn watchdog_state(&self, final_message: &str) -> Value {
        json!({
            "task": truncate(&self.user_message, JEV_MAX_TASK_CHARS),
            "files_edited": self.edited_paths,
            "tool_call_count": self.tool_calls,
            "recent_tool_calls": self.trace_json(),
            "final_message": truncate(final_message, JEV_MAX_MESSAGE_CHARS),
        })
    }

    fn trace_json(&self) -> Vec<Value> {
        self.trace
            .iter()
            .map(|e| json!({"tool": e.tool, "args": e.args, "exit_code": e.exit_code, "output": e.output}))
            .collect()
    }
}

/// Stable key for a call: tool name + args with sorted keys and trimmed strings.
pub fn call_key(name: &str, args: &Value) -> String {
    format!("{name}({})", trim_strings(args))
}

fn trim_strings(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.trim().to_string()),
        Value::Array(items) => Value::Array(items.iter().map(trim_strings).collect()),
        // Sorted explicitly: another crate may enable serde_json's `preserve_order`.
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), trim_strings(v)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn command_of(name: &str, kind: ToolKind, args: &Value) -> String {
    if kind == ToolKind::Command
        && let Some(cmd) = args.get("command").and_then(Value::as_str)
    {
        return cmd.trim().to_string();
    }
    call_key(name, args)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn first_meaningful_line(output: &str) -> String {
    let line = output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("[exit code"))
        .unwrap_or_default();
    truncate(line, JEV_MAX_OUTPUT_CHARS)
}

const CHANGE_VERBS: &[&str] = &[
    "fix",
    "fixes",
    "fixed",
    "fixing",
    "add",
    "adds",
    "added",
    "adding",
    "implement",
    "implements",
    "implemented",
    "implementing",
    "implementation",
    "refactor",
    "refactors",
    "refactored",
    "refactoring",
    "rename",
    "renames",
    "renamed",
    "renaming",
    "update",
    "updates",
    "updated",
    "updating",
    "create",
    "creates",
    "created",
    "creating",
    "bump",
    "bumps",
    "bumped",
    "bumping",
    "write",
    "writes",
    "wrote",
    "writing",
    "modify",
    "modifies",
    "modified",
    "modifying",
    "change",
    "changes",
    "changed",
    "changing",
    "edit",
    "edits",
    "edited",
    "editing",
    "patch",
    "patches",
    "patched",
    "patching",
    "remove",
    "removes",
    "removed",
    "removing",
    "delete",
    "deletes",
    "deleted",
    "deleting",
    "generate",
    "generates",
    "generated",
    "generating",
    "harden",
    "hardens",
    "debug",
    "debugs",
    "debugged",
    "debugging",
    "rewrite",
    "rewrites",
    "rewriting",
    "reorganize",
    "migrate",
    "migrates",
    "migrated",
    "migrating",
    "port",
    "ports",
    "ported",
    "porting",
    "build",
    "builds",
    "make",
    "makes",
];

const QUESTION_STARTERS: &[&str] = &[
    "what ",
    "which ",
    "how ",
    "why ",
    "where ",
    "who ",
    "when ",
    "can you explain",
    "could you explain",
    "explain ",
    "tell me about",
    "is there ",
    "are there ",
    "does ",
    "do ",
    "should ",
];

/// A question that asks for an answer, not a change.
pub fn is_pure_question(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let starts = QUESTION_STARTERS.iter().any(|q| lower.starts_with(q));
    starts && (lower.ends_with('?') || !lower.contains('\n'))
}

/// Whether the task wording asks for code or file changes.
pub fn task_asks_for_changes(text: &str) -> bool {
    if is_pure_question(text) {
        return false;
    }
    text.to_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .any(|word| CHANGE_VERBS.contains(&word))
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
