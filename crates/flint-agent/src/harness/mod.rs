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
const JEV_BLOCKER_RECOVERY_THRESHOLD: f64 = 0.6;

/// Harness state for one user turn.
type FinishNudge = (NudgeReason, guard::FinishProgress);

pub struct Harness {
    guard: TurnGuard,
    jev: Option<JevClient>,
    jev_calls: u32,
    /// An unavailable optional judge must not add repeated timeouts this turn.
    jev_unavailable: bool,
    /// Verdicts the judge returned this turn, drained by the session to emit.
    jev_verdicts: Vec<JevVerdict>,
    continuations: u32,
    last_finish_nudge: Option<FinishNudge>,
    continuation_stop: Option<ContinuationStop>,
    blocker_recovery_nudged: bool,
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

    #[tokio::test]
    async fn protected_blockers_never_reach_the_recovery_judge_or_nudge() {
        for source in ["user review", "child dependency", "agent report"] {
            let mut task = TaskRecord::new(1, "Analyze the source.".into(), 0);
            task.add_blocker("Input is unavailable", source);
            if source == "agent report" {
                task.note_scope_denial("run_command", &serde_json::json!({"command":"cargo test"}));
            }
            let mut harness = Harness::new("Analyze the source.", None);
            harness.set_task_record(&task);
            assert_eq!(
                harness.finish_decision("Cannot proceed.", &[]).await,
                FinishDecision::Stop
            );
            assert!(!harness.blocker_recovery_nudged);
            assert_eq!(harness.jev_calls, 0);
        }
    }

    #[tokio::test]
    async fn blocker_recovery_obeys_the_existing_total_followup_budget() {
        let mut task = TaskRecord::new(1, "What changed?".into(), 0);
        task.add_blocker("No activity in scratch folder", "agent report");
        let mut harness = Harness::new("What changed?", None);
        harness.set_task_record(&task);
        harness.continuations = MAX_CONTINUATIONS;
        assert_eq!(
            harness.finish_decision("Cannot tell.", &[]).await,
            FinishDecision::BudgetExhausted
        );
        assert_eq!(
            harness.continuation_stop().unwrap().followups,
            MAX_CONTINUATIONS
        );
        assert_eq!(task.blockers.len(), 1);
    }

    async fn judge_fixture(
        status: &str,
    ) -> (JevClient, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = crate::JevConfig {
            api_key: "offline-fixture".into(),
            base_url: format!("http://{}", listener.local_addr().unwrap()),
            model: "fixture".into(),
        };
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let status = status.to_string();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0; 4096];
                if socket.read(&mut request).await.unwrap_or(0) == 0 {
                    continue;
                }
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = r#"{"answers":{"verified":{"noul":0.1},"blocker_recovery":{"noul":0.9},"stuck":{"noul":0.9}}}"#;
                let reply = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (JevClient::new(config), calls)
    }

    #[tokio::test]
    async fn a_failed_judge_is_attempted_once_per_turn_and_retried_next_turn() {
        let (client, calls) = judge_fixture("503 Unavailable").await;
        let mut harness = Harness::new("Fix the code", Some(client.clone()));
        for _ in 0..JEV_MAX_CALLS_PER_TURN {
            assert!(
                harness
                    .judge(serde_json::json!({}), JevQuestion::Verified, 0.5, false)
                    .await
                    .is_none()
            );
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(harness.jev_calls, 1);
        assert!(harness.take_jev_verdicts().is_empty());
        let mut task = TaskRecord::new(1, "Fix the code".into(), 0);
        task.observed_edits = 1;
        harness.set_task_record(&task);
        assert!(matches!(
            harness.finish_decision("Done.", &[]).await,
            FinishDecision::Continue {
                reason: NudgeReason::Verify,
                ..
            }
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let mut next_turn = Harness::new("Fix the code", Some(client));
        assert!(
            next_turn
                .judge(serde_json::json!({}), JevQuestion::Verified, 0.5, false)
                .await
                .is_none()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn exhausted_finish_budget_skips_judge_but_keeps_the_unresolved_condition() {
        for informational in [false, true] {
            let (client, calls) = judge_fixture("200 OK").await;
            let goal = if informational {
                "What changed?"
            } else {
                "Fix the code"
            };
            let mut task = TaskRecord::new(1, goal.into(), 0);
            if informational {
                task.add_blocker("No activity in scratch folder", "agent report");
            } else {
                task.observed_edits = 1;
            }
            let mut harness = Harness::new(goal, Some(client));
            harness.set_task_record(&task);
            harness.continuations = MAX_CONTINUATIONS;
            assert_eq!(
                harness.finish_decision("Done.", &[]).await,
                FinishDecision::BudgetExhausted
            );
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert_eq!(harness.jev_calls, 0);
            assert!(harness.take_jev_verdicts().is_empty());
            assert!(!harness.continuation_stop().unwrap().condition.is_empty());
            assert!(!task.has_current_verification() && !task.is_accepted());
        }
    }

    #[tokio::test]
    async fn exhausted_finish_budget_does_not_disable_midturn_loop_judgments() {
        let (client, calls) = judge_fixture("200 OK").await;
        let mut harness = Harness::new("Fix the code", Some(client));
        harness.continuations = MAX_CONTINUATIONS;
        assert_eq!(
            harness
                .judge(serde_json::json!({}), JevQuestion::Stuck, 0.6, true)
                .await,
            Some((0.9, true))
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
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
            jev_unavailable: false,
            jev_verdicts: Vec::new(),
            continuations: 0,
            last_finish_nudge: None,
            continuation_stop: None,
            blocker_recovery_nudged: false,
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
        if self.jev_unavailable
            || self.jev_calls >= JEV_MAX_CALLS_PER_TURN
            || (question != JevQuestion::Stuck && self.continuations >= MAX_CONTINUATIONS)
        {
            return None;
        }
        let jev = self.jev.as_ref()?;
        self.jev_calls += 1;
        let Some(probability) = jev.ask_noul(&state, question.name(), question.noul()).await else {
            // Keep deterministic guards active. A new turn gets a fresh chance
            // to use JEV; a transient service failure never changes permissions.
            self.jev_unavailable = true;
            return None;
        };
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
        let nudge = if self.guard.has_blockers() {
            if !self.blocker_recovery_nudged && self.guard.can_recover_blocker() {
                self.blocker_recovery_nudged = true;
                let mut state = self.guard.blocker_state(final_text);
                state["available_tools"] = serde_json::json!(
                    tool_names
                        .iter()
                        .take(40)
                        .map(|name| name.chars().take(160).collect::<String>())
                        .collect::<Vec<_>>()
                );
                let recover = self
                    .judge(
                        state,
                        JevQuestion::BlockerRecovery,
                        JEV_BLOCKER_RECOVERY_THRESHOLD,
                        true,
                    )
                    .await
                    .is_none_or(|(_, confirmed)| confirmed);
                recover.then(|| {
                    (
                        NudgeReason::BlockerRecovery,
                        guard::BLOCKER_RECOVERY_NUDGE.into(),
                    )
                })
            } else {
                None
            }
        } else if let Some(message) = self.guard.analysis_scope_nudge(final_text) {
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
