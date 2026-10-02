//! The contract between the agent engine and any front end.
//!
//! A front end starts a session with [`crate::spawn_session`], sends [`Op`]s,
//! and renders the [`AgentEvent`] stream. Nothing here depends on a UI
//! toolkit or an async runtime: both directions are `async-channel`s, which
//! work from GPUI's executor and from tokio alike.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Everything a session needs to run.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// OpenAI-compatible base URL, e.g. `http://127.0.0.1:18433/v1`.
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// Directory the agent works in; tools resolve relative paths against it.
    pub workspace: PathBuf,
    pub approval: ApprovalMode,
    /// Optional JEV judge; `None` keeps the harness on its heuristics.
    pub jev: Option<JevConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalMode {
    /// Run every tool call without asking.
    Auto,
    /// Ask before shell commands and file writes; reads run freely.
    AskForChanges,
}

#[derive(Debug, Clone)]
pub struct JevConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

/// Front end -> engine.
#[derive(Debug, Clone)]
pub enum Op {
    /// Start a turn with this user message (queued if a turn is running).
    UserMessage(String),
    /// Stop the running turn as soon as possible.
    Interrupt,
    /// Answer an [`AgentEvent::ApprovalRequested`].
    Approval {
        call_id: String,
        decision: ApprovalDecision,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approve,
    /// Approve this and every later call in the session.
    ApproveAlways,
    Deny,
}

/// Engine -> front end. Events arrive in order; a turn is everything between
/// `TurnStarted` and `TurnFinished` with the same `turn_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentEvent {
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
    TurnFinished {
        turn_id: u64,
        reason: TurnEndReason,
    },
    /// A non-fatal problem worth showing (provider error, bad config, ...).
    Error(String),
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
