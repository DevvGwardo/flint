//! Claude Code, Codex and other agents as flint sessions, over the Agent
//! Client Protocol (JSON-RPC on the adapter's stdio).
//!
//! [`spawn_acp_session`] returns the same [`SessionHandle`] as flint's own
//! engine: the app sends [`flint_agent::Op`]s and renders
//! [`flint_agent::AgentEvent`]s without knowing which agent runs. The agent
//! runs its own loop, so flint's harness nudges don't apply here.

mod files;
mod launch;
mod live;
mod mapper;
mod options;
mod runner;
mod saved;
#[cfg(test)]
#[path = "session_options_tests.rs"]
mod session_options_tests;
mod shared;
mod terminal_meta;
mod terminals;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::SessionHandle;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

pub use launch::AcpAgent;

/// Adapter stderr kept for error messages.
const STDERR_TAIL_CHARS: usize = 4_000;

/// Where and how an ACP session runs.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub workspace: PathBuf,
    /// Holds `acp.json` (the agent's session id) so a reopened session can
    /// continue with `session/load`.
    pub session_dir: Option<PathBuf>,
    /// `Auto` answers the agent's permission requests with "allow";
    /// otherwise each one becomes an approval in the UI.
    pub approval: ApprovalMode,
    /// Ask the agent to report each command's terminal (the ACP
    /// "terminal output" extension), shown as read-only terminal tabs.
    /// Claude Code's adapter then sends command output only that way, which
    /// flint also shows on the tool card.
    pub agent_terminals: bool,
}

/// Starts the agent's ACP adapter in the workspace and returns the session's
/// channels. Startup problems arrive as `AgentEvent::Error` with the fix.
pub fn spawn_acp_session(agent: AcpAgent, mut config: AcpConfig) -> SessionHandle {
    if let Ok(canonical) = config.workspace.canonicalize() {
        config.workspace = canonical;
    }
    let (ops_tx, ops_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("flint-acp".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = events_tx.try_send(AgentEvent::Error(format!(
                        "cannot start the ACP runtime: {err}"
                    )));
                    return;
                }
            };
            runtime.block_on(run_process(agent, config, ops_rx, events_tx));
        });
    if let Err(err) = spawned {
        eprintln!("flint: cannot start the ACP session thread: {err}");
    }
    SessionHandle {
        ops: ops_tx,
        events: events_rx,
    }
}

async fn run_process(
    agent: AcpAgent,
    config: AcpConfig,
    ops: async_channel::Receiver<flint_agent::Op>,
    events: async_channel::Sender<AgentEvent>,
) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = launch::search_path(home.as_deref());
    let (program, args) = match agent.command(&path) {
        Ok(command) => command,
        Err(message) => {
            let _ = events.try_send(AgentEvent::Error(message));
            return;
        }
    };
    let mut command = Command::new(&program);
    command
        .args(&args)
        .current_dir(&config.workspace)
        .env("PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let keep_api_key = std::env::var_os("FLINT_CLAUDE_USE_API_KEY").is_some();
    for (name, _) in std::env::vars_os() {
        if let Some(name) = name.to_str()
            && agent.drops_env(name, keep_api_key)
        {
            command.env_remove(name);
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = events.try_send(AgentEvent::Error(format!(
                "Couldn't start {} ({}: {err}). {}",
                agent.name(),
                program.display(),
                agent.install_hint()
            )));
            return;
        }
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = events.try_send(AgentEvent::Error(format!(
            "{} started without stdio pipes",
            agent.name()
        )));
        return;
    };
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let tail = Arc::clone(&stderr_tail);
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut tail) = tail.lock() {
                    push_tail(&mut tail, &line);
                }
            }
        });
    }
    let pid = child.id();
    let context = runner::RunContext {
        agent: agent.clone(),
        workspace: config.workspace,
        session_dir: config.session_dir,
        approval: config.approval,
        agent_terminals: config.agent_terminals,
        ops,
        events: events.clone(),
        stderr: Arc::clone(&stderr_tail),
    };
    runner::run(stdout.compat(), stdin.compat_write(), context).await;
    // The session is over (shutdown or the agent went away): stop the
    // adapter and anything it started.
    #[cfg(unix)]
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pid;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;
    let _ = child.kill().await;
}

/// Appends a stderr line, keeping only the last [`STDERR_TAIL_CHARS`].
fn push_tail(tail: &mut String, line: &str) {
    tail.push_str(line);
    tail.push('\n');
    if tail.len() > STDERR_TAIL_CHARS {
        let cut = tail.len() - STDERR_TAIL_CHARS;
        let cut = (cut..tail.len())
            .find(|i| tail.is_char_boundary(*i))
            .unwrap_or(0);
        tail.drain(..cut);
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
