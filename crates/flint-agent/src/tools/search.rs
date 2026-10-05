//! `grep`: ripgrep when installed, otherwise a bounded walk with `regex`.

use std::path::Path;
use std::process::Stdio;

use regex::Regex;
use serde_json::Map;
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::ToolOutcome;
use super::display_path;
use super::resolve;
use super::str_arg;

const MAX_MATCH_LINES: usize = 200;
const MAX_LINE_CHARS: usize = 300;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 4096;
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
            let cancel = cancel.clone();
            match tokio::task::spawn_blocking(move || {
                fallback(&workspace, &pattern, &full, glob.as_deref(), &cancel)
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
    if cancel.is_cancelled() {
        return Some(Err("interrupted".to_string()));
    }
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
        .arg("--")
        .arg(target.strip_prefix(workspace).map_or(target, |rel| {
            if rel.as_os_str().is_empty() {
                Path::new(".")
            } else {
                rel
            }
        }))
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => return Some(Err(format!("rg failed: {err}"))),
    };
    let mut stdout = BufReader::new(child.stdout.take()?).lines();
    let mut stderr = child.stderr.take()?;
    // Drain both pipes concurrently, keeping only bounded output. Stop rg
    // once the extra match proves the displayed results are truncated.
    let read_stdout = async {
        let mut lines = Vec::new();
        while let Some(line) = stdout.next_line().await? {
            lines.push(line.strip_prefix("./").unwrap_or(&line).to_string());
            if lines.len() > MAX_MATCH_LINES {
                child.start_kill()?;
                break;
            }
        }
        Ok::<_, std::io::Error>(lines)
    };
    let read_stderr = async {
        let mut kept = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stderr.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let room = MAX_STDERR_BYTES - kept.len();
            kept.extend_from_slice(&buf[..n.min(room)]);
        }
        Ok::<_, std::io::Error>(kept)
    };
    let output = tokio::select! {
        output = async { tokio::try_join!(read_stdout, read_stderr) } =>
            output.map_err(|err| format!("rg failed: {err}")),
        () = cancel.cancelled() => Err("interrupted".to_string()),
    };
    let (lines, stderr) = match output {
        Ok(output) => output,
        Err(err) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Some(Err(err));
        }
    };
    let status = tokio::select! {
        status = child.wait() => match status {
            Ok(status) => status,
            Err(err) => return Some(Err(format!("rg failed: {err}"))),
        },
        () = cancel.cancelled() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Some(Err("interrupted".to_string()));
        }
    };
    // rg exits 1 for "no matches" and 2 for errors.
    // A signal is successful only when we intentionally stopped at the cap.
    if lines.len() <= MAX_MATCH_LINES && !status.success() && status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&stderr);
        return Some(Err(stderr.lines().next().unwrap_or("rg error").to_string()));
    }
    Some(Ok(lines))
}

fn fallback(
    workspace: &Path,
    pattern: &str,
    target: &Path,
    glob: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Vec<String>, String> {
    if cancel.is_cancelled() {
        return Err("interrupted".to_string());
    }
    let re = Regex::new(pattern).map_err(|err| format!("invalid pattern: {err}"))?;
    let suffix = glob.and_then(|g| g.strip_prefix('*'));
    let mut out = Vec::new();
    let mut stack = vec![target.to_path_buf()];
    while let Some(path) = stack.pop() {
        if cancel.is_cancelled() {
            return Err("interrupted".to_string());
        }
        if out.len() > MAX_MATCH_LINES {
            break;
        }
        if path.is_dir() {
            let Ok(read) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in read.flatten() {
                if cancel.is_cancelled() {
                    return Err("interrupted".to_string());
                }
                let name = entry.file_name();
                let hidden = name.to_string_lossy().starts_with('.');
                let symlink = entry.file_type().map_or(true, |kind| kind.is_symlink());
                if !(hidden || symlink || SKIP_DIRS.iter().any(|s| name == *s)) {
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
            if cancel.is_cancelled() {
                return Err("interrupted".to_string());
            }
            if re.is_match(line) {
                let line: String = line.chars().take(MAX_LINE_CHARS).collect();
                out.push(format!("{shown}:{}:{line}", i + 1));
                if out.len() > MAX_MATCH_LINES {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_caps_matches_even_within_one_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("many.txt"), "match\n".repeat(10_000)).expect("write");
        let found = fallback(
            dir.path(),
            "match",
            dir.path(),
            None,
            &CancellationToken::new(),
        )
        .expect("search");
        assert_eq!(found.len(), MAX_MATCH_LINES + 1);
    }

    #[cfg(unix)]
    #[test]
    fn fallback_does_not_follow_descendant_symlinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("outside.txt"), "needle\n").expect("write");
        std::os::unix::fs::symlink(
            outside.path().join("outside.txt"),
            dir.path().join("linked.txt"),
        )
        .expect("link");
        let found = fallback(
            dir.path(),
            "needle",
            dir.path(),
            None,
            &CancellationToken::new(),
        )
        .expect("search");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn fallback_obeys_cancellation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            fallback(dir.path(), "needle", dir.path(), None, &cancel),
            Err("interrupted".into())
        );
    }

    #[tokio::test]
    async fn ripgrep_caps_results_and_handles_option_like_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("-matches.txt");
        std::fs::write(&path, "match\n".repeat(10_000)).expect("write");
        let cancel = CancellationToken::new();
        let Some(result) = ripgrep(dir.path(), "match", &path, None, &cancel).await else {
            return; // The fallback is tested independently when rg is absent.
        };
        // rg's per-file cap is 50; verify that "-matches.txt" is not an option.
        assert_eq!(result.expect("search").len(), 50);
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("{i}.txt")), "match\n".repeat(50))
                .expect("write");
        }
        let result = ripgrep(dir.path(), "match", dir.path(), None, &cancel)
            .await
            .expect("rg installed")
            .expect("search");
        assert_eq!(result.len(), MAX_MATCH_LINES + 1);
    }

    #[tokio::test]
    async fn ripgrep_reports_invalid_patterns_and_cancellation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = CancellationToken::new();
        if let Some(result) = ripgrep(dir.path(), "[", dir.path(), None, &cancel).await {
            assert!(result.is_err());
        }
        cancel.cancel();
        assert_eq!(
            ripgrep(dir.path(), "needle", dir.path(), None, &cancel).await,
            Some(Err("interrupted".into()))
        );
    }
}
