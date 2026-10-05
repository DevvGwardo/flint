//! The ACP `terminal/*` client methods: flint runs a command for the agent,
//! keeps its output (within the agent's byte limit, dropping the oldest
//! output first), streams it to a read-only terminal tab, and reports its
//! exit status. Commands start in the workspace unless the agent says
//! otherwise, and never outside it.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use agent_client_protocol::schema::v1::CreateTerminalRequest;
use agent_client_protocol::schema::v1::TerminalExitStatus;
use async_channel::Sender;
use flint_agent::AgentEvent;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::watch;

use crate::files::inside_workspace;
use crate::output::Utf8Decoder;
use crate::output::append_tail;
use crate::process::DRAIN_GRACE;
use crate::process::stop;

/// Output kept when the agent sets no limit.
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

/// Compatibility for adapters that send a shell script instead of argv. Try
/// the literal executable first, so paths containing spaces remain intact.
fn shell_candidate(request: &CreateTerminalRequest, cwd: &Path) -> bool {
    if !request.args.is_empty() || cwd.join(&request.command).exists() {
        return false;
    }
    let syntax = request
        .command
        .chars()
        .any(|c| c.is_whitespace() || "|&;<>()$`'\"\\*?~".contains(c));
    if !syntax {
        return false;
    }
    // A missing literal path with spaces is not automatically a shell script.
    if Path::new(&request.command).is_absolute()
        || request.command.starts_with("./")
        || request.command.starts_with("../")
    {
        let first = request.command.split_whitespace().next().unwrap_or("");
        return cwd.join(first).is_file()
            || request.command.chars().any(|c| "|&;<>()$`'\"".contains(c));
    }
    true
}

fn command_for(
    program: &str,
    args: &[String],
    request: &CreateTerminalRequest,
    cwd: &Path,
) -> Command {
    let mut command = Command::new(program);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    command
        .args(args)
        .current_dir(cwd)
        .env("PATH", crate::launch::search_path(home.as_deref()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for var in &request.env {
        command.env(&var.name, &var.value);
    }
    #[cfg(unix)]
    command.process_group(0);
    command
}

#[cfg(unix)]
fn shell(request: &CreateTerminalRequest) -> String {
    use std::os::unix::fs::PermissionsExt;
    request
        .env
        .iter()
        .rev()
        .find(|var| var.name == "SHELL")
        .map(|var| var.value.clone())
        .or_else(|| std::env::var("SHELL").ok())
        .filter(|shell| {
            Path::new(shell).is_absolute()
                && std::fs::metadata(shell)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .unwrap_or_else(|| "/bin/sh".to_string())
}

/// Shape-only diagnostics: scripts, argument values and environment values
/// can contain credentials, and must never enter the application log.
pub(crate) fn request_diagnostic(request: &CreateTerminalRequest) -> String {
    format!(
        "command=<redacted>, command_bytes={}, multiline={}, shell_syntax={}, args_count={}, cwd={:?}, env_names={:?}",
        request.command.len(),
        request.command.contains(['\n', '\r']),
        request
            .command
            .chars()
            .any(|c| c.is_whitespace() || "|&;<>()$`'\"".contains(c)),
        request.args.len(),
        request.cwd,
        request.env.iter().map(|var| &var.name).collect::<Vec<_>>(),
    )
}

pub(crate) fn diagnostic_reason(request: &CreateTerminalRequest, reason: &str) -> String {
    let mut reason = reason.to_string();
    for value in std::iter::once(&request.command)
        .chain(request.args.iter())
        .chain(request.env.iter().map(|var| &var.value))
        .filter(|value| !value.is_empty())
    {
        reason = reason.replace(&format!("{value:?}"), "<redacted>");
        reason = reason.replace(value, "<redacted>");
    }
    reason
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<String>,
}

impl Exit {
    pub fn to_acp(&self) -> TerminalExitStatus {
        TerminalExitStatus::new()
            .exit_code(self.code.and_then(|c| u32::try_from(c).ok()))
            .signal(self.signal.clone())
    }
}

struct Running {
    output: Mutex<(String, bool)>,
    limit: usize,
    exit: watch::Receiver<Option<Exit>>,
    kill: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// The terminals one session created.
pub struct Terminals {
    workspace: PathBuf,
    events: Sender<AgentEvent>,
    label_prefix: String,
    next: AtomicU64,
    running: Mutex<HashMap<String, Arc<Running>>>,
}

impl Terminals {
    pub fn new(workspace: &Path, events: Sender<AgentEvent>, label_prefix: &str) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            events,
            label_prefix: label_prefix.to_string(),
            next: AtomicU64::new(1),
            running: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, id: &str) -> Result<Arc<Running>, String> {
        self.running
            .lock()
            .ok()
            .and_then(|map| map.get(id).cloned())
            .ok_or_else(|| format!("no terminal {id}"))
    }

    /// Starts the command; returns its terminal id.
    pub fn create(&self, request: &CreateTerminalRequest) -> Result<String, String> {
        let cwd = match &request.cwd {
            Some(cwd) => inside_workspace(&self.workspace, cwd)?,
            None => self.workspace.clone(),
        };
        if request.command.trim().is_empty() {
            return Err(format!(
                "cannot run an empty command (cwd: {})",
                cwd.display()
            ));
        }
        if !cwd.is_dir() {
            return Err(format!(
                "terminal cwd is not a directory: {}",
                cwd.display()
            ));
        }
        let mut spawned = command_for(&request.command, &request.args, request, &cwd).spawn();
        let mut program = request.command.clone();
        #[cfg(unix)]
        if spawned.as_ref().is_err_and(|err| {
            matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidFilename
            )
        }) && shell_candidate(request, &cwd)
        {
            program = shell(request);
            spawned = command_for(
                &program,
                &["-c".to_string(), request.command.clone()],
                request,
                &cwd,
            )
            .spawn();
        }
        let mut child = spawned
            .map_err(|err| format!("cannot run {program:?}: {err} (cwd: {})", cwd.display()))?;
        let pid = child.id();
        let mut group = crate::process::GroupGuard::new(pid);
        let id = format!("flint-term-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
        let running = Arc::new(Running {
            output: Mutex::new((String::new(), false)),
            limit: request.output_byte_limit.map_or(DEFAULT_OUTPUT_LIMIT, |l| {
                usize::try_from(l)
                    .unwrap_or(usize::MAX)
                    .min(DEFAULT_OUTPUT_LIMIT)
            }),
            exit: exit_rx,
            kill: Mutex::new(Some(kill_tx)),
        });
        if let Ok(mut map) = self.running.lock() {
            map.insert(id.clone(), Arc::clone(&running));
        }
        let line = std::iter::once(request.command.as_str())
            .chain(request.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = self.events.try_send(AgentEvent::TerminalStarted {
            terminal_id: id.clone(),
            call_id: None,
            label: format!("{}: {line}", self.label_prefix),
            cwd: Some(cwd),
        });

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let events = self.events.clone();
        let terminal_id = id.clone();
        tokio::spawn(async move {
            let pump_running = Arc::clone(&running);
            let pump_events = events.clone();
            let pump_id = terminal_id.clone();
            let mut pumps = tokio::spawn(async move {
                tokio::join!(
                    pump(stdout, &pump_running, &pump_events, &pump_id),
                    pump(stderr, &pump_running, &pump_events, &pump_id)
                )
            });
            let (status, stopped) = tokio::select! {
                status = child.wait() => (status.ok(), false),
                _ = kill_rx => {
                    stop(&mut child, pid, DRAIN_GRACE).await;
                    (child.wait().await.ok(), true)
                }
            };
            // A leader can exit with descendants retaining its pipes. Reap it
            // independently, stop the owned group and only bound the drain.
            if !stopped {
                stop(&mut child, pid, DRAIN_GRACE).await;
            }
            group.disarm();
            if tokio::time::timeout(DRAIN_GRACE, &mut pumps).await.is_err() {
                pumps.abort();
                let _ = pumps.await;
            }
            let exit = Exit {
                code: status.and_then(|s| s.code()),
                signal: status.and_then(|s| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        s.signal().map(|n| format!("signal {n}"))
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = s;
                        None
                    }
                }),
            };
            let _ = events.try_send(AgentEvent::TerminalExited {
                terminal_id,
                exit_code: exit.code,
            });
            let _ = exit_tx.send(Some(exit));
        });
        Ok(id)
    }

    /// Output so far, whether it was truncated, and the exit status if done.
    pub fn output(&self, id: &str) -> Result<(String, bool, Option<Exit>), String> {
        let running = self.get(id)?;
        let (output, truncated) = running.output.lock().map(|o| o.clone()).unwrap_or_default();
        let exit = running.exit.borrow().clone();
        Ok((output, truncated, exit))
    }

    pub async fn wait_for_exit(&self, id: &str) -> Result<Exit, String> {
        let mut exit = self.get(id)?.exit.clone();
        loop {
            if let Some(status) = exit.borrow().clone() {
                return Ok(status);
            }
            if exit.changed().await.is_err() {
                return Ok(Exit::default());
            }
        }
    }

    pub fn kill(&self, id: &str) -> Result<(), String> {
        let running = self.get(id)?;
        if let Some(kill) = running.kill.lock().ok().and_then(|mut k| k.take()) {
            let _ = kill.send(());
        }
        Ok(())
    }

    /// Kills (if still running) and forgets the terminal.
    pub fn release(&self, id: &str) -> Result<(), String> {
        self.kill(id)?;
        if let Ok(mut map) = self.running.lock() {
            map.remove(id);
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Ok(map) = self.running.lock() {
            for running in map.values() {
                if let Some(kill) = running.kill.lock().ok().and_then(|mut k| k.take()) {
                    let _ = kill.send(());
                }
            }
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn pump(
    reader: Option<impl AsyncRead + Unpin>,
    running: &Running,
    events: &Sender<AgentEvent>,
    id: &str,
) {
    let Some(mut reader) = reader else {
        return;
    };
    let mut buf = vec![0u8; 8192];
    let mut decoder = Utf8Decoder::default();
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => 0,
            Ok(n) => n,
        };
        let text = decoder.decode(&buf[..n], n == 0);
        if let Ok(mut output) = running.output.lock() {
            let truncated = append_tail(&mut output.0, &text, running.limit);
            output.1 |= truncated;
        }
        if !text.is_empty() {
            let _ = events.try_send(AgentEvent::TerminalOutput {
                terminal_id: id.to_string(),
                data: text,
                replace: false,
            });
        }
        if n == 0 {
            return;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn request(script: &str, limit: u64) -> CreateTerminalRequest {
        serde_json::from_value(serde_json::json!({
            "sessionId": "test", "command": "/bin/sh", "args": ["-c", script],
            "outputByteLimit": limit
        }))
        .expect("request")
    }

    async fn run(request: CreateTerminalRequest) -> (String, Exit) {
        let dir = tempfile::tempdir().unwrap();
        let (events, _) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let id = terminals.create(&request).unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(5), terminals.wait_for_exit(&id))
            .await
            .unwrap()
            .unwrap();
        (terminals.output(&id).unwrap().0, exit)
    }

    #[tokio::test]
    async fn whole_shell_line_runs_and_streams() {
        let request = CreateTerminalRequest::new("test", "echo a && echo b");
        let (output, exit) = run(request).await;
        assert_eq!(output, "a\nb\n");
        assert_eq!(exit.code, Some(0));
    }

    #[tokio::test]
    async fn symlinked_workspace_runs_git_against_a_disposable_local_remote() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap().join("real workspace");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("linked workspace");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let (events, received) = async_channel::unbounded();
        let terminals = Terminals::new(&link, events, "test");
        // Push a synthetic blob tag, not a commit: no author configuration,
        // credentials, network remote or existing repository is involved.
        let request = serde_json::from_value(serde_json::json!({
            "sessionId": "test", "cwd": real,
            "command": "set -eu\ngit init -q source\ngit init -q --bare remote.git\nprintf 'local fixture' > source/payload\nblob=$(git -C source hash-object -w payload)\ngit -C source update-ref refs/tags/flint-terminal-fixture \"$blob\"\ngit -C source push -q ../remote.git refs/tags/flint-terminal-fixture\ntest \"$(git --git-dir=remote.git rev-parse refs/tags/flint-terminal-fixture)\" = \"$blob\"\nprintf 'local git push ok\\n'",
            "env": [
                {"name": "GIT_CONFIG_GLOBAL", "value": "/dev/null"},
                {"name": "GIT_CONFIG_SYSTEM", "value": "/dev/null"},
                {"name": "GIT_CONFIG_COUNT", "value": "0"},
                {"name": "GIT_TERMINAL_PROMPT", "value": "0"},
                {"name": "SHELL", "value": "/bin/sh"}
            ]
        })).unwrap();
        let id = terminals.create(&request).unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(10), terminals.wait_for_exit(&id))
            .await
            .unwrap()
            .unwrap();
        let output = terminals.output(&id).unwrap().0;
        assert_eq!(exit.code, Some(0), "{output}");
        assert!(output.contains("local git push ok"), "{output}");
        assert!(std::iter::from_fn(|| received.try_recv().ok()).any(
            |event| matches!(event, AgentEvent::TerminalOutput { data, .. }
            if data.contains("local git push ok"))
        ));
    }

    #[tokio::test]
    async fn multiline_heredoc_runs() {
        let request = CreateTerminalRequest::new("test", "cat <<'END'\na\nb\nEND");
        let (output, exit) = run(request).await;
        assert_eq!(output, "a\nb\n");
        assert_eq!(exit.code, Some(0));
    }

    #[tokio::test]
    async fn shell_scripts_longer_than_a_filename_still_run() {
        let script = format!("cat <<'END'\n{}\nEND", "long content ".repeat(100));
        let (output, exit) = run(CreateTerminalRequest::new("test", script)).await;
        assert_eq!(output, format!("{}\n", "long content ".repeat(100)));
        assert_eq!(exit.code, Some(0));
    }

    #[tokio::test]
    async fn explicit_arguments_are_never_shell_interpreted() {
        let request = CreateTerminalRequest::new("test", "echo")
            .args(vec!["hi; echo injected".into(), "$HOME".into()]);
        let (output, exit) = run(request).await;
        assert_eq!(output, "hi; echo injected $HOME\n");
        assert_eq!(exit.code, Some(0));
    }

    #[tokio::test]
    async fn literal_executable_path_with_spaces_stays_direct() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("space and $dollar");
        std::fs::write(&program, "#!/bin/sh\nprintf literal").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (output, exit) = run(CreateTerminalRequest::new(
            "test",
            program.to_str().unwrap(),
        ))
        .await;
        assert_eq!(output, "literal");
        assert_eq!(exit.code, Some(0));
    }

    #[tokio::test]
    async fn missing_program_and_cwd_have_clear_errors_without_start_events() {
        let dir = tempfile::tempdir().unwrap();
        let (events, received) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let error = terminals
            .create(&CreateTerminalRequest::new("test", "flint-nonexistent-bin"))
            .unwrap_err();
        assert!(
            error.contains("cannot run \"flint-nonexistent-bin\""),
            "{error}"
        );
        assert!(
            error.contains(&format!("cwd: {}", dir.path().display())),
            "{error}"
        );
        let error = terminals
            .create(&CreateTerminalRequest::new("test", "echo ok").cwd(dir.path().join("missing")))
            .unwrap_err();
        assert!(error.contains("not a directory"), "{error}");
        assert!(received.try_recv().is_err());
    }

    #[tokio::test]
    async fn request_environment_and_invalid_shell_fallback_are_respected() {
        let request = serde_json::from_value(serde_json::json!({
            "sessionId": "test", "command": "printf '%s' \"$FLINT_TEST\"",
            "env": [{"name": "FLINT_TEST", "value": "literal value"},
                    {"name": "SHELL", "value": "/flint-nonexistent-shell"}],
        }))
        .unwrap();
        let (output, exit) = run(request).await;
        assert_eq!(output, "literal value");
        assert_eq!(exit.code, Some(0));
    }

    #[test]
    fn terminal_diagnostics_do_not_log_values() {
        let request: CreateTerminalRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "test", "command": "echo script-secret\nnext",
            "args": ["argument-secret"], "env": [{"name": "TOKEN", "value": "env-secret"}],
        }))
        .unwrap();
        let diagnostic = request_diagnostic(&request);
        assert!(diagnostic.contains("multiline=true") && diagnostic.contains("TOKEN"));
        let reason = diagnostic_reason(
            &request,
            "echo script-secret\nnext argument-secret env-secret",
        );
        let escaped = diagnostic_reason(&request, &format!("cannot run {:?}", request.command));
        for secret in ["script-secret", "argument-secret", "env-secret"] {
            assert!(!diagnostic.contains(secret));
            assert!(!reason.contains(secret));
            assert!(!escaped.contains(secret));
        }
    }

    #[tokio::test]
    async fn inherited_pipe_does_not_hold_exit_open() {
        let dir = tempfile::tempdir().unwrap();
        let (events, _) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let id = terminals
            .create(&request("sleep 2 & printf done", 1024))
            .unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(1), terminals.wait_for_exit(&id))
            .await
            .expect("leader exit does not wait on inherited pipes")
            .unwrap();
        assert_eq!(exit.code, Some(0));
        assert_eq!(terminals.output(&id).unwrap().0, "done");
        terminals.release(&id).unwrap();
    }

    #[tokio::test]
    async fn kill_escalates_for_term_ignoring_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let (events, _) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let id = terminals.create(&request(
            "trap '' TERM; (trap '' TERM; sleep 2; printf escaped > escaped) & echo ready; wait",
            1024,
        )).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !terminals.output(&id).unwrap().0.contains("ready") {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        terminals.kill(&id).unwrap();
        tokio::time::timeout(Duration::from_secs(3), terminals.wait_for_exit(&id))
            .await
            .expect("kill completes")
            .unwrap();
        // The intentionally broken baseline is safe too: the descendant exits
        // by itself after two seconds instead of leaving an infinite process.
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert!(
            !dir.path().join("escaped").exists(),
            "owned descendant survived kill"
        );
    }

    #[tokio::test]
    async fn release_stops_the_owned_group_before_forgetting_it() {
        let dir = tempfile::tempdir().unwrap();
        let (events, received) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let id = terminals.create(&request(
            "trap '' TERM; (trap '' TERM; sleep 2; printf escaped > escaped) & echo ready; wait", 1024,
        )).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !terminals.output(&id).unwrap().0.contains("ready") {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        terminals.release(&id).unwrap();
        assert!(terminals.output(&id).is_err());
        tokio::time::timeout(Duration::from_secs(3), async {
            while let Ok(event) = received.recv().await {
                if matches!(event, AgentEvent::TerminalExited { .. }) {
                    break;
                }
            }
        })
        .await
        .expect("release still delivers exit");
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert!(
            !dir.path().join("escaped").exists(),
            "owned descendant survived release"
        );
    }

    #[tokio::test]
    async fn split_utf8_and_eof_tail_are_decoded_per_pipe() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let (_exit_tx, exit) = watch::channel(None);
        let running = Arc::new(Running {
            output: Mutex::new((String::new(), false)),
            limit: 1024,
            exit,
            kill: Mutex::new(None),
        });
        let (events, received) = async_channel::unbounded();
        let pumped = Arc::clone(&running);
        let task = tokio::spawn(async move { pump(Some(reader), &pumped, &events, "test").await });
        use tokio::io::AsyncWriteExt;
        // A complete ASCII prefix gives a deterministic read barrier before
        // supplying the rest of the split multibyte sequence.
        writer.write_all(b"x\xe7").await.unwrap();
        let first = received.recv().await.unwrap();
        assert!(matches!(first, AgentEvent::TerminalOutput { data, .. } if data == "x"));
        writer.write_all(b"\x95\x8c\xf0\x9f").await.unwrap();
        writer.shutdown().await.unwrap();
        task.await.unwrap();
        assert_eq!(running.output.lock().unwrap().0, "x界\u{fffd}");
    }

    #[tokio::test]
    async fn requested_output_limit_has_a_hard_storage_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let (events, _) = async_channel::unbounded();
        let terminals = Terminals::new(dir.path(), events, "test");
        let id = terminals
            .create(&request("printf small", u64::MAX))
            .unwrap();
        assert!(terminals.get(&id).unwrap().limit <= DEFAULT_OUTPUT_LIMIT);
        terminals.wait_for_exit(&id).await.unwrap();
    }
}
