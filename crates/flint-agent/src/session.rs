//! The agent loop: one model call per step, tools between steps, harness
//! nudges between steps and at the end of a turn.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use async_channel::Receiver;
use async_channel::Sender;
use serde_json::Map;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::context::ContextTracker;
use crate::harness::Harness;
use crate::harness::args::RepairedArgs;
use crate::harness::args::closest_tool_name;
use crate::harness::args::repair_tool_args;
use crate::harness::jev::JevClient;
use crate::persist;
use crate::persist::Saver;
use crate::prompt::system_prompt;
use crate::protocol::AgentConfig;
use crate::protocol::AgentEvent;
use crate::protocol::ApprovalDecision;
use crate::protocol::ApprovalMode;
use crate::protocol::FALLBACK_CONTEXT_WINDOW_TOKENS;
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
use crate::tools;

/// Model calls allowed in one user turn.
pub const MAX_STEPS_PER_TURN: u32 = 60;

/// Runs a session until `Shutdown` or until the front end drops its sender.
pub(crate) async fn run(config: AgentConfig, ops: Receiver<Op>, events: Sender<AgentEvent>) {
    let current = Arc::new(Mutex::new(CancellationToken::new()));
    let effort = Arc::new(Mutex::new(config.reasoning_effort));
    let (messages_tx, messages_rx) = async_channel::unbounded::<String>();
    let (approvals_tx, approvals_rx) = async_channel::unbounded::<(String, ApprovalDecision)>();

    // Ops are handled on their own task so Interrupt and Approval reach a
    // running turn, and user messages queue while one is in flight.
    let ops_current = Arc::clone(&current);
    let ops_effort = Arc::clone(&effort);
    tokio::spawn(async move {
        while let Ok(op) = ops.recv().await {
            match op {
                Op::UserMessage(text) => {
                    let _ = messages_tx.send(text).await;
                }
                Op::Interrupt => cancel_current(&ops_current),
                Op::Approval { call_id, decision } => {
                    let _ = approvals_tx.send((call_id, decision)).await;
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
        cancel_current(&ops_current);
        messages_tx.close();
    });

    let auto_budget = config.context_budget_tokens == 0;
    let mut session = Session::new(config, events, approvals_rx, effort);
    if auto_budget {
        let limits = session.provider.model_limits().await;
        session.context.set_budget(budget_for_window(limits));
    }
    while let Ok(text) = messages_rx.recv().await {
        let token = CancellationToken::new();
        if let Ok(mut slot) = current.lock() {
            *slot = token.clone();
        }
        session.run_turn(text, &token).await;
    }
    if let Some(saver) = session.saver.take() {
        saver.flush().await;
    }
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

fn cancel_current(current: &Mutex<CancellationToken>) {
    if let Ok(token) = current.lock() {
        token.cancel();
    }
}

struct Session {
    workspace: PathBuf,
    approval: ApprovalMode,
    approve_always: bool,
    provider: Provider,
    jev: Option<JevClient>,
    history: Vec<Message>,
    events: Sender<AgentEvent>,
    approvals: Receiver<(String, ApprovalDecision)>,
    turn_id: u64,
    /// Tool specs, serialized once.
    tools_json: String,
    effort: Arc<Mutex<Option<ReasoningEffort>>>,
    effort_notice_sent: bool,
    context: ContextTracker,
    saver: Option<Saver>,
}

impl Session {
    fn new(
        config: AgentConfig,
        events: Sender<AgentEvent>,
        approvals: Receiver<(String, ApprovalDecision)>,
        effort: Arc<Mutex<Option<ReasoningEffort>>>,
    ) -> Self {
        let tools_json = serde_json::to_string(&tools::tool_specs()).unwrap_or_default();
        let mut history = vec![Message::System(system_prompt(&config.workspace))];
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
            workspace: config.workspace,
            approval: config.approval,
            approve_always: false,
            events,
            approvals,
            turn_id,
            tools_json,
            effort,
            effort_notice_sent: false,
        }
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

    async fn run_turn(&mut self, text: String, cancel: &CancellationToken) {
        self.turn_id += 1;
        let turn_id = self.turn_id;
        self.emit(AgentEvent::TurnStarted { turn_id });
        let reason = self.turn_loop(text, turn_id, cancel).await;
        self.save();
        self.emit(AgentEvent::TurnFinished { turn_id, reason });
    }

    async fn turn_loop(
        &mut self,
        text: String,
        turn_id: u64,
        cancel: &CancellationToken,
    ) -> TurnEndReason {
        self.history.push(Message::User(text.clone()));
        let mut harness = Harness::new(&text, self.jev.clone());
        let mut usage = Usage::default();

        for step in 0..MAX_STEPS_PER_TURN {
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
                Ok(completion) => completion,
                Err(ProviderError::Cancelled) => return TurnEndReason::Interrupted,
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
            if let Some(call_usage) = completion.usage {
                self.context.observe(&self.history, call_usage.input_tokens);
                usage.input_tokens += call_usage.input_tokens;
                usage.cached_input_tokens += call_usage.cached_input_tokens;
                usage.output_tokens += call_usage.output_tokens;
                usage.reasoning_tokens += call_usage.reasoning_tokens;
                self.emit(AgentEvent::Usage(usage));
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
                match harness
                    .continuation(&completion.text, &tools::TOOL_NAMES)
                    .await
                {
                    Some((reason, message)) => {
                        self.nudge(reason, message);
                        self.save();
                        continue;
                    }
                    None => return TurnEndReason::Completed,
                }
            }

            for (index, call) in calls.iter().enumerate() {
                if cancel.is_cancelled() {
                    self.skip_remaining(&calls[index..]);
                    return TurnEndReason::Interrupted;
                }
                self.run_call(call, &mut harness, cancel).await;
            }
            self.save();
            if cancel.is_cancelled() {
                return TurnEndReason::Interrupted;
            }
        }
        TurnEndReason::StepLimit
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
        if !tools::TOOL_NAMES.contains(&name.as_str())
            && let Some(found) = closest_tool_name(&name, &tools::TOOL_NAMES)
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
        let kind = tools::tool_kind(&call.name);
        let args_value = Value::Object(call.args.clone());
        let call_id = call.wire.id.clone();
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
        let started = Instant::now();

        let outcome = if let Some(error) = &call.error {
            tools::ToolOutcome {
                output: format!(
                    "Error: the arguments are not valid JSON ({error}). Send a JSON object."
                ),
                exit_code: None,
                success: false,
                diff: None,
            }
        } else if !self.approved(&call_id, kind, &summary, cancel).await {
            let message = if cancel.is_cancelled() {
                "Interrupted before this ran."
            } else {
                "The user declined this action. Ask what they want instead, or try another approach."
            };
            tools::ToolOutcome {
                output: message.to_string(),
                exit_code: None,
                success: false,
                diff: None,
            }
        } else {
            let events = self.events.clone();
            let output_id = call_id.clone();
            let on_output = move |chunk: String| {
                let _ = events.try_send(AgentEvent::ToolOutputDelta {
                    call_id: output_id.clone(),
                    chunk,
                });
            };
            tools::execute(&self.workspace, &call.name, &call.args, &on_output, cancel).await
        };

        harness.record_tool_result(
            &call.name,
            kind,
            &args_value,
            &outcome.output,
            outcome.exit_code,
            outcome.success,
        );
        self.history.push(Message::Tool {
            call_id: call_id.clone(),
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

    /// Asks the front end when the mode requires it. False means don't run.
    async fn approved(
        &mut self,
        call_id: &str,
        kind: ToolKind,
        summary: &str,
        cancel: &CancellationToken,
    ) -> bool {
        let needs_approval = match kind {
            ToolKind::Command | ToolKind::Edit => true,
            ToolKind::Read | ToolKind::Search | ToolKind::Other => false,
        };
        if self.approval == ApprovalMode::Auto || self.approve_always || !needs_approval {
            return true;
        }
        self.emit(AgentEvent::ApprovalRequested {
            call_id: call_id.to_string(),
            kind,
            summary: summary.to_string(),
        });
        loop {
            let answer = tokio::select! {
                answer = self.approvals.recv() => answer,
                () = cancel.cancelled() => return false,
            };
            let Ok((id, decision)) = answer else {
                return false;
            };
            if id != call_id {
                continue;
            }
            return match decision {
                ApprovalDecision::Approve => true,
                ApprovalDecision::ApproveAlways => {
                    self.approve_always = true;
                    true
                }
                ApprovalDecision::Deny => false,
            };
        }
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
