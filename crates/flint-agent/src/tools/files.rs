//! File tools: read_file, write_file, edit_file, list_dir.

use std::path::Path;

use serde_json::Map;
use serde_json::Value;

use super::ToolOutcome;
use super::bool_arg;
use super::diff::file_diff;
use super::display_path;
use super::int_arg;
use super::resolve;
use super::str_arg;

const DEFAULT_READ_LINES: u64 = 400;
const MAX_READ_LINES: u64 = 2_000;
const MAX_READ_CHARS: usize = 40_000;
const MAX_LINE_CHARS: usize = 2_000;
const MAX_LIST_ENTRIES: usize = 500;
const SKIP_DIRS: [&str; 5] = [".git", "node_modules", "target", ".venv", "__pycache__"];

pub(super) async fn read_file(workspace: &Path, args: &Map<String, Value>) -> ToolOutcome {
    let Some(path) = str_arg(args, "path") else {
        return ToolOutcome::error("`path` is required");
    };
    let full = match resolve(workspace, path) {
        Ok(full) => full,
        Err(err) => return ToolOutcome::error(err),
    };
    let bytes = match tokio::fs::read(&full).await {
        Ok(bytes) => bytes,
        Err(err) => return ToolOutcome::error(format!("cannot read {path}: {err}")),
    };
    if bytes.contains(&0) {
        return ToolOutcome::error(format!("{path} looks like a binary file"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len() as u64;
    let offset = int_arg(args, "offset").unwrap_or(1).max(1);
    let limit = int_arg(args, "limit")
        .unwrap_or(DEFAULT_READ_LINES)
        .clamp(1, MAX_READ_LINES);
    if total == 0 {
        return ToolOutcome::ok(format!("{path} is empty."));
    }
    if offset > total {
        return ToolOutcome::error(format!(
            "offset {offset} is past the end of {path} ({total} lines)"
        ));
    }
    let width = total.to_string().len();
    let mut out = String::new();
    let mut last = offset - 1;
    for (i, line) in lines
        .iter()
        .enumerate()
        .skip((offset - 1) as usize)
        .take(limit as usize)
    {
        let line: String = if line.chars().count() > MAX_LINE_CHARS {
            let mut cut: String = line.chars().take(MAX_LINE_CHARS).collect();
            cut.push_str(" …[line truncated]");
            cut
        } else {
            (*line).to_string()
        };
        let row = format!("{:>width$}\t{line}\n", i + 1);
        if out.len() + row.len() > MAX_READ_CHARS {
            break;
        }
        out.push_str(&row);
        last = i as u64 + 1;
    }
    if last < total {
        out.push_str(&format!(
            "[showing lines {offset}-{last} of {total}; pass offset={} to read more]",
            last + 1
        ));
    }
    ToolOutcome::ok(out)
}

pub(super) async fn write_file(workspace: &Path, args: &Map<String, Value>) -> ToolOutcome {
    let (Some(path), Some(content)) = (str_arg(args, "path"), str_arg(args, "content")) else {
        return ToolOutcome::error("`path` and `content` are required");
    };
    let full = match resolve(workspace, path) {
        Ok(full) => full,
        Err(err) => return ToolOutcome::error(err),
    };
    let old = tokio::fs::read_to_string(&full).await.ok();
    if let Some(parent) = full.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        return ToolOutcome::error(format!("cannot create {}: {err}", parent.display()));
    }
    if let Err(err) = tokio::fs::write(&full, content).await {
        return ToolOutcome::error(format!("cannot write {path}: {err}"));
    }
    let shown = display_path(workspace, &full);
    let diff = file_diff(&shown, old.as_deref(), content);
    let verb = if diff.created { "Created" } else { "Wrote" };
    ToolOutcome {
        output: format!("{verb} {shown} (+{} -{})", diff.added, diff.removed),
        exit_code: None,
        success: true,
        diff: Some(diff),
    }
}

pub(super) async fn edit_file(workspace: &Path, args: &Map<String, Value>) -> ToolOutcome {
    let (Some(path), Some(old_string), Some(new_string)) = (
        str_arg(args, "path"),
        str_arg(args, "old_string"),
        str_arg(args, "new_string"),
    ) else {
        return ToolOutcome::error("`path`, `old_string` and `new_string` are required");
    };
    if old_string.is_empty() {
        return ToolOutcome::error("`old_string` is empty; use write_file to create a file");
    }
    if old_string == new_string {
        return ToolOutcome::error(
            "`old_string` and `new_string` are identical; nothing to change",
        );
    }
    let full = match resolve(workspace, path) {
        Ok(full) => full,
        Err(err) => return ToolOutcome::error(err),
    };
    let old = match tokio::fs::read_to_string(&full).await {
        Ok(old) => old,
        Err(err) => return ToolOutcome::error(format!("cannot read {path}: {err}")),
    };
    let matches = old.matches(old_string).count();
    let replace_all = bool_arg(args, "replace_all");
    match (matches, replace_all) {
        (0, _) => {
            return ToolOutcome::error(format!(
                "old_string was not found in {path}. It must match exactly, including whitespace and \
                 indentation; read_file the region again and copy the text."
            ));
        }
        (1, _) | (_, true) => {}
        (n, false) => {
            return ToolOutcome::error(format!(
                "old_string matches {n} places in {path}. Include more surrounding lines to make it \
                 unique, or set replace_all to true."
            ));
        }
    }
    let new = old.replace(old_string, new_string);
    if let Err(err) = tokio::fs::write(&full, &new).await {
        return ToolOutcome::error(format!("cannot write {path}: {err}"));
    }
    let shown = display_path(workspace, &full);
    let diff = file_diff(&shown, Some(&old), &new);
    let times = if matches > 1 {
        format!(" ({matches} occurrences)")
    } else {
        String::new()
    };
    ToolOutcome {
        output: format!("Edited {shown}{times} (+{} -{})", diff.added, diff.removed),
        exit_code: None,
        success: true,
        diff: Some(diff),
    }
}

pub(super) async fn list_dir(workspace: &Path, args: &Map<String, Value>) -> ToolOutcome {
    let path = str_arg(args, "path").unwrap_or(".");
    let depth = int_arg(args, "depth").unwrap_or(1).clamp(1, 3) as usize;
    let full = match resolve(workspace, path) {
        Ok(full) => full,
        Err(err) => return ToolOutcome::error(err),
    };
    let root = full.clone();
    let listing = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        let mut truncated = false;
        walk(&root, &root, depth, &mut entries, &mut truncated);
        (entries, truncated)
    })
    .await;
    let (entries, truncated) = match listing {
        Ok(listing) => listing,
        Err(err) => return ToolOutcome::error(format!("listing failed: {err}")),
    };
    if !full.is_dir() {
        return ToolOutcome::error(format!("{path} is not a directory"));
    }
    let mut out = entries.join("\n");
    if entries.is_empty() {
        out = format!("{path} is empty.");
    }
    if truncated {
        out.push_str(&format!("\n[stopped at {MAX_LIST_ENTRIES} entries]"));
    }
    ToolOutcome::ok(out)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>, truncated: &mut bool) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<_> = read.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for entry in children {
        if out.len() >= MAX_LIST_ENTRIES {
            *truncated = true;
            return;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            out.push(format!("{rel}/"));
            let name = entry.file_name();
            let skip = SKIP_DIRS.iter().any(|s| name == *s);
            if depth > 1 && !skip {
                walk(root, &path, depth - 1, out, truncated);
            }
        } else {
            out.push(rel);
        }
    }
}
