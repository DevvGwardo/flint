//! Host-owned task scope and observed evidence. A model can report progress,
//! never rewrite the user's requirements or accept its own work.
//!
//! Revision numbers describe observed tool activity, not a complete filesystem
//! audit. Passing a command or finding a file does not prove semantic correctness.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::harness::guard::{git_subcommand, is_pure_question, task_asks_for_changes};
use crate::protocol::{ToolKind, TurnEndReason};
use crate::tools::{PlanStep, ToolOutcome, head_tail};

pub const TASK_VERSION: u32 = 1;
pub const MAX_REQUIREMENTS: usize = 32;
const MAX_CHECKS: usize = 64;
const MAX_FILES: usize = 64;
const MAX_BLOCKERS: usize = 32;
const MAX_REPORT_CHARS: usize = 4_000;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskMode {
    Conversation,
    Work,
    ReadOnly,
    #[default]
    Unspecified,
}

impl TaskMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conversation => "Answer",
            Self::Work => "Work",
            Self::ReadOnly => "Read only",
            Self::Unspecified => "Unspecified",
        }
    }

    pub fn infer(text: &str) -> Self {
        let lower = text.trim().to_lowercase();
        if requests_read_only(text) {
            return Self::ReadOnly;
        }
        if is_pure_question(text) {
            return Self::Conversation;
        }
        if task_asks_for_changes(text)
            || words(&lower).any(|word| {
                matches!(
                    word,
                    "decompile" | "recover" | "produce" | "deliver" | "solve"
                )
            })
        {
            return Self::Work;
        }
        if ["hello", "hi", "thanks", "thank you", "hey"]
            .iter()
            .any(|greeting| lower.trim_end_matches(['.', '!']) == *greeting)
        {
            Self::Conversation
        } else if words(&lower).any(|word| {
            matches!(
                word,
                "research" | "review" | "investigate" | "analyze" | "analyse" | "explain"
            )
        }) {
            Self::ReadOnly
        } else if lower.starts_with("do ") {
            Self::Work
        } else {
            Self::Unspecified
        }
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// Explicit read-only wording wins over incidental words such as "bug" or
/// "implementation". This is conservative intent inference, not an NLP parser.
pub fn requests_read_only(text: &str) -> bool {
    let lower = text.to_lowercase().replace('’', "'");
    [
        "research only",
        "research-only",
        "analysis only",
        "analysis-only",
        "report only",
        "report-only",
        "do not change",
        "don't change",
        "do not make changes",
        "do not make any changes",
        "don't make changes",
        "don't make any changes",
        "dont make changes",
        "dont make any changes",
        "do not edit",
        "don't edit",
        "do not modify",
        "don't modify",
        "do not implement",
        "don't implement",
        "no file changes",
        "without modifying",
        "without changing",
        "without changes",
        "read-only",
        "read only",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
        || analysis_without_implementation(&lower)
}

fn analysis_without_implementation(text: &str) -> bool {
    let mut request = text.trim();
    while let Some(rest) = [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "i want you to ",
    ]
    .iter()
    .find_map(|prefix| request.strip_prefix(prefix))
    {
        request = rest.trim_start();
    }
    let first = words(request).next().unwrap_or_default();
    if !matches!(
        first,
        "analyze"
            | "analyse"
            | "review"
            | "research"
            | "investigate"
            | "inspect"
            | "audit"
            | "explain"
            | "identify"
            | "list"
            | "find"
    ) {
        return false;
    }
    let changes = [
        "fix",
        "implement",
        "change",
        "edit",
        "write",
        "apply",
        "update",
        "create",
        "delete",
        "remove",
        "refactor",
        "add",
        "build",
        "repair",
        "modify",
        "make",
    ];
    // An explicit second action authorizes work. Incidental "gaps to fix",
    // "implementation details", and proposed fixes do not.
    let tokens: Vec<_> = words(request).collect();
    let is_action = |mut action: &[&str]| {
        while let ["please" | "then" | "also", rest @ ..]
        | ["can" | "could" | "would", "you", rest @ ..] = action
        {
            action = rest;
        }
        action.first().is_some_and(|verb| changes.contains(verb))
            && !matches!(action, ["make", "recommendations" | "suggestions", ..])
    };
    if tokens.iter().enumerate().any(|(index, word)| {
        matches!(*word, "and" | "then" | "also") && is_action(&tokens[index + 1..])
    }) {
        return false;
    }
    !request.split(['.', ';', '\n']).skip(1).any(|clause| {
        let tokens: Vec<_> = words(clause).collect();
        is_action(&tokens)
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementSpec {
    /// Semantic acceptance always requires the user's review.
    Review(String),
    /// Existence/nonempty evidence only, never content correctness.
    File(String),
    /// Exact command, in this session's workspace.
    Command(String),
}

impl RequirementSpec {
    pub fn text(&self) -> &str {
        match self {
            Self::Review(text) | Self::File(text) | Self::Command(text) => text,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Review(_) => "Review",
            Self::File(_) => "File",
            Self::Command(_) => "Check",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: u64,
    pub spec: RequirementSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskContract {
    pub id: u64,
    pub revision: u64,
    /// Exact initial user instruction, never replaced by a model summary.
    pub original_request: String,
    pub objective: String,
    pub mode: TaskMode,
    pub requirements: Vec<Requirement>,
    pub restrictions: Vec<String>,
    /// User instructions added at safe boundaries, retained across compaction.
    pub instructions: Vec<String>,
}

/// Only explicit user controls can submit this operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEdit {
    pub task_id: u64,
    pub expected_revision: u64,
    pub objective: String,
    pub mode: TaskMode,
    pub requirements: Vec<RequirementSpec>,
    pub restrictions: Vec<String>,
    pub clear_reported_blockers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    /// A conversational answer, not acceptance of a deliverable.
    Answered,
    NeedsReview,
    Partial,
    Blocked,
    /// No trustworthy assessment (old logs or an independent external loop).
    Unknown,
    /// Explicit user acceptance, never model self-certification.
    Accepted,
    /// Current checks plus a separate read-only review, not user acceptance.
    Verified,
}

impl TaskOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Answered => "Answered",
            Self::NeedsReview => "Needs review",
            Self::Partial => "Partial",
            Self::Blocked => "Blocked",
            Self::Unknown => "Unverified",
            Self::Accepted => "Accepted by you",
            Self::Verified => "Verified automatically",
        }
    }

    pub fn allows_queue(self) -> bool {
        matches!(self, Self::Answered | Self::Accepted | Self::Verified)
    }

    /// Whether a child reporting this outcome produced work the parent can build
    /// on. `Accepted` and `Verified` are the strongest outcomes, `NeedsReview`
    /// is a usable report and `Answered` is conversational. Blocked, partial and
    /// unknown work is not task success.
    pub fn is_usable_child_report(self) -> bool {
        matches!(
            self,
            Self::Answered | Self::NeedsReview | Self::Accepted | Self::Verified
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Observed,
    Passed,
    Failed,
    Stale,
    Missing,
    NeedsReview,
    Unknown,
}

impl EvidenceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observed => "Observed",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Stale => "Stale",
            Self::Missing => "Missing",
            Self::NeedsReview => "Needs review",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementAssessment {
    pub requirement_id: u64,
    pub state: EvidenceState,
    pub detail: String,
    pub evidence_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub id: u64,
    pub call_id: String,
    pub command: String,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub started_revision: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub detail: String,
    pub verification: bool,
}

impl CommandEvidence {
    pub fn state(&self, scope: u64, work: u64) -> EvidenceState {
        // A failure remains a failure until the same command passes. Do not
        // hide it as merely stale after an edit or an unrelated successful run.
        if !self.success {
            EvidenceState::Failed
        } else if self.contract_revision != scope
            || self.work_revision != work
            || self.started_revision != self.work_revision
        {
            EvidenceState::Stale
        } else {
            EvidenceState::Passed
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Present { bytes: u64, fingerprint: u64 },
    Missing,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEvidence {
    pub path: String,
    pub state: FileState,
    pub reported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub text: String,
    /// "agent report", "child <id>", or "user review".
    pub source: String,
    /// Successful inspection sequence when this claim was first recorded.
    #[serde(default)]
    pub inspection_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionEvidence {
    pub id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub tool: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDenial {
    pub contract_revision: u64,
    pub tool: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildAssessment {
    pub session_id: String,
    pub task_id: u64,
    pub outcome: TaskOutcome,
    pub work_revision: u64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAssessment {
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub outcome: TaskOutcome,
    pub reason: TurnEndReason,
    pub requirements: Vec<RequirementAssessment>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Accept,
    NeedsWork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReview {
    pub turn_id: u64,
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
    pub decision: ReviewDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub contract_revision: u64,
    pub work_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticReviewVerdict {
    Clean,
    NeedsFixes,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomaticReview {
    pub contract_revision: u64,
    pub work_revision: u64,
    pub model: String,
    pub verdict: AutomaticReviewVerdict,
    pub summary: String,
    pub findings: Vec<String>,
    pub reviewed_requirements: Vec<u64>,
    pub inspected_files: Vec<FileEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationStopCause {
    NoProgress,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationStop {
    pub cause: ContinuationStopCause,
    pub followups: u32,
    pub condition: String,
}

impl ContinuationStop {
    pub fn label(&self) -> &'static str {
        match self.cause {
            ContinuationStopCause::NoProgress => "Stopped without recovery progress",
            ContinuationStopCause::BudgetExhausted => "Continuation budget exhausted",
        }
    }

    pub fn detail(&self) -> String {
        let why = match self.cause {
            ContinuationStopCause::NoProgress => {
                "The same finish condition remained without new recovery progress"
            }
            ContinuationStopCause::BudgetExhausted => {
                "The bounded automatic follow-up budget was exhausted"
            }
        };
        format!(
            "{why} after {} follow-up(s). {}",
            self.followups, self.condition
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub version: u32,
    pub source: TaskSource,
    pub contract: TaskContract,
    pub work_revision: u64,
    pub checks: Vec<CommandEvidence>,
    pub files: Vec<FileEvidence>,
    pub blockers: Vec<Blocker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_inspection: Option<InspectionEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposed_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_denials: Vec<ScopeDenial>,
    pub children: Vec<ChildAssessment>,
    pub plan: Vec<PlanStep>,
    pub report: Option<String>,
    pub assessment: Option<TaskAssessment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_stop: Option<ContinuationStop>,
    pub acceptance: Option<Acceptance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic_review: Option<AutomaticReview>,
    pub observed_edits: usize,
    pub unknown_effects: bool,
    /// Never silently lose an acceptance condition when observation caps fill.
    pub evidence_overflow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    Native,
    External,
    Legacy,
}

impl TaskRecord {
    pub fn can_automatically_review(&self) -> bool {
        self.source == TaskSource::Native
            && self.contract.mode == TaskMode::Work
            && !self.is_accepted()
            && self.blockers.is_empty()
            && self.scope_denials.is_empty()
            && !self.evidence_overflow
            && !self.unknown_effects
            && self.continuation_stop.is_none()
            && !self.plan.iter().any(|step| step.status.is_open())
            && self.has_current_verification()
            && self.assessment.as_ref().is_some_and(|assessment| {
                assessment.task_id == self.contract.id
                    && assessment.contract_revision == self.contract.revision
                    && assessment.work_revision == self.work_revision
                    && assessment.reason == TurnEndReason::Completed
                    && assessment
                        .issues
                        .iter()
                        .all(|issue| issue.starts_with("Automatic review: "))
            })
            && self.automatic_review.as_ref().is_none_or(|review| {
                review.contract_revision != self.contract.revision
                    || review.work_revision != self.work_revision
            })
    }

    pub fn is_automatically_verified(&self) -> bool {
        self.source == TaskSource::Native
            && self.contract.mode == TaskMode::Work
            && self.blockers.is_empty()
            && self.scope_denials.is_empty()
            && !self.evidence_overflow
            && !self.unknown_effects
            && self.continuation_stop.is_none()
            && !self.plan.iter().any(|step| step.status.is_open())
            && self.has_current_verification()
            && !self
                .checks
                .iter()
                .any(|check| self.failed_verification_is_unresolved(check))
            && self.children.iter().all(|child| {
                child.issues.is_empty()
                    && matches!(
                        child.outcome,
                        TaskOutcome::Answered
                            | TaskOutcome::NeedsReview
                            | TaskOutcome::Accepted
                            | TaskOutcome::Verified
                    )
            })
            && self.automatic_review.as_ref().is_some_and(|review| {
                review.verdict == AutomaticReviewVerdict::Clean
                && review.contract_revision == self.contract.revision
                && review.work_revision == self.work_revision
                && review.findings.is_empty()
                && !review.inspected_files.is_empty()
                && review.inspected_files.iter().all(|inspected| {
                    self.files
                        .iter()
                        .any(|file| file.path == inspected.path && file.state == inspected.state)
                }) && self.contract.requirements.iter().all(|requirement| {
                review.reviewed_requirements.contains(&requirement.id) && match &requirement.spec {
                    RequirementSpec::Review(_) => true,
                    RequirementSpec::File(path) => self.files.iter().any(|file| {
                        file.path == *path
                            && matches!(file.state, FileState::Present { bytes, .. } if bytes > 0)
                    }),
                    RequirementSpec::Command(command) => self.checks.iter().any(|check| {
                        check.command == command.trim()
                            && check.state(self.contract.revision, self.work_revision)
                                == EvidenceState::Passed
                    }),
                }
            })
            })
    }

    pub(crate) fn record_automatic_review(
        &mut self,
        review: AutomaticReview,
    ) -> Result<(), String> {
        if !self.can_automatically_review()
            || review.contract_revision != self.contract.revision
            || review.work_revision != self.work_revision
        {
            return Err("Task evidence changed or is not ready for automatic review.".into());
        }
        validate_automatic_review(&review, self)?;
        let additional_files = review
            .inspected_files
            .iter()
            .filter(|inspected| !self.files.iter().any(|file| file.path == inspected.path))
            .count();
        if self.files.len().saturating_add(additional_files) > MAX_FILES {
            return Err("Independent review exceeded the tracked file evidence limit.".into());
        }
        // Keep current observations separate from immutable review receipts.
        // Comparing every refresh with an old receipt would permanently dirty
        // the work clock after the builder repairs a reviewed file.
        for inspected in &review.inspected_files {
            self.note_file(&inspected.path, false);
            if let Some(file) = self
                .files
                .iter_mut()
                .find(|file| file.path == inspected.path)
            {
                file.state = inspected.state.clone();
            }
        }
        self.automatic_review = Some(review);
        self.assess(TurnEndReason::Completed, true);
        Ok(())
    }

    /// Passing execution evidence, not semantic certification or user acceptance.
    pub fn has_current_verification(&self) -> bool {
        self.checks.iter().any(|check| {
            check.verification
                && check.state(self.contract.revision, self.work_revision) == EvidenceState::Passed
        })
    }

    pub(crate) fn failed_verification_is_unresolved(&self, check: &CommandEvidence) -> bool {
        if check.success || !check.verification {
            return false;
        }
        let required = self.contract.original_request.contains(&check.command)
            || self.contract.objective.contains(&check.command)
            || self
                .contract
                .instructions
                .iter()
                .any(|text| text.contains(&check.command))
            || self.contract.requirements.iter().any(|requirement| {
                matches!(&requirement.spec, RequirementSpec::Command(command)
                    if command.trim() == check.command)
            });
        let later_current_check = self.checks.iter().any(|later| {
            later.id > check.id
                && later.verification
                && later.state(self.contract.revision, self.work_revision) == EvidenceState::Passed
        });
        required
            || !later_current_check
            || check.exit_code != Some(1)
            || !crate::command_evidence::is_unexecuted_python_check(&check.command, &check.detail)
    }

    /// An explicit queue policy may continue after this result without accepting it.
    /// Never release incomplete, external, stale, or unassessed work this way.
    pub fn verified_for_queue(&self) -> bool {
        self.source == TaskSource::Native
            && self.blockers.is_empty()
            && !self.evidence_overflow
            && !self.plan.iter().any(|step| step.status.is_open())
            && self.has_current_verification()
            && self.assessment.as_ref().is_some_and(|assessment| {
                assessment.task_id == self.contract.id
                    && assessment.contract_revision == self.contract.revision
                    && assessment.work_revision == self.work_revision
                    && assessment.outcome == TaskOutcome::NeedsReview
                    && assessment.reason == TurnEndReason::Completed
                    && assessment.issues.is_empty()
            })
    }

    pub fn new(id: u64, request: String, work_revision: u64) -> Self {
        let mode = TaskMode::infer(&request);
        let mut task = Self {
            version: TASK_VERSION,
            source: TaskSource::Native,
            contract: TaskContract {
                id,
                revision: 1,
                objective: if request.trim().is_empty() {
                    "Review the supplied images (no textual objective was provided).".into()
                } else {
                    request.clone()
                },
                original_request: request,
                mode,
                requirements: vec![Requirement {
                    id: 1,
                    spec: RequirementSpec::Review("Deliver the requested result".into()),
                }],
                restrictions: Vec::new(),
                instructions: Vec::new(),
            },
            work_revision,
            checks: Vec::new(),
            files: Vec::new(),
            blockers: Vec::new(),
            last_inspection: None,
            proposed_actions: Vec::new(),
            scope_denials: Vec::new(),
            children: Vec::new(),
            plan: Vec::new(),
            report: None,
            assessment: None,
            continuation_stop: None,
            acceptance: None,
            automatic_review: None,
            observed_edits: 0,
            unknown_effects: false,
            evidence_overflow: false,
        };
        task.add_explicit_requirements(&task.contract.original_request.clone());
        task
    }

    pub fn unverified(id: u64, request: String, reason: TurnEndReason, source: TaskSource) -> Self {
        let mut task = Self::new(
            id.max(1),
            if request.trim().is_empty() {
                "Earlier request (scope unavailable)".into()
            } else {
                request
            },
            0,
        );
        task.source = source;
        task.contract.mode = TaskMode::Unspecified;
        let mut assessment = task.assess(reason, true);
        assessment.outcome = TaskOutcome::Unknown;
        assessment.issues = vec![match source {
            TaskSource::External => "This agent runs its own loop. Native task checks and restrictions do not govern it.".into(),
            _ => "This saved turn has no independent task assessment.".into(),
        }];
        for requirement in &mut assessment.requirements {
            requirement.state = EvidenceState::Unknown;
            requirement.detail = "No independent evidence was recorded.".into();
        }
        task.assessment = Some(assessment);
        task
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != TASK_VERSION
            || self.contract.id == 0
            || self.contract.revision == 0
            || self.contract.objective.trim().is_empty()
            || self.contract.requirements.is_empty()
            || self.contract.requirements.len() > MAX_REQUIREMENTS
            || self.checks.len() > MAX_CHECKS
            || self.files.len() > MAX_FILES
            || self.blockers.len() > MAX_BLOCKERS
            || self.proposed_actions.len() > MAX_BLOCKERS
            || self.scope_denials.len() > MAX_CHECKS
            || self.children.len() > 32
            || self.plan.len() > 50
            || self.continuation_stop.as_ref().is_some_and(|stop| {
                stop.followups > crate::harness::MAX_CONTINUATIONS
                    || stop.condition.trim().is_empty()
                    || stop.condition.chars().count() > MAX_REPORT_CHARS
            })
        {
            return Err("The saved task record is invalid or unsupported.".into());
        }
        if self
            .proposed_actions
            .iter()
            .any(|action| action.trim().is_empty() || action.chars().count() > 1_000)
            || self.scope_denials.iter().any(|denial| {
                denial.contract_revision == 0
                    || denial.contract_revision > self.contract.revision
                    || !matches!(
                        denial.tool.as_str(),
                        "edit_file" | "write_file" | "run_command"
                    )
                    || denial.target.trim().is_empty()
                    || denial.target.chars().count() > MAX_REPORT_CHARS
            })
        {
            return Err("The saved task scope feedback is invalid.".into());
        }
        if self.last_inspection.as_ref().is_some_and(|inspection| {
            inspection.id == 0
                || inspection.contract_revision == 0
                || inspection.contract_revision > self.contract.revision
                || inspection.work_revision > self.work_revision
                || !matches!(
                    inspection.tool.as_str(),
                    "read_file" | "list_dir" | "grep" | "run_command"
                )
                || inspection.target.chars().count() > 600
        }) || self.blockers.iter().any(|blocker| {
            blocker.inspection_id
                > self
                    .last_inspection
                    .as_ref()
                    .map_or(0, |inspection| inspection.id)
        }) {
            return Err("The saved task inspection evidence is inconsistent.".into());
        }
        let mut ids = BTreeSet::new();
        if self.contract.requirements.iter().any(|requirement| {
            requirement.id == 0
                || requirement.spec.text().trim().is_empty()
                || !ids.insert(requirement.id)
        }) {
            return Err("The saved task requirements are invalid.".into());
        }
        if let Some(assessment) = &self.assessment
            && (assessment.task_id != self.contract.id
                || assessment.contract_revision != self.contract.revision
                || assessment.work_revision != self.work_revision
                || (assessment.outcome == TaskOutcome::Accepted) != self.is_accepted()
                || (assessment.outcome == TaskOutcome::Verified
                    && !self.is_automatically_verified())
                || assessment.requirements.len() != self.contract.requirements.len()
                || self.contract.requirements.iter().any(|requirement| {
                    assessment
                        .requirements
                        .iter()
                        .filter(|result| result.requirement_id == requirement.id)
                        .count()
                        != 1
                }))
        {
            return Err("The saved task assessment is inconsistent.".into());
        }
        if let Some(review) = &self.automatic_review {
            validate_automatic_review(review, self)?;
        }
        if self.acceptance.is_some() && (self.assessment.is_none() || !self.is_accepted()) {
            return Err("The saved task acceptance is inconsistent.".into());
        }
        let mut checks = BTreeSet::new();
        if self.checks.iter().any(|check| {
            check.id == 0
                || !checks.insert(check.id)
                || check.command.trim().is_empty()
                || check.started_revision > check.work_revision
                || check.work_revision > self.work_revision
                || check.contract_revision == 0
                || check.contract_revision > self.contract.revision
        }) {
            return Err("The saved task check evidence is inconsistent.".into());
        }
        Ok(())
    }

    pub fn edit(&mut self, edit: TaskEdit) -> Result<(), String> {
        if edit.task_id != self.contract.id || edit.expected_revision != self.contract.revision {
            return Err("The task changed. Reopen its details before revising it.".into());
        }
        if edit.objective.trim().is_empty()
            || edit.objective.chars().count() > 8_000
            || edit.requirements.is_empty()
            || edit.requirements.len() > MAX_REQUIREMENTS
            || edit
                .requirements
                .iter()
                .any(|spec| spec.text().trim().is_empty() || spec.text().chars().count() > 1_000)
            || edit.restrictions.len() > 32
            || edit
                .restrictions
                .iter()
                .any(|text| text.chars().count() > 1_000)
        {
            return Err("Use a goal, 1-32 requirements, and at most 32 short restrictions.".into());
        }
        let revision = self
            .contract
            .revision
            .checked_add(1)
            .ok_or("Task revision limit reached.")?;
        let mut next_id = self
            .contract
            .requirements
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0);
        let mut requirements = Vec::new();
        for spec in edit.requirements {
            if requirements.iter().any(|r: &Requirement| r.spec == spec) {
                return Err("Each requirement must be unique.".into());
            }
            let id = match self.contract.requirements.iter().find(|r| r.spec == spec) {
                Some(existing) => existing.id,
                None => {
                    next_id = next_id
                        .checked_add(1)
                        .ok_or("Requirement identifier limit reached.")?;
                    next_id
                }
            };
            requirements.push(Requirement { id, spec });
        }
        self.contract.revision = revision;
        self.contract.objective = edit.objective.trim().into();
        self.contract.mode = edit.mode;
        self.contract.requirements = requirements;
        self.contract.restrictions = edit.restrictions;
        self.automatic_review = None;
        self.acceptance = None;
        self.assessment = None;
        if edit.clear_reported_blockers {
            self.blockers.clear();
        }
        Ok(())
    }

    pub fn add_instruction(&mut self, text: String) {
        self.contract.revision = self.contract.revision.saturating_add(1);
        // Later restrictions can tighten policy, never silently loosen it.
        if requests_read_only(&text) {
            self.contract.mode = TaskMode::ReadOnly;
        } else if self.contract.mode != TaskMode::ReadOnly {
            match TaskMode::infer(&text) {
                TaskMode::Work => self.contract.mode = TaskMode::Work,
                TaskMode::ReadOnly if self.contract.mode != TaskMode::Work => {
                    self.contract.mode = TaskMode::ReadOnly;
                }
                _ => {}
            }
        }
        self.add_explicit_requirements(&text);
        self.contract.instructions.push(text);
        self.automatic_review = None;
        self.acceptance = None;
        self.assessment = None;
    }

    fn add_explicit_requirements(&mut self, text: &str) {
        // Ordinary prose stays in protected user context. Only labelled
        // obligations become mechanical criteria, including later steering.
        let mut next_id = self
            .contract
            .requirements
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0);
        for line in text.lines() {
            let Some((kind, text)) = line.split_once(':') else {
                continue;
            };
            let text = text.trim();
            let spec = match kind.trim().to_ascii_lowercase().as_str() {
                "required file" | "deliverable" => {
                    RequirementSpec::File(text.trim_matches('`').into())
                }
                "required check" => RequirementSpec::Command(text.trim_matches('`').into()),
                "acceptance" | "requirement" => RequirementSpec::Review(text.into()),
                _ => continue,
            };
            if self.contract.requirements.iter().any(|r| r.spec == spec) {
                continue;
            }
            if spec.text().is_empty() || self.contract.requirements.len() == MAX_REQUIREMENTS {
                self.evidence_overflow = true;
                continue;
            }
            let Some(id) = next_id.checked_add(1) else {
                self.evidence_overflow = true;
                continue;
            };
            next_id = id;
            self.contract.requirements.push(Requirement { id, spec });
        }
    }

    pub fn set_work_revision(&mut self, revision: u64) {
        if revision != self.work_revision {
            self.work_revision = revision;
            self.acceptance = None;
            self.assessment = None;
        }
    }

    /// Explain stale evidence without treating it as current verification.
    pub fn stale_verification_detail(&self) -> Option<String> {
        let passed = self
            .checks
            .iter()
            .filter(|check| {
                check.verification
                    && check.success
                    && check.contract_revision == self.contract.revision
                    && check.started_revision == check.work_revision
            })
            .max_by_key(|check| check.id)?;
        if passed.work_revision == self.work_revision {
            return None;
        }
        let mutation = self
            .checks
            .iter()
            .filter(|check| {
                check.id > passed.id
                    && check.work_revision > passed.work_revision
                    && crate::command_evidence::classify(&check.command).mutates
            })
            .min_by_key(|check| check.id);
        Some(match mutation {
            Some(check) => format!(
                "Earlier verification became stale after `{}`. Run dependency/setup operations before the final checks, then rerun the relevant checks and finish without further edits or installs.",
                head_tail(&check.command, 300)
            ),
            None => "Earlier verification became stale after subsequent work or file changes. Rerun the relevant checks after the final change, then finish without further edits or installs.".into(),
        })
    }

    pub fn is_accepted(&self) -> bool {
        self.acceptance.as_ref().is_some_and(|accepted| {
            accepted.contract_revision == self.contract.revision
                && accepted.work_revision == self.work_revision
        })
    }

    pub fn review(&mut self, review: &TaskReview) -> Result<(), String> {
        if review.task_id != self.contract.id
            || review.contract_revision != self.contract.revision
            || review.work_revision != self.work_revision
            || self.assessment.is_none()
        {
            return Err(
                "The task or its evidence changed. Review the current result first.".into(),
            );
        }
        self.acceptance = (review.decision == ReviewDecision::Accept).then_some(Acceptance {
            contract_revision: review.contract_revision,
            work_revision: review.work_revision,
        });
        if review.decision == ReviewDecision::NeedsWork {
            self.add_blocker("The user requested more work.", "user review");
        }
        let reason = self
            .assessment
            .as_ref()
            .expect("reviewable task")
            .reason
            .clone();
        self.assess(reason, true);
        Ok(())
    }

    pub fn add_blocker(&mut self, text: &str, source: &str) {
        let text = head_tail(text.trim(), 1_000);
        if text.is_empty() || self.blockers.iter().any(|blocker| blocker.text == text) {
            return;
        }
        if self.blockers.len() == MAX_BLOCKERS {
            self.evidence_overflow = true;
            return;
        }
        self.blockers.push(Blocker {
            text,
            source: source.into(),
            inspection_id: self
                .last_inspection
                .as_ref()
                .map_or(0, |inspection| inspection.id),
        });
        self.acceptance = None;
    }

    /// Inspection evidence can resolve an informational obstacle, never verify
    /// an edit or satisfy an explicit file/check requirement.
    pub(crate) fn informational(&self) -> bool {
        self.source == TaskSource::Native
            && matches!(
                self.contract.mode,
                TaskMode::Conversation | TaskMode::ReadOnly
            )
            && self.observed_edits == 0
            && self
                .contract
                .requirements
                .iter()
                .all(|requirement| matches!(requirement.spec, RequirementSpec::Review(_)))
    }

    pub(crate) fn observe_inspection(&mut self, name: &str, kind: ToolKind, args: &Value) {
        let inspection = matches!(name, "read_file" | "list_dir" | "grep")
            || (kind == ToolKind::Command
                && read_only_call_allowed(name, kind, args)
                && !may_mutate(name, kind, args)
                && !is_verification(args["command"].as_str().unwrap_or_default()));
        if !inspection {
            return;
        }
        let Some(id) = self
            .last_inspection
            .as_ref()
            .map_or(0, |inspection| inspection.id)
            .checked_add(1)
        else {
            self.evidence_overflow = true;
            return;
        };
        self.last_inspection = Some(InspectionEvidence {
            id,
            contract_revision: self.contract.revision,
            work_revision: self.work_revision,
            tool: name.into(),
            target: args["path"]
                .as_str()
                .or_else(|| args["command"].as_str())
                .unwrap_or(".")
                .chars()
                .take(600)
                .collect(),
        });
    }

    /// Reported progress is not a new contract and cannot certify a criterion.
    pub fn report(&mut self, args: &Value) -> Result<(), String> {
        let summary = args
            .get("summary")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .ok_or("A short `summary` is required.")?;
        let strings = |key: &str, cap: usize| -> Result<Vec<String>, String> {
            let values = args
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("`{key}` must be an array."))?;
            if values.len() > cap {
                return Err(format!("`{key}` has too many entries."));
            }
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|text| !text.trim().is_empty() && text.chars().count() <= 1_000)
                        .map(|text| text.trim().to_string())
                        .ok_or_else(|| format!("Every `{key}` entry must be short nonempty text."))
                })
                .collect()
        };
        let blockers = strings("blockers", MAX_BLOCKERS)?;
        let deliverables = strings("deliverables", MAX_FILES)?;
        let proposed_actions = if args.get("proposed_actions").is_some() {
            Some(strings("proposed_actions", MAX_BLOCKERS)?)
        } else {
            None
        };
        let resolved = if args.get("resolved_blockers").is_some() {
            strings("resolved_blockers", MAX_BLOCKERS)?
        } else {
            Vec::new()
        };
        if summary.chars().count() > MAX_REPORT_CHARS {
            return Err("Keep the summary below 4,000 characters.".into());
        }
        if !blockers.is_empty()
            && blockers
                .iter()
                .all(|text| self.optional_scope_blocker(text))
        {
            return Err(Self::analysis_guidance().into());
        }
        if !resolved.is_empty() {
            if resolved.iter().any(|text| {
                blockers.contains(text)
                    || !self
                        .blockers
                        .iter()
                        .any(|blocker| blocker.text == *text && blocker.source == "agent report")
            }) {
                return Err(
                    "Resolve only exact previously reported agent blockers. User-review blockers cannot be cleared by the agent.".into(),
                );
            }
            let inspected = self.informational()
                && self.last_inspection.as_ref().is_some_and(|inspection| {
                    inspection.contract_revision == self.contract.revision
                        && inspection.work_revision == self.work_revision
                        && self
                            .blockers
                            .iter()
                            .filter(|blocker| resolved.contains(&blocker.text))
                            .all(|blocker| inspection.id > blocker.inspection_id)
                });
            if !self.has_current_verification() && !inspected {
                return Err(
                    "Run a current passing check before resolving reported blockers. \
                     For a question or read-only analysis without required files/checks, \
                     a new successful inspection after the blocker was reported also qualifies."
                        .into(),
                );
            }
        }
        self.report = Some(summary.into());
        if let Some(actions) = proposed_actions {
            self.proposed_actions = actions;
        }
        self.blockers.retain(|blocker| {
            blocker.source != "agent report" || !resolved.contains(&blocker.text)
        });
        if !resolved.is_empty() {
            self.acceptance = None;
            self.assessment = None;
        }
        for blocker in blockers {
            self.add_blocker(&blocker, "agent report");
        }
        for path in deliverables {
            self.note_file(&path, true);
        }
        Ok(())
    }

    pub fn analysis_only(&self) -> bool {
        self.source == TaskSource::Native
            && self.contract.mode == TaskMode::ReadOnly
            && self.observed_edits == 0
            && !self.unknown_effects
            && self
                .contract
                .requirements
                .iter()
                .all(|requirement| matches!(requirement.spec, RequirementSpec::Review(_)))
    }

    pub(crate) fn analysis_guidance() -> &'static str {
        "This task asks for findings, not changes. Return the analysis in chat using \
         read_file, list_dir, grep or a simple inspection command. Do not update a report \
         file, run executable tests, or continue an earlier editing task. Those optional \
         actions are not blockers to this review. Put suggested follow-up work in \
         proposed_actions and ask whether the user wants it done. If information needed \
         for the analysis is genuinely inaccessible, report that specific obstacle. \
         Do not claim new verification, completed fixes, or user acceptance."
    }

    pub(crate) fn scope_guidance(&self) -> &'static str {
        if self.analysis_only() {
            Self::analysis_guidance()
        } else {
            "This scope permits inspection, not writes or executable tests. Do not continue \
             an earlier editing workflow. Return any findings possible within the current \
             scope. Explicit required files and checks still apply: if this policy prevents \
             a requested action, explain that specific blocker and ask for a scope revision. \
             Do not remove requirements or claim verification or acceptance."
        }
    }

    pub(crate) fn note_scope_denial(&mut self, tool: &str, args: &Value) {
        let target = match tool {
            "edit_file" | "write_file" => args["path"].as_str(),
            "run_command" => args["command"].as_str(),
            _ => None,
        };
        let Some(target) = target.filter(|text| !text.trim().is_empty()) else {
            return;
        };
        if target.chars().count() > MAX_REPORT_CHARS {
            return;
        }
        let denial = ScopeDenial {
            contract_revision: self.contract.revision,
            tool: tool.into(),
            target: target.into(),
        };
        if self.scope_denials.contains(&denial) {
            return;
        }
        if self.scope_denials.len() == MAX_CHECKS {
            self.evidence_overflow = true;
        } else {
            self.scope_denials.push(denial);
        }
    }

    /// A narrow correction for a host-observed, unrequested local action.
    /// Never infer this from permission wording alone, or discard real blockers.
    pub(crate) fn optional_scope_blocker(&self, text: &str) -> bool {
        if !self.analysis_only() {
            return false;
        }
        let lower = text.to_ascii_lowercase();
        if !["read-only", "read only", "analysis-only"]
            .iter()
            .any(|phrase| lower.contains(phrase))
        {
            return false;
        }
        let requests = std::iter::once(self.contract.original_request.as_str())
            .chain(std::iter::once(self.contract.objective.as_str()))
            .chain(self.contract.instructions.iter().map(String::as_str));
        let requests: Vec<_> = requests.collect();
        self.scope_denials.iter().any(|denial| {
            denial.contract_revision == self.contract.revision
                && (lower.contains(&denial.tool)
                    || lower.contains(&denial.target.to_ascii_lowercase()))
                && !requests.iter().any(|request| {
                    request
                        .to_ascii_lowercase()
                        .contains(&denial.target.to_ascii_lowercase())
                        || task_asks_for_changes(request)
                        || (denial.tool == "run_command"
                            && words(&request.to_ascii_lowercase()).any(|word| {
                                matches!(
                                    word,
                                    "test"
                                        | "tests"
                                        | "check"
                                        | "checks"
                                        | "verify"
                                        | "verification"
                                        | "typecheck"
                                )
                            }))
                })
                && (denial.tool != "run_command" || is_verification(&denial.target))
        })
    }

    pub fn note_file(&mut self, path: &str, reported: bool) {
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.reported |= reported;
        } else if self.files.len() < MAX_FILES {
            self.files.push(FileEvidence {
                path: path.into(),
                state: FileState::Unknown("Not inspected yet".into()),
                reported,
            });
        } else {
            self.evidence_overflow = true;
        }
    }

    /// Returns true when a previously fingerprinted file changed externally.
    /// Newly observed/missing files are not retroactive verification.
    pub fn refresh_files(&mut self, workspace: &Path) -> bool {
        self.refresh_files_cached(workspace, &mut FileScanCache::default())
    }

    /// [`Self::refresh_files`] with fingerprints carried between scans. A
    /// scan after every tool call must not re-read the whole evidence set
    /// when only metadata moved.
    pub(crate) fn refresh_files_cached(
        &mut self,
        workspace: &Path,
        cache: &mut FileScanCache,
    ) -> bool {
        let required: Vec<_> = self
            .contract
            .requirements
            .iter()
            .filter_map(|requirement| match &requirement.spec {
                RequirementSpec::File(path) => Some(path.clone()),
                _ => None,
            })
            .collect();
        for path in required {
            self.note_file(&path, false);
        }
        let mut changed = false;
        for file in &mut self.files {
            let state = file_state_cached(workspace, &file.path, cache);
            if !matches!(&file.state, FileState::Unknown(why) if why == "Not inspected yet")
                && file.state != state
            {
                changed = true;
            }
            file.state = state;
        }
        if changed {
            self.acceptance = None;
            self.assessment = None;
        }
        changed
    }

    pub(crate) fn observe_command(
        &mut self,
        call_id: &str,
        command: &str,
        timing: CommandTiming,
        outcome: &ToolOutcome,
        verification: bool,
    ) {
        if command.trim().is_empty() {
            // The failed tool event remains in history, but malformed
            // arguments have no command identity to use as check evidence.
            return;
        }
        self.set_work_revision(timing.finished);
        let evidence = CommandEvidence {
            id: timing.order,
            call_id: call_id.into(),
            command: command.trim().into(),
            contract_revision: self.contract.revision,
            work_revision: timing.finished,
            started_revision: timing.started,
            success: outcome.success && outcome.exit_code.is_none_or(|code| code == 0),
            exit_code: outcome.exit_code,
            detail: head_tail(&outcome.output, 600),
            verification,
        };
        self.store_check(evidence, false);
    }

    fn store_check(&mut self, evidence: CommandEvidence, newer_only: bool) {
        if let Some(existing) = self
            .checks
            .iter_mut()
            .find(|check| check.command == evidence.command)
        {
            if !newer_only || existing.id < evidence.id {
                *existing = evidence;
            }
        } else if self.checks.len() < MAX_CHECKS {
            self.checks.push(evidence);
        } else if let Some(index) = self.checks.iter().position(|check| {
            !check.verification
                && !self.contract.requirements.iter().any(|requirement| {
                    matches!(&requirement.spec, RequirementSpec::Command(required) if required.trim() == check.command)
                })
        }) {
            // Inspection history remains in the event log. Keep scarce task
            // evidence slots for checks rather than every exploratory command.
            self.checks.remove(index);
            self.checks.push(evidence);
        } else if evidence.verification {
            self.evidence_overflow = true;
        }
    }

    pub(crate) fn absorb_child(&mut self, child: &TaskRecord) {
        for check in &child.checks {
            let mut check = check.clone();
            check.contract_revision = self.contract.revision;
            self.store_check(check, true);
        }
        for file in &child.files {
            self.note_file(&file.path, file.reported);
        }
        self.unknown_effects |= child.unknown_effects;
        self.evidence_overflow |= child.evidence_overflow;
    }

    pub(crate) fn note_child(&mut self, session_id: &str, assessment: &TaskAssessment) {
        let child = ChildAssessment {
            session_id: session_id.into(),
            task_id: assessment.task_id,
            outcome: assessment.outcome,
            work_revision: assessment.work_revision,
            issues: assessment.issues.iter().take(16).cloned().collect(),
        };
        if let Some(existing) = self
            .children
            .iter_mut()
            .find(|child| child.session_id == session_id)
        {
            *existing = child;
        } else if self.children.len() < 32 {
            self.children.push(child);
        } else {
            self.evidence_overflow = true;
        }
    }

    pub(crate) fn note_final(&mut self, text: &str) {
        if text
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("blocked:")
            && !self.optional_scope_blocker(text.lines().next().unwrap_or(text))
        {
            self.add_blocker(text.lines().next().unwrap_or(text), "agent report");
        }
    }

    pub fn assess(&mut self, reason: TurnEndReason, answered: bool) -> TaskAssessment {
        let automatically_verified = self.is_automatically_verified();
        let requirements: Vec<_> = self
            .contract
            .requirements
            .iter()
            .map(|requirement| {
                let (state, detail, evidence_id) = match &requirement.spec {
                    RequirementSpec::Review(_) if automatically_verified => (
                        EvidenceState::Passed,
                        "A separate read-only review found no actionable issues at this revision. This is not user acceptance.".into(),
                        None,
                    ),
                    RequirementSpec::Review(_) => (
                        EvidenceState::NeedsReview,
                        "Content and scope need your review.".into(),
                        None,
                    ),
                    RequirementSpec::File(path) => match self
                        .files
                        .iter()
                        .find(|file| file.path == *path)
                        .map(|file| &file.state)
                    {
                        Some(FileState::Present { bytes, .. }) if *bytes > 0 => (
                            EvidenceState::Observed,
                            format!(
                                "Nonempty file observed ({bytes} bytes). Content is not certified."
                            ),
                            None,
                        ),
                        Some(FileState::Present { .. }) => {
                            (EvidenceState::Missing, "The file is empty.".into(), None)
                        }
                        Some(FileState::Unknown(why)) => {
                            (EvidenceState::Unknown, why.clone(), None)
                        }
                        _ => (
                            EvidenceState::Missing,
                            "Required file not found.".into(),
                            None,
                        ),
                    },
                    RequirementSpec::Command(command) => match self
                        .checks
                        .iter()
                        .find(|check| check.command == command.trim())
                    {
                        Some(check) => (
                            check.state(self.contract.revision, self.work_revision),
                            check.detail.clone(),
                            Some(check.id),
                        ),
                        None => (
                            EvidenceState::Missing,
                            "This exact check has not completed.".into(),
                            None,
                        ),
                    },
                };
                RequirementAssessment {
                    requirement_id: requirement.id,
                    state,
                    detail,
                    evidence_id,
                }
            })
            .collect();
        let mut issues = Vec::new();
        if !matches!(reason, TurnEndReason::Completed) {
            issues.push(match &reason {
                TurnEndReason::Interrupted => "Execution was interrupted.".into(),
                TurnEndReason::StepLimit => "The execution step budget was exhausted.".into(),
                TurnEndReason::ContinuationLimit => self.continuation_stop.as_ref()
                    .map(ContinuationStop::detail)
                    .unwrap_or_else(|| "Automatic follow-ups stopped with unresolved finish conditions. Review the remaining issues below.".into()),
                TurnEndReason::Failed(why) => head_tail(why, 1_000),
                TurnEndReason::Completed => unreachable!(),
            });
        }
        for requirement in &requirements {
            if matches!(
                requirement.state,
                EvidenceState::Missing
                    | EvidenceState::Failed
                    | EvidenceState::Stale
                    | EvidenceState::Unknown
            ) {
                let spec = self
                    .contract
                    .requirements
                    .iter()
                    .find(|r| r.id == requirement.requirement_id)
                    .expect("owned requirement");
                issues.push(format!(
                    "{}: {} ({})",
                    spec.spec.kind_label(),
                    spec.spec.text(),
                    requirement.state.label()
                ));
            }
        }
        for check in &self.checks {
            if self.failed_verification_is_unresolved(check)
                && !issues.iter().any(|issue| issue.contains(&check.command))
            {
                issues.push(format!("Command failed: {}", check.command));
            }
        }
        if self.observed_edits > 0 && !self.has_current_verification() {
            issues.push("No current verification was observed after the file changes.".into());
            if let Some(detail) = self.stale_verification_detail() {
                issues.push(detail);
            }
            if let Some(hint) = self
                .checks
                .iter()
                .filter(|check| !check.verification)
                .max_by_key(|check| check.id)
                .and_then(|check| crate::command_evidence::verification_rejection(&check.command))
            {
                issues.push(hint.into());
            }
        }
        if self.contract.mode == TaskMode::Work
            && self.observed_edits == 0
            && self.files.is_empty()
            && self.checks.is_empty()
            && self.children.is_empty()
        {
            issues.push(
                "No deliverable or completed check was observed for this work request.".into(),
            );
        }
        if self.plan.iter().any(|step| step.status.is_open()) {
            issues.push("The agent's plan still has open steps.".into());
        }
        for child in &self.children {
            // NeedsReview is a usable child report, not a failed dependency.
            // The parent still needs its own current verification and review.
            if !child.outcome.is_usable_child_report() {
                issues.push(format!(
                    "Child {}: {}",
                    child.session_id,
                    child.outcome.label()
                ));
            }
        }
        if self.evidence_overflow {
            issues.push("The observation limit was reached; some evidence is unknown.".into());
        }
        if let Some(review) = &self.automatic_review
            && review.verdict != AutomaticReviewVerdict::Clean
        {
            issues.extend(
                review
                    .findings
                    .iter()
                    .map(|finding| format!("Automatic review: {finding}")),
            );
        }
        let outcome = if self.is_accepted() {
            TaskOutcome::Accepted
        } else if !self.blockers.is_empty()
            || self
                .automatic_review
                .as_ref()
                .is_some_and(|review| review.verdict == AutomaticReviewVerdict::Blocked)
        {
            TaskOutcome::Blocked
        } else if !issues.is_empty() {
            TaskOutcome::Partial
        } else if automatically_verified {
            TaskOutcome::Verified
        } else if self.contract.mode == TaskMode::Conversation
            && answered
            && self.observed_edits == 0
            && !self.unknown_effects
            && self.contract.requirements.len() == 1
        {
            TaskOutcome::Answered
        } else if !answered && self.checks.is_empty() && self.files.is_empty() {
            TaskOutcome::Unknown
        } else {
            TaskOutcome::NeedsReview
        };
        let assessment = TaskAssessment {
            task_id: self.contract.id,
            contract_revision: self.contract.revision,
            work_revision: self.work_revision,
            outcome,
            reason,
            requirements,
            issues,
        };
        self.assessment = Some(assessment.clone());
        assessment
    }

    /// Protected, authoritative scope. Tool reports and editable plans are
    /// deliberately separate. This message is not compacted or restored from
    /// model history.
    pub fn context(&self) -> String {
        let context = format!(
            "[Host task contract]\n{}\n\
             Keep the original goal and restrictions. Plans and report_task cannot remove \
             requirements or certify completion. Report real blockers and deliverable paths \
             with report_task before concluding substantial work. After fixing an agent-reported \
             blocker and passing a current check (or a new relevant inspection for an \
             informational task), resolve its exact text with resolved_blockers. \
             Omitting it does not resolve it; user-review blockers remain protected. \
             Never treat a normal stop, \
             a file's existence, or an unrelated passing command as accepted work.\n\
             Observed work revision: {}. Check results: {}\nReported blockers: {}\n\
             Last successful inspection: {}",
            serde_json::to_string(&self.contract).expect("task contract serializes"),
            self.work_revision,
            serde_json::to_string(&self.checks).expect("checks serialize"),
            serde_json::to_string(&self.blockers).expect("blockers serialize"),
            serde_json::to_string(&self.last_inspection).expect("inspection serializes"),
        );
        let context = if let Some(review) = &self.automatic_review {
            format!(
                "{context}\n[Independent automatic review feedback]\n{}\n\
                 Fix these actionable findings within the protected task's scope. Only a new \
                 independent review can clear this feedback; do not claim user acceptance.",
                serde_json::to_string(review).expect("review serializes"),
            )
        } else {
            context
        };
        if self.contract.mode == TaskMode::ReadOnly {
            format!(
                "{context}\n[Current review scope]\n{}\n\
                 The current contract overrides earlier conversation history. Previous edits, \
                 checks and report files do not create new requirements. Existing reports may \
                 be read, but not updated. A mode change is only needed for requested execution \
                 or writes, not to finish an ordinary review. \
                 Proposed actions (not performed or approved): {}",
                self.scope_guidance(),
                serde_json::to_string(&self.proposed_actions).expect("actions serialize"),
            )
        } else {
            context
        }
    }
}

pub(crate) fn is_continue_request(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches(['.', '!'])
            .to_ascii_lowercase()
            .as_str(),
        "continue" | "please continue" | "keep going" | "go on" | "resume" | "retry" | "finish it"
    )
}

/// Shared by the parent and children. A result spanning another child's edit
/// is stale even when the child returns its report later.
#[derive(Debug, Default)]
pub(crate) struct WorkClock {
    revision: AtomicU64,
    order: AtomicU64,
    read_only: AtomicBool,
    pending_read_only: AtomicU64,
}

pub(crate) struct CommandTiming {
    pub started: u64,
    pub finished: u64,
    pub order: u64,
}

impl WorkClock {
    pub fn read_only(&self) -> bool {
        self.read_only.load(Ordering::SeqCst) || self.pending_read_only.load(Ordering::SeqCst) > 0
    }

    pub fn set_read_only(&self, read_only: bool) {
        self.read_only.store(read_only, Ordering::SeqCst);
    }
    /// Inbox restrictions apply immediately, even behind a deferred control.
    /// Starting another scope cannot clear them before they are consumed.
    pub fn queue_restriction(&self, request: &str) {
        if requests_read_only(request) {
            self.pending_read_only.fetch_add(1, Ordering::SeqCst);
        }
    }
    pub fn consume_restriction(&self, request: &str) {
        if requests_read_only(request) {
            self.set_read_only(true);
            self.pending_read_only
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    Some(count.saturating_sub(1))
                })
                .ok();
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    pub fn advance(&self) -> u64 {
        self.revision
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1)
    }

    pub fn restore(&self, revision: u64, order: u64) {
        self.revision.fetch_max(revision, Ordering::SeqCst);
        self.order.fetch_max(order, Ordering::SeqCst);
    }

    pub fn next_observation(&self) -> u64 {
        self.order.fetch_add(1, Ordering::SeqCst).saturating_add(1)
    }
}

/// Conservative known checks for evidence labeling only, not a security rule.
pub(crate) fn is_verification(command: &str) -> bool {
    crate::command_evidence::classify(command).verification
}

pub(crate) fn may_mutate(name: &str, kind: ToolKind, args: &Value) -> bool {
    match kind {
        ToolKind::Edit => true,
        ToolKind::Command => {
            let command = args["command"].as_str().unwrap_or_default();
            crate::command_evidence::classify(command).mutates
        }
        _ => name.starts_with(crate::mcp::PREFIX),
    }
}

/// A small allowlist for read-only mode. Complex shell syntax, interpreters,
/// output redirection and MCP are refused rather than guessed safe.
pub(crate) fn read_only_call_allowed(name: &str, kind: ToolKind, args: &Value) -> bool {
    if name.starts_with(crate::mcp::PREFIX) || kind == ToolKind::Edit {
        return false;
    }
    if kind != ToolKind::Command {
        return true;
    }
    let command = args["command"].as_str().unwrap_or_default();
    if command.contains(['>', '<', '$', '`', ';', '|', '&', '\n', '(', ')', '\\']) {
        return false;
    }
    if cfg!(windows) && command.contains(['%', '!', '^', '\r']) {
        // cmd expands environment variables even inside double quotes, and
        // caret/delayed expansion has no equivalent in the POSIX parser.
        return false;
    }
    let Some(decoded) = shlex::split(command) else {
        return false;
    };
    let words: Vec<_> = decoded.iter().map(String::as_str).collect();
    let Some(program) = words.first() else {
        return false;
    };
    if words.iter().any(|word| {
        matches!(*word, "-c" | "--pre" | "--hostname-bin")
            || word.starts_with("--output")
            || word.starts_with("--pre=")
            || word.starts_with("--hostname-bin=")
    }) {
        return false;
    }
    if *program == "git"
        && words.iter().skip(1).any(|word| {
            word.starts_with("-c")
                || word.starts_with("--config-env")
                || word.starts_with("--exec-path")
                || word.starts_with("--ext-diff")
                || word.starts_with("--textconv")
                || word.starts_with("--open-files-in-pager")
        })
    {
        return false;
    }
    if *program == "file"
        && words.iter().skip(1).any(|word| {
            *word == "--compile"
                || (word.starts_with('-') && !word.starts_with("--") && word.contains('C'))
        })
    {
        return false;
    }
    match *program {
        "pwd" | "ls" | "cat" | "head" | "tail" | "wc" | "stat" | "file" | "rg" | "grep" => true,
        "git" => git_subcommand(&words[1..])
            .is_some_and(|sub| matches!(sub, "status" | "diff" | "log" | "show" | "ls-files")),
        _ => false,
    }
}

pub(crate) fn validate_automatic_review(
    review: &AutomaticReview,
    task: &TaskRecord,
) -> Result<(), String> {
    if task.source != TaskSource::Native
        || review.contract_revision == 0
        || review.contract_revision > task.contract.revision
        || review.work_revision > task.work_revision
        || review.model.trim().is_empty()
        || review.model.len() > 512
        || review.summary.trim().is_empty()
        || review.summary.chars().count() > MAX_REPORT_CHARS
        || review.findings.len() > MAX_BLOCKERS
        || review
            .findings
            .iter()
            .any(|finding| finding.trim().is_empty() || finding.chars().count() > 1_000)
        || review.inspected_files.len() > MAX_FILES
        || review.inspected_files.iter().any(|file| {
            file.path.trim().is_empty()
                || !matches!(file.state, FileState::Present { bytes, .. } if bytes > 0)
        })
        || review.reviewed_requirements.len() > MAX_REQUIREMENTS
        || (review.verdict == AutomaticReviewVerdict::Clean) != review.findings.is_empty()
        || (review.verdict == AutomaticReviewVerdict::Clean && review.inspected_files.is_empty())
    {
        return Err("Independent review evidence is invalid or incomplete.".into());
    }
    if review.verdict == AutomaticReviewVerdict::Clean
        && review.contract_revision == task.contract.revision
        && !task
            .contract
            .requirements
            .iter()
            .all(|requirement| review.reviewed_requirements.contains(&requirement.id))
    {
        return Err("Independent review did not cover every protected requirement.".into());
    }
    let inspected_paths = review
        .inspected_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    let reviewed_ids = review
        .reviewed_requirements
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if inspected_paths.len() != review.inspected_files.len()
        || reviewed_ids.len() != review.reviewed_requirements.len()
        || (review.contract_revision == task.contract.revision
            && reviewed_ids.iter().any(|id| {
                !task
                    .contract
                    .requirements
                    .iter()
                    .any(|requirement| requirement.id == *id)
            }))
    {
        return Err("Independent review contains duplicate or unknown evidence.".into());
    }
    if review.verdict == AutomaticReviewVerdict::Clean
        && review.contract_revision == task.contract.revision
        && review.work_revision == task.work_revision
        && task.files.iter().any(|file| {
            matches!(file.state, FileState::Present { bytes, .. } if bytes > 0)
                && !inspected_paths.contains(file.path.as_str())
        })
    {
        return Err("Independent review must inspect every observed task file.".into());
    }
    Ok(())
}

pub(crate) fn file_state(workspace: &Path, path: &str) -> FileState {
    file_state_cached(workspace, path, &mut FileScanCache::default())
}

/// Fingerprints already computed for files whose metadata has not moved since.
///
/// Deliberately not part of [`FileState`]: evidence stays the durable truth, so
/// a reuse decision can never change what a saved session claims to have seen.
#[derive(Debug, Default, Clone)]
pub(crate) struct FileScanCache {
    entries: std::collections::BTreeMap<std::path::PathBuf, Fingerprint>,
}

#[derive(Debug, Clone, Copy)]
struct Fingerprint {
    bytes: u64,
    fingerprint: u64,
    modified: Option<std::time::SystemTime>,
}

/// A file modified this recently is hashed even when its metadata matches,
/// because coarse filesystem timestamps can hide a same-size rewrite inside
/// the same tick.
const FINGERPRINT_MTIME_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// The cache holds short-lived metadata, so it is kept far below the evidence
/// cap rather than sized by it.
const FINGERPRINT_CACHE_MAX: usize = MAX_FILES * 4;

impl FileScanCache {
    fn reuse(&self, path: &Path, metadata: &std::fs::Metadata) -> Option<FileState> {
        let cached = self.entries.get(path)?;
        if cached.bytes != metadata.len() {
            return None;
        }
        let modified = metadata.modified().ok()?;
        if cached.modified != Some(modified) {
            return None;
        }
        if modified
            .elapsed()
            .map_or(true, |settled| settled < FINGERPRINT_MTIME_GRACE)
        {
            return None;
        }
        Some(FileState::Present {
            bytes: cached.bytes,
            fingerprint: cached.fingerprint,
        })
    }

    fn remember(
        &mut self,
        path: std::path::PathBuf,
        bytes: u64,
        fingerprint: u64,
        modified: Option<std::time::SystemTime>,
    ) {
        if self.entries.len() >= FINGERPRINT_CACHE_MAX && !self.entries.contains_key(&path) {
            self.entries.clear();
        }
        self.entries.insert(
            path,
            Fingerprint {
                bytes,
                fingerprint,
                modified,
            },
        );
    }

    fn forget(&mut self, path: &Path) {
        self.entries.remove(path);
    }
}

pub(crate) fn file_state_cached(
    workspace: &Path,
    path: &str,
    cache: &mut FileScanCache,
) -> FileState {
    let path = match crate::tools::resolve(workspace, path) {
        Ok(path) => path,
        Err(_) => {
            return FileState::Unknown(
                "Path is outside the workspace or cannot be resolved.".into(),
            );
        }
    };
    let mut file = match crate::file_io::open_regular_file(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            cache.forget(&path);
            return FileState::Missing;
        }
        Err(_) => return FileState::Unknown("The file could not be read.".into()),
    };
    let metadata = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return FileState::Unknown("Not a readable regular file.".into()),
    };
    if metadata.len() > MAX_FILE_BYTES {
        cache.forget(&path);
        return FileState::Unknown("File exceeds the 8 MB fingerprint limit.".into());
    }
    if let Some(state) = cache.reuse(&path, &metadata) {
        return state;
    }
    let modified = metadata.modified().ok();
    let fingerprint = match fingerprint_file(&mut file) {
        Ok(fingerprint) => fingerprint,
        Err(state) => {
            cache.forget(&path);
            return state;
        }
    };
    let (bytes, hash) = fingerprint;
    if bytes != metadata.len() {
        cache.forget(&path);
        return FileState::Unknown("The file changed while it was being inspected.".into());
    }
    cache.remember(path, bytes, hash, modified);
    FileState::Present {
        bytes,
        fingerprint: hash,
    }
}

fn fingerprint_file(file: &mut std::fs::File) -> Result<(u64, u64), FileState> {
    // Versioned FNV-1a fingerprint for drift detection, not a security digest.
    let mut hash = 0xcbf29ce484222325_u64;
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => {
                return Err(FileState::Unknown(
                    "The file changed or could not be read.".into(),
                ));
            }
        };
        bytes += count as u64;
        if bytes > MAX_FILE_BYTES {
            return Err(FileState::Unknown(
                "File exceeds the 8 MB fingerprint limit.".into(),
            ));
        }
        for byte in &buffer[..count] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok((bytes, hash))
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
