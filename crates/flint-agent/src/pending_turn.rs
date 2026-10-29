//! Stop targets an active turn, or the first accepted turn still awaiting
//! startup. A later submission owns a different token and remains runnable.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct State {
    active: Option<Arc<CancellationToken>>,
    pending: VecDeque<Arc<CancellationToken>>,
}

#[derive(Default)]
pub(crate) struct StartFence {
    state: Mutex<State>,
    bootstrap: CancellationToken,
}

pub(crate) struct QueuedStart {
    fence: Arc<StartFence>,
    token: Arc<CancellationToken>,
}

impl StartFence {
    pub(crate) fn queue(self: &Arc<Self>) -> QueuedStart {
        let token = Arc::new(CancellationToken::new());
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .push_back(Arc::clone(&token));
        QueuedStart {
            fence: Arc::clone(self),
            token,
        }
    }

    pub(crate) fn interrupt(&self) {
        self.bootstrap.cancel();
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(token) = state.active.as_ref().or_else(|| state.pending.front()) {
            token.cancel();
        }
    }

    pub(crate) fn bootstrap_token(&self) -> CancellationToken {
        self.bootstrap.clone()
    }
}

impl QueuedStart {
    pub(crate) fn enter(&self) -> CancellationToken {
        let mut state = self
            .fence
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .pending
            .retain(|token| !Arc::ptr_eq(token, &self.token));
        state.active = Some(Arc::clone(&self.token));
        self.token.as_ref().clone()
    }
}

impl Drop for QueuedStart {
    fn drop(&mut self) {
        let mut state = self
            .fence
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .pending
            .retain(|token| !Arc::ptr_eq(token, &self.token));
        if state
            .active
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &self.token))
        {
            state.active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_before_start_cannot_be_reset_by_a_fresh_turn_token() {
        let fence = Arc::new(StartFence::default());
        let first = fence.queue();
        let later = fence.queue();
        fence.interrupt();
        assert!(first.enter().is_cancelled());
        drop(first);
        assert!(!later.enter().is_cancelled());
    }

    #[test]
    fn stop_active_turn_preserves_pending_work_and_future_submissions() {
        let fence = Arc::new(StartFence::default());
        let first = fence.queue();
        let token = first.enter();
        let later = fence.queue();
        fence.interrupt();
        assert!(token.is_cancelled());
        drop(first);
        assert!(!later.enter().is_cancelled());
        drop(later);
        assert!(!fence.queue().enter().is_cancelled());
    }

    #[test]
    fn idle_interrupt_does_not_poison_the_next_explicit_submission() {
        let fence = Arc::new(StartFence::default());
        fence.interrupt();
        assert!(fence.bootstrap_token().is_cancelled());
        assert!(!fence.queue().enter().is_cancelled());
    }

    #[test]
    fn consumed_steering_does_not_leave_a_phantom_pending_start() {
        let fence = Arc::new(StartFence::default());
        let current = fence.queue();
        let active = current.enter();
        let steering = fence.queue();
        drop(steering);
        fence.interrupt();
        assert!(active.is_cancelled());
        drop(current);
        assert!(!fence.queue().enter().is_cancelled());
    }
}
