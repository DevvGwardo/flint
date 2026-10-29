//! Bounded, task-scoped reminders. This module cannot activate executable code.
//!
//! Only the native host may supply turn boundaries and completed task evidence.
//! Saved data selects code-owned text; no saved or model-authored text is a prompt.
use crate::orchestration::fs::WriterLease;
use crate::{TaskMode, TaskOutcome, TaskRecord, TaskSource, TurnEndReason};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::io::{self, Write};
use std::path::Path;

pub const MAX_STORE_BYTES: usize = 256 * 1024;
pub const MAX_TASK_BYTES: usize = 1024 * 1024;
pub const MAX_CANDIDATES: usize = 32;
pub const MAX_EVIDENCE: usize = 64;
pub const MAX_EVENTS: usize = 128;
const FILE: &str = "registry.json";
const ROLLBACK_RESERVE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintKind {
    DirectVerificationReminder,
    FreshVerificationReminder,
    CloseOpenPlanReminder,
}

impl HintKind {
    /// The entire prompt contribution. Never interpolate evidence into this text.
    pub fn prompt_text(self) -> &'static str {
        match self {
            Self::DirectVerificationReminder => {
                "For this task, directly verify the changed work with the relevant permitted checks before reporting completion. If verification is unavailable, report that limitation; do not claim it passed."
            }
            Self::FreshVerificationReminder => {
                "For this task, rerun the relevant permitted verification after the final change. Earlier checks are not current after subsequent changes; report any remaining verification limitation honestly."
            }
            Self::CloseOpenPlanReminder => {
                "Before finishing this task, reconcile open plan steps with the observed work. Mark only genuinely completed steps complete, explicitly drop superseded steps, and leave unresolved work open with an honest explanation."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Storage,
    Corrupt,
    SourceMismatch,
    BoundaryOrder,
    Limit,
    Poisoned,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Storage => "Private improvement storage is unavailable; use baseline behavior.",
            Self::Corrupt => "Invalid improvement registry; preserve it and use baseline behavior.",
            Self::SourceMismatch => "Improvement evidence does not match the native task boundary.",
            Self::BoundaryOrder => "Improvement boundaries must be ordered and task-bound.",
            Self::Limit => "Improvement registry limit reached; no change was applied.",
            Self::Poisoned => "Improvement storage requires reopening; use baseline behavior.",
        })
    }
}

impl std::error::Error for Error {}

/// Host provenance, not a model receipt. All fields must match the current turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSource {
    pub session_uid: String,
    pub turn_id: u64,
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckFact {
    pub id: u64,
    pub command_sha256: String,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub started_revision: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub verification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: u64,
    pub recorded_revision: u64,
    pub source: EvidenceSource,
    /// SHA-256 of serde_json's compact serialization of the original TaskRecord.
    /// The task's free text is deliberately not stored in this registry.
    pub task_sha256: String,
    pub outcome: TaskOutcome,
    pub observed_edits: u64,
    pub checks: Vec<CheckFact>,
    pub plan_steps: u16,
    pub open_plan_steps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Pending,
    Active,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: u64,
    pub kind: HintKind,
    pub evidence_id: u64,
    pub status: CandidateStatus,
    pub activated_revision: Option<u64>,
    pub activated_turn: Option<u64>,
    pub rolled_back_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Observed {
        evidence_id: u64,
        candidate_ids: Vec<u64>,
    },
    Activated {
        candidate_ids: Vec<u64>,
        turn_id: u64,
    },
    Enabled {
        enabled: bool,
    },
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub revision: u64,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub turn_id: u64,
    pub task_id: Option<u64>,
    pub contract_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySnapshot {
    pub version: u32,
    pub session_uid: String,
    pub revision: u64,
    pub enabled: bool,
    pub boundary: Option<Boundary>,
    pub evidence: Vec<Evidence>,
    pub candidates: Vec<Candidate>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryHints {
    pub revision: u64,
    pub turn_id: u64,
    pub enabled: bool,
    pub kinds: Vec<HintKind>,
}

impl BoundaryHints {
    pub fn prompt_texts(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.kinds.iter().map(|kind| kind.prompt_text())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Ignored,
    Duplicate,
    Staged { candidate_ids: Vec<u64> },
}

/// One writer per dedicated registry directory, independent of history's lease.
/// Parents must be trusted host-owned directories, never model-selected paths.
pub struct ImprovementStore {
    lease: WriterLease,
    state: RegistrySnapshot,
    saved: Vec<u8>,
    poisoned: bool,
    #[cfg(test)]
    fault: Option<&'static str>,
}

impl ImprovementStore {
    /// Use a dedicated child such as `<session-directory>/improvement`.
    /// Corruption is an error, never an excuse to overwrite the saved registry.
    pub fn open(path: &Path, session_uid: &str) -> Result<Self, Error> {
        if !valid_uid(session_uid) {
            return Err(Error::SourceMismatch);
        }
        // Unix ownership/mode/link checks below are part of this lane's
        // contract. Do not silently weaken it on an unvalidated platform.
        if !cfg!(unix) {
            return Err(Error::Storage);
        }
        private_directory_if_present(path)?;
        let lease = WriterLease::acquire(path).map_err(|_| Error::Storage)?;
        private_directory_if_present(path)?;
        private_file(&lease, ".flint-writer.lock", MAX_STORE_BYTES)?;
        let bytes = lease
            .directory
            .read_optional(FILE, MAX_STORE_BYTES)
            .map_err(|_| Error::Storage)?;
        let (state, saved) = if let Some(bytes) = bytes {
            private_file(&lease, FILE, MAX_STORE_BYTES)?;
            let state: RegistrySnapshot =
                serde_json::from_slice(&bytes).map_err(|_| Error::Corrupt)?;
            validate(&state, session_uid)?;
            (state, bytes)
        } else {
            let state = RegistrySnapshot {
                version: 1,
                session_uid: session_uid.into(),
                revision: 0,
                enabled: true,
                boundary: None,
                evidence: Vec::new(),
                candidates: Vec::new(),
                events: Vec::new(),
            };
            let bytes = serialize(&state)?;
            lease
                .directory
                .replace(FILE, &bytes)
                .map_err(|_| Error::Storage)?;
            (state, bytes)
        };
        Ok(Self {
            lease,
            state,
            saved,
            poisoned: false,
            #[cfg(test)]
            fault: None,
        })
    }

    /// A detached, inspectable copy; modifying it cannot change this store.
    pub fn inspect(&self) -> Result<RegistrySnapshot, Error> {
        self.check_disk()?;
        Ok(self.state.clone())
    }

    /// Call once before constructing a native turn prompt. Repeating the same
    /// boundary is idempotent and cannot activate candidates learned in that turn.
    /// Hints never cross task IDs or contract revisions, even within a session.
    pub fn read_boundary(
        &mut self,
        turn_id: u64,
        task: Option<&TaskRecord>,
    ) -> Result<BoundaryHints, Error> {
        self.check_disk()?;
        if turn_id == 0 {
            return Err(Error::BoundaryOrder);
        }
        let scope = task.filter(|task| {
            task.source == TaskSource::Native && task.contract.mode == TaskMode::Work
        });
        if let Some(task) = scope {
            task.validate().map_err(|_| Error::SourceMismatch)?;
        }
        let boundary = Boundary {
            turn_id,
            task_id: scope.map(|task| task.contract.id),
            contract_revision: scope.map(|task| task.contract.revision),
        };
        if let Some(previous) = &self.state.boundary {
            if turn_id < previous.turn_id || (turn_id == previous.turn_id && previous != &boundary)
            {
                return Err(Error::BoundaryOrder);
            }
            if previous == &boundary {
                return Ok(self.hints(&boundary));
            }
        }
        let mut next = self.state.clone();
        next.revision = increment(next.revision)?;
        next.boundary = Some(boundary.clone());
        let mut activated = Vec::new();
        if next.enabled {
            for candidate in &mut next.candidates {
                let evidence = &next.evidence[candidate.evidence_id as usize - 1];
                if candidate.status == CandidateStatus::Pending
                    && evidence.source.turn_id < turn_id
                    && boundary.task_id == Some(evidence.source.task_id)
                    && boundary.contract_revision == Some(evidence.source.contract_revision)
                {
                    candidate.status = CandidateStatus::Active;
                    candidate.activated_revision = Some(next.revision);
                    candidate.activated_turn = Some(turn_id);
                    activated.push(candidate.id);
                }
            }
        }
        if !activated.is_empty() {
            event(
                &mut next,
                Action::Activated {
                    candidate_ids: activated,
                    turn_id,
                },
            )?;
        }
        self.commit(next)?;
        Ok(self.hints(&boundary))
    }

    /// Observe only after the host has installed the completed assessment.
    /// No prompt, tool, permission, budget, or executable candidate is accepted.
    pub fn observe_completed_task(
        &mut self,
        source: &EvidenceSource,
        task: &TaskRecord,
    ) -> Result<Observation, Error> {
        self.check_disk()?;
        let boundary = self.state.boundary.as_ref().ok_or(Error::SourceMismatch)?;
        if source.session_uid != self.state.session_uid
            || source.turn_id != boundary.turn_id
            || Some(source.task_id) != boundary.task_id
            || Some(source.contract_revision) != boundary.contract_revision
            || source.task_id != task.contract.id
            || source.contract_revision != task.contract.revision
            || source.work_revision != task.work_revision
            || task.source != TaskSource::Native
        {
            return Err(Error::SourceMismatch);
        }
        task.validate().map_err(|_| Error::SourceMismatch)?;
        if !self.state.enabled || !eligible(task) {
            return Ok(Observation::Ignored);
        }
        let evidence = evidence(source, task)?;
        if self
            .state
            .evidence
            .iter()
            .any(|old| old.source == *source && old.task_sha256 != evidence.task_sha256)
        {
            return Err(Error::SourceMismatch);
        }
        let kinds: Vec<_> = [
            HintKind::DirectVerificationReminder,
            HintKind::FreshVerificationReminder,
            HintKind::CloseOpenPlanReminder,
        ]
        .into_iter()
        .filter(|kind| supports(&evidence, *kind))
        .filter(|kind| {
            !self.state.candidates.iter().any(|candidate| {
                let old = &self.state.evidence[candidate.evidence_id as usize - 1];
                candidate.kind == *kind
                    && old.source.task_id == source.task_id
                    && old.source.contract_revision == source.contract_revision
            })
        })
        .collect();
        if kinds.is_empty() {
            return Ok(
                if self.state.evidence.iter().any(|old| {
                    old.source.task_id == source.task_id
                        && old.source.contract_revision == source.contract_revision
                        && old.source.work_revision == source.work_revision
                }) {
                    Observation::Duplicate
                } else {
                    Observation::Ignored
                },
            );
        }
        if self.state.evidence.len() >= MAX_EVIDENCE
            || self.state.candidates.len() + kinds.len() > MAX_CANDIDATES
        {
            return Err(Error::Limit);
        }
        let mut next = self.state.clone();
        next.revision = increment(next.revision)?;
        let mut evidence = evidence;
        evidence.id = next.evidence.len() as u64 + 1;
        evidence.recorded_revision = next.revision;
        let evidence_id = evidence.id;
        next.evidence.push(evidence);
        let mut ids = Vec::new();
        for kind in kinds {
            let id = next.candidates.len() as u64 + 1;
            next.candidates.push(Candidate {
                id,
                kind,
                evidence_id,
                status: CandidateStatus::Pending,
                activated_revision: None,
                activated_turn: None,
                rolled_back_revision: None,
            });
            ids.push(id);
        }
        event(
            &mut next,
            Action::Observed {
                evidence_id,
                candidate_ids: ids.clone(),
            },
        )?;
        self.commit(next)?;
        Ok(Observation::Staged { candidate_ids: ids })
    }

    /// Explicit host control. Reenabling does not itself activate pending hints.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), Error> {
        self.check_disk()?;
        if self.state.enabled == enabled {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.revision = increment(next.revision)?;
        next.enabled = enabled;
        event(&mut next, Action::Enabled { enabled })?;
        self.commit(next)
    }

    /// Durable safe baseline: disable learning/output and terminally retire all
    /// existing candidates. Old evidence cannot resurrect them after reenabling.
    pub fn rollback(&mut self) -> Result<(), Error> {
        self.check_disk()?;
        if !self.state.enabled
            && self
                .state
                .candidates
                .iter()
                .all(|candidate| candidate.status == CandidateStatus::RolledBack)
        {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.revision = increment(next.revision)?;
        next.enabled = false;
        for candidate in &mut next.candidates {
            if candidate.status != CandidateStatus::RolledBack {
                candidate.status = CandidateStatus::RolledBack;
                candidate.rolled_back_revision = Some(next.revision);
            }
        }
        event(&mut next, Action::Rollback)?;
        self.commit(next)
    }

    fn hints(&self, boundary: &Boundary) -> BoundaryHints {
        let kinds = if self.state.enabled {
            self.state
                .candidates
                .iter()
                .filter(|candidate| {
                    let source = &self.state.evidence[candidate.evidence_id as usize - 1].source;
                    candidate.status == CandidateStatus::Active
                        && boundary.task_id == Some(source.task_id)
                        && boundary.contract_revision == Some(source.contract_revision)
                })
                .map(|candidate| candidate.kind)
                .collect()
        } else {
            Vec::new()
        };
        BoundaryHints {
            revision: self.state.revision,
            turn_id: boundary.turn_id,
            enabled: self.state.enabled,
            kinds,
        }
    }

    fn check_disk(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        private_file(&self.lease, FILE, MAX_STORE_BYTES)?;
        if self
            .lease
            .directory
            .read(FILE, MAX_STORE_BYTES)
            .map_err(|_| Error::Storage)?
            != self.saved
        {
            return Err(Error::Corrupt);
        }
        Ok(())
    }

    fn commit(&mut self, next: RegistrySnapshot) -> Result<(), Error> {
        validate(&next, &self.state.session_uid)?;
        let bytes = serialize(&next)?;
        if bytes.len() > MAX_STORE_BYTES - ROLLBACK_RESERVE_BYTES
            && next.events.last().is_none_or(|event| {
                event.revision != next.revision || event.action != Action::Rollback
            })
        {
            return Err(Error::Limit);
        }
        self.check_disk()?;
        // A publication error can happen after rename. Never keep using a
        // possibly divergent in-memory state, even if the old file still exists.
        #[cfg(not(test))]
        let result = self.lease.directory.replace(FILE, &bytes);
        #[cfg(test)]
        let result = self.lease.directory.replace_with(
            FILE,
            &bytes,
            || {
                if self.fault == Some("before") {
                    Err(io::Error::other("Injected pre-publication failure."))
                } else {
                    Ok(())
                }
            },
            || {
                if self.fault == Some("after") {
                    Err(io::Error::other("Injected post-publication failure."))
                } else {
                    Ok(())
                }
            },
        );
        if result.is_err() {
            self.poisoned = true;
            return Err(Error::Storage);
        }
        self.state = next;
        self.saved = bytes;
        Ok(())
    }
}

fn eligible(task: &TaskRecord) -> bool {
    task.contract.mode == TaskMode::Work
        && task.assessment.as_ref().is_some_and(|assessment| {
            assessment.reason == TurnEndReason::Completed
                && matches!(
                    assessment.outcome,
                    TaskOutcome::Partial | TaskOutcome::NeedsReview
                )
        })
        && task.blockers.is_empty()
        && task.scope_denials.is_empty()
        && task.children.is_empty()
        && task.acceptance.is_none()
        && task.automatic_review.is_none()
        && task.continuation_stop.is_none()
        && !task.unknown_effects
        && !task.evidence_overflow
        && task.checks.iter().all(|check| check.success)
}

fn evidence(source: &EvidenceSource, task: &TaskRecord) -> Result<Evidence, Error> {
    let mut hash = BoundedHash {
        hash: Sha256::new(),
        remaining: MAX_TASK_BYTES,
    };
    serde_json::to_writer(&mut hash, task).map_err(|_| Error::Limit)?;
    Ok(Evidence {
        id: 0,
        recorded_revision: 0,
        source: source.clone(),
        task_sha256: format!("{:x}", hash.hash.finalize()),
        outcome: task
            .assessment
            .as_ref()
            .ok_or(Error::SourceMismatch)?
            .outcome,
        observed_edits: u64::try_from(task.observed_edits).map_err(|_| Error::Limit)?,
        checks: task
            .checks
            .iter()
            .map(|check| CheckFact {
                id: check.id,
                command_sha256: format!("{:x}", Sha256::digest(check.command.as_bytes())),
                contract_revision: check.contract_revision,
                work_revision: check.work_revision,
                started_revision: check.started_revision,
                success: check.success,
                exit_code: check.exit_code,
                verification: check.verification,
            })
            .collect(),
        plan_steps: task.plan.len() as u16,
        open_plan_steps: task
            .plan
            .iter()
            .filter(|step| step.status.is_open())
            .count() as u16,
    })
}

fn supports(evidence: &Evidence, kind: HintKind) -> bool {
    let any = evidence.checks.iter().any(|check| check.verification);
    let current = evidence.checks.iter().any(|check| {
        check.verification
            && check.success
            && check.exit_code == Some(0)
            && check.contract_revision == evidence.source.contract_revision
            && check.started_revision == check.work_revision
            && check.work_revision == evidence.source.work_revision
    });
    match kind {
        HintKind::DirectVerificationReminder => evidence.observed_edits > 0 && !any,
        HintKind::FreshVerificationReminder => {
            evidence.observed_edits > 0
                && !current
                && evidence.checks.iter().any(|check| {
                    check.verification
                        && check.success
                        && check.exit_code == Some(0)
                        && check.contract_revision == evidence.source.contract_revision
                        && check.started_revision == check.work_revision
                        && check.work_revision < evidence.source.work_revision
                })
        }
        HintKind::CloseOpenPlanReminder => evidence.open_plan_steps > 0,
    }
}

fn event(state: &mut RegistrySnapshot, action: Action) -> Result<(), Error> {
    // Reserve the last audit slot for a terminal rollback, never evict history.
    let limit = if action == Action::Rollback {
        MAX_EVENTS
    } else {
        MAX_EVENTS - 1
    };
    if state.events.len() >= limit {
        return Err(Error::Limit);
    }
    state.events.push(Event {
        revision: state.revision,
        action,
    });
    Ok(())
}

fn increment(value: u64) -> Result<u64, Error> {
    value
        .checked_add(1)
        .filter(|value| *value < u64::MAX)
        .ok_or(Error::Limit)
}

fn valid_uid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

struct BoundedHash {
    hash: Sha256,
    remaining: usize,
}

impl Write for BoundedHash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("Task evidence exceeds its byte limit."));
        }
        self.hash.update(bytes);
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize(state: &RegistrySnapshot) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|_| Error::Corrupt)?;
    if bytes.len() > MAX_STORE_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

fn validate(state: &RegistrySnapshot, uid: &str) -> Result<(), Error> {
    if state.version != 1
        || state.session_uid != uid
        || !valid_uid(uid)
        || state.revision == u64::MAX
        || state.evidence.len() > MAX_EVIDENCE
        || state.candidates.len() > MAX_CANDIDATES
        || state.events.len() > MAX_EVENTS
        || (state.events.len() == MAX_EVENTS
            && (state.enabled
                || state
                    .events
                    .last()
                    .is_none_or(|event| event.action != Action::Rollback)))
        || state.boundary.as_ref().is_some_and(|boundary| {
            boundary.turn_id == 0
                || boundary.task_id.is_none() != boundary.contract_revision.is_none()
                || boundary.task_id == Some(0)
                || boundary.contract_revision == Some(0)
        })
        || (state.revision == 0
            && (state.boundary.is_some()
                || !state.evidence.is_empty()
                || !state.candidates.is_empty()
                || !state.events.is_empty()
                || !state.enabled))
    {
        return Err(Error::Corrupt);
    }
    for (index, evidence) in state.evidence.iter().enumerate() {
        if evidence.id != index as u64 + 1
            || evidence.recorded_revision == 0
            || evidence.recorded_revision > state.revision
            || evidence.source.session_uid != uid
            || evidence.source.task_id == 0
            || evidence.source.contract_revision == 0
            || evidence.source.turn_id == 0
            || state
                .boundary
                .as_ref()
                .is_none_or(|boundary| evidence.source.turn_id > boundary.turn_id)
            || !valid_digest(&evidence.task_sha256)
            || !matches!(
                evidence.outcome,
                TaskOutcome::Partial | TaskOutcome::NeedsReview
            )
            || evidence.checks.len() > 64
            || evidence.plan_steps > 50
            || evidence.open_plan_steps > evidence.plan_steps
        {
            return Err(Error::Corrupt);
        }
        for (index, check) in evidence.checks.iter().enumerate() {
            if check.id == 0
                || !check.success
                || !valid_digest(&check.command_sha256)
                || check.contract_revision == 0
                || check.contract_revision > evidence.source.contract_revision
                || check.started_revision > check.work_revision
                || check.work_revision > evidence.source.work_revision
                || evidence.checks[..index]
                    .iter()
                    .any(|old| old.id == check.id)
            {
                return Err(Error::Corrupt);
            }
        }
    }
    for (index, candidate) in state.candidates.iter().enumerate() {
        let evidence = candidate
            .evidence_id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .and_then(|index| state.evidence.get(index))
            .ok_or(Error::Corrupt)?;
        if candidate.id != index as u64 + 1
            || !supports(evidence, candidate.kind)
            || state.candidates[..index].iter().any(|old| {
                let source = &state.evidence[old.evidence_id as usize - 1].source;
                old.kind == candidate.kind
                    && source.task_id == evidence.source.task_id
                    && source.contract_revision == evidence.source.contract_revision
            })
            || candidate.activated_revision.is_some() != candidate.activated_turn.is_some()
            || candidate.activated_revision.is_some_and(|revision| {
                revision <= evidence.recorded_revision || revision > state.revision
            })
            || candidate.activated_turn.is_some_and(|turn| {
                turn <= evidence.source.turn_id
                    || state
                        .boundary
                        .as_ref()
                        .is_none_or(|boundary| turn > boundary.turn_id)
            })
            || candidate.rolled_back_revision.is_some_and(|revision| {
                revision <= evidence.recorded_revision
                    || revision > state.revision
                    || candidate
                        .activated_revision
                        .is_some_and(|active| revision <= active)
            })
            || match candidate.status {
                CandidateStatus::Pending => {
                    candidate.activated_revision.is_some()
                        || candidate.rolled_back_revision.is_some()
                }
                CandidateStatus::Active => {
                    candidate.activated_revision.is_none()
                        || candidate.rolled_back_revision.is_some()
                }
                CandidateStatus::RolledBack => candidate.rolled_back_revision.is_none(),
            }
        {
            return Err(Error::Corrupt);
        }
    }
    validate_events(state)
}

/// Replay the bounded audit log to reject fabricated state/status combinations.
fn validate_events(state: &RegistrySnapshot) -> Result<(), Error> {
    let mut enabled = true;
    let mut observed = vec![false; state.evidence.len()];
    let mut statuses = vec![None; state.candidates.len()];
    let mut last = 0;
    for event in &state.events {
        if event.revision <= last || event.revision > state.revision {
            return Err(Error::Corrupt);
        }
        last = event.revision;
        match &event.action {
            Action::Observed {
                evidence_id,
                candidate_ids,
            } => {
                let index = evidence_id
                    .checked_sub(1)
                    .and_then(|id| usize::try_from(id).ok())
                    .filter(|index| *index < observed.len())
                    .ok_or(Error::Corrupt)?;
                if !enabled
                    || observed[index]
                    || candidate_ids.is_empty()
                    || candidate_ids.len() > 3
                    || state.evidence[index].recorded_revision != event.revision
                {
                    return Err(Error::Corrupt);
                }
                observed[index] = true;
                for id in candidate_ids {
                    let index = candidate_index(state, *id)?;
                    if statuses[index].is_some()
                        || state.candidates[index].evidence_id != *evidence_id
                    {
                        return Err(Error::Corrupt);
                    }
                    statuses[index] = Some(CandidateStatus::Pending);
                }
            }
            Action::Activated {
                candidate_ids,
                turn_id,
            } => {
                if !enabled || candidate_ids.is_empty() || candidate_ids.len() > MAX_CANDIDATES {
                    return Err(Error::Corrupt);
                }
                for id in candidate_ids {
                    let index = candidate_index(state, *id)?;
                    let candidate = &state.candidates[index];
                    if statuses[index] != Some(CandidateStatus::Pending)
                        || candidate.activated_revision != Some(event.revision)
                        || candidate.activated_turn != Some(*turn_id)
                    {
                        return Err(Error::Corrupt);
                    }
                    statuses[index] = Some(CandidateStatus::Active);
                }
            }
            Action::Enabled { enabled: value } => {
                if enabled == *value {
                    return Err(Error::Corrupt);
                }
                enabled = *value;
            }
            Action::Rollback => {
                for (index, status) in statuses.iter_mut().enumerate() {
                    if status.is_some() && *status != Some(CandidateStatus::RolledBack) {
                        if state.candidates[index].rolled_back_revision != Some(event.revision) {
                            return Err(Error::Corrupt);
                        }
                        *status = Some(CandidateStatus::RolledBack);
                    }
                }
                enabled = false;
            }
        }
    }
    if enabled != state.enabled
        || observed.iter().any(|observed| !observed)
        || statuses
            .iter()
            .zip(&state.candidates)
            .any(|(status, candidate)| *status != Some(candidate.status))
    {
        return Err(Error::Corrupt);
    }
    Ok(())
}

fn candidate_index(state: &RegistrySnapshot, id: u64) -> Result<usize, Error> {
    id.checked_sub(1)
        .and_then(|id| usize::try_from(id).ok())
        .filter(|index| *index < state.candidates.len())
        .ok_or(Error::Corrupt)
}

fn private_directory_if_present(path: &Path) -> Result<(), Error> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Error::Storage),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Storage);
    }
    private_metadata(&metadata)
}

fn private_file(lease: &WriterLease, name: &str, limit: usize) -> Result<(), Error> {
    let metadata = lease
        .directory
        .reader(name, limit)
        .map_err(|_| Error::Storage)?
        .metadata()
        .map_err(|_| Error::Storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(Error::Storage);
        }
    }
    private_metadata(&metadata)
}

fn private_metadata(metadata: &std::fs::Metadata) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(Error::Storage);
        }
    }
    #[cfg(not(unix))]
    let _ = metadata; // Windows creation ACLs/reparse checks come from repository storage.
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "improvement_tests.rs"]
mod tests;
