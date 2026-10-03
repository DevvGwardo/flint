//! Engine traffic on the root view: sending messages (with attachments),
//! starting engines, pumping their events in coalesced batches, the
//! animation clock, interrupts, approvals and reasoning effort.

use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::ReasoningEffort;
use flint_agent::TurnEndReason;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::engine;
use crate::store;
use crate::store::Logged;

impl FlintApp {
    pub fn apply_event(&mut self, ix: usize, event: AgentEvent, cx: &mut Context<Self>) {
        let uid = self.sessions[ix].uid;
        let now = self.now();
        self.apply_events(uid, vec![event], now, cx);
    }

    /// Folds a batch of events into a session and redraws once.
    pub(crate) fn apply_events(
        &mut self,
        uid: u64,
        events: Vec<AgentEvent>,
        now: Duration,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        let active = self.active;
        let mut finished = false;
        for event in events {
            // A command an agent runs gets a read-only tab in the terminal
            // dock; the event itself carries nothing the transcript shows.
            match &event {
                AgentEvent::TerminalStarted {
                    terminal_id, label, ..
                } => self.agent_terminal_started(uid, terminal_id.clone(), label.clone(), cx),
                AgentEvent::TerminalOutput {
                    terminal_id,
                    data,
                    replace,
                } => self.agent_terminal_output(uid, terminal_id, data, *replace, cx),
                AgentEvent::TerminalExited {
                    terminal_id,
                    exit_code,
                } => self.agent_terminal_exited(uid, terminal_id, *exit_code, cx),
                _ => {}
            }
            let session = &mut self.sessions[ix];
            session.log(Logged::Event(event.clone()));
            session.note_timing(&event);
            if let AgentEvent::Error(message) = &event
                && message.contains("doesn't accept reasoning_effort")
            {
                self.effort_supported = false;
            }
            let diff = match &event {
                AgentEvent::ToolCallFinished {
                    diff: Some(diff), ..
                } => Some(diff.clone()),
                _ => None,
            };
            finished |= matches!(event, AgentEvent::TurnFinished { .. });
            match &event {
                AgentEvent::SessionOptions(options) => {
                    self.sessions[ix].options = options.clone();
                    self.sessions[ix].agent_ready = true;
                }
                AgentEvent::Error(_) if !self.sessions[ix].agent_ready => {
                    self.sessions[ix].agent_failed = true;
                }
                _ => {}
            }
            let session = &mut self.sessions[ix];
            let change = session.view.fold(event, now);
            session.apply(change);
            if let Some(diff) = diff {
                session.record_edit(diff);
            }
        }
        let session = &mut self.sessions[ix];
        session.touched = SystemTime::now();
        if finished {
            session.settle_changes();
            session.save_meta();
            if ix != active {
                session.unread = true;
            }
        }
        if finished && crate::automation::dump_state(self) && self.options.exit_after_turn {
            // Leave time for the harness to take its final screenshot.
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
        self.ensure_ticker(cx);
        cx.notify();
    }

    /// Starts the animation clock while any session is working; it stops
    /// itself once all are idle, so an idle window never repaints.
    pub(crate) fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_some() || !self.sessions.iter().any(|s| s.view.running) {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(crate::ui::TICK_MS))
                    .await;
                let keep = this.update(cx, |app, cx| {
                    let running = app.sessions.iter().any(|s| s.view.running);
                    if running {
                        cx.notify();
                    } else {
                        app.ticker = None;
                    }
                    running
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        }));
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.slash.is_some() {
            self.run_selected_slash(window, cx);
            return;
        }
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(kind) = crate::agents::parse_command(&text) {
            self.composer
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.choose_agent(kind, window, cx);
            return;
        }
        let ix = self.active;
        if self.sessions[ix].view.running && self.sessions[ix].ops.is_none() {
            return;
        }
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.mention = None;
        let attachments = std::mem::take(&mut self.attachments);
        let message = crate::mention::attach(&text, &self.sessions[ix].workspace, &attachments);
        self.send_message(ix, text, message, cx);
    }

    /// Shows `text` in the transcript and sends `message` (the text plus any
    /// attachments) to the session's engine, starting it if needed.
    pub(crate) fn send_message(
        &mut self,
        ix: usize,
        text: String,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let session = &mut self.sessions[ix];
        let change = session.view.push_user(text.clone());
        session.apply(change);
        session.touched = SystemTime::now();
        session.submitted_at = Some(Instant::now());
        session.first_token = None;
        session.first_text = None;
        session.last_message = Some((text.clone(), message.clone()));
        if !self.options.ephemeral() && session.dir.is_none() {
            session.dir = Some(store::sessions_dir(&self.home).join(store::new_id()));
        }
        match self.ensure_engine(ix, cx) {
            Ok(()) => {
                let session = &mut self.sessions[ix];
                session.log(Logged::User(text));
                session.save_meta();
                if let Some(ops) = &session.ops {
                    ops.try_send(Op::UserMessage(message)).ok();
                }
            }
            Err(err) => self.apply_event(ix, AgentEvent::Error(format!("{err:#}")), cx),
        }
        cx.notify();
    }

    /// Re-sends the last message of the active session (error card "Retry").
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        let Some((text, message)) = self.sessions[ix].last_message.clone() else {
            return;
        };
        if self.sessions[ix].view.running {
            return;
        }
        // A failed start leaves no engine; drop a dead one so it restarts.
        if self.sessions[ix]
            .ops
            .as_ref()
            .is_some_and(|ops| ops.is_closed())
        {
            self.sessions[ix].ops = None;
            self.sessions[ix].agent_failed = false;
        }
        self.send_message(ix, text, message, cx);
    }

    /// Starts the engine for a session on its first message.
    pub(crate) fn ensure_engine(
        &mut self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if self.sessions[ix].ops.is_some() {
            return Ok(());
        }
        let kind = self.sessions[ix].agent;
        if let Some(handle) = self.spawn_acp(ix, kind) {
            self.attach_engine(ix, handle, cx);
            return Ok(());
        }
        let session = &self.sessions[ix];
        let mut config = engine::config_for(
            &session.workspace,
            &self.settings,
            self.key_path.as_deref(),
            &self.key_sources,
            self.approval,
        )?;
        config.session_dir = session.dir.clone();
        config.reasoning_effort = self.effort.filter(|_| self.effort_supported);
        self.attach_engine(ix, flint_agent::spawn_session(config), cx);
        Ok(())
    }

    /// Connects a session to an engine: ops go to `handle.ops`, and events
    /// from `handle.events` are folded into the session in batches (one
    /// redraw per batch, at most ~120 per second). The UI tests attach
    /// scripted channels here instead of a real engine.
    pub fn attach_engine(
        &mut self,
        ix: usize,
        handle: flint_agent::SessionHandle,
        cx: &mut Context<Self>,
    ) {
        let events = handle.events.clone();
        let uid = self.sessions[ix].uid;
        let session = &mut self.sessions[ix];
        session.ops = Some(handle.ops);
        session.pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                let applied = this.update(cx, |app, cx| {
                    let now = app.now();
                    app.apply_events(uid, batch, now, cx);
                });
                if applied.is_err() {
                    break;
                }
                // Let the next burst accumulate instead of redrawing per delta.
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
            }
        }));
    }

    pub fn interrupt(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        if !self.sessions[ix].view.running {
            return;
        }
        if let Some(ops) = &self.sessions[ix].ops {
            ops.try_send(Op::Interrupt).ok();
            return;
        }
        // Demo: stop playback and close the turn.
        self.sessions[ix].pump = None;
        let turn_id = self.sessions[ix].view.turn_id;
        self.apply_event(
            ix,
            AgentEvent::TurnFinished {
                turn_id,
                reason: TurnEndReason::Interrupted,
            },
            cx,
        );
    }

    pub fn answer_approval(
        &mut self,
        call_id: String,
        decision: ApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        let session = &mut self.sessions[self.active];
        let change = session.view.resolve_approval(&call_id, decision);
        session.apply(change);
        if let Some(ops) = &session.ops {
            ops.try_send(Op::Approval { call_id, decision }).ok();
        }
        if decision == ApprovalDecision::ApproveAlways {
            self.approval = ApprovalMode::Auto;
        }
        cx.notify();
    }

    /// Answers the oldest unanswered approval in the active session.
    pub fn answer_pending(&mut self, decision: ApprovalDecision, cx: &mut Context<Self>) {
        if let Some((call_id, _, _)) = self.session().view.pending_approval() {
            self.answer_approval(call_id, decision, cx);
        }
    }

    /// Cycles reasoning effort and tells every running engine.
    pub fn cycle_effort(&mut self, cx: &mut Context<Self>) {
        self.effort = match self.effort {
            None | Some(ReasoningEffort::High) => Some(ReasoningEffort::Low),
            Some(ReasoningEffort::Low) => Some(ReasoningEffort::Medium),
            Some(ReasoningEffort::Medium) => Some(ReasoningEffort::High),
        };
        self.broadcast_effort();
        cx.notify();
    }

    pub(crate) fn broadcast_effort(&self) {
        for session in &self.sessions {
            if let Some(ops) = &session.ops {
                ops.try_send(Op::SetReasoningEffort(self.effort)).ok();
            }
        }
    }
}
