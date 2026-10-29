# Bounded native improvement reminders

This lane is **data-only, task-scoped prompt reminders**, not self-modifying code.
Executable/source/binary candidate activation remains **held**. There is no
generated-source execution, hot swap, tool, permission, approval, review-criteria,
or budget mutation API.

## Native integration

The native session owns the store through `adaptive_session.rs`. Desktop
sessions enable the capability; other engine clients default to disabled
(`ExecutionOptions::improvement_hints`). In-memory sessions without protected
history do not create a registry.

The latest active parent task's **Adaptive hints** menu shows confirmed status
and exposes **Learning on**, **Learning paused**, and **Roll back all learned
hints**. Controls use the native `SetSessionOption` route and queue behind a
running turn. Models and GenUI actions cannot change them. The option description
shows the registry revision and candidate count. An unavailable registry is
reported without blocking baseline task execution.

The stored session identity is the SHA-256 of the host-owned session directory
path. Moving/copying a registry to another directory fails closed rather than
silently applying its evidence elsewhere.

The integration calls `read_boundary` after task establishment and before prompt
construction. It observes only completed normal work turns, never independent
review. Cancelled boundaries do not activate candidates. Injected fixed reminders
are suppressed if scope changes mid-turn, the task becomes read-only, or a child
inherits the context.

Use one `ImprovementStore` per native session, owned by the native turn loop:

```rust,ignore
use flint_agent::improvement::{EvidenceSource, ImprovementStore};

// A host-owned private child, NOT the live history directory itself.
let mut improvements =
    ImprovementStore::open(&session_dir.join("improvement"), &session_uid)?;

// After the accepted input has established the task/contract and allocated its
// persisted native turn ID, but BEFORE constructing that turn's prompt:
let hints = improvements.read_boundary(turn_id, task.as_ref())?;
for fixed_text in hints.prompt_texts() {
    // Add this static reminder to the turn's host-owned context.
    // Never replace user instructions or immutable policy/gates with it.
}

// After normal native execution/assessment is complete, not during review:
if let Some(task) = task.as_ref() {
    let source = EvidenceSource {
        session_uid: session_uid.clone(),
        turn_id,
        task_id: task.contract.id,
        contract_revision: task.contract.revision,
        work_revision: task.work_revision,
    };
    improvements.observe_completed_task(&source, task)?;
}
```

The host, not a model, supplies the UID, turn ID, task, path and controls. UIDs
must be 1–128 ASCII letters/digits/`-`/`_`. Turn IDs are positive, persisted and
monotonically increasing across restarts. Open only after native history has
established the session's identity. Retain the writer for the native session's
lifetime. Do not share this child with another session, B1 or classic history.

Every error means **omit reminders and continue baseline behavior**; do not use
cached reminders, recreate/overwrite a corrupt registry, or retry publication
on a poisoned handle. Reopen only through the normal host lifecycle after
diagnosis. Optional improvement storage must never prevent the underlying
native task, including Stop, from running.

`read_boundary` is idempotent for the same turn and task/contract. Observation
only stages `Pending` candidates. Activation requires a strictly later boundary
for the **same task ID and contract revision**, even after restart. Calling the
same boundary after observation or reenabling cannot activate a candidate.
Other tasks, revised contracts, read-only work and absent tasks receive no hints.
Already-copied turn context cannot be revoked mid-turn; explicit controls affect
subsequent context reads, never mutate an in-flight prompt.

## Fixed whitelist and evidence

All emitted text is returned by `HintKind::prompt_text()`:

* `DirectVerificationReminder`: observed edits, no direct verification evidence.
* `FreshVerificationReminder`: observed edits and a successful, exit-zero
  verification at the same contract revision but an earlier work revision, with
  no current exit-zero verification.
* `CloseOpenPlanReminder`: a completed native assessment still has open plan
  steps. The fixed text explicitly forbids marking unresolved work complete.

Input must validate as a native `Work` `TaskRecord`, matching the current host
boundary and source receipt exactly. Only `Completed` assessments with outcome
`Partial` or `NeedsReview` qualify. Active/unassessed, interrupted, failed,
limited, accepted, verified, blocked, external and legacy records do not qualify.
The conservative learner additionally excludes any failed command, blocker,
scope denial, child result, unknown effect, overflow, continuation stop,
acceptance or automatic-review receipt. Automatic-review-only issue strings do
not create hints. No issue/report/plan/command prose is used as a rule or copied
into prompts.

One candidate per `(task ID, contract revision, hint kind)` is allowed for the
registry's entire lifetime, including after rollback. Repeated observations do
not create candidate versions, evidence entries or events. Contradictory task
bytes for an already recorded exact source receipt are rejected.

## Inspection and rollback

`inspect()` returns a detached typed snapshot. The human-readable
`improvement/registry.json` contains:

* Schema version, session UID, monotonic registry revision and current boundary.
* Typed evidence: exact source receipt, original TaskRecord compact-JSON SHA-256,
  observed-edit count, check IDs/revisions/success/exit/verification flags,
  command SHA-256s and total/open plan counts.
* Candidates linked to evidence, status (`Pending`, `Active`, `RolledBack`),
  activation turn/revision and rollback revision.
* A bounded append-only logical audit of observation, activation, enable/disable
  and rollback. Replay must agree with saved candidate statuses and enablement.

The original task remains in native history, not duplicated in this registry.
Digests bind the original task serialization and command bytes without copying
their potentially sensitive or adversarial text. This is host-observed execution
evidence, **not** a semantic correctness certificate.

`set_enabled(false)` durably disables both learning and hint output, preserving
the audit. `set_enabled(true)` does not activate pending candidates until a later
boundary. `rollback()` disables learning/output and permanently retires all
existing candidates while preserving evidence and audit. Repeated rollback is
idempotent. Reenabling cannot resurrect a retired candidate. No API accepts a
caller-supplied candidate, prompt string, source file, command or policy patch.

## Storage/security limits

* At most **256 KiB** saved registry, **32 candidates**, **64 evidence records**,
  **128 audit events**; each evidence record has at most **64 checks**, **50 plan
  steps**, and a streamed original-task serialization capped at **1 MiB**.
* Ordinary writes reserve **8 KiB** and one final audit slot for terminal
  rollback. History is not pruned or silently evicted. At capacity, new learning
  or controls fail with `Limit`; the final rollback remains available. Numeric
  exhaustion also fails rather than wrapping; disk failures cannot guarantee a
  durable rollback and require baseline behavior.
* Unknown fields, unknown kinds/actions, duplicate JSON struct fields,
  unsupported versions, malformed digests, dangling evidence, duplicate
  candidates, impossible revisions/statuses and inconsistent audit are rejected.
* Reuses `orchestration::fs::{WriterLease, Directory}` for descriptor-relative,
  no-follow regular-file access, bounded reads, atomic replacement, private
  temporary files, file/directory sync and a nonblocking exclusive OS lease.
* On Unix, directories must be owner-private, files owner-private and
  single-link; broad existing permissions are rejected, not silently changed.
  New directories/files use 0700/0600 creation modes. A separate child lease
  cannot contend with the native history writer.
* Every API checks the saved bytes against its loaded snapshot. Out-of-band
  changes stop use without overwriting them. Publication failure poisons the
  handle, including a failure after atomic rename.

Threat boundary: parent directories and the OS account are trusted. Descriptor
anchoring prevents child symlink traversal, not a hostile same-UID process from
replacing ancestors/lock names or rewriting an entirely self-consistent registry
while this process is stopped. Digests are not signatures, and there is no
external anti-rollback counter against offline replacement of an older valid
file. Registry audit records are immutable through the API, not filesystem-WORM.
Normal storage uses two files (snapshot and lease); a hard crash can leave a
private temporary from the shared atomic-write helper. No automatic orphan
deletion is attempted.

This implementation is validated on macOS/Unix and **fails closed on non-Unix**
until equivalent existing-owner/ACL/link checks are validated. It performs no
cross-session learning, automatic hint expiry, model-based rule synthesis,
benchmarking, live inference, or executable activation. Desktop controls are
provided by the host, not by the learner or renderer.

## Verification

Focused checks:

```sh
cargo test -p flint-agent improvement --locked --offline
cargo clippy -p flint-agent --all-targets --locked --offline -- -D warnings
rustfmt --edition 2024 --check crates/flint-agent/src/improvement.rs crates/flint-agent/src/improvement_tests.rs
```

Tests cover fixed-text/no-text-injection behavior, conservative evidence
selection, source/task mismatch, next-boundary ordering, idempotent observation,
restart/rollback/disable persistence, strict JSON/provenance validation, limits,
private permissions, links, exclusive writers, byte drift and atomic-publication
failures before/after rename. No shared-source registration is required to run
an isolated copied-workspace verification.
