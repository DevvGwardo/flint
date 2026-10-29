//! Queue mutations and delivery. Only eligible completed turns advance the queue;
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

/// Fail-closed directory resolution for prompt-queue saves.
/// Assigns a fresh session directory when none exists, then returns a clone.
/// Returns `None` only if the directory is still missing (e.g. allocation
/// bypassed in tests), letting callers pause instead of unwrapping.
/// `pub` for integration-test coverage; not part of the UI surface.
pub fn resolve_queue_dir(
    home: &std::path::Path,
    dir: &mut Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if dir.is_none() {
        *dir = Some(store::sessions_dir(home).join(store::new_id()));
    }
    dir.clone()
}

impl FlintApp {
    pub fn prompt_busy(&self, ix: usize) -> bool {
        self.sessions[ix].view.running
            || self.sessions[ix].prompt_pending
            || self.sessions[ix].steering_pending.is_some()
            || self.sessions[ix].task_update_pending.is_some()
    }

    pub(crate) fn save_prompt_queue(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        if self.options.ephemeral()
            || (self.sessions[ix].dir.is_none() && self.sessions[ix].prompt_queue.items.is_empty())
        {
            return true;
        }
        let session = &mut self.sessions[ix];
        if session.prompt_queue.unreadable {
            cx.notify();
            return false;
        }
        let Some(dir) = resolve_queue_dir(&self.home, &mut session.dir) else {
            session.prompt_queue.save_failed = true;
            session.prompt_queue.paused = true;
            session.prompt_queue.error = Some("Couldn't save the prompt queue: missing session directory. Pending work is paused.".into());
            cx.notify();
            return false;
        };
        let result = session
            .save_meta()
            .and_then(|()| session.prompt_queue.save(&dir));
        if let Err(error) = result {
            session.prompt_queue.save_failed = true;
            session.prompt_queue.paused = true;
            session.prompt_queue.error = Some(format!(
                "Couldn't save the prompt queue: {error}. Pending work is paused."
            ));
            cx.notify();
            return false;
        }
        session.prompt_queue.save_failed = false;
        if session
            .prompt_queue
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("Couldn't save the prompt queue:"))
        {
            session.prompt_queue.error = None;
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

    fn restore_prompt_queue(&mut self, ix: usize, mut previous: crate::prompt_queue::Queue) {
        let queue = &mut self.sessions[ix].prompt_queue;
        previous.error = queue.error.take();
        previous.save_failed = queue.save_failed;
        previous.paused = true;
        *queue = previous;
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
        if self.sessions[ix].prompt_queue.paused && self.queue_needs_review(ix) {
            self.request_queue_override(None, cx);
            return;
        }
        self.sessions[ix].prompt_queue.paused = !self.sessions[ix].prompt_queue.paused;
        self.sessions[ix].prompt_queue.resume_after_review = false;
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
            self.restore_prompt_queue(ix, old);
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
            self.restore_prompt_queue(ix, old);
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
            self.restore_prompt_queue(ix, old);
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
            if self.queue_needs_review(ix) {
                self.request_queue_override(Some(id), cx);
                return;
            }
            self.dispatch_queued_prompt(ix, id, cx);
            return;
        }
        let Some(ops) = self.sessions[ix].ops.clone().filter(|ops| !ops.is_closed()) else {
            return;
        };
        if self.sessions[ix].agent == AgentKind::Flint {
            let op = if prompt.text != prompt.message {
                Op::UserMessageWithContext {
                    request: prompt.text,
                    text: prompt.message,
                    images: prompt.images,
                    steering_id: Some(id),
                }
            } else {
                Op::SteerMessage {
                    id,
                    text: prompt.message,
                    images: prompt.images,
                }
            };
            if ops.try_send(op).is_ok() {
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
        } else if matches!(reason, TurnEndReason::Completed) && self.auto_continue_task(ix, cx) {
            self.hold_queue_for_review(ix, cx);
        } else if matches!(reason, TurnEndReason::Completed) && self.queue_result_eligible(ix) {
            self.release_queue_review(ix, cx);
            self.dispatch_next_prompt(uid, cx);
        } else {
            self.hold_queue_for_review(ix, cx);
        }
    }

    pub(crate) fn dispatch_next_prompt(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.prompt_busy(ix)
            || self.sessions[ix].stopping
            || self.sessions[ix].prompt_queue.paused
            || self.sessions[ix].prompt_queue.save_failed
            || self.sessions[ix].prompt_queue.unreadable
            || self.sessions[ix].history_save_failed
            || self.sessions[ix].storage_failed()
            || self.queue_edit.as_ref().is_some_and(|edit| edit.uid == uid)
        {
            return;
        }
        if self.queue_needs_review(ix)
            && self.sessions[ix].prompt_queue.review_override_turn
                != Some(self.sessions[ix].view.turn_id)
        {
            self.hold_queue_for_review(ix, cx);
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
        if self.prompt_busy(ix)
            || self.sessions[ix].stopping
            || self.sessions[ix].prompt_queue.save_failed
            || self.sessions[ix].prompt_queue.unreadable
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
        if self.dispatch_prompt(ix, prompt, cx) {
            self.sessions[ix].prompt_queue.review_override_turn = None;
            self.sessions[ix].prompt_queue.remove(id);
            self.save_prompt_queue(ix, cx);
        } else {
            self.sessions[ix].prompt_queue.paused = true;
            self.save_prompt_queue(ix, cx);
        }
    }

    pub(crate) fn queue_needs_review(&self, ix: usize) -> bool {
        let view = &self.sessions[ix].view;
        !view.running && view.last_reason.is_some() && !self.queue_result_eligible(ix)
    }

    /// Execution completion is necessary, not acceptance. The opt-in branch
    /// delegates evidence/revision checks to the native task record.
    fn queue_result_eligible(&self, ix: usize) -> bool {
        let session = &self.sessions[ix];
        let view = &session.view;
        matches!(view.last_reason, Some(TurnEndReason::Completed))
            && view.pending_approvals == 0
            && !view.has_pending_approval()
            && (view.outcome().allows_queue()
                || (self.settings.queue_after_verification
                    && session.agent == AgentKind::Flint
                    && view
                        .task
                        .as_ref()
                        .is_some_and(|task| task.verified_for_queue())))
    }

    pub(crate) fn hold_queue_for_review(&mut self, ix: usize, cx: &mut Context<Self>) {
        let outcome = self.sessions[ix].view.outcome();
        let queue = &mut self.sessions[ix].prompt_queue;
        let hold = Some(format!(
            "{}. Review the task before running queued work.",
            outcome.label()
        ));
        let changed = queue.review_hold != hold
            || (!queue.items.is_empty() && !queue.paused)
            || queue.save_failed;
        if queue.review_hold.is_none() {
            queue.resume_after_review = !queue.paused && !queue.items.is_empty();
        }
        queue.review_hold = hold;
        if !queue.items.is_empty() {
            queue.paused = true;
        }
        if changed {
            self.save_prompt_queue(ix, cx);
        }
    }

    fn release_queue_review(&mut self, ix: usize, cx: &mut Context<Self>) {
        let queue = &mut self.sessions[ix].prompt_queue;
        let resume = queue.resume_after_review;
        let held = queue.review_hold.take().is_some();
        let changed = held || resume || queue.save_failed;
        queue.resume_after_review = false;
        if held && resume {
            queue.paused = false;
        }
        self.queue_resume_confirm = None;
        if changed {
            self.save_prompt_queue(ix, cx);
        }
    }

    pub(crate) fn queue_task_reviewed(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.prompt_busy(ix) {
            return;
        }
        if !self.sessions[ix].view.running
            && self.queue_result_eligible(ix)
            && self.sessions[ix].prompt_queue.review_hold.is_some()
        {
            self.release_queue_review(ix, cx);
            self.dispatch_next_prompt(self.sessions[ix].uid, cx);
        } else if self.queue_needs_review(ix) {
            self.hold_queue_for_review(ix, cx);
        }
    }

    pub fn request_queue_override(&mut self, prompt_id: Option<u64>, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.prompt_busy(ix) || !self.queue_needs_review(ix) {
            return;
        }
        self.queue_resume_confirm = Some(crate::task_ui::QueueConfirmation {
            key: crate::task_ui::review_key(&self.sessions[ix]),
            prompt_id,
        });
        self.queue_popover = true;
        cx.notify();
    }

    pub fn confirm_queue_override(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.queue_resume_confirm.take() else {
            return;
        };
        let ix = self.active;
        if confirm.key != crate::task_ui::review_key(&self.sessions[ix])
            || self.prompt_busy(ix)
            || self.sessions[ix].stopping
        {
            return;
        }
        let turn_id = self.sessions[ix].view.turn_id;
        let queue = &mut self.sessions[ix].prompt_queue;
        queue.paused = false;
        queue.review_hold = None;
        queue.resume_after_review = false;
        queue.review_override_turn = Some(turn_id);
        if self.save_prompt_queue(ix, cx) {
            if let Some(id) = confirm.prompt_id {
                self.dispatch_queued_prompt(ix, id, cx);
            } else {
                self.dispatch_next_prompt(self.sessions[ix].uid, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn queue_engine_stopped(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.sessions[ix].prompt_pending = false;
        self.sessions[ix].steering_pending = None;
        self.sessions[ix].prompt_queue.steer_after_turn = None;
        // An engine stop is an explicit pause, not a releasable review hold.
        self.sessions[ix].prompt_queue.resume_after_review = false;
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
