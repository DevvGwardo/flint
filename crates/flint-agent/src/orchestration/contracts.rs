//! Frozen additive contracts. Later execution surfaces remain unsupported in B1.

use super::{
    ChildId, CommandId, EngineError, ExternalHandle, ExternalStart, ResolvedTarget, RunId,
    SessionId,
};
use crate::watch::WatchCommand;
use crate::{
    AgentEvent, ApprovalMode, ImageAttachment, ReasoningEffort, RequirementSpec, TaskContract,
    TaskEdit, TaskMode, TaskOutcome, TaskRecord, TaskReview, TurnEndReason, Usage,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProfileRef {
    pub id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Digest(pub [u8; 32]); // SHA-256, algorithm versioned on disk.

impl Digest {
    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = Vec::with_capacity(64);
        for &b in &self.0 {
            bytes.push(HEX[(b >> 4) as usize]);
            bytes.push(HEX[(b & 0x0f) as usize]);
        }
        // Safety: HEX table only contains valid ASCII hex digits '0'-'9', 'a'-'f'
        unsafe { String::from_utf8_unchecked(bytes) }
    }
}
impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}
impl AsRef<[u8; 32]> for Digest {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}
impl From<[u8; 32]> for Digest {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

impl From<TaskEdit> for ScopeEdit {
    fn from(edit: TaskEdit) -> Self {
        Self {
            task_id: edit.task_id,
            expected_revision: edit.expected_revision,
            objective: edit.objective,
            mode: edit.mode,
            requirements: edit.requirements,
            restrictions: edit.restrictions,
            clear_reported_blockers: edit.clear_reported_blockers,
        }
    }
}
impl From<ScopeEdit> for TaskEdit {
    fn from(edit: ScopeEdit) -> Self {
        Self {
            task_id: edit.task_id,
            expected_revision: edit.expected_revision,
            objective: edit.objective,
            mode: edit.mode,
            requirements: edit.requirements,
            restrictions: edit.restrictions,
            clear_reported_blockers: edit.clear_reported_blockers,
        }
    }
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            queued_commands: 20,
            queued_bytes: 32 * 1024 * 1024,
            command_inbox: 32,
            record_delivery: 256,
            progress_keys: 128,
            progress_bytes: 1024 * 1024,
            outbox_bytes: 8 * 1024 * 1024,
            active_children: 4,
            retained_children: 32,
            history_page_rows: 100,
            history_page_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowId {
    pub conversation: ConversationId,
    pub ordinal: u64, // Never reused/reindexed after compaction.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationId {
    pub root: SessionId,
    pub child: Option<ChildId>, // Single delegation level.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptSnapshot {
    pub request: String,        // Exact authoritative user instruction.
    pub effective_text: String, // Frozen attached/context text.
    pub images: Vec<ImageAttachment>,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Delivery {
    NextTurn,
    NativeSafeBoundary,
    StopThenSend, // Explicit; not ACP "steering."
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskRevision {
    pub turn_id: u64,
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScopeEdit {
    pub task_id: u64,
    pub expected_revision: u64,
    pub objective: String,
    pub mode: TaskMode,
    pub requirements: Vec<RequirementSpec>,
    pub restrictions: Vec<String>,
    pub clear_reported_blockers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EngineCommand {
    pub id: CommandId,
    pub conversation: ConversationId,
    pub payload: CommandPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommandPayload {
    Send {
        prompt: PromptSnapshot,
        delivery: Delivery,
    },
    ContinueTask {
        task_id: u64,
        expected_revision: u64,
    },
    EditTask(ScopeEdit),
    ReviewTask(TaskReview),
    CancelQueued {
        command_id: CommandId,
        expected_queue_revision: u64,
    },
    ReplaceQueued {
        command_id: CommandId,
        expected_queue_revision: u64,
        replacement: Box<EngineCommand>, // New identity, same conversation.
    },
    MoveQueued {
        command_id: CommandId,
        before: Option<CommandId>,
        expected_queue_revision: u64,
    },
    PauseQueue {
        expected_queue_revision: u64,
    },
    ResumeQueue {
        expected_queue_revision: u64,
        restart_generation: u64,
    },
    SetOption {
        id: String,
        value: String,
    },
    StartChild(ChildSpec),
    Transfer(TransferSpec),
    UndoLastTurn {
        expected_turn_id: u64,
    },
    Watch(WatchCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandReceipt {
    pub id: CommandId,
    pub conversation: ConversationId,
    pub task: Option<TaskHandle>, // Includes reserved child address for StartChild.
    pub payload_digest: Digest,
    pub accepted_sequence: u64,
    pub accepted_revision: u64,
    pub status_revision: u64,
    pub state: CommandState,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommandState {
    Accepted,         // Payload durable; not started.
    Held(HoldReason), // Still accepted, deliberately not scheduled.
    Started {
        run_id: RunId,
        turn_id: u64,
        safe_boundary_step: Option<u32>,
    },
    Finished {
        run_id: Option<RunId>, // Queue controls may finish without a turn.
        turn_id: Option<u64>,
        reason: CommandFinish,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommandFinish {
    Executed(TurnEndReason), // Never implies TaskOutcome::Accepted.
    Applied,                 // Scope/review/queue/settings operation.
    CancelledBeforeStart,
    Superseded {
        replacement: CommandId,
    },
    Failed {
        code: EngineErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HoldReason {
    Restart {
        generation: u64,
    },
    Paused,
    TaskReview {
        revision: TaskRevision,
        outcome: TaskOutcome,
    },
    Interrupted,
    AmbiguousEffects {
        run_id: RunId,
        intent_ids: Vec<u64>,
    },
    ProviderUnavailable,
    ResumeNeedsConsent,
    StorageUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EngineErrorCode {
    InvalidCommand,
    Unsupported,
    IdentityConflict,
    StaleRevision,
    NotFound,
    NotOwned,
    QueueFull,
    Storage,
    CorruptState,
    Busy,
    ProviderUnavailable,
    Cancelled,
    Deadline,
    SecretUnavailable,
}

impl std::fmt::Display for EngineErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCommand => f.write_str("InvalidCommand"),
            Self::Unsupported => f.write_str("Unsupported"),
            Self::IdentityConflict => f.write_str("IdentityConflict"),
            Self::StaleRevision => f.write_str("StaleRevision"),
            Self::NotFound => f.write_str("NotFound"),
            Self::NotOwned => f.write_str("NotOwned"),
            Self::QueueFull => f.write_str("QueueFull"),
            Self::Storage => f.write_str("Storage"),
            Self::CorruptState => f.write_str("CorruptState"),
            Self::Busy => f.write_str("Busy"),
            Self::ProviderUnavailable => f.write_str("ProviderUnavailable"),
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Deadline => f.write_str("Deadline"),
            Self::SecretUnavailable => f.write_str("SecretUnavailable"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueueProjection {
    pub revision: u64,
    pub restart_generation: u64,
    pub paused: bool,
    pub hold: Option<HoldReason>,
    pub items: Vec<QueuedCommand>, // At most 20, metadata only.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueuedCommand {
    pub receipt: CommandReceipt,
    pub preview: String, // <=512 UTF-8 bytes; no attachment copies.
    pub captured_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReceiptPage {
    pub receipts: Vec<CommandReceipt>,
    pub next_before_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordPage {
    pub records: Vec<EngineRecord>,
    pub next_after_sequence: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrchestrationOptions {
    pub root_id: SessionId,
    pub target: ProviderTarget,
    pub limits: EngineLimits,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineLimits {
    pub queued_commands: usize,    // Default 20 per conversation.
    pub queued_bytes: usize,       // Default 32 MiB per root including children.
    pub command_inbox: usize,      // Default 32.
    pub record_delivery: usize,    // Default 256 record refs, no giant bodies.
    pub progress_keys: usize,      // Default 128 per root.
    pub progress_bytes: usize,     // Default 1 MiB per root.
    pub outbox_bytes: usize,       // Default 8 MiB metadata, object bodies excluded.
    pub active_children: usize,    // Default/hard ceiling 4.
    pub retained_children: usize,  // Default/hard ceiling 32.
    pub history_page_rows: usize,  // Default/hard ceiling 100.
    pub history_page_bytes: usize, // Default/hard ceiling 256 KiB.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StopScope {
    Conversation(ConversationId),
    Task(TaskHandle),
    Root,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StopAck {
    pub scope: StopScope,
    pub affected_runs: Vec<RunId>,
    pub cancellation_requested: bool, // Not Finished and not durable side-effect proof.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ApprovalAddress {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub call_id: String,
}

pub struct QueueContinuationPermit {
    pub(crate) _private: (),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EffectClass {
    ReadOnly,
    WorkspaceMutation,
    ExternalOrUnknown,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum IntentState {
    Prepared,
    Dispatching, // Snapshot persisted immediately before side-effect invocation.
    Completed { success: bool },
    AbandonedBeforeDispatch,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SideEffectIntent {
    pub id: u64,
    pub command_id: CommandId,
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub call_id: String,
    pub effect: EffectClass,
    pub argument_digest: Digest, // No credentials; approved visible args stored separately.
    pub work_revision: u64,
    pub state: IntentState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EngineEvent {
    Record(Box<EngineRecord>),
    Progress(Box<ProgressUpdate>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineRecord {
    pub sequence: u64, // Root-wide durable sequence, strictly increasing.
    pub revision: u64,
    pub conversation: ConversationId,
    pub row: Option<RowId>,
    pub body: EngineRecordBody,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EngineRecordBody {
    Receipt(CommandReceipt),
    Agent(AgentRecordPayload), // Existing critical event compatibility.
    UserStarted {
        command_id: CommandId,
        prompt: PromptDisplay,
    },
    RowSealed(HistoryRow),
    ChildChanged(TaskStatus),
    RichOutput(crate::rich::RichOutput),
    WatchNotification(crate::watch::PrNotification),
    RecoveryHeld {
        generation: u64,
        reason: HoldReason,
    },
    Capabilities(ProviderCapabilities),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentRecordPayload {
    Inline(Box<AgentEvent>),       // <=16 KiB; never an image/body-sized record.
    Stored(crate::rich::AssetRef), // Validated JSON object, lazily resolved.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptDisplay {
    pub request: String,
    pub image_names: Vec<String>,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProgressKind {
    Text,
    Reasoning,
    ToolOutput,
    TerminalOutput,
    Usage,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProgressUpdate {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub row: Option<RowId>,
    pub call_id: Option<String>,
    pub terminal_id: Option<String>,
    pub kind: ProgressKind,
    pub generation: u64, // Reject old-step updates.
    pub value: ProgressValue,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProgressValue {
    TextPreview {
        utf8_start: u64, // Offset of retained suffix in the logical output.
        text: String,
        total_utf8_bytes: u64,
    },
    Usage(Usage),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConversationSummary {
    pub conversation: ConversationId,
    pub title: Option<String>,
    pub target: ProviderTarget,
    pub last_record_sequence: u64,
    pub snapshot_revision: u64,
    pub last_turn_id: u64,
    pub running: bool,
    pub queued: usize,
    pub hold: Option<HoldReason>,
    pub outcome: TaskOutcome,
    pub unread: bool,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskHandle {
    pub conversation: ConversationId,
    pub command_id: CommandId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskRunState {
    Queued,
    Held(HoldReason),
    Running {
        run_id: RunId,
        turn_id: u64,
    },
    Finished {
        reason: TurnEndReason,
        outcome: TaskOutcome,
        result_row: Option<RowId>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskStatus {
    pub handle: TaskHandle,
    pub target: ProviderTarget,
    pub parent: ConversationId,
    pub label: String,
    pub revision: u64,
    pub state: TaskRunState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WaitMode {
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WaitRequest {
    pub handles: Vec<TaskHandle>, // 1..32, all owned by this root.
    pub mode: WaitMode,
    pub timeout_ms: u64, // <=600,000; 0 is immediate snapshot.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WaitResult {
    pub statuses: Vec<TaskStatus>,
    pub timed_out: bool, // Deadline never silently cancels tasks.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChildSpec {
    pub label: String,
    pub child: Option<ChildId>, // Resume existing child, preserving its target.
    pub target: Option<ProviderTarget>, // For new child only.
    pub prompt: PromptSnapshot,
    pub delivery: Delivery,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProviderTarget {
    Native {
        profile: ProfileRef,
        model: String,
        effort: Option<ReasoningEffort>,
    },
    Acp {
        profile: ProfileRef, // App-owned local adapter profile identity.
        agent_id: String,    // AcpAgent::id(), never a shell command.
        options: std::collections::BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FeatureSupport {
    Supported,
    Unsupported { reason: String },
    Unknown { reason: String }, // Never treated as enabled.
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilities {
    pub target: ProviderTarget,
    pub safe_boundary_steering: FeatureSupport,
    pub stop_then_send: FeatureSupport,
    pub session_resume: FeatureSupport,
    pub prompt_images: FeatureSupport,
    pub rich_output: FeatureSupport,
    pub native_task_authority: FeatureSupport,
    pub shared_workspace_evidence: FeatureSupport,
    pub inherited_read_only: FeatureSupport,
    pub inherited_native_sandbox: FeatureSupport,
    pub portable_transfer: FeatureSupport,
    pub provider_native_fork: FeatureSupport,
    pub private_host_secret: FeatureSupport,
    pub history_retrieval_tool: FeatureSupport,
}

pub trait TargetResolver: Send + Sync {
    fn resolve(&self, target: ProviderTarget) -> BoxFuture<Result<ResolvedTarget, EngineError>>;
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InheritedPolicy {
    pub approval: ApprovalMode,
    pub native_sandbox_required: bool,
    pub read_only: bool,
    pub restrictions: Vec<String>,
    pub task_revision: TaskRevision,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExternalPrompt {
    pub command_id: CommandId,
    pub run_id: RunId,
    pub turn_id: u64, // Host-assigned, no competing counter increment.
    pub prompt: PromptSnapshot,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExternalTurnResult {
    pub reason: TurnEndReason,
    pub task: TaskRecord, // Source=External; never native verification.
    pub adapter_checkpoint: serde_json::Value, // Validated bounded, no credentials.
}

pub trait ExternalConversation: Send + Sync {
    fn prompt(&self, prompt: ExternalPrompt) -> BoxFuture<Result<ExternalTurnResult, EngineError>>;
    fn cancel(&self, run_id: RunId) -> BoxFuture<Result<(), EngineError>>;
    fn set_option(&self, id: String, value: String) -> BoxFuture<Result<(), EngineError>>;
    fn shutdown(&self) -> BoxFuture<Result<(), EngineError>>;
}

pub trait ExternalAgentBridge: Send + Sync {
    fn preflight(
        &self,
        target: ProviderTarget,
        policy: InheritedPolicy,
    ) -> BoxFuture<Result<ProviderCapabilities, EngineError>>;
    fn start(&self, start: ExternalStart) -> BoxFuture<Result<ExternalHandle, EngineError>>;
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RowRole {
    User,
    Assistant,
    Tool,
    SystemNotice,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryRow {
    pub id: RowId,
    pub turn_id: u64,
    pub role: RowRole,
    pub source_target: ProviderTarget,
    pub content: crate::rich::ContentRef,
    pub content_digest: Digest,
    pub citations: Vec<Citation>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Citation {
    pub row: RowId,
    pub utf8_start: u64,
    pub utf8_end: u64,
    pub content_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryPageRequest {
    pub conversation: ConversationId,
    pub before_ordinal: Option<u64>,
    pub limit: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub next_before_ordinal: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryExcerpt {
    pub citation: Citation,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferMode {
    Handoff,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferPreviewSpec {
    pub mode: TransferMode,
    pub source: ConversationId,
    pub source_revision: u64,
    pub source_task: TaskRevision,
    pub through_row: RowId,
    pub destination_root: SessionId,
    pub target: ProviderTarget,
    pub current: PromptSnapshot, // Retained exactly, never a generated summary.
    pub budget: TransferBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferSpec {
    pub preview: TransferPreviewSpec,
    pub expected_transfer_digest: Digest, // Required digest from accepted preview.
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferBudget {
    pub max_input_tokens: u64,
    pub reserve_output_tokens: u64,
    pub max_utf8_bytes: usize,
    pub max_images: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TransferPreview {
    pub source_revision: u64,
    pub transfer_digest: Digest,
    pub estimated_input_tokens: u64,
    pub bytes: usize,
    pub included_rows: Vec<RowId>,
    pub omitted: Vec<HistoryOmission>,
    pub capability_warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryOmission {
    pub conversation: ConversationId,
    pub first_ordinal: u64,
    pub last_ordinal: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lineage {
    pub source: ConversationId,
    pub through_row: RowId,
    pub destination: ConversationId,
    pub target: ProviderTarget,
    pub transfer_digest: Digest,
    pub mode: TransferMode,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DestinationReservation {
    pub root: SessionId,
    pub transfer_command: CommandId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TransferPackage {
    pub lineage: Lineage,
    pub current: PromptSnapshot,
    pub attributed_history: String,
    pub inherited_scope: TaskContract,
    pub inherited_hold: Option<HoldReason>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DestinationAccepted {
    pub conversation: ConversationId,
    pub first_command: CommandReceipt, // Held, never auto-sent by reservation.
}

pub trait DestinationRegistry: Send + Sync {
    fn reserve(
        &self,
        root: SessionId,
        transfer_command: CommandId,
    ) -> BoxFuture<Result<DestinationReservation, EngineError>>;
    fn accept(
        &self,
        reservation: DestinationReservation,
        package: TransferPackage,
    ) -> BoxFuture<Result<DestinationAccepted, EngineError>>;
}
