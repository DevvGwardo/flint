//! `run_command`: `sh -c` in the workspace with streamed output, a timeout,
//! cancellation, and head+tail truncation for the model.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Map;
use serde_json::Value;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::ToolOutcome;
use super::int_arg;
use super::str_arg;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
/// Characters of command output the model sees.
const MODEL_OUTPUT_CHARS: usize = 8_000;
/// Raw output kept in memory before truncation (protects against floods).
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
/// How long to keep reading after the shell exits.
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(300);

pub(super) async fn run_command(
    workspace: &Path,
    args: &Map<String, Value>,
    on_output: &(dyn Fn(String) + Send + Sync),
    cancel: &CancellationToken,
) -> ToolOutcome {
    let Some(command) = str_arg(args, "command").filter(|c| !c.trim().is_empty()) else {
        return ToolOutcome::error("`command` is required");
    };
    let timeout = Duration::from_secs(
        int_arg(args, "timeout_secs")
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS),
    );

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return ToolOutcome::error(format!("failed to start command: {err}")),
    };
    let pid = child.id();
    let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(pump(stdout, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(pump(stderr, tx.clone()));
    }
    drop(tx);

    let mut captured: Vec<u8> = Vec::new();
    let mut ended = None;
    let mut status = None;
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    // After the shell exits, a background child may still hold the pipes
    // open; read what is already there, then stop.
    let drain = tokio::time::sleep(Duration::from_secs(365 * 24 * 3600));
    tokio::pin!(drain);
    loop {
        tokio::select! {
            chunk = rx.recv() => match chunk {
                Ok(bytes) => {
                    on_output(String::from_utf8_lossy(&bytes).into_owned());
                    if captured.len() < MAX_CAPTURE_BYTES {
                        captured.extend_from_slice(&bytes);
                    }
                }
                // Both pipes closed: the process (group) is done writing.
                Err(_) => break,
            },
            exited = child.wait(), if status.is_none() => {
                status = Some(exited.ok());
                drain.as_mut().reset(tokio::time::Instant::now() + DRAIN_AFTER_EXIT);
            }
            () = &mut drain => break,
            () = &mut deadline => {
                ended = Some(format!("[timed out after {}s; process killed]", timeout.as_secs()));
                break;
            }
            () = cancel.cancelled() => {
                ended = Some("[interrupted; process killed]".to_string());
                break;
            }
        }
    }
    if ended.is_some() {
        kill_group(pid);
        let _ = child.kill().await;
    }
    let status = match status {
        Some(status) => status,
        None => child.wait().await.ok(),
    };
    let exit_code = match &ended {
        Some(_) => None,
        None => status.and_then(|s| s.code()),
    };
    let text = String::from_utf8_lossy(&captured);
    let mut output = head_tail(text.trim_end(), MODEL_OUTPUT_CHARS);
    if !output.is_empty() {
        output.push('\n');
    }
    match (&ended, exit_code) {
        (Some(note), _) => output.push_str(note),
        (None, Some(code)) => output.push_str(&format!("[exit code: {code}]")),
        (None, None) => output.push_str("[terminated by signal]"),
    }
    ToolOutcome {
        output,
        exit_code,
        success: ended.is_none() && exit_code == Some(0),
        diff: None,
    }
}

async fn pump(mut reader: impl AsyncRead + Unpin, tx: async_channel::Sender<Vec<u8>>) {
    let mut buf = vec![0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Kills the whole process group so `sh -c` children don't outlive it.
fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Keeps the head and the tail of long output, so trailing errors and test
/// summaries survive truncation. `max` counts characters.
pub fn head_tail(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let head_len = max * 2 / 5;
    let tail_len = max - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    let omitted = total - head_len - tail_len;
    format!("{head}\n… [{omitted} characters omitted] …\n{tail}")
}
