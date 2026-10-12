# T3 shared backend contracts

Status: architect-approved contract baseline before implementation. Sections 1,
2 and 11 are frozen for B1 to the extent authorized by `T3-SLICE-ENGINE-01.md`;
the later-slice surfaces are approved design contracts, not claims of existing
capabilities. An operation not implemented by its gated slice returns
Unsupported before changing state. Future API corrections need an explicit
architect ruling before either owner edits the shared contract.

Public contract types below live in `flint_agent::orchestration` except `rich`,
`secrets`, and `watch` as marked. Existing `Op`, `AgentEvent`, `AgentConfig`,
`SessionHandle`, TaskRecord and TaskReview remain available. No GPUI type enters
the engine. Code blocks specify intended Rust signatures, not compiled results.

## Architect rulings

1. Accept engine-owned durable acceptance, not UI/channel acknowledgement.
2. Accept at-most-once automatic dispatch with ambiguous-effect holds. Reject
   exactly-once claims for arbitrary provider/tools.
3. Accept a canonical history-v3 checkpoint and one writer. Immutable objects
   finalize before references; no separately authoritative queue JSON.
4. Accept additive handles/records and legacy compatibility. No new legacy
   variants are authorized in B1.
5. Accept visible rejection of strict ACP inherited sandbox/read-only requests
   until a real enforcing bridge exists. External work is not native evidence.
6. Accept explicit endpoint-local profiles and non-destructive lineage transfers.
7. Accept concatenation or bounded previews with authoritative final content;
   never last-delta replacement or dropped critical records.
8. Accept one-use private host-auth sinks only. Reject arbitrary plaintext
   shell/MCP/ACP injection and claims of process-memory secrecy.
9. Modify preview scope: the user selected a full isolated rendering bridge.
   Native partial Mermaid/HTML rendering is not final completion. Inert source
   storage and safe fallback remain useful, not proof of visual parity.
10. Accept cancellable conditional read-only PR HTTP watches, no recurring CLI
    process or automatic task/remote action.
11. Accept sequential bounded slices per owner, with real app integration before
    a feature is counted complete.

Transfer preparation uses a separate `TransferPreviewSpec`, not a zero-digest
sentinel. App owns profiles, presentation/review of external tasks, unsent
outbox, destination lifecycle and private input. An external review stays
`TaskSource::External`; it cannot mint native verification. Root reservation is
held/idempotent and cross-root commits are recoverable, not jointly atomic.
Native omitted-history retrieval and app reattachment are distinct from an
unimplemented ACP MCP retrieval bridge.

Approved B1 direct dependencies are `sha2 = "=0.10.9"` and
`uuid = { version = "=1.26.1", features = ["v4"] }`. The backend worker may edit
its crate manifest and perform the corresponding offline-only lock resolution
in the serialized validation slot. No general dependency upgrade is authorized.
Later dependency/features need their own slice approval.

## 1. Identity, immutable commands and receipts

All durable value types in this section derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` unless stated otherwise; deserialize/load also validates limits and referential integrity. Private-ID newtypes expose constructors/FromStr and do not accept unchecked arbitrary path components.

```rust
pub struct SessionId(String);      // Existing saved root ID, not GPUI uid.
pub struct ChildId(String);        // Existing "agent-N" IDs remain valid.
pub struct CommandId(String);      // UUID v4 lowercase, immutable identity.
pub struct RunId(String);          // Unique dispatch generation.
pub struct ProfileRef {
    pub id: String,
    pub revision: u64,
}
pub struct Digest(pub [u8; 32]);    // SHA-256, algorithm versioned on disk.
pub struct RowId {
    pub conversation: ConversationId,
    pub ordinal: u64,              // Never reused/reindexed after compaction.
}
pub struct ConversationId {
    pub root: SessionId,
    pub child: Option<ChildId>,    // Single delegation level.
}

impl SessionId {
    pub fn parse(value: &str) -> Result<Self, EngineError>;
    pub fn as_str(&self) -> &str;
}
impl ChildId {
    pub fn parse(value: &str) -> Result<Self, EngineError>;
    pub fn as_str(&self) -> &str;
}
impl CommandId {
    pub fn fresh() -> Self;
    pub fn parse(value: &str) -> Result<Self, EngineError>;
    pub fn as_str(&self) -> &str;
}

pub struct PromptSnapshot {
    pub request: String,           // Exact authoritative user instruction.
    pub effective_text: String,    // Frozen attached/context text.
    pub images: Vec<ImageAttachment>,
    pub citations: Vec<Citation>,
}
pub enum Delivery {
    NextTurn,
    NativeSafeBoundary,
    StopThenSend,                  // Explicit; not ACP "steering."
}
pub struct TaskRevision {
    pub turn_id: u64,
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
}
pub struct ScopeEdit {
    pub task_id: u64,
    pub expected_revision: u64,
    pub objective: String,
    pub mode: TaskMode,
    pub requirements: Vec<RequirementSpec>,
    pub restrictions: Vec<String>,
    pub clear_reported_blockers: bool,
}
impl From<TaskEdit> for ScopeEdit { /* exact field mapping */ }
impl From<ScopeEdit> for TaskEdit { /* exact field mapping */ }

pub struct EngineCommand {
    pub id: CommandId,
    pub conversation: ConversationId,
    pub payload: CommandPayload,
}
pub enum CommandPayload {
    Send { prompt: PromptSnapshot, delivery: Delivery },
    ContinueTask { task_id: u64, expected_revision: u64 },
    EditTask(ScopeEdit),
    ReviewTask(TaskReview),
    CancelQueued { command_id: CommandId, expected_queue_revision: u64 },
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
    PauseQueue { expected_queue_revision: u64 },
    ResumeQueue { expected_queue_revision: u64, restart_generation: u64 },
    SetOption { id: String, value: String },
    StartChild(ChildSpec),
    Transfer(TransferSpec),
    UndoLastTurn { expected_turn_id: u64 },
    Watch(WatchCommand),
}

pub struct CommandReceipt {
    pub id: CommandId,
    pub conversation: ConversationId,
    pub task: Option<TaskHandle>,  // Includes reserved child address for StartChild.
    pub payload_digest: Digest,
    pub accepted_sequence: u64,
    pub accepted_revision: u64,
    pub status_revision: u64,
    pub state: CommandState,
}
pub enum CommandState {
    Accepted,                     // Payload durable; not started.
    Held(HoldReason),             // Still accepted, deliberately not scheduled.
    Started {
        run_id: RunId,
        turn_id: u64,
        safe_boundary_step: Option<u32>,
    },
    Finished {
        run_id: Option<RunId>,    // Queue controls may finish without a turn.
        turn_id: Option<u64>,
        reason: CommandFinish,
    },
}
pub enum CommandFinish {
    Executed(TurnEndReason),      // Never implies TaskOutcome::Accepted.
    Applied,                     // Scope/review/queue/settings operation.
    CancelledBeforeStart,
    Superseded { replacement: CommandId },
    Failed { code: EngineErrorCode, message: String },
}
pub enum HoldReason {
    Restart { generation: u64 },
    Paused,
    TaskReview { revision: TaskRevision, outcome: TaskOutcome },
    Interrupted,
    AmbiguousEffects { run_id: RunId, intent_ids: Vec<u64> },
    ProviderUnavailable,
    ResumeNeedsConsent,
    StorageUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    pub code: EngineErrorCode,
    pub message: String,          // Bounded/safe; never echo credential/error body.
}
pub enum EngineErrorCode {
    InvalidCommand, Unsupported, IdentityConflict, StaleRevision,
    NotFound, NotOwned, QueueFull, Storage, CorruptState, Busy,
    ProviderUnavailable, Cancelled, Deadline, SecretUnavailable,
}
```

`CommandPayload` contains values, not functions/secret bytes/approvals/cancellation channels. It is strictly bounded on submission (including serialized JSON size and nesting). Nested ReplaceQueued is accepted only for a replacement Send; nested control/replace/transfer commands are rejected. The entire replacement and supersession are one snapshot transaction, before either can run.

Command identity is immutable: same ID + byte-canonical payload returns the existing receipt, regardless of current status; same ID + different conversation/payload returns IdentityConflict and leaves the old entry untouched. Canonical encoding is versioned and deterministic (no timestamps/secret runtime data, sorted maps). Caller creates and stores the ID before submitting. Exact accepted envelope/digest tombstone remains retained on disk for the session lifetime; active memory is paginated, never an unbounded receipt map.

Accepted means the canonical snapshot referencing the frozen payload has passed the commit barrier. Channel send success, provider startup, row display or TurnStarted are not acceptance. A lost reply is reconciled using the same ID. Rejected requests have no accepted receipt; errors do not mean an earlier acceptance with the same ID was rolled back.

NativeSafeBoundary start is the actual durable instruction injection into the containing turn/task, not merely arrival at intake. Its Finished status is attached to that containing turn's termination. NextTurn never opportunistically steers. StopThenSend explicitly cancels the current run and remains queued until cancellation completes; cancelled/ambiguous effects cause a hold unless a separate user-issued live permit authorizes continuation.

## 2. Additive session/client surface

New APIs do not require adding enum variants or fields to existing SessionHandle literals. Agent and ACP implement the same orchestration clients.

```rust
pub struct OrchestrationHandle {
    pub ops: async_channel::Sender<Op>, // Compatibility path, not durable receipt API.
    pub commands: CommandClient,
    pub controls: ControlClient,
    pub events: async_channel::Receiver<EngineEvent>,
    pub secrets: flint_agent::secrets::SecretInbox,
}

#[derive(Clone)]
pub struct CommandClient { /* opaque bounded supervisor endpoint */ }
impl CommandClient {
    pub async fn submit(&self, command: EngineCommand)
        -> Result<CommandReceipt, EngineError>;
    pub async fn receipt(&self, id: &CommandId)
        -> Result<Option<CommandReceipt>, EngineError>;
    pub async fn queue(&self, conversation: &ConversationId)
        -> Result<QueueProjection, EngineError>;
    pub async fn queued_prompt(&self, id: &CommandId)
        -> Result<PromptSnapshot, EngineError>; // One bounded on-demand editor read.
    pub async fn recent_receipts(&self, before_sequence: Option<u64>, limit: usize)
        -> Result<ReceiptPage, EngineError>;
    pub async fn acknowledge_records(&self, through_sequence: u64)
        -> Result<(), EngineError>;
    pub async fn records(&self, after_sequence: u64, limit: usize)
        -> Result<RecordPage, EngineError>;
    pub async fn summary(&self, conversation: &ConversationId)
        -> Result<ConversationSummary, EngineError>;
    pub async fn history(&self, request: HistoryPageRequest)
        -> Result<HistoryPage, EngineError>;
    pub async fn resolve_citation(&self, citation: Citation)
        -> Result<HistoryExcerpt, EngineError>;
    pub async fn task(&self, handle: &TaskHandle)
        -> Result<TaskStatus, EngineError>;
    pub async fn wait(&self, request: WaitRequest)
        -> Result<WaitResult, EngineError>;
    pub async fn confirm_queue_continuation(
        &self, conversation: ConversationId, revision: TaskRevision,
        command_id: CommandId, restart_generation: u64,
    ) -> Result<QueueContinuationPermit, EngineError>;
    pub async fn resume_one(&self, permit: QueueContinuationPermit)
        -> Result<CommandReceipt, EngineError>;
    pub async fn prepare_transfer(&self, spec: &TransferPreviewSpec)
        -> Result<TransferPreview, EngineError>;
}

pub struct QueueProjection {
    pub revision: u64,
    pub restart_generation: u64,
    pub paused: bool,
    pub hold: Option<HoldReason>,
    pub items: Vec<QueuedCommand>, // At most 20, metadata only.
}
pub struct QueuedCommand {
    pub receipt: CommandReceipt,
    pub preview: String,         // <=512 UTF-8 bytes; no attachment copies.
    pub captured_bytes: u64,
}
pub struct ReceiptPage {
    pub receipts: Vec<CommandReceipt>,
    pub next_before_sequence: Option<u64>,
}
pub struct RecordPage {
    pub records: Vec<EngineRecord>,
    pub next_after_sequence: u64,
    pub has_more: bool,
}

pub struct OrchestrationOptions {
    pub root_id: SessionId,
    pub target: ProviderTarget,
    pub limits: EngineLimits,
}
pub struct HostServices {
    pub profiles: std::sync::Arc<dyn TargetResolver>,
    pub external: Option<std::sync::Arc<dyn ExternalAgentBridge>>,
    pub pr_watch: Option<std::sync::Arc<dyn PrWatchSource>>,
    pub destinations: Option<std::sync::Arc<dyn DestinationRegistry>>,
}
pub struct EngineLimits {
    pub queued_commands: usize,     // Default 20 per conversation.
    pub queued_bytes: usize,        // Default 32 MiB per root including children.
    pub command_inbox: usize,       // Default 32.
    pub record_delivery: usize,     // Default 256 record refs, no giant bodies.
    pub progress_keys: usize,       // Default 128 per root.
    pub progress_bytes: usize,      // Default 1 MiB per root.
    pub outbox_bytes: usize,        // Default 8 MiB metadata, object bodies excluded.
    pub active_children: usize,     // Default/hard ceiling 4.
    pub retained_children: usize,   // Default/hard ceiling 32.
    pub history_page_rows: usize,   // Default/hard ceiling 100.
    pub history_page_bytes: usize,  // Default/hard ceiling 256 KiB.
}
pub fn spawn_orchestrated_session(
    config: AgentConfig, options: OrchestrationOptions, services: HostServices,
) -> Result<OrchestrationHandle, EngineError>;

// In flint_acp:
pub fn spawn_orchestrated_acp_session(
    agent: AcpAgent, config: AcpConfig,
    options: flint_agent::orchestration::OrchestrationOptions,
    services: flint_agent::orchestration::HostServices,
) -> Result<flint_agent::orchestration::OrchestrationHandle,
            flint_agent::orchestration::EngineError>;
```

Spawn's Result covers synchronous argument/lease/thread startup only; later provider/adapter failures are visible engine records plus held command state. Durable submission requires `session_dir: Some`; ephemeral legacy sessions remain supported and must not receive a falsely durable receipt. A later separately labeled memory-only orchestration mode would need an explicit durability field, not implicit acceptance.

**Legacy integration:** keep existing `spawn_session(AgentConfig) -> SessionHandle` and `spawn_acp_session(AcpAgent, AcpConfig) -> SessionHandle`. Refactor their executors internally but retain public shapes and ordinary behaviors. New app sessions use new handles/events only. Never consume a legacy event projection and EngineEvent for the same session, or record the same prompt twice. If opening v3 through an old function, use the safe canonical writer and preserve all fields; do not start a legacy coalescing writer over v3.

**UI contract:** GPUI starts an async submit task, retains command ID/payload in its small unsent outbox until a receipt/query confirms engine ownership, shows “accepting” distinct from queued/running, and updates queue from engine projection. Composer/history rows are engine Started/injected records, not speculative pre-send user logs. Explicit legacy queue migration assigns deterministic namespaced source IDs mapped to fresh command IDs, atomically accepts the import paused, and preserves the old file until all receipts are reconciled.

## 3. Controls, queue review and side-effect intent

Controls are nonserialized one-use/live operations. Separate lanes/atomic cancellation keep interrupts and approval replies independent of command intake, data emission and snapshot I/O.

```rust
#[derive(Clone)]
pub struct ControlClient { /* opaque */ }
pub enum StopScope {
    Conversation(ConversationId),
    Task(TaskHandle),
    Root,
}
pub struct StopAck {
    pub scope: StopScope,
    pub affected_runs: Vec<RunId>,
    pub cancellation_requested: bool, // Not Finished and not durable side-effect proof.
}
pub struct ApprovalAddress {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub call_id: String,
}
pub struct QueueContinuationPermit { /* non-Clone, no serde/debug of token */ }
impl ControlClient {
    pub async fn stop(&self, scope: StopScope) -> Result<StopAck, EngineError>;
    pub async fn approve(&self, address: ApprovalAddress, decision: ApprovalDecision)
        -> Result<(), EngineError>;
    pub async fn shutdown(&self) -> Result<(), EngineError>;
}

pub enum EffectClass { ReadOnly, WorkspaceMutation, ExternalOrUnknown }
pub enum IntentState {
    Prepared,
    Dispatching,       // Snapshot persisted immediately before side-effect invocation.
    Completed { success: bool },
    AbandonedBeforeDispatch,
    Ambiguous,
}
pub struct SideEffectIntent {
    pub id: u64,
    pub command_id: CommandId,
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub call_id: String,
    pub effect: EffectClass,
    pub argument_digest: Digest,  // No credentials; approved visible args stored separately.
    pub work_revision: u64,
    pub state: IntentState,
}
```

Stop fanout sets cancellation tokens before any awaited event write. Multiple approval replies resolve one parked responder only, validate owner/run, and never create new approval authority. Native `ApproveAlways` semantics remain existing Approvals policy; ACP keeps peer permissions distinct. Secret submission, queue resume and target selection do not imply filesystem/tool approval.

QueueContinuationPermit is generated only by the explicit user confirmation client (never a model tool), bound to command/conversation/task+work revisions/restart generation, expires on any change/stop/restart and authorizes **one next command**, not acceptance or replay of an uncertain action. It is not a serializable field in ResumeQueue. Native task review is still `TaskRecord::review(TaskReview)` with durable checkpoint. External task acceptance remains a clearly app-owned review; an optional bridge to store it must preserve `TaskSource::External` and cannot pretend the peer's loop became native-verified.

Prepared/Dispatching/Completed intent barriers are immutable commits, not coalesced latest snapshots. A failed barrier prevents the side effect. After restart, Accepted-but-never-started work becomes Held(Restart); Started work is held/assessed interrupted with uncertain Dispatching intents Ambiguous. Never auto-reexecute a started identity, including a purely read-only provider turn. The user can abandon it and submit a fresh explicitly authorized command; no unprovable “exactly once” promise.

Restriction tightening is applied to the shared WorkClock after durable acceptance, using command-keyed pending restrictions; removing/superseding before start removes only that command's pending restriction. Preserve recheck-after-approval protection. Legacy string-only restriction APIs can wrap this keyed implementation.

## 4. Delivery, stable rows and lightweight projections

```rust
pub enum EngineEvent {
    Record(EngineRecord),
    Progress(ProgressUpdate),
}
pub struct EngineRecord {
    pub sequence: u64,              // Root-wide durable sequence, strictly increasing.
    pub revision: u64,
    pub conversation: ConversationId,
    pub row: Option<RowId>,
    pub body: EngineRecordBody,
}
pub enum EngineRecordBody {
    Receipt(CommandReceipt),
    Agent(AgentRecordPayload),     // Existing critical event compatibility.
    UserStarted { command_id: CommandId, prompt: PromptDisplay },
    RowSealed(HistoryRow),
    ChildChanged(TaskStatus),
    RichOutput(flint_agent::rich::RichOutput),
    WatchNotification(flint_agent::watch::PrNotification),
    RecoveryHeld { generation: u64, reason: HoldReason },
    Capabilities(ProviderCapabilities),
}
pub enum AgentRecordPayload {
    Inline(Box<AgentEvent>),       // <=16 KiB; never an image/body-sized record.
    Stored(flint_agent::rich::AssetRef), // Validated JSON object, lazily resolved.
}
pub struct PromptDisplay {
    pub request: String,
    pub image_names: Vec<String>,
    pub citations: Vec<Citation>,
}
pub enum ProgressKind { Text, Reasoning, ToolOutput, TerminalOutput, Usage }
pub struct ProgressUpdate {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub row: Option<RowId>,
    pub call_id: Option<String>,
    pub terminal_id: Option<String>,
    pub kind: ProgressKind,
    pub generation: u64,            // Reject old-step updates.
    pub value: ProgressValue,
}
pub enum ProgressValue {
    TextPreview {
        utf8_start: u64,             // Offset of retained suffix in the logical output.
        text: String,
        total_utf8_bytes: u64,
    },
    Usage(Usage),
}
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
pub fn read_conversation_summary(dir: &std::path::Path)
    -> Result<Option<ConversationSummary>, EngineError>;
pub fn read_agent_record(
    dir: &std::path::Path, payload: &AgentRecordPayload, max_bytes: usize,
) -> Result<AgentEvent, EngineError>;
```

Critical Agent events include all turn/step boundaries, approvals, accepted instructions, task updates/assessments, tool starts/results, terminal starts/exits, child starts/results, errors, file reverts, option confirmations and SessionStopped. They must not enter a lossy `try_send` path. Text/Reasoning/ToolOutput/TerminalOutput/Usage become separately replaceable previews, or adjacent lossless concatenation for legacy consumers; TextDelta itself is not a latest-value metric.

Flush the pending preview for a stream key before its barrier record. Event consumer may replace previews, but RowSealed/ToolCallFinished carries the authoritative retained final text/body ref, explicit truncation if already bounded by tool/provider rules, and terminal status. Ordering and persistence of durable records are independent of repaint batches.

Bounded delivery holds record references, not repeated entire TaskRecord/images. Large legacy AgentEvent payloads are finalized as validated `application/x-flint-agent-event+json` objects; `read_agent_record` is an on-demand bounded compatibility resolver, not a per-token whole-row copy. The on-disk outbox retains every undelivered critical record; acknowledgement allows delivery compaction but not deletion of transcript/receipt identity history. When its cap is reached, stop admitting work and pause/cancel producer execution without blocking controls. Storage failure leaves accepted state recoverable and execution stopped; it cannot be solved by dropping an approval/terminal event. Bounded RAM with unlimited accepted disk production is not claimed; storage exhaustion is visible/fail-closed.

Records/row/asset objects are finalized before the snapshot references them; the snapshot is the commit point. A queue receipt is included in that same commit, not a later best-effort events.jsonl append. Coalesced previews have no durable sequence and are not required for recovery. A derived summary has a watermark; stale summary never overrides the canonical task/ledger.

## 5. Async task handles and child addressability

```rust
pub struct TaskHandle {
    pub conversation: ConversationId,
    pub command_id: CommandId,
}
pub enum TaskRunState {
    Queued,
    Held(HoldReason),
    Running { run_id: RunId, turn_id: u64 },
    Finished { reason: TurnEndReason, outcome: TaskOutcome, result_row: Option<RowId> },
}
pub struct TaskStatus {
    pub handle: TaskHandle,
    pub target: ProviderTarget,
    pub parent: ConversationId,
    pub label: String,
    pub revision: u64,
    pub state: TaskRunState,
}
pub enum WaitMode { Any, All }
pub struct WaitRequest {
    pub handles: Vec<TaskHandle>,   // 1..32, all owned by this root.
    pub mode: WaitMode,
    pub timeout_ms: u64,           // <=600,000; 0 is immediate snapshot.
}
pub struct WaitResult {
    pub statuses: Vec<TaskStatus>,
    pub timed_out: bool,           // Deadline never silently cancels tasks.
}
pub struct ChildSpec {
    pub label: String,
    pub child: Option<ChildId>,    // Resume existing child, preserving its target.
    pub target: Option<ProviderTarget>, // For new child only.
    pub prompt: PromptSnapshot,
    pub delivery: Delivery,
}
```

StartChild reserves the child address in the acceptance transaction and returns it in `CommandReceipt.task`, together with durable registry status; retry does not allocate another child. UI follow-up is Send addressed to that ConversationId, not a new root session or prompt against root selected model. StopScope::Task cancels only that task; root stop fans out all root-owned live tasks. Legacy `spawn_agent(session_id, model)` remains a synchronous awaiting wrapper over the registry; `session_id` + target/model override is invalid.

Wait implementation subscribes to tokio watch/Notify before reading completion state, then checks in a loop driven by revision changes/deadline/cancellation; no 8ms polling and no lost wakeups. Results are retained once in the durable registry, so finished-before-wait works. Four permits are shared by old/new UI/model child dispatch. Children do not get child-start tools. Active children use root shared TaskRecord observation epoch/undo/sandbox/approvals. Parent terminal assessment waits for or explicitly records cancellation/blocked live child work; child TurnFinished never terminates root.

## 6. Targets, capabilities and dependency-inverted ACP bridge

```rust
pub enum ProviderTarget {
    Native {
        profile: ProfileRef,
        model: String,
        effort: Option<ReasoningEffort>,
    },
    Acp {
        profile: ProfileRef,       // App-owned local adapter profile identity.
        agent_id: String,          // AcpAgent::id(), never a shell command.
        options: std::collections::BTreeMap<String, String>,
    },
}
pub enum FeatureSupport {
    Supported,
    Unsupported { reason: String },
    Unknown { reason: String },    // Never treated as enabled.
}
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
pub type BoxFuture<T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

pub enum ResolvedTarget { Native(ResolvedNative), External }
pub struct ResolvedNative {
    pub target: ProviderTarget,
    pub base_url: String,          // Origin vetted against this profile.
    pub credential: flint_agent::secrets::SecretBytes,
    pub capabilities: ProviderCapabilities,
}
pub trait TargetResolver: Send + Sync {
    fn resolve(&self, target: ProviderTarget)
        -> BoxFuture<Result<ResolvedTarget, EngineError>>;
}
pub struct InheritedPolicy {
    pub approval: ApprovalMode,
    pub native_sandbox_required: bool,
    pub read_only: bool,
    pub restrictions: Vec<String>,
    pub task_revision: TaskRevision,
}
pub struct ExternalStart {
    pub conversation: ConversationId,
    pub target: ProviderTarget,
    pub workspace: std::path::PathBuf,
    pub session_dir: std::path::PathBuf,
    pub policy: InheritedPolicy,
    pub events: ExternalEventSink, // Common bounded/durable sink, not Sender<AgentEvent>.
}
#[derive(Clone)]
pub struct ExternalEventSink { /* supervisor-owned capability */ }
impl ExternalEventSink {
    pub async fn record(&self, event: AgentEvent)
        -> Result<(), EngineError>;
    pub fn progress(&self, update: ProgressUpdate)
        -> Result<(), EngineError>;
    pub async fn rich(&self, output: flint_agent::rich::RichOutput)
        -> Result<(), EngineError>;
    pub async fn intent(&self, intent: SideEffectIntent)
        -> Result<(), EngineError>;
}
pub struct ExternalPrompt {
    pub command_id: CommandId,
    pub run_id: RunId,
    pub turn_id: u64,               // Host-assigned, no competing counter increment.
    pub prompt: PromptSnapshot,
}
pub struct ExternalTurnResult {
    pub reason: TurnEndReason,
    pub task: TaskRecord,          // Source=External; never native verification.
    pub adapter_checkpoint: serde_json::Value, // Validated bounded, no credentials.
}
pub trait ExternalConversation: Send + Sync {
    fn prompt(&self, prompt: ExternalPrompt)
        -> BoxFuture<Result<ExternalTurnResult, EngineError>>;
    fn cancel(&self, run_id: RunId) -> BoxFuture<Result<(), EngineError>>;
    fn set_option(&self, id: String, value: String)
        -> BoxFuture<Result<(), EngineError>>;
    fn shutdown(&self) -> BoxFuture<Result<(), EngineError>>;
}
pub struct ExternalHandle {
    pub conversation: std::sync::Arc<dyn ExternalConversation>,
    pub capabilities: ProviderCapabilities,
}
pub trait ExternalAgentBridge: Send + Sync {
    fn preflight(&self, target: ProviderTarget, policy: InheritedPolicy)
        -> BoxFuture<Result<ProviderCapabilities, EngineError>>;
    fn start(&self, start: ExternalStart)
        -> BoxFuture<Result<ExternalHandle, EngineError>>;
}

// In flint_acp, app constructs this with explicit approved local profiles:
pub struct AcpProfile {
    pub profile: flint_agent::orchestration::ProfileRef,
    pub agent: AcpAgent,
}
pub struct AcpBridge { /* opaque */ }
impl AcpBridge {
    pub fn new(profiles: Vec<AcpProfile>) -> Self;
}
impl flint_agent::orchestration::ExternalAgentBridge for AcpBridge { /* ... */ }
```

Native resolver builds one endpoint/key/model per profile. Profile revision is persisted so reopening does not silently select a modified endpoint or credential identity. API key is runtime-only; Debug redacts and serde is absent on ResolvedNative. Existing AgentConfig key fields remain for compatibility; new profile credential is converted only at the trusted native provider boundary, then original private buffer zeroized. Never serialize AgentConfig.

Preflight validates profile/model/policy and local executable availability before child/session/workspace mutation. No automatic npx package installation/download for the new bridge. Runtime ACP negotiation may still fail after process launch: mark held/unavailable, do not send a prompt, and report unsupported capability. Vendor names do not establish capabilities.

Current ACP cannot enforce native sandbox/read-only policy for peer-private actions. It must report Unsupported and reject strict inherited policy. A future enforcing process wrapper/isolation bridge would be a separate reviewed slice, not a boolean passed to AcpConfig. External edits invalidate root verification conservatively and never synthesize passing native checks. Failed `session/load` is ResumeNeedsConsent; no silent new conversation receiving queued work.

## 7. History, citations and bounded handoff/fork

```rust
pub enum RowRole { User, Assistant, Tool, SystemNotice }
pub struct HistoryRow {
    pub id: RowId,
    pub turn_id: u64,
    pub role: RowRole,
    pub source_target: ProviderTarget,
    pub content: flint_agent::rich::ContentRef,
    pub content_digest: Digest,
    pub citations: Vec<Citation>,
    pub truncated: bool,
}
pub struct Citation {
    pub row: RowId,
    pub utf8_start: u64,
    pub utf8_end: u64,
    pub content_digest: Digest,
}
pub struct HistoryPageRequest {
    pub conversation: ConversationId,
    pub before_ordinal: Option<u64>,
    pub limit: usize,
    pub max_bytes: usize,
}
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub next_before_ordinal: Option<u64>,
}
pub struct HistoryExcerpt {
    pub citation: Citation,
    pub text: String,
    pub truncated: bool,
}
pub enum TransferMode { Handoff, Fork }
pub struct TransferPreviewSpec {
    pub mode: TransferMode,
    pub source: ConversationId,
    pub source_revision: u64,
    pub source_task: TaskRevision,
    pub through_row: RowId,
    pub destination_root: SessionId,
    pub target: ProviderTarget,
    pub current: PromptSnapshot,     // Retained exactly, never a generated summary.
    pub budget: TransferBudget,
}
pub struct TransferSpec {
    pub preview: TransferPreviewSpec,
    pub expected_transfer_digest: Digest, // Required digest from accepted preview.
}
pub struct TransferBudget {
    pub max_input_tokens: u64,
    pub reserve_output_tokens: u64,
    pub max_utf8_bytes: usize,
    pub max_images: usize,
}
pub struct TransferPreview {
    pub source_revision: u64,
    pub transfer_digest: Digest,
    pub estimated_input_tokens: u64,
    pub bytes: usize,
    pub included_rows: Vec<RowId>,
    pub omitted: Vec<HistoryOmission>,
    pub capability_warnings: Vec<String>,
}
pub struct HistoryOmission {
    pub conversation: ConversationId,
    pub first_ordinal: u64,
    pub last_ordinal: u64,
    pub reason: String,
}
pub struct Lineage {
    pub source: ConversationId,
    pub through_row: RowId,
    pub destination: ConversationId,
    pub target: ProviderTarget,
    pub transfer_digest: Digest,
    pub mode: TransferMode,
}
pub struct DestinationReservation {
    pub root: SessionId,
    pub transfer_command: CommandId,
}
pub struct TransferPackage {
    pub lineage: Lineage,
    pub current: PromptSnapshot,
    pub attributed_history: String,
    pub inherited_scope: TaskContract,
    pub inherited_hold: Option<HoldReason>,
}
pub struct DestinationAccepted {
    pub conversation: ConversationId,
    pub first_command: CommandReceipt, // Held, never auto-sent by reservation.
}
pub trait DestinationRegistry: Send + Sync {
    fn reserve(&self, root: SessionId, transfer_command: CommandId)
        -> BoxFuture<Result<DestinationReservation, EngineError>>;
    fn accept(
        &self, reservation: DestinationReservation, package: TransferPackage,
    ) -> BoxFuture<Result<DestinationAccepted, EngineError>>;
}
```

`prepare_transfer` takes `TransferPreviewSpec`; execution wraps it in
`TransferSpec` with the required returned digest. Submit requires equality with
the preview digest and all source revisions still current. No zero-digest
sentinel authorizes execution.

Row IDs address the retained transcript catalog, not compacted provider message positions or GPUI row indices. Sealed row content is immutable; streaming edits have a generation, and replacement/branching creates a new row ID. Citations must be valid UTF-8 boundaries with expected digest; invalid/outdated selection fails visibly, never resolves to a different current row. Full-body reads are capped; content refs avoid whole-history copying. Native omitted-history retrieval tool can call the same bounded resolver.

Transfer serializes attributed inert excerpts (“historical user/assistant/tool report from target X”), never promotes imported material into the current user's authoritative request. Use current request in destination TaskRecord; source protected scope/restrictions are host-owned inherited constraints, not a model-written objective. Preserve source outstanding review/unknown-effect state in lineage/projection, do not import Acceptance/passing checks. Old tool/reasoning state is portable descriptive text only, not executable provider messages.

Budget accounts for destination system/task/tool schema, current request, image charge and output reserve. Estimates are estimates, with explicit byte hard cap. If current request+required host scope alone exceeds the budget, reject rather than silently truncate. Trim oldest excerpts atomically by rows/tool-safe chunks, include omission citations, and retain source retrieval in app. ACP has no guaranteed native retrieval tool; explicitly expose app lookup/reattach instead of promising it.

Handoff pauses the source and creates a **new destination**; fork leaves source checkpoint intact. Both require idle source/children and rechecked work revision; stopping mid-side-effect makes an ambiguous hold. Transfer creation never performs checkout/reset/file rewind. A transcript fork is not a filesystem checkpoint restore. Destination acceptance is separate and source evidence stays intact.

DestinationRegistry is an app-supplied lifecycle bridge, not a dependency from agent to app. It resolves the app's actual sessions root, records presentation metadata and starts the appropriate native/ACP orchestrated session only after target preflight. Reservation is idempotent by `(destination_root, transfer_command)`, rejects a preexisting unrelated root, and has no provider/workspace side effects. Source TransferPrepared/pause commits before destination acceptance; destination frozen package/lineage/held first command commits before source TransferFinished. These are a recoverable two-phase protocol, not an atomic cross-directory rename. Crash gaps reconcile the same reservation and keep both sides held; they never auto-send destination work or erase source evidence. A new explicit user resume is required. Package excludes source Acceptance/check-pass certificates and provider reasoning; inherited_scope is host-owned constraints only, not a replacement for current.request.

## 8. Typed rich output (`flint_agent::rich`)

```rust
pub struct AssetId(String);
pub struct AssetRef {
    pub id: AssetId,
    pub mime_type: String,
    pub bytes: u64,
    pub digest: Digest,
}
pub enum ContentRef {
    Inline(String),                 // <=16 KiB, safe plain/Markdown source.
    Asset(AssetRef),                 // Engine-owned relative object, not arbitrary PathBuf.
}
pub enum DiagramLanguage { Mermaid, Dot }
pub enum RichPart {
    Text { content: ContentRef, markdown: bool },
    Image { asset: AssetRef, alt: String },
    HtmlSource { content: ContentRef },
    DiagramSource { language: DiagramLanguage, content: ContentRef },
    ResourceLink { uri: String, name: String, mime_type: Option<String> },
    Unsupported { mime_type: String, reason: String },
}
pub struct RichOutput {
    pub row: RowId,
    pub call_id: Option<String>,
    pub parts: Vec<RichPart>,        // <=32.
    pub model_text: String,          // Existing bounded text result + nontext placeholders.
    pub truncated: bool,
}
pub struct RichToolOutcome {
    pub legacy: flint_agent::tools::ToolOutcome,
    pub parts: Vec<RichPart>,
    pub truncated: bool,
}
pub struct RichLimits {
    pub parts: usize,                // 32.
    pub decoded_asset_bytes: usize,  // <=1 MiB at current MCP 1 MiB frame cap.
    pub output_bytes: usize,         // <=4 MiB across ACP output parts.
    pub source_bytes: usize,         // <=256 KiB HTML/diagram.
}
pub fn read_asset(
    session_dir: &std::path::Path, asset: &AssetRef, max_bytes: usize,
) -> Result<Vec<u8>, EngineError>;
```

Rich types derive serde except runtime wrapper RichToolOutcome (ToolOutcome currently is not serde). Keep public ToolOutcome fields unchanged; existing `McpHub::call(...) -> ToolOutcome` projects new `call_rich(...) -> RichToolOutcome` internally. Add exact `McpHub::call_rich(&self, full_name: &str, args: &serde_json::Map<String, serde_json::Value>, cancel: &tokio_util::sync::CancellationToken, assets: &AssetWriter) -> RichToolOutcome`, where `AssetWriter` is an opaque bounded engine asset-store handle in `rich`, passed only by executor/bridge. ACP mapper likewise separates content extraction from pure event mapping and delegates object writes outside its mutex.

AssetWriter API: `pub async fn put(&self, mime_type: &str, bytes: Vec<u8>) -> Result<AssetRef, EngineError>`. Only validated image/source MIME types from tools; internal event JSON MIME is reserved for the host record writer, not model-supplied artifacts. Image PNG/JPEG/WebP raster only, no SVG renderer. Assets are finalized before durable refs; immutable ID/digest checked on read, no symlink/path traversal or network resolution.

MCP text remains model text; image base64 becomes a private bounded file ref, not large strings copied into events/history. HTML embedded resources and Mermaid/Dot source get typed inert source only. MCP structuredContent is a bounded textual/JSON source part, not raw arbitrary event Value. ACP text/image/embedded-resource/resource-link parts are preserved; tool content updates replace the supplied part set as ACP semantics dictate, not blindly append duplicates.

Do not auto-fetch ResourceLink URIs. Untrusted URI is validated for allowed external display/open; no credential-bearing, javascript/data/file/app-native schemes. App must render source as text unless its own restricted renderer is actually supported and tested (no script, external loads, file reads, native bridge, credential/session access). A raw HTML string is never safe executable markup merely because RichPart tags it.

## 9. Private secret channel (`flint_agent::secrets`)

No secret-bearing type derives Serialize/Deserialize/Clone/PartialEq, and Debug prints a constant redaction. The opaque reply handle is move-only and private; values never travel through Op/AgentEvent/EngineRecord/Message.

```rust
pub struct SecretBytes { /* Zeroizing<Vec<u8>>, private storage */ }
impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Result<Self, SecretError>;
    pub fn clear(&mut self);
}
impl std::fmt::Debug for SecretBytes { /* "<redacted>" only */ }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretPurpose {
    NativeProviderAuth { profile: ProfileRef },
    ReadOnlyPrAuth { profile: ProfileRef, origin: String },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretDescriptor {
    pub conversation: ConversationId,
    pub run_id: RunId,
    pub purpose: SecretPurpose,
    pub title: String,              // Host-authored, never reflected input value.
    pub expires_after_ms: u64,      // <=300,000.
}
pub struct SecretRequest {
    pub descriptor: SecretDescriptor,
    pub reply: SecretReplyHandle,
}
pub struct SecretReplyHandle { /* one-shot sender, no public token string */ }
pub struct SecretInbox { /* async_channel::Receiver<SecretRequest>, capacity 4 */ }
impl SecretInbox {
    pub async fn recv(&self) -> Result<SecretRequest, SecretError>;
}
impl SecretReplyHandle {
    pub fn submit(self, value: SecretBytes) -> Result<(), SecretError>;
    pub fn cancel(self);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretError {
    Expired, Cancelled, WrongOwner, TooLarge, Empty, Closed, Unsupported,
}
```

Engine retains a private one-shot receiver bound to conversation/run/profile-origin/one pending sink, so guessed/cross-session/stale tokens are not reply addresses. Handle Drop cancels. Stop/restart invalidates outstanding handles; requester cannot serialize/reconstruct them. Maximum value size 16 KiB; four outstanding requests per root; no persistence of values/descriptors carrying private channels. Audit records may state only that a private request was pending/cancelled, never carry value/handle.

Sink is host-controlled: native auth header/read-only PR header only. No shell command/env/stdin, file content, prompt, model-visible tool args/result or MCP/ACP JSON plaintext sink. Requesting a credential does not approve the underlying action. Provider origin changes invalidate permission; redirects disabled for the sink. Raw credential-bearing headers/errors/responses must not be Debugged, dumped or forwarded; provider response quarantine/redaction is defense in depth, not a proof against malicious encodings/echoes.

App integration requires a truly private input, not ordinary composer/history/stash. The UI consumes handle in an async task, clears its field, excludes it from automation/debug state and ordinary copy/capture paths. Physical keyboard/VoiceOver/security boundaries remain unverified without real authorized exercise. Zeroizing a buffer does not guarantee absence from OS memory/core dumps or copies in a third-party HTTP library.

## 10. PR watches (`flint_agent::watch`)

```rust
pub struct WatchId(String);
pub struct NotificationId(String);
impl WatchId {
    pub fn fresh() -> Self;
    pub fn parse(value: &str) -> Result<Self, EngineError>;
    pub fn as_str(&self) -> &str;
}
impl NotificationId {
    pub fn as_str(&self) -> &str;
}
pub enum PrProvider { GitHub }
pub struct PrIdentity {
    pub provider: PrProvider,
    pub origin: String,             // https API origin, configured allowlist.
    pub owner: String,
    pub repository: String,
    pub number: u64,
}
pub struct PrWatchSpec {
    pub id: WatchId,
    pub conversation: ConversationId,
    pub pr: PrIdentity,
    pub auth_profile: Option<ProfileRef>,
    pub interval_secs: u64,         // Minimum 60 seconds.
}
pub enum PrState { Open, Closed, Merged }
pub struct PrSnapshot {
    pub state: PrState,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub mergeability: Option<String>,
    pub checks: std::collections::BTreeMap<String, String>,
    pub reviews: std::collections::BTreeMap<String, String>,
    pub fingerprint: Digest,
}
pub struct PrWatchRecord {
    pub spec: PrWatchSpec,
    pub revision: u64,
    pub paused: bool,
    pub latest: Option<PrSnapshot>,
    pub etag: Option<String>,
    pub last_checked_unix_ms: Option<u64>,
    pub last_error: Option<String>,
}
pub struct PrNotification {
    pub id: NotificationId,
    pub watch_id: WatchId,
    pub conversation: ConversationId,
    pub previous: Option<Digest>,
    pub current: Digest,
    pub summary: String,
    pub observed_unix_ms: u64,
    pub acknowledged: bool,
}
pub enum WatchCommand {
    Put { spec: PrWatchSpec, expected_revision: Option<u64> },
    Pause { id: WatchId, expected_revision: u64, paused: bool },
    Remove { id: WatchId, expected_revision: u64 },
    Refresh { id: WatchId, expected_revision: u64 },
    Acknowledge { id: NotificationId, expected_fingerprint: Digest },
}
pub enum PrPollResult { NotModified, Snapshot { snapshot: PrSnapshot, etag: Option<String> } }
pub trait PrWatchSource: Send + Sync {
    fn poll(&self, watch: PrWatchRecord)
        -> BoxFuture<Result<PrPollResult, EngineError>>;
}
```

Built-in source uses existing reqwest with fixed read-only GitHub routes/allowed origin; explicit enterprise API origin requires user configuration. Private auth is resolved through trusted profile/secret service, not PrWatchRecord. No user-supplied arbitrary endpoint/path/query credential. At most eight watches per root, one in-flight read per watch, consolidated scheduler with cancellation, >=60-second interval/backoff/conditional requests. No adapter/provider/gh child process per poll.

Fingerprint is versioned SHA-256 of normalized selected fields, not arbitrary JSON order, poll time or ETag. Store previous/current fingerprint before notification delivery; identical state or HTTP304 does not produce a new notification. A stale in-flight response cannot overwrite a later watch revision/removal. Initial successful poll establishes baseline, not an invented change. Persist notifications with exact fingerprint acknowledgement; cap 64 pending per root and visibly pause a watch rather than silently discard unread changes. Reopen watches paused until explicit user network resume.

Watches do not grant task acceptance, create queued work, run git fetch/checkout or comment/merge/update remote state. Provider errors become bounded watch status, not fake PR changes. Read-only source bridge tests are local fake HTTP; real PR service support is unverified until explicitly exercised.

## 11. Persistence transaction and integration rules

1. One writer lease owns canonical history/ledger/task/adapter checkpoint. Existing v1/v2 loaders migrate without deleting starting history. Snapshot v3 keeps top-level legacy history projection and adds versioned orchestration state, next sequence/row/run allocators, restart generation, queue, receipt index, intent state, lineage, outbox watermark and referenced immutable objects.
2. Submit actor validates identity/policy/limits, builds **immutable revision R** containing payload and Accepted receipt, finalizes needed payload objects, commits R, then resolves caller reply/emits receipt. Execution cannot observe an uncommitted command. Do not replace a pending immutable barrier with the latest ordinary save.
3. Before native run start, commit Started/run/turn ID, protected request/task transition and user row. Before each potentially mutating tool invocation, recheck cancellation/read-only/approval, commit Dispatching intent, then invoke. Awaited completion commit includes actual evidence and final bounded result before Finished. Separate control tokens are checked across these awaits.
4. Session execution mutations merge through the supervisor with expected run/revision; they do not write stale whole snapshots that erase accepted queue entries. No two independent writers for queue/history. Child snapshots/objects finalize before root references final child results.
5. ACP prompt has a conservative external intent; client fs/terminal actions also record guarded intents. Direct peer tools cannot be atomically observed by host; cancellation/restart remains uncertain and outcome External/Unknown. Preserve workspace confinement/approval behavior; never advertise missing enforcement.
6. `sync_all` file then rename then directory-sync (Unix) defines acceptance barrier. Error stops scheduling; no Accepted reply on failed commit. A committed snapshot + lost receipt is resolved idempotently. A started snapshot + crash is held, never replayed. Only referenced, validated record/object state is replayed.
7. Receipt/history/navigation queries are read-only, paged and bounded. No full-root history clone per token. Summary index repair cannot manufacture a task result or run work.
8. Stopped-task review/edit APIs acquire the same writer lease and preserve orchestration fields. Archive/delete requires stopped/flush verification across parent/children/bridge, retaining existing archive guard.
9. Legacy UI store is not command authority. App can retain old logs for old sessions and its own presentation state; it must not double-write engine canonical ledger or replay preview deltas as new commands.

## Integration decisions and slice boundaries

- Additive handles and history v3/single writer are approved. Legacy consumers
  must not rename/drop v3 as corrupt or write v2 over it.
- Separate TransferPreviewSpec is approved. Source/destination stay held through
  recoverable preparation/acceptance; the app owns lifecycle integration.
- External review remains external presentation authority and preserves source
  provenance. The receipt bridge is frozen before B5 app integration.
- Profile settings and resolver are app-owned; B5 freezes concrete UI resolution
  and credential-origin behavior before implementing them.
- Strict ACP inherited policy is visibly unsupported pending actual enforcement.
- Full isolated HTML/Mermaid renderer has a separate PHASE 0/implementation lane.
  Source storage alone does not complete preview support.
- ACP retrieval through an additional MCP bridge is not implied. Native tool
  retrieval and bounded app lookup/reattach remain separately supported.
- Only the named B1 dependency additions/lock updates are approved here.

No code or validation outcome is claimed by this contract document.
