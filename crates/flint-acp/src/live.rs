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
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::schema::v1::SetSessionConfigOptionRequest;
use agent_client_protocol::schema::v1::StopReason;
use agent_client_protocol::schema::v1::TextContent;
use async_channel::Receiver;
use flint_agent::AgentEvent;
use flint_agent::ImageAttachment;
use flint_agent::Op;
use flint_agent::TurnEndReason;

use crate::launch::describe;
use crate::mapper::Mapper;
use crate::runner::Shared;
use crate::saved::Saved;
use crate::saved::save_saved;

pub(crate) struct Live {
    cx: ConnectionTo<agent_client_protocol::Agent>,
    shared: Arc<Shared>,
    session_id: SessionId,
    session_dir: Option<PathBuf>,
    /// Option values the user picked, by option id.
    chosen: BTreeMap<String, String>,
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
    pub async fn set_option(&mut self, id: &str, value: &str) {
        let options = self.shared.options();
        let Some(wire) = options.wire_value(id, value) else {
            self.shared.emit_all(vec![AgentEvent::Error(format!(
                "{} has no setting {id} = {value}.",
                self.shared.agent.name()
            ))]);
            return;
        };
        let request =
            SetSessionConfigOptionRequest::new(self.session_id.clone(), id.to_string(), wire);
        match self.cx.send_request(request).block_task().await {
            Ok(response) => {
                self.shared.set_options(&response.config_options);
                self.chosen.insert(id.to_string(), value.to_string());
                self.save();
            }
            Err(err) => self.shared.emit_all(vec![AgentEvent::Error(format!(
                "{} didn't change {id}: {}",
                self.shared.agent.name(),
                err.message
            ))]),
        }
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

    /// Handles one op between turns.
    async fn idle_op(&mut self, op: Option<Op>) -> Next {
        match op {
            Some(Op::UserMessage(text)) => Next::Prompt(text, Vec::new()),
            Some(Op::UserMessageWithImages { text, images }) => Next::Prompt(text, images),
            Some(Op::Approval { call_id, decision }) => {
                self.shared.resolve(&call_id, decision);
                Next::Continue
            }
            Some(Op::SetSessionOption { id, value }) => {
                self.set_option(&id, &value).await;
                Next::Continue
            }
            Some(Op::Interrupt | Op::SetReasoningEffort(_)) => Next::Continue,
            Some(Op::Shutdown) | None => Next::Stop,
        }
    }

    pub async fn run(mut self, ops: Receiver<Op>) {
        let mut queue: VecDeque<(String, Vec<ImageAttachment>)> = VecDeque::new();
        loop {
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
        let result = loop {
            tokio::select! {
                result = &mut prompt => break result,
                op = ops.recv() => match op {
                    Ok(Op::UserMessage(text)) => queue.push_back((text, Vec::new())),
                    Ok(Op::UserMessageWithImages { text, images }) => queue.push_back((text, images)),
                    Ok(Op::Approval { call_id, decision }) => shared.resolve(&call_id, decision),
                    Ok(Op::SetSessionOption { id, value }) => self.set_option(&id, &value).await,
                    Ok(Op::SetReasoningEffort(_)) => {}
                    Ok(Op::Interrupt) => {
                        cancelled = true;
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(self.session_id.clone()));
                    }
                    Ok(Op::Shutdown) | Err(_) => {
                        keep_going = false;
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(self.session_id.clone()));
                        // Give the agent a moment to stop cleanly.
                        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut prompt).await {
                            Ok(result) => break result,
                            Err(_) => return false,
                        }
                    }
                },
            }
        };
        let (reason, mut closing) = match result {
            Ok(response) => {
                let reason = match response.stop_reason {
                    StopReason::EndTurn => TurnEndReason::Completed,
                    StopReason::Cancelled => TurnEndReason::Interrupted,
                    StopReason::MaxTurnRequests => TurnEndReason::StepLimit,
                    StopReason::MaxTokens => {
                        TurnEndReason::Failed("the agent hit its output token limit".to_string())
                    }
                    StopReason::Refusal => {
                        TurnEndReason::Failed("the agent refused to continue".to_string())
                    }
                    _ => {
                        TurnEndReason::Failed("the agent stopped for an unknown reason".to_string())
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
            Err(err) => {
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
        };
        closing.push(AgentEvent::TurnFinished { turn_id, reason });
        shared.emit_all(closing);
        self.save();
        keep_going
    }
}
