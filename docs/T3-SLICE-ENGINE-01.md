# T3 engine slice 01: durable acceptance and recovery

Status: approved for implementation. Gates are frozen before results.

## Ownership and authority

Backend Factory worker owns `crates/flint-agent/**`, with ACP changes only if
needed to prevent destructive v3 compatibility behavior. Frontend owns
`crates/flint-app/**`; do not edit those files or documentation.

Preserve all starting dirty/untracked source and existing task scope/review,
WorkClock, filesystem guards and approvals. No real provider/adapter calls,
native-app launches, commits, pushes or user-binary changes.

Read `T3-IMPLEMENTATION.md` and `T3-CONTRACTS.md`. The eleven backend PHASE 0
disagreements are accepted, except preview fidelity is modified to the user's
chosen full isolated rendering bridge. Later capabilities remain unsupported.

## Bounded implementation

Implement B1 only:

1. Validated session/conversation/command identities, prompt snapshots,
   versioned canonical digest and immutable accepted command envelopes.
2. Additive ledger/store/client surface with acceptance, receipt lookup, bounded
   receipt/queue queries and paused/recovery-held projection. No provider
   execution or scheduling is enabled yet. Do not expose unfinished capability
   booleans as Supported.
3. One canonical history-v3 checkpoint containing unchanged legacy history/task
   plus orchestration state. Referenced command/receipt objects are immutable
   and committed before the canonical reference. Versions 1/2 remain readable.
   Keep original damaged/unknown histories in place, with visible errors.
4. OS-backed exclusive writer lease, private files, unique temporary files,
   no-follow regular-file validation, file sync, rename and directory sync.
   A PID file is not a lease. Do not overwrite/delete unrelated temporary files.
5. Commit acceptance before resolving a receipt. Identical retried identity
   returns the retained receipt; conflicting conversation/payload fails without
   changing it. Caller cancellation cannot orphan accepted work.
6. On restart, increment generation and hold accepted work. Started or uncertain
   work is never automatically redispatched. No execution in this slice.
7. Native stopped-task read/edit/review helpers preserve every orchestration
   field and share the writer exclusion. Legacy loading/writing must never move
   v3 aside as damaged, drop ledger fields, or overwrite it with v2.
8. Retain durable digest/identity tombstones for the session lifetime. Active
   memory and query pages are bounded. Do not load every historical payload or
   every receipt into an unbounded in-memory map on reopen.

The public legacy `Op`, `AgentEvent`, `AgentConfig` and `SessionHandle` shapes
remain unchanged. Planned later types may be declared for compatibility, but
unfinished methods fail visibly and do not create provider/workspace effects.
No shell/MCP/ACP secret plaintext path or competing child executor.

## Dependency and build ownership

Approved additions only:

- `sha2 = "=0.10.9"`
- `uuid = { version = "=1.26.1", features = ["v4"] }`

Backend may edit its manifest and only the corresponding root lock entries.
Do offline dependency resolution in the same run-owned validation slot as
tests. Do not update other pinned versions. If `--locked` rejects the deliberate
manifest addition, retain that failure, resolve once offline under the slot,
review the narrow lock diff, then use `--locked` thereafter.

Runner:
`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/specs/validate.py`.
Do not start separate Cargo/build/native processes. Format owned files only
while the frontend worker is active.

## Frozen acceptance gates

1. Identical retry returns same identity, accepted sequence and frozen digest;
   conflicting payload/conversation cannot alter acceptance.
2. Receipt is never emitted on failed precommit/object/snapshot persistence.
   Fault injection covers before/after rename and lost-reply recovery.
3. Cancellation of the caller after intake is reconciled by same-ID retry.
4. Simultaneous writer and stopped review/edit race fail visibly rather than
   clobbering canonical state.
5. Versions 1/2/3 and unknown/corrupt/symlink inputs are tested without deleting
   or relocating user history. Stopped review/edit round-trips queue/receipts.
6. Reopen holds queued/started work and never calls a provider or side effect.
   Started state is not silently downgraded to runnable Accepted.
7. Active queue/count/bytes and receipt/history pages enforce their limits.
   Many historical receipts do not require loading all captured payloads.
8. Existing agent/persistence/task and affected ACP regressions pass, plus new
   focused transaction/identity/recovery tests.
9. Formatting and focused checks pass, with raw failures retained. The builder
   returns facts, not an acceptance verdict.

## Reporting and next boundary

Write raw commands/exits/counts/artifact paths and concrete limitations to
run-root `specs/backend-b1-results.md`. Report exact implemented public
signatures so the frontend can integrate without guessing.

B2 scheduler/native intent integration requires another frozen slice and
independent architect validation of B1. A storage helper with no later real
engine/app entry point is foundation, not completed orchestration.
