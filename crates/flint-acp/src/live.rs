//! An open ACP session: one prompt per user message, interrupts, approvals
//! and option changes, with `acp.json` kept up to date.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::ConnectionTo;
use agent_client_protocol::schema::v1::CancelNotification;
use agent_client_protocol::schema::v1::ContentBlock;
use agent_client_protocol::schema::v1::ImageContent;
use agent_client_protocol::schema::v1::PromptRequest;
use agent_client_protocol::schema::v1::SessionConfigOption;
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::schema::v1::SetSessionConfigOptionRequest;
use agent_client_protocol::schema::v1::StopReason;
use agent_client_protocol::schema::v1::TextContent;
use async_channel::Receiver;
use flint_agent::AgentEvent;
use flint_agent::ImageAttachment;
use flint_agent::Op;
use flint_agent::TurnEndReason;
use serde::Deserialize;
use serde::Serialize;

use crate::launch::AcpAgent;
use crate::launch::describe;
use crate::mapper::Mapper;
use crate::runner::Shared;
use crate::saved::Saved;
use crate::saved::save_saved;

/// Droid acknowledges setting changes with `{}` and sends the current options
/// separately via `config_option_update`. Keep the standard request on the wire
/// while accepting either that acknowledgement or a full options response.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest)]
#[serde(transparent)]
#[request(method = "session/set_config_option", response = DroidSetOptionResponse)]
struct DroidSetOptionRequest(SetSessionConfigOptionRequest);

#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct DroidSetOptionResponse {
    config_options: Option<Vec<SessionConfigOption>>,
}

pub(crate) struct Live {
    cx: ConnectionTo<agent_client_protocol::Agent>,
    shared: Arc<Shared>,
    session_id: SessionId,
    session_dir: Option<PathBuf>,
    /// Option values the user picked, by option id.
    chosen: BTreeMap<String, String>,
    identity_ready: bool,
    identity_choices: BTreeMap<String, String>,
}

/// What an op means for the loop.
enum Next {
    Prompt(String, Vec<ImageAttachment>),
    Continue,
    Stop,
}

impl Live {
    pub fn new(
        cx: ConnectionTo<agent_client_protocol::Agent>,
        shared: Arc<Shared>,
        session_id: SessionId,
        session_dir: Option<PathBuf>,
        chosen: BTreeMap<String, String>,
    ) -> Self {
        Self {
            cx,
            shared,
            session_id,
            session_dir,
            chosen,
            identity_ready: true,
            identity_choices: Default::default(),
        }
    }

    pub fn save(&self) {
        if let Some(dir) = &self.session_dir {
            save_saved(
                dir,
                &Saved {
                    agent: self.shared.agent.id(),
                    session_id: self.session_id.0.to_string(),
                    turn_id: self.shared.with_mapper(|m| m.turn_id()).unwrap_or(0),
                    options: self.chosen.clone(),
                },
            );
        }
    }

    /// Sets one option (`session/set_config_option`) and shows the agent's
    /// updated options. Unknown options or values are reported, not sent.
    pub async fn set_option(&mut self, id: &str, value: &str) -> bool {
        let options = self.shared.options();
        let provider = options.get(id).is_some_and(|option| {
            option.category.as_deref() == Some("provider") || option.id == "provider"
        });
        let Some(wire) = options.wire_value(id, value) else {
            self.shared.emit_all(vec![AgentEvent::Error(format!(
                "{} has no setting {id} = {value}.",
                self.shared.agent.name()
            ))]);
            return false;
        };
        let request =
            SetSessionConfigOptionRequest::new(self.session_id.clone(), id.to_string(), wire);
        let result = if self.shared.agent == AcpAgent::Droid {
            self.shared
                .rpc(
                    self.cx
                        .send_request(DroidSetOptionRequest(request))
                        .block_task(),
                )
                .await
                .map(|response| response.config_options)
        } else {
            self.shared
                .rpc(self.cx.send_request(request).block_task())
                .await
                .map(|response| Some(response.config_options))
        };
        match result {
            Ok(options) => {
                // An acknowledgement must not erase options received in a
                // notification, or optimistically change permission modes.
                if let Some(options) = options {
                    self.shared.set_options(&options);
                }
                let previous = self.chosen.insert(id.to_string(), value.to_string());
                if provider {
                    if let Err(err) = self.confirm_option(id, value).await {
                        if let Some(previous) = previous {
                            self.chosen.insert(id.to_string(), previous);
                        } else {
                            self.chosen.remove(id);
                        }
                        self.shared.emit_all(vec![AgentEvent::Error(format!(
                            "{} didn't change {id}: provider confirmation failed ({}).",
                            self.shared.agent.name(),
                            err.message
                        ))]);
                        self.save();
                        return false;
                    }
                    // The provider's reported model belongs to this saved
                    // conversation too, not just future-session defaults.
                    for model in self
                        .shared
                        .options()
                        .list
                        .iter()
                        .filter(|option| option.category.as_deref() == Some("model"))
                    {
                        self.chosen.insert(model.id.clone(), model.current.clone());
                    }
                }
                self.save();
                true
            }
            Err(_) if *self.shared.shutdown.borrow() || self.shared.interrupt_pending() => false,
            Err(err) => {
                self.shared.emit_all(vec![AgentEvent::Error(format!(
                    "{} didn't change {id}: {}",
                    self.shared.agent.name(),
                    err.message
                ))]);
                false
            }
        }
    }

    /// Restore identity choices before processing queued prompts. An adapter's
    /// default must not silently replace a selection the user explicitly saved.
    pub async fn restore_preferences(&mut self, preferred: &BTreeMap<String, String>) -> bool {
        self.identity_ready = false;
        self.identity_choices = preferred.clone();
        let initial = self.shared.options();
        let mut choices: Vec<_> = preferred.iter().collect();
        // Changing provider may replace the entire offered model list.
        choices.sort_by_key(|(id, _)| {
            usize::from(
                id.as_str() != "provider"
                    && initial
                        .get(id)
                        .is_none_or(|option| option.category.as_deref() != Some("provider")),
            )
        });
        for (id, value) in choices {
            let options = self.shared.options();
            let supported = options.get(id).is_some_and(|option| {
                (matches!(option.category.as_deref(), Some("model" | "provider"))
                    || option.id == "provider")
                    && option.choices.iter().any(|choice| choice.value == *value)
            });
            if !supported {
                self.shared.emit_all(vec![AgentEvent::Error(format!(
                    "{} no longer offers the saved model/provider setting {id} = {value}. Choose an available value before sending a prompt.",
                    self.shared.agent.name()
                ))]);
                return false;
            }
            if options
                .get(id)
                .is_some_and(|option| option.current == *value)
            {
                self.chosen.insert(id.clone(), value.clone());
                continue;
            }
            if !self.set_option(id, value).await {
                return false;
            }
            if let Err(err) = self.confirm_option(id, value).await {
                let shared = &self.shared;
                if !*shared.shutdown.borrow() && !shared.interrupt_pending() {
                    shared.emit_all(vec![AgentEvent::Error(format!(
                        "{} didn't confirm the saved model/provider setting {id} = {value}: {}. No prompt was sent.",
                        shared.agent.name(), err.message
                    ))]);
                }
                return false;
            }
        }
        self.identity_ready = true;
        true
    }

    async fn confirm_option(
        &self,
        id: &str,
        value: &str,
    ) -> Result<(), agent_client_protocol::Error> {
        let shared = &self.shared;
        let mut changed = shared.options_changed.subscribe();
        shared
            .rpc(async {
                loop {
                    if shared
                        .options()
                        .get(id)
                        .is_some_and(|option| option.current == value)
                    {
                        return Ok(());
                    }
                    changed.changed().await.map_err(|_| {
                        agent_client_protocol::Error::new(-32000, "agent settings channel closed")
                    })?;
                }
            })
            .await
    }

    /// Re-applies saved choices that differ from the agent's current values
    /// (a new session after the old one couldn't be reopened).
    pub async fn reapply(&mut self, saved: &BTreeMap<String, String>) {
        for (id, value) in saved {
            let differs = self
                .shared
                .options()
                .get(id)
                .is_some_and(|option| option.current != *value);
            if differs {
                self.set_option(id, value).await;
            }
        }
    }

    fn undo_unavailable(&self) {
        self.shared.emit_all(vec![AgentEvent::Error(format!(
            "Undo is only available for Flint's own agent, not {}.",
            self.shared.agent.name()
        ))]);
    }

    /// Handles one op between turns.
    async fn idle_op(&mut self, op: Option<Op>) -> Next {
        if !self.identity_ready
            && matches!(
                op,
                Some(Op::UserMessage(_) | Op::UserMessageWithImages { .. })
            )
        {
            self.shared.emit_all(vec![AgentEvent::Error(
                "No prompt was sent: select an available model/provider to replace the saved selection that couldn't be restored.".into()
            )]);
            return Next::Continue;
        }
        match op {
            Some(Op::UserMessage(text)) => Next::Prompt(text, Vec::new()),
            Some(Op::UserMessageWithImages { text, images }) => Next::Prompt(text, images),
            Some(Op::SteerMessage { .. }) => {
                self.shared.emit_all(vec![AgentEvent::Error(
                    "In-turn steering is only available for Flint's native agent.".into(),
                )]);
                Next::Continue
            }
            Some(Op::Approval { call_id, decision }) => {
                self.shared.resolve(&call_id, decision);
                Next::Continue
            }
            Some(Op::SetSessionOption { id, value }) => {
                let identity = self.shared.options().get(&id).is_some_and(|option| {
                    matches!(option.category.as_deref(), Some("model" | "provider"))
                        || option.id == "provider"
                });
                if self.set_option(&id, &value).await && !self.identity_ready && identity {
                    let mut preferred = self.identity_choices.clone();
                    preferred.insert(id, value);
                    // Fixing the model alone must not release prompts while
                    // a requested provider (or another identity slot) is invalid.
                    self.restore_preferences(&preferred).await;
                }
                Next::Continue
            }
            Some(Op::UndoLastTurn) => {
                self.undo_unavailable();
                Next::Continue
            }
            Some(Op::Interrupt) => {
                self.shared
                    .interrupts_seen
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Next::Continue
            }
            Some(Op::SetReasoningEffort(_)) => Next::Continue,
            Some(Op::Shutdown) | None => Next::Stop,
        }
    }

    pub async fn run(mut self, ops: Receiver<Op>) {
        let mut queue: VecDeque<(String, Vec<ImageAttachment>)> = VecDeque::new();
        loop {
            if *self.shared.shutdown.borrow() {
                return;
            }
            let (text, images) = match queue.pop_front() {
                Some(prompt) => prompt,
                None => match self.idle_op(ops.recv().await.ok()).await {
                    Next::Prompt(text, images) => (text, images),
                    Next::Continue => continue,
                    Next::Stop => return,
                },
            };
            if !self.turn(text, images, &ops, &mut queue).await {
                return;
            }
        }
    }

    /// Runs one prompt. Returns false when the session should end.
    async fn turn(
        &mut self,
        text: String,
        images: Vec<ImageAttachment>,
        ops: &Receiver<Op>,
        queue: &mut VecDeque<(String, Vec<ImageAttachment>)>,
    ) -> bool {
        let shared = Arc::clone(&self.shared);
        let opening = shared.with_mapper(Mapper::start_turn).unwrap_or_default();
        let turn_id = shared.with_mapper(|m| m.turn_id()).unwrap_or(0);
        shared.emit_all(opening);
        self.save();

        let cx = self.cx.clone();
        let mut blocks = vec![ContentBlock::Text(TextContent::new(text))];
        blocks.extend(
            images
                .into_iter()
                .map(|image| ContentBlock::Image(ImageContent::new(image.data, image.mime_type))),
        );
        let prompt = cx
            .send_request(PromptRequest::new(self.session_id.clone(), blocks))
            .block_task();
        futures::pin_mut!(prompt);
        let mut cancelled = false;
        let mut keep_going = true;
        let mut cancellation_deadline = None;
        let result = loop {
            tokio::select! {
                result = &mut prompt => break Some(result),
                _ = async {
                    match cancellation_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    // A peer that ignored cancel cannot safely receive another
                    // prompt. Close this connection after the terminal events.
                    keep_going = false;
                    break None;
                }
                op = ops.recv(), if keep_going => match op {
                    Ok(Op::UserMessage(text)) => queue.push_back((text, Vec::new())),
                    Ok(Op::UserMessageWithImages { text, images }) => queue.push_back((text, images)),
                    Ok(Op::SteerMessage { .. }) => self.shared.emit_all(vec![AgentEvent::Error(
                        "In-turn steering is only available for Flint's native agent.".into(),
                    )]),
                    Ok(Op::Approval { call_id, decision }) => shared.resolve(&call_id, decision),
                    Ok(Op::SetSessionOption { id, value }) => {
                        if let Some(deadline) = cancellation_deadline {
                            if tokio::time::timeout_at(deadline, self.set_option(&id, &value)).await.is_err() {
                                keep_going = false;
                                break None;
                            }
                        } else {
                            self.set_option(&id, &value).await;
                        }
                    }
                    Ok(Op::SetReasoningEffort(_)) => {}
                    Ok(Op::UndoLastTurn) => self.undo_unavailable(),
                    Ok(Op::Interrupt) => {
                        shared.interrupts_seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        cancelled = true;
                        cancellation_deadline.get_or_insert_with(|| tokio::time::Instant::now() + std::time::Duration::from_secs(2));
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(self.session_id.clone()));
                    }
                    Ok(Op::Shutdown) | Err(_) => {
                        keep_going = false;
                        cancelled = true;
                        cancellation_deadline.get_or_insert_with(|| tokio::time::Instant::now() + std::time::Duration::from_secs(2));
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(self.session_id.clone()));
                    }
                },
            }
        };
        let (reason, mut closing) = match result {
            Some(Ok(response)) => {
                let reason = if cancelled {
                    TurnEndReason::Interrupted
                } else {
                    match response.stop_reason {
                        StopReason::EndTurn => TurnEndReason::Completed,
                        StopReason::Cancelled => TurnEndReason::Interrupted,
                        StopReason::MaxTurnRequests => TurnEndReason::StepLimit,
                        StopReason::MaxTokens => TurnEndReason::Failed(
                            "the agent hit its output token limit".to_string(),
                        ),
                        StopReason::Refusal => {
                            TurnEndReason::Failed("the agent refused to continue".to_string())
                        }
                        _ => TurnEndReason::Failed(
                            "the agent stopped for an unknown reason".to_string(),
                        ),
                    }
                };
                let mut closing = shared
                    .with_mapper(|m| m.close_open_calls(reason == TurnEndReason::Interrupted))
                    .unwrap_or_default();
                if let Some(usage) = &response.usage {
                    closing.push(Mapper::usage(usage));
                }
                (reason, closing)
            }
            Some(Err(err)) => {
                let message = describe(
                    &shared.agent,
                    &err.message,
                    Some(i32::from(err.code).into()),
                    &shared.stderr_tail(),
                );
                let closing = shared
                    .with_mapper(|m| m.close_open_calls(true))
                    .unwrap_or_default();
                let reason = if cancelled {
                    TurnEndReason::Interrupted
                } else {
                    TurnEndReason::Failed(message.clone())
                };
                let mut closing = closing;
                if !cancelled {
                    closing.push(AgentEvent::Error(message));
                }
                (reason, closing)
            }
            None => (
                TurnEndReason::Interrupted,
                shared
                    .with_mapper(|m| m.close_open_calls(true))
                    .unwrap_or_default(),
            ),
        };
        closing.push(AgentEvent::TurnFinished { turn_id, reason });
        shared.emit_all(closing);
        self.save();
        keep_going
    }
}
