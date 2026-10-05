//! The agent loop: one model call per step, tools between steps, harness
//! nudges between steps and at the end of a turn.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Instant;

use async_channel::Receiver;
use async_channel::Sender;
use futures_util::StreamExt;
use serde_json::Map;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::approvals::Approvals;
use crate::approvals::Request;
use crate::context::ContextTracker;
use crate::harness::Harness;
use crate::harness::args::RepairedArgs;
use crate::harness::args::closest_tool_name;
use crate::harness::args::repair_tool_args;
use crate::harness::jev::JevClient;
use crate::mcp::McpHub;
use crate::persist;
use crate::persist::Saver;
use crate::prompt::general_system_prompt;
use crate::prompt::system_prompt;
use crate::protocol::AgentConfig;
use crate::protocol::AgentEvent;
use crate::protocol::ApprovalDecision;
use crate::protocol::ApprovalMode;
use crate::protocol::FALLBACK_CONTEXT_WINDOW_TOKENS;
use crate::protocol::ImageAttachment;
use crate::protocol::NudgeReason;
use crate::protocol::Op;
use crate::protocol::ReasoningEffort;
use crate::protocol::ToolKind;
use crate::protocol::TurnEndReason;
use crate::protocol::Usage;
use crate::provider::ChatRequest;
use crate::provider::Message;
use crate::provider::ModelLimits;
use crate::provider::Provider;
use crate::provider::ProviderError;
use crate::provider::RawToolCall;
use crate::provider::StreamDelta;
use crate::subagents::Subagents;
use crate::tools;

/// Model calls allowed in one user turn: a cost backstop, not a working
/// budget. A turn that is still working runs as long as it needs — the
/// harness guard, not this cap, is what stops a spinning turn (a loop
/// detector, and the verify/watchdog nudges when the model stops early) —
/// so only a turn that goes far past any real task ends as
/// [`TurnEndReason::StepLimit`].
pub const MAX_STEPS_PER_TURN: u32 = 600;

/// Model calls allowed in one subagent task. Up to four children run at
/// once, so their cap is lower than the parent's: a delegated task is
/// smaller by design, and a runaway child should end long before the parent.
pub const MAX_CHILD_STEPS_PER_TURN: u32 = 200;

/// Runs a session until `Shutdown` or until the front end drops its sender.
pub(crate) async fn run(config: AgentConfig, ops: Receiver<Op>, events: Sender<AgentEvent>) {
    let current = Arc::new(Mutex::new(CancellationToken::new()));
    let effort = Arc::new(Mutex::new(config.reasoning_effort));
    let approvals = Arc::new(Approvals::default());
    let stopping = Arc::new(AtomicBool::new(false));
    let (messages_tx, messages_rx) = async_channel::unbounded::<Input>();

    // Ops are handled on their own task so Interrupt and Approval reach a
    // running turn, and user messages queue while one is in flight.
    let ops_current = Arc::clone(&current);
    let ops_effort = Arc::clone(&effort);
    let ops_approvals = Arc::clone(&approvals);
    let ops_stopping = Arc::clone(&stopping);
    tokio::spawn(async move {
        while let Ok(op) = ops.recv().await {
            match op {
                Op::UserMessage(text) => {
                    let _ = messages_tx.send(Input::Message(text, Vec::new())).await;
                }
                Op::UserMessageWithImages { text, images } => {
                    let _ = messages_tx.send(Input::Message(text, images)).await;
                }
                Op::SteerMessage { id, text, images } => {
                    let _ = messages_tx.send(Input::Steer { id, text, images }).await;
                }
                Op::UndoLastTurn => {
                    let _ = messages_tx.send(Input::Undo).await;
                }
                Op::Interrupt => cancel_current(&ops_current),
                Op::Approval { call_id, decision } => {
                    ops_approvals.respond(&call_id, decision);
                }
                Op::SetReasoningEffort(level) => {
                    if let Ok(mut slot) = ops_effort.lock() {
                        *slot = level;
                    }
                }
                // flint's engine exposes no options beyond effort (its own op).
                Op::SetSessionOption { .. } => {}
                Op::Shutdown => break,
            }
        }
        ops_stopping.store(true, Ordering::SeqCst);
        cancel_current(&ops_current);
        messages_tx.close();
    });

    let auto_budget = config.context_budget_tokens == 0;
    let mcp = if config.mcp_servers.is_empty() {
        None
    } else {
        let (hub, errors) = McpHub::start(&config.mcp_servers, &config.workspace).await;
        for error in errors {
            let _ = events.try_send(AgentEvent::Error(error));
        }
        Some(Arc::new(hub))
    };
    let tools = tools::ToolContext::new(config.workspace.clone(), config.sandbox);
    let mut session = Session::new(config, events, approvals, effort, tools, mcp, false);
    session.inbox = Some(messages_rx);
    if auto_budget {
        let limits = session.provider.model_limits().await;
        session.context.set_budget(budget_for_window(limits));
    }
    while let Some(input) = session.next_input().await {
        let (text, images, steering) = match input {
            Input::Message(text, images) => (text, images, None),
            Input::Steer { id, text, images } => (text, images, Some(id)),
            Input::Undo => {
                session.undo();
                continue;
            }
        };
        let token = CancellationToken::new();
        if let Ok(mut slot) = current.lock() {
            *slot = token.clone();
            if stopping.load(Ordering::SeqCst) {
                break;
            }
        }
        if let Some(id) = steering {
            session.emit(AgentEvent::SteeringAccepted { id });
        }
        session.run_turn_with_images(text, images, &token).await;
    }
    let mut history_saved = true;
    if let Some(saver) = session.saver.take()
        && saver.flush().await.is_err()
    {
        history_saved = false;
        session.emit(AgentEvent::Error(
            "Couldn't save engine history. The session was kept; do not archive it.".into(),
        ));
    }
    if let Some(children) = session.subagents.take()
        && children.flush().await.is_err()
    {
        history_saved = false;
        session.emit(AgentEvent::Error(
            "Couldn't save engine history. Subagent history could not be saved; do not archive it."
                .into(),
        ));
    }
    session.emit(AgentEvent::SessionStopped { history_saved });
}

/// Prompt budget for a model: its context window minus room for the reply.
/// The reserve is the model's output cap, capped at 64k so a huge cap
/// (deepseek-v4.1-flash allows 384k) doesn't swallow the window, and at least
/// 8k. Unknown windows fall back to [`FALLBACK_CONTEXT_WINDOW_TOKENS`].
pub(crate) fn budget_for_window(limits: Option<ModelLimits>) -> u64 {
    let Some(limits) = limits else {
        return FALLBACK_CONTEXT_WINDOW_TOKENS - 16_000;
    };
    let reserve = limits.max_output.unwrap_or(32_000).clamp(8_000, 64_000);
    limits
        .context_window
        .saturating_sub(reserve)
        .max(limits.context_window / 2)
}

/// What the front end sends a session between (or during) turns.
pub(super) enum Input {
    Message(String, Vec<ImageAttachment>),
    Steer {
        id: u64,
        text: String,
        images: Vec<ImageAttachment>,
    },
    Undo,
}

/// Retries of one step after the stream broke mid-reply.
const MAX_STEP_RETRIES: u32 = 2;

fn cancel_current(current: &Mutex<CancellationToken>) {
    if let Ok(token) = current.lock() {
        token.cancel();
    }
}

pub(super) struct Session {
    approval: ApprovalMode,
    config: AgentConfig,
    provider: Provider,
    jev: Option<JevClient>,
    pub(super) history: Vec<Message>,
    pub(super) events: Sender<AgentEvent>,
    approvals: Arc<Approvals>,
    turn_id: u64,
    /// Tool specs, serialized once.
    tools_json: String,
    effort: Arc<Mutex<Option<ReasoningEffort>>>,
    effort_notice_sent: bool,
    context: ContextTracker,
    pub(super) saver: Option<Saver>,
    subagents: Option<Subagents>,
    pub(super) call_prefix: String,
    tool_names: Vec<String>,
    pub(super) last_usage: Usage,
    /// Workspace, file tracker and sandbox for this agent's tools.
    tools: tools::ToolContext,
    mcp: Option<Arc<McpHub>>,
    max_steps: u32,
    child: bool,
    /// Messages and undo requests from the front end (the parent only).
    /// Messages that arrive mid-turn are handed to the running turn.
    inbox: Option<Receiver<Input>>,
    /// Inputs taken from the inbox mid-turn that wait for the turn to end.
    deferred: VecDeque<Input>,
    /// Told to the model with the next user message (e.g. an undo).
    pending_note: Option<String>,
}

impl Session {
    pub(super) fn new(
        config: AgentConfig,
        events: Sender<AgentEvent>,
        approvals: Arc<Approvals>,
        effort: Arc<Mutex<Option<ReasoningEffort>>>,
        tools: tools::ToolContext,
        mcp: Option<Arc<McpHub>>,
        child: bool,
    ) -> Self {
        let mut specs = tools::tool_specs();
        if child {
            specs.retain(|spec| spec["function"]["name"] != tools::SPAWN_AGENT);
        }
        let mut tool_names: Vec<String> = tools::TOOL_NAMES
            .iter()
            .filter(|name| !child || **name != tools::SPAWN_AGENT)
            .map(|name| (*name).to_string())
            .collect();
        if let Some(hub) = &mcp {
            specs.extend(hub.specs());
            tool_names.extend(hub.tools().iter().map(|t| t.full_name.clone()));
        }
        let tools_json = serde_json::to_string(&specs).unwrap_or_default();
        let prompt = if config.general {
            general_system_prompt(&config.workspace)
        } else {
            system_prompt(&config.workspace)
        };
        let mut history = vec![Message::System(prompt)];
        let mut turn_id = 0;
        if let Some(dir) = &config.session_dir {
            match persist::load(dir) {
                Ok(Some(restored)) => {
                    turn_id = restored.turn_id;
                    history.extend(restored.messages);
                }
                Ok(None) => {}
                Err(message) => {
                    let _ = events.try_send(AgentEvent::Error(format!(
                        "Couldn't restore the saved conversation: {message}. Starting fresh."
                    )));
                }
            }
        }
        Self {
            subagents: (!child).then(|| Subagents::new(config.clone(), tools.child(), mcp.clone())),
            provider: Provider::new(&config.base_url, &config.model, &config.api_key),
            jev: config.jev.clone().map(JevClient::new),
            context: ContextTracker::new(
                if config.context_budget_tokens == 0 {
                    budget_for_window(None)
                } else {
                    config.context_budget_tokens
                },
                tools_json.len(),
            ),
            saver: config.session_dir.clone().map(Saver::new),
            history,
            approval: config.approval,
            config,
            events,
            approvals,
            turn_id,
            tools_json,
            effort,
            effort_notice_sent: false,
            call_prefix: String::new(),
            tool_names,
            last_usage: Usage::default(),
            tools,
            mcp,
            max_steps: if child {
                MAX_CHILD_STEPS_PER_TURN
            } else {
                MAX_STEPS_PER_TURN
            },
            child,
            inbox: None,
            deferred: VecDeque::new(),
            pending_note: None,
        }
    }

    /// The next input: one deferred during a turn, else from the inbox.
    async fn next_input(&mut self) -> Option<Input> {
        if let Some(input) = self.deferred.pop_front() {
            return Some(input);
        }
        self.inbox.as_ref()?.recv().await.ok()
    }

    /// Messages the user sent while this turn runs. Undo requests wait for
    /// the turn to end.
    fn take_steering(&mut self) -> Vec<(String, Vec<ImageAttachment>)> {
        let mut messages = Vec::new();
        if !self.deferred.is_empty() {
            return messages;
        }
        let Some(inbox) = &self.inbox else {
            return messages;
        };
        while let Ok(input) = inbox.try_recv() {
            match input {
                Input::Message(text, images) if self.deferred.is_empty() => {
                    messages.push((text, images));
                }
                Input::Steer { id, text, images } if self.deferred.is_empty() => {
                    self.events
                        .try_send(AgentEvent::SteeringAccepted { id })
                        .ok();
                    messages.push((text, images));
                }
                other => self.deferred.push_back(other),
            }
        }
        messages
    }

    /// Restores the files the last turn with changes edited.
    fn undo(&mut self) {
        let report = match self.tools.seen.journal().lock() {
            Ok(mut journal) => journal.undo_last(&self.tools.seen),
            Err(_) => Err("The undo journal is unavailable.".to_string()),
        };
        match report {
            Ok(report) => {
                if !report.diffs.is_empty() {
                    let files: Vec<&str> = report.diffs.iter().map(|d| d.path.as_str()).collect();
                    self.pending_note = Some(format!(
                        "[Note: the user undid your file changes from an earlier turn; these files \
                         are back to how they were before it: {}. Re-read them before editing.]",
                        files.join(", ")
                    ));
                }
                self.emit(AgentEvent::FilesReverted {
                    turn_id: report.turn_id,
                    diffs: report.diffs,
                    skipped: report
                        .skipped
                        .into_iter()
                        .map(|(path, why)| format!("{path}: {why}"))
                        .collect(),
                });
            }
            Err(error) => self.emit(AgentEvent::Error(error)),
        }
    }

    fn tool_name_refs(&self) -> Vec<&str> {
        self.tool_names.iter().map(String::as_str).collect()
    }

    /// Queues a snapshot of the history for the background writer.
    fn save(&self) {
        if let Some(saver) = &self.saver {
            saver.save(persist::snapshot(self.turn_id, &self.history));
        }
    }

    /// Compacts old history when the context estimate is over the trigger.
    fn compact_if_needed(&mut self) {
        if let Some((before_tokens, after_tokens)) = self.context.maybe_compact(&mut self.history) {
            self.emit(AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            });
        }
    }

    fn emit(&self, event: AgentEvent) {
        let _ = self.events.try_send(event);
    }

    pub(super) async fn run_turn(
        &mut self,
        text: String,
        cancel: &CancellationToken,
    ) -> TurnEndReason {
        self.run_turn_with_images(text, Vec::new(), cancel).await
    }

    async fn run_turn_with_images(
        &mut self,
        text: String,
        images: Vec<ImageAttachment>,
        cancel: &CancellationToken,
    ) -> TurnEndReason {
        self.turn_id += 1;
        let turn_id = self.turn_id;
        self.emit(AgentEvent::TurnStarted { turn_id });
        let reason = self.turn_loop(text, images, turn_id, cancel).await;
        self.save();
        self.emit(AgentEvent::TurnFinished {
            turn_id,
            reason: reason.clone(),
        });
        reason
    }

    async fn turn_loop(
        &mut self,
        text: String,
        images: Vec<ImageAttachment>,
        turn_id: u64,
        cancel: &CancellationToken,
    ) -> TurnEndReason {
        if !self.child
            && let Ok(mut journal) = self.tools.seen.journal().lock()
        {
            journal.begin_turn(turn_id);
        }
        let mut harness = Harness::new(&text, self.jev.clone());
        self.push_user(text, images);
        let mut usage = Usage::default();
        self.last_usage = usage;
        let mut retries = 0;

        for step in 0..self.max_steps {
            if cancel.is_cancelled() {
                return TurnEndReason::Interrupted;
            }
            for (text, images) in self.take_steering() {
                harness.add_user_message(&text);
                self.push_user(text, images);
            }
            if let Some(nudge) = harness.before_step().await {
                self.nudge(NudgeReason::Stuck, nudge);
            }
            self.compact_if_needed();
            self.emit(AgentEvent::StepStarted { turn_id, step });
            // DeepSeek thinking mode needs reasoning back on tool-call
            // messages. Replaying it on every turn's (not just this one's)
            // keeps each message byte-identical once sent, so the provider's
            // prompt cache survives turn boundaries; compaction drops old
            // reasoning when space runs short.
            let replay_reasoning_from = 0;
            let effort = self.effort.lock().ok().and_then(|slot| *slot);
            let events = self.events.clone();
            let mut on_delta = move |delta: StreamDelta| {
                let event = match delta {
                    StreamDelta::Reasoning(text) => AgentEvent::ReasoningDelta(text),
                    StreamDelta::Text(text) => AgentEvent::TextDelta(text),
                };
                let _ = events.try_send(event);
            };
            let request = ChatRequest {
                messages: &self.history,
                replay_reasoning_from,
                tools_json: &self.tools_json,
                effort,
            };
            let completion = match self
                .provider
                .complete(&request, &mut on_delta, cancel)
                .await
            {
                Ok(completion) => {
                    retries = 0;
                    completion
                }
                Err(ProviderError::Cancelled) => return TurnEndReason::Interrupted,
                Err(ProviderError::Dropped(message)) if retries < MAX_STEP_RETRIES => {
                    // The reply was cut off after it started showing: run
                    // the step again rather than ending the turn.
                    retries += 1;
                    self.emit(AgentEvent::Error(format!(
                        "The model's reply was cut off ({message}). Retrying this step."
                    )));
                    continue;
                }
                Err(ProviderError::Dropped(message)) => {
                    self.emit(AgentEvent::Error(message.clone()));
                    return TurnEndReason::Failed(message);
                }
                Err(ProviderError::Failed(message)) => {
                    self.emit(AgentEvent::Error(message.clone()));
                    return TurnEndReason::Failed(message);
                }
            };
            if effort.is_some() && !self.provider.effort_supported() && !self.effort_notice_sent {
                self.effort_notice_sent = true;
                self.emit(AgentEvent::Error(
                    "This model endpoint doesn't accept reasoning_effort; continuing without it."
                        .to_string(),
                ));
            }
            // Defense in depth: never prepare or run tools from an incomplete,
            // filtered or otherwise unsupported model outcome.
            if !matches!(
                completion.finish_reason.as_deref(),
                Some("stop" | "tool_calls")
            ) {
                let message = format!("Unsafe model outcome: {:?}", completion.finish_reason);
                self.emit(AgentEvent::Error(message.clone()));
                return TurnEndReason::Failed(message);
            }
            if let Some(call_usage) = completion.usage {
                self.context.observe(&self.history, call_usage.input_tokens);
                usage.input_tokens += call_usage.input_tokens;
                usage.cached_input_tokens += call_usage.cached_input_tokens;
                usage.output_tokens += call_usage.output_tokens;
                usage.reasoning_tokens += call_usage.reasoning_tokens;
                self.emit(AgentEvent::Usage(usage));
                self.last_usage = usage;
            }

            let calls: Vec<PreparedCall> = completion
                .tool_calls
                .iter()
                .map(|raw| self.prepare_call(raw))
                .collect();
            self.history.push(Message::Assistant {
                content: completion.text.clone(),
                reasoning: completion.reasoning.clone(),
                tool_calls: calls.iter().map(|c| c.wire.clone()).collect(),
            });

            if calls.is_empty() {
                // A message the user sent while the model was finishing
                // starts another pass of the same turn.
                let steered = self.take_steering();
                if !steered.is_empty() {
                    for (text, images) in steered {
                        harness.add_user_message(&text);
                        self.push_user(text, images);
                    }
                    self.save();
                    continue;
                }
                let names = self.tool_name_refs();
                match harness.continuation(&completion.text, &names).await {
                    Some((reason, message)) => {
                        self.nudge(reason, message);
                        self.save();
                        continue;
                    }
                    None => return TurnEndReason::Completed,
                }
            }

            let mut index = 0;
            while index < calls.len() {
                let call = &calls[index];
                if cancel.is_cancelled() {
                    self.skip_remaining(&calls[index..]);
                    return TurnEndReason::Interrupted;
                }
                if call.name == tools::SPAWN_AGENT && self.subagents.is_some() {
                    let end = calls[index..]
                        .iter()
                        .position(|call| call.name != tools::SPAWN_AGENT)
                        .map_or(calls.len(), |offset| index + offset);
                    self.run_subagents(&calls[index..end], &mut harness, &mut usage, cancel)
                        .await;
                    index = end;
                } else if runs_in_parallel(call) {
                    let end = calls[index..]
                        .iter()
                        .position(|call| !runs_in_parallel(call))
                        .map_or(calls.len(), |offset| index + offset);
                    self.run_reads(&calls[index..end], &mut harness, cancel)
                        .await;
                    index = end;
                } else {
                    self.run_call(call, &mut harness, cancel).await;
                    index += 1;
                }
            }
            self.save();
            if cancel.is_cancelled() {
                return TurnEndReason::Interrupted;
            }
        }
        TurnEndReason::StepLimit
    }

    /// Adds a user message to the history, with any pending note first.
    fn push_user(&mut self, text: String, images: Vec<ImageAttachment>) {
        let text = match self.pending_note.take() {
            Some(note) => format!("{note}\n\n{text}"),
            None => text,
        };
        self.history.push(if images.is_empty() {
            Message::User(text)
        } else {
            Message::UserWithImages { text, images }
        });
    }

    fn nudge(&mut self, reason: NudgeReason, message: String) {
        self.emit(AgentEvent::HarnessNudge {
            reason,
            message: message.clone(),
        });
        self.history.push(Message::Nudge(message));
    }

    /// Repairs the tool name and arguments of a raw call.
    fn prepare_call(&self, raw: &RawToolCall) -> PreparedCall {
        let mut name = raw.name.clone();
        let names = self.tool_name_refs();
        if !names.contains(&name.as_str())
            && let Some(found) = closest_tool_name(&name, &names)
        {
            self.emit(AgentEvent::ToolRepaired {
                tool: found.to_string(),
                detail: format!("renamed from {name}"),
            });
            name = found.to_string();
        }
        let (args, error) = match repair_tool_args(&raw.arguments) {
            RepairedArgs::Ok { value, repaired } => {
                if repaired {
                    self.emit(AgentEvent::ToolRepaired {
                        tool: name.clone(),
                        detail: "repaired arguments".to_string(),
                    });
                }
                (value, None)
            }
            RepairedArgs::Invalid { error } => (Map::new(), Some(error)),
        };
        let arguments = if error.is_some() {
            "{}".to_string()
        } else {
            Value::Object(args.clone()).to_string()
        };
        PreparedCall {
            wire: RawToolCall {
                id: raw.id.clone(),
                name: name.clone(),
                arguments,
            },
            name,
            args,
            error,
        }
    }

    async fn run_call(
        &mut self,
        call: &PreparedCall,
        harness: &mut Harness,
        cancel: &CancellationToken,
    ) {
        let started = self.start_call(call, harness);
        let kind = tools::tool_kind(&call.name);
        let call_id = format!("{}{}", self.call_prefix, call.wire.id);
        let summary = tools::summary(&call.name, &call.args);
        let outcome = if let Some(error) = &call.error {
            tools::ToolOutcome::error(format!(
                "the arguments are not valid JSON ({error}). Send a JSON object."
            ))
        } else if !self
            .approved(&call_id, &call.name, kind, &summary, &call.args, cancel)
            .await
        {
            let message = if cancel.is_cancelled() {
                "Interrupted before this ran."
            } else {
                "The user declined this action. Ask what they want instead, or try another approach."
            };
            tools::ToolOutcome::error(message)
        } else if call.name == tools::UPDATE_PLAN {
            match tools::parse_plan(&call.args) {
                Ok(plan) => {
                    harness.record_plan(
                        plan.iter()
                            .filter(|s| s.status.is_open())
                            .map(|s| s.step.clone())
                            .collect(),
                    );
                    tools::ToolOutcome::ok(tools::render_plan(&plan))
                }
                Err(error) => tools::ToolOutcome::error(error),
            }
        } else if call.name == tools::LIST_MODELS {
            match self.provider.list_models(cancel).await {
                Ok(models) => {
                    let offset = call
                        .args
                        .get("offset")
                        .and_then(Value::as_u64)
                        .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
                        .min(models.len());
                    let mut bytes = 0;
                    let shown: Vec<_> = models
                        .iter()
                        .skip(offset)
                        .take(100)
                        .take_while(|id| {
                            bytes += serde_json::to_string(id).map_or(0, |id| id.len()) + 1;
                            bytes <= 16_000
                        })
                        .collect();
                    if shown.is_empty() && offset < models.len() {
                        tools::ToolOutcome::error("A model id exceeds the listing output limit.")
                    } else {
                        let next = offset + shown.len();
                        tools::ToolOutcome::ok(serde_json::json!({
                            "models": shown,
                            "total": models.len(),
                            "next_offset": (next < models.len()).then_some(next),
                            "parent_model": self.config.model,
                            "subagent_model": self.config.subagent_model.as_ref().unwrap_or(&self.config.model),
                        }).to_string())
                    }
                }
                Err(error) => tools::ToolOutcome::error(error),
            }
        } else if !self.tool_names.contains(&call.name) {
            tools::ToolOutcome::error(format!(
                "Tool `{}` is unavailable in this session.",
                call.name
            ))
        } else if call.name.starts_with(crate::mcp::PREFIX)
            && let Some(hub) = &self.mcp
        {
            hub.call(&call.name, &call.args, cancel).await
        } else {
            let events = self.events.clone();
            let output_id = call_id.clone();
            let on_output = move |chunk: String| {
                let _ = events.try_send(AgentEvent::ToolOutputDelta {
                    call_id: output_id.clone(),
                    chunk,
                });
            };
            tools::execute(&self.tools, &call.name, &call.args, &on_output, cancel).await
        };
        self.finish_call(call, outcome, harness, started);
    }

    /// Runs consecutive read-only calls (reads, listings, searches) at once.
    /// They need no approval and change nothing, so their order doesn't
    /// matter; results are recorded in call order.
    async fn run_reads(
        &mut self,
        calls: &[PreparedCall],
        harness: &mut Harness,
        cancel: &CancellationToken,
    ) {
        if let [call] = calls {
            return self.run_call(call, harness, cancel).await;
        }
        let started: Vec<Instant> = calls
            .iter()
            .map(|call| self.start_call(call, harness))
            .collect();
        let outcomes = futures_util::future::join_all(
            calls
                .iter()
                .map(|call| tools::execute(&self.tools, &call.name, &call.args, &|_| {}, cancel)),
        )
        .await;
        for ((call, outcome), started) in calls.iter().zip(outcomes).zip(started) {
            self.finish_call(call, outcome, harness, started);
        }
    }

    fn start_call(&self, call: &PreparedCall, harness: &mut Harness) -> Instant {
        let kind = tools::tool_kind(&call.name);
        let args_value = Value::Object(call.args.clone());
        let call_id = format!("{}{}", self.call_prefix, call.wire.id);
        let summary = tools::summary(&call.name, &call.args);
        self.emit(AgentEvent::ToolCallStarted {
            call_id: call_id.clone(),
            name: call.name.clone(),
            kind,
            args: args_value.clone(),
            summary: summary.clone(),
        });
        let path = tools::edit_path(&call.name, &call.args);
        harness.record_tool_call(&call.name, kind, &args_value, path.as_deref());
        Instant::now()
    }

    fn finish_call(
        &mut self,
        call: &PreparedCall,
        outcome: tools::ToolOutcome,
        harness: &mut Harness,
        started: Instant,
    ) {
        let kind = tools::tool_kind(&call.name);
        let args_value = Value::Object(call.args.clone());
        let call_id = format!("{}{}", self.call_prefix, call.wire.id);
        harness.record_tool_result(
            &call.name,
            kind,
            &args_value,
            &outcome.output,
            outcome.exit_code,
            outcome.success,
        );
        self.history.push(Message::Tool {
            call_id: call.wire.id.clone(),
            content: outcome.output.clone(),
        });
        self.emit(AgentEvent::ToolCallFinished {
            call_id,
            output: outcome.output,
            exit_code: outcome.exit_code,
            success: outcome.success,
            diff: outcome.diff,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
    }

    async fn run_subagents(
        &mut self,
        calls: &[PreparedCall],
        harness: &mut Harness,
        usage: &mut Usage,
        cancel: &CancellationToken,
    ) {
        let mut children = self.subagents.take().expect("parent has subagents");
        let models = if calls
            .iter()
            .any(|call| call.args.get("model").is_some_and(|v| !v.is_null()))
        {
            Some(self.provider.list_models(cancel).await)
        } else {
            None
        };
        let effort = self.effort.lock().ok().and_then(|slot| *slot);
        let mut jobs = Vec::new();
        for call in calls {
            let started = self.start_call(call, harness);
            let prepared = if let Some(error) = &call.error {
                Err(format!("Invalid arguments: {error}"))
            } else if cancel.is_cancelled() {
                Err("Interrupted before this ran.".to_string())
            } else {
                children.prepare(&call.args, models.as_ref(), self.approvals.clone(), effort)
            };
            match prepared {
                Ok((child, message)) => {
                    self.emit(AgentEvent::SubagentStarted {
                        call_id: call.wire.id.clone(),
                        session_id: child.id.clone(),
                        model: child.model.clone(),
                    });
                    let events = self.events.clone();
                    jobs.push(async move {
                        let result = crate::subagents::run_child(
                            child,
                            message,
                            &call.wire.id,
                            events,
                            cancel,
                        )
                        .await;
                        (call, started, result)
                    });
                }
                Err(error) => {
                    self.finish_call(call, tools::ToolOutcome::error(error), harness, started)
                }
            }
        }
        let mut running = futures_util::stream::iter(jobs)
            .buffer_unordered(crate::subagents::MAX_PARALLEL_SUBAGENTS);
        while let Some((call, started, result)) = running.next().await {
            // Child edits and verification count as work by the parent too.
            let mut args = std::collections::HashMap::new();
            for event in result.work {
                match event {
                    AgentEvent::ToolCallStarted {
                        call_id,
                        name,
                        kind,
                        args: value,
                        ..
                    } => {
                        let path = value
                            .as_object()
                            .and_then(|args| tools::edit_path(&name, args));
                        harness.record_tool_call(&name, kind, &value, path.as_deref());
                        args.insert(call_id, (name, kind, value));
                    }
                    AgentEvent::ToolCallFinished {
                        call_id,
                        output,
                        exit_code,
                        success,
                        ..
                    } => {
                        if let Some((name, kind, value)) = args.remove(&call_id) {
                            harness.record_tool_result(
                                &name, kind, &value, &output, exit_code, success,
                            );
                        }
                    }
                    _ => {}
                }
            }
            usage.input_tokens += result.child.session.last_usage.input_tokens;
            usage.cached_input_tokens += result.child.session.last_usage.cached_input_tokens;
            usage.output_tokens += result.child.session.last_usage.output_tokens;
            usage.reasoning_tokens += result.child.session.last_usage.reasoning_tokens;
            self.last_usage = *usage;
            self.emit(AgentEvent::Usage(*usage));
            self.finish_call(call, result.outcome, harness, started);
            children.put(result.child);
        }
        self.subagents = Some(children);
    }

    pub(super) async fn initialize_child_budget(&mut self, cancel: &CancellationToken) {
        if self.config.context_budget_tokens == 0 {
            let limits = tokio::select! {
                limits = self.provider.model_limits() => limits,
                () = cancel.cancelled() => None,
            };
            self.context.set_budget(budget_for_window(limits));
        }
    }

    /// Asks the front end when the mode requires it. False means don't run.
    async fn approved(
        &mut self,
        call_id: &str,
        name: &str,
        kind: ToolKind,
        summary: &str,
        args: &Map<String, Value>,
        cancel: &CancellationToken,
    ) -> bool {
        // Fetches can send data out; MCP tools can do anything.
        let request = if name == tools::FETCH_URL {
            let url = args.get("url").and_then(Value::as_str).unwrap_or(summary);
            Some(Request::Fetch(
                tools::fetch::url_host(url).unwrap_or_default(),
            ))
        } else if name.starts_with(crate::mcp::PREFIX) {
            Some(Request::Mcp(name.to_string()))
        } else {
            match kind {
                ToolKind::Command | ToolKind::Edit => Some(Request::new(
                    kind,
                    args.get("command").and_then(Value::as_str),
                )),
                ToolKind::Read | ToolKind::Search | ToolKind::Other => None,
            }
        };
        let Some(request) = request else {
            return true;
        };
        if self.approval == ApprovalMode::Auto {
            return true;
        }
        let Some(answer) = self.approvals.register(call_id, request) else {
            return true;
        };
        self.emit(AgentEvent::ApprovalRequested {
            call_id: call_id.to_string(),
            kind,
            summary: summary.to_string(),
        });
        let decision = tokio::select! {
            answer = answer => answer.ok(),
            () = cancel.cancelled() => None,
        };
        self.approvals.remove(call_id);
        matches!(
            decision,
            Some(ApprovalDecision::Approve | ApprovalDecision::ApproveAlways)
        )
    }

    /// Keeps the history valid after an interrupt: every call gets a result.
    fn skip_remaining(&mut self, calls: &[PreparedCall]) {
        for call in calls {
            self.history.push(Message::Tool {
                call_id: call.wire.id.clone(),
                content: "Interrupted before this ran.".to_string(),
            });
        }
    }
}

/// Read-only calls that may run alongside each other: valid reads, listings
/// and searches.
fn runs_in_parallel(call: &PreparedCall) -> bool {
    call.error.is_none()
        && matches!(
            call.name.as_str(),
            tools::READ_FILE | tools::LIST_DIR | tools::GREP
        )
}

struct PreparedCall {
    /// The call as recorded in history (repaired name and arguments).
    wire: RawToolCall,
    name: String,
    args: Map<String, Value>,
    /// Arguments that could not be parsed.
    error: Option<String>,
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
