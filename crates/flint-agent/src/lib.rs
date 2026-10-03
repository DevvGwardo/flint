//! flint's agent engine: a Chat Completions agent loop with local tools and
//! the harness (loop detection, verify-before-done, zero-edit watchdog,
//! tool-call repair, optional JEV judge). No UI code lives here.

mod approvals;
#[cfg(test)]
mod bench_tests;
pub mod config;
mod context;
pub mod harness;
mod persist;
pub mod prompt;
pub mod protocol;
pub mod provider;
mod session;
mod subagents;
pub mod tools;

pub use protocol::*;
pub use session::MAX_STEPS_PER_TURN;

/// Both ends of a running session.
pub struct SessionHandle {
    pub ops: async_channel::Sender<Op>,
    pub events: async_channel::Receiver<AgentEvent>,
}

/// Starts a session on its own thread (with its own tokio runtime) and
/// returns its channels. The session ends on [`Op::Shutdown`] or when every
/// `ops` sender is dropped.
pub fn spawn_session(mut config: AgentConfig) -> SessionHandle {
    if let Ok(canonical) = config.workspace.canonicalize() {
        config.workspace = canonical;
    }
    let (ops_tx, ops_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("flint-session".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = events_tx.try_send(AgentEvent::Error(format!(
                        "cannot start the agent runtime: {err}"
                    )));
                    return;
                }
            };
            runtime.block_on(session::run(config, ops_rx, events_tx));
        });
    if let Err(err) = spawned {
        // The receiver is still returned; the front end sees a closed stream.
        eprintln!("flint: cannot start the session thread: {err}");
    }
    SessionHandle {
        ops: ops_tx,
        events: events_rx,
    }
}
