//! Local tools the model can call: shell commands, file reads and edits,
//! directory listing and search. Every result is bounded before it reaches
//! the model.

mod command;
mod diff;
pub mod fetch;
mod files;
pub mod sandbox;
mod search;
pub(crate) mod tracker;

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub use command::head_tail;
pub use diff::file_diff;
pub use tracker::FileTracker;
pub use tracker::UndoReport;

use crate::protocol::FileDiff;
use crate::protocol::ToolKind;

pub const RUN_COMMAND: &str = "run_command";
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";
pub const LIST_DIR: &str = "list_dir";
pub const GREP: &str = "grep";
pub const SPAWN_AGENT: &str = "spawn_agent";
pub const LIST_MODELS: &str = "list_models";
pub const UPDATE_PLAN: &str = "update_plan";
pub const FETCH_URL: &str = "fetch_url";

/// Names of every tool, in the order they are offered.
pub const TOOL_NAMES: [&str; 10] = [
    RUN_COMMAND,
    READ_FILE,
    WRITE_FILE,
    EDIT_FILE,
    LIST_DIR,
    GREP,
    FETCH_URL,
    UPDATE_PLAN,
    SPAWN_AGENT,
    LIST_MODELS,
];

/// What a finished tool call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    /// Text the model sees (already bounded).
    pub output: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub diff: Option<FileDiff>,
}

impl ToolOutcome {
    pub(crate) fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            exit_code: None,
            success: true,
            diff: None,
        }
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self {
            output: format!("Error: {}", message.into()),
            exit_code: None,
            success: false,
            diff: None,
        }
    }
}

/// The kind of a tool, for the UI and the harness.
pub fn tool_kind(name: &str) -> ToolKind {
    match name {
        RUN_COMMAND => ToolKind::Command,
        READ_FILE | LIST_DIR => ToolKind::Read,
        WRITE_FILE | EDIT_FILE => ToolKind::Edit,
        GREP | FETCH_URL => ToolKind::Search,
        _ => ToolKind::Other,
    }
}

/// The file an edit tool writes, if this call is an edit.
pub fn edit_path(name: &str, args: &Map<String, Value>) -> Option<String> {
    match tool_kind(name) {
        ToolKind::Edit => str_arg(args, "path").map(str::to_string),
        ToolKind::Command | ToolKind::Read | ToolKind::Search | ToolKind::Other => None,
    }
}

/// One-line human summary of a call.
pub fn summary(name: &str, args: &Map<String, Value>) -> String {
    let path = str_arg(args, "path").unwrap_or(".");
    let text = match name {
        RUN_COMMAND => str_arg(args, "command")
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
        READ_FILE => match (int_arg(args, "offset"), int_arg(args, "limit")) {
            (Some(offset), Some(limit)) => format!(
                "{path}:{offset}-{}",
                offset.saturating_add(limit.saturating_sub(1))
            ),
            (Some(offset), None) => format!("{path}:{offset}"),
            (None, Some(limit)) => format!("{path}:1-{limit}"),
            (None, None) => path.to_string(),
        },
        GREP => format!("{} in {path}", str_arg(args, "pattern").unwrap_or_default()),
        FETCH_URL => str_arg(args, "url").unwrap_or_default().trim().to_string(),
        SPAWN_AGENT => str_arg(args, "label").unwrap_or("Subagent").to_string(),
        UPDATE_PLAN => match parse_plan(args) {
            Ok(plan) => {
                let done = plan
                    .iter()
                    .filter(|s| s.status == StepStatus::Completed)
                    .count();
                format!("Plan: {done}/{} done", plan.len())
            }
            Err(_) => "Plan".to_string(),
        },
        WRITE_FILE | EDIT_FILE | LIST_DIR => path.to_string(),
        _ => serde_json::to_string(args).unwrap_or_default(),
    };
    let mut text: String = text.chars().take(160).collect();
    if text.is_empty() {
        text = name.to_string();
    }
    text
}

/// OpenAI function-tool definitions for every tool.
pub fn tool_specs() -> Vec<Value> {
    let spec = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }
            }
        })
    };
    vec![
        spec(
            RUN_COMMAND,
            "Run a shell command (sh -c) in the workspace. Use it to build, test, run scripts, and use git. \
             Output is the head and tail of stdout+stderr plus the exit code. Keep output bounded \
             (head, tail, grep -m).",
            json!({
                "command": {"type": "string", "description": "The shell command to run."},
                "timeout_secs": {"type": "integer", "description": "Timeout in seconds (default 120, max 600)."}
            }),
            &["command"],
        ),
        spec(
            READ_FILE,
            "Read a text file with line numbers. Use offset/limit for large files.",
            json!({
                "path": {"type": "string", "description": "File path, relative to the workspace."},
                "offset": {"type": "integer", "description": "First line to read (1-based)."},
                "limit": {"type": "integer", "description": "Number of lines (default 400, max 2000)."}
            }),
            &["path"],
        ),
        spec(
            WRITE_FILE,
            "Create a file or replace its whole content. Creates parent directories. Prefer edit_file for \
             changes to existing files.",
            json!({
                "path": {"type": "string", "description": "File path, relative to the workspace."},
                "content": {"type": "string", "description": "The complete new file content."}
            }),
            &["path", "content"],
        ),
        spec(
            EDIT_FILE,
            "Replace an exact snippet in a file. old_string must match the file exactly (including \
             whitespace) and be unique unless replace_all is true; include surrounding lines to make it unique.",
            json!({
                "path": {"type": "string", "description": "File path, relative to the workspace."},
                "old_string": {"type": "string", "description": "Exact text to replace."},
                "new_string": {"type": "string", "description": "Replacement text."},
                "replace_all": {"type": "boolean", "description": "Replace every occurrence."}
            }),
            &["path", "old_string", "new_string"],
        ),
        spec(
            LIST_DIR,
            "List a directory (dirs end with /). Skips .git, node_modules and target.",
            json!({
                "path": {"type": "string", "description": "Directory, relative to the workspace (default .)."},
                "depth": {"type": "integer", "description": "Levels to descend (default 1, max 3)."}
            }),
            &[],
        ),
        spec(
            GREP,
            "Search file contents with a regular expression (ripgrep syntax). Returns path:line:text, bounded.",
            json!({
                "pattern": {"type": "string", "description": "Regular expression."},
                "path": {"type": "string", "description": "File or directory to search (default .)."},
                "glob": {"type": "string", "description": "Only search files matching this glob, e.g. *.rs."}
            }),
            &["pattern"],
        ),
        spec(
            FETCH_URL,
            "Fetch a web page or file over http(s) and return it as text (HTML is reduced to \
             readable text, bounded). Use it for documentation, API references, changelogs and \
             issues the task depends on. Not for downloading dependencies: use the package manager.",
            json!({
                "url": {"type": "string", "description": "The http:// or https:// URL."}
            }),
            &["url"],
        ),
        spec(
            UPDATE_PLAN,
            "Keep a short step-by-step plan for multi-step tasks. Send the whole plan each time, \
             with exactly one step in_progress while you work. Mark steps completed as you finish \
             them. Before you end the turn, every step must be completed, or dropped with the \
             reason in your final message. Skip it for simple one-step tasks.",
            json!({
                "plan": {
                    "type": "array",
                    "description": "Every step, in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": {"type": "string", "description": "One short imperative sentence."},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed", "dropped"]}
                        },
                        "required": ["step", "status"],
                        "additionalProperties": false
                    }
                }
            }),
            &["plan"],
        ),
        spec(
            SPAWN_AGENT,
            "Delegate a self-contained task to an isolated subagent in the same workspace. \
             It does not see your conversation: include all needed context in message. \
             Returns only its final answer and a session_id for follow-ups. Consecutive independent \
             calls in one response run in parallel (up to four); use disjoint write sets for edits. \
             Do not delegate a single file read or duplicate delegated work. Omit model to use \
             the configured subagent model, otherwise the parent model. For an explicit override, \
             call list_models first and pass an exact model id. Never silently substitute models. \
             A resumed session retains its model: do not combine model and session_id.",
            json!({
                "label": {"type": "string", "description": "Short task label for the UI."},
                "message": {"type": "string", "description": "Full task and context, or a short follow-up."},
                "session_id": {"type": "string", "description": "Existing child session to resume. Omit for a new session."},
                "model": {"type": "string", "description": "Optional exact model id from list_models, on the configured endpoint."}
            }),
            &["label", "message"],
        ),
        spec(
            LIST_MODELS,
            "List model ids available on the configured endpoint, plus the parent and default \
             subagent model. Results are paginated; use next_offset to read another page. \
             Use before selecting an explicit spawn_agent model.",
            json!({
                "offset": {"type": "integer", "description": "First model index to return (default 0)."}
            }),
            &[],
        ),
    ]
}

/// One step of an [`UPDATE_PLAN`] plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Dropped,
}

impl StepStatus {
    fn parse(text: &str) -> Option<Self> {
        match text
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "pending" | "todo" => Some(Self::Pending),
            "in_progress" | "active" | "doing" => Some(Self::InProgress),
            "completed" | "complete" | "done" => Some(Self::Completed),
            "dropped" | "skipped" | "cancelled" | "canceled" => Some(Self::Dropped),
            _ => None,
        }
    }

    fn mark(self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[>]",
            Self::Completed => "[x]",
            Self::Dropped => "[-]",
        }
    }

    /// Whether the step still needs work.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}

/// The plan in [`UPDATE_PLAN`] arguments.
pub fn parse_plan(args: &Map<String, Value>) -> Result<Vec<PlanStep>, String> {
    let items = match args.get("plan") {
        Some(Value::Array(items)) => items,
        // Cheap models sometimes send the array as a JSON string.
        Some(Value::String(text)) => {
            return match serde_json::from_str::<Value>(text) {
                Ok(Value::Array(items)) => {
                    let mut map = Map::new();
                    map.insert("plan".into(), Value::Array(items));
                    parse_plan(&map)
                }
                _ => Err("`plan` must be an array of {step, status}.".into()),
            };
        }
        _ => return Err("`plan` must be an array of {step, status}.".into()),
    };
    let mut plan = Vec::with_capacity(items.len());
    for item in items {
        let step = item
            .get("step")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("Every plan item needs a non-empty `step`.")?;
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .and_then(StepStatus::parse)
            .ok_or("`status` must be pending, in_progress, completed or dropped.")?;
        plan.push(PlanStep {
            step: step.chars().take(300).collect(),
            status,
        });
    }
    if plan.len() > 50 {
        return Err("Keep the plan to 50 steps or fewer.".into());
    }
    Ok(plan)
}

/// The plan as the model sees it back.
pub fn render_plan(plan: &[PlanStep]) -> String {
    let done = plan.iter().filter(|s| !s.status.is_open()).count();
    let mut out = format!("Plan updated ({done}/{} closed):", plan.len());
    for step in plan {
        out.push_str(&format!("\n{} {}", step.status.mark(), step.step));
    }
    out
}

/// What one agent's tools work with.
#[derive(Debug)]
pub struct ToolContext {
    /// Canonical workspace directory.
    pub workspace: PathBuf,
    /// What this agent has read or written of each file.
    pub seen: FileTracker,
    /// The `sandbox-exec` policy for commands; `None` runs them unconfined.
    pub sandbox: Option<String>,
}

impl ToolContext {
    /// A context for `workspace`, sandboxing commands when `sandbox` is set
    /// and the platform supports it.
    pub fn new(workspace: PathBuf, sandbox: bool) -> Self {
        let sandbox = sandbox.then(|| sandbox::profile(&workspace)).flatten();
        let seen = FileTracker::for_workspace(&workspace);
        Self {
            workspace,
            seen,
            sandbox,
        }
    }

    /// The context for a subagent: same workspace, sandbox and undo journal,
    /// its own record of what it has read.
    pub fn child(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            seen: self.seen.child(),
            sandbox: self.sandbox.clone(),
        }
    }
}

/// Runs one tool call. `on_output` receives streamed command output.
pub async fn execute(
    ctx: &ToolContext,
    name: &str,
    args: &Map<String, Value>,
    on_output: &(dyn Fn(String) + Send + Sync),
    cancel: &CancellationToken,
) -> ToolOutcome {
    let (workspace, seen) = (ctx.workspace.as_path(), &ctx.seen);
    match name {
        RUN_COMMAND => {
            command::run_command(workspace, args, ctx.sandbox.as_deref(), on_output, cancel).await
        }
        READ_FILE => files::read_file(workspace, args, seen, cancel).await,
        WRITE_FILE => files::write_file(workspace, args, seen, cancel).await,
        EDIT_FILE => files::edit_file(workspace, args, seen, cancel).await,
        LIST_DIR => files::list_dir(workspace, args).await,
        GREP => search::grep(workspace, args, cancel).await,
        FETCH_URL => fetch::fetch_url(args, cancel).await,
        _ => ToolOutcome::error(format!(
            "unknown tool `{name}`. Available tools: {}",
            TOOL_NAMES.join(", ")
        )),
    }
}

fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn int_arg(args: &Map<String, Value>, key: &str) -> Option<u64> {
    let value = args.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

fn bool_arg(args: &Map<String, Value>, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        Some(Value::Null | Value::Number(_) | Value::Array(_) | Value::Object(_)) | None => false,
    }
}

/// Resolves `path` against the workspace, refusing paths that leave it.
fn resolve(workspace: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        workspace.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    let outside = || {
        format!(
            "path `{path}` is outside the workspace {}",
            workspace.display()
        )
    };
    if !normalized.starts_with(workspace) {
        return Err(outside());
    }
    // A symlink inside the workspace may point outside it: check where the
    // deepest existing part of the path really is. A dangling link can't be
    // resolved, so it is refused (writing through it would create its target).
    if let Ok(root) = workspace.canonicalize() {
        for part in normalized
            .ancestors()
            .take_while(|p| p.starts_with(workspace))
        {
            if part.symlink_metadata().is_err() {
                continue;
            }
            match part.canonicalize() {
                Ok(real) if real.starts_with(&root) => {
                    // Canonicalize the existing identity, including directory
                    // aliases; append only the not-yet-existing suffix.
                    let suffix = normalized.strip_prefix(part).unwrap_or(Path::new(""));
                    return Ok(if suffix.as_os_str().is_empty() {
                        real
                    } else {
                        real.join(suffix)
                    });
                }
                Ok(_) | Err(_) => return Err(outside()),
            }
        }
    }
    Ok(normalized)
}

/// Workspace-relative display form of an absolute path.
fn display_path(workspace: &Path, path: &Path) -> String {
    path.strip_prefix(workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
