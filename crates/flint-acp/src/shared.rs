//! State shared by a connection's request handlers and its session loop:
//! the update mapper, parked permission requests, the agent's options and
//! the terminals flint runs for it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use agent_client_protocol::Responder;
use agent_client_protocol::schema::v1::PermissionOption;
use agent_client_protocol::schema::v1::PermissionOptionKind;
use agent_client_protocol::schema::v1::RequestPermissionOutcome;
use agent_client_protocol::schema::v1::RequestPermissionResponse;
use agent_client_protocol::schema::v1::SelectedPermissionOutcome;
use agent_client_protocol::schema::v1::SessionConfigOption;
use async_channel::Sender;
use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;

use crate::launch::AcpAgent;
use crate::mapper::Mapper;
use crate::options::Options;
use crate::terminals::Terminals;

type PendingPermission = (Responder<RequestPermissionResponse>, Vec<PermissionOption>);

/// State shared by the connection's handlers and the session loop.
pub(crate) struct Shared {
    pub agent: AcpAgent,
    pub workspace: PathBuf,
    pub events: Sender<AgentEvent>,
    pub stderr: Arc<Mutex<String>>,
    pub mapper: Mutex<Mapper>,
    pub pending: Mutex<HashMap<String, PendingPermission>>,
    pub auto_approve: AtomicBool,
    /// `session/load` replays history as updates; the UI already has it.
    pub loading: AtomicBool,
    pub options: Mutex<Options>,
    pub options_changed: tokio::sync::watch::Sender<u64>,
    pub terminals: Terminals,
    /// Opt in to the "terminal output" extension.
    pub agent_terminals: bool,
    pub shutdown: tokio::sync::watch::Receiver<bool>,
    pub interrupts: tokio::sync::watch::Sender<u64>,
    pub interrupts_seen: std::sync::atomic::AtomicU64,
}

impl Shared {
    pub fn interrupt_pending(&self) -> bool {
        *self.interrupts.borrow() > self.interrupts_seen.load(Ordering::Relaxed)
    }

    /// Non-prompt RPCs must not block Shutdown or wait forever on a peer.
    pub async fn rpc<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, agent_client_protocol::Error>>,
    ) -> Result<T, agent_client_protocol::Error> {
        let mut shutdown = self.shutdown.clone();
        let mut interrupts = self.interrupts.subscribe();
        let seen = self.interrupts_seen.load(Ordering::Relaxed);
        tokio::select! {
            biased;
            _ = shutdown.wait_for(|stopped| *stopped) =>
                Err(agent_client_protocol::Error::new(-32000, "ACP session is shutting down")),
            _ = interrupts.wait_for(|epoch| *epoch > seen) =>
                Err(agent_client_protocol::Error::new(-32000, "ACP request interrupted")),
            result = tokio::time::timeout(std::time::Duration::from_secs(30), future) =>
                result.unwrap_or_else(|_| Err(agent_client_protocol::Error::new(-32000, "ACP request timed out after 30 seconds"))),
        }
    }

    pub fn emit_all(&self, events: Vec<AgentEvent>) {
        for event in events {
            let _ = self.events.try_send(event);
        }
    }

    pub fn with_mapper<T>(&self, f: impl FnOnce(&mut Mapper) -> T) -> Option<T> {
        self.mapper.lock().ok().map(|mut mapper| f(&mut mapper))
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn options(&self) -> Options {
        self.options.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// Stores the agent's options and tells the UI. When the agent has a
    /// permission mode, that mode decides approvals: flint stops answering
    /// permission requests on its own and shows each one.
    pub fn set_options(&self, raw: &[SessionConfigOption]) {
        let options = Options::from_acp(raw);
        let first_mode = options.has_mode() && !self.options().has_mode();
        if first_mode {
            self.auto_approve.store(false, Ordering::Relaxed);
        }
        let list = options.list.clone();
        if let Ok(mut slot) = self.options.lock() {
            *slot = options;
        }
        self.options_changed
            .send_modify(|epoch| *epoch = epoch.wrapping_add(1));
        self.emit_all(vec![AgentEvent::SessionOptions(list)]);
    }

    /// Answers a pending permission with the option matching `decision`.
    pub fn resolve(&self, call_id: &str, decision: ApprovalDecision) {
        let Some((responder, options)) =
            self.pending.lock().ok().and_then(|mut p| p.remove(call_id))
        else {
            return;
        };
        if decision == ApprovalDecision::ApproveAlways {
            self.auto_approve.store(true, Ordering::Relaxed);
        }
        let _ = responder.respond(permission_response(&options, decision));
    }

    /// Cancels every unanswered permission (the turn is being cancelled).
    pub fn cancel_pending(&self) {
        let pending: Vec<PendingPermission> = self
            .pending
            .lock()
            .map(|mut p| p.drain().map(|(_, v)| v).collect())
            .unwrap_or_default();
        for (responder, _) in pending {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
        }
    }
}

/// The ACP outcome for a flint decision: the closest option the agent offered.
pub(crate) fn permission_response(
    options: &[PermissionOption],
    decision: ApprovalDecision,
) -> RequestPermissionResponse {
    let preference: &[PermissionOptionKind] = match decision {
        ApprovalDecision::Approve => &[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ],
        ApprovalDecision::ApproveAlways => &[
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::AllowOnce,
        ],
        ApprovalDecision::Deny => &[
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ],
    };
    let chosen = preference
        .iter()
        .find_map(|kind| options.iter().find(|option| option.kind == *kind));
    let outcome = match chosen {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    };
    RequestPermissionResponse::new(outcome)
}
