//! `grep`: ripgrep when installed, otherwise a bounded walk with `regex`.

use std::path::Path;
use std::process::Stdio;

use regex::Regex;
use serde_json::Map;
use serde_json::Value;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::ToolOutcome;
use super::display_path;
use super::resolve;
use super::str_arg;

const MAX_MATCH_LINES: usize = 200;
const MAX_LINE_CHARS: usize = 300;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const SKIP_DIRS: [&str; 5] = [".git", "node_modules", "target", ".venv", "__pycache__"];

pub(super) async fn grep(
    workspace: &Path,
    args: &Map<String, Value>,
    cancel: &CancellationToken,
) -> ToolOutcome {
    let Some(pattern) = str_arg(args, "pattern").filter(|p| !p.is_empty()) else {
        return ToolOutcome::error("`pattern` is required");
    };
    let path = str_arg(args, "path").unwrap_or(".");
    let glob = str_arg(args, "glob");
    let full = match resolve(workspace, path) {
        Ok(full) => full,
        Err(err) => return ToolOutcome::error(err),
    };
    let lines = match ripgrep(workspace, pattern, &full, glob, cancel).await {
        Some(result) => result,
        None => {
            let (workspace, pattern, glob) = (
                workspace.to_path_buf(),
                pattern.to_string(),
                glob.map(str::to_string),
            );
            match tokio::task::spawn_blocking(move || {
                fallback(&workspace, &pattern, &full, glob.as_deref())
            })
            .await
            {
                Ok(result) => result,
                Err(err) => Err(format!("search failed: {err}")),
            }
        }
    };
    match lines {
        Ok(lines) if lines.is_empty() => ToolOutcome::ok("No matches."),
        Ok(mut lines) => {
            let more = lines.len() > MAX_MATCH_LINES;
            lines.truncate(MAX_MATCH_LINES);
            let mut out = lines.join("\n");
            if more {
                out.push_str(&format!(
                    "\n[stopped at {MAX_MATCH_LINES} matches; narrow the pattern or path]"
                ));
            }
            ToolOutcome::ok(out)
        }
        Err(err) => ToolOutcome::error(err),
    }
}

/// `None` when ripgrep isn't installed.
async fn ripgrep(
    workspace: &Path,
    pattern: &str,
    target: &Path,
    glob: Option<&str>,
    cancel: &CancellationToken,
) -> Option<Result<Vec<String>, String>> {
    let mut cmd = Command::new("rg");
    cmd.args([
        "--line-number",
        "--no-heading",
        "--color",
        "never",
        "--max-count",
        "50",
    ])
    .arg(format!("--max-columns={MAX_LINE_CHARS}"))
    .arg("--max-columns-preview");
    if let Some(glob) = glob {
        cmd.arg("--glob").arg(glob);
    }
    cmd.arg("--regexp")
        .arg(pattern)
        .arg(target.strip_prefix(workspace).map_or(target, |rel| {
            if rel.as_os_str().is_empty() {
                Path::new(".")
            } else {
                rel
            }
        }))
        .current_dir(workspace)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::select! {
        output = cmd.output() => output,
        () = cancel.cancelled() => return Some(Err("interrupted".to_string())),
    };
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => return Some(Err(format!("rg failed: {err}"))),
    };
    // rg exits 1 for "no matches" and 2 for errors.
    if output.status.code() == Some(2) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Some(Err(stderr.lines().next().unwrap_or("rg error").to_string()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(Ok(stdout
        .lines()
        .take(MAX_MATCH_LINES + 1)
        .map(|l| l.strip_prefix("./").unwrap_or(l).to_string())
        .collect()))
}

fn fallback(
    workspace: &Path,
    pattern: &str,
    target: &Path,
    glob: Option<&str>,
) -> Result<Vec<String>, String> {
    let re = Regex::new(pattern).map_err(|err| format!("invalid pattern: {err}"))?;
    let suffix = glob.and_then(|g| g.strip_prefix('*'));
    let mut out = Vec::new();
    let mut stack = vec![target.to_path_buf()];
    while let Some(path) = stack.pop() {
        if out.len() > MAX_MATCH_LINES {
            break;
        }
        if path.is_dir() {
            let Ok(read) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in read.flatten() {
                let name = entry.file_name();
                let hidden = name.to_string_lossy().starts_with('.');
                if !(hidden || SKIP_DIRS.iter().any(|s| name == *s)) {
                    stack.push(entry.path());
                }
            }
            continue;
        }
        if suffix.is_some_and(|s| !path.to_string_lossy().ends_with(s)) {
            continue;
        }
        if path.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let shown = display_path(workspace, &path);
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                let line: String = line.chars().take(MAX_LINE_CHARS).collect();
                out.push(format!("{shown}:{}:{line}", i + 1));
            }
        }
    }
    Ok(out)
}
