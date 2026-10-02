//! Local tools the model can call: shell commands, file reads and edits,
//! directory listing and search. Every result is bounded before it reaches
//! the model.

mod command;
mod diff;
mod files;
mod search;

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub use command::head_tail;
pub use diff::file_diff;

use crate::protocol::FileDiff;
use crate::protocol::ToolKind;

pub const RUN_COMMAND: &str = "run_command";
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";
pub const LIST_DIR: &str = "list_dir";
pub const GREP: &str = "grep";

/// Names of every tool, in the order they are offered.
pub const TOOL_NAMES: [&str; 6] = [
    RUN_COMMAND,
    READ_FILE,
    WRITE_FILE,
    EDIT_FILE,
    LIST_DIR,
    GREP,
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
    fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            exit_code: None,
            success: true,
            diff: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
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
        GREP => ToolKind::Search,
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
            (Some(offset), Some(limit)) => format!("{path}:{offset}-{}", offset + limit - 1),
            (Some(offset), None) => format!("{path}:{offset}"),
            (None, Some(limit)) => format!("{path}:1-{limit}"),
            (None, None) => path.to_string(),
        },
        GREP => format!("{} in {path}", str_arg(args, "pattern").unwrap_or_default()),
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
    ]
}

/// Runs one tool call. `on_output` receives streamed command output.
pub async fn execute(
    workspace: &Path,
    name: &str,
    args: &Map<String, Value>,
    on_output: &(dyn Fn(String) + Send + Sync),
    cancel: &CancellationToken,
) -> ToolOutcome {
    match name {
        RUN_COMMAND => command::run_command(workspace, args, on_output, cancel).await,
        READ_FILE => files::read_file(workspace, args).await,
        WRITE_FILE => files::write_file(workspace, args).await,
        EDIT_FILE => files::edit_file(workspace, args).await,
        LIST_DIR => files::list_dir(workspace, args).await,
        GREP => search::grep(workspace, args, cancel).await,
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
    if normalized.starts_with(workspace) {
        Ok(normalized)
    } else {
        Err(format!(
            "path `{path}` is outside the workspace {}",
            workspace.display()
        ))
    }
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
