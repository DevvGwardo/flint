//! Opt-in recovery of the current protected native task, not queue advancement
//! or permission to turn a read-only review into implementation.

use flint_agent::{
    AgentKind, AutomaticReviewVerdict, TaskMode, TaskOutcome, TaskRecord, TaskSource, TurnEndReason,
};
use gpui_kit::*;

use crate::app::FlintApp;

pub const MAX_AUTO_CONTINUATIONS: u32 = 3;

#[derive(Default)]
pub struct AutoContinueState {
    pub attempts: u32,
    pub paused_reason: Option<&'static str>,
    previous: Option<TaskRecord>,
}

impl AutoContinueState {
    pub(crate) fn pause(&mut self, reason: &'static str) {
        self.paused_reason = Some(reason);
    }

    fn prepare(&mut self, task: &TaskRecord) -> bool {
        if self.paused_reason.is_some() {
            return false;
        }
        if self.attempts >= MAX_AUTO_CONTINUATIONS {
            self.pause(
                "The three automatic follow-ups were used. Review the task before continuing.",
            );
            return false;
        }
        if self.previous.as_ref().is_some_and(|previous| {
            previous.contract.id == task.contract.id
                && previous.contract.revision == task.contract.revision
                && previous.work_revision == task.work_revision
                && previous.blockers == task.blockers
                && previous.files == task.files
                && previous.plan == task.plan
                && previous
                    .assessment
                    .as_ref()
                    .map(|a| (&a.issues, &a.requirements))
                    == task
                        .assessment
                        .as_ref()
                        .map(|a| (&a.issues, &a.requirements))
        }) {
            self.pause("No recovery progress was observed. Review the task before continuing.");
            return false;
        }
        self.previous = Some(task.clone());
        self.attempts += 1;
        true
    }
}

fn needs_recovery(task: &TaskRecord) -> bool {
    task.source == TaskSource::Native
        && task.contract.mode == TaskMode::Work
        && !task.is_accepted()
        && !task.evidence_overflow
        && task.continuation_stop.is_none()
        && task.scope_denials.is_empty()
        && !task
            .blockers
            .iter()
            .any(|blocker| blocker.source == "user review")
        && task.assessment.as_ref().is_some_and(|assessment| {
            assessment.task_id == task.contract.id
                && assessment.contract_revision == task.contract.revision
                && assessment.work_revision == task.work_revision
                && assessment.reason == TurnEndReason::Completed
                && matches!(
                    assessment.outcome,
                    TaskOutcome::Partial | TaskOutcome::Blocked
                )
        })
}

impl FlintApp {
    pub(crate) fn auto_continue_task(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let session = &self.sessions[ix];
        if !self.settings.auto_continue_fixes
            || session.agent != AgentKind::Flint
            || self.prompt_busy(ix)
            || session.stopping
            || session.history_save_failed
            || session.storage_failed()
            || session.native_history_conflict
            || session.prompt_queue.save_failed
            || session.prompt_queue.unreadable
            || session.steering_pending.is_some()
            || session.view.pending_approvals > 0
            || session.view.has_pending_approval()
            || !matches!(session.view.last_reason, Some(TurnEndReason::Completed))
            || session.ops.as_ref().is_none_or(|ops| ops.is_closed())
        {
            return false;
        }
        let Some(task) = session.view.task.as_ref().cloned() else {
            return false;
        };
        if self.sessions[ix].auto_continue.paused_reason.is_some() {
            return false;
        }
        if task.can_automatically_review() {
            self.hold_queue_for_review(ix, cx);
            if self.sessions[ix].prompt_queue.save_failed {
                return false;
            }
            return self.dispatch_automatic_review(
                ix,
                task.contract.id,
                task.contract.revision,
                task.work_revision,
                cx,
            );
        }
        if task.automatic_review.as_ref().is_some_and(|review| {
            review.verdict == AutomaticReviewVerdict::Blocked
                && review.work_revision == task.work_revision
                && review.contract_revision == task.contract.revision
        }) {
            self.sessions[ix].auto_continue.pause(
                "Independent review is blocked on evidence or access. Review its findings before continuing.",
            );
            return false;
        }
        if !needs_recovery(&task) {
            return false;
        }
        self.hold_queue_for_review(ix, cx);
        if self.sessions[ix].prompt_queue.save_failed {
            return false;
        }
        if !self.sessions[ix].auto_continue.prepare(&task) {
            return false;
        }
        self.dispatch_automatic_task_continuation(ix, task.contract.id, task.contract.revision, cx)
    }

    pub fn resume_automatic_task(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        if !self.settings.auto_continue_fixes
            || self.prompt_busy(ix)
            || self.sessions[ix].stopping
            || !self.storage_ready(ix, cx)
        {
            return;
        }
        if let Err(error) = self.ensure_engine(ix, cx) {
            self.store_error = Some(format!("{error:#}"));
            cx.notify();
            return;
        }
        self.sessions[ix].auto_continue = Default::default();
        self.auto_continue_task(ix, cx);
        cx.notify();
    }
}
