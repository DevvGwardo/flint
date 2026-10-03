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

/// Output kept when the agent sets no limit.
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

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
        let mut command = Command::new(&request.command);
        command
            .args(&request.args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for var in &request.env {
            command.env(&var.name, &var.value);
        }
        let mut child = command
            .spawn()
            .map_err(|err| format!("cannot run {}: {err}", request.command))?;
        let id = format!("flint-term-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
        let running = Arc::new(Running {
            output: Mutex::new((String::new(), false)),
            limit: request.output_byte_limit.map_or(DEFAULT_OUTPUT_LIMIT, |l| {
                usize::try_from(l).unwrap_or(usize::MAX)
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
            let pumps = async {
                tokio::join!(
                    pump(stdout, &running, &events, &terminal_id),
                    pump(stderr, &running, &events, &terminal_id)
                )
            };
            let status = tokio::select! {
                (_, status) = async { (pumps.await, child.wait().await) } => status.ok(),
                _ = kill_rx => {
                    let _ = child.kill().await;
                    child.wait().await.ok()
                }
            };
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
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
        if let Ok(mut output) = running.output.lock() {
            output.0.push_str(&text);
            if output.0.len() > running.limit {
                let mut cut = output.0.len() - running.limit;
                while !output.0.is_char_boundary(cut) {
                    cut += 1;
                }
                output.0.drain(..cut);
                output.1 = true;
            }
        }
        let _ = events.try_send(AgentEvent::TerminalOutput {
            terminal_id: id.to_string(),
            data: text,
            replace: false,
        });
    }
}
