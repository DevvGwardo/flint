//! Per-turn guardrails for the agent loop. Deliberately simple rules:
//!
//! - Stuck: the same tool call (normalized args) three times, the same
//!   command failing twice in a row with identical output, two different
//!   commands in a row failing with identical output, or a file re-read over
//!   lines already read three times without editing it -> a nudge to step
//!   back. A call seen twice becomes a *suspect* the JEV judge may confirm.
//! - Verify before done: files were edited and no real check (a command
//!   other than ls/cat/grep/git status and the like) ran after the last edit
//!   when the model tries to finish -> one nudge to run tests/build. If the
//!   last check after the edits failed -> one nudge to fix it or say why not.
//! - Zero-edit watchdog: the task asks for changes, the model used tools but
//!   edited nothing, and it tries to finish -> one nudge to do the work.
//! - Open plan: the model's `update_plan` still has open steps when it tries
//!   to finish -> one nudge to finish or drop them.
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
pub type FinishProgress = (usize, u64, usize, usize);

pub const VERIFY_NUDGE: &str = "The changes or deliverables in this turn haven't been verified \
    after the latest work. Run the relevant tests/build/lint (or explain why they can't be run) \
    directly, without output-filtering pipes or exit-masking wrappers. The command tool already \
    captures bounded output. Do not make further edits unless the check requires them; \
    verify the final state and report the actual result.";

pub const WATCHDOG_NUDGE: &str = "You inspected files and gathered context, but haven't modified \
    any files or completed the requested changes yet. Implement the solution and verify it \
    before concluding.";

/// The nudge when the last check after the edits failed.
pub fn failed_check_nudge(command: &str, exit_code: Option<i32>) -> String {
    let code = exit_code.map_or_else(|| "failed".to_string(), |c| format!("exit {c}"));
    format!(
        "The last check after your edits failed: `{command}` ({code}). Fix the cause and run it \
         again. If the failure is unrelated to your change, say so in your final message with the \
         evidence instead of claiming success."
    )
}

/// The nudge when the plan still has open steps.
pub fn open_plan_nudge(open: &[String]) -> String {
    let mut steps: Vec<String> = open.iter().take(5).map(|s| format!("- {s}")).collect();
    if open.len() > 5 {
        steps.push(format!("- … and {} more", open.len() - 5));
    }
    format!(
        "Your plan still has open steps:\n{}\nFinish them, or call update_plan to mark them \
         dropped and say why in your final message.",
        steps.join("\n")
    )
}

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
    contract: Option<crate::task::TaskContract>,
    task: Option<crate::task::TaskRecord>,
    call_counts: HashMap<String, u32>,
    last_failure: HashMap<String, String>,
    failure_streak: HashMap<String, u32>,
    pending_stuck: Option<String>,
    stuck_suspect: Option<String>,
    tool_calls: u32,
    edited_files: bool,
    /// A successful shell/MCP operation may change work outside the file tools.
    possible_changes: bool,
    /// A real check (not just inspection) ran after the last edit.
    check_after_last_edit: bool,
    /// The latest real check, when it failed: command and exit code.
    failed_checks: BTreeMap<String, Option<i32>>,
    /// Output of the latest failed command, of any command.
    last_failed_output: Option<String>,
    /// Line ranges read per file since that file was last edited.
    read_ranges: HashMap<String, Vec<(u64, u64)>>,
    /// Reads per file that overlapped an earlier read.
    rereads: HashMap<String, u32>,
    open_plan: Vec<String>,
    verify_nudged: bool,
    /// Recovery activity survives later invalidation. It never certifies work.
    verification_progress: u64,
    failed_check_nudged: bool,
    watchdog_nudged: bool,
    plan_nudged: bool,
    trace: Vec<TraceEntry>,
    edited_paths: Vec<String>,
    commands_after_last_edit: Vec<CommandEntry>,
}

impl TurnGuard {
    pub fn new(user_message: &str) -> Self {
        Self {
            user_message: user_message.to_string(),
            contract: None,
            task: None,
            call_counts: HashMap::new(),
            last_failure: HashMap::new(),
            failure_streak: HashMap::new(),
            pending_stuck: None,
            stuck_suspect: None,
            tool_calls: 0,
            edited_files: false,
            possible_changes: false,
            check_after_last_edit: false,
            failed_checks: BTreeMap::new(),
            last_failed_output: None,
            read_ranges: HashMap::new(),
            rereads: HashMap::new(),
            open_plan: Vec::new(),
            verify_nudged: false,
            verification_progress: 0,
            failed_check_nudged: false,
            watchdog_nudged: false,
            plan_nudged: false,
            trace: Vec::new(),
            edited_paths: Vec::new(),
            commands_after_last_edit: Vec::new(),
        }
    }

    /// Records a call attempt before it runs, without claiming an edit happened.
    pub fn record_tool_call(
        &mut self,
        name: &str,
        kind: ToolKind,
        args: &Value,
        _path: Option<&str>,
    ) {
        self.tool_calls += 1;
        match kind {
            ToolKind::Edit => {}
            ToolKind::Command => {
                if self.commands_after_last_edit.len() == JEV_MAX_COMMANDS {
                    self.commands_after_last_edit.remove(0);
                }
                self.commands_after_last_edit.push(CommandEntry {
                    command: truncate(&command_of(name, kind, args), JEV_MAX_ARGS_CHARS),
                    exit_code: None,
                });
            }
            ToolKind::Read if name == "read_file" => self.record_read(args),
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

    /// Records only a completed, successful mutation.
    fn record_edit(&mut self, path: Option<&str>) {
        self.edited_files = true;
        self.check_after_last_edit = false;
        self.verify_nudged = false;
        self.failed_check_nudged = false;
        self.commands_after_last_edit.clear();
        if let Some(path) = path {
            let key = path_key(path);
            self.read_ranges.remove(&key);
            self.rereads.remove(&key);
            // Reading a file back after editing it is not a repeat.
            let quoted = [format!("\"{key}\""), format!("\"./{key}\"")];
            self.call_counts.retain(|call, _| {
                !(call.starts_with("read_file(") && quoted.iter().any(|q| call.contains(q)))
            });
            let path = truncate(path, JEV_MAX_PATH_CHARS);
            if !self.edited_paths.contains(&path) && self.edited_paths.len() < JEV_MAX_EDITED_FILES
            {
                self.edited_paths.push(path);
            }
        }
    }

    /// Counts a read that covers lines this turn already read from the same
    /// file (without editing it in between).
    fn record_read(&mut self, args: &Value) {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return;
        };
        let number = |key: &str| {
            args.get(key).and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
        };
        let start = number("offset").unwrap_or(1).max(1);
        let end = start.saturating_add(number("limit").unwrap_or(READ_DEFAULT_LINES).max(1) - 1);
        let key = path_key(path);
        let ranges = self.read_ranges.entry(key.clone()).or_default();
        let overlaps = ranges.iter().any(|&(s, e)| start <= e && s <= end);
        ranges.push((start, end));
        if !overlaps {
            return;
        }
        let count = self.rereads.entry(key.clone()).or_insert(0);
        *count += 1;
        let call = format!("read_file {key}");
        if *count == REREAD_TRIGGER - 1 && self.pending_stuck.is_none() {
            self.stuck_suspect = Some(call);
        } else if count.is_multiple_of(REREAD_TRIGGER) {
            self.pending_stuck = Some(stuck_nudge(&call));
            self.stuck_suspect = None;
        }
    }

    /// Folds a message the user sent mid-turn into the task the finish rules
    /// read.
    pub fn add_user_message(&mut self, text: &str) {
        self.user_message.push('\n');
        self.user_message.push_str(text);
    }

    pub fn set_task_contract(&mut self, contract: &crate::task::TaskContract) {
        self.contract = Some(contract.clone());
    }

    /// Share the host's revision-aware evidence with the guard and JEV instead
    /// of maintaining a contradictory, tool-name-only notion of completion.
    pub fn set_task_record(&mut self, task: &crate::task::TaskRecord) {
        self.verification_progress = self.verification_progress.max(
            task.checks
                .iter()
                .filter(|check| {
                    check.verification
                        && check.success
                        && check.contract_revision == task.contract.revision
                        && check.started_revision == check.work_revision
                })
                .map(|check| check.id)
                .max()
                .unwrap_or(0),
        );
        if self.task.as_ref().is_some_and(|previous| {
            previous.work_revision != task.work_revision
                || previous.contract.revision != task.contract.revision
        }) {
            self.verify_nudged = false;
            self.failed_check_nudged = false;
        }
        self.set_task_contract(&task.contract);
        self.edited_files |= task.observed_edits > 0;
        self.check_after_last_edit = task.has_current_verification();
        self.failed_checks = task
            .checks
            .iter()
            .filter(|check| task.failed_verification_is_unresolved(check))
            .map(|check| (check.command.clone(), check.exit_code))
            .collect();
        // Restored/continued plans are host evidence too, even when this
        // turn has not issued a new update_plan call.
        self.open_plan = task
            .plan
            .iter()
            .filter(|step| step.status.is_open())
            .map(|step| step.step.clone())
            .collect();
        self.task = Some(task.clone());
    }

    pub fn analysis_scope_nudge(&self, final_text: &str) -> Option<String> {
        let first = final_text.lines().next().unwrap_or(final_text);
        let lower = first.trim_start().to_ascii_lowercase();
        let claims_blocked = lower.starts_with("blocked:")
            || [
                "that's the blocker",
                "that is the blocker",
                "cannot complete",
                "can't complete",
                "cannot finish",
                "can't finish",
                "unable to finish",
            ]
            .iter()
            .any(|phrase| lower.contains(phrase));
        self.task
            .as_ref()
            .filter(|task| claims_blocked && task.optional_scope_blocker(first))
            .map(|_| crate::task::TaskRecord::analysis_guidance().to_string())
    }

    /// Records the model's latest plan: the steps still open.
    pub fn record_plan(&mut self, open: Vec<String>) {
        self.open_plan = open;
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
        if kind == ToolKind::Edit && success {
            self.record_edit(args.get("path").and_then(Value::as_str));
        }
        if success && kind != ToolKind::Edit && crate::task::may_mutate(name, kind, args) {
            self.possible_changes = true;
            self.check_after_last_edit = false;
            self.verify_nudged = false;
            self.failed_check_nudged = false;
            // Keep the completing command, not checks from before its effects.
            let current = if kind == ToolKind::Command {
                self.commands_after_last_edit.pop()
            } else {
                None
            };
            self.commands_after_last_edit.clear();
            self.commands_after_last_edit.extend(current);
        }
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
        let verifies = crate::task::is_verification(&command)
            || self.contract.as_ref().is_some_and(|contract| {
                contract.requirements.iter().any(|requirement| {
                    matches!(&requirement.spec, crate::task::RequirementSpec::Command(required) if required.trim() == command.trim())
                })
            });
        if kind == ToolKind::Command && verifies {
            self.check_after_last_edit = true;
            if success {
                self.failed_checks.remove(&command);
            } else {
                self.failed_checks.insert(command.clone(), exit_code);
            }
        }
        if success {
            self.last_failed_output = None;
            self.last_failure.remove(&command);
            self.failure_streak.remove(&command);
            // A command that now succeeds may legitimately be re-run.
            if kind == ToolKind::Command {
                self.call_counts.remove(&call_key(name, args));
            }
            return;
        }
        let normalized = output.trim().to_string();
        // A different command (or a tweaked one) failing exactly the same way
        // in a row is the same loop.
        if !normalized.is_empty()
            && self.last_failed_output.as_ref() == Some(&normalized)
            && self.last_failure.get(&command) != Some(&normalized)
        {
            self.pending_stuck = Some(stuck_nudge(&command));
        }
        self.last_failed_output = Some(normalized.clone());
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
        if self.changed_work() {
            return (!self.check_after_last_edit
                && self.failed_checks.is_empty()
                && !self.verify_nudged)
                .then_some(FinishCheck::Verify);
        }
        (self.tool_calls > 0
            && !self.observed_work()
            && !self.watchdog_nudged
            && self.watchdog_allowed())
        .then_some(FinishCheck::Watchdog)
    }

    pub fn verification_nudge(&self) -> String {
        let hint = self.task.as_ref().and_then(|task| {
            task.checks
                .iter()
                .filter(|check| {
                    check.verification
                        || crate::command_evidence::verification_rejection(&check.command).is_some()
                })
                .max_by_key(|check| check.id)
                .and_then(|check| crate::command_evidence::verification_rejection(&check.command))
        });
        let mut message = hint.map_or_else(
            || VERIFY_NUDGE.into(),
            |hint| format!("{VERIFY_NUDGE}\n{hint}"),
        );
        if let Some(detail) = self
            .task
            .as_ref()
            .and_then(|task| task.stale_verification_detail())
        {
            message.push('\n');
            message.push_str(&detail);
        }
        message
    }

    /// Real edits, successfully executed checks, closed plan steps and usable child
    /// reports are progress. An unknown shell command bumping a revision isn't.
    pub fn finish_progress(&self) -> Option<FinishProgress> {
        self.task.as_ref().map(|task| {
            (
                task.observed_edits,
                self.verification_progress,
                task.plan
                    .iter()
                    .filter(|step| !step.status.is_open())
                    .count(),
                task.children
                    .iter()
                    .filter(|child| {
                        matches!(
                            child.outcome,
                            crate::task::TaskOutcome::Answered
                                | crate::task::TaskOutcome::NeedsReview
                                | crate::task::TaskOutcome::Accepted
                        )
                    })
                    .count(),
            )
        })
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
        if self.changed_work()
            && !self.check_after_last_edit
            && self.failed_checks.is_empty()
            && !self.verify_nudged
        {
            self.verify_nudged = true;
            return Some((NudgeReason::Verify, self.verification_nudge()));
        }
        if !self.changed_work()
            && !self.observed_work()
            && self.tool_calls > 0
            && !self.watchdog_nudged
            && self.watchdog_allowed()
        {
            self.watchdog_nudged = true;
            return Some((NudgeReason::Watchdog, WATCHDOG_NUDGE.to_string()));
        }
        self.factual_finish()
    }

    /// Finish rules that read facts rather than guess, so they apply even
    /// when the JEV judge has ruled on verify/watchdog: a failed last check,
    /// and open plan steps.
    pub fn factual_finish(&mut self) -> Option<(NudgeReason, String)> {
        if let Some((command, exit_code)) = self.failed_checks.first_key_value()
            && self.changed_work()
            && !self.failed_check_nudged
        {
            self.failed_check_nudged = true;
            return Some((NudgeReason::Verify, failed_check_nudge(command, *exit_code)));
        }
        if !self.open_plan.is_empty() && !self.plan_nudged {
            self.plan_nudged = true;
            return Some((NudgeReason::Watchdog, open_plan_nudge(&self.open_plan)));
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
        let commands: Vec<Value> = if let Some(task) = &self.task {
            let mut checks: Vec<_> = task
                .checks
                .iter()
                .filter(|check| {
                    check.contract_revision == task.contract.revision
                        && check.work_revision == task.work_revision
                })
                .collect();
            checks.sort_by_key(|check| check.id);
            checks
                .into_iter()
                .rev()
                .take(JEV_MAX_COMMANDS)
                .map(|check| {
                    json!({
                        "command": truncate(&check.command, JEV_MAX_ARGS_CHARS),
                        "exit_code": check.exit_code,
                        "verification": check.verification,
                        "state": check.state(task.contract.revision, task.work_revision),
                    })
                })
                .collect()
        } else {
            self.commands_after_last_edit
                .iter()
                .map(|c| json!({"command": c.command, "exit_code": c.exit_code}))
                .collect()
        };
        let mut state = json!({
            "files_edited": self.edited_paths,
            "commands_after_last_edit": commands,
            "final_message": truncate(final_message, JEV_MAX_MESSAGE_CHARS),
        });
        if let Some(contract) = &self.contract {
            state["task_contract"] = judge_contract(contract);
        }
        self.add_task_evidence(&mut state);
        state
    }

    /// State for the watchdog judge.
    pub fn watchdog_state(&self, final_message: &str) -> Value {
        let mut state = json!({
            "task": truncate(&self.user_message, JEV_MAX_TASK_CHARS),
            "files_edited": self.edited_paths,
            "tool_call_count": self.tool_calls,
            "recent_tool_calls": self.trace_json(),
            "final_message": truncate(final_message, JEV_MAX_MESSAGE_CHARS),
        });
        if let Some(contract) = &self.contract {
            state["task_contract"] = judge_contract(contract);
        }
        self.add_task_evidence(&mut state);
        state
    }

    fn changed_work(&self) -> bool {
        self.edited_files || self.possible_changes
    }

    fn observed_work(&self) -> bool {
        self.task.as_ref().is_some_and(|task| {
            task.has_current_verification()
                || task.files.iter().any(|file| {
                    matches!(file.state, crate::task::FileState::Present { bytes, .. } if bytes > 0)
                })
        })
    }

    fn add_task_evidence(&self, state: &mut Value) {
        if self.possible_changes {
            state["untracked_command_effects"] = json!(true);
        }
        if let Some(task) = &self.task {
            state["current_verification_passed"] = json!(task.has_current_verification());
            state["evidence_overflow"] = json!(task.evidence_overflow);
            state["observed_deliverables"] = json!(
                task.files
                    .iter()
                    .take(JEV_MAX_EDITED_FILES)
                    .map(|file| json!({
                        "path": truncate(&file.path, JEV_MAX_PATH_CHARS),
                        "state": file.state,
                    }))
                    .collect::<Vec<_>>()
            );
        }
    }

    fn watchdog_allowed(&self) -> bool {
        self.contract.as_ref().map_or_else(
            || task_asks_for_changes(&self.user_message),
            |contract| contract.mode == crate::task::TaskMode::Work,
        )
    }

    fn trace_json(&self) -> Vec<Value> {
        self.trace
            .iter()
            .map(|e| json!({"tool": e.tool, "args": e.args, "exit_code": e.exit_code, "output": e.output}))
            .collect()
    }
}

fn judge_contract(contract: &crate::task::TaskContract) -> Value {
    json!({
        "revision": contract.revision,
        "objective": truncate(&contract.objective, JEV_MAX_TASK_CHARS),
        "mode": contract.mode,
        "requirements": contract.requirements.iter().take(JEV_MAX_EDITED_FILES).map(|requirement| {
            json!({
                "id": requirement.id,
                "kind": requirement.spec.kind_label(),
                "text": truncate(requirement.spec.text(), JEV_MAX_ARGS_CHARS),
            })
        }).collect::<Vec<_>>(),
        "requirement_count": contract.requirements.len(),
        "requirements_truncated": contract.requirements.len() > JEV_MAX_EDITED_FILES,
        "restrictions": contract.restrictions.iter().take(JEV_MAX_EDITED_FILES)
            .map(|text| truncate(text, JEV_MAX_ARGS_CHARS)).collect::<Vec<_>>(),
    })
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

/// Lines `read_file` returns without a limit (see `tools/files.rs`).
const READ_DEFAULT_LINES: u64 = 400;
/// Overlapping re-reads of one file before the stuck nudge.
const REREAD_TRIGGER: u32 = 3;

fn path_key(path: &str) -> String {
    let path = path.trim();
    path.strip_prefix("./").unwrap_or(path).to_string()
}

/// Programs that only look around: running them after an edit verifies
/// nothing.
const INSPECTION_PROGRAMS: &[&str] = &[
    "ls", "cat", "head", "tail", "echo", "pwd", "grep", "rg", "find", "fd", "wc", "sed", "awk",
    "tree", "stat", "file", "which", "printf", "true", "sleep", "cd", "less", "more", "nl", "sort",
    "uniq", "cut", "du", "df", "date", "whoami", "type", "realpath", "basename", "dirname",
];
const INSPECTION_GIT: &[&str] = &[
    "status", "diff", "log", "show", "branch", "blame", "ls-files",
];

/// The subcommand Git will actually run for the words after `git`.
///
/// Global options that take a separate value (`-C <path>`) consume it, so
/// `git -C status reset --hard` yields `reset`, not `status`. Any global
/// option not listed here (including `-c`, `--config-env`, `--exec-path`
/// and `-p`) yields `None` so callers fail closed.
pub fn git_subcommand<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        match arg {
            "-C" | "--git-dir" | "--work-tree" | "--namespace" | "--attr-source" => {
                args.next()?;
            }
            "-P"
            | "--no-pager"
            | "--bare"
            | "--no-replace-objects"
            | "--no-lazy-fetch"
            | "--no-optional-locks"
            | "--literal-pathspecs"
            | "--glob-pathspecs"
            | "--noglob-pathspecs"
            | "--icase-pathspecs"
            | "--no-advice" => {}
            _ if [
                "--git-dir=",
                "--work-tree=",
                "--namespace=",
                "--attr-source=",
            ]
            .iter()
            .any(|prefix| arg.starts_with(prefix)) => {}
            _ if arg.starts_with('-') => return None,
            sub => return Some(sub),
        }
    }
    None
}

/// Whether every part of a shell command only inspects (ls, cat, git diff,
/// …). `cd x && cargo test | tail` is a real check.
pub fn is_inspection(command: &str) -> bool {
    command
        .split(['\n', ';', '|', '&'])
        .map(str::trim)
        // `2>&1` splits into a bare `1`: a redirect, not a program.
        .filter(|part| !part.is_empty() && !part.starts_with(|c: char| c.is_ascii_digit()))
        .all(|part| {
            let mut words = part
                .split_whitespace()
                .skip_while(|w| w.contains('=') && !w.starts_with('-'));
            match words.next() {
                None => true,
                Some("git") => git_subcommand(&words.collect::<Vec<_>>())
                    .is_some_and(|sub| INSPECTION_GIT.contains(&sub)),
                Some(program) => {
                    let program = program.rsplit('/').next().unwrap_or(program);
                    INSPECTION_PROGRAMS.contains(&program)
                }
            }
        })
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
    "broken",
    "bug",
    "bugs",
    "buggy",
    "crash",
    "crashes",
    "crashing",
    "fails",
    "failing",
    "replace",
    "replaces",
    "move",
    "convert",
    "extract",
    "inline",
    "support",
    "install",
    "upgrade",
    "downgrade",
    "improve",
    "optimize",
    "optimise",
    "speed",
    "clean",
    "cleanup",
    "resolve",
    "finish",
    "complete",
    "enable",
    "disable",
    "wire",
    "integrate",
    "extend",
    "tweak",
    "adjust",
    "tighten",
    "address",
    "tackle",
    "split",
    "merge",
    "format",
    "document",
    "translate",
    "scaffold",
    "setup",
    "configure",
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
    "should ",
];

/// A question that asks for an answer, not a change.
pub fn is_pure_question(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let starts = QUESTION_STARTERS.iter().any(|q| lower.starts_with(q));
    (starts && (lower.ends_with('?') || !lower.contains('\n')))
        || (lower.starts_with("do ") && lower.ends_with('?'))
}

/// Whether the task wording asks for code or file changes.
pub fn task_asks_for_changes(text: &str) -> bool {
    if is_pure_question(text) || crate::task::requests_read_only(text) {
        return false;
    }
    text.to_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .any(|word| CHANGE_VERBS.contains(&word))
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
