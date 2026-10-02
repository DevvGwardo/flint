//! flint's agent engine: a Chat Completions agent loop with local tools and
//! the flash harness (loop detection, verify-before-done, zero-edit watchdog,
//! tool-call repair, optional JEV judge). No UI code lives here.

pub mod protocol;

pub use protocol::*;

/// Both ends of a running session.
pub struct SessionHandle {
    pub ops: async_channel::Sender<Op>,
    pub events: async_channel::Receiver<AgentEvent>,
}

/// Starts a session on its own thread and returns its channels.
pub fn spawn_session(config: AgentConfig) -> SessionHandle {
    let (ops_tx, _ops_rx) = async_channel::unbounded();
    let (_events_tx, events_rx) = async_channel::unbounded();
    let _ = config;
    // Implemented by the engine work (session.rs).
    SessionHandle {
        ops: ops_tx,
        events: events_rx,
    }
}
