//! Queue mutations and delivery. Only completed turns advance the queue;
//! stopping, failures and restart leave pending work paused for review.

use flint_agent::{AgentKind, Op, TurnEndReason};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::*;

use crate::app::FlintApp;
use crate::prompt_queue::Prompt;
use crate::store;

pub struct QueueEdit {
    pub uid: u64,
    pub id: u64,
    pub input: Entity<TextareaState>,
    _subscription: Subscription,
}

impl FlintApp {
    pub fn prompt_busy(&self, ix: usize) -> bool {
        self.sessions[ix].view.running
            || self.sessions[ix].prompt_pending
            || self.sessions[ix].steering_pending.is_some()
    }

    pub(crate) fn save_prompt_queue(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        if self.options.ephemeral() {
            return true;
        }
        let session = &mut self.sessions[ix];
        if session.prompt_queue.unreadable {
            cx.notify();
            return false;
        }
        if session.dir.is_none() {
            session.dir = Some(store::sessions_dir(&self.home).join(store::new_id()));
        }
        let result = session
            .save_meta()
            .and_then(|()| session.prompt_queue.save(session.dir.as_deref().unwrap()));
        if let Err(error) = result {
            session.prompt_queue.paused = true;
            session.prompt_queue.error = Some(format!(
                "Couldn't save the prompt queue: {error}. Pending work is paused."
            ));
            cx.notify();
            return false;
        }
        true
    }

    pub(crate) fn enqueue_prompt(
        &mut self,
        ix: usize,
        prompt: Prompt,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let id = match self.sessions[ix].prompt_queue.enqueue(prompt) {
            Ok(id) => id,
            Err(error) => {
                self.sessions[ix].prompt_queue.error = Some(error);
                cx.notify();
                return None;
            }
        };
        if !self.save_prompt_queue(ix, cx) {
            self.sessions[ix].prompt_queue.remove(id);
            return None;
        }
        cx.notify();
        Some(id)
    }

    pub fn toggle_prompt_queue(&mut self, cx: &mut Context<Self>) {
        self.queue_popover = !self.queue_popover;
        self.mention = None;
        self.slash = None;
        self.option_menu = None;
        self.agent_menu = false;
        self.project_menu = None;
        cx.notify();
    }

    pub fn pause_prompt_queue(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        self.sessions[ix].prompt_queue.paused = !self.sessions[ix].prompt_queue.paused;
        self.sessions[ix].prompt_queue.error = None;
        if self.save_prompt_queue(ix, cx) && !self.sessions[ix].prompt_queue.paused {
            self.dispatch_next_prompt(self.sessions[ix].uid, cx);
        }
        cx.notify();
    }

    pub fn remove_queued_prompt(&mut self, id: u64, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.sessions[ix].steering_pending == Some(id)
            || self.sessions[ix].prompt_queue.steer_after_turn == Some(id)
        {
            return;
        }
        let old = self.sessions[ix].prompt_queue.clone();
        self.sessions[ix].prompt_queue.remove(id);
        if !self.save_prompt_queue(ix, cx) {
            let error = self.sessions[ix].prompt_queue.error.take();
            self.sessions[ix].prompt_queue = old;
            self.sessions[ix].prompt_queue.paused = true;
            self.sessions[ix].prompt_queue.error = error;
        }
        if self.queue_edit.as_ref().is_some_and(|edit| edit.id == id) {
            self.queue_edit = None;
        }
        cx.notify();
    }

    pub fn move_queued_prompt(&mut self, id: u64, delta: isize, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.sessions[ix].steering_pending == Some(id) {
            return;
        }
        let old = self.sessions[ix].prompt_queue.clone();
        self.sessions[ix].prompt_queue.move_by(id, delta);
        if !self.save_prompt_queue(ix, cx) {
            let error = self.sessions[ix].prompt_queue.error.take();
            self.sessions[ix].prompt_queue = old;
            self.sessions[ix].prompt_queue.paused = true;
            self.sessions[ix].prompt_queue.error = error;
        }
        cx.notify();
    }

    pub fn edit_queued_prompt(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.session().steering_pending == Some(id)
            || self.session().prompt_queue.steer_after_turn == Some(id)
        {
            return;
        }
        let Some(prompt) = self
            .session()
            .prompt_queue
            .items
            .iter()
            .find(|prompt| prompt.id == id)
        else {
            return;
        };
        let text = prompt.text.clone();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 5)
                .submit_on_enter(true)
        });
        input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        let subscription =
            cx.subscribe_in(&input, window, |app, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    app.save_queued_edit(window, cx);
                }
            });
        self.queue_edit = Some(QueueEdit {
            uid: self.session().uid,
            id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub fn save_queued_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.queue_edit else { return };
        if edit.uid != self.session().uid {
            self.queue_edit = None;
            return;
        }
        let text = edit.input.read(cx).value().trim().to_string();
        let id = edit.id;
        let ix = self.active;
        let Some(prompt) = self.sessions[ix]
            .prompt_queue
            .items
            .iter()
            .find(|prompt| prompt.id == id)
        else {
            self.queue_edit = None;
            return;
        };
        if text.is_empty() && prompt.images.is_empty() {
            self.sessions[ix].prompt_queue.error =
                Some("A queued prompt needs text or an image.".into());
            cx.notify();
            return;
        }
        let updated = prompt.with_text(text);
        let old = self.sessions[ix].prompt_queue.clone();
        if let Err(error) = self.sessions[ix].prompt_queue.replace(id, updated) {
            self.sessions[ix].prompt_queue.error = Some(error);
            cx.notify();
            return;
        }
        if self.save_prompt_queue(ix, cx) {
            self.queue_edit = None;
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            self.dispatch_next_prompt(self.sessions[ix].uid, cx);
        } else {
            let error = self.sessions[ix].prompt_queue.error.take();
            self.sessions[ix].prompt_queue = old;
            self.sessions[ix].prompt_queue.paused = true;
            self.sessions[ix].prompt_queue.error = error;
        }
        cx.notify();
    }

    pub fn steer_queued_prompt(&mut self, id: u64, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.sessions[ix].stopping
            || self.sessions[ix].steering_pending.is_some()
            || self.sessions[ix].prompt_queue.steer_after_turn.is_some()
            || !self.storage_ready(ix, cx)
        {
            return;
        }
        let Some(prompt) = self.sessions[ix]
            .prompt_queue
            .items
            .iter()
            .find(|prompt| prompt.id == id)
            .cloned()
        else {
            return;
        };
        if !self.sessions[ix].view.running && !self.sessions[ix].prompt_pending {
            self.dispatch_queued_prompt(ix, id, cx);
            return;
        }
        let Some(ops) = self.sessions[ix].ops.clone().filter(|ops| !ops.is_closed()) else {
            return;
        };
        if self.sessions[ix].agent == AgentKind::Flint {
            if ops
                .try_send(Op::SteerMessage {
                    id,
                    text: prompt.message,
                    images: prompt.images,
                })
                .is_ok()
            {
                self.sessions[ix].steering_pending = Some(id);
            }
        } else if ops.try_send(Op::Interrupt).is_ok() {
            self.sessions[ix].prompt_queue.steer_after_turn = Some(id);
        }
        if self.queue_edit.as_ref().is_some_and(|edit| edit.id == id) {
            self.queue_edit = None;
        }
        cx.notify();
    }

    pub(crate) fn acknowledge_steering(&mut self, ix: usize, id: u64, cx: &mut Context<Self>) {
        if self.sessions[ix].steering_pending != Some(id) {
            return;
        }
        let Some(prompt) = self.sessions[ix].prompt_queue.remove(id) else {
            return;
        };
        self.sessions[ix].steering_pending = None;
        self.record_prompt(ix, &prompt, crate::app_engine::PromptRecord::Batched, cx);
        self.save_prompt_queue(ix, cx);
    }

    pub(crate) fn queue_turn_finished(
        &mut self,
        uid: u64,
        reason: &TurnEndReason,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.sessions[ix].stopping
            || self.sessions[ix].history_save_failed
            || self.sessions[ix].storage_failed()
            || self.sessions[ix].view.running
        {
            return;
        }
        if self.sessions[ix]
            .ops
            .as_ref()
            .is_none_or(|ops| ops.is_closed())
        {
            self.queue_engine_stopped(ix, cx);
            return;
        }
        if let Some(id) = self.sessions[ix].prompt_queue.steer_after_turn.take() {
            self.dispatch_queued_prompt(ix, id, cx);
        } else if matches!(reason, TurnEndReason::Completed) {
            self.dispatch_next_prompt(uid, cx);
        } else if !self.sessions[ix].prompt_queue.items.is_empty() {
            self.sessions[ix].prompt_queue.paused = true;
            self.save_prompt_queue(ix, cx);
        }
    }

    pub(crate) fn dispatch_next_prompt(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.prompt_busy(ix)
            || self.sessions[ix].stopping
            || self.sessions[ix].prompt_queue.paused
            || self.sessions[ix].history_save_failed
            || self.sessions[ix].storage_failed()
            || self.queue_edit.as_ref().is_some_and(|edit| edit.uid == uid)
        {
            return;
        }
        if let Some(id) = self.sessions[ix]
            .prompt_queue
            .items
            .front()
            .map(|prompt| prompt.id)
        {
            self.dispatch_queued_prompt(ix, id, cx);
        }
    }

    fn dispatch_queued_prompt(&mut self, ix: usize, id: u64, cx: &mut Context<Self>) {
        if self.prompt_busy(ix) || self.sessions[ix].stopping || !self.storage_ready(ix, cx) {
            return;
        }
        let Some(prompt) = self.sessions[ix]
            .prompt_queue
            .items
            .iter()
            .find(|prompt| prompt.id == id)
            .cloned()
        else {
            return;
        };
        if self.dispatch_prompt(ix, prompt.shown, prompt.message, prompt.images, cx) {
            self.sessions[ix].prompt_queue.remove(id);
            self.save_prompt_queue(ix, cx);
        } else {
            self.sessions[ix].prompt_queue.paused = true;
            self.save_prompt_queue(ix, cx);
        }
    }

    pub(crate) fn queue_engine_stopped(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.sessions[ix].prompt_pending = false;
        self.sessions[ix].steering_pending = None;
        self.sessions[ix].prompt_queue.steer_after_turn = None;
        if !self.sessions[ix].prompt_queue.items.is_empty() {
            self.sessions[ix].prompt_queue.paused = true;
            self.save_prompt_queue(ix, cx);
        }
    }

    pub fn steer_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session().selected_subagent.is_some() {
            return;
        }
        let ix = self.active;
        if self.permission_choice_open
            || self.pending_image_pastes > 0
            || self.sessions[ix].stopping
            || self.sessions[ix].steering_pending.is_some()
            || self.sessions[ix].prompt_queue.steer_after_turn.is_some()
        {
            return;
        }
        if !self.prompt_busy(ix) {
            self.submit(window, cx);
            return;
        }
        if let Some(prompt) = self.prepare_composer_prompt(cx)
            && let Some(id) = self.enqueue_prompt(ix, prompt, cx)
        {
            self.clear_submitted_draft(window, cx);
            self.steer_queued_prompt(id, cx);
        }
    }
}
