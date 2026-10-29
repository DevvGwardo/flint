//! Host-owned reminder learning at native task boundaries. No executable,
//! tool, approval, acceptance or budget policy can be modified by this module.
use super::*;
use crate::improvement::{EvidenceSource, HintKind, ImprovementStore};
use crate::protocol::{IMPROVEMENT_OPTION_ID, OptionChoice, SessionOption};
use sha2::{Digest, Sha256};

impl Session {
    pub(super) fn initialize_improvements(&mut self, allowed: bool) {
        if !allowed || self.child || self.saver.is_none() {
            return;
        }
        let Some(directory) = &self.config.session_dir else {
            return;
        };
        let uid = format!(
            "{:x}",
            Sha256::digest(directory.as_os_str().as_encoded_bytes())
        );
        self.improvements = ImprovementStore::open(&directory.join("improvement"), &uid).ok();
        self.publish_improvement_option();
    }

    fn improvement_failed(&mut self) {
        self.improvements = None;
        self.boundary_hints = None;
        self.publish_improvement_option();
    }

    pub(super) fn publish_improvement_option(&self) {
        let snapshot = self
            .improvements
            .as_ref()
            .and_then(|store| store.inspect().ok());
        let enabled = snapshot.as_ref().is_some_and(|snapshot| snapshot.enabled);
        let current = if snapshot.is_none() {
            "unavailable"
        } else if enabled {
            "enabled"
        } else {
            "paused"
        };
        let description = snapshot.as_ref().map_or_else(
            || "Reminder learning is unavailable. Baseline task execution is unchanged. No saved registry was overwritten.".into(),
            |snapshot| format!(
                "Data-only verification reminders, revision {} ({} candidates). Applied only at a later turn boundary for the same task and scope. Rollback disables learning and retires all candidates. Never changes code, tools, approvals, checks or budgets.",
                snapshot.revision, snapshot.candidates.len(),
            ),
        );
        let choices = if snapshot.is_some() {
            [
                ("enabled", "Learning on"),
                ("paused", "Learning paused"),
                ("rollback", "Roll back all learned hints"),
            ]
            .into_iter()
            .map(|(value, name)| OptionChoice {
                value: value.into(),
                name: name.into(),
                description: None,
            })
            .collect()
        } else {
            Vec::new()
        };
        self.emit(AgentEvent::SessionOptions(vec![SessionOption {
            id: IMPROVEMENT_OPTION_ID.into(),
            name: "Adaptive hints".into(),
            description: Some(description),
            category: Some("native_improvements".into()),
            current: current.into(),
            choices,
        }]));
    }

    pub(super) fn set_improvement_policy(&mut self, value: &str) {
        let Some(store) = &mut self.improvements else {
            return;
        };
        let result = match value {
            "enabled" => store.set_enabled(true),
            "paused" => store.set_enabled(false),
            "rollback" => store.rollback(),
            _ => return,
        };
        self.boundary_hints = None;
        if result.is_err() {
            self.improvement_failed();
        } else {
            self.update_task_context();
            self.publish_improvement_option();
        }
    }

    pub(super) fn prepare_improvement_boundary(&mut self, cancel: &CancellationToken) {
        self.boundary_hints = None;
        if cancel.is_cancelled()
            || self.child
            || self.read_only_scope()
            || self
                .task
                .as_ref()
                .is_none_or(|task| task.contract.mode != TaskMode::Work)
        {
            return;
        }
        let Some(store) = &mut self.improvements else {
            return;
        };
        match store.read_boundary(self.turn_id, self.task.as_ref()) {
            Ok(hints) => {
                if let Some(task) = &self.task {
                    self.boundary_hints =
                        Some((task.contract.id, task.contract.revision, hints.kinds));
                }
                self.publish_improvement_option();
            }
            Err(_) => self.improvement_failed(),
        }
    }

    pub(super) fn learn_completed_task(&mut self, reason: &TurnEndReason) {
        if self.child
            || *reason != TurnEndReason::Completed
            || self
                .task
                .as_ref()
                .is_none_or(|task| task.contract.mode != TaskMode::Work)
        {
            return;
        }
        let Some(store) = &mut self.improvements else {
            return;
        };
        let Some(task) = &self.task else {
            return;
        };
        let result = store.inspect().and_then(|snapshot| {
            store.observe_completed_task(
                &EvidenceSource {
                    session_uid: snapshot.session_uid,
                    turn_id: self.turn_id,
                    task_id: task.contract.id,
                    contract_revision: task.contract.revision,
                    work_revision: task.work_revision,
                },
                task,
            )
        });
        if result.is_err() {
            self.improvement_failed();
        } else {
            self.publish_improvement_option();
        }
    }

    pub(super) fn improvement_context(&self) -> String {
        if self.child || self.read_only_scope() {
            return String::new();
        }
        let Some(task) = &self.task else {
            return String::new();
        };
        let Some((id, revision, kinds)) = &self.boundary_hints else {
            return String::new();
        };
        if task.contract.mode != TaskMode::Work
            || *id != task.contract.id
            || *revision != task.contract.revision
            || kinds.is_empty()
        {
            return String::new();
        }
        format!(
            "\nHost-owned evidence-derived reminders (not permissions or verification evidence):\n{}",
            kinds
                .iter()
                .copied()
                .map(HintKind::prompt_text)
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}
