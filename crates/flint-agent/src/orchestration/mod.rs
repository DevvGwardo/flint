//! B1 durable acceptance foundation. No provider execution is enabled.
mod contracts;
#[cfg_attr(windows, path = "fs_windows.rs")]
pub(crate) mod fs;
mod identity;
pub(crate) mod store;
#[cfg(test)]
mod tests;

use crate::watch::PrWatchSource;
use crate::{AgentConfig, ApprovalDecision, Op};
pub use contracts::*;
pub use identity::*;
use std::io::Read;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    pub code: EngineErrorCode,
    pub message: String,
}
impl EngineError {
    pub(crate) fn new(code: EngineErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub(crate) fn unsupported() -> Self {
        Self::new(
            EngineErrorCode::Unsupported,
            "This operation is not enabled by the B1 acceptance slice.",
        )
    }
}
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for EngineError {}

pub struct ResolvedNative {
    pub target: ProviderTarget,
    pub base_url: String,
    pub credential: crate::secrets::SecretBytes,
    pub capabilities: ProviderCapabilities,
}
pub enum ResolvedTarget {
    Native(Box<ResolvedNative>),
    External,
}
pub struct ExternalStart {
    pub conversation: ConversationId,
    pub target: ProviderTarget,
    pub workspace: std::path::PathBuf,
    pub session_dir: std::path::PathBuf,
    pub policy: InheritedPolicy,
    pub events: ExternalEventSink,
}
#[derive(Clone)]
pub struct ExternalEventSink {
    _private: (),
}
impl ExternalEventSink {
    pub async fn record(&self, _: crate::AgentEvent) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
    pub fn progress(&self, _: ProgressUpdate) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn rich(&self, _: crate::rich::RichOutput) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn intent(&self, _: SideEffectIntent) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
}
pub struct ExternalHandle {
    pub conversation: Arc<dyn ExternalConversation>,
    pub capabilities: ProviderCapabilities,
}
pub struct HostServices {
    pub profiles: Arc<dyn TargetResolver>,
    pub external: Option<Arc<dyn ExternalAgentBridge>>,
    pub pr_watch: Option<Arc<dyn PrWatchSource>>,
    pub destinations: Option<Arc<dyn DestinationRegistry>>,
}

type Request = Box<dyn FnOnce(&mut store::Store) + Send>;
#[derive(Clone)]
pub struct CommandClient {
    tx: async_channel::Sender<Request>,
}
#[derive(Clone)]
pub struct ControlClient {
    commands: async_channel::Sender<Request>,
    stopping: Arc<AtomicBool>,
    done: tokio::sync::watch::Receiver<bool>,
}
pub struct OrchestrationHandle {
    pub ops: async_channel::Sender<Op>,
    pub commands: CommandClient,
    pub controls: ControlClient,
    pub events: async_channel::Receiver<EngineEvent>,
    pub secrets: crate::secrets::SecretInbox,
}

impl CommandClient {
    async fn query<T: Send + 'static>(
        &self,
        action: impl FnOnce(&mut store::Store) -> Result<T, EngineError> + Send + 'static,
    ) -> Result<T, EngineError> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.tx
            .send(Box::new(move |store| {
                let _ = reply.send(action(store));
            }))
            .await
            .map_err(|_| {
                EngineError::new(
                    EngineErrorCode::Cancelled,
                    "Engine acceptance client is closed.",
                )
            })?;
        result.await.map_err(|_| {
            EngineError::new(
                EngineErrorCode::Cancelled,
                "Engine acceptance reply was disconnected.",
            )
        })?
    }
    pub async fn submit(&self, command: EngineCommand) -> Result<CommandReceipt, EngineError> {
        // Bound captured inputs before placing them in the bounded actor inbox,
        // rather than allocating an arbitrarily large canonical envelope.
        if let Err(error) = store::validate_intake(&command) {
            // Even unsupported/oversized replacements cannot reuse an
            // already accepted identity under a different payload.
            if self.receipt(&command.id).await?.is_some() {
                return Err(EngineError::new(
                    EngineErrorCode::IdentityConflict,
                    "Accepted command identity is immutable.",
                ));
            }
            return Err(error);
        }
        self.query(move |store| store.submit(command)).await
    }
    pub async fn receipt(&self, id: &CommandId) -> Result<Option<CommandReceipt>, EngineError> {
        let id = id.clone();
        self.query(move |store| store.receipt(&id)).await
    }
    pub async fn queue(
        &self,
        conversation: &ConversationId,
    ) -> Result<QueueProjection, EngineError> {
        let conversation = conversation.clone();
        self.query(move |store| store.queue(&conversation)).await
    }
    pub async fn queued_prompt(&self, id: &CommandId) -> Result<PromptSnapshot, EngineError> {
        let id = id.clone();
        self.query(move |store| store.queued_prompt(&id)).await
    }
    pub async fn recent_receipts(
        &self,
        before_sequence: Option<u64>,
        limit: usize,
    ) -> Result<ReceiptPage, EngineError> {
        self.query(move |store| store.recent(before_sequence, limit))
            .await
    }
    pub async fn summary(
        &self,
        conversation: &ConversationId,
    ) -> Result<ConversationSummary, EngineError> {
        let conversation = conversation.clone();
        self.query(move |store| store.summary(&conversation)).await
    }
    pub async fn records(&self, _: u64, _: usize) -> Result<RecordPage, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn acknowledge_records(&self, _: u64) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn history(&self, request: HistoryPageRequest) -> Result<HistoryPage, EngineError> {
        if request.limit == 0
            || request.limit > 100
            || request.max_bytes == 0
            || request.max_bytes > 256 * 1024
        {
            return Err(EngineError::new(
                EngineErrorCode::InvalidCommand,
                "History page exceeds its bounds.",
            ));
        }
        Err(EngineError::unsupported())
    }
    pub async fn resolve_citation(&self, _: Citation) -> Result<HistoryExcerpt, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn task(&self, _: &TaskHandle) -> Result<TaskStatus, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn wait(&self, _: WaitRequest) -> Result<WaitResult, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn confirm_queue_continuation(
        &self,
        _: ConversationId,
        _: TaskRevision,
        _: CommandId,
        _: u64,
    ) -> Result<QueueContinuationPermit, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn resume_one(
        &self,
        _: QueueContinuationPermit,
    ) -> Result<CommandReceipt, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn prepare_transfer(
        &self,
        _: &TransferPreviewSpec,
    ) -> Result<TransferPreview, EngineError> {
        Err(EngineError::unsupported())
    }
}
impl ControlClient {
    pub async fn stop(&self, _: StopScope) -> Result<StopAck, EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn approve(
        &self,
        _: ApprovalAddress,
        _: ApprovalDecision,
    ) -> Result<(), EngineError> {
        Err(EngineError::unsupported())
    }
    pub async fn shutdown(&self) -> Result<(), EngineError> {
        self.stopping.store(true, Ordering::SeqCst);
        self.commands.close();
        let mut done = self.done.clone();
        done.wait_for(|done| *done).await.map_err(|_| {
            EngineError::new(
                EngineErrorCode::Storage,
                "Acceptance worker stopped without a shutdown acknowledgement.",
            )
        })?;
        Ok(())
    }
}

/// Opens acceptance storage only. Neither config nor host services are used
/// to resolve credentials, start processes, contact providers, or schedule work.
pub fn spawn_orchestrated_session(
    config: AgentConfig,
    options: OrchestrationOptions,
    _services: HostServices,
) -> Result<OrchestrationHandle, EngineError> {
    let dir = config.session_dir.ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InvalidCommand,
            "Durable acceptance requires a session directory.",
        )
    })?;
    open_client(&dir, options)
}

/// B1 storage entry point for hosts that intentionally do not start an executor.
pub fn open_acceptance_session(
    dir: &Path,
    options: OrchestrationOptions,
) -> Result<OrchestrationHandle, EngineError> {
    open_client(dir, options)
}
fn open_client(
    dir: &Path,
    options: OrchestrationOptions,
) -> Result<OrchestrationHandle, EngineError> {
    let inbox = options.limits.command_inbox;
    let store = store::Store::open(dir, options)?;
    let (tx, rx) = async_channel::bounded::<Request>(inbox);
    let (ops, incoming_ops) = async_channel::bounded::<Op>(1);
    let (events_tx, events) = async_channel::bounded::<EngineEvent>(1);
    // There are no B1 execution events. Receipts are authoritative replies and
    // query projections; no background lossy receipt broadcast is invented.
    drop(events_tx);
    // Legacy ops fail visibly as a closed channel in this nonexecuting surface.
    incoming_ops.close();
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let (finished, done) = tokio::sync::watch::channel(false);
    std::thread::Builder::new()
        .name("flint-acceptance".into())
        .spawn(move || {
            let mut store = store;
            while let Ok(action) = rx.recv_blocking() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                action(&mut store);
                if stop.load(Ordering::SeqCst) {
                    break;
                }
            }
            // Closing async-channel alone retains queued callbacks while a
            // sender exists. Drop them explicitly so cancelled replies settle.
            rx.close();
            while rx.try_recv().is_ok() {}
            drop(store);
            let _ = finished.send(true);
        })
        .map_err(|_| {
            EngineError::new(
                EngineErrorCode::Storage,
                "Couldn't start the acceptance worker.",
            )
        })?;
    Ok(OrchestrationHandle {
        ops,
        commands: CommandClient { tx: tx.clone() },
        controls: ControlClient {
            commands: tx,
            stopping,
            done,
        },
        events,
        secrets: crate::secrets::SecretInbox { _private: () },
    })
}

pub fn read_conversation_summary(dir: &Path) -> Result<Option<ConversationSummary>, EngineError> {
    // Ignore legacy transcript bodies when building the lightweight projection.
    #[derive(serde::Deserialize)]
    struct Projection {
        version: u64,
        turn_id: u64,
        #[serde(default)]
        task: Option<crate::TaskRecord>,
        orchestration: Option<store::State>,
    }
    let directory = fs::Directory::open(dir).map_err(store::storage)?;
    let file = match directory.reader("history.json", store::MAX_SNAPSHOT_BYTES) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(store::storage(error)),
    };
    let projection: Projection = serde_json::from_reader(std::io::BufReader::new(
        file.take(store::MAX_SNAPSHOT_BYTES as u64 + 1),
    ))
    .map_err(|_| {
        EngineError::new(
            EngineErrorCode::CorruptState,
            "Couldn't read the engine summary. History has been kept.",
        )
    })?;
    if !matches!(projection.version, 1..=3) {
        return Err(EngineError::new(
            EngineErrorCode::CorruptState,
            "Unsupported history version.",
        ));
    }
    let Some(state) = projection.orchestration else {
        if projection.version == 3 {
            return Err(EngineError::new(
                EngineErrorCode::CorruptState,
                "Missing v3 orchestration.",
            ));
        }
        return Ok(None);
    };
    store::validate_lineage(&directory, &state)?;
    if let Some(task) = &projection.task {
        task.validate().map_err(|_| {
            EngineError::new(EngineErrorCode::CorruptState, "Invalid task projection.")
        })?;
    }
    store::summary_from(
        &serde_json::json!({"turn_id":projection.turn_id,"task":projection.task}),
        &state,
    )
    .map(Some)
}

pub fn read_agent_record(
    _: &Path,
    _: &AgentRecordPayload,
    _: usize,
) -> Result<crate::AgentEvent, EngineError> {
    Err(EngineError::unsupported())
}
