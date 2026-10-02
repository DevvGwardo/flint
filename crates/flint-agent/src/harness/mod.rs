//! The harness: guardrails that make a cheap model finish tasks.
//!
//! - Between steps, [`Harness::before_step`] returns a stuck nudge when the
//!   turn is looping (confirmed one repeat early by the JEV judge if present).
//! - When the model stops, [`Harness::continuation`] returns a nudge for one
//!   more pass (leaked call, unverified edits, nothing done on a change
//!   request), at most [`MAX_CONTINUATIONS`] times per turn.
//! - Tool-call arguments and names are repaired in [`args`].

pub mod args;
pub mod guard;
pub mod jev;
pub mod leaked;

use serde_json::Value;

use crate::protocol::NudgeReason;
use crate::protocol::ToolKind;
use guard::FinishCheck;
use guard::TurnGuard;
use guard::VERIFY_NUDGE;
use guard::WATCHDOG_NUDGE;
use jev::JevClient;
use jev::NoulQuestion;

/// Extra passes the harness may request per turn.
pub const MAX_CONTINUATIONS: u32 = 2;
/// JEV requests per turn: a judgment is a nudge, not a conversation.
pub const JEV_MAX_CALLS_PER_TURN: u32 = 6;
const JEV_STUCK_NUDGE_THRESHOLD: f64 = 0.6;
const JEV_VERIFY_NUDGE_THRESHOLD: f64 = 0.5;
const JEV_WATCHDOG_NUDGE_THRESHOLD: f64 = 0.6;

/// Harness state for one user turn.
pub struct Harness {
    guard: TurnGuard,
    jev: Option<JevClient>,
    jev_calls: u32,
    continuations: u32,
}

impl Harness {
    pub fn new(user_message: &str, jev: Option<JevClient>) -> Self {
        Self {
            guard: TurnGuard::new(user_message),
            jev,
            jev_calls: 0,
            continuations: 0,
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

    async fn judge(&mut self, state: Value, name: &str, question: NoulQuestion) -> Option<f64> {
        if self.jev_calls >= JEV_MAX_CALLS_PER_TURN {
            return None;
        }
        let jev = self.jev.as_ref()?;
        self.jev_calls += 1;
        jev.ask_noul(&state, name, question).await
    }

    /// A stuck nudge to append before the next model call, if any.
    pub async fn before_step(&mut self) -> Option<String> {
        if let Some(suspect) = self.guard.take_stuck_suspect() {
            let state = self.guard.stall_state();
            if let Some(p_stuck) = self.judge(state, "stuck", jev::STUCK_QUESTION).await
                && p_stuck >= JEV_STUCK_NUDGE_THRESHOLD
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
        if self.continuations >= MAX_CONTINUATIONS {
            return None;
        }
        let nudge = match leaked::detect_leaked_tool_call(final_text, tool_names) {
            Some(call) => Some((NudgeReason::LeakedCall, leaked::leaked_call_nudge(&call))),
            None => self.judged_finish_nudge(final_text).await,
        };
        if nudge.is_some() {
            self.continuations += 1;
        }
        nudge
    }

    async fn judged_finish_nudge(&mut self, final_text: &str) -> Option<(NudgeReason, String)> {
        match self.guard.finish_check() {
            Some(FinishCheck::Verify) => {
                let state = self.guard.verify_state(final_text);
                if let Some(p_verified) = self.judge(state, "verified", jev::VERIFY_QUESTION).await
                {
                    if p_verified >= JEV_VERIFY_NUDGE_THRESHOLD {
                        return None;
                    }
                    self.guard.mark_nudged(FinishCheck::Verify);
                    return Some((NudgeReason::Verify, VERIFY_NUDGE.to_string()));
                }
            }
            Some(FinishCheck::Watchdog) => {
                let state = self.guard.watchdog_state(final_text);
                if let Some(p_needs_edits) =
                    self.judge(state, "watchdog", jev::WATCHDOG_QUESTION).await
                {
                    if p_needs_edits < JEV_WATCHDOG_NUDGE_THRESHOLD {
                        return None;
                    }
                    self.guard.mark_nudged(FinishCheck::Watchdog);
                    return Some((NudgeReason::Watchdog, WATCHDOG_NUDGE.to_string()));
                }
            }
            None => {}
        }
        self.guard.before_finish()
    }
}
