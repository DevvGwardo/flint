//! The contract between the agent engine and any front end.
//!
//! A front end starts a session with [`crate::spawn_session`], sends [`Op`]s,
//! and renders the [`AgentEvent`] stream. Nothing here depends on a UI
//! toolkit or an async runtime: both directions are `async-channel`s, which
//! work from GPUI's executor and from tokio alike.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Everything a session needs to run. `Debug` redacts the API keys.
#[derive(Clone)]
pub struct AgentConfig {
    /// OpenAI-compatible base URL, e.g. `https://api.openai.com/v1`.
    pub base_url: String,
    pub model: String,
    /// Default model for new subagents on this endpoint; `None` inherits the parent.
    pub subagent_model: Option<String>,
    pub api_key: String,
    /// Directory the agent works in; tools resolve relative paths against it.
    pub workspace: PathBuf,
    /// General-purpose assistance rather than assuming a coding project.
    pub general: bool,
    pub approval: ApprovalMode,
    /// Optional JEV judge; `None` keeps the harness on its heuristics.
    pub jev: Option<JevConfig>,
    /// Where the conversation is saved (`history.json`) and restored from.
    /// `None` keeps the session in memory only.
    pub session_dir: Option<PathBuf>,
    /// Token budget for the model context. History is compacted when the
    /// estimate passes 80% of it. `0` (the default) sizes it from the model's
    /// reported context window.
    pub context_budget_tokens: u64,
    /// Sent as `reasoning_effort`; `None` leaves the provider default.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Run shell commands in a sandbox that only lets them write inside the
    /// workspace, temp directories and build caches (macOS; ignored where
    /// unsupported).
    pub sandbox: bool,
    /// MCP servers whose tools the agent may call.
    pub mcp_servers: Vec<McpServerConfig>,
}

/// A Model Context Protocol server started over stdio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Short name; its tools are offered as `mcp__<name>__<tool>`.
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// Default for [`AgentConfig::context_budget_tokens`]: `0` sizes the budget
/// from the model's context window as the provider reports it (see
/// [`crate::provider::Provider::model_limits`]), falling back to
/// [`FALLBACK_CONTEXT_WINDOW_TOKENS`] when it doesn't.
pub const DEFAULT_CONTEXT_BUDGET_TOKENS: u64 = 0;

/// Context window assumed when the provider doesn't report one.
pub const FALLBACK_CONTEXT_WINDOW_TOKENS: u64 = 128_000;

/// How hard a reasoning model should think.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// The wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalMode {
    /// Run every tool call without asking.
    Auto,
    /// Ask before shell commands and file writes; reads run freely.
    AskForChanges,
}

#[derive(Clone)]
pub struct JevConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("subagent_model", &self.subagent_model)
            .field("api_key", &"<redacted>")
            .field("workspace", &self.workspace)
            .field("general", &self.general)
            .field("approval", &self.approval)
            .field("jev", &self.jev)
            .field("session_dir", &self.session_dir)
            .field("context_budget_tokens", &self.context_budget_tokens)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("sandbox", &self.sandbox)
            .field(
                "mcp_servers",
                &self.mcp_servers.iter().map(|s| &s.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl std::fmt::Debug for JevConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JevConfig")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

/// Front end -> engine.
#[derive(Debug, Clone)]
pub enum Op {
    /// Start a turn with this user message (queued if a turn is running).
    UserMessage(String),
    /// Start a turn with visual context, encoded once before leaving the UI.
    UserMessageWithImages {
        text: String,
        images: Vec<ImageAttachment>,
    },
    /// Add a user instruction at the next safe model boundary. Native only;
    /// SteeringAccepted acknowledges when it enters the conversation.
    SteerMessage {
        id: u64,
        text: String,
        images: Vec<ImageAttachment>,
    },
    /// Stop the running turn as soon as possible.
    Interrupt,
    /// Answer an [`AgentEvent::ApprovalRequested`].
    Approval {
        call_id: String,
        decision: ApprovalDecision,
    },
    /// Change the reasoning effort for later model calls (`None` = provider default).
    SetReasoningEffort(Option<ReasoningEffort>),
    /// Restore the files the agent changed in its most recent turn with
    /// changes (flint's own engine; queued behind a running turn). Answered
    /// by [`AgentEvent::FilesReverted`] or an [`AgentEvent::Error`].
    UndoLastTurn,
    /// Set one of the agent's [`SessionOption`]s to one of its choices.
    SetSessionOption {
        id: String,
        value: String,
    },
    Shutdown,
}

/// An image in a user prompt. `data` is base64 without a data-URL prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub name: String,
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approve,
    /// Approve this and all pending parent/child calls, and from now on the
    /// native engine's calls like it: every edit after an edit, or commands
    /// with the same program and subcommand after a command (exact matches
    /// only for chained commands and risky programs). ACP agents implement
    /// their own permissions.
    ApproveAlways,
    Deny,
}

/// Engine -> front end. Events arrive in order; a turn is everything between
/// `TurnStarted` and `TurnFinished` with the same `turn_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentEvent {
    /// A native steering instruction entered the agent's conversation.
    SteeringAccepted {
        id: u64,
    },
    /// Native engine shutdown, after parent and child history writers finish.
    SessionStopped {
        history_saved: bool,
    },
    TurnStarted {
        turn_id: u64,
    },
    /// A new model call within the turn (one per agent-loop step). Text and
    /// reasoning deltas that follow belong to this step's assistant message.
    StepStarted {
        turn_id: u64,
        step: u32,
    },
    ReasoningDelta(String),
    TextDelta(String),
    ToolCallStarted {
        call_id: String,
        name: String,
        kind: ToolKind,
        /// Parsed (and possibly repaired) arguments.
        args: serde_json::Value,
        /// One-line human summary, e.g. `npm test` or `src/main.rs:10-40`.
        summary: String,
    },
    /// Streaming output from a running command.
    ToolOutputDelta {
        call_id: String,
        chunk: String,
    },
    ToolCallFinished {
        call_id: String,
        /// The text the model sees as the result (already truncated).
        output: String,
        exit_code: Option<i32>,
        success: bool,
        /// Present for file edits.
        diff: Option<FileDiff>,
        duration_ms: u64,
    },
    /// A child session attached to a `spawn_agent` call.
    SubagentStarted {
        call_id: String,
        session_id: String,
        model: String,
    },
    /// Child activity, kept separate from the parent's conversation.
    SubagentEvent {
        call_id: String,
        event: Box<AgentEvent>,
    },
    ApprovalRequested {
        call_id: String,
        kind: ToolKind,
        summary: String,
    },
    /// The harness steered the model (stuck loop, unverified edits, ...).
    HarnessNudge {
        reason: NudgeReason,
        message: String,
    },
    /// The harness fixed a malformed tool call instead of failing it.
    ToolRepaired {
        tool: String,
        detail: String,
    },
    /// Cumulative token usage for the turn so far.
    Usage(Usage),
    /// Old history was trimmed to stay within the context budget. Token
    /// counts are estimates of the request size before and after.
    ContextCompacted {
        before_tokens: u64,
        after_tokens: u64,
    },
    TurnFinished {
        turn_id: u64,
        reason: TurnEndReason,
    },
    /// A non-fatal problem worth showing (provider error, bad config, ...).
    Error(String),
    /// Files an undo restored (one diff each, current -> restored) and files
    /// it left alone with the reason.
    FilesReverted {
        turn_id: u64,
        diffs: Vec<FileDiff>,
        skipped: Vec<String>,
    },
    /// The agent's adjustable settings (model, reasoning, mode, …), sent
    /// when the session is ready and whenever they change. An ACP session
    /// always sends this once it is ready, even if the list is empty.
    SessionOptions(Vec<SessionOption>),
    /// The agent ran a command whose output can be shown as a read-only
    /// terminal tab. `call_id` links it to its tool call.
    TerminalStarted {
        terminal_id: String,
        call_id: Option<String>,
        label: String,
        cwd: Option<PathBuf>,
    },
    /// Output for a terminal: appended, or the whole output when `replace`.
    TerminalOutput {
        terminal_id: String,
        data: String,
        replace: bool,
    },
    TerminalExited {
        terminal_id: String,
        exit_code: Option<i32>,
    },
}

/// One setting the agent exposes, e.g. its model or permission mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `mode`, `model`, `thought_level`, `model_config`, or agent-specific.
    pub category: Option<String>,
    /// The current choice's `value`.
    pub current: String,
    pub choices: Vec<OptionChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

impl SessionOption {
    /// The current choice, if it is one of the listed ones.
    pub fn current_choice(&self) -> Option<&OptionChoice> {
        self.choices.iter().find(|c| c.value == self.current)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    Command,
    Read,
    Edit,
    Search,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NudgeReason {
    Stuck,
    Verify,
    Watchdog,
    LeakedCall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    /// Unified diff with 3 lines of context.
    pub unified: String,
    pub added: usize,
    pub removed: usize,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnEndReason {
    Completed,
    Interrupted,
    /// Hit the step limit for one turn.
    StepLimit,
    Failed(String),
}

/// Which agent runs a session: flint's own engine or an ACP agent. Saved in
/// the session's metadata so the UI can badge it and reopen it the same way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    #[default]
    Flint,
    ClaudeCode,
    Codex,
    Droid,
}

impl AgentKind {
    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Flint => "flint",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::Droid => "Droid",
        }
    }
}
