//! One approval inbox for the parent and all children, addressed by call id.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::oneshot;

use crate::protocol::ApprovalDecision;

#[derive(Default)]
pub(crate) struct Approvals {
    pub always: AtomicBool,
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>,
}

impl Approvals {
    pub fn register(&self, id: &str) -> Option<oneshot::Receiver<ApprovalDecision>> {
        let mut pending = self.pending.lock().expect("approval inbox");
        if self.always.load(Ordering::Relaxed) {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        pending.insert(id.to_string(), tx);
        Some(rx)
    }

    pub fn remove(&self, id: &str) {
        self.pending.lock().expect("approval inbox").remove(id);
    }

    pub fn respond(&self, id: &str, decision: ApprovalDecision) {
        let mut pending = self.pending.lock().expect("approval inbox");
        if let Some(tx) = pending.remove(id) {
            if decision == ApprovalDecision::ApproveAlways {
                self.always.store(true, Ordering::Relaxed);
                for (_, waiting) in pending.drain() {
                    let _ = waiting.send(ApprovalDecision::Approve);
                }
            }
            let _ = tx.send(decision);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approve_always_releases_all_pending_calls_and_future_calls() {
        let approvals = Approvals::default();
        let first = approvals.register("agent-1:edit").expect("first");
        let second = approvals.register("agent-2:edit").expect("second");
        approvals.respond("unknown", ApprovalDecision::ApproveAlways);
        assert!(!approvals.always.load(Ordering::Relaxed));
        approvals.respond("agent-1:edit", ApprovalDecision::ApproveAlways);
        assert_eq!(first.await.expect("first"), ApprovalDecision::ApproveAlways);
        assert_eq!(second.await.expect("second"), ApprovalDecision::Approve);
        assert!(approvals.register("agent-3:edit").is_none());
    }
}
