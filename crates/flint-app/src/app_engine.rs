//! Engine traffic on the root view: sending messages (with attachments),
//! starting engines, pumping their events in coalesced batches, the
//! animation clock, interrupts, approvals and reasoning effort.

use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ImageAttachment;
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
        let started = Instant::now();
        let event_count = events.len();
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
            if let AgentEvent::SessionStopped { history_saved } = &event {
                session.engine_shutdown_verified = *history_saved;
                session.history_save_failed |= !history_saved;
            }
            if matches!(&event, AgentEvent::Error(message)
                if message.starts_with("Couldn't save engine history.")
                    || message.starts_with("Session panicked"))
            {
                session.history_save_failed = true;
            }
            if let Err(err) = session.log(Logged::Event(event.clone())) {
                self.store_error = Some(format!("Couldn't save session event: {err}"));
                // Stop production at the first storage failure, retaining all
                // queued records without allowing more turns to accumulate.
                if !session.stopping
                    && let Some(ops) = &session.ops
                    && ops.try_send(Op::Shutdown).is_ok()
                {
                    session.stopping = true;
                }
            }
            session.note_timing(&event);
            if let AgentEvent::Error(message) = &event
                && message.contains("doesn't accept reasoning_effort")
            {
                self.effort_supported = false;
            }
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
            session.record_event_edit(&event);
            let change = session.view.fold(event, now);
            session.apply(change);
        }
        let session = &mut self.sessions[ix];
        session.touched = SystemTime::now();
        if finished {
            if let Err(err) = session.save_meta() {
                self.store_error = Some(format!("Couldn't save session details: {err}"));
            }
            if ix != active {
                session.unread = true;
            }
        }
        if finished {
            self.settle_changes_async(uid, cx);
        }
        self.watch_persistence(uid, cx);
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
        crate::automation::record_event_batch(event_count, started);
    }

    pub(crate) fn settle_changes_async(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.sessions[ix].view.running {
            return;
        }
        let Some(snapshot) = self.sessions[ix].changes_snapshot() else {
            return;
        };
        let compute = cx
            .background_executor()
            .spawn(async move { snapshot.compute() });
        self.sessions[ix].changes_task = Some(cx.spawn(async move |this, cx| {
            let settled = compute.await;
            this.update(cx, |app, cx| {
                if let Some(ix) = app.session_index(uid) {
                    let session = &mut app.sessions[ix];
                    if session.apply_settled_changes(settled) {
                        crate::automation::dump_state(app);
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
    }

    fn watch_persistence(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        let session = &mut self.sessions[ix];
        if session.persistence_task.is_some() {
            return;
        }
        let Some(writer) = &session.event_writer else {
            return;
        };
        let failures = writer.failures();
        let poll = self.options.engine_poll;
        session.persistence_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let error = if poll {
                    match failures.try_recv() {
                        Ok(error) => error,
                        Err(async_channel::TryRecvError::Closed) => break,
                        Err(async_channel::TryRecvError::Empty) => {
                            cx.background_executor()
                                .timer(Duration::from_millis(8))
                                .await;
                            continue;
                        }
                    }
                } else {
                    let Ok(error) = failures.recv().await else {
                        break;
                    };
                    error
                };
                if this
                    .update(cx, |app, cx| {
                        if let Some(ix) = app.session_index(uid) {
                            let session = &mut app.sessions[ix];
                            if !session
                                .event_writer
                                .as_ref()
                                .is_some_and(store::EventWriter::has_error)
                            {
                                return;
                            }
                            app.store_error = Some(format!("Couldn't save session event: {error}"));
                            if !session.stopping
                                && let Some(ops) = &session.ops
                                && ops.try_send(Op::Shutdown).is_ok()
                            {
                                session.stopping = true;
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
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
        if self.permission_choice_open || self.pending_image_pastes > 0 {
            return;
        }
        if self.slash.is_some() {
            self.run_selected_slash(window, cx);
            return;
        }
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() && self.image_attachments.is_empty() {
            return;
        }
        if self.image_attachments.is_empty()
            && let Some(kind) = crate::agents::parse_command(&text)
        {
            self.composer
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.choose_agent(kind, window, cx);
            return;
        }
        let ix = self.active;
        if self.sessions[ix].stopping {
            return;
        }
        if !self.storage_ready(ix, cx) {
            return;
        }
        if self
            .store_error
            .as_deref()
            .is_some_and(|message| message.starts_with("Engine stopped."))
        {
            self.store_error = None;
        }
        if self.sessions[ix].view.running && self.sessions[ix].ops.is_none() {
            return;
        }
        let images: Vec<ImageAttachment> = match self
            .image_attachments
            .iter()
            .map(|path| crate::image_attach::load(path))
            .collect()
        {
            Ok(images) => images,
            Err(error) => {
                self.store_error = Some(error);
                cx.notify();
                return;
            }
        };
        let shown = images.iter().fold(text.clone(), |mut shown, image| {
            if !shown.is_empty() {
                shown.push('\n');
            }
            shown.push_str(&format!("[Image: {}]", image.name));
            shown
        });
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.mention = None;
        let attachments = std::mem::take(&mut self.attachments);
        self.image_attachments.clear();
        self.pasted_image_files.clear();
        let message = crate::mention::attach(&text, &self.sessions[ix].workspace, &attachments);
        self.send_message_with_images(ix, shown, message, images, cx);
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
        self.send_message_with_images(ix, text, message, Vec::new(), cx);
    }

    fn send_message_with_images(
        &mut self,
        ix: usize,
        text: String,
        message: String,
        images: Vec<ImageAttachment>,
        cx: &mut Context<Self>,
    ) {
        if self.sessions[ix].stopping {
            self.store_error = Some(
                "The engine is stopping. Wait for it to finish saving before continuing.".into(),
            );
            cx.notify();
            return;
        }
        if !self.storage_ready(ix, cx) {
            return;
        }
        let session = &mut self.sessions[ix];
        let change = session.view.push_user(text.clone());
        session.apply(change);
        session.touched = SystemTime::now();
        session.submitted_at = Some(Instant::now());
        session.first_token = None;
        session.first_text = None;
        session.last_message = Some((text.clone(), message.clone(), images.clone()));
        if !self.options.ephemeral() && session.dir.is_none() {
            session.dir = Some(store::sessions_dir(&self.home).join(store::new_id()));
        }
        // Save even when engine setup fails. A shown prompt must survive restart
        // before any engine can act on it.
        if let Err(err) = session
            .log(Logged::User(text))
            .and_then(|()| session.flush_records())
            .and_then(|()| session.save_meta())
        {
            self.store_error = Some(format!("Couldn't save session message: {err}"));
            if let Some(ops) = &session.ops
                && ops.try_send(Op::Shutdown).is_ok()
            {
                session.stopping = true;
            }
            self.watch_persistence(self.sessions[ix].uid, cx);
            cx.notify();
            return;
        }
        self.watch_persistence(self.sessions[ix].uid, cx);
        match self.ensure_engine(ix, cx) {
            Ok(()) => {
                if let Some(ops) = &self.sessions[ix].ops {
                    if images.is_empty() {
                        ops.try_send(Op::UserMessage(message)).ok();
                    } else {
                        ops.try_send(Op::UserMessageWithImages {
                            text: message,
                            images,
                        })
                        .ok();
                    }
                }
            }
            Err(err) => self.apply_event(ix, AgentEvent::Error(format!("{err:#}")), cx),
        }
        cx.notify();
    }

    fn storage_ready(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        if self.sessions[ix].storage_failed()
            && let Err(error) = self.sessions[ix].flush_records()
        {
            self.store_error = Some(format!(
                "Couldn't save session events: {error}. Fix storage before continuing."
            ));
            cx.notify();
            return false;
        }
        true
    }

    /// Re-sends the last message of the active session (error card "Retry").
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.sessions[ix].stopping {
            return;
        }
        let Some((text, message, images)) = self.sessions[ix].last_message.clone() else {
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
        self.send_message_with_images(ix, text, message, images, cx);
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
        self.sessions[ix].native_model = Some(config.model.clone());
        self.sessions[ix].native_approval = Some(config.approval);
        self.sessions[ix].native_allow_all = false;
        self.sessions[ix].engine_shutdown_verified = false;
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
        let poll = self.options.engine_poll;
        let uid = self.sessions[ix].uid;
        let session = &mut self.sessions[ix];
        session.ops = Some(handle.ops);
        session.pump = Some(cx.spawn(async move |this, cx| {
            loop {
                let first = if poll {
                    match events.try_recv() {
                        Ok(event) => event,
                        Err(async_channel::TryRecvError::Closed) => break,
                        Err(async_channel::TryRecvError::Empty) => {
                            cx.background_executor()
                                .timer(Duration::from_millis(8))
                                .await;
                            continue;
                        }
                    }
                } else {
                    let Ok(event) = events.recv().await else {
                        break;
                    };
                    event
                };
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
            this.update(cx, |app, cx| {
                if let Some(ix) = app.session_index(uid) {
                    app.sessions[ix].ops = None;
                    app.sessions[ix].pump = None;
                    app.sessions[ix].native_model = None;
                    app.sessions[ix].native_approval = None;
                    app.sessions[ix].native_allow_all = false;
                    if !app.sessions[ix].engine_shutdown_verified {
                        app.apply_events(
                            uid,
                            vec![AgentEvent::SessionStopped {
                                history_saved: false,
                            }],
                            app.now(),
                            cx,
                        );
                    }
                    if app.sessions[ix].view.running {
                        app.apply_events(
                            uid,
                            vec![AgentEvent::TurnFinished {
                                turn_id: app.sessions[ix].view.turn_id,
                                reason: TurnEndReason::Interrupted,
                            }],
                            app.now(),
                            cx,
                        );
                    }
                    if app.sessions[ix].stopping {
                        app.sessions[ix].stopping = false;
                        if !app.sessions[ix].storage_failed() {
                            app.store_error = Some(
                                "Engine stopped. Archive this session or continue work.".into(),
                            );
                        }
                    }
                    cx.notify();
                }
            })
            .ok();
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
        // The broad action needs its own explicit confirmation, including
        // when invoked via the A shortcut.
        if decision == ApprovalDecision::ApproveAlways
            && self.approval_confirm.as_deref() != Some(&call_id)
        {
            self.approval_confirm = Some(call_id);
            cx.notify();
            return;
        }
        self.approval_confirm = None;
        self.approval_preview = None;
        let session = &mut self.sessions[self.active];
        if decision == ApprovalDecision::ApproveAlways
            && session.agent == flint_agent::AgentKind::Flint
        {
            session.native_allow_all = true;
        }
        let change = session.view.resolve_approval_scoped(
            &call_id,
            decision,
            session.agent == flint_agent::AgentKind::Flint,
        );
        session.apply(change);
        if let Some(ops) = &session.ops {
            ops.try_send(Op::Approval { call_id, decision }).ok();
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
