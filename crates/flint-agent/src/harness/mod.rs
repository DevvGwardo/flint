//! The harness: guardrails that make a cheap model finish tasks.
//!
//! - Between steps, [`Harness::before_step`] returns a stuck nudge when the
//!   turn is looping (confirmed one repeat early by the JEV judge if present).
//! - When the model stops, [`Harness::continuation`] returns a nudge for one
//!   more pass (leaked call, unverified edits, a failed last check, nothing
//!   done on a change request, open plan steps), at most
//!   [`MAX_CONTINUATIONS`] times per turn.
//! - Tool-call arguments and names are repaired in [`args`].

pub mod args;
pub mod guard;
pub mod jev;
pub mod leaked;

use serde_json::Value;

use crate::protocol::JevQuestion;
use crate::protocol::JevVerdict;
use crate::protocol::NudgeReason;
use crate::protocol::ToolKind;
use crate::task::{ContinuationStop, ContinuationStopCause};
use guard::FinishCheck;
use guard::TurnGuard;
use guard::WATCHDOG_NUDGE;
use jev::JevClient;

/// Extra passes the harness may request per turn: room for each finish rule
/// to fire once (leaked call, verify, failed check, watchdog or open plan).
pub const MAX_CONTINUATIONS: u32 = 4;
/// JEV requests per turn: a judgment is a nudge, not a conversation.
pub const JEV_MAX_CALLS_PER_TURN: u32 = 6;
const JEV_STUCK_NUDGE_THRESHOLD: f64 = 0.6;
const JEV_VERIFY_NUDGE_THRESHOLD: f64 = 0.5;
const JEV_WATCHDOG_NUDGE_THRESHOLD: f64 = 0.6;

/// Harness state for one user turn.
type FinishNudge = (NudgeReason, guard::FinishProgress);

pub struct Harness {
    guard: TurnGuard,
    jev: Option<JevClient>,
    jev_calls: u32,
    /// Verdicts the judge returned this turn, drained by the session to emit.
    jev_verdicts: Vec<JevVerdict>,
    continuations: u32,
    last_finish_nudge: Option<FinishNudge>,
    continuation_stop: Option<ContinuationStop>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskRecord;

    #[tokio::test]
    async fn repeated_finish_nudges_without_real_progress_stop_as_incomplete() {
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        let mut harness = Harness::new("Fix the code", None);
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { .. }
        ));
        // Possible shell effects may bump the revision, but no actual edit
        // or accepted check was observed.
        task.set_work_revision(1);
        harness.set_task_record(&task);
        assert_eq!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::NoProgress
        );
    }

    #[tokio::test]
    async fn actual_edits_allow_a_new_bounded_verification_followup() {
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        let mut harness = Harness::new("Fix the code", None);
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { .. }
        ));
        task.set_work_revision(1);
        task.observed_edits = 2;
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { .. }
        ));
    }

    fn command(task: &mut TaskRecord, command: &str, order: u64, revision: u64) {
        task.observe_command(
            command,
            command,
            crate::task::CommandTiming {
                started: revision,
                finished: revision,
                order,
            },
            &crate::tools::ToolOutcome {
                output: "ok".into(),
                exit_code: Some(0),
                success: true,
                diff: None,
            },
            crate::task::is_verification(command),
        );
    }

    #[tokio::test]
    async fn passing_bun_checks_then_install_allow_recovery_but_never_certify_stale_work() {
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        let mut harness = Harness::new("Fix the code", None);
        command(&mut task, "bun test | tail -40", 1, 0);
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { .. }
        ));
        command(&mut task, "bun run typecheck", 2, 0);
        command(&mut task, "bun test", 3, 0);
        harness.set_task_record(&task);
        command(&mut task, "bun install --frozen-lockfile", 4, 2);
        harness.set_task_record(&task);
        assert!(!task.has_current_verification());
        task.assess(crate::TurnEndReason::Completed, true);
        assert!(!task.verified_for_queue());
        assert!(matches!(harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { message, .. } if message.contains("bun install --frozen-lockfile")
                && !message.contains("The shell wrapper")));
        command(&mut task, "bun run typecheck", 5, 2);
        command(&mut task, "bun test", 6, 2);
        harness.set_task_record(&task);
        assert_eq!(
            harness.finish_decision("checked", &[]).await,
            FinishDecision::Stop
        );
        assert!(task.has_current_verification());
        assert!(harness.continuation_stop().is_none());
    }

    #[tokio::test]
    async fn checking_then_invalidating_forever_still_exhausts_the_fixed_budget() {
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        let mut harness = Harness::new("Fix the code", None);
        for pass in 0..=MAX_CONTINUATIONS {
            command(
                &mut task,
                "bun test",
                u64::from(pass) * 2 + 1,
                u64::from(pass) * 2,
            );
            command(
                &mut task,
                "bun install --frozen-lockfile",
                u64::from(pass) * 2 + 2,
                u64::from(pass) * 2 + 2,
            );
            harness.set_task_record(&task);
            let decision = harness.finish_decision("done", &[]).await;
            if pass < MAX_CONTINUATIONS {
                assert!(matches!(decision, FinishDecision::Continue { .. }));
            } else {
                assert_eq!(decision, FinishDecision::BudgetExhausted);
            }
        }
        let stop = harness.continuation_stop().unwrap();
        assert_eq!(stop.cause, ContinuationStopCause::BudgetExhausted);
        assert_eq!(stop.followups, MAX_CONTINUATIONS);
        assert!(!task.has_current_verification());
    }

    #[tokio::test]
    async fn overlapping_check_epochs_are_not_recovery_progress() {
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        let mut harness = Harness::new("Fix the code", None);
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::Continue { .. }
        ));
        command(&mut task, "bun test", 1, 1);
        task.checks[0].started_revision = 0;
        harness.set_task_record(&task);
        assert_eq!(
            harness.finish_decision("done", &[]).await,
            FinishDecision::NoProgress
        );
        let stop = harness.continuation_stop().unwrap();
        assert_eq!(stop.cause, ContinuationStopCause::NoProgress);
        assert_eq!(stop.followups, 1);
    }
}

/// Bounded execution control. `Stop` is not a judgment of task acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishDecision {
    Continue {
        reason: NudgeReason,
        message: String,
    },
    Stop,
    BudgetExhausted,
    NoProgress,
}

impl Harness {
    pub fn new(user_message: &str, jev: Option<JevClient>) -> Self {
        Self {
            guard: TurnGuard::new(user_message),
            jev,
            jev_calls: 0,
            jev_verdicts: Vec::new(),
            continuations: 0,
            last_finish_nudge: None,
            continuation_stop: None,
        }
    }

    pub fn record_tool_call(
        &mut self,
        name: &str,
        kind: ToolKind,
        args: &Value,
        path: Option<&str>,
    ) {
        self.guard.record_tool_call(name, kind, args, path);
    }

    pub fn record_tool_result(
        &mut self,
        name: &str,
        kind: ToolKind,
        args: &Value,
        output: &str,
        exit_code: Option<i32>,
        success: bool,
    ) {
        self.guard
            .record_tool_result(name, kind, args, output, exit_code, success);
    }

    /// A message the user sent while the turn was running.
    pub fn add_user_message(&mut self, text: &str) {
        self.guard.add_user_message(text);
    }

    pub fn set_task_contract(&mut self, contract: &crate::task::TaskContract) {
        self.guard.set_task_contract(contract);
    }

    pub fn set_task_record(&mut self, task: &crate::task::TaskRecord) {
        self.guard.set_task_record(task);
    }

    /// The model's latest `update_plan`: the steps still open.
    pub fn record_plan(&mut self, open: Vec<String>) {
        self.guard.record_plan(open);
    }

    /// Asks the judge about `question` and records the verdict. Returns the
    /// probability and whether the guard acts on it, or `None` without a
    /// verdict. `nudge_on_high` says which way the threshold points: a high
    /// probability confirms a stuck loop or a missing edit, but a high
    /// probability clears the verify check.
    async fn judge(
        &mut self,
        state: Value,
        question: JevQuestion,
        threshold: f64,
        nudge_on_high: bool,
    ) -> Option<(f64, bool)> {
        if self.jev_calls >= JEV_MAX_CALLS_PER_TURN {
            return None;
        }
        let jev = self.jev.as_ref()?;
        self.jev_calls += 1;
        let probability = jev
            .ask_noul(&state, question.name(), question.noul())
            .await?;
        let confirmed = if nudge_on_high {
            probability >= threshold
        } else {
            probability < threshold
        };
        self.jev_verdicts.push(JevVerdict {
            question,
            probability,
            confirmed,
        });
        Some((probability, confirmed))
    }

    /// Verdicts the judge returned since the last drain, for the front end.
    pub fn take_jev_verdicts(&mut self) -> Vec<JevVerdict> {
        std::mem::take(&mut self.jev_verdicts)
    }

    /// A stuck nudge to append before the next model call, if any.
    pub async fn before_step(&mut self) -> Option<String> {
        if let Some(suspect) = self.guard.take_stuck_suspect() {
            let state = self.guard.stall_state();
            if let Some((_, confirmed)) = self
                .judge(state, JevQuestion::Stuck, JEV_STUCK_NUDGE_THRESHOLD, true)
                .await
                && confirmed
            {
                self.guard.confirm_stuck(&suspect);
            }
        }
        self.guard.take_stuck_nudge()
    }

    /// After the model stopped with `final_text`: a nudge for one more pass,
    /// or `None` to finish the turn.
    pub async fn continuation(
        &mut self,
        final_text: &str,
        tool_names: &[&str],
    ) -> Option<(NudgeReason, String)> {
        match self.finish_decision(final_text, tool_names).await {
            FinishDecision::Continue { reason, message } => Some((reason, message)),
            FinishDecision::Stop | FinishDecision::BudgetExhausted | FinishDecision::NoProgress => {
                None
            }
        }
    }

    pub async fn finish_decision(
        &mut self,
        final_text: &str,
        tool_names: &[&str],
    ) -> FinishDecision {
        self.continuation_stop = None;
        let nudge = if let Some(message) = self.guard.analysis_scope_nudge(final_text) {
            Some((NudgeReason::Watchdog, message))
        } else {
            match leaked::detect_leaked_tool_call(final_text, tool_names) {
                Some(call) => Some((NudgeReason::LeakedCall, leaked::leaked_call_nudge(&call))),
                None => self.judged_finish_nudge(final_text).await,
            }
        };
        match nudge {
            Some((_, message)) if self.continuations >= MAX_CONTINUATIONS => {
                self.continuation_stop = Some(ContinuationStop {
                    cause: ContinuationStopCause::BudgetExhausted,
                    followups: self.continuations,
                    condition: crate::tools::head_tail(&message, 3_800),
                });
                FinishDecision::BudgetExhausted
            }
            Some((reason, message)) => {
                if let Some(progress) = self.guard.finish_progress() {
                    if self.last_finish_nudge.as_ref().is_some_and(
                        |(previous_reason, previous_progress)| {
                            previous_reason == &reason && previous_progress == &progress
                        },
                    ) {
                        self.continuation_stop = Some(ContinuationStop {
                            cause: ContinuationStopCause::NoProgress,
                            followups: self.continuations,
                            condition: crate::tools::head_tail(&message, 3_800),
                        });
                        return FinishDecision::NoProgress;
                    }
                    self.last_finish_nudge = Some((reason, progress));
                }
                self.continuations += 1;
                FinishDecision::Continue { reason, message }
            }
            None => FinishDecision::Stop,
        }
    }

    pub fn continuation_stop(&self) -> Option<ContinuationStop> {
        self.continuation_stop.clone()
    }

    async fn judged_finish_nudge(&mut self, final_text: &str) -> Option<(NudgeReason, String)> {
        match self.guard.finish_check() {
            Some(FinishCheck::Verify) => {
                let state = self.guard.verify_state(final_text);
                if let Some((_, confirmed)) = self
                    .judge(
                        state,
                        JevQuestion::Verified,
                        JEV_VERIFY_NUDGE_THRESHOLD,
                        false,
                    )
                    .await
                {
                    self.guard.mark_nudged(FinishCheck::Verify);
                    return if confirmed {
                        Some((NudgeReason::Verify, self.guard.verification_nudge()))
                    } else {
                        self.guard.factual_finish()
                    };
                }
            }
            Some(FinishCheck::Watchdog) => {
                let state = self.guard.watchdog_state(final_text);
                if let Some((_, confirmed)) = self
                    .judge(
                        state,
                        JevQuestion::Watchdog,
                        JEV_WATCHDOG_NUDGE_THRESHOLD,
                        true,
                    )
                    .await
                {
                    self.guard.mark_nudged(FinishCheck::Watchdog);
                    return if confirmed {
                        Some((NudgeReason::Watchdog, WATCHDOG_NUDGE.to_string()))
                    } else {
                        self.guard.factual_finish()
                    };
                }
            }
            None => {}
        }
        self.guard.before_finish()
    }
}
